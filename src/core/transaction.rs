use crate::crypto::primitives::{verify_signature, KeyPair};
use crate::core::hash::calculate_hash;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

pub const DEFAULT_CHAIN_ID: u64 = 1337;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionType {
    Transfer,
    Stake,
    Unstake,
    Vote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub from: String,
    pub to: String,
    pub amount: u64,
    pub fee: u64,
    pub nonce: u64,
    pub data: Vec<u8>,
    pub timestamp: u128,
    pub hash: String,
    pub signature: Option<Vec<u8>>,
    pub chain_id: u64,
    pub tx_type: TransactionType,
}
impl Transaction {
    pub fn new(from: String, to: String, amount: u64, data: Vec<u8>) -> Self {
        Self::new_with_chain_id(
            from,
            to,
            amount,
            0,
            0,
            data,
            DEFAULT_CHAIN_ID,
            TransactionType::Transfer,
        )
    }

    pub fn new_stake(from: String, amount: u64, nonce: u64) -> Self {
        Self::new_with_chain_id(
            from,
            String::new(),
            amount,
            0,
            nonce,
            vec![],
            DEFAULT_CHAIN_ID,
            TransactionType::Stake,
        )
    }

    pub fn new_with_chain_id(
        from: String,
        to: String,
        amount: u64,
        fee: u64,
        nonce: u64,
        data: Vec<u8>,
        chain_id: u64,
        tx_type: TransactionType,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let mut tx = Transaction {
            from,
            to,
            amount,
            fee,
            nonce,
            data,
            timestamp,
            hash: String::new(),
            signature: None,
            chain_id,
            tx_type,
        };
        tx.hash = tx.calculate_hash();
        tx
    }
    pub fn new_with_fee(
        from: String,
        to: String,
        amount: u64,
        fee: u64,
        nonce: u64,
        data: Vec<u8>,
    ) -> Self {
        Self::new_with_chain_id(
            from,
            to,
            amount,
            fee,
            nonce,
            data,
            DEFAULT_CHAIN_ID,
            TransactionType::Transfer,
        )
    }
    pub fn genesis() -> Self {
        let mut tx = Transaction {
            from: "0".repeat(64),
            to: "0".repeat(64),
            amount: 0,
            fee: 0,
            nonce: 0,
            data: b"BUDLUM_GENESIS_TX".to_vec(),
            timestamp: 0,
            hash: String::new(),
            signature: None,
            chain_id: DEFAULT_CHAIN_ID,
            tx_type: TransactionType::Transfer,
        };
        tx.hash = tx.calculate_hash();
        tx
    }
    pub fn signing_hash(&self) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        hasher.update(b"BDLM_TX_V2");
        hasher.update(self.from.as_bytes());
        hasher.update(self.to.as_bytes());
        hasher.update(self.amount.to_le_bytes());
        hasher.update(self.fee.to_le_bytes());
        hasher.update(self.nonce.to_le_bytes());
        hasher.update(&self.data);
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(self.chain_id.to_le_bytes());

        let type_byte = match self.tx_type {
            TransactionType::Transfer => 0,
            TransactionType::Stake => 1,
            TransactionType::Unstake => 2,
            TransactionType::Vote => 3,
        };
        hasher.update(&[type_byte]);

        hasher.finalize().into()
    }
    pub fn calculate_hash(&self) -> String {
        hex::encode(self.signing_hash())
    }
    pub fn sign(&mut self, keypair: &KeyPair) {
        let expected_from = keypair.public_key_hex();
        if self.from != expected_from {
            println!(
                "Warning: TX.from ({}) doesn't match keypair pubkey ({})",
                &self.from[..16.min(self.from.len())],
                &expected_from[..16]
            );
        }
        let signing_hash = self.signing_hash();
        let signature = keypair.sign(&signing_hash);
        self.signature = Some(signature.to_vec());
    }
    pub fn verify(&self) -> bool {
        let zero_addr = "0".repeat(64);
        if self.from == zero_addr && self.to == zero_addr && self.signature.is_none() {
            return true;
        }
        let signature = match &self.signature {
            Some(s) => s,
            None => {
                println!("TX has no signature");
                return false;
            }
        };
        let public_key = match hex::decode(&self.from) {
            Ok(pk) => pk,
            Err(e) => {
                println!("Invalid from address (not valid hex): {}", e);
                return false;
            }
        };
        if public_key.len() != 32 {
            println!(
                "Invalid public key length: expected 32, got {}",
                public_key.len()
            );
            return false;
        }
        let signing_hash = self.signing_hash();
        match verify_signature(&signing_hash, signature, &public_key) {
            Ok(()) => true,
            Err(e) => {
                println!("TX signature verification failed: {}", e);
                false
            }
        }
    }
    pub fn is_valid(&self) -> bool {
        if !self.verify() {
            return false;
        }
        let zero_addr = "0".repeat(64);
        if self.from == zero_addr {
            return true;
        }
        match self.tx_type {
            TransactionType::Transfer => {
                if self.to.is_empty() {
                    println!("Transfer TX has empty 'to' address");
                    return false;
                }
            }
            TransactionType::Stake => {
                if self.amount == 0 {
                    println!("Stake amount cannot be 0");
                    return false;
                }
            }
            TransactionType::Unstake => {}
            TransactionType::Vote => {}
        }
        true
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
    pub fn total_cost(&self) -> u64 {
        self.amount.saturating_add(self.fee)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_transaction_creation() {
        let tx = Transaction::new("alice".into(), "bob".into(), 100, vec![]);
        assert_eq!(tx.amount, 100);
        assert_eq!(tx.tx_type, TransactionType::Transfer);
        assert!(tx.signature.is_none());
    }
    #[test]
    fn test_transaction_with_fee() {
        let tx = Transaction::new_with_fee("alice".into(), "bob".into(), 100, 5, 1, vec![]);
        assert_eq!(tx.fee, 5);
        assert_eq!(tx.nonce, 1);
        assert_eq!(tx.total_cost(), 105);
    }
    #[test]
    fn test_genesis_transaction() {
        let genesis = Transaction::genesis();
        assert!(genesis.verify());
        assert!(genesis.is_valid());
    }
    #[test]
    fn test_stake_transaction() {
        let tx = Transaction::new_stake("alice".into(), 500, 1);
        assert_eq!(tx.amount, 500);
        assert_eq!(tx.tx_type, TransactionType::Stake);
    }
    #[test]
    fn test_sign_and_verify() {
        let keypair = KeyPair::generate().unwrap();
        let mut tx = Transaction::new_with_fee(
            keypair.public_key_hex(),
            "recipient".into(),
            50,
            1,
            0,
            vec![],
        );
        assert!(!tx.verify());
        tx.sign(&keypair);
        assert!(tx.verify());
        assert!(tx.is_valid());
    }
}
