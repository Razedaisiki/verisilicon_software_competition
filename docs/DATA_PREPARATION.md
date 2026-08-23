# Development Dataset Conversion

The Rust `sr` runtime accepts strict PPM P6 or explicit packed RGB888 raw input.
It does not accept PNG. `scripts/convert_div2k.py` is a developer-only data
preparation tool for the constrained local DIV2K PNG archive; it is not part of
the submitted super-resolution runtime path.

The local source directory and downloaded archive are ignored specifically as
`DIV2K_test_LR/` and `DIV2K_test_LR.zip`. Generated output should remain under
an ignored directory such as `target/`.

## Supported PNG subset

The converter is self-written with the Python standard library. It validates
the PNG signature, chunk ordering and CRCs, IHDR, consecutive IDAT structure,
IEND, zlib stream length, and scanline filters. It accepts only positive-size,
noninterlaced, 8-bit truecolor RGB PNG with PNG compression and filter methods
set to zero. Scanline filters 0 through 4, including Paeth, are reconstructed.
Unknown critical chunks, corrupt CRCs, malformed ordering, truncation, trailing
file data, and trailing compressed data are rejected.

Before reading or decompressing image payloads, the tool enforces a 64 MiB
source-file limit, a 16,777,216-pixel decoded limit, and a 40 MiB decoded RGB
limit. These conservative bounds cover the current DIV2K files and 3840x2160 or
4096x2160 developer images while rejecting oversized memory and CPU inputs.
PNG chunk types must also keep the specification's reserved third-byte bit at
zero.

The tool intentionally does not use Pillow, OpenCV, ImageMagick, NumPy, an
external executable, or a Rust image-processing dependency.

## Convert

The output directory must not already exist. Candidates are all `.png` files
below the input directory, matched case-insensitively and processed in sorted
relative-path order. Relative directories are preserved and extensions become
`.ppm` or `.raw`.

Strict PPM P6:

```text
python scripts/convert_div2k.py --format ppm DIV2K_test_LR target/div2k-ppm
```

Packed row-major RGB888:

```text
python scripts/convert_div2k.py --format raw DIV2K_test_LR target/div2k-raw
```

The converter builds and validates the complete output in a temporary sibling
directory and atomically renames that directory into place only after every
candidate succeeds. A failed batch removes its staging directory, and existing
output directories are refused.

PPM output is directly compatible with the two-argument `sr` command. Raw
output has no header, padding, or dimension sidecar; pass each image's known
width and height to `sr --raw-rgb8`. This packed layout remains provisional
assumption A-002.

## Check

```text
python scripts/check_div2k_converter.py
```

The check generates its own synthetic PNG bytes and requires no downloaded
dataset. It covers all five PNG filters, exact PPM and raw bytes, deterministic
directory layout, CRC corruption, unsupported formats, unknown critical chunks,
reserved-bit violations, oversized source and decoded dimensions, truncation,
trailing data, no candidates, and overwrite refusal. Linux and Windows CI run
this check.

## Caveats

The downloaded archive contains no HR ground truth and no authoritative README
defining contest conversion, color management, output naming, or scoring. Its
PNG files have no sRGB or gamma chunks. Conversion therefore preserves decoded
RGB sample bytes without claiming an official color interpretation. The
converter does not resolve assumptions A-001 through A-008 or make PNG an
official contest exchange format.
