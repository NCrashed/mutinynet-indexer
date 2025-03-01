use super::Error;
use crate::vault::{VaultAction, VaultVersion};
use bitcoin::consensus::Decodable;
use bitcoin::hashes::Hash;
use bitcoin::{BlockHash, Transaction, Txid};
use core::str::FromStr;
use rusqlite::{types::Type, Row};
use std::io::Cursor;

// Helper that extracts T from field in row. Need separate trait as orphan
// rools doesn't allow us define FromSql for external types.
pub trait FieldDecode<T> {
    fn field_decode(&self, index: usize) -> Result<T, rusqlite::Error>;
}

impl FieldDecode<Txid> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<Txid, rusqlite::Error> {
        let txid_bytes = self.get::<_, Vec<u8>>(index)?;
        let txid_bytes_sized = txid_bytes.clone().try_into().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                Type::Blob,
                Box::new(Error::TxidWrongSize(txid_bytes)),
            )
        })?;
        Ok(Txid::from_byte_array(txid_bytes_sized))
    }
}

impl FieldDecode<BlockHash> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<BlockHash, rusqlite::Error> {
        let hash_bytes = self.get::<_, Vec<u8>>(index)?;
        let hash_bytes_sized = hash_bytes.clone().try_into().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                Type::Blob,
                Box::new(Error::TipBlockHashWrongSize(hash_bytes)),
            )
        })?;
        Ok(BlockHash::from_byte_array(hash_bytes_sized))
    }
}

impl FieldDecode<VaultVersion> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<VaultVersion, rusqlite::Error> {
        let version_str = self.get::<_, String>(index)?;
        let version = VaultVersion::from_str(&version_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(e))
        })?;
        Ok(version)
    }
}

impl FieldDecode<VaultAction> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<VaultAction, rusqlite::Error> {
        let action_str = self.get::<_, String>(index)?;
        let action = VaultAction::from_str(&action_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(index, Type::Text, Box::new(e))
        })?;
        Ok(action)
    }
}

impl FieldDecode<[u8; 20]> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<[u8; 20], rusqlite::Error> {
        let bytes = self.get::<_, Vec<u8>>(index)?;
        let bytes_sized = bytes.clone().try_into().map_err(|_| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                Type::Blob,
                Box::new(Error::ArrayWrongSize(20, bytes)),
            )
        })?;
        Ok(bytes_sized)
    }
}

impl FieldDecode<Option<[u8; 20]>> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<Option<[u8; 20]>, rusqlite::Error> {
        let mbytes = self.get::<_, Option<Vec<u8>>>(index)?;
        let mbytes_sized = mbytes.map(|bytes| {
            bytes.clone().try_into().map_err(|_| {
                rusqlite::Error::FromSqlConversionFailure(
                    index,
                    Type::Blob,
                    Box::new(Error::ArrayWrongSize(20, bytes)),
                )
            })
        });
        invert(mbytes_sized)
    }
}

impl FieldDecode<Transaction> for Row<'_> {
    fn field_decode(&self, index: usize) -> Result<Transaction, rusqlite::Error> {
        let tx_bytes = self.get::<_, Vec<u8>>(index)?;
        let tx = Transaction::consensus_decode(&mut Cursor::new(&tx_bytes)).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                Type::Blob,
                Box::new(Error::TransactionDecode(tx_bytes, e)),
            )
        })?;
        Ok(tx)
    }
}

// Helper that encodes T for SQL representation. Need separate trait as orphan
// rools doesn't allow us define ToSql for external types.
pub trait FieldEncode {
    type SqlRepr;
    fn field_encode(&self) -> Self::SqlRepr;
}

impl<'a> FieldEncode for &'a Txid {
    type SqlRepr = &'a [u8];

    fn field_encode(&self) -> &'a [u8] {
        &self.as_raw_hash().as_byte_array()[..]
    }
}

impl<'a> FieldEncode for &'a BlockHash {
    type SqlRepr = &'a [u8];

    fn field_encode(&self) -> &'a [u8] {
        &self.as_raw_hash().as_byte_array()[..]
    }
}

impl FieldEncode for VaultAction {
    type SqlRepr = String;

    fn field_encode(&self) -> String {
        self.to_str().to_owned()
    }
}

// That is called traverse in Haskell
pub fn invert<T, E>(x: Option<Result<T, E>>) -> Result<Option<T>, E> {
    x.map_or(Ok(None), |v| v.map(Some))
}
