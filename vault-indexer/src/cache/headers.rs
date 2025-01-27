use super::error::Error;
use crate::db::{DatabaseHeaders, DatabaseMeta, HeaderRecord};
use bitcoin::{block::Header, BlockHash, Work};
use core::{fmt::Display, iter::Iterator};
use log::*;
use sqlite::Connection;
use std::collections::HashMap;

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
        })?;
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
        for header in chain.headers() {
            let hash = header.block_hash();
            let header_record = self
                .headers
                .get_mut(&hash)
                .ok_or(Error::MissingHeader(hash))?;
            header_record.in_longest = false;
            self.dirty.push(hash);
        }
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
        for header in chain.headers() {
            let hash = header.block_hash();
            if self.headers.contains_key(&hash) {
                // activate
                let header_record = self
                    .headers
                    .get_mut(&hash)
                    .ok_or(Error::MissingHeader(hash))?;
                header_record.in_longest = true;
                self.dirty.push(hash);
                prev_record = header_record.clone();
            } else {
                // insert new
                let new_record = HeaderRecord {
                    header,
                    height: prev_record.height + 1,
                    in_longest: true,
                };
                self.headers.insert(hash, new_record.clone());
                self.dirty.push(hash);
                prev_record = new_record;
            }
        }
        trace!("Make the best tip as: {}", chain.tip_hash());
        self.best_tip = chain.tip_hash();
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
                self.dirty.push(hash);
                prev_record = new_record;
            }
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
