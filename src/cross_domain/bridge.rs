use crate::core::address::Address;
use crate::core::hash::hash_fields_bytes;
use crate::cross_domain::event_tree::{DomainEvent, DomainEventKind};
use crate::cross_domain::message::{
    CrossDomainMessage, CrossDomainMessageParams, MessageId, MessageKind,
};
use crate::cross_domain::nonce::ReplayNonceStore;
use crate::domain::types::{DomainId, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub type AssetId = Hash32;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BridgeStatus {
    Active { domain: DomainId },
    Locked { domain: DomainId },
    Minted { domain: DomainId },
    Burned { domain: DomainId },
    Unlocked { domain: DomainId },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BridgeTransfer {
    pub message_id: MessageId,
    pub asset_id: AssetId,
    pub source_domain: DomainId,
    pub target_domain: DomainId,
    pub owner: Address,
    pub recipient: Address,
    pub amount: u128,
    pub status: BridgeStatus,
    pub source_event_hash: Hash32,
}

#[derive(Debug, Clone)]
pub struct BridgeError(pub String);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Bridge error: {}", self.0)
    }
}

impl std::error::Error for BridgeError {}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BridgeState {
    asset_locations: BTreeMap<AssetId, BridgeStatus>,
    transfers: BTreeMap<MessageId, BridgeTransfer>,
    replay: ReplayNonceStore,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            asset_locations: BTreeMap::new(),
            transfers: BTreeMap::new(),
            replay: ReplayNonceStore::new(),
        }
    }

    pub fn register_asset(
        &mut self,
        asset_id: AssetId,
        domain: DomainId,
    ) -> Result<(), BridgeError> {
        if self.asset_locations.contains_key(&asset_id) {
            return Err(BridgeError("Asset is already registered".into()));
        }
        self.asset_locations
            .insert(asset_id, BridgeStatus::Active { domain });
        Ok(())
    }

    pub fn lock(
        &mut self,
        source_domain: DomainId,
        target_domain: DomainId,
        source_height: u64,
        event_index: u32,
        asset_id: AssetId,
        owner: Address,
        recipient: Address,
        amount: u128,
        expiry_height: u64,
    ) -> Result<(BridgeTransfer, DomainEvent), BridgeError> {
        self.require_asset_status(
            asset_id,
            BridgeStatus::Active {
                domain: source_domain,
            },
        )?;
        let nonce = self.replay.next_nonce(source_domain, target_domain, owner);
        let payload_hash = bridge_payload_hash(asset_id, amount);
        let message = CrossDomainMessage::new(CrossDomainMessageParams {
            source_domain,
            target_domain,
            source_height,
            event_index,
            nonce,
            sender: owner,
            recipient,
            payload_hash,
            kind: MessageKind::BridgeLock,
            expiry_height,
        });
        let event = DomainEvent {
            domain_id: source_domain,
            domain_height: source_height,
            event_index,
            kind: DomainEventKind::BridgeLocked,
            emitter: owner,
            message: Some(message.clone()),
            payload_hash,
        };
        let transfer = BridgeTransfer {
            message_id: message.message_id,
            asset_id,
            source_domain,
            target_domain,
            owner,
            recipient,
            amount,
            status: BridgeStatus::Locked {
                domain: source_domain,
            },
            source_event_hash: event.leaf_hash(),
        };

        self.asset_locations.insert(
            asset_id,
            BridgeStatus::Locked {
                domain: source_domain,
            },
        );
        self.transfers.insert(transfer.message_id, transfer.clone());
        Ok((transfer, event))
    }

    pub fn mint(&mut self, message: &CrossDomainMessage) -> Result<(), BridgeError> {
        if !message.verify_id() {
            return Err(BridgeError("Invalid cross-domain message id".into()));
        }
        let transfer = self
            .transfers
            .get(&message.message_id)
            .ok_or_else(|| BridgeError("Unknown bridge transfer".into()))?;
        if self.replay.is_processed(&message.message_id) {
            return Err(BridgeError(
                "Cross-domain message was already processed".into(),
            ));
        }
        if transfer.status
            != (BridgeStatus::Locked {
                domain: message.source_domain,
            })
        {
            return Err(BridgeError(
                "Transfer is not locked on source domain".into(),
            ));
        }
        self.replay
            .mark_processed(message.message_id)
            .map_err(BridgeError)?;

        let transfer = self
            .transfers
            .get_mut(&message.message_id)
            .ok_or_else(|| BridgeError("Unknown bridge transfer".into()))?;

        transfer.status = BridgeStatus::Minted {
            domain: message.target_domain,
        };
        self.asset_locations.insert(
            transfer.asset_id,
            BridgeStatus::Minted {
                domain: message.target_domain,
            },
        );
        Ok(())
    }

    pub fn burn(&mut self, message_id: MessageId, domain: DomainId) -> Result<(), BridgeError> {
        self.burn_with_event(message_id, domain, 0, 0, 0)
            .map(|_| ())
    }

    pub fn burn_with_event(
        &mut self,
        message_id: MessageId,
        domain: DomainId,
        domain_height: u64,
        event_index: u32,
        expiry_height: u64,
    ) -> Result<DomainEvent, BridgeError> {
        let transfer = self
            .transfers
            .get(&message_id)
            .ok_or_else(|| BridgeError("Unknown bridge transfer".into()))?;
        if transfer.status != (BridgeStatus::Minted { domain }) {
            return Err(BridgeError("Transfer is not minted on burn domain".into()));
        }
        let asset_id = transfer.asset_id;
        let amount = transfer.amount;
        let source_domain = transfer.source_domain;
        let owner = transfer.owner;
        let recipient = transfer.recipient;

        let nonce = self.replay.next_nonce(domain, source_domain, recipient);
        let payload_hash = bridge_payload_hash(asset_id, amount);
        let message = CrossDomainMessage::new_correlated(
            CrossDomainMessageParams {
                source_domain: domain,
                target_domain: source_domain,
                source_height: domain_height,
                event_index,
                nonce,
                sender: recipient,
                recipient: owner,
                payload_hash,
                kind: MessageKind::BridgeBurn,
                expiry_height,
            },
            message_id,
        );
        let event = DomainEvent {
            domain_id: domain,
            domain_height,
            event_index,
            kind: DomainEventKind::BridgeBurned,
            emitter: recipient,
            message: Some(message),
            payload_hash,
        };

        let transfer = self
            .transfers
            .get_mut(&message_id)
            .ok_or_else(|| BridgeError("Unknown bridge transfer".into()))?;
        transfer.status = BridgeStatus::Burned { domain };
        self.asset_locations
            .insert(transfer.asset_id, BridgeStatus::Burned { domain });
        Ok(event)
    }

    pub fn unlock(
        &mut self,
        message_id: MessageId,
        source_domain: DomainId,
    ) -> Result<(), BridgeError> {
        let transfer = self
            .transfers
            .get_mut(&message_id)
            .ok_or_else(|| BridgeError("Unknown bridge transfer".into()))?;
        if transfer.status
            != (BridgeStatus::Burned {
                domain: transfer.target_domain,
            })
        {
            return Err(BridgeError(
                "Transfer is not burned on target domain".into(),
            ));
        }
        if transfer.source_domain != source_domain {
            return Err(BridgeError("Unlock source domain mismatch".into()));
        }
        transfer.status = BridgeStatus::Unlocked {
            domain: source_domain,
        };
        self.asset_locations.insert(
            transfer.asset_id,
            BridgeStatus::Active {
                domain: source_domain,
            },
        );
        Ok(())
    }

    pub fn root(&self) -> Hash32 {
        let leaves: Vec<Hash32> = self
            .asset_locations
            .iter()
            .map(|(asset_id, status)| {
                let status = status_bytes(status);
                hash_fields_bytes(&[b"BDLM_BRIDGE_ASSET_LEAF_V1", asset_id, &status])
            })
            .collect();
        crate::settlement::commitment_tree::merkle_root(&leaves)
    }

    pub fn replay_root(&self) -> Hash32 {
        self.replay.root()
    }

    pub fn source_event_hash(&self, message_id: &MessageId) -> Option<Hash32> {
        self.transfers
            .get(message_id)
            .map(|transfer| transfer.source_event_hash)
    }

    pub fn transfer(&self, message_id: &MessageId) -> Option<&BridgeTransfer> {
        self.transfers.get(message_id)
    }

    fn require_asset_status(
        &self,
        asset_id: AssetId,
        expected: BridgeStatus,
    ) -> Result<(), BridgeError> {
        let current = self
            .asset_locations
            .get(&asset_id)
            .ok_or_else(|| BridgeError("Unknown asset".into()))?;
        if current != &expected {
            return Err(BridgeError(
                "Asset is not active in the source domain".into(),
            ));
        }
        Ok(())
    }
}

pub fn bridge_payload_hash(asset_id: AssetId, amount: u128) -> Hash32 {
    hash_fields_bytes(&[b"BDLM_BRIDGE_PAYLOAD_V1", &asset_id, &amount.to_le_bytes()])
}

fn status_bytes(status: &BridgeStatus) -> Vec<u8> {
    match status {
        BridgeStatus::Active { domain } => status_with_domain(b"active", *domain),
        BridgeStatus::Locked { domain } => status_with_domain(b"locked", *domain),
        BridgeStatus::Minted { domain } => status_with_domain(b"minted", *domain),
        BridgeStatus::Burned { domain } => status_with_domain(b"burned", *domain),
        BridgeStatus::Unlocked { domain } => status_with_domain(b"unlocked", *domain),
    }
}

fn status_with_domain(tag: &[u8], domain: DomainId) -> Vec<u8> {
    let mut out = tag.to_vec();
    out.extend_from_slice(&domain.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_prevents_replay_mint() {
        let mut bridge = BridgeState::new();
        let asset = hash_fields_bytes(&[b"asset"]);
        let owner = Address::zero();
        let recipient = Address::zero();
        bridge.register_asset(asset, 1).unwrap();

        let (_transfer, event) = bridge
            .lock(1, 2, 10, 0, asset, owner, recipient, 100, 1000)
            .unwrap();
        let message = event.message.unwrap();

        bridge.mint(&message).unwrap();
        assert!(bridge.mint(&message).is_err());
    }

    #[test]
    fn bridge_rejects_double_lock_and_out_of_order_transitions() {
        let mut bridge = BridgeState::new();
        let asset = hash_fields_bytes(&[b"asset"]);
        let owner = Address::from([1u8; 32]);
        let recipient = Address::from([2u8; 32]);
        bridge.register_asset(asset, 1).unwrap();

        let (transfer, event) = bridge
            .lock(1, 2, 10, 0, asset, owner, recipient, 100, 1000)
            .unwrap();

        assert!(bridge
            .lock(1, 2, 11, 0, asset, owner, recipient, 100, 1000)
            .is_err());
        assert!(bridge.burn(transfer.message_id, 2).is_err());
        assert!(bridge.unlock(transfer.message_id, 1).is_err());

        let message = event.message.unwrap();
        bridge.mint(&message).unwrap();
        assert!(bridge.unlock(transfer.message_id, 1).is_err());
        bridge.burn(transfer.message_id, 2).unwrap();
        assert!(bridge.unlock(transfer.message_id, 9).is_err());
        bridge.unlock(transfer.message_id, 1).unwrap();
    }
}
