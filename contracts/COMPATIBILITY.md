# Contract compatibility policy

## PAPER adapter-route migration (v1 to v2)

PAPER configuration v2 adds a required non-secret adapter route. Producers
must publish `contracts/json-schema/v2/paper-configuration.schema.json` before
selecting a registry route. Consumers accept v1 only to reopen and reconcile an
existing single-account local-IBKR PAPER journal; its exact derived legacy
route intentionally retains the v1 journal fingerprint and cannot initialize a
new journal. New or changed routes require v2, which binds the route labels and
the adapter's non-secret implementation/configuration fingerprint to the
durable PAPER configuration. Rollback selects the v1-compatible reader only
for the unchanged pre-existing route; it does not delete, rewrite, or repoint a
journal. A v1-to-v2 cutover occurs only after clean reconciliation with no
working or `UNKNOWN` orders: retain the v1 journal as immutable evidence and
begin a separately named v2 journal.

The event envelope is the replay and audit boundary. Published v1 payloads are
additive-only: fields may be added only when consumers can ignore them, and no
field may be renamed, retyped, or silently change meaning. Breaking changes
require a new event type version, for example `risk.decision.v2`.

Decimal quantities and monetary values are JSON strings with up to eight
fractional digits. Floating-point JSON numbers are not permitted for accounting
or order quantities.


Protobuf fields are never reused. New fields receive new tag numbers; a removed
field is reserved before the next compatibility release. Worker output is not
trusted merely because it is schema-valid: the Rust core always validates the
canonical intent and applies risk before creating an OMS order.

## Verification

- Validate JSON ingress against the applicable versioned schema in `json-schema`.
- Ensure canonical event serialization remains stable with deterministic replay tests.
- Treat new required fields or changed enum meaning as a major contract change.
- Document a producer/consumer migration before publishing an incompatible version.

## Pre-release contract status

The checked-in operations, options, commercial-control, and storage-receipt
JSON schemas are pre-release repository contracts until a release explicitly
publishes them. Their required provenance fields must be frozen before external
adoption; after publication, changes such as a new required per-book identity,
predecessor fingerprint, retention confirmation field, dataset identity/hash,
or release-signature binding require a v2 schema plus a documented
producer/consumer migration.

## Execution plan evidence (Phase 3)

`contracts/json-schema/v1/execution-plan-evidence.schema.json` introduces the
additive `execution.plan.v1` evidence model. Existing orders remain on their
original lifecycle contracts and are not rewritten or backfilled. Additive
PostgreSQL tables `venue_capabilities`, `execution_plan_evidence`,
`execution_route_decisions`, and `execution_benchmark_evidence` retain source-event
linkage and SHA-256 content hashes under tenant RLS. Rollback disables Phase 3
planners and readers while preserving all stored evidence for replay and audit.

## Workstation and recovery contracts (Increment 1)

`contracts/json-schema/v1/operator-task.schema.json` and
`contracts/json-schema/v1/recovery-manifest.schema.json` are additive v1
operational contracts for attention management and recovery verification.
They preserve tenant and environment isolation, immutable transition history,
and strict separation of secrets.

## Research and assistant contracts (Increment 2)

`contracts/json-schema/v1/research-hypothesis.schema.json`,
`contracts/json-schema/v1/experiment-lineage.schema.json`,
`contracts/json-schema/v1/research-job.schema.json`, and
`contracts/json-schema/v1/assistant-evidence.schema.json` are additive v1
contracts for the research-to-backtest workflow. They enforce frozen hypothesis
specifications before evaluation, retention of failed optimization ideas, idempotent
job execution leases, and read-only, citation-grounded assistant evidence.

