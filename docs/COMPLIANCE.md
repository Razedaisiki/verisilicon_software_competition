# Compliance Audit

This audit covers the repository state against `docs/REQUIREMENTS.md` and
assumptions A-001 through A-009. `Implemented` means repository evidence exists
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
| PDF-005 exact `./sr --batch <in_dir> <out_dir>` | Provisional | `src/cli.rs` implements and tests deterministic mixed `.ppm`, `.raw`, and `.rgb` discovery, adaptive persistent frame workers, byte-exact batch-versus-single output, ordered partial failures, output creation, and no-candidate handling; A-007 records provisional semantics. | Official recursion, overwrite, ordering, extension, failure, worker, and timing-interface policy. |
| PDF-006 byte-identical repeated output | Implemented | Bicubic and frozen-quality tests compare repeated exact `Image` equality; the selected pipeline matches its explicit 64/2/24/80 path and forced serial/parallel outputs exactly; retained scalar oracles cover constant, gradient, edge, checker, odd, thin, and 1x1 inputs; `scripts/check_processing_bench.py` checks fixed serial/parallel checksums. | Official input corpus and any required cross-platform reference hashes. |
| PDF-007 offline one-click build | Implemented | Zero dependency graph in `Cargo.toml` and `Cargo.lock`; the package `CMakeLists.txt` runs Cargo offline and locked, and compares the rebuilt executable with `bin/sr.exe`; Windows CI rebuilds the packaged source and requires byte identity. | Official availability of the declared Rust/MSVC/CMake environment. |
| PDF-008 CPU-only; threading and SIMD allowed | Implemented | `src/algorithm.rs` uses bounded standard-library CPU threads; processing modules contain CPU integer code; `docs/PERFORMANCE.md` documents dispatch. No GPU API or dependency exists. | Official CPU, core/thread policy, compiler flags, and target architecture. |
| PDF-009 no mature image-processing libraries | Implemented | `[dependencies]` is empty; `cargo tree --locked --edges normal` contains only `verisilicon_sr`; codecs, color conversion, scaling, and metrics are repository code under `src/`. | Committee interpretation of prohibited tooling, including build-time tools if separately constrained. |
| PDF-010 no neural networks/models/runtime model files | Implemented | No model loader or inference path exists; the fixed package source allowlist contains only ASCII Rust/TOML text; `.gitignore` excludes Cmodel and generated assets. | Official audit procedure and any additional forbidden filename rules. |
| PDF-011 quality, subjective, speed, AI Coding scoring | Blocked | `docs/EVALUATION.md` records the confirmed equal-image mean Y-PSNR and mean Y-SSIM categories and their 50/50 weights. The local paired evaluator and locked Eval30 database provide reproducible development evidence. | Official dataset, exact metric numerics, cross-metric normalization, thresholds, subjective procedure, result schema, and AI Coding log format. |
| PDF-012 at least 1 FPS and quality at least bicubic | Blocked | Local processing-only 1920x1080 evidence in `docs/PERFORMANCE.md` has medians above 1 FPS; the project-local Catmull-Rom anchor is fully specified in `docs/ALGORITHM.md`; quality regression tests avoid organizer-equivalence or superiority claims. | Official platform, timing API/boundary, evaluation dataset, organizer comparison outputs, and quality threshold calculation. |
| PDF-013 required submission structure and records | Provisional | `scripts/submission_package.py` creates, verifies, and safely extracts a deterministic uncompressed TAR with source, an organizer-listed CMake build entry, the precompiled evaluation executable `bin/sr.exe`, algorithm and AI Coding documents, external logs, and README. Windows CI checks repeated byte-identical TAR creation and offline binary rebuild identity. | Archive filename and any additional result/log schema details. |
| Rust language acceptability | Blocked | `Cargo.toml` pins `rust-version = "1.85"`; Windows CI validates Rust 1.85 and the `x86_64-pc-windows-msvc` target; all runtime code is standard-library Rust. | Explicit committee confirmation that Rust binaries and the required toolchain are accepted in the submission environment. |
| Baseline versus quality selection | Provisional | Public CLI paths select accepted `SelectedQualityPipeline` parameters 64/2/24/80. The prior 64/2/32/64 selection remains a frozen fine-sweep and rejected-experiment anchor; `fine-finalist` aliases the promoted selection. Eval30 selected 64/2/24/80 for both metrics in every fold, and the one-candidate 100-image DIV2K comparison improved over 64/2/32/64 by +0.051761 dB Y-PSNR and +0.000836439 Y-SSIM, with both metrics positive on every image. The recommended, confidence-gated, and bilinear-chroma experiments were rejected. | Official baseline definition and evidence that the selected candidate meets the admission comparison. |
| Hardware-track Cmodel exclusion | Implemented | No Cmodel source, binary, archive, output, or integration exists; `.gitignore` and package verification forbid Cmodel artifacts. The previously received Cmodel matched the hardware track and was removed by project decision. | Reassess only if the official software package explicitly identifies a distinct software-track asset. |

## Project decision evidence matrix

| Decision | Status | Exact repository evidence | Remaining review |
| --- | --- | --- | --- |
| PRJ-001 implement in Rust | Implemented | `Cargo.toml` declares Rust 2024 and Rust 1.85; all runtime source is under `src/*.rs`; CI pins 1.85.0. | Confirm Rust is permitted by the committee environment. |
| PRJ-002 prefer standard library and zero runtime dependencies | Implemented | `[dependencies]` is empty; `Cargo.lock` contains only this package; `cargo tree --locked --edges normal` is a single node. | Re-audit if any dependency is proposed. |
| PRJ-003 English ASCII repository content | Implemented | `scripts/check_ascii.py` covers repository text including Rust, Markdown, Python, shell, PowerShell, TOML, and YAML; CI runs it. | Official supplied files may be non-ASCII and must remain local unless packaging rules require otherwise. |
| PRJ-004 record milestones in changelog | Implemented | `CHANGELOG.md` contains Unreleased entries for implementation, optimization, evaluation, and packaging milestones. | Move entries only during an authorized release. |
| PRJ-005 small commits and GitHub Actions | Implemented | `.github/workflows/ci.yml` runs Windows format, lint, test, build, dependency, converter, benchmark, package, extraction, and rebuild checks; commit atomicity remains a review-time source-control check. | Primary reviewer must review and create commits; this audit does not commit or push. |
| PRJ-006 deterministic scalar correctness oracle | Implemented | `scale_plane_2x_reference` and pipeline reference paths in `src/algorithm/bicubic.rs` and `src/algorithm/quality.rs` are test-only oracles; forced serial/parallel equality covers the frozen pipelines, and the selected wrapper is checked against its explicit parameters under both policies. | Compare against official reference outputs when supplied. |
| PRJ-007 Windows-only maintenance | Implemented | CI has one `windows-latest` job; package `CMakeLists.txt` provides one-click reconstruction, and `bin/sr.exe` provides the precompiled standard-test executable. | Re-audit only if the evaluation platform changes. |
| PRJ-008 offline locked DIV2K pairs | Implemented | `scripts/div2k_pairs.py`, `prepare_div2k.ps1`, and `scripts/check_div2k_pairs.py` default to the selected 100 validation pairs 0801-0900 and optionally enforce train 0001-0800 or both. They enforce exact selected membership, strict RGB8 PNG, HR=2xLR, deterministic split-only PPM and SHA-256 manifests, safe paths, atomic publication, and overwrite refusal. `evaluation/div2k/local/` is ignored; Eval30 and input-only test IDs 0901-1000 remain separate. | Locally verify the genuine validation archives used by the current optimization; do not treat this development corpus as the organizer's hidden evaluation set. |

## Assumption evidence matrix

| Assumption | Status | Exact repository evidence | Replacement dependency |
| --- | --- | --- | --- |
| A-001 PPM P6 maxval 255, one byte/sample | Provisional | `src/io/ppm.rs` enforces the rule and tests comments, CRLF, malformed headers, lengths, and leading raster bytes; README documents it. | Committee PPM rules and reference files. |
| A-002 packed row-major RGB8 raw | Confirmed working contract | `src/spec.rs` fixes 1920x1080 input, 3840x2160 output, 6,220,800 input bytes, and 24,883,200 output bytes; `src/io/raw.rs` and `src/cli.rs` enforce the contract; tests cover full-size raw, a leading `P6` byte sequence, uppercase extensions, exact output size, and malformed lengths. The diagnostic `--raw-rgb8` form remains dimension-explicit. | Obtain a versioned written organizer record and confirm official filename and metadata conventions. |
| A-003 BT.601 full-range fixed-point color | Provisional | Coefficients, rounding, clipping, and fixed vectors are in `src/algorithm/color.rs`; both pipelines call this module. | Official color space, range, coefficients, and reference vectors. |
| A-004 fixed 2x scale | Provisional | `src/spec.rs` exposes only `Scale::X2`; configuration and algorithm tests check exact output dimensions and overflow. | Confirmation that no other scale or dimension policy is required. |
| A-005 Catmull-Rom a=-0.5 bicubic | Project-local | Exact Q7 phase weights, half-pixel mapping, borders, Q14 rounding, oracle, and tests are in `src/algorithm/bicubic.rs`; `docs/ALGORITHM.md` records coefficient origin and denies organizer equivalence. | No coefficient file is expected; compare against organizer evaluation evidence when available. |
| A-006 processing-only timing | Provisional | `examples/processing_bench.rs` places `Instant` only around algorithm calls after warm-up; `docs/PERFORMANCE.md` records raw runs and limitations. | Official timing API, process/I/O boundary, warm-up, repetitions, CPU controls, and compiler settings. |
| A-007 deterministic non-recursive non-overwriting batch | Provisional | Batch coordinator and ordered mixed-format `.ppm`, `.raw`, and `.rgb` discovery tests are in `src/cli.rs`; README and `docs/ASSUMPTIONS.md` state all current rules. | Official batch discovery, recursion, overwrite, ordering, failure, and reporting behavior. |
| A-008 legacy and paired luma diagnostics | Provisional | Formulas and fixed tests are in `src/metrics.rs`; `docs/EVALUATION.md` separates legacy global SSIM from the implemented valid-window paired contract. | Official Y transform, metric numerics, SSIM windows and borders, cross-metric normalization, dataset, thresholds, and result format. |
| A-009 recommended architecture numerical details | Project-local | `src/algorithm/recommended.rs` isolates the fixed-point V1 implementation and oracle tests; evaluator selection is explicit and the public runtime remains the separately selected ungated quality candidate. | Organizer YUV, convolution, interpolation, border, rounding, and clipping details if supplied. |

## Repository-wide controls

- The runtime dependency graph is empty and processing is CPU-only.
- No tracked PDF, Cmodel artifact, model, generated result, binary, or archive
  is permitted in the package source tree.
- Repository-authored text is checked by `scripts/check_ascii.py`.
- Real DIV2K sources and generated pairs remain under the ignored
  `evaluation/div2k/local/` workspace; only the offline preparation contract is
  tracked.
- CI runs Rust 1.85 formatting, lint, tests, release build, deterministic
  benchmark checksum checks, deterministic TAR creation, safe extraction, and
  packaged offline rebuild identity on Windows.
- Real exported AI logs are required at package creation and are never checked
  into the repository. The CI-only synthetic log is not submission evidence.

## Blocking official inputs

Final compliance remains blocked on the versioned official dataset and
comparison evidence, a versioned written record of the organizer-confirmed raw
contract, timing API and platform, exact metric numerics, cross-metric
normalization and thresholds, archive
filename, result record schema, AI Coding log format, and confirmation that Rust 1.85 is
accepted. No additional bicubic coefficient file is expected. The removed
hardware-track Cmodel is not treated as a software-track dependency.
