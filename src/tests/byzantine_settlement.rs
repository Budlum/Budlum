#[cfg(test)]
mod byzantine_settlement_tests {
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::pow::PoWEngine;
    use crate::core::address::Address;
    use crate::core::block::Block;
    use crate::domain::plugin::default_domain;
    use crate::domain::{ConsensusKind, DomainCommitment, DomainStatus};
    use crate::domain::finality_adapter::{hash_finality_proof, FinalityProof};
    use crate::chain::finality::{FinalityCert, ValidatorSetSnapshot};
    use std::sync::Arc;
    use tokio::sync::RwLock;

    #[tokio::test]
    async fn test_multi_consensus_settlement_determinism_and_invalid_commitment_rejection() {
        let make_node = || {
            let consensus = Arc::new(PoWEngine::new(0));
            let mut node = Blockchain::new(consensus, None, 1337, None);
            
            let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
            let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
            let poa = default_domain(3, ConsensusKind::PoA, 1339, "poa-authority-quorum", 0);
            
            node.register_consensus_domain(pow).unwrap();
            node.register_consensus_domain(pos).unwrap();
            node.register_consensus_domain(poa).unwrap();
            node
        };

        let mut node_a = make_node();
        let mut node_b = make_node();

        let pow_domain = node_a.domain_registry.get(1).unwrap().clone();
        let pos_domain = node_a.domain_registry.get(2).unwrap().clone();
        let poa_domain = node_a.domain_registry.get(3).unwrap().clone();

        let mut pow_commitments = Vec::new();
        let mut pos_commitments = Vec::new();
        let mut poa_commitments = Vec::new();

        for i in 1..=100 {
            let mut b_pow = Block::new(i, "pow".repeat(32), vec![]);
            b_pow.state_root = format!("pow_state_{}", i).repeat(32)[0..64].to_string();
            b_pow.tx_root = b_pow.calculate_tx_root();
            b_pow.hash = b_pow.calculate_hash();
            let mut com_pow = DomainCommitment::from_block(&pow_domain, &b_pow, [i as u8; 32], [0u8; 32], i).unwrap();
            let proof_pow = FinalityProof::PoW { confirmations: 10, total_work_hint: 1000 + i as u128 };
            com_pow.finality_proof_hash = hash_finality_proof(&proof_pow);
            pow_commitments.push((com_pow, proof_pow));

            let mut b_pos = Block::new(i, "pos".repeat(32), vec![]);
            b_pos.state_root = format!("pos_state_{}", i).repeat(32)[0..64].to_string();
            b_pos.tx_root = b_pos.calculate_tx_root();
            b_pos.hash = b_pos.calculate_hash();
            let mut com_pos = DomainCommitment::from_block(&pos_domain, &b_pos, [i as u8; 32], [0u8; 32], i).unwrap();
            let proof_pos = FinalityProof::PoS {
                cert: FinalityCert {
                    epoch: 1, checkpoint_height: i, checkpoint_hash: b_pos.hash.clone(),
                    agg_sig_bls: vec![0u8; 48], bitmap: vec![1], set_hash: "set".to_string(),
                },
                validator_snapshot: ValidatorSetSnapshot {
                    epoch: 1, validators: vec![], set_hash: "set".to_string(), total_stake: 100,
                },
            };
            com_pos.finality_proof_hash = hash_finality_proof(&proof_pos);
            pos_commitments.push((com_pos, proof_pos));

            let mut b_poa = Block::new(i, "poa".repeat(32), vec![]);
            b_poa.state_root = format!("poa_state_{}", i).repeat(32)[0..64].to_string();
            b_poa.tx_root = b_poa.calculate_tx_root();
            b_poa.hash = b_poa.calculate_hash();
            let mut com_poa = DomainCommitment::from_block(&poa_domain, &b_poa, [i as u8; 32], [0u8; 32], i).unwrap();
            let proof_poa = FinalityProof::PoA { signer_count: 5, validator_count: 5 };
            com_poa.finality_proof_hash = hash_finality_proof(&proof_poa);
            poa_commitments.push((com_poa, proof_poa));
        }

        let mut all_commitments = Vec::new();
        all_commitments.extend(pow_commitments.clone());
        all_commitments.extend(pos_commitments.clone());
        all_commitments.extend(poa_commitments.clone());

        for (com, _proof) in all_commitments.iter() {
            node_a.submit_domain_commitment(com.clone()).unwrap();
        }
        let root_a = node_a.build_global_header(None).domain_commitment_root;

        let mut all_commitments_shuffled = all_commitments.clone();
        all_commitments_shuffled.reverse(); 
        for (com, _proof) in all_commitments_shuffled.iter() {
            node_b.submit_domain_commitment(com.clone()).unwrap();
        }
        let root_b = node_b.build_global_header(None).domain_commitment_root;

        assert_eq!(root_a, root_b, "Global root must be order-independent");

        let (mut fake_com, proof) = pow_commitments[0].clone();
        fake_com.state_root = [0xFFu8; 32];
        fake_com.sequence = 9999; 
        assert!(node_a.submit_verified_domain_commitment(fake_com, proof).is_err(), "Invalid state root in proof must be rejected");

        let header = node_a.seal_global_header(None).unwrap();
        let (mut rollback_com, _proof) = pow_commitments[0].clone();
        rollback_com.sequence = 0; 
        assert!(node_a.submit_domain_commitment(rollback_com).is_err(), "Cannot rollback or duplicate finalized height");
        
        let header_after = node_a.seal_global_header(None).unwrap();
        assert_eq!(header.calculate_hash_bytes(), header_after.previous_global_hash, "Global hash chain must be stable");

        let poa_domain_id = poa_domain.id;
        node_a.domain_registry.set_status(poa_domain_id, DomainStatus::Frozen).unwrap(); 
        let mut com_poa_new = poa_commitments[0].0.clone();
        com_poa_new.domain_height = 999;
        com_poa_new.sequence = 999;
        assert!(node_a.submit_domain_commitment(com_poa_new).is_err(), "Frozen domain should not accept commitments");
        
        assert!(node_a.submit_domain_commitment(pow_commitments[0].0.clone()).is_err(), "Already committed PoW still exists and protected");
    }

    #[tokio::test]
    async fn test_cross_domain_double_spend_protection() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut node = Blockchain::new(consensus, None, 1337, None);
        
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
        node.register_consensus_domain(pow.clone()).unwrap();
        node.register_consensus_domain(pos.clone()).unwrap();

        let alice = Address::from([0xA1u8; 32]);
        node.state.add_balance(&alice, 1000);
        assert_eq!(node.state.get_nonce(&alice), 0);

        let mut b_pow = Block::new(1, "pow".repeat(32), vec![]);
        b_pow.state_root = "pow_state".repeat(32)[0..64].to_string();
        b_pow.tx_root = b_pow.calculate_tx_root();
        b_pow.hash = b_pow.calculate_hash();
        
        let mut com_pow = DomainCommitment::from_block(&pow, &b_pow, [0u8; 32], [0u8; 32], 1).unwrap();
        com_pow.state_updates.insert(alice, 1); // Claims consuming nonce 0 -> 1

        let mut b_pos = Block::new(1, "pos".repeat(32), vec![]);
        b_pos.state_root = "pos_state".repeat(32)[0..64].to_string();
        b_pos.tx_root = b_pos.calculate_tx_root();
        b_pos.hash = b_pos.calculate_hash();
        
        let mut com_pos = DomainCommitment::from_block(&pos, &b_pos, [0u8; 32], [0u8; 32], 1).unwrap();
        com_pos.state_updates.insert(alice, 1); // Also claims consuming nonce 0 -> 1

        let pow_res = node.submit_domain_commitment(com_pow);
        assert!(pow_res.is_ok(), "First commitment should be accepted: {:?}", pow_res.err());
        assert_eq!(node.state.get_nonce(&alice), 1);

        let res2 = node.submit_domain_commitment(com_pos);
        assert!(res2.is_ok(), "Second commitment for same nonce should be accepted into registry but ignore state update");
        assert_eq!(node.state.get_nonce(&alice), 1, "Nonce must not be double-spent");

        assert_eq!(node.state.get_nonce(&alice), 1, "Nonce should remain at 1 after rejected double-spend");
    }

    #[tokio::test]
    async fn test_cross_domain_double_spend_order_independence() {
        let make_node = || {
            let consensus = std::sync::Arc::new(crate::consensus::pow::PoWEngine::new(0));
            let mut node = Blockchain::new(consensus, None, 1337, None);
            let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
            let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
            node.register_consensus_domain(pow.clone()).unwrap();
            node.register_consensus_domain(pos.clone()).unwrap();
            let alice = Address::from([0xA1u8; 32]);
            node.state.add_balance(&alice, 1000);
            (node, pow, pos, alice)
        };

        let (mut node_a, pow_a, pos_a, alice_a) = make_node();
        let (mut node_b, _pow_b, _pos_b, alice_b) = make_node();

        let mut b_pow = Block::new(1, "pow".repeat(32), vec![]);
        b_pow.state_root = "pow_state".repeat(32)[0..64].to_string();
        b_pow.tx_root = b_pow.calculate_tx_root();
        b_pow.hash = b_pow.calculate_hash();
        let mut com_pow = DomainCommitment::from_block(&pow_a, &b_pow, [0u8; 32], [0u8; 32], 1).unwrap();
        com_pow.state_updates.insert(alice_a, 1);

        let mut b_pos = Block::new(1, "pos".repeat(32), vec![]);
        b_pos.state_root = "pos_state".repeat(32)[0..64].to_string();
        b_pos.tx_root = b_pos.calculate_tx_root();
        b_pos.hash = b_pos.calculate_hash();
        let mut com_pos = DomainCommitment::from_block(&pos_a, &b_pos, [0u8; 32], [0u8; 32], 1).unwrap();
        com_pos.state_updates.insert(alice_a, 1);

        let com_pow_b = com_pow.clone();
        let com_pos_b = com_pos.clone();

        assert!(node_a.submit_domain_commitment(com_pow).is_ok());
        let res_a = node_a.submit_domain_commitment(com_pos);
        assert!(res_a.is_ok(), "Idempotent/Buffered registry should return Ok");
        assert_eq!(node_a.state.get_nonce(&alice_a), 1);

        assert!(node_b.submit_domain_commitment(com_pos_b).is_ok());
        let res_b = node_b.submit_domain_commitment(com_pow_b);
        assert!(res_b.is_ok(), "Idempotent/Buffered registry should return Ok");
        assert_eq!(node_b.state.get_nonce(&alice_b), 1);

        assert_eq!(node_a.state.get_nonce(&alice_a), node_b.state.get_nonce(&alice_b));
    }

    #[tokio::test]
    async fn test_cross_domain_non_conflicting_updates_can_coexist() {
        let consensus = std::sync::Arc::new(crate::consensus::pow::PoWEngine::new(0));
        let mut node = Blockchain::new(consensus, None, 1337, None);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
        node.register_consensus_domain(pow.clone()).unwrap();
        node.register_consensus_domain(pos.clone()).unwrap();

        let alice = Address::from([0xA1u8; 32]);
        let bob = Address::from([0xB2u8; 32]);
        node.state.add_balance(&alice, 1000);
        node.state.add_balance(&bob, 1000);

        let mut b_pow = Block::new(1, "pow".repeat(32), vec![]);
        b_pow.state_root = "pow_state".repeat(32)[0..64].to_string();
        b_pow.tx_root = b_pow.calculate_tx_root();
        b_pow.hash = b_pow.calculate_hash();
        let mut com_pow = DomainCommitment::from_block(&pow, &b_pow, [0u8; 32], [0u8; 32], 1).unwrap();
        com_pow.state_updates.insert(alice, 1);

        let mut b_pos = Block::new(1, "pos".repeat(32), vec![]);
        b_pos.state_root = "pos_state".repeat(32)[0..64].to_string();
        b_pos.tx_root = b_pos.calculate_tx_root();
        b_pos.hash = b_pos.calculate_hash();
        let mut com_pos = DomainCommitment::from_block(&pos, &b_pos, [0u8; 32], [0u8; 32], 1).unwrap();
        com_pos.state_updates.insert(bob, 1);

        assert!(node.submit_domain_commitment(com_pow).is_ok());
        assert!(node.submit_domain_commitment(com_pos).is_ok());

        assert_eq!(node.state.get_nonce(&alice), 1);
        assert_eq!(node.state.get_nonce(&bob), 1);
    }

    #[tokio::test]
    async fn test_parallel_cross_domain_stress_determinism() {
        use rand::seq::SliceRandom;
        use rand::rng;

        let make_node = || {
            let consensus = std::sync::Arc::new(crate::consensus::pow::PoWEngine::new(0));
            let mut node = Blockchain::new(consensus, None, 1337, None);
            for i in 1..=5 {
                let pow = default_domain(i, ConsensusKind::PoW, 1337 + i as u64, "pow-confirmation-depth", 0);
                node.register_consensus_domain(pow).unwrap();
            }
            node
        };

        let mut node_a = make_node();
        let mut node_b = make_node();

        let mut commitments = Vec::new();
        let accounts: Vec<Address> = (0..100).map(|i| Address::from([i as u8; 32])).collect();
        
        for i in 0..1000 {
            let domain_id = (i % 5) + 1;
            let addr_idx = i % 100;
            let nonce = (i / 100) + 1;
            
            let mut block = Block::new(i as u64, format!("hash_{}", i), vec![]);
            block.state_root = format!("state_{}", i);
            block.tx_root = block.calculate_tx_root();
            block.hash = block.calculate_hash();
            
            let domain = node_a.domain_registry.get(domain_id).unwrap();
            let mut com = DomainCommitment::from_block(&domain, &block, [0u8; 32], [0u8; 32], i as u64).unwrap();
            com.state_updates.insert(accounts[addr_idx as usize], nonce as u64);
            commitments.push(com);
        }

        let mut commitments_a = commitments.clone();
        let mut commitments_b = commitments.clone();
        
        let mut rng = rng();
        commitments_a.shuffle(&mut rng);
        commitments_b.shuffle(&mut rng);

        for com in commitments_a {
            let _ = node_a.submit_domain_commitment(com);
        }
        for com in commitments_b {
            let _ = node_b.submit_domain_commitment(com);
        }

        for addr in &accounts {
            assert_eq!(
                node_a.state.get_nonce(addr),
                node_b.state.get_nonce(addr),
                "Determinism failed for account {:?}", addr
            );
        }
    }

    #[tokio::test]
    async fn test_concurrent_tokio_submission() {
        let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
        let node = Blockchain::new(consensus, None, 1337, None);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        let mut node = node;
        node.register_consensus_domain(pow.clone()).unwrap();
        let alice = Address::from([0xA1u8; 32]);
        node.state.add_balance(&alice, 1000);
        
        let node_shared = Arc::new(RwLock::new(node));
        let mut handles = Vec::new();

        for i in 0..100 {
            let n_arc = node_shared.clone();
            let p_arc = pow.clone();
            let a_arc = alice;
            let h = tokio::spawn(async move {
                let mut block = Block::new(i as u64, format!("h_{}", i), vec![]);
                block.hash = block.calculate_hash();
                let mut com = DomainCommitment::from_block(&p_arc, &block, [0u8; 32], [0u8; 32], i as u64).unwrap();
                com.state_updates.insert(a_arc, 1);
                
                let mut node_write = n_arc.write().await;
                node_write.submit_domain_commitment(com)
            });
            handles.push(h);
        }

        let mut success_count = 0;
        for h in handles {
            if h.await.unwrap().is_ok() {
                success_count += 1;
            }
        }

        assert_eq!(success_count, 100, "All unique heights should be accepted into registry");
        let node_final = node_shared.read().await;
        assert_eq!(node_final.state.get_nonce(&alice), 1, "Nonce should only be updated by the first valid height");
    }

    #[tokio::test]
    async fn test_crash_recovery() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("crash_recovery_db");
        let path_str = path.to_str().unwrap();

        let alice = Address::from([0xA1u8; 32]);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);

        {
            let storage = crate::storage::db::Storage::new(path_str).unwrap();
            let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
            let mut node = Blockchain::new(consensus, Some(storage), 1337, None);
            node.register_consensus_domain(pow.clone()).unwrap();
            node.state.add_balance(&alice, 1000);
            
            let mut block = Block::new(1, "h1".to_string(), vec![]);
            block.hash = block.calculate_hash();
            let mut com = DomainCommitment::from_block(&pow, &block, [0u8; 32], [0u8; 32], 1).unwrap();
            com.state_updates.insert(alice, 1);
            node.submit_domain_commitment(com).unwrap();
            
            assert_eq!(node.state.get_nonce(&alice), 1);
        }

        {
            let storage = crate::storage::db::Storage::new(path_str).unwrap();
            let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
            let node = Blockchain::new(consensus, Some(storage), 1337, None);
            
            assert_eq!(node.state.get_nonce(&alice), 1);
            assert!(node.domain_registry.get(1).is_some());
            assert_eq!(node.domain_commitment_registry.len(), 1);
        }
    }

    #[tokio::test]
    async fn test_merkle_root_replay() {
        let make_node = || {
            let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
            let mut node = Blockchain::new(consensus, None, 1337, None);
            let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
            node.register_consensus_domain(pow.clone()).unwrap();
            (node, pow)
        };

        let (mut node_a, pow_a) = make_node();
        let (mut node_b, _) = make_node();

        let mut block = Block::new(1, "h1".to_string(), vec![]);
        block.hash = block.calculate_hash();
        let mut com = DomainCommitment::from_block(&pow_a, &block, [0u8; 32], [0u8; 32], 1).unwrap();
        com.state_updates.insert(Address::from([1u8; 32]), 1);

        node_a.submit_domain_commitment(com.clone()).unwrap();
        node_b.submit_domain_commitment(com).unwrap();

        let root_a = node_a.build_global_header(None).domain_commitment_root;
        let root_b = node_b.build_global_header(None).domain_commitment_root;

        assert_eq!(root_a, root_b);
        assert_ne!(root_a, [0u8; 32]);
    }

    #[tokio::test]
    async fn test_network_partition_convergence() {
        let make_node = || {
            let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
            let mut node = Blockchain::new(consensus, None, 1337, None);
            let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
            let pos = default_domain(2, ConsensusKind::PoS, 1338, "pos-qc-finality", 0);
            node.register_consensus_domain(pow.clone()).unwrap();
            node.register_consensus_domain(pos.clone()).unwrap();
            (node, pow, pos)
        };

        let (mut node_a, pow_a, pos_a) = make_node();
        let (mut node_b, _, _) = make_node();

        let alice = Address::from([1u8; 32]);
        let bob = Address::from([2u8; 32]);
        node_a.state.add_balance(&alice, 1000);
        node_a.state.add_balance(&bob, 1000);
        node_b.state.add_balance(&alice, 1000);
        node_b.state.add_balance(&bob, 1000);

        let mut b1 = Block::new(1, "h1".to_string(), vec![]);
        let mut com1 = DomainCommitment::from_block(&pow_a, &b1, [0u8; 32], [0u8; 32], 1).unwrap();
        com1.state_updates.insert(alice, 1);

        let mut b2 = Block::new(1, "h2".to_string(), vec![]);
        let mut com2 = DomainCommitment::from_block(&pos_a, &b2, [0u8; 32], [0u8; 32], 1).unwrap();
        com2.state_updates.insert(bob, 1);

        node_a.submit_domain_commitment(com1.clone()).unwrap();
        node_b.submit_domain_commitment(com2.clone()).unwrap();

        assert_ne!(node_a.state.get_nonce(&alice), node_b.state.get_nonce(&alice));

        node_a.submit_domain_commitment(com2).unwrap();
        node_b.submit_domain_commitment(com1).unwrap();

        assert_eq!(node_a.state.get_nonce(&alice), 1);
        assert_eq!(node_a.state.get_nonce(&bob), 1);
        assert_eq!(node_b.state.get_nonce(&alice), 1);
        assert_eq!(node_b.state.get_nonce(&bob), 1);
        let mut h_a = node_a.build_global_header(None);
        h_a.timestamp_ms = 0;
        let mut h_b = node_b.build_global_header(None);
        h_b.timestamp_ms = 0;
        assert_eq!(h_a.calculate_hash(), h_b.calculate_hash());
    }

    #[tokio::test]
    async fn test_byzantine_domain_equivocation() {
        let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
        let mut node = Blockchain::new(consensus, None, 1337, None);
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 0);
        node.register_consensus_domain(pow.clone()).unwrap();

        let alice = Address::from([1u8; 32]);
        node.state.add_balance(&alice, 1000);

        let mut b1 = Block::new(1, "h1".to_string(), vec![]);
        let mut com1 = DomainCommitment::from_block(&pow, &b1, [0u8; 32], [0u8; 32], 1).unwrap();
        com1.state_updates.insert(alice, 1);

        let mut b2 = Block::new(1, "h2".to_string(), vec![]);
        let mut com2 = DomainCommitment::from_block(&pow, &b2, [0u8; 32], [0u8; 32], 2).unwrap();
        com2.state_updates.insert(alice, 1);

        node.submit_domain_commitment(com1).unwrap();
        let res = node.submit_domain_commitment(com2);
        
        assert!(res.is_err(), "Equivocation (same height, different hash) must be rejected");
        assert_eq!(node.state.get_nonce(&alice), 1);
        assert_eq!(node.domain_registry.get(1).unwrap().status, DomainStatus::Frozen);
    }

    #[tokio::test]
    async fn test_async_gossip_packet_duplication_idempotency() {
        let mut node = make_node();
        let alice = Address::from([0xA1u8; 32]);
        node.state.add_balance(&alice, 1000);
        let com = make_commitment_for_account(&node, 1, alice, 1, 1);
        let mut accepted = 0;
        let mut rejected = 0;
        for _ in 0..20 {
            let res = node.submit_domain_commitment(com.clone());
            if res.is_ok() {
                accepted += 1;
            } else {
                rejected += 1;
            }
        }
        assert_eq!(accepted, 20);
        assert_eq!(rejected, 0);
        assert_eq!(node.state.get_nonce(&alice), 1);
    }

    #[tokio::test]
    async fn test_old_partition_replay_message_rejected() {
        let mut node = make_node();
        let alice = Address::from([0xA1u8; 32]);
        node.state.add_balance(&alice, 1000);
        let c1 = make_commitment_for_account(&node, 1, alice, 1, 1);
        let c2 = make_commitment_for_account(&node, 1, alice, 2, 2);
        let c3 = make_commitment_for_account(&node, 1, alice, 3, 3);
        assert!(node.submit_domain_commitment(c1.clone()).is_ok());
        assert!(node.submit_domain_commitment(c2).is_ok());
        assert!(node.submit_domain_commitment(c3).is_ok());
        assert_eq!(node.state.get_nonce(&alice), 3);
        let replay = node.submit_domain_commitment(c1);
        assert!(replay.is_ok(), "Exact duplicate should be idempotent (Ok)");
        assert_eq!(node.state.get_nonce(&alice), 3);
    }

    #[tokio::test]
    async fn test_async_gossip_message_delay_reordering_convergence() {
        let mut nodes = vec![make_node(), make_node(), make_node()];
        let accounts: Vec<Address> = (0..10).map(|i| Address::from([i as u8; 32])).collect();
        for node in nodes.iter_mut() {
            for acc in &accounts {
                node.state.add_balance(acc, 1000);
            }
        }
        let commitments = make_non_conflicting_commitments(&nodes[0], &accounts, 30);
        let order_a = commitments.clone();
        let mut order_b = commitments.clone();
        let mut order_c = commitments.clone();
        order_b.reverse();
        order_c.rotate_left(7);
        for com in order_a {
            let _ = nodes[0].submit_domain_commitment(com);
        }
        for com in order_b {
            let _ = nodes[1].submit_domain_commitment(com);
        }
        for com in order_c {
            let _ = nodes[2].submit_domain_commitment(com);
        }
        let mut h0_header = nodes[0].build_global_header(None);
        h0_header.timestamp_ms = 0;
        let h0 = h0_header.calculate_hash();

        for i in 1..nodes.len() {
            let mut hi_header = nodes[i].build_global_header(None);
            hi_header.timestamp_ms = 0;
            assert_eq!(
                h0,
                hi_header.calculate_hash(),
                "conflict tie-break is not deterministic across nodes"
            );
        }
        for acc in &accounts {
            let expected = nodes[0].state.get_nonce(acc);
            for node in &nodes[1..] {
                assert_eq!(expected, node.state.get_nonce(acc));
            }
        }
    }

    #[tokio::test]
    async fn test_async_gossip_packet_drop_later_recovery() {
        let mut nodes = vec![make_node(), make_node(), make_node()];
        let accounts: Vec<Address> = (0..10).map(|i| Address::from([i as u8; 32])).collect();
        for node in nodes.iter_mut() {
            for acc in &accounts {
                node.state.add_balance(acc, 1000);
            }
        }
        let commitments = make_non_conflicting_commitments(&nodes[0], &accounts, 30);
        for com in commitments[0..30].iter().cloned() {
            let _ = nodes[0].submit_domain_commitment(com);
        }
        for com in commitments[0..10].iter().cloned() {
            let _ = nodes[1].submit_domain_commitment(com);
        }
        for com in commitments[10..20].iter().cloned() {
            let _ = nodes[2].submit_domain_commitment(com);
        }
        let mut h0_header = nodes[0].build_global_header(None);
        h0_header.timestamp_ms = 0;
        let h0_hash = h0_header.calculate_hash();

        let mut h1_header = nodes[1].build_global_header(None);
        h1_header.timestamp_ms = 0;
        let h1_hash = h1_header.calculate_hash();

        assert_ne!(h0_hash, h1_hash);

        for node in nodes.iter_mut() {
            for com in commitments.iter().cloned() {
                let _ = node.submit_domain_commitment(com);
            }
        }
        
        let mut expected_header = nodes[0].build_global_header(None);
        expected_header.timestamp_ms = 0;
        let expected_hash = expected_header.calculate_hash();

        for i in 1..nodes.len() {
            let mut hi_header = nodes[i].build_global_header(None);
            hi_header.timestamp_ms = 0;
            assert_eq!(expected_hash, hi_header.calculate_hash());
        }
    }

    #[tokio::test]
    async fn test_partial_commit_propagation_conflict_deterministic_tiebreak() {
        let mut nodes = vec![make_node(), make_node(), make_node()];
        let alice = Address::from([0xA1u8; 32]);
        for node in nodes.iter_mut() {
            node.state.add_balance(&alice, 1000);
        }
        let com_pow = make_commitment_for_account(&nodes[0], 1, alice, 1, 1);
        let com_pos = make_commitment_for_account(&nodes[0], 2, alice, 1, 1);
        let _ = nodes[0].submit_domain_commitment(com_pow.clone());
        let _ = nodes[1].submit_domain_commitment(com_pos.clone());
        assert_eq!(nodes[0].state.get_nonce(&alice), 1);
        assert_eq!(nodes[1].state.get_nonce(&alice), 1);
        assert_eq!(nodes[2].state.get_nonce(&alice), 0);
        for node in nodes.iter_mut() {
            let _ = node.submit_domain_commitment(com_pow.clone());
            let _ = node.submit_domain_commitment(com_pos.clone());
        }
        for node in &nodes {
            assert_eq!(node.state.get_nonce(&alice), 1);
        }
        let mut h0_header = nodes[0].build_global_header(None);
        h0_header.timestamp_ms = 0;
        let h0 = h0_header.calculate_hash();

        for i in 1..nodes.len() {
            let mut hi_header = nodes[i].build_global_header(None);
            hi_header.timestamp_ms = 0;
            assert_eq!(
                h0,
                hi_header.calculate_hash(),
                "conflict tie-break is not deterministic across nodes"
            );
        }
    }

    #[tokio::test]
    async fn test_async_gossip_random_delay_duplicate_drop_convergence() {
        use rand::{Rng, SeedableRng};
        use rand::rngs::StdRng;
        use rand::seq::SliceRandom;
        let mut rng = StdRng::seed_from_u64(42);
        let node_count = 5;
        let mut nodes: Vec<Blockchain> = (0..node_count).map(|_| make_node()).collect();
        let accounts: Vec<Address> = (0..50).map(|i| Address::from([i as u8; 32])).collect();
        for node in nodes.iter_mut() {
            for acc in &accounts {
                node.state.add_balance(acc, 1000);
            }
        }
        let commitments = make_non_conflicting_commitments(&nodes[0], &accounts, 500);
        for com in commitments.iter().cloned() {
            for node_idx in 0..node_count {
                if rng.random_bool(0.20) {
                    continue;
                }
                let duplicates = if rng.random_bool(0.30) { 2 } else { 1 };
                for _ in 0..duplicates {
                    let _ = nodes[node_idx].submit_domain_commitment(com.clone());
                }
            }
        }
        for _round in 0..5 {
            let mut shuffled = commitments.clone();
            shuffled.shuffle(&mut rng);
            for node in nodes.iter_mut() {
                for com in shuffled.iter().cloned() {
                    let _ = node.submit_domain_commitment(com);
                }
            }
        }
        let mut expected_header = nodes[0].build_global_header(None);
        expected_header.timestamp_ms = 0;
        let expected_hash = expected_header.calculate_hash();

        for i in 1..node_count {
            let mut hi_header = nodes[i].build_global_header(None);
            hi_header.timestamp_ms = 0;
            assert_eq!(
                expected_hash,
                hi_header.calculate_hash(),
                "node {} failed convergence under random gossip chaos",
                i
            );
        }
        for acc in &accounts {
            let expected_nonce = nodes[0].state.get_nonce(acc);
            for i in 1..node_count {
                assert_eq!(expected_nonce, nodes[i].state.get_nonce(acc));
            }
        }
    }

    #[tokio::test]
    async fn test_gossip_equivocation_detection_freezes_domain_globally() {
        let mut nodes = vec![make_node(), make_node(), make_node()];
        let domain_id = 1;
        let c1 = make_domain_commitment_at_height(&nodes[0], domain_id, 10, "state_a", 10);
        let c2 = make_domain_commitment_at_height(&nodes[0], domain_id, 10, "state_b", 11);
        let _ = nodes[0].submit_domain_commitment(c1.clone());
        let _ = nodes[1].submit_domain_commitment(c2.clone());
        for node in nodes.iter_mut() {
            let _ = node.submit_domain_commitment(c1.clone());
            let _ = node.submit_domain_commitment(c2.clone());
        }
        for node in nodes.iter() {
            let domain = node.domain_registry.get(domain_id as u32).unwrap();
            assert_eq!(domain.status, DomainStatus::Frozen);
        }
        let c3 = make_domain_commitment_at_height(&nodes[0], domain_id, 11, "state_c", 12);
        for node in nodes.iter_mut() {
            assert!(node.submit_domain_commitment(c3.clone()).is_err());
        }
    }

    #[tokio::test]
    async fn test_out_of_order_domain_height_buffering_or_rejection() {
        let mut node = make_node();
        let alice = Address::from([0xA1u8; 32]);
        node.state.add_balance(&alice, 1000);

        let c8 = make_commitment_for_account(&node, 1, alice, 8, 8);
        let c9 = make_commitment_for_account(&node, 1, alice, 9, 9);
        let c10 = make_commitment_for_account(&node, 1, alice, 10, 10);

        let r10 = node.submit_domain_commitment(c10);
        assert!(r10.is_ok(), "height 10 should be buffered in registry");
        assert_eq!(node.state.get_nonce(&alice), 0);

        for i in 1..8 {
            let ci = make_commitment_for_account(&node, 1, alice, i, i);
            node.submit_domain_commitment(ci).unwrap();
        }
        assert_eq!(node.state.get_nonce(&alice), 7);

        node.submit_domain_commitment(c8).unwrap();
        assert_eq!(node.state.get_nonce(&alice), 8);

        node.submit_domain_commitment(c9).unwrap();
        assert_eq!(node.state.get_nonce(&alice), 10);
    }

    fn make_node() -> Blockchain {
        let consensus = Arc::new(crate::consensus::pow::PoWEngine::new(0));
        let mut node = Blockchain::new(consensus, None, 1337, None);

        for i in 1..=5 {
            let (kind, adapter) = match i {
                1 => (ConsensusKind::PoW, "pow-confirmation-depth"),
                2 => (ConsensusKind::PoS, "pos-qc-finality"),
                3 => (ConsensusKind::PoA, "poa-authority-quorum"),
                _ => (ConsensusKind::PoW, "pow-confirmation-depth"),
            };

            let domain = default_domain(
                i as u32,
                kind,
                1337 + i as u64,
                adapter,
                0,
            );

            node.register_consensus_domain(domain).unwrap();
        }

        node
    }

    fn make_commitment_for_account(
        node: &Blockchain,
        domain_id: u32,
        account: Address,
        nonce: u64,
        sequence: u64,
    ) -> DomainCommitment {
        let domain = node.domain_registry.get(domain_id).unwrap();

        let mut block = Block::new(
            sequence,
            format!("parent_{}", sequence),
            vec![],
        );

        block.state_root = format!("state_{}_{}", domain_id, sequence)
            .repeat(8)[0..64]
            .to_string();

        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();

        let mut com = DomainCommitment::from_block(
            domain,
            &block,
            [0u8; 32],
            [0u8; 32],
            sequence,
        )
        .unwrap();

        com.state_updates.insert(account, nonce);
        com
    }

    fn make_non_conflicting_commitments(
        node: &Blockchain,
        accounts: &[Address],
        count: usize,
    ) -> Vec<DomainCommitment> {
        let mut commitments = Vec::new();

        for i in 0..count {
            let domain_id = ((i % 5) + 1) as u32;
            let account = accounts[i % accounts.len()];
            let height = (i / 5) as u64 + 1;
            let nonce = height; // Simplified: 1 commitment = 1 nonce increment
            
            let mut com = make_commitment_for_account(node, domain_id, account, height, i as u64 + 1);
            com.state_updates.insert(account, nonce);
            commitments.push(com);
        }

        commitments
    }

    fn make_domain_commitment_at_height(
        node: &Blockchain,
        domain_id: u32,
        height: u64,
        state_root: &str,
        sequence: u64,
    ) -> DomainCommitment {
        let domain = node.domain_registry.get(domain_id).unwrap();

        let mut block = Block::new(
            height,
            format!("parent_{}", height),
            vec![],
        );

        block.state_root = format!("{:0<64}", state_root.repeat(32))[0..64].to_string();
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();

        DomainCommitment::from_block(
            domain,
            &block,
            [0u8; 32],
            [0u8; 32],
            sequence,
        )
        .unwrap()
    }
}
