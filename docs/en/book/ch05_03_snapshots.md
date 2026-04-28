# Chapter 5.3: Snapshots and Data Pruning

Snapshots and pruning keep a blockchain sustainable over time. Without them, historical data grows forever and new nodes take too long to join.

## 1. Problem: Chain Bloat

Every block, transaction, state root, certificate, and index consumes disk. A long-lived network must decide which data is required for safety and which data can be archived or pruned.

## 2. Data Structures: Snapshot

### Struct: `Snapshot`

A snapshot records a height, block hash, state root, accounts, validator set, and enough metadata to restart from a known-good point.

Hardening improves snapshots by aligning them with finalized checkpoints, not arbitrary heights.

## 3. Algorithms: Pruning Logic

### Function: `create_snapshot`

`create_snapshot` captures the current canonical state and writes it to a portable representation. This is the recovery point.

### Function: `prune_history` and Pruning Hook

Pruning removes historical data older than the retention window while preserving finalized safety boundaries and required indexes.

### Why Finality Awareness?

Pruning behind non-final data is dangerous. Budlum prunes only with finality awareness so the node never deletes data it might need for a valid reorg.

## 3.5 Replay Semantics

Snapshots must preserve enough information for a node to replay forward deterministically from the snapshot height.

### Why a Safety Margin?

A safety margin keeps recent history even after finality, giving operators room for audits, diagnostics, and delayed peer sync.

## 4. State Sync

New nodes can download a recent snapshot, verify its root, and then sync only the remaining blocks. This turns multi-day bootstrap into a much shorter process.

## Summary

1.  **Disk savings:** old unnecessary data does not accumulate forever.
2.  **Speed:** new nodes can join quickly.
3.  **Sustainability:** the chain can run for years without endless storage growth.

