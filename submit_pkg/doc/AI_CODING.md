# AI Coding Process and Provenance

## Scope and Responsibility

AI assistance was used throughout requirements analysis, design, coding, test
generation, performance investigation, documentation, and packaging. The human
participant selected the competition direction, supplied organizer material,
approved each major step, and retained final submission responsibility. The
primary agent reviewed delegated work before accepting or committing it.

The Git history is the authoritative record of accepted source changes. Tests,
fixed evaluation manifests, reproducible builds, byte comparisons, and Windows
CI were used to verify AI-produced changes instead of accepting them solely
from natural-language output.

## Tools

| Tool | Use in this project |
| --- | --- |
| Codex desktop agent | Requirements review, repository work, implementation, code review, tests, documentation, Git, and packaging |
| `luna_worker` custom agent | Bounded delegated tasks with model `gpt-5.6-luna` and maximum reasoning effort; primary-agent review remained mandatory |
| Rust 1.85 and Cargo | Dependency-free runtime implementation, formatting, linting, tests, release builds, and dependency inspection |
| PowerShell | Windows-only repository build, dataset workflows, evaluation commands, and reproducibility checks |
| CMake | Organizer-listed submission build entry, exact Rust validation, offline reconstruction, and binary-identity enforcement |
| Python standard tooling | Development-only conversion, dataset validation, benchmark policy checks, and deterministic TAR creation |
| Git and GitHub Actions | One logical commit per accepted step and automated Windows verification |

No generated model code was copied into the project without repository-level
review. No C implementation or hardware-track Cmodel was included in the
software submission.

## Representative Key Prompts

The following are concise English translations or summaries of the important
human directions. They preserve the engineering intent; the complete original
conversation is supplied separately under `logs/`.

1. "Read the current contest PDF and code. Use Rust instead of C if the required
   input, output, and submission formats remain compatible."
2. "Remove the hardware Cmodel from the plan. Build the software structure and
   use explicit provisional assumptions until organizer details arrive."
3. "Maintain Windows only. Provide one-click build tooling and a precompiled
   `bin/sr.exe` for evaluation-machine standard testing."
4. "Support PPM P6 or organizer-defined packed RGB888. The scored raw input is
   1920x1080 and the program must produce a 2x 3840x2160 output."
5. "Automate evaluation using mean Y-channel PSNR and Y-channel SSIM, weighted
   equally, and keep a balanced local image database."
6. "Implement the organizer-recommended RGB-to-YUV, luma refinement, and chroma
   interpolation direction as a baseline, then compare it before adoption."
7. "Improve both image quality and FPS, account for a six-core local machine
   and an eight-core evaluation machine, and avoid excessive parameter runs."
8. "Limit the later validation comparison to 100 image pairs. Record every
   logical step in the changelog and commit each accepted step separately."
9. "Use the exact submission layout, keep only source needed to build
   `sr.exe`, and let the primary agent review every completed step."

## Iteration Summary

### 1. Requirements and interface alignment

The contest documents were separated into software-track requirements and an
unrelated hardware Cmodel. Rust was selected while preserving a native Windows
executable and the specified file interfaces. Provisional assumptions were
tracked explicitly, then updated when the organizer confirmed packed RGB888,
1920x1080 input, 2x output, and the absence of a separate bicubic coefficient
file.

Relevant milestones include `de55f81`, `efc6ef5`, `79c214f`, and `57d1e22`.

### 2. Deterministic baseline and runtime

The first runtime path added strict PPM P6 and packed RGB888 codecs, fixed-point
full-range YCbCr conversion, separable Catmull-Rom bicubic 2x scaling, and a
stable command-line interface. Single-file and batch modes were verified for
dimensions, byte counts, malformed input, repeatability, and failure codes.

Relevant milestones include `5bbbb7c`, `3157530`, `c642b10`, and `c29a383`.

### 3. Quality experiments and evaluation

A luma-only edge-adaptive pipeline was added with directional refinement,
sharpening, and local-envelope anti-ringing. The project then implemented
paired Y-PSNR and valid-window Y-MSSIM evaluation, a balanced 30-image
development catalog, stratified parameter search, and a separate 100-pair
validation comparison.

The organizer-recommended nearest-plus-convolution luma and bilinear-chroma
architecture was implemented as an isolated candidate. Confidence gating and
bilinear chroma variants were also isolated and measured. Candidates that did
not improve the selected evidence were retained only for reproducibility and
were not routed into `sr.exe`.

Relevant milestones include `8cd2d58`, `907b699`, `3bb4aef`, `10109c3`,
`9c9166f`, `d0cd691`, `9b878b8`, and `b393241`.

### 4. Performance engineering

The separable scaler was changed from a full horizontal intermediate plane to
a four-row cache without changing output bytes. Independent color planes gained
bounded threading, followed by adaptive batch frame workers and neighborhood
reuse. Checksums and serial/parallel equality tests guarded determinism while
local 1080p benchmarks supplied performance evidence.

Relevant milestones include `9c86ba9`, `3fca20d`, `296f776`, `ceae896`, and
`0352331`.

### 5. Submission hardening

The workflow was restricted to Windows, the precompiled executable role was
separated from the source build role, and deterministic uncompressed USTAR
creation was added. The extracted build must run offline and reproduce
`bin/sr.exe` byte for byte. Archive validation rejects unsafe paths, links,
unexpected files, non-ASCII source documents, non-PE binaries, and empty logs.
The final staging allowlist excludes developer examples and external tests.

Relevant milestones include `e21e719`, `81e8066`, `24237d1`, and `8fcbdc7`.

## Review and Validation Policy

Each accepted logical step received an English changelog entry and a separate
Git commit. The main review checks included:

- `cargo fmt --all -- --check`
- `cargo clippy --locked --all-targets --all-features -- -D warnings`
- `cargo test --locked --all-targets --all-features`
- Windows offline release builds with a locked dependency graph
- serial, parallel, and repeated-output byte equality
- fixed raw RGB888 byte-count and routing tests
- paired quality reports with deterministic manifests and output hashes
- staged-source rebuild comparison against `bin/sr.exe`
- deterministic uncompressed TAR creation and strict archive verification
- GitHub Actions confirmation after each accepted pushed step

## Conversation Logs

Complete AI conversation exports are mandatory final packaging inputs. They
must be placed under `submit_pkg/logs/`. The packaging tool rejects a missing
or empty log directory and copies the provided records without rewriting them.
No fake conversation log is included in the final submission. CI uses a file
explicitly labeled as synthetic only to test archive structure; that artifact
is never used as a submission record.
