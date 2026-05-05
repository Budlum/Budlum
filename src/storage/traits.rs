use crate::chain::finality::FinalityCert;
use crate::consensus::pos::Checkpoint;
use crate::consensus::qc::QcBlob;
use crate::core::account::Account;
use crate::core::address::Address;
use crate::core::block::{Block, BlockHeader};
use crate::core::transaction::Transaction;
use crate::cross_domain::message::CrossDomainMessage;
use crate::cross_domain::BridgeState;
use crate::domain::{ConsensusDomain, DomainCommitment};
use crate::settlement::GlobalBlockHeader;
use std::collections::HashMap;

pub type SeenBlockMap = HashMap<(Address, u64), (BlockHeader, Vec<u8>)>;

pub trait BlockchainStorage: Send + Sync {
    fn insert_block(&self, block: &Block) -> std::io::Result<()>;
    fn commit_block(&self, block: &Block, state_root: &str) -> std::io::Result<()>;
    fn get_block(&self, hash: &str) -> std::io::Result<Option<Block>>;
    fn get_block_by_height(&self, height: u64) -> std::io::Result<Option<Block>>;
    fn get_canonical_height(&self) -> std::io::Result<u64>;
    fn save_canonical_height(&self, height: u64) -> std::io::Result<()>;
    fn save_state_root(&self, height: u64, state_root: &str) -> std::io::Result<()>;
    fn get_state_root(&self, height: u64) -> std::io::Result<Option<String>>;
    fn save_last_hash(&self, hash: &str) -> std::io::Result<()>;
    fn get_last_hash(&self) -> std::io::Result<Option<String>>;
    fn load_chain(&self) -> std::io::Result<Vec<Block>>;
    fn delete_block(&self, height: u64) -> std::io::Result<()>;
    fn save_qc_blob(&self, height: u64, blob: &QcBlob) -> std::io::Result<()>;
    fn get_qc_blob(&self, height: u64) -> std::io::Result<Option<QcBlob>>;
    fn delete_qc_blob(&self, height: u64) -> std::io::Result<()>;
    fn save_finality_cert(&self, height: u64, cert: &FinalityCert) -> std::io::Result<()>;
    fn get_finality_cert(&self, height: u64) -> std::io::Result<Option<FinalityCert>>;
    fn delete_finality_cert(&self, height: u64) -> std::io::Result<()>;
    fn save_consensus_domain(&self, domain: &ConsensusDomain) -> std::io::Result<()>;
    fn load_consensus_domains(&self) -> std::io::Result<Vec<ConsensusDomain>>;
    fn save_domain_commitment(&self, commitment: &DomainCommitment) -> std::io::Result<()>;
    fn save_domain_commitment_batch(
        &self,
        commitment: &DomainCommitment,
        domains: &[ConsensusDomain],
    ) -> std::io::Result<()>;
    fn load_domain_commitments(&self) -> std::io::Result<Vec<DomainCommitment>>;
    fn save_global_header(&self, header: &GlobalBlockHeader) -> std::io::Result<()>;
    fn get_global_header(&self, height: u64) -> std::io::Result<Option<GlobalBlockHeader>>;
    fn load_global_headers(&self) -> std::io::Result<Vec<GlobalBlockHeader>>;
    fn save_bridge_state(&self, bridge_state: &BridgeState) -> std::io::Result<()>;
    fn load_bridge_state(&self) -> std::io::Result<Option<BridgeState>>;
    fn save_cross_domain_message(&self, message: &CrossDomainMessage) -> std::io::Result<()>;
    fn load_cross_domain_messages(&self) -> std::io::Result<Vec<CrossDomainMessage>>;
    fn save_tx_index(&self, tx_hash: &str, block_height: u64) -> std::io::Result<()>;
    fn get_tx_block_height(&self, tx_hash: &str) -> std::io::Result<Option<u64>>;
    fn delete_tx_index(&self, tx_hash: &str) -> std::io::Result<()>;
    fn save_account(&self, pubkey: &Address, account: &Account) -> std::io::Result<()>;
    fn load_all_accounts(&self) -> std::io::Result<HashMap<Address, Account>>;
    fn save_mempool_tx(&self, tx: &Transaction) -> std::io::Result<()>;
    fn remove_mempool_tx(&self, tx_hash: &str) -> std::io::Result<()>;
    fn load_mempool_txs(&self) -> std::io::Result<Vec<Transaction>>;
    fn save_checkpoint(&self, checkpoint: &Checkpoint) -> std::io::Result<()>;
    fn load_checkpoints(&self) -> std::io::Result<Vec<Checkpoint>>;
    fn save_seen_block(&self, header: &BlockHeader, sig: &[u8]) -> std::io::Result<()>;
    fn load_all_seen_blocks(&self) -> std::io::Result<SeenBlockMap>;
    fn flush_batch(&self) -> std::io::Result<usize>;
}
