use core::net::{IpAddr, Ipv4Addr, SocketAddr};
use core::sync::atomic::{self, AtomicBool};
use core::time::Duration;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::mpmc::{self, Receiver, Sender};
use std::sync::mpsc::SendError;
use std::sync::Arc;
use std::thread::{self, sleep};
use std::time::{SystemTime, UNIX_EPOCH};

use bitcoin::consensus::{self, encode};
use bitcoin::key::rand::RngCore;
use bitcoin::p2p::message::{NetworkMessage, RawNetworkMessage};
use bitcoin::p2p::message_network::VersionMessage;
use bitcoin::{p2p, secp256k1};
use log::*;
use thiserror::Error;

use crate::Network;

use super::event::Event;

/// How we introduce ourselves to other nodes
/// TODO: make configurable
const DEFAULT_USER_AGENT: &str = "Vault indexer 0.1.0";

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Failed to send event to bus: {0}")]
    EventBusSend(#[from] SendError<Event>),
    #[error("Failed to receive event to bus, disconnected")]
    EventBusRecv,
    #[error("Cannot clone socket handle: {0}")]
    SocketCloneFail(std::io::Error),
    #[error("Cannot properly shutdown TCP socket: {0}")]
    SocketShutdownFail(std::io::Error),
    #[error("Failed to resolve node address {0}: {1}")]
    FailedResolve(String, std::io::Error),
    #[error("Cannot resolve network address of the node {0}")]
    NoSocketAddress(String),
    #[error("Cannot connect to the node {0}: {1}")]
    Connection(String, std::io::Error),
    #[error("Cannot send the message {0:?}, reason: {1}")]
    SendingMsg(NetworkMessage, std::io::Error),
    #[error("Cannot peek header of the next message: {0}")]
    ReceivingHeader(std::io::Error),
    #[error("Cannot peek next message payload: {0}")]
    ReceivingPayload(std::io::Error),
    #[error("Wrong network magic (wrong network), received: {0:x?}, expected: {1:x?}")]
    WrongMagic(Vec<u8>, [u8; 4]),
    #[error("Cannot decode network message: {0}, payload: {1:x?}")]
    DecodingMessage(bitcoin::consensus::encode::Error, Vec<u8>),
    #[error("First message from the peer is not version message")]
    NoVersionMessage,
    #[error("Second message from the peer is not version ack")]
    NoVerackMessage,
}

/// Reconnection delay in seconds
const RECONNECTION_TIMEOUT: u64 = 10;

// The endless blocking worker for the node connection, will process events and recoverable errors inside
pub fn node_worker(
    address: &str,
    network: Network,
    start_height: u32,
    events_sender: Sender<Event>,
    events_receiver: Receiver<Event>,
) -> Result<(), Error> {
    loop {
        match node_process(
            address,
            network,
            start_height,
            events_sender.clone(),
            events_receiver.clone(),
        ) {
            Err(e @ Error::EventBusSend(_)) => {
                // We consider that unrecoverable failure of the system
                error!("{e}");
                return Err(e);
            }
            Err(e @ Error::FailedResolve(_, _)) => {
                // Not valid address of the node, no need to reconnect
                error!("{e}");
                return Err(e);
            }
            Err(e @ Error::NoSocketAddress(_)) => {
                // Perhaphs DNS failure, no need to reconnect
                error!("{e}");
                return Err(e);
            }
            Err(e @ Error::WrongMagic(_, _)) => {
                // Misconfigured node or indexer, exiting
                error!("{e}");
                return Err(e);
            }
            Err(e) => {
                error!("{e}");
                events_sender.send(Event::Disconnected)?;
                warn!("Reconnecting to the node in {RECONNECTION_TIMEOUT} seconds...");
                sleep(Duration::from_secs(RECONNECTION_TIMEOUT));
            }
            Ok(_) => {
                // Termination procedure
                return Ok(());
            }
        }
    }
}

// Body of worker that connects to the node and processes all messages incoming and outcoming
fn node_process(
    address: &str,
    network: Network,
    start_height: u32,
    events_sender: Sender<Event>,
    events_receiver: Receiver<Event>,
) -> Result<(), Error> {
    let stream: TcpStream = node_handshake(address, network, start_height)?;
    events_sender.send(Event::Handshaked)?;
    let stop_flag = Arc::new(AtomicBool::new(false));

    // One thread read from the socket
    let mut receiver_stream = stream.try_clone().map_err(Error::SocketCloneFail)?;
    let receiver_handle = {
        let stop_flag = stop_flag.clone();
        thread::spawn(move || -> Result<(), Error> {
            loop {
                if stop_flag.load(atomic::Ordering::Relaxed) {
                    break Ok(());
                }

                match receive_message(&mut receiver_stream, network) {
                    Ok(msg) => {
                        events_sender
                            .send(Event::IncomingMessage(msg))
                            .map_err(Error::EventBusSend)?;
                    }
                    Err(e @ Error::DecodingMessage(_, _)) => {
                        // We consider that recoverable
                        error!("{e}");
                    }
                    Err(e) => return Err(e), // Should reconnect
                }
            }
        })
    };

    // Second thread is for sending messages to socket
    let mut sender_stream = stream.try_clone().map_err(Error::SocketCloneFail)?;
    let sending_handle = {
        let stop_flag = stop_flag.clone();
        thread::spawn(move || -> Result<(), Error> {
            loop {
                if stop_flag.load(atomic::Ordering::Relaxed) {
                    break Ok(());
                }

                match events_receiver.recv_timeout(Duration::from_millis(100)) {
                    Err(mpmc::RecvTimeoutError::Timeout) => (), // take a chance to check termination
                    Err(mpmc::RecvTimeoutError::Disconnected) => return Err(Error::EventBusRecv),
                    Ok(Event::OutcomingMessage(msg)) => {
                        send_message(&mut sender_stream, network, msg)?;
                    }
                    _ => (),
                }
            }
        })
    };

    // The main thread of the node worker listens for sending messages
    loop {
        // First, check that receiving thread is alive
        if receiver_handle.is_finished() {
            stop_flag.store(true, atomic::Ordering::Relaxed);
            // Shutting down socket will interrupt any reads from the socket to awake other thread
            stream
                .shutdown(Shutdown::Both)
                .map_err(Error::SocketShutdownFail)?;
            let res1 = receiver_handle.join();
            let res2 = sending_handle.join(); // wait when second thread exits
            match res1 {
                Ok(Ok(())) => (),                       // termination process
                Ok(Err(e)) => return Err(e),            // error in the receiving thread
                Err(e) => std::panic::resume_unwind(e), // panic in the receiving thread, non recoverable also
            }
            match res2 {
                Ok(Ok(())) => break,                    // termination process
                Ok(Err(e)) => return Err(e),            // error in the sending thread
                Err(e) => std::panic::resume_unwind(e), // panic in the sending thread, non recoverable also
            }
        }

        // Next, check that sending thread is alive
        if sending_handle.is_finished() {
            stop_flag.store(true, atomic::Ordering::Relaxed);
            // Shutting down socket will interrupt any reads from the socket to awake other thread
            stream
                .shutdown(Shutdown::Both)
                .map_err(Error::SocketShutdownFail)?;
            let res1 = sending_handle.join();
            let res2 = receiver_handle.join(); // wait when second thread exits
            match res1 {
                Ok(Ok(())) => (),                       // termination process
                Ok(Err(e)) => return Err(e),            // error in the sending thread
                Err(e) => std::panic::resume_unwind(e), // panic in the sending thread, non recoverable also
            }
            match res2 {
                Ok(Ok(())) => break,                    // termination process
                Ok(Err(e)) => return Err(e),            // error in the receive thread
                Err(e) => std::panic::resume_unwind(e), // panic in the receive thread, non recoverable also
            }
        }

        // Don't do busy loop there
        sleep(Duration::from_millis(100));
    }

    Ok(())
}

// Connect to node and do all handshake protocol (version exchange and verack messages)
fn node_handshake(address: &str, network: Network, start_height: u32) -> Result<TcpStream, Error> {
    trace!("Resolving address to node {address}...");
    let mut sock_addrs = address
        .to_socket_addrs()
        .map_err(|e| Error::FailedResolve(address.to_owned(), e))?;
    let node_addr = if let Some(addr) = sock_addrs.next() {
        addr
    } else {
        return Err(Error::NoSocketAddress(address.to_owned()));
    };

    // TODO: use connect_timeout and list of nodes
    trace!("Connecting to the {address} node...");
    let mut stream =
        TcpStream::connect(address).map_err(|e| Error::Connection(address.to_owned(), e))?;
    info!("Connected to the {address} node");

    trace!("Handshaking");
    let ver_msg = build_version_message(&node_addr, DEFAULT_USER_AGENT, start_height);
    send_message(&mut stream, network, ver_msg)?;
    trace!("Sent version message, awaiting version msg from peer...");

    let first_msg = receive_message(&mut stream, network)?;
    if let NetworkMessage::Version(_) = first_msg {
        // really don't care the correctness of the message
        trace!("Got version message from peer");
    } else {
        return Err(Error::NoVersionMessage);
    }
    // Send verack message that we accept their version
    send_message(&mut stream, network, NetworkMessage::Verack)?;
    trace!("Sent verack message");

    trace!("Awaiting verack from their side");
    let second_msg = receive_message(&mut stream, network)?;
    if let NetworkMessage::Verack = second_msg {
        trace!("Got verack message from peer");
    } else {
        return Err(Error::NoVerackMessage);
    }
    trace!("Handshake finish");
    Ok(stream)
}

fn send_message(
    stream: &mut TcpStream,
    network: Network,
    msg: NetworkMessage,
) -> Result<(), Error> {
    trace!("Sending message: {msg:?}");
    let raw_msg = RawNetworkMessage::new(network.magic(), msg.clone());
    let bytes = encode::serialize(&raw_msg);
    stream
        .write_all(&bytes)
        .map_err(|e| Error::SendingMsg(msg.clone(), e))?;
    stream.flush().map_err(|e| Error::SendingMsg(msg, e))?;
    Ok(())
}

fn receive_message(stream: &mut TcpStream, network: Network) -> Result<NetworkMessage, Error> {
    // Header size is 24 bytes
    const HEADER_SIZE: usize = 24;
    let mut header_buf = [0u8; HEADER_SIZE];
    stream
        .read_exact(&mut header_buf)
        .map_err(Error::ReceivingHeader)?;
    trace!("Received header");
    // Checking magic bytes
    let magic = &header_buf[0..4];
    let our_magic = network.magic().to_bytes();
    if magic != our_magic {
        return Err(Error::WrongMagic(magic.to_owned(), our_magic));
    }
    // Extracting the payload size from the header
    let payload_len_bytes = &header_buf[16..20];
    let payload_len =
        u32::from_le_bytes(payload_len_bytes.try_into().expect("statically known size"));
    trace!("Payload size: {payload_len}");

    // Get all payload
    let mut payload = vec![0u8; HEADER_SIZE + payload_len as usize];
    stream
        .read_exact(&mut payload[HEADER_SIZE..])
        .map_err(Error::ReceivingPayload)?;
    trace!("Read payload");
    // Copy header into start of payload and parse
    payload[0..HEADER_SIZE].copy_from_slice(&header_buf);
    let msg: RawNetworkMessage =
        consensus::deserialize(&payload).map_err(|e| Error::DecodingMessage(e, payload))?;
    trace!("Deserialized message: {msg:?}");
    Ok(msg.into_payload())
}

// https://en.bitcoin.it/wiki/Protocol_documentation#version
fn build_version_message(
    address: &SocketAddr,
    user_agent: &str,
    start_height: u32,
) -> NetworkMessage {
    // "bitfield of features to be enabled for this connection"
    let services = p2p::ServiceFlags::NONE;

    // "standard UNIX timestamp in seconds"
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| error!("Impossible! We cannot get seconds since UNIX epoch. {e}"))
        .unwrap_or(Duration::from_secs(0)) // we don't want to panic there
        .as_secs();

    // "The network address of the node receiving this message"
    let addr_recv = p2p::Address::new(address, p2p::ServiceFlags::NONE);

    // "The network address of the node emitting this message"
    // We can leave it zero
    let my_address = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
    let addr_from = p2p::Address::new(&my_address, p2p::ServiceFlags::NONE);

    // "Node random nonce, randomly generated every time a version packet is sent. This nonce is used to detect connections to self."
    let nonce: u64 = secp256k1::rand::thread_rng().next_u64();

    // Construct the message
    let msg = VersionMessage::new(
        services,
        timestamp as i64,
        addr_recv,
        addr_from,
        nonce,
        user_agent.to_owned(),
        start_height as i32,
    );
    NetworkMessage::Version(msg)
}
