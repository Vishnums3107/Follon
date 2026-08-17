# Months 15–17 deterministic options implementation status

**Status:** versioned European-option chain analytics, multi-leg expiry scenarios, and exact cross-environment reconciliation are implemented and tested. This is not evidence of a broker-connected options account. Observed options reconciliation from an independent PAPER/LIVE broker adapter: **0 sessions**.

## Implemented capabilities

| Roadmap capability | Implementation | Status |
| --- | --- | --- |
| Option contract and chains | Strict canonical contract IDs, underlying identity, expiration, strike, right, multiplier, currency, canonical reference version, snapshot timestamp, quotes, and deterministic chain fingerprint | Implemented and tested |
| Volatility and Greeks | Fixed-point, bounded European Black–Scholes approximation for price, delta, gamma, vega, theta, rho, plus deterministic implied-volatility bisection from the exact bid/ask midpoint | Implemented and tested |
| Multi-leg / expiry workflow | Validated long/short whole-contract legs, exact entry premiums/multipliers, and deterministic expiry scenarios with per-leg and total P&L | Implemented and tested |
| Cross-environment reconciliation | Explicit-time exact comparison of BACKTEST, PAPER, and LIVE option books, including cash, positions, marks, realized P&L, and each book's separately declared strategy/data/config/replay/chain/model identity; the outer reconciliation configuration has an exact source-byte hash and every declared export is normalized/fingerprinted | Implemented and tested |
| Options CLI / reports | Immutable analysis dashboard and Markdown report, strict configuration ingress, and content hashes | Implemented and tested |
| Replay UI | Strict desktop parser and evidence-only display for option analytics, scenarios, provenance, and reconciliation differences | Implemented and typechecked |

## Model and product boundary

The v1 model intentionally supports **European exercise only**. It fails closed for missing/invalid economics and avoids passing an American-style option through an inapplicable pricing model. It also rejects impossible European no-arbitrage premiums, expired valuation inputs, platform-math calls, and use of the workstation clock.

The calculations use Follon’s signed eight-decimal fixed-point type with bounded deterministic implementations of logarithm, exponential, square root, normal density, normal CDF, and bisection. The model version is recorded as `follon-european-black-scholes-fixed-v1`; changing its approximation or rounding requires a new model version and regenerated evidence.

## Reproducibility and reconciliation

The outer reconciliation configuration hash is computed from its exact loaded bytes rather than trusted from a self-declared field. Each option book separately carries its full strategy/data/config/replay/chain/model identity as well as its own account ID, source-export ID, source-export hash, `as_of`, and currency; reconciliation compares those identities rather than injecting one shared value. The normalized source-export hash is recomputed from the complete declared book payload, including its run identity, before it can participate in reconciliation. This proves internal consistency of the declared export, not that a raw broker export or its signer has been independently verified. `reconcile_option_books_at` requires an explicit reconciliation instant and performs a read-only exact comparison; it never overwrites a PAPER or LIVE position to make a report clean. The report preserves environment-specific source, run-identity, and book fingerprints. A clean result means the compared economics and run identity agree—not that independent export hashes are identical. Differences remain explicit `IDENTITY_MISMATCH`, `CASH_MISMATCH`, or `POSITION_MISMATCH` records.

The included fixture contains a call spread and equal BACKTEST/PAPER/LIVE exports. It proves local deterministic reproduction, not an external broker result.

```powershell
cargo run -p follon-cli --bin follon-options -- validate-config tests/fixtures/config/options-v1.json
cargo run -p follon-cli --bin follon-options -- analyze tests/fixtures/config/options-v1.json var/options-dashboard.json
cargo run -p follon-cli --bin follon-options -- report tests/fixtures/config/options-v1.json var/options-report.md
```

Repeat an identical command in place to verify immutable publication is idempotent. Load `var/options-dashboard.json` in the desktop evidence shell to inspect the frozen chain and reconciliation result.

## External acceptance gate remains open

“Options reconcile and reproduce across backtest, paper, and live” becomes an operational claim only after an independently reviewed option-capable broker adapter exports normalized books for the same account/instant, the exact relevant chain/reference snapshot is retained, and all three books reconcile without unresolved difference. Before enabling real options orders, additionally review OCC/venue contract symbology, exercise/assignment, early exercise and dividend risk, margin, corporate actions, settlement, currency, commissions, tax, halt behavior, stale quotes, market-data licensing, and the broker’s multi-leg order semantics.

No options order endpoint, broker credential, exercise instruction, assignment workflow, or live trading permission is introduced by this repository phase.
