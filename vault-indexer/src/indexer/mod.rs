use core::result::Result;
use std::sync::mpmc::sync_channel;
pub use bitcoin::Network;
use event::EVENTS_CAPACITY;
use thiserror::Error;

use node::node_worker;

mod node; 
mod event;

/// All kind of errors the indexer can produce
#[derive(Debug, Error)]
pub enum Error {
    #[error("Node worker failure: {0}")]
    Node(#[from] node::Error),
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
}

impl Indexer {
    pub fn builder() -> IndexerBuilder {
        IndexerBuilder::new()
    }

    /// Which network the indexer is configured for
    pub fn network(&self) -> Network {
        self.network
    }

    pub fn node_status(&self) -> NodeStatus {
        NodeStatus::Disconnected
    }

    /// Executes the internal threads (connection to the node, indexing worker) and awaits
    /// of their termination. Intended to be run in separate thread.
    pub fn run(&self) -> Result<(), Error> {
        let (events_sender, events_receiver) = sync_channel(EVENTS_CAPACITY);

        node_worker(&self.node_address, self.network, self.start_height, events_sender, events_receiver)?;
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
        })
    }
}
