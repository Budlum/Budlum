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
use budlum_core::domain::{
    default_domain, ConsensusKind, PoADomainPlugin, PoSDomainPlugin, PoWDomainPlugin,
};
use budlum_core::network::node::Node;
use budlum_core::network::protocol::NetworkMessage;
use budlum_core::rpc::RpcServer;
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
    let mut config = NodeConfig::parse();
    config.load_with_file();
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting default subscriber failed");

    if let Some(ref path) = config.gen_key {
        match budlum_core::crypto::primitives::ValidatorKeys::generate() {
            Ok(keys) => {
                keys.save(path).expect("Failed to save key");
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
        let storage = Storage::new(&config.db_path).expect("Failed to open DB");
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
        let storage = Storage::new(&config.db_path).expect("Failed to open DB");
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
            Arc::new(PoSEngine::new(pos_config, keys))
        }
        ConsensusType::PoA => {
            println!("PoA mode");
            let poa_keypair = config
                .validator_key_file
                .as_ref()
                .and_then(|path| load_signing_key(path));
            Arc::new(PoAEngine::new(
                PoAConfig {
                    validators_file: Some(config.validators_file.clone()),
                    ..Default::default()
                },
                poa_keypair,
            ))
        }
    };

    let storage = match Storage::new(&config.db_path) {
        Ok(s) => Some(s),
        Err(e) => {
            println!("Failed to initialize storage: {}", e);
            None
        }
    };

    let pruning_manager = PruningManager::new(1000, 100, "./data/snapshots".to_string());

    let mut blockchain =
        Blockchain::new(consensus.clone(), storage, chain_id, Some(pruning_manager));

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
        eprintln!("Set [bootnodes].addresses in config/mainnet.toml or pass --bootstrap.");
        std::process::exit(1);
    }

    let mut node = Node::new_with_bootstrap(chain.clone(), bootstraps.clone()).unwrap();
    node.apply_network_security(network);

    for addr in &bootstraps {
        if let Err(e) = node.bootstrap(&addr) {
            eprintln!("Failed to bootstrap from {}: {}", addr, e);
        }
    }

    node.listen(port).unwrap();
    if let Some(ref addr) = config.dial {
        node.dial(addr).expect("Failed to dial");
    }
    let client = node.get_client();
    let peer_id = node.peer_id.to_string();
    println!("Node PeerID: {}", peer_id);
    let cli_producer_address = config
        .validator_address
        .as_ref()
        .and_then(|addr_str| Address::from_hex(addr_str).ok())
        .or(local_signer_address);

    let rpc_addr = format!("{}:{}", config.rpc_host, config.rpc_port);
    let rpc_server = RpcServer::new(chain.clone(), node.get_client());
    tokio::spawn(async move {
        if let Err(e) = rpc_server.run(rpc_addr.clone()).await {
            eprintln!("RPC Server Error on {}: {}", rpc_addr, e);
        } else {
            println!("JSON-RPC Server running on {}", rpc_addr);
        }
    });

    let metrics = budlum_core::core::metrics::Metrics::new();
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
