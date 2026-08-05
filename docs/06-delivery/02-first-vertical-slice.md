# First vertical slice

## Exact scope

Implement one end-to-end, non-live flow:

```text
Historical bar event
  → Python strategy worker
  → order intent
  → risk decision
  → simulated order
  → simulated fill
  → position update
  → P&L update
  → audit event
  → desktop display
```

## Minimal deliverables

1. Historical data importer and normalized bar-event contract.
2. Persisted event log and controllable replay clock.
3. One Python strategy that subscribes to bars and emits a canonical order intent.
4. Risk decision implementation with a minimal, versioned policy.
5. Simulated OMS/execution path with a deterministic fill model.
6. Decimal portfolio update and immutable audit events.
7. Desktop evidence view showing every transition and current simulated position/P&L.
8. A deterministic replay test proving byte-for-byte/semantic-equivalent event output for identical inputs.

## Explicitly excluded from this slice

Live broker connectivity, indicators, options, advanced charting, AI features, multiple brokers, full authentication, and comprehensive UI screens. They are all downstream of a correct, replayable order path.

## Acceptance criteria

- The order cannot reach the simulator without an auditable risk approval.
- Re-running with the same dataset, configuration, strategy bundle, and seed gives the same outcome.
- The UI can explain the source bar, strategy decision, risk decision, simulated fill, and resulting P&L.
- A failed/ambiguous simulated submission results in an explicit state; no step silently disappears.
