# Chapter 2.1: Cryptographic Identity and Signatures

Validator keys can now contain an Ed25519 signing key, a VRF key, and an optional Dilithium key.

Dilithium is used for Optimistic QC sidecars. Active validators that participate in PQ-gated finality must expose `pq_public_key`, and QC blobs are verified against the validator snapshot before they are persisted or used for finality.

Older key files without PQ material can still load, but they are not sufficient for PQ-required validation paths.

