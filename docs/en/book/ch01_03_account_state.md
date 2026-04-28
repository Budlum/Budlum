# Chapter 1.3: Account State and State Machine Architecture

Account state is the chain's memory.

It stores account balances, nonces, validators, governance values, fee parameters, and Merkle caches. Dirty tracking allows Budlum to update only changed accounts instead of hashing the whole state every time.

Validator records include BLS and Dilithium public keys, so finality and PQ-QC verification are tied to validator identity.

