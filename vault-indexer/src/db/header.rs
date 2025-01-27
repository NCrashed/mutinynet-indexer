use super::error::Error;
use super::tools::*;
use bitcoin::{
    block::Header,
    consensus::{Decodable, Encodable},
    hashes::Hash,
    BlockHash,
};
use sqlite::{Connection, State, Value};
use core::ops::FnMut;
use std::io::Cursor;

#[derive(Debug, Clone)]
pub struct HeaderRecord {
    pub header: Header,
    pub height: u32,
    pub in_longest: bool,
}

pub trait DatabaseHeaders {
    /// Find stored header record in the database
    fn load_block_header(&self, block_id: BlockHash) -> Result<Option<HeaderRecord>, Error>;

    /// Iterate all stored headers and call a closure for them
    fn load_block_headers<F>(&self, body: F) -> Result<(), Error>
        where F: FnMut(HeaderRecord) -> ();

    /// Stores the header in the database, doesn't mark it as longest chain, but checks that we have the parent in place.
    fn store_block_header(&self, header: Header) -> Result<(), Error> {
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
        // self.update_longest_chain(header.block_hash())
    }

    /// Stores the header without checking that we have the parent in the database
    fn store_raw_header(
        &self,
        header: Header,
        height: i64,
        prev_hash: BlockHash,
        in_longest: bool,
    ) -> Result<(), Error>;
}

impl DatabaseHeaders for Connection {
    /// Find stored header record in the database
    fn load_block_header(&self, block_id: BlockHash) -> Result<Option<HeaderRecord>, Error> {
        let query =
            "SELECT height, raw, in_longest FROM headers WHERE block_hash = :block_hash LIMIT 1";
        let mut statement = self.prepare(query).map_err(Error::PrepareQuery)?;
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

    fn load_block_headers<F>(&self, mut body: F) -> Result<(), Error>
            where F: FnMut(HeaderRecord) -> () 
    {
        let query =
            "SELECT height, raw, in_longest FROM headers";
        let mut statement = self.prepare(query).map_err(Error::PrepareQuery)?;

        while let State::Row = statement.next().map_err(Error::QueryNextRow)? {
            let height = statement.read_field::<i64>("height")?;
            let raw_header = statement.read_field::<Vec<u8>>("raw")?;
            let in_longest = statement.read_field::<i64>("in_longest")?;

            let mut header_cursor = Cursor::new(raw_header);
            let header =
                Header::consensus_decode(&mut header_cursor).map_err(Error::DecodeHeader)?;
            let record = HeaderRecord {
                header,
                height: height as u32,
                in_longest: in_longest != 0,
            };
            body(record)
        }
        Ok(())
    }

    fn store_raw_header(
        &self,
        header: Header,
        height: i64,
        prev_hash: BlockHash,
        in_longest: bool,
    ) -> Result<(), Error> {
        let query =
            "INSERT INTO headers VALUES(:block_hash, :height, :prev_block_hash, :raw, :in_longest)
                ON CONFLICT(block_hash) DO UPDATE SET in_longest=excluded.in_longest";

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
}
