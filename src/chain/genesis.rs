use crate::core::address::Address;
use crate::core::block::{Block, DEFAULT_CHAIN_ID};
use crate::core::chain_config::Network;
use crate::core::transaction::Transaction;
use serde::{Deserialize, Serialize};

pub const BLOCK_REWARD: u64 = 50;

pub const BASE_FEE: u64 = 1;

pub const GENESIS_ALLOCATION: u64 = 1_000_000_000;

pub const GENESIS_TIMESTAMP: u128 = 0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisConfig {
    pub chain_id: u64,

    pub allocations: Vec<(Address, u64)>,

    pub validators: Vec<Address>,

    pub block_reward: u64,

    pub base_fee: u64,

    pub gas_schedule: crate::core::transaction::GasSchedule,

    pub timestamp: u128,
}

impl Default for GenesisConfig {
    fn default() -> Self {
        GenesisConfig {
            chain_id: DEFAULT_CHAIN_ID,
            allocations: vec![],
            validators: vec![],
            block_reward: BLOCK_REWARD,
            base_fee: BASE_FEE,
            gas_schedule: Network::Devnet.gas_schedule(),
            timestamp: GENESIS_TIMESTAMP,
        }
    }
}

impl GenesisConfig {
    pub fn new(chain_id: u64) -> Self {
        GenesisConfig {
            chain_id,
            ..Default::default()
        }
    }

    pub fn for_network(network: Network) -> Self {
        match network {
            Network::Mainnet => mainnet_genesis(),
            Network::Testnet => testnet_genesis(),
            Network::Devnet => devnet_genesis(),
        }
    }

    pub fn with_allocation(mut self, address: Address, amount: u64) -> Self {
        self.allocations.push((address, amount));
        self
    }

    pub fn with_validator(mut self, address: Address) -> Self {
        self.validators.push(address);
        self
    }

    pub fn build_genesis_block(&self) -> Block {
        let genesis_tx = Transaction::genesis();

        let mut block = Block {
            index: 0,
            timestamp: self.timestamp,
            previous_hash: "0".repeat(64),
            hash: String::new(),
            transactions: vec![genesis_tx],
            nonce: 0,
            producer: None,
            signature: None,
            chain_id: self.chain_id,
            slashing_evidence: None,
            state_root: "0".repeat(64),
            tx_root: "0".repeat(64),
            epoch: 0,
            slot: 0,
            vrf_output: Vec::new(),
            vrf_proof: Vec::new(),
            validator_set_hash: "0".repeat(64),
        };

        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();
        block
    }
}

fn address(byte: u8) -> Address {
    Address::from([byte; 32])
}

pub fn mainnet_genesis() -> GenesisConfig {
    GenesisConfig {
        chain_id: Network::Mainnet.chain_id().value(),
        allocations: vec![(address(0x10), 500_000_000), (address(0x11), 500_000_000)],
        validators: vec![address(0x20), address(0x21), address(0x22), address(0x23)],
        block_reward: 25,
        base_fee: Network::Mainnet.gas_schedule().base_fee,
        gas_schedule: Network::Mainnet.gas_schedule(),
        timestamp: 1_735_689_600_000,
    }
}

pub fn testnet_genesis() -> GenesisConfig {
    GenesisConfig {
        chain_id: Network::Testnet.chain_id().value(),
        allocations: vec![
            (address(0x30), 1_000_000_000),
            (address(0x31), 1_000_000_000),
        ],
        validators: vec![address(0x40), address(0x41), address(0x42)],
        block_reward: 50,
        base_fee: Network::Testnet.gas_schedule().base_fee,
        gas_schedule: Network::Testnet.gas_schedule(),
        timestamp: 1_735_689_600_000,
    }
}

pub fn devnet_genesis() -> GenesisConfig {
    GenesisConfig {
        chain_id: Network::Devnet.chain_id().value(),
        allocations: vec![(address(0x01), GENESIS_ALLOCATION)],
        validators: vec![address(0x02)],
        block_reward: BLOCK_REWARD,
        base_fee: Network::Devnet.gas_schedule().base_fee,
        gas_schedule: Network::Devnet.gas_schedule(),
        timestamp: GENESIS_TIMESTAMP,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = GenesisConfig::default();
        assert_eq!(config.chain_id, DEFAULT_CHAIN_ID);
        assert_eq!(config.block_reward, BLOCK_REWARD);
        assert_eq!(config.base_fee, BASE_FEE);
        assert_eq!(config.timestamp, GENESIS_TIMESTAMP);
    }

    #[test]
    fn test_genesis_deterministic() {
        let config = GenesisConfig::default();
        let genesis1 = config.build_genesis_block();
        let genesis2 = config.build_genesis_block();

        assert_eq!(genesis1.hash, genesis2.hash);
        assert_eq!(genesis1.timestamp, GENESIS_TIMESTAMP);
    }

    #[test]
    fn test_network_genesis_configs_are_distinct() {
        let mainnet = GenesisConfig::for_network(Network::Mainnet);
        let testnet = GenesisConfig::for_network(Network::Testnet);
        let devnet = GenesisConfig::for_network(Network::Devnet);

        assert_ne!(mainnet.chain_id, testnet.chain_id);
        assert_ne!(mainnet.block_reward, devnet.block_reward);
        assert_ne!(mainnet.validators, testnet.validators);
        assert_ne!(mainnet.gas_schedule, testnet.gas_schedule);
    }

    #[test]
    fn test_config_builder() {
        let config = GenesisConfig::new(42)
            .with_allocation(Address::from_hex(&"0".repeat(64)).unwrap(), 1000)
            .with_validator(Address::from_hex(&"1".repeat(64)).unwrap());

        assert_eq!(config.chain_id, 42);
        assert_eq!(config.allocations.len(), 1);
        assert_eq!(config.validators.len(), 1);
    }
}
