# Repository guide

This structure follows the system boundaries. Keep top-level directories owned by
an accepted contract or by executable/testable repository support. Planned
future areas should stay out of the tree until their owning implementation
lands.

| Path | Owns |
| --- | --- |
| `apps/desktop` | React/Vite evidence dashboard, Tauri v2 native host, and bounded local HTTP server |
| `apps/cli` | Operator and developer command-line tools |
| `core/domain` | Pure shared domain types and invariants |
| `core/execution`, `core/risk`, `core/accounting` | Broker-neutral EMS, portfolio-wide risk, and exact multi-currency/margin accounting |
| `core/identity` | Customer password rotation, TOTP/recovery MFA, session, tenant, revocation, and RBAC invariants |
| `core/instrument` through `core/secrets` | Other trading-core modules, each owning one bounded capability |
| `adapters/brokers/ibkr` | PAPER bridge plus signed/review-bound controlled-LIVE adapter edge |
| `adapters/persistence/postgres` | Checksum-bound PostgreSQL migrations, events/outbox/checkpoints, forced RLS, IAM/accounting/broker and complete product projection schema |
| `services/trading-api` | Versioned tonic gRPC topology for scheduled/passive/options-combination EMS, aggregate risk, margin, health, and PostgreSQL startup |
| `python/strategy-sdk` | Supported strategy interface |
| `python/storage-adapter` | Research storage publication and catalogue adapter |
| `python/ibkr-gateway` | PAPER-only Python bridge protocol helper |
| `python/examples` | Non-production strategy examples |
| `contracts/protobuf` / `json-schema` | Versioned inter-module contracts |
| `infra` | Terraform, container definitions, monitoring configuration |
| `tests/fixtures` | Shared deterministic fixtures |
| `tests/security` | Security and supply-chain contract tests |
| `tools` | Deterministic developer/release automation |
| `docs` | Product, architecture, operations, security, compliance, and user documentation |

Reserved plan areas such as `apps/web`, `adapters/market-data`,
`adapters/notifications`, and `python/research` should be created only when a
reviewed implementation needs them. The deployed `services/trading-api` is a
thin delivery boundary over the modular core; it does not move accounting,
risk, identity, or execution policy into the service framework.

## Dependency direction

`apps` and `adapters` depend on stable `contracts` and `core` interfaces. `core/domain` depends on no delivery framework, broker SDK, database driver, or UI package. Domain modules may not depend on an adapter.

## Initial repository rule

Create the Rust workspace, Python package, and TypeScript applications only after the event, instrument, intent, and risk-decision contracts have been reviewed. Every new executable module needs a corresponding test location and documentation owner.
