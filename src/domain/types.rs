use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::hash::hash_fields_bytes;
use crate::domain::finality_adapter::FinalityProof;
use serde::{Deserialize, Serialize};

pub type DomainId = u32;
pub type Hash32 = [u8; 32];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ConsensusKind {
    PoW,
    PoS,
    PoA,
    Bft,
    Zk,
    Custom(String),
}

impl ConsensusKind {
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            ConsensusKind::PoW => b"pow".to_vec(),
            ConsensusKind::PoS => b"pos".to_vec(),
            ConsensusKind::PoA => b"poa".to_vec(),
            ConsensusKind::Bft => b"bft".to_vec(),
            ConsensusKind::Zk => b"zk".to_vec(),
            ConsensusKind::Custom(name) => {
                let mut out = b"custom:".to_vec();
                out.extend_from_slice(name.as_bytes());
                out
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DomainStatus {
    Active,
    Frozen,
    Retired,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RootScheme {
    BudlumBlockV2,
    Sha256,
    Sha3_256,
    Custom(String),
}

impl RootScheme {
    pub fn as_bytes(&self) -> Vec<u8> {
        match self {
            RootScheme::BudlumBlockV2 => b"budlum-block-v2".to_vec(),
            RootScheme::Sha256 => b"sha256".to_vec(),
            RootScheme::Sha3_256 => b"sha3-256".to_vec(),
            RootScheme::Custom(name) => {
                let mut out = b"custom:".to_vec();
                out.extend_from_slice(name.as_bytes());
                out
            }
        }
    }
}

fn default_domain_operator() -> Option<Address> {
    Some(Address::zero())
}

fn default_domain_operator_bond() -> u64 {
    crate::domain::registry::MIN_DOMAIN_OPERATOR_BOND
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConsensusDomain {
    pub id: DomainId,
    pub kind: ConsensusKind,
    pub status: DomainStatus,
    pub domain_chain_id: u64,
    #[serde(default = "default_domain_operator")]
    pub operator: Option<Address>,
    #[serde(default = "default_domain_operator_bond")]
    pub operator_bond: u64,
    pub config_hash: Hash32,
    pub validator_set_hash: Hash32,
    pub finality_adapter: String,
    pub min_confirmations: u64,
    pub bridge_enabled: bool,
    pub block_hash_scheme: RootScheme,
    pub state_root_scheme: RootScheme,
    pub tx_root_scheme: RootScheme,
    pub last_committed_height: u64,
    pub last_committed_hash: Hash32,
    /// How far this domain's `state_updates` have actually been applied to
    /// the global account state. Distinct from `last_committed_height`
    /// (which only tracks that a commitment was recorded and sequence/
    /// equivocation-checked): settlement is deferred to `settle_pending_domain_commitments`
    /// so that cross-domain conflicts are resolved in a fixed, domain-id order
    /// rather than by network arrival order.
    #[serde(default)]
    pub last_settled_height: u64,
    /// For PoW domains: the maximum allowed header hash (big-endian, smaller
    /// = harder). Real proof-of-work headers submitted as finality proof must
    /// each independently hash below this floor, and below their own claimed
    /// target — the operator is responsible for setting this to reflect the
    /// real difficulty of the domain's chain. Defaults to the maximum
    /// (`[0xFF; 32]`), which accepts any nonce and provides no real security;
    /// operators of a genuine PoW domain must lower it.
    #[serde(default = "default_min_pow_target")]
    pub min_pow_target: Hash32,
}

fn default_min_pow_target() -> Hash32 {
    [0xFFu8; 32]
}

impl ConsensusDomain {
    pub fn is_active(&self) -> bool {
        self.status == DomainStatus::Active
    }

    pub fn has_operator_bond(&self, minimum_bond: u64) -> bool {
        self.operator.is_some() && self.operator_bond >= minimum_bond
    }
}

/// Canonical leaf encoding for a single cross-domain state update, used to
/// build/verify `DomainCommitment::state_root` as a Merkle root over
/// `state_updates` (see `compute_state_updates_root`).
pub fn state_update_leaf_hash(address: &Address, nonce: u64) -> Hash32 {
    hash_fields_bytes(&[
        b"BDLM_STATE_UPDATE_V1",
        address.as_bytes(),
        &nonce.to_le_bytes(),
    ])
}

/// Computes the Merkle root that `DomainCommitment::state_root` must equal
/// for its `state_updates` to be considered authentic. Because the
/// commitment's `domain_block_hash`/`finality_proof_hash` are already
/// verified against the domain's real consensus (PoW/PoS/BFT), and
/// `state_root` is part of the commitment's own hash, requiring this
/// equality means an attacker cannot attach arbitrary `state_updates` to a
/// legitimately finalized commitment — any change to the update set changes
/// the required `state_root`, which is covered by the finality proof.
pub fn compute_state_updates_root(
    state_updates: &std::collections::BTreeMap<Address, u64>,
) -> Hash32 {
    let leaves: Vec<Hash32> = state_updates
        .iter()
        .map(|(addr, nonce)| state_update_leaf_hash(addr, *nonce))
        .collect();
    crate::settlement::commitment_tree::merkle_root(&leaves)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainCommitment {
    pub domain_id: DomainId,
    pub domain_height: u64,
    pub domain_block_hash: Hash32,
    pub parent_domain_block_hash: Hash32,
    /// Merkle root over `state_updates` (see `compute_state_updates_root`) —
    /// NOT the domain's raw internal state root. Verified at acceptance time
    /// so `state_updates` can't be tampered with independently of the
    /// already-finality-proven commitment. Use `insert_state_update` to keep
    /// this in sync instead of mutating `state_updates` directly.
    pub state_root: Hash32,
    pub tx_root: Hash32,
    pub event_root: Hash32,
    pub finality_proof_hash: Hash32,
    pub consensus_kind: ConsensusKind,
    pub validator_set_hash: Hash32,
    pub timestamp_ms: u128,
    pub sequence: u64,
    pub producer: Option<Address>,
    pub state_updates: std::collections::BTreeMap<Address, u64>,
}

impl DomainCommitment {
    pub fn from_block(
        domain: &ConsensusDomain,
        block: &Block,
        event_root: Hash32,
        finality_proof_hash: Hash32,
        sequence: u64,
    ) -> Result<Self, String> {
        Ok(Self {
            domain_id: domain.id,
            domain_height: block.index,
            domain_block_hash: normalize_hash32(
                b"domain_block_hash",
                domain.id,
                &domain.block_hash_scheme,
                block.hash.as_bytes(),
            )?,
            parent_domain_block_hash: normalize_hash32(
                b"parent_domain_block_hash",
                domain.id,
                &domain.block_hash_scheme,
                block.previous_hash.as_bytes(),
            )?,
            // No state updates yet — insert_state_update() keeps this in
            // sync as updates are added.
            state_root: compute_state_updates_root(&std::collections::BTreeMap::new()),
            tx_root: normalize_hash32(
                b"tx_root",
                domain.id,
                &domain.tx_root_scheme,
                block.tx_root.as_bytes(),
            )?,
            event_root,
            finality_proof_hash,
            consensus_kind: domain.kind.clone(),
            validator_set_hash: domain.validator_set_hash,
            timestamp_ms: block.timestamp,
            sequence,
            producer: block.producer,
            state_updates: std::collections::BTreeMap::new(),
        })
    }

    /// Inserts (or updates) a single account's state update and keeps
    /// `state_root` in sync. Prefer this over mutating `state_updates`
    /// directly — a commitment whose `state_root` doesn't match
    /// `compute_state_updates_root(&state_updates)` is rejected at
    /// acceptance time.
    pub fn insert_state_update(&mut self, address: Address, new_nonce: u64) {
        self.state_updates.insert(address, new_nonce);
        self.state_root = compute_state_updates_root(&self.state_updates);
    }

    /// The payload finality proofs actually attest to: domain identity,
    /// position, and every root this commitment claims — crucially
    /// including `state_root`, not just `domain_block_hash`. Without this,
    /// a valid (commitment, proof) pair for one `state_updates` batch could
    /// be replayed against a different, still self-consistent
    /// `state_updates`/`state_root` pair, since a proof binding only
    /// `domain_block_hash` never actually vouches for the state transition.
    ///
    /// Deliberately excludes `finality_proof_hash`: that field is only
    /// known *after* a proof exists (it hashes the proof itself), so it
    /// cannot be part of what the proof commits to without becoming
    /// circular. Also excludes `producer`/`timestamp_ms`, which carry no
    /// security-relevant claim about domain state.
    pub fn commitment_payload_hash(&self) -> Hash32 {
        // Deliberately excludes `sequence` (a caller-assigned submission
        // counter, not domain state — the same real commitment can be
        // legitimately resubmitted under a different sequence number and
        // must still be recognized as identical) as well as
        // `finality_proof_hash`/`producer`/`timestamp_ms` (see doc comment).
        hash_fields_bytes(&[
            b"BDLM_DOMAIN_COMMITMENT_PAYLOAD_V1",
            &self.domain_id.to_le_bytes(),
            &self.domain_height.to_le_bytes(),
            &self.domain_block_hash,
            &self.parent_domain_block_hash,
            &self.state_root,
            &self.tx_root,
            &self.event_root,
            &self.consensus_kind.as_bytes(),
            &self.validator_set_hash,
        ])
    }

    pub fn leaf_hash(&self) -> Hash32 {
        let kind = self.consensus_kind.as_bytes();
        let producer = self
            .producer
            .map(|address| address.as_bytes().to_vec())
            .unwrap_or_default();

        let mut state_updates_bytes = Vec::new();
        for (addr, nonce) in &self.state_updates {
            state_updates_bytes.extend_from_slice(addr.as_bytes());
            state_updates_bytes.extend_from_slice(&nonce.to_le_bytes());
        }

        hash_fields_bytes(&[
            b"BDLM_DOMAIN_COMMITMENT_V1",
            &self.domain_id.to_le_bytes(),
            &self.domain_height.to_le_bytes(),
            &self.domain_block_hash,
            &self.parent_domain_block_hash,
            &self.state_root,
            &self.tx_root,
            &self.event_root,
            &self.finality_proof_hash,
            &kind,
            &self.validator_set_hash,
            &self.timestamp_ms.to_le_bytes(),
            &self.sequence.to_le_bytes(),
            &producer,
            &state_updates_bytes,
        ])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedDomainCommitment {
    pub commitment: DomainCommitment,
    pub proof: FinalityProof,
}

impl VerifiedDomainCommitment {
    pub fn leaf_hash(&self) -> Hash32 {
        self.commitment.leaf_hash()
    }
}

pub fn normalize_hash32(
    tag: &[u8],
    domain_id: DomainId,
    scheme: &RootScheme,
    raw: &[u8],
) -> Result<Hash32, String> {
    if let Ok(decoded) = hex::decode(raw) {
        if decoded.len() == 32 {
            let mut out = [0u8; 32];
            out.copy_from_slice(&decoded);
            return Ok(out);
        }
    }

    if raw.len() == 32 {
        let mut out = [0u8; 32];
        out.copy_from_slice(raw);
        return Ok(out);
    }

    let scheme_bytes = scheme.as_bytes();
    Ok(hash_fields_bytes(&[
        b"BDLM_NORMALIZED_ROOT_V1",
        tag,
        &domain_id.to_le_bytes(),
        &scheme_bytes,
        raw,
    ]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_hash32_accepts_hex_and_hashes_non_32_byte_input() {
        let hex_root = "11".repeat(32);
        let normalized =
            normalize_hash32(b"state", 1, &RootScheme::BudlumBlockV2, hex_root.as_bytes()).unwrap();
        assert_eq!(normalized, [0x11u8; 32]);

        let custom = normalize_hash32(
            b"state",
            1,
            &RootScheme::Custom("foreign".into()),
            b"short-root",
        )
        .unwrap();
        assert_ne!(custom, [0u8; 32]);
        assert_ne!(custom, normalized);
    }
}
