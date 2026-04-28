# Budlum Blockchain Book

Welcome!

This book is the living technical documentation for the **Budlum Blockchain** project. It explains not only what the code does, but why the architecture is shaped this way.

Budlum is written in **Rust** and focuses on security, modularity, readable protocol design, and production hardening.

Core topics:

- **Cryptography:** Ed25519, BLS finality, Dilithium PQ attestations, Merkle proofs.
- **Networking:** libp2p, Gossipsub, request-response sync, peer reputation.
- **Storage:** Sled, atomic block commits, snapshots, finality/QC persistence.
- **Consensus:** modular PoW, PoS, PoA, BLS finality, and PQ-gated checkpoints.
- **Operations:** RPC, metrics, fast sync, chaos tests, and database integrity tooling.

Start with [Chapter 1](ch01_basics.md), or jump directly to [Optimistic QC and PQ Attestation](ch03_06_qc.md) for the latest PQ-QC flow.

