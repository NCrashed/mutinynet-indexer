use bitcoin::{BlockHash, Txid};
use thiserror::Error;

use crate::{vault::AssumeCustodyErr, Network};

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Failed to open database: {0}")]
    Open(rusqlite::Error),
    #[error("Update SQlite pragma failure: {0}")]
    UpdatePragma(rusqlite::Error),
    #[error("Failed to create tables: {0}")]
    CreateSchema(rusqlite::Error),
    #[error("Failed to prepare SQLite query: {0}")]
    PrepareQuery(rusqlite::Error),
    #[error("Failed execution of query: {0}")]
    ExecuteQuery(rusqlite::Error),
    #[error("Failed fetching next row of query: {0}")]
    FetchRow(rusqlite::Error),
    #[error("Failed to start transaction: {0}")]
    StartTransaction(rusqlite::Error),
    #[error("Failed to commit transaction: {0}")]
    CommitTransaction(rusqlite::Error),
    #[error("Cannot encode Bitcoin header: {0}")]
    EncodeHeader(bitcoin::io::Error),
    #[error("Missing header for block: {0}")]
    MissingHeader(BlockHash),
    #[error("We encountered orphan block header {0} with no parent {1}")]
    OrphanBlock(BlockHash, BlockHash),
    #[error("Query '{0}' should be executed by single step")]
    ShouldExecuteOneRow(String),
    #[error("Failed to parse tip block hash (expected 32 bytes): {0:x?}")]
    TipBlockHashWrongSize(Vec<u8>),
    #[error("Failed to parse transaction hash (expected 32 bytes): {0:x?}")]
    TxidWrongSize(Vec<u8>),
    #[error("Failed to decode fixed sized (len {0}) array from the vector: {1:x?}")]
    ArrayWrongSize(usize, Vec<u8>),
    #[error("Database doesn't have a metadata row!")]
    NoMetadata,
    #[error("Database stored network {0} doesn't match the current network {1}. Recreate the database, please.")]
    DatabaseNetworkMismatch(Network, Network),
    #[error("Cannot encode bitcoin transaction: {0}")]
    EncodeBitcoinTransaction(bitcoin::io::Error),
    #[error("Vault transaction doesn't have inputs, txid: {0}")]
    VaultTxNoInputs(Txid),
    #[error("Cannot find vault for given transaction {0}")]
    UnknownVaultTx(Txid),
    #[error("Cannot find vault with given open transcation {0}")]
    UnknownVaultId(Txid),
    #[error("Cannot assume BTC volume: {0}")]
    AssumeBtcVolume(#[from] AssumeCustodyErr),
}
