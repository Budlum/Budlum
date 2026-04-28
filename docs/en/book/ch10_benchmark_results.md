# Chapter 10: Benchmark Results

Budlum benchmarks measure both small internal operations and the larger single-node transaction pipeline.

## Micro-Benchmarks

Micro-benchmarks isolate cryptography, hashing, Merkle root updates, storage writes, and transaction validation. They help identify regressions before they become full-system bottlenecks.

## Internal Pipeline (Single Node)

The single-node pipeline measures how quickly a node can accept transactions, validate them, update state, build blocks, and persist results.

## Analysis and Bottlenecks

### 1. Transaction Ingestion

RPC and gossip intake must validate quickly enough to reject bad transactions without starving honest traffic.

### 2. State Root Updates

Incremental Merkle updates are critical. Recomputing full state for every transaction would cap throughput sharply.

### 3. Execution Power

BudZKVM and contract execution need strict gas limits and proof checks so execution cost stays predictable.

