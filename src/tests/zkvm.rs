#[cfg(test)]
mod zkvm_tests {
    use crate::chain::blockchain::Blockchain;
    use crate::consensus::pow::PoWEngine;
    use crate::core::account::AccountState;
    use crate::core::address::Address;
    use crate::core::transaction::{Transaction, TransactionType};
    use crate::crypto::primitives::KeyPair;
    use crate::execution::executor::Executor;
    use crate::execution::zkvm::{ZkVmExecutor, DEFAULT_CONTRACT_GAS_LIMIT};
    use crate::network::proto_conversions::pb;
    use bud_isa::{Instruction, Opcode};
    use std::sync::Arc;

    fn inst(opcode: Opcode, rd: u8, rs1: u8, rs2: u8, imm: i32) -> u64 {
        Instruction {
            opcode,
            rd,
            rs1,
            rs2,
            imm,
        }
        .encode()
    }

    fn bytecode(program: Vec<u64>) -> Vec<u8> {
        program
            .into_iter()
            .flat_map(|instruction| instruction.to_le_bytes())
            .collect()
    }

    fn logging_program(value: i32) -> Vec<u8> {
        bytecode(vec![
            inst(Opcode::Load, 1, 0, 0, value),
            inst(Opcode::Log, 0, 1, 0, 0),
            inst(Opcode::Halt, 0, 0, 0, 0),
        ])
    }

    fn infinite_loop_program() -> Vec<u8> {
        bytecode(vec![inst(Opcode::Jmp, 0, 0, 0, 0)])
    }

    fn signed_contract_tx(keypair: &KeyPair, fee: u64, nonce: u64, code: Vec<u8>) -> Transaction {
        let from = Address::from(keypair.public_key_bytes());
        let mut tx = Transaction::new_contract_call(from, fee, nonce, code);
        tx.sign(keypair);
        tx
    }

    #[test]
    fn zkvm_executor_returns_receipt_for_valid_bytecode() {
        let receipt =
            ZkVmExecutor::execute_bytecode(&logging_program(42), DEFAULT_CONTRACT_GAS_LIMIT)
                .unwrap();

        assert_eq!(receipt.events, vec![42]);
        assert_eq!(receipt.steps, 3);
        assert!(receipt.gas_used > 0);
        assert!(receipt.proof_bytes > 0);
    }

    #[test]
    fn zkvm_executor_rejects_malformed_bytecode() {
        let err = ZkVmExecutor::execute_bytecode(&[1, 2, 3], DEFAULT_CONTRACT_GAS_LIMIT)
            .expect_err("malformed bytecode must be rejected");

        assert!(err.contains("multiple of 8"));
    }

    #[test]
    fn zkvm_executor_maps_out_of_gas_to_error() {
        let err = ZkVmExecutor::execute_bytecode(&infinite_loop_program(), 3)
            .expect_err("gas exhaustion must abort execution");

        assert_eq!(err, "BudZKVM execution failed");
    }

    #[test]
    fn contract_call_transaction_validation_requires_bytecode_shape() {
        let keypair = KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());

        let mut tx = Transaction::new_contract_call(from, 1, 0, vec![1, 2, 3]);
        tx.sign(&keypair);

        assert!(!tx.is_valid());

        let mut state = AccountState::new();
        state.add_balance(&from, 100);
        let err = state
            .validate_transaction(&tx)
            .expect_err("invalid contract bytecode must fail state validation");
        assert!(err.contains("bytecode"));
    }

    #[test]
    fn contract_call_failure_is_atomic_for_sender_state() {
        let keypair = KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        let mut state = AccountState::new();
        state.add_balance(&from, 100);

        let tx = signed_contract_tx(&keypair, 5, 0, infinite_loop_program());
        let before_balance = state.get_balance(&from);
        let before_nonce = state.get_nonce(&from);

        let err = Executor::apply_transaction(&mut state, &tx)
            .expect_err("failing VM execution must reject the tx");

        assert_eq!(err, "BudZKVM execution failed");
        assert_eq!(state.get_balance(&from), before_balance);
        assert_eq!(state.get_nonce(&from), before_nonce);
    }

    #[test]
    fn contract_call_is_included_in_block_and_mutates_sender_once() {
        let keypair = KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&from);

        let tx = signed_contract_tx(&keypair, 7, 0, logging_program(9));
        blockchain
            .add_transaction(tx.clone())
            .expect("valid contract call should enter mempool");

        let producer = Address::from_hex(&"09".repeat(32)).unwrap();
        let block = blockchain
            .produce_block(producer)
            .expect("contract call block should be produced");

        assert_eq!(block.transactions.len(), 1);
        assert_eq!(block.transactions[0].hash, tx.hash);
        assert_eq!(block.transactions[0].tx_type, TransactionType::ContractCall);
        assert_eq!(blockchain.state.get_nonce(&from), 1);
        assert_eq!(blockchain.state.get_balance(&from), 1_000_000_000 - 7);
        assert!(blockchain.mempool.is_empty());
        assert_eq!(block.state_root, blockchain.state.calculate_state_root());
    }

    #[test]
    fn tx_precheck_reports_invalid_contract_call_without_mempool_side_effect() {
        let keypair = KeyPair::generate().unwrap();
        let from = Address::from(keypair.public_key_bytes());
        let consensus = Arc::new(PoWEngine::new(1));
        let mut blockchain = Blockchain::new(consensus, None, 1337, None);
        blockchain.init_genesis_account(&from);

        let mut tx = Transaction::new_contract_call(from, 1, 0, vec![0xaa, 0xbb]);
        tx.sign(&keypair);

        let precheck = blockchain.tx_precheck(&tx);

        assert_eq!(precheck["accepted"], false);
        let reasons = precheck["reasons"].as_array().unwrap();
        assert!(reasons
            .iter()
            .any(|reason| reason == "invalid_contract_bytecode"));
        assert!(blockchain.mempool.is_empty());
    }

    #[test]
    fn contract_call_proto_round_trip_preserves_type_and_payload() {
        let keypair = KeyPair::generate().unwrap();
        let tx = signed_contract_tx(&keypair, 11, 0, logging_program(13));

        let proto = pb::ProtoTransaction::from(&tx);
        assert_eq!(proto.tx_type, pb::ProtoTransactionType::ContractCall as i32);

        let decoded = Transaction::try_from(proto).unwrap();
        assert_eq!(decoded.tx_type, TransactionType::ContractCall);
        assert_eq!(decoded.data, tx.data);
        assert_eq!(decoded.hash, tx.hash);
        assert!(decoded.verify());
    }
}
