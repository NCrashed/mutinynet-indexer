use bitcoin::{
    block::Header,
    consensus::{Decodable, Encodable},
    hashes::Hash,
    network, BlockHash,
};
use log::*;
use sqlite::{Connection, ReadableWithIndex, State, Statement, Value};
use core::unimplemented;
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
                tip_block_hash TEXT NOT NULL
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
        let mut statement = self
            .connection
            .prepare(query)
            .map_err(Error::PrepareQuery)?;

        const HEADER_SIZE: usize = 80;
        let mut raw = vec![0u8; HEADER_SIZE];
        header
            .consensus_encode(&mut Cursor::new(&mut raw))
            .map_err(Error::EncodeHeader)?;

        statement
            .bind_iter::<_, (_, Value)>([
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
            ])
            .map_err(Error::BindQuery)?;

        if let State::Done = statement.next().map_err(Error::QueryNextRow)? {
            Ok(())
        } else {
            Err(Error::ShouldExecuteOneRow("insert header".to_owned()))
        }
    }

    /// Get current best main chain
    pub fn get_main_tip(&self) -> Result<BlockHash, Error> {
        unimplemented!()
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
