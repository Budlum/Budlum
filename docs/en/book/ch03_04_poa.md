# Proof of Authority (PoA)

Proof of Authority is designed for private or permissioned networks where a known set of authorities is allowed to produce blocks.

## How It Works

1.  **Authority list:** in Budlum PoA mode, authorized addresses are read from `validators.json` at node startup and loaded into the validator set.
2.  **Round-robin production:** authorities produce blocks in order.
3.  **Signature validation:** blocks are accepted only if signed by the authority expected for that slot.

## Advantages

-   Very fast block production.
-   Low operational cost.
-   Simple governance for consortium and test environments.

## Disadvantages

-   It is not fully permissionless.
-   The authority set must be managed carefully.
-   Trust is concentrated in known operators.

## Implementation Notes

PoA is useful for devnets, enterprise deployments, and controlled environments where predictability matters more than open validator participation.

