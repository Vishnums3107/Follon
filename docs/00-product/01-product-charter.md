# Product charter

## Purpose

Build a trustworthy trading operating system that unifies market data, research, strategy development, backtesting, paper trading, controlled live execution, portfolio risk, monitoring, journaling, and reporting.

## Product promise

Users can develop, validate, deploy, and supervise systematic or discretionary workflows from one auditable platform. The same strategy API and event models are used in research, replay, simulation, paper, and live modes.

## Primary differentiators

- Deterministic replay of what the system knew and did.
- Explicit, versioned risk controls before every executable order.
- Broker-independent internal models with replaceable adapters.
- Local-first handling of strategies and broker credentials where possible.
- Explainable orders, rejections, fills, positions, and P&L changes.
- High-information-density professional UX.

## Success definition

The system is successful when a user can explain any trading decision from source data through strategy version, risk decision, broker response, portfolio effect, and audit trail.

## Operating principles

1. Correctness before speed.
2. Risk before convenience.
3. Replay before live trading.
4. One excellent workflow before many partial workflows.
5. Strategy code cannot bypass the trading kernel.
