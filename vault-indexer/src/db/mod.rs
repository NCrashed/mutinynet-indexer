pub mod error;
pub mod header;
pub mod loaders;
pub mod metadata;
pub mod vault;

use crate::db::vault::DatabaseVault;
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
    rescan: bool,
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
                block_hash          BLOB(32) NOT NULL PRIMARY KEY,
                height              INTEGER NOT NULL,
                prev_block_hash     BLOB(32) NOT NULL,
                raw                 BLOB NOT NULL,
                in_longest          INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_headers_prev_hash ON headers(prev_block_hash);
            CREATE INDEX IF NOT EXISTS idx_headers_height ON headers(height);

            CREATE TABLE IF NOT EXISTS metadata(
                id INTEGER PRIMARY KEY CHECK (id = 0), -- The table has only one row
                network TEXT NOT NULL,
                tip_block_hash BLOB(32) NOT NULL,
                scanned_height INTEGER NOT NULL
            );

            CREATE TABLE IF NOT EXISTS vaults(
                open_txid           BLOB(32) NOT NULL PRIMARY KEY, -- Assume that we cannot have two vaults operations in single tx
                output              INTEGER NOT NULL,
                balance             INTEGER NOT NULL,
                oracle_price        INTEGER NOT NULL,
                oracle_timestamp    INTEGER NOT NULL,
                liquidation_price   INTEGER,
                liquidation_hash    BLOB(32),
                custody             INTEGER NOT NULL,
                last_tx             BLOB(32) NOT NULL
            );

            CREATE TABLE IF NOT EXISTS transactions(
                txid                BLOB(32) NOT NULL PRIMARY KEY, -- Assume that we cannot have two vaults operations in single tx
                output              INTEGER NOT NULL,
                block_pos           INTEGER NOT NULL, -- Ordering in block is important for state update order
                vault_txid          BLOB(32) NOT NULL,
                -- Fields extracted from transaction
                version             TEXT NOT NULL,
                action              TEXT NOT NULL,
                balance             INTEGER NOT NULL,
                oracle_price        INTEGER NOT NULL,
                oracle_timestamp    INTEGER NOT NULL,
                liquidation_price   INTEGER,
                liquidation_hash    BLOB(32),
                -- Metainfo 
                block_hash          BLOB(32) NOT NULL,
                height              INTEGER NOT NULL,
                in_longest          INTEGER NOT NULL,
                raw_tx              BLOB NOT NULL,
                btc_custody         INTEGER NOT NULL,
                unit_volume         INTEGER NOT NULL, -- Assume that balance delta is units volume
                btc_volume          INTEGER NOT NULL, -- Assume that BTC volume is sum of other outputs minus change (non tap outputs) and custody counts only for opening transaction
                prev_tx             BLOB(32),

                FOREIGN KEY (vault_txid) REFERENCES vaults(open_txid),
                FOREIGN KEY (block_hash) REFERENCES headers(block_hash),
                FOREIGN KEY (prev_tx) REFERENCES transactions(txid)
            );

            CREATE INDEX IF NOT EXISTS idx_transactions_vault_id ON transactions(vault_txid);
            CREATE INDEX IF NOT EXISTS idx_transactions_action ON transactions(action);
            CREATE INDEX IF NOT EXISTS idx_transactions_height ON transactions(height);
            CREATE INDEX IF NOT EXISTS idx_transactions_height_in_longest ON transactions(height, in_longest);
            CREATE INDEX IF NOT EXISTS idx_transactions_block_hash ON transactions(block_hash);
            CREATE INDEX IF NOT EXISTS idx_transactions_in_longest ON transactions(in_longest);
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

    if rescan {
        connection.drop_vaults()?;
        connection.set_scanned_height(start_height)?;
    }

    Ok(connection)
}
