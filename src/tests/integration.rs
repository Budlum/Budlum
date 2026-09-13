#[cfg(test)]
mod integration_tests {
    use crate::chain::blockchain::Blockchain;
    use crate::chain::finality::{
        checkpoint_signing_message, is_checkpoint_height, sign_bls, verify_bls_sig,
        FinalityAggregator, Precommit, Prevote, ValidatorEntry, ValidatorSetSnapshot,
    };
    use crate::consensus::poa::PoAConfig;
    use crate::consensus::pos::PoSConfig;
    use crate::consensus::{poa::PoAEngine, pos::PoSEngine, pow::PoWEngine, ConsensusEngine};
    use crate::core::account::{AccountState, Validator};
    use crate::core::address::Address;
    use crate::core::block::Block;
    use crate::core::governance::ProposalType;
    use crate::core::transaction::Transaction;
    use crate::crypto::primitives::BlsKeypair;
    use crate::crypto::primitives::KeyPair;
    use crate::crypto::primitives::ValidatorKeys;
    use crate::execution::executor::Executor;
    use std::sync::Arc;

    #[test]
    fn test_governance_full_lifecycle() {
        let mut state = AccountState::new();
        let val_kp = KeyPair::generate().unwrap();
        let val_addr = Address::from(val_kp.public_key_bytes());

        state.add_balance(&val_addr, 1000);
        state.add_validator(val_addr, 1000);

        let p_type = ProposalType::ChangeBaseFee(10);
        let mut prop_tx = Transaction::new_proposal(val_addr, p_type, 1, 0);
        prop_tx.sign(&val_kp);

        Executor::apply_transaction(&mut state, &prop_tx).unwrap();
        assert_eq!(state.governance.proposals.len(), 1);
        let prop_id = state.governance.proposals[0].id;

        let mut vote_tx = Transaction::new_vote(val_addr, prop_id, true, 1);
        vote_tx.sign(&val_kp);

        Executor::apply_transaction(&mut state, &vote_tx).unwrap();

        state.advance_epoch(1000); // 0 -> 1
        state.advance_epoch(2000); // 1 -> 2

        assert_eq!(
            state.governance.proposals[0].status,
            crate::core::governance::ProposalStatus::Executed
        );
    }

    #[test]
    fn test_poa_rejects_unsigned_block() {
        let keypair = KeyPair::generate().unwrap();
        let validator_pubkey = Address::from(keypair.public_key_bytes());

        let mut state = AccountState::new();
        state
            .validators
            .insert(validator_pubkey, Validator::new(validator_pubkey, 0));
        state.validators.get_mut(&validator_pubkey).unwrap().active = true;

        let config = PoAConfig::default();
        let engine = PoAEngine::new(config, Some(keypair));
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.hash = block.calculate_hash();

        let result = engine.validate_block(&block, &[], &state);
        assert!(result.is_err(), "Unsigned block should be rejected in PoA");
    }

    #[test]
    fn test_poa_rejects_forged_signature() {
        let validator_keypair = KeyPair::generate().unwrap();
        let validator_pubkey = Address::from(validator_keypair.public_key_bytes());

        let mut state = AccountState::new();
        state
            .validators
            .insert(validator_pubkey, Validator::new(validator_pubkey, 0));
        state.validators.get_mut(&validator_pubkey).unwrap().active = true;

        let config = PoAConfig::default();
        let engine = PoAEngine::new(config, Some(validator_keypair));

        let attacker_keypair = KeyPair::generate().unwrap();
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.producer = Some(validator_pubkey);
        block.sign(&attacker_keypair);

        let result = engine.validate_block(&block, &[], &state);
        assert!(result.is_err(), "Forged signature should be rejected");
    }

    #[test]
    fn test_pos_requires_signature() {
        let keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let keypair = keys.sig_key.clone();
        let validator_pubkey = Address::from(keypair.public_key_bytes());

        let mut state = AccountState::new();
        state.add_balance(&validator_pubkey, 2000);
        let mut validator = Validator::new(validator_pubkey, 1000);
        validator.active = true;
        state.validators.insert(validator_pubkey, validator);

        let config = PoSConfig {
            min_stake: 100,
            ..Default::default()
        };
        let engine = PoSEngine::new(config, Some(keys));

        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.producer = Some(validator_pubkey);
        block.hash = block.calculate_hash();

        let result = engine.validate_block(&block, &[], &state);
        assert!(result.is_err(), "PoS should reject unsigned blocks");
    }

    #[test]
    fn test_signed_transaction_flow() {
        let sender_keypair = KeyPair::generate().unwrap();
        let sender_pubkey = Address::from(sender_keypair.public_key_bytes());
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&sender_pubkey);

        let recipient = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut tx = Transaction::new(sender_pubkey, recipient, 100, vec![]);
        tx.fee = 1;
        tx.nonce = 0;
        tx.sign(&sender_keypair);

        let result = blockchain.add_transaction(tx);
        assert!(result.is_ok(), "Signed TX with balance should be accepted");

        let miner = Address::from_hex(&"03".repeat(32)).unwrap();
        blockchain.produce_block(miner);
        assert!(blockchain.is_valid());
        assert_eq!(blockchain.chain.len(), 2);
    }

    #[test]
    fn test_unsigned_transaction_rejected() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        let alice = Address::from_hex(&"01".repeat(32)).unwrap();
        let bob = Address::from_hex(&"02".repeat(32)).unwrap();
        let tx = Transaction::new(alice, bob, 100, vec![]);
        let result = blockchain.add_transaction(tx);
        assert!(result.is_err(), "Unsigned TX should be rejected");
    }

    #[test]
    fn test_insufficient_balance_rejected() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = Address::from(keypair.public_key_bytes());
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        let recipient = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut tx = Transaction::new(pubkey, recipient, 100, vec![]);
        tx.fee = 1;
        tx.nonce = 0;
        tx.sign(&keypair);

        let result = blockchain.add_transaction(tx);
        assert!(
            result.is_err(),
            "TX with insufficient balance should be rejected"
        );
    }

    #[test]
    fn test_replay_attack_protection() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = Address::from(keypair.public_key_bytes());
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);

        let recipient = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut tx1 = Transaction::new(pubkey, recipient, 10, vec![]);
        tx1.fee = 1;
        tx1.nonce = 0;
        tx1.sign(&keypair);

        blockchain.add_transaction(tx1.clone()).unwrap();
        let miner = Address::from_hex(&"03".repeat(32)).unwrap();
        blockchain.produce_block(miner);

        let result = blockchain.add_transaction(tx1);
        assert!(result.is_err(), "Replay attack should be prevented");
    }

    #[test]
    fn test_invalid_nonce_rejected() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = Address::from(keypair.public_key_bytes());
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);

        let recipient = Address::from_hex(&"02".repeat(32)).unwrap();
        let mut tx = Transaction::new(pubkey, recipient, 10, vec![]);
        tx.fee = 1;
        tx.nonce = 1;
        tx.sign(&keypair);

        let result = blockchain.add_transaction(tx);
        assert!(result.is_err(), "TX with invalid nonce should be rejected");
    }

    #[test]
    fn test_block_signature_verification() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = Address::from(keypair.public_key_bytes());
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.sign(&keypair);

        assert_eq!(block.producer.as_ref().unwrap(), &pubkey);
        assert!(block.verify_signature());

        let attacker = Address::from_hex(&"04".repeat(32)).unwrap();
        block
            .transactions
            .push(Transaction::new(attacker, attacker, 1000000, vec![]));
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();

        assert!(
            !block.verify_signature(),
            "Signature for old hash should fail verification"
        );
    }

    #[test]
    fn test_poa_round_robin_signed() {
        let keypair1 = KeyPair::generate().unwrap();
        let keypair2 = KeyPair::generate().unwrap();
        let pubkey1 = Address::from(keypair1.public_key_bytes());
        let pubkey2 = Address::from(keypair2.public_key_bytes());

        let mut state = AccountState::new();
        state.validators.insert(pubkey1, Validator::new(pubkey1, 0));
        state.validators.insert(pubkey2, Validator::new(pubkey2, 0));
        state.validators.get_mut(&pubkey1).unwrap().active = true;
        state.validators.get_mut(&pubkey2).unwrap().active = true;

        let config = PoAConfig {
            quorum_ratio: 0.66,
            block_period: 5,
            ..PoAConfig::default()
        };

        let engine = PoAEngine::new(config, Some(keypair1));

        let validators = state.get_active_validators();

        if validators.len() < 2 {
            return;
        }

        let expected = engine.expected_proposer(0, &validators).unwrap();

        assert!(state.validators.contains_key(&expected.address));

        let mut block = Block::new(0, "0".repeat(64), vec![]);

        let mut my_slot = 0;
        if expected.address != pubkey1 {
            my_slot = 1;
        }
        block.index = my_slot;

        let expected_my_slot = engine.expected_proposer(my_slot, &validators).unwrap();

        if expected_my_slot.address == pubkey1 {
            let result = engine.prepare_block(&mut block, &state);
            assert!(result.is_ok());
            assert!(block.signature.is_some());
        }
    }
    #[test]
    fn test_finality_checkpoint_enforcement() {
        use crate::chain::finality::FinalityCert;
        use crate::consensus::qc::{sign_attestation, QcBlob};

        let keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let sig_key = keys.sig_key.clone();
        let pubkey = Address::from(sig_key.public_key_bytes());
        let pq_key = keys.pq_key.clone().unwrap();

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);
        blockchain.state.validators.clear();

        let mut validator = crate::core::account::Validator::new(pubkey, 1000);
        validator.active = true;

        let mut sk_bytes = [0u8; 64];
        sk_bytes[0] = 42;
        let bls_sk = bls12_381::Scalar::from_bytes_wide(&sk_bytes);
        let bls_pk_point = bls12_381::G2Affine::from(bls12_381::G2Projective::generator() * bls_sk);
        let bls_pk = bls_pk_point.to_compressed().to_vec();

        validator.bls_public_key = bls_pk.clone();
        validator.pop_signature = vec![0u8; 48];
        validator.pq_public_key = pq_key.public_key_bytes().to_vec();
        blockchain.state.validators.insert(pubkey, validator);

        for _ in 1..=10 {
            blockchain.produce_block(pubkey);
        }

        let checkpoint_block = blockchain.chain[10].clone();

        let mut cert = FinalityCert {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: checkpoint_block.hash.clone(),
            agg_sig_bls: Vec::new(),
            bitmap: vec![0b0000_0001],
            set_hash: blockchain.get_validator_set_hash(),
        };

        let msg = cert.signing_message();
        let h_msg_point = crate::chain::finality::hash_to_g1(&msg);
        let sig_point = bls12_381::G1Projective::from(h_msg_point) * bls_sk;
        cert.agg_sig_bls = bls12_381::G1Affine::from(sig_point)
            .to_compressed()
            .to_vec();

        let qc_blob = QcBlob::new(
            cert.epoch,
            cert.checkpoint_height,
            cert.checkpoint_hash.clone(),
            vec![sign_attestation(
                &pq_key,
                cert.epoch,
                cert.checkpoint_height,
                &cert.checkpoint_hash,
                0,
                pubkey.to_string(),
            )
            .unwrap()],
        );
        let pending_result = blockchain.handle_finality_cert(cert.clone());
        assert!(pending_result.is_err());
        assert!(pending_result
            .unwrap_err()
            .contains("Missing verified QC blob"));
        assert_eq!(blockchain.finalized_height, 0);
        assert_eq!(
            blockchain
                .pending_finality_certs
                .get(&cert.checkpoint_height)
                .map(|certs| certs.len()),
            Some(1)
        );

        blockchain.import_qc_blob(qc_blob).unwrap();
        assert_eq!(blockchain.finalized_height, 10);
        assert_eq!(blockchain.finalized_hash, checkpoint_block.hash);
        assert!(!blockchain
            .pending_finality_certs
            .contains_key(&cert.checkpoint_height));

        let mut conflicting_block = Block::new(10, "wrong_prev".into(), vec![]);
        conflicting_block.hash = "conflicting_hash".into();
        conflicting_block.producer = Some(pubkey);
        conflicting_block.sign(&sig_key);

        let result = blockchain.validate_and_add_block(conflicting_block);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("conflicts with finalized checkpoint"));
    }

    #[test]
    fn test_invalid_qc_blob_import_does_not_slash_validator() {
        use crate::consensus::qc::{sign_attestation, QcBlob};

        let keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let sig_key = keys.sig_key.clone();
        let pubkey = Address::from(sig_key.public_key_bytes());
        let pq_key = keys.pq_key.clone().unwrap();

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);
        blockchain.state.validators.clear();

        let mut validator = crate::core::account::Validator::new(pubkey, 2_000);
        validator.active = true;
        validator.pq_public_key = pq_key.public_key_bytes().to_vec();
        blockchain.state.validators.insert(pubkey, validator);

        for _ in 1..=10 {
            blockchain.produce_block(pubkey);
        }

        let checkpoint_hash = blockchain.chain[10].hash.clone();
        let mut blob = QcBlob::new(
            1,
            10,
            checkpoint_hash.clone(),
            vec![
                sign_attestation(&pq_key, 1, 10, &checkpoint_hash, 0, pubkey.to_string()).unwrap(),
            ],
        );
        blob.pq_signatures[0].dilithium_signature[0] ^= 0x5A;
        blob.merkle_root = QcBlob::compute_merkle_root(&blob.pq_signatures);

        let result = blockchain.import_qc_blob(blob);

        assert!(result.is_err());
        assert!(blockchain.get_qc_blob(10).is_none());
        let validator = blockchain.state.get_validator(&pubkey).unwrap();
        assert!(!validator.slashed);
        assert_eq!(validator.stake, 2_000);
    }

    #[test]
    fn test_qc_blob_import_rejects_non_checkpoint_height() {
        use crate::consensus::qc::{sign_attestation, QcBlob};

        let keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let sig_key = keys.sig_key.clone();
        let pubkey = Address::from(sig_key.public_key_bytes());
        let pq_key = keys.pq_key.clone().unwrap();

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);
        blockchain.state.validators.clear();

        let mut validator = crate::core::account::Validator::new(pubkey, 2_000);
        validator.active = true;
        validator.pq_public_key = pq_key.public_key_bytes().to_vec();
        blockchain.state.validators.insert(pubkey, validator);

        for _ in 1..=9 {
            blockchain.produce_block(pubkey);
        }

        let block = blockchain.chain[9].clone();
        let blob = QcBlob::new(
            1,
            9,
            block.hash.clone(),
            vec![sign_attestation(&pq_key, 1, 9, &block.hash, 0, pubkey.to_string()).unwrap()],
        );

        let result = blockchain.import_qc_blob(blob);

        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("not a valid checkpoint height"));
        assert!(blockchain.get_qc_blob(9).is_none());
    }

    #[test]
    fn test_qc_fault_proof_invalidates_cert_without_slashing() {
        use crate::chain::finality::{FinalityCert, ValidatorEntry, ValidatorSetSnapshot};
        use crate::consensus::qc::{sign_attestation, QcBlob};

        let keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let sig_key = keys.sig_key.clone();
        let pq_key = keys.pq_key.clone().unwrap();
        let pubkey = Address::from(sig_key.public_key_bytes());

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);
        blockchain.state.validators.clear();

        let mut validator = crate::core::account::Validator::new(pubkey, 2_000);
        validator.active = true;
        validator.pq_public_key = pq_key.public_key_bytes().to_vec();

        let mut sk_bytes = [0u8; 64];
        sk_bytes[0] = 7;
        let bls_sk = bls12_381::Scalar::from_bytes_wide(&sk_bytes);
        let bls_pk_point = bls12_381::G2Affine::from(bls12_381::G2Projective::generator() * bls_sk);
        validator.bls_public_key = bls_pk_point.to_compressed().to_vec();
        validator.pop_signature = vec![0u8; 48];
        blockchain.state.validators.insert(pubkey, validator);

        for _ in 1..=10 {
            blockchain.produce_block(pubkey);
        }

        let checkpoint_block = blockchain.chain[10].clone();
        let mut cert = FinalityCert {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: checkpoint_block.hash.clone(),
            agg_sig_bls: Vec::new(),
            bitmap: vec![0b0000_0001],
            set_hash: blockchain.get_validator_set_hash(),
        };

        let msg = cert.signing_message();
        let h_msg_point = crate::chain::finality::hash_to_g1(&msg);
        let sig_point = bls12_381::G1Projective::from(h_msg_point) * bls_sk;
        cert.agg_sig_bls = bls12_381::G1Affine::from(sig_point)
            .to_compressed()
            .to_vec();

        let valid_blob = QcBlob::new(
            cert.epoch,
            cert.checkpoint_height,
            cert.checkpoint_hash.clone(),
            vec![sign_attestation(
                &pq_key,
                cert.epoch,
                cert.checkpoint_height,
                &cert.checkpoint_hash,
                0,
                pubkey.to_string(),
            )
            .unwrap()],
        );
        blockchain.import_qc_blob(valid_blob.clone()).unwrap();
        blockchain.handle_finality_cert(cert).unwrap();
        assert_eq!(blockchain.finalized_height, 10);

        let snapshot = ValidatorSetSnapshot::new(
            1,
            vec![ValidatorEntry {
                address: pubkey,
                stake: 2_000,
                bls_public_key: bls_pk_point.to_compressed().to_vec(),
                pop_signature: vec![0u8; 48],
                pq_public_key: pq_key.public_key_bytes().to_vec(),
            }],
        );

        let mut invalid_blob = valid_blob.clone();
        invalid_blob.pq_signatures[0].dilithium_signature[0] ^= 0x5A;
        invalid_blob.merkle_root = QcBlob::compute_merkle_root(&invalid_blob.pq_signatures);
        let proof = invalid_blob.detect_fault_proofs(&snapshot).pop().unwrap();

        blockchain
            .verified_qc_blobs
            .insert(invalid_blob.checkpoint_height, invalid_blob);

        blockchain.handle_qc_fault_proof(proof).unwrap();

        let validator = blockchain.state.get_validator(&pubkey).unwrap();
        assert!(!validator.slashed);
        assert_eq!(validator.stake, 2_000);
        assert_eq!(blockchain.finalized_height, 0);
    }

    #[test]
    #[should_panic(expected = "Startup Chain ID mismatch!")]
    fn test_startup_chain_id_verification_fails() {
        use crate::storage::db::Storage;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();

        let storage = Storage::new(path).unwrap();
        let mut genesis_block = Block::new(0, "0".repeat(64), vec![]);
        genesis_block.chain_id = 100;
        genesis_block.hash = genesis_block.calculate_hash();
        storage.insert_block(&genesis_block).unwrap();
        storage.save_last_hash(&genesis_block.hash).unwrap();
        storage.save_canonical_height(0).unwrap();

        drop(storage);

        let consensus = Arc::new(PoAEngine::new(PoAConfig::default(), None));
        let storage2 = Storage::new(path).unwrap();
        let _bc = Blockchain::new(consensus, Some(storage2), 1337, None);
    }

    #[test]
    fn test_prevote_precommit_full_lifecycle() {
        use crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
        let keypair = KeyPair::generate().unwrap();
        let validator_addr = Address::from(keypair.public_key_bytes());

        let mut state = AccountState::new();
        state.add_balance(&validator_addr, 10000);
        state.add_validator(validator_addr, 5000);

        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state = state;

        let cp_height = FINALITY_CHECKPOINT_INTERVAL;
        for _ in 1..cp_height {
            bc.produce_block(validator_addr);
        }
        let block = bc.produce_block(validator_addr).unwrap();
        assert_eq!(block.index, cp_height);
        assert!(is_checkpoint_height(block.index));

        bc.start_prevote_phase(block.index, block.hash.clone());
        assert!(bc.finality_aggregator.is_some());

        let epoch = block.epoch;
        let hash = block.hash.clone();

        let vote1 = Prevote {
            epoch,
            checkpoint_height: cp_height,
            checkpoint_hash: hash.clone(),
            voter_id: validator_addr,
            sig_bls: vec![],
        };
        assert!(bc.handle_prevote(vote1).is_ok());

        let vote2 = Precommit {
            epoch,
            checkpoint_height: cp_height,
            checkpoint_hash: hash.clone(),
            voter_id: validator_addr,
            sig_bls: vec![],
        };
        let result = bc.handle_precommit(vote2);
        assert!(
            result.is_ok(),
            "single validator should reach both prevote and precommit quorum"
        );
    }

    #[test]
    fn test_prevote_rejects_wrong_checkpoint_hash() {
        let keypair = KeyPair::generate().unwrap();
        let v_addr = Address::from(keypair.public_key_bytes());

        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state.add_balance(&v_addr, 10000);
        bc.state.add_validator(v_addr, 5000);

        let cp_height = crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
        for _ in 1..cp_height {
            bc.produce_block(v_addr);
        }
        let _block = bc.produce_block(v_addr).unwrap();

        bc.start_prevote_phase(cp_height, "correct_hash".to_string());

        let wrong_vote = Prevote {
            epoch: 0,
            checkpoint_height: cp_height,
            checkpoint_hash: "wrong_hash".to_string(),
            voter_id: v_addr,
            sig_bls: vec![],
        };
        let result = bc.handle_prevote(wrong_vote);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("hash mismatch"));
    }

    fn make_validator_keys_with_bls() -> ValidatorKeys {
        let mut keys = ValidatorKeys::generate().unwrap();
        // Ensure BLS key exists
        if keys.bls_key.is_none() {
            keys.bls_key = Some(BlsKeypair::generate().unwrap());
        }
        keys
    }

    fn make_validator_snapshot(
        addrs: &[(Address, u64)],
        bls_keys: &[BlsKeypair],
    ) -> ValidatorSetSnapshot {
        let entries: Vec<ValidatorEntry> = addrs
            .iter()
            .enumerate()
            .map(|(i, (addr, stake))| {
                let pop_msg =
                    crate::chain::finality::pop_signing_message(addr, &bls_keys[i].public_key);
                let pop_sig = sign_bls(&bls_keys[i].secret_key, &pop_msg);
                ValidatorEntry {
                    address: *addr,
                    stake: *stake,
                    bls_public_key: bls_keys[i].public_key.clone(),
                    pop_signature: pop_sig,
                    pq_public_key: vec![],
                }
            })
            .collect();
        ValidatorSetSnapshot::new(1, entries)
    }

    #[test]
    fn test_bls_sign_prevote_and_verify() {
        let bls_key = BlsKeypair::generate().unwrap();
        let voter = Address::from([1u8; 32]);

        let vote = Prevote {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "test_cp_hash".to_string(),
            voter_id: voter,
            sig_bls: vec![],
        };

        let msg = vote.signing_message();
        let sig = sign_bls(&bls_key.secret_key, &msg);
        assert_eq!(sig.len(), 48);

        let result = verify_bls_sig(&bls_key.public_key, &msg, &sig);
        assert!(result.is_ok());

        // Wrong message should fail verification
        let wrong_msg = b"wrong message";
        let result = verify_bls_sig(&bls_key.public_key, wrong_msg, &sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_bls_sign_precommit_and_verify() {
        let bls_key = BlsKeypair::generate().unwrap();

        let msg = checkpoint_signing_message(1, 10, "test_cp_hash");
        let sig = sign_bls(&bls_key.secret_key, &msg);
        assert_eq!(sig.len(), 48);

        assert!(verify_bls_sig(&bls_key.public_key, &msg, &sig).is_ok());
    }

    #[test]
    fn test_bls_signer_in_pos_engine() {
        let keys = make_validator_keys_with_bls();
        let bls_sk = keys.bls_key.as_ref().unwrap().secret_key;
        let bls_pk = keys.bls_key.as_ref().unwrap().public_key.clone();

        let engine = PoSEngine::new(PoSConfig::default(), Some(keys));

        let retrieved_sk = engine.bls_secret_key();
        assert!(retrieved_sk.is_some());
        assert_eq!(retrieved_sk.unwrap(), bls_sk);

        let retrieved_pk = engine.bls_public_key();
        assert!(retrieved_pk.is_some());
        assert_eq!(retrieved_pk.unwrap(), bls_pk);
    }

    #[test]
    fn test_blockchain_sign_prevote_with_bls() {
        let keys = make_validator_keys_with_bls();
        let bls_pk = keys.bls_key.as_ref().unwrap().public_key.clone();
        let voter = Address::from(keys.sig_key.public_key_bytes());

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let bc = Blockchain::new(consensus, None, 1337, None);

        let prevote = bc.sign_prevote(1, 10, "test_cp_hash", &voter).unwrap();

        assert_eq!(prevote.epoch, 1);
        assert_eq!(prevote.checkpoint_height, 10);
        assert_eq!(prevote.checkpoint_hash, "test_cp_hash");
        assert_eq!(prevote.voter_id, voter);
        assert_eq!(prevote.sig_bls.len(), 48);

        // Verify the BLS signature
        let msg = prevote.signing_message();
        assert!(verify_bls_sig(&bls_pk, &msg, &prevote.sig_bls).is_ok());
    }

    #[test]
    fn test_blockchain_sign_precommit_with_bls() {
        let keys = make_validator_keys_with_bls();
        let bls_pk = keys.bls_key.as_ref().unwrap().public_key.clone();
        let voter = Address::from(keys.sig_key.public_key_bytes());

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let bc = Blockchain::new(consensus, None, 1337, None);

        let precommit = bc.sign_precommit(1, 10, "test_cp_hash", &voter).unwrap();

        assert_eq!(precommit.epoch, 1);
        assert_eq!(precommit.checkpoint_height, 10);
        assert_eq!(precommit.checkpoint_hash, "test_cp_hash");
        assert_eq!(precommit.voter_id, voter);
        assert_eq!(precommit.sig_bls.len(), 48);

        let msg = precommit.signing_message();
        assert!(verify_bls_sig(&bls_pk, &msg, &precommit.sig_bls).is_ok());
    }

    #[test]
    fn test_full_finality_flow_with_bls_signatures() {
        let n_validators = 4;
        let mut bls_keys = Vec::new();
        let mut addrs = Vec::new();

        for i in 0..n_validators {
            let bls_key = BlsKeypair::generate().unwrap();
            let mut addr_bytes = [0u8; 32];
            addr_bytes[0] = (i + 1) as u8;
            let addr = Address::from(addr_bytes);
            bls_keys.push(bls_key);
            addrs.push((addr, 1000u64));
        }

        let snapshot = make_validator_snapshot(&addrs, &bls_keys);

        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snapshot.clone());

        // 3 out of 4 validators send BLS-signed prevotes (meets 2/3 quorum)
        for i in 0..3 {
            let vote = Prevote {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: addrs[i].0,
                sig_bls: vec![],
            };
            let msg = vote.signing_message();
            let sig = sign_bls(&bls_keys[i].secret_key, &msg);

            let signed_vote = Prevote {
                sig_bls: sig,
                ..vote
            };
            agg.add_prevote(signed_vote).unwrap();
        }

        assert!(agg.prevote_quorum_reached);

        // 3 validators send BLS-signed precommits
        for i in 0..3 {
            let pc = Precommit {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: addrs[i].0,
                sig_bls: vec![],
            };
            let msg = pc.signing_message();
            let sig = sign_bls(&bls_keys[i].secret_key, &msg);

            let signed_pc = Precommit { sig_bls: sig, ..pc };
            agg.add_precommit(signed_pc).unwrap();
        }

        assert!(agg.precommit_quorum_reached);

        let cert = agg.try_produce_cert().unwrap();
        assert_eq!(cert.epoch, 1);
        assert_eq!(cert.checkpoint_height, 10);
        assert_eq!(cert.checkpoint_hash, "cp_hash");
        assert_eq!(cert.signer_count(4), 3);

        // Verify the produced certificate
        assert!(cert.verify(&snapshot).is_ok());

        // Verify signer bitmap
        for (i, (addr, _stake)) in addrs.iter().enumerate().take(3) {
            let idx = snapshot.validator_index(addr).unwrap();
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            assert!(
                cert.bitmap[byte_idx] & (1 << bit_idx) != 0,
                "Validator {} should be in bitmap",
                i
            );
        }
    }

    #[test]
    fn test_aggregator_state_reporting() {
        let bls_key = BlsKeypair::generate().unwrap();
        let mut addr_bytes = [0u8; 32];
        addr_bytes[0] = 1;
        let addr = Address::from(addr_bytes);

        let snapshot = make_validator_snapshot(&[(addr, 2000)], std::slice::from_ref(&bls_key));

        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snapshot);

        let state = agg.get_state();
        assert!(state.active);
        assert_eq!(state.epoch, 1);
        assert_eq!(state.checkpoint_height, 10);
        assert!(!state.prevote_quorum_reached);
        assert!(!state.precommit_quorum_reached);
        assert_eq!(state.prevote_count, 0);
        assert_eq!(state.precommit_count, 0);

        // Add prevote
        let vote = Prevote {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: addr,
            sig_bls: vec![],
        };
        let msg = vote.signing_message();
        let sig = sign_bls(&bls_key.secret_key, &msg);
        agg.add_prevote(Prevote {
            sig_bls: sig,
            ..vote
        })
        .unwrap();

        let state = agg.get_state();
        assert!(state.prevote_quorum_reached);
        assert_eq!(state.prevote_count, 1);
    }

    #[test]
    fn test_byzantine_equivocating_prevote_rejected() {
        let bls_key = BlsKeypair::generate().unwrap();
        let mut addr_bytes = [0u8; 32];
        addr_bytes[0] = 1;
        let addr = Address::from(addr_bytes);

        let snapshot = make_validator_snapshot(&[(addr, 1000)], std::slice::from_ref(&bls_key));

        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snapshot);

        let vote1 = Prevote {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: addr,
            sig_bls: vec![],
        };
        let msg = vote1.signing_message();
        let sig = sign_bls(&bls_key.secret_key, &msg);

        agg.add_prevote(Prevote {
            sig_bls: sig.clone(),
            ..vote1.clone()
        })
        .unwrap();

        // Duplicate prevote from same validator should be rejected
        let result = agg.add_prevote(Prevote {
            sig_bls: sig,
            ..vote1
        });
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Duplicate"));
    }

    #[test]
    fn test_precommit_before_prevote_quorum_rejected() {
        let bls_key = BlsKeypair::generate().unwrap();
        let mut addr_bytes = [0u8; 32];
        addr_bytes[0] = 1;
        let addr = Address::from(addr_bytes);

        let snapshot = make_validator_snapshot(&[(addr, 1000)], std::slice::from_ref(&bls_key));

        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snapshot);

        let pc = Precommit {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: addr,
            sig_bls: vec![],
        };
        let msg = pc.signing_message();
        let sig = sign_bls(&bls_key.secret_key, &msg);

        let result = agg.add_precommit(Precommit { sig_bls: sig, ..pc });
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("Cannot precommit before prevote quorum"));
    }

    #[test]
    fn test_finality_cert_rejects_tampered_aggregate_signature() {
        let n = 4;
        let mut bls_keys = Vec::new();
        let mut addrs = Vec::new();
        for i in 0..n {
            let bls_key = BlsKeypair::generate().unwrap();
            let mut addr_bytes = [0u8; 32];
            addr_bytes[0] = (i + 1) as u8;
            addrs.push((Address::from(addr_bytes), 1000u64));
            bls_keys.push(bls_key);
        }
        let snapshot = make_validator_snapshot(&addrs, &bls_keys);

        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snapshot.clone());

        for i in 0..3 {
            let vote = Prevote {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: addrs[i].0,
                sig_bls: vec![],
            };
            let msg = vote.signing_message();
            let sig = sign_bls(&bls_keys[i].secret_key, &msg);
            agg.add_prevote(Prevote {
                sig_bls: sig,
                ..vote
            })
            .unwrap();
        }

        for i in 0..3 {
            let pc = Precommit {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: addrs[i].0,
                sig_bls: vec![],
            };
            let msg = pc.signing_message();
            let sig = sign_bls(&bls_keys[i].secret_key, &msg);
            agg.add_precommit(Precommit { sig_bls: sig, ..pc }).unwrap();
        }

        let mut cert = agg.try_produce_cert().unwrap();
        assert!(cert.verify(&snapshot).is_ok());

        // Tamper with the aggregated signature (modify middle bytes of the G1 compressed point)
        if cert.agg_sig_bls.len() > 16 {
            cert.agg_sig_bls[10] ^= 0xFF;
        }
        let result = cert.verify(&snapshot);
        assert!(
            result.is_err(),
            "Tampered cert should fail verification, got: {:?}",
            result
        );
    }

    #[test]
    fn test_no_bls_key_returns_none() {
        // PoW engine has no BLS key
        let engine = PoWEngine::new(0);
        assert!(engine.bls_secret_key().is_none());
        assert!(engine.bls_public_key().is_none());

        // PoA engine without keys has no BLS key
        let poa = PoAEngine::new(PoAConfig::default(), None);
        assert!(poa.bls_secret_key().is_none());
    }

    #[test]
    fn test_sign_prevote_fails_without_bls_key() {
        let keypair = KeyPair::generate().unwrap();
        let voter = Address::from(keypair.public_key_bytes());

        // PoW has no BLS key
        let consensus = Arc::new(PoWEngine::new(0));
        let bc = Blockchain::new(consensus, None, 1337, None);

        let result = bc.sign_prevote(1, 10, "test", &voter);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("No BLS secret key"));
    }

    // --- P2P Hardening Tests ---

    #[test]
    fn test_peer_manager_durable_ban_roundtrip() {
        use crate::network::peer_manager::PeerManager;
        let mut pm = PeerManager::new();

        let banned_ids: Vec<String> = (0..3)
            .map(|_| libp2p::PeerId::random().to_base58())
            .collect();

        pm.reload_banned_peers(&banned_ids);

        let persisted = pm.get_persisted_banned_peers();
        assert_eq!(persisted.len(), 3);
        for id in &banned_ids {
            let pid = id.parse::<libp2p::PeerId>().unwrap();
            assert!(pm.is_banned(&pid));
        }
    }

    #[test]
    fn test_peer_manager_unban_clears_ban() {
        use crate::network::peer_manager::PeerManager;
        let mut pm = PeerManager::new();
        let pid = libp2p::PeerId::random();

        let banned = vec![pid.to_base58()];
        pm.reload_banned_peers(&banned);
        assert!(pm.is_banned(&pid));

        pm.unban_peer(&pid);
        assert!(!pm.is_banned(&pid));
        assert_eq!(pm.get_score(&pid), 0);
    }

    #[test]
    fn test_persistent_identity_load_or_generate() {
        use crate::network::node::load_or_generate_identity_key;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let path = dir.path().join("test-node-id.key");
        let path_str = path.to_str().unwrap();

        // First call: generates and saves
        let key1 = load_or_generate_identity_key(Some(path_str));
        let peer_id1 = libp2p::PeerId::from(key1.public());

        assert!(path.exists());

        // Second call: loads existing key
        let key2 = load_or_generate_identity_key(Some(path_str));
        let peer_id2 = libp2p::PeerId::from(key2.public());

        assert_eq!(
            peer_id1, peer_id2,
            "Persistent identity should survive reload"
        );
    }

    #[test]
    fn test_dns_seed_resolution_resolves_addresses() {
        use crate::network::node::resolve_dns_seeds;

        let addrs = resolve_dns_seeds(&["localhost".to_string()], 4001);
        assert!(!addrs.is_empty(), "localhost should resolve");
        for addr in &addrs {
            assert!(addr.starts_with("/ip4/") || addr.starts_with("/ip6/"));
        }
    }

    #[test]
    fn test_dns_seed_resolution_handles_invalid() {
        use crate::network::node::resolve_dns_seeds;

        let addrs = resolve_dns_seeds(&["invalid.nonexistent.domain.test".to_string()], 4001);
        // Should not panic, returns empty
        assert!(addrs.is_empty());
    }

    #[test]
    fn test_rpc_operator_default_local_only() {
        let config = crate::rpc::RpcSecurityConfig::operator_default();
        assert!(config.allowed_ips.contains(&"127.0.0.1".to_string()));
        assert!(config.allowed_ips.contains(&"::1".to_string()));
        assert!(!config.auth_required);
        assert!(config.max_connections.is_some());
        assert!(config.max_connections.unwrap() > 0);
    }

    // --- V2 Snapshot / Replay Equivalence Tests ---

    #[test]
    fn test_v2_snapshot_preserves_consensus_metadata() {
        use crate::chain::snapshot::{StateSnapshotV2, StateSnapshotV2Params};
        use crate::core::account::AccountState;

        let mut state = AccountState::new();
        let addr = Address::from([1u8; 32]);
        state.add_balance(&addr, 5000);
        state.epoch_index = 42;
        state.base_fee = 15;
        state.block_reward = 100;
        state.bridge_root = [0xAB; 32];
        state.message_root = [0xCD; 32];

        let params = StateSnapshotV2Params {
            height: 200,
            block_hash: "block_hash_v2".into(),
            genesis_hash: "genesis_hash".into(),
            chain_id: 1337,
            finalized_height: 100,
            finalized_hash: "final_hash".into(),
            finality_certificates: vec![],
        };

        let v2 = StateSnapshotV2::from_state(&state, params);
        assert_eq!(v2.schema_version, 2);
        assert_eq!(v2.height, 200);
        assert_eq!(v2.epoch_index, 42);
        assert_eq!(v2.base_fee, 15);
        assert_eq!(v2.block_reward, 100);
        assert_eq!(v2.bridge_root, [0xAB; 32]);
        assert_eq!(v2.message_root, [0xCD; 32]);
        assert!(v2.verify());
    }

    #[test]
    fn test_v2_snapshot_restore_replay_equivalent() {
        use crate::chain::snapshot::{StateSnapshotV2, StateSnapshotV2Params};
        use crate::core::account::AccountState;

        let mut original = AccountState::new();
        let addr = Address::from([1u8; 32]);
        original.add_balance(&addr, 10000);
        original.epoch_index = 10;
        original.base_fee = 20;
        original.block_reward = 75;
        original
            .unbonding_queue
            .push(crate::core::account::UnbondingEntry {
                address: addr,
                amount: 500,
                release_epoch: 15,
            });

        let params = StateSnapshotV2Params {
            height: 100,
            block_hash: "h".into(),
            genesis_hash: "g".into(),
            chain_id: 1,
            finalized_height: 50,
            finalized_hash: "fh".into(),
            finality_certificates: vec![],
        };

        let v2 = StateSnapshotV2::from_state(&original, params);
        let mut restored = AccountState::from_snapshot_v2(&v2);

        assert_eq!(restored.epoch_index, original.epoch_index);
        assert_eq!(restored.base_fee, original.base_fee);
        assert_eq!(restored.block_reward, original.block_reward);
        assert_eq!(restored.unbonding_queue.len(), 1);
        assert_eq!(restored.unbonding_queue[0].amount, 500);
        assert_eq!(restored.unbonding_queue[0].release_epoch, 15);
        assert_eq!(restored.get_balance(&addr), 10000);

        let state_root_original = original.calculate_state_root();
        let state_root_restored = restored.calculate_state_root();
        assert_eq!(
            state_root_original, state_root_restored,
            "State root must be identical after V2 snapshot roundtrip"
        );
    }

    #[test]
    fn test_v1_snapshot_roundtrip() {
        use crate::chain::snapshot::StateSnapshot;

        let mut state = crate::core::account::AccountState::new();
        let addr = Address::from([1u8; 32]);
        state.add_balance(&addr, 3000);

        let snapshot =
            StateSnapshot::from_state(50, "test_hash".into(), 1337, &state, 10, "fin_hash".into());

        assert!(snapshot.verify());
        let bytes = snapshot.to_bytes();
        let parsed = StateSnapshot::from_bytes(&bytes).unwrap();
        assert!(parsed.verify());
        assert_eq!(parsed.height, 50);
    }

    #[test]
    fn test_v2_snapshot_serialization_roundtrip() {
        use crate::chain::snapshot::{StateSnapshotV2, StateSnapshotV2Params};

        let mut state = crate::core::account::AccountState::new();
        state.add_balance(&Address::from([2u8; 32]), 7000);

        let params = StateSnapshotV2Params {
            height: 300,
            block_hash: "bh".into(),
            genesis_hash: "gh".into(),
            chain_id: 42,
            finalized_height: 200,
            finalized_hash: "fh".into(),
            finality_certificates: vec![],
        };

        let v2 = StateSnapshotV2::from_state(&state, params);
        let bytes = v2.to_bytes();
        let parsed = StateSnapshotV2::from_bytes(&bytes).unwrap();
        assert_eq!(parsed.schema_version, 2);
        assert_eq!(parsed.height, 300);
        assert_eq!(parsed.chain_id, 42);
        assert!(parsed.verify());
    }

    #[test]
    fn test_pruning_manager_v2_save_and_load() {
        use crate::chain::snapshot::{PruningManager, StateSnapshotV2, StateSnapshotV2Params};
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let pm = PruningManager::new(100, 10, dir.path().to_str().unwrap().to_string());

        let mut state = crate::core::account::AccountState::new();
        state.add_balance(&Address::from([3u8; 32]), 5000);

        let params = StateSnapshotV2Params {
            height: 100,
            block_hash: "test".into(),
            genesis_hash: "gen".into(),
            chain_id: 1337,
            finalized_height: 50,
            finalized_hash: "fin".into(),
            finality_certificates: vec![],
        };

        let v2 = StateSnapshotV2::from_state(&state, params);
        pm.save_snapshot_v2(&v2).unwrap();

        let loaded = pm.load_latest_snapshot_v2().unwrap().unwrap();
        assert_eq!(loaded.height, 100);
        assert_eq!(loaded.chain_id, 1337);
        assert!(loaded.verify());
    }

    // --- RpcMode / Health Endpoint Tests ---

    #[test]
    fn test_rpc_mode_public_vs_operator_defaults() {
        use crate::rpc::RpcSecurityConfig;

        let public_default = RpcSecurityConfig::default();
        let operator_default = RpcSecurityConfig::operator_default();

        assert!(operator_default
            .allowed_ips
            .contains(&"127.0.0.1".to_string()));
        assert!(!operator_default.auth_required);
        assert_eq!(operator_default.max_connections, Some(10));
        assert!(operator_default.max_request_body_size.is_some());

        assert!(public_default.allowed_ips.is_empty());
        assert!(!public_default.auth_required);
        assert_eq!(public_default.max_connections, None);
    }

    #[test]
    fn test_rpc_security_config_with_body_limits() {
        use crate::rpc::RpcSecurityConfig;

        let config = RpcSecurityConfig {
            max_request_body_size: Some(1024),
            max_connections: Some(50),
            ..Default::default()
        };

        assert_eq!(config.max_request_body_size, Some(1024));
        assert_eq!(config.max_connections, Some(50));
    }

    // --- mDNS Policy / Node Construction Tests ---

    #[tokio::test]
    async fn test_mdns_policy_flag_is_stored() {
        use crate::chain::blockchain::Blockchain;
        use crate::consensus::pow::PoWEngine;
        use crate::network::node::Node;

        let consensus = std::sync::Arc::new(PoWEngine::new(0));
        let bc = Blockchain::new(consensus, None, 1337, None);
        let (_actor, handle) = crate::chain::chain_actor::ChainActor::new(bc);

        let mdns_on = Node::with_key(
            handle.clone(),
            libp2p::identity::Keypair::generate_ed25519(),
            true,
        )
        .unwrap();
        assert!(mdns_on.mdns_enabled);

        let mdns_off =
            Node::with_key(handle, libp2p::identity::Keypair::generate_ed25519(), false).unwrap();
        assert!(!mdns_off.mdns_enabled);
    }

    #[tokio::test]
    async fn test_node_with_identity_sets_path() {
        use crate::chain::blockchain::Blockchain;
        use crate::consensus::pow::PoWEngine;
        use crate::network::node::Node;

        let consensus = std::sync::Arc::new(PoWEngine::new(0));
        let bc = Blockchain::new(consensus, None, 1337, None);
        let (_actor, handle) = crate::chain::chain_actor::ChainActor::new(bc);

        let node = Node::new(handle)
            .unwrap()
            .with_identity(Some("/tmp/test-id.key".to_string()));

        assert!(node.identity_path.is_some());
        assert_eq!(
            node.identity_path.unwrap().to_str().unwrap(),
            "/tmp/test-id.key"
        );
    }

    // --- Metrics Wiring Tests ---

    #[test]
    fn test_blockchain_emit_chain_metrics_updates_gauges() {
        use crate::chain::blockchain::Blockchain;
        use crate::consensus::pow::PoWEngine;
        use crate::core::metrics::Metrics;
        use std::sync::Arc;

        let consensus = Arc::new(PoWEngine::new(0));
        let metrics = Arc::new(Metrics::new());
        let bc = Blockchain::new(consensus.clone(), None, 1337, None).with_metrics(metrics.clone());

        bc.emit_chain_metrics();
        assert_eq!(metrics.chain_height.get(), 1); // genesis block
        assert_eq!(metrics.blocks_produced.get(), 1);
    }

    #[test]
    fn test_blockchain_emit_tx_processed_increments_counter() {
        use crate::chain::blockchain::Blockchain;
        use crate::consensus::pow::PoWEngine;
        use crate::core::metrics::Metrics;
        use std::sync::Arc;

        let consensus = Arc::new(PoWEngine::new(0));
        let metrics = Arc::new(Metrics::new());
        let bc = Blockchain::new(consensus.clone(), None, 1337, None).with_metrics(metrics.clone());

        bc.emit_tx_processed(5);
        assert_eq!(metrics.transactions_processed.get(), 5);

        bc.emit_tx_processed(3);
        assert_eq!(metrics.transactions_processed.get(), 8);
    }

    #[test]
    fn test_blockchain_emit_reorg_increments() {
        use crate::chain::blockchain::Blockchain;
        use crate::consensus::pow::PoWEngine;
        use crate::core::metrics::Metrics;
        use std::sync::Arc;

        let consensus = Arc::new(PoWEngine::new(0));
        let metrics = Arc::new(Metrics::new());
        let bc = Blockchain::new(consensus.clone(), None, 1337, None).with_metrics(metrics.clone());

        assert_eq!(metrics.reorgs_total.get(), 0);
        bc.emit_reorg();
        assert_eq!(metrics.reorgs_total.get(), 1);
    }

    #[test]
    fn test_blockchain_emit_mempool_events() {
        use crate::chain::blockchain::Blockchain;
        use crate::consensus::pow::PoWEngine;
        use crate::core::metrics::Metrics;
        use std::sync::Arc;

        let consensus = Arc::new(PoWEngine::new(0));
        let metrics = Arc::new(Metrics::new());
        let bc = Blockchain::new(consensus.clone(), None, 1337, None).with_metrics(metrics.clone());

        bc.emit_mempool_eviction();
        bc.emit_mempool_eviction();
        assert_eq!(metrics.mempool_evictions.get(), 2);

        bc.emit_mempool_cleanup();
        assert_eq!(metrics.mempool_expired_cleanups.get(), 1);
    }

    #[test]
    fn test_metrics_default_encodes_help_text() {
        use crate::core::metrics::Metrics;

        let metrics = Metrics::new();
        metrics.chain_height.set(42);
        let encoded = metrics.encode();

        assert!(encoded.contains("budlum_chain_height 42"));
        assert!(encoded.contains("# HELP budlum_chain_height"));
    }

    // --- ConsensusEngine bls_secret_key defaults ---

    #[test]
    fn test_pow_engine_bls_key_is_none() {
        use crate::consensus::pow::PoWEngine;
        use crate::consensus::ConsensusEngine;

        let engine = PoWEngine::new(2);
        assert!(engine.bls_secret_key().is_none());
        assert!(engine.bls_public_key().is_none());
    }

    #[test]
    fn test_poa_engine_bls_key_is_none_by_default() {
        use crate::consensus::poa::{PoAConfig, PoAEngine};
        use crate::consensus::ConsensusEngine;

        let engine = PoAEngine::new(PoAConfig::default(), None);
        assert!(engine.bls_secret_key().is_none());
        assert!(engine.bls_public_key().is_none());
    }

    #[test]
    fn test_pos_engine_bls_key_is_some_when_validator_keys_have_bls() {
        use crate::consensus::pos::{PoSConfig, PoSEngine};
        use crate::consensus::ConsensusEngine;
        use crate::crypto::primitives::ValidatorKeys;

        let keys = ValidatorKeys::generate().unwrap();
        assert!(keys.bls_key.is_some());

        let engine = PoSEngine::new(PoSConfig::default(), Some(keys));
        assert!(engine.bls_secret_key().is_some());
        assert!(engine.bls_public_key().is_some());
    }
}
