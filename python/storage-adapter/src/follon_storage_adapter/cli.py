"""Command-line entry point for explicit storage operations."""

from __future__ import annotations

import argparse
import json
import os
from pathlib import Path
from urllib.parse import urlsplit

import boto3
from botocore.config import Config

from .storage import DuckDbCatalog, ImmutableS3Store, ParquetBarStore


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(prog="follon-storage")
    commands = root.add_subparsers(dest="command", required=True)
    parquet = commands.add_parser("publish-bars")
    parquet.add_argument("source", type=Path)
    parquet.add_argument("target", type=Path)
    parquet.add_argument("--dataset-id", required=True)
    parquet.add_argument("--dataset-version", required=True)
    register = commands.add_parser("register-dataset")
    register.add_argument("parquet", type=Path)
    register.add_argument("database", type=Path)
    entries = commands.add_parser("list-datasets")
    entries.add_argument("database", type=Path)
    ensure = commands.add_parser("ensure-bucket")
    ensure.add_argument("--bucket", required=True)
    ensure.add_argument("--endpoint-url", default=os.environ.get("FOLLON_S3_ENDPOINT_URL"))
    for name in ("publish-artifact", "recover-artifact"):
        command = commands.add_parser(name)
        command.add_argument("--bucket", required=True)
        command.add_argument("--key", required=True)
        command.add_argument("--endpoint-url", default=os.environ.get("FOLLON_S3_ENDPOINT_URL"))
        if name == "publish-artifact":
            command.add_argument("--allow-unencrypted-development", action="store_true")
        command.add_argument("path", type=Path)
    return root


def _s3(endpoint_url: str | None, allow_unencrypted_development: bool = False) -> ImmutableS3Store:
    encryption = "AES256"
    if allow_unencrypted_development:
        hostname = urlsplit(endpoint_url or "").hostname
        if hostname not in {"127.0.0.1", "localhost", "minio"}:
            raise ValueError("unencrypted development storage is restricted to a local MinIO endpoint")
        encryption = None
    client = boto3.client(
        "s3",
        endpoint_url=endpoint_url,
        region_name=os.environ.get("AWS_REGION", "us-east-1"),
        config=Config(signature_version="s3v4", s3={"addressing_style": "path"}),
    )
    return ImmutableS3Store(client, server_side_encryption=encryption)


def main() -> None:
    args = parser().parse_args()
    if args.command == "publish-bars":
        result = ParquetBarStore.publish(args.source, args.target, args.dataset_id, args.dataset_version)
        result.write(Path(f"{args.target}.receipt.json"))
        print(result.to_json())
    elif args.command == "register-dataset":
        receipt = ParquetBarStore.inspect(args.parquet)
        DuckDbCatalog.register(args.database, receipt)
        print(receipt.to_json())
    elif args.command == "list-datasets":
        print(json.dumps(DuckDbCatalog.entries(args.database), sort_keys=True, separators=(",", ":")))
    elif args.command == "ensure-bucket":
        print(json.dumps(_s3(args.endpoint_url).ensure_versioned_bucket(args.bucket), sort_keys=True))
    elif args.command == "publish-artifact":
        print(json.dumps(_s3(args.endpoint_url, args.allow_unencrypted_development).publish(args.path, args.bucket, args.key), sort_keys=True))
    else:
        print(json.dumps(_s3(args.endpoint_url).recover(args.bucket, args.key, args.path), sort_keys=True))


if __name__ == "__main__":
    main()
