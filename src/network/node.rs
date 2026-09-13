use crate::network::protocol::NetworkMessage;
use libp2p::{
    futures::StreamExt,
    gossipsub, identify, identity,
    kad::{
        store::MemoryStore, Behaviour as Kademlia, Config as KademliaConfig, Event as KademliaEvent,
    },
    mdns, noise, ping, request_response,
    swarm::{NetworkBehaviour, SwarmEvent},
    tcp, yamux, Multiaddr, PeerId, StreamProtocol, Swarm,
};
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::error::Error;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tracing::{info, warn};
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
use crate::chain::finality::{Precommit, Prevote};
use crate::network::peer_manager::PeerManager;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
#[allow(clippy::large_enum_variant)]
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
    sync_state: Arc<AtomicUsize>,
    peer_manager: Arc<Mutex<PeerManager>>,
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
    pub fn is_syncing(&self) -> bool {
        self.sync_state.load(Ordering::SeqCst) == 1
    }
    pub fn broadcast_domain_commitment_sync(&self, commitment: crate::domain::DomainCommitment) {
        let _ = self.sender.try_send(NodeCommand::Broadcast(
            "blocks".into(),
            NetworkMessage::DomainCommitment(commitment),
        ));
    }
    pub fn broadcast_verified_domain_commitment_sync(
        &self,
        payload: crate::domain::VerifiedDomainCommitment,
    ) {
        let _ = self.sender.try_send(NodeCommand::Broadcast(
            "blocks".into(),
            NetworkMessage::VerifiedDomainCommitment(payload),
        ));
    }
    pub fn broadcast_cross_domain_message_sync(
        &self,
        msg: crate::cross_domain::CrossDomainMessage,
    ) {
        let _ = self.sender.try_send(NodeCommand::Broadcast(
            "blocks".into(),
            NetworkMessage::CrossDomainMessage(msg),
        ));
    }
    pub fn broadcast_slashing_evidence_sync(
        &self,
        evidence: crate::consensus::pos::SlashingEvidence,
    ) {
        let _ = self.sender.try_send(NodeCommand::Broadcast(
            "blocks".into(),
            NetworkMessage::SlashingEvidence(evidence),
        ));
    }
    pub fn admin_ban_peer(&self, peer_id: &PeerId) -> Result<(), String> {
        let mut pm = self
            .peer_manager
            .lock()
            .map_err(|_| "Peer manager lock poisoned".to_string())?;
        pm.ban_peer(peer_id);
        Ok(())
    }
    pub fn admin_unban_peer(&self, peer_id: &PeerId) -> Result<(), String> {
        let mut pm = self
            .peer_manager
            .lock()
            .map_err(|_| "Peer manager lock poisoned".to_string())?;
        pm.unban_peer(peer_id);
        Ok(())
    }
    pub fn admin_list_banned_peers(&self) -> Result<Vec<String>, String> {
        let pm = self
            .peer_manager
            .lock()
            .map_err(|_| "Peer manager lock poisoned".to_string())?;
        Ok(pm
            .get_banned_peers()
            .iter()
            .map(|p| p.to_string())
            .collect())
    }
}
#[tokio::test]
async fn test_node_creation() {
    use crate::chain::blockchain::Blockchain;
    use crate::chain::chain_actor::ChainActor;
    use crate::consensus::pow::PoWEngine;
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

pub fn load_or_generate_identity_key(path: Option<&str>) -> identity::Keypair {
    if let Some(p) = path {
        let file_path = std::path::Path::new(p);
        if file_path.exists() {
            match std::fs::read(file_path) {
                Ok(bytes) => {
                    if let Ok(keypair) = identity::Keypair::from_protobuf_encoding(&bytes) {
                        info!("Loaded persistent P2P identity from {}", p);
                        return keypair;
                    }
                    warn!("Failed to decode identity file {}, generating new key", p);
                }
                Err(e) => warn!("Cannot read identity file {}: {}, generating new key", p, e),
            }
        }
        let key = identity::Keypair::generate_ed25519();
        if let Some(parent) = file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match key.to_protobuf_encoding() {
            Ok(encoded) => {
                if let Err(e) = std::fs::write(file_path, &encoded) {
                    warn!("Failed to save identity key to {}: {}", p, e);
                } else {
                    info!("Saved new P2P identity key to {}", p);
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let _ =
                        std::fs::set_permissions(file_path, std::fs::Permissions::from_mode(0o600));
                }
            }
            Err(e) => warn!("Failed to encode identity key: {}", e),
        }
        key
    } else {
        let key = identity::Keypair::generate_ed25519();
        info!("Generated ephemeral P2P identity (no identity file configured)");
        key
    }
}

pub fn resolve_dns_seeds(seeds: &[String], port: u16) -> Vec<String> {
    let mut addrs = Vec::new();
    for seed in seeds {
        let dns_host = format!("{}:{}", seed, if seed.contains(':') { 0 } else { port });
        match std::net::ToSocketAddrs::to_socket_addrs(&dns_host.as_str()) {
            Ok(socket_addrs) => {
                for sa in socket_addrs {
                    let multiaddr: String = match sa {
                        std::net::SocketAddr::V4(addr) => {
                            format!("/ip4/{}/tcp/{}", addr.ip(), addr.port())
                        }
                        std::net::SocketAddr::V6(addr) => {
                            format!("/ip6/{}/tcp/{}", addr.ip(), addr.port())
                        }
                    };
                    addrs.push(multiaddr);
                }
            }
            Err(e) => warn!("DNS seed resolution failed for {}: {}", seed, e),
        }
    }
    addrs
}

pub struct Node {
    swarm: Swarm<BudlumBehaviour>,
    command_rx: mpsc::Receiver<NodeCommand>,
    command_tx: mpsc::Sender<NodeCommand>,
    pub peer_id: PeerId,
    pub chain: ChainHandle,
    pub peer_manager: Arc<Mutex<PeerManager>>,
    pub bootstrap_peers: Vec<String>,
    pub dns_seeds: Vec<String>,
    pub peer_count: Arc<AtomicUsize>,
    pub sync_state: Arc<AtomicUsize>,
    #[allow(clippy::type_complexity)]
    pub in_progress_snapshots: HashMap<u64, (u64, Vec<Option<Vec<u8>>>)>,
    pub max_peers: usize,
    pub validator_address: Option<crate::core::address::Address>,
    pub last_precommit_height: u64,
    pub identity_path: Option<std::path::PathBuf>,
    pub banned_peer_db: Option<std::path::PathBuf>,
    pub mdns_enabled: bool,
    pub metrics: Option<Arc<crate::core::metrics::Metrics>>,
}

impl Node {
    pub fn new(chain: ChainHandle) -> Result<Self, Box<dyn Error>> {
        let local_key = identity::Keypair::generate_ed25519();
        Self::with_key(chain, local_key, true)
    }

    pub fn with_key(
        chain: ChainHandle,
        local_key: identity::Keypair,
        mdns_enabled: bool,
    ) -> Result<Self, Box<dyn Error>> {
        let peer_id = PeerId::from(local_key.public());
        info!("Node ID: {} (mDNS: {})", peer_id, mdns_enabled);
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
            .map_err(std::io::Error::other)?;
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
                let kad_config =
                    KademliaConfig::new(libp2p::StreamProtocol::new("/budlum/kad/1.0.0"));
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
        let sync_state = Arc::new(AtomicUsize::new(0));
        Ok(Node {
            swarm,
            peer_id,
            command_tx,
            command_rx,
            chain,
            peer_manager,
            bootstrap_peers: Vec::new(),
            dns_seeds: Vec::new(),
            peer_count,
            sync_state,
            in_progress_snapshots: HashMap::new(),
            max_peers: MAX_PEERS,
            validator_address: None,
            last_precommit_height: 0,
            identity_path: None,
            banned_peer_db: None,
            mdns_enabled,
            metrics: None,
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
    pub fn apply_network_security(&mut self, network: crate::core::chain_config::Network) {
        let security = network.security_config();
        self.max_peers = security.max_peers;
        self.mdns_enabled = security.mdns_enabled;
        if security.persist_banned_peers && self.banned_peer_db.is_none() {
            self.banned_peer_db = Some(std::path::PathBuf::from(
                format!("./data/{:?}/banned-peers.json", network).to_lowercase(),
            ));
        }
    }

    pub fn with_identity(mut self, path: Option<String>) -> Self {
        self.identity_path = path.map(std::path::PathBuf::from);
        self
    }

    pub fn with_banned_peer_db(mut self, path: Option<String>) -> Self {
        self.banned_peer_db = path.map(std::path::PathBuf::from);
        self
    }

    pub fn with_dns_seeds(mut self, seeds: Vec<String>) -> Self {
        self.dns_seeds = seeds;
        self
    }

    pub fn with_bootstrap_peers(mut self, peers: Vec<String>) -> Self {
        self.bootstrap_peers = peers;
        self
    }

    pub fn with_metrics(mut self, metrics: Arc<crate::core::metrics::Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }
    pub fn get_client(&self) -> NodeClient {
        NodeClient {
            sender: self.command_tx.clone(),
            peer_id: self.peer_id,
            peer_count: self.peer_count.clone(),
            sync_state: self.sync_state.clone(),
            peer_manager: self.peer_manager.clone(),
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
    fn load_banned_peers_from_db(&self) {
        let Some(ref db_path) = self.banned_peer_db else {
            return;
        };
        match std::fs::read_to_string(db_path) {
            Ok(data) => {
                #[derive(serde::Deserialize)]
                struct BanList {
                    banned_peers: Vec<String>,
                }
                if let Ok(list) = serde_json::from_str::<BanList>(&data) {
                    if !list.banned_peers.is_empty() {
                        if let Ok(mut pm) = self.peer_manager.lock() {
                            pm.reload_banned_peers(&list.banned_peers);
                            info!(
                                "Reloaded {} banned peers from {}",
                                list.banned_peers.len(),
                                db_path.display()
                            );
                        }
                    }
                }
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    warn!("Failed to read banned peer DB: {}", e);
                }
            }
        }
    }

    fn save_banned_peers_to_db(&self) {
        let Some(ref db_path) = self.banned_peer_db else {
            return;
        };
        let banned_peers = match self.peer_manager.lock() {
            Ok(pm) => pm.get_persisted_banned_peers(),
            Err(_) => return,
        };
        if banned_peers.is_empty() {
            return;
        }
        let json = serde_json::json!({ "banned_peers": banned_peers });
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
            if let Err(e) = std::fs::write(
                db_path,
                serde_json::to_string_pretty(&json).unwrap_or_default(),
            ) {
                warn!("Failed to persist banned peers: {}", e);
            }
        }
    }

    pub async fn run(&mut self) {
        info!("Node running...");

        // Load durable banned peers
        self.load_banned_peers_from_db();

        // Bootstrap from configured peers
        for addr in self.bootstrap_peers.clone() {
            if let Err(e) = self.bootstrap(&addr) {
                warn!("Bootstrap dial failed for {}: {}", addr, e);
            }
        }

        // Resolve and dial DNS seeds
        if !self.dns_seeds.is_empty() {
            let dns_addrs = resolve_dns_seeds(&self.dns_seeds, 0);
            for addr in &dns_addrs {
                if let Err(e) = self.dial(addr) {
                    warn!("DNS seed dial failed for {}: {}", addr, e);
                }
            }
        }

        let mut gc_interval = tokio::time::interval(Duration::from_secs(60));
        let mut discovery_interval = tokio::time::interval(Duration::from_secs(300));
        let mut finality_interval = tokio::time::interval(Duration::from_secs(30));
        let mut slashing_gossip_interval = tokio::time::interval(Duration::from_secs(5));
        let mut dht_interval = tokio::time::interval(DHT_BOOTSTRAP_INTERVAL);
        let mut banning_interval = tokio::time::interval(Duration::from_secs(60));
        let mut ban_persist_interval = tokio::time::interval(Duration::from_secs(300));
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
                    // Resolve validator address lazily
                    if self.validator_address.is_none() {
                        self.validator_address = self.chain.get_validator_address().await;
                    }

                    let Some(voter_addr) = self.validator_address else {
                        continue;
                    };

                    let height = self.chain.get_height().await;
                    let checkpoint_interval = crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
                    let checkpoint_height = (height / checkpoint_interval) * checkpoint_interval;

                    // --- Check aggregator state for auto-precommit ---
                    let agg_state = self.chain.get_aggregator_state().await;
                    if agg_state.active
                        && agg_state.prevote_quorum_reached
                        && !agg_state.precommit_quorum_reached
                        && checkpoint_height > self.last_precommit_height
                    {
                        match self.chain.sign_precommit(
                            agg_state.epoch,
                            agg_state.checkpoint_height,
                            agg_state.checkpoint_hash.clone(),
                            voter_addr,
                        ).await {
                            Ok(precommit) => {
                                info!(
                                    "Finality: auto-precommit for checkpoint height {} (epoch {})",
                                    agg_state.checkpoint_height, agg_state.epoch
                                );
                                let vote_msg = NetworkMessage::Precommit {
                                    epoch: precommit.epoch,
                                    checkpoint_height: precommit.checkpoint_height,
                                    checkpoint_hash: precommit.checkpoint_hash,
                                    voter_id: voter_addr.to_hex(),
                                    sig_bls: precommit.sig_bls,
                                };
                                let topic = gossipsub::IdentTopic::new("blocks");
                                let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, vote_msg.to_bytes());
                                self.last_precommit_height = agg_state.checkpoint_height;
                            }
                            Err(e) => {
                                warn!("Failed to sign precommit: {}", e);
                            }
                        }
                    }

                    // --- Periodic prevote ---
                    if checkpoint_height > 0 && checkpoint_height > last_voted_height {
                        if let Some(block) = self.chain.get_block(checkpoint_height).await {
                            let epoch = checkpoint_height / checkpoint_interval;
                            match self.chain.sign_prevote(
                                epoch,
                                checkpoint_height,
                                block.hash.clone(),
                                voter_addr,
                            ).await {
                                Ok(prevote) => {
                                    info!(
                                        "Finality: voting for checkpoint height {} (epoch {})",
                                        checkpoint_height, epoch
                                    );
                                    let vote_msg = NetworkMessage::Prevote {
                                        epoch: prevote.epoch,
                                        checkpoint_height: prevote.checkpoint_height,
                                        checkpoint_hash: prevote.checkpoint_hash,
                                        voter_id: voter_addr.to_hex(),
                                        sig_bls: prevote.sig_bls,
                                    };
                                    let topic = gossipsub::IdentTopic::new("blocks");
                                    let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, vote_msg.to_bytes());
                                    last_voted_height = checkpoint_height;
                                }
                                Err(e) => {
                                    warn!("Failed to sign prevote: {}", e);
                                }
                            }
                        }
                    }
                }
                _ = slashing_gossip_interval.tick() => {
                    for evidence in self.chain.drain_slashing_evidence().await {
                        let topic = gossipsub::IdentTopic::new("blocks");
                        let msg = NetworkMessage::SlashingEvidence(evidence);
                        if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, msg.to_bytes()) {
                            warn!("Failed to gossip slashing evidence: {}", e);
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
                _ = ban_persist_interval.tick() => {
                    self.save_banned_peers_to_db();
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
                            if count > self.max_peers {
                                warn!("Max peers reached ({}/{}), disconnecting {}", count, self.max_peers, peer_id);
                                let _ = self.swarm.disconnect_peer_id(peer_id);
                                self.peer_count.fetch_sub(1, Ordering::SeqCst);
                                continue;
                            }
                            if let Some(ref m) = self.metrics {
                                m.p2p_peers_connected.set(count as i64);
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
                                    self.sync_state.store(1, Ordering::SeqCst);
                                    if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, msg.to_bytes()) {
                                        warn!("Failed to request headers: {}", e);
                                        self.sync_state.store(0, Ordering::SeqCst);
                                    }
                                }
                            }
                        }
                        SwarmEvent::ConnectionClosed { peer_id, .. } => {
                            self.peer_count.fetch_sub(1, Ordering::SeqCst);
                            if let Some(ref m) = self.metrics {
                                m.p2p_peers_connected.set(self.peer_count.load(Ordering::SeqCst) as i64);
                            }
                            warn!("Disconnected from {}, Peers: {}", peer_id, self.peer_count.load(Ordering::SeqCst));
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Ping(_event)) => {
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Mdns(event)) => {
                            if !self.mdns_enabled {
                                continue;
                            }
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

                            if let Some(ref m) = self.metrics {
                                m.p2p_messages_received.inc();
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
                                        if let Some(ref m) = self.metrics {
                                            let now_ms = std::time::SystemTime::now()
                                                .duration_since(std::time::UNIX_EPOCH)
                                                .map(|d| d.as_millis())
                                                .unwrap_or(block.timestamp);
                                            let latency_secs =
                                                now_ms.saturating_sub(block.timestamp) as f64 / 1000.0;
                                            m.block_propagation_seconds.observe(latency_secs);
                                        }
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
                                                    self.sync_state.store(1, Ordering::SeqCst);
                                                    let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                                }
                                            }
                                        } else {
                                            info!("Block #{} is ahead of our chain (height={}), requesting sync", block.index, our_height);
                                            let locator = self.chain.get_locator().await;
                                            let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            self.sync_state.store(1, Ordering::SeqCst);
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

                                    NetworkMessage::SlashingEvidence(evidence) => {
                                        match self.chain.submit_slashing_evidence(evidence.clone()).await {
                                            Ok(_) => {
                                                info!("Accepted slashing evidence from {}", peer_id);
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_good_behavior(&peer_id);
                                                }
                                                let topic = gossipsub::IdentTopic::new("blocks");
                                                let _ = self.swarm.behaviour_mut().gossipsub.publish(
                                                    topic,
                                                    NetworkMessage::SlashingEvidence(evidence).to_bytes(),
                                                );
                                            }
                                            Err(e) => {
                                                warn!("Rejected slashing evidence from {}: {}", peer_id, e);
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_invalid_block(&peer_id);
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
                                            self.sync_state.store(1, Ordering::SeqCst);
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
                                            let session_id = rand::random::<u64>();
                                            for (i, chunk_data) in chunks.into_iter().enumerate() {
                                                let chunk_msg = NetworkMessage::SnapshotChunk {
                                                    height,
                                                    index: i as u32,
                                                    total,
                                                    data: chunk_data,
                                                    session_id,
                                                };
                                                let topic = gossipsub::IdentTopic::new("blocks");
                                                let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, chunk_msg.to_bytes());
                                            }
                                            info!("Sent {} snapshot chunks for height {} (session={})", total, height, session_id);
                                        } else {
                                            let response = NetworkMessage::StateSnapshotResponse { height, state_root: "".into(), ok: false };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, response.to_bytes());
                                        }
                                    }

                                    NetworkMessage::SnapshotChunk { height, index, total, data, session_id } => {
                                        info!("Received snapshot chunk {}/{} for height {} (session={})", index + 1, total, height, session_id);

                                        // Validate session: if we have an active session for this height,
                                        // reject chunks from a different session
                                        if let Some((existing_session, _)) = self.in_progress_snapshots.get(&height) {
                                            if *existing_session != session_id {
                                                warn!("Rejecting snapshot chunk from stale session {} (current {}) for height {}", session_id, existing_session, height);
                                                continue;
                                            }
                                        }

                                        let entry = &mut self.in_progress_snapshots
                                            .entry(height)
                                            .or_insert_with(|| (session_id, vec![None; total as usize]))
                                            .1;
                                        if (index as usize) < entry.len() {
                                            entry[index as usize] = Some(data);
                                        }

                                        if entry.iter().all(|c| c.is_some()) {
                                            info!("Snapshot reassembly complete for height {} (session={})", height, session_id);
                                            let mut full_data = Vec::new();
                                            for chunk in entry.drain(..) {
                                                full_data.extend(chunk.unwrap());
                                            }
                                            self.in_progress_snapshots.remove(&height);

                                            match crate::chain::snapshot::StateSnapshot::from_bytes(&full_data) {
                                                Ok(snapshot) => {
                                                    let our_chain_id = self.chain.get_chain_id().await;
                                                    if snapshot.chain_id != our_chain_id {
                                                        warn!("Received snapshot with invalid chain_id: expected {}, got {}", our_chain_id, snapshot.chain_id);
                                                        continue;
                                                    }
                                                    let our_height = self.chain.get_height().await;
                                                    if snapshot.height < our_height.saturating_sub(100) {
                                                        warn!("Received snapshot for too old height: {}", snapshot.height);
                                                        continue;
                                                    }
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
                                            if block.index > h {
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
                                        if !crate::core::encoding::is_compatible_version(version_major, version_minor) {
                                            warn!("Peer {} has incompatible protocol v{}.{}. Banning.", peer_id, version_major, version_minor);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).ban_peer(&peer_id);
                                            continue;
                                        }
                                        info!("Handshake from {}: v{}.{}, chain={}, height={}, val_set={}, schemes={:?}",
                                            peer_id, version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes);
                                        self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).set_handshaked(&peer_id, true);
                                        let our_height = self.chain.get_height().await;
                                        if best_height > our_height {
                                            let locator = self.chain.get_locator().await;
                                            let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            self.sync_state.store(1, Ordering::SeqCst);
                                            if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes()) {
                                                warn!("Failed to request headers after handshake: {}", e);
                                                self.sync_state.store(0, Ordering::SeqCst);
                                            }
                                        }

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
                                        if !crate::core::encoding::is_compatible_version(version_major, version_minor) {
                                            warn!("Peer {} Ack has incompatible protocol v{}.{}. Banning.", peer_id, version_major, version_minor);
                                            self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); }).ban_peer(&peer_id);
                                            continue;
                                        }
                                        info!("HandshakeAck from {}: v{}.{}, chain={}, height={}, val_set={}, schemes={:?}",
                                            peer_id, version_major, version_minor, chain_id, best_height, validator_set_hash, supported_schemes);
                                        {
                                            let mut pm = self.peer_manager.lock().unwrap_or_else(|e| { tracing::error!("PeerManager lock poisoned: {}", e); std::process::exit(1); });
                                            pm.set_handshaked(&peer_id, true);
                                            pm.report_good_behavior(&peer_id);
                                        }
                                        let our_height = self.chain.get_height().await;
                                        if best_height > our_height {
                                            let locator = self.chain.get_locator().await;
                                            let req = NetworkMessage::GetHeaders { locator, limit: 500 };
                                            let topic = gossipsub::IdentTopic::new("blocks");
                                            self.sync_state.store(1, Ordering::SeqCst);
                                            if let Err(e) = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes()) {
                                                warn!("Failed to request headers after handshake ack: {}", e);
                                                self.sync_state.store(0, Ordering::SeqCst);
                                            }
                                        }
                                    }

                                    NetworkMessage::Prevote { epoch, checkpoint_height, checkpoint_hash, voter_id, sig_bls } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_vote_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded vote rate limit or lock error. Ignoring Prevote.", peer_id);
                                            continue;
                                        }
                                        info!("Prevote from {}: epoch={}, height={}, hash={}..., voter={}",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())], voter_id);

                                        let voter_addr = match crate::core::address::Address::from_hex(&voter_id) {
                                            Ok(addr) => addr,
                                            Err(e) => {
                                                warn!("Invalid voter_id in Prevote: {}", e);
                                                continue;
                                            }
                                        };

                                        let prevote = Prevote {
                                            epoch,
                                            checkpoint_height,
                                            checkpoint_hash,
                                            voter_id: voter_addr,
                                            sig_bls,
                                        };
                                        match self.chain.handle_prevote(prevote).await {
                                            Ok(_) => {
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_good_behavior(&peer_id);
                                                }
                                            }
                                            Err(e) => {
                                                warn!("Prevote from {} rejected: {}", peer_id, e);
                                            }
                                        }
                                    }

                                    NetworkMessage::Precommit { epoch, checkpoint_height, checkpoint_hash, voter_id, sig_bls } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_vote_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded vote rate limit or lock error. Ignoring Precommit.", peer_id);
                                            continue;
                                        }
                                        info!("Precommit from {}: epoch={}, height={}, hash={}..., voter={}",
                                            peer_id, epoch, checkpoint_height, &checkpoint_hash[..16.min(checkpoint_hash.len())], voter_id);

                                        let voter_addr = match crate::core::address::Address::from_hex(&voter_id) {
                                            Ok(addr) => addr,
                                            Err(e) => {
                                                warn!("Invalid voter_id in Precommit: {}", e);
                                                continue;
                                            }
                                        };

                                        let precommit = Precommit {
                                            epoch,
                                            checkpoint_height,
                                            checkpoint_hash,
                                            voter_id: voter_addr,
                                            sig_bls,
                                        };
                                        match self.chain.handle_precommit(precommit).await {
                                            Ok(Some(cert)) => {
                                                info!("FinalityCert produced from precommit: epoch={}, height={}", cert.epoch, cert.checkpoint_height);
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_good_behavior(&peer_id);
                                                }
                                                let topic = gossipsub::IdentTopic::new("blocks");
                                                let _ = self.swarm.behaviour_mut().gossipsub.publish(
                                                    topic,
                                                    NetworkMessage::FinalityCert {
                                                        epoch: cert.epoch,
                                                        checkpoint_height: cert.checkpoint_height,
                                                        checkpoint_hash: cert.checkpoint_hash,
                                                        agg_sig_bls: cert.agg_sig_bls,
                                                        bitmap: cert.bitmap,
                                                        set_hash: cert.set_hash,
                                                    }
                                                    .to_bytes(),
                                                );
                                            }
                                            Ok(None) => {
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_good_behavior(&peer_id);
                                                }
                                            }
                                            Err(e) => {
                                                warn!("Precommit from {} rejected: {}", peer_id, e);
                                            }
                                        }
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
                                                if e.contains("Missing verified QC blob") {
                                                    let topic = gossipsub::IdentTopic::new("blocks");
                                                    let req = NetworkMessage::GetQcBlob {
                                                        epoch,
                                                        checkpoint_height,
                                                    };
                                                    let _ = self.swarm.behaviour_mut().gossipsub.publish(topic, req.to_bytes());
                                                } else if let Ok(mut pm) = self.peer_manager.lock() {
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

                                    NetworkMessage::QcBlobResponse { epoch, checkpoint_height, found, blob_data, .. } => {
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
                                            match serde_json::from_slice::<crate::consensus::qc::QcBlob>(&blob_data) {
                                                Ok(blob) => {
                                                    if blob.epoch != epoch || blob.checkpoint_height != checkpoint_height {
                                                        warn!(
                                                            "QcBlobResponse metadata mismatch from {}: expected epoch={}, height={}, got epoch={}, height={}",
                                                            peer_id,
                                                            epoch,
                                                            checkpoint_height,
                                                            blob.epoch,
                                                            blob.checkpoint_height
                                                        );
                                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                                            pm.report_bad_behavior(&peer_id);
                                                        }
                                                        continue;
                                                    }

                                                    match self.chain.import_qc_blob(blob).await {
                                                        Ok(_) => {
                                                            if let Ok(mut pm) = self.peer_manager.lock() {
                                                                pm.report_good_behavior(&peer_id);
                                                            }
                                                        }
                                                        Err(e) => {
                                                            warn!("Failed to import QcBlob from {}: {}", peer_id, e);
                                                            if let Ok(mut pm) = self.peer_manager.lock() {
                                                                pm.report_bad_behavior(&peer_id);
                                                            }
                                                        }
                                                    }
                                                }
                                                Err(e) => {
                                                    warn!("Failed to parse QcBlobResponse from {}: {}", peer_id, e);
                                                    if let Ok(mut pm) = self.peer_manager.lock() {
                                                        pm.report_bad_behavior(&peer_id);
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    NetworkMessage::QcFaultProof { proof_data } => {
                                        let rate_limit_ok = self.peer_manager.lock()
                                            .map(|mut pm| pm.check_blob_rate_limit(&peer_id))
                                            .unwrap_or(false);
                                        if !rate_limit_ok {
                                            warn!("Peer {} exceeded blob rate limit or lock error. Ignoring QcFaultProof.", peer_id);
                                            continue;
                                        }

                                        match serde_json::from_slice::<crate::consensus::qc::QcFaultProof>(&proof_data) {
                                            Ok(proof) => {
                                                match self.chain.handle_qc_fault_proof(proof).await {
                                                    Ok(_) => {
                                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                                            pm.report_good_behavior(&peer_id);
                                                        }
                                                    }
                                                    Err(e) => {
                                                        warn!("Failed to apply QcFaultProof from {}: {}", peer_id, e);
                                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                                            pm.report_bad_behavior(&peer_id);
                                                        }
                                                    }
                                                }
                                            }
                                            Err(e) => {
                                                warn!("Failed to parse QcFaultProof from {}: {}", peer_id, e);
                                                if let Ok(mut pm) = self.peer_manager.lock() {
                                                    pm.report_bad_behavior(&peer_id);
                                                }
                                            }
                                        }
                                    }
                                    NetworkMessage::DomainCommitment(commitment) => {
                                        warn!(
                                            "Ignoring raw DomainCommitment from {} for domain {}; verified finality proof is required",
                                            peer_id, commitment.domain_id
                                        );
                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_bad_behavior(&peer_id);
                                        }
                                    }
                                    NetworkMessage::VerifiedDomainCommitment(payload) => {
                                        info!(
                                            "Received VerifiedDomainCommitment from {} for domain {}",
                                            peer_id, payload.commitment.domain_id
                                        );
                                        let payload_clone = payload.clone();
                                        let chain = self.chain.clone();
                                        let swarm_cmd_tx = self.command_tx.clone();
                                        tokio::spawn(async move {
                                            match chain.submit_verified_domain_commitment(payload_clone.clone()).await {
                                                Ok(_) => {
                                                    let msg = NetworkMessage::VerifiedDomainCommitment(payload_clone);
                                                    let _ = swarm_cmd_tx.send(NodeCommand::Broadcast("blocks".into(), msg)).await;
                                                }
                                                Err(e) => {
                                                    warn!("Failed to process VerifiedDomainCommitment from {}: {}", peer_id, e);
                                                }
                                            }
                                        });
                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_good_behavior(&peer_id);
                                        }
                                    }
                                    NetworkMessage::CrossDomainMessage(msg_obj) => {
                                        info!("Received CrossDomainMessage from {} for bridge", peer_id);
                                        let msg_clone = msg_obj.clone();
                                        let chain = self.chain.clone();
                                        let swarm_cmd_tx = self.command_tx.clone();
                                        tokio::spawn(async move {
                                            match chain.submit_cross_domain_message(msg_clone.clone()).await {
                                                Ok(_) => {
                                                    let msg = NetworkMessage::CrossDomainMessage(msg_clone);
                                                    let _ = swarm_cmd_tx.send(NodeCommand::Broadcast("blocks".into(), msg)).await;
                                                }
                                                Err(e) => {
                                                    warn!("Failed to process CrossDomainMessage from {}: {}", peer_id, e);
                                                }
                                            }
                                        });
                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_good_behavior(&peer_id);
                                        }
                                    }
                                    NetworkMessage::GlobalHeader(header) => {
                                        info!(
                                            "GlobalHeader from {}: height={}, hash={}...",
                                            peer_id,
                                            header.global_height,
                                            &header.calculate_hash()[..16]
                                        );
                                        if let Ok(mut pm) = self.peer_manager.lock() {
                                            pm.report_good_behavior(&peer_id);
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
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Identify(identify::Event::Received { info, .. })) => {
                            info!("Received identity from {:?}", info.public_key.to_peer_id());
                            for addr in info.listen_addrs {
                                self.swarm.behaviour_mut().kad.add_address(&info.public_key.to_peer_id(), addr);
                            }
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Kad(KademliaEvent::RoutingUpdated { peer, .. })) => {
                            info!("Kademlia: Routing updated for peer {}", peer);
                        }
                        SwarmEvent::Behaviour(BudlumBehaviourEvent::Sync(event)) => {
                            match event {
                                request_response::Event::Message { peer, message, .. } => {
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
                                                            if let Some(last) = headers.last() {
                                                                let to = last.index;
                                                                let req = NetworkMessage::GetBlocksRange { from, to };
                                                                self.sync_state.store(1, Ordering::SeqCst);
                                                                let _ = self.swarm.behaviour_mut().sync.send_request(&peer, req.to_bytes());
                                                            }
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
                                                        self.sync_state.store(0, Ordering::SeqCst);
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
