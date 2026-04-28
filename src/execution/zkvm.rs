use bud_proof::{DefaultAdapter as Prover, ProverAdapter};
use bud_vm::Vm;

pub const DEFAULT_CONTRACT_GAS_LIMIT: u64 = 1_000_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ZkVmReceipt {
    pub gas_used: u64,
    pub steps: usize,
    pub events: Vec<u64>,
    pub proof_bytes: usize,
}

pub struct ZkVmExecutor;

impl ZkVmExecutor {
    pub fn execute_bytecode(bytecode: &[u8], gas_limit: u64) -> Result<ZkVmReceipt, String> {
        if bytecode.is_empty() {
            return Err("Empty BudZKVM bytecode".into());
        }
        if bytecode.len() % 8 != 0 {
            return Err("BudZKVM bytecode length must be a multiple of 8 bytes".into());
        }

        let program = decode_program(bytecode)?;
        let mut vm = Vm::with_gas_limit(1024, gas_limit);

        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            vm.run(&program);
        }))
        .map_err(|_| "BudZKVM execution failed".to_string())?;

        let proof = Prover::prove(&vm.trace, vm.trace.len());
        if !Prover::verify(&proof, vm.trace.len()) {
            return Err("BudZKVM proof verification failed".into());
        }

        Ok(ZkVmReceipt {
            gas_used: vm.gas_used,
            steps: vm.trace.len(),
            events: vm.events,
            proof_bytes: proof.data.len(),
        })
    }
}

fn decode_program(bytecode: &[u8]) -> Result<Vec<u64>, String> {
    bytecode
        .chunks_exact(8)
        .map(|chunk| {
            let bytes: [u8; 8] = chunk
                .try_into()
                .map_err(|_| "Invalid BudZKVM instruction encoding".to_string())?;
            Ok(u64::from_le_bytes(bytes))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use bud_isa::{Instruction, Opcode};

    #[test]
    fn executes_simple_budzkvm_program() {
        let program = vec![
            Instruction {
                opcode: Opcode::Load,
                rd: 1,
                rs1: 0,
                rs2: 0,
                imm: 7,
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

        let receipt =
            ZkVmExecutor::execute_bytecode(&bytecode, DEFAULT_CONTRACT_GAS_LIMIT).unwrap();

        assert_eq!(receipt.events, vec![7]);
        assert!(receipt.steps > 0);
        assert!(receipt.proof_bytes > 0);
    }
}
