use crate::chain::blockchain::Blockchain;
use crate::core::block::Block;
use crate::core::address::Address;
use crate::core::transaction::Transaction;
use crate::chain::finality::FinalityCert;
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
    GetChainId(oneshot::Sender<u64>),
    GetBaseFee(oneshot::Sender<u64>),
    GetValidatorSetHash(oneshot::Sender<String>),
    GetMempoolSize(oneshot::Sender<usize>),
    HandleFinalityCert(FinalityCert, oneshot::Sender<Result<(), String>>),
    CleanupMempool(oneshot::Sender<usize>),
    TryReorg(Vec<Block>, oneshot::Sender<Result<bool, String>>),
    GetChainInfo(oneshot::Sender<String>),
    GetLocator(oneshot::Sender<Vec<String>>),
    FindCommonHeight(Vec<String>, oneshot::Sender<Option<u64>>),
    GetQcBlob(u64, oneshot::Sender<Option<crate::consensus::qc::QcBlob>>),
    GetFinalityCert(u64, oneshot::Sender<Option<crate::chain::finality::FinalityCert>>),
    GetStateRoot(u64, oneshot::Sender<Option<String>>),
    AddBalance(Address, u64, oneshot::Sender<()>),
    InitGenesis(Address, oneshot::Sender<()>),
    GetStateSnapshotData(u64, oneshot::Sender<Option<crate::chain::snapshot::StateSnapshot>>),
    ApplySnapshot(crate::chain::snapshot::StateSnapshot, oneshot::Sender<Result<(), String>>),
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
        res_rx.await.unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn produce_block(&self, producer: Address) -> Option<Block> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::ProduceBlock(producer, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn validate_and_add_block(&self, block: Block) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::ValidateAndAddBlock(block, res_tx)).await;
        res_rx.await.unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn get_transaction_by_hash(&self, hash: String) -> Option<Transaction> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetTransactionByHash(hash, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_tx_receipt(&self, hash: String) -> Option<serde_json::Value> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetTxReceipt(hash, tx)).await;
        rx.await.unwrap_or(None)
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
        let _ = self.tx.send(ChainCommand::HandleFinalityCert(cert, res_tx)).await;
        res_rx.await.unwrap_or_else(|_| Err("Actor dropped".to_string()))
    }

    pub async fn cleanup_mempool(&self) -> usize {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::CleanupMempool(tx)).await;
        rx.await.unwrap_or(0)
    }

    pub async fn try_reorg(&self, fork: Vec<Block>) -> Result<bool, String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::TryReorg(fork, res_tx)).await;
        res_rx.await.unwrap_or_else(|_| Err("Actor dropped".to_string()))
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
        let _ = self.tx.send(ChainCommand::FindCommonHeight(locator, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_qc_blob(&self, height: u64) -> Option<crate::consensus::qc::QcBlob> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetQcBlob(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_finality_cert(&self, height: u64) -> Option<crate::chain::finality::FinalityCert> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetFinalityCert(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn get_state_root(&self, height: u64) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetStateRoot(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn add_balance(&self, address: &Address, amount: u64) {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::AddBalance(*address, amount, tx)).await;
        let _ = rx.await;
    }

    pub async fn init_genesis_account(&self, address: &Address) {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::InitGenesis(*address, tx)).await;
        let _ = rx.await;
    }

    pub async fn get_state_snapshot_data(&self, height: u64) -> Option<crate::chain::snapshot::StateSnapshot> {
        let (tx, rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::GetStateSnapshotData(height, tx)).await;
        rx.await.unwrap_or(None)
    }

    pub async fn apply_snapshot(&self, snapshot: crate::chain::snapshot::StateSnapshot) -> Result<(), String> {
        let (res_tx, res_rx) = oneshot::channel();
        let _ = self.tx.send(ChainCommand::ApplySnapshot(snapshot, res_tx)).await;
        res_rx.await.unwrap_or_else(|_| Err("Actor dropped".to_string()))
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
                    let block = self.blockchain.chain.iter().find(|b| b.hash == hash).cloned();
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
                    let _ = res_tx.send(self.blockchain.add_transaction(tx_obj).map_err(|e| e.to_string()));
                }
                ChainCommand::ProduceBlock(producer, tx) => {
                    let block = self.blockchain.produce_block(producer);
                    let _ = tx.send(block);
                }
                ChainCommand::ValidateAndAddBlock(block, res_tx) => {
                    let _ = res_tx.send(self.blockchain.validate_and_add_block(block).map_err(|e| e.to_string()));
                }
                ChainCommand::GetTransactionByHash(hash, tx) => {
                    let tx_obj = self.blockchain.get_transaction_by_hash(&hash);
                    let _ = tx.send(tx_obj);
                }
                ChainCommand::GetTxReceipt(hash, tx) => {
                    let receipt = self.blockchain.get_transaction_receipt(&hash);
                    let _ = tx.send(receipt);
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
                    let _ = res_tx.send(self.blockchain.handle_finality_cert(cert).map_err(|e| e.to_string()));
                }
                ChainCommand::CleanupMempool(tx) => {
                    let removed = self.blockchain.mempool.cleanup_expired();
                    let _ = tx.send(removed);
                }
                ChainCommand::TryReorg(fork, res_tx) => {
                    let _ = res_tx.send(self.blockchain.try_reorg(fork).map_err(|e| e.to_string()));
                }
                ChainCommand::GetChainInfo(tx) => {
                    let info = format!("Height: {}, BaseFee: {}, Mempool: {}", 
                        self.blockchain.chain.len(), self.blockchain.state.base_fee, self.blockchain.mempool.len());
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
                        self.blockchain.chain.iter().position(|b| &b.hash == hash).map(|p| p as u64)
                    });
                    let _ = tx.send(common);
                }
                ChainCommand::GetQcBlob(height, tx) => {
                    let res = self.blockchain.storage.as_ref().and_then(|s| s.get_qc_blob(height).unwrap_or(None));
                    let _ = tx.send(res);
                }
                ChainCommand::GetFinalityCert(height, tx) => {
                    let res = self.blockchain.storage.as_ref().and_then(|s| s.get_finality_cert(height).unwrap_or(None));
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
            }
        }
    }
}
