# Contracts

This directory contains the versioned Protobuf and JSON Schema contracts for
event envelopes, instruments, order intents, risk decisions, lifecycle events,
executions, portfolios, strategy workers, and the complete backtest
configuration. A contract change requires compatibility notes and deterministic
serialization tests. Runtime configuration rejects unknown fields and is
content-addressed from its exact bytes.
