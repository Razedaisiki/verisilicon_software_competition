#!/usr/bin/env python3
"""Prepare and verify deterministic offline DIV2K HR/LR pairs."""

from __future__ import annotations

import argparse
import concurrent.futures
import csv
import hashlib
import io
import json
import os
import re
import shutil
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path, PurePosixPath

from convert_div2k import DecodedPng, decode_png


class Div2kError(ValueError):
    """Stable validation error for offline DIV2K preparation."""


@dataclass(frozen=True)
class SplitSpec:
    name: str
    first_id: int
    last_id: int
    hr_directory: str
    lr_directory: str

    def ids(self) -> tuple[str, ...]:
        return tuple(f"{value:04d}" for value in range(self.first_id, self.last_id + 1))


TRAIN_SPLIT = SplitSpec(
    "train",
    1,
    800,
    "DIV2K_train_HR",
    "DIV2K_train_LR_bicubic/X2",
)
VALIDATION_SPLIT = SplitSpec(
    "validation",
    801,
    900,
    "DIV2K_valid_HR",
    "DIV2K_valid_LR_bicubic/X2",
)
DIV2K_SPLITS = (TRAIN_SPLIT, VALIDATION_SPLIT)
DEFAULT_SPLITS = (VALIDATION_SPLIT,)
TEST_IDS = tuple(f"{value:04d}" for value in range(901, 1001))
LOCK_HEADER = [
    "split",
    "id",
    "hr_source_sha256",
    "hr_source_width",
    "hr_source_height",
    "lr_source_sha256",
    "lr_source_width",
    "lr_source_height",
    "hr_ppm_sha256",
    "lr_ppm_sha256",
]


@dataclass(frozen=True)
class SourcePair:
    split: str
    image_id: str
    hr_path: Path
    lr_path: Path

    def hr_relative(self) -> str:
        return f"{self.split}/hr/{self.image_id}.ppm"

    def lr_relative(self) -> str:
        return f"{self.split}/lr/{self.image_id}.ppm"

    def evaluator_id(self) -> str:
        return f"div2k_{self.split}_{self.image_id}"

    def manifest_hr_path(self) -> str:
        return f"hr/{self.image_id}.ppm"

    def manifest_lr_path(self) -> str:
        return f"lr/{self.image_id}.ppm"


@dataclass(frozen=True)
class LockRecord:
    split: str
    image_id: str
    hr_source_sha256: str
    hr_source_width: int
    hr_source_height: int
    lr_source_sha256: str
    lr_source_width: int
    lr_source_height: int
    hr_ppm_sha256: str
    lr_ppm_sha256: str

    def row(self) -> list[object]:
        return [
            self.split,
            self.image_id,
            self.hr_source_sha256,
            self.hr_source_width,
            self.hr_source_height,
            self.lr_source_sha256,
            self.lr_source_width,
            self.lr_source_height,
            self.hr_ppm_sha256,
            self.lr_ppm_sha256,
        ]


def fail(message: str) -> None:
    raise Div2kError(message)


def _safe_relative_directory(value: str) -> PurePosixPath:
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or "\\" in value
        or any(part in ("", ".", "..") for part in path.parts)
        or path.as_posix() != value
    ):
        fail(f"unsafe split directory: {value!r}")
    return path


def validate_split_specs(splits: tuple[SplitSpec, ...]) -> None:
    names: set[str] = set()
    ids: set[str] = set()
    directories: set[str] = set()
    for split in splits:
        if not re.fullmatch(r"[a-z][a-z0-9_-]*", split.name) or split.name in names:
            fail(f"invalid or duplicate split name: {split.name!r}")
        if split.first_id < 1 or split.last_id > 9999 or split.first_id > split.last_id:
            fail(f"invalid ID range for split {split.name}")
        split_ids = set(split.ids())
        overlap = ids & split_ids
        if overlap:
            fail(f"split ID ranges overlap at {sorted(overlap)[0]}")
        for directory in (split.hr_directory, split.lr_directory):
            _safe_relative_directory(directory)
            if directory in directories:
                fail(f"duplicate split source directory: {directory}")
            directories.add(directory)
        names.add(split.name)
        ids.update(split_ids)


validate_split_specs(DIV2K_SPLITS)
if set(TEST_IDS) & {image_id for split in DIV2K_SPLITS for image_id in split.ids()}:
    fail("DIV2K paired and test ID ranges overlap")


def _source_directory(source_root: Path, relative: str) -> Path:
    return source_root.joinpath(*_safe_relative_directory(relative).parts)


def _classify_cross_split(
    names: set[str],
    split: SplitSpec,
    splits: tuple[SplitSpec, ...],
    *,
    lr: bool,
) -> list[str]:
    expression = r"([0-9]{4})x2\.png" if lr else r"([0-9]{4})\.png"
    own = set(split.ids())
    paired_ids = {image_id for item in splits for image_id in item.ids()}
    result = []
    for name in names:
        match = re.fullmatch(expression, name)
        if match and match.group(1) in paired_ids - own:
            result.append(name)
    return sorted(result)


def _inspect_exact_directory(
    directory: Path,
    expected_names: set[str],
    split: SplitSpec,
    splits: tuple[SplitSpec, ...],
    *,
    lr: bool,
) -> None:
    if directory.is_symlink() or not directory.is_dir():
        fail(f"source directory is missing or is not a real directory: {directory}")
    actual_names: set[str] = set()
    special: list[str] = []
    for child in directory.iterdir():
        if child.is_symlink() or not child.is_file():
            special.append(child.name)
        else:
            actual_names.add(child.name)
    missing = sorted(expected_names - actual_names)
    extra = sorted(actual_names - expected_names)
    cross_split = _classify_cross_split(extra, split, splits, lr=lr)
    if missing or extra or special:
        fail(
            f"source membership mismatch in {directory}: "
            f"missing={missing}, extra={extra}, cross_split={cross_split}, special={sorted(special)}"
        )


def discover_source_pairs(
    source_root: Path,
    splits: tuple[SplitSpec, ...] = DIV2K_SPLITS,
) -> list[SourcePair]:
    validate_split_specs(splits)
    if source_root.is_symlink() or not source_root.is_dir():
        fail(f"source root must be a real directory: {source_root}")
    pairs: list[SourcePair] = []
    seen_ids: set[str] = set()
    for split in splits:
        ids = split.ids()
        hr_directory = _source_directory(source_root, split.hr_directory)
        lr_directory = _source_directory(source_root, split.lr_directory)
        _inspect_exact_directory(
            hr_directory,
            {f"{image_id}.png" for image_id in ids},
            split,
            splits,
            lr=False,
        )
        _inspect_exact_directory(
            lr_directory,
            {f"{image_id}x2.png" for image_id in ids},
            split,
            splits,
            lr=True,
        )
        for image_id in ids:
            if image_id in seen_ids:
                fail(f"duplicate source ID: {image_id}")
            seen_ids.add(image_id)
            pairs.append(
                SourcePair(
                    split.name,
                    image_id,
                    hr_directory / f"{image_id}.png",
                    lr_directory / f"{image_id}x2.png",
                )
            )
    return pairs


def _read_png(path: Path) -> tuple[bytes, DecodedPng]:
    if path.is_symlink() or not path.is_file():
        fail(f"source PNG is missing or is not a regular file: {path}")
    payload = path.read_bytes()
    try:
        decoded = decode_png(payload)
    except ValueError as error:
        fail(f"invalid RGB8 PNG {path}: {error}")
    return payload, decoded


def _ppm_bytes(decoded: DecodedPng) -> bytes:
    return f"P6\n{decoded.width} {decoded.height}\n255\n".encode("ascii") + decoded.rgb


def _sha256(payload: bytes) -> str:
    return hashlib.sha256(payload).hexdigest()


def _csv_bytes(rows: list[list[object]]) -> bytes:
    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    writer.writerows(rows)
    return output.getvalue().encode("ascii")


def _pairs_bytes(split: SplitSpec, pairs: list[SourcePair]) -> bytes:
    lines = ["id\tlr_path\thr_path\n"]
    for pair in pairs:
        if pair.split != split.name:
            continue
        lines.append(
            f"{pair.evaluator_id()}\t{pair.manifest_lr_path()}\t{pair.manifest_hr_path()}\n"
        )
    return "".join(lines).encode("ascii")


def _manifest_bytes(
    pairs: list[SourcePair],
    records: list[LockRecord],
    splits: tuple[SplitSpec, ...],
) -> dict[str, bytes]:
    manifests = {
        "dataset-lock.csv": _lock_bytes(records),
        "dataset-metadata.json": _metadata_bytes(splits),
    }
    for split in splits:
        manifests[f"{split.name}/pairs.tsv"] = _pairs_bytes(split, pairs)
    return manifests


def _metadata_bytes(splits: tuple[SplitSpec, ...]) -> bytes:
    metadata = {
        "image_format": "RGB8 PPM P6",
        "pairing": "official DIV2K bicubic X2",
        "scale": 2,
        "schema_version": 1,
        "splits": [
            {
                "count": split.last_id - split.first_id + 1,
                "first_id": f"{split.first_id:04d}",
                "last_id": f"{split.last_id:04d}",
                "name": split.name,
            }
            for split in splits
        ],
        "test_ids": {
            "first_id": TEST_IDS[0],
            "last_id": TEST_IDS[-1],
            "paired": False,
            "role": "input-only",
        },
    }
    return (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode("ascii")


def _manifest_lock_bytes(manifests: dict[str, bytes]) -> bytes:
    return "".join(
        f"{_sha256(manifests[name])}  {name}\n" for name in sorted(manifests)
    ).encode("ascii")


@dataclass(frozen=True)
class PairJob:
    pair: SourcePair
    prepared_dir: Path
    mode: str


def _process_pair_job(job: PairJob) -> LockRecord:
    pair = job.pair
    hr_source, hr = _read_png(pair.hr_path)
    lr_source, lr = _read_png(pair.lr_path)
    if hr.width != lr.width * 2 or hr.height != lr.height * 2:
        fail(
            f"HR/LR dimensions are not exact X2 for {pair.image_id}: "
            f"HR={hr.width}x{hr.height}, LR={lr.width}x{lr.height}"
        )
    hr_ppm = _ppm_bytes(hr)
    lr_ppm = _ppm_bytes(lr)
    hr_path = job.prepared_dir / pair.hr_relative()
    lr_path = job.prepared_dir / pair.lr_relative()
    if job.mode == "prepare":
        hr_path.write_bytes(hr_ppm)
        lr_path.write_bytes(lr_ppm)
    elif job.mode == "verify":
        if not hr_path.is_file() or hr_path.is_symlink() or hr_path.read_bytes() != hr_ppm:
            fail(f"prepared HR PPM content mismatch for {pair.image_id}")
        if not lr_path.is_file() or lr_path.is_symlink() or lr_path.read_bytes() != lr_ppm:
            fail(f"prepared LR PPM content mismatch for {pair.image_id}")
    elif job.mode == "validate":
        pass
    else:
        fail(f"invalid pair processing mode: {job.mode}")
    return LockRecord(
        pair.split,
        pair.image_id,
        _sha256(hr_source),
        hr.width,
        hr.height,
        _sha256(lr_source),
        lr.width,
        lr.height,
        _sha256(hr_ppm),
        _sha256(lr_ppm),
    )


def _process_pairs(pairs: list[SourcePair], prepared_dir: Path, mode: str) -> list[LockRecord]:
    jobs = [PairJob(pair, prepared_dir, mode) for pair in pairs]
    worker_count = min(4, os.cpu_count() or 1, len(jobs))
    with concurrent.futures.ProcessPoolExecutor(max_workers=worker_count) as executor:
        return list(executor.map(_process_pair_job, jobs, chunksize=1))


def _lock_bytes(records: list[LockRecord]) -> bytes:
    return _csv_bytes([LOCK_HEADER, *(record.row() for record in records)])


def _expected_tree(pairs: list[SourcePair], splits: tuple[SplitSpec, ...]) -> tuple[set[str], set[str]]:
    directories = {
        path
        for split in splits
        for path in (split.name, f"{split.name}/hr", f"{split.name}/lr")
    }
    files = {"dataset-lock.csv", "dataset-metadata.json", "manifest-sha256.txt"}
    files.update(f"{split.name}/pairs.tsv" for split in splits)
    for pair in pairs:
        files.add(pair.hr_relative())
        files.add(pair.lr_relative())
    return directories, files


def _inspect_tree(root: Path) -> tuple[set[str], set[str]]:
    if root.is_symlink() or not root.is_dir():
        fail(f"prepared path must be a real directory: {root}")
    directories: set[str] = set()
    files: set[str] = set()
    pending = [root]
    while pending:
        current = pending.pop()
        for child in current.iterdir():
            relative = child.relative_to(root).as_posix()
            if child.is_symlink():
                fail(f"prepared tree contains a symlink: {relative}")
            if child.is_dir():
                directories.add(relative)
                pending.append(child)
            elif child.is_file():
                files.add(relative)
            else:
                fail(f"prepared tree contains a special entry: {relative}")
    return directories, files


def _validate_pair_path(value: str, expected: str) -> None:
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or "\\" in value
        or any(part in ("", ".", "..") for part in path.parts)
        or path.as_posix() != expected
    ):
        fail(f"unsafe or unexpected pair path: {value!r}")


def _verify_pairs_manifest(payload: bytes, split: SplitSpec, pairs: list[SourcePair]) -> None:
    try:
        text = payload.decode("ascii")
    except UnicodeDecodeError:
        fail("pairs.tsv must contain only ASCII text")
    lines = text.splitlines(keepends=True)
    split_pairs = [pair for pair in pairs if pair.split == split.name]
    if not lines or lines[0] != "id\tlr_path\thr_path\n" or len(lines) != len(split_pairs) + 1:
        fail("pairs.tsv header, line endings, or row count is invalid")
    for line, pair in zip(lines[1:], split_pairs):
        if not line.endswith("\n") or line.count("\t") != 2:
            fail(f"malformed pairs.tsv row for {pair.image_id}")
        evaluator_id, lr_path, hr_path = line[:-1].split("\t")
        if evaluator_id != pair.evaluator_id():
            fail(f"pairs.tsv order mismatch for {pair.image_id}")
        _validate_pair_path(lr_path, pair.manifest_lr_path())
        _validate_pair_path(hr_path, pair.manifest_hr_path())


def _verify_prepared(
    prepared_dir: Path,
    pairs: list[SourcePair],
    records: list[LockRecord],
    splits: tuple[SplitSpec, ...],
) -> None:
    expected_directories, expected_files = _expected_tree(pairs, splits)
    actual_directories, actual_files = _inspect_tree(prepared_dir)
    if actual_directories != expected_directories or actual_files != expected_files:
        expected = expected_directories | expected_files
        actual = actual_directories | actual_files
        fail(
            "prepared membership mismatch: "
            f"missing={sorted(expected - actual)}, extra={sorted(actual - expected)}"
        )

    manifests = _manifest_bytes(pairs, records, splits)
    for name, expected in manifests.items():
        actual = (prepared_dir / name).read_bytes()
        if actual != expected:
            for split in splits:
                if name == f"{split.name}/pairs.tsv":
                    _verify_pairs_manifest(actual, split, pairs)
            fail(f"prepared manifest does not match deterministic content: {name}")
    for split in splits:
        _verify_pairs_manifest(manifests[f"{split.name}/pairs.tsv"], split, pairs)
    if (prepared_dir / "manifest-sha256.txt").read_bytes() != _manifest_lock_bytes(manifests):
        fail("manifest-sha256.txt does not match prepared manifests")

    for pair, record in zip(pairs, records):
        hr_payload = (prepared_dir / pair.hr_relative()).read_bytes()
        lr_payload = (prepared_dir / pair.lr_relative()).read_bytes()
        if _sha256(hr_payload) != record.hr_ppm_sha256:
            fail(f"prepared HR PPM SHA-256 mismatch for {pair.image_id}")
        if _sha256(lr_payload) != record.lr_ppm_sha256:
            fail(f"prepared LR PPM SHA-256 mismatch for {pair.image_id}")
        hr_header = f"P6\n{record.hr_source_width} {record.hr_source_height}\n255\n".encode("ascii")
        lr_header = f"P6\n{record.lr_source_width} {record.lr_source_height}\n255\n".encode("ascii")
        if not hr_payload.startswith(hr_header) or not lr_payload.startswith(lr_header):
            fail(f"prepared PPM header mismatch for {pair.image_id}")


def prepare_dataset(
    source_root: Path,
    prepared_dir: Path,
    splits: tuple[SplitSpec, ...] = DEFAULT_SPLITS,
) -> int:
    if prepared_dir.exists() or prepared_dir.is_symlink():
        fail(f"refusing to overwrite prepared output: {prepared_dir}")
    pairs = discover_source_pairs(source_root, splits)
    prepared_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(prefix=f".{prepared_dir.name}.staging-", dir=prepared_dir.parent)
    )
    try:
        for split in splits:
            (staging / split.name / "hr").mkdir(parents=True)
            (staging / split.name / "lr").mkdir(parents=True)

        records = _process_pairs(pairs, staging, "prepare")
        manifests = _manifest_bytes(pairs, records, splits)
        for name, payload in manifests.items():
            (staging / name).write_bytes(payload)
        (staging / "manifest-sha256.txt").write_bytes(_manifest_lock_bytes(manifests))
        _verify_prepared(staging, pairs, records, splits)
        os.rename(staging, prepared_dir)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    return len(pairs)


def validate_sources(
    source_root: Path,
    splits: tuple[SplitSpec, ...] = DEFAULT_SPLITS,
) -> int:
    pairs = discover_source_pairs(source_root, splits)
    _process_pairs(pairs, Path(), "validate")
    return len(pairs)


def verify_dataset(
    source_root: Path,
    prepared_dir: Path,
    splits: tuple[SplitSpec, ...] = DEFAULT_SPLITS,
) -> int:
    pairs = discover_source_pairs(source_root, splits)

    records = _process_pairs(pairs, prepared_dir, "verify")
    _verify_prepared(prepared_dir, pairs, records, splits)
    return len(pairs)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    for command in ("prepare", "verify"):
        subparser = commands.add_parser(command)
        subparser.add_argument("source_root", type=Path)
        subparser.add_argument("prepared_dir", type=Path)
        subparser.add_argument(
            "--split",
            choices=("all", "train", "validation"),
            default="validation",
            help="selected source/output split (default: validation)",
        )
    validate = commands.add_parser("validate-sources")
    validate.add_argument("source_root", type=Path)
    validate.add_argument(
        "--split",
        choices=("all", "train", "validation"),
        default="validation",
        help="selected source split (default: validation)",
    )
    return parser


def main(arguments: list[str] | None = None) -> int:
    options = build_parser().parse_args(sys.argv[1:] if arguments is None else arguments)
    try:
        selected_splits = {
            "all": DIV2K_SPLITS,
            "train": (TRAIN_SPLIT,),
            "validation": (VALIDATION_SPLIT,),
        }[options.split]
        if options.command == "validate-sources":
            count = validate_sources(options.source_root, selected_splits)
            print(f"Validated {count} offline DIV2K source pairs for {options.split}.")
        elif options.command == "prepare":
            count = prepare_dataset(options.source_root, options.prepared_dir, selected_splits)
            print(
                f"Prepared {count} offline DIV2K pairs for {options.split} "
                f"at {options.prepared_dir}."
            )
        else:
            count = verify_dataset(options.source_root, options.prepared_dir, selected_splits)
            print(
                f"Verified {count} offline DIV2K pairs for {options.split} "
                f"at {options.prepared_dir}."
            )
    except (Div2kError, OSError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
