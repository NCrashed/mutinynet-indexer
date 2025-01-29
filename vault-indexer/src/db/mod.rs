pub mod error;
pub mod header;
pub mod metadata;

use crate::Network;
pub use error::Error;
pub use header::*;
use log::*;
pub use metadata::*;
use rusqlite::Connection;
use std::path::Path;

pub fn initialize_db<P: AsRef<Path>>(
    filename: P,
    network: Network,
    start_height: u32,
) -> Result<Connection, Error> {
    trace!("Opening database {:?}", filename.as_ref());
    let mut connection = if filename.as_ref().to_str() == Some(":memory:") {
        Connection::open_in_memory().map_err(Error::Open)?
    } else {
        Connection::open(filename).map_err(Error::Open)?
    };

    trace!("Settings pragmas");
    // Keep temporary tables in memory to speed up copying of big blobs
    connection
        .pragma_update(None, "temp_store", "MEMORY")
        .map_err(Error::UpdatePragma)?;
    // WAL mode writes changes to a sequential write-ahead log, and then later synchronizes it back to the main database
    connection
        .pragma_update(None, "journal_mode", "WAL")
        .map_err(Error::UpdatePragma)?;
    // Give the OS responsibility for the IO to disk
    connection
        .pragma_update(None, "synchronous", "normal")
        .map_err(Error::UpdatePragma)?;
    // Set higher limit for journal
    connection
        .pragma_update(None, "journal_size_limit", "6144000")
        .map_err(Error::UpdatePragma)?;

    trace!("Creation of schema");
    let query = r#"
            CREATE TABLE IF NOT EXISTS headers(
                block_hash          BLOB NOT NULL PRIMARY KEY,
                height              INTEGER NOT NULL,
                prev_block_hash     BLOB NOT NULL,
                raw                 BLOB NOT NULL,
                in_longest          INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_headers_prev_hash ON headers(prev_block_hash);
            CREATE INDEX IF NOT EXISTS idx_headers_height ON headers(height);

            CREATE TABLE IF NOT EXISTS metadata(
                id INTEGER PRIMARY KEY CHECK (id = 0), -- The table has only one row
                network TEXT NOT NULL,
                tip_block_hash BLOB NOT NULL,
                scanned_height INTEGER NOT NULL
            );
        "#;
    connection
        .execute_batch(query)
        .map_err(Error::CreateSchema)?;

    // Store genesis hash to initiate main chain
    let genesis = network.genesis_header();
    if connection
        .load_block_header(genesis.block_hash())?
        .is_none()
    {
        connection.store_raw_headers(&[(genesis, 0i64, true)])?;
    }

    // Store initial metadata if missing
    if !connection.has_metadata()? {
        connection.store_metadata(&DbMetadata {
            network,
            tip_block_hash: genesis.block_hash(),
            scanned_height: start_height,
        })?;
    } else {
        let db_network = connection.get_network()?;
        if network != db_network {
            return Err(Error::DatabaseNetworkMismatch(db_network, network));
        }
    }

    Ok(connection)
}
