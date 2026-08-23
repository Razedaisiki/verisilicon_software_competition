# Submission Algorithm Description

The command-line processing path uses the repository's deterministic
`BicubicBaseline`. It is a project-local development and evaluation anchor. It
is not claimed to be byte-equivalent to the organizer's hidden public-bicubic
LR generation or to an organizer reference implementation.

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
the final plane sample to 0 through 255. Scaled Y, Cb, and Cr are converted
back to RGB8 with the fixed inverse formulas and clipping in
`src/algorithm/color.rs`.

The optimized scaler caches four horizontal rows. Tests retain a full signed
intermediate scalar implementation as an exact oracle. Automatic parallelism
uses at most three standard-library workers for the independent color planes;
serial and parallel results are required to match the oracle byte for byte.

The optional `QualityPipeline` is not selected by the command-line interface
and is not part of this submitted processing path.

The optional `RecommendedBaselineV1` is also excluded from the command-line
processing path. It is a deterministic development experiment based on the
organizer-supplied RGB-to-YUV, nearest-plus-convolution luma, and bilinear
chroma architecture. Its missing numerical details are fixed by project-local
assumption A-009. Eval30 evidence did not justify replacing the bicubic path.
