# Requirements Traceability

This document separates official software contest requirements from project decisions and provisional assumptions. Official facts come from the software contest PDF. Project decisions define this repository's implementation approach. Provisional assumptions remain replaceable until the committee package supplies exact details.

## Software contest PDF facts

| ID | Requirement | Status | Source | Verification |
| --- | --- | --- | --- | --- |
| PDF-001 | Input is RGB8 imagery, primarily 1920 by 1080. | Confirmed | Software contest PDF | Validate sample type and dimensions before processing. |
| PDF-002 | Output is 2x RGB8 imagery, primarily 3840 by 2160. | Confirmed | Software contest PDF | Verify output type and dimensions. |
| PDF-003 | Supported exchange formats are PPM P6 or committee-defined raw RGB. | Confirmed | Software contest PDF | Add valid, malformed, and truncated format fixtures. |
| PDF-004 | Single-file invocation is `./sr <input> <output>`. | Confirmed | Software contest PDF | Exercise the exact command form in acceptance tests. |
| PDF-005 | Batch invocation is `./sr --batch <in_dir> <out_dir>`. | Confirmed | Software contest PDF | Test directory processing, output creation, and failure reporting. |
| PDF-006 | Repeated runs with identical inputs and configuration must produce byte-identical output. | Confirmed | Software contest PDF | Hash outputs from repeated and cross-thread-count runs. |
| PDF-007 | The submission must build offline with a one-click build procedure. | Confirmed | Software contest PDF | Build in a clean environment with network access disabled. |
| PDF-008 | Processing is CPU-only. Threading and SSE, AVX, or NEON CPU instructions are allowed. | Confirmed | Software contest PDF | Audit build features and run on declared CPU targets. |
| PDF-009 | Mature image-processing libraries are prohibited in the super-resolution processing chain. | Confirmed | Software contest PDF | Review the dependency graph and processing call path. |
| PDF-010 | Neural networks, pretrained models, and runtime model files are prohibited. | Confirmed | Software contest PDF | Audit source, package contents, and runtime file access. |
| PDF-011 | Scoring includes objective quality, subjective quality, speed, and AI Coding evaluation. | Confirmed | Software contest PDF | Map evaluation evidence to all four scoring categories. |
| PDF-012 | Admission requires at least 1 frame per second and quality at least equal to the prescribed bicubic baseline. | Confirmed | Software contest PDF | Run the official benchmark and quality comparison when supplied. |
| PDF-013 | The submission is an uncompressed TAR containing `submit_pkg/src/`, precompiled evaluation executable `bin/sr.exe`, a one-click build entry (`build.sh`, Makefile, or CMake), `doc/ALGORITHM.md`, `doc/AI_CODING.md`, `logs/`, and `README.md`. The Windows package selects `CMakeLists.txt`. | Confirmed | Software contest PDF page 10 and Windows platform interpretation | Validate archive type, exact directory roles, rebuildability, records, and AI Coding logs. |

## Organizer-confirmed evaluation facts

| ID | Requirement | Status | Source | Verification |
| --- | --- | --- | --- | --- |
| ORG-001 | Objective quality consists of mean per-image Y-PSNR and mean per-image Y-SSIM, weighted 50 percent each. | Confirmed | User-supplied organizer clarification | Report the two dataset means separately until cross-metric normalization is specified. |
| ORG-002 | Every test image has equal weight in both objective dataset averages. | Confirmed | User-supplied organizer clarification | Average unrounded per-image values; do not pool pixels or SSIM windows across images. |

The exact local paired HR/LR development contract and its boundary from the
organizer implementation are recorded in `docs/EVALUATION.md`.

## Project decisions

These choices are repository policy, not claims about the contest PDF.

| ID | Decision | Status | Source | Verification |
| --- | --- | --- | --- | --- |
| PRJ-001 | Implement the solution in Rust. | Adopted | Project milestone brief | Build with the pinned Rust toolchain. |
| PRJ-002 | Prefer the Rust standard library and keep runtime dependencies at zero unless a reviewed need is demonstrated. | Adopted | Project milestone brief | Review `Cargo.lock` and document any exception. |
| PRJ-003 | Keep repository-authored documentation, command help, logs, and commit messages in English ASCII. | Adopted | Project milestone brief | Run an ASCII validation over tracked text artifacts. |
| PRJ-004 | Record each implementation milestone in `CHANGELOG.md`. | Adopted | Project milestone brief | Require an Unreleased entry during review. |
| PRJ-005 | Use small atomic commits and GitHub Actions checks. | Adopted | Project milestone brief | Review commit scope and require format, lint, test, and build jobs. |
| PRJ-006 | Preserve a deterministic scalar implementation as the correctness oracle for optimized CPU paths. | Adopted | Engineering decision | Compare optimized output byte-for-byte with scalar output. |
| PRJ-007 | Maintain build, CI, and package execution for Windows only. | Adopted | User platform decision | Run the pinned MSVC target in Windows CI; keep the package CMake entry and precompiled `bin/sr.exe`. |
| PRJ-008 | Keep paired DIV2K preparation offline, local-only, split-explicit, hash-locked, and separate from Eval30 and input-only test images; default to the current 100-image validation workflow. | Adopted | Dataset preparation milestone | Validate the selected exact ID set, RGB8 PNG, HR=2xLR, deterministic PPM and manifests, atomic publication, and synthetic validation/train/all failures. |

## Provisional assumptions

These interfaces are planning defaults, not official facts. Their definitions and replacement boundaries are maintained in `docs/ASSUMPTIONS.md`.

| ID | Assumption | Status | Source |
| --- | --- | --- | --- |
| A-001 | PPM P6 uses 8-bit samples with `maxval` 255. | Provisional | Project planning |
| A-002 | Official raw is fixed 1920x1080 packed row-major RGB888 in R/G/B order with top-to-bottom rows and no header or padding; output is fixed 3840x2160 in the same layout. | Confirmed working contract | Direct organizer clarification; written package record pending |
| A-003 | The project pipeline uses BT.601 full-range fixed-point color conversion. | Provisional | Project planning |
| A-004 | The public processing scale is fixed at 2x. | Provisional | Software scope |
| A-005 | The local bicubic development anchor uses Catmull-Rom parameter `a = -0.5` without claiming organizer equivalence. | Project-local | Project planning and organizer clarification that no coefficient file will follow |
| A-006 | Initial performance measurement uses a processing-only timing boundary. | Provisional | Project planning |
| A-007 | Batch mode is non-recursive, deterministically ordered, and refuses existing outputs. | Provisional | Project planning |
| A-008 | The existing luma PSNR and global luma SSIM remain legacy synthetic diagnostics; the paired HR/LR local-window contract is documented separately. | Provisional | Project planning and `docs/EVALUATION.md` |
| A-009 | The organizer-recommended Y-nearest-plus-convolution and UV-bilinear architecture uses explicitly project-selected fixed-point details until numerical parameters are supplied. | Project-local | User-supplied organizer reference slide |

## Missing official package details

The following details must be replaced with versioned committee data before final acceptance:

- Versioned written confirmation of the raw working contract, including file
  naming and any metadata requirements outside the packed payload.
- Official timing API, measurement boundary, warm-up procedure, host platform, CPU configuration, thread policy, and compiler settings.
- Official image dataset, exact Y/PSNR/SSIM numerical rules, cross-metric normalization, thresholds, and subjective review procedure.
- Archive filename, result record, and AI Coding log schema details.

When the official package is available, add its version, filename, page or section locator, and exact acceptance evidence to the relevant row before freezing an interface.
