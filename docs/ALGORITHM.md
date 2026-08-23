# Submission Algorithm Description

The command-line processing path uses the repository's deterministic ungated
`SelectedQualityPipeline`. It starts from `BicubicBaseline`, the project-local
development and evaluation anchor. The anchor is not claimed to be
byte-equivalent to the organizer's hidden public-bicubic LR generation or to
an organizer reference implementation.

Input RGB8 is converted to full-range YCbCr8 with the Q8 integer formulas in
`src/algorithm/color.rs`. Each plane is scaled independently by a separable 2x
Catmull-Rom cubic kernel with parameter `a = -0.5`, chosen by this project. The
organizer clarified that no additional official bicubic coefficient file will
be supplied.

For half-pixel mapping, even output coordinates use source phase 0.75 with Q7
weights `[-3, 29, 111, -9]` at offsets `[-2, -1, 0, 1]`. Odd coordinates use
source phase 0.25 with Q7 weights `[-9, 111, 29, -3]` at offsets
`[-1, 0, 1, 2]`. Each set sums to 128. Source coordinates outside the image
are clamped to the nearest border sample.

Horizontal Q7 results remain signed. The vertical pass accumulates a signed
Q14 value, rounds to nearest with exact halves away from zero, and clips only
the final plane sample to 0 through 255. Processed Y and scaled Cb and Cr are
converted back to RGB8 with the fixed inverse formulas and clipping in
`src/algorithm/color.rs`.

The optimized scaler caches four horizontal rows. Tests retain a full signed
intermediate scalar implementation as an exact scaling oracle. Automatic
parallelism uses at most three standard-library workers for the independent
color planes. Forced serial and parallel selected-pipeline results must match
each other and the explicit selected-parameter path byte for byte.

`SelectedQualityPipeline` enhances only the bicubic luma. Integer Sobel
orientation uses edge threshold 64 and axis ratio 2. Samples are refined along
the detected edge with Q8 gain 32, then receive a four-neighbor luma detail
term with Q8 gain 64. Every result is clamped to the original bicubic luma's
radius-one 3x3 envelope; chroma remains bicubic. All five Eval30 training folds
independently selected this ungated tuple for both local metrics.

The original `QualityPipeline` remains frozen and separately available to the
library and evaluator. `SelectedQualityPipeline` is tested against an explicit
call with `SELECTED_UNGATED_PARAMETERS` for exact equality.

`ConfidenceGatedQualityPipeline` starts from that selected ungated result and
blends its luma residual with an integer Q8 confidence. Confidence requires
three neighboring normal contrasts to have the same sign, penalizes tangent
intensity disagreement and contrast-profile changes, and ramps from zero at
evidence 8 to full residual at evidence 48. Flat or sign-incoherent regions
fall back to the bicubic luma. The existing local-envelope clamp remains.
Eval30 measured this gate below the selected ungated result, so it remains an
explicit failed experiment rather than a default path.

`BilinearChromaQualityPipeline` keeps the selected luma path byte-for-byte but
uses the project bilinear 2x scaler for Cb and Cr. Direct-composition tests lock
this separation and retain exact serial/parallel results. Eval30 measured the
candidate below selected ungated by `0.001765 dB` Y-PSNR and `0.000028696`
Y-SSIM, while local 1080p measurements found no repeatable throughput gain.
It remains an explicit failed experiment; public processing keeps bicubic
chroma.

The optional `RecommendedBaselineV1` is also excluded from the command-line
processing path. It is a deterministic development experiment based on the
organizer-supplied RGB-to-YUV, nearest-plus-convolution luma, and bilinear
chroma architecture. Its missing numerical details are fixed by project-local
assumption A-009. Eval30 evidence did not justify replacing the bicubic path.
