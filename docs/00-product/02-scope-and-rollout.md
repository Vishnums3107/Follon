# Scope and rollout

## Release 1: the only active product scope

- US equities and ETFs.
- One broker: Interactive Brokers.
- One customer-controlled account.
- Bar and quote data.
- Paper trading, then limited-capital live trading.
- Market, limit, stop, and bracket orders.
- End-of-day and intraday strategies.

## Sequenced expansion

| Release | Additions |
| --- | --- |
| 2 | Listed options, multi-leg orders, Greeks, volatility surfaces, portfolio-level options risk, and futures through the existing adapter |
| 3 | Second broker, multiple accounts, allocation/rebalancing, FIX, team permissions, and approvals |
| 4 | India-specific broker support, selected SEBI-aligned workflow, and India-specific contract, expiry, margin, and session rules |

Only one expansion choice is permitted in months 21–24: second broker, India integration, FIX, multi-account allocation, or team approvals.

## Hard non-goals for the first two years

- HFT, latency arbitrage, exchange membership, direct market access, or custody.
- Pooled money, personalized investment advice, copy trading, or a public strategy marketplace.
- Ten broker integrations, mobile, social features, custom databases, or custom cryptography.
- Kubernetes before measured need.
- Autonomous AI strategies without human-defined limits.

## Compliance boundary

Customers retain their own brokerage accounts and approve their strategies and limits. The product provides infrastructure, execution workflow, analytics, and records; it must not promise profitability.
