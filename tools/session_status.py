#!/usr/bin/env python3
"""Machine-verified delivery status for Follon.

Runs every automated verification suite this repository owns, captures each
suite's *real* process exit code (never a pipe's), and rewrites the generated
block inside ``docs/06-delivery/16-delivery-state.md``.

This exists because this repository has twice recorded fabricated status
(conformance-audit items 23-24 and item 45). A status line a human typed is a
claim; a status line this tool wrote is a measurement. Nothing in the generated
block is hand-editable -- the markers are checked and the block is replaced
wholesale on every run.

Usage::

    python tools/session_status.py            # run every suite, rewrite the block
    python tools/session_status.py --fast     # skip the slow suites
    python tools/session_status.py --check    # fail if the block is stale

The PostgreSQL integration suite runs only when ``FOLLON_TEST_DATABASE_URL`` names
a disposable database, and the block reports it skipped otherwise. Retained evidence
is append-only there, so such a database is never cleaned by deleting rows: drop and
recreate it when you want it empty.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, field
from datetime import datetime, timezone
from pathlib import Path

REPOSITORY_ROOT = Path(__file__).resolve().parent.parent
STATE_DOCUMENT = REPOSITORY_ROOT / "docs" / "06-delivery" / "16-delivery-state.md"
STATUS_JSON = REPOSITORY_ROOT / "var" / "follon-delivery-status.json"

BEGIN_MARKER = "<!-- BEGIN GENERATED STATUS -- tools/session_status.py -->"
END_MARKER = "<!-- END GENERATED STATUS -- tools/session_status.py -->"

# `cargo test` prints one `test result:` line per binary; the passed count is
# the second capture. Summed per line because cargo emits no workspace total.
TEST_RESULT_PATTERN = re.compile(
    r"^test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored", re.MULTILINE
)
PYTEST_PATTERN = re.compile(r"^(\d+) passed", re.MULTILINE)


@dataclass
class Suite:
    """One automated verification suite and how to run it."""

    key: str
    title: str
    command: list[str]
    working_directory: Path
    slow: bool = False
    # A suite whose binary is absent reports SKIPPED, not FAILED, so a machine
    # without Node still produces an honest status line instead of a false red.
    requires: str | None = None
    # Likewise for a suite that needs an environment variable, such as the
    # database tests, which run only against a disposable PostgreSQL. It is
    # reported as skipped, never omitted, so the block says what was not run.
    requires_environment: str | None = None


@dataclass
class SuiteOutcome:
    """The measured result of running one suite."""

    key: str
    title: str
    command: str
    status: str
    exit_code: int | None
    passed: int | None = None
    failed: int | None = None
    ignored: int | None = None
    detail: str = ""
    log_tail: list[str] = field(default_factory=list)


def suites() -> list[Suite]:
    """Declares every suite that gates this repository."""
    desktop = REPOSITORY_ROOT / "apps" / "desktop"
    return [
        Suite(
            key="rust_workspace",
            title="Rust workspace (`cargo test --workspace --all-targets`)",
            command=["cargo", "test", "--workspace", "--all-targets"],
            working_directory=REPOSITORY_ROOT,
            slow=True,
            requires="cargo",
        ),
        Suite(
            key="rust_fmt",
            title="Rust formatting (`cargo fmt --all -- --check`)",
            command=["cargo", "fmt", "--all", "--", "--check"],
            working_directory=REPOSITORY_ROOT,
            requires="cargo",
        ),
        Suite(
            key="rust_clippy",
            title="Rust lints (`cargo clippy --workspace --all-targets -D warnings`)",
            command=[
                "cargo",
                "clippy",
                "--workspace",
                "--all-targets",
                "--",
                "-D",
                "warnings",
            ],
            working_directory=REPOSITORY_ROOT,
            slow=True,
            requires="cargo",
        ),
        Suite(
            key="postgres_integration",
            title="PostgreSQL integration (`cargo test -p follon-postgres -- --ignored`)",
            command=["cargo", "test", "-p", "follon-postgres", "--", "--ignored"],
            working_directory=REPOSITORY_ROOT,
            requires="cargo",
            requires_environment="FOLLON_TEST_DATABASE_URL",
        ),
        Suite(
            key="tauri_workspace",
            title="Tauri host workspace (`cargo test` in `apps/desktop/src-tauri`)",
            command=["cargo", "test"],
            working_directory=desktop / "src-tauri",
            requires="cargo",
        ),
        Suite(
            key="python",
            title="Python suite (`pytest`)",
            command=[sys.executable, "-m", "pytest", "-q"],
            working_directory=REPOSITORY_ROOT,
        ),
        Suite(
            key="desktop_evidence",
            title="Desktop evidence regressions (`npm run test:evidence`)",
            command=["npm", "run", "test:evidence"],
            working_directory=desktop,
            slow=True,
            requires="npm",
        ),
        Suite(
            key="server_contract",
            title="Desktop server contract (`apps/desktop/test/server_contract.py`)",
            command=[sys.executable, str(desktop / "test" / "server_contract.py")],
            working_directory=REPOSITORY_ROOT,
        ),
    ]


def resolve_command(command: list[str]) -> list[str]:
    """Makes a command launchable without a shell, including on Windows.

    `npm` resolves to `npm.cmd` on Windows, which `CreateProcess` cannot launch
    directly. Routing those through `cmd /c` keeps `shell=False` everywhere
    else, so no argument is ever re-parsed by a shell.
    """
    resolved = shutil.which(command[0])
    if resolved is None:
        return command
    if sys.platform == "win32" and resolved.lower().endswith((".cmd", ".bat")):
        return ["cmd", "/c", resolved, *command[1:]]
    return [resolved, *command[1:]]


def run_suite(suite: Suite, *, fast: bool) -> SuiteOutcome:
    """Runs one suite and records its genuine process exit code."""
    printable = " ".join(suite.command)
    if suite.requires and shutil.which(suite.requires) is None:
        return SuiteOutcome(
            key=suite.key,
            title=suite.title,
            command=printable,
            status="SKIPPED",
            exit_code=None,
            detail=f"`{suite.requires}` is not on PATH on this machine",
        )
    if suite.requires_environment and not os.environ.get(suite.requires_environment):
        return SuiteOutcome(
            key=suite.key,
            title=suite.title,
            command=printable,
            status="SKIPPED",
            exit_code=None,
            detail=f"`{suite.requires_environment}` is not set, so there is no disposable database to run against",
        )
    if fast and suite.slow:
        return SuiteOutcome(
            key=suite.key,
            title=suite.title,
            command=printable,
            status="SKIPPED",
            exit_code=None,
            detail="skipped by --fast",
        )
    print(f"[+] {suite.title}")
    completed = subprocess.run(
        resolve_command(suite.command),
        cwd=str(suite.working_directory),
        capture_output=True,
        text=True,
        shell=False,
        encoding="utf-8",
        errors="replace",
    )
    combined = f"{completed.stdout}\n{completed.stderr}"
    passed = failed = ignored = None
    matches = TEST_RESULT_PATTERN.findall(combined)
    if matches:
        passed = sum(int(match[1]) for match in matches)
        failed = sum(int(match[2]) for match in matches)
        ignored = sum(int(match[3]) for match in matches)
    elif suite.key == "python":
        pytest_match = PYTEST_PATTERN.search(combined)
        if pytest_match:
            passed = int(pytest_match.group(1))
            failed = 0
            ignored = 0
    status = "PASS" if completed.returncode == 0 else "FAIL"
    tail: list[str] = []
    if status == "FAIL":
        tail = [line for line in combined.strip().splitlines() if line.strip()][-25:]
    print(f"    exit={completed.returncode} -> {status}")
    return SuiteOutcome(
        key=suite.key,
        title=suite.title,
        command=printable,
        status=status,
        exit_code=completed.returncode,
        passed=passed,
        failed=failed,
        ignored=ignored,
        log_tail=tail,
    )


def git_facts() -> dict[str, str]:
    """Reads the repository facts a resuming session needs first."""

    def git(*args: str) -> str:
        result = subprocess.run(
            ["git", *args],
            cwd=str(REPOSITORY_ROOT),
            capture_output=True,
            text=True,
            encoding="utf-8",
            errors="replace",
        )
        return result.stdout.strip() if result.returncode == 0 else "unavailable"

    dirty = git("status", "--porcelain")
    return {
        "branch": git("rev-parse", "--abbrev-ref", "HEAD"),
        "head": git("rev-parse", "--short", "HEAD"),
        "head_subject": git("log", "-1", "--pretty=%s"),
        "head_committed_at": git("log", "-1", "--pretty=%cI"),
        "uncommitted_paths": str(
            len([line for line in dirty.splitlines() if line.strip()])
        ),
    }


def render_block(
    outcomes: list[SuiteOutcome], facts: dict[str, str], generated_at: str
) -> str:
    """Renders the generated Markdown block. Every number here was measured."""
    lines = [
        BEGIN_MARKER,
        "",
        "> Generated by `python tools/session_status.py`. Do not hand-edit: the next",
        "> run replaces this block wholesale. Every exit code below is the suite",
        "> process's own return code, captured directly rather than through a pipe.",
        "",
        f"**Measured at:** {generated_at}  ",
        f"**Branch:** `{facts['branch']}`  ",
        f"**HEAD:** `{facts['head']}` -- {facts['head_subject']} ({facts['head_committed_at']})  ",
        f"**Uncommitted paths:** {facts['uncommitted_paths']}",
        "",
        "| Suite | Status | Exit | Passed | Failed | Ignored |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for outcome in outcomes:
        exit_cell = "--" if outcome.exit_code is None else str(outcome.exit_code)
        detail = f" -- {outcome.detail}" if outcome.detail else ""
        lines.append(
            f"| {outcome.title}{detail} | **{outcome.status}** | {exit_cell} | "
            f"{'--' if outcome.passed is None else outcome.passed} | "
            f"{'--' if outcome.failed is None else outcome.failed} | "
            f"{'--' if outcome.ignored is None else outcome.ignored} |"
        )
    failures = [outcome for outcome in outcomes if outcome.status == "FAIL"]
    lines.append("")
    if failures:
        lines.append(f"**{len(failures)} suite(s) failing.** Tail of each failure:")
        lines.append("")
        for outcome in failures:
            lines.append(f"- `{outcome.command}` (exit {outcome.exit_code}):")
            lines.append("")
            lines.append("  ```")
            for line in outcome.log_tail:
                lines.append(f"  {line}")
            lines.append("  ```")
    else:
        measured = [outcome for outcome in outcomes if outcome.status == "PASS"]
        skipped = [outcome for outcome in outcomes if outcome.status == "SKIPPED"]
        lines.append(
            f"**All {len(measured)} executed suite(s) green"
            + (f"; {len(skipped)} skipped." if skipped else ".")
            + "**"
        )
    lines.extend(["", END_MARKER])
    return "\n".join(lines)


def splice(document: str, block: str) -> str:
    """Replaces the generated block, refusing a document missing its markers."""
    start = document.find(BEGIN_MARKER)
    end = document.find(END_MARKER)
    if start == -1 or end == -1 or end < start:
        raise SystemExit(
            f"{STATE_DOCUMENT} is missing its generated-status markers; refusing to guess."
        )
    return document[:start] + block + document[end + len(END_MARKER) :]


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fast", action="store_true", help="skip the slow suites")
    parser.add_argument(
        "--check",
        action="store_true",
        help="exit non-zero if the state document's block is stale or a suite failed",
    )
    arguments = parser.parse_args()

    outcomes = [run_suite(suite, fast=arguments.fast) for suite in suites()]
    facts = git_facts()
    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    block = render_block(outcomes, facts, generated_at)

    document = STATE_DOCUMENT.read_text(encoding="utf-8")
    updated = splice(document, block)
    if arguments.check:
        # A stale block is reported, never silently rewritten, so CI can tell
        # "nobody re-ran it" apart from "a suite regressed".
        stale = updated != document
        broken = any(outcome.status == "FAIL" for outcome in outcomes)
        if stale:
            print("[-] docs/06-delivery/16-delivery-state.md status block is stale")
        if broken:
            print("[-] at least one suite failed")
        return 1 if (stale or broken) else 0

    STATE_DOCUMENT.write_text(updated, encoding="utf-8", newline="\n")
    STATUS_JSON.parent.mkdir(parents=True, exist_ok=True)
    STATUS_JSON.write_text(
        json.dumps(
            {
                "generated_at": generated_at,
                "git": facts,
                "suites": [
                    {
                        "key": outcome.key,
                        "command": outcome.command,
                        "status": outcome.status,
                        "exit_code": outcome.exit_code,
                        "passed": outcome.passed,
                        "failed": outcome.failed,
                        "ignored": outcome.ignored,
                        "detail": outcome.detail,
                    }
                    for outcome in outcomes
                ],
            },
            indent=2,
            sort_keys=True,
        )
        + "\n",
        encoding="utf-8",
        newline="\n",
    )
    print(f"\n[+] wrote {STATE_DOCUMENT.relative_to(REPOSITORY_ROOT)}")
    print(f"[+] wrote {STATUS_JSON.relative_to(REPOSITORY_ROOT)}")
    return 1 if any(outcome.status == "FAIL" for outcome in outcomes) else 0


if __name__ == "__main__":
    raise SystemExit(main())
