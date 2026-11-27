#!/usr/bin/env python3
"""The M8 synthetic fight: a 12 x 12 grass map with no events, a hero far off, and two hostile armies
of four bodies each (a melee leader, a melee, a shooter and a caster), 3 tiles apart (1536 units at
--tile-units 512). Both armies are of faction 4, hostile to the player. Also writes a synthetic
units version 2 file with made-up names and numbers (no game data). Synthetic input only.

  make_m8_map.py <out map.json> <out units.json>
"""
import json
import sys

W = H = 12
RELATIONS = [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]]


def army(aid, x, y):
    return {
        "id": aid, "x": x, "y": y, "name": "Army %d" % aid, "leader_name": "Leader %d" % aid,
        "leader_unit": 1, "leader_level": 0,
        # Leader (unit 1) + 3 troops = four bodies: a melee, a shooter and a caster.
        "troops": [{"unit": 1, "level": 0, "count": 1}, {"unit": 2, "level": 0, "count": 1},
                   {"unit": 3, "level": 0, "count": 1}],
        "faction": 4, "relations": [-2, -2, 1, 3], "inactive": 0, "patrols": 0, "patrol_radius": 0,
        "behaviour": 0, "home_building": 0, "respawn_days": 0, "ship": 0,
    }


def build():
    return {
        "format": "razdor-dtm-json",
        "version": 1,
        "source_file": "synthetic-m8",
        "width": W,
        "height": H,
        "info": {
            "title": "Тест M8", "description": "", "campaign_name": "", "next_map": "",
            "scenario_kind": 1, "start_time": 1000, "victory_event": 0, "defeat_event": 0,
            "relations": RELATIONS, "generator_seed": 0,
        },
        "heroes": [{
            "archetype": "knight", "gold": 0, "mana": 0, "start_building": 0, "x": 1, "y": 10,
            "troops": [], "artifacts": [], "spells": [],
        }],
        "legend": {"surfaces": [{"code": 5, "name": "grass_lowland"}]},
        "terrain": {"encoding": "hex-rows", "rows": ["5" * W for _ in range(H)]},
        "objects": [],
        "buildings": [],
        "armies": [army(1, 2, 6), army(2, 5, 6)],
        "points": [],
        "events": [],
        "named_characters": [],
        "has_scenario_picture": False,
    }


def unit(uid, name, blow, shot, power=0, school=None, direction=None):
    return {
        "id": uid, "name": name, "cost": 10, "hit_points": 80,
        "attack": {"blow": blow, "shot": shot}, "defence": {"blow": 8, "shot": 4},
        "magic_power": power, "magic": school, "magic_direction": direction,
        "protection": {"life": 10, "death": 10, "elemental": 10}, "initiative": 10,
        "actions": 1, "regeneration": 0, "vampirism": 0, "ranged": shot > 0, "flying": False,
        "nature": "normal", "bonus": None, "evasion": None, "min_magic_power": None,
        "mana_drain": None, "experience": {"start": 60, "growth_percent": 140},
        "level_up": {"hit_points": 6},
    }


def units():
    return {
        "format": "razdor-units-json", "version": 2, "language": "en", "count": 3,
        "units": [
            unit(1, "Test Melee", 12, 0),
            unit(2, "Test Shooter", 0, 8),
            unit(3, "Test Caster", 0, 0, power=20, school="elemental", direction="to_enemy"),
        ],
        "spell_count": 0, "spells": [],
    }


def main(argv):
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[1], "w", encoding="utf-8") as f:
        json.dump(build(), f, ensure_ascii=False, indent=1)
    with open(argv[2], "w", encoding="utf-8") as f:
        json.dump(units(), f, ensure_ascii=False, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
