use crate::db::vault::advance::DatabaseVaultAdvance;
use crate::vault::{OraclePrice, UnitAmount, VaultId, VaultTx};
use crate::Network;
use crate::{indexer::event::Event, Indexer};
use bitcoin::hex::HexToArrayError;
use bitcoin::{BlockHash, Txid};
use bus::BusReader;
use core::str::FromStr;
use log::{error, trace};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use thiserror::Error;
use websocket::stream::sync::TcpStream;
use websocket::sync::Client;
use websocket::sync::Server;
use websocket::{Message, OwnedMessage, WebSocketError};

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
    #[error("Database error: {0}")]
    Database(#[from] crate::db::error::Error),
    #[error("Failed to send message to queue")]
    SendingBus,
    #[error("Failed to get lock on database")]
    DbLock,
}

/// Starts a background thread that implements websocket service for indexer
pub fn start_websocket_server(indexer: Arc<Indexer>, bind_addr: &str) -> Result<(), Error> {
    let server = Server::bind(bind_addr)?;
    let network = indexer.network();
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
                match client_handler(network, client, &addr, events_bus, database) {
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
    AllHistory(Vec<VaultTxInfo>),
    VaultHistory(Vec<VaultTxInfo>),
    Dummy,
}

#[derive(Serialize)]
pub struct ClientError {
    pub error: String,
}

#[derive(Serialize)]
pub struct VaultTxInfo {
    pub vault_id: String,
    pub txid: String,
    pub op_return_output: u32,
    pub version: String,
    pub action: String,
    pub balance: UnitAmount,
    pub oracle_price: OraclePrice,
    pub oracle_timestamp: u32,
    pub liquidation_price: Option<OraclePrice>,
    pub liquidation_hash: Option<String>,
    pub block_hash: String,
    pub height: u32,
    pub tx_url: String,
}

impl VaultTxInfo {
    pub fn new(
        network: Network,
        vault_id: VaultId,
        vault_tx: &VaultTx,
        block_hash: BlockHash,
        height: u32,
    ) -> Self {
        VaultTxInfo {
            vault_id: vault_id.to_string(),
            txid: vault_tx.txid.to_string(),
            op_return_output: vault_tx.output,
            version: vault_tx.version.to_string(),
            action: vault_tx.action.to_string(),
            balance: vault_tx.balance,
            oracle_price: vault_tx.oracle_price,
            oracle_timestamp: vault_tx.oracle_timestamp,
            liquidation_price: vault_tx.liquidation_price,
            liquidation_hash: vault_tx.liquidation_hash.map(|h| hex::encode(&h)),
            block_hash: block_hash.to_string(),
            height,
            tx_url: network.explorer_url(vault_tx.txid),
        }
    }
}

/// Max amount of queued messages in websocket
const MAX_WEBSOCKET_MESSAGES: usize = 10000;

fn client_handler(
    network: Network,
    client: Client<TcpStream>,
    addr: &str,
    events_bus: BusReader<Event>,
    database: Arc<Mutex<Connection>>,
) -> Result<(), Error> {
    let (mut client_receiver, mut client_sender) = client.split().unwrap();
    let (bus_sender, bus_receiver) = mpsc::sync_channel(MAX_WEBSOCKET_MESSAGES);

    // Spawn listener of indexer events
    thread::spawn({
        let sender = bus_sender.clone();
        let addr = addr.to_owned();
        move || -> Result<(), Error> {
            for event in events_bus {
                match event {
                    Event::NewTransaction(new_tx) => {
                        trace!(
                            "Got message about new tx {} for vault {}",
                            new_tx.vault_tx.txid,
                            new_tx.vault_id
                        );
                        let info = VaultTxInfo::new(
                            network,
                            new_tx.vault_id,
                            &new_tx.vault_tx,
                            new_tx.block_hash,
                            new_tx.height,
                        );
                        let encoded_info = match serde_json::to_string_pretty(&info) {
                            Err(e) => {
                                error!("Failed to encode tx {} for vault {} for client {addr}, reason: {}", new_tx.vault_tx.txid, new_tx.vault_id, e);
                                continue;
                            }
                            Ok(str) => str,
                        };
                        sender
                            .send(Message::text(encoded_info))
                            .map_err(|_| Error::SendingBus)?;
                    }
                    _ => (),
                }
            }
            Ok(())
        }
    });

    // Spawn thread that will send all messages to the websocket
    thread::spawn(move || -> Result<(), Error> {
        for msg in bus_receiver {
            client_sender.send_message(&msg)?;
        }
        Ok(())
    });

    let sender = bus_sender;
    for res_message in client_receiver.incoming_messages() {
        let message: OwnedMessage = res_message?;
        match message {
            OwnedMessage::Text(txt) => {
                let request: Request = match serde_json::from_str(&txt) {
                    Err(e) => {
                        error!("Failed to decode client {addr} request: {e}");
                        let err_msg = serde_json::to_string(&ClientError {
                            error: e.to_string(),
                        })?;
                        sender
                            .send(Message::text(err_msg))
                            .map_err(|_| Error::SendingBus)?;
                        continue;
                    }
                    Ok(request) => request,
                };
                trace!("Client {addr} request: {request:?}");
                let response = match process_request(network, request, database.clone()) {
                    Err(e) => {
                        error!("Failed to process client {addr} request: {e}");
                        let err_msg = serde_json::to_string(&ClientError {
                            error: e.to_string(),
                        })?;
                        sender
                            .send(Message::text(err_msg))
                            .map_err(|_| Error::SendingBus)?;
                        continue;
                    }
                    Ok(response) => response,
                };
                let encoded_response = serde_json::to_string(&response)?;
                sender
                    .send(Message::text(encoded_response))
                    .map_err(|_| Error::SendingBus)?;
            }
            OwnedMessage::Binary(_) => {
                sender
                    .send(Message::text("Expected JSON request"))
                    .map_err(|_| Error::SendingBus)?;
                return Err(Error::UnsupportedBinary);
            }
            OwnedMessage::Ping(v) => {
                sender
                    .send(Message::pong(v))
                    .map_err(|_| Error::SendingBus)?;
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

fn process_request(
    network: Network,
    request: Request,
    database: Arc<Mutex<Connection>>,
) -> Result<Response, Error> {
    match request {
        Request::AllHistory {
            timestamp_start,
            timestamp_end,
        } => handler_all_history(network, database, timestamp_start, timestamp_end),
        Request::VaultHistory {
            vault_open_txid,
            timestamp_start,
            timestamp_end,
        } => {
            let txid = Txid::from_str(&vault_open_txid)
                .map_err(|e| Error::ValidateTxid(vault_open_txid, e))?;
            handler_vault_history(network, database, txid, timestamp_start, timestamp_end)
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
    network: Network,
    database: Arc<Mutex<Connection>>,
    timestamp_start: Option<u32>,
    timestamp_end: Option<u32>,
) -> Result<Response, Error> {
    let conn = database.lock().map_err(|_| Error::DbLock)?;
    let metas = conn.range_history_all(timestamp_start, timestamp_end)?;
    let infos = metas
        .into_iter()
        .map(|meta| {
            VaultTxInfo::new(
                network,
                meta.vault_id,
                &meta.vault_tx,
                meta.block_hash,
                meta.height,
            )
        })
        .collect();
    Ok(Response::AllHistory(infos))
}

fn handler_vault_history(
    network: Network,
    database: Arc<Mutex<Connection>>,
    vault_open_txid: Txid,
    timestamp_start: Option<u32>,
    timestamp_end: Option<u32>,
) -> Result<Response, Error> {
    let conn = database.lock().map_err(|_| Error::DbLock)?;
    let metas = conn.range_history_vault(vault_open_txid, timestamp_start, timestamp_end)?;
    let infos = metas
        .into_iter()
        .map(|meta| {
            VaultTxInfo::new(
                network,
                meta.vault_id,
                &meta.vault_tx,
                meta.block_hash,
                meta.height,
            )
        })
        .collect();
    Ok(Response::VaultHistory(infos))
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
