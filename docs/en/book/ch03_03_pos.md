# Chapter 3.3: Proof of Stake (PoS) Engine and RANDAO

Proof of Stake replaces computational work with economic collateral. Validators lock stake and earn the right to propose or vote according to deterministic selection rules.

## 1. Data Structures: Rules of the Game

### Structs: `PoSConfig` and `PoSEngine`

`PoSConfig` defines slot duration, epoch length, stake thresholds, slashing parameters, and timing rules. `PoSEngine` applies those rules while implementing the consensus interface.

## 2. Algorithms: Leader Selection and Penalties

### Function: `expected_proposer`

Budlum uses VRF-style leader selection so that the proposer for a slot can be verified by everyone but cannot be easily manipulated in advance.

### Determinism and Fixed-Point Math

Mainnet hardening removed `f64` from consensus-critical calculations. Fixed-point arithmetic guarantees the same result on macOS, Windows, Linux, and different CPU architectures.

### Slot-Based Deterministic Timestamps

Slot time is derived from deterministic chain parameters. This prevents validators from gaining unfair advantage by manipulating timestamps.

## 3. Slashing Evidence

### Double Proposal

Producing two different blocks for the same slot is slashable. Evidence proves the conflict and allows the state machine to penalize the validator.

### Function: `record_block`

`record_block` persists observed proposals and acts as a detective: if a validator signs conflicting blocks, the system can detect it later.

Detected evidence is not trapped inside the detecting node. The PoS engine exposes pending evidence to the chain actor, the node gossips it as `NetworkMessage::SlashingEvidence`, and producers include pending evidence in blocks so every honest node can apply the same slash.

### Why RANDAO / XOR-Mix?

RANDAO-style mixing reduces the ability of any single validator to control randomness. The network combines contributions rather than trusting one source.

### Function: `prepare_block`

`prepare_block` fills consensus metadata, verifies proposer eligibility, and prepares the block for validation by the rest of the network.

## Summary

PoS gives Budlum energy efficiency, deterministic scheduling, devnet-grade reward distribution, and economic security through executable slashing.
