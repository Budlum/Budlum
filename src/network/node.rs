use crate::network::protocol::NetworkMessage;
use libp2p::{
    futures::StreamExt,
    gossipsub, identify, identity,
    kad::{
        store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig, Event as KademliaEvent,
    },
    mdns, noise, ping,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm,
};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tracing::{info, warn};
#[derive(NetworkBehaviour)]
pub struct BudlumBehaviour {
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    mdns: mdns::tokio::Behaviour,
    gossipsub: gossipsub::Behaviour,
    kad: Kademlia<MemoryStore>,
}
use crate::network::peer_manager::PeerManager;
use crate::chain::blockchain::Blockchain;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
pub enum NodeCommand {
    Subscribe(String),
    Broadcast(String, NetworkMessage),
    ListPeers,
}
#[derive(Clone)]
pub struct NodeClient {
    sender: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
}
impl NodeClient {
    pub async fn subscribe(&self, topic: String) {
        let _ = self.sender.send(NodeCommand::Subscribe(topic)).await;
    }
    pub async fn broadcast(&self, topic: String, msg: NetworkMessage) {
        let _ = self.sender.send(NodeCommand::Broadcast(topic, msg)).await;
    }
    pub async fn list_peers(&self) {
        let _ = self.sender.send(NodeCommand::ListPeers).await;
    }
}
#[tokio::test]
async fn test_node_creation() {
    use crate::consensus::pow::PoWEngine;
    let consensus = std::sync::Arc::new(PoWEngine::new(2));
    let blockchain = Arc::new(Mutex::new(Blockchain::new(consensus, None, 1337, None)));
    let node = Node::new(blockchain);
    assert!(node.is_ok());
}
pub struct Node {
    swarm: Swarm<BudlumBehaviour>,
    command_rx: mpsc::Receiver<NodeCommand>,
    command_tx: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
    pub blockchain: Arc<Mutex<Blockchain>>,
    pub peer_manager: Arc<Mutex<PeerManager>>,
    pub bootstrap_peers: Vec<String>,
}
impl Node {
    pub fn new(blockchain: Arc<Mutex<Blockchain>>) -> Result<Self, Box<dyn Error>> {
        let local_key = identity::Keypair::generate_ed25519();
        let peer_id = PeerId::from(local_key.public());
        info!("Node ID: {}", peer_id);
        let message_id_fn = |message: &gossipsub::Message| {
            let mut s = DefaultHasher::new();
            message.data.hash(&mut s);
            gossipsub::MessageId::from(s.finish().to_string())
        };
        let gossipsub_config = gossipsub::ConfigBuilder::default()
            .heartbeat_interval(Duration::from_secs(10))
            .validation_mode(gossipsub::ValidationMode::Strict)
            .message_id_fn(message_id_fn)
            .max_transmit_size(crate::network::protocol::MAX_MESSAGE_SIZE)
            .build()
            .map_err(|msg| std::io::Error::new(std::io::ErrorKind::Other, msg))?;
        let gossipsub = gossipsub::Behaviour::new(
            gossipsub::MessageAuthenticity::Signed(local_key.clone()),
            gossipsub_config,
        )?;
        let swarm = libp2p::SwarmBuilder::with_existing_identity(local_key)
            .with_tokio()
            .with_tcp(
                tcp::Config::default(),
                noise::Config::new,
                yamux::Config::default,
            )?
            .with_behaviour(|key| {
                let mdns = mdns::tokio::Behaviour::new(
                    mdns::Config::default(),
                    key.public().to_peer_id(),
                )?;
                let kad_store = MemoryStore::new(key.public().to_peer_id());
                let kad_config = KademliaConfig::default();
                let kademlia =
                    Kademlia::with_config(key.public().to_peer_id(), kad_store, kad_config);
                let identify = identify::Behaviour::new(identify::Config::new(
                    "/budlum/1.0.0".to_string(),
                    key.public(),
                ));
                Ok(BudlumBehaviour {
                    ping: ping::Behaviour::new(
                        ping::Config::new().with_interval(Duration::from_secs(15)),
                    ),
                    identify,
                    mdns,
                    gossipsub,
                    kad: kademlia,
                })
            })?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();
        let (command_tx, command_rx) = mpsc::channel(32);
        let peer_manager = Arc::new(Mutex::new(PeerManager::new()));
        Ok(Node {
            swarm,
            peer_id,
            command_tx,
            command_rx,
            blockchain,
            peer_manager,
            bootstrap_peers: Vec::new(),
        })
    }
    pub fn new_with_bootstrap(
        blockchain: Arc<Mutex<Blockchain>>,
        bootstrap_peers: Vec<String>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut node = Self::new(blockchain)?;
        node.bootstrap_peers = bootstrap_peers;
        Ok(node)
    }
    pub fn get_client(&self) -> NodeClient {
        NodeClient {
            sender: self.command_tx.clone(),
            peer_id: self.peer_id,
        }
    }
    pub fn listen(&mut self, port: u16) -> Result<(), Box<dyn Error>> {
        let addr: Multiaddr = format!("/ip4/0.0.0.0/tcp/{}", port).parse()?;
        self.swarm.listen_on(addr)?;
        info!("Listening on port {}", port);
        Ok(())
    }
    pub fn dial(&mut self, addr: &str) -> Result<(), Box<dyn Error>> {
        let remote: Multiaddr = addr.parse()?;
        self.swarm.dial(remote)?;
        info!("Dialing {}", addr);
        Ok(())
    }
    pub fn bootstrap(&mut self, addr: &str) -> Result<(), Box<dyn Error>> {
        let multiaddr: Multiaddr = addr.parse()?;
        let peer_id = match multiaddr
            .iter()
            .find(|p| matches!(p, libp2p::multiaddr::Protocol::P2p(_)))
        {
            Some(libp2p::multiaddr::Protocol::P2p(peer_id)) => peer_id,
            _ => return Err("Bootstrap address must contain /p2p/<ID>".into()),
        };
        info!("Bootstrapping via {}", addr);
        self.swarm
            .behaviour_mut()
            .kad
            .add_address(&peer_id, multiaddr);
        self.swarm.behaviour_mut().kad.bootstrap()?;
        Ok(())
    }
    pub async fn run(&mut self) {
        info!("Node running...");
        for addr in self.bootstrap_peers.clone() {
            if let Err(e) = self.bootstrap(&addr) {
                warn!("Bootstrap dial failed for {}: {}", addr, e);
            }
        }
        let mut gc_interval = tokio::time::interval(Duration::from_secs(60));
        let mut discovery_interval = tokio::time::interval(Duration::from_secs(300));

        loop {
            tokio::select! {
                _ = gc_interval.tick() => {
                    let mut chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                    let removed = chain.mempool.cleanup_expired();
                    if removed > 0 {
                        info!("Cleaned up {} expired transactions from mempool", removed);
                    }
                    drop(chain);

                    let mut pm = self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); });
                    pm.cleanup_expired_bans();
                }
                _ = discovery_interval.tick() => {
                    info!("Running periodic peer discovery...");
                    for addr in self.bootstrap_peers.clone() {
                        if let Err(e) = self.bootstrap(&addr) {
                            warn!("Periodic bootstrap failed for {}: {}", addr, e);
                        }
                    }
                }
                cmd = self.command_rx.recv() => {
                    if let Some(cmd) = cmd {
                        match cmd {
                            NodeCommand::Subscribe(topic) => {
                                let topic = gossipsub::IdentTopic::new(topic);
                                if let Err(e) = self.swarm.behaviour_mut().gossipsub.subscribe(&topic) {
                                    warn!("Failed to subscribe: {}", e);
                                } else {
                                    info!("Subscribed to topic: {}", topic);
                                }
                            }
                            NodeCommand::Broadcast(topic, msg) => {
                                let topic = gossipsub::IdentTopic::new(topic);
                                let data = msg.to_bytes();
                                if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic.clone(), data) {
                                    warn!("Failed to publish: {}", e);
                                } else {
                                    info!("Broadcasted to {}: {:?}", topic, msg);
                                }
                            }
                            NodeCommand::ListPeers => {
                                let peers: Vec<_> = self.swarm.behaviour().gossipsub.all_peers().collect();
                                info!("Connected peers: {:?}", peers.len());
                                for (peer, _topics) in peers {
                                    info!(" - {}", peer);
                                }
                            }
                        }
                    }
                }
                event = self.swarm.select_next_some() => {
                    match event {
                        SwarmEvent::NewListenAddr { address, .. } => {
                            info!("Listening on {}", address);
                        }
                        SwarmEvent::ConnectionEstablished { peer_id, .. } => {
                            info!("Connected to {}", peer_id);
                            let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });

                            let handshake = NetworkMessage::Handshake {
                                version_major: crate::core::encoding::PROTOCOL_VERSION_MAJOR,
                                version_minor: crate::core::encoding::PROTOCOL_VERSION_MINOR,
                                chain_id: chain.chain_id,
                                best_height: chain.chain.len() as u64,
                                validator_set_hash: chain.get_validator_set_hash(),
                                supported_schemes: vec!["ED25519".to_string(), "BLS".to_string(), "DILITHIUM".to_string()],
                            };

                            info!("DEBUG: Connected to {}, Chain length: {}, sending Handshake", peer_id, chain.chain.len());

                            let topic = gossipsub::IdentTopic::new("blocks");
                            if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, handshake.to_bytes()) {
                                warn!("Failed to send Handshake: {}", e);
                            }

                            if chain.chain.len() == 1 {
                                let locator = vec![chain.chain.last().unwrap().hash.clone()];
                                drop(chain);
                                info!("New connection, requesting headers...");
                                let topic = gossipsub::IdentTopic::new("blocks");
                                let msg = NetworkMessage::GetHeaders {
                                    locator,
                                    limit: 2000,
                                };
                                if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, msg.to_bytes()) {
                                    warn!("Failed to request headers: {}", e);
                                }
                            }
                        }
                        SwarmEvent::ConnectionClosed { peer_id, .. } => {
                            warn!("Disconnected from {}", peer_id);
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Ping(event)) => {
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Mdns(event)) => {
                            match event {
                                mdns::Event::Discovered(peers) => {
                                    for (peer_id, addr) in peers {
                                        info!("mDNS discovered: {} at {}", peer_id, addr);
                                        self.swarm.behaviour_mut().gossipsub.add_explicit_peer(&peer_id);
                                        if let Err(e) = self.swarm.dial(addr.clone()) {
                                            warn!("Failed to dial discovered peer: {}", e);
                                        }
                                    }
                                }
                                mdns::Event::Expired(peers) => {
                                    for (peer_id, _) in peers {
                                        info!("mDNS expired: {}", peer_id);
                                        self.swarm.behaviour_mut().gossipsub.remove_explicit_peer(&peer_id);
                                    }
                                }
                            }
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Gossipsub(gossipsub::Event::Message {
                            propagation_source: peer_id,
                            message_id: id,
                            message,
                        })) => {

                            if self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).is_banned(&peer_id) {
                                warn!("Ignoring message from banned peer {}", peer_id);
                                continue;
                            }

                            if !self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).check_rate_limit(&peer_id) {
                                warn!("Rate limit exceeded for peer {}", peer_id);
                                continue;
                            }

                            info!("Received from {}: id={}", peer_id, id);
                            match NetworkMessage::from_bytes_validated(&message.data) {
                                Ok(msg) => {
                                    let is_handshake_msg = matches!(
                                        msg,
                                        NetworkMessage::Handshake { .. } | NetworkMessage::HandshakeAck { .. }
                                    );

                                    if !is_handshake_msg && !self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).is_handshaked(&peer_id) {
                                        warn!("Peer {} sent {:?} before completing handshake, dropping.", peer_id, msg);

                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_invalid_tx(&peer_id);
                                        continue;
                                    }

                                    match msg {
                                        NetworkMessage::Block(block) => {
                                        if let Err(e) = NetworkMessage::validate_block_size(&block) {
                                            warn!("Received oversized block from {}: {:?}", peer_id, e);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_oversized_message(&peer_id);
                                            continue;
                                        }
                                        info!("BLOCK: #{} Hash: {}...", block.index, &block.hash[..8]);
                                        let mut chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        if block.index == chain.chain.len() as u64 {
                                            match chain.validate_and_add_block(block.clone()) {
                                                Ok(_) => {
                                                    info!("Added block #{} to local chain", block.index);
                                                    self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_good_behavior(&peer_id);
                                                }
                                                Err(e) => {
                                                    warn!("Block validation failed: {}", e);
                                                    self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_invalid_block(&peer_id);
                                                }
                                            }
                                        }
                                    }
                                    NetworkMessage::Transaction(tx) => {
                                        if let Err(e) = NetworkMessage::validate_tx_size(&tx) {
                                            warn!("Received oversized transaction from {}: {:?}", peer_id, e);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_oversized_message(&peer_id);
                                            continue;
                                        }
                                        info!("TX: {}->{} Amount: {}",
                                            &tx.from[..8], &tx.to[..8], tx.amount);
                                        let mut chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        match chain.add_transaction(tx) {
                                            Ok(_) => {
                                                self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_good_behavior(&peer_id);
                                            }
                                            Err(e) => {
                                                warn!("Failed to add transaction: {}", e);
                                                self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_invalid_tx(&peer_id);
                                            }
                                        }
                                    }

                                    NetworkMessage::GetHeaders { locator, limit } => {
                                        info!("GetHeaders request from {} (locator: {} hashes, limit: {})",
                                            peer_id, locator.len(), limit);
                                        let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });

                                        let start_idx = locator.iter()
                                            .find_map(|hash| {
                                                chain.chain.iter().position(|b| &b.hash == hash)
                                            })
                                            .map(|i| i + 1)
                                            .unwrap_or(0);

                                        let end_idx = (start_idx + limit as usize).min(chain.chain.len());
                                        let headers: Vec<_> = chain.chain[start_idx..end_idx]
                                            .iter()
                                            .map(|b| crate::core::block::BlockHeader::from_block(b))
                                            .collect();

                                        info!("Sending {} headers to {}", headers.len(), peer_id);
                                        let response = NetworkMessage::Headers(headers);
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                    }

                                    NetworkMessage::Headers(headers) => {
                                        if headers.len() > crate::network::protocol::MAX_HEADERS_PER_REQUEST as usize {
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_invalid_block(&peer_id);
                                            continue;
                                        }
                                        if !headers.is_empty() {
                                            let from = headers[0].index;
                                            let to = headers.last().unwrap().index;
                                            let req = NetworkMessage::GetBlocksRange { from, to };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                        }
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_good_behavior(&peer_id);
                                    }

                                    NetworkMessage::GetBlocksRange { from, to } => {
                                        info!("GetBlocksRange request from {} ({}..{})", peer_id, from, to);
                                        let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });

                                        let from_idx = from as usize;
                                        let to_idx = (to as usize).min(chain.chain.len());
                                        let max_blocks = crate::network::protocol::MAX_CHAIN_SYNC_BLOCKS;
                                        let to_idx = to_idx.min(from_idx + max_blocks);

                                        if from_idx < chain.chain.len() {
                                            let blocks: Vec<_> = chain.chain[from_idx..to_idx].to_vec();
                                            info!("Sending {} blocks to {}", blocks.len(), peer_id);
                                            let response = NetworkMessage::Blocks(blocks);
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                        }
                                    }

                                    NetworkMessage::Blocks(blocks) => {
                                        if blocks.len() > crate::network::protocol::MAX_CHAIN_SYNC_BLOCKS {
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_invalid_block(&peer_id);
                                            continue;
                                        }
                                        let mut chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        if !blocks.is_empty() {
                                            let start_idx = blocks[0].index as usize;
                                            if start_idx < chain.chain.len() && chain.chain[start_idx].hash != blocks[0].hash {
                                                let mut new_chain = chain.chain[..start_idx].to_vec();
                                                new_chain.extend(blocks.clone());
                                                let _ = chain.try_reorg(new_chain);
                                            } else {
                                                for block in blocks {
                                                    if block.index == chain.chain.len() as u64 {
                                                        let _ = chain.validate_and_add_block(block.clone());
                                                    }
                                                }
                                            }
                                        }
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_good_behavior(&peer_id);
                                    }

                                    NetworkMessage::NewTip { height, hash } => {
                                        let our_height = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); }).chain.len() as u64;
                                        if height > our_height {
                                            let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                            let mut locator = Vec::new();
                                            let mut step = 1;
                                            let mut current = chain.chain.len().saturating_sub(1);
                                            while current > 0 && locator.len() < 10 {
                                                locator.push(chain.chain[current].hash.clone());
                                                current = current.saturating_sub(step);
                                                step *= 2;
                                            }
                                            if locator.is_empty() && !chain.chain.is_empty() {
                                                locator.push(chain.chain[0].hash.clone());
                                            }
                                            let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                        }
                                    }

                                    NetworkMessage::GetStateSnapshot { height } => {
                                        info!("GetStateSnapshot request from {} (height: {})", peer_id, height);
                                        let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        let (state_root, ok) = if let Some(ref store) = chain.storage {
                                            match store.get_state_root(height) {
                                                Ok(Some(root)) => (root, true),
                                                _ => (String::new(), false),
                                            }
                                        } else {
                                            (String::new(), false)
                                        };
                                        let response = NetworkMessage::StateSnapshotResponse { height, state_root, ok };
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                    }

                                    NetworkMessage::StateSnapshotResponse { height, state_root, ok } => {
                                        if ok {
                                            info!("StateSnapshot height={} root={}...", height, &state_root[..16.min(state_root.len())]);
                                        } else {
                                            warn!("StateSnapshot unavailable at height={}", height);
                                        }
                                    }

                                    NetworkMessage::GetBlocksByHeight { from_height, to_height } => {
                                        info!("GetBlocksByHeight [{}, {}] from {}", from_height, to_height, peer_id);
                                        let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        let cap = crate::network::protocol::MAX_SNAP_BATCH;
                                        let to_height = to_height.min(from_height + cap);
                                        let mut blocks = Vec::new();
                                        if let Some(ref store) = chain.storage {
                                            for h in from_height..=to_height {
                                                match store.get_block_by_height(h) {
                                                    Ok(Some(b)) => blocks.push(b),
                                                    _ => break,
                                                }
                                            }
                                        } else {
                                            let from = from_height as usize;
                                            let to = (to_height as usize + 1).min(chain.chain.len());
                                            if from < chain.chain.len() {
                                                blocks = chain.chain[from..to].to_vec();
                                            }
                                        }
                                        info!("Sending {} blocks by height to {}", blocks.len(), peer_id);
                                        let response = NetworkMessage::BlocksByHeight(blocks);
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                    }

                                    NetworkMessage::BlocksByHeight(blocks) => {
                                        if blocks.len() > crate::network::protocol::MAX_SNAP_BATCH as usize {
                                            warn!("Too many snap-sync blocks from {}", peer_id);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_invalid_block(&peer_id);
                                            continue;
                                        }
                                        info!("Snap-sync: {} blocks from {}", blocks.len(), peer_id);
                                        let mut chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        for block in blocks {
                                            if block.index < chain.chain.len() as u64 {
                                                continue;
                                            }
                                            match chain.validate_and_add_block(block.clone()) {
                                                Ok(_) => info!("Snap-sync applied block #{}", block.index),
                                                Err(e) => warn!("Snap-sync block #{} failed: {}", block.index, e),
                                            }
                                        }
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_good_behavior(&peer_id);
                                    }

                                    NetworkMessage::SnapshotChunk { height, index, total, data } => {
                                        info!("SnapshotChunk from {}: height={}, {}/{}, {} bytes",
                                            peer_id, height, index, total, data.len());

                                    }

                                    NetworkMessage::Handshake { version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes } => {
                                        let my_chain_id = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); }).chain_id;
                                        if chain_id != my_chain_id {
                                            warn!("Peer {} has wrong chain_id {} (expected {}). Banning.", peer_id, chain_id, my_chain_id);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).ban_peer(&peer_id);
                                            continue;
                                        }
                                        info!("Handshake from {}: v{}.{}, chain={}, height={}, val_set={}, schemes={:?}",
                                            peer_id, version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes);
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).set_handshaked(&peer_id, true);

                                        let chain = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); });
                                        let response = NetworkMessage::HandshakeAck {
                                            version_major: crate::core::encoding::PROTOCOL_VERSION_MAJOR,
                                            version_minor: crate::core::encoding::PROTOCOL_VERSION_MINOR,
                                            chain_id: chain.chain_id,
                                            best_height: chain.chain.len() as u64,
                                            validator_set_hash: chain.get_validator_set_hash(),
                                            supported_schemes: vec!["ED25519".to_string(), "BLS".to_string(), "DILITHIUM".to_string()],
                                        };
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let data = response.to_bytes();
                                        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
                                            warn!("Failed to send HandshakeAck: {}", e);
                                        }
                                    }

                                    NetworkMessage::HandshakeAck { version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes } => {
                                        let my_chain_id = self.blockchain.lock().unwrap_or_else(|e| { tracing::error!("Blockchain lock poisoned: {}", e); std::process::exit(1); }).chain_id;
                                        if chain_id != my_chain_id {
                                            warn!("Peer {} Ack with wrong chain_id {} (expected {}). Banning.", peer_id, chain_id, my_chain_id);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).ban_peer(&peer_id);
                                            continue;
                                        }
                                        info!("HandshakeAck from {}: v{}.{}, chain={}, height={}, val_set={}, schemes={:?}",
                                            peer_id, version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes);
                                        let mut pm = self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); });
                                        pm.set_handshaked(&peer_id, true);
                                        pm.report_good_behavior(&peer_id);
                                    }

                                    NetworkMessage::Prevote { epoch, checkpoint_height, checkpoint_hash, voter_id, .. } => {
                                        if !self.peer_manager.lock().unwrap().check_vote_rate_limit(&peer_id) {
                                            warn!("Peer {} exceeded vote rate limit. Ignoring Prevote.", peer_id);
                                            continue;
                                        }
                                        info!("Prevote from {}: epoch={}, height={}, hash={}..., voter={}",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())], voter_id);
                                    }

                                    NetworkMessage::Precommit { epoch, checkpoint_height, checkpoint_hash, voter_id, .. } => {
                                        if !self.peer_manager.lock().unwrap().check_vote_rate_limit(&peer_id) {
                                            warn!("Peer {} exceeded vote rate limit. Ignoring Precommit.", peer_id);
                                            continue;
                                        }
                                        info!("Precommit from {}: epoch={}, height={}, hash={}..., voter={}",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())], voter_id);
                                    }

                                    NetworkMessage::FinalityCert { epoch, checkpoint_height, checkpoint_hash, agg_sig_bls, bitmap, set_hash } => {
                                        if !self.peer_manager.lock().unwrap().check_vote_rate_limit(&peer_id) {
                                            warn!("Peer {} exceeded vote rate limit. Ignoring FinalityCert.", peer_id);
                                            continue;
                                        }
                                        info!("FinalityCert from {}: epoch={}, height={}, hash={}...",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())]);

                                        let cert = crate::chain::finality::FinalityCert {
                                            epoch,
                                            checkpoint_height,
                                            checkpoint_hash,
                                            agg_sig_bls,
                                            bitmap,
                                            set_hash,
                                        };

                                        let mut chain = self.blockchain.lock().unwrap();
                                        if let Err(e) = chain.handle_finality_cert(cert) {
                                            warn!("Failed to apply FinalityCert from {}: {}", peer_id, e);
                                            self.peer_manager.lock().unwrap().report_bad_behavior(&peer_id);
                                        } else {
                                            self.peer_manager.lock().unwrap().report_good_behavior(&peer_id);
                                        }
                                    }

                                    NetworkMessage::GetQcBlob { epoch, checkpoint_height } => {
                                        if !self.peer_manager.lock().unwrap().check_rate_limit(&peer_id) {
                                            continue;
                                        }
                                        info!("GetQcBlob from {}: epoch={}, height={}", peer_id, epoch, checkpoint_height);

                                        let storage = self.blockchain.lock().unwrap().storage.clone();
                                        if let Some(store) = storage {
                                            if let Ok(Some(blob)) = store.get_qc_blob(checkpoint_height) {
                                                let response = NetworkMessage::QcBlobResponse {
                                                    epoch: blob.epoch,
                                                    checkpoint_height: blob.checkpoint_height,
                                                    checkpoint_hash: blob.checkpoint_hash.clone(),
                                                    blob_data: serde_json::to_vec(&blob).unwrap_or_default(),
                                                    found: true,
                                                };
                                                let topic = gossipsub::IdentTopic::new("blocks");
                                                let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                            } else {
                                                let response = NetworkMessage::QcBlobResponse {
                                                    epoch,
                                                    checkpoint_height,
                                                    checkpoint_hash: String::new(),
                                                    blob_data: Vec::new(),
                                                    found: false,
                                                };
                                                let topic = gossipsub::IdentTopic::new("blocks");
                                                let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                            }
                                        }
                                    }

                                    NetworkMessage::QcBlobResponse { epoch, checkpoint_height, found, .. } => {
                                        if !self.peer_manager.lock().unwrap().check_blob_rate_limit(&peer_id) {
                                            warn!("Peer {} exceeded blob rate limit. Ignoring QcBlobResponse.", peer_id);
                                            continue;
                                        }
                                        info!("QcBlobResponse from {}: epoch={}, height={}, found={}",
                                            peer_id, epoch, checkpoint_height, found);

                                        if found {
                                            self.peer_manager.lock().unwrap().report_good_behavior(&peer_id);
                                        }
                                    }
                                }
                                }
                                Err(e) => {
                                    warn!("Computed invalid message from {}: {:?}", peer_id, e);

                                    self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_oversized_message(&peer_id);
                                }
                            }
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Identify(event)) => {
                            if let identify::Event::Received { info, .. } = event {
                                info!("Received identity from {:?}", info.public_key.to_peer_id());
                                for addr in info.listen_addrs {
                                    self.swarm.behaviour_mut().kad.add_address(&info.public_key.to_peer_id(), addr);
                                }
                            }
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Kad(event)) => {
                            match event {
                                KademliaEvent::RoutingUpdated { peer, .. } => {
                                    info!("Kademlia: Routing updated for peer {}", peer);
                                }
                                _ => {}
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }
}
