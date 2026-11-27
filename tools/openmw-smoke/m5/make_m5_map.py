#!/usr/bin/env python3
"""The M5 synthetic scenario: a 16x16 grass map with events, a building zone, a point, two armies
and the victory and defeat events. Synthetic input only (no game content), written for the
converter (`razdor-openmw <out.json> -o <dir>`) and the driver in `scripts/razdor_m5/` (player.lua
walks the same tiles and expects the event order that `m5/EXPECTED.txt` lists).

  make_m5_map.py <out.json>
"""
import copy
import json
import sys

W = H = 16
COND = {
    "squad_count": 0, "army_strength": 0, "army_inactive": 0, "stats_check": 0, "level": 0, "gold": 0,
    "holiness_mana": 0, "buildings_check": 0, "buildings": [0, 0, 0], "buildings_owner": [0, 0, 0],
    "units_check": 0, "units": [0, 0, 0], "units_named": [0, 0, 0], "units_owner": [0, 0, 0],
    "artifacts_check": 0, "artifacts": [0, 0, 0], "artifacts_owner": [0, 0, 0], "defeated_check": 0,
    "defeated_armies": [0, 0], "happened_yes_check": 0, "happened_yes": [0, 0], "not_happened_check": 0,
    "not_happened": [0, 0], "beaten_check": 0, "beaten_armies": [0, 0], "happened_no_check": 0,
    "happened_no": [0, 0], "meet_army": 0, "army_active": 0, "confirm_question": 0, "army_at_home": 0,
}
RES = {
    "patrol_army": 0, "patrol_delta": 0, "relative_event": 0, "relative_delay_hours": 0, "cast_spell": 0,
    "picture": 0, "experience": 0, "gold": 0, "mana": 0, "spells_learned": [0, 0, 0, 0],
    "units_add": [0, 0, 0, 0], "units_add_named": [0, 0, 0, 0], "units_remove": [0, 0, 0, 0],
    "units_remove_named": [0, 0, 0, 0], "artifacts_add": [0, 0, 0, 0], "artifacts_remove": [0, 0, 0, 0],
    "activate_armies": [0, 0], "deactivate_army": 0, "completes_quest": 0, "delay_hours": 0,
    "light_lanterns": [0, 0, 0, 0], "removed_units_to_army": 0, "new_hero_class": 0, "chained_event": 0,
    "units_from_army": 0, "move_to_hero": 0, "show_army": 0, "hero_one_hp": 0, "start_battle_with": 0,
    "no_meeting": 0, "repeat_after_yes": 0,
}
KIND = {1: "global", 2: "local", 3: "quest", 4: "rumour"}


def event(eid, kind, title, question="", message="", flags=None, once=1, start=1000, repeat=0, duration=0,
          cond=None, res=None, subordinate=0):
    c = copy.deepcopy(COND)
    r = copy.deepcopy(RES)
    c.update(cond or {})
    r.update(res or {})
    return {
        "id": eid, "kind": kind, "kind_name": KIND[kind], "title": title, "question": question,
        "message": message, "flags": flags, "start_time": start, "repeat": repeat, "duration": duration,
        "archetype": 0, "once": once, "subordinate": subordinate, "generate_battle_army": 0,
        "has_custom_picture": False, "conditions": c, "results": r,
    }


def flag(raw, set_=None, require_set=None, require_unset=None):
    return {"raw": raw, "set": set_, "clear": None, "require_set": require_set, "require_unset": require_unset}


def scenario():
    events = [
        # 1: a global message at the start.
        event(1, 1, "Начало", message="Добро пожаловать, рыцарь."),
        # 2: a global question (Yes): sets flag A, gold, mana, activates army 2, a relative event 3 after
        # one hour, and the message after Yes.
        event(2, 1, "Вопрос%+A", question="Вы пойдёте к башне?", message="Вы решились.", flags=flag("+A", set_="A"),
              cond={"confirm_question": 1},
              res={"gold": 50, "mana": 5, "activate_armies": [2, 0], "relative_event": 3, "relative_delay_hours": 1}),
        # 3: relative to event 2 (its start is far off until then): a message, then the chain 4.
        event(3, 1, "Через час", message="Прошёл час.", start=999999999, res={"chained_event": 4}),
        # 4: a silent local chain of 3 (no question, no message): gold and flag C.
        event(4, 2, "Тихо%+C", flags=flag("+C", set_="C"), res={"gold": 10}),
        # 5: a global question (No): nothing else; the No counts as a firing.
        event(5, 1, "Пропустить", question="Пропустить встречу?", message="Пропущено.", cond={"confirm_question": 1}),
        # 6: after No to 5 (happened with No).
        event(6, 1, "Отказ", message="Вы отказались от встречи.", cond={"happened_no_check": 1, "happened_no": [5, 0]}),
        # 7: flag A is set (tested).
        event(7, 1, "Флаг", message="Флаг A установлен.", flags=flag("", require_set="A")),
        # 8: army 2 is beaten; a chain to the quest 10.
        event(8, 1, "Разбит", message="Армия разбита.", cond={"beaten_check": 1, "beaten_armies": [2, 0]},
              res={"chained_event": 10}),
        # 9: the victory event's neighbour is the defeat event; 14 and 15 are the end events.
        event(9, 2, "Резерв", message="Резерв."),
        # 10: a quest (journal) taken by a chain.
        event(10, 3, "Поиск ключа", message="Найдите ключ в башне."),
        # 11: a silent global: the quest 10 has fired, so it completes the quest and chains the victory.
        event(11, 1, "Ключ", cond={"happened_yes_check": 1, "happened_yes": [10, 0]},
              res={"completes_quest": 10, "chained_event": 14}),
        # 12: the building zone: stepping into building 1 offers it.
        event(12, 2, "В замке", message="Вы вошли в замок."),
        # 13: the point at (12, 12).
        event(13, 2, "Точка", message="Вы нашли знак."),
        # 14: victory (a local event, so only the chain of 11 opens it).
        event(14, 2, "Победа#VICTORY", message="Башня взята."),
        # 15: defeat (never fires here).
        event(15, 2, "Поражение#DEFEAT", message="Отряд разбит."),
        # 16: meeting army 1: the leader within a tile of the player.
        event(16, 1, "Встреча", message="Встреча с отрядом.", cond={"meet_army": 1}),
        # 17-26: the tracked results, each one after the one before (a chain of global messages). 17 finds
        # artifact 7, 18 checks that the hero has it, 20 learns spell 3, 21 patrol army 1 by +2, 22 shows
        # army 1, 23 moves army 1 next to the hero, 24 changes the hero's class to unit 3, 25 starts the
        # battle with army 2 (hostile, next to the hero), 26 loses artifact 7, and 19 checks that it is gone.
        event(17, 1, "Находка", message="Вы нашли артефакт.",
              cond={"happened_yes_check": 1, "happened_yes": [7, 0]}, res={"artifacts_add": [7, 0, 0, 0]}),
        event(18, 1, "Артефакт", message="Артефакт у вас.",
              cond={"happened_yes_check": 1, "happened_yes": [17, 0], "artifacts_check": 1,
                    "artifacts": [7, 0, 0], "artifacts_owner": [1, 0, 0]}),
        event(19, 1, "Утрата", message="Артефакта больше нет.",
              cond={"happened_yes_check": 1, "happened_yes": [26, 0], "artifacts_check": 1,
                    "artifacts": [7, 0, 0], "artifacts_owner": [6, 0, 0]}),
        event(20, 1, "Заклинание", message="Заклинание изучено.",
              cond={"happened_yes_check": 1, "happened_yes": [18, 0]}, res={"spells_learned": [3, 0, 0, 0]}),
        event(21, 1, "Дозор", message="Дозор расширен.",
              cond={"happened_yes_check": 1, "happened_yes": [20, 0]}, res={"patrol_army": 1, "patrol_delta": 2}),
        event(22, 1, "Смотр", message="Отряд на виду.",
              cond={"happened_yes_check": 1, "happened_yes": [21, 0]}, res={"show_army": 1}),
        event(23, 1, "К герою", message="Отряд подошёл.",
              cond={"happened_yes_check": 1, "happened_yes": [22, 0]}, res={"move_to_hero": 1}),
        event(24, 1, "Класс", message="Новый класс.",
              cond={"happened_yes_check": 1, "happened_yes": [23, 0]}, res={"new_hero_class": 3}),
        event(25, 1, "Бой", message="Бой начинается.",
              cond={"happened_yes_check": 1, "happened_yes": [24, 0]}, res={"start_battle_with": 2}),
        event(26, 1, "Потеря", message="Артефакт потерян.",
              cond={"happened_yes_check": 1, "happened_yes": [25, 0]}, res={"artifacts_remove": [7, 0, 0, 0]}),
    ]
    return events


def build():
    rows = ["5" * W for _ in range(H)]
    events = scenario()
    return {
        "format": "razdor-dtm-json",
        "version": 1,
        "source_file": "synthetic-m5",
        "width": W,
        "height": H,
        "info": {
            "title": "Тест M5", "description": "", "campaign_name": "", "next_map": "",
            "scenario_kind": 1, "start_time": 1000, "victory_event": 14, "defeat_event": 15,
            "relations": [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]], "generator_seed": 0,
        },
        "heroes": [{
            "archetype": "knight", "gold": 100, "mana": 150, "start_building": 1, "x": 2, "y": 13,
            "troops": [{"unit": 4, "level": 0, "count": 1}], "artifacts": [], "spells": [],
        }],
        "legend": {"surfaces": [{"code": 5, "name": "grass_lowland"}]},
        "terrain": {"encoding": "hex-rows", "rows": rows},
        "objects": [],
        "buildings": [{
            "index": 1, "x": 6, "y": 8, "size_x": 2, "size_y": 2, "kind": 3, "kind_name": "castle",
            "faction": 1, "owner_army": 255, "events": [12], "start_for": [1],
        }],
        "armies": [
            {"id": 1, "x": 10, "y": 5, "name": "Отряд", "leader_name": "Лидер", "leader_unit": 13,
             "leader_level": 0, "troops": [{"unit": 9, "level": 0, "count": 2}], "faction": 4,
             "relations": [-2, -2, 1, 3], "inactive": 0, "patrols": 0, "patrol_radius": 0, "behaviour": 0,
             "home_building": 0, "respawn_days": 0, "ship": 0},
            {"id": 2, "x": 12, "y": 3, "name": "Засада", "leader_name": "Лидер 2", "leader_unit": 13,
             "leader_level": 0, "troops": [{"unit": 9, "level": 0, "count": 1}], "faction": 4,
             "relations": [-2, -2, 1, 3], "inactive": 1, "patrols": 0, "patrol_radius": 0, "behaviour": 0,
             "home_building": 0, "respawn_days": 0, "ship": 0},
        ],
        "points": [{"id": 1, "x": 12, "y": 12, "model": 9, "radius": 0, "active": 0, "events": [13]}],
        "events": events,
        "named_characters": [],
        "has_scenario_picture": False,
    }


def main(argv):
    if len(argv) != 2:
        print(__doc__, file=sys.stderr)
        return 2
    with open(argv[1], "w", encoding="utf-8") as f:
        json.dump(build(), f, ensure_ascii=False, indent=1)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
