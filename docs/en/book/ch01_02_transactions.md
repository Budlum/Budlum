# Chapter 1.2: Transactions and Data Transfer Architecture

Transactions describe value movement and state changes. In Budlum they are also the boundary where replay protection, fee logic, indexing, contract-call checks, and signature validation meet.

## 1. Data Structures: Anatomy of a Transaction

### Enum: `TransactionType`

`TransactionType` separates intent:

-   ordinary transfers,
-   staking and validator operations,
-   votes,
-   contract calls,
-   genesis allocations.

This lets the state machine validate each operation with the right business rules instead of treating every transaction as a simple balance transfer.

### Struct: `Transaction`

Core fields include:

-   `from` and `to`: sender and receiver addresses.
-   `amount`: transferred value.
-   `fee`: payment to the producer and the main ordering signal in the mempool.
-   `nonce`: replay protection and sender-level ordering.
-   `chain_id`: network isolation between Mainnet, Testnet, and Devnet.
-   `tx_type`: the operation type.
-   `hash` and `signature`: transaction identity and authorization.

## 2. Dynamic Fee Market

Budlum treats fees as more than an optional tip. Fees prioritize transactions, protect the network from spam, and give producers an economic reason to include valid transactions.

For contract calls, the gas schedule defines base cost, per-instruction cost, and validation cost. This keeps BudZKVM execution bounded and predictable.

## 3. Transaction Indexing

When a transaction is committed, Budlum records an index from transaction hash to block height. This allows fast lookups without scanning the entire chain.

## 4. Security Algorithms

### Strict Signature Verification

A transaction must be signed over its canonical signing hash. Invalid signatures are rejected before the transaction enters the network or the mempool.

### Genesis Spoofing Protection

Genesis transactions are special because they are unsigned allocations. Mainnet hardening prevents peers from broadcasting fake transactions with `from = "genesis"` to inflate balances. Genesis effects are accepted only through the controlled genesis path.

### Block-Level Chain ID Validation

Every transaction inside a block must match the block's `chain_id`. This prevents a transaction from one network being replayed on another.

### ContractCall Bytecode Validation

BudZKVM contract calls are checked for shape before execution: bytecode must be non-empty and aligned to the expected instruction width. Bad payloads are rejected early.

### Function: `signing_hash`

The signing hash includes the fields that define transaction intent. It intentionally excludes `hash` and `signature`; otherwise signing would become circular.

`nonce` and `chain_id` are mandatory in this hash. Without them, replay attacks would be possible.

### Function: `sign`

Signing is like placing a handwritten signature under a finished document. First the message is formed with `signing_hash`, then the private key signs it. If the content changes by even one bit, the signature no longer verifies.

