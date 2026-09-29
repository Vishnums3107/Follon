"""The status tool reports what it did not run, and says why (delivery state E7.7).

The PostgreSQL suite runs only against a disposable database. On a machine without one
the status block must show it skipped and name the variable that would enable it, and
it must never show a suite it could not run as passing or, worse, leave it out.
"""

from __future__ import annotations

import contextlib
import io
import os
import sys
import unittest
from pathlib import Path
from unittest import mock

from tools import session_status
from tools.session_status import Suite, SuiteOutcome, render_block

VARIABLE = "FOLLON_STATUS_TEST_URL"


def run_suite(suite: Suite, *, fast: bool) -> SuiteOutcome:
    """The tool's own `run_suite`, without the progress lines it prints."""
    with contextlib.redirect_stdout(io.StringIO()):
        return session_status.run_suite(suite, fast=fast)


def probe(**overrides: object) -> Suite:
    return Suite(
        key="probe",
        title="Probe",
        command=[sys.executable, "-c", "print('ran')"],
        working_directory=Path.cwd(),
        **overrides,  # type: ignore[arg-type]
    )


class EnvironmentGatedSuiteTests(unittest.TestCase):
    def test_a_suite_whose_variable_is_unset_is_skipped_and_names_it(self) -> None:
        with mock.patch.dict(os.environ):
            os.environ.pop(VARIABLE, None)
            outcome = run_suite(probe(requires_environment=VARIABLE), fast=False)
        self.assertEqual(outcome.status, "SKIPPED")
        self.assertIsNone(outcome.exit_code)
        self.assertIn(VARIABLE, outcome.detail)

    def test_an_empty_variable_is_as_good_as_unset(self) -> None:
        with mock.patch.dict(os.environ, {VARIABLE: ""}):
            outcome = run_suite(probe(requires_environment=VARIABLE), fast=False)
        self.assertEqual(outcome.status, "SKIPPED")

    def test_a_suite_whose_variable_is_set_runs_and_reports_its_own_exit_code(self) -> None:
        with mock.patch.dict(os.environ, {VARIABLE: "postgresql://disposable"}):
            outcome = run_suite(probe(requires_environment=VARIABLE), fast=False)
        self.assertEqual((outcome.status, outcome.exit_code), ("PASS", 0))

    def test_a_suite_that_fails_when_it_runs_is_a_failure_not_a_skip(self) -> None:
        failing = probe(requires_environment=VARIABLE)
        failing.command = [sys.executable, "-c", "raise SystemExit(3)"]
        with mock.patch.dict(os.environ, {VARIABLE: "postgresql://disposable"}):
            outcome = run_suite(failing, fast=False)
        self.assertEqual((outcome.status, outcome.exit_code), ("FAIL", 3))

    def test_the_database_suite_is_declared_and_needs_the_database_url(self) -> None:
        declared = {suite.key: suite for suite in session_status.suites()}
        suite = declared["postgres_integration"]
        self.assertEqual(suite.requires_environment, "FOLLON_TEST_DATABASE_URL")
        self.assertEqual(
            suite.command, ["cargo", "test", "-p", "follon-postgres", "--", "--ignored"]
        )
        # Every other suite runs wherever its tool is installed.
        others = [s for key, s in declared.items() if key != "postgres_integration"]
        self.assertTrue(all(s.requires_environment is None for s in others))

    def test_the_block_shows_a_skipped_suite_and_counts_it(self) -> None:
        skipped = SuiteOutcome(
            key="postgres_integration",
            title="PostgreSQL integration",
            command="cargo test",
            status="SKIPPED",
            exit_code=None,
            detail="`FOLLON_TEST_DATABASE_URL` is not set, so there is no disposable database to run against",
        )
        passed = SuiteOutcome(
            key="python", title="Python", command="pytest", status="PASS", exit_code=0,
            passed=3, failed=0, ignored=0,
        )
        facts = {
            "branch": "main", "head": "abc1234", "head_subject": "subject",
            "head_committed_at": "2026-09-29T00:00:00+00:00", "uncommitted_paths": "0",
        }
        block = render_block([skipped, passed], facts, "2026-09-29T00:00:00Z")
        self.assertIn("| PostgreSQL integration -- `FOLLON_TEST_DATABASE_URL` is not set", block)
        self.assertIn("**SKIPPED**", block)
        self.assertIn("**All 1 executed suite(s) green; 1 skipped.**", block)


if __name__ == "__main__":
    unittest.main()
