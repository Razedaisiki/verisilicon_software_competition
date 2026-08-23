# Offline DIV2K Pair Workspace

This directory is reserved for local, ignored DIV2K source and prepared data.
DIV2K assets are for academic research use under their upstream terms. No
source archive, PNG, generated PPM pair, or local hash lock is redistributed by
this repository.

Expected official archive roles and filenames are `DIV2K_train_HR.zip`,
`DIV2K_train_LR_bicubic_X2.zip`, `DIV2K_valid_HR.zip`, and
`DIV2K_valid_LR_bicubic_X2.zip`. Acquisition and extraction are user-controlled;
the repository provides no download automation.

The paired development contract is explicit:

- the default and current optimization input is exactly the 100 validation IDs
  `0801` through `0900`;
- the 800 train IDs `0001` through `0800` are unused by the current
  optimization and remain available only through explicit train/all selection;
- each HR PNG must be exactly twice its paired bicubic X2 LR PNG in each axis;
- test IDs `0901` through `1000` remain input-only and are never added to a
  paired manifest;
- source PNGs must pass the repository's strict noninterlaced RGB8 decoder.

Expected source layout under a user-supplied source root:

```text
DIV2K_train_HR/0001.png ... 0800.png
DIV2K_train_LR_bicubic/X2/0001x2.png ... 0800x2.png
DIV2K_valid_HR/0801.png ... 0900.png
DIV2K_valid_LR_bicubic/X2/0801x2.png ... 0900x2.png
```

Prepare and immediately verify the default 100 validation pairs on Windows:

```text
powershell -ExecutionPolicy Bypass -File .\prepare_div2k.ps1 -SourceDirectory path\to\DIV2K
```

Validate one fully acquired source split without publishing output:

```text
python scripts/div2k_pairs.py validate-sources path\to\DIV2K --split validation
```

The default output is `evaluation/div2k/local/prepared`. Preparation is
offline, atomic, deterministic, non-overwriting, and standard-library-only.
The default tree contains only `validation/pairs.tsv`, its 100 pairs, and locks
covering that selected split. It contains no train tree and no combined
evaluator manifest. `-Split Train` and `-Split All` are optional; use a distinct
output path for each non-default selection because existing output is refused.
See `docs/DATA_PREPARATION.md` for the complete validation and lock contract.
The first frozen-candidate aggregate is recorded in `evaluation/div2k/RESULTS.md`.
