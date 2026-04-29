use crate::consensus::{ConsensusEngine, ConsensusError};
use crate::core::account::AccountState;
use crate::core::block::Block;
use crate::core::hash::hash_fields_bytes;
use crate::domain::finality_adapter::{
    block_finality_proof_hash, empty_event_root, BftFinalityAdapter, DomainFinalityAdapter,
    PoAFinalityAdapter, PoSFinalityAdapter, PoWFinalityAdapter, ZkFinalityAdapter,
};
use crate::domain::types::{
    ConsensusDomain, ConsensusKind, DomainCommitment, DomainId, DomainStatus, Hash32, RootScheme,
};
use std::sync::Arc;

#[derive(Clone)]
pub struct DomainContext<'a> {
    pub domain: &'a ConsensusDomain,
    pub chain: &'a [Block],
    pub state: &'a AccountState,
    pub sequence: u64,
}

#[derive(Debug, Clone)]
pub struct DomainError(pub String);

impl std::fmt::Display for DomainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Domain error: {}", self.0)
    }
}

impl std::error::Error for DomainError {}

impl From<ConsensusError> for DomainError {
    fn from(value: ConsensusError) -> Self {
        DomainError(value.0)
    }
}

pub trait ConsensusDomainPlugin: Send + Sync {
    fn kind(&self) -> ConsensusKind;
    fn consensus(&self) -> &dyn ConsensusEngine;
    fn finality_adapter(&self) -> &dyn DomainFinalityAdapter;

    fn validate_domain_block(
        &self,
        block: &Block,
        ctx: DomainContext<'_>,
    ) -> Result<(), DomainError> {
        self.consensus()
            .full_validate(block, ctx.chain, ctx.state)
            .map_err(DomainError::from)
    }

    fn extract_commitment(
        &self,
        block: &Block,
        ctx: DomainContext<'_>,
    ) -> Result<DomainCommitment, DomainError> {
        if !ctx.domain.is_active() {
            return Err(DomainError(format!(
                "Domain {} is not active",
                ctx.domain.id
            )));
        }

        DomainCommitment::from_block(
            ctx.domain,
            block,
            empty_event_root(),
            block_finality_proof_hash(block),
            ctx.sequence,
        )
        .map_err(DomainError)
    }
}

pub struct PoWDomainPlugin {
    consensus: Arc<dyn ConsensusEngine>,
    finality: PoWFinalityAdapter,
}

impl PoWDomainPlugin {
    pub fn new(consensus: Arc<dyn ConsensusEngine>) -> Self {
        Self {
            consensus,
            finality: PoWFinalityAdapter::default(),
        }
    }
}

impl ConsensusDomainPlugin for PoWDomainPlugin {
    fn kind(&self) -> ConsensusKind {
        ConsensusKind::PoW
    }

    fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }

    fn finality_adapter(&self) -> &dyn DomainFinalityAdapter {
        &self.finality
    }
}

pub struct PoSDomainPlugin {
    consensus: Arc<dyn ConsensusEngine>,
    finality: PoSFinalityAdapter,
}

impl PoSDomainPlugin {
    pub fn new(consensus: Arc<dyn ConsensusEngine>) -> Self {
        Self {
            consensus,
            finality: PoSFinalityAdapter,
        }
    }
}

impl ConsensusDomainPlugin for PoSDomainPlugin {
    fn kind(&self) -> ConsensusKind {
        ConsensusKind::PoS
    }

    fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }

    fn finality_adapter(&self) -> &dyn DomainFinalityAdapter {
        &self.finality
    }
}

pub struct PoADomainPlugin {
    consensus: Arc<dyn ConsensusEngine>,
    finality: PoAFinalityAdapter,
}

impl PoADomainPlugin {
    pub fn new(consensus: Arc<dyn ConsensusEngine>) -> Self {
        Self {
            consensus,
            finality: PoAFinalityAdapter::default(),
        }
    }
}

impl ConsensusDomainPlugin for PoADomainPlugin {
    fn kind(&self) -> ConsensusKind {
        ConsensusKind::PoA
    }

    fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }

    fn finality_adapter(&self) -> &dyn DomainFinalityAdapter {
        &self.finality
    }
}

pub struct BftDomainPlugin {
    consensus: Arc<dyn ConsensusEngine>,
    finality: BftFinalityAdapter,
}

impl BftDomainPlugin {
    pub fn new(consensus: Arc<dyn ConsensusEngine>) -> Self {
        Self {
            consensus,
            finality: BftFinalityAdapter::default(),
        }
    }
}

impl ConsensusDomainPlugin for BftDomainPlugin {
    fn kind(&self) -> ConsensusKind {
        ConsensusKind::Bft
    }

    fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }

    fn finality_adapter(&self) -> &dyn DomainFinalityAdapter {
        &self.finality
    }
}

pub struct ZkDomainPlugin {
    consensus: Arc<dyn ConsensusEngine>,
    finality: ZkFinalityAdapter,
}

impl ZkDomainPlugin {
    pub fn new(consensus: Arc<dyn ConsensusEngine>) -> Self {
        Self {
            consensus,
            finality: ZkFinalityAdapter,
        }
    }
}

impl ConsensusDomainPlugin for ZkDomainPlugin {
    fn kind(&self) -> ConsensusKind {
        ConsensusKind::Zk
    }

    fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }

    fn finality_adapter(&self) -> &dyn DomainFinalityAdapter {
        &self.finality
    }
}

pub fn default_domain(
    id: DomainId,
    kind: ConsensusKind,
    domain_chain_id: u64,
    finality_adapter: impl Into<String>,
    min_confirmations: u64,
) -> ConsensusDomain {
    let kind_bytes = kind.as_bytes();
    let config_hash: Hash32 = hash_fields_bytes(&[
        b"BDLM_DOMAIN_CONFIG_V1",
        &id.to_le_bytes(),
        &domain_chain_id.to_le_bytes(),
        &kind_bytes,
    ]);

    ConsensusDomain {
        id,
        kind,
        status: DomainStatus::Active,
        domain_chain_id,
        config_hash,
        validator_set_hash: [0u8; 32],
        finality_adapter: finality_adapter.into(),
        min_confirmations,
        bridge_enabled: false,
        block_hash_scheme: RootScheme::BudlumBlockV2,
        state_root_scheme: RootScheme::BudlumBlockV2,
        tx_root_scheme: RootScheme::BudlumBlockV2,
    }
}

