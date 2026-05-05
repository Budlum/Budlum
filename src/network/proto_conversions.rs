use crate::consensus::pos::SlashingEvidence;
use crate::core::address::Address;
use crate::core::block::{Block, BlockHeader};
use crate::core::transaction::{Transaction, TransactionType};
use crate::network::protocol::NetworkMessage;

pub mod pb {
    include!(concat!(env!("OUT_DIR"), "/budlum.network.rs"));
}

impl From<&Transaction> for pb::ProtoTransaction {
    fn from(tx: &Transaction) -> Self {
        pb::ProtoTransaction {
            from: tx.from.to_string(),
            to: tx.to.to_string(),
            amount: tx.amount,
            fee: tx.fee,
            nonce: tx.nonce,
            data: tx.data.clone(),
            timestamp: tx.timestamp.to_string(),
            hash: tx.hash.clone(),
            signature: tx.signature.clone().unwrap_or_default(),
            chain_id: tx.chain_id,
            tx_type: match tx.tx_type {
                TransactionType::Transfer => pb::ProtoTransactionType::Transfer as i32,
                TransactionType::Stake => pb::ProtoTransactionType::Stake as i32,
                TransactionType::Unstake => pb::ProtoTransactionType::Unstake as i32,
                TransactionType::Vote => pb::ProtoTransactionType::Vote as i32,
                TransactionType::ContractCall => pb::ProtoTransactionType::ContractCall as i32,
            },
        }
    }
}

impl TryFrom<pb::ProtoTransaction> for Transaction {
    type Error = String;
    fn try_from(proto: pb::ProtoTransaction) -> Result<Self, Self::Error> {
        let timestamp = proto
            .timestamp
            .parse::<u128>()
            .map_err(|e| format!("Invalid block timestamp string: {}", e))?;
        let signature = if proto.signature.is_empty() {
            None
        } else {
            Some(proto.signature)
        };
        let tx_type = match pb::ProtoTransactionType::try_from(proto.tx_type) {
            Ok(pb::ProtoTransactionType::Transfer) => TransactionType::Transfer,
            Ok(pb::ProtoTransactionType::Stake) => TransactionType::Stake,
            Ok(pb::ProtoTransactionType::Unstake) => TransactionType::Unstake,
            Ok(pb::ProtoTransactionType::Vote) => TransactionType::Vote,
            Ok(pb::ProtoTransactionType::ContractCall) => TransactionType::ContractCall,
            Err(_) => return Err("Invalid transaction type in proto payload".into()),
        };

        Ok(Transaction {
            from: Address::from_hex(&proto.from)
                .map_err(|e| format!("Invalid from address: {}", e))?,
            to: Address::from_hex(&proto.to).map_err(|e| format!("Invalid to address: {}", e))?,
            amount: proto.amount,
            fee: proto.fee,
            nonce: proto.nonce,
            data: proto.data,
            timestamp,
            hash: proto.hash,
            signature,
            chain_id: proto.chain_id,
            tx_type,
        })
    }
}

impl From<&SlashingEvidence> for pb::ProtoSlashingEvidence {
    fn from(ev: &SlashingEvidence) -> Self {
        pb::ProtoSlashingEvidence {
            header1: Some(pb::ProtoBlockHeader::from(&ev.header1)),
            header2: Some(pb::ProtoBlockHeader::from(&ev.header2)),
            signature1: ev.signature1.clone(),
            signature2: ev.signature2.clone(),
        }
    }
}

impl TryFrom<pb::ProtoSlashingEvidence> for SlashingEvidence {
    type Error = String;
    fn try_from(proto: pb::ProtoSlashingEvidence) -> Result<Self, Self::Error> {
        let header1 = proto.header1.ok_or("Missing header1 in proto evidence")?;
        let header2 = proto.header2.ok_or("Missing header2 in proto evidence")?;
        Ok(SlashingEvidence {
            header1: BlockHeader::try_from(header1)?,
            header2: BlockHeader::try_from(header2)?,
            signature1: proto.signature1,
            signature2: proto.signature2,
        })
    }
}

impl From<&BlockHeader> for pb::ProtoBlockHeader {
    fn from(header: &BlockHeader) -> Self {
        pb::ProtoBlockHeader {
            index: header.index,
            timestamp: header.timestamp.to_string(),
            previous_hash: header.previous_hash.clone(),
            hash: header.hash.clone(),
            producer: header.producer.map(|p| p.to_string()).unwrap_or_default(),
            chain_id: header.chain_id,
            state_root: header.state_root.clone(),
            tx_root: header.tx_root.clone(),
            slashing_evidence: header
                .slashing_evidence
                .as_ref()
                .unwrap_or(&vec![])
                .iter()
                .map(pb::ProtoSlashingEvidence::from)
                .collect(),
            nonce: header.nonce,
            epoch: header.epoch,
            slot: header.slot,
            vrf_output: header.vrf_output.clone(),
            vrf_proof: header.vrf_proof.clone(),
            validator_set_hash: header.validator_set_hash.clone(),
        }
    }
}

impl TryFrom<pb::ProtoBlockHeader> for BlockHeader {
    type Error = String;
    fn try_from(proto: pb::ProtoBlockHeader) -> Result<Self, Self::Error> {
        let timestamp = proto
            .timestamp
            .parse::<u128>()
            .map_err(|e| format!("Invalid block header timestamp string: {}", e))?;
        let producer = if proto.producer.is_empty() {
            None
        } else {
            Some(
                Address::from_hex(&proto.producer)
                    .map_err(|e| format!("Invalid producer address: {}", e))?,
            )
        };
        let mut evidence = Vec::new();
        for ev in proto.slashing_evidence {
            evidence.push(SlashingEvidence::try_from(ev)?);
        }
        let slashing_evidence = if evidence.is_empty() {
            None
        } else {
            Some(evidence)
        };
        Ok(BlockHeader {
            index: proto.index,
            timestamp,
            previous_hash: proto.previous_hash,
            hash: proto.hash,
            producer,
            chain_id: proto.chain_id,
            state_root: proto.state_root,
            tx_root: proto.tx_root,
            slashing_evidence,
            nonce: proto.nonce,
            epoch: proto.epoch,
            slot: proto.slot,
            vrf_output: proto.vrf_output,
            vrf_proof: proto.vrf_proof,
            validator_set_hash: proto.validator_set_hash,
        })
    }
}

impl From<&Block> for pb::ProtoBlock {
    fn from(block: &Block) -> Self {
        pb::ProtoBlock {
            index: block.index,
            timestamp: block.timestamp.to_string(),
            previous_hash: block.previous_hash.clone(),
            hash: block.hash.clone(),
            transactions: block
                .transactions
                .iter()
                .map(pb::ProtoTransaction::from)
                .collect(),
            nonce: block.nonce,
            producer: block.producer.map(|p| p.to_string()).unwrap_or_default(),
            signature: block.signature.clone().unwrap_or_default(),
            chain_id: block.chain_id,
            slashing_evidence: block
                .slashing_evidence
                .as_ref()
                .unwrap_or(&vec![])
                .iter()
                .map(pb::ProtoSlashingEvidence::from)
                .collect(),
            state_root: block.state_root.clone(),
            tx_root: block.tx_root.clone(),
            epoch: block.epoch,
            slot: block.slot,
            vrf_output: block.vrf_output.clone(),
            vrf_proof: block.vrf_proof.clone(),
            validator_set_hash: block.validator_set_hash.clone(),
        }
    }
}

impl TryFrom<pb::ProtoBlock> for Block {
    type Error = String;
    fn try_from(proto: pb::ProtoBlock) -> Result<Self, Self::Error> {
        let timestamp = proto
            .timestamp
            .parse::<u128>()
            .map_err(|e| format!("Invalid block timestamp string: {}", e))?;
        let producer = if proto.producer.is_empty() {
            None
        } else {
            Some(
                Address::from_hex(&proto.producer)
                    .map_err(|e| format!("Invalid producer address: {}", e))?,
            )
        };
        let signature = if proto.signature.is_empty() {
            None
        } else {
            Some(proto.signature)
        };

        let mut evidence = Vec::new();
        for ev in proto.slashing_evidence {
            evidence.push(SlashingEvidence::try_from(ev)?);
        }
        let slashing_evidence = if evidence.is_empty() {
            None
        } else {
            Some(evidence)
        };

        let mut transactions = Vec::new();
        for t in proto.transactions {
            transactions.push(Transaction::try_from(t)?);
        }

        Ok(Block {
            index: proto.index,
            timestamp,
            previous_hash: proto.previous_hash,
            hash: proto.hash,
            transactions,
            nonce: proto.nonce,
            producer,
            signature,
            chain_id: proto.chain_id,
            slashing_evidence,
            state_root: proto.state_root,
            tx_root: proto.tx_root,
            epoch: proto.epoch,
            slot: proto.slot,
            vrf_output: proto.vrf_output,
            vrf_proof: proto.vrf_proof,
            validator_set_hash: proto.validator_set_hash,
        })
    }
}

impl From<&NetworkMessage> for pb::ProtoNetworkMessage {
    fn from(msg: &NetworkMessage) -> Self {
        let payload = match msg {
            NetworkMessage::Handshake {
                version_major,
                version_minor,
                chain_id,
                best_height,
                validator_set_hash,
                supported_schemes,
            } => pb::proto_network_message::Payload::Handshake(pb::ProtoHandshake {
                version_major: *version_major,
                version_minor: *version_minor,
                chain_id: *chain_id,
                best_height: *best_height,
                validator_set_hash: validator_set_hash.clone(),
                supported_schemes: supported_schemes.clone(),
            }),
            NetworkMessage::HandshakeAck {
                version_major,
                version_minor,
                chain_id,
                best_height,
                validator_set_hash,
                supported_schemes,
            } => pb::proto_network_message::Payload::HandshakeAck(pb::ProtoHandshakeAck {
                version_major: *version_major,
                version_minor: *version_minor,
                chain_id: *chain_id,
                best_height: *best_height,
                validator_set_hash: validator_set_hash.clone(),
                supported_schemes: supported_schemes.clone(),
            }),
            NetworkMessage::Block(block) => {
                pb::proto_network_message::Payload::Block(pb::ProtoBlock::from(block))
            }
            NetworkMessage::Transaction(tx) => {
                pb::proto_network_message::Payload::Transaction(pb::ProtoTransaction::from(tx))
            }
            NetworkMessage::GetHeaders { locator, limit } => {
                pb::proto_network_message::Payload::GetHeaders(pb::ProtoGetHeaders {
                    locator: locator.clone(),
                    limit: *limit,
                })
            }
            NetworkMessage::Headers(headers) => {
                pb::proto_network_message::Payload::Headers(pb::ProtoHeaders {
                    headers: headers.iter().map(pb::ProtoBlockHeader::from).collect(),
                })
            }
            NetworkMessage::GetBlocksRange { from, to } => {
                pb::proto_network_message::Payload::GetBlocksRange(pb::ProtoGetBlocksRange {
                    from_index: *from,
                    to_index: *to,
                })
            }
            NetworkMessage::Blocks(blocks) => {
                pb::proto_network_message::Payload::Blocks(pb::ProtoBlocks {
                    blocks: blocks.iter().map(pb::ProtoBlock::from).collect(),
                })
            }
            NetworkMessage::GetBlocksByHeight {
                from_height,
                to_height,
            } => {
                pb::proto_network_message::Payload::GetBlocksByHeight(pb::ProtoGetBlocksByHeight {
                    from_height: *from_height,
                    to_height: *to_height,
                })
            }
            NetworkMessage::BlocksByHeight(blocks) => {
                pb::proto_network_message::Payload::BlocksByHeight(pb::ProtoBlocksByHeight {
                    blocks: blocks.iter().map(pb::ProtoBlock::from).collect(),
                })
            }
            NetworkMessage::StateSnapshotResponse {
                height,
                state_root,
                ok,
            } => pb::proto_network_message::Payload::StateSnapshotResponse(
                pb::ProtoStateSnapshotResponse {
                    height: *height,
                    state_root: state_root.clone(),
                    ok: *ok,
                },
            ),
            NetworkMessage::NewTip { height, hash } => {
                pb::proto_network_message::Payload::NewTip(pb::ProtoNewTip {
                    height: *height,
                    hash: hash.clone(),
                })
            }
            NetworkMessage::GetStateSnapshot { height } => {
                pb::proto_network_message::Payload::GetStateSnapshot(pb::ProtoGetStateSnapshot {
                    height: *height,
                })
            }
            NetworkMessage::SnapshotChunk {
                height,
                index,
                total,
                data,
            } => pb::proto_network_message::Payload::SnapshotChunk(pb::ProtoSnapshotChunk {
                height: *height,
                index: *index,
                total: *total,
                data: data.clone(),
            }),
            NetworkMessage::Prevote {
                epoch,
                checkpoint_height,
                checkpoint_hash,
                voter_id,
                sig_bls,
            } => pb::proto_network_message::Payload::Prevote(pb::ProtoPrevote {
                epoch: *epoch,
                checkpoint_height: *checkpoint_height,
                checkpoint_hash: checkpoint_hash.clone(),
                voter_id: voter_id.clone(),
                sig_bls: sig_bls.clone(),
            }),
            NetworkMessage::Precommit {
                epoch,
                checkpoint_height,
                checkpoint_hash,
                voter_id,
                sig_bls,
            } => pb::proto_network_message::Payload::Precommit(pb::ProtoPrecommit {
                epoch: *epoch,
                checkpoint_height: *checkpoint_height,
                checkpoint_hash: checkpoint_hash.clone(),
                voter_id: voter_id.clone(),
                sig_bls: sig_bls.clone(),
            }),
            NetworkMessage::FinalityCert {
                epoch,
                checkpoint_height,
                checkpoint_hash,
                agg_sig_bls,
                bitmap,
                set_hash,
            } => pb::proto_network_message::Payload::FinalityCert(pb::ProtoFinalityCert {
                epoch: *epoch,
                checkpoint_height: *checkpoint_height,
                checkpoint_hash: checkpoint_hash.clone(),
                agg_sig_bls: agg_sig_bls.clone(),
                bitmap: bitmap.clone(),
                set_hash: set_hash.clone(),
            }),
            NetworkMessage::GetQcBlob {
                epoch,
                checkpoint_height,
            } => pb::proto_network_message::Payload::GetQcBlob(pb::ProtoGetQcBlob {
                epoch: *epoch,
                checkpoint_height: *checkpoint_height,
            }),
            NetworkMessage::QcBlobResponse {
                epoch,
                checkpoint_height,
                checkpoint_hash,
                blob_data,
                found,
            } => pb::proto_network_message::Payload::QcBlobResponse(pb::ProtoQcBlobResponse {
                epoch: *epoch,
                checkpoint_height: *checkpoint_height,
                checkpoint_hash: checkpoint_hash.clone(),
                blob_data: blob_data.clone(),
                found: *found,
            }),
            NetworkMessage::QcFaultProof { proof_data } => {
                pb::proto_network_message::Payload::QcFaultProof(pb::ProtoQcFaultProof {
                    proof_data: proof_data.clone(),
                })
            }
            NetworkMessage::DomainCommitment(commitment) => {
                pb::proto_network_message::Payload::DomainCommitment(pb::ProtoDomainCommitment {
                    data: serde_json::to_vec(commitment).unwrap_or_default(),
                })
            }
            NetworkMessage::VerifiedDomainCommitment(payload) => {
                pb::proto_network_message::Payload::VerifiedDomainCommitment(
                    pb::ProtoVerifiedDomainCommitment {
                        data: serde_json::to_vec(payload).unwrap_or_default(),
                    },
                )
            }
            NetworkMessage::SlashingEvidence(evidence) => {
                pb::proto_network_message::Payload::SlashingEvidence(
                    pb::ProtoSlashingEvidence::from(evidence),
                )
            }
            NetworkMessage::GlobalHeader(header) => {
                pb::proto_network_message::Payload::GlobalHeader(pb::ProtoGlobalHeader {
                    data: serde_json::to_vec(header).unwrap_or_default(),
                })
            }
            NetworkMessage::CrossDomainMessage(msg) => {
                pb::proto_network_message::Payload::CrossDomainMessage(
                    pb::ProtoCrossDomainMessage {
                        data: serde_json::to_vec(msg).unwrap_or_default(),
                    },
                )
            }
        };

        pb::ProtoNetworkMessage {
            payload: Some(payload),
        }
    }
}

impl TryFrom<pb::ProtoNetworkMessage> for NetworkMessage {
    type Error = String;
    fn try_from(proto: pb::ProtoNetworkMessage) -> Result<Self, Self::Error> {
        let payload = proto
            .payload
            .ok_or("Empty payload in ProtoNetworkMessage")?;
        match payload {
            pb::proto_network_message::Payload::Handshake(h) => Ok(NetworkMessage::Handshake {
                version_major: h.version_major,
                version_minor: h.version_minor,
                chain_id: h.chain_id,
                best_height: h.best_height,
                validator_set_hash: h.validator_set_hash,
                supported_schemes: h.supported_schemes,
            }),
            pb::proto_network_message::Payload::HandshakeAck(h) => {
                Ok(NetworkMessage::HandshakeAck {
                    version_major: h.version_major,
                    version_minor: h.version_minor,
                    chain_id: h.chain_id,
                    best_height: h.best_height,
                    validator_set_hash: h.validator_set_hash,
                    supported_schemes: h.supported_schemes,
                })
            }
            pb::proto_network_message::Payload::Block(b) => {
                Ok(NetworkMessage::Block(Block::try_from(b)?))
            }
            pb::proto_network_message::Payload::Transaction(t) => {
                Ok(NetworkMessage::Transaction(Transaction::try_from(t)?))
            }
            pb::proto_network_message::Payload::GetHeaders(h) => Ok(NetworkMessage::GetHeaders {
                locator: h.locator,
                limit: h.limit,
            }),
            pb::proto_network_message::Payload::Headers(h) => {
                let mut headers = Vec::new();
                for header in h.headers {
                    headers.push(BlockHeader::try_from(header)?);
                }
                Ok(NetworkMessage::Headers(headers))
            }
            pb::proto_network_message::Payload::GetBlocksRange(r) => {
                Ok(NetworkMessage::GetBlocksRange {
                    from: r.from_index,
                    to: r.to_index,
                })
            }
            pb::proto_network_message::Payload::Blocks(b) => {
                let mut blocks = Vec::new();
                for block in b.blocks {
                    blocks.push(Block::try_from(block)?);
                }
                Ok(NetworkMessage::Blocks(blocks))
            }
            pb::proto_network_message::Payload::GetBlocksByHeight(r) => {
                Ok(NetworkMessage::GetBlocksByHeight {
                    from_height: r.from_height,
                    to_height: r.to_height,
                })
            }
            pb::proto_network_message::Payload::BlocksByHeight(b) => {
                let mut blocks = Vec::new();
                for block in b.blocks {
                    blocks.push(Block::try_from(block)?);
                }
                Ok(NetworkMessage::BlocksByHeight(blocks))
            }
            pb::proto_network_message::Payload::StateSnapshotResponse(r) => {
                Ok(NetworkMessage::StateSnapshotResponse {
                    height: r.height,
                    state_root: r.state_root,
                    ok: r.ok,
                })
            }
            pb::proto_network_message::Payload::NewTip(t) => Ok(NetworkMessage::NewTip {
                height: t.height,
                hash: t.hash,
            }),
            pb::proto_network_message::Payload::GetStateSnapshot(s) => {
                Ok(NetworkMessage::GetStateSnapshot { height: s.height })
            }
            pb::proto_network_message::Payload::SnapshotChunk(c) => {
                Ok(NetworkMessage::SnapshotChunk {
                    height: c.height,
                    index: c.index,
                    total: c.total,
                    data: c.data,
                })
            }
            pb::proto_network_message::Payload::Prevote(v) => Ok(NetworkMessage::Prevote {
                epoch: v.epoch,
                checkpoint_height: v.checkpoint_height,
                checkpoint_hash: v.checkpoint_hash,
                voter_id: v.voter_id,
                sig_bls: v.sig_bls,
            }),
            pb::proto_network_message::Payload::Precommit(v) => Ok(NetworkMessage::Precommit {
                epoch: v.epoch,
                checkpoint_height: v.checkpoint_height,
                checkpoint_hash: v.checkpoint_hash,
                voter_id: v.voter_id,
                sig_bls: v.sig_bls,
            }),
            pb::proto_network_message::Payload::FinalityCert(f) => {
                Ok(NetworkMessage::FinalityCert {
                    epoch: f.epoch,
                    checkpoint_height: f.checkpoint_height,
                    checkpoint_hash: f.checkpoint_hash,
                    agg_sig_bls: f.agg_sig_bls,
                    bitmap: f.bitmap,
                    set_hash: f.set_hash,
                })
            }
            pb::proto_network_message::Payload::GetQcBlob(q) => Ok(NetworkMessage::GetQcBlob {
                epoch: q.epoch,
                checkpoint_height: q.checkpoint_height,
            }),
            pb::proto_network_message::Payload::QcBlobResponse(q) => {
                Ok(NetworkMessage::QcBlobResponse {
                    epoch: q.epoch,
                    checkpoint_height: q.checkpoint_height,
                    checkpoint_hash: q.checkpoint_hash,
                    blob_data: q.blob_data,
                    found: q.found,
                })
            }
            pb::proto_network_message::Payload::QcFaultProof(p) => {
                Ok(NetworkMessage::QcFaultProof {
                    proof_data: p.proof_data,
                })
            }
            pb::proto_network_message::Payload::DomainCommitment(c) => {
                let commitment = serde_json::from_slice(&c.data)
                    .map_err(|e| format!("Invalid domain commitment payload: {}", e))?;
                Ok(NetworkMessage::DomainCommitment(commitment))
            }
            pb::proto_network_message::Payload::VerifiedDomainCommitment(c) => {
                let payload = serde_json::from_slice(&c.data)
                    .map_err(|e| format!("Invalid verified domain commitment payload: {}", e))?;
                Ok(NetworkMessage::VerifiedDomainCommitment(payload))
            }
            pb::proto_network_message::Payload::SlashingEvidence(e) => Ok(
                NetworkMessage::SlashingEvidence(SlashingEvidence::try_from(e)?),
            ),
            pb::proto_network_message::Payload::GlobalHeader(h) => {
                let header = serde_json::from_slice(&h.data)
                    .map_err(|e| format!("Invalid global header payload: {}", e))?;
                Ok(NetworkMessage::GlobalHeader(header))
            }
            pb::proto_network_message::Payload::CrossDomainMessage(m) => {
                let msg = serde_json::from_slice(&m.data)
                    .map_err(|e| format!("Invalid cross domain message payload: {}", e))?;
                Ok(NetworkMessage::CrossDomainMessage(msg))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::primitives::KeyPair;

    #[test]
    fn test_transaction_proto_conversion() {
        let keypair = KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        let mut tx = Transaction::new_with_fee(from, Address::zero(), 100, 1, 42, vec![1, 2, 3, 4]);
        tx.sign(&keypair);

        let proto_tx = pb::ProtoTransaction::from(&tx);

        let decoded_tx =
            Transaction::try_from(proto_tx).expect("Failed to decode proto transaction");

        assert_eq!(tx, decoded_tx);
    }

    #[test]
    fn test_block_proto_conversion() {
        let keypair = KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        let mut tx = Transaction::new(from, Address::zero(), 50, vec![]);
        tx.sign(&keypair);

        let mut block = Block::new(10, "PREV_HASH".to_string(), vec![tx]);
        block.state_root = "STATE_ROOT".to_string();
        block.tx_root = "TX_ROOT".to_string();
        block.producer = Some(from);
        block.sign(&keypair);

        let proto_block = pb::ProtoBlock::from(&block);

        let decoded_block = Block::try_from(proto_block).expect("Failed to decode proto block");

        assert_eq!(block, decoded_block);
    }

    #[test]
    fn test_network_message_block_conversion() {
        let block = Block::new(1, "PREV".to_string(), vec![]);
        let msg = NetworkMessage::Block(block);

        let proto_msg = pb::ProtoNetworkMessage::from(&msg);

        let decoded_msg =
            NetworkMessage::try_from(proto_msg).expect("Failed to decode NetworkMessage");

        if let (NetworkMessage::Block(orig_b), NetworkMessage::Block(dec_b)) = (&msg, &decoded_msg)
        {
            assert_eq!(orig_b, dec_b);
        } else {
            panic!("Decoded message is not a Block");
        }
    }

    #[test]
    fn test_qc_fault_proof_message_conversion() {
        let proof_data = br#"{"version":1}"#.to_vec();
        let msg = NetworkMessage::QcFaultProof {
            proof_data: proof_data.clone(),
        };

        let proto_msg = pb::ProtoNetworkMessage::from(&msg);
        let decoded_msg =
            NetworkMessage::try_from(proto_msg).expect("Failed to decode QcFaultProof message");

        match decoded_msg {
            NetworkMessage::QcFaultProof {
                proof_data: decoded,
            } => {
                assert_eq!(decoded, proof_data);
            }
            _ => panic!("Expected QcFaultProof message"),
        }
    }

    #[test]
    fn test_domain_commitment_message_conversion() {
        let commitment = crate::domain::DomainCommitment {
            domain_id: 1,
            domain_height: 42,
            domain_block_hash: [1u8; 32],
            parent_domain_block_hash: [2u8; 32],
            state_root: [3u8; 32],
            tx_root: [4u8; 32],
            event_root: [5u8; 32],
            finality_proof_hash: [6u8; 32],
            consensus_kind: crate::domain::ConsensusKind::PoW,
            validator_set_hash: [7u8; 32],
            timestamp_ms: 123,
            sequence: 9,
            producer: None,
            state_updates: std::collections::BTreeMap::new(),
        };
        let msg = NetworkMessage::DomainCommitment(commitment.clone());
        let proto_msg = pb::ProtoNetworkMessage::from(&msg);
        let decoded_msg =
            NetworkMessage::try_from(proto_msg).expect("Failed to decode DomainCommitment");

        match decoded_msg {
            NetworkMessage::DomainCommitment(decoded) => assert_eq!(decoded, commitment),
            _ => panic!("Expected DomainCommitment message"),
        }
    }

    #[test]
    fn test_verified_domain_commitment_message_conversion() {
        let commitment = crate::domain::DomainCommitment {
            domain_id: 1,
            domain_height: 42,
            domain_block_hash: [1u8; 32],
            parent_domain_block_hash: [2u8; 32],
            state_root: [3u8; 32],
            tx_root: [4u8; 32],
            event_root: [5u8; 32],
            finality_proof_hash: crate::domain::hash_finality_proof(
                &crate::domain::FinalityProof::PoW {
                    confirmations: 64,
                    total_work_hint: 1000,
                },
            ),
            consensus_kind: crate::domain::ConsensusKind::PoW,
            validator_set_hash: [7u8; 32],
            timestamp_ms: 123,
            sequence: 9,
            producer: None,
            state_updates: std::collections::BTreeMap::new(),
        };
        let payload = crate::domain::VerifiedDomainCommitment {
            commitment: commitment.clone(),
            proof: crate::domain::FinalityProof::PoW {
                confirmations: 64,
                total_work_hint: 1000,
            },
        };
        let msg = NetworkMessage::VerifiedDomainCommitment(payload);
        let proto_msg = pb::ProtoNetworkMessage::from(&msg);
        let decoded_msg =
            NetworkMessage::try_from(proto_msg).expect("Failed to decode VerifiedDomainCommitment");

        match decoded_msg {
            NetworkMessage::VerifiedDomainCommitment(decoded) => {
                assert_eq!(decoded.commitment, commitment);
                match decoded.proof {
                    crate::domain::FinalityProof::PoW {
                        confirmations,
                        total_work_hint,
                    } => {
                        assert_eq!(confirmations, 64);
                        assert_eq!(total_work_hint, 1000);
                    }
                    _ => panic!("Expected PoW finality proof"),
                }
            }
            _ => panic!("Expected VerifiedDomainCommitment message"),
        }
    }

    #[test]
    fn test_global_header_message_conversion() {
        let header = crate::settlement::GlobalBlockHeader {
            version: 1,
            global_height: 7,
            previous_global_hash: [1u8; 32],
            chain_id: 1337,
            timestamp_ms: 456,
            domain_registry_root: [2u8; 32],
            domain_commitment_root: [3u8; 32],
            message_root: [4u8; 32],
            bridge_state_root: [5u8; 32],
            replay_nonce_root: [6u8; 32],
            proposer: None,
            settlement_finality_root: [7u8; 32],
        };
        let msg = NetworkMessage::GlobalHeader(header.clone());
        let proto_msg = pb::ProtoNetworkMessage::from(&msg);
        let decoded_msg =
            NetworkMessage::try_from(proto_msg).expect("Failed to decode GlobalHeader");

        match decoded_msg {
            NetworkMessage::GlobalHeader(decoded) => assert_eq!(decoded, header),
            _ => panic!("Expected GlobalHeader message"),
        }
    }

    #[test]
    fn test_cross_domain_message_conversion() {
        let msg_inner = crate::cross_domain::CrossDomainMessage::new(
            crate::cross_domain::message::CrossDomainMessageParams {
                source_domain: 1,
                target_domain: 2,
                source_height: 10,
                event_index: 0,
                nonce: 42,
                sender: crate::core::address::Address::zero(),
                recipient: crate::core::address::Address::zero(),
                payload_hash: [9u8; 32],
                kind: crate::cross_domain::MessageKind::BridgeLock,
                expiry_height: 100,
            },
        );
        let msg = NetworkMessage::CrossDomainMessage(msg_inner.clone());
        let proto_msg = pb::ProtoNetworkMessage::from(&msg);
        let decoded_msg =
            NetworkMessage::try_from(proto_msg).expect("Failed to decode CrossDomainMessage");

        match decoded_msg {
            NetworkMessage::CrossDomainMessage(decoded) => assert_eq!(decoded, msg_inner),
            _ => panic!("Expected CrossDomainMessage"),
        }
    }
}
