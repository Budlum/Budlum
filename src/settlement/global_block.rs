use crate::core::address::Address;
use crate::core::hash::hash_fields_bytes;
use crate::domain::types::Hash32;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GlobalBlockHeader {
    pub version: u16,
    pub global_height: u64,
    pub previous_global_hash: Hash32,
    pub chain_id: u64,
    pub timestamp_ms: u128,
    pub domain_registry_root: Hash32,
    pub domain_commitment_root: Hash32,
    pub message_root: Hash32,
    pub bridge_state_root: Hash32,
    pub replay_nonce_root: Hash32,
    pub proposer: Option<Address>,
    pub settlement_finality_root: Hash32,
    /// Root of Budlum's own global account state (balances/nonces) at the
    /// referenced height — see `AccountState::calculate_state_root`. Without
    /// this, two nodes could seal identical-looking global headers while
    /// their underlying account state genuinely diverged.
    pub global_state_root: Hash32,
    /// True if `global_state_root` was taken from a block height already
    /// covered by a BLS finality certificate on Budlum's own chain (i.e. a
    /// real quorum of Budlum's validators agreed on it). False means it was
    /// taken from the current, not-yet-finalized chain tip — which happens
    /// on chains with no active BLS validator committee (e.g. a plain PoW
    /// devnet) — and callers must not treat the header as consensus-final.
    pub global_state_finalized: bool,
}

impl GlobalBlockHeader {
    pub fn calculate_hash_bytes(&self) -> Hash32 {
        let proposer = self
            .proposer
            .map(|address| address.as_bytes().to_vec())
            .unwrap_or_default();

        hash_fields_bytes(&[
            b"BDLM_GLOBAL_BLOCK_V1",
            &self.version.to_le_bytes(),
            &self.global_height.to_le_bytes(),
            &self.previous_global_hash,
            &self.chain_id.to_le_bytes(),
            &self.timestamp_ms.to_le_bytes(),
            &self.domain_registry_root,
            &self.domain_commitment_root,
            &self.message_root,
            &self.bridge_state_root,
            &self.replay_nonce_root,
            &proposer,
            &self.settlement_finality_root,
            &self.global_state_root,
            &[self.global_state_finalized as u8],
        ])
    }

    pub fn calculate_hash(&self) -> String {
        hex::encode(self.calculate_hash_bytes())
    }
}
