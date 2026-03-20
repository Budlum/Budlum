use crate::core::block::Block;
use sled::Db;
use std::str::from_utf8;
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
        self.db.insert(key, val)?;
        let height_key = format!("HEIGHT:{}", block.index);
        self.db
            .insert(height_key.as_bytes(), block.hash.as_bytes())?;
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
            self.db.remove(&hash_val)?;
            self.db.remove(key.as_bytes())?;
            let state_root_key = format!("STATE_ROOT:{}", height);
            self.db.remove(state_root_key.as_bytes())?;
            let cert_key = format!("FINALITY_CERT:{}", height);
            self.db.remove(cert_key.as_bytes())?;
            let qc_key = format!("QC_BLOB:{}", height);
            self.db.remove(qc_key.as_bytes())?;
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
            let hash = from_utf8(&val).unwrap().to_string();
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
}
