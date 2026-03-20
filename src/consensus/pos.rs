use super::{ConsensusEngine, ConsensusError};
use crate::core::account::AccountState;
use crate::core::block::Block;
use hex;
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct PoSConfig {
    pub min_stake: u64,
    pub slot_duration: u64,
    pub epoch_length: u64,
    pub annual_reward_rate: f64,
    pub slashing_penalty: f64,
    pub double_sign_penalty: f64,
    pub unbonding_epochs: u64,
}
impl Default for PoSConfig {
    fn default() -> Self {
        PoSConfig {
            min_stake: 1000,
            slot_duration: 6,
            epoch_length: 32,
            annual_reward_rate: 0.05,
            slashing_penalty: 0.10,
            double_sign_penalty: 0.50,
            unbonding_epochs: crate::core::account::UNBONDING_EPOCHS,
        }
    }
}

use serde::{Deserialize, Serialize};

use crate::core::block::BlockHeader;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlashingEvidence {
    pub header1: BlockHeader,
    pub header2: BlockHeader,
    pub signature1: Vec<u8>,
    pub signature2: Vec<u8>,
}

impl SlashingEvidence {
    pub fn new(
        header1: BlockHeader,
        header2: BlockHeader,
        signature1: Vec<u8>,
        signature2: Vec<u8>,
    ) -> Self {
        SlashingEvidence {
            header1,
            header2,
            signature1,
            signature2,
        }
    }
}
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub block_index: u64,
    pub block_hash: String,
    pub timestamp: u128,
}
use crate::crypto::primitives::ValidatorKeys;

use std::sync::RwLock;

pub struct PoSEngine {
    pub config: PoSConfig,
    seen_blocks: RwLock<HashMap<(String, u64), (BlockHeader, Vec<u8>)>>,
    pub slashing_evidence: RwLock<Vec<SlashingEvidence>>,
    checkpoints: RwLock<Vec<Checkpoint>>,
    validator_keys: Option<ValidatorKeys>,
    epoch_seed: RwLock<[u8; 32]>,
}
impl PoSEngine {
    pub fn new(config: PoSConfig, validator_keys: Option<ValidatorKeys>) -> Self {
        PoSEngine {
            config,
            seen_blocks: RwLock::new(HashMap::new()),
            slashing_evidence: RwLock::new(Vec::new()),
            checkpoints: RwLock::new(Vec::new()),
            validator_keys,
            epoch_seed: RwLock::new([0u8; 32]),
        }
    }

    pub fn verify_evidence(&self, evidence: &SlashingEvidence) -> bool {
        if evidence.header1.index != evidence.header2.index {
            return false;
        }
        if evidence.header1.producer != evidence.header2.producer {
            return false;
        }
        if evidence.header1.producer.is_none() {
            return false;
        }

        if !evidence.header1.verify_signature(&evidence.signature1) {
            return false;
        }
        if !evidence.header2.verify_signature(&evidence.signature2) {
            return false;
        }

        if evidence.header1.hash == evidence.header2.hash {
            return false;
        }

        true
    }

    pub fn get_slashing_evidence(&self) -> Result<Vec<SlashingEvidence>, ConsensusError> {
        self.slashing_evidence
            .read()
            .map(|guard| guard.clone())
            .map_err(|_| ConsensusError("Failed to acquire read lock on slashing evidence".into()))
    }

    pub fn get_checkpoints(&self) -> Result<Vec<Checkpoint>, ConsensusError> {
        self.checkpoints
            .read()
            .map(|guard| guard.clone())
            .map_err(|_| ConsensusError("Failed to acquire read lock on checkpoints".into()))
    }

    pub fn add_checkpoint(&self, block: &Block) -> Result<(), ConsensusError> {
        let mut checkpoints = self
            .checkpoints
            .write()
            .map_err(|_| ConsensusError("Failed to acquire write lock on checkpoints".into()))?;
        checkpoints.push(Checkpoint {
            block_index: block.index,
            block_hash: block.hash.clone(),
            timestamp: block.timestamp,
        });
        Ok(())
    }
    pub fn is_before_checkpoint(&self, block: &Block) -> bool {
        if let Ok(guard) = self.checkpoints.read() {
            if let Some(last_cp) = guard.last() {
                return block.index < last_cp.block_index;
            }
        }
        false
    }
    pub fn calculate_seed(
        &self,
        chain_id: u64,
        epoch: u64,
        slot: u64,
        validator_set_hash: &str,
    ) -> [u8; 32] {
        let prev_seed = self.epoch_seed.read().unwrap_or_else(|e| e.into_inner());
        let mut hasher = Sha3_256::new();
        hasher.update(chain_id.to_le_bytes());
        hasher.update(epoch.to_le_bytes());
        hasher.update(slot.to_le_bytes());
        hasher.update(*prev_seed);
        hasher.update(validator_set_hash.as_bytes());
        hasher.finalize().into()
    }

    pub fn calculate_vrf_threshold(&self, stake: u64, total_stake: u64) -> u64 {
        if total_stake == 0 || stake == 0 {
            return 0;
        }
        let prob = (stake as f64 / total_stake as f64) * crate::core::chain_config::VRF_BASE_PROB;
        if prob >= 1.0 {
            u64::MAX
        } else {
            (prob * (u64::MAX as f64)) as u64
        }
    }

    pub fn check_vrf_threshold(&self, vrf_output: &[u8], threshold: u64) -> bool {
        let mut hasher = Sha3_256::new();
        hasher.update(vrf_output);
        let hash = hasher.finalize();
        let y = u64::from_le_bytes(hash[0..8].try_into().unwrap_or([0; 8]));
        y < threshold
    }
    pub fn is_validator(&self, pubkey: &str, state: &AccountState) -> bool {
        state.get_validator(pubkey).map_or(false, |v| {
            v.active && !v.slashed && v.stake >= self.config.min_stake
        })
    }
    #[allow(dead_code)]
    fn calculate_reward(&self, validator_stake: u64) -> u64 {
        let slots_per_year = 365 * 24 * 60 * 60 / self.config.slot_duration;
        let reward = (validator_stake as f64 * self.config.annual_reward_rate
            / slots_per_year as f64) as u64;
        reward.max(1)
    }

    pub fn serialize_state(&self) -> Result<Vec<u8>, String> {
        let state = serde_json::json!({
            "checkpoints": self.checkpoints.read().map_err(|_| "Lock error".to_string())?.iter().map(|c| {
                serde_json::json!({
                    "block_index": c.block_index,
                    "block_hash": c.block_hash,
                    "timestamp": c.timestamp,
                })
            }).collect::<Vec<_>>(),
            "slashing_evidence": *self.slashing_evidence.read().map_err(|_| "Lock error".to_string())?,
        });
        serde_json::to_vec(&state).map_err(|e| format!("Serialization error: {}", e))
    }
    pub fn save_state(&self, db: &sled::Db) -> Result<(), String> {
        let data = self.serialize_state()?;
        db.insert("POS_STATE", data)
            .map_err(|e| format!("DB insert error: {}", e))?;
        db.flush().map_err(|e| format!("DB flush error: {}", e))?;
        println!(
            "PoS state saved: {} new checkpoints",
            self.checkpoints
                .read()
                .map_err(|_| "Lock error".to_string())?
                .len()
        );
        Ok(())
    }
    pub fn load_state(&mut self, db: &sled::Db) -> Result<(), String> {
        let data = match db.get("POS_STATE") {
            Ok(Some(d)) => d,
            Ok(None) => {
                println!("No saved PoS state found, starting fresh");
                return Ok(());
            }
            Err(e) => return Err(format!("DB read error: {}", e)),
        };
        let state: serde_json::Value =
            serde_json::from_slice(&data).map_err(|e| format!("Deserialization error: {}", e))?;

        if let Some(checkpoints_data) = state.get("checkpoints").and_then(|c| c.as_array()) {
            let mut checkpoints = self
                .checkpoints
                .write()
                .map_err(|_| "Lock error".to_string())?;
            for cp in checkpoints_data {
                let block_index = cp.get("block_index").and_then(|i| i.as_u64()).unwrap_or(0);
                let block_hash = cp
                    .get("block_hash")
                    .and_then(|h| h.as_str())
                    .unwrap_or("")
                    .to_string();
                let timestamp = cp.get("timestamp").and_then(|t| t.as_u64()).unwrap_or(0) as u128;
                checkpoints.push(Checkpoint {
                    block_index,
                    block_hash,
                    timestamp,
                });
            }
        }

        println!(
            "PoS state loaded: {} checkpoints",
            self.checkpoints
                .read()
                .map_err(|_| "Lock error".to_string())?
                .len()
        );
        Ok(())
    }
}
impl ConsensusEngine for PoSEngine {
    fn prepare_block(&self, block: &mut Block, state: &AccountState) -> Result<(), ConsensusError> {
        let slot = block.index;
        let epoch = slot / crate::core::chain_config::EPOCH_LEN;
        block.epoch = epoch;
        block.slot = slot;

        let active_validators = state.get_active_validators();
        let total_stake = state.get_total_stake();

        if let Ok(mut evidences) = self.slashing_evidence.write() {
            if !evidences.is_empty() {
                block.slashing_evidence = Some(evidences.clone());
                evidences.clear();
            }
        }

        if !active_validators.is_empty() {
            if let Some(keys) = &self.validator_keys {
                let pubkey = keys.sig_key.public_key_hex();

                if let Some(validator) = state.get_validator(&pubkey) {
                    if validator.active
                        && !validator.slashed
                        && validator.stake >= self.config.min_stake
                    {
                        let seed = self.calculate_seed(
                            block.chain_id,
                            epoch,
                            slot,
                            &block.validator_set_hash,
                        );
                        let (vrf_io, vrf_proof, _) = keys.vrf_key.vrf_sign(
                            schnorrkel::context::signing_context(b"BUDLUM_VRF").bytes(&seed),
                        );
                        let vrf_output = vrf_io.to_preout().to_bytes();
                        let proof_bytes = vrf_proof.to_bytes();

                        let threshold = self.calculate_vrf_threshold(validator.stake, total_stake);
                        if self.check_vrf_threshold(&vrf_output, threshold) {
                            block.vrf_output = vrf_output.to_vec();
                            block.vrf_proof = proof_bytes.to_vec();
                            block.sign(&keys.sig_key);
                            return Ok(());
                        }
                    }
                }
            }
            return Err(ConsensusError(
                "Not selected as VRF leader for this slot".into(),
            ));
        } else {
            block.hash = block.calculate_hash();
        }
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
        if self.is_before_checkpoint(block) {
            return Err(ConsensusError(
                "Block is before last checkpoint (possible long-range attack)".into(),
            ));
        }

        let active_validators = state.get_active_validators();
        if !active_validators.is_empty() {
            let producer = block
                .producer
                .as_ref()
                .ok_or_else(|| ConsensusError("Block has no producer".into()))?;

            let validator = state
                .get_validator(producer)
                .ok_or_else(|| ConsensusError("Unknown block producer".into()))?;
            if !validator.active || validator.slashed || validator.stake < self.config.min_stake {
                return Err(ConsensusError("Producer is not an active validator".into()));
            }

            if validator.vrf_public_key.is_empty() {
                return Err(ConsensusError(
                    "Producer has no registered VRF public key".into(),
                ));
            }

            if let Ok(public_key) = schnorrkel::PublicKey::from_bytes(&validator.vrf_public_key) {
                let seed = self.calculate_seed(
                    block.chain_id,
                    block.epoch,
                    block.slot,
                    &block.validator_set_hash,
                );

                let mut output_bytes = [0u8; 32];
                if block.vrf_output.len() == 32 {
                    output_bytes.copy_from_slice(&block.vrf_output);
                } else {
                    return Err(ConsensusError("Invalid VRF output length".into()));
                }

                if let Ok(vrf_preout) = schnorrkel::vrf::VRFPreOut::from_bytes(&output_bytes) {
                    if let Ok(vrf_proof) = schnorrkel::vrf::VRFProof::from_bytes(&block.vrf_proof) {
                        if public_key
                            .vrf_verify(
                                schnorrkel::context::signing_context(b"BUDLUM_VRF").bytes(&seed),
                                &vrf_preout,
                                &vrf_proof,
                            )
                            .is_err()
                        {
                            return Err(ConsensusError("VRF proof verification failed".into()));
                        }
                    } else {
                        return Err(ConsensusError("Invalid VRF proof format".into()));
                    }
                } else {
                    return Err(ConsensusError("Invalid VRF output format".into()));
                }
            } else {
                return Err(ConsensusError("Invalid VRF public key format".into()));
            }

            let threshold = self.calculate_vrf_threshold(validator.stake, state.get_total_stake());
            if !self.check_vrf_threshold(&block.vrf_output, threshold) {
                return Err(ConsensusError(
                    "VRF output does not meet leadership threshold".into(),
                ));
            }

            if !block.verify_signature() {
                return Err(ConsensusError("Invalid block signature".into()));
            }

            if let Some(evidences) = &block.slashing_evidence {
                for (i, evidence) in evidences.iter().enumerate() {
                    if !self.verify_evidence(evidence) {
                        return Err(ConsensusError(format!("Invalid slashing evidence #{}", i)));
                    }

                    if let Some(producer) = &evidence.header1.producer {
                        if state.get_validator(producer).is_none() {
                            println!(
                                " Warning: Slashing evidence for unknown validator {}",
                                producer
                            );
                        } else {
                            println!(" Valid Slashing Evidence found for validator {}", producer);
                        }
                    } else {
                        return Err(ConsensusError("Evidence header missing producer".into()));
                    }
                }
            }

            println!(
                "PoS: Block {} validated (producer: {}, stake: {})",
                block.index,
                &producer[..16.min(producer.len())],
                validator.stake
            );
        } else {
            if block.hash != block.calculate_hash() {
                return Err(ConsensusError("Invalid block hash".into()));
            }
        }
        Ok(())
    }
    fn consensus_type(&self) -> &'static str {
        "PoS"
    }
    fn info(&self) -> String {
        format!(
            "PoS (min_stake: {}, checkpoints: {})",
            self.config.min_stake,
            self.checkpoints.read().map(|c| c.len()).unwrap_or(0)
        )
    }
    fn select_best_chain<'a>(&self, chains: &[&'a [Block]]) -> Option<&'a [Block]> {
        if chains.is_empty() {
            return None;
        }
        chains
            .iter()
            .max_by_key(|c| self.fork_choice_score(c))
            .copied()
    }

    fn fork_choice_score(&self, chain: &[Block]) -> u128 {
        let last_checkpoint_height = if let Ok(guard) = self.checkpoints.read() {
            guard.last().map(|c| c.block_index).unwrap_or(0)
        } else {
            0
        };
        (last_checkpoint_height as u128) * 1000 + chain.len() as u128
    }

    fn record_block(&self, block: &Block) -> Result<(), ConsensusError> {
        let producer = block
            .producer
            .as_ref()
            .ok_or(ConsensusError("Block has no producer".into()))?;
        let header = BlockHeader::from_block(block);
        let signature = block.signature.clone().unwrap_or_default();
        let key = (producer.clone(), header.index);

        let block_hash_bytes =
            hex::decode(&block.hash).unwrap_or_else(|_| block.hash.as_bytes().to_vec());
        let mut block_contrib = Sha3_256::new();
        block_contrib.update(&block_hash_bytes);
        let contribution: [u8; 32] = block_contrib.finalize().into();
        if let Ok(mut seed) = self.epoch_seed.write() {
            for (i, byte) in seed.iter_mut().enumerate() {
                *byte ^= contribution[i];
            }
        }

        let mut seen_blocks = self
            .seen_blocks
            .write()
            .map_err(|_| ConsensusError("Lock error on seen_blocks".into()))?;

        if let Some(existing) = seen_blocks.get(&key) {
            if existing.0.hash != header.hash {
                println!(
                    "DOUBLE-SIGN: {} signed two blocks for slot {}!",
                    producer, header.index
                );
                let evidence = SlashingEvidence::new(
                    existing.0.clone(),
                    header,
                    existing.1.clone(),
                    signature,
                );
                let mut slashing_evidence = self
                    .slashing_evidence
                    .write()
                    .map_err(|_| ConsensusError("Lock error on slashing_evidence".into()))?;
                slashing_evidence.push(evidence);
            }
        } else {
            seen_blocks.insert(key, (header, signature));
            if block.index > 0 && block.index % self.config.epoch_length == 0 {
                if let Ok(mut seed) = self.epoch_seed.write() {
                    *seed = [0u8; 32];
                }
                let _ = self.add_checkpoint(block);
            }
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::account::AccountState;
    use crate::crypto::primitives::{KeyPair, ValidatorKeys};
    use crate::core::transaction::Transaction;
    use crate::execution::executor::Executor;

    fn create_stake_tx(keypair: &KeyPair, amount: u64, nonce: u64) -> Transaction {
        let mut tx = Transaction::new_stake(keypair.public_key_hex(), amount, nonce);
        tx.sign(keypair);
        tx
    }

    #[test]
    fn test_validator_threshold() {
        let mut state = AccountState::new();
        let alice = ValidatorKeys::generate().unwrap();
        state.add_balance(&alice.sig_key.public_key_hex(), 2000);

        let tx = create_stake_tx(&alice.sig_key, 1000, 1);
        Executor::apply_transaction(&mut state, &tx).unwrap();

        let engine = PoSEngine::new(PoSConfig::default(), None);
        let threshold = engine.calculate_vrf_threshold(1000, 1000);
        assert_eq!(threshold, u64::MAX);
    }

    #[test]
    fn test_double_sign_detection() {
        let engine = PoSEngine::new(PoSConfig::default(), None);
        let alice = KeyPair::generate().unwrap();

        let mut block1 = Block::new(10, "prev".into(), vec![]);
        block1.producer = Some(alice.public_key_hex());
        block1.hash = "hash1".to_string();
        block1.sign(&alice);

        let mut block2 = Block::new(10, "prev".into(), vec![]);
        block2.timestamp += 1000;
        block2.producer = Some(alice.public_key_hex());
        block2.hash = "hash2".to_string();
        block2.sign(&alice);

        engine.record_block(&block1).unwrap();
        engine.record_block(&block2).unwrap();

        assert_eq!(engine.slashing_evidence.read().unwrap().len(), 1);
        let evidence = engine.slashing_evidence.read().unwrap()[0].clone();
        assert_eq!(evidence.header1.index, 10);
        assert!(engine.verify_evidence(&evidence));
    }

    #[test]
    fn test_minimum_stake() {
        let mut state = AccountState::new();
        let alice = KeyPair::generate().unwrap();
        state.add_balance(&alice.public_key_hex(), 2000);

        let config = PoSConfig {
            min_stake: 1000,
            ..Default::default()
        };
        let engine = PoSEngine::new(config, None);

        let tx = create_stake_tx(&alice, 500, 1);
        Executor::apply_transaction(&mut state, &tx).unwrap();

        assert!(!engine.is_validator(&alice.public_key_hex(), &state));

        let tx2 = create_stake_tx(&alice, 500, 2);
        Executor::apply_transaction(&mut state, &tx2).unwrap();

        assert!(engine.is_validator(&alice.public_key_hex(), &state));
    }
}
