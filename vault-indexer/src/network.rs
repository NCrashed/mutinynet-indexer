/// Which network we run the indexer on
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Network {
    /// Main network
    Bitcoin,
    /// Also includes Mutiny signet
    Signet,
}
