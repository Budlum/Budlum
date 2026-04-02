use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::core::address::Address;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalType {
    ChangeBaseFee(u64),
    ChangeBlockReward(u64),
    SlashValidator(Address),
    ParameterUpdate(String, String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ProposalStatus {
    Active,
    Passed,
    Failed,
    Executed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub p_type: ProposalType,
    pub start_epoch: u64,
    pub end_epoch: u64,
    pub votes_for: u64,     // Total stake voting FOR
    pub votes_against: u64, // Total stake voting AGAINST
    pub status: ProposalStatus,
    pub voters: HashMap<Address, bool>, // Address -> Vote (true = for)
}

impl Proposal {
    pub fn new(id: u64, proposer: Address, p_type: ProposalType, start_epoch: u64, duration: u64) -> Self {
        Proposal {
            id,
            proposer,
            p_type,
            start_epoch,
            end_epoch: start_epoch + duration,
            votes_for: 0,
            votes_against: 0,
            status: ProposalStatus::Active,
            voters: HashMap::new(),
        }
    }

    pub fn add_vote(&mut self, voter: Address, stake: u64, vote_for: bool) -> Result<(), String> {
        if self.status != ProposalStatus::Active {
            return Err("Proposal is not active".into());
        }
        if self.voters.contains_key(&voter) {
            return Err("Already voted".into());
        }

        if vote_for {
            self.votes_for = self.votes_for.saturating_add(stake);
        } else {
            self.votes_against = self.votes_against.saturating_add(stake);
        }
        self.voters.insert(voter, vote_for);
        Ok(())
    }

    pub fn finalize(&mut self, total_stake: u64, quorum_pct: u64) {
        let reached_quorum = (self.votes_for + self.votes_against) * 100 >= total_stake * quorum_pct;
        if reached_quorum && self.votes_for > self.votes_against {
            self.status = ProposalStatus::Passed;
        } else {
            self.status = ProposalStatus::Failed;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GovernanceState {
    pub proposals: Vec<Proposal>,
    pub next_proposal_id: u64,
}

impl GovernanceState {
    pub fn create_proposal(&mut self, proposer: Address, p_type: ProposalType, current_epoch: u64, duration: u64) -> u64 {
        let id = self.next_proposal_id;
        let proposal = Proposal::new(id, proposer, p_type, current_epoch, duration);
        self.proposals.push(proposal);
        self.next_proposal_id += 1;
        id
    }

    pub fn find_proposal_mut(&mut self, id: u64) -> Option<&mut Proposal> {
        self.proposals.iter_mut().find(|p| p.id == id)
    }

    pub fn active_proposals(&self) -> Vec<&Proposal> {
        self.proposals.iter().filter(|p| p.status == ProposalStatus::Active).collect()
    }
}
