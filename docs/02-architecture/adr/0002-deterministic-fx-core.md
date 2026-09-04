# ADR 0002: isolate FX pricing from transport

- **Status:** Accepted
- **Date:** 2026-09-04

## Context

FX extends the existing multi-currency accounting kernel, but spot, forwards,
and swaps have value dates that cannot be inferred from a current cash-conversion
rate. A vendor feed, trading adapter, or local clock in the pricing path would
break replay parity and could confuse a dated contract price with spot.

## Decision

Create `core/fx` as a transport-free deterministic module. It owns canonical
pairs, value dates, fixed-point quotes, pricing snapshots, source/receive-time
validation, and replay-stable selection. Canonical instruments use its value
date and pair types. Risk turns an explicit, fresh snapshot into an ordinary
candidate; accounting admits only a spot snapshot to its generic cash FX book.

FX reference/pricing persistence is tenant-isolated, append-only evidence. No
strategy, desktop, mobile, API client, or FX module may own a broker adapter or
credential. A future adapter may translate a risk-approved OMS request only
after a separate contract and reconciliation review.

## Consequences

- Replay inputs are complete: a result records pair, product, value date,
  source ordering, source/receive time, and reference version.
- Stale, late, duplicate, ambiguous, mismatched-date, or malformed pricing
  fails closed rather than selecting a substitute mark.
- This is not a deployment approval for FX data, FX execution, settlement,
  prime brokerage, custody, or a live broker route.
