from __future__ import annotations

import os
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from tools.postgres_recovery import RecoveryError, require_connection_environment, restore_drill


class PostgresRecoverySafetyTests(unittest.TestCase):
    def test_password_environment_is_refused(self) -> None:
        with patch.dict(os.environ, {
            "PGHOST": "localhost", "PGPORT": "5432", "PGDATABASE": "follon", "PGUSER": "follon", "PGPASSWORD": "secret",
        }, clear=True):
            with self.assertRaises(RecoveryError):
                require_connection_environment()

    def test_restore_target_requires_exact_disposable_confirmation(self) -> None:
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {
            "PGHOST": "localhost", "PGPORT": "5432", "PGDATABASE": "postgres", "PGUSER": "follon",
        }, clear=True):
            root = Path(directory)
            dump = root / "backup.dump"
            manifest = root / "backup.json"
            dump.write_bytes(b"not reached")
            manifest.write_text("{}", encoding="utf-8")
            with self.assertRaises(RecoveryError):
                restore_drill(dump, manifest, "follon", "follon", root / "receipt.json")


if __name__ == "__main__":
    unittest.main()
