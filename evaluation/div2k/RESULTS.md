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

## Fine-finalist acceptance

After the separate Eval30 fine sweep selected `64/2/24/80`, exactly that one
fixed finalist was compared on the same 100 validation pairs. No train images
or additional parameter candidates were evaluated.

| Pipeline | Mean Y-PSNR (dB) | Mean Y-SSIM |
| --- | ---: | ---: |
| Bicubic baseline | 31.101564 | 0.894103940 |
| Previous selected `64/2/32/64` | 31.270450 | 0.897669936 |
| Fine finalist `64/2/24/80` | 31.322210 | 0.898506375 |
| Finalist minus previous selected | +0.051761 | +0.000836439 |
| Finalist minus baseline | +0.220647 | +0.004402435 |

The baseline rows in the previous and finalist reports match for all 100
images. The finalist improves both metrics over the previous selection on all
100 images; there are no per-image ties or regressions in either metric. This
accepts the finalist for a separate promotion milestone. The validation set is
now consumed for this selection and must not be reused to tune another search.

```text
cargo run --offline --locked --release --example paired_eval -- evaluation/div2k/local/prepared/validation/pairs.tsv target/div2k-fine-finalist-report.csv bicubic fine-finalist
```

- Evaluator wall time including the release-build check: 59.7 seconds
- Report SHA-256: `b0a491e7241df3968f812d839a85e7b57be310b4e4d89de4ecc8e4e0c338cdbc`
