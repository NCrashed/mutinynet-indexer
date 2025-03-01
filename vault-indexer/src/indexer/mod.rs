use crate::db::vault::rune::DatabaseRune;
use bitcoin::{
    block::Header,
    p2p::{message::NetworkMessage, message_blockdata::Inventory},
    Block, BlockHash, Transaction,
};
use bus::{Bus, BusReader};
use core::{
    result::Result,
    sync::atomic::{self, AtomicBool, AtomicU32},
    time::Duration,
};
use event::{Event, NewUnitTx, EVENTS_CAPACITY};
use log::*;
pub use network::Network;
use rusqlite::Connection;
use std::{
    path::{Path, PathBuf},
    sync::{
        mpmc::{self, Sender},
        mpsc::SendError,
        Arc, Mutex,
    },
};
use std::{sync::mpmc::sync_channel, thread};
use thiserror::Error;

use node::{node_worker, MAX_HEADERS_PER_MSG};

use crate::{
    cache::headers::HeadersCache,
    db::{self, initialize_db, metadata::DatabaseMeta, vault::DatabaseVault},
    vault::{UnitTransaction, VaultTx},
};

pub mod event;
pub mod network;
mod node;

#[derive(Error, Debug)]
#[error(transparent)]
pub struct Error(Box<ErrorKind>);

impl<E> From<E> for Error
where
    ErrorKind: From<E>,
{
    fn from(err: E) -> Self {
        Error(Box::new(ErrorKind::from(err)))
    }
}

/// All kind of errors the indexer can produce
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ErrorKind {
    #[error("Failed to read from events bus, disconnected.")]
    EventBusRecv,
    #[error("Failed to send event to bus: {0}")]
    EventBusSend(#[from] SendError<Event>),
    #[error("Node worker failure: {0}")]
    Node(#[from] node::Error),
    #[error("Database failure: {0}")]
    Database(#[from] db::Error),
    #[error("Cache error: {0}")]
    Cache(#[from] crate::cache::Error),
    #[error("Failed to lock on headers cache, poisoned")]
    HeadersCacheLock,
    #[error("Failed to lock on database, poisoned")]
    DatabaseLock,
    #[error("Failed to lock on events bus, poisoned")]
    EventsBusLock,
}

/// The possible state of connection to bitcoin node we have.
///
/// We don't take into account handshaking substate as it have no pratical value
/// to the API user.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeStatus {
    Disconnected,
    Connected,
}

/// The core object that holds all resources of the indexer server. The main object
/// the user of the code should interact with.
pub struct Indexer {
    network: Network,
    node_address: String,
    start_height: u32,
    node_connected: Arc<AtomicBool>,
    database: Arc<Mutex<Connection>>,
    headers_cache: Arc<Mutex<HeadersCache>>,
    batch_size: u32,
    remote_height: Arc<AtomicU32>,
    rescan: bool,
    events_bus: Arc<Mutex<Bus<Event>>>,
}

impl Indexer {
    pub fn builder() -> IndexerBuilder {
        IndexerBuilder::new()
    }

    /// Which network the indexer is configured for
    pub fn network(&self) -> Network {
        self.network
    }

    /// Get current state of connection the node
    pub fn node_status(&self) -> NodeStatus {
        let connected = self.node_connected.load(atomic::Ordering::Relaxed);
        if connected {
            NodeStatus::Connected
        } else {
            NodeStatus::Disconnected
        }
    }

    /// Get the height of known main chain of blocks we have sequence of headers for
    pub fn chain_height(&self) -> Result<u32, Error> {
        Ok(self
            .headers_cache
            .lock()
            .map_err(|_| ErrorKind::HeadersCacheLock)?
            .get_current_height())
    }

    /// Get the height we already have scanned
    pub fn scanned_height(&self) -> Result<u32, Error> {
        Ok(self.start_height)
    }

    /// Get access to internal database (for making queries)
    pub fn get_database(&self) -> Arc<Mutex<Connection>> {
        self.database.clone()
    }

    /// Make a events receiver to listen events about the indexing
    pub fn add_event_reader(&self) -> Result<BusReader<Event>, Error> {
        let mut events_bus = self
            .events_bus
            .lock()
            .map_err(|_| ErrorKind::EventsBusLock)?;
        Ok(events_bus.add_rx())
    }

    /// Executes the internal threads (connection to the node, indexing worker) and awaits
    /// of their termination. Intended to be run in separate thread.
    pub fn run(&self) -> Result<(), Error> {
        // Make events fan-in
        let (events_sender, events_receiver) = sync_channel(EVENTS_CAPACITY);
        // Make events fan-out
        let mut events_bus = self
            .events_bus
            .lock()
            .map_err(|_| ErrorKind::EventsBusLock)?;
        // Register all readers of events in advance
        let node_receiver = events_bus.add_rx();
        let mut main_receiver = events_bus.add_rx();
        // Don't hold lock
        drop(events_bus);
        // Make a flag to terminate threads after the main runner exits
        let stop_flag = Arc::new(AtomicBool::new(false));

        // User requested rescan of blocks
        if self.rescan {
            let conn = self.database.lock().map_err(|_| ErrorKind::DatabaseLock)?;
            conn.set_scanned_height(self.start_height)?;
        }

        // Connect fain-in and fan-out through dispatcher thread
        thread::spawn({
            let events_bus = self.events_bus.clone();
            move || -> Result<(), Error> {
                // Will end as soon as events receiver is dropped
                for event in events_receiver.iter() {
                    let mut events_bus = events_bus.lock().map_err(|_| ErrorKind::EventsBusLock)?;
                    events_bus.broadcast(event);
                }
                Ok(())
            }
        });

        let node_handle = {
            let address = self.node_address.clone();
            let network = self.network;
            let start_height = self.start_height;
            let events_sender = events_sender.clone();
            thread::spawn(move || -> Result<(), Error> {
                node_worker(
                    &address,
                    network,
                    start_height,
                    events_sender,
                    node_receiver,
                )?;
                Ok(())
            })
        };

        // Here we track how many blocks we
        let mut batch_left = 0;
        let mut max_scanned_height = 0;
        loop {
            // Terminate if node worker ends with unrecoverable error
            if node_handle.is_finished() {
                stop_flag.store(true, atomic::Ordering::Relaxed);
                events_sender.send(Event::Termination)?;
                let res = node_handle.join();
                match res {
                    Ok(Ok(_)) => break, // termination
                    Ok(Err(e)) => return Err(e),
                    Err(e) => std::panic::resume_unwind(e), // panic in the node worker
                }
            }

            match main_receiver.recv_timeout(Duration::from_millis(100)) {
                Err(mpmc::RecvTimeoutError::Timeout) => (), // take a chance to check termination
                Err(mpmc::RecvTimeoutError::Disconnected) => {
                    stop_flag.store(true, atomic::Ordering::Relaxed);
                    events_sender.send(Event::Termination)?;
                    return Err(ErrorKind::EventBusRecv.into());
                }
                Ok(Event::Handshaked(remote_height)) => {
                    self.on_handshake(remote_height, &events_sender)?
                }
                Ok(Event::Disconnected) => {
                    self.node_connected.store(false, atomic::Ordering::Relaxed);
                }
                Ok(Event::IncomingMessage(msg)) => match msg {
                    NetworkMessage::Ping(nonce) => {
                        events_sender.send(Event::OutcomingMessage(NetworkMessage::Pong(nonce)))?
                    }
                    NetworkMessage::Headers(headers) => {
                        self.on_new_headers(headers, &events_sender, &mut batch_left)?
                    }
                    NetworkMessage::Block(block) => self.on_new_block(
                        block,
                        &events_sender,
                        &mut batch_left,
                        &mut max_scanned_height,
                    )?,
                    NetworkMessage::Inv(invs) => self.on_new_invs(invs, &events_sender)?,
                    _ => (),
                },
                _ => (),
            }
        }

        Ok(())
    }

    fn on_handshake(&self, remote_height: u32, events_sender: &Sender<Event>) -> Result<(), Error> {
        self.node_connected.store(true, atomic::Ordering::Relaxed);
        self.remote_height
            .store(remote_height, atomic::Ordering::Relaxed);

        // start requesting headers
        trace!("Requesting first headers");
        let cache = self
            .headers_cache
            .lock()
            .map_err(|_| ErrorKind::HeadersCacheLock)?;
        let headers_msg = cache.make_get_headers()?;
        events_sender.send(Event::OutcomingMessage(NetworkMessage::GetHeaders(
            headers_msg,
        )))?;
        Ok(())
    }

    /// Reaction to the new headers from remote peer. Also requests a batch of blocks if
    /// we synced all headers. Updates the local batch counter for the [on_new_block]
    fn on_new_headers(
        &self,
        headers: Vec<Header>,
        events_sender: &Sender<Event>,
        batch_left: &mut i64,
    ) -> Result<(), Error> {
        debug!("Got {} headers from remote node", headers.len());
        {
            // Very important to lock first on the cache and next to the connection everywhere or we can deadlock
            let mut cache = self
                .headers_cache
                .lock()
                .map_err(|_| ErrorKind::HeadersCacheLock)?;
            cache.update_longest_chain(&headers)?;
            let mut conn = self.database.lock().map_err(|_| ErrorKind::DatabaseLock)?;
            cache.store(&mut conn)?;
            let current_height = cache.get_current_height();
            let mut remote_height = self.remote_height.load(atomic::Ordering::Relaxed);
            // Avoid messages that we synced over 100% (remote height is set on the handshake time)
            if current_height > remote_height {
                self.remote_height
                    .store(current_height, atomic::Ordering::Relaxed);
                remote_height = current_height;
            }
            let progress = 100.0 * current_height as f64 / remote_height as f64;
            info!(
                "New headers height {}, progress: {:.03}%",
                current_height, progress
            );
        }

        if headers.len() == MAX_HEADERS_PER_MSG {
            let headers_msg = {
                let cache = self
                    .headers_cache
                    .lock()
                    .map_err(|_| ErrorKind::HeadersCacheLock)?;
                cache.make_get_headers()?
            };
            debug!("Requesting next headers batch");
            events_sender.send(Event::OutcomingMessage(NetworkMessage::GetHeaders(
                headers_msg,
            )))?
        } else if *batch_left <= 0 {
            // Request blocks to scan
            let cache = self
                .headers_cache
                .lock()
                .map_err(|_| ErrorKind::HeadersCacheLock)?;
            let height = cache.get_current_height();
            let scanned_height = {
                let conn = self.database.lock().map_err(|_| ErrorKind::DatabaseLock)?;
                conn.get_scanned_height()?
            };

            let msg: NetworkMessage = cache.make_get_blocks(scanned_height + 1, self.batch_size)?;
            events_sender.send(Event::OutcomingMessage(msg))?;
            // Remember how much blocks we expect
            let actual_batch = self.batch_size.min(height - scanned_height);
            debug!("Request {} blocks", actual_batch);
            *batch_left += actual_batch as i64;
        }
        Ok(())
    }

    /// React on new arrived block. Also updates the local information how many blocks left in batches and
    /// cached maximum height of that batch.
    fn on_new_block(
        &self,
        block: Block,
        events_sender: &Sender<Event>,
        batch_left: &mut i64,
        max_scanned_height: &mut u32,
    ) -> Result<(), Error> {
        trace!("Current batch size: {}", *batch_left);
        let hash = block.block_hash();
        let height = {
            let cache = self
                .headers_cache
                .lock()
                .map_err(|_| ErrorKind::HeadersCacheLock)?;
            cache.get_header(hash)?.height
        };

        debug!("Got block: {}", hash);
        self.process_block(block, height)?;
        *batch_left -= 1;

        // Remember max height we scanned
        let scanned_height = (*max_scanned_height).max(height);
        *max_scanned_height = scanned_height;
        // Scanned all blocks from batch, request next one
        trace!("Batch left: {}", batch_left);
        if *batch_left <= 0 {
            // Display progress
            let cache = self
                .headers_cache
                .lock()
                .map_err(|_| ErrorKind::HeadersCacheLock)?;
            let current_height = cache.get_current_height();
            let scanned_part = 100.0 * scanned_height as f64 / current_height as f64;
            info!(
                "Scanned {}/{} {:.03}%",
                scanned_height, current_height, scanned_part
            );

            // Store how much we scanned
            let conn = self.database.lock().map_err(|_| ErrorKind::DatabaseLock)?;
            conn.set_scanned_height(scanned_height)?;

            if scanned_height < current_height {
                let msg: NetworkMessage =
                    cache.make_get_blocks(scanned_height + 1, self.batch_size)?;
                events_sender.send(Event::OutcomingMessage(msg))?;
                let actual_batch = self.batch_size.min(current_height - scanned_height);
                debug!("Request {} blocks", actual_batch);
                *batch_left += actual_batch as i64;
            }
        }
        Ok(())
    }

    /// Remote node will send inventory messages if there are new blocks mined.
    /// Here we request header of that block to trigger sync logic above in [on_new_headers]
    /// and [on_new_block]
    fn on_new_invs(
        &self,
        invs: Vec<Inventory>,
        events_sender: &Sender<Event>,
    ) -> Result<(), Error> {
        for inv in invs {
            if let Inventory::Block(hash) = inv {
                let cache = self
                    .headers_cache
                    .lock()
                    .map_err(|_| ErrorKind::HeadersCacheLock)?;

                // Check if we know the header
                if cache.get_header(hash).is_err() {
                    let headers_msg = cache.make_get_headers()?;
                    events_sender.send(Event::OutcomingMessage(NetworkMessage::GetHeaders(
                        headers_msg,
                    )))?;
                }
            }
        }
        Ok(())
    }

    /// Iterate over transactions in the block and parse them. Stores the found vault
    /// transactions in database.
    fn process_block(&self, block: Block, height: u32) -> Result<(), Error> {
        let block_hash = block.block_hash();
        for (i, tx) in block.txdata.into_iter().enumerate() {
            // Detect vault transactions
            if self.detect_vault_tx(block_hash, height, i, &tx)? {
                continue;
            }
            // Detect UNIT token transactions
            self.detect_unit_tx(block_hash, height, i, &tx)?;
        }
        Ok(())
    }

    /// If given transaction is Vault related, store it inside the database
    fn detect_vault_tx(
        &self,
        block_hash: BlockHash,
        height: u32,
        i: usize,
        tx: &Transaction,
    ) -> Result<bool, Error> {
        match VaultTx::from_tx(&tx) {
            Err(err) => {
                if !err.is_definetely_not_vault() {
                    error!("Got transaction {}, that possible vault related, but we failed to parse with: {err}", tx.compute_wtxid());
                    //panic!("Stop here for debug");
                }
                Ok(false)
            }
            Ok(vtx) => {
                info!("New vault {} transaction: {}", vtx.action, vtx.txid);
                debug!("Found a vault transaction: {:#?}", vtx);

                let mut conn = self.database.lock().map_err(|_| ErrorKind::DatabaseLock)?;
                match conn.store_vault_tx(&vtx, block_hash, i, height, &tx) {
                    Err(e) => {
                        error!("Failed to store vault tx {} from block {block_hash} at height {height}, reason: {e}", vtx.txid);
                        //panic!("Stop here for debug");
                    }
                    Ok(meta) => {
                        let mut events_bus = self
                            .events_bus
                            .lock()
                            .map_err(|_| ErrorKind::EventsBusLock)?;
                        events_bus.broadcast(Event::NewTransaction(meta));
                    }
                }
                Ok(true)
            }
        }
    }

    // If given transaction is UNIT related runestone (push 13), store it in database
    fn detect_unit_tx(
        &self,
        block_hash: BlockHash,
        height: u32,
        i: usize,
        tx: &Transaction,
    ) -> Result<bool, Error> {
        match UnitTransaction::from_tx(&tx) {
            Err(err) => {
                if !err.is_definetely_not_unit() {
                    trace!("Got transaction {}, that possible UNIT related, but we failed to parse with error: {err}", tx.compute_txid());
                    //panic!("Stop here for debug");
                }
                Ok(false)
            }
            Ok(utx) => {
                info!("New UNIT transaction: {}", utx.txid);
                debug!("Found a vault transaction: {:#?}", utx);

                let mut conn = self.database.lock().map_err(|_| ErrorKind::DatabaseLock)?;
                match conn.store_unit_tx(&tx, utx.unit_amount) {
                    Err(e) => {
                        error!("Failed to store vault tx {} from block {block_hash} at height {height}, reason: {e}", tx.compute_txid());
                        //panic!("Stop here for debug");
                    }
                    Ok(_) => {
                        let mut events_bus = self
                            .events_bus
                            .lock()
                            .map_err(|_| ErrorKind::EventsBusLock)?;
                        events_bus.broadcast(Event::NewUnitTransaction(NewUnitTx {
                            utx,
                            block_hash,
                            block_pos: i,
                            height,
                        }));
                    }
                }
                Ok(true)
            }
        }
    }
}

// A way to get lazy building behavior where order of settings doesn't affect
// the result. For instance, setting network after or before node address must not
// change the result.
type LazyBuilder<T> = Box<dyn FnOnce() -> T>;

/// Builder of indexer allows to specify parameters to the system before actually making a new instance
/// of the service.
pub struct IndexerBuilder {
    network_builder: LazyBuilder<Network>,
    node_builder: LazyBuilder<String>,
    start_height_builder: LazyBuilder<u32>,
    db_path_builder: LazyBuilder<PathBuf>,
    batch_size_builder: LazyBuilder<u32>,
    rescan_builder: LazyBuilder<bool>,
}

impl IndexerBuilder {
    fn new() -> Self {
        IndexerBuilder {
            network_builder: Box::new(|| Network::Bitcoin),
            node_builder: Box::new(|| "45.79.52.207:38333".to_owned()),
            start_height_builder: Box::new(|| 0),
            db_path_builder: Box::new(|| ":memory:".into()),
            batch_size_builder: Box::new(|| 500),
            rescan_builder: Box::new(|| false),
        }
    }

    pub fn network(mut self, network: Network) -> Self {
        self.network_builder = Box::new(move || network);
        self
    }

    pub fn node<A: Into<String>>(mut self, address: A) -> Self {
        let addr_str: String = address.into();
        self.node_builder = Box::new(move || addr_str);
        self
    }

    /// Setup SQlite state path. By default is ":memory:"
    pub fn db<P: AsRef<Path>>(mut self, path: P) -> Self {
        let path_buf = path.as_ref().into();
        self.db_path_builder = Box::new(move || path_buf);
        self
    }

    /// Setup how many blocks request per one request
    pub fn batch_size(mut self, size: u32) -> Self {
        self.batch_size_builder = Box::new(move || size);
        self
    }

    /// From which block to start scanning the blockchain
    pub fn start_height(mut self, height: u32) -> Self {
        self.start_height_builder = Box::new(move || height);
        self
    }

    /// If set the block scanning begins from the start height.
    /// Doesn't reset the headers registry.
    pub fn rescan(mut self, flag: bool) -> Self {
        self.rescan_builder = Box::new(move || flag);
        self
    }

    pub fn build(self) -> Result<Indexer, Error> {
        let start_height = (self.start_height_builder)();
        let db_path = (self.db_path_builder)();
        let network = (self.network_builder)();
        let rescan = (self.rescan_builder)();
        let database = initialize_db(&db_path, network, start_height, rescan)?;
        let headers_cache = HeadersCache::load(&database)?;
        Ok(Indexer {
            network,
            node_address: (self.node_builder)(),
            start_height,
            node_connected: Arc::new(AtomicBool::new(false)),
            database: Arc::new(Mutex::new(database)),
            headers_cache: Arc::new(Mutex::new(headers_cache)),
            batch_size: (self.batch_size_builder)(),
            remote_height: Arc::new(AtomicU32::new(0)),
            rescan,
            events_bus: Arc::new(Mutex::new(Bus::new(EVENTS_CAPACITY))),
        })
    }
}
