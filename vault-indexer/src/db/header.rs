use super::error::Error;
use bitcoin::{
    block::Header,
    consensus::{Decodable, Encodable},
    hashes::Hash,
    BlockHash,
};
use core::ops::FnMut;
use rusqlite::{
    named_params, params_from_iter,
    types::{Type, Value},
    Connection,
};
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
    where
        F: FnMut(HeaderRecord);

    /// Stores the header in the database, doesn't mark it as longest chain, but checks that we have the parent in place.
    fn store_block_header(&mut self, header: Header) -> Result<(), Error> {
        let parent_header =
            self.load_block_header(header.prev_blockhash)?
                .ok_or(Error::OrphanBlock(
                    header.block_hash(),
                    header.prev_blockhash,
                ))?;

        self.store_raw_headers(&[(header, (parent_header.height + 1) as i64, false)])
    }

    /// Stores the header without checking that we have the parent in the database
    fn store_raw_headers(&mut self, headers: &[(Header, i64, bool)]) -> Result<(), Error>;
}

impl DatabaseHeaders for Connection {
    /// Find stored header record in the database
    fn load_block_header(&self, block_hash: BlockHash) -> Result<Option<HeaderRecord>, Error> {
        let query =
            "SELECT height, raw, in_longest FROM headers WHERE block_hash = :block_hash LIMIT 1";
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let block_hash_bytes = block_hash.as_raw_hash().as_byte_array();
        let mut result = statement
            .query_map(named_params! { ":block_hash": block_hash_bytes }, |row| {
                let height = row.get::<_, i64>(0)?;
                let raw_header = row.get::<_, Vec<u8>>(1)?;
                let in_longest = row.get::<_, i64>(2)?;

                let mut header_cursor = Cursor::new(raw_header);
                let header = Header::consensus_decode(&mut header_cursor).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(1, Type::Blob, Box::new(e))
                })?;
                Ok(HeaderRecord {
                    header,
                    height: height as u32,
                    in_longest: in_longest != 0,
                })
            })
            .map_err(Error::ExecuteQuery)?;

        if let Some(record) = result.next() {
            Ok(Some(record.map_err(Error::FetchRow)?))
        } else {
            Ok(None)
        }
    }

    fn load_block_headers<F>(&self, mut body: F) -> Result<(), Error>
    where
        F: FnMut(HeaderRecord),
    {
        let query = "SELECT height, raw, in_longest FROM headers";
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let result = statement
            .query_map([], |row| {
                let height = row.get::<_, i64>(0)?;
                let raw_header = row.get::<_, Vec<u8>>(1)?;
                let in_longest = row.get::<_, i64>(2)?;

                let mut header_cursor = Cursor::new(raw_header);
                let header = Header::consensus_decode(&mut header_cursor).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(1, Type::Blob, Box::new(e))
                })?;
                let record = HeaderRecord {
                    header,
                    height: height as u32,
                    in_longest: in_longest != 0,
                };
                Ok(record)
            })
            .map_err(Error::ExecuteQuery)?;

        for record in result {
            body(record.map_err(Error::FetchRow)?)
        }
        Ok(())
    }

    fn store_raw_headers(&mut self, headers: &[(Header, i64, bool)]) -> Result<(), Error> {
        // Size for one batch, tuned manually
        const BATCH_SIZE: usize = 500;

        // The shared transaction for all batches
        let tx = self.transaction().map_err(Error::StartTransaction)?;

        let mut start = 0;
        while start < headers.len() {
            let end = (start + BATCH_SIZE).min(headers.len());
            let batch = &headers[start..end];

            // Start making the batched SQL query
            let mut sql = String::from(
                r#"
                INSERT INTO headers (block_hash, height, prev_block_hash, raw, in_longest)
                VALUES
                "#,
            );

            // Collecting N parts "(?, ?, ?, ?, ?)" batch.len() times
            let mut values_placeholders = Vec::with_capacity(batch.len());
            for _ in batch {
                values_placeholders.push("(?, ?, ?, ?, ?)".to_string());
            }
            sql.push_str(&values_placeholders.join(", "));

            // Finish query with on conflict part
            sql.push_str(
                r#"
                ON CONFLICT(block_hash)
                    DO UPDATE SET
                        in_longest = excluded.in_longest
                "#,
            );

            let mut stmt = tx.prepare(&sql).map_err(Error::PrepareQuery)?;

            // Collect all parameters
            let mut params = Vec::with_capacity(batch.len() * 5); // 5 fields per record
            for (header, height, in_longest) in batch {
                // Encoding header
                const HEADER_SIZE: usize = 80;
                let mut raw = vec![0u8; HEADER_SIZE];
                header
                    .consensus_encode(&mut Cursor::new(&mut raw))
                    .map_err(Error::EncodeHeader)?;

                let prev_hash = header.prev_blockhash;

                // Fill in the same order as (?,?,?,?,?)
                params.push(Value::Blob(
                    header.block_hash().as_raw_hash().as_byte_array().to_vec(),
                ));
                params.push(Value::Integer(*height));
                params.push(Value::Blob(
                    prev_hash.as_raw_hash().as_byte_array().to_vec(),
                ));
                params.push(Value::Blob(raw));
                params.push(Value::Integer(if *in_longest { 1 } else { 0 }));
            }

            // Bulk insert here
            stmt.execute(params_from_iter(params))
                .map_err(Error::ExecuteQuery)?;
            start = end;
        }

        // Finish this mayhem
        tx.commit().map_err(Error::CommitTransaction)?;
        Ok(())
    }
}
