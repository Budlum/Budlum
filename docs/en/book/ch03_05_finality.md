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

### QC Gating

`FinalityCert` acceptance is no longer just BLS aggregate signature verification. A checkpoint is finalized only when:

1.  The checkpoint height and hash match the local chain.
2.  The certificate is verified against the validator snapshot for its epoch.
3.  The signer indexes are derived from the certificate bitmap.
4.  A verified `QC_BLOB` exists for the same checkpoint.
5.  The `QcBlob` contains valid Dilithium attestations for the BLS signers.

If a `FinalityCert` arrives before the corresponding `QC_BLOB`, the node queues it as pending, requests `GetQcBlob`, and retries the certificate automatically after the blob is imported. Finality therefore does not depend on message ordering.

Validator verification also uses epoch snapshots instead of pretending the current active set is historical. This prevents old checkpoints from being verified against the wrong validator set after validator changes.

## 5. Slashing: `DoubleVote`

Voting for two conflicting checkpoints in the same epoch is a serious fault. A double-vote proof can identify the validator and trigger slashing.

### QC Fault Proof and Finality Invalidation

The finality layer also handles faulty PQ attestations. If a `QcFaultProof` proves that a leaf inside a stored `QcBlob` contains an invalid Dilithium signature, finality metadata from that checkpoint can be invalidated.

Current invalid-Dilithium Merkle proofs do **not** slash validators directly. Slashable QC verdicts are reserved for stronger signed or ZK-backed evidence. `QcFaultProof` can now be carried as a P2P message, parsed by recipients, verified against the stored blob and epoch snapshot, then applied as a verdict.

## 6. Fork Choice and Reorg Protection

The rule is simple: **no node may switch to a fork that starts before a finalized checkpoint**. This makes finalized transactions irreversible from the user's perspective.

## Summary

1.  **Efficiency:** many BLS signatures become one certificate.
2.  **Certainty:** checkpoints reduce reorg risk.
3.  **Economic security:** double-vote proofs make cheating expensive.
