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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FinalityProof {
    PoW {
        confirmations: u64,
        total_work_hint: u128,
    },
    PoS {
        cert: FinalityCert,
        validator_snapshot: ValidatorSetSnapshot,
    },
    PoA {
        signer_count: u64,
        validator_count: u64,
    },
    Bft {
        round: u64,
        signer_count: u64,
        total_validators: u64,
        commit_hash: Hash32,
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
        _commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let min_depth = domain.min_confirmations.max(self.default_min_confirmations);
        match proof {
            FinalityProof::PoW { confirmations, .. } if *confirmations >= min_depth => {
                Ok(FinalityStatus::Finalized)
            }
            FinalityProof::PoW { confirmations, .. } => Ok(FinalityStatus::Pending {
                required_depth: min_depth,
                observed_depth: *confirmations,
            }),
            _ => Err(FinalityError("Expected PoW finality proof".into())),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PoSFinalityAdapter;

impl DomainFinalityAdapter for PoSFinalityAdapter {
    fn adapter_name(&self) -> &'static str {
        "pos-qc-finality"
    }

    fn verify_finality(
        &self,
        _domain: &ConsensusDomain,
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

        if cert.checkpoint_height != commitment.domain_height {
            return Ok(FinalityStatus::Rejected(
                "PoS cert height does not match commitment".into(),
            ));
        }

        let commitment_hash = hex::encode(commitment.domain_block_hash);
        if cert.checkpoint_hash != commitment_hash {
            return Ok(FinalityStatus::Rejected(
                "PoS cert hash does not match commitment".into(),
            ));
        }

        cert.verify(validator_snapshot)
            .map_err(|e| FinalityError(format!("Invalid PoS finality cert: {}", e)))?;

        Ok(FinalityStatus::Finalized)
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
        _domain: &ConsensusDomain,
        _commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::PoA {
            signer_count,
            validator_count,
        } = proof
        else {
            return Err(FinalityError("Expected PoA finality proof".into()));
        };

        if *validator_count == 0 {
            return Ok(FinalityStatus::Rejected(
                "PoA validator set is empty".into(),
            ));
        }

        let required = (*validator_count * self.quorum_numerator + self.quorum_denominator - 1)
            / self.quorum_denominator;

        if *signer_count >= required {
            Ok(FinalityStatus::Finalized)
        } else {
            Ok(FinalityStatus::Pending {
                required_depth: required,
                observed_depth: *signer_count,
            })
        }
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
        _domain: &ConsensusDomain,
        commitment: &DomainCommitment,
        proof: &FinalityProof,
    ) -> Result<FinalityStatus, FinalityError> {
        let FinalityProof::Bft {
            round: _,
            signer_count,
            total_validators,
            commit_hash,
        } = proof
        else {
            return Err(FinalityError("Expected BFT finality proof".into()));
        };

        if *total_validators == 0 {
            return Ok(FinalityStatus::Rejected(
                "BFT validator set is empty".into(),
            ));
        }

        if *commit_hash != commitment.domain_block_hash {
            return Ok(FinalityStatus::Rejected(
                "BFT commit hash does not match commitment block hash".into(),
            ));
        }

        let required = (*total_validators * self.quorum_numerator) / self.quorum_denominator + 1;
        if *signer_count >= required {
            Ok(FinalityStatus::Finalized)
        } else {
            Ok(FinalityStatus::Pending {
                required_depth: required,
                observed_depth: *signer_count,
            })
        }
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
            return Ok(FinalityStatus::Rejected(
                "ZK proof hash is zero".into(),
            ));
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
        }
    }

    #[test]
    fn pow_finality_requires_confirmation_depth_and_rejects_wrong_proof() {
        let domain = default_domain(1, ConsensusKind::PoW, 1337, "pow-confirmation-depth", 80);
        let commitment = commitment(ConsensusKind::PoW);
        let adapter = PoWFinalityAdapter::default();

        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoW {
                        confirmations: 79,
                        total_work_hint: 100,
                    },
                )
                .unwrap(),
            FinalityStatus::Pending {
                required_depth: 80,
                observed_depth: 79,
            }
        );

        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoW {
                        confirmations: 80,
                        total_work_hint: 100,
                    },
                )
                .unwrap(),
            FinalityStatus::Finalized
        );

        assert!(adapter
            .verify_finality(
                &domain,
                &commitment,
                &FinalityProof::PoA {
                    signer_count: 3,
                    validator_count: 4,
                },
            )
            .is_err());
    }

    #[test]
    fn poa_finality_enforces_quorum_and_empty_validator_set_rejection() {
        let domain = default_domain(2, ConsensusKind::PoA, 1337, "poa-authority-quorum", 0);
        let commitment = commitment(ConsensusKind::PoA);
        let adapter = PoAFinalityAdapter::default();

        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoA {
                        signer_count: 2,
                        validator_count: 4,
                    },
                )
                .unwrap(),
            FinalityStatus::Pending {
                required_depth: 3,
                observed_depth: 2,
            }
        );
        assert_eq!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoA {
                        signer_count: 3,
                        validator_count: 4,
                    },
                )
                .unwrap(),
            FinalityStatus::Finalized
        );
        assert!(matches!(
            adapter
                .verify_finality(
                    &domain,
                    &commitment,
                    &FinalityProof::PoA {
                        signer_count: 0,
                        validator_count: 0,
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
            checkpoint_hash: hex::encode(commitment.domain_block_hash),
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
