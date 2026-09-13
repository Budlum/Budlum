use crate::chain::finality::{
    FinalityAggregator, FinalityCert, Precommit, Prevote, ValidatorEntry, ValidatorSetSnapshot,
};
use crate::chain::genesis::{GenesisConfig, GENESIS_TIMESTAMP};
use crate::chain::snapshot::PruningManager;
use crate::consensus::pos::SlashingEvidence;
use crate::consensus::qc::{QcBlob, QcFaultProof, QcProofAction, QcProofVerdict};
use crate::consensus::ConsensusEngine;
use crate::core::account::{AccountState, GENESIS_BALANCE};
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::chain_config::Network;
use crate::core::transaction::Transaction;
use crate::cross_domain::{
    BridgeState, CrossDomainMessageRegistry, DomainEvent, DomainEventKind, MerkleProof, MessageKind,
};
use crate::domain::{
    hash_finality_proof, BftFinalityAdapter, ConsensusDomain, ConsensusDomainRegistry,
    ConsensusKind, DomainCommitment, DomainCommitmentRegistry, DomainFinalityAdapter, DomainId,
    DomainPluginRegistry, DomainStatus, FinalityProof, FinalityStatus, PoAFinalityAdapter,
    PoSFinalityAdapter, PoWFinalityAdapter, ZkFinalityAdapter,
};
use crate::execution::executor::Executor;
use crate::mempool::pool::Mempool;
use crate::settlement::{
    merkle_root, GlobalBlockHeader, ProofVerificationError, SettlementProofVerifier,
    VerifiedDomainEvent,
};
use crate::storage::db::Storage;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info, warn};

pub const MAX_REORG_DEPTH: usize = 100;
pub const FINALITY_DEPTH: usize = 50;
pub const EPOCH_LENGTH: u64 = 32;

pub struct Blockchain {
    pub chain: Vec<Block>,
    pub consensus: Arc<dyn ConsensusEngine>,
    pub mempool: Mempool,
    pub storage: Option<Storage>,
    pub state: AccountState,
    pub chain_id: u64,
    pub pruning_manager: Option<PruningManager>,
    pub finalized_height: u64,
    pub finalized_hash: String,
    pub genesis_time: u128,
    pub verified_qc_blobs: BTreeMap<u64, QcBlob>,
    pub validator_snapshots: BTreeMap<u64, ValidatorSetSnapshot>,
    pub pending_finality_certs: BTreeMap<u64, Vec<FinalityCert>>,
    pub domain_registry: ConsensusDomainRegistry,
    pub domain_commitment_registry: DomainCommitmentRegistry,
    pub bridge_state: BridgeState,
    pub global_headers: Vec<GlobalBlockHeader>,
    pub plugin_registry: DomainPluginRegistry,
    pub message_registry: CrossDomainMessageRegistry,
    pub settlement_finality_hashes: Vec<crate::domain::Hash32>,
    pub pending_slashing_evidence: Vec<SlashingEvidence>,
    pub finality_aggregator: Option<FinalityAggregator>,
    pub metrics: Option<Arc<crate::core::metrics::Metrics>>,
}
impl Blockchain {
    pub fn with_metrics(mut self, metrics: Arc<crate::core::metrics::Metrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    pub(crate) fn emit_chain_metrics(&self) {
        if let Some(ref m) = self.metrics {
            let height = self.chain.len() as i64;
            m.chain_height.set(height);
            m.finalized_height.set(self.finalized_height as i64);
            m.blocks_produced.inc();
            m.finality_lag
                .set((height as u64).saturating_sub(self.finalized_height) as i64);
            m.mempool_size.set(self.mempool.len() as i64);
        }
    }

    pub fn emit_tx_processed(&self, count: u64) {
        if let Some(ref m) = self.metrics {
            m.transactions_processed.inc_by(count);
        }
    }

    pub fn emit_reorg(&self) {
        if let Some(ref m) = self.metrics {
            m.reorgs_total.inc();
        }
    }

    pub fn emit_mempool_eviction(&self) {
        if let Some(ref m) = self.metrics {
            m.mempool_evictions.inc();
        }
    }

    pub fn emit_mempool_cleanup(&self) {
        if let Some(ref m) = self.metrics {
            m.mempool_expired_cleanups.inc();
        }
    }

    pub fn new(
        consensus: Arc<dyn ConsensusEngine>,
        storage: Option<Storage>,
        chain_id: u64,
        pruning_manager: Option<PruningManager>,
    ) -> Self {
        Self::new_with_genesis(consensus, storage, chain_id, pruning_manager, None)
    }

    pub fn new_with_genesis(
        consensus: Arc<dyn ConsensusEngine>,
        storage: Option<Storage>,
        chain_id: u64,
        pruning_manager: Option<PruningManager>,
        genesis_config: Option<GenesisConfig>,
    ) -> Self {
        info!("Consensus: {}", consensus.info());
        let mut chain_vec = Vec::new();

        let mut loaded_chain = false;
        if let Some(ref store) = storage {
            if let Ok(c) = store.load_chain() {
                if !c.is_empty() {
                    chain_vec = c;
                    loaded_chain = true;
                    info!("Loaded chain from DB: {} blocks", chain_vec.len());
                }
            }
        }

        let resolved_genesis_config = genesis_config.unwrap_or_else(|| {
            Network::from_chain_id(chain_id)
                .map(GenesisConfig::for_network)
                .unwrap_or_else(|| GenesisConfig::new(chain_id))
        });

        let mut state = resolved_genesis_config.build_state();

        if !loaded_chain {
            let genesis = resolved_genesis_config.build_genesis_block();
            if let Some(ref store) = storage {
                let mut accounts_to_save = Vec::new();
                for (pubkey, account) in &state.accounts {
                    accounts_to_save.push((*pubkey, account.clone()));
                }
                let batch = crate::storage::traits::DurableCommitBatch {
                    block: genesis.clone(),
                    state_root: genesis.state_root.clone(),
                    finality_cert: None,
                    global_headers: Vec::new(),
                    bridge_state: Some(BridgeState::new()),
                    accounts: accounts_to_save,
                };
                if let Err(e) = store.commit_durable_batch(&batch) {
                    error!("Failed to persist genesis block to storage: {:?}", e);
                }
            }
            chain_vec.push(genesis);
        } else {
            // VERIFICATION CHAIN (Doğrulama Zinciri):
            // Check that the existing genesis block in DB matches resolved_genesis_config / network config!
            let db_genesis = &chain_vec[0];
            let expected_genesis = resolved_genesis_config.build_genesis_block();

            // 1. Chain ID check
            if db_genesis.chain_id != chain_id {
                error!(
                    "CRITICAL ERROR: Startup Chain ID mismatch! DB genesis chain_id: {}, Configured chain_id: {}. Fail-closed startup.",
                    db_genesis.chain_id, chain_id
                );
                #[cfg(not(test))]
                std::process::exit(1);
                #[cfg(test)]
                panic!("Startup Chain ID mismatch!");
            }

            // 2. Genesis Hash check
            if db_genesis.hash != expected_genesis.hash {
                error!(
                    "CRITICAL ERROR: Startup Genesis Hash mismatch! DB genesis hash: {}, Expected hash: {}. Fail-closed startup.",
                    db_genesis.hash, expected_genesis.hash
                );
                #[cfg(not(test))]
                std::process::exit(1);
                #[cfg(test)]
                panic!("Startup Genesis Hash mismatch!");
            }

            // 3. Network Magic check
            let db_network = Network::from_chain_id(db_genesis.chain_id);
            let current_network = Network::from_chain_id(chain_id);
            if db_network != current_network {
                error!(
                    "CRITICAL ERROR: Startup Network Magic mismatch! DB network: {:?}, Configured network: {:?}. Fail-closed startup.",
                    db_network.map(|n| n.magic_bytes()), current_network.map(|n| n.magic_bytes())
                );
                #[cfg(not(test))]
                std::process::exit(1);
                #[cfg(test)]
                panic!("Startup Network Magic mismatch!");
            }
        }

        let mut snapshot_height = 0;
        let mut restored_finalized_height = 0;
        let mut restored_finalized_hash = chain_vec[0].hash.clone();

        if let Some(ref pm) = pruning_manager {
            // Try V2 snapshot first, fall back to V1
            let v2_loaded = if let Ok(Some(v2_snapshot)) = pm.load_latest_snapshot_v2() {
                if v2_snapshot.chain_id == chain_id {
                    state = crate::core::account::AccountState::from_snapshot_v2(&v2_snapshot);
                    snapshot_height = v2_snapshot.height;
                    restored_finalized_height = v2_snapshot.finalized_height;
                    restored_finalized_hash = v2_snapshot.finalized_hash.clone();
                    info!(
                        "Restored state from V2 snapshot at height {} (finalized={}, epoch={}, base_fee={})",
                        snapshot_height, restored_finalized_height, v2_snapshot.epoch_index, v2_snapshot.base_fee
                    );
                    true
                } else {
                    warn!(
                        "V2 snapshot chain_id mismatch (expected {}, got {}). Trying V1 fallback.",
                        chain_id, v2_snapshot.chain_id
                    );
                    false
                }
            } else {
                false
            };

            if !v2_loaded {
                if let Ok(Some(snapshot)) = pm.load_latest_snapshot() {
                    if snapshot.chain_id == chain_id {
                        for (addr, balance) in &snapshot.balances {
                            let acc = state.get_or_create(addr);
                            acc.balance = *balance;
                        }
                        for (addr, nonce) in &snapshot.nonces {
                            let acc = state.get_or_create(addr);
                            acc.nonce = *nonce;
                        }
                        snapshot_height = snapshot.height;
                        restored_finalized_height = snapshot.finalized_height;
                        restored_finalized_hash = snapshot.finalized_hash.clone();
                        info!(
                            "Restored state from V1 snapshot at height {} (finalized={})",
                            snapshot_height, restored_finalized_height
                        );
                    } else {
                        warn!(
                            "V1 snapshot chain_id mismatch (expected {}, got {}). Ignoring.",
                            chain_id, snapshot.chain_id
                        );
                    }
                }
            }
        }

        let chain_len = chain_vec.len();
        let start_index = if snapshot_height > 0 && snapshot_height < chain_len as u64 {
            (snapshot_height + 1) as usize
        } else {
            if snapshot_height >= chain_len as u64 {
                warn!("Chain shorter than snapshot height, replaying from genesis");
                0
            } else {
                1
            }
        };

        info!(
            "Replaying blocks from index {} to {}...",
            start_index,
            chain_len - 1
        );

        let mut validator_snapshots = BTreeMap::new();
        validator_snapshots.insert(
            state.epoch_index,
            Self::build_validator_snapshot_from_state(state.epoch_index, &state),
        );

        for block in chain_vec.iter().skip(start_index) {
            state = match Self::apply_block_effects(&state, block) {
                Ok(next_state) => next_state,
                Err(e) => {
                    error!(
                        "Failed to apply block {} during init: {}. Corrupted database, exiting.",
                        block.index, e
                    );
                    std::process::exit(1);
                }
            };
            validator_snapshots.insert(
                state.epoch_index,
                Self::build_validator_snapshot_from_state(state.epoch_index, &state),
            );
        }

        let mempool_config = Network::from_chain_id(chain_id)
            .map(|network| network.mempool_config())
            .unwrap_or_default();
        let mut mempool = Mempool::new(mempool_config);
        if let Some(ref store) = storage {
            if let Ok(txs) = store.load_mempool_txs() {
                let count = txs.len();
                for tx in txs {
                    let _ = mempool.add_transaction(tx);
                }
                if count > 0 {
                    info!("Restored {} transactions from mempool persistence", count);
                }
            }
        }

        let mut domain_registry = ConsensusDomainRegistry::new();
        let mut domain_commitment_registry = DomainCommitmentRegistry::new();
        let mut bridge_state = BridgeState::new();
        let mut global_headers = Vec::new();
        let mut message_registry = CrossDomainMessageRegistry::new();

        if let Some(ref store) = storage {
            if let Ok(domains) = store.load_consensus_domains() {
                for domain in domains {
                    if let Err(e) = Self::validate_consensus_domain_registration(&domain) {
                        warn!("Skipping invalid stored consensus domain: {}", e);
                        continue;
                    }
                    if let Err(e) = domain_registry.register(domain) {
                        warn!("Skipping duplicate stored consensus domain: {}", e);
                    }
                }
            }

            if let Ok(commitments) = store.load_domain_commitments() {
                for commitment in commitments {
                    if let Err(e) = Self::validate_stored_domain_commitment_metadata(
                        &domain_registry,
                        &commitment,
                    ) {
                        warn!("Skipping invalid stored domain commitment: {}", e);
                        continue;
                    }
                    if let Err(e) = domain_commitment_registry.insert(commitment.clone()) {
                        warn!("Skipping duplicate stored domain commitment: {}", e);
                    } else {
                        for (addr, new_nonce) in &commitment.state_updates {
                            if *new_nonce > state.get_nonce(addr) {
                                let account = state.get_or_create(addr);
                                account.nonce = *new_nonce;
                            }
                        }
                    }
                }
            }

            if let Ok(Some(stored_bridge_state)) = store.load_bridge_state() {
                bridge_state = stored_bridge_state;
            }

            if let Ok(stored_global_headers) = store.load_global_headers() {
                global_headers =
                    Self::validated_global_header_prefix(stored_global_headers, chain_id);
            }

            if let Ok(messages) = store.load_cross_domain_messages() {
                let mut registry = CrossDomainMessageRegistry::new();
                for msg in messages {
                    if let Err(e) = registry.insert(msg) {
                        warn!("Skipping duplicate cross domain message: {}", e);
                    }
                }
                message_registry = registry;
            }
        }

        let mut bc = Blockchain {
            chain: chain_vec,
            consensus,
            mempool,
            storage,
            state,
            chain_id,
            pruning_manager,
            finalized_height: restored_finalized_height,
            finalized_hash: restored_finalized_hash,
            genesis_time: 0,
            verified_qc_blobs: BTreeMap::new(),
            validator_snapshots,
            pending_finality_certs: BTreeMap::new(),
            domain_registry,
            domain_commitment_registry,
            bridge_state,
            global_headers,
            plugin_registry: DomainPluginRegistry::new(),
            message_registry,
            settlement_finality_hashes: Vec::new(),
            pending_slashing_evidence: Vec::new(),
            finality_aggregator: None,
            metrics: None,
        };

        if let Some(first) = bc.chain.first() {
            bc.genesis_time = first.timestamp;
        } else {
            bc.genesis_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_millis())
                .unwrap_or_default();
        }

        bc
    }

    #[allow(dead_code)]
    fn load_chain_from_db(&mut self, last_hash: String) -> std::io::Result<()> {
        let mut current_hash = last_hash;
        let mut blocks = Vec::new();
        if let Some(ref store) = self.storage {
            while let Ok(Some(block)) = store.get_block(&current_hash) {
                blocks.push(block.clone());
                if block.previous_hash == "0".repeat(64) {
                    break;
                }
                current_hash = block.previous_hash;
            }
        }
        if blocks.is_empty() {
            return Err(std::io::Error::other("Chain broken or empty"));
        }
        blocks.reverse();
        self.chain = blocks;
        info!("Loaded {} blocks from disk", self.chain.len());
        if let Some(store) = &self.storage {
            let _ = self.consensus.load_state(store);
        }
        Ok(())
    }
    #[allow(dead_code)]
    fn create_genesis_block(&mut self) {
        let genesis_block = Block::genesis();
        self.chain.push(genesis_block.clone());
        if let Some(ref store) = self.storage {
            let _ = store.insert_block(&genesis_block);
            let _ = store.save_last_hash(&genesis_block.hash);
        }
    }
    pub fn last_block(&self) -> &Block {
        self.chain.last().expect("Chain should never be empty")
    }

    pub fn register_consensus_domain(&mut self, domain: ConsensusDomain) -> Result<(), String> {
        Self::validate_consensus_domain_registration(&domain)?;
        self.domain_registry.register(domain.clone())?;
        if let Some(store) = &self.storage {
            store
                .save_consensus_domain(&domain)
                .map_err(|e| format!("Failed to persist consensus domain: {}", e))?;
        }
        Ok(())
    }

    pub fn submit_slashing_evidence(&mut self, evidence: SlashingEvidence) -> Result<(), String> {
        if !Self::verify_slashing_evidence(&evidence) {
            return Err("Invalid slashing evidence".into());
        }
        if self.pending_slashing_evidence.iter().any(|existing| {
            existing.header1.hash == evidence.header1.hash
                && existing.header2.hash == evidence.header2.hash
                && existing.signature1 == evidence.signature1
                && existing.signature2 == evidence.signature2
        }) {
            return Ok(());
        }
        self.pending_slashing_evidence.push(evidence);
        Ok(())
    }

    pub fn drain_local_slashing_evidence(&mut self) -> Vec<SlashingEvidence> {
        let mut evidence = self
            .consensus
            .drain_slashing_evidence()
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to drain consensus slashing evidence: {}", e);
                Vec::new()
            });
        for item in &evidence {
            let _ = self.submit_slashing_evidence(item.clone());
        }
        evidence.extend(self.pending_slashing_evidence.clone());
        evidence
    }

    fn verify_slashing_evidence(evidence: &SlashingEvidence) -> bool {
        if evidence.header1.index != evidence.header2.index {
            return false;
        }
        if evidence.header1.producer != evidence.header2.producer {
            return false;
        }
        if evidence.header1.producer.is_none() {
            return false;
        }
        if evidence.header1.hash == evidence.header2.hash {
            return false;
        }
        evidence.header1.verify_signature(&evidence.signature1)
            && evidence.header2.verify_signature(&evidence.signature2)
    }

    fn validate_consensus_domain_registration(domain: &ConsensusDomain) -> Result<(), String> {
        if domain.id == 0 {
            return Err("Consensus domain id 0 is reserved".into());
        }
        if domain.domain_chain_id == 0 {
            return Err(format!("Domain {} has invalid chain id 0", domain.id));
        }

        let expected_adapter = match &domain.kind {
            ConsensusKind::PoW => Some("pow-confirmation-depth"),
            ConsensusKind::PoS => Some("pos-qc-finality"),
            ConsensusKind::PoA => Some("poa-authority-quorum"),
            ConsensusKind::Bft => Some("bft-quorum-commit"),
            ConsensusKind::Zk => Some("zk-proof-verification"),
            ConsensusKind::Custom(name) => {
                if name.trim().is_empty() {
                    return Err(format!(
                        "Domain {} has empty custom consensus name",
                        domain.id
                    ));
                }
                None
            }
        };

        if let Some(expected) = expected_adapter {
            if domain.finality_adapter != expected {
                return Err(format!(
                    "Domain {} adapter mismatch: expected {}, got {}",
                    domain.id, expected, domain.finality_adapter
                ));
            }
        } else if domain.finality_adapter.trim().is_empty() {
            return Err(format!("Domain {} has empty finality adapter", domain.id));
        }

        Ok(())
    }

    fn validate_stored_domain_commitment_metadata(
        registry: &ConsensusDomainRegistry,
        commitment: &DomainCommitment,
    ) -> Result<(), String> {
        let domain = registry
            .get(commitment.domain_id)
            .ok_or_else(|| format!("Unknown consensus domain {}", commitment.domain_id))?;
        if !domain.is_active() {
            return Err(format!("Domain {} is not active", commitment.domain_id));
        }
        if domain.kind != commitment.consensus_kind {
            return Err(format!(
                "Commitment consensus kind mismatch for domain {}",
                commitment.domain_id
            ));
        }
        Ok(())
    }

    fn validated_global_header_prefix(
        headers: Vec<GlobalBlockHeader>,
        chain_id: u64,
    ) -> Vec<GlobalBlockHeader> {
        let mut valid = Vec::new();
        let mut expected_prev_hash = [0u8; 32];
        for (expected_height, header) in headers.into_iter().enumerate() {
            let expected_height = expected_height as u64;
            if header.chain_id != chain_id {
                warn!(
                    "Skipping stored global header with wrong chain id: height={}, chain_id={}",
                    header.global_height, header.chain_id
                );
                break;
            }
            if header.global_height != expected_height {
                warn!(
                    "Skipping stored global header with non-contiguous height: expected={}, got={}",
                    expected_height, header.global_height
                );
                break;
            }
            if header.previous_global_hash != expected_prev_hash {
                warn!(
                    "Skipping stored global header with broken hash chain: height={}",
                    header.global_height
                );
                break;
            }

            expected_prev_hash = header.calculate_hash_bytes();
            valid.push(header);
        }

        valid
    }

    #[cfg(not(test))]
    pub fn submit_domain_commitment(
        &mut self,
        _commitment: DomainCommitment,
    ) -> Result<(), String> {
        Err(
            "Raw domain commitment submission is disabled; verified finality proof is required"
                .into(),
        )
    }

    #[cfg(test)]
    pub fn submit_domain_commitment(&mut self, commitment: DomainCommitment) -> Result<(), String> {
        self.accept_domain_commitment(commitment)
    }

    fn accept_domain_commitment(&mut self, commitment: DomainCommitment) -> Result<(), String> {
        let domain = self.validate_domain_commitment_metadata(&commitment)?;

        if domain.status == DomainStatus::Frozen {
            return Err(format!("Domain {} is frozen", commitment.domain_id));
        }

        if domain.validator_set_hash != [0u8; 32]
            && commitment.validator_set_hash != domain.validator_set_hash
        {
            return Err(format!(
                "Commitment validator set hash mismatch for domain {}",
                commitment.domain_id
            ));
        }

        if commitment.domain_height <= domain.last_committed_height {
            if let Some(existing) = self
                .domain_commitment_registry
                .find_by_height(commitment.domain_id, commitment.domain_height)
            {
                if existing.domain_block_hash == commitment.domain_block_hash {
                    return Ok(());
                }
                let d_mut = self
                    .domain_registry
                    .get_mut(commitment.domain_id)
                    .ok_or_else(|| format!("Domain {} not found", commitment.domain_id))?;
                d_mut.status = DomainStatus::Frozen;
                if let Some(store) = &self.storage {
                    let _ = store.save_consensus_domain(d_mut);
                }
                return Err(format!(
                    "Equivocation or invalid sequence detected for domain {} height {}",
                    commitment.domain_id, commitment.domain_height
                ));
            }
            if commitment.domain_height == domain.last_committed_height
                && commitment.domain_block_hash == domain.last_committed_hash
            {
                return Ok(());
            }
            return Err(format!(
                "Stale or conflicting commitment for domain {} height {}",
                commitment.domain_id, commitment.domain_height
            ));
        }

        if let Some(existing) = self
            .domain_commitment_registry
            .find_by_height(commitment.domain_id, commitment.domain_height)
        {
            if existing.domain_block_hash == commitment.domain_block_hash
                && existing.sequence == commitment.sequence
            {
                return Ok(());
            } else {
                let d_mut = self
                    .domain_registry
                    .get_mut(commitment.domain_id)
                    .ok_or_else(|| format!("Domain {} not found", commitment.domain_id))?;
                d_mut.status = DomainStatus::Frozen;
                if let Some(store) = &self.storage {
                    let _ = store.save_consensus_domain(d_mut);
                }
                return Err(format!(
                    "Equivocation or invalid sequence detected for domain {} height {}",
                    commitment.domain_id, commitment.domain_height
                ));
            }
        }

        if commitment.domain_height == domain.last_committed_height + 1 {
            self.validate_commitment_state_updates(&commitment)?;
        }

        self.domain_commitment_registry.insert(commitment.clone())?;
        let updated_domains = self.apply_pending_commitments(commitment.domain_id)?;

        if let Some(store) = &self.storage {
            store
                .save_domain_commitment_batch(&commitment, &updated_domains)
                .map_err(|e| {
                    format!(
                        "Failed to atomically persist domain commitment batch: {}",
                        e
                    )
                })?;
        }

        Ok(())
    }

    fn validate_commitment_state_updates(
        &self,
        commitment: &DomainCommitment,
    ) -> Result<(), String> {
        for (addr, new_nonce) in &commitment.state_updates {
            if *new_nonce <= self.state.get_nonce(addr) {
                return Err(format!(
                    "Commitment nonce invariant violation for domain {} height {}",
                    commitment.domain_id, commitment.domain_height
                ));
            }
        }
        Ok(())
    }

    fn apply_pending_commitments(
        &mut self,
        domain_id: DomainId,
    ) -> Result<Vec<ConsensusDomain>, String> {
        let mut updated_domains = Vec::new();
        loop {
            let last_height = self
                .domain_registry
                .get(domain_id)
                .ok_or_else(|| format!("Domain {} not found", domain_id))?
                .last_committed_height;
            let next_height = last_height + 1;
            #[cfg(not(test))]
            let last_hash = self
                .domain_registry
                .get(domain_id)
                .ok_or_else(|| format!("Domain {} not found", domain_id))?
                .last_committed_hash;

            if let Some(com) = self
                .domain_commitment_registry
                .find_by_height(domain_id, next_height)
            {
                #[cfg(not(test))]
                if last_hash != [0u8; 32] && com.parent_domain_block_hash != last_hash {
                    let d_mut = self
                        .domain_registry
                        .get_mut(domain_id)
                        .ok_or_else(|| format!("Domain {} not found", domain_id))?;
                    d_mut.status = DomainStatus::Frozen;
                    return Err(format!(
                        "Domain {} parent hash mismatch at height {}",
                        domain_id, next_height
                    ));
                }

                self.validate_commitment_state_updates(&com)?;

                for (addr, new_nonce) in &com.state_updates {
                    let account = self.state.get_or_create(addr);
                    account.nonce = *new_nonce;
                }

                let d_mut = self
                    .domain_registry
                    .get_mut(domain_id)
                    .ok_or_else(|| format!("Domain {} not found", domain_id))?;
                d_mut.last_committed_height = next_height;
                d_mut.last_committed_hash = com.domain_block_hash;

                updated_domains.push(d_mut.clone());
            } else {
                break;
            }
        }
        Ok(updated_domains)
    }

    pub fn submit_verified_domain_commitment(
        &mut self,
        commitment: DomainCommitment,
        proof: FinalityProof,
    ) -> Result<(), String> {
        self.verify_domain_commitment_finality(&commitment, &proof)?;
        self.accept_domain_commitment(commitment)
    }

    pub fn verify_domain_commitment_finality(
        &self,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<(), String> {
        let domain = self.validate_domain_commitment_metadata(commitment)?;
        let expected_proof_hash = hash_finality_proof(proof);
        if commitment.finality_proof_hash != expected_proof_hash {
            return Err(format!(
                "Finality proof hash mismatch for domain {} height {}",
                commitment.domain_id, commitment.domain_height
            ));
        }

        let status = match domain.kind {
            ConsensusKind::PoW => {
                let adapter = PoWFinalityAdapter::default();
                self.ensure_adapter_name(domain, adapter.adapter_name())?;
                adapter.verify_finality(domain, commitment, proof)
            }
            ConsensusKind::PoS => {
                let adapter = PoSFinalityAdapter;
                self.ensure_adapter_name(domain, adapter.adapter_name())?;
                adapter.verify_finality(domain, commitment, proof)
            }
            ConsensusKind::PoA => {
                let adapter = PoAFinalityAdapter::default();
                self.ensure_adapter_name(domain, adapter.adapter_name())?;
                adapter.verify_finality(domain, commitment, proof)
            }
            ConsensusKind::Bft => {
                let adapter = BftFinalityAdapter::default();
                self.ensure_adapter_name(domain, adapter.adapter_name())?;
                adapter.verify_finality(domain, commitment, proof)
            }
            ConsensusKind::Zk => {
                let adapter = ZkFinalityAdapter;
                self.ensure_adapter_name(domain, adapter.adapter_name())?;
                adapter.verify_finality(domain, commitment, proof)
            }
            ConsensusKind::Custom(_) => {
                if let Some(plugin) = self.plugin_registry.get(domain.id) {
                    let fa = plugin.finality_adapter();
                    self.ensure_adapter_name(domain, fa.adapter_name())?;
                    fa.verify_finality(domain, commitment, proof)
                } else {
                    return Err(format!(
                        "No plugin registered for custom domain {}",
                        commitment.domain_id
                    ));
                }
            }
        }
        .map_err(|e| e.to_string())?;

        match status {
            FinalityStatus::Finalized => Ok(()),
            FinalityStatus::Pending {
                required_depth,
                observed_depth,
            } => Err(format!(
                "Domain commitment is not finalized: required={}, observed={}",
                required_depth, observed_depth
            )),
            FinalityStatus::Rejected(reason) => {
                Err(format!("Domain commitment finality rejected: {}", reason))
            }
        }
    }

    fn validate_domain_commitment_metadata(
        &self,
        commitment: &DomainCommitment,
    ) -> Result<&ConsensusDomain, String> {
        let domain = self
            .domain_registry
            .get(commitment.domain_id)
            .ok_or_else(|| format!("Unknown consensus domain {}", commitment.domain_id))?;

        if domain.status == DomainStatus::Frozen {
            return Err(format!("Domain {} is frozen", commitment.domain_id));
        }

        if !domain.is_active() {
            return Err(format!("Domain {} is not active", commitment.domain_id));
        }

        if domain.kind != commitment.consensus_kind {
            return Err(format!(
                "Commitment consensus kind mismatch for domain {}",
                commitment.domain_id
            ));
        }

        Ok(domain)
    }

    fn ensure_adapter_name(
        &self,
        domain: &ConsensusDomain,
        expected: &'static str,
    ) -> Result<(), String> {
        if domain.finality_adapter != expected {
            return Err(format!(
                "Domain {} finality adapter mismatch: expected {}, got {}",
                domain.id, expected, domain.finality_adapter
            ));
        }
        Ok(())
    }

    pub fn build_global_header(&self, proposer: Option<Address>) -> GlobalBlockHeader {
        let previous_global_hash = self
            .global_headers
            .last()
            .map(GlobalBlockHeader::calculate_hash_bytes)
            .unwrap_or([0u8; 32]);

        let settlement_finality_root = if self.settlement_finality_hashes.is_empty() {
            merkle_root(&[])
        } else {
            merkle_root(&self.settlement_finality_hashes)
        };

        GlobalBlockHeader {
            version: 1,
            global_height: self.global_headers.len() as u64,
            previous_global_hash,
            chain_id: self.chain_id,
            timestamp_ms: self.global_headers.len() as u128,
            domain_registry_root: self.domain_registry.root(),
            domain_commitment_root: self.domain_commitment_registry.root(),
            message_root: self.message_registry.root(),
            bridge_state_root: self.bridge_state.root(),
            replay_nonce_root: self.bridge_state.replay_root(),
            proposer,
            settlement_finality_root,
        }
    }

    pub fn seal_global_header(
        &mut self,
        proposer: Option<Address>,
    ) -> Result<GlobalBlockHeader, String> {
        let header = self.build_global_header(proposer);
        if let Some(store) = &self.storage {
            store
                .save_global_header(&header)
                .map_err(|e| format!("Failed to persist global header: {}", e))?;
        }
        self.global_headers.push(header.clone());
        Ok(header)
    }

    pub fn verify_domain_event_proof(
        &self,
        domain_id: crate::domain::DomainId,
        domain_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: DomainEvent,
        proof: &MerkleProof,
    ) -> Result<VerifiedDomainEvent, ProofVerificationError> {
        SettlementProofVerifier::verify_event_from_registry(
            &self.domain_commitment_registry,
            domain_id,
            domain_height,
            sequence,
            expected_block_hash,
            event,
            proof,
        )
    }

    pub fn mint_bridge_transfer_from_verified_event(
        &mut self,
        source_domain: crate::domain::DomainId,
        source_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: DomainEvent,
        proof: &MerkleProof,
    ) -> Result<(), String> {
        let verified = self
            .verify_domain_event_proof(
                source_domain,
                source_height,
                sequence,
                expected_block_hash,
                event,
                proof,
            )
            .map_err(|e| e.to_string())?;

        if verified.event.kind != DomainEventKind::BridgeLocked {
            return Err("Verified event is not a bridge lock event".into());
        }

        let verified_event_hash = verified.event.leaf_hash();
        let message = verified
            .event
            .message
            .clone()
            .ok_or_else(|| "Verified bridge lock event is missing message".to_string())?;

        if message.kind != MessageKind::BridgeLock {
            return Err("Verified event message is not a bridge lock message".into());
        }
        if verified.event.payload_hash != message.payload_hash {
            return Err("Verified bridge event payload hash mismatch".into());
        }
        if let Some(expected_event_hash) = self.bridge_state.source_event_hash(&message.message_id)
        {
            if expected_event_hash != verified_event_hash {
                return Err("Verified bridge source event hash mismatch".into());
            }
        }

        self.bridge_state
            .mint(&message)
            .map_err(|e| e.to_string())?;

        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        Ok(())
    }

    pub fn register_bridge_asset(
        &mut self,
        asset_id: crate::cross_domain::AssetId,
        domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        let domain_ref = self
            .domain_registry
            .get(domain)
            .ok_or_else(|| format!("Domain {} not found", domain))?;
        if !domain_ref.is_active() || !domain_ref.bridge_enabled {
            return Err(format!("Domain {} is not bridge-enabled", domain));
        }
        self.bridge_state
            .register_asset(asset_id, domain)
            .map_err(|e| e.to_string())?;
        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn lock_bridge_transfer(
        &mut self,
        source_domain: crate::domain::DomainId,
        target_domain: crate::domain::DomainId,
        source_height: u64,
        event_index: u32,
        asset_id: crate::cross_domain::AssetId,
        owner: Address,
        recipient: Address,
        amount: u128,
        expiry_height: u64,
    ) -> Result<(crate::cross_domain::BridgeTransfer, DomainEvent), String> {
        for domain_id in [source_domain, target_domain] {
            let domain = self
                .domain_registry
                .get(domain_id)
                .ok_or_else(|| format!("Domain {} not found", domain_id))?;
            if !domain.is_active() || !domain.bridge_enabled {
                return Err(format!("Domain {} is not bridge-enabled", domain_id));
            }
        }
        if source_domain == target_domain {
            return Err("Bridge source and target domains must differ".into());
        }
        if amount == 0 {
            return Err("Bridge transfer amount must be non-zero".into());
        }
        if expiry_height <= source_height {
            return Err("Bridge transfer expiry must be after source height".into());
        }
        let result = self
            .bridge_state
            .lock(
                source_domain,
                target_domain,
                source_height,
                event_index,
                asset_id,
                owner,
                recipient,
                amount,
                expiry_height,
            )
            .map_err(|e| e.to_string())?;
        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        if let Some(message) = result.1.message.clone() {
            self.submit_cross_domain_message(message)?;
        }
        Ok(result)
    }

    pub fn submit_cross_domain_message(
        &mut self,
        message: crate::cross_domain::CrossDomainMessage,
    ) -> Result<(), String> {
        self.message_registry.insert(message.clone())?;
        if let Some(store) = &self.storage {
            store
                .save_cross_domain_message(&message)
                .map_err(|e| format!("Failed to persist cross-domain message: {}", e))?;
        }
        Ok(())
    }

    pub fn burn_bridge_transfer(
        &mut self,
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        let _ = (message_id, domain);
        Err("Raw bridge burn is disabled; use a target-domain burn event path".into())
    }

    pub fn burn_bridge_transfer_with_event(
        &mut self,
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
        domain_height: u64,
        event_index: u32,
        expiry_height: u64,
    ) -> Result<DomainEvent, String> {
        let event = self
            .bridge_state
            .burn_with_event(
                message_id,
                domain,
                domain_height,
                event_index,
                expiry_height,
            )
            .map_err(|e| e.to_string())?;
        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        if let Some(message) = event.message.clone() {
            self.submit_cross_domain_message(message)?;
        }
        Ok(event)
    }

    pub fn unlock_bridge_transfer(
        &mut self,
        message_id: crate::cross_domain::MessageId,
        source_domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        let _ = (message_id, source_domain);
        Err("Raw bridge unlock is disabled; use a verified bridge burn event".into())
    }

    pub fn unlock_bridge_transfer_from_verified_event(
        &mut self,
        target_domain: crate::domain::DomainId,
        target_height: u64,
        sequence: u64,
        expected_block_hash: Option<crate::domain::Hash32>,
        event: DomainEvent,
        proof: &MerkleProof,
    ) -> Result<(), String> {
        let verified = self
            .verify_domain_event_proof(
                target_domain,
                target_height,
                sequence,
                expected_block_hash,
                event,
                proof,
            )
            .map_err(|e| e.to_string())?;

        if verified.event.kind != DomainEventKind::BridgeBurned {
            return Err("Verified event is not a bridge burn event".into());
        }

        let message = verified
            .event
            .message
            .clone()
            .ok_or_else(|| "Verified bridge burn event is missing message".to_string())?;

        if message.kind != MessageKind::BridgeBurn {
            return Err("Verified event message is not a bridge burn message".into());
        }
        if !message.verify_id() {
            return Err("Verified bridge burn message id is invalid".into());
        }
        if verified.event.payload_hash != message.payload_hash {
            return Err("Verified bridge burn event payload hash mismatch".into());
        }

        let transfer_id = message
            .correlation_id
            .ok_or_else(|| "Verified bridge burn message is missing correlation id".to_string())?;
        let transfer = self
            .bridge_state
            .transfer(&transfer_id)
            .ok_or_else(|| "Unknown bridge transfer".to_string())?;
        let source_domain = transfer.source_domain;
        if transfer.target_domain != message.source_domain {
            return Err("Verified bridge burn source domain mismatch".into());
        }
        if source_domain != message.target_domain {
            return Err("Verified bridge burn target domain mismatch".into());
        }
        if transfer.recipient != message.sender {
            return Err("Verified bridge burn sender mismatch".into());
        }
        if transfer.owner != message.recipient {
            return Err("Verified bridge burn recipient mismatch".into());
        }
        let expected_payload_hash =
            crate::cross_domain::bridge::bridge_payload_hash(transfer.asset_id, transfer.amount);
        if message.payload_hash != expected_payload_hash {
            return Err("Verified bridge burn payload does not match transfer".into());
        }

        self.bridge_state
            .unlock(transfer_id, source_domain)
            .map_err(|e| e.to_string())?;
        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        Ok(())
    }
    fn get_tx_block_height_timed(
        &self,
        store: &Storage,
        hash: &str,
    ) -> std::io::Result<Option<u64>> {
        let read_start = std::time::Instant::now();
        let result = store.get_tx_block_height(hash);
        if let Some(ref m) = self.metrics {
            m.storage_read_seconds
                .observe(read_start.elapsed().as_secs_f64());
        }
        result
    }
    pub fn get_transaction_by_hash(&self, hash: &str) -> Option<Transaction> {
        if let Some(ref store) = self.storage {
            if let Ok(Some(height)) = self.get_tx_block_height_timed(store, hash) {
                if let Some(block) = self.chain.get(height as usize) {
                    if let Some(tx) = block.transactions.iter().find(|t| t.hash == hash) {
                        return Some(tx.clone());
                    }
                }
            }
        }
        for block in &self.chain {
            if let Some(tx) = block.transactions.iter().find(|t| t.hash == hash) {
                return Some(tx.clone());
            }
        }
        self.mempool.get(hash).cloned()
    }
    pub fn get_transaction_receipt(&self, hash: &str) -> Option<serde_json::Value> {
        if let Some(ref store) = self.storage {
            if let Ok(Some(height)) = self.get_tx_block_height_timed(store, hash) {
                return Some(serde_json::json!({
                    "transactionHash": hash,
                    "blockNumber": format!("0x{:x}", height),
                    "status": "0x1"
                }));
            }
        }
        for block in &self.chain {
            if block.transactions.iter().any(|tx| tx.hash == hash) {
                return Some(serde_json::json!({
                    "transactionHash": hash,
                    "blockNumber": format!("0x{:x}", block.index),
                    "status": "0x1"
                }));
            }
        }
        None
    }
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.state.get_nonce(address)
    }

    pub fn get_validator_set_hash(&self) -> String {
        self.build_validator_snapshot(self.state.epoch_index)
            .set_hash
    }

    fn build_validator_snapshot(&self, epoch: u64) -> ValidatorSetSnapshot {
        Self::build_validator_snapshot_from_state(epoch, &self.state)
    }

    fn build_validator_snapshot_from_state(
        epoch: u64,
        state: &AccountState,
    ) -> ValidatorSetSnapshot {
        let active_validators = state.get_active_validators();
        let entries: Vec<ValidatorEntry> = active_validators
            .into_iter()
            .map(|v| ValidatorEntry {
                address: v.address,
                stake: v.stake,
                bls_public_key: v.bls_public_key.clone(),
                pop_signature: v.pop_signature.clone(),
                pq_public_key: v.pq_public_key.clone(),
            })
            .collect();
        ValidatorSetSnapshot::new(epoch, entries)
    }

    fn record_validator_snapshot(&mut self, epoch: u64) {
        let snapshot = self.build_validator_snapshot(epoch);
        self.validator_snapshots.insert(epoch, snapshot);
    }

    fn validator_snapshot_for_epoch(&self, epoch: u64) -> ValidatorSetSnapshot {
        if epoch == self.state.epoch_index {
            return self.build_validator_snapshot(epoch);
        }
        self.validator_snapshots
            .get(&epoch)
            .cloned()
            .unwrap_or_else(|| self.build_validator_snapshot(epoch))
    }

    pub fn get_qc_blob(&self, height: u64) -> Option<QcBlob> {
        self.verified_qc_blobs.get(&height).cloned().or_else(|| {
            self.storage
                .as_ref()
                .and_then(|store| store.get_qc_blob(height).unwrap_or(None))
        })
    }

    fn maybe_apply_detected_qc_faults(
        &mut self,
        snapshot: &ValidatorSetSnapshot,
        blob: &QcBlob,
    ) -> Result<(), String> {
        let proofs = blob.detect_fault_proofs(snapshot);
        for proof in proofs {
            let verdict = proof.verify_against_blob(blob, snapshot)?;
            self.apply_qc_fault_verdict(&proof, verdict)?;
        }
        Ok(())
    }

    fn invalidate_finality_from_height(&mut self, from_height: u64) {
        let checkpoint_interval = crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
        let qc_heights: Vec<u64> = self
            .verified_qc_blobs
            .range(from_height..)
            .map(|(height, _)| *height)
            .collect();
        for height in qc_heights {
            self.verified_qc_blobs.remove(&height);
        }

        if let Some(store) = &self.storage {
            let mut height = from_height;
            let upper_bound = self.chain.len().saturating_sub(1) as u64;
            while height <= upper_bound {
                let _ = store.delete_finality_cert(height);
                let _ = store.delete_qc_blob(height);
                if let Some(next) = height.checked_add(checkpoint_interval) {
                    height = next;
                } else {
                    break;
                }
            }
        }

        let mut new_finalized_height = 0;
        let mut new_finalized_hash = self
            .chain
            .first()
            .map(|block| block.hash.clone())
            .unwrap_or_default();

        if let Some(store) = &self.storage {
            let mut height = self.chain.len().saturating_sub(1) as u64;
            while height >= checkpoint_interval {
                if let Ok(Some(cert)) = store.get_finality_cert(height) {
                    new_finalized_height = cert.checkpoint_height;
                    new_finalized_hash = cert.checkpoint_hash;
                    break;
                }
                height = height.saturating_sub(checkpoint_interval);
                if height == 0 {
                    break;
                }
            }
        }

        self.finalized_height = new_finalized_height;
        self.finalized_hash = new_finalized_hash;

        if let Some(store) = &self.storage {
            let _ = store.save_canonical_height(self.finalized_height);
        }
    }

    fn apply_qc_fault_verdict(
        &mut self,
        proof: &QcFaultProof,
        verdict: QcProofVerdict,
    ) -> Result<(), String> {
        use crate::core::chain_config::FIXED_POINT_SCALE;

        if verdict.slash_validator || verdict.action == QcProofAction::SlashValidator {
            let validator_address = Address::from_hex(&proof.validator_address)
                .map_err(|e| format!("Invalid QC fault-proof validator address: {}", e))?;

            let slash_ratio_fixed = (50 * FIXED_POINT_SCALE) / 100;
            let _ = self.state.slash_validator(
                &validator_address,
                slash_ratio_fixed,
                "slashable QC fault",
            );
        }

        if let Some(height) = verdict.invalidate_from_height {
            self.invalidate_finality_from_height(height);
        }
        Ok(())
    }

    pub fn import_qc_blob(&mut self, blob: QcBlob) -> Result<(), String> {
        if !crate::chain::finality::is_checkpoint_height(blob.checkpoint_height) {
            return Err(format!(
                "Height {} is not a valid checkpoint height",
                blob.checkpoint_height
            ));
        }

        let block = self
            .chain
            .get(blob.checkpoint_height as usize)
            .ok_or_else(|| {
                format!(
                    "Missing checkpoint block at height {}",
                    blob.checkpoint_height
                )
            })?;

        if block.hash != blob.checkpoint_hash {
            return Err(format!(
                "QcBlob checkpoint hash mismatch: expected {}, got {}",
                block.hash, blob.checkpoint_hash
            ));
        }

        let snapshot = self.validator_snapshot_for_epoch(blob.epoch);
        blob.verify_against_snapshot(&snapshot, None, Some(self.state.epoch_index))?;

        self.verified_qc_blobs
            .insert(blob.checkpoint_height, blob.clone());
        if let Some(store) = &self.storage {
            let _ = store.save_qc_blob(blob.checkpoint_height, &blob);
        }

        self.process_pending_finality_certs(blob.checkpoint_height)?;
        Ok(())
    }

    pub fn handle_qc_fault_proof(&mut self, proof: QcFaultProof) -> Result<(), String> {
        let blob = self
            .get_qc_blob(proof.checkpoint_height)
            .ok_or_else(|| format!("Missing QC blob at height {}", proof.checkpoint_height))?;
        let snapshot = self.validator_snapshot_for_epoch(proof.epoch);
        let verdict = proof.verify_against_blob(&blob, &snapshot)?;
        self.apply_qc_fault_verdict(&proof, verdict)
    }

    fn projected_sender_state(&self, tx: &Transaction) -> (u64, u64) {
        let mut expected_nonce = self.state.get_nonce(&tx.from);
        let mut spendable_balance = self.state.get_balance(&tx.from);

        for pending in self.mempool.sender_transactions(&tx.from) {
            if pending.nonce == tx.nonce {
                continue;
            }
            if pending.nonce < expected_nonce {
                continue;
            }
            if pending.nonce != expected_nonce {
                break;
            }

            let pending_cost = pending.total_cost();
            if spendable_balance < pending_cost {
                break;
            }

            spendable_balance = spendable_balance.saturating_sub(pending_cost);
            expected_nonce = expected_nonce.saturating_add(1);
        }

        (expected_nonce, spendable_balance)
    }

    fn validate_pool_transaction(&self, tx: &Transaction) -> Result<(), String> {
        if tx.chain_id != self.chain_id {
            return Err(format!(
                "Invalid Chain ID: expected {}, got {}",
                self.chain_id, tx.chain_id
            ));
        }
        if tx.from == Address::zero() {
            return Err("Genesis transactions cannot be submitted to the mempool".into());
        }

        let (expected_nonce, spendable_balance) = self.projected_sender_state(tx);
        self.state
            .validate_transaction_with_context(tx, expected_nonce, spendable_balance)
    }

    pub fn tx_precheck(&self, tx: &Transaction) -> serde_json::Value {
        let mut reasons = Vec::new();

        if tx.chain_id != self.chain_id {
            reasons.push("invalid_chain_id".to_string());
        }
        if tx.from == Address::zero() {
            reasons.push("genesis_transaction_forbidden".to_string());
        }
        if !tx.verify() {
            reasons.push("invalid_signature".to_string());
        }
        if tx.fee < self.state.base_fee {
            reasons.push("fee_too_low".to_string());
        }

        let (expected_nonce, spendable_balance) = self.projected_sender_state(tx);
        if tx.nonce < expected_nonce {
            reasons.push("nonce_too_low".to_string());
        } else if tx.nonce > expected_nonce {
            reasons.push("nonce_too_high".to_string());
        }
        if spendable_balance < tx.total_cost() {
            reasons.push("insufficient_funds".to_string());
        }

        match tx.tx_type {
            crate::core::transaction::TransactionType::Transfer => {
                if tx.to == Address::zero() {
                    reasons.push("missing_to_address".to_string());
                }
            }
            crate::core::transaction::TransactionType::Stake => {
                if tx.amount == 0 {
                    reasons.push("invalid_stake_amount".to_string());
                }
            }
            crate::core::transaction::TransactionType::Unstake => {
                match self.state.get_validator(&tx.from) {
                    Some(validator) if validator.stake >= tx.amount => {}
                    Some(_) => reasons.push("insufficient_stake".to_string()),
                    None => reasons.push("not_a_validator".to_string()),
                }
            }
            crate::core::transaction::TransactionType::Vote => {
                if self.state.get_validator(&tx.from).is_none() {
                    reasons.push("not_a_validator".to_string());
                }
            }
            crate::core::transaction::TransactionType::ContractCall => {
                if tx.amount != 0 {
                    reasons.push("contract_amount_must_be_zero".to_string());
                }
                if tx.data.is_empty() || !tx.data.len().is_multiple_of(8) {
                    reasons.push("invalid_contract_bytecode".to_string());
                }
            }
        }

        if reasons.is_empty() {
            let mut probe = self.mempool.clone();
            if let Err(err) = probe.add_transaction(tx.clone()) {
                let reason = match err {
                    crate::mempool::pool::MempoolError::PoolFull => "pool_full",
                    crate::mempool::pool::MempoolError::DuplicateTransaction => {
                        "duplicate_transaction"
                    }
                    crate::mempool::pool::MempoolError::FeeTooLow => "fee_too_low",
                    crate::mempool::pool::MempoolError::SenderLimitReached => {
                        "sender_limit_reached"
                    }
                    crate::mempool::pool::MempoolError::InvalidNonce => "invalid_nonce",
                    crate::mempool::pool::MempoolError::TransactionExpired => "transaction_expired",
                    crate::mempool::pool::MempoolError::RbfFeeTooLow => "rbf_fee_too_low",
                    crate::mempool::pool::MempoolError::InvalidTransaction(_) => {
                        "invalid_transaction"
                    }
                };
                reasons.push(reason.to_string());
            }
        }

        serde_json::json!({
            "accepted": reasons.is_empty(),
            "reasons": reasons
        })
    }

    fn collect_block_transactions(&self) -> Vec<Transaction> {
        let pending_txs = self.mempool.get_sorted_transactions(10000);
        let mut valid_txs = Vec::new();
        let mut temp_state = self.state.clone();
        let mut included = std::collections::HashSet::new();
        let mut progress = true;

        while progress {
            progress = false;
            for tx in &pending_txs {
                if included.contains(&tx.hash) {
                    continue;
                }
                if temp_state.validate_transaction(tx).is_err() {
                    continue;
                }
                if Executor::apply_transaction_checked(&mut temp_state, tx).is_ok() {
                    valid_txs.push(tx.clone());
                    included.insert(tx.hash.clone());
                    progress = true;
                }
            }
        }

        for tx in &pending_txs {
            if !included.contains(&tx.hash) && self.state.validate_transaction(tx).is_err() {
                warn!("Discarding invalid transaction: {}", tx.hash);
            }
        }

        valid_txs
    }

    fn adjust_base_fee(state: &mut AccountState, tx_count: usize) {
        let tx_count = tx_count as u64;
        let target = 50u64;
        let max_base_fee = 10_000_000;

        if tx_count > target {
            state.base_fee = state
                .base_fee
                .saturating_add(state.base_fee / 8)
                .min(max_base_fee);
        } else if tx_count < target {
            state.base_fee = state.base_fee.saturating_sub(state.base_fee / 8).max(1);
        }
    }

    fn apply_system_effects(state: &mut AccountState, block: &Block) {
        if let Some(evidences) = &block.slashing_evidence {
            let slash_ratio_fixed = (10 * crate::core::chain_config::FIXED_POINT_SCALE) / 100;
            state.apply_slashing(evidences, slash_ratio_fixed);
        }

        if block.index > 0 && block.index.is_multiple_of(EPOCH_LENGTH) {
            state.advance_epoch(block.timestamp);
        }

        if block.index > 0 {
            Self::adjust_base_fee(state, block.transactions.len());
        }
    }

    fn commit_block_durable(
        &self,
        block: &Block,
        committed_state: &AccountState,
    ) -> Result<(), String> {
        if let Some(ref store) = self.storage {
            let mut accounts_to_save = Vec::new();
            for (pubkey, account) in &committed_state.accounts {
                accounts_to_save.push((*pubkey, account.clone()));
            }

            let finality_cert = self
                .pending_finality_certs
                .get(&block.index)
                .and_then(|certs| certs.first().cloned());

            let batch = crate::storage::traits::DurableCommitBatch {
                block: block.clone(),
                state_root: block.state_root.clone(),
                finality_cert,
                global_headers: self.global_headers.clone(),
                bridge_state: Some(self.bridge_state.clone()),
                accounts: accounts_to_save,
            };

            let write_start = std::time::Instant::now();
            let result = store.commit_durable_batch(&batch);
            if let Some(ref m) = self.metrics {
                m.storage_write_seconds
                    .observe(write_start.elapsed().as_secs_f64());
            }
            result.map_err(|e| format!("Failed to commit durable batch: {}", e))?;
        }
        Ok(())
    }

    fn apply_block_effects(
        base_state: &AccountState,
        block: &Block,
    ) -> Result<AccountState, String> {
        let mut next_state = base_state.clone();
        Executor::apply_block_checked(
            &mut next_state,
            &block.transactions,
            block.producer.as_ref(),
        )
        .map_err(|e| format!("Failed to apply block: {}", e))?;
        Self::apply_system_effects(&mut next_state, block);
        Ok(next_state)
    }

    pub fn produce_block(&mut self, producer_address: Address) -> Option<Block> {
        let round_start = std::time::Instant::now();
        let index = self.chain.len() as u64;
        let previous_hash = self
            .chain
            .last()
            .map(|block| block.hash.clone())
            .unwrap_or_else(|| "0".repeat(64));
        let valid_txs = self.collect_block_transactions();
        let mut block = Block::new(index, previous_hash, valid_txs);
        if !self.pending_slashing_evidence.is_empty() {
            block.slashing_evidence = Some(self.pending_slashing_evidence.clone());
        }
        block.producer = Some(producer_address);
        block.timestamp =
            self.genesis_time + (index as u128 * crate::core::chain_config::SLOT_MS as u128);
        block.validator_set_hash = self.get_validator_set_hash();

        if self
            .consensus
            .preview_block(&mut block, &self.state)
            .is_err()
        {
            return None;
        }

        let mut committed_state = match Self::apply_block_effects(&self.state, &block) {
            Ok(state) => state,
            Err(_) => return None,
        };
        committed_state.bridge_root = self.bridge_state.root();
        committed_state.message_root = self.message_registry.root();
        let settlement_root = if self.settlement_finality_hashes.is_empty() {
            merkle_root(&[])
        } else {
            merkle_root(&self.settlement_finality_hashes)
        };
        committed_state.settlement_root = settlement_root;
        committed_state.global_header_summary = self
            .global_headers
            .last()
            .map(|h| h.calculate_hash_bytes())
            .unwrap_or([0u8; 32]);
        block.state_root = committed_state.calculate_state_root();

        if let Err(_e) = self.consensus.prepare_block(&mut block, &self.state) {
            return None;
        }

        // Commit durably to database first, ensuring fail-closed security
        if let Err(e) = self.commit_block_durable(&block, &committed_state) {
            tracing::error!(
                "Failed to commit block {} durably: {}. Block production aborted.",
                block.index,
                e
            );
            return None;
        }

        self.state = committed_state;
        self.record_validator_snapshot(self.state.epoch_index);

        self.chain.push(block.clone());
        if block.slashing_evidence.is_some() {
            self.pending_slashing_evidence.clear();
        }

        if let Some(last_block) = self.chain.last() {
            if let Err(e) = self
                .consensus
                .record_block(last_block, self.storage.as_ref())
            {
                warn!("Engine record block error: {}", e);
            }
        }

        for tx in &block.transactions {
            self.mempool.remove_transaction(&tx.hash);
            if let Some(ref store) = self.storage {
                let _ = store.remove_mempool_tx(&tx.hash);
            }
        }

        self.mempool.set_min_fee(self.state.base_fee);
        self.emit_chain_metrics();
        if let Some(ref m) = self.metrics {
            m.consensus_round_seconds
                .observe(round_start.elapsed().as_secs_f64());
        }
        Some(block)
    }
    pub fn mine_pending_transactions(&mut self, miner_address: Address) {
        self.produce_block(miner_address);
    }
    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), String> {
        self.validate_pool_transaction(&transaction)
            .map_err(|e| format!("Invalid transaction: {}", e))?;

        self.mempool
            .add_transaction(transaction.clone())
            .map_err(|e| format!("Mempool error: {:?}", e))?;
        if let Some(ref store) = self.storage {
            let _ = store.save_mempool_tx(&transaction);
        }
        Ok(())
    }

    pub fn init_genesis_account(&mut self, address: &Address) {
        let account = self.state.get_or_create(address);
        if account.balance < GENESIS_BALANCE {
            account.balance = GENESIS_BALANCE;
        }
    }

    pub fn validate_and_add_block(&mut self, block: Block) -> Result<(), String> {
        if block.index <= self.finalized_height && block.hash != self.finalized_hash {
            if let Some(finalized_path_block) = self.chain.get(block.index as usize) {
                if finalized_path_block.hash != block.hash {
                    return Err(format!(
                        "Block at height {} conflicts with finalized checkpoint",
                        block.index
                    ));
                }
            } else {
                return Err(format!(
                    "Block at height {} is below finalized height {}",
                    block.index, self.finalized_height
                ));
            }
        }

        if block.chain_id != self.chain_id {
            return Err(format!(
                "Invalid Chain ID: expected {}, got {}",
                self.chain_id, block.chain_id
            ));
        }

        let expected_tx_root = block.calculate_tx_root();
        if block.tx_root != expected_tx_root {
            return Err(format!(
                "tx_root mismatch: expected {}, got {}",
                expected_tx_root, block.tx_root
            ));
        }

        let expected_hash = block.calculate_hash();
        if block.hash != expected_hash {
            return Err(format!(
                "block hash mismatch: expected {}, got {}",
                expected_hash, block.hash
            ));
        }

        if block.index > 0 && block.state_root.is_empty() {
            return Err("Block missing state_root".into());
        }

        if let Err(e) = self
            .consensus
            .full_validate(&block, &self.chain, &self.state)
        {
            return Err(format!("Consensus validation failed: {}", e));
        }

        let mut temp_state = self.state.clone();
        for (i, tx) in block.transactions.iter().enumerate() {
            if tx.chain_id != block.chain_id {
                return Err(format!(
                    "Invalid transaction at index {}: Chain ID mismatch. Expected {}, got {}",
                    i, block.chain_id, tx.chain_id
                ));
            }
            if block.index > 0 && tx.from == Address::zero() {
                return Err(format!(
                    "Invalid transaction at index {}: 'genesis' transactions only allowed in genesis block", i
                ));
            }
            if block.index > 0 {
                if let Err(e) = temp_state.validate_transaction(tx) {
                    return Err(format!("Invalid transaction at index {}: {}", i, e));
                }
            }
            if let Err(e) = Executor::apply_transaction_checked(&mut temp_state, tx) {
                return Err(format!("Failed to apply transaction at index {}: {}", i, e));
            }
        }

        let mut commit_state = Self::apply_block_effects(&self.state, &block)?;

        if block.index > 0 {
            commit_state.bridge_root = self.bridge_state.root();
            commit_state.message_root = self.message_registry.root();
            let settlement_root = if self.settlement_finality_hashes.is_empty() {
                merkle_root(&[])
            } else {
                merkle_root(&self.settlement_finality_hashes)
            };
            commit_state.settlement_root = settlement_root;
            commit_state.global_header_summary = self
                .global_headers
                .last()
                .map(|h| h.calculate_hash_bytes())
                .unwrap_or([0u8; 32]);
            let computed_root = commit_state.calculate_state_root();
            if computed_root != block.state_root {
                return Err(format!(
                    "State root mismatch: expected {}, got {}",
                    block.state_root, computed_root
                ));
            }
        }

        // Commit durably to database first, ensuring fail-closed security
        self.commit_block_durable(&block, &commit_state)
            .map_err(|e| format!("Failed to commit block {} durably: {}", block.index, e))?;

        self.state = commit_state;
        self.record_validator_snapshot(self.state.epoch_index);
        self.mempool.set_min_fee(self.state.base_fee);

        self.chain.push(block);

        if let Some(last_block) = self.chain.last() {
            if let Err(e) = self
                .consensus
                .record_block(last_block, self.storage.as_ref())
            {
                warn!("Engine record block error: {}", e);
            }
        }

        if let Some(last_block) = self.chain.last() {
            for tx in last_block.transactions.iter() {
                self.mempool.remove_transaction(&tx.hash);
                if let Some(ref store) = self.storage {
                    let _ = store.remove_mempool_tx(&tx.hash);
                }
            }
        }

        if let (Some(pruning_manager), Some(last_block)) =
            (self.pruning_manager.as_ref(), self.chain.last())
        {
            let height = last_block.index;
            if pruning_manager.should_create_snapshot(height) {
                let genesis_hash = self
                    .chain
                    .first()
                    .map(|b| b.hash.clone())
                    .unwrap_or_default();
                let certs: Vec<FinalityCert> = self
                    .pending_finality_certs
                    .values()
                    .flat_map(|v| v.iter().cloned())
                    .collect();
                let params = crate::chain::snapshot::StateSnapshotV2Params {
                    height,
                    block_hash: last_block.hash.clone(),
                    genesis_hash,
                    chain_id: self.chain_id,
                    finalized_height: self.finalized_height,
                    finalized_hash: self.finalized_hash.clone(),
                    finality_certificates: certs,
                };
                let v2_snapshot =
                    crate::chain::snapshot::StateSnapshotV2::from_state(&self.state, params);
                if let Err(e) = pruning_manager.save_snapshot_v2(&v2_snapshot) {
                    warn!("Failed to save V2 snapshot at height {}: {}", height, e);
                } else {
                    info!(
                        "Saved V2 state snapshot at height {} (epoch={}, base_fee={})",
                        height, self.state.epoch_index, self.state.base_fee
                    );

                    let prunable = pruning_manager.get_prunable_blocks(
                        self.chain.len() as u64,
                        height,
                        self.finalized_height,
                    );
                    if !prunable.is_empty() {
                        if let Some(ref store) = self.storage {
                            for block_index in &prunable {
                                let _ = store.delete_block(*block_index);
                            }
                            info!("Pruned {} old blocks from disk", prunable.len());
                        }
                    }
                }
            }
        }

        self.emit_chain_metrics();
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        for i in 0..self.chain.len() {
            let block = &self.chain[i];
            let previous_chain = &self.chain[..i];
            let dummy_state = AccountState::new();
            if let Err(e) = self
                .consensus
                .validate_block(block, previous_chain, &dummy_state)
            {
                warn!("Block {} validation failed: {}", i, e);
                return false;
            }
        }
        true
    }
    pub fn is_valid_chain(&self, chain: &[Block]) -> bool {
        if chain.is_empty() {
            return false;
        }

        let genesis = &chain[0];
        if genesis.index != 0
            || genesis.previous_hash != "0".repeat(64)
            || genesis.timestamp != GENESIS_TIMESTAMP
            || genesis.hash != genesis.calculate_hash()
            || genesis.chain_id != self.chain_id
        {
            return false;
        }

        for i in 0..chain.len() {
            let block = &chain[i];
            let previous_chain = &chain[..i];
            let dummy_state = AccountState::new();
            if self
                .consensus
                .validate_block(block, previous_chain, &dummy_state)
                .is_err()
            {
                return false;
            }
        }
        true
    }
    pub fn find_fork_point(&self, other_chain: &[Block]) -> Option<usize> {
        for (i, block) in self.chain.iter().enumerate() {
            if i >= other_chain.len() {
                return None;
            }
            if block.hash != other_chain[i].hash {
                return Some(i);
            }
        }
        None
    }
    pub fn try_reorg(&mut self, new_chain: Vec<Block>) -> Result<bool, String> {
        if !self.consensus.is_better_chain(&self.chain, &new_chain) {
            return Ok(false);
        }
        if !self.is_valid_chain(&new_chain) {
            return Err("Invalid chain".to_string());
        }

        let fork_point = self.find_fork_point(&new_chain).unwrap_or(0);
        let reorg_depth = self.chain.len().saturating_sub(fork_point);

        if reorg_depth > MAX_REORG_DEPTH {
            return Err(format!(
                "Reorg depth {} exceeds max {}",
                reorg_depth, MAX_REORG_DEPTH
            ));
        }

        let finalized_height = self.chain.len().saturating_sub(FINALITY_DEPTH);
        if fork_point < finalized_height {
            return Err("Cannot reorg past finality depth".to_string());
        }

        info!(
            "Reorg: replacing {} blocks from height {}",
            reorg_depth, fork_point
        );

        let old_chain = self.chain.clone();
        let new_state = Blockchain::rebuild_state(&new_chain)?;

        for block in &old_chain[fork_point..] {
            self.verified_qc_blobs.remove(&block.index);
        }

        self.chain = new_chain;
        self.state = new_state;
        self.validator_snapshots.clear();
        self.record_validator_snapshot(self.state.epoch_index);

        let mut new_pending = Vec::new();

        let mut chain_txs = std::collections::HashSet::new();
        for block in &self.chain {
            for tx in &block.transactions {
                chain_txs.insert(tx.hash.clone());
            }
        }

        for tx in &self.mempool.get_sorted_transactions(1000) {
            if !chain_txs.contains(&tx.hash) {
                new_pending.push(tx.clone());
            }
        }

        self.mempool =
            crate::mempool::pool::Mempool::new(crate::mempool::pool::MempoolConfig::default());
        for tx in new_pending {
            let _ = self.mempool.add_transaction(tx);
        }

        if let Some(ref store) = self.storage {
            for block in &old_chain[fork_point..] {
                let _ = store.delete_block(block.index);
                for tx in &block.transactions {
                    let _ = store.delete_tx_index(&tx.hash);
                }
            }
            let mut current_state = if fork_point > 0 {
                Blockchain::rebuild_state(&self.chain[..fork_point])?
            } else {
                AccountState::new()
            };
            for block in &self.chain[fork_point..] {
                current_state = Self::apply_block_effects(&current_state, block)?;
                self.commit_block_durable(block, &current_state)?;
            }
            if let Some(last) = self.chain.last() {
                let _ = store.save_last_hash(&last.hash);
            }
        }

        self.mempool.set_min_fee(self.state.base_fee);
        Ok(true)
    }

    pub fn get_state_root(&self, height: u64) -> Option<String> {
        self.storage
            .as_ref()
            .and_then(|store| store.get_state_root(height).unwrap_or(None))
    }

    fn rebuild_state(chain: &[Block]) -> Result<AccountState, String> {
        let chain_id = chain
            .first()
            .map(|block| block.chain_id)
            .unwrap_or_default();
        let genesis_config = Network::from_chain_id(chain_id)
            .map(GenesisConfig::for_network)
            .unwrap_or_else(|| GenesisConfig::new(chain_id));
        let mut state = genesis_config.build_state();

        for block in chain.iter().skip(1) {
            state = Self::apply_block_effects(&state, block)
                .map_err(|e| format!("Failed to rebuild state at block {}: {}", block.index, e))?;
        }
        Ok(state)
    }
    pub fn print_info(&self) {
        info!("Blockchain Info");
        info!("Consensus: {}", self.consensus.info());
        info!("Length: {}", self.chain.len());
        info!("Pending Tx: {}", self.mempool.len());
        for block in &self.chain {
            info!("Block #{}: {}", block.index, &block.hash[..16]);
        }
    }
    pub fn get_state_snapshot(&self, height: u64) -> Option<crate::chain::snapshot::StateSnapshot> {
        if height >= self.chain.len() as u64 {
            return None;
        }
        let block = &self.chain[height as usize];
        let state = Self::rebuild_state(&self.chain[..=height as usize]).ok()?;
        let finalized_height = self.finalized_height.min(height);
        let finalized_hash = self
            .chain
            .get(finalized_height as usize)
            .map(|block| block.hash.clone())
            .unwrap_or_else(|| self.finalized_hash.clone());

        // Produce V2 snapshot with full consensus metadata
        let genesis_hash = self
            .chain
            .first()
            .map(|b| b.hash.clone())
            .unwrap_or_default();
        let certs: Vec<FinalityCert> = self
            .pending_finality_certs
            .values()
            .flat_map(|v| v.iter())
            .filter(|c| c.checkpoint_height <= height)
            .cloned()
            .collect();

        let params = crate::chain::snapshot::StateSnapshotV2Params {
            height,
            block_hash: block.hash.clone(),
            genesis_hash,
            chain_id: self.chain_id,
            finalized_height,
            finalized_hash: finalized_hash.clone(),
            finality_certificates: certs,
        };
        let v2 = crate::chain::snapshot::StateSnapshotV2::from_state(&state, params);

        // Encode V2 to JSON bytes; receivers try V2 first, V1 as fallback
        let v2_bytes = v2.to_bytes();

        // Wrap as V1-compatible bytes (serde ignores unknown fields on V1 parse)
        let mut snapshot = crate::chain::snapshot::StateSnapshot::from_state(
            height,
            block.hash.clone(),
            self.chain_id,
            &state,
            finalized_height,
            finalized_hash,
        );
        // Embed V2 data for capable receivers
        snapshot.snapshot_hash = format!("__v2__{}", hex::encode(&v2_bytes));
        Some(snapshot)
    }

    pub fn apply_state_snapshot(
        &mut self,
        snapshot: crate::chain::snapshot::StateSnapshot,
    ) -> Result<(), String> {
        use crate::chain::snapshot::StateSnapshotV2;

        // Try V2 restore if available (embedded in snapshot_hash prefix)
        if let Some(v2_data) = snapshot.snapshot_hash.strip_prefix("__v2__") {
            if let Ok(v2_bytes) = hex::decode(v2_data) {
                if let Ok(v2) = StateSnapshotV2::from_bytes(&v2_bytes) {
                    if v2.verify() && v2.chain_id == self.chain_id {
                        return self.apply_v2_snapshot(&v2);
                    }
                }
            }
        }

        // Fall back to V1 restore
        if !snapshot.verify() {
            return Err("Snapshot verification failed".into());
        }
        if snapshot.height < self.finalized_height {
            return Err(format!(
                "Snapshot height {} is older than current finalized height {}",
                snapshot.height, self.finalized_height
            ));
        }
        if snapshot.chain_id != self.chain_id {
            return Err(format!(
                "Snapshot chain_id mismatch: expected {}, got {}",
                self.chain_id, snapshot.chain_id
            ));
        }
        if snapshot.finalized_height > snapshot.height {
            return Err("Snapshot finalized height exceeds snapshot height".into());
        }

        let Some(block) = self.chain.get(snapshot.height as usize) else {
            return Err(format!(
                "Snapshot height {} is not available in local chain",
                snapshot.height
            ));
        };
        if block.hash != snapshot.block_hash {
            return Err(format!(
                "Snapshot block hash mismatch at height {}",
                snapshot.height
            ));
        }

        let finalized_block = self
            .chain
            .get(snapshot.finalized_height as usize)
            .ok_or_else(|| {
                format!(
                    "Snapshot finalized height {} is not available in local chain",
                    snapshot.finalized_height
                )
            })?;
        if finalized_block.hash != snapshot.finalized_hash {
            return Err(format!(
                "Snapshot finalized hash mismatch at height {}",
                snapshot.finalized_height
            ));
        }

        let mut snapshot_state = AccountState::from_snapshot(&snapshot);
        let snapshot_state_root = snapshot_state.calculate_state_root();
        if !block.state_root.is_empty() && snapshot_state_root != block.state_root {
            return Err(format!(
                "Snapshot state root mismatch: expected {}, got {}",
                block.state_root, snapshot_state_root
            ));
        }

        self.state = snapshot_state;
        self.finalized_height = snapshot.finalized_height;
        self.finalized_hash = snapshot.finalized_hash;
        self.mempool.set_min_fee(self.state.base_fee);

        Ok(())
    }

    fn apply_v2_snapshot(
        &mut self,
        v2: &crate::chain::snapshot::StateSnapshotV2,
    ) -> Result<(), String> {
        if v2.height < self.finalized_height {
            return Err(format!(
                "V2 snapshot height {} older than finalized {}",
                v2.height, self.finalized_height
            ));
        }

        let Some(block) = self.chain.get(v2.height as usize) else {
            return Err("V2 snapshot height not in local chain".into());
        };
        if block.hash != v2.block_hash {
            return Err("V2 snapshot block hash mismatch".into());
        }

        let mut v2_state = AccountState::from_snapshot_v2(v2);
        let state_root = v2_state.calculate_state_root();
        if !block.state_root.is_empty() && state_root != block.state_root {
            return Err(format!(
                "V2 snapshot state root mismatch: expected {}, got {}",
                block.state_root, state_root
            ));
        }

        self.state = v2_state;
        self.finalized_height = v2.finalized_height;
        self.finalized_hash = v2.finalized_hash.clone();
        self.mempool.set_min_fee(self.state.base_fee);

        // Restore finality certs from V2 snapshot
        for cert in &v2.finality_certificates {
            self.pending_finality_certs
                .entry(cert.checkpoint_height)
                .or_default()
                .push(cert.clone());
        }

        info!(
            "Applied V2 snapshot at height {} (epoch={}, base_fee={}, certs={})",
            v2.height,
            v2.epoch_index,
            v2.base_fee,
            v2.finality_certificates.len()
        );
        Ok(())
    }

    fn process_pending_finality_certs(&mut self, checkpoint_height: u64) -> Result<(), String> {
        let Some(certs) = self.pending_finality_certs.remove(&checkpoint_height) else {
            return Ok(());
        };

        let mut last_err = None;
        for cert in certs {
            if let Err(e) = self.handle_finality_cert(cert.clone()) {
                if e.contains("Missing verified QC blob") {
                    self.pending_finality_certs
                        .entry(checkpoint_height)
                        .or_default()
                        .push(cert);
                }
                last_err = Some(e);
            }
        }

        if let Some(e) = last_err {
            return Err(e);
        }
        Ok(())
    }

    pub fn handle_finality_cert(&mut self, cert: FinalityCert) -> Result<(), String> {
        if cert.checkpoint_height <= self.finalized_height {
            return Ok(());
        }

        if !crate::chain::finality::is_checkpoint_height(cert.checkpoint_height) {
            return Err(format!(
                "Height {} is not a valid checkpoint height",
                cert.checkpoint_height
            ));
        }

        if let Some(block) = self.chain.get(cert.checkpoint_height as usize) {
            if block.hash != cert.checkpoint_hash {
                return Err(format!(
                    "Certificate hash {} mismatch with our block hash {} at height {}",
                    cert.checkpoint_hash, block.hash, cert.checkpoint_height
                ));
            }
        } else {
            return Err(format!(
                "We don't have block at height {} yet",
                cert.checkpoint_height
            ));
        }

        let snapshot = self.validator_snapshot_for_epoch(cert.epoch);

        cert.verify(&snapshot)?;

        let blob = match self.get_qc_blob(cert.checkpoint_height) {
            Some(blob) => blob,
            None => {
                self.pending_finality_certs
                    .entry(cert.checkpoint_height)
                    .or_default()
                    .push(cert.clone());
                return Err(format!(
                    "Missing verified QC blob for checkpoint {}",
                    cert.checkpoint_height
                ));
            }
        };
        let signer_indices = cert.signer_indices(snapshot.validators.len());
        blob.verify_against_snapshot(
            &snapshot,
            Some(&signer_indices),
            Some(self.state.epoch_index),
        )?;
        self.maybe_apply_detected_qc_faults(&snapshot, &blob)?;

        self.finalized_height = cert.checkpoint_height;
        self.finalized_hash = cert.checkpoint_hash.clone();

        info!(
            "FINALIZED checkpoint: height={}, hash={}",
            self.finalized_height, self.finalized_hash
        );

        if let Some(ref store) = self.storage {
            let _ = store.save_finality_cert(self.finalized_height, &cert);
            let _ = store.save_canonical_height(self.finalized_height);
        }

        Ok(())
    }

    pub fn handle_prevote(&mut self, vote: Prevote) -> Result<(), String> {
        let aggregator = self
            .finality_aggregator
            .as_mut()
            .ok_or("No active finality aggregator")?;
        aggregator.add_prevote(vote).map_err(|e| e.to_string())
    }

    pub fn handle_precommit(&mut self, vote: Precommit) -> Result<Option<FinalityCert>, String> {
        let aggregator = self
            .finality_aggregator
            .as_mut()
            .ok_or("No active finality aggregator")?;
        aggregator.add_precommit(vote).map_err(|e| e.to_string())?;

        if aggregator.precommit_quorum_reached {
            let cert = aggregator.try_produce_cert();
            if let Some(cert) = &cert {
                info!(
                    "FinalityCert produced: epoch={}, height={}",
                    cert.epoch, cert.checkpoint_height
                );
                self.finality_aggregator = None;
            }
            Ok(cert)
        } else {
            Ok(None)
        }
    }

    pub fn sign_prevote(
        &self,
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: &str,
        voter_id: &Address,
    ) -> Result<Prevote, String> {
        let sk = self
            .consensus
            .bls_secret_key()
            .ok_or("No BLS secret key available")?;

        let msg = {
            let dummy = Prevote {
                epoch,
                checkpoint_height,
                checkpoint_hash: checkpoint_hash.to_string(),
                voter_id: *voter_id,
                sig_bls: vec![],
            };
            dummy.signing_message()
        };

        let sig = crate::chain::finality::sign_bls(&sk, &msg);

        Ok(Prevote {
            epoch,
            checkpoint_height,
            checkpoint_hash: checkpoint_hash.to_string(),
            voter_id: *voter_id,
            sig_bls: sig,
        })
    }

    pub fn sign_precommit(
        &self,
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: &str,
        voter_id: &Address,
    ) -> Result<Precommit, String> {
        let sk = self
            .consensus
            .bls_secret_key()
            .ok_or("No BLS secret key available")?;

        let msg = crate::chain::finality::checkpoint_signing_message(
            epoch,
            checkpoint_height,
            checkpoint_hash,
        );

        let sig = crate::chain::finality::sign_bls(&sk, &msg);

        Ok(Precommit {
            epoch,
            checkpoint_height,
            checkpoint_hash: checkpoint_hash.to_string(),
            voter_id: *voter_id,
            sig_bls: sig,
        })
    }

    pub fn start_prevote_phase(&mut self, checkpoint_height: u64, checkpoint_hash: String) {
        let epoch = checkpoint_height / crate::core::chain_config::EPOCH_LEN;
        let mut aggregator = FinalityAggregator::new(epoch, checkpoint_height, checkpoint_hash);
        let snapshot = self.validator_snapshot_for_epoch(epoch);
        aggregator.set_validator_snapshot(snapshot);
        self.finality_aggregator = Some(aggregator);
        info!(
            "Started prevote phase for checkpoint height={} (epoch={})",
            checkpoint_height, epoch
        );
    }

    pub fn get_aggregator_state(&self) -> crate::chain::finality::AggregatorState {
        self.finality_aggregator
            .as_ref()
            .map(|agg| agg.get_state())
            .unwrap_or_else(crate::chain::finality::AggregatorState::inactive)
    }

    pub fn consensus(&self) -> &dyn ConsensusEngine {
        self.consensus.as_ref()
    }
}

impl Clone for Blockchain {
    fn clone(&self) -> Self {
        Blockchain {
            chain: self.chain.clone(),
            consensus: Arc::clone(&self.consensus),
            mempool: Mempool::default(),
            storage: self.storage.clone(),
            state: self.state.clone(),
            chain_id: self.chain_id,
            pruning_manager: self.pruning_manager.clone(),
            finalized_height: self.finalized_height,
            finalized_hash: self.finalized_hash.clone(),
            genesis_time: self.genesis_time,
            verified_qc_blobs: self.verified_qc_blobs.clone(),
            validator_snapshots: self.validator_snapshots.clone(),
            pending_finality_certs: self.pending_finality_certs.clone(),
            domain_registry: self.domain_registry.clone(),
            domain_commitment_registry: self.domain_commitment_registry.clone(),
            bridge_state: self.bridge_state.clone(),
            global_headers: self.global_headers.clone(),
            plugin_registry: DomainPluginRegistry::new(),
            message_registry: self.message_registry.clone(),
            settlement_finality_hashes: self.settlement_finality_hashes.clone(),
            pending_slashing_evidence: self.pending_slashing_evidence.clone(),
            finality_aggregator: None,
            metrics: self.metrics.clone(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::poa::{PoAConfig, PoAEngine};
    use crate::consensus::PoWEngine;
    use crate::crypto::primitives::KeyPair;
    use crate::storage::db::Storage;
    use tempfile::tempdir;

    #[test]
    fn test_blockchain_with_pow() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        let keypair = KeyPair::generate().unwrap();
        let pubkey = Address::from(keypair.public_key_bytes());

        blockchain.state.add_balance(&pubkey, 100);

        let recipient = Address::from([1u8; 32]);
        let mut tx = Transaction::new(pubkey, recipient, 50, vec![]);
        tx.fee = 1;
        tx.sign(&keypair);

        blockchain.add_transaction(tx).unwrap();

        blockchain.produce_block(Address::zero());
        assert!(blockchain.is_valid());
        assert_eq!(blockchain.chain.len(), 2);
    }

    #[test]
    fn test_epoch_transition_and_unjailing() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        let mut val_bytes = [0u8; 32];
        val_bytes[0] = 1;
        let validator_addr = Address::from(val_bytes);
        blockchain.state.add_validator(validator_addr, 1000);

        if let Some(v) = blockchain.state.get_validator_mut(&validator_addr) {
            v.jailed = true;
            v.active = false;
            v.jail_until = 0;
        }

        assert_eq!(blockchain.state.epoch_index, 0);
        if let Some(v) = blockchain.state.get_validator(&validator_addr) {
            assert!(v.jailed);
        }

        for _ in 0..EPOCH_LENGTH {
            blockchain.produce_block(Address::zero());
        }

        assert_eq!(blockchain.chain.len(), (EPOCH_LENGTH as usize) + 1);

        assert_eq!(blockchain.state.epoch_index, 1);

        if let Some(v) = blockchain.state.get_validator(&validator_addr) {
            assert!(!v.jailed, "Validator should have been unjailed");
            assert!(v.active, "Validator should be active");
        } else {
            panic!("Validator not found");
        }
    }

    #[test]
    fn test_slashing_execution() {
        use crate::consensus::pos::{PoSConfig, SlashingEvidence};
        use crate::consensus::PoSEngine;
        use crate::core::block::BlockHeader;

        let alice_keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let alice_key = alice_keys.sig_key.clone();
        let alice_vrf_pub = alice_keys.vrf_key.public.to_bytes().to_vec();
        let alice_pub = Address::from(alice_key.public_key_bytes());

        let config = PoSConfig {
            slashing_penalty: (50 * crate::core::chain_config::FIXED_POINT_SCALE) / 100,
            ..Default::default()
        };

        let engine = Arc::new(PoSEngine::new(config.clone(), Some(alice_keys.clone())));

        let mut blockchain = Blockchain::new(engine.clone(), None, 1337, None);

        blockchain.state.validators.clear();
        blockchain.state.add_validator(alice_pub, 2000);
        if let Some(v) = blockchain.state.get_validator_mut(&alice_pub) {
            v.vrf_public_key = alice_vrf_pub.clone();
        }
        blockchain.state.add_balance(&alice_pub, 100);

        let mut real_b1 = Block::new(10, "prev".into(), vec![]);
        real_b1.producer = Some(alice_pub);
        real_b1.hash = real_b1.calculate_hash();
        let sig1 = alice_key.sign(&real_b1.calculate_hash_bytes()).to_vec();
        real_b1.signature = Some(sig1.clone());
        let h1 = BlockHeader::from_block(&real_b1);

        let mut real_b2 = Block::new(10, "prev".into(), vec![]);
        real_b2.timestamp += 1;
        real_b2.producer = Some(alice_pub);
        real_b2.hash = real_b2.calculate_hash();
        let sig2 = alice_key.sign(&real_b2.calculate_hash_bytes()).to_vec();
        real_b2.signature = Some(sig2.clone());
        let h2 = BlockHeader::from_block(&real_b2);

        let evidence = SlashingEvidence::new(h1, h2, sig1, sig2);

        {
            let mut guard = engine.slashing_evidence.write().unwrap();
            guard.push(evidence);
        }

        blockchain.produce_block(alice_pub);

        let produced_block = blockchain.chain.last().unwrap();
        assert!(
            produced_block.slashing_evidence.is_some(),
            "Block should contain slashing evidence"
        );
        assert_eq!(produced_block.slashing_evidence.as_ref().unwrap().len(), 1);

        let fresh_engine = Arc::new(PoSEngine::new(config, Some(alice_keys)));
        let mut blockchain2 = Blockchain::new(fresh_engine, None, 1337, None);
        blockchain2.state.validators.clear();
        blockchain2.state.add_validator(alice_pub, 2000);
        if let Some(v) = blockchain2.state.get_validator_mut(&alice_pub) {
            v.vrf_public_key = alice_vrf_pub.clone();
        }
        blockchain2.state.add_balance(&alice_pub, 100);
        blockchain2
            .validate_and_add_block(produced_block.clone())
            .unwrap();

        let validator = blockchain2.state.get_validator(&alice_pub).unwrap();
        assert!(validator.slashed, "Validator should be slashed");
        assert!(!validator.active);
        assert!(validator.stake < 2000);
    }

    #[test]
    fn test_fee_reaches_producer() {
        let consensus = Arc::new(PoWEngine::new(0));
        let sender = KeyPair::generate().unwrap();
        let sender_pub = Address::from(sender.public_key_bytes());
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state.add_balance(&sender_pub, 1000);

        let recipient = Address::from([2u8; 32]);
        let mut tx = Transaction::new_with_fee(sender_pub, recipient, 100, 5, 0, vec![]);
        tx.sign(&sender);
        bc.add_transaction(tx).unwrap();

        let mut miner_bytes = [0u8; 32];
        miner_bytes[0] = 1;
        let miner_addr = Address::from(miner_bytes);
        bc.produce_block(miner_addr);
        assert_eq!(bc.state.get_balance(&miner_addr), 55);
    }

    #[test]
    fn test_fee_reaches_actual_poa_signer() {
        let signer = KeyPair::generate().unwrap();
        let signer_addr = Address::from(signer.public_key_bytes());
        let consensus = Arc::new(PoAEngine::new(PoAConfig::default(), Some(signer)));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state.add_validator(signer_addr, 1);

        bc.produce_block(Address::zero()).unwrap();

        assert_eq!(bc.state.get_balance(&signer_addr), bc.state.block_reward);
        assert_eq!(bc.state.get_balance(&Address::zero()), 0);
    }

    #[test]
    fn test_accepts_queued_sender_nonces() {
        let consensus = Arc::new(PoWEngine::new(0));
        let sender = KeyPair::generate().unwrap();
        let sender_pub = Address::from(sender.public_key_bytes());
        let recipient = Address::from([7u8; 32]);
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state.add_balance(&sender_pub, 1_000);

        let mut tx0 = Transaction::new_with_fee(sender_pub, recipient, 10, 1, 0, vec![]);
        tx0.sign(&sender);
        bc.add_transaction(tx0).unwrap();

        let mut tx1 = Transaction::new_with_fee(sender_pub, recipient, 15, 2, 1, vec![]);
        tx1.sign(&sender);
        bc.add_transaction(tx1).unwrap();

        let block = bc.produce_block(Address::from([9u8; 32])).unwrap();
        assert_eq!(block.transactions.len(), 2);
        assert_eq!(bc.state.get_nonce(&sender_pub), 2);
        assert_eq!(bc.state.get_balance(&recipient), 25);
    }

    #[test]
    fn test_restart_replays_epoch_state() {
        let tmp = tempdir().unwrap();
        let db_path = tmp.path().join("budlum.db");
        let db_path = db_path.to_string_lossy().to_string();
        let storage = Storage::new(&db_path).unwrap();
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, Some(storage), 1337, None);

        for _ in 0..EPOCH_LENGTH {
            bc.produce_block(Address::from([3u8; 32])).unwrap();
        }

        assert_eq!(bc.state.epoch_index, 1);
        let expected_height = bc.last_block().index;
        drop(bc);

        let restarted = Blockchain::new(
            Arc::new(PoWEngine::new(0)),
            Some(Storage::new(&db_path).unwrap()),
            1337,
            None,
        );

        assert_eq!(restarted.state.epoch_index, 1);
        assert_eq!(restarted.last_block().index, expected_height);
    }

    #[test]
    fn test_validate_rejects_empty_state_root() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);

        let mut block = Block::new(1, bc.last_block().hash.clone(), vec![]);
        block.chain_id = 1337;
        block.state_root = String::new();
        block.hash = block.calculate_hash();

        let result = bc.validate_and_add_block(block);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("state_root"));
    }

    #[test]
    fn test_validate_rejects_finalized_conflict() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);

        bc.finalized_height = 0;
        bc.finalized_hash = bc.chain[0].hash.clone();

        let mut bad_block = bc.chain[0].clone();
        bad_block.previous_hash = "wrong".to_string();
        bad_block.hash = bad_block.calculate_hash();

        let result = bc.validate_and_add_block(bad_block);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("conflicts with finalized"));
    }

    #[test]
    fn test_validate_rejects_tampered_tx_root() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);

        let mut block = Block::new(1, bc.last_block().hash.clone(), vec![]);
        block.chain_id = 1337;
        block.state_root = "a".repeat(64);
        block.tx_root = "b".repeat(64);
        block.hash = block.calculate_hash();

        let result = bc.validate_and_add_block(block);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("tx_root"));
    }

    #[test]
    fn test_validate_rejects_tampered_hash() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);

        let mut block = Block::new(1, bc.last_block().hash.clone(), vec![]);
        block.chain_id = 1337;
        block.state_root = "a".repeat(64);
        block.hash = "c".repeat(64);

        let result = bc.validate_and_add_block(block);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("hash"));
    }

    #[test]
    fn test_historical_snapshot_rebuilds_state_at_height() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        let late_account = Address::from([8u8; 32]);
        bc.state.add_balance(&late_account, 123);

        let snapshot = bc.get_state_snapshot(0).unwrap();

        assert_eq!(snapshot.height, 0);
        assert_eq!(snapshot.balances.get(&late_account), None);
        assert_eq!(snapshot.block_hash, bc.chain[0].hash);
    }

    #[test]
    fn test_apply_snapshot_rejects_wrong_chain_id() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        let snapshot = crate::chain::snapshot::StateSnapshot::from_state(
            0,
            bc.chain[0].hash.clone(),
            42,
            &bc.state,
            0,
            bc.chain[0].hash.clone(),
        );

        let result = bc.apply_state_snapshot(snapshot);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("chain_id"));
    }

    #[test]
    fn test_apply_snapshot_rejects_unknown_block_hash() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        let snapshot = crate::chain::snapshot::StateSnapshot::from_state(
            0,
            "bad_hash".to_string(),
            1337,
            &bc.state,
            0,
            bc.chain[0].hash.clone(),
        );

        let result = bc.apply_state_snapshot(snapshot);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("block hash"));
    }

    #[test]
    fn test_apply_snapshot_accepts_local_verified_snapshot() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        let snapshot = bc.get_state_snapshot(0).unwrap();
        bc.state.add_balance(&Address::from([9u8; 32]), 500);

        bc.apply_state_snapshot(snapshot).unwrap();

        assert_eq!(bc.state.get_balance(&Address::from([9u8; 32])), 0);
    }

    #[test]
    fn test_start_prevote_phase_creates_aggregator() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        let cp_height = crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
        let cp_hash = "a".repeat(64);

        bc.start_prevote_phase(cp_height, cp_hash.clone());

        assert!(bc.finality_aggregator.is_some());
        let agg = bc.finality_aggregator.as_ref().unwrap();
        assert_eq!(agg.checkpoint_height, cp_height);
        assert_eq!(agg.checkpoint_hash, cp_hash);
        assert!(!agg.prevote_quorum_reached);
        assert!(!agg.precommit_quorum_reached);
    }

    #[test]
    fn test_handle_prevote_rejects_when_no_aggregator() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);

        let vote = Prevote {
            epoch: 0,
            checkpoint_height: 10,
            checkpoint_hash: "hash".to_string(),
            voter_id: Address::from([1u8; 32]),
            sig_bls: vec![],
        };
        let result = bc.handle_prevote(vote);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("No active finality aggregator"));
    }

    #[test]
    fn test_handle_precommit_rejects_when_no_aggregator() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);

        let vote = Precommit {
            epoch: 0,
            checkpoint_height: 10,
            checkpoint_hash: "hash".to_string(),
            voter_id: Address::from([1u8; 32]),
            sig_bls: vec![],
        };
        let result = bc.handle_precommit(vote);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("No active finality aggregator"));
    }

    #[test]
    fn test_produce_block_advances_to_checkpoint() {
        let consensus = Arc::new(PoWEngine::new(0));
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state.add_balance(&Address::from([1u8; 32]), 1000);

        let cp_height = crate::core::chain_config::FINALITY_CHECKPOINT_INTERVAL;
        while bc.chain.len() < cp_height as usize {
            bc.produce_block(Address::from([1u8; 32]));
        }

        assert_eq!(bc.chain.len() as u64, cp_height);
        assert_eq!(bc.chain.last().unwrap().index, cp_height - 1);
    }
}
