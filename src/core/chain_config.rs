use serde::{Deserialize, Serialize};
pub const PROTOCOL_VERSION: u32 = 1;

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, clap::ValueEnum, Default,
)]
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
            Network::Mainnet => MAINNET_BOOTNODES.iter().map(|s| s.to_string()).collect(),
            Network::Testnet => TESTNET_BOOTNODES.iter().map(|s| s.to_string()).collect(),
            Network::Devnet => DEVNET_BOOTNODES.iter().map(|s| s.to_string()).collect(),
        }
    }

    pub fn fallback_bootnodes(&self) -> Vec<String> {
        match self {
            Network::Mainnet => MAINNET_FALLBACK_BOOTNODES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            Network::Testnet => TESTNET_FALLBACK_BOOTNODES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            Network::Devnet => Vec::new(),
        }
    }

    pub fn dns_seeds(&self) -> Vec<String> {
        match self {
            Network::Mainnet => MAINNET_DNS_SEEDS.iter().map(|s| s.to_string()).collect(),
            Network::Testnet => TESTNET_DNS_SEEDS.iter().map(|s| s.to_string()).collect(),
            Network::Devnet => Vec::new(),
        }
    }

    pub fn from_chain_id(chain_id: u64) -> Option<Self> {
        match chain_id {
            1 => Some(Network::Mainnet),
            42 => Some(Network::Testnet),
            1337 => Some(Network::Devnet),
            _ => None,
        }
    }

    pub fn epoch_len(&self) -> u64 {
        self.consensus_params().epoch_len
    }

    pub fn min_stake(&self) -> u64 {
        self.consensus_params().min_stake
    }

    pub fn finality_quorum(&self) -> (u64, u64) {
        let params = self.consensus_params();
        (
            params.finality_quorum_numerator,
            params.finality_quorum_denominator,
        )
    }

    pub fn slot_ms(&self) -> u64 {
        self.consensus_params().slot_ms
    }

    pub fn consensus_params(&self) -> ConsensusParams {
        match self {
            Network::Mainnet => ConsensusParams {
                epoch_len: 100,
                min_stake: 1_000_000,
                slot_ms: 6_000,
                finality_checkpoint_interval: 10,
                finality_quorum_numerator: 2,
                finality_quorum_denominator: 3,
                max_votes_per_msg: 128,
            },
            Network::Testnet => ConsensusParams {
                epoch_len: 50,
                min_stake: 10_000,
                slot_ms: 3_000,
                finality_checkpoint_interval: 5,
                finality_quorum_numerator: 2,
                finality_quorum_denominator: 3,
                max_votes_per_msg: 128,
            },
            Network::Devnet => ConsensusParams {
                epoch_len: 10,
                min_stake: 1_000,
                slot_ms: 1_000,
                finality_checkpoint_interval: 2,
                finality_quorum_numerator: 1,
                finality_quorum_denominator: 2,
                max_votes_per_msg: 64,
            },
        }
    }

    pub fn mempool_config(&self) -> crate::mempool::pool::MempoolConfig {
        match self {
            Network::Mainnet => crate::mempool::pool::MempoolConfig {
                max_size: 100_000,
                max_per_sender: 100,
                min_fee: 10,
                tx_ttl_secs: 1_800,
                rbf_bump_percent: 15,
            },
            Network::Testnet => crate::mempool::pool::MempoolConfig {
                max_size: 50_000,
                max_per_sender: 200,
                min_fee: 1,
                tx_ttl_secs: 3_600,
                rbf_bump_percent: 10,
            },
            Network::Devnet => crate::mempool::pool::MempoolConfig::default(),
        }
    }

    pub fn security_config(&self) -> SecurityConfig {
        match self {
            Network::Mainnet => SecurityConfig {
                max_peers: 100,
                peer_rate_limit_per_minute: 120,
                rpc_rate_limit_per_minute: 300,
                rpc_auth_required: true,
                persist_banned_peers: true,
                mdns_enabled: false,
            },
            Network::Testnet => SecurityConfig {
                max_peers: 75,
                peer_rate_limit_per_minute: 240,
                rpc_rate_limit_per_minute: 600,
                rpc_auth_required: true,
                persist_banned_peers: true,
                mdns_enabled: false,
            },
            Network::Devnet => SecurityConfig {
                max_peers: 25,
                peer_rate_limit_per_minute: 1_000,
                rpc_rate_limit_per_minute: 10_000,
                rpc_auth_required: false,
                persist_banned_peers: false,
                mdns_enabled: true,
            },
        }
    }

    pub fn magic_bytes(&self) -> [u8; 4] {
        match self {
            Network::Mainnet => *b"BDLM",
            Network::Testnet => *b"BDLT",
            Network::Devnet => *b"BDLD",
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

const MAINNET_BOOTNODES: &[&str] = &[];
const TESTNET_BOOTNODES: &[&str] = &[];
const DEVNET_BOOTNODES: &[&str] = &[];
const MAINNET_FALLBACK_BOOTNODES: &[&str] = &[];
const TESTNET_FALLBACK_BOOTNODES: &[&str] = &[];
const MAINNET_DNS_SEEDS: &[&str] = &[];
const TESTNET_DNS_SEEDS: &[&str] = &[];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsensusParams {
    pub epoch_len: u64,
    pub min_stake: u64,
    pub slot_ms: u64,
    pub finality_checkpoint_interval: u64,
    pub finality_quorum_numerator: u64,
    pub finality_quorum_denominator: u64,
    pub max_votes_per_msg: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityConfig {
    pub max_peers: usize,
    pub peer_rate_limit_per_minute: u64,
    pub rpc_rate_limit_per_minute: u64,
    pub rpc_auth_required: bool,
    pub persist_banned_peers: bool,
    pub mdns_enabled: bool,
}

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
