use std::io::Cursor;

use bitcoin::consensus::Encodable;
use bitcoin::{BlockHash, Txid};
use log::trace;
use rusqlite::{named_params, Connection, Row};

use super::super::error::Error;
use super::super::loaders::*;
use crate::vault::{UnitAmount, VaultAction, VaultId, VaultTx};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VaultTxMeta {
    pub vault_id: VaultId,
    pub vault_tx: VaultTx,
    pub block_hash: BlockHash,
    pub block_pos: usize,
    pub height: u32,
    pub btc_custody: u64,
    pub unit_volume: i32,
    pub btc_volume: i64,
    pub prev_tx: Txid,
}

/// Operations with vault in database
pub trait DatabaseVault {
    /// Get stored newtork type in the database
    fn store_vault_tx(
        &mut self,
        tx: &VaultTx,
        block_hash: BlockHash,
        block_pos: usize,
        height: u32,
        raw_tx: &bitcoin::Transaction,
    ) -> Result<VaultTxMeta, Error>;

    /// Find vault by transaction that is related to it
    fn find_vault_by_tx(&self, txid: Txid) -> Result<Option<Txid>, Error>;

    /// Delete ALL info about vaults and transactions
    fn drop_vaults(&self) -> Result<(), Error>;
}

impl DatabaseVault for Connection {
    fn store_vault_tx(
        &mut self,
        tx: &VaultTx,
        block_hash: BlockHash,
        block_pos: usize,
        height: u32,
        raw_tx: &bitcoin::Transaction,
    ) -> Result<VaultTxMeta, Error> {
        trace!("Search vault");
        let vault_id = find_parent_vault(self, &tx, &raw_tx)?;

        let conn_tx = self.transaction().map_err(Error::StartTransaction)?;

        // Fetch custody and balance infromation to properly save updates in metainfo
        let (btc_custody, prev_custody, prev_balance, prev_tx) = if tx.action == VaultAction::Open {
            let btc_custody = create_vault(&conn_tx, &tx, raw_tx)?;
            trace!("Get vault information for freshly created");
            let (_, prev_balance, prev_tx) = get_vault_chaining_info(&conn_tx, vault_id)?;
            (btc_custody, btc_custody, prev_balance, prev_tx) // Prev custody and current are the same for new one
        } else {
            trace!("Get vault information");
            let (prev_custody, prev_balance, prev_tx) =
                get_vault_chaining_info(&conn_tx, vault_id)?;
            let btc_custody = update_vault(&conn_tx, vault_id, &tx, raw_tx)?;
            (btc_custody, prev_custody, prev_balance, prev_tx)
        };

        let (unit_volume, btc_volume) = insert_vault_tx_raw(
            &conn_tx,
            tx,
            vault_id,
            block_hash,
            block_pos,
            height,
            raw_tx,
            prev_custody,
            prev_balance,
            prev_tx,
        )?;

        conn_tx.commit().map_err(Error::CommitTransaction)?;

        Ok(VaultTxMeta {
            vault_id,
            vault_tx: tx.clone(),
            block_hash,
            block_pos,
            height,
            btc_custody,
            unit_volume,
            btc_volume,
            prev_tx,
        })
    }

    /// Find vault by transaction that is related to it
    fn find_vault_by_tx(&self, txid: Txid) -> Result<Option<Txid>, Error> {
        let query = r#"
            SELECT vault_txid FROM transactions 
            WHERE txid = :txid
            LIMIT 1
        "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let mut rows = statement
            .query_map(named_params! {":txid": (&txid).field_encode()}, |row| {
                let vault_txid = row.field_decode(0)?;
                Ok(vault_txid)
            })
            .map_err(Error::ExecuteQuery)?;

        if let Some(row) = rows.next() {
            Ok(Some(row.map_err(Error::FetchRow)?))
        } else {
            Ok(None)
        }
    }

    fn drop_vaults(&self) -> Result<(), Error> {
        let query = r#"
            DELETE FROM transactions;
            DELETE FROM vaults;
        "#;
        self.execute_batch(query).map_err(Error::ExecuteQuery)?;
        Ok(())
    }
}

fn insert_vault_tx_raw(
    conn: &Connection,
    tx: &VaultTx,
    vault_id: Txid,
    block_hash: BlockHash,
    block_pos: usize,
    height: u32,
    raw_tx: &bitcoin::Transaction,
    prev_custody: u64,
    prev_balance: UnitAmount,
    prev_tx: Txid,
) -> Result<(i32, i64), Error> {
    trace!("Inserting vault transaction in db");
    let query = r#"
        INSERT INTO transactions VALUES(
            :txid,
            :output,
            :block_pos,
            :vault_txid,
            :version,
            :action,
            :balance,
            :oracle_price,
            :oracle_timestamp,
            :liquidation_price,
            :liquidation_hash,
            :block_hash,
            :height,
            :in_longest,
            :raw_tx, 
            :btc_custody,
            :unit_volume,
            :btc_volume,
            :prev_tx)
    "#;

    let mut tx_bytes = vec![];
    raw_tx
        .consensus_encode(&mut Cursor::new(&mut tx_bytes))
        .map_err(Error::EncodeBitcoinTransaction)?;

    let unit_volume = tx.balance as i32 - prev_balance as i32;
    let cur_custody = tx.assume_custody_value(&raw_tx)?;
    let btc_volume = cur_custody as i64 - prev_custody as i64;
    let mut statement = conn.prepare_cached(query).map_err(Error::PrepareQuery)?;
    statement
        .execute(named_params! {
            ":txid": (&tx.txid).field_encode(),
            ":output": tx.output as i64,
            ":block_pos": block_pos as i64,
            ":vault_txid": (&vault_id).field_encode(),
            ":version": tx.version.to_string(),
            ":action": tx.action.to_string(),
            ":balance": tx.balance as i64,
            ":oracle_price": tx.oracle_price as i64,
            ":oracle_timestamp": tx.oracle_timestamp as i64,
            ":liquidation_price": tx.liquidation_price,
            ":liquidation_hash": tx.liquidation_hash,
            ":block_hash": (&block_hash).field_encode(),
            ":height": height as i64,
            ":in_longest": 1, // assume that we don't scan forks
            ":raw_tx": tx_bytes,
            ":btc_custody": cur_custody,
            ":unit_volume": unit_volume,
            ":btc_volume": btc_volume,
            ":prev_tx": (&prev_tx).field_encode(),
        })
        .map_err(Error::ExecuteQuery)?;
    Ok((unit_volume, btc_volume))
}

fn create_vault(
    conn: &Connection,
    tx: &VaultTx,
    raw_tx: &bitcoin::Transaction,
) -> Result<u64, Error> {
    trace!("Inserting new vault in db");
    assert_eq!(
        tx.action,
        VaultAction::Open,
        "Creation of vault is only possible with opening tx"
    );
    let custody = tx.assume_custody_value(raw_tx)?;
    let query = r#"
            INSERT INTO vaults VALUES(
                :open_txid,
                :output,
                :balance,
                :oracle_price,
                :oracle_timestamp,
                :liquidation_price,
                :liquidation_hash,
                :custody,
                :last_tx
            )
        "#;
    let mut statement = conn.prepare_cached(query).map_err(Error::PrepareQuery)?;
    statement
        .execute(named_params! {
            ":open_txid": (&tx.txid).field_encode(),
            ":output": tx.output as i64,
            ":balance": tx.balance as i64,
            ":oracle_price": tx.oracle_price as i64,
            ":oracle_timestamp": tx.oracle_timestamp as i64,
            ":liquidation_price": tx.liquidation_price,
            ":liquidation_hash": tx.liquidation_hash,
            ":custody": custody,
            ":last_tx": (&tx.txid).field_encode(),
        })
        .map_err(Error::ExecuteQuery)?;
    Ok(custody)
}

fn update_vault(
    conn: &Connection,
    vault_id: Txid,
    tx: &VaultTx,
    raw_tx: &bitcoin::Transaction,
) -> Result<u64, Error> {
    trace!("Updating vault in db");
    assert!(
        tx.action != VaultAction::Open,
        "Update of vault is only possible with non opening tx"
    );
    let next_custody = tx.assume_custody_value(raw_tx)?;

    let query = r#"
            UPDATE vaults SET 
                balance = :balance,
                oracle_price = :oracle_price,
                oracle_timestamp = :oracle_timestamp,
                liquidation_price = :liquidation_price,
                liquidation_hash = :liquidation_hash,
                custody = :custody,
                last_tx = :last_tx
            WHERE open_txid = :vault_id
        "#;
    let mut statement = conn.prepare_cached(query).map_err(Error::PrepareQuery)?;
    statement
        .execute(named_params! {
            ":vault_id": (&vault_id).field_encode(),
            ":balance": tx.balance as i64,
            ":oracle_price": tx.oracle_price as i64,
            ":oracle_timestamp": tx.oracle_timestamp as i64,
            ":liquidation_price": tx.liquidation_price,
            ":liquidation_hash": tx.liquidation_hash,
            ":custody": next_custody,
            ":last_tx": (&tx.txid).field_encode(),
        })
        .map_err(Error::ExecuteQuery)?;
    Ok(next_custody)
}

// Helper that inspects bitcoin transaction and tries to identify vault by inputs
fn find_parent_vault(
    conn: &Connection,
    vtx: &VaultTx,
    raw_tx: &bitcoin::Transaction,
) -> Result<Txid, Error> {
    if vtx.action == VaultAction::Open {
        Ok(vtx.txid)
    } else {
        // Assume that first input is always related to the vault
        let first_input = raw_tx
            .input
            .first()
            .ok_or(Error::VaultTxNoInputs(vtx.txid))?;
        let parent_txid = first_input.previous_output.txid;
        let vault_id = conn
            .find_vault_by_tx(parent_txid)?
            .ok_or(Error::UnknownVaultTx(vtx.txid))?;
        Ok(vault_id)
    }
}

fn get_vault_chaining_info(conn: &Connection, vault_id: Txid) -> Result<(u64, u32, Txid), Error> {
    let query = r#"
        SELECT custody, balance, last_tx FROM vaults WHERE open_txid = :vault_id LIMIT 1
    "#;
    let mut statement = conn.prepare_cached(query).map_err(Error::PrepareQuery)?;
    let mut rows = statement
        .query_map(
            named_params! {
                ":vault_id": (&vault_id).field_encode(),
            },
            |row| {
                let custody = row.get(0)?;
                let balance = row.get(1)?;
                let last_tx = row.field_decode(2)?;
                Ok((custody, balance, last_tx))
            },
        )
        .map_err(Error::ExecuteQuery)?;

    if let Some(row) = rows.next() {
        Ok(row.map_err(Error::FetchRow)?)
    } else {
        Err(Error::UnknownVaultId(vault_id))
    }
}

pub fn load_vault_meta(row: &Row<'_>) -> Result<VaultTxMeta, rusqlite::Error> {
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
        btc_custody: row.get(15)?,
        unit_volume: row.get(16)?,
        btc_volume: row.get(17)?,
        prev_tx: row.field_decode(18)?,
    })
}
