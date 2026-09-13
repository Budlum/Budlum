use crate::core::account::Account;
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use crate::cross_domain::message::CrossDomainMessage;
use crate::cross_domain::BridgeState;
use crate::domain::{ConsensusDomain, DomainCommitment};
use crate::settlement::GlobalBlockHeader;
use crate::storage::traits::{BlockchainStorage, DurableCommitBatch, SeenBlockMap};
use serde::{de::DeserializeOwned, Serialize};
use sled::Db;
use std::str::from_utf8;
use tracing::info;

fn encode<T: Serialize>(value: &T) -> std::io::Result<Vec<u8>> {
    bincode::serialize(value)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

fn decode<T: DeserializeOwned>(value: &[u8]) -> std::io::Result<T> {
    bincode::deserialize(value)
        .or_else(|_| serde_json::from_slice(value))
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

/// A single schema migration step: transforms the on-disk data in place.
type MigrationFn = fn(&Db) -> std::io::Result<()>;

#[derive(Clone, Debug)]
pub struct Storage {
    db: Db,
}
impl Storage {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let db = Self::open_with_retry(path)?;
        let storage = Storage { db };
        storage.apply_migrations()?;
        storage.recover_interrupted_commit()?;
        Ok(storage)
    }

    /// Opens the database, retrying briefly if the OS reports the file lock
    /// as transiently unavailable (EAGAIN). This happens when a database is
    /// reopened at the same path immediately after a previous handle was
    /// dropped — under load, the OS/sled may not have released the advisory
    /// lock yet. Any other error (corruption, permissions, ...) is returned
    /// immediately without retrying.
    fn open_with_retry(path: &str) -> std::io::Result<Db> {
        const MAX_ATTEMPTS: u32 = 5;
        const RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(50);

        let mut last_err = None;
        for attempt in 0..MAX_ATTEMPTS {
            match sled::open(path) {
                Ok(db) => return Ok(db),
                Err(e) => {
                    let io_err: std::io::Error = e.into();
                    let is_transient_lock_error = io_err.kind() == std::io::ErrorKind::WouldBlock
                        || io_err.to_string().contains("could not acquire lock");
                    if !is_transient_lock_error || attempt + 1 == MAX_ATTEMPTS {
                        return Err(io_err);
                    }
                    last_err = Some(io_err);
                    std::thread::sleep(RETRY_DELAY);
                }
            }
        }
        Err(last_err.unwrap_or_else(|| {
            std::io::Error::other(format!("failed to open database at {}", path))
        }))
    }

    /// Staged migrations, ordered by ascending `to_version`. Each entry transforms
    /// the on-disk data from `to_version - 1` (or wherever it currently sits) to
    /// `to_version`. To ship a new schema version: bump `CURRENT_SCHEMA_VERSION`
    /// in `apply_migrations` and push a `(N, migrate_fn)` entry here — the
    /// executor below applies registered steps in order and stamps the version
    /// after each one, so a crash mid-migration resumes at the right step
    /// instead of silently skipping data transformation.
    fn migrations() -> Vec<(u64, MigrationFn)> {
        vec![
            // (2, migrate_v1_to_v2),
        ]
    }

    /// Applies registered migration steps from `current_version` up to
    /// `target_version`, in order. Refuses to open a database whose stored
    /// version is newer than what this binary supports (downgrade protection).
    /// If a step fails, the schema version marker stays at the last
    /// successfully completed step so a retry resumes correctly.
    fn run_migrations(
        db: &Db,
        current_version: u64,
        target_version: u64,
        steps: &[(u64, MigrationFn)],
    ) -> std::io::Result<()> {
        if current_version > target_version {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "Database schema version {} is newer than the version this binary supports ({}); refusing to open with an older binary",
                    current_version, target_version
                ),
            ));
        }
        let mut current = current_version;
        for (to_version, migrate) in steps {
            if *to_version <= current || *to_version > target_version {
                continue;
            }
            info!(
                "Applying storage migration: schema v{} -> v{}",
                current, to_version
            );
            migrate(db)?;
            db.insert(b"SCHEMA_VERSION", to_version.to_string().as_bytes())?;
            db.flush()?;
            current = *to_version;
        }
        if current < target_version {
            // No migration registered for the remaining gap (e.g. a brand-new,
            // empty database) — just stamp the target version.
            db.insert(b"SCHEMA_VERSION", target_version.to_string().as_bytes())?;
            db.flush()?;
        }
        Ok(())
    }

    pub fn apply_migrations(&self) -> std::io::Result<()> {
        const CURRENT_SCHEMA_VERSION: u64 = 1;
        let current = self.schema_version()?;
        Self::run_migrations(
            &self.db,
            current,
            CURRENT_SCHEMA_VERSION,
            &Self::migrations(),
        )
    }

    pub fn schema_version(&self) -> std::io::Result<u64> {
        if let Some(val) = self.db.get(b"SCHEMA_VERSION")? {
            let s = from_utf8(&val)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(s.parse().unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    pub fn create_snapshot<P: AsRef<std::path::Path>>(&self, path: P) -> std::io::Result<()> {
        let mut snapshot = Vec::new();
        for item in self.db.iter() {
            let (key, value) = item?;
            snapshot.push((key.to_vec(), value.to_vec()));
        }
        let bytes = encode(&snapshot)?;
        std::fs::write(path, bytes)?;
        Ok(())
    }
    pub fn insert_block(&self, block: &Block) -> std::io::Result<()> {
        let key = block.hash.clone();
        let val = encode(block)?;
        let height_key = format!("HEIGHT:{}", block.index);
        let mut batch = sled::Batch::default();
        batch.insert(key.as_bytes(), val.as_slice());
        batch.insert(height_key.as_bytes(), block.hash.as_bytes());
        self.db.apply_batch(batch)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn commit_block(&self, block: &Block, state_root: &str) -> std::io::Result<()> {
        let mut batch = sled::Batch::default();

        let block_bytes = encode(block)?;
        batch.insert(block.hash.as_bytes(), block_bytes.as_slice());

        let height_key = format!("HEIGHT:{}", block.index);
        batch.insert(height_key.as_bytes(), block.hash.as_bytes());

        batch.insert(b"LAST", block.hash.as_bytes());

        let state_key = format!("STATE_ROOT:{}", block.index);
        batch.insert(state_key.as_bytes(), state_root.as_bytes());

        batch.insert(b"CANONICAL_HEIGHT", block.index.to_string().as_bytes());

        for tx in &block.transactions {
            let tx_idx_key = format!("TX_IDX:{}", tx.hash);
            batch.insert(tx_idx_key.as_bytes(), block.index.to_string().as_bytes());
        }

        self.db.apply_batch(batch)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn recover_interrupted_commit(&self) -> std::io::Result<()> {
        if let Some(height_bytes) = self.db.get(b"IN_PROGRESS_HEIGHT")? {
            let height_str = from_utf8(&height_bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let height: u64 = height_str.parse().unwrap_or(0);
            tracing::warn!(
                "Interrupted commit detected at height {}. Initiating rollback...",
                height
            );

            let mut batch = sled::Batch::default();

            // 1. If we wrote block H, clean up transaction indices and the block itself
            if let Some(block) = self.get_block_by_height(height)? {
                for tx in &block.transactions {
                    let tx_idx_key = format!("TX_IDX:{}", tx.hash);
                    batch.remove(tx_idx_key.as_bytes());
                }
                batch.remove(block.hash.as_bytes());
            }

            // 2. Remove block indexes at height H
            let height_key = format!("HEIGHT:{}", height);
            batch.remove(height_key.as_bytes());
            batch.remove(format!("STATE_ROOT:{}", height).as_bytes());
            batch.remove(format!("FINALITY_CERT:{}", height).as_bytes());
            batch.remove(format!("QC_BLOB:{}", height).as_bytes());

            // 3. Revert CANONICAL_HEIGHT and LAST hash to H-1
            if height > 0 {
                let prev_height = height - 1;
                let prev_height_key = format!("HEIGHT:{}", prev_height);
                if let Some(prev_hash_bytes) = self.db.get(prev_height_key.as_bytes())? {
                    batch.insert(b"LAST", &prev_hash_bytes);
                } else {
                    batch.remove(b"LAST");
                }
                batch.insert(b"CANONICAL_HEIGHT", prev_height.to_string().as_bytes());
            } else {
                batch.remove(b"LAST");
                batch.remove(b"CANONICAL_HEIGHT");
            }

            // 4. Remove the IN_PROGRESS_HEIGHT marker
            batch.remove(b"IN_PROGRESS_HEIGHT");

            // Apply rollback batch atomically
            self.db.apply_batch(batch)?;
            self.db.flush()?;
            tracing::info!("Rollback for height {} completed successfully.", height);
        }
        Ok(())
    }

    pub fn commit_durable_batch(&self, batch: &DurableCommitBatch) -> std::io::Result<()> {
        // Write IN_PROGRESS_HEIGHT marker and flush it
        self.db.insert(
            b"IN_PROGRESS_HEIGHT",
            batch.block.index.to_string().as_bytes(),
        )?;
        self.db.flush()?;

        let mut b = sled::Batch::default();

        // 1. Block
        let block_bytes = encode(&batch.block)?;
        b.insert(batch.block.hash.as_bytes(), block_bytes.as_slice());

        // 2. Height mapping
        let height_key = format!("HEIGHT:{}", batch.block.index);
        b.insert(height_key.as_bytes(), batch.block.hash.as_bytes());

        // 3. LAST tip hash
        b.insert(b"LAST", batch.block.hash.as_bytes());

        // 4. State root
        let state_key = format!("STATE_ROOT:{}", batch.block.index);
        b.insert(state_key.as_bytes(), batch.state_root.as_bytes());

        // 5. Canonical height
        b.insert(
            b"CANONICAL_HEIGHT",
            batch.block.index.to_string().as_bytes(),
        );

        // 6. Transaction indexes
        for tx in &batch.block.transactions {
            let tx_idx_key = format!("TX_IDX:{}", tx.hash);
            b.insert(
                tx_idx_key.as_bytes(),
                batch.block.index.to_string().as_bytes(),
            );
        }

        // 7. Finality Certificate
        if let Some(ref cert) = batch.finality_cert {
            let cert_key = format!("FINALITY_CERT:{}", batch.block.index);
            let cert_val = encode(cert)?;
            b.insert(cert_key.as_bytes(), cert_val.as_slice());
        }

        // 8. Global headers
        for header in &batch.global_headers {
            let key = format!("GLOBAL_HEADER:{}", header.global_height);
            let hash_key = format!("GLOBAL_HEADER_HASH:{}", header.calculate_hash());
            let val = encode(header)?;
            b.insert(key.as_bytes(), val.as_slice());
            b.insert(
                hash_key.as_bytes(),
                header.global_height.to_string().as_bytes(),
            );
            b.insert(
                b"LAST_GLOBAL_HEIGHT",
                header.global_height.to_string().as_bytes(),
            );
        }

        // 9. Bridge state
        if let Some(ref bridge_state) = batch.bridge_state {
            let val = encode(bridge_state)?;
            b.insert(b"BRIDGE_STATE", val.as_slice());
        }

        // 10. Accounts
        for (pubkey, account) in &batch.accounts {
            let key = format!("ACCT:{}", pubkey);
            let val = encode(account)?;
            b.insert(key.as_bytes(), val.as_slice());
        }

        // 11. Settled Domains
        for domain in &batch.settled_domains {
            let key = format!("DOMAIN:{}", domain.id);
            let val = encode(domain)?;
            b.insert(key.as_bytes(), val.as_slice());
        }

        // 11. Remove IN_PROGRESS_HEIGHT marker
        b.remove(b"IN_PROGRESS_HEIGHT");

        // Apply batch atomically and flush
        self.db.apply_batch(b)?;
        self.db.flush()?;

        Ok(())
    }

    pub fn get_block(&self, hash: &str) -> std::io::Result<Option<Block>> {
        if let Some(val) = self.db.get(hash)? {
            let block: Block = decode(&val)?;
            Ok(Some(block))
        } else {
            Ok(None)
        }
    }
    pub fn get_block_by_height(&self, height: u64) -> std::io::Result<Option<Block>> {
        let height_key = format!("HEIGHT:{}", height);
        if let Some(hash_bytes) = self.db.get(height_key.as_bytes())? {
            let hash = from_utf8(&hash_bytes)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .to_string();
            self.get_block(&hash)
        } else {
            Ok(None)
        }
    }
    pub fn get_canonical_height(&self) -> std::io::Result<u64> {
        if let Some(val) = self.db.get("CANONICAL_HEIGHT")? {
            let s = from_utf8(&val)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(s.parse().unwrap_or(0))
        } else {
            Ok(0)
        }
    }

    pub fn delete_block(&self, height: u64) -> std::io::Result<()> {
        let key = format!("HEIGHT:{}", height);
        if let Some(hash_val) = self.db.get(key.as_bytes())? {
            let mut batch = sled::Batch::default();
            batch.remove(&hash_val);
            batch.remove(key.as_bytes());
            batch.remove(format!("STATE_ROOT:{}", height).as_bytes());
            batch.remove(format!("FINALITY_CERT:{}", height).as_bytes());
            batch.remove(format!("QC_BLOB:{}", height).as_bytes());
            self.db.apply_batch(batch)?;
            self.db.flush()?;
        }
        Ok(())
    }
    pub fn save_qc_blob(
        &self,
        height: u64,
        blob: &crate::consensus::qc::QcBlob,
    ) -> std::io::Result<()> {
        let key = format!("QC_BLOB:{}", height);
        let val = encode(blob)?;
        self.db.insert(key.as_bytes(), val)?;
        self.db.flush()?;
        Ok(())
    }
    pub fn get_qc_blob(
        &self,
        height: u64,
    ) -> std::io::Result<Option<crate::consensus::qc::QcBlob>> {
        let key = format!("QC_BLOB:{}", height);
        if let Some(val) = self.db.get(key.as_bytes())? {
            let blob = decode(&val)?;
            Ok(Some(blob))
        } else {
            Ok(None)
        }
    }
    pub fn delete_qc_blob(&self, height: u64) -> std::io::Result<()> {
        let key = format!("QC_BLOB:{}", height);
        self.db.remove(key.as_bytes())?;
        self.db.flush()?;
        Ok(())
    }
    pub fn save_finality_cert(
        &self,
        height: u64,
        cert: &crate::chain::finality::FinalityCert,
    ) -> std::io::Result<()> {
        let key = format!("FINALITY_CERT:{}", height);
        let val = encode(cert)?;
        self.db.insert(key.as_bytes(), val)?;
        self.db.flush()?;
        Ok(())
    }
    pub fn get_finality_cert(
        &self,
        height: u64,
    ) -> std::io::Result<Option<crate::chain::finality::FinalityCert>> {
        let key = format!("FINALITY_CERT:{}", height);
        if let Some(val) = self.db.get(key.as_bytes())? {
            let cert = decode(&val)?;
            Ok(Some(cert))
        } else {
            Ok(None)
        }
    }
    pub fn delete_finality_cert(&self, height: u64) -> std::io::Result<()> {
        let key = format!("FINALITY_CERT:{}", height);
        self.db.remove(key.as_bytes())?;
        self.db.flush()?;
        Ok(())
    }
    pub fn save_canonical_height(&self, height: u64) -> std::io::Result<()> {
        self.db
            .insert("CANONICAL_HEIGHT", height.to_string().as_bytes())?;
        self.db.flush()?;
        Ok(())
    }
    pub fn save_state_root(&self, height: u64, state_root: &str) -> std::io::Result<()> {
        let key = format!("STATE_ROOT:{}", height);
        self.db.insert(key.as_bytes(), state_root.as_bytes())?;
        self.db.flush()?;
        Ok(())
    }

    pub fn save_consensus_domain(&self, domain: &ConsensusDomain) -> std::io::Result<()> {
        let key = format!("DOMAIN:{}", domain.id);
        let val = encode(domain)?;
        self.db.insert(key.as_bytes(), val)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn load_consensus_domains(&self) -> std::io::Result<Vec<ConsensusDomain>> {
        let mut domains: Vec<ConsensusDomain> = Vec::new();
        for item in self.db.scan_prefix(b"DOMAIN:") {
            let (_key, val) = item?;
            domains.push(decode(&val)?);
        }
        domains.sort_by_key(|domain| domain.id);
        Ok(domains)
    }

    pub fn save_domain_commitment(&self, commitment: &DomainCommitment) -> std::io::Result<()> {
        let key = format!(
            "DOMAIN_COMMITMENT:{}:{}:{}",
            commitment.domain_id, commitment.domain_height, commitment.sequence
        );
        let val = encode(commitment)?;
        self.db.insert(key.as_bytes(), val)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn save_domain_commitment_batch(
        &self,
        commitment: &DomainCommitment,
        domains: &[ConsensusDomain],
    ) -> std::io::Result<()> {
        let commitment_key = format!(
            "DOMAIN_COMMITMENT:{}:{}:{}",
            commitment.domain_id, commitment.domain_height, commitment.sequence
        );
        let commitment_val = encode(commitment)?;
        let mut batch = sled::Batch::default();
        batch.insert(commitment_key.as_bytes(), commitment_val.as_slice());

        for domain in domains {
            let domain_key = format!("DOMAIN:{}", domain.id);
            let domain_val = encode(domain)?;
            batch.insert(domain_key.as_bytes(), domain_val.as_slice());
        }

        self.db.apply_batch(batch)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn load_domain_commitments(&self) -> std::io::Result<Vec<DomainCommitment>> {
        let mut commitments: Vec<DomainCommitment> = Vec::new();
        for item in self.db.scan_prefix(b"DOMAIN_COMMITMENT:") {
            let (_key, val) = item?;
            commitments.push(decode(&val)?);
        }
        commitments.sort_by_key(|commitment| {
            (
                commitment.domain_id,
                commitment.domain_height,
                commitment.sequence,
            )
        });
        Ok(commitments)
    }

    pub fn save_global_header(&self, header: &GlobalBlockHeader) -> std::io::Result<()> {
        let key = format!("GLOBAL_HEADER:{}", header.global_height);
        let hash_key = format!("GLOBAL_HEADER_HASH:{}", header.calculate_hash());
        let val = encode(header)?;

        let mut batch = sled::Batch::default();
        batch.insert(key.as_bytes(), val.as_slice());
        batch.insert(
            hash_key.as_bytes(),
            header.global_height.to_string().as_bytes(),
        );
        batch.insert(
            b"LAST_GLOBAL_HEIGHT",
            header.global_height.to_string().as_bytes(),
        );
        self.db.apply_batch(batch)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn get_global_header(&self, height: u64) -> std::io::Result<Option<GlobalBlockHeader>> {
        let key = format!("GLOBAL_HEADER:{}", height);
        if let Some(val) = self.db.get(key.as_bytes())? {
            Ok(Some(decode(&val)?))
        } else {
            Ok(None)
        }
    }

    pub fn load_global_headers(&self) -> std::io::Result<Vec<GlobalBlockHeader>> {
        let mut headers: Vec<GlobalBlockHeader> = Vec::new();
        for item in self.db.scan_prefix(b"GLOBAL_HEADER:") {
            let (_key, val) = item?;
            headers.push(decode(&val)?);
        }
        headers.sort_by_key(|header| header.global_height);
        Ok(headers)
    }

    pub fn save_bridge_state(&self, bridge_state: &BridgeState) -> std::io::Result<()> {
        let val = encode(bridge_state)?;
        self.db.insert(b"BRIDGE_STATE", val)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn load_bridge_state(&self) -> std::io::Result<Option<BridgeState>> {
        if let Some(val) = self.db.get(b"BRIDGE_STATE")? {
            let decoded = decode(&val)?;
            Ok(Some(decoded))
        } else {
            Ok(None)
        }
    }

    pub fn save_cross_domain_message(&self, message: &CrossDomainMessage) -> std::io::Result<()> {
        let key = format!("XDOMAIN_MSG:{}", hex::encode(message.message_id));
        let val = encode(message)?;
        self.db.insert(key.as_bytes(), val)?;
        self.db.flush()?;
        Ok(())
    }

    pub fn load_cross_domain_messages(&self) -> std::io::Result<Vec<CrossDomainMessage>> {
        let mut messages: Vec<CrossDomainMessage> = Vec::new();
        for item in self.db.scan_prefix(b"XDOMAIN_MSG:") {
            let (_key, val) = item?;
            messages.push(decode(&val)?);
        }
        Ok(messages)
    }

    pub fn get_state_root(&self, height: u64) -> std::io::Result<Option<String>> {
        let key = format!("STATE_ROOT:{}", height);
        if let Some(val) = self.db.get(key.as_bytes())? {
            let root = from_utf8(&val)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .to_string();
            Ok(Some(root))
        } else {
            Ok(None)
        }
    }
    pub fn save_last_hash(&self, hash: &str) -> std::io::Result<()> {
        self.db.insert("LAST", hash.as_bytes())?;
        self.db.flush()?;
        Ok(())
    }
    pub fn get_last_hash(&self) -> std::io::Result<Option<String>> {
        if let Some(val) = self.db.get("LAST")? {
            let hash = from_utf8(&val)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?
                .to_string();
            Ok(Some(hash))
        } else {
            Ok(None)
        }
    }
    pub fn load_chain(&self) -> std::io::Result<Vec<Block>> {
        let mut chain = Vec::new();
        if let Some(mut current_hash) = self.get_last_hash()? {
            while let Ok(Some(block)) = self.get_block(&current_hash) {
                chain.push(block.clone());
                if block.previous_hash == "0".repeat(64) {
                    break;
                }
                current_hash = block.previous_hash;
            }
        }
        chain.reverse();
        Ok(chain)
    }
    pub fn db(&self) -> &Db {
        &self.db
    }
    pub fn save_tx_index(&self, tx_hash: &str, block_height: u64) -> std::io::Result<()> {
        let key = format!("TX_IDX:{}", tx_hash);
        self.db
            .insert(key.as_bytes(), block_height.to_string().as_bytes())?;
        Ok(())
    }
    pub fn get_tx_block_height(&self, tx_hash: &str) -> std::io::Result<Option<u64>> {
        let key = format!("TX_IDX:{}", tx_hash);
        if let Some(val) = self.db.get(key.as_bytes())? {
            let s = from_utf8(&val)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            Ok(s.parse().ok())
        } else {
            Ok(None)
        }
    }
    pub fn delete_tx_index(&self, tx_hash: &str) -> std::io::Result<()> {
        let key = format!("TX_IDX:{}", tx_hash);
        self.db.remove(key.as_bytes())?;
        Ok(())
    }
    pub fn save_account(&self, pubkey: &Address, account: &Account) -> std::io::Result<()> {
        let key = format!("ACCT:{}", pubkey);
        let val = encode(account)?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn load_all_accounts(
        &self,
    ) -> std::io::Result<std::collections::HashMap<Address, Account>> {
        let mut accounts = std::collections::HashMap::new();
        for item in self.db.scan_prefix(b"ACCT:") {
            let (key, val) = item?;
            let key_str = from_utf8(&key)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let pubkey_str = key_str.strip_prefix("ACCT:").unwrap_or(key_str);
            let pubkey = Address::from_hex(pubkey_str)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let account: Account = decode(&val)?;
            accounts.insert(pubkey, account);
        }
        Ok(accounts)
    }
    pub fn save_mempool_tx(&self, tx: &Transaction) -> std::io::Result<()> {
        let key = format!("MEMPOOL:{}", tx.hash);
        let val = encode(tx)?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn remove_mempool_tx(&self, tx_hash: &str) -> std::io::Result<()> {
        let key = format!("MEMPOOL:{}", tx_hash);
        self.db.remove(key.as_bytes())?;
        Ok(())
    }
    pub fn load_mempool_txs(&self) -> std::io::Result<Vec<Transaction>> {
        let mut txs = Vec::new();
        for item in self.db.scan_prefix(b"MEMPOOL:") {
            let (_key, val) = item?;
            let tx: Transaction = decode(&val)?;
            txs.push(tx);
        }
        Ok(txs)
    }
    pub fn save_checkpoint(
        &self,
        checkpoint: &crate::consensus::pos::Checkpoint,
    ) -> std::io::Result<()> {
        let key = format!("CP:{}", checkpoint.block_index);
        let val = encode(checkpoint)?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn load_checkpoints(&self) -> std::io::Result<Vec<crate::consensus::pos::Checkpoint>> {
        let mut cps = Vec::new();
        for item in self.db.scan_prefix(b"CP:") {
            let (_key, val) = item?;
            let cp: crate::consensus::pos::Checkpoint = decode(&val)?;
            cps.push(cp);
        }
        cps.sort_by_key(|c| c.block_index);
        Ok(cps)
    }
    pub fn save_seen_block(
        &self,
        header: &crate::core::block::BlockHeader,
        sig: &[u8],
    ) -> std::io::Result<()> {
        let producer_str = header
            .producer
            .map(|p| p.to_string())
            .unwrap_or_else(|| "unknown".to_string());
        let key = format!("SEEN:{}:{}", producer_str, header.index);
        let val = encode(&(header, sig))?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn load_all_seen_blocks(&self) -> std::io::Result<SeenBlockMap> {
        let mut seen = std::collections::HashMap::new();
        for item in self.db.scan_prefix(b"SEEN:") {
            let (key, val) = item?;
            let key_str = from_utf8(&key).unwrap_or("");
            let parts: Vec<&str> = key_str
                .strip_prefix("SEEN:")
                .unwrap_or(key_str)
                .split(':')
                .collect();
            if parts.len() == 2 {
                let producer = Address::from_hex(parts[0])
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                let index = parts[1].parse().unwrap_or(0);
                let data: (crate::core::block::BlockHeader, Vec<u8>) = decode(&val)?;
                seen.insert((producer, index), data);
            }
        }
        Ok(seen)
    }
    pub fn flush_batch(&self) -> std::io::Result<usize> {
        Ok(self.db.flush()?)
    }

    pub fn check_integrity(&self) -> Result<Vec<String>, String> {
        let mut errors = Vec::new();
        let height = self.get_canonical_height().map_err(|e| e.to_string())?;

        info!("Starting integrity audit up to height {}", height);

        let mut prev_hash = "0".repeat(64);
        for i in 0..=height {
            let block_res = self.get_block_by_height(i);
            match block_res {
                Ok(Some(block)) => {
                    let calc_hash = block.calculate_hash();
                    if block.hash != calc_hash {
                        errors.push(format!(
                            "Block {}: hash mismatch (stored: {}, calc: {})",
                            i, block.hash, calc_hash
                        ));
                    }

                    if i > 0 && block.previous_hash != prev_hash {
                        errors.push(format!(
                            "Block {}: linkage error (expected prev: {}, got: {})",
                            i, prev_hash, block.previous_hash
                        ));
                    }

                    prev_hash = block.hash.clone();
                }
                Ok(None) => {
                    errors.push(format!("Block {}: missing in index", i));
                }
                Err(e) => {
                    errors.push(format!("Block {}: read error: {}", i, e));
                }
            }
        }

        Ok(errors)
    }

    pub fn repair_index(&self) -> Result<(), String> {
        tracing::info!("Starting database index repair...");
        let last_hash = match self.get_last_hash() {
            Ok(Some(h)) => h,
            _ => return Err("Cannot repair: No tip found in DB".into()),
        };

        let mut current_hash = last_hash;
        let mut count = 0;
        while let Ok(Some(block)) = self.get_block(&current_hash) {
            let height_key = format!("HEIGHT:{}", block.index);
            self.db
                .insert(height_key.as_bytes(), block.hash.as_bytes())
                .map_err(|e| e.to_string())?;

            let state_key = format!("STATE_ROOT:{}", block.index);
            self.db
                .insert(state_key.as_bytes(), block.state_root.as_bytes())
                .map_err(|e| e.to_string())?;

            for tx in &block.transactions {
                let tx_idx_key = format!("TX_IDX:{}", tx.hash);
                self.db
                    .insert(tx_idx_key.as_bytes(), block.index.to_string().as_bytes())
                    .map_err(|e| e.to_string())?;
            }

            if block.index == 0 {
                self.db
                    .insert(b"CANONICAL_HEIGHT", b"0")
                    .map_err(|e| e.to_string())?;
            } else {
                let current_canonical = self.get_canonical_height().unwrap_or(0);
                if block.index > current_canonical {
                    self.save_canonical_height(block.index)
                        .map_err(|e| e.to_string())?;
                }
            }

            count += 1;
            if block.previous_hash == "0".repeat(64) || block.previous_hash.is_empty() {
                break;
            }
            current_hash = block.previous_hash;
        }
        tracing::info!("Repair complete. Re-indexed {} blocks", count);
        Ok(())
    }
}

impl BlockchainStorage for Storage {
    fn insert_block(&self, block: &Block) -> std::io::Result<()> {
        Storage::insert_block(self, block)
    }

    fn commit_block(&self, block: &Block, state_root: &str) -> std::io::Result<()> {
        Storage::commit_block(self, block, state_root)
    }

    fn get_block(&self, hash: &str) -> std::io::Result<Option<Block>> {
        Storage::get_block(self, hash)
    }

    fn get_block_by_height(&self, height: u64) -> std::io::Result<Option<Block>> {
        Storage::get_block_by_height(self, height)
    }

    fn get_canonical_height(&self) -> std::io::Result<u64> {
        Storage::get_canonical_height(self)
    }

    fn save_canonical_height(&self, height: u64) -> std::io::Result<()> {
        Storage::save_canonical_height(self, height)
    }

    fn save_state_root(&self, height: u64, state_root: &str) -> std::io::Result<()> {
        Storage::save_state_root(self, height, state_root)
    }

    fn get_state_root(&self, height: u64) -> std::io::Result<Option<String>> {
        Storage::get_state_root(self, height)
    }

    fn save_last_hash(&self, hash: &str) -> std::io::Result<()> {
        Storage::save_last_hash(self, hash)
    }

    fn get_last_hash(&self) -> std::io::Result<Option<String>> {
        Storage::get_last_hash(self)
    }

    fn load_chain(&self) -> std::io::Result<Vec<Block>> {
        Storage::load_chain(self)
    }

    fn delete_block(&self, height: u64) -> std::io::Result<()> {
        Storage::delete_block(self, height)
    }

    fn save_qc_blob(
        &self,
        height: u64,
        blob: &crate::consensus::qc::QcBlob,
    ) -> std::io::Result<()> {
        Storage::save_qc_blob(self, height, blob)
    }

    fn get_qc_blob(&self, height: u64) -> std::io::Result<Option<crate::consensus::qc::QcBlob>> {
        Storage::get_qc_blob(self, height)
    }

    fn delete_qc_blob(&self, height: u64) -> std::io::Result<()> {
        Storage::delete_qc_blob(self, height)
    }

    fn save_finality_cert(
        &self,
        height: u64,
        cert: &crate::chain::finality::FinalityCert,
    ) -> std::io::Result<()> {
        Storage::save_finality_cert(self, height, cert)
    }

    fn get_finality_cert(
        &self,
        height: u64,
    ) -> std::io::Result<Option<crate::chain::finality::FinalityCert>> {
        Storage::get_finality_cert(self, height)
    }

    fn delete_finality_cert(&self, height: u64) -> std::io::Result<()> {
        Storage::delete_finality_cert(self, height)
    }

    fn save_consensus_domain(&self, domain: &ConsensusDomain) -> std::io::Result<()> {
        Storage::save_consensus_domain(self, domain)
    }

    fn load_consensus_domains(&self) -> std::io::Result<Vec<ConsensusDomain>> {
        Storage::load_consensus_domains(self)
    }

    fn save_domain_commitment(&self, commitment: &DomainCommitment) -> std::io::Result<()> {
        Storage::save_domain_commitment(self, commitment)
    }

    fn save_domain_commitment_batch(
        &self,
        commitment: &DomainCommitment,
        domains: &[ConsensusDomain],
    ) -> std::io::Result<()> {
        Storage::save_domain_commitment_batch(self, commitment, domains)
    }

    fn load_domain_commitments(&self) -> std::io::Result<Vec<DomainCommitment>> {
        Storage::load_domain_commitments(self)
    }

    fn save_global_header(&self, header: &GlobalBlockHeader) -> std::io::Result<()> {
        Storage::save_global_header(self, header)
    }

    fn get_global_header(&self, height: u64) -> std::io::Result<Option<GlobalBlockHeader>> {
        Storage::get_global_header(self, height)
    }

    fn load_global_headers(&self) -> std::io::Result<Vec<GlobalBlockHeader>> {
        Storage::load_global_headers(self)
    }

    fn save_bridge_state(&self, bridge_state: &BridgeState) -> std::io::Result<()> {
        Storage::save_bridge_state(self, bridge_state)
    }

    fn load_bridge_state(&self) -> std::io::Result<Option<BridgeState>> {
        Storage::load_bridge_state(self)
    }

    fn save_cross_domain_message(&self, message: &CrossDomainMessage) -> std::io::Result<()> {
        Storage::save_cross_domain_message(self, message)
    }

    fn load_cross_domain_messages(&self) -> std::io::Result<Vec<CrossDomainMessage>> {
        Storage::load_cross_domain_messages(self)
    }

    fn save_tx_index(&self, tx_hash: &str, block_height: u64) -> std::io::Result<()> {
        Storage::save_tx_index(self, tx_hash, block_height)
    }

    fn get_tx_block_height(&self, tx_hash: &str) -> std::io::Result<Option<u64>> {
        Storage::get_tx_block_height(self, tx_hash)
    }

    fn delete_tx_index(&self, tx_hash: &str) -> std::io::Result<()> {
        Storage::delete_tx_index(self, tx_hash)
    }

    fn save_account(&self, pubkey: &Address, account: &Account) -> std::io::Result<()> {
        Storage::save_account(self, pubkey, account)
    }

    fn load_all_accounts(&self) -> std::io::Result<std::collections::HashMap<Address, Account>> {
        Storage::load_all_accounts(self)
    }

    fn save_mempool_tx(&self, tx: &Transaction) -> std::io::Result<()> {
        Storage::save_mempool_tx(self, tx)
    }

    fn remove_mempool_tx(&self, tx_hash: &str) -> std::io::Result<()> {
        Storage::remove_mempool_tx(self, tx_hash)
    }

    fn load_mempool_txs(&self) -> std::io::Result<Vec<Transaction>> {
        Storage::load_mempool_txs(self)
    }

    fn save_checkpoint(
        &self,
        checkpoint: &crate::consensus::pos::Checkpoint,
    ) -> std::io::Result<()> {
        Storage::save_checkpoint(self, checkpoint)
    }

    fn load_checkpoints(&self) -> std::io::Result<Vec<crate::consensus::pos::Checkpoint>> {
        Storage::load_checkpoints(self)
    }

    fn save_seen_block(
        &self,
        header: &crate::core::block::BlockHeader,
        sig: &[u8],
    ) -> std::io::Result<()> {
        Storage::save_seen_block(self, header, sig)
    }

    fn load_all_seen_blocks(&self) -> std::io::Result<SeenBlockMap> {
        Storage::load_all_seen_blocks(self)
    }

    fn flush_batch(&self) -> std::io::Result<usize> {
        Storage::flush_batch(self)
    }

    fn commit_durable_batch(&self, batch: &DurableCommitBatch) -> std::io::Result<()> {
        Storage::commit_durable_batch(self, batch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::account::Account;
    use crate::core::address::Address;
    use crate::core::block::Block;
    use tempfile::tempdir;

    #[test]
    fn reopening_locked_database_retries_until_lock_is_released() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("locked-reopen-db");
        let path_str = path.to_str().unwrap().to_string();

        // Hold the lock on a background thread for a short window, then release it.
        let holder_path = path_str.clone();
        let holder = std::thread::spawn(move || {
            let storage = Storage::new(&holder_path).unwrap();
            std::thread::sleep(std::time::Duration::from_millis(120));
            drop(storage);
        });

        // Give the holder thread a moment to actually acquire the lock first.
        std::thread::sleep(std::time::Duration::from_millis(20));

        // This would fail immediately without the retry loop, since the holder
        // above is still open on the same path.
        let reopened = Storage::new(&path_str);
        holder.join().unwrap();

        assert!(
            reopened.is_ok(),
            "expected retry to succeed once the lock was released, got: {:?}",
            reopened.err()
        );
    }

    #[test]
    fn test_durable_commit_batch_and_recovery() {
        let dir = tempdir().unwrap();
        let path = dir.path().to_str().unwrap();

        // 1. Create a storage instance
        let storage = Storage::new(path).unwrap();

        // 2. Form a block and some accounts
        let mut block = Block::new(1, "0".repeat(64), vec![]);
        block.hash = block.calculate_hash();

        let addr = Address::from_hex(&"01".repeat(32)).unwrap();
        let account = Account::with_balance(addr, 500);

        let batch = DurableCommitBatch {
            block: block.clone(),
            state_root: "dummy_state_root".to_string(),
            finality_cert: None,
            global_headers: vec![],
            bridge_state: None,
            accounts: vec![(addr, account)],
            settled_domains: Vec::new(),
        };

        // 3. Commit it!
        storage.commit_durable_batch(&batch).unwrap();

        // 4. Verify successfully written
        let loaded_block = storage.get_block_by_height(1).unwrap().unwrap();
        assert_eq!(loaded_block.hash, block.hash);

        let accounts = storage.load_all_accounts().unwrap();
        assert_eq!(accounts.get(&addr).unwrap().balance, 500);

        // 5. Simulate an interrupted commit at height 2
        storage.db.insert(b"IN_PROGRESS_HEIGHT", b"2").unwrap();
        let mut block2 = Block::new(2, block.hash.clone(), vec![]);
        block2.hash = block2.calculate_hash();
        storage
            .db
            .insert(block2.hash.as_bytes(), encode(&block2).unwrap())
            .unwrap();
        storage
            .db
            .insert(b"HEIGHT:2", block2.hash.as_bytes())
            .unwrap();
        storage.db.insert(b"STATE_ROOT:2", b"half_state").unwrap();
        storage.db.insert(b"LAST", block2.hash.as_bytes()).unwrap();
        storage.db.insert(b"CANONICAL_HEIGHT", b"2").unwrap();
        storage.db.flush().unwrap();

        // Drop the first storage handle to release the file lock
        drop(storage);

        // 6. Instantiate a new storage on the same path, which triggers recovery
        let storage2 = Storage::new(path).unwrap();

        // 7. Verify recovery successfully rolled back height 2 and restored tip to height 1
        assert!(storage2.db.get(b"IN_PROGRESS_HEIGHT").unwrap().is_none());
        assert!(storage2.get_block_by_height(2).unwrap().is_none());
        assert_eq!(storage2.get_canonical_height().unwrap(), 1);
        assert_eq!(storage2.get_last_hash().unwrap().unwrap(), block.hash);
    }

    fn migration_marker_fn(name: &'static str) -> MigrationFn {
        // sled::Db doesn't let us close over `name`, so route through a
        // per-name key written directly instead of a closure (fn pointers only).
        match name {
            "mark_a" => |db: &Db| -> std::io::Result<()> {
                db.insert(b"MIGRATION_MARK_A", b"applied")?;
                Ok(())
            },
            "mark_b" => |db: &Db| -> std::io::Result<()> {
                db.insert(b"MIGRATION_MARK_B", b"applied")?;
                Ok(())
            },
            "fails" => |_db: &Db| -> std::io::Result<()> {
                Err(std::io::Error::other("simulated migration failure"))
            },
            _ => unreachable!(),
        }
    }

    #[test]
    fn migrations_apply_in_order_and_stamp_each_step() {
        let dir = tempdir().unwrap();
        let db: Db = sled::open(dir.path()).unwrap();

        let steps: Vec<(u64, MigrationFn)> = vec![
            (2, migration_marker_fn("mark_a")),
            (3, migration_marker_fn("mark_b")),
        ];

        Storage::run_migrations(&db, 1, 3, &steps).unwrap();

        assert_eq!(db.get(b"MIGRATION_MARK_A").unwrap().unwrap(), b"applied");
        assert_eq!(db.get(b"MIGRATION_MARK_B").unwrap().unwrap(), b"applied");
        let version = from_utf8(&db.get(b"SCHEMA_VERSION").unwrap().unwrap())
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn migrations_skip_steps_already_applied() {
        let dir = tempdir().unwrap();
        let db: Db = sled::open(dir.path()).unwrap();

        // Already at version 2 — the (2, ...) step must not re-run.
        let steps: Vec<(u64, MigrationFn)> = vec![
            (2, migration_marker_fn("mark_a")),
            (3, migration_marker_fn("mark_b")),
        ];

        Storage::run_migrations(&db, 2, 3, &steps).unwrap();

        assert!(db.get(b"MIGRATION_MARK_A").unwrap().is_none());
        assert_eq!(db.get(b"MIGRATION_MARK_B").unwrap().unwrap(), b"applied");
    }

    #[test]
    fn migration_failure_leaves_version_at_last_successful_step() {
        let dir = tempdir().unwrap();
        let db: Db = sled::open(dir.path()).unwrap();

        let steps: Vec<(u64, MigrationFn)> = vec![
            (2, migration_marker_fn("mark_a")),
            (3, migration_marker_fn("fails")),
        ];

        let result = Storage::run_migrations(&db, 1, 3, &steps);
        assert!(result.is_err());

        // Step to v2 succeeded and was stamped; the failing step to v3 was not.
        assert_eq!(db.get(b"MIGRATION_MARK_A").unwrap().unwrap(), b"applied");
        let version = from_utf8(&db.get(b"SCHEMA_VERSION").unwrap().unwrap())
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert_eq!(version, 2);
    }

    #[test]
    fn opening_a_newer_schema_with_older_binary_is_rejected() {
        let dir = tempdir().unwrap();
        let db: Db = sled::open(dir.path()).unwrap();

        // Database was written by a future binary at schema v5; ours only supports v3.
        let result = Storage::run_migrations(&db, 5, 3, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn fresh_database_is_stamped_to_target_version_with_no_registered_migrations() {
        let dir = tempdir().unwrap();
        let db: Db = sled::open(dir.path()).unwrap();

        Storage::run_migrations(&db, 0, 1, &[]).unwrap();

        let version = from_utf8(&db.get(b"SCHEMA_VERSION").unwrap().unwrap())
            .unwrap()
            .parse::<u64>()
            .unwrap();
        assert_eq!(version, 1);
    }
}
