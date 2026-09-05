# Master-plan conformance audit

**Audit updated: 2026-08-31; Phase 2 deterministic FX-core delta: 2026-09-04; Phase 3 execution-plan evidence delta: 2026-09-04.
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

The 2026-08-31 verification run retained the following local results:

- `cargo test --workspace --all-targets`: **148 passed, 0 failed, 1 ignored**.
  The ignored test is the disposable PostgreSQL round trip and requires an
  operator-provided `FOLLON_TEST_DATABASE_URL`.
- `cargo clippy --workspace --all-targets -- -D warnings` and repository-wide
  Rust formatting pass. `cargo audit` scanned 241 locked Rust dependencies
  against the current RustSec advisory database without a vulnerability finding.
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
| 5.1 Canonical instruments | Implemented for broker-neutral reference scope | `core/instrument` has permanent IDs, effective-dated versions, symbols, venue, asset class, currency, broker IDs, tick/lot sizes, multiplier, calendars, cash-security settlement lag, option underlying/expiry/strike/right/style/settlement, future root/last-trade/expiry/settlement/margin class, and FX spot/forward/swap base/quote/value-date terms. Current dataset/reference identity is projected into Research Lab and Strategy Studio. | FX reference contracts have no CLI/UI composition or broker route. Production vendor symbol-master ingestion, licensed data operations, and broker acceptance remain external; reference completeness is not permission to trade every declared class. |
| 5.2 Market data | Partial | Strict historical trade/bar import, deterministic OHLCV construction, normalized source/receive-time quotes, spread/size validation, duplicate/out-of-order/sequence-gap/delay/staleness classification, exchange sessions and halts, corporate-action inputs, Parquet publication, DuckDB verification, immutable S3-compatible publication, and dataset views are implemented. `core/fx` adds value-dated fixed-point spot/forward/swap snapshots with source/receive-time, sequence, staleness, and replay-order refusal. | No CLI/UI or adapter consumes the FX snapshots. No production licensed live quote/trade vendor connection, gap-repair operation, stale-feed operating history, broad vendor symbol-master ingestion, or corporate-action operations service exists. |
| 5.3 Python strategy SDK | Implemented local replay boundary | Isolated worker handshake, strategy/version identity, bundle hashing, deterministic bar-to-intent contract, point-in-time historical queries, deterministic SMA/EMA helpers, immutable portfolio snapshots, bounded saved state with fingerprints, bounded custom metrics, example strategy, schemas, and Strategy Studio projection are implemented. The Rust replay host sends the strict history/portfolio/cash/state frame to Python workers, applies replayed fills to the host-owned portfolio view, and rejects tampered fingerprints, look-ahead metrics, malformed metrics, or protocol drift. Strategy code cannot access broker adapters or credentials. | Direct Python fill/risk callbacks, a deployed gRPC strategy-worker host, and production worker deployment remain external integration work. |
| 5.4 Professional backtester | Implemented CLI projection; runner-internal accounting remains bounded | Event-driven replay, exact decimal accounting, spread, adverse slippage, attributed commission/exchange/regulatory charges, latency, per-bar partial-fill caps, persistent working orders, post-cost limit protection, sessions/halts, dividends/splits, point-in-time universe membership, long/short accounting, borrow availability/recall calculation, exact borrow/cash-debit financing, multi-currency FX, initial-margin capital checks, delisting settlement, immutable reports/manifests, experiment records, and Backtest Explorer capability evidence are implemented and tested. Every CLI backtest derives a hashed advanced-account result from the same canonical event stream and refuses publication when its capital or lifecycle checks fail. Explicit economics use `advanced_account`; older configurations use a deterministic fully-paid profile derived from immutable reference data. | Multi-account allocation and proof against production-size performance targets remain. The in-run `BacktestRunner` ledger is retained for backward-compatible event construction, so an operator must consume the advanced-account sidecar for advanced economics. |
| 5.5 OMS | Implemented for current market/limit scope | Stable client identities, idempotency, legal state transitions, cancel/replace, out-of-order evidence, UNKNOWN handling, restart recovery, reconciliation, and causal audit events exist in simulation/PAPER/controlled-LIVE. Execution Blotter renders the lifecycle. | It is not a claim of complete OMS coverage for every future order type, asset class, or live broker. |
| 5.6 EMS | Implemented as broker-neutral planning and local TCA; capital gate open | `core/execution` implements immediate, exact TWAP, forecast-volume VWAP, POV/participation, urgency-weighted arrival price, sequential display-size Iceberg, deterministic weighted AlgoWheel with schedule tie-breaking, strict post-only passive cancel/replace with monotonic chase collars, capability-gated multi-venue smart routing (`smart_route_with_capabilities`), stop/stop-limit bracket children, monotonic trailing stops, exact basket legs, and atomic ratio/net-price-protected options combinations. Content-addressed `ExecutionPlanEvidence` records bind parent order, scheduled slices, route decisions, frozen arrival/target benchmarks, and a SHA-256 fingerprint. `follon-tca` produces immutable parent-order implementation-shortfall reports against frozen benchmarks. Quantity conservation and safety boundaries are tested; the versioned gRPC service exposes scheduled execution, cancel-before-replace passive plans, and synchronized net-price-protected option combinations without discarding venue/order-kind/stop fields. | Options-combination atomicity requires a native-combo adapter or rejection before transmitting any leg. TCA relies on operator-supplied frozen evidence and does not validate a broker statement. Every vendor transport still needs independent human review and broker-backed PAPER/LIVE acceptance. |
| 5.7 Risk engine | Implemented portfolio kernel; operating gate open | `core/risk` evaluates gross/net, leverage, concentration, daily loss, drawdown, margin utilization, delta/gamma, instrument permissions/restrictions, sector/asset/currency/strategy buckets, open orders, order rate, self-trade, and kill state. A fresh FX snapshot can only create an ordinary local candidate with retained snapshot/version/value-date evidence; it still receives the same aggregate risk decision. The kernel returns exact reason codes, is exposed over gRPC, and is visible in Risk Cockpit capability mapping. | FX candidate construction has no gRPC or OMS composition. Production policy calibration, latency/load evidence, independent validation, live-feed staleness history, and clean broker-backed operating sessions remain external. |
| 5.8 Portfolio/accounting | Implemented multi-currency/margin kernel; external statement gate open | `core/accounting` provides per-currency balanced double entry, idempotent projection, fresh direct/inverse FX, spot-snapshot-only cash conversion, multi-currency cash/long/short valuation, initial/maintenance margin, excess liquidity, margin-call projection, FIFO/LIFO/highest-cost tax-lot disposal, and exact cash-debit/short-borrow financing accrual. PostgreSQL has deferred balanced-journal constraints; gRPC exposes valuation; Portfolio/Journal surface the capability. | Tax outputs are deterministic accounting facts, not jurisdiction-specific tax advice. Broad broker-statement ingestion, multi-prime allocation, and qualifying production reconciliation history remain external/integration work. |
| 5.9 Risk cockpit | Implemented for planned aggregate fields; operating gate open | The cockpit maps portfolio exposure, leverage/drawdown/margin/Greeks and bucket controls alongside kill switches, working/UNKNOWN orders, incidents, broker/reconciliation health, attribution, and evidence links. | Real alert delivery/on-call ownership, live-feed heartbeat history, and operated production evidence remain external. |
| 5.10 Audit and replay | Implemented for current scope | Canonical causal events, correlation/causation, append-only journals, hash-chain verification, immutable artifacts, restart replay, configuration/dataset/strategy hashes, and replay/incident/journal views are implemented. | Production retention/WORM policy, centralized tenant audit, independently operated log custody, and regulator/customer retention evidence remain deployment obligations. |

## Architecture and repository conformance

| Requirement | Status | Finding |
| --- | --- | --- |
| Rust deterministic core | Implemented | Workspace crates own domain, instruments, value-dated FX pricing, data, backtest, OMS/control plane, PAPER, LIVE safety, operations, options, and commercial primitives. Fixed-point `Decimal` is used for money/quantity decisions. |
| Isolated Python strategy/data tools | Implemented for local boundary | Strategy and storage packages are isolated and tested. The worker identity and bundle are bound into run evidence. |
| React + TypeScript + Tauri desktop | Implemented package; signed-installer acceptance open | React 19 owns the ten-workspace shell, Vite emits a production bundle, and a Tauri v2 host builds with no custom privileged commands. TypeScript, browser-module, evidence, server, Vite, and native Cargo checks pass; the Windows release build produced both MSI and NSIS bundles. Code signing/notarization, clean-machine install testing, and visual click acceptance remain release evidence. |
| PostgreSQL transactional store | Implemented adapter; live DB evidence open | Checksum-bound versioned migrations create events, atomic outbox, checkpoints, balanced journals, IAM/session/recovery-code, risk-policy, broker command/receipt, broker-account, strategy/config/reference versions, value-dated FX reference/pricing evidence, full order/execution/position projections, audit indexes, and billing-evidence tables with forced RLS. Composite tenant/parent foreign keys prevent cross-tenant linkage beneath RLS; OMS projections enforce lifecycle, TIF, price-field, quantity, client-ID, and idempotency invariants. Event append uses aggregate locks and content-bound idempotency; outbox claims use `SKIP LOCKED`; TLS and CI disposable-database paths exist. The available local server required an unavailable password, so this run did not retain a live DB receipt. |
| Parquet + DuckDB research store | Implemented locally | Deterministic Parquet, hash/row revalidation, catalogue registration, receipts, recovery verification, and dashboard indexing exist. |
| Object storage | Implemented locally; production gate | Versioned immutable S3-compatible publication and recovery exist against local MinIO. KMS, retention/object lock, replication, monitoring, and drilled production recovery remain external. |
| Protobuf/gRPC contracts | Implemented topology; production evidence open | `follon-trading-api` serves health, scheduled/passive/options-combination EMS, portfolio-risk, and margin APIs; validates tenant/account/strategy scope; migrates/health-checks PostgreSQL; requires database TLS plus server certificate/key/client CA in production; and is packaged in development and production Compose. Runtime container acceptance is not retained because Docker Desktop is unavailable. |
| REST/WebSocket UI boundary | Partial | A bounded read-only REST API serves all ten workspaces; the earlier local evidence client supports projection-only WebSocket evidence. There is no authenticated privileged write control plane. |
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
| Execution Blotter | SIMULATION/PAPER/LIVE separation, intents, risk, orders, fills, UNKNOWN and lifecycle state, frozen-benchmark TCA, and local risk-benchmark evidence | No submit/cancel/replace button; TCA and benchmark artifacts are read-only local evidence, not broker or availability acceptance |
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
| Property/model/fault testing | Partial | Unit, integration, end-to-end, malformed-input, fault-injection, restart, reconnect, out-of-order, latency, partial-fill, and tamper tests exist. A comprehensive state-model/property test program for every planned asset/order type is not complete. |
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
| Immutable audit and signed release | Implemented locally | Hash-chained journals, canonical manifests, detached Ed25519 signatures, and trusted-key verification exist. Production HSM/KMS custody and independent approval remain external. |
| SBOM | Implemented 2026-08-22 | `tools/generate_sbom.py` creates a deterministic CycloneDX 1.6 Cargo/npm/Python inventory bound to source revision and lockfile hashes; CI tests, generates, and retains it. Vulnerability disposition remains a release operation. |
| Dependency/static/secret scanning | Partial | CI has advisory/dependency and secret checks plus compiler/lint/test gates. Complete SAST/DAST coverage and security-operation ownership are not evidenced. |
| Dashboard authentication | Partial | Production mode requires protected credentials; exact constant-time Basic auth, no-store/CSP headers, direct-peer sliding-window rate limiting, `429` and `Retry-After` are tested. This is an operator-only loopback gate. |
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

## Business-readiness decision

**Not approved for capital-bearing or customer-facing production use.** The
repository mechanisms and packages are deployable candidates after automated
verification, but the open external gates above are material. The next
master-plan action remains to configure and independently review the real IBKR
PAPER environment, retain 30 clean sessions, complete security/legal/deployment
approvals, and record them through the tamper-evident acceptance ledger. Broad
LIVE or commercial promotion before those gates would violate the plan's own
evidence-gated sequence.
