# Chapter 4.1: Node Architecture and Event Loop

A Budlum node is the bridge between the blockchain state machine and the peer-to-peer network. It receives blocks and transactions, validates them, gossips useful data, and asks peers for missing history.

For PQ-QC, the node keeps finality robust against message ordering. If a `FinalityCert` arrives before its `QC_BLOB`, the chain queues the certificate, the node requests `GetQcBlob`, and blob import retries pending finality automatically.

## 1. Data Structures: The Connection Point

### Struct: `BudlumBehaviour`

`BudlumBehaviour` combines libp2p protocols such as Gossipsub, Kademlia discovery, request-response sync, and peer management into one network behavior.

### Struct: `Node`

`Node` owns the swarm, channels, local chain handle, peer manager, and runtime configuration. It is responsible for turning network events into chain actions.

### Struct: `NodeClient`

`NodeClient` is the external handle used by other parts of the system to send commands into the node without directly controlling the network loop.

### Design Decision: Actor Model

Budlum separates chain execution from network IO through `ChainActor` and `ChainHandle`. This keeps validation, storage, and networking from locking each other unnecessarily.

## 2. The Event Loop

The node's event loop waits on multiple sources at once:

-   libp2p swarm events,
-   commands from local clients,
-   block or transaction announcements,
-   maintenance timers.

`tokio::select!` is the core tool here. It lets the node react to whichever event becomes ready first without blocking the rest of the system.

The loop also drains locally detected slashing evidence and gossips it periodically. This keeps validator penalties from depending on the detecting node being the next block producer.

## 3. Fork Choice and Reorg

When a block arrives, Budlum classifies it:

1.  **Sequential block:** the block extends the current tip and can be validated directly.
2.  **Fork:** the block is behind the current height but has a different hash, indicating a network split or competing branch.
3.  **Sync gap:** the block is far ahead, so the node requests missing headers or blocks.
4.  **Handshake height gap:** a peer reports a higher `best_height` during handshake, so the node starts headers-first sync immediately instead of waiting for later block traffic.

Finality protection prevents the node from reorganizing behind finalized checkpoints.

`bud_syncing` reports the node's real `Syncing`, `Synced`, or stalled sync status rather than a hardcoded value.

### Function: `handle_network_event`

This function translates raw libp2p events into domain actions: peer connected, peer disconnected, message received, sync request arrived, or gossip data received.

Peer counts and network limits are tracked here so the node can maintain healthy connectivity without accepting unlimited load.

## 4. Maintenance and Operations

Operational hardening includes periodic peer cleanup, sync health checks, request timeouts, and defensive handling of malformed messages. The node should degrade safely rather than panic on bad network input.
