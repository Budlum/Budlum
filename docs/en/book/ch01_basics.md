# Chapter 1: Fundamentals and Data Structures

Budlum is built from a small set of core structures: blocks, transactions, and account state.

- **Blocks** package transactions and link to previous blocks through hashes.
- **Transactions** describe value transfers, staking operations, votes, and contract calls.
- **Account state** stores balances, nonces, validator data, and Merkle roots.

These structures form the deterministic state machine that every node must reproduce.

