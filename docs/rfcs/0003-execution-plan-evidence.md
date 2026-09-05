# RFC 0003: execution-plan contracts, capability routing, and TCA evidence

- Status: implemented for Phase 3
- Owners: Trading Core / Risk-OMS
- Decision date: 2026-09-04
- Delivery phase: Phase 3

## Scope

The existing execution crate already provides deterministic immediate, TWAP,
VWAP, participation, arrival-price, passive cancel/replace, brackets,
trailing stops, basket, smart routing, option combinations, and immutable TCA.
This phase closes the remaining planner-contract gap with iceberg and bounded
algorithm-wheel plans, explicit venue capability gating, and durable plan,
route-decision, and benchmark evidence structures.

Every planner consumes a risk-approved parent request and emits instructions;
it cannot submit an order. Any resulting child must still traverse OMS and a
reviewed adapter. An atomic option combination remains native-only or is
rejected before transmitting any leg.

## Contract decisions

- `ExecutionAlgorithm::Iceberg` splits a parent into sequential, display-size
  limit/market children. It conserves exact fixed-point quantity; no refresh is
  scheduled before the configured interval.
- `ExecutionAlgorithm::AlgoWheel` allocates a parent by explicit weights among
  bounded, non-wheel child algorithms. Each subplan is independently
  deterministic; no algorithm is selected from live outcomes or wall-clock
  state.
- Capability-gated routing requires one capability record for every routed
  venue and refuses unknown, duplicate, or unsupported order kinds before
  producing a route decision.
- Versioned execution-plan evidence binds parent identity, algorithm, children,
  source capability version, route decision, frozen benchmarks, and a
  content-addressed plan fingerprint. It is evidence, not a broker acceptance.

## Persistence and rollback

Migration `0005_execution_plan_evidence.sql` is additive and tenant-RLS. It
adds immutable plan, venue-capability, route-decision, and benchmark evidence
tables with source-event linkage and content hashes. Existing orders retain
their original contract and are not backfilled or rewritten.

Rollback disables the Phase 3 readers/planners and new composition flag while
preserving all evidence for replay and audit. It cannot retry or modify a
working broker order.

## Tests

Tests prove quantity conservation, deterministic wheel tie/order behavior,
iceberg interval/order, route capability refusal, cancel-before-replace,
frozen benchmark TCA, plan serialization, and existing full-workspace replay
regression. Clients and strategies receive no adapter or credential surface.
