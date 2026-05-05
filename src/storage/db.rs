use crate::core::account::Account;
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use crate::cross_domain::message::CrossDomainMessage;
use crate::cross_domain::BridgeState;
use crate::domain::{ConsensusDomain, DomainCommitment};
use crate::settlement::GlobalBlockHeader;
use crate::storage::traits::{BlockchainStorage, SeenBlockMap};
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

#[derive(Clone, Debug)]
pub struct Storage {
    db: Db,
}
impl Storage {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let db = sled::open(path)?;
        let storage = Storage { db };
        storage.apply_migrations()?;
        Ok(storage)
    }

    pub fn apply_migrations(&self) -> std::io::Result<()> {
        const CURRENT_SCHEMA_VERSION: u64 = 1;
        let current = self.schema_version()?;
        if current < CURRENT_SCHEMA_VERSION {
            self.db.insert(
                b"SCHEMA_VERSION",
                CURRENT_SCHEMA_VERSION.to_string().as_bytes(),
            )?;
            self.db.flush()?;
        }
        Ok(())
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
        let last_hash_key = "LAST_BLOCK_HASH";
        let last_hash = match self.db.get(last_hash_key) {
            Ok(Some(h)) => String::from_utf8_lossy(&h).to_string(),
            _ => return Err("Cannot repair: No tip found in DB".into()),
        };

        let mut current_hash = last_hash;
        let mut count = 0;
        loop {
            if let Ok(Some(data)) = self.db.get(format!("BLOCK:{}", current_hash)) {
                let block: crate::core::block::Block =
                    decode(&data).map_err(|e| format!("De-serial error during repair: {}", e))?;

                let height_key = format!("BLOCK_HEIGHT:{}", block.index);
                self.db
                    .insert(height_key, block.hash.as_bytes())
                    .map_err(|e| e.to_string())?;

                let hash_key = format!("BLOCK_HASH:{}", block.index);
                self.db
                    .insert(hash_key, block.hash.as_bytes())
                    .map_err(|e| e.to_string())?;

                for tx in &block.transactions {
                    self.db
                        .insert(format!("TX_BLOCK:{}", tx.hash), &block.index.to_le_bytes())
                        .map_err(|e| e.to_string())?;
                }

                count += 1;
                if block.previous_hash == "0".repeat(64) {
                    break;
                }
                current_hash = block.previous_hash;
            } else {
                break;
            }
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
}
