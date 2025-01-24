use bitcoin::p2p::message::NetworkMessage;

/// Amount of events in the internal bus allowed unprocessed
pub const EVENTS_CAPACITY: usize = 32000;

/// Events that are passed between parts of the system
pub enum Event {
    /// Node passes handshake process
    Handshaked,
    /// We lost connection to the node
    Disconnected,
    /// Node sent a new message to us
    IncomingMessage(NetworkMessage),
    /// We want to send a message to node
    OutcomingMessage(NetworkMessage),
}
