use crate::core::address::Address;
use crate::core::hash::hash_fields_bytes;
use crate::cross_domain::message::CrossDomainMessage;
use crate::domain::types::{DomainId, Hash32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DomainEventKind {
    BridgeLocked,
    BridgeMinted,
    BridgeBurned,
    BridgeUnlocked,
    MessageEmitted,
    Custom(Vec<u8>),
}

impl DomainEventKind {
    fn as_bytes(&self) -> Vec<u8> {
        match self {
            DomainEventKind::BridgeLocked => b"bridge-locked".to_vec(),
            DomainEventKind::BridgeMinted => b"bridge-minted".to_vec(),
            DomainEventKind::BridgeBurned => b"bridge-burned".to_vec(),
            DomainEventKind::BridgeUnlocked => b"bridge-unlocked".to_vec(),
            DomainEventKind::MessageEmitted => b"message-emitted".to_vec(),
            DomainEventKind::Custom(bytes) => {
                let mut out = b"custom:".to_vec();
                out.extend_from_slice(bytes);
                out
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainEvent {
    pub domain_id: DomainId,
    pub domain_height: u64,
    pub event_index: u32,
    pub kind: DomainEventKind,
    pub emitter: Address,
    pub message: Option<CrossDomainMessage>,
    pub payload_hash: Hash32,
}

impl DomainEvent {
    pub fn leaf_hash(&self) -> Hash32 {
        let kind = self.kind.as_bytes();
        let message_id = self
            .message
            .as_ref()
            .map(|message| message.message_id)
            .unwrap_or([0u8; 32]);

        hash_fields_bytes(&[
            b"BDLM_DOMAIN_EVENT_V1",
            &self.domain_id.to_le_bytes(),
            &self.domain_height.to_le_bytes(),
            &self.event_index.to_le_bytes(),
            &kind,
            self.emitter.as_bytes(),
            &message_id,
            &self.payload_hash,
        ])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MerkleProof {
    pub leaf: Hash32,
    pub index: usize,
    pub siblings: Vec<Hash32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DomainEventTree {
    events: Vec<DomainEvent>,
}

impl DomainEventTree {
    pub fn new() -> Self {
        Self { events: Vec::new() }
    }

    pub fn push(&mut self, event: DomainEvent) {
        self.events.push(event);
    }

    pub fn events(&self) -> &[DomainEvent] {
        &self.events
    }

    pub fn root(&self) -> Hash32 {
        let leaves: Vec<Hash32> = self.events.iter().map(DomainEvent::leaf_hash).collect();
        crate::settlement::commitment_tree::merkle_root(&leaves)
    }

    pub fn proof(&self, index: usize) -> Option<MerkleProof> {
        if index >= self.events.len() {
            return None;
        }

        let mut idx = index;
        let mut level: Vec<Hash32> = self.events.iter().map(DomainEvent::leaf_hash).collect();
        let leaf = level[index];
        let mut siblings = Vec::new();

        while level.len() > 1 {
            let sibling_idx = if idx % 2 == 0 { idx + 1 } else { idx - 1 };
            let sibling = level.get(sibling_idx).copied().unwrap_or(level[idx]);
            siblings.push(sibling);

            let mut next = Vec::with_capacity((level.len() + 1) / 2);
            for pair in level.chunks(2) {
                let left = pair[0];
                let right = if pair.len() == 2 { pair[1] } else { pair[0] };
                next.push(hash_fields_bytes(&[b"BDLM_MERKLE_NODE_V1", &left, &right]));
            }
            idx /= 2;
            level = next;
        }

        Some(MerkleProof {
            leaf,
            index,
            siblings,
        })
    }
}

impl MerkleProof {
    pub fn verify(&self, expected_root: Hash32) -> bool {
        let mut hash = self.leaf;
        let mut index = self.index;

        for sibling in &self.siblings {
            hash = if index % 2 == 0 {
                hash_fields_bytes(&[b"BDLM_MERKLE_NODE_V1", &hash, sibling])
            } else {
                hash_fields_bytes(&[b"BDLM_MERKLE_NODE_V1", sibling, &hash])
            };
            index /= 2;
        }

        hash == expected_root
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cross_domain::message::{CrossDomainMessage, CrossDomainMessageParams, MessageKind};

    fn hash(label: &[u8]) -> Hash32 {
        crate::core::hash::hash_fields_bytes(&[label])
    }

    fn event(index: u32) -> DomainEvent {
        let payload_hash = hash(&[index as u8]);
        let message = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain: 1,
            target_domain: 2,
            source_height: 9,
            event_index: index,
            nonce: index as u64,
            sender: Address::from([1u8; 32]),
            recipient: Address::from([2u8; 32]),
            payload_hash,
            kind: MessageKind::BridgeLock,
            expiry_height: 50,
        });

        DomainEvent {
            domain_id: 1,
            domain_height: 9,
            event_index: index,
            kind: DomainEventKind::BridgeLocked,
            emitter: Address::from([1u8; 32]),
            message: Some(message),
            payload_hash,
        }
    }

    #[test]
    fn event_merkle_proof_verifies_and_rejects_tampering() {
        let mut tree = DomainEventTree::new();
        for index in 0..5 {
            tree.push(event(index));
        }

        let root = tree.root();
        let proof = tree.proof(3).expect("proof should exist");
        assert!(proof.verify(root));

        let mut tampered = proof.clone();
        tampered.siblings[0] = hash(b"bad sibling");
        assert!(!tampered.verify(root));

        assert!(!proof.verify(hash(b"bad root")));
    }
}
