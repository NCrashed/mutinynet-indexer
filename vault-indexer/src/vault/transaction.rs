use core::{fmt::Display, unimplemented};

pub use bitcoin::Wtxid;
use thiserror::Error;

/// Action inside the vault tx
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VaultAction {
    // Open new vault
    Open,
    // Deposit BTC
    Deposit, 
    // Withdraw BTC
    Withdraw,
    // Borrow UNIT
    Borrow, 
    // Repay UNIT
    Repay,
}

/// Known versions of vault transaction
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum VaultVersion {
    Vault1_0 = 1,
    Unknown(u8),
}

impl VaultVersion {
    pub fn to_protocol(&self) -> u8 {
        match self {
            VaultVersion::Vault1_0 => 1, 
            VaultVersion::Unknown(v) => *v,
        }
    }

    pub fn from_protocol(v: u8) -> Self {
        match v {
            1 => VaultVersion::Vault1_0,
            _ => VaultVersion::Unknown(v),
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
pub type LiquidationHash = [u8; 32];

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
    pub liquidation_price: OraclePrice,
    /// Hash of the liquidation
    pub liquidation_hash: LiquidationHash,
}

#[derive(Debug, Error, PartialEq)]
pub enum NotVaultReason {
    #[error("Not expected version {0}")]
    WrongVersion(VaultVersion),
    #[error("No op_return output detected")]
    NoOpReturn,
}

#[derive(Debug, Error, PartialEq)]
pub enum ParseError {
    #[error("Transaction is not Vault tx: {0}")]
    NotVaultTx(NotVaultReason),
}

impl VaultTx {
    /// Detect and parse the vault transaction from the given bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        unimplemented!()
    }
}