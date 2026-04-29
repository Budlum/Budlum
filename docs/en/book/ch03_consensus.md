# Chapter 3: Consensus Mechanisms

The heart of a blockchain is its consensus mechanism. In a distributed network, all nodes must agree on which block is valid and which direction the chain should follow.

In this chapter, we will examine the three consensus mechanisms supported by the Budlum project:

1.  **Proof of Work (PoW):** Classic Bitcoin-style mining based on computational power.
2.  **Proof of Stake (PoS):** A modern, energy-efficient system based on economic collateral.
3.  **Proof of Authority (PoA):** A system for private networks that trusts a defined set of authorities.

Budlum is built on a modular structure, the `ConsensusEngine` trait, that can switch between these mechanisms.

However, Budlum goes beyond just switching engines. It pioneers a **Multi-Consensus Settlement Architecture**, allowing PoW, PoS, and PoA networks to operate concurrently as isolated domains. These independent networks settle their cryptographic proofs (commitments) into a unified Global Block, enabling trustless cross-domain automation and secure asset bridging without centralized intermediaries.

