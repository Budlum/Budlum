# Chapter 3.1: Consensus Engine Interface

The `ConsensusEngine` trait isolates consensus-specific logic from the blockchain, storage, mempool, and networking layers.

This makes engines easier to test and lets Budlum evolve consensus rules without rewriting the whole chain.

