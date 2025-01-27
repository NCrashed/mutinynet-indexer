use bitcoin::BlockHash;
use thiserror::Error;

#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    #[error("Database: {0}")]
    Database(#[from] crate::db::Error),
    #[error("No header with hash: {0}")]
    MissingHeader(BlockHash),
    #[error("We already has header (possible a loop in chain detected): {0}")]
    AlreadyExisting(BlockHash),
    #[error("Cannot extend chain tip {0} with the header {1}, parent doesn't match")]
    ChainMismatchTip(BlockHash, BlockHash),
    #[error("Cannot extend chain root {0} with the header {1}, parent doesn't match")]
    ChainMismatchRoot(BlockHash, BlockHash),
}