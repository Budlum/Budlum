use crate::domain::plugin::ConsensusDomainPlugin;
use crate::domain::types::DomainId;
use std::collections::BTreeMap;
use std::sync::Arc;

pub struct DomainPluginRegistry {
    plugins: BTreeMap<DomainId, Arc<dyn ConsensusDomainPlugin>>,
}

impl DomainPluginRegistry {
    pub fn new() -> Self {
        Self {
            plugins: BTreeMap::new(),
        }
    }

    pub fn register(
        &mut self,
        domain_id: DomainId,
        plugin: Arc<dyn ConsensusDomainPlugin>,
    ) -> Result<(), String> {
        if self.plugins.contains_key(&domain_id) {
            return Err(format!(
                "Plugin already registered for domain {}",
                domain_id
            ));
        }
        self.plugins.insert(domain_id, plugin);
        Ok(())
    }

    pub fn get(&self, domain_id: DomainId) -> Option<&Arc<dyn ConsensusDomainPlugin>> {
        self.plugins.get(&domain_id)
    }

    pub fn remove(&mut self, domain_id: DomainId) -> Option<Arc<dyn ConsensusDomainPlugin>> {
        self.plugins.remove(&domain_id)
    }

    pub fn domain_ids(&self) -> Vec<DomainId> {
        self.plugins.keys().copied().collect()
    }

    pub fn len(&self) -> usize {
        self.plugins.len()
    }

    pub fn is_empty(&self) -> bool {
        self.plugins.is_empty()
    }
}

impl Default for DomainPluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::pow::PoWEngine;
    use crate::domain::plugin::PoWDomainPlugin;

    #[test]
    fn register_and_retrieve_plugin() {
        let mut registry = DomainPluginRegistry::new();
        let engine = Arc::new(PoWEngine::new(1));
        let plugin = Arc::new(PoWDomainPlugin::new(engine));
        registry.register(1, plugin).unwrap();
        assert!(registry.get(1).is_some());
        assert!(registry.get(2).is_none());
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut registry = DomainPluginRegistry::new();
        let engine = Arc::new(PoWEngine::new(1));
        let plugin = Arc::new(PoWDomainPlugin::new(engine.clone()));
        registry.register(1, plugin).unwrap();
        let plugin2 = Arc::new(PoWDomainPlugin::new(engine));
        assert!(registry.register(1, plugin2).is_err());
    }
}
