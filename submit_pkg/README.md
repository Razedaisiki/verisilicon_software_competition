# Windows 2x Super-Resolution Submission

This directory is the Windows evaluation package for deterministic 2x RGB8
super resolution. `bin/sr.exe` is the precompiled standard-test executable.
`CMakeLists.txt` is the organizer-compatible source build and binary-identity
check.

## Package Contents

```text
submit_pkg/
|-- src/                 Rust source, Cargo manifest, and lockfile
|-- bin/sr.exe           Precompiled Windows evaluation executable
|-- CMakeLists.txt       CMake release build and byte comparison
|-- doc/ALGORITHM.md     Pipeline, module principles, and coefficient sources
|-- doc/AI_CODING.md     AI tools, key prompts, and iteration summary
|-- logs/                Complete exported AI conversation records
`-- README.md            Build, run, and command-line instructions
```

## Platform and Build

Declared Rust target: `x86_64-pc-windows-msvc`

Requirements are 64-bit Windows, CMake 3.20 or newer, exact Rust
1.85.0, Cargo, and the declared MSVC target. The project has no
third-party Rust dependencies. The release binary statically links the MSVC C
runtime. From `submit_pkg`, run:

```powershell
cmake -S . -B cmake-build
cmake --build cmake-build --config Release
```

CMake verifies the exact compiler identity, then runs Cargo with
`--offline --locked --release`, disables incremental compilation, enables
reproducible static-CRT MSVC linking, and remaps the source path. It writes the
rebuilt executable to `rebuilt/sr.exe` and compares every byte with `bin/sr.exe`.
A difference or build failure produces a nonzero exit status.

## Run the Precompiled Executable

Show the built-in command summary:

```powershell
.\bin\sr.exe --help
```

Process one PPM P6 image:

```powershell
.\bin\sr.exe input.ppm output.ppm
```

Process one official-size packed RGB888 image:

```powershell
.\bin\sr.exe input.raw output.raw
```

Process every supported file in one directory:

```powershell
.\bin\sr.exe --batch input_directory output_directory
```

The output directory is created when needed. Batch output keeps each input
filename because input and output are in separate directories.

## Command-Line Interface

```text
sr <input> <output>
sr --raw-rgb8 <width> <height> <input.raw> <output.raw>
sr --batch <in_dir> <out_dir>
sr --help
```

### Single-file mode

`sr <input> <output>` selects the format from the input extension. `.ppm`
selects strict PPM P6. `.raw` and `.rgb` select fixed packed RGB888 at
1920x1080. Unknown or missing extensions are accepted only when the complete
input can be identified unambiguously. The output encoding follows the input
format, not the output filename extension. Single-file output may replace an
existing file.

PPM input must use binary P6, `maxval` 255, and one packed RGB byte triplet per
pixel. Its dimensions are read from the header. Output dimensions are exactly
twice the input width and height.

Official raw input is row-major interleaved RGB888 with no header or row
padding. It is exactly 1920x1080x3 = 6,220,800 bytes. Output is exactly
3840x2160x3 = 24,883,200 bytes in the same channel order.

`--raw-rgb8` is an explicit variable-dimension developer interface. Width and
height must be positive decimal integers, and the input length must equal
`width * height * 3` bytes.

### Batch mode

`--batch` scans only the top level of the input directory for `.ppm`, `.raw`,
and `.rgb` files, sorts them by filename, skips unrelated files, and preserves
the input format and filename. It never overwrites an existing batch output.
It continues after per-file failures and reports failures in deterministic
filename order. Frame concurrency adapts to available logical processors and
is capped at eight workers to bound memory use.

### Exit statuses

| Status | Meaning |
| ---: | --- |
| 0 | Command completed successfully |
| 2 | Invalid command-line arguments |
| 4 | Input, output, format, processing, or batch failure |

## Technical Records

See `doc/ALGORITHM.md` for the selected processing pipeline, arithmetic,
module design, coefficient values, and provenance. See `doc/AI_CODING.md` for
the AI-assisted engineering process, representative prompts, review policy,
and iteration history. Complete exported conversations are stored in `logs/`.
