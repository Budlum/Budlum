# Budlum Core Specification (v0.6)

## 1. Multi-Consensus Settlement

Budlum acts as a **Global Settlement Layer** for heterogeneous consensus domains. It ensures that state transitions across these domains are archived, verified, and settled deterministically.

### 1.1 Registry-First Archival
All domain commitments (headers, state roots, proofs) are first archived in the `DomainCommitmentRegistry`. This ensures that even if a domain is later frozen due to equivocation, the history remains available for audit and replay.

### 1.2 Equivocation Detection
If a domain producer signs two different commitments for the same height/slot, the settlement layer detects this as **equivocation**.
- **Action**: The domain is immediately marked as `Frozen` in the `ConsensusDomainRegistry`.
- **Slashing**: If the domain has an operator bond, the bond is slashed.

### 1.3 Atomic Persistence
Settlement state transitions (Commitment + Domain Height Update + Hash Update) are performed in a single storage batch to prevent partial state corruption during node crashes.

### 1.4 Deterministic Cross-Domain Settlement (v0.4)
Recording a commitment into `DomainCommitmentRegistry` (sequence/equivocation checks, `last_committed_height`) is separate from applying its `state_updates` to global account state.

- **Recording** happens immediately on `accept_domain_commitment` and only touches the arriving domain's own history — safe regardless of arrival order.
- **Settlement** — actually mutating global account state — is deferred to `Blockchain::settle_pending_domain_commitments()`, called at the start of `produce_block()`. It walks every registered domain in fixed ascending `domain_id` order (the `ConsensusDomainRegistry`'s `BTreeMap` iteration order) and drains each domain's next sequentially-ready commitment.
- **Conflict resolution**: if two domains' `state_updates` both target the same account and the settlement-time nonce check fails (`new_nonce <= current_nonce`) for one of them, that specific update is skipped — the domain's own commitment stays validly recorded, only its effect on global state is dropped. Because settlement always processes domains in the same fixed order on every node, the lowest-`domain_id` claim always wins, deterministically, regardless of which commitment reached the network first.
- Each `ConsensusDomain` tracks `last_settled_height` (how far settlement has progressed) separately from `last_committed_height` (how far the domain's own recorded sequence has progressed).

### 1.5 State-Root-Bound Transitions (v0.4)
`DomainCommitment.state_root` is a Merkle root over `state_updates` (`compute_state_updates_root`), not the domain's raw internal block state root:

- Each `(address, nonce)` entry hashes to a canonical leaf (`state_update_leaf_hash`); the root is computed the same way as `DomainEventTree`'s bridge-event Merkle tree.
- `accept_domain_commitment` rejects any commitment where `state_root != compute_state_updates_root(state_updates)`.
- Because `state_root` is part of `DomainCommitment::leaf_hash()`, and the commitment is already verified against the domain's real finality proof (PoW/PoS/PoA/BFT), this closes the gap where `state_updates` could be swapped independently of an already-finality-proven commitment: any change to the update set requires a different `state_root`, which requires a different finalized block.
- Use `DomainCommitment::insert_state_update(address, nonce)` to keep `state_updates` and `state_root` in sync — mutating `state_updates` directly produces a commitment that will be rejected at acceptance time.

### 1.6 Block-Level Settlement Replay (v0.5)

§1.4 makes settlement order-independent, but a receiving validator originally recomputed `settle_pending_domain_commitments()` from its *own* view of recorded domain commitments — which could differ from the producer's if the validator had already received (or was still missing) domain data the producer didn't have at production time, causing a valid block to be spuriously rejected or, worse, silently diverge without either node's `state_root` check ever seeing a mismatch on data outside the block itself.

- `Block` now carries `settlement_watermarks: BTreeMap<DomainId, u64>` — the exact per-domain `last_settled_height` the producer reached during `settle_pending_domain_commitments()` — and `settlement_batch_root`, both fed into the block's own hash.
- `Blockchain::replay_settlement_to_watermarks(targets)` applies settlement bounded by explicit per-domain target heights, never further, even if more domain commitments are already locally available. `settle_pending_domain_commitments()` is now a thin wrapper calling this with each domain's `last_committed_height` as the target.
- `validate_and_add_block()` first checks `block.settlement_batch_root` against the recomputed root (see §1.9), then replays settlement bounded by `block.settlement_watermarks` before computing `commit_state` — so a validator reaches byte-identical account state to the producer for this block, rather than independently deciding how much domain data to settle. If the watermarks require a domain commitment the validator hasn't recorded yet, replay fails closed (a `MissingDomainCommitment:`-prefixed error — see §3.5) rather than skipping or guessing.

### 1.7 Atomic Settlement & Block Application (v0.6)

Through v0.5, `replay_settlement_to_watermarks` mutated live account state and the live `ConsensusDomainRegistry` directly, and persisted each settled domain's cursor via its own individual, error-swallowing `save_consensus_domain` call — independent of whether the rest of block validation (transaction application, state-root check, durable commit) actually succeeded.

- Both `produce_block()` and `validate_and_add_block()` now replay settlement into **temporary** `AccountState`/`ConsensusDomainRegistry` clones. `self.state`/`self.domain_registry` are only swapped to the new values *after* `commit_block_durable()` succeeds.
- `DurableCommitBatch` gained a `settled_domains: Vec<ConsensusDomain>` field, written into the *same* atomic `sled` batch as the block, account balances, and other consensus state (`Storage::commit_durable_batch`) — settlement cursors are no longer persisted via a separate, independently-failable write.
- A failed transaction, a bad state root, a missing domain commitment, or a disk error during commit now leaves `self.state`, `self.domain_registry`, and the canonical chain completely unchanged; nothing partially applies.

### 1.8 Startup & Reorg Share the Watermark-Bounded Replay Path (v0.6)

Through v0.5, node startup unconditionally applied *every* stored domain commitment's `state_updates` to account state regardless of whether that commitment had ever actually been included in a block's `settlement_watermarks` — so a commitment recorded via commitment gossip but never settled by any block could still mutate state on a plain restart. Reorg never rebuilt `domain_registry` settlement cursors at all, and switched `self.chain`/`self.state` over to the new chain *before* durably persisting it, risking an in-memory/on-disk split on a failure partway through.

- `Blockchain::rebuild_state_and_registry(chain)` replays a chain from genesis using each historical block's own `settlement_watermarks` via `replay_settlement_to_watermarks` — the same function block validation uses. Startup, `try_reorg()`, and `get_state_snapshot()` all use it instead of separately-implemented replay logic.
- `try_reorg()` now durably persists every post-fork block of the *new* chain (via `commit_block_durable`, propagating any error with `?` rather than panicking) before touching `self.chain`, `self.state`, `self.domain_registry`, or deleting the old chain's blocks. A failure at any point during that persistence loop leaves the node exactly on its old, still-canonical chain.

### 1.9 Canonical Commitment Identity (v0.6)

Through v0.5, equivocation/duplicate-resubmission checks in `accept_domain_commitment` compared raw `commitment.domain_block_hash` (and, in one branch, `sequence`) to decide whether an incoming commitment was "the same one already recorded." A resubmission sharing the same `domain_block_hash` but carrying a materially different `state_root`/`state_updates` could be silently accepted as a harmless duplicate instead of flagged as equivocation.

- Both dedup checks now compare `existing.commitment_payload_hash() == commitment.commitment_payload_hash()` (§3.3.4) instead of the raw block hash. `commitment_payload_hash()` deliberately excludes `sequence` — a caller-assigned submission counter, not domain state — so a legitimate resubmission of the same commitment under a different sequence number still counts as identical, while a same-block-hash resubmission with a different `state_root` is correctly caught as equivocation.
- `compute_settlement_batch_root` now hashes the ordered list of `commitment_payload_hash()` values for every commitment actually applied during a settlement pass (a Merkle root over commitment identities), not a hash of the raw watermark map — so the batch root reflects exactly *which* commitments were applied, not just how far each domain's cursor moved.
- `validate_and_add_block()` rejects a block whose `settlement_watermarks` would regress any domain below its current (`self.domain_registry`) `last_settled_height` — a regression can't be caught by the batch-root check alone, since settling nothing for a domain is still a self-consistent, correctly-rooted empty batch.

---

## 2. Validator Economics (PoS)

### 2.1 Slashing Evidence
Double-signing evidence consists of two conflicting headers signed by the same validator.
- **Propagation**: Evidence is gossiped via `NetworkMessage::SlashingEvidence`.
- **Execution**: When evidence is included in a block, the validator's stake is reduced by `slash_ratio_fixed` and they are moved to the `jailed` state.

### 2.2 Block Rewards
Rewards are calculated per block as `total_fees + block_reward`. They are credited to the producer's account balance during block execution.

---

## 3. Network Protocol

### 3.1 Handshake & Sync
On connection, nodes exchange `Handshake` messages. If a peer reports a higher height, a `GetHeaders` request is automatically triggered.

### 3.2 BLS Finality Protocol (v0.3)

The finality protocol uses BLS12-381 signatures for aggregated threshold verification.

#### 3.2.1 BLS Key Management
- `BlsKeypair` (secret key: `Scalar`, public key: 96-byte G2 compressed) is stored in `ValidatorKeys::bls_key`.
- `ConsensusEngine` trait exposes `bls_secret_key() -> Option<Scalar>` and `bls_public_key() -> Option<Vec<u8>>`.
- PoS engine populates BLS keys from `ValidatorKeys`; PoW/PoA return `None`.

#### 3.2.2 Vote Signing
- `sign_bls(sk, msg)` hashes the message to a G1 curve point (`hash_to_g1`) and multiplies by the secret key, producing a 48-byte compressed G1 signature.
- `verify_bls_sig(pk, msg, sig)` verifies the pairing: `e(sig, G2_gen) == e(H(msg), pk)`.
- `hash_to_g1` (v0.5) uses `bls12_381`'s built-in RFC 9380 hash-to-curve (`ExpandMsgXmd<Sha256>` + SSWU, domain-separated with `BUDLUM_BLS_SIG_V2_BLS12381G1_XMD:SHA-256_SSWU_RO_`). The prior construction computed `H(m) = scalar_hash(m) · G` — a public, deterministic scalar multiple of the generator — which meant `H(m₂) = (s₂/s₁) · H(m₁)` for any two messages was computable without the secret key, so any valid signature could be rescaled into a valid signature over an *arbitrary different message* (`σ₂ = (s₂/s₁) · σ₁`). RFC 9380 hash-to-curve maps messages to points with no known discrete-log relationship to each other or to `G`, closing this universal forgery.
- The regression test (v0.6) reconstructs the *exact* old vulnerable scalar derivation (SHA3-256 with the `BUDLUM_BLS_SIG_DST` domain tag, matching the historical `hash_to_g1`) to compute the real `s₂/s₁` ratio and forge `σ₂ = (s₂/s₁) · σ₁`, rather than testing against an arbitrary wrong scalar — verified to succeed against the old implementation and fail against the current one.

#### 3.2.3 Protocol Phases
At each checkpoint height (`FINALITY_CHECKPOINT_INTERVAL = 10`):

1. **Prevote Phase**: Started automatically when a node produces a checkpoint block. Validators sign prevotes with their BLS secret key via `Blockchain::sign_prevote()`. Votes are broadcast via GossipSub.

2. **Precommit Phase**: The periodic voting loop polls `get_aggregator_state()`. When prevote quorum (2/3 stake) is detected, validators automatically sign and broadcast precommits via `Blockchain::sign_precommit()`.

3. **Certificate Production**: Once precommit quorum is reached, `FinalityAggregator::try_produce_cert()` aggregates all G1 precommit signatures, produces a signer bitmap, and creates a `FinalityCert`. The cert is gossiped network-wide.

#### 3.2.4 Certificate Verification
`FinalityCert::verify(snapshot)`:
0. **(v0.6)** Calls `snapshot.verify_self_consistent()` first — see §3.2.5. Without this, the checks below would verify a cert against whatever `validators` list happens to accompany a snapshot, regardless of whether that list is what the snapshot's own `set_hash`/`total_stake` actually claim.
1. Validates `set_hash` and epoch match.
2. Builds signer list from bitmap, sums voted stake (overflow-checked, v0.6), checks ≥ quorum.
3. For each signer, parses its BLS public key and (v0.6) rejects an identity-point key and requires a valid proof-of-possession (`verify_pop`) before including it in the aggregate — see §3.2.5.
4. Aggregates G2 public keys of signers.
5. Verifies BLS pairing: `e(agg_sig, -G2_gen) + e(H(msg), agg_pk) == 0`.

The gossip path: `GossipSub` → `Node` → `ChainHandle::handle_prevote/handle_precommit` → `ChainActor` → `Blockchain::finality_aggregator`.

#### 3.2.5 Validator Snapshot Self-Consistency (v0.6)

A `ValidatorSetSnapshot` arriving inside a `FinalityProof` (domain-level PoS/PoA/BFT quorum certs) is attacker-suppliable data, not something the receiver generated itself. Through v0.5, `FinalityCert::verify` only compared `self.set_hash` against `snapshot.set_hash` — both fields inside the same untrusted structure — never checking that `set_hash`/`total_stake` were actually derived from `snapshot.validators`. An attacker could set `snapshot.set_hash` to a domain's real, registered hash while populating `snapshot.validators` with their own keypairs; since they hold the matching secret keys, they could produce a cert that passed every existing check. Separately, `verify_pop()` existed but was called nowhere outside its own unit test, and an identity-point BLS public key trivially satisfies the pairing equation (`e(sig=O, G2_gen) == e(H(m), pk=O)` both degenerate to the `Gt` identity) with no real key at all — the classic BLS rogue-key gap proof-of-possession exists to close.

- `ValidatorSetSnapshot::verify_self_consistent()`: recomputes `compute_hash(&self.validators)` and the checked-sum of stakes, rejecting the snapshot if either doesn't match the claimed `set_hash`/`total_stake`; also requires `validators` to be strictly sorted ascending by address (rejects duplicates and enforces the one canonical order the signature bitmap indexes into). Called at the start of every `FinalityCert::verify`.
- For each signer bit set in the cert's bitmap, `verify()` additionally rejects an identity-point BLS public key and requires `verify_pop(validator)` to pass before that signer's key is added to the aggregate.

### 3.3 Domain Finality Adapters (v0.4)

Each `ConsensusKind` a domain registers under has a corresponding `DomainFinalityAdapter` that `verify_finality(domain, commitment, proof)` before a `VerifiedDomainCommitment` is accepted.

#### 3.3.1 PoW: Real Header-Chain Verification
`FinalityProof::PoW { headers: Vec<PoWHeaderProof> }` — a chain of headers, `headers[0]` mining the committed block, each subsequent header extending it.

- `PoWHeaderProof { prev_hash, target, nonce, timestamp_ms, extra }`; `hash()` is `SHA256("BDLM_POW_HEADER_V1" || prev_hash || target || nonce || timestamp_ms || extra)`.
- Validity requires, for every header: `hash() <= target` (real proof-of-work), and `target <= domain.min_pow_target` (the domain's registered difficulty floor — operators must set this to reflect their chain's real difficulty; the default `[0xFF; 32]` provides no real security).
- `headers[0].extra` must equal `commitment.commitment_payload_hash()` (v0.5; previously just `commitment.domain_block_hash` — see 3.3.4), binding the proof-of-work chain to this specific commitment.
- Confirmation depth is `headers.len() - 1`, computed from real, independently-verified headers — not a submitted number.
- This verifies a Budlum-defined header format's own proof-of-work; it does not sync or validate an external chain's actual header history (e.g. Bitcoin-compatible headers) — that remains future work.

#### 3.3.2 PoA / BFT / PoS: Shared BLS Quorum Verification
`FinalityProof::PoA` and `FinalityProof::Bft` now carry the same `{ cert: FinalityCert, validator_snapshot: ValidatorSetSnapshot }` shape as `FinalityProof::PoS`, verified by a shared `verify_quorum_cert()`:

1. `cert.checkpoint_height` must match `commitment.domain_height`, and `cert.checkpoint_hash` must equal `hex(commitment.commitment_payload_hash())` (v0.5; see 3.3.4).
2. `validator_snapshot.set_hash` must match `cert.set_hash` **and** the domain's registered `validator_set_hash` — unconditionally; there is no zero-hash bypass (see 3.3.3).
3. `cert.verify(validator_snapshot)` performs the real BLS pairing check described in 3.2.4.

A self-reported `signer_count`/`validator_count` is no longer sufficient for PoA or BFT domains to reach `Finalized` — a genuine BLS aggregate signature from the domain's registered validator set is required, exactly as for PoS.

#### 3.3.3 Validator-Set Registration Requirement
`validate_consensus_domain_registration` rejects registering a PoS/PoA/BFT-kind domain with `validator_set_hash == [0u8; 32]`. Previously, a zero hash caused the binding check above to be skipped entirely, letting any attacker-generated key set produce an accepted finality certificate for that domain. Operators must register a real validator set hash.

#### 3.3.4 Finality Proofs Bind `state_root`, Not Just `domain_block_hash` (v0.5)

§1.5 makes `state_updates` self-consistent with `state_root`, but through v0.4 no finality proof actually attested to *which* `state_root` accompanied a given `domain_block_hash` — PoW's `extra` field and the BLS quorum cert's `checkpoint_hash` both bound only `domain_block_hash`. Since `state_updates`/`state_root` are Budlum-side settlement data, not part of an external domain's own block hash, an attacker (or a malfunctioning producer) could take one commitment's exact finality proof and pair it with a *different*, still internally-self-consistent `state_updates`/`state_root` for the same domain block, and have it accepted as equally finalized — silently rewriting the nonce updates a finalized commitment actually settles.

- `DomainCommitment::commitment_payload_hash()` hashes domain identity, position, and every root the commitment claims (`domain_id`, `domain_height`, `domain_block_hash`, `parent_domain_block_hash`, `state_root`, `tx_root`, `event_root`, `consensus_kind`, `validator_set_hash`) — deliberately excluding `finality_proof_hash` (only known after a proof exists — see above) and (v0.6) `sequence`, a caller-assigned submission counter rather than domain state (see §1.9).
- Both the PoW `extra` binding (3.3.1) and the quorum-cert `checkpoint_hash` binding (3.3.2) now check against `commitment_payload_hash()` instead of the bare `domain_block_hash`. Reusing a valid finality proof against a commitment with a swapped `state_updates`/`state_root` now fails verification, because the payload hash the proof was produced over no longer matches.

### 3.4 JSON-RPC API (`bud_`)

The node exposes a standard JSON-RPC 2.0 interface via **two separate listeners** (public + operator).

#### Public Listener (default: `0.0.0.0:8545`)
- API key auth, CORS allowlists, per-IP rate limiting
- Trusted proxy validation (only configured proxies may set `X-Forwarded-For`)
- 10MB body limit, 500 max connections

#### Operator Listener (default: `127.0.0.1:8546`)
- Localhost-only, no auth, no rate limiting
- 50MB body limit, 10 max connections

| Method | Description |
|--------|-------------|
| `bud_chainId` | Returns the chain ID. |
| `bud_blockNumber` | Returns the latest block height. |
| `bud_sendRawTransaction` | Submits a signed transaction. |
| `bud_registerConsensusDomain` | Registers a new consensus domain. |
| `bud_submitVerifiedDomainCommitment` | Submits a verified domain commitment. |
| `bud_syncing` | Returns true if the node is currently syncing. |
| `bud_health` | Health status: `status`, `blockHeight`, `peerCount`, `syncing`. |
| `bud_nodeInfo` | Node identity: `chainId`, `peerId`, `validatorSetHash`, `rpcMode`. |
| `bud_adminBanPeer` | **Operator-only.** Ban a peer by libp2p PeerId. Rejected with an error on the public listener. |
| `bud_adminUnbanPeer` | **Operator-only.** Lift a ban on a peer by libp2p PeerId. Rejected with an error on the public listener. |
| `bud_adminListBannedPeers` | **Operator-only.** List currently banned peer IDs. Rejected with an error on the public listener. |

### 3.5 Missing-Commitment Retry Queue (v0.6)

Through v0.5, a block that arrived before the domain commitment(s) its `settlement_watermarks` require was rejected the same way a genuinely invalid block was — `validate_and_add_block` returned an error, and the network layer penalized the sending peer for it, even though the block itself could be entirely legitimate and simply arrived ahead of commitment gossip.

- `replay_settlement_to_watermarks` returns a `MissingDomainCommitment:`-prefixed error when a required commitment hasn't been recorded yet, distinguishing "we're missing data" from every other validation failure.
- `Blockchain::pending_blocks: HashMap<String, Block>` queues such blocks by hash instead of discarding them. `retry_pending_blocks()` — called whenever a new domain commitment is accepted — retries every queued block, keeping it queued if it's still missing data, dropping it only on a genuine (non-missing-data) validation failure.
- `src/network/node.rs`'s direct-gossip block handler checks for the `MissingDomainCommitment:` prefix and does not call `report_invalid_block` for it, so a peer isn't penalized merely for gossip arriving out of order.

---

## 4. Security & Signing

### 4.1 ConsensusSigner Trait
Block signing is abstracted behind the `ConsensusSigner` trait (`src/crypto/signer.rs`):

- **`KeyPairSigner`**: Local Ed25519 key file (devnet/testnet default).
- **`Pkcs11Signer`**: Hardware Security Module via `cryptoki` (mainnet requirement). Loads the PKCS#11 module, opens a session on the configured slot, authenticates with the token PIN, and signs via CKM_EDDSA.

Both backends are injected into `PoSEngine` and `PoAEngine` via `with_signer()` constructors.

### 4.2 P2P Security (v0.3)

- **Persistent Identity**: `load_or_generate_identity_key(path)` loads the P2P Ed25519 keypair from disk or generates and saves a new one. Preserves `PeerId` across restarts.
- **Durable Peer Bans**: Banned `PeerId`s are persisted to JSON every 5 minutes and reloaded on startup. Controlled by `persist_banned_peers` in `SecurityConfig`.
- **mDNS Policy**: Mainnet and Testnet disable mDNS; Devnet enables it. Controlled by `mdns_enabled` in `SecurityConfig`.
- **DNS Seeds**: `resolve_dns_seeds(seeds, port)` resolves DNS hostnames to `/ip4/` or `/ip6/` multiaddrs and dials them at startup.

### 4.3 Storage Architecture

Budlum uses a trait-based storage abstraction (`BlockchainStorage`) currently implemented via `sled`.

- **Prefixes**:
  - `ACCT:<addr>`: Account data.
  - `BLOCK:<hash>`: Full block data.
  - `DOMAIN:<id>`: Domain configuration and state.
  - `DOMAIN_COMMITMENT:<id>:<height>:<seq>`: Archived commitments.
  - `QC_BLOB:<height>`: Quorum Certificate blobs.
  - `FINALITY_CERT:<height>`: Finality certificates.
  - `GLOBAL_HEADER:<height>`: Global settlement headers.

---

## 5. State Snapshot V2 (v0.3)

### 5.1 Format
`StateSnapshotV2` (`schema_version = 2`) captures:
- Chain identity, balances, nonces, validators (with BLS/PQ keys)
- Consensus metadata: epoch index, base fee, block reward
- Cross-domain roots: bridge, message, settlement, global header summary
- Unbonding queue and verified finality certificates
- SHA3-256 integrity hash

### 5.2 Restore
- `AccountState::from_snapshot_v2()` preserves all consensus metadata for replay equivalence.
- Startup tries V2 snapshot first; falls back to V1.
- P2P snapshots embed V2 data for backward-compatible transport.
- `apply_v2_snapshot()` restores finality certificates alongside account state.

### 5.3 Chunk-Session Binding
`SnapshotChunk` carries a random `session_id`. Receivers reject chunks with mismatched session IDs, preventing cross-peer chunk mixing.

---

## 6. Global Settlement Headers (v0.4)

`GlobalBlockHeader` is Budlum's top-level settlement commitment: a hash chain of `domain_registry_root`, `domain_commitment_root`, `message_root`, `bridge_state_root`, `replay_nonce_root`, and `settlement_finality_root`, produced by `Blockchain::build_global_header()` and persisted by `seal_global_header()`.

### 6.1 Real Account State Commitment
`global_state_root` is the Merkle root of Budlum's own real account state (`AccountState::calculate_state_root()`, reused from the block that produced it), not a value derived only from domain/bridge/message data. Without it, two nodes could seal structurally-identical global headers while their underlying account state had actually diverged.

### 6.2 Honest Finalization Status
`global_state_finalized` is `true` only when `global_state_root` was taken from a chain height already covered by a BLS finality certificate (`finalized_height`) **and** a validator committee actually exists (`AccountState::get_active_validators()` is non-empty) — a chain with no registered validators can never claim BLS finality regardless of what `finalized_height` defaults to. When either condition fails, `global_state_root` reflects the current, unfinalized chain tip and `global_state_finalized` is `false`.

### 6.3 Known Limitation
`seal_global_header()` remains a local, synchronous call — it is not yet gated behind a dedicated settlement-level consensus round requiring a fresh quorum certificate per seal. `global_state_finalized` reports whether the *account state it references* is BLS-finalized; it does not mean the *act of sealing this specific global header* was itself subject to a settlement-level vote. Closing that gap is tracked as future work (see README Research Roadmap).

### 6.4 Explicit Underlying-Block Binding (v0.6)

Through v0.5, `global_state_root` referenced *a* block's account-state root, but the header never recorded *which* block that was — two headers carrying the same root value but different provenance (e.g. after a bug or attack picked the wrong source block) were indistinguishable from the header alone.

- `GlobalBlockHeader` now carries `underlying_block_height: u64` and `underlying_block_hash: Hash32` — the exact block `global_state_root` was taken from (the same `state_root_source` block §6.1/§6.2 already select) — both fed into `calculate_hash_bytes()`, so tampering either independently changes the header's own hash.
