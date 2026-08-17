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
