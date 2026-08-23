# Paired HR/LR Evaluation Contract

This document separates confirmed organizer scoring facts from the local,
fully specified diagnostic contract proposed for reproducible development. It
does not claim that the local color transform, SSIM implementation, report
format, or baseline implementation is the organizer's hidden implementation.

## Confirmed objective-scoring facts

- The objective score uses mean per-image Y-channel PSNR and mean per-image
  Y-channel SSIM.
- Y-PSNR contributes 50 percent and Y-SSIM contributes 50 percent.
- Every test image has equal weight in each dataset mean, regardless of pixel
  count.

The organizer has not specified how PSNR and SSIM, which have different units
and ranges, are normalized or composed into one numeric 50/50 score. Therefore
the local report records both dataset averages separately and does not invent a
combined objective score.

## Pairing and evaluation scope

Evaluation requires ground-truth HR and corresponding LR images. A manifest is
an ASCII tab-separated file with this exact header:

```text
id<TAB>lr_path<TAB>hr_path
```

Each later row contains one pair. `id` must match `[A-Za-z0-9._-]+` and be
unique. Paths are resolved relative to the manifest directory. Duplicate IDs,
missing or duplicate files, malformed rows, unsupported formats, an empty
manifest, or a generated SR image whose dimensions differ from HR are errors.
Pairs are sorted by `id` before processing and reduction.

The selected pipeline first generates SR from LR. Metrics compare that SR image
with its paired HR image. LR is never compared directly with SR or HR for an
objective score. No implicit resize, alignment, crop, or color correction is
permitted.

When HR is unavailable, the tool can produce SR output and run deterministic
invariant or visual checks, but it cannot report Y-PSNR, Y-SSIM, an objective
score, or a quality comparison with the baseline.

## Local luma contract

The local luma transform is named `bt601-full-q8-v1`. For each RGB8 pixel:

```text
Y = clip_u8(round_nearest((77 R + 150 G + 29 B) / 256))
```

For nonnegative RGB8 input, the exact implementation is
`(77 R + 150 G + 29 B + 128) >> 8`. This is the existing project-local
full-range fixed-point transform. It is not asserted to match an organizer
transform.

## Per-image Y-PSNR

Y-PSNR uses every HR/SR pixel; there is no border crop:

```text
SSE = sum((Y_hr - Y_sr)^2)
MSE = SSE / pixel_count
PSNR-Y = 10 log10(255^2 / MSE)
```

SSE is accumulated exactly before conversion to floating point. Identical Y
planes report `inf`; the value is not capped without an organizer rule.

## Per-image Y-SSIM

The local metric is single-scale mean SSIM over 11x11 Gaussian windows with
sigma 1.5, `K1 = 0.01`, `K2 = 0.03`, and sample range `L = 255`. Thus
`C1 = 6.5025` and `C2 = 58.5225`.

For deterministic kernel generation, the symmetric one-dimensional Q20 weights
are fixed as:

```text
[1078, 7968, 37750, 114673, 223352, 278934,
 223352, 114673, 37750, 7968, 1078]
```

They sum to 1,048,576. The two-dimensional weight is the separable product and
has denominator `2^40`. For each valid window center, use population weighted
moments:

```text
SSIM = ((2 mu_x mu_y + C1) (2 sigma_xy + C2)) /
       ((mu_x^2 + mu_y^2 + C1) (sigma_x^2 + sigma_y^2 + C2))
```

Only windows fully contained in the image are evaluated. There is no padding,
reflection, replication, or additional image crop. Images narrower or shorter
than 11 pixels are errors. Local SSIM values are averaged in row-major order.
Tiny negative variances caused only by floating-point cancellation may be
clamped to zero; the final SSIM value is not clamped.

The existing whole-image global SSIM in `src/metrics.rs` remains a legacy
synthetic regression diagnostic. It is not this local-window MSSIM contract,
and its existing thresholds must not be reused as MSSIM thresholds.

The SSIM definition is based on the authors' reference description and paper:

- https://www.cns.nyu.edu/~lcv/ssim/
- https://www.cns.nyu.edu/pub/Lcv/wang03-reprint.pdf

## Dataset reduction and baseline comparison

For `N` valid pairs, calculate each dataset result as an arithmetic mean of the
unrounded per-image metric values:

```text
mean_psnr_y = sum(per_image_psnr_y_db) / N
mean_ssim_y = sum(per_image_ssim_y) / N
```

This gives each image exactly `1/N` weight. Do not pool pixel errors or SSIM
windows across images. Reduction order is sorted pair ID, using compensated
floating-point summation. If any per-image PSNR is `inf`, its dataset mean is
`inf` and the report records how many infinite values occurred.

Evaluate the project baseline and candidate against the same manifest and HR
files. Report both pairs of averages and the candidate-minus-baseline delta for
each metric. A positive delta is locally better for that metric. Do not claim
admission, superiority, or a combined 50/50 score from these deltas without the
organizer's normalization and acceptance procedure.

## Exact report schema

The report is ASCII RFC 4180 CSV with LF line endings and this header:

```text
record_type,pipeline,id,lr_path,hr_path,width,height,image_count,infinite_psnr_count,psnr_y_db,ssim_y
```

Rules:

- `record_type=image` has one row per pair and pipeline. It records dimensions,
  leaves aggregate counts empty, prints PSNR with six decimal places or `inf`,
  and prints SSIM with nine decimal places.
- `record_type=dataset_average` has one row for `baseline` and one for
  `candidate`; `id` is `__dataset_average__`, paths and dimensions are empty,
  and both count columns are populated.
- `record_type=dataset_delta` has pipeline `candidate-minus-baseline`, ID
  `__dataset_delta__`, empty paths and dimensions, image count populated,
  infinite count empty, and the two signed average deltas. A PSNR delta
  involving infinity is reported as `undefined`, not a fabricated number.
- Image rows are sorted by ID, with baseline before candidate. Aggregate rows
  follow image rows in baseline, candidate, then delta order.
- CSV quoting follows RFC 4180. Metrics are aggregated before display rounding.

The output path must not already exist. Validate the complete manifest and all
pairs before processing. Write the report to a new temporary file in the output
directory, flush and close it, then atomically rename it to the requested path.
Any failure removes the temporary file and publishes no partial report.

## Local implementation

The dependency-free `paired_eval` Rust example implements this contract for
an explicitly selected candidate and baseline without changing those
pipelines. The default baseline selector is `bicubic`. The optional
`recommended` selector chooses the isolated `RecommendedBaselineV1` experiment.
The default candidate selector is the frozen `quality` pipeline. Explicit
`selected-ungated`, `confidence-gated`, and `bilinear-chroma` selectors choose
the coarse-sweep winner, its isolated gating experiment, or the isolated
bilinear-chroma experiment.
On Windows, the complete locked Eval30 workflow is:

```text
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Baseline recommended -Report target/eval30-recommended-v1.csv
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Candidate selected-ungated -Report target/eval30-selected-ungated.csv
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Candidate confidence-gated -Report target/eval30-confidence-gated.csv
```

For a prepared compatible manifest and an explicit report path, the evaluator
can also be run directly:

```text
cargo run --offline --locked --release --example paired_eval -- path/to/pairs.tsv path/to/report.csv bicubic
cargo run --offline --locked --release --example paired_eval -- path/to/pairs.tsv path/to/recommended-report.csv recommended
cargo run --offline --locked --release --example paired_eval -- path/to/pairs.tsv path/to/selected-report.csv bicubic selected-ungated
cargo run --offline --locked --release --example paired_eval -- path/to/pairs.tsv path/to/gated-report.csv bicubic confidence-gated
```

The CSV keeps the versioned `baseline` role label. The default candidate keeps
the legacy `candidate` role; explicit candidates use their selector as the
role. Every retained report must use a distinct filename and record the
selector command alongside its hash. Omitting both selectors remains
byte-compatible with explicit `bicubic quality`.

The Python dataset utilities and Rust evaluator are development-only tools and
are not included in the Windows submission candidate.

## Parameter sweep and cross-validation

The dependency-free `quality_sweep` example evaluates a fixed, deterministic
nine-configuration coarse grid for the existing luma quality arithmetic. The
frozen public candidate parameters are always included. It derives the content
category by removing one final underscore-or-hyphen numeric suffix from each
pair ID, then assigns sorted members of every category round-robin across
folds. Every category must contain at least as many images as the requested
fold count.

For every fold, candidate selection uses only the complementary training
images. PSNR and SSIM are ranked and selected independently, and the training
Pareto frontier is recorded. Held-out metrics, per-category deltas, and the two
out-of-fold summaries are reported afterward. No cross-metric scalar score is
created. All metrics are calculated from final RGB output with this document's
unchanged Y-PSNR and Y-MSSIM implementations.

The Windows runner verifies Eval30, builds offline, refuses to overwrite its
CSV report, and publishes the result atomically:

```text
powershell -ExecutionPolicy Bypass -File .\sweep.ps1
```

The default is five folds and `target/quality-sweep.csv`. The Rust example also
accepts an explicit compatible manifest, output path, and fold count:

```text
cargo run --offline --locked --release --example quality_sweep -- path/to/pairs.tsv path/to/results.csv 5
```

## Remaining official unknowns

- Exact organizer Y transform and rounding.
- Exact PSNR infinity or cap policy.
- Exact SSIM window weights, border policy, moment convention, and numerical
  precision.
- Cross-metric normalization and composition used before applying the confirmed
  50/50 weights.
- Dataset files, baseline outputs, thresholds, report schema, and acceptance
  tooling.

These unknowns require a distinct organizer-compatible implementation when
specified; they must not silently change this versioned local contract.
