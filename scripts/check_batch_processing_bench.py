#!/usr/bin/env python3
"""Check batch worker and inner-policy determinism without a timing gate."""

from __future__ import annotations

import subprocess
import sys


EXPECTED_CHECKSUM = "a0582bd112492615"
CASES = ((1, "serial"), (2, "serial"), (2, "parallel"), (4, "serial"))


def run_case(workers: int, policy: str) -> dict[str, str]:
    command = [
        "cargo",
        "run",
        "--quiet",
        "--locked",
        "--release",
        "--example",
        "batch_processing_bench",
        "--",
        policy,
        str(workers),
        "8",
        "5",
        "3",
        "2",
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
    for workers, policy in CASES:
        fields = run_case(workers, policy)
        expected = {
            "requested_frame_workers": str(workers),
            "requested_inner_policy": policy,
            "input": "8x5",
            "output": "16x10",
            "frames_per_batch": "3",
            "warmup_batch_iterations": "1",
            "measured_batch_iterations": "2",
            "measured_frames": "6",
            "checksum": EXPECTED_CHECKSUM,
        }
        for key, value in expected.items():
            if fields.get(key) != value:
                print(
                    f"batch benchmark check failed: {workers}/{policy} expected "
                    f"{key}={value}, received {fields.get(key)!r}",
                    file=sys.stderr,
                )
                return 1

        peak_text = fields.get("peak_simultaneous_frames", "")
        if not peak_text.isdigit() or not 1 <= int(peak_text) <= min(workers, 3):
            print(
                f"batch benchmark check failed: invalid peak count {peak_text!r}",
                file=sys.stderr,
            )
            return 1

    print("Batch processing benchmark checksum check passed without timing gates.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
