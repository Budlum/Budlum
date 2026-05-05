# Budlum Core Specification (v0.1)

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

### 3.2 JSON-RPC API (`bud_`)
The node exposes a standard JSON-RPC 2.0 interface.

| Method | Description |
|--------|-------------|
| `bud_chainId` | Returns the chain ID. |
| `bud_blockNumber` | Returns the latest block height. |
| `bud_sendRawTransaction` | Submits a signed transaction. |
| `bud_registerConsensusDomain` | Registers a new consensus domain. |
| `bud_submitDomainCommitment` | Submits a commitment from a domain. |
| `bud_syncing` | Returns true if the node is currently syncing. |

---

## 4. Storage Architecture

Budlum uses a trait-based storage abstraction (`BlockchainStorage`) currently implemented via `sled`.

- **Prefixes**:
  - `ACCT:<addr>`: Account data.
  - `BLOCK:<hash>`: Full block data.
  - `DOMAIN:<id>`: Domain configuration and state.
  - `DOMAIN_COMMITMENT:<id>:<height>:<seq>`: Archived commitments.
