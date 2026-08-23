#!/usr/bin/env python3
"""Convert constrained DIV2K RGB8 PNG files to PPM P6 or packed RGB888."""

from __future__ import annotations

import argparse
import binascii
import concurrent.futures
import os
import shutil
import struct
import sys
import tempfile
import zlib
from dataclasses import dataclass
from pathlib import Path


PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"
BYTES_PER_PIXEL = 3
MAX_SOURCE_PNG_BYTES = 64 * 1024 * 1024
MAX_DECODED_PIXELS = 4096 * 4096
MAX_DECODED_RGB_BYTES = 40 * 1024 * 1024


class ConverterError(ValueError):
    """Stable validation error for developer conversion."""


@dataclass(frozen=True)
class DecodedPng:
    width: int
    height: int
    rgb: bytes


def _error(message: str) -> ConverterError:
    return ConverterError(message)


def _paeth(left: int, above: int, upper_left: int) -> int:
    estimate = left + above - upper_left
    left_distance = abs(estimate - left)
    above_distance = abs(estimate - above)
    upper_left_distance = abs(estimate - upper_left)
    if left_distance <= above_distance and left_distance <= upper_left_distance:
        return left
    if above_distance <= upper_left_distance:
        return above
    return upper_left


def _unfilter(raw: bytes, width: int, height: int) -> bytes:
    row_size = width * BYTES_PER_PIXEL
    expected = (row_size + 1) * height
    if len(raw) != expected:
        raise _error("decompressed PNG data has an unexpected size")
    output = bytearray(row_size * height)
    previous = bytearray(row_size)
    offset = 0
    output_offset = 0
    for _ in range(height):
        filter_type = raw[offset]
        offset += 1
        if filter_type > 4:
            raise _error("PNG scanline uses an unsupported filter")
        filtered = raw[offset : offset + row_size]
        offset += row_size
        current = bytearray(row_size)
        for index, value in enumerate(filtered):
            left = current[index - BYTES_PER_PIXEL] if index >= BYTES_PER_PIXEL else 0
            above = previous[index]
            upper_left = previous[index - BYTES_PER_PIXEL] if index >= BYTES_PER_PIXEL else 0
            if filter_type == 0:
                predictor = 0
            elif filter_type == 1:
                predictor = left
            elif filter_type == 2:
                predictor = above
            elif filter_type == 3:
                predictor = (left + above) // 2
            else:
                predictor = _paeth(left, above, upper_left)
            current[index] = (value + predictor) & 0xFF
        output[output_offset : output_offset + row_size] = current
        output_offset += row_size
        previous = current
    return bytes(output)


def decode_png(data: bytes) -> DecodedPng:
    if len(data) > MAX_SOURCE_PNG_BYTES:
        raise _error("source PNG exceeds the 64 MiB limit")
    if not data.startswith(PNG_SIGNATURE):
        raise _error("invalid PNG signature")
    offset = len(PNG_SIGNATURE)
    width = height = None
    idat_parts: list[bytes] = []
    seen_ihdr = False
    seen_plte = False
    seen_idat = False
    idat_closed = False
    seen_iend = False

    while offset < len(data):
        if len(data) - offset < 12:
            raise _error("truncated PNG chunk")
        length = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        chunk_type = data[offset : offset + 4]
        offset += 4
        if len(chunk_type) != 4 or not all(
            65 <= byte <= 90 or 97 <= byte <= 122 for byte in chunk_type
        ):
            raise _error("invalid PNG chunk type")
        if chunk_type[2] & 0x20:
            raise _error("PNG chunk type uses the reserved bit")
        if length > len(data) - offset - 4:
            raise _error("truncated PNG chunk")
        chunk_data = data[offset : offset + length]
        offset += length
        expected_crc = struct.unpack_from(">I", data, offset)[0]
        offset += 4
        actual_crc = binascii.crc32(chunk_type)
        actual_crc = binascii.crc32(chunk_data, actual_crc) & 0xFFFFFFFF
        if actual_crc != expected_crc:
            raise _error("PNG chunk CRC mismatch")

        if not seen_ihdr and chunk_type != b"IHDR":
            raise _error("IHDR must be the first PNG chunk")
        if chunk_type == b"IHDR":
            if seen_ihdr or length != 13:
                raise _error("PNG must contain one valid IHDR chunk")
            width, height, depth, color, compression, filtering, interlace = struct.unpack(
                ">IIBBBBB", chunk_data
            )
            if width == 0 or height == 0:
                raise _error("PNG dimensions must be positive")
            pixel_count = width * height
            if pixel_count > MAX_DECODED_PIXELS:
                raise _error("PNG dimensions exceed the decoded pixel limit")
            if pixel_count * BYTES_PER_PIXEL > MAX_DECODED_RGB_BYTES:
                raise _error("PNG dimensions exceed the decoded RGB byte limit")
            if (depth, color, compression, filtering, interlace) != (8, 2, 0, 0, 0):
                raise _error("PNG must be noninterlaced 8-bit RGB truecolor")
            seen_ihdr = True
        elif chunk_type == b"PLTE":
            if seen_plte or seen_idat or length == 0 or length > 768 or length % 3 != 0:
                raise _error("invalid PLTE chunk")
            seen_plte = True
        elif chunk_type == b"IDAT":
            if idat_closed:
                raise _error("IDAT chunks must be consecutive")
            seen_idat = True
            idat_parts.append(chunk_data)
        elif chunk_type == b"IEND":
            if length != 0 or not seen_idat:
                raise _error("invalid IEND chunk")
            seen_iend = True
            if offset != len(data):
                raise _error("trailing data after IEND")
            break
        else:
            if seen_idat:
                idat_closed = True
            if chunk_type[0] & 0x20 == 0:
                name = chunk_type.decode("ascii")
                raise _error(f"unsupported critical PNG chunk: {name}")

    if not seen_iend or width is None or height is None:
        raise _error("PNG is missing required structure")
    expected_size = (width * BYTES_PER_PIXEL + 1) * height
    decompressor = zlib.decompressobj()
    try:
        raw = decompressor.decompress(b"".join(idat_parts), expected_size + 1)
        if len(raw) <= expected_size:
            raw += decompressor.flush(expected_size + 1 - len(raw))
    except zlib.error as error:
        raise _error("invalid PNG zlib stream") from error
    if (
        len(raw) != expected_size
        or not decompressor.eof
        or decompressor.unused_data
        or decompressor.unconsumed_tail
    ):
        raise _error("PNG zlib stream has an unexpected size or trailing data")
    return DecodedPng(width, height, _unfilter(raw, width, height))


def discover_candidates(input_directory: Path) -> list[Path]:
    candidates = [
        path
        for path in input_directory.rglob("*")
        if path.is_file() and path.suffix.lower() == ".png"
    ]
    return sorted(candidates, key=lambda path: path.relative_to(input_directory).as_posix())


def _encoded(decoded: DecodedPng, output_format: str) -> bytes:
    if output_format == "raw":
        return decoded.rgb
    header = f"P6\n{decoded.width} {decoded.height}\n255\n".encode("ascii")
    return header + decoded.rgb


def _convert_one(job: tuple[str, str, str]) -> None:
    source_name, destination_name, output_format = job
    source = Path(source_name)
    if source.stat().st_size > MAX_SOURCE_PNG_BYTES:
        raise _error("source PNG exceeds the 64 MiB limit")
    decoded = decode_png(source.read_bytes())
    Path(destination_name).write_bytes(_encoded(decoded, output_format))


def convert_directory(input_directory: Path, output_directory: Path, output_format: str) -> int:
    if not input_directory.is_dir():
        raise _error("input directory does not exist")
    if output_directory.exists():
        raise _error("output directory already exists")
    candidates = discover_candidates(input_directory)
    if not candidates:
        raise _error("input directory contains no PNG candidates")
    extension = ".ppm" if output_format == "ppm" else ".raw"
    destinations = [
        output_directory / source.relative_to(input_directory).with_suffix(extension)
        for source in candidates
    ]
    normalized = [os.path.normcase(str(path.resolve(strict=False))) for path in destinations]
    if len(normalized) != len(set(normalized)):
        raise _error("candidate output paths collide")

    output_directory.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(
        tempfile.mkdtemp(
            prefix=f".{output_directory.name}.tmp-", dir=output_directory.parent
        )
    )
    try:
        jobs: list[tuple[str, str, str]] = []
        for source, destination in zip(candidates, destinations):
            staged_destination = staging / destination.relative_to(output_directory)
            staged_destination.parent.mkdir(parents=True, exist_ok=True)
            jobs.append((str(source), str(staged_destination), output_format))
        worker_count = min(4, os.cpu_count() or 1, len(jobs))
        with concurrent.futures.ProcessPoolExecutor(max_workers=worker_count) as executor:
            for _ in executor.map(_convert_one, jobs):
                pass
        os.replace(staging, output_directory)
    except Exception:
        shutil.rmtree(staging, ignore_errors=True)
        raise
    return len(candidates)


def parse_arguments(arguments: list[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Convert constrained DIV2K PNG files for developer evaluation."
    )
    parser.add_argument("--format", required=True, choices=("ppm", "raw"))
    parser.add_argument("input_directory", type=Path)
    parser.add_argument("output_directory", type=Path)
    return parser.parse_args(arguments)


def main(arguments: list[str] | None = None) -> int:
    options = parse_arguments(sys.argv[1:] if arguments is None else arguments)
    try:
        count = convert_directory(
            options.input_directory, options.output_directory, options.format
        )
    except (ConverterError, OSError) as error:
        print(f"Error: {error}", file=sys.stderr)
        return 1
    label = "PPM P6" if options.format == "ppm" else "packed RGB888"
    print(f"Converted {count} PNG files to {label} under {options.output_directory}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
