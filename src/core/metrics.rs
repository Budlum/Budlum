use prometheus::{Encoder, IntCounter, IntGauge, Registry, TextEncoder};
use std::sync::Arc;

#[derive(Clone)]
pub struct Metrics {
    pub registry: Arc<Registry>,
    pub chain_height: IntGauge,
    pub peer_count: IntGauge,
    pub mempool_size: IntGauge,
    pub blocks_produced: IntCounter,
    pub transactions_processed: IntCounter,
    pub reorgs_total: IntCounter,
    pub finalized_height: IntGauge,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let chain_height = IntGauge::new("budlum_chain_height", "Current chain height").unwrap();
        let peer_count = IntGauge::new("budlum_peer_count", "Connected peers").unwrap();
        let mempool_size = IntGauge::new("budlum_mempool_size", "Pending transactions").unwrap();
        let blocks_produced =
            IntCounter::new("budlum_blocks_produced", "Total blocks produced").unwrap();
        let transactions_processed =
            IntCounter::new("budlum_transactions_processed", "Total transactions").unwrap();
        let reorgs_total = IntCounter::new("budlum_reorgs_total", "Total chain reorgs").unwrap();
        let finalized_height =
            IntGauge::new("budlum_finalized_height", "Finalized block height").unwrap();

        registry.register(Box::new(chain_height.clone())).unwrap();
        registry.register(Box::new(peer_count.clone())).unwrap();
        registry.register(Box::new(mempool_size.clone())).unwrap();
        registry
            .register(Box::new(blocks_produced.clone()))
            .unwrap();
        registry
            .register(Box::new(transactions_processed.clone()))
            .unwrap();
        registry.register(Box::new(reorgs_total.clone())).unwrap();
        registry
            .register(Box::new(finalized_height.clone()))
            .unwrap();

        Metrics {
            registry: Arc::new(registry),
            chain_height,
            peer_count,
            mempool_size,
            blocks_produced,
            transactions_processed,
            reorgs_total,
            finalized_height,
        }
    }

    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder.encode(&metric_families, &mut buffer).unwrap();
        String::from_utf8(buffer).unwrap()
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
