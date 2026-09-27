"""The checkout must hold the repository's exact bytes on every platform.

Follon hashes checked-in inputs byte for byte: a backtest configuration's
content hash, a Python strategy bundle's files, and the built-in strategy's
own source, which `follon-backtest` embeds with `include_str!`. Converting
line endings on checkout therefore changed published evidence. The same
commit gave a Windows checkout a different configuration hash from a Linux
one (audit item 75). `.gitattributes` pins every text file to LF, and these
tests hold the checkout to it.
"""

from __future__ import annotations

import contextlib
import importlib.util
import io
import subprocess
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
ADVANCED_FIXTURES = "tests/fixtures/config/advanced"

# Checked-in files whose exact bytes reach a published hash.
HASHED_INPUTS = (
    "tests/fixtures/config/backtest-v1.json",
    "tests/fixtures/historical-bars/spy-one-minute.csv",
    "core/control-plane/src/lib.rs",
    "python/examples/worker_buy_once_strategy.py",
    "python/strategy-sdk/src/follon_strategy_sdk/bundle.py",
)


def git(*arguments: str) -> bytes:
    return subprocess.run(
        ["git", *arguments], cwd=REPOSITORY_ROOT, check=True, capture_output=True
    ).stdout


class CheckoutLineEndingTests(unittest.TestCase):
    def test_text_files_are_pinned_to_lf_and_binaries_are_not_converted(self) -> None:
        for path in HASHED_INPUTS:
            attributes = git("check-attr", "text", "eol", "--", path).decode()
            self.assertIn(f"{path}: text: auto", attributes)
            self.assertIn(f"{path}: eol: lf", attributes)
        icon = "apps/desktop/src-tauri/icons/32x32.png"
        self.assertIn(f"{icon}: text: unset", git("check-attr", "text", "--", icon).decode())

    def test_no_tracked_text_file_is_checked_out_with_crlf(self) -> None:
        converted = []
        for line in git("ls-files", "--eol").decode().splitlines():
            # The attribute column can hold a space ("attr/text=auto eol=lf"),
            # and the path always follows a tab.
            states, path = line.split("\t", 1)
            index_state, worktree_state = states.split()[:2]
            if index_state == "i/lf" and worktree_state in ("w/crlf", "w/mixed"):
                converted.append(path)
        # Git treats an unmodified file as up to date even after the rule
        # changes, so a stale file must be deleted and checked out again.
        self.assertEqual(
            converted, [], "stale checkout: delete each file, then `git checkout -- <path>`"
        )

    def test_hashed_inputs_hold_the_repository_bytes(self) -> None:
        for path in HASHED_INPUTS:
            with self.subTest(path=path):
                # `git diff` compares content after the LF rule, so a
                # line-ending conversion alone is not a change it skips.
                edited = subprocess.run(
                    ["git", "diff", "--quiet", "--", path], cwd=REPOSITORY_ROOT
                ).returncode
                if edited:
                    self.skipTest(f"{path} has uncommitted edits")
                # `cat-file` prints the index blob itself, with no conversion.
                blob = git("cat-file", "blob", f":{path}")
                self.assertEqual((REPOSITORY_ROOT / path).read_bytes(), blob)

    @unittest.skipUnless(
        importlib.util.find_spec("jsonschema"), "the fixture generator needs jsonschema"
    )
    def test_the_advanced_fixture_generator_reproduces_the_checked_in_bytes(self) -> None:
        # The pipeline regenerates these checked-in fixtures on every run. In
        # text mode, Python wrote them with CRLF on Windows.
        spec = importlib.util.spec_from_file_location(
            "build_advanced_evidence_fixtures",
            REPOSITORY_ROOT / "tools" / "build_advanced_evidence_fixtures.py",
        )
        generator = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(generator)
        with tempfile.TemporaryDirectory() as directory:
            generator.TARGET_DIR = Path(directory)
            with contextlib.redirect_stdout(io.StringIO()):
                generator.validate_and_write()
            written = sorted(Path(directory).iterdir())
            checked_in = git("ls-files", "--", ADVANCED_FIXTURES).decode().split()
            self.assertEqual(
                [f"{ADVANCED_FIXTURES}/{path.name}" for path in written], sorted(checked_in)
            )
            for path in written:
                with self.subTest(fixture=path.name):
                    blob = git("cat-file", "blob", f":{ADVANCED_FIXTURES}/{path.name}")
                    self.assertEqual(path.read_bytes(), blob)


if __name__ == "__main__":
    unittest.main()
