# Chapter 2.1: Cryptographic Identity and Signatures

Cryptographic signatures are how Budlum proves identity without passwords or central accounts. A public key identifies the actor; the private key authorizes actions.

## 1. Data Structures: Our Identity Card

### Struct: `KeyPair`

`KeyPair` holds the private signing key and the public verification key. The private key must remain secret. The public key can be shared and is used as an address-like identity.

## 2. Algorithms: Signing and Verification

### Function: `new`

`new` creates a fresh wallet by drawing secure randomness from the operating system. Entropy matters: predictable keys are not keys at all.

### Function: `sign`

Signing takes a message hash and produces an Ed25519 signature. Ed25519 is deterministic, fast, and widely trusted for modern systems.

### Function: `verify_signature`

Verification checks that a signature was produced by the private key corresponding to a given public key. Nodes use this for transactions, block production, and validator messages.

### Function: `public_key_hex`

Budlum exposes public keys as hex strings because they are easy to serialize, log, store, and pass through JSON-RPC.

## 3. Hardening Phase 2: Additional Cryptographic Schemes

### BLS Signatures

BLS signatures support aggregation. Thousands of validator signatures can be combined into one compact certificate, which is ideal for finality.

### Dilithium

Dilithium provides post-quantum signature capability. It is heavier than Ed25519, so Budlum uses it carefully for future-facing attestation paths.

### Proof of Possession

Proof of Possession prevents rogue-key attacks in aggregated signature systems. Validators must prove that they own the private key behind the public key they register.

## Summary

1.  **Hybrid security:** Ed25519 for speed, BLS for aggregation, Dilithium for post-quantum readiness.
2.  **Scalability:** BLS aggregation keeps finality messages compact.
3.  **Future readiness:** the architecture can handle post-quantum scenarios before they become urgent.

