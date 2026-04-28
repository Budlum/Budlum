# Chapter 1.2: Transactions and Data Transfer Architecture

Transactions express state transitions.

They include sender, receiver, amount, fee, nonce, chain ID, type, payload, hash, and signature. The nonce prevents replay, while the chain ID prevents cross-network reuse.

Budlum validates transaction size, signature, fee, nonce, sender balance, chain ID, and special rules such as genesis spoofing protection and BudZKVM bytecode shape checks.

The mempool orders transactions by fee while preserving sender nonce order.

