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

#[derive(Parser, Debug, Clone)]
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

    #[arg(skip)]
    pub rpc_enabled: bool,

    #[arg(skip)]
    pub rpc_auth_required: bool,

    #[arg(skip)]
    pub rpc_api_key_env: Option<String>,

    #[arg(skip)]
    pub rpc_allowed_ips: Vec<String>,

    #[arg(skip)]
    pub rpc_cors_origins: Vec<String>,

    #[arg(skip)]
    pub rpc_rate_limit_per_minute: Option<u64>,

    #[arg(long, default_value = "validators.json")]
    pub validators_file_cli: Option<String>,

    #[arg(long)]
    pub check_db: bool,

    #[arg(long)]
    pub repair_db: bool,

    // Strict Config V2 Fields
    #[arg(long, default_value = "rpc")]
    pub role: String, // validator | sentry | seed | rpc | archive

    #[arg(long)]
    pub genesis_file: Option<String>,

    #[arg(long)]
    pub data_dir: Option<String>,

    #[arg(long)]
    pub snapshot_dir: Option<String>,

    #[arg(long)]
    pub backup_dir: Option<String>,

    #[arg(long)]
    pub backups_enabled: Option<bool>,

    #[arg(long)]
    pub p2p_identity_file: Option<String>,

    #[arg(long)]
    pub dns_seeds: Vec<String>,

    #[arg(long)]
    pub max_peers: Option<usize>,

    #[arg(long)]
    pub banned_peer_db: Option<String>,

    #[arg(long)]
    pub mdns_enabled: Option<bool>,

    #[arg(long)]
    pub rpc_public_listener: Option<String>,

    #[arg(long)]
    pub rpc_operator_listener: Option<String>,

    #[arg(long)]
    pub rpc_trusted_proxies: Vec<String>,

    #[arg(long)]
    pub metrics_listener: Option<String>,

    #[arg(long)]
    pub signer_backend: Option<String>, // local | softhsm | pkcs11

    #[arg(long)]
    pub pkcs11_module_path: Option<String>,

    #[arg(long)]
    pub pkcs11_slot_id: Option<u64>,

    #[arg(long)]
    pub pkcs11_token_pin_env: Option<String>,

    #[arg(long)]
    pub features_governance: bool,

    #[arg(long)]
    pub features_zkvm_contracts: bool,

    #[arg(long)]
    pub features_pruning: bool,

    /// Archive node: never prune block history, even if pruning is enabled.
    #[arg(long)]
    pub archive_mode: bool,
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
            rpc_enabled: true,
            rpc_auth_required: false,
            rpc_api_key_env: None,
            rpc_allowed_ips: Vec::new(),
            rpc_cors_origins: Vec::new(),
            rpc_rate_limit_per_minute: None,
            validators_file_cli: None,
            check_db: false,
            repair_db: false,
            role: "rpc".to_string(),
            genesis_file: None,
            data_dir: None,
            snapshot_dir: None,
            backup_dir: None,
            backups_enabled: None,
            p2p_identity_file: None,
            dns_seeds: Vec::new(),
            max_peers: None,
            banned_peer_db: None,
            mdns_enabled: None,
            rpc_public_listener: None,
            rpc_operator_listener: None,
            rpc_trusted_proxies: Vec::new(),
            metrics_listener: None,
            signer_backend: None,
            pkcs11_module_path: None,
            pkcs11_slot_id: None,
            pkcs11_token_pin_env: None,
            features_governance: false,
            features_zkvm_contracts: false,
            features_pruning: false,
            archive_mode: false,
        }
    }
}

// Strict Config V2 typed configuration structure.
// Any unknown field will throw a deserialization error.
#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct FileConfig {
    pub network: Option<NetworkSection>,
    pub node: Option<NodeSection>,
    pub storage: Option<StorageSection>,
    pub p2p: Option<P2pSection>,
    pub rpc: Option<RpcSection>,
    pub metrics: Option<MetricsSection>,
    pub validator: Option<ValidatorSection>,
    pub features: Option<FeaturesSection>,
    pub consensus: Option<ConsensusSection>, // for integration of other sections
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct NetworkSection {
    pub profile: Option<String>,
    pub chain_id: Option<u64>,
    pub genesis_file: Option<String>,
    pub name: Option<String>, // backwards-compatibility alias for profile in tests
    pub port: Option<u16>,    // backwards-compatibility alias in tests
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct NodeSection {
    pub role: Option<String>, // validator | sentry | seed | rpc | archive
    pub dial: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct StorageSection {
    pub data_dir: Option<String>,
    pub snapshot_dir: Option<String>,
    pub backup_dir: Option<String>,
    pub backups_enabled: Option<bool>,
    pub db_path: Option<String>, // backwards-compatibility alias
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct P2pSection {
    pub identity_file: Option<String>,
    pub bootnodes: Option<Vec<String>>,
    pub dns_seeds: Option<Vec<String>>,
    pub max_peers: Option<usize>,
    pub banned_peer_db: Option<String>,
    pub mdns_enabled: Option<bool>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct RpcSection {
    pub public_listener: Option<String>,
    pub operator_listener: Option<String>,
    pub trusted_proxies: Option<Vec<String>>,
    pub enabled: Option<bool>,
    pub api_key_env: Option<String>,
    pub host: Option<String>, // backwards-compatibility
    pub port: Option<u16>,    // backwards-compatibility
    pub auth_required: Option<bool>,
    pub allowed_ips: Option<Vec<String>>,
    pub cors_origins: Option<Vec<String>>,
    pub rate_limit_per_minute: Option<u64>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct MetricsSection {
    pub listener: Option<String>,
    pub port: Option<u16>, // backwards-compatibility
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ValidatorSection {
    pub signer: Option<SignerSection>,
    pub key_file: Option<String>, // backwards-compatibility
    pub backend: Option<String>,  // backwards-compatibility
    pub address: Option<String>,  // backwards-compatibility
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct SignerSection {
    pub backend: Option<String>, // local | softhsm | pkcs11
    pub pkcs11: Option<Pkcs11Section>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct Pkcs11Section {
    pub module_path: Option<String>,
    pub slot_id: Option<u64>,
    pub token_pin_env: Option<String>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct FeaturesSection {
    pub governance: Option<bool>,
    pub zkvm_contracts: Option<bool>,
    pub pruning: Option<bool>,
    pub archive_mode: Option<bool>,
}

#[derive(Debug, serde::Deserialize, Default, Clone)]
#[serde(deny_unknown_fields)]
pub struct ConsensusSection {
    #[serde(rename = "type")]
    pub consensus_type: Option<String>,
    pub min_stake: Option<u64>,
    pub epoch_len: Option<u64>,
}

// Backwards-compatibility legacy config format.
// If V2 config parsing fails due to unknown fields, we try legacy config only if it does not contain strict section markers.
#[derive(Debug, serde::Deserialize, Default)]
#[serde(deny_unknown_fields)]
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
        if let Some(path) = self.config.clone() {
            let content = std::fs::read_to_string(&path).unwrap_or_else(|e| {
                eprintln!(
                    "CRITICAL ERROR: Failed to read config file at {}: {}",
                    path, e
                );
                std::process::exit(1);
            });

            // 1. Parse the file first, then overlay environment values.
            match toml::from_str::<FileConfig>(&content) {
                Ok(fc) => {
                    self.apply_file_config(fc);
                    println!("Loaded Strict Config V2 from: {}", path);
                }
                Err(e) => {
                    // Try to parse as legacy config to see if it is valid legacy format
                    match toml::from_str::<LegacyFileConfig>(&content) {
                        Ok(legacy) => {
                            eprintln!("WARNING: Config parsed as legacy format, please migrate to Strict Config V2!");
                            self.apply_legacy_config(legacy);
                        }
                        Err(_) => {
                            eprintln!(
                                "CRITICAL CONFIGURATION ERROR: Failed to parse TOML configuration."
                            );
                            eprintln!("Details: {}", e);
                            eprintln!("Ensure there are no unknown/invalid fields or wrong section blocks.");
                            std::process::exit(1);
                        }
                    }
                }
            }
        }

        // Environment values override file configuration and also work without a config file.
        self.load_from_env();

        // Strict runtime rules apply equally to file-based and CLI-only starts.
        self.validate_strict_rules();

        // Node roles control whether public listeners may start.
        self.apply_role_rules();
    }

    fn apply_file_config(&mut self, fc: FileConfig) {
        if let Some(network) = fc.network {
            let profile_name = network.profile.or(network.name);
            if let Some(profile) = profile_name {
                self.network = match profile.as_str() {
                    "mainnet" => Network::Mainnet,
                    "testnet" => Network::Testnet,
                    "devnet" => Network::Devnet,
                    other => {
                        eprintln!("CRITICAL: Invalid network profile '{}'", other);
                        std::process::exit(1);
                    }
                };
            }
            if self.chain_id.is_none() {
                self.chain_id = network.chain_id;
            }
            if self.port.is_none() {
                self.port = network.port;
            }
            if self.genesis_file.is_none() {
                self.genesis_file = network.genesis_file;
            }
        }

        if let Some(node) = fc.node {
            if let Some(role) = node.role {
                match role.as_str() {
                    "validator" | "sentry" | "seed" | "rpc" | "archive" => {
                        self.role = role;
                    }
                    other => {
                        eprintln!("CRITICAL: Invalid node role '{}'", other);
                        std::process::exit(1);
                    }
                }
            }
            if self.dial.is_none() {
                self.dial = node.dial;
            }
        }

        if let Some(storage) = fc.storage {
            if let Some(data_dir) = storage.data_dir.or(storage.db_path) {
                self.db_path = data_dir.clone();
                self.data_dir = Some(data_dir);
            }
            if self.snapshot_dir.is_none() {
                self.snapshot_dir = storage.snapshot_dir;
            }
            if self.backup_dir.is_none() {
                self.backup_dir = storage.backup_dir;
            }
            if self.backups_enabled.is_none() {
                self.backups_enabled = storage.backups_enabled;
            }
        }

        if let Some(p2p) = fc.p2p {
            if self.p2p_identity_file.is_none() {
                self.p2p_identity_file = p2p.identity_file;
            }
            if self.bootnodes.is_empty() {
                if let Some(addresses) = p2p.bootnodes {
                    self.bootnodes.extend(addresses);
                }
            }
            if self.dns_seeds.is_empty() {
                if let Some(seeds) = p2p.dns_seeds {
                    self.dns_seeds = seeds;
                }
            }
            if self.max_peers.is_none() {
                self.max_peers = p2p.max_peers;
            }
            if self.banned_peer_db.is_none() {
                self.banned_peer_db = p2p.banned_peer_db;
            }
            if self.mdns_enabled.is_none() {
                self.mdns_enabled = p2p.mdns_enabled;
            }
            if self.bootstrap.is_none() {
                self.bootstrap = self.bootnodes.first().cloned();
            }
        }

        if let Some(rpc) = fc.rpc {
            if let Some(enabled) = rpc.enabled {
                self.rpc_enabled = enabled;
            }
            if let Some(api_key_env) = rpc.api_key_env {
                self.rpc_api_key_env = Some(api_key_env);
            }
            if let Some(public_listener) = rpc.public_listener.or_else(|| {
                if let (Some(h), Some(p)) = (&rpc.host, rpc.port) {
                    Some(format!("{}:{}", h, p))
                } else {
                    None
                }
            }) {
                self.rpc_public_listener = Some(public_listener.clone());
                if let Some((host, port)) = parse_listener(&public_listener) {
                    self.rpc_host = host;
                    self.rpc_port = port;
                }
            }
            if self.rpc_operator_listener.is_none() {
                self.rpc_operator_listener = rpc.operator_listener;
            }
            if self.rpc_trusted_proxies.is_empty() {
                if let Some(proxies) = rpc.trusted_proxies {
                    self.rpc_trusted_proxies = proxies;
                }
            }
            if self.rpc_allowed_ips.is_empty() {
                if let Some(allowed) = rpc.allowed_ips {
                    self.rpc_allowed_ips = allowed;
                }
            }
            if self.rpc_cors_origins.is_empty() {
                if let Some(cors) = rpc.cors_origins {
                    self.rpc_cors_origins = cors;
                }
            }
            if self.rpc_rate_limit_per_minute.is_none() {
                self.rpc_rate_limit_per_minute = rpc.rate_limit_per_minute;
            }
            if let Some(auth) = rpc.auth_required {
                self.rpc_auth_required = auth;
            }
        }

        if let Some(metrics) = fc.metrics {
            if let Some(listener) = metrics
                .listener
                .or_else(|| metrics.port.map(|p| format!("0.0.0.0:{}", p)))
            {
                self.metrics_listener = Some(listener.clone());
                if let Some((_, port)) = parse_listener(&listener) {
                    self.metrics_port = port;
                }
            }
        }

        if let Some(validator) = fc.validator {
            if let Some(signer) = validator.signer {
                if self.signer_backend.is_none() {
                    self.signer_backend = signer.backend;
                }
                if let Some(pkcs11) = signer.pkcs11 {
                    if self.pkcs11_module_path.is_none() {
                        self.pkcs11_module_path = pkcs11.module_path;
                    }
                    if self.pkcs11_slot_id.is_none() {
                        self.pkcs11_slot_id = pkcs11.slot_id;
                    }
                    if self.pkcs11_token_pin_env.is_none() {
                        self.pkcs11_token_pin_env = pkcs11.token_pin_env;
                    }
                }
            }
            if self.validator_key_file.is_none() {
                self.validator_key_file = validator.key_file;
            }
            if self.validator_address.is_none() {
                self.validator_address = validator.address;
            }
        }

        if let Some(features) = fc.features {
            if let Some(gov) = features.governance {
                self.features_governance = gov;
            }
            if let Some(zkvm) = features.zkvm_contracts {
                self.features_zkvm_contracts = zkvm;
            }
            if let Some(pruning) = features.pruning {
                self.features_pruning = pruning;
            }
            if let Some(archive) = features.archive_mode {
                self.archive_mode = archive;
            }
        }

        if let Some(consensus) = fc.consensus {
            if self.consensus.is_none() {
                self.consensus = consensus.consensus_type.as_deref().and_then(|s| match s {
                    "pow" => Some(ConsensusType::PoW),
                    "pos" => Some(ConsensusType::PoS),
                    "poa" => Some(ConsensusType::PoA),
                    _ => None,
                });
            }
            if self.min_stake == 1000 {
                if let Some(min_stake) = consensus.min_stake {
                    self.min_stake = min_stake;
                }
            }
        }
    }

    fn apply_legacy_config(&mut self, legacy: LegacyFileConfig) {
        if let Some(ref name) = legacy.network {
            self.network = match name.as_str() {
                "mainnet" => Network::Mainnet,
                "testnet" => Network::Testnet,
                "devnet" => Network::Devnet,
                _ => self.network,
            };
        }
        if self.consensus.is_none() {
            self.consensus = legacy.consensus.as_deref().and_then(|s| match s {
                "pow" => Some(ConsensusType::PoW),
                "pos" => Some(ConsensusType::PoS),
                "poa" => Some(ConsensusType::PoA),
                _ => None,
            });
        }
        if let Some(diff) = legacy.difficulty {
            self.difficulty = diff;
        }
        if let Some(min_s) = legacy.min_stake {
            self.min_stake = min_s;
        }
        if self.port.is_none() {
            self.port = legacy.port;
        }
        if let Some(ref db) = legacy.db_path {
            self.db_path = db.clone();
        }
        if let Some(ref rpc_h) = legacy.rpc_host {
            self.rpc_host = rpc_h.clone();
        }
        if let Some(rpc_p) = legacy.rpc_port {
            self.rpc_port = rpc_p;
        }
        if let Some(metrics_p) = legacy.metrics_port {
            self.metrics_port = metrics_p;
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
    }

    // Load configuration values from Environment variables.
    // Env has higher precedence than file configuration, so we only apply if not already set by Env.
    fn load_from_env(&mut self) {
        if let Ok(net) = std::env::var("BUDLUM_NETWORK") {
            self.network = match net.as_str() {
                "mainnet" => Network::Mainnet,
                "testnet" => Network::Testnet,
                "devnet" => Network::Devnet,
                _ => self.network,
            };
        }
        if let Ok(role) = std::env::var("BUDLUM_ROLE") {
            self.role = role;
        }
        if let Ok(chain_id) = std::env::var("BUDLUM_CHAIN_ID") {
            if let Ok(id) = chain_id.parse::<u64>() {
                self.chain_id = Some(id);
            }
        }
        if let Ok(db) = std::env::var("BUDLUM_DB_PATH") {
            self.db_path = db;
        }
        if let Ok(key) = std::env::var("BUDLUM_VALIDATOR_KEY") {
            self.validator_key_file = Some(key);
        }
    }

    // Strict validation rules for production environments (especially Mainnet profile)
    fn validate_strict_rules(&self) {
        if let Some(chain_id) = self.chain_id {
            let expected_chain_id = self.network.chain_id().value();
            if chain_id != expected_chain_id {
                eprintln!(
                    "CRITICAL CONFIGURATION ERROR: Network profile '{}' requires chain ID {}, got {}.",
                    self.network, expected_chain_id, chain_id
                );
                std::process::exit(1);
            }
        }

        if self.network == Network::Mainnet {
            // Rule 1: Mainnet validators may not fall back to local key files.
            if self.role == "validator" && self.signer_backend.as_deref() != Some("pkcs11") {
                eprintln!("CRITICAL SECURITY FAILURE: Mainnet validators require validator.signer.backend = 'pkcs11'.");
                std::process::exit(1);
            }
            if self.role == "validator" {
                let module_path = self.pkcs11_module_path.as_deref().unwrap_or_default();
                if module_path.is_empty() {
                    eprintln!("CRITICAL SECURITY FAILURE: Mainnet validators require a PKCS#11 module_path.");
                    std::process::exit(1);
                }
                let pin_env = self.pkcs11_token_pin_env.as_deref().unwrap_or_default();
                if pin_env.is_empty() {
                    eprintln!("CRITICAL SECURITY FAILURE: Mainnet validators require a PKCS#11 token_pin_env.");
                    std::process::exit(1);
                }
                if std::env::var(pin_env)
                    .map(|pin| pin.is_empty())
                    .unwrap_or(true)
                {
                    eprintln!("CRITICAL SECURITY FAILURE: PKCS#11 PIN environment variable '{}' is missing or empty.", pin_env);
                    std::process::exit(1);
                }
                println!(
                    "INFO: Mainnet validator will use PKCS#11 HSM backend (module: {}, slot: {})",
                    module_path,
                    self.pkcs11_slot_id.unwrap_or(0)
                );
            }

            // Rule 2: No mDNS on mainnet!
            if self.mdns_enabled == Some(true) {
                eprintln!("CRITICAL SECURITY FAILURE: mDNS discovery is STRICTLY PROHIBITED on Mainnet profile!");
                std::process::exit(1);
            }

            // Rule 3: No empty bootnodes on mainnet!
            if self.bootnodes.is_empty() && self.bootstrap.is_none() {
                eprintln!("CRITICAL SECURITY FAILURE: Mainnet profile cannot start without explicit seed/bootnodes!");
                std::process::exit(1);
            }

            // Rule 4: No placeholder genesis files on mainnet!
            if let Some(ref genesis) = self.genesis_file {
                if genesis.contains("placeholder")
                    || genesis.contains("devnet")
                    || genesis.contains("testnet")
                {
                    eprintln!("CRITICAL SECURITY FAILURE: Placeholder genesis config is STRICTLY PROHIBITED on Mainnet!");
                    std::process::exit(1);
                }
            } else {
                eprintln!("CRITICAL SECURITY FAILURE: Genesis configuration file must be specified for Mainnet profile!");
                std::process::exit(1);
            }

            if self.features_governance || self.features_zkvm_contracts || self.features_pruning {
                eprintln!("CRITICAL SECURITY FAILURE: governance, zkvm_contracts and pruning must remain disabled for Mainnet v1.");
                std::process::exit(1);
            }
        }
    }

    // Node role based rules to avoid opening unnecessary listeners.
    fn apply_role_rules(&mut self) {
        match self.role.as_str() {
            "validator" | "sentry" | "seed" => {
                // Disabled public listeners for non-public-facing nodes
                self.rpc_enabled = false;
                println!(
                    "Node started as '{}' role. Disabling public JSON-RPC listeners for safety.",
                    self.role
                );
            }
            "rpc" | "archive" => {
                // Public RPC enabled
                self.rpc_enabled = true;
            }
            _ => {}
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

fn parse_listener(listener: &str) -> Option<(String, u16)> {
    let parts: Vec<&str> = listener.split(':').collect();
    if parts.len() == 2 {
        let host = parts[0].to_string();
        if let Ok(port) = parts[1].parse::<u16>() {
            return Some((host, port));
        }
    }
    None
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
