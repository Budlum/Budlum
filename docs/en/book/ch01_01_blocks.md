# Chapter 1.1: Block Structure and Chain Architecture

This chapter explains the **block**, the most fundamental unit of a blockchain, by looking at why each field exists and what security property it supports.

## 1. Data Structures: What Do We Store, and Why?

Blockchain data cannot be stored casually. Every byte has a purpose and a cost. In Budlum, the block structure is split into `BlockHeader` and `Block`.

### Struct: `BlockHeader`

The header is the block's identity. It does not contain transaction payloads; it contains metadata, roots, and proofs.

Important fields:

| Field | Why It Exists |
| --- | --- |
| `index` | The block's position in the chain. The genesis block is `0`. |
| `timestamp` | Proves when the block was produced and feeds PoW difficulty and PoS epoch logic. |
| `previous_hash` | Links the block to its parent. If the parent changes by one bit, this link breaks. |
| `hash` | The SHA3-256 digest of the header. It is the block's identifier. |
| `producer` | The miner or validator public key that produced the block. |
| `chain_id` | Separates Mainnet, Testnet, and Devnet and prevents replay across networks. |
| `state_root` | Merkle root of account state. Light clients can verify balances against it. |
| `tx_root` | Merkle root of transactions inside the block. |
| `slashing_evidence` | Optional PoS evidence for validators that signed conflicting data. |
| `nonce` | PoW trial counter. It may be zero in PoS mode. |
| `epoch` and `slot` | Time and validator-set scheduling metadata. |
| `vrf_output` and `vrf_proof` | Proof that a validator was selected as leader. |
| `validator_set_hash` | Digest of the active validator set used by the finality layer. |

### Struct: `Block`

A full block contains the header fields plus the payload:

-   `transactions`: the list of transfers, stake operations, votes, or contract calls.
-   `signature`: the producer's signature proving that the block was authorized by the claimed producer.

## 2. Algorithms and Functions

### Function: `calculate_hash`

The block hash is computed from a deterministic sequence of fields. Budlum uses:

1.  **Domain separation** with `b"BDLM_BLOCK_V2"` so transaction hashes and block hashes cannot collide semantically.
2.  **Deterministic serialization** with `bincode` for complex structures such as `slashing_evidence`.
3.  **Little-endian byte order** through `to_le_bytes`, ensuring identical hashes across CPU architectures.
4.  **Complete field coverage**, including nonce, timestamp, roots, VRF data, and validator set references.

This produces the avalanche property: even a tiny change creates a completely different hash.

### Function: `calculate_tx_root`

The transaction root represents thousands of transactions with one 32-byte hash. Transactions are hashed as leaves, paired upward, and reduced until a single Merkle root remains. If the number of leaves is odd, the last leaf is duplicated.

This is what makes SPV-style light clients possible: a client can prove that a transaction exists using only a Merkle path instead of downloading the whole block.

### Function: `verify_signature`

Block verification never trusts `self.hash` blindly. The node recalculates the block hash from the local data, compares it with the advertised hash, decodes the producer key, and verifies the Ed25519 signature.

The result is a mathematical guarantee that the block content was not modified and that it was signed by the corresponding private key.

### Function: `mine`

In PoW mode, mining repeatedly changes `nonce` and recalculates the hash until the result satisfies the difficulty target, such as a prefix of zeroes.

This work is intentionally expensive. To rewrite history, an attacker would need to redo the work for the changed block and every block after it, which makes tampering economically impractical.

