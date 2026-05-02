pub mod commitment_registry;
pub mod finality_adapter;
pub mod plugin;
pub mod plugin_registry;
pub mod registry;
pub mod types;

pub use commitment_registry::DomainCommitmentRegistry;
pub use finality_adapter::{
    hash_finality_proof, BftFinalityAdapter, DomainFinalityAdapter, FinalityError, FinalityProof,
    FinalityStatus, PoAFinalityAdapter, PoSFinalityAdapter, PoWFinalityAdapter, ZkFinalityAdapter,
};
pub use plugin::{
    default_domain, BftDomainPlugin, ConsensusDomainPlugin, DomainContext, DomainError,
    PoADomainPlugin, PoSDomainPlugin, PoWDomainPlugin, ZkDomainPlugin,
};
pub use plugin_registry::DomainPluginRegistry;
pub use registry::ConsensusDomainRegistry;
pub use types::{
    normalize_hash32, ConsensusDomain, ConsensusKind, DomainCommitment, DomainId, DomainStatus,
    Hash32, RootScheme, VerifiedDomainCommitment,
};
