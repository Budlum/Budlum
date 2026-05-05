use crate::core::account::AccountState;
use crate::core::address::Address;
use crate::core::transaction::{Transaction, TransactionType};
use crate::error::{BudlumError, BudlumResult};
use crate::execution::zkvm::{ZkVmExecutor, DEFAULT_CONTRACT_GAS_LIMIT};

pub struct Executor;

impl Executor {
    pub fn apply_transaction(state: &mut AccountState, tx: &Transaction) -> Result<(), String> {
        Self::apply_transaction_checked(state, tx).map_err(|e| e.message().to_string())
    }

    pub fn apply_transaction_checked(
        state: &mut AccountState,
        tx: &Transaction,
    ) -> BudlumResult<()> {
        if tx.from == Address::zero() {
            return Ok(());
        }

        let total_cost = tx.total_cost();

        {
            let sender_account = state.get_or_create(&tx.from);
            if sender_account.balance < total_cost {
                return Err(BudlumError::validation(
                    "insufficient_balance",
                    "Insufficient balance",
                ));
            }
        }

        match tx.tx_type {
            TransactionType::Transfer => {
                let sender = state.get_or_create(&tx.from);
                sender.balance = sender.balance.saturating_sub(total_cost);
                sender.nonce = sender.nonce.saturating_add(1);

                let receiver = state.get_or_create(&tx.to);
                receiver.balance = receiver.balance.saturating_add(tx.amount);
            }
            TransactionType::Stake => {
                let sender = state.get_or_create(&tx.from);
                sender.balance = sender.balance.saturating_sub(total_cost);
                sender.nonce = sender.nonce.saturating_add(1);

                let stake_amount = tx.amount;
                let validator = state.get_validator_mut(&tx.from);

                if let Some(v) = validator {
                    v.stake = v.stake.saturating_add(stake_amount);
                    v.active = true;
                } else {
                    state.add_validator(tx.from, stake_amount);
                }
            }
            TransactionType::Unstake => {
                let sender_start_balance = state.get_balance(&tx.from);
                if sender_start_balance < tx.fee {
                    return Err(BudlumError::validation(
                        "insufficient_fee_balance",
                        "Insufficient balance for fee",
                    ));
                }

                if let Some(validator) = state.get_validator_mut(&tx.from) {
                    if validator.stake < tx.amount {
                        return Err(BudlumError::validation(
                            "insufficient_stake",
                            "Insufficient stake",
                        ));
                    }
                    validator.stake = validator.stake.saturating_sub(tx.amount);
                    if validator.stake == 0 {
                        validator.active = false;
                    }
                } else {
                    return Err(BudlumError::validation("not_validator", "Not a validator"));
                }

                state
                    .unbonding_queue
                    .push(crate::core::account::UnbondingEntry {
                        address: tx.from,
                        amount: tx.amount,
                        release_epoch: state.epoch_index + crate::core::account::UNBONDING_EPOCHS,
                    });

                let sender = state.get_or_create(&tx.from);
                sender.balance = sender.balance.saturating_sub(tx.fee);
                sender.nonce = sender.nonce.saturating_add(1);
            }
            TransactionType::Vote => {
                let sender_acc = state.get_or_create(&tx.from);
                sender_acc.balance = sender_acc.balance.saturating_sub(tx.fee);
                sender_acc.nonce = sender_acc.nonce.saturating_add(1);

                if tx.to != Address::zero() {
                    if let Some(target) = state.get_validator_mut(&tx.to) {
                        if tx.amount > 0 {
                            target.votes_for += 1;
                        } else {
                            target.votes_against += 1;
                        }
                        tracing::info!("Validator Vote recorded: {} -> {}", tx.from, tx.to);
                    }
                } else if !tx.data.is_empty() {
                    if tx.data.len() >= 9 {
                        if tx.data.len() == 9 {
                            let vote_for = tx.data[0] != 0;
                            let mut id_bytes = [0u8; 8];
                            id_bytes.copy_from_slice(&tx.data[1..9]);
                            let proposal_id = u64::from_le_bytes(id_bytes);

                            let voter_stake =
                                state.get_validator(&tx.from).map(|v| v.stake).unwrap_or(0);
                            if voter_stake == 0 {
                                return Err(BudlumError::validation(
                                    "governance_voter_not_validator",
                                    "Only validators can vote in governance",
                                ));
                            }

                            if let Some(proposal) = state.governance.find_proposal_mut(proposal_id)
                            {
                                proposal
                                    .add_vote(tx.from, voter_stake, vote_for)
                                    .map_err(|e| {
                                        BudlumError::validation("governance_vote_failed", e)
                                    })?;
                                tracing::info!(
                                    "Governance Vote: Proposal {} from {}",
                                    proposal_id,
                                    tx.from
                                );
                            } else {
                                return Err(BudlumError::validation(
                                    "proposal_not_found",
                                    "Proposal not found",
                                ));
                            }
                        } else {
                            // Likely a Proposal: [duration (8), ProposalType (...)]
                            let mut dur_bytes = [0u8; 8];
                            dur_bytes.copy_from_slice(&tx.data[0..8]);
                            let duration = u64::from_le_bytes(dur_bytes);

                            let p_type: crate::core::governance::ProposalType =
                                serde_json::from_slice(&tx.data[8..]).map_err(|e| {
                                    BudlumError::validation(
                                        "invalid_proposal_data",
                                        format!("Invalid proposal data: {}", e),
                                    )
                                })?;

                            let id = state.governance.create_proposal(
                                tx.from,
                                p_type,
                                state.epoch_index,
                                duration,
                            );
                            tracing::info!(
                                "Governance Proposal Created: ID {} from {}",
                                id,
                                tx.from
                            );
                        }
                    }
                }
            }
            TransactionType::ContractCall => {
                ZkVmExecutor::execute_bytecode(&tx.data, DEFAULT_CONTRACT_GAS_LIMIT)
                    .map_err(|e| BudlumError::validation("contract_execution_failed", e))?;

                let sender = state.get_or_create(&tx.from);
                sender.balance = sender.balance.saturating_sub(tx.fee);
                sender.nonce = sender.nonce.saturating_add(1);
            }
        }

        Ok(())
    }

    pub fn apply_block(
        state: &mut AccountState,
        transactions: &[Transaction],
        block_producer: Option<&Address>,
    ) -> Result<(), String> {
        Self::apply_block_checked(state, transactions, block_producer)
            .map_err(|e| e.message().to_string())
    }

    pub fn apply_block_checked(
        state: &mut AccountState,
        transactions: &[Transaction],
        block_producer: Option<&Address>,
    ) -> BudlumResult<()> {
        let mut total_fees: u64 = 0;
        for tx in transactions {
            if tx.from == Address::zero() {
                continue;
            }
            if let Err(e) = Self::apply_transaction_checked(state, tx) {
                return Err(BudlumError::validation(
                    "transaction_apply_failed",
                    format!("TX apply failed: {}", e),
                ));
            }
            total_fees = total_fees.saturating_add(tx.fee);
        }
        if let Some(producer) = block_producer {
            let reward = total_fees.saturating_add(state.block_reward);
            if reward > 0 {
                let producer_account = state.get_or_create(producer);
                producer_account.balance = producer_account.balance.saturating_add(reward);
                tracing::info!(
                    "Producer {} received reward: {} (fees: {}, block: {})",
                    producer,
                    reward,
                    total_fees,
                    state.block_reward
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::account::AccountState;
    use crate::core::transaction::{Transaction, TransactionType};
    use bud_isa::{Instruction, Opcode};

    #[test]
    fn test_apply_block_reward() {
        let mut state = AccountState::new();
        let producer = Address::from_hex(&"0".repeat(64)).unwrap();
        let txs = vec![];

        Executor::apply_block(&mut state, &txs, Some(&producer)).unwrap();

        let reward = state.block_reward;
        let account = state.get_or_create(&producer);
        assert_eq!(account.balance, reward);
    }

    #[test]
    fn test_apply_block_reward_with_fees() {
        let mut state = AccountState::new();
        let producer = Address::from_hex(&"01".repeat(32)).unwrap();
        let alice = Address::from_hex(&"02".repeat(32)).unwrap();
        state.add_balance(&alice, 100);

        let mut tx = Transaction::new(alice, Address::zero(), 10, vec![]);
        tx.fee = 5;
        tx.nonce = 0;

        Executor::apply_block(&mut state, &[tx], Some(&producer)).unwrap();

        let reward = state.block_reward;
        let producer_acc = state.get_or_create(&producer);
        assert_eq!(producer_acc.balance, reward + 5);

        let alice_acc = state.get_or_create(&alice);
        assert_eq!(alice_acc.balance, 100 - 15);
    }

    #[test]
    fn test_vote_for_transaction() {
        let mut state = AccountState::new();
        let alice = Address::from_hex(&"01".repeat(32)).unwrap();
        let val_pubkey = Address::from_hex(&"02".repeat(32)).unwrap();

        state.add_balance(&alice, 100);
        state.add_validator(val_pubkey, 1000);

        let mut tx = Transaction::new(alice, val_pubkey, 1, vec![]);
        tx.tx_type = TransactionType::Vote;
        tx.fee = 2;

        Executor::apply_transaction(&mut state, &tx).unwrap();

        let validator = state.get_validator(&val_pubkey).unwrap();
        assert_eq!(validator.votes_for, 1);
        assert_eq!(validator.votes_against, 0);

        let alice_acc = state.get_or_create(&alice);
        assert_eq!(alice_acc.balance, 98);
    }

    #[test]
    fn test_vote_against_transaction() {
        let mut state = AccountState::new();
        let alice = Address::from_hex(&"01".repeat(32)).unwrap();
        let val_pubkey = Address::from_hex(&"02".repeat(32)).unwrap();

        state.add_balance(&alice, 100);
        state.add_validator(val_pubkey, 1000);

        let mut tx = Transaction::new(alice, val_pubkey, 0, vec![]);
        tx.tx_type = TransactionType::Vote;
        tx.fee = 2;

        Executor::apply_transaction(&mut state, &tx).unwrap();

        let validator = state.get_validator(&val_pubkey).unwrap();
        assert_eq!(validator.votes_for, 0);
        assert_eq!(validator.votes_against, 1);
    }

    #[test]
    fn test_contract_call_executes_budzkvm_bytecode() {
        let mut state = AccountState::new();
        let alice = Address::from_hex(&"03".repeat(32)).unwrap();
        state.add_balance(&alice, 100);

        let program = vec![
            Instruction {
                opcode: Opcode::Load,
                rd: 1,
                rs1: 0,
                rs2: 0,
                imm: 11,
            }
            .encode(),
            Instruction {
                opcode: Opcode::Log,
                rd: 0,
                rs1: 1,
                rs2: 0,
                imm: 0,
            }
            .encode(),
            Instruction {
                opcode: Opcode::Halt,
                rd: 0,
                rs1: 0,
                rs2: 0,
                imm: 0,
            }
            .encode(),
        ];
        let bytecode: Vec<u8> = program
            .into_iter()
            .flat_map(|instruction| instruction.to_le_bytes())
            .collect();
        let tx = Transaction::new_contract_call(alice, 7, 0, bytecode);

        Executor::apply_transaction(&mut state, &tx).unwrap();

        let alice_acc = state.get_or_create(&alice);
        assert_eq!(alice_acc.balance, 93);
        assert_eq!(alice_acc.nonce, 1);
    }
}
