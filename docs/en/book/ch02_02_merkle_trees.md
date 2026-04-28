# Chapter 2.2: Merkle Trees and Data Integrity

Merkle trees let Budlum prove that data exists inside a large set without sending the entire set. They are the bridge between compact block headers and verifiable data.

## 1. Conceptual Basis: Why a Tree?

Instead of hashing all transactions into one flat blob, each transaction becomes a leaf. Pairs of leaves are hashed upward until one root remains. If one transaction changes, the root changes.

## 2. Code Analysis: `calculate_tx_root`

`calculate_tx_root` hashes transactions, groups them in pairs, duplicates the final item when the count is odd, and repeats until one root remains.

This root is stored in the block header as `tx_root`.

## 3. Light Client Logic

A light client can:

1.  Download only block headers.
2.  Read the `tx_root`.
3.  Ask a full node whether a transaction is under that root.
4.  Receive the Merkle proof path.
5.  Verify the path locally.

The client does not need the whole block or the whole chain.

## 4. Hardening Phase 2: QC Blob and PQ Signatures

Post-quantum signatures can be large. Putting every Dilithium signature directly into the block header would bloat blocks.

Budlum solves this with a signature Merkle tree:

1.  Each validator's Dilithium signature is a leaf.
2.  The signatures form a tree.
3.  The root is committed in the block header.

This keeps headers light while still allowing fraud proofs for invalid signatures.

## 5. Incremental State Merkle Tree

Budlum hardening also uses an incremental Merkle structure for account state:

1.  Account changes mark accounts as dirty.
2.  Leaf hashes are cached.
3.  `calculate_state_root` updates only changed leaves and affected paths.

## Summary

1.  **Transactions** are proven through `tx_root`.
2.  **Accounts** are proven through `state_root` using an incremental Merkle tree.
3.  **PQ security** remains manageable through `QcBlob` roots.
4.  **Light clients** can verify balances and transactions without downloading full state.

