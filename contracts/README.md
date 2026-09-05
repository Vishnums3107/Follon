# Contracts

This directory contains the versioned Protobuf and JSON Schema contracts for
event envelopes, instruments, order intents, risk decisions, lifecycle events,
executions, portfolios, strategy workers, and the complete backtest
configuration, plus read-only paper, controlled-live, operations-workbench, and
deterministic-options dashboards, controlled parameter-revision change
artifacts, and commercial-control artifacts for provisioning, pseudonymous
subscription evidence, privacy/retention plans, signed releases, and
self-hosting readiness. The portable storage dataset receipt binds a canonical
dataset identity/version to immutable Parquet and source hashes, row count, and
time bounds for dashboard and catalog consumers. A contract change requires
compatibility notes and deterministic serialization tests. Runtime
configuration rejects unknown fields and is content-addressed from its exact
bytes.

`json-schema/v1/fx-pricing-snapshot.schema.json` is the `fx.price.v1`
interchange shape for value-dated FX spot, forward, and swap observations. It
does not imply a vendor connection or an executable FX route; a source adapter
must still normalize and retain its evidence before the shared Risk/OMS path
can consider a resulting declarative intent.

`json-schema/v1/execution-plan-evidence.schema.json` is the `execution.plan.v1`
evidence interchange shape for deterministic parent/child execution plans,
capability-gated route decisions, and frozen arrival/target benchmarks. Each record
binds parent intent, scheduled slices, route decisions, and a SHA-256 fingerprint.
It represents auditable execution evidence, not broker acceptance or live execution.

`json-schema/v1/operator-task.schema.json` defines the `operator.task.v1`
attention queue contract (SOLO-03, SOLO-05). It preserves explicit cause,
severity, environment, account, linked evidence IDs, permitted actions, and
tamper-evident transition history without offering browser-side bypass of Risk/OMS.

`json-schema/v1/recovery-manifest.schema.json` defines the `recovery.manifest.v1`
evidence capsule contract (LIFE-01, LIFE-02). It binds schema and configuration
hashes, backup destinations with checksums, non-secret key recovery material references,
and measured restore drill results to verify offline and DR recoverability.

`json-schema/v1/research-hypothesis.schema.json` defines the `research.hypothesis.v1`
contract (RES-01). It binds mechanism, target universe, evaluation horizon, explicit
assumptions, failure criteria, and frozen evaluation plans (dataset, cost model, slippage)
before optimization begins, ensuring later alterations require a new attributable version.

`json-schema/v1/experiment-lineage.schema.json` defines the `experiment.lineage.v1`
contract (RES-04). It tracks parent runs, input/output fingerprints, candidate trials,
and failed-idea memory with explicit rejection reasons, upholding the invariant that
selecting a winning strategy never erases the search history that produced it.

`json-schema/v1/research-job.schema.json` defines the `research.job.v1` typed job
contract. It provides idempotent execution leases, frozen specification binding,
and monotonic state progression (QUEUED -> RUNNING -> COMPLETED/FAILED/CANCELLED).

`json-schema/v1/assistant-evidence.schema.json` defines the `assistant.evidence.v1`
contract (AI-01). It records read-only copilot queries, prompt versions, retrieved
record IDs, tool execution attempts, uncertainty scores in basis points, and human
disposition, ensuring model outputs remain evidence-grounded and offline-fallback safe.

## PAPER configuration v2 migration

`json-schema/v2/paper-configuration.schema.json` adds a required, non-secret
PAPER adapter route (`adapter_id`, `venue_id`, and `environment`). The
`follon-paper-status` composition registers that route with the OMS-owned
adapter registry and fingerprints it into new PAPER journals. Existing v1
configuration remains readable only as a migration bridge: it is routed through
the exact legacy local-IBKR route with a compatible fingerprint so an existing
journal can be reconciled and retained. The present CLI accepts only the
reviewed `adapter.ibkr.paper.*` / `venue.ibkr.paper` v2 binding; future adapter
implementations need their own reviewed composition. To move an existing
account to v2, first complete a clean reconciliation with no working or
`UNKNOWN` orders, retain the v1 journal as immutable evidence, then start a
separately named v2 journal. V1 cannot initialize a journal or add/change a
route.
