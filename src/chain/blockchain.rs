use crate::chain::finality::{FinalityCert, ValidatorEntry, ValidatorSetSnapshot};
use crate::chain::genesis::{GenesisConfig, GENESIS_TIMESTAMP};
use crate::chain::snapshot::PruningManager;
use crate::consensus::qc::{QcBlob, QcFaultProof, QcProofAction, QcProofVerdict};
use crate::consensus::ConsensusEngine;
use crate::core::account::AccountState;
use crate::core::address::Address;
use crate::core::block::Block;
use crate::core::chain_config::Network;
use crate::core::transaction::Transaction;
use crate::cross_domain::{
    BridgeState, CrossDomainMessageRegistry, DomainEvent, DomainEventKind, MerkleProof, MessageKind,
};
use crate::domain::{
    hash_finality_proof, BftFinalityAdapter, ConsensusDomain, ConsensusDomainRegistry,
    ConsensusKind, DomainCommitment, DomainCommitmentRegistry, DomainFinalityAdapter,
    DomainPluginRegistry, FinalityProof, FinalityStatus, PoAFinalityAdapter, PoSFinalityAdapter,
    PoWFinalityAdapter, ZkFinalityAdapter,
};
use crate::execution::executor::Executor;
use crate::mempool::pool::{Mempool, MempoolConfig};
use crate::settlement::{
    merkle_root, GlobalBlockHeader, ProofVerificationError, SettlementProofVerifier,
    VerifiedDomainEvent,
};
use crate::storage::db::Storage;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::info;

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
}
impl Blockchain {
    pub fn new(
        consensus: Arc<dyn ConsensusEngine>,
        storage: Option<Storage>,
        chain_id: u64,
        pruning_manager: Option<PruningManager>,
    ) -> Self {
        println!("Consensus: {}", consensus.info());
        let mut chain_vec = Vec::new();
        let mut state = AccountState::new();

        let mut loaded_chain = false;
        if let Some(ref store) = storage {
            if let Ok(c) = store.load_chain() {
                if !c.is_empty() {
                    chain_vec = c;
                    loaded_chain = true;
                    println!("Loaded chain from DB: {} blocks", chain_vec.len());
                }
            }
        }

        if !loaded_chain {
            let genesis_config = Network::from_chain_id(chain_id)
                .map(GenesisConfig::for_network)
                .unwrap_or_else(|| GenesisConfig::new(chain_id));
            let genesis = genesis_config.build_genesis_block();
            chain_vec.push(genesis);
        }

        let mut snapshot_height = 0;
        let mut restored_finalized_height = 0;
        let mut restored_finalized_hash = chain_vec[0].hash.clone();

        if let Some(ref pm) = pruning_manager {
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
                    println!(
                        "Restored state from snapshot at height {} (finalized={})",
                        snapshot_height, restored_finalized_height
                    );
                } else {
                    println!(
                        " Snapshot chain_id mismatch (expected {}, got {}). Ignoring.",
                        chain_id, snapshot.chain_id
                    );
                }
            }
        }

        let chain_len = chain_vec.len();
        let start_index = if snapshot_height > 0 && snapshot_height < chain_len as u64 {
            (snapshot_height + 1) as usize
        } else {
            if snapshot_height >= chain_len as u64 {
                println!(" Chain shorter than snapshot height! Replaying from Genesis.");
                0
            } else {
                0
            }
        };

        println!(
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
                    println!("CRITICAL: Failed to apply block {} during init: {}. Corrupted database, exiting.", block.index, e);
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
            .unwrap_or_else(MempoolConfig::default);
        let mut mempool = Mempool::new(mempool_config);
        if let Some(ref store) = storage {
            if let Ok(txs) = store.load_mempool_txs() {
                let count = txs.len();
                for tx in txs {
                    let _ = mempool.add_transaction(tx);
                }
                if count > 0 {
                    println!("Restored {} transactions from mempool persistence", count);
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
                    if let Err(e) = domain_registry.register(domain) {
                        println!("Skipping duplicate stored consensus domain: {}", e);
                    }
                }
            }

            if let Ok(commitments) = store.load_domain_commitments() {
                for commitment in commitments {
                    if let Err(e) = domain_commitment_registry.insert(commitment) {
                        println!("Skipping duplicate stored domain commitment: {}", e);
                    }
                }
            }

            if let Ok(Some(stored_bridge_state)) = store.load_bridge_state() {
                bridge_state = stored_bridge_state;
            }

            if let Ok(stored_global_headers) = store.load_global_headers() {
                global_headers = stored_global_headers;
            }
            
            if let Ok(messages) = store.load_cross_domain_messages() {
                let mut registry = CrossDomainMessageRegistry::new();
                for msg in messages {
                    if let Err(e) = registry.insert(msg) {
                        println!("Skipping duplicate cross domain message: {}", e);
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
        };

        if let Some(first) = bc.chain.first() {
            bc.genesis_time = first.timestamp;
        } else {
            bc.genesis_time = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis();
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
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                "Chain broken or empty",
            ));
        }
        blocks.reverse();
        self.chain = blocks;
        println!("Loaded {} blocks from disk", self.chain.len());
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
        self.domain_registry.register(domain.clone())?;
        if let Some(store) = &self.storage {
            store
                .save_consensus_domain(&domain)
                .map_err(|e| format!("Failed to persist consensus domain: {}", e))?;
        }
        Ok(())
    }

    pub fn submit_domain_commitment(&mut self, commitment: DomainCommitment) -> Result<(), String> {
        self.validate_domain_commitment_metadata(&commitment)?;

        self.domain_commitment_registry.insert(commitment.clone())?;
        if let Some(store) = &self.storage {
            store
                .save_domain_commitment(&commitment)
                .map_err(|e| format!("Failed to persist domain commitment: {}", e))?;
        }
        Ok(())
    }

    pub fn submit_verified_domain_commitment(
        &mut self,
        commitment: DomainCommitment,
        proof: FinalityProof,
    ) -> Result<(), String> {
        self.verify_domain_commitment_finality(&commitment, &proof)?;
        self.submit_domain_commitment(commitment)
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
            timestamp_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_millis(),
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

        let message = verified
            .event
            .message
            .ok_or_else(|| "Verified bridge lock event is missing message".to_string())?;

        if message.kind != MessageKind::BridgeLock {
            return Err("Verified event message is not a bridge lock message".into());
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

    pub fn burn_bridge_transfer(
        &mut self,
        message_id: crate::cross_domain::MessageId,
        domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        self.bridge_state
            .burn(message_id, domain)
            .map_err(|e| e.to_string())?;
        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        Ok(())
    }

    pub fn unlock_bridge_transfer(
        &mut self,
        message_id: crate::cross_domain::MessageId,
        source_domain: crate::domain::DomainId,
    ) -> Result<(), String> {
        self.bridge_state
            .unlock(message_id, source_domain)
            .map_err(|e| e.to_string())?;
        if let Some(store) = &self.storage {
            store
                .save_bridge_state(&self.bridge_state)
                .map_err(|e| format!("Failed to persist bridge state: {}", e))?;
        }
        Ok(())
    }
    pub fn get_transaction_by_hash(&self, hash: &str) -> Option<Transaction> {
        if let Some(ref store) = self.storage {
            if let Ok(Some(height)) = store.get_tx_block_height(hash) {
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
            if let Ok(Some(height)) = store.get_tx_block_height(hash) {
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
                if tx.data.is_empty() || tx.data.len() % 8 != 0 {
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
                if Executor::apply_transaction(&mut temp_state, tx).is_ok() {
                    valid_txs.push(tx.clone());
                    included.insert(tx.hash.clone());
                    progress = true;
                }
            }
        }

        for tx in &pending_txs {
            if !included.contains(&tx.hash) && self.state.validate_transaction(tx).is_err() {
                println!("Discarding invalid transaction: {}", tx.hash);
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

        if block.index > 0 && block.index % EPOCH_LENGTH == 0 {
            state.advance_epoch(block.timestamp);
        }

        if block.index > 0 {
            Self::adjust_base_fee(state, block.transactions.len());
        }
    }

    fn apply_block_effects(
        base_state: &AccountState,
        block: &Block,
    ) -> Result<AccountState, String> {
        let mut next_state = base_state.clone();
        Executor::apply_block(
            &mut next_state,
            &block.transactions,
            block.producer.as_ref(),
        )
        .map_err(|e| format!("Failed to apply block: {}", e))?;
        Self::apply_system_effects(&mut next_state, block);
        Ok(next_state)
    }

    pub fn produce_block(&mut self, producer_address: Address) -> Option<Block> {
        let index = self.chain.len() as u64;
        let previous_hash = self.chain.last().unwrap().hash.clone();
        let valid_txs = self.collect_block_transactions();
        let mut block = Block::new(index, previous_hash, valid_txs);
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
        block.state_root = committed_state.calculate_state_root();

        if let Err(_e) = self.consensus.prepare_block(&mut block, &self.state) {
            return None;
        }

        self.state = committed_state;
        self.record_validator_snapshot(self.state.epoch_index);

        if let Some(ref store) = self.storage {
            let _ = store.commit_block(&block, &block.state_root);
        }

        self.chain.push(block.clone());

        if let Some(last_block) = self.chain.last() {
            if let Err(e) = self
                .consensus
                .record_block(last_block, self.storage.as_ref())
            {
                println!("Engine record block error: {}", e);
            }
        }

        for tx in &block.transactions {
            self.mempool.remove_transaction(&tx.hash);
            if let Some(ref store) = self.storage {
                let _ = store.remove_mempool_tx(&tx.hash);
            }
        }

        self.mempool.set_min_fee(self.state.base_fee);
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
        self.state.add_balance(address, 1_000_000_000);
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
            if let Err(e) = Executor::apply_transaction(&mut temp_state, tx) {
                return Err(format!("Failed to apply transaction at index {}: {}", i, e));
            }
        }

        let mut commit_state = Self::apply_block_effects(&self.state, &block)?;

        if block.index > 0 {
            let computed_root = commit_state.calculate_state_root();
            if computed_root != block.state_root {
                return Err(format!(
                    "State root mismatch: expected {}, got {}",
                    block.state_root, computed_root
                ));
            }
        }

        if let Some(ref store) = self.storage {
            let _ = store.commit_block(&block, &block.state_root);
        }

        self.state = commit_state;
        self.record_validator_snapshot(self.state.epoch_index);
        self.mempool.set_min_fee(self.state.base_fee);

        self.chain.push(block);

        if let Some(last_block) = self.chain.last() {
            if let Err(e) = self
                .consensus
                .record_block(last_block, self.storage.as_ref())
            {
                println!("Engine record block error: {}", e);
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
                let snapshot = crate::chain::snapshot::StateSnapshot::from_state(
                    height,
                    last_block.hash.clone(),
                    self.chain_id,
                    &self.state,
                    self.finalized_height,
                    self.finalized_hash.clone(),
                );
                if let Err(e) = pruning_manager.save_snapshot(&snapshot) {
                    println!("Failed to save snapshot at height {}: {}", height, e);
                } else {
                    println!("Saved state snapshot at height {}", height);

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
                            println!("Pruned {} old blocks from disk", prunable.len());
                        }
                    }
                }
            }
        }

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
                println!("Block {} validation failed: {}", i, e);
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
            if let Err(_) = self
                .consensus
                .validate_block(block, previous_chain, &dummy_state)
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

        println!(
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
            for block in &self.chain[fork_point..] {
                let _ = store.commit_block(block, &block.state_root);
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
        let mut state = AccountState::new();
        for block in chain.iter() {
            state = Self::apply_block_effects(&state, block)
                .map_err(|e| format!("Failed to rebuild state at block {}: {}", block.index, e))?;
        }
        Ok(state)
    }
    pub fn print_info(&self) {
        println!("================================");
        println!("Blockchain Info");
        println!("================================");
        println!("Consensus: {}", self.consensus.info());
        println!("Length: {}", self.chain.len());
        println!("Pending Tx: {}", self.mempool.len());
        println!("================================");
        for block in &self.chain {
            println!(" Block #{}: {}", block.index, &block.hash[..16]);
        }
    }
    pub fn get_state_snapshot(&self, height: u64) -> Option<crate::chain::snapshot::StateSnapshot> {
        if height >= self.chain.len() as u64 {
            return None;
        }
        let block = &self.chain[height as usize];
        Some(crate::chain::snapshot::StateSnapshot::from_state(
            height,
            block.hash.clone(),
            self.chain_id,
            &self.state,
            self.finalized_height,
            self.finalized_hash.clone(),
        ))
    }

    pub fn apply_state_snapshot(
        &mut self,
        snapshot: crate::chain::snapshot::StateSnapshot,
    ) -> Result<(), String> {
        if !snapshot.verify() {
            return Err("Snapshot verification failed".into());
        }
        self.state = AccountState::from_snapshot(&snapshot);
        self.finalized_height = snapshot.finalized_height;
        self.finalized_hash = snapshot.finalized_hash;
        self.mempool.set_min_fee(self.state.base_fee);

        if self.chain.len() < snapshot.height as usize + 1 {
            let mut stubs = Vec::new();
            let start = self.chain.len();
            for i in start..=snapshot.height as usize {
                let mut stub = Block::new(i as u64, "stub".into(), vec![]);
                if i == snapshot.height as usize {
                    stub.hash = snapshot.block_hash.clone();
                } else {
                    stub.hash = format!("stub_{}", i);
                }
                stubs.push(stub);
            }
            self.chain.extend(stubs);
        }

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

        let mut config = PoSConfig::default();
        config.slashing_penalty = (50 * crate::core::chain_config::FIXED_POINT_SCALE) / 100;

        let engine = Arc::new(PoSEngine::new(config.clone(), Some(alice_keys.clone())));

        let mut blockchain = Blockchain::new(engine.clone(), None, 1337, None);

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
        blockchain2.state.add_validator(alice_pub.clone(), 2000);
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
}
