use crate::chain::finality::{FinalityCert, ValidatorSetSnapshot};
use crate::core::block::Block;
use crate::domain::types::{ConsensusDomain, DomainCommitment, Hash32};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum FinalityStatus {
    Pending {
        required_depth: u64,
        observed_depth: u64,
    },
    Finalized,
    Rejected(String),
}

/// A single proof-of-work header submitted as evidence of chain depth on top
/// of a committed block. Verified independently: its own hash must satisfy
/// its own claimed `target`, its `target` must not be easier than the
/// domain's registered `min_pow_target` floor, and (for headers after the
/// first) it must link to the previous header's hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PoWHeaderProof {
    pub prev_hash: Hash32,
    /// Maximum allowed hash value for this header (big-endian; smaller =
    /// harder). The header is only valid if `hash() <= target`.
    pub target: Hash32,
    pub nonce: u64,
    pub timestamp_ms: u128,
    /// On the base header (index 0), must equal the commitment's
    /// `commitment_payload_hash()` — this is what binds the proof-of-work
    /// chain to the specific commitment being finalized, including its
    /// claimed `state_root`, not just its raw `domain_block_hash`.
    /// Confirmation headers built on top may leave this zeroed.
    pub extra: Hash32,
}

const POW_HEADER_HASH_DOMAIN: &[u8] = b"BDLM_POW_HEADER_V1";

impl PoWHeaderProof {
    pub fn hash(&self) -> Hash32 {
        crate::core::hash::hash_fields_bytes(&[
            POW_HEADER_HASH_DOMAIN,
            &self.prev_hash,
            &self.target,
            &self.nonce.to_le_bytes(),
            &self.timestamp_ms.to_le_bytes(),
            &self.extra,
        ])
    }

    pub fn meets_own_target(&self) -> bool {
        self.hash() <= self.target
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinalityProof {
    /// A chain of real proof-of-work headers, `headers[0]` being the header
    /// that mines the committed block and each subsequent header extending
    /// it. Depth (`headers.len() - 1`) is the confirmation count.
    PoW {
        headers: Vec<PoWHeaderProof>,
    },
    PoS {
        cert: FinalityCert,
        validator_snapshot: ValidatorSetSnapshot,
    },
    /// Reuses the same BLS aggregate-signature machinery as PoS: a quorum of
    /// the domain's registered validator set signs off on the commitment.
    PoA {
        cert: FinalityCert,
        validator_snapshot: ValidatorSetSnapshot,
    },
    /// Same BLS aggregate-signature verification as PoA/PoS.
    Bft {
        cert: FinalityCert,
        validator_snapshot: ValidatorSetSnapshot,
    },
    Zk {
        proof_hash: Hash32,
        verifier_key_hash: Hash32,
        public_inputs_hash: Hash32,
    },
    Raw(Vec<u8>),
}

#[derive(Debug, Clone)]
pub struct FinalityError(pub String);

impl std::fmt::Display for FinalityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Finality error: {}", self.0)
    }
}

impl std::error::Error for FinalityError {}

pub trait DomainFinalityAdapter: Send + Sync {
    fn adapter_name(&self) -> &'static str;

    fn verify_finality(
        &self,
        domain: &ConsensusDomain,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError>;
}

#[derive(Debug, Clone)]
pub struct PoWFinalityAdapter {
    pub default_min_confirmations: u64,
}

impl Default for PoWFinalityAdapter {
    fn default() -> Self {
        Self {
            default_min_confirmations: 64,
        }
    }
}

impl DomainFinalityAdapter for PoWFinalityAdapter {
    fn adapter_name(&self) -> &'static str {
        "pow-confirmation-depth"
    }

    fn verify_finality(
        &self,
        domain: &ConsensusDomain,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::PoW { headers } = proof else {
            return Err(FinalityError("Expected PoW finality proof".into()));
        };

        let Some(base) = headers.first() else {
            return Ok(FinalityStatus::Rejected(
                "PoW proof must include at least one header".into(),
            ));
        };

        if base.extra != commitment.commitment_payload_hash() {
            return Ok(FinalityStatus::Rejected(
                "Base PoW header does not commit to this block".into(),
            ));
        }

        for (i, header) in headers.iter().enumerate() {
            if header.target > domain.min_pow_target {
                return Ok(FinalityStatus::Rejected(format!(
                    "PoW header {} is below the domain's required difficulty",
                    i
                )));
            }
            if !header.meets_own_target() {
                return Ok(FinalityStatus::Rejected(format!(
                    "PoW header {} hash does not satisfy its own claimed target",
                    i
                )));
            }
            if i > 0 && header.prev_hash != headers[i - 1].hash() {
                return Ok(FinalityStatus::Rejected(format!(
                    "PoW header {} does not link to the previous header",
                    i
                )));
            }
        }

        let confirmations = (headers.len() - 1) as u64;
        let min_depth = domain.min_confirmations.max(self.default_min_confirmations);
        if confirmations >= min_depth {
            Ok(FinalityStatus::Finalized)
        } else {
            Ok(FinalityStatus::Pending {
                required_depth: min_depth,
                observed_depth: confirmations,
            })
        }
    }
}

/// Shared BLS quorum-certificate verification used by PoS, PoA, and BFT
/// adapters: a quorum of the domain's registered validator set must have
/// signed off on this exact commitment.
fn verify_quorum_cert(
    domain: &ConsensusDomain,
    commitment: &DomainCommitment,
    cert: &FinalityCert,
    validator_snapshot: &ValidatorSetSnapshot,
    adapter_label: &str,
) -> Result<FinalityStatus, FinalityError> {
    if cert.checkpoint_height != commitment.domain_height {
        return Ok(FinalityStatus::Rejected(format!(
            "{} cert height does not match commitment",
            adapter_label
        )));
    }

    let commitment_hash = hex::encode(commitment.commitment_payload_hash());
    if cert.checkpoint_hash != commitment_hash {
        return Ok(FinalityStatus::Rejected(format!(
            "{} cert hash does not match commitment",
            adapter_label
        )));
    }

    if validator_snapshot.set_hash != cert.set_hash {
        return Ok(FinalityStatus::Rejected(format!(
            "{} cert set hash does not match validator snapshot",
            adapter_label
        )));
    }

    // Registration guarantees domain.validator_set_hash is non-zero for
    // PoS/PoA/BFT domains (see validate_consensus_domain_registration), so
    // this binding is always enforced — there is no bypass path left for an
    // attacker to substitute their own throwaway validator set.
    match hex::decode(&validator_snapshot.set_hash) {
        Ok(decoded_set_hash) if decoded_set_hash.len() == 32 => {
            let mut snapshot_set_hash = [0u8; 32];
            snapshot_set_hash.copy_from_slice(&decoded_set_hash);
            if snapshot_set_hash != domain.validator_set_hash {
                return Ok(FinalityStatus::Rejected(format!(
                    "{} validator snapshot does not match registered domain set",
                    adapter_label
                )));
            }
            if commitment.validator_set_hash != [0u8; 32]
                && commitment.validator_set_hash != snapshot_set_hash
            {
                return Ok(FinalityStatus::Rejected(format!(
                    "{} commitment validator set does not match finality proof",
                    adapter_label
                )));
            }
        }
        _ => {
            return Ok(FinalityStatus::Rejected(format!(
                "{} validator snapshot set_hash is not a valid 32-byte hex hash",
                adapter_label
            )));
        }
    }

    cert.verify(validator_snapshot)
        .map_err(|e| FinalityError(format!("Invalid {} finality cert: {}", adapter_label, e)))?;

    Ok(FinalityStatus::Finalized)
}

#[derive(Debug, Clone, Default)]
pub struct PoSFinalityAdapter;

impl DomainFinalityAdapter for PoSFinalityAdapter {
    fn adapter_name(&self) -> &'static str {
        "pos-qc-finality"
    }

    fn verify_finality(
        &self,
        domain: &ConsensusDomain,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::PoS {
            cert,
            validator_snapshot,
        } = proof
        else {
            return Err(FinalityError("Expected PoS finality proof".into()));
        };
        verify_quorum_cert(domain, commitment, cert, validator_snapshot, "PoS")
    }
}

#[derive(Debug, Clone)]
pub struct PoAFinalityAdapter {
    pub quorum_numerator: u64,
    pub quorum_denominator: u64,
}

impl Default for PoAFinalityAdapter {
    fn default() -> Self {
        Self {
            quorum_numerator: 2,
            quorum_denominator: 3,
        }
    }
}

impl DomainFinalityAdapter for PoAFinalityAdapter {
    fn adapter_name(&self) -> &'static str {
        "poa-authority-quorum"
    }

    fn verify_finality(
        &self,
        domain: &ConsensusDomain,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::PoA {
            cert,
            validator_snapshot,
        } = proof
        else {
            return Err(FinalityError("Expected PoA finality proof".into()));
        };
        verify_quorum_cert(domain, commitment, cert, validator_snapshot, "PoA")
    }
}

#[derive(Debug, Clone)]
pub struct BftFinalityAdapter {
    pub quorum_numerator: u64,
    pub quorum_denominator: u64,
}

impl Default for BftFinalityAdapter {
    fn default() -> Self {
        Self {
            quorum_numerator: 2,
            quorum_denominator: 3,
        }
    }
}

impl DomainFinalityAdapter for BftFinalityAdapter {
    fn adapter_name(&self) -> &'static str {
        "bft-quorum-commit"
    }

    fn verify_finality(
        &self,
        domain: &ConsensusDomain,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::Bft {
            cert,
            validator_snapshot,
        } = proof
        else {
            return Err(FinalityError("Expected BFT finality proof".into()));
        };
        verify_quorum_cert(domain, commitment, cert, validator_snapshot, "BFT")
    }
}

#[derive(Debug, Clone, Default)]
pub struct ZkFinalityAdapter;

impl DomainFinalityAdapter for ZkFinalityAdapter {
    fn adapter_name(&self) -> &'static str {
        "zk-proof-verification"
    }

    fn verify_finality(
        &self,
        _domain: &ConsensusDomain,
        _commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::Zk {
            proof_hash,
            verifier_key_hash,
            public_inputs_hash,
        } = proof
        else {
            return Err(FinalityError("Expected ZK finality proof".into()));
        };

        if *proof_hash == [0u8; 32] {
            return Ok(FinalityStatus::Rejected("ZK proof hash is zero".into()));
        }
        if *verifier_key_hash == [0u8; 32] {
            return Ok(FinalityStatus::Rejected(
                "ZK verifier key hash is zero".into(),
            ));
        }
        if *public_inputs_hash == [0u8; 32] {
            return Ok(FinalityStatus::Rejected(
                "ZK public inputs hash is zero".into(),
            ));
        }

        Ok(FinalityStatus::Finalized)
    }
}

pub fn hash_finality_proof(proof: &FinalityProof) -> [u8; 32] {
    let encoded = bincode::serialize(proof).unwrap_or_default();
    crate::core::hash::hash_fields_bytes(&[b"BDLM_FINALITY_PROOF_V1", &encoded])
}

pub fn empty_event_root() -> [u8; 32] {
    crate::core::hash::hash_fields_bytes(&[b"BDLM_EMPTY_DOMAIN_EVENT_ROOT_V1"])
}

pub fn block_finality_proof_hash(_block: &Block) -> [u8; 32] {
    crate::core::hash::hash_fields_bytes(&[b"BDLM_NO_FINALITY_PROOF_YET_V1"])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::finality::FinalityCert;
    use crate::domain::plugin::default_domain;
    use crate::domain::types::{ConsensusKind, DomainCommitment};

    fn commitment(kind: ConsensusKind) -> DomainCommitment {
        DomainCommitment {
            domain_id: 1,
            domain_height: 10,
            domain_block_hash: [1u8; 32],
            parent_domain_block_hash: [0u8; 32],
            state_root: [2u8; 32],
            tx_root: [3u8; 32],
            event_root: [4u8; 32],
            finality_proof_hash: [5u8; 32],
            consensus_kind: kind,
            validator_set_hash: [6u8; 32],
            timestamp_ms: 123,
            sequence: 0,
            producer: None,
            state_updates: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn pow_finality_requires_confirmation_depth_and_rejects_wrong_proof() {
        let domain = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 3);
        let commitment = commitment(ConsensusKind::PoW);
        let adapter = PoWFinalityAdapter {
            default_min_confirmations: 3,
        };

        let short_chain = crate::tests::finality_proof_support::mine_pow_chain(
            commitment.commitment_payload_hash(),
            domain.min_pow_target,
            2,
        );
        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoW {
                        headers: short_chain,
                    },
                )
                .unwrap(),
            FinalityStatus::Pending {
                required_depth: 3,
                observed_depth: 2,
            }
        );

        let full_chain = crate::tests::finality_proof_support::mine_pow_chain(
            commitment.commitment_payload_hash(),
            domain.min_pow_target,
            3,
        );
        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoW {
                        headers: full_chain,
                    },
                )
                .unwrap(),
            FinalityStatus::Finalized
        );

        // A header that doesn't actually satisfy its own claimed target is a
        // forged proof, not just "not enough confirmations yet".
        let mut forged = crate::tests::finality_proof_support::mine_pow_chain(
            commitment.commitment_payload_hash(),
            domain.min_pow_target,
            3,
        );
        forged[0].nonce = forged[0].nonce.wrapping_add(1);
        assert!(matches!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoW { headers: forged }
                )
                .unwrap(),
            FinalityStatus::Rejected(_)
        ));

        let snapshot = ValidatorSetSnapshot::new(0, vec![]);
        assert!(adapter
            .verify_finality(
                &domain,
                &commitment,
                &FinalityProof::PoA {
                    cert: FinalityCert {
                        epoch: 0,
                        checkpoint_height: 0,
                        checkpoint_hash: String::new(),
                        agg_sig_bls: vec![],
                        bitmap: vec![],
                        set_hash: snapshot.set_hash.clone(),
                    },
                    validator_snapshot: snapshot,
                },
            )
            .is_err());
    }

    #[test]
    fn poa_finality_enforces_quorum_and_empty_validator_set_rejection() {
        let commitment = DomainCommitment {
            domain_height: 10,
            validator_set_hash: [0u8; 32],
            ..commitment(ConsensusKind::PoA)
        };
        let (snapshot, keys) = crate::tests::finality_proof_support::make_validator_set(4, 100);
        let mut domain = default_domain(2, ConsensusKind::PoA, 1337, "poa-authority-quorum", 0);
        domain.validator_set_hash =
            crate::tests::finality_proof_support::snapshot_domain_hash(&snapshot);
        let adapter = PoAFinalityAdapter::default();

        let full_cert = crate::tests::finality_proof_support::sign_quorum_cert(
            commitment.domain_height,
            commitment.commitment_payload_hash(),
            &snapshot,
            &keys,
            &[0, 1, 2, 3],
        );
        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoA {
                        cert: full_cert,
                        validator_snapshot: snapshot.clone(),
                    },
                )
                .unwrap(),
            FinalityStatus::Finalized
        );

        // Only 1 of 4 validators signed: below the 2/3 quorum, real BLS
        // verification must reject it rather than trust a claimed count.
        let short_cert = crate::tests::finality_proof_support::sign_quorum_cert(
            commitment.domain_height,
            commitment.commitment_payload_hash(),
            &snapshot,
            &keys,
            &[0],
        );
        assert!(adapter
            .verify_finality(
                &domain,
                &commitment,
                &FinalityProof::PoA {
                    cert: short_cert,
                    validator_snapshot: snapshot,
                },
            )
            .is_err());

        let empty_snapshot = ValidatorSetSnapshot::new(0, vec![]);
        assert!(matches!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoA {
                        cert: FinalityCert {
                            epoch: 0,
                            checkpoint_height: commitment.domain_height,
                            checkpoint_hash: hex::encode(commitment.commitment_payload_hash()),
                            agg_sig_bls: vec![],
                            bitmap: vec![],
                            set_hash: empty_snapshot.set_hash.clone(),
                        },
                        validator_snapshot: empty_snapshot,
                    },
                )
                .unwrap(),
            FinalityStatus::Rejected(_)
        ));
    }

    #[test]
    fn pos_finality_rejects_mismatched_height_or_hash_before_signature_work() {
        let domain = default_domain(3, ConsensusKind::PoS, 1337, "pos-qc-finality", 0);
        let commitment = commitment(ConsensusKind::PoS);
        let adapter = PoSFinalityAdapter;
        let snapshot = ValidatorSetSnapshot::new(0, vec![]);

        let wrong_height = FinalityCert {
            epoch: 0,
            checkpoint_height: 9,
            checkpoint_hash: hex::encode(commitment.commitment_payload_hash()),
            agg_sig_bls: vec![],
            bitmap: vec![],
            set_hash: snapshot.set_hash.clone(),
        };
        assert!(matches!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoS {
                        cert: wrong_height,
                        validator_snapshot: snapshot.clone(),
                    },
                )
                .unwrap(),
            FinalityStatus::Rejected(_)
        ));

        let wrong_hash = FinalityCert {
            epoch: 0,
            checkpoint_height: commitment.domain_height,
            checkpoint_hash: "ff".repeat(32),
            agg_sig_bls: vec![],
            bitmap: vec![],
            set_hash: snapshot.set_hash.clone(),
        };
        assert!(matches!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoS {
                        cert: wrong_hash,
                        validator_snapshot: snapshot,
                    },
                )
                .unwrap(),
            FinalityStatus::Rejected(_)
        ));
    }
}
