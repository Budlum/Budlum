# Chapter 4.2: Peer Management, Reputation, and Network Protection

Peer management protects the node from spam, invalid data, and hostile behavior while keeping honest peers connected.

## 1. Data Structures: Scorecard and Rate Limits

### Struct: `PeerScore`

`PeerScore` tracks a peer's behavior: invalid blocks, invalid transactions, rate-limit hits, bans, and useful contributions.

Scores turn vague trust into explicit policy. A peer that repeatedly sends bad data loses reputation and can be banned.

### Why These Values?

The values are tuned to punish harmful behavior quickly while avoiding accidental bans for a single network glitch.

## 2. Functions and Math

### DHT Bootstrapping and Discovery

Kademlia DHT lets nodes discover peers without a central registry. Bootnodes provide an initial entrance into the network.

### Active Reputation Filtering

Before accepting or forwarding data, the node checks whether the sender is known, banned, or rate-limited.

### Function: `check_rate_limit`

Rate limiting prevents spam. A peer has a limited budget for messages in a time window. If it exceeds that budget, messages can be dropped or the peer can be penalized.

### Granular Rate Limits

Votes, blobs, ordinary gossip, and sync messages may have different limits. Even if a peer exhausts a general message budget, consensus-critical voting can remain isolated when appropriate.

### Function: `report_invalid_block`

Invalid blocks are severe. Reporting one lowers the peer score and can trigger a ban if the behavior repeats.

## 3. Ban Cleanup

Bans are not always permanent. The manager periodically removes expired bans so temporary problems do not exclude a peer forever.

### Function: `ban_peer`

`ban_peer` records the ban reason and expiry time, disconnects the peer, and prevents immediate reconnection.

## 4. Integration

The peer manager is consulted by the node, sync layer, gossip handlers, and validation paths. The result is a network that can remain open while still defending itself.

