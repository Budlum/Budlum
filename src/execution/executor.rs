use crate::core::account::AccountState;
use crate::core::address::Address;
use crate::core::transaction::{Transaction, TransactionType};
use crate::consensus::pos::SlashingEvidence;
use crate::chain::genesis::BLOCK_REWARD;

pub struct Executor;

impl Executor {
    pub fn apply_transaction(state: &mut AccountState, tx: &Transaction) -> Result<(), String> {
        if tx.from == Address::zero() {
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
                    return Err("Insufficient balance for fee".into());
                }

                if let Some(validator) = state.get_validator_mut(&tx.from) {
                    if validator.stake < tx.amount {
                        return Err("Insufficient stake".into());
                    }
                    validator.stake = validator.stake.saturating_sub(tx.amount);
                    if validator.stake == 0 {
                        validator.active = false;
                    }
                } else {
                    return Err("Not a validator".into());
                }

                state.unbonding_queue.push(crate::core::account::UnbondingEntry {
                    address: tx.from,
                    amount: tx.amount,
                    release_epoch: state.epoch_index + crate::core::account::UNBONDING_EPOCHS,
                });

                let sender = state.get_or_create(&tx.from);
                sender.balance = sender.balance.saturating_sub(tx.fee);
                sender.nonce = sender.nonce.saturating_add(1);
            }
            TransactionType::Vote => {
                let sender = state.get_or_create(&tx.from);
                sender.balance = sender.balance.saturating_sub(tx.fee);
                sender.nonce = sender.nonce.saturating_add(1);

                if let Some(target) = state.get_validator_mut(&tx.to) {
                    if tx.amount > 0 {
                        target.votes_for += 1;
                    } else {
                        target.votes_against += 1;
                    }
                    tracing::info!(
                        "Vote recorded: {} voted {} validator {}",
                        tx.from,
                        if tx.amount > 0 { "FOR" } else { "AGAINST" },
                        tx.to
                    );
                }
            }
        }

        Ok(())
    }

    pub fn apply_block(
        state: &mut AccountState,
        transactions: &[Transaction],
        block_producer: Option<&Address>,
    ) -> Result<(), String> {
        let mut total_fees: u64 = 0;
        for tx in transactions {
            if tx.from == Address::zero() {
                continue;
            }
            if let Err(e) = Self::apply_transaction(state, tx) {
                return Err(format!("TX apply failed: {}", e));
            }
            total_fees = total_fees.saturating_add(tx.fee);
        }
        if let Some(producer) = block_producer {
            let reward = total_fees.saturating_add(BLOCK_REWARD);
            if reward > 0 {
                let producer_account = state.get_or_create(producer);
                producer_account.balance = producer_account.balance.saturating_add(reward);
                tracing::info!("Producer {} received reward: {} (fees: {}, block: {})", producer, reward, total_fees, BLOCK_REWARD);
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

    #[test]
    fn test_apply_block_reward() {
        let mut state = AccountState::new();
        let producer = Address::from_hex(&"0".repeat(64)).unwrap();
        let txs = vec![];
        
        Executor::apply_block(&mut state, &txs, Some(&producer)).unwrap();
        
        let account = state.get_or_create(&producer);
        assert_eq!(account.balance, BLOCK_REWARD);
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
        
        let producer_acc = state.get_or_create(&producer);
        assert_eq!(producer_acc.balance, BLOCK_REWARD + 5);
        
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
}
