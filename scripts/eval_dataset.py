#!/usr/bin/env python3
"""Validate, fetch, and prepare the local paired HR/LR evaluation dataset."""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
import re
import shutil
import sys
import tempfile
import time
import urllib.error
import urllib.parse
import urllib.request
import warnings
from dataclasses import dataclass
from pathlib import Path
from pathlib import PurePosixPath


SCHEMA_VERSION = 1
CATEGORIES = {"nature", "render", "text_ui"}
ENTRIES_PER_CATEGORY = 10
TOTAL_ENTRIES = 30
MIN_TARGET_WIDTH = 2560
MIN_TARGET_HEIGHT = 1440
MAX_DOWNLOAD_BYTES = 160 * 1024 * 1024
MAX_SOURCE_PIXELS = 180_000_000
USER_AGENT = (
    "verisilicon-eval-dataset/1.0 "
    "(local research; https://github.com/Razedaisiki/verisilicon_software_competition)"
)
REQUEST_DELAY_SECONDS = 1.0
RETRY_DELAYS_SECONDS = (2.0, 4.0, 8.0, 16.0, 32.0)
RETRY_HTTP_STATUS = {429, 500, 502, 503, 504}
PILLOW_VERSION = "11.3.0"
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}")
ID_PATTERN = re.compile(r"[A-Za-z0-9._-]+")
CATALOG_KEYS = {"schema_version", "category", "entries"}
ENTRY_KEYS = {
    "id",
    "title",
    "author",
    "source_page",
    "download_url",
    "license",
    "license_url",
    "source_width",
    "source_height",
    "source_mime",
    "target_width",
    "target_height",
    "sha256",
    "notes",
}
ALLOWED_LICENSES = {
    "AGPL-3.0-or-later",
    "CC0-1.0",
    "CC-BY-2.0",
    "CC-BY-2.5",
    "CC-BY-3.0",
    "CC-BY-4.0",
    "CC-BY-SA-2.0",
    "CC-BY-SA-2.5",
    "CC-BY-SA-3.0",
    "CC-BY-SA-4.0",
    "GPL-2.0-or-later",
    "GPL-3.0-only",
    "PDM-1.0",
}
MIME_SUFFIXES = {
    "image/jpeg": ".jpg",
    "image/png": ".png",
    "image/webp": ".webp",
}
MIME_FORMATS = {
    "image/jpeg": "JPEG",
    "image/png": "PNG",
    "image/webp": "WEBP",
}


class DatasetError(ValueError):
    """Stable validation or preparation failure."""


@dataclass(frozen=True)
class Entry:
    id: str
    title: str
    author: str
    source_page: str
    download_url: str
    license: str
    license_url: str
    source_width: int
    source_height: int
    source_mime: str
    target_width: int
    target_height: int
    sha256: str | None
    notes: str
    category: str

    def local_name(self) -> str:
        return self.id + MIME_SUFFIXES[self.source_mime]


def fail(message: str) -> None:
    raise DatasetError(message)


def _is_int(value: object) -> bool:
    return type(value) is int


def _require_ascii_text(value: object, field: str, *, nonempty: bool = True) -> str:
    if type(value) is not str:
        fail(f"{field} must be a string")
    if nonempty and not value:
        fail(f"{field} must not be empty")
    try:
        value.encode("ascii")
    except UnicodeEncodeError:
        fail(f"{field} must contain only ASCII text")
    if "\0" in value or "\r" in value or "\n" in value:
        fail(f"{field} contains a forbidden control character")
    return value


def _require_https(value: object, field: str) -> str:
    text = _require_ascii_text(value, field)
    parsed = urllib.parse.urlsplit(text)
    if parsed.scheme != "https" or not parsed.netloc or parsed.username or parsed.password:
        fail(f"{field} must be an absolute HTTPS URL without credentials")
    return text


def _parse_entry(raw: object, category: str, location: str) -> Entry:
    if type(raw) is not dict or set(raw) != ENTRY_KEYS:
        actual = set(raw) if type(raw) is dict else set()
        fail(f"{location} entry keys mismatch: missing={sorted(ENTRY_KEYS - actual)}, extra={sorted(actual - ENTRY_KEYS)}")
    entry_id = _require_ascii_text(raw["id"], f"{location}.id")
    if ID_PATTERN.fullmatch(entry_id) is None:
        fail(f"{location}.id must match [A-Za-z0-9._-]+")
    license_name = _require_ascii_text(raw["license"], f"{location}.license")
    if license_name not in ALLOWED_LICENSES:
        fail(f"{location}.license is not an allowed reusable license: {license_name}")
    source_mime = _require_ascii_text(raw["source_mime"], f"{location}.source_mime")
    if source_mime not in MIME_SUFFIXES:
        fail(f"{location}.source_mime is unsupported: {source_mime}")
    dimensions: dict[str, int] = {}
    for field in ("source_width", "source_height", "target_width", "target_height"):
        value = raw[field]
        if not _is_int(value) or value <= 0:
            fail(f"{location}.{field} must be a positive integer")
        dimensions[field] = value
    target_width = dimensions["target_width"]
    target_height = dimensions["target_height"]
    if target_width % 2 or target_height % 2:
        fail(f"{location} target dimensions must both be even")
    if target_width < MIN_TARGET_WIDTH or target_height < MIN_TARGET_HEIGHT:
        fail(f"{location} target dimensions must be at least {MIN_TARGET_WIDTH}x{MIN_TARGET_HEIGHT}")
    if target_width * 9 != target_height * 16:
        fail(f"{location} target dimensions must have exact 16:9 aspect ratio")
    if target_width > dimensions["source_width"] or target_height > dimensions["source_height"]:
        fail(f"{location} target dimensions exceed source dimensions")
    if dimensions["source_width"] * dimensions["source_height"] > MAX_SOURCE_PIXELS:
        fail(f"{location} source dimensions exceed the 180,000,000 pixel limit")
    digest = raw["sha256"]
    if digest is not None and (type(digest) is not str or SHA256_PATTERN.fullmatch(digest) is None):
        fail(f"{location}.sha256 must be null or 64 lowercase hexadecimal characters")
    return Entry(
        id=entry_id,
        title=_require_ascii_text(raw["title"], f"{location}.title"),
        author=_require_ascii_text(raw["author"], f"{location}.author"),
        source_page=_require_https(raw["source_page"], f"{location}.source_page"),
        download_url=_require_https(raw["download_url"], f"{location}.download_url"),
        license=license_name,
        license_url=_require_https(raw["license_url"], f"{location}.license_url"),
        source_width=dimensions["source_width"],
        source_height=dimensions["source_height"],
        source_mime=source_mime,
        target_width=target_width,
        target_height=target_height,
        sha256=digest,
        notes=_require_ascii_text(raw["notes"], f"{location}.notes", nonempty=False),
        category=category,
    )


def load_catalogs(catalog_dir: Path, *, locked: bool) -> list[Entry]:
    if catalog_dir.is_symlink() or not catalog_dir.is_dir():
        fail(f"catalog path must be a real directory: {catalog_dir}")
    paths = sorted(catalog_dir.glob("*.json"), key=lambda path: path.name)
    if not paths:
        fail(f"catalog directory contains no JSON files: {catalog_dir}")
    entries: list[Entry] = []
    seen_categories: set[str] = set()
    for path in paths:
        if path.is_symlink() or not path.is_file():
            fail(f"catalog must be a regular file, not a symlink: {path}")
        data = path.read_bytes()
        if any(byte > 0x7F for byte in data) or b"\0" in data:
            fail(f"catalog must contain only ASCII JSON text: {path.name}")
        try:
            raw = json.loads(data)
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            fail(f"invalid JSON catalog {path.name}: {error}")
        if type(raw) is not dict or set(raw) != CATALOG_KEYS:
            actual = set(raw) if type(raw) is dict else set()
            fail(f"catalog keys mismatch in {path.name}: missing={sorted(CATALOG_KEYS - actual)}, extra={sorted(actual - CATALOG_KEYS)}")
        if raw["schema_version"] != SCHEMA_VERSION or not _is_int(raw["schema_version"]):
            fail(f"{path.name}.schema_version must be integer {SCHEMA_VERSION}")
        category = _require_ascii_text(raw["category"], f"{path.name}.category")
        if category not in CATEGORIES:
            fail(f"unsupported category in {path.name}: {category}")
        if category in seen_categories:
            fail(f"duplicate category catalog: {category}")
        seen_categories.add(category)
        raw_entries = raw["entries"]
        if type(raw_entries) is not list:
            fail(f"{path.name}.entries must be an array")
        if len(raw_entries) != ENTRIES_PER_CATEGORY:
            fail(f"category {category} must contain exactly {ENTRIES_PER_CATEGORY} entries")
        entries.extend(
            _parse_entry(item, category, f"{path.name}.entries[{index}]")
            for index, item in enumerate(raw_entries)
        )
    if seen_categories != CATEGORIES or len(entries) != TOTAL_ENTRIES:
        fail(f"catalogs must contain exactly {TOTAL_ENTRIES} entries: 10 each for nature, render, and text_ui")
    ids = [entry.id for entry in entries]
    urls = [entry.download_url for entry in entries]
    if len(ids) != len(set(ids)):
        fail("entry IDs must be unique across all catalogs")
    if len(urls) != len(set(urls)):
        fail("download URLs must be unique across all catalogs")
    if locked and any(entry.sha256 is None for entry in entries):
        fail("locked catalogs require a SHA-256 digest for every entry")
    return sorted(entries, key=lambda entry: entry.id)


class _HttpsOnlyRedirectHandler(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, request, file_pointer, code, message, headers, new_url):
        if urllib.parse.urlsplit(new_url).scheme != "https":
            fail(f"refusing redirect away from HTTPS: {new_url}")
        return super().redirect_request(request, file_pointer, code, message, headers, new_url)


_URL_OPENER = urllib.request.build_opener(_HttpsOnlyRedirectHandler())


def _open_url(request: urllib.request.Request):
    return _URL_OPENER.open(request, timeout=60)


def _open_url_with_retries(request: urllib.request.Request):
    for attempt, fallback_delay in enumerate(RETRY_DELAYS_SECONDS, start=1):
        try:
            return _open_url(request)
        except urllib.error.HTTPError as error:
            if error.code not in RETRY_HTTP_STATUS:
                raise
            retry_after = error.headers.get("Retry-After")
            error.close()
            delay = fallback_delay
            if retry_after is not None:
                try:
                    delay = max(delay, min(60.0, float(retry_after)))
                except ValueError:
                    pass
            print(
                f"HTTP {error.code}; retry {attempt}/{len(RETRY_DELAYS_SECONDS)} in {delay:.1f}s",
                file=sys.stderr,
            )
            time.sleep(delay)
        except urllib.error.URLError:
            print(
                f"network error; retry {attempt}/{len(RETRY_DELAYS_SECONDS)} in {fallback_delay:.1f}s",
                file=sys.stderr,
            )
            time.sleep(fallback_delay)
    return _open_url(request)


def _require_pillow():
    try:
        import PIL
        from PIL import Image, ImageOps
    except ImportError:
        fail(f"Pillow exactly {PILLOW_VERSION} is required")
    if PIL.__version__ != PILLOW_VERSION:
        fail(f"Pillow exactly {PILLOW_VERSION} is required; found {PIL.__version__}")
    Image.MAX_IMAGE_PIXELS = MAX_SOURCE_PIXELS
    return Image, ImageOps


def _hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _validate_decoded(path: Path, entry: Entry) -> None:
    Image, ImageOps = _require_pillow()
    try:
        with warnings.catch_warnings():
            warnings.simplefilter("error", Image.DecompressionBombWarning)
            with Image.open(path) as image:
                if image.format != MIME_FORMATS[entry.source_mime]:
                    fail(f"decoded format mismatch for {entry.id}: expected {MIME_FORMATS[entry.source_mime]}, found {image.format}")
                oriented = ImageOps.exif_transpose(image)
                oriented.load()
                if oriented.size != (entry.source_width, entry.source_height):
                    fail(f"decoded oriented dimensions mismatch for {entry.id}: expected {entry.source_width}x{entry.source_height}, found {oriented.width}x{oriented.height}")
    except DatasetError:
        raise
    except Exception as error:
        fail(f"failed to decode {entry.id}: {error}")


def _validate_local_file(path: Path, entry: Entry, *, require_hash: bool) -> str:
    if path.is_symlink() or not path.is_file():
        fail(f"download is missing or is not a regular file: {path}")
    if path.stat().st_size > MAX_DOWNLOAD_BYTES:
        fail(f"download exceeds the 160 MiB limit: {entry.id}")
    digest = _hash_file(path)
    if require_hash and digest != entry.sha256:
        fail(f"existing download hash mismatch; refusing overwrite: {entry.id}")
    _validate_decoded(path, entry)
    return digest


def _download_to_temporary(entry: Entry, directory: Path) -> Path:
    request = urllib.request.Request(entry.download_url, headers={"User-Agent": USER_AGENT})
    temporary_handle = tempfile.NamedTemporaryFile(prefix=f".{entry.id}.", suffix=".download", dir=directory, delete=False)
    temporary = Path(temporary_handle.name)
    try:
        with temporary_handle, _open_url_with_retries(request) as response:
            final_url = response.geturl()
            if urllib.parse.urlsplit(final_url).scheme != "https":
                fail(f"refusing response away from HTTPS: {final_url}")
            length_header = response.headers.get("Content-Length")
            expected_length = None
            if length_header is not None:
                try:
                    expected_length = int(length_header)
                except ValueError:
                    fail(f"invalid Content-Length for {entry.id}")
                if expected_length < 0 or expected_length > MAX_DOWNLOAD_BYTES:
                    fail(f"Content-Length exceeds the 160 MiB limit: {entry.id}")
            total = 0
            while True:
                chunk = response.read(1024 * 1024)
                if not chunk:
                    break
                total += len(chunk)
                if total > MAX_DOWNLOAD_BYTES:
                    fail(f"download exceeds the 160 MiB limit: {entry.id}")
                temporary_handle.write(chunk)
            temporary_handle.flush()
            os.fsync(temporary_handle.fileno())
            if expected_length is not None and total != expected_length:
                fail(f"Content-Length mismatch for {entry.id}: expected {expected_length}, received {total}")
        return temporary
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def fetch_entries(entries: list[Entry], local_dir: Path, *, bootstrap: bool) -> list[tuple[str, str]]:
    if local_dir.exists() and (local_dir.is_symlink() or not local_dir.is_dir()):
        fail(f"local download path is not a real directory: {local_dir}")
    local_dir.mkdir(parents=True, exist_ok=True)
    results: list[tuple[str, str]] = []
    for index, entry in enumerate(entries, start=1):
        destination = local_dir / entry.local_name()
        if destination.exists() or destination.is_symlink():
            print(f"[{index}/{len(entries)}] verifying {entry.id}", file=sys.stderr)
            digest = _validate_local_file(destination, entry, require_hash=entry.sha256 is not None)
        else:
            print(f"[{index}/{len(entries)}] downloading {entry.id}", file=sys.stderr)
            time.sleep(REQUEST_DELAY_SECONDS)
            temporary = _download_to_temporary(entry, local_dir)
            try:
                digest = _hash_file(temporary)
                if entry.sha256 is not None and digest != entry.sha256:
                    fail(f"download hash mismatch for {entry.id}")
                _validate_decoded(temporary, entry)
                try:
                    os.link(temporary, destination)
                except FileExistsError:
                    fail(f"download destination appeared during fetch; refusing overwrite: {destination}")
            finally:
                temporary.unlink(missing_ok=True)
        if entry.sha256 is None:
            if not bootstrap:
                fail("null hashes require --bootstrap")
            results.append((entry.id, digest))
    for entry_id, digest in results:
        print(f"{entry_id} {digest}")
    return results


def _write_ppm(path: Path, image) -> str:
    width, height = image.size
    payload = f"P6\n{width} {height}\n255\n".encode("ascii") + image.tobytes()
    path.write_bytes(payload)
    return hashlib.sha256(payload).hexdigest()


def _csv_bytes(rows: list[list[object]]) -> bytes:
    import io

    output = io.StringIO(newline="")
    writer = csv.writer(output, lineterminator="\n")
    writer.writerows(rows)
    return output.getvalue().encode("utf-8")


def prepare_entries(entries: list[Entry], local_dir: Path, prepared_dir: Path) -> None:
    Image, ImageOps = _require_pillow()
    if prepared_dir.exists() or prepared_dir.is_symlink():
        fail(f"refusing to overwrite prepared output: {prepared_dir}")
    if local_dir.is_symlink() or not local_dir.is_dir():
        fail(f"local download path must be a real directory: {local_dir}")
    for entry in entries:
        _validate_local_file(local_dir / entry.local_name(), entry, require_hash=True)
    prepared_dir.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix=f".{prepared_dir.name}.staging-", dir=prepared_dir.parent))
    try:
        hr_dir = staging / "hr"
        lr_dir = staging / "lr"
        hr_dir.mkdir()
        lr_dir.mkdir()
        pairs_rows: list[list[object]] = [["id", "lr_path", "hr_path"]]
        attribution_rows: list[list[object]] = [["id", "title", "author", "source_page", "download_url", "license", "license_url", "notes"]]
        lock_rows: list[list[object]] = [["id", "source_sha256", "source_width", "source_height", "crop_left", "crop_top", "hr_sha256", "hr_width", "hr_height", "lr_sha256", "lr_width", "lr_height"]]
        for entry in entries:
            source_path = local_dir / entry.local_name()
            with Image.open(source_path) as decoded:
                oriented = ImageOps.exif_transpose(decoded)
                oriented.load()
                if oriented.size != (entry.source_width, entry.source_height):
                    fail(f"oriented dimensions mismatch for {entry.id}: expected {entry.source_width}x{entry.source_height}, found {oriented.width}x{oriented.height}")
                rgb = oriented.convert("RGB")
                left = (entry.source_width - entry.target_width) // 2
                top = (entry.source_height - entry.target_height) // 2
                hr = rgb.crop((left, top, left + entry.target_width, top + entry.target_height))
                lr_size = (entry.target_width // 2, entry.target_height // 2)
                lr = hr.resize(lr_size, resample=Image.Resampling.BICUBIC)
                hr_relative = f"hr/{entry.id}.ppm"
                lr_relative = f"lr/{entry.id}.ppm"
                hr_hash = _write_ppm(staging / hr_relative, hr)
                lr_hash = _write_ppm(staging / lr_relative, lr)
            pairs_rows.append([entry.id, lr_relative, hr_relative])
            attribution_rows.append([entry.id, entry.title, entry.author, entry.source_page, entry.download_url, entry.license, entry.license_url, entry.notes])
            lock_rows.append([entry.id, entry.sha256, entry.source_width, entry.source_height, left, top, hr_hash, entry.target_width, entry.target_height, lr_hash, entry.target_width // 2, entry.target_height // 2])
        (staging / "pairs.tsv").write_bytes(
            "".join("\t".join(str(value) for value in row) + "\n" for row in pairs_rows).encode("ascii")
        )
        (staging / "attribution.csv").write_bytes(_csv_bytes(attribution_rows))
        (staging / "dataset-lock.csv").write_bytes(_csv_bytes(lock_rows))
        (staging / "dataset-metadata.json").write_bytes(_expected_metadata(len(entries)))
        os.rename(staging, prepared_dir)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise


def command_validate(args: argparse.Namespace) -> None:
    entries = load_catalogs(args.catalog_dir, locked=args.locked)
    print(f"Validated {len(entries)} catalog entries.")


def command_fetch(args: argparse.Namespace) -> None:
    entries = load_catalogs(args.catalog_dir, locked=not args.bootstrap)
    fetch_entries(entries, args.local_dir, bootstrap=args.bootstrap)


def command_prepare(args: argparse.Namespace) -> None:
    entries = load_catalogs(args.catalog_dir, locked=True)
    prepare_entries(entries, args.local_dir, args.prepared_dir)
    print(f"Prepared {len(entries)} paired HR/LR images at {args.prepared_dir}.")


def _expected_metadata(entry_count: int) -> bytes:
    metadata = {
        "entry_count": entry_count,
        "hr_crop_policy": "center-no-resize",
        "image_format": "RGB8 PPM P6",
        "lr_downsampler": "Pillow-BICUBIC",
        "pillow_version": PILLOW_VERSION,
        "scale": 2,
        "schema_version": 1,
    }
    return (json.dumps(metadata, indent=2, sort_keys=True) + "\n").encode("ascii")


def _inspect_prepared_tree(prepared_dir: Path) -> tuple[set[str], set[str]]:
    if prepared_dir.is_symlink() or not prepared_dir.is_dir():
        fail(f"prepared dataset path must be a real directory: {prepared_dir}")
    directories: set[str] = set()
    files: set[str] = set()
    pending = [prepared_dir]
    while pending:
        current = pending.pop()
        for child in current.iterdir():
            relative = child.relative_to(prepared_dir).as_posix()
            if child.is_symlink():
                fail(f"prepared dataset contains a symlink: {relative}")
            if child.is_dir():
                directories.add(relative)
                pending.append(child)
            elif child.is_file():
                files.add(relative)
            else:
                fail(f"prepared dataset contains a special entry: {relative}")
    return directories, files


def _safe_pair_path(value: str, directory: str, entry_id: str) -> None:
    try:
        value.encode("ascii")
    except UnicodeEncodeError:
        fail(f"pair path must contain only ASCII text: {value!r}")
    path = PurePosixPath(value)
    if (
        path.is_absolute()
        or "\\" in value
        or any(part in ("", ".", "..") for part in path.parts)
        or path.as_posix() != f"{directory}/{entry_id}.ppm"
    ):
        fail(f"unsafe or unexpected pair path: {value!r}")


def _verify_pairs(path: Path, entries: list[Entry]) -> None:
    try:
        text = path.read_bytes().decode("ascii")
    except UnicodeDecodeError:
        fail("pairs.tsv must contain only ASCII text")
    lines = text.splitlines(keepends=True)
    if not lines or lines[0] != "id\tlr_path\thr_path\n" or len(lines) != len(entries) + 1:
        fail("pairs.tsv header, line endings, or row count is invalid")
    for line, entry in zip(lines[1:], entries):
        if not line.endswith("\n") or line.count("\t") != 2:
            fail(f"malformed pairs.tsv row for {entry.id}")
        entry_id, lr_path, hr_path = line[:-1].split("\t")
        if entry_id != entry.id:
            fail(f"pairs.tsv ID order mismatch: expected {entry.id}")
        _safe_pair_path(lr_path, "lr", entry.id)
        _safe_pair_path(hr_path, "hr", entry.id)


def _read_lock_rows(path: Path, entries: list[Entry]) -> dict[str, dict[str, str]]:
    header = [
        "id",
        "source_sha256",
        "source_width",
        "source_height",
        "crop_left",
        "crop_top",
        "hr_sha256",
        "hr_width",
        "hr_height",
        "lr_sha256",
        "lr_width",
        "lr_height",
    ]
    try:
        text = path.read_bytes().decode("ascii")
    except UnicodeDecodeError:
        fail("dataset-lock.csv must contain only ASCII text")
    if not text.endswith("\n") or "\r" in text:
        fail("dataset-lock.csv must use LF line endings")
    rows = list(csv.reader(text.splitlines()))
    if not rows or rows[0] != header or len(rows) != len(entries) + 1:
        fail("dataset-lock.csv header or row count is invalid")
    result: dict[str, dict[str, str]] = {}
    for raw, entry in zip(rows[1:], entries):
        if len(raw) != len(header) or raw[0] != entry.id or raw[0] in result:
            fail(f"dataset-lock.csv row mismatch for {entry.id}")
        result[entry.id] = dict(zip(header, raw))
    return result


def _verify_ppm(path: Path, width: int, height: int) -> str:
    header = f"P6\n{width} {height}\n255\n".encode("ascii")
    expected_length = len(header) + width * height * 3
    if path.stat().st_size != expected_length:
        fail(f"PPM length mismatch: {path.name}")
    digest = hashlib.sha256()
    with path.open("rb") as source:
        actual_header = source.read(len(header))
        if actual_header != header:
            fail(f"PPM header or dimensions mismatch: {path.name}")
        digest.update(actual_header)
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def verify_prepared(entries: list[Entry], prepared_dir: Path) -> None:
    expected_directories = {"hr", "lr"}
    expected_files = {
        "attribution.csv",
        "dataset-lock.csv",
        "dataset-metadata.json",
        "pairs.tsv",
    }
    for entry in entries:
        expected_files.add(f"hr/{entry.id}.ppm")
        expected_files.add(f"lr/{entry.id}.ppm")
    actual_directories, actual_files = _inspect_prepared_tree(prepared_dir)
    if actual_directories != expected_directories or actual_files != expected_files:
        expected = expected_directories | expected_files
        actual = actual_directories | actual_files
        fail(
            "prepared dataset membership mismatch: "
            f"missing={sorted(expected - actual)}, extra={sorted(actual - expected)}"
        )
    if (prepared_dir / "dataset-metadata.json").read_bytes() != _expected_metadata(len(entries)):
        fail("prepared dataset-metadata.json does not match the deterministic contract")
    _verify_pairs(prepared_dir / "pairs.tsv", entries)
    attribution_rows: list[list[object]] = [["id", "title", "author", "source_page", "download_url", "license", "license_url", "notes"]]
    for entry in entries:
        attribution_rows.append([entry.id, entry.title, entry.author, entry.source_page, entry.download_url, entry.license, entry.license_url, entry.notes])
    if (prepared_dir / "attribution.csv").read_bytes() != _csv_bytes(attribution_rows):
        fail("prepared attribution.csv does not match the locked catalogs")
    lock_rows = _read_lock_rows(prepared_dir / "dataset-lock.csv", entries)
    for entry in entries:
        row = lock_rows[entry.id]
        expected_values = {
            "source_sha256": entry.sha256,
            "source_width": str(entry.source_width),
            "source_height": str(entry.source_height),
            "crop_left": str((entry.source_width - entry.target_width) // 2),
            "crop_top": str((entry.source_height - entry.target_height) // 2),
            "hr_width": str(entry.target_width),
            "hr_height": str(entry.target_height),
            "lr_width": str(entry.target_width // 2),
            "lr_height": str(entry.target_height // 2),
        }
        for field, expected in expected_values.items():
            if row[field] != expected:
                fail(f"dataset-lock.csv {field} mismatch for {entry.id}")
        hr_hash = _verify_ppm(
            prepared_dir / "hr" / f"{entry.id}.ppm",
            entry.target_width,
            entry.target_height,
        )
        lr_hash = _verify_ppm(
            prepared_dir / "lr" / f"{entry.id}.ppm",
            entry.target_width // 2,
            entry.target_height // 2,
        )
        if row["hr_sha256"] != hr_hash or row["lr_sha256"] != lr_hash:
            fail(f"dataset-lock.csv PPM SHA-256 mismatch for {entry.id}")


def command_verify(args: argparse.Namespace) -> None:
    entries = load_catalogs(args.catalog_dir, locked=True)
    verify_prepared(entries, args.prepared_dir)
    print(f"Verified {len(entries)} paired HR/LR images at {args.prepared_dir}.")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate")
    validate.add_argument("catalog_dir", type=Path)
    validate.add_argument("--locked", action="store_true")
    validate.set_defaults(function=command_validate)
    fetch = commands.add_parser("fetch")
    fetch.add_argument("catalog_dir", type=Path)
    fetch.add_argument("local_dir", type=Path)
    fetch.add_argument("--bootstrap", action="store_true")
    fetch.set_defaults(function=command_fetch)
    prepare = commands.add_parser("prepare")
    prepare.add_argument("catalog_dir", type=Path)
    prepare.add_argument("local_dir", type=Path)
    prepare.add_argument("prepared_dir", type=Path)
    prepare.set_defaults(function=command_prepare)
    verify = commands.add_parser("verify")
    verify.add_argument("catalog_dir", type=Path)
    verify.add_argument("prepared_dir", type=Path)
    verify.set_defaults(function=command_verify)
    return parser


def main() -> int:
    args = build_parser().parse_args()
    try:
        args.function(args)
    except (DatasetError, OSError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
