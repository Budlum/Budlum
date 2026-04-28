use crate::core::address::Address;
use crate::core::governance::ProposalType;
use crate::crypto::primitives::{verify_signature, KeyPair};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

pub const DEFAULT_CHAIN_ID: u64 = 1337;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct GasSchedule {
    pub base_fee: u64,
    pub gas_per_byte: u64,
    pub gas_per_signature: u64,
    pub transfer_gas: u64,
    pub stake_gas: u64,
    pub vote_gas: u64,
}

impl crate::core::chain_config::Network {
    pub fn gas_schedule(&self) -> GasSchedule {
        match self {
            crate::core::chain_config::Network::Mainnet => GasSchedule {
                base_fee: 10,
                gas_per_byte: 2,
                gas_per_signature: 1_000,
                transfer_gas: 21_000,
                stake_gas: 45_000,
                vote_gas: 35_000,
            },
            crate::core::chain_config::Network::Testnet => GasSchedule {
                base_fee: 1,
                gas_per_byte: 1,
                gas_per_signature: 500,
                transfer_gas: 21_000,
                stake_gas: 35_000,
                vote_gas: 25_000,
            },
            crate::core::chain_config::Network::Devnet => GasSchedule {
                base_fee: 1,
                gas_per_byte: 1,
                gas_per_signature: 100,
                transfer_gas: 1_000,
                stake_gas: 2_000,
                vote_gas: 1_500,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TransactionType {
    Transfer,
    Stake,
    Unstake,
    Vote,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Transaction {
    pub from: Address,
    pub to: Address,
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
    pub fn new(from: Address, to: Address, amount: u64, data: Vec<u8>) -> Self {
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

    pub fn new_stake(from: Address, amount: u64, nonce: u64) -> Self {
        Self::new_with_chain_id(
            from,
            Address::zero(),
            amount,
            0,
            nonce,
            vec![],
            DEFAULT_CHAIN_ID,
            TransactionType::Stake,
        )
    }

    pub fn new_proposal(from: Address, p_type: ProposalType, duration: u64, nonce: u64) -> Self {
        let mut data = Vec::new();
        data.extend_from_slice(&duration.to_le_bytes());
        data.extend_from_slice(&serde_json::to_vec(&p_type).unwrap_or_default());

        Self::new_with_chain_id(
            from,
            Address::zero(),
            0,
            0,
            nonce,
            data,
            DEFAULT_CHAIN_ID,
            TransactionType::Vote,
        )
    }

    pub fn new_vote(from: Address, proposal_id: u64, vote_for: bool, nonce: u64) -> Self {
        let mut data = Vec::new();
        data.push(if vote_for { 1 } else { 0 });
        data.extend_from_slice(&proposal_id.to_le_bytes());

        Self::new_with_chain_id(
            from,
            Address::zero(),
            0,
            0,
            nonce,
            data,
            DEFAULT_CHAIN_ID,
            TransactionType::Vote,
        )
    }

    pub fn new_with_chain_id(
        from: Address,
        to: Address,
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
        from: Address,
        to: Address,
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
            from: Address::zero(),
            to: Address::zero(),
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
        let expected_from = Address::from(keypair.public_key_bytes());
        if self.from != expected_from {
            println!(
                "Warning: TX.from ({}) doesn't match keypair pubkey ({})",
                self.from, expected_from
            );
        }
        let signing_hash = self.signing_hash();
        let signature = keypair.sign(&signing_hash);
        self.signature = Some(signature.to_vec());
    }
    pub fn verify(&self) -> bool {
        if self.from == Address::zero() && self.to == Address::zero() && self.signature.is_none() {
            return true;
        }
        let signature = match &self.signature {
            Some(s) => s,
            None => {
                println!("TX has no signature");
                return false;
            }
        };
        let public_key = &self.from.0;
        let signing_hash = self.signing_hash();
        match verify_signature(&signing_hash, signature, public_key) {
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
        if self.from == Address::zero() {
            return true;
        }
        match self.tx_type {
            TransactionType::Transfer => {
                if self.to == Address::zero() {
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

    pub fn estimate_gas_with_schedule(&self, schedule: GasSchedule) -> u64 {
        let intrinsic = match self.tx_type {
            TransactionType::Transfer => schedule.transfer_gas,
            TransactionType::Stake | TransactionType::Unstake => schedule.stake_gas,
            TransactionType::Vote => schedule.vote_gas,
        };
        let signature_gas = if self.signature.is_some() {
            schedule.gas_per_signature
        } else {
            0
        };
        intrinsic
            .saturating_add((self.data.len() as u64).saturating_mul(schedule.gas_per_byte))
            .saturating_add(signature_gas)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_transaction_creation() {
        let recipient = Address::from([1u8; 32]);
        let tx = Transaction::new(Address::zero(), recipient, 100, vec![]);
        assert_eq!(tx.amount, 100);
        assert_eq!(tx.tx_type, TransactionType::Transfer);
        assert!(tx.signature.is_none());
    }
    #[test]
    fn test_transaction_with_fee() {
        let recipient = Address::from([1u8; 32]);
        let tx = Transaction::new_with_fee(Address::zero(), recipient, 100, 5, 1, vec![]);
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
        let tx = Transaction::new_stake(Address::zero(), 500, 1);
        assert_eq!(tx.amount, 500);
        assert_eq!(tx.tx_type, TransactionType::Stake);
    }
    #[test]
    fn test_sign_and_verify() {
        let keypair = KeyPair::generate().unwrap();
        let alice = Address::from(keypair.public_key_bytes());
        let recipient = Address::from([1u8; 32]);
        let mut tx = Transaction::new_with_fee(alice, recipient, 50, 1, 0, vec![]);
        assert!(!tx.verify());
        tx.sign(&keypair);
        assert!(tx.verify());
        assert!(tx.is_valid());
    }
}
