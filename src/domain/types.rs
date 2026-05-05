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
}

impl ConsensusDomain {
    pub fn is_active(&self) -> bool {
        self.status == DomainStatus::Active
    }

    pub fn has_operator_bond(&self, minimum_bond: u64) -> bool {
        self.operator.is_some() && self.operator_bond >= minimum_bond
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DomainCommitment {
    pub domain_id: DomainId,
    pub domain_height: u64,
    pub domain_block_hash: Hash32,
    pub parent_domain_block_hash: Hash32,
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
            state_root: normalize_hash32(
                b"state_root",
                domain.id,
                &domain.state_root_scheme,
                block.state_root.as_bytes(),
            )?,
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
