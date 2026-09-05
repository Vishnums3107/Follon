# Follon: end-to-end product implementation plan

Date: 2026-09-05. Status: implementation roadmap, not a production-readiness certificate.

## Product outcome

Make one complete workflow excellent: select attributable data, develop a
versioned strategy, run a reproducible backtest, explain its results, promote
through PAPER gates, supervise risk and execution, reconcile the portfolio,
and reconstruct every decision. Extend asset classes and commercial distribution
only after that workflow has operational evidence.

"World's best" and "unique" are aspirations, not verified comparative claims.
Measure quality through reproducibility, completion rates, fault recovery,
decision explainability, accessibility, and independently reviewed release
evidence. This document proposes differentiated features without claiming
novelty, profitability, or operational broker readiness.

## Repository assessment

The strongest existing foundation is the Rust modular core, fixed-point domain
model, event contracts, deterministic replay/backtest, PAPER OMS, risk,
accounting, and evidence tooling. The Python SDK provides a bounded strategy
interface. PostgreSQL persistence, IAM primitives, and the gRPC service exist.
The React desktop combines an imperative evidence controller with embedded
React order tickets and a Tauri host.

The main product gap is composition: implemented modules and tests do not by
themselves provide a connected, authenticated, recoverable customer workflow.
Most desktop pages inspect bounded evidence projections. The native trading
host explicitly requires a configured Risk/OMS gateway. The local HTTP evidence
server is not a trading command service or a customer marketplace backend.

Source anchors:

- [Product charter](../00-product/01-product-charter.md)
- [Repository guide](../02-architecture/03-repository-guide.md)
- [Strategy and backtesting contract](../03-capabilities/02-strategy-sdk-and-backtesting.md)
- [Desktop implementation](../../apps/desktop/src/workspaces.ts)
- [Evidence API](../../apps/desktop/server.py)
- [Current conformance audit](14-master-plan-conformance-audit.md)
- [Existing release gates](03-roadmap-and-gates.md)

The working tree contained backend changes before this work began, including
execution-plan evidence and removal of the alerts package. Those changes were
preserved. Passing tests on this checkout do not independently establish the
correctness of every pre-existing change.

## Delivered in this change

| Area | Delivered behavior | Remaining limit |
| --- | --- | --- |
| Navigation | Twelve grouped workspaces; Strategies, Marketplace, Backtest, and News are explicit destinations; existing route IDs retained | Multiple definitions still need a shared typed route registry |
| Layout | Compact terminal header, scrollable sidebar, responsive collections, keyboard skip link and focus handling | Browser screenshots and assistive-technology acceptance still required |
| Marketplace | Search and category filtering over actual local research assets; incremental cards; inspect original artifact | No remote publishers, purchases, installs, or trust certification |
| Research tables | Text search and 20-record pagination across populated domain tables | Search covers only records supplied to each bounded table |
| News | Missing, non-integer, or out-of-range signal inputs display Unavailable | No new external news provider or browser-side classifier |
| Strategies | Deduplicate exports by complete specification fingerprint; display dataset version/hash and engine; exclude missing configuration hashes from counts | No strategy editor, bundle uploader, job launcher, or deployment service |
| Evidence | Older artifact responses cannot overwrite newer selections; unavailable downloads are hidden; section anchors preserve workspace | Additional loading-state and refresh-preservation work remains |
| Optional stream | Match WebSocket host and secure transport correctly; reject URL credentials; bound retained events and clear obsolete artifact download metadata | No new stream service or provider connection is supplied |
| Development | Vite forwards versioned evidence requests to the loopback server | Evidence service must be running separately |
| Ticket lifecycle | Unmount embedded ticket roots before changing workspaces; initially select PAPER without borrowing a LIVE account | Native routing and independently evidenced live activation remain external |

## Page and component specification

Keep navigation organized around user tasks: Monitor; Research; Operate; Govern.
Keep advanced implementation inventories and raw evidence below the principal
workspace task. Display environment, account, source time, connectivity, and
projection freshness consistently. Missing information must remain unknown;
never derive a green readiness badge from the absence of events.

| Page | Fully functional target | Acceptance evidence |
| --- | --- | --- |
| Command Center | Selected account/environment, feed freshness, active jobs, risk headroom, incidents, recommended next task | No false healthy state after disconnect; account switching never leaks another account's state |
| Strategies | Versioned editor/import, parameter schema, dependency lock, validation, bundle diff, lifecycle history | Tampered bundle rejected; draft cannot deploy; reviewed version cannot change in place |
| Marketplace | Publisher-owned versioned listings for strategies and datasets; signed manifests, provenance, license, review and revocation | Unverified asset cannot execute; revoked version cannot be newly installed; purchase retries cannot duplicate entitlements |
| News | Provider status, point-in-time headlines, deduplication, ticker/source filters, revision history, sentiment attribution | Replaying a decision excludes headlines received later; every signal identifies model/version and source event |
| Backtest | Dataset/config selection, queued jobs, progress/cancel, immutable outputs, compatible-run comparisons | Repeat run matches hashes; job restart preserves state; incompatible economics produce explicit comparison warning |
| Research Lab | Dataset validation/import, corporate actions, universe history, missing-data reports, notebook provenance | No silent look-ahead or survivorship changes; every dataset version is content addressed |
| Execution | Intent entry, review, risk outcome, OMS state, cancel/replace, reconciliation | Duplicate submission and reconnect cannot duplicate fills; UNKNOWN remains unresolved until authoritative evidence |
| Risk | Account and aggregate exposures, limits, scenario loss, policy version, kill controls | Every order traverses risk; stale prices and missing policy prevent approval |
| Portfolio | Positions, cash, fees, FX, settlements, attribution and explainable P&L | Balanced journals; independent broker reconciliation; all totals attributable to ledger entries |
| Replay & Incidents | Timeline, causal graph, saved incident ranges, reconstruction comparison | Same frozen inputs reproduce the event trail; intervention creates a separate scenario |
| Journal | Search, linked decisions, annotations, review workflow, export | Append-only edits with actor and predecessor; no mutation of historical records |
| Administration | Accounts, roles, session/MFA management, integration health, entitlements, backups | Tenant-isolation tests, session revocation, secret redaction and restore drills |

Shared components should include a route registry, account/environment bar,
source/freshness badge, exact-decimal renderer, accessible paginated table,
provenance inspector, job-state panel, typed error/retry panel, and an explicit
confirmation component for capital-affecting actions. An artifact's filesystem
modification time is not a substitute for market-event freshness.

## Required architecture and contracts

Preserve the existing dependency direction and mandatory sequence:

```text
Market event -> strategy -> declarative intent -> risk -> OMS
  -> simulator / approved broker edge -> execution -> portfolio -> audit -> UI
```

Extend the existing service boundary instead of placing execution or credentials
inside the read-only evidence server. Use a typed client behind the desktop
delivery layer. Authorize tenant, actor, account, environment, and permission on
every command and projection request; a hidden UI button is not authorization.

The following are proposed contracts, not currently exposed endpoints:

| Contract | Required fields/invariants | Owner |
| --- | --- | --- |
| StrategyVersion | Tenant, strategy/version, manifest hash, dependency lock, parameter schema, author, predecessor | Control plane and SDK |
| ResearchJob | Job ID, idempotency key, frozen specification, state version, worker lease, output manifest, failure reason | Control plane |
| DatasetVersion | Content hash, coverage, universe version, received-time policy, corrections and corporate actions | Market data |
| AssetListing | Publisher, immutable asset version, signed manifest, rights/license reference, review and revocation state | Commercial boundary |
| NewsProjection | Event ID, source, received/event times, revisions, linked instrument IDs, model provenance | News core and data delivery |
| PromotionDecision | Strategy/config/dataset identities, test results, risk policy, reviewers, scope and expiry | Control plane and risk |

Version schemas before introducing producers and consumers. Represent monetary
values as exact decimals or fixed-point values across transport; never use
JavaScript floating point for authoritative trading or accounting decisions.
Use durable idempotency, transactional outbox, monotonic state transitions, and
tenant-scoped immutable artifacts. Bound logs, artifact size, and query results.

Job execution requires process/container isolation, resource limits, a
restricted filesystem/network policy, dependency verification, termination,
and secret-free workers. Clearing environment variables alone does not sandbox
untrusted marketplace Python code. Never execute a discovered local artifact
merely because it is listed or has a backtest result.

## Delivery sequence

### 1. Establish a reliable application foundation

Owner: desktop and delivery layer. Dependencies: current evidence API.

Consolidate navigation definitions, make route/error/loading states explicit,
preserve filters and drafts during refresh, and introduce reusable collection
components. Replace whole-workspace imperative rebuilds incrementally with
owned React components. Add browser tests for deep links, back/forward,
out-of-order responses, empty and corrupt data, keyboard operation, and narrow
screens. Keep old routes valid.

Exit: all twelve routes load directly; local API failure has actionable retry;
no console errors, clipped controls, lost drafts, or misleading live status.
Capture desktop and mobile-width screenshots with both empty and populated
fixtures. Package and exercise the Tauri host separately from the web bundle.

### 2. Close the research-to-backtest workflow

Owner: control plane, SDK, market data, desktop. Dependencies: phase 1.

Create/import a strategy version; validate parameters and dependencies; select
a frozen dataset; submit an idempotent job; display queued/running/completed/
failed/cancelled states; inspect outputs; rerun from its manifest. Store worker
leases durably so a crash cannot silently publish a partial run as complete.

Exit: a new user completes the workflow without hand-editing evidence files;
repeat execution yields identical hashes; cancellation and restart have
regression evidence. Account for fees, slippage, corporate actions and universe
bias before displaying comparative performance claims.

### 3. Connect point-in-time news and market data

Owner: market data/news adapters and core. Dependencies: phase 2 contracts.

Implement provider adapters with explicit entitlements, rate limits, outage
states, source timestamps, received timestamps, duplicate handling and revision
history. Store sentiment model identity and bounded integer signal inputs.
Support source/ticker/time filtering and causal links to strategy decisions.

Exit: disconnect/reconnect and late-correction fixtures pass; replay admits only
information available at the simulated time. Missing or stale feed state is
visible without inventing headlines, quotes, or sentiment.

### 4. Close PAPER operations and recovery

Owner: risk, OMS, accounting, approved PAPER adapter, desktop. Dependencies:
phases 1 and 2; approved operational broker setup.

Connect the native command gateway to the owning application route. Present
intent, risk approval, submission, acknowledgement, fill, portfolio effect and
audit as separate states. Add durable request receipts, idempotent retry,
reconciliation, independent kill paths and restart recovery.

Exit: duplicate, out-of-order, partial-fill, cancel/reject, disconnect and restart
scenarios cannot cause duplicate orders or silent accounting loss. Exercise a
complete strategy-to-PAPER session with a recorded reconciliation report.
An accepted IPC request must never be labelled a broker execution.

### 5. Introduce trusted asset distribution

Owner: commercial/IAM, storage, strategy runtime. Dependencies: phase 2 and
worker isolation evidence; billing and legal decisions before paid commerce.

Start with private/team asset catalogues, then publisher review, signed
versions, license records, compatibility checks, revocation, and sandboxed
installation. Add commerce only after entitlement and payment-event replay
tests exist. Keep returns contextualized by dataset, benchmark, costs, sample
window and out-of-sample evidence; never rank assets using raw return alone.

Exit: install/upgrade/rollback/revoke is auditable and tenant isolated; unsigned
or incompatible assets cannot execute; publisher compromise has a tested
revocation path. Commercial launch needs independent operational evidence.

### 6. Harden production and evaluate controlled live

Owner: operations, security, risk and release engineering. Dependencies: all
applicable existing release gates, especially PAPER and reconciliation.

Complete identity integration, secret-provider deployment, backup/restore,
migration drills, observability, incident response, canary deployment and
rollback. Execute the repository's controlled-live approval and activation
process with independently reviewed broker and capital limits.

Exit: restore and recovery measurements meet agreed objectives; monitoring
detects stale feeds and reconciliation failures; rollback is exercised;
capital-bearing activation has current approval evidence. Do not enable live
transport through a UI redesign or a marketplace install.

## Differentiated product experiments

These are product proposals to validate after the foundational workflow works.

| Proposal | User value | Experiment and acceptance measure |
| --- | --- | --- |
| Decision passport | One attributable view from headline/data through strategy, risk, fill, accounting and review | Reconstruct sampled decisions with all causal links and no external spreadsheet |
| Counterfactual replay | Explore another risk rule or parameter without altering recorded history | Fork a frozen run; expose changed inputs and new output identity; identical fork inputs repeat exactly |
| Strategy robustness card | Show evidence quality instead of a single attractive return | Combine out-of-sample, regime, cost sensitivity and turnover; expose missing dimensions rather than invent a score |
| Promotion checklist | Show precisely why a strategy cannot progress to PAPER or live | Every unmet condition links to a test, policy or external gate; approval binds exact immutable versions |
| Incident replay packet | Export enough context for reproducible support and review | Reproduce a selected failure from a secret-redacted packet and verify all file hashes |
| Portfolio interaction lab | Expose correlated strategy risks before deployment | Replay combined strategies through shared capital and risk constraints using the same event ordering |

Do not present these as implemented features. Evaluate their usefulness through
task completion, explanation time, recovery time, and user comprehension before
adding dashboards or opaque optimization scores.

## Advanced solo-operator system: institutional depth, sustainable ownership

The advanced solo-operator specification below extends this roadmap. Its
capabilities are proposed targets, including extensions of existing primitives;
they are not additions to the delivered-feature inventory above.

### Ambition and public reference points

Build a personal trading institution whose research, execution, risk, records,
and recovery one operator can understand and supervise. The requested
"$100 million system" describes the desired depth and engineering quality;
it is not a valuation, a development budget, or a claim of economic returns.
"Imperishable" means maintainable, recoverable, portable, and replaceable over
many technology generations. No software can guarantee perpetual availability
or eliminate market, hardware, provider, and operator failures.

Use public product capabilities as reference points, not claims about access to
J.P. Morgan's private internal systems or permission to reproduce proprietary
technology. Public materials reviewed on 2026-09-05 provide these anchors:

| Public reference | Documented capability | Follon design interpretation |
| --- | --- | --- |
| [TradeStation RadarScreen documentation](https://help.tradestation.com/10_00/eng/tradestationhelp/rs/about_radarscreen.htm) | Real-time scanning with historical data and customizable indicators | An attributable scanner sharing definitions with charts and backtests |
| [TradeStation Portfolio Maestro documentation](https://help.tradestation.com/10_00/eng/tsportfolio/general/about_portfolio_maestro.htm) | Evaluate groups of strategies across baskets of symbols | Portfolio experiments with shared capital, risk, costs, and reproducible inputs |
| [TradeStation desktop overview](https://www.tradestation.com/platforms-and-tools/desktop/) | Strategy development/testing with EasyLanguage and Matrix/options tools | Linked chart, strategy, depth, and execution workspaces with validated commands |
| [J.P. Morgan Execute](https://markets.jpmorgan.com/pricing-and-execution/execute) | Multi-asset execution; pre-trade, real-time, and post-trade transaction-cost analytics; workflow tools and alerts | Connect analysis, risk, execution quality, and review around each order |

The Follon column is a proposed interpretation. Matching institutional workflow
quality does not confer institutional liquidity, market-data rights, credit,
broker capabilities, or regulatory permissions. Build the shared workflow first;
add a market only with its own data, model, adapter, and reconciliation evidence.

### Solo-operation design rules

- One primary task queue joins research, execution exceptions, settlement,
  maintenance, and review. Each item identifies urgency, consequence, evidence,
  estimated attention, and the next permitted action.
- A private installation is useful without customers, billing, ratings, social
  features, or a public marketplace. Private reusable assets take priority.
- Prefer a modular monolith, one documented local installation, and optional
  bounded workers. Add a distributed service only when measured workload or
  fault isolation justifies its operational cost.
- Run automated checks, reports, backups, and isolated research within explicit
  budgets. Live exposure, credentials, and risk-policy changes remain subject
  to the owning authorization and release contracts.
- Keep morning preparation, session monitoring, and end-of-day reconciliation
  short and repeatable. Advanced controls remain available through focused
  workspaces, keyboard navigation, and a searchable command palette.
- One person reviewing twice is not independent four-eyes approval. Where the
  existing live policy requires another authorized reviewer, use one; if none
  is available, that action remains unavailable. AI does not count as a signer.

### Priority and ownership convention

All feature IDs below are backlog identifiers, not implemented API names.

| Priority | Scope and dependency | Solo delivery rule |
| --- | --- | --- |
| S0 | Extend phases 1-2: dependable local research, evidence, and recovery | Finish before expanding the operating footprint |
| S1 | Extend phases 3-4: licensed feeds and connected PAPER supervision | Admit one complete workflow at a time |
| S2 | Extend phases 5-6: advanced portfolio/execution and trusted distribution | Require measured need and the applicable external gate |
| S3 | Later experiments and independently gated asset-class expansion | Prototype in research; no automatic production promotion |

Owners are repository boundaries, not a demand for separate employees. The
same developer may implement sequentially; external independence requirements
still apply where specified. Every item must retain the existing event, risk,
OMS, fixed-point accounting, and immutable-evidence invariants.

### A. Workstation and daily operating cockpit

| ID / priority | Feature and solo-person value | Owning boundary | Acceptance condition |
| --- | --- | --- | --- |
| SOLO-01 / S0 | Linked workspace layouts: charts, scanner, news, strategy, orders, and portfolio follow an explicitly selected instrument and account; save monitor layouts and keyboard shortcuts | Desktop | Switching symbol never changes account/environment implicitly; layout survives restart and a missing monitor |
| SOLO-02 / S0 | Universal command/search palette over strategies, instruments, runs, events, documentation, and permitted actions | Desktop and projections | Results preserve source and permission scope; an execution command opens a validated ticket rather than silently submitting |
| SOLO-03 / S0 | Daily operating brief: changed positions, unresolved orders, dataset failures, scheduled events, and due reviews with source times | Operations and desktop | Every statement links to evidence; unknown positions or unavailable feeds prevent an all-clear summary |
| SOLO-04 / S1 | Explainable market scanner: reusable indicator columns, event conditions, point-in-time universe membership, saved screens, and ranked reasons | Market data and strategy SDK | Scanner/chart/replay use identical versioned definitions; missing values remain missing; ranking can be reproduced |
| SOLO-05 / S1 | Attention queue and alarm consolidation: merge related incidents, suppress duplicates, show the underlying cause and acknowledgement deadline | Operations | Critical risk/reconciliation alarms cannot be hidden by convenience ranking; one incident produces one evolving task |
| SOLO-06 / S1 | Session playbooks: prepare, observe, operate, reconcile, and review; pre-approved away mode for unattended intervals | Operations and risk | Away-mode expiry or heartbeat loss follows a tested policy; no automatic flattening is inferred; broker-side protection and escalation limitations are explicit |

### B. Strategy engineering and advanced research

| ID / priority | Feature and solo-person value | Owning boundary | Acceptance condition |
| --- | --- | --- | --- |
| RES-01 / S0 | Hypothesis notebook: record the expected mechanism, horizon, universe, costs, failure conditions, and evaluation plan before optimization | Research/control plane | Freeze the plan before evaluation; later amendments create attributable versions |
| RES-02 / S0 | Strategy composition studio: typed signal, sizing, entry, exit, and portfolio constraints; visual and code views describe the same versioned representation | SDK and control plane | Both views produce equivalent test traces; unsupported semantics block conversion; strategies emit intents only |
| RES-03 / S0 | Event-by-event debugger: step through normalized data, indicator state, intent, risk decision, and simulated fill with causal links | Replay and desktop | Stepping preserves event ordering and hashes; inspect exactly what was known at the chosen time |
| RES-04 / S0 | Experiment graph and failed-idea memory: compare branches, retain rejected hypotheses, search similar prior trials, and track the entire optimization history | Backtest/control plane | Failed runs and tested candidates remain visible; selecting a winner cannot erase the trials that produced it |
| RES-05 / S1 | Robustness laboratory: held-out evaluation, walk-forward windows, leakage-aware splits, parameter stability, cost shocks, and uncertainty reporting | Backtest and research | Evaluation boundaries are frozen; overlap/leakage checks and trial counts accompany results; no claim that these methods guarantee future returns |
| RES-06 / S1 | Portfolio experiment engine: concurrent strategies with shared cash, order contention, fees, turnover, constraints, and allocation rules | Backtest, risk, accounting | Combined results come from joint event simulation; adding isolated strategy returns is not accepted as a portfolio simulation |
| RES-07 / S2 | Capacity and execution sensitivity: model participation, spread, latency, partial fills, queue uncertainty, borrow, and financing | Execution and backtest | Every modeled assumption is versioned; bar-only data cannot be presented as observed queue-level evidence |
| RES-08 / S2 | Champion/challenger research and strategy retirement: monitor declared performance/risk assumptions, run alternatives in shadow, and propose promotion or retirement | Control plane and operations | Deterioration creates a review task; model drift cannot silently increase size, replace code, or change a deployed version |

### C. News, data intelligence, and market understanding

| ID / priority | Feature and solo-person value | Owning boundary | Acceptance condition |
| --- | --- | --- | --- |
| DATA-01 / S0 | Data quality console: gaps, duplicates, corrections, schema changes, source disagreement, calendar errors, and affected-run lookup | Market data | Quarantined inputs cannot silently enter accepted research; corrections preserve earlier versions |
| DATA-02 / S1 | Point-in-time knowledge graph linking companies, instruments, filings, headlines, economic events, strategies, and exposures | News and research projections | Relationships have provenance and availability time; later-discovered facts cannot leak into historical replay |
| DATA-03 / S1 | News revision and novelty timeline: distinguish first report, syndicated duplicate, correction, model interpretation, and conflicting sources | News | A corrected headline does not overwrite the original; uncertain entity resolution is visible |
| DATA-04 / S1 | Event exposure calendar: show upcoming announcements, corporate actions, trading halts, expiry, and settlement obligations relevant to held or watched instruments | Instrument, news, operations | Calendar source/timezone/version is recorded; missing calendars or rights do not generate invented events |
| DATA-05 / S2 | Assumption-aware regime monitor: describe volatility, liquidity, trend and correlation changes and which strategy assumptions are affected | Research and risk projections | Indicators identify lookback, as-of time, uncertainty, and model version; explanatory labels are not asserted causes |
| DATA-06 / S2 | Feed substitution workbench: compare providers and test semantic differences before migrating | Market-data adapters | Symbol mapping, timestamps, adjustments, coverage, and entitlements pass parity checks; switch creates a new source identity |

### D. Institutional-style portfolio, execution, and control

| ID / priority | Feature and solo-person value | Owning boundary | Acceptance condition |
| --- | --- | --- | --- |
| EXEC-01 / S1 | Unified discretionary/systematic tickets: chart-initiated drafts, depth/ladder view where licensed, brackets and baskets where supported | Desktop, risk and OMS | Manual and automated intents share risk and audit; simulated/local protective orders are visibly distinguished from broker-native protection |
| EXEC-02 / S1 | Order decision passport: show opportunity, rejected alternatives, intended size, policy inputs, routing plan, fills, fees, and portfolio consequences | Execution and evidence projections | One order can be reconstructed without manually joining logs; every causal link references an immutable identity |
| EXEC-03 / S2 | Execution coach: pre-trade cost scenarios, in-flight deviation alerts, arrival/VWAP benchmarks, and post-trade implementation shortfall | Execution and accounting | Benchmark definitions, intervals, eligible observations and arrival references freeze before execution; realized VWAP uses subsequent attributable observations; estimates and realized costs remain separate; analysis never bypasses OMS |
| EXEC-04 / S2 | Capability-aware execution planner for schedules, participation, passive behavior, and order combinations | Execution and reviewed adapters | Unsupported instructions reject explicitly; cancel/replace races and residual quantities survive restart/reconciliation |
| RISK-01 / S1 | Exposure graph across instruments, strategies, sectors, currencies, and common factors; show concentration and shared dependencies | Risk and accounting | Every aggregate reconciles to positions and model inputs; factor-model estimates are labelled and versioned |
| RISK-02 / S2 | Scenario loss and liquidity lab: historical/synthetic shocks, correlation breaks, spread widening, financing stress and liquidation constraints | Risk, instrument and accounting | Scenario assumptions are frozen; displayed loss is not a guarantee or universal worst case; unavailable liquidity remains unknown |
| RISK-03 / S2 | Capital allocation workbench: compare constraint-based allocations under turnover, concentration, cash, and funding limits | Risk and control plane | Proposed allocations have reproducible inputs and a policy explanation; applying one follows intent/risk/OMS authorization |
| PORT-01 / S1 | Personal fund ledger: trade-to-cash reconciliation, fees, tax lots, distributions, FX and statement exports for professional review | Accounting and persistence | Exports reconcile to journals; jurisdiction-specific tax logic requires its own versioned rules and review |
| PORT-02 / S3 | Options/FX/futures workbenches: volatility surfaces, Greeks, stress, exercise/assignment, expiry, roll and settlement planning | Relevant domain modules and reviewed adapters | Existing analytics can be extended in research; each new executable market requires separate model and broker reconciliation acceptance |

### E. Evidence-grounded AI and bounded automation

AI is an optional assistance layer. The deterministic engine remains the
authority for prices, accounting, risk and execution. Persist model identity,
prompt/template version, retrieved evidence IDs, tool requests and outputs.
Replay stored AI outputs as evidence; do not assume a model will reproduce
identical prose or decisions from a seed. External text and imported strategies
are untrusted input, not instructions to the operator's tools.

| ID / priority | Feature and solo-person value | Owning boundary | Acceptance condition |
| --- | --- | --- | --- |
| AI-01 / S0 | Read-only research copilot: ask why a result changed, what caused a rejection, or which sources support a claim | Delivery layer and evidence retrieval | Answers cite permitted artifacts; absent evidence triggers an explicit unknown; model outage leaves core workflows usable |
| AI-02 / S1 | Strategy drafting assistant: convert a plain-language hypothesis into proposed typed rules, code, test cases and an assumptions list | SDK tooling | Generated code is isolated and reviewed; compilation/backtest are required; no auto-install or hidden deployment |
| AI-03 / S1 | Research critic: propose falsification tests, identify missing costs or data coverage, and find contradictory prior experiments | Research projections | Each criticism is attributable to a rule, source or declared heuristic; model agreement is not independent approval |
| AI-04 / S1 | Budgeted research scheduler: run approved experiment templates overnight, checkpoint progress, stop at compute/storage limits, and prepare a review brief | Control plane and isolated workers | Explicit datasets, allowed tools, CPU/GPU/time/spend caps and cancellation; no secrets, broker access, or self-expanded scope |
| AI-05 / S2 | Personal operations assistant: diagnose incident evidence, propose a tested runbook step, draft a repair or upgrade plan | Operations tooling | Automatic actions limited to documented idempotent repairs of non-trading services; no self-restart into active live submission or silent risk-policy changes |
| AI-06 / S2 | Model evaluation and portability console: compare factuality, citation accuracy, injection resistance, latency and cost on retained tasks | Quality tooling | A replacement model passes the fixed evaluation set; local/offline fallback exposes reduced capability rather than inventing answers |

### F. Private marketplace and reusable personal assets

| ID / priority | Feature and solo-person value | Owning boundary | Acceptance condition |
| --- | --- | --- | --- |
| ASSET-01 / S0 | Private asset vault for strategy templates, indicators, datasets, reports, dashboards and playbooks | Research and storage | Each asset has an immutable version, source, compatible runtime, dependencies, and reproducibility links |
| ASSET-02 / S1 | Evidence-based asset comparison: compare evaluation coverage, costs, missing assumptions, source freshness and compatibility | Desktop and research | Keep separate evidence dimensions; no fabricated trust score, rating, publisher badge, or return guarantee |
| ASSET-03 / S2 | Sandbox installation preview: show dependency changes, required permissions, estimated resource use, revocation and rollback behavior | Runtime and package boundary | Reject undeclared capabilities; preview must not execute untrusted code outside isolation |
| ASSET-04 / S2 | Portable strategy capsules: export authorized code/configuration, manifests, results and replay instructions | SDK and storage | Verify on a clean supported installation; licensed datasets are included only when redistribution rights allow it |

### G. Differentiation through connected evidence

The following combinations are the proposed signature experiences. Their value
is testable; commercial uniqueness would require separate market research.

1. **Explain this moment.** Select a timestamp and see the market knowledge,
   strategy state, portfolio, policy and decision available then. Link SOLO-01,
   RES-03, DATA-02 and EXEC-02. Acceptance: a reviewer reconstructs a sampled
   decision from the screen and verifies its original output identity.
2. **Show what would invalidate this strategy.** Link a frozen hypothesis to
   falsification tests, changing data quality, model drift and review tasks.
   Combine RES-01, RES-04, RES-05, RES-08 and AI-03. Acceptance: a known injected
   assumption failure creates the expected review with reproducible evidence.
3. **Why are these different strategies losing together?** Join exposure,
   market regime, shared data/provider dependencies, financing and order
   contention. Combine RES-06, DATA-05 and RISK-01. Distinguish observed
   co-movement from a proven causal explanation.
4. **What changes if this input is corrected?** Follow a revised dataset or news
   item to affected experiments and decisions; rerun a separate scenario while
   retaining the historical record. Combine DATA-01, DATA-03 and experiment
   lineage. Acceptance: identify every fixture-linked affected run without
   silently rewriting a previous result.
5. **Can I safely leave the desk?** Show unresolved uncertainty, current
   protection type, authorized unattended window, connectivity and escalation
   readiness. Combine SOLO-05, SOLO-06 and EXEC-01. Acceptance: loss of a required
   dependency prevents an affirmative readiness result.
6. **Rebuild my entire workspace.** Recover the application, approved assets,
   research history and reconciled account view on replacement hardware using
   the survivability contracts below. Acceptance: a recorded clean-machine
   exercise completes within the chosen recovery objective.

### H. Longevity, recovery, and replaceable infrastructure

Optimize for graceful degradation and demonstrated recovery. A functioning
screen is insufficient if its execution state, accounting, or source data is
uncertain. Restoring a database does not prove the broker has the same orders.

| ID / priority | Function | Required implementation and proof |
| --- | --- | --- |
| LIFE-01 / S0 | Recovery capsule | Preserve signed release identity, dependency locks, schemas, migrations, approved configs, runbooks, artifact manifests and backup metadata; secrets use a separate encrypted recovery process |
| LIFE-02 / S0 | Backup and restore verification | Automated encrypted backups, checksums, independently stored copies and clean-environment restores; declare recovery-point and recovery-time objectives and measure actual results |
| LIFE-03 / S0 | Offline research continuity | Cached documentation, permitted datasets, archived artifacts and deterministic research remain usable without cloud AI or billing; online trading dependencies are explicitly unavailable |
| LIFE-04 / S1 | Restart and reconnect discipline | Reconstruct durable receipts and reconcile broker evidence before resuming; ambiguous commands stay UNKNOWN; do not resend an uncertain submission as a new order |
| LIFE-05 / S1 | Watchdog with bounded recovery | Detect process death, stale feeds, disk pressure, certificate expiry, clock disagreement and broken backups; use restart budgets and escalation instead of endless restart loops |
| LIFE-06 / S1 | Failure drills | Simulate disk-full, power loss, corrupt backup, dropped acknowledgements, duplicated fills, partition, stale prices and unavailable operator; preserve expected state and recovery evidence |
| LIFE-07 / S2 | Single-writer standby | Only add a second execution host when justified; require fencing, durable ownership epochs and broker reconciliation so partition cannot create two active submitters |
| LIFE-08 / S0 | Long-term data readability | Versioned schemas, documented formats, migration fixtures, checksum inventory and export tools; retain readers for archived evidence and prove old artifacts remain interpretable |
| LIFE-09 / S1 | Dependency and vendor replacement | Inventory feeds, brokers, runtimes, libraries, licenses and end-of-support dates; retain adapter contracts and replacement tests; test entitlement and semantic differences before switching |
| LIFE-10 / S1 | Safe upgrades and reversible delivery | Signed packages, reproducible-build evidence where feasible, compatibility checks, snapshot-before-migration, canary and rollback drills; irreversible migrations need an explicit recovery plan |
| LIFE-11 / S1 | Security and access continuity | Least-privilege service identities, separated secrets, offline recovery material, rotation/expiry drills and auditable emergency access; protect credentials without making one lost device unrecoverable |
| LIFE-12 / S0 | Maintenance and resource budget | Track operator minutes, alert load, storage growth, compute spend and restore duration; cap research jobs and archive permitted data before resource pressure threatens operations |

Default deployment target: one well-documented workstation/server and an
independent backup destination. A cold spare is preferable to untested active
failover. Hardware sizing and network redundancy follow measured data volume,
broker requirements and recovery objectives; no institutional throughput is
assumed from a developer-laptop benchmark.

### I. Proposed contracts for the advanced functions

Introduce these through versioned contracts and their existing owning modules;
do not create a new microservice for each row. Reuse current identities and
receipts rather than duplicating authority.

| Proposed contract | Minimum contents and invariant |
| --- | --- |
| ResearchHypothesis | Mechanism, universe, evaluation horizon, assumptions, failure criteria, frozen evaluation plan and predecessor |
| ExperimentLineage | Hypothesis, parent runs, complete input/output fingerprints, all tested candidates, failure state and evaluation windows |
| KnowledgeSnapshot | Source records, available-at timestamps, entity-resolution version, rights reference and correction lineage |
| AssumptionMonitor | Bound strategy version, input sources, thresholds, explicit evaluation time, observed violation and review task |
| AutomationMandate | Owner, allowed tasks/tools, input scope, resource limits, expiry, cancellation, and required approval class; no implicit broker authority |
| AssistantEvidence | Model/template version, retrieved record IDs, generated output, tool attempts, uncertainty and human disposition |
| OperatorTask | Cause, severity, affected account/environment, evidence IDs, permitted action, acknowledgement and resolution history |
| RecoveryManifest | Release/schema identities, backup and checkpoint hashes, key-recovery reference without secret material, tested restore procedure and last drill result |
| AdapterQualification | Version, supported capabilities, source semantics, reconciliation tests, operational gates, expiry and revocation |
| ContinuityPolicy | Allowed degraded modes, unattended interval, dependency requirements, restart budget, escalation and return-to-service conditions |

### J. Solo delivery order and measurable quality

Keep one implementation workstream active. Choose the next item by its effect
on the complete workflow, defect reduction, operator attention saved and ongoing
maintenance burden. Do not implement the whole catalogue simultaneously.

| Increment | Implement together | Demonstrable result |
| --- | --- | --- |
| 1: Own the workstation | SOLO-01/02/03, DATA-01, ASSET-01, LIFE-01/02/03/08/12 | Search and inspect a local strategy/run, see missing prerequisites, export it and restore its evidence |
| 2: Close research | RES-01/02/03/04, AI-01, typed job and hypothesis contracts | Draft a hypothesis, create a strategy, debug a run and explain a result from retained sources |
| 3: Establish robustness | RES-05/06, DATA-02/03/04, AI-02/03/04, ASSET-02 | Run bounded evaluations with point-in-time news and compare the full trial history |
| 4: Operate PAPER alone | SOLO-04/05/06, EXEC-01/02, RISK-01, PORT-01, LIFE-04/05/06/09/10/11 | Supervise a complete PAPER session and recover a simulated fault with reconciled evidence |
| 5: Add measured depth | Selected RES-07/08, DATA-05/06, EXEC-03/04, RISK-02/03, AI-05/06, ASSET-03/04, LIFE-07 | Admit only capabilities with justified workload, tests and applicable production gates |
| 6: Evaluate expansion | PORT-02 and separately approved market/provider adapters | One new market achieves its own data, research, risk, execution and settlement acceptance |

The increments prioritize work inside the earlier six delivery phases; they
do not waive or reorder mandatory release gates. A private solo system need
not build public commerce or enterprise customer acquisition to improve its
research tools. Applicable live and asset-class gates still govern execution.

Proposed acceptance targets below are engineering objectives to benchmark on
declared hardware and workloads, not measured current performance:

| Dimension | Proposed target and measurement |
| --- | --- |
| Reproducibility | Identical supported engine/runtime and frozen inputs produce identical canonical outputs across the retained regression corpus |
| Accounting and attribution | Every accepted execution maps to balanced attributable entries; no unresolved reconciliation discrepancy is labelled healthy |
| Operator workload | Routine morning and closing workflows each take at most 15 minutes on the declared reference session; measure active operator time and exclude exceptional incidents visibly |
| Attention quality | Zero missed critical alarms in injected failure scenarios; track duplicate alarms, false positives and interruptions per session |
| Recoverability | Meet the existing ordinary control-plane recovery objective of 15 minutes in an exercised setup; measure data RPO separately and reconcile broker state before resuming |
| Durability | No locally acknowledged durable command/audit loss under the declared local crash model; host/site-loss guarantees depend on verified independent replication, not a periodic backup claim |
| Interactivity | Proposed p95 local search/navigation response below 250 ms for a declared indexed dataset; report cold starts and degraded-mode behavior separately |
| Risk latency | Validate the existing local-core p99 objective below 5 ms under declared contention; report adapter/network time separately |
| Availability | Use the existing 99.9% production objective with a stated measurement window and component scope; count stale or unusable trading service as unavailable |
| Portability | Restore a retained release and evidence capsule on a clean supported machine without relying on the original development environment |
| AI assistance | Zero unauthorized tool actions in the retained adversarial evaluation set; publish citation/error rates and abstention behavior; do not generalize a finite test result to zero real-world risk |
| Maintainability | Track monthly maintenance time and supported dependency age; set the operator's sustainable budget before admitting a feature that adds a new service |

For every accepted advanced feature attach a small evidence packet: user task,
contract version, sample dataset, successful trace, failure trace, acceptance
measurement, operating cost, recovery procedure and UI demonstration. This is
how the system earns institutional-quality behavior while remaining manageable
by one person.

## Verification record for the original implementation

Validation on the working checkout during this change:

- `cargo test --workspace --quiet`: passed; one integration test remained ignored.
- `python -m pytest -q`: 35 passed under the current root configuration.
- Desktop `npm run typecheck`, `npm run test:evidence`, and `npm run build:web`:
  passed during implementation; final reruns recorded in the task report.
- `python apps/desktop/test/server_contract.py`: updated for renamed pages and
  Marketplace; 12 tests passed in the independent test pass.
- Persistent DOM-harness regressions cover marketplace category/search,
  pagination, empty states, inspect callbacks, and news row-to-artifact mapping
  after filtering. Signal and strategy-specification identity regressions also
  run through `test:evidence`. These tests do not replace browser layout checks.
- Browser runtime returned no connected browsers. Visual, responsive rendering,
  browser console and assistive-technology checks are not certified here.

The root Python invocation does not encompass every explicitly runnable test
script or an external database/broker deployment. The Rust workspace excludes
the separately built Tauri host. No actual provider, billing, authenticated
multi-tenant deployment, broker session, or operational restore was exercised
by these source-level changes.

For each subsequent phase retain: reviewed contracts, deterministic fixtures,
failure/restart tests, user-flow tests, security-boundary tests, visual evidence,
operator runbook, migration and rollback procedure, and explicit release-gate
status. Replace open-ended "fix all bugs" claims with a reproducible issue list,
severity, reproduction, owning module, regression, and closure evidence.

## Verification record for Increments 3 and 4 implementation

Validation on the working checkout during the delivery of Increments 3 and 4:

- `cargo test --workspace --quiet`: 11+9+1+3+... all passed; 0 failed.
- `python -m pytest -q`: 35 passed cleanly in 1.57s.
- `python apps/desktop/test/server_contract.py`: 12 passed in 1.284s.
- Desktop `npm run typecheck`: clean TypeScript compilation without warnings.
- Desktop `npm run test:evidence`: all 8 regression suites passed:
  * CLI dashboard / desktop evidence-contract test passed
  * Browser module graph / workspace shell contract passed
  * Workspace sentiment and reproducibility identity regression tests passed
  * Marketplace and paginated news collection regressions passed
  * Workstation cockpit, typed routes, daily brief, and Increment 1 contract regressions passed
  * Research contracts, schemas, workspace cockpits, and event debugger regression tests passed
  * Robustness laboratory, portfolio experiment, point-in-time knowledge graph, and scheduler regression tests passed
  * PAPER operations, market scanner, decision passport, exposure graph, fund ledger, and watchdog recovery regression tests passed
- Desktop `npm run build:web`: Vite production client bundle built in 303ms.
- 9 new versioned v1 JSON schemas added under `contracts/json-schema/v1/`:
  * `robustness-evaluation.schema.json` (RES-05)
  * `portfolio-experiment.schema.json` (RES-06)
  * `knowledge-snapshot.schema.json` (DATA-02)
  * `event-exposure-calendar.schema.json` (DATA-04)
  * `automation-mandate.schema.json` (AI-04)
  * `order-decision-passport.schema.json` (EXEC-02)
  * `exposure-graph.schema.json` (RISK-01)
  * `fund-ledger-statement.schema.json` (PORT-01)
  * `continuity-policy.schema.json` (SOLO-06, LIFE-04/05/06)

## Verification record for Increments 5 and 6 implementation

Validation on the working checkout during the delivery of Increments 5 and 6 (Measured Depth & Multi-Asset Expansion):

- `cargo test --workspace --quiet`: passed cleanly across all packages; 0 failed.
- `python -m pytest -q`: 35 passed cleanly in 1.46s.
- `python apps/desktop/test/server_contract.py`: 12 passed in 1.314s.
- Desktop `npm run typecheck`: clean TypeScript compilation without errors or warnings.
- Desktop `npm run test:evidence`: all 9 regression suites passed:
  * CLI dashboard / desktop evidence-contract test passed
  * Browser module graph / workspace shell contract passed
  * Workspace sentiment and reproducibility identity regression tests passed
  * Marketplace and paginated news collection regressions passed
  * Workstation cockpit, typed routes, daily brief, and Increment 1 contract regressions passed
  * Research contracts, schemas, workspace cockpits, and event debugger regression tests passed cleanly
  * Robustness laboratory, portfolio experiment, point-in-time knowledge graph, and scheduler regression tests passed cleanly
  * PAPER operations, market scanner, decision passport, exposure graph, fund ledger, and watchdog recovery regression tests passed cleanly
  * Measured depth & multi-asset expansion regression tests (Increments 5 & 6) passed cleanly
- Desktop `npm run build:web`: Vite production client bundle built in 258ms.
- 7 new versioned v1 JSON schemas added under `contracts/json-schema/v1/`:
  * `assumption-regime-monitor.schema.json` (DATA-05)
  * `feed-substitution-parity.schema.json` (DATA-06)
  * `execution-coach-benchmark.schema.json` (EXEC-03, RES-07)
  * `scenario-loss-simulation.schema.json` (RISK-02)
  * `capital-allocation-plan.schema.json` (RISK-03)
  * `sandbox-installation-preview.schema.json` (ASSET-03, ASSET-04)
  * `adapter-qualification.schema.json` (LIFE-07, PORT-02)
- Domain types, typeguards, and parsing functions added to `apps/desktop/src/evidence.ts`.
- Desktop panels integrated across 6 workspaces in `apps/desktop/src/workspaces.ts`:
  * `feed-substitution-panel` (Research Lab)
  * `regime-monitor-panel` (News Cockpit)
  * `execution-coach-panel` (Execution Blotter)
  * `scenario-loss-panel` (Risk Cockpit)
  * `capital-allocation-panel` (Risk Cockpit)
  * `sandbox-preview-panel` (Marketplace)
  * `adapter-qualification-panel` (Administration)

## Verification record for Section G Connected Evidence & Advanced Capabilities

Validation on the working checkout completing the entire end-to-end product plan (Section G Signature Connected Experiences & Advanced Capabilities RES-08, EXEC-04, AI-05, AI-06, ASSET-04, PORT-02):

- `cargo test --workspace --quiet`: passed cleanly across all packages; 0 failed.
- `python -m pytest -q`: 35 passed cleanly in 2.61s.
- `python apps/desktop/test/server_contract.py`: 12 passed in 1.252s.
- Desktop `npm run typecheck`: clean TypeScript compilation without errors or warnings.
- Desktop `npm run test:evidence`: all 10 regression suites passed:
  * CLI dashboard / desktop evidence-contract test passed
  * Browser module graph / workspace shell contract passed
  * Workspace sentiment and reproducibility identity regression tests passed
  * Marketplace and paginated news collection regressions passed
  * Workstation cockpit, typed routes, daily brief, and Increment 1 contract regressions passed
  * Research contracts, schemas, workspace cockpits, and event debugger regression tests passed cleanly
  * Robustness laboratory, portfolio experiment, point-in-time knowledge graph, and scheduler regression tests passed cleanly
  * PAPER operations, market scanner, decision passport, exposure graph, fund ledger, and watchdog recovery regression tests passed cleanly
  * Measured depth & multi-asset expansion regression tests (Increments 5 & 6) passed cleanly
  * Connected evidence & advanced capabilities regression tests passed cleanly
- Desktop `npm run build:web`: Vite production client bundle built in 260ms.
- 6 new versioned v1 JSON schemas added under `contracts/json-schema/v1/`:
  * `champion-challenger-evaluation.schema.json` (RES-08)
  * `capability-execution-planner.schema.json` (EXEC-04)
  * `operations-diagnosis-runbook.schema.json` (AI-05)
  * `model-evaluation-benchmark.schema.json` (AI-06)
  * `strategy-capsule-manifest.schema.json` (ASSET-04)
  * `multi-asset-expansion-plan.schema.json` (PORT-02)
- Domain types, typeguards, and parsing functions added to `apps/desktop/src/evidence.ts`.
- Section G signature connected experiences and capability panels integrated in `apps/desktop/src/workspaces.ts`:
  * `#away-desk-readiness-panel` in Command Center (Experience 5: "Can I safely leave the desk?", SOLO-05, SOLO-06, EXEC-01)
  * `#input-correction-panel` in Research Lab (Experience 4: "What changes if this input is corrected?", DATA-01, DATA-03, Lineage)
  * `#champion-challenger-panel` in Strategy Studio (RES-08)
  * `#strategy-invalidation-panel` in Strategy Studio (Experience 2: "Show what would invalidate this strategy?", RES-01/04/05/08, AI-03)
  * `#execution-planner-panel` in Execution Blotter (EXEC-04)
  * `#joint-correlation-panel` in Risk Cockpit (Experience 3: "Why are these different strategies losing together?", RES-06, DATA-05, RISK-01)
  * `#multi-asset-panel` in Portfolio (PORT-02: Options/FX/Futures roll & settlement planning)
  * `#explain-moment-panel` in Replay & Incidents (Experience 1: "Explain this moment", SOLO-01, RES-03, DATA-02, EXEC-02)
  * `#strategy-capsule-panel` in Marketplace (ASSET-04)
  * `#operations-assistant-panel` in Administration (AI-05)
  * `#model-evaluation-panel` in Administration (AI-06)
  * `#workspace-rebuild-panel` in Administration (Experience 6: "Rebuild my entire workspace", LIFE-01..03)

