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

The eventual Tauri host will provide an authenticated WebSocket endpoint at
`/api/v1/evidence`; this TypeScript shell is kept framework-light until the
control-plane stream contract is exercised end to end.

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

Development is loopback-only without authentication. Production mode supports
fail-closed HTTP Basic authentication for an operator deployment and requires
a password of at least 16 characters; it must be placed behind TLS. Customer
identity, entitlement enforcement, and role-based authorization remain external
gates and are not implied by this local evidence surface.

The small dashboard API intentionally exposes only safe evidence filenames,
rejects traversal and symlink escapes, sends a restrictive content-security
policy, caps individual rendered artifacts at 10 MiB, and bounds records in the
unified workspace projection. Browser module imports use explicit `.js` URLs so
the unbundled ESM graph works on the static server.

## Validate the projection

```powershell
npm install
npm run test:evidence
```
