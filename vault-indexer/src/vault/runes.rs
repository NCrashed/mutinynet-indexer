use core::matches;

use bitcoin::{Transaction, Txid};
use ordinals::{Artifact, Cenotaph, RuneId, Runestone};
use thiserror::Error;

use super::UnitAmount;

/// UNIT token ID inside the runestones
pub const UNIT_RUNE_ID: RuneId = RuneId {
    block: 1527352,
    tx: 1,
};

/// Parsed info from runestone with edicts about UNIT token
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitTransaction {
    pub txid: Txid,
    pub unit_amount: UnitAmount,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("The {0} is not a rune transaction")]
    NotRuneTx(Txid),
    #[error("The {0} is cenotaph: {1:#?}")]
    Cenotaph(Txid, Cenotaph),
    #[error("The {0} doesn't have edicts for UNIT, runestone: {1:#?}")]
    DontHaveUnitRune(Txid, Runestone),
}

impl Error {
    pub fn is_definetely_not_unit(&self) -> bool {
        matches!(self, Error::NotRuneTx(_))
    }
}

impl UnitTransaction {
    pub fn from_tx(tx: &Transaction) -> Result<Self, Error> {
        let txid = tx.compute_txid();
        let artifact = Runestone::decipher(&tx).ok_or(Error::NotRuneTx(txid))?;
        match artifact {
            Artifact::Runestone(runestone) => {
                let mut unit_amount = 0;
                let mut units_encoutered = false;
                for edict in runestone.edicts.iter() {
                    if edict.id == UNIT_RUNE_ID {
                        unit_amount += edict.amount;
                        units_encoutered = true;
                    }
                }
                if !units_encoutered {
                    Err(Error::DontHaveUnitRune(txid, runestone))
                } else {
                    Ok(UnitTransaction {
                        txid,
                        unit_amount: unit_amount as u32,
                    })
                }
            }
            Artifact::Cenotaph(cenotaph) => Err(Error::Cenotaph(txid, cenotaph)),
        }
    }
}
