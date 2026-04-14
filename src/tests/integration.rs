#[cfg(test)]
mod integration_tests {
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::poa::PoAConfig;
    use crate::consensus::pos::PoSConfig;
    use crate::consensus::{poa::PoAEngine, pos::PoSEngine, pow::PoWEngine, ConsensusEngine};
    use crate::core::account::{AccountState, Validator};
    use crate::core::address::Address;
    use crate::core::block::Block;
    use crate::core::governance::ProposalType;
    use crate::core::transaction::Transaction;
    use crate::crypto::primitives::KeyPair;
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
        use crate::chain::finality::{FinalityCert, ValidatorEntry};

        let keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let sig_key = keys.sig_key.clone();
        let pubkey = Address::from(sig_key.public_key_bytes());

        let consensus = Arc::new(PoSEngine::new(PoSConfig::default(), Some(keys)));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&pubkey);

        let mut validator = crate::core::account::Validator::new(pubkey, 1000);
        validator.active = true;

        let mut sk_bytes = [0u8; 64];
        sk_bytes[0] = 42;
        let bls_sk = bls12_381::Scalar::from_bytes_wide(&sk_bytes);
        let bls_pk_point = bls12_381::G2Affine::from(bls12_381::G2Projective::generator() * bls_sk);
        let bls_pk = bls_pk_point.to_compressed().to_vec();

        validator.bls_public_key = bls_pk.clone();
        validator.pop_signature = vec![0u8; 48];
        blockchain.state.validators.insert(pubkey, validator);

        for _ in 1..=10 {
            blockchain.produce_block(pubkey);
        }

        let checkpoint_block = blockchain.chain[10].clone();

        use bls12_381::G2Affine;
        let valid_pk = G2Affine::generator().to_compressed().to_vec();

        let _entry = ValidatorEntry {
            address: pubkey,
            stake: 1000,
            bls_public_key: valid_pk.clone(),
            pop_signature: Vec::new(),
        };

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

        blockchain.handle_finality_cert(cert).unwrap();
        assert_eq!(blockchain.finalized_height, 10);
        assert_eq!(blockchain.finalized_hash, checkpoint_block.hash);

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
}
