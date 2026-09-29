"""Test fixture: a protocol-v1 strategy worker that misbehaves on demand.

    misbehaving-worker.py BUNDLE_HASH STRATEGY_ID STRATEGY_VERSION MODE [SIZE]

It announces the identity it is given, then behaves as MODE on its first
request (delivery state E7.2). Nothing here is a strategy; each mode is one way
an untrusted worker can hurt its parent:

    well       answers with a valid error frame, again for every request
    silent     reads the request and never answers
    deaf       never reads its input, so a large request blocks the writer
    endless    answers with 8 MiB and no newline, then exits
    oversized  answers with one complete 1 MiB frame
    truncated  answers with half a frame and exits
    garbage    answers with a short line that is not JSON
    exact      answers with a valid error frame of exactly SIZE bytes, newline included
"""

import json
import sys
import time

# A stalled worker gives up after this long, so one the parent failed to end cannot
# outlive a test run holding its pipes open.
STALL_SECONDS = 30

bundle_hash, strategy_id, strategy_version, mode = sys.argv[1:5]
ready = {
    "type": "ready",
    "protocol_version": 1,
    "bundle_hash": bundle_hash,
    "strategy_id": strategy_id,
    "strategy_version": strategy_version,
}
print(json.dumps(ready), flush=True)


def error_frame(code, padding=""):
    frame = {"type": "error", "protocol_version": 1, "code": code}
    if padding:
        frame["message"] = padding
    return json.dumps(frame, separators=(",", ":"))


def write_raw(text):
    sys.stdout.buffer.write(text.encode("utf-8"))
    sys.stdout.buffer.flush()


if mode == "deaf":
    time.sleep(STALL_SECONDS)
elif mode == "well":
    for _line in sys.stdin:
        write_raw(error_frame("fixture.ok") + "\n")
else:
    sys.stdin.readline()
    if mode == "silent":
        time.sleep(STALL_SECONDS)
    elif mode == "endless":
        chunk = "x" * (1024 * 1024)
        for _ in range(8):
            write_raw(chunk)
    elif mode == "oversized":
        write_raw("x" * (1024 * 1024) + "\n")
    elif mode == "truncated":
        write_raw('{"type":"strategy_output"')
    elif mode == "garbage":
        write_raw("this is not json\n")
    elif mode == "exact":
        size = int(sys.argv[5])
        base = len(error_frame("fixture.exact")) + 1
        padded = error_frame("fixture.exact", "p" * (size - base - len(',"message":""')))
        assert len(padded) + 1 == size, (len(padded) + 1, size)
        write_raw(padded + "\n")
    else:
        raise SystemExit(f"unknown mode {mode}")
