# Chapter 5.1: Persistent Storage (Sled DB)

Sled stores Budlum data with prefixed keys:

- `HEIGHT:{number}` maps height to block hash.
- `TX_IDX:{hash}` maps transaction hash to block height.
- `ACCT:{pubkey}` stores account state.
- `MEMPOOL:{hash}` persists pending transactions.
- `QC_BLOB:{height}` stores verified PQ sidecars.
- `FINALITY_CERT:{height}` stores finalized checkpoint certificates.

After a valid `QcFaultProof`, finality and QC records from the affected checkpoint can be deleted so disk state stays consistent with in-memory finality state.

