# Chapter 12: Production Hardening Status

This chapter is the operational truth table for the current repository. Budlum Core is a controlled public-devnet candidate. It is not audited Mainnet software and must not carry real economic value.

## 1. Implemented Protections

| Area | Current behavior |
| --- | --- |
| Configuration | Strict Config V2 rejects unknown fields, profile/chain-ID mismatches, unsafe Mainnet feature flags, missing Mainnet genesis, empty Mainnet seed configuration, and Mainnet mDNS. |
| Genesis | `genesis build` never prints private key material. Automatic allocation-key generation is devnet-only and requires an explicit output file. Non-devnet genesis requires explicit validators. |
| Startup | Storage open failures stop startup. Existing databases are checked against configured genesis identity. Custom genesis files must parse and match the selected chain ID. |
| State commitment | `ConsensusStateV2` commits accounts, validators, unbonding, economics, bridge, message, settlement, and global-header summary state. |
| Persistence | Canonical changes use `DurableCommitBatch` with an `IN_PROGRESS_HEIGHT` recovery marker and atomic Sled write batch. |
| Snapshot staging | Snapshot files are numerically ordered; corrupt latest files are quarantined. `StateSnapshotV2` is the canonical runtime format with full consensus metadata (epoch, fees, rewards, cross-domain roots, unbonding queue, finality certs). |
| RPC | Separate public and operator HTTP listeners. Public: API-key auth, CORS allowlists, per-IP rate limiting, trusted-proxy validation, 10MB body limit, 500 max connections. Operator: localhost-only, no auth, 50MB body limit. `bud_health` and `bud_nodeInfo` endpoints. |
| CI | GitHub Actions pins Rust `1.94.0`, checks formatting, runs `cargo check`, denies Clippy warnings, executes workspace tests, and builds `--release --locked`. |
| PKCS#11 | `ConsensusSigner` trait + `Pkcs11Signer` adapter (via `cryptoki`) + `KeyPairSigner` local fallback. `ConsensusEngine` trait exposes `fn signer()`. Block signing uses HSM when configured, with local file fallback. |
| BLS Finality | `BlsKeypair` in `ValidatorKeys` with new `sign_bls()` / `verify_bls_sig()` primitives. `ConsensusEngine::bls_secret_key()` exposed through PoS engine. Validators produce BLS-signed prevote/precommit messages. Periodic auto-precommit triggers when prevote quorum is reached. `FinalityCert` verification via BLS pairing. |
| P2P Hardening | Persistent node identity via `p2p_identity_file` (load-or-generate pattern). Durable peer bans persisted to JSON every 5 minutes and reloaded on startup. mDNS policy honors per-network `mdns_enabled` flag. DNS seed resolution via `resolve_dns_seeds()`. |

## 2. Staged or Partial Work

| Area | Boundary |
| --- | --- |
| Finality | Prevote/Precommit structs, `FinalityAggregator`, certificate production and BLS verification are all implemented and tested. BLS-signed vote production from validators is wired: `sign_prevote()` and `sign_precommit()` use the validator's BLS secret key. The periodic voting loop auto-signs and broadcasts BLS prevotes; auto-precommit fires when the aggregator reports prevote quorum reached. Adversarial multi-node finality tests are implemented. |
| P2P | Version and chain ID are enforced. Persistent identity, durable bans, mDNS policy, and DNS seed resolution are wired at runtime. Validator-set hash and supported-scheme policy remain pending. |
| RPC | Config parses public/operator listeners and trusted proxies. Runtime starts two separate servers (public + operator). `is_ip_allowed` enforces trusted-proxy validation: only requests from configured proxy IPs may use `X-Forwarded-For`. Header-derived client IPs are constrained to trusted proxies. Health and node-info endpoints are live. `is_per_ip_rate_limited` enforces a per-IP sliding-window quota (`src/rpc/server.rs`). `bud_adminBanPeer`/`bud_adminUnbanPeer`/`bud_adminListBannedPeers` are operator-only: `require_operator()` rejects them on the public listener. |
| Metrics | Prometheus descriptors and endpoint exist. Live collectors are wired: `budlum_chain_height`, `budlum_finalized_height`, `budlum_finality_lag`, `budlum_blocks_produced`, `budlum_transactions_processed`, `budlum_reorgs_total`, `budlum_mempool_size`, `budlum_mempool_evictions`, `budlum_mempool_expired_cleanups`, `budlum_p2p_messages_received`, `budlum_p2p_peers_connected`. Histograms are now observed too: `block_propagation_seconds` (on gossip block receipt, `src/network/node.rs`), `consensus_round_seconds` (per `produce_block()` call, `src/chain/blockchain.rs`), `storage_write_seconds` (per durable commit batch), `storage_read_seconds` (per `get_tx_block_height` lookup from `get_transaction_by_hash`/`get_transaction_receipt`). |
| Snapshot V2 | V2 is the canonical runtime format. `AccountState::from_snapshot_v2()` restores all consensus metadata (epoch, base_fee, block_reward, unbonding_queue, cross-domain roots, finality certs). `Blockchain` startup tries V2 then V1. P2P snapshots embed V2 data for backward-compatible transport. `apply_state_snapshot` tries V2 restore first. Chunk-session binding via `session_id` prevents cross-session chunk mixing. Replay equivalence is verified. `PruningManager::with_archive_mode(true)` (config: `features.archive_mode`) makes `get_prunable_blocks` always return empty, so an archive node keeps full block history while still taking snapshots. Scheduled backup restore drills remain an operational runbook item, not code. |
| Storage | Durable block commit exists. `Storage::run_migrations` is a real ordered-executor: it applies registered `(to_version, fn)` steps in sequence, stamps `SCHEMA_VERSION` after each step (so a crash mid-migration resumes correctly), and refuses to open a database whose stored version is newer than the binary supports. No migration steps are registered yet, since `CURRENT_SCHEMA_VERSION` has never changed — the executor itself is tested with synthetic steps (`src/storage/db.rs`). Released migration *steps* and restore drills remain for whenever a real schema bump ships. |
| Deployment | Docker multi-stage image, 4-node docker-compose devnet with Prometheus, systemd unit file, and fuzz harnesses exist. Release ceremony records and incident runbooks remain. |

## 3. Explicit Mainnet Blockers

1.  ~~Implement and audit the PKCS#11 consensus signer adapter.~~ **DONE (v0.2-dev)**
2.  ~~Complete BLS-signed prevote/precommit vote production from validators, live certificate gossip broadcast, and adversarial multi-node finality tests.~~ **DONE (v0.3-dev):** `BlsKeypair`, `sign_bls()`, BLS-signed prevote/precommit, auto-precommit at prevote quorum, 12 BLS/finality tests including byzantine equivocation, tampered aggregate sig, and full 4-validator finality flow.
3.  ~~Separate public and operator RPC servers, enforce trusted proxies, add health endpoints, and define connection/body limits, per-client quotas, and operator-only admin methods.~~ **DONE (v0.3-dev):** Dual `RpcServer` via `RpcMode::Public`/`RpcMode::Operator`, separate listeners in `main.rs`, trusted-proxy `is_ip_allowed`, `bud_health`/`bud_nodeInfo` endpoints, `max_request_body_size` (10MB/50MB), `max_connections` (500/10), per-IP rate limiting, `bud_adminBanPeer`/`bud_adminUnbanPeer`/`bud_adminListBannedPeers` gated by `require_operator()`.
4.  ~~Wire persistent P2P identity, discovery policy, DNS seeds, and durable peer bans.~~ **DONE (v0.3-dev):** `load_or_generate_identity_key()`, `with_identity()`/`with_banned_peer_db()`/`with_dns_seeds()` builders, durable ban JSON persistence, mDNS policy via `Network::security_config()`, DNS seed resolution and dialing.
5.  ~~Finish Snapshot V2 restore, authenticated distribution, chunk-session binding, replay equivalence, backup restore drills, and archive policy.~~ **DONE (v0.3-dev):** `AccountState::from_snapshot_v2()`, V2 as canonical format in `PruningManager`, `Blockchain` startup V2-first restore, P2P V2 transport, chunk `session_id` binding, `apply_v2_snapshot()` with full metadata + finality cert restore, replay equivalence verified, `archive_mode` flag disables pruning entirely. Scheduled backup restore drills remain (operational, not code).
6.  Keep governance, BudZKVM contracts, and pruning disabled for Mainnet v1 until separately reviewed.
7.  ~~Produce deployment packaging, release ceremony records, observability dashboards, incident runbooks, fault injection results, fuzzing results, and an external security audit.~~ **PARTIAL (v0.3-dev):** Docker multi-stage image, 4-node docker-compose + Prometheus, systemd unit, 4 cargo-fuzz targets, live Prometheus collector wiring. External audit and production runbooks remain.

## 4. Release Gates

Every release candidate should run:

```bash
nix develop --command cargo fmt --all -- --check
nix develop --command cargo clippy --workspace --all-targets --all-features -- -D warnings
nix develop --command cargo test --workspace
nix develop --command cargo build --release --locked
git diff --check
```

Current: **342 tests.** `cargo clippy -D warnings` passes on the CI-pinned Rust 1.94.0 toolchain; a newer local toolchain may surface new lints as clippy itself evolves.

## 5. What Remains for Mainnet v1

- External security audit
- Scheduled backup restore drills (archive-node policy itself is now implemented via `archive_mode`)
- Production runbooks and incident response procedures
