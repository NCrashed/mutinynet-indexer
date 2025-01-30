use std::io::Cursor;

use bitcoin::consensus::Encodable;
use bitcoin::{BlockHash, Txid};
use log::trace;
use rusqlite::{named_params, Connection};

use super::super::error::Error;
use super::super::loaders::*;
use crate::vault::{UnitAmount, VaultAction, VaultId, VaultTx};

/// Operations with vault in database
pub trait DatabaseVault {
    /// Get stored newtork type in the database
    fn store_vault_tx(
        &self,
        tx: &VaultTx,
        block_hash: BlockHash,
        block_pos: usize,
        height: u32,
        raw_tx: &bitcoin::Transaction,
    ) -> Result<VaultId, Error>;

    /// Find vault by transaction that is related to it
    fn find_vault_by_tx(&self, txid: Txid) -> Result<Option<Txid>, Error>;

    /// Delete ALL info about vaults and transactions
    fn drop_vaults(&self) -> Result<(), Error>;
}

impl DatabaseVault for Connection {
    fn store_vault_tx(
        &self,
        tx: &VaultTx,
        block_hash: BlockHash,
        block_pos: usize,
        height: u32,
        raw_tx: &bitcoin::Transaction,
    ) -> Result<VaultId, Error> {
        let vault_id = find_parent_vault(self, &tx, &raw_tx)?;
        if tx.action == VaultAction::Open {
            create_vault(self, &tx)?;
        } else {
            update_vault(self, &tx)?;
        }
        let prev_balance = find_prev_balance(self, vault_id, block_pos, height)?;
        insert_vault_tx_raw(
            self,
            tx,
            vault_id,
            block_hash,
            block_pos,
            height,
            raw_tx,
            prev_balance,
        )?;
        Ok(vault_id)
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
    prev_balance: Option<UnitAmount>,
) -> Result<(), Error> {
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
            :units_volume,
            :btc_volume)
    "#;

    let mut tx_bytes = vec![];
    raw_tx
        .consensus_encode(&mut Cursor::new(&mut tx_bytes))
        .map_err(Error::EncodeBitcoinTransaction)?;

    let units_volume = prev_balance.map_or(tx.balance as i32, |old_balance| {
        tx.balance as i32 - old_balance as i32
    });
    let btc_volume = tx.assume_btc_volume(raw_tx)?;
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
            ":units_volume": units_volume,
            ":btc_volume": btc_volume,
        })
        .map_err(Error::ExecuteQuery)?;
    Ok(())
}

fn create_vault(conn: &Connection, tx: &VaultTx) -> Result<(), Error> {
    trace!("Inserting new vault in db");
    assert_eq!(
        tx.action,
        VaultAction::Open,
        "Creation of vault is only possible with opening tx"
    );

    let query = r#"
            INSERT INTO vaults VALUES(
                :open_txid,
                :output,
                :balance,
                :oracle_price,
                :oracle_timestamp,
                :liquidation_price,
                :liquidation_hash
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
            ":liquidation_hash": tx.liquidation_hash
        })
        .map_err(Error::ExecuteQuery)?;
    Ok(())
}

fn update_vault(conn: &Connection, tx: &VaultTx) -> Result<(), Error> {
    trace!("Updating vault in db");
    assert!(
        tx.action != VaultAction::Open,
        "Update of vault is only possible with non opening tx"
    );

    let query = r#"
            UPDATE vaults SET 
                balance = :balance,
                oracle_price = :oracle_price,
                oracle_timestamp = :oracle_timestamp,
                liquidation_price = :liquidation_price,
                liquidation_hash = :liquidation_hash
            WHERE open_txid = :vault_id
        "#;
    let mut statement = conn.prepare_cached(query).map_err(Error::PrepareQuery)?;
    statement
        .execute(named_params! {
            ":vault_id": (&tx.txid).field_encode(),
            ":balance": tx.balance as i64,
            ":oracle_price": tx.oracle_price as i64,
            ":oracle_timestamp": tx.oracle_timestamp as i64,
            ":liquidation_price": tx.liquidation_price,
            ":liquidation_hash": tx.liquidation_hash
        })
        .map_err(Error::ExecuteQuery)?;
    Ok(())
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

fn find_prev_balance(
    conn: &Connection,
    vault_id: Txid,
    block_pos: usize,
    height: u32,
) -> Result<Option<UnitAmount>, Error> {
    let query = r#"
        SELECT
            height,
            block_pos,
            balance
        FROM transactions
        WHERE
            vault_txid = :vault_id
            AND (
                height < :height
                OR (height = :height AND block_pos < :block_pos)
            )
        ORDER BY height DESC, block_pos DESC
        LIMIT 1
    "#;

    let mut statement = conn.prepare_cached(query).map_err(Error::PrepareQuery)?;
    let mut rows = statement
        .query_map(
            named_params! {
                ":vault_id": (&vault_id).field_encode(),
                ":height": height,
                ":block_pos": block_pos,
            },
            |row| {
                let balance = row.get(2)?;
                Ok(balance)
            },
        )
        .map_err(Error::ExecuteQuery)?;

    if let Some(row) = rows.next() {
        Ok(Some(row.map_err(Error::FetchRow)?))
    } else {
        Ok(None)
    }
}
