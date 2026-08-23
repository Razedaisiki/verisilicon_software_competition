# Provisional Assumptions

These assumptions enable implementation planning before all committee package details are available. They are not official contest facts. Each assumption has a replacement boundary so that later clarification remains localized and reviewable.

## A-001: PPM P6 uses 8-bit samples

- Provisional rule: accept and emit binary PPM P6 with `maxval` 255 and one byte per RGB sample.
- Reason: the software scope requires RGB8, and this is the smallest interoperable PPM P6 representation.
- Replacement boundary: isolate PPM parsing and encoding behind the image I/O layer. If the committee package specifies other header or sample rules, replace only that layer and its format tests.

## A-002: Organizer-confirmed fixed packed RGB888 working contract

- Confirmed working rule: official raw input is fixed 1920x1080, tightly packed
  row-major RGB888 in R, G, B byte order, with top-to-bottom rows, no header, and
  no stride padding. Input is exactly 6,220,800 bytes. Output is fixed
  3840x2160 in the same layout and exactly 24,883,200 bytes.
- Source: direct organizer clarification supplied during implementation. A
  versioned written package statement is still requested for final audit.
- Interface rule: two-argument `.raw` or `.rgb` input uses these fixed
  dimensions. The explicit `--raw-rgb8 <width> <height>` command remains a
  variable-dimension developer diagnostic path.
- Replacement boundary: raw geometry and layout constants remain centralized.
  Reconcile them only if a versioned organizer document contradicts the working
  contract.

## A-003: The project pipeline uses BT.601 full-range fixed-point math

- Provisional rule: the project processing pipeline uses a documented BT.601 full-range fixed-point RGB and YUV conversion with explicit coefficients, rounding, and clipping.
- Reason: integer arithmetic supports deterministic CPU execution and avoids hidden library behavior.
- Replacement boundary: keep color transforms in a dedicated module. Replace coefficients, range, rounding, or color space only through a versioned pipeline update with fixed test vectors.

## A-004: Scale is fixed at 2x

- Provisional rule: every public processing path doubles width and height.
- Reason: the confirmed software scope maps primary 1920 by 1080 RGB8 input to 3840 by 2160 RGB8 output.
- Replacement boundary: keep scale explicit in internal pipeline configuration, but do not publish other scale factors unless the committee requirements permit them.

## A-005: Local bicubic anchor uses a equals -0.5

- Project-local rule: development comparisons use separable Catmull-Rom bicubic interpolation with cubic parameter `a = -0.5`.
- Reason: it provides a deterministic dependency-free anchor. The organizer clarified that no additional coefficient file will be supplied.
- Non-equivalence rule: this anchor is not claimed to reproduce the organizer's hidden bicubic LR generation byte for byte.
- Replacement boundary: keep the kernel, coordinate mapping, border policy, and rounding isolated. Change them only for a measured algorithm decision or versioned organizer reference evidence.

## A-006: Timing measures processing only

- Provisional rule: initial performance measurements exclude process startup and file I/O. They include conversion from decoded RGB8, required pipeline allocation, super-resolution processing, and production of the in-memory RGB8 output.
- Reason: this isolates algorithm performance while the official timing API and platform are missing.
- Replacement boundary: keep the timing boundary centralized around the algorithm call. Add the official measurement path without deleting diagnostic sub-measurements.

## A-007: Batch processing is non-recursive and non-overwriting

- Provisional rule: batch mode selects regular `.ppm`, `.raw`, and `.rgb` files
  ASCII case-insensitively. It does not recurse. Candidates are processed in
  deterministic filename order, unrelated entries are skipped, filenames and
  input formats are preserved, and the output directory is created when
  candidates exist. Raw candidates use the fixed A-002 geometry.
- Failure rule: existing outputs are never overwritten. Processing continues
  after per-file failures, diagnostics retain candidate order, and the command
  fails if any candidate fails. Success requires at least one completed image
  and no failures. An input directory with no candidates is an error.
- Reason: these rules provide deterministic and recoverable behavior while official batch details are missing.
- Replacement boundary: isolate discovery, ordering, overwrite, continuation, and status policies in the batch coordinator. Replace them together if the committee package defines different behavior.

## A-008: Quality metrics are provisional diagnostics

- Provisional rule: diagnostic PSNR and SSIM operate on deterministic fixed-point BT.601 luma and require equal image dimensions. Identical luma images return explicit infinite PSNR.
- PSNR definition: `10 * log10(255^2 / MSE)`, using population mean squared luma error.
- SSIM definition: one global population-statistics window over the complete luma image, using `((2 mx my + C1) (2 covariance + C2)) / ((mx^2 + my^2 + C1) (variance_x + variance_y + C2))`. Constants are `L = 255`, `K1 = 0.01`, `K2 = 0.03`, `C1 = 6.5025`, and `C2 = 58.5225`.
- Reason: deterministic local diagnostics are needed for regression testing and visual QA while the official metric implementation and dataset are unavailable.
- Replacement boundary: keep diagnostic metrics separate from official score reporting. Add the committee implementation as a distinct evaluation path and do not silently reinterpret existing regression thresholds.

## A-009: Recommended architecture has project-selected fixed-point details

- Organizer guidance: the supplied reference slide presents a proven starting
  architecture that converts RGB to YUV, applies nearest-neighbor 2x plus a
  3x3 luma sharpening convolution, applies bilinear or polyphase interpolation
  to chroma, then converts back to RGB.
- Missing numerical details: the slide does not specify the YUV matrix or
  range, convolution coefficients, coordinate mapping, border policy,
  rounding, clipping, or anti-ringing rule. It also labels bicubic, Lanczos,
  edge-adaptive, and reconstruction methods as possible directions rather than
  defining one mandatory byte-exact baseline.
- Project-local V1 rule: retain A-003 color conversion; use nearest-neighbor
  luma 2x, Q8 four-neighbor Laplacian gain 32 with a clamped 3x3 local
  envelope, and separable half-pixel Q8 bilinear chroma with phase weights 64
  and 192.
- Non-equivalence rule: `RecommendedBaseline` is an experimental deterministic
  instance of the suggested architecture, not a claim of organizer byte
  equivalence and not the public runtime default until measured and reviewed.
- Replacement boundary: keep the complete implementation in its own pipeline.
  Change coefficients or select it publicly only in a separate measured
  algorithm decision.

## Change control

- Do not spread provisional values across CLI, I/O, algorithm, and benchmark code.
- Link each affected implementation and test to its assumption ID until an official requirement replaces it.
- When replacing an assumption, update `docs/REQUIREMENTS.md`, this file, tests, user documentation, and `CHANGELOG.md` in one atomic change.
- Keep all repository artifacts and commit messages in English ASCII.
