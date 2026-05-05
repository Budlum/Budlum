# Multi-Consensus Settlement Layer

This document outlines the architecture, design goals, and implementation details of Budlum's Multi-Consensus Settlement Layer (Model B).

## 1. Problem
Traditional blockchains are bound to a single consensus mechanism (e.g., PoW only or PoS only). Scaling often involves L2s or sidechains that "bridge" assets, creating fragmented liquidity and complex trust assumptions. There is no standard way for multiple heterogeneous consensus domains to update a single, shared global state deterministically without trust-heavy intermediaries.

## 2. Design Goal
The goal of Budlum is to create a **Universal Settlement Layer** that:
- Supports parallel execution of multiple consensus domains (PoW, PoS, BFT, ZK).
- Enforces a single, unified global account state.
- Ensures Byzantine fault tolerance at the settlement level.
- Provides a "Registry-First" approach where all commitments are recorded before verification, ensuring out-of-order data resilience.
- Persists commitment acceptance and domain height updates atomically so restart recovery cannot observe a half-committed settlement transition.

## 3. Consensus Domain Model
A **Consensus Domain** is an independent blockchain or execution environment with its own rules.
- **Identity:** Each domain has a unique `DomainId`.
- **Kind:** Defines the consensus type (PoW, PoS, etc.).
- **Operator Identity:** Each registered domain carries an operator address and minimum bond. Registrations without an operator bond are rejected.
- **Registry:** The Settlement Layer tracks all active domains, their current heights, their operator bond, and their `ValidatorSetHash`.
- **Adapters:** Each domain uses a specific `FinalityAdapter` to prove its state transitions to the Settlement Layer.

## 4. DomainCommitment Structure
A `DomainCommitment` is the cryptographic proof submitted by a domain to the settlement layer:
- `domain_id`: Source of the update.
- `domain_height`: The height of the block being committed.
- `state_root`: The resulting state of the domain.
- `state_updates`: A map of account nonces/balances modified in this commitment.
- `finality_proof_hash`: Reference to the consensus-specific proof (e.g., PoW nonce or PoS signatures).

## 5. Settlement Layer
The Settlement Layer acts as the "Supreme Court" of the Budlum ecosystem. It does not execute transactions; it verifies **commitments**.
- It maintains a `GlobalBlockHeader` which is a Merkle aggregate of all verified domain commitments.
- It manages the **Global Registry** of domains and their statuses (Active, Frozen, Retired).

## 6. Global Shared-State Safety
To prevent cross-domain double-spending, Budlum enforces the **Nonce Invariant**:
$$Account_{nonce}^{Global} < Commitment_{nonce}^{Domain}$$
A commitment is only valid if its nonce is strictly greater than the current global nonce for that account. This ensures that even if two domains try to update the same account, only one can succeed at a given "Global Height."

## 7. Deterministic Conflict Resolution
If two domains submit conflicting updates for the same account nonce:
- The first commitment to reach the global settlement registry (via P2P arrival or block inclusion) is accepted.
- Any subsequent commitment for the same nonce is rejected as a **Fraudulent Equivocation**.

## 8. Gossip and Network Convergence
Commitments are spread via a **Gossip Mesh** (`libp2p-gossipsub`).
- **Convergence:** Honest nodes eventually receive the same set of commitments.
- **Idempotency:** Re-submitting the same commitment has no effect on the state.
- **Buffering:** Out-of-order commitments (e.g., receiving height 10 before height 9) are stored in a `pending_buffer` and applied once the gap is filled.

## 9. Byzantine Domain Handling
If a domain behaves maliciously (equivocation):
- **Evidence:** The conflicting commitments are stored in the registry as proof.
- **Global Freeze:** The domain's status is changed to `Frozen`. No further commitments from this domain will ever be accepted.
- **Slashing Trigger:** Frozen domains provide an economic penalty hook through the operator bond model.

Validator-level equivocation is handled separately by PoS slashing evidence:
- A node that detects double-signing stores `SlashingEvidence`.
- Evidence is gossiped as a top-level `NetworkMessage::SlashingEvidence`.
- Producers include pending evidence in later blocks, where state execution applies stake slashing.

## 10. Persistence and Crash Recovery
The layer currently uses a persistent **Sled DB** behind a `BlockchainStorage` trait. Values are written with binary serialization and legacy JSON reads are still tolerated during migration. The storage backend stores:
- All domain commitments (verified and pending).
- The current status of all domains.
- The global state tree.
- Atomic settlement batches covering commitment insert + domain height/hash updates.
- Node restart logic ensures that pending commitments and `Frozen` statuses are restored immediately, preventing "equivocation-on-restart" attacks.

## 11. Current Limitations
The current repository is suitable for a controlled public devnet, but not for audited mainnet deployment:
- **Audit Pending:** Security audits and performance hardening are still required.
- **Operational Hardening:** RPC rate limiting/auth, Docker/systemd packaging, health checks, fuzzing, and full clippy cleanup are still open.
- **Error Refactor:** Structured `BudlumError` exists and critical execution paths use it, but some public APIs still expose legacy `Result<T, String>` wrappers.
- **Formal Verification:** The mathematical invariants have not yet been formally verified using TLA+ or similar tools.
- **Early Adapters:** PoS and BFT adapters currently use simulated signature counts.

## 12. Test Coverage
The layer is verified through a **Byzantine Chaos Matrix**, covering:
- Network partitions and reconciliation.
- Double-spend protection across domains.
- Node crash/recovery cycles.
- High-concurrency stress testing.
