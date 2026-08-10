"""Emits one contract-invalid line for process-session poisoning tests."""

import json
import sys


for line in sys.stdin:
    request = json.loads(line)
    print(
        json.dumps(
            {
                "protocol_version": 1,
                "request_id": request["request_id"],
                "ok": True,
                "result": {"not": "an event list"},
                "error": None,
            },
            separators=(",", ":"),
        ),
        flush=True,
    )
