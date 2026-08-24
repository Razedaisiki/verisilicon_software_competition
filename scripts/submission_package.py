#!/usr/bin/env python3
"""Create, verify, or safely extract a deterministic Windows TAR candidate."""

from __future__ import annotations

import argparse
import io
import os
import re
import shutil
import sys
import tarfile
from pathlib import Path, PurePosixPath


PREFIX = "submit_pkg"
FIXED_MTIME = 0
FILE_MODE = 0o644
EXEC_MODE = 0o755
TARGET_PATTERN = re.compile(r"[A-Za-z0-9_.-]+")
SOURCE_FILES = (
    "Cargo.lock",
    "Cargo.toml",
    "src/algorithm.rs",
    "src/algorithm/bicubic.rs",
    "src/algorithm/color.rs",
    "src/algorithm/quality.rs",
    "src/algorithm/recommended.rs",
    "src/cli.rs",
    "src/fixtures.rs",
    "src/image.rs",
    "src/io.rs",
    "src/io/ppm.rs",
    "src/io/raw.rs",
    "src/lib.rs",
    "src/main.rs",
    "src/metrics.rs",
    "src/spec.rs",
)
DOC_FILES = {
    f"{PREFIX}/doc/ALGORITHM.md": "docs/ALGORITHM.md",
    f"{PREFIX}/doc/AI_CODING.md": "docs/AI_CODING.md",
}
FIXED_FILES = {
    *(f"{PREFIX}/src/{name}" for name in SOURCE_FILES),
    *DOC_FILES,
    f"{PREFIX}/bin/sr.exe",
    f"{PREFIX}/build.ps1",
    f"{PREFIX}/README.md",
}
BASE_DIRS = {
    PREFIX,
    f"{PREFIX}/bin",
    f"{PREFIX}/doc",
    f"{PREFIX}/logs",
    f"{PREFIX}/src",
}


def fail(message: str) -> None:
    raise ValueError(message)


def validate_archive_path(name: str) -> PurePosixPath:
    try:
        name.encode("ascii")
    except UnicodeEncodeError:
        fail(f"non-ASCII archive path: {name!r}")
    path = PurePosixPath(name)
    if path.is_absolute() or not path.parts or any(part in ("", ".", "..") for part in path.parts):
        fail(f"unsafe archive path: {name!r}")
    if path.parts[0] != PREFIX or "\\" in name:
        fail(f"path escapes {PREFIX}: {name!r}")
    return path


def validate_target(target: str) -> None:
    if TARGET_PATTERN.fullmatch(target) is None:
        fail(f"invalid Rust target triple: {target!r}")


def require_regular(path: Path, label: str) -> None:
    if path.is_symlink() or not path.is_file():
        fail(f"{label} is missing, is a symlink, or is not a regular file: {path}")


def require_safe_repository_file(root: Path, relative: str) -> Path:
    current = root
    for part in PurePosixPath(relative).parts:
        current = current / part
        if current.is_symlink():
            fail(f"source path contains a symlink: {relative}")
    require_regular(current, "required source")
    data = current.read_bytes()
    if b"\0" in data or any(byte > 0x7F for byte in data):
        fail(f"source payload is not English ASCII text: {relative}")
    return current


def collect_logs(logs_root: Path) -> dict[str, bytes]:
    if logs_root.is_symlink() or not logs_root.is_dir():
        fail(f"logs path must be a real directory, not a symlink: {logs_root}")
    payloads: dict[str, bytes] = {}
    for directory, directory_names, file_names in os.walk(logs_root, followlinks=False):
        base = Path(directory)
        for name in directory_names:
            if (base / name).is_symlink():
                fail(f"logs directory contains a symlink: {base / name}")
        for name in file_names:
            source = base / name
            require_regular(source, "log entry")
            relative = source.relative_to(logs_root).as_posix()
            archive_name = f"{PREFIX}/logs/{relative}"
            validate_archive_path(archive_name)
            payloads[archive_name] = source.read_bytes()
    if not payloads or sum(len(data) for data in payloads.values()) == 0:
        fail("logs directory must contain at least one non-empty exported conversation log")
    return payloads


def build_script(target: str) -> bytes:
    text = f'''$ErrorActionPreference = "Stop"
$packageRoot = $PSScriptRoot
$targetTriple = "{target}"
$manifest = Join-Path $packageRoot "src\\Cargo.toml"
$targetDir = Join-Path $packageRoot "build-target"
$candidate = Join-Path $targetDir "$targetTriple\\release\\sr.exe"
$rebuiltDir = Join-Path $packageRoot "rebuilt"
$rebuilt = Join-Path $rebuiltDir "sr.exe"
$packaged = Join-Path $packageRoot "bin\\sr.exe"
$separator = [char]0x1f
$previousOffline = $env:CARGO_NET_OFFLINE
$previousIncremental = $env:CARGO_INCREMENTAL
$previousRustFlags = $env:CARGO_ENCODED_RUSTFLAGS

try {{
    $env:CARGO_NET_OFFLINE = "true"
    $env:CARGO_INCREMENTAL = "0"
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        "-C",
        "link-arg=/Brepro",
        "--remap-path-prefix=$packageRoot\\src=."
    ) -join $separator
    cargo build --offline --locked --release --target $targetTriple --manifest-path $manifest --target-dir $targetDir
    if ($LASTEXITCODE -ne 0) {{
        throw "Cargo release build failed with exit code $LASTEXITCODE."
    }}
}} finally {{
    $env:CARGO_NET_OFFLINE = $previousOffline
    $env:CARGO_INCREMENTAL = $previousIncremental
    $env:CARGO_ENCODED_RUSTFLAGS = $previousRustFlags
}}
if (-not (Test-Path -LiteralPath $candidate -PathType Leaf)) {{
    throw "Expected rebuilt executable was not produced: $candidate"
}}
New-Item -ItemType Directory -Path $rebuiltDir -Force | Out-Null
Copy-Item -LiteralPath $candidate -Destination $rebuilt -Force
$expected = [IO.File]::ReadAllBytes($packaged)
$actual = [IO.File]::ReadAllBytes($rebuilt)
if ($expected.Length -ne $actual.Length) {{
    throw "Rebuilt executable length differs from packaged bin/sr.exe."
}}
for ($index = 0; $index -lt $expected.Length; $index++) {{
    if ($expected[$index] -ne $actual[$index]) {{
        throw "Rebuilt executable differs from packaged bin/sr.exe at byte $index."
    }}
}}
Write-Output "Offline rebuild is byte-identical to bin/sr.exe for $targetTriple."
'''
    return text.encode("ascii")


def package_readme(target: str) -> bytes:
    return f"""# Windows 2x Super-Resolution Submission

This directory is the Windows evaluation package for deterministic 2x RGB8
super resolution. `bin/sr.exe` is the precompiled standard-test executable.
`build.ps1` is the one-click source build and binary-identity check.

## Package Contents

```text
submit_pkg/
|-- src/                 Rust source, Cargo manifest, and lockfile
|-- bin/sr.exe           Precompiled Windows evaluation executable
|-- build.ps1            Offline one-click release build and byte comparison
|-- doc/ALGORITHM.md     Pipeline, module principles, and coefficient sources
|-- doc/AI_CODING.md     AI tools, key prompts, and iteration summary
|-- logs/                Complete exported AI conversation records
`-- README.md            Build, run, and command-line instructions
```

## Platform and Build

Declared Rust target: `{target}`

Requirements are 64-bit Windows, Rust 1.85.0, Cargo, and the declared MSVC
target. The project has no third-party Rust dependencies. From `submit_pkg`,
run:

```powershell
powershell -ExecutionPolicy Bypass -File .\\build.ps1
```

The script runs Cargo with `--offline --locked --release`, disables incremental
compilation, enables reproducible MSVC linking, and remaps the source path. It
writes the rebuilt executable to `rebuilt/sr.exe` and compares every byte with
`bin/sr.exe`. A difference or build failure produces a nonzero exit status.

## Run the Precompiled Executable

Show the built-in command summary:

```powershell
.\\bin\\sr.exe --help
```

Process one PPM P6 image:

```powershell
.\\bin\\sr.exe input.ppm output.ppm
```

Process one official-size packed RGB888 image:

```powershell
.\\bin\\sr.exe input.raw output.raw
```

Process every supported file in one directory:

```powershell
.\\bin\\sr.exe --batch input_directory output_directory
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
""".encode("ascii")


def add_directory(archive: tarfile.TarFile, name: str) -> None:
    info = tarfile.TarInfo(name)
    info.type = tarfile.DIRTYPE
    info.size = 0
    apply_metadata(info, EXEC_MODE)
    archive.addfile(info)


def add_file(archive: tarfile.TarFile, name: str, data: bytes, mode: int) -> None:
    info = tarfile.TarInfo(name)
    info.size = len(data)
    apply_metadata(info, mode)
    archive.addfile(info, io.BytesIO(data))


def apply_metadata(info: tarfile.TarInfo, mode: int) -> None:
    info.mode = mode
    info.mtime = FIXED_MTIME
    info.uid = 0
    info.gid = 0
    info.uname = ""
    info.gname = ""


def parent_directories(names: set[str]) -> set[str]:
    directories = set(BASE_DIRS)
    for name in names:
        parent = PurePosixPath(name).parent
        while str(parent) != ".":
            directories.add(parent.as_posix())
            parent = parent.parent
    return directories


def collect_fixed_payloads(binary: Path, target: str) -> dict[str, bytes]:
    validate_target(target)
    require_regular(binary, "Windows executable")
    if binary.name.lower() != "sr.exe" or binary.stat().st_size < 2:
        fail("explicit binary must be a non-empty file named sr.exe")
    if binary.read_bytes()[:2] != b"MZ":
        fail("explicit binary is not a Windows PE executable")
    root = Path(__file__).resolve().parent.parent
    payloads: dict[str, bytes] = {}
    for relative in SOURCE_FILES:
        source = require_safe_repository_file(root, relative)
        payloads[f"{PREFIX}/src/{relative}"] = source.read_bytes()
    for archive_name, relative in DOC_FILES.items():
        payloads[archive_name] = require_safe_repository_file(root, relative).read_bytes()
    payloads[f"{PREFIX}/bin/sr.exe"] = binary.read_bytes()
    payloads[f"{PREFIX}/build.ps1"] = build_script(target)
    payloads[f"{PREFIX}/README.md"] = package_readme(target)
    return payloads


def stage_package(destination: Path, binary: Path, target: str) -> None:
    if destination.name != PREFIX:
        fail(f"staging directory must be named {PREFIX}: {destination}")
    if destination.exists():
        fail(f"refusing to overwrite existing staging directory: {destination}")
    payloads = collect_fixed_payloads(binary, target)
    directories = parent_directories(set(payloads))
    destination.mkdir(parents=True)
    try:
        for archive_name in sorted(directories - {PREFIX}):
            relative = PurePosixPath(archive_name).relative_to(PREFIX)
            destination.joinpath(*relative.parts).mkdir()
        for archive_name, data in sorted(payloads.items()):
            relative = PurePosixPath(archive_name).relative_to(PREFIX)
            destination.joinpath(*relative.parts).write_bytes(data)
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    print(f"Staged Windows submission directory without conversation logs: {destination}")


def create_package(output: Path, binary: Path, logs: Path, target: str) -> None:
    if output.suffix.lower() != ".tar":
        fail("output path must end in .tar")
    if output.exists():
        fail(f"refusing to overwrite existing package: {output}")
    payloads = collect_fixed_payloads(binary, target)
    payloads.update(collect_logs(logs))
    directories = parent_directories(set(payloads))
    entries = sorted(directories | set(payloads))
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with output.open("xb") as stream:
            with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as archive:
                for name in entries:
                    validate_archive_path(name)
                    if name in directories:
                        add_directory(archive, name)
                    else:
                        mode = EXEC_MODE if name in (f"{PREFIX}/bin/sr.exe", f"{PREFIX}/build.ps1") else FILE_MODE
                        add_file(archive, name, payloads[name], mode)
    except Exception:
        output.unlink(missing_ok=True)
        raise
    print(f"Created Windows submission candidate: {output}")


def read_target(archive: tarfile.TarFile) -> str:
    readme = archive.extractfile(f"{PREFIX}/README.md")
    if readme is None:
        fail("submission README is not a regular file")
    text = readme.read().decode("ascii")
    marker = "Declared Rust target: `"
    lines = [line for line in text.splitlines() if line.startswith(marker) and line.endswith("`")]
    if len(lines) != 1:
        fail("submission README has no unique declared Rust target")
    target = lines[0][len(marker):-1]
    validate_target(target)
    if text.encode("ascii") != package_readme(target):
        fail("submission README does not match the generated template")
    return target


def verify_package(package: Path) -> None:
    if package.suffix.lower() != ".tar" or not package.is_file():
        fail(f"package must be an existing uncompressed .tar file: {package}")
    with tarfile.open(package, mode="r:") as archive:
        members = archive.getmembers()
        names = [member.name for member in members]
        if len(names) != len(set(names)):
            fail("archive contains duplicate paths")
        if len(names) != len({name.casefold() for name in names}):
            fail("archive contains paths that collide on Windows")
        if names != sorted(names):
            fail("archive entries are not sorted")
        for name in names:
            validate_archive_path(name)
        file_names = {member.name for member in members if member.isfile()}
        directory_names = {member.name for member in members if member.isdir()}
        if any(not (member.isfile() or member.isdir()) for member in members):
            fail("archive contains a link or special entry")
        log_files = {name for name in file_names if name.startswith(f"{PREFIX}/logs/")}
        if not log_files:
            fail("archive has no exported conversation logs")
        expected_files = set(FIXED_FILES) | log_files
        if file_names != expected_files:
            fail(f"archive file membership mismatch: {sorted(file_names ^ expected_files)}")
        expected_directories = parent_directories(file_names)
        if directory_names != expected_directories:
            fail(f"archive directory membership mismatch: {sorted(directory_names ^ expected_directories)}")
        for member in members:
            expected_mode = EXEC_MODE if member.isdir() or member.name in (f"{PREFIX}/bin/sr.exe", f"{PREFIX}/build.ps1") else FILE_MODE
            if (member.mode, member.mtime, member.uid, member.gid, member.uname, member.gname) != (expected_mode, FIXED_MTIME, 0, 0, "", ""):
                fail(f"unexpected metadata: {member.name}")
            if member.pax_headers:
                fail(f"extended TAR metadata is not allowed: {member.name}")
        target = read_target(archive)
        build = archive.extractfile(f"{PREFIX}/build.ps1")
        if build is None or build.read() != build_script(target):
            fail("build.ps1 does not match the declared-target template")
        binary = archive.extractfile(f"{PREFIX}/bin/sr.exe")
        if binary is None or binary.read(2) != b"MZ":
            fail("packaged sr.exe is not a Windows PE executable")
        if sum(member.size for member in members if member.name in log_files) == 0:
            fail("packaged conversation logs are all empty")
        for name in set(FIXED_FILES) - {f"{PREFIX}/bin/sr.exe"}:
            extracted = archive.extractfile(name)
            if extracted is None:
                fail(f"expected text payload is not regular: {name}")
            data = extracted.read()
            if b"\0" in data or any(byte > 0x7F for byte in data):
                fail(f"submission source/document is not ASCII text: {name}")
    print(f"Verified Windows submission candidate: {package}")


def extract_package(package: Path, destination: Path) -> None:
    verify_package(package)
    if destination.exists():
        fail(f"refusing to overwrite extraction destination: {destination}")
    destination.mkdir(parents=True)
    try:
        with tarfile.open(package, mode="r:") as archive:
            for member in archive.getmembers():
                target = destination.joinpath(*PurePosixPath(member.name).parts)
                if member.isdir():
                    target.mkdir()
                else:
                    source = archive.extractfile(member)
                    if source is None:
                        fail(f"cannot extract non-regular entry: {member.name}")
                    target.write_bytes(source.read())
                os.chmod(target, member.mode)
    except Exception:
        shutil.rmtree(destination, ignore_errors=True)
        raise
    print(f"Extracted Windows submission candidate: {destination}")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)
    create = commands.add_parser("create")
    create.add_argument("package", type=Path)
    create.add_argument("--binary", type=Path, required=True)
    create.add_argument("--logs", type=Path, required=True)
    create.add_argument("--target", required=True)
    stage = commands.add_parser("stage")
    stage.add_argument("destination", type=Path)
    stage.add_argument("--binary", type=Path, required=True)
    stage.add_argument("--target", required=True)
    verify = commands.add_parser("verify")
    verify.add_argument("package", type=Path)
    extract = commands.add_parser("extract")
    extract.add_argument("package", type=Path)
    extract.add_argument("destination", type=Path)
    return result


def main() -> int:
    args = parser().parse_args()
    try:
        if args.command == "create":
            create_package(args.package, args.binary, args.logs, args.target)
        elif args.command == "stage":
            stage_package(args.destination, args.binary, args.target)
        elif args.command == "verify":
            verify_package(args.package)
        else:
            extract_package(args.package, args.destination)
    except (OSError, ValueError, tarfile.TarError, UnicodeError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
