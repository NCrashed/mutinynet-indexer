use super::super::Error;
use crate::db::loaders::FieldDecode;
use crate::db::loaders::FieldEncode;
use crate::vault::UnitAmount;
use bitcoin::consensus::Encodable;
use bitcoin::{Transaction, Txid};
use rusqlite::{named_params, Connection};
use std::io::Cursor;

/// Stored info about UNIT transaction
pub struct UnitTxMeta {
    pub transaction: Transaction,
    pub unit_amount: UnitAmount,
}

/// Operations with UNIT rune token in database
pub trait DatabaseRune {
    /// Store UNIT related transaction to the DB
    fn store_unit_tx(&mut self, tx: &Transaction, unit_amount: UnitAmount) -> Result<(), Error>;

    /// Find the UNIT transaction by its txid
    fn load_unit_tx(&self, txid: Txid) -> Result<UnitTxMeta, Error>;

    /// Delete ALL info about UNIT transactions
    fn drop_unit_index(&self) -> Result<(), Error>;
}

impl DatabaseRune for Connection {
    fn store_unit_tx(&mut self, tx: &Transaction, unit_amount: UnitAmount) -> Result<(), Error> {
        let query = r#"
            INSERT INTO transactions_runes VALUES(:txid, :raw_tx, :unit_amount)
        "#;
        let mut tx_bytes = vec![];
        tx.consensus_encode(&mut Cursor::new(&mut tx_bytes))
            .map_err(Error::EncodeBitcoinTransaction)?;

        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        statement
            .execute(named_params! {
                ":txid": (&tx.compute_txid()).field_encode(),
                ":raw_tx": tx_bytes,
                ":unit_amount": unit_amount,
            })
            .map_err(Error::ExecuteQuery)?;
        Ok(())
    }

    fn load_unit_tx(&self, txid: Txid) -> Result<UnitTxMeta, Error> {
        let query = r#"
            SELECT * FROM transactions_runes WHERE txid = :txid
        "#;
        let mut statement = self.prepare_cached(query).map_err(Error::PrepareQuery)?;
        let mut rows = statement
            .query_map(
                named_params! {
                    ":vault_id": (&txid).field_encode(),
                },
                |row| {
                    let transaction = row.field_decode(1)?;
                    let unit_amount = row.get(2)?;
                    Ok(UnitTxMeta {
                        transaction,
                        unit_amount,
                    })
                },
            )
            .map_err(Error::ExecuteQuery)?;

        if let Some(row) = rows.next() {
            Ok(row.map_err(Error::FetchRow)?)
        } else {
            Err(Error::UnknownUnitTx(txid))
        }
    }

    fn drop_unit_index(&self) -> Result<(), Error> {
        let query = r#"
            DELETE FROM transactions_runes;
        "#;
        self.execute_batch(query).map_err(Error::ExecuteQuery)?;
        Ok(())
    }
}
