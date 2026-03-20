use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::core::chain_config::{MAX_QC_BLOB_BYTES, QC_BLOB_TTL_EPOCHS};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QcBlob {
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub pq_signatures: Vec<PqSignatureEntry>,
    pub merkle_root: String,
    pub created_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqSignatureEntry {
    pub validator_index: u32,
    pub validator_address: String,
    pub dilithium_signature: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PqFraudProof {
    pub epoch: u64,
    pub validator_index: u32,
    pub validator_address: String,
    pub claimed_bls_sig: Vec<u8>,
    pub dilithium_signature: Vec<u8>,
    pub merkle_proof: Vec<Vec<u8>>,
    pub leaf_index: u32,
}

impl QcBlob {
    pub fn new(
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: String,
        pq_signatures: Vec<PqSignatureEntry>,
    ) -> Self {
        let merkle_root = Self::compute_merkle_root(&pq_signatures);
        QcBlob {
            epoch,
            checkpoint_height,
            checkpoint_hash,
            pq_signatures,
            merkle_root,
            created_epoch: epoch,
        }
    }

    pub fn compute_merkle_root(signatures: &[PqSignatureEntry]) -> String {
        if signatures.is_empty() {
            return String::from(
                "0000000000000000000000000000000000000000000000000000000000000000",
            );
        }

        let mut leaves: Vec<[u8; 32]> = signatures
            .iter()
            .map(|entry| {
                let mut hasher = Sha3_256::new();
                hasher.update(entry.validator_index.to_le_bytes());
                hasher.update(entry.validator_address.as_bytes());
                hasher.update(&entry.dilithium_signature);
                let result = hasher.finalize();
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&result);
                arr
            })
            .collect();

        while leaves.len() > 1 {
            let mut next_level = Vec::new();
            let mut i = 0;
            while i < leaves.len() {
                let left = &leaves[i];
                let right = if i + 1 < leaves.len() {
                    &leaves[i + 1]
                } else {
                    left
                };
                let mut hasher = Sha3_256::new();
                hasher.update(left);
                hasher.update(right);
                let result = hasher.finalize();
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&result);
                next_level.push(arr);
                i += 2;
            }
            leaves = next_level;
        }

        hex::encode(leaves[0])
    }

    pub fn is_expired(&self, current_epoch: u64) -> bool {
        current_epoch > self.created_epoch + QC_BLOB_TTL_EPOCHS
    }

    pub fn validate_size(&self) -> Result<(), String> {
        let estimated_size = self
            .pq_signatures
            .iter()
            .map(|s| s.dilithium_signature.len() + s.validator_address.len() + 8)
            .sum::<usize>();

        if estimated_size > MAX_QC_BLOB_BYTES {
            return Err(format!(
                "QcBlob too large: {} bytes (max: {})",
                estimated_size, MAX_QC_BLOB_BYTES
            ));
        }
        Ok(())
    }

    pub fn verify_merkle_root(&self) -> bool {
        let computed = Self::compute_merkle_root(&self.pq_signatures);
        computed == self.merkle_root
    }
}

impl PqFraudProof {
    pub fn new(
        epoch: u64,
        validator_index: u32,
        validator_address: String,
        claimed_bls_sig: Vec<u8>,
        dilithium_signature: Vec<u8>,
        merkle_proof: Vec<Vec<u8>>,
        leaf_index: u32,
    ) -> Self {
        PqFraudProof {
            epoch,
            validator_index,
            validator_address,
            claimed_bls_sig,
            dilithium_signature,
            merkle_proof,
            leaf_index,
        }
    }

    pub fn verify_inclusion(&self, merkle_root: &str) -> Result<(), String> {
        let mut hasher = Sha3_256::new();
        hasher.update(self.validator_index.to_le_bytes());
        hasher.update(self.validator_address.as_bytes());
        hasher.update(&self.dilithium_signature);
        let leaf_hash = hasher.finalize();

        let mut current = [0u8; 32];
        current.copy_from_slice(&leaf_hash);

        let mut idx = self.leaf_index;
        for proof_element in &self.merkle_proof {
            let mut hasher = Sha3_256::new();
            if idx % 2 == 0 {
                hasher.update(&current);
                hasher.update(proof_element);
            } else {
                hasher.update(proof_element);
                hasher.update(&current);
            }
            let result = hasher.finalize();
            current.copy_from_slice(&result);
            idx /= 2;
        }

        let computed_root = hex::encode(current);
        if computed_root != merkle_root {
            return Err(format!(
                "Merkle proof invalid: computed {} != expected {}",
                computed_root, merkle_root
            ));
        }
        Ok(())
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.dilithium_signature.is_empty() {
            return Err("Empty Dilithium signature".into());
        }
        if self.claimed_bls_sig.is_empty() {
            return Err("Empty claimed BLS signature".into());
        }
        if self.merkle_proof.is_empty() {
            return Err("Empty merkle proof".into());
        }
        Ok(())
    }
}

pub fn pq_signing_message(epoch: u64, checkpoint_hash: &str, validator_index: u32) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(b"BUDLUM_PQ_QC");
    msg.extend_from_slice(&epoch.to_le_bytes());
    msg.extend_from_slice(checkpoint_hash.as_bytes());
    msg.extend_from_slice(&validator_index.to_le_bytes());
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entries(n: usize) -> Vec<PqSignatureEntry> {
        (0..n)
            .map(|i| PqSignatureEntry {
                validator_index: i as u32,
                validator_address: format!("validator_{}", i),
                dilithium_signature: vec![i as u8; 64],
            })
            .collect()
    }

    #[test]
    fn test_qc_blob_creation() {
        let entries = make_entries(4);
        let blob = QcBlob::new(1, 100, "cp_hash".into(), entries);
        assert_eq!(blob.epoch, 1);
        assert_eq!(blob.checkpoint_height, 100);
        assert!(!blob.merkle_root.is_empty());
        assert!(blob.verify_merkle_root());
    }

    #[test]
    fn test_merkle_root_deterministic() {
        let entries = make_entries(4);
        let root1 = QcBlob::compute_merkle_root(&entries);
        let root2 = QcBlob::compute_merkle_root(&entries);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_merkle_root_changes_with_data() {
        let entries1 = make_entries(4);
        let mut entries2 = make_entries(4);
        entries2[0].dilithium_signature = vec![0xFF; 64];
        let root1 = QcBlob::compute_merkle_root(&entries1);
        let root2 = QcBlob::compute_merkle_root(&entries2);
        assert_ne!(root1, root2);
    }

    #[test]
    fn test_empty_merkle_root() {
        let root = QcBlob::compute_merkle_root(&[]);
        assert_eq!(root.len(), 64);
        assert!(root.chars().all(|c| c == '0'));
    }

    #[test]
    fn test_blob_expiry() {
        let entries = make_entries(2);
        let blob = QcBlob::new(1, 100, "cp".into(), entries);
        assert!(!blob.is_expired(5));
        assert!(!blob.is_expired(11));
        assert!(blob.is_expired(12));
    }

    #[test]
    fn test_blob_size_validation() {
        let entries = make_entries(4);
        let blob = QcBlob::new(1, 100, "cp".into(), entries);
        assert!(blob.validate_size().is_ok());
    }

    #[test]
    fn test_fraud_proof_validation() {
        let proof = PqFraudProof::new(
            1,
            0,
            "validator_0".into(),
            vec![1; 48],
            vec![1; 64],
            vec![vec![0; 32]],
            0,
        );
        assert!(proof.validate().is_ok());
    }

    #[test]
    fn test_fraud_proof_rejects_empty() {
        let proof = PqFraudProof::new(
            1,
            0,
            "validator_0".into(),
            vec![],
            vec![1; 64],
            vec![vec![0; 32]],
            0,
        );
        assert!(proof.validate().is_err());

        let proof2 = PqFraudProof::new(
            1,
            0,
            "validator_0".into(),
            vec![1; 48],
            vec![],
            vec![vec![0; 32]],
            0,
        );
        assert!(proof2.validate().is_err());
    }

    #[test]
    fn test_pq_signing_message_deterministic() {
        let msg1 = pq_signing_message(1, "hash", 0);
        let msg2 = pq_signing_message(1, "hash", 0);
        assert_eq!(msg1, msg2);

        let msg3 = pq_signing_message(2, "hash", 0);
        assert_ne!(msg1, msg3);
    }

    #[test]
    fn test_single_entry_merkle() {
        let entries = make_entries(1);
        let blob = QcBlob::new(1, 100, "cp".into(), entries);
        assert!(blob.verify_merkle_root());
    }
}
