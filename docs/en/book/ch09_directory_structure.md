# Chapter 9: Directory Structure and Modularity

As Budlum grows, it uses a **layered architecture** to keep complexity manageable.

## 1. Current Directory Structure

```text
├── config/                  # Network configuration files
├── mainnet.toml             # Mainnet node profile
├── src/
│   ├── main.rs              # Application entry point, CLI and service runner
│   ├── lib.rs               # Public module exports
│   ├── cli/                 # CLI commands and argument handling
│   ├── core/                # Core data structures: Block, Tx, Account, Config, Metrics
│   ├── chain/               # Blockchain logic, actor, genesis, snapshots
│   ├── network/             # P2P infrastructure: Node, PeerManager, Protocol, SyncCodec
│   ├── rpc/                 # JSON-RPC server and API definitions
│   ├── storage/             # Sled database, migrations, snapshot export
│   ├── execution/           # State transitions and BudZKVM backend
│   ├── consensus/           # PoW, PoA, PoS, finality
│   ├── mempool/             # Transaction pool management
│   └── tests/               # Integration, chaos, security, and performance tests
```

## 2. Modularity Rules

-   **No dependency above core:** `core/` stays at the bottom and contains fundamental types.
-   **Separated consensus:** consensus engines act like plugins and can be injected into the chain.
-   **Message channels:** modules usually communicate through asynchronous channels instead of direct cross-calls.
-   **Network profiles:** Mainnet, Testnet, and Devnet parameters are centralized in `src/core/chain_config.rs`.
-   **Genesis isolation:** `src/chain/genesis.rs` builds separate genesis config for each network.
-   **Execution backend isolation:** `src/execution/executor.rs` is the L1 state transition entry point; BudZKVM details stay in `src/execution/zkvm.rs`.

## 3. Developer Experience

This structure lets a developer add a consensus algorithm by changing `consensus/` and the relevant core block metadata without touching unrelated modules.

To add a new network profile, update:

1.  `src/core/chain_config.rs`: chain ID, port, consensus, gas, mempool, and security values.
2.  `src/chain/genesis.rs`: genesis allocation and validator set.
3.  `config/<network>.toml`: operator config.
4.  `docs/en/book/ch07_network_distinctions.md`: network documentation.

