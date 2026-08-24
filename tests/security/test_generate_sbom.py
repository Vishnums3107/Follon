from __future__ import annotations

import json
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


REPOSITORY_ROOT = Path(__file__).resolve().parents[2]
GENERATOR = REPOSITORY_ROOT / "tools" / "generate_sbom.py"


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


if __name__ == "__main__":
    unittest.main()
