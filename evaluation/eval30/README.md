# Eval30 Development Dataset

Eval30 is a local, reproducible 2x quality-evaluation dataset. It contains 30
locked source records split equally across `nature`, `render`, and `text_ui`.
The source images and generated PPM files are deliberately ignored by Git.

This is not an organizer dataset and is not evidence of official score
compatibility. It is a balanced local regression set for mean per-image
Y-PSNR and Y-SSIM comparisons under `docs/EVALUATION.md`.

## Contents

- `sources/*.json`: source page, direct download URL, author, license,
  dimensions, target crop, and locked SHA-256 for every image.
- `local/source/`: downloaded originals; ignored by Git.
- `local/prepared/hr/`: RGB8 PPM P6 references; ignored by Git.
- `local/prepared/lr/`: deterministic Pillow bicubic 2x inputs; ignored by Git.
- `local/prepared/pairs.tsv`: relative LR/HR pair manifest.
- `local/prepared/attribution.csv`: attribution carried into the local build.
- `local/prepared/dataset-lock.csv`: source, crop, HR, and LR hashes.

Twenty-nine references are 3840x2160. `render_05` is 2560x1440 because its
source is not tall enough for a 3840x2160 crop. Every LR image is exactly half
the corresponding HR width and height.

## Reproduce on Windows

Install the exact development-only image dependency:

```text
python -m pip install -r evaluation/requirements.txt
```

Validate the tracked lock, download or verify originals, and prepare pairs:

```text
python scripts/eval_dataset.py validate evaluation/eval30/sources --locked
python scripts/eval_dataset.py fetch evaluation/eval30/sources evaluation/eval30/local/source
python scripts/eval_dataset.py prepare evaluation/eval30/sources evaluation/eval30/local/source evaluation/eval30/local/prepared
python scripts/eval_dataset.py verify evaluation/eval30/sources evaluation/eval30/local/prepared
```

Preparation refuses to overwrite an existing output. Remove or rename the
local prepared directory explicitly when a fresh build is intended.
Verification independently checks exact tree membership, pair paths,
metadata, attribution, crop records, dimensions, PPM byte lengths, and all HR
and LR hashes.

## Deterministic preparation

The tool applies EXIF orientation, converts to RGB8, takes the exact centered
16:9 crop declared by each record without resizing the HR reference, and uses
Pillow 11.3.0 `Resampling.BICUBIC` to create the half-size LR input. It writes
strict PPM P6 with `maxval` 255.

The Python utility and Pillow are development tools only. Neither is part of
the Rust runtime or the official submission candidate.

## Licensing

The tracked catalogs are the attribution and provenance index. Images remain
subject to their individual licenses, and several UI screenshots mention
layered terms in `notes`. Do not redistribute the downloaded or prepared
dataset without reviewing and satisfying every applicable source-page term.
