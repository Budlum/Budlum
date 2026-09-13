//! Shared helpers for constructing real, independently-verifiable finality
//! proofs in tests: mined PoW header chains and BLS quorum certificates
//! (used identically by the PoS/PoA/BFT adapters). Centralized here so every
//! test file exercises the adapters' real verification logic instead of
//! stubbing it out.
#![cfg(test)]

use crate::chain::finality::{sign_bls, FinalityCert, ValidatorEntry, ValidatorSetSnapshot};
use crate::crypto::primitives::BlsKeypair;
use crate::domain::finality_adapter::PoWHeaderProof;
use crate::domain::types::Hash32;

/// Mines a single header extending `prev_hash`, satisfying `target`.
pub fn mine_pow_header(prev_hash: Hash32, target: Hash32, extra: Hash32) -> PoWHeaderProof {
    for nonce in 0u64..1_000_000 {
        let header = PoWHeaderProof {
            prev_hash,
            target,
            nonce,
            timestamp_ms: 0,
            extra,
        };
        if header.meets_own_target() {
            return header;
        }
    }
    panic!("failed to mine a header satisfying target within the test iteration budget");
}

/// Mines a base header committing to `domain_block_hash`, followed by
/// `confirmations` additional headers extending it, all at `target`.
pub fn mine_pow_chain(
    domain_block_hash: Hash32,
    target: Hash32,
    confirmations: usize,
) -> Vec<PoWHeaderProof> {
    let mut headers = vec![mine_pow_header([0u8; 32], target, domain_block_hash)];
    for _ in 0..confirmations {
        let prev_hash = headers.last().unwrap().hash();
        headers.push(mine_pow_header(prev_hash, target, [0u8; 32]));
    }
    headers
}

/// Builds a validator set (all signing) and a valid BLS quorum certificate
/// over `domain_block_hash` at `domain_height` — usable interchangeably as a
/// PoS/PoA/BFT finality proof, since all three adapters verify the same way.
pub fn make_quorum_proof(
    domain_height: u64,
    domain_block_hash: Hash32,
    num_validators: usize,
    stake_each: u64,
) -> (FinalityCert, ValidatorSetSnapshot) {
    let mut keys = Vec::new();
    let mut entries = Vec::new();
    for i in 0..num_validators {
        let kp = BlsKeypair::generate().unwrap();
        let addr = crate::core::address::Address::from([(i + 1) as u8; 32]);
        entries.push(ValidatorEntry {
            address: addr,
            stake: stake_each,
            bls_public_key: kp.public_key.clone(),
            pop_signature: vec![],
            pq_public_key: vec![],
        });
        keys.push(kp);
    }
    let snapshot = ValidatorSetSnapshot::new(1, entries);
    let checkpoint_hash = hex::encode(domain_block_hash);
    let msg = crate::chain::finality::checkpoint_signing_message(
        snapshot.epoch,
        domain_height,
        &checkpoint_hash,
    );

    let mut bitmap = vec![0u8; num_validators.div_ceil(8)];
    let mut agg_sig = bls12_381::G1Projective::identity();
    for (idx, kp) in keys.iter().enumerate() {
        bitmap[idx / 8] |= 1 << (idx % 8);
        let sig_bytes = sign_bls(&kp.secret_key, &msg);
        let sig_array: [u8; 48] = sig_bytes.try_into().unwrap();
        let sig_affine = bls12_381::G1Affine::from_compressed(&sig_array).unwrap();
        agg_sig += bls12_381::G1Projective::from(sig_affine);
    }

    let cert = FinalityCert {
        epoch: snapshot.epoch,
        checkpoint_height: domain_height,
        checkpoint_hash,
        agg_sig_bls: bls12_381::G1Affine::from(agg_sig).to_compressed().to_vec(),
        bitmap,
        set_hash: snapshot.set_hash.clone(),
    };

    (cert, snapshot)
}
