use budlum_core::chain::blockchain::Blockchain;
use budlum_core::chain::chain_actor::ChainActor;
use budlum_core::chain::snapshot::PruningManager;
use budlum_core::cli::{ConsensusType, NodeConfig};
use budlum_core::consensus::poa::{PoAConfig, PoAEngine};
use budlum_core::consensus::pos::{PoSConfig, PoSEngine};
use budlum_core::consensus::pow::PoWEngine;
use budlum_core::consensus::ConsensusEngine;
use budlum_core::core::address::Address;
use budlum_core::core::transaction::Transaction;
use budlum_core::crypto::primitives::{KeyPair, ValidatorKeys};
use budlum_core::crypto::signer::ConsensusSigner;
use budlum_core::domain::{
    default_domain, ConsensusKind, PoADomainPlugin, PoSDomainPlugin, PoWDomainPlugin,
};
use budlum_core::network::node::Node;
use budlum_core::network::protocol::NetworkMessage;
use budlum_core::rpc::{RpcMode, RpcSecurityConfig, RpcServer};
use budlum_core::storage::db::Storage;

use clap::Parser;
use std::sync::Arc;
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

fn load_signing_key(path: &str) -> Option<KeyPair> {
    ValidatorKeys::load(path)
        .map(|keys| keys.sig_key)
        .or_else(|_| KeyPair::load(path))
        .ok()
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() >= 3 && args[1] == "genesis" && args[2] == "build" {
        let mut output_path = "./genesis.json".to_string();
        let mut chain_id = 1337u64;
        let mut validators = Vec::new();
        let mut allocations = Vec::new();
        let mut block_reward = 50u64;
        let mut base_fee = 1u64;
        let mut dev_key_output = None;

        let mut i = 3;
        while i < args.len() {
            match args[i].as_str() {
                "--output" | "-o" => {
                    if i + 1 < args.len() {
                        output_path = args[i + 1].clone();
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --output");
                        std::process::exit(1);
                    }
                }
                "--chain-id" | "-c" => {
                    if i + 1 < args.len() {
                        chain_id = match args[i + 1].parse() {
                            Ok(v) => v,
                            Err(_) => {
                                eprintln!("Error: Invalid chain-id '{}'", args[i + 1]);
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --chain-id");
                        std::process::exit(1);
                    }
                }
                "--validators" | "-v" => {
                    if i + 1 < args.len() {
                        let addrs = &args[i + 1];
                        for addr_str in addrs.split(',') {
                            if let Ok(addr) = Address::from_hex(addr_str.trim()) {
                                validators.push(addr);
                            } else {
                                eprintln!("Error: Invalid validator address '{}'", addr_str);
                                std::process::exit(1);
                            }
                        }
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --validators");
                        std::process::exit(1);
                    }
                }
                "--allocations" | "-a" => {
                    if i + 1 < args.len() {
                        let allocs_str = &args[i + 1];
                        for item in allocs_str.split(',') {
                            let parts: Vec<&str> = item.split(':').collect();
                            if parts.len() == 2 {
                                let addr = match Address::from_hex(parts[0].trim()) {
                                    Ok(a) => a,
                                    Err(_) => {
                                        eprintln!("Error: Invalid allocation address '{}'", parts[0]);
                                        std::process::exit(1);
                                    }
                                };
                                let amount: u64 = match parts[1].trim().parse() {
                                    Ok(v) => v,
                                    Err(_) => {
                                        eprintln!("Error: Invalid allocation amount '{}'", parts[1]);
                                        std::process::exit(1);
                                    }
                                };
                                allocations.push((addr, amount));
                            } else {
                                eprintln!("Error: Invalid allocation format '{}' (expected address:amount)", item);
                                std::process::exit(1);
                            }
                        }
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --allocations");
                        std::process::exit(1);
                    }
                }
                "--block-reward" => {
                    if i + 1 < args.len() {
                        block_reward = match args[i + 1].parse() {
                            Ok(v) => v,
                            Err(_) => {
                                eprintln!("Error: Invalid block-reward '{}'", args[i + 1]);
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --block-reward");
                        std::process::exit(1);
                    }
                }
                "--base-fee" => {
                    if i + 1 < args.len() {
                        base_fee = match args[i + 1].parse() {
                            Ok(v) => v,
                            Err(_) => {
                                eprintln!("Error: Invalid base-fee '{}'", args[i + 1]);
                                std::process::exit(1);
                            }
                        };
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --base-fee");
                        std::process::exit(1);
                    }
                }
                "--dev-key-output" => {
                    if i + 1 < args.len() {
                        dev_key_output = Some(args[i + 1].clone());
                        i += 2;
                    } else {
                        eprintln!("Error: Missing value for --dev-key-output");
                        std::process::exit(1);
                    }
                }
                other => {
                    eprintln!("Unknown argument: {}", other);
                    std::process::exit(1);
                }
            }
        }

        // Dev convenience path: never print generated private key material.
        if allocations.is_empty() {
            if chain_id
                != budlum_core::core::chain_config::Network::Devnet
                    .chain_id()
                    .value()
            {
                eprintln!(
                    "Error: automatic allocation key generation is only allowed for the devnet chain ID"
                );
                std::process::exit(1);
            }
            let key_output = dev_key_output.unwrap_or_else(|| {
                eprintln!(
                    "Error: --allocations is required unless --dev-key-output is provided for a generated dev key"
                );
                std::process::exit(1);
            });
            let dev_key = match KeyPair::generate() {
                Ok(k) => k,
                Err(e) => {
                    eprintln!("Error: Failed to generate dev keypair: {}", e);
                    std::process::exit(1);
                }
            };
            let dev_addr = Address::from(dev_key.public_key_bytes());
            if let Err(e) = dev_key.save(&key_output) {
                eprintln!("Error: Failed to save generated dev key: {}", e);
                std::process::exit(1);
            }
            allocations.push((dev_addr, 1_000_000_000));
            validators.push(dev_addr);
            println!("No allocations/validators provided. Generated default devnet keypair:");
            println!("Address: {}", dev_addr);
            println!("Private key saved to: {}", key_output);
        }
        if chain_id
            != budlum_core::core::chain_config::Network::Devnet
                .chain_id()
                .value()
            && validators.is_empty()
        {
            eprintln!("Error: non-devnet genesis files require an explicit --validators list");
            std::process::exit(1);
        }

        let genesis_config = budlum_core::chain::genesis::GenesisConfig {
            chain_id,
            allocations,
            validators,
            block_reward,
            base_fee,
            gas_schedule: budlum_core::core::chain_config::Network::from_chain_id(chain_id)
                .map(|n| n.gas_schedule())
                .unwrap_or_else(|| budlum_core::core::chain_config::Network::Devnet.gas_schedule()),
            timestamp: 0,
        };

        let data = match serde_json::to_string_pretty(&genesis_config) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("Error: Failed to serialize genesis config: {}", e);
                std::process::exit(1);
            }
        };
        if let Err(e) = std::fs::write(&output_path, data) {
            eprintln!("Error: Failed to write genesis file '{}': {}", output_path, e);
            std::process::exit(1);
        }
        println!(
            "Genesis configuration file built and saved to: {}",
            output_path
        );
        return;
    }

    let mut config = NodeConfig::parse();
    config.load_with_file();
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    if let Err(e) = tracing::subscriber::set_global_default(subscriber) {
        eprintln!("Error: Failed to set default tracing subscriber: {}", e);
        std::process::exit(1);
    }

    if let Some(ref path) = config.gen_key {
        match budlum_core::crypto::primitives::ValidatorKeys::generate() {
            Ok(keys) => {
                if let Err(e) = keys.save(path) {
                    eprintln!("Error: Failed to save key to '{}': {}", path, e);
                    std::process::exit(1);
                }
                println!("Validator key generated and saved to: {}", path);
                println!(
                    "Address: {}",
                    Address::from(keys.sig_key.public_key_bytes())
                );
            }
            Err(e) => eprintln!("Error generating key: {}", e),
        }
        return;
    }

    if config.check_db {
        let storage = match Storage::new(&config.db_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: Failed to open DB at '{}': {}", config.db_path, e);
                std::process::exit(1);
            }
        };
        println!(
            "🔍 Starting Database Integrity Audit on: {}",
            config.db_path
        );
        match storage.check_integrity() {
            Ok(errors) => {
                if errors.is_empty() {
                    println!("✅ Integrity Audit PASSED. No corruptions found.");
                } else {
                    println!("❌ Integrity Audit FAILED! Found {} errors.", errors.len());
                    for err in errors {
                        println!("   - {}", err);
                    }
                    if config.repair_db {
                        println!("🔧 Attempting automatic repair...");
                        if let Err(e) = storage.repair_index() {
                            eprintln!("❌ Repair failed: {}", e);
                        } else {
                            println!(
                                "✅ Repair successful. Please run --check-db again to verify."
                            );
                        }
                    } else {
                        println!("💡 Tip: Run with --repair-db to attempt index reconstruction.");
                    }
                }
            }
            Err(e) => eprintln!("System error during audit: {}", e),
        }
        return;
    }

    if config.repair_db {
        let storage = match Storage::new(&config.db_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error: Failed to open DB at '{}': {}", config.db_path, e);
                std::process::exit(1);
            }
        };
        println!("🔧 Starting manual Database Repair on: {}", config.db_path);
        if let Err(e) = storage.repair_index() {
            eprintln!("Repair failed: {}", e);
        } else {
            println!("Repair complete. Re-indexing finished.");
        }
        return;
    }

    let network = config.network;
    let port = config.port.unwrap_or(network.default_port());
    let chain_id = config.chain_id.unwrap_or(network.chain_id().value());
    let network_params = network.consensus_params();
    if config.min_stake == 1000 {
        config.min_stake = network_params.min_stake;
    }
    let consensus_type = config.consensus.unwrap_or(match network {
        budlum_core::core::chain_config::Network::Mainnet => ConsensusType::PoS,
        budlum_core::core::chain_config::Network::Testnet => ConsensusType::PoS,
        budlum_core::core::chain_config::Network::Devnet => ConsensusType::PoW,
    });
    let poa_validators = if consensus_type == ConsensusType::PoA {
        config.load_validator_addresses()
    } else {
        Vec::new()
    };
    let local_signer_address = config
        .validator_key_file
        .as_ref()
        .and_then(|path| load_signing_key(path))
        .map(|key| Address::from(key.public_key_bytes()));

    println!("Budlum Node - v0.2.0 (Framework Edition)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Configuration:");
    println!("   Network: {}", network);
    println!("   Chain ID: {}", chain_id);
    println!("   Port: {}", port);
    println!("   Consensus: {:?}", consensus_type);
    println!("   Privacy: {:?}", config.privacy);
    println!("   DB Path: {}", config.db_path);
    println!(
        "   Metrics: http://127.0.0.1:{}/metrics",
        config.metrics_port
    );
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

    let hsm_signer: Option<Arc<dyn ConsensusSigner>> = if config.signer_backend.as_deref() == Some("pkcs11") {
        let module_path = config.pkcs11_module_path.as_deref().unwrap_or("");
        let slot_id = config.pkcs11_slot_id.unwrap_or(0);
        let pin_env = config.pkcs11_token_pin_env.as_deref().unwrap_or("");
        if module_path.is_empty() || pin_env.is_empty() {
            eprintln!("ERROR: PKCS#11 backend requires --pkcs11-module-path and --pkcs11-token-pin-env");
            std::process::exit(1);
        }
        match budlum_core::crypto::pkcs11::Pkcs11Signer::new(
            module_path.to_string(),
            slot_id,
            pin_env.to_string(),
        ) {
            Ok(signer) => {
                println!("PKCS#11 HSM initialized (slot: {})", slot_id);
                Some(Arc::new(signer))
            }
            Err(e) => {
                eprintln!("CRITICAL: Failed to initialize PKCS#11 signer: {}", e);
                std::process::exit(1);
            }
        }
    } else {
        None
    };

    let consensus: Arc<dyn ConsensusEngine> = match consensus_type {
        ConsensusType::PoW => {
            println!(" PoW mode - difficulty: {}", config.difficulty);
            Arc::new(PoWEngine::new(config.difficulty))
        }
        ConsensusType::PoS => {
            println!("PoS mode - min stake: {}", config.min_stake);
            let pos_config = PoSConfig {
                min_stake: config.min_stake,
                slot_duration: (network_params.slot_ms / 1000).max(1),
                epoch_length: network_params.epoch_len,
                ..Default::default()
            };
            let keys = if let Some(ref path) = config.validator_key_file {
                match budlum_core::crypto::primitives::ValidatorKeys::load(path) {
                    Ok(k) => Some(k),
                    Err(e) => {
                        println!("Failed to load validator keys from {}: {}", path, e);
                        None
                    }
                }
            } else {
                None
            };
            if let Some(signer) = hsm_signer {
                Arc::new(PoSEngine::with_signer(pos_config, keys, signer))
            } else {
                Arc::new(PoSEngine::new(pos_config, keys))
            }
        }
        ConsensusType::PoA => {
            println!("PoA mode");
            let poa_keypair = config
                .validator_key_file
                .as_ref()
                .and_then(|path| load_signing_key(path));
            if let Some(signer) = hsm_signer {
                Arc::new(PoAEngine::with_signer(
                    PoAConfig {
                        validators_file: Some(config.validators_file.clone()),
                        ..Default::default()
                    },
                    poa_keypair,
                    signer,
                ))
            } else {
                Arc::new(PoAEngine::new(
                    PoAConfig {
                        validators_file: Some(config.validators_file.clone()),
                        ..Default::default()
                    },
                    poa_keypair,
                ))
            }
        }
    };

    let storage = Some(Storage::new(&config.db_path).unwrap_or_else(|e| {
        eprintln!(
            "CRITICAL: Failed to initialize storage at {}: {}",
            config.db_path, e
        );
        std::process::exit(1);
    }));

    let genesis_config = if let Some(ref path) = config.genesis_file {
        let data = std::fs::read_to_string(path).unwrap_or_else(|e| {
            eprintln!("CRITICAL: Failed to read genesis file {}: {}", path, e);
            std::process::exit(1);
        });
        let custom_genesis: budlum_core::chain::genesis::GenesisConfig =
            serde_json::from_str(&data).unwrap_or_else(|e| {
                eprintln!("CRITICAL: Failed to parse genesis JSON {}: {}", path, e);
                std::process::exit(1);
            });
        if custom_genesis.chain_id != chain_id {
            eprintln!(
                "CRITICAL: Genesis chain ID {} does not match configured chain ID {}",
                custom_genesis.chain_id, chain_id
            );
            std::process::exit(1);
        }
        println!("Loaded custom genesis configuration from: {}", path);
        Some(custom_genesis)
    } else {
        None
    };

    let pruning_manager = config.features_pruning.then(|| {
        PruningManager::new(
            1000,
            100,
            config
                .snapshot_dir
                .clone()
                .unwrap_or_else(|| "./data/snapshots".to_string()),
        )
    });

    let metrics = Arc::new(budlum_core::core::metrics::Metrics::new());

    let mut blockchain = Blockchain::new_with_genesis(
        consensus.clone(),
        storage,
        chain_id,
        pruning_manager,
        genesis_config,
    )
    .with_metrics(metrics.clone());

    let domain_id = 1u32;
    let (domain_kind, adapter_name, min_conf) = match consensus_type {
        ConsensusType::PoW => (ConsensusKind::PoW, "pow-confirmation-depth", 64u64),
        ConsensusType::PoS => (ConsensusKind::PoS, "pos-qc-finality", 0u64),
        ConsensusType::PoA => (ConsensusKind::PoA, "poa-authority-quorum", 0u64),
    };

    let domain_def = default_domain(
        domain_id,
        domain_kind.clone(),
        chain_id,
        adapter_name,
        min_conf,
    );
    if blockchain.domain_registry.get(domain_id).is_none() {
        if let Err(e) = blockchain.register_consensus_domain(domain_def) {
            println!("Domain kaydi basarisiz: {}", e);
        } else {
            println!("Domain {} ({:?}) kaydedildi", domain_id, consensus_type);
        }
    }

    let plugin: std::sync::Arc<dyn budlum_core::domain::ConsensusDomainPlugin> =
        match consensus_type {
            ConsensusType::PoW => std::sync::Arc::new(PoWDomainPlugin::new(consensus.clone())),
            ConsensusType::PoS => std::sync::Arc::new(PoSDomainPlugin::new(consensus.clone())),
            ConsensusType::PoA => std::sync::Arc::new(PoADomainPlugin::new(consensus.clone())),
        };
    if let Err(e) = blockchain.plugin_registry.register(domain_id, plugin) {
        println!("Plugin kaydi basarisiz: {}", e);
    }

    for validator in &poa_validators {
        blockchain.state.add_validator(*validator, 1);
    }

    let (chain_actor, chain) = ChainActor::new(blockchain);
    tokio::spawn(async move {
        chain_actor.run().await;
    });

    if let Some(_keys) = match consensus_type {
        ConsensusType::PoS => {
            if let Some(ref v_path) = config.validator_key_file {
                if let Ok(keys) = budlum_core::crypto::primitives::ValidatorKeys::load(v_path) {
                    let addr = Address::from(keys.sig_key.public_key_bytes());
                    println!("Auto-bootstrapping validator: {}", addr);
                    Some(keys)
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    } {}

    if consensus_type == ConsensusType::PoA {
        if !poa_validators.is_empty() {
            println!("Initializing PoA validators: {:?}", poa_validators);
        } else {
            println!(" No validators configured!");
        }
    }

    let mut bootstraps = Vec::new();
    if let Some(ref addr) = config.bootstrap {
        bootstraps.push(addr.clone());
    } else if !config.bootnodes.is_empty() {
        bootstraps.extend(config.bootnodes.clone());
    } else {
        bootstraps.extend(network.bootnodes());
        bootstraps.extend(network.fallback_bootnodes());
    }

    if network == budlum_core::core::chain_config::Network::Mainnet && bootstraps.is_empty() {
        eprintln!("Refusing to start mainnet without at least one configured bootnode.");
        eprintln!("Set p2p.bootnodes in config/mainnet.toml or pass --bootstrap.");
        std::process::exit(1);
    }

    // Load or generate persistent P2P identity
    let identity_key =
        budlum_core::network::node::load_or_generate_identity_key(config.p2p_identity_file.as_deref());

    let node = match Node::with_key(chain.clone(), identity_key, true) {
        Ok(n) => n,
        Err(e) => {
            eprintln!("Error: Failed to initialize P2P node: {}", e);
            std::process::exit(1);
        }
    };
    let mut node = node
        .with_identity(config.p2p_identity_file.clone())
        .with_dns_seeds(config.dns_seeds.clone())
        .with_banned_peer_db(config.banned_peer_db.clone())
        .with_bootstrap_peers(bootstraps.clone())
        .with_metrics(metrics.clone());
    node.apply_network_security(network);

    for addr in &bootstraps {
        if let Err(e) = node.bootstrap(addr) {
            eprintln!("Failed to bootstrap from {}: {}", addr, e);
        }
    }

    if let Err(e) = node.listen(port) {
        eprintln!("Failed to listen on port {}: {}", port, e);
        std::process::exit(1);
    }
    if let Some(ref addr) = config.dial {
        if let Err(e) = node.dial(addr) {
            eprintln!("Failed to dial {}: {}", addr, e);
        }
    }
    let client = node.get_client();
    let peer_id = node.peer_id.to_string();
    println!("Node PeerID: {}", peer_id);
    let cli_producer_address = config
        .validator_address
        .as_ref()
        .and_then(|addr_str| Address::from_hex(addr_str).ok())
        .or(local_signer_address);

    if config.rpc_enabled {
        let rpc_security = match RpcSecurityConfig::from_env(
            config.rpc_auth_required,
            config.rpc_api_key_env.as_deref(),
            config.rpc_allowed_ips.clone(),
            config.rpc_cors_origins.clone(),
            config.rpc_rate_limit_per_minute,
        ) {
            Ok(mut security) => {
                security.trusted_proxies = config.rpc_trusted_proxies.clone();
                security.max_request_body_size = Some(10 * 1024 * 1024);
                security.max_connections = Some(500);
                security
            }
            Err(e) => {
                eprintln!("RPC configuration error: {}", e);
                return;
            }
        };

        // Public RPC listener
        let public_addr = config
            .rpc_public_listener
            .clone()
            .unwrap_or_else(|| format!("{}:{}", config.rpc_host, config.rpc_port));
        let pub_server = RpcServer::with_security_and_mode(
            chain.clone(),
            node.get_client(),
            rpc_security.clone(),
            RpcMode::Public,
        );
        let pub_addr = public_addr.clone();
        tokio::spawn(async move {
            if let Err(e) = pub_server.run(pub_addr.clone()).await {
                eprintln!("Public RPC Server Error on {}: {}", pub_addr, e);
            } else {
                println!("Public JSON-RPC Server running on {}", pub_addr);
            }
        });

        // Operator RPC listener (localhost-only, no auth by default)
        if let Some(operator_addr) = config.rpc_operator_listener.as_ref() {
            let op_security = RpcSecurityConfig::operator_default();
            let op_server = RpcServer::with_security_and_mode(
                chain.clone(),
                node.get_client(),
                op_security,
                RpcMode::Operator,
            );
            let op_addr = operator_addr.clone();
            tokio::spawn(async move {
                if let Err(e) = op_server.run(op_addr.clone()).await {
                    eprintln!("Operator RPC Server Error on {}: {}", op_addr, e);
                } else {
                    println!("Operator JSON-RPC Server running on {}", op_addr);
                }
            });
        }
    } else {
        println!("JSON-RPC Server disabled by config");
    }

    let metrics_clone = metrics.clone();
    let metrics_port = config.metrics_port;
    tokio::spawn(async move {
        use http_body_util::Full;
        use hyper::service::service_fn;
        use hyper::{body::Bytes, Request, Response};
        use hyper_util::rt::TokioIo;

        let listener =
            match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", metrics_port)).await {
                Ok(l) => l,
                Err(e) => {
                    eprintln!("Metrics server bind error: {}", e);
                    return;
                }
            };
        println!("Prometheus metrics on :{}/metrics", metrics_port);
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                let m = metrics_clone.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(
                            io,
                            service_fn(move |_req: Request<hyper::body::Incoming>| {
                                let body = m.encode();
                                async move {
                                    Ok::<_, std::convert::Infallible>(Response::new(Full::new(
                                        Bytes::from(body),
                                    )))
                                }
                            }),
                        )
                        .await;
                });
            }
        }
    });

    tokio::select! {
        _ = node.run() => {},
        _ = tokio::signal::ctrl_c() => {
            tracing::info!("Shutdown signal received");
            match chain.flush_storage().await {
                Ok(bytes) => tracing::info!("Storage flushed: {} bytes", bytes),
                Err(e) => tracing::error!("Storage flush failed during shutdown: {}", e),
            }
        },
        _ = async {
            let mut stdin = tokio::io::BufReader::new(tokio::io::stdin());
            let mut line = String::new();
            client.subscribe("blocks".to_string()).await;
            client.subscribe("transactions".to_string()).await;
            loop {
                line.clear();
                use tokio::io::AsyncBufReadExt;
                if stdin.read_line(&mut line).await.is_ok() {
                    let cmd = line.trim();
                    match cmd {
                        "tx" => {
                            let alice = Address::from_hex(&"01".repeat(32)).unwrap();
                            let bob = Address::from_hex(&"02".repeat(32)).unwrap();
                            let tx = Transaction::new(
                                alice,
                                bob,
                                10,
                                b"demo tx".to_vec(),
                            );
                            client.broadcast("transactions".to_string(), NetworkMessage::Transaction(tx)).await;
                        }
                        "block" | "mine" => {
                            let producer = cli_producer_address.unwrap_or(Address::zero());
                            let _ = chain.produce_block(producer).await;
                        }
                        "chain" => {
                            let info = chain.get_chain_info().await;
                            println!("{}", info);
                        }
                        "peers" => {
                            client.list_peers().await;
                        }
                        "sync" => {
                            let msg = NetworkMessage::GetHeaders {
                                locator: Vec::new(),
                                limit: 2000,
                            };
                            client.broadcast("blocks".to_string(), msg).await;
                        }
                        "help" => {
                            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                            println!("Commands:");
                            println!("   tx    - Send demo transaction");
                            println!("   mine  - Produce new block");
                            println!("   chain - Show blockchain info");
                            println!("   peers - List connected peers");
                            println!("   sync  - Request chain sync");
                            println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                        }
                        _ => {}
                    }
                }
            }
        } => {}
    }
}
