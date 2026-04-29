use crate::core::address::Address;
use crate::core::hash::hash_fields_bytes;
use crate::domain::types::{DomainId, Hash32};
use serde::{Deserialize, Serialize};

pub type MessageId = Hash32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MessageKind {
    BridgeLock,
    BridgeMint,
    BridgeBurn,
    BridgeUnlock,
    Custom(Vec<u8>),
}

impl MessageKind {
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            MessageKind::BridgeLock => b"bridge-lock".to_vec(),
            MessageKind::BridgeMint => b"bridge-mint".to_vec(),
            MessageKind::BridgeBurn => b"bridge-burn".to_vec(),
            MessageKind::BridgeUnlock => b"bridge-unlock".to_vec(),
            MessageKind::Custom(bytes) => {
                let mut out = b"custom:".to_vec();
                out.extend_from_slice(bytes);
                out
            }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CrossDomainMessage {
    pub message_id: MessageId,
    pub source_domain: DomainId,
    pub target_domain: DomainId,
    pub source_height: u64,
    pub event_index: u32,
    pub nonce: u64,
    pub sender: Address,
    pub recipient: Address,
    pub payload_hash: Hash32,
    pub kind: MessageKind,
    pub expiry_height: u64,
}

impl CrossDomainMessage {
    pub fn new(params: CrossDomainMessageParams) -> Self {
        let mut message = CrossDomainMessage {
            message_id: [0u8; 32],
            source_domain: params.source_domain,
            target_domain: params.target_domain,
            source_height: params.source_height,
            event_index: params.event_index,
            nonce: params.nonce,
            sender: params.sender,
            recipient: params.recipient,
            payload_hash: params.payload_hash,
            kind: params.kind,
            expiry_height: params.expiry_height,
        };
        message.message_id = message.calculate_message_id();
        message
    }

    pub fn calculate_message_id(&self) -> MessageId {
        let kind = self.kind.as_bytes();
        hash_fields_bytes(&[
            b"BDLM_CROSS_DOMAIN_MESSAGE_V1",
            &self.source_domain.to_le_bytes(),
            &self.target_domain.to_le_bytes(),
            &self.source_height.to_le_bytes(),
            &self.event_index.to_le_bytes(),
            &self.nonce.to_le_bytes(),
            self.sender.as_bytes(),
            self.recipient.as_bytes(),
            &self.payload_hash,
            &kind,
            &self.expiry_height.to_le_bytes(),
        ])
    }

    pub fn verify_id(&self) -> bool {
        self.message_id == self.calculate_message_id()
    }
}

#[derive(Debug, Clone)]
pub struct CrossDomainMessageParams {
    pub source_domain: DomainId,
    pub target_domain: DomainId,
    pub source_height: u64,
    pub event_index: u32,
    pub nonce: u64,
    pub sender: Address,
    pub recipient: Address,
    pub payload_hash: Hash32,
    pub kind: MessageKind,
    pub expiry_height: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(label: &[u8]) -> Hash32 {
        crate::core::hash::hash_fields_bytes(&[label])
    }

    #[test]
    fn message_id_is_deterministic_and_tamper_evident() {
        let params = CrossDomainMessageParams {
            source_domain: 1,
            target_domain: 2,
            source_height: 42,
            event_index: 7,
            nonce: 3,
            sender: Address::from([1u8; 32]),
            recipient: Address::from([2u8; 32]),
            payload_hash: hash(b"payload"),
            kind: MessageKind::BridgeLock,
            expiry_height: 100,
        };

        let message_a = CrossDomainMessage::new(params.clone());
        let message_b = CrossDomainMessage::new(params);
        assert_eq!(message_a.message_id, message_b.message_id);
        assert!(message_a.verify_id());

        let mut tampered = message_a.clone();
        tampered.nonce += 1;
        assert!(!tampered.verify_id());
    }
}
