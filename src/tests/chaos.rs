#[cfg(test)]
mod chaos_tests {
    use crate::chain::blockchain::{Blockchain, MAX_REORG_DEPTH};
    use crate::consensus::pow::PoWEngine;
    use crate::core::address::Address;
    use crate::core::transaction::Transaction;
    use crate::crypto::primitives::KeyPair;
    use std::sync::Arc;

    #[test]
    fn test_chaos_network_partition_recovery() {
        let consensus_a = Arc::new(PoWEngine::new(0));
        let mut chain_a = Blockchain::new(consensus_a, None, 1337, None);

        let consensus_b = Arc::new(PoWEngine::new(0));
        let mut chain_b = Blockchain::new(consensus_b, None, 1337, None);

        assert_eq!(chain_a.chain.len(), 1);
        assert_eq!(chain_b.chain.len(), 1);

        let producer_a = Address::from_hex(&"01".repeat(32)).unwrap();
        for _ in 0..3 {
            chain_a.produce_block(producer_a);
        }

        let producer_b = Address::from_hex(&"02".repeat(32)).unwrap();
        for _ in 0..5 {
            chain_b.produce_block(producer_b);
        }

        assert_eq!(chain_a.chain.len(), 4);
        assert_eq!(chain_b.chain.len(), 6);

        let result = chain_a.try_reorg(chain_b.chain.clone());

        assert!(
            result.is_ok(),
            "Reorg should be successful: {:?}",
            result.err()
        );
        assert!(result.unwrap(), "Should have performed reorg");
        assert_eq!(chain_a.chain.len(), 6, "Chain A should now be length 6");
        assert_eq!(
            chain_a.chain.last().unwrap().hash,
            chain_b.chain.last().unwrap().hash
        );
    }

    #[test]
    fn test_chaos_mempool_flood_stress() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        println!("Flooding mempool with 1000 transactions from 1000 senders...");
        for i in 0..1000 {
            let sender = KeyPair::generate().unwrap();
            let sender_pub = Address::from(sender.public_key_bytes());
            blockchain.state.add_balance(&sender_pub, 100);

            let mut recipient_bytes = [0u8; 32];
            recipient_bytes[0] = ((i % 250) + 1) as u8;
            let recipient = Address::from(recipient_bytes);
            let mut tx = Transaction::new(sender_pub, recipient, 1, vec![]);
            tx.nonce = 0;
            tx.fee = 1;
            tx.sign(&sender);
            blockchain.add_transaction(tx).unwrap();
        }

        assert_eq!(blockchain.mempool.len(), 1000);

        let miner = Address::from_hex(&"03".repeat(32)).unwrap();
        blockchain.produce_block(miner);

        println!("Mempool size after block: {}", blockchain.mempool.len());
        assert_eq!(
            blockchain.mempool.len(),
            0,
            "Mempool should be empty after processing all txs"
        );
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

        let producer_a = Address::from_hex(&"01".repeat(32)).unwrap();
        for _ in 0..(MAX_REORG_DEPTH + 10) {
            chain_a.produce_block(producer_a);
        }

        let producer_b = Address::from_hex(&"02".repeat(32)).unwrap();
        for _ in 0..(MAX_REORG_DEPTH + 20) {
            chain_b.produce_block(producer_b);
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

        let alice = Address::from_hex(&"01".repeat(32)).unwrap();
        let bob = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut invalid_tx = Transaction::new(alice, bob, 100, vec![]);
        invalid_tx.signature = Some(vec![0; 64]);

        let result = blockchain.add_transaction(invalid_tx);
        assert!(
            result.is_err(),
            "Invalid signature should be rejected immediately"
        );
    }

    #[test]
    fn test_chaos_cross_domain_replay_and_tamper_resistance() {
        use crate::core::hash::hash_fields_bytes;
        use crate::cross_domain::BridgeState;

        let mut bridge = BridgeState::new();
        let owner = Address::from([7u8; 32]);
        let recipient = Address::from([8u8; 32]);

        for i in 0..64u8 {
            let asset = hash_fields_bytes(&[b"asset", &[i]]);
            bridge.register_asset(asset, 1).unwrap();

            let (_transfer, event) = bridge
                .lock(
                    1,
                    2,
                    100 + i as u64,
                    i as u32,
                    asset,
                    owner,
                    recipient,
                    10,
                    10_000,
                )
                .unwrap();
            let message = event.message.expect("lock event should carry message");

            let mut tampered = message.clone();
            tampered.target_domain = 3;
            assert!(
                bridge.mint(&tampered).is_err(),
                "tampered message must not mint"
            );

            bridge.mint(&message).unwrap();
            assert!(
                bridge.mint(&message).is_err(),
                "replayed message must not mint twice"
            );
        }
    }

    #[test]
    fn test_chaos_domain_event_proof_rejects_wrong_root_and_index() {
        use crate::core::hash::hash_fields_bytes;
        use crate::cross_domain::message::CrossDomainMessageParams;
        use crate::cross_domain::{
            CrossDomainMessage, DomainEvent, DomainEventKind, DomainEventTree, MessageKind,
        };

        let owner = Address::from([9u8; 32]);
        let recipient = Address::from([10u8; 32]);
        let mut tree = DomainEventTree::new();

        for i in 0..32u32 {
            let payload_hash = hash_fields_bytes(&[b"payload", &i.to_le_bytes()]);
            let message = CrossDomainMessage::new(CrossDomainMessageParams {
                source_domain: 1,
                target_domain: 2,
                source_height: 500,
                event_index: i,
                nonce: i as u64,
                sender: owner,
                recipient,
                payload_hash,
                kind: MessageKind::BridgeLock,
                expiry_height: 2_000,
            });
            tree.push(DomainEvent {
                domain_id: 1,
                domain_height: 500,
                event_index: i,
                kind: DomainEventKind::BridgeLocked,
                emitter: owner,
                message: Some(message),
                payload_hash,
            });
        }

        let root = tree.root();
        let proof = tree.proof(17).unwrap();
        assert!(proof.verify(root));

        let mut wrong_index = proof.clone();
        wrong_index.index = 18;
        assert!(!wrong_index.verify(root));
        assert!(!proof.verify(hash_fields_bytes(&[b"foreign root"])));
    }

    #[test]
    fn test_chaos_global_header_tracks_domain_commitment_root() {
        use crate::core::block::Block;
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        let domain = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);

        blockchain
            .register_consensus_domain(domain.clone())
            .unwrap();
        let before = blockchain.build_global_header(None);

        let mut block = Block::new(1, blockchain.last_block().hash.clone(), vec![]);
        block.state_root = "11".repeat(32);
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();

        let commitment =
            DomainCommitment::from_block(&domain, &block, [2u8; 32], [3u8; 32], 0).unwrap();
        blockchain
            .submit_domain_commitment(commitment.clone())
            .unwrap();
        assert!(blockchain.submit_domain_commitment(commitment).is_err());

        let after = blockchain.build_global_header(None);
        assert_ne!(
            before.domain_commitment_root, after.domain_commitment_root,
            "domain commitment root must change after a commitment is accepted"
        );
        assert_eq!(
            after.domain_registry_root,
            blockchain.domain_registry.root(),
            "global header must bind the active domain registry root"
        );
    }

    #[test]
    fn test_chaos_settlement_state_survives_storage_reload() {
        use crate::core::block::Block;
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};
        use crate::storage::db::Storage;

        let tempdir = tempfile::tempdir().unwrap();
        let db_path = tempdir.path().join("settlement-state");
        let db_path = db_path.to_str().unwrap();

        {
            let storage = Storage::new(db_path).unwrap();
            let consensus = Arc::new(PoWEngine::new(0));
            let mut blockchain = Blockchain::new(consensus, Some(storage), 1337, None);
            let domain = default_domain(7, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
            blockchain
                .register_consensus_domain(domain.clone())
                .unwrap();

            let mut block = Block::new(1, blockchain.last_block().hash.clone(), vec![]);
            block.state_root = "22".repeat(32);
            block.tx_root = block.calculate_tx_root();
            block.hash = block.calculate_hash();

            let commitment =
                DomainCommitment::from_block(&domain, &block, [4u8; 32], [5u8; 32], 0).unwrap();
            blockchain.submit_domain_commitment(commitment).unwrap();
            blockchain.seal_global_header(None).unwrap();
        }

        let storage = Storage::new(db_path).unwrap();
        let consensus = Arc::new(PoWEngine::new(0));
        let blockchain = Blockchain::new(consensus, Some(storage), 1337, None);

        assert!(blockchain.domain_registry.get(7).is_some());
        assert_eq!(blockchain.global_headers.len(), 1);
        assert_eq!(blockchain.domain_commitment_registry.len(), 1);
    }

    #[test]
    fn test_chaos_mixed_consensus_rejects_unknown_duplicate_and_wrong_kind_commitments() {
        use crate::core::block::Block;
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
        let poa = default_domain(3, ConsensusKind::PoA, 1339, "poa-authority-quorum", 0);

        for domain in [poa.clone(), pow.clone(), pos.clone()] {
            blockchain.register_consensus_domain(domain).unwrap();
        }

        let make_commitment = |domain: &crate::domain::ConsensusDomain, height: u64, seed: u8| {
            let mut block = Block::new(height, "bb".repeat(32), vec![]);
            block.state_root = format!("{:02x}", seed).repeat(32);
            block.tx_root = block.calculate_tx_root();
            block.hash = block.calculate_hash();
            DomainCommitment::from_block(domain, &block, [seed; 32], [seed + 1; 32], 0).unwrap()
        };

        let pow_commitment = make_commitment(&pow, 100, 1);
        blockchain
            .submit_domain_commitment(pow_commitment.clone())
            .unwrap();
        assert!(blockchain.submit_domain_commitment(pow_commitment).is_err());

        let mut unknown = make_commitment(&pos, 101, 2);
        unknown.domain_id = 99;
        assert!(blockchain.submit_domain_commitment(unknown).is_err());

        let mut wrong_kind = make_commitment(&poa, 102, 3);
        wrong_kind.consensus_kind = ConsensusKind::PoW;
        assert!(blockchain.submit_domain_commitment(wrong_kind).is_err());

        blockchain
            .submit_domain_commitment(make_commitment(&pos, 103, 4))
            .unwrap();
        blockchain
            .submit_domain_commitment(make_commitment(&poa, 104, 5))
            .unwrap();
        assert_eq!(blockchain.domain_commitment_registry.len(), 3);
    }

    #[test]
    fn test_chaos_global_roots_are_stable_for_same_multiconsensus_inputs() {
        use crate::core::block::Block;
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        fn fill(order: &[u32]) -> Blockchain {
            let consensus = Arc::new(PoWEngine::new(0));
            let mut blockchain = Blockchain::new(consensus, None, 1337, None);
            let domains = [
                default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0),
                default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0),
                default_domain(3, ConsensusKind::PoA, 1339, "poa-authority-quorum", 0),
            ];

            for id in order {
                let domain = domains.iter().find(|domain| domain.id == *id).unwrap();
                blockchain
                    .register_consensus_domain(domain.clone())
                    .unwrap();
            }

            for domain in domains {
                let mut block = Block::new(domain.id as u64, "cc".repeat(32), vec![]);
                block.timestamp = 1_000 + domain.id as u128;
                block.state_root = format!("{:02x}", domain.id).repeat(32);
                block.tx_root = block.calculate_tx_root();
                block.hash = block.calculate_hash();
                let commitment = DomainCommitment::from_block(
                    &domain,
                    &block,
                    [domain.id as u8; 32],
                    [domain.id as u8 + 10; 32],
                    0,
                )
                .unwrap();
                blockchain.submit_domain_commitment(commitment).unwrap();
            }
            blockchain
        }

        let a = fill(&[1, 2, 3]);
        let b = fill(&[3, 1, 2]);

        assert_eq!(a.domain_registry.root(), b.domain_registry.root());
        assert_eq!(
            a.build_global_header(None).domain_commitment_root,
            b.build_global_header(None).domain_commitment_root
        );
    }

    #[test]
    fn test_chaos_event_proof_rejects_foreign_domain_and_payload_tamper() {
        use crate::core::hash::hash_fields_bytes;
        use crate::cross_domain::message::CrossDomainMessageParams;
        use crate::cross_domain::{
            CrossDomainMessage, DomainEvent, DomainEventKind, DomainEventTree, MessageKind,
        };
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let owner = Address::from([31u8; 32]);
        let recipient = Address::from([32u8; 32]);
        let payload_hash = hash_fields_bytes(&[b"payload"]);
        let message = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: pow.id,
            target_domain: 2,
            source_height: 77,
            event_index: 0,
            nonce: 0,
            sender: owner,
            recipient,
            payload_hash,
            kind: MessageKind::BridgeLock,
            expiry_height: 999,
        });
        let event = DomainEvent {
            domain_id: pow.id,
            domain_height: 77,
            event_index: 0,
            kind: DomainEventKind::BridgeLocked,
            emitter: owner,
            message: Some(message),
            payload_hash,
        };

        let mut tree = DomainEventTree::new();
        tree.push(event.clone());
        let mut block = crate::core::block::Block::new(77, "dd".repeat(32), vec![]);
        block.state_root = "44".repeat(32);
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();
        let mut commitment =
            DomainCommitment::from_block(&pow, &block, tree.root(), [9u8; 32], 0).unwrap();
        commitment.event_root = tree.root();
        blockchain.submit_domain_commitment(commitment).unwrap();

        let proof = tree.proof(0).unwrap();
        assert!(blockchain
            .verify_domain_event_proof(pow.id, 77, 0, None, event.clone(), &proof)
            .is_ok());

        let mut foreign_domain = event.clone();
        foreign_domain.domain_id = 99;
        assert!(blockchain
            .verify_domain_event_proof(pow.id, 77, 0, None, foreign_domain, &proof)
            .is_err());

        let mut payload_tampered = event;
        payload_tampered.payload_hash = hash_fields_bytes(&[b"tampered"]);
        assert!(blockchain
            .verify_domain_event_proof(pow.id, 77, 0, None, payload_tampered, &proof)
            .is_err());
    }

    #[test]
    fn test_chaos_verified_submission_rejects_wrong_proof_type_for_each_domain() {
        use crate::domain::finality_adapter::{hash_finality_proof, FinalityProof};
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        let poa = default_domain(2, ConsensusKind::PoA, 1338, "poa-authority-quorum", 0);
        blockchain.register_consensus_domain(pow.clone()).unwrap();
        blockchain.register_consensus_domain(poa.clone()).unwrap();

        let make_commitment = |domain: &crate::domain::ConsensusDomain, proof: &FinalityProof| {
            let mut block = crate::core::block::Block::new(1, "ee".repeat(32), vec![]);
            block.state_root = "55".repeat(32);
            block.tx_root = block.calculate_tx_root();
            block.hash = block.calculate_hash();
            let mut commitment =
                DomainCommitment::from_block(domain, &block, [1u8; 32], [2u8; 32], 0).unwrap();
            commitment.finality_proof_hash = hash_finality_proof(proof);
            commitment
        };

        let wrong_for_pow = FinalityProof::PoA {
            signer_count: 10,
            validator_count: 10,
        };
        assert!(blockchain
            .submit_verified_domain_commitment(make_commitment(&pow, &wrong_for_pow), wrong_for_pow)
            .is_err());

        let wrong_for_poa = FinalityProof::PoW {
            confirmations: 100,
            total_work_hint: 1000,
        };
        assert!(blockchain
            .submit_verified_domain_commitment(make_commitment(&poa, &wrong_for_poa), wrong_for_poa)
            .is_err());
    }

    #[test]
    fn test_chaos_many_global_headers_preserve_hash_chain_under_root_changes() {
        use crate::core::hash::hash_fields_bytes;
        use crate::cross_domain::BridgeState;

        let consensus = Arc::new(PoWEngine::new(0));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        let mut bridge = BridgeState::new();

        let mut previous_hash = [0u8; 32];
        for round in 0..16u8 {
            let asset = hash_fields_bytes(&[b"asset", &[round]]);
            let owner = Address::from([round.saturating_add(1); 32]);
            let recipient = Address::from([round.saturating_add(33); 32]);
            bridge.register_asset(asset, 1).unwrap();
            let (_transfer, _event) = bridge
                .lock(1, 2, round as u64, 0, asset, owner, recipient, 1, 1000)
                .unwrap();
            blockchain.bridge_state = bridge.clone();

            let header = blockchain.seal_global_header(None).unwrap();
            assert_eq!(header.global_height, round as u64);
            assert_eq!(header.previous_global_hash, previous_hash);
            previous_hash = header.calculate_hash_bytes();
        }

        assert_eq!(blockchain.global_headers.len(), 16);
    }

    #[tokio::test]
    async fn test_chaos_multi_consensus_concurrent_validation_and_settlement() {
        use crate::core::block::Block;
        use crate::cross_domain::message::{CrossDomainMessage, CrossDomainMessageParams, MessageKind};
        use crate::domain::finality_adapter::{hash_finality_proof, FinalityProof};
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        let consensus_settlement = Arc::new(PoWEngine::new(0));
        let mut settlement_node = Blockchain::new(consensus_settlement, None, 1337, None);

        let pow_domain = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        let pos_domain = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
        let poa_domain = default_domain(3, ConsensusKind::PoA, 1339, "poa-authority-quorum", 0);

        settlement_node.register_consensus_domain(pow_domain.clone()).unwrap();
        settlement_node.register_consensus_domain(pos_domain.clone()).unwrap();
        settlement_node.register_consensus_domain(poa_domain.clone()).unwrap();

        let mut commitments_to_submit = Vec::new();
        let mut messages_to_submit = Vec::new();

        let num_blocks = 50;

        for i in 1..=num_blocks {
            let mut block_pow = Block::new(i, "pow".repeat(32), vec![]);
            block_pow.state_root = format!("pow_state_{}", i).repeat(32)[0..64].to_string();
            block_pow.tx_root = block_pow.calculate_tx_root();
            block_pow.hash = block_pow.calculate_hash();
            let mut pow_com = DomainCommitment::from_block(&pow_domain, &block_pow, [1u8; 32], [2u8; 32], i as u64).unwrap();
            let pow_proof = FinalityProof::PoW { confirmations: 10, total_work_hint: 1000 + (i as u128) };
            pow_com.finality_proof_hash = hash_finality_proof(&pow_proof);
            commitments_to_submit.push((pow_com, pow_proof));

            let mut block_pos = Block::new(i, "pos".repeat(32), vec![]);
            block_pos.state_root = format!("pos_state_{}", i).repeat(32)[0..64].to_string();
            block_pos.tx_root = block_pos.calculate_tx_root();
            block_pos.hash = block_pos.calculate_hash();
            let mut pos_com = DomainCommitment::from_block(&pos_domain, &block_pos, [3u8; 32], [4u8; 32], i as u64).unwrap();
            use crate::chain::finality::{FinalityCert, ValidatorSetSnapshot};
            let pos_proof = FinalityProof::PoS {
                cert: FinalityCert {
                    epoch: i as u64,
                    checkpoint_height: i as u64,
                    checkpoint_hash: format!("pos_hash_{}", i),
                    agg_sig_bls: vec![0u8; 48],
                    bitmap: vec![255],
                    set_hash: "snap_hash".to_string(),
                },
                validator_snapshot: ValidatorSetSnapshot {
                    epoch: i as u64,
                    validators: vec![],
                    set_hash: "snap_hash".to_string(),
                    total_stake: 100,
                },
            };
            pos_com.finality_proof_hash = hash_finality_proof(&pos_proof);
            commitments_to_submit.push((pos_com, pos_proof));

            let mut block_poa = Block::new(i, "poa".repeat(32), vec![]);
            block_poa.state_root = format!("poa_state_{}", i).repeat(32)[0..64].to_string();
            block_poa.tx_root = block_poa.calculate_tx_root();
            block_poa.hash = block_poa.calculate_hash();
            let mut poa_com = DomainCommitment::from_block(&poa_domain, &block_poa, [5u8; 32], [6u8; 32], i as u64).unwrap();
            let poa_proof = FinalityProof::PoA { signer_count: 5, validator_count: 7 };
            poa_com.finality_proof_hash = hash_finality_proof(&poa_proof);
            commitments_to_submit.push((poa_com, poa_proof));

            let msg = CrossDomainMessage::new(CrossDomainMessageParams {
                source_domain: 1,
                target_domain: 2,
                source_height: i,
                event_index: 0,
                nonce: i,
                sender: Address::from([7u8; 32]),
                recipient: Address::from([8u8; 32]),
                payload_hash: [i as u8; 32],
                kind: MessageKind::BridgeLock,
                expiry_height: 1000 + i,
            });
            messages_to_submit.push(msg);
        }

        for (com, _proof) in commitments_to_submit {
            assert!(settlement_node.submit_domain_commitment(com.clone()).is_ok());
            assert!(settlement_node.submit_domain_commitment(com).is_err());
        }

        for msg in messages_to_submit {
            assert!(settlement_node.message_registry.insert(msg).is_ok());
        }


        let global_header = settlement_node.seal_global_header(None).unwrap();

        assert_eq!(global_header.global_height, 0);
        assert_eq!(settlement_node.domain_commitment_registry.len(), (num_blocks * 3) as usize);
        assert_eq!(settlement_node.message_registry.len(), num_blocks as usize);
        
        let global_header_2 = settlement_node.seal_global_header(None).unwrap();
        assert_eq!(global_header_2.global_height, 1);
        assert_eq!(global_header_2.previous_global_hash, global_header.calculate_hash_bytes());
    }

    #[tokio::test]
    async fn test_chaos_end_to_end_cross_domain_concurrent_transfers() {
        use crate::core::block::Block;
        use crate::core::hash::hash_fields_bytes;
        use crate::cross_domain::{DomainEventTree, CrossDomainMessageRegistry};
        use crate::domain::plugin::default_domain;
        use crate::domain::{ConsensusKind, DomainCommitment};

        let consensus_settlement = Arc::new(PoWEngine::new(0));
        let mut settlement_node = Blockchain::new(consensus_settlement, None, 1337, None);

        let pos_domain = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
        let poa_domain = default_domain(3, ConsensusKind::PoA, 1339, "poa-authority-quorum", 0);

        settlement_node.register_consensus_domain(pos_domain.clone()).unwrap();
        settlement_node.register_consensus_domain(poa_domain.clone()).unwrap();
        let num_transfers = 100;
        let mut pos_event_tree = DomainEventTree::new();
        let mut transfers_data = Vec::new();

        for i in 1..=num_transfers {
            let asset = hash_fields_bytes(&[b"test-asset", &(i as u64).to_le_bytes()]);
            settlement_node.bridge_state.register_asset(asset, pos_domain.id).unwrap();

            let sender = Address::from([i as u8; 32]);
            let recipient = Address::from([(i + 1) as u8; 32]);
            
            let (transfer, lock_event) = settlement_node.bridge_state
                .lock(pos_domain.id, poa_domain.id, 1, (i - 1) as u32, asset, sender, recipient, 100, 50000)
                .unwrap();
            
            pos_event_tree.push(lock_event.clone());
            transfers_data.push((transfer, lock_event));
        }

        let mut pos_block = Block::new(1, "pos_prev".repeat(32), vec![]);
        pos_block.state_root = "pos_state_root".repeat(32)[0..64].to_string();
        pos_block.tx_root = pos_block.calculate_tx_root();
        pos_block.hash = pos_block.calculate_hash();
        
        let mut pos_com = DomainCommitment::from_block(&pos_domain, &pos_block, [2u8; 32], [3u8; 32], 1).unwrap();
        pos_com.event_root = pos_event_tree.root();

        settlement_node.submit_domain_commitment(pos_com).unwrap();

        for (_transfer, event) in &transfers_data {
            let msg = event.message.clone().unwrap();
            settlement_node.message_registry.insert(msg).unwrap();
        }

        let global_header = settlement_node.seal_global_header(None).unwrap();
        assert_eq!(global_header.domain_commitment_root, settlement_node.domain_commitment_registry.root());
        assert_eq!(global_header.message_root, settlement_node.message_registry.root());

        for (i, (transfer, lock_event)) in transfers_data.iter().enumerate() {
            let proof = pos_event_tree.proof(i).unwrap();
            
            let mint_result = settlement_node.mint_bridge_transfer_from_verified_event(
                pos_domain.id, 1, 1, None, lock_event.clone(), &proof
            );
            assert!(mint_result.is_ok(), "Mint failed for transfer {}: {:?}", i, mint_result.unwrap_err());

            let duplicate_mint_result = settlement_node.mint_bridge_transfer_from_verified_event(
                pos_domain.id, 1, 1, None, lock_event.clone(), &proof
            );
            assert!(duplicate_mint_result.is_err(), "Replay attack should be prevented for transfer {}", i);
        }

        let final_header = settlement_node.seal_global_header(None).unwrap();
        assert_ne!(final_header.bridge_state_root, [0u8; 32]);
    }
}
