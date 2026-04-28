# Chapter 5.1: Persistent Storage (Sled DB)

This chapter explains how data moves from memory to disk, how the `sled` database is organized, and why Budlum uses a key-value design.

## 1. Data Structures: What Is Sled?

Budlum uses **Sled**, an embedded NoSQL key-value database, instead of a traditional SQL server.

### Why Sled?

1.  **Embedded:** no external PostgreSQL-style installation is needed.
2.  **Fast:** optimized for modern disk workloads.
3.  **Thread-safe:** many threads can read and write safely.

### Struct: `Storage`

`Storage` wraps the Sled database handle. Cloning it is cheap because it copies the handle, not the entire database.

## 2. Schema Design

Sled has keys and values, not tables. Budlum uses prefixes to keep data organized:

| Data | Key Format | Purpose |
| --- | --- | --- |
| Block | `{Hash}` | Store block bodies by hash. |
| Height | `HEIGHT:{Number}` | Find a block hash by height. |
| Transaction | `TX_IDX:{Hash}` | Find the block height for a transaction. |
| Account | `ACCT:{PubKey}` | Store per-account balance and nonce. |
| Mempool | `MEMPOOL:{Hash}` | Persist pending transactions. |
| QC Blob | `QC_BLOB:{Height}` | Audit checkpoint signatures. |
| Finality Cert | `FINALITY_CERT:{Height}` | Store finalized checkpoint proof. |
| State Root | `STATE_ROOT:{Height}` | Record canonical state root. |
| Canonical Height | `CANONICAL_HEIGHT` | Track canonical chain height. |
| Last Block | `LAST` | Point to the current tip. |
| Schema Version | `SCHEMA_VERSION` | Track migration level. |

## 3. Code Analysis

### Function: `commit_block`

Block commits are atomic. The block, height index, state root, transaction indexes, and finality metadata are written in one batch so crashes do not leave half-written canonical data.

### Per-Account Persistence

Instead of storing the entire state as one giant JSON blob, each account is stored independently under `ACCT:{PubKey}`. Updating one account writes only that account.

### Function: `load_chain`

At startup, Budlum reads the `LAST` pointer and walks backward through previous hashes until genesis, then reverses the list to rebuild the chain.

## 5. Metadata Consistency After Reorg

Reorgs update not only block bodies but also canonical metadata: height indexes, state roots, transaction indexes, finality certificates, QC blobs, and the `LAST` pointer.

## 6. Migrations and Snapshot Export

`Storage::new` runs migrations on startup and writes `SCHEMA_VERSION = 1`. Snapshot export can dump Sled key-value pairs as JSON for backup and recovery workflows.

