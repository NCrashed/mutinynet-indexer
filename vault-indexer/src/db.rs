use sqlite::Connection;
use std::path::Path;
use thiserror::Error;
use log::*;

#[derive(Debug, Error)]
pub enum Error {
    #[error("Failed to open database: {0}")]
    Open(sqlite::Error),
    #[error("Failed to create tables: {0}")]
    CreateSchema(sqlite::Error),
}

/// Wraps SQlite connection and caches prepared statements
pub struct Database {
    connection: Connection,
}

impl Database {
    pub fn new<P: AsRef<Path>>(filename: P) -> Result<Self, Error> {
        trace!("Opening database {:?}", filename.as_ref());
        let connection = sqlite::open(filename).map_err(Error::Open)?;
        let mut db = Database {
            connection
        };
        trace!("Creation of schema and prepared statements");
        db.initialize()?;
        Ok(db)
    }

    fn initialize(&mut self) -> Result<(), Error> {
        self.connection.execute("
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
                tip_block_hash TEXT NOT NULL
            );
        ").map_err(Error::CreateSchema)?;

        Ok(())
    }
}
