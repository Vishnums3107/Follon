# Dashboard feature integration and remaining-work status

**Implementation snapshot: 2026-08-22.** The Docker dashboard now provides ten
functional, workspace-specific read-only views over every capability that has
an implemented repository evidence contract. It no longer treats a list of
artifact links as a workspace implementation.

This record distinguishes three different claims:

1. **Integrated:** implemented code has a typed projection and a usable view.
2. **Evidence absent:** the code/view exists, but no matching local artifact has
   been generated or retained.
3. **External or not implemented:** a broker, customer, compliance, identity,
   payment, signing, or product capability does not exist in this repository or
   has not passed its evidence gate.

## Implemented dashboard architecture

- `/api/v1/workspaces` creates a bounded, read-only projection from allowlisted
  immutable evidence beneath `var/`.
- The browser validates every workspace count, artifact, dataset, backtest, and
  typed dashboard record before it renders it. Invalid projection data fails
  closed instead of being interpreted as operational state.
- JSON dashboard contracts, canonical NDJSON events, journals, experiment
  records, manifests, CSV dataset structure, and portable immutable-Parquet
  receipts are integrated without allowing artifact content to execute as
  markup or script.
- Browser ESM imports use explicit `.js` paths. This fixes the prior blank
  dashboard failure in which `/dist/evidence`, `/dist/catalog`, and related
  extensionless imports returned HTTP 404.
- Every workspace has summary metrics, domain-specific tables, state or gate
  interpretation, and direct links to its source artifacts.
- The generic evidence inspector remains available for complete source review
  and download. It is supporting functionality, not the workspace itself.
- The authenticated loopback deployment applies constant-time credential
  checks and a bounded per-direct-peer sliding-window rate limit with `429` and
  `Retry-After`. This remains an operator gate, not customer IAM or RBAC.

## Primary workspace coverage

| Workspace | Implemented integrated functions |
| --- | --- |
| Command Center | Container/dependency health, consolidated system/broker/strategy/risk status, environment readiness, derived external gate progress, operator attention queue, recent evidence |
| Research Lab | Dataset schema/row inventory, inert Jupyter notebook inventory, experiment catalogue, completed backtests, frozen option-chain analytics |
| Strategy Studio | Strategy/version/bundle identities, exact configuration and dataset binding, engine/source identity, isolated Python-worker contract |
| Backtest Explorer | Run comparison, canonical fill-level trade evidence, P&L/return/drawdown metrics, tagged regime/sensitivity dimensions, completion manifests, reproducibility hashes, options expiry scenarios |
| Execution Blotter | SIMULATION/PAPER/LIVE separation, intent/risk/order/fill timeline, correlation/causation, UNKNOWN counts, all reviewed out-of-order/cancel/replace lifecycle conditions |
| Risk Cockpit | Equity, exposure, drawdown, limits, breaches, kill switches, deterministic alerts, PAPER/LIVE reconciliation state |
| Portfolio | Operations/PAPER/LIVE internal positions, exact P&L attribution, options scenarios, cross-environment option-book reconciliation |
| Replay and Incidents | Event-type distribution, causal replay timeline, journal coverage, UNKNOWN and reconciliation incident state |
| Journal | PAPER, controlled-LIVE, operations, and commercial chain cursors, health, sequence, head hashes, decision/annotation fields, entry/correlation identities, and unified append-only records |
| Administration | Provisioning/subscription ledger, entitlement boundary, privacy/retention artifacts, signed-release artifacts, self-host readiness, auth/deployment boundary |

The Execution Blotter also renders every retained `risk.decision.v1` event with
the decision/intent identity, approval outcome, machine-readable reason codes,
exact evaluated inputs and limits, policy version, and actor. This keeps a
rejection explainable from the primary operating screen without adding any
browser-side order or policy mutation.

Jupyter `.ipynb` files are allowlisted as research evidence and summarized by
format, cell type, output count, kernel, and language. They are opened only in
the inert JSON evidence inspector; the server and browser never execute cells
or trust notebook HTML/JavaScript output.

Backtest Explorer renders each retained simulator-sourced canonical
`execution.fill.v1` record
with execution/order identity, instrument, side, quantity, price, and fee. It
also renders experiment `tags` as regime, sensitivity/scenario, and additional
dimensions bound to the run and specification fingerprint. The current local
experiment records are untagged, so the interface states `Not tagged` instead
of manufacturing regime or sensitivity classifications from performance.

Journal renders the operations journal's validated, non-secret `details` map as
decision or annotation evidence together with entry/correlation identity,
actor, hash, and source artifact. Journal append remains an operator-only CLI
action so the read-only dashboard cannot rewrite audit history.

## Implemented feature-domain coverage

| Domain | Dashboard integration | Current local evidence |
| --- | --- | --- |
| Market data and instruments | Imports/datasets, canonical columns and row counts, immutable Parquet receipts, bars, quote/feed-quality and complete settlement/reference/calendar/corporate-action capability map | Integrated when CSV or storage-receipt artifacts exist |
| Replay and simulation | Canonical causal events, strategy-to-intent-to-risk-to-OMS-to-fill-to-portfolio trail | Integrated |
| Research and backtests | Run specifications, Python-worker identities and bounded service API, metrics, accounting, reports, manifests, experiments, and explicit advanced-account assumptions | Integrated; legacy artifacts remain visibly bounded |
| PAPER operations | Status, audit head, working/UNKNOWN orders, kill switches, positions, reconciliation, 30-session gate | Integrated; observed gate remains 0/30 |
| Controlled LIVE | SHADOW/CANARY monitoring, audit, incidents, positions, reconciliation, 60-session gate | Monitoring integrated; no connected live adapter/control plane |
| Operations workbench | Risk, attribution, alerts, schedule, journal, configuration and reproducibility identities | Integrated |
| Options | Frozen chain, fixed-point European analytics/Greeks, expiry scenarios, expiration exercise/assignment settlement capability, declared-book reconciliation | Integrated; external broker-backed acceptance remains absent |
| Commercial and deployment | Ledger, provisioning/subscription facts, artifact inventory, release/readiness status and boundaries | Ledger integrated; release/readiness evidence is absent locally |

At the snapshot above the live API reports 76 artifacts, 4 datasets, 11
backtests, 4 experiment records, 195 canonical events, and 7 journal records.
One dataset is a portable immutable-Parquet receipt; the remaining three are
CSV sources. The latest typed PAPER, LIVE, operations, and options dashboards
are all available to their owning workspaces.

## Deliberately excluded privileged actions

The web server remains read-only. The following implemented local commands are
represented by their outputs and status but are not invoked from the browser:

- PAPER kill-switch activation/deactivation;
- operations journal append, schedule completion, and parameter revision;
- commercial provisioning/subscription observation and entitlement checks;
- privacy/retention execution;
- release key generation/signing and self-host readiness publication;
- any broker submit, cancel, replace, reconnect, approval, or credential action.

Exposing these through the current loopback evidence server would bypass the
filesystem ACL, explicit confirmation, two-person, offline signer, managed
secret, or broker boundary required by their owning runbooks. A future writable
control plane must have authenticated identities, roles, MFA, CSRF protection,
idempotency, approval policy, centralized tamper-evident audit, and separate
deployment review before any such button is added.

## Remaining product and external work

The following work is **not complete** and must not be represented as business
or live readiness:

### Research and storage

- Explicit spread/slippage pricing, post-cost limit protection, venue and
  instrument halts, bar latency, persistent working orders, and partial-fill
  caps are implemented and represented in Backtest Explorer. Effective-dated
  instruments fail closed after their configured end.
- The advanced backtest account implements economic delisting settlement,
  short/borrow/recall/financing behavior, fresh FX, and portfolio initial-margin
  checks. The existing CLI runner still emits legacy long-only/single-currency
  artifacts; the dashboard states that boundary instead of implying the
  advanced account was selected. Multi-account allocation remains excluded.
- Deterministic Parquet publication, DuckDB hash/row-validated registration,
  immutable versioned S3-compatible publication, read-after-write validation,
  and verified recovery are implemented. The dashboard indexes portable JSON
  receipts from its read-only evidence mount rather than opening binary
  Parquet or DuckDB files in the browser.
- Production KMS/key custody, object lock/retention, replication, backup and
  recovery drills, monitoring, and ownership remain deployment obligations;
  local MinIO success is not production approval.
- Add a remote strategy-worker transport only if a reviewed deployment requires
  it.

### PAPER and controlled LIVE

- Run and retain 30 real clean IBKR PAPER sessions; current evidence is 0/30.
- Independently pin, review, deploy, and operate the real PAPER TWS/Gateway
  environment.
- Implement and review a capital-bearing `LiveBrokerAdapter`, managed vault or
  keychain helper, authenticated four-eyes control plane, roles/MFA, monitored
  network boundary, backup/restore, and incident ownership.
- Run and retain 60 controlled small-capital live sessions; current evidence is
  0/60. The current LIVE command is deliberately monitoring-only.

### Operator adoption and options

- Observe five design partners completing normal work unaided; current evidence
  is 0/5.
- Obtain an independently verified option-capable broker export and a clean
  BACKTEST/PAPER/LIVE reconciliation session; current evidence is zero.
- Before real options operation, implement/review American exercise and
  assignment, early exercise/dividend risk, margin, settlement, corporate
  actions, commission/tax, halt/staleness, licensing, and broker multi-leg order
  semantics. No options order endpoint exists.

### Commercial and production deployment

- Implement a customer identity and authorization service with tenant isolation,
  roles, MFA, session/revocation policy, and gateway entitlement enforcement.
- Integrate a reviewed payment provider without accepting card data in Follon.
- Establish an HSM/KMS or controlled offline release signer, trusted-key
  distribution/revocation, SBOM/vulnerability review, and independent release
  approval. No signed-release or self-host-readiness receipt is present in the
  current local evidence set.
- Complete legal/compliance/privacy review, penetration testing, customer
  support/on-call, backup restoration, TLS/reverse-proxy hardening, monitoring,
  and retention operations for the exact deployment.
- Retain evidence for either ten paying professionals or three paying
  organizations; current evidence is zero.

## Verification

```powershell
npm --prefix apps/desktop run test:evidence
python apps/desktop/test/server_contract.py
python -m pip install python/storage-adapter
python -m unittest discover -s python/storage-adapter/tests -v
cargo test --workspace --all-targets
docker compose -f infra/compose.dev.yml up -d --build
Invoke-RestMethod http://127.0.0.1:8080/api/v1/workspaces
```

Manual visual acceptance is still required in an extension-free browser:

1. Hard-refresh `http://127.0.0.1:8080`.
2. Open all ten workspace navigation items and confirm that the title, summary
   metrics, domain tables, URL hash, and capability drawer update.
3. From at least one table in every workspace, open a source artifact and
   confirm that the evidence inspector loads it without a console 404.
4. Confirm `/dist/main.js`, `/dist/evidence.js`, `/dist/catalog.js`, and
   `/dist/workspaces.js` all return HTTP 200 and no inline-script CSP exception
   is introduced.
5. Repeat at desktop and narrow/mobile widths and verify keyboard activation of
   workspace navigation and clickable evidence rows.

This manual visual step is recorded because the connected browser-testing
surface was unavailable during the 2026-08-22 conformance session. Source,
contract, API, and container verification do not replace that final human
visual acceptance.

The documentation-wide continuation order is in the
[step-by-step implementation matrix](13-step-by-step-implementation-matrix.md).
The complete PDF requirement verdict is in the
[master-plan conformance audit](14-master-plan-conformance-audit.md).
