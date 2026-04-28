# Chapter 5.2: Transaction Pool (Mempool) Mechanics

The mempool holds valid but unconfirmed transactions. It is both a waiting room and a market where transactions compete for block space.

## 1. Data Structures: Multiple Orderings

### Struct: `Mempool`

The mempool keeps transactions by hash, by sender and nonce, and by fee ordering. These views support lookup, replacement, and block selection.

### Why Three Structures?

One structure cannot efficiently answer every question. Hash maps are good for direct lookup, nonce queues are good for sender ordering, and fee ordering is good for miner selection.

## 2. Algorithms: Selection and Cleanup

### Function: `add_transaction`

`add_transaction` validates incoming transactions, rejects duplicates, applies RBF rules when needed, stores accepted transactions, and persists them to disk.

### `get_sorted_transactions` and Block Selection

Block selection walks transactions from highest fee downward, but it must respect nonce order:

1.  Transactions are considered by fee.
2.  A temporary state is created.
3.  A transaction enters the block only if it is valid against the temporary state.
4.  `nonce=2` cannot enter before `nonce=1` for the same sender.
5.  Skipped transactions can be retried after earlier nonces are included.

## 3. RBF (Replace By Fee)

Replacement asks:

1.  Does the sender already have a transaction with the same nonce?
2.  Is the new fee at least 10% higher?
3.  If yes, remove the old transaction and add the new one.

## 4. Garbage Collection

### Function: `cleanup_expired`

Expired or invalid transactions are removed from memory and disk. This prevents unbounded growth.

## 5. Mempool Persistence and Resilience

1.  **Save on arrival:** accepted transactions are written to `MEMPOOL:{hash}`.
2.  **Remove on mined or evicted:** included, replaced, or expired transactions are deleted.
3.  **Startup recovery:** `Blockchain::new()` scans `MEMPOOL:` keys and rebuilds the pool.
4.  **Unwrap audit:** invalid data logs errors instead of panicking the node.

## 6. Nonce Queue Semantics

Nonce queues protect sender ordering and make it possible to hold future transactions until earlier nonces arrive.

## 7. Transaction Gossip

1.  The user submits a transaction with `bud_sendRawTransaction`.
2.  The node adds it to the local mempool.
3.  If valid, it broadcasts on the `transactions` Gossipsub topic.
4.  Other nodes validate and forward it to their peers.

