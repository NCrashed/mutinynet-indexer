use super::super::Error;
use crate::{
    db::loaders::{FieldDecode, FieldEncode},
    vault::{UnitAmount, VaultAction, VaultId, VaultTx},
};
use bitcoin::{BlockHash, Txid};
use rusqlite::{named_params, Connection, Row};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultTxMeta {
    pub vault_id: VaultId,
    pub vault_tx: VaultTx,
    pub block_hash: BlockHash,
    pub block_pos: usize,
    pub height: u32,
    pub units_volume: i32,
    pub btc_volume: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionAggItem {
    pub timestamp_start: u32,
    pub unit_volume: UnitAmount,
    pub btc_volume: u64,
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

    fn action_aggregated(
        &self,
        action: VaultAction,
        timespan: u32,
    ) -> Result<Vec<ActionAggItem>, Error>;
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

    fn action_aggregated(
        &self,
        action: VaultAction,
        timespan: u32,
    ) -> Result<Vec<ActionAggItem>, Error> {
        let query = r#"
            SELECT 
                (oracle_timestamp / :span) * :span AS time_bucket,
                SUM(units_volume) AS total_units_volume,
                SUM(btc_volume)   AS total_btc_volume
            FROM transactions
            WHERE action = :action
            GROUP BY time_bucket
            ORDER BY time_bucket;
        "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let rows = statement
            .query_map(
                named_params! {
                    ":action": action.field_encode(),
                    ":span": timespan
                },
                |row| {
                    Ok(ActionAggItem {
                        timestamp_start: row.get(0)?,
                        unit_volume: row.get::<_, i32>(1)?.abs() as u32,
                        btc_volume: row.get::<_, i64>(2)?.abs() as u64,
                    })
                },
            )
            .map_err(Error::ExecuteQuery)?;
        Ok(rows
            .map(|row| row.map_err(Error::FetchRow))
            .collect::<Result<Vec<_>, Error>>()?)
    }
}

fn load_vault_meta(row: &Row<'_>) -> Result<VaultTxMeta, rusqlite::Error> {
    Ok(VaultTxMeta {
        vault_id: row.field_decode(3)?,
        vault_tx: VaultTx {
            txid: row.field_decode(0)?,
            output: row.get(1)?,
            version: row.field_decode(4)?,
            action: row.field_decode(5)?,
            balance: row.get(6)?,
            oracle_price: row.get(7)?,
            oracle_timestamp: row.get(8)?,
            liquidation_price: row.get(9)?,
            liquidation_hash: row.field_decode(10)?,
        },
        block_hash: row.field_decode(11)?,
        block_pos: row.get(2)?,
        height: row.get(12)?,
        units_volume: row.get(13)?,
        btc_volume: row.get(14)?,
    })
}
