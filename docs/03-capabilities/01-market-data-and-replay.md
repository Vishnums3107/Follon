# Market data and replay

## Responsibilities

- Normalize quotes, trades, bars, sessions, and corporate actions.
- Preserve source timestamps and local receive timestamps.
- Detect stale, delayed, missing, and (when supplied) sequence-gapped data.
- Construct bars deterministically from normalized events.
- Apply explicit exchange calendars, holidays, halts, and session boundaries.
- Store raw source events separately from normalized events.
- Deliver historical replay through the same event interface used by live strategies.

## First implementation

Start with historical bar ingestion for US equities/ETFs, a normalized bar event, persistent event storage, and a controllable replay clock. Quote/trade feeds, gap handling, and live market-data subscriptions follow only after deterministic bar replay is proven.

## Invariants

- Replaying identical recorded inputs, reference data, configuration, and strategy version produces identical output events.
- Data freshness is explicit and available to risk checks.
- No event is silently reordered, dropped, or rewritten without an audit record.
- Historical simulations avoid future information and model corporate actions, delistings, fees, spreads, slippage, latency, session boundaries, halts, and relevant short constraints before being treated as decision evidence.
