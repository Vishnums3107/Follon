"""Test fixture: a protocol-v1 strategy worker that reports its string-hash seed.

It announces the bundle identity given on its command line, then answers every
callback with an error frame whose code carries the hash of a fixed string.
Two runs report the same code only when Python seeds string hashing
identically in both processes (delivery state E7.3).
"""

import json
import sys

bundle_hash, strategy_id, strategy_version = sys.argv[1:4]
ready = {
    "type": "ready",
    "protocol_version": 1,
    "bundle_hash": bundle_hash,
    "strategy_id": strategy_id,
    "strategy_version": strategy_version,
}
print(json.dumps(ready), flush=True)
for _line in sys.stdin:
    code = f"hash.{hash('follon') & 0xFFFFFFFFFFFFFFFF:016x}"
    print(json.dumps({"type": "error", "protocol_version": 1, "code": code}), flush=True)
