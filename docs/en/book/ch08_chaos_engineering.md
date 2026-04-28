# Chapter 8: Chaos Engineering and Tests

Chaos engineering checks whether Budlum remains safe when the network behaves badly.

## 1. Test Categories

Budlum tests cover integration behavior, hardening rules, network failure simulation, ZKVM execution, and performance.

## 2. Chaos Test Scenarios

1.  **Network partitioning:** communication between nodes is cut and later restored while finality must remain safe.
2.  **Reorg protection:** unexpectedly deep reorganizations are rejected when they conflict with finality rules.
3.  **Sync corruption:** nodes that send bad chain data are detected and the correct chain is recovered.

## 3. Running Tests

Run the relevant Rust test targets from the repository root with `cargo test`. More focused tests can be run by test name or module when developing a specific subsystem.

## 4. Philosophy: Fail Early, Fail Safely

Distributed systems fail. The goal is to fail in ways that preserve safety: reject bad data, log useful context, keep honest peers connected, and avoid panics from malformed input.

