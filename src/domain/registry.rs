use crate::core::hash::hash_fields_bytes;
use crate::domain::types::{ConsensusDomain, DomainId, DomainStatus, Hash32};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConsensusDomainRegistry {
    domains: BTreeMap<DomainId, ConsensusDomain>,
}

impl ConsensusDomainRegistry {
    pub fn new() -> Self {
        Self {
            domains: BTreeMap::new(),
        }
    }

    pub fn register(&mut self, domain: ConsensusDomain) -> Result<(), String> {
        if self.domains.contains_key(&domain.id) {
            return Err(format!("Domain {} is already registered", domain.id));
        }
        self.domains.insert(domain.id, domain);
        Ok(())
    }

    pub fn get(&self, id: DomainId) -> Option<&ConsensusDomain> {
        self.domains.get(&id)
    }

    pub fn get_mut(&mut self, id: DomainId) -> Option<&mut ConsensusDomain> {
        self.domains.get_mut(&id)
    }

    pub fn set_status(&mut self, id: DomainId, status: DomainStatus) -> Result<(), String> {
        let domain = self
            .domains
            .get_mut(&id)
            .ok_or_else(|| format!("Unknown domain {}", id))?;
        domain.status = status;
        Ok(())
    }

    pub fn active_domains(&self) -> impl Iterator<Item = &ConsensusDomain> {
        self.domains
            .values()
            .filter(|domain| domain.status == DomainStatus::Active)
    }

    pub fn domains(&self) -> Vec<ConsensusDomain> {
        self.domains.values().cloned().collect()
    }

    pub fn root(&self) -> Hash32 {
        let leaves: Vec<Hash32> = self.domains.values().map(domain_leaf_hash).collect();
        crate::settlement::commitment_tree::merkle_root(&leaves)
    }
}

pub fn domain_leaf_hash(domain: &ConsensusDomain) -> Hash32 {
    let kind = domain.kind.as_bytes();
    let status = match domain.status {
        DomainStatus::Active => b"active".as_slice(),
        DomainStatus::Frozen => b"frozen".as_slice(),
        DomainStatus::Retired => b"retired".as_slice(),
    };
    let block_scheme = domain.block_hash_scheme.as_bytes();
    let state_scheme = domain.state_root_scheme.as_bytes();
    let tx_scheme = domain.tx_root_scheme.as_bytes();

    hash_fields_bytes(&[
        b"BDLM_DOMAIN_REGISTRY_LEAF_V1",
        &domain.id.to_le_bytes(),
        &kind,
        status,
        &domain.domain_chain_id.to_le_bytes(),
        &domain.config_hash,
        &domain.validator_set_hash,
        domain.finality_adapter.as_bytes(),
        &domain.min_confirmations.to_le_bytes(),
        &[domain.bridge_enabled as u8],
        &block_scheme,
        &state_scheme,
        &tx_scheme,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::plugin::default_domain;
    use crate::domain::types::ConsensusKind;

    #[test]
    fn registry_root_is_order_independent_by_domain_id() {
        let domain_a = default_domain(1, ConsensusKind::PoW, 1337, "pow", 64);
        let domain_b = default_domain(2, ConsensusKind::PoS, 1338, "pos", 0);

        let mut first = ConsensusDomainRegistry::new();
        first.register(domain_b.clone()).unwrap();
        first.register(domain_a.clone()).unwrap();

        let mut second = ConsensusDomainRegistry::new();
        second.register(domain_a).unwrap();
        second.register(domain_b).unwrap();

        assert_eq!(first.root(), second.root());
    }

    #[test]
    fn duplicate_domain_registration_is_rejected() {
        let domain = default_domain(1, ConsensusKind::PoW, 1337, "pow", 64);
        let mut registry = ConsensusDomainRegistry::new();
        registry.register(domain.clone()).unwrap();
        assert!(registry.register(domain).is_err());
    }
}
