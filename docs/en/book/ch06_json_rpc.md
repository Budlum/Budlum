# Chapter 6: JSON-RPC 2.0 API

The JSON-RPC API is Budlum's integration layer for wallets, dashboards, scripts, and external services.

## 1. Running

The RPC server is configured through TOML and exposes Budlum-specific methods with the `bud_` prefix.

### Configuration File

Typical configuration includes bind address, port, connection limits, request size limits, optional auth settings, CORS origins, and rate limits.

## 2. Observability: Prometheus Metrics

Budlum exposes operational metrics for Prometheus and Grafana. Metrics cover block height, peer counts, mempool size, RPC activity, and validation behavior.

## 3. Supported Methods (`bud_` Prefix)

Common methods include:

-   `bud_blockNumber`
-   `bud_getBalance`
-   `bud_sendRawTransaction`
-   `bud_getTransaction`
-   `bud_getBlockByNumber`
-   `bud_txPrecheck`

## 4. Example Usage

### Query Block Count

Use JSON-RPC over HTTP to call `bud_blockNumber`.

### Query Balance

Call `bud_getBalance` with an account public key or address.

### BudZKVM ContractCall Precheck

`bud_txPrecheck` validates transaction shape and BudZKVM bytecode alignment before the user pays to propagate or include the transaction.

## 5. Architecture and Security

1.  **Max connections:** limits active clients and prevents resource exhaustion.
2.  **Max request size:** rejects oversized JSON payloads.
3.  **Transaction validation:** `bud_sendRawTransaction` checks size and cryptographic signature before gossip.
4.  **Panic prevention:** critical server paths use `Result` rather than crashing on malformed input.
5.  **Config-based auth readiness:** TOML fields standardize `auth_required`, `api_key_env`, `allowed_ips`, `cors_origins`, and `rate_limit_per_minute`.
6.  **ContractCall shape checks:** precheck and mempool validation reject empty or misaligned BudZKVM bytecode.

## 6. How Realistic Is `bud_txPrecheck`?

`bud_txPrecheck` is a fast early warning system. It does not replace full block execution, but it helps wallets and operators catch malformed transactions before broadcasting them.

