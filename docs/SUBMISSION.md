# Provisional Build and Submission Guide

This guide describes the current source review package. It is not the official
contest submission procedure. Reconcile every checklist item below when the
committee package arrives.

## Prerequisites

- Rust 1.85.0 with Cargo installed locally.
- Python 3 for repository validation and provisional packaging.
- No network access is needed after the Rust toolchain is installed because the
  Rust dependency graph is empty.

## One-click release build

Linux or another Unix-like shell:

```text
sh build.sh
./bin/sr --help
```

Windows PowerShell:

```text
powershell -ExecutionPolicy Bypass -File .\build.ps1
.\bin\sr.exe --help
```

Both scripts run `cargo build --locked --release` and copy the resulting binary
under ignored `bin/`.

## Run

Linux:

```text
./bin/sr input.ppm output.ppm
./bin/sr --raw-rgb8 1920 1080 input.raw output.raw
./bin/sr --batch input_directory output_directory
```

Windows PowerShell:

```text
.\bin\sr.exe input.ppm output.ppm
.\bin\sr.exe --raw-rgb8 1920 1080 input.raw output.raw
.\bin\sr.exe --batch input_directory output_directory
```

The raw layout and batch behavior remain provisional assumptions A-002 and
A-007. The two-argument PPM command uses `BicubicBaseline`.

## Test and audit

Run on either platform from the repository root:

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
python scripts/check_ascii.py
python scripts/check_processing_bench.py
cargo tree --locked --edges normal
```

The expected dependency tree contains only `verisilicon_sr`.

## Provisional evaluation

Generate synthetic visual artifacts in a new ignored directory:

```text
cargo run --locked --release --example visual_qa -- target/visual_qa
```

Run processing-only diagnostics with an explicit pipeline and policy:

```text
cargo run --locked --release --example processing_bench -- baseline auto 1920 1080 3
cargo run --locked --release --example processing_bench -- quality auto 1920 1080 3
```

These metrics and timings are local diagnostics. They are not official scores
or admission evidence.

## Create and verify the provisional review package

Choose explicit unused output paths under ignored `target/`:

```text
python scripts/review_package.py create target/review-package.zip
python scripts/review_package.py verify target/review-package.zip
```

Creation refuses to overwrite an existing ZIP. The source allowlist is fixed in
`scripts/review_package.py`. Archive paths are sorted ASCII relative paths with
fixed timestamps and permissions and no compression. `MANIFEST.sha256` records
the SHA-256 digest of every source payload. Verification checks exact
membership, hashes, safe paths, ordering, duplicate paths, fixed metadata, and
forbidden names and extensions.

The package excludes PDFs, Cmodel data, models, binaries, raw images, generated
results, `target/`, `bin/`, `.git/`, local configuration, and secrets. Do not
rename this ZIP to an official submission or add results until the committee
template and record formats are known.

## Official-package reconciliation checklist

- Record the committee package version, filenames, and section/page locators.
- Confirm Rust 1.85 and the intended target triples are accepted.
- Replace A-001 and A-002 with official PPM and raw layout rules and fixtures.
- Replace A-003 with the official color conversion if one is prescribed.
- Confirm the public scale and accepted dimensions currently covered by A-004.
- Replace A-005 with exact bicubic coefficients, coordinates, borders, rounding,
  and reference hashes.
- Replace A-006 with the official timing API, boundary, warm-up, repetition,
  compiler, CPU, and thread settings.
- Replace A-007 with official batch discovery, overwrite, failure, and reporting
  rules.
- Add the official metrics and dataset as a distinct path; do not relabel A-008
  diagnostics as official.
- Run official baseline equality, quality admission, speed, and subjective
  procedures on the declared platform.
- Keep the removed hardware-track Cmodel excluded. Reassess only if the official
  software package explicitly identifies a distinct software-track asset.
- Replace the provisional ZIP allowlist and archive name with the official
  directory template and filename rules.
- Add required result records, notices, AI Coding logs, and validation output in
  their exact official schemas.
- Re-run the complete compliance matrix and remove every `Blocked` status only
  when versioned official evidence exists.
