# Algorithm Design and Coefficient Provenance

## Selected Runtime Pipeline

The public command-line path uses `SelectedQualityPipeline`. It is deterministic,
dependency-free, fixed-point where coefficient arithmetic is required, and
restricted to 2x scaling.

```text
+------------------+
| RGB888 input     |
| PPM P6 or raw    |
+--------+---------+
         |
         v
+------------------+       +------------------------------+
| RGB -> YCbCr8    |------>| Split Y, Cb, and Cr planes   |
| full-range Q8    |       +---------------+--------------+
+------------------+                       |
                            +--------------+--------------+
                            |                             |
                            v                             v
                 +----------------------+      +----------------------+
                 | Y: separable         |      | Cb and Cr: separable |
                 | bicubic 2x           |      | bicubic 2x           |
                 +----------+-----------+      +----------+-----------+
                            |                             |
                            v                             |
                 +----------------------+                 |
                 | Sobel orientation    |                 |
                 | directional refine   |                 |
                 | sharpen + anti-ring  |                 |
                 +----------+-----------+                 |
                            +--------------+--------------+
                                           |
                                           v
                              +--------------------------+
                              | Merge Y, Cb, Cr          |
                              | YCbCr8 -> RGB888 Q8      |
                              +------------+-------------+
                                           |
                                           v
                              +--------------------------+
                              | 2x RGB888 output         |
                              | PPM P6 or packed raw     |
                              +--------------------------+
```

Chroma remains the bicubic result. Only bicubic luma is enhanced. The selected
path does not use the retained recommended, confidence-gated, or bilinear-chroma
experiments.

## Module Responsibilities

| Module | Principle and responsibility |
| --- | --- |
| `src/main.rs` | Native process entry point and stable exit status propagation |
| `src/cli.rs` | Argument parsing, format routing, processing-only timing boundary, deterministic batch scheduling, and output policy |
| `src/io/ppm.rs` | Strict binary PPM P6 RGB8 decode and deterministic encode |
| `src/io/raw.rs` | Headerless row-major interleaved RGB888 decode and encode |
| `src/algorithm/color.rs` | Fixed-point full-range BT.601 RGB8 and YCbCr8 conversion |
| `src/algorithm/bicubic.rs` | Four-tap separable Catmull-Rom bicubic 2x scaling with a four-row cache |
| `src/algorithm/quality.rs` | Sobel edge classification, directional luma refinement, sharpening, and local-envelope anti-ringing |
| `src/algorithm.rs` | Algorithm interface, bounded channel parallelism, and error handling |
| `src/image.rs` | Checked owned RGB8 image representation |
| `src/spec.rs` | Checked dimensions, fixed 2x scale, and official raw geometry constants |

## RGB and YCbCr Arithmetic

All color coefficients are Q8 integers. Signed divisions round to nearest with
exact halves away from zero, and final channel values are clamped to `[0, 255]`.

Forward conversion:

```text
Y  = round(( 77 R + 150 G +  29 B) / 256)
Cb = 128 + round((-43 R -  85 G + 128 B) / 256)
Cr = 128 + round((128 R - 107 G -  21 B) / 256)
```

Inverse conversion:

```text
R = Y + round(359 (Cr - 128) / 256)
G = Y - round((88 (Cb - 128) + 183 (Cr - 128)) / 256)
B = Y + round(454 (Cb - 128) / 256)
```

These are project-selected Q8 approximations of full-range BT.601 equations.
They are stored as named source constants in `src/algorithm/color.rs`. They are
not claimed to be organizer-supplied coefficients.

## Separable Bicubic 2x Scaling

The baseline is the Catmull-Rom cubic kernel with parameter `a = -0.5`. The
half-pixel mapping has two Q7 phases:

| Output coordinate | Source phase | Source offsets | Q7 weights |
| --- | ---: | --- | --- |
| Even | 0.75 | `[-2, -1, 0, 1]` | `[-3, 29, 111, -9]` |
| Odd | 0.25 | `[-1, 0, 1, 2]` | `[-9, 111, 29, -3]` |

Each weight set sums to 128. The horizontal pass retains signed Q7 values. The
vertical pass forms a signed Q14 result, rounds once, and clamps only the final
plane sample. Coordinates outside the image use nearest-border replication.

The coefficients were derived from the project-selected Catmull-Rom kernel and
quantized to exact normalized Q7 phase sums. The organizer confirmed that no
additional official bicubic coefficient file would be supplied. Source values
are `EVEN_PHASE_WEIGHTS`, `ODD_PHASE_WEIGHTS`, `WEIGHT_SHIFT = 7`, and
`COMBINED_SHIFT = 14` in `src/algorithm/bicubic.rs`.

`CACHE_ROWS = 4` follows directly from the four vertical taps. It reduces the
horizontal working set without changing arithmetic or output bytes.

## Selected Luma Enhancement

Enhancement reads only the unmodified bicubic Y plane. For each output sample,
a clamped 3x3 neighborhood is loaded and Sobel gradients are computed:

```text
Gx = (NE + 2 E + SE) - (NW + 2 W + SW)
Gy = (SW + 2 S + SE) - (NW + 2 N + NE)
```

The selected parameters are:

| Constant | Value | Meaning |
| --- | ---: | --- |
| edge threshold | 64 | Minimum dominant absolute Sobel component |
| axis dominance ratio | 2 | Horizontal or vertical classification ratio |
| directional refine gain | 24/256 | Blend toward the pair along the detected edge |
| sharpening gain | 80/256 | Gain on center minus four-neighbor low pass |
| envelope radius | 1 | Clamp against the original bicubic 3x3 minimum and maximum |

If `max(abs(Gx), abs(Gy)) < 64`, the sample is classified as flat. Otherwise,
the ratio 2 selects a horizontal or vertical edge; remaining samples use the
gradient signs to select one diagonal. The directed value `D` is the rounded
average of the two samples along that orientation. For center `C`:

```text
refined   = C + round((D - C) * 24 / 256)
low_pass  = round((W + E + N + S) / 4)
sharpened = refined + round((C - low_pass) * 80 / 256)
output    = clamp(sharpened, minimum_3x3, maximum_3x3)
```

The local-envelope clamp prevents new luma extrema and limits ringing. Cb and
Cr are not sharpened. The tuple `(64, 2, 24, 80)` was selected by the recorded
stratified Eval30 fine search, where all five training folds selected it for
both local metrics, then accepted after a separate 100-pair validation run.
The source constant is `SELECTED_UNGATED_PARAMETERS` in
`src/algorithm/quality.rs`.

## Coefficient and Threshold Provenance

| Values | Status | Source or selection reason |
| --- | --- | --- |
| RGB-to-YCbCr `77,150,29,-43,-85,128,-107,-21` | Active | Project Q8 quantization of full-range BT.601 |
| YCbCr-to-RGB `359,88,183,454` | Active | Project Q8 quantization of full-range BT.601 inverse |
| Bicubic `a=-0.5`; Q7 phase taps shown above | Active | Project Catmull-Rom development anchor; no organizer coefficient file supplied |
| Quality `(64,2,24,80)`, radius 1 | Active | Stratified Eval30 selection plus separate 100-pair validation |
| Quality `(48,2,64,48)`, radius 1 | Retained, inactive | Initial project quality candidate and regression reference |
| Quality `(64,2,32,64)`, radius 1 | Retained, inactive | Historical fine-sweep anchor used by rejected experiments |
| Confidence evidence `8` to `48` | Retained, inactive | Project-designed ramp; measured below matching ungated control |
| Recommended refine gain `32/256` | Retained, inactive | Project-local assumption filling an unspecified slide coefficient |
| Recommended bilinear phases `64/256`, `192/256` | Retained, inactive | Exact quarter and three-quarter half-pixel bilinear weights |
| Channel workers `3` | Active engineering limit | One independent worker per Y, Cb, and Cr plane |
| Parallel threshold `131072` input pixels | Active engineering threshold | Project benchmark-guided overhead cutoff |
| Batch worker cap `8` | Active engineering limit | Approximately 60 MiB per official-size task and a conservative 512 MiB cap |

All listed values appear as source constants or source literals. No binary
coefficient table is loaded at runtime.

## Retained Non-Selected Pipelines

`RecommendedBaselineV1` implements the organizer-suggested direction as an
isolated experiment: RGB-to-YCbCr, nearest-neighbor 2x luma, 3x3 bounded luma
refinement, Q8 bilinear chroma, and YCbCr-to-RGB. Its numerical details missing
from the slide were fixed by documented project assumptions. Eval30 evidence
did not justify replacing the bicubic selected path.

`ConfidenceGatedQualityPipeline` blends the historical quality residual using
a Q8 confidence ramp from evidence 8 to 48. It was measured below its matching
ungated control. `BilinearChromaQualityPipeline` retained historical luma but
replaced bicubic chroma with bilinear chroma; it also measured below its control.
Both remain reproducibility references and are not called by `sr.exe`.

## Determinism and Parallel Execution

Automatic single-frame processing considers channel parallelism at 131,072
input pixels and uses at most three standard-library workers. Batch processing
uses persistent frame workers capped at eight. Large batches use serial
per-frame pipelines to avoid nested oversubscription. Small batches may use
inner channel parallelism when logical processors are available.

Serial and parallel selected-pipeline outputs are regression-tested for exact
byte equality. The packaged `build.ps1` also rebuilds from submitted source and
compares the result byte for byte with `bin/sr.exe`.
