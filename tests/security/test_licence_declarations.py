"""Every first-party licence declaration names the root LICENSE's licence.

The root LICENSE was MIT while every Cargo manifest and the strategy SDK
declared Apache-2.0. The operator decided on MIT on 2026-09-29 (delivery
state E7.11); this keeps the declarations from drifting apart again.
"""

from __future__ import annotations

import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LICENCE = "MIT"


def manifest(path: str) -> dict:
    return tomllib.loads((ROOT / path).read_text(encoding="utf-8"))


class LicenceDeclarationTests(unittest.TestCase):
    def test_the_root_licence_file_is_mit(self) -> None:
        self.assertEqual(
            (ROOT / "LICENSE").read_text(encoding="utf-8").splitlines()[0],
            "MIT License",
        )

    def test_every_cargo_manifest_declares_it(self) -> None:
        workspace = manifest("Cargo.toml")
        self.assertEqual(workspace["workspace"]["package"]["license"], LICENCE)
        for member in workspace["workspace"]["members"]:
            with self.subTest(member=member):
                package = manifest(f"{member}/Cargo.toml")["package"]
                self.assertIn(package.get("license"), ({"workspace": True}, LICENCE))
        # The desktop host is a separate Cargo workspace.
        self.assertEqual(manifest("apps/desktop/src-tauri/Cargo.toml")["package"]["license"], LICENCE)

    def test_every_python_package_declares_it(self) -> None:
        for path in ("python/strategy-sdk/pyproject.toml", "python/storage-adapter/pyproject.toml"):
            with self.subTest(path=path):
                # An SPDX expression, as PEP 639 and setuptools 77+ expect.
                self.assertEqual(manifest(path)["project"].get("license"), LICENCE)


if __name__ == "__main__":
    unittest.main()
