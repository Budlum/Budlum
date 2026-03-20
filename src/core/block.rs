use crate::crypto::primitives::{verify_signature, KeyPair};
use crate::core::hash::hash_fields;
use crate::core::transaction::Transaction;
use serde::{Deserialize, Serialize};

pub const DEFAULT_CHAIN_ID: u64 = 1337;
use crate::consensus::pos::SlashingEvidence;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BlockHeader {
    pub index: u64,
    pub timestamp: u128,
    pub previous_hash: String,
    pub hash: String,
    pub producer: Option<String>,
    pub chain_id: u64,
    pub state_root: String,
    pub tx_root: String,
    pub slashing_evidence: Option<Vec<SlashingEvidence>>,
    pub nonce: u64,
    pub epoch: u64,
    pub slot: u64,
    pub vrf_output: Vec<u8>,
    pub vrf_proof: Vec<u8>,
    pub validator_set_hash: String,
}

impl BlockHeader {
    pub fn from_block(block: &Block) -> Self {
        BlockHeader {
            index: block.index,
            timestamp: block.timestamp,
            previous_hash: block.previous_hash.clone(),
            hash: block.hash.clone(),
            producer: block.producer.clone(),
            chain_id: block.chain_id,
            state_root: block.state_root.clone(),
            tx_root: block.tx_root.clone(),
            slashing_evidence: block.slashing_evidence.clone(),
            nonce: block.nonce,
            epoch: block.epoch,
            slot: block.slot,
            vrf_output: block.vrf_output.clone(),
            vrf_proof: block.vrf_proof.clone(),
            validator_set_hash: block.validator_set_hash.clone(),
        }
    }

    pub fn calculate_hash(&self) -> String {
        let producer_bytes = self
            .producer
            .as_ref()
            .map(|p| p.as_bytes().to_vec())
            .unwrap_or_default();

        let evidence_bytes = self
            .slashing_evidence
            .as_ref()
            .map(|e| bincode::serialize(e).unwrap_or_default())
            .unwrap_or_default();

        hash_fields(&[
            b"BDLM_BLOCK_V2",
            &self.index.to_le_bytes(),
            &self.timestamp.to_le_bytes(),
            self.previous_hash.as_bytes(),
            self.tx_root.as_bytes(),
            &self.nonce.to_le_bytes(),
            &producer_bytes,
            &evidence_bytes,
            &self.chain_id.to_le_bytes(),
            self.state_root.as_bytes(),
            &self.epoch.to_le_bytes(),
            &self.slot.to_le_bytes(),
            &self.vrf_output,
            &self.vrf_proof,
            self.validator_set_hash.as_bytes(),
        ])
    }

    pub fn verify_signature(&self, signature: &[u8]) -> bool {
        let producer_hex = match &self.producer {
            Some(p) => p,
            None => return false,
        };
        let public_key = match hex::decode(producer_hex) {
            Ok(pk) => pk,
            Err(_) => return false,
        };
        let calculated_hash = self.calculate_hash();
        if calculated_hash != self.hash {
            return false;
        }
        verify_signature(self.hash.as_bytes(), signature, &public_key).is_ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Block {
    pub index: u64,
    pub timestamp: u128,
    pub previous_hash: String,
    pub hash: String,
    pub transactions: Vec<Transaction>,
    pub nonce: u64,
    pub producer: Option<String>,
    pub signature: Option<Vec<u8>>,
    pub chain_id: u64,
    pub slashing_evidence: Option<Vec<SlashingEvidence>>,
    pub state_root: String,
    pub tx_root: String,
    pub epoch: u64,
    pub slot: u64,
    pub vrf_output: Vec<u8>,
    pub vrf_proof: Vec<u8>,
    pub validator_set_hash: String,
}

impl Block {
    pub fn new(index: u64, previous_hash: String, transactions: Vec<Transaction>) -> Self {
        Self::new_with_chain_id(index, previous_hash, transactions, DEFAULT_CHAIN_ID)
    }

    pub fn new_with_chain_id(
        index: u64,
        previous_hash: String,
        transactions: Vec<Transaction>,
        chain_id: u64,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let mut block = Block {
            index,
            timestamp,
            previous_hash,
            hash: String::new(),
            transactions,
            nonce: 0,
            producer: None,
            signature: None,
            chain_id,
            slashing_evidence: None,
            state_root: String::new(),
            tx_root: String::new(),
            epoch: 0,
            slot: 0,
            vrf_output: Vec::new(),
            vrf_proof: Vec::new(),
            validator_set_hash: String::new(),
        };
        block.tx_root = block.calculate_tx_root();
        block.hash = block.calculate_hash();
        block
    }
    pub fn genesis() -> Self {
        let mut block = Block::new(0, "0".repeat(64), vec![Transaction::genesis()]);
        block.timestamp = 0;
        block.hash = block.calculate_hash();
        block
    }

    pub fn calculate_tx_root(&self) -> String {
        let mut tx_hashes: Vec<String> =
            self.transactions.iter().map(|tx| tx.hash.clone()).collect();

        if tx_hashes.is_empty() {
            return "0".repeat(64);
        }

        while tx_hashes.len() > 1 {
            let mut next_level = Vec::new();
            for chunk in tx_hashes.chunks(2) {
                let left = &chunk[0];
                let right = if chunk.len() > 1 { &chunk[1] } else { left };
                let combined = format!("{}{}", left, right);
                next_level.push(hex::encode(crate::core::hash::calculate_hash(
                    combined.as_bytes(),
                )));
            }
            tx_hashes = next_level;
        }

        tx_hashes[0].clone()
    }

    pub fn calculate_hash(&self) -> String {
        let producer_bytes = self
            .producer
            .as_ref()
            .map(|p| p.as_bytes().to_vec())
            .unwrap_or_default();
        let evidence_bytes = self
            .slashing_evidence
            .as_ref()
            .map(|e| bincode::serialize(e).unwrap_or_default())
            .unwrap_or_default();

        hash_fields(&[
            b"BDLM_BLOCK_V2",
            &self.index.to_le_bytes(),
            &self.timestamp.to_le_bytes(),
            self.previous_hash.as_bytes(),
            self.tx_root.as_bytes(),
            &self.nonce.to_le_bytes(),
            &producer_bytes,
            &evidence_bytes,
            &self.chain_id.to_le_bytes(),
            self.state_root.as_bytes(),
            &self.epoch.to_le_bytes(),
            &self.slot.to_le_bytes(),
            &self.vrf_output,
            &self.vrf_proof,
            self.validator_set_hash.as_bytes(),
        ])
    }
    pub fn sign(&mut self, keypair: &KeyPair) {
        self.producer = Some(keypair.public_key_hex());
        self.hash = self.calculate_hash();
        let signature = keypair.sign(self.hash.as_bytes());
        self.signature = Some(signature.to_vec());
        println!(
            "Block {} signed by {}",
            self.index,
            &self.producer.as_ref().unwrap()[..16]
        );
    }

    pub fn verify_signature(&self) -> bool {
        let producer_hex = match &self.producer {
            Some(p) => p,
            None => {
                println!("Block has no producer");
                return false;
            }
        };
        let signature = match &self.signature {
            Some(s) => s,
            None => {
                println!("Block has no signature");
                return false;
            }
        };
        let public_key = match hex::decode(producer_hex) {
            Ok(pk) => pk,
            Err(e) => {
                println!("Invalid producer hex: {}", e);
                return false;
            }
        };
        match verify_signature(self.hash.as_bytes(), signature, &public_key) {
            Ok(()) => {
                println!("Block {} signature verified", self.index);
                true
            }
            Err(e) => {
                println!("Signature verification failed: {}", e);
                false
            }
        }
    }
    pub fn verify_signature_with_pubkey(&self, expected_pubkey_hex: &str) -> bool {
        let producer_hex = match &self.producer {
            Some(p) => p,
            None => return false,
        };
        if producer_hex != expected_pubkey_hex {
            println!(
                "Wrong producer. Expected: {}..., Got: {}...",
                &expected_pubkey_hex[..16.min(expected_pubkey_hex.len())],
                &producer_hex[..16.min(producer_hex.len())]
            );
            return false;
        }
        self.verify_signature()
    }
    pub fn mine(&mut self, difficulty: usize) {
        let target = "0".repeat(difficulty);
        while !self.hash.starts_with(&target) {
            self.nonce += 1;
            self.hash = self.calculate_hash();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_genesis_block() {
        let genesis = Block::genesis();
        assert_eq!(genesis.index, 0);
        assert_eq!(genesis.previous_hash, "0".repeat(64));
        assert!(!genesis.hash.is_empty());
    }
    #[test]
    fn test_mining() {
        let mut block = Block::genesis();
        block.mine(1);
        assert!(block.hash.starts_with("0"));
    }
    #[test]
    fn test_ed25519_sign_and_verify() {
        let keypair = KeyPair::generate().unwrap();
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.sign(&keypair);
        assert!(block.signature.is_some());
        assert_eq!(block.signature.as_ref().unwrap().len(), 64);
        assert!(block.verify_signature());
    }
    #[test]
    fn test_signature_with_specific_pubkey() {
        let keypair = KeyPair::generate().unwrap();
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.sign(&keypair);
        assert!(block.verify_signature_with_pubkey(&keypair.public_key_hex()));
        let other_keypair = KeyPair::generate().unwrap();
        assert!(!block.verify_signature_with_pubkey(&other_keypair.public_key_hex()));
    }
    #[test]
    fn test_modified_block_fails_verification() {
        let keypair = KeyPair::generate().unwrap();
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.sign(&keypair);
        block.nonce = 12345;
        block.hash = block.calculate_hash();
        assert!(!block.verify_signature());
    }
}
