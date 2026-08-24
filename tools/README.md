# Repository Tools

Tools in this directory are deterministic repository automation, not runtime
trading services.

| Tool | Purpose |
| --- | --- |
| `generate_sbom.py` | Generates an immutable CycloneDX 1.6 SBOM from `Cargo.lock`, `apps/desktop/package-lock.json`, and Python package metadata. |

Example:

```bash
python tools/generate_sbom.py --source-revision local.check --output build/follon-sbom.cdx.json
```
