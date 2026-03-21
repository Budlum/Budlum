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

    fn to_hex(n: u64) -> String {
        format!("0x{:x}", n)
    }

    fn block_to_json(b: Block) -> serde_json::Value {
        serde_json::json!({
            "number": Self::to_hex(b.index),
            "hash": b.hash,
            "parentHash": b.previous_hash,
            "timestamp": Self::to_hex(b.timestamp as u64),
            "transactions": b.transactions.into_iter().map(Self::tx_to_json).collect::<Vec<_>>(),
            "producer": b.producer,
            "signature": b.signature.map(hex::encode),
            "stateRoot": b.state_root,
            "txRoot": b.tx_root,
        })
    }

    fn tx_to_json(t: Transaction) -> serde_json::Value {
        serde_json::json!({
            "hash": t.hash,
            "from": t.from,
            "to": t.to,
            "amount": Self::to_hex(t.amount),
            "fee": Self::to_hex(t.fee),
            "nonce": Self::to_hex(t.nonce),
            "timestamp": Self::to_hex(t.timestamp as u64),
            "type": format!("{:?}", t.tx_type),
            "chainId": Self::to_hex(t.chain_id),
        })
    }
}

#[jsonrpsee::core::async_trait]
impl BudlumApiServer for RpcServer {
    fn chain_id(&self) -> Result<String, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(Self::to_hex(bc.chain_id))
    }

    fn block_number(&self) -> Result<String, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(Self::to_hex(bc.chain.len() as u64))
    }

    fn get_block_by_number(&self, number: u64) -> Result<serde_json::Value, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        match bc.chain.get(number as usize).cloned() {
            Some(b) => Ok(Self::block_to_json(b)),
            None => Ok(serde_json::Value::Null),
        }
    }

    fn get_block_by_hash(&self, hash: String) -> Result<serde_json::Value, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        match bc.chain.iter().find(|b| b.hash == hash).cloned() {
            Some(b) => Ok(Self::block_to_json(b)),
            None => Ok(serde_json::Value::Null),
        }
    }

    fn get_balance(&self, address: String) -> Result<String, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(Self::to_hex(bc.state.get_balance(&address)))
    }

    fn get_nonce(&self, address: String) -> Result<String, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        Ok(Self::to_hex(bc.get_nonce(&address)))
    }

    fn send_raw_transaction(&self, tx: Transaction) -> Result<String, ErrorObjectOwned> {
        let tx_hash = tx.hash.clone();
        let mut bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        bc.add_transaction(tx).map_err(|e| ErrorObjectOwned::owned(-32603, e.to_string(), None::<()>))?;
        Ok(tx_hash)
    }

    fn get_transaction_by_hash(&self, hash: String) -> Result<serde_json::Value, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        match bc.get_transaction_by_hash(&hash) {
            Some(t) => Ok(Self::tx_to_json(t)),
            None => Ok(serde_json::Value::Null),
        }
    }

    fn get_transaction_receipt(&self, hash: String) -> Result<serde_json::Value, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        if let Some(tx) = bc.get_transaction_by_hash(&hash) {
            let block_height = bc.chain.iter().enumerate()
                .find(|(_, b)| b.transactions.iter().any(|t| t.hash == hash))
                .map(|(i, _)| i as u64);

            Ok(serde_json::json!({
                "transactionHash": tx.hash,
                "blockNumber": block_height.map(Self::to_hex).unwrap_or_else(|| "null".to_string()),
                "from": tx.from,
                "to": tx.to,
                "amount": Self::to_hex(tx.amount),
                "gasUsed": "0x5208", 
                "status": "0x1"
            }))
        } else {
            Ok(serde_json::Value::Null)
        }
    }

    fn gas_price(&self) -> Result<String, ErrorObjectOwned> {
        Ok(Self::to_hex(1))
    }

    fn estimate_gas(&self, _tx: Transaction) -> Result<String, ErrorObjectOwned> {
        Ok(Self::to_hex(21000))
    }

    fn tx_precheck(&self, tx: Transaction) -> Result<serde_json::Value, ErrorObjectOwned> {
        let bc = self.blockchain.lock().map_err(|_| ErrorObjectOwned::owned(-32000, "Lock failed", None::<()>))?;
        let mut reasons = Vec::new();
        
        let current_nonce = bc.get_nonce(&tx.from);
        if tx.nonce < current_nonce {
            reasons.push("nonce_too_low");
        }
        
        let balance = bc.state.get_balance(&tx.from);
        if balance < tx.amount + tx.fee {
            reasons.push("insufficient_funds");
        }

        if tx.chain_id != bc.chain_id {
            reasons.push("invalid_chain_id");
        }

        Ok(serde_json::json!({
            "accepted": reasons.is_empty(),
            "reasons": reasons
        }))
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

    fn net_peer_count(&self) -> Result<String, ErrorObjectOwned> {
        Ok(Self::to_hex(self.node.peer_count.load(std::sync::atomic::Ordering::SeqCst) as u64))
    }
}
