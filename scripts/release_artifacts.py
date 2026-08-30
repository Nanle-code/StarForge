#!/usr/bin/env python3
"""Prepare and verify the files published by the release workflow."""

from __future__ import annotations

import argparse
import hashlib
import shutil
from pathlib import Path

EXPECTED_ARCHIVES = {
    "starforge-linux-x86_64.tar.gz",
    "starforge-linux-aarch64.tar.gz",
    "starforge-macos-aarch64.tar.gz",
    "starforge-windows-x86_64.zip",
}


def prepare_release(source: Path, destination: Path) -> list[Path]:
    """Copy the expected archives and write a deterministic SHA-256 manifest."""
    if not source.is_dir():
        raise ValueError(f"artifact directory does not exist: {source}")

    archives = sorted(path for path in source.rglob("*") if path.is_file())
    archive_names = {path.name for path in archives}
    unexpected = archive_names - EXPECTED_ARCHIVES
    missing = EXPECTED_ARCHIVES - archive_names
    if unexpected:
        raise ValueError(f"unsupported release archives: {', '.join(sorted(unexpected))}")
    if missing:
        raise ValueError(f"missing release archives: {', '.join(sorted(missing))}")
    if len(archives) != len(archive_names):
        raise ValueError("duplicate release archive names found")

    destination.mkdir(parents=True, exist_ok=True)
    published = []
    for archive in sorted(archives, key=lambda path: path.name):
        target = destination / archive.name
        shutil.copy2(archive, target)
        published.append(target)

    manifest = destination / "SHA256SUMS.txt"
    manifest.write_text(
        "".join(
            f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n"
            for path in published
        ),
        encoding="ascii",
    )
    return published


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="directory containing downloaded build artifacts")
    parser.add_argument("destination", type=Path, help="directory for release files")
    args = parser.parse_args()
    try:
        prepare_release(args.source, args.destination)
    except (OSError, ValueError) as error:
        parser.error(str(error))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())