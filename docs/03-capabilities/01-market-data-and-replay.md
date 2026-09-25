# Market data and replay

## Responsibilities

- Normalize quotes, trades, bars, sessions, and corporate actions.
- Preserve source timestamps and local receive timestamps.
- Detect stale, delayed, missing, and (when supplied) sequence-gapped data.
- Construct bars deterministically from normalized events.
- Apply explicit exchange calendars, holidays, halts, and session boundaries.
- Store raw source events separately from normalized events.
- Deliver historical replay through the same event interface used by live strategies.

## Implemented repository boundary

Historical trade/bar ingestion, deterministic bar construction, persistent
events, replay clock, effective-dated instrument/reference economics, and a
normalized quote contract are implemented. Quote monitoring validates spread
and sizes, retains source/local receive time and source sequence, and classifies
healthy, delayed, stale, gap, duplicate, and out-of-order observations.
`follon_market_data::repair_quote_gaps` and the `follon-repair-quotes` CLI fill
recorded sequence gaps from a supplied recovery batch. They never interpolate,
they refuse a batch that contradicts the recording, and they declare every
unfilled gap. Three things remain deployment/vendor gates:

- a licensed live vendor subscription;
- reconnecting and re-requesting a gap window from a vendor;
- observed feed-availability history.

## Invariants

- Replaying identical recorded inputs, reference data, configuration, and strategy version produces identical output events.
- Data freshness is explicit and available to risk checks.
- No event is silently reordered, dropped, or rewritten without an audit record.
- Historical simulations avoid future information and model corporate actions, delistings, fees, spreads, slippage, latency, session boundaries, halts, and relevant short constraints before being treated as decision evidence.
