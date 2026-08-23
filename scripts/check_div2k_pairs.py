#!/usr/bin/env python3
"""Exercise the offline DIV2K pair preparation and lock workflow."""

from __future__ import annotations

import hashlib
import json
import subprocess
import tempfile
from pathlib import Path

from check_div2k_converter import make_png
from div2k_pairs import (
    DIV2K_SPLITS,
    TEST_IDS,
    Div2kError,
    SplitSpec,
    build_parser,
    discover_source_pairs,
    prepare_dataset,
    validate_sources,
    verify_dataset,
)


SYNTHETIC_SPLITS = (
    SplitSpec("train", 1, 3, "DIV2K_train_HR", "DIV2K_train_LR_bicubic/X2"),
    SplitSpec("validation", 4, 5, "DIV2K_valid_HR", "DIV2K_valid_LR_bicubic/X2"),
)


def png(width: int, height: int, seed: int, *, color_type: int = 2) -> bytes:
    rgb = bytes((index * 37 + seed * 19 + 11) & 0xFF for index in range(width * height * 3))
    return make_png(width, height, rgb, [index % 5 for index in range(height)], color_type=color_type)


def populate_sources(root: Path) -> None:
    for split in SYNTHETIC_SPLITS:
        hr_directory = root.joinpath(*split.hr_directory.split("/"))
        lr_directory = root.joinpath(*split.lr_directory.split("/"))
        hr_directory.mkdir(parents=True)
        lr_directory.mkdir(parents=True)
        for value in range(split.first_id, split.last_id + 1):
            image_id = f"{value:04d}"
            (hr_directory / f"{image_id}.png").write_bytes(png(12, 12, value))
            (lr_directory / f"{image_id}x2.png").write_bytes(png(6, 6, value + 100))
    test_directory = root / "DIV2K_test_LR_bicubic" / "X2"
    test_directory.mkdir(parents=True)
    (test_directory / "0901x2.png").write_bytes(png(6, 6, 901))


def expect_error(call, fragment: str) -> None:
    try:
        call()
    except Div2kError as error:
        if fragment not in str(error):
            raise AssertionError(f"expected {fragment!r}, received {str(error)!r}") from error
    else:
        raise AssertionError(f"expected DIV2K failure containing {fragment!r}")


def tree_hashes(root: Path) -> dict[str, str]:
    return {
        path.relative_to(root).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(root.rglob("*"))
        if path.is_file()
    }


def run_rust_example(example: str, arguments: list[Path | str]) -> None:
    completed = subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--locked",
            "--example",
            example,
            "--",
            *(str(argument) for argument in arguments),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )
    if completed.returncode != 0:
        raise AssertionError(
            f"{example} rejected generated manifest:\n{completed.stdout}{completed.stderr}"
        )


def main() -> int:
    assert DIV2K_SPLITS[0].ids() == tuple(f"{value:04d}" for value in range(1, 801))
    assert DIV2K_SPLITS[1].ids() == tuple(f"{value:04d}" for value in range(801, 901))
    assert TEST_IDS == tuple(f"{value:04d}" for value in range(901, 1001))
    parser = build_parser()
    assert parser.parse_args(["prepare", "sources", "prepared"]).split == "validation"
    assert parser.parse_args(["verify", "sources", "prepared"]).split == "validation"
    assert parser.parse_args(["validate-sources", "sources"]).split == "validation"
    assert parser.parse_args(
        ["prepare", "sources", "prepared", "--split", "train"]
    ).split == "train"
    assert parser.parse_args(
        ["verify", "sources", "prepared", "--split", "all"]
    ).split == "all"

    with tempfile.TemporaryDirectory(prefix="div2k-pairs-check-") as temporary:
        root = Path(temporary)
        sources = root / "sources"
        populate_sources(sources)
        pairs = discover_source_pairs(sources, SYNTHETIC_SPLITS)
        assert [(pair.split, pair.image_id) for pair in pairs] == [
            ("train", "0001"),
            ("train", "0002"),
            ("train", "0003"),
            ("validation", "0004"),
            ("validation", "0005"),
        ]
        assert validate_sources(sources, (SYNTHETIC_SPLITS[1],)) == 2

        validation_splits = (SYNTHETIC_SPLITS[1],)
        validation_first = root / "prepared-validation-one"
        validation_second = root / "prepared-validation-two"
        assert prepare_dataset(sources, validation_first, validation_splits) == 2
        assert verify_dataset(sources, validation_first, validation_splits) == 2
        assert not (validation_first / "pairs.tsv").exists()
        assert not (validation_first / "train").exists()
        validation_manifest = validation_first / "validation" / "pairs.tsv"
        assert validation_manifest.read_text("ascii").splitlines() == [
            "id\tlr_path\thr_path",
            "div2k_validation_0004\tlr/0004.ppm\thr/0004.ppm",
            "div2k_validation_0005\tlr/0005.ppm\thr/0005.ppm",
        ]
        assert set(tree_hashes(validation_first)) == {
            "dataset-lock.csv",
            "dataset-metadata.json",
            "manifest-sha256.txt",
            "validation/hr/0004.ppm",
            "validation/hr/0005.ppm",
            "validation/lr/0004.ppm",
            "validation/lr/0005.ppm",
            "validation/pairs.tsv",
        }
        metadata = json.loads(
            (validation_first / "dataset-metadata.json").read_text("ascii")
        )
        assert metadata["splits"] == [
            {"count": 2, "first_id": "0004", "last_id": "0005", "name": "validation"}
        ]
        lock_lines = (validation_first / "dataset-lock.csv").read_text("ascii").splitlines()
        assert len(lock_lines) == 3
        assert all(line.startswith("validation,") for line in lock_lines[1:])
        manifest_lock_paths = {
            line.split("  ", 1)[1]
            for line in (validation_first / "manifest-sha256.txt")
            .read_text("ascii")
            .splitlines()
        }
        assert manifest_lock_paths == {
            "dataset-lock.csv",
            "dataset-metadata.json",
            "validation/pairs.tsv",
        }
        run_rust_example(
            "paired_eval", [validation_manifest, root / "validation-report.csv"]
        )
        run_rust_example(
            "quality_sweep", [validation_manifest, root / "validation-sweep.csv", "2"]
        )
        assert prepare_dataset(sources, validation_second, validation_splits) == 2
        assert tree_hashes(validation_first) == tree_hashes(validation_second)
        expect_error(
            lambda: prepare_dataset(sources, validation_first, validation_splits),
            "refusing to overwrite",
        )

        train_splits = (SYNTHETIC_SPLITS[0],)
        train_prepared = root / "prepared-train"
        assert prepare_dataset(sources, train_prepared, train_splits) == 3
        assert verify_dataset(sources, train_prepared, train_splits) == 3
        assert (train_prepared / "train" / "pairs.tsv").is_file()
        assert not (train_prepared / "validation").exists()

        all_prepared = root / "prepared-all"
        assert prepare_dataset(sources, all_prepared, SYNTHETIC_SPLITS) == 5
        assert verify_dataset(sources, all_prepared, SYNTHETIC_SPLITS) == 5
        train_manifest = all_prepared / "train" / "pairs.tsv"
        assert train_manifest.read_text("ascii").splitlines() == [
            "id\tlr_path\thr_path",
            "div2k_train_0001\tlr/0001.ppm\thr/0001.ppm",
            "div2k_train_0002\tlr/0002.ppm\thr/0002.ppm",
            "div2k_train_0003\tlr/0003.ppm\thr/0003.ppm",
        ]
        assert (all_prepared / "validation" / "pairs.tsv").is_file()

        train_hr = sources / "DIV2K_train_HR"
        missing_path = train_hr / "0001.png"
        missing_payload = missing_path.read_bytes()
        missing_path.unlink()
        expect_error(
            lambda: discover_source_pairs(sources, SYNTHETIC_SPLITS),
            "missing=['0001.png']",
        )
        missing_path.write_bytes(missing_payload)

        extra_path = train_hr / "unexpected.png"
        extra_path.write_bytes(png(12, 12, 9))
        expect_error(
            lambda: discover_source_pairs(sources, SYNTHETIC_SPLITS),
            "extra=['unexpected.png']",
        )
        extra_path.unlink()

        cross_split_path = train_hr / "0004.png"
        cross_split_path.write_bytes(png(12, 12, 4))
        expect_error(
            lambda: discover_source_pairs(sources, SYNTHETIC_SPLITS),
            "cross_split=['0004.png']",
        )
        cross_split_path.unlink()

        malformed_path = train_hr / "0002.png"
        original = malformed_path.read_bytes()
        malformed_path.write_bytes(png(12, 12, 2, color_type=6))
        failed_output = root / "failed-rgb8"
        expect_error(
            lambda: prepare_dataset(sources, failed_output, SYNTHETIC_SPLITS),
            "noninterlaced 8-bit RGB truecolor",
        )
        assert not failed_output.exists()
        assert not list(root.glob(f".{failed_output.name}.staging-*"))
        malformed_path.write_bytes(original)

        wrong_size_path = train_hr / "0003.png"
        original = wrong_size_path.read_bytes()
        wrong_size_path.write_bytes(png(11, 12, 3))
        failed_output = root / "failed-scale"
        expect_error(
            lambda: prepare_dataset(sources, failed_output, SYNTHETIC_SPLITS),
            "dimensions are not exact X2",
        )
        assert not failed_output.exists()
        assert not list(root.glob(f".{failed_output.name}.staging-*"))
        wrong_size_path.write_bytes(original)

        pairs_path = all_prepared / "train" / "pairs.tsv"
        original = pairs_path.read_bytes()
        pairs_path.write_bytes(original.replace(b"lr/0001.ppm", b"../escape.ppm", 1))
        expect_error(
            lambda: verify_dataset(sources, all_prepared, SYNTHETIC_SPLITS),
            "unsafe or unexpected pair path",
        )
        pairs_path.write_bytes(original)

        lock_path = all_prepared / "dataset-lock.csv"
        original = lock_path.read_bytes()
        lock_path.write_bytes(original + b"tamper")
        expect_error(
            lambda: verify_dataset(sources, all_prepared, SYNTHETIC_SPLITS),
            "dataset-lock.csv",
        )
        lock_path.write_bytes(original)

        ppm_path = all_prepared / "validation" / "lr" / "0005.ppm"
        original = ppm_path.read_bytes()
        ppm_path.write_bytes(original[:-1] + bytes([original[-1] ^ 1]))
        expect_error(
            lambda: verify_dataset(sources, all_prepared, SYNTHETIC_SPLITS),
            "prepared LR PPM content mismatch",
        )
        ppm_path.write_bytes(original)

        manifest_lock = all_prepared / "manifest-sha256.txt"
        original = manifest_lock.read_bytes()
        manifest_lock.write_bytes(b"0" * len(original))
        expect_error(
            lambda: verify_dataset(sources, all_prepared, SYNTHETIC_SPLITS),
            "manifest-sha256.txt",
        )
        manifest_lock.write_bytes(original)
        assert verify_dataset(sources, all_prepared, SYNTHETIC_SPLITS) == 5

    print("Offline DIV2K pair preparation check passed with deterministic locks and failures.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
