#!/usr/bin/env python3
"""Orientation probe for the converter (spec 2.2, M4): synthetic input only, no game content.

  probe_orient.py json <out.json>          a 16x16 map, four surfaces by quarter (row 0 = north)
  probe_orient.py json-sub <out.json>      the same size; the four quarters of cell (0,0) differ
  probe_orient.py patch <plugin.omwaddon>  raises one vertex of cell (0,0) to 800 (row 1, column 2)

`patch` rewrites the VHGT of the converter's LAND (0,0) in place with the same encoding as the
crate's `tes3::encode_vhgt` (offset then deltas, int8, 3 padding bytes). The record size does not
change. Run it on the converter's output for the json above, not on any game file.
"""
import struct
import sys

FLAT = 96
RAISED = 800
VERTS = 65
STEP = 8


def probe_map():
    # North rows (0..7) are water, south rows (8..15) land; west columns 0..7, east 8..15.
    north = ("2" * 8 + "1" * 8)
    south = ("5" * 8 + "a" * 8)
    rows = [north] * 8 + [south] * 8
    return {
        "format": "razdor-dtm-json",
        "version": 1,
        "source_file": "synthetic-orientation-probe",
        "width": 16,
        "height": 16,
        "info": {"generator_seed": 0},
        "legend": {"surfaces": [
            {"code": 1, "name": "coastal_water"},
            {"code": 2, "name": "deep_sea"},
            {"code": 5, "name": "grass_lowland"},
            {"code": 10, "name": "sand_dunes"},
        ]},
        "terrain": {"encoding": "hex-rows", "rows": rows},
    }


def sub_map():
    """Texel-level probe: the four quarters of cell (0, 0) differ (the cell is tiles 0..7 from the
    south-west corner). Sand south-west, green south-east, lava north-west, road north-east; the
    rest of the map is deep sea."""
    rows = []
    for jrow in range(16):          # JSON row 0 is the north
        from_south = 15 - jrow
        line = ""
        for x in range(16):
            if from_south < 8 and x < 8:
                south = from_south < 4
                west = x < 4
                line += {(True, True): "a", (True, False): "5",
                         (False, True): "3", (False, False): "4"}[(south, west)]
            else:
                line += "2"
        rows.append(line)
    m = probe_map()
    m["terrain"]["rows"] = rows
    m["legend"]["surfaces"] = [{"code": c, "name": n} for c, n in
                               [(2, "deep_sea"), (3, "lava_fields"), (4, "road"),
                                (5, "grass_lowland"), (10, "sand_dunes")]]
    return m


def encode_vhgt(heights):
    q = [(h + 4) // STEP for h in heights]
    deltas = [0] * (VERTS * VERTS)
    for row in range(VERTS):
        for col in range(VERTS):
            at = row * VERTS + col
            if col > 0:
                d = q[at] - q[at - 1]
            elif row > 0:
                d = q[at] - q[at - VERTS]
            else:
                d = 0
            if not -128 <= d <= 127:
                raise SystemExit(f"height step too large at row {row} col {col}")
            deltas[at] = d
    out = struct.pack("<f", float(q[0]))
    out += bytes(d & 0xFF for d in deltas)
    out += b"\0\0\0"
    return out


def records(data):
    """Yields (name, start, size, subs) with subs = [(name, data_start, data_size)]."""
    pos = 0
    while pos < len(data):
        name = data[pos:pos + 4]
        size = struct.unpack_from("<I", data, pos + 4)[0]
        body = pos + 16
        subs = []
        p = body
        while p < body + size:
            sname = data[p:p + 4]
            ssize = struct.unpack_from("<I", data, p + 4)[0]
            subs.append((sname, p + 8, ssize))
            p += 8 + ssize
        yield name, body, size, subs
        pos = body + size


def patch(path):
    with open(path, "rb") as f:
        data = bytearray(f.read())
    done = False
    for name, _, _, subs in records(bytes(data)):
        if name != b"LAND":
            continue
        intv = next(s for s in subs if s[0] == b"INTV")
        cx, cy = struct.unpack_from("<ii", data, intv[1])
        if (cx, cy) != (0, 0):
            continue
        vhgt = next(s for s in subs if s[0] == b"VHGT")
        heights = [FLAT] * (VERTS * VERTS)
        heights[1 * VERTS + 2] = RAISED
        blob = encode_vhgt(heights)
        if len(blob) != vhgt[2]:
            raise SystemExit(f"VHGT size {vhgt[2]} != {len(blob)}")
        data[vhgt[1]:vhgt[1] + vhgt[2]] = blob
        done = True
    if not done:
        raise SystemExit("no LAND for cell (0, 0)")
    with open(path, "wb") as f:
        f.write(data)
    print(f"patched {path}: vertex row 1, column 2 of cell (0, 0) = {RAISED}")


def main(argv):
    import json
    if len(argv) == 3 and argv[1] in ("json", "json-sub"):
        with open(argv[2], "w", encoding="utf-8") as f:
            json.dump(probe_map() if argv[1] == "json" else sub_map(), f, indent=1)
        return 0
    if len(argv) == 3 and argv[1] == "patch":
        patch(argv[2])
        return 0
    print(__doc__)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
