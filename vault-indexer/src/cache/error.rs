use bitcoin::BlockHash;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Database: {0}")]
    Database(#[from] crate::db::Error),
    #[error("No header with hash: {0}")]
    MissingHeader(BlockHash),
}