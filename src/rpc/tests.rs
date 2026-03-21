#[cfg(test)]
mod tests {
    use crate::rpc::server::RpcServer;
    use crate::rpc::api::BudlumApiServer;
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::PoWEngine;
    use crate::core::transaction::Transaction;
    use crate::network::node::Node;
    use std::sync::{Arc, Mutex};

    fn setup() -> (RpcServer, Arc<Mutex<Blockchain>>) {
        let consensus = Arc::new(PoWEngine::new(0));
        let blockchain = Arc::new(Mutex::new(Blockchain::new(consensus, None, 1337, None)));
        let node_struct = Node::new(blockchain.clone()).unwrap();
        let node_client = node_struct.get_client();
        (RpcServer::new(blockchain.clone(), node_client), blockchain)
    }

    #[tokio::test]
    async fn test_rpc_chain_info() {
        let (server, _) = setup();
        assert_eq!(server.chain_id().unwrap(), "0x539");
        assert_eq!(server.net_version().unwrap(), "1337");
    }

    #[tokio::test]
    async fn test_rpc_block_methods() {
        let (server, bc) = setup();
        assert_eq!(server.block_number().unwrap(), "0x1");
        
        let genesis_hash = bc.lock().unwrap().chain[0].hash.clone();
        let block_by_hash = server.get_block_by_hash(genesis_hash.clone()).unwrap();
        assert_eq!(block_by_hash["hash"], genesis_hash);
        assert_eq!(block_by_hash["number"], "0x0");

        let block_by_num = server.get_block_by_number(0).unwrap();
        assert_eq!(block_by_num["hash"], genesis_hash);
    }

    #[tokio::test]
    async fn test_rpc_account_methods() {
        let (server, bc) = setup();
        let addr = "test_addr";
        {
            let mut bc_lock = bc.lock().unwrap();
            bc_lock.init_genesis_account(addr);
        }
        assert_eq!(server.get_balance(addr.to_string()).unwrap(), "0x3b9aca00");
        assert_eq!(server.get_nonce(addr.to_string()).unwrap(), "0x0");
    }

    #[tokio::test]
    async fn test_rpc_transaction_methods() {
        let (server, bc) = setup();
        let keypair = crate::crypto::primitives::KeyPair::generate().unwrap();
        let from = keypair.public_key_hex();
        
        {
            let mut bc_lock = bc.lock().unwrap();
            bc_lock.state.add_balance(&from, 1000);
        }

        let mut tx = Transaction::new(from.clone(), "bob".into(), 100, vec![]);
        tx.fee = 1;
        tx.sign(&keypair);
        let tx_hash = tx.hash.clone();
        
        server.send_raw_transaction(tx.clone()).unwrap();
        
        let retrieved_tx = server.get_transaction_by_hash(tx_hash.clone()).unwrap();
        assert_eq!(retrieved_tx["hash"], tx_hash);
        assert_eq!(retrieved_tx["amount"], "0x64");

        let receipt = server.get_transaction_receipt(tx_hash.clone()).unwrap();
        assert_eq!(receipt["transactionHash"], tx_hash);
        assert_eq!(receipt["status"], "0x1");
        assert_eq!(receipt["blockNumber"], "null");
    }

    #[tokio::test]
    async fn test_rpc_network_methods() {
        let (server, _) = setup();
        assert_eq!(server.net_listening().unwrap(), true);
        assert_eq!(server.net_peer_count().unwrap(), "0x0");
        assert_eq!(server.syncing().unwrap(), false);
    }

    #[tokio::test]
    async fn test_rpc_fee_methods() {
        let (server, _) = setup();
        assert_eq!(server.gas_price().unwrap(), "0x1");
        let tx = Transaction::new("a".into(), "b".into(), 100, vec![]);
        assert_eq!(server.estimate_gas(tx).unwrap(), "0x5208");
    }

    #[tokio::test]
    async fn test_rpc_tx_precheck() {
        let (server, bc) = setup();
        let keypair = crate::crypto::primitives::KeyPair::generate().unwrap();
        let from = keypair.public_key_hex();
        
        let mut tx = Transaction::new(from.clone(), "bob".into(), 100, vec![]);
        tx.nonce = 10; 
        let precheck = server.tx_precheck(tx.clone()).unwrap();
        assert_eq!(precheck["accepted"], false);
        assert!(precheck["reasons"].as_array().unwrap().iter().any(|r| r == "insufficient_funds"));

        {
            let mut bc_lock = bc.lock().unwrap();
            bc_lock.state.add_balance(&from, 1000);
        }
        let precheck2 = server.tx_precheck(tx).unwrap();
        assert_eq!(precheck2["accepted"], true);
    }
}
