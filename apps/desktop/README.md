# Follon trading terminal

This client provides ten integrated operator workspaces, including active
order-entry controls for the PAPER and LIVE execution environments. A bounded
`/api/v1/workspaces` projection combines dataset
structure, experiments, backtests, manifests, causal events, OMS lifecycle,
PAPER and controlled-live monitoring, operations risk/attribution, options,
journals, and commercial/deployment status.
The operations view exposes risk limits, attribution, alerts, schedules, replay and
configuration identities, the journal-bound projection fingerprint, and the
verified journal cursor alongside PAPER
kill switches, `UNKNOWN` orders, reconciliation incidents, positions, and
promotion evidence. The desktop supplies order submit, cancel, and position
close requests through its native command boundary.

React owns the application shell, Vite emits the deployable web bundle, and the
Tauri v2 host provides the native desktop boundary with privileged custom
commands. Those commands validate a declarative request before passing it to
the configured Risk/OMS route; the web bundle does not receive broker
credentials or adapter access.
The checked-in host returns an explicit route-unavailable response until that
gateway is configured, rather than reporting a fictitious trade.

The packaged client reads its versioned evidence API from the loopback service
at `http://127.0.0.1:8080`. The service grants cross-origin access only to the
exact Tauri asset origins; it must be running before the native client.

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
lets the operator filter, inspect, or download immutable evidence. The
evidence service has no broker credentials or order-entry endpoint; order
controls use Tauri IPC instead.

The documented screen catalogue is implemented as tailored workspaces: Command
Center, Research Lab, Strategy Studio, Backtest Explorer, Execution Blotter,
Risk Cockpit, Portfolio, Replay and Incidents, Journal, and Administration.
Each workspace renders domain-specific metrics and tables, links back to exact
source artifacts, and keeps documented external acceptance gates visible.

Development is loopback-only without authentication. The compatibility server
supports HTTP Basic authentication for an operator deployment and requires a
password of at least 16 characters. Production Compose places it behind
client-certificate TLS. The customer IAM/RBAC/MFA kernel and durable schema are
implemented separately. The evidence service remains separate from the
privileged native trading-command boundary.

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
