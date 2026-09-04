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
