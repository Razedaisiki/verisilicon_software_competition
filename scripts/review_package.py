#!/usr/bin/env python3
"""Create or verify the deterministic provisional source review package."""

from __future__ import annotations

import hashlib
import re
import sys
import zipfile
from pathlib import Path, PurePosixPath


MANIFEST_NAME = "MANIFEST.sha256"
FIXED_TIMESTAMP = (1980, 1, 1, 0, 0, 0)
FIXED_MODE = 0o100644
SOURCE_PATHS = (
    ".github/workflows/ci.yml",
    ".gitignore",
    "CHANGELOG.md",
    "Cargo.lock",
    "Cargo.toml",
    "README.md",
    "build.ps1",
    "build.sh",
    "docs/ASSUMPTIONS.md",
    "docs/COMPLIANCE.md",
    "docs/PERFORMANCE.md",
    "docs/REQUIREMENTS.md",
    "docs/SUBMISSION.md",
    "examples/processing_bench.rs",
    "examples/visual_qa.rs",
    "scripts/check_ascii.py",
    "scripts/check_processing_bench.py",
    "scripts/review_package.py",
    "src/algorithm.rs",
    "src/algorithm/bicubic.rs",
    "src/algorithm/color.rs",
    "src/algorithm/quality.rs",
    "src/cli.rs",
    "src/fixtures.rs",
    "src/image.rs",
    "src/io.rs",
    "src/io/ppm.rs",
    "src/io/raw.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/metrics.rs",
    "src/spec.rs",
    "tests/quality_regression.rs",
)
FORBIDDEN_PARTS = {
    ".git",
    "bin",
    "cmodel",
    "dist",
    "generated",
    "out",
    "output",
    "package",
    "pkg",
    "target",
    "temp",
    "tmp",
}
FORBIDDEN_SUFFIXES = {
    ".7z",
    ".bin",
    ".dll",
    ".exe",
    ".gz",
    ".model",
    ".onnx",
    ".pdf",
    ".raw",
    ".so",
    ".tar",
    ".tmp",
    ".zip",
}
MANIFEST_PATTERN = re.compile(r"([0-9a-f]{64})  ([\x20-\x7e]+)")


def fail(message: str) -> None:
    raise ValueError(message)


def validate_path(name: str) -> None:
    try:
        name.encode("ascii")
    except UnicodeEncodeError:
        fail(f"non-ASCII archive path: {name!r}")
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or any(part in ("", ".", "..") for part in path.parts):
        fail(f"unsafe archive path: {name!r}")
    lowered_parts = {part.lower() for part in path.parts}
    if lowered_parts & FORBIDDEN_PARTS:
        fail(f"forbidden archive path: {name!r}")
    if path.suffix.lower() in FORBIDDEN_SUFFIXES:
        fail(f"forbidden archive extension: {name!r}")


def archive_info(name: str) -> zipfile.ZipInfo:
    info = zipfile.ZipInfo(name, FIXED_TIMESTAMP)
    info.compress_type = zipfile.ZIP_STORED
    info.create_system = 3
    info.external_attr = FIXED_MODE << 16
    return info


def manifest_bytes(payloads: dict[str, bytes]) -> bytes:
    lines = [f"{hashlib.sha256(payloads[name]).hexdigest()}  {name}\n" for name in sorted(payloads)]
    return "".join(lines).encode("ascii")


def create_package(output: Path) -> None:
    if output.suffix.lower() != ".zip":
        fail("output path must end in .zip")
    if output.exists():
        fail(f"refusing to overwrite existing package: {output}")
    root = Path(__file__).resolve().parent.parent
    payloads: dict[str, bytes] = {}
    for name in SOURCE_PATHS:
        validate_path(name)
        source = root.joinpath(*PurePosixPath(name).parts)
        if source.is_symlink() or not source.is_file():
            fail(f"required package source is missing or not a regular file: {name}")
        payloads[name] = source.read_bytes()

    output.parent.mkdir(parents=True, exist_ok=True)
    archive_payloads = dict(payloads)
    archive_payloads[MANIFEST_NAME] = manifest_bytes(payloads)
    with zipfile.ZipFile(output, "x", allowZip64=True) as archive:
        for name in sorted(archive_payloads):
            archive.writestr(archive_info(name), archive_payloads[name])
    print(f"Created provisional review package: {output}")


def verify_package(package: Path) -> None:
    if not package.is_file():
        fail(f"package does not exist: {package}")
    expected_names = set(SOURCE_PATHS) | {MANIFEST_NAME}
    with zipfile.ZipFile(package, "r") as archive:
        infos = archive.infolist()
        names = [info.filename for info in infos]
        if len(names) != len(set(names)):
            fail("archive contains duplicate paths")
        if set(names) != expected_names:
            missing = sorted(expected_names - set(names))
            extra = sorted(set(names) - expected_names)
            fail(f"archive membership mismatch; missing={missing}, extra={extra}")
        if names != sorted(names):
            fail("archive paths are not sorted")
        for info in infos:
            validate_path(info.filename)
            if info.is_dir():
                fail(f"directory entry is not allowed: {info.filename}")
            if info.date_time != FIXED_TIMESTAMP:
                fail(f"non-deterministic timestamp: {info.filename}")
            if info.compress_type != zipfile.ZIP_STORED:
                fail(f"unexpected compression method: {info.filename}")
            if info.create_system != 3 or info.external_attr >> 16 != FIXED_MODE:
                fail(f"unexpected file metadata: {info.filename}")

        manifest_text = archive.read(MANIFEST_NAME).decode("ascii")
        manifest: dict[str, str] = {}
        previous = ""
        for line in manifest_text.splitlines():
            match = MANIFEST_PATTERN.fullmatch(line)
            if match is None:
                fail(f"malformed manifest line: {line!r}")
            digest, name = match.groups()
            validate_path(name)
            if name in manifest or name <= previous:
                fail("manifest paths are duplicate or unsorted")
            manifest[name] = digest
            previous = name
        if set(manifest) != set(SOURCE_PATHS):
            fail("manifest membership does not match source payload")
        for name, expected_digest in manifest.items():
            actual_digest = hashlib.sha256(archive.read(name)).hexdigest()
            if actual_digest != expected_digest:
                fail(f"manifest hash mismatch: {name}")
    print(f"Verified provisional review package: {package}")


def usage() -> int:
    print("Usage: review_package.py <create|verify> <package.zip>", file=sys.stderr)
    return 2


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in ("create", "verify"):
        return usage()
    try:
        if sys.argv[1] == "create":
            create_package(Path(sys.argv[2]))
        else:
            verify_package(Path(sys.argv[2]))
    except (OSError, ValueError, zipfile.BadZipFile, UnicodeError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
