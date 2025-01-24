use bitcoin::p2p::Magic;

/// Extended network enum that includes also the Mutiny signet
#[derive(Copy, PartialEq, Eq, PartialOrd, Ord, Clone, Hash, Debug)]
pub enum Network {
    /// Mainnet Bitcoin.
    Bitcoin,
    /// Bitcoin's testnet network. (In future versions this will be combined
    /// into a single variant containing the version)
    Testnet,
    /// Bitcoin's testnet4 network. (In future versions this will be combined
    /// into a single variant containing the version)
    Testnet4,
    /// Bitcoin's signet network.
    Signet,
    /// Mutiny custom signet network.
    Mutinynet,
    /// Bitcoin's regtest network.
    Regtest,
}

impl Network {
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
}
