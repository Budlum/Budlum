# Budlum Core Specification (v0.3)

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
- `sign_bls(sk, msg)` hashes the message to G1 (`hash_to_g1`) and multiplies by the secret key, producing a 48-byte compressed G1 signature.
- `verify_bls_sig(pk, msg, sig)` verifies the pairing: `e(sig, G2_gen) == e(H(msg), pk)`.

#### 3.2.3 Protocol Phases
At each checkpoint height (`FINALITY_CHECKPOINT_INTERVAL = 10`):

1. **Prevote Phase**: Started automatically when a node produces a checkpoint block. Validators sign prevotes with their BLS secret key via `Blockchain::sign_prevote()`. Votes are broadcast via GossipSub.

2. **Precommit Phase**: The periodic voting loop polls `get_aggregator_state()`. When prevote quorum (2/3 stake) is detected, validators automatically sign and broadcast precommits via `Blockchain::sign_precommit()`.

3. **Certificate Production**: Once precommit quorum is reached, `FinalityAggregator::try_produce_cert()` aggregates all G1 precommit signatures, produces a signer bitmap, and creates a `FinalityCert`. The cert is gossiped network-wide.

#### 3.2.4 Certificate Verification
`FinalityCert::verify(snapshot)`:
1. Validates `set_hash` and epoch match.
2. Builds signer list from bitmap, sums voted stake, checks ≥ quorum.
3. Aggregates G2 public keys of signers.
4. Verifies BLS pairing: `e(agg_sig, -G2_gen) + e(H(msg), agg_pk) == 0`.

The gossip path: `GossipSub` → `Node` → `ChainHandle::handle_prevote/handle_precommit` → `ChainActor` → `Blockchain::finality_aggregator`.

### 3.3 JSON-RPC API (`bud_`)

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
