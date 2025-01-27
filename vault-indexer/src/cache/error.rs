use bitcoin::BlockHash;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Database: {0}")]
    Database(#[from] crate::db::Error),
    #[error("No header with hash: {0}")]
    MissingHeader(BlockHash),
    #[error("We already has header (possible a loop in chain detected): {0}")]
    AlreadyExisting(BlockHash),
}