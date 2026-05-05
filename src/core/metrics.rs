use prometheus::{Encoder, Histogram, HistogramOpts, IntCounter, IntGauge, Registry, TextEncoder};
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
    pub block_propagation_seconds: Histogram,
    pub mempool_sender_count: IntGauge,
    pub peer_connection_quality: IntGauge,
    pub consensus_round_seconds: Histogram,
    pub finality_lag: IntGauge,
    pub storage_db_size_bytes: IntGauge,
    pub storage_write_seconds: Histogram,
    pub storage_read_seconds: Histogram,
    pub settlement_commitments_total: IntCounter,
    pub settlement_frozen_domains: IntGauge,
    pub settlement_global_headers_sealed: IntCounter,
    pub settlement_equivocations_detected: IntCounter,
    pub p2p_peers_connected: IntGauge,
    pub p2p_messages_received: IntCounter,
    pub p2p_gossip_duplicates: IntCounter,
    pub p2p_sync_requests: IntCounter,
    pub mempool_evictions: IntCounter,
    pub mempool_expired_cleanups: IntCounter,
}

impl Metrics {
    pub fn new() -> Self {
        let registry = Registry::new();

        let chain_height =
            IntGauge::new("budlum_chain_height", "Current chain height").expect("metric");
        let peer_count = IntGauge::new("budlum_peer_count", "Connected peers").expect("metric");
        let mempool_size =
            IntGauge::new("budlum_mempool_size", "Pending transactions").expect("metric");
        let blocks_produced =
            IntCounter::new("budlum_blocks_produced", "Total blocks produced").expect("metric");
        let transactions_processed =
            IntCounter::new("budlum_transactions_processed", "Total transactions").expect("metric");
        let reorgs_total =
            IntCounter::new("budlum_reorgs_total", "Total chain reorgs").expect("metric");
        let finalized_height =
            IntGauge::new("budlum_finalized_height", "Finalized block height").expect("metric");
        let block_propagation_seconds = Histogram::with_opts(HistogramOpts::new(
            "budlum_block_propagation_seconds",
            "Observed block propagation time in seconds",
        ))
        .expect("metric");
        let mempool_sender_count =
            IntGauge::new("budlum_mempool_sender_count", "Distinct senders in mempool")
                .expect("metric");
        let peer_connection_quality = IntGauge::new(
            "budlum_peer_connection_quality",
            "Aggregate peer quality score",
        )
        .expect("metric");
        let consensus_round_seconds = Histogram::with_opts(HistogramOpts::new(
            "budlum_consensus_round_seconds",
            "Consensus round duration in seconds",
        ))
        .expect("metric");
        let finality_lag =
            IntGauge::new("budlum_finality_lag", "Head height minus finalized height")
                .expect("metric");
        let storage_db_size_bytes = IntGauge::new(
            "budlum_storage_db_size_bytes",
            "Approximate storage size in bytes",
        )
        .expect("metric");
        let storage_write_seconds = Histogram::with_opts(HistogramOpts::new(
            "budlum_storage_write_seconds",
            "Storage write latency in seconds",
        ))
        .expect("metric");
        let storage_read_seconds = Histogram::with_opts(HistogramOpts::new(
            "budlum_storage_read_seconds",
            "Storage read latency in seconds",
        ))
        .expect("metric");
        let settlement_commitments_total = IntCounter::new(
            "budlum_settlement_commitments_total",
            "Total settlement commitments processed",
        )
        .expect("metric");
        let settlement_frozen_domains = IntGauge::new(
            "budlum_settlement_frozen_domains",
            "Frozen settlement domains",
        )
        .expect("metric");
        let settlement_global_headers_sealed = IntCounter::new(
            "budlum_settlement_global_headers_sealed",
            "Total sealed settlement global headers",
        )
        .expect("metric");
        let settlement_equivocations_detected = IntCounter::new(
            "budlum_settlement_equivocations_detected",
            "Total settlement equivocations detected",
        )
        .expect("metric");
        let p2p_peers_connected = IntGauge::new(
            "budlum_p2p_peers_connected",
            "Currently connected P2P peers",
        )
        .expect("metric");
        let p2p_messages_received = IntCounter::new(
            "budlum_p2p_messages_received",
            "Total P2P messages received",
        )
        .expect("metric");
        let p2p_gossip_duplicates = IntCounter::new(
            "budlum_p2p_gossip_duplicates",
            "Duplicate gossip messages observed",
        )
        .expect("metric");
        let p2p_sync_requests = IntCounter::new(
            "budlum_p2p_sync_requests",
            "P2P sync requests sent or handled",
        )
        .expect("metric");
        let mempool_evictions = IntCounter::new(
            "budlum_mempool_evictions",
            "Transactions evicted from mempool",
        )
        .expect("metric");
        let mempool_expired_cleanups = IntCounter::new(
            "budlum_mempool_expired_cleanups",
            "Expired mempool cleanup runs",
        )
        .expect("metric");

        registry
            .register(Box::new(chain_height.clone()))
            .expect("metric");
        registry
            .register(Box::new(peer_count.clone()))
            .expect("metric");
        registry
            .register(Box::new(mempool_size.clone()))
            .expect("metric");
        registry
            .register(Box::new(blocks_produced.clone()))
            .expect("metric");
        registry
            .register(Box::new(transactions_processed.clone()))
            .expect("metric");
        registry
            .register(Box::new(reorgs_total.clone()))
            .expect("metric");
        registry
            .register(Box::new(finalized_height.clone()))
            .expect("metric");
        registry
            .register(Box::new(block_propagation_seconds.clone()))
            .expect("metric");
        registry
            .register(Box::new(mempool_sender_count.clone()))
            .expect("metric");
        registry
            .register(Box::new(peer_connection_quality.clone()))
            .expect("metric");
        registry
            .register(Box::new(consensus_round_seconds.clone()))
            .expect("metric");
        registry
            .register(Box::new(finality_lag.clone()))
            .expect("metric");
        registry
            .register(Box::new(storage_db_size_bytes.clone()))
            .expect("metric");
        registry
            .register(Box::new(storage_write_seconds.clone()))
            .expect("metric");
        registry
            .register(Box::new(storage_read_seconds.clone()))
            .expect("metric");
        registry
            .register(Box::new(settlement_commitments_total.clone()))
            .expect("metric");
        registry
            .register(Box::new(settlement_frozen_domains.clone()))
            .expect("metric");
        registry
            .register(Box::new(settlement_global_headers_sealed.clone()))
            .expect("metric");
        registry
            .register(Box::new(settlement_equivocations_detected.clone()))
            .expect("metric");
        registry
            .register(Box::new(p2p_peers_connected.clone()))
            .expect("metric");
        registry
            .register(Box::new(p2p_messages_received.clone()))
            .expect("metric");
        registry
            .register(Box::new(p2p_gossip_duplicates.clone()))
            .expect("metric");
        registry
            .register(Box::new(p2p_sync_requests.clone()))
            .expect("metric");
        registry
            .register(Box::new(mempool_evictions.clone()))
            .expect("metric");
        registry
            .register(Box::new(mempool_expired_cleanups.clone()))
            .expect("metric");

        Metrics {
            registry: Arc::new(registry),
            chain_height,
            peer_count,
            mempool_size,
            blocks_produced,
            transactions_processed,
            reorgs_total,
            finalized_height,
            block_propagation_seconds,
            mempool_sender_count,
            peer_connection_quality,
            consensus_round_seconds,
            finality_lag,
            storage_db_size_bytes,
            storage_write_seconds,
            storage_read_seconds,
            settlement_commitments_total,
            settlement_frozen_domains,
            settlement_global_headers_sealed,
            settlement_equivocations_detected,
            p2p_peers_connected,
            p2p_messages_received,
            p2p_gossip_duplicates,
            p2p_sync_requests,
            mempool_evictions,
            mempool_expired_cleanups,
        }
    }

    pub fn encode(&self) -> String {
        let encoder = TextEncoder::new();
        let metric_families = self.registry.gather();
        let mut buffer = Vec::new();
        encoder
            .encode(&metric_families, &mut buffer)
            .expect("metrics encoding should not fail");
        String::from_utf8(buffer).expect("Prometheus text encoder emitted invalid UTF-8")
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self::new()
    }
}
