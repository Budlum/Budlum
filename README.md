# ⚡ Budlum Core

> **A controlled public-devnet candidate for Layer-1 blockchain research: modular, deterministic, and multi-consensus native.**

[![Build Status](https://img.shields.io/badge/build-passing-brightgreen)](https://github.com/rade/budlum-core)
[![Test Coverage](https://img.shields.io/badge/tests-342-blue)](https://github.com/rade/budlum-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.94.0-orange.svg)](https://www.rust-lang.org/)

---

> [!CAUTION]
> **Controlled Public Devnet Candidate (v0.3-dev)**
>
> Budlum Core is suitable for controlled public devnet experiments with clear risk disclaimers. It is **NOT** audited mainnet software, has not completed professional security review, and should **NOT** be used for financial transactions or production applications carrying real value.

---

Budlum Core is a Rust-based Layer-1 blockchain framework designed for engineers and protocol researchers who want to explore modular consensus, deterministic state settlement, and cross-domain interoperability.

If this project helps your research, please support it:
⭐ **Star the repo** | 🍴 **Fork it** | 🧠 **Open a discussion**

---

## 🏗️ Architectural Vision

Most blockchain frameworks are optimized for a single consensus worldview. Budlum is designed as a **Universal Settlement Layer** to research how heterogeneous networks (PoW, PoS, BFT) can achieve deterministic state convergence without centralized intermediaries.

### Why Budlum?
- 🔁 **Heterogeneous Settlement**: Infrastructure for running parallel consensus domains (PoW, PoS, BFT) on a unified settlement layer.
- 🌉 **Verified Trustless Interop**: Experimental bridge flow where lock, mint, burn, and unlock are tied to committed domain events and Merkle proofs.
- 🧠 **Deterministic Execution**: Research into replay-safe state transitions and consistent global headers.
- 🧩 **Modular Core**: Decoupled consensus, networking, and execution layers for rapid prototyping.
- 🌐 **P2P Native**: Built on `libp2p` with GossipSub, persistent identity, DNS seed resolution, and durable peer banning.
- 🛡️ **BLS Finality**: Two-phase BLS-signed prevote/precommit protocol with aggregated signature verification and auto-precommit.
- 🩺 **Dual RPC**: Separate public and operator JSON-RPC 2.0 listeners with health endpoints, per-IP rate limiting, and trusted-proxy enforcement.
- 📦 **Deployment Ready**: Docker multi-stage image, docker-compose 4-node devnet, systemd unit, and Prometheus metrics collectors.
- 🛠️ **Developer First**: Book-style technical documentation, JSON-RPC reference material, and a growing adversarial test suite.

---

## 🏗️ Architecture Overview

```mermaid
graph TD
    User(("User")) --> CLI["CLI / RPC (Public + Operator)"]
    CLI --> Node["Node Service"]

    subgraph "Settlement Layer"
        Node --> ChainHandle["ChainHandle"]
        ChainHandle --> ChainActor["ChainActor"]
        ChainActor --> State["Global Account State"]
        ChainActor --> Buffer["Pending Commitment Buffer"]
        ChainActor --> Storage["Atomic Persistence"]
    end

    subgraph "Consensus Domains"
        ChainActor -.-> Engine["ConsensusEngine Trait"]
        Engine --> PoW["Proof of Work"]
        Engine --> PoS["Proof of Stake + VRF + BLS"]
        Engine --> PoA["Proof of Authority"]
        Engine --> BFT["BFT Finality Adapters"]
    end

    subgraph "Execution & ZK"
        ChainActor --> Executor["State Executor"]
        Executor --> ZKVM["BudZKVM (Experimental)"]
    end

    subgraph "Networking"
        Node --> Libp2p["libp2p Swarm"]
        Libp2p --> Gossip["GossipSub"]
        Libp2p --> Identity["Persistent Identity"]
        Libp2p --> Reputation["Peer Scoring + Durable Bans"]
    end

    subgraph "Observability"
        Node --> Metrics["Prometheus /metrics"]
        Metrics --> Grafana["Grafana (optional)"]
    end
```

---

## 🧩 Devnet Candidate Features (v0.3)

### 🌍 Multi-Consensus Settlement (Model B)
- **Verified-Only Commitments**: RPC paths reject raw domain commitments; settlement updates must arrive as `VerifiedDomainCommitment` with a matching finality proof hash.
- **Adapter Hardening**: PoW requires confirmation depth; PoS binds finality certificate, validator snapshot, and registered validator-set hash.
- **Parent-Linked Domain History**: Rejects commitments whose `parent_domain_block_hash` does not link to the last committed domain block.
- **Strict Nonce Invariant**: Stale or equal nonce updates are rejected before durable insertion.
- **Byzantine Resilience**: Global state convergence verified via an 18-test "Chaos Matrix" under simulated partitions and delays.
- **Equivocation Immunity**: Protocol-level detection and global freezing of conflicting domains; duplicate commitments remain idempotent.
- **Atomic Settlement Persistence**: Commitment insertions and domain height/hash updates persisted in one storage batch.

### 🌉 Verified Cross-Domain Bridge
- **Bridge-Enabled Domains Only**: Asset registration requires active, registered, bridge-enabled domains.
- **Safe Lock Constraints**: Source/target domains must differ, transfer amount must be non-zero.
- **Raw Burn/Unlock Disabled**: Direct bridge burn and unlock calls rejected.
- **Proof-Based Return Path**: Funds return only after target-domain `BridgeBurned` event is committed and verified.

### 🛡️ BLS Finality Protocol (v0.3)
- **`BlsKeypair`**: BLS12-381 keypair integrated into `ValidatorKeys` with `sign_bls()` / `verify_bls_sig()` primitives.
- **Signed Votes**: Validators produce BLS-signed prevote/precommit messages via `ConsensusEngine::bls_secret_key()`.
- **Auto-Precommit**: Periodic loop detects prevote quorum and automatically signs + broadcasts precommit.
- **Aggregate Verification**: `FinalityCert` verified with BLS pairing: `e(sig, G2_gen) == e(H(msg), agg_pk)`.
- **Adversarial Tests**: Byzantine equivocation rejection, tampered aggregate signature detection, full 4-validator flow.

### ⚡ RPC Hardening (v0.3)
- **Dual Listeners**: Separate public and operator HTTP servers.
- **Trusted Proxy**: Only configured proxy IPs may set `X-Forwarded-For` for client identification.
- **Per-IP Rate Limiting**: Independent token buckets per client IP, 60-second sliding window.
- **Health Endpoints**: `bud_health` (status/height/peers) and `bud_nodeInfo` (chainId/peerId/rpcMode).
- **Operator-Only Admin Methods**: `bud_adminBanPeer`/`bud_adminUnbanPeer`/`bud_adminListBannedPeers` are rejected on the public listener and only work on the operator listener.
- **Body/Connection Limits**: Public 10MB/500 conn, Operator 50MB/10 conn.

### 🌐 P2P Hardening (v0.3)
- **Persistent Identity**: `load_or_generate_identity_key()` — P2P keypair survives restarts.
- **Durable Peer Bans**: JSON-persisted bans reloaded on startup, saved every 5 minutes.
- **mDNS Policy**: Per-network (`mainnet`/`testnet` off, `devnet` on).
- **DNS Seed Resolution**: `resolve_dns_seeds()` resolves hostnames to multiaddrs at startup.

### 💾 Snapshot V2 (v0.3)
- **Canonical V2 Format**: `StateSnapshotV2` with full consensus metadata (epoch, base_fee, block_reward, unbonding_queue, cross-domain roots, finality certs).
- **Replay Equivalence**: `AccountState::from_snapshot_v2()` preserves all consensus state; state root matches original.
- **Chunk-Session Binding**: `session_id` in `SnapshotChunk` prevents cross-peer chunk mixing.
- **V2-First Restore**: Startup tries V2 snapshot, falls back to V1.
- **Archive Mode**: `--archive-mode` / `features.archive_mode` disables pruning entirely so archive nodes keep full block history while still taking snapshots.

### 📊 Observability (v0.3)
- **Prometheus**: Live collectors for chain height, finalized height, blocks produced, transactions, reorgs, mempool size/evictions/cleanups, P2P messages/peers.
- **Docker**: Multi-stage Debian image, 4-node `docker-compose` with Prometheus.
- **systemd**: Production unit file with sandboxing and restart policy.
- **Fuzzing**: 4 `cargo-fuzz` targets (block, transaction, snapshot, consensus header).

---

## 🧪 Verification & Test Coverage

- **Total Tests**: `342` (All passing ✅)
- **Byzantine Chaos Matrix**: 18 scenarios covering network partitions, duplication, out-of-order delivery, and domain equivocation.
- **BLS Finality Tests**: 12 tests for sign/verify, aggregator flow, byzantine equivocation, certificate tampering, replay equivalence.
- **RPC Security Tests**: Auth, CORS, IP filtering, per-IP rate limiting, trusted proxy, operator defaults.
- **P2P Hardening Tests**: Persistent identity, durable ban roundtrip, DNS seed resolution.
- **Snapshot V2 Tests**: V2 metadata preservation, replay equivalence, serialization roundtrip, PruningManager V2 save/load.
- **Metrics Tests**: Chain metrics emit, counter increments, encoding format.
- **Distributed Devnet Simulation**: Gossip convergence across a 5-node `libp2p` mesh.

To run the full suite:
```bash
nix develop --command cargo test --workspace
```

To run fuzz targets:
```bash
cd fuzz && cargo fuzz run block_deserialize
```

---

## 🔒 Production Hardening Status

**v0.3-dev** closes 5 of 7 Mainnet blockers. Remaining work: external security audit, scheduled backup restore drills, and production runbooks. A `ConsensusStateV2` migration executor now exists (`Storage::run_migrations`), though no real migration steps are registered yet since the schema has never changed.

Read the book's [**Production Hardening Status**](docs/en/book/ch12_production_hardening.md) for the full implementation matrix.

---

## ⚡ Quick Start

### Requirements
- Rust `1.94.0`, `protoc`, sibling checkout of [BudZKVM](https://github.com/Budlum/BudZKVM)

### Build
```bash
git clone https://github.com/Budlum/BudZKVM.git
git clone https://github.com/rade/budlum-core.git infra
cd infra
cargo build --release
```

### Run a Devnet Node
```bash
# Proof of Work
./target/release/budlum-core --consensus pow --difficulty 3 --port 4001

# Proof of Stake
./target/release/budlum-core --consensus pos --min-stake 5000 --db-path ./data/pos_node
```

### Docker 4-node Devnet
```bash
docker compose up -d
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"bud_blockNumber","params":[],"id":1}' \
  http://localhost:8545
```

### RPC Usage
```bash
# Health check
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"bud_health","params":[],"id":1}' \
  http://localhost:8545

# Block height
curl -X POST -H "Content-Type: application/json" \
  --data '{"jsonrpc":"2.0","method":"bud_blockNumber","params":[],"id":1}' \
  http://localhost:8545
```

See the [**Protocol Specification**](SPECIFICATION.md) for the full API reference.

---

## 🗺️ Research Roadmap

- [x] **Devnet Economic Hardening**: Validator reward distribution and slashing execution.
- [x] **Settlement Atomicity**: Atomic commitment + domain height/hash persistence.
- [x] **Verified Settlement Hardening**: Proof-gated commitments, parent-link checks, strict nonce.
- [x] **Verified Bridge Return Path**: Bridge unlock requires committed target-domain burn event proof.
- [x] **Sync Hardening**: Handshake-triggered headers-first sync.
- [x] **PKCS#11 HSM Signer**: `ConsensusSigner` trait, `Pkcs11Signer`, `KeyPairSigner`.
- [x] **BLS Finality Protocol**: `BlsKeypair`, signed prevote/precommit, auto-precommit, 12 tests.
- [x] **RPC Dual Listener**: Public/operator servers, trusted proxies, health endpoints, per-IP rate limiting.
- [x] **P2P Hardening**: Persistent identity, durable bans, mDNS policy, DNS seeds.
- [x] **Snapshot V2**: Canonical V2 restore, replay equivalence, chunk-session binding.
- [x] **Observability**: Prometheus live collectors, Metrics wiring.
- [x] **Deployment**: Docker image, docker-compose, systemd unit, fuzz targets.
- [ ] **ZKVM Optimizations**: Improving STARK proof generation performance.
- [ ] **Formal Verification**: Researching TLA+ models for settlement convergence.
- [ ] **External Audit**: Professional security review.
- [ ] **Privacy Layer**: Exploring Monero/Zcash-style privacy primitives.
- [ ] **AI Execution Layer**: Investigating AI-assisted protocol automation.

Budlum is built for protocol researchers and developers who like looking under the hood. We welcome technical reviews, protocol design discussions, and security feedback.

## 🤝 Join the Research

### How to Contribute:
1. ⭐ **Star the Repository**: It helps other researchers find our work.
2. 🍴 **Fork & Experiment**: Try building a custom `ConsensusKind`!
3. 🧠 **Open a Discussion**: Have an idea for the Privacy Layer or AI Execution?
4. 🐛 **Report Bugs**: Use GitHub Issues for any technical anomalies.

Read [`CONTRIBUTING.md`](CONTRIBUTING.md) before participating. For security-sensitive reports, please use [`SECURITY.md`](SECURITY.md).

---

## 📄 License

MIT License. Copyright (c) 2026 The Budlum Developers.
