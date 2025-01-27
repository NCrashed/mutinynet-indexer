use bitcoin::BlockHash;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Failed to open database: {0}")]
    Open(sqlite::Error),
    #[error("Failed to create tables: {0}")]
    CreateSchema(sqlite::Error),
    #[error("Failed to prepare SQLite query: {0}")]
    PrepareQuery(sqlite::Error),
    #[error("Failed to bind arguments to query: {0}")]
    BindQuery(sqlite::Error),
    #[error("Failed to extract next row from query: {0}")]
    QueryNextRow(sqlite::Error),
    #[error("Failed to read row field {0}: {1}")]
    ReadField(String, sqlite::Error),
    #[error("Cannot decode Bitcoin header: {0}")]
    DecodeHeader(bitcoin::consensus::encode::Error),
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
}
