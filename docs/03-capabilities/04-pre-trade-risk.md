# Pre-trade risk engine

Every order intent passes explicit, versioned checks before an executable order exists. Broker controls are complementary; they do not replace platform controls, and platform controls do not claim to replace a broker-dealer's regulatory responsibilities.

## Required checks

| Area | Checks |
| --- | --- |
| Permission and market state | Instrument permission, session, halt, restricted list, data freshness |
| Order shape | Price collar, quantity, notional, open-order count, rate, duplicate, self-trade prevention |
| Capital and exposure | Buying power/margin reserve; symbol, sector, asset-class, currency, gross/net, concentration, and strategy-allocation limits |
| Loss control | Daily realized and total loss, drawdown state |
| Emergency control | Global, account, strategy, and symbol kill switches |

## Decision contract

A decision records approved/rejected status, machine-readable reason codes, input values, evaluated limits, policy version, decision timestamp, correlation ID, and actor/source. It is emitted even for a rejection.

## Invariants

- Risk evaluation is deterministic for identical input state and policy version.
- A strategy cannot bypass risk checks.
- A kill switch takes effect independently of strategy-worker health.
- Stale data blocks affected strategies or orders according to a documented safe policy.
- Dangerous policy changes require attributable versioning and appropriate approval.
- Aggregate limits are ratios of equity, so they cannot be computed once equity is
  zero or negative. That is not permission to skip them: a PAPER or controlled-LIVE
  account with aggregate risk configured refuses every order that does not move a
  position toward flat, with `PORTFOLIO_EQUITY_NOT_POSITIVE`. A trade that reduces a
  position, without crossing through flat, still passes, so an account that is
  underwater can close exposure and nothing else. A combination reduces risk only
  if every leg does.
