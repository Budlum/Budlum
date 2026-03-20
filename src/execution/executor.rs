use crate::core::account::AccountState;
use crate::core::transaction::{Transaction, TransactionType};
use crate::consensus::pos::SlashingEvidence;

pub struct Executor;

impl Executor {
    pub fn apply_transaction(state: &mut AccountState, tx: &Transaction) -> Result<(), String> {
        if tx.from == "genesis" {
            return Ok(());
        }

        let total_cost = tx.total_cost();

        {
            let sender_account = state.get_or_create(&tx.from);
            if sender_account.balance < total_cost {
                return Err("Insufficient balance".into());
            }
        }

        match tx.tx_type {
            TransactionType::Transfer => {
                let sender = state.get_or_create(&tx.from);
                sender.balance -= total_cost;
                sender.nonce += 1;

                let receiver = state.get_or_create(&tx.to);
                receiver.balance += tx.amount;
            }
            TransactionType::Stake => {
                let sender = state.get_or_create(&tx.from);
                sender.balance -= total_cost;
                sender.nonce += 1;

                let stake_amount = tx.amount;
                let validator = state.get_validator_mut(&tx.from);
                
                if let Some(v) = validator {
                    v.stake += stake_amount;
                    v.active = true;
                } else {
                    state.add_validator(tx.from.clone(), stake_amount);
                }
                println!("Stake added: {} now has {}", tx.from, stake_amount);
            }
            TransactionType::Unstake => {
                let sender_start_balance = state.get_balance(&tx.from);
                if sender_start_balance < tx.fee {
                    return Err("Insufficient balance for fee".into());
                }

                if let Some(validator) = state.get_validator_mut(&tx.from) {
                    if validator.stake < tx.amount {
                        return Err("Insufficient stake".into());
                    }
                    validator.stake -= tx.amount;
                    if validator.stake == 0 {
                        validator.active = false;
                    }
                    println!(
                        "Unstake queued: {} amount {} releases at epoch {}",
                        tx.from,
                        tx.amount,
                        state.epoch_index + crate::core::account::UNBONDING_EPOCHS
                    );
                } else {
                    return Err("Not a validator".into());
                }

                state.unbonding_queue.push(crate::core::account::UnbondingEntry {
                    address: tx.from.clone(),
                    amount: tx.amount,
                    release_epoch: state.epoch_index + crate::core::account::UNBONDING_EPOCHS,
                });

                let sender = state.get_or_create(&tx.from);
                sender.balance -= tx.fee;
                sender.nonce += 1;
            }
            TransactionType::Vote => {
                let sender = state.get_or_create(&tx.from);
                sender.balance -= tx.fee;
                sender.nonce += 1;

                println!("Vote TX processed from {}", tx.from);
            }
        }

        Ok(())
    }

    pub fn apply_block(
        state: &mut AccountState,
        transactions: &[Transaction],
        block_producer: Option<&str>,
    ) -> Result<(), String> {
        let mut total_fees: u64 = 0;
        for tx in transactions {
            if tx.from == "genesis" {
                continue;
            }
            if let Err(e) = Self::apply_transaction(state, tx) {
                return Err(format!("TX apply failed: {}", e));
            }
            total_fees += tx.fee;
        }
        if let Some(producer) = block_producer {
            if total_fees > 0 {
                let producer_account = state.get_or_create(producer);
                producer_account.balance += total_fees;
                println!(
                    "Block producer {} received {} in fees",
                    &producer[..16.min(producer.len())],
                    total_fees
                );
            }
        }
        Ok(())
    }
}
