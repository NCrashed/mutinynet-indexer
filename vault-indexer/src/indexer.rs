use core::{net::SocketAddr, result::Result};
use std::net::ToSocketAddrs;

/// All kind of errors the indexer can produce
#[derive(Debug)]
pub enum Error {

}

/// Which network we run the indexer on 
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Network {
    /// Main network
    Bitcoin,
    /// Also includes Mutiny signet
    Signet,
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

}

impl Indexer {
    pub fn builder() -> IndexerBuilder {
        IndexerBuilder::new()
    }

    pub fn node_status(&self) -> NodeStatus {
        NodeStatus::Disconnected
    }

    /// Executes the internal threads (connection to the node, indexing worker) and awaits 
    /// of their termination. Intended to be run in separate thread.
    pub fn run(&self) -> Result<(), Error> {
        loop {}
    }
}

// A way to get lazy building behavior where order of settings doesn't affect
// the result. For instance, setting network after or before node address must not
// change the result.
type LazyBuilder<S, T> = Box<dyn Fn(&S) -> Result<T, Error>>;

/// Builder of indexer allows to specify parameters to the system before actually making a new instance 
/// of the service.
pub struct IndexerBuilder {

}

impl IndexerBuilder {
    fn new() -> Self {
        IndexerBuilder {  }
    }

    pub fn network(mut self, network: Network) -> Self {
        self
    }

    pub fn node<A: ToSocketAddrs>(mut self, address: A) -> Self {
        self
    }

    pub fn build(self) -> Result<Indexer, Error> {
        Ok(Indexer {})
    }
}