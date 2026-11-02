#!/usr/bin/env python3
"""Writes the terrain picture of chapter 2 «Марка» of the Pepelnaya Marka package.

    python3 -I tools/pepelnaya-marka-terrain-marka.py [out.png]

The picture is 100×100 cells, an indexed PNG whose palette is the engine's default terrain palette
(`src/pack/terrain.rs`, DEFAULT_PALETTE), in the engine's order, so a palette index is a terrain
code. Stdlib only; the output is committed (it is Razdor's own content). The map it paints is
described in `examples/packs/pepelnaya-marka/maps/marka/map.toml`: the river Pepelka from the north
edge to the south edge with three fords, the burned west bank with the ruined abbey (north-west),
the salt marsh with its pans (south-west), the burned east bank with the garrison town at the
middle ford, the mill village (north-east of the town), the barrow hills (north-east), and the
Free Companies' camp on grass (south-east). The far north-east corner is impassable snow with the
victory's reach cell in it.
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
    """The cells, row by row (row 0 at the top), as palette codes. Later fills win."""
    # The March is dry plain, burned where the farms were.
    cells = [[CODE["dry_plain"]] * SIZE for _ in range(SIZE)]

    def fill(x0, y0, x1, y1, kind):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                cells[y][x] = CODE[kind]

    # West bank: the abbey's unburned garden in the north-west, the woods and fields by the
    # shrine, the burned ground by the road, and the salt marsh in the south-west.
    fill(3, 3, 26, 22, "grass_lowland")
    fill(5, 4, 10, 8, "stony_soil")
    fill(3, 26, 22, 44, "grass_plain")
    fill(26, 50, 44, 56, "scorched_land")
    fill(24, 32, 32, 40, "stony_soil")
    fill(2, 60, 34, 94, "marsh")
    fill(2, 86, 8, 92, "impassable_swamp")
    fill(22, 64, 26, 70, "impassable_swamp")
    fill(26, 58, 40, 66, "sand_dunes")
    fill(13, 67, 19, 73, "dry_plain")

    # East bank: the burned farmland, the garrison town's fields, the mill's meadow, the barrow
    # hills, the mountains by the north-east road, and the camp's grass in the south-east.
    fill(50, 50, 99, 78, "scorched_land")
    fill(68, 36, 99, 70, "scorched_land")
    fill(52, 38, 66, 52, "grass_plain")
    fill(64, 24, 76, 36, "grass_lowland")
    fill(78, 2, 93, 22, "stony_soil")
    fill(90, 40, 99, 56, "stony_soil")
    fill(78, 74, 94, 88, "grass_plain")
    fill(60, 86, 76, 99, "grass_lowland")

    # The far north-east corner: snow the hero cannot cross. The victory's reach cell is a single
    # grass cell inside it, walled in by the snow on all eight sides, so no walker reaches it.
    fill(94, 0, 99, 5, "impassable_snowdrifts")
    fill(98, 2, 98, 2, "grass_plain")

    # The roads: the central road across the middle ford, the north road to the abbey and across
    # the north ford, the marsh spur, the caravan road to the camp, the road to the mill, and the
    # south road across the south ford.
    fill(16, 47, 57, 48, "road")
    fill(28, 20, 29, 46, "road")
    fill(22, 18, 57, 19, "road")
    fill(16, 48, 17, 66, "road")
    fill(58, 49, 59, 79, "road")
    fill(24, 79, 84, 80, "road")
    fill(58, 30, 59, 43, "road")
    fill(58, 30, 70, 31, "road")

    # The river Pepelka from the north edge to the south edge, and its three fords (the roads'
    # cells on the river become fords).
    fill(47, 0, 49, 99, "coastal_water")
    fill(47, 18, 49, 19, "shallows_fords")
    fill(47, 47, 49, 48, "shallows_fords")
    fill(47, 79, 49, 80, "shallows_fords")
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
    default = Path(__file__).resolve().parent.parent / "examples/packs/pepelnaya-marka/maps/marka/terrain.png"
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png(paint()))
    print(f"wrote {out} ({SIZE}×{SIZE}, {len(PALETTE)} terrain colours)")


if __name__ == "__main__":
    main()
