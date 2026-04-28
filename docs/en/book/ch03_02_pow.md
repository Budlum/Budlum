# Chapter 3.2: Proof of Work (PoW) Engine

Proof of Work is the classic Bitcoin-style consensus mechanism. It turns block production into a computational race where valid blocks must satisfy a difficulty target.

## 1. Data Structures: Mining Settings

### Struct: `PoWConfig`

`PoWConfig` defines mining difficulty, target block time, and adjustment intervals. These values determine how expensive block production should be.

### Struct: `PoWEngine`

`PoWEngine` owns the configuration and implements the consensus interface for mining and validation.

## 2. Algorithms: Mining and Difficulty

### Function: `mine`

Mining repeatedly changes `nonce`, recalculates the block hash, and checks whether the hash starts with the required target prefix. When the target is met, the block is ready.

### Why `nonce`?

The block content is otherwise fixed. A fixed input gives a fixed hash. `nonce` is the harmless variable that lets miners search for a different hash without changing the transaction payload.

### Function: `calculate_new_difficulty`

Difficulty adjustment keeps the network stable. If blocks are produced too quickly, difficulty increases. If they are too slow, difficulty decreases, while never dropping below the configured minimum.

### Design Decision: Damping

Budlum adjusts difficulty only when the observed period is far outside the expected period. This prevents constant oscillation and keeps block production smoother.

### Function: `validate_block`

Every node independently recalculates the hash and checks the target. The network never assumes that "someone else validated it." Verification is local and mandatory.

