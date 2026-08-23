#!/usr/bin/env python3
"""Offline synthetic checks for eval_dataset.py."""

from __future__ import annotations

import contextlib
import csv
import hashlib
import io
import json
import tempfile
import threading
import urllib.parse
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import eval_dataset


def expect_error(action, text: str) -> None:
    try:
        action()
    except eval_dataset.DatasetError as error:
        if text not in str(error):
            raise AssertionError(f"expected error containing {text!r}, found {error!r}") from error
    else:
        raise AssertionError(f"expected DatasetError containing {text!r}")


def png_bytes(index: int) -> bytes:
    from PIL import Image

    image = Image.new("RGB", (36, 22))
    pixels = []
    for y in range(22):
        for x in range(36):
            pixels.append(((x * 7 + index) & 255, (y * 11 + index * 3) & 255, (x * 5 + y * 9 + index * 13) & 255))
    image.putdata(pixels)
    output = io.BytesIO()
    image.save(output, format="PNG", optimize=False, compress_level=6)
    return output.getvalue()


def entry(category: str, index: int, digest: str | None) -> dict[str, object]:
    entry_id = f"{category}-{index:02d}"
    return {
        "id": entry_id,
        "title": f"Synthetic {entry_id}",
        "author": "Dataset checker",
        "source_page": f"https://fixtures.invalid/page/{entry_id}",
        "download_url": f"https://fixtures.invalid/files/{entry_id}.png",
        "license": "CC0-1.0",
        "license_url": "https://creativecommons.org/publicdomain/zero/1.0/",
        "source_width": 36,
        "source_height": 22,
        "source_mime": "image/png",
        "target_width": 32,
        "target_height": 18,
        "sha256": digest,
        "notes": "Synthetic checker fixture only.",
    }


def write_catalogs(directory: Path, digests: dict[str, str] | None = None) -> None:
    directory.mkdir(parents=True, exist_ok=True)
    for category in sorted(eval_dataset.CATEGORIES):
        entries = []
        for index in range(10):
            entry_id = f"{category}-{index:02d}"
            entries.append(entry(category, index, None if digests is None else digests[entry_id]))
        payload = {"schema_version": 1, "category": category, "entries": entries}
        (directory / f"{category}.json").write_text(json.dumps(payload, indent=2, sort_keys=True) + "\n", encoding="ascii")


class FixtureHandler(BaseHTTPRequestHandler):
    payloads: dict[str, bytes] = {}
    user_agents: list[str] = []

    def do_GET(self) -> None:
        payload = self.payloads.get(self.path)
        if payload is None:
            self.send_error(404)
            return
        self.user_agents.append(self.headers.get("User-Agent", ""))
        self.send_response(200)
        self.send_header("Content-Type", "image/png")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, format: str, *args: object) -> None:
        pass


class HttpsIdentityResponse:
    def __init__(self, response, original_url: str):
        self.response = response
        self.original_url = original_url
        self.headers = response.headers

    def read(self, size: int = -1) -> bytes:
        return self.response.read(size)

    def geturl(self) -> str:
        return self.original_url

    def __enter__(self):
        self.response.__enter__()
        return self

    def __exit__(self, kind, value, traceback):
        return self.response.__exit__(kind, value, traceback)


def ppm(path: Path) -> tuple[int, int, bytes]:
    data = path.read_bytes()
    magic, dimensions, maximum, raster = data.split(b"\n", 3)
    assert magic == b"P6" and maximum == b"255"
    width_text, height_text = dimensions.split()
    width, height = int(width_text), int(height_text)
    assert len(raster) == width * height * 3
    return width, height, raster


def directory_hashes(directory: Path) -> dict[str, str]:
    return {
        path.relative_to(directory).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
        for path in sorted(directory.rglob("*"))
        if path.is_file()
    }


def main() -> int:
    Image, _ = eval_dataset._require_pillow()
    if Image.__version__ != eval_dataset.PILLOW_VERSION:
        raise AssertionError("Pillow version check did not enforce 11.3.0")
    assert Image.MAX_IMAGE_PIXELS == eval_dataset.MAX_SOURCE_PIXELS

    with tempfile.TemporaryDirectory(prefix="eval-dataset-check-") as temporary_text:
        temporary = Path(temporary_text)
        catalogs = temporary / "catalogs"
        write_catalogs(catalogs)

        expect_error(lambda: eval_dataset.load_catalogs(catalogs, locked=False), "at least 2560x1440")
        eval_dataset.MIN_TARGET_WIDTH = 32
        eval_dataset.MIN_TARGET_HEIGHT = 18
        eval_dataset.REQUEST_DELAY_SECONDS = 0.0
        eval_dataset.RETRY_DELAYS_SECONDS = (0.0, 0.0, 0.0, 0.0, 0.0)
        bootstrap_entries = eval_dataset.load_catalogs(catalogs, locked=False)
        assert len(bootstrap_entries) == 30
        expect_error(lambda: eval_dataset.load_catalogs(catalogs, locked=True), "locked catalogs")

        oriented_path = temporary / "oriented.jpg"
        oriented_source = Image.new("RGB", (22, 36), (17, 34, 51))
        exif = Image.Exif()
        exif[274] = 6
        oriented_source.save(oriented_path, format="JPEG", exif=exif)
        oriented_entry = eval_dataset.Entry(
            **{
                **bootstrap_entries[0].__dict__,
                "id": "oriented-check",
                "source_mime": "image/jpeg",
                "source_width": 36,
                "source_height": 22,
            }
        )
        eval_dataset._validate_decoded(oriented_path, oriented_entry)

        nature_path = catalogs / "nature.json"
        original_nature = nature_path.read_bytes()
        malformed = json.loads(original_nature)
        malformed["entries"][0]["unexpected"] = True
        nature_path.write_text(json.dumps(malformed), encoding="ascii")
        expect_error(lambda: eval_dataset.load_catalogs(catalogs, locked=False), "entry keys mismatch")
        nature_path.write_bytes(original_nature)
        malformed = json.loads(original_nature)
        malformed["entries"][0]["source_width"] = 20_000
        malformed["entries"][0]["source_height"] = 10_000
        nature_path.write_text(json.dumps(malformed), encoding="ascii")
        expect_error(lambda: eval_dataset.load_catalogs(catalogs, locked=False), "180,000,000 pixel")
        nature_path.write_bytes(original_nature)
        malformed = json.loads(original_nature)
        malformed["entries"][0]["title"] = "non-ascii-\u2603"
        nature_path.write_text(json.dumps(malformed, ensure_ascii=False), encoding="utf-8")
        expect_error(lambda: eval_dataset.load_catalogs(catalogs, locked=False), "ASCII JSON")
        nature_path.write_bytes(original_nature)

        FixtureHandler.payloads = {}
        for category_index, category in enumerate(sorted(eval_dataset.CATEGORIES)):
            for index in range(10):
                entry_id = f"{category}-{index:02d}"
                FixtureHandler.payloads[f"/files/{entry_id}.png"] = png_bytes(category_index * 10 + index)
        server = ThreadingHTTPServer(("127.0.0.1", 0), FixtureHandler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        original_open = eval_dataset._open_url

        def local_open(request: urllib.request.Request):
            parsed = urllib.parse.urlsplit(request.full_url)
            local_url = f"http://127.0.0.1:{server.server_port}{parsed.path}"
            local_request = urllib.request.Request(local_url, headers=dict(request.header_items()))
            return HttpsIdentityResponse(urllib.request.urlopen(local_request), request.full_url)

        eval_dataset._open_url = local_open
        try:
            catalog_before = {path.name: path.read_bytes() for path in catalogs.glob("*.json")}
            downloads = temporary / "downloads"
            stdout = io.StringIO()
            with contextlib.redirect_stdout(stdout):
                bootstrap_results = eval_dataset.fetch_entries(
                    eval_dataset.load_catalogs(catalogs, locked=False), downloads, bootstrap=True
                )
            assert len(bootstrap_results) == 30
            lines = stdout.getvalue().splitlines()
            assert lines == sorted(lines) and len(lines) == 30
            assert all(len(line.split()) == 2 and len(line.split()[1]) == 64 for line in lines)
            assert catalog_before == {path.name: path.read_bytes() for path in catalogs.glob("*.json")}
            assert FixtureHandler.user_agents and set(FixtureHandler.user_agents) == {eval_dataset.USER_AGENT}

            digests = {entry_id: digest for entry_id, digest in bootstrap_results}
            locked_catalogs = temporary / "locked-catalogs"
            write_catalogs(locked_catalogs, digests)
            entries = eval_dataset.load_catalogs(locked_catalogs, locked=True)

            def forbidden_network(request):
                raise AssertionError(f"locked reuse unexpectedly accessed network: {request.full_url}")

            eval_dataset._open_url = forbidden_network
            assert eval_dataset.fetch_entries(entries, downloads, bootstrap=False) == []

            first = entries[0]
            first_path = downloads / first.local_name()
            correct = first_path.read_bytes()
            first_path.write_bytes(correct + b"tamper")
            expect_error(
                lambda: eval_dataset.fetch_entries(entries, downloads, bootstrap=False),
                "refusing overwrite",
            )
            assert first_path.read_bytes() == correct + b"tamper"
            first_path.write_bytes(correct)

            redirect_downloads = temporary / "redirect-downloads"

            class InsecureResponse:
                headers = {"Content-Length": "0"}

                def geturl(self):
                    return "http://fixtures.invalid/insecure"

                def __enter__(self):
                    return self

                def __exit__(self, kind, value, traceback):
                    return False

                def read(self, size=-1):
                    return b""

            eval_dataset._open_url = lambda request: InsecureResponse()
            expect_error(
                lambda: eval_dataset.fetch_entries([first], redirect_downloads, bootstrap=False),
                "away from HTTPS",
            )

            eval_dataset._open_url = forbidden_network
            prepared = temporary / "prepared"
            eval_dataset.prepare_entries(entries, downloads, prepared)
            eval_dataset.verify_prepared(entries, prepared)
            assert (prepared / "pairs.tsv").read_text("ascii").splitlines()[0] == "id\tlr_path\thr_path"
            assert len((prepared / "pairs.tsv").read_text("ascii").splitlines()) == 31
            assert len((prepared / "attribution.csv").read_text("utf-8").splitlines()) == 31
            assert len((prepared / "dataset-lock.csv").read_text("utf-8").splitlines()) == 31
            lock_header = (prepared / "dataset-lock.csv").read_text("utf-8").splitlines()[0]
            assert "crop_left,crop_top" in lock_header
            with (prepared / "dataset-lock.csv").open(newline="", encoding="utf-8") as lock_file:
                lock_by_id = {row["id"]: row for row in csv.DictReader(lock_file)}
            assert lock_by_id[first.id]["crop_left"] == "2"
            assert lock_by_id[first.id]["crop_top"] == "2"
            metadata = json.loads((prepared / "dataset-metadata.json").read_text("ascii"))
            assert metadata == {
                "entry_count": 30,
                "hr_crop_policy": "center-no-resize",
                "image_format": "RGB8 PPM P6",
                "lr_downsampler": "Pillow-BICUBIC",
                "pillow_version": "11.3.0",
                "scale": 2,
                "schema_version": 1,
            }

            hr_width, hr_height, hr_raster = ppm(prepared / "hr" / f"{first.id}.ppm")
            lr_width, lr_height, lr_raster = ppm(prepared / "lr" / f"{first.id}.ppm")
            assert (hr_width, hr_height) == (32, 18)
            assert (lr_width, lr_height) == (16, 9)
            with Image.open(downloads / first.local_name()) as source:
                expected_hr = source.convert("RGB").crop((2, 2, 34, 20))
                expected_lr = expected_hr.resize((16, 9), resample=Image.Resampling.BICUBIC)
            assert hr_raster == expected_hr.tobytes()
            assert lr_raster == expected_lr.tobytes()

            prepared_second = temporary / "prepared-second"
            eval_dataset.prepare_entries(entries, downloads, prepared_second)
            assert directory_hashes(prepared) == directory_hashes(prepared_second)
            eval_dataset.verify_prepared(entries, prepared_second)
            expect_error(lambda: eval_dataset.prepare_entries(entries, downloads, prepared), "refusing to overwrite")

            metadata_path = prepared_second / "dataset-metadata.json"
            metadata_bytes = metadata_path.read_bytes()
            metadata_path.write_bytes(metadata_bytes.replace(b'"scale": 2', b'"scale": 4'))
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "dataset-metadata.json",
            )
            metadata_path.write_bytes(metadata_bytes)

            pairs_path = prepared_second / "pairs.tsv"
            pairs_bytes = pairs_path.read_bytes()
            pairs_path.write_bytes(pairs_bytes.replace(f"lr/{first.id}.ppm".encode("ascii"), b"../escape.ppm"))
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "unsafe or unexpected pair path",
            )
            pairs_path.write_bytes(pairs_bytes)

            lock_path = prepared_second / "dataset-lock.csv"
            lock_bytes = lock_path.read_bytes()
            locked_hash = lock_by_id[first.id]["hr_sha256"].encode("ascii")
            lock_path.write_bytes(lock_bytes.replace(locked_hash, b"0" * 64, 1))
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "PPM SHA-256 mismatch",
            )
            lock_path.write_bytes(lock_bytes)

            verified_hr = prepared_second / "hr" / f"{first.id}.ppm"
            verified_hr_bytes = verified_hr.read_bytes()
            verified_hr.write_bytes(verified_hr_bytes[:-1] + bytes([verified_hr_bytes[-1] ^ 1]))
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "PPM SHA-256 mismatch",
            )
            verified_hr.write_bytes(verified_hr_bytes[:-1])
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "PPM length mismatch",
            )
            verified_hr.write_bytes(b"Q" + verified_hr_bytes[1:])
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "PPM header or dimensions mismatch",
            )
            verified_hr.write_bytes(verified_hr_bytes)

            missing_path = prepared_second / "lr" / f"{first.id}.ppm"
            missing_bytes = missing_path.read_bytes()
            missing_path.unlink()
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "membership mismatch",
            )
            missing_path.write_bytes(missing_bytes)
            extra_path = prepared_second / "extra.txt"
            extra_path.write_text("extra", encoding="ascii")
            expect_error(
                lambda: eval_dataset.verify_prepared(entries, prepared_second),
                "membership mismatch",
            )
            extra_path.unlink()

            symlink_path = prepared_second / "linked.ppm"
            try:
                symlink_path.symlink_to(verified_hr)
            except OSError:
                pass
            else:
                expect_error(
                    lambda: eval_dataset.verify_prepared(entries, prepared_second),
                    "contains a symlink",
                )
                symlink_path.unlink()
            eval_dataset.verify_prepared(entries, prepared_second)

            failed = temporary / "failed-prepared"
            original_write_ppm = eval_dataset._write_ppm

            def injected_failure(path, image):
                raise eval_dataset.DatasetError("injected prepare failure")

            eval_dataset._write_ppm = injected_failure
            try:
                expect_error(lambda: eval_dataset.prepare_entries(entries, downloads, failed), "injected")
            finally:
                eval_dataset._write_ppm = original_write_ppm
            assert not failed.exists()
            assert not list(temporary.glob(f".{failed.name}.staging-*"))
        finally:
            eval_dataset._open_url = original_open
            server.shutdown()
            server.server_close()
            thread.join(timeout=5)

    print("Evaluation dataset automation check passed with 30 offline synthetic entries.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
