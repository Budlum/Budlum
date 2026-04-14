use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};

use crate::chain::finality::ValidatorSetSnapshot;
use crate::core::chain_config::{MAX_QC_BLOB_BYTES, QC_BLOB_TTL_EPOCHS};
use crate::crypto::primitives::PqKeyPair;

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
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
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

    fn leaf_hash(entry: &PqSignatureEntry) -> [u8; 32] {
        let mut hasher = Sha3_256::new();
        hasher.update(entry.validator_index.to_le_bytes());
        hasher.update(entry.validator_address.as_bytes());
        hasher.update(&entry.dilithium_signature);
        let result = hasher.finalize();
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&result);
        arr
    }

    fn merkle_layers(signatures: &[PqSignatureEntry]) -> Vec<Vec<[u8; 32]>> {
        if signatures.is_empty() {
            return Vec::new();
        }

        let mut layers: Vec<Vec<[u8; 32]>> = Vec::new();
        layers.push(signatures.iter().map(Self::leaf_hash).collect());

        while layers.last().map(|layer| layer.len()).unwrap_or(0) > 1 {
            let current = layers.last().cloned().unwrap_or_default();
            let mut next_level = Vec::new();
            let mut i = 0;
            while i < current.len() {
                let left = &current[i];
                let right = if i + 1 < current.len() {
                    &current[i + 1]
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
            layers.push(next_level);
        }

        layers
    }

    pub fn compute_merkle_root(signatures: &[PqSignatureEntry]) -> String {
        if signatures.is_empty() {
            return String::from(
                "0000000000000000000000000000000000000000000000000000000000000000",
            );
        }

        let layers = Self::merkle_layers(signatures);
        hex::encode(
            layers
                .last()
                .and_then(|layer| layer.first())
                .copied()
                .unwrap_or([0u8; 32]),
        )
    }

    pub fn merkle_proof(&self, leaf_index: usize) -> Result<Vec<Vec<u8>>, String> {
        if self.pq_signatures.is_empty() {
            return Err("QcBlob has no PQ signatures".into());
        }
        if leaf_index >= self.pq_signatures.len() {
            return Err(format!(
                "Leaf index {} out of range for {} signatures",
                leaf_index,
                self.pq_signatures.len()
            ));
        }

        let layers = Self::merkle_layers(&self.pq_signatures);
        let mut proof = Vec::new();
        let mut idx = leaf_index;

        for layer in layers.iter().take(layers.len().saturating_sub(1)) {
            let sibling_idx = if idx % 2 == 0 {
                (idx + 1).min(layer.len().saturating_sub(1))
            } else {
                idx.saturating_sub(1)
            };
            proof.push(layer[sibling_idx].to_vec());
            idx /= 2;
        }

        Ok(proof)
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

    pub fn verify_against_snapshot(
        &self,
        snapshot: &ValidatorSetSnapshot,
        required_signers: Option<&[usize]>,
        current_epoch: Option<u64>,
    ) -> Result<(), String> {
        if self.epoch != snapshot.epoch {
            return Err(format!(
                "QcBlob epoch mismatch: expected {}, got {}",
                snapshot.epoch, self.epoch
            ));
        }
        if self.checkpoint_height == 0 {
            return Err("QcBlob checkpoint height must be > 0".into());
        }
        if self.checkpoint_hash.is_empty() {
            return Err("QcBlob checkpoint hash is empty".into());
        }
        if let Some(epoch) = current_epoch {
            if self.is_expired(epoch) {
                return Err(format!(
                    "QcBlob expired at epoch {} (created at {})",
                    epoch, self.created_epoch
                ));
            }
        }
        self.validate_size()?;
        if !self.verify_merkle_root() {
            return Err("QcBlob merkle root mismatch".into());
        }

        let mut verified_indices = HashSet::new();
        for entry in &self.pq_signatures {
            let idx = entry.validator_index as usize;
            let validator = snapshot.validators.get(idx).ok_or_else(|| {
                format!(
                    "QcBlob references unknown validator index {}",
                    entry.validator_index
                )
            })?;

            if validator.address.to_string() != entry.validator_address {
                return Err(format!(
                    "QcBlob validator address mismatch at index {}: expected {}, got {}",
                    entry.validator_index, validator.address, entry.validator_address
                ));
            }

            if validator.pq_public_key.is_empty() {
                return Err(format!(
                    "Validator {} has no Dilithium public key",
                    validator.address
                ));
            }

            if !verified_indices.insert(idx) {
                return Err(format!(
                    "Duplicate PQ signature for validator index {}",
                    entry.validator_index
                ));
            }

            let message =
                pq_signing_message(self.epoch, &self.checkpoint_hash, entry.validator_index);
            PqKeyPair::verify(
                &validator.pq_public_key,
                &message,
                &entry.dilithium_signature,
            )
            .map_err(|e| {
                format!(
                    "Invalid Dilithium signature for validator {}: {}",
                    validator.address, e
                )
            })?;
        }

        if let Some(required_signers) = required_signers {
            for signer_idx in required_signers {
                if !verified_indices.contains(signer_idx) {
                    return Err(format!(
                        "QcBlob missing PQ attestation for validator index {}",
                        signer_idx
                    ));
                }
            }
        }

        Ok(())
    }

    pub fn detect_fraud_proofs(&self, snapshot: &ValidatorSetSnapshot) -> Vec<PqFraudProof> {
        let mut proofs = Vec::new();

        for (leaf_index, entry) in self.pq_signatures.iter().enumerate() {
            let Some(validator) = snapshot.validators.get(entry.validator_index as usize) else {
                continue;
            };
            if validator.address.to_string() != entry.validator_address {
                continue;
            }
            if validator.pq_public_key.is_empty() {
                continue;
            }

            let message =
                pq_signing_message(self.epoch, &self.checkpoint_hash, entry.validator_index);
            if PqKeyPair::verify(
                &validator.pq_public_key,
                &message,
                &entry.dilithium_signature,
            )
            .is_err()
            {
                if let Ok(merkle_proof) = self.merkle_proof(leaf_index) {
                    proofs.push(PqFraudProof::new(
                        self.epoch,
                        self.checkpoint_height,
                        self.checkpoint_hash.clone(),
                        entry.validator_index,
                        entry.validator_address.clone(),
                        vec![0u8; 48],
                        entry.dilithium_signature.clone(),
                        merkle_proof,
                        leaf_index as u32,
                    ));
                }
            }
        }

        proofs
    }
}

impl PqFraudProof {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: String,
        validator_index: u32,
        validator_address: String,
        claimed_bls_sig: Vec<u8>,
        dilithium_signature: Vec<u8>,
        merkle_proof: Vec<Vec<u8>>,
        leaf_index: u32,
    ) -> Self {
        PqFraudProof {
            epoch,
            checkpoint_height,
            checkpoint_hash,
            validator_index,
            validator_address,
            claimed_bls_sig,
            dilithium_signature,
            merkle_proof,
            leaf_index,
        }
    }

    pub fn verify_inclusion(&self, merkle_root: &str) -> Result<(), String> {
        let mut current = QcBlob::leaf_hash(&PqSignatureEntry {
            validator_index: self.validator_index,
            validator_address: self.validator_address.clone(),
            dilithium_signature: self.dilithium_signature.clone(),
        });

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
        if self.checkpoint_height == 0 {
            return Err("Empty checkpoint height".into());
        }
        if self.checkpoint_hash.is_empty() {
            return Err("Empty checkpoint hash".into());
        }
        if self.dilithium_signature.is_empty() {
            return Err("Empty Dilithium signature".into());
        }
        if self.claimed_bls_sig.is_empty() {
            return Err("Empty claimed BLS signature".into());
        }
        if self.merkle_proof.is_empty() && self.leaf_index != 0 {
            return Err("Empty merkle proof".into());
        }
        Ok(())
    }

    pub fn verify_against_blob(
        &self,
        blob: &QcBlob,
        snapshot: &ValidatorSetSnapshot,
    ) -> Result<(), String> {
        self.validate()?;

        if self.epoch != blob.epoch {
            return Err("Fraud proof epoch mismatch".into());
        }
        if self.checkpoint_height != blob.checkpoint_height {
            return Err("Fraud proof checkpoint height mismatch".into());
        }
        if self.checkpoint_hash != blob.checkpoint_hash {
            return Err("Fraud proof checkpoint hash mismatch".into());
        }

        self.verify_inclusion(&blob.merkle_root)?;

        let entry = blob
            .pq_signatures
            .get(self.leaf_index as usize)
            .ok_or_else(|| format!("Leaf index {} out of range", self.leaf_index))?;
        if entry.validator_index != self.validator_index
            || entry.validator_address != self.validator_address
            || entry.dilithium_signature != self.dilithium_signature
        {
            return Err("Fraud proof leaf does not match blob contents".into());
        }

        let validator = snapshot
            .validators
            .get(self.validator_index as usize)
            .ok_or_else(|| format!("Unknown validator index {}", self.validator_index))?;
        if validator.address.to_string() != self.validator_address {
            return Err("Fraud proof validator address mismatch".into());
        }
        if validator.pq_public_key.is_empty() {
            return Err("Validator has no Dilithium public key".into());
        }

        let message = pq_signing_message(self.epoch, &self.checkpoint_hash, self.validator_index);
        if PqKeyPair::verify(
            &validator.pq_public_key,
            &message,
            &self.dilithium_signature,
        )
        .is_ok()
        {
            return Err("Fraud proof targets a valid Dilithium signature".into());
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

pub fn sign_attestation(
    pq_key: &PqKeyPair,
    epoch: u64,
    checkpoint_height: u64,
    checkpoint_hash: &str,
    validator_index: u32,
    validator_address: String,
) -> Result<PqSignatureEntry, String> {
    let _ = checkpoint_height;
    let message = pq_signing_message(epoch, checkpoint_hash, validator_index);
    let signature = pq_key
        .sign(&message)
        .map_err(|e| format!("Dilithium sign failed: {}", e))?;
    Ok(PqSignatureEntry {
        validator_index,
        validator_address,
        dilithium_signature: signature,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::finality::ValidatorEntry;
    use crate::core::address::Address;

    fn make_snapshot_with_pq_keys(n: usize) -> (ValidatorSetSnapshot, Vec<PqKeyPair>) {
        let mut validators = Vec::new();
        let mut keys = Vec::new();

        for i in 0..n {
            let pq_key = PqKeyPair::generate();
            let address = Address::from([(i as u8) + 1; 32]);
            validators.push(ValidatorEntry {
                address,
                stake: 1_000,
                bls_public_key: Vec::new(),
                pop_signature: Vec::new(),
                pq_public_key: pq_key.public_key_bytes().to_vec(),
            });
            keys.push(pq_key);
        }

        (ValidatorSetSnapshot::new(1, validators), keys)
    }

    fn make_signed_entries(
        snapshot: &ValidatorSetSnapshot,
        keys: &[PqKeyPair],
        checkpoint_hash: &str,
    ) -> Vec<PqSignatureEntry> {
        snapshot
            .validators
            .iter()
            .enumerate()
            .map(|(idx, validator)| {
                sign_attestation(
                    &keys[idx],
                    snapshot.epoch,
                    100,
                    checkpoint_hash,
                    idx as u32,
                    validator.address.to_string(),
                )
                .unwrap()
            })
            .collect()
    }

    #[test]
    fn test_qc_blob_creation() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(4);
        let entries = make_signed_entries(&snapshot, &keys, "cp_hash");
        let blob = QcBlob::new(1, 100, "cp_hash".into(), entries);
        assert_eq!(blob.epoch, 1);
        assert_eq!(blob.checkpoint_height, 100);
        assert!(!blob.merkle_root.is_empty());
        assert!(blob.verify_merkle_root());
    }

    #[test]
    fn test_merkle_root_deterministic() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(4);
        let entries = make_signed_entries(&snapshot, &keys, "cp_hash");
        let root1 = QcBlob::compute_merkle_root(&entries);
        let root2 = QcBlob::compute_merkle_root(&entries);
        assert_eq!(root1, root2);
    }

    #[test]
    fn test_merkle_root_changes_with_data() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(4);
        let entries1 = make_signed_entries(&snapshot, &keys, "cp_hash");
        let mut entries2 = entries1.clone();
        entries2[0].dilithium_signature[0] ^= 0xFF;
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
        let (snapshot, keys) = make_snapshot_with_pq_keys(2);
        let entries = make_signed_entries(&snapshot, &keys, "cp");
        let blob = QcBlob::new(1, 100, "cp".into(), entries);
        assert!(!blob.is_expired(5));
        assert!(!blob.is_expired(11));
        assert!(blob.is_expired(12));
    }

    #[test]
    fn test_blob_size_validation() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(4);
        let entries = make_signed_entries(&snapshot, &keys, "cp");
        let blob = QcBlob::new(1, 100, "cp".into(), entries);
        assert!(blob.validate_size().is_ok());
    }

    #[test]
    fn test_qc_blob_verify_against_snapshot() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(3);
        let entries = make_signed_entries(&snapshot, &keys, "cp");
        let blob = QcBlob::new(snapshot.epoch, 100, "cp".into(), entries);
        assert!(blob
            .verify_against_snapshot(&snapshot, Some(&[0, 1, 2]), Some(snapshot.epoch))
            .is_ok());
    }

    #[test]
    fn test_detect_fraud_proof_for_invalid_signature() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(2);
        let mut entries = make_signed_entries(&snapshot, &keys, "cp");
        entries[1].dilithium_signature[0] ^= 0xAA;
        let blob = QcBlob::new(snapshot.epoch, 100, "cp".into(), entries);
        let proofs = blob.detect_fraud_proofs(&snapshot);
        assert_eq!(proofs.len(), 1);
        assert_eq!(proofs[0].validator_index, 1);
        assert!(proofs[0].verify_against_blob(&blob, &snapshot).is_ok());
    }

    #[test]
    fn test_fraud_proof_rejects_valid_signature() {
        let (snapshot, keys) = make_snapshot_with_pq_keys(1);
        let entries = make_signed_entries(&snapshot, &keys, "cp");
        let blob = QcBlob::new(snapshot.epoch, 100, "cp".into(), entries);
        let proof = PqFraudProof::new(
            snapshot.epoch,
            100,
            "cp".into(),
            0,
            snapshot.validators[0].address.to_string(),
            vec![1; 48],
            blob.pq_signatures[0].dilithium_signature.clone(),
            blob.merkle_proof(0).unwrap(),
            0,
        );
        assert!(proof.verify_against_blob(&blob, &snapshot).is_err());
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
        let (snapshot, keys) = make_snapshot_with_pq_keys(1);
        let entries = make_signed_entries(&snapshot, &keys, "cp");
        let blob = QcBlob::new(1, 100, "cp".into(), entries);
        assert!(blob.verify_merkle_root());
    }
}
