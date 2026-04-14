use crate::core::address::Address;
use crate::core::transaction::Transaction;
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct MempoolConfig {
    pub max_size: usize,

    pub max_per_sender: usize,

    pub min_fee: u64,

    pub tx_ttl_secs: u64,

    pub rbf_bump_percent: u64,
}

impl Default for MempoolConfig {
    fn default() -> Self {
        MempoolConfig {
            max_size: 20000,
            max_per_sender: 100,
            min_fee: 1,
            tx_ttl_secs: 3600,
            rbf_bump_percent: 10,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum MempoolError {
    PoolFull,
    DuplicateTransaction,
    FeeTooLow,
    SenderLimitReached,
    InvalidNonce,
    TransactionExpired,
    RbfFeeTooLow,
    InvalidTransaction(String),
}

#[derive(Debug, Clone)]
struct PendingTx {
    tx: Transaction,
    added_at: u128,
}

#[derive(Clone)]
pub struct Mempool {
    config: MempoolConfig,

    transactions: HashMap<String, PendingTx>,

    by_sender: HashMap<Address, BTreeMap<u64, String>>,

    by_fee: BTreeMap<u64, HashSet<String>>,
}

impl Mempool {
    pub fn new(config: MempoolConfig) -> Self {
        Mempool {
            config,
            transactions: HashMap::new(),
            by_sender: HashMap::new(),
            by_fee: BTreeMap::new(),
        }
    }

    pub fn add_transaction(&mut self, tx: Transaction) -> Result<(), MempoolError> {
        if self.transactions.contains_key(&tx.hash) {
            return Err(MempoolError::DuplicateTransaction);
        }

        if tx.fee < self.config.min_fee {
            return Err(MempoolError::FeeTooLow);
        }

        if self.transactions.len() >= self.config.max_size {
            if !self.evict_lowest_fee(&tx) {
                return Err(MempoolError::PoolFull);
            }
        }

        let sender_count = self.by_sender.get(&tx.from).map(|v| v.len()).unwrap_or(0);

        if let Some(existing_hash) = self.find_tx_by_sender_nonce(&tx.from, tx.nonce) {
            let existing = self.transactions.get(&existing_hash).unwrap();
            let min_new_fee =
                existing.tx.fee + (existing.tx.fee * self.config.rbf_bump_percent / 100);
            if tx.fee < min_new_fee {
                return Err(MempoolError::RbfFeeTooLow);
            }

            self.remove_transaction(&existing_hash);
        } else {
            if sender_count >= self.config.max_per_sender {
                return Err(MempoolError::SenderLimitReached);
            }
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        self.by_sender
            .entry(tx.from)
            .or_insert_with(BTreeMap::new)
            .insert(tx.nonce, tx.hash.clone());
        self.by_fee
            .entry(tx.fee)
            .or_insert_with(HashSet::new)
            .insert(tx.hash.clone());

        self.transactions
            .insert(tx.hash.clone(), PendingTx { tx, added_at: now });

        Ok(())
    }

    pub fn remove_transaction(&mut self, hash: &str) -> Option<Transaction> {
        if let Some(pending) = self.transactions.remove(hash) {
            if let Some(sender_txs) = self.by_sender.get_mut(&pending.tx.from) {
                sender_txs.remove(&pending.tx.nonce);
                if sender_txs.is_empty() {
                    self.by_sender.remove(&pending.tx.from);
                }
            }

            if let Some(fee_txs) = self.by_fee.get_mut(&pending.tx.fee) {
                fee_txs.remove(hash);
                if fee_txs.is_empty() {
                    self.by_fee.remove(&pending.tx.fee);
                }
            }
            return Some(pending.tx);
        }
        None
    }

    pub fn get_sorted_transactions(&self, limit: usize) -> Vec<Transaction> {
        let mut result = Vec::with_capacity(limit);

        for (_, hashes) in self.by_fee.iter().rev() {
            for hash in hashes {
                if result.len() >= limit {
                    return result;
                }
                if let Some(pending) = self.transactions.get(hash) {
                    result.push(pending.tx.clone());
                }
            }
        }
        result
    }

    pub fn cleanup_expired(&mut self) -> usize {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();

        let ttl_ms = self.config.tx_ttl_secs as u128 * 1000;
        let expired: Vec<String> = self
            .transactions
            .iter()
            .filter(|(_, p)| now - p.added_at > ttl_ms)
            .map(|(h, _)| h.clone())
            .collect();

        let count = expired.len();
        for hash in expired {
            self.remove_transaction(&hash);
        }
        count
    }

    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub fn get(&self, hash: &str) -> Option<&Transaction> {
        self.transactions.get(hash).map(|p| &p.tx)
    }

    pub fn sender_transactions(&self, sender: &Address) -> Vec<Transaction> {
        self.by_sender
            .get(sender)
            .map(|nonces| {
                nonces
                    .values()
                    .filter_map(|hash| {
                        self.transactions
                            .get(hash)
                            .map(|pending| pending.tx.clone())
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn drain(&mut self) -> Vec<Transaction> {
        let txs: Vec<Transaction> = self.transactions.values().map(|p| p.tx.clone()).collect();
        self.transactions.clear();
        self.by_sender.clear();
        self.by_fee.clear();
        txs
    }

    fn find_tx_by_sender_nonce(&self, sender: &Address, nonce: u64) -> Option<String> {
        self.by_sender
            .get(sender)
            .and_then(|nonces| nonces.get(&nonce).cloned())
    }

    fn evict_lowest_fee(&mut self, new_tx: &Transaction) -> bool {
        if let Some((&lowest_fee, hashes)) = self.by_fee.iter().next() {
            if new_tx.fee > lowest_fee {
                if let Some(hash) = hashes.iter().next().cloned() {
                    self.remove_transaction(&hash);
                    return true;
                }
            }
        }
        false
    }

    pub fn set_min_fee(&mut self, min_fee: u64) {
        self.config.min_fee = min_fee;
    }
}

impl Default for Mempool {
    fn default() -> Self {
        Self::new(MempoolConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_tx(from_hex: &str, nonce: u64, fee: u64) -> Transaction {
        let from = Address::from_hex(from_hex).unwrap();
        let mut tx = Transaction::new(from, Address::zero(), 100, vec![]);
        tx.nonce = nonce;
        tx.fee = fee;
        tx.hash = format!("tx_{}_{}", from_hex, nonce);
        tx
    }

    #[test]
    fn test_add_and_get() {
        let mut pool = Mempool::default();
        let tx = create_test_tx(&"01".repeat(32), 0, 10);
        assert!(pool.add_transaction(tx.clone()).is_ok());
        assert_eq!(pool.len(), 1);
        assert!(pool.get(&tx.hash).is_some());
    }

    #[test]
    fn test_duplicate_rejection() {
        let mut pool = Mempool::default();
        let tx = create_test_tx(&"01".repeat(32), 0, 10);
        pool.add_transaction(tx.clone()).unwrap();
        assert_eq!(
            pool.add_transaction(tx),
            Err(MempoolError::DuplicateTransaction)
        );
    }

    #[test]
    fn test_fee_too_low() {
        let mut pool = Mempool::default();
        let tx = create_test_tx(&"01".repeat(32), 0, 0);
        assert_eq!(pool.add_transaction(tx), Err(MempoolError::FeeTooLow));
    }

    #[test]
    fn test_sender_limit() {
        let config = MempoolConfig {
            max_per_sender: 2,
            ..Default::default()
        };
        let mut pool = Mempool::new(config);

        let alice_hex = "01".repeat(32);
        pool.add_transaction(create_test_tx(&alice_hex, 0, 10))
            .unwrap();
        pool.add_transaction(create_test_tx(&alice_hex, 1, 10))
            .unwrap();
        assert_eq!(
            pool.add_transaction(create_test_tx(&alice_hex, 2, 10)),
            Err(MempoolError::SenderLimitReached)
        );
    }

    #[test]
    fn test_sorted_by_fee() {
        let mut pool = Mempool::default();
        pool.add_transaction(create_test_tx(&"01".repeat(32), 0, 5))
            .unwrap();
        pool.add_transaction(create_test_tx(&"02".repeat(32), 0, 20))
            .unwrap();
        pool.add_transaction(create_test_tx(&"03".repeat(32), 0, 10))
            .unwrap();

        let sorted = pool.get_sorted_transactions(10);
        assert_eq!(sorted[0].fee, 20);
        assert_eq!(sorted[1].fee, 10);
        assert_eq!(sorted[2].fee, 5);
    }

    #[test]
    fn test_rbf() {
        let mut pool = Mempool::default();
        let alice_hex = "01".repeat(32);
        let tx1 = create_test_tx(&alice_hex, 0, 10);
        pool.add_transaction(tx1).unwrap();

        let mut tx2 = create_test_tx(&alice_hex, 0, 15);
        tx2.hash = "tx_alice_0_v2".to_string();
        assert!(pool.add_transaction(tx2).is_ok());
        assert_eq!(pool.len(), 1);
    }

    #[test]
    fn test_cleanup_expired() {
        let mut config = MempoolConfig::default();
        config.tx_ttl_secs = 1;
        let mut pool = Mempool::new(config);

        let tx = create_test_tx(&"01".repeat(32), 0, 10);
        pool.add_transaction(tx).unwrap();
        assert_eq!(pool.len(), 1);

        std::thread::sleep(std::time::Duration::from_secs(2));

        let removed = pool.cleanup_expired();
        assert_eq!(removed, 1);
        assert_eq!(pool.len(), 0);
        assert!(pool.is_empty());
    }
}
