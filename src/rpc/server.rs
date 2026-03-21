use jsonrpsee::server::Server;
use jsonrpsee::types::error::ErrorObjectOwned;
use std::sync::{Arc, Mutex};
use crate::chain::blockchain::Blockchain;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use crate::network::node::NodeClient;
use super::api::BudlumApiServer;

pub struct RpcServer {
    blockchain: Arc<Mutex<Blockchain>>,
    node: NodeClient,
}

impl RpcServer {
    pub fn new(blockchain: Arc<Mutex<Blockchain>>, node: NodeClient) -> Self {
        Self { blockchain, node }
    }

    pub async fn run(self, addr: String) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let server = Server::builder().build(addr).await?;
        let handle = server.start(self.into_rpc());
        
        tokio::spawn(handle.stopped());
        Ok(())
    }
}

#[jsonrpsee::core::async_trait]
impl BudlumApiServer for RpcServer {
    fn chain_id(&self) -> Result<u64, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.chain_id)
    }

    fn block_number(&self) -> Result<u64, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.chain.len() as u64)
    }

    fn get_block_by_number(&self, number: u64) -> Result<Option<Block>, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.chain.get(number as usize).cloned())
    }

    fn get_block_by_hash(&self, hash: String) -> Result<Option<Block>, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.chain.iter().find(|b| b.hash == hash).cloned())
    }

    fn get_balance(&self, address: String) -> Result<u64, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.state.get_balance(&address))
    }

    fn get_nonce(&self, address: String) -> Result<u64, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.get_nonce(&address))
    }

    fn send_raw_transaction(&self, tx: Transaction) -> Result<String, ErrorObjectOwned> {
        let tx_hash = tx.hash.clone();
        let mut bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        bc.add_transaction(tx).map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))?;
        Ok(tx_hash)
    }

    fn get_transaction_by_hash(&self, hash: String) -> Result<Option<Transaction>, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.get_transaction_by_hash(&hash))
    }

    fn get_transaction_receipt(&self, hash: String) -> Result<serde_json::Value, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        if let Some(tx) = bc.get_transaction_by_hash(&hash) {
            Ok(serde_json::json!({
                "transactionHash": tx.hash,
                "from": tx.from,
                "to": tx.to,
                "amount": tx.amount,
                "status": "0x1"
            }))
        } else {
            Ok(serde_json::Value::Null)
        }
    }

    fn gas_price(&self) -> Result<u64, ErrorObjectOwned> {
        Ok(1)
    }

    fn estimate_gas(&self, _tx: Transaction) -> Result<u64, ErrorObjectOwned> {
        Ok(21000)
    }

    fn syncing(&self) -> Result<bool, ErrorObjectOwned> {
        Ok(false)
    }

    fn net_version(&self) -> Result<String, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(bc.chain_id.to_string())
    }

    fn net_listening(&self) -> Result<bool, ErrorObjectOwned> {
        Ok(true)
    }

    fn net_peer_count(&self) -> Result<usize, ErrorObjectOwned> {
        Ok(self.node.peer_count.load(std::sync::atomic::Ordering::SeqCst))
    }
}
