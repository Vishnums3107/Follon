# ADR 0003: execution-plan evidence and capability-gated routing

- **Status:** Accepted
- **Date:** 2026-09-04

## Context

The trading core requires deterministic translation of risk-approved parent
orders into executable child instructions without granting algorithms direct
broker transport access or allowing unverified venue routing. Previous phases
provided baseline TWAP, VWAP, arrival price, and unconstrained smart routing.
Iceberg and multi-algorithm wheel execution, venue capability verification,
and durable execution-plan evidence were required to prevent capability mismatches
and ensure auditability.

## Decision

1. Extend `core/execution` with `ExecutionAlgorithm::Iceberg` and
   `ExecutionAlgorithm::AlgoWheel`. Iceberg slices strictly conserve parent
   quantity and respect scheduled display intervals. AlgoWheel deterministically
   allocates across bounded non-wheel sub-algorithms using fixed basis points
   and breaks schedule offset ties deterministically.
2. Require explicit `VenueCapability` verification in `smart_route_with_capabilities`.
   Any route request referencing an unknown venue, duplicate capability, or
   unsupported order kind fails closed before producing route decisions.
3. Establish `ExecutionPlanEvidence`, `RouteDecision`, and
   `ExecutionBenchmarkEvidence` as immutable, content-addressed records bound
   by a SHA-256 fingerprint.
4. Provide additive PostgreSQL persistence in migration `0005_execution_plan_evidence.sql`
   enforcing tenant row-level security. Rollback disables Phase 3 readers without
   deleting retained audit evidence.

## Consequences

- All child order scheduling is deterministic and conserved; repeated runs
  with identical inputs yield bitwise identical execution plans.
- Venues cannot receive unsupported order kinds or unverified sizes.
- Execution plans, routing decisions, and frozen arrival/target benchmarks are
  permanently attributable through cryptographic fingerprints.
- Planners remain strictly bounded: strategies and execution algorithms cannot
  access broker adapters, credentials, or bypass Risk/OMS.
