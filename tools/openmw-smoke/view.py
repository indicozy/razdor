#!/usr/bin/env python3
"""Default camera for the smoke picture of a converted map (tools/openmw-smoke/play.sh).

    python3 -I view.py <map.json>

Prints `x,y,z,pitch,yaw` for run.sh --camera. The camera stands two tiles north-west of the
hero's start tile (the start tile is also where the player is, so the land around it loads) and
looks at the map's first building (or four tiles east of the start when there is none). The
height is a fixed 1800 units above the origin, and the pitch is 0.3 rad down.

Tile (tx, ty) is at world (512 + 1024 tx, (H - 1 - ty) 1024 + 512), H the map height: row 0 of
the terrain is the map's north edge and world +y is north. This is the converter's mapping (see
docs/specs/2026-11-27-openmw-world.md, section 3.2).
"""
import json
import math
import sys


def world(tx, ty, height):
    return 512 + tx * 1024, (height - 1 - ty) * 1024 + 512


def main():
    with open(sys.argv[1], encoding='utf-8') as f:
        data = json.load(f)
    height = int(data['height'])
    heroes = data.get('heroes') or []
    if heroes:
        sx, sy = int(heroes[0]['x']), int(heroes[0]['y'])
    else:
        sx, sy = height // 2, height // 2
    buildings = data.get('buildings') or []
    if buildings:
        tx, ty = int(buildings[0]['x']), int(buildings[0]['y'])
    else:
        tx, ty = sx + 4, sy
    cam_x, cam_y = world(sx - 2, sy - 2, height)
    look_x, look_y = world(tx, ty, height)
    fx, fy = look_x - cam_x, look_y - cam_y
    n = math.hypot(fx, fy) or 1.0
    fx, fy = fx / n, fy / n
    # OpenMW forward for yaw is (-sin yaw, cos yaw), as the smoke pictures confirm.
    yaw = math.atan2(-fx, fy)
    print('%d,%d,%d,%.2f,%.3f' % (cam_x, cam_y, 1800, 0.30, yaw))


if __name__ == '__main__':
    main()
