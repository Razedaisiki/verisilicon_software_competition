#!/usr/bin/env python3
"""Test the stdlib-only development PNG converter with synthetic fixtures."""

from __future__ import annotations

import binascii
import struct
import subprocess
import sys
import tempfile
import zlib
from pathlib import Path

from convert_div2k import (
    MAX_SOURCE_PNG_BYTES,
    ConverterError,
    decode_png,
    discover_candidates,
)


CONVERTER = Path(__file__).with_name("convert_div2k.py")


def chunk(kind: bytes, data: bytes) -> bytes:
    crc = binascii.crc32(kind)
    crc = binascii.crc32(data, crc) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", crc)


def paeth(left: int, above: int, upper_left: int) -> int:
    estimate = left + above - upper_left
    distances = (abs(estimate - left), abs(estimate - above), abs(estimate - upper_left))
    if distances[0] <= distances[1] and distances[0] <= distances[2]:
        return left
    if distances[1] <= distances[2]:
        return above
    return upper_left


def filtered_rows(rgb: bytes, width: int, height: int, filters: list[int]) -> bytes:
    row_size = width * 3
    encoded = bytearray()
    previous = bytes(row_size)
    for row_index in range(height):
        filter_type = filters[row_index]
        current = rgb[row_index * row_size : (row_index + 1) * row_size]
        encoded.append(filter_type)
        for index, value in enumerate(current):
            left = current[index - 3] if index >= 3 else 0
            above = previous[index]
            upper_left = previous[index - 3] if index >= 3 else 0
            if filter_type == 0:
                predictor = 0
            elif filter_type == 1:
                predictor = left
            elif filter_type == 2:
                predictor = above
            elif filter_type == 3:
                predictor = (left + above) // 2
            else:
                predictor = paeth(left, above, upper_left)
            encoded.append((value - predictor) & 0xFF)
        previous = current
    return bytes(encoded)


def make_png(
    width: int,
    height: int,
    rgb: bytes,
    filters: list[int],
    *,
    bit_depth: int = 8,
    color_type: int = 2,
    critical_chunk: bytes | None = None,
) -> bytes:
    ihdr = struct.pack(">IIBBBBB", width, height, bit_depth, color_type, 0, 0, 0)
    compressed = zlib.compress(filtered_rows(rgb, width, height, filters))
    split = max(1, len(compressed) // 2)
    pieces = [
        b"\x89PNG\r\n\x1a\n",
        chunk(b"IHDR", ihdr),
        chunk(b"tIME", struct.pack(">HBBBBB", 2026, 1, 2, 3, 4, 5)),
    ]
    if critical_chunk is not None:
        pieces.append(chunk(critical_chunk, b"fixture"))
    pieces.extend(
        [chunk(b"IDAT", compressed[:split]), chunk(b"IDAT", compressed[split:]), chunk(b"IEND", b"")]
    )
    return b"".join(pieces)


def run_converter(output_format: str, input_dir: Path, output_dir: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [
            sys.executable,
            str(CONVERTER),
            "--format",
            output_format,
            str(input_dir),
            str(output_dir),
        ],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        errors="replace",
        check=False,
    )


def expect_decode_error(data: bytes, expected: str) -> None:
    try:
        decode_png(data)
    except ConverterError as error:
        if str(error) != expected:
            raise AssertionError(f"expected {expected!r}, received {str(error)!r}") from error
    else:
        raise AssertionError(f"expected decode failure: {expected}")


def main() -> int:
    width, height = 4, 5
    rgb = bytes((index * 37 + 11) & 0xFF for index in range(width * height * 3))
    all_filters_png = make_png(width, height, rgb, [0, 1, 2, 3, 4])
    second_rgb = bytes((index * 19 + 7) & 0xFF for index in range(3 * 2 * 3))
    second_png = make_png(3, 2, second_rgb, [4, 1])
    decoded = decode_png(all_filters_png)
    assert (decoded.width, decoded.height, decoded.rgb) == (width, height, rgb)

    with tempfile.TemporaryDirectory(prefix="div2k-converter-check-") as temporary:
        root = Path(temporary)
        input_dir = root / "input"
        (input_dir / "z").mkdir(parents=True)
        (input_dir / "a").mkdir(parents=True)
        (input_dir / "z" / "second.png").write_bytes(second_png)
        (input_dir / "a" / "first.PNG").write_bytes(all_filters_png)
        relative = [path.relative_to(input_dir).as_posix() for path in discover_candidates(input_dir)]
        assert relative == ["a/first.PNG", "z/second.png"]

        ppm_output = root / "ppm"
        ppm_result = run_converter("ppm", input_dir, ppm_output)
        assert ppm_result.returncode == 0, ppm_result.stderr
        assert ppm_result.stdout == f"Converted 2 PNG files to PPM P6 under {ppm_output}\n"
        expected_ppm = f"P6\n{width} {height}\n255\n".encode("ascii") + rgb
        assert (ppm_output / "a" / "first.ppm").read_bytes() == expected_ppm
        assert (ppm_output / "z" / "second.ppm").read_bytes() == b"P6\n3 2\n255\n" + second_rgb

        raw_output = root / "raw"
        raw_result = run_converter("raw", input_dir, raw_output)
        assert raw_result.returncode == 0, raw_result.stderr
        assert raw_result.stdout == f"Converted 2 PNG files to packed RGB888 under {raw_output}\n"
        assert (raw_output / "a" / "first.raw").read_bytes() == rgb
        assert (raw_output / "z" / "second.raw").read_bytes() == second_rgb

        before = (ppm_output / "a" / "first.ppm").read_bytes()
        overwrite = run_converter("ppm", input_dir, ppm_output)
        assert overwrite.returncode == 1
        assert overwrite.stderr == "Error: output directory already exists\n"
        assert (ppm_output / "a" / "first.ppm").read_bytes() == before

        empty = root / "empty"
        empty.mkdir()
        no_candidates = run_converter("raw", empty, root / "unused")
        assert no_candidates.returncode == 1
        assert no_candidates.stderr == "Error: input directory contains no PNG candidates\n"

        oversized_input = root / "oversized-input"
        oversized_input.mkdir()
        with (oversized_input / "oversized.png").open("wb") as oversized_file:
            oversized_file.seek(MAX_SOURCE_PNG_BYTES)
            oversized_file.write(b"\0")
        oversized_source = run_converter("raw", oversized_input, root / "oversized-output")
        assert oversized_source.returncode == 1
        assert oversized_source.stderr == "Error: source PNG exceeds the 64 MiB limit\n"
        assert not (root / "oversized-output").exists()

    corrupt_crc = bytearray(all_filters_png)
    idat = corrupt_crc.index(b"IDAT")
    corrupt_crc[idat + 4] ^= 1
    expect_decode_error(bytes(corrupt_crc), "PNG chunk CRC mismatch")
    unsupported = make_png(width, height, rgb, [0, 1, 2, 3, 4], bit_depth=16)
    expect_decode_error(unsupported, "PNG must be noninterlaced 8-bit RGB truecolor")
    unknown_critical = make_png(
        width, height, rgb, [0, 1, 2, 3, 4], critical_chunk=b"ABCD"
    )
    expect_decode_error(unknown_critical, "unsupported critical PNG chunk: ABCD")
    reserved_bit = make_png(
        width, height, rgb, [0, 1, 2, 3, 4], critical_chunk=b"ABcD"
    )
    expect_decode_error(reserved_bit, "PNG chunk type uses the reserved bit")
    oversized_pixel_ihdr = struct.pack(">IIBBBBB", 4097, 4097, 8, 2, 0, 0, 0)
    oversized_pixels = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            chunk(b"IHDR", oversized_pixel_ihdr),
            chunk(b"IDAT", zlib.compress(b"")),
            chunk(b"IEND", b""),
        ]
    )
    expect_decode_error(oversized_pixels, "PNG dimensions exceed the decoded pixel limit")
    oversized_rgb_ihdr = struct.pack(">IIBBBBB", 4096, 3500, 8, 2, 0, 0, 0)
    oversized_rgb = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            chunk(b"IHDR", oversized_rgb_ihdr),
            chunk(b"IDAT", zlib.compress(b"")),
            chunk(b"IEND", b""),
        ]
    )
    expect_decode_error(oversized_rgb, "PNG dimensions exceed the decoded RGB byte limit")
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    compressed = zlib.compress(filtered_rows(rgb, width, height, [0, 1, 2, 3, 4]))
    split = len(compressed) // 2
    nonconsecutive = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            chunk(b"IHDR", ihdr),
            chunk(b"IDAT", compressed[:split]),
            chunk(b"tIME", struct.pack(">HBBBBB", 2026, 1, 2, 3, 4, 5)),
            chunk(b"IDAT", compressed[split:]),
            chunk(b"IEND", b""),
        ]
    )
    expect_decode_error(nonconsecutive, "IDAT chunks must be consecutive")
    compressed_trailing = b"".join(
        [
            b"\x89PNG\r\n\x1a\n",
            chunk(b"IHDR", ihdr),
            chunk(b"IDAT", compressed + b"extra"),
            chunk(b"IEND", b""),
        ]
    )
    expect_decode_error(
        compressed_trailing, "PNG zlib stream has an unexpected size or trailing data"
    )
    expect_decode_error(all_filters_png[:-1], "truncated PNG chunk")
    expect_decode_error(all_filters_png + b"extra", "trailing data after IEND")

    print("DIV2K converter check passed with filters 0 through 4 and failure coverage.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
