use super::error::Error;
use crate::db::{DatabaseHeaders, DatabaseMeta, HeaderRecord};
use bitcoin::{block::Header, hashes::Hash, BlockHash, Work};
use core::{fmt::Display, iter::Iterator};
use log::*;
use sqlite::Connection;
use std::collections::HashMap;

pub struct HeadersCache {
    headers: HashMap<BlockHash, HeaderRecord>,
    best_tip: BlockHash,
    height: u32,
    main_chain: Vec<BlockHash>,
    dirty: Vec<BlockHash>,
    orphans: HashMap<BlockHash, Header>,
}

impl HeadersCache {
    /// Load all headers from database
    pub fn load(conn: &Connection) -> Result<Self, Error> {
        let mut headers = HashMap::new();
        conn.load_block_headers(|record| {
            headers.insert(record.header.block_hash(), record);
        })?;
        let best_tip = conn.get_main_tip()?;
        let mut cache = HeadersCache {
            headers,
            best_tip,
            height: 0,
            main_chain: vec![],
            dirty: vec![],
            orphans: HashMap::new(),
        };
        cache.fill_main_chain()?;
        Ok(cache)
    }

    fn fill_main_chain(&mut self) -> Result<(), Error> {
        let tip_record = self.get_header(self.best_tip)?.clone();
        let empty_hash = BlockHash::from_byte_array([0u8; 32]);
        self.height = tip_record.height;
        self.main_chain.resize(tip_record.height as usize + 1, empty_hash);
        
        let mut current_record = tip_record;
        loop {
            let curr_height = current_record.height;
            self.main_chain[curr_height as usize] = current_record.header.block_hash();
            if current_record.height == 0 {
                break;
            }
            current_record = self.get_header(current_record.header.prev_blockhash)?.clone();
            assert_eq!(curr_height, current_record.height+1);
        }
        Ok(())
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

    /// Query the header in the cache. Doesn't gurantee that the header in the main chain
    pub fn get_header(&self, hash: BlockHash) -> Result<&HeaderRecord, Error> {
        self.headers.get(&hash).ok_or(Error::MissingHeader(hash))
    }

    /// Get the block hash that is in main chain in the given height
    pub fn get_blockhash_at(&self, height: u32) -> Option<BlockHash> {
        self.main_chain.get(height as usize).cloned()
    }

    /// Get the Bitcoin core locator of current main chain.
    ///
    /// The locator is list of hashes that is sampled across the chain
    /// and helps to identify which chain extension we want to ask from
    /// remote peer.
    pub fn get_locator_main_chain(&self) -> Result<Vec<BlockHash>, Error> {
        let mut hashes = vec![];
        let heights = get_locator_heights(self.height);
        for i in heights {
            let hash = self
                .get_blockhash_at(i)
                .ok_or(Error::MissingHeaderHeight(i))?;
            hashes.push(hash);
        }
        Ok(hashes)
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
            trace!("Extending te current main chain");

            let tip_record = self
                .headers
                .get(&self.best_tip)
                .ok_or(Error::MissingHeader(self.best_tip))?
                .clone();
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
        } else {
            trace!("Fork detected");

            // Check if we have the header in the cache at all (or we stash them in separate orphans house for a while)
            let new_tip_hash = first_header.prev_blockhash;
            if !self.headers.contains_key(&new_tip_hash) {
                trace!("The new chain is orphan");
                for header in headers {
                    self.orphans.insert(header.block_hash(), *header);
                    return Ok(());
                }
            }

            // Find the first shared ancestor of the current chain and the new one
            trace!("Finding the mutual ancestor");
            let mut new_chain =
                self.get_chain_until(first_header.prev_blockhash, |r| r.in_longest)?;
            trace!("Extending the new chain with arrived headers");
            new_chain.extend_tip(&headers)?;
            trace!("Getting the main chain until has the mutual ancestor");
            let main_chain = self.get_chain_until(self.best_tip, |r| {
                r.header.block_hash() == new_chain.root_hash()
            })?;
            if new_chain.total_work() > main_chain.total_work() {
                trace!("Total work of new chain is greater, inactivating main chain");
                // Reorganization
                // TODO: inactivate index in vault transactions
                self.inactivate(&main_chain)?;
                trace!("Activating new chain");
                self.store_active(new_chain)?;
            } else {
                trace!("Total work of current active chain is greater, storing fork");
                // Just store fork
                self.store_inactive(new_chain)?;
            }
        }

        // Now we can retry orphans after new blocks arrived
        self.process_orphans()?;
        Ok(())
    }

    /// Collect all headers from given tip to first block (including) that turns the predicate to true
    fn get_chain_until<F>(&self, tip: BlockHash, pred: F) -> Result<HeaderChain, Error>
    where
        F: Fn(&HeaderRecord) -> bool,
    {
        let mut current_record = self.headers.get(&tip).ok_or(Error::MissingHeader(tip))?;

        let mut chain = HeaderChain::new(current_record.header);
        if pred(current_record) {
            return Ok(chain);
        }

        trace!("Made a starting chain {chain}");
        loop {
            let next_hash = current_record.header.prev_blockhash;
            current_record = self
                .headers
                .get(&next_hash)
                .ok_or(Error::MissingHeader(next_hash))?;
            trace!("Testing next record: {current_record:?}");

            if pred(current_record) {
                break;
            }

            chain.push_root(current_record.header)?;
        }
        Ok(chain)
    }

    /// Mark all the headers from given chain (except the root) as inactive
    fn inactivate(&mut self, chain: &HeaderChain) -> Result<(), Error> {
        for header in chain.headers().skip(1) {
            let hash = header.block_hash();
            let header_record = self
                .headers
                .get_mut(&hash)
                .ok_or(Error::MissingHeader(hash))?;
            header_record.in_longest = false;
            self.dirty.push(hash);
        }
        let root_record = self.get_header(chain.root_hash())?.clone();
        self.best_tip = root_record.header.block_hash();
        self.height = root_record.height;
        self.main_chain.truncate(self.height as usize);
        Ok(())
    }

    /// Store headers from the chain as main chain sequence
    fn store_active(&mut self, chain: HeaderChain) -> Result<(), Error> {
        trace!("Activation of chain: {chain}");
        let root_hash = chain.root_hash();
        let mut prev_record = self
            .headers
            .get(&root_hash)
            .ok_or(Error::MissingHeader(root_hash))?
            .clone();
        let start_height = prev_record.height;
        let new_height = start_height + chain.len() as u32 - 1;
        let zero_hash = BlockHash::from_byte_array([0u8; 32]);
        self.main_chain.resize(new_height as usize + 1, zero_hash);
        
        for header in chain.headers() {
            let hash = header.block_hash();
            if self.headers.contains_key(&hash) {
                // activate
                let header_record = self
                    .headers
                    .get_mut(&hash)
                    .ok_or(Error::MissingHeader(hash))?;
                header_record.in_longest = true;
                self.main_chain[header_record.height as usize] = hash;
                self.dirty.push(hash);
                prev_record = header_record.clone();
            } else {
                // insert new
                let height = prev_record.height + 1;
                let new_record = HeaderRecord {
                    header,
                    height,
                    in_longest: true,
                };
                self.headers.insert(hash, new_record.clone());
                self.main_chain[height as usize] = hash;
                self.orphans.remove(&hash);
                self.dirty.push(hash);
                prev_record = new_record;
            }
        }

        trace!("Make the best tip as: {}", chain.tip_hash());
        self.best_tip = chain.tip_hash();
        self.height = new_height;

        Ok(())
    }

    /// Store theaders from the chain as not main sequence
    fn store_inactive(&mut self, chain: HeaderChain) -> Result<(), Error> {
        let root_hash = chain.root_hash();
        let mut prev_record = self
            .headers
            .get(&root_hash)
            .ok_or(Error::MissingHeader(root_hash))?
            .clone();
        for header in chain.headers() {
            let hash = header.block_hash();
            if !self.headers.contains_key(&hash) {
                let new_record = HeaderRecord {
                    header,
                    height: prev_record.height + 1,
                    in_longest: false,
                };
                self.headers.insert(hash, new_record.clone());
                self.orphans.remove(&hash);
                self.dirty.push(hash);
                prev_record = new_record;
            }
        }
        Ok(())
    }

    /// Retry orphans headers and try to add them to the main graph
    fn process_orphans(&mut self) -> Result<(), Error> {
        let mut removed_orphans: Vec<BlockHash> = vec![];
        let mut adopted_oprhans = vec![];
        for orphan in self.orphans.values().cloned() {
            if self.headers.contains_key(&orphan.prev_blockhash) {
                adopted_oprhans.push(orphan);
                removed_orphans.push(orphan.block_hash());
            }
        }
        for orphan in adopted_oprhans {
            self.update_longest_chain(&[orphan])?;
        }
        for orphan in removed_orphans {
            self.orphans.remove(&orphan);
        }
        Ok(())
    }
}

/// Encapsulates the list of headers, possible not from genesis. The idea behind that we have
/// a structure that effeciently grows in both directions and always has at least 1 element.
#[derive(Debug)]
pub struct HeaderChain {
    root: Header,           // genesis of the chain (not the genesis of the whole blockchain)
    trunk_rev: Vec<Header>, // Headers that are growing reverse, last element is the second header after root
    trunk_for: Vec<Header>, // Headers that are growing forward, last element is the tip of the chain
}

impl Display for HeaderChain {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Chain: ")?;
        for hash in self.headers().map(|h| h.block_hash()) {
            write!(f, "{hash},")?;
        }
        Ok(())
    }
}

impl HeaderChain {
    pub fn new(root: Header) -> Self {
        HeaderChain {
            root,
            trunk_rev: vec![],
            trunk_for: vec![],
        }
    }

    pub fn len(&self) -> usize {
        1 + self.trunk_rev.len() + self.trunk_for.len()
    }

    /// Add headers to the end of the chain, fails if the first header references
    /// other block than the tipe of the chain.
    pub fn extend_tip(&mut self, headers: &[Header]) -> Result<(), Error> {
        if let Some(header) = headers.first() {
            let current_tip = self.tip_hash();
            if current_tip == header.prev_blockhash {
                self.trunk_for.extend_from_slice(&headers);
                Ok(())
            } else {
                Err(Error::ChainMismatchTip(current_tip, header.block_hash()))
            }
        } else {
            Ok(())
        }
    }

    /// Add a header to the begining of the chain, fails if the first header references
    /// other block than the tipe of the chain.
    pub fn push_root(&mut self, header: Header) -> Result<(), Error> {
        let current_root = self.root_hash();
        if header.block_hash() == self.root.prev_blockhash {
            self.trunk_rev.push(self.root);
            self.root = header;
            Ok(())
        } else {
            Err(Error::ChainMismatchRoot(current_root, header.block_hash()))
        }
    }

    /// Calculate total work of the chain
    pub fn total_work(&self) -> Work {
        let half_work = self
            .trunk_rev
            .iter()
            .fold(self.root.work(), |w, header| w + header.work());
        self.trunk_for
            .iter()
            .fold(half_work, |w, header| w + header.work())
    }

    /// The first block hash in the chain
    pub fn root_hash(&self) -> BlockHash {
        self.root.block_hash()
    }

    pub fn root(&self) -> Header {
        self.root
    }

    pub fn tip_hash(&self) -> BlockHash {
        self.trunk_for
            .last()
            .map(|h| h.block_hash())
            .unwrap_or(self.root_hash())
    }

    pub fn tip(&self) -> Header {
        self.trunk_for.last().cloned().unwrap_or(self.root)
    }

    pub fn headers(&self) -> impl Iterator<Item = Header> + use<'_> {
        [self.root]
            .into_iter()
            .chain(self.trunk_rev.iter().rev().cloned())
            .chain(self.trunk_for.iter().cloned())
    }
}

/// We sample block hashes exponentionally (^2) from the tip of the chain
fn get_locator_heights(height: u32) -> Vec<u32> {
    let mut is = vec![];
    let mut step = 1;
    let mut i = height as i32;
    while i > 0 {
        if is.len() >= 10 {
            // chain is too short from genesis
            step *= 2;
        }
        is.push(i as u32);
        i -= step;
    }
    is.push(0);
    is
}
