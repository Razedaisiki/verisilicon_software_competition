# Verisilicon Rust Super-Resolution

This repository is the Rust software-track project for a dependency-free 2x image super-resolution command-line tool.

Milestone 6 provides an end-to-end dependency-free command-line path for strict PPM P6 and provisional packed RGB8 raw files. Both public processing commands use the unchanged scalar `BicubicBaseline`.

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

## Commands

```text
sr <input.ppm> <output.ppm>
sr --raw-rgb8 <width> <height> <input.raw> <output.raw>
sr --batch <in_dir> <out_dir>
sr --help
```

The official two-argument command decodes strict RGB8 PPM P6, scales it by 2x with `BicubicBaseline`, and writes deterministic PPM P6 output. The explicit raw command uses provisional assumption A-002: packed row-major RGB8 in R, G, B order with no header or row padding. Raw dimensions are mandatory.

Batch mode processes non-recursive regular PPM files in deterministic filename order, accepts `.ppm` case-insensitively, creates the output directory, preserves filenames, and skips unrelated entries. It refuses every existing output, continues after per-file failures, and returns status 4 if any file fails or no candidates exist. These semantics are provisional assumption A-007.

Invalid arguments return status 2, processing failures return status 4, and successful commands return status 0. Single-file commands retain standard output replacement behavior.

Processing-only timing follows provisional assumption A-006. Timing starts immediately before the algorithm call and stops immediately after it, excluding decode and encode. Normal commands do not print timing values.

`QualityPipeline` remains an opt-in library API and is not selected by the CLI.

## Performance diagnostics

The scalar bicubic implementation uses a four-row horizontal cache instead of
a full-plane signed intermediate. This preserves exact coefficients, rounding,
borders, and output while reducing the scaler's horizontal working set. For
inputs of at least 131,072 pixels on hosts reporting at least two-way available
parallelism, the pipelines run their three independent Y, Cb, and Cr channel
branches with at most three standard-library workers. Smaller inputs remain
serial.

Run the reproducible processing-only benchmark with:

```text
cargo run --locked --release --example processing_bench -- baseline auto 640 360 5
cargo run --locked --release --example processing_bench -- quality auto 640 360 5
```

The benchmark uses a deterministic in-memory gradient, one unmeasured warm-up,
and excludes fixture generation and I/O from timing. See
`docs/PERFORMANCE.md` for memory calculations, local before/after measurements,
limitations, thread failure semantics, and serial-versus-parallel measurements.

## Diagnostic quality evaluation

The library exposes provisional luma PSNR and global luma SSIM diagnostics. They use the project's deterministic fixed-point BT.601 luma conversion and require equal image dimensions. Identical luma images have explicit infinite PSNR. These implementations are not claimed to match the missing official scoring tools.

The global SSIM diagnostic uses population variance and covariance over the complete image with `L = 255`, `K1 = 0.01`, `K2 = 0.03`, `C1 = 6.5025`, and `C2 = 58.5225`. It is not a sliding-window or multiscale SSIM variant.

Generate deterministic synthetic PPM artifacts and print baseline and quality diagnostic metrics with:

```text
cargo run --locked --release --example visual_qa -- <output_dir>
```

The example generates constant, smooth-gradient, hard-edge, and checker-detail cases. It refuses to overwrite any planned artifact. Generated files are local evaluation output and must not be committed.

## Development checks

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
python scripts/check_ascii.py
```

Official contest documents, assets, images, archives, and generated outputs are local inputs and are not tracked in this repository.

See `docs/REQUIREMENTS.md` for requirement traceability and `docs/ASSUMPTIONS.md` for provisional interface and algorithm choices.
