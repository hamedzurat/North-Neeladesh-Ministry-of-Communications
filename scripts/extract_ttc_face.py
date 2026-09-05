#!/usr/bin/env python3
"""Extract one TrueType face from a TrueType Collection."""

from pathlib import Path
import struct
import sys


def u16(data: bytes, offset: int) -> int:
    return struct.unpack_from(">H", data, offset)[0]


def u32(data: bytes, offset: int) -> int:
    return struct.unpack_from(">I", data, offset)[0]


def align4(value: int) -> int:
    return (value + 3) & ~3


def extract(source: Path, destination: Path, face_index: int) -> None:
    data = source.read_bytes()
    if len(data) < 12 or data[:4] != b"ttcf":
        raise ValueError(f"{source} is not a TrueType Collection")

    face_count = u32(data, 8)
    offsets_end = 12 + face_count * 4
    if face_index < 0 or face_index >= face_count or offsets_end > len(data):
        raise ValueError(f"invalid face index {face_index} in {source}")

    face_offset = u32(data, 12 + face_index * 4)
    if face_offset + 12 > len(data):
        raise ValueError("face directory is outside the collection")

    sfnt_version = data[face_offset : face_offset + 4]
    table_count = u16(data, face_offset + 4)
    directory_end = face_offset + 12 + table_count * 16
    if directory_end > len(data):
        raise ValueError("face table directory is outside the collection")

    tables = []
    for index in range(table_count):
        record = face_offset + 12 + index * 16
        tag = data[record : record + 4]
        checksum = u32(data, record + 4)
        table_offset = u32(data, record + 8)
        table_length = u32(data, record + 12)
        table_end = table_offset + table_length
        if table_end > len(data):
            raise ValueError(f"table {tag!r} is outside the collection")
        tables.append((tag, checksum, data[table_offset:table_end]))

    output = bytearray()
    output.extend(sfnt_version)
    output.extend(data[face_offset + 4 : face_offset + 12])
    directory_start = len(output)
    output.extend(b"\0" * (table_count * 16))

    new_offsets = {}
    for tag, checksum, table_data in tables:
        table_offset = align4(len(output))
        output.extend(b"\0" * (table_offset - len(output)))
        new_offsets[tag] = table_offset
        output.extend(table_data)

    head_offset = None
    for index, (tag, checksum, table_data) in enumerate(tables):
        record = directory_start + index * 16
        table_offset = new_offsets[tag]
        output[record : record + 4] = tag
        output[record + 4 : record + 8] = struct.pack(">I", checksum)
        output[record + 8 : record + 12] = struct.pack(">I", table_offset)
        output[record + 12 : record + 16] = struct.pack(">I", len(table_data))
        if tag == b"head":
            head_offset = table_offset

    if head_offset is not None and head_offset + 12 <= len(output):
        output[head_offset + 8 : head_offset + 12] = b"\0\0\0\0"
        checksum = sum(
            int.from_bytes(output[index : index + 4].ljust(4, b"\0"), "big")
            for index in range(0, len(output), 4)
        ) & 0xFFFFFFFF
        adjustment = (0xB1B0AFBA - checksum) & 0xFFFFFFFF
        output[head_offset + 8 : head_offset + 12] = struct.pack(">I", adjustment)

    destination.parent.mkdir(parents=True, exist_ok=True)
    destination.write_bytes(output)


def main() -> int:
    if len(sys.argv) not in (3, 4):
        print(f"usage: {sys.argv[0]} SOURCE.ttc DESTINATION.ttf [FACE_INDEX]", file=sys.stderr)
        return 2

    try:
        extract(Path(sys.argv[1]), Path(sys.argv[2]), int(sys.argv[3]) if len(sys.argv) == 4 else 0)
    except (OSError, ValueError, struct.error) as error:
        print(f"font extraction failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
