use bitcoin::{
    block::Header,
    consensus::{Decodable, Encodable},
    hashes::Hash,
    BlockHash,
};
use core::convert::TryInto;
use log::*;
use sqlite::{Bindable, Connection, ReadableWithIndex, State, Statement, Value};
use std::{io::Cursor, path::Path};
use thiserror::Error;

use crate::Network;

#[derive(Debug, Error)]
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
    #[error("We encountered orphan block header {0} with no parent {1}")]
    OrphanBlock(BlockHash, BlockHash),
    #[error("Query '{0}' should be executed by single step")]
    ShouldExecuteOneRow(String),
    #[error("Failed to parse tip block hash (expected 32 bytes): {0:x?}")]
    TipBlockHashWrongSize(Vec<u8>),
    #[error("Database doesn't have a metadata row!")]
    NoMetadata,
}

/// Wraps SQlite connection and caches prepared statements
pub struct Database {
    connection: Connection,
}

#[derive(Debug, Clone)]
pub struct HeaderRecord {
    pub header: Header,
    pub height: u32,
    pub in_longest: bool,
}

#[derive(Debug, Clone)]
struct Metadata {
    tip_block_hash: BlockHash,
}

impl Database {
    pub fn new<P: AsRef<Path>>(filename: P, network: Network) -> Result<Self, Error> {
        trace!("Opening database {:?}", filename.as_ref());
        let connection = sqlite::open(filename).map_err(Error::Open)?;
        let mut db = Database { connection };
        trace!("Creation of schema");
        db.initialize(network)?;
        Ok(db)
    }

    fn initialize(&mut self, network: Network) -> Result<(), Error> {
        self.connection
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
        if self.load_block_header(genesis.block_hash())?.is_none() {
            let zero_hash = BlockHash::from_byte_array([0; 32]);
            self.store_raw_header(genesis, 0, zero_hash, true)?;
        }

        // Store initial metadata if missing
        if !self.has_metadata()? {
            self.store_metadata(&Metadata {
                tip_block_hash: genesis.block_hash(),
            })?;
        }
        Ok(())
    }

    /// Find stored header record in the database
    pub fn load_block_header(&self, block_id: BlockHash) -> Result<Option<HeaderRecord>, Error> {
        let query =
            "SELECT height, raw, in_longest FROM headers WHERE block_hash = :block_hash LIMIT 1";
        let mut statement = self
            .connection
            .prepare(query)
            .map_err(Error::PrepareQuery)?;
        statement
            .bind((":block_hash", &block_id.as_raw_hash().as_byte_array()[..]))
            .map_err(Error::BindQuery)?;

        if let State::Row = statement.next().map_err(Error::QueryNextRow)? {
            let height = statement.read_field::<i64>("height")?;
            let raw_header = statement.read_field::<Vec<u8>>("raw")?;
            let in_longest = statement.read_field::<i64>("in_longest")?;

            let mut header_cursor = Cursor::new(raw_header);
            let header =
                Header::consensus_decode(&mut header_cursor).map_err(Error::DecodeHeader)?;
            Ok(Some(HeaderRecord {
                header,
                height: height as u32,
                in_longest: in_longest != 0,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn store_block_header(&self, header: Header) -> Result<(), Error> {
        // TODO: process orphan headers (perhaps in separate table)
        let parent_header =
            self.load_block_header(header.prev_blockhash)?
                .ok_or(Error::OrphanBlock(
                    header.block_hash(),
                    header.prev_blockhash,
                ))?;

        self.store_raw_header(
            header,
            (parent_header.height + 1) as i64,
            parent_header.header.block_hash(),
            false,
        )
    }

    fn store_raw_header(
        &self,
        header: Header,
        height: i64,
        prev_hash: BlockHash,
        in_longest: bool,
    ) -> Result<(), Error> {
        let query =
            "INSERT INTO headers VALUES(:block_hash, :height, :prev_block_hash, :raw, :in_longest)";

        const HEADER_SIZE: usize = 80;
        let mut raw = vec![0u8; HEADER_SIZE];
        header
            .consensus_encode(&mut Cursor::new(&mut raw))
            .map_err(Error::EncodeHeader)?;

        self.single_execute::<_, (_, Value)>(
            "insert header",
            query,
            [
                (
                    ":block_hash",
                    header.block_hash().as_raw_hash().as_byte_array()[..].into(),
                ),
                (":height", height.into()),
                (
                    ":prev_block_hash",
                    prev_hash.as_raw_hash().as_byte_array()[..].into(),
                ),
                (":raw", raw.into()),
                (":in_longest", (if in_longest { 1 } else { 0 }).into()),
            ],
        )
    }

    /// Get current best main chain
    pub fn get_main_tip(&self) -> Result<BlockHash, Error> {
        let meta = self.load_metada()?;
        Ok(meta.tip_block_hash)
    }

    /// Returns true if we have single row in metadata table
    fn has_metadata(&self) -> Result<bool, Error> {
        let query = "SELECT count(id) as count FROM metadata";
        let mut statement = self
            .connection
            .prepare(query)
            .map_err(Error::PrepareQuery)?;

        if let State::Row = statement.next().map_err(Error::QueryNextRow)? {
            let count = statement.read_field::<i64>("count")?;
            Ok(count != 0)
        } else {
            Ok(false)
        }
    }

    // Update metadata table
    fn store_metadata(&self, meta: &Metadata) -> Result<(), Error> {
        let query = "INSERT INTO metadata VALUES(0, :tip_block_hash)
                ON CONFLICT(id) DO UPDATE SET tip_block_hash=excluded.tip_block_hash";
        self.single_execute::<_, (_, Value)>(
            "upsert metadata",
            query,
            [(
                ":tip_block_hash",
                meta.tip_block_hash.as_raw_hash().as_byte_array()[..].into(),
            )],
        )
    }

    // Fetch all metadata from table
    fn load_metada(&self) -> Result<Metadata, Error> {
        let query = "SELECT * FROM metadata LIMIT 1";
        let mut statement = self
            .connection
            .prepare(query)
            .map_err(Error::PrepareQuery)?;

        if let State::Row = statement.next().map_err(Error::QueryNextRow)? {
            let tip_block_hash_bytes = statement.read_field::<Vec<u8>>("tip_block_hash")?;
            let tip_block_hash_sized = tip_block_hash_bytes
                .clone()
                .try_into()
                .map_err(|_| Error::TipBlockHashWrongSize(tip_block_hash_bytes))?;
            let tip_block_hash = BlockHash::from_byte_array(tip_block_hash_sized);
            Ok(Metadata { tip_block_hash })
        } else {
            Err(Error::NoMetadata)
        }
    }

    // Helper to execute inserts, updates and deletes (etc) that should be done in one step with no return results
    fn single_execute<T, U>(&self, tag: &str, query: &str, binds: T) -> Result<(), Error>
    where
        T: IntoIterator<Item = U>,
        U: Bindable,
    {
        let mut statement = self
            .connection
            .prepare(query)
            .map_err(Error::PrepareQuery)?;

        statement.bind_iter(binds).map_err(Error::BindQuery)?;

        if let State::Done = statement.next().map_err(Error::QueryNextRow)? {
            Ok(())
        } else {
            Err(Error::ShouldExecuteOneRow(tag.to_owned()))
        }
    }
}

// Helper trait to simplify reading fields from statement and use self syntax
trait ReadField {
    fn read_field<T: ReadableWithIndex>(&self, name: &str) -> Result<T, Error>;
}

impl<'c> ReadField for Statement<'c> {
    fn read_field<T: ReadableWithIndex>(&self, name: &str) -> Result<T, Error> {
        let val = self
            .read::<T, _>(name)
            .map_err(|e| Error::ReadField(name.to_owned(), e))?;
        Ok(val)
    }
}
