# Provisional Assumptions

These assumptions enable implementation planning before all committee package details are available. They are not official contest facts. Each assumption has a replacement boundary so that later clarification remains localized and reviewable.

## A-001: PPM P6 uses 8-bit samples

- Provisional rule: accept and emit binary PPM P6 with `maxval` 255 and one byte per RGB sample.
- Reason: the software scope requires RGB8, and this is the smallest interoperable PPM P6 representation.
- Replacement boundary: isolate PPM parsing and encoding behind the image I/O layer. If the committee package specifies other header or sample rules, replace only that layer and its format tests.

## A-002: Raw input is packed RGB8

- Provisional rule: raw images contain tightly packed, row-major RGB8 pixels in R, G, B order with no header or row padding. Dimensions come from the committee context or batch configuration.
- Reason: the authoritative raw layout is not yet available.
- Replacement boundary: keep raw layout selection in a format descriptor. Replace channel order, byte order, stride, dimensions, or metadata handling without changing the processing algorithm.

## A-003: The project pipeline uses BT.601 full-range fixed-point math

- Provisional rule: the project processing pipeline uses a documented BT.601 full-range fixed-point RGB and YUV conversion with explicit coefficients, rounding, and clipping.
- Reason: integer arithmetic supports deterministic CPU execution and avoids hidden library behavior.
- Replacement boundary: keep color transforms in a dedicated module. Replace coefficients, range, rounding, or color space only through a versioned pipeline update with fixed test vectors.

## A-004: Scale is fixed at 2x

- Provisional rule: every public processing path doubles width and height.
- Reason: the confirmed software scope maps primary 1920 by 1080 RGB8 input to 3840 by 2160 RGB8 output.
- Replacement boundary: keep scale explicit in internal pipeline configuration, but do not publish other scale factors unless the committee requirements permit them.

## A-005: Bicubic uses a equals -0.5

- Provisional rule: the initial admission baseline uses separable bicubic interpolation with cubic parameter `a = -0.5`.
- Reason: it provides a deterministic, dependency-free baseline while exact committee coefficients and data are missing.
- Replacement boundary: isolate the baseline kernel, coordinate mapping, border policy, and rounding. Replace them together when the official baseline definition arrives.

## A-006: Timing measures processing only

- Provisional rule: initial performance measurements exclude process startup and file I/O. They include conversion from decoded RGB8, required pipeline allocation, super-resolution processing, and production of the in-memory RGB8 output.
- Reason: this isolates algorithm performance while the official timing API and platform are missing.
- Replacement boundary: centralize timing boundaries in the benchmark harness. Add the official measurement path without deleting diagnostic sub-measurements.

## Change control

- Do not spread provisional values across CLI, I/O, algorithm, and benchmark code.
- Link each affected implementation and test to its assumption ID until an official requirement replaces it.
- When replacing an assumption, update `docs/REQUIREMENTS.md`, this file, tests, user documentation, and `CHANGELOG.md` in one atomic change.
- Keep all repository artifacts and commit messages in English ASCII.
