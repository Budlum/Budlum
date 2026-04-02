use crate::core::block::Block;
use crate::core::account::Account;
use crate::core::address::Address;
use crate::core::transaction::Transaction;
use sled::Db;
use std::str::from_utf8;
use tracing::info;
#[derive(Clone, Debug)]
pub struct Storage {
    db: Db,
}
impl Storage {
    pub fn new(path: &str) -> std::io::Result<Self> {
        let db = sled::open(path)?;
        Ok(Storage { db })
    }
    pub fn insert_block(&self, block: &Block) -> std::io::Result<()> {
        let key = block.hash.clone();
        let val = serde_json::to_vec(block)?;
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
        
        let block_bytes = serde_json::to_vec(block)?;
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
            let block: Block = serde_json::from_slice(&val)?;
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
        let val = serde_json::to_vec(blob)?;
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
            let blob = serde_json::from_slice(&val)?;
            Ok(Some(blob))
        } else {
            Ok(None)
        }
    }
    pub fn save_finality_cert(
        &self,
        height: u64,
        cert: &crate::chain::finality::FinalityCert,
    ) -> std::io::Result<()> {
        let key = format!("FINALITY_CERT:{}", height);
        let val = serde_json::to_vec(cert)?;
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
            let cert = serde_json::from_slice(&val)?;
            Ok(Some(cert))
        } else {
            Ok(None)
        }
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
        self.db.insert(key.as_bytes(), block_height.to_string().as_bytes())?;
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
    pub fn save_account(&self, pubkey: &Address, account: &Account) -> std::io::Result<()> {
        let key = format!("ACCT:{}", pubkey);
        let val = serde_json::to_vec(account)?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn load_all_accounts(&self) -> std::io::Result<std::collections::HashMap<Address, Account>> {
        let mut accounts = std::collections::HashMap::new();
        for item in self.db.scan_prefix(b"ACCT:") {
            let (key, val) = item?;
            let key_str = from_utf8(&key)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let pubkey_str = key_str.strip_prefix("ACCT:").unwrap_or(key_str);
            let pubkey = Address::from_hex(pubkey_str)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            let account: Account = serde_json::from_slice(&val)?;
            accounts.insert(pubkey, account);
        }
        Ok(accounts)
    }
    pub fn save_mempool_tx(&self, tx: &Transaction) -> std::io::Result<()> {
        let key = format!("MEMPOOL:{}", tx.hash);
        let val = serde_json::to_vec(tx)?;
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
            let tx: Transaction = serde_json::from_slice(&val)?;
            txs.push(tx);
        }
        Ok(txs)
    }
    pub fn save_checkpoint(&self, checkpoint: &crate::consensus::pos::Checkpoint) -> std::io::Result<()> {
        let key = format!("CP:{}", checkpoint.block_index);
        let val = serde_json::to_vec(checkpoint)?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn load_checkpoints(&self) -> std::io::Result<Vec<crate::consensus::pos::Checkpoint>> {
        let mut cps = Vec::new();
        for item in self.db.scan_prefix(b"CP:") {
            let (_key, val) = item?;
            let cp: crate::consensus::pos::Checkpoint = serde_json::from_slice(&val)?;
            cps.push(cp);
        }
        cps.sort_by_key(|c| c.block_index);
        Ok(cps)
    }
    pub fn save_seen_block(&self, header: &crate::core::block::BlockHeader, sig: &[u8]) -> std::io::Result<()> {
        let producer_str = header.producer.map(|p| p.to_string()).unwrap_or_else(|| "unknown".to_string());
        let key = format!("SEEN:{}:{}", producer_str, header.index);
        let val = serde_json::to_vec(&(header, sig))?;
        self.db.insert(key.as_bytes(), val)?;
        Ok(())
    }
    pub fn load_all_seen_blocks(&self) -> std::io::Result<std::collections::HashMap<(Address, u64), (crate::core::block::BlockHeader, Vec<u8>)>> {
        let mut seen = std::collections::HashMap::new();
        for item in self.db.scan_prefix(b"SEEN:") {
            let (key, val) = item?;
            let key_str = from_utf8(&key).unwrap_or("");
            let parts: Vec<&str> = key_str.strip_prefix("SEEN:").unwrap_or(key_str).split(':').collect();
            if parts.len() == 2 {
                let producer = Address::from_hex(parts[0]).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
                let index = parts[1].parse().unwrap_or(0);
                let data: (crate::core::block::BlockHeader, Vec<u8>) = serde_json::from_slice(&val)?;
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
                    // Check hash
                    let calc_hash = block.calculate_hash();
                    if block.hash != calc_hash {
                        errors.push(format!("Block {}: hash mismatch (stored: {}, calc: {})", i, block.hash, calc_hash));
                    }
                    
                    // Check linkage
                    if i > 0 && block.previous_hash != prev_hash {
                        errors.push(format!("Block {}: linkage error (expected prev: {}, got: {})", i, prev_hash, block.previous_hash));
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
                let block: crate::core::block::Block = serde_json::from_slice(&data)
                    .map_err(|e| format!("De-serial error during repair: {}", e))?;
                
                let height_key = format!("BLOCK_HEIGHT:{}", block.index);
                self.db.insert(height_key, block.hash.as_bytes()).map_err(|e| e.to_string())?;
                
                let hash_key = format!("BLOCK_HASH:{}", block.index);
                self.db.insert(hash_key, block.hash.as_bytes()).map_err(|e| e.to_string())?;

                for tx in &block.transactions {
                    self.db.insert(format!("TX_BLOCK:{}", tx.hash), &block.index.to_le_bytes()).map_err(|e| e.to_string())?;
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
