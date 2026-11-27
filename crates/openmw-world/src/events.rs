//! The scenario events for the Lua runtime (spec 3.11, 3.12; M5): the events, the event points and
//! the building zones, as Lua literals in `map_data.lua`, and the journal records (DIAL and INFO)
//! of the quest events (spec 3.11, "Quests").
//!
//! The events stay the exporter's JSON (`events[]`, `points[]`): every field is written with its
//! JSON name, so the Lua runtime reads what the exporter wrote, and nothing is compiled per event.

use crate::grid::Grid;
use crate::input::{Hero, Map};
use crate::lua::string;
use crate::tes3::{zstring, Record};
use crate::units::Units;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt::Write;

/// The placeholder cost of a unit without the install's numbers (the M4 formula's level-0 body:
/// `30 + 20 · level`, with level 1 for a level-0 unit).
const PLACEHOLDER_COST: i64 = 50;

/// The cost of every unit the map's armies and hero use (spec 3.8, events.lua army strength): the
/// install's `cost` (at least 1, as the game's hook keeps a tactical cost positive), else the placeholder.
pub fn unit_costs(map: &Map, units: Option<&Units>) -> BTreeMap<i64, i64> {
    let mut ids = Vec::new();
    for a in &map.armies {
        ids.push(a.leader_unit);
        ids.extend(a.troops.iter().map(|t| t.unit));
    }
    for h in &map.heroes {
        ids.extend(h.troops.iter().map(|t| t.unit));
    }
    ids.into_iter()
        .map(|id| {
            let cost = match units.and_then(|us| us.get(id)) {
                Some(u) if u.cost > 0 => u.cost,
                Some(_) => 1,
                None => PLACEHOLDER_COST,
            };
            (id, cost)
        })
        .collect()
}

/// Lua keywords: a JSON key with one of these names is written in brackets.
const LUA_KEYWORDS: [&str; 22] = [
    "and", "break", "do", "else", "elseif", "end", "false", "for", "function", "goto", "if", "in", "local", "nil",
    "not", "or", "repeat", "return", "then", "true", "until", "while",
];

/// The journal index of a quest's first entry (taking the quest) and of its finished entry
/// (spec 3.11: `addJournalEntry(10)` and `addJournalEntry(100)`).
pub const JOURNAL_TAKEN: i32 = 10;
pub const JOURNAL_FINISHED: i32 = 100;

/// A JSON value as a Lua literal. Keys are written as names, or in brackets when they are not names
/// or are Lua keywords (`repeat`). A `null` field is left out of its table. Strings go through
/// `lua::string`, so Cyrillic is UTF-8 in the file.
pub fn lua_value(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("nil"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&n.to_string()),
        Value::String(s) => out.push_str(&string(s)),
        Value::Array(items) => {
            out.push('{');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                lua_value(item, out);
            }
            out.push('}');
        }
        Value::Object(fields) => {
            out.push('{');
            let mut first = true;
            // serde_json without preserve_order keeps the keys sorted, so the text is the same on
            // every run.
            for (key, value) in fields {
                if value.is_null() {
                    continue;
                }
                if !first {
                    out.push_str(", ");
                }
                first = false;
                lua_key(key, out);
                out.push_str(" = ");
                lua_value(value, out);
            }
            out.push('}');
        }
    }
}

fn lua_key(key: &str, out: &mut String) {
    let is_name = !key.is_empty()
        && key.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if is_name && !LUA_KEYWORDS.contains(&key) {
        out.push_str(key);
    } else {
        let _ = write!(out, "[{}]", string(key));
    }
}

/// The hero class an event's `archetype` field tests (0 any, 1 knight, 2 archmage, 3 ranger, as
/// events.md §3 has it). 0 for an unknown archetype.
pub fn class_of(archetype: &str) -> i64 {
    match archetype {
        "knight" => 1,
        "archmage" => 2,
        "ranger" => 3,
        _ => 0,
    }
}

/// The building kinds that are bridges (not zones: a bridge is walked over, spec 3.7).
fn is_bridge(kind: i64) -> bool {
    kind == 13 || kind == 14
}

/// The Lua fragment of `map_data.lua` for the scenario: `info`, `hero`, `events`, `points`, `zones`
/// and `army_units`. Deterministic: lists keep the file's order.
pub fn fragment(map: &Map, hero: Option<&Hero>, costs: &BTreeMap<i64, i64>) -> String {
    let mut s = String::new();
    let start_time = map.info.start_time.unwrap_or(0);
    let _ = writeln!(
        s,
        "  info = {{ title = {}, start_time = {}, victory = {}, defeat = {}, next_map = {} }},",
        string(&map.info.title),
        start_time,
        map.info.victory_event,
        map.info.defeat_event,
        string(&map.info.next_map)
    );
    match hero {
        Some(h) => {
            let _ = write!(
                s,
                "  hero = {{ archetype = {}, class = {}, gold = {}, mana = {}, start_building = {}, party = {{",
                string(&h.archetype),
                class_of(&h.archetype),
                h.gold,
                h.mana,
                h.start_building
            );
            for (i, t) in h.troops.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                let _ = write!(s, "{{ unit = {}, level = {}, count = {}, named = 0 }}", t.unit, t.level, t.count);
            }
            let list = |ids: &[i64]| ids.iter().map(i64::to_string).collect::<Vec<_>>().join(", ");
            let _ = writeln!(
                s,
                "}}, artifacts = {{{}}}, spells = {{{}}} }},",
                list(&h.artifacts),
                list(&h.spells)
            );
        }
        None => s.push_str("  hero = { archetype = \"\", class = 0, gold = 0, mana = 0, start_building = 0, party = {}, artifacts = {}, spells = {} },\n"),
    }

    s.push_str("  costs = {");
    for (i, (unit, cost)) in costs.iter().enumerate() {
        if i > 0 {
            s.push_str(", ");
        }
        let _ = write!(s, "[{unit}] = {cost}");
    }
    s.push_str("},\n");
    s.push_str("  events = {\n");
    for ev in &map.events {
        s.push_str("    ");
        lua_value(ev, &mut s);
        s.push_str(",\n");
    }
    s.push_str("  },\n");

    s.push_str("  points = {\n");
    for p in &map.points {
        s.push_str("    ");
        lua_value(p, &mut s);
        s.push_str(",\n");
    }
    s.push_str("  },\n");

    // The building zones: the footprint tiles (DT's coordinates, y from the north) of every
    // building that is not a bridge, with its events (spec 3.7, Q6: stepping in fires them).
    s.push_str("  zones = {\n");
    for b in map.buildings.iter().filter(|b| !is_bridge(b.kind)) {
        let tiles = Grid::footprint_tiles(b.x, b.y, b.size_x, b.size_y);
        let mut t = String::new();
        for (i, (x, y)) in tiles.iter().enumerate() {
            if i > 0 {
                t.push_str(", ");
            }
            let _ = write!(t, "{{{}, {}}}", x, y);
        }
        let mut ev = String::new();
        for (i, id) in b.events.iter().enumerate() {
            if i > 0 {
                ev.push_str(", ");
            }
            let _ = write!(ev, "{}", id);
        }
        let _ = writeln!(
            s,
            "    {{ index = {}, kind = {}, faction = {}, tiles = {{{}}}, events = {{{}}} }},",
            b.index, b.kind, b.faction, t, ev
        );
    }
    s.push_str("  },\n");

    // The army units by army id, for the conditions that test a faction's units (codes 2-5), and the
    // leader's unit with each troop line's count, for the army-strength test (events.lua).
    s.push_str("  army_units = {\n");
    for a in &map.armies {
        let units: Vec<String> = a.troops.iter().map(|t| t.unit.to_string()).collect();
        let counts: Vec<String> = a.troops.iter().map(|t| t.count.to_string()).collect();
        let _ = writeln!(
            s,
            "    [{}] = {{ faction = {}, units = {{{}}}, counts = {{{}}}, leader = {} }},",
            a.id,
            a.faction,
            units.join(", "),
            counts.join(", "),
            a.leader_unit
        );
    }
    s.push_str("  },\n");
    s
}

/// A quest's name as the journal shows it: the title before its first `%` (a flag script) or `#`
/// (a window style or placeholder marker), trailing spaces removed (spec 10, `0x4833f0`).
pub fn display_title(title: &str) -> &str {
    title.split(['%', '#']).next().unwrap_or("").trim_end()
}

/// The text of a journal entry: the title, the question and the message, one paragraph each.
/// `#\` (a line break in the texts) becomes a newline.
fn journal_text(title: &str, question: &str, message: &str) -> String {
    let mut parts = vec![display_title(title).to_string()];
    for part in [question, message] {
        if !part.is_empty() {
            parts.push(part.to_string());
        }
    }
    parts.join("\n\n").replace("#\\", "\n")
}

/// One INFO record of a journal DIAL: the id, the previous and next ids (empty: none), the 12 bytes
/// of dialogue data, the text, and `QSTF` (the quest is finished) when `finished` is set. The journal
/// index is the second field of DATA (bytes 4..8): OpenMW 0.51 looks the entry up there (checked in
/// the game, 2026-11-27: `addJournalEntry(10)` and `(100)` find their entries). OpenMW's loader has no
/// `INDX` or `INTV` subrecord for it ("Unknown subrecord" in esmtool), and esmtool does not check it.
fn info_record(id: &str, index: i32, text: &str, finished: bool) -> Record {
    let mut data = vec![0u8; 12];
    data[4..8].copy_from_slice(&index.to_le_bytes());
    let mut info = Record::new(b"INFO")
        .sub(b"INAM", zstring(id))
        .sub(b"PNAM", zstring(""))
        .sub(b"NNAM", zstring(""))
        .sub(b"DATA", data)
        .sub(b"NAME", zstring(text));
    if finished {
        info = info.sub(b"QSTF", vec![1]);
    }
    info
}

/// The journal records of the map's quest events (kind 3): one DIAL of type journal (4), named
/// `rz_q<id>`, and two INFO records after it. Index 10 is the taken entry (title, question and
/// message); index 100 the finished one (`QSTF`, the title and "done").
pub fn journal_records(map: &Map) -> Vec<Record> {
    let mut out = Vec::new();
    for ev in &map.events {
        if ev.get("kind").and_then(Value::as_i64) != Some(3) {
            continue;
        }
        let id = ev.get("id").and_then(Value::as_i64).unwrap_or(0);
        let text = |k: &str| ev.get(k).and_then(Value::as_str).unwrap_or("").to_string();
        let title = text("title");
        let quest = format!("rz_q{id}");
        out.push(Record::new(b"DIAL").sub(b"NAME", zstring(&quest)).sub(b"DATA", vec![4]));
        let taken = journal_text(&title, &text("question"), &text("message"));
        out.push(info_record(&format!("{quest}_{JOURNAL_TAKEN}"), JOURNAL_TAKEN, &taken, false));
        let done = format!("{}\n\nВыполнено.", display_title(&title));
        out.push(info_record(&format!("{quest}_{JOURNAL_FINISHED}"), JOURNAL_FINISHED, &done, true));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::Map;
    use crate::tes3;

    fn scenario() -> Map {
        let text = r#"{
          "format": "razdor-dtm-json", "version": 1, "width": 4, "height": 4,
          "info": {"title": "Тест", "start_time": 1000, "victory_event": 3, "defeat_event": 4,
                   "next_map": "", "relations": [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]]},
          "heroes": [{"archetype": "knight", "gold": 100, "mana": 150, "start_building": 1, "x": 0, "y": 0,
                      "troops": [{"unit": 4, "level": 0, "count": 1}]}],
          "terrain": {"encoding": "hex-rows", "rows": ["5555", "5555", "5555", "5555"]},
          "buildings": [{"index": 1, "x": 2, "y": 2, "size_x": 2, "size_y": 2, "kind": 3, "faction": 1, "events": [5]},
                        {"index": 2, "x": 3, "y": 3, "size_x": 1, "size_y": 1, "kind": 13, "faction": 0, "events": []}],
          "armies": [{"id": 1, "x": 3, "y": 0, "faction": 4, "troops": [{"unit": 9, "level": 0, "count": 2}]}],
          "points": [{"id": 1, "x": 0, "y": 3, "events": [6]}],
          "events": [
            {"id": 1, "kind": 1, "kind_name": "global", "title": "Начало%+A", "question": "", "message": "Привет",
             "flags": {"raw": "+A", "set": "A", "clear": null, "require_set": null, "require_unset": null},
             "start_time": 1000, "repeat": 1440, "duration": 1440, "archetype": 0, "once": 1, "subordinate": 0,
             "conditions": {"gold": 0, "buildings": [0, 0, 0]}, "results": {"gold": 5}},
            {"id": 2, "kind": 3, "kind_name": "quest", "title": "Задание#VICTORY", "question": "Пойдёте?",
             "message": "Найдите \"ключ\"", "flags": null, "start_time": 0, "repeat": 0, "duration": 0,
             "archetype": 0, "once": 1, "subordinate": 0, "conditions": {}, "results": {}}
          ]
        }"#;
        Map::from_json(text).expect("the synthetic scenario parses")
    }

    #[test]
    fn the_fragment_has_the_events_with_their_json_names_and_utf8_text() {
        let map = scenario();
        let text = fragment(&map, map.heroes.first(), &unit_costs(&map, None));
        assert!(text.contains("[\"repeat\"] = 1440"), "{text}");
        assert!(text.contains("title = \"Начало%+A\""), "{text}");
        assert!(text.contains("flags = {raw = \"+A\", set = \"A\"}"), "{text}");
        assert!(!text.contains("clear"), "a null field is left out: {text}");
        assert!(text.contains("hero = { archetype = \"knight\", class = 1, gold = 100, mana = 150, start_building = 1"), "{text}");
        assert!(text.contains("party = {{ unit = 4, level = 0, count = 1, named = 0 }}"), "{text}");
        assert!(text.contains("zones = {"), "{text}");
        assert!(text.contains("{ index = 1, kind = 3, faction = 1, tiles = {{1, 1}, {2, 1}, {1, 2}, {2, 2}}, events = {5} },"), "{text}");
        assert!(!text.contains("index = 2"), "a bridge is not a zone: {text}");
        assert!(text.contains("points = {\n    {"), "{text}");
        assert!(text.contains("] = { faction = 4, units = {9}, counts = {") || text.contains("units = {9}, counts = {"), "{text}");
        // Without --units every unit of the map costs the placeholder; the table is written in the fragment.
        let costs = unit_costs(&map, None);
        assert!(!costs.is_empty() && costs.values().all(|&c| c == PLACEHOLDER_COST), "{costs:?}");
        assert!(text.contains("costs = {[") && text.contains("[9] = 50"), "{text}");
        assert!(text.contains("victory = 3, defeat = 4"), "{text}");
    }

    #[test]
    fn the_fragment_is_the_same_on_every_run() {
        let map = scenario();
        let costs = unit_costs(&map, None);
        assert_eq!(fragment(&map, map.heroes.first(), &costs), fragment(&map, map.heroes.first(), &costs));
    }

    #[test]
    fn a_keyword_key_is_written_in_brackets_and_a_plain_name_is_not() {
        let mut s = String::new();
        lua_key("repeat", &mut s);
        lua_key("duration", &mut s);
        lua_key("a b", &mut s);
        assert_eq!(s, "[\"repeat\"]duration[\"a b\"]");
    }

    #[test]
    fn the_display_title_stops_at_the_flag_script_and_the_style_marker() {
        assert_eq!(display_title("Начало%+A"), "Начало");
        assert_eq!(display_title("Задание#VICTORY"), "Задание");
        assert_eq!(display_title("Двое  "), "Двое");
    }

    #[test]
    fn the_quest_records_round_trip_and_the_text_is_cp1251() {
        let map = scenario();
        let records = journal_records(&map);
        assert_eq!(records.len(), 3, "one DIAL and two INFO for the one quest");
        let bytes = tes3::encode(&records);
        let back = tes3::decode(&bytes).expect("the records decode");
        assert_eq!(back.len(), 3);
        assert_eq!(back[0].name, *b"DIAL");
        assert_eq!(tes3::read_zstring(back[0].get(b"NAME").unwrap()), "rz_q2");
        assert_eq!(back[0].get(b"DATA").unwrap(), &[4u8][..], "the journal type");
        assert_eq!(tes3::read_zstring(back[1].get(b"INAM").unwrap()), "rz_q2_10");
        assert_eq!(&back[1].get(b"DATA").unwrap()[4..8], &10i32.to_le_bytes()[..], "the journal index");
        assert_eq!(tes3::read_zstring(back[1].get(b"NAME").unwrap()), "Задание\n\nПойдёте?\n\nНайдите \"ключ\"");
        assert_eq!(tes3::read_zstring(back[2].get(b"INAM").unwrap()), "rz_q2_100");
        assert_eq!(&back[2].get(b"DATA").unwrap()[4..8], &100i32.to_le_bytes()[..], "the journal index");
        assert_eq!(back[2].get(b"QSTF").unwrap(), &[1u8][..]);
        // cp1251: the Cyrillic bytes are one byte per letter, not UTF-8.
        let name = back[1].get(b"NAME").unwrap();
        assert!(!name.windows(2).any(|w| w == [0xD0, 0x97]), "not UTF-8");
    }
}
