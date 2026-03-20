
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

use clap::Parser;
use std::sync::{Arc, Mutex};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[tokio::main]
async fn main() {
    let config = NodeConfig::parse();
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

    println!("Budlum Node - v0.2.0 (Framework Edition)");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Configuration:");
    println!("   Port: {}", config.port);
    println!("   Consensus: {:?}", config.consensus);
    println!("   Privacy: {:?}", config.privacy);
    println!("   DB Path: {}", config.db_path);
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    
    let consensus: Arc<dyn ConsensusEngine> = match config.consensus {
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

    let blockchain = Arc::new(Mutex::new(Blockchain::new(
        consensus,
        storage,
        config.chain_id,
        Some(pruning_manager),
    )));

    if let Some(_keys) = match config.consensus {
        ConsensusType::PoS => {
           let mut bc = blockchain.lock().unwrap();
           if let Some(ref v_path) = config.validator_key_file {
               if let Ok(keys) = budlum_core::crypto::primitives::ValidatorKeys::load(v_path) {
                    let addr = keys.sig_key.public_key_hex();
                    println!("Auto-bootstrapping validator: {}", addr);
                    bc.state.add_balance(&addr, 1_000_000);
                    let mut v = Validator::new(addr.clone(), 100_000);
                    v.active = true;
                    v.vrf_public_key = keys.vrf_key.public.to_bytes().to_vec();
                    bc.state.validators.insert(addr, v);
                    Some(keys)
               } else { None }
           } else { None }
        }
        _ => None,
     } {
       
    }

    if let ConsensusType::PoA = config.consensus {
        let validators = config.load_validators();
        if !validators.is_empty() {
            println!("Initializing PoA validators: {:?}", validators);
            let mut bc = blockchain.lock().unwrap();
            for addr in validators {
                let mut v = Validator::new(addr.clone(), 0);
                v.active = true;
                bc.state.validators.insert(addr, v);
            }
        } else {
            println!(" No validators configured!");
        }
    }

    let mut node = Node::new(blockchain.clone()).unwrap();
    if let Some(ref addr) = config.bootstrap {
        if let Err(e) = node.bootstrap(addr) {
            eprintln!("Failed to bootstrap: {}", e);
        }
    }
    node.listen(config.port).unwrap();
    if let Some(ref addr) = config.dial {
        node.dial(addr).expect("Failed to dial");
    }
    let client = node.get_client();
    let peer_id = node.peer_id;
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
                            let mut chain = blockchain.lock().unwrap();
                            let producer = if let Some(addr) = &config.validator_address {
                                addr.clone()
                            } else {
                                peer_id.to_string()
                            };
                            chain.produce_block(producer);
                        }
                        "chain" => {
                            let chain = blockchain.lock().unwrap();
                            chain.print_info();
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
