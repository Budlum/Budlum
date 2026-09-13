use crate::core::address::Address;
use bls12_381::hash_to_curve::{ExpandMsgXmd, HashToCurve};
use bls12_381::{G1Affine, G1Projective, G2Affine, G2Projective, Scalar};
use serde::{Deserialize, Serialize};
use sha3::{Digest, Sha3_256};
use std::collections::HashMap;

use crate::core::chain_config::{
    FINALITY_CHECKPOINT_INTERVAL, FINALITY_QUORUM_DENOMINATOR, FINALITY_QUORUM_NUMERATOR,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSetSnapshot {
    pub epoch: u64,
    pub validators: Vec<ValidatorEntry>,
    pub set_hash: String,
    pub total_stake: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorEntry {
    pub address: Address,
    pub stake: u64,
    #[serde(default)]
    pub bls_public_key: Vec<u8>,
    #[serde(default)]
    pub pop_signature: Vec<u8>,
    #[serde(default)]
    pub pq_public_key: Vec<u8>,
}

impl ValidatorSetSnapshot {
    pub fn new(epoch: u64, validators: Vec<ValidatorEntry>) -> Self {
        let total_stake = validators.iter().map(|v| v.stake).sum();
        let set_hash = Self::compute_hash(&validators);
        ValidatorSetSnapshot {
            epoch,
            validators,
            set_hash,
            total_stake,
        }
    }

    pub fn compute_hash(validators: &[ValidatorEntry]) -> String {
        let mut sorted_validators = validators.to_vec();
        sorted_validators.sort_by_key(|v| v.address);

        let mut hasher = Sha3_256::new();
        for v in sorted_validators {
            hasher.update(v.address.0);
            hasher.update(v.stake.to_le_bytes());
            hasher.update(&v.bls_public_key);
            hasher.update(&v.pq_public_key);
        }
        hex::encode(hasher.finalize())
    }

    pub fn find_validator(&self, address: &Address) -> Option<&ValidatorEntry> {
        self.validators.iter().find(|v| &v.address == address)
    }

    pub fn validator_index(&self, address: &Address) -> Option<usize> {
        self.validators.iter().position(|v| &v.address == address)
    }

    pub fn quorum_stake(&self) -> u64 {
        (self.total_stake * FINALITY_QUORUM_NUMERATOR) / FINALITY_QUORUM_DENOMINATOR + 1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Prevote {
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub voter_id: Address,
    pub sig_bls: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Precommit {
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub voter_id: Address,
    pub sig_bls: Vec<u8>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinalityCert {
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub agg_sig_bls: Vec<u8>,
    pub bitmap: Vec<u8>,
    pub set_hash: String,
}

impl Prevote {
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(b"BUDLUM_PREVOTE");
        msg.extend_from_slice(&self.epoch.to_le_bytes());
        msg.extend_from_slice(&self.checkpoint_height.to_le_bytes());
        msg.extend_from_slice(self.checkpoint_hash.as_bytes());
        msg
    }
}

impl Precommit {
    pub fn signing_message(&self) -> Vec<u8> {
        checkpoint_signing_message(self.epoch, self.checkpoint_height, &self.checkpoint_hash)
    }
}

pub fn is_checkpoint_height(height: u64) -> bool {
    height > 0 && height.is_multiple_of(FINALITY_CHECKPOINT_INTERVAL)
}

pub fn checkpoint_signing_message(epoch: u64, height: u64, hash: &str) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(b"BUDLUM_PRECOMMIT");
    msg.extend_from_slice(&epoch.to_le_bytes());
    msg.extend_from_slice(&height.to_le_bytes());
    msg.extend_from_slice(hash.as_bytes());
    msg
}

pub fn pop_signing_message(address: &Address, bls_pk: &[u8]) -> Vec<u8> {
    let mut msg = Vec::new();
    msg.extend_from_slice(b"BUDLUM_BLS_POP");
    msg.extend_from_slice(&address.0);
    msg.extend_from_slice(bls_pk);
    msg
}

/// RFC 9380-compliant hash-to-curve (SSWU map over SHA-256, via the
/// `bls12_381` crate's built-in `hash_to_curve` module). Do NOT replace this
/// with "hash the message to a scalar and multiply the generator" — that
/// construction leaks the discrete log between any two messages' hashes
/// (it's just the ratio of their hash-derived scalars), which lets anyone
/// who has observed a single valid signature rescale it into a valid
/// signature over an arbitrary different message, without the secret key.
const BLS_SIG_DST: &[u8] = b"BUDLUM_BLS_SIG_V2_BLS12381G1_XMD:SHA-256_SSWU_RO_";

pub fn hash_to_g1(msg: &[u8]) -> G1Affine {
    let point = <G1Projective as HashToCurve<ExpandMsgXmd<sha2_09::Sha256>>>::hash_to_curve(
        msg,
        BLS_SIG_DST,
    );
    G1Affine::from(point)
}

pub fn sign_bls(sk: &Scalar, msg: &[u8]) -> Vec<u8> {
    let h_msg = hash_to_g1(msg);
    let sig = G1Affine::from(G1Projective::from(h_msg) * sk);
    sig.to_compressed().to_vec()
}

pub fn verify_bls_sig(pk: &[u8], msg: &[u8], sig: &[u8]) -> Result<(), String> {
    let pk_bytes: [u8; 96] = pk
        .try_into()
        .map_err(|_| "Invalid BLS public key length".to_string())?;
    let pk_affine = G2Affine::from_compressed(&pk_bytes);
    if pk_affine.is_none().into() {
        return Err("Invalid BLS public key encoding".to_string());
    }

    let sig_bytes: [u8; 48] = sig
        .try_into()
        .map_err(|_| "Invalid BLS signature length".to_string())?;
    let sig_affine = G1Affine::from_compressed(&sig_bytes);
    if sig_affine.is_none().into() {
        return Err("Invalid BLS signature encoding".to_string());
    }

    let h_msg = hash_to_g1(msg);
    let g2_gen_neg = -G2Affine::generator();

    let pairing_result = bls12_381::multi_miller_loop(&[
        (&sig_affine.unwrap(), &g2_gen_neg.into()),
        (&h_msg, &pk_affine.unwrap().into()),
    ])
    .final_exponentiation();

    if pairing_result != bls12_381::Gt::identity() {
        return Err("BLS signature verification failed".into());
    }
    Ok(())
}

pub fn verify_pop(entry: &ValidatorEntry) -> bool {
    if entry.bls_public_key.is_empty() || entry.pop_signature.is_empty() {
        return false;
    }

    // Parse BLS Public Key (G2)
    let pk_bytes: [u8; 96] = match entry.bls_public_key.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let pk_affine = G2Affine::from_compressed(&pk_bytes);
    if pk_affine.is_none().into() {
        return false;
    }

    // Parse PoP Signature (G1)
    let sig_bytes: [u8; 48] = match entry.pop_signature.as_slice().try_into() {
        Ok(b) => b,
        Err(_) => return false,
    };
    let sig_affine = G1Affine::from_compressed(&sig_bytes);
    if sig_affine.is_none().into() {
        return false;
    }

    // Verify PoP: e(sig, G2_gen) == e(H(pop_msg), pk)
    let msg = pop_signing_message(&entry.address, &entry.bls_public_key);
    let h_msg = hash_to_g1(&msg);

    let g2_gen_neg = -G2Affine::generator();
    let pairing_result = bls12_381::multi_miller_loop(&[
        (&sig_affine.unwrap(), &g2_gen_neg.into()),
        (&h_msg, &pk_affine.unwrap().into()),
    ])
    .final_exponentiation();

    pairing_result == bls12_381::Gt::identity()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AggregatorState {
    pub active: bool,
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub prevote_quorum_reached: bool,
    pub precommit_quorum_reached: bool,
    pub prevote_count: usize,
    pub precommit_count: usize,
}

impl AggregatorState {
    pub fn inactive() -> Self {
        AggregatorState {
            active: false,
            epoch: 0,
            checkpoint_height: 0,
            checkpoint_hash: String::new(),
            prevote_quorum_reached: false,
            precommit_quorum_reached: false,
            prevote_count: 0,
            precommit_count: 0,
        }
    }
}

#[derive(Debug)]
pub struct FinalityAggregator {
    pub epoch: u64,
    pub checkpoint_height: u64,
    pub checkpoint_hash: String,
    pub prevotes: HashMap<Address, Prevote>,
    pub precommits: HashMap<Address, Precommit>,
    pub validator_snapshot: Option<ValidatorSetSnapshot>,
    pub prevote_quorum_reached: bool,
    pub precommit_quorum_reached: bool,
}

impl FinalityAggregator {
    pub fn new(epoch: u64, checkpoint_height: u64, checkpoint_hash: String) -> Self {
        FinalityAggregator {
            epoch,
            checkpoint_height,
            checkpoint_hash,
            prevotes: HashMap::new(),
            precommits: HashMap::new(),
            validator_snapshot: None,
            prevote_quorum_reached: false,
            precommit_quorum_reached: false,
        }
    }

    pub fn set_validator_snapshot(&mut self, snapshot: ValidatorSetSnapshot) {
        self.validator_snapshot = Some(snapshot);
    }

    pub fn add_prevote(&mut self, vote: Prevote) -> Result<(), String> {
        if vote.epoch != self.epoch {
            return Err("Prevote epoch mismatch".into());
        }
        if vote.checkpoint_hash != self.checkpoint_hash {
            return Err("Prevote checkpoint hash mismatch".into());
        }
        if vote.checkpoint_height != self.checkpoint_height {
            return Err("Prevote checkpoint height mismatch".into());
        }

        if let Some(ref snapshot) = self.validator_snapshot {
            if snapshot.find_validator(&vote.voter_id).is_none() {
                return Err("Voter not in validator set".into());
            }
        }

        if self.prevotes.contains_key(&vote.voter_id) {
            return Err("Duplicate prevote".into());
        }

        self.prevotes.insert(vote.voter_id, vote);
        self.check_prevote_quorum();
        Ok(())
    }

    pub fn add_precommit(&mut self, vote: Precommit) -> Result<(), String> {
        if vote.epoch != self.epoch {
            return Err("Precommit epoch mismatch".into());
        }
        if vote.checkpoint_hash != self.checkpoint_hash {
            return Err("Precommit checkpoint hash mismatch".into());
        }
        if vote.checkpoint_height != self.checkpoint_height {
            return Err("Precommit checkpoint height mismatch".into());
        }

        if !self.prevote_quorum_reached {
            return Err("Cannot precommit before prevote quorum".into());
        }

        if let Some(ref snapshot) = self.validator_snapshot {
            if snapshot.find_validator(&vote.voter_id).is_none() {
                return Err("Voter not in validator set".into());
            }
        }

        if self.precommits.contains_key(&vote.voter_id) {
            return Err("Duplicate precommit".into());
        }

        self.precommits.insert(vote.voter_id, vote);
        self.check_precommit_quorum();
        Ok(())
    }

    fn check_prevote_quorum(&mut self) {
        if let Some(ref snapshot) = self.validator_snapshot {
            let voted_stake: u64 = self
                .prevotes
                .keys()
                .filter_map(|addr| snapshot.find_validator(addr))
                .map(|v| v.stake)
                .sum();
            if voted_stake >= snapshot.quorum_stake() {
                self.prevote_quorum_reached = true;
            }
        }
    }

    fn check_precommit_quorum(&mut self) {
        if let Some(ref snapshot) = self.validator_snapshot {
            let voted_stake: u64 = self
                .precommits
                .keys()
                .filter_map(|addr| snapshot.find_validator(addr))
                .map(|v| v.stake)
                .sum();
            if voted_stake >= snapshot.quorum_stake() {
                self.precommit_quorum_reached = true;
            }
        }
    }

    pub fn try_produce_cert(&self) -> Option<FinalityCert> {
        if !self.precommit_quorum_reached {
            return None;
        }

        let snapshot = self.validator_snapshot.as_ref()?;

        let mut bitmap = vec![0u8; snapshot.validators.len().div_ceil(8)];
        let mut agg_sig = G1Projective::identity();

        for (addr, precommit) in &self.precommits {
            if let Some(idx) = snapshot.validator_index(addr) {
                bitmap[idx / 8] |= 1 << (idx % 8);

                let sig_bytes: [u8; 48] = precommit
                    .sig_bls
                    .as_slice()
                    .try_into()
                    .map_err(|_| "Invalid precommit signature length".to_string())
                    .ok()?;
                let sig_affine = G1Affine::from_compressed(&sig_bytes);
                if sig_affine.is_some().into() {
                    agg_sig += G1Projective::from(sig_affine.unwrap());
                }
            }
        }

        Some(FinalityCert {
            epoch: self.epoch,
            checkpoint_height: self.checkpoint_height,
            checkpoint_hash: self.checkpoint_hash.clone(),
            agg_sig_bls: G1Affine::from(agg_sig).to_compressed().to_vec(),
            bitmap,
            set_hash: snapshot.set_hash.clone(),
        })
    }

    pub fn get_state(&self) -> AggregatorState {
        AggregatorState {
            active: true,
            epoch: self.epoch,
            checkpoint_height: self.checkpoint_height,
            checkpoint_hash: self.checkpoint_hash.clone(),
            prevote_quorum_reached: self.prevote_quorum_reached,
            precommit_quorum_reached: self.precommit_quorum_reached,
            prevote_count: self.prevotes.len(),
            precommit_count: self.precommits.len(),
        }
    }
}

impl FinalityCert {
    pub fn verify(&self, snapshot: &ValidatorSetSnapshot) -> Result<(), String> {
        if self.set_hash != snapshot.set_hash {
            return Err("Validator set hash mismatch".into());
        }
        if self.epoch != snapshot.epoch {
            return Err("Epoch mismatch".into());
        }

        let mut voted_stake: u64 = 0;
        let mut signers_pks = Vec::new();
        for (idx, validator) in snapshot.validators.iter().enumerate() {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if byte_idx < self.bitmap.len() && (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                voted_stake += validator.stake;

                let pk_bytes: [u8; 96] =
                    validator
                        .bls_public_key
                        .as_slice()
                        .try_into()
                        .map_err(|_| {
                            format!("Invalid BLS public key length for {}", validator.address)
                        })?;
                let pk = G2Affine::from_compressed(&pk_bytes);
                if pk.is_none().into() {
                    return Err(format!(
                        "Invalid BLS public key encoding for {}",
                        validator.address
                    ));
                }
                signers_pks.push(G2Projective::from(pk.unwrap()));
            }
        }

        if voted_stake < snapshot.quorum_stake() {
            return Err(format!(
                "Insufficient quorum: {} < {} (need {}/{})",
                voted_stake,
                snapshot.quorum_stake(),
                FINALITY_QUORUM_NUMERATOR,
                FINALITY_QUORUM_DENOMINATOR
            ));
        }

        if signers_pks.is_empty() {
            return Err("No signers in bitmap".into());
        }

        // Aggregate Public Keys
        let mut agg_pk = G2Projective::identity();
        for pk in signers_pks {
            agg_pk += pk;
        }
        let agg_pk_affine = G2Affine::from(agg_pk);

        // Parse Aggregated Signature (G1)
        let sig_bytes: [u8; 48] = self
            .agg_sig_bls
            .as_slice()
            .try_into()
            .map_err(|_| "Invalid aggregated BLS signature length".to_string())?;
        let sig_affine = G1Affine::from_compressed(&sig_bytes);
        if sig_affine.is_none().into() {
            return Err("Invalid aggregated BLS signature encoding".into());
        }

        // Hash message to G1
        let msg = self.signing_message();
        let h_msg = hash_to_g1(&msg);

        // Verify pairing: e(sig, G2_gen) == e(H(msg), agg_pk)
        // Which is equivalent to: e(sig, -G2_gen) + e(H(msg), agg_pk) == 0 (identity)
        let g2_gen_neg = -G2Affine::generator();

        let pairing_result = bls12_381::multi_miller_loop(&[
            (&sig_affine.unwrap(), &g2_gen_neg.into()),
            (&h_msg, &agg_pk_affine.into()),
        ])
        .final_exponentiation();

        if pairing_result != bls12_381::Gt::identity() {
            return Err("BLS aggregate signature verification failed".into());
        }

        Ok(())
    }

    pub fn signing_message(&self) -> Vec<u8> {
        checkpoint_signing_message(self.epoch, self.checkpoint_height, &self.checkpoint_hash)
    }

    pub fn signer_count(&self, validator_count: usize) -> usize {
        let mut count = 0;
        for idx in 0..validator_count {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if byte_idx < self.bitmap.len() && (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                count += 1;
            }
        }
        count
    }

    pub fn signer_indices(&self, validator_count: usize) -> Vec<usize> {
        let mut indices = Vec::new();
        for idx in 0..validator_count {
            let byte_idx = idx / 8;
            let bit_idx = idx % 8;
            if byte_idx < self.bitmap.len() && (self.bitmap[byte_idx] & (1 << bit_idx)) != 0 {
                indices.push(idx);
            }
        }
        indices
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression test for a fixed universal-forgery vulnerability: an
    /// earlier `hash_to_g1` implementation computed `H(m) = scalar_hash(m) *
    /// G1_generator`, a public, invertible relationship between any two
    /// messages' hash points. That let anyone who observed a single valid
    /// signature rescale it (by the public ratio of the two messages'
    /// hash-derived scalars) into a valid signature over an arbitrary
    /// different message, without ever touching the secret key. With a real
    /// RFC 9380 hash-to-curve, no such public discrete-log relationship
    /// exists between H(m1) and H(m2), so the same rescaling attack must no
    /// longer produce a signature that verifies.
    #[test]
    fn hash_to_g1_forgery_attack_no_longer_succeeds() {
        let (sk, pk_bytes, _) = make_test_key(1);

        let msg1 = b"legitimate precommit for block A";
        let sig1 = sign_bls(&sk, msg1);
        let msg2 = b"malicious finalized block B";

        // Old attack: scale the observed signature by an attacker-chosen
        // scalar and hope it lands on a valid signature for msg2. With a
        // real hash-to-curve this has negligible success probability (it
        // would require solving discrete log), unlike the old scheme where
        // the exact right ratio was directly computable.
        let sig1_affine = G1Affine::from_compressed(&sig1.clone().try_into().unwrap()).unwrap();
        let arbitrary_scalar = Scalar::from(12345u64);
        let forged_sig = G1Affine::from(G1Projective::from(sig1_affine) * arbitrary_scalar)
            .to_compressed()
            .to_vec();
        assert!(verify_bls_sig(&pk_bytes, msg2, &forged_sig).is_err());

        // The real signature for msg2 must still verify normally.
        let real_sig2 = sign_bls(&sk, msg2);
        assert!(verify_bls_sig(&pk_bytes, msg2, &real_sig2).is_ok());
    }

    fn make_test_key(seed: u8) -> (Scalar, Vec<u8>, Vec<u8>) {
        let mut sk_bytes = [0u8; 64];
        sk_bytes[0] = seed + 1;
        let sk = Scalar::from_bytes_wide(&sk_bytes);

        let pk = G2Affine::from(G2Projective::generator() * sk);
        let pk_compressed = pk.to_compressed().to_vec();

        (sk, pk_compressed, vec![])
    }

    fn sign_msg(sk: Scalar, msg: &[u8]) -> Vec<u8> {
        let h_msg = hash_to_g1(msg);
        let sig = G1Affine::from(G1Projective::from(h_msg) * sk);
        sig.to_compressed().to_vec()
    }

    fn make_snapshot_with_keys(n: usize, stake_each: u64) -> (ValidatorSetSnapshot, Vec<Scalar>) {
        let mut sks = Vec::new();
        let validators: Vec<ValidatorEntry> = (0..n)
            .map(|i| {
                let (sk, pk_bytes, _) = make_test_key(i as u8);
                sks.push(sk);
                let mut addr_bytes = [0u8; 32];
                addr_bytes[0] = (i + 1) as u8;
                let addr = Address::from(addr_bytes);

                let pop_msg = pop_signing_message(&addr, &pk_bytes);
                let pop_sig = sign_msg(sk, &pop_msg);

                ValidatorEntry {
                    address: addr,
                    stake: stake_each,
                    bls_public_key: pk_bytes,
                    pop_signature: pop_sig,
                    pq_public_key: Vec::new(),
                }
            })
            .collect();
        (ValidatorSetSnapshot::new(1, validators), sks)
    }

    #[test]
    fn test_validator_set_snapshot() {
        let (snap, _) = make_snapshot_with_keys(4, 1000);
        assert_eq!(snap.total_stake, 4000);
        assert_eq!(snap.quorum_stake(), 2667);
    }

    #[test]
    fn test_verify_pop() {
        let (snap, _) = make_snapshot_with_keys(1, 1000);
        assert!(verify_pop(&snap.validators[0]));

        let mut invalid = snap.validators[0].clone();
        invalid.pop_signature[0] ^= 0xFF;
        assert!(!verify_pop(&invalid));
    }

    #[test]
    fn test_checkpoint_height() {
        assert!(!is_checkpoint_height(0));
        assert!(is_checkpoint_height(10));
    }

    #[test]
    fn test_prevote_signing_message() {
        let vote = Prevote {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "abc".into(),
            voter_id: Address::zero(),
            sig_bls: vec![],
        };
        let msg = vote.signing_message();
        assert!(msg.starts_with(b"BUDLUM_PREVOTE"));
    }

    #[test]
    fn test_aggregator_prevote_flow() {
        let (snap, _) = make_snapshot_with_keys(4, 1000);
        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snap);

        for i in 0..3 {
            let mut addr_bytes = [0u8; 32];
            addr_bytes[0] = (i + 1) as u8;
            let vote = Prevote {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: Address::from(addr_bytes),
                sig_bls: vec![i as u8; 48],
            };
            agg.add_prevote(vote).unwrap();
        }
        assert!(agg.prevote_quorum_reached);
    }

    #[test]
    fn test_aggregator_rejects_duplicate() {
        let (snap, _) = make_snapshot_with_keys(4, 1000);
        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snap);

        let mut addr_bytes = [0u8; 32];
        addr_bytes[0] = 1;
        let vote = Prevote {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: Address::from(addr_bytes),
            sig_bls: vec![0; 48],
        };
        agg.add_prevote(vote.clone()).unwrap();
        assert!(agg.add_prevote(vote).is_err());
    }

    #[test]
    fn test_aggregator_rejects_wrong_epoch() {
        let (snap, _) = make_snapshot_with_keys(4, 1000);
        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snap);

        let mut addr_bytes = [0u8; 32];
        addr_bytes[0] = 1;
        let vote = Prevote {
            epoch: 99,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: Address::from(addr_bytes),
            sig_bls: vec![0; 48],
        };
        assert!(agg.add_prevote(vote).is_err());
    }

    #[test]
    fn test_precommit_requires_prevote_quorum() {
        let (snap, _) = make_snapshot_with_keys(4, 1000);
        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snap);

        let mut addr_bytes = [0u8; 32];
        addr_bytes[0] = 1;
        let pc = Precommit {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: Address::from(addr_bytes),
            sig_bls: vec![0; 48],
        };
        assert!(agg.add_precommit(pc).is_err());
    }

    #[test]
    fn test_full_finality_flow() {
        let (snap, sks) = make_snapshot_with_keys(4, 1000);
        let mut agg = FinalityAggregator::new(1, 10, "cp_hash".into());
        agg.set_validator_snapshot(snap.clone());

        for i in 0..3 {
            let vote = Prevote {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: snap.validators[i].address,
                sig_bls: vec![],
            };
            agg.add_prevote(vote).unwrap();
        }
        assert!(agg.prevote_quorum_reached);

        let mut agg_sig = G1Projective::identity();
        for (i, sk) in sks.iter().enumerate().take(3) {
            let pc = Precommit {
                epoch: 1,
                checkpoint_height: 10,
                checkpoint_hash: "cp_hash".into(),
                voter_id: snap.validators[i].address,
                sig_bls: vec![],
            };

            let sig_bytes = sign_msg(*sk, &pc.signing_message());
            let mut pc_signed = pc;
            pc_signed.sig_bls = sig_bytes.clone();

            agg.add_precommit(pc_signed).unwrap();

            let sig_affine = G1Affine::from_compressed(&sig_bytes.try_into().unwrap()).unwrap();
            agg_sig += G1Projective::from(sig_affine);
        }
        assert!(agg.precommit_quorum_reached);

        let mut cert = agg.try_produce_cert().expect("Should produce cert");
        cert.agg_sig_bls = G1Affine::from(agg_sig).to_compressed().to_vec();

        assert_eq!(cert.epoch, 1);
        assert_eq!(cert.checkpoint_height, 10);
        assert_eq!(cert.checkpoint_hash, "cp_hash");
        assert_eq!(cert.set_hash, snap.set_hash);
        assert_eq!(cert.signer_count(4), 3);

        assert!(cert.verify(&snap).is_ok());
    }

    #[test]
    fn test_cert_verify_rejects_insufficient_quorum() {
        let (snap, sks) = make_snapshot_with_keys(4, 1000);
        let pc = Precommit {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            voter_id: snap.validators[0].address,
            sig_bls: vec![],
        };
        let sig_bytes = sign_msg(sks[0], &pc.signing_message());

        let cert = FinalityCert {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            agg_sig_bls: sig_bytes,
            bitmap: vec![0b0000_0001],
            set_hash: snap.set_hash.clone(),
        };
        let result = cert.verify(&snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Insufficient quorum"));
    }

    #[test]
    fn test_cert_verify_rejects_wrong_set_hash() {
        let (snap, _) = make_snapshot_with_keys(4, 1000);
        let cert = FinalityCert {
            epoch: 1,
            checkpoint_height: 10,
            checkpoint_hash: "cp_hash".into(),
            agg_sig_bls: vec![1; 48],
            bitmap: vec![0b0000_1111],
            set_hash: "wrong_hash".into(),
        };
        let result = cert.verify(&snap);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("set hash mismatch"));
    }
}
