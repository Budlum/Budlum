# Chapter 3.5: Finality Layer (BLS)

This chapter explains the **BLS finality layer**, which gives Budlum irreversible checkpoints. Finality prevents long-lived chain splits and gives users confidence that finalized transactions will not be reorganized away.

## 1. Why a Finality Layer?

In ordinary PoW or PoS systems, a block is considered safer as more blocks are built on top of it. Budlum adds a voting layer so selected checkpoints can become final much sooner.

Core goals:

-   **Speed:** checkpoints can become final quickly.
-   **Security:** malicious validators can be slashed.
-   **Immutability:** nodes do not reorganize behind finalized checkpoints.

## 2. Two-Phase Voting Protocol

### Phase 1: Prevote

Validators inspect the checkpoint block and sign a BLS prevote if they consider it valid. When at least two thirds of the validator set prevotes, the first phase is complete.

### Phase 2: Precommit

After prevote quorum, validators issue precommits. With two thirds precommit quorum, the checkpoint is marked finalized.

## 3. Automatic Voting Loop

Hardening added a background voting mechanism so validators do not need manual intervention. When the current height is a checkpoint and no vote has been cast, the node broadcasts a vote through the network.

## 4. Data Structure: `FinalityCert`

`FinalityCert` stores the finalized height, block hash, aggregate BLS signature, signer bitmap, and validator-set hash.

### Aggregation Math

Budlum aggregates signatures with real BLS group arithmetic. This keeps the certificate size compact even when the validator count grows.

## 5. Slashing: `DoubleVote`

Voting for two conflicting checkpoints in the same epoch is a serious fault. A double-vote proof can identify the validator and trigger slashing.

## 6. Fork Choice and Reorg Protection

The rule is simple: **no node may switch to a fork that starts before a finalized checkpoint**. This makes finalized transactions irreversible from the user's perspective.

## Summary

1.  **Efficiency:** many BLS signatures become one certificate.
2.  **Certainty:** checkpoints reduce reorg risk.
3.  **Economic security:** double-vote proofs make cheating expensive.

