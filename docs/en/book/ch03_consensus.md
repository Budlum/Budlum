# Chapter 3: Consensus Mechanisms

The heart of a blockchain is its consensus mechanism. In a distributed network, all nodes must agree on which block is valid and which direction the chain should follow.

In this chapter, we will examine the three consensus mechanisms supported by the Budlum project:

1.  **Proof of Work (PoW):** Classic Bitcoin-style mining based on computational power.
2.  **Proof of Stake (PoS):** A modern, energy-efficient system based on economic collateral.
3.  **Proof of Authority (PoA):** A system for private networks that trusts a defined set of authorities.

Budlum is built on a modular structure, the `ConsensusEngine` trait, that can switch between these mechanisms.

