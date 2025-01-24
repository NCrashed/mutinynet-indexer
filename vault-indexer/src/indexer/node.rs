use core::net::SocketAddr;
use core::unimplemented;
use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};

use log::*;
use thiserror::Error;

use crate::Network;

/// How we introduce ourselves to other nodes
/// TODO: make configurable
const DEFAULT_USER_AGENT: &str = "Vault indexer 0.1";

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to resolve node address {0}: {1}")]
    FailedResolve(String, std::io::Error),
    #[error("Cannot resolve network address of the node {0}")]
    NoSocketAddress(String),
    #[error("Cannot connect to the node {0}: {1}")]
    Connection(String, std::io::Error),
    #[error("Cannot send the message {0}, due the {2} and payload: {1:x?}")]
    SendingMsg(String, Vec<u8>, std::io::Error),
}

pub fn node_worker(address: &str, network: Network, start_height: i32) -> Result<(), Error> {
    let stream = node_handshake(address, network, start_height)?;
    Ok(())
}

fn node_handshake(address: &str, network: Network, start_height: i32) -> Result<TcpStream, Error> {
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
    stream
        .write_all(&ver_msg)
        .map_err(|e| Error::SendingMsg("version".to_owned(), ver_msg.clone(), e))?;
    trace!("Sent version message: {ver_msg:x?}");

    Ok(stream)
}

// https://en.bitcoin.it/wiki/Protocol_documentation#version
fn build_version_message(address: &SocketAddr, user_agent: &str, start_height: i32) -> Vec<u8> {
    unimplemented!()
}
