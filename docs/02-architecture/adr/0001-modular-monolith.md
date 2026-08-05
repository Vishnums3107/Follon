# ADR 0001: Start with a modular monolith

- **Status:** Accepted
- **Date:** 2026-08-04

## Context

The product needs durable state, risk controls, broker recovery, deterministic replay, and strategy isolation. It is being built by a small team initially and must minimize operational burden without compromising component boundaries.

## Decision

Build one deployable Rust trading control plane composed of separately owned modules. Run strategy code in isolated Python workers over gRPC/Protobuf. Keep broker integration behind adapters. Use managed PostgreSQL and S3-compatible storage.

## Consequences

- Development, testing, and incident recovery are simpler than an initial microservice deployment.
- Strong module and contract boundaries are mandatory so future extraction remains possible.
- A control-plane failure has a broader blast radius, so persistence, restart recovery, reconciliation, and fail-safe trading policies are first-class requirements.
- Kafka and Kubernetes are deferred until evidence justifies their cost.
