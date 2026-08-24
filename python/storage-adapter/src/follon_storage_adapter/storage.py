"""Deterministic local analytics and immutable S3-compatible artifact storage."""

from __future__ import annotations

import csv
import hashlib
import json
import os
import re
import tempfile
from dataclasses import asdict, dataclass
from datetime import datetime, timezone
from decimal import Decimal, InvalidOperation
from pathlib import Path
from typing import Any, BinaryIO

import duckdb
import pyarrow as pa
import pyarrow.parquet as pq


BAR_HEADER = (
    "event_time",
    "instrument_id",
    "open",
    "high",
    "low",
    "close",
    "volume",
    "interval_seconds",
    "exchange_timezone",
)
CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
BUCKET_NAME = re.compile(r"^[a-z0-9][a-z0-9.-]{1,61}[a-z0-9]$")
UTC_TIMESTAMP = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}Z$")
DECIMAL_SCALE = Decimal("0.00000001")
CHUNK_SIZE = 1024 * 1024


class StorageError(RuntimeError):
    """A storage invariant failed without mutating the intended target."""


@dataclass(frozen=True)
class DatasetReceipt:
    """Verified identity of one immutable Parquet dataset."""

    dataset_id: str
    dataset_version: str
    parquet_path: str
    parquet_sha256: str
    source_sha256: str
    row_count: int
    starts_at: str
    ends_at: str
    schema_version: int = 1

    def to_json(self) -> str:
        """Return stable receipt JSON."""
        payload = asdict(self)
        payload["storage_receipt_schema_version"] = payload.pop("schema_version")
        payload["parquet_file"] = Path(payload.pop("parquet_path")).name
        return json.dumps(payload, sort_keys=True, separators=(",", ":"))

    def write(self, path: Path) -> None:
        """Publish the receipt immutably beside its dataset."""
        _write_immutable_bytes(path, (self.to_json() + "\n").encode())


def _require_id(name: str, value: str) -> None:
    if len(value) > 128 or not CANONICAL_ID.fullmatch(value):
        raise StorageError(f"{name} must be a canonical ID")


def _regular_file(path: Path, name: str) -> Path:
    absolute = path.resolve()
    if path.is_symlink() or not absolute.is_file():
        raise StorageError(f"{name} must be a regular non-symlink file")
    return absolute


def _sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        while chunk := handle.read(CHUNK_SIZE):
            digest.update(chunk)
    return digest.hexdigest()


def _decimal(value: str, name: str, *, positive: bool = True) -> Decimal:
    try:
        parsed = Decimal(value)
        exact = parsed.quantize(DECIMAL_SCALE)
    except (InvalidOperation, ValueError) as error:
        raise StorageError(f"{name} is not an exact decimal with at most 8 places") from error
    if not parsed.is_finite() or parsed != exact or (positive and parsed <= 0):
        raise StorageError(f"{name} is outside the supported decimal boundary")
    return exact


def _write_immutable_bytes(target: Path, content: bytes) -> None:
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        if target.is_symlink() or not target.is_file() or target.read_bytes() != content:
            raise StorageError("immutable target already exists with different content")
        return
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{target.name}.", suffix=".tmp", dir=target.parent)
    temporary = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temporary, target)
    finally:
        temporary.unlink(missing_ok=True)


class ParquetBarStore:
    """Publishes the canonical normalized-bar CSV contract as immutable Parquet."""

    @staticmethod
    def publish(source: Path, target: Path, dataset_id: str, dataset_version: str) -> DatasetReceipt:
        _require_id("dataset_id", dataset_id)
        _require_id("dataset_version", dataset_version)
        source = _regular_file(source, "source dataset")
        rows: list[dict[str, Any]] = []
        identities: set[tuple[str, str]] = set()
        previous: tuple[str, str] | None = None
        with source.open("r", encoding="utf-8", newline="") as handle:
            reader = csv.DictReader(handle)
            if tuple(reader.fieldnames or ()) != BAR_HEADER:
                raise StorageError("historical-bar CSV header does not match the v1 contract")
            for number, row in enumerate(reader, start=2):
                event_time = row["event_time"]
                instrument_id = row["instrument_id"]
                if not UTC_TIMESTAMP.fullmatch(event_time):
                    raise StorageError(f"row {number} has a noncanonical UTC timestamp")
                try:
                    parsed_time = datetime.strptime(event_time, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=timezone.utc)
                except ValueError as error:
                    raise StorageError(f"row {number} has an invalid UTC timestamp") from error
                _require_id(f"row {number} instrument_id", instrument_id)
                key = (event_time, instrument_id)
                if key in identities or (previous is not None and key <= previous):
                    raise StorageError("historical bars must be unique and canonically ordered")
                identities.add(key)
                previous = key
                open_price = _decimal(row["open"], "open")
                high = _decimal(row["high"], "high")
                low = _decimal(row["low"], "low")
                close = _decimal(row["close"], "close")
                volume = _decimal(row["volume"], "volume", positive=False)
                if volume < 0 or high < max(open_price, low, close) or low > min(open_price, high, close):
                    raise StorageError(f"row {number} violates OHLCV invariants")
                try:
                    interval = int(row["interval_seconds"])
                except ValueError as error:
                    raise StorageError(f"row {number} has an invalid interval") from error
                if interval <= 0 or interval > 86_400 or not row["exchange_timezone"]:
                    raise StorageError(f"row {number} has invalid interval/timezone context")
                rows.append({
                    "event_time": parsed_time,
                    "instrument_id": instrument_id,
                    "open": open_price,
                    "high": high,
                    "low": low,
                    "close": close,
                    "volume": volume,
                    "interval_seconds": interval,
                    "exchange_timezone": row["exchange_timezone"],
                })
        if not rows:
            raise StorageError("historical-bar dataset is empty")
        source_hash = _sha256_file(source)
        decimal_type = pa.decimal128(38, 8)
        schema = pa.schema([
            ("event_time", pa.timestamp("s", tz="UTC")),
            ("instrument_id", pa.string()),
            ("open", decimal_type),
            ("high", decimal_type),
            ("low", decimal_type),
            ("close", decimal_type),
            ("volume", decimal_type),
            ("interval_seconds", pa.uint32()),
            ("exchange_timezone", pa.string()),
        ], metadata={
            b"follon.schema_version": b"1",
            b"follon.dataset_id": dataset_id.encode(),
            b"follon.dataset_version": dataset_version.encode(),
            b"follon.source_sha256": source_hash.encode(),
        })
        table = pa.Table.from_pylist(rows, schema=schema)
        target.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temporary_name = tempfile.mkstemp(prefix=f".{target.name}.", suffix=".tmp", dir=target.parent)
        os.close(descriptor)
        temporary = Path(temporary_name)
        try:
            pq.write_table(table, temporary, compression="zstd", version="2.6", write_statistics=True)
            content = temporary.read_bytes()
            _write_immutable_bytes(target, content)
        finally:
            temporary.unlink(missing_ok=True)
        return ParquetBarStore.inspect(target)

    @staticmethod
    def inspect(path: Path) -> DatasetReceipt:
        path = _regular_file(path, "Parquet dataset")
        with path.open("rb") as handle:
            parquet = pq.ParquetFile(handle)
            metadata = parquet.schema_arrow.metadata or {}
            row_count = parquet.metadata.num_rows
            required = [b"follon.dataset_id", b"follon.dataset_version", b"follon.source_sha256"]
            if any(key not in metadata for key in required) or row_count <= 0:
                raise StorageError("Parquet dataset is missing Follon identity metadata")
            event_times = parquet.read(columns=["event_time"]).column("event_time")
            raw_times = event_times.cast(pa.int64()).to_pylist()
            unit_divisor = {"s": 1, "ms": 1_000, "us": 1_000_000, "ns": 1_000_000_000}.get(event_times.type.unit)
            if unit_divisor is None:
                raise StorageError("Parquet event_time has an unsupported precision")
            epoch_seconds = [value // unit_divisor for value in raw_times]
        render = lambda value: datetime.fromtimestamp(value, tz=timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
        return DatasetReceipt(
            dataset_id=metadata[b"follon.dataset_id"].decode(),
            dataset_version=metadata[b"follon.dataset_version"].decode(),
            parquet_path=str(path),
            parquet_sha256=_sha256_file(path),
            source_sha256=metadata[b"follon.source_sha256"].decode(),
            row_count=row_count,
            starts_at=render(epoch_seconds[0]),
            ends_at=render(epoch_seconds[-1]),
        )


class DuckDbCatalog:
    """Durable local catalogue over immutable external Parquet datasets."""

    @staticmethod
    def register(database: Path, receipt: DatasetReceipt) -> None:
        parquet = _regular_file(Path(receipt.parquet_path), "Parquet dataset")
        if _sha256_file(parquet) != receipt.parquet_sha256:
            raise StorageError("Parquet content no longer matches its receipt")
        database.parent.mkdir(parents=True, exist_ok=True)
        if database.is_symlink():
            raise StorageError("DuckDB catalogue cannot be a symlink")
        connection = duckdb.connect(str(database))
        try:
            connection.execute("""
                CREATE TABLE IF NOT EXISTS dataset_catalog (
                    dataset_id VARCHAR NOT NULL,
                    dataset_version VARCHAR NOT NULL,
                    parquet_path VARCHAR NOT NULL,
                    parquet_sha256 VARCHAR NOT NULL,
                    source_sha256 VARCHAR NOT NULL,
                    row_count UBIGINT NOT NULL,
                    starts_at TIMESTAMPTZ NOT NULL,
                    ends_at TIMESTAMPTZ NOT NULL,
                    PRIMARY KEY (dataset_id, dataset_version)
                )
            """)
            actual_rows = connection.execute("SELECT count(*) FROM read_parquet(?)", [str(parquet)]).fetchone()[0]
            if actual_rows != receipt.row_count:
                raise StorageError("DuckDB row verification does not match the receipt")
            existing = connection.execute(
                "SELECT parquet_path, parquet_sha256, source_sha256, row_count FROM dataset_catalog WHERE dataset_id=? AND dataset_version=?",
                [receipt.dataset_id, receipt.dataset_version],
            ).fetchone()
            expected = (str(parquet), receipt.parquet_sha256, receipt.source_sha256, receipt.row_count)
            if existing is not None:
                if existing[:4] != expected:
                    raise StorageError("dataset identity already maps to different immutable content")
                return
            connection.execute(
                "INSERT INTO dataset_catalog VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
                [receipt.dataset_id, receipt.dataset_version, str(parquet), receipt.parquet_sha256,
                 receipt.source_sha256, receipt.row_count, receipt.starts_at, receipt.ends_at],
            )
            connection.execute("CHECKPOINT")
        finally:
            connection.close()

    @staticmethod
    def entries(database: Path) -> list[dict[str, Any]]:
        database = _regular_file(database, "DuckDB catalogue")
        connection = duckdb.connect(str(database), read_only=True)
        try:
            rows = connection.execute(
                "SELECT dataset_id, dataset_version, parquet_path, parquet_sha256, source_sha256, row_count FROM dataset_catalog ORDER BY dataset_id, dataset_version"
            ).fetchall()
            return [dict(zip(("dataset_id", "dataset_version", "parquet_path", "parquet_sha256", "source_sha256", "row_count"), row)) for row in rows]
        finally:
            connection.close()


def _valid_object_key(key: str) -> bool:
    parts = key.split("/")
    return bool(key) and len(key.encode()) <= 1024 and not key.startswith("/") and "\\" not in key \
        and all(part not in ("", ".", "..") for part in parts) and all(ord(char) >= 32 for char in key)


def _body_hash(body: BinaryIO, destination: BinaryIO | None = None) -> tuple[str, int]:
    digest = hashlib.sha256()
    size = 0
    while chunk := body.read(CHUNK_SIZE):
        digest.update(chunk)
        size += len(chunk)
        if destination is not None:
            destination.write(chunk)
    close = getattr(body, "close", None)
    if callable(close):
        close()
    return digest.hexdigest(), size


class ImmutableS3Store:
    """Idempotent, verified S3/MinIO artifact publisher and recovery reader."""

    def __init__(self, client: Any, *, server_side_encryption: str | None = "AES256"):
        if server_side_encryption not in {"AES256", None}:
            raise StorageError("unsupported server-side encryption policy")
        self.client = client
        self.server_side_encryption = server_side_encryption

    @staticmethod
    def _error_code(error: Exception) -> str:
        response = getattr(error, "response", {})
        return str(response.get("Error", {}).get("Code", ""))

    @classmethod
    def _not_found(cls, error: Exception) -> bool:
        return cls._error_code(error) in {"404", "NoSuchKey", "NotFound"}

    def _remote_identity(self, bucket: str, key: str) -> tuple[str, int] | None:
        try:
            response = self.client.get_object(Bucket=bucket, Key=key)
        except Exception as error:  # SDK exception type differs across S3-compatible servers.
            if self._not_found(error):
                return None
            raise StorageError("object-store read failed") from error
        digest, size = _body_hash(response["Body"])
        declared = (response.get("Metadata") or {}).get("sha256")
        if declared != digest or int(response.get("ContentLength", size)) != size:
            raise StorageError("remote object content or SHA-256 metadata is inconsistent")
        return digest, size

    def ensure_versioned_bucket(self, bucket: str) -> dict[str, Any]:
        if not BUCKET_NAME.fullmatch(bucket) or ".." in bucket:
            raise StorageError("bucket name is invalid")
        created = False
        try:
            self.client.head_bucket(Bucket=bucket)
        except Exception as error:
            if self._error_code(error) in {"403", "AccessDenied"}:
                # Some S3-compatible deployments deny HeadBucket while allowing
                # explicit versioning/object operations. The verified call below
                # remains authoritative and still fails closed without access.
                pass
            elif not self._not_found(error):
                raise StorageError("bucket lookup failed") from error
            else:
                try:
                    self.client.create_bucket(Bucket=bucket)
                    created = True
                except Exception as create_error:
                    raise StorageError("bucket creation failed") from create_error
        try:
            self.client.put_bucket_versioning(Bucket=bucket, VersioningConfiguration={"Status": "Enabled"})
            status = self.client.get_bucket_versioning(Bucket=bucket).get("Status")
        except Exception as error:
            raise StorageError("bucket versioning verification failed") from error
        if status != "Enabled":
            raise StorageError("bucket versioning is not enabled")
        return {"bucket": bucket, "created": created, "versioning": status}

    def publish(self, source: Path, bucket: str, key: str) -> dict[str, Any]:
        source = _regular_file(source, "artifact")
        if not BUCKET_NAME.fullmatch(bucket) or not _valid_object_key(key):
            raise StorageError("bucket and normalized object key are required")
        digest = _sha256_file(source)
        size = source.stat().st_size
        existing = self._remote_identity(bucket, key)
        if existing is not None:
            if existing != (digest, size):
                raise StorageError("immutable object key already contains different content")
            return {"bucket": bucket, "key": key, "sha256": digest, "bytes": size, "idempotent": True}
        try:
            request: dict[str, Any] = {
                "Bucket": bucket,
                "Key": key,
                "ContentType": "application/octet-stream",
                "Metadata": {"sha256": digest, "follon-schema-version": "1"},
                # Prevent two publishers from racing between the identity read
                # above and the write. Versioning preserves history, but an
                # immutable logical key must never gain a conflicting latest
                # version.
                "IfNoneMatch": "*",
            }
            if self.server_side_encryption is not None:
                request["ServerSideEncryption"] = self.server_side_encryption
            with source.open("rb") as handle:
                self.client.put_object(Body=handle, **request)
        except Exception as error:
            if self._error_code(error) in {"409", "412", "ConditionalRequestConflict", "PreconditionFailed"}:
                raced = self._remote_identity(bucket, key)
                if raced == (digest, size):
                    return {"bucket": bucket, "key": key, "sha256": digest, "bytes": size, "idempotent": True}
                if raced is not None:
                    raise StorageError("immutable object key already contains different content") from error
            raise StorageError("object-store publication failed") from error
        if self._remote_identity(bucket, key) != (digest, size):
            raise StorageError("published object failed read-after-write verification")
        return {"bucket": bucket, "key": key, "sha256": digest, "bytes": size, "idempotent": False}

    def recover(self, bucket: str, key: str, target: Path) -> dict[str, Any]:
        if not BUCKET_NAME.fullmatch(bucket) or not _valid_object_key(key):
            raise StorageError("bucket and normalized object key are required")
        try:
            response = self.client.get_object(Bucket=bucket, Key=key)
        except Exception as error:
            raise StorageError("object-store recovery read failed") from error
        target.parent.mkdir(parents=True, exist_ok=True)
        descriptor, temporary_name = tempfile.mkstemp(prefix=f".{target.name}.", suffix=".tmp", dir=target.parent)
        temporary = Path(temporary_name)
        try:
            with os.fdopen(descriptor, "wb") as handle:
                digest, size = _body_hash(response["Body"], handle)
                handle.flush()
                os.fsync(handle.fileno())
            declared = (response.get("Metadata") or {}).get("sha256")
            declared_size = int(response.get("ContentLength", size))
            if declared != digest or declared_size != size:
                raise StorageError("recovered object does not match its SHA-256 metadata or length")
            content = temporary.read_bytes()
            _write_immutable_bytes(target, content)
        finally:
            temporary.unlink(missing_ok=True)
        return {"bucket": bucket, "key": key, "target": str(target.resolve()), "sha256": digest, "bytes": size}
