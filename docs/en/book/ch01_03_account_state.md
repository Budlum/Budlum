# Chapter 1.3: Account State and State Machine Architecture

Account state is the blockchain's memory. It records balances, nonces, validator information, and the Merkle structures that make the current state provable.

## 1. Data Structures: What Lives in Memory?

### Struct: `Account`

An account is similar to a bank account:

-   `balance` stores spendable value.
-   `nonce` orders outgoing transactions and prevents replay.
-   optional metadata can support staking or execution paths.

### Struct: `Validator`

A validator represents an actor that can participate in consensus. Its fields track stake, public keys, status, and evidence-related metadata. These values determine whether the validator can propose, vote, or be slashed.

### Struct: `AccountState`

`AccountState` is the global memory of the chain. It keeps accounts, validators, cached Merkle leaves, dirty-account tracking, and dynamic parameters.

Dirty tracking matters because a single account update should not force the node to recompute the entire state tree. Budlum hardening moved `state_root` calculation to an **Incremental Merkle Trie**:

1.  **Low latency:** only affected paths are updated, giving roughly $O(\log N)$ behavior.
2.  **Disk-friendly persistence:** changed accounts are written individually as `ACCT:{pubkey}`.
3.  **Merkle proofs:** light clients can verify account balances with a path and the root.

## 2. Dynamic Parameters and Governance

Runtime parameters such as gas values, limits, and validator settings are part of the state model. This gives the chain a controlled way to evolve without scattering configuration across unrelated modules.

## 3. Functions and Business Logic

### `validate_transaction` and `validate_transaction_with_context`

Validation checks balance, nonce, chain ID, transaction type rules, fee requirements, bytecode shape, and signature correctness. The context-aware version can validate against temporary state, which is essential when selecting multiple transactions for the same block.

### `apply_transaction`

Application mutates state only after validation. It debits the sender, credits the receiver, increments nonce, handles fees, and marks modified accounts as dirty.

The `get_or_create` detail is important: receiving funds can create a new account without requiring it to exist beforehand.

### BudZKVM Atomicity Rule

For contract calls, Budlum applies an all-or-nothing rule:

1.  The sender's fee balance and nonce are verified.
2.  Bytecode shape is checked.
3.  `ZkVmExecutor::execute_bytecode` decodes bytecode, runs the VM with a gas limit, produces a proof, and verifies the proof.
4.  Only if every step succeeds are the fee and nonce committed.

### `apply_block_checked` and legacy `apply_block`

Block-level application is deterministic. Critical execution paths use `apply_block_checked`, which returns `BudlumResult<()>` and carries structured `BudlumError` data. The legacy `apply_block` wrapper still returns `Result<(), String>` for compatibility. If any transaction fails, the whole block is rejected.

### `apply_slashing`

Slashing applies economic justice. A validator that signs conflicting data can be removed or penalized according to evidence verified by the chain.
