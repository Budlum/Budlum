use crate::network::protocol::NetworkMessage;
use libp2p::{
    futures::StreamExt,
    gossipsub, identify, identity,
    kad::{
        store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig, Event as KademliaEvent,
    },
    mdns, noise, ping,
    request_response,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, Swarm, StreamProtocol,
};
use std::collections::hash_map::DefaultHasher;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::time::Duration;
use tracing::{info, warn};
use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
#[derive(NetworkBehaviour)]
pub struct BudlumBehaviour {
    ping: ping::Behaviour,
    identify: identify::Behaviour,
    mdns: mdns::tokio::Behaviour,
    gossipsub: gossipsub::Behaviour,
    kad: Kademlia<MemoryStore>,
    sync: request_response::Behaviour<crate::network::sync_codec::SyncCodec>,
}
use crate::chain::chain_actor::ChainHandle;
use crate::network::peer_manager::PeerManager;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
pub enum NodeCommand {
    Subscribe(String),
    Broadcast(String, NetworkMessage),
    BroadcastTx(crate::core::transaction::Transaction),
    ListPeers,
}
#[derive(Clone)]
pub struct NodeClient {
    sender: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
    pub peer_count: Arc<AtomicUsize>,
}
impl NodeClient {
    pub async fn subscribe(&self, topic: String) {
        let _ = self.sender.send(NodeCommand::Subscribe(topic)).await;
    }
    pub async fn broadcast(&self, topic: String, msg: NetworkMessage) {
        let _ = self.sender.send(NodeCommand::Broadcast(topic, msg)).await;
    }
    pub async fn broadcast_tx(&self, tx: crate::core::transaction::Transaction) {
        let _ = self.sender.send(NodeCommand::BroadcastTx(tx)).await;
    }
    pub fn broadcast_tx_sync(&self, tx: crate::core::transaction::Transaction) {
        let _ = self.sender.try_send(NodeCommand::BroadcastTx(tx));
    }
    pub async fn list_peers(&self) {
        let _ = self.sender.send(NodeCommand::ListPeers).await;
    }
}
#[tokio::test]
async fn test_node_creation() {
    use crate::consensus::pow::PoWEngine;
    use crate::chain::chain_actor::ChainActor;
    use crate::chain::blockchain::Blockchain;
    let consensus = std::sync::Arc::new(PoWEngine::new(2));
    let blockchain = Blockchain::new(consensus, None, 1337, None);
    let (chain_actor, chain) = ChainActor::new(blockchain);
    tokio::spawn(async move {
        chain_actor.run().await;
    });
    let node = Node::new(chain);
    assert!(node.is_ok());
}
pub const MAX_PEERS: usize = 50;
pub const DHT_BOOTSTRAP_INTERVAL: Duration = Duration::from_secs(300);

pub struct Node {
    swarm: Swarm<BudlumBehaviour>,
    command_rx: mpsc::Receiver<NodeCommand>,
    command_tx: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
    pub chain: ChainHandle,
    pub peer_manager: Arc<Mutex<PeerManager>>,
    pub bootstrap_peers: Vec<String>,
    pub peer_count: Arc<AtomicUsize>,
    pub in_progress_snapshots: HashMap<u64, Vec<Option<Vec<u8>>>>,
}

impl Node {
    pub fn new(chain: ChainHandle) -> Result<Self, Box<dyn Error>> {
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
                let kad_config = KademliaConfig::new(libp2p::StreamProtocol::new("/budlum/kad/1.0.0"));
                let kademlia =
                    Kademlia::with_config(key.public().to_peer_id(), kad_store, kad_config);
                let identify = identify::Behaviour::new(identify::Config::new(
                    "/budlum/1.0.0".to_string(),
                    key.public(),
                ));
                let sync = request_response::Behaviour::new(
                    [(
                        StreamProtocol::new("/budlum/sync/1.0.0"),
                        request_response::ProtocolSupport::Full,
                    )],
                    request_response::Config::default(),
                );

                Ok(BudlumBehaviour {
                    ping: ping::Behaviour::new(
                        ping::Config::new().with_interval(Duration::from_secs(15)),
                    ),
                    identify,
                    mdns,
                    gossipsub,
                    kad: kademlia,
                    sync,
                })
            })?
            .with_swarm_config(|cfg| cfg.with_idle_connection_timeout(Duration::from_secs(60)))
            .build();
        let (command_tx, command_rx) = mpsc::channel(32);
        let peer_manager = Arc::new(Mutex::new(PeerManager::new()));
        let peer_count = Arc::new(AtomicUsize::new(0));
        Ok(Node {
            swarm,
            peer_id,
            command_tx,
            command_rx,
            chain,
            peer_manager,
            bootstrap_peers: Vec::new(),
            peer_count,
            in_progress_snapshots: HashMap::new(),
        })
    }
    pub fn new_with_bootstrap(
        chain: ChainHandle,
        bootstrap_peers: Vec<String>,
    ) -> Result<Self, Box<dyn Error>> {
        let mut node = Self::new(chain)?;
        node.bootstrap_peers = bootstrap_peers;
        Ok(node)
    }
    pub fn get_client(&self) -> NodeClient {
        NodeClient {
            sender: self.command_tx.clone(),
            peer_id: self.peer_id,
            peer_count: self.peer_count.clone(),
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
        let mut finality_interval = tokio::time::interval(Duration::from_secs(30));
        let mut dht_interval = tokio::time::interval(DHT_BOOTSTRAP_INTERVAL);
        let mut banning_interval = tokio::time::interval(Duration::from_secs(60));
        let mut last_voted_height: u64 = 0;

        loop {
            tokio::select! {
                _ = gc_interval.tick() => {
                    let removed = self.chain.cleanup_mempool().await;
                    if removed > 0 {
                        info!("Cleaned up {} expired transactions from mempool", removed);
                    }

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
                _ = finality_interval.tick() => {
                    let height = self.chain.get_height().await;
                    let checkpoint_interval = crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
                    let checkpoint_height = (height / checkpoint_interval) * checkpoint_interval;

                    if checkpoint_height > 0 && checkpoint_height > last_voted_height {
                        if let Some(block) = self.chain.get_block(checkpoint_height).await {
                            let epoch = checkpoint_height / checkpoint_interval;
                            let vote_msg = NetworkMessage::Prevote {
                                epoch,
                                checkpoint_height,
                                checkpoint_hash: block.hash.clone(),
                                voter_id: self.peer_id.to_string(),
                                sig_bls: Vec::new(),
                            };
                            info!("Finality: voting for checkpoint height {} (epoch {})", checkpoint_height, epoch);
                            let topic = gossipsub::IdentTopic::new("blocks");
                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, vote_msg.to_bytes());
                            last_voted_height = checkpoint_height;
                        }
                    }
                }
                _ = dht_interval.tick() => {
                    info!("Running periodic DHT bootstrapping...");
                    let _ = self.swarm.behaviour_mut().kad.bootstrap();
                }
                _ = banning_interval.tick() => {
                    let banned_peers = {
                        match self.peer_manager.lock() {
                            Ok(pm) => pm.get_banned_peers(),
                            Err(e) => {
                                tracing::error!("PeerManager lock poisoned in banning task: {}", e);
                                Vec::new()
                            }
                        }
                    };
                    for peer_id in banned_peers {
                        warn!("Proactively disconnecting banned peer: {}", peer_id);
                        let _ = self.swarm.disconnect_peer_id(peer_id);
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
                            NodeCommand::BroadcastTx(tx) => {
                                let msg = NetworkMessage::Transaction(tx);
                                let topic = gossipsub::IdentTopic::new("transactions");
                                if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, msg.to_bytes()) {
                                    warn!("Failed to gossip transaction: {}", e);
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
                            let count = self.peer_count.fetch_add(1, Ordering::SeqCst) + 1;
                            if count > MAX_PEERS {
                                warn!("Max peers reached ({}/{}), disconnecting {}", count, MAX_PEERS, peer_id);
                                let _ = self.swarm.disconnect_peer_id(peer_id);
                                self.peer_count.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                            info!("Connected to {}, Peers: {}", peer_id, count);

                            let handshake = NetworkMessage::Handshake {
                                version_major: crate::core::encoding::PROTOCOL_VERSION_MAJOR,
                                version_minor: crate::core::encoding::PROTOCOL_VERSION_MINOR,
                                chain_id: self.chain.get_chain_id().await,
                                best_height: self.chain.get_height().await + 1,
                                validator_set_hash: self.chain.get_validator_set_hash().await,
                                supported_schemes: vec!["ED25519".to_string(), "BLS".to_string(), "DILITHIUM".to_string()],
                            };

                            let chain_len = self.chain.get_height().await + 1;
                            info!("DEBUG: Connected to {}, Chain length: {}, sending Handshake", peer_id, chain_len);

                            let topic = gossipsub::IdentTopic::new("blocks");
                            if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, handshake.to_bytes()) {
                                warn!("Failed to send Handshake: {}", e);
                            }

                            if self.chain.get_height().await == 0 {
                                if let Some(last_block) = self.chain.get_block(0).await {
                                    let locator = vec![last_block.hash];
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
                        }
                        SwarmEvent::ConnectionClosed { peer_id, .. } => {
                            self.peer_count.fetch_sub(1, Ordering::SeqCst);
                            warn!("Disconnected from {}, Peers: {}", peer_id, self.peer_count.load(Ordering::SeqCst));
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Ping(_event)) => {
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

                            if let Ok(pm) = self.peer_manager.lock() {
                                if pm.is_banned(&peer_id) {
                                    warn!("Ignoring message from banned peer {}", peer_id);
                                    continue;
                                }
                            }

                            if !self.peer_manager.lock().map(|mut pm| pm.check_rate_limit(&peer_id)).unwrap_or(false) {
                                warn!("Rate limit exceeded or lock error for peer {}", peer_id);
                                continue;
                            }

                            info!("Received from {}: id={}", peer_id, id);
                            match NetworkMessage::from_bytes_validated(&message.data) {
                                Ok(msg) => {
                                    let is_handshake_msg = matches!(
                                        msg,
                                        NetworkMessage::Handshake { .. } | NetworkMessage::HandshakeAck { .. }
                                    );

                                    let is_handshaked = self.peer_manager.lock()
                                        .map(|pm| pm.is_handshaked(&peer_id))
                                        .unwrap_or(false);

                                    if !is_handshake_msg && !is_handshaked {
                                        warn!("Peer {} sent {:?} before completing handshake, dropping.", peer_id, msg);

                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_invalid_tx(&peer_id);
                                        }
                                        continue;
                                    }

                                    match msg {
                                        NetworkMessage::Block(block) => {
                                        if let Err(e) = NetworkMessage::validate_block_size(&block) {
                                            warn!("Received oversized block from {}: {:?}", peer_id, e);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_oversized_message(&peer_id);
                                            continue;
                                        }
                                        info!("BLOCK: #{} Hash: {}...", block.index, &block.hash[..8.min(block.hash.len())]);
                                        let our_height = self.chain.get_height().await;
                                        if block.index == our_height + 1 {
                                            match self.chain.validate_and_add_block(block.clone()).await {
                                                Ok(_) => {
                                                    info!("Added block #{} to local chain", block.index);
                                                    if let Ok(mut pm) = self.peer_manager.lock() {
                                                        pm.report_good_behavior(&peer_id);
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!("Block validation failed: {}", e);
                                                    if let Ok(mut pm) = self.peer_manager.lock() {
                                                        pm.report_invalid_block(&peer_id);
                                                    }
                                                }
                                            }
                                        } else if block.index <= our_height {
                                            if let Some(our_block) = self.chain.get_block(block.index).await {
                                                if our_block.hash != block.hash {
                                                    info!("Fork detected at height {} (ours: {}... theirs: {}...)", block.index, &our_block.hash[..8.min(our_block.hash.len())], &block.hash[..8.min(block.hash.len())]);
                                                    
                                                    info!("Fork detected at height {} - initiating sync to resolve fork", block.index);
                                                    let locator = self.chain.get_locator().await;
                                                    let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                                    let topic = gossipsub::IdentTopic::new("blocks");
                                                    let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                                }
                                            }
                                        } else {
                                            info!("Block #{} is ahead of our chain (height={}), requesting sync", block.index, our_height);
                                            let locator = self.chain.get_locator().await;
                                            let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                        }
                                    }
                                    NetworkMessage::Transaction(tx) => {
                                        if let Err(e) = NetworkMessage::validate_tx_size(&tx) {
                                            warn!("Received oversized transaction from {}: {:?}", peer_id, e);
                                            if let Ok(mut pm) = self.peer_manager.lock() {
                                                pm.report_oversized_message(&peer_id);
                                            }
                                            continue;
                                        }
                                        info!("Broadcasted tx: {} from: {} to: {} amount: {}", 
                            &tx.hash[..8], tx.from, tx.to, tx.amount);
                                        match self.chain.add_transaction(tx).await {
                                            Ok(_) => {
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_good_behavior(&peer_id);
                                                }
                                            }
                                            Err(e) => {
                                                warn!("Failed to add transaction: {}", e);
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_invalid_tx(&peer_id);
                                                }
                                            }
                                        }
                                    }

                                    NetworkMessage::GetHeaders { locator, limit } => {
                                        info!("GetHeaders request from {} (locator: {} hashes, limit: {})",
                                            peer_id, locator.len(), limit);
                                        
                                        let start_idx_opt = self.chain.find_common_height(locator).await;
                                        let start_idx = start_idx_opt.map(|i| i + 1).unwrap_or(0) as usize;

                                        let height = self.chain.get_height().await + 1;
                                        let end_idx = (start_idx + limit as usize).min(height as usize);
                                        
                                        let mut headers = Vec::new();
                                        for h in start_idx..end_idx {
                                            if let Some(block) = self.chain.get_block(h as u64).await {
                                                headers.push(crate::core::block::BlockHeader::from_block(&block));
                                            }
                                        }

                                        info!("Sending {} headers to {}", headers.len(), peer_id);
                                        let response = NetworkMessage::Headers(headers);
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                    }

                                    NetworkMessage::Headers(headers) => {
                                        if headers.len() > crate::network::protocol::MAX_HEADERS_PER_REQUEST as usize {
                                            if let Ok(mut pm) = self.peer_manager.lock() {
                                                pm.report_invalid_block(&peer_id);
                                            }
                                            continue;
                                        }
                                        if let Some(last_header) = headers.last() {
                                            let from = headers[0].index;
                                            let to = last_header.index;
                                            let req = NetworkMessage::GetBlocksRange { from, to };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                        }
                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_good_behavior(&peer_id);
                                        }
                                    }

                                    NetworkMessage::GetBlocksRange { from, to } => {
                                        info!("GetBlocksRange request from {} ({}..{})", peer_id, from, to);
                                        let our_height = self.chain.get_height().await + 1;

                                        let from_idx = from as usize;
                                        let to_idx = (to as usize).min(our_height as usize);
                                        let max_blocks = crate::network::protocol::MAX_CHAIN_SYNC_BLOCKS;
                                        let to_idx = to_idx.min(from_idx + max_blocks);

                                        if (from_idx as u64) < our_height {
                                            let mut blocks = Vec::new();
                                            for h in from_idx..to_idx {
                                                if let Some(block) = self.chain.get_block(h as u64).await {
                                                    blocks.push(block);
                                                }
                                            }
                                            info!("Sending {} blocks to {}", blocks.len(), peer_id);
                                            let response = NetworkMessage::Blocks(blocks);
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                        }
                                    }

                                    NetworkMessage::Blocks(blocks) => {
                                        if blocks.len() > crate::network::protocol::MAX_CHAIN_SYNC_BLOCKS {
                                            if let Ok(mut pm) = self.peer_manager.lock() {
                                                pm.report_invalid_block(&peer_id);
                                            }
                                            continue;
                                        }
                                        if !blocks.is_empty() {
                                            let start_idx = blocks[0].index;
                                            let our_block_at_start = self.chain.get_block(start_idx).await;
                                            if let Some(our_b) = our_block_at_start {
                                                if our_b.hash != blocks[0].hash {
                                                    let _ = self.chain.try_reorg(blocks.clone()).await;
                                                } else {
                                                    for block in blocks {
                                                        let h = self.chain.get_height().await;
                                                        if block.index == h + 1 {
                                                            let _ = self.chain.validate_and_add_block(block.clone()).await;
                                                        }
                                                    }
                                                }
                                            } else {
                                                for block in blocks {
                                                    let h = self.chain.get_height().await;
                                                    if block.index == h + 1 {
                                                        let _ = self.chain.validate_and_add_block(block.clone()).await;
                                                    }
                                                }
                                            }
                                        }
                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_good_behavior(&peer_id);
                                        }
                                    }

                                    NetworkMessage::NewTip { height, hash: _ } => {
                                        let our_height = self.chain.get_height().await;
                                        if height > our_height {
                                            let locator = self.chain.get_locator().await;
                                            let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                        }
                                    }

                                    NetworkMessage::StateSnapshotResponse { height, state_root, ok } => {
                                        if ok {
                                            info!("Received StateSnapshotResponse: height={}, root={}", height, state_root);
                                        } else {
                                            warn!("Peer {} reported snapshot unavailable at height {}", peer_id, height);
                                        }
                                    }

                                    NetworkMessage::GetStateSnapshot { height } => {
                                        info!("GetStateSnapshot request from {} (height: {})", peer_id, height);
                                        let snapshot_opt = self.chain.get_state_snapshot_data(height).await;
                                        if let Some(snapshot) = snapshot_opt {
                                            let chunks = snapshot.chunk(512 * 1024); // 512KB chunks
                                            let total = chunks.len() as u32;
                                            for (i, chunk_data) in chunks.into_iter().enumerate() {
                                                let chunk_msg = NetworkMessage::SnapshotChunk {
                                                    height,
                                                    index: i as u32,
                                                    total,
                                                    data: chunk_data,
                                                };
                                                let topic = gossipsub::IdentTopic::new("blocks");
                                                let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, chunk_msg.to_bytes());
                                            }
                                            info!("Sent {} snapshot chunks for height {}", total, height);
                                        } else {
                                            let response = NetworkMessage::StateSnapshotResponse { height, state_root: "".into(), ok: false };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                        }
                                    }

                                    NetworkMessage::SnapshotChunk { height, index, total, data } => {
                                        info!("Received snapshot chunk {}/{} for height {}", index + 1, total, height);
                                        let entry = self.in_progress_snapshots.entry(height).or_insert_with(|| vec![None; total as usize]);
                                        if (index as usize) < entry.len() {
                                            entry[index as usize] = Some(data);
                                        }

                                        if entry.iter().all(|c| c.is_some()) {
                                            info!("Snapshot reassembly complete for height {}", height);
                                            let mut full_data = Vec::new();
                                            for chunk in entry.drain(..) {
                                                full_data.extend(chunk.unwrap());
                                            }
                                            self.in_progress_snapshots.remove(&height);
                                            
                                            match crate::chain::snapshot::StateSnapshot::from_bytes(&full_data) {
                                                Ok(snapshot) => {
                                                    info!("Applying snapshot at height {}", snapshot.height);
                                                    let chain = self.chain.clone();
                                                    tokio::spawn(async move {
                                                        if let Err(e) = chain.apply_snapshot(snapshot).await {
                                                            warn!("Failed to apply snapshot: {}", e);
                                                        }
                                                    });
                                                }
                                                Err(e) => warn!("Failed to parse reassembled snapshot: {}", e),
                                            }
                                        }
                                    }

                                    NetworkMessage::GetBlocksByHeight { from_height, to_height } => {
                                        info!("GetBlocksByHeight [{}, {}] from {}", from_height, to_height, peer_id);
                                        let mut blocks = Vec::new();
                                        for h in from_height..=to_height {
                                            if let Some(b) = self.chain.get_block(h).await {
                                                blocks.push(b);
                                                if blocks.len() >= crate::network::protocol::MAX_SNAP_BATCH as usize {
                                                    break;
                                                }
                                            } else {
                                                break;
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
                                        for block in blocks {
                                            let h = self.chain.get_height().await;
                                            if block.index >= h + 1 {
                                                match self.chain.validate_and_add_block(block.clone()).await {
                                                    Ok(_) => info!("Snap-sync applied block #{}", block.index),
                                                    Err(e) => warn!("Snap-sync block #{} failed: {}", block.index, e),
                                                }
                                            }
                                        }
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).report_good_behavior(&peer_id);
                                    }

                                    NetworkMessage::Handshake { version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes } => {
                                        let my_chain_id = self.chain.get_chain_id().await;
                                        if chain_id != my_chain_id {
                                            warn!("Peer {} has wrong chain_id {} (expected {}). Banning.", peer_id, chain_id, my_chain_id);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).ban_peer(&peer_id);
                                            continue;
                                        }
                                        info!("Handshake from {}: v{}.{}, chain={}, height={}, val_set={}, schemes={:?}",
                                            peer_id, version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes);
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).set_handshaked(&peer_id, true);

                                        let response = NetworkMessage::HandshakeAck {
                                            version_major: crate::core::encoding::PROTOCOL_VERSION_MAJOR,
                                            version_minor: crate::core::encoding::PROTOCOL_VERSION_MINOR,
                                            chain_id: my_chain_id,
                                            best_height: self.chain.get_height().await + 1,
                                            validator_set_hash: self.chain.get_validator_set_hash().await,
                                            supported_schemes: vec!["ED25519".to_string(), "BLS".to_string(), "DILITHIUM".to_string()],
                                        };
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let data = response.to_bytes();
                                        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, data) {
                                            warn!("Failed to send HandshakeAck: {}", e);
                                        }
                                    }

                                    NetworkMessage::HandshakeAck { version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes } => {
                                        let my_chain_id = self.chain.get_chain_id().await;
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
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_vote_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded vote rate limit or lock error. Ignoring Prevote.", peer_id);
                                            continue;
                                        }
                                        info!("Prevote from {}: epoch={}, height={}, hash={}..., voter={}",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())], voter_id);
                                    }

                                    NetworkMessage::Precommit { epoch, checkpoint_height, checkpoint_hash, voter_id, .. } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_vote_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded vote rate limit or lock error. Ignoring Precommit.", peer_id);
                                            continue;
                                        }
                                        info!("Precommit from {}: epoch={}, height={}, hash={}..., voter={}",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())], voter_id);
                                    }

                                    NetworkMessage::FinalityCert { epoch, checkpoint_height, checkpoint_hash, agg_sig_bls, bitmap, set_hash } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_vote_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded vote rate limit or lock error. Ignoring FinalityCert.", peer_id);
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

                                        match self.chain.handle_finality_cert(cert).await {
                                            Ok(_) => {
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_good_behavior(&peer_id);
                                                }
                                            }
                                            Err(e) => {
                                                warn!("Failed to apply FinalityCert from {}: {}", peer_id, e);
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_bad_behavior(&peer_id);
                                                }
                                            }
                                        }
                                    }

                                    NetworkMessage::GetQcBlob { epoch, checkpoint_height } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            continue;
                                        }
                                        info!("GetQcBlob from {}: epoch={}, height={}", peer_id, epoch, checkpoint_height);

                                        let blob = self.chain.get_qc_blob(checkpoint_height).await;
                                        let found = blob.is_some();
                                        let response = NetworkMessage::QcBlobResponse {
                                            epoch,
                                            checkpoint_height,
                                            checkpoint_hash: blob.as_ref().map(|b| b.checkpoint_hash.clone()).unwrap_or_default(),
                                            blob_data: blob.as_ref().map(|b| serde_json::to_vec(b).unwrap_or_default()).unwrap_or_default(),
                                            found,
                                        };
                                        let topic = gossipsub::IdentTopic::new("blocks");
                                        let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                    }

                                    NetworkMessage::QcBlobResponse { epoch, checkpoint_height, found, .. } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_blob_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded blob rate limit or lock error. Ignoring QcBlobResponse.", peer_id);
                                            continue;
                                        }
                                        info!("QcBlobResponse from {}: epoch={}, height={}, found={}",
                                            peer_id, epoch, checkpoint_height, found);

                                        if found {
                                            if let Ok(mut pm) = self.peer_manager.lock() {
                                                pm.report_good_behavior(&peer_id);
                                            }
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
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Sync(event)) => {
                            match event {
                                request_response::Event::Message { peer, message } => {
                                    match message {
                                        request_response::Message::Request { request, channel, .. } => {
                                            if let Ok(msg) = NetworkMessage::from_bytes(&request) {
                                                match msg {
                                                    NetworkMessage::GetHeaders { locator, limit } => {
                                                        let start_idx_opt = self.chain.find_common_height(locator).await;
                                                        let start_idx = start_idx_opt.map(|i| i + 1).unwrap_or(0) as usize;
                                                        let height = self.chain.get_height().await + 1;
                                                        let end_idx = (start_idx + limit as usize).min(height as usize);
                                                        
                                                        let mut headers = Vec::new();
                                                        for h in start_idx..end_idx {
                                                            if let Some(block) = self.chain.get_block(h as u64).await {
                                                                headers.push(crate::core::block::BlockHeader::from_block(&block));
                                                            }
                                                        }
                                                        let response = NetworkMessage::Headers(headers);
                                                        let _ = self.swarm.behaviour_mut().sync.send_response(channel, response.to_bytes());
                                                    }
                                                    NetworkMessage::GetBlocksRange { from, to } => {
                                                        let our_height = self.chain.get_height().await + 1;
                                                        let from_idx = from as usize;
                                                        let to_idx = (to as usize).min(our_height as usize);
                                                        let max_blocks = crate::network::protocol::MAX_CHAIN_SYNC_BLOCKS;
                                                        let to_idx = to_idx.min(from_idx + max_blocks);

                                                        let mut blocks = Vec::new();
                                                        if (from_idx as u64) < our_height {
                                                            for h in from_idx..to_idx {
                                                                if let Some(block) = self.chain.get_block(h as u64).await {
                                                                    blocks.push(block);
                                                                }
                                                            }
                                                        }
                                                        let response = NetworkMessage::Blocks(blocks);
                                                        let _ = self.swarm.behaviour_mut().sync.send_response(channel, response.to_bytes());
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                        request_response::Message::Response { response, .. } => {
                                            if let Ok(msg) = NetworkMessage::from_bytes(&response) {
                                                match msg {
                                                    NetworkMessage::Headers(headers) => {
                                                        if !headers.is_empty() {
                                                            let from = headers[0].index;
                                                            let to = headers.last().unwrap().index;
                                                            let req = NetworkMessage::GetBlocksRange { from, to };
                                                            let _ = self.swarm.behaviour_mut().sync.send_request(&peer, req.to_bytes());
                                                        }
                                                        self.peer_manager.lock().unwrap().report_good_behavior(&peer);
                                                    }
                                                    NetworkMessage::Blocks(blocks) => {
                                                        if !blocks.is_empty() {
                                                            let start_idx = blocks[0].index;
                                                            let our_block = self.chain.get_block(start_idx).await;
                                                            if let Some(our_b) = our_block {
                                                                if our_b.hash != blocks[0].hash {
                                                                    let _ = self.chain.try_reorg(blocks).await;
                                                                } else {
                                                                    for block in blocks {
                                                                        let h = self.chain.get_height().await;
                                                                        if block.index == h + 1 {
                                                                            let _ = self.chain.validate_and_add_block(block).await;
                                                                        }
                                                                    }
                                                                }
                                                            } else {
                                                                for block in blocks {
                                                                    let h = self.chain.get_height().await;
                                                                    if block.index == h + 1 {
                                                                        let _ = self.chain.validate_and_add_block(block).await;
                                                                    }
                                                                }
                                                            }
                                                        }
                                                        self.peer_manager.lock().unwrap().report_good_behavior(&peer);
                                                    }
                                                    _ => {}
                                                }
                                            }
                                        }
                                    }
                                }
                                request_response::Event::OutboundFailure { peer, error, .. } => {
                                    warn!("Outbound sync failure to {}: {:?}", peer, error);
                                    let mut pm = self.peer_manager.lock().unwrap();
                                    pm.report_timeout(&peer);
                                }
                                request_response::Event::InboundFailure { peer, error, .. } => {
                                    warn!("Inbound sync failure from {}: {:?}", peer, error);
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
