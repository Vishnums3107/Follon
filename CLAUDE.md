# Follon — working agreement for any agent session

Follon is a risk-first, multi-asset trading operating system written as a Rust
modular monolith, with a Python strategy SDK and a React/Tauri desktop. Its
defining requirement is **research-to-live parity**: a strategy must behave
equivalently in research, deterministic replay, simulation, PAPER, and
controlled LIVE.

## Read this first, every session

**[`docs/06-delivery/16-delivery-state.md`](docs/06-delivery/16-delivery-state.md)**
— the resume-here document. It holds the machine-measured suite status, the
remaining backlog, the external gates, and the session log. Read it before
planning anything.

Then:

```bash
git status --short                # what the last session left uncommitted
python tools/session_status.py    # re-measure every suite; rewrites the status block
```

## Non-negotiable rules

Each of these exists because violating it produced a real defect in this
repository. `docs/06-delivery/14-master-plan-conformance-audit.md` records which.

1. **No synthetic data presented as evidence.** Fixtures live in
   `tests/fixtures/`. `var/` holds only what a real computation produced. A
   dashboard panel with no real backing renders its empty state — it is never
   filled with plausible-looking numbers.
2. **Capture exit codes directly, never through a pipe.**
   `cmd > log 2>&1; echo $?`, then read the log. `cmd | grep ...` returns
   grep's status and has already masked a broken build here.
3. **Never record a status you did not measure.**
4. **Land slices whole.** Cut a multi-session epic into units that each
   compile, test, and stand alone. Do not half-land an epic.
5. **Every behavioural change carries a test that fails without it.** For a new
   property test, first verify it catches a deliberately injected defect.
6. **Money is fixed-point `Decimal`.** Never `f64` on a value path.
7. **Credential and authority boundaries.** Strategy code never sees broker
   credentials. The desktop never contacts a broker; it submits a validated
   request to the Risk/OMS route, the sole submission authority.
8. **The audit is append-only.** Add a numbered item to "Locally closed gaps"
   for work that closes a gap; correct an earlier claim in place only when it
   was false, and say so explicitly.

## Layout

| Path | What it is |
| --- | --- |
| `core/domain` | Fixed-point types, canonical event envelope, `OrderIntent` |
| `core/execution` | Broker-neutral EMS algorithms and the option-combo planner |
| `core/risk` | Portfolio risk kernel and aggregate composition |
| `core/accounting` | Multi-currency, margin, FIFO/LIFO tax lots (long and short) |
| `core/control-plane` | OMS order-lifecycle state machine, capsule, provenance |
| `core/paper`, `core/live` | Environment services: risk gate, OMS, journal, reconciliation |
| `core/backtest` | Deterministic replay runner and `AdvancedBacktestAccount` |
| `adapters/brokers/ibkr` | Real IBKR transport, hard-locked to paper ports and loopback |
| `adapters/persistence/postgres` | Transactional, tenant-isolated store |
| `services/trading-api` | Versioned gRPC boundary |
| `apps/desktop` | React/Vite web bundle plus the Tauri v2 privileged host |
| `python/` | Strategy SDK and examples (no adapter or credential access) |
| `tools/` | Evidence pipeline, SBOM, release gates, session status |
| `var/` | Generated evidence artifacts — never hand-edited, never committed fixtures |

## Verification

```bash
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cd apps/desktop/src-tauri && cargo test          # separate workspace
python -m pytest -q
cd apps/desktop && npm run test:evidence
python apps/desktop/test/server_contract.py      # run from the repository root
python tools/generate_pipeline_evidence.py       # full 23-step evidence pipeline
```

`python tools/session_status.py` runs all of the above except the pipeline and
records the real result. Three PostgreSQL tests are ignored unless
`FOLLON_TEST_DATABASE_URL` points at a disposable database.

## What this repository is not

It is **not approved for capital-bearing or customer-facing production use**,
by its own audit. Multiple external gates — 30 clean PAPER sessions, an
independent penetration test, legal review, paying users — sit at zero and
cannot be closed by writing code. Do not describe it as production-ready, and
do not add a feature that implies it is.
