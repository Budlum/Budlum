#[cfg(test)]
mod chaos_tests {
    use crate::chain::blockchain::{Blockchain, MAX_REORG_DEPTH};
    use crate::consensus::pow::PoWEngine;
    use crate::crypto::primitives::KeyPair;
    use crate::core::transaction::Transaction;
    use std::sync::Arc;

    #[test]
    fn test_chaos_network_partition_recovery() {
        let consensus_a = Arc::new(PoWEngine::new(0)); 
        let mut chain_a = Blockchain::new(consensus_a, None, 1337, None);

        let consensus_b = Arc::new(PoWEngine::new(0));
        let mut chain_b = Blockchain::new(consensus_b, None, 1337, None);

        assert_eq!(chain_a.chain.len(), 1);
        assert_eq!(chain_b.chain.len(), 1);

        for _ in 0..3 {
            chain_a.produce_block("producer_a".to_string());
        }

        for _ in 0..5 {
            chain_b.produce_block("producer_b".to_string());
        }

        assert_eq!(chain_a.chain.len(), 4);
        assert_eq!(chain_b.chain.len(), 6);

        let result = chain_a.try_reorg(chain_b.chain.clone());
        
        assert!(result.is_ok(), "Reorg should be successful: {:?}", result.err());
        assert!(result.unwrap(), "Should have performed reorg");
        assert_eq!(chain_a.chain.len(), 6, "Chain A should now be length 6");
        assert_eq!(chain_a.chain.last().unwrap().hash, chain_b.chain.last().unwrap().hash);
    }

    #[test]
    fn test_chaos_mempool_flood_stress() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        
        println!("Flooding mempool with 1000 transactions from 1000 senders...");
        for i in 0..1000 {
            let sender = KeyPair::generate().unwrap();
            let sender_pub = sender.public_key_hex();
            blockchain.state.add_balance(&sender_pub, 100);

            let mut tx = Transaction::new(sender_pub, format!("recipient_{}", i), 1, vec![]);
            tx.nonce = 0;
            tx.fee = 1;
            tx.sign(&sender);
            blockchain.add_transaction(tx).unwrap();
        }

        assert_eq!(blockchain.mempool.len(), 1000);

        blockchain.produce_block("miner".to_string());
        
        println!("Mempool size after block: {}", blockchain.mempool.len());
        assert_eq!(blockchain.mempool.len(), 0, "Mempool should be empty after processing all txs");
        assert_eq!(blockchain.chain.last().unwrap().transactions.len(), 1000);
    }

    #[test]
    fn test_chaos_reorg_depth_protection() {
        let pow_config = crate::consensus::PoWConfig {
            difficulty: 0,
            adjustment_interval: 10000,
            ..Default::default()
        };
        
        let consensus_a = Arc::new(PoWEngine::with_config(pow_config.clone()));
        let mut chain_a = Blockchain::new(consensus_a, None, 1337, None);

        let consensus_b = Arc::new(PoWEngine::with_config(pow_config));
        let mut chain_b = Blockchain::new(consensus_b, None, 1337, None);

        for _ in 0..(MAX_REORG_DEPTH + 10) {
            chain_a.produce_block("a".into());
        }

        for _ in 0..(MAX_REORG_DEPTH + 20) {
            chain_b.produce_block("b".into());
        }

        let result = chain_a.try_reorg(chain_b.chain.clone());
        println!("Reorg result: {:?}", result);
        
        assert!(result.is_err(), "Deep reorg should be rejected with Err");
        assert!(result.unwrap_err().contains("exceeds max"));
    }

    #[test]
    fn test_chaos_invalid_tx_rejection() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        
        let mut invalid_tx = Transaction::new("alice".into(), "bob".into(), 100, vec![]);
        invalid_tx.signature = Some(vec![0; 64]); 

        let result = blockchain.add_transaction(invalid_tx);
        assert!(result.is_err(), "Invalid signature should be rejected immediately");
    }
}
