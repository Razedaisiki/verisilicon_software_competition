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
output has no header, padding, or dimension sidecar. Use the two-argument path
only for converted images that are exactly 1920x1080; it applies the fixed
organizer-confirmed A-002 geometry. For the varying DIV2K dimensions, pass each
image's known width and height to the developer `sr --raw-rgb8` path.

## Check

```text
python scripts/check_div2k_converter.py
```

The check generates its own synthetic PNG bytes and requires no downloaded
dataset. It covers all five PNG filters, exact PPM and raw bytes, deterministic
directory layout, CRC corruption, unsupported formats, unknown critical chunks,
reserved-bit violations, oversized source and decoded dimensions, truncation,
trailing data, no candidates, and overwrite refusal. Windows CI runs this
check.

## Offline paired DIV2K preparation

`scripts/div2k_pairs.py` is a separate offline paired-dataset workflow. It does
not download data, does not use `evaluation/eval30`, and does not reinterpret
the existing DIV2K test images as ground truth. The default and current
optimization workflow uses exactly the 100 validation IDs `0801` through
`0900`. The 800 train IDs `0001` through `0800` are unused for the current
optimization; they remain available through explicit train/all selection.
Test IDs `0901` through `1000` remain input-only.

The current validation sources are expected to originate from these official
archive roles and filenames:

```text
DIV2K_valid_HR.zip
DIV2K_valid_LR_bicubic_X2.zip
```

Optional train/all selection additionally uses `DIV2K_train_HR.zip` and
`DIV2K_train_LR_bicubic_X2.zip`.

The script performs no download or archive extraction. DIV2K assets are for
academic research use under their upstream terms. Do not redistribute source
archives, PNG images, generated PPM pairs, or generated locks containing local
asset hashes.

After user-controlled extraction, the source root must contain exactly the
files for each selected split inside its inspected directories:

```text
DIV2K_train_HR/0001.png ... 0800.png
DIV2K_train_LR_bicubic/X2/0001x2.png ... 0800x2.png
DIV2K_valid_HR/0801.png ... 0900.png
DIV2K_valid_LR_bicubic/X2/0801x2.png ... 0900x2.png
```

Other source-root directories, including unselected train directories and a
test-image directory, are outside that invocation and ignored. Missing, extra,
cross-split, duplicate, symlinked, nested, or special entries inside selected
directories are errors.
Every source uses the same strict standard-library RGB8 PNG decoder as
`convert_div2k.py`. HR width and height must each equal twice the paired LR
dimension. Decode and PPM work uses an ordered standard-library process pool
bounded to four workers; result and error observation remains source-ID ordered.

Run preparation and verification directly:

```text
python scripts/div2k_pairs.py validate-sources path/to/DIV2K --split validation
python scripts/div2k_pairs.py prepare path/to/DIV2K evaluation/div2k/local/prepared --split validation
python scripts/div2k_pairs.py verify path/to/DIV2K evaluation/div2k/local/prepared --split validation
```

All three commands accept `validation`, `train`, or `all` through `--split` and
default to `validation`. The default therefore reads and publishes only 100
pairs, even when train sources also exist. Only an explicit `train` or `all`
selection inspects the 800 train images.

Or use the Windows wrapper, which performs both operations:

```text
powershell -ExecutionPolicy Bypass -File .\prepare_div2k.ps1 -SourceDirectory path\to\DIV2K
```

The wrapper's validated `-Split` parameter defaults to `Validation`; `Train`
and `All` are optional. Use a distinct prepared path for another selection
because publication refuses existing output.

Preparation emits deterministic PPM P6 directories and one `pairs.tsv` for
each selected split. The default tree therefore contains only
`validation/hr`, `validation/lr`, and `validation/pairs.tsv`; it has no train
tree. The optional all tree contains separate `train/pairs.tsv` and
`validation/pairs.tsv` files. Each uses the generic evaluators' exact
`id<TAB>lr_path<TAB>hr_path` contract. IDs are `div2k_train_NNNN` or
`div2k_validation_NNNN`; image paths are relative to their own split. No
combined evaluator manifest is ever emitted. `dataset-lock.csv` records
SHA-256 for every selected source PNG and prepared PPM plus exact dimensions.
`dataset-metadata.json` records only the selected split ranges plus the
input-only test boundary. `manifest-sha256.txt` locks only the selected split
manifests, the dataset lock, and metadata.

The current 100-image optimization and comparison use validation; train is not
used:

```text
cargo run --locked --release --example quality_sweep -- evaluation/div2k/local/prepared/validation/pairs.tsv target/div2k-validation-sweep.csv 5
cargo run --locked --release --example paired_eval -- evaluation/div2k/local/prepared/validation/pairs.tsv target/div2k-validation-report.csv bicubic selected-ungated
```

The complete tree is built and checked in a temporary sibling directory, then
published with one directory rename. Existing output paths are refused and a
failed preparation removes its staging directory. Verification rejects path
traversal, symlinks, missing or extra prepared files, source or PPM changes,
manifest changes, wrong ordering, and noncanonical paths.

The ignored workspace is `evaluation/div2k/local/`; real PNG, PPM, ZIP, lock,
and manifest data must not be committed. The synthetic standard-library check
requires no real DIV2K files:

```text
python scripts/check_div2k_pairs.py
```

## Caveats

The original input-only test archive contains no HR ground truth and no
authoritative README defining contest conversion, color management, output
naming, or scoring. Its PNG files have no sRGB or gamma chunks. Conversion
therefore preserves decoded RGB sample bytes without claiming an official color
interpretation. Neither workflow resolves assumptions A-001 through A-008 or
makes PNG an official contest exchange format.
