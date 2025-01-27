use core::{iter::Iterator, ops::FnMut, unimplemented};
use std::collections::HashMap;

use super::error::Error;
use crate::db::{DatabaseHeaders, DatabaseMeta, HeaderRecord};
use bitcoin::{block::Header, BlockHash, Work};
use sqlite::Connection;

pub struct HeadersCache {
    headers: HashMap<BlockHash, HeaderRecord>,
    best_tip: BlockHash,
    dirty: Vec<BlockHash>,
    orphans: HashMap<BlockHash, Header>,
}

impl HeadersCache {
    /// Load all headers from database
    pub fn load(conn: &Connection) -> Result<Self, Error> {
        let mut headers = HashMap::new();
        conn.load_block_headers(|record| {
            headers.insert(record.header.block_hash(), record);
        });
        let best_tip = conn.get_main_tip()?;
        Ok(HeadersCache {
            headers,
            best_tip,
            dirty: vec![],
            orphans: HashMap::new(),
        })
    }

    /// Dump all dirty parts of cache to the database
    pub fn store(&mut self, conn: &Connection) -> Result<(), Error> {
        conn.set_best_tip(self.best_tip)?;
        for block_hash in self.dirty.iter() {
            let record = self
                .headers
                .get(block_hash)
                .ok_or(Error::MissingHeader(*block_hash))?;
            conn.store_raw_header(
                record.header,
                record.height as i64,
                record.header.prev_blockhash,
                record.in_longest,
            )?;
        }
        self.dirty = vec![];
        Ok(())
    }

    /// Checks if the given header chain extends the longest chain and saves metadata.
    ///
    /// If the extended chain is not the longest, traverses back both the longest and current
    /// to find the common ancestor and compare the total work of the chains.
    pub fn update_longest_chain(&mut self, headers: &[Header]) -> Result<(), Error> {
        let first_header = if let Some(header) = headers.first() {
            header
        } else {
            return Ok(());
        };

        // Check if we updates the tip (the optmistic scenario)
        if self.best_tip == first_header.prev_blockhash {
            let tip_record = self
                .headers
                .get(&self.best_tip)
                .ok_or(Error::MissingHeader(self.best_tip))?.clone();
            // Add to the main chain all headers one by one
            for (i, header) in headers.iter().enumerate() {
                let block_hash = header.block_hash();
                if self.headers.contains_key(&block_hash) {
                    return Err(Error::AlreadyExisting(block_hash));
                }
                self.orphans.remove(&block_hash); // remove from orhpans if some headers arrived too early
                self.headers.insert(
                    block_hash,
                    HeaderRecord {
                        header: *header,
                        height: tip_record.height + (i as u32),
                        in_longest: true,
                    },
                );
                self.dirty.push(block_hash);
            }
            // Update the tip
            self.best_tip = headers.last().unwrap_or(first_header).block_hash();

            
        } else { // We are forking
            // Check if we have the header in the cache at all (or we stash them in separate orphans house for a while)
            let new_tip_hash = first_header.prev_blockhash;
            if !self.headers.contains_key(&new_tip_hash) {
                for header in headers {
                    self.orphans.insert(header.block_hash(), *header);
                    return Ok(());
                }
            }

            // Find the first shared ancestor of the current chain and the new one
            let mut new_chain = self.get_chain_until(first_header.prev_blockhash, |r| r.in_longest)?;
            new_chain.extend_tip(&headers)?;
            let main_chain = self.get_chain_until(self.best_tip, |r| r.header.block_hash() == new_chain.root_hash())?;
            if new_chain.total_work() > main_chain.total_work() { // Reorganization
                // TODO: inactivate index in vault transactions
                self.inactivate(&main_chain)?;
                self.store_active(new_chain)?;
            } else {  // Just store fork
                self.store_inactive(new_chain)?;
            }
        }
        Ok(())
    }

    /// Collect all headers from given tip to first block (including) that turns the predicate to true
    fn get_chain_until<F>(&self, tip: BlockHash, pred: F) -> Result<HeaderChain, Error> 
        where 
        F: Fn(&HeaderRecord) -> bool
    {
        unimplemented!()
    }

    /// Mark all the headers from given chain (except the root) as inactive
    fn inactivate(&mut self, chain: &HeaderChain) -> Result<(), Error> {
        unimplemented!()
    }

    /// Store headers from the chain as main chain sequence
    fn store_active(&mut self, chain: HeaderChain) -> Result<(), Error> {
        unimplemented!()
    }

    /// Store theaders from the chain as not main sequence
    fn store_inactive(&mut self, chain: HeaderChain) -> Result<(), Error> {
        unimplemented!()
    }
}

/// Encapsulates the list of headers, possible not from genesis
pub struct HeaderChain {}

impl HeaderChain {
    /// Add headers to the end of the chain, fails if the first header references
    /// other block than the tipe of the chain.
    pub fn extend_tip(&mut self, headers: &[Header]) -> Result<(), Error> {
        unimplemented!()
    } 

    pub fn total_work(&self) -> Work {
        unimplemented!()
    }

    /// The first block hash in the chain
    pub fn root_hash(&self) -> BlockHash {
        unimplemented!()
    }
}