use crate::db::vault::advance::DatabaseVaultAdvance;
use crate::db::vault::{ActionAggItem, VaultTxMeta};
use crate::vault::{OraclePrice, UnitAmount, VaultAction, VaultId, VaultTx};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize, Serialize)]
pub enum TimeSpan {
    Hour,
    Day,
    Week,
    Month,
}

impl TimeSpan {
    /// Get amounts of seconds for the time span
    pub fn time_width(&self) -> u32 {
        match self {
            TimeSpan::Hour => 3600,
            TimeSpan::Day => 3600 * 24,
            TimeSpan::Week => 3600 * 24 * 7,
            TimeSpan::Month => 3600 * 24 * 7 * 30,
        }
    }
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
        action: VaultAction,
        timespan: Option<TimeSpan>,
    },
    #[serde(rename = "overall_volume")]
    OverallVolume {},
}

#[derive(Debug, Serialize)]
pub struct OverallVolume {
    btc_volume: i64,
    unit_volume: i64,
}

#[allow(clippy::large_enum_variant)]
#[derive(Serialize)]
pub enum Response {
    NewTranscation(VaultTxInfo),
    AllHistory(Vec<VaultTxInfo>),
    VaultHistory(Vec<VaultTxInfo>),
    ActionHistory(Vec<ActionAggItem>),
    OverallVolume(OverallVolume),
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
    pub btc_custody: u64,
    pub unit_volume: i32,
    pub btc_volume: i64,
    pub prev_tx: String,
}

impl VaultTxInfo {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        network: Network,
        vault_id: VaultId,
        vault_tx: &VaultTx,
        block_hash: BlockHash,
        height: u32,
        btc_custody: u64,
        unit_volume: i32,
        btc_volume: i64,
        prev_tx: Txid,
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
            liquidation_hash: vault_tx.liquidation_hash.map(hex::encode),
            block_hash: block_hash.to_string(),
            height,
            tx_url: network.explorer_url(vault_tx.txid),
            btc_custody,
            unit_volume,
            btc_volume,
            prev_tx: network.explorer_url(prev_tx),
        }
    }

    pub fn from_db_metainfo(network: Network, meta: &VaultTxMeta) -> Self {
        VaultTxInfo::new(
            network,
            meta.vault_id,
            &meta.vault_tx,
            meta.block_hash,
            meta.height,
            meta.btc_custody,
            meta.unit_volume,
            meta.btc_volume,
            meta.prev_tx,
        )
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
                if let Event::NewTransaction(new_tx) = event {
                    trace!(
                        "Got message about new tx {} for vault {}",
                        new_tx.vault_tx.txid,
                        new_tx.vault_id
                    );
                    let info = VaultTxInfo::from_db_metainfo(network, &new_tx);
                    let encoded_info = match serde_json::to_string(&Response::NewTranscation(info))
                    {
                        Err(e) => {
                            error!(
                                "Failed to encode tx {} for vault {} for client {addr}, reason: {}",
                                new_tx.vault_tx.txid, new_tx.vault_id, e
                            );
                            continue;
                        }
                        Ok(str) => str,
                    };
                    sender
                        .send(Message::text(encoded_info))
                        .map_err(|_| Error::SendingBus)?;
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
        Request::ActionHistory { action, timespan } => {
            handler_action_history(database, action, timespan)
        }
        Request::OverallVolume {} => handler_overall_volume(database),
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
        .map(|meta| VaultTxInfo::from_db_metainfo(network, &meta))
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
        .map(|meta| VaultTxInfo::from_db_metainfo(network, &meta))
        .collect();
    Ok(Response::VaultHistory(infos))
}

fn handler_action_history(
    database: Arc<Mutex<Connection>>,
    action: VaultAction,
    timespan: Option<TimeSpan>,
) -> Result<Response, Error> {
    let conn = database.lock().map_err(|_| Error::DbLock)?;
    let aggs = conn.action_aggregated(
        action,
        timespan.map_or(TimeSpan::Day.time_width(), |t| t.time_width()),
    )?;
    Ok(Response::ActionHistory(aggs))
}

fn handler_overall_volume(database: Arc<Mutex<Connection>>) -> Result<Response, Error> {
    let conn = database.lock().map_err(|_| Error::DbLock)?;
    let (btc_volume, unit_volume) = conn.overall_volume()?;
    Ok(Response::OverallVolume(OverallVolume {
        btc_volume,
        unit_volume,
    }))
}
