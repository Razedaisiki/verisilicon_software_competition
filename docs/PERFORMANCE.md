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
