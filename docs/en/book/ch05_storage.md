# Chapter 5: Storage and Efficiency

A blockchain is not only about networking and consensus. It also depends on how data is stored and managed. In this chapter, we will examine Budlum's data layer.

The data layer has three main components:

1.  **Persistent Storage:** Where blocks and state are written to disk, using Sled DB.
2.  **Temporary Memory (Mempool):** The in-memory pool where transactions wait before confirmation.
3.  **Data Pruning:** Removing old data to save disk space while keeping compact summaries through snapshots.

The way these components work together directly affects node performance and disk usage.

