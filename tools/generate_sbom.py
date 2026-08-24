#!/usr/bin/env python3
"""Generate a deterministic, lockfile-backed Follon software bill of materials."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import tempfile
import tomllib
from pathlib import Path
from typing import Any


SCHEMA_VERSION = 1
GENERATOR_VERSION = "follon-sbom-generator-v1"
SOURCE_REVISION = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]{0,127}$")
PYTHON_REQUIREMENT_NAME = re.compile(r"^([A-Za-z0-9][A-Za-z0-9._-]*)")


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def normalized_component(component: dict[str, Any]) -> dict[str, Any]:
    return {key: value for key, value in component.items() if value not in (None, "", [], {})}


def cargo_components(lock_path: Path) -> list[dict[str, Any]]:
    document = tomllib.loads(lock_path.read_text(encoding="utf-8"))
    components = []
    for package in document.get("package", []):
        if not isinstance(package, dict):
            raise ValueError("Cargo.lock contains a malformed package record")
        name = package.get("name")
        version = package.get("version")
        if not isinstance(name, str) or not isinstance(version, str):
            raise ValueError("Cargo.lock package identity is incomplete")
        components.append(normalized_component({
            "ecosystem": "cargo",
            "name": name,
            "version": version,
            "source": package.get("source"),
            "checksum": package.get("checksum"),
            "declared_in": ["Cargo.lock"],
        }))
    return components


def npm_components(lock_path: Path) -> list[dict[str, Any]]:
    document = json.loads(lock_path.read_text(encoding="utf-8"))
    if document.get("lockfileVersion") != 3 or not isinstance(document.get("packages"), dict):
        raise ValueError("desktop package-lock.json must use npm lockfileVersion 3")
    components = []
    for package_path, package in document["packages"].items():
        if package_path == "":
            continue
        if not isinstance(package_path, str) or not isinstance(package, dict):
            raise ValueError("package-lock.json contains a malformed package record")
        marker = "node_modules/"
        if marker not in package_path:
            raise ValueError(f"unsupported npm lockfile package path: {package_path}")
        name = package_path.rsplit(marker, 1)[1]
        version = package.get("version")
        if not name or not isinstance(version, str):
            raise ValueError("package-lock.json package identity is incomplete")
        components.append(normalized_component({
            "ecosystem": "npm",
            "name": name,
            "version": version,
            "integrity": package.get("integrity"),
            "license": package.get("license"),
            "development": bool(package.get("dev", False)),
            "optional": bool(package.get("optional", False)),
            "declared_in": ["apps/desktop/package-lock.json"],
        }))
    return components


def python_components(pyproject_paths: list[Path], repository_root: Path) -> list[dict[str, Any]]:
    components: list[dict[str, Any]] = []
    for path in pyproject_paths:
        relative = path.relative_to(repository_root).as_posix()
        document = tomllib.loads(path.read_text(encoding="utf-8"))
        project = document.get("project")
        if not isinstance(project, dict):
            raise ValueError(f"{relative} lacks a [project] table")
        project_name = project.get("name")
        project_version = project.get("version")
        if not isinstance(project_name, str) or not isinstance(project_version, str):
            raise ValueError(f"{relative} lacks project name/version")
        components.append({
            "ecosystem": "python",
            "name": project_name,
            "version": project_version,
            "role": "application",
            "declared_in": [relative],
        })
        dependency_groups = [
            ("runtime", project.get("dependencies", [])),
            ("build", document.get("build-system", {}).get("requires", [])),
        ]
        for role, requirements in dependency_groups:
            if not isinstance(requirements, list) or not all(isinstance(item, str) for item in requirements):
                raise ValueError(f"{relative} contains malformed {role} dependencies")
            for requirement in requirements:
                match = PYTHON_REQUIREMENT_NAME.match(requirement)
                if match is None:
                    raise ValueError(f"unsupported Python requirement in {relative}: {requirement}")
                components.append({
                    "ecosystem": "python",
                    "name": match.group(1),
                    "version_requirement": requirement[len(match.group(1)):],
                    "role": role,
                    "declared_in": [relative],
                })
    return components


def merge_components(components: list[dict[str, Any]]) -> list[dict[str, Any]]:
    merged: dict[tuple[str, str, str, str, str], dict[str, Any]] = {}
    for component in components:
        key = (
            str(component.get("ecosystem", "")),
            str(component.get("name", "")),
            str(component.get("version", "")),
            str(component.get("version_requirement", "")),
            str(component.get("role", "")),
        )
        existing = merged.get(key)
        if existing is None:
            existing = dict(component)
            existing["declared_in"] = sorted(set(component.get("declared_in", [])))
            merged[key] = existing
            continue
        existing["declared_in"] = sorted(
            set(existing.get("declared_in", [])) | set(component.get("declared_in", []))
        )
        for field in ("source", "checksum", "integrity", "license"):
            candidate = component.get(field)
            if candidate not in (None, "") and existing.get(field) not in (None, "", candidate):
                raise ValueError(f"conflicting {field} for {key[0]} component {key[1]}")
            if candidate not in (None, ""):
                existing[field] = candidate
    return [merged[key] for key in sorted(merged)]


def cyclonedx_component(component: dict[str, Any]) -> dict[str, Any]:
    identity = json.dumps(component, sort_keys=True, separators=(",", ":")).encode("utf-8")
    reference = f"urn:follon:component:{hashlib.sha256(identity).hexdigest()}"
    result: dict[str, Any] = {
        "type": "application" if component.get("role") == "application" else "library",
        "bom-ref": reference,
        "name": component["name"],
        "properties": [
            {"name": "follon:ecosystem", "value": component["ecosystem"]},
            {
                "name": "follon:declared-in",
                "value": ",".join(component.get("declared_in", [])),
            },
        ],
    }
    if component.get("version"):
        result["version"] = component["version"]
    if component.get("development"):
        result["scope"] = "excluded"
    elif component.get("optional"):
        result["scope"] = "optional"
    else:
        result["scope"] = "required"
    for field in ("version_requirement", "role", "source", "integrity", "license"):
        value = component.get(field)
        if value not in (None, ""):
            result["properties"].append({"name": f"follon:{field.replace('_', '-')}", "value": str(value)})
    checksum = component.get("checksum")
    if isinstance(checksum, str) and re.fullmatch(r"[a-fA-F0-9]{64}", checksum):
        result["hashes"] = [{"alg": "SHA-256", "content": checksum.lower()}]
    elif checksum not in (None, ""):
        result["properties"].append({"name": "follon:checksum", "value": str(checksum)})
    result["properties"].sort(key=lambda item: (item["name"], item["value"]))
    return result


def build_sbom(repository_root: Path, source_revision: str) -> dict[str, Any]:
    if SOURCE_REVISION.fullmatch(source_revision) is None:
        raise ValueError("source revision must be a bounded printable revision identifier")
    cargo_lock = repository_root / "Cargo.lock"
    npm_lock = repository_root / "apps" / "desktop" / "package-lock.json"
    pyprojects = sorted((repository_root / "python").glob("*/pyproject.toml"))
    required_inputs = [cargo_lock, npm_lock, *pyprojects]
    missing = [str(path) for path in required_inputs if not path.is_file() or path.is_symlink()]
    if missing:
        raise ValueError(f"required dependency inputs are missing or unsafe: {missing}")
    inventory = merge_components([
        *cargo_components(cargo_lock),
        *npm_components(npm_lock),
        *python_components(pyprojects, repository_root),
    ])
    inputs = [
        {"path": path.relative_to(repository_root).as_posix(), "sha256": sha256_file(path)}
        for path in required_inputs
    ]
    inputs.sort(key=lambda item: item["path"])
    components = sorted(
        (cyclonedx_component(component) for component in inventory),
        key=lambda component: component["bom-ref"],
    )
    return {
        "$schema": "http://cyclonedx.org/schema/bom-1.6.schema.json",
        "bomFormat": "CycloneDX",
        "specVersion": "1.6",
        "version": SCHEMA_VERSION,
        "metadata": {
            "component": {
                "type": "application",
                "bom-ref": f"urn:follon:source:{hashlib.sha256(source_revision.encode('utf-8')).hexdigest()}",
                "name": "follon",
                "version": source_revision,
            },
            "tools": {
                "components": [
                    {
                        "type": "application",
                        "name": "follon-sbom-generator",
                        "version": GENERATOR_VERSION,
                    }
                ]
            },
            "properties": [
                {
                    "name": f"follon:input-sha256:{item['path']}",
                    "value": item["sha256"],
                }
                for item in inputs
            ],
        },
        "components": components,
    }


def publish_immutable(path: Path, document: dict[str, Any]) -> None:
    payload = (json.dumps(document, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")
    if path.is_symlink():
        raise ValueError("SBOM output must not be a symbolic link")
    if path.exists():
        if path.is_file() and path.read_bytes() == payload:
            return
        raise ValueError("refusing to overwrite a conflicting SBOM output")
    path.parent.mkdir(parents=True, exist_ok=True)
    descriptor, temporary_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent)
    temporary_path = Path(temporary_name)
    try:
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(payload)
            stream.flush()
            os.fsync(stream.fileno())
        try:
            os.link(temporary_path, path)
        except FileExistsError:
            if not path.is_symlink() and path.is_file() and path.read_bytes() == payload:
                return
            raise ValueError("refusing to overwrite a conflicting SBOM output") from None
    finally:
        temporary_path.unlink(missing_ok=True)


def parse_arguments() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository-root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--source-revision", required=True)
    parser.add_argument("--output", type=Path, required=True)
    return parser.parse_args()


def main() -> int:
    arguments = parse_arguments()
    repository_root = arguments.repository_root.resolve(strict=True)
    if not repository_root.is_dir():
        raise ValueError("repository root must be a directory")
    document = build_sbom(repository_root, arguments.source_revision)
    publish_immutable(arguments.output.absolute(), document)
    print(
        f"SBOM published: {arguments.output} "
        f"({len(document['components'])} locked or declared components)"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
