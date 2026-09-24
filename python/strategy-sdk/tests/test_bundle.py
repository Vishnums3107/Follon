from hashlib import sha256
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from follon_strategy_sdk import strategy_bundle_hash, strategy_bundle_lock
from follon_strategy_sdk.bundle import (
    BUNDLE_FORMAT,
    _bundle_digest,
    _source_files,
    strategy_bundle_runtime,
)
from follon_strategy_sdk.bundle_lock import main as bundle_lock_main

SDK_SOURCE = Path(__file__).resolve().parents[1] / "src"

# `archive_hashes_to_the_python_sdk_bundle_hash` in
# core/control-plane/src/capsule.rs pins the same tree, runtime and digest, so
# the Rust capsule archive and this hash cannot drift apart silently.
VECTOR_RUNTIME = "cpython|3.12.10|linux"
VECTOR_HASH = "dd161695d4e3fa38bd1c790567c2431df6d0b5c6be447dfc9b2e43193aaea62f"


def write_vector_tree(root: Path) -> tuple[Path, Path]:
    strategy = root / "strategy"
    sdk = root / "sdk"
    (strategy / "pkg").mkdir(parents=True)
    sdk.mkdir()
    (strategy / "alpha.py").write_bytes(b"ALPHA = 1\n")
    # Code-point order puts this first; NTFS directory order puts it last.
    (strategy / "Zeta.py").write_bytes(b"ZETA = 26\n")
    (strategy / "pkg" / "beta.py").write_bytes(b"from alpha import ALPHA\n")
    (strategy / "notes.txt").write_bytes(b"not source\n")
    (sdk / "__init__.py").write_bytes(b'"""sdk"""\n')
    return strategy, sdk


def write_strategy(root: Path) -> Path:
    root.mkdir()
    strategy_file = root / "alpha_strategy.py"
    strategy_file.write_bytes(b"from follon_strategy_sdk import Strategy\n\nclass Alpha(Strategy):\n    pass\n")
    (root / "helpers").mkdir()
    (root / "helpers" / "levels.py").write_bytes(b"LEVEL = 100\n")
    return strategy_file


class BundleIdentityTest(unittest.TestCase):
    def test_bundle_digest_matches_the_rust_capsule_vector(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            strategy, sdk = write_vector_tree(Path(directory))
            namespaces = (("strategy", _source_files(strategy)), ("sdk", _source_files(sdk)))
            self.assertEqual(
                [path for _, files in namespaces for path, _ in files],
                ["Zeta.py", "alpha.py", "pkg/beta.py", "__init__.py"],
            )
            self.assertEqual(_bundle_digest(namespaces, VECTOR_RUNTIME), VECTOR_HASH)

    def test_lock_describes_exactly_the_bundle_it_hashes(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "bundle"
            strategy_file = write_strategy(root)
            contents = strategy_bundle_lock(root, strategy_file, "Alpha")
            lock = json.loads(contents)

            self.assertEqual(
                contents,
                json.dumps(lock, sort_keys=True, separators=(",", ":")).encode("ascii"),
            )
            self.assertEqual(lock["strategy_bundle_hash"], strategy_bundle_hash(root))
            self.assertEqual(lock["runtime"], strategy_bundle_runtime())
            self.assertEqual(lock["bundle_format"], BUNDLE_FORMAT)
            self.assertEqual(
                lock["entry_point"], {"class_name": "Alpha", "strategy_file": "alpha_strategy.py"}
            )
            strategy, sdk = lock["namespaces"]
            self.assertEqual(
                (strategy["name"], [entry["path"] for entry in strategy["files"]]),
                ("strategy", ["alpha_strategy.py", "helpers/levels.py"]),
            )
            self.assertEqual(sdk["name"], "sdk")
            sdk_root = SDK_SOURCE / "follon_strategy_sdk"
            self.assertEqual(
                [entry["path"] for entry in sdk["files"]],
                sorted(path.name for path in sdk_root.glob("*.py")),
            )
            for namespace_root, namespace in ((root, strategy), (sdk_root, sdk)):
                for entry in namespace["files"]:
                    source = (namespace_root / entry["path"]).read_bytes()
                    self.assertEqual(entry["bytes"], len(source))
                    self.assertEqual(entry["sha256"], sha256(source).hexdigest())

            (root / "helpers" / "levels.py").write_bytes(b"LEVEL = 101\n")
            changed = json.loads(strategy_bundle_lock(root, strategy_file, "Alpha"))
            self.assertNotEqual(changed["strategy_bundle_hash"], lock["strategy_bundle_hash"])

    def test_lock_refuses_an_entry_point_outside_the_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "bundle"
            write_strategy(root)
            outside = Path(directory) / "outside.py"
            outside.write_bytes(b"pass\n")
            with self.assertRaises(ValueError):
                strategy_bundle_lock(root, outside, "Alpha")
            (root / "notes.txt").write_bytes(b"not source\n")
            with self.assertRaises(ValueError):
                strategy_bundle_lock(root, root / "notes.txt", "Alpha")
            with self.assertRaises(ValueError):
                strategy_bundle_lock(root, root / "alpha_strategy.py", "not-an-identifier")

    def test_lock_command_writes_a_new_file_only(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "bundle"
            strategy_file = write_strategy(root)
            output = Path(directory) / "dependency.lock"
            arguments = [
                "--bundle-root", str(root),
                "--strategy-file", str(strategy_file),
                "--class-name", "Alpha",
                "--output", str(output),
            ]
            self.assertEqual(bundle_lock_main(arguments), 0)
            self.assertEqual(output.read_bytes(), strategy_bundle_lock(root, strategy_file, "Alpha"))
            output.write_bytes(b"operator edit")
            self.assertEqual(bundle_lock_main(arguments), 2)
            self.assertEqual(output.read_bytes(), b"operator edit")

    def test_lock_module_runs_without_a_double_import(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / "bundle"
            strategy_file = write_strategy(root)
            completed = subprocess.run(
                [
                    sys.executable, "-W", "error", "-m", "follon_strategy_sdk.bundle_lock",
                    "--bundle-root", str(root),
                    "--strategy-file", str(strategy_file),
                    "--class-name", "Alpha",
                    "--output", str(Path(directory) / "dependency.lock"),
                ],
                capture_output=True,
                text=True,
                env={"PYTHONPATH": str(SDK_SOURCE), "SYSTEMROOT": os.environ.get("SYSTEMROOT", "")},
                check=False,
            )
            self.assertEqual(completed.returncode, 0, completed.stderr)
            self.assertEqual(completed.stdout.strip(), strategy_bundle_hash(root))


if __name__ == "__main__":
    unittest.main()
