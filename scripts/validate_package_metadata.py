#!/usr/bin/env python3
"""Validate repository and documentation links in publishable Cargo manifests."""

from __future__ import annotations

import argparse
import sys
from pathlib import Path
from urllib.parse import urlparse

try:
    import tomllib
except ModuleNotFoundError:
    tomllib = None


CANONICAL_REPOSITORY = "https://github.com/Josetic224/StarForge"
MANIFESTS = (
    Path("Cargo.toml"),
    Path("crates/starforge-wasm/Cargo.toml"),
    Path("crates/starforge-plugin-sdk/Cargo.toml"),
    Path("wasm/Cargo.toml"),
)


class MetadataValidationError(ValueError):
    """Raised when a package manifest has incomplete or unsafe links."""


def _validate_url(value: object, field: str, manifest: Path) -> str:
    if not isinstance(value, str):
        raise MetadataValidationError(f"{manifest}: {field} must be a URL string")
    parsed = urlparse(value)
    if parsed.scheme != "https" or parsed.netloc != "github.com":
        raise MetadataValidationError(f"{manifest}: {field} must use an HTTPS URL")
    if value != CANONICAL_REPOSITORY:
        raise MetadataValidationError(
            f"{manifest}: {field} must point to {CANONICAL_REPOSITORY}"
        )
    return value


def validate_manifest(manifest: Path) -> None:
    if tomllib is None:
        raise RuntimeError("Python 3.11 or newer is required (tomllib is unavailable)")
    try:
        data = tomllib.loads(manifest.read_text(encoding="utf-8"))
    except (OSError, tomllib.TOMLDecodeError) as error:
        raise MetadataValidationError(f"{manifest}: cannot parse manifest: {error}") from error

    package = data.get("package")
    if not isinstance(package, dict):
        raise MetadataValidationError(f"{manifest}: missing [package] table")
    package_name = package.get("name")
    if not isinstance(package_name, str) or not package_name:
        raise MetadataValidationError(f"{manifest}: package.name is required")

    _validate_url(package.get("repository"), "package.repository", manifest)
    _validate_url(package.get("homepage"), "package.homepage", manifest)
    expected_docs = f"https://docs.rs/{package_name}"
    if package.get("documentation") != expected_docs:
        raise MetadataValidationError(
            f"{manifest}: package.documentation must be {expected_docs}"
        )


def validate_manifests(root: Path, manifests: tuple[Path, ...] = MANIFESTS) -> None:
    for relative_manifest in manifests:
        manifest = root / relative_manifest
        if not manifest.is_file():
            raise MetadataValidationError(f"{relative_manifest}: manifest does not exist")
        validate_manifest(manifest)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", type=Path, default=Path.cwd())
    args = parser.parse_args(argv)
    try:
        validate_manifests(args.root.resolve())
    except (MetadataValidationError, RuntimeError) as error:
        print(f"package metadata validation failed: {error}", file=sys.stderr)
        return 1
    print(f"validated {len(MANIFESTS)} package manifests")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())