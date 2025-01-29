use bitcoin::{
    consensus::Decodable,
    opcodes::all::{OP_PUSHBYTES_14, OP_PUSHNUM_8, OP_RETURN},
    Script, Transaction,
};
use core::{assert_eq, fmt::Display};
use log::*;
use std::io::Cursor;

pub use bitcoin::Wtxid;
use thiserror::Error;

/// Action inside the vault tx
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum VaultAction {
    // Open new vault
    Open = 0x6f,
    // Deposit BTC
    Deposit = 0x64,
    // Withdraw BTC
    Withdraw = 0x77,
    // Borrow UNIT
    Borrow = 0x62,
    // Repay UNIT
    Repay = 0x72,
}

impl VaultAction {
    pub fn to_protocol(&self) -> u8 {
        *self as u8
    }

    pub fn from_protocol(v: u8) -> Option<Self> {
        match v {
            0x6f => Some(VaultAction::Open),
            0x64 => Some(VaultAction::Deposit),
            0x77 => Some(VaultAction::Withdraw),
            0x62 => Some(VaultAction::Borrow),
            0x72 => Some(VaultAction::Repay),
            _ => None,
        }
    }
}

/// Known versions of vault transaction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum VaultVersion {
    Vault1_0 = 1,
}

impl VaultVersion {
    pub fn to_protocol(&self) -> u8 {
        match self {
            VaultVersion::Vault1_0 => 1,
        }
    }

    pub fn from_protocol(v: u8) -> Option<Self> {
        match v {
            1 => Some(VaultVersion::Vault1_0),
            _ => None,
        }
    }
}

impl Display for VaultVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_protocol())
    }
}

/// Fixed point (2 decimals) amount of stable units
pub type UnitAmount = u32;

/// TODO
pub type OraclePrice = u32;

/// Liquidation hash stored in byte array
//pub type LiquidationHash = [u8; 32];
pub type LiquidationHash = Vec<u8>;

/// Contains metadata about the vault transaction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultTx {
    /// Hash of transaction where we found the vault tx
    pub txid: Wtxid,
    /// The output number with the op_return
    pub output: u32,
    /// Version of the transaction
    pub version: VaultVersion,
    /// The encoded action
    pub action: VaultAction,
    /// Balance of UNIT
    pub balance: UnitAmount,
    /// Recorded oracle price
    pub oracle_price: OraclePrice,
    /// UNIX timestamp of the oracle
    pub oracle_timestamp: u32,
    /// The price when liquidation will happen
    pub liquidation_price: Option<OraclePrice>,
    /// Hash of the liquidation
    pub liquidation_hash: Option<LiquidationHash>,
}

/// Fields that we expect in the op_return payload
#[derive(Debug, Error, PartialEq)]
pub enum MissingVaultField {
    #[error("version")]
    Version,
    #[error("action")]
    Action,
    #[error("balance")]
    Balance,
    #[error("oracle price")]
    OraclePrice,
    #[error("oracle timestamp")]
    OracleTimestamp,
    #[error("liquidation price")]
    LiquidationPrice,
    #[error("liquidation hash")]
    LiquidationHash,
}

#[derive(Debug, Error, PartialEq)]
pub enum NotVaultReason {
    #[error("No OP_RETURN output detected")]
    NoOpReturn,
    #[error("No OP_PUSHNUM_8 after OP_RETURN")]
    NoOpPush8,
    #[error("Expected OP_PUSHNUM_8 but got opcode {0}")]
    MismatchOpPush8(u8),
    #[error("No OP_PUSHBYTES_14 after OP_RETURN")]
    NoOpPushbytes14,
    #[error("Expected OP_PUSHBYTES_14 but got opcode {0}")]
    MismatchOpPushbytes14(u8),
    #[error("Missing {0} field")]
    MissingField(MissingVaultField),
    #[error("Not expected version {0}")]
    WrongVersion(u8),
    #[error("Not expected action {0}")]
    WrongAction(u8),
}

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("Cannot decode Bitcoin transaction: {0}")]
    InvalidParentTx(#[from] bitcoin::consensus::encode::Error),
    #[error("Transaction is not Vault tx: {0}")]
    NotVaultTx(NotVaultReason),
}

impl VaultTx {
    /// Detect and parse the vault transaction from the given bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        // Decode vessel bitcoin transaction
        let tx: Transaction = Transaction::consensus_decode(&mut Cursor::new(bytes))?;
        // Find first op_return
        let (out_i, op_return_out): (usize, &Script) = tx
            .output
            .iter()
            .enumerate()
            .map(|(i, out)| (i, out.script_pubkey.as_script()))
            .find(|(_, out)| out.is_op_return())
            .ok_or(ParseError::NotVaultTx(NotVaultReason::NoOpReturn))?;

        // Now let parse instructions one by one
        let mut instructions = op_return_out.bytes();
        // Skip op_return
        let op_return: u8 = instructions
            .next()
            .ok_or(ParseError::NotVaultTx(NotVaultReason::NoOpReturn))?;
        assert_eq!(op_return, OP_RETURN.to_u8()); // if fires, the is_op_return fn is a lyier

        // Skip OP_PUSHNUM_8
        let op_pushnum_8: u8 = instructions
            .next()
            .ok_or(ParseError::NotVaultTx(NotVaultReason::NoOpPush8))?;
        if op_pushnum_8 != OP_PUSHNUM_8.to_u8() {
            return Err(ParseError::NotVaultTx(NotVaultReason::MismatchOpPush8(
                op_pushnum_8,
            )));
        }

        // Skip op_push14
        let op_pushbytes_14: u8 = instructions
            .next()
            .ok_or(ParseError::NotVaultTx(NotVaultReason::NoOpPushbytes14))?;
        if op_pushbytes_14 != OP_PUSHBYTES_14.to_u8() {
            return Err(ParseError::NotVaultTx(
                NotVaultReason::MismatchOpPushbytes14(op_pushbytes_14),
            ));
        }

        // Parse version field
        let version_code: u8 =
            instructions
                .next()
                .ok_or(ParseError::NotVaultTx(NotVaultReason::MissingField(
                    MissingVaultField::Version,
                )))?;
        let version = VaultVersion::from_protocol(version_code).ok_or(ParseError::NotVaultTx(
            NotVaultReason::WrongVersion(version_code),
        ))?;

        // Parse action field
        let action_code: u8 =
            instructions
                .next()
                .ok_or(ParseError::NotVaultTx(NotVaultReason::MissingField(
                    MissingVaultField::Action,
                )))?;
        let action = VaultAction::from_protocol(action_code).ok_or(ParseError::NotVaultTx(
            NotVaultReason::WrongAction(action_code),
        ))?;

        // Fetch units balance
        let balance = instructions.next_u32_be().ok_or(ParseError::NotVaultTx(
            NotVaultReason::MissingField(MissingVaultField::Balance),
        ))?;
        // Note that in requirements the timestamp is going BEFORE the price, but in the 
        // blockchain it is as here.
        // Fetch oracle timestamp
        let oracle_timestamp = instructions.next_u32_be().ok_or(ParseError::NotVaultTx(
            NotVaultReason::MissingField(MissingVaultField::OracleTimestamp),
        ))?;
        // Fetch oracle price
        let oracle_price = instructions.next_u32_be().ok_or(ParseError::NotVaultTx(
            NotVaultReason::MissingField(MissingVaultField::OraclePrice),
        ))?;

        // Fetch liqudation price
        // let liquidation_price = instructions.next_u32_be().ok_or(ParseError::NotVaultTx(
        //     NotVaultReason::MissingField(MissingVaultField::LiquidationPrice),
        // ))?;
        let liquidation_price = instructions.next_u32_be();

        // Take remaining bytes as hash
        // let liquidation_hash =
        //     instructions
        //         .next32()
        //         .ok_or(ParseError::NotVaultTx(NotVaultReason::MissingField(
        //             MissingVaultField::LiquidationHash,
        //         )))?;
        let liquidation_hash = instructions.collect::<Vec<_>>();

        Ok(VaultTx {
            txid: tx.compute_wtxid(),
            output: out_i as u32,
            version,
            action,
            balance,
            oracle_price,
            oracle_timestamp,
            liquidation_price,
            liquidation_hash: if liquidation_hash.is_empty() {
                None
            } else {
                Some(liquidation_hash)
            },
        })
    }
}

trait BytesParser {
    fn next4(&mut self) -> Option<[u8; 4]>;

    fn next32(&mut self) -> Option<[u8; 32]>;

    fn next_u32_be(&mut self) -> Option<u32> {
        self.next4().map(|bytes| u32::from_be_bytes(bytes))
    }
}

impl<T: Iterator<Item = u8>> BytesParser for T {
    fn next4(&mut self) -> Option<[u8; 4]> {
        let mut buf = [0u8; 4];
        for i in 0..4 {
            buf[i] = self.next()?;
        }
        Some(buf)
    }

    fn next32(&mut self) -> Option<[u8; 32]> {
        let mut buf = [0u8; 32];
        for i in 0..32 {
            buf[i] = self.next()?;
        }
        Some(buf)
    }
}
