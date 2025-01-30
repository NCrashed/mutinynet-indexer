use bitcoin::{BlockHash, Txid};
use rusqlite::{named_params, Connection, Row};

use super::super::Error;
use crate::{
    db::loaders::{FieldDecode, FieldEncode},
    vault::{VaultId, VaultTx},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultTxMeta {
    pub vault_id: VaultId,
    pub vault_tx: VaultTx,
    pub block_hash: BlockHash,
    pub height: u32,
}

/// Operations with vault in database for some complex queries required for the
/// websocket service to operate.
pub trait DatabaseVaultAdvance {
    fn range_history_all(
        &self,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<Vec<VaultTxMeta>, Error>;

    fn range_history_vault(
        &self,
        vault_id: Txid,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<Vec<VaultTxMeta>, Error>;
}

impl DatabaseVaultAdvance for Connection {
    fn range_history_all(
        &self,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<Vec<VaultTxMeta>, Error> {
        let query = r#"
            SELECT * FROM transactions
            WHERE oracle_timestamp >= :start AND oracle_timestamp < :end
        "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let rows = statement
            .query_map(
                named_params! {":start": start.unwrap_or(0), ":end": end.unwrap_or(u32::MAX)},
                load_vault_meta,
            )
            .map_err(Error::ExecuteQuery)?;
        Ok(rows
            .map(|row| row.map_err(Error::FetchRow))
            .collect::<Result<Vec<_>, Error>>()?)
    }

    fn range_history_vault(
        &self,
        vault_id: Txid,
        start: Option<u32>,
        end: Option<u32>,
    ) -> Result<Vec<VaultTxMeta>, Error> {
        let query = r#"
            SELECT * FROM transactions
            WHERE vault_txid = :vault_id AND oracle_timestamp >= :start AND oracle_timestamp < :end
        "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let rows = statement
            .query_map(
                named_params! {
                    ":vault_id": (&vault_id).field_encode(),
                    ":start": start.unwrap_or(0),
                    ":end": end.unwrap_or(u32::MAX)
                },
                load_vault_meta,
            )
            .map_err(Error::ExecuteQuery)?;
        Ok(rows
            .map(|row| row.map_err(Error::FetchRow))
            .collect::<Result<Vec<_>, Error>>()?)
    }
}

fn load_vault_meta(row: &Row<'_>) -> Result<VaultTxMeta, rusqlite::Error> {
    Ok(VaultTxMeta {
        vault_id: row.field_decode(2)?,
        vault_tx: VaultTx {
            txid: row.field_decode(0)?,
            output: row.get(1)?,
            version: row.field_decode(3)?,
            action: row.field_decode(4)?,
            balance: row.get(5)?,
            oracle_price: row.get(6)?,
            oracle_timestamp: row.get(7)?,
            liquidation_price: row.get(8)?,
            liquidation_hash: row.field_decode(9)?,
        },
        block_hash: row.field_decode(10)?,
        height: row.get(11)?,
    })
}
