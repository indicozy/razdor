#!/usr/bin/env python3
"""Writes the terrain picture of the example package's first map.

    python3 -I tools/pack-example-terrain.py [out.png]

The picture is 50×50 cells, an indexed PNG whose palette is the engine's default terrain palette
(`src/pack/terrain.rs`, DEFAULT_PALETTE), in the engine's order, so a palette index is a
terrain code. Stdlib only; the output is committed (it is Razdor's own content). The map it
paints is described in `examples/packs/hello-story/maps/first-light/map.toml`: a meadow, a
river (coastal water) with a ford of shallows, a road across the ford, two lowland patches, a
marsh and a stony hill, and the open meadow.
"""
import struct
import sys
import zlib
from pathlib import Path

# The engine's default palette, in the order of its terrain codes (src/pack/terrain.rs).
PALETTE = [
    ("shallows_fords", (0x8F, 0xD0, 0xE0)),
    ("coastal_water", (0x3A, 0x7F, 0xBF)),
    ("deep_sea", (0x1D, 0x4F, 0x8A)),
    ("lava_fields", (0xC8, 0x3A, 0x1A)),
    ("road", (0xB0, 0x88, 0x50)),
    ("grass_lowland", (0x9C, 0xC4, 0x6A)),
    ("grass_plain", (0x6F, 0xA8, 0x4A)),
    ("dry_plain", (0xC9, 0xB5, 0x6A)),
    ("marsh", (0x5F, 0x7A, 0x4A)),
    ("impassable_swamp", (0x3D, 0x4F, 0x32)),
    ("sand_dunes", (0xE6, 0xD2, 0x8A)),
    ("clay_soil", (0xA0, 0x70, 0x4A)),
    ("stony_soil", (0x8A, 0x8A, 0x80)),
    ("scorched_land", (0x4A, 0x3A, 0x33)),
    ("snowy_ground", (0xF0, 0xF4, 0xF8)),
    ("impassable_snowdrifts", (0xC8, 0xD8, 0xE8)),
]
CODE = {name: i for i, (name, _) in enumerate(PALETTE)}
SIZE = 50


def paint():
    """The cells, row by row (row 0 at the top), as palette codes."""
    cells = [[CODE["grass_plain"]] * SIZE for _ in range(SIZE)]

    def fill(x0, y0, x1, y1, kind):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                cells[y][x] = CODE[kind]

    # Two lowland patches on the meadow, a dry patch in the south-west.
    fill(4, 30, 15, 38, "grass_lowland")
    fill(34, 6, 44, 11, "grass_lowland")
    fill(2, 40, 9, 46, "dry_plain")
    # A marsh in the north-east and a stony rise in the north-west.
    fill(40, 2, 46, 4, "marsh")
    fill(6, 3, 10, 6, "stony_soil")
    # The river, from the north edge to the south edge, with a ford of shallows.
    fill(24, 0, 26, 49, "coastal_water")
    fill(24, 21, 26, 27, "shallows_fords")
    # The road from the town across the ford and on to the east.
    fill(11, 24, 23, 24, "road")
    fill(27, 24, 40, 24, "road")
    return cells


def png(cells):
    """An indexed (colour type 3) PNG of the cells, with the palette in the engine's order."""
    height = len(cells)
    width = len(cells[0])
    raw = bytearray()
    for row in cells:
        raw.append(0)  # filter: none
        raw.extend(row)

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    header = struct.pack(">IIBBBBB", width, height, 8, 3, 0, 0, 0)
    plte = b"".join(bytes(rgb) for _, rgb in PALETTE)
    return b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", header) + chunk(b"PLTE", plte) + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b"")


def main():
    out = Path(sys.argv[1] if len(sys.argv) > 1 else "examples/packs/hello-story/maps/first-light/terrain.png")
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png(paint()))
    print(f"wrote {out} ({SIZE}×{SIZE}, {len(PALETTE)} terrain colours)")


if __name__ == "__main__":
    main()
