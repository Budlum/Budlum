# Chapter 4.3: Network Protocol and Messaging

Budlum network messages include blocks, transactions, sync requests, finality votes, finality certificates, QC blob requests/responses, and QC fault proofs.

QC messages:

- `GetQcBlob { epoch, checkpoint_height }`: asks peers for the PQ sidecar of a checkpoint.
- `QcBlobResponse { epoch, checkpoint_height, checkpoint_hash, blob_data, found }`: returns a serialized `QcBlob`.
- `QcFaultProof { proof_data }`: broadcasts a serialized `QcFaultProof`.

Recipients parse, validate, and persist QC blobs before they become authoritative. Fault proofs are verified against the stored blob and can invalidate finality metadata.

