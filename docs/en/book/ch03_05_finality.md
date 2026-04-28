# Chapter 3.5: Finality Layer (BLS)

Budlum finality combines BLS aggregate certificates with verified PQ-QC sidecars.

`FinalityCert` verification now requires:

1. A valid checkpoint height.
2. A matching local checkpoint block hash.
3. A historical validator snapshot for the certificate epoch.
4. A valid BLS aggregate signature and signer bitmap.
5. A verified `QC_BLOB:{height}` for the same checkpoint.
6. Dilithium attestations covering every BLS signer.

If a `FinalityCert` arrives before the corresponding QC blob, the node stores it as pending and requests `GetQcBlob`. When the blob is imported successfully, pending certificates for that checkpoint are retried automatically.

`QcFaultProof` can invalidate finality metadata from the affected checkpoint. Current invalid-Dilithium proofs do not slash validators; slashable verdicts are reserved for stronger signed or ZK-backed evidence.

