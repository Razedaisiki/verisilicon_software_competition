# Performance Notes

These measurements are local diagnostics, not official contest results or
portable performance claims.

## Scalar optimization

The 2x bicubic scaler retains the original coefficients, half-pixel phases,
border clamping, signed horizontal values, Q14 rounding, and final clipping.
The original implementation materialized every horizontal Q7 sample before
the vertical pass. The optimized implementation keeps only the four horizontal
rows needed by the current vertical phase and reuses them as the output advances.
Exact regression tests compare both implementations on fixed vectors,
single-pixel and thin images, odd dimensions, gradients, edges, and checker
detail. The baseline and quality pipelines are also compared repeatedly.

For an input plane of width `W` and height `H`, the old horizontal working set
was `2 * W * H * 4` bytes. The row cache is `4 * 2 * W * 4`, or `32 * W`
bytes. At 640x360 this changes the horizontal working set per scaler call from
1,843,200 bytes to 20,480 bytes, a 98.9% reduction. Output storage is unchanged.
Source planes and the quality pipeline's temporary bicubic luma are explicitly
dropped after their last use. These are allocation-lifetime calculations; no
process RSS claim is made.

## Processing-only benchmark

The benchmark creates a deterministic smooth gradient in memory, performs one
unmeasured warm-up, and measures only calls to the selected algorithm. Decode,
encode, fixture construction, and reporting are outside the timed interval.
Run it with:

```text
cargo run --locked --release --example processing_bench -- baseline auto 640 360 5
cargo run --locked --release --example processing_bench -- quality auto 640 360 5
```

The output includes elapsed time, throughput, and a deterministic checksum.
For less sensitivity to host noise, run the command at least three times and
compare medians.

Local measurements on 2026-08-23 used an AMD Ryzen 5 5500U, Windows MSVC,
Rust 1.85.0, a 640x360 input, a 1280x720 output, one warm-up, five measured
iterations, and three process runs:

All elapsed values in the table are totals for the five measured frames, not
per-frame latency.

| Pipeline | Before 5-frame median | After 5-frame median | After 5-frame range | Local median change |
| --- | ---: | ---: | ---: | ---: |
| Baseline | 236.721 ms | 163.539 ms | 150.960-168.420 ms | 30.9% lower |
| Quality | 278.758 ms | 272.389 ms | 218.953-308.431 ms | 2.3% lower |

The ranges show substantial host noise, especially for the quality pipeline.
The result supports the smaller working set and a local baseline improvement;
it does not establish performance on other machines or workloads.

## Threading policy

Automatic execution uses three independent channel branches: Y, Cb, and Cr.
The quality Y branch performs both scaling and luma enhancement in its worker.
The cap is three standard-library workers, with no row-level or pixel-level
thread creation. Automatic parallel execution requires an input of at least
131,072 pixels and `available_parallelism()` of at least two; otherwise it is
serial. Explicit serial and parallel policies exist for diagnostics and exact
regression testing.

Workers are created with `thread::Builder::spawn_scoped`, making a spawn error
representable as `AlgorithmError::ThreadSpawnFailed`. Every successfully
spawned handle is explicitly joined before the scope returns, including after
a partial spawn failure or worker panic. A joined panic becomes
`AlgorithmError::WorkerPanicked`, so the scope does not unwind and no partial
image is returned. If failures coincide, the stable precedence is spawn
failure, worker panic, then a worker-returned algorithm error.

Forced serial and parallel tests compare exact `Image` equality against the
retained full-intermediate oracle. Coverage includes constants, gradients,
horizontal and vertical edges, checker detail, odd dimensions, thin images,
1x1 images, and three repeated runs for both pipelines. Focused collector tests
inject a panic and a simulated partial spawn failure, verify all successful
handles complete, and verify the stable public error variants.

## Local threading measurements

The same 2026-08-23 host and processing-only benchmark were used to force both
policies. Each cell is the median of three process runs. The 640x360 cases use
five measured frames per run; the 1280x720 cases use three. Elapsed values are
totals for those frames.

| Input | Pipeline | Serial median | Parallel median | Local median change |
| --- | --- | ---: | ---: | ---: |
| 640x360 | Baseline | 149.616 ms | 79.367 ms | 47.0% lower |
| 640x360 | Quality | 266.046 ms | 155.406 ms | 41.6% lower |
| 1280x720 | Baseline | 298.495 ms | 182.326 ms | 38.9% lower |
| 1280x720 | Quality | 504.850 ms | 343.295 ms | 32.0% lower |

All serial and parallel runs produced the same checksum at each resolution:
`fd3181d6999db271` at 640x360 and `7089b2055a59146d` at 1280x720. These local
results do not imply a universal speedup; scheduler load, core topology, image
content, and platform thread costs can change the result.

## Provisional 1 FPS evidence

The 1 FPS requirement remains provisional until the official timing boundary,
platform, workload, and scoring tools are available. The following is local
evidence only, not a compliance result.

The host was an AMD Ryzen 5 5500U with Radeon Graphics running 64-bit Windows
11 version 10.0.26200. `available_parallelism()` reported 12. The build used
`rustc 1.85.0 (4d91de4e4 2025-02-17)`, the x86_64-pc-windows-msvc target, and
LLVM 19.1.7. Each release run scaled a deterministic 1920x1080 gradient to
3840x2160, used Auto policy, performed one unmeasured warm-up, and measured
three processing-only frames. Auto selected parallel. Five independent process
runs produced:

| Pipeline | Elapsed totals for three frames (ms) | Reported FPS | Median FPS | Checksum |
| --- | --- | --- | ---: | --- |
| Baseline | 494.732, 462.229, 412.661, 539.952, 421.409 | 6.064, 6.490, 7.270, 5.556, 7.119 | 6.490 | `98e3c40731c269e9` |
| Quality | 729.118, 798.859, 882.504, 819.258, 759.724 | 4.115, 3.755, 3.399, 3.662, 3.949 | 3.755 | `98e3c40731c269e9` |

All runs were above 1 FPS on this host. This does not establish performance on
the unknown official platform or establish that the provisional timing boundary
matches the official one.

## SIMD evidence and decision

The current release library was also built with:

```text
cargo rustc --locked --release --lib -- --emit=asm
```

Generated assembly remained under ignored `target/` paths. Inspection of the
emitted `scale_plane_2x` and `enhance_luma` function bodies found scalar byte
loads and integer arithmetic. The scaler body used XMM only for zeroing or
moving state; it contained no packed arithmetic. The enhancement body contained
no XMM, YMM, or ZMM instructions. Therefore this Rust 1.85.0 generic x86_64
release build provides no evidence that the hot arithmetic loops are
auto-vectorized. This conclusion is limited to the inspected host, target, and
compiler configuration.

No hand-written `std::arch` SIMD path is added. The official CPU and toolchain
are unknown, while a correct implementation would need architecture guards,
runtime feature detection, scalar fallback, unsafe intrinsic code, exact
fixed-point equivalence across borders and tails, and Windows and Linux build
coverage. The existing safe threaded scalar path already exceeds the provisional
target locally, so those risks are not justified by current evidence.

Reconsider intrinsics if the official CPU/toolchain is specified or if the
current implementation fails the official target. Any future SIMD path must be
narrowly dispatched, preserve the scalar fallback, match the scalar oracle
exactly for all dimensions and tail widths, compile on every CI target, and show
a material repeatable improvement over Auto threaded scalar processing.

## CI benchmark correctness

`scripts/check_processing_bench.py` runs the release benchmark for both
pipelines with forced serial and parallel policies on an 8x5 image. It checks
the requested and selected policies, dimensions, fixed checksums, and exact
serial/parallel checksum equality. CI does not inspect elapsed time or enforce a
wall-clock threshold.
