use crate::domain::types::{DomainCommitment, DomainId, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DomainCommitmentRegistry {
    commitments: BTreeMap<(DomainId, u64, u64), DomainCommitment>,
    seen_blocks: HashSet<(DomainId, u64, Hash32)>,
}

impl DomainCommitmentRegistry {
    pub fn new() -> Self {
        Self {
            commitments: BTreeMap::new(),
            seen_blocks: HashSet::new(),
        }
    }

    pub fn insert(&mut self, commitment: DomainCommitment) -> Result<(), String> {
        let key = (
            commitment.domain_id,
            commitment.domain_height,
            commitment.sequence,
        );
        let block_key = (
            commitment.domain_id,
            commitment.domain_height,
            commitment.domain_block_hash,
        );

        if self.commitments.contains_key(&key) {
            return Err(format!(
                "Duplicate commitment for domain={}, height={}, sequence={}",
                commitment.domain_id, commitment.domain_height, commitment.sequence
            ));
        }

        if self.seen_blocks.contains(&block_key) {
            return Err(format!(
                "Domain block already committed: domain={}, height={}",
                commitment.domain_id, commitment.domain_height
            ));
        }

        self.seen_blocks.insert(block_key);
        self.commitments.insert(key, commitment);
        Ok(())
    }

    pub fn commitments_for_global_block(&self) -> Vec<DomainCommitment> {
        self.commitments.values().cloned().collect()
    }

    pub fn get(
        &self,
        domain_id: DomainId,
        domain_height: u64,
        sequence: u64,
    ) -> Option<&DomainCommitment> {
        self.commitments.get(&(domain_id, domain_height, sequence))
    }

    pub fn get_by_block_hash(
        &self,
        domain_id: DomainId,
        domain_height: u64,
        block_hash: Hash32,
    ) -> Option<&DomainCommitment> {
        self.commitments.values().find(|commitment| {
            commitment.domain_id == domain_id
                && commitment.domain_height == domain_height
                && commitment.domain_block_hash == block_hash
        })
    }

    pub fn len(&self) -> usize {
        self.commitments.len()
    }

    pub fn is_empty(&self) -> bool {
        self.commitments.is_empty()
    }

    pub fn root(&self) -> Hash32 {
        let leaves: Vec<Hash32> = self
            .commitments
            .values()
            .map(DomainCommitment::leaf_hash)
            .collect();
        crate::settlement::commitment_tree::merkle_root(&leaves)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::types::ConsensusKind;

    fn commitment(sequence: u64, block_hash: Hash32) -> DomainCommitment {
        DomainCommitment {
            domain_id: 1,
            domain_height: 10,
            domain_block_hash: block_hash,
            parent_domain_block_hash: [9u8; 32],
            state_root: [3u8; 32],
            tx_root: [4u8; 32],
            event_root: [5u8; 32],
            finality_proof_hash: [6u8; 32],
            consensus_kind: ConsensusKind::PoW,
            validator_set_hash: [7u8; 32],
            timestamp_ms: 123,
            sequence,
            producer: None,
        }
    }

    #[test]
    fn duplicate_commitment_key_is_rejected() {
        let mut registry = DomainCommitmentRegistry::new();
        registry.insert(commitment(0, [1u8; 32])).unwrap();
        assert!(registry.insert(commitment(0, [2u8; 32])).is_err());
    }

    #[test]
    fn same_domain_block_cannot_be_committed_twice() {
        let mut registry = DomainCommitmentRegistry::new();
        registry.insert(commitment(0, [1u8; 32])).unwrap();
        assert!(registry.insert(commitment(1, [1u8; 32])).is_err());
    }
}
