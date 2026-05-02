# Budlum Blockchain Book

Welcome!

This book is the living technical documentation for the **Budlum Blockchain** project. It is not a plain API reference. It is intended as a window into the mind of a blockchain architect.

Our goal is to answer not only "What does this code do?", but also **"Why was it designed this way?", "Which problem does it solve?"**, and **"What alternatives were considered?"**

The codebase is written in **Rust** and uses modern technologies:
-   **Cryptography:** Ed25519 and BLS for finality, Merkle Patricia Trie for account state
-   **Networking:** Libp2p with Request-Response Sync, Gossipsub, and Kademlia DHT
-   **Database:** Sled, an embedded key-value store
-   **Consensus:** Pluggable PoW, PoS, and automatic finality voting
-   **Observability:** Prometheus metrics, compatible with Grafana
-   **Administration:** A flexible TOML-based configuration system

---

## How to Read This Book

The book follows a "From the Architect's Eye" approach and is organized into major sections. Reading it in order is recommended.

### 1. [Chapter 1: Fundamentals and Data Structures](ch01_basics.md)
The atomic pieces of a blockchain.
-   [Blocks](ch01_01_blocks.md): Why do **Merkle Trees** exist? How does **SPV** (Light Client) work?
-   [Transactions](ch01_02_transactions.md): What is a **Replay Attack**? How does a **Nonce** prevent it?
-   [Account State](ch01_03_account_state.md): **State Machine** logic and the **UTXO vs Account** model comparison.

### 2. [Chapter 2: Cryptography](ch02_crypto.md)
The mathematical foundation of security.
-   [Signatures](ch02_01_signatures.md): Why **Ed25519**? What is a deterministic signature?
-   [Hash Trees](ch02_02_merkle_trees.md): How can data integrity be proven with **O(log N)** cost?

### 3. [Chapter 3: Consensus](ch03_consensus.md)
Decentralized decision-making mechanisms.
-   [Engine Interface](ch03_01_intro.md): **Modular Architecture** and the `ConsensusEngine` trait.
-   [Proof of Work](ch03_02_pow.md): Satoshi's vision and an analysis of the **Difficulty Adjustment Algorithm**.
-   [Proof of Stake](ch03_03_pos.md): The modern approach: the **Nothing at Stake** problem, **Slashing**, and **Leader Selection**.

### 4. [Chapter 4: Networking and P2P](ch04_networking.md)
The shared language of computers.
-   [Node Architecture](ch04_01_node.md): The **Tokio Event Loop** and asynchronous programming.
-   [Peer Manager](ch04_02_peer_manager.md): Reputation management through **Game Theory** and protection against **Sybil Attacks**.
-   [Protocol](ch04_03_protocol.md): **Protobuf** network messages, deterministic internal serialization, and network limits.

### 5. [Chapter 5: Storage and Efficiency](ch05_storage.md)
Data persistence.
-   [Database](ch05_01_storage.md): Durable storage based on **Sled**, migrations, and snapshot export.
-   [Mempool](ch05_02_mempool.md): The **Fee Market**.
-   [Snapshot](ch05_03_snapshots.md): **Pruning**.

### 6. [Chapter 6: JSON-RPC API](ch06_json_rpc.md)
Integration with the outside world. The **Budlum Standard** (`bud_`) methods and usage guide.

### 7. [Chapter 7: Network Separation](ch07_network_distinctions.md)
**Mainnet, Testnet, and Devnet** configurations. CLI-based network selection and isolation.

### 8. [Chapter 8: Chaos Engineering](ch08_chaos_engineering.md)
**Chaos Tests** for network resilience, re-org protection, and failure simulation.

### 9. [Chapter 9: Directory Structure](ch09_directory_structure.md)
The new **Layered Modular Architecture** and file layout.

### 11. [Chapter 11: Multi-Consensus Settlement & Byzantine Resilience](ch11_multi_consensus_settlement.md)
**Model B: Buffered Registry** architecture, network partition recovery, and deterministic global consensus proofs under Byzantine conditions.

---

## Contributing

Budlum is an open-source project. You can find the code under `infra/src` and connect the theory in this book with the implementation. If you find an issue or have a clearer explanation, please contribute.

Enjoy the read,
*The Budlum Core Team*

