use crate::chain::blockchain::Blockchain;
use crate::chain::finality::FinalityCert;
use crate::consensus::qc::{QcBlob, QcFaultProof};
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub enum ChainCommand {
    GetHeight(oneshot::Sender<u64>),
    GetBlock(u64, oneshot::Sender<Option<Block>>),
    GetBlockByHash(String, oneshot::Sender<Option<Block>>),
    GetBalance(Address, oneshot::Sender<u64>),
    GetNonce(Address, oneshot::Sender<u64>),
    AddTransaction(Transaction, oneshot::Sender<Result<(), String>>),
    ProduceBlock(Address, oneshot::Sender<Option<Block>>),
    ValidateAndAddBlock(Block, oneshot::Sender<Result<(), String>>),
    GetTransactionByHash(String, oneshot::Sender<Option<Transaction>>),
    GetTxReceipt(String, oneshot::Sender<Option<serde_json::Value>>),
    TxPrecheck(Transaction, oneshot::Sender<serde_json::Value>),
    GetChainId(oneshot::Sender<u64>),
    GetBaseFee(oneshot::Sender<u64>),
    GetValidatorSetHash(oneshot::Sender<String>),
    GetMempoolSize(oneshot::Sender<usize>),
    HandleFinalityCert(FinalityCert, oneshot::Sender<Result<(), String>>),
    ImportQcBlob(QcBlob, oneshot::Sender<Result<(), String>>),
    HandleQcFaultProof(QcFaultProof, oneshot::Sender<Result<(), String>>),
    SubmitSlashingEvidence(
        crate::consensus::pos::SlashingEvidence,
        oneshot::Sender<Result<(), String>>,
    ),
    DrainSlashingEvidence(oneshot::Sender<Vec<crate::consensus::pos::SlashingEvidence>>),
    CleanupMempool(oneshot::Sender<usize>),
    TryReorg(Vec<Block>, oneshot::Sender<Result<bool, String>>),
    GetChainInfo(oneshot::Sender<String>),
    GetLocator(oneshot::Sender<Vec<String>>),
    FindCommonHeight(Vec<String>, oneshot::Sender<Option<u64>>),
    GetQcBlob(u64, oneshot::Sender<Option<crate::consensus::qc::QcBlob>>),
    GetFinalityCert(
        u64,
        oneshot::Sender<Option<crate::chain::finality::FinalityCert>>,
    ),
    GetStateRoot(u64, oneshot::Sender<Option<String>>),
    AddBalance(Address, u64, oneshot::Sender<()>),
    InitGenesis(Address, oneshot::Sender<()>),
    GetStateSnapshotData(
        u64,
        oneshot::Sender<Option<crate::chain::snapshot::StateSnapshot>>,
    ),
    ApplySnapshot(
        crate::chain::snapshot::StateSnapshot,
        oneshot::Sender<Result<(), String>>,
    ),
    GetSettlementInfo(oneshot::Sender<serde_json::Value>),
    GetGlobalHeader(
        u64,
        oneshot::Sender<Option<crate::settlement::GlobalBlockHeader>>,
    ),
    GetDomainCommitments(oneshot::Sender<Vec<crate::domain::DomainCommitment>>),
    GetConsensusDomains(oneshot::Sender<Vec<crate::domain::ConsensusDomain>>),
    RegisterConsensusDomain(
        crate::domain::ConsensusDomain,
        oneshot::Sender<Result<(), String>>,
    ),
    SubmitDomainCommitment(
        crate::domain::DomainCommitment,
        oneshot::Sender<Result<(), String>>,
    ),
    SubmitVerifiedDomainCommitment(
        crate::domain::VerifiedDomainCommitment,
        oneshot::Sender<Result<(), String>>,
    ),
    SubmitCrossDomainMessage(
        crate::cross_domain::CrossDomainMessage,
        oneshot::Sender<Result<(), String>>,
    ),
    BuildGlobalHeader(oneshot::Sender<Result<crate::settlement::GlobalBlockHeader, String>>),
    GetDomainHeight(
        crate::domain::DomainId,
        oneshot::Sender<Result<u64, String>>,
    ),
    RegisterBridgeAsset {
        asset_id: crate::cross_domain::AssetId,
        domain: crate::domain::DomainId,
        response: oneshot::Sender<Result<(), String>>,
    },
    LockBridgeTransfer {
        source_domain: crate::domain::DomainId,
        target_domain: crate::domain::DomainId,
        source_height: u64,
        event_index: u32,
        asset_id: crate::cross_domain::AssetId,
        owner: crate::core::address::Address,
        recipient: crate::core::address::Address,
        amount: u128,
        expiry_height: u64,
        response: oneshot::Sender<
            Result<
                (
                    crate::cross_domain::BridgeTransfer,
                    crate::cross_domain::DomainEvent,
                ),
                String,
            >,
        >,
    },
    MintBridgeTransferFromVerifiedEvent {
        source_domain: crate::domain::DomainId,
        source_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: crate::cross_domain::DomainEvent,
        proof: crate::cross_domain::MerkleProof,
        response: oneshot::Sender<Result<(), String>>,
    },
    BurnBridgeTransfer {
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
        response: oneshot::Sender<Result<(), String>>,
    },
    UnlockBridgeTransfer {
        message_id: crate::cross_domain::MessageId,
        source_domain: crate::domain::DomainId,
        response: oneshot::Sender<Result<(), String>>,
    },
    SealGlobalHeader(oneshot::Sender<Result<crate::settlement::GlobalBlockHeader, String>>),
    FlushStorage(oneshot::Sender<Result<usize, String>>),
}

#[derive(Clone)]
pub struct ChainHandle {
    tx: mpsc::Sender<ChainCommand>,
}

impl ChainHandle {
    pub fn new(tx: mpsc::Sender<ChainCommand>) -> Self {
        Self { tx }
    }

    pub async fn get_height(&self) -> u64 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetHeight(tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn get_block(&self, height: u64) -> Option<Block> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetBlock(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_block_by_hash(&self, hash: String) -> Option<Block> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetBlockByHash(hash, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_balance(&self, addr: &Address) -> u64 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetBalance(*addr, tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn get_nonce(&self, addr: &Address) -> u64 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetNonce(*addr, tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn add_transaction(&self, tx: Transaction) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::AddTransaction(tx, res_tx)).await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn produce_block(&self, producer: Address) -> Option<Block> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::ProduceBlock(producer, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn validate_and_add_block(&self, block: Block) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::ValidateAndAddBlock(block, res_tx))
            .await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn get_transaction_by_hash(&self, hash: String) -> Option<Transaction> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::GetTransactionByHash(hash, tx))
            .await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_tx_receipt(&self, hash: String) -> Option<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetTxReceipt(hash, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn tx_precheck(&self, tx_obj: Transaction) -> serde_json::Value {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::TxPrecheck(tx_obj, tx)).await;
        rx.await.unwrap_or_else(|_| {
            serde_json::json!({
                "accepted": false,
                "reasons": ["actor_dropped"]
            })
        })
    }

    pub async fn get_chain_id(&self) -> u64 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetChainId(tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn get_base_fee(&self) -> u64 {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetBaseFee(tx)).await;
        rx.await.unwrap_or(1)
    }

    pub async fn get_validator_set_hash(&self) -> String {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetValidatorSetHash(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn get_mempool_size(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetMempoolSize(tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn handle_finality_cert(&self, cert: FinalityCert) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::HandleFinalityCert(cert, res_tx))
            .await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn cleanup_mempool(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::CleanupMempool(tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn import_qc_blob(&self, blob: QcBlob) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::ImportQcBlob(blob, res_tx)).await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn handle_qc_fault_proof(&self, proof: QcFaultProof) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::HandleQcFaultProof(proof, res_tx))
            .await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn submit_slashing_evidence(
        &self,
        evidence: crate::consensus::pos::SlashingEvidence,
    ) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::SubmitSlashingEvidence(evidence, res_tx))
            .await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn drain_slashing_evidence(&self) -> Vec<crate::consensus::pos::SlashingEvidence> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::DrainSlashingEvidence(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn try_reorg(&self, fork: Vec<Block>) -> Result<bool, String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::TryReorg(fork, res_tx)).await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn get_chain_info(&self) -> String {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetChainInfo(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn get_locator(&self) -> Vec<String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetLocator(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn find_common_height(&self, locator: Vec<String>) -> Option<u64> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::FindCommonHeight(locator, tx))
            .await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_qc_blob(&self, height: u64) -> Option<crate::consensus::qc::QcBlob> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetQcBlob(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_finality_cert(
        &self,
        height: u64,
    ) -> Option<crate::chain::finality::FinalityCert> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::GetFinalityCert(height, tx))
            .await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_state_root(&self, height: u64) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetStateRoot(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn add_balance(&self, address: &Address, amount: u64) {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::AddBalance(*address, amount, tx))
            .await;
        let _ = rx.await;
    }

    pub async fn init_genesis_account(&self, address: &Address) {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::InitGenesis(*address, tx)).await;
        let _ = rx.await;
    }

    pub async fn get_state_snapshot_data(
        &self,
        height: u64,
    ) -> Option<crate::chain::snapshot::StateSnapshot> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::GetStateSnapshotData(height, tx))
            .await;
        rx.await.unwrap_or(None)
    }

    pub async fn apply_snapshot(
        &self,
        snapshot: crate::chain::snapshot::StateSnapshot,
    ) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::ApplySnapshot(snapshot, res_tx))
            .await;
        res_rx
            .await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn get_settlement_info(&self) -> serde_json::Value {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetSettlementInfo(tx)).await;
        rx.await.unwrap_or_else(|_| {
            serde_json::json!({
                "error": "actor_dropped"
            })
        })
    }

    pub async fn get_global_header(
        &self,
        height: u64,
    ) -> Option<crate::settlement::GlobalBlockHeader> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::GetGlobalHeader(height, tx))
            .await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_domain_commitments(&self) -> Vec<crate::domain::DomainCommitment> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetDomainCommitments(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn get_domain_height(
        &self,
        domain_id: crate::domain::DomainId,
    ) -> Result<u64, String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::GetDomainHeight(domain_id, tx))
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn build_global_header(
        &self,
        _dummy: Option<()>,
    ) -> Result<crate::settlement::GlobalBlockHeader, String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::BuildGlobalHeader(tx)).await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn get_consensus_domains(&self) -> Vec<crate::domain::ConsensusDomain> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetConsensusDomains(tx)).await;
        rx.await.unwrap_or_default()
    }

    pub async fn register_consensus_domain(
        &self,
        domain: crate::domain::ConsensusDomain,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::RegisterConsensusDomain(domain, tx))
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn submit_domain_commitment(
        &self,
        commitment: crate::domain::DomainCommitment,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::SubmitDomainCommitment(commitment, tx))
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn submit_verified_domain_commitment(
        &self,
        payload: crate::domain::VerifiedDomainCommitment,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::SubmitVerifiedDomainCommitment(payload, tx))
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn submit_cross_domain_message(
        &self,
        message: crate::cross_domain::CrossDomainMessage,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::SubmitCrossDomainMessage(message, tx))
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn register_bridge_asset(
        &self,
        asset_id: crate::cross_domain::AssetId,
        domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::RegisterBridgeAsset {
                asset_id,
                domain,
                response: tx,
            })
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn lock_bridge_transfer(
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
    ) -> Result<
        (
            crate::cross_domain::BridgeTransfer,
            crate::cross_domain::DomainEvent,
        ),
        String,
    > {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::LockBridgeTransfer {
                source_domain,
                target_domain,
                source_height,
                event_index,
                asset_id,
                owner,
                recipient,
                amount,
                expiry_height,
                response: tx,
            })
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn mint_bridge_transfer_from_verified_event(
        &self,
        source_domain: crate::domain::DomainId,
        source_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: crate::cross_domain::DomainEvent,
        proof: crate::cross_domain::MerkleProof,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::MintBridgeTransferFromVerifiedEvent {
                source_domain,
                source_height,
                sequence,
                expected_block_hash,
                event,
                proof,
                response: tx,
            })
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn burn_bridge_transfer(
        &self,
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::BurnBridgeTransfer {
                message_id,
                domain,
                response: tx,
            })
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn unlock_bridge_transfer(
        &self,
        message_id: crate::cross_domain::MessageId,
        source_domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        let (tx, rx) = oneshot::channel();
        let _ = self
            .tx
            .send(ChainCommand::UnlockBridgeTransfer {
                message_id,
                source_domain,
                response: tx,
            })
            .await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn seal_global_header(&self) -> Result<crate::settlement::GlobalBlockHeader, String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::SealGlobalHeader(tx)).await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn flush_storage(&self) -> Result<usize, String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::FlushStorage(tx)).await;
        rx.await
            .unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }
}

pub struct ChainActor {
    blockchain: Blockchain,
    rx: mpsc::Receiver<ChainCommand>,
}

impl ChainActor {
    pub fn new(blockchain: Blockchain) -> (Self, ChainHandle) {
        let (tx, rx) = mpsc::channel(1000);
        (Self { blockchain, rx }, ChainHandle { tx })
    }

    pub async fn run(mut self) {
        while let Some(cmd) = self.rx.recv().await {
            match cmd {
                ChainCommand::GetHeight(tx) => {
                    let height = self.blockchain.chain.len().saturating_sub(1) as u64;
                    let _ = tx.send(height);
                }
                ChainCommand::GetBlock(height, tx) => {
                    let block = self.blockchain.chain.get(height as usize).cloned();
                    let _ = tx.send(block);
                }
                ChainCommand::GetBlockByHash(hash, tx) => {
                    let block = self
                        .blockchain
                        .chain
                        .iter()
                        .find(|b| b.hash == hash)
                        .cloned();
                    let _ = tx.send(block);
                }
                ChainCommand::GetBalance(addr, tx) => {
                    let balance = self.blockchain.state.get_balance(&addr);
                    let _ = tx.send(balance);
                }
                ChainCommand::GetNonce(addr, tx) => {
                    let nonce = self.blockchain.state.get_nonce(&addr);
                    let _ = tx.send(nonce);
                }
                ChainCommand::AddTransaction(tx_obj, res_tx) => {
                    let _ = res_tx.send(
                        self.blockchain
                            .add_transaction(tx_obj)
                            .map_err(|e| e.to_string()),
                    );
                }
                ChainCommand::ProduceBlock(producer, tx) => {
                    let block = self.blockchain.produce_block(producer);
                    let _ = tx.send(block);
                }
                ChainCommand::ValidateAndAddBlock(block, res_tx) => {
                    let _ = res_tx.send(
                        self.blockchain
                            .validate_and_add_block(block)
                            .map_err(|e| e.to_string()),
                    );
                }
                ChainCommand::GetTransactionByHash(hash, tx) => {
                    let tx_obj = self.blockchain.get_transaction_by_hash(&hash);
                    let _ = tx.send(tx_obj);
                }
                ChainCommand::GetTxReceipt(hash, tx) => {
                    let receipt = self.blockchain.get_transaction_receipt(&hash);
                    let _ = tx.send(receipt);
                }
                ChainCommand::TxPrecheck(tx_obj, tx) => {
                    let _ = tx.send(self.blockchain.tx_precheck(&tx_obj));
                }
                ChainCommand::GetChainId(tx) => {
                    let _ = tx.send(self.blockchain.chain_id);
                }
                ChainCommand::GetBaseFee(tx) => {
                    let _ = tx.send(self.blockchain.state.base_fee);
                }
                ChainCommand::GetValidatorSetHash(tx) => {
                    let _ = tx.send(self.blockchain.get_validator_set_hash());
                }
                ChainCommand::GetMempoolSize(tx) => {
                    let _ = tx.send(self.blockchain.mempool.len());
                }
                ChainCommand::HandleFinalityCert(cert, res_tx) => {
                    let _ = res_tx.send(
                        self.blockchain
                            .handle_finality_cert(cert)
                            .map_err(|e| e.to_string()),
                    );
                }
                ChainCommand::ImportQcBlob(blob, res_tx) => {
                    let _ = res_tx.send(
                        self.blockchain
                            .import_qc_blob(blob)
                            .map_err(|e| e.to_string()),
                    );
                }
                ChainCommand::HandleQcFaultProof(proof, res_tx) => {
                    let _ = res_tx.send(
                        self.blockchain
                            .handle_qc_fault_proof(proof)
                            .map_err(|e| e.to_string()),
                    );
                }
                ChainCommand::SubmitSlashingEvidence(evidence, res_tx) => {
                    let _ = res_tx.send(self.blockchain.submit_slashing_evidence(evidence));
                }
                ChainCommand::DrainSlashingEvidence(tx) => {
                    let _ = tx.send(self.blockchain.drain_local_slashing_evidence());
                }
                ChainCommand::CleanupMempool(tx) => {
                    let removed = self.blockchain.mempool.cleanup_expired();
                    let _ = tx.send(removed);
                }
                ChainCommand::TryReorg(fork, res_tx) => {
                    let _ = res_tx.send(self.blockchain.try_reorg(fork).map_err(|e| e.to_string()));
                }
                ChainCommand::GetChainInfo(tx) => {
                    let info = format!(
                        "Height: {}, BaseFee: {}, Mempool: {}",
                        self.blockchain.chain.len(),
                        self.blockchain.state.base_fee,
                        self.blockchain.mempool.len()
                    );
                    let _ = tx.send(info);
                }
                ChainCommand::GetLocator(tx) => {
                    let mut locator = Vec::new();
                    let mut step = 1;
                    let mut current = self.blockchain.chain.len().saturating_sub(1);
                    while current > 0 && locator.len() < 10 {
                        locator.push(self.blockchain.chain[current].hash.clone());
                        current = current.saturating_sub(step);
                        step *= 2;
                    }
                    if locator.is_empty() && !self.blockchain.chain.is_empty() {
                        locator.push(self.blockchain.chain[0].hash.clone());
                    }
                    let _ = tx.send(locator);
                }
                ChainCommand::FindCommonHeight(locator, tx) => {
                    let common = locator.iter().find_map(|hash| {
                        self.blockchain
                            .chain
                            .iter()
                            .position(|b| &b.hash == hash)
                            .map(|p| p as u64)
                    });
                    let _ = tx.send(common);
                }
                ChainCommand::GetQcBlob(height, tx) => {
                    let res = self.blockchain.get_qc_blob(height);
                    let _ = tx.send(res);
                }
                ChainCommand::GetFinalityCert(height, tx) => {
                    let res = self
                        .blockchain
                        .storage
                        .as_ref()
                        .and_then(|s| s.get_finality_cert(height).unwrap_or(None));
                    let _ = tx.send(res);
                }
                ChainCommand::GetStateRoot(height, tx) => {
                    let res = self.blockchain.get_state_root(height);
                    let _ = tx.send(res);
                }
                ChainCommand::AddBalance(addr, amount, tx) => {
                    self.blockchain.state.add_balance(&addr, amount);
                    let _ = tx.send(());
                }
                ChainCommand::InitGenesis(addr, tx) => {
                    self.blockchain.init_genesis_account(&addr);
                    let _ = tx.send(());
                }
                ChainCommand::GetStateSnapshotData(height, tx) => {
                    let res = self.blockchain.get_state_snapshot(height);
                    let _ = tx.send(res);
                }
                ChainCommand::ApplySnapshot(snapshot, res_tx) => {
                    let res = self.blockchain.apply_state_snapshot(snapshot);
                    let _ = res_tx.send(res.map_err(|e: String| e.to_string()));
                }
                ChainCommand::GetSettlementInfo(tx) => {
                    let header = self.blockchain.build_global_header(None);
                    let info = serde_json::json!({
                        "globalHeight": self.blockchain.global_headers.len(),
                        "latestGlobalHash": self.blockchain.global_headers.last().map(|h| h.calculate_hash()),
                        "pendingGlobalHash": header.calculate_hash(),
                        "domainRegistryRoot": hex::encode(header.domain_registry_root),
                        "domainCommitmentRoot": hex::encode(header.domain_commitment_root),
                        "bridgeStateRoot": hex::encode(header.bridge_state_root),
                        "replayNonceRoot": hex::encode(header.replay_nonce_root),
                        "domainCommitmentCount": self.blockchain.domain_commitment_registry.len(),
                    });
                    let _ = tx.send(info);
                }
                ChainCommand::GetGlobalHeader(height, tx) => {
                    let header = self.blockchain.global_headers.get(height as usize).cloned();
                    let _ = tx.send(header);
                }
                ChainCommand::GetDomainCommitments(tx) => {
                    let commitments = self
                        .blockchain
                        .domain_commitment_registry
                        .commitments_for_global_block();
                    let _ = tx.send(commitments);
                }
                ChainCommand::GetConsensusDomains(tx) => {
                    let _ = tx.send(self.blockchain.domain_registry.domains());
                }
                ChainCommand::RegisterConsensusDomain(domain, res_tx) => {
                    let _ = res_tx.send(self.blockchain.register_consensus_domain(domain));
                }
                ChainCommand::SubmitDomainCommitment(commitment, res_tx) => {
                    let _ = res_tx.send(self.blockchain.submit_domain_commitment(commitment));
                }
                ChainCommand::SubmitVerifiedDomainCommitment(payload, res_tx) => {
                    let _ = res_tx.send(
                        self.blockchain
                            .submit_verified_domain_commitment(payload.commitment, payload.proof),
                    );
                }
                ChainCommand::SubmitCrossDomainMessage(message, res_tx) => {
                    let _ = res_tx.send(self.blockchain.submit_cross_domain_message(message));
                }
                ChainCommand::BuildGlobalHeader(res_tx) => {
                    let header = self.blockchain.build_global_header(None);
                    let _ = res_tx.send(Ok(header));
                }
                ChainCommand::GetDomainHeight(domain_id, res_tx) => {
                    let res = self
                        .blockchain
                        .domain_registry
                        .get(domain_id)
                        .map(|d| d.last_committed_height)
                        .ok_or_else(|| format!("Domain {} not found", domain_id));
                    let _ = res_tx.send(res);
                }
                ChainCommand::RegisterBridgeAsset {
                    asset_id,
                    domain,
                    response,
                } => {
                    let _ = response.send(self.blockchain.register_bridge_asset(asset_id, domain));
                }
                ChainCommand::LockBridgeTransfer {
                    source_domain,
                    target_domain,
                    source_height,
                    event_index,
                    asset_id,
                    owner,
                    recipient,
                    amount,
                    expiry_height,
                    response,
                } => {
                    let _ = response.send(self.blockchain.lock_bridge_transfer(
                        source_domain,
                        target_domain,
                        source_height,
                        event_index,
                        asset_id,
                        owner,
                        recipient,
                        amount,
                        expiry_height,
                    ));
                }
                ChainCommand::MintBridgeTransferFromVerifiedEvent {
                    source_domain,
                    source_height,
                    sequence,
                    expected_block_hash,
                    event,
                    proof,
                    response,
                } => {
                    let _ =
                        response.send(self.blockchain.mint_bridge_transfer_from_verified_event(
                            source_domain,
                            source_height,
                            sequence,
                            expected_block_hash,
                            event,
                            &proof,
                        ));
                }
                ChainCommand::BurnBridgeTransfer {
                    message_id,
                    domain,
                    response,
                } => {
                    let _ = response.send(self.blockchain.burn_bridge_transfer(message_id, domain));
                }
                ChainCommand::UnlockBridgeTransfer {
                    message_id,
                    source_domain,
                    response,
                } => {
                    let _ = response.send(
                        self.blockchain
                            .unlock_bridge_transfer(message_id, source_domain),
                    );
                }
                ChainCommand::SealGlobalHeader(res_tx) => {
                    let _ = res_tx.send(self.blockchain.seal_global_header(None));
                }
                ChainCommand::FlushStorage(res_tx) => {
                    let res = self
                        .blockchain
                        .storage
                        .as_ref()
                        .map(|storage| storage.flush_batch().map_err(|e| e.to_string()))
                        .unwrap_or(Ok(0));
                    let _ = res_tx.send(res);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::pow::PoWEngine;
    use crate::core::address::Address;
    use std::sync::Arc;

    async fn setup_actor() -> ChainHandle {
        let consensus = Arc::new(PoWEngine::new(0));
        let blockchain = Blockchain::new(consensus, None, 1337, None);
        let (chain_actor, chain) = ChainActor::new(blockchain);
        tokio::spawn(async move {
            chain_actor.run().await;
        });
        chain
    }

    #[tokio::test]
    async fn test_actor_submit_domain_commitment() {
        let chain = setup_actor().await;
        let domain = crate::domain::plugin::default_domain(
            1,
            crate::domain::ConsensusKind::PoW,
            1337,
            "pow-confirmation-depth",
            0,
        );
        chain
            .register_consensus_domain(domain.clone())
            .await
            .unwrap();

        let block = crate::core::block::Block::new(1, "aa".repeat(32), vec![]);
        let commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block, [2u8; 32], [3u8; 32], 0)
                .unwrap();

        assert!(chain.submit_domain_commitment(commitment).await.is_ok());
    }

    #[tokio::test]
    async fn test_actor_submit_verified_domain_commitment() {
        let chain = setup_actor().await;
        let domain = crate::domain::plugin::default_domain(
            1,
            crate::domain::ConsensusKind::PoW,
            1337,
            "pow-confirmation-depth",
            0,
        );
        chain
            .register_consensus_domain(domain.clone())
            .await
            .unwrap();

        let block = crate::core::block::Block::new(1, "aa".repeat(32), vec![]);
        let proof = crate::domain::FinalityProof::PoW {
            confirmations: 64,
            total_work_hint: 1000,
        };
        let mut commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block, [2u8; 32], [3u8; 32], 0)
                .unwrap();
        commitment.finality_proof_hash = crate::domain::hash_finality_proof(&proof);

        let payload = crate::domain::VerifiedDomainCommitment { commitment, proof };
        assert!(chain
            .submit_verified_domain_commitment(payload)
            .await
            .is_ok());
        assert_eq!(chain.get_domain_commitments().await.len(), 1);
    }

    #[tokio::test]
    async fn test_actor_submit_cross_domain_message() {
        let chain = setup_actor().await;

        let msg = crate::cross_domain::CrossDomainMessage::new(
            crate::cross_domain::message::CrossDomainMessageParams {
                source_domain: 1,
                target_domain: 2,
                source_height: 10,
                event_index: 0,
                nonce: 42,
                sender: Address::zero(),
                recipient: Address::zero(),
                payload_hash: [9u8; 32],
                kind: crate::cross_domain::MessageKind::BridgeLock,
                expiry_height: 100,
            },
        );

        assert!(chain.submit_cross_domain_message(msg).await.is_ok());
    }
}
