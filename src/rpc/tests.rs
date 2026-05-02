#[cfg(test)]
mod tests {
    use crate::chain::blockchain::Blockchain;
    use crate::chain::chain_actor::{ChainActor, ChainHandle};
    use crate::consensus::pow::PoWEngine;
    use crate::core::address::Address;
    use crate::core::transaction::Transaction;
    use crate::network::node::Node;
    use crate::rpc::api::BudlumApiServer;
    use crate::rpc::server::RpcServer;
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
        let hex_genesis_hash = if genesis_hash.starts_with("0x") {
            genesis_hash
        } else {
            format!("0x{}", genesis_hash)
        };

        let block_by_hash = server
            .get_block_by_hash(hex_genesis_hash.clone())
            .await
            .unwrap();
        println!(
            "bud_getBlockByHash: {}",
            serde_json::to_string_pretty(&block_by_hash).unwrap()
        );
        assert_eq!(block_by_hash["hash"], hex_genesis_hash);
        assert!(block_by_hash["parentHash"]
            .as_str()
            .unwrap()
            .starts_with("0x"));

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

        let retrieved_tx = server
            .get_transaction_by_hash(hex_tx_hash.clone())
            .await
            .unwrap();
        println!(
            "bud_getTransactionByHash: {}",
            serde_json::to_string_pretty(&retrieved_tx).unwrap()
        );
        assert_eq!(retrieved_tx["hash"], hex_tx_hash);
        assert!(retrieved_tx["signature"]
            .as_str()
            .unwrap()
            .starts_with("0x"));

        let receipt = server
            .get_transaction_receipt(hex_tx_hash.clone())
            .await
            .unwrap();
        error_to_json_result(server.get_transaction_receipt(hex_tx_hash.clone()).await);
        println!(
            "bud_getTransactionReceipt (pending): {}",
            serde_json::to_string_pretty(&receipt).unwrap()
        );

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
        println!(
            "bud_txPrecheck (no sig): {}",
            serde_json::to_string_pretty(&precheck).unwrap()
        );
        assert_eq!(precheck["accepted"], false);
        assert!(precheck["reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r == "invalid_signature"));

        bc.add_balance(&from, 1000).await;

        let precheck2 = server.tx_precheck(tx.clone()).await.unwrap();
        assert_eq!(precheck2["accepted"], false);

        tx.sign(&keypair);
        let precheck3 = server.tx_precheck(tx).await.unwrap();
        println!(
            "bud_txPrecheck (with sig): {}",
            serde_json::to_string_pretty(&precheck3).unwrap()
        );
        assert_eq!(precheck3["accepted"], true);
    }

    #[tokio::test]
    async fn test_rpc_settlement_methods() {
        let (server, chain) = setup().await;
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

        let mut block = crate::core::block::Block::new(1, "aa".repeat(32), vec![]);
        block.timestamp = 1234;
        block.state_root = "11".repeat(32);
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();
        let commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block, [2u8; 32], [3u8; 32], 0)
                .unwrap();
        chain
            .submit_domain_commitment(commitment.clone())
            .await
            .unwrap();
        let sealed = chain.seal_global_header().await.unwrap();

        let info = server.get_settlement_info().await.unwrap();
        assert_eq!(info["globalHeight"], 1);
        assert_eq!(info["domainCommitmentCount"], 1);
        assert!(info["latestGlobalHash"].as_str().unwrap().len() == 64);

        let header = server.get_global_header(0).await.unwrap();
        assert_eq!(header["globalHeight"], "0x0");
        assert_eq!(
            header["hash"].as_str().unwrap(),
            format!("0x{}", sealed.calculate_hash())
        );

        let missing = server.get_global_header(999).await.unwrap();
        assert!(missing.is_null());

        let commitments = server.get_domain_commitments().await.unwrap();
        let commitments = commitments.as_array().unwrap();
        assert_eq!(commitments.len(), 1);
        assert_eq!(commitments[0]["domainId"], 1);
        assert_eq!(
            commitments[0]["domainBlockHash"],
            format!("0x{}", hex::encode(commitment.domain_block_hash))
        );

        let domains = server.get_consensus_domains().await.unwrap();
        assert_eq!(domains.as_array().unwrap().len(), 1);
        assert_eq!(domains[0]["domainId"], 1);

        let poa_domain = crate::domain::plugin::default_domain(
            2,
            crate::domain::ConsensusKind::PoA,
            1338,
            "poa-authority-quorum",
            0,
        );
        let registration = server
            .register_consensus_domain(poa_domain.clone())
            .await
            .unwrap();
        assert_eq!(registration["domainId"], 2);
        assert!(registration["domainRegistryRoot"]
            .as_str()
            .unwrap()
            .starts_with("0x"));
        assert!(server.register_consensus_domain(poa_domain).await.is_err());
        assert_eq!(
            server
                .get_consensus_domains()
                .await
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            2
        );

        let mut block2 = block.clone();
        block2.index = 2;
        let raw_commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block2, [4u8; 32], [5u8; 32], 1)
                .unwrap();
        let raw_err = server
            .submit_domain_commitment(raw_commitment.clone())
            .await
            .unwrap_err();
        assert!(raw_err
            .message()
            .contains("Raw domain commitment submission is disabled"));

        let raw_rejected_commitments = server.get_domain_commitments().await.unwrap();
        assert_eq!(raw_rejected_commitments.as_array().unwrap().len(), 1);

        let proof2 = crate::domain::FinalityProof::PoW {
            confirmations: 64,
            total_work_hint: 4000,
        };
        let new_commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block2, [4u8; 32], [5u8; 32], 1)
                .unwrap();
        let mut new_commitment = new_commitment;
        new_commitment.finality_proof_hash = crate::domain::hash_finality_proof(&proof2);
        let result = server
            .submit_verified_domain_commitment(crate::domain::VerifiedDomainCommitment {
                commitment: new_commitment.clone(),
                proof: proof2,
            })
            .await
            .unwrap();
        assert_eq!(
            result,
            format!("0x{}", hex::encode(new_commitment.leaf_hash()))
        );

        let commitments2 = server.get_domain_commitments().await.unwrap();
        assert_eq!(commitments2.as_array().unwrap().len(), 2);

        let mut block3 = block.clone();
        block3.index = 3;
        let proof = crate::domain::FinalityProof::PoW {
            confirmations: 64,
            total_work_hint: 5000,
        };
        let mut verified_commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block3, [6u8; 32], [7u8; 32], 2)
                .unwrap();
        verified_commitment.finality_proof_hash = crate::domain::hash_finality_proof(&proof);
        let verified_payload = crate::domain::VerifiedDomainCommitment {
            commitment: verified_commitment.clone(),
            proof,
        };
        let verified_result = server
            .submit_verified_domain_commitment(verified_payload)
            .await
            .unwrap();
        assert_eq!(
            verified_result,
            format!("0x{}", hex::encode(verified_commitment.leaf_hash()))
        );

        let mut block4 = block.clone();
        block4.index = 4;
        let weak_proof = crate::domain::FinalityProof::PoW {
            confirmations: 1,
            total_work_hint: 5001,
        };
        let mut weak_commitment =
            crate::domain::DomainCommitment::from_block(&domain, &block4, [8u8; 32], [9u8; 32], 3)
                .unwrap();
        weak_commitment.finality_proof_hash = crate::domain::hash_finality_proof(&weak_proof);
        let weak_payload = crate::domain::VerifiedDomainCommitment {
            commitment: weak_commitment,
            proof: weak_proof,
        };
        assert!(server
            .submit_verified_domain_commitment(weak_payload)
            .await
            .is_err());

        let commitments3 = server.get_domain_commitments().await.unwrap();
        assert_eq!(commitments3.as_array().unwrap().len(), 3);

        let cross_domain_msg = crate::cross_domain::CrossDomainMessage::new(
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

        let msg_result = server
            .submit_cross_domain_message(cross_domain_msg.clone())
            .await
            .unwrap();
        assert_eq!(
            msg_result,
            format!("0x{}", hex::encode(cross_domain_msg.message_id))
        );

        let asset_id = [42u8; 32];
        let bridge_registration = server.register_bridge_asset(asset_id, 1).await.unwrap();
        assert_eq!(bridge_registration["status"], "registered");

        let owner = Address::from([11u8; 32]);
        let recipient = Address::from([12u8; 32]);
        let lock_result = server
            .lock_bridge_transfer(1, 2, 20, 0, asset_id, owner, recipient, 100, 1000)
            .await
            .unwrap();
        let lock_event: crate::cross_domain::DomainEvent =
            serde_json::from_value(lock_result["event"].clone()).unwrap();
        let message_id = lock_event.message.as_ref().unwrap().message_id;

        let mut event_tree = crate::cross_domain::DomainEventTree::new();
        event_tree.push(lock_event.clone());

        let mut bridge_block = block.clone();
        bridge_block.index = 20;
        let bridge_proof = crate::domain::FinalityProof::PoW {
            confirmations: 64,
            total_work_hint: 6000,
        };
        let mut bridge_commitment = crate::domain::DomainCommitment::from_block(
            &domain,
            &bridge_block,
            event_tree.root(),
            [0u8; 32],
            4,
        )
        .unwrap();
        bridge_commitment.finality_proof_hash = crate::domain::hash_finality_proof(&bridge_proof);
        server
            .submit_verified_domain_commitment(crate::domain::VerifiedDomainCommitment {
                commitment: bridge_commitment,
                proof: bridge_proof,
            })
            .await
            .unwrap();

        let mint_result = server
            .mint_bridge_transfer(1, 20, 4, None, lock_event, event_tree.proof(0).unwrap())
            .await
            .unwrap();
        assert_eq!(mint_result["status"], "minted");

        let burn_result = server.burn_bridge_transfer(message_id, 2).await.unwrap();
        assert_eq!(burn_result["status"], "burned");

        let unlock_result = server.unlock_bridge_transfer(message_id, 1).await.unwrap();
        assert_eq!(unlock_result["status"], "unlocked");

        let rpc_sealed = server.seal_global_header().await.unwrap();
        assert_eq!(rpc_sealed["globalHeight"], "0x1");
        assert!(rpc_sealed["domainRegistryRoot"]
            .as_str()
            .unwrap()
            .starts_with("0x"));
    }
}
