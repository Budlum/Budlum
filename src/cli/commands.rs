use crate::core::address::Address;
use crate::core::chain_config::Network;
use clap::Parser;
use std::path::Path;
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum, Default)]
pub enum ConsensusType {
    #[default]
    #[value(name = "pow")]
    PoW,
    #[value(name = "pos")]
    PoS,
    #[value(name = "poa")]
    PoA,
}
impl std::fmt::Display for ConsensusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusType::PoW => write!(f, "PoW (Proof of Work)"),
            ConsensusType::PoS => write!(f, "PoS (Proof of Stake)"),
            ConsensusType::PoA => write!(f, "PoA (Proof of Authority)"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum, Default)]
pub enum PrivacyLevel {
    #[default]
    #[value(name = "none")]
    None,
    #[value(name = "stealth")]
    Stealth,
    #[value(name = "confidential")]
    Confidential,
    #[value(name = "full")]
    Full,
}
impl std::fmt::Display for PrivacyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrivacyLevel::None => write!(f, "None (Public)"),
            PrivacyLevel::Stealth => write!(f, "Stealth Addresses"),
            PrivacyLevel::Confidential => write!(f, "Confidential Transactions"),
            PrivacyLevel::Full => write!(f, "Full Privacy"),
        }
    }
}
#[derive(Parser, Debug)]
#[command(name = "budlum-core")]
#[command(about = "Budlum privacy-focused blockchain node")]
pub struct NodeConfig {
    #[arg(long, default_value = "devnet")]
    pub network: Network,
    #[arg(long)]
    pub consensus: Option<ConsensusType>,
    #[arg(long, default_value = "2")]
    pub difficulty: usize,
    #[arg(long, default_value = "1000")]
    pub min_stake: u64,
    #[arg(long, default_value = "none")]
    pub privacy: PrivacyLevel,
    #[arg(long, default_value = "11")]
    pub ring_size: usize,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long)]
    pub bootstrap: Option<String>,
    #[arg(skip)]
    pub bootnodes: Vec<String>,
    #[arg(long, default_value = "./data/budlum.db")]
    pub db_path: String,
    #[arg(long, default_value = "./validators.json")]
    pub validators_file: String,
    #[arg(long)]
    pub validator_address: Option<String>,
    #[arg(long)]
    pub dial: Option<String>,
    #[arg(long)]
    pub chain_id: Option<u64>,
    #[arg(long)]
    pub validator_key_file: Option<String>,
    #[arg(long)]
    pub gen_key: Option<String>,
    #[arg(long, default_value = "127.0.0.1")]
    pub rpc_host: String,
    #[arg(long, default_value = "8545")]
    pub rpc_port: u16,
    #[arg(long)]
    pub config: Option<String>,
    #[arg(long, default_value = "9090")]
    pub metrics_port: u16,
    #[arg(long, default_value = "validators.json")]
    pub validators_file_cli: Option<String>,
    #[arg(long)]
    pub check_db: bool,
    #[arg(long)]
    pub repair_db: bool,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            network: Network::Devnet,
            consensus: None,
            difficulty: 2,
            min_stake: 1000,
            privacy: PrivacyLevel::None,
            ring_size: 11,
            port: None,
            bootstrap: None,
            bootnodes: Vec::new(),
            db_path: "./data/budlum.db".to_string(),
            validators_file: "./validators.json".to_string(),
            validator_address: None,
            dial: None,
            chain_id: None,
            validator_key_file: None,
            gen_key: None,
            rpc_host: "127.0.0.1".to_string(),
            rpc_port: 8545,
            config: None,
            metrics_port: 9090,
            validators_file_cli: None,
            check_db: false,
            repair_db: false,
        }
    }
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct FileConfig {
    pub network: Option<NetworkSection>,
    pub consensus: Option<ConsensusSection>,
    pub bootnodes: Option<BootnodesSection>,
    pub rpc: Option<RpcSection>,
    pub metrics: Option<MetricsSection>,
    pub storage: Option<StorageSection>,
    pub validator: Option<ValidatorSection>,
    pub security: Option<SecuritySection>,
    pub node: Option<NodeSection>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct NetworkSection {
    pub name: Option<String>,
    pub chain_id: Option<u64>,
    pub port: Option<u16>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct ConsensusSection {
    #[serde(rename = "type")]
    pub consensus_type: Option<String>,
    pub min_stake: Option<u64>,
    pub epoch_len: Option<u64>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct BootnodesSection {
    pub addresses: Option<Vec<String>>,
    pub fallback: Option<Vec<String>>,
    pub dns_seeds: Option<Vec<String>>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct RpcSection {
    pub enabled: Option<bool>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub auth_required: Option<bool>,
    pub api_key_env: Option<String>,
    pub allowed_ips: Option<Vec<String>>,
    pub cors_origins: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<u64>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct MetricsSection {
    pub port: Option<u16>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct StorageSection {
    pub db_path: Option<String>,
    pub snapshot_dir: Option<String>,
    pub backups_enabled: Option<bool>,
    pub backup_dir: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct ValidatorSection {
    pub key_file: Option<String>,
    pub address: Option<String>,
    pub backend: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct SecuritySection {
    pub max_peers: Option<usize>,
    pub banned_peer_db: Option<String>,
    pub mdns_enabled: Option<bool>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct NodeSection {
    pub dial: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default)]
pub struct LegacyFileConfig {
    pub network: Option<String>,
    pub consensus: Option<String>,
    pub difficulty: Option<usize>,
    pub min_stake: Option<u64>,
    pub port: Option<u16>,
    pub db_path: Option<String>,
    pub rpc_host: Option<String>,
    pub rpc_port: Option<u16>,
    pub metrics_port: Option<u16>,
    pub bootstrap: Option<String>,
    pub validator_key_file: Option<String>,
    pub validator_address: Option<String>,
}

impl NodeConfig {
    pub fn load_with_file(&mut self) {
        if let Some(ref path) = self.config {
            match std::fs::read_to_string(path) {
                Ok(content) => match toml::from_str::<FileConfig>(&content) {
                    Ok(fc) => {
                        if let Some(network) = fc.network {
                            if let Some(name) = network.name {
                                self.network = match name.as_str() {
                                    "mainnet" => Network::Mainnet,
                                    "testnet" => Network::Testnet,
                                    "devnet" => Network::Devnet,
                                    other => {
                                        println!(
                                            "Unknown network '{}' in config, keeping CLI value",
                                            other
                                        );
                                        self.network
                                    }
                                };
                            }
                            if self.chain_id.is_none() {
                                self.chain_id = network.chain_id;
                            }
                            if self.port.is_none() {
                                self.port = network.port;
                            }
                        }
                        if let Some(consensus) = fc.consensus {
                            if self.consensus.is_none() {
                                self.consensus = consensus.consensus_type.as_deref().and_then(|s| match s {
                                    "pow" => Some(ConsensusType::PoW),
                                    "pos" => Some(ConsensusType::PoS),
                                    "poa" => Some(ConsensusType::PoA),
                                    other => {
                                        println!("Unknown consensus '{}' in config, keeping CLI value", other);
                                        None
                                    }
                                });
                            }
                            if self.min_stake == 1000 {
                                if let Some(min_stake) = consensus.min_stake {
                                    self.min_stake = min_stake;
                                }
                            }
                        }
                        if let Some(bootnodes) = fc.bootnodes {
                            if self.bootnodes.is_empty() {
                                if let Some(addresses) = bootnodes.addresses {
                                    self.bootnodes.extend(addresses);
                                }
                                if let Some(fallback) = bootnodes.fallback {
                                    self.bootnodes.extend(fallback);
                                }
                            }
                            if self.bootstrap.is_none() {
                                self.bootstrap = self.bootnodes.first().cloned();
                            }
                        }
                        if let Some(rpc) = fc.rpc {
                            if let Some(host) = rpc.host {
                                if self.rpc_host == "127.0.0.1" || self.rpc_host.is_empty() {
                                    self.rpc_host = host;
                                }
                            }
                            if let Some(port) = rpc.port {
                                if self.rpc_port == 8545 {
                                    self.rpc_port = port;
                                }
                            }
                        }
                        if let Some(metrics) = fc.metrics {
                            if let Some(port) = metrics.port {
                                if self.metrics_port == 9090 {
                                    self.metrics_port = port;
                                }
                            }
                        }
                        if let Some(storage) = fc.storage {
                            if let Some(db) = storage.db_path {
                                if self.db_path == "./data/budlum.db" || self.db_path.is_empty() {
                                    self.db_path = db;
                                }
                            }
                        }
                        if let Some(validator) = fc.validator {
                            if self.validator_key_file.is_none() {
                                self.validator_key_file = validator.key_file;
                            }
                            if self.validator_address.is_none() {
                                self.validator_address = validator.address;
                            }
                        }
                        if let Some(node) = fc.node {
                            if self.dial.is_none() {
                                self.dial = node.dial;
                            }
                        }
                        if let Ok(legacy) = toml::from_str::<LegacyFileConfig>(&content) {
                            if self.port.is_none() {
                                self.port = legacy.port;
                            }
                            if self.bootstrap.is_none() {
                                self.bootstrap = legacy.bootstrap;
                            }
                            if self.validator_key_file.is_none() {
                                self.validator_key_file = legacy.validator_key_file;
                            }
                            if self.validator_address.is_none() {
                                self.validator_address = legacy.validator_address;
                            }
                            if let Some(ref db) = legacy.db_path {
                                if self.db_path == "./data/budlum.db" || self.db_path.is_empty() {
                                    self.db_path = db.clone();
                                }
                            }
                            if let Some(ref host) = legacy.rpc_host {
                                if self.rpc_host == "127.0.0.1" || self.rpc_host.is_empty() {
                                    self.rpc_host = host.clone();
                                }
                            }
                            if let Some(rp) = legacy.rpc_port {
                                if self.rpc_port == 8545 {
                                    self.rpc_port = rp;
                                }
                            }
                            if let Some(mp) = legacy.metrics_port {
                                if self.metrics_port == 9090 {
                                    self.metrics_port = mp;
                                }
                            }
                        }
                        println!("Loaded config from: {}", path);
                    }
                    Err(e) => println!("Failed to parse config file: {}", e),
                },
                Err(e) => println!("Failed to read config file: {}", e),
            }
        }
    }
    pub fn load_validators(&self) -> Vec<String> {
        let path = Path::new(&self.validators_file);
        if !path.exists() {
            println!(" Validators file not found: {}", self.validators_file);
            return vec![];
        }
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<ValidatorsConfig>(&content) {
                Ok(config) => {
                    println!(
                        "Loaded {} validators from {}",
                        config.validators.len(),
                        self.validators_file
                    );
                    config.validators
                }
                Err(e) => {
                    println!("Failed to parse validators file: {}", e);
                    vec![]
                }
            },
            Err(e) => {
                println!("Failed to read validators file: {}", e);
                vec![]
            }
        }
    }

    pub fn load_validator_addresses(&self) -> Vec<Address> {
        self.load_validators()
            .into_iter()
            .filter_map(|validator| match Address::from_hex(&validator) {
                Ok(address) => Some(address),
                Err(err) => {
                    println!("Skipping invalid validator address {}: {}", validator, err);
                    None
                }
            })
            .collect()
    }
}
#[derive(Debug, serde::Deserialize)]
struct ValidatorsConfig {
    validators: Vec<String>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_consensus_type_parsing() {
        assert_eq!(ConsensusType::PoW as u8, 0);
    }
}
