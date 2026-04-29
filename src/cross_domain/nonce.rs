use crate::core::address::Address;
use crate::cross_domain::message::MessageId;
use crate::domain::types::DomainId;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayNonceStore {
    outbound_nonces: BTreeMap<(DomainId, DomainId, Address), u64>,
    processed_messages: BTreeSet<MessageId>,
}

impl ReplayNonceStore {
    pub fn new() -> Self {
        Self {
            outbound_nonces: BTreeMap::new(),
            processed_messages: BTreeSet::new(),
        }
    }

    pub fn next_nonce(
        &mut self,
        source_domain: DomainId,
        target_domain: DomainId,
        sender: Address,
    ) -> u64 {
        let key = (source_domain, target_domain, sender);
        let nonce = self.outbound_nonces.get(&key).copied().unwrap_or(0);
        self.outbound_nonces.insert(key, nonce.saturating_add(1));
        nonce
    }

    pub fn mark_processed(&mut self, message_id: MessageId) -> Result<(), String> {
        if !self.processed_messages.insert(message_id) {
            return Err("Cross-domain message was already processed".into());
        }
        Ok(())
    }

    pub fn is_processed(&self, message_id: &MessageId) -> bool {
        self.processed_messages.contains(message_id)
    }

    pub fn root(&self) -> [u8; 32] {
        let mut leaves = Vec::new();

        for ((source, target, sender), nonce) in &self.outbound_nonces {
            leaves.push(crate::core::hash::hash_fields_bytes(&[
                b"BDLM_NONCE_LEAF_V1",
                &source.to_le_bytes(),
                &target.to_le_bytes(),
                sender.as_bytes(),
                &nonce.to_le_bytes(),
            ]));
        }

        for message_id in &self.processed_messages {
            leaves.push(crate::core::hash::hash_fields_bytes(&[
                b"BDLM_PROCESSED_MESSAGE_LEAF_V1",
                message_id,
            ]));
        }

        crate::settlement::commitment_tree::merkle_root(&leaves)
    }
}
