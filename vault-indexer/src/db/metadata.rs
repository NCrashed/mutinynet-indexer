use super::error::Error;
use super::tools::*;
use bitcoin::{hashes::Hash, BlockHash};
use core::convert::TryInto;
use sqlite::{Connection, State, Value};

#[derive(Debug, Clone)]
pub struct DbMetadata {
    pub tip_block_hash: BlockHash,
    pub scanned_height: u32,
}

pub trait DatabaseMeta {
    /// Get current best main chain
    fn get_main_tip(&self) -> Result<BlockHash, Error>;

    /// Update the head of the chain with largest PoW work
    fn set_best_tip(&self, tip: BlockHash) -> Result<(), Error>;

    /// Get current scanned height
    fn get_scanned_height(&self) -> Result<u32, Error>;

    /// Update the scanned height (until which block we searched the blockchain)
    fn set_scanned_height(&self, height: u32) -> Result<(), Error>;

    /// Returns true if we have single row in metadata table
    fn has_metadata(&self) -> Result<bool, Error>;

    // Update metadata table
    fn store_metadata(&self, meta: &DbMetadata) -> Result<(), Error>;

    // Fetch all metadata from table
    fn load_metada(&self) -> Result<DbMetadata, Error>;
}

impl DatabaseMeta for Connection {
    fn get_main_tip(&self) -> Result<BlockHash, Error> {
        let meta = self.load_metada()?;
        Ok(meta.tip_block_hash)
    }

    fn set_best_tip(&self, tip: BlockHash) -> Result<(), Error> {
        let mut meta = self.load_metada()?;
        meta.tip_block_hash = tip;
        self.store_metadata(&meta)
    }

    fn get_scanned_height(&self) -> Result<u32, Error> {
        let meta = self.load_metada()?;
        Ok(meta.scanned_height)
    }

    fn set_scanned_height(&self, height: u32) -> Result<(), Error> {
        let mut meta = self.load_metada()?;
        meta.scanned_height = height;
        self.store_metadata(&meta)
    }

    fn has_metadata(&self) -> Result<bool, Error> {
        let query = "SELECT count(id) as count FROM metadata";
        let mut statement = self.prepare(query).map_err(Error::PrepareQuery)?;

        if let State::Row = statement.next().map_err(Error::QueryNextRow)? {
            let count = statement.read_field::<i64>("count")?;
            Ok(count != 0)
        } else {
            Ok(false)
        }
    }

    fn store_metadata(&self, meta: &DbMetadata) -> Result<(), Error> {
        let query = "INSERT INTO metadata VALUES(0, :tip_block_hash, :scanned_height)
                    ON CONFLICT(id) DO UPDATE SET tip_block_hash=excluded.tip_block_hash, scanned_height=excluded.scanned_height";
        self.single_execute::<_, (_, Value)>(
            "upsert metadata",
            query,
            [
                (
                    ":tip_block_hash",
                    meta.tip_block_hash.as_raw_hash().as_byte_array()[..].into(),
                ),
                (":scanned_height", (meta.scanned_height as i64).into()),
            ],
        )
    }

    fn load_metada(&self) -> Result<DbMetadata, Error> {
        let query = "SELECT * FROM metadata LIMIT 1";
        let mut statement = self.prepare(query).map_err(Error::PrepareQuery)?;

        if let State::Row = statement.next().map_err(Error::QueryNextRow)? {
            let tip_block_hash_bytes = statement.read_field::<Vec<u8>>("tip_block_hash")?;
            let tip_block_hash_sized = tip_block_hash_bytes
                .clone()
                .try_into()
                .map_err(|_| Error::TipBlockHashWrongSize(tip_block_hash_bytes))?;
            let scanned_height = statement.read_field::<i64>("scanned_height")?;
            let tip_block_hash = BlockHash::from_byte_array(tip_block_hash_sized);
            Ok(DbMetadata {
                tip_block_hash,
                scanned_height: scanned_height as u32,
            })
        } else {
            Err(Error::NoMetadata)
        }
    }
}
