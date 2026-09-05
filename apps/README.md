# Follon Applications (`apps/`)

This directory contains the operator user interfaces and execution clients for the Follon trading operating system. Application code is strictly downstream of the deterministic core engine and versioned schema contracts.

## Architecture and Operating Boundaries

The application layer adheres to strict security, separation-of-privilege, and evidence-first invariants:

1. **Deterministic Downstream Consumers**: UIs and CLIs project immutable facts, receipts, and audit logs. They never calculate P&L, risk limits, or fills browser-side.
2. **Privileged Native Boundary**: Trading intents (order submission, order cancellation, position closes) traverse an explicit, least-privilege Tauri native host IPC command boundary to the configured Risk/OMS route. Web bundles never possess broker credentials or raw gateway sockets.
3. **Loopback Evidence Service**: The React application reads versioned projections and artifact indexes from the loopback service (`http://127.0.0.1:8080`). Missing information displays explicitly as `UNKNOWN` or `Unavailable`; green readiness is never inferred from the absence of alerts.
4. **Offline and Air-Gapped Operation**: Research, backtesting, and incident replay functions operate completely offline without cloud AI, telemetry, or external subscriptions.

---

## Subdirectories

### 1. [`apps/desktop`](desktop/) — Trading Terminal & Cockpit
A React 19 and Tauri v2 workstation terminal providing 12 specialized operational workspaces:
- **Monitor**:
  - `command-center`: Real-time session brief, market scanner, attention queue, and operational playbooks.
  - `execution-blotter`: Causal execution blotter, explainable pre-trade risk decisions, order tickets, transaction cost analysis (TCA), and execution coach benchmark (EXEC-03, RES-07).
  - `risk-cockpit`: Real-time operations exposure controls, versioned limits, cross-strategy factor exposure graph, scenario loss stress lab (RISK-02), and capital allocation plans (RISK-03).
  - `portfolio`: Reconciled multi-environment positions, net attribution, double-entry fund ledger, and FIFO tax lots.
- **Research**:
  - `research-lab`: Dataset inventory, inert notebook browser, frozen hypothesis notebook (RES-01), data quality console (DATA-01), and feed substitution parity audits (DATA-06).
  - `strategy-studio`: Strategy specification catalog, composition studio (RES-02), read-only research critic (AI-02), and budgeted research scheduler (AI-04).
  - `backtest-explorer`: Reproducible backtest results, parameter stability, walk-forward robustness lab (RES-05), portfolio joint experiments (RES-06), and failed idea memory.
  - `news-cockpit`: Immutable headline receipts, sentiment vectors, point-in-time knowledge graph (DATA-02), news revision timeline (DATA-03), event exposure calendar (DATA-04), and assumption regime monitor (DATA-05).
- **Operate**:
  - `marketplace`: Evidence-based asset comparisons without synthetic ratings, and sandboxed package installation preview (ASSET-03, ASSET-04).
  - `replay-incidents`: Deterministic event-by-event causal debugger and incident timeline reconstruction.
  - `journal`: Auditable decision journal, compliance records, and double-entry accounting.
- **Govern**:
  - `administration`: Commercial ledgers, deployment boundaries, operational watchdogs, and broker adapter qualifications (LIFE-07, PORT-02).

### 2. [`apps/cli`](cli/) — Operator Replay, Backtest & Bar Builder CLI
Command-line binaries compiled from Rust (`follon-cli`) providing scriptable and headless workflows:
- `follon-replay`: Deterministic replay of market bars into NDJSON event envelopes.
- `follon-backtest`: Reproducible backtest execution with complete input/output SHA-256 fingerprint manifests.
- `follon-build-bars`: High-performance deterministic trade importer and time-bar builder.

---

## Quickstart & Verification

### Running the Desktop Environment
```powershell
# In terminal 1 (repository root): Start evidence server
python apps/desktop/server.py

# In terminal 2 (apps/desktop): Start Vite dev server
cd apps/desktop
npm install
npm run dev
```

### Running Full Validation Suite
```powershell
# Validate desktop contracts, DOM harnesses, and production bundle
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop run test:evidence
npm --prefix apps/desktop run build:web

# Validate server contract
python apps/desktop/test/server_contract.py
```
