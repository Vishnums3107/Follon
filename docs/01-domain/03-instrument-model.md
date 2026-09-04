# Instrument model

The instrument service owns a canonical identity for every tradable product. Symbols are display and lookup attributes only; they must never be permanent primary keys.

## Required attributes

| Group | Attributes |
| --- | --- |
| Identity | Internal instrument ID, symbol, exchange symbol, broker-specific IDs, asset class, venue, currency |
| Trading rules | Tick size, lot size, multiplier, trading calendar, market sessions, settlement rules |
| Derivative terms | Expiry, strike, option right, contract specification, FX base/quote pair and explicit value date(s) |
| History | Corporate-action history and version-effective reference-data changes |

## Initial boundary

The active operating boundary remains US equities and ETFs. The deterministic
reference core also validates listed options/futures and FX spot/forward/swap
economics for replay and evidence work. Those contracts do not configure a
tradable broker route, market-data vendor, custody arrangement, or live asset
class.

## Invariants

- Reference data is versioned and effective-dated.
- A market or broker event resolves to one canonical instrument before it reaches strategy or portfolio logic.
- Historical replay uses the reference-data version effective at the replayed time.
- Trading calendar and session logic are explicit dependencies, never inferred from local machine time.
