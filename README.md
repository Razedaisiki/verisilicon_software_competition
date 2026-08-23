# Verisilicon Rust Super-Resolution

This repository is the Rust software-track project for a dependency-free 2x image super-resolution command-line tool.

The project provides an end-to-end dependency-free command-line path for strict
PPM P6 and organizer-confirmed fixed packed RGB888 raw files. Public processing
commands use the deterministic ungated `SelectedQualityPipeline`.

The frozen `QualityPipeline` remains an opt-in library and evaluator path. The
unchanged `BicubicBaseline` remains the scalar correctness oracle and local
comparison anchor.

The selected pipeline changes only bicubic luma. It uses integer Sobel edge
orientation with threshold 64 and axis ratio 2, Q8 same-direction refinement
with gain 32, Q8 luma sharpening with gain 64, and a radius-one anti-ringing
envelope taken from the unenhanced bicubic luma. Chroma remains bicubic. Every
changed luma sample is clamped to its original 3x3 local range.

## Bicubic baseline

The baseline converts RGB8 to full-range YCbCr8 using documented Q8 integer coefficients, scales all three planes, and converts the result back to RGB8. The scaler uses half-pixel mapping and the project-selected Catmull-Rom parameter `a = -0.5`. It is a local development anchor, not a claim of byte equivalence with the organizer's bicubic downsampling.

The two exact Q7 polyphase weight sets are `[-3, 29, 111, -9]` and `[-9, 111, 29, -3]`. Horizontal results remain signed until the vertical pass. Borders are clamped, combined Q14 results are rounded to nearest with halves away from zero, and only final samples are clipped to the 8-bit range.

## Recommended architecture experiment

`RecommendedBaselineV1` is a development-only deterministic instance of the
organizer-supplied reference architecture. It uses nearest-neighbor 2x luma,
a Q8 gain-32 four-neighbor Laplacian refinement with a clamped local envelope,
and half-pixel Q8 bilinear chroma. The reference slide does not provide these
numerical details, so V1 is not claimed to be organizer byte-equivalent.

The public `sr` commands use `SelectedQualityPipeline`. Eval30 measured V1
below the bicubic anchor in both local quality metrics and below it in local
processing throughput, so the experiment is retained for evidence rather than
selected as the default.

`BilinearChromaQualityPipeline` is another explicit negative experiment. It
keeps the selected bicubic-plus-enhanced luma path and changes only Cb/Cr to
bilinear 2x scaling. Eval30 measured it slightly below the selected pipeline in
both Y metrics, and interleaved 1080p runs found no repeatable speed gain. The
public command-line path therefore retains bicubic chroma.

## PPM P6 codec

The library codec accepts RGB8 PPM P6 data with decimal width and height, `maxval` exactly 255, legal header whitespace, and comments before header values. It requires an exact packed RGB8 raster with no trailing bytes. Encoding uses a deterministic header and raster representation.

The separator after `maxval` is exactly one whitespace character. CRLF is treated as one logical separator. Additional bytes, including whitespace and `#`, belong to the raster so valid leading pixel bytes are never discarded.

## Build

The minimum supported Rust version is 1.85.

```text
cargo build --locked --release
```

The maintained Windows PowerShell entry point copies the release binary to
`bin/sr.exe`:

```text
powershell -ExecutionPolicy Bypass -File .\build.ps1
```

## Commands

```text
sr <input> <output>
sr --raw-rgb8 <width> <height> <input.raw> <output.raw>
sr --batch <in_dir> <out_dir>
sr --help
```

The two-argument command accepts strict PPM P6 with dynamic dimensions or the
fixed official raw working contract: 1920x1080 packed row-major RGB888 in R, G,
B byte order, top-to-bottom rows, with no header or stride padding. Fixed raw
input is exactly 6,220,800 bytes and output is 3840x2160 and exactly 24,883,200
bytes.

Input extension takes precedence: `.ppm` selects strict PPM, while `.raw` and
`.rgb` select fixed official raw, all ASCII case-insensitively. Unknown or
missing extensions are detected only when the complete input is either valid
PPM or has the exact official raw length. Ambiguous valid inputs must be renamed
with an explicit supported extension. Output encoding always follows the
selected input format, regardless of the output filename extension. The
`--raw-rgb8` command remains an explicit variable-dimension developer path.

Batch mode processes non-recursive regular `.ppm`, `.raw`, and `.rgb` files in
deterministic filename order, ASCII case-insensitively. It preserves filenames
and input formats, creates the output directory, and skips unrelated entries.
It refuses every existing output, continues after per-file failures, and
returns status 4 if any file fails or no candidates exist. Discovery and
failure semantics remain provisional assumption A-007; raw geometry is fixed.

Batch scheduling adapts to the logical parallelism reported by the operating
system. Scored-size batches use up to eight persistent frame workers with a
serial pipeline per frame, bounding the expected official-geometry working set
near 512 MiB and avoiding nested oversubscription. Small batches use channel
parallelism only when it fits the reported capacity; one runnable image keeps
the normal automatic policy. Completion may be out of order, while output
bytes, failure reduction, and diagnostics remain in deterministic filename
order.

Invalid arguments return status 2, processing failures return status 4, and successful commands return status 0. Single-file commands retain standard output replacement behavior.

Processing-only timing follows provisional assumption A-006. Timing starts immediately before the algorithm call and stops immediately after it, excluding decode and encode. Normal commands do not print timing values.

`SelectedQualityPipeline` is the CLI path. The original `QualityPipeline`
remains frozen as an opt-in library and evaluator API.

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
cargo run --locked --release --example processing_bench -- recommended auto 640 360 5
cargo run --locked --release --example processing_bench -- quality auto 640 360 5
cargo run --locked --release --example processing_bench -- selected-ungated auto 640 360 5
cargo run --locked --release --example processing_bench -- confidence-gated auto 640 360 5
```

Isolate the scalar luma-enhancement hot loop with:

```text
cargo run --locked --release --example luma_enhance_bench -- 3840 2160 10
```

This diagnostic performs one unmeasured warm-up, excludes fixture generation
and output hashing from timing, and prints a deterministic checksum. CI
compiles it but does not enforce a timing threshold.

Measure processing-only batch throughput with an explicit frame-worker count
and per-frame policy:

```text
cargo run --locked --release --example batch_processing_bench -- serial 6 1920 1080 12 2
cargo run --locked --release --example batch_processing_bench -- parallel 1 1920 1080 12 2
```

The example creates its deterministic inputs and persistent worker set before
timing, warms one complete batch, excludes output hashing, and reports batch
FPS plus an order-stable checksum. It is a diagnostic tool; CI verifies
concurrency correctness without enforcing a timing threshold.

The benchmark uses a deterministic in-memory gradient, one unmeasured warm-up,
and excludes fixture generation and I/O from timing. See
`docs/PERFORMANCE.md` for memory calculations, local before/after measurements,
limitations, thread failure semantics, and serial-versus-parallel measurements.

Local 1920x1080-to-3840x2160 measurements exceed the provisional 1 FPS target
on the documented development host, but they are not official compliance
evidence. Hand-written SIMD is currently deferred: the official CPU and
toolchain are unknown, the existing threaded scalar implementation already
clears the provisional target locally, and an intrinsic path would add unsafe,
dispatch, and cross-platform validation risk without demonstrated need.

## Diagnostic quality evaluation

The library exposes provisional luma PSNR and global luma SSIM diagnostics. They use the project's deterministic fixed-point BT.601 luma conversion and require equal image dimensions. Identical luma images have explicit infinite PSNR. These implementations are not claimed to match the missing official scoring tools.

The global SSIM diagnostic uses population variance and covariance over the complete image with `L = 255`, `K1 = 0.01`, `K2 = 0.03`, `C1 = 6.5025`, and `C2 = 58.5225`. It is not a sliding-window or multiscale SSIM variant.

The confirmed objective categories are mean per-image Y-PSNR and mean
per-image Y-SSIM, weighted 50 percent each with equal weight for every test
image. `docs/EVALUATION.md` defines a separate paired HR/LR local evaluation
contract, including windowed MSSIM and a stable report schema. The current
global SSIM remains a legacy synthetic diagnostic; no combined objective score
is invented while the organizer's cross-metric normalization is unknown.

Generate deterministic synthetic PPM artifacts and print baseline and quality diagnostic metrics with:

```text
cargo run --locked --release --example visual_qa -- <output_dir>
```

The example generates constant, smooth-gradient, hard-edge, and checker-detail cases. It refuses to overwrite any planned artifact. Generated files are local evaluation output and must not be committed.

## Development dataset conversion

The official `sr` runtime remains PPM/raw-only. For local evaluation, the
Python-standard-library-only `scripts/convert_div2k.py` tool converts the
constrained ignored DIV2K PNG directory to strict PPM P6 or packed RGB888 while
preserving relative paths. PNG decoding never enters the Rust runtime or
submission processing path.

```text
python scripts/convert_div2k.py --format ppm DIV2K_test_LR target/div2k-ppm
python scripts/convert_div2k.py --format raw DIV2K_test_LR target/div2k-raw
python scripts/check_div2k_converter.py
```

See `docs/DATA_PREPARATION.md` for validation rules, atomic output behavior,
usage, and unresolved official conversion caveats.

## Offline DIV2K paired dataset

The paired DIV2K workflow is separate from Eval30 and performs no downloads.
The current optimization workflow uses exactly the 100 validation pairs
`0801`-`0900`; the 800 train pairs are unused. Selected sources pass strict
RGB8 PNG decoding and exact HR=2xLR dimension checks. Test IDs `0901`-`1000`
remain input-only and are excluded from every paired manifest. The current
archive roles are `DIV2K_valid_HR.zip` and
`DIV2K_valid_LR_bicubic_X2.zip`. Optional train/all operation additionally
uses `DIV2K_train_HR.zip` and `DIV2K_train_LR_bicubic_X2.zip`. Assets are local
academic-research inputs and must not be redistributed.

```text
powershell -ExecutionPolicy Bypass -File .\prepare_div2k.ps1 -SourceDirectory path\to\DIV2K
python scripts/div2k_pairs.py validate-sources path\to\DIV2K --split validation
python scripts/div2k_pairs.py verify path\to\DIV2K evaluation/div2k/local/prepared --split validation
python scripts/check_div2k_pairs.py
```

Generated PPM pairs, source hashes, output hashes, split membership, and
manifest hashes are locked below the ignored `evaluation/div2k/local/` tree.
The workflow refuses existing output and publishes only by a completed sibling
directory rename. It never reads or modifies `evaluation/eval30`.

The default prepared tree contains only this evaluator-compatible manifest:

```text
evaluation/div2k/local/prepared/validation/pairs.tsv
```

No root-level combined manifest is created. `-Split Train` and `-Split All`
remain optional capabilities and produce only their selected split manifests.
They are not used by the current 100-image optimization. Run the current sweep
and comparison against the validation manifest:

```text
cargo run --locked --release --example quality_sweep -- evaluation/div2k/local/prepared/validation/pairs.tsv target/div2k-validation-sweep.csv 5
cargo run --locked --release --example paired_eval -- evaluation/div2k/local/prepared/validation/pairs.tsv target/div2k-validation-report.csv bicubic selected-ungated
```

The first held-out 100-image validation run placed the selected candidate above
the local bicubic anchor by `+0.168886 dB` Y-PSNR and `+0.003565996` Y-SSIM.
See `evaluation/div2k/RESULTS.md`; this is local development evidence, not an
official score.

## Local paired evaluation dataset

The development-only Eval30 catalog locks 30 reusable internet sources: 10
nature photographs, 10 game-like CG renders, and 10 text or UI screenshots.
The preparation tool creates an equal-category paired HR/LR database using
centered 16:9 HR crops and deterministic Pillow 11.3.0 bicubic 2x
downsampling. Images and generated artifacts remain local and ignored by Git;
the tracked catalogs retain source, author, license, dimensions, and SHA-256.

```text
python -m pip install -r evaluation/requirements.txt
python scripts/eval_dataset.py validate evaluation/eval30/sources --locked
python scripts/eval_dataset.py fetch evaluation/eval30/sources evaluation/eval30/local/source
python scripts/eval_dataset.py prepare evaluation/eval30/sources evaluation/eval30/local/source evaluation/eval30/local/prepared
python scripts/eval_dataset.py verify evaluation/eval30/sources evaluation/eval30/local/prepared
python scripts/check_eval_dataset.py
```

See `evaluation/eval30/README.md` for the exact local layout, preparation
contract, and redistribution warning. Eval30 is not an organizer dataset and
does not enter the Windows submission candidate.

After preparing the database, run the complete Windows evaluation workflow:

```text
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1
```

The script verifies the locked database, builds the dependency-free Rust
`paired_eval` release example, runs the unchanged baseline and quality
candidate on every LR image, and writes `target/eval30-report.csv`. It refuses
to overwrite an existing report. See `evaluation/eval30/RESULTS.md` for the
first byte-reproducible complete comparison.

Select the development-only recommended architecture explicitly and use a
distinct report filename:

```text
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Baseline recommended -Report target/eval30-recommended-v1.csv
```

Evaluate the coarse-sweep winner or the isolated confidence-gated experiment
against the bicubic anchor with distinct report names:

```text
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Candidate selected-ungated -Report target/eval30-selected-ungated.csv
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Candidate confidence-gated -Report target/eval30-confidence-gated.csv
```

The gated experiment suppresses residuals outside coherent edges, but Eval30
measured it below the selected ungated candidate on every image. It remains an
explicit diagnostic and is not selected by `sr.exe`.

Run the deterministic nine-configuration coarse parameter sweep with
five-fold category-stratified cross-validation:

```text
powershell -ExecutionPolicy Bypass -File .\sweep.ps1
```

The sweep measures final RGB outputs through the same Y-PSNR and Y-MSSIM
implementation. It reports training and held-out deltas separately, ranks
PSNR and SSIM independently, and does not invent an unknown combined score.
It is a development tool and does not change the public `sr.exe` pipeline.

## Development checks

```text
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features --locked
python scripts/check_ascii.py
python scripts/check_div2k_converter.py
python scripts/check_div2k_pairs.py
python scripts/eval_dataset.py validate evaluation/eval30/sources --locked
python scripts/check_eval_dataset.py
python scripts/check_batch_processing_bench.py
```

Official contest documents, assets, images, archives, and generated outputs are local inputs and are not tracked in this repository.

Create and verify a deterministic uncompressed Windows TAR candidate only after
building the declared target and exporting the real AI conversation logs:

```text
powershell -ExecutionPolicy Bypass -File .\build.ps1
python scripts/submission_package.py create target/submission-candidate.tar --binary bin/sr.exe --logs path/to/exported-ai-logs --target x86_64-pc-windows-msvc
python scripts/submission_package.py verify target/submission-candidate.tar
```

The Windows package keeps the two required roles: `build.ps1` is the one-click
build program, and `bin/sr.exe` is the precompiled executable used for standard
evaluation-machine testing.
See `docs/COMPLIANCE.md` for the
evidence audit, `docs/SUBMISSION.md` for build/evaluation/package instructions
and the official-package reconciliation checklist, `docs/EVALUATION.md` for
paired HR/LR metric and reporting rules, `docs/REQUIREMENTS.md` for
traceability, and `docs/ASSUMPTIONS.md` for provisional choices.
