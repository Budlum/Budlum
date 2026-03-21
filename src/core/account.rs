use crate::consensus::pos::SlashingEvidence;
use crate::storage::db::Storage;
use crate::core::transaction::{Transaction, TransactionType};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
pub const MIN_TX_FEE: u64 = 1;
pub const GENESIS_BALANCE: u64 = 1_000_000_000;
pub const UNBONDING_EPOCHS: u64 = 7;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnbondingEntry {
    pub address: String,
    pub amount: u64,
    pub release_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    pub public_key: String,
    pub balance: u64,
    pub nonce: u64,
}
impl Account {
    pub fn new(public_key: String) -> Self {
        Account {
            public_key,
            balance: 0,
            nonce: 0,
        }
    }
    pub fn with_balance(public_key: String, balance: u64) -> Self {
        Account {
            public_key,
            balance,
            nonce: 0,
        }
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Validator {
    pub address: String,
    pub stake: u64,
    pub active: bool,
    pub slashed: bool,
    pub jailed: bool,
    pub jail_until: u64,
    pub last_proposed_block: Option<u64>,
    pub votes_for: u64,
    pub votes_against: u64,
    pub vrf_public_key: Vec<u8>,
}

impl Validator {
    pub fn new(address: String, stake: u64) -> Self {
        Validator {
            address,
            stake,
            active: true,
            slashed: false,
            jailed: false,
            jail_until: 0,
            last_proposed_block: None,
            votes_for: 0,
            votes_against: 0,
            vrf_public_key: Vec::new(),
        }
    }
    pub fn effective_stake(&self) -> u64 {
        if self.slashed || self.jailed {
            0
        } else {
            self.stake
        }
    }
    pub fn is_eligible(&self, current_block: u64) -> bool {
        self.active && !self.slashed && (!self.jailed || current_block >= self.jail_until)
    }
}

#[derive(Clone)]
pub struct AccountState {
    pub accounts: HashMap<String, Account>,
    pub validators: HashMap<String, Validator>,
    pub unbonding_queue: Vec<UnbondingEntry>,
    storage: Option<Storage>,
    pub epoch_index: u64,
    pub last_epoch_time: u64,
    dirty_accounts: HashSet<String>,
    cached_leaves: Vec<[u8; 32]>,
    cached_keys: Vec<String>,
}
impl AccountState {
    pub fn new() -> Self {
        AccountState {
            accounts: HashMap::new(),
            validators: HashMap::new(),
            unbonding_queue: Vec::new(),
            storage: None,
            epoch_index: 0,
            last_epoch_time: 0,
            dirty_accounts: HashSet::new(),
            cached_leaves: Vec::new(),
            cached_keys: Vec::new(),
        }
    }
    pub fn with_storage(storage: Storage) -> Self {
        let mut state = AccountState {
            accounts: HashMap::new(),
            validators: HashMap::new(),
            unbonding_queue: Vec::new(),
            storage: Some(storage),
            epoch_index: 0,
            last_epoch_time: 0,
            dirty_accounts: HashSet::new(),
            cached_leaves: Vec::new(),
            cached_keys: Vec::new(),
        };
        if let Err(e) = state.load_from_storage() {
            println!("Could not load account state: {}", e);
        }
        state
    }
    pub fn state_root(&self) -> String {
        #[derive(Serialize)]
        struct CanonicalStateV1<'a> {
            version: u8,
            accounts: std::collections::BTreeMap<&'a String, &'a Account>,
            validators: std::collections::BTreeMap<&'a String, &'a Validator>,
            unbonding_queue: &'a Vec<UnbondingEntry>,
            epoch_index: u64,
            last_epoch_time: u64,
        }

        let canonical = CanonicalStateV1 {
            version: 1,
            accounts: self.accounts.iter().collect(),
            validators: self.validators.iter().collect(),
            unbonding_queue: &self.unbonding_queue,
            epoch_index: self.epoch_index,
            last_epoch_time: self.last_epoch_time,
        };

        let bytes = bincode::serialize(&canonical).unwrap_or_default();

        let mut prefix_bytes = b"BDLM_STATE_V1".to_vec();
        prefix_bytes.extend(bytes);

        crate::core::hash::calculate_hash(&prefix_bytes)
    }
    pub fn init_genesis(&mut self, genesis_pubkey: &str) {
        let account = Account::with_balance(genesis_pubkey.to_string(), GENESIS_BALANCE);
        self.accounts.insert(genesis_pubkey.to_string(), account);
        println!("Genesis account created: {} coins", GENESIS_BALANCE);
    }
    pub fn add_validator(&mut self, address: String, stake: u64) {
        let validator = Validator::new(address.clone(), stake);
        self.validators.insert(address, validator);
    }
    pub fn get_total_stake(&self) -> u64 {
        self.validators
            .values()
            .filter(|v| v.active && !v.slashed)
            .map(|v| v.stake)
            .sum()
    }
    pub fn get_active_validators(&self) -> Vec<&Validator> {
        let mut validators: Vec<&Validator> = self
            .validators
            .values()
            .filter(|v| v.active && !v.slashed)
            .collect();
        validators.sort_by(|a, b| a.address.cmp(&b.address));
        validators
    }
    pub fn get_validator(&self, address: &str) -> Option<&Validator> {
        self.validators.get(address)
    }
    pub fn get_validator_mut(&mut self, address: &str) -> Option<&mut Validator> {
        self.validators.get_mut(address)
    }

    pub fn get_balance(&self, public_key: &str) -> u64 {
        self.accounts
            .get(public_key)
            .map(|a| a.balance)
            .unwrap_or(0)
    }
    pub fn get_nonce(&self, public_key: &str) -> u64 {
        self.accounts.get(public_key).map(|a| a.nonce).unwrap_or(0)
    }
    pub fn get_or_create(&mut self, public_key: &str) -> &mut Account {
        if !self.accounts.contains_key(public_key) {
            self.accounts
                .insert(public_key.to_string(), Account::new(public_key.to_string()));
        }
        self.dirty_accounts.insert(public_key.to_string());
        self.accounts.get_mut(public_key).unwrap()
    }
    pub fn validate_transaction(&self, tx: &Transaction) -> Result<(), String> {
        if tx.from == "0".repeat(64) {
            return Ok(());
        }
        if !tx.verify() {
            return Err("Invalid signature".into());
        }
        if tx.fee < MIN_TX_FEE {
            return Err(format!("Fee too low: {} < {}", tx.fee, MIN_TX_FEE));
        }
        let expected_nonce = self.get_nonce(&tx.from);
        if tx.nonce != expected_nonce {
            return Err(format!(
                "Invalid nonce: expected {}, got {}",
                expected_nonce, tx.nonce
            ));
        }
        let balance = self.get_balance(&tx.from);
        let total_cost = tx.total_cost();
        if balance < total_cost {
            return Err(format!(
                "Insufficient balance: {} < {} (amount: {}, fee: {})",
                balance, total_cost, tx.amount, tx.fee
            ));
        }

        match tx.tx_type {
            TransactionType::Transfer => {
                if tx.to.is_empty() {
                    return Err("Transfer missing 'to' address".into());
                }
            }
            TransactionType::Stake => {
                if tx.amount == 0 {
                    return Err("Stake amount must be > 0".into());
                }
            }
            TransactionType::Unstake => {
                if let Some(validator) = self.validators.get(&tx.from) {
                    if validator.stake < tx.amount {
                        return Err(format!(
                            "Insufficient stake: {} < {}",
                            validator.stake, tx.amount
                        ));
                    }
                } else {
                    return Err("Not a validator".into());
                }
            }
            TransactionType::Vote => {
                if !self.validators.contains_key(&tx.from) {
                    return Err("Only validators can vote".into());
                }
            }
        }

        Ok(())
    }

    pub fn apply_slashing(&mut self, evidences: &[SlashingEvidence], slash_ratio: f64) {
        for evidence in evidences {
            if let Some(producer) = &evidence.header1.producer {
                if let Some(validator) = self.validators.get_mut(producer) {
                    if !validator.slashed {
                        let penalty = (validator.stake as f64 * slash_ratio) as u64;
                        validator.stake = validator.stake.saturating_sub(penalty);
                        validator.slashed = true;
                        validator.active = false;
                        let jail_duration = 3600 * 24;

                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs();
                        validator.jail_until = now + jail_duration;
                        println!("Slashed validator {} for {} stake", producer, penalty);
                    }
                }
            }
        }
    }

    pub fn process_unbonding(&mut self) {
        let current_epoch = self.epoch_index;
        let mut released: Vec<(String, u64)> = Vec::new();
        self.unbonding_queue.retain(|entry| {
            if entry.release_epoch <= current_epoch {
                released.push((entry.address.clone(), entry.amount));
                false
            } else {
                true
            }
        });
        for (addr, amount) in released {
            let account = self.get_or_create(&addr);
            account.balance += amount;
            println!(
                "Unbonding released: {} received {} coins",
                &addr[..16.min(addr.len())],
                amount
            );
        }
    }

    pub fn advance_epoch(&mut self, current_timestamp: u128) {
        self.epoch_index += 1;
        self.last_epoch_time = current_timestamp as u64;
        println!("Epoch advanced to {}", self.epoch_index);

        self.process_unbonding();

        let current_time_sec = (current_timestamp / 1000) as u64;

        for (addr, validator) in self.validators.iter_mut() {
            if validator.jailed && validator.jail_until <= current_time_sec {
                println!("Validator {} released from jail", addr);
                validator.jailed = false;
                if validator.stake > 0 {
                    validator.active = true;
                }
            }
        }
    }
    pub fn add_balance(&mut self, public_key: &str, amount: u64) {
        let account = self.get_or_create(public_key);
        account.balance += amount;
        self.dirty_accounts.insert(public_key.to_string());
    }
    pub fn save_to_storage(&self) -> Result<(), String> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(()),
        };
        for (pubkey, account) in &self.accounts {
            storage
                .save_account(pubkey, account)
                .map_err(|e| format!("Storage error: {}", e))?;
        }
        storage
            .db()
            .flush()
            .map_err(|e| format!("Flush error: {}", e))?;
        Ok(())
    }
    fn load_from_storage(&mut self) -> Result<(), String> {
        let storage = match &self.storage {
            Some(s) => s,
            None => return Ok(()),
        };
        match storage.load_all_accounts() {
            Ok(accounts) => {
                println!("Loaded {} accounts from storage", accounts.len());
                self.accounts = accounts;
            }
            Err(e) => {
                if let Ok(Some(data)) = storage.db().get("ACCOUNT_STATE") {
                    let accounts: HashMap<String, Account> = serde_json::from_slice(&data)
                        .map_err(|e| format!("Deserialization error: {}", e))?;
                    self.accounts = accounts;
                    println!("Loaded {} accounts from legacy blob", self.accounts.len());
                } else {
                    println!("Could not load accounts: {}", e);
                }
            }
        }
        Ok(())
    }
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }
    #[allow(dead_code)]
    pub fn print_balances(&self) {
        println!("Account Balances:");
        for (pubkey, account) in &self.accounts {
            println!(
                "  {}...  balance: {}, nonce: {}",
                &pubkey[..16.min(pubkey.len())],
                account.balance,
                account.nonce
            );
        }
    }
    pub fn get_all_balances(&self) -> HashMap<String, u64> {
        self.accounts
            .iter()
            .map(|(k, v)| (k.clone(), v.balance))
            .collect()
    }
    pub fn get_all_nonces(&self) -> HashMap<String, u64> {
        self.accounts
            .iter()
            .map(|(k, v)| (k.clone(), v.nonce))
            .collect()
    }

    pub fn calculate_state_root(&mut self) -> String {
        use sha2::{Digest, Sha256};

        if self.accounts.is_empty() {
            return "0".repeat(64);
        }

        let mut sorted_accounts: Vec<_> = self.accounts.iter().collect();
        sorted_accounts.sort_by(|a, b| a.0.cmp(b.0));

        let keys_changed = self.cached_keys.len() != sorted_accounts.len()
            || self.cached_keys.iter().zip(sorted_accounts.iter()).any(|(k, (sk, _))| k != *sk);

        if keys_changed || self.cached_leaves.is_empty() {
            self.cached_keys = sorted_accounts.iter().map(|(k, _)| (*k).clone()).collect();
            self.cached_leaves = sorted_accounts
                .iter()
                .map(|(pubkey, account)| {
                    let mut h = Sha256::new();
                    h.update(b"BDLM_LEAF_V2");
                    h.update(pubkey.as_bytes());
                    h.update(account.balance.to_le_bytes());
                    h.update(account.nonce.to_le_bytes());
                    h.finalize().into()
                })
                .collect();
        } else {
            for dirty_key in &self.dirty_accounts {
                if let Some(pos) = self.cached_keys.iter().position(|k| k == dirty_key) {
                    if let Some(account) = self.accounts.get(dirty_key) {
                        let mut h = Sha256::new();
                        h.update(b"BDLM_LEAF_V2");
                        h.update(dirty_key.as_bytes());
                        h.update(account.balance.to_le_bytes());
                        h.update(account.nonce.to_le_bytes());
                        self.cached_leaves[pos] = h.finalize().into();
                    }
                }
            }
        }

        self.dirty_accounts.clear();

        let mut level = self.cached_leaves.clone();
        while level.len() > 1 {
            let mut next_level = Vec::new();
            let mut i = 0;
            while i < level.len() {
                let left = &level[i];
                let right = if i + 1 < level.len() {
                    &level[i + 1]
                } else {
                    left
                };
                let mut h = Sha256::new();
                h.update(b"BDLM_NODE_V2");
                h.update(left);
                h.update(right);
                next_level.push(h.finalize().into());
                i += 2;
            }
            level = next_level;
        }

        hex::encode(level[0])
    }
    pub fn clear_dirty(&mut self) {
        self.dirty_accounts.clear();
    }
}
impl Default for AccountState {
    fn default() -> Self {
        Self::new()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::primitives::KeyPair;
    #[test]
    fn test_new_account() {
        let account = Account::new("pubkey123".into());
        assert_eq!(account.balance, 0);
        assert_eq!(account.nonce, 0);
    }
    #[test]
    fn test_account_with_balance() {
        let account = Account::with_balance("pubkey123".into(), 1000);
        assert_eq!(account.balance, 1000);
    }
    #[test]
    fn test_account_state_balance() {
        let mut state = AccountState::new();
        state.add_balance("alice", 500);
        assert_eq!(state.get_balance("alice"), 500);
        assert_eq!(state.get_balance("bob"), 0);
    }
    #[test]
    fn test_transfer() {
        let alice = KeyPair::generate().unwrap();
        let bob = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx = Transaction::new_with_fee(
            alice.public_key_hex(),
            bob.public_key_hex(),
            100,
            5,
            0,
            vec![],
        );
        tx.sign(&alice);
        assert!(state.validate_transaction(&tx).is_ok());
        crate::execution::executor::Executor::apply_transaction(&mut state, &tx).unwrap();
        assert_eq!(state.get_balance(&alice.public_key_hex()), 895);
        assert_eq!(state.get_balance(&bob.public_key_hex()), 100);
        assert_eq!(state.get_nonce(&alice.public_key_hex()), 1);
    }
    #[test]
    fn test_insufficient_balance() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 50);
        let mut tx =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 100, 1, 0, vec![]);
        tx.sign(&alice);
        assert!(state.validate_transaction(&tx).is_err());
    }
    #[test]
    fn test_wrong_nonce() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 100, 1, 5, vec![]);
        tx.sign(&alice);
        let result = state.validate_transaction(&tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("nonce"));
    }
    #[test]
    fn test_replay_protection() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx1 =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 50, 1, 0, vec![]);
        tx1.sign(&alice);
        assert!(state.validate_transaction(&tx1).is_ok());
        crate::execution::executor::Executor::apply_transaction(&mut state, &tx1).unwrap();
        assert!(state.validate_transaction(&tx1).is_err());
        let mut tx2 =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 50, 1, 1, vec![]);
        tx2.sign(&alice);
        assert!(state.validate_transaction(&tx2).is_ok());
    }
    #[test]
    fn test_fee_too_low() {
        let alice = KeyPair::generate().unwrap();
        let mut state = AccountState::new();
        state.add_balance(&alice.public_key_hex(), 1000);
        let mut tx =
            Transaction::new_with_fee(alice.public_key_hex(), "bob".into(), 100, 0, 0, vec![]);
        tx.sign(&alice);
        let result = state.validate_transaction(&tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Fee"));
    }
}
