use serde::{Deserialize, Serialize};
pub const PROTOCOL_VERSION: u32 = 1;
pub const CHAIN_ID_MAINNET: u64 = 1;
pub const CHAIN_ID_TESTNET: u64 = 42;
pub const CHAIN_ID_DEVNET: u64 = 1337;

pub const EPOCH_LEN: u64 = 100;
pub const SLOT_MS: u64 = 1000;
pub const FINALITY_CHECKPOINT_INTERVAL: u64 = 100;
pub const FINALITY_QUORUM_NUMERATOR: u64 = 2;
pub const FINALITY_QUORUM_DENOMINATOR: u64 = 3;
pub const VRF_BASE_PROB: f64 = 1.0;
pub const QC_BLOB_TTL_EPOCHS: u64 = 10;
pub const MAX_QC_BLOB_BYTES: usize = 1_048_576;
pub const MAX_VOTES_PER_MSG: usize = 128;
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChainId(pub u64);
impl ChainId {
    pub const MAINNET: ChainId = ChainId(CHAIN_ID_MAINNET);
    pub const TESTNET: ChainId = ChainId(CHAIN_ID_TESTNET);
    pub const DEVNET: ChainId = ChainId(CHAIN_ID_DEVNET);
    pub fn new(value: u64) -> Self {
        ChainId(value)
    }
    pub fn value(&self) -> u64 {
        self.0
    }
    pub fn name(&self) -> &'static str {
        match self.0 {
            1 => "mainnet",
            42 => "testnet",
            1337 => "devnet",
            _ => "custom",
        }
    }
}
impl Default for ChainId {
    fn default() -> Self {
        ChainId::DEVNET
    }
}
impl From<u64> for ChainId {
    fn from(value: u64) -> Self {
        ChainId(value)
    }
}
impl std::fmt::Display for ChainId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}({})", self.name(), self.0)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_chain_id_values() {
        assert_eq!(ChainId::MAINNET.value(), 1);
        assert_eq!(ChainId::TESTNET.value(), 42);
        assert_eq!(ChainId::DEVNET.value(), 1337);
        assert_eq!(ChainId::new(999).value(), 999);
    }
    #[test]
    fn test_chain_id_display() {
        assert_eq!(format!("{}", ChainId::MAINNET), "mainnet(1)");
        assert_eq!(format!("{}", ChainId::new(123)), "custom(123)");
    }
}
