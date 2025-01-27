pub mod error;
pub mod header;
pub mod metadata;
pub mod tools;

use crate::Network;
use bitcoin::{hashes::Hash, BlockHash};
pub use error::Error;
pub use header::*;
use log::*;
pub use metadata::*;
use sqlite::Connection;
use std::path::Path;

pub fn initialize_db<P: AsRef<Path>>(filename: P, network: Network) -> Result<Connection, Error> {
    trace!("Opening database {:?}", filename.as_ref());
    let connection = sqlite::open(filename).map_err(Error::Open)?;

    trace!("Creation of schema");
    connection
        .execute(
            "
        CREATE TABLE IF NOT EXISTS headers(
            block_hash          BLOB NOT NULL PRIMARY KEY,
            height              INTEGER NOT NULL,
            prev_block_hash     BLOB NOT NULL,
            raw                 BLOB NOT NULL,
            in_longest          INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS idx_headers_prev_hash ON headers(prev_block_hash);
        CREATE INDEX IF NOT EXISTS idx_headers_height ON headers(height);

        CREATE TABLE IF NOT EXISTS metadata (
            id INTEGER PRIMARY KEY CHECK (id = 0), -- The table has only one row
            tip_block_hash BLOB NOT NULL
        );
    ",
        )
        .map_err(Error::CreateSchema)?;

    // Store genesis hash to initiate main chain
    let genesis = network.genesis_header();
    if connection
        .load_block_header(genesis.block_hash())?
        .is_none()
    {
        let zero_hash = BlockHash::from_byte_array([0; 32]);
        connection.store_raw_header(genesis, 0, zero_hash, true)?;
    }

    // Store initial metadata if missing
    if !connection.has_metadata()? {
        connection.store_metadata(&DbMetadata {
            tip_block_hash: genesis.block_hash(),
        })?;
    }

    Ok(connection)
}
