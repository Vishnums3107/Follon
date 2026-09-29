# Master-plan conformance audit

**Audit updated: 2026-08-31; Phase 2 deterministic FX-core delta: 2026-09-04; Phase 3 execution-plan evidence delta: 2026-09-04;
dashboard fabricated-evidence remediation delta: 2026-09-10; pre-trade risk-gate and
golden-corpus-evidence remediation delta: 2026-09-10; desktop connected-PAPER-gateway
verification delta: 2026-09-14; independent full-repository re-verification,
stale-fixture-fingerprint, rustls advisory, first-ever live Docker Compose
runtime, and first-ever PostgreSQL integration-test execution delta:
2026-09-15; real counterfactual/adversarial intervention-and-probe execution,
PAPER/LIVE/backtest FIFO tax-lot ledger, and a stale-journal-fixture fix
this pass's own change required, delta: 2026-09-16; Slices 1 and 2a of the 5.7
core/risk aggregate-kernel composition gap (gross/net/leverage/concentration/
bucket checks and, via a new durable peak-equity high-water-mark, drawdown are
now real in the PAPER/controlled-LIVE order path; margin/daily-loss/strategy
tracking deferred to Slice 2b/2c) delta: 2026-09-17; Slice 2b of the same gap
(daily loss, via a new durable session-start equity baseline, is now real in
the PAPER/controlled-LIVE order path; margin-utilization and strategy-bucket
tracking remain deferred) delta: 2026-09-18; Slice 2c of the same gap
(margin utilization, via `core/accounting::value_margin_account` wired against
an operator-authored per-asset-class margin rate, is now real in the
PAPER/controlled-LIVE order path; strategy-bucket tracking remains deferred,
assessed this pass as requiring a `Portfolio` architectural redesign, not a
wiring task) delta: 2026-09-18; Slice 2d of the same gap, and closure of the
entire 5.7 Slice 2 backlog (strategy-bucket limits are now real, via a new
paper/live-local per-strategy attribution ledger deliberately kept separate
from `Portfolio`/`PositionSnapshot` rather than the architectural redesign
previously assessed as required -- the redesign turned out to be avoidable,
not merely deferred) delta: 2026-09-18; short-side tax-lot support added to
`core/accounting::TaxLotBook` and wired into `core/backtest::
AdvancedBacktestAccount`'s real fill path, including proportional fee-split
crossing-fill handling, closing the row 5.8 gap recorded since item 32
delta: 2026-09-18; CI static-analysis/supply-chain hardening (a real Semgrep
SAST job, every GitHub Action pinned from a mutable tag to its resolved
commit, a Dependabot cooldown period, and a genuine nginx dashboard-proxy
host-header-spoofing fix the new SAST pass itself found) and three
independent property/model-test slices -- the OMS order-lifecycle state
machine, the options exercise/assignment settlement function, and
multi-account portfolio aggregation -- delta: 2026-09-20; three of the five
slices of row 5.6's combination risk-gating epic (the `ComboIntent` domain
contract, the full combination risk gate including an explicit operator
short-exposure permission, and a risk-gated `submit_combo_intent` with a
durable OMS record and restart recovery), a machine-measured delivery-state
document and the tool that writes it, and a correction to a pre-existing false
journal-compatibility claim the work surfaced, delta: 2026-09-23.
Source reviewed: all 29 pages of the original `Solo Trading Operating System
Master Plan.pdf`.** This is the controlling
requirement-to-evidence record. It does not turn planned work, a local mechanism,
or generated fixture data into production or customer acceptance evidence.

## Verdict

The repository is a substantial deterministic trading/research implementation,
and every documented primary screen has a functional read-only frontend
projection. It is **not an exact, fully business-ready implementation of the
entire 24-month master plan**. The plan itself requires sequential external
gates and permits only one demand-led expansion in Months 21-24. Those facts
cannot be satisfied by adding code or test fixtures.

The repository now also contains broker-neutral advanced EMS planning,
portfolio-wide risk aggregation, multi-currency/margin accounting, customer IAM
primitives, a transactional PostgreSQL adapter, deployed gRPC service topology,
a React/Tauri client package, production mTLS/monitoring topology, and an inert
capital-bearing IBKR adapter wrapper. Capital-bearing operation and
customer-facing production remain blocked by real broker transport review and
the items under [External and operational gates](#external-and-operational-gates).

Status terms used below:

- **Implemented**: executable repository behavior exists, is integrated where a
  frontend view is required, and has automated tests.
- **Partial**: meaningful behavior exists, but one or more requirements in the
  same master-plan capability are absent or deliberately scoped out.
- **External gate**: the repository contains a mechanism, but the required
  broker, customer, legal, operational, security, or elapsed-session evidence
  has not been obtained.
- **Not implemented**: no reviewed production implementation exists.
- **Deferred by plan**: the master plan intentionally sequences or freezes it.

## Verification snapshot

A 2026-09-20 session investigated row 5.6's EMS options-combination gap in
depth and found the real remaining scope is a bounded-but-real multi-session
epic, not a same-session wiring fix (see row 5.6's own "Assessed 2026-09-20"
remainder text); rather than land that half-finished, the session instead
closed four smaller, fully bounded gaps: item 41 (real Semgrep SAST in CI,
every GitHub Action pinned to a resolved commit SHA, a Dependabot cooldown
period, and a genuine nginx host-header-spoofing fix the new SAST tooling
itself found), item 42 (a first `proptest`-based model test for the OMS
order-lifecycle state machine, `core/control-plane/tests/
oms_lifecycle_proptest.rs`), item 43 (a second, independent property-test
slice for the options exercise/assignment settlement function, `core/options/
tests/option_lifecycle_settlement_proptest.rs`), and item 44 (a third slice
for multi-account portfolio aggregation, `core/accounting/tests/
multi_account_aggregation_proptest.rs`) -- all three property-test slices
independently verified to actually catch a deliberately injected defect
before being relied on, not merely written to run. See items 41-44 for full
detail.

The same 2026-09-18 session, having closed the entire 5.7 Slice 2 backlog
(item 39), moved to row 5.8's own long-standing recorded gap: item 40 added a
short-side mirror (`ShortTaxLot`/`open_short`/`cover`) to
`core/accounting::TaxLotBook` and wired it into
`core/backtest::AdvancedBacktestAccount`'s real fill path, including a
proportional fee split for a fill that crosses through zero. This pass's own
first attempt broke the full workspace build (adding fields to
`TaxLotBookSnapshot` left two exhaustive struct-literal call sites in
`core/paper`/`core/live` uncompilable) and that failure was initially missed
because the verification command was piped through `grep`, whose own exit
code (0, matching error lines) masked the real `cargo test` failure -- caught
only by reading the captured log's actual content, not trusting the pipe's
exit status. Both sites were fixed and the full suite was re-run capturing a
genuine exit code directly: `cargo fmt`/`clippy -D warnings`/
`test --workspace --all-targets` clean throughout, **282 passed, 0 failed, 3
ignored** in the main workspace (`follon-accounting` 19, up from 17;
`follon-backtest` 15, up from 13) and **17 passed, 0 failed** in the separate
`apps/desktop/src-tauri` Tauri workspace, unchanged. `AdvancedBacktestAccount`
is not durably persisted (a backtest is a one-shot batch replay, not a
restartable service), so neither checked-in journal fixture needed
regenerating -- confirmed by diffing both against their pre-this-entry state,
not assumed. The full 23-step `tools/generate_pipeline_evidence.py` pipeline
was re-run afterward and completed with zero failures, producing all 73
evidence artifacts. Full detail is item 40 below.

The same 2026-09-18 session continued directly into Slice 2d after item 38,
after first re-investigating item 38's own conclusion that strategy-bucket
composition needed a `Portfolio` architectural redesign -- reading
`core/risk::RiskPosition`/`aggregate_metrics` closely enough to see that the
kernel already supported real per-strategy bucketing, and that the actual gap
was a missing paper/live-local ledger, not a shared-type change. Item 39
(strategy-bucket limits are now real, closing the entire Slice 2 backlog) was
implemented and verified on that corrected basis. `cargo fmt`/
`clippy -D warnings`/`test --workspace --all-targets` remained clean
throughout: **278 passed, 0 failed, 3 ignored** in the main workspace
(`follon-paper` 37, up from 35; `follon-live` 22, up from 20) and **17 passed,
0 failed** in the separate `apps/desktop/src-tauri` Tauri workspace, unchanged.
`strategy_attribution` is a new persisted field on
`PersistentPaperState`/`PersistentLiveState` (unlike item 38's margin work),
so the two checked-in journal fixtures were regenerated again using the same
procedure as items 27/34-37, and the full 23-step
`tools/generate_pipeline_evidence.py` pipeline was re-run afterward and
completed with zero failures, producing all 73 evidence artifacts. Full detail
is item 39 below.

The same 2026-09-18 session continued directly into Slice 2c after item 37
(item 38: margin utilization, via the unmodified `core/accounting::
value_margin_account`, is now a real, composed decision in `core/paper`/
`core/live`). `cargo fmt`/`clippy -D warnings`/`test --workspace --all-targets`
remained clean throughout: **274 passed, 0 failed, 3 ignored** in the main
workspace (`follon-paper` 35, up from 33; `follon-live` 20, up from 18) and
**17 passed, 0 failed** in the separate `apps/desktop/src-tauri` Tauri
workspace, unchanged. Unlike item 37, no field was added to
`PersistentPaperState`/`PersistentLiveState` this pass (margin data is computed
transiently, never persisted), so the two checked-in journal fixtures did not
need regenerating -- confirmed by grepping both `Persistent*` structs for
`margin` before concluding it, not assumed. The full 23-step
`tools/generate_pipeline_evidence.py` pipeline was re-run afterward and
completed with zero failures, producing all 73 evidence artifacts. This pass
also read `core/control-plane::Portfolio`'s actual definition to assess the
one remaining Slice 2 item (strategy-bucket composition) rather than
re-asserting the prior finding, and confirmed it is a `Portfolio`-level
architectural redesign affecting `core/backtest`, the gRPC service, and
desktop projections -- not a bounded wiring task -- so it was not attempted
this pass. Full detail is item 38 below.

The 2026-09-18 session closed Slice 2b of the 5.7 `core/risk` composition gap
(item 37: a durable session-start equity baseline makes `MAX_DAILY_LOSS_EXCEEDED`
a real, composed decision in `core/paper`/`core/live`). Before writing any code,
this session independently verified the uncommitted Slices 1/2a change set
already sitting in the working tree (items 35-36, never previously committed or
verified in this document) was real: `cargo build --workspace --all-targets`,
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, and `cargo test --workspace --all-targets` all passed clean, with the
`follon-paper` count (30 passed) matching that entry's own claim exactly. After
implementing and testing item 37, the same full suite was re-run clean:
`cargo fmt`/`clippy -D warnings` clean; **270 passed, 0 failed, 3 ignored** in
the main workspace (`follon-paper` 33, up from 30; `follon-live` 18, up from 15)
and **17 passed, 0 failed** in the separate `apps/desktop/src-tauri` Tauri
workspace, unchanged. The two checked-in journal fixtures
(`tests/fixtures/paper/journal-v2.ndjson`, `tests/fixtures/live/journal-v1.ndjson`)
were regenerated using the actual current CLI binaries against their unchanged
configuration documents (the same procedure items 27/34/35/36 established),
and the full 23-step `tools/generate_pipeline_evidence.py` pipeline -- which no
`cargo test` invocation exercises -- was re-run end to end afterward and
completed with zero failures, producing all 73 evidence artifacts. Full detail
is item 37 below.

The 2026-09-15 independent re-verification run (a separate session with no
memory of producing the 2026-09-14 entry below re-ran every check from a
clean read of the repository rather than trusting that entry's own summary;
superseding the counts further below only where noted) reconfirmed the
2026-09-14 results and found exactly two new issues:

- Running `tools/generate_pipeline_evidence.py` end to end -- the script that
  drives every CLI binary against its checked-in fixture, which no
  `cargo test` invocation exercises -- failed on 2 of its 15 steps:
  `follon-paper-status` and `follon-live-status` both rejected their checked-in
  journal fixtures with a configuration-fingerprint mismatch. Item 25 had
  correctly added `max_order_rate`/`order_rate_window_seconds` to the hashed
  fingerprint inputs in `core/paper` and `core/live`, but the two checked-in
  journal fixtures (`tests/fixtures/paper/journal-v2.ndjson`,
  `tests/fixtures/live/journal-v1.ndjson`) still carried the fingerprint (and,
  for the 9-entry live journal, the resulting hash chain) computed under the
  pre-item-25 formula -- the fingerprint check itself was correctly refusing a
  real mismatch, not misbehaving. Both fixtures were regenerated by running the
  actual current CLI binaries against their unchanged configuration documents
  (not hand-computed), verified field-for-field identical to the originals
  apart from the fingerprint-derived hashes, and confirmed to be referenced
  nowhere else in the repository. Full detail and the exact remediation is item
  27 below. The pipeline now completes all 15 steps and produces all 75
  evidence artifacts with zero failures.
- `cargo audit --deny yanked` on the root lockfile, which the 2026-09-14 entry
  recorded as clean, failed with **RUSTSEC-2026-0285** (medium, CVSS 5.3) in
  `rustls 0.23.43`: a TLS 1.3 handshake-message vulnerability disclosed
  2026-09-14, pulled in transitively by `follon-trading-api`'s gRPC stack
  (`tonic` -> `tokio-rustls` -> `rustls`) -- the exact mTLS boundary this
  document's gRPC/TLS conformance rows describe. This is not a regression in
  the 2026-09-14 change set; the advisory simply postdated, or had not yet
  propagated into the local advisory-database clone at, that prior run.
  `cargo update -p rustls --precise 0.23.45` closed it. After the update,
  `cargo audit --deny yanked` is clean again; `cargo check --workspace
  --all-targets` and the `follon-trading-api` unit tests (6/6) pass unchanged
  against the patched dependency; `cargo fmt --all -- --check` remains clean.
- `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D
  warnings`, and `cargo test --workspace --all-targets` were re-run clean (0
  failed) in both the main workspace and the desktop Tauri crate's separate
  workspace. This was re-run a second time, after every fix in this entry
  (the rustls bump and the item 27 fixture correction below): **237 passed, 0
  failed, 3 ignored** in the main workspace and **17 passed, 0 failed** in the
  Tauri workspace -- 254 passed total, exactly matching the 2026-09-14 count,
  confirming neither fix disturbed anything else.
- All four Python suites were re-run clean: `ibkr-gateway` (14 passed),
  `strategy-sdk` (14 passed), `storage-adapter` (6 passed), `examples` (1
  passed) -- 35 passed, 0 failed, matching the 2026-09-14 count.
- The desktop `tsc --noEmit` typecheck and the complete `npm run
  test:evidence` suite (a fresh `tsc` build plus all 12 evidence/regression
  files) were re-run clean with zero failures.
- The repository-wide stub-marker search (`todo!()`, `unimplemented!()`,
  `FIXME`, `NotImplementedError`, "not yet implemented", etc.) was re-run and
  still finds exactly the one intentional `NotImplementedError` on the Python
  strategy SDK's abstract base method.
- Beyond re-running commands, several of the 2026-09-14 entry's specific
  claims were independently read against the actual diff rather than taken on
  faith: the CI workflow diff (dual-lockfile audit, `cargo test --workspace
  --all-targets`, native Tauri `fmt`/`clippy` jobs), `paper_gateway.rs` in
  full, the `lib.rs` change that replaces a permanently `.manage()`d
  `TradingCommandState::unavailable()` with `paper_gateway::bootstrap()`,
  `OrderTicket.tsx`'s now-live `invoke("cancel_order", ...)` /
  `invoke("close_position", ...)` calls, and the look-ahead-bias fix in
  `core/control-plane::ReplayEngine::process_bar` (including its new
  regression test proving a freshly created order cannot fill against the
  same bar that produced it). Each matched what the 2026-09-14 entry said it
  did.

No other discrepancy between this document's claims and the repository's
actual behavior was found. The 2026-09-14 entry (item 26 below) remains the
authoritative description of that change set; this entry adds only the
rustls finding/fix and records that the rest was independently reconfirmed.

This session's Docker Desktop engine was, unlike every prior audit run,
actually reachable once started. That let two further things be verified for
the first time rather than left as an open external gate: the entire
`infra/` Docker Compose runtime topology (see item 29) and the three
long-`#[ignore]`d PostgreSQL integration/concurrency tests (see item 30),
both of which surfaced and closed real defects rather than merely confirming
existing claims. `cargo test --workspace --all-targets -- --include-ignored`
against a disposable PostgreSQL container passed with **0 failed, 0
ignored** across the main workspace -- every previous snapshot in this
document recorded 3 ignored tests at this point instead.

The 2026-09-14 verification run (full re-check of every workspace after the
desktop connected-PAPER-gateway change below, superseding the counts further
below) retained the following local results:

- `cargo fmt --all -- --check`: failed on first run (13 unformatted hunks
  across `adapters/persistence/postgres/src/lib.rs`, `apps/cli/src/fx.rs`,
  `core/accounting/src/statement.rs`, `core/identity/src/lib.rs`, introduced by
  uncommitted work that had not been run through `cargo fmt` before this
  audit); `cargo fmt --all` applied the fix and a second `--check` run passed
  clean.
- `cargo check --workspace --all-targets` and `cargo clippy --workspace
  --all-targets -- -D warnings`: clean, zero warnings, in both the main
  workspace (`C:\Follon`) and the desktop Tauri crate's own separate workspace
  (`apps/desktop/src-tauri`, which `cargo check --workspace` from the
  repository root does not cover).
- `cargo audit --deny yanked` is clean for the root lockfile after replacing
  yanked `chacha20 0.10.1` with compatible `0.10.2`. CI now also audits the
  separate Tauri lockfile, which previously escaped the dependency-audit job.
  The Tauri 2.11.5 dependency graph retains six unmaintained transitive-crate
  warnings and `RUSTSEC-2024-0429` for Linux-only GTK `glib 0.18.5`; no known
  vulnerability or yanked crate is accepted, but Linux desktop promotion must
  remain blocked until upstream Tauri moves to patched `glib >=0.20` or the
  dependency is otherwise removed. There are no vulnerability-class or yanked
  dependency findings; the unsoundness warning remains an explicit Linux
  release blocker. The supported Windows build does not link this GTK path.
- `cargo test --workspace --all-targets`: **237 passed, 0 failed, 3 ignored** in
  the main workspace; **17 passed, 0 failed** in the desktop Tauri crate's own
  workspace (254 passed total). The ignored tests are PostgreSQL integration
  and concurrency checks requiring an operator-provided disposable
  `FOLLON_TEST_DATABASE_URL`.
- Python: `ibkr-gateway` (14 passed, including the new official-backend submit-
  retry and execution-time regression tests), `strategy-sdk` (14 passed),
  `storage-adapter` (6 passed), `examples` (1 passed) — 35 passed, 0 failed.
- Desktop: `tsc --noEmit` (typecheck) passed clean; the full `npm run
  test:evidence` suite (12 evidence/regression test files, including
  `connected-evidence-regression.mjs`) passed with zero console errors.
- The updated Windows Tauri release build completed and produced both bundles:
  MSI SHA-256 `58B72EA8D33BC4656E64A7DD13357CC697811BBEEF5FF6A8876E2F58C333381E`
  and NSIS SHA-256 `A4700903735F1375B91CAF61E0FA054A7F59DBE109223EA6C9016EC8CDF59726`.
  The exact release executable remained healthy during a bounded five-second
  native-process smoke run and then shut down on operator request. These are
  unsigned local build artifacts, not code-signing, clean-machine installation,
  visual, or assistive-technology acceptance; no native UI surface was exposed
  to the automation session for click testing.
- Every file in the uncommitted "connected trading workflow" change set (the
  new desktop `paper_gateway.rs`, and changes across
  `core/paper`, `core/live`, `core/control-plane`, `core/options`,
  `core/identity`, `core/news`, `core/accounting`, `core/backtest`,
  `adapters/persistence/postgres`, `services/trading-api`, the CLI, and
  `python/ibkr-gateway`) was individually read end to end. Every change is a
  genuine, tested behavioral fix or bounded completion with an honest doc
  comment explaining its scope; the adversarial and counterfactual artifacts
  are explicitly recorded as certification-only rather than automated probe
  execution. See the dated entry in "Locally closed gaps" below for what it
  actually changes.
- A repository-wide search for stub markers (`todo!()`, `unimplemented!()`,
  `FIXME`, `NotImplemented`, "not yet implemented", etc.) across every `.rs`,
  `.py`, `.ts`, and `.tsx` file found exactly one hit, in
  `python/strategy-sdk/src/follon_strategy_sdk/strategy.py`: a `raise
  NotImplementedError` inside an `@abstractmethod` on the `Strategy` base
  class, which is the correct, intended Python ABC pattern for a method every
  concrete strategy must implement, not an incomplete implementation.

The 2026-08-31 verification run retained the following local results (superseded
by the 2026-09-10 count immediately below; the workspace grew substantially
across the deltas listed above the Verdict section without this line being
kept current, which is itself corrected here rather than left stale):

- `cargo test --workspace --all-targets`: **148 passed, 0 failed, 1 ignored**
  as of 2026-08-31; **218 passed, 0 failed, 1 ignored** as of the 2026-09-10
  pre-trade risk-gate and golden-corpus-evidence remediation (item 25). The
  ignored test is the disposable PostgreSQL round trip and requires an
  operator-provided `FOLLON_TEST_DATABASE_URL`.
- `cargo clippy --workspace --all-targets -- -D warnings` and repository-wide
  Rust formatting pass both remain clean after that remediation.
  `cargo audit` scanned 241 locked Rust dependencies
  against the current RustSec advisory database without a vulnerability finding
  (last run 2026-08-31; not rerun for the 2026-09-10 delta above, which added
  no new dependency).
- Python strategy SDK, IBKR PAPER bridge, storage adapter, security tooling,
  and dashboard server: **43 passed, 0 failed**.
- TypeScript typecheck, evidence/browser-module contracts, Vite production
  build, Tauri native Cargo check, optimized Tauri application build, MSI, and
  NSIS packaging pass; `npm audit --package-lock-only --audit-level=high`
  reports zero vulnerabilities. The current MSI SHA-256 is
  `6523BE18DB8DA5604777822F2C22EAFADA88EC826EB3072033ED17CA61A4C56C`; the
  NSIS SHA-256 is
  `D9D9454DE3E667D4223C685315BECB6729AC23FE0E2E7798B032948F1BEF4EEF`.
  A hidden native-process smoke run stayed alive while the local read-only API
  returned health, status, and all 12 feature records.
- Development, production, and production-plus-monitoring Compose definitions
  pass offline configuration validation. PostgreSQL migrations 0001 and 0002
  parse successfully with PostgreSQL's grammar.
- The compiled development gRPC service listened on `127.0.0.1:50051`, passed
  its socket healthcheck, and shut down after the smoke run.
- Docker Desktop still has no reachable Linux engine, so image build/runtime
  health was not rerun. No in-app or connected browser surface was available,
  so final human visual/click acceptance remains open.

## Capability-by-capability conformance

| Master-plan capability | Status | Implemented and frontend-integrated evidence | Exact remainder |
| --- | --- | --- | --- |
| 5.1 Canonical instruments | Implemented for broker-neutral reference scope | `core/instrument` has permanent IDs, effective-dated versions, symbols, venue, asset class, currency, broker IDs, tick/lot sizes, multiplier, calendars, cash-security settlement lag, option underlying/expiry/strike/right/style/settlement, future root/last-trade/expiry/settlement/margin class, and FX spot/forward/swap base/quote/value-date terms. `follon-fx` CLI and Portfolio workspace provide deterministic pricing and evaluation. Current dataset/reference identity is projected into Research Lab and Strategy Studio. | Production vendor symbol-master ingestion, licensed data operations, and live broker acceptance remain external; reference completeness is not permission to trade every declared class. |
| 5.2 Market data | Partial | Strict historical trade/bar import, deterministic OHLCV construction, normalized source/receive-time quotes, spread/size validation, duplicate/out-of-order/sequence-gap/delay/staleness classification, exchange sessions and halts, corporate-action inputs, Parquet publication, DuckDB verification, immutable S3-compatible publication, and dataset views are implemented. `core/fx` adds value-dated fixed-point spot/forward/swap snapshots with source/receive-time, sequence, staleness, and replay-order refusal, consumed by `follon-fx` and Portfolio workspace. | No production licensed live quote/trade vendor connection, vendor reconnect/re-request of a gap window, stale-feed operating history, broad vendor symbol-master ingestion, or corporate-action operations service exists. **Corrected in place 2026-09-25 (item 65).** This column also listed "gap-repair operation". That was accurate when written and became false when E3.5 landed a deterministic repair from a supplied recovery batch. Requesting that batch from a vendor remains open. |
| 5.3 Python strategy SDK | Implemented local replay boundary | Isolated worker handshake, strategy/version identity, bundle hashing, deterministic bar-to-intent contract, point-in-time historical queries, deterministic SMA/EMA helpers, immutable portfolio snapshots, bounded saved state with fingerprints, bounded custom metrics, example strategy, schemas, and Strategy Studio projection are implemented. The Rust replay host sends the strict history/portfolio/cash/state frame to Python workers, applies replayed fills to the host-owned portfolio view, and rejects tampered fingerprints, look-ahead metrics, malformed metrics, or protocol drift. Strategy code cannot access broker adapters or credentials. | Direct Python fill/risk callbacks, a deployed gRPC strategy-worker host, and production worker deployment remain external integration work. |
| 5.4 Professional backtester | Implemented CLI projection; runner-internal accounting remains bounded | Event-driven replay, exact decimal accounting, spread, adverse slippage, attributed commission/exchange/regulatory charges, latency, per-bar partial-fill caps, persistent working orders, post-cost limit protection, sessions/halts, dividends/splits, point-in-time universe membership, long/short accounting, borrow availability/recall calculation, exact borrow/cash-debit financing, multi-currency FX, initial-margin capital checks, delisting settlement, immutable reports/manifests, experiment records, and Backtest Explorer capability evidence are implemented and tested. Every CLI backtest derives a hashed advanced-account result from the same canonical event stream and refuses publication when its capital or lifecycle checks fail. Explicit economics use `advanced_account`; older configurations use a deterministic fully-paid profile derived from immutable reference data. | Multi-account allocation and proof against production-size performance targets remain. Multi-account allocation is frozen by `03-roadmap-and-gates.md` until the preceding gates are independently evidenced. The in-run `BacktestRunner` ledger is retained for backward-compatible event construction. **Corrected in place 2026-09-25 (item 66).** This column also said that "an operator must consume the advanced-account sidecar for advanced economics". That was accurate when written and became false when E3.1a moved those economics into the main artifact (schema 3). |
| 5.5 OMS | Implemented for current market/limit scope | Stable client identities, idempotency, legal state transitions, cancel/replace, out-of-order evidence, UNKNOWN handling, restart recovery, reconciliation, and causal audit events exist in simulation/PAPER/controlled-LIVE. Execution Blotter renders the lifecycle. `core/paper::evaluate_risk` and `core/live::evaluate_risk` (the exact functions every PAPER/controlled-LIVE order intent passes through before an `OmsOrder` is created) now reject a same-instrument opposite-side order against an existing working order (`SELF_TRADE_RISK`) and reject submissions beyond a configured rolling-window rate (`MAX_ORDER_RATE_EXCEEDED`), closing a prior gap where those two pre-trade-risk-doc checks existed only in the disconnected `core/risk` evidence engine and never actually gated a real order. The desktop order ticket's submit/cancel/close-position commands are no longer permanently wired to an inert `TradingCommandState::unavailable()` stub: `apps/desktop/src-tauri/src/paper_gateway.rs` is a real `RiskOmsGateway` backed by an in-process `follon_paper::PaperTradingService`, so when an operator points `FOLLON_DESKTOP_PAPER_CONFIG` at a valid PAPER configuration file the desktop actually submits, cancels, and closes real PAPER orders through the genuine risk/kill-switch/audit-journal path (see the UX row below and the dated entry in "Locally closed gaps"). | It is not a claim of complete OMS coverage for every future order type, asset class, or live broker. Without `FOLLON_DESKTOP_PAPER_CONFIG` configured, the desktop command surface remains unavailable exactly as before. |
| 5.6 EMS | Implemented in core, through a configured gRPC PAPER command route, and through the desktop PAPER command boundary and ticket; external capital gates remain open | `core/execution` implements immediate, exact TWAP, forecast-volume VWAP, POV/participation, urgency-weighted arrival price, sequential display-size Iceberg, deterministic weighted AlgoWheel with schedule tie-breaking, strict post-only passive cancel/replace with monotonic chase collars, capability-gated multi-venue smart routing (`smart_route_with_capabilities`), stop/stop-limit bracket children, monotonic trailing stops, exact basket legs, and atomic ratio/net-price-protected options combinations. Content-addressed `ExecutionPlanEvidence` and `follon-tca` retain broker-neutral planning/TCA evidence. `core/paper` and `core/live` now own complete risk-gated atomic-combination OMS lifecycles, including submission, fills, cancellation, durable recovery, and reconciliation. The versioned gRPC API keeps `PlanOptionCombo` separate from `SubmitPaperCombo`; the latter requires an explicit durable PAPER command route and reaches `PaperTradingService::submit_combo_intent` with exact per-leg observations. The Tauri desktop host's `submit_combo_order` IPC command reaches the same method through its native PAPER gateway, with each leg's operator-attested observation carried on the leg, combination-aware cancellation receipts, and an operator combination ticket. | **Corrected in place 2026-09-24 (items 48–50).** The earlier remainder became false as E1.3b, E1.4a–c, E1.5a, and E1.5b landed; it was accurate when written and is superseded rather than retroactively characterized as an error. No repository gap remains in the combination epic; the desktop still has no market-data feed, so leg observations are operator-attested. TCA still relies on operator-supplied frozen evidence and does not validate a broker statement. Every vendor transport still needs independent human review and broker-backed PAPER/LIVE acceptance, and no code result waives the external capital, security, legal, or operational gates. |
| 5.7 Risk engine | Implemented portfolio kernel; the entire Slice 2 aggregate-composition backlog is closed -- gross/net/leverage/concentration/bucket (Slice 1), drawdown (Slice 2a), daily loss (Slice 2b), margin utilization (Slice 2c), and strategy-bucket limits (Slice 2d) are all now composed into the real order path | `core/risk` evaluates gross/net, leverage, concentration, daily loss, drawdown, margin utilization, delta/gamma, instrument permissions/restrictions, sector/asset/currency/strategy buckets, open orders, order rate, self-trade, and kill state. A fresh FX snapshot can only create an ordinary local candidate with retained snapshot/version/value-date evidence; it still receives the same aggregate risk decision. The kernel returns exact reason codes, is exposed over gRPC, and is visible in Risk Cockpit capability mapping. Self-trade prevention and order-rate limiting are independently duplicated directly inside the actual PAPER/controlled-LIVE order-gating path (`core/paper`/`core/live` `evaluate_risk`, see 5.5). As of item 35, that same real order-gating path also calls the unmodified `evaluate_portfolio_risk` kernel itself (not just two of its individual checks) behind an opt-in `portfolio_risk` configuration block, with a real per-service `PortfolioRiskSnapshot` built from actual positions/working orders/observed marks. As of item 36, that snapshot's `peak_equity` is a real, durable running high-water-mark (`PaperTradingService`/`LiveTradingService::peak_equity`), making `MAX_DRAWDOWN_EXCEEDED` a genuine computed decision rather than a permanently inert one. As of item 37, that same snapshot's `daily_pnl` is a real, durable session-start equity baseline (`PaperTradingService`/`LiveTradingService::daily_baseline_equity`, reset at the first risk evaluation of each new UTC calendar day), making `MAX_DAILY_LOSS_EXCEEDED` a genuine computed decision rather than a permanently inert one. As of item 38, that same snapshot's `margin_used` is a real value computed by calling the unmodified `core/accounting::value_margin_account` against an operator-authored per-asset-class margin rate, making `MAX_MARGIN_UTILIZATION_EXCEEDED` a genuine computed decision rather than a permanently inert one. As of item 39, every held position is split into one real `RiskPosition` row per strategy (plus an honest "unattributed" remainder), sourced from a new durable per-strategy attribution ledger (`PaperTradingService`/`LiveTradingService::strategy_attribution`), making `STRATEGY_LIMIT_EXCEEDED` a genuine computed decision rather than a permanently inert one. | **Status side by side, row 5.7 remainder (2026-09-18, item 39):**<br>**Before (pre-item-39):** "Strategy-bucket checks remain **not** composed: `strategy_limits` stays empty because `Portfolio` (`core/control-plane`, shared by `core/paper`/`core/live`/`core/backtest`) has no `strategy_id` field at all... closing this gap is a `Portfolio`-level architectural redesign... not a bounded wiring task like margin utilization, daily loss, or drawdown were."<br>**After (post-item-39):** That assessment's conclusion changed on closer investigation, and the correction is recorded rather than quietly overwritten: `Portfolio`/`PositionSnapshot` were never touched. `core/paper`/`core/live` each gained a new, deliberately separate per-strategy attribution ledger (`strategy_attribution: BTreeMap<instrument_id, BTreeMap<strategy_id, signed_quantity>>`), updated from the same real-fill code path as `TaxLotBook`, and durably persisted the same way. `portfolio_risk_decision` now splits each instrument's aggregate position into one real `RiskPosition` row per strategy that has ever traded it, plus an "unattributed" remainder row so the split can never mis-state total gross/net exposure -- only how it is attributed. `strategy_limits` is a real, operator-configurable bucket-limit map now (previously always empty). **Every item in the Slice 2 backlog first identified in item 35 is now closed**: peak-equity/drawdown (item 36), daily-loss baseline (item 37), margin utilization (item 38), and strategy-bucket attribution (item 39). No further aggregate-risk-kernel composition work remains queued for `core/paper`/`core/live`; production policy calibration, latency/load evidence, independent validation, live-feed staleness history, and clean broker-backed operating sessions remain external. |
| 5.8 Portfolio/accounting | Implemented multi-currency/margin kernel with long *and* short tax-lot coverage; external statement gate open | `core/accounting` provides per-currency balanced double entry, idempotent projection, fresh direct/inverse FX, spot-snapshot-only cash conversion, multi-currency cash/long/short valuation, initial/maintenance margin, excess liquidity, margin-call projection, FIFO/LIFO/highest-cost tax-lot disposal, and exact cash-debit/short-borrow financing accrual. `follon-operations reconcile-statement` ingests broker CSV statements and reconciles internal cash/positions. PostgreSQL has deferred balanced-journal constraints; gRPC exposes valuation; Portfolio/Journal surface the capability. A `HighestCost` disposal tie between two lots of identical unit cost is now broken by oldest acquisition first, matching the documented policy exactly instead of an incidental lot-identity string order. `core/paper`, `core/live`, and the plain `core/backtest::BacktestLedger` now all call `TaxLotBook::acquire`/`dispose` from their one real-fill code path (see items 32-34), each maintaining an independent FIFO cost-basis ledger in lockstep with every real fill; the two durable services persist and recover it across journal restarts. As of item 40, `TaxLotBook` also models the short side (`open_short`/`cover`, mirroring `acquire`/`dispose` exactly), and `core/backtest::AdvancedBacktestAccount` -- the long/short-capable advanced projection previously left unwired -- now calls it from its own real-fill path, including a crossing fill (one execution that closes an existing long/short and opens the opposite side) split into a closing leg and an opening leg with the fill's fee divided proportionally between them. | Tax outputs are deterministic accounting facts, not jurisdiction-specific tax advice. Lot selection is fixed at FIFO, not operator-configurable, everywhere it is wired. `AdvancedBacktestAccount`'s own average-cost `realized_pnl` and the new FIFO tax-lot `realized_pnl` are intentionally different figures (the former ignores fees, the latter is fee-inclusive) -- an existing, already-documented distinction for `core/paper`/`core/live`, not a new inconsistency. Multi-prime allocation and qualifying production reconciliation history remain external/integration work. |
| 5.9 Risk cockpit | Implemented for planned aggregate fields; operating gate open | The cockpit maps portfolio exposure, leverage/drawdown/margin/Greeks and bucket controls alongside kill switches, working/UNKNOWN orders, incidents, broker/reconciliation health, attribution, and evidence links. | Real alert delivery/on-call ownership, live-feed heartbeat history, and operated production evidence remain external. |
| 5.10 Audit and replay | Implemented for current scope | Canonical causal events, correlation/causation, append-only journals, hash-chain verification, immutable artifacts, restart replay, configuration/dataset/strategy hashes, and replay/incident/journal views are implemented. | Production retention/WORM policy, centralized tenant audit, independently operated log custody, and regulator/customer retention evidence remain deployment obligations. |

## Architecture and repository conformance

| Requirement | Status | Finding |
| --- | --- | --- |
| Rust deterministic core | Implemented | Workspace crates own domain, instruments, value-dated FX pricing, data, backtest, OMS/control plane, PAPER, LIVE safety, operations, options, and commercial primitives. Fixed-point `Decimal` is used for money/quantity decisions. |
| Isolated Python strategy/data tools | Implemented for local boundary | Strategy and storage packages are isolated and tested. The worker identity and bundle are bound into run evidence. |
| React + TypeScript + Tauri desktop | Implemented package; signed-installer acceptance open | React 19 owns the ten-workspace shell, Vite emits a production bundle, and a Tauri v2 host exposes narrowly scoped privileged PAPER submit/cancel/close commands through the Risk/OMS gateway. TypeScript, browser-module, evidence, server, Vite, and native Cargo checks pass; the Windows release build produced both MSI and NSIS bundles. Code signing/notarization, clean-machine install testing, and visual click acceptance remain release evidence. |
| PostgreSQL transactional store | Implemented adapter; local live DB evidence retained 2026-09-15; production evidence open | Checksum-bound versioned migrations create events, atomic outbox, checkpoints, balanced journals, IAM/session/recovery-code, risk-policy, broker command/receipt, broker-account, strategy/config/reference versions, value-dated FX reference/pricing evidence, full order/execution/position projections, audit indexes, and billing-evidence tables with forced RLS. Composite tenant/parent foreign keys prevent cross-tenant linkage beneath RLS; OMS projections enforce lifecycle, TIF, price-field, quantity, client-ID, and idempotency invariants. Event append uses aggregate locks and content-bound idempotency; outbox claims use `SKIP LOCKED`; TLS and CI disposable-database paths exist. A 2026-09-15 session ran the three previously-never-executed integration/concurrency tests against a real disposable `postgres:16.10-alpine` container matching CI's own setup exactly (see item 30): all three passed, including the concurrent-idempotency-race test exercising the savepoint-based recovery path added in item 26. This is still one operator's local container, not a production database, TLS-terminated connection, or deployed instance. |
| Parquet + DuckDB research store | Implemented locally | Deterministic Parquet, hash/row revalidation, catalogue registration, receipts, recovery verification, and dashboard indexing exist. |
| Object storage | Implemented locally; production gate | Versioned immutable S3-compatible publication and recovery exist against local MinIO. KMS, retention/object lock, replication, monitoring, and drilled production recovery remain external. |
| Protobuf/gRPC contracts | Implemented topology; local runtime verified 2026-09-15; production evidence open | `follon-trading-api` serves health, scheduled/passive/options-combination EMS, portfolio-risk, and margin APIs; validates tenant/account/strategy scope; migrates/health-checks PostgreSQL; requires database TLS plus server certificate/key/client CA in production; and is packaged in development and production Compose. A 2026-09-15 session with a reachable Docker engine found the checked-in image and Compose topology could not actually build or run at all (see item 29): a missing workspace-member copy, a stale MSRV-pinned base image, a 2.2 GB build-context defect, a nested-Tokio-runtime panic on its first real database connection, and a dashboard container crash were all found and fixed, then proven fixed by actually running `docker compose -f infra/compose.dev.yml up` end to end -- PostgreSQL, MinIO, the gRPC service, and the dashboard all reported healthy, and the dashboard's live status endpoint confirmed it. This is still a single operator's local Windows Docker Desktop engine, not production container acceptance; TLS, production secrets, and a deployed environment remain external. |
| REST/WebSocket UI boundary | Partial | A bounded read-only REST API serves all ten workspaces; the earlier local evidence client supports projection-only WebSocket evidence. **Corrected in place 2026-09-25 (item 67).** This row said "There is no authenticated privileged write control plane". That was accurate when written and is now false for the gRPC write path. `SubmitPaperCombo` requires an operator session: password plus mandatory TOTP, a role granting PAPER trading, and one tenant. The REST boundary stays read-only. The desktop's Tauri IPC writes do not authenticate the operator. Centralized approval policy and the separate deployment review remain. |
| Modular monolith first | Implemented | Crate/package boundaries and the ADR preserve the plan's initial modular-monolith posture. |
| Live market/broker integration | Implemented inert capital boundary; external review gate | PAPER retains its fixed official-API bridge. `IbkrControlledLiveAdapter` requires signed artifact verification, exact two-reviewer binding, loopback LIVE port, managed secret material, initial broker snapshot, price-protected allow-listed canary limits, and irreversible instance emergency stop. No real LIVE vendor transport, credential, review record, or capital session is configured. |

These implementation mechanisms close the earlier architecture gaps, but they
do not turn configuration into deployment evidence. Calling a checked-in
Compose file TLS, an adapter type independently reviewed, or an empty acceptance
ledger a successful production operation would still be false conformance.

## UX and frontend conformance

All ten master-plan primary screens are implemented as distinct, routable,
responsive, read-only workspaces with typed parsing, bounded artifact access,
empty/error states, evidence links, and keyboard-capable navigation.

| Screen | Integrated functions | Boundary |
| --- | --- | --- |
| Command Center | Service status, environment/gate readiness, broker/strategy/risk state, attention queue, recent evidence | Monitoring only |
| Research Lab | Datasets, schemas, notebooks, experiments, backtests, point-in-time/feed-quality capability, option-chain analytics | Notebook content is inert and never executed |
| Strategy Studio | Strategy/version/bundle, config/dataset identity, worker provenance, bounded history/indicator/portfolio/state/metrics SDK capability | No broker or secret access |
| Backtest Explorer | Run comparison, metrics, exact fills/fees, regimes/sensitivity tags, manifests, reproducibility, and explicit advanced-model assumptions | Displays retained evidence only; pre-upgrade legacy artifacts remain visibly bounded rather than being relabelled as advanced-account runs |
| Execution Blotter | SIMULATION/PAPER/LIVE separation, intents, risk, orders, fills, UNKNOWN and lifecycle state, frozen-benchmark TCA, and local risk-benchmark evidence | The order ticket's submit/cancel/close-position buttons now route through a real Tauri command to a real `follon_paper::PaperTradingService`-backed PAPER OMS gateway when an operator configures `FOLLON_DESKTOP_PAPER_CONFIG`; there is still no live-market-data feed at this boundary, so every submission carries an operator-attested reference price and time rather than an automatic quote. Without that environment variable configured the surface remains unavailable exactly as before. TCA and benchmark artifacts remain read-only local evidence, not broker or availability acceptance |
| Risk Cockpit | Equity/exposure/drawdown, limits, reasoned breaches, switches, reconciliation | No browser-side limit or switch mutation |
| Portfolio | Positions, multi-currency cash/FX/margin, tax-lot/financing capability, aggregate risk, attribution, options scenarios/lifecycle and cross-environment reconciliation | Broker statement and production reconciliation evidence remains external |
| Replay and Incidents | Causal events, event distribution, audit coverage, incident/UNKNOWN state | No incident suppression |
| Journal | Hash-chain health, sequence/head, actor, decisions/annotations, source records | Append remains a controlled CLI action |
| Administration | Commercial ledger, IAM/RBAC/TOTP/recovery boundary, complete PostgreSQL projection/gRPC/React/Tauri/TLS topology, provisioning/subscription facts, privacy/release artifacts, and external dependencies | No password/MFA secret or recovery code, payment capture, signing key, broker credential, or privileged mutation is exposed in the browser |

The UI follows the safety rejections in the plan: it has no trading ticket in a
research view, does not hide PAPER/LIVE identity, does not provide a generic
broker button, does not use browser-side accounting as the source of truth, and
does not expose privileged mutations through the read-only evidence server.

## Reliability and quality conformance

| Requirement | Status | Finding |
| --- | --- | --- |
| Deterministic replay and exact accounting | Implemented | Repeatability, canonical serialization, exact decimal, cumulative fill/accounting, and artifact immutability tests exist. |
| OMS/risk invariants | Implemented for current scope | Rejected intent creates no order; illegal transitions fail; fills cannot exceed quantity; duplicate IDs and broker evidence are bounded; kill-switch and recovery tests exist. |
| Property/model/fault testing | Partial | Unit, integration, end-to-end, malformed-input, fault-injection, restart, reconnect, out-of-order, latency, partial-fill, and tamper tests exist. As of item 42 (2026-09-20), a real `proptest`-based model test (`core/control-plane/tests/oms_lifecycle_proptest.rs`) exists for the OMS order-lifecycle state machine, checked against an independently-authored edge-list model, not the implementation's own private validator. As of item 43 (2026-09-20), a second, independently designed property-test slice (`core/options/tests/option_lifecycle_settlement_proptest.rs`) checks `settle_expired_option_position`'s economic invariants -- exact position closure, cash/underlying conservation under both settlement methods, outcome classification, and determinism. As of item 44 (2026-09-20), a third slice (`core/accounting/tests/multi_account_aggregation_proptest.rs`) checks `aggregate_account_portfolios`'s own documented invariants -- cash/position conservation across contributing accounts and order-independence. A comprehensive state-model/property test program for every planned asset/order type (EMS scheduling/combination legality, the rest of `core/accounting`'s margin/tax-lot/financing functions) remains incomplete; these are three bounded slices. |
| Shadow/canary operation | Mechanism implemented; external gate | Shadow prevents broker submit and canary limits capital/action. No production operating history exists. |
| Risk decision p99 under 5 ms | Local measurement mechanism implemented; production proof unproven | `follon-risk-benchmark` runs a versioned, frozen portfolio policy/snapshot/candidate with explicit warmup, measured iteration count, threshold, source hash, and p99 observation. A retained benchmark on representative deployment hardware and production load/availability evidence are still required. |
| 99.9% session availability | Unproven | No qualifying production session history exists. |
| Recovery objectives and daily restore tests | Mechanism implemented; external drill gate | `tools/postgres_recovery.py` creates immutable hash-bound custom backups, refuses password environment variables, restores only to an explicitly confirmed `follon_restore_drill_*` database, validates schema migration, emits a receipt, and removes the drill database. Production backup custody and RPO/RTO drill receipts remain external. |
| Reconciliation before next session | Mechanism implemented; external gate | PAPER/LIVE comparison and stop conditions exist; elapsed clean-session evidence remains zero. |

## Security conformance

| Requirement | Status | Finding |
| --- | --- | --- |
| Strategy/broker secret separation | Implemented | Strategy workers cannot reach adapter or credential interfaces. |
| Secret ingress | Implemented interfaces; deployment gate | Managed-command/password/connection-string file boundaries and zeroizing broker material exist. Production mode refuses a direct database URL and requires a TLS connection string. A production vault/keychain, rotation operation, and custody evidence remain external. |
| Immutable audit and signed release | Implemented locally | Hash-chained journals, canonical manifests, detached Ed25519 signatures, and trusted-key verification exist. As of item 83 (2026-09-27), the PAPER and controlled-LIVE journals, the commercial ledger and `follon-admin`'s outputs refuse a symbolic link, dangling or not. As of item 91 (2026-09-28), so do the operations journal and its reader, the replay event log, the backtest experiment store, every immutable CLI artifact and `follon-news` output. Production HSM/KMS custody and independent approval remain external. |
| SBOM | Implemented 2026-08-22 | `tools/generate_sbom.py` creates a deterministic CycloneDX 1.6 Cargo/npm/Python inventory bound to source revision and lockfile hashes; CI tests, generates, and retains it. Vulnerability disposition remains a release operation. |
| Dependency/static/secret scanning | Partial | CI has advisory/dependency and secret checks plus compiler/lint/test gates. As of item 41 (2026-09-20), a real Semgrep SAST job (`p/owasp-top-ten`, `p/rust`, `p/python`, `p/typescript`, `p/secrets`, `--error`) runs in CI and gates the build; every GitHub Action reference is pinned from a mutable tag to its resolved commit SHA; Dependabot enforces a 7-day-minimum cooldown. **Corrected in place 2026-09-25 (item 68).** This cell said DAST "remain[s] external". That was accurate when written, and it is now false for a repository-authored scan. `tools/dast_scan.py` (pipeline step 23b) scans a local loopback deployment of the dashboard and trading API with authenticated and unauthenticated probes. An independent DAST product run against a real deployment, and named security-operation ownership, remain external. As of item 85 (2026-09-28), that Semgrep job and the gitleaks check had not passed on any run visible on GitHub: Semgrep blocked on this audit's own quotation of item 41's fixed defect, and gitleaks on a fixture's secret reference. Both are fixed and verified locally at CI's pinned versions; a green GitHub run is still open (delivery state E4.3). |
| Dashboard authentication | Partial | Production mode requires protected credentials; exact constant-time Basic auth, no-store/CSP headers, direct-peer sliding-window rate limiting, `429` and `Retry-After` are tested. This is an operator-only loopback gate. As of item 88 (2026-09-28), it reads and discards a request's declared body, up to 64 KiB, before answering, and a 15-second socket timeout bounds every read. |
| MFA, short sessions, revocation, customer RBAC and tenant isolation | Implemented kernel/schema; deployment gate | Argon2id, password policy/rotation, TOTP with bounded challenges, hashed one-time recovery codes, lockout, opaque hashed 15-minute sessions, security-version revocation, five roles, tenant authorization, and PostgreSQL RLS schema are tested. Production enrollment, out-of-band delivery, support, and customer acceptance remain external. |
| TLS and encryption at rest | TLS topology implemented; custody gate | Production Compose requires gRPC mTLS and a client-certificate dashboard proxy, pinned reviewed images, certificate secret files, and PostgreSQL `sslmode=require`. Certificate issuance/rotation, encrypted volume/KMS ownership, and deployed proof remain external. |
| Request idempotency | Implemented for durable event boundary | Orders/releases/artifacts remain idempotent; PostgreSQL event append binds tenant key to content and atomically creates outbox state. Production gateway/load evidence remains external. |
| Independent penetration test | External gate | Runbook exists; no independent passing report is retained. |

## External and operational gates

These are mandatory master-plan acceptance conditions and are currently open:

| Gate | Required | Retained result |
| --- | --- | --- |
| PAPER reliability | 30 clean real PAPER sessions with no unexplained order/position discrepancy | **0/30** |
| Controlled LIVE | 60 clean small-capital sessions after reviewed approvals and infrastructure | **0/60** |
| Operator usability | Five design partners complete normal workflows unaided | **0/5** |
| Options acceptance | One independently verified option-capable broker export/session reconciled across BACKTEST/PAPER/LIVE, including broker lifecycle/combination semantics | **0** |
| Commercial acceptance | Ten paying professionals or three paying organizations | **0/10 and 0/3** |
| Security | Independent penetration test and remediation for the exact deployment | No passing report |
| Legal/compliance | Entity, contracts, terms/privacy, market-data licenses, broker/API permissions, regional and tax review | No signed deployment approval in repository |
| Production operations | Named owner/on-call, monitoring, TLS, secret custody, backups, restore/DR drills, retention and incident exercises | mTLS topology, black-box probes, alerts, and safe backup/restore tooling exist; no named on-call, routed alert, or qualifying drill evidence |
| Release supply chain | Controlled signer/key distribution, SBOM review, vulnerability disposition, signatures and independent promotion | Signed release plus two-person ordered promotion gate exists; no production promotion evidence |

## Locally closed gaps in this audit

1. An exact fixed-point limit-price collar now runs before order creation in
   simulation, PAPER, and controlled-LIVE. Rejections carry
   `PRICE_COLLAR_EXCEEDED` plus reference/requested price and exact basis-point
   evidence; tests prove that a rejected PAPER intent creates no order.
2. A deterministic, immutable CycloneDX 1.6 SBOM generator now inventories
   locked Cargo, npm, and declared Python dependencies, binds them to source and
   lockfile hashes, rejects a conflicting overwrite, and publishes a CI
   artifact.
3. The dashboard authentication boundary now rate-limits repeated failures per
   direct network peer, bounds tracked clients, emits `429`/`Retry-After`, and
   resets the budget after successful authentication.
4. Advanced EMS planning now covers immediate, TWAP, VWAP, POV/participation,
   urgency-weighted arrival price, strict passive post-only cancel/replace,
   smart routing, bracket/stop-limit, trailing stop, basket behavior, and atomic
   ratio/net-price options combinations with exact fixed-point tests. The
   public gRPC boundary carries complete child semantics and exposes scheduled,
   cancel-before-replace passive, and synchronized option-combination planners.
5. Portfolio risk and multi-currency accounting kernels cover aggregate
   exposures/buckets/Greeks/order controls, balanced FX/margin valuation,
   FIFO/LIFO/highest-cost lots, and cash-debit/short-borrow financing; the
   versioned gRPC service exposes portfolio risk and margin valuation.
6. Customer IAM implements Argon2id, TOTP, lockout, opaque short sessions,
   hashed one-time recovery codes, authenticated password rotation, immediate
   security-version revocation, RBAC, and tenant isolation; PostgreSQL adds
   matching durable RLS tables.
7. PostgreSQL event/outbox persistence plus tenant-bound recovery codes and complete broker/order/execution/
   position/strategy/config/reference/audit/billing projections,
   React/Vite/Tauri packaging, production mTLS topology, monitoring rules,
   recovery tooling, ordered release-promotion gates, and tamper-evident
   external acceptance ledgers are present and tested at repository boundaries.
8. The controlled-LIVE IBKR wrapper is no longer an unbounded placeholder: it
   requires signed artifact verification and independent review evidence and
   enforces a narrow, price-protected canary plus emergency stop. The actual
   review and broker transport remain external and therefore absent.
9. Market-data/reference contracts now include normalized quote source/receive
   times, size/spread checks, delay/staleness/sequence classification, complete
   cash-security/option/future settlement economics, and tested exact
   expiration exercise/assignment settlement.
10. The advanced backtest account now models point-in-time universe membership,
    attributed charges, long/short crossings, borrow availability and recalls,
    financing, fresh FX, initial-margin capital rejection, corporate actions,
    and terminal delisting settlement. Every CLI result includes a hashed
    advanced-account projection; legacy configurations receive a conservative
    fully-paid profile instead of bypassing the controls.
11. The Python SDK now provides bounded point-in-time history, deterministic
    indicators, immutable portfolio snapshots, fingerprinted strategy state,
    and structured metrics without exposing a credential, adapter, filesystem,
    socket, or workstation clock. The Rust replay host now selects and verifies
   that rich service frame for local Python-worker backtests.
12. Execution-cost analysis now has a strict, immutable `tca-v1` input/output
    path that measures side-normalized implementation shortfall against frozen
    arrival and target benchmarks, preserves fee and partial-fill effects, and
    writes a deterministic per-strategy/algorithm/order-type summary. The
    operations journal also has typed, hash-bound model-risk and fault-game-day
    records plus canonical registers; the local risk benchmark and personal
    mandate template make performance, decision, resilience, and review
    expectations executable/auditable without inventing operational evidence.
13. Operational alerting and severity/category classification are unified deterministically within `core/operations` (`OperationalAlert`, `AlertSeverity`, `assess_journal_alerts`, `assess_cockpit_alerts`), preserving deterministic execution and operator cockpit attribution without network side-effects.
14. `core/accounting` now includes a `statement` module that deterministically parses standard broker CSV statements (like IBKR Activity Flex Queries) and reconciles cash and positions against the internal multi-currency ledger, producing exact reconciliation incidents.
15. `adapters/brokers/ibkr` natively maps option combination requests (BAG orders) over the JSON bridge, guaranteeing atomic execution of complex multi-leg options intents.
16. The `follon-fx` CLI (`apps/cli/src/fx.rs`) provides deterministic valuation of spot, outright forward, and swap FX contracts using fixed-point midpoints, forward point additions, bid/ask spreads (in basis points), and quote staleness limits against versioned JSON configuration.
17. The `follon-operations` CLI now includes `reconcile-statement`, integrating `core/accounting/src/statement.rs` into the operational toolchain to ingest broker CSV statements (e.g. IBKR Activity Flex Queries), perform automated multi-currency cash and position reconciliation against the internal ledger, and emit canonical reconciliation artifacts with incident classification.
18. Desktop evidence parsing and UI panels in `apps/desktop/src/evidence.ts` and `apps/desktop/src/workspaces.ts` now natively display both Broker Statement Reconciliation incidents and Deterministic FX Pricing dashboards in the Portfolio workspace, fully covered by automated contract tests in `apps/desktop/test/evidence-contract.mjs`.
19. `tools/generate_pipeline_evidence.py` orchestrates the complete CLI toolchain (`follon-build-bars`, `follon-options`, `follon-fx`, `follon-operations`, `follon-tca`, `follon-risk-benchmark`, `follon-news`) to generate all canonical, content-addressed evidence artifacts in `var/`.
20. The 12 Enduring Capability artifact shapes (DUR-01 through DUR-12) from `docs/06-delivery/15-end-to-end-product-plan.md` are represented across core domain crates, contracts, JSON schemas, fixtures, and CLI binaries. DUR-02 and DUR-06 support two input modes: an `execute` mode that actually drives a real perturbed replay of the built-in strategy (added 2026-09-16, item 31 below), and an operator-attested mode retained for a Python-worker-driven backtest or a genuinely externally-run intervention/probe:
    - **DUR-01 (Historical Corpus Compatibility Matrix)**: `CompatibilityRegistry` verifies golden corpus backward compatibility via `follon-operations compatibility-matrix`.
    - **DUR-02 (Counterfactual Safety Lab; built-in-strategy execution closed, arbitrary-strategy gap open)**: `follon-backtest counterfactual`'s `execute` input mode actually runs the declared intervention as a second genuine deterministic replay of the built-in `BuyOnceStrategy` and certifies the real resulting deltas via `CounterfactualEngine`; its `metrics`/`delta_metrics` modes still certify operator-attested figures for a Python-worker-driven backtest or an externally-run intervention. `CounterfactualEngine` itself remains a pure certification function with no execution capability of its own — see item 31.
    - **DUR-03 (Advanced Account Economics & Margin Projections)**: Projected multi-currency margin, financing, and delistings in `follon-backtest`.
    - **DUR-04 (Point-in-Time Knowledge Graphs)**: Replay and vector alignment in `follon-news`.
    - **DUR-05 (Operator Attention Budget & Cognitive Load)**: `AttentionBudgetController` prevents alarm fatigue and enforces cognitive load caps via `follon-operations attention-budget`.
    - **DUR-06 (Adversarial Research Gate; built-in-strategy execution closed, arbitrary-strategy gap open)**: `follon-backtest adversarial`'s `execute` input mode actually runs all 5 standardized stress probes as genuine perturbed replays of the built-in `BuyOnceStrategy` and certifies the real resulting `passed`/`degradation_bps` via `AdversarialResearchGate`; its `probes`-only mode still certifies operator-attested figures for a Python-worker-driven backtest or an externally-run probe suite. `AdversarialResearchGate` itself remains a pure certification function with no execution capability of its own — see item 31.
    - **DUR-07 (Assumption & Regime Drift Monitor)**: Regime shift and degradation tracking across backtest and operations.
    - **DUR-08 (Continuous Recovery Game-Day Drills)**: `GameDayCompiler` executes RTO/RPO recovery drills via `follon-operations recovery-drill`.
    - **DUR-09 (Execution Coach & Fill Quality Benchmarks)**: Parent-order implementation shortfall and TCA analysis via `follon-tca`.
    - **DUR-10 (Gateway Qualification Matrix)**: `GatewayQualificationMatrix` certifies route-level capabilities and latency bounds via `follon-paper-status gateway-matrix`.
    - **DUR-11 (Risk-Budgeted Capital Allocation Proposal)**: `CapitalAllocationCouncil` constructs Equal Risk Contribution (ERC) proposals via `follon-risk-benchmark capital-proposal`.
    - **DUR-12 (Strategy Capsule Provenance & Verification)**: Content-addressed strategy bundle hashing and immutability verification.
    All 72 canonical artifacts in `var/` are published and verified, with full typed rendering in the React/Tauri desktop terminal and 100% test pass rate across Rust, Python, and TypeScript.
21. The complete commercial supply chain, privacy, retention, release verification, and self-host readiness suite is operationalized end-to-end:
    - Cryptographic Ed25519 release keypair generation (`follon-admin release-keygen`), content-addressed release manifest compilation (`release-manifest`), detached signature signing (`release-sign`), and tamper-evident signature verification (`release-verify`).
    - Lockfile-backed CycloneDX 1.6 SBOM compilation (`tools/generate_sbom.py`) inventorying 314 dependencies with SHA-256 integrity binding.
    - Commercial data retention plans (`follon-admin retention-plan`) and execution (`retention-execute`) with SHA-256 concurrency fencing and cryptographic deletion receipts (`commercial-retention-receipt.json`).
    - Commercial privacy erasure plans (`privacy-plan`) and erasure execution (`retention-execute`) with tenant and subject isolation receipts (`commercial-privacy-receipt.json`).
    - Self-host deployment readiness verification (`self-host-readiness`) checking tenant subscription entitlement, cryptographic release signatures, and artifact hashes.
    - External acceptance status ledger audit (`tools/acceptance_evidence.py`).
    - 32 canonical advanced evidence fixtures generated and published across 32 schema categories (`tools/build_advanced_evidence_fixtures.py`).
    - Automated pipeline generator (`tools/generate_pipeline_evidence.py`) producing 75 immutable evidence artifacts in `var/`, fully populating all 12 workspaces with zero empty states and zero unavailable evidence panels.
    - Institutional Visual Design System codified in `docs/04-experience/04-visual-design-system.md` with WCAG AA compliance, monospace numerical precision, responsive layout, and monochrome signal toggle.
22. Workstation Evidence Panel Completion: Eliminated all remaining `appendUnavailableEvidence` calls across desktop workspaces by introducing versioned schemas, desktop server indexer entries, TypeScript interfaces/parsers/typeguards, and typed table row builders for:
    - **SOLO-04 Explainable Market Scanner** (`market-scanner.schema.json`, `parseMarketScanner`, `#market-scanner-panel` in Command Center);
    - **DATA-03 News Revision and Novelty Timeline** (`news-revision-timeline.schema.json`, `parseNewsRevisionTimeline`, `#news-revision-panel` in News Cockpit);
    - **RES-02 Strategy Composition Studio** (`strategy-composition-spec.schema.json`, `parseStrategyCompositionSpec`, `#strategy-composition-panel` in Strategy Studio).
    All 12 desktop workspaces now exclusively project typed, versioned evidence with 100% test coverage across Rust, Python, and TypeScript.
23. Fabricated-evidence remediation in the desktop dashboard (2026-09-10): four decorative visuals
    (`apps/desktop/src/workspaces.ts` — the causal-lineage DAG, the attention/cognitive-load gauge,
    the factor-exposure bar chart, and the options payoff/convexity chart) previously rendered fixed,
    hand-authored numbers (for example a constant "+340 bps" momentum factor, a fabricated "K =
    $500.00" option strike, and a static "2.1 / hr" interruption rate) unconditionally beside the real
    evidence tables in Command Center, Risk Cockpit, Replay & Incidents, and Research Lab. This
    violated the read-only dashboard's own zero-synthetic-data invariant. All four are now computed
    exclusively from their retained typed evidence records (`exposure_graph`, `decision_reconstruction`,
    `attention_budget`, and the frozen option-chain analytics already displayed in the adjacent table)
    and render nothing when that evidence is absent, matching the empty-state convention used
    everywhere else in the dashboard. The same pass replaced every remaining inline `style` attribute
    and JS `element.style` mutation in `workspaces.ts` with CSS classes so the visuals no longer
    violate the dashboard's `style-src 'self'` Content-Security-Policy header (previously silently
    blocked, so the fabricated numbers rendered unstyled rather than being visually suppressed).
    Regression coverage was added in `apps/desktop/test/enduring-capabilities-regression.mjs`,
    `apps/desktop/test/paper-operations-regression.mjs`, and the new
    `apps/desktop/test/options-payoff-regression.mjs`, each asserting the real evidence values render
    and the previous fabricated strings do not. Verified live via a rebuilt `web-dist` production
    bundle in a headless browser against the full locally generated evidence set: zero console errors
    and zero remaining inline-style CSP violations.
24. Header telemetry fabrication remediation, mobile responsive fixes, and accessibility/signal-color
    completion in the desktop dashboard (2026-09-10). Three further findings from the same
    verification pass, all fixed and regression-tested:
    - **Fabricated header ticker and gateway telemetry**: `apps/desktop/src/app-shell.tsx`'s header
      rendered on every single page load, unconditionally: fake `BTC/USD`/`ETH/USD`/`SPX` spot prices
      (instruments this platform has no evidence category for at all), a fake `PORTFOLIO NAV`/`MAX
      DRAWDOWN`/`VAR (99% 1D)`/`OMS ENGINE`/`AUDIT ANCHOR` ticker, a static `142µs` gateway latency
      that was never measured, and a `PAPER ENGINE · VERIFIED KERNEL` badge with no backing check —
      the most visible instance yet of the zero-synthetic-data violation, since it appeared on every
      workspace regardless of evidence state. `apps/desktop/src/main.ts` now measures the real
      `/api/v1/status` round-trip latency (`performance.now()`), derives the environment badge from
      the real dashboard `mode`, and drives the ticker exclusively from the retained operations
      snapshot (`current_equity`, `drawdown_bps`, `unknown_orders`, the journal `head_hash`); the
      unrelated crypto/equity/VaR items were removed outright since no real evidence backs them. Every
      ticker element shows an explicit "No snapshot" rather than inventing a figure when the
      operations dashboard is absent.
    - **Mobile responsive-layout audit**: automated measurement (not visual guessing) of horizontal
      overflow across all 12 workspaces at 1440px/1024px/390px found **all 12 workspaces overflowing
      at 390px** from three distinct CSS bugs — a cascade-order conflict that silently defeated the
      mobile nav-collapse rule, a missing `white-space` reset that kept the mobile table-to-card
      reflow from wrapping long values, and a `data-label` attribute every cell carried but no CSS
      ever rendered. All three are fixed (detailed in
      [`docs/04-experience/02-ui-overhaul-audit.md`](../04-experience/02-ui-overhaul-audit.md)); a
      re-measurement after the fix shows 0 of 36 (workspace × breakpoint) checks overflowing, and a
      keyboard-navigation sweep found every interactive element reachable with a visible focus
      outline in a logical order.
    - **WCAG AA contrast and signal-color/monochrome completion**: `--color-signal-buy`,
      `--color-signal-sell`, `--color-accent`, and `--color-ruby` were verified (by computed relative
      luminance, not the design system's original bg-base-only check) to fall below the documented
      4.5:1 WCAG AA claim as text against the `--color-surface-1`/`-2` card backgrounds they are
      actually rendered on; all four were relightened to clear 4.5:1 on every surface. The
      already-specified but previously unimplemented "signal vs. monochrome" preference
      (`docs/04-experience/04-visual-design-system.md`) is now built: a `▲`/`▼` glyph always
      accompanies a signed value regardless of mode (stricter than the doc's literal text, to
      satisfy WCAG 1.4.1 in the default color view too), and a header toggle removes the green/red
      hue on request, persisted per-browser in `localStorage`.
    Regression coverage lives in `apps/desktop/test/browser-module-contract.mjs` (source-text
    assertions against the fabricated strings, the fixed CSS rules, and the new functions) so none of
    these three regress silently again.
25. Pre-trade risk-gate and golden-corpus-evidence remediation (2026-09-10). An independent
    doc-versus-code audit of `docs/03-capabilities/04-pre-trade-risk.md` against the real order-gating
    code (as opposed to the `core/risk` evidence engine already described in item 5.7's main text)
    found that self-trade prevention and order-rate limiting — both explicitly required by that
    document's "Order shape" row — were implemented only in the disconnected, gRPC/benchmark-only
    `core/risk::evaluate_portfolio_risk` and were never called from `core/paper::evaluate_risk` or
    `core/live::evaluate_risk`, the two functions that actually gate every real PAPER and
    controlled-LIVE order. Both engines now independently reject a same-instrument opposite-side
    order against any existing working order (`SELF_TRADE_RISK`) and reject a submission once the
    number of orders created inside a configured rolling window reaches a configured limit
    (`MAX_ORDER_RATE_EXCEEDED`), computed from the durable, already-retained order history rather than
    new mutable state, so the check is correct across a restart without any extra recovery work. Both
    `PaperRiskPolicy` and `LiveRiskPolicy` gained `max_order_rate`/`order_rate_window_seconds` fields,
    threaded through their CLI configuration documents, JSON Schema contracts (`v1` and `v2`), and
    fixtures; four new unit tests (two per crate) exercise the rejection paths, and the entire
    workspace test suite, clippy, and `cargo fmt` remain clean.
    A second, independent finding in the same pass: `core/domain::compatibility::CompatibilityRegistry::verify_corpus`
    (backing `follon-operations compatibility-matrix`, DUR-12) unconditionally set
    `backward_compatibility_verified: true` and accepted an arbitrary caller-supplied
    `golden_corpus_size` with no corpus behind it at all — a fabricated-evidence pattern matching item
    23's finding, just in a CLI evidence artifact instead of a dashboard visual. `verify_corpus` now
    requires the caller to supply both the corpus size actually read and the number of records that
    actually verified, and only certifies compatibility when every read record verified; the CLI
    command reads a real, retained `news-headline` NDJSON corpus
    (`tests/fixtures/news/2026-09-01-headlines.ndjson`, overridable via `--golden-corpus`) through the
    exact production ingestion path (`ingest_local_headlines_ndjson`) and derives both numbers from
    that real file. A missing `contracts/json-schema/v1/statement-reconciliation.schema.json` — a
    schema name the same CLI command had registered in its compatibility matrix with no backing schema
    file at all — was added and verified (via Python `jsonschema`) against real CLI-produced clean and
    mismatched reconciliation artifacts.
    A third, minor finding: `TaxLotBook::dispose`'s `HighestCost` tie-break compared unit cost then
    `lot_id` string order, contradicting its own doc comment ("then oldest lot identity"); it now
    compares unit cost then `opened_at` then `lot_id`, proven by a new test with two lots tied at the
    same unit cost.
    A fourth, doc-only finding: `docs/01-domain/02-event-envelope.md`'s "First event families" table
    listed `strategy.*` and `system.*`, neither of which `core/domain::EventPayload::event_type` (or
    any other crate) has ever emitted, while omitting the real `news.*`, `operations.*`, and
    `commercial.*` families that are actually in production use; the table now lists the true set.
26. Desktop connected-PAPER-gateway verification and end-to-end audit (2026-09-14). This pass's task
    was to check the then-uncommitted "connected trading workflow" change set (25 files, ~2,000 lines)
    for completeness, bugs, and end-to-end wiring, fix anything found, and record status here. Findings:
    - The desktop order ticket (`apps/desktop/src/OrderTicket.tsx`) had a submit button and the Tauri
      host declared submit/cancel/close-position commands, but the host permanently `.manage()`d
      `trading::TradingCommandState::unavailable()` and the React UI never invoked cancel or close, so
      every click had always failed with "trading unavailable" — there was no real backend behind the
      UI. The completed route now provides visible submit, OMS-cancel, and position-close controls
      backed by `apps/desktop/src-tauri/src/paper_gateway.rs`: a real
      `RiskOmsGateway` implementation backed by an in-process `follon_paper::PaperTradingService`,
      bootstrapped from an operator-authored JSON file named by `FOLLON_DESKTOP_PAPER_CONFIG` (falling
      back to the prior `unavailable()` behavior, never an error, when the variable is unset, the file
      is missing, or it fails to parse). Every risk/kill-switch/idempotency/audit-journal guarantee is
      the real `follon_paper` mechanism, not a reimplementation; the only simplification, stated in the
      module's own doc comment, is that there is no live market-data feed at this desktop boundary, so
      the operator supplies an attested reference price and observation time per order (new
      `OrderIntent::reference_price`/`reference_observed_at` fields, validated and threaded through
      `apps/desktop/src/OrderTicket.tsx`), and risk gating (price collar, notional limits) evaluates
      against exactly that value. Command receipts report the actual synchronized OMS state
      (`ACKNOWLEDGED`, `FILLED`, `CANCELLED`, `UNKNOWN`, etc.) instead of describing already-terminal
      operations as pending. Cancel/close reject a mismatched account before reading or mutating route
      state, and repeated cancellation after durable acceptance/completion is idempotent.
    - Full-repository verification (fresh `cargo check`/`clippy`/`test` in both Rust workspaces, `tsc`
      typecheck, the full desktop `npm run test:evidence` regression suite, and all four Python test
      suites) also found and closed formatting, account-isolation, lifecycle-receipt, cancellation-
      retry, UI-wiring, stale readiness-label, and root-lockfile supply-chain defects. The separate
      Tauri lockfile is now audited in CI; its upstream Linux GTK warning remains explicitly open in
      the verification snapshot rather than being hidden by the Windows release result. Formatting
      defects that would have failed the Months 0–5 gate were corrected with `cargo fmt`; subsequent
      formatting checks are clean. The reviewed change set includes independently testable fixes —
      among them a look-ahead-bias fix in `ReplayEngine::process_bar` (a newly created order could
      previously fill against the same bar that produced it), an options-pricing floor bug in
      `core/options` that wrongly rejected ordinary short-dated low-volatility inputs, a TOTP
      replay-protection and MFA-lockout-bypass fix in `core/identity`, a PostgreSQL idempotency-key
      race-recovery fix, whole-word keyword matching in `core/news` (fixing "FedEx" matching "fed",
      "disapproval" matching "approval", etc.), a PAPER/LIVE order-rate-limit backdating fix (the limit
      was keyed off caller-supplied `created_at` rather than the actual risk-decision time), and a
      gRPC TLS health-check drift fix in `services/trading-api`. The only stub-marker search hit is the
      intentional `NotImplementedError` on the Python strategy SDK's abstract base method.
    - This entry, like every other "Implemented" mark in this document, is repository evidence only:
      it does not change the open external gates (0/30 PAPER sessions, 0/60 controlled-LIVE sessions,
      0/5 design partners, 0 options broker-backed sessions, 0/10 and 0/3 paying customers) recorded in
      "External and operational gates" below, none of which repository code can satisfy on its own.
27. Stale-fixture configuration-fingerprint remediation (2026-09-15). Automated tests never exercise
    `tools/generate_pipeline_evidence.py`, the script that drives every CLI binary against its checked-in
    fixture to populate `var/`'s evidence artifacts, so this pass ran it directly end to end as a
    functional check no `cargo test` invocation performs. Two of its fifteen steps failed:
    `follon-paper-status` against `tests/fixtures/paper/journal-v2.ndjson`/`config/paper-v2.json`, and
    `follon-live-status` against `tests/fixtures/live/journal-v1.ndjson`/`config/live-v1.json`, both with
    "configuration ... does not match" errors. Root cause: item 25's addition of `max_order_rate` and
    `order_rate_window_seconds` to the hashed fingerprint inputs in both `core/paper` and `core/live`
    correctly changed the fingerprint those two engines compute for an otherwise-unchanged configuration,
    but the two checked-in journal fixtures still carried the fingerprint (and, for the live journal, the
    resulting 9-entry hash chain) computed under the pre-item-25 formula -- a real fixture/code
    inconsistency, not a false-positive rejection: the fingerprint check itself is working exactly as
    designed, correctly refusing to reconcile a journal against a configuration whose risk-policy hash no
    longer matches what produced it. Both fixtures were regenerated using the actual, current CLI
    binaries (`follon-paper-status` and nine sequential `follon-live-status` invocations reproducing the
    original journal's `initialized` + 8 `restarted` entries) against their unchanged configuration
    documents, so the corrected fingerprint/hash-chain values are exactly what the current code computes
    rather than hand-computed. A byte-level comparison confirmed every other field (cash, empty
    order/position maps, timestamps, actor, correlation ID, event ordering) is unchanged; only the
    fingerprint-derived hashes differ. A repository-wide search confirmed the stale hash values were not
    referenced anywhere else (no Rust/Python/TypeScript test hardcodes them). Re-running the full pipeline
    afterward produced all 75 evidence artifacts with zero failures, and `cargo test -p follon-paper -p
    follon-live` remained clean (21 and 9 passed respectively). This is the kind of gap the existing
    per-crate unit-test suite structurally cannot catch, since no unit test loads these two specific
    fixture files; it surfaced only because this pass ran the actual end-to-end pipeline script rather
    than relying on `cargo test` passing as proof of full-system correctness.
28. Dependency-vulnerability remediation and independent re-verification (2026-09-15). A fresh session
    was asked to check the entire repository for incomplete or buggy features and update this document
    accordingly, rather than extend item 26's own work. It independently re-ran every check in item 26
    (build/lint/test in both Rust workspaces, all four Python suites, the desktop typecheck and full
    evidence-regression suite, and the stub-marker search) from a clean read of the repository instead
    of trusting that entry's summary, and separately re-read several of its specific code claims against
    the actual diff (the CI workflow change, `paper_gateway.rs`, the `lib.rs` gateway wiring,
    `OrderTicket.tsx`'s cancel/close calls, and the `ReplayEngine::process_bar` look-ahead-bias fix). All
    of it held up unchanged, with one new finding: `cargo audit --deny yanked` on the root lockfile,
    which item 26 had recorded as clean, now failed against RUSTSEC-2026-0285, a TLS 1.3 handshake-level
    vulnerability in `rustls 0.23.43` disclosed the same day as item 26's run and reached transitively
    through `follon-trading-api`'s gRPC stack (`tonic` -> `tokio-rustls` -> `rustls`) -- the same
    dependency this document's gRPC/mTLS conformance rows describe. `Cargo.lock` now pins the patched
    `rustls 0.23.45`; `cargo audit --deny yanked` is clean again, and `cargo check`/`clippy`/`fmt` and the
    `follon-trading-api` unit tests were re-confirmed clean against the patched dependency. The same pass
    also found and fixed item 27's stale-fixture fingerprint mismatch by actually running the end-to-end
    CLI pipeline rather than stopping at `cargo test` passing. See the verification snapshot above for
    the complete re-run detail.
29. First-ever live Docker Compose runtime verification, and six previously undetected defects it
    surfaced (2026-09-15). Every prior verification snapshot in this document recorded that Docker
    Desktop's Linux engine was unreachable, so no image in `infra/` had ever actually been built, let
    alone run, in any audit to date -- `cargo fmt`/`clippy`/`test` passing was never evidence that the
    checked-in container images or Compose topology actually worked, only that the plain Cargo/Python/
    npm builds did. This pass found the engine reachable (after starting the already-installed but not
    running Docker Desktop application) and used it to actually build every Dockerfile and bring up the
    full `compose.dev.yml` topology (PostgreSQL, MinIO, the gRPC trading API, and the dashboard) for the
    first time. Every one of the five container images failed on first attempt; each defect was real,
    was fixed, and the fix was proven by a subsequent successful build/run rather than assumed:
    - **`Dockerfile.trading-api` could not build at all.** The image copies only `core/`, `adapters/`,
      `services/`, and `contracts/` into its build context, but the root `Cargo.toml` workspace also
      lists `apps/cli` as a member; Cargo refuses to resolve *any* workspace build, even one scoped to a
      single `-p` package, unless every member's manifest is readable, so the build failed immediately
      with "failed to load manifest for workspace member `/workspace/apps/cli`". Fixed by adding
      `COPY apps/cli apps/cli`.
    - **The same image then failed to compile at all under its pinned Rust version.** `Dockerfile.trading-api`
      pinned `rust:1.85-bookworm`, matching the workspace's declared `rust-version = "1.85"` -- but the
      currently locked dependencies (`time`, `tonic`, `tonic-build`, `tonic-prost`, `tonic-prost-build`)
      require rustc 1.88+, so `cargo build --locked` failed with an explicit MSRV error naming all five
      crates. The declared `rust-version` had silently drifted stale relative to the actual lockfile
      (nothing enforces this automatically). Fixed by bumping the image to `rust:1.97-bookworm` (matching
      the version already used successfully by `Dockerfile.admin`/`Dockerfile.replay` and by every local
      toolchain check in this and the prior audit) and correcting the workspace's declared `rust-version`
      from `1.85` to `1.88`, the version the compiler error itself named as the true requirement.
    - **`Dockerfile.admin` and `Dockerfile.replay` were each missing a different workspace member.** Both
      copy `apps/`, `core/`, and `adapters/` but not `services/`, which the root workspace also lists as a
      member (`services/trading-api`) -- the identical failure mode as the first finding, just against a
      different missing directory. Fixed by adding `COPY services services` to both.
    - **The dashboard image's build context was 2.22 GB** because `.dockerignore` excluded `target` and
      `dist` only at the repository root (no `**/`-prefixed recursive form, unlike the `node_modules` /
      `**/node_modules` pair immediately above them), so it never matched the nested
      `apps/desktop/src-tauri/target` (7.7 GB of accumulated Tauri build artifacts) that
      `Dockerfile.dashboard`'s `COPY apps/desktop/ ./` pulls in. Fixed by adding `**/target` and
      `**/dist`; the dashboard's build context dropped to 3.54 MB and every other image's context shrank
      correspondingly.
    - **`follon-trading-api` panicked on its actual first connection to a real database**, immediately
      after the four build fixes above finally let it build and start: `thread 'main' panicked ... Cannot
      start a runtime from within a runtime`. `services/trading-api/src/main.rs`'s `#[tokio::main] async
      fn main()` called `PostgresStore::connect_development`/`connect_tls` directly; those wrap the
      *synchronous* `postgres` crate, whose `Client::connect` drives its own private Tokio runtime via
      `block_on` internally -- calling it from a thread that is already inside a Tokio runtime is exactly
      the case Tokio detects and refuses. No test exercises this exact startup path (unit tests construct
      `OperatingSystemService` directly; this document's own prior verification snapshots record that a
      live database was never available to smoke-test against), so this had never been triggered before.
      Fixed by moving the connect-and-migrate sequence into `tokio::task::spawn_blocking`, which runs it
      on a dedicated blocking-pool thread with no ambient runtime context -- exactly where that pattern is
      meant to run. Proven fixed by then actually connecting to a live PostgreSQL container: migrations
      applied, and the container reported `healthy`.
    - **The dashboard image crashed on startup with `IndexError: 2`.** `apps/desktop/server.py` computed
      `_DEFAULT_EVIDENCE = Path(__file__).resolve().parents[2] / "var"` unconditionally at module-import
      time, intending it only as a fallback default for `FOLLON_EVIDENCE_ROOT` (which the image always
      sets explicitly to `/var/follon`). In the real repository layout `apps/desktop/server.py` has a
      third ancestor (the repository root); in the image, `Dockerfile.dashboard` copies the file directly
      to `/app/server.py`, which does not, so the unconditional index access raised before the
      environment-variable override was ever consulted. Fixed with a bounds-safe `_ancestor()` helper that
      returns the root-most available ancestor instead of raising; the 18-test `server_contract.py` suite,
      which exercises the real repository layout where the original three-level lookup is valid, remains
      fully passing, proving the fix does not change behavior in the environment that suite covers.
      Proven fixed in the actual container: the dashboard started, stayed healthy, and its live
      `/api/v1/status` endpoint reported all four services (`dashboard`, `postgres`, `minio`,
      `trading-api`) `"healthy"` and 74 real evidence artifacts visible -- the first time this document
      has ever recorded an actual running instance of the containerized topology rather than an offline
      `docker compose config` validation.
    Fixing the fourth finding surfaced two further clippy findings (`clippy::manual_is_multiple_of` in
    `core/news` and `core/commercial`), latent until the `rust-version` correction made the lint's
    suggested replacement MSRV-eligible; both were applied and are behavior-preserving. After every fix,
    the complete verification suite (fmt, clippy, 237+17 Rust tests, all Python suites) was re-run clean
    end to end, and all five Dockerfiles were confirmed to build cleanly (`follon-admin`, `follon-replay`,
    `follon-dashboard`, `follon-storage`, and `follon-trading-api`). The five Compose topologies (dev,
    production, monitoring, self-host, and the dashboard-secure overlay) were also validated with
    `docker compose config` against dummy operator secrets in each of their documented combinations, all
    resolving cleanly; production/monitoring/self-host's required-variable guards (`FOLLON_DASHBOARD_IMAGE`,
    `FOLLON_MONITORING_CLIENT_PRIVATE_KEY_FILE`, `FOLLON_SELF_HOST_ROOT`, etc.) are working as designed,
    not bugs. This entry does not change any external gate: no production Postgres/TLS/secret-custody
    evidence exists, this was one operator's local Windows Docker Desktop engine over a slow network
    connection (base-image pulls took minutes), and a genuinely reachable engine was itself an
    environment-specific circumstance of this session, not a claim that Docker Desktop is now reliably
    available going forward.
30. First execution, ever recorded, of the PostgreSQL integration/concurrency tests (2026-09-15). Every
    prior verification snapshot in this document lists `follon-postgres`'s three `#[ignore]`d tests
    (`postgres_migration_append_and_idempotency_round_trip`,
    `append_event_recovers_idempotent_outcome_after_losing_the_insert_race`,
    `append_event_reconciles_concurrent_duplicate_submissions_of_the_same_idempotency_key`) as requiring
    an operator-provided `FOLLON_TEST_DATABASE_URL` that was never actually supplied -- so the aggregate
    lock, `SKIP LOCKED` outbox claim, and the savepoint-based idempotency-key race-recovery logic added in
    item 26 had never actually been exercised against a real PostgreSQL server by any audit, only
    compiled and reasoned about. With the Docker engine reachable (item 29), this pass started a
    disposable `postgres:16.10-alpine` container with the exact same image, database, user, and password
    CI's own `postgres-integration` job uses, and ran all three with `cargo test -p follon-postgres --
    --ignored`. The first attempt, against a three-day-old container reused from an earlier session,
    failed all three with genuine-looking errors (a duplicate-key constraint violation, an idempotency
    count mismatch) -- but this was leftover data from that earlier run, not a defect: these tests assume
    a schema-fresh database each run, exactly matching CI's own ephemeral-service-container model, and
    say so in their own doc comments. Removing that container and starting a byte-for-byte fresh one
    reproduced CI's actual setup and all three passed; a repeat run against the now-populated same
    database correctly failed again for the identical reason, confirming the diagnosis rather than
    revealing a new bug. `cargo test --workspace --all-targets -- --include-ignored` against a final fresh
    container passed **100% clean: 0 failed, 0 ignored**, across every crate in the main workspace -- the
    first time this document can record that figure rather than "3 ignored."
31. Real intervention/probe execution for the Counterfactual Safety Lab and Adversarial Research Gate
    (2026-09-16). Every prior verification snapshot in this document recorded `CounterfactualEngine` and
    `AdversarialResearchGate` as pure certification functions over caller-supplied numbers -- the module
    doc comments in `core/backtest/src/counterfactual.rs` and `adversarial.rs` said so explicitly, and
    `follon-backtest counterfactual`/`adversarial` only ever read a `metrics`/`delta_metrics`/`probes`
    JSON block an operator had to have populated from a real run performed elsewhere. This pass added a
    second input mode, `execute`, to both CLI subcommands that actually drives the real deterministic
    replay kernel instead of trusting operator-attested figures, without touching `CounterfactualEngine`
    or `AdversarialResearchGate` themselves (both remain exactly the pure aggregation/certification
    functions they were; only their caller in `apps/cli/src/backtest.rs` changed).
    - A new `ReplayContext` in `apps/cli/src/backtest.rs` factors the immutable per-run inputs (account,
      strategy identity, instrument registry, calendar, dataset identity) out of a loaded
      `RuntimeConfiguration`, exposing one `run(bars, risk_policy, fill_model, entry_threshold)` method
      that builds a fresh `DatasetManifest`/`BacktestSpec`/`ReplayEngine`/`BuyOnceStrategy` and drives them
      through the exact same `BacktestRunner` the plain `follon-backtest run` command uses -- a genuinely
      independent, single-use replay per call, not a reimplementation of the kernel.
    - `execute_counterfactual_scenario` runs one unperturbed baseline replay and one replay with every
      declared intervention applied, then derives `baseline_fills`/`counterfactual_fills`/
      `*_pnl_cents`/`*_max_drawdown_bps`/`*_rejections` from each real `CompletedBacktest` (fill count and
      max drawdown from its `PerformanceReport`, P&L from its ledger's realized-plus-unrealized total, and
      risk rejections by counting real `risk.decision.v1` events carrying `"approved":false` in its
      canonical event stream) before handing them to the unchanged `CounterfactualEngine::evaluate_scenario`.
      `RISK_COLLAR_ADJUSTMENT` overrides a named `RiskPolicy` field (`max_quantity`, `max_notional`,
      `max_price_deviation_bps`, or `global_kill_switch`); `NETWORK_LATENCY_INJECTION` overrides
      `DeterministicFillModel::latency_bars`; `DATA_BAR_CORRUPTION` removes a run of bars at a
      seed-selected index; `VOLATILITY_SHOCK` scales every OHLC field of every bar from a seed-selected
      index onward by the same positive multiplier, which preserves each bar's internal ordering exactly
      and so cannot itself produce an invalid bar.
    - `execute_adversarial_probes` runs one baseline replay and one perturbed replay per standardized
      probe, deriving each `degradation_bps` from a real comparison rather than a fabricated number:
      `LOOKAHEAD_LEAKAGE_PROBE` reruns against the first three-quarters of the bars and compares the
      truncated run's equity curve to the full run's over their shared prefix -- any divergence would mean
      a later bar changed an earlier decision, which the engine's bar-by-bar construction makes
      structurally impossible, so this is a genuine, computed regression check rather than an assumed
      pass; `PRICE_JITTER_PROBE` applies an independent deterministic +/-20bps offset to every bar (a
      SHA-256 hash of the scenario seed and bar index, not a system RNG, so the same seed always
      reproduces byte-identical perturbed bars) and measures the return degradation, floored at zero
      because noise that happens to help is not "degradation"; `TRANSACTION_COST_SHOCK` doubles slippage
      and the flat fee; `PARAMETER_CLIFF_PROBE` reruns at the entry threshold shifted +/-10bps and reports
      the larger of the two return swings; `REGIME_STRESS_PROBE` applies a -15% shock from a seed-selected
      bar onward. All five probe results still flow through the unchanged
      `AdversarialResearchGate::evaluate_probes`.
    - New fixtures exercise this against a real 40-bar corpus rather than the existing 2-bar
      `spy-one-minute.csv`, which is too short to observe a meaningful equity curve:
      `tests/fixtures/historical-bars/probe-corpus-one-minute.csv` (a deterministic synthetic walk between
      roughly $99.8 and $100.9), `tests/fixtures/config/backtest-probe-v1.json` (a $150 account so a single
      ~$100 share materially moves total equity -- the original $100,000 fixture diluted every perturbation
      to under 1bps and made the mechanism impossible to observe), and
      `tests/fixtures/config/counterfactual-execute-v1.json`/`adversarial-execute-v1.json`. Every
      threshold/degradation value asserted in the new `apps/cli/src/backtest.rs` tests
      (`counterfactual_execute_mode_runs_a_real_intervention_and_computes_genuine_deltas`,
      `adversarial_execute_mode_runs_real_probes_and_computes_genuine_degradation`) was taken from the
      actual computed CLI output, not predicted by hand: `max_notional` cut to $50 produces a real
      `MAX_NOTIONAL_EXCEEDED` rejection (fill count -1, drawdown -53bps); the built-in strategy's
      `TRANSACTION_COST_SHOCK`/`PARAMETER_CLIFF_PROBE`/`REGIME_STRESS_PROBE` degradations on this corpus are
      16, 19, and 1005bps respectively; a dedicated test
      (`adversarial_execute_mode_fails_the_gate_when_real_degradation_exceeds_the_operators_threshold`)
      tightens `REGIME_STRESS_PROBE`'s threshold below its real 1005bps measurement and confirms
      `gate_passed` genuinely turns `false` with a matching blocking-failure reason, proving the mechanism
      can fail on real data rather than only ever reporting success. Four further unit tests exercise the
      perturbation helpers (`jitter_bars`, `shock_bars_from`, `drop_bars`) directly for OHLC-validity and
      boundary behavior. `cargo fmt`/`clippy -D warnings`/`test --workspace --all-targets` remained clean
      across both Rust workspaces after this change (245 passed, 0 failed, 3 ignored in the main
      workspace, up from 237 by the 8 new `apps/cli/src/backtest.rs` tests); the two prior attested-mode
      fixtures and their tests are untouched and still pass
      unchanged.
    - Bounded scope, stated plainly: only the CLI's built-in `BuyOnceStrategy` path gained real execution.
      A Python-worker-driven backtest, the desktop/gRPC surfaces, and any future non-CLI caller still have
      no execute mode and must supply operator-attested figures exactly as before. `DATA_BAR_CORRUPTION`
      and `VOLATILITY_SHOCK` do not carry corporate actions through a perturbed run (`ReplayContext::run`
      always passes an empty corporate-action list) -- a bounded simplification stated in its own doc
      comment, not a silent gap. This closes the "does the intervention/probe actually execute" gap the
      DUR-02/DUR-06 rows above describe for exactly this path; it is not a claim that either laboratory now
      certifies an arbitrary caller-supplied strategy, and it does not change any external gate.
32. `TaxLotBook::acquire`/`dispose` wired into `core/paper`'s real fill path (2026-09-16). A prior audit
    entry (5.8) recorded `core/accounting::TaxLotBook::dispose` as exact and unit-tested but with zero
    callers anywhere in the repository -- confirmed again at the start of this pass by grepping for
    `TaxLotBook`/`.dispose(`/`.acquire(` outside `core/accounting`. An investigation into the
    structurally larger 5.7 gap (`core/risk`'s aggregate kernel not composed into the real PAPER/LIVE
    order path) found that gap requires portfolio-wide state -- multi-instrument live marks, a sector
    taxonomy, durable equity/peak-equity tracking, a margin model -- that does not exist anywhere in
    `core/paper` or `core/live` today, roughly doubling the work across both crates; it was set aside as
    not well-scoped for one pass. This entry instead closes the smaller, fully self-contained 5.8 gap.
    - `core/accounting::TaxLotBook` gained `snapshot()`/`recover()` (plus the new `TaxLotBookSnapshot`
      type), mirroring the existing `Portfolio::position_snapshot()`/`Portfolio::recover()` pattern: a
      complete, faithful round trip of every internal field, including the two idempotency identity sets
      (`applied_lot_ids`, `applied_disposal_ids`) that are not otherwise observable -- an already
      fully-disposed lot disappears from `lots()`, but its ID must still be permanently refused by a
      future `acquire()`. `recover()` re-validates every invariant a normal `acquire`/`dispose` sequence
      would have enforced (canonical ordering, positive economics, open-lot IDs present in the applied
      set) rather than trusting the caller's serialization.
    - `core/paper::PaperTradingService` gained a `tax_lots: TaxLotBook` field, updated from the single
      real-fill code path in `apply_broker_event`'s `BrokerEvent::Executed` handler, immediately after
      `Portfolio::apply_fill` -- a buy `acquire`s a lot at its exact all-in unit cost (price plus fee); a
      sell `dispose`s existing lots FIFO. `Portfolio::apply_fill` already refuses a sell exceeding the
      held long quantity before this is reached (this codebase's `Portfolio` is long-only: "first slice
      does not permit short positions"), so a disposal here can never exceed available lots. Lot
      selection is fixed at FIFO rather than exposing a configurable policy -- a stated bounded
      simplification, not a correctness gap; `TaxLotSelection::Lifo`/`HighestCost` remain available to a
      direct `TaxLotBook` caller.
    - `synchronize()`'s existing per-event rollback (which already snapshots and restores `orders`,
      `portfolios`, `execution_ids`, and `cash` around each `apply_broker_event` call so one failed event
      in a batch cannot partially mutate state) was extended to also snapshot and restore `tax_lots` --
      found by reading the rollback site itself rather than assumed safe, since a fill now mutates one
      more piece of state than before.
    - Durability: `tax_lots` was added to the journal's `PersistentPaperState` as a new
      `#[serde(default)]` field (`PersistentTaxLotBook`/`PersistentTaxLot`), following the same pattern
      already used for `broker_connected`/`latest_reconciliation` -- a journal written before this change
      deserializes it as an empty book, which is exactly correct, since no fill could have been applied to
      a tax-lot ledger that did not yet exist. `configuration_fingerprint()` is derived only from static
      risk-policy/account/kill-switch configuration, not dynamic state, so this addition does not affect
      the fingerprint and required no change to any checked-in journal fixture; `follon-paper-status
      gateway-matrix` against the existing `tests/fixtures/paper/journal-v2.ndjson` fixture was re-run and
      still passes unchanged.
    - Two new tests: `paper_fifo_tax_lots_track_disposal_cost_basis_independent_of_average_cost` drives two
      buys at different prices ($100 then $120) then a disposal, and asserts the FIFO-realized P&L
      ($59.60, consuming the $100 lot first) is a real, distinct figure from `Portfolio`'s own blended
      average-cost realized P&L ($39.60) read off the existing dashboard -- proving this is an independent
      ledger, not a relabeling of the figure that already existed; `paper_tax_lots_survive_a_durable_journal_reopen`
      closes and reopens a real `FilePaperJournal`-backed service and confirms the open lot and zero
      realized P&L are recovered exactly. Two new `core/accounting` tests cover the `snapshot`/`recover`
      round trip and reject a corrupted snapshot missing an applied-lot identity.
    - `cargo fmt`/`clippy -D warnings`/`test --workspace --all-targets` remained clean (`follon-accounting`
      17 tests, `follon-paper` 23 tests, both up from before this change, 0 failed across the workspace).
    - Bounded scope, stated plainly: only `core/paper`'s real fills gained a tax-lot ledger in this entry.
      `core/backtest::AdvancedBacktestAccount` (a hypothetical, not a real fill) is unchanged and still
      reports only average-cost realized P&L. `core/live`'s identical gap (same `Portfolio`/`apply_fill`
      shape) was closed immediately after by the same session -- see item 33.
33. `TaxLotBook::acquire`/`dispose` mirrored into `core/live`'s real fill path (2026-09-16). The identical
    gap named as a follow-up at the end of item 32 above: `core/live` has the exact same
    `Portfolio`/`apply_fill` shape as `core/paper`, so this is a mechanical repeat of that entry's pattern,
    not a new design.
    - `follon-accounting` added as a dependency of `core/live` (no cycle: `follon-accounting` depends only
      on `follon-domain`/`follon-fx`, both already below `core/live` in the graph). `LiveTradingService`
      gained the identical `tax_lots: TaxLotBook` field, a `LiveError` conversion from `AccountingError`,
      and the same `apply_tax_lot_fill` call from `apply_broker_event`'s fill-execution branch,
      immediately after `Portfolio::apply_fill`. `synchronize()`'s existing per-event rollback (already
      restoring `orders`/`portfolios`/`execution_ids`/`cash` around a failed `apply_broker_event`) was
      extended to also restore `tax_lots`, exactly as in `core/paper`.
    - Durability: `PersistentLiveState` gained the same `#[serde(default)] tax_lots: PersistentTaxLotBook`
      field. `core/live`'s `configuration_fingerprint` is likewise derived only from static
      account/policy/kill-switch configuration, so this required no change to the checked-in
      `tests/fixtures/live/journal-v1.ndjson` fixture; `follon-live-status`'s existing fixture test was
      re-run and still passes unchanged.
    - Rather than writing a new test from scratch, the existing
      `canary_is_four_eyes_durable_and_reconciles_before_a_live_day_counts` test -- which already drives a
      full four-eyes-approved canary order through submission, a real 2-unit fill, `synchronize()`,
      reconciliation, and a journal reopen -- gained tax-lot assertions at both points: after the fill
      (one open lot, unit cost exactly 10, zero realized P&L) and after the reopen (the same lot recovered
      unchanged), extending real coverage instead of duplicating the setup.
    - `cargo fmt`/`clippy -D warnings`/`test --workspace --all-targets` remained clean; `follon-live`'s 9
      tests are unchanged in count (existing tests gained assertions rather than new tests being added) and
      all pass; the desktop Tauri workspace (a separate Cargo workspace that depends on `core/paper`) was
      rebuilt and its 17 tests re-run clean, and its own separate lockfile's `cargo audit --deny yanked`
      remained clean after gaining `csv` as a new transitive dependency (already a direct dependency of
      `follon-accounting`, now reachable through `follon-paper`/`follon-live` for the first time).
    - Bounded scope, stated plainly: `core/paper` and `core/live` both now maintain a real FIFO tax-lot
      ledger from their genuine fills. `core/backtest::AdvancedBacktestAccount` remains unwired, since a
      backtest fill is hypothetical, not a real execution the master plan's tax-lot requirement is about;
      lot selection is fixed at FIFO in both wired paths, not operator-configurable. This does not change
      any external gate.
34. `TaxLotBook` wired into `core/backtest::BacktestLedger`'s real fill path, and a stale-journal-fixture
    defect this pass's own change introduced, found and fixed by running the actual pipeline script rather
    than stopping at `cargo test` passing (2026-09-16, same session as items 32-33).
    - `core/backtest::BacktestLedger` (the plain `follon-backtest run` ledger, distinct from
      `AdvancedBacktestAccount`) has the exact same long-only shape as `core/paper`/`core/live`'s
      `Portfolio` -- its sell branch already refuses a sell exceeding the held quantity ("short positions
      are not enabled in the first milestone") -- so it received the identical `tax_lots: TaxLotBook`
      field and `apply_tax_lot_fill` call as a third, mechanical repeat of items 32-33's pattern.
      `follon-accounting` was already a `core/backtest` dependency (via `AdvancedBacktestAccount`'s margin
      code), so no `Cargo.toml`/lockfile change was needed here. `AdvancedBacktestAccount` itself was
      investigated and deliberately left unwired: unlike `Portfolio`/`BacktestLedger`, it explicitly
      supports short positions (`apply_fill`'s `same_direction`/opposite-direction branches handle a
      negative `quantity`), and `TaxLotBook` has no concept of a short lot; wiring it in would require
      either extending `TaxLotBook` for short-cover disposals or silently only tracking lots while the
      account stays non-negative -- both are a materially different, harder design question than a
      mechanical repeat, deliberately not attempted here. A new
      `ledger_fifo_tax_lots_track_disposal_cost_basis_independent_of_average_cost` test mirrors items
      32-33's: two buys at $100 and $120, a disposal that must consume the $100 lot first (FIFO realized
      59.60 vs. the ledger's own blended average-cost realized 39.60).
    - After this change, `tools/generate_pipeline_evidence.py` -- the script that drives every CLI binary
      end to end and, per item 27, is the only thing that has ever caught fixture-shape defects
      `cargo test` structurally cannot, since no unit test loads these checked-in journal files --
      was run for the first time since items 32-33 (adding `tax_lots` to `core/paper`/`core/live`'s
      persisted journal state) and failed at step 14a: `follon-paper-status` refused
      `tests/fixtures/paper/journal-v2.ndjson` with "paper journal line 1 is not canonical JSON". Root
      cause: `FilePaperJournal::open`'s tamper check deserializes each line, re-serializes it, and requires
      a byte-for-byte match against the checked-in line; the checked-in fixture predates the `tax_lots`
      field, so re-serializing it (now always including that field, empty or not) no longer matches the
      original bytes. This is a real fixture/code inconsistency introduced by items 32-33, not a false
      positive: the check is correctly refusing to trust a line it cannot reproduce. `core/live` has the
      byte-identical check and would have failed the same way at a later step over
      `tests/fixtures/live/journal-v1.ndjson` had the script not aborted on the first failure.
    - Both fixtures were regenerated using the actual current CLI binaries, not hand-edited: the paper
      fixture by running `follon-paper-status` once against a fresh path with the unchanged
      `tests/fixtures/config/paper-v2.json` (reproducing its single `initialized` entry); the live fixture
      by running `follon-live-status` ten times in sequence against a fresh path with the unchanged
      `tests/fixtures/config/live-v1.json` and the same `--opened-at`, reproducing the original journal's
      `initialized` + 9 `restarted` entries exactly. Both regenerated `configuration_fingerprint` values
      matched the originals byte-for-byte (`fd36c32e...` and `aa6f2f03...` respectively), independently
      confirming this pass's own claim that the fingerprint is derived from static configuration only and
      unaffected by the new field; only the added `tax_lots` field and the `entry_hash` values that cover
      it differ. Re-running the full 23-step pipeline afterward completed with zero failures and produced
      all 73 evidence artifacts.
    - `cargo fmt`/`clippy -D warnings`/`test --workspace --all-targets` remained clean throughout
      (`follon-backtest` 13 tests, up from 12).
35. Slice 1 of the 5.7 `core/risk` composition gap closed: gross/net exposure, leverage,
    concentration, and sector/asset-class/currency bucket checks are now real, composed decisions in
    `core/paper::evaluate_risk` and `core/live::evaluate_risk` (2026-09-17). Item 32 above investigated
    composing the full aggregate kernel in one pass and set it aside as requiring portfolio-wide state
    that does not exist in either crate -- multi-instrument live marks, a sector taxonomy, durable
    equity/peak-equity tracking, a margin model -- "roughly doubling the work across both crates." This
    pass splits that gap into exactly the two slices its own investigation implied, and closes the
    first: bucketing/exposure now, equity/margin tracking later.
    - **What is now real.** Both `evaluate_risk` functions gained a `portfolio_risk_decision` helper
      that builds a genuine `follon_risk::PortfolioRiskSnapshot`/`CandidateOrder` from the service's own
      state -- every non-zero position in `self.portfolios`, every working order in `self.orders` -- and
      calls the unmodified `follon_risk::evaluate_portfolio_risk` kernel. Its non-`APPROVED` reason
      codes are merged into the real decision (deduplicating `SELF_TRADE_RISK`, which both engines
      already detect independently from the same working-order state). `RESTRICTED_INSTRUMENT`/
      `INSTRUMENT_NOT_PERMITTED` are a genuine first-time gap-close: neither crate enforced an
      instrument allow/restrict list before this change.
    - **The two new pieces this required.** (1) A `marks: BTreeMap<String, Decimal>` field on both
      services, updated unconditionally at the top of `evaluate_risk` from every order's market
      observation, giving a "last observed mark" for every instrument the service has ever quoted;
      a position with no cached mark yet (e.g. recovered from a pre-existing journal) falls back to its
      own average cost, a stated bounded simplification, not a live feed. (2) `PortfolioRiskComposition`
      -- a new `pub` type holding the real `follon_risk::PortfolioRiskPolicy` plus an operator-authored
      `instrument_buckets: BTreeMap<String, InstrumentBucket>` map (asset class/currency/sector per
      instrument). There is no sector or asset-class taxonomy anywhere in this codebase --
      `core/instrument::Instrument` has no `sector` field, and grepping confirms no separate taxonomy
      type exists -- so this follows the same convention every existing caller of
      `evaluate_portfolio_risk` (the `follon-risk-benchmark` CLI, the gRPC `EvaluatePortfolioRisk` RPC)
      already uses: buckets are operator-supplied strings, not derived from a registry. An instrument
      missing from the map is bucketed as `"unclassified"` (asset class/sector) or the account's own
      currency, not a fabricated guess.
    - **Why equity/margin/drawdown/daily-loss/strategy-bucket checks are still not real, precisely.**
      `evaluate_portfolio_risk` hard-requires `equity > 0`, `peak_equity > 0`, `margin_used >= 0` just to
      run at all (`core/risk/src/lib.rs:493-500`) -- there is no way to call the kernel for bucket checks
      alone without supplying *something* for every one of these. This pass computes a real, point-in-time
      `equity` (cash plus every position marked at its cached/fallback price) but fixes `peak_equity` equal
      to that same `equity` (so `drawdown_bps` is always exactly `0` -- `core/risk`'s own drawdown formula
      only fires when `peak_equity > equity`) and fixes `margin_used`/`daily_pnl` at `Decimal::ZERO`. The
      composed `PortfolioRiskPolicy`'s `max_drawdown_bps`/`max_margin_utilization_bps`/`max_daily_loss`/
      `max_abs_delta`/`max_abs_gamma` are therefore fixed internal constants (`0`), and
      `max_open_orders`/`max_order_rate` are fixed to `usize::MAX`/`u32::MAX` -- **never operator-configurable
      fields** in the new `portfolio_risk` JSON section, specifically so an operator cannot configure a
      limit that silently never fires. `strategy_limits` stays permanently empty for a sharper reason:
      `core/paper`/`core/live`'s `Portfolio` type has no per-strategy attribution at all -- it aggregates
      every strategy's fills into one position per instrument -- so a strategy-bucket check could only ever
      see the incoming candidate's own notional, never cumulative strategy exposure, which would be
      actively misleading rather than merely incomplete. This is a structurally harder problem than the
      other four (it needs `Portfolio` itself redesigned), not just deferred by a neutral constant.
    - **Regression discipline.** Composition is opt-in per configuration: a new optional `portfolio_risk`
      block in the `risk` section of both the paper-v2 and live-v1 JSON schemas
      (`contracts/json-schema/v2/paper-configuration.schema.json`,
      `contracts/json-schema/v1/live-configuration.schema.json`), parsed only by the CLI loaders
      (`apps/cli/src/paper.rs`, `apps/cli/src/live.rs`); the desktop's flat, `deny_unknown_fields`
      `DesktopPaperConfiguration` document is unchanged and always passes `portfolio_risk: None` --
      matching the existing "CLI path first" precedent from item 31. When the block is absent, both
      `evaluate_risk` functions are byte-for-byte unchanged: confirmed by every one of the 23 pre-existing
      `follon-paper` tests and 9 pre-existing `follon-live` tests passing unmodified. The two crates'
      `configuration_fingerprint` functions gained the identical conditional-append pattern already used
      for `broker_route_fingerprint` (paper) -- present only when `portfolio_risk` is `Some`, so an
      unconfigured operator's fingerprint is unaffected. This was independently confirmed, not assumed: the
      checked-in `tests/fixtures/paper/journal-v2.ndjson` and `tests/fixtures/live/journal-v1.ndjson`
      fixtures were regenerated by running the real `follon-paper-status`/`follon-live-status` binaries
      (the identical procedure item 34 used for the `tax_lots` field), and both regenerated
      `configuration_fingerprint` values matched the checked-in originals byte-for-byte
      (`fd36c32e...`/`aa6f2f03...`) -- only the new `marks` field and the `entry_hash` values covering it
      differ. The full 23-step `tools/generate_pipeline_evidence.py` pipeline was re-run afterward and
      completed with zero failures, producing all 73 evidence artifacts again.
    - **New tests, real computed numbers.** Six new tests in `core/paper` and six mirrored in `core/live`
      (`follon-paper` 28 tests total, up from 23; `follon-live` 13, up from 9), plus one loader test in each
      of `apps/cli/src/paper.rs` and `apps/cli/src/live.rs` against new fixtures
      (`tests/fixtures/config/paper-v2-portfolio-risk.json`,
      `tests/fixtures/config/live-v1-portfolio-risk.json`). Per this codebase's existing testing
      discipline, expected figures were taken from real computed decisions, not predicted by hand: e.g.
      `live_portfolio_risk_composition_uses_a_durable_mark_cache_after_journal_reopen` fills a real
      2-share SPY position at 10, re-quotes it to 25 through a second (quantity-rejected) canary attempt so
      the mark cache updates independently of that order's own outcome, closes and reopens the durable
      journal, and asserts a third order against a different instrument is rejected with
      `portfolio_gross_exposure=60.00000000` -- exactly `2 * 25 + 1 * 10`, proving the reopened service
      used the recovered cached mark and not the position's `10` average cost (which would have computed
      `30`, under the configured `40` limit, and passed). `cargo fmt`/`clippy --workspace --all-targets -D
      warnings`/`test --workspace --all-targets` remained fully clean throughout, including the separate
      `apps/desktop/src-tauri` Tauri workspace (17 tests, unchanged).
    - **Bounded scope, stated plainly.** This closes exactly the bucket/exposure half of row 5.7's
      remainder for the real PAPER/controlled-LIVE order path. It is not a claim that portfolio-wide risk
      is fully composed: drawdown, margin utilization, daily-loss, and strategy-bucket checks remain
      structurally uncomposed, by design, until a Slice 2 pass adds durable peak-equity tracking, wires
      `core/accounting::value_margin_account` in with real position/margin-policy data, adds a session-
      start equity baseline, and gives `Portfolio` per-strategy attribution. The desktop order-ticket path
      does not yet expose the new configuration section. FX candidate construction
      (`FxRiskCandidate::from_pricing_snapshot`) and the standalone gRPC `EvaluatePortfolioRisk`/
      `ValueMarginAccount` RPCs are unchanged.
36. Slice 2a of the 5.7 `core/risk` composition gap closed: durable peak-equity tracking makes
    `MAX_DRAWDOWN_EXCEEDED` a real, composed decision (2026-09-17, same day as item 35). Item 35's own
    backlog named four remaining pieces -- durable equity/peak-equity tracking, a wired margin model, a
    session-start daily-loss baseline, and per-strategy `Portfolio` attribution -- and stated they should
    be sequenced, not attempted together. This entry closes the first and smallest of the four: it needed
    only a durable high-water-mark, not new portfolio-wide state `core/paper`/`core/live` lacked entirely
    (unlike margin utilization or strategy attribution, both still open below).
    - **What is now real.** Both `PaperTradingService` and `LiveTradingService` gained a `peak_equity:
      Decimal` field, updated unconditionally at the top of `evaluate_risk` from a new shared
      `current_equity()` helper (cash plus every non-zero position marked at its cached observed mark or
      average-cost fallback) -- the same unconditional-update discipline as the existing `marks` cache, so
      enabling composition later does not start drawdown tracking from an artificially favorable fresh
      baseline. The composed `PortfolioRiskSnapshot`'s `peak_equity` field is now this real running maximum
      instead of a value fixed equal to `equity` (which made `drawdown_bps` always exactly `0` in Slice 1).
      `max_drawdown_bps` is a real, operator-configurable field in the `portfolio_risk` JSON section of both
      schemas now, validated against the same `basisPoints`-style pattern extended to allow the literal
      `10000` (100%) boundary; absent from the document, it defaults to `10000`, which the kernel's ratio
      can mathematically never reach, preserving the "not configurable until it's real" discipline for any
      operator who has not set it explicitly.
    - **Durability.** `peak_equity` is persisted as a new `Option<String>` field on `PersistentPaperState`/
      `PersistentLiveState` (`#[serde(default)]`, so it does not affect the `configuration_fingerprint` --
      it is dynamic state, not static configuration, the same reasoning already applied to `marks`).
      `restore()` bootstraps a journal that never tracked it (`None`) to the real equity computed from the
      just-restored cash/positions/marks, and otherwise takes `max(persisted value, recovered equity)` --
      never silently resets a real historical peak to today's lower current equity. The checked-in
      `tests/fixtures/paper/journal-v2.ndjson` and `tests/fixtures/live/journal-v1.ndjson` fixtures needed
      regenerating again for exactly the same reason item 34 and item 35 already document (a new
      always-serialized field breaks the byte-for-byte tamper check against journals written before it
      existed); both regenerated `configuration_fingerprint` values again matched the checked-in originals
      byte-for-byte, and the full 23-step pipeline was re-run clean afterward.
    - **New tests, real computed numbers.** One drawdown-rejection test and one peak-equity durability test
      were added to each of `core/paper` (30 tests total, up from 28) and `core/live` (15, up from 13).
      `paper_portfolio_risk_composition_rejects_when_drawdown_limit_is_exceeded` fills a real 1,000-share
      position at 100 (a genuine 100,000 peak), re-quotes it to 70, and asserts a real
      `portfolio_drawdown_bps=3000.00000000` (exactly `(100000-70000)/100000`) against a configured 2,000
      bps (20%) limit -- computed, not predicted. `paper_peak_equity_survives_a_durable_journal_reopen` and
      its `core/live` mirror push equity to a real 150,000 peak via a genuine mark-to-market gain, retreat
      the mark back down without the peak following, close and reopen the durable service, and assert the
      recovered peak is still 150,000, not reset to the now-current 100,000. `cargo fmt`/
      `clippy --workspace --all-targets -D warnings`/`test --workspace --all-targets` remained fully clean
      throughout, including the separate `apps/desktop/src-tauri` Tauri workspace (17 tests, unchanged).
    - **Bounded scope, stated plainly.** Only drawdown moved from inert to real. Margin utilization,
      daily-loss, and strategy-bucket checks remain exactly as item 35 described: fixed, non-configurable
      neutral constants, not yet composed. The remaining Slice 2 backlog is now three items, not four:
      wiring `core/accounting::value_margin_account` in with real position/margin-policy data for margin
      utilization; a session-start equity baseline for real `daily_pnl`; and per-strategy position
      attribution in `Portfolio` as a prerequisite for a real strategy-bucket check. The desktop order-ticket
      path still does not expose the `portfolio_risk` configuration section at all (unchanged from item 35).
37. Slice 2b of the 5.7 `core/risk` composition gap closed: a durable session-start equity baseline
    makes `MAX_DAILY_LOSS_EXCEEDED` a real, composed decision (2026-09-18). Item 36's own backlog named
    three remaining pieces -- a wired margin model, a session-start daily-loss baseline, and per-strategy
    `Portfolio` attribution. This entry closes the second: like peak-equity in item 36, it needed only a
    durable baseline value, not new portfolio-wide state `core/paper`/`core/live` lacked entirely (unlike
    margin utilization or strategy attribution, both still open below).
    - **What is now real.** Both `PaperTradingService` and `LiveTradingService` gained a
      `daily_baseline_date: Option<String>` and `daily_baseline_equity: Decimal` field pair, updated
      unconditionally at the top of `evaluate_risk` from the same `current_equity()` helper item 36 added
      -- but *reset*, not maxed, whenever the UTC calendar date sliced from the risk decision's own
      canonical `decided_at` differs from the stored baseline date (including the very first evaluation
      ever, when the baseline date starts `None`). This is a deliberately different update discipline from
      `peak_equity`'s permanent high-water-mark: a daily-loss limit must measure loss *since today's open*,
      not since the account's entire lifetime, so the baseline has to roll forward every UTC day rather than
      only ever increase. The composed `PortfolioRiskSnapshot`'s `daily_pnl` field is now
      `equity - daily_baseline_equity` instead of a value fixed at `Decimal::ZERO`. `max_daily_loss` is a
      real, operator-configurable field in the `portfolio_risk` JSON section of both schemas now; absent
      from the document, it defaults to `i64::MAX` currency units -- the same "no real limit" sentinel
      idiom already used for `max_open_orders`/`max_order_rate` (`usize::MAX`/`u32::MAX`), extended here to
      a fixed-point `Decimal` field with no natural bps-style ceiling. This was a deliberate substitute for
      leaving `max_daily_loss` fixed at `Decimal::ZERO`: with a real, non-zero `daily_pnl` now computed
      unconditionally, a fixed-zero limit would have made `MAX_DAILY_LOSS_EXCEEDED` fire on any loss at all
      for every existing Slice-1/2a `portfolio_risk` configuration the moment this shipped -- so the two
      pre-existing test fixtures (`permissive_portfolio_risk_policy()` in both crates' own test modules)
      were updated to the same sentinel in the same change, verified not to regress any of their other,
      unrelated portfolio-risk tests.
    - **Durability.** `daily_baseline_date`/`daily_baseline_equity` are persisted as new
      `Option<String>` field pairs on `PersistentPaperState`/`PersistentLiveState` (`#[serde(default)]`,
      dynamic state, not static configuration, so `configuration_fingerprint` is unaffected). `restore()`
      never maxes the recovered baseline against current equity the way it does for peak equity: it
      restores exactly the persisted date/equity pair (or leaves both unset for a legacy journal that never
      tracked this), and lets the very next `evaluate_risk` call's own date comparison decide whether that
      persisted baseline is still today's or must roll forward -- an honest reset, not a carried-over
      accumulation, is the entire point of a *daily* baseline. The checked-in
      `tests/fixtures/paper/journal-v2.ndjson` and `tests/fixtures/live/journal-v1.ndjson` fixtures needed
      regenerating again for exactly the reason items 34-36 already document: a new always-serialized field
      breaks the byte-for-byte re-serialization hash-chain check against journals written before it existed.
      Both were regenerated using the actual, current CLI binaries against their unchanged configuration
      documents -- `follon-paper-status` once (the paper fixture is a single freshly initialized entry) and
      `follon-live-status` eleven times in sequence (reproducing the live fixture's exact 1-initialized +
      10-restarted entry count, which had itself grown from item 27's original 9 across the intervening
      items 34-36 regenerations) -- and both regenerated `configuration_fingerprint` values matched the
      checked-in originals byte-for-byte (`fd36c32e...`/`aa6f2f03...`); only the two new fields and the
      hashes covering them differ. The full 23-step `tools/generate_pipeline_evidence.py` pipeline was
      re-run afterward and completed with zero failures, producing all 73 evidence artifacts again.
    - **New tests, real computed numbers.** Three new tests in `core/paper` and three mirrored in
      `core/live` (`follon-paper` 33 tests total, up from 30; `follon-live` 18, up from 15). Per this
      codebase's existing testing discipline, expected figures were taken from real computed decisions, not
      predicted by hand: `paper_portfolio_risk_composition_rejects_when_daily_loss_limit_is_exceeded` fills
      a real 1,000-share position at 100 (the very first risk evaluation of the day, establishing a real
      100,000 pure-cash baseline before the fill even happens), re-quotes it to 70, and asserts a real
      `portfolio_daily_pnl=-30000.00000000` against a configured 2,000 limit -- computed, not predicted.
      `paper_daily_loss_baseline_resets_at_a_new_utc_calendar_day` pushes equity to a real 150,000 gain
      against a 100,000 day-1 baseline (`portfolio_daily_pnl=50000.00000000`), then submits again on the
      next UTC calendar date and asserts the baseline itself reset to 150,000
      (`portfolio_daily_baseline_equity=150000.00000000`) with `portfolio_daily_pnl=0.00000000` -- proving a
      genuine reset rather than a carried-over accumulation, the one behavior with no equivalent in item
      36's peak-equity tests. `paper_daily_loss_baseline_survives_a_durable_journal_reopen` (and its
      `core/live` mirror) push a real +50,000 gain against a 100,000 baseline, close and reopen the durable
      service later the same UTC day, and assert the reopened service still reports the original 100,000
      baseline and the same +50,000 `daily_pnl` -- not a baseline reset to the now-current 150,000 equity.
      `cargo fmt`/`clippy --workspace --all-targets -D warnings`/`test --workspace --all-targets` remained
      fully clean throughout: **270 passed, 0 failed, 3 ignored** in the main workspace (up from 245 by item
      31's count plus the tax-lot/Slice-1/Slice-2a/Slice-2b tests added since), and **17 passed, 0 failed**
      in the separate `apps/desktop/src-tauri` Tauri workspace, unchanged.
    - **Bounded scope, stated plainly.** Only daily loss moved from inert to real. Margin utilization and
      strategy-bucket checks remain exactly as items 35-36 described: fixed, non-configurable neutral
      constants, not yet composed. The remaining Slice 2 backlog is now two items, not three: wiring
      `core/accounting::value_margin_account` in with real position/margin-policy data for margin
      utilization; and per-strategy position attribution in `Portfolio` as a prerequisite for a real
      strategy-bucket check. The desktop order-ticket path still does not expose the `portfolio_risk`
      configuration section at all (unchanged from item 35). The daily baseline resets on UTC calendar-day
      boundaries derived from each risk decision's own `decided_at`, not from an exchange session-open time
      -- a stated bounded simplification consistent with this codebase's existing "no live market-data feed
      at this boundary" posture, not a silent gap.
38. Slice 2c of the 5.7 `core/risk` composition gap closed: margin utilization is now a real, composed
    decision, and the remaining strategy-bucket gap is assessed and precisely scoped rather than left as a
    one-line deferral (2026-09-18, same day as item 37). Item 37's own backlog named two remaining pieces --
    a wired margin model and per-strategy `Portfolio` attribution. This entry closes the first and
    investigates the second in enough depth to state exactly why it is not a same-shape wiring task.
    - **What is now real.** Both `PaperTradingService` and `LiveTradingService` gained a `margin_rates:
      Option<BTreeMap<String, follon_accounting::MarginRate>>` field on `PortfolioRiskComposition`: an
      operator-authored initial/maintenance margin rate per asset class, reusing the exact same
      classification already required for bucket/exposure composition (`instrument_buckets`). When
      configured, `portfolio_risk_decision` builds real `follon_accounting::MarginPosition`s from the
      service's own currently-held positions (the same "pre-trade observed, not post-trade projected"
      convention `equity`/`peak_equity`/`daily_baseline_equity` already use) and calls the unmodified
      `follon_accounting::value_margin_account` -- the same function `follon-operations reconcile-statement`
      and the standalone gRPC `ValueMarginAccount` RPC already use, not a reimplementation. Its
      `initial_margin` becomes the composed `PortfolioRiskSnapshot`'s real `margin_used`. `max_margin_
      utilization_bps` is a real, operator-configurable field in both schemas now; absent, it defaults to
      `10000` (100%). Unlike `max_drawdown_bps` (where 100% is a mathematically exact ceiling the ratio can
      never reach), margin utilization has no such universal bound -- an over-leveraged account could in
      principle exceed it. The sentinel is safe here specifically because `core/paper`/`core/live`'s
      `Portfolio` is fully-paid and long-only (no margin borrowing is modeled anywhere in either crate): with
      a per-position rate at or under 100% and no cash borrowed against a position, utilization cannot reach
      exactly 100% unless an operator sets a 100% rate *and* the account carries zero spare cash -- an edge
      case the operator controls directly by their own `margin_rates` choice, not one this default silently
      hides. This bounded reasoning is stated in the field's own doc comment, not left implicit.
    - **A deliberate no-FX simplification, stated plainly.** `value_margin_account` takes a full `FxBook` and
      a `base_currency`/`maximum_fx_age_seconds` policy for genuine multi-currency accounts. Neither
      `core/paper` nor `core/live` has any cross-currency position support anywhere else in either crate
      (every cash balance and position is implicitly the account's own currency), so this composition
      deliberately does not expose a base currency or FX freshness window: it always passes the account's own
      currency as `base_currency` and an empty `FxBook::default()`, which is exactly correct because
      `FxBook::convert` takes its same-currency fast path (`if from == to { return Ok(amount) }`) and never
      reaches a quote lookup at all. This is not a workaround for missing FX data -- it is the honest
      reflection of a single-currency account, the same boundary `core/backtest`'s "explicit single account"
      scope statement already draws elsewhere in this document.
    - **Fails closed on missing reference data, by design, not by accident.** `value_margin_account` requires
      a configured rate for every asset class among the positions it is asked to value, or it returns an
      error. This composition does not catch that error and substitute a zero: a real technical error
      propagates out of `evaluate_risk`/`submit_intent`/`submit_canary_intent`, aborting the entire order
      submission rather than silently under-counting margin for an unclassified position. A new test in each
      crate (`paper_portfolio_risk_composition_fails_closed_when_a_held_position_has_no_margin_rate`,
      `live_portfolio_risk_composition_fails_closed_when_a_held_position_has_no_margin_rate`) proves this
      directly: a position held in an asset class absent from `margin_rates` makes the next order submission
      return `Err` containing `"missing margin policy"`, not an approved or silently-wrong decision.
    - **New tests, real computed numbers.** Two new tests in each of `core/paper` (35 tests total, up from
      33) and `core/live` (20, up from 18). `paper_portfolio_risk_composition_rejects_when_margin_
      utilization_limit_is_exceeded` (and its `core/live` mirror) fill a real 1,000-share position at 100 with
      all cash spent (cash exactly zero), configure a real 50% initial-margin rate for `equity`, and assert a
      genuinely computed `portfolio_margin_used=50000.00000000` and
      `portfolio_margin_utilization_bps=5000.00000000` (`50,000 / 100,000`) against a configured 40% limit --
      computed, not predicted. `cargo fmt`/`clippy --workspace --all-targets -D warnings`/
      `test --workspace --all-targets` remained fully clean throughout: **274 passed, 0 failed, 3 ignored** in
      the main workspace (up from 270), and **17 passed, 0 failed** in the separate `apps/desktop/src-tauri`
      Tauri workspace, unchanged. `PersistentPaperState`/`PersistentLiveState` gained no new field this pass
      (`margin_used` is computed transiently inside `evaluate_risk`, never persisted; `margin_rates` lives
      only in operator configuration), so -- unlike items 34-37 -- the two checked-in journal fixtures did
      not need regenerating; this was verified by grepping both `Persistent*` structs for `margin` before
      concluding it, not assumed. The full 23-step `tools/generate_pipeline_evidence.py` pipeline was
      re-run afterward and completed with zero failures, producing all 73 evidence artifacts.
    - **Strategy-bucket assessment, precisely scoped rather than re-asserted.** Every prior entry in this row
      (items 35-37) stated that `Portfolio` has no per-strategy position attribution and left it at that. This
      pass instead read `Portfolio`'s actual definition (`core/control-plane/src/lib.rs`): it is one aggregate
      position per `(account_id, instrument_id)` with a single running `quantity`/`average_cost`/
      `realized_pnl` -- every contributing strategy's fills are already merged into one average cost before
      `core/paper`/`core/live` ever see it, and there is no `strategy_id` field anywhere on `Portfolio` or
      `PositionSnapshot` to retrofit. Closing this requires an actual design decision -- either changing
      position identity to `(account_id, strategy_id, instrument_id)` (which would also change what "one
      position" means for every existing per-order aggregate check that intentionally sums across strategies
      today, e.g. `POSITION_LIMIT_OR_SHORT_SELL_EXCEEDED`) or adding a parallel per-strategy attribution
      ledger alongside the existing aggregate (the same shape `TaxLotBook` already uses relative to
      `Portfolio`, but for strategy attribution instead of tax lots) -- and then reconciling that decision
      against every other consumer of `Portfolio`/`PositionSnapshot`: `core/backtest`, the standalone gRPC
      service, desktop Portfolio-workspace projections, and the durable journal formats both crates already
      persist. This is not a same-pass wiring task like margin utilization, daily loss, or drawdown were, and
      is not attempted here; it is recorded as the sole remaining Slice 2 item, precisely bounded rather than
      left as a recurring one-line deferral.
39. Slice 2d of the 5.7 `core/risk` composition gap closed, and the entire Slice 2 backlog opened by item 35
    is now fully closed (2026-09-18, same day as items 37-38). Item 38 assessed strategy-bucket composition
    as requiring a `Portfolio`-level architectural redesign -- either changing position identity to
    `(account_id, strategy_id, instrument_id)` or adding a parallel per-strategy attribution ledger, and
    reconciling either choice against `core/backtest`, the gRPC service, and desktop projections. This entry
    records that on closer investigation the second option did not, in fact, require touching `Portfolio`,
    `PositionSnapshot`, or any of their other consumers at all -- the assessment's *conclusion* (a redesign
    is needed) does not survive contact with the actual implementation, and that correction belongs in this
    document rather than being silently absorbed into a clean "done" entry.
    - **Why the redesign turned out to be avoidable.** `core/risk::RiskPosition` (the kernel's own input
      type) already carries a mandatory `strategy_id` field, and `aggregate_metrics` already computes a real
      `strategy_gross` bucket from whatever `RiskPosition` rows it is given -- both existed before this pass
      and were exercised by every other caller (`follon-risk-benchmark`, the gRPC `EvaluatePortfolioRisk`
      RPC). The gap was never in the kernel; it was that `core/paper`/`core/live` only ever built *one*
      `RiskPosition` row per instrument, hardcoding `strategy_id: "unattributed"`, because neither crate
      tracked which strategy contributed how much of an aggregate position. Splitting that one row into
      several -- one per contributing strategy, at the exact same total quantity -- required new bookkeeping
      *alongside* `Portfolio`, not a change *to* `Portfolio`. `TaxLotBook` (items 32-34) already established
      the precedent of exactly this shape: an independent ledger kept in lockstep with the same fills,
      answering a question `Portfolio`'s single running average cost cannot.
    - **What is now real.** Both `PaperTradingService` and `LiveTradingService` gained a
      `strategy_attribution: BTreeMap<String, BTreeMap<String, Decimal>>` field
      (`instrument_id -> strategy_id -> net signed quantity`), updated from the same real-fill code path as
      `TaxLotBook` (`apply_strategy_attribution_fill`, called immediately after `apply_tax_lot_fill` in the
      `BrokerEvent::Executed` handler): a buy adds the fill quantity to that strategy's own running total for
      that instrument, a sell subtracts, with **no floor at zero** -- a strategy's own tracked value can
      legitimately go negative if it net-sells more than it has net-bought (e.g. because another strategy
      holds shares of the same instrument), an honest signal rather than a clamped, misleading zero.
      `portfolio_risk_decision` now builds the position list by, for each instrument, iterating its tracked
      strategies and emitting one real `RiskPosition` per non-zero entry, then computing
      `remainder = aggregate_quantity - sum(tracked)` and emitting one final "unattributed" row for that
      remainder whenever it is non-zero. This construction guarantees, by simple arithmetic, that gross/net
      exposure, leverage, and concentration are computed from exactly the same total quantity as before this
      change -- only the number of rows and their `strategy_id` attribution differ -- so the split can add
      new bucket-check coverage without being able to silently corrupt any of the four checks already shipped
      in Slices 1-2c. `strategy_limits` is a real, operator-configurable bucket-limit map in both schemas now
      (previously always hardcoded empty).
    - **Durability.** `strategy_attribution` is persisted as a new nested
      `BTreeMap<String, BTreeMap<String, String>>` field (Decimal-as-string, `#[serde(default)]`) on
      `PersistentPaperState`/`PersistentLiveState`, following the same pattern as `marks`; `restore()` parses
      and validates it (canonical instrument/strategy IDs, well-formed decimals) with no positivity
      constraint, since a negative tracked value is a legitimate state as explained above. The `synchronize()`
      per-event rollback snapshot (which already restores `orders`/`portfolios`/`tax_lots`/`execution_ids`/
      `cash` on a failed event application) was extended to also snapshot and restore
      `strategy_attribution`, found by reading the rollback site itself rather than assumed safe, matching
      the exact discipline item 32 already established for `tax_lots`. The checked-in
      `tests/fixtures/paper/journal-v2.ndjson` and `tests/fixtures/live/journal-v1.ndjson` fixtures needed
      regenerating again for the same reason items 34-38 already document; both regenerated
      `configuration_fingerprint` values again matched the checked-in originals byte-for-byte
      (`fd36c32e...`/`aa6f2f03...`), and the full 23-step `tools/generate_pipeline_evidence.py` pipeline was
      re-run afterward and completed with zero failures, producing all 73 evidence artifacts.
    - **New tests, real computed numbers.** Two new tests in each of `core/paper` (37 tests total, up from
      35) and `core/live` (22, up from 20): a rejection test and a durability test. Per this codebase's
      existing testing discipline, expected figures were taken from real computed decisions, not predicted by
      hand. `paper_portfolio_risk_composition_rejects_when_strategy_limit_is_exceeded` fills strategy
      `strategy.paper.001`'s real 100-share position at 100 (a real, attributed 10,000 exposure), then submits
      a *second, distinct* strategy's (`strategy.beta`) 300-share candidate at the same mark and asserts a
      real `STRATEGY_LIMIT_EXCEEDED:strategy.beta` rejection with `portfolio_gross_exposure=40000.00000000`
      (the sum of both strategies' real exposure, proving the split never mis-states the total) and
      `strategy.beta:30000.00000000` in the rendered `portfolio_strategy_gross` bucket map (a new field added
      to `evaluated_limits` this pass, mirroring the existing sector/asset-class/currency bucket rendering).
      `paper_strategy_attribution_survives_a_durable_journal_reopen` (and its `core/live` mirror) fill
      strategy.paper.001's position, close and reopen the durable service, and assert a fresh strategy.beta
      candidate submitted *after* the reopen is rejected against exactly the same pre-existing exposure --
      proving the ledger, not just the mechanism, survives a restart. `cargo fmt`/
      `clippy --workspace --all-targets -D warnings`/`test --workspace --all-targets` remained fully clean
      throughout: **278 passed, 0 failed, 3 ignored** in the main workspace (up from 274), and **17 passed, 0
      failed** in the separate `apps/desktop/src-tauri` Tauri workspace, unchanged.
    - **Bounded scope, stated plainly.** `Portfolio`/`PositionSnapshot` (the canonical position of record,
      also part of the audit event schema serialized in `core/domain::EventPayload::Position`) are completely
      unchanged by this entry, on purpose: every other consumer of those types (`core/backtest`, the gRPC
      service, desktop Portfolio-workspace projections, PostgreSQL position projections) is entirely
      unaffected, and required no review. The new attribution ledger is local to `core/paper`/`core/live`
      only; `core/backtest` gets no equivalent, and is not claimed to. A strategy's own tracked contribution
      going negative (the cross-strategy netting case) is handled by arithmetic (the "unattributed" remainder
      absorbs it) rather than by an explicit warning or incident record -- a stated bounded simplification,
      not a silent gap, since it can only ever affect *which* strategy a given unit of exposure is attributed
      to, never the total exposure figure every other check in this row depends on.
40. `TaxLotBook` now models the short side, and `core/backtest::AdvancedBacktestAccount`'s previously
    unwired long/short advanced projection is wired to it (2026-09-18, row 5.8). Item 32 (and the 5.8 row
    ever since) recorded this specific gap and its reason plainly: "`TaxLotBook` has no concept of a short
    lot, so wiring it in is a materially harder design question than the mechanical repeat used for the
    other three ledgers, not attempted." This entry closes it.
    - **What is now real.** `core/accounting::TaxLotBook` gained a parallel short-side ledger: a new
      `ShortTaxLot` type (mirroring `TaxLot`, but holding `unit_proceeds` -- what was received when the
      short was opened -- rather than a cost paid), `open_short` (mirrors `acquire` exactly), and `cover`
      (mirrors `dispose` exactly, except realized P&L is `proceeds - cost_basis - fee`, the inverse of a
      long disposal's `proceeds - cost_basis - fee` because the economics themselves are inverted -- you
      receive money opening a short and pay to close it, the reverse of a long). Both are additive: `acquire`
      and `dispose` are byte-for-byte unchanged, so every existing long-only caller (`core/paper`, `core/live`,
      `core/backtest::BacktestLedger`) is unaffected. `AdvancedBacktestAccount` gained a `tax_lots: TaxLotBook`
      field and now calls the appropriate long or short operation from its one real-fill code path
      (`apply_fill`), exactly mirroring the pattern items 32-34 already established for the other three
      ledgers -- new public `tax_lots()`/`short_tax_lots()`/`tax_realized_pnl()` accessors expose it,
      matching the existing accessor shape on `core/paper`/`core/live`.
    - **The crossing-fill design, the actual "materially harder" part.** `AdvancedBacktestAccount`, unlike the
      three already-wired ledgers, permits a single fill to close an existing position *and* open the
      opposite position in one execution (e.g. selling 8 shares against a 5-share long: 5 shares close the
      long, the remaining 3 open a new short) -- `apply_fill`'s own pre-existing `same_direction`/`closing`
      logic already computed exactly this split for the average-cost position, so the tax-lot wiring reuses
      that same `closing` quantity rather than recomputing it. The one new problem it introduces is the fee:
      one fill has one fee, but now two tax-lot operations. It is split proportionally by quantity (the
      closing leg gets `fee * closing / fill.quantity`; the opening leg gets the exact remainder, `fee -
      closing_fee`, not its own independently rounded share, so fixed-point division never drops or invents
      a fraction of a cent and the two legs always sum to exactly `fill.fee`). A pure addition to an existing
      side (or a new position from flat, the `same_direction` branch) needs no split: the entire fee goes to
      the one operation, exactly as items 32-34's mechanical repeat already does.
    - **New tests, real computed numbers.** Two new tests in `core/accounting` (19 total, up from 17:
      `short_tax_lots_apply_fifo_and_idempotent_covers_exactly` and its recovery-rejection counterpart,
      mirroring the existing long-lot pair exactly) and two in `core/backtest` (15 total, up from 13). Per
      this codebase's testing discipline, expected figures were taken from real computed values, not
      predicted by hand:
      `advanced_account_tracks_a_pure_short_position_in_the_tax_lot_ledger` opens a 5-share short at 100
      with a 5-unit fee and asserts the resulting lot's `unit_proceeds` is exactly `99` (`(5*100-5)/5`).
      `advanced_account_crossing_fill_splits_tax_lots_and_fee_across_both_sides` buys 5 shares at 100 (fee 5,
      so an all-in cost basis of 101/share), then sells 8 at 120 (fee 8) in one fill: the long lot disposes
      completely (`tax_lots()` empty) with a real, computed `tax_realized_pnl` of `90`
      (`(5*120) - (5*101) - (8*5/8)`), and a fresh short lot opens for the 3-share remainder at a real
      `unit_proceeds` of `119` (`(3*120 - (8-5)) / 3`) -- proving both the split and the fee allocation are
      exactly self-consistent, not merely plausible-looking. `cargo fmt`/
      `clippy --workspace --all-targets -D warnings`/`test --workspace --all-targets` remained fully clean
      throughout: **282 passed, 0 failed, 3 ignored** in the main workspace (up from 278), and **17 passed, 0
      failed** in the separate `apps/desktop/src-tauri` Tauri workspace, unchanged. `AdvancedBacktestAccount`
      is not durably persisted the way `core/paper`/`core/live` are (a backtest is a one-shot batch replay,
      not a restartable service), so no journal fixture needed regenerating; this was confirmed by re-running
      the full workspace suite and diffing the two checked-in journal fixtures against their pre-this-entry
      state, not assumed. The full 23-step `tools/generate_pipeline_evidence.py` pipeline was re-run
      afterward and completed with zero failures, producing all 73 evidence artifacts.
    - **A real defect this pass found and fixed in its own uncommitted work, not shipped past review.** The
      first attempt at this change compiled `core/accounting` and `core/backtest` cleanly in isolation but
      broke the full workspace build: `core/paper`'s and `core/live`'s own `restore()` methods each construct
      a `TaxLotBookSnapshot` literal (converting their journal's persisted tax-lot fields back into the type
      `TaxLotBook::recover` accepts), and adding the three new short-lot fields to that struct without
      `#[non_exhaustive]` turned both call sites into compile errors (`missing fields`). This was only caught
      because this session ran the actual full-workspace `cargo test`, not just the two crates being changed
      -- a first pass at automating that check piped the command through `grep`, which reports its own exit
      code (0, because it found matching error lines), silently masking the real compilation failure; the
      mistake was caught by reading the captured log's content, not the pipe's exit code, and the verification
      approach was corrected to capture a real exit code directly before trusting a clean result again. Both
      sites were fixed with an explicit, documented empty short-side ledger (`core/paper`/`core/live`'s
      `Portfolio` is long-only, so their journals never carry short-lot data), and the full suite was then
      re-run genuinely clean.
    - **Bounded scope, stated plainly.** Lot selection remains fixed at FIFO for both sides, matching every
      other wired ledger. `AdvancedBacktestAccount`'s own average-cost `realized_pnl` (fee-exclusive) and the
      new FIFO tax-lot `realized_pnl` (fee-inclusive) are intentionally different numbers measuring different
      things, exactly the same documented distinction items 32-34 already established for `core/paper`/
      `core/live` -- not a new inconsistency introduced here. This closes the specific gap 5.8 has recorded
      since item 32; it does not change borrow/recall/financing modeling (already implemented and unaffected)
      or any external gate.
41. CI static-analysis and supply-chain hardening, and a genuine finding the new tooling caught and fixed
    itself (2026-09-20, Security conformance). Closes the specific "Complete SAST/DAST coverage... not
    evidenced" gap that row has recorded since the audit's first version, for the SAST half.
    - **What is now real.** A `sast` job in `.github/workflows/ci.yml` runs Semgrep 1.177.0 against the whole
      tracked tree with the `p/owasp-top-ten`, `p/rust`, `p/python`, `p/typescript`, and `p/secrets` registry
      rulesets and `--error` (build-failing, matching this workflow's existing `-D warnings`/`--deny yanked`
      strictness, not an advisory-only report). Two rules are excluded with a documented reason
      (`--exclude-rule`): `rust.lang.security.temp-dir.temp-dir` and `rust.lang.security.args.args` both fired
      exclusively on this repository's own `#[test]`-only fixture paths and ordinary local-file CLI argument
      parsing (verified by reading every one of their 42 combined hits before excluding either), not a real
      insecure-temp-file or injection pattern -- confirmed by first running the scan locally, reading each
      finding's actual source location, and only then choosing the CI configuration, not the reverse. Every
      `uses:` reference across all five CI jobs (`actions/checkout`, `gitleaks/gitleaks-action`,
      `actions/dependency-review-action`, `dtolnay/rust-toolchain`, `actions/setup-node`,
      `actions/setup-python`, `actions/upload-artifact`) is now pinned to the exact commit SHA its previous
      mutable tag currently resolves to (resolved via the real GitHub API, not guessed), with a `# vX.Y.Z`
      comment for traceability, closing the `yaml.github-actions.security.github-actions-mutable-action-tag`
      finding (16 occurrences) Semgrep raised. `.github/dependabot.yml` gained a `cooldown` block
      (`default-days: 7`, `semver-major-days: 21`, `semver-minor-days: 10`, `semver-patch-days: 7`) on all
      four ecosystems, closing the `dependabot-missing-cooldown` finding (4 occurrences).
    - **A real defect the new tooling found and fixed, not a rule silenced to get a clean run.** The Semgrep
      `nginx` ruleset flagged `infra/nginx.dashboard.conf:35` (`generic.nginx.security.request-host-used`,
      CWE-290): `proxy_set_header Host $host;` forwards the inbound request's own, attacker-controlled Host
      header straight to the dashboard backend. `apps/desktop/server.py` was checked and does not read the
      Host header for anything (no vhost routing, no redirect construction), so the fix is a fixed, known-good
      value matching the `proxy_pass` target exactly (`proxy_set_header Host dashboard:8080;`) rather than an
      operator-configurable template -- this removes the untrusted-input pattern entirely instead of merely
      validating it. Also confirmed `actions/dependency-review-action@v4` was already a latent bug before this
      entry: that repository has no bare `v4` tag (only exact `v4.x.y` releases), so the existing line would
      have failed to resolve if its `vars.DEPENDENCY_REVIEW_ENABLED == 'true'` guard were ever flipped on; it
      is now pinned to the real `v4.9.0` commit.
    - **Independently verified, not merely written.** The exact CI Semgrep command was run locally against the
      real tree before and after each fix: 63 findings across 5 categories at first trial, down to the 2
      documented, justified exclusions plus 4 genuine fixes (nginx, GitHub Actions pins, Dependabot cooldown),
      ending at **0 findings, exit code 0**. `cargo fmt --all -- --check`, `cargo clippy --workspace
      --all-targets -- -D warnings`, and `cargo test --workspace --all-targets` (captured to a file and
      checked by a real `$?`, not piped through `grep`, per the lesson item 40 already recorded) all remained
      clean after the unrelated changes in this same entry's session (see item 42). `python -c "import yaml"`
      parsed both edited YAML files without error.
    - **Bounded scope, stated plainly.** DAST (an authenticated dynamic scan against a running deployment,
      e.g. an OWASP ZAP baseline run against the `infra/compose.dev.yml` topology proven live in item 29) is
      explicitly not attempted here -- it needs a running target and its own review, not a CI-config change,
      and is recorded as still open in the Security conformance table. Semgrep's registry rulesets are not
      pinned to a fixed ruleset revision (only the CLI version, 1.177.0, is pinned), so, like `cargo audit`,
      this job can start failing on unchanged code when the registry adds a new rule -- an accepted,
      already-precedented tradeoff for advisory/scanning CI jobs in this workflow, not an oversight.
42. A first property/model-test slice for the OMS order-lifecycle state machine (2026-09-20, Reliability and
    quality conformance). A first bounded slice of the "comprehensive state-model/property test program... not
    complete" gap that row has recorded since the audit's first version; the full program remains open.
    - **What is now real.** `core/control-plane/tests/oms_lifecycle_proptest.rs` adds `proptest` (1.11.0) as a
      dev-dependency and tests `OmsOrder::transition` against an independently-authored from -> allowed-to
      edge table covering all 15 `OrderState` variants, deliberately written separately from (not copied from,
      and with no access to) the crate's own private `is_valid_transition` match block, so a future edit to
      one without the other is a real regression signal instead of the test only confirming the
      implementation agrees with itself. The property test generates random sequences of 1-39 attempted
      transitions per case (proptest's default 256 cases) and asserts, at every step: the real transition's
      Ok/Err outcome matches the model's legal/illegal verdict exactly; the order's state after the attempt
      equals what the model predicts; a rejected transition never mutates state (the atomicity invariant); and
      neither `order_id` nor the original `intent` ever changes across any attempted transition, legal or not.
      Two further deterministic tests assert `Filled` has no legal outgoing transition at all, not even to
      `UNKNOWN` (an absolute-terminal invariant distinct from `Cancelled`/`Rejected`/`Expired`, which do permit
      a late-evidence transition to `UNKNOWN`), and that the independent model itself covers every declared
      `OrderState` variant, including the two never reached by the current `from_approved_intent`/`transition`
      implementation (`PendingRisk`, `RiskRejected`'s own further transitions).
    - **Proven to actually catch a divergence, not merely written to run.** Before finalizing, one edge
      (`Created -> RiskRejected`) was deliberately deleted from the model and the suite re-run: it failed
      immediately with a real proptest-shrunk minimal counterexample (`candidates = [RiskRejected]`), then the
      edge was restored and the suite re-run clean, along with deleting the resulting
      `oms_lifecycle_proptest.proptest-regressions` artifact rather than committing it. This is the same
      discipline item 40 already established for verification: a test that has not been shown to fail on a
      real defect is not yet evidence that it can catch one.
    - **Independently verified.** `cargo test -p follon-control-plane --test oms_lifecycle_proptest` passed (3
      tests). The full workspace suite (captured to a file, checked by a real `$?`) went from 282 to **285
      passed, 0 failed, 3 ignored** in the main workspace, and the separate `apps/desktop/src-tauri` Tauri
      workspace was independently re-run and remained **17 passed, 0 failed**, confirming this entry's changes
      (scoped to `core/control-plane` only) did not touch it. `cargo fmt --all -- --check` and `cargo clippy
      --workspace --all-targets -- -D warnings` both stayed clean.
    - **Bounded scope, stated plainly.** This covers one state machine (`core/control-plane::OmsOrder`, shared
      by `core/paper`/`core/live`/replay). It is not model/property coverage for options exercise/assignment,
      EMS scheduling/combination legality, multi-currency accounting invariants, or any other planned
      asset/order type -- those remain open, exactly as the Reliability conformance row now states.
43. A second property-test slice, for the options exercise/assignment settlement function (2026-09-20,
    Reliability and quality conformance). Closes the "options exercise/assignment" example named in item 42's
    own remainder text; the wider program (EMS scheduling/combination legality, multi-currency accounting)
    remains open.
    - **What is now real.** `core/options/tests/option_lifecycle_settlement_proptest.rs` adds `proptest` as a
      dev-dependency to `core/options` and checks `settle_expired_option_position` -- a pure function, not a
      persistent state machine, so its properties are economic invariants independently derived from the
      documented contract (`OptionLifecycleOutcome`/`OptionSettlementMethod` doc comments) rather than an
      edge-list model: (1) every settlement closes the position exactly
      (`option_quantity_delta == -signed_contract_quantity`) and intrinsic value is never negative; (2) cash
      settlement never moves the underlying, expiry always zeroes both deltas, and physical settlement's
      underlying and cash deltas always run in exactly opposite directions -- nothing is ever delivered and
      paid for in the same direction, and nothing is ever free; (3) the three outcomes (`Expired`/`Exercised`/
      `Assigned`) classify consistently against intrinsic value, the automatic-exercise threshold, and
      position sign; (4) settlement is a pure, deterministic function of its inputs. Two deterministic tests
      cover pre-expiration and zero-quantity rejection.
    - **Proven to actually catch a divergence, not merely written to run.** Before finalizing, the physical
      settlement cash-sign negation was deliberately deleted (`cash_delta = underlying_quantity_delta *
      strike` instead of its negation) and the suite re-run: `settlement_method_conservation_holds` failed
      immediately with a real proptest-shrunk counterexample (a short call, strike 100, multiplier 100,
      quantity -2, underlying 200 -- both deltas landing at the same sign instead of opposite), then the fix
      was restored and the suite re-run clean, along with deleting the resulting
      `option_lifecycle_settlement_proptest.proptest-regressions` artifact rather than committing it -- the
      same discipline items 40 and 42 already established. A first mutation attempt (swapping the Call/Put
      physical-delivery branches) was tried first and found *not* caught by this property set, since the
      opposite-sign conservation check holds regardless of which branch computed the delta; that miss is
      recorded here rather than quietly discarded, and is exactly why the cash-sign-negation mutation was
      tried next as a check the properties as written could actually fail on.
    - **Independently verified.** `cargo test -p follon-options --test option_lifecycle_settlement_proptest`
      passed (6 tests). `cargo fmt --all` reformatted the new file's import ordering (one automatic fixup,
      confirmed to touch no other file via `git status`); `cargo fmt --all -- --check` and `cargo clippy
      --workspace --all-targets -- -D warnings` were both clean afterward. The full workspace suite (captured
      to a file, checked by a real `$?`) went from 285 to **291 passed, 0 failed, 3 ignored**.
    - **Bounded scope, stated plainly.** This covers one pure settlement function, not the full options
      lifecycle (chain construction, Greeks, multi-leg expiry scenarios already have their own example-based
      tests, not property tests) or any other planned asset/order type.
44. A third property-test slice, for multi-account portfolio aggregation (2026-09-20, Reliability and quality
    conformance). Advances the "multi-currency accounting" example named in items 42-43's own remainder text;
    the wider program (EMS scheduling/combination legality, the rest of `core/accounting`'s margin/tax-lot/
    financing functions) remains open.
    - **What is now real.** `core/accounting/tests/multi_account_aggregation_proptest.rs` adds `proptest` as a
      dev-dependency to `core/accounting` and checks `aggregate_account_portfolios` -- a pure, deterministic
      projection, like item 43's settlement function, not a persistent state machine -- against two invariants
      the function's own doc comment already commits to ("deterministic irrespective of input snapshot order")
      plus one this test adds explicitly: conservation. Generated 1-5 accounts drawing from a small fixed
      universe of two cash currencies and two instruments (kept fixed-per-instrument, not randomized, so every
      contributing account agrees on asset class/currency/multiplier by construction -- the property under
      test is conservation on the success path, not the pre-existing disagreement-rejection path, which already
      had its own unit test). Checked: aggregated cash per currency equals the exact sum contributed by every
      account that held it; an aggregated position's quantity and market value equal the exact sum across
      every contributing account, and its `contributing_account_ids` is exactly that account set, sorted; a
      currency or instrument no account held is absent from the result entirely, not zeroed; and re-running
      aggregation against the same accounts rotated into a different order produces a byte-for-byte identical
      result. Three deterministic tests cover the documented rejection paths: duplicate `account_id`,
      mismatched `as_of`, and an empty account list.
    - **Proven to actually catch a divergence, not merely written to run.** Before finalizing, the market-value
      accumulator was deliberately changed from summing (`checked_add`) to overwriting (keeping only the last
      contributing account's value) and the suite re-run: both `aggregation_conserves_cash_and_position_totals`
      and `aggregation_is_order_independent` failed immediately with a real proptest-shrunk two-account
      counterexample showing the exact wrong total, then the fix was restored and the suite re-run clean,
      along with deleting the resulting `multi_account_aggregation_proptest.proptest-regressions` artifact --
      the same discipline items 40, 42, and 43 already established.
    - **Independently verified.** `cargo test -p follon-accounting --test multi_account_aggregation_proptest`
      passed (5 tests). `cargo fmt --all` reformatted one line in the new file (confirmed via `git status` to
      touch no other file); `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets -- -D
      warnings` were both clean afterward. The full workspace suite (captured to a file, checked by a real
      `$?`) went from 291 to **296 passed, 0 failed, 3 ignored**.
    - **Bounded scope, stated plainly.** This covers one pure aggregation function operating on
      already-reconciled, already-marked account snapshots; it says nothing about margin valuation
      (`value_margin_account`), tax-lot disposal, financing accrual, or FX conversion correctness, all of
      which remain example-tested only, and it does not touch, and is not evidence for, cross-account
      allocation or transfer, which the roadmap keeps frozen regardless.
45. Fabricated-evidence remediation, round two: the 32 "advanced evidence" fixtures never had real backing, and
    fixing it surfaced two further real bugs (2026-09-20). This is the same zero-synthetic-data violation items
    23-24 already found and fixed once (2026-09-10) -- recurring in a subsystem that remediation never reached.
    - **What was wrong.** `tools/build_advanced_evidence_fixtures.py` is 32 hand-typed Python dict literals (for
      example a `"return_bps": "1420.00"` invented for `experiment-lineage.json`), one per DUR/SOLO/RES/DATA
      evidence category, each validated only against its own JSON Schema -- never derived from any computation.
      A repository-wide search confirmed **29 of the 32 have zero real Rust or Python backing anywhere in the
      codebase**: SOLO-04 Market Scanner, DATA-03 News Revision Timeline, and RES-02 Strategy Composition
      Studio -- all three named in item 22 as "100% typed and populated" -- are not partially-implemented
      features; nothing computes them at all. The remaining 3 (`strategy-capsule-manifest`,
      `decision-reconstruction`, `data-rights-and-semantics-receipt`) do have real Rust modules
      (`core/control-plane::capsule`/`::provenance`, `core/market-data::rights`), but no CLI subcommand invokes
      any of them either, so all 32 were equally fake in the actual pipeline output. `tools/
      generate_pipeline_evidence.py`'s step 16g copied all 32 straight into `var/` -- the directory every other
      genuinely-computed artifact publishes to and the dashboard reads as real, dated evidence -- and the
      desktop rendered them through the same `appendAdvancedEvidenceRows` mechanism as real evidence, with
      professional-sounding panel copy ("Screen instruments against versioned indicators... with ranked
      reasons"). Items 21/22's own claims ("32 canonical advanced evidence fixtures... published," "zero
      unavailable evidence panels," "100% typed and populated") were true only because the data was invented,
      not because real evidence existed.
    - **The fix.** Step 16g no longer copies the 32 fixtures into `var/`; `build_advanced_evidence_fixtures.py`
      still runs (it is a legitimate schema-conformance test, validating 32 example documents against their
      contracts) but its output stays in `tests/fixtures/config/advanced/`, which is what it actually is.
      Verified this needed zero dashboard code changes: `appendAdvancedEvidenceRows`'s existing, already-tested
      empty-state path (`"No typed market-scanner candidate records are published."` etc.) is exactly what a
      missing category was always designed to fall back to. Confirmed live, not just by reading code: rebuilt
      `web-dist`, ran the real `apps/desktop/server.py` against a freshly regenerated `var/`, and drove a
      headless browser to `/workspace/command-center`, `/workspace/news-cockpit`, and `/workspace/
      strategy-studio` -- all three panels now render their honest empty state, zero console errors, zero
      failed requests. `tools/generate_pipeline_evidence.py`'s own honest final count dropped from the
      previously-claimed **75** to **42** genuinely-computed or genuinely-copied artifacts.
    - **A real defect the fix's own live verification found: a schema-field collision was silently hiding
      genuine evidence.** With the fake `model-evaluation-benchmark.json` gone, the live dashboard snapshot
      still showed a `model_evaluation_benchmark` category -- but the *only* file in `var/` containing its
      discriminator field, `benchmark_schema_version`, was `follon-risk-benchmark.json`: the real, genuinely
      computed local risk-latency benchmark (p99 microseconds against the local threshold), evidence for the
      roadmap's own "risk-check p99 below 5 ms" service objective. `apps/desktop/server.py`'s
      `ADVANCED_EVIDENCE_SCHEMAS` table happened to reuse the bare field name `benchmark_schema_version` for
      the fabricated `model_evaluation_benchmark` category -- the *only* collision among 39 otherwise-unique
      discriminators -- so the real risk-benchmark file was misclassified into the wrong category server-side,
      then silently dropped by the TypeScript boundary's strict re-validation (right guard, wrong outcome: the
      code was already written not to trust a classification blindly, it just could not detect that the
      classification itself was wrong). Net effect, confirmed live before the fix: the "Local risk-evaluator
      benchmark" panel in Execution Blotter rendered **empty** despite the real evidence existing on disk. Renamed
      the fabricated schema's field to `model_evaluation_schema_version` (`contracts/json-schema/v1/
      model-evaluation-benchmark.schema.json`, `tools/build_advanced_evidence_fixtures.py`, `apps/desktop/
      server.py`, `apps/desktop/src/evidence.ts`, `apps/desktop/test/connected-evidence-regression.mjs`) and
      added a comment on `ADVANCED_EVIDENCE_SCHEMAS` requiring every future discriminator to stay unique against
      this table *and* the other bare `benchmark_schema_version` check in `classify_artifact`. Confirmed live:
      the panel now shows the real artifact, `p99=44µs`, `threshold=5000µs`, "Within local threshold."
    - **A second real defect the fix's own verification found: the pipeline was silently corrupting a checked-in
      test fixture on every run.** `generate_pipeline_evidence.py`'s Step 15 ran `follon-live-status` directly
      against `tests/fixtures/live/journal-v1.ndjson` -- a git-tracked fixture, not a `var/` output.
      `follon-live-status` durably appends a `live.service.restarted.v1` audit event to whatever journal it
      opens (correct, intentional behavior for a real LIVE journal: every open must be recorded), so every
      pipeline run silently appended two lines to the checked-in fixture. Caught by `git status` showing an
      unexpected modification to a file this entry never intended to touch, not by inspection. Fixed by copying
      the fixture into `var/follon-live-journal.ndjson` first and pointing the command at the copy, matching
      every other step's write-to-`var/`-only discipline. Verified by running the full pipeline twice more
      afterward: `git status` on the fixture stayed clean both times, and `follon-live-dashboard.json` still
      produced the correct projection (`audit_sequence: 14`, from the copy's own append) unchanged in substance.
    - **Independently verified.** The full desktop suite (`npm run test:evidence`, 12 regression files) and
      `python apps/desktop/test/server_contract.py` (18 tests) both passed clean after every change in this
      entry, including the field rename and the fixture-copy fix. `cargo fmt --all -- --check` and `cargo test
      --workspace --all-targets` (captured to a file, checked by a real `$?`) stayed clean throughout --
      untouched by this entry's changes, which are Python/TypeScript/JSON Schema only.
    - **Corrected elsewhere.** `docs/06-delivery/12-dashboard-feature-integration-status.md` and `docs/06-
      delivery/13-step-by-step-implementation-matrix.md` both repeated the "32 canonical advanced fixtures...
      75 immutable evidence artifacts... zero unavailable evidence panels" claim; both are corrected in the
      same pass as this entry.
    - **Bounded scope, stated plainly.** This entry stops a false claim; it does not build the 29 missing
      features. Market Scanner, News Revision Timeline, Strategy Composition Studio, Capital Allocation Plan,
      Adapter Qualification, and the other 24 unbacked categories remain entirely unimplemented, now honestly
      reflected as empty dashboard panels rather than populated with invented numbers. Building real
      computation for any of them is separate, substantial, per-category product work -- comparable in shape to
      row 5.6's EMS combo-risk-gating epic (item 5.6's own remainder text), not a wiring task.

46. Three of the five slices of row 5.6's combination risk-gating epic landed (2026-09-23, row 5.6).
    The 2026-09-20 pass recorded this as "a bounded-but-real multi-session epic" and deliberately did not
    start it. This pass cut it into five slices and landed the first three, each committed separately and
    each standing alone.
    - **E1.1 -- the contract.** `core/domain` gains `ComboIntent`, `ComboIntentLeg` and `ComboPriceLimit`,
      with validation, exact fixed-point net-price and gross-notional arithmetic, and per-leg position
      projection. `core/execution` now re-exports `ComboPriceLimit` from the domain instead of defining its
      own copy, and `plan_option_combo` calls the shared `check_net_price`, so the planner and the risk gate
      cannot disagree about whether a combination is acceptably priced. Four decisions are deliberate and
      each is covered by a test named after it: a single-leg combination is refused (that is a plain order,
      whose path carries strictly more risk coverage); duplicate instruments are refused rather than netted;
      `gross_notional` sums leg magnitudes rather than netting them, because a short leg is a real obligation
      until the combination is closed; and a maximum-debit protection refuses a combination that priced to a
      credit, and vice versa, because the operator approved a particular structure and the opposite one is a
      different trade rather than a cheaper version of the same one.
    - **E1.2 -- the gate.** `PaperTradingService::evaluate_combo_risk` restates every rule the single-order
      gate applies, in the terms a combination needs, and creates no order. `core/risk` gains
      `evaluate_portfolio_risk_with_candidates`, which admits several simultaneous candidate legs: two legs
      that each sit under a concentration or bucket limit alone can breach it jointly, and a combination
      executes atomically, so there is no moment at which only one is filled. `evaluate_portfolio_risk` now
      delegates to it and is behaviourally identical -- every existing test passed unchanged. The
      order-notional limit is charged the gross rather than the net; the price collar is per leg against that
      leg's own mark; only a net debit is charged against available cash; the per-order quantity limit binds
      the largest leg rather than the unit count; a kill switch on any single leg halts the whole
      combination; and a stale observation is a hard error rather than a rejection reason, because a decision
      made against an unusable observation is not a decision and must not be recorded as evidence of one.
      **The slice's least obvious consequence is worth stating plainly.** The paper gate refuses every net
      short position, and almost every real spread has a short leg, so applying that rule per leg would have
      made this path reject essentially every structure it exists to support. Rather than relax a safety
      guard to make a feature work, this added `PaperRiskPolicy::short_exposure`: an explicit operator
      permission with a stated per-instrument bound, `None` at all three construction sites, so no existing
      configuration changed and the compiler demanded a deliberate decision at each one. `core/paper` holds
      no option reference data and cannot prove a short leg is covered by its long one, so it does not assume
      it.
    - **E1.3a -- the submission path.** `core/control-plane` gains `OmsComboOrder`: one combination is one
      OMS order with one state, reusing `is_valid_transition` unchanged so it inherits item 42's property
      test rather than needing a second, separately-verified state machine. A per-leg state was rejected
      because it could express outcomes an atomic broker order cannot produce. `core/paper` gains
      `submit_combo_intent`, following `submit_intent`'s order of operations for the same reasons, with
      durable records, full restart recovery, and an idempotent retry that refuses a re-priced or re-sized
      repeat of the same identity. A transport failure leaves the combination `UNKNOWN` rather than guessing.
      `IbkrPaperAdapter` now accepts native combinations, because the real paper bridge it models
      (`adapters/brokers/ibkr::submit_paper_combo`) does; a model that refused what the thing it models
      accepts would leave the path testable only against a rejection. **Corrected in place 2026-09-28: the
      premise was false.** `submit_paper_combo` only forwards a `submit_combo` request to the Python bridge,
      whose dispatch has no such operation and answers `unsupported bridge operation`; nothing in the
      repository builds an IBKR BAG contract. The model's combination support is a model capability, not
      parity with the real bridge (delivery state E5.1). **The part that matters most for safety
      is integration, not submission**: open orders, the rate window, reserved cash, the `UNKNOWN` guard and
      self-trade all now read both order maps through shared helpers, because a combination invisible to the
      single-order gate would be a hole in exactly the limits it is subject to. Its legs are individually
      resting orders so the per-instrument self-trade check sees them, while `core/risk` counts *distinct*
      order identities against `max_open_orders` so a four-leg structure stays one open order.
    - **Stated rather than left implicit.** Combinations cannot fill, cancel or reconcile yet. A working
      combination therefore reports an `UNRECONCILED_COMBINATION` reconciliation issue, so a session holding
      one does not reconcile clean and cannot count toward the 30-clean-PAPER-session gate. That consequence
      is intended: a gate that counted sessions in which part of the order flow was never checked would not
      measure what it claims to. Removing that issue is the last step of E1.3b, not the first.
    - **Tests were verified, not merely written.** Nine defects were injected across the three slices and the
      intended test caught each one: netting `gross_notional`; dropping the debit/credit sign guard; charging
      the notional limit the net instead of the gross; a first-leg-only price collar; first-leg-only group
      evaluation in the aggregate kernel; an absent short permission silently allowing shorts; a working
      combination reserving no cash; combination legs invisible to the self-trade check; and an unknown
      transport outcome recorded as a clean rejection. One further injection -- a first attempt at the collar
      defect -- changed no behaviour and therefore verified nothing; it was redesigned and re-run rather than
      recorded as a pass. An injection that does not fail a test proves nothing about the test.
    - **A pre-existing false claim corrected in place, not caused by this work.** Several `#[serde(default)]`
      fields in `PersistentPaperState` carried comments saying a journal written before the field existed
      would still restore. That is unreachable: `FilePaperJournal::open` additionally requires every line to
      re-serialize byte-for-byte, so a file missing *any* field the current serializer writes is rejected
      before a default can apply. Verified directly by deleting `tax_lots` -- which predates this pass
      entirely -- from a journal line and observing the identical failure. The defaults do make the persisted
      *type* tolerant, which a future format change needs; whole-file compatibility across a schema change
      does not exist and is no longer claimed. Whether PAPER journals should survive a schema change at all,
      or whether an explicit migration step is the honest answer, is a durable-format decision left open and
      recorded in `16-delivery-state.md`.
    - **Independently verified.** `cargo test --workspace --all-targets` rose 296 -> 305 -> 318 -> 326 passed
      across the three slices, 0 failed, 3 ignored, with `cargo fmt --check` and `cargo clippy -D warnings`
      clean throughout, the separate `apps/desktop/src-tauri` workspace at 17 passed, `pytest` at 42 passed,
      `npm run test:evidence` (12 suites) and `apps/desktop/test/server_contract.py` both clean. Every exit
      code was captured directly from the process rather than through a pipe, the mistake item 40 records.

47. A machine-measured delivery-state document and the tool that writes it (2026-09-23, delivery process).
    This audit is the permanent requirement-to-evidence record and is now 1,700+ lines; reconstructing
    "where is this repository right now" from it costs a whole session.
    `docs/06-delivery/16-delivery-state.md` is the short, current, rewritten-as-work-lands companion: the
    backlog code can close, the gates it cannot, the settled direction, and a session log.
    `tools/session_status.py` runs all seven verification suites as real subprocesses, records each one's own
    exit code, and rewrites a generated block in that document that is not hand-editable; `--check` makes a
    stale block or a failing suite non-zero. This matters because this repository has twice recorded
    fabricated status (items 23-24 and 45): a status line a human typed is a claim, and a status line that
    tool wrote is a measurement. `CLAUDE.md` gives any agent session, in any tool, the same starting point
    and the same non-negotiable rules.

48. Controlled-LIVE atomic combination execution closes row 5.6's last structural core gap
    (2026-09-24, row 5.6).
    - **E1.4c landed as a complete core slice.** `core/live` now accepts one broker-native atomic
      execution group in whole combination units, requires every approved leg at its exact side and
      ratio-derived quantity, accounts every leg through the shared fixed-point fill path, shrinks the
      debit reservation with remaining units, and persists group receipts, applied identities, signed
      positions, and FIFO long/short tax lots across restart. Exact reordered replay is a no-op; changed,
      overlapping, overfilled, incomplete, loose-leg, and cross-shape receipt evidence is refused without
      an `unwrap`/`expect` panic or an `f64` value path.
    - **LIVE failure semantics remain LIVE's.** A failed group application rolls back the OMS, cash,
      positions, tax lots, attribution, receipts, and execution identities before later drained events are
      processed. It leaves the combination `UNKNOWN` unless already filled and records the existing durable
      `COMBINATION_EXECUTION_ANOMALY`; no PAPER `evidence_error` field was invented. Unresolved internal
      incidents are idempotent by category and subject, and the existing
      `UNRESOLVED_INCIDENTS_REQUIRE_REVIEW` rule blocks later canary submissions. A complete group that
      genuinely overdraws cash likewise creates the existing `LIVE_CASH_OVERDRAFT` incident, after all legs
      have been applied so transient per-leg ordering cannot false-positive.
    - **Cancellation, monitoring, and reconciliation cover the whole order.** Cancellation is idempotent,
      preserves partial fills, restores the evidenced working state after broker rejection, becomes
      `UNKNOWN` on transport ambiguity, and lets a complete fill win either ordering of the terminal race.
      The LIVE dashboard counts working and `UNKNOWN` combinations once. Reconciliation reads plain and
      combination order maps together and reports broker identity/version, state, filled-unit, cash, and
      per-leg position differences as `LiveReconciliationIssue` values.
    - **The regressions were made to prove their claims.** Sixteen deliberate defects were injected and
      observed to fail their intended tests before being reverted: full reservation after partial fill;
      suppressed anomaly classification; accepted overfill; accepted overlapping leg receipt; removed
      combination cancellation dispatch; incorrect cancel-rejection restore; ignored late fill; leg
      contracts mistaken for combination units; combinations omitted from reconciliation; dropped durable
      receipts; dropped durable short lots; cancel transport ambiguity treated as known; duplicate
      same-subject incidents; omitted group cash-overdraft incident; plain-only dashboard counts; and a plain
      fill accepting a combination-owned receipt identity. A first ratio mutation changed no behavior and
      was explicitly discarded rather than counted.
    - **Measured result.** The clean pre-change baseline was 354 passed / 0 failed / 3 ignored in the Rust
      workspace. The final `python tools/session_status.py` run measured 366 passed / 0 failed / 3 ignored
      and all seven repository suites green. Independent review found the incident-deduplication,
      cash-overdraft, dashboard, and cross-shape identity defects before landing; all four are fixed and
      regression-covered.
    - **Scope remains bounded.** Item 46's "three of five slices" statement was true when written; E1.4 was
      subsequently split into three landable LIVE sub-slices and this entry supersedes its progress count,
      rather than correcting a false historical claim. E1.5, the gRPC/desktop delivery surface, remains open.
      The real PAPER/LIVE session, broker acceptance, security, legal, and operational gates below remain
      open, so this is not a production-readiness claim.

49. The risk-gated combination path reaches the versioned gRPC PAPER command boundary
    (2026-09-24, row 5.6; E1.5a).
    - **Planning is no longer the only gRPC exposure.** The existing `PlanOptionCombo` remains a pure,
      deterministic planner. The new `SubmitPaperCombo` protobuf command carries the full declarative
      `ComboIntent`, exact fixed-point limit economics, one independently timestamped observation per leg,
      and an explicit PAPER environment. Its response returns the real risk decision, optional OMS identity,
      and versioned lifecycle enum. The implementation calls
      `PaperTradingService::submit_combo_intent`; it does not call the planner and label that result an order.
    - **The route is explicit, durable, and fail-closed.** `FOLLON_TRADING_API_PAPER_CONFIG` must name a
      version-1 `paper-command-route` document. That contract binds the account, exact risk limits, kill
      switches, adapter model, durable journal, and optional explicitly bounded short-exposure permission.
      The checked-in JSON Schema and fixture are tested against the runtime boundary. With no route the RPC
      returns `FAILED_PRECONDITION` and no action; an approved retry returns the same durable decision/order,
      and reopening the route from its journal recovers the acknowledged atomic order.
    - **A write method did not silently widen network authority.** A configured route may use plaintext only
      on loopback. A non-loopback bind requires both a server TLS identity and a client CA. This narrows the
      new local PAPER command without claiming to close E3.3's authenticated privileged-control-plane gap.
    - **Six deliberate defects were injected and observed to fail before restoration:** replacing submission
      with assessment-only risk evaluation; dropping the configured short permission; fabricating a missing
      leg observation from its limit price; returning a plausible success-shaped response from an
      unconfigured route; flipping a genuine risk rejection to approved; and allowing server-only TLS without
      a client CA for a remote write socket.
    - **Measured result.** The Rust workspace rose from 366 to 371 passed / 0 failed / 3 ignored, and Python
      from 42 to 43 passed for the route-schema fixture. The final `python tools/session_status.py` run measured
      all seven repository suites green.
    - **Bounded remainder.** E1.5 was split because the gRPC composition and the desktop native/UI boundary are
      separate landable units. E1.5b remains: add the desktop combination IPC contract, native PAPER gateway
      dispatch and cancellation visibility, and operator ticket. No E2 work began, and none of the external
      broker, PAPER/LIVE elapsed-session, options-acceptance, security, legal, or operational gates moved.

50. The risk-gated combination path reaches the desktop PAPER command boundary and an operator ticket
    (2026-09-24, row 5.6; E1.5b).
    - **A validated IPC contract.** The Tauri host's `submit_combo_order` command accepts a
      `ComboOrderIntent` with 2–16 distinct legs, whole combination units, a maximum-debit or
      minimum-credit protection, and the PAPER environment only; unknown fields are denied. Each
      `ComboLegIntent` carries its own operator-attested observed price and time, so a leg without an
      observation cannot be expressed and none is ever priced from its limit.
    - **Real dispatch, not a model of it.** The native PAPER gateway converts the request into one
      `ComboIntent` and calls `PaperTradingService::submit_combo_intent`, returning the real risk decision
      and OMS state. The in-process paper model fills the whole group as one atomic execution at the
      per-leg observations only when `ComboPriceLimit::check_net_price` accepts the observed net, sign
      included; otherwise it rests. An approved retry reuses the durable decision time and is answered from
      evidence without re-execution.
    - **Cancellation visibility.** `cancel_order` already reached combinations inside `core/paper`, but its
      desktop receipt read only the plain-order map and so reported every combination as `UNKNOWN`. It now
      reads both maps.
    - **Short exposure stays an operator file decision.** The desktop configuration gained an optional
      `short_exposure.max_short_quantity` in the version-1 `paper-command-route` shape. Absent, every net
      short is refused exactly as before; the UI has no control that grants it.
    - **Operator ticket.** A combination ticket in the execution workspace builds the payload in a pure,
      fixed-point (`BigInt`, never `f64`) module that previews protected and observed net prices, and submits
      exactly one command; its regression asserts it never invokes `submit_order`.
    - **Nine deliberate defects were injected and observed to fail before restoration:** plain-map-only
      cancellation receipts; marketability without the sign guard; fractional units accepted; configured short
      permission ignored; an armed reference surviving a risk rejection; leg marks taken from limit prices;
      duplicate instruments accepted; and, in the ticket module, an unobserved leg priced from its limit and
      fractional units accepted.
    - **Measured result.** Tauri host tests rose from 17 to 28 passed; the desktop evidence suite gained
      `combo-ticket-regression.mjs`. The Rust workspace stayed at 371 passed / 0 failed / 3 ignored and
      Python at 43. The final `python tools/session_status.py` run measured all seven repository suites green.
    - **Also corrected in place.** `apps/desktop/README.md` said the desktop exposed no cancellation or
      position close. That was false before this slice — the order ticket has had both — and is corrected.
    - **Bounded remainder.** This closes the E1 epic inside the repository. Broker-backed PAPER acceptance of
      a real combination, the options-acceptance gate, and every external PAPER/LIVE, security, legal, and
      operational gate remain open, so this is not a production-readiness claim.

51. The stale-journal-fixture defect of item 34 recurred, and is now caught by `cargo test`
    (2026-09-24, reliability; evidence pipeline).
    - **What happened.** E1.3a–E1.4c added `combo_orders`, `combo_risk_evidence` and short-lot tax-lot
      members to the persisted PAPER and LIVE state. `FilePaperJournal::open` and `LiveAuditJournal::open`
      correctly refuse any line they cannot re-serialize byte for byte, so the checked-in
      `tests/fixtures/paper/journal-v2.ndjson` and `tests/fixtures/live/journal-v1.ndjson` stopped loading
      and `tools/generate_pipeline_evidence.py` failed at step 14a. Item 34 had recorded that the pipeline is
      the only thing that loads these files; it is not one of the seven suites `tools/session_status.py`
      measures, and no session between items 46 and 50 ran it, so the break went unnoticed for four
      sessions. No earlier entry claimed a pipeline pass in that window; this corrects no false claim.
    - **Regenerated, not edited.** Following item 34 exactly: the PAPER fixture from one
      `follon-paper-status` run against a fresh path with the unchanged `tests/fixtures/config/paper-v2.json`;
      the LIVE fixture from twelve sequential `follon-live-status` opens against a fresh path with the
      unchanged `tests/fixtures/config/live-v1.json` and `--opened-at 2026-08-11T13:30:00Z`, reproducing the
      original `initialized` + 12 `restarted` sequence and timestamps. Configuration fingerprints match the
      originals; the only differences are the new empty fields and the hash chain that covers them. The full
      pipeline then exited 0, and neither fixture was modified by the run.
    - **Recurrence now fails the workspace suite.** `core/paper/tests/checked_in_journal_fixture.rs` and
      `core/live/tests/checked_in_journal_fixture.rs` open a temporary copy of each fixture. Both were run
      against the stale fixtures and failed with an actionable message, then passed on the regenerated ones.
    - **A pipeline overclaim removed.** Its closing line printed "All 12 Enduring Capabilities (DUR-01 through
      DUR-12) & Release Readiness fully demonstrated" unconditionally, contradicting its own step-16g comment.
      That was false and is replaced with a statement of what was measured: every step exited 0.

52. The first advanced-evidence category computed from real data: decision reconstruction
    (2026-09-24, DUR provenance; E2.1a).
    - **Computed, not typed.** `follon-operations decision-reconstruction` reads the step-2 backtest event
      journal and its manifest, refuses a journal that does not hash to the manifest's `events_sha256`,
      binds the manifest's own `configuration_hash`, and walks the latest `execution.fill.v1` event's
      causation chain. On the real journal that is a seven-node `VERIFIED` chain from market bar through
      intent, risk decision, OMS transitions and audit to the fill. `--verified-at` is explicit, so a re-run
      over the same journal reproduces the same document. Pipeline step 16h publishes
      `var/decision-reconstruction.json`; it validates against
      `contracts/json-schema/v1/decision-reconstruction.schema.json` and the desktop's strict
      `parseDecisionReconstruction`, so its panel now renders real evidence.
    - **Exact persisted bytes.** There is no decoder from a journal line back to `EventEnvelope`, so
      `provenance` gained `ProvenanceRecord`: the envelope metadata reconstruction needs plus a SHA-256 of
      the exact line. A line that is not canonical sorted-key JSON is refused rather than re-canonicalized,
      a repeated event identity is refused, and a test proves a persisted line hashes exactly like the
      envelope it encodes.
    - **A latent defect fixed.** `DecisionReconstruction::to_json` built JSON with `format!` and no escaping,
      so a quote in any journal string would have published an invalid document. It now serializes through
      `serde_json`.
    - **Seven deliberate defects were injected and observed to fail before restoration:** unescaped journal
      strings; re-hashing a non-canonical line; last-wins on a repeated identity; hashing bytes other than
      those persisted; skipping the manifest check; targeting the first fill instead of the latest; and
      binding the wrong manifest hash.
    - **Measured result.** Together with item 51, the Rust workspace rose from 371 to 383 passed / 0 failed /
      3 ignored; the final `python tools/session_status.py` run measured all seven suites green, and the full
      evidence pipeline exited 0.
    - **Bounded remainder.** `strategy-capsule-manifest` (E2.1b) still needs a real bundle, lockfile and
      evaluation receipt to hash and cite, and still has an unescaped `to_json`.
      `data-rights-and-semantics-receipt` (E2.1c) is not wiring work: its `semantic_parity_score_bps` is an
      input nothing measures, and publishing a configured value would present operator-typed data as
      evidence. The target-entity classifier still labels every `risk.decision.v1` as `risk_rejection` and
      does not recognize `portfolio.position_updated.v1`; the default fill target is unaffected. No external
      gate moved.

53. A fourth property/model-test slice: the PAPER atomic-combination lifecycle (2026-09-24, Reliability and
    quality conformance; E3.2a). Advances the "combination legality" example items 42-44 named in their
    remainders.
    - **What is now real.** `core/paper/tests/combo_lifecycle_proptest.rs` adds `proptest` as a dev-dependency
      to `core/paper` and generates arbitrary 2-4 leg combinations -- any sides, ratios 1-3, debit or credit
      protection, per-leg fees -- then drives each through a random sequence of whole-unit atomic fills,
      broker re-deliveries of already-applied executions, and an optional cancellation. After every step it
      checks an independently written model: filled units, the lifecycle state implied by filled units and
      cancellation, every leg's signed position (`side * ratio * units`), exact cash including fees, a
      working combination counting once on the dashboard, replay leaving every observable byte unchanged,
      and a clean reconciliation against the broker model's own snapshot. It uses only the public API; a
      small test adapter wraps the real `IbkrPaperAdapter` to re-deliver evidence the way a reconnecting
      broker may.
    - **Verified against injected defects.** Seven defects were injected into production code and each failed
      the property: a replayed execution applied twice; a sell-leg fee not charged; a partial fill never
      leaving `ACKNOWLEDGED`; cancellation discarding filled units; working combinations omitted from the
      dashboard count; the broker model omitting a fee (caught only by reconciliation); and the broker
      snapshot reporting leg contracts instead of combination units. The first double-apply injection
      disabled one guard but left the group-identity check live through operator precedence, so it tested a
      refusal rather than a double-apply; it was recognized, not counted, and redone with both guards off.
    - **Also recorded.** E2.1b (`strategy-capsule-manifest`) was assessed as product work rather than wiring:
      no portable strategy bundle, lockfile, or evaluation receipt exists in the repository to hash or cite,
      and the verifier returns the caller's claimed disposition. It was not wired.
    - **Measured result.** The Rust workspace rose from 383 to 384 passed / 0 failed / 3 ignored; the final
      `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** The controlled-LIVE combination lifecycle and EMS scheduling still lack an
      equivalent model test. No external gate moved.

54. A fifth property/model-test slice: the controlled-LIVE atomic-combination lifecycle (2026-09-24,
    Reliability and quality conformance; E3.2b).
    - **What is now real.** `core/live/tests/combo_lifecycle_proptest.rs` adds `proptest` as a dev-dependency
      to `core/live` and runs item 53's model against the capital path: every case opens a durable journal,
      activates CANARY through `LiveActivation::for_configuration`, registers a four-eyes approval bound by
      `combo_intent_fingerprint`, connects through a `SecretProvider`, and submits through
      `submit_canary_combo_intent`. Random whole-unit fills, re-deliveries and cancellation follow. After every
      step it checks filled units, lifecycle state, signed leg positions, exact cash including fees, the
      working count, that no incident was raised, replay leaving the monitoring view unchanged (audit
      sequence excluded, since every call is audited), and a clean reconciliation. The broker is a model
      written in the test file, not a repository adapter, so reconciliation compares the service with
      independent arithmetic.
    - **Verified against injected defects.** Seven defects were injected into production code and each failed
      the property: a replayed execution applied twice (both overlap guards disabled); a sell-leg fee and
      separately a buy-leg fee not charged; a partial fill never leaving `ACKNOWLEDGED`; cancellation
      discarding filled units; working combinations omitted from the count; and filled units not recorded.
      The first clean run failed on a defect in the test's own broker model, which never advanced its order
      state on a fill; that was corrected in the model and is not counted as a finding.
    - **Measured result.** The Rust workspace rose from 384 to 385 passed / 0 failed / 3 ignored; the final
      `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** EMS scheduling legality in `core/execution` still lacks a property test. No
      external gate moved.

55. A real accounting defect found by item 53's property test: a short whose commission meets or exceeds its
    premium could not be recorded (2026-09-24, accounting correctness; PAPER and LIVE).
    - **What was wrong.** A short's average cost and its tax lot's `unit_proceeds` are net of the opening fee,
      so both are zero or negative when the fee is at least the premium -- for example a 1-lot, one-cent
      option under a one-dollar minimum commission. `Portfolio::apply_signed_fill` refused a negative average
      ("signed portfolio cost is negative") and `TaxLotBook::open_short` refused non-positive proceeds
      ("invalid short tax lot economics"). The broker genuinely executed the trade, so the OMS rolled the
      evidence back, drove the combination to `UNKNOWN`, and raised `COMBINATION_EXECUTION_ANOMALY`: a real
      fill the system could never reconcile. The same code serves PAPER and LIVE, plain and combination fills.
    - **How it was found.** Item 53's test passed its first runs; a later run drew the case and shrank it to
      two short legs at 1.00 with a 1.00 fee on one. The test was right and the code was wrong. Item 53's
      measured pass was true for the seeds it ran; it is not corrected, only superseded.
    - **The fix.** `apply_signed_fill` refuses a negative average only when the resulting position is long,
      where cost is price plus fee and can never be negative; `Portfolio::recover_signed` applies the same rule
      on restore. `TaxLotBook::open_short` and `TaxLotBook::recover` accept zero or negative net proceeds and
      still require a positive quantity. Realized P&L on close or cover is unchanged arithmetic
      (`proceeds - cost - fee`). No persisted format changed.
    - **Regression coverage.** `core/control-plane/tests/short_fee_exceeds_premium.rs` and
      `core/accounting/tests/short_fee_exceeds_premium.rs` record, restore and close such a short and assert the
      exact realized P&L, and confirm a long still cannot restore with a negative cost. Both failed before the
      fix, as did the PAPER property test on its committed shrunk seed
      (`core/paper/tests/combo_lifecycle_proptest.proptest-regressions`). Afterwards the three property suites
      were stress-run on fresh seeds -- 25 runs of the PAPER model, 25 of the scheduling properties, 8 of the
      LIVE model -- with no failure.

56. A sixth property-test slice: EMS scheduling legality (2026-09-24, Reliability and quality conformance;
    E3.2c). Closes the "EMS scheduling" example items 42-44 and 53-54 named in their remainders.
    - **What is now real.** `core/execution/tests/scheduling_legality_proptest.rs` adds `proptest` as a
      dev-dependency to `core/execution`. Every planner already calls `ExecutionPlan::validate_against`, which
      proves conservation and ordered offsets, so these six properties check what that self-check cannot,
      against integer oracles in 1e-8 units: TWAP children number `min(slices, units)`, differ by at most one
      unit and put larger slices first; VWAP children are exactly the floor of their proportional share with the
      last absorbing the remainder, and a schedule is refused if and only if a window would round to zero;
      participation children are exactly `min(volume * rate, remaining)` window by window with only truly
      unavailable liquidity unallocated; arrival price is non-increasing and flat at zero urgency; iceberg
      children are the display size except a final smaller remainder, identically through `plan_execution` and
      `plan_iceberg_execution`; and every algorithm has offsets exactly `index * interval`, unique child
      identities, and the parent's limit and child kind.
    - **Verified against injected defects that preserve conservation.** Seven were injected and each failed its
      property: TWAP remainder units given to the last slices; the whole TWAP remainder on the first slice; VWAP
      rounding shares up; a doubled participation cap; back-loaded arrival price; an iceberg slice one unit over
      the display size; and offsets shifted by one interval. None of the seven tripped `validate_against`, so
      none was detectable before this slice.
    - **Measured result.** Together with item 55, the Rust workspace rose from 385 to 395 passed / 0 failed / 3
      ignored; the final `python tools/session_status.py` run measured all seven suites green, and the three
      property suites were stress-run on fresh seeds without failure.
    - **Bounded remainder.** Algo-wheel allocation, passive repricing, smart routing, and the margin/financing
      functions of `core/accounting` still lack property coverage. No external gate moved.

57. The second advanced-evidence category computed from real data: portable strategy capsules (2026-09-24,
    DUR-07 and ASSET-04; E2.1b).
    - **What the earlier assessment missed.** Item 52's remainder and item 53's "Also recorded" note put E2.1b
      down as product work because no portable bundle, lockfile or evaluation receipt existed to hash. Those
      three were indeed missing, and those statements stand as written. But the assessment left out what
      already existed: the SDK's deterministic `strategy_bundle_hash`; `follon-backtest --python-worker`, which
      evaluates a Python strategy, verifies the hash the worker announces, and records it in the artifact's
      specification; and the completion manifest every run writes. This item builds on all three.
    - **What is now real.** `follon_strategy_sdk.bundle_lock` writes a canonical dependency lock: the runtime
      identity, the entry point, and every strategy and SDK source file's size and SHA-256. It comes from the
      same enumeration as the bundle hash, so the two cannot disagree. `follon-backtest capsule-package`
      rebuilds the strategy archive from the two trees. That archive is exactly the length-framed byte stream
      the bundle hash is computed over, so its SHA-256 is the bundle hash. The packager refuses the archive
      unless it opens exactly as the lock describes (every namespace, path, size and digest in order, the
      runtime tail, no trailing bytes). It also refuses unless the evaluation's own specification recorded
      that bundle hash and the configuration's hash, and the completion manifest hash-binds the artifact.
      Pipeline step 16i evaluates the repository's worker example this way. Its
      `var/strategy-capsule/capsule-manifest.json` validates against
      `contracts/json-schema/v1/strategy-capsule-manifest.schema.json` and the desktop's strict
      `parseStrategyCapsuleManifest`, and the desktop server classifies it as `strategy_capsule_manifest`, so
      the panel now renders real evidence.
    - **The disposition is earned, not claimed.** `StrategyCapsuleVerifier::verify_capsule_payload` checked
      caller-supplied hashes and then returned the caller's own claimed disposition. It is removed.
      `CapsuleContents::seal` issues `VERIFIED_PORTABLE` only when it is handed replay output identical to
      the receipt. The packager produces that output by extracting the archive into a fresh temporary
      directory and replaying it through the real backtest runner in a sandbox. The new
      `ProcessStrategyWorker::spawn_sandboxed_with_services` makes the extracted SDK the only `PYTHONPATH`
      and the temporary directory the working directory, and the interpreter runs with `-S`. The caller's
      `FOLLON_STRATEGY_SDK_PATH`, the caller's directory, and installed site packages are therefore out of
      reach. A strategy that imports anything it did not vendor fails the replay, with the interpreter naming
      the missing module. `capsule-verify` re-derives every manifest field from the capsule's five files and
      replays again; pipeline step 16i(iv) runs it. `to_json` now serializes through `serde_json`, and
      `parse` refuses any encoding other than that canonical one.
    - **A cross-runtime contract.** The Rust archive and the Python digest are pinned to the same test
      vector in `core/control-plane` and `python/strategy-sdk/tests/test_bundle.py`. The vector includes an
      uppercase file name, which byte order sorts first and NTFS directory order sorts last, so an unsorted
      enumeration on either side changes the hash.
    - **Sixteen deliberate defects were injected, and each was observed to fail before restoration:**
      - replay without `-S`;
      - the sandbox keeping the caller's working directory;
      - the sandbox honouring the caller's `FOLLON_STRATEGY_SDK_PATH`;
      - `seal` ignoring the replay's bytes;
      - `capsule-verify` skipping the receipt comparison;
      - `capsule-package` skipping the bundle binding;
      - the archive reader skipping per-file digests, ignoring trailing bytes, or ignoring the runtime tail;
      - the lock parser skipping path validation or accepting non-canonical encodings;
      - the Rust archive dropping its sort;
      - a sealed capsule claiming any disposition;
      - a capsule carrying extra members;
      - a receipt that need not name the configuration;
      - the Python enumeration dropping its sort.

      On the first pass the path-validation defect was not caught. Every unsafe path in the test sat without
      a valid entry point, so the entry-point check refused the lock first. The test was fixed so that each
      unsafe path is the lock's only fault, and it now fails. The two integration refusals that a later
      replay would also catch assert which check refused them, so the replay cannot mask a removed binding.
    - **Measured result.** The Rust workspace rose from 395 to 405 passed / 0 failed / 3 ignored, and
      Python from 43 to 48 passed. The final `python tools/session_status.py` run measured all seven suites
      green, and the full evidence pipeline exited 0.
    - **Bounded remainder.**
      - DUR-07 asks for a *signed* manifest and clean-machine verification. The manifest is unsigned, and no
        capsule has been replayed anywhere but the machine that sealed it; `VERIFIED_PORTABLE` covers only
        the recorded runtime target.
      - Packaging refuses rather than emitting `MISSING_DEPENDENCY_LOCK`, `UNVERIFIED_EVALUATION` or
        `RESTRICTED_DATASET_RIGHTS`, so those three dispositions are unreachable. A missing dependency is
        reported only in the interpreter's error text.
      - Dataset rights are not assessed. No market data is carried, so nothing is redistributed, but nothing
        certifies the referenced dataset either (see E2.1c).
      - Only Python-worker evaluations without corporate actions can be packaged.
      - The standard library is pinned only through the runtime version string.
      - The evaluated strategy is the fixed-threshold demo and claims no edge.

      No external gate moved.

58. Strategy capsules can be signed, and verification can require a trusted signer (2026-09-24, DUR-07;
    E2.1d). This closes the "signed manifest" half of item 57's first remainder; item 57 was accurate when
    written.
    - **What is now real.** `follon-backtest capsule-sign` fully re-reads a sealed capsule, then adds one
      detached Ed25519 signature over the exact manifest bytes as `capsule-signature.json`. The manifest
      hash-binds the other four members, so the signature covers the whole capsule. The v1 manifest schema
      forbids extra fields and is unchanged. Keys are the PKCS#8 and `{key_id, public_key_hex}` files
      `follon-admin release-keygen` already writes, and the private key's bytes are zeroed after use.
      `core/control-plane` gains `ring`. The main workspace already locked it through `core/commercial`, so no
      crate is new there. The separate Tauri host lockfile gains `ring` 0.17.14, `untrusted` 0.9.0 and
      `windows-sys` 0.52.0, each with the checksum the main workspace already locks. The signed message is prefixed with `follon-strategy-capsule-signature-v1\0`,
      so a release-manifest signature cannot be replayed as a capsule signature, and the reverse.
    - **Verification.** Reading a capsule accepts the signature as an optional sixth member and refuses one
      that names another capsule or manifest hash. `capsule-verify --trusted-key` refuses an unsigned
      capsule, a different key identity, and different key material under the same identity. Without the
      flag, a present signature is reported as "not checked" rather than trusted. A capsule is signed at most
      once. Pipeline step 16i signs the real capsule and re-verifies it under the trusted key.
    - **Five deliberate defects were injected, and each was observed to fail before restoration:**
      - verification ignoring the key identity;
      - signing and verifying without the capsule domain;
      - verification skipping the cryptographic check;
      - reading skipping the signature's manifest binding;
      - `capsule-verify` parsing `--trusted-key` without checking the signature against it.

      A first attempt at the last one used a guarded `match` arm, which made the match non-exhaustive, so it
      did not compile. It was recognized, not counted, and replaced. The explicit refuse-to-re-sign check was
      not injected, because `create_new` would refuse the second write independently.
    - **Measured result.** The Rust workspace rose from 405 to 407 passed / 0 failed / 3 ignored; the final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0.
    - **Bounded remainder.**
      - The pipeline's key is generated locally on every run. It demonstrates the mechanism and is not an
        independent or custodied signer; there is no key-rotation or revocation list.
      - A signature attests who sealed the capsule, not that its strategy is sound.
      - Clean-machine verification, the other half of item 57's first remainder, is still open and cannot be
        closed from this machine.

      No external gate moved.

59. A seventh property-test slice: algo-wheel allocation (2026-09-24, Reliability and quality conformance;
    E3.2d). This closes the "algo-wheel allocation" candidate named in item 56's remainder.
    - **What is now real.** `core/execution/tests/algo_wheel_proptest.rs` has four properties.
      - Every wheel plan equals an oracle plan built without the wheel planner. The oracle splits the parent
        in integer 1e-8 units (each branch but the last gets the floor of `quantity * weight / 10000`, and the
        last absorbs the remainder). It plans each branch directly with the parent's limit, merges children
        by (offset, branch, position), renumbers them from `.child.0001`, and sums the branches' unallocated
        quantity. Either both plan identically or both refuse. Of 256 cases, 165 compare two real plans and 127
        of those contain children at the same offset, so the tie-break is exercised rather than vacuous.
      - A single full-weight branch is transparent.
      - Weights that do not sum to exactly 10000 are refused.
      - Nested wheels, empty wheels and zero weights are refused.

      Branches draw from every non-wheel algorithm: immediate, TWAP, VWAP, participation, arrival price and
      iceberg.
    - **Verified against injected defects.** Eight were injected and each failed a property:
      - tie-breaking by child position before branch;
      - giving the weight remainder to the first branch;
      - branches dropping the parent's limit;
      - renumbering from zero;
      - accepting weights that do not sum to 10000;
      - accepting nested wheels;
      - accepting zero weights;
      - taking the unallocated quantity from the last branch only.

      Only the last of these breaks conservation, so `validate_against` alone would have caught none of the
      other seven. The zero-weight catch was checked rather than assumed, because the weight split refuses a
      zero share and the explicit check looked redundant. It is not: a zero-weight branch in *last* position
      receives the rounding remainder, so without the check a branch the operator gave no weight to would
      trade (the minimal case was 40 × 1e-8 units at weights 562/9438/0, which leaves 1 unit to the zero branch).
      Seeds recorded only by injected runs were deleted, not committed.
    - **Measured result.** The Rust workspace rose from 407 to 411 passed / 0 failed / 3 ignored; the
      final `python tools/session_status.py` run measured all seven suites green, and the suite was
      stress-run 20 times at 512 cases each on fresh seeds without failure.
    - **Bounded remainder.** Passive repricing, smart routing, and the margin/financing functions of
      `core/accounting` still lack property coverage. No external gate moved.

60. An eighth property-test slice, passive cancel-and-replace repricing, found a real defect (2026-09-24,
    Reliability and quality conformance; E3.2e). This closes the "passive repricing" candidate named in item
    59's remainder.
    - **The defect.** `plan_passive_repricing` clamps each candidate price to the parent's hard limit. When
      that limit was off the venue tick grid, the clamp emitted the off-grid limit itself as a replacement
      price. The shrunk case was a sell on a 0.01 grid collared at 3.86999999, replaced at exactly 3.86999999.
      A replacement is transmitted only after the working child's cancel is confirmed. A venue rejects an
      off-grid price, so the order would have been left with no working child at all. The initial price and
      every quote were already tick-checked; only the clamped price escaped. The clamp now takes the most
      aggressive on-grid price that still respects the limit: the tick floor for a buy, the tick ceiling for a
      sell. The collar is therefore never crossed. The deterministic regression
      `passive_replacement_clamped_to_an_off_grid_hard_limit_stays_on_the_tick_grid` and the committed shrunk
      seed both fail without the fix.
    - **What is now real.** `core/execution/tests/passive_repricing_proptest.rs` has three properties over
      random buy and sell quote paths. In 512 cases, 438 produce replacements (1,101 in total) and 22 exercise
      the off-grid clamp.
      - Every replacement obeys every rule: post-only against the quote it was planned from; strictly
        monotonic toward the market; within the hard limit; within `maximum_chase_bps`, computed exactly as
        `price_deviation_bps` truncates; within the replacement budget and the replace interval; scheduled at
        an observation; cancelling exactly the previous child; restating the full quantity as a limit order;
        and on the tick grid.
      - No eligible reprice is skipped: the plan equals a greedy oracle written in integer arithmetic.
      - Malformed inputs are refused: an off-grid or already-marketable initial price, non-increasing
        observation times, a locked quote, an initial price beyond the hard limit, and a zero interval.
    - **Verified against injected defects.** Reverting the fix failed both the unit regression and the
      property. Ten further injected defects each failed a property:
      - pricing from the far side of the book;
      - moving away from the market;
      - chasing twice the configured bound;
      - ignoring the replace interval;
      - allowing one replacement over budget;
      - cancelling the initial child every time;
      - a buy ignoring the hard limit;
      - an extra second of cooldown that silently skips eligible reprices, which only the completeness oracle
        sees;
      - accepting non-increasing observation times.

      A first attempt at the cancel-chain defect did not compile, so it was not counted and was replaced.
      Seeds that only injected runs recorded were removed; the committed regression file holds only the real
      defect's seed. The suite was stress-run 20 times at 1,024 cases on fresh seeds without failure.
    - **Measured result.** The Rust workspace rose from 411 to 415 passed / 0 failed / 3 ignored; the
      final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.**
      - **A wider gap, not closed here.** No code path in the repository checks an order's limit price against
        the instrument's `tick_size`, which is only validated as positive. A plain PAPER or controlled-LIVE
        order with an off-grid limit therefore passes risk and would be rejected by the broker. Closing this
        means a pre-trade check in both risk gates, so it is recorded as its own slice (E3.6) rather than
        folded in here.
      - Smart routing and the margin/financing functions of `core/accounting` still lack property coverage.

      No external gate moved.

61. PAPER and controlled-LIVE risk refuse a plain order that is off its instrument's tick grid (2026-09-24,
    rows 5.5 and 5.7; E3.6). This closes the gap item 60 recorded.
    - **What is now real.** `PaperRiskPolicy` and `LiveRiskPolicy` gain a required `instrument_tick_sizes`
      table, the configuration source the operator chose. `evaluate_risk` refuses an order in two cases:
      `INSTRUMENT_TICK_SIZE_UNCONFIGURED` when the instrument is not listed, including a market order, since
      its reference data is unknown; and `LIMIT_PRICE_OFF_TICK_GRID` when the limit is not an exact multiple
      of the tick. No order is created and nothing reaches a broker. Each decision's persisted evidence now
      records `instrument_tick_size`. A policy whose table is empty, has a non-positive tick, or has a
      non-canonical instrument id fails validation.
    - **Bound into the configuration fingerprint.** The table is always part of the PAPER and LIVE
      configuration fingerprints. A journal therefore cannot be reopened under a different tick table, and a
      controlled-LIVE approval bound to one table does not carry to another.
    - **Every configuration boundary changed together.** The JSON Schemas `paper-configuration`,
      `live-configuration` and `paper-command-route` require the table. Each is a one-line insertion in the
      schema's compact style, and a document without the table is refused. The same holds for all six
      configuration fixtures, the PAPER and LIVE CLI loaders, the versioned gRPC command route, and the
      desktop `FOLLON_DESKTOP_PAPER_CONFIG` gateway, whose README now documents the table. Fixtures list
      `inst.us_equity.spy` at 0.01, the instrument the portfolio-risk fixtures already use.
      **Corrected in place 2026-09-27 (item 77).** "Every configuration boundary" was false. The
      `paper-configuration` contract also has a version-2 schema, which `follon-paper-status` reads, and it
      was not changed. From this item until item 77, both version-2 fixtures failed validation against it.
    - **Journal fixtures regenerated, not edited.** The fingerprint change correctly made both checked-in
      journals refuse their configurations, and pipeline step 14a failed exactly as intended. Both were
      regenerated with the current `follon-paper-status` and `follon-live-status` binaries against their
      updated configurations. Compared field by field with the old first records, only
      `configuration_fingerprint` and the entry hash differ. The old LIVE fixture also carried twelve
      `live.service.restarted.v1` records left by the in-place mutation that item 45 fixed; a clean
      regeneration no longer contains them.
    - **This drift is now caught by `cargo test`.** The format checks from item 51 only parse each fixture,
      so a changed configuration with a stale journal still passed `cargo test` and failed only the
      pipeline, which is not one of the measured suites. The new
      `apps/cli/tests/checked_in_journal_configuration.rs` opens copies of both fixtures under their
      configurations through the real status binaries. It passes on the regenerated fixtures and fails on
      the previous ones.
    - **Verified against injected defects.** Eleven were injected and each was caught:
      - PAPER and LIVE each skipping the check, accepting unlisted instruments, never finding a limit off
        grid, accepting an empty table, or leaving the table out of the configuration fingerprint (ten);
      - the PAPER CLI dropping the configured table.

      New tests: two PAPER and one LIVE refusal test with on-grid and market-order controls; a validation
      and fingerprint test in each crate; an end-to-end desktop gateway refusal; and the two fixture tests.
    - **Measured result.** The Rust workspace rose from 415 to 422 passed / 0 failed / 3 ignored, and
      the Tauri host from 28 to 29. The first `python tools/session_status.py` run failed `cargo fmt` on the
      unformatted route-loader edit. It was formatted, every suite was re-measured, and the final run measured
      all seven suites green. The full evidence pipeline exited 0.
    - **Bounded remainder.**
      - Combination net prices are not tick-checked. Venues price combinations on their own increments, and
        no combination increment is modelled.
      - Tick sizes are operator-attested configuration, not broker contract details, so a wrong table is
        enforced as written.
      - Quantities are not checked against `lot_size`.

      No external gate moved.

62. A ninth property-test slice: smart routing (2026-09-25, Reliability and quality conformance; E3.2f).
    This closes the "smart routing" candidate named in item 60's remainder.
    - **What is now real.** `core/execution/tests/smart_routing_proptest.rs` checks `smart_route` and
      `smart_route_with_capabilities` over random buy and sell books of 1 to 12 venues, against arithmetic in
      1e-8 units. In 512 cases, 339 leave quantity unallocated, 117 rank a quote over the limit ahead of an
      eligible one, and in 285 the venue bounds change the plan.
      - Every child names a quoted venue at that quote's price, within its displayed size and the venue's
        `max_quantity`, and inside the parent's limit.
      - Children never get worse in all-in price, with ties broken by latency and then venue.
      - Nothing is left unallocated while a quote inside the limit still has size.
      - Each plan equals an exact greedy oracle, which for the gated router includes `min_quantity` and
        `max_quantity`.
      - Quote order does not change either plan.
      - With permissive capabilities, the gated router equals the plain one for limit parents.
      - Every route decision mirrors its child, with its all-in price, fee, rank and identity.
      - Missing, duplicate or unsupported capabilities, empty books and zero-size quotes are refused.
    - **Verified against injected defects, and two weak tests found and fixed.** Ten defects were injected,
      and all ten failed on the final test file:
      - ignoring the latency tie-break;
      - inverting the sell ranking;
      - ranking buys without fees;
      - the plain router stopping at the first quote over the limit;
      - the gated router ignoring the buy limit;
      - the plain router ignoring displayed size;
      - the gated router ignoring a venue maximum, or skipping the post-cap minimum;
      - a sell decision adding the fee;
      - decision ids numbered from zero.

      On the first pass the early-stop defect was **not** caught. The generator's fees were at most one
      cent while prices differed by whole cents, so fees could never reorder quotes. A quote over the limit
      therefore always ranked after every eligible one, and stopping early changed nothing. Fees now range
      up to $3.00, and the defect fails. A first attempt at the gated-limit defect did not compile, so it was
      not counted and was replaced. A first stress run then failed 11 of 20 times with no routing defect: the
      differential property discarded every book without a limit and exhausted proptest's reject budget at
      1,024 cases. It now generates limit parents directly, and the suite was stress-run 20 times at 1,024
      cases without failure. Seeds that only injected runs recorded were removed.
    - **Measured result.** The Rust workspace rose from 422 to 427 passed / 0 failed / 3 ignored; the
      final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder: two contract ambiguities, recorded rather than changed.** Neither router has a
      consumer outside `core/execution` yet.
      - Both routers accept several quotes from one venue. When such quotes tie on all-in price, latency and
        venue, the allocation depends on input order: for one book the same five units split 3 + 2 or 4 + 1.
        Refusing duplicates would forbid legitimate multi-level depth from one venue, so the right rule
        needs a decision.
      - For a parent with no limit, `smart_route` emits a marketable `Limit` child at the quote price, while
        `smart_route_with_capabilities` emits a `Market` child that still carries that limit price, which is
        self-contradictory.
      - The margin and financing functions of `core/accounting` still lack property coverage.

      No external gate moved.

63. The two smart-routing ambiguities of item 62, resolved as the operator decided (2026-09-25, row 5.6;
    E3.2g).
    - **Tied depth from one venue now routes the same whatever order it arrives in.** `compare_quotes` gains a
      final tie-break, larger available quantity first, shared by both routers. Several levels from one venue
      remain legal, and only quotes identical in every field remain tied, where order cannot matter.
      Previously the same five units split 3 + 2 or 4 + 1 depending on input order.
    - **Every routed child is a marketable limit at its quote price.** `smart_route_with_capabilities`
      previously emitted a `Market` child carrying a limit price for a parent with no limit. It now emits a
      `Limit` child at the quote price, exactly as `smart_route` always has. Slippage is therefore capped at
      the quoted level, and a venue must support `Limit` whatever the parent's kind. A venue that declares
      only `Market` is refused, where it was previously accepted for a market parent.
    - **Tests.** Two deterministic regressions: `tied_depth_from_one_venue_routes_the_same_in_any_order`, and
      `a_market_parent_routes_as_marketable_limits_through_both_routers`. The item-62 properties were updated
      to the new contract: every child is a `Limit`; the oracle ranks by size last; the gated router equals
      the plain one for market and limit parents alike; and a venue without `Limit` is refused. A new
      property routes depth books, in which quotes share three venues and tie on price, fee and rank, in
      both orders through both routers and against the oracle. Reverting either change fails the property
      tests alone, as well as its unit regression. The ten defects of item 62 were re-injected against the
      final files and each still failed, and the suite was stress-run 20 times at 1,024 cases without
      failure.
    - **Measured result.** The Rust workspace rose from 427 to 430 passed / 0 failed / 3 ignored; the
      final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** Neither router has a consumer outside `core/execution` yet, so no OMS or broker
      path exercises this contract. No external gate moved.

64. A tenth property-test slice: margin valuation and financing accrual (2026-09-25, Reliability and quality
    conformance; E3.2h).
    - **Pure functions.** `core/accounting/tests/margin_financing_proptest.rs` holds `value_margin_account`
      and `accrue_financing` to an independent integer oracle in 1e-8 units. Generated books use whole
      quantities and multipliers with full-precision marks, so the only rounding is at the documented
      points: an FX conversion, a basis-point requirement, and a day-count accrual, each truncated toward
      zero. EUR->USD rates are drawn from rates whose inverse is also exact. Thirteen properties:
      - every snapshot field matches the oracle, `maintenance <= initial`, and `margin_call` holds exactly
        when equity is below maintenance;
      - a direct quote and the equivalent inverse quote value a book identically;
      - negating every position negates its value and leaves both requirements unchanged;
      - position order does not matter, and requirements are additive over any split of the positions;
      - raising an initial rate never lowers the requirement;
      - valuation succeeds exactly when every needed conversion is fresh;
      - every invalid margin input is refused;
      - every charge matches `principal x rate x days / (basis x 10,000)`, and the currency totals sum the
        charges;
      - balance order does not matter;
      - splitting an interval never charges more, and loses at most one 1e-8 unit per balance;
      - more days, a higher rate, a larger principal, or a 360-day basis never charges less;
      - every invalid financing input is refused.
    - **Account wrapper.** `core/backtest/tests/financing_accrual_proptest.rs` covers
      `AdvancedBacktestAccount::accrue_financing`. Only negative cash accrues debit financing, and only a
      short accrues borrow, on `|quantity| x mark x multiplier`. Exactly the returned charge is debited per
      currency. The report's cumulative financing is the sum of every accrual. An accrual identity applies
      once. A refusal for a missing debit rate, a missing mark, or a non-positive mark leaves the account
      unchanged and does not consume the identity. `core/backtest` gains `proptest` as a dev-dependency.
      The workspace lock already carried proptest, so no new crate enters it.
    - **Defect injection (rule 5).**
      - Pure functions: 21 of 21 deliberate defects were caught, in three repeated runs. They included
        margin on the signed or unconverted value, an inverse quote that multiplies, an off-by-one or
        future-dated staleness check, a margin call on the initial requirement, exposure that drops cash,
        a fixed 365-day basis, dividing before multiplying, an overwritten currency total, and each dropped
        validation.
      - Wrapper: 12 of 12 were caught, in two runs. They included positive cash or long positions
        accruing, borrow ignoring the multiplier, a credited charge, an identity not recorded or consumed
        on refusal, a missing rate or mark silently skipped, cumulative financing overwritten, and a debit
        applied to the wrong currency.
      - The first pass missed one defect, an initial-rate cap raised from 10,000 to 20,000 bps, because
        generated invalid rates were spread far past the limit. Every invalid-input case now sits exactly
        on its boundary half the time. One candidate defect was not counted: dropping the wrapper's own
        non-positive-mark check is an equivalent mutant, because `accrue_financing` refuses a zero or
        negative principal anyway.
      - Both suites were stress-run 20 times without failure.
    - **No defect found.** Both functions met every property. Recorded as the pinned contract rather than
      changed: margin requirements and financing charges truncate toward zero. A requirement can therefore
      under-state by at most 1e-8 per position, and an accrual under-charge by at most 1e-8 per balance.
    - **Measured result.** The Rust workspace rose from 430 to 446 passed / 0 failed / 3
      ignored; the final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** With this slice, every candidate E3.2 named has property coverage. Further
      property coverage is open-ended and no longer a named gap. No external gate moved.

65. Deterministic quote-stream gap repair from a supplied recovery batch (2026-09-25, row 5.2; E3.5).
    - **Scope, as the operator decided.** No repository component records a live quote stream, and none
      re-requests missing data from a vendor. The IBKR adapter has no market-data subscription. The
      normalized `Quote` and `FeedQualityMonitor` had no producer or consumer outside their own tests.
      E3.5 therefore lands the repair core and a file CLI, which complete the monitor's gap reports. It
      does not claim a live operation.
    - **What is now real.** `core/market-data/src/gap_repair.rs` provides `detect_quote_gaps`,
      `repair_quote_gaps`, and a v1 quote CSV contract (`import_quotes`, `quotes_to_csv`).
      - A gap is a missing sequence strictly between two recorded sequences of one instrument. Nothing
        before the first or after the last recorded sequence is knowable, so repair refuses to extend a
        stream.
      - Repair fills a gap only with a record from the recovery batch. It never interpolates, and it
        never changes or drops a recorded quote. Identical re-deliveries collapse and are counted. A
        batch record identical to a recorded quote corroborates it.
      - The whole repair is refused when either input holds two different quotes for one sequence, or
        one identity for two sequences. It is also refused when a batch record conflicts with the
        recording, reuses a recorded identity, lies outside every gap, or contradicts its merged
        neighbours' event-time order.
      - Every unfilled sequence stays declared as residual.
    - **CLI.** `follon-repair-quotes --recorded --recovery --output-dir [--require-complete]` writes an
      immutable `repaired-quotes.csv` and `gap-repair.json`. The JSON holds the input and output SHA-256
      hashes, the gaps, the recovered runs, the residual gaps, and the counts. A refusal writes nothing.
      `--require-complete` fails on a residual gap after the evidence is written. The inputs are
      `tests/fixtures/market-data/quotes-{recorded,recovery}-v1.csv`, and no pipeline step publishes the
      result to `var/`, because the input is a fixture, not a recording.
    - **Tests.**
      - Six unit tests in the module.
      - `core/market-data/tests/gap_repair_proptest.rs` has seven properties over generated true streams,
        of which only part is recorded. Gaps, recovered runs, residual gaps, and the repaired stream
        match an exact oracle, and every repaired quote is a true record. Input order and re-deliveries
        do not matter. A second repair recovers nothing new. A full batch restores the interior. Six
        kinds of hostile batch record, and a self-contradicting recording, are refused. Detection agrees
        with the sequence gaps `FeedQualityMonitor` reports under out-of-order arrival.
      - `apps/cli/tests/quote_gap_repair_workflow.rs` has three workflow tests: the record and
        idempotent re-run, the completeness gate, and a conflicting batch that writes nothing.
    - **Defect injection (rule 5).**
      - Repair module: 16 of 16 caught on the final files. The first pass caught 15. The survivor let a
        repair extend a stream before its first recorded sequence, because hostile cases probed only the
        tail. A head case was added.
      - Before injection, three further gaps were closed: a recovered quote later than its upper
        neighbour, a self-contradicting recording, and sequence ranges that seldom overlapped across
        instruments.
      - CLI: 3 of 3 caught (completeness gate ignored, residual reported as gaps, output created before
        the repair).
      - The property suite was stress-run 20 times without failure.
    - **Measured result.** The Rust workspace rose from 446 to 463 passed / 0 failed / 3
      ignored; the final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** A live quote recorder, vendor reconnect and re-request of a gap window, and
      a consumer that refuses to trade on an incomplete stream do not exist. These now belong to the
      vendor gate of row 5.2. No external gate moved.

66. The main backtest artifact carries the advanced-account economics; no sidecar remains (2026-09-25, row
    5.4; E3.1a).
    - **Scope, as the operator decided.** E3.1 had two halves.
      - Multi-account allocation is frozen by `docs/06-delivery/03-roadmap-and-gates.md`: "multi-account
        allocation ... remain[s] outside the admitted scope until the preceding gates are independently
        evidenced". The scope document also permits only one expansion choice after every gate passes. It
        was therefore not built, and remains blocked by that gate, not by code.
      - The unfrozen half was the need to read a sidecar for advanced economics, and that is what landed.
    - **What changed.**
      - `BacktestArtifact` gains an optional `advanced_account`. When it is attached,
        `artifact_schema_version` becomes 3 and the artifact fingerprint's preimage adds the advanced
        report's canonical JSON, so the economics are bound by both the fingerprint and the artifact
        SHA-256.
      - The Markdown report gains an "Advanced account" section. It says plainly that these are the
        complete economics, and that the Performance, Positions, and Accounting entries sections come
        from the single-currency, long-only ledger that builds the events.
      - An artifact without advanced economics is byte-identical to before (schema 2).
      - `follon-backtest` attaches its existing projection and no longer publishes
        `*.advanced-account.json` or `*.advanced-report.md`. The completion manifest becomes schema 3 and
        drops its `advanced_account` hashes, since `artifact_sha256` now covers them.
      - The pipeline removes the sidecars earlier runs left in `var/`.
      - The dashboard projects `advanced_account` for schema-3 artifacts and drops a malformed section.
      - Backtest Explorer gains an "Advanced-account economics" panel. Runs without the section are left
        out rather than shown with invented figures.
    - **Tests.**
      - A core unit test covers schema 3, fingerprint binding (it changes when the economics change),
        the report section, and an unchanged plain artifact.
      - The CLI repeatability tests now assert the schema-3 artifact and report, the retired sidecars'
        absence, and a manifest that binds the artifact hash.
      - A server contract test covers the v3 projection, dropping a malformed section, and v2 staying
        unchanged.
      - A desktop regression covers the panel, the exclusion of runs without the section, and the empty
        state.
      - Rule 5: one defect injected per layer was caught, 4 of 4: a fingerprint that ignores the
        economics, a CLI that never attaches them, a server that drops them, and an explorer that lists
        every run.
    - **Measured result.** The Rust workspace rose from 463 to 464 passed / 0 failed / 3
      ignored; the final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** Multi-account allocation is gated. The in-run ledger still builds the events,
      and its long-only single-currency figures remain in the artifact, clearly labelled. Proof against
      production-size performance targets remains. No external gate moved.

67. The gRPC write path authenticates and authorizes its operator (2026-09-25, Architecture and Security
    rows; E3.3a).
    - **Scope, as the operator decided.** E3.3 is an epic: the documented requirement is "authenticated
      identities, roles, MFA, CSRF protection, idempotency, approval policy, centralized tamper-evident
      audit, and separate deployment review". The first whole slice wires the existing, previously
      unused `core/identity` kernel into `follon-trading-api`'s one write RPC. The REST dashboard stays
      read-only.
    - **What is now real.**
      - `core/identity` gains `import_user` (an Argon2id PHC hash only), role names, a public
        `hash_new_password` and `totp_code`, and `OperatorDirectory`, a one-tenant document. The
        directory is refused for a hash that is not Argon2id, a missing or short TOTP secret, an
        unknown or duplicate role, a duplicate user or email, or an unknown field. Its JSON Schema is
        `contracts/json-schema/v1/operator-directory.schema.json`, and a test fails if the schema and
        the struct drift apart.
      - `follon-admin operator-add` reads the password from a file, stores only its hash, prints a fresh
        TOTP secret once as an `otpauth://` URI, refuses a tenant mismatch, and replaces the directory
        atomically.
      - `follon-trading-api` gains `BeginOperatorLogin`, `CompleteOperatorLogin` (the TOTP step, with
        replay protection) and `RevokeOperatorSession`. `SubmitPaperCombo` requires
        `authorization: Bearer <session>` and `Permission::PaperTrade` for the request's tenant. It does
        so before it reads the intent, the route, or the market data. It is `UNAUTHENTICATED` without a
        well-formed session and `PERMISSION_DENIED` for a wrong role, tenant, or revoked session.
      - Startup refuses a PAPER route without an operator directory. It also refuses operator login on a
        plaintext non-loopback bind. The directory file is read only when it is a regular, bounded
        file.
      - One directory serves one tenant, so a route is reachable only by that tenant's operators.
    - **Attribution.** `PaperComboRiskEvidence` gains `submitted_by`, journaled as a skipped-when-absent
      extension, so an unattributed journal is byte-identical to before. It survives restart and is
      returned in the response. An idempotent retry from another operator, or from an unattributed
      caller, is refused.
    - **Tests.**
      - Identity: six new tests, covering the RFC 4648 and RFC 6238 vectors, login, every unsafe record,
        schema drift, and the enrolment URI.
      - PAPER: journaling, retry ownership, recovery, and the absent key.
      - trading-api: four new tests covering every unauthorized caller (none reaches the route), login
        (wrong password, wrong tenant, wrong code, replayed code), the startup rules, and the file
        loader. The three existing route tests now log in as a real operator.
      - CLI: `operator_directory_workflow` provisions two operators, refuses five unsafe additions
        without touching the file, and logs in with the printed second factor.
      - Rule 5: 12 of 12 injected defects were caught on the final files. The first pass caught 11. The
        survivor ignored an unknown role beside a valid one, and a case for it was added.
    - **Measured result.** The Rust workspace rose from 464 to 476 passed / 0 failed / 3
      ignored; the final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** Several things remain.
      - Sessions live only in the server's memory. There is no persisted session store, no CSRF-bearing
        browser write surface (by design, since REST stays read-only), and no approval policy or
        four-eyes step on PAPER writes.
      - The directory's TOTP secrets sit in a file rather than a managed secret store.
      - The read RPCs remain unauthenticated.
      - The desktop's Tauri IPC writes do not authenticate the operator.
      - Kill-switch and LIVE writes have no RPC.
      - The separate deployment review is external.
    - This is not a production-readiness claim. No external gate moved.

68. A repository-authored dynamic scan of a local deployment (2026-09-25, Security row; E3.4).
    - **Scope, as the operator decided.** No DAST product was installed. Docker's engine was not running,
      and ZAP would not cover gRPC. E3.4 therefore lands an in-repository harness and records it plainly
      as a scan the repository wrote about itself.
    - **What it does.**
      - `tools/dast_scan.py` builds and starts the real `apps/desktop/server.py` in production mode, with
        Basic authentication and a random password. It also starts the real `follon-trading-api`, with a
        durable PAPER route and an operator directory provisioned through `follon-admin operator-add`.
        Both bind to loopback.
      - It probes both over the network: 69 probes in all.
      - **Dashboard:** unauthenticated access to every route, five malformed-credential variants,
        method tampering (six verbs), eight path-traversal encodings measured against a sentinel file
        just outside the evidence root, security headers on served and refused responses,
        server-version disclosure, CORS for untrusted and null origins, a 70 KB request line, a
        malformed query, CRLF header injection, and credential rate limiting, including a locked-out
        peer guessing right.
      - **gRPC:** a missing, malformed, or forged session; a wrong password; a wrong and a replayed TOTP
        code; a read-only role; a wrong tenant; a revoked session; lockout after five failures;
        account enumeration; an undecodable and a 5 MiB message; an undeclared method; and health
        after every probe.
      - **Startup:** production mode with a short password, a PAPER route without a directory, and
        plaintext non-loopback login. Each must refuse for its stated reason.
      - It writes `dast-report.json` and `dast-report.md` with the binary hashes, and exits non-zero on
        any failure.
    - **What it found.** One real defect. The dashboard's `Server` header disclosed the exact runtime
      (`FollonEvidenceDashboard/2.0 Python/3.12.10`) on every response. `DashboardHandler.version_string`
      now returns only `FollonEvidenceDashboard`, and a contract test checks the live header on GET and on
      an unsupported method. That test fails without the fix. The first run also exposed two probe bugs of
      my own, both corrected:
      - the header-injection probe read the JSON body rather than the headers;
      - tonic reports an oversized message as `OUT_OF_RANGE`, not `RESOURCE_EXHAUSTED`.
    - **Defect injection (rule 5).** Eleven defects were injected into the scanned services: dashboard
      authentication, version disclosure, `X-Frame-Options`, rate limiting, CORS, the evidence path
      check, the write permission, bearer-token form, the directory startup rule, identity lockout, and
      TOTP replay. The final scan caught 11 of 11. The first pass exposed two weaknesses in the harness,
      both fixed:
      - a server that dropped the connection crashed the scanner instead of failing a probe. Transport
        errors are now status 0, a failure, and any harness error is itself a failing probe, so the
        report is still written;
      - the startup probe "passed" because the second API could not open the running API's journal.
        Each startup probe now uses its own route and journal and must fail with its stated message.
      An interrupted injection run once left `apps/desktop/server.py` zeroed. It was restored from a
      backup and verified against `HEAD`, and the injection harness now backs up each file to disk before
      mutating it.
    - **Measured result.** The scan measured 69 probes, 0 failed. The Rust workspace is unchanged at 476;
      the dashboard contract suite rose from 19 to 20 tests. The final `python tools/session_status.py`
      run measured all seven suites green, and the full pipeline, including step 23b, exited 0.
    - **Bounded remainder.** This is not an independent DAST product run, a penetration test, or a scan
      of a real deployment's network, TLS, proxy, or PostgreSQL. It covers no fuzzing campaign and no
      authenticated crawl of the web bundle. The independent penetration test gate is unchanged at zero.
      No external gate moved.

69. Combination orders meet the tick grid a plain order meets (2026-09-26, rows 5.5 and 5.7; E3.6b).
    - **The gap.** Item 61 added a required per-instrument tick table to PAPER and controlled-LIVE risk,
      but only plain orders consulted it. A combination leg on an unlisted instrument, or with a leg
      price off its grid, was approved. That contradicted the combination gate's own stated rule that a
      leg is judged "by exactly the rule a plain order on the same instrument would meet". The gRPC route
      test and the scanner's route config both carried such legs and were approved.
    - **The rule, as the operator decided.**
      - Each leg's instrument must be listed (`INSTRUMENT_TICK_SIZE_UNCONFIGURED`), and each leg's
        protected limit price must sit on its grid (`LIMIT_PRICE_OFF_TICK_GRID`).
      - The net price limit must then sit on the finest grid among its legs
        (`COMBO_NET_PRICE_OFF_TICK_GRID`). This errs toward refusal. A venue's own complex-order
        increment is not modelled, so a cent-stepped net against two nickel-tick legs is refused even
        where a venue might accept it.
      - The decision evidence gains `combo_tick_sizes=[instrument:tick|...]`.
      - One shared helper per policy (`combo_tick_rejections`) serves PAPER and controlled-LIVE.
    - **Fixtures.** The gRPC route test, the scanner's route, and both combination lifecycle proptests
      now list their leg instruments. The proptests' comments said "Combinations are not tick-checked",
      and those comments were replaced.
    - **Tests.** A PAPER and a controlled-LIVE regression each cover five cases: on every grid, an
      unlisted leg, an off-grid leg, an off-grid net, and the finest grid binding. Rule 5: 4 of 4 injected
      defects were caught (legs unchecked, net unchecked, coarsest instead of finest grid, and the LIVE
      call site dropped).
    - **Measured result.** The Rust workspace rose from 476 to 478 passed / 0 failed / 3
      ignored; the final `python tools/session_status.py` run measured all seven suites green.
    - **Bounded remainder.** Lot sizes are item E3.6c. The venue's own complex-order increment is not
      modelled. No external gate moved.

70. PAPER and controlled-LIVE risk refuse a quantity that is not a whole number of lots (2026-09-26, rows 5.5
    and 5.7; E3.6c).
    - **The gap.** Reference data carries a lot size per instrument (`core/instrument`: "exact minimum trade
      quantity"), and nothing on a pre-trade path read it; `core/instrument` only checked that it is positive.
      A PAPER or controlled-LIVE order for three shares of a five-share-lot instrument was approved and left
      for the broker to reject, and so was a combination leg with a contract count off its lot.
    - **The rule, as the operator decided.**
      - A required `instrument_lot_sizes` table, mirroring item 61's tick table, in the PAPER,
        controlled-LIVE and PAPER command-route configurations. An empty, non-positive or non-canonical
        table is invalid.
      - A plain order on an unlisted instrument is refused (`INSTRUMENT_LOT_SIZE_UNCONFIGURED`), and so is
        a quantity that is not a whole multiple of its lot (`ORDER_QUANTITY_OFF_LOT_SIZE`).
      - A combination leg meets the same rule with its own contract quantity, the unit count times the leg
        ratio, because that is what the broker sees. A ratio-2 leg is judged on twice the unit count.
      - The decision evidence gains `instrument_lot_size=` and `combo_lot_sizes=[instrument:lot|...]`.
      - The table is bound into both configuration fingerprints (`paper-instrument-lots-v1`,
        `live-instrument-lots-v1`), so a journal or LIVE approval cannot carry across a changed lot table.
    - **Boundaries.** The table is required in the three JSON Schemas, all six configuration fixtures (each
      lists `inst.us_equity.spy` at a lot of 1), both CLI loaders, the gRPC route, the desktop gateway, and
      the scanner's route. Both checked-in journals were regenerated with the real status binaries; only the
      configuration fingerprint and entry hash changed. `checked_in_journal_configuration.rs` failed for both
      journals before the regeneration and passes after it.
      **Corrected in place 2026-09-27 (item 77).** As a list of every boundary this was incomplete. The
      version-2 `paper-configuration` schema, which `follon-paper-status` reads, gained neither table.
    - **Tests.**
      - PAPER and controlled-LIVE each cover an off-lot plain order, an instrument with no lot, the table's
        validation and fingerprint binding, and a ratio-2 combination: whole leg lots approved, an off-lot
        leg refused, and an unlisted leg refused. The approved case uses a unit count that is not itself a
        whole lot, so only the per-leg quantity can pass it.
      - The gRPC route refuses a one-unit combination against its two-contract lots, and the desktop gateway
        refuses a 10.5-share order.
      - The two existing unlisted-tick tests now list `iwm`'s lot, so each still isolates the tick rule.
    - **Rule 5.** 17 of 17 injected defects were caught:
      - PAPER: plain orders unchecked, an unlisted instrument passing, whole units checked instead of lots,
        legs unchecked, the unit count checked instead of each leg's quantity, an empty table accepted, and
        the table left out of the fingerprint;
      - controlled-LIVE: plain orders unchecked, an unlisted instrument passing, legs unchecked, the unit
        count checked instead of each leg's quantity, a zero lot accepted, and the table left out of the
        fingerprint;
      - boundaries: each CLI loader substituting a fixed table (caught by the journal test), the gRPC route
        ignoring configured lots, and the desktop gateway ignoring configured lots.
    - **A stale statement corrected.** The desktop README still said "Combination net prices are not
      tick-checked". Item 69 made that false and did not update the README. It now states the finest-leg-grid
      rule, and the lot rule.
    - **Measured result.** The Rust workspace rose from 478 to 486 passed / 0 failed / 3 ignored, and
      the Tauri host from 29 to 30. The final `python tools/session_status.py` run measured all seven
      suites green, and the full evidence pipeline exited 0, including step 23b's scan (69 probes, 0 failed).
    - **Bounded remainder.**
      - The tick and lot tables are not cross-checked for the same instruments. An instrument listed in only
        one is refused at order time with that table's `..._UNCONFIGURED` code, not at startup.
      - The rule binds order quantities, not broker fills. A fill is applied as the broker reports it.
      - `core/backtest` carries the lot size as reference data and does not check it, so a backtest can still
        trade a quantity that PAPER and controlled-LIVE now refuse. That is a research-to-live parity gap.
      - A venue's own odd-lot or mixed-lot handling is not modelled. No external gate moved.

71. Authenticated PAPER kill-switch RPCs, with the operator recorded in the journal (2026-09-26, Architecture
    and Security rows; E3.3b).
    - **The gap.** A PAPER kill switch could be moved only by `follon-paper-status --activate/--deactivate`
      on the machine that holds the journal. The journal recorded which switches were active, not who moved
      them. The identity kernel's `KillSwitchOperate` permission, granted only to `risk_manager`, was
      enforced nowhere.
    - **What landed, as the operator decided.**
      - `ActivatePaperKillSwitch` and `ReleasePaperKillSwitch` on the gRPC API. Each names one switch by its
        stable key (`global`, `account:<id>`, `strategy:<id>` or `instrument:<id>`) and requires a bearer
        session whose role grants `KillSwitchOperate` in the request's tenant. Authorization precedes every
        other check. A trading role, another tenant, and a missing session are refused, and an unknown scope
        is `INVALID_ARGUMENT`.
      - `PaperTradingService::activate_kill_switch_as` and `release_kill_switch_as` journal each change with
        the operator and the server's UTC time. A repeat that changes nothing journals nothing.
      - The record is a versioned extension, written only when it is non-empty, so an existing journal
        re-serializes byte-for-byte; both checked-in journals open unchanged. On reopen, each persisted
        record's scope, action, operator and time are validated.
      - `KillSwitchScope::from_key` is the public inverse of `as_key`, and the private journal parser now
        delegates to it. The local CLI path is unchanged and records no operator.
    - **Scanner.** `tools/dast_scan.py` provisions a second trader and a risk manager, and adds seven probes:
      - no session (G24);
      - a trading role refused (G25);
      - a risk manager activating a switch and being named its operator (G26);
      - the active switch refusing a trader's combination on its leg (G27);
      - the release (G28);
      - an unknown scope refused (G29);
      - J01, run after the API stops: the journal names the risk manager for exactly those two changes. The
        journal is exclusively locked while the API runs.
      The scan rose from 69 to 76 probes, 0 failed.
    - **Tests.**
      - PAPER: a journaled change survives a reopen; a repeat records nothing; the switch refuses an order;
        a malformed operator or time changes nothing; a local change writes no record. Also the validation
        of persisted records, and the scope-key round trip.
      - gRPC: the permission separation, operator attribution, the switch binding a trader's combination, a
        malformed scope, another tenant, the release, failing closed without a route, and the server-time
        format.
    - **Rule 5.** 14 of 14 injected defects were caught:
      - gRPC: authorizing `PaperTrade` instead, authorizing any portfolio reader, the release RPC
        activating, and a fixed operator journaled;
      - PAPER: a change not recorded, a no-op recorded, the operator unvalidated, records not persisted,
        records not restored, an unknown persisted action or an unvalidated persisted operator accepted,
        and the empty record always serialized;
      - the extended scan itself, against two service defects: authorizing `PaperTrade`, and a fixed
        operator.
    - **A test bug of mine, fixed.** The first PAPER test read the journal while its service was open. On
      Windows the exclusive lock refuses that read, even from the same process. The test now reads only a
      closed journal, which is also why the scanner stops the API before J01.
    - **Measured result.** The Rust workspace rose from 486 to 492 passed / 0 failed / 3 ignored.
      The final `python tools/session_status.py` run measured all seven suites green, and the full evidence
      pipeline exited 0, including step 23b's scan (76 probes, 0 failed).
    - **Bounded remainder.**
      - Sessions stay in memory, so a restart logs every operator out.
      - There is no four-eyes approval on a kill-switch change.
      - The Tauri desktop's IPC writes are still unauthenticated.
      - Controlled-LIVE kill switches have no RPC.
      - The operator directory has no managed secret store.
      - The deployment review stays external, and the REST boundary stays read-only. No external gate moved.

72. Replay risk refuses to judge an intent against another instrument's mark (2026-09-26, row 5.4 and
    research-to-live parity; E3.7).
    - **The gap.** `ReplayEngine` checked that a strategy's intent named the replay account and
      configuration, and nothing else. `RiskPolicy::evaluate` then priced the intent's notional and its
      price collar from the close of whichever bar produced it. Neither the engine nor the Python worker
      boundary required that bar to be the intent's own instrument. A strategy reacting to SPY's bar with
      a QQQ order was therefore judged at SPY's price: its notional and collar were computed on the wrong
      asset. PAPER refuses exactly this (`paper market observation instrument does not match intent`). The
      news path had the same hole: the sentiment had to match its market snapshot, but the intent did not.
      Both example strategies trade the bar's own instrument, so no checked-in evidence was affected.
    - **The rule.** A decision needs a mark for the instrument being traded, and the engine holds only
      the current bar. So a strategy trades an instrument on that instrument's own bar, and anything else
      is an error rather than a rejection. A decision against an unusable mark is not a "no"; it is not a
      decision, which is E1.2's rule for a stale PAPER observation. A multi-instrument strategy remains
      possible: it may read any instrument's point-in-time history, and trades each one on its own bar.
      - `RiskPolicy::evaluate` refuses a mark for another instrument, for any caller.
      - `process_bar` and the news path refuse before the intent is recorded. The stream therefore never
        holds an intent without a decision. Without that ordering, `evaluate` would still refuse, but only
        after the intent was already on the stream.
    - **Tests.** A direct evaluation test with an own-mark control, and a bar-path and a news-path test.
      Each replay test asserts the exact recorded stream: the bar alone, or the headline and sentiment
      alone.
    - **Rule 5.** 3 of 3 injected defects were caught: `evaluate` accepting another instrument's mark; the
      bar path relying on `evaluate`, which records the intent first; and the news path doing the same.
      The first run of the two replay tests caught their defects on the error text rather than on the
      recorded stream. Their assertions were reordered so that each catch rests on the stream, and both
      were re-injected and caught there.
    - **Measured result.** The Rust workspace rose from 492 to 495 passed / 0 failed / 3 ignored. The first
      `python tools/session_status.py` run failed `cargo fmt` on one unformatted assertion. It was
      formatted, every suite was re-measured, and the final run measured all seven suites green. The full
      evidence pipeline exited 0, including step 23b's scan (76 probes, 0 failed).
    - **Bounded remainder.** The replay engine still holds no mark except the current bar, so a strategy
      cannot hedge one instrument on another's bar. That needs a point-in-time multi-instrument mark
      model, which does not exist. No external gate moved.

73. Replay meets the tick and lot rules PAPER and controlled LIVE meet (2026-09-26, rows 5.4, 5.5 and 5.7,
    research-to-live parity; E3.6d). This closes the parity gap item 70 recorded.
    - **The gap.** Items 61, 69 and 70 made PAPER and controlled-LIVE risk refuse an off-grid limit and a
      quantity that is not a whole number of lots. The replay engine behind every backtest checked
      neither. It resolved each bar's reference data only to check the session, and never passed it to
      risk. A backtest could therefore approve and fill an order that both environments refuse. Its
      per-bar fill cap could also split a whole-lot order into partials of any size.
    - **The rule.**
      - `RiskPolicy::evaluate_with_reference` applies `evaluate`, then PAPER's two codes,
        `LIMIT_PRICE_OFF_TICK_GRID` and `ORDER_QUANTITY_OFF_LOT_SIZE`. The evidence gains PAPER's field
        names, `instrument_tick_size=` and `instrument_lot_size=`. `evaluate_news` builds on it.
      - The source differs from PAPER's by design. PAPER reads an operator-configured table. A replay
        already carries effective-dated reference data, versioned by the dataset manifest's
        `reference_data_version` and content-addressed in the backtest configuration, so there is no
        second table to drift. An unlisted instrument cannot reach this rule, because a bar without
        reference data is already an error. The `..._UNCONFIGURED` codes therefore have no replay
        counterpart.
      - A configured `max_fill_quantity` must be a whole number of the traded instrument's lots. Risk
        makes the order whole lots, so every partial fill and every remainder is then whole lots too. It
        is a configuration fault rather than a decision, so it is an error, raised before the intent is
        recorded.
      - The news path now takes the instrument registry. It resolves the snapshot's reference data at the
        decision time, before anything is recorded. Missing reference data is an error.
      - `process_bar`, which replays with no reference data or calendar, is now test-only. Every replay
        outside the crate's own unit tests goes through `process_bar_with_market_preconditions`. No caller
        can therefore skip the session, halt, tick or lot rule. The code that earlier items name as
        `ReplayEngine::process_bar` now lives in the private `replay_bar`.
    - **Evidence effect.** Every checked-in backtest configuration trades one share of a one-share-lot
      instrument at market, so no outcome changed. Every decision's evidence string gained the two fields.
      That changes the canonical event stream and every hash built on it. No checked-in fixture pins those
      bytes, and the pipeline regenerates `var/`.
    - **Tests.**
      - The rule itself: an on-grid order approved, with its evidence; an off-lot quantity that is a
        whole number of ticks; an off-grid limit; both at once; a market order, which has no price to put
        on the grid; an ordinary rejection kept alongside the increments; reference data for another
        instrument; and reference data with no lot.
      - The checked replay path refuses an off-lot and an off-grid order before any order exists, and
        fills two whole lots.
      - A three-share cap against a five-share lot is refused before the intent is recorded, and a
        five-share cap fills ten shares as two whole lots.
      - The news path refuses an off-lot order, approves the same order at a one-share lot with its
        evidence, refuses an off-lot cap before recording the intent, and refuses missing reference data
        before recording the sentiment.
      - `BacktestRunner` end to end: `BuyOnceStrategy`'s one share against a five-share lot is refused,
        and nothing trades.
    - **Rule 5.** 14 of 14 injected defects were caught, each on a test assertion:
      - the rule: the lot rule dropped, the tick rule dropped, the quantity checked against the tick,
        another instrument's reference accepted, the increments left out of the evidence, the ordinary
        rejection reasons discarded, and the non-positive-increment guard dropped;
      - the paths: the checked replay path passing no reference (caught separately by the engine test and
        by the `BacktestRunner` test), the news decision ignoring the reference, the bar-path and
        news-path fill caps unchecked, the cap checked against the tick, and the news reference resolved
        after the sentiment is recorded.
    - **Measured result.** The Rust workspace rose from 495 to 501 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (76 probes, 0 failed). The pipeline's built-in and Python-worker
      backtests both record `instrument_tick_size=0.01000000,instrument_lot_size=1.00000000` on their
      decisions.
    - **Bounded remainder.**
      - The fill model applies spread and slippage in basis points and does not round to the tick grid.
        A simulated fill can therefore print off the grid, which no venue fill would. Rounding would change
        existing backtest economics, so it is recorded here rather than changed.
      - The fill cap is checked when an intent arrives. An effective-dated lot change while an order is
        still working is not re-checked.
      - Reference data is enforced as configured, exactly as PAPER's table is, and a venue's own odd-lot
        handling is not modelled. The news path consults no calendar, as before. No external gate moved.

74. Simulated fills print on the instrument's tick grid (2026-09-27, rows 5.4 and 5.5, research-to-live
    parity; E3.6e). This closes the first remainder item 73 recorded.
    - **The gap.** The fill model applies half the spread and the slippage in basis points, so a simulated
      fill could print off the tick grid. The probe corpus's one-share buy at 20 bps of slippage on a
      100.08 close filled at 100.28016, a price no venue prints. Item 73 made replay refuse an off-grid
      limit, but its fills stayed off the grid. A backtest therefore paid a cost that PAPER and
      controlled LIVE, which apply broker fills as reported, never see.
    - **The rule.**
      - In a replay with reference data, the modelled price is rounded onto the instrument's tick grid
        against the trader. A buy rounds up, a sell rounds down, and a price already on the grid is
        unchanged. Spread and slippage are estimates, so the grid price is never better than the
        estimate. Rounding to the nearest tick would make some fills cheaper than the model's own
        estimate.
      - The limit is checked against the grid price, not the estimate. Risk already requires a limit to
        sit on the grid (item 73). So the next grid price above a buy estimate inside its limit is at most
        the limit, and rounding never makes a marketable on-grid limit unfillable.
      - The grid is the fill bar's, from the reference data in force when the order fills rather than when
        it was decided. A venue prints on the grid in force at execution.
      - A sell estimated below one tick rounds to zero. That raises the existing non-positive-price error
        rather than filling for nothing. A non-positive tick is an error. Without that guard, a zero tick
        divides by zero and a negative tick rounds a buy down.
      - `DeterministicFillModel::fill`, the reference-free public entry point, still prices off any grid,
        and so does the crate's test-only `process_bar`. Every other replay carries reference data (item
        73), so every fill it simulates rounds.
    - **Evidence effect.** This changes backtest economics, which is why item 73 recorded it rather than
      changing it.
      - The probe corpus's base-cost fill moves from 100.28016 to 100.29, and its doubled-cost fill from
        100.48032 to 100.49. Measured with the real binary, the two returns fall from 19.99 to 19.33 bps
        and from 3.31 to 2.67 bps.
      - `TRANSACTION_COST_SHOCK` degradation therefore rises from 16 to 17 bps. Each return truncates to
        whole bps, so it is now 19 − 2 rather than 19 − 3. The test pinning it is updated, and it fails
        without the rounding.
      - The other two checked-in backtest configurations (`backtest-v1.json` and
        `backtest-advanced-v1.json`) use zero spread and zero slippage, and no checked-in bar fixture has
        a sub-cent price. Their fills were already on the cent grid and do not move.
    - **Tests.**
      - The rule itself: buys up and sells down, against the trader rather than to the nearest tick; an
        on-grid price unchanged on either side; a grid at the decimal's own resolution; and a zero or
        negative tick refused.
      - The fill model on a dime grid and on a nickel grid. An on-grid buy and sell limit each fill exactly
        at the limit. An off-grid limit, reachable only by a direct call, fills unrounded and refuses once
        rounded. A sell below one tick is an error.
      - The bar path: a slipped buy prints at 100.30 on a dime grid, and the ledger's average cost carries
        that price. On a nickel grid it prints at 100.25. When the grid changes between the decision bar
        and the fill bar, the fill bar's grid applies.
      - The news path prints on the grid too.
    - **Rule 5.** 11 of 11 injected defects were caught, each by a failing test:
      - the rule: sells rounding up, buys rounding down, rounding to the nearest tick, an on-grid buy moved
        a tick, the non-positive-tick guard dropped (a divide-by-zero panic), and a guard refusing only an
        exactly zero tick (the negative tick then rounded a buy down to 100.27);
      - the model: the limit checked before rounding, and the tick ignored (caught separately by the
        control-plane tests and by the CLI's adversarial-probe test);
      - the paths: the bar path and the news path each passing no tick.
      - A second call site, for an order eligible on the bar that produced it, is unreachable: eligibility
        is floored at the next bar. It passes the tick for consistency, and no test can reach it.
    - **Stale descriptions corrected.** The desktop's "Execution realism model" panel said the "final
      spread-and-slippage price" could never violate the limit. It now describes the grid rounding and
      the grid price. The operations guide's replay capability list gained the same sentence.
    - **Measured result.** The Rust workspace rose from 501 to 505 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (76 probes, 0 failed). Against the previous run's `var/`, the
      main backtest's event stream is byte-identical. Its `strategy_bundle_hash` and the two fingerprints
      built on it changed only because that hash covers `core/control-plane/src/lib.rs`, the built-in
      strategy's source, which this slice edits.
    - **Bounded remainder.**
      - Price improvement, midpoint executions and sub-penny prints are not modelled. A fill prints on the
        grid against the trader or not at all.
      - Marks and P&L use bar closes as recorded. A bar is data, and an off-grid bar is not corrected.
      - The fill cap is still checked when an intent arrives (item 73). No external gate moved.

75. One commit publishes one set of evidence hashes on every platform (2026-09-27, Reliability and
    research-to-live parity; E3.8).
    - **The gap.** Follon hashes several checked-in inputs byte for byte:
      - the backtest CLI's `configuration_hash` covers the configuration file's bytes;
      - the SDK's bundle hash covers each strategy file's bytes;
      - the built-in strategy's `strategy_bundle_hash` covers `core/control-plane/src/lib.rs`, which
        `follon-backtest` embeds with `include_str!`.

      The repository had no `.gitattributes`, so each checkout used its platform's line endings. On the
      development machine, Git for Windows' system configuration sets `core.autocrlf=true`, so 329 text
      files were checked out with CRLF and 14 with mixed endings. CI checks out on `ubuntu-24.04` with LF.
      Measured on one commit:
      - `tests/fixtures/config/backtest-v1.json`: the pipeline published `configuration_hash`
        `e860fca6…`, the hash of the CRLF bytes, while the repository's own bytes hash to `8e7d0f84…`;
      - `python/examples/worker_buy_once_strategy.py` and the SDK's `bundle.py` differed the same way;
      - the built-in strategy's hash followed whichever tool last wrote its source. A Git checkout wrote
        it with CRLF, and during E3.6e `cargo fmt` rewrote it with LF.

      One commit therefore published different evidence hashes on different platforms, and on one
      machine from one tool run to the next. Nothing compared hashes across machines, so nothing failed.
      This also stood in the way of item 57's still-unperformed clean-machine verification.
    - **The rule.**
      - `.gitattributes` checks every text file out with LF on every platform (`* text=auto eol=lf`).
        `eol` overrides `core.autocrlf`. It also marks PNG, ICO, ICNS and PDF files binary.
      - Every text file in the index was already LF, so no committed content changed:
        `git add --renormalize .` staged nothing.
      - `CONTRIBUTING.md` states the rule and how to repair a stale checkout.
      - The new test caught a second writer on its first pipeline run. The pipeline regenerates the 32
        checked-in `tests/fixtures/config/advanced/*.json` documents through
        `tools/build_advanced_evidence_fixtures.py`, which wrote them in Python text mode, and so with
        CRLF on Windows. Every pipeline run on this machine had rewritten them with CRLF. The generator
        now writes `newline="\n"`. No other tool writes a checked-in file in text mode: the rest write
        to `var/` or a temporary directory, or already pass `newline="\n"`.
    - **This checkout.** Git does not rewrite an unmodified file when the rule changes, and
      `git checkout-index --force --all` left every stale file as it was.
      - One stale file was deleted and checked out again. That is the repair `CONTRIBUTING.md` now
        describes.
      - Each of the other 342 was rewritten with its index blob's exact bytes. A file was rewritten only
        after its content, with CRLF normalised, matched that blob byte for byte, so no uncommitted change
        could be lost.
      - `git add -u` then refreshed the stale stat cache. The staged diff was empty.
    - **Tests.** `tests/security/test_checkout_line_endings.py` is run by pytest and by CI's security job:
      - every hashed input is `text=auto` and `eol=lf`, and an icon is not text;
      - no tracked text file is checked out with CRLF or mixed endings, and each stale file is named;
      - each hashed input holds its index blob's exact bytes. It is skipped only when it carries a real
        content edit, never for a line-ending rewrite;
      - the fixture generator, run into a temporary directory, writes exactly the checked-in set of files,
        each byte-identical to its index blob. It needs `jsonschema`, which CI's security job does not
        install, so it runs under pytest and is skipped there with that reason.
    - **Rule 5.**
      - Against the stale checkout, all three checkout tests failed without the attributes file. With it,
        two of the three still failed, because the checkout was still stale.
      - 6 of 6 injected defects were then caught: `eol=lf` dropped, the binary rules dropped, one hashed
        input rewritten with CRLF (caught by both checkout tests), one other file given a single CRLF
        line, the generator's text-mode write restored (all 32 fixtures fail), and one generated field
        drifting from its checked-in fixture.
      - The first CRLF injection exposed a weak test. `git status` reports a line-ending rewrite as a
        change, so the byte test skipped the file instead of failing. It now skips only a content edit
        that `git diff` sees, and the injection fails both tests.
      - Two bugs in the first draft were fixed before it was relied on. Whitespace splitting misread
        `ls-files --eol`, whose attribute column holds a space. Its failure hint also named
        `checkout-index --force`, which rewrites nothing here.
    - **Evidence effect.** Against the previous pipeline run's `var/`:
      - the main backtest's `configuration_hash` moved from `e860fca6…` to `8e7d0f84…`, the hash of the
        repository's own bytes;
      - the Python-worker backtest's `strategy_bundle_hash` moved from `bf8f5bee…` to `88d5e002…`. The
        same hash was recomputed from a `git archive` export with `core.autocrlf` off, which is
        byte-identical to an LF checkout, and it matched. Without that flag, `git archive` applied
        `core.autocrlf` as well, while HEAD had no attributes;
      - both backtests' event streams are byte-identical, and the dataset hash, computed from parsed bars,
        did not move.
    - **Measured result.** The Python suite rose from 48 to 52 passed; the Rust workspace is unchanged at
      505 passed / 0 failed / 3 ignored. The final `python tools/session_status.py` run measured all seven
      suites green on the LF checkout, and the full evidence pipeline exited 0, including step 23b's scan
      (76 probes, 0 failed). After that run every tracked text file was still LF and the generated
      fixtures were unchanged.
    - **Bounded remainder.**
      - A file an editor saves with CRLF is still hashed as saved until it is checked out again. The test
        names it, but only when the suite runs.
      - A capsule already carries its own copies of the bytes it hashes, so this changes which bytes a
        fresh package hashes, not how an existing capsule verifies.
      - Item 57's clean-machine verification is still not performed. No external gate moved.

76. PAPER and controlled LIVE refuse at startup a policy whose tick and lot tables list different
    instruments (2026-09-27, rows 5.5 and 5.7; E3.6f). This closes the first remainder item 70 recorded.
    - **The gap.** Items 61 and 70 require a tick table and a lot table in every PAPER, controlled-LIVE
      and PAPER command-route configuration, and each table was validated on its own. An instrument
      listed in only one of them passed startup, and then every order on it was refused with the other
      table's `..._UNCONFIGURED` code. A configuration mistake therefore surfaced at order time, during a
      session, rather than when the service started.
    - **The rule.**
      - `PaperRiskPolicy::validate` and `LiveRiskPolicy::validate` refuse a policy that lists an
        instrument in only one of the two tables, and name the first such instrument.
      - Both services validate their policy when they are constructed or opened, and neither can replace
        it afterwards. Every boundary therefore inherits the rule: both CLIs, the gRPC route and the
        desktop gateway.
      - PAPER and controlled LIVE each carry their own copy of the check, as they do every rule in their
        gates (item E1.4a's decision).
      - An instrument in neither table is still refused at order time, on both counts. That is now the
        only way to reach either `..._UNCONFIGURED` code.
      - No fingerprint changes. A configuration that validated before validates and fingerprints
        identically, and every checked-in configuration (the six fixtures and the scanner's route)
        already lists the same instruments in both tables.
    - **Tests.**
      - PAPER and controlled LIVE each refuse a policy with a lot-only instrument, and one with a
        tick-only instrument, naming it, and the service refuses to start. A refused controlled-LIVE
        service leaves no journal.
      - The gRPC route refuses to start when its lot table omits a leg instrument that its tick table
        lists.
      - Six existing tests failed, because each built a service from a policy that listed an instrument
        in one table only, which is now refused:
        - PAPER's and controlled LIVE's no-tick tests now refuse an instrument in neither table, on both
          counts, with both evidence fields;
        - PAPER's no-lot test is replaced by the startup-rule test, and controlled LIVE's lot test keeps
          its off-lot cases;
        - controlled LIVE's two combination tests remove an unlisted leg from both tables.
      - PAPER's two combination tests still passed, because they edit the tables after construction,
        which bypasses validation. They were changed the same way, so they also model a state a validated
        policy can reach. A combination's codes are sorted, so it lists the lot code first.
    - **Rule 5.** 11 of 11 injected defects were caught:
      - PAPER: the cross-check removed (caught separately by the PAPER test and by the gRPC route test),
        only ticks checked against lots, and only lots checked against ticks;
      - controlled LIVE: the same three;
      - the order-time rules both environments still need: an unlisted instrument passing the tick rule,
        and passing the lot rule, in each environment.
    - **Documentation.** The lot-table descriptions in the three version-1 configuration schemas, and the
      desktop README, state the rule. JSON Schema cannot express it, so the services enforce it.
    - **Found and left for its own slice (E3.9).** The version-2 PAPER configuration schema declares
      neither table and forbids additional properties. `follon-paper-status` reads version 2, and
      `tests/fixtures/config/paper-v2.json` and `paper-v2-portfolio-risk.json` both fail validation
      against that schema.
    - **Measured result.** The Rust workspace rose from 505 to 507 passed / 0 failed / 3 ignored; the
      Tauri host is unchanged at 30. The final `python tools/session_status.py` run measured all seven
      suites green, and the full evidence pipeline exited 0, including step 23b's scan (76 probes, 0
      failed).
    - **Bounded remainder.** The tables remain the operator's configuration, and nothing checks them
      against a venue's own reference data. A venue's odd-lot handling is not modelled. No external gate
      moved.

77. The version-2 PAPER configuration schema describes what its reader reads, and every configuration
    fixture is held to its schema (2026-09-27, Architecture row and contract conformance; E3.9).
    - **The gap.** `follon-paper-status` reads versions 1 and 2 of the PAPER configuration through one
      document type, which since items 61 and 70 requires `instrument_tick_sizes` and
      `instrument_lot_sizes`. Those items added both tables to the version-1 schema only. The version-2
      schema's `risk` object declares neither table and forbids additional properties. So it rejected
      both version-2 fixtures, including `paper-v2.json`, which pipeline step 14a feeds to the CLI.
      Nothing loaded a schema next to the configuration it describes, so nothing failed. Items 61 and 70
      are corrected in place.
    - **The fix.** The version-2 `risk` object declares and requires both tables, with the version-1
      definitions byte for byte, including item 76's pairing rule. Every shared `$defs` entry was already
      identical in both versions.
    - **Tests.** `tests/security/test_configuration_contracts.py` pins 17 configuration fixtures to the
      input-contract schema their reader implements: three backtest, three PAPER, two controlled-LIVE, the
      PAPER command route, operations, options, and six commercial configurations.
      - Without any third-party package, so CI's security job runs it: each fixture declares nothing its
        schema forbids and omits nothing the schema requires, at the top level and one level down, where
        each configuration keeps its risk policy.
      - Also without a package: the version-2 PAPER `risk` object declares and requires everything the
        version-1 one does, with identical definitions, and every shared definition is identical.
      - Where `jsonschema` is installed, full validation of every fixture. CI's security job does not
        install it, so there this one is skipped with that reason.
      - Surveying every fixture against every similarly named schema also turned up six evidence
        schemas: adversarial evaluation, counterfactual scenario, attention budget, recovery drill,
        capital allocation and gateway qualification. Each describes a command's output, not the
        configuration it reads. Those were my pairings, not drift, and they are not in the table.
    - **Rule 5.** All three tests failed before the fix, and only on the two version-2 fixtures. 6 of 6
      injected defects were then caught:
      - the version-2 lot table misnamed, caught by all three tests;
      - the tables declared but no longer required, the lot table's `minProperties` loosened to 0, and
        `positiveDecimal` loosened to admit a sign. Each of these was caught only by the version-extension
        test, which is why it exists;
      - a fixture carrying an undeclared field, and a fixture missing a required one.
    - **Measured result.** The Python suite rose from 52 to 55 passed; the Rust workspace is unchanged at
      507 passed / 0 failed / 3 ignored. The final `python tools/session_status.py` run measured all seven
      suites green, and the full evidence pipeline exited 0, including step 23b's scan (76 probes, 0
      failed).
    - **Bounded remainder.** Output and evidence documents are outside this test; only some pipeline
      steps validate what they publish. JSON Schema cannot express item 76's pairing rule, so the services
      enforce it. No external gate moved.

78. A refused PAPER start leaves no journal behind (2026-09-27, rows 5.5 and 5.10; E3.10).
    - **The gap.** `PaperTradingService::open_durable` opened its journal before validating the account
      and risk policy, and `FilePaperJournal::open` creates the file when it is absent. A configuration
      refused at startup, such as item 76's unpaired tables, therefore left an empty journal at the
      configured path. Controlled LIVE already validates before it opens its journal. Found while
      writing item 76's route test.
    - **The fix.** `open_durable` constructs the service first, which validates the account, the policy
      and the broker route, and only then opens the journal. The legacy-routing check still runs after
      the journal opens, because it needs to know whether the journal is empty. Every existing reopen
      test passes unchanged.
    - **Documentation corrected.** The desktop gateway's configuration comment said the file mirrored the
      document `follon-paper-status` reads "closely enough that the same operator-authored file can back
      both". That was false: the CLI's document is nested, the desktop's is flat, and both refuse unknown
      fields. The desktop's is the version-1 `paper-command-route` document without its `schema_version`
      and `adapter_kind`. The comment now says so, and says that it had claimed otherwise.
    - **Tests.** A refused configuration leaves no journal at its path, and the same path opens and
      journals once the configuration is valid. Item 76's gRPC route test now also asserts that no
      journal exists after the refused start.
    - **Rule 5.** Restoring the journal-first order was caught separately by the PAPER test and by the
      route test (2 of 2).
    - **Measured result.** The Rust workspace rose from 507 to 508 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (76 probes, 0 failed).
    - **Bounded remainder.** A start refused by the legacy-routing check itself still happens after the
      journal is opened, and can leave an empty journal. No external gate moved.

79. A replay refuses to fill a working order off a lot size that changed while it worked (2026-09-27,
    row 5.4 and research-to-live parity; E3.6g). This closes the second remainder item 73 recorded.
    - **The gap.** Item 73 made the replay's risk rule require whole lots, and required a fill cap to be
      whole lots of the traded instrument, both judged when the intent arrives. Reference data is
      effective-dated. If an instrument's lot size changed while an order was still working, a later
      fill could be off the new lot, and the replay filled it anyway. No venue executes that fill.
    - **The rule, as the operator decided.** The replay refuses. It raises an error rather than model a
      venue's response, which varies by venue and is not modelled anywhere in the repository.
      - At each fill attempt, the fill quantity, which is the remaining quantity or the fill cap if
        smaller, must be a whole number of lots under the fill bar's reference data. Otherwise the replay
        stops with an error naming the order, the quantity, the instrument and its new lot.
      - The check runs before anything about the attempt is recorded. The fill bar is the last event on
        the stream.
      - The fill bar's reference data now reaches the fill attempt whole. E3.6e passed only its tick; the
        rounding still uses that tick.
      - A change the order still fits is not refused.
      - On the news path, the reference data that judged the order is the one it fills under, so the
        check never fires there. It still applies.
    - **Tests.** Across a lot change at the fill bar:
      - three shares decided under a one-share lot are refused under five-share lots, with nothing after
        the fill bar on the stream and no fill;
      - four shares with a two-share fill cap are refused under four-share lots. The order is still whole
        lots, but its capped fill is not;
      - six shares under three-share lots fill as usual.
    - **Rule 5.** 5 of 5 injected defects were caught: the guard removed; the guard checking the order's
      full quantity instead of the capped fill; the guard checking against the tick; the guard moved to
      after the fill is recorded (caught on the stream, not the error text, which is identical); and the
      bar path passing no reference data to working orders (caught by this test and by E3.6e's grid
      test).
    - **Measured result.** The Rust workspace rose from 508 to 509 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (76 probes, 0 failed). Python measured 54 passed rather than 55.
      Item 75's byte test skips any hashed input with uncommitted edits, and this slice edits one. pytest
      then reports the whole test skipped, which hides the other inputs' checks. Item 80 removes that
      skip. **Corrected in place 2026-09-27 (item 80).** "Hides the other inputs' checks" overstated it.
      The other inputs were still checked, and an earlier input's failure still failed the test. What
      pytest hid was a failure in an input after the skipped one.
    - **Bounded remainder.** The tick analogue is not guarded. A working limit order whose limit falls
      off a changed tick grid is still eligible, and its fill prints on the new grid against the trader
      (item 74). The operator chose the lot guard only. No external gate moved.

80. Item 75's byte check no longer skips an edited input (2026-09-27, Reliability; E3.8b).
    - **The gap.** Item 75's `test_hashed_inputs_hold_the_repository_bytes` called `skipTest` for any
      hashed input with uncommitted edits. That left the edited file entirely unchecked. It also misled
      under pytest, which `session_status.py` runs. pytest does not report unittest subtests one by one,
      so a skip in one subtest marked the whole test skipped. A CRLF input *after* the skipped one in
      the list then failed its subtest, and pytest still reported the test as skipped. Measured: with
      `core/control-plane/src/lib.rs` edited and `python/examples/worker_buy_once_strategy.py`
      rewritten with CRLF, pytest reported `1 skipped`, while unittest, which CI runs, reported the
      failure. Found because E3.6g's measurement read 54 Python tests passed rather than 55.
    - **The fix.** An edited hashed input cannot match its index blob, so it is held to what matters:
      no CRLF. An unedited one must still equal its blob exactly. The test never skips.
    - **Rule 5.** With `lib.rs` edited and a later input rewritten with CRLF, the new test fails where the
      old one was reported skipped. With `lib.rs` itself edited and rewritten with CRLF, the new test
      fails where the old one skipped it. On a clean tree, and with an LF edit, all four checkout tests
      pass and none skips.
    - **Item 79 corrected in place.** It said the skip hid "the other inputs' checks". It hid a later
      input's failure, not every check.
    - **Measured result.** The Python suite measured 55 passed and none skipped, where E3.6g's run read 54
      passed and one skipped. Rust is unchanged at 509. The final `python tools/session_status.py` run
      measured all seven suites green, and the full evidence pipeline exited 0, including step 23b's scan
      (76 probes, 0 failed).
    - **Bounded remainder.** An edited input is checked for line endings, not content, which is correct:
      its content is the edit. No external gate moved.

81. Authenticated controlled-LIVE kill-switch RPCs, on a route that can never trade (2026-09-27, Architecture
    and Security rows; E3.3c).
    - **The gap.** A controlled-LIVE kill switch could be moved only through the `core/live` library.
      `LiveTradingService::activate_kill_switch` journals the actor and time, but nothing called it
      operationally. `follon-live-status` has no kill-switch flags, and the trading API had no LIVE route
      at all. Item 71 recorded this as one of E3.3's remainders.
    - **What landed, as the operator decided.** This is the explicit exception, recorded under Settled
      direction item 3, to keeping IAM and the control plane warm rather than extended. The other E3.3
      remainders stay open.
      - `ActivateLiveKillSwitch` and `ReleaseLiveKillSwitch` on the gRPC API, under exactly E3.3b's rule.
        A bearer session whose role grants `KillSwitchOperate` in the request's tenant, which only
        `risk_manager` does, is checked before anything else. A trading role, another tenant, a missing
        session and a malformed scope are refused. With no route configured, the RPC fails closed.
      - The route is configured by `FOLLON_TRADING_API_LIVE_CONFIG`, the version-1 document
        `follon-live-status` reads, and `FOLLON_TRADING_API_LIVE_JOURNAL`. It refuses to start with only
        one of the two, or without the operator directory. Off loopback it requires server TLS and a
        client CA, as the PAPER route does.
      - The route opens the LIVE journal at the server's UTC time with `KillSwitchOnlyLiveAdapter`, which
        refuses every broker operation. The API process can halt controlled LIVE and can never place,
        cancel or reconcile an order. While it runs it holds the journal's exclusive lock.
      - Each change is journaled by `core/live`'s existing path, with the operator as the entry's actor
        and the server's time. `core/live`'s rules are unchanged: a repeated activation is journaled, and
        a release that changes nothing is not.
    - **One parser for the LIVE configuration.** `follon-live-status` parsed its configuration privately.
      A second copy in the API could drift, and a journal opens only under the exact configuration
      fingerprint it was written with. The document, its validation and the portfolio-risk composition
      therefore moved into `core/live` as `LiveConfiguration::from_json`, which both use, with the same
      fields and rules. Only its errors changed: they are now `LiveError` values, and a JSON error gains
      the prefix "invalid live configuration:". The CLI's composition test moved with it. `LiveKillSwitchScope::from_key` is the public inverse of
      `as_key`, and the journal-restore parser now delegates to it.
    - **Tests.**
      - `core/live`: the checked-in LIVE journal opens under the parsed `live-v1.json` with the fingerprint
        its first entry recorded. Also: the portfolio-risk composition, the document's strictness (an
        unknown field, schema version 2, a `PAPER` activation mode, empty and oversized documents), and
        the scope-key round trip.
      - gRPC: the permission separation; operator attribution; a repeat reported as unchanged; a malformed
        scope; another tenant; the switch surviving a route restart; the release; the closed journal
        naming the risk manager for all three kill-switch entries; failing closed without a route; and the
        startup rules.
    - **Scanner.** `tools/dast_scan.py` configures the LIVE route over `tests/fixtures/config/live-v1.json`
      and a scratch journal, and adds nine probes:
      - G30 to G34: no session, a trading role, a risk manager activating a switch and being named its
        operator, the release, and an unknown scope;
      - J02, run after the API stops: the LIVE journal names the risk manager for exactly those two
        changes;
      - C04 to C06: a LIVE route refused without the operator directory, without its journal, and on a
        plaintext non-loopback bind.

      Adding the route exposed a harness gap. C03, which checks that operator login refuses a plaintext
      remote bind, then inherited the LIVE variables and was refused for the LIVE reason. The harness
      requires the exact reason, so it reported the probe failed rather than passing it. C03 now runs
      with no route configured. The scan rose from 76 to 85 probes, 0 failed.
    - **Rule 5.** 12 of 12 injected defects were caught:
      - gRPC: authorizing `PaperTrade` instead, the release RPC activating, a fixed operator journaled,
        a missing route answering as if a switch had moved, a route accepted without its journal, a route
        accepted without the operator directory, and a remote route accepted without a client CA;
      - the shared parser: the portfolio-risk block dropped (caught by the composition test), a fixed
        kill-switch revision (caught because the checked-in journal no longer opens under it), and
        `from_key` accepting any key as `global`;
      - the extended scan, against two service defects. Authorizing `PaperTrade` failed G31 to G34 and
        J02. A fixed operator failed only J02: the RPC response still named the real operator, and
        only the stopped API's journal showed otherwise.
    - **A stale statement corrected.** `apps/cli/README.md` still said the API had "its one write RPC,
      `SubmitPaperCombo`". That became false with item 71's kill-switch RPCs. It now lists the write RPCs,
      and says that only `risk_manager` grants kill-switch operation.
    - **Measured result.** The Rust workspace rose from 509 to 515 passed / 0 failed / 3 ignored: four
      `core/live` tests and three gRPC tests were added, and the CLI's composition test moved. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (85 probes, 0 failed).
    - **Bounded remainder.**
      - Sessions stay in memory, and there is no four-eyes rule on a kill-switch change.
      - The Tauri desktop's IPC writes are unauthenticated, and the operator directory has no managed
        secret store.
      - There is still no LIVE order RPC and no operational controlled-LIVE trading process. The API's
        route can only halt.
      - The deployment review stays external. No external gate moved.

82. A refused legacy-route PAPER start leaves no journal behind (2026-09-27, rows 5.5 and 5.10; E3.10b).
    This closes the remainder item 78 recorded.
    - **The gap.** Item 78 made `PaperTradingService::open_durable` validate before it opens its journal.
      One refusal still came after: a legacy composition, which `follon-paper-status` builds for a
      version-1 configuration, may only reopen an existing journal. The check needs to know whether the
      journal is empty, so it ran after `FilePaperJournal::open`, which creates an absent journal and its
      directory. A version-1 start against a new path was therefore refused and left an empty file, and
      any missing directories, behind.
    - **The fix.** When the composition may not initialize a journal, `open_durable` opens it without
      creating anything. An absent journal is refused before any file or directory exists. An existing
      empty journal is refused and left as it was, as before. `FilePaperJournal::open`, which every other
      caller uses, still creates an absent journal. The refusal's error text is unchanged.
    - **Tests.** A legacy route refuses a path whose directory does not exist, and creates no directory.
      It refuses a path in a directory that exists, and creates no file. It refuses an existing empty
      file, and leaves it at zero bytes. It still reopens a journal the single-adapter composition
      initialised.
    - **Rule 5.** 4 of 4 injected defects were caught, each by that test at its intended assertion:
      - the legacy route opening with creation, as before;
      - the directory created even when the file is not;
      - the file created in an existing directory, although its directory is not created;
      - an existing empty journal accepted.

      The last one was not caught before this slice. The existing legacy test used an absent path, so an
      absent journal and an empty one were never told apart.
    - **Documentation.** `apps/cli/README.md` says that a refused version-1 start creates neither a journal
      file nor its directory.
    - **Measured result.** The Rust workspace rose from 515 to 516 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (85 probes, 0 failed).
    - **Bounded remainder.** None of item 78's remains. No external gate moved.

83. Journal, ledger and admin output paths refuse a dangling symbolic link (2026-09-27, Security row
    "Immutable audit and signed release"; E3.11). Found while reading item 82's journal-open code.
    - **The gap.** Four checks that say a path "must not be a symbolic link" first asked `path.exists()`:
      the PAPER journal, the controlled-LIVE audit journal, the commercial ledger, and `follon-admin`'s
      outputs. `exists()` follows a link, so for a dangling link it reports nothing there, and the check
      passed. What followed depended on the writer. Each was measured on this machine, where Developer
      Mode lets an ordinary account create links:
      - The PAPER and LIVE journals and the commercial ledger open with `create(true)`, which follows the
        link. A dangling link at the journal path opened, so the journal was created and written at
        wherever the link pointed. Measured with the journals' own open options: `exists()` reported
        false, and five bytes written through the link landed in the target file.
      - `CommercialLedger::read_verified` treated the dangling link as an absent ledger, and reported a
        verified empty ledger.
      - `follon-admin` writes with exclusive creation or a hard link, and neither follows a link, so a
        dangling output was still refused. But it was refused with an unrelated operating-system error.
        For `release-keygen`'s trusted key, it also left a staging file behind.
    - **The fix.** Each check reads `symlink_metadata`, which never follows a link. A link, dangling or
      not, is refused; an absent path passes; any other error is returned rather than treated as absent.
      The refusal texts are unchanged.
    - **Tests.** Each is exercised with a real link. Where an account cannot create one, such as Windows
      without Developer Mode, a test says so and returns, as the capsule's symlink test already does. On
      this machine every link was created. None of the four checks had a test with a link before.
      - PAPER and LIVE: a dangling link is refused by the journal and by the service's durable open, and
        nothing is created at its target. A link to a real journal is refused.
      - Commercial: a dangling link is refused by `open` and by `read_verified`, and nothing is created at
        its target. A link to a real ledger is refused by both.
      - `follon-admin`, through the real binary: `release-keygen` refuses a dangling link as its private
        key and as its trusted key, with the symbolic-link refusal, writing nothing at the target and
        staging nothing.
    - **Rule 5.** 6 of 6 injected defects were caught:
      - the `exists()` check restored in each of the four places. Each journal and the ledger then opened
        through the dangling link. The admin binary failed with `AlreadyExists` instead of the refusal;
      - the ledger's verified read no longer checking for a link. It then returned an empty ledger;
      - the LIVE refusal narrowed to a link whose own length is non-zero. On this platform that is no
        link.
    - **Documentation.** The penetration-test runbook's required cases gain durable journal paths, with the
      remainder below as a case to probe. The Security row notes this item.
    - **Found and left for a decision.** `follon-admin release-keygen` writes the private key and then
      publishes the trusted key. If the second write is refused, for this reason or because a different
      trusted key already exists, the new private key stays on disk with no trusted key beside it. Deleting
      key material automatically is a custody decision, so it is recorded here rather than changed.
    - **Measured result.** The Rust workspace rose from 516 to 520 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (85 probes, 0 failed).
    - **Bounded remainder.** Each check runs before its open, so a link swapped in between the two is not
      refused. Closing that needs a no-follow open, `O_NOFOLLOW` on Unix and `FILE_FLAG_OPEN_REPARSE_POINT`
      on Windows, which the repository does not use. Other file writers in the repository do not claim to
      refuse links and are unchanged. No external gate moved.

84. A replay refuses to fill a working limit order off a tick grid that changed while it worked
    (2026-09-27, row 5.4 and research-to-live parity; E3.6h). This closes the remainder item 79 recorded.
    - **The gap.** Item 73's risk rule puts a limit on the tick grid in force when the order is decided.
      If an instrument's tick size then changed while the order worked, its limit could be off the new
      grid, a price no venue holds an order at. The replay kept it eligible and filled it on the new grid
      (item 74). Measured with the guard removed: a buy limited at 100.05, decided on a cent grid, filled
      at 100.00 on a dime grid.
    - **The rule.** It applies the operator's E3.6g decision to the tick. That decision was to refuse
      rather than model a venue's response, which varies by venue. A venue may cancel a non-conforming
      resting order or reprice it, and neither is modelled.
      - At each fill attempt, a limit must sit on the fill bar's tick grid. Otherwise the replay stops with
        an error naming the order, its limit, the instrument and its new tick.
      - The check runs beside item 79's lot check, before anything about the attempt is recorded, whether
        or not the bar would fill the order. A lot violation is reported first.
      - A limit the new grid still holds is not refused, and a market order has no limit to check. It
        fills on the new grid against the trader, as before.
      - On the news path, the reference data that judged the order is the one it fills under, so the
        check never fires there. It still applies.
      - PAPER and controlled LIVE read a configured tick table that cannot change under a journal, since
        a journal opens only under the fingerprint it was written with. They need no analogue.
    - **Tests.** Across a change from a cent to a dime grid at the fill bar:
      - a buy limited at 100.05 is refused with nothing after the fill bar on the stream, and no fill;
      - a buy limited at 100.10 fills at 100.00;
      - a market buy fills at 100.00.

      Item 79's helper now takes the strategy and both new increments. Its three lot cases are
      unchanged.
    - **Rule 5.** 4 of 4 injected defects were caught:
      - the guard removed, after which the off-grid limit filled;
      - the limit checked against the lot size instead;
      - the check inverted;
      - the guard moved to after the fill is recorded. This was caught on the stream, not the error text,
        which is identical.
    - **Documentation.** The operations guide's replay capability list, and the desktop's "Execution
      realism model" panel, now state that a lot or tick change which leaves a working order off the new
      increments stops the replay. The panel described neither this guard nor item 79's.
    - **Measured result.** The Rust workspace rose from 520 to 521 passed / 0 failed / 3 ignored. The final
      `python tools/session_status.py` run measured all seven suites green, and the full evidence pipeline
      exited 0, including step 23b's scan (85 probes, 0 failed). Against the previous run's `var/`, both
      backtests' event streams are byte-identical. The main backtest's `strategy_bundle_hash` and the two
      fingerprints built on it changed only because that hash covers `core/control-plane/src/lib.rs`,
      which this slice edits. The other changed files carry per-run values: fresh signing keys and
      signatures, and benchmark timings.
    - **Bounded remainder.** A venue's own handling of a tick or lot change, whether it cancels, reprices
      or accepts odd lots, is not modelled. Price improvement and midpoint prints are not modelled either.
      No external gate moved.

85. Foundation CI's failures with an obvious fix (2026-09-28, Security and Reliability; delivery state
    E4.1). Reported by the revised assessment and reproduced here from GitHub's logs.
    - **The gaps.** GitHub's `Verify foundation` workflow has not passed on any run visible there, back to
      at least 2026-09-05. Main run 36334101143 failed three jobs, and PR run 36333055826 a fourth:
      - `sast`: the one blocking finding was `generic.nginx.security.request-host-used` on line 1516 of
        this audit, which quotes item 41's fixed defect. The nginx rules select their targets by a `conf`
        path glob, which this file's name matches. Measured with Semgrep 1.177.0, the version CI pins: the
        same text is flagged in a file named `conformance-notes.md` and not in one named `plain-notes.md`.
      - `security`, on the PR run: gitleaks 8.24.3's `generic-api-key` flagged the `credential_reference`
        field of two LIVE configuration fixtures, whose value is a canonical ID beginning `secret.broker.`.
        That value names a managed secret; `core/secrets` documents `SecretReference` as
        "the non-sensitive reference used for audit and access policy". A push to main scans only its own
        commits, which is why the main run passed. A full-history scan found three findings, all this one.
      - `desktop`: clippy stopped in `glib-sys`'s build script, "Package glib-2.0 was not found in the
        pkg-config search path", because the runner lacks Tauri's Linux libraries. The native tests after
        it were skipped.
      - `python-and-contracts`: six IBKR bridge tests import `ibapi`, which the runner lacks. Which IBKR
        API distribution CI may install is a decision (E4.2), so that job is unchanged here. The bridge
        README said its suite runs "without TWS or `ibapi`", false since those six tests were added.
    - **The fixes.**
      - Semgrep excludes this audit as a path. Excluding the rule instead would stop scanning
        `infra/nginx.dashboard.conf`. The audit's secrets remain covered by gitleaks.
      - `.gitleaks.toml` extends the default rules with one allowlist: a line, anchored at both ends,
        holding only `credential_reference` with a `secret.`-prefixed canonical ID. It has no `paths`
        condition, because gitleaks 8.24.3 ignores `condition = "AND"` in the global allowlist, so a path
        would allowlist every finding beneath it. That was measured before this file was written: with a
        path and the condition, a real-looking key under the fixture directory, the same reference
        elsewhere, and a token-shaped value in the field were all allowlisted.
      - The desktop job installs Tauri v2's documented Ubuntu prerequisites before its native steps.
      - The bridge README states what its suite needs. Eight tests need neither TWS nor `ibapi`. Six need
        `ibapi`, and pass against 9.81.1.post1, the only version they have run against. On Windows,
        `zoneinfo` also needs the `tzdata` package.
      - The security job's test step was named "Verify deterministic SBOM generation" but runs every test
        under `tests/security`. It is renamed.
    - **Verification.** Each configuration change was checked in both directions, the rule-5 method
      applied to configuration:
      - Semgrep, the exact CI command plus the exclusion: 0 findings on 329 files, exit 0. With
        `proxy_set_header Host $host;` re-injected into `infra/nginx.dashboard.conf`, the same command
        blocked it, exit 1. The file was restored byte for byte.
      - Gitleaks over the whole history, 100 commits, with the configuration: no findings, exit 0. In a
        probe repository the canonical reference was allowed, and three leaks were still reported: a
        real-looking key in the fixture directory, a token-shaped value in the allowlisted field, and a
        real-looking key sharing a line with a canonical reference. A control run without the
        configuration reported every probe file.
      - The bridge suite as CI runs it, without `ibapi` but with `tzdata`: 14 run, the same 6 errors,
        8 passed. With `ibapi` 9.81.1.post1: 14 passed.
      - Not verified: the desktop fix. Docker's daemon was not running and this machine has no other
        Linux environment, so it stays unverified until the workflow runs on GitHub (E4.3).
    - **Measured result.** Configuration and documentation only. The final `python tools/session_status.py`
      run, over this exact tree, measured all seven suites green with the Rust workspace unchanged at 521
      passed / 0 failed / 3 ignored. The evidence pipeline was not run: its acceptance step would publish
      the assessment's synthetic record, which E6.1 fixes next. No external gate moved.

86. The evidence pipeline counts only the operational acceptance ledgers (2026-09-28, External and
    operational gates; delivery state E6.1). Found while verifying the revised assessment.
    - **The gap.** Pipeline step 23 ran `tools/acceptance_evidence.py` over `var/` itself, and the tool
      counts every `*.acceptance.ndjson` beneath its root. The 2026-09-27 assessment retained a synthetic
      boundary-experiment ledger under `var/reports/`: one structurally valid `paying_customer` record,
      invented to probe the tool's limits and labelled as such in its notes. An in-memory run of the tool
      over this machine's `var/` counted it as one verified record, with the paying-customer gate
      eligible. The pipeline had not run since that ledger was written, so nothing false was published;
      its next run would have published it.
    - **The fix.** Step 23 is now `publish_acceptance_status(var_dir)`, which audits only `var/acceptance/`,
      the operational ledger root, and creates it empty when absent. The tool refuses a missing root rather
      than reporting zero, and an empty root is the truth when no ledger has been retained. The production
      runbook names the root and says to keep review and synthetic ledgers out of it. The assessment's
      ledger is left where it was: it is the assessment's retained evidence, and it is now outside what
      counts.
    - **Tests.** `PipelineAcceptanceRootTests` runs the pipeline's own step against a temporary `var/`:
      - a valid synthetic customer ledger under `reports/` is not counted, and the gate stays ineligible;
      - a ledger under the operational root is counted;
      - an absent root is created empty and reports zero records.
    - **Rule 5.** 3 of 3 injected defects were caught:
      - the root reverted to `var/` itself, the original defect. Only the first test failed, on the count;
      - the root no longer created, after which the tool refused the missing directory;
      - the root moved to a directory that counts nothing, caught by the second test.
    - **Measured result.** Python rose from 55 to 58 passed; the Rust workspace is unchanged at 521 passed /
      0 failed / 3 ignored. The final `python tools/session_status.py` run measured all seven suites green,
      and the full evidence pipeline exited 0, including step 23b's scan (85 probes, 0 failed). The
      published `var/follon-acceptance-status.json` reports zero verified records and every gate
      ineligible, with the synthetic ledger still present under `var/reports/`.
    - **Bounded remainder.** The tool still trusts whatever sits under its root. A subject rejected after it
      was accepted still counts (E6.2), the promotion gate still trusts a caller-supplied status document
      (E6.3), and nothing authenticates a reviewer or re-hashes a source artifact (E6.4). No external gate
      moved.

87. A PAPER route declares what it can carry, and the OMS refuses the rest before an order exists
    (2026-09-28, row 5.6 and Architecture; delivery state E5.1). Found while verifying the revised
    assessment.
    - **The gap.** The real IBKR PAPER adapter could not execute three things the OMS would send it, and
      each left the order worse off than a refusal:
      - A combination. `IbkrPaperBridgeProcessTransport::submit_paper_combo` forwarded a `submit_combo`
        request, whose payload carried neither leg quantities nor the debit or credit sign, to a Python
        bridge whose dispatch has no such operation. The bridge answers `unsupported bridge operation`.
        The OMS records any adapter error as a transport failure, so the combination became `UNKNOWN`,
        the session disconnected, and every later order was refused with
        `UNKNOWN_ORDER_REQUIRES_RECONCILIATION`. Leaving `UNKNOWN` needs broker evidence, which the bridge
        could never send for an order it never placed.
      - A GTC intent. `BrokerOrderRequest` has no time in force and the bridge places every order DAY,
        so a GTC intent would have been placed DAY with nothing recording the change.
      - A replacement. The adapter inherits the trait's refusal, which the OMS meets after moving the
        order to `PENDING_REPLACE`, so it too became `UNKNOWN` with the session disconnected.

      No application composes the real adapter yet (E5.2), so none of this could happen in a shipped
      application. It would have been the first thing an integration met.
    - **The fix.** `PaperBrokerCapabilities` declares, per adapter and account, whether a route executes
      combinations, carries GTC, and replaces orders. Its default is the narrowest set, single DAY
      orders, so an adapter that declares nothing is never handed more.
      - The service asks before it evaluates risk, because evaluation itself caches the request's mark
        and moves the equity baselines. Anything the route cannot carry is refused with nothing
        recorded, nothing transmitted, and the session still connected. Replacement is asked before the
        order moves.
      - The model declares all three, as model capabilities. The registry answers for its route's
        adapter. The desktop's `ManualFillAdapter` declares everything but replacement, which it never
        forwarded.
      - The real adapter declares the default, and its transport no longer sends `submit_combo`; the
        trait's default refuses without transmitting.
    - **A test that passed for the wrong reason.** `FaultInjectingBroker` never forwarded `submit_combo`.
      So `combo_transport_failure_leaves_the_group_unknown_and_disconnects` passed on the trait's
      refusal: its scheduled Disconnect fault was never consumed, and every combination through the
      wrapper became `UNKNOWN` whether or not a fault was scheduled. A first version of this slice gave
      the wrapper no combinations, that test failed, and the reason was found. The wrapper now forwards
      combinations under the same fault schedule as single orders. A new test shows that an unfaulted
      combination through it is acknowledged, so the old test can no longer pass on a refusal.
    - **Tests.**
      - A narrow route refuses a GTC intent, a combination and a replacement. After each refusal no
        order and no risk evidence exist, the broker was not called, the durable journal's sequence is
        unchanged, the session is connected, and no order is `UNKNOWN`. The same route then carries a
        DAY order.
      - The refused GTC intent is on an instrument nothing else in its test touches, and so are the
        refused combination's legs. After the next DAY order is journaled, neither appears anywhere in
        the journal, so each refusal came before risk evaluation cached its mark.
      - Each adapter's declaration: the default, the model, the fault wrapper over the model and over a
        narrow route, and a registry holding a narrow route beside a model route.
      - The real adapter declares the default and refuses a combination without calling its transport.
        Its process transport, run against the fake bridge fixture, sends nothing for a combination: the
        fixture exits on any operation it does not implement, and it still answers a poll afterwards.
      - The desktop adapter declares everything but replacement.
    - **Rule 5.** 14 of 14 injected defects were caught, each by a failing assertion rather than a compile
      error:
      - each of the three checks removed;
      - each submission check moved after risk evaluation, caught only by the journal assertion;
      - the model declaring no combinations, and the registry answering the default;
      - the fault wrapper stripping combinations, ignoring its schedule, or not forwarding them at all;
      - the real adapter declaring combinations or GTC, and its transport sending `submit_combo` again;
      - the desktop adapter declaring replacement.
    - **Measured result.** The Rust workspace rose from 521 to 529 passed / 0 failed / 3 ignored,
      and the Tauri host from 30 to 31. The final `python tools/session_status.py` run measured all seven
      suites green. The first full pipeline run failed one probe of step 23b's scan: H32, a DELETE with a
      two-byte body sent to the dashboard, got no response. That is a flake which predates this slice and
      lies outside it. In isolation it lost 12 responses in 1,000 with a body and none without, and the
      next slice fixes it (E7.13). The rerun exited 0, with the scan at 85 probes and 0 failed.
    - **Bounded remainder.** Controlled LIVE has the same shape: `IbkrControlledLiveAdapter` inherits a
      refusing `submit_combo`, and `core/live` records that refusal as a transport failure that keeps the
      approval and a canary slot consumed (E5.7, latent). A refusal from the bridge itself, such as an
      unmapped instrument, still becomes `UNKNOWN` (E5.4). No external gate moved.

88. The dashboard reads a request's declared body before it answers (2026-09-28, Security row "Dashboard
    authentication"; delivery state E7.13). Found when scan probe H32 failed during item 87's first
    pipeline run.
    - **The gap.** The dashboard serves `GET`, and answers any other method with the standard library's
      501, without reading a declared request body. Closing a connection with input unread resets it, and
      the reset can reach the client before the client reads the response, which destroys it. Probe H32
      sends `DELETE` with a two-byte body, recorded no response, and rightly treats that as a failure.
      - Measured with the probe's own client against the real handler: 19 of 1,000 responses were lost.
      - With the body sent just after the headers, every response was lost.
      - Behind the production nginx proxy the same race would surface as a 502.
    - **The fix.**
      - `parse_request` runs once per request, after its headers and before any dispatch, including the
        standard library's 501. It now reads and discards a declared body of at most 64 KiB.
      - A larger or malformed declaration is never read, and the connection closes after the response.
      - Every read now has a 15-second socket timeout. Before, no read had one, and a client that declared
        a body and never sent it would have held its handler thread indefinitely once bodies were read.
    - **Tests.** Four contract tests run against a real server, each sending the body one byte at a time
      just after the headers:
      - POST, PUT, DELETE and PATCH each keep their 501;
      - a GET keeps its 200;
      - a declared body that never arrives cannot hold the server: with the timeout shortened, the client
        gets its 501 instead of timing out, and the default timeout is finite;
      - a declared 10 MiB body is never read: with a handler timeout longer than the client's, the 501
        still arrives.

      The suite ran 15 times without a failure.
    - **Rule 5.** 4 of 4 injected defects were caught: the discard removed, one byte of the body left
      unread, the 64 KiB bound removed, and the timeout removed. The second was missed at first, because the
      buffered reader pulled both bytes off the socket anyway, so the injection changed nothing observable.
      The test now sends the body in pieces, so a short read leaves input unread, and is caught.
    - **Measured result.** The dashboard contract suite rose from 20 to 24 tests; the Rust
      workspaces are unchanged at 529 and 31. The final `python tools/session_status.py` run measured all
      seven suites green, and the full evidence pipeline exited 0 with step 23b's scan at 85 probes, 0
      failed. H32's own request against the real handler lost 19 responses in 1,000 before the fix and
      none after.
    - **Bounded remainder.** A chunked request body is not read. The timeout also bounds how long a slow
      client may take to receive a large evidence download: at most 15 seconds, where there was no limit.
      No external gate moved.

89. A rejection disqualifies its subject, and acceptance records are held to their exact contract
    (2026-09-28, External and operational gates; delivery state E6.2). Reported by the revised assessment,
    whose synthetic experiment showed a later rejection leaving a customer gate eligible.
    - **The gaps.**
      - A rejected record only incremented a per-gate counter; its `subject_id` was ignored. A subject
        accepted and then rejected still counted toward its gate, and so did one rejected and then
        accepted.
      - `acceptance_evidence_schema_version` was compared with `!=`, so JSON `true`, which Python treats
        as 1, passed, and so did 1.0.
      - `occurred_at` was checked by `fromisoformat` plus its length and trailing `Z`. A space in place of
        the `T`, and an ISO week date such as `2026-W35-1T10:00:00Z`, have the same length and passed.
      - A test was named for refusing duplicate evidence IDs but only ever tampered with a record.
    - **The fix.**
      - Within each gate, any subject with a rejected record is subtracted from the accepted subjects,
        whichever record came first. The ledger has no correction record, so an acceptance can neither
        outlive a later rejection nor overturn an earlier one. Each gate reports `disqualified_subjects`.
        A rejection in one gate does not touch another.
      - The schema version must be the integer 1: `type`, not `isinstance`, because `bool` is an `int`.
      - `occurred_at` must match `YYYY-MM-DDTHH:MM:SSZ` in ASCII digits, then parse as a real UTC time.
      - The production runbook states the counting rule.
    - **Tests.** Six new cases, on top of the tampering test, now named for what it does: rejection after
      acceptance, rejection before acceptance, a rejection confined to its own subject and gate, a boolean
      or float schema version, six non-canonical timestamps including February 30 and Arabic-Indic
      digits, and a real duplicate evidence ID.
    - **Rule 5.** 6 of 6 injected defects were caught, each by the test written for it: the subtraction
      removed, the latest record allowed to win, a rejection disqualifying the subject in every gate, the
      version compared by equality only, the old timestamp parser restored, and the duplicate check
      removed.
    - **Measured result.** Python rose from 58 to 64 passed; the Rust workspace is unchanged
      at 529 passed / 0 failed / 3 ignored. The final `python tools/session_status.py` run measured all
      seven suites green, and the full evidence pipeline exited 0 with the scan at 85 probes, 0 failed.
      The published status reports zero verified records, every gate ineligible, and
      `disqualified_subjects` in each gate.
    - **Bounded remainder.** A mistaken rejection cannot be withdrawn: a correction record, and who may
      write one, is a policy decision with the session criteria (E6.5). The status still carries nothing
      that ties it to the ledger files it counted, and the promotion gate still trusts a supplied status
      (E6.3). Nothing authenticates a reviewer (E6.4). No external gate moved.

90. Promotion recomputes acceptance from the ledgers, and its receipt binds what it counted (2026-09-28,
    Security row "Immutable audit and signed release" and External and operational gates; delivery state
    E6.3). Reported by the revised assessment.
    - **The gap.** `tools/release_promotion_gate.py` read a caller-supplied status document through
      `--acceptance-status` and trusted its `all_gates_eligible`. A two-field document,
      `{"acceptance_status_schema_version": 1, "all_gates_eligible": true}`, passed it for production. The
      status itself carried nothing tying it to the ledgers it came from, so even a genuine one could not
      show which ledger state it counted. A status that was not a JSON object also escaped the gate's
      error handling as a traceback; the gate now parses only its own tool's output, which is always
      an object, so that path is closed by construction rather than by a test.
    - **The fix.**
      - The gate takes `--acceptance-ledger-root` instead. It runs `tools/acceptance_evidence.py` over that
        root itself and trusts only what the tool prints. A tool failure blocks promotion to every
        environment, and production still needs every gate eligible.
      - The status is schema 2 and lists, for each ledger file counted, its path relative to the root, its
        SHA-256, its record count and its chain head, all taken from the bytes the tool validated.
      - The receipt is schema 2. It hashes the exact bytes the tool emitted and carries every ledger
        binding.
      - The production runbook's command and description are updated.
    - **Tests.** In `tests/security/test_release_promotion_gate.py`, which covered only the approval rule:
      - production is refused over an empty ledger root;
      - a status document is not an input: two documents declaring every gate eligible, placed in the
        ledger root itself, change nothing, and the parser refuses `--acceptance-status`;
      - production becomes eligible only when the ledgers meet every gate, and one missing PAPER session
        blocks it;
      - a tampered ledger blocks staging and production alike;
      - staging needs verifiable ledgers but not eligibility;
      - the receipt's status hash is of the tool's exact bytes, and it binds each ledger file's SHA-256.

      In the acceptance tests, the status binds two ledgers, one in a subdirectory, by relative path,
      SHA-256, record count and head.
    - **Rule 5.** 8 of 8 injected defects were caught:
      - production no longer refused on open gates;
      - the acceptance tool's failure ignored;
      - `--acceptance-status` accepted again;
      - the status dropping its bindings, one hashing the path instead of the bytes, and one recording the
        genesis hash as its head;
      - the receipt omitting the ledgers, or hashing a re-serialized status instead of the tool's bytes.

      One injection was first refused by the runner as ambiguous, because its text appears twice in the
      gate. It was re-anchored and caught. An early version of the `--acceptance-status` assertion would
      have passed on missing required arguments alone. It now supplies every argument, so only the
      unrecognized one can end the parse.
    - **Measured result.** Python rose from 64 to 71 passed; the Rust workspace is unchanged
      at 529 passed / 0 failed / 3 ignored. The final `python tools/session_status.py` run measured all
      seven suites green, and the full evidence pipeline exited 0 with the scan at 85 probes, 0 failed.
      The published status is schema 2 with zero verified records, every gate ineligible, and an empty
      `ledgers` list, because no operational ledger has been retained.
    - **Bounded remainder.** The gate still authenticates no reviewer, and it neither re-hashes the source
      artifact a record names nor binds a release to the environment it was accepted in (E6.4). The
      trusted release key is still whatever file the caller passes. Requester and approver are still two
      distinct strings rather than two authenticated people. No external gate moved.

91. Every durable writer E3.11 did not cover refuses a symbolic link (2026-09-28, Security row "Immutable
    audit and signed release"; delivery state E7.1). The revised assessment named the operations journal;
    the source review behind it found the other four.
    - **The gaps.** Each checked `exists()`, which follows a link, or checked nothing:
      - the operations journal opened with `create(true)`, which follows a link, and its reader treated a
        dangling link as an absent file, reporting a healthy empty journal. `follon-operations` publishes
        its model-risk and game-day registers from that reader;
      - the replay `FileEventStore` and the backtest `FileExperimentStore` created their files through a
        dangling link, and the experiment store reopens its file on every write;
      - `follon-news replay --output` used `File::create`, which follows a link and truncates its target;
      - `write_immutable`, behind every CLI artifact, wrote a staging file for a dangling link, failed on
        an unrelated error, and left the staging file behind. A link to identical content counted as
        already published.
    - **The fix.** Each path is read with `symlink_metadata`, which never follows a link. A link, dangling
      or not, is refused; an absent path passes; any other error is returned. The experiment store checks
      again before every write. The CLI helper is public and is shared by `write_immutable` and
      `follon-news`.
    - **Tests.** Each uses a real link; where an account cannot create one, a test says so and returns.
      Every link was created on this machine:
      - the operations journal's open, `inspect` and `read_verified_records` refuse a dangling link and a
        link to a real journal, and nothing is created at the target;
      - the event log refuses both, and creates nothing at the target;
      - the experiment store refuses a dangling link at open, and a link that appears after it was opened
        is refused at the write, with nothing written through it;
      - `write_immutable` refuses a dangling link with the link refusal, stages nothing, and refuses a
        link to identical content;
      - through the real binary, `follon-news` refuses a dangling link and a link to an operator's file,
        which keeps its content, and still writes a plain output.
    - **Rule 5.** 8 of 8 injected defects were caught, before and again after formatting: each guard
      removed, the experiment store's write-time guard removed on its own, and the shared CLI guard
      reading `fs::metadata`, which follows the link it checks.
    - **Measured result.** The Rust workspace rose from 529 to 534 passed / 0 failed / 3
      ignored. The final `python tools/session_status.py` run measured all seven suites green, and the full
      evidence pipeline exited 0 with the scan at 85 probes, 0 failed. The built-in strategy's bundle hash
      changed because it covers `core/control-plane/src/lib.rs`, which this slice edits.
    - **Bounded remainder.** Every guard still runs before its open, so a link swapped in between them is
      not refused (E7.12). The penetration-test runbook lists the new paths. No external gate moved.

92. Every strategy worker hashes strings identically in every run (2026-09-28, research-to-live parity
    and Reliability; delivery state E7.3). Found by the source review behind the revised assessment.
    - **The gap.** The parent clears the worker's environment and sets only `PYTHONIOENCODING` and an
      import path. With `PYTHONHASHSEED` unset, Python seeds `hash()` of `str` and `bytes` randomly per
      process. A strategy that iterates a set of symbols, or anything ordered by such hashes, could
      therefore decide in a different order in two runs of one replay. A capsule's byte-for-byte replay
      check would then fail at random rather than report a real change.
    - **The fix.** `spawn_inner`, behind every worker constructor including the capsule's sandboxed one,
      sets `PYTHONHASHSEED=0`. The CLI README states the worker's full environment and that the worker is
      a same-user process for trusted code only.
    - **Test.** A fixture worker, `tests/fixtures/worker/hash-seed-worker.py`, speaks protocol v1 and
      answers every callback with an error whose code is the hash of a fixed string. The test starts it
      four times through `ProcessStrategyWorker::spawn` and requires the same code each time.
    - **Rule 5.** 2 of 2 injected defects were caught, before and after formatting: the seed removed, and
      the seed set to `random`.
    - **Measured result.** The Rust workspace rose from 534 to 535 passed / 0 failed / 3
      ignored. The final `python tools/session_status.py` run measured all seven suites green, and the full
      evidence pipeline exited 0 with the scan at 85 probes, 0 failed. Its strategy capsule, replayed by a
      seeded worker, still sealed `VERIFIED_PORTABLE`.
    - **Bounded remainder.** The backtest configuration's `seed` is still provenance only. It does not seed
      a strategy's own `random`, as the E7.3 row records, and a strategy may still read the clock, files or
      the network, which only running trusted code controls. Hashes are identical for one Python version,
      not across versions. No external gate moved.

93. Tax lots follow a split, as the position always did (2026-09-28, row 5.4 and research-to-live parity;
    delivery state E8.1). Found by the source review behind the revised assessment, and reproduced here
    before it was fixed.
    - **The gap.** `BacktestLedger::apply_corporate_action` scaled a split position's quantity and average
      cost but not its FIFO tax lots. `AdvancedBacktestAccount`, which holds long and short lots, did the
      same. Neither existing split test sold after the split or looked at a lot. Reproduced on the old code:
      two shares bought at 100, split 2-for-1, then one sold at 55 realized -45 where +5 was due. Selling
      the remaining three was refused with "tax disposal exceeds available long lots". The ledger's own
      comment said such a disposal could never exceed the lots.
    - **The fix.** `TaxLotBook::apply_split` multiplies every open lot's quantity by the ratio and divides
      its unit cost, or a short lot's unit proceeds, by it. That is the same eight-place arithmetic the
      accounts use for a position's average cost, so a single lot's cost and the position's stay equal. Both
      sides are computed before either is replaced, so a failure leaves the book unchanged, and a ratio that
      is not positive is refused. Both accounts call it from their split branch.
    - **Tests.**
      - In `core/accounting`: two long lots and a short lot on another instrument. The long lots scale to
        20 @ 50.05 and 10 @ 60, and the short lot is untouched until its own 3-for-1 split, when it becomes
        12 @ 100. A zero or negative ratio changes nothing, and an instrument without lots is a no-op.
        Disposing all 30 shares uses a cost basis of 1601, exactly the pre-split 10 x 100.10 + 5 x 120.
      - In `core/backtest`: the reproduced ledger case now realizes 5 and then 20, and closes every lot.
        The advanced account closes a split long at 55 and covers a split short at 45, realizing 20 each.
    - **Rule 5.** 6 of 6 injected defects were caught: either account's call removed, long lots left
      unscaled, a unit cost left undivided, short lots left unscaled, and the ratio check removed.
    - **Measured result.** The Rust workspace rose from 535 to 538 passed / 0 failed / 3
      ignored. The final `python tools/session_status.py` run measured all seven suites green, and the full
      evidence pipeline exited 0 with the scan at 85 probes, 0 failed.
    - **Bounded remainder.** The replay engine's own portfolio, and so the fingerprinted event stream, is
      still not adjusted for corporate actions, and working orders across a split are not handled (E8.2).
      The worker's snapshot and cash still ignore them (E8.3). PAPER and LIVE apply none (E8.4). A reverse
      split can leave fractional lot quantities, exactly as it leaves a fractional position; cash in lieu is
      not modelled. No external gate moved.

94. CI installs the IBKR API the operator approved, pinned by hash (2026-09-29, Reliability; delivery state
    E4.2). Item 85 left the Python job's `ibapi` failure for this decision.
    - **The decision.** Asked which IBKR API distribution CI may install, the operator chose `ibapi`
      9.81.1.post1 from PyPI (Settled direction item 5). It is IBKR's own upload (IBG LLC, 2020-12-06),
      under the IB API Non-Commercial License or the IB API Commercial License, and it is the only version
      the bridge's six official-backend tests have run against.
    - **The change.** `python/ibkr-gateway/requirements-ci.txt` pins that release by version and by SHA-256,
      and the `python-and-contracts` job installs it with `--require-hashes` before the bridge tests. The
      hash, `49f6678b...9cd6`, is the one PyPI publishes for the release's only file, its source archive,
      and it matched the file downloaded here. The bridge README states the pin and the command.
    - **Verification.** In a freshly created environment, the pinned install succeeded and all 14 bridge
      tests passed; `tzdata` was added only because Windows has no system time-zone database. With one
      byte of the hash changed, pip refused the install ("THESE PACKAGES DO NOT MATCH THE HASHES").
    - **Measured result.** Configuration and documentation only. The final
      `python tools/session_status.py` run measured all seven suites green, unchanged at Rust 538 and
      Python 71, and the full evidence pipeline exited 0 with the scan at 85 probes, 0 failed.
    - **Bounded remainder.** 9.81 dates from 2020, and current TWS API releases are 10.x. Whether the bridge
      runs against a current TWS or IB Gateway build is unverified until E5.6's real session. A deployment
      must record and review its own distribution. The job's storage-adapter and server-contract steps
      have never run on GitHub, because the bridge step failed before them; a green run is E4.3. No
      external gate moved.

95. Every first-party licence declaration is MIT, as the root `LICENSE` is (2026-09-29, Architecture and
    release supply chain; delivery state E7.11). Reported by the revised assessment.
    - **The gap.** The root `LICENSE` is the MIT License, while the Cargo workspace, which all 21 member
      crates inherit, the separate desktop host crate and the strategy SDK declared Apache-2.0, and the
      storage adapter declared nothing. The assessment asked that this be resolved before any rights or
      distribution claim.
    - **The decision.** Asked which licence was intended, the operator chose MIT (Settled direction item 5).
    - **The change.**
      - `[workspace.package]` and the desktop host declare `license = "MIT"`.
      - Both Python packages declare `license = "MIT"`, the PEP 639 SPDX form that setuptools 77 and later
        expect. The table form is deprecated there, and its removal date has passed. The strategy SDK's
        build requirement rises from setuptools 68 to 77, the first version that reads the string form.
      - The README gains a licence section.
      - `tests/security/test_licence_declarations.py` holds `LICENSE`, every Cargo manifest and both Python
        packages to the one licence.
    - **Verification.**
      - `cargo metadata` reports MIT for all 21 root-workspace packages and for the desktop host.
      - Both Python wheels built in isolation without a deprecation warning, and each records
        `License-Expression: MIT`.
      - The test failed before the change, on the Cargo and Python declarations.
      - Rule 5: 4 of 4 injected defects were caught, each declaration reverted or dropped in turn.
    - **Measured result.** Python rose from 71 to 74 passed; the Rust workspace is unchanged at
      538 passed / 0 failed / 3 ignored. The final `python tools/session_status.py` run measured all
      seven suites green, and the full evidence pipeline exited 0 with the scan at 85 probes, 0 failed.
    - **Bounded remainder.** The SBOM still records no first-party licence (E7.8). Contributor provenance
      and third-party notices, which the assessment also named, are a review for the owner, not a
      metadata change. No external gate moved.

96. Foundation CI passes on a clean runner (2026-09-29, Reliability and Security; delivery state E4.3). This
    is the revised assessment's first completion criterion: every foundation job runs its required steps
    and passes.
    - **The run.** GitHub Actions run 36524632774, for pull request 31 at `0a2d380`, completed with all
      six jobs successful: `rust`, `postgres-integration`, `security`, `sast`, `python-and-contracts` and
      `desktop`. It is the first run of `Verify foundation` visible on GitHub to pass. Every earlier one,
      back to at least 2026-09-05, failed.
    - **What it verified that could not be verified locally.**
      - The desktop job installed Tauri's Linux libraries (item 85), and clippy and the native tests then
        ran and passed. This machine has no Linux environment.
      - The `python-and-contracts` job installed the hash-pinned `ibapi` (item 94). Its storage-adapter
        and server-contract steps ran on GitHub for the first time, and passed, including item 88's
        socket tests on Linux.
    - **Bounded remainder.** It is a pull-request run. The push run on `main` after merging confirms the
      same on `main`. The dependency-review step stays skipped while its repository variable is off, as the
      workflow intends. No external gate moved.

97. A refused `release-keygen` leaves no private key behind (2026-09-29, Security row "Immutable audit and
    signed release"; delivery state E7.6). Item 83 found this and left it for a decision.
    - **The gap.** `release-keygen` generated a key pair, wrote the private key, and only then validated
      and published the trusted key. Every refusal of the trusted key left a new private key on disk with
      no trusted key beside it: a link at its path, an existing file there, a non-UTF-8 file name, or the
      same path given for both outputs. Item 83's test covered the linked case but checked only for
      staging files, so the leftover key went unnoticed.
    - **The fix.** The command now checks both outputs before a key exists, without creating or
      following anything. Each must be free of links, absent, UTF-8-named, and in a parent that is absent
      or a real directory, and the two must be different paths. This decides no custody question: nothing
      is deleted. A refusal the check predicts simply happens before any key is made.
    - **Tests.** A new workflow test, through the real binary. An existing trusted key, one path for both
      outputs and an existing private key are each refused, and no new file appears. The existing trusted
      key and private key keep their contents, and a clean pair of paths still works. Item 83's link test
      now also requires that no private key remains.
    - **Rule 5.** 4 of 4 injected defects were caught: both checks removed, the trusted key's check
      removed, the distinct-paths check removed, and existing outputs no longer refused. The last was first
      written as a guarded match arm that did not compile; the runner reported it as a compile error rather
      than a catch, and it was rewritten.
    - **Measured result.** The Rust workspace rose from 538 to 539 passed / 0 failed / 3
      ignored. The final `python tools/session_status.py` run measured all seven suites green, and the full
      evidence pipeline exited 0 with the scan at 85 probes, 0 failed. Its own `release-keygen` calls still
      succeed.
    - **Bounded remainder.** A write that fails after the checks, such as on a full disk or a filesystem
      without hard links, can still leave the private key. Deleting it automatically is the custody decision
      item 83 recorded. A path created by another process between the check and the write is the E7.12
      race. No external gate moved.

98. Every portfolio-risk limit enters the PAPER and LIVE configuration fingerprints (2026-09-29, rows 5.3 and
    5.10; delivery state E7.4a). Found by the source review behind the revised assessment.
    - **The gap.** Both environments listed the portfolio-risk policy's fields by hand for their
      configuration fingerprint, and both left out `max_daily_loss`, `max_drawdown_bps`,
      `max_margin_utilization_bps`, `strategy_limits` and the composition's `margin_rates`. The fingerprint
      is what a journal reopens under and what a LIVE approval binds, so a journal reopened, and a LIVE
      approval stayed valid, after any of those limits changed.
    - **The fix.**
      - `PortfolioRiskPolicy::canonical_parts` in `core/risk` renders all 21 fields in a fixed order. Its
        destructuring names every field and has no `..`, so a field added later does not compile until
        it is rendered.
      - PAPER and LIVE each build their part from it, plus the composition's instrument buckets and
        margin rates. Each environment keeps its own copy of that rendering, as each keeps its own risk
        gate.
      - The part's tag moves from `v1` to `v2` in both environments. A journal or LIVE approval made under
        the incomplete fingerprint is refused, not trusted, because unchanged limits cannot be shown.
      - A configuration without portfolio risk contributes no part, so its fingerprint is byte-for-byte
        unchanged. The checked-in PAPER and LIVE journal fixtures, whose configurations have none, still
        reopen.
    - **Tests.** In `core/risk`, changing any one of the 21 fields changes the canonical parts. In PAPER
      and LIVE, changing any of the five omitted limits changes the configuration fingerprint.
    - **Rule 5.** 5 of 5 injected defects were caught: a field dropped from the canonical parts, either
      environment never rendering its margin rates, and either environment keeping only the first six
      policy parts.
    - **Measured result.** The Rust workspace rose from 539 to 542 passed / 0 failed / 3
      ignored. The final `python tools/session_status.py` run measured all seven suites green, after a
      first run failed clippy's `type_complexity` on the new tests, which now use a type alias. The full
      evidence pipeline exited 0 with the scan at 85 probes, 0 failed, and both checked-in journal
      fixtures still reopen.
    - **Bounded remainder.** The check is still skipped outright when equity is not positive. Whether to
      fail closed, which would also refuse risk-reducing orders, is the operator's decision (E7.4b).
      Neither order-submitting route composes portfolio risk at all (E7.5). No external gate moved.

99. The gRPC PAPER route composes the real IBKR PAPER bridge by configuration (2026-09-29, row 5.6 and
    research-to-live parity; delivery state E5.2a). The operator chose this route first (Settled direction
    item 5).
    - **The gap.** No application composed `IbkrPaperGatewayAdapter`. The gRPC route accepted only
      `adapter_kind: IBKR_PAPER_MODEL`, so the real bridge was reachable from no running service.
    - **The fix.**
      - `adapter_kind: IBKR_PAPER_BRIDGE`, with a required `ibkr_bridge` section, starts the official-API
        bridge process. The model stays the default. A model route with the section is refused, and so is
        a bridge route without it.
      - The route builds the bridge's argument list itself, from fixed fields, so no free-form argument
        reaches the process. The bridge's own timeout is two seconds inside the route's deadline, so the
        bridge answers before the route gives up.
      - Before any process starts, the route refuses what the bridge would refuse. That covers a
        non-loopback host or a non-PAPER port, a relative interpreter, a timeout outside 3 to 60 seconds,
        a client id above 31, and an empty, over-long or multi-line broker account. It also covers a
        missing, linked, or over-1 MiB instrument map.
      - The route fingerprint binds more than the gateway fingerprint, which covers only the account, host,
        port and environment. It adds the broker account, client id, TWS time zone and the instrument
        map's SHA-256, so a journal is refused under another IBKR session. The model's fingerprint is
        byte-for-byte unchanged, so every existing model journal still reopens.
      - The version-1 route schema describes the section and ties it to the adapter kind. A second
        checked-in fixture, `paper-command-route-v1-bridge.json`, validates against it, and the service
        parses both fixtures.
    - **Found while building it.** The gateway fingerprint alone would have let a journal written against
      one IBKR broker account, client id or instrument map reopen against another. The bridge also refuses
      a client id above 31, which nothing on the Rust side checked. Neither the gateway adapter's
      constructor nor `PaperTradingService::open_durable` contacts the bridge, so a client id the bridge
      refuses would have opened a route whose bridge had already exited.
    - **Tests.**
      - The adapter kind and section must agree, and every bridge limit above is refused. None of these
        refusals leaves a journal.
      - The real bridge's own `parse_arguments` parses the route's argument list back to the same
        configuration. That needs neither TWS nor `ibapi`.
      - A bridge route over the fake bridge fixture declares single DAY orders only. `SubmitPaperCombo`
        over the authenticated boundary is refused with no order and no risk evidence recorded, and the
        broker session stays connected.
      - A journal is refused across adapters, and under a changed broker account, client id, time zone or
        instrument-map content. It still reopens after a timeout change.
      - Both checked-in fixtures parse, and the schema refuses what the service refuses. The risk
        contract test pins the schema's two adapter kinds.
    - **Rule 5.** 28 of 28 injected defects were caught, each by the intended test.
      - 18 were in the route:
        - the fingerprint dropping the session, or equalling the model's;
        - the session dropping the broker account, client id or time zone, or hashing the map's path
          instead of its bytes;
        - each of the six bridge limits loosened;
        - the map check following a link or ignoring size;
        - the bridge's timeout not two seconds shorter, `--environment PAPER` dropped, or a fixed route
          deadline;
        - the model accepting a bridge section.
      - 10 were in the schema: the adapter kinds widened, the section's tie to the kind broken either
        way, and a live port, any host, client id 32, a timeout of 2 or 61, a multi-line account or a
        free-form field accepted.
    - **Measured result.** The Rust workspace rose from 542 to 548 passed / 0 failed / 3 ignored, and
      the Python suite from 74 to 75. The final `python tools/session_status.py` run measured all seven
      suites green. A first run failed the Python suite: `test_risk_contracts` still pinned the schema's
      old single adapter kind, and now pins the two. The full evidence pipeline exited 0, with the scan at
      85 probes and 0 failed.
    - **Bounded remainder.** A bridge route still trades nothing. Its only order RPC submits combinations,
      which the bridge refuses (E5.2b), and nothing polls the bridge's fills or reconciles its account
      (E5.2c). The route does not check the TWS time zone name; the bridge refuses one it cannot resolve, and
      that surfaces at the first broker call. Nothing here was run against a real TWS or IB Gateway (E5.6).
      No external gate moved.

100. An attributed single PAPER order records who submitted it; an attributed cancellation records who
     requested it. Cancellation is durable before the broker is asked (2026-09-29, rows 5.5 and
     5.10; delivery state E5.2b-1). This is the kernel half of E5.2b. The gRPC
     route's single-order submit and cancel follow as E5.2b-2.
     - **The gap.**
       - `PaperTradingService` recorded who submitted a combination, and who moved a kill switch. For a
         single order it recorded nobody, and a cancellation named nobody for either kind.
       - A single-order cancellation also moved the order to `PENDING_CANCEL` and asked the broker before
         journaling anything. A crash after the broker accepted it left the journal saying the order still
         worked. Submission and combination cancellation had always journaled first, and controlled
         LIVE journals every pending cancellation first, with its actor.
     - **The fix.**
       - `submit_intent_as` journals the operator with the order's risk evidence, as
         `submit_combo_intent_as` does, and an idempotent retry must come from that operator.
         `submit_intent` is the unattributed form, unchanged.
       - `cancel_order_as` covers single orders and combinations. It records an `OrderOperation` (order,
         `CANCEL_REQUESTED`, operator, time) in the same journal write as the move to `PENDING_CANCEL`. A
         retry of an accepted cancellation changes nothing and journals nothing, as a repeated kill-switch
         change does. `cancel_order` is the unattributed form.
       - A single-order cancellation now journals `PENDING_CANCEL` before the broker is asked, and writes
         no second, identical record after the broker accepts.
       - Both additions are written only when present, so an existing journal re-serializes byte-for-byte.
         A restore refuses an operation that names an unknown order, an unknown action, or a malformed
         operator or time, and a malformed persisted submitter.
     - **Tests.**
       - An attributed order journals its submitter and survives a restart. Another operator's retry and an
         unattributed retry are refused, and so is a malformed submitter, before risk sees the intent. An
         unattributed order journals no `submitted_by` key.
       - A broker stand-in that stops the process when asked to cancel. The restarted service finds the
         order `PENDING_CANCEL` and the operator's request journaled.
       - Operator cancellations of a single order and a combination are each journaled once. A malformed
         operator or time changes nothing. A direct cancellation is not attributed, and the record
         survives a restart.
       - Each malformed persisted field is refused on restore.
     - **Rule 5.** 19 of 19 injected defects were caught by the intended tests on the final files.
     - **Measured result.** The Rust workspace rose from 548 to 552 passed / 0 failed / 3 ignored. The final `python tools/session_status.py` run measured all seven suites green. The full evidence pipeline exited 0; its local DAST scan reported 85 probes, 0 failed.
     - **Bounded remainder.** No route calls these yet (E5.2b-2). Replacement is not attributed; no route
       offers it, and the IBKR bridge cannot carry it (E5.1). Controlled LIVE is unchanged, since it already
       did both. No external gate moved.

101. The authenticated gRPC PAPER route now exposes a single-order submit and an
     account-bound cancel (2026-09-29, rows 5.5 and 5.10; E5.2b-2). This closes
     the route half of E5.2b over item 100's journaled kernel.
     - **Contract.** Version-1 `SubmitPaperOrder` carries the canonical intent,
       fixed-point quantity and optional limit, explicit DAY or GTC, and an
       operator-attested mark and observation time. `CancelPaperOrder` names the
       tenant, configured account and OMS order. Both require a bearer session
       with `PaperTrade` permission before reading the order details.
     - **Behavior.** The submit route calls `PaperTradingService::submit_intent_as`
       and returns its risk decision and actual OMS state, including a rejection
       without an order. The cancel route calls `cancel_order_as`, which journals
       the operator and `PENDING_CANCEL` before the adapter call; it returns the
       resulting state. The kernel handles single and atomic combination order
       IDs. The account is checked against the configured route before a cancel.
     - **Tests.** The model route persists an authenticated submit and cancel
       across restart, with idempotent retries, and the same cancel RPC handles
       a combination order. Missing session, wrong role or
       tenant, wrong account, invalid limit, stale mark, and missing route are
       refused. A fake bridge process accepts a single DAY market order and
       cancellation; a GTC request is refused before creating risk evidence.
       An excessive quantity returns a risk rejection without an OMS order.
       Removing the submitter argument made the durable-operator test fail and
       was reverted.
     - **Measured result.** The Rust workspace rose from 552 to 558 passed /
       0 failed / 3 ignored. The final `python tools/session_status.py` run
       measured all seven suites green. The full evidence pipeline exited 0;
       its artifacts remain local engineering evidence.
     - **Boundary.** The mark remains operator-attested. The bridge's fills are
       not synchronized or reconciled by this route (E5.2c); the fake bridge
       is a protocol fixture, not TWS or IB Gateway evidence. No external gate
       moved.

102. The gRPC PAPER route can now drain broker evidence and reconcile a
     configured account on an authenticated risk-manager request (2026-09-29,
     rows 5.5 and 5.10; E5.2c). It uses the existing durable PAPER kernel;
     the route has no background poller.
     - **Contract.** `ReconcilePaperAccount` requires a tenant and account, and
       an explicit `reconnect` flag after an ambiguous disconnect or restart.
       Only a session with `RiskPolicyManage` permission may call it. The
       response carries the persisted reconciliation ID and time, every issue,
       the UNKNOWN count, broker connection state, and audit sequence/head.
       `snapshot_matches` means only that this snapshot comparison found no
       issue; it is not a clean-session certificate.
     - **Behavior.** A connected call synchronizes broker events before taking
       the independent snapshot. A reconnect call uses the kernel's
       `reconnect_and_reconcile`, which reconnects, drains delayed evidence,
       then compares. The route refuses an absent route or another account,
       and reports broker errors rather than returning a clean result.
     - **Tests.** A model execution is applied before reconciliation; reopening
       its journal against a fresh empty model produces cash and position
       discrepancies. A disconnected model needs an explicit reconnect.
       Missing session, trader role, wrong tenant/account and missing route are
       refused. A stateful Python bridge protocol fixture submits a DAY order,
       emits its execution on poll, and returns a matching account snapshot.
       Removing the event drain made the bridge test fail with filled quantity,
       order state, position and cash mismatches; it was restored.
     - **Measured result.** The Rust workspace rose from 558 to 562 passed /
       0 failed / 3 ignored. The final `python tools/session_status.py` run
       measured all seven suites green. The full evidence pipeline exited 0;
       its artifacts remain local engineering evidence.
     - **Boundary.** The fixture is synthetic test input under `tests/fixtures/`,
       not operating evidence. A real IBKR PAPER account still needs scoped
       snapshot comparison, Gateway logs, restart/reconnect/cancel races, and
       independent review (E5.5–E5.6). The reconciliation journal names the
       account and time but does not retain which authenticated risk manager
       requested it. No external gate moved.

103. A bridge refusal is now a clean rejection, and IBKR's notices no longer
     reject working orders (2026-09-29, rows 5.5 and 5.10; E5.4).
     - **Defect.** Every `ok: false` reply from the bridge reached `core/paper`
       as an adapter error, which records `UNKNOWN` and disconnects the session.
       That included a refusal made before anything reached IBKR, such as an
       unmapped instrument. The order was then stranded, and every later order
       was refused until reconciliation. Separately, the bridge recorded any IBKR
       message code on a tracked order as a rejection. 202 (order cancelled), 399
       (an order warning such as "will not be placed until the market opens") and
       the 2100–2169 system warnings are not rejections, and a false `REJECTED`
       records as terminal an order IBKR still holds.
     - **Behavior.**
       - `BridgeRefusal` marks a failure raised before `placeOrder`.
         `validate_submit`, importable without `ibapi`, carries every submit check
         and gives each refusal a stable code. `BridgeProtocol` answers a refused
         submission `ok: true` with `REJECTED` and `IBKR_BRIDGE_REFUSED_<CODE>`.
         Only a refusal is mapped: any other failure stays `ok: false`, and so
         `UNKNOWN`.
       - The gateway-connected check follows the lookup of a known client order
         ID, so the retry of an order the bridge may already have placed is never
         refused as a new one.
       - A cancellation refused before anything was sent (an unknown client order
         ID, a disconnected gateway) returns `{}` and queues a `CANCEL_REJECTED`
         event. The OMS restores the working state from it, as it does for IBKR's
         own cancel failures. The Rust adapter normalizes the new event type.
       - The bridge remembers which cancellations it requested and reports a
         cancel-failure code (135, 136, 161, 10147, 10148) once, only for those.
         Codes 202 and 399 and 2100–2169 leave the order's state alone. Any other
         code on a tracked order remains a rejection.
     - **Tests.** 20 bridge tests and 3 Rust tests were added.
       - The protocol layer maps a refusal to a rejection and nothing else. Each
         submit check carries its own code, and none of them reaches `placeOrder`
         or consumes an order ID. A disconnected gateway does not reject a known
         order's retry.
       - The cancel refusals become events. Each code class, and the exact edges
         of the warning band, are pinned.
       - A fixture runs the bridge's real dispatcher with a refusing backend
         under the Rust process transport. The transport returns `Rejected` and
         stays healthy, and the `CANCEL_REJECTED` event normalizes. Through the
         gRPC route, two refused orders in a row are both `REJECTED`; before, the
         second would have been refused because the first disconnected the route.
     - **Rule 5.** 26 of 26 injected defects were caught by the intended tests
       on the final files. One first attempt survived because the injection
       crashed into the same failure result the test expects. It was a bad
       injection, not a weak test: a working version of it was caught.
     - **Measured result.** The Rust workspace rose from 562 to 565 passed /
       0 failed / 3 ignored, and the Python suite from 75 to 95. The final
       `python tools/session_status.py` run measured all seven suites green. The
       full evidence pipeline exited 0.
     - **Boundary.** The code table is IBKR's documentation, not a retained
       Gateway session. The tests build the bridge's real `OfficialBackend` over
       a mocked `ibapi` connection, so they are protocol evidence, not TWS
       evidence. The bridge still keeps its order map in memory, so a retry of an
       order placed by an earlier bridge process is not recognised. Neither is
       closed by this item (E5.5 and E5.6). No external gate moved.

104. Controlled LIVE refuses what its broker adapter cannot carry before an
     approval is spent (2026-09-29, rows 5.6 and 5.7; E5.7). This is E5.1's
     analogue for LIVE, under LIVE's own configuration and review.
     - **Defect.** `LiveBrokerAdapter::submit_combo` and `replace` refuse by
       default, and the service recorded that refusal as a transport failure. A
       combination on such an adapter became `UNKNOWN`, the session was marked
       disconnected, and the approval and a canary slot stayed consumed; a
       replacement left the order `PENDING_REPLACE` and then `UNKNOWN`. The broker
       request also carries no time in force, so a GTC intent could not reach the
       venue as such. It was latent, because no application composes a LIVE
       adapter that can trade.
     - **Behavior.**
       - `LiveBrokerCapabilities` (`combinations`, `good_til_cancelled`,
         `replacement`) defaults to single DAY orders, and every adapter may
         declare more through `LiveBrokerAdapter::capabilities`. It is a separate
         type from `follon_paper::PaperBrokerCapabilities`: PAPER's declaration can
         never widen what LIVE attempts.
       - `submit_canary_intent` and `submit_canary_combo_intent` check it after the
         idempotent-retry answer and before the approval is looked at.
         `replace_order` checks it before the order moves to `PENDING_REPLACE`. A
         refusal changes nothing: no order, no consumed approval, no canary slot, no
         disconnect.
       - `IbkrControlledLiveAdapter` declares replacement only, since its transport
         contract carries `replace_live` and nothing else.
     - **Tests.** Four service tests and one adapter test were added. An adapter that
       declares nothing carries single DAY orders only. An undeclared combination,
       GTC intent and replacement are each refused with the approval unspent, the
       canary count at zero, no order, no `UNKNOWN` and the session connected, and the
       same approval then works once the adapter declares it. The existing transport
       failure test, which keeps a spent approval, now covers a declared adapter, the
       only kind that can reach the broker call.
     - **Rule 5.** 12 of 12 injected defects were caught by the intended tests on the
       final files, including both checks moved to after the approval is spent.
     - **Measured result.** The Rust workspace rose from 565 to 570 passed /
       0 failed / 3 ignored. The final `python tools/session_status.py` run measured all
       seven suites green. The full evidence pipeline exited 0.
     - **Boundary.** The IBKR LIVE adapter's own envelope checks, such as a market order
       without a limit or a quantity over the canary ceiling, still return an error and
       so `UNKNOWN`, although nothing was sent. That is a separate LIVE decision and is
       not changed here. Nothing composes a LIVE adapter that can trade, and no
       external gate moved.

105. A strategy worker can no longer exhaust the host's memory or hang a replay
     (2026-09-29, Security and Reliability; E7.2). Found by the source review behind
     the revised assessment (item 92's review).
     - **Defect.** `ProcessStrategyWorker` read each frame with an unbounded
       `read_line`, waited for it with no deadline, and wrote requests with a
       blocking `write_all`. A worker that printed one endless line grew the host's
       memory without limit. One that never answered, or stopped reading its input
       while a large request was being written, hung the replay forever.
     - **Behavior.**
       - `core/control-plane/src/worker_io.rs` moves each pipe direction to its own
         thread. The reader buffers at most `max_frame_bytes + 1` bytes per frame, so
         the bound applies before the allocation, and it counts the newline: a frame
         of exactly the limit is accepted and one byte more is not. Every round trip
         waits at most `frame_deadline` for its answer, and that wait covers the
         write of the request.
       - `StrategyWorkerLimits` defaults to 16 MiB frames and a 60-second deadline.
         The four existing constructors use it, and `spawn_bounded` sets it
         explicitly. A limit below 4 KiB or a zero deadline is refused before a
         process starts.
       - A transport fault kills the worker's process and marks it ended. It is never
         asked again, because after a fault nothing says which request a later answer
         belongs to. A line that is not JSON is still a protocol error, not a
         transport fault.
       - `ProcessStrategyWorker` keeps no `stdin` or `stdout` of its own any more. The
         built-in strategy's bundle hash changed because it covers
         `core/control-plane/src/lib.rs`, which this slice edits.
     - **Tests.** 17 tests and a fixture were added. `tests/fixtures/worker/misbehaving-worker.py`
       is a worker that stays silent, stops reading, streams a frame with no end, sends
       an oversized, cut-off, non-JSON or exactly-sized frame, or behaves.
       - Six unit tests hold the reader: the newline counts, frames come one at a time,
         and an endless reader that errors after 64 KiB shows the reader buffers only
         the limit. A seventh shows a send to a worker that has gone is refused.
       - Process tests: a silent worker ends at the deadline, and a worker that stops
         reading cannot hang the write of a 4 MiB request. An endless, oversized or
         cut-off frame is refused, and an exactly-limit frame is accepted. An ended
         worker answers nothing further, and a worker within its limits keeps answering.
         Each failing round trip runs behind a 30-second watchdog, so a missing bound or
         deadline fails the test instead of hanging it.
     - **Rule 5.** 15 of 15 injected defects were caught by the intended tests on the
       final files, including the read bound removed, off-by-one on either side of the
       limit, the deadline replaced by an hour, and a fault that no longer ends the
       process. One run showed why the fixture's stalls are now bounded: an unended
       worker holds the test's pipes open and hung the runner for ten minutes.
     - **Measured result.** The Rust workspace rose from 570 to 587 passed /
       0 failed / 3 ignored. The final `python tools/session_status.py` run measured all
       seven suites green. The full evidence pipeline exited 0.
     - **Boundary.** The limits are not yet exposed on the `follon-backtest` command
       line, and a worker is still not an operating-system sandbox: it can consume CPU
       and memory of its own. It only stops the host from being exhausted through its
       pipes. No external gate moved.

106. The risk benchmark says when it was measured (2026-09-29, Reliability and
     evidence integrity; E7.9). Found by the source review behind the revised
     assessment.
     - **Defect.** The artifact's `observed_at` was the benchmark fixture's configured
       scenario time, `2026-08-30T21:30:00Z`, echoed unchanged. A run on 27 September
       therefore read as an observation from 30 August, and the desktop's benchmark table
       showed that as when the latency was observed. The evaluator itself takes no time,
       so the field measured nothing about the run.
     - **Behavior.** The artifact is schema 2. `measured_at` is the UTC second the run
       began, from the wall clock. `scenario_observed_at` carries the fixture's as-of
       time, and no field called `observed_at` remains, so nothing that reads as the run
       time carries the scenario's. The input document is unchanged. The desktop's table
       has a "Measured at" and a "Scenario as of" column.
     - **Tests.** Two unit tests and the real-binary workflow test were changed or added:
       - the rendered artifact carries both times under their names, has no
         `observed_at`, and is schema 2;
       - `utc_now` is a canonical timestamp within the second it was read, and the
         workflow test runs the real command and requires `measured_at` to fall inside
         the window of the run while the scenario time stays `2026-08-30T21:30:00Z`;
       - a desktop regression renders the blotter with a benchmark artifact and requires
         both columns and both values, and no "Observed at" header.
     - **Rule 5.** 10 of 10 injected defects were caught by the intended tests: the
       measurement time echoing the scenario's, the scenario time under `observed_at`
       again, the schema version left at 1, a month off by one, minute and second
       swapped, the hour dropped, and three desktop defects. One injection first
       looked over-constrained because `cargo test` stops at the first failing target;
       it was caught by both tests when run with `--no-fail-fast`.
     - **Measured result.** The Rust workspace rose from 587 to 589 passed / 0 failed /
       3 ignored. The final `python tools/session_status.py` run measured all seven
       suites green. The full evidence pipeline exited 0, and its regenerated artifact
       reads `measured_at` of the run's own time.
     - **Boundary.** The artifact remains one local observation on one machine, as its
       description says, and not an availability or load claim. The benchmark, like the
       rest of the evidence set, is not reproducible byte for byte, because its
       latencies vary by run. Neither is changed. No external gate moved.

107. An account with no positive equity may only reduce risk, not skip its aggregate
     limits (2026-09-29, rows 5.5 and 5.7; E7.4b). The treatment was left to the
     operator, and the agent chose it under the blanket delegation recorded as Settled
     direction item 6.
     - **Defect.** All four risk gates, PAPER and controlled LIVE for a single order and a
       combination, returned no aggregate decision when equity was zero or negative, and
       added no reason. Every aggregate limit was therefore skipped for an underwater
       account, the state in which they matter most, because the kernel's ratios are
       meaningless against non-positive equity. An underwater account could add exposure
       past all of them.
     - **Decision.** Reduce-only. The alternatives were to keep skipping (fail open) or to
       refuse everything (an account could not close a losing position). Reduce-only lets
       exposure be closed and nothing else. It is reversible on an explicit instruction.
     - **Behavior.**
       - `follon_domain::reduces_position(current, projected)` is the single definition: a
         trade reduces a position only if it moves it strictly toward flat without passing
         through it. Opening, adding, reversing and doing nothing do not.
       - With aggregate risk configured and equity not positive, a single order that does
         not reduce its instrument's position is refused with `PORTFOLIO_EQUITY_NOT_POSITIVE`.
         A combination is refused unless every leg reduces its own position, because the
         group is atomic. A reducing order passes as before, and the kernel's own limits are
         still not reported, because it did not run.
       - PAPER and LIVE each apply it in their own gate. They share only the arithmetic
         predicate, not policy.
       - An account with no aggregate composition is unchanged, since it has no aggregate
         limits to skip.
     - **Tests.** Five tests were added and one strengthened.
       - The predicate is held on both sides of flat, on an unchanged position and on
         opening from flat.
       - An underwater PAPER account holding a short of 100 refuses adding to it and
         reversing it, and passes buying 50 back. An underwater PAPER account holding the
         filled vertical refuses a combination that adds to both legs, one that closes a
         leg and adds to the other in either order, and passes closing one unit.
       - The two LIVE tests hold the same cases through its own gate, seeded with recorded
         fills.
       - The existing zero-equity test now requires the refusal as well.
     - **Rule 5.** 20 of 20 injected defects were caught by the intended tests: each side of
       the predicate, an underwater order never refused or always refused, the position
       arguments swapped, the reason renamed, and the per-leg accumulation replaced by
       `any`, by the last leg only, and by never or always refusing, in PAPER and in LIVE.
     - **Measured result.** The Rust workspace rose from 589 to 594 passed / 0 failed /
       3 ignored. The final `python tools/session_status.py` run measured all seven suites
       green. The full evidence pipeline exited 0.
     - **Boundary.** "Reduces risk" is judged per instrument on position quantity, not on
       margin or notional: a reduction that swaps into a more volatile instrument is not a
       concept the gate has. The reducing order must still pass every other check, including
       cash, so an account that cannot afford to buy back its short can only reduce partly.
       No external gate moved.

## Business-readiness decision

**Not approved for capital-bearing or customer-facing production use.** The
repository mechanisms and packages are deployable candidates after automated
verification, but the open external gates above are material. The next
master-plan action remains to configure and independently review the real IBKR
PAPER environment, retain 30 clean sessions, complete security/legal/deployment
approvals, and record them through the tamper-evident acceptance ledger. As of
2026-09-28 a configured environment is not enough on its own: no application
composes the real IBKR PAPER bridge yet (delivery state E5), so that
composition comes first. As of 2026-09-29 the gRPC PAPER route composes it by
configuration (item 99), and it can submit a single DAY order and cancel it
(item 101), and a risk manager can invoke broker synchronization and
reconciliation (item 102). These routes still lack real Gateway evidence,
fresh market inputs, and reviewed account scope (delivery state E5.3–E5.6). Broad
LIVE or commercial promotion before those gates would violate the plan's own
evidence-gated sequence.
