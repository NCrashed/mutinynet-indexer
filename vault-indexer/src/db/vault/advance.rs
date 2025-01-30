use super::{super::Error, load_vault_meta, VaultTxMeta};
use crate::{
    db::loaders::{invert, FieldEncode},
    vault::{UnitAmount, VaultAction},
};
use bitcoin::Txid;
use rusqlite::{named_params, Connection};
use serde::{Deserialize, Serialize};

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

    fn overall_volume(&self) -> Result<(u64, UnitAmount), Error>;
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
                SUM(abs(unit_volume)) AS total_unit_volume,
                SUM(abs(btc_volume))   AS total_btc_volume
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
                        unit_volume: row.get::<_, i32>(1)? as u32,
                        btc_volume: row.get::<_, i64>(2)? as u64,
                    })
                },
            )
            .map_err(Error::ExecuteQuery)?;
        Ok(rows
            .map(|row| row.map_err(Error::FetchRow))
            .collect::<Result<Vec<_>, Error>>()?)
    }

    fn overall_volume(&self) -> Result<(u64, UnitAmount), Error> {
        let query = r#"
            SELECT 
                SUM(abs(btc_volume))   AS total_btc_volume,
                SUM(abs(unit_volume)) AS total_unit_volume
            FROM transactions;
        "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let mut rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?.abs() as u64,
                    row.get::<_, i32>(1)?.abs() as u32,
                ))
            })
            .map_err(Error::ExecuteQuery)?;
        let res = invert(rows.next().map(|row| row.map_err(Error::FetchRow)))?;
        Ok(res.unwrap_or((0, 0)))
    }
}
