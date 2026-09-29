from __future__ import annotations

import importlib.util
import json
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path
from typing import Any


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
GENERATOR = REPOSITORY_ROOT / "tools" / "generate_sbom.py"
LICENCE = "MIT"


def load_generator() -> Any:
    specification = importlib.util.spec_from_file_location("follon_generate_sbom", GENERATOR)
    module = importlib.util.module_from_spec(specification)
    specification.loader.exec_module(module)
    return module


def property_of(component: dict[str, Any], name: str) -> str | None:
    return next(
        (item["value"] for item in component["properties"] if item["name"] == name), None
    )


def manifest(path: str) -> dict[str, Any]:
    return tomllib.loads((REPOSITORY_ROOT / path).read_text(encoding="utf-8"))


class SoftwareBillOfMaterialsTests(unittest.TestCase):
    def test_generator_is_deterministic_complete_and_immutable(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "follon-sbom.json"
            command = [
                sys.executable,
                str(GENERATOR),
                "--repository-root",
                str(REPOSITORY_ROOT),
                "--source-revision",
                "revision.test.001",
                "--output",
                str(output),
            ]
            subprocess.run(command, check=True, capture_output=True, text=True)
            first = output.read_bytes()
            subprocess.run(command, check=True, capture_output=True, text=True)
            self.assertEqual(output.read_bytes(), first)

            document = json.loads(first)
            self.assertEqual(document["bomFormat"], "CycloneDX")
            self.assertEqual(document["specVersion"], "1.6")
            self.assertEqual(document["version"], 1)
            self.assertEqual(document["metadata"]["component"]["version"], "revision.test.001")
            self.assertEqual(
                {
                    next(
                        property_["value"]
                        for property_ in component["properties"]
                        if property_["name"] == "follon:ecosystem"
                    )
                    for component in document["components"]
                },
                {"cargo", "npm", "python"},
            )
            self.assertEqual(
                document["components"],
                sorted(document["components"], key=lambda item: item["bom-ref"]),
            )
            input_hashes = document["metadata"]["properties"]
            self.assertTrue(input_hashes)
            self.assertTrue(all(len(item["value"]) == 64 for item in input_hashes))

            conflicting = [*command]
            conflicting[conflicting.index("revision.test.001")] = "revision.test.002"
            result = subprocess.run(conflicting, capture_output=True, text=True)
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("refusing to overwrite", result.stderr)


class FirstPartyBillTests(unittest.TestCase):
    """The desktop's Cargo workspace and every first-party licence are in the bill (E7.8)."""

    @classmethod
    def setUpClass(cls) -> None:
        cls.generator = load_generator()
        cls.sbom = cls.generator.build_sbom(REPOSITORY_ROOT, "revision.test.001")
        cls.components = cls.sbom["components"]

    def cargo(self, name: str) -> list[dict[str, Any]]:
        return [
            component
            for component in self.components
            if component["name"] == name and property_of(component, "follon:ecosystem") == "cargo"
        ]

    def test_the_desktop_cargo_workspace_is_part_of_the_bill(self) -> None:
        # Tauri is a dependency of the desktop host only, so the root lockfile
        # never names it.
        tauri = self.cargo("tauri")
        self.assertTrue(tauri)
        for component in tauri:
            self.assertEqual(
                property_of(component, "follon:declared-in"),
                "apps/desktop/src-tauri/Cargo.lock",
            )
        self.assertIn(
            "follon:input-sha256:apps/desktop/src-tauri/Cargo.lock",
            {item["name"] for item in self.sbom["metadata"]["properties"]},
        )

    def test_a_crate_in_both_workspaces_names_both_lockfiles(self) -> None:
        (domain,) = self.cargo("follon-domain")
        self.assertEqual(
            property_of(domain, "follon:declared-in"),
            "Cargo.lock,apps/desktop/src-tauri/Cargo.lock",
        )

    def test_every_first_party_component_records_its_licence(self) -> None:
        workspace = manifest("Cargo.toml")["workspace"]
        expected_crates = {
            manifest(f"{member}/Cargo.toml")["package"]["name"]
            for member in workspace["members"]
        } | {manifest("apps/desktop/src-tauri/Cargo.toml")["package"]["name"]}
        first_party = [
            component
            for component in self.components
            if property_of(component, "follon:first-party") == "true"
        ]
        by_ecosystem: dict[str, set[str]] = {}
        for component in first_party:
            by_ecosystem.setdefault(
                property_of(component, "follon:ecosystem"), set()
            ).add(component["name"])
            self.assertEqual(property_of(component, "follon:license"), LICENCE)
            self.assertEqual(component["licenses"], [{"license": {"id": LICENCE}}])
        self.assertEqual(by_ecosystem["cargo"], expected_crates)
        self.assertEqual(
            by_ecosystem["python"], {"follon-strategy-sdk", "follon-storage-adapter"}
        )
        self.assertEqual(by_ecosystem["npm"], {"@follon/desktop"})

    def test_the_bill_names_the_repositorys_own_licence(self) -> None:
        self.assertEqual(
            self.sbom["metadata"]["component"]["licenses"],
            [{"license": {"id": LICENCE}}],
        )

    def test_nothing_from_a_registry_claims_to_be_first_party(self) -> None:
        for component in self.components:
            if property_of(component, "follon:source"):
                self.assertIsNone(property_of(component, "follon:first-party"), component["name"])

    def test_a_licence_expression_is_not_written_as_an_identifier(self) -> None:
        licences = self.generator.cyclonedx_licences
        self.assertEqual(licences("MIT"), [{"license": {"id": "MIT"}}])
        self.assertEqual(
            licences("MIT OR Apache-2.0"), [{"expression": "MIT OR Apache-2.0"}]
        )

    def test_a_first_party_crate_that_declares_no_licence_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "member").mkdir()
            (root / "apps/desktop/src-tauri").mkdir(parents=True)
            (root / "Cargo.toml").write_text(
                '[workspace]\nmembers = ["member"]\n[workspace.package]\nlicense = "MIT"\n',
                encoding="utf-8",
            )
            (root / "apps/desktop/src-tauri/Cargo.toml").write_text(
                '[package]\nname = "desktop"\nlicense = "MIT"\n', encoding="utf-8"
            )
            member = root / "member" / "Cargo.toml"
            member.write_text('[package]\nname = "member"\n', encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "member declares no licence"):
                self.generator.first_party_cargo_licences(root)
            # A workspace-inherited licence resolves to the workspace's own.
            member.write_text(
                '[package]\nname = "member"\nlicense.workspace = true\n', encoding="utf-8"
            )
            self.assertEqual(
                self.generator.first_party_cargo_licences(root),
                {"member": "MIT", "desktop": "MIT"},
            )

    def test_a_path_package_with_no_manifest_here_is_refused(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            lock = Path(directory) / "Cargo.lock"
            lock.write_text(
                '[[package]]\nname = "stranger"\nversion = "1.0.0"\n', encoding="utf-8"
            )
            with self.assertRaisesRegex(ValueError, "stranger .* has no manifest here"):
                self.generator.cargo_components(lock, "Cargo.lock", {})
            self.assertTrue(
                self.generator.cargo_components(lock, "Cargo.lock", {"stranger": "MIT"})[0][
                    "first_party"
                ]
            )

    def test_first_party_python_and_npm_packages_must_declare_a_licence(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            pyproject = root / "python" / "package" / "pyproject.toml"
            pyproject.parent.mkdir(parents=True)
            pyproject.write_text(
                '[project]\nname = "package"\nversion = "1.0"\n', encoding="utf-8"
            )
            with self.assertRaisesRegex(ValueError, "must declare its licence"):
                self.generator.python_components([pyproject], root)
            package_json = root / "package.json"
            package_json.write_text(
                '{"name": "@follon/x", "version": "1.0.0"}', encoding="utf-8"
            )
            with self.assertRaisesRegex(ValueError, "name, a version and a licence"):
                self.generator.npm_first_party_component(package_json, "package.json")


if __name__ == "__main__":
    unittest.main()
