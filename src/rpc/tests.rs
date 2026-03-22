#[cfg(test)]
mod tests {
    use crate::rpc::server::RpcServer;
    use crate::rpc::api::BudlumApiServer;
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::pow::PoWEngine;
    use crate::core::transaction::Transaction;
    use crate::core::address::Address;
    use crate::network::node::Node;
    use crate::chain::chain_actor::{ChainActor, ChainHandle};
    use std::sync::Arc;

    async fn setup() -> (RpcServer, ChainHandle) {
        let consensus = Arc::new(PoWEngine::new(0));
        let blockchain = Blockchain::new(consensus, None, 1337, None);
        let (chain_actor, chain) = ChainActor::new(blockchain);
        tokio::spawn(async move {
            chain_actor.run().await;
        });
        let node_struct = Node::new(chain.clone()).unwrap();
        let node_client = node_struct.get_client();
        (RpcServer::new(chain.clone(), node_client), chain)
    }

    #[tokio::test]
    async fn test_rpc_chain_info() {
        let (server, _) = setup().await;
        let chain_id = server.chain_id().await.unwrap();
        println!("bud_chainId: {}", chain_id);
        assert_eq!(chain_id, "0x539");
    }

    #[tokio::test]
    async fn test_rpc_block_methods() {
        let (server, bc) = setup().await;
        let block_number = server.block_number().await.unwrap();
        println!("bud_blockNumber: {}", block_number);
        
        assert_eq!(block_number, "0x0");
        
        let genesis = bc.get_block(0).await.unwrap();
        let genesis_hash = genesis.hash.clone();
        let hex_genesis_hash = if genesis_hash.starts_with("0x") { genesis_hash } else { format!("0x{}", genesis_hash) };
        
        let block_by_hash = server.get_block_by_hash(hex_genesis_hash.clone()).await.unwrap();
        println!("bud_getBlockByHash: {}", serde_json::to_string_pretty(&block_by_hash).unwrap());
        assert_eq!(block_by_hash["hash"], hex_genesis_hash);
        assert!(block_by_hash["parentHash"].as_str().unwrap().starts_with("0x"));

        let block_by_num = server.get_block_by_number(0).await.unwrap();
        assert_eq!(block_by_num["hash"], hex_genesis_hash);
        
        let missing_block = server.get_block_by_number(999).await.unwrap();
        assert!(missing_block.is_null());
    }

    #[tokio::test]
    async fn test_rpc_account_methods() {
        let (server, bc) = setup().await;
        let addr = Address::from_hex(&"01".repeat(32)).unwrap();
        bc.init_genesis_account(&addr).await;
        
        let balance = server.get_balance(addr.to_string()).await.unwrap();
        println!("bud_getBalance: {}", balance);
        assert_eq!(balance, "0x3b9aca00");
    }

    #[tokio::test]
    async fn test_rpc_transaction_methods() {
        let (server, bc) = setup().await;
        let keypair = crate::crypto::primitives::KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        
        bc.add_balance(&from, 1000).await;

        let bob = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut tx = Transaction::new(from.clone(), bob, 100, vec![]);
        tx.fee = 1;
        tx.sign(&keypair);
        let hex_tx_hash = format!("0x{}", tx.hash);
        
        server.send_raw_transaction(tx.clone()).await.unwrap();
        
        let retrieved_tx = server.get_transaction_by_hash(hex_tx_hash.clone()).await.unwrap();
        println!("bud_getTransactionByHash: {}", serde_json::to_string_pretty(&retrieved_tx).unwrap());
        assert_eq!(retrieved_tx["hash"], hex_tx_hash);
        assert!(retrieved_tx["signature"].as_str().unwrap().starts_with("0x"));

        let receipt = server.get_transaction_receipt(hex_tx_hash.clone()).await.unwrap();
        error_to_json_result(server.get_transaction_receipt(hex_tx_hash.clone()).await);
        println!("bud_getTransactionReceipt (pending): {}", serde_json::to_string_pretty(&receipt).unwrap());

        assert!(receipt.is_null());
    }

    fn error_to_json_result<T>(res: Result<T, jsonrpsee::types::error::ErrorObjectOwned>) {
        let _ = res;
    }

    #[tokio::test]
    async fn test_rpc_error_cases() {
        let (server, _) = setup().await;
        
        let alice = Address::zero();
        let bob = Address::zero();
        let tx = Transaction::new(alice, bob, 100, vec![]);
        let result = server.send_raw_transaction(tx).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code(), -32602); 
        println!("Error Case (Invalid Params): {}", err);
    }

    #[tokio::test]
    async fn test_rpc_tx_precheck() {
        let (server, bc) = setup().await;
        let keypair = crate::crypto::primitives::KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        
        let bob = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut tx = Transaction::new(from.clone(), bob, 100, vec![]);
        tx.fee = 1;
        let precheck = server.tx_precheck(tx.clone()).await.unwrap();
        println!("bud_txPrecheck (no sig): {}", serde_json::to_string_pretty(&precheck).unwrap());
        assert_eq!(precheck["accepted"], false);
        assert!(precheck["reasons"].as_array().unwrap().iter().any(|r| r == "invalid_signature"));

        bc.add_balance(&from, 1000).await;
        
        let precheck2 = server.tx_precheck(tx.clone()).await.unwrap();
        assert_eq!(precheck2["accepted"], false);

        tx.sign(&keypair);
        let precheck3 = server.tx_precheck(tx).await.unwrap();
        println!("bud_txPrecheck (with sig): {}", serde_json::to_string_pretty(&precheck3).unwrap());
        assert_eq!(precheck3["accepted"], true);
    }
}
