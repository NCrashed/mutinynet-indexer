pub use bitcoin::Txid;
use bitcoin::{
    consensus::Decodable,
    opcodes::all::{OP_PUSHBYTES_14, OP_PUSHBYTES_38, OP_PUSHNUM_8, OP_RETURN},
    Script, Transaction, TxIn, TxOut,
};
use core::{assert_eq, fmt::Display, matches, str::FromStr};
use log::*;
use serde::{Deserialize, Serialize};
use std::io::Cursor;
use thiserror::Error;

/// Action inside the vault tx
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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

impl Display for VaultAction {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

#[derive(Debug, Clone, Error)]
#[error("Unknown vault action {0}")]
pub struct UnknownVaultActionStr(String);

impl FromStr for VaultAction {
    type Err = UnknownVaultActionStr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "open" => Ok(VaultAction::Open),
            "deposit" => Ok(VaultAction::Deposit),
            "withdraw" => Ok(VaultAction::Withdraw),
            "borrow" => Ok(VaultAction::Borrow),
            "repay" => Ok(VaultAction::Repay),
            _ => Err(UnknownVaultActionStr(s.to_owned())),
        }
    }
}

impl VaultAction {
    pub fn to_protocol(self) -> u8 {
        self as u8
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

    pub fn to_str(self) -> &'static str {
        match self {
            VaultAction::Open => "open",
            VaultAction::Deposit => "deposit",
            VaultAction::Withdraw => "withdraw",
            VaultAction::Borrow => "borrow",
            VaultAction::Repay => "repay",
        }
    }

    // Which operations we consider as increase of volume or decrease
    pub fn unit_volume_sign(self) -> i32 {
        match self {
            VaultAction::Repay => -1,
            VaultAction::Open | VaultAction::Borrow => 1,
            _ => 1,
        }
    }
}

/// Known versions of vault transaction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VaultVersion {
    // Doesn't have liquidation price and hash. Also oracle price and timestamp are swapped.
    Vault1Legacy,
    // The new format
    Vault1,
}

impl VaultVersion {
    pub fn to_protocol(&self) -> u8 {
        match self {
            VaultVersion::Vault1Legacy => 1,
            VaultVersion::Vault1 => 1,
        }
    }

    pub fn from_protocol(v: u8) -> Option<Self> {
        match v {
            1 => Some(VaultVersion::Vault1),
            _ => None,
        }
    }

    pub fn to_str(&self) -> &str {
        match self {
            VaultVersion::Vault1Legacy => "1_legacy",
            VaultVersion::Vault1 => "1",
        }
    }
}

impl Display for VaultVersion {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

#[derive(Debug, Clone, Error)]
#[error("Unknown vault protocol version {0}")]
pub struct UnknownVaultVersionStr(String);

impl FromStr for VaultVersion {
    type Err = UnknownVaultVersionStr;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "1_legacy" => Ok(VaultVersion::Vault1Legacy),
            "1" => Ok(VaultVersion::Vault1),
            _ => Err(UnknownVaultVersionStr(s.to_owned())),
        }
    }
}

/// Fixed point (2 decimals) amount of stable units
pub type UnitAmount = u32;

/// TODO
pub type OraclePrice = u32;

/// Length of liquidation hash in bytes
pub const LIQUIDATION_HASH_LEN: usize = 20;

/// Liquidation hash stored in byte array
pub type LiquidationHash = [u8; LIQUIDATION_HASH_LEN];

/// Vault id is a opening transaction ID
pub type VaultId = Txid;

/// Contains metadata about the vault transaction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct VaultTx {
    /// Hash of transaction where we found the vault tx
    pub txid: Txid,
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
}

#[derive(Debug, Error, PartialEq)]
pub enum VaultParseError {
    #[error("No OP_RETURN output detected")]
    NoOpReturn,
    #[error("No OP_PUSHNUM_8 after OP_RETURN")]
    NoOpPush8,
    #[error("Expected OP_PUSHNUM_8 but got opcode {0}")]
    MismatchOpPush8(u8),
    #[error("No OP_PUSHBYTES_14 after OP_RETURN")]
    NoOpPushbytes14,
    #[error("Expected OP_PUSHBYTES_14 or OP_PUSHBYTES_38 but got opcode {0}")]
    MismatchOpPushbytes(u8),
    #[error("Missing {0} field")]
    MissingField(MissingVaultField),
    #[error("Not expected version {0}")]
    WrongVersion(u8),
    #[error("Not expected action {0}")]
    WrongAction(u8),
    #[error("Liquidation hash has unexpected length (not 20): {0}")]
    LiquidationHashInvalidLength(usize),
}

impl VaultParseError {
    /// Helps detect which transactions are possible vault but we incorrectly parse them
    pub fn is_definetely_not_vault(&self) -> bool {
        matches!(
            *self,
            Self::NoOpReturn | Self::NoOpPush8 | Self::MismatchOpPush8(_) | Self::NoOpPushbytes14
        )
    }
}

#[derive(Debug, Error)]
pub enum TxParseError {
    #[error("Cannot decode Bitcoin transaction: {0}")]
    InvalidParentTx(#[from] bitcoin::consensus::encode::Error),
    #[error("Transaction is not Vault tx: {0}")]
    NotVaultTx(VaultParseError),
}

impl VaultTx {
    /// Detect and parse the vault transaction from the given bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, TxParseError> {
        // Decode vessel bitcoin transaction
        let tx: Transaction = Transaction::consensus_decode(&mut Cursor::new(bytes))?;
        VaultTx::from_tx(&tx).map_err(TxParseError::NotVaultTx)
    }

    /// Detect and parse the vault transaction from the given Bitcoin vessel transaction
    pub fn from_tx(tx: &Transaction) -> Result<Self, VaultParseError> {
        // Find first op_return
        let (out_i, op_return_out): (usize, &Script) = tx
            .output
            .iter()
            .enumerate()
            .map(|(i, out)| (i, out.script_pubkey.as_script()))
            .find(|(_, out)| out.is_op_return())
            .ok_or(VaultParseError::NoOpReturn)?;

        // Now let parse instructions one by one
        let mut instructions = op_return_out.bytes();
        // Skip op_return
        let op_return: u8 = instructions.next().ok_or(VaultParseError::NoOpReturn)?;
        assert_eq!(op_return, OP_RETURN.to_u8()); // if fires, the is_op_return fn is a lyier

        // Skip OP_PUSHNUM_8
        let op_pushnum_8: u8 = instructions.next().ok_or(VaultParseError::NoOpPush8)?;
        if op_pushnum_8 != OP_PUSHNUM_8.to_u8() {
            return Err(VaultParseError::MismatchOpPush8(op_pushnum_8));
        }

        // Skip op_push bytes 14 or 38 bytes
        let op_pushbytes: u8 = instructions
            .next()
            .ok_or(VaultParseError::NoOpPushbytes14)?;
        if op_pushbytes != OP_PUSHBYTES_14.to_u8() && op_pushbytes != OP_PUSHBYTES_38.to_u8() {
            return Err(VaultParseError::MismatchOpPushbytes(op_pushbytes));
        }
        // We distinguish the new format from legacy by length of the payload
        let is_new_format = op_pushbytes == OP_PUSHBYTES_38.to_u8();

        // Parse version field
        let version_code: u8 = instructions
            .next()
            .ok_or(VaultParseError::MissingField(MissingVaultField::Version))?;
        let version = VaultVersion::from_protocol(version_code)
            .ok_or(VaultParseError::WrongVersion(version_code))?;

        // Parse action field
        let action_code: u8 = instructions
            .next()
            .ok_or(VaultParseError::MissingField(MissingVaultField::Action))?;
        let action = VaultAction::from_protocol(action_code)
            .ok_or(VaultParseError::WrongAction(action_code))?;

        // Fetch units balance
        let balance = instructions
            .next_u32_be()
            .ok_or(VaultParseError::MissingField(MissingVaultField::Balance))?;

        // The new format (that is longer) has first price and timestamp, legacy has reverse
        let (oracle_price, oracle_timestamp) = if is_new_format {
            // Fetch oracle price
            let oracle_price = instructions
                .next_u32_be()
                .ok_or(VaultParseError::MissingField(
                    MissingVaultField::OraclePrice,
                ))?;
            // Fetch oracle timestamp
            let oracle_timestamp =
                instructions
                    .next_u32_be()
                    .ok_or(VaultParseError::MissingField(
                        MissingVaultField::OracleTimestamp,
                    ))?;
            (oracle_price, oracle_timestamp)
        } else {
            // Fetch oracle timestamp
            let oracle_timestamp =
                instructions
                    .next_u32_be()
                    .ok_or(VaultParseError::MissingField(
                        MissingVaultField::OracleTimestamp,
                    ))?;
            // Fetch oracle price
            let oracle_price = instructions
                .next_u32_be()
                .ok_or(VaultParseError::MissingField(
                    MissingVaultField::OraclePrice,
                ))?;

            (oracle_price, oracle_timestamp)
        };

        // Fetch liqudation price
        let liquidation_price = instructions.next_u32_be();

        // Take remaining bytes as hash
        let bytes_left = instructions.len();
        if bytes_left != 0 && bytes_left != LIQUIDATION_HASH_LEN {
            return Err(VaultParseError::LiquidationHashInvalidLength(bytes_left));
        }
        let liquidation_hash = instructions.next20();

        Ok(VaultTx {
            txid: tx.compute_txid(),
            output: out_i as u32,
            version: match version {
                VaultVersion::Vault1 if !is_new_format => VaultVersion::Vault1Legacy,
                _ => version,
            },
            action,
            balance,
            oracle_price,
            oracle_timestamp,
            liquidation_price,
            liquidation_hash,
        })
    }
}

#[derive(Debug, Error)]
pub enum AssumeCustodyErr {
    #[error("Open transaction {0} has no custody output")]
    Open(Txid),
    #[error("Deposit transaction {0} has no outputs for custody")]
    Deposit(Txid),
    #[error("Withdraw transaction {0} has no outputs for custody")]
    Withdraw(Txid),
}

impl VaultTx {
    /// Try assume BTC amount held inside the custody.
    pub fn assume_custody_value(&self, tx: &Transaction) -> Result<u64, AssumeCustodyErr> {
        match self.action {
            VaultAction::Open => {
                // First output and second outputs look like a UTXO connectors or inscriptions, so assume 3rd one is usually a custody
                let custody_output: &TxOut = tx
                    .output
                    .get(2)
                    .ok_or(AssumeCustodyErr::Open(tx.compute_txid()))?;
                Ok(custody_output.value.to_sat())
            }
            VaultAction::Deposit
            | VaultAction::Withdraw
            | VaultAction::Borrow
            | VaultAction::Repay => {
                // First output looks like volume of custody (same script)
                let cur_custody: &TxOut = tx
                    .output
                    .first()
                    .ok_or(AssumeCustodyErr::Deposit(tx.compute_txid()))?;

                Ok(cur_custody.value.to_sat())
            }
        }
    }
}

#[derive(Debug, Error)]
pub enum AssumeUnitTxErr {
    #[error("There is no UTXO connector in the inputs (should be at index {CONNECTOR_INPUT_POS}) in {0} vault tx: {1}")]
    Connector(VaultAction, Txid),
}

// Assume that is always 2nd one
const CONNECTOR_INPUT_POS: usize = 1;

impl VaultTx {
    /// Try to assume which input is related to the parent phase 1 transaction that contains UNIT amounts
    pub fn assume_parent_unit_tx(&self, tx: &Transaction) -> Result<Option<Txid>, AssumeUnitTxErr> {
        match self.action {
            VaultAction::Open | VaultAction::Borrow | VaultAction::Repay => {
                let connector_input: &TxIn = tx
                    .input
                    .get(CONNECTOR_INPUT_POS)
                    .ok_or(AssumeUnitTxErr::Connector(self.action, tx.compute_txid()))?;
                Ok(Some(connector_input.previous_output.txid))
            }
            VaultAction::Deposit | VaultAction::Withdraw => Ok(None),
        }
    }
}

trait BytesParser {
    fn next4(&mut self) -> Option<[u8; 4]>;

    fn next20(&mut self) -> Option<[u8; 20]>;

    fn next_u32_be(&mut self) -> Option<u32> {
        self.next4().map(u32::from_be_bytes)
    }
}

impl<T: Iterator<Item = u8>> BytesParser for T {
    fn next4(&mut self) -> Option<[u8; 4]> {
        let mut buf = [0u8; 4];
        for item in &mut buf {
            *item = self.next()?;
        }
        Some(buf)
    }

    fn next20(&mut self) -> Option<[u8; 20]> {
        let mut buf = [0u8; 20];
        for item in &mut buf {
            *item = self.next()?;
        }
        Some(buf)
    }
}
