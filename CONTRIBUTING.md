# Contributing

This repository prioritizes trading correctness and traceability over feature throughput.

Before opening a change:

1. Identify the owning document and update it if the contract or behaviour changes.
2. Record an ADR for a material architectural decision.
3. Add tests for all changed invariants, error paths, duplicate events, and recovery behaviour.
4. Keep broker types and credentials outside the domain and strategy SDK.
5. Use canonical IDs, UTC timestamps, and decimal/fixed-point accounting values.
6. Keep text files LF. `.gitattributes` checks every text file out with LF on every platform,
   because configuration, bundle and source hashes cover exact bytes. A checkout made before that
   rule existed keeps its old line endings until each file is deleted and checked out again;
   `tests/security/test_checkout_line_endings.py` names any stale file.

No change may allow strategy code to bypass pre-trade risk or weaken auditability. Do not use production credentials or market data in local fixtures or automated tests.
