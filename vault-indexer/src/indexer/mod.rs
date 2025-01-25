use core::{
    result::Result,
    sync::atomic::{self, AtomicBool},
    time::Duration,
};
use event::{Event, EVENTS_CAPACITY};
pub use network::Network;
use std::sync::mpmc;
use std::{sync::mpmc::sync_channel, thread};
use thiserror::Error;

use node::node_worker;

mod event;
mod network;
mod node;

/// All kind of errors the indexer can produce
#[derive(Debug, Error)]
pub enum Error {
    #[error("Node worker failure: {0}")]
    Node(#[from] node::Error),
    #[error("Failed to read from events bus, disconnected.")]
    EventBusRecv,
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
    node_connected: AtomicBool,
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

    /// Executes the internal threads (connection to the node, indexing worker) and awaits
    /// of their termination. Intended to be run in separate thread.
    pub fn run(&self) -> Result<(), Error> {
        let (events_sender, events_receiver) = sync_channel(EVENTS_CAPACITY);

        let node_handle = {
            let address = self.node_address.clone();
            let network = self.network;
            let start_height = self.start_height;
            let events_sender = events_sender.clone();
            let events_receiver = events_receiver.clone();
            thread::spawn(move || -> Result<(), Error> {
                node_worker(
                    &address,
                    network,
                    start_height,
                    events_sender,
                    events_receiver,
                )?;
                Ok(())
            })
        };

        loop {
            // Terminate if node worker ends with unrecoverable error
            if node_handle.is_finished() {
                let res = node_handle.join();
                match res {
                    Ok(Ok(_)) => break, // termination
                    Ok(Err(e)) => return Err(e),
                    Err(e) => std::panic::resume_unwind(e), // panic in the node worker
                }
            }

            match events_receiver.recv_timeout(Duration::from_millis(100)) {
                Err(mpmc::RecvTimeoutError::Timeout) => (), // take a chance to check termination
                Err(mpmc::RecvTimeoutError::Disconnected) => return Err(Error::EventBusRecv),
                Ok(Event::Handshaked) => {
                    self.node_connected.store(true, atomic::Ordering::Relaxed);
                }
                Ok(Event::Disconnected) => {
                    self.node_connected.store(false, atomic::Ordering::Relaxed);
                }
                Ok(Event::IncomingMessage(msg)) => {}
                _ => (),
            }
        }

        Ok(())
    }
}

// A way to get lazy building behavior where order of settings doesn't affect
// the result. For instance, setting network after or before node address must not
// change the result.
type LazyBuilder<T> = Box<dyn FnOnce() -> Result<T, Error>>;

/// Builder of indexer allows to specify parameters to the system before actually making a new instance
/// of the service.
pub struct IndexerBuilder {
    network_builder: LazyBuilder<Network>,
    node_builder: LazyBuilder<String>,
    start_height_builder: LazyBuilder<u32>,
}

impl IndexerBuilder {
    fn new() -> Self {
        IndexerBuilder {
            network_builder: Box::new(|| Ok(Network::Bitcoin)),
            node_builder: Box::new(|| Ok("45.79.52.207:38333".to_owned())),
            start_height_builder: Box::new(|| Ok(0)),
        }
    }

    pub fn network(mut self, network: Network) -> Self {
        self.network_builder = Box::new(move || Ok(network));
        self
    }

    pub fn node<A: Into<String>>(mut self, address: A) -> Self {
        let addr_str: String = address.into();
        self.node_builder = Box::new(move || Ok(addr_str));
        self
    }

    pub fn build(self) -> Result<Indexer, Error> {
        Ok(Indexer {
            network: (self.network_builder)()?,
            node_address: (self.node_builder)()?,
            start_height: (self.start_height_builder)()?,
            node_connected: AtomicBool::new(false),
        })
    }
}
