use bitcoin::BlockHash;
use thiserror::Error;

use crate::Network;

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
    #[error("Database doesn't have a metadata row!")]
    NoMetadata,
    #[error("Database stored network {0} doesn't match the current network {1}. Recreate the database, please.")]
    DatabaseNetworkMismatch(Network, Network),
}
