#!/usr/bin/env python3
"""Writes the terrain picture of chapter 5 «Курганы» of the Pepelnaya Marka package.

    python3 -I tools/pepelnaya-marka-terrain-barrows.py [out.png]

The picture is 200×200 cells, an indexed PNG whose palette is the engine's default terrain palette
(`src/pack/terrain.rs`, DEFAULT_PALETTE), in the engine's order, so a palette index is a terrain
code. Stdlib only; the output is committed (it is Razdor's own content). The map it paints is
described in `examples/packs/pepelnaya-marka/maps/barrows/map.toml`: the green fields of the south,
the river Ash across the middle with its one ford, the burned centre, the marsh in the south-west,
the dunes in the east, and the barrow country in the north-east with the King's palace on its hill
under the snow.
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
SIZE = 200


def paint():
    """The cells, row by row (row 0 at the top), as palette codes."""
    cells = [[CODE["dry_plain"]] * SIZE for _ in range(SIZE)]

    def fill(x0, y0, x1, y1, kind):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                cells[y][x] = CODE[kind]

    # The west: meadows in the north-west, the burned centre, the marsh in the south-west.
    fill(0, 0, 110, 70, "grass_plain")
    fill(0, 0, 60, 40, "grass_lowland")
    fill(60, 60, 118, 117, "scorched_land")
    fill(0, 150, 38, 199, "marsh")
    fill(0, 170, 10, 190, "impassable_swamp")
    fill(12, 172, 22, 180, "dry_plain")
    # The east: the barrow country (stony north, burned below), the snow on its ridge, the dunes.
    fill(118, 0, 199, 39, "stony_soil")
    fill(120, 40, 199, 90, "scorched_land")
    fill(184, 0, 199, 18, "impassable_snowdrifts")
    fill(170, 90, 199, 140, "sand_dunes")
    # The river Ash from the west edge to the east edge; its ford is shallows.
    fill(0, 118, 199, 124, "coastal_water")
    fill(94, 118, 106, 124, "shallows_fords")
    # The clay of the ford's south bank, and the stony ground of the chapel, the keep and the camp.
    fill(90, 130, 110, 150, "clay_soil")
    fill(44, 166, 52, 174, "stony_soil")
    fill(56, 146, 64, 154, "stony_soil")
    fill(170, 156, 182, 164, "clay_soil")
    # The palace's hill: stony ground round the throne, and the barrow mounds on the way.
    fill(160, 20, 172, 32, "stony_soil")
    # The road: from the hero's start north to the ford, over it, and on to the palace.
    fill(100, 125, 100, 192, "road")
    fill(100, 100, 100, 117, "road")
    for t in range(0, 61):
        cells[100 - t][100 + t] = CODE["road"]
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
    default = Path(__file__).resolve().parent.parent / "examples/packs/pepelnaya-marka/maps/barrows/terrain.png"
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png(paint()))
    print(f"wrote {out} ({SIZE}×{SIZE}, {len(PALETTE)} terrain colours)")


if __name__ == "__main__":
    main()
