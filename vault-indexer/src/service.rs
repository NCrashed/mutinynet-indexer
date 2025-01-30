use crate::{indexer::event::Event, Indexer};
use bitcoin::hex::HexToArrayError;
use bitcoin::Txid;
use bus::BusReader;
use core::str::FromStr;
use log::{error, trace};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use std::thread;
use thiserror::Error;
use websocket::stream::sync::TcpStream;
use websocket::sync::Client;
use websocket::{sync::Server, Message};
use websocket::{OwnedMessage, WebSocketError};

#[derive(Debug, Error)]
pub enum Error {
    #[error("Websocket error: {0}")]
    Websocket(#[from] std::io::Error),
    #[error("Client error: {0}")]
    ClientError(#[from] WebSocketError),
    #[error("Client sent binary payload")]
    UnsupportedBinary,
    #[error("Cannot encode JSON for response: {0}")]
    EncodingMessage(#[from] serde_json::Error),
    #[error("Cannot parse transaction hash {0}, reason: {1}")]
    ValidateTxid(String, HexToArrayError),
}

/// Starts a background thread that implements websocket service for indexer
pub fn start_websocket_server(indexer: Arc<Indexer>, bind_addr: &str) -> Result<(), Error> {
    let server = Server::bind(bind_addr)?;
    // Listen new connections in new thread
    thread::spawn(move || {
        trace!("Spawn websocket server thread");
        for connection in server.filter_map(Result::ok) {
            let events_bus = match indexer.add_event_reader() {
                Err(e) => {
                    error!("Failed to get events bus for new connection: {e}");
                    continue;
                }
                Ok(v) => v,
            };
            let database = indexer.get_database().clone();

            // Spawn a new thread for each connection.
            trace!("New websocket connection");
            thread::spawn(move || {
                let client = match connection.accept() {
                    Err((stream, e)) => {
                        let addr = stream
                            .peer_addr()
                            .map_or("".to_owned(), |addr| addr.to_string());
                        error!("Failed to accept {addr} connection: {e}");
                        return;
                    }
                    Ok(client) => client,
                };
                let addr = client
                    .peer_addr()
                    .map_or("".to_owned(), |addr| addr.to_string());
                trace!("Handshaked with {addr}");
                match client_handler(client, &addr, events_bus, database) {
                    Err(e) => {
                        error!("Connection with {addr} closed with error: {e}");
                    }
                    Ok(_) => {
                        trace!("Connection with {addr} closed normally");
                    }
                }
            });
        }
    });
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(tag = "method")]
pub enum Request {
    #[serde(rename = "range_history_all")]
    AllHistory {
        timestamp_start: Option<u32>,
        timestamp_end: Option<u32>,
    },
    #[serde(rename = "vault_history_tx")]
    VaultHistory {
        vault_open_txid: String,
        timestamp_start: Option<u32>,
        timestamp_end: Option<u32>,
    },
    #[serde(rename = "action_history")]
    ActionHistory {
        timestamp_start: Option<u32>,
        timestamp_end: Option<u32>,
    },
    #[serde(rename = "overall_volume")]
    OverallVolume {
        timestamp_start: Option<u32>,
        timestamp_end: Option<u32>,
    },
}

#[derive(Serialize)]
pub enum Response {
    Dummy,
}

#[derive(Serialize)]
pub struct ClientError {
    pub error: String,
}

fn client_handler(
    client: Client<TcpStream>,
    addr: &str,
    events_bus: BusReader<Event>,
    database: Arc<Mutex<Connection>>,
) -> Result<(), Error> {
    let (mut receiver, mut sender) = client.split().unwrap();
    for res_message in receiver.incoming_messages() {
        let message: OwnedMessage = res_message?;
        match message {
            OwnedMessage::Text(txt) => {
                let request: Request = match serde_json::from_str(&txt) {
                    Err(e) => {
                        error!("Failed to decode client {addr} request: {e}");
                        let err_msg = serde_json::to_string(&ClientError {
                            error: e.to_string(),
                        })?;
                        sender.send_message(&Message::text(err_msg))?;
                        continue;
                    }
                    Ok(request) => request,
                };
                trace!("Client {addr} request: {request:?}");
                let response = match process_request(request, database.clone()) {
                    Err(e) => {
                        error!("Failed to process client {addr} request: {e}");
                        let err_msg = serde_json::to_string(&ClientError {
                            error: e.to_string(),
                        })?;
                        sender.send_message(&Message::text(err_msg))?;
                        continue;
                    }
                    Ok(response) => response,
                };
                let encoded_response = serde_json::to_string(&response)?;
                sender.send_message(&Message::text(encoded_response))?;
            }
            OwnedMessage::Binary(b) => {
                sender.send_message(&Message::text("Expected JSON request"))?;
                return Err(Error::UnsupportedBinary);
            }
            OwnedMessage::Ping(v) => {
                sender.send_message(&Message::pong(v))?;
            }
            OwnedMessage::Close(data) => {
                trace!("Client closed with {data:?}");
                break;
            }
            OwnedMessage::Pong(_) => (),
        }
    }
    Ok(())
}

fn process_request(request: Request, database: Arc<Mutex<Connection>>) -> Result<Response, Error> {
    match request {
        Request::AllHistory {
            timestamp_start,
            timestamp_end,
        } => handler_all_history(database, timestamp_start, timestamp_end),
        Request::VaultHistory {
            vault_open_txid,
            timestamp_start,
            timestamp_end,
        } => {
            let txid = Txid::from_str(&vault_open_txid)
                .map_err(|e| Error::ValidateTxid(vault_open_txid, e))?;
            handler_vault_history(database, txid, timestamp_start, timestamp_end)
        }
        Request::ActionHistory {
            timestamp_start,
            timestamp_end,
        } => handler_action_history(database, timestamp_start, timestamp_end),
        Request::OverallVolume {
            timestamp_start,
            timestamp_end,
        } => handler_overall_volume(database, timestamp_start, timestamp_end),
    }
}

fn handler_all_history(
    database: Arc<Mutex<Connection>>,
    timestamp_start: Option<u32>,
    timestamp_end: Option<u32>,
) -> Result<Response, Error> {
    Ok(Response::Dummy)
}

fn handler_vault_history(
    database: Arc<Mutex<Connection>>,
    vault_open_txid: Txid,
    timestamp_start: Option<u32>,
    timestamp_end: Option<u32>,
) -> Result<Response, Error> {
    Ok(Response::Dummy)
}

fn handler_action_history(
    database: Arc<Mutex<Connection>>,
    timestamp_start: Option<u32>,
    timestamp_end: Option<u32>,
) -> Result<Response, Error> {
    Ok(Response::Dummy)
}

fn handler_overall_volume(
    database: Arc<Mutex<Connection>>,
    timestamp_start: Option<u32>,
    timestamp_end: Option<u32>,
) -> Result<Response, Error> {
    Ok(Response::Dummy)
}
