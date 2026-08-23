# Changelog

All notable project changes are recorded in this file.

## Unreleased

### Added

- Added the English ASCII-only repository foundation.
- Added requirement traceability and centralized provisional assumptions.
- Added ignore rules for official assets, build products, generated outputs, and local tooling clutter.
- Added a dependency-free Rust 2024 library and `sr` binary scaffold.
- Added checked dimensions, RGB8 image ownership, and replaceable I/O and algorithm interfaces.
- Added CLI argument validation with documented exit statuses.
- Added an offline release build script, ASCII policy checker, and cross-platform CI workflow.
- Added a strict dependency-free RGB8 PPM P6 byte codec and standard-library file adapter.
- Added deterministic PPM encoding and validation for malformed headers, dimensions, raster length, separators, and trailing data.
- Added in-memory and temporary-file codec tests, including leading raster whitespace and hash bytes.
- Added deterministic fixed-point BT.601 full-range RGB8 and YCbCr8 conversion.
- Added a checked scalar separable 2x Catmull-Rom bicubic plane scaler.
- Added the `BicubicBaseline` pipeline with fixed vectors, border, impulse, gradient, color, dimension, and repeatability tests.
- Added an opt-in deterministic scalar `QualityPipeline` that changes only bicubic luma.
- Added fixed-point edge orientation, direction-guided refinement, controlled sharpening, and local-envelope anti-ringing.
- Added quality-candidate tests for constants, borders, orientations, envelopes, dimensions, mismatch handling, repeatability, and synthetic edges.
- Added a strict provisional packed row-major RGB8 raw codec for assumption A-002.
- Wired the two-argument PPM P6 command through the scalar bicubic baseline.
- Added explicit raw RGB8 processing with required dimensions and stable CLI exit statuses.
- Added standard-library end-to-end CLI tests for PPM, raw RGB8, malformed input, dimensions, and exit codes.
- Added deterministic non-recursive PPM batch processing with output creation and unrelated-file skipping.
- Added non-overwriting batch output policy, ordered failure reporting, and partial-failure continuation.
- Added centralized processing-only timing around algorithm execution without normal timing output.
- Added provisional batch assumption A-007 and standard-library batch integration tests.
- Added self-written provisional diagnostic luma PSNR and global luma SSIM metrics.
- Added deterministic source-generated constant, gradient, hard-edge, and checker-detail fixtures.
- Added fixed-threshold quality regression coverage without asserting pipeline superiority.
- Added a deterministic `visual_qa` example that writes PPM artifacts and reports diagnostic metrics.
- Added provisional metric assumption A-008 and documented the exact formulas and constants.
- Added an exact four-row bicubic working-set optimization with retained scalar regression oracles.
- Added exact baseline and quality equivalence tests for synthetic, odd-sized, thin, and single-pixel inputs.
- Added a reproducible processing-only release benchmark with deterministic checksums.
- Added documented memory calculations and local before/after scalar measurements.
- Added bounded three-channel standard-library execution with an automatic size and available-parallelism policy.
- Added stable spawn-failure and worker-panic algorithm errors with join-all scoped-thread handling.
- Added forced serial/parallel oracle equality tests and local two-resolution threading measurements.
- Added local 1920x1080 processing evidence for the provisional 1 FPS target.
- Documented compiler assembly inspection and the evidence-gated decision to defer hand-written SIMD.
- Added cross-platform CI benchmark policy and checksum verification without timing gates.
- Added a repository-wide compliance evidence matrix without upgrading provisional claims.
- Added Linux and Windows build, run, evaluation, packaging, and official-package reconciliation instructions.
- Added a deterministic source-only provisional review ZIP creator and strict manifest verifier.
- Added cross-platform CI creation and verification of the provisional review package.
- Added a Rust 1.85 PowerShell one-click release build entry point.
- Added specific ignore rules for the local DIV2K test directory and archive.
- Added a dependency-free developer PNG-to-PPM/raw converter with strict PNG validation and atomic directory output.
- Added synthetic cross-platform converter checks covering every PNG scanline filter and required failure paths.
- Documented development-only DIV2K preparation without adding PNG to the Rust runtime path.
- Bounded developer PNG source and decoded sizes and enforced the PNG chunk-type reserved bit.
- Added organizer-confirmed fixed 1920x1080 packed RGB888 input and 3840x2160 output constants.
- Extended the two-argument command with extension-first PPM/raw selection and safe unknown-extension detection.
- Extended deterministic batch discovery to `.ppm`, `.raw`, and `.rgb` while preserving each input format.
- Added full-size official raw routing, byte-count, content-hash, and mixed-format discovery tests.
- Switched the maintained build and CI scope to Windows only.
- Replaced the provisional review ZIP with a deterministic uncompressed Windows TAR candidate workflow.
- Added package algorithm and AI Coding documents plus mandatory external conversation-log input.
- Documented `build.ps1` as the one-click build program and `bin/sr.exe` as the precompiled standard-test executable.
- Reclassified Catmull-Rom bicubic as a project-local development anchor after confirmation that no coefficient file will follow.

### Fixed

- Restored Rust 1.85 CI compatibility for the PPM adapter.
