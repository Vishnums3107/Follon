# Scope and rollout

## Release 1: the only active product scope

- US equities and ETFs.
- One broker: Interactive Brokers.
- One customer-controlled account.
- Bar and quote data.
- Paper trading, then limited-capital live trading.
- Market, limit, stop, and bracket orders.
- End-of-day and intraday strategies.

Listed-options analytics and other later-release primitives may exist as
research artifacts, but they are not part of the active product scope and do
not make their release gate complete.

## Sequenced expansion

| Release | Additions |
| --- | --- |
| 2 | Listed options, multi-leg orders, Greeks, volatility surfaces, portfolio-level options risk, and futures through the existing adapter |
| 3 | Second broker, multiple accounts, allocation/rebalancing, FIX, team permissions, and approvals |
| 4 | India-specific broker support, selected SEBI-aligned workflow, and India-specific contract, expiry, margin, and session rules |

After every preceding operational and commercial gate has passed, permit only
one expansion choice: second broker, India integration, FIX, multi-account
allocation, or team approvals.

## Hard non-goals for the initial gated release cycle

- HFT, latency arbitrage, exchange membership, direct market access, or custody.
- Pooled money, personalized investment advice, copy trading, or a public strategy marketplace.
- Ten broker integrations, mobile, social features, custom databases, or custom cryptography.
- Kubernetes before measured need.
- Autonomous AI strategies without human-defined limits.

## Compliance boundary

Customers retain their own brokerage accounts and approve their strategies and limits. The product provides infrastructure, execution workflow, analytics, and records; it must not promise profitability.

As of 2026-08-13, SEBI's retail algorithmic-trading framework, implementation
standards, and exchange operational modalities are applicable to all Indian
stock brokers from 2026-04-01. India-facing API algo distribution is therefore
a current compliance perimeter, not a future rule to revisit only during
Release 4. Release 1 remains US-only: do not onboard an India-facing broker,
algo vendor, or customer workflow until Indian counsel and the selected broker
have reviewed registration, exchange, API, strategy-tagging, audit, and
operational obligations for the exact model. See the
[compliance posture](../05-quality-security/03-market-data-and-compliance.md).
