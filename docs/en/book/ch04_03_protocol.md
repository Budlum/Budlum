# Chapter 4.3: Network Protocol and Messaging

This chapter explains the language Budlum machines use to talk to one another: `NetworkMessage`, serialization, sync requests, and network limits.

## 1. Data Structures: Shared Language

# Protocol Messages and Serialization

Budlum nodes use a custom `NetworkMessage` protocol for peer-to-peer communication and data sharing.

## What Does `NetworkMessage` Include?

1.  **Handshake / HandshakeAck:** new peers verify version and `chain_id`. Hardening Phase 2 also verifies `validator_set_hash` and `supported_schemes`.
2.  **Block:** broadcasts a newly produced block.
3.  **Transaction:** broadcasts new transactions.
4.  **Prevote / Precommit:** BLS finality votes.
5.  **FinalityCert:** threshold-signed proof that a checkpoint was finalized.
6.  **GetQcBlob / QcBlobResponse:** shares Dilithium-signed blob packages for optimistic QC verification.
7.  **QcFaultProof:** broadcasts proof bytes for invalid PQ attestations.
8.  **NewTip and sync messages:** used for chain synchronization, including requests such as `GetBlocksByHeight`.

The full list should be checked in `src/network/protocol.rs`.

## Publishing with Gossipsub

Budlum Core uses **Gossipsub** for broad announcements such as blocks and transactions. Gossip is fast, but it is not ideal for large historical transfers.

## Request-Response Synchronization

Production hardening moves large transfers, such as downloading old blocks, to one-to-one **Request-Response** sync.

-   **Protocol ID:** `/budlum/sync/1.0.0`
-   **SyncCodec:** length-prefixed serialization over streams.
-   **Actor integration:** `Node` forwards incoming requests to `ChainActor`, serving blocks and headers without global locking.

This reduces network traffic because sync asks a specific peer for a specific range instead of shouting to the whole network.

## QC Messages

-   `GetQcBlob { epoch, checkpoint_height }`: asks peers for the PQ sidecar required by a checkpoint.
-   `QcBlobResponse { epoch, checkpoint_height, checkpoint_hash, blob_data, found }`: carries the serialized blob. The receiver parses it, checks metadata, verifies the Merkle root and Dilithium signatures, persists it as `QC_BLOB:{height}`, and retries pending finality certificates for that checkpoint.
-   `QcFaultProof { proof_data }`: carries a serialized `QcFaultProof`. The receiver verifies it against the stored blob and validator snapshot before applying the verdict.

## Serialization

Budlum uses a hybrid serialization model:

-   **Protobuf:** high-performance network messages and core structures.
-   **Serde JSON:** readable high-level configuration and diagnostic data.
-   **Bincode:** deterministic byte-for-byte encoding for slashing evidence and similar structures.

### Why Protobuf?

JSON is readable but heavier. Protobuf creates smaller binary payloads and reduces CPU and bandwidth cost.

## 3. Limits and Security

Network input is untrusted. Message size limits prevent memory exhaustion attacks, and oversized blocks or messages are rejected automatically.
