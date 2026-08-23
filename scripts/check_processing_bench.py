#!/usr/bin/env python3
"""Exercise deterministic benchmark policies without enforcing wall-clock timing."""

from __future__ import annotations

import subprocess
import sys


EXPECTED_CHECKSUMS = {
    "baseline": "65b8c09ec62e070b",
    "recommended": "1a4040a279219597",
    "quality": "1211042eb00b138c",
    "selected-ungated": "6ee8bc73b89869d2",
    "confidence-gated": "7896b83e18e630ba",
    "bilinear-chroma": "d7cfb61c1ac3f05d",
}


def run_case(mode: str, policy: str) -> dict[str, str]:
    command = [
        "cargo",
        "run",
        "--quiet",
        "--locked",
        "--release",
        "--example",
        "processing_bench",
        "--",
        mode,
        policy,
        "8",
        "5",
        "1",
    ]
    completed = subprocess.run(
        command,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
    )
    fields: dict[str, str] = {}
    for line in completed.stdout.splitlines():
        key, separator, value = line.partition("=")
        if separator:
            fields[key] = value
    return fields


def main() -> int:
    for mode, expected_checksum in EXPECTED_CHECKSUMS.items():
        observed: dict[str, str] = {}
        for policy in ("serial", "parallel"):
            fields = run_case(mode, policy)
            expected = {
                "mode": mode,
                "requested_policy": policy,
                "selected_policy": policy,
                "input": "8x5",
                "output": "16x10",
                "checksum": expected_checksum,
            }
            for key, value in expected.items():
                if fields.get(key) != value:
                    print(
                        f"benchmark check failed: {mode}/{policy} expected "
                        f"{key}={value}, received {fields.get(key)!r}",
                        file=sys.stderr,
                    )
                    return 1
            observed[policy] = fields["checksum"]
        if observed["serial"] != observed["parallel"]:
            print(
                f"benchmark check failed: {mode} serial and parallel checksums differ",
                file=sys.stderr,
            )
            return 1

    print("Processing benchmark policy check passed without timing gates.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
