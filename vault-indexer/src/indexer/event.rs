use bitcoin::{p2p::message::NetworkMessage, BlockHash};

use crate::vault::{VaultId, VaultTx};

/// Amount of events in the internal bus allowed unprocessed
pub const EVENTS_CAPACITY: usize = 32000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewTransactionEvent {
    pub vault_id: VaultId,
    pub vault_tx: VaultTx,
    pub vessel_tx: bitcoin::Transaction,
    pub block_hash: BlockHash,
    pub height: u32,
}

/// Events that are passed between parts of the system
#[derive(Debug, Clone)]
pub enum Event {
    /// Node passes handshake process, returns height of remote node
    Handshaked(u32),
    /// We lost connection to the node
    Disconnected,
    /// Node sent a new message to us
    IncomingMessage(NetworkMessage),
    /// We want to send a message to node
    OutcomingMessage(NetworkMessage),
    /// Event to terminate internal workers
    Termination,
    /// Event fired when we encounter new vault transaction
    NewTransaction(NewTransactionEvent),
}
