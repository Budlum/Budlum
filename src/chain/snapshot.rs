use crate::chain::finality::FinalityCert;
use crate::core::account::AccountState;
use crate::core::address::Address;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub height: u64,
    pub block_hash: String,
    pub chain_id: u64,
    pub created_at: u128,
    pub balances: HashMap<Address, u64>,
    pub nonces: HashMap<Address, u64>,
    pub finalized_height: u64,
    pub finalized_hash: String,
    pub validators: HashMap<Address, crate::core::account::Validator>,
    pub snapshot_hash: String,
}
impl StateSnapshot {
    pub fn from_state(
        height: u64,
        block_hash: String,
        chain_id: u64,
        account_state: &AccountState,
        finalized_height: u64,
        finalized_hash: String,
    ) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let balances = account_state.get_all_balances();
        let nonces = account_state.get_all_nonces();
        let validators = account_state.validators.clone().into_iter().collect();
        let mut snapshot = StateSnapshot {
            height,
            block_hash,
            chain_id,
            created_at,
            balances,
            nonces,
            finalized_height,
            finalized_hash,
            validators,
            snapshot_hash: String::new(),
        };
        snapshot.snapshot_hash = snapshot.calculate_hash();
        snapshot
    }
    fn calculate_hash(&self) -> String {
        use sha3::{Digest, Sha3_256};
        let mut hasher = Sha3_256::new();
        hasher.update(self.height.to_le_bytes());
        hasher.update(self.block_hash.as_bytes());
        hasher.update(self.chain_id.to_le_bytes());
        let mut balance_keys: Vec<_> = self.balances.keys().collect();
        balance_keys.sort();
        for key in balance_keys {
            hasher.update(key.0);
            hasher.update(self.balances[key].to_le_bytes());
        }
        let mut nonce_keys: Vec<_> = self.nonces.keys().collect();
        nonce_keys.sort();
        for key in nonce_keys {
            hasher.update(key.0);
            hasher.update(self.nonces[key].to_le_bytes());
        }
        let mut validator_keys: Vec<_> = self.validators.keys().collect();
        validator_keys.sort();
        for key in validator_keys {
            hasher.update(key.0);
            let v = &self.validators[key];
            hasher.update(v.stake.to_le_bytes());
            hasher.update([v.active as u8]);
            hasher.update([v.slashed as u8]);
            hasher.update([v.jailed as u8]);
            hasher.update(v.jail_until.to_le_bytes());
            hasher.update(&v.bls_public_key);
            hasher.update(&v.pop_signature);
            hasher.update(&v.pq_public_key);
        }
        hasher.update(self.finalized_height.to_le_bytes());
        hasher.update(self.finalized_hash.as_bytes());
        hex::encode(hasher.finalize())
    }
    pub fn verify(&self) -> bool {
        self.snapshot_hash == self.calculate_hash()
    }
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }
    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(data).map_err(|e| format!("Failed to parse snapshot: {}", e))
    }
    pub fn size(&self) -> usize {
        self.to_bytes().len()
    }

    pub fn chunk(&self, chunk_size: usize) -> Vec<Vec<u8>> {
        let data = self.to_bytes();
        data.chunks(chunk_size).map(|c| c.to_vec()).collect()
    }
}
#[derive(Clone)]
pub struct PruningManager {
    pub min_blocks_to_keep: u64,
    pub snapshot_interval: u64,
    pub snapshot_dir: String,
    pub archive_mode: bool,
}
impl PruningManager {
    pub fn new(min_blocks: u64, snapshot_interval: u64, snapshot_dir: String) -> Self {
        PruningManager {
            min_blocks_to_keep: min_blocks,
            snapshot_interval,
            snapshot_dir,
            archive_mode: false,
        }
    }
    /// Archive nodes keep full block history: snapshots still work normally,
    /// but `get_prunable_blocks` never returns anything to delete.
    pub fn with_archive_mode(mut self, archive_mode: bool) -> Self {
        self.archive_mode = archive_mode;
        self
    }
    pub fn should_create_snapshot(&self, height: u64) -> bool {
        height > 0 && height.is_multiple_of(self.snapshot_interval)
    }
    pub fn get_prunable_blocks(
        &self,
        chain_length: u64,
        latest_snapshot_height: u64,
        finalized_height: u64,
    ) -> Vec<u64> {
        if self.archive_mode {
            return vec![];
        }
        if chain_length <= self.min_blocks_to_keep {
            return vec![];
        }
        let prune_up_to = chain_length.saturating_sub(self.min_blocks_to_keep);

        let safe_prune_up_to = prune_up_to
            .min(latest_snapshot_height)
            .min(finalized_height);
        if safe_prune_up_to == 0 {
            return vec![];
        }
        (1..safe_prune_up_to).collect()
    }
    pub fn save_snapshot(&self, snapshot: &StateSnapshot) -> Result<(), String> {
        use std::fs;
        use std::path::Path;
        let dir = Path::new(&self.snapshot_dir);
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|e| format!("Failed to create snapshot dir: {}", e))?;
        }
        let filename = format!("snapshot_{}.json", snapshot.height);
        let path = dir.join(filename);
        let data = serde_json::to_string_pretty(snapshot)
            .map_err(|e| format!("Failed to serialize snapshot: {}", e))?;
        fs::write(&path, data).map_err(|e| format!("Failed to write snapshot: {}", e))?;
        println!(
            "Snapshot saved: {} ({} accounts)",
            path.display(),
            snapshot.balances.len()
        );
        Ok(())
    }
    pub fn load_latest_snapshot(&self) -> Result<Option<StateSnapshot>, String> {
        use std::fs;
        use std::path::Path;
        let dir = Path::new(&self.snapshot_dir);
        if !dir.exists() {
            return Ok(None);
        }
        let mut snapshots: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read snapshot dir: {}", e))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|e| e == "json")
                    .unwrap_or(false)
            })
            .collect();
        if snapshots.is_empty() {
            return Ok(None);
        }
        // Numerical sort by height
        snapshots.sort_by_key(|entry| {
            std::cmp::Reverse(get_snapshot_height(&entry.path()).unwrap_or(0))
        });
        let latest_path = snapshots[0].path();
        let data = fs::read_to_string(&latest_path)
            .map_err(|e| format!("Failed to read snapshot: {}", e))?;
        let snapshot: StateSnapshot = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                let mut quarantine_path = latest_path.clone();
                quarantine_path.set_extension("json.corrupted");
                let _ = fs::rename(&latest_path, &quarantine_path);
                return Err(format!("Failed to parse snapshot: {}", e));
            }
        };
        if !snapshot.verify() {
            let mut quarantine_path = latest_path.clone();
            quarantine_path.set_extension("json.corrupted");
            let _ = fs::rename(&latest_path, &quarantine_path);
            return Err("Snapshot integrity check failed".to_string());
        }
        println!("Loaded snapshot at height {}", snapshot.height);
        Ok(Some(snapshot))
    }

    pub fn save_snapshot_v2(&self, snapshot: &StateSnapshotV2) -> Result<(), String> {
        use std::fs;
        use std::path::Path;
        let dir = Path::new(&self.snapshot_dir);
        if !dir.exists() {
            fs::create_dir_all(dir).map_err(|e| format!("Failed to create snapshot dir: {}", e))?;
        }
        let filename = format!("snapshot_{}.json", snapshot.height);
        let path = dir.join(filename);
        let data = serde_json::to_string_pretty(snapshot)
            .map_err(|e| format!("Failed to serialize snapshot v2: {}", e))?;
        fs::write(&path, data).map_err(|e| format!("Failed to write snapshot v2: {}", e))?;
        println!(
            "Snapshot V2 saved: {} ({} accounts)",
            path.display(),
            snapshot.balances.len()
        );
        Ok(())
    }

    pub fn load_latest_snapshot_v2(&self) -> Result<Option<StateSnapshotV2>, String> {
        use std::fs;
        use std::path::Path;
        let dir = Path::new(&self.snapshot_dir);
        if !dir.exists() {
            return Ok(None);
        }
        let mut snapshots: Vec<_> = fs::read_dir(dir)
            .map_err(|e| format!("Failed to read snapshot dir: {}", e))?
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|e| e == "json")
                    .unwrap_or(false)
            })
            .collect();
        if snapshots.is_empty() {
            return Ok(None);
        }
        // Numerical sort by height
        snapshots.sort_by_key(|entry| {
            std::cmp::Reverse(get_snapshot_height(&entry.path()).unwrap_or(0))
        });
        let latest_path = snapshots[0].path();
        let data = fs::read_to_string(&latest_path)
            .map_err(|e| format!("Failed to read snapshot: {}", e))?;
        let snapshot: StateSnapshotV2 = match serde_json::from_str(&data) {
            Ok(s) => s,
            Err(e) => {
                let mut quarantine_path = latest_path.clone();
                quarantine_path.set_extension("json.corrupted");
                let _ = fs::rename(&latest_path, &quarantine_path);
                return Err(format!("Failed to parse snapshot V2: {}", e));
            }
        };
        if !snapshot.verify() {
            let mut quarantine_path = latest_path.clone();
            quarantine_path.set_extension("json.corrupted");
            let _ = fs::rename(&latest_path, &quarantine_path);
            return Err("Snapshot V2 integrity check failed".to_string());
        }
        println!("Loaded snapshot V2 at height {}", snapshot.height);
        Ok(Some(snapshot))
    }
}

fn get_snapshot_height(path: &std::path::Path) -> Option<u64> {
    let stem = path.file_stem()?.to_str()?;
    let height_str = stem.strip_prefix("snapshot_")?;
    height_str.parse::<u64>().ok()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshotV2 {
    pub schema_version: u32,
    pub height: u64,
    pub block_hash: String,
    pub genesis_hash: String,
    pub chain_id: u64,
    pub created_at: u128,
    pub balances: HashMap<Address, u64>,
    pub nonces: HashMap<Address, u64>,
    pub finalized_height: u64,
    pub finalized_hash: String,
    pub validators: HashMap<Address, crate::core::account::Validator>,
    pub unbonding_queue: Vec<crate::core::account::UnbondingEntry>,
    pub finality_certificates: Vec<FinalityCert>,

    // ConsensusStateV2 fields:
    pub epoch_index: u64,
    pub last_epoch_time: u64,
    pub base_fee: u64,
    pub block_reward: u64,
    pub bridge_root: [u8; 32],
    pub message_root: [u8; 32],
    pub settlement_root: [u8; 32],
    pub global_header_summary: [u8; 32],

    pub snapshot_hash: String,
}

pub struct StateSnapshotV2Params {
    pub height: u64,
    pub block_hash: String,
    pub genesis_hash: String,
    pub chain_id: u64,
    pub finalized_height: u64,
    pub finalized_hash: String,
    pub finality_certificates: Vec<FinalityCert>,
}

impl StateSnapshotV2 {
    pub fn from_state(account_state: &AccountState, params: StateSnapshotV2Params) -> Self {
        let created_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        let balances = account_state.get_all_balances();
        let nonces = account_state.get_all_nonces();
        let validators = account_state.validators.clone().into_iter().collect();
        let unbonding_queue = account_state.unbonding_queue.clone();

        let mut snapshot = StateSnapshotV2 {
            schema_version: 2,
            height: params.height,
            block_hash: params.block_hash,
            genesis_hash: params.genesis_hash,
            chain_id: params.chain_id,
            created_at,
            balances,
            nonces,
            finalized_height: params.finalized_height,
            finalized_hash: params.finalized_hash,
            validators,
            unbonding_queue,
            finality_certificates: params.finality_certificates,
            epoch_index: account_state.epoch_index,
            last_epoch_time: account_state.last_epoch_time,
            base_fee: account_state.base_fee,
            block_reward: account_state.block_reward,
            bridge_root: account_state.bridge_root,
            message_root: account_state.message_root,
            settlement_root: account_state.settlement_root,
            global_header_summary: account_state.global_header_summary,
            snapshot_hash: String::new(),
        };
        snapshot.snapshot_hash = snapshot.calculate_hash();
        snapshot
    }

    fn calculate_hash(&self) -> String {
        use sha3::{Digest, Sha3_256};
        let mut hasher = Sha3_256::new();
        hasher.update(self.schema_version.to_le_bytes());
        hasher.update(self.height.to_le_bytes());
        hasher.update(self.block_hash.as_bytes());
        hasher.update(self.genesis_hash.as_bytes());
        hasher.update(self.chain_id.to_le_bytes());

        let mut balance_keys: Vec<_> = self.balances.keys().collect();
        balance_keys.sort();
        for key in balance_keys {
            hasher.update(key.0);
            hasher.update(self.balances[key].to_le_bytes());
        }

        let mut nonce_keys: Vec<_> = self.nonces.keys().collect();
        nonce_keys.sort();
        for key in nonce_keys {
            hasher.update(key.0);
            hasher.update(self.nonces[key].to_le_bytes());
        }

        let mut validator_keys: Vec<_> = self.validators.keys().collect();
        validator_keys.sort();
        for key in validator_keys {
            hasher.update(key.0);
            let v = &self.validators[key];
            hasher.update(v.stake.to_le_bytes());
            hasher.update([v.active as u8]);
            hasher.update([v.slashed as u8]);
            hasher.update([v.jailed as u8]);
            hasher.update(v.jail_until.to_le_bytes());
            hasher.update(&v.bls_public_key);
            hasher.update(&v.pop_signature);
            hasher.update(&v.pq_public_key);
        }

        for entry in &self.unbonding_queue {
            hasher.update(entry.address.0);
            hasher.update(entry.amount.to_le_bytes());
            hasher.update(entry.release_epoch.to_le_bytes());
        }

        hasher.update(self.finalized_height.to_le_bytes());
        hasher.update(self.finalized_hash.as_bytes());

        hasher.update(self.epoch_index.to_le_bytes());
        hasher.update(self.last_epoch_time.to_le_bytes());
        hasher.update(self.base_fee.to_le_bytes());
        hasher.update(self.block_reward.to_le_bytes());
        hasher.update(self.bridge_root);
        hasher.update(self.message_root);
        hasher.update(self.settlement_root);
        hasher.update(self.global_header_summary);

        hex::encode(hasher.finalize())
    }

    pub fn verify(&self) -> bool {
        self.snapshot_hash == self.calculate_hash()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, String> {
        serde_json::from_slice(data).map_err(|e| format!("Failed to parse snapshot V2: {}", e))
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_snapshot_creation() {
        let account_state = AccountState::new();
        let snapshot = StateSnapshot::from_state(
            100,
            "blockhash123".to_string(),
            1337,
            &account_state,
            0,
            "genhash".to_string(),
        );
        assert_eq!(snapshot.height, 100);
        assert_eq!(snapshot.chain_id, 1337);
        assert!(!snapshot.snapshot_hash.is_empty());
    }
    #[test]
    fn test_snapshot_verify() {
        let account_state = AccountState::new();
        let snapshot = StateSnapshot::from_state(
            50,
            "hash".to_string(),
            42,
            &account_state,
            10,
            "finalhash".to_string(),
        );
        assert!(snapshot.verify());
    }
    #[test]
    fn test_pruning_manager() {
        let manager = PruningManager::new(100, 1000, "./snapshots".to_string());

        let prunable = manager.get_prunable_blocks(50, 0, 0);
        assert!(prunable.is_empty());

        let prunable = manager.get_prunable_blocks(200, 50, 50);
        assert_eq!(prunable.len(), 49);
    }
    #[test]
    fn test_archive_mode_never_prunes() {
        let manager =
            PruningManager::new(100, 1000, "./snapshots".to_string()).with_archive_mode(true);

        // Same inputs that produced 49 prunable blocks without archive mode.
        let prunable = manager.get_prunable_blocks(200, 50, 50);
        assert!(prunable.is_empty());

        // Even a very long chain with everything finalized/snapshotted stays untouched.
        let prunable = manager.get_prunable_blocks(1_000_000, 999_999, 999_999);
        assert!(prunable.is_empty());
    }
    #[test]
    fn test_archive_mode_defaults_to_false() {
        let manager = PruningManager::new(100, 1000, "./snapshots".to_string());
        assert!(!manager.archive_mode);
    }
    #[test]
    fn test_snapshot_interval() {
        let manager = PruningManager::new(100, 1000, "./snapshots".to_string());
        assert!(!manager.should_create_snapshot(0));
        assert!(!manager.should_create_snapshot(500));
        assert!(manager.should_create_snapshot(1000));
        assert!(manager.should_create_snapshot(2000));
    }

    #[test]
    fn test_snapshot_v2_creation_and_numerical_sorting() {
        let account_state = AccountState::new();
        let snapshot_v2 = StateSnapshotV2::from_state(
            &account_state,
            StateSnapshotV2Params {
                height: 105,
                block_hash: "block_hash_v2".to_string(),
                genesis_hash: "genesis_hash_v2".to_string(),
                chain_id: 42,
                finalized_height: 50,
                finalized_hash: "finalized_hash_v2".to_string(),
                finality_certificates: vec![],
            },
        );

        assert_eq!(snapshot_v2.schema_version, 2);
        assert_eq!(snapshot_v2.height, 105);
        assert!(snapshot_v2.verify());

        let bytes = snapshot_v2.to_bytes();
        let deserialized = StateSnapshotV2::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.height, 105);
        assert_eq!(deserialized.schema_version, 2);
        assert!(deserialized.verify());

        // Test numerical sorting helper
        let path1 = std::path::Path::new("snapshot_100.json");
        let path2 = std::path::Path::new("snapshot_9.json");
        assert_eq!(get_snapshot_height(path1).unwrap(), 100);
        assert_eq!(get_snapshot_height(path2).unwrap(), 9);
    }

    #[test]
    fn test_snapshot_quarantine() {
        use std::fs;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let manager = PruningManager::new(100, 1000, dir.path().to_str().unwrap().to_string());

        // 1. Create a dummy corrupted snapshot file
        let path = dir.path().join("snapshot_50.json");
        fs::write(&path, "corrupted JSON data").unwrap();

        // 2. Try loading it
        let res = manager.load_latest_snapshot();
        assert!(res.is_err());

        // 3. Verify it was quarantined (renamed to snapshot_50.json.corrupted)
        let quarantined_path = dir.path().join("snapshot_50.json.corrupted");
        assert!(quarantined_path.exists());
        assert!(!path.exists());
    }
}
