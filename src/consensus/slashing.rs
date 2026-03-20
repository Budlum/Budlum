use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum SlashingType {
    DoubleSign,
    DoubleProposal,
    DoubleVote,
    Downtime,
    InvalidBlock,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlashingEvidence {
    pub offense_type: SlashingType,
    pub validator: String,
    pub height: u64,
    pub epoch: u64,
    pub slot: u64,
    pub block_hash_1: Option<String>,
    pub block_hash_2: Option<String>,
    pub signature_1: Option<Vec<u8>>,
    pub signature_2: Option<Vec<u8>>,
    pub vrf_output_1: Option<Vec<u8>>,
    pub vrf_output_2: Option<Vec<u8>>,
    pub checkpoint_hash_1: Option<String>,
    pub checkpoint_hash_2: Option<String>,
    pub timestamp: u128,
    pub reporter: String,
}
impl SlashingEvidence {
    pub fn double_sign(
        validator: String,
        height: u64,
        block_hash_1: String,
        block_hash_2: String,
        signature_1: Vec<u8>,
        signature_2: Vec<u8>,
        reporter: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            offense_type: SlashingType::DoubleSign,
            validator,
            height,
            epoch: 0,
            slot: 0,
            block_hash_1: Some(block_hash_1),
            block_hash_2: Some(block_hash_2),
            signature_1: Some(signature_1),
            signature_2: Some(signature_2),
            vrf_output_1: None,
            vrf_output_2: None,
            checkpoint_hash_1: None,
            checkpoint_hash_2: None,
            timestamp,
            reporter,
        }
    }

    pub fn double_proposal(
        validator: String,
        epoch: u64,
        slot: u64,
        block_hash_1: String,
        block_hash_2: String,
        signature_1: Vec<u8>,
        signature_2: Vec<u8>,
        vrf_output_1: Vec<u8>,
        vrf_output_2: Vec<u8>,
        reporter: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            offense_type: SlashingType::DoubleProposal,
            validator,
            height: slot,
            epoch,
            slot,
            block_hash_1: Some(block_hash_1),
            block_hash_2: Some(block_hash_2),
            signature_1: Some(signature_1),
            signature_2: Some(signature_2),
            vrf_output_1: Some(vrf_output_1),
            vrf_output_2: Some(vrf_output_2),
            checkpoint_hash_1: None,
            checkpoint_hash_2: None,
            timestamp,
            reporter,
        }
    }
    pub fn downtime(validator: String, height: u64, reporter: String) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            offense_type: SlashingType::Downtime,
            validator,
            height,
            epoch: 0,
            slot: 0,
            block_hash_1: None,
            block_hash_2: None,
            signature_1: None,
            signature_2: None,
            vrf_output_1: None,
            vrf_output_2: None,
            checkpoint_hash_1: None,
            checkpoint_hash_2: None,
            timestamp,
            reporter,
        }
    }

    pub fn double_vote(
        validator: String,
        epoch: u64,
        checkpoint_hash_1: String,
        checkpoint_hash_2: String,
        sig_bls_1: Vec<u8>,
        sig_bls_2: Vec<u8>,
        reporter: String,
    ) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis();
        SlashingEvidence {
            offense_type: SlashingType::DoubleVote,
            validator,
            height: 0,
            epoch,
            slot: 0,
            block_hash_1: None,
            block_hash_2: None,
            signature_1: Some(sig_bls_1),
            signature_2: Some(sig_bls_2),
            vrf_output_1: None,
            vrf_output_2: None,
            checkpoint_hash_1: Some(checkpoint_hash_1),
            checkpoint_hash_2: Some(checkpoint_hash_2),
            timestamp,
            reporter,
        }
    }
    pub fn verify_double_sign(&self) -> Result<(), String> {
        if self.offense_type != SlashingType::DoubleSign {
            return Err("Wrong offense type".to_string());
        }
        let hash1 = self.block_hash_1.as_ref().ok_or("Missing block_hash_1")?;
        let hash2 = self.block_hash_2.as_ref().ok_or("Missing block_hash_2")?;
        if hash1 == hash2 {
            return Err("Block hashes are identical".to_string());
        }
        let sig1 = self.signature_1.as_ref().ok_or("Missing signature_1")?;
        let sig2 = self.signature_2.as_ref().ok_or("Missing signature_2")?;
        if sig1 == sig2 {
            return Err("Signatures are identical".to_string());
        }
        let pubkey_bytes =
            hex::decode(&self.validator).map_err(|e| format!("Invalid validator pubkey: {}", e))?;
        if pubkey_bytes.len() != 32 {
            return Err("Invalid validator pubkey length".to_string());
        }
        Ok(())
    }

    pub fn verify_double_proposal(&self) -> Result<(), String> {
        if self.offense_type != SlashingType::DoubleProposal {
            return Err("Wrong offense type".to_string());
        }
        let hash1 = self.block_hash_1.as_ref().ok_or("Missing block_hash_1")?;
        let hash2 = self.block_hash_2.as_ref().ok_or("Missing block_hash_2")?;
        if hash1 == hash2 {
            return Err("Block hashes are identical — not a double proposal".to_string());
        }
        self.signature_1.as_ref().ok_or("Missing signature_1")?;
        self.signature_2.as_ref().ok_or("Missing signature_2")?;
        let vrf1 = self.vrf_output_1.as_ref().ok_or("Missing vrf_output_1")?;
        let vrf2 = self.vrf_output_2.as_ref().ok_or("Missing vrf_output_2")?;
        if vrf1 != vrf2 {
            return Err(
                "VRF outputs differ — same slot should produce same VRF output".to_string(),
            );
        }
        if self.epoch == 0 && self.slot == 0 {
            return Err("Invalid epoch/slot for double proposal".to_string());
        }
        Ok(())
    }

    pub fn verify_double_vote(&self) -> Result<(), String> {
        if self.offense_type != SlashingType::DoubleVote {
            return Err("Wrong offense type".to_string());
        }
        let ch1 = self
            .checkpoint_hash_1
            .as_ref()
            .ok_or("Missing checkpoint_hash_1")?;
        let ch2 = self
            .checkpoint_hash_2
            .as_ref()
            .ok_or("Missing checkpoint_hash_2")?;
        if ch1 == ch2 {
            return Err("Checkpoint hashes are identical — not a double vote".to_string());
        }
        self.signature_1.as_ref().ok_or("Missing BLS signature_1")?;
        self.signature_2.as_ref().ok_or("Missing BLS signature_2")?;
        if self.epoch == 0 {
            return Err("Invalid epoch for double vote evidence".to_string());
        }
        Ok(())
    }

    pub fn slash_amount(&self, stake: u64) -> u64 {
        match self.offense_type {
            SlashingType::DoubleSign => stake,
            SlashingType::DoubleProposal => stake,
            SlashingType::DoubleVote => stake,
            SlashingType::Downtime => stake / 10,
            SlashingType::InvalidBlock => stake / 2,
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_double_sign_evidence() {
        let evidence = SlashingEvidence::double_sign(
            "abc123".repeat(5),
            100,
            "hash1".to_string(),
            "hash2".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            "reporter_pubkey".to_string(),
        );
        assert_eq!(evidence.offense_type, SlashingType::DoubleSign);
        assert_eq!(evidence.height, 100);
        assert!(evidence.block_hash_1.is_some());
        assert!(evidence.block_hash_2.is_some());
    }

    #[test]
    fn test_double_proposal_evidence() {
        let vrf_out = vec![42u8; 32];
        let evidence = SlashingEvidence::double_proposal(
            "ab".repeat(32),
            5,
            50,
            "hash_a".to_string(),
            "hash_b".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            vrf_out.clone(),
            vrf_out.clone(),
            "reporter".to_string(),
        );
        assert_eq!(evidence.offense_type, SlashingType::DoubleProposal);
        assert_eq!(evidence.epoch, 5);
        assert_eq!(evidence.slot, 50);
        assert!(evidence.verify_double_proposal().is_ok());
        assert_eq!(evidence.slash_amount(1000), 1000);
    }

    #[test]
    fn test_double_proposal_rejects_same_hash() {
        let vrf_out = vec![42u8; 32];
        let evidence = SlashingEvidence::double_proposal(
            "ab".repeat(32),
            5,
            50,
            "same_hash".to_string(),
            "same_hash".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            vrf_out.clone(),
            vrf_out.clone(),
            "reporter".to_string(),
        );
        let result = evidence.verify_double_proposal();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("identical"));
    }

    #[test]
    fn test_double_proposal_rejects_different_vrf() {
        let evidence = SlashingEvidence::double_proposal(
            "ab".repeat(32),
            5,
            50,
            "hash_a".to_string(),
            "hash_b".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            vec![1u8; 32],
            vec![2u8; 32],
            "reporter".to_string(),
        );
        let result = evidence.verify_double_proposal();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("VRF outputs differ"));
    }

    #[test]
    fn test_slash_amounts() {
        let evidence =
            SlashingEvidence::downtime("validator".to_string(), 50, "reporter".to_string());
        assert_eq!(evidence.slash_amount(1000), 100);
    }

    #[test]
    fn test_verify_double_sign_requires_different_hashes() {
        let evidence = SlashingEvidence {
            offense_type: SlashingType::DoubleSign,
            validator: "a".repeat(64),
            height: 100,
            epoch: 0,
            slot: 0,
            block_hash_1: Some("hash".to_string()),
            block_hash_2: Some("hash".to_string()),
            signature_1: Some(vec![1, 2, 3]),
            signature_2: Some(vec![4, 5, 6]),
            vrf_output_1: None,
            vrf_output_2: None,
            checkpoint_hash_1: None,
            checkpoint_hash_2: None,
            timestamp: 0,
            reporter: "reporter".to_string(),
        };
        let result = evidence.verify_double_sign();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("identical"));
    }

    #[test]
    fn test_double_vote_evidence() {
        let evidence = SlashingEvidence::double_vote(
            "ab".repeat(32),
            10,
            "checkpoint_a".to_string(),
            "checkpoint_b".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            "reporter".to_string(),
        );
        assert_eq!(evidence.offense_type, SlashingType::DoubleVote);
        assert_eq!(evidence.epoch, 10);
        assert!(evidence.verify_double_vote().is_ok());
        assert_eq!(evidence.slash_amount(1000), 1000);
    }

    #[test]
    fn test_double_vote_rejects_same_checkpoint() {
        let evidence = SlashingEvidence::double_vote(
            "ab".repeat(32),
            10,
            "same_cp".to_string(),
            "same_cp".to_string(),
            vec![1, 2, 3],
            vec![4, 5, 6],
            "reporter".to_string(),
        );
        let result = evidence.verify_double_vote();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("identical"));
    }
}
