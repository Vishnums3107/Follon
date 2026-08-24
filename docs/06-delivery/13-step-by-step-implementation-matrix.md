# Step-by-step implementation matrix

**Reconciled snapshot: 2026-08-24.** This is the execution order for work
described across the product, architecture, capability, quality, issue, and
operations documents. It separates code that can be completed locally from
evidence that requires a broker, customer, owner decision, or production
environment. A later row cannot be presented as gate completion merely because
its mechanism exists.

## Ordered implementation sequence

| Step | Deliverable and documentary source | State | Evidence or next action |
| --- | --- | --- | --- |
| 0 | Deterministic research baseline - roadmap Months 0-5 | Complete | Versioned instruments/calendar, normalized input, strategy isolation, risk, OMS, accounting, reports, manifests, and repeatability tests |
| 1 | Explicit execution prices and pre-trade collar - backtesting/risk capabilities | Complete 2026-08-22 | Full spread pays/concedes one half-spread, slippage is adverse, fees are exact, post-cost limit prices are enforced, and an exact configured price collar rejects unsafe limit requests before order creation across simulation, PAPER, and controlled-LIVE; evaluated values remain audit evidence |
| 2 | Point-in-time tradability - market-data and backtesting capabilities | Complete for replay eligibility 2026-08-17 | Effective-dated instruments fail closed at `effective_to`; explicit venue-wide and instrument-specific halt intervals block strategy evaluation at exact UTC boundaries |
| 3 | Latency and partial fills - backtesting and reliability capabilities | Complete 2026-08-17 | Configurable bar latency, durable in-memory working orders, per-bar fill caps, unique execution IDs, cumulative portfolio updates, and terminal-state regression tests |
| 4 | Decision-grade storage adapters - storage architecture | Complete locally 2026-08-17 | Canonical bars publish as deterministic immutable Parquet; DuckDB revalidates hash and row count; S3-compatible publication is versioned, immutable, read-back verified, and recoverable; portable receipts are indexed by the dashboard |
| 4A | Advanced research/accounting kernels - backtesting and portfolio capabilities | Kernels complete 2026-08-24; CLI orchestration partial | Point-in-time universe validation, attributed charges, long/short crossings, borrow availability/recalls, financing, multi-currency FX, initial-margin rejection, delisting settlement, tax lots, and portfolio margin are implemented and tested. The default v1 artifact runner remains explicitly narrower. |
| 5 | PAPER operational gate - roadmap Months 6-8 | Mechanism implemented; external evidence pending | Independently configure the reviewed IBKR PAPER environment and retain 30 clean sessions; current observed count is 0/30 |
| 6 | Controlled LIVE gate - roadmap Months 9-11 | Monitoring mechanism implemented; capital-bearing control plane pending | Requires managed secrets, authenticated roles/MFA/four-eyes actions, reviewed live adapter, operations ownership, and 60 clean small-capital sessions; current count is 0/60 |
| 7 | Operator adoption gate - roadmap Months 12-14 | Workbench implemented; external evidence pending | Five design partners must complete normal workflows unaided; current count is 0/5 |
| 8 | Options operational gate - roadmap Months 15-17 | Analytics implemented; broker evidence pending | Obtain one independently verified option-capable broker export and reconcile BACKTEST/PAPER/LIVE; no options order path is authorized |
| 9 | Commercial production gate - roadmap Months 18-20 | Local evidence primitives and deterministic SBOM implemented; external platform work pending | Customer identity/authorization, payment provider, production key custody, legal/security operations, penetration testing, SBOM review/vulnerability disposition, and 10 professionals or 3 organizations remain |
| 10 | One demand-led expansion - roadmap Months 21-24 | Frozen | Select only after prior gates and measured paid demand |

## Scope decisions that prevent false completeness

- Borrow modeling is not silently approximated. The advanced account requires
  explicit shortability, availability, recall quantity, rate, and mark inputs;
  the legacy runner remains long-only and does not claim those controls.
- Portfolio capital allocation is not silently approximated. Multi-account and
  allocation behavior remains frozen by the active roadmap gate. The current
  backtest uses an explicit single account and opening cash balance.
- The effective-date boundary prevents post-delist replay bars from reaching a
  strategy, and the advanced account can close a signed position at an explicit
  zero-or-positive terminal settlement value. A legacy artifact without that
  versioned event remains unsuitable for delisting-dependent research.
- The dashboard is read-only. Privileged broker, risk, retention, signing, and
  provisioning actions remain in their controlled CLI/runbook boundaries until
  an authenticated write control plane is separately reviewed.

## Current next step

Step 5 is an external operating-evidence gate. The reviewed PAPER environment
must be independently configured and 30 clean sessions must be retained; the
current repository evidence remains 0/30. Code and generated local fixtures
cannot satisfy this gate.

The Step 4 storage mechanism is complete locally, but a production deployment
must still supply reviewed KMS/key custody, retention or object-lock policy,
replication, backups, recovery drills, monitoring, and operator ownership.
Local MinIO verification is not production storage approval.

Governance checklists in
[foundation readiness](01-foundation-readiness.md) require an accountable human
decision or external configuration. Code must not mark those boxes complete.

The detailed capability/architecture/UX/security verdict is in the
[master-plan conformance audit](14-master-plan-conformance-audit.md).
