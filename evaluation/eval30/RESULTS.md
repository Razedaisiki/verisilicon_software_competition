# Eval30 Reference Result

This record captures the first complete local Eval30 comparison. It is not an
official score and does not define the organizer's unknown cross-metric
normalization.

## Dataset averages

| Pipeline | Mean Y-PSNR (dB) | Mean Y-SSIM |
| --- | ---: | ---: |
| Baseline | 32.871287 | 0.927772122 |
| Candidate | 32.900263 | 0.927909630 |
| Candidate minus baseline | +0.028976 | +0.000137508 |

Every value above is the equal-image mean across all 30 locked pairs. The
candidate is locally ahead on both complete-dataset diagnostics, but the
margins are small and do not establish official superiority.

## Category diagnostics

These category values are derived from the displayed per-image report values.
They are diagnostic only and are not additional score components.

| Category | Pipeline | Mean Y-PSNR (dB) | Mean Y-SSIM |
| --- | --- | ---: | ---: |
| Nature | Baseline | 30.893626 | 0.864374861 |
| Nature | Candidate | 30.867199 | 0.863925609 |
| Render | Baseline | 37.676061 | 0.954509286 |
| Render | Candidate | 37.737117 | 0.954800042 |
| Text/UI | Baseline | 30.044173 | 0.964432218 |
| Text/UI | Candidate | 30.096472 | 0.965003239 |

The current candidate regresses slightly on the nature subset and improves on
the render and text/UI subsets. Algorithm tuning is intentionally outside this
evaluation-automation milestone.

## Reproduction evidence

- Windows 11 10.0.26200
- AMD Ryzen 5 5500U with Radeon Graphics
- Rust 1.85.0 release build
- Complete-run elapsed times: 49.642 seconds and 50.233 seconds
- Both reports were byte-identical
- Report SHA-256: `309162abaa3ac4925542bbaea417a1a14faec671746c1e49a3b07e317d0ef816`

The CSV itself remains a generated local artifact under `target/` and is not
committed. Recreate it with `evaluate.ps1`.

## RecommendedBaselineV1 experiment

The organizer reference architecture was instantiated with the project-local
A-009 arithmetic and selected explicitly with:

```text
powershell -ExecutionPolicy Bypass -File .\evaluate.ps1 -Baseline recommended -Report target/eval30-recommended-v1.csv
```

| Pipeline | Mean Y-PSNR (dB) | Mean Y-SSIM |
| --- | ---: | ---: |
| Bicubic baseline | 32.871287 | 0.927772122 |
| RecommendedBaselineV1 | 31.004843 | 0.912974736 |
| Quality candidate | 32.900263 | 0.927909630 |

RecommendedBaselineV1 minus bicubic is `-1.866444 dB` and `-0.014797386`
Y-SSIM. The candidate minus RecommendedBaselineV1 report delta is
`+1.895419 dB` and `+0.014934894` Y-SSIM. The repeated recommended reports
were byte-identical with SHA-256
`fbed4df60bd856bc737abff79b61227ce0f625b2e3eda40ae71200ce78bea47b`.

The experiment is worse than the existing bicubic anchor in all three content
categories. It therefore remains an explicit evaluation option and does not
replace the public runtime baseline.
