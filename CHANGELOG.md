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

### Fixed

- Restored Rust 1.85 CI compatibility for the PPM adapter.
