use crate::core::block::{Block, BlockHeader};
use crate::core::transaction::Transaction;

pub const PROTOCOL_VERSION_MAJOR: u32 = 1;

pub const PROTOCOL_VERSION_MINOR: u32 = 0;

pub const PROTOCOL_VERSION: &str = "1.0.0";

pub const NETWORK_MAGIC: [u8; 4] = [0xBD, 0x4C, 0x4D, 0x01];

#[derive(Debug, Clone, PartialEq)]
pub enum EncodingError {
    InvalidLength,
    InvalidMagic,
    VersionMismatch { expected: u32, got: u32 },
    DeserializationFailed(String),
}

pub fn encode_transaction(tx: &Transaction) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256);

    bytes.extend_from_slice(&PROTOCOL_VERSION_MAJOR.to_le_bytes());

    bytes.extend_from_slice(tx.from.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(tx.to.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(&tx.amount.to_le_bytes());
    bytes.extend_from_slice(&tx.fee.to_le_bytes());
    bytes.extend_from_slice(&tx.nonce.to_le_bytes());
    bytes.extend_from_slice(&tx.timestamp.to_le_bytes());

    bytes.extend_from_slice(&(tx.data.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&tx.data);

    bytes.extend_from_slice(tx.hash.as_bytes());
    bytes.push(0x00);

    if let Some(ref sig) = tx.signature {
        bytes.push(0x01);
        bytes.extend_from_slice(&(sig.len() as u32).to_le_bytes());
        bytes.extend_from_slice(sig);
    } else {
        bytes.push(0x00);
    }

    bytes
}

pub fn encode_block_header(header: &BlockHeader) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256);

    bytes.extend_from_slice(&PROTOCOL_VERSION_MAJOR.to_le_bytes());

    bytes.extend_from_slice(&header.index.to_le_bytes());
    bytes.extend_from_slice(&header.timestamp.to_le_bytes());
    bytes.extend_from_slice(header.previous_hash.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(header.hash.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(&header.chain_id.to_le_bytes());
    bytes.extend_from_slice(header.state_root.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(header.tx_root.as_bytes());
    bytes.push(0x00);

    if let Some(ref producer) = header.producer {
        bytes.push(0x01);
        bytes.extend_from_slice(producer.as_bytes());
        bytes.push(0x00);
    } else {
        bytes.push(0x00);
    }

    bytes
}

pub fn encode_block_summary(block: &Block) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(512);

    bytes.extend_from_slice(&PROTOCOL_VERSION_MAJOR.to_le_bytes());

    bytes.extend_from_slice(&block.index.to_le_bytes());
    bytes.extend_from_slice(&block.timestamp.to_le_bytes());
    bytes.extend_from_slice(block.previous_hash.as_bytes());
    bytes.push(0x00);
    bytes.extend_from_slice(&block.nonce.to_le_bytes());
    bytes.extend_from_slice(&block.chain_id.to_le_bytes());
    bytes.extend_from_slice(block.state_root.as_bytes());
    bytes.push(0x00);

    bytes.extend_from_slice(&(block.transactions.len() as u32).to_le_bytes());

    bytes
}

pub fn is_compatible_version(remote_major: u32, remote_minor: u32) -> bool {
    if remote_major != PROTOCOL_VERSION_MAJOR {
        return false;
    }

    remote_minor <= PROTOCOL_VERSION_MINOR
}

pub fn create_version_message() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16);
    bytes.extend_from_slice(&NETWORK_MAGIC);
    bytes.extend_from_slice(&PROTOCOL_VERSION_MAJOR.to_le_bytes());
    bytes.extend_from_slice(&PROTOCOL_VERSION_MINOR.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::address::Address;

    #[test]
    fn test_protocol_version() {
        assert_eq!(PROTOCOL_VERSION, "1.0.0");
        assert_eq!(PROTOCOL_VERSION_MAJOR, 1);
    }

    #[test]
    fn test_version_compatibility() {
        assert!(is_compatible_version(1, 0));
        assert!(!is_compatible_version(2, 0));
        assert!(!is_compatible_version(0, 1));
    }

    #[test]
    fn test_version_message() {
        let msg = create_version_message();
        assert_eq!(&msg[0..4], &NETWORK_MAGIC);
        assert_eq!(msg.len(), 12);
    }

    #[test]
    fn test_tx_encoding_deterministic() {
        let alice = Address::from_hex(&"01".repeat(32)).unwrap();
        let bob = Address::from_hex(&"02".repeat(32)).unwrap();
        let tx = Transaction::new(alice, bob, 100, vec![1, 2, 3]);
        let enc1 = encode_transaction(&tx);
        let enc2 = encode_transaction(&tx);
        assert_eq!(enc1, enc2);
    }

    #[test]
    fn test_block_header_encoding() {
        let header = BlockHeader {
            index: 1,
            timestamp: 12345,
            previous_hash: "abc".to_string(),
            hash: "def".to_string(),
            producer: Some(Address::from_hex(&"03".repeat(32)).unwrap()),
            chain_id: 1337,
            state_root: "root".to_string(),
            tx_root: "tx_root".to_string(),
            nonce: 0,
            slashing_evidence: None,
            epoch: 0,
            slot: 0,
            vrf_output: Vec::new(),
            vrf_proof: Vec::new(),
            validator_set_hash: String::new(),
        };
        let enc = encode_block_header(&header);
        assert!(!enc.is_empty());

        let version = u32::from_le_bytes([enc[0], enc[1], enc[2], enc[3]]);
        assert_eq!(version, PROTOCOL_VERSION_MAJOR);
    }
}
