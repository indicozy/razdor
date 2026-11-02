#!/usr/bin/env python3
"""Writes the terrain picture of chapter 3б «Солончаки» of the Pepelnaya Marka package.

    python3 -I tools/pepelnaya-marka-terrain-salt.py [out.png]

The picture is 100×100 cells, an indexed PNG whose palette is the engine's default terrain palette
(`src/pack/terrain.rs`, DEFAULT_PALETTE), in the engine's order, so a palette index is a terrain
code. Stdlib only; the output is committed (it is Razdor's own content). The map it paints is
described in `examples/packs/pepelnaya-marka/maps/salt/map.toml`: the salt marsh over most of the
west, the causeway that runs north from the marsh to the watch, the marsh folk's dry island of
Reedmere and the witch's island, the hidden trail through the bog, and the salt flats in the east
with the Salt Gate.
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
SIZE = 100


def paint():
    """The cells, row by row (row 0 at the top), as palette codes."""
    # The salt marsh is the base; the flats of the east are pale sand.
    cells = [[CODE["marsh"]] * SIZE for _ in range(SIZE)]

    def fill(x0, y0, x1, y1, kind):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                cells[y][x] = CODE[kind]

    # The swamp along the north edge and two deep pools in the bog (impassable).
    fill(0, 0, 59, 3, "impassable_swamp")
    fill(8, 18, 20, 28, "impassable_swamp")
    fill(56, 76, 64, 84, "impassable_swamp")
    # The salt flats of the east, and the dry ground of the Salt Gate's road.
    fill(60, 0, 99, 99, "sand_dunes")
    fill(56, 14, 62, 22, "dry_plain")
    # The marsh folk's island of Reedmere, and the witch's island in the north-west.
    fill(14, 52, 30, 66, "grass_plain")
    fill(6, 36, 16, 44, "dry_plain")
    # The causeway: a road across the marsh, from the south end to the watch's north end.
    fill(33, 12, 35, 90, "road")
    # The hidden trail of Tess: dry ground through the bog, from the causeway's east to the flats.
    fill(44, 52, 47, 55, "dry_plain")
    fill(48, 46, 55, 51, "dry_plain")
    fill(56, 38, 62, 45, "dry_plain")
    # The ruined den of the bandits, the ground of the smugglers' start, the bog of the ghosts.
    fill(46, 70, 50, 74, "stony_soil")
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
    default = Path(__file__).resolve().parent.parent / "examples/packs/pepelnaya-marka/maps/salt/terrain.png"
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png(paint()))
    print(f"wrote {out} ({SIZE}×{SIZE}, {len(PALETTE)} terrain colours)")


if __name__ == "__main__":
    main()
