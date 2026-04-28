use crate::core::block::{Block, BlockHeader};
use crate::core::transaction::Transaction;
use serde::{Deserialize, Serialize};

pub const MAX_MESSAGE_SIZE: usize = 10 * 1024 * 1024;
pub const MAX_BLOCK_SIZE: usize = 1 * 1024 * 1024;
pub const MAX_TX_SIZE: usize = 100 * 1024;
pub const MAX_CHAIN_SYNC_BLOCKS: usize = 500;
pub const MAX_HEADERS_PER_REQUEST: u32 = 2000;

#[derive(Debug, Clone, PartialEq)]
pub enum MessageError {
    TooLarge(usize),
    ParseError(String),
    VersionMismatch { expected: u32, got: u32 },
}

pub const MAX_SNAP_BATCH: u64 = 256;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NetworkMessage {
    Handshake {
        version_major: u32,
        version_minor: u32,
        chain_id: u64,
        best_height: u64,
        validator_set_hash: String,
        supported_schemes: Vec<String>,
    },
    HandshakeAck {
        version_major: u32,
        version_minor: u32,
        chain_id: u64,
        best_height: u64,
        validator_set_hash: String,
        supported_schemes: Vec<String>,
    },

    Block(Block),
    Transaction(Transaction),

    GetHeaders {
        locator: Vec<String>,
        limit: u32,
    },

    Headers(Vec<BlockHeader>),

    GetBlocksRange {
        from: u64,
        to: u64,
    },

    Blocks(Vec<Block>),

    GetBlocksByHeight {
        from_height: u64,
        to_height: u64,
    },

    BlocksByHeight(Vec<Block>),

    StateSnapshotResponse {
        height: u64,
        state_root: String,
        ok: bool,
    },

    NewTip {
        height: u64,
        hash: String,
    },

    GetStateSnapshot {
        height: u64,
    },

    SnapshotChunk {
        height: u64,
        index: u32,
        total: u32,
        data: Vec<u8>,
    },

    Prevote {
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: String,
        voter_id: String,
        sig_bls: Vec<u8>,
    },

    Precommit {
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: String,
        voter_id: String,
        sig_bls: Vec<u8>,
    },

    FinalityCert {
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: String,
        agg_sig_bls: Vec<u8>,
        bitmap: Vec<u8>,
        set_hash: String,
    },

    GetQcBlob {
        epoch: u64,
        checkpoint_height: u64,
    },

    QcBlobResponse {
        epoch: u64,
        checkpoint_height: u64,
        checkpoint_hash: String,
        blob_data: Vec<u8>,
        found: bool,
    },

    QcFaultProof {
        proof_data: Vec<u8>,
    },
}
impl NetworkMessage {
    pub fn to_bytes(&self) -> Vec<u8> {
        use prost::Message;
        let proto_msg = crate::network::proto_conversions::pb::ProtoNetworkMessage::from(self);
        proto_msg.encode_to_vec()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        use prost::Message;
        let proto_msg = crate::network::proto_conversions::pb::ProtoNetworkMessage::decode(bytes)
            .map_err(|e| format!("Protobuf decode error: {}", e))?;
        Self::try_from(proto_msg)
    }

    pub fn from_bytes_validated(bytes: &[u8]) -> Result<Self, MessageError> {
        if bytes.len() > MAX_MESSAGE_SIZE {
            return Err(MessageError::TooLarge(bytes.len()));
        }
        Self::from_bytes(bytes).map_err(|e| MessageError::ParseError(e))
    }

    pub fn validate_block_size(block: &Block) -> Result<(), MessageError> {
        use prost::Message;
        let proto_block = crate::network::proto_conversions::pb::ProtoBlock::from(block);
        let size = proto_block.encoded_len();
        if size > MAX_BLOCK_SIZE {
            return Err(MessageError::TooLarge(size));
        }
        Ok(())
    }

    pub fn validate_tx_size(tx: &Transaction) -> Result<(), MessageError> {
        use prost::Message;
        let proto_tx = crate::network::proto_conversions::pb::ProtoTransaction::from(tx);
        let size = proto_tx.encoded_len();
        if size > MAX_TX_SIZE {
            return Err(MessageError::TooLarge(size));
        }
        Ok(())
    }
}
