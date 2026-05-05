# Settlement Layer Test Matrix & Architecture

This document tracks the verification status of the Multi-Consensus Settlement Layer and provides an architectural overview.

## 1. Test Matrix

| Test Name | Property | Status |
|-----------|----------|--------|
| `test_cross_domain_double_spend_protection` | Shared-state safety | ✅ Passed |
| `test_parallel_cross_domain_stress_determinism` | Stress determinism | ✅ Passed |
| `test_async_gossip_random_delay_duplicate_drop` | Gossip convergence | ✅ Passed |
| `test_frozen_domain_persistence` | Byzantine state persistence | ✅ Passed |
| `test_adversarial_finality_proofs` | Finality proof validation | ✅ Passed |
| `test_restart_pending_buffer_persistence` | Crash recovery | ✅ Passed |
| `test_distributed_gossip_convergence` | Real-node convergence | ✅ Passed |

## 2. Architecture Diagram

```mermaid
graph TD
    subgraph "Consensus Domains"
        D1[PoW Domain]
        D2[PoS Domain]
        D3[ZK Domain]
    end

    subgraph "Budlum Node (Settlement Layer)"
        Registry[Domain Registry]
        Buffer[Pending Buffer]
        Verifier[Finality Verifier]
        State[Global Account State]
        Storage[(Storage Trait + Atomic Batch)]
    end

    D1 -- "Commitment + Proof" --> P2P[GossipSub Mesh]
    D2 -- "Commitment + Proof" --> P2P
    D3 -- "Commitment + Proof" --> P2P

    P2P --> Registry
    Registry --> Buffer
    Buffer --> Verifier
    Verifier --> State
    State --> Storage

    Verifier -- "Equivocation Detected" --> Freeze[Global Freeze Domain]
    Freeze --> Registry
    Registry --> Bond[Operator Bond]
```

## 3. Current Risks & Limitations

### Risks
- **Early-Stage Adapters:** Finality proof adapters (PoS/BFT) are currently using high-level signature threshold logic rather than full cryptographic BLS/Ed25519 verification.
- **Networking Scale:** While tested with 5 nodes in a controlled harness, the behavior under 100+ nodes with high latency is not yet benchmarked.
- **Economic Safety:** Validator slashing and rewards are implemented for devnet-grade PoS flows, and domain registration now requires operator identity and bond. Mainnet-grade governance, bond sizing, and audit review are still required.

### Limitations
- **Controlled Public Devnet Ready:** The current code can support a public devnet with clear experimental disclaimers.
- **Not Mainnet-Ready:** The codebase still requires professional security audits, operational hardening, fuzzing, and full API/error cleanup before mainnet.
- **Formal Verification:** No TLA+ or formal proofs for the consensus convergence.
- **Public Testnet Scope:** Public devnet is appropriate; audited production/mainnet deployment is not.
- **Structured Errors:** `BudlumError` exists and critical execution paths use it, but legacy `Result<T, String>` compatibility remains in several APIs.

## 4. Budlum Core v0.1 — Controlled Public Devnet Candidate
The current state of the repository represents a **controlled public devnet candidate**, not an audited mainnet implementation.

**Key Achievements:**
- [x] Deterministic global state for heterogeneous domains.
- [x] Byzantine equivocation immunity (Model B).
- [x] Atomic settlement persistence for commitment + domain height/hash updates.
- [x] Distributed node convergence verified.
- [x] Slashing evidence gossip and block inclusion path.
- [x] PoS slashing/reward execution for devnet-grade validator economics.
