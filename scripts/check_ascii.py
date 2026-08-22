#!/usr/bin/env python3
"""Reject non-ASCII bytes in repository-authored text files."""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


TEXT_SUFFIXES = {
    ".md",
    ".py",
    ".rs",
    ".sh",
    ".toml",
    ".txt",
    ".yaml",
    ".yml",
}
TEXT_NAMES = {".gitignore"}


def repository_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        check=True,
        stdout=subprocess.PIPE,
    )
    return [Path(name) for name in result.stdout.decode("utf-8").split("\0") if name]


def is_repository_text(path: Path) -> bool:
    return path.name in TEXT_NAMES or path.suffix.lower() in TEXT_SUFFIXES


def main() -> int:
    failures: list[str] = []
    checked = 0

    for path in repository_files():
        if not is_repository_text(path):
            continue
        checked += 1
        data = path.read_bytes()
        if any(byte > 0x7F for byte in data):
            failures.append(f"{path}: contains non-ASCII bytes")
        if b"\0" in data:
            failures.append(f"{path}: contains NUL bytes")

    if failures:
        print("ASCII policy check failed:", file=sys.stderr)
        for failure in failures:
            print(f"- {failure}", file=sys.stderr)
        return 1

    print(f"ASCII policy check passed for {checked} text files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
