#[cfg(test)]
mod settlement_prod_tests {
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::pow::PoWEngine;
    use crate::core::address::Address;
    use crate::core::block::Block;
    use crate::core::hash::hash_fields_bytes;
    use crate::cross_domain::message::CrossDomainMessageParams;
    use crate::cross_domain::{
        CrossDomainMessage, DomainEvent, DomainEventKind, DomainEventTree, MessageKind,
    };
    use crate::domain::finality_adapter::{hash_finality_proof, FinalityProof};
    use crate::domain::plugin::default_domain;
    use crate::domain::{ConsensusKind, DomainCommitment, DomainStatus};
    use crate::storage::db::Storage;
    use std::sync::Arc;

    fn test_chain() -> Blockchain {
        Blockchain::new(Arc::new(PoWEngine::new(0)), None, 1337, None)
    }

    fn domain(id: u32, kind: ConsensusKind) -> crate::domain::ConsensusDomain {
        let adapter = match kind {
            ConsensusKind::PoW => "pow-confirmation-depth",
            ConsensusKind::PoS => "pos-qc-finality",
            ConsensusKind::PoA => "poa-authority-quorum",
            _ => "custom",
        };
        default_domain(id, kind, 1337 + id as u64, adapter, 0)
    }

    fn commitment_for(
        domain: &crate::domain::ConsensusDomain,
        height: u64,
        sequence: u64,
        seed: u8,
    ) -> DomainCommitment {
        let mut block = Block::new(height, "aa".repeat(32), vec![]);
        block.timestamp = 0;
        block.state_root = format!("{:02x}", seed).repeat(32);
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();
        DomainCommitment::from_block(
            domain,
            &block,
            [seed; 32],
            [seed.saturating_add(1); 32],
            sequence,
        )
        .unwrap()
    }

    fn commitment_with_proof(
        domain: &crate::domain::ConsensusDomain,
        height: u64,
        sequence: u64,
        seed: u8,
        proof: &FinalityProof,
    ) -> DomainCommitment {
        let mut commitment = commitment_for(domain, height, sequence, seed);
        commitment.finality_proof_hash = hash_finality_proof(proof);
        commitment
    }

    #[test]
    fn pow_pos_poa_domains_can_all_contribute_to_one_global_commitment_root() {
        let mut blockchain = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        let pos = domain(2, ConsensusKind::PoS);
        let poa = domain(3, ConsensusKind::PoA);

        blockchain.register_consensus_domain(pow.clone()).unwrap();
        blockchain.register_consensus_domain(pos.clone()).unwrap();
        blockchain.register_consensus_domain(poa.clone()).unwrap();

        let before = blockchain.build_global_header(None);
        blockchain
            .submit_domain_commitment(commitment_for(&pow, 10, 0, 1))
            .unwrap();
        blockchain
            .submit_domain_commitment(commitment_for(&pos, 11, 0, 2))
            .unwrap();
        blockchain
            .submit_domain_commitment(commitment_for(&poa, 12, 0, 3))
            .unwrap();

        let after = blockchain.build_global_header(None);
        assert_ne!(before.domain_commitment_root, after.domain_commitment_root);
        assert_eq!(blockchain.domain_commitment_registry.len(), 3);
        assert_eq!(
            after.domain_registry_root,
            blockchain.domain_registry.root()
        );
    }

    #[test]
    fn settlement_rejects_cross_consensus_kind_confusion() {
        let mut blockchain = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let mut commitment = commitment_for(&pow, 10, 0, 1);
        commitment.consensus_kind = ConsensusKind::PoS;

        let err = blockchain.submit_domain_commitment(commitment).unwrap_err();
        assert!(err.contains("consensus kind mismatch"));
        assert!(blockchain.domain_commitment_registry.is_empty());
    }

    #[test]
    fn settlement_rejects_frozen_domain_commitments() {
        let mut blockchain = test_chain();
        let poa = domain(3, ConsensusKind::PoA);
        blockchain.register_consensus_domain(poa.clone()).unwrap();
        blockchain
            .domain_registry
            .set_status(poa.id, DomainStatus::Frozen)
            .unwrap();

        let err = blockchain
            .submit_domain_commitment(commitment_for(&poa, 1, 0, 8))
            .unwrap_err();
        assert!(err.contains("not active"));
        assert!(blockchain.domain_commitment_registry.is_empty());
    }

    #[test]
    fn sealed_global_headers_form_a_hash_chain() {
        let mut blockchain = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        blockchain.register_consensus_domain(pow.clone()).unwrap();
        blockchain
            .submit_domain_commitment(commitment_for(&pow, 1, 0, 1))
            .unwrap();

        let first = blockchain.seal_global_header(None).unwrap();
        blockchain
            .submit_domain_commitment(commitment_for(&pow, 2, 0, 2))
            .unwrap();
        let second = blockchain.seal_global_header(None).unwrap();

        assert_eq!(first.global_height, 0);
        assert_eq!(second.global_height, 1);
        assert_eq!(second.previous_global_hash, first.calculate_hash_bytes());
        assert_ne!(first.calculate_hash(), second.calculate_hash());
    }

    #[test]
    fn multi_consensus_settlement_state_round_trips_through_storage() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("multi-consensus-settlement");
        let path = path.to_str().unwrap();

        {
            let storage = Storage::new(path).unwrap();
            let mut blockchain =
                Blockchain::new(Arc::new(PoWEngine::new(0)), Some(storage), 1337, None);
            for (id, kind, seed) in [
                (1, ConsensusKind::PoW, 1u8),
                (2, ConsensusKind::PoS, 2u8),
                (3, ConsensusKind::PoA, 3u8),
            ] {
                let domain = domain(id, kind);
                blockchain
                    .register_consensus_domain(domain.clone())
                    .unwrap();
                blockchain
                    .submit_domain_commitment(commitment_for(&domain, id as u64, 0, seed))
                    .unwrap();
            }
            blockchain.seal_global_header(None).unwrap();
        }

        let storage = Storage::new(path).unwrap();
        let blockchain = Blockchain::new(Arc::new(PoWEngine::new(0)), Some(storage), 1337, None);

        assert!(blockchain.domain_registry.get(1).is_some());
        assert!(blockchain.domain_registry.get(2).is_some());
        assert!(blockchain.domain_registry.get(3).is_some());
        assert_eq!(blockchain.domain_commitment_registry.len(), 3);
        assert_eq!(blockchain.global_headers.len(), 1);
    }

    #[test]
    fn verified_pow_commitment_requires_finalized_depth_and_matching_proof_hash() {
        let mut blockchain = test_chain();
        let pow = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 4);
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let pending_proof = FinalityProof::PoW {
            confirmations: 3,
            total_work_hint: 100,
        };
        let pending_commitment = commitment_with_proof(&pow, 10, 0, 1, &pending_proof);
        let err = blockchain
            .submit_verified_domain_commitment(pending_commitment, pending_proof)
            .unwrap_err();
        assert!(err.contains("not finalized"));
        assert!(blockchain.domain_commitment_registry.is_empty());

        let finalized_proof = FinalityProof::PoW {
            confirmations: 64,
            total_work_hint: 200,
        };
        let mut bad_hash_commitment = commitment_with_proof(&pow, 10, 0, 1, &finalized_proof);
        bad_hash_commitment.finality_proof_hash = [9u8; 32];
        let err = blockchain
            .submit_verified_domain_commitment(bad_hash_commitment, finalized_proof.clone())
            .unwrap_err();
        assert!(err.contains("proof hash mismatch"));

        let finalized_commitment = commitment_with_proof(&pow, 10, 0, 1, &finalized_proof);
        blockchain
            .submit_verified_domain_commitment(finalized_commitment, finalized_proof)
            .unwrap();
        assert_eq!(blockchain.domain_commitment_registry.len(), 1);
    }

    #[test]
    fn verified_poa_commitment_requires_authority_quorum() {
        let mut blockchain = test_chain();
        let poa = domain(3, ConsensusKind::PoA);
        blockchain.register_consensus_domain(poa.clone()).unwrap();

        let weak_proof = FinalityProof::PoA {
            signer_count: 2,
            validator_count: 4,
        };
        let weak_commitment = commitment_with_proof(&poa, 3, 0, 3, &weak_proof);
        let err = blockchain
            .submit_verified_domain_commitment(weak_commitment, weak_proof)
            .unwrap_err();
        assert!(err.contains("not finalized"));

        let quorum_proof = FinalityProof::PoA {
            signer_count: 3,
            validator_count: 4,
        };
        let quorum_commitment = commitment_with_proof(&poa, 3, 0, 3, &quorum_proof);
        blockchain
            .submit_verified_domain_commitment(quorum_commitment, quorum_proof)
            .unwrap();
        assert_eq!(blockchain.domain_commitment_registry.len(), 1);
    }

    #[test]
    fn verified_commitment_rejects_wrong_adapter_configuration() {
        let mut blockchain = test_chain();
        let mut pow = domain(1, ConsensusKind::PoW);
        pow.finality_adapter = "poa-authority-quorum".into();
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let proof = FinalityProof::PoW {
            confirmations: 64,
            total_work_hint: 100,
        };
        let commitment = commitment_with_proof(&pow, 1, 0, 1, &proof);
        let err = blockchain
            .submit_verified_domain_commitment(commitment, proof)
            .unwrap_err();
        assert!(err.contains("finality adapter mismatch"));
    }

    #[test]
    fn settlement_verifies_domain_event_proofs_from_committed_event_root() {
        let mut blockchain = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let mut event_tree = DomainEventTree::new();
        for index in 0..3u32 {
            let payload_hash = hash_fields_bytes(&[b"bridge-payload", &index.to_le_bytes()]);
            let message = CrossDomainMessage::new(CrossDomainMessageParams {
                source_domain: pow.id,
                target_domain: 2,
                source_height: 44,
                event_index: index,
                nonce: index as u64,
                sender: Address::from([1u8; 32]),
                recipient: Address::from([2u8; 32]),
                payload_hash,
                kind: MessageKind::BridgeLock,
                expiry_height: 1000,
            });
            event_tree.push(DomainEvent {
                domain_id: pow.id,
                domain_height: 44,
                event_index: index,
                kind: DomainEventKind::BridgeLocked,
                emitter: Address::from([1u8; 32]),
                message: Some(message),
                payload_hash,
            });
        }

        let mut commitment = commitment_for(&pow, 44, 0, 9);
        commitment.event_root = event_tree.root();
        let expected_block_hash = commitment.domain_block_hash;
        blockchain
            .submit_domain_commitment(commitment.clone())
            .unwrap();

        let event = event_tree.events()[1].clone();
        let proof = event_tree.proof(1).unwrap();
        let verified = blockchain
            .verify_domain_event_proof(
                pow.id,
                44,
                0,
                Some(expected_block_hash),
                event.clone(),
                &proof,
            )
            .unwrap();
        assert_eq!(verified.event.event_index, 1);

        assert!(blockchain
            .verify_domain_event_proof(pow.id, 44, 0, Some([0u8; 32]), event.clone(), &proof)
            .is_err());

        let mut wrong_index = proof.clone();
        wrong_index.index = 2;
        assert!(blockchain
            .verify_domain_event_proof(
                pow.id,
                44,
                0,
                Some(expected_block_hash),
                event,
                &wrong_index
            )
            .is_err());

        let missing_event = event_tree.events()[0].clone();
        let missing_proof = event_tree.proof(0).unwrap();
        assert!(blockchain
            .verify_domain_event_proof(pow.id, 999, 0, None, missing_event, &missing_proof,)
            .is_err());
    }

    #[test]
    fn bridge_mint_is_only_called_after_settlement_event_proof_verifies() {
        let mut blockchain = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let asset_id = hash_fields_bytes(&[b"canonical-asset"]);
        let owner = Address::from([11u8; 32]);
        let recipient = Address::from([12u8; 32]);
        blockchain
            .bridge_state
            .register_asset(asset_id, pow.id)
            .unwrap();

        let (_transfer, lock_event) = blockchain
            .bridge_state
            .lock(pow.id, 2, 55, 0, asset_id, owner, recipient, 500, 2_000)
            .unwrap();
        let message_id = lock_event.message.as_ref().unwrap().message_id;

        let mut tree = DomainEventTree::new();
        tree.push(lock_event.clone());
        let mut commitment = commitment_for(&pow, 55, 0, 4);
        commitment.event_root = tree.root();
        blockchain.submit_domain_commitment(commitment).unwrap();

        let proof = tree.proof(0).unwrap();
        blockchain
            .mint_bridge_transfer_from_verified_event(
                pow.id,
                55,
                0,
                None,
                lock_event.clone(),
                &proof,
            )
            .unwrap();
        assert!(
            blockchain
                .mint_bridge_transfer_from_verified_event(pow.id, 55, 0, None, lock_event, &proof)
                .is_err(),
            "verified messages still replay-protect at bridge state"
        );
        blockchain.burn_bridge_transfer(message_id, 2).unwrap();
        blockchain
            .unlock_bridge_transfer(message_id, pow.id)
            .unwrap();
    }

    #[test]
    fn bridge_mint_rejects_verified_non_lock_event() {
        let mut blockchain = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        blockchain.register_consensus_domain(pow.clone()).unwrap();

        let payload_hash = hash_fields_bytes(&[b"minted-event"]);
        let message = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: pow.id,
            target_domain: 2,
            source_height: 88,
            event_index: 0,
            nonce: 0,
            sender: Address::from([1u8; 32]),
            recipient: Address::from([2u8; 32]),
            payload_hash,
            kind: MessageKind::BridgeMint,
            expiry_height: 100,
        });
        let event = DomainEvent {
            domain_id: pow.id,
            domain_height: 88,
            event_index: 0,
            kind: DomainEventKind::BridgeMinted,
            emitter: Address::from([1u8; 32]),
            message: Some(message),
            payload_hash,
        };

        let mut tree = DomainEventTree::new();
        tree.push(event.clone());
        let mut commitment = commitment_for(&pow, 88, 0, 5);
        commitment.event_root = tree.root();
        blockchain.submit_domain_commitment(commitment).unwrap();

        let proof = tree.proof(0).unwrap();
        let err = blockchain
            .mint_bridge_transfer_from_verified_event(pow.id, 88, 0, None, event, &proof)
            .unwrap_err();
        assert!(err.contains("not a bridge lock event"));
    }

    #[test]
    fn global_header_hash_changes_when_bridge_or_replay_roots_change() {
        use crate::cross_domain::BridgeState;

        let blockchain = test_chain();
        let baseline = blockchain.build_global_header(None);

        let mut bridge = BridgeState::new();
        let asset_id = hash_fields_bytes(&[b"asset-root-change"]);
        let owner = Address::from([21u8; 32]);
        let recipient = Address::from([22u8; 32]);
        bridge.register_asset(asset_id, 1).unwrap();
        let (_transfer, event) = bridge
            .lock(1, 2, 1, 0, asset_id, owner, recipient, 1, 100)
            .unwrap();
        let message = event.message.unwrap();

        let mut changed = test_chain();
        changed.bridge_state = bridge.clone();
        let after_lock = changed.build_global_header(None);
        assert_ne!(baseline.bridge_state_root, after_lock.bridge_state_root);
        assert_ne!(baseline.replay_nonce_root, after_lock.replay_nonce_root);
        assert_ne!(baseline.calculate_hash(), after_lock.calculate_hash());

        bridge.mint(&message).unwrap();
        changed.bridge_state = bridge;
        let after_mint = changed.build_global_header(None);
        assert_ne!(after_lock.bridge_state_root, after_mint.bridge_state_root);
        assert_ne!(after_lock.replay_nonce_root, after_mint.replay_nonce_root);
        assert_ne!(after_lock.calculate_hash(), after_mint.calculate_hash());
    }

    fn bft_domain(id: u32) -> crate::domain::ConsensusDomain {
        default_domain(id, ConsensusKind::Bft, 1337 + id as u64, "bft-quorum-commit", 0)
    }

    fn zk_domain(id: u32) -> crate::domain::ConsensusDomain {
        default_domain(id, ConsensusKind::Zk, 1337 + id as u64, "zk-proof-verification", 0)
    }

    #[test]
    fn bft_finality_requires_two_thirds_plus_one_quorum() {
        let mut bc = test_chain();
        let dom = bft_domain(10);
        bc.register_consensus_domain(dom.clone()).unwrap();

        let weak = FinalityProof::Bft {
            round: 1,
            signer_count: 2,
            total_validators: 4,
            commit_hash: [0u8; 32],
        };
        let mut c = commitment_for(&dom, 5, 0, 10);
        c.consensus_kind = ConsensusKind::Bft;
        c.finality_proof_hash = hash_finality_proof(&weak);
        let err = bc.submit_verified_domain_commitment(c.clone(), weak).unwrap_err();
        assert!(err.contains("not match") || err.contains("not finalized"));

        let strong = FinalityProof::Bft {
            round: 1,
            signer_count: 3,
            total_validators: 4,
            commit_hash: c.domain_block_hash,
        };
        c.finality_proof_hash = hash_finality_proof(&strong);
        bc.submit_verified_domain_commitment(c, strong).unwrap();
        assert_eq!(bc.domain_commitment_registry.len(), 1);
    }

    #[test]
    fn bft_finality_rejects_empty_validator_set() {
        let mut bc = test_chain();
        let dom = bft_domain(11);
        bc.register_consensus_domain(dom.clone()).unwrap();

        let proof = FinalityProof::Bft {
            round: 0, signer_count: 0, total_validators: 0, commit_hash: [1u8; 32],
        };
        let mut c = commitment_for(&dom, 1, 0, 11);
        c.consensus_kind = ConsensusKind::Bft;
        c.finality_proof_hash = hash_finality_proof(&proof);
        let err = bc.submit_verified_domain_commitment(c, proof).unwrap_err();
        assert!(err.contains("Rejected") || err.contains("empty"));
    }

    #[test]
    fn bft_finality_rejects_commit_hash_mismatch() {
        let mut bc = test_chain();
        let dom = bft_domain(12);
        bc.register_consensus_domain(dom.clone()).unwrap();

        let proof = FinalityProof::Bft {
            round: 1, signer_count: 4, total_validators: 4, commit_hash: [0xFFu8; 32],
        };
        let mut c = commitment_for(&dom, 1, 0, 12);
        c.consensus_kind = ConsensusKind::Bft;
        c.finality_proof_hash = hash_finality_proof(&proof);
        let err = bc.submit_verified_domain_commitment(c, proof).unwrap_err();
        assert!(err.contains("Rejected") || err.contains("not match"));
    }

    #[test]
    fn zk_finality_accepts_valid_proof_hashes() {
        let mut bc = test_chain();
        let dom = zk_domain(20);
        bc.register_consensus_domain(dom.clone()).unwrap();

        let proof = FinalityProof::Zk {
            proof_hash: [1u8; 32],
            verifier_key_hash: [2u8; 32],
            public_inputs_hash: [3u8; 32],
        };
        let mut c = commitment_for(&dom, 1, 0, 20);
        c.consensus_kind = ConsensusKind::Zk;
        c.finality_proof_hash = hash_finality_proof(&proof);
        bc.submit_verified_domain_commitment(c, proof).unwrap();
        assert_eq!(bc.domain_commitment_registry.len(), 1);
    }

    #[test]
    fn zk_finality_rejects_zero_proof_hash() {
        let mut bc = test_chain();
        let dom = zk_domain(21);
        bc.register_consensus_domain(dom.clone()).unwrap();

        for (ph, vk, pi, label) in [
            ([0u8;32], [2u8;32], [3u8;32], "proof_hash zero"),
            ([1u8;32], [0u8;32], [3u8;32], "verifier_key zero"),
            ([1u8;32], [2u8;32], [0u8;32], "public_inputs zero"),
        ] {
            let proof = FinalityProof::Zk {
                proof_hash: ph, verifier_key_hash: vk, public_inputs_hash: pi,
            };
            let mut c = commitment_for(&dom, 1, 0, 21);
            c.consensus_kind = ConsensusKind::Zk;
            c.finality_proof_hash = hash_finality_proof(&proof);
            let err = bc.submit_verified_domain_commitment(c, proof).unwrap_err();
            assert!(err.contains("Rejected") || err.contains("zero"), "should reject: {}", label);
        }
    }

    #[test]
    fn zk_finality_rejects_wrong_proof_type() {
        let mut bc = test_chain();
        let dom = zk_domain(22);
        bc.register_consensus_domain(dom.clone()).unwrap();

        let wrong_proof = FinalityProof::PoW { confirmations: 100, total_work_hint: 999 };
        let mut c = commitment_for(&dom, 1, 0, 22);
        c.consensus_kind = ConsensusKind::Zk;
        c.finality_proof_hash = hash_finality_proof(&wrong_proof);
        assert!(bc.submit_verified_domain_commitment(c, wrong_proof).is_err());
    }

    #[test]
    fn attack_fake_finality_proof_hash_tampered() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let real_proof = FinalityProof::PoW { confirmations: 3, total_work_hint: 10 };
        let fake_proof = FinalityProof::PoW { confirmations: 999, total_work_hint: 10 };
        let mut c = commitment_for(&pow, 10, 0, 1);
        c.finality_proof_hash = hash_finality_proof(&fake_proof);
        let err = bc.submit_verified_domain_commitment(c, real_proof).unwrap_err();
        assert!(err.contains("proof hash mismatch"));
    }

    #[test]
    fn attack_domain_spoofing_consensus_kind_swap() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let mut c = commitment_for(&pow, 10, 0, 1);
        c.consensus_kind = ConsensusKind::Bft;
        let err = bc.submit_domain_commitment(c).unwrap_err();
        assert!(err.contains("mismatch"));
    }

    #[test]
    fn attack_commitment_to_unregistered_domain() {
        let bc = test_chain();
        let phantom = domain(99, ConsensusKind::PoW);
        let c = commitment_for(&phantom, 1, 0, 99);
        let mut bc = bc;
        let err = bc.submit_domain_commitment(c).unwrap_err();
        assert!(err.contains("Unknown"));
    }

    #[test]
    fn attack_double_commitment_same_block_hash() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let c1 = commitment_for(&pow, 10, 0, 1);
        bc.submit_domain_commitment(c1.clone()).unwrap();

        let mut c2 = c1.clone();
        c2.sequence = 1;
        let err = bc.submit_domain_commitment(c2).unwrap_err();
        assert!(err.contains("already committed"));
    }

    #[test]
    fn attack_commitment_to_retired_domain() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();
        bc.domain_registry.set_status(1, DomainStatus::Retired).unwrap();

        let c = commitment_for(&pow, 1, 0, 1);
        let err = bc.submit_domain_commitment(c).unwrap_err();
        assert!(err.contains("not active"));
    }

    #[test]
    fn attack_bridge_double_lock_same_asset() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let asset = hash_fields_bytes(&[b"double-lock-asset"]);
        let owner = Address::from([10u8; 32]);
        let recipient = Address::from([20u8; 32]);
        bc.bridge_state.register_asset(asset, 1).unwrap();
        bc.bridge_state.lock(1, 2, 1, 0, asset, owner, recipient, 100, 500).unwrap();
        let err = bc.bridge_state.lock(1, 3, 2, 0, asset, owner, recipient, 100, 500).unwrap_err();
        assert!(err.to_string().contains("not active"));
    }

    #[test]
    fn attack_bridge_mint_without_lock() {
        let mut bc = test_chain();
        let fake_msg = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: 1, target_domain: 2, source_height: 1, event_index: 0,
            nonce: 0, sender: Address::from([1u8; 32]), recipient: Address::from([2u8; 32]),
            payload_hash: [0u8; 32], kind: MessageKind::BridgeLock, expiry_height: 100,
        });
        let err = bc.bridge_state.mint(&fake_msg).unwrap_err();
        assert!(err.to_string().contains("Unknown"));
    }

    #[test]
    fn attack_bridge_unlock_without_burn() {
        let mut bc = test_chain();
        let asset = hash_fields_bytes(&[b"unlock-no-burn"]);
        let owner = Address::from([1u8; 32]);
        let recipient = Address::from([2u8; 32]);
        bc.bridge_state.register_asset(asset, 1).unwrap();
        let (transfer, event) = bc.bridge_state.lock(1, 2, 1, 0, asset, owner, recipient, 50, 100).unwrap();
        let msg = event.message.unwrap();
        bc.bridge_state.mint(&msg).unwrap();
        let err = bc.bridge_state.unlock(transfer.message_id, 1).unwrap_err();
        assert!(err.to_string().contains("not burned"));
    }

    #[test]
    fn attack_bridge_burn_wrong_domain() {
        let mut bc = test_chain();
        let asset = hash_fields_bytes(&[b"burn-wrong-domain"]);
        let owner = Address::from([1u8; 32]);
        let recipient = Address::from([2u8; 32]);
        bc.bridge_state.register_asset(asset, 1).unwrap();
        let (transfer, event) = bc.bridge_state.lock(1, 2, 1, 0, asset, owner, recipient, 50, 100).unwrap();
        let msg = event.message.unwrap();
        bc.bridge_state.mint(&msg).unwrap();
        let err = bc.bridge_state.burn(transfer.message_id, 9).unwrap_err();
        assert!(err.to_string().contains("not minted"));
    }

    #[test]
    fn attack_replay_cross_domain_message_after_mint() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let asset = hash_fields_bytes(&[b"replay-test"]);
        let owner = Address::from([1u8; 32]);
        let recipient = Address::from([2u8; 32]);
        bc.bridge_state.register_asset(asset, 1).unwrap();
        let (_t, event) = bc.bridge_state.lock(1, 2, 1, 0, asset, owner, recipient, 100, 500).unwrap();
        let msg = event.message.as_ref().unwrap();

        let mut tree = DomainEventTree::new();
        tree.push(event.clone());
        let mut commitment = commitment_for(&pow, 1, 0, 50);
        commitment.event_root = tree.root();
        bc.submit_domain_commitment(commitment).unwrap();

        let proof = tree.proof(0).unwrap();
        bc.mint_bridge_transfer_from_verified_event(1, 1, 0, None, event.clone(), &proof).unwrap();
        let err = bc.mint_bridge_transfer_from_verified_event(1, 1, 0, None, event, &proof).unwrap_err();
        assert!(err.contains("already processed") || err.contains("replay"));
    }

    #[test]
    fn attack_merkle_proof_forged_sibling() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let mut tree = DomainEventTree::new();
        for i in 0..4u32 {
            let ph = hash_fields_bytes(&[b"forge-test", &i.to_le_bytes()]);
            let msg = CrossDomainMessage::new(CrossDomainMessageParams {
                source_domain: 1, target_domain: 2, source_height: 10, event_index: i,
                nonce: i as u64, sender: Address::from([1u8;32]), recipient: Address::from([2u8;32]),
                payload_hash: ph, kind: MessageKind::BridgeLock, expiry_height: 1000,
            });
            tree.push(DomainEvent {
                domain_id: 1, domain_height: 10, event_index: i,
                kind: DomainEventKind::BridgeLocked, emitter: Address::from([1u8;32]),
                message: Some(msg), payload_hash: ph,
            });
        }

        let mut commitment = commitment_for(&pow, 10, 0, 60);
        commitment.event_root = tree.root();
        bc.submit_domain_commitment(commitment).unwrap();

        let event = tree.events()[1].clone();
        let mut forged_proof = tree.proof(1).unwrap();
        forged_proof.siblings[0] = [0xFFu8; 32];
        let err = bc.verify_domain_event_proof(1, 10, 0, None, event, &forged_proof).unwrap_err();
        assert_eq!(err, crate::settlement::ProofVerificationError::InvalidMerkleProof);
    }

    #[test]
    fn attack_event_domain_height_mismatch() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let ph = hash_fields_bytes(&[b"height-mismatch"]);
        let msg = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: 1, target_domain: 2, source_height: 10, event_index: 0,
            nonce: 0, sender: Address::from([1u8;32]), recipient: Address::from([2u8;32]),
            payload_hash: ph, kind: MessageKind::BridgeLock, expiry_height: 100,
        });
        let event = DomainEvent {
            domain_id: 1, domain_height: 999,
            event_index: 0, kind: DomainEventKind::BridgeLocked,
            emitter: Address::from([1u8;32]), message: Some(msg), payload_hash: ph,
        };

        let mut tree = DomainEventTree::new();
        tree.push(event.clone());
        let mut commitment = commitment_for(&pow, 10, 0, 70);
        commitment.event_root = tree.root();
        bc.submit_domain_commitment(commitment).unwrap();

        let proof = tree.proof(0).unwrap();
        let err = bc.verify_domain_event_proof(1, 10, 0, None, event, &proof).unwrap_err();
        assert_eq!(err, crate::settlement::ProofVerificationError::EventHeightMismatch);
    }

    #[test]
    fn five_consensus_domains_produce_distinct_global_commitment_root() {
        let mut bc = test_chain();
        let domains: Vec<_> = vec![
            domain(1, ConsensusKind::PoW),
            domain(2, ConsensusKind::PoS),
            domain(3, ConsensusKind::PoA),
            bft_domain(4),
            zk_domain(5),
        ];
        for d in &domains {
            bc.register_consensus_domain(d.clone()).unwrap();
        }
        let before = bc.build_global_header(None);

        for (i, d) in domains.iter().enumerate() {
            let mut c = commitment_for(d, 1, 0, (i + 1) as u8);
            c.consensus_kind = d.kind.clone();
            bc.submit_domain_commitment(c).unwrap();
        }
        let after = bc.build_global_header(None);
        assert_ne!(before.domain_commitment_root, after.domain_commitment_root);
        assert_eq!(bc.domain_commitment_registry.len(), 5);
    }

    #[test]
    fn global_header_message_root_reflects_message_registry() {
        use crate::cross_domain::CrossDomainMessageRegistry;

        let mut bc = test_chain();
        let baseline = bc.build_global_header(None);

        let msg = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: 1, target_domain: 2, source_height: 5, event_index: 0,
            nonce: 0, sender: Address::from([1u8;32]), recipient: Address::from([2u8;32]),
            payload_hash: hash_fields_bytes(&[b"msg-root-test"]),
            kind: MessageKind::BridgeLock, expiry_height: 100,
        });
        bc.message_registry.insert(msg).unwrap();
        let after = bc.build_global_header(None);
        assert_ne!(baseline.message_root, after.message_root);
        assert_ne!(baseline.calculate_hash(), after.calculate_hash());
    }

    #[test]
    fn settlement_finality_root_reflects_finality_hashes() {
        let mut bc = test_chain();
        let baseline = bc.build_global_header(None);

        bc.settlement_finality_hashes.push([1u8; 32]);
        let after = bc.build_global_header(None);
        assert_ne!(baseline.settlement_finality_root, after.settlement_finality_root);

        bc.settlement_finality_hashes.push([2u8; 32]);
        let after2 = bc.build_global_header(None);
        assert_ne!(after.settlement_finality_root, after2.settlement_finality_root);
    }

    #[test]
    fn plugin_registry_prevents_duplicate_and_allows_removal() {
        use crate::domain::{DomainPluginRegistry, PoWDomainPlugin};

        let mut reg = DomainPluginRegistry::new();
        let engine = Arc::new(PoWEngine::new(0));
        let p1 = Arc::new(PoWDomainPlugin::new(engine.clone()));
        let p2 = Arc::new(PoWDomainPlugin::new(engine));
        reg.register(1, p1).unwrap();
        assert!(reg.register(1, p2).is_err());
        assert!(reg.get(1).is_some());
        reg.remove(1);
        assert!(reg.get(1).is_none());
    }

    #[test]
    fn message_registry_rejects_tampered_message_id() {
        use crate::cross_domain::CrossDomainMessageRegistry;

        let mut reg = CrossDomainMessageRegistry::new();
        let mut msg = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: 1, target_domain: 2, source_height: 1, event_index: 0,
            nonce: 0, sender: Address::from([1u8;32]), recipient: Address::from([2u8;32]),
            payload_hash: [5u8; 32], kind: MessageKind::BridgeLock, expiry_height: 50,
        });
        msg.nonce = 999;
        assert!(reg.insert(msg).is_err());
    }

    #[test]
    fn commitment_leaf_hash_is_deterministic_and_tamper_evident() {
        let pow = domain(1, ConsensusKind::PoW);
        let c1 = commitment_for(&pow, 10, 0, 1);
        let c2 = commitment_for(&pow, 10, 0, 1);
        assert_eq!(c1.leaf_hash(), c2.leaf_hash());

        let c3 = commitment_for(&pow, 10, 0, 2);
        assert_ne!(c1.leaf_hash(), c3.leaf_hash());
    }

    #[test]
    fn global_block_hash_chain_integrity_over_five_blocks() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        bc.register_consensus_domain(pow.clone()).unwrap();

        let mut prev_hash = [0u8; 32];
        for i in 0..5u64 {
            bc.submit_domain_commitment(commitment_for(&pow, i + 1, 0, (i + 1) as u8)).unwrap();
            let header = bc.seal_global_header(None).unwrap();
            assert_eq!(header.global_height, i);
            assert_eq!(header.previous_global_hash, prev_hash);
            prev_hash = header.calculate_hash_bytes();
        }
        assert_eq!(bc.global_headers.len(), 5);

        for i in 1..5 {
            assert_eq!(
                bc.global_headers[i].previous_global_hash,
                bc.global_headers[i - 1].calculate_hash_bytes()
            );
        }
    }

    #[test]
    fn full_bridge_lifecycle_lock_mint_burn_unlock_with_proof_verification() {
        let mut bc = test_chain();
        let pow = domain(1, ConsensusKind::PoW);
        let pos = domain(2, ConsensusKind::PoS);
        bc.register_consensus_domain(pow.clone()).unwrap();
        bc.register_consensus_domain(pos.clone()).unwrap();

        let asset = hash_fields_bytes(&[b"lifecycle-asset"]);
        let alice = Address::from([0xAA; 32]);
        let bob = Address::from([0xBB; 32]);
        bc.bridge_state.register_asset(asset, pow.id).unwrap();

        let (transfer, lock_event) = bc.bridge_state
            .lock(pow.id, pos.id, 100, 0, asset, alice, bob, 1000, 5000)
            .unwrap();

        let mut tree = DomainEventTree::new();
        tree.push(lock_event.clone());
        let mut commitment = commitment_for(&pow, 100, 0, 80);
        commitment.event_root = tree.root();
        bc.submit_domain_commitment(commitment).unwrap();

        let proof = tree.proof(0).unwrap();
        bc.mint_bridge_transfer_from_verified_event(
            pow.id, 100, 0, None, lock_event, &proof,
        ).unwrap();

        bc.burn_bridge_transfer(transfer.message_id, pos.id).unwrap();
        bc.unlock_bridge_transfer(transfer.message_id, pow.id).unwrap();

        let final_header = bc.seal_global_header(None).unwrap();
        assert_ne!(final_header.bridge_state_root, [0u8; 32]);
    }

    #[test]
    fn adapter_name_mismatch_blocks_all_consensus_types() {
        let mut bc = test_chain();

        let mut bft = bft_domain(30);
        bft.finality_adapter = "wrong-adapter".into();
        bc.register_consensus_domain(bft.clone()).unwrap();

        let proof = FinalityProof::Bft {
            round: 1, signer_count: 4, total_validators: 4,
            commit_hash: [1u8; 32],
        };
        let mut c = commitment_for(&bft, 1, 0, 30);
        c.consensus_kind = ConsensusKind::Bft;
        c.finality_proof_hash = hash_finality_proof(&proof);
        let err = bc.submit_verified_domain_commitment(c, proof).unwrap_err();
        assert!(err.contains("adapter mismatch"));
    }

    #[test]
    fn normalize_hash32_consistency_across_schemes() {
        use crate::domain::types::{normalize_hash32, RootScheme};

        let raw_32 = "ab".repeat(32);
        let n1 = normalize_hash32(b"tag", 1, &RootScheme::BudlumBlockV2, raw_32.as_bytes()).unwrap();
        let n2 = normalize_hash32(b"tag", 1, &RootScheme::Sha256, raw_32.as_bytes()).unwrap();
        assert_eq!(n1, n2);

        let short = b"short";
        let s1 = normalize_hash32(b"tag", 1, &RootScheme::BudlumBlockV2, short).unwrap();
        let s2 = normalize_hash32(b"tag", 1, &RootScheme::Sha256, short).unwrap();
        assert_ne!(s1, s2);

        let s3 = normalize_hash32(b"tag", 2, &RootScheme::Sha256, short).unwrap();
        assert_ne!(s2, s3);
    }
}
