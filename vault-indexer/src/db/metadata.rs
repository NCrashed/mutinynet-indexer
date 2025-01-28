use super::error::Error;
use bitcoin::{hashes::Hash, BlockHash};
use core::convert::TryInto;
use rusqlite::{named_params, types::Type, Connection};

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
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;

        let mut result = statement
            .query_map([], |row| {
                let count = row.get::<_, i64>(0)?;
                Ok(count != 0)
            })
            .map_err(Error::ExecuteQuery)?;

        if let Some(row) = result.next() {
            Ok(row.map_err(Error::FetchRow)?)
        } else {
            Ok(false)
        }
    }

    fn store_metadata(&self, meta: &DbMetadata) -> Result<(), Error> {
        let query = r#"
            INSERT INTO metadata VALUES(0, :tip_block_hash, :scanned_height)
                    ON CONFLICT(id) DO UPDATE SET 
                        tip_block_hash=excluded.tip_block_hash, 
                        scanned_height=excluded.scanned_height
            "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        statement
            .execute(named_params! {
                ":tip_block_hash": &meta.tip_block_hash.as_raw_hash().as_byte_array()[..],
                ":scanned_height": meta.scanned_height as i64,
            })
            .map_err(Error::ExecuteQuery)?;
        Ok(())
    }

    fn load_metada(&self) -> Result<DbMetadata, Error> {
        let query = "SELECT * FROM metadata LIMIT 1";
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;

        let mut rows = statement
            .query_map([], |row| {
                let tip_block_hash_bytes = row.get::<_, Vec<u8>>(1)?;
                let tip_block_hash_sized =
                    tip_block_hash_bytes.clone().try_into().map_err(|_| {
                        rusqlite::Error::FromSqlConversionFailure(
                            1,
                            Type::Blob,
                            Box::new(Error::TipBlockHashWrongSize(tip_block_hash_bytes)),
                        )
                    })?;
                let scanned_height = row.get::<_, i64>(2)?;
                let tip_block_hash = BlockHash::from_byte_array(tip_block_hash_sized);
                Ok(DbMetadata {
                    tip_block_hash,
                    scanned_height: scanned_height as u32,
                })
            })
            .map_err(Error::ExecuteQuery)?;

        if let Some(row) = rows.next() {
            Ok(row.map_err(Error::FetchRow)?)
        } else {
            Err(Error::NoMetadata)
        }
    }
}
