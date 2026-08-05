# Repository guide

This structure follows the system boundaries. Directories are intentionally empty until their owning contracts are accepted.

| Path | Owns |
| --- | --- |
| `apps/desktop` | Tauri desktop client |
| `apps/web` | Browser client |
| `apps/cli` | Operator and developer command-line tools |
| `core/domain` | Pure shared domain types and invariants |
| `core/instrument` through `core/alerts` | Trading-core modules, each owning one bounded capability |
| `adapters/brokers/ibkr` | Interactive Brokers translation layer |
| `adapters/market-data` / `notifications` | External provider integrations |
| `python/strategy-sdk` | Supported strategy interface |
| `python/research` / `examples` | Research helpers and non-production examples |
| `contracts/protobuf` / `json-schema` | Versioned inter-module contracts |
| `infra` | Terraform, container definitions, monitoring configuration |
| `tests` | Simulation, replay, integration, and fault-injection suites |
| `docs` | Product, architecture, operations, security, compliance, and user documentation |

## Dependency direction

`apps` and `adapters` depend on stable `contracts` and `core` interfaces. `core/domain` depends on no delivery framework, broker SDK, database driver, or UI package. Domain modules may not depend on an adapter.

## Initial repository rule

Create the Rust workspace, Python package, and TypeScript applications only after the event, instrument, intent, and risk-decision contracts have been reviewed. Every new executable module needs a corresponding test location and documentation owner.
