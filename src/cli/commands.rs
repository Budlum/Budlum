use clap::Parser;
use std::path::Path;
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum ConsensusType {
    #[value(name = "pow")]
    PoW,
    #[value(name = "pos")]
    PoS,
    #[value(name = "poa")]
    PoA,
}
impl std::fmt::Display for ConsensusType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConsensusType::PoW => write!(f, "PoW (Proof of Work)"),
            ConsensusType::PoS => write!(f, "PoS (Proof of Stake)"),
            ConsensusType::PoA => write!(f, "PoA (Proof of Authority)"),
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum PrivacyLevel {
    #[value(name = "none")]
    None,
    #[value(name = "stealth")]
    Stealth,
    #[value(name = "confidential")]
    Confidential,
    #[value(name = "full")]
    Full,
}
impl std::fmt::Display for PrivacyLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrivacyLevel::None => write!(f, "None (Public)"),
            PrivacyLevel::Stealth => write!(f, "Stealth Addresses"),
            PrivacyLevel::Confidential => write!(f, "Confidential Transactions"),
            PrivacyLevel::Full => write!(f, "Full Privacy"),
        }
    }
}
#[derive(Parser, Debug)]
#[command(name = "budlum-core")]
#[command(about = "Budlum privacy-focused blockchain node")]
pub struct NodeConfig {
    #[arg(long, default_value = "pow")]
    pub consensus: ConsensusType,
    #[arg(long, default_value = "2")]
    pub difficulty: usize,
    #[arg(long, default_value = "1000")]
    pub min_stake: u64,
    #[arg(long, default_value = "none")]
    pub privacy: PrivacyLevel,
    #[arg(long, default_value = "11")]
    pub ring_size: usize,
    #[arg(long, default_value = "4001")]
    pub port: u16,
    #[arg(long)]
    pub bootstrap: Option<String>,
    #[arg(long, default_value = "./data/budlum.db")]
    pub db_path: String,
    #[arg(long, default_value = "./validators.json")]
    pub validators_file: String,
    #[arg(long)]
    pub validator_address: Option<String>,
    #[arg(long)]
    pub dial: Option<String>,
    #[arg(long, default_value = "1337")]
    pub chain_id: u64,
    #[arg(long)]
    pub validator_key_file: Option<String>,
    #[arg(long)]
    pub gen_key: Option<String>,
}
impl NodeConfig {
    pub fn load_validators(&self) -> Vec<String> {
        let path = Path::new(&self.validators_file);
        if !path.exists() {
            println!(" Validators file not found: {}", self.validators_file);
            return vec![];
        }
        match std::fs::read_to_string(path) {
            Ok(content) => match serde_json::from_str::<ValidatorsConfig>(&content) {
                Ok(config) => {
                    println!(
                        "Loaded {} validators from {}",
                        config.validators.len(),
                        self.validators_file
                    );
                    config.validators
                }
                Err(e) => {
                    println!("Failed to parse validators file: {}", e);
                    vec![]
                }
            },
            Err(e) => {
                println!("Failed to read validators file: {}", e);
                vec![]
            }
        }
    }
}
#[derive(Debug, serde::Deserialize)]
struct ValidatorsConfig {
    validators: Vec<String>,
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_consensus_type_parsing() {
        assert_eq!(ConsensusType::PoW as u8, 0);
    }
}
