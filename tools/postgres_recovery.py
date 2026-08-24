#!/usr/bin/env python3
"""Create verified PostgreSQL backups and run isolated restore drills without secret arguments."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import subprocess
import sys
from datetime import UTC, datetime
from pathlib import Path

CANONICAL_ID = re.compile(r"^[a-z0-9._-]+$")
DRILL_DATABASE = re.compile(r"^follon_restore_drill_[a-z0-9_]+$")


class RecoveryError(RuntimeError):
    """Raised when a recovery action is unsafe or incomplete."""


def require_connection_environment() -> dict[str, str]:
    if os.environ.get("PGPASSWORD"):
        raise RecoveryError("PGPASSWORD is refused; use a protected PGPASSFILE or managed identity")
    required = ("PGHOST", "PGPORT", "PGDATABASE", "PGUSER")
    missing = [name for name in required if not os.environ.get(name)]
    if missing:
        raise RecoveryError(f"missing libpq settings: {', '.join(missing)}")
    passfile = os.environ.get("PGPASSFILE")
    if passfile and (not Path(passfile).is_file() or Path(passfile).is_symlink()):
        raise RecoveryError("PGPASSFILE must be an existing non-symlinked protected file")
    return {name: os.environ[name] for name in required}


def executable(name: str) -> str:
    found = shutil.which(name)
    if found is None:
        raise RecoveryError(f"required PostgreSQL tool is unavailable: {name}")
    return found


def run(command: list[str], *, capture: bool = False) -> str:
    result = subprocess.run(
        command,
        check=False,
        shell=False,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE if capture else subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    if result.returncode != 0:
        reason = result.stderr.strip().splitlines()[-1] if result.stderr.strip() else "unknown failure"
        raise RecoveryError(f"PostgreSQL tool failed: {reason[:512]}")
    return result.stdout if capture else ""


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def atomic_json(path: Path, value: object) -> None:
    temporary = path.with_suffix(path.suffix + ".tmp")
    with temporary.open("w", encoding="utf-8", newline="\n") as destination:
        json.dump(value, destination, sort_keys=True, separators=(",", ":"))
        destination.write("\n")
        destination.flush()
        os.fsync(destination.fileno())
    temporary.replace(path)


def create_backup(output_directory: Path, backup_id: str, created_at: str) -> Path:
    connection = require_connection_environment()
    if CANONICAL_ID.fullmatch(backup_id) is None:
        raise RecoveryError("backup_id must be a canonical ID")
    output_directory.mkdir(parents=True, exist_ok=True)
    if output_directory.is_symlink():
        raise RecoveryError("backup output directory cannot be a symlink")
    output_directory = output_directory.resolve(strict=True)
    dump = output_directory / f"{backup_id}.dump"
    manifest = output_directory / f"{backup_id}.manifest.json"
    if dump.exists() or manifest.exists():
        raise RecoveryError("backup ID already exists; backups are immutable")
    temporary = dump.with_suffix(".dump.tmp")
    try:
        run([
            executable("pg_dump"),
            "--format=custom",
            "--compress=9",
            "--no-owner",
            "--no-privileges",
            "--file", str(temporary),
            "--host", connection["PGHOST"],
            "--port", connection["PGPORT"],
            "--username", connection["PGUSER"],
            connection["PGDATABASE"],
        ])
        if not temporary.is_file() or temporary.stat().st_size == 0:
            raise RecoveryError("pg_dump did not create a non-empty backup")
        temporary.replace(dump)
        atomic_json(manifest, {
            "postgres_backup_manifest_schema_version": 1,
            "backup_id": backup_id,
            "created_at": created_at,
            "database": connection["PGDATABASE"],
            "host": connection["PGHOST"],
            "bytes": dump.stat().st_size,
            "sha256": sha256_file(dump),
            "format": "postgres-custom",
        })
        return manifest
    finally:
        temporary.unlink(missing_ok=True)


def restore_drill(
    dump: Path,
    manifest: Path,
    target_database: str,
    confirmation: str,
    receipt: Path,
) -> None:
    connection = require_connection_environment()
    if DRILL_DATABASE.fullmatch(target_database) is None or confirmation != target_database:
        raise RecoveryError("restore target must be an explicitly confirmed follon_restore_drill_* database")
    if dump.is_symlink() or manifest.is_symlink():
        raise RecoveryError("backup and manifest cannot be symlinks")
    dump = dump.resolve(strict=True)
    manifest_data = json.loads(manifest.resolve(strict=True).read_text(encoding="utf-8"))
    if (
        not dump.is_file()
        or manifest_data.get("sha256") != sha256_file(dump)
        or manifest_data.get("bytes") != dump.stat().st_size
    ):
        raise RecoveryError("backup does not match its manifest")
    common = ["--host", connection["PGHOST"], "--port", connection["PGPORT"], "--username", connection["PGUSER"]]
    created = False
    started_at = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
    try:
        run([executable("createdb"), *common, target_database])
        created = True
        run([
            executable("pg_restore"), *common,
            "--dbname", target_database,
            "--exit-on-error",
            "--single-transaction",
            "--no-owner",
            "--no-privileges",
            str(dump),
        ])
        migration = run([
            executable("psql"), *common,
            "--dbname", target_database,
            "--tuples-only", "--no-align",
            "--command", "SELECT COALESCE(MAX(version),0) FROM follon_schema_migrations;",
        ], capture=True).strip()
        if not migration.isdigit() or int(migration) < 1:
            raise RecoveryError("restored database is missing the Follon schema migration")
        completed_at = datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")
        receipt.parent.mkdir(parents=True, exist_ok=True)
        atomic_json(receipt, {
            "restore_drill_receipt_schema_version": 1,
            "backup_id": manifest_data.get("backup_id"),
            "backup_sha256": manifest_data["sha256"],
            "target_database": target_database,
            "started_at": started_at,
            "completed_at": completed_at,
            "schema_migration": int(migration),
            "result": "verified",
        })
    finally:
        if created:
            run([executable("dropdb"), *common, target_database])


def utc_now() -> str:
    return datetime.now(UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    subparsers = parser.add_subparsers(dest="operation", required=True)
    backup = subparsers.add_parser("backup")
    backup.add_argument("--output-directory", required=True, type=Path)
    backup.add_argument("--backup-id", required=True)
    drill = subparsers.add_parser("restore-drill")
    drill.add_argument("--dump", required=True, type=Path)
    drill.add_argument("--manifest", required=True, type=Path)
    drill.add_argument("--target-database", required=True)
    drill.add_argument("--confirm-disposable-database", required=True)
    drill.add_argument("--receipt", required=True, type=Path)
    arguments = parser.parse_args(argv)
    try:
        if arguments.operation == "backup":
            print(create_backup(arguments.output_directory, arguments.backup_id, utc_now()))
        else:
            restore_drill(
                arguments.dump,
                arguments.manifest,
                arguments.target_database,
                arguments.confirm_disposable_database,
                arguments.receipt,
            )
    except (RecoveryError, OSError, json.JSONDecodeError) as error:
        print(f"PostgreSQL recovery operation failed: {error}", file=sys.stderr)
        return 2
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
