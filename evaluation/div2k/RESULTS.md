# DIV2K Validation Reference Result

This record captures the first local 100-image DIV2K validation comparison.
It is development evidence, not an organizer score and not the hidden contest
dataset.

The selected candidate had already been frozen from Eval30 before this DIV2K
validation run. IDs `0801` through `0900` were used only as one held-out
acceptance set; the 800 train images were not used for this optimization.

## Dataset averages

| Pipeline | Mean Y-PSNR (dB) | Mean Y-SSIM |
| --- | ---: | ---: |
| Bicubic baseline | 31.101564 | 0.894103940 |
| Selected ungated | 31.270450 | 0.897669936 |
| Selected minus baseline | +0.168886 | +0.003565996 |

Every value is the equal-image mean across all 100 validation pairs under the
local metric contract in `docs/EVALUATION.md`. Both deltas are positive, but
the result does not establish equivalence with unknown organizer metric
details or performance on the hidden contest set.

## Reproduction evidence

```text
cargo run --offline --locked --release --example paired_eval -- evaluation/div2k/local/prepared/validation/pairs.tsv target/div2k-validation-selected.csv bicubic selected-ungated
```

- Windows 11 10.0.26200
- AMD Ryzen 5 5500U with Radeon Graphics
- Rust 1.85.0 release build
- Repeat evaluator run: 55.664 seconds wall time
- Both generated reports were byte-identical
- Report SHA-256: `706da5cc884ee6a60767eed4f87487790be7552393176ea2aa5e60dbed96ad54`

The source archives, PNG files, prepared PPM files, per-image report, and
source/output locks remain ignored local artifacts and are not redistributed.
