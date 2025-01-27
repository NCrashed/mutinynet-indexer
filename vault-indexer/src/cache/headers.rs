use core::unimplemented;
use std::collections::HashMap;

use bitcoin::{block::Header, BlockHash};
use sqlite::Connection;
use super::error::Error;
use crate::db::{DatabaseHeaders, DatabaseMeta, HeaderRecord};

pub struct HeadersCache {
    headers: HashMap<BlockHash, HeaderRecord>,
    best_tip: BlockHash,
    dirty: Vec<BlockHash>,
}

impl HeadersCache {
    /// Load all headers from database
    pub fn load(conn: &Connection) -> Result<Self, Error> {
        let mut headers = HashMap::new();
        conn.load_block_headers(|record| { headers.insert(record.header.block_hash(), record); });
        let best_tip = conn.get_main_tip()?;
        Ok(HeadersCache {
            headers,
            best_tip,
            dirty: vec![],
        })
    }

    /// Dump all dirty parts of cache to the database
    pub fn store(&mut self, conn: &Connection) -> Result<(), Error> {
        conn.set_best_tip(self.best_tip)?;
        for block_hash in self.dirty.iter() {
            let record = self.headers.get(block_hash).ok_or(Error::MissingHeader(*block_hash))?;
            conn.store_raw_header(record.header, record.height as i64, record.header.prev_blockhash, record.in_longest)?;
        }
        self.dirty = vec![];
        Ok(())
    }

    /// Checks if the given header chain extends the longest chain and saves metadata.
    /// 
    /// If the extended chain is not the longest, traverses back both the longest and current
    /// to find the common ancestor and compare the total work of the chains.
    /// 
    /// Note: we expect that the tip we update is already stored in the database
    pub fn update_longest_chain(&mut self, headers: &[Header]) -> Result<(), Error> {
        unimplemented!()
    }
}


    // fn update_longest_chain(&self, tip: BlockHash) -> Result<(), Error> {
    //     let tip_record = self.load_block_header(tip)?.ok_or(Error::MissingHeader(tip))?;
    //     let mut current_parent = tip_record.header.prev_blockhash;
    //     let mut chain = vec![tip_record];
    //     loop {
    //         let parent = self.load_block_header(current_parent)?.ok_or(Error::OrphanBlock(tip, current_parent))?;
    //         if parent.in_longest {
    //             for record in chain {
    //                 // HERE
                    
    //             }
    //             break; 
    //         } else {
    //             current_parent = parent.header.prev_blockhash;
    //             chain.push(parent);
    //         }
    //     }
    // }