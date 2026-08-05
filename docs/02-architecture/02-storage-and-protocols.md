# Storage and protocols

| Need | Initial technology | Responsibility |
| --- | --- | --- |
| Transactional state | PostgreSQL | Users, accounts, config, orders, executions, positions, risk limits, audit indexes, and billing metadata |
| Analytical datasets | Parquet + DuckDB | Historical bars/ticks, backtest datasets, feature matrices, local analysis, portable exports |
| Durable artifacts | S3-compatible object storage | Raw data, reports, logs, strategy bundles, backups, release artifacts |
| Contract definitions | Protobuf and JSON Schema | Internal worker RPC and externally validated JSON surfaces |
| Worker RPC | gRPC/Protobuf | Control-plane-to-strategy-worker communication |
| Client state stream | WebSocket | Live state updates to web and desktop clients |
| Administrative interface | REST | Identity, configuration, and administration operations |

## Storage rules

- PostgreSQL is the source of truth for current transactional state; derived views can be rebuilt.
- Raw and normalized market data are retained separately.
- Large historical data and artifacts stay out of transactional tables.
- Every persisted trading record references immutable software and configuration versions where applicable.
- Decimal/fixed-point representations are mandatory for monetary values and quantities requiring exact accounting.

## Deferred infrastructure

Do not introduce Kafka, a custom database, or Kubernetes during foundation work. Managed equivalents and a single deployable control plane are preferred until requirements are measured.
