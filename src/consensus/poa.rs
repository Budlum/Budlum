use super::{ConsensusEngine, ConsensusError};
use crate::core::account::{AccountState, Validator};
use crate::core::address::Address;
use crate::core::block::Block;
#[derive(Debug, Clone)]
pub struct PoAConfig {
    pub block_period: u64,
    pub epoch_length: u64,
    pub quorum_ratio: f64,
    pub validators_file: Option<String>,
}
impl Default for PoAConfig {
    fn default() -> Self {
        PoAConfig {
            block_period: 5,
            epoch_length: 30000,
            quorum_ratio: 0.67,
            validators_file: None,
        }
    }
}

use crate::crypto::primitives::KeyPair;

pub struct PoAEngine {
    pub config: PoAConfig,
    keypair: Option<KeyPair>,
}

impl PoAEngine {
    pub fn new(config: PoAConfig, keypair: Option<KeyPair>) -> Self {
        PoAEngine { config, keypair }
    }
    pub fn with_config(
        config: PoAConfig,
        _validators: Vec<Address>,
        keypair: Option<KeyPair>,
    ) -> Self {
        PoAEngine { config, keypair }
    }

    pub fn expected_proposer<'a>(
        &self,
        block_index: u64,
        active_validators: &'a [&Validator],
    ) -> Option<&'a Validator> {
        if active_validators.is_empty() {
            return None;
        }
        let slot = (block_index as usize) % active_validators.len();
        Some(active_validators[slot])
    }

    pub fn active_validator_count(&self, state: &AccountState) -> usize {
        state.get_active_validators().len()
    }
}

impl ConsensusEngine for PoAEngine {
    fn prepare_block(&self, block: &mut Block, state: &AccountState) -> Result<(), ConsensusError> {
        let slot = block.index;
        let active_refs = state.get_active_validators();

        let expected_signer_addr =
            if let Some(expected) = self.expected_proposer(slot, &active_refs) {
                expected.address
            } else {
                if block.index == 0 {
                    Address::zero()
                } else {
                    return Err(ConsensusError("No active validators found".into()));
                }
            };

        if expected_signer_addr != Address::zero() {
            println!(
                "PoA: Block {} should be proposed by: {}",
                slot,
                expected_signer_addr
            );

            if let Some(kp) = &self.keypair {
                let kp: &KeyPair = kp;
                let our_addr = Address::from(kp.public_key_bytes());
                if our_addr == expected_signer_addr {
                    block.sign(kp);
                    println!(
                        " PoA: Block {} signed by us ({})",
                        block.index,
                        expected_signer_addr
                    );
                } else {
                    /*
                    println!(
                        " PoA: We are not the proposer (us: {}, expected: {})",
                        keypair.public_key_hex(),
                        expected_signer_addr
                    );
                    */
                }
            } else {
                println!(" PoA: No keypair configured, cannot sign block");
            }
        }

        if block.signature.is_none() {
            block.hash = block.calculate_hash();
        }

        println!("PoA: Block {} prepared", block.index);
        Ok(())
    }

    fn validate_block(
        &self,
        block: &Block,
        chain: &[Block],
        state: &AccountState,
    ) -> Result<(), ConsensusError> {
        if block.index == 0 {
            if block.hash != block.calculate_hash() {
                return Err(ConsensusError("Invalid genesis block hash".into()));
            }
            return Ok(());
        }
        if let Some(prev_block) = chain.last() {
            if block.previous_hash != prev_block.hash {
                return Err(ConsensusError(format!(
                    "Previous hash mismatch. Expected: {}, Got: {}",
                    prev_block.hash, block.previous_hash
                )));
            }
        }

        let active_refs = state.get_active_validators();
        if !active_refs.is_empty() {
            let expected = self
                .expected_proposer(block.index, &active_refs)
                .ok_or_else(|| ConsensusError("No proposer for this slot".into()))?;

            let producer = block
                .producer
                .as_ref()
                .ok_or_else(|| ConsensusError("Block has no producer".into()))?;

            if producer != &expected.address {
                return Err(ConsensusError(format!(
                    "Wrong proposer. Expected: {}, Got: {}",
                    expected.address,
                    producer
                )));
            }

            if !block.verify_signature() {
                return Err(ConsensusError("Invalid block signature".into()));
            }

            println!(
                "PoA: Block {} signature verified (producer: {})",
                block.index,
                producer
            );
        } else {
            if block.hash != block.calculate_hash() {
                return Err(ConsensusError("Invalid block hash".into()));
            }
        }
        Ok(())
    }
    fn consensus_type(&self) -> &'static str {
        "PoA"
    }
    fn info(&self) -> String {
        format!(
            "PoA (validators: in-state, quorum: {:.0}%)",
            self.config.quorum_ratio * 100.0
        )
    }

    fn fork_choice_score(&self, chain: &[Block]) -> u128 {
        chain.len() as u128
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::account::{AccountState, Validator};
    use crate::core::address::Address;
    use crate::crypto::primitives::KeyPair;

    #[test]
    fn test_proposer_rotation() {
        let mut state = AccountState::new();
        let alice = KeyPair::generate().unwrap();
        let bob = KeyPair::generate().unwrap();
        let alice_addr = Address::from(alice.public_key_bytes());
        let bob_addr = Address::from(bob.public_key_bytes());

        state.validators.insert(
            alice_addr,
            Validator::new(alice_addr, 0),
        );
        state.validators.insert(
            bob_addr,
            Validator::new(bob_addr, 0),
        );

        state
            .validators
            .get_mut(&alice_addr)
            .unwrap()
            .active = true;
        state
            .validators
            .get_mut(&bob_addr)
            .unwrap()
            .active = true;

        let engine = PoAEngine::new(PoAConfig::default(), None);

        let active_refs = state.get_active_validators();

        if active_refs.len() < 2 {
            return;
        }

        let p1 = engine.expected_proposer(1, &active_refs).unwrap();
        let p2 = engine.expected_proposer(2, &active_refs).unwrap();

        assert_ne!(p1.address, p2.address);
    }

    #[test]
    fn test_poa_signing() {
        let keypair = KeyPair::generate().unwrap();
        let pubkey = Address::from(keypair.public_key_bytes());

        let mut state = AccountState::new();
        state
            .validators
            .insert(pubkey, Validator::new(pubkey, 0));
        state.validators.get_mut(&pubkey).unwrap().active = true;

        let engine = PoAEngine::new(PoAConfig::default(), Some(keypair));

        let mut block = Block::new(1, "prev".into(), vec![]);

        engine.prepare_block(&mut block, &state).unwrap();

        assert!(block.producer.is_some());
        assert_eq!(block.producer.as_ref().unwrap(), &pubkey);
        assert!(block.signature.is_some());
        assert!(block.verify_signature());
    }
}
