"""Create an unpublished Windows fork candidate archive with a source receipt.

The input binary's source, version, platform, and release suitability are not verified.
"""
import argparse
import hashlib
import json
import os
import re
import tempfile
import stat
import subprocess
import zipfile
from pathlib import Path

if __package__:
    from .fork_release_preflight import preflight
else:
    from fork_release_preflight import preflight


def info(name):
    entry = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
    entry.compress_type = zipfile.ZIP_DEFLATED
    entry.external_attr = 0o100644 << 16
    return entry


def candidate_directory(commit):
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ValueError("candidate commit must be a full lowercase SHA-1")
    base = Path(tempfile.gettempdir()) / "herdr-fork-packaging"
    candidate = base / commit
    for directory in (base, candidate):
        metadata = directory.lstat()
        if not stat.S_ISDIR(metadata.st_mode) or getattr(metadata, "st_file_attributes", 0) & 0x400:
            raise ValueError("candidate directory must not be a symlink or reparse point")
    return candidate


def package(version, upstream_commit):
    receipt = preflight(version, upstream_commit)
    directory = candidate_directory(receipt["candidate_commit"])
    binary = directory / "herdr.exe"
    output = directory / "candidate.zip"
    binary_metadata = binary.lstat()
    if not stat.S_ISREG(binary_metadata.st_mode) or getattr(binary_metadata, "st_file_attributes", 0) & 0x400:
        raise ValueError("binary must be a regular non-symlink file")
    if output.exists() or output.is_symlink():
        raise ValueError("output already exists")
    with binary.open("rb") as source:
        if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
            raise ValueError("binary must be a regular file")
        if source.read(2) != b"MZ":
            raise ValueError("binary does not have a Windows executable header")
        source.seek(0)
        digest = hashlib.sha256()
        size = 0
        with output.open("xb") as destination:
            with zipfile.ZipFile(destination, "w") as archive:
                with archive.open(info("herdr.exe"), "w") as member:
                    while chunk := source.read(1024 * 1024):
                        digest.update(chunk)
                        size += len(chunk)
                        member.write(chunk)
                manifest = {"schema": 1, "source_preflight": receipt,
                            "artifact": {"name": "herdr.exe", "sha256": digest.hexdigest(), "size": size},
                            "assurance": "binary_source_version_platform_and_release_suitability_unverified"}
                archive.writestr(info("fork-provenance.json"), json.dumps(manifest, sort_keys=True, indent=2) + "\n")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    parser.add_argument("--upstream-commit", required=True)
    args = parser.parse_args()
    print(json.dumps(package(args.version, args.upstream_commit), sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, KeyError, subprocess.CalledProcessError) as error:
        raise SystemExit(f"error: {error}") from error
