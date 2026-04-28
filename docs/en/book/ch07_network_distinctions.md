# Chapter 7: Network Distinctions

Budlum separates Mainnet, Testnet, and Devnet so data, peers, chain IDs, ports, genesis files, and operational expectations do not mix.

## 1. Network Types

-   **Mainnet:** production network with real economic value and strict security expectations.
-   **Testnet:** public testing network with separate chain ID and test funds.
-   **Devnet:** local or development network optimized for fast iteration.

## 2. Selecting a Network with the CLI

Examples:

```bash
# Start in devnet mode, the default
budlum node --network devnet

# Start in testnet mode
budlum node --network testnet

# Start with a TOML config file
budlum node --config config/testnet.toml
```

## 3. Configuration Logic

`src/core/chain_config.rs` centralizes chain ID, ports, consensus settings, gas values, mempool limits, and security values.

## 4. Config Files

`config/devnet.toml`, `config/testnet.toml`, and `config/mainnet.toml` carry operator-facing values such as bind addresses, bootnodes, database paths, and RPC settings.

## 5. Security and Isolation

Network isolation prevents replay attacks, accidental peer mixing, and corrupted local databases. Operators should keep data directories and keys separate per network.

