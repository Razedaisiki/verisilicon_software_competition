# Windows Build and Submission Candidate Guide

This repository maintains Windows only. The uncompressed TAR contains the two
required evaluation roles: `build.ps1` is the one-click build program, and
`bin/sr.exe` is the precompiled executable used by the evaluation machine for
standard testing. Final packaging still requires real conversation exports and
any organizer-specified archive filename or record schema.

## Prerequisites

- 64-bit Windows.
- Rust 1.85.0 with the `x86_64-pc-windows-msvc` target.
- Python 3 for validation and deterministic TAR creation.
- PowerShell.

The Rust dependency graph is empty. Build and package rebuild operations use
Cargo offline and locked modes.

## Repository build

```text
powershell -ExecutionPolicy Bypass -File .\build.ps1
.\bin\sr.exe --help
```

The script runs Cargo offline, locked, and in release mode for
`x86_64-pc-windows-msvc`. It disables incremental compilation, enables the MSVC
linker's reproducible mode, remaps the source root, and copies the executable
to `bin/sr.exe`.

## Run

```text
.\bin\sr.exe input.ppm output.ppm
.\bin\sr.exe input.raw output.raw
.\bin\sr.exe --raw-rgb8 1920 1080 input.raw output.raw
.\bin\sr.exe --batch input_directory output_directory
```

The two-argument raw path uses fixed 1920x1080 packed RGB888 input and produces
fixed 3840x2160 packed RGB888 output. See `docs/ASSUMPTIONS.md` for the exact
working contract and the remaining written-confirmation dependency.

Batch mode adapts its persistent frame-worker count to available logical
processors and candidate count, with an eight-worker memory cap. Large batches
use serial per-frame pipelines to avoid nested oversubscription. Small batches
use inner channel parallelism only when all candidates fit within the reported
logical parallelism. Output bytes, overwrite policy, and failure reporting
remain deterministic.

## Validate the repository

```text
cargo fmt --all -- --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-targets --all-features
cargo build --locked --release
python scripts/check_ascii.py
python scripts/check_div2k_converter.py
python scripts/check_processing_bench.py
python scripts/check_batch_processing_bench.py
cargo tree --locked --edges normal
```

The expected dependency tree contains only `verisilicon_sr`.

## Create the Windows TAR candidate

First export the complete AI conversation logs to a dedicated directory. The
creator rejects missing, symlinked, or empty log directories and never invents
logs.

To prepare the exact submission directory before the real conversation export
is available, use `stage`. The destination must be named `submit_pkg`; this
creates every fixed payload and an empty `logs/` directory without weakening
the final TAR checks. The staged source allowlist contains only the Cargo
manifest, lockfile, and Rust modules compiled for `sr.exe`; developer examples
and external regression tests stay in the repository and are not submitted.

```text
powershell -ExecutionPolicy Bypass -File .\build.ps1
python scripts/submission_package.py stage submit_pkg --binary bin/sr.exe --target x86_64-pc-windows-msvc
python scripts/submission_package.py create target/submission-candidate.tar --binary bin/sr.exe --logs path/to/exported-ai-logs --target x86_64-pc-windows-msvc
python scripts/submission_package.py verify target/submission-candidate.tar
python scripts/submission_package.py extract target/submission-candidate.tar target/submission-review
powershell -ExecutionPolicy Bypass -File .\target\submission-review\submit_pkg\build.ps1
```

Creation refuses to overwrite an existing archive. The TAR is uncompressed and
uses sorted paths, fixed metadata, safe regular-file entries, English ASCII
source and documents, executable metadata for the Windows binary and build
entry point, and a fixed source allowlist. Verification rejects links, special
entries, unsafe paths, Windows case collisions, unexpected members, non-PE
executables, empty logs, and template changes.

Staging likewise refuses to overwrite an existing directory. It is an
inspection workspace, not a valid final submission until real conversation
exports have been placed under `logs/` and the strict TAR creator succeeds.

The extracted `build.ps1` invokes Cargo with `--offline --locked --release` for
the declared target. It rebuilds from `submit_pkg/src/` and compares every byte
of the rebuilt executable with `submit_pkg/bin/sr.exe`.

The candidate contains:

```text
submit_pkg/
|-- src/
|-- bin/sr.exe
|-- build.ps1
|-- doc/ALGORITHM.md
|-- doc/AI_CODING.md
|-- logs/
`-- README.md
```

## Final reconciliation checklist

- Obtain written acceptance of Rust 1.85 and `x86_64-pc-windows-msvc`.
- Record the required archive filename, result record schema, and AI Coding log
  export format.
- Confirm the exact PPM interpretation and versioned raw RGB888 contract.
- Integrate the official timing API and platform settings when supplied.
- Run official quality, speed, and subjective procedures on the declared host.
- Supply real exported AI conversation logs; never submit the synthetic CI log.
- Keep the hardware-track Cmodel and all local DIV2K assets out of the package.
