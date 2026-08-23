# Compliance Audit

This audit covers the repository state against `docs/REQUIREMENTS.md` and
assumptions A-001 through A-008. `Implemented` means repository evidence exists
for the currently known rule. `Provisional` means the implementation is usable
but depends on an unconfirmed project assumption. `Blocked` means official
package information or assets are required. No provisional row is an official
compliance claim.

## Requirement evidence matrix

| Requirement | Status | Exact repository evidence | Remaining official-package dependency |
| --- | --- | --- | --- |
| PDF-001 RGB8 input, primarily 1920x1080 | Implemented | `src/image.rs` defines `Rgb8`; `src/io/ppm.rs` and `src/io/raw.rs` validate exact raster lengths; `src/spec.rs` fixes official raw input at 1920x1080 RGB888 (6,220,800 bytes); CLI tests cover PPM and full-size raw decoding. | Official dataset, accepted PPM dimension variants, and a versioned written record of the organizer-confirmed raw contract. |
| PDF-002 2x RGB8 output, primarily 3840x2160 | Implemented | `src/spec.rs` fixes public `Scale::X2` and checks overflow; algorithm and CLI tests assert exact doubled dimensions. | Official reference outputs and any permitted exceptional dimensions. |
| PDF-003 PPM P6 or committee raw RGB | Provisional | Strict PPM implementation and malformed/truncated tests are in `src/io/ppm.rs`; `src/spec.rs`, `src/io/raw.rs`, and `src/cli.rs` implement the organizer-confirmed 1920x1080 packed row-major RGB888 contract with exact 6,220,800-byte input and 24,883,200-byte output. Extension-first routing and content detection are tested. | Exact committee PPM interpretation and a versioned written raw contract, including official filename and metadata conventions. |
| PDF-004 exact `./sr <input> <output>` | Implemented | `src/cli.rs` parses the two-argument form; tests exercise PPM, full-size fixed raw, extension precedence, output-suffix independence, ambiguity, and invalid lengths; `src/main.rs` exposes binary `sr`; README lists the exact command. | Official acceptance fixture and platform invocation environment. |
| PDF-005 exact `./sr --batch <in_dir> <out_dir>` | Provisional | `src/cli.rs` implements and tests deterministic mixed `.ppm`, `.raw`, and `.rgb` discovery, output creation, partial failures, and no-candidate handling; A-007 records provisional semantics. | Official recursion, overwrite, ordering, extension, and failure policy. |
| PDF-006 byte-identical repeated output | Implemented | Bicubic and quality tests compare repeated exact `Image` equality; forced serial and parallel paths match the retained scalar oracle across constant, gradient, edge, checker, odd, thin, and 1x1 inputs; `scripts/check_processing_bench.py` checks fixed serial/parallel checksums. | Official input corpus and any required cross-platform reference hashes. |
| PDF-007 offline one-click build | Implemented | Zero dependency graph in `Cargo.toml` and `Cargo.lock`; `build.sh` and `build.ps1` run `cargo build --locked --release` and copy the binary; CI builds on Linux and Windows. | Official definition of offline environment, preinstalled Rust toolchain, and mandated build entry-point name. |
| PDF-008 CPU-only; threading and SIMD allowed | Implemented | `src/algorithm.rs` uses bounded standard-library CPU threads; processing modules contain CPU integer code; `docs/PERFORMANCE.md` documents dispatch. No GPU API or dependency exists. | Official CPU, core/thread policy, compiler flags, and target architecture. |
| PDF-009 no mature image-processing libraries | Implemented | `[dependencies]` is empty; `cargo tree --locked --edges normal` contains only `verisilicon_sr`; codecs, color conversion, scaling, and metrics are repository code under `src/`. | Committee interpretation of prohibited tooling, including build-time tools if separately constrained. |
| PDF-010 no neural networks/models/runtime model files | Implemented | No model loader or inference path exists; package allowlist rejects model/binary/archive artifacts; `.gitignore` excludes Cmodel and generated assets. | Official audit procedure and any additional forbidden filename rules. |
| PDF-011 quality, subjective, speed, AI Coding scoring | Blocked | `src/metrics.rs`, `tests/quality_regression.rs`, `examples/visual_qa.rs`, and `examples/processing_bench.rs` provide provisional local diagnostics. | Official dataset, metrics, weights, subjective procedure, result schema, and AI Coding log format. |
| PDF-012 at least 1 FPS and quality at least bicubic | Blocked | Local processing-only 1920x1080 evidence in `docs/PERFORMANCE.md` has medians above 1 FPS; quality regression tests avoid superiority claims; CLI intentionally selects `BicubicBaseline`, while `QualityPipeline` is opt-in. | Official platform, timing API/boundary, baseline coefficients/reference, dataset, and quality threshold calculation. |
| PDF-013 required submission structure and records | Blocked | `scripts/review_package.py` creates a deterministic provisional source review ZIP; `docs/SUBMISSION.md` documents current build, run, evaluation, and package steps. | Official directory template, archive name, result records, AI Coding logs, licenses/notices, and submission validation tool. |
| Rust language acceptability | Blocked | `Cargo.toml` pins `rust-version = "1.85"`; Linux/Windows CI validates Rust 1.85; all runtime code is standard-library Rust. | Explicit committee confirmation that Rust binaries and the required toolchain are accepted in the submission environment. |
| Baseline versus quality selection | Provisional | Public CLI paths select `BicubicBaseline` in `src/cli.rs`; `QualityPipeline` remains an opt-in library path and is covered by exact deterministic tests. | Official baseline definition and evidence that any quality candidate meets the admission comparison. |
| Hardware-track Cmodel exclusion | Implemented | No Cmodel source, binary, archive, output, or integration exists; `.gitignore` and package verification forbid Cmodel artifacts. The previously received Cmodel matched the hardware track and was removed by project decision. | Reassess only if the official software package explicitly identifies a distinct software-track asset. |

## Project decision evidence matrix

| Decision | Status | Exact repository evidence | Remaining review |
| --- | --- | --- | --- |
| PRJ-001 implement in Rust | Implemented | `Cargo.toml` declares Rust 2024 and Rust 1.85; all runtime source is under `src/*.rs`; CI pins 1.85.0. | Confirm Rust is permitted by the committee environment. |
| PRJ-002 prefer standard library and zero runtime dependencies | Implemented | `[dependencies]` is empty; `Cargo.lock` contains only this package; `cargo tree --locked --edges normal` is a single node. | Re-audit if any dependency is proposed. |
| PRJ-003 English ASCII repository content | Implemented | `scripts/check_ascii.py` covers repository text including Rust, Markdown, Python, shell, PowerShell, TOML, and YAML; CI runs it. | Official supplied files may be non-ASCII and must remain local unless packaging rules require otherwise. |
| PRJ-004 record milestones in changelog | Implemented | `CHANGELOG.md` contains Unreleased entries for implementation, optimization, evaluation, and packaging milestones. | Move entries only during an authorized release. |
| PRJ-005 small commits and GitHub Actions | Implemented | `.github/workflows/ci.yml` runs a Linux/Windows validation matrix; commit atomicity remains a review-time source-control check. | Primary reviewer must review and create commits; this audit does not commit or push. |
| PRJ-006 deterministic scalar correctness oracle | Implemented | `scale_plane_2x_reference` and pipeline reference paths in `src/algorithm/bicubic.rs` and `src/algorithm/quality.rs` are test-only oracles; forced serial/parallel equality tests cover both pipelines. | Compare against official reference outputs when supplied. |

## Assumption evidence matrix

| Assumption | Status | Exact repository evidence | Replacement dependency |
| --- | --- | --- | --- |
| A-001 PPM P6 maxval 255, one byte/sample | Provisional | `src/io/ppm.rs` enforces the rule and tests comments, CRLF, malformed headers, lengths, and leading raster bytes; README documents it. | Committee PPM rules and reference files. |
| A-002 packed row-major RGB8 raw | Confirmed working contract | `src/spec.rs` fixes 1920x1080 input, 3840x2160 output, 6,220,800 input bytes, and 24,883,200 output bytes; `src/io/raw.rs` and `src/cli.rs` enforce the contract; tests cover full-size raw, a leading `P6` byte sequence, uppercase extensions, exact output size, and malformed lengths. The diagnostic `--raw-rgb8` form remains dimension-explicit. | Obtain a versioned written organizer record and confirm official filename and metadata conventions. |
| A-003 BT.601 full-range fixed-point color | Provisional | Coefficients, rounding, clipping, and fixed vectors are in `src/algorithm/color.rs`; both pipelines call this module. | Official color space, range, coefficients, and reference vectors. |
| A-004 fixed 2x scale | Provisional | `src/spec.rs` exposes only `Scale::X2`; configuration and algorithm tests check exact output dimensions and overflow. | Confirmation that no other scale or dimension policy is required. |
| A-005 Catmull-Rom a=-0.5 bicubic | Provisional | Exact Q7 phase weights, half-pixel mapping, borders, Q14 rounding, oracle, and tests are in `src/algorithm/bicubic.rs`; README documents them. | Official coefficients, coordinates, border handling, rounding, and reference output. |
| A-006 processing-only timing | Provisional | `examples/processing_bench.rs` places `Instant` only around algorithm calls after warm-up; `docs/PERFORMANCE.md` records raw runs and limitations. | Official timing API, process/I/O boundary, warm-up, repetitions, CPU controls, and compiler settings. |
| A-007 deterministic non-recursive non-overwriting batch | Provisional | Batch coordinator and ordered mixed-format `.ppm`, `.raw`, and `.rgb` discovery tests are in `src/cli.rs`; README and `docs/ASSUMPTIONS.md` state all current rules. | Official batch discovery, recursion, overwrite, ordering, failure, and reporting behavior. |
| A-008 provisional luma PSNR/global SSIM | Provisional | Formulas and fixed tests are in `src/metrics.rs`; regression use is in `tests/quality_regression.rs`; visual reporting is in `examples/visual_qa.rs`. | Official metrics, windows, color transform, dataset, thresholds, and result format. |

## Repository-wide controls

- The runtime dependency graph is empty and processing is CPU-only.
- No tracked PDF, Cmodel artifact, model, generated result, binary, tarball, or
  ZIP is permitted in the provisional review package.
- Repository-authored text is checked by `scripts/check_ascii.py`.
- CI runs Rust 1.85 formatting, lint, tests, release build, deterministic
  benchmark checksum checks, package creation, and package verification on
  Linux and Windows.
- The review ZIP is deliberately non-official. Its source-only membership must
  be reconciled with the committee submission template before use.

## Blocking official inputs

Final compliance remains blocked on the versioned official dataset, bicubic
baseline coefficients and reference outputs, a versioned written record of the
organizer-confirmed raw contract, timing API and
platform, metric definitions and thresholds, submission directory and archive
naming template, result record schema, AI Coding log format, and confirmation
that Rust 1.85 is accepted. The removed hardware-track Cmodel is not treated as
a software-track dependency.
