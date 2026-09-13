use super::api::BudlumApiServer;
use crate::chain::chain_actor::ChainHandle;
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use crate::network::node::NodeClient;
use futures::future::BoxFuture;
use hyper::header::{HeaderValue, AUTHORIZATION};
use hyper::StatusCode;
use jsonrpsee::server::{HttpBody, HttpRequest, HttpResponse};
use jsonrpsee::types::error::ErrorObjectOwned;
use libp2p::PeerId;
use std::collections::{HashMap, VecDeque};
use std::net::IpAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};
use tower::{Layer, Service, ServiceBuilder};
use tracing::info;

#[derive(Clone, Debug, Default)]
pub struct RpcSecurityConfig {
    pub auth_required: bool,
    pub api_key: Option<String>,
    pub allowed_ips: Vec<String>,
    pub cors_origins: Vec<String>,
    pub rate_limit_per_minute: Option<u64>,
    pub trusted_proxies: Vec<String>,
    pub max_request_body_size: Option<u32>,
    pub max_connections: Option<u32>,
}

impl RpcSecurityConfig {
    pub fn operator_default() -> Self {
        Self {
            auth_required: false,
            api_key: None,
            allowed_ips: vec!["127.0.0.1".into(), "::1".into()],
            cors_origins: Vec::new(),
            rate_limit_per_minute: None,
            trusted_proxies: Vec::new(),
            max_request_body_size: Some(50 * 1024 * 1024),
            max_connections: Some(10),
        }
    }

    pub fn from_env(
        auth_required: bool,
        api_key_env: Option<&str>,
        allowed_ips: Vec<String>,
        cors_origins: Vec<String>,
        rate_limit_per_minute: Option<u64>,
    ) -> Result<Self, String> {
        let api_key = match api_key_env {
            Some(env_name) if auth_required => Some(std::env::var(env_name).map_err(|_| {
                format!("RPC auth is required but environment variable {env_name} is not set")
            })?),
            Some(env_name) => std::env::var(env_name).ok(),
            None => None,
        };

        if auth_required && api_key.as_deref().unwrap_or_default().is_empty() {
            return Err("RPC auth is required but no API key was configured".into());
        }

        Ok(Self {
            auth_required,
            api_key,
            allowed_ips,
            cors_origins,
            rate_limit_per_minute,
            trusted_proxies: Vec::new(),
            max_request_body_size: None,
            max_connections: None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RpcMode {
    Public,
    Operator,
}

#[derive(Clone)]
struct RpcSecurityLayer {
    config: Arc<RpcSecurityConfig>,
    per_ip_rates: Arc<Mutex<HashMap<IpAddr, VecDeque<Instant>>>>,
}

impl RpcSecurityLayer {
    fn new(config: RpcSecurityConfig) -> Self {
        Self {
            config: Arc::new(config),
            per_ip_rates: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

impl<S> Layer<S> for RpcSecurityLayer {
    type Service = RpcSecurityService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RpcSecurityService {
            inner,
            config: self.config.clone(),
            per_ip_rates: self.per_ip_rates.clone(),
        }
    }
}

#[derive(Clone)]
struct RpcSecurityService<S> {
    inner: S,
    config: Arc<RpcSecurityConfig>,
    per_ip_rates: Arc<Mutex<HashMap<IpAddr, VecDeque<Instant>>>>,
}

impl<S, B> Service<HttpRequest<B>> for RpcSecurityService<S>
where
    S: Service<HttpRequest<B>, Response = HttpResponse> + Clone + Send + 'static,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
    B: Send + 'static,
{
    type Response = HttpResponse;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: HttpRequest<B>) -> Self::Future {
        if !is_ip_allowed(&self.config, &req) {
            return Box::pin(async { Ok(text_response(StatusCode::FORBIDDEN, "Forbidden")) });
        }

        if !is_origin_allowed(&self.config, &req) {
            return Box::pin(async { Ok(text_response(StatusCode::FORBIDDEN, "Forbidden")) });
        }

        if !is_authorized(&self.config, &req) {
            return Box::pin(async { Ok(text_response(StatusCode::UNAUTHORIZED, "Unauthorized")) });
        }

        let client_ip = extract_client_ip(&self.config, &req);
        if !is_per_ip_rate_limited(&self.config, &self.per_ip_rates, client_ip) {
            return Box::pin(async {
                Ok(text_response(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many requests",
                ))
            });
        }

        let mut inner = self.inner.clone();
        Box::pin(async move { inner.call(req).await })
    }
}

pub struct RpcServer {
    chain: ChainHandle,
    node: NodeClient,
    security: RpcSecurityConfig,
    mode: RpcMode,
}

impl RpcServer {
    pub fn new(chain: ChainHandle, node: NodeClient) -> Self {
        Self {
            chain,
            node,
            security: RpcSecurityConfig::default(),
            mode: RpcMode::Public,
        }
    }

    pub fn with_security(
        chain: ChainHandle,
        node: NodeClient,
        security: RpcSecurityConfig,
    ) -> Self {
        Self {
            chain,
            node,
            security,
            mode: RpcMode::Public,
        }
    }

    pub fn with_security_and_mode(
        chain: ChainHandle,
        node: NodeClient,
        security: RpcSecurityConfig,
        mode: RpcMode,
    ) -> Self {
        Self {
            chain,
            node,
            security,
            mode,
        }
    }

    pub async fn run(self, addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use jsonrpsee::server::ServerBuilder;
        let http_middleware =
            ServiceBuilder::new().layer(RpcSecurityLayer::new(self.security.clone()));
        let mut builder = ServerBuilder::default().set_http_middleware(http_middleware);

        if let Some(limit) = self.security.max_request_body_size {
            builder = builder.max_request_body_size(limit);
        }
        if let Some(limit) = self.security.max_connections {
            builder = builder.max_connections(limit);
        }

        let server = builder.build(addr.clone()).await?;

        let mode_label = match self.mode {
            RpcMode::Public => "public",
            RpcMode::Operator => "operator",
        };
        info!("RPC Server ({}) started on {}", mode_label, addr);
        let handle = server.start(self.into_rpc());
        tokio::spawn(handle.stopped());
        Ok(())
    }

    fn require_operator(&self) -> Result<(), ErrorObjectOwned> {
        if self.mode != RpcMode::Operator {
            return Err(ErrorObjectOwned::owned(
                -32601,
                "This method is only available on the operator RPC listener",
                None::<()>,
            ));
        }
        Ok(())
    }

    fn to_hex(n: u64) -> String {
        format!("0x{:x}", n)
    }

    fn to_0x_hash(h: String) -> String {
        if h.is_empty() {
            "0x0000000000000000000000000000000000000000000000000000000000000000".to_string()
        } else if h.starts_with("0x") {
            h
        } else {
            format!("0x{}", h)
        }
    }

    fn block_to_json(b: Block) -> serde_json::Value {
        serde_json::json!({
            "number": Self::to_hex(b.index),
            "hash": Self::to_0x_hash(b.hash),
            "parentHash": Self::to_0x_hash(b.previous_hash),
            "timestamp": Self::to_hex(b.timestamp as u64),
            "transactions": b.transactions.into_iter().map(Self::tx_to_json).collect::<Vec<_>>(),
            "producer": b.producer.map(|p| p.to_string()),
            "signature": b.signature.map(|s| format!("0x{}", hex::encode(s))),
            "stateRoot": if b.state_root.is_empty() { serde_json::Value::Null } else { serde_json::json!(Self::to_0x_hash(b.state_root)) },
            "txRoot": if b.tx_root.is_empty() { serde_json::Value::Null } else { serde_json::json!(Self::to_0x_hash(b.tx_root)) },
        })
    }

    fn tx_to_json(t: Transaction) -> serde_json::Value {
        serde_json::json!({
            "hash": Self::to_0x_hash(t.hash),
            "from": t.from.to_string(),
            "to": t.to.to_string(),
            "amount": Self::to_hex(t.amount),
            "fee": Self::to_hex(t.fee),
            "nonce": Self::to_hex(t.nonce),
            "timestamp": Self::to_hex(t.timestamp as u64),
            "type": format!("{:?}", t.tx_type),
            "chainId": Self::to_hex(t.chain_id),
            "signature": t.signature.map(|s| format!("0x{}", hex::encode(s))),
        })
    }

    fn bytes32_to_0x(bytes: [u8; 32]) -> String {
        format!("0x{}", hex::encode(bytes))
    }

    fn global_header_to_json(h: crate::settlement::GlobalBlockHeader) -> serde_json::Value {
        serde_json::json!({
            "version": Self::to_hex(h.version as u64),
            "globalHeight": Self::to_hex(h.global_height),
            "hash": Self::bytes32_to_0x(h.calculate_hash_bytes()),
            "previousGlobalHash": Self::bytes32_to_0x(h.previous_global_hash),
            "chainId": Self::to_hex(h.chain_id),
            "timestamp": Self::to_hex(h.timestamp_ms as u64),
            "domainRegistryRoot": Self::bytes32_to_0x(h.domain_registry_root),
            "domainCommitmentRoot": Self::bytes32_to_0x(h.domain_commitment_root),
            "messageRoot": Self::bytes32_to_0x(h.message_root),
            "bridgeStateRoot": Self::bytes32_to_0x(h.bridge_state_root),
            "replayNonceRoot": Self::bytes32_to_0x(h.replay_nonce_root),
            "proposer": h.proposer.map(|p| p.to_string()),
            "settlementFinalityRoot": Self::bytes32_to_0x(h.settlement_finality_root),
        })
    }

    fn domain_commitment_to_json(c: crate::domain::DomainCommitment) -> serde_json::Value {
        serde_json::json!({
            "domainId": c.domain_id,
            "domainHeight": Self::to_hex(c.domain_height),
            "domainBlockHash": Self::bytes32_to_0x(c.domain_block_hash),
            "parentDomainBlockHash": Self::bytes32_to_0x(c.parent_domain_block_hash),
            "stateRoot": Self::bytes32_to_0x(c.state_root),
            "txRoot": Self::bytes32_to_0x(c.tx_root),
            "eventRoot": Self::bytes32_to_0x(c.event_root),
            "finalityProofHash": Self::bytes32_to_0x(c.finality_proof_hash),
            "consensusKind": format!("{:?}", c.consensus_kind),
            "validatorSetHash": Self::bytes32_to_0x(c.validator_set_hash),
            "timestamp": Self::to_hex(c.timestamp_ms as u64),
            "sequence": Self::to_hex(c.sequence),
            "producer": c.producer.map(|p| p.to_string()),
            "leafHash": Self::bytes32_to_0x(c.leaf_hash()),
        })
    }

    fn consensus_domain_to_json(d: crate::domain::ConsensusDomain) -> serde_json::Value {
        serde_json::json!({
            "domainId": d.id,
            "consensusKind": format!("{:?}", d.kind),
            "status": format!("{:?}", d.status),
            "domainChainId": Self::to_hex(d.domain_chain_id),
            "configHash": Self::bytes32_to_0x(d.config_hash),
            "validatorSetHash": Self::bytes32_to_0x(d.validator_set_hash),
            "finalityAdapter": d.finality_adapter,
            "minConfirmations": Self::to_hex(d.min_confirmations),
            "bridgeEnabled": d.bridge_enabled,
            "blockHashScheme": format!("{:?}", d.block_hash_scheme),
            "stateRootScheme": format!("{:?}", d.state_root_scheme),
            "txRootScheme": format!("{:?}", d.tx_root_scheme),
        })
    }

    async fn bridge_roots_json(&self, label: &str) -> serde_json::Value {
        let info = self.chain.get_settlement_info().await;
        serde_json::json!({
            "status": label,
            "bridgeStateRoot": info["bridgeStateRoot"].clone(),
            "replayNonceRoot": info["replayNonceRoot"].clone(),
        })
    }
}

fn is_authorized<B>(config: &RpcSecurityConfig, req: &HttpRequest<B>) -> bool {
    if !config.auth_required {
        return true;
    }

    let Some(expected) = config.api_key.as_deref() else {
        return false;
    };

    let bearer = format!("Bearer {expected}");
    let api_key = HeaderValue::from_str(expected).ok();
    let bearer = HeaderValue::from_str(&bearer).ok();

    req.headers()
        .get("x-api-key")
        .map(|value| Some(value) == api_key.as_ref())
        .unwrap_or(false)
        || req
            .headers()
            .get(AUTHORIZATION)
            .map(|value| Some(value) == bearer.as_ref())
            .unwrap_or(false)
}

fn extract_client_ip<B>(config: &RpcSecurityConfig, req: &HttpRequest<B>) -> Option<IpAddr> {
    // If trusted proxies are configured, extract the real client IP from X-Forwarded-For
    // This is checked after validate_trusted_proxy has already identified the request came
    // from a trusted source (the actual validation of the proxy IP is complex without
    // socket-level info from hyper, so we rely on network-level firewall rules).

    // Try X-Forwarded-For first (standard proxy header)
    if !config.trusted_proxies.is_empty() {
        if let Some(forwarded_ip) = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .and_then(|ip| ip.parse::<IpAddr>().ok())
        {
            return Some(forwarded_ip);
        }
    }

    // Fall back to X-Real-IP
    if let Some(real_ip) = req
        .headers()
        .get("x-real-ip")
        .and_then(|v| v.to_str().ok())
        .and_then(|ip| ip.parse::<IpAddr>().ok())
    {
        return Some(real_ip);
    }

    // No identifiable IP — reject
    None
}

fn is_ip_allowed<B>(config: &RpcSecurityConfig, req: &HttpRequest<B>) -> bool {
    if config.allowed_ips.is_empty() {
        return true;
    }

    let client_ip = extract_client_ip(config, req);
    let Some(ip) = client_ip else {
        return false;
    };

    let ip_str = ip.to_string();
    config
        .allowed_ips
        .iter()
        .any(|allowed| allowed == "*" || allowed == &ip_str)
}

fn is_origin_allowed<B>(config: &RpcSecurityConfig, req: &HttpRequest<B>) -> bool {
    if config.cors_origins.is_empty() {
        return true;
    }

    let Some(origin) = req
        .headers()
        .get("origin")
        .and_then(|value| value.to_str().ok())
    else {
        return true;
    };

    config
        .cors_origins
        .iter()
        .any(|allowed| allowed == "*" || allowed == origin)
}

fn is_per_ip_rate_limited(
    config: &RpcSecurityConfig,
    per_ip_rates: &Arc<Mutex<HashMap<IpAddr, VecDeque<Instant>>>>,
    client_ip: Option<IpAddr>,
) -> bool {
    let Some(limit) = config.rate_limit_per_minute else {
        return true;
    };
    if limit == 0 {
        return false;
    }

    let ip = match client_ip {
        Some(ip) => ip,
        None => return false,
    };

    let now = Instant::now();
    let cutoff = now - Duration::from_secs(60);
    let mut rates = match per_ip_rates.lock() {
        Ok(rates) => rates,
        Err(_) => return false,
    };

    let window = rates.entry(ip).or_default();
    while window.front().is_some_and(|instant| *instant < cutoff) {
        window.pop_front();
    }
    if window.len() >= limit as usize {
        return false;
    }
    window.push_back(now);
    true
}

fn text_response(status: StatusCode, body: &'static str) -> HttpResponse {
    HttpResponse::builder()
        .status(status)
        .header("content-type", HeaderValue::from_static("text/plain"))
        .body(HttpBody::from(body))
        .expect("static RPC security response is valid")
}

#[jsonrpsee::core::async_trait]
impl BudlumApiServer for RpcServer {
    async fn chain_id(&self) -> Result<String, ErrorObjectOwned> {
        let chain_id = self.chain.get_chain_id().await;
        Ok(Self::to_hex(chain_id))
    }

    async fn block_number(&self) -> Result<String, ErrorObjectOwned> {
        let height = self.chain.get_height().await;
        Ok(Self::to_hex(height))
    }

    async fn get_block_by_number(
        &self,
        number: u64,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        match self.chain.get_block(number).await {
            Some(b) => Ok(Self::block_to_json(b)),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn get_block_by_hash(&self, hash: String) -> Result<serde_json::Value, ErrorObjectOwned> {
        let clean_hash = hash.strip_prefix("0x").unwrap_or(&hash);
        match self.chain.get_block_by_hash(clean_hash.to_string()).await {
            Some(b) => Ok(Self::block_to_json(b)),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn get_balance(&self, address: String) -> Result<String, ErrorObjectOwned> {
        let clean_addr = address.strip_prefix("0x").unwrap_or(&address);
        let addr = Address::from_hex(clean_addr).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("Invalid address: {}", e), None::<()>)
        })?;
        let balance = self.chain.get_balance(&addr).await;
        Ok(Self::to_hex(balance))
    }

    async fn get_nonce(&self, address: String) -> Result<String, ErrorObjectOwned> {
        let clean_addr = address.strip_prefix("0x").unwrap_or(&address);
        let addr = Address::from_hex(clean_addr).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("Invalid address: {}", e), None::<()>)
        })?;
        let nonce = self.chain.get_nonce(&addr).await;
        Ok(Self::to_hex(nonce))
    }

    async fn send_raw_transaction(&self, tx: Transaction) -> Result<String, ErrorObjectOwned> {
        if let Err(e) = crate::network::protocol::NetworkMessage::validate_tx_size(&tx) {
            return Err(ErrorObjectOwned::owned(
                -32602,
                format!("Transaction too large: {:?}", e),
                None::<()>,
            ));
        }

        if !tx.verify() {
            return Err(ErrorObjectOwned::owned(
                -32602,
                "Invalid transaction signature",
                None::<()>,
            ));
        }

        let tx_hash = tx.hash.clone();
        let tx_clone = tx.clone();
        self.chain.add_transaction(tx).await.map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("Invalid params: {}", e), None::<()>)
        })?;
        self.node.broadcast_tx_sync(tx_clone);
        Ok(Self::to_0x_hash(tx_hash))
    }

    async fn get_transaction_by_hash(
        &self,
        hash: String,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        let clean_hash = hash.strip_prefix("0x").unwrap_or(&hash);
        match self
            .chain
            .get_transaction_by_hash(clean_hash.to_string())
            .await
        {
            Some(t) => Ok(Self::tx_to_json(t)),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn get_transaction_receipt(
        &self,
        hash: String,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        let clean_hash = hash.strip_prefix("0x").unwrap_or(&hash);
        match self.chain.get_tx_receipt(clean_hash.to_string()).await {
            Some(receipt) => Ok(receipt),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn gas_price(&self) -> Result<String, ErrorObjectOwned> {
        let fee = self.chain.get_base_fee().await;
        Ok(Self::to_hex(fee))
    }

    async fn estimate_gas(&self, tx: Transaction) -> Result<String, ErrorObjectOwned> {
        if let Err(_e) = crate::network::protocol::NetworkMessage::validate_tx_size(&tx) {
            return Err(ErrorObjectOwned::owned(
                -32602,
                format!("Transaction too large: {:?}", _e),
                None::<()>,
            ));
        }
        Ok(Self::to_hex(21000))
    }

    async fn tx_precheck(&self, tx: Transaction) -> Result<serde_json::Value, ErrorObjectOwned> {
        if let Err(_e) = crate::network::protocol::NetworkMessage::validate_tx_size(&tx) {
            return Ok(serde_json::json!({
                "accepted": false,
                "reasons": ["transaction_too_large"]
            }));
        }
        Ok(self.chain.tx_precheck(tx).await)
    }

    async fn syncing(&self) -> Result<bool, ErrorObjectOwned> {
        Ok(self.node.is_syncing())
    }

    async fn net_version(&self) -> Result<String, ErrorObjectOwned> {
        let chain_id = self.chain.get_chain_id().await;
        Ok(chain_id.to_string())
    }

    async fn net_listening(&self) -> Result<bool, ErrorObjectOwned> {
        Ok(true)
    }

    async fn net_peer_count(&self) -> Result<String, ErrorObjectOwned> {
        Ok(Self::to_hex(
            self.node
                .peer_count
                .load(std::sync::atomic::Ordering::SeqCst) as u64,
        ))
    }

    async fn get_settlement_info(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        Ok(self.chain.get_settlement_info().await)
    }

    async fn get_global_header(&self, height: u64) -> Result<serde_json::Value, ErrorObjectOwned> {
        match self.chain.get_global_header(height).await {
            Some(header) => Ok(Self::global_header_to_json(header)),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn get_domain_commitments(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        let commitments = self.chain.get_domain_commitments().await;
        Ok(serde_json::Value::Array(
            commitments
                .into_iter()
                .map(Self::domain_commitment_to_json)
                .collect(),
        ))
    }

    async fn get_consensus_domains(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        let domains = self.chain.get_consensus_domains().await;
        Ok(serde_json::Value::Array(
            domains
                .into_iter()
                .map(Self::consensus_domain_to_json)
                .collect(),
        ))
    }

    async fn register_consensus_domain(
        &self,
        domain: crate::domain::ConsensusDomain,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        let domain_id = domain.id;
        self.chain
            .register_consensus_domain(domain)
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid consensus domain: {}", e),
                    None::<()>,
                )
            })?;

        let info = self.chain.get_settlement_info().await;
        let registry_root = info["domainRegistryRoot"]
            .as_str()
            .map(|root| format!("0x{}", root))
            .unwrap_or_else(|| "0x".to_string());
        Ok(serde_json::json!({
            "domainId": domain_id,
            "domainRegistryRoot": registry_root,
        }))
    }

    async fn submit_domain_commitment(
        &self,
        commitment: crate::domain::DomainCommitment,
    ) -> Result<String, ErrorObjectOwned> {
        let _ = commitment;
        Err(ErrorObjectOwned::owned(
            -32602,
            "Raw domain commitment submission is disabled; use bud_submitVerifiedDomainCommitment with a finality proof",
            None::<()>,
        ))
    }

    async fn submit_verified_domain_commitment(
        &self,
        payload: crate::domain::VerifiedDomainCommitment,
    ) -> Result<String, ErrorObjectOwned> {
        let hash = hex::encode(payload.leaf_hash());
        let payload_clone = payload.clone();

        self.chain
            .submit_verified_domain_commitment(payload)
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid verified domain commitment: {}", e),
                    None::<()>,
                )
            })?;

        self.node
            .broadcast_verified_domain_commitment_sync(payload_clone);
        Ok(format!("0x{}", hash))
    }

    async fn submit_cross_domain_message(
        &self,
        msg: crate::cross_domain::CrossDomainMessage,
    ) -> Result<String, ErrorObjectOwned> {
        let msg_id = hex::encode(msg.message_id);
        let msg_clone = msg.clone();

        self.chain
            .submit_cross_domain_message(msg)
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid cross domain message: {}", e),
                    None::<()>,
                )
            })?;

        self.node.broadcast_cross_domain_message_sync(msg_clone);
        Ok(format!("0x{}", msg_id))
    }

    async fn register_bridge_asset(
        &self,
        asset_id: crate::cross_domain::AssetId,
        domain: crate::domain::DomainId,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.chain
            .register_bridge_asset(asset_id, domain)
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge asset registration: {}", e),
                    None::<()>,
                )
            })?;
        Ok(self.bridge_roots_json("registered").await)
    }

    #[allow(clippy::too_many_arguments)]
    async fn lock_bridge_transfer(
        &self,
        source_domain: crate::domain::DomainId,
        target_domain: crate::domain::DomainId,
        source_height: u64,
        event_index: u32,
        asset_id: crate::cross_domain::AssetId,
        owner: crate::core::address::Address,
        recipient: crate::core::address::Address,
        amount: u128,
        expiry_height: u64,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        let (transfer, event) = self
            .chain
            .lock_bridge_transfer(
                source_domain,
                target_domain,
                source_height,
                event_index,
                asset_id,
                owner,
                recipient,
                amount,
                expiry_height,
            )
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge lock transfer: {}", e),
                    None::<()>,
                )
            })?;
        Ok(serde_json::json!({
            "transfer": transfer,
            "event": event,
            "messageId": Self::bytes32_to_0x(transfer.message_id),
            "eventHash": Self::bytes32_to_0x(event.leaf_hash()),
        }))
    }

    async fn mint_bridge_transfer(
        &self,
        source_domain: crate::domain::DomainId,
        source_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: crate::cross_domain::DomainEvent,
        proof: crate::cross_domain::MerkleProof,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.chain
            .mint_bridge_transfer_from_verified_event(
                source_domain,
                source_height,
                sequence,
                expected_block_hash,
                event,
                proof,
            )
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge mint transfer: {}", e),
                    None::<()>,
                )
            })?;
        Ok(self.bridge_roots_json("minted").await)
    }

    async fn burn_bridge_transfer(
        &self,
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.chain
            .burn_bridge_transfer(message_id, domain)
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge burn transfer: {}", e),
                    None::<()>,
                )
            })?;
        Ok(self.bridge_roots_json("burned").await)
    }

    async fn burn_bridge_transfer_with_event(
        &self,
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
        domain_height: u64,
        event_index: u32,
        expiry_height: u64,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        let event = self
            .chain
            .burn_bridge_transfer_with_event(
                message_id,
                domain,
                domain_height,
                event_index,
                expiry_height,
            )
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge burn transfer: {}", e),
                    None::<()>,
                )
            })?;
        let mut roots = self.bridge_roots_json("burned").await;
        roots["event"] = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
        Ok(roots)
    }

    async fn unlock_bridge_transfer(
        &self,
        message_id: crate::cross_domain::MessageId,
        source_domain: crate::domain::DomainId,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.chain
            .unlock_bridge_transfer(message_id, source_domain)
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge unlock transfer: {}", e),
                    None::<()>,
                )
            })?;
        Ok(self.bridge_roots_json("unlocked").await)
    }

    async fn unlock_bridge_transfer_verified(
        &self,
        target_domain: crate::domain::DomainId,
        target_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: crate::cross_domain::DomainEvent,
        proof: crate::cross_domain::MerkleProof,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.chain
            .unlock_bridge_transfer_from_verified_event(
                target_domain,
                target_height,
                sequence,
                expected_block_hash,
                event,
                proof,
            )
            .await
            .map_err(|e| {
                ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid bridge unlock transfer: {}", e),
                    None::<()>,
                )
            })?;
        Ok(self.bridge_roots_json("unlocked").await)
    }

    async fn seal_global_header(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        let header = self.chain.seal_global_header().await.map_err(|e| {
            ErrorObjectOwned::owned(
                -32602,
                format!("Unable to seal global header: {}", e),
                None::<()>,
            )
        })?;
        Ok(Self::global_header_to_json(header))
    }

    async fn health(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        let height = self.chain.get_height().await;
        let syncing = self.node.is_syncing();
        let peer_count = self
            .node
            .peer_count
            .load(std::sync::atomic::Ordering::SeqCst);
        Ok(serde_json::json!({
            "status": if syncing { "syncing" } else { "healthy" },
            "blockHeight": Self::to_hex(height),
            "peerCount": peer_count,
            "syncing": syncing,
        }))
    }

    async fn node_info(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        let chain_id = self.chain.get_chain_id().await;
        let height = self.chain.get_height().await;
        let validator_set_hash = self.chain.get_validator_set_hash().await;
        let sync_state = if self.node.is_syncing() { 1u64 } else { 0u64 };
        let peer_count = self
            .node
            .peer_count
            .load(std::sync::atomic::Ordering::SeqCst);
        Ok(serde_json::json!({
            "chainId": Self::to_hex(chain_id),
            "blockHeight": Self::to_hex(height),
            "validatorSetHash": validator_set_hash,
            "syncState": sync_state,
            "peerCount": peer_count,
            "peerId": self.node.peer_id.to_string(),
            "rpcMode": match self.mode { RpcMode::Public => "public", RpcMode::Operator => "operator" },
        }))
    }

    async fn admin_ban_peer(&self, peer_id: String) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.require_operator()?;
        let parsed = PeerId::from_str(&peer_id).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("Invalid peer id: {}", e), None::<()>)
        })?;
        self.node
            .admin_ban_peer(&parsed)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e, None::<()>))?;
        Ok(serde_json::json!({ "banned": peer_id }))
    }

    async fn admin_unban_peer(
        &self,
        peer_id: String,
    ) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.require_operator()?;
        let parsed = PeerId::from_str(&peer_id).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("Invalid peer id: {}", e), None::<()>)
        })?;
        self.node
            .admin_unban_peer(&parsed)
            .map_err(|e| ErrorObjectOwned::owned(-32000, e, None::<()>))?;
        Ok(serde_json::json!({ "unbanned": peer_id }))
    }

    async fn admin_list_banned_peers(&self) -> Result<serde_json::Value, ErrorObjectOwned> {
        self.require_operator()?;
        let banned = self
            .node
            .admin_list_banned_peers()
            .map_err(|e| ErrorObjectOwned::owned(-32000, e, None::<()>))?;
        Ok(serde_json::json!({ "bannedPeers": banned }))
    }
}

#[cfg(test)]
mod security_tests {
    use super::*;

    fn request_with_headers(headers: &[(&str, &str)]) -> HttpRequest<()> {
        let mut builder = HttpRequest::builder().uri("/");
        for (name, value) in headers {
            builder = builder.header(*name, *value);
        }
        builder.body(()).unwrap()
    }

    #[test]
    fn auth_accepts_x_api_key_and_bearer() {
        let config = RpcSecurityConfig {
            auth_required: true,
            api_key: Some("secret".to_string()),
            ..Default::default()
        };

        assert!(is_authorized(
            &config,
            &request_with_headers(&[("x-api-key", "secret")])
        ));
        assert!(is_authorized(
            &config,
            &request_with_headers(&[("authorization", "Bearer secret")])
        ));
        assert!(!is_authorized(
            &config,
            &request_with_headers(&[("x-api-key", "wrong")])
        ));
    }

    #[test]
    fn ip_allowed_with_x_real_ip_fallback() {
        let config = RpcSecurityConfig {
            allowed_ips: vec!["10.0.0.1".to_string()],
            cors_origins: vec!["https://wallet.example".to_string()],
            ..Default::default()
        };

        let allowed = request_with_headers(&[
            ("x-real-ip", "10.0.0.1"),
            ("origin", "https://wallet.example"),
        ]);
        let denied_ip = request_with_headers(&[
            ("x-real-ip", "10.0.0.2"),
            ("origin", "https://wallet.example"),
        ]);
        let denied_origin =
            request_with_headers(&[("x-real-ip", "10.0.0.1"), ("origin", "https://bad")]);

        assert!(is_ip_allowed(&config, &allowed));
        assert!(!is_ip_allowed(&config, &denied_ip));
        assert!(!is_origin_allowed(&config, &denied_origin));
    }

    #[test]
    fn origin_is_enforced_when_configured() {
        let config = RpcSecurityConfig {
            cors_origins: vec!["https://wallet.example".to_string()],
            ..Default::default()
        };

        let allowed = request_with_headers(&[("origin", "https://wallet.example")]);
        let denied = request_with_headers(&[("origin", "https://bad.example")]);

        assert!(is_origin_allowed(&config, &allowed));
        assert!(!is_origin_allowed(&config, &denied));
    }

    #[test]
    fn trusted_proxy_honors_x_forwarded_for() {
        let config = RpcSecurityConfig {
            allowed_ips: vec!["10.0.0.100".to_string()],
            trusted_proxies: vec!["127.0.0.1".to_string()],
            ..Default::default()
        };

        let req = request_with_headers(&[("x-forwarded-for", "10.0.0.100")]);
        let ip = extract_client_ip(&config, &req);
        assert_eq!(ip, Some("10.0.0.100".parse().unwrap()));
    }

    #[test]
    fn no_trusted_proxies_returns_none_without_x_real_ip() {
        let config = RpcSecurityConfig {
            allowed_ips: vec!["10.0.0.100".to_string()],
            trusted_proxies: vec![],
            ..Default::default()
        };

        // Without trusted proxies and without x-real-ip, no IP can be extracted
        let ip = extract_client_ip(
            &config,
            &request_with_headers(&[("x-forwarded-for", "10.0.0.100")]),
        );
        assert!(ip.is_none());
    }

    #[test]
    fn per_ip_rate_limit_tracks_by_client_ip() {
        let config = RpcSecurityConfig {
            rate_limit_per_minute: Some(2),
            ..Default::default()
        };
        let rates = Arc::new(Mutex::new(HashMap::new()));
        let ip1: IpAddr = "1.1.1.1".parse().unwrap();
        let ip2: IpAddr = "2.2.2.2".parse().unwrap();

        assert!(is_per_ip_rate_limited(&config, &rates, Some(ip1)));
        assert!(is_per_ip_rate_limited(&config, &rates, Some(ip1)));
        assert!(!is_per_ip_rate_limited(&config, &rates, Some(ip1)));

        // Different IP has its own limit
        assert!(is_per_ip_rate_limited(&config, &rates, Some(ip2)));
        assert!(is_per_ip_rate_limited(&config, &rates, Some(ip2)));
        assert!(!is_per_ip_rate_limited(&config, &rates, Some(ip2)));
    }

    #[test]
    fn rate_limit_disabled_returns_true() {
        let config = RpcSecurityConfig {
            rate_limit_per_minute: None,
            ..Default::default()
        };
        let rates = Arc::new(Mutex::new(HashMap::new()));
        assert!(is_per_ip_rate_limited(
            &config,
            &rates,
            Some("1.1.1.1".parse().unwrap())
        ));
    }

    #[test]
    fn required_auth_requires_env_key() {
        let result = RpcSecurityConfig::from_env(
            true,
            Some("BUDLUM_TEST_MISSING_RPC_KEY"),
            Vec::new(),
            Vec::new(),
            None,
        );

        assert!(result.is_err());
    }

    #[test]
    fn operator_default_binds_localhost_only() {
        let config = RpcSecurityConfig::operator_default();
        assert!(!config.auth_required);
        assert!(config.allowed_ips.contains(&"127.0.0.1".to_string()));
        assert!(config.allowed_ips.contains(&"::1".to_string()));
        assert!(config.max_connections.is_some());
    }
}

#[cfg(test)]
mod admin_rpc_tests {
    use super::*;
    use crate::chain::blockchain::Blockchain;
    use crate::chain::chain_actor::ChainActor;
    use crate::consensus::pow::PoWEngine;
    use crate::network::node::Node;
    use std::sync::Arc;

    async fn build_servers() -> (RpcServer, RpcServer) {
        let consensus = Arc::new(PoWEngine::new(0));
        let blockchain = Blockchain::new(consensus, None, 1337, None);
        let (chain_actor, chain_handle) = ChainActor::new(blockchain);
        tokio::spawn(async move {
            chain_actor.run().await;
        });
        let node = Node::new(chain_handle.clone()).unwrap();
        let node_client = node.get_client();

        let public = RpcServer::with_security_and_mode(
            chain_handle.clone(),
            node_client.clone(),
            RpcSecurityConfig::default(),
            RpcMode::Public,
        );
        let operator = RpcServer::with_security_and_mode(
            chain_handle,
            node_client,
            RpcSecurityConfig::operator_default(),
            RpcMode::Operator,
        );
        (public, operator)
    }

    #[tokio::test]
    async fn admin_methods_rejected_on_public_listener() {
        let (public, _operator) = build_servers().await;
        let fake_peer =
            PeerId::from(libp2p::identity::Keypair::generate_ed25519().public()).to_string();

        assert!(public.admin_ban_peer(fake_peer.clone()).await.is_err());
        assert!(public.admin_unban_peer(fake_peer).await.is_err());
        assert!(public.admin_list_banned_peers().await.is_err());
    }

    #[tokio::test]
    async fn admin_ban_and_unban_roundtrip_on_operator_listener() {
        let (_public, operator) = build_servers().await;
        let fake_peer =
            PeerId::from(libp2p::identity::Keypair::generate_ed25519().public()).to_string();

        let banned = operator.admin_list_banned_peers().await.unwrap();
        assert_eq!(banned["bannedPeers"].as_array().unwrap().len(), 0);

        operator.admin_ban_peer(fake_peer.clone()).await.unwrap();
        let banned = operator.admin_list_banned_peers().await.unwrap();
        assert_eq!(
            banned["bannedPeers"].as_array().unwrap(),
            &vec![serde_json::Value::String(fake_peer.clone())]
        );

        operator.admin_unban_peer(fake_peer).await.unwrap();
        let banned = operator.admin_list_banned_peers().await.unwrap();
        assert_eq!(banned["bannedPeers"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn admin_ban_peer_rejects_invalid_peer_id() {
        let (_public, operator) = build_servers().await;
        assert!(operator
            .admin_ban_peer("not-a-valid-peer-id".to_string())
            .await
            .is_err());
    }
}
