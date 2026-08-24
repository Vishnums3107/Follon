# Product charter

## Purpose

Build a trustworthy trading operating system that unifies market data, research, strategy development, backtesting, paper trading, controlled live execution, portfolio risk, monitoring, journaling, and reporting.

This charter is a reference architecture for the complete product, not a
promise that one founder will ship every capability on a fixed calendar. The
committed product is always the smallest slice admitted by the current
evidence gate in the [roadmap](../06-delivery/03-roadmap-and-gates.md).

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
6. Customer and operational evidence before scope expansion.
