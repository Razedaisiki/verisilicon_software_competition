# Verisilicon Rust Super-Resolution

This repository is the Rust software-track project for a dependency-free 2x image super-resolution command-line tool.

Milestone 2 provides a buildable Rust 2024 binary and library scaffold. It defines checked image dimensions, RGB8 image ownership, replaceable image I/O and algorithm boundaries, and the required CLI shapes. Image decoding and scaling are intentionally not implemented yet.

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

Processing commands currently return exit status 3 with an English not-implemented message. Invalid arguments return exit status 2.

## Development checks

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
python scripts/check_ascii.py
```

Official contest documents, assets, images, archives, and generated outputs are local inputs and are not tracked in this repository.

See `docs/REQUIREMENTS.md` for requirement traceability and `docs/ASSUMPTIONS.md` for provisional interface and algorithm choices.
