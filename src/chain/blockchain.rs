use crate::core::account::AccountState;
use crate::chain::finality::{ValidatorEntry, ValidatorSetSnapshot};
use crate::consensus::ConsensusEngine;
use crate::chain::genesis::{GenesisConfig, GENESIS_TIMESTAMP};
use crate::mempool::pool::{Mempool, MempoolConfig};
use crate::chain::snapshot::PruningManager;
use crate::storage::db::Storage;
use crate::core::block::Block;
use crate::core::transaction::Transaction;
use crate::execution::executor::Executor;
use std::sync::Arc;
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
            let genesis = GenesisConfig::new(chain_id).build_genesis_block();
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

        for block in chain_vec.iter().skip(start_index) {
            if let Err(e) = Executor::apply_block(&mut state, &block.transactions, block.producer.as_deref()) {
                println!("CRITICAL: Failed to apply block {} during init: {}. Corrupted database, exiting.", block.index, e);
                std::process::exit(1);
            }
        }

        Blockchain {
            chain: chain_vec,
            consensus,
            mempool: Mempool::new(MempoolConfig::default()),
            storage,
            state,
            chain_id,
            pruning_manager,
            finalized_height: restored_finalized_height,
            finalized_hash: restored_finalized_hash,
        }
    }

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
        Ok(())
    }
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

    pub fn get_validator_set_hash(&self) -> String {
        let active_validators = self.state.get_active_validators();
        let entries: Vec<ValidatorEntry> = active_validators
            .into_iter()
            .map(|v| ValidatorEntry {
                address: v.address.clone(),
                stake: v.stake,
                bls_public_key: Vec::new(),
                pop_signature: Vec::new(),
            })
            .collect();
        ValidatorSetSnapshot::compute_hash(&entries)
    }
    pub fn produce_block(&mut self, producer_address: String) {
        let index = self.chain.len() as u64;
        let previous_hash = self.chain.last().unwrap().hash.clone();

        let mut valid_txs = Vec::new();
        let mut temp_state = self.state.clone();

        let pending_txs = self.mempool.get_sorted_transactions(1000);
        for tx in &pending_txs {
            if let Ok(_) = temp_state.validate_transaction(tx) {
                if let Ok(_) = Executor::apply_transaction(&mut temp_state, tx) {
                    valid_txs.push(tx.clone());
                }
            } else {
                println!("Discarding invalid transaction: {}", tx.hash);
            }
        }

        let mut block = Block::new(index, previous_hash, valid_txs);
        println!(
            "Producing block {} with {} ({} txs)...",
            index,
            self.consensus.consensus_type(),
            block.transactions.len()
        );

        block.producer = Some(producer_address.clone());

        let mut state_for_root = self.state.clone();
        if let Err(e) = Executor::apply_block(&mut state_for_root, &block.transactions, block.producer.as_deref()) {
            println!(
                "Failed to apply block inside produce_block (state for root): {}",
                e
            );
            return;
        }
        block.state_root = state_for_root.calculate_state_root();

        if let Err(e) = self.consensus.prepare_block(&mut block, &self.state) {
            println!("Block preparation failed: {}", e);
            return;
        }

        println!("Block produced: {}", block.hash);
        if let Some(ref store) = self.storage {
            let _ = store.insert_block(&block);
            let _ = store.save_last_hash(&block.hash);
            let _ = store.save_state_root(block.index, &block.state_root);
            let _ = store.save_canonical_height(block.index);
        }

        if let Err(e) = Executor::apply_block(&mut self.state, &block.transactions, block.producer.as_deref())
        {
            println!("Failed to apply block to canonical state: {}", e);
            return;
        }

        if block.index > 0 && block.index % EPOCH_LENGTH == 0 {
            self.state.advance_epoch(block.timestamp);
        }

        self.chain.push(block.clone());

        for tx in &block.transactions {
            self.mempool.remove_transaction(&tx.hash);
        }
    }
    pub fn mine_pending_transactions(&mut self, miner_address: String) {
        self.produce_block(miner_address);
    }
    pub fn add_transaction(&mut self, transaction: Transaction) -> Result<(), String> {
        if transaction.chain_id != self.chain_id {
            return Err(format!(
                "Invalid Chain ID: expected {}, got {}",
                self.chain_id, transaction.chain_id
            ));
        }
        if transaction.from == "genesis" {
            return Err("Genesis transactions cannot be submitted to the mempool".into());
        }
        if !transaction.verify() {
            return Err("Invalid transaction signature".into());
        }
        if let Err(e) = self.state.validate_transaction(&transaction) {
            return Err(format!("Invalid transaction: {}", e));
        }

        self.mempool
            .add_transaction(transaction)
            .map_err(|e| format!("Mempool error: {:?}", e))
    }

    pub fn init_genesis_account(&mut self, address: &str) {
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
            if block.index > 0 && tx.from == "genesis" {
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

        let mut commit_state = self.state.clone();
        if let Err(e) = Executor::apply_block(&mut commit_state, &block.transactions, block.producer.as_deref()) {
            return Err(format!("Failed to apply block: {}", e));
        }

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
            let _ = store.insert_block(&block);
            let _ = store.save_last_hash(&block.hash);
            let _ = store.save_state_root(block.index, &block.state_root);
            let _ = store.save_canonical_height(block.index);
        }

        if let Some(evidences) = &block.slashing_evidence {
            let slash_ratio = 0.1;
            commit_state.apply_slashing(evidences, slash_ratio);
        }

        if block.index > 0 && block.index % EPOCH_LENGTH == 0 {
            commit_state.advance_epoch(block.timestamp);
        }

        self.state = commit_state;

        self.chain.push(block);

        if let Err(e) = self.consensus.record_block(self.chain.last().unwrap()) {
            println!("Engine record block error: {}", e);
        }

        for tx in self.chain.last().unwrap().transactions.iter() {
            self.mempool.remove_transaction(&tx.hash);
        }

        if let Some(ref pruning_manager) = self.pruning_manager {
            let last_block = self.chain.last().unwrap();
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

        let new_state = Blockchain::rebuild_state(&new_chain)?;

        self.chain = new_chain;
        self.state = new_state;

        let mut new_pending = Vec::new();

        let mut chain_txs = std::collections::HashSet::new();
        for block in &self.chain {
            for tx in &block.transactions {
                chain_txs.insert(tx.hash.clone());
            }
        }

        for tx in &self.mempool.get_sorted_transactions(1000) {
            if !chain_txs.contains(&tx.hash) {
                if self.state.validate_transaction(tx).is_ok() {
                    new_pending.push(tx.clone());
                }
            }
        }

        self.mempool = Mempool::new(MempoolConfig::default());
        for tx in new_pending {
            let _ = self.mempool.add_transaction(tx);
        }

        if let Some(ref store) = self.storage {
            if let Some(last) = self.chain.last() {
                let _ = store.save_last_hash(&last.hash);
            }
            for block in &self.chain[fork_point..] {
                let _ = store.insert_block(block);
            }
        }

        Ok(true)
    }

    fn rebuild_state(chain: &[Block]) -> Result<AccountState, String> {
        let mut state = AccountState::new();
        for block in chain.iter() {
            if let Err(e) = Executor::apply_block(&mut state, &block.transactions, block.producer.as_deref()) {
                return Err(format!(
                    "Failed to rebuild state at block {}: {}",
                    block.index, e
                ));
            }
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
    pub fn handle_finality_cert(
        &mut self,
        cert: crate::chain::finality::FinalityCert,
    ) -> Result<(), String> {
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

        let active_validators = self.state.get_active_validators();
        let entries: Vec<ValidatorEntry> = active_validators
            .into_iter()
            .map(|v| ValidatorEntry {
                address: v.address.clone(),
                stake: v.stake,
                bls_public_key: Vec::new(),
                pop_signature: Vec::new(),
            })
            .collect();
        let snapshot = ValidatorSetSnapshot::new(cert.epoch, entries);

        cert.verify(&snapshot)?;

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
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::consensus::PoWEngine;
    use crate::crypto::primitives::KeyPair;

    #[test]
    fn test_blockchain_with_pow() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        let keypair = KeyPair::generate().unwrap();
        let pubkey = keypair.public_key_hex();

        blockchain.state.add_balance(&pubkey, 100);

        let mut tx = Transaction::new(pubkey.clone(), "bob".to_string(), 50, vec![]);
        tx.fee = 1;
        tx.sign(&keypair);

        blockchain.add_transaction(tx).unwrap();

        blockchain.produce_block("miner1".to_string());
        assert!(blockchain.is_valid());
        assert_eq!(blockchain.chain.len(), 2);
    }

    #[test]
    fn test_epoch_transition_and_unjailing() {
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);

        let validator_addr = "validator1".to_string();
        blockchain.state.add_validator(validator_addr.clone(), 1000);

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
            blockchain.produce_block("miner".to_string());
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
        use crate::core::block::BlockHeader;
        use crate::consensus::pos::{PoSConfig, SlashingEvidence};
        use crate::consensus::PoSEngine;

        let alice_keys = crate::crypto::primitives::ValidatorKeys::generate().unwrap();
        let alice_key = alice_keys.sig_key.clone();
        let alice_vrf_pub = alice_keys.vrf_key.public.to_bytes().to_vec();
        let alice_pub = alice_key.public_key_hex();

        let mut config = PoSConfig::default();
        config.slashing_penalty = 0.50;

        let engine = Arc::new(PoSEngine::new(config, Some(alice_keys)));

        let mut blockchain = Blockchain::new(engine.clone(), None, 1337, None);

        blockchain.state.add_validator(alice_pub.clone(), 2000);
        if let Some(v) = blockchain.state.get_validator_mut(&alice_pub) {
            v.vrf_public_key = alice_vrf_pub.clone();
        }
        blockchain.state.add_balance(&alice_pub, 100);

        let mut real_b1 = Block::new(10, "prev".into(), vec![]);
        real_b1.producer = Some(alice_pub.clone());
        real_b1.hash = real_b1.calculate_hash();
        let sig1 = alice_key.sign(real_b1.hash.as_bytes()).to_vec();
        real_b1.signature = Some(sig1.clone());
        let h1 = BlockHeader::from_block(&real_b1);

        let mut real_b2 = Block::new(10, "prev".into(), vec![]);
        real_b2.timestamp += 1;
        real_b2.producer = Some(alice_pub.clone());
        real_b2.hash = real_b2.calculate_hash();
        let sig2 = alice_key.sign(real_b2.hash.as_bytes()).to_vec();
        real_b2.signature = Some(sig2.clone());
        let h2 = BlockHeader::from_block(&real_b2);

        let evidence = SlashingEvidence::new(h1, h2, sig1, sig2);

        {
            let mut guard = engine.slashing_evidence.write().unwrap();
            guard.push(evidence);
        }

        blockchain.produce_block(alice_pub.clone());

        let produced_block = blockchain.chain.last().unwrap();
        assert!(
            produced_block.slashing_evidence.is_some(),
            "Block should contain slashing evidence"
        );
        assert_eq!(produced_block.slashing_evidence.as_ref().unwrap().len(), 1);

        let mut blockchain2 = Blockchain::new(engine.clone(), None, 1337, None);
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
        let sender_pub = sender.public_key_hex();
        let mut bc = Blockchain::new(consensus, None, 1337, None);
        bc.state.add_balance(&sender_pub, 1000);

        let mut tx = Transaction::new_with_fee(sender_pub.clone(), "bob".into(), 100, 5, 0, vec![]);
        tx.sign(&sender);
        bc.add_transaction(tx).unwrap();

        bc.produce_block("miner_addr".to_string());
        assert_eq!(bc.state.get_balance("miner_addr"), 5);
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
