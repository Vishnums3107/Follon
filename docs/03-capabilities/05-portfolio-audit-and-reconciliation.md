# Portfolio, audit, and reconciliation

## Internal portfolio ledger

Track independently of the broker:

- Cash by currency, buying power, and margin estimates.
- Positions and average cost.
- Realized/unrealized P&L, fees, commissions, dividends, corporate actions, and FX translation.
- Strategy and account attribution.

Use decimal or fixed-point arithmetic. Binary floating point is not permitted for accounting values.

## Reconciliation

Compare internal orders, executions, positions, balances, and cash with broker records at reconnect, end of day, and scheduled checkpoints. Differences create incidents with evidence; they are never silently overwritten.

## Audit and incident replay

From an incident ID, an operator must be able to reconstruct:

1. Available market data and its freshness.
2. Strategy and configuration versions.
3. Intent generation and risk decisions.
4. Broker request and response history.
5. Order, fill, position, and P&L transitions.

The immutable [event envelope](../01-domain/02-event-envelope.md) is the evidence chain. Audit coverage for trading actions is a service objective, not an optional analytics feature.
