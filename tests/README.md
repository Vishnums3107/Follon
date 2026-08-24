# Test suites

Organize tests by the boundary they protect. Each new trading invariant must be
automatically tested, especially duplicate events, disconnects, restart
recovery, and risk-boundary conditions.

| Path | Owns |
| --- | --- |
| `apps/cli/tests` | CLI-driven workflow and repeatability tests |
| `apps/desktop/test` | Evidence-dashboard contract tests |
| `python/*/tests` | Python package protocol and adapter tests |
| `tests/fixtures` | Shared deterministic inputs |
| `tests/security` | Security, contract, and supply-chain checks |
