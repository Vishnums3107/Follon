# System architecture

## Decision

Start as a modular monolith in Rust, with isolated Python strategy workers. Do not begin with microservices, Kafka, or Kubernetes.

```text
React + TypeScript client (web / Tauri desktop)
                 │ HTTPS / WebSocket
Rust trading control plane
 identity · config · instruments · market data · replay · backtest
 OMS · execution · risk · portfolio · reconciliation · audit · alerts
          │ gRPC / Protobuf             │ adapter API
Python strategy workers              OMS-owned PAPER adapter registry
                                      -> IBKR, then reviewed future adapters
                 │
PostgreSQL · Parquet/DuckDB · S3-compatible object storage · append-only event log
```

## Boundary rules

- The Rust core owns trading state, risk, broker adapters, normalization, portfolio accounting, and replay orchestration.
- Python workers may receive events, query approved data/services, emit metrics, persist strategy state, and submit order intents only.
- The frontend is a projection of server-owned state; it does not make unaudited trading-state transitions.
- Adapters translate external protocols into canonical contracts and must not leak broker-specific types into the domain layer.
- The PAPER adapter registry is configured only at the OMS composition boundary.
  It routes one canonical account to one reviewed adapter instance, fails closed
  on missing or duplicate routes, and exposes neither credentials nor adapter
  calls to clients or strategy workers.

## Evolution trigger

Split a module into a service only when measured operational, scaling, or deployment requirements demonstrate that the modular monolith cannot satisfy them. Retain contracts and event envelopes so a later split is incremental.
