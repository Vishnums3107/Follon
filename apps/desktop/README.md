# Follon evidence dashboard

This client provides ten integrated operator workspaces over server-owned,
immutable evidence. A bounded `/api/v1/workspaces` projection combines dataset
structure, experiments, backtests, manifests, causal events, OMS lifecycle,
PAPER and controlled-live monitoring, operations risk/attribution, options,
journals, and commercial/deployment status without exposing a trading action.
The operations view exposes risk limits, attribution, alerts, schedules, replay and
configuration identities, the journal-bound projection fingerprint, and the
verified journal cursor alongside PAPER
kill switches, `UNKNOWN` orders, reconciliation incidents, positions, and
promotion evidence. It deliberately contains no action that can create,
approve, cancel, schedule, configure, journal, or transmit an order.

React owns the application shell, Vite emits the deployable web bundle, and the
Tauri v2 host provides the native desktop boundary without privileged custom
commands. The existing fail-closed TypeScript evidence controller is loaded
only after React mounts the complete workspace DOM.

The packaged client reads its versioned API from the loopback evidence service
at `http://127.0.0.1:8080`. The server grants read-only cross-origin access only
to the exact Tauri asset origins; it must be running before the native client.

## Open the local dashboard

The development Docker profile packages this projection as a loopback-only web
dashboard. From the repository root:

```powershell
docker compose --env-file infra/.env -f infra/compose.dev.yml up -d --build
```

Open `http://127.0.0.1:8080`. The dashboard reports dependency health, maps all
implemented product areas to their current acceptance gates, automatically
indexes supported `.ndjson`, `.json`, `.md`, and `.csv` artifacts recursively
under `var/`, and
lets the operator filter, inspect, or download immutable evidence. It has no
broker, credentials, or state-changing controls.

The documented screen catalogue is implemented as tailored workspaces: Command
Center, Research Lab, Strategy Studio, Backtest Explorer, Execution Blotter,
Risk Cockpit, Portfolio, Replay and Incidents, Journal, and Administration.
Each workspace renders domain-specific metrics and tables, links back to exact
source artifacts, and keeps documented external acceptance gates visible.

Development is loopback-only without authentication. The compatibility server
supports fail-closed HTTP Basic authentication for an operator deployment and
requires a password of at least 16 characters. Production Compose places it
behind client-certificate TLS. The customer IAM/RBAC/MFA kernel and durable
schema are implemented separately; no customer identity or privileged trading
mutation is exposed through this read-only server.

The small dashboard API intentionally exposes only safe evidence filenames,
rejects traversal and symlink escapes, sends a restrictive content-security
policy, caps individual rendered artifacts at 10 MiB, and bounds records in the
unified workspace projection. Browser module imports use explicit `.js` URLs so
the unbundled ESM graph works on the static server.

## Validate the projection

```powershell
npm install
npm run test:evidence
npm run typecheck
npm run build:web
cargo check --manifest-path src-tauri/Cargo.toml
npm run build:desktop
```
