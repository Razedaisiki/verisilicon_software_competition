# Verisilicon Rust Super-Resolution

This repository is the Rust software-track project for a dependency-free 2x image super-resolution command-line tool.

Milestone 3 provides a buildable Rust 2024 binary and library foundation. It defines checked image dimensions, RGB8 image ownership, replaceable image I/O and algorithm boundaries, and a strict dependency-free PPM P6 codec. Scaling and CLI processing are intentionally not implemented yet.

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

Processing commands currently return exit status 3 with an English not-implemented message. Invalid arguments return exit status 2. The codec is not wired to these commands yet.

## Development checks

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
python scripts/check_ascii.py
```

Official contest documents, assets, images, archives, and generated outputs are local inputs and are not tracked in this repository.

See `docs/REQUIREMENTS.md` for requirement traceability and `docs/ASSUMPTIONS.md` for provisional interface and algorithm choices.
