# RFC 0002: deterministic FX pricing and risk contracts

- Status: implemented for the local deterministic core
- Owners: Trading Core / Risk-OMS
- Decision date: 2026-09-04
- Delivery phase: Phase 2

## Decision

Phase 2 selects FX as the next asset-class extension. It adds versioned,
value-dated reference and pricing contracts for FX spot, a single-date FX
forward, and two-leg FX swaps. Every mark is fixed-point and selected at an
explicit replay time. This is a core-only capability: it does not connect an
FX venue, configure a data vendor, submit an order, or broaden the active
US-equities PAPER/controlled-LIVE operating scope.

The unchanged flow is:

```text
normalized FX snapshot -> frozen FX risk candidate -> portfolio risk decision
-> OMS -> configured adapter route -> normalized execution -> accounting
-> immutable audit evidence
```

An `FxRiskCandidate` is only a typed form of `CandidateOrder`. It cannot call
an adapter, and it must still be evaluated by the ordinary portfolio risk
kernel before any OMS operation. Strategies and all clients remain limited to
declarative intents.

## Contracts

- `follon-fx` owns `FxPair`, `FxValueDate`, product kind, bid/ask terms, and
  `FxPricingSnapshot`. Source and receive timestamps, source sequence, pair,
  value dates, and a `fx.price.v1` reference version are retained in every
  snapshot.
- A spot/forward snapshot has one outright price and value date. A swap has
  independently priced near/far legs, and the near date must precede the far
  date. A value-date mismatch, stale quote, late quote, duplicate snapshot, or
  reused or ambiguous source sequence fails closed.
- Canonical instrument economics add `FX_SPOT`, `FX_FORWARD`, and `FX_SWAP`.
  The instrument settlement currency must equal the pair quote currency.
- Accounting accepts a `FX_SPOT` snapshot only for generic cash conversion;
  dated forward/swap prices cannot silently become spot conversion rates.
- Risk creates a generic candidate only from a fresh frozen snapshot. It
  records snapshot ID, reference version, and selected value date while using
  the existing aggregate exposure, currency, asset-class, delta, and policy
  controls.

## Persistence and migration

Migration `0004_fx_reference_pricing.sql` is additive and checksum-bound. It
adds tenant-RLS, effective-dated `fx_instrument_economics_versions` and
append-only `fx_pricing_snapshots`, retaining hash-bound terms and source
event linkage. Existing instrument and order records are untouched.

Rollback is reader-only: disable FX reference/pricing consumers and any
deployment feature flag, while retaining the tables and source evidence for
replay and audit. No migration drops data or rewrites prior records.

## Evidence and tests

The automated evidence covers input-order-independent replay selection,
staleness and value-date refusal, spot/forward/swap reference validation,
spot-only accounting conversion, ordinary risk-policy rejection, and the
existing workspace regression suite. It is not broker, venue, custody,
settlement, licensing, or regulatory acceptance evidence.
