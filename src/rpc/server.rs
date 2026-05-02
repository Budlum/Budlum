use super::api::BudlumApiServer;
use crate::chain::chain_actor::ChainHandle;
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use crate::network::node::NodeClient;
use jsonrpsee::types::error::ErrorObjectOwned;
use tracing::info;

pub struct RpcServer {
    chain: ChainHandle,
    node: NodeClient,
}

impl RpcServer {
    pub fn new(chain: ChainHandle, node: NodeClient) -> Self {
        Self { chain, node }
    }

    pub async fn run(self, addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        use jsonrpsee::server::ServerBuilder;
        let server = ServerBuilder::default().build(addr.clone()).await?;

        info!("RPC Server started on {}", addr);
        let handle = server.start(self.into_rpc());
        tokio::spawn(handle.stopped());
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
        let clean_hash = if hash.starts_with("0x") {
            &hash[2..]
        } else {
            &hash
        };
        match self.chain.get_block_by_hash(clean_hash.to_string()).await {
            Some(b) => Ok(Self::block_to_json(b)),
            None => Ok(serde_json::Value::Null),
        }
    }

    async fn get_balance(&self, address: String) -> Result<String, ErrorObjectOwned> {
        let clean_addr = if address.starts_with("0x") {
            &address[2..]
        } else {
            &address
        };
        let addr = Address::from_hex(clean_addr).map_err(|e| {
            ErrorObjectOwned::owned(-32602, format!("Invalid address: {}", e), None::<()>)
        })?;
        let balance = self.chain.get_balance(&addr).await;
        Ok(Self::to_hex(balance))
    }

    async fn get_nonce(&self, address: String) -> Result<String, ErrorObjectOwned> {
        let clean_addr = if address.starts_with("0x") {
            &address[2..]
        } else {
            &address
        };
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
        let clean_hash = if hash.starts_with("0x") {
            &hash[2..]
        } else {
            &hash
        };
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
        let clean_hash = if hash.starts_with("0x") {
            &hash[2..]
        } else {
            &hash
        };
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
        Ok(false)
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
}
