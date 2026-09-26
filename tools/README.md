# Repository Tools

Tools in this directory are deterministic repository automation, not runtime
trading services.

| Tool | Purpose |
| --- | --- |
| `generate_sbom.py` | Generates an immutable CycloneDX 1.6 SBOM from `Cargo.lock`, `apps/desktop/package-lock.json`, and Python package metadata. |
| `dast_scan.py` | Starts the real dashboard (production mode) and `follon-trading-api` (PAPER route plus operator authentication) on loopback. It probes both over the network for authentication bypass, rate limiting and lockout, method tampering, path traversal, headers, version disclosure, CORS, malformed or oversized input, session misuse, and unsafe startup configurations. It writes `dast-report.json` and `dast-report.md` and exits non-zero on any failed probe. It is a repository-authored scan, not an independent DAST run or a penetration test. |

Example:

```bash
python tools/generate_sbom.py --source-revision local.check --output build/follon-sbom.cdx.json
```

```bash
python tools/dast_scan.py --output-dir var/dast
```
