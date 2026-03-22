use serde::{Serialize, Deserialize};
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum, Default)]
pub enum Network {
    Mainnet,
    Testnet,
    #[default]
    Devnet,
}

impl Network {
    pub fn chain_id(&self) -> ChainId {
        match self {
            Network::Mainnet => ChainId(1),
            Network::Testnet => ChainId(42),
            Network::Devnet => ChainId(1337),
        }
    }

    pub fn default_port(&self) -> u16 {
        match self {
            Network::Mainnet => 4001,
            Network::Testnet => 5001,
            Network::Devnet => 6001,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Network::Mainnet => "mainnet",
            Network::Testnet => "testnet",
            Network::Devnet => "devnet",
        }
    }

    pub fn bootnodes(&self) -> Vec<String> {
        match self {
            Network::Mainnet => vec![
                "/ip4/1.2.3.4/tcp/4001/p2p/QmMainnetBootnode1".to_string(),
            ],
            Network::Testnet => vec![
                "/ip4/5.6.7.8/tcp/5001/p2p/QmTestnetBootnode1".to_string(),
            ],
            Network::Devnet => vec![],
        }
    }
}

impl std::fmt::Display for Network {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

pub const EPOCH_LEN: u64 = 100;
pub const SLOT_MS: u64 = 1000;
pub const FINALITY_CHECKPOINT_INTERVAL: u64 = 10;
pub const FINALITY_QUORUM_NUMERATOR: u64 = 2;
pub const FINALITY_QUORUM_DENOMINATOR: u64 = 3;
pub const FIXED_POINT_SCALE: u64 = 1_000_000;
pub const VRF_BASE_PROB: u64 = FIXED_POINT_SCALE;
pub const QC_BLOB_TTL_EPOCHS: u64 = 10;
pub const MAX_QC_BLOB_BYTES: usize = 1_048_576;
pub const MAX_VOTES_PER_MSG: usize = 128;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChainId(pub u64);

impl ChainId {
    pub fn new(value: u64) -> Self {
        ChainId(value)
    }
    pub fn value(&self) -> u64 {
        self.0
    }
}

impl Default for ChainId {
    fn default() -> Self {
        Network::Devnet.chain_id()
    }
}

impl From<u64> for ChainId {
    fn from(value: u64) -> Self {
        ChainId(value)
    }
}

impl std::fmt::Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_network_configs() {
        assert_eq!(Network::Mainnet.chain_id().value(), 1);
        assert_eq!(Network::Testnet.chain_id().value(), 42);
        assert_eq!(Network::Devnet.chain_id().value(), 1337);
        assert_eq!(Network::Mainnet.default_port(), 4001);
    }
}
