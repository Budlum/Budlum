# Chapter 2.2: Merkle Trees and Data Integrity

Merkle trees let Budlum prove membership without sending full data sets.

Budlum uses them for transaction roots, state roots, and QC sidecars. In Optimistic QC, Dilithium signatures live in `QcBlob` objects and the Merkle root commits to all PQ attestations.

If a bad Dilithium signature is found, `QcBlob::detect_fault_proofs` can build a `QcFaultProof` containing the leaf and Merkle path.

