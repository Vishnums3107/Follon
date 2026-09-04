# RFC 0001: phased multi-asset platform delivery

- Status: accepted for Phase 1 implementation
- Owners: Trading Core / Risk-OMS
- Decision date: 2026-09-04
- Scope of this change: Phase 1 only

## Context and safety boundary

Follon currently has a deterministic, single-account PAPER OMS and a concrete
IBKR PAPER transport.  The existing `PaperBrokerAdapter` contract is already
broker-neutral at the call boundary, but cancel, replace, polling, and
reconnection are implicitly scoped to the one adapter instance.  That makes
adapter selection non-explicit and prevents a controlled configuration from
hosting more than one broker-account route.

This RFC defines an institutional target architecture, not evidence that a
broker, venue, asset class, custody arrangement, regulatory workflow, or live
service has been approved or connected.  Phase 1 remains PAPER-only.  It does
not add credentials to a client or strategy, alter risk evaluation, enable a
new live endpoint, or treat a configured route as broker acceptance evidence.

The invariant flow remains:

```text
market event -> strategy or desktop -> order intent -> risk decision -> OMS
-> adapter router -> selected broker/venue adapter -> normalized execution
-> portfolio update -> immutable audit event -> evidence UI
```

Only the OMS calls the adapter router.  Every outbound operation is account
scoped; an unresolved transport outcome remains `UNKNOWN` and can be resolved
only through normalized broker evidence or reconciliation.

## Target architecture

The reusable adapter surface is a canonical, versioned-in-code PAPER contract:

- `PaperBrokerRoute` binds one canonical `account_id` to an opaque deployment
  `adapter_id`, a canonical `venue_id`, and the literal `PAPER` environment.
- `PaperBrokerRegistry` owns a deterministic `BTreeMap` of routes and isolated
  adapter instances.  It selects an adapter only from an OMS request's
  `account_id`; unknown, duplicate, or cross-environment routes fail closed.
  The durable route fingerprint binds both route metadata and the adapter's
  non-secret implementation/configuration fingerprint.
- All stateful adapter operations are explicitly account scoped.  In
  particular, cancel and replace requests include an account, and polling and
  reconnection require an account argument.  This prevents an account-local
  client order id from accidentally reaching another route.
- Adapters remain protocol translation only.  FIX and native REST/WebSocket
  adapters can implement the same normalized contract later, without exposing
  broker types, credentials, or sockets to core domain, strategies, desktop,
  mobile, or web clients.
- `core/accounting` has a read-only, fixed-point multi-account aggregation
  projection.  It combines cash by currency and positions by their full
  `(instrument, asset class, currency)` identity while preserving deterministic
  signed market value.  It requires a common as-of time and configuration
  fingerprint and retains each source reconciliation identity. It performs no
  allocation, netting, trading, or cross-account transfer.

The existing IBKR mapping is one route per configured IBKR PAPER account:

```text
PaperAccount(account.paper.a)
  -> PaperBrokerRoute(adapter.ibkr.paper.a, venue.ibkr, PAPER)
  -> IbkrPaperGatewayAdapter / IbkrPaperAdapter
```

The existing adapter's local PAPER-only endpoint validation, idempotency key,
bounded private-process protocol, normalized evidence, reconnect, and
reconciliation behavior are retained unchanged.  A second account or venue is
an additional route and adapter instance, not a special case in an IBKR class.

## Phase plan

### Phase 1 — adapter router and multi-account aggregation

**Scoped diff.** Generalize `core/paper` operations to be account scoped; add
the deterministic `PaperBrokerRegistry` / `PaperBrokerRoute`; update the IBKR
PAPER implementations to the revised contract; and add the pure aggregation
projection in `core/accounting`.  Update the adapter, OMS, and architecture
documentation.  No strategy, desktop, protobuf, live-adapter, or credential
interface changes are included.

**Schemas and persistence.** The in-process PAPER adapter contract is revised
atomically with all producers and consumers.  This phase adds no PostgreSQL
schema because routes are supplied by controlled deployment composition and
the existing per-account PAPER journal already fingerprints account
configuration.  The subsequent durable route-registration migration must be
additive: append-only, tenant-scoped `broker_adapter_versions` and
`broker_account_route_versions` records, effective timestamps, a route
fingerprint captured on every OMS order, and a rollback that disables the new
configuration reader while preserving the records.  No existing order, audit,
or journal record will be rewritten.

**PAPER migration.** Deploy the revised binary with the exact legacy local-IBKR
registry composition only to reopen an existing v1 journal; it cannot create a
new journal or repoint the route. Run a clean reconciliation with no working or
`UNKNOWN` orders, retain the v1 journal and client ids as immutable evidence,
then start a separately named v2 journal with an explicitly fingerprinted
route. Add a second route only after its adapter-specific contract and PAPER
reconciliation evidence are reviewed. A route removal first blocks new intents,
leaves working orders `UNKNOWN` until reconciliation, and never discards
evidence.

**Tests.** Unit tests cover route validation, duplicate/unknown route refusal,
account-isolated routing, IBKR compatibility, fixed-point multi-currency
aggregation, and input-order-independent output.  Integration coverage places
the registry behind `PaperTradingService` so an intent still receives risk
evaluation before adapter submission.  The same ordered account snapshots
produce exactly equal aggregation output in repeated replay-style runs.

### Phase 2 — asset-class pricing and risk extensions

**Scoped diff.** Add the next selected asset class (FX or futures, selected by
reviewed value and data evidence) to canonical instrument, market-data,
pricing, risk, and settlement contracts.  Use explicit sessions, calendars,
contract multipliers, currencies, and fixed-point values.

**Schemas and migration.** Version instrument/reference and pricing snapshot
contracts before use.  Additive PostgreSQL reference/projection tables carry
effective-date versions and checksum-bound migrations; rollback disables the
new readers and preserves imported evidence.

**Tests.** Deterministic replay fixtures include session boundaries, stale data,
roll/expiry or FX value dates, exact risk values, and replay equality.

### Phase 3 — order types, algos, routing, and TCA

**Scoped diff.** Version parent/child intent and execution-plan contracts for
the remaining order/algo set, capability-gated smart routing, and immutable
pre/post-trade TCA.  Atomic combinations remain native-only or reject before
any leg is transmitted.

**Schemas and migration.** Add versioned plan, capability, route-decision, and
benchmark evidence records.  PostgreSQL changes are additive with backfill
readers and a rollback flag; old orders continue on their original contract.

**Tests.** Quantity conservation, deterministic schedule/routing ties,
cancel-before-replace, capability refusal, audit trace, and full replay
equality.

### Phase 4 — custody, settlement, corporate actions, and tax lots

**Scoped diff.** Add isolated sub-custodian/settlement adapters, DVP/RVP state
machines, corporate-action evidence, and explicit FIFO/LIFO/specific-ID lot
selection.  No settlement state is inferred from a missing message.

**Schemas and migration.** Version settlement instruction, lifecycle,
corporate-action, and lot-allocation records.  Use additive, reversible
projections and retention of every source instruction/evidence version.

**Tests.** Atomic balanced journals, idempotent late evidence, failed/unknown
settlement, action replay, lot conservation, and deterministic replay.

### Phase 5 — compliance, surveillance, and reporting adapters

**Scoped diff.** Add versioned pre-trade controls, surveillance observations,
and jurisdictional reporting adapter interfaces.  Compliance decisions are
explicit risk inputs and do not become broker-side bypasses.

**Schemas and migration.** Add effective-dated policy/list versions and
append-only observations/report payload metadata.  Additive migrations retain
previous policy and reporting evidence for rollback and audit.

**Tests.** Restricted-list/limit/wash-trade scenarios, alert idempotency,
reporting retry uncertainty, tenant isolation, and deterministic replay.

### Phase 6 — advisory and portfolio construction

**Scoped diff.** Implement risk-profile, IPS, model-portfolio, and rebalance
governance objects.  They create proposals or strategy constraints, never a
claim of personalized advice or direct executable order path.

**Schemas and migration.** Version policy/profile/model/rebalance proposal
contracts and add immutable, tenant-isolated version tables.  Rollback
disables proposal generation and retains all accepted/rejected evidence.

**Tests.** Version selection, constraint refusal, deterministic rebalance
output, approval trace, and replay equality.

### Phase 7 — entitlements and collaborative signals

**Scoped diff.** Add market-data entitlement/redistribution ledger plus
internal signal sharing and copy-execution as declarative intents.  Every copy
still traverses the same Risk/OMS route independently.

**Schemas and migration.** Version entitlement grants, source rights, signal
provenance, copy policy, and target-account intent records using additive RLS
tables.  Rollback disables sharing while preserving entitlement/audit history.

**Tests.** Tenant and entitlement denial, provenance, no direct-adapter access,
per-copy risk decision, idempotency, and deterministic replay.

### Phase 8 — mobile companion

**Scoped diff.** Build a read/approve-only client for monitoring, alerts,
four-eyes approval, and kill-switch requests.  It has no adapter or secret
dependency and cannot submit an executable broker command.

**Schemas and migration.** Version mobile session/device/approval request
contracts and add additive, revocable device records.  Rollback revokes client
sessions without deleting approval evidence.

**Tests.** Permission and signature checks, offline/retry uncertainty,
read-only boundary tests, approval replay, and deterministic core replay.

### Phase 9 — HA and observability

**Scoped diff.** Define multi-region topology, RTO/RPO objectives, structured
logs/metrics/traces, immutable failover evidence, and controlled chaos/failover
tests.  Failover must make ambiguous broker state `UNKNOWN` rather than retry
blindly.

**Schemas and migration.** Version health, checkpoint, recovery, and topology
evidence.  Additive migrations support dual-read/dual-write only with explicit
cutover and a documented rollback to the prior region.

**Tests.** Fault injection, restart/failover/reconciliation, RPO restore,
trace correlation, tenant isolation, and deterministic replay from the same
event/configuration/seed inputs.

## Definition of done for every phase

Every phase ships independently with versioned contracts, focused unit and
integration tests, a deterministic replay regression, architecture and
operator documentation, an additive PostgreSQL migration/rollback plan where
persistence changes, and a reviewer-traceable audit path from order intent to
portfolio evidence.  No client or SDK may obtain adapter or credential access.
