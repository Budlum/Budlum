# Chapter 3.1: Consensus Engine Interface

Budlum treats consensus as a replaceable engine. The chain should not care whether a block came from PoW, PoS, or PoA as long as the selected engine can prepare and validate it.

## 1. Data Structures: Abstraction

### Trait: `ConsensusEngine`

`ConsensusEngine` defines the shared interface for consensus implementations. Engines expose block preparation, validation, and fork-choice behavior through one contract.

## 2. Design Decision: Why a Trait?

A trait gives Budlum:

1.  **Testability:** tests can use a `MockEngine` to produce blocks instantly without mining.
2.  **Flexibility:** the network can move to another consensus engine through a hard-fork-style software upgrade.
3.  **Isolation:** PoW, PoS, and PoA logic remain separated from core block and storage code.

## 3. Fork Choice Rule

Fork choice decides which valid branch should be canonical. Budlum combines the active consensus rule with finality protection: once a checkpoint is finalized, nodes must not reorganize behind it.

