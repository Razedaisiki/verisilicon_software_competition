# Verisilicon Rust Super-Resolution

This repository is the Rust software-track project for a dependency-free 2x image super-resolution command-line tool.

Milestone 4 provides a buildable Rust 2024 binary and library foundation. It includes checked image types, a strict PPM P6 codec, deterministic fixed-point BT.601 full-range color conversion, and a scalar separable 2x Catmull-Rom bicubic baseline. CLI processing is intentionally not wired yet.

Milestone 5 also provides an opt-in `QualityPipeline`. It is an unscored quality candidate, not a claim of measured superiority. The unchanged `BicubicBaseline` remains the scalar correctness oracle.

The candidate changes only bicubic luma. It uses integer Sobel edge orientation, Q8 same-direction refinement with gain 64, Q8 luma sharpening with gain 48, and a radius-one anti-ringing envelope taken from the unenhanced bicubic luma. Chroma remains bicubic. Every changed luma sample is clamped to its original 3x3 local range.

## Bicubic baseline

The baseline converts RGB8 to full-range YCbCr8 using documented Q8 integer coefficients, scales all three planes, and converts the result back to RGB8. The scaler uses half-pixel mapping and the provisional Catmull-Rom parameter `a = -0.5`.

The two exact Q7 polyphase weight sets are `[-3, 29, 111, -9]` and `[-9, 111, 29, -3]`. Horizontal results remain signed until the vertical pass. Borders are clamped, combined Q14 results are rounded to nearest with halves away from zero, and only final samples are clipped to the 8-bit range.

## PPM P6 codec

The library codec accepts RGB8 PPM P6 data with decimal width and height, `maxval` exactly 255, legal header whitespace, and comments before header values. It requires an exact packed RGB8 raster with no trailing bytes. Encoding uses a deterministic header and raster representation.

The separator after `maxval` is exactly one whitespace character. CRLF is treated as one logical separator. Additional bytes, including whitespace and `#`, belong to the raster so valid leading pixel bytes are never discarded.

## Build

The minimum supported Rust version is 1.85.

```text
cargo build --locked --release
```

On a Unix-like host, the offline one-click build entry point copies the release binary to `bin/sr`:

```text
sh build.sh
```

## Command skeleton

```text
sr <input> <output>
sr --batch <in_dir> <out_dir>
sr --help
```

Processing commands currently return exit status 3 with an English not-implemented message. Invalid arguments return exit status 2. The codec and baseline are not wired to these commands yet.

## Development checks

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
python scripts/check_ascii.py
```

Official contest documents, assets, images, archives, and generated outputs are local inputs and are not tracked in this repository.

See `docs/REQUIREMENTS.md` for requirement traceability and `docs/ASSUMPTIONS.md` for provisional interface and algorithm choices.
