#!/usr/bin/env python3
"""Writes the terrain picture of chapter 1 «Брод» of the Pepelnaya Marka package.

    python3 -I tools/pepelnaya-marka-terrain.py [out.png]

The picture is 50×50 cells, an indexed PNG whose palette is the engine's default terrain palette
(`src/pack/terrain.rs`, DEFAULT_PALETTE), in the engine's order, so a palette index is a terrain
code. Stdlib only; the output is committed (it is Razdor's own content). The map it paints is
described in `examples/packs/pepelnaya-marka/maps/brod/map.toml`: burned fields on both banks of
the river Pepelka (coastal water), three fords of shallows across it, the road through the middle
ford, the ruined mill's scorched ground, the stony barrow rise in the north-east, and the marsh in
the south-west with the village on its dry island.
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
    # The March is dry plain, burned where the farms were.
    cells = [[CODE["dry_plain"]] * SIZE for _ in range(SIZE)]

    def fill(x0, y0, x1, y1, kind):
        for y in range(y0, y1 + 1):
            for x in range(x0, x1 + 1):
                cells[y][x] = CODE[kind]

    # West bank: unburned meadow in the north-west, burned fields by the road, the marsh in the
    # south-west with the village's dry island in it, and a stony rise by the woods.
    fill(3, 3, 14, 13, "grass_lowland")
    fill(6, 16, 18, 30, "scorched_land")
    fill(6, 3, 10, 6, "stony_soil")
    fill(2, 33, 16, 47, "marsh")
    fill(2, 40, 5, 46, "impassable_swamp")
    fill(8, 37, 10, 39, "dry_plain")

    # The river Pepelka from the north edge to the south edge, and its three fords.
    fill(23, 0, 25, 49, "coastal_water")
    for top in (8, 22, 38):
        fill(23, top, 25, top + 1, "shallows_fords")

    # East bank: the scorched farmland, the mill's ground, the barrow rise and the tower's stones.
    fill(26, 26, 49, 49, "scorched_land")
    fill(30, 36, 40, 44, "grass_lowland")
    fill(28, 14, 38, 21, "scorched_land")
    fill(38, 0, 49, 12, "stony_soil")
    fill(42, 12, 46, 16, "stony_soil")

    # The roads: east of the river to the hero's start, west of it to the marsh.
    fill(26, 23, 40, 23, "road")
    fill(40, 23, 40, 28, "road")
    fill(11, 23, 22, 23, "road")
    fill(11, 23, 11, 36, "road")
    return blend(cells, [("dry_plain", "clay_soil", 0.62, 7, 101), ("dry_plain", "scorched_land", 0.70, 9, 103), ("scorched_land", "dry_plain", 0.64, 8, 107), ("grass_lowland", "grass_plain", 0.55, 8, 109)], ("dry_plain", "sand_dunes", 211))


# Terrain variety: natural patches in the big uniform fields, and a ragged beach where land meets
# water. Only passable land kinds change into each other (never a road, water, marsh or impassable
# kind), so the routes, fords and story cells stay as they are. The noise is value noise, stdlib only.
def _hash01(x, y, salt):
    h = ((x * 73856093) ^ (y * 19349663) ^ (salt * 83492791)) & 0xFFFFFFFF
    h = ((h ^ (h >> 13)) * 0x5BD1E995) & 0xFFFFFFFF
    h ^= h >> 15
    h = (h * 0x27D4EB2D) & 0xFFFFFFFF
    h ^= h >> 15
    return h / 4294967296.0


def _noise(x, y, size, salt):
    """Value noise in [0, 1] at a scale of `size` cells (bilinear, smoothed)."""
    gx, gy = x / size, y / size
    ix, iy = int(gx), int(gy)
    fx, fy = gx - ix, gy - iy
    fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = _hash01(ix, iy, salt), _hash01(ix + 1, iy, salt)
    c, d = _hash01(ix, iy + 1, salt), _hash01(ix + 1, iy + 1, salt)
    top, bottom = a + (b - a) * fx, c + (d - c) * fx
    return top + (bottom - top) * fy


def blend(cells, rules, shore):
    """`rules`: (base, into, threshold, size, salt): where the noise is above the threshold, a `base`
    cell takes the kind `into` (patches of clay in the dry plain, forest floor in the grass). `shore`:
    (base, into, salt): a `base` cell next to water takes `into` where the noise is high (a ragged beach)."""
    h, w = len(cells), len(cells[0])
    for base, into, threshold, size, salt in rules:
        b, t = CODE[base], CODE[into]
        out = [row[:] for row in cells]
        for y in range(h):
            for x in range(w):
                if cells[y][x] == b and _noise(x, y, size, salt) > threshold:
                    out[y][x] = t
        cells = out
    water = {CODE[k] for k in ("coastal_water", "deep_sea", "shallows_fords")}
    base, into, salt = shore
    b, t = CODE[base], CODE[into]
    out = [row[:] for row in cells]
    for y in range(h):
        for x in range(w):
            if cells[y][x] != b or _noise(x, y, 2.5, salt) < 0.45:
                continue
            if any(0 <= y + dy < h and 0 <= x + dx < w and cells[y + dy][x + dx] in water
                   for dy in (-1, 0, 1) for dx in (-1, 0, 1)):
                out[y][x] = t
    return out


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
    default = Path(__file__).resolve().parent.parent / "examples/packs/pepelnaya-marka/maps/brod/terrain.png"
    out = Path(sys.argv[1]) if len(sys.argv) > 1 else default
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_bytes(png(paint()))
    print(f"wrote {out} ({SIZE}×{SIZE}, {len(PALETTE)} terrain colours)")


if __name__ == "__main__":
    main()
