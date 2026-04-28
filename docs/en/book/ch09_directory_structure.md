# Chapter 9: Directory Structure and Modularity

Budlum is organized by layer:

- `core/`: fundamental types and configuration.
- `chain/`: blockchain state, finality, snapshots, and actor API.
- `consensus/`: PoW, PoS, PoA, finality support, and QC logic.
- `network/`: libp2p node, protocol, peer manager, and sync codec.
- `storage/`: Sled persistence and indexes.
- `execution/`: transaction and BudZKVM execution.
- `mempool/`: transaction pool.
- `tests/`: integration, hardening, chaos, and performance tests.

