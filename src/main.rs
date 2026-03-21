
use budlum_core::chain::blockchain::Blockchain;
use budlum_core::core::account::Validator;
use budlum_core::core::transaction::Transaction;
use budlum_core::consensus::pow::PoWEngine;
use budlum_core::consensus::pos::{PoSEngine, PoSConfig};
use budlum_core::consensus::poa::{PoAEngine, PoAConfig};
use budlum_core::consensus::ConsensusEngine;
use budlum_core::network::node::Node;
use budlum_core::network::protocol::NetworkMessage;
use budlum_core::storage::db::Storage;
use budlum_core::chain::snapshot::PruningManager;
use budlum_core::cli::{ConsensusType, NodeConfig};
use budlum_core::rpc::RpcServer;
use budlum_core::chain::chain_actor::{ChainActor, ChainHandle};

use clap::Parser;
use std::sync::{Arc, Mutex};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

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
                println!("Address: {}", keys.sig_key.public_key_hex());
            }
            Err(e) => eprintln!("Error generating key: {}", e),
        }
        return;
    }

    let network = config.network;
    let port = config.port.unwrap_or(network.default_port());
    let chain_id = config.chain_id.unwrap_or(network.chain_id().value());
    let consensus_type = config.consensus.unwrap_or(match network {
        budlum_core::core::chain_config::Network::Mainnet => ConsensusType::PoS,
        budlum_core::core::chain_config::Network::Testnet => ConsensusType::PoS,
        budlum_core::core::chain_config::Network::Devnet => ConsensusType::PoW,
    });

    println!("Budlum Node - v0.2.0 (Framework Edition)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Configuration:");
    println!("   Network: {}", network);
    println!("   Chain ID: {}", chain_id);
    println!("   Port: {}", port);
    println!("   Consensus: {:?}", consensus_type);
    println!("   Privacy: {:?}", config.privacy);
    println!("   DB Path: {}", config.db_path);
    println!("   Metrics: http://127.0.0.1:{}/metrics", config.metrics_port);
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
            Arc::new(PoAEngine::new(
                PoAConfig::default(),
                None,
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

    let blockchain = Blockchain::new(
        consensus,
        storage,
        chain_id,
        Some(pruning_manager),
    );

    let (chain_actor, chain) = ChainActor::new(blockchain);
    tokio::spawn(async move {
        chain_actor.run().await;
    });

    if let Some(_keys) = match consensus_type {
        ConsensusType::PoS => {
           if let Some(ref v_path) = config.validator_key_file {
               if let Ok(keys) = budlum_core::crypto::primitives::ValidatorKeys::load(v_path) {
                    let addr = keys.sig_key.public_key_hex();
                    println!("Auto-bootstrapping validator: {}", addr);
                    // Add balance and validator info via actor or directly if before actor run
                    // For simplicity, we assume the user can do this via RPC or we add a command
                    Some(keys)
               } else { None }
           } else { None }
        }
        _ => None,
     } {
       
    }

    if let Some(ConsensusType::PoA) = config.consensus {
        let validators = config.load_validators();
        if !validators.is_empty() {
            println!("Initializing PoA validators: {:?}", validators);
            // This logic should be moved into Blockchain::new or handled via actor
        } else {
            println!(" No validators configured!");
        }
    }

    let mut node = Node::new(chain.clone()).unwrap();
    
    let mut bootstraps = Vec::new();
    if let Some(ref addr) = config.bootstrap {
        bootstraps.push(addr.clone());
    } else {
        bootstraps.extend(network.bootnodes());
    }

    for addr in bootstraps {
        if let Err(e) = node.bootstrap(&addr) {
            eprintln!("Failed to bootstrap from {}: {}", addr, e);
        }
    }

    node.listen(port).unwrap();
    if let Some(ref addr) = config.dial {
        node.dial(addr).expect("Failed to dial");
    }
    let client = node.get_client();
    let peer_id = node.peer_id;

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
        use hyper::service::service_fn;
        use hyper::{Request, Response, body::Bytes};
        use http_body_util::Full;
        use hyper_util::rt::TokioIo;

        let listener = match tokio::net::TcpListener::bind(format!("0.0.0.0:{}", metrics_port)).await {
            Ok(l) => l,
            Err(e) => { eprintln!("Metrics server bind error: {}", e); return; }
        };
        println!("Prometheus metrics on :{}/metrics", metrics_port);
        loop {
            if let Ok((stream, _)) = listener.accept().await {
                let m = metrics_clone.clone();
                tokio::spawn(async move {
                    let io = TokioIo::new(stream);
                    let _ = hyper::server::conn::http1::Builder::new()
                        .serve_connection(io, service_fn(move |_req: Request<hyper::body::Incoming>| {
                            let body = m.encode();
                            async move {
                                Ok::<_, std::convert::Infallible>(
                                    Response::new(Full::new(Bytes::from(body)))
                                )
                            }
                        }))
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
                            let tx = Transaction::new(
                                peer_id.to_string(),
                                "recipient".to_string(),
                                10,
                                b"demo tx".to_vec(),
                            );
                            client.broadcast("transactions".to_string(), NetworkMessage::Transaction(tx)).await;
                        }
                        "block" | "mine" => {
                            let producer = if let Some(addr) = &config.validator_address {
                                addr.clone()
                            } else {
                                peer_id.to_string()
                            };
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
