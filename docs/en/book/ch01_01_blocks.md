# Chapter 1.1: Block Structure and Chain Architecture

A block is the basic unit of the chain. It carries transaction data, consensus metadata, roots, and signatures.

Important fields include:

- `index`: the block height.
- `previous_hash`: the parent link.
- `hash`: the deterministic digest of the block.
- `state_root`: the account-state commitment after execution.
- `tx_root`: the Merkle root of transactions.
- `producer`: the block producer address.
- `chain_id`: network isolation for mainnet, testnet, and devnet.
- `validator_set_hash`: the validator-set commitment used by finality.

Budlum hashes blocks with deterministic serialization and domain separation so all nodes compute the same result.

