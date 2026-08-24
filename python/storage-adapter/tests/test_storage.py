from __future__ import annotations

import io
import json
import tempfile
import unittest
from pathlib import Path

from follon_storage_adapter import DuckDbCatalog, ImmutableS3Store, ParquetBarStore, StorageError


BARS = """event_time,instrument_id,open,high,low,close,volume,interval_seconds,exchange_timezone
2026-01-02T14:31:00Z,inst.us_equity.spy,100.00000000,101.00000000,99.00000000,100.00000000,1000.00000000,60,America/New_York
2026-01-02T14:32:00Z,inst.us_equity.spy,100.00000000,102.00000000,99.50000000,101.00000000,800.00000000,60,America/New_York
"""


class NotFound(Exception):
    response = {"Error": {"Code": "NoSuchKey"}}


class PreconditionFailed(Exception):
    response = {"Error": {"Code": "PreconditionFailed"}}


class MemoryS3:
    def __init__(self) -> None:
        self.objects: dict[tuple[str, str], tuple[bytes, dict[str, str]]] = {}
        self.buckets: set[str] = set()
        self.versioning: dict[str, str] = {}
        self.last_put_options: dict[str, object] = {}

    def head_bucket(self, *, Bucket: str):
        if Bucket not in self.buckets:
            raise NotFound()

    def create_bucket(self, *, Bucket: str):
        self.buckets.add(Bucket)

    def put_bucket_versioning(self, *, Bucket: str, VersioningConfiguration):
        self.versioning[Bucket] = VersioningConfiguration["Status"]

    def get_bucket_versioning(self, *, Bucket: str):
        return {"Status": self.versioning.get(Bucket)}

    def get_object(self, *, Bucket: str, Key: str):
        try:
            body, metadata = self.objects[(Bucket, Key)]
        except KeyError as error:
            raise NotFound() from error
        return {"Body": io.BytesIO(body), "Metadata": metadata.copy(), "ContentLength": len(body)}

    def put_object(self, *, Bucket: str, Key: str, Body, Metadata, **options):
        self.last_put_options = options
        if options.get("IfNoneMatch") == "*" and (Bucket, Key) in self.objects:
            raise PreconditionFailed()
        self.objects[(Bucket, Key)] = (Body.read(), Metadata.copy())


class StorageAdapterTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.source = self.root / "bars.csv"
        self.source.write_text(BARS, encoding="utf-8", newline="")

    def tearDown(self) -> None:
        self.temporary.cleanup()

    def test_parquet_publication_is_deterministic_and_self_describing(self) -> None:
        first = self.root / "first.parquet"
        second = self.root / "second.parquet"
        receipt = ParquetBarStore.publish(self.source, first, "dataset.spy", "v1")
        repeated = ParquetBarStore.publish(self.source, second, "dataset.spy", "v1")
        self.assertEqual(first.read_bytes(), second.read_bytes())
        self.assertEqual(receipt.parquet_sha256, repeated.parquet_sha256)
        self.assertEqual(receipt.row_count, 2)
        self.assertEqual(receipt.starts_at, "2026-01-02T14:31:00Z")
        self.assertEqual(json.loads(receipt.to_json())["dataset_id"], "dataset.spy")

    def test_parquet_target_is_immutable_and_csv_contract_fails_closed(self) -> None:
        target = self.root / "bars.parquet"
        ParquetBarStore.publish(self.source, target, "dataset.spy", "v1")
        ParquetBarStore.publish(self.source, target, "dataset.spy", "v1")
        changed = self.root / "changed.csv"
        changed.write_text(BARS.replace("101.00000000,800", "100.50000000,800"), encoding="utf-8")
        with self.assertRaises(StorageError):
            ParquetBarStore.publish(changed, target, "dataset.spy", "v1")
        malformed = self.root / "malformed.csv"
        malformed.write_text(BARS.replace(
            "2026-01-02T14:32:00Z,inst.us_equity.spy,100.00000000,102.00000000",
            "2026-01-02T14:32:00Z,inst.us_equity.spy,103.00000000,102.00000000",
        ), encoding="utf-8")
        with self.assertRaises(StorageError):
            ParquetBarStore.publish(malformed, self.root / "bad.parquet", "dataset.spy", "v1")

    def test_duckdb_catalog_registers_and_verifies_external_parquet(self) -> None:
        parquet = self.root / "bars.parquet"
        receipt = ParquetBarStore.publish(self.source, parquet, "dataset.spy", "v1")
        database = self.root / "research.duckdb"
        DuckDbCatalog.register(database, receipt)
        DuckDbCatalog.register(database, receipt)
        entries = DuckDbCatalog.entries(database)
        self.assertEqual(len(entries), 1)
        self.assertEqual(entries[0]["row_count"], 2)
        self.assertEqual(entries[0]["parquet_sha256"], receipt.parquet_sha256)

    def test_s3_publication_and_recovery_are_verified_and_idempotent(self) -> None:
        client = MemoryS3()
        store = ImmutableS3Store(client)
        bucket = store.ensure_versioned_bucket("follon-evidence")
        self.assertTrue(bucket["created"])
        self.assertEqual(store.ensure_versioned_bucket("follon-evidence")["versioning"], "Enabled")
        artifact = self.root / "artifact.json"
        artifact.write_text('{"artifact":"immutable"}', encoding="utf-8")
        first = store.publish(artifact, "follon-evidence", "backtests/run-1/artifact.json")
        second = store.publish(artifact, "follon-evidence", "backtests/run-1/artifact.json")
        self.assertFalse(first["idempotent"])
        self.assertTrue(second["idempotent"])
        self.assertEqual(client.last_put_options["IfNoneMatch"], "*")
        recovered = self.root / "recovered.json"
        receipt = store.recover("follon-evidence", "backtests/run-1/artifact.json", recovered)
        self.assertEqual(recovered.read_bytes(), artifact.read_bytes())
        self.assertEqual(receipt["sha256"], first["sha256"])

    def test_s3_conflicts_tampering_and_unsafe_keys_fail_closed(self) -> None:
        client = MemoryS3()
        store = ImmutableS3Store(client)
        artifact = self.root / "artifact.json"
        artifact.write_text("first", encoding="utf-8")
        store.publish(artifact, "follon-evidence", "runs/one.json")
        artifact.write_text("second", encoding="utf-8")
        with self.assertRaises(StorageError):
            store.publish(artifact, "follon-evidence", "runs/one.json")
        with self.assertRaises(StorageError):
            store.publish(artifact, "follon-evidence", "../escape.json")
        body, metadata = client.objects[("follon-evidence", "runs/one.json")]
        client.objects[("follon-evidence", "runs/one.json")] = (body + b"tampered", metadata)
        with self.assertRaises(StorageError):
            store.recover("follon-evidence", "runs/one.json", self.root / "recovered.json")

    def test_s3_conditional_write_rejects_a_conflicting_race(self) -> None:
        class RacingS3(MemoryS3):
            def put_object(self, *, Bucket: str, Key: str, Body, Metadata, **options):
                self.objects[(Bucket, Key)] = (b"other writer", {"sha256": "tampered"})
                raise PreconditionFailed()

        artifact = self.root / "artifact.json"
        artifact.write_text("expected", encoding="utf-8")
        with self.assertRaises(StorageError):
            ImmutableS3Store(RacingS3()).publish(artifact, "follon-evidence", "runs/race.json")


if __name__ == "__main__":
    unittest.main()
