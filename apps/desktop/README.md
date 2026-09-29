# Follon trading terminal

This client provides twelve integrated operator workspaces, including a
PAPER-only order-intent ticket that remains disabled until a native Risk/OMS
route is configured. A bounded
`/api/v1/workspaces` projection combines dataset
structure, experiments, backtests, manifests, causal events, OMS lifecycle,
PAPER and controlled-live monitoring, operations risk/attribution, options,
journals, and commercial/deployment status.
The operations view exposes risk limits, attribution, alerts, schedules, replay and
configuration identities, the journal-bound projection fingerprint, and the
verified journal cursor alongside PAPER
kill switches, `UNKNOWN` orders, reconciliation incidents, positions, and
promotion evidence. The desktop exposes PAPER order submission, cancellation,
and position close, plus a combination ticket that submits one atomic
multi-leg PAPER combination as a single Risk/OMS order. Controlled-LIVE actions
remain outside the checked-in workstation flow.

React owns the application shell, Vite emits the deployable web bundle, and the
Tauri v2 host provides a separate native desktop command boundary. Its
PAPER-only command path validates a declarative request before passing it to a
configured Risk/OMS route; the web bundle does not receive broker credentials
or adapter access.
The checked-in host returns an explicit route-unavailable response until that
gateway is configured, rather than reporting a fictitious trade.

The `submit_combo_order` command accepts 2–16 distinct legs, a whole number of
units, and a maximum-debit or minimum-credit net protection. Each leg carries
its own operator-attested observed price and time, because no market-data feed
is wired into the host; a leg without one cannot be expressed. The gateway
routes the combination through `PaperTradingService::submit_combo_intent`, and
`cancel_order` reports a combination's real OMS state. Net short exposure — and
so almost any spread with a short leg — is refused unless the operator-authored
`FOLLON_DESKTOP_PAPER_CONFIG` file states an explicit bound:

```json
"short_exposure": { "max_short_quantity": "10" }
```

The UI never grants that permission itself.

The same file must list the venue tick size and lot size of every instrument
the operator may trade. Both tables must list the same instruments: a file
that lists an instrument in only one of them is refused when the gateway
starts. Each of these is refused before anything reaches the broker:

- an order for an instrument missing from both tables
  (`INSTRUMENT_TICK_SIZE_UNCONFIGURED`, `INSTRUMENT_LOT_SIZE_UNCONFIGURED`);
- a limit price off its instrument's grid (`LIMIT_PRICE_OFF_TICK_GRID`);
- a quantity that is not a whole number of lots (`ORDER_QUANTITY_OFF_LOT_SIZE`).

```json
"instrument_tick_sizes": { "inst.us_equity.aapl": "0.01" },
"instrument_lot_sizes": { "inst.us_equity.aapl": "1" }
```

A combination leg meets the same rules on its own instrument, with its own
contract quantity. The combination's net price limit must also sit on the
finest tick among its legs (`COMBO_NET_PRICE_OFF_TICK_GRID`). A venue's own
complex-order increment is not modelled.

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
Center, Research Lab, Strategies, Marketplace, Backtest, News, Execution Blotter,
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

## Source layout

`src/` is organised by feature; each folder has an `index.ts` that re-exports its
public API, and importers go through it.

| Path | Holds |
| --- | --- |
| `react-main.tsx`, `main.ts`, `app-shell.tsx` | Entry point, gateway wiring and the React shell |
| `routes.ts`, `catalog.ts`, `command-palette.ts` | Typed workspace routes, the feature catalogue and the palette |
| `evidence/` | Evidence types, parsers and type guards by domain (`core`, `operations`, `research`, `planning`, `durability`) and the DOM `render` module |
| `workspaces/` | The workspace snapshot parser, the dispatcher (`render.ts`), one module per workspace, and the shared `panels`, `visualizers`, `format` and `advanced-evidence` helpers |
| `orders/` | The PAPER order ticket, the combination ticket and the combination-intent model |

The evidence contract tests read these folders as one logical module each, so
adding a file to a feature folder needs no test change.

## Validate the projection

```powershell
npm install
npm run test:evidence
npm run typecheck
npm run build:web
cargo check --manifest-path src-tauri/Cargo.toml
npm run build:desktop
```

## Research navigation and local marketplace

The primary navigation groups monitoring, research, trading operations, and
administration. Strategies, Marketplace, Backtest, and News are dedicated
workspaces. Existing strategy-studio, backtest-explorer, and news-cockpit route
IDs remain compatible; Marketplace is available at `/#workspace/marketplace`.
Navigation updates the document title and keyboard focus. Evidence section
anchors preserve the selected workspace. Tables provide local text filtering
and 20-record pages; filtering retains the original artifact/action mapping.
These controls operate on the bounded server projection, not an unlimited
historical database query.

Marketplace lists only indexed market-data, research, and replay artifacts.
Search, category selection, incremental listing, and artifact inspection work
locally. Publishing, purchases, ratings, verified publisher identity, bundle
installation, and executable strategy deployment require additional services.
No synthetic listings, return claims, or approval badges are generated.

Start the local evidence server in one terminal from the repository root:

```powershell
python apps/desktop/server.py
```

In another terminal, from `apps/desktop`, run `npm run dev`. Vite proxies
`/api/v1` to `http://127.0.0.1:8080`; open `http://127.0.0.1:1420`.
The production bundle remains available through the Python server after
`npm run build:web`. Font stacks use installed fonts and system fallbacks;
there is no external font request.

See [the end-to-end product plan](../../docs/06-delivery/15-end-to-end-product-plan.md)
for integration gaps, acceptance criteria, and the proposed implementation order.
