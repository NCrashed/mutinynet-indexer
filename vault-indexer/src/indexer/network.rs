use clap::ValueEnum;
use core::{fmt::Display, str::FromStr};
use std::io::Cursor;
use thiserror::Error;

use bitcoin::{block::Header, consensus::Decodable, constants::genesis_block, p2p::Magic};

// Extract from: btc-cli getblockheader 00000008819873e925422c1ff0f99f7cc9bbb232af63a077a480a3633bee1ef6 false
const MUTINY_SIGNET_GENESIS_HEADER: [u8; 80] = [
    0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x3b, 0xa3, 0xed, 0xfd, 0x7a, 0x7b, 0x12, 0xb2, 0x7a, 0xc7, 0x2c, 0x3e,
    0x67, 0x76, 0x8f, 0x61, 0x7f, 0xc8, 0x1b, 0xc3, 0x88, 0x8a, 0x51, 0x32, 0x3a, 0x9f, 0xb8, 0xaa,
    0x4b, 0x1e, 0x5e, 0x4a, 0x00, 0x8f, 0x4d, 0x5f, 0xae, 0x77, 0x03, 0x1e, 0x8a, 0xd2, 0x22, 0x03,
];

/// Extended network enum that includes also the Mutiny signet
#[derive(Copy, PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug, ValueEnum)]
pub enum Network {
    /// Mainnet Bitcoin.
    Bitcoin,
    /// Bitcoin's testnet network.
    Testnet,
    /// Bitcoin's testnet4 network.
    Testnet4,
    /// Bitcoin's signet network.
    Signet,
    /// Mutiny custom signet network.
    Mutinynet,
    /// Bitcoin's regtest network.
    Regtest,
}

impl Display for Network {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.to_str())
    }
}

#[derive(Debug, Error)]
#[error("Unknown network {0}")]
pub struct NetworkFromStrErr(String);

impl FromStr for Network {
    type Err = NetworkFromStrErr;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value.to_lowercase().as_str() {
            "bitcoin" => Ok(Network::Bitcoin),
            "testnet" => Ok(Network::Testnet),
            "testnet4" => Ok(Network::Testnet4),
            "signet" => Ok(Network::Signet),
            "mutinynet" => Ok(Network::Mutinynet),
            "regtest" => Ok(Network::Regtest),
            _ => Err(NetworkFromStrErr(value.to_owned())),
        }
    }
}

impl Network {
    /// Convert to human readable format.
    ///
    /// Property:
    /// `from_str(v.to_str()) == v`
    pub fn to_str(&self) -> &str {
        match self {
            Network::Bitcoin => "bitcoin",
            Network::Testnet => "testnet",
            Network::Testnet4 => "testnet4",
            Network::Signet => "signet",
            Network::Mutinynet => "mutinynet",
            Network::Regtest => "regtest",
        }
    }

    /// Return the network magic bytes, which should be encoded little-endian
    /// at the start of every message
    ///
    /// # Examples
    ///
    /// ```rust
    /// use bitcoin::p2p::Magic;
    /// use bitcoin::Network;
    ///
    /// let network = Network::Bitcoin;
    /// assert_eq!(network.magic(), Magic::from_bytes([0xF9, 0xBE, 0xB4, 0xD9]));
    /// ```
    pub fn magic(self) -> Magic {
        match self {
            Network::Bitcoin => Magic::from(bitcoin::Network::Bitcoin),
            Network::Testnet => Magic::from(bitcoin::Network::Testnet),
            Network::Testnet4 => Magic::from(bitcoin::Network::Testnet4),
            Network::Signet => Magic::from(bitcoin::Network::Signet),
            Network::Regtest => Magic::from(bitcoin::Network::Regtest),
            Network::Mutinynet => Magic::from_bytes([0xa5, 0xdf, 0x2d, 0xcb]), // debug.log search for Signet derived magic (message start): a5df2dcb
        }
    }

    /// Get header of genesis block for given chain
    pub fn genesis_header(self) -> Header {
        match self {
            Network::Bitcoin => genesis_block(bitcoin::Network::Bitcoin).header,
            Network::Testnet => genesis_block(bitcoin::Network::Testnet).header,
            Network::Testnet4 => genesis_block(bitcoin::Network::Testnet4).header,
            Network::Signet => genesis_block(bitcoin::Network::Signet).header,
            Network::Regtest => genesis_block(bitcoin::Network::Regtest).header,
            Network::Mutinynet => {
                Header::consensus_decode(&mut Cursor::new(MUTINY_SIGNET_GENESIS_HEADER))
                    .expect("Mutinynet genesis block decode")
            }
        }
    }
}
