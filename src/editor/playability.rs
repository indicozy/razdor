//! The original editor's playability score (DTMapEdit 0x5a0724,
//! docs/reference/editor/mapcheck-files.md §6): points for the map's content, penalties for
//! monotonous terrain, scattered buildings, uneven rewards and design slips, mapped through a
//! curve to the score stored at header 0x122, with the quest count at 0x126, and one line
//! appended to `MapData.Txt`.

use crate::dt::dtm::{CustomArtefact, Event, Scenario};
use crate::dt::text;

use super::grid::{impassable, terrain_or_zero, Marks};
use super::palette::{ArtefactFact, Names};

/// The three square rings around a cell: the 8 cells at distance 1, the 16 at 2, the 24 at
/// 3 (offset tables 0x5becdc, 0x5bed10; only the sets matter, as the counts are sums).
fn rings() -> [Vec<(i64, i64)>; 3] {
    std::array::from_fn(|k| {
        let r = k as i64 + 1;
        let mut v = Vec::new();
        for dx in -r..=r {
            for dy in -r..=r {
                if dx.abs().max(dy.abs()) == r {
                    v.push((dx, dy));
                }
            }
        }
        v
    })
}

/// Delphi's `Round`: halves to even.
fn round(x: f64) -> i64 {
    crate::rules::experience::round_half_even(x)
}

/// The words of a text for the near-duplicate test (0x5303e8): `,./?!:"` and line breaks
/// become spaces, runs of spaces one, the ends trimmed; an empty result is one empty word.
pub fn words(s: &str) -> Vec<String> {
    let mut t: String = s.chars().map(|c| if ",./?!:\"\n\r".contains(c) { ' ' } else { c }).collect();
    while t.contains("  ") {
        t = t.replace("  ", " ");
    }
    let t = t.trim_matches(|c: char| c <= ' ');
    t.split(' ').map(str::to_string).collect()
}

/// The word-level edit distance of two texts (0x530554).
pub fn word_distance(a: &str, b: &str) -> i64 {
    let (a, b) = (words(a), words(b));
    let mut prev: Vec<i64> = (0..=b.len() as i64).collect();
    for (i, wa) in a.iter().enumerate() {
        let mut row = vec![i as i64 + 1];
        for (j, wb) in b.iter().enumerate() {
            let cost = (wa != wb) as i64;
            row.push((prev[j + 1] + 1).min(row[j] + 1).min(prev[j] + cost));
        }
        prev = row;
    }
    prev[b.len()]
}

/// The terrain counters of §6.3, in the report's order: T1–T3, L1–L3, W1–W3.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Variety {
    pub terrain: [u16; 3],
    pub land: [u16; 3],
    pub water: [u16; 3],
}

/// The cells at least 3 cells from each edge that differ from fewer than 2, 3 and 4 cells
/// of their first one, two and three rings. The loops take the column range from the height
/// and the row range from the width (only a map that is not square notices); cells off the
/// map read as 0. The counters are 16-bit, as in the original.
pub fn variety(s: &Scenario, marks: &Marks) -> Variety {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let rings = rings();
    let mut v = Variety::default();
    let thresholds = [2, 3, 4];
    for x in 3..h - 3 {
        for y in 3..w - 3 {
            let centre = marks.at(x, y);
            let land = !(0..=2).contains(&centre);
            let code = terrain_or_zero(s, x, y);
            let (mut by_mark, mut by_code) = (0, 0);
            for (k, ring) in rings.iter().enumerate() {
                by_mark += ring.iter().filter(|(dx, dy)| marks.at(x + dx, y + dy) != centre).count();
                by_code += ring.iter().filter(|(dx, dy)| terrain_or_zero(s, x + dx, y + dy) != code).count();
                if by_mark < thresholds[k] {
                    let c = if land { &mut v.land[k] } else { &mut v.water[k] };
                    *c = c.wrapping_add(1);
                }
                if by_code < thresholds[k] {
                    v.terrain[k] = v.terrain[k].wrapping_add(1);
                }
            }
        }
    }
    v
}

/// One scoring.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    /// Points after the content (§6.2), after the terrain (§6.3), after the spacing (§6.4)
    /// and before the curve (§6.6).
    pub stages: [i64; 4],
    pub variety: Variety,
    /// The score (header 0x122).
    pub score: u16,
    /// The quests (header 0x126).
    pub quests: u8,
    /// The line for `MapData.Txt`, without its line end.
    pub line: String,
}

/// Why no score was made: where the original would stop with an error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreError {
    /// A map narrower than 50 cells: the original divides by zero.
    TooNarrow,
    /// The score or the quest count does not fit its header field: a range error.
    OutOfRange,
}

/// An artefact by id: the install's list, then the map's custom artefacts after it.
fn artefact(names: &Names, custom: &[CustomArtefact], id: u32) -> ArtefactFact {
    let base = names.facts.artefacts.iter().map(|a| a.id).max().unwrap_or(0);
    if id > base {
        if let Some(c) = custom.get((id - base - 1) as usize) {
            let cost = i32::from_le_bytes(c.record[0x20..0x24].try_into().expect("230-byte record"));
            return ArtefactFact { id, cost, kind: c.record[0x24] };
        }
    }
    names.facts.artefact(id)
}

/// Building points by type (jump table 0x5a0945).
fn building_points(kind: u8) -> i64 {
    match kind {
        1 | 3 | 9 => 4,
        2 | 4 | 6 | 7 | 10 => 3,
        5 | 11 => 2,
        8 | 15 => 1,
        12 => 5,
        _ => 0,
    }
}

/// The value of an event (§6.2) and the rewards it adds to the statistic.
fn event_value(names: &Names, custom: &[CustomArtefact], e: &Event, quests: &mut i64, rewards: &mut Vec<i64>) -> i64 {
    let c = &e.conditions;
    let r = &e.results;
    let mut v = match e.kind {
        2 => 15,
        3 => {
            *quests += 1;
            40
        }
        4 => 5,
        _ => 0,
    };
    if c.meet_army != 0 {
        v += 10;
    }
    for (check, ids) in [(c.happened_yes_check, c.happened_yes), (c.happened_no_check, c.happened_no), (c.not_happened_check, c.not_happened)] {
        if check != 0 && ids[0] != 0 {
            v += if ids[1] == 0 { 10 } else { 15 };
        }
    }
    // A flag test (`=X` or `=/X`) in the title's script.
    let test = e.title.split_once('%').and_then(|(_, script)| script.split_once('=')).map_or("", |(_, t)| t);
    if !test.is_empty() {
        v += 15;
    }
    if c.confirm_question == 1 {
        v += 20;
    }
    for (on, points) in [(c.artifacts_check, 10), (c.buildings_check, 5), (c.defeated_check, 10), (c.units_check, 5)] {
        if on != 0 {
            v += points;
        }
    }
    if r.relative_event != 0 {
        v += 15;
    }
    if r.activate_armies[0] != 0 {
        v += 5;
    }
    if r.cast_spell != 0 {
        v += 10;
    }
    let mut gained = 0;
    for &item in r.artifacts_add.iter().filter(|i| **i != 0) {
        let a = artefact(names, custom, item as u32);
        let cost = if a.cost < 0 { -a.cost / 500 } else { a.cost } as i64;
        let value = (cost / if a.kind < 8 { 250 } else { 1500 }).min(15);
        v += value;
        rewards.push(value);
        gained += 1;
    }
    if gained > 0 {
        v += 2;
    }
    if r.gold > 0 {
        v += 5;
        rewards.push(r.gold as i64 / 2000 + 1);
    } else if r.gold < 0 {
        v += 2;
    }
    v
}

/// Scores `s` as the score button does. `custom` are the map's custom artefacts (still in
/// the editor's artefact list until a save).
pub fn score(s: &Scenario, names: &Names, custom: &[CustomArtefact]) -> Result<Score, ScoreError> {
    let w = s.width() as i64;
    let h = s.height() as i64;
    if w / 50 == 0 {
        return Err(ScoreError::TooNarrow);
    }
    let facts = &names.facts;
    let marks = Marks::build(s);
    let mut points: i64 = 0;
    for a in &s.armies {
        points += a.unknown_80 as i64 / 50;
        for &item in a.artifacts.iter().filter(|i| **i != 0) {
            let f = artefact(names, custom, item as u32);
            let cost = if f.cost < 0 { -f.cost / 1000 } else { f.cost } as i64;
            points += (cost / if f.kind < 8 { 500 } else { 2000 }).min(10);
        }
    }
    for b in &s.buildings {
        points += building_points(b.kind);
        if b.random_artifacts_for_sale != 0 {
            points += 2 + b.price_max as i64 / 1500;
        }
    }
    let (mut quests, mut weak) = (0i64, 0i64);
    let mut rewards = Vec::new();
    for (i, e) in s.events.iter().enumerate() {
        let id = (i + 1) as u16;
        let mut v = event_value(names, custom, e, &mut quests, &mut rewards);
        // Near duplicates: the other events whose messages and questions (where both have
        // them) are within one word. Exact copies with a message only give −1 + 0 and do not
        // count.
        let dups = s
            .events
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && {
                let mut d = -1;
                if !e.message.is_empty() && !o.message.is_empty() {
                    d += word_distance(&o.message, &e.message);
                }
                if !e.question.is_empty() && !o.question.is_empty() {
                    d += word_distance(&o.question, &e.question);
                }
                d != -1 && d < 2
            })
            .count() as i64;
        if dups > 0 {
            v /= dups;
        }
        // Text length.
        if !e.title.starts_with('-') {
            let len = text::encode(&e.question).len() + text::encode(&e.message).len();
            let t = round((len as f64).sqrt()) - 9;
            if t <= 0 && v <= 10 && s.header.victory_event != id && s.header.defeat_event != id {
                v = if v >= 1 { t } else { -10 };
                weak += 1;
            } else {
                v += t / 2;
            }
        }
        points += v;
    }
    points += 5 * s.armies.len() as i64 + 5 * (s.events.len() as i64 - weak) + s.points.len() as i64;
    // The reward statistic, kept in units of 1/10000 as the original's integers.
    let n = rewards.len() as f64;
    let (mean, spread) = if rewards.is_empty() {
        (0, 0)
    } else {
        let m = rewards.iter().sum::<i64>() as f64 / n;
        let sq = rewards.iter().map(|v| (v * v) as f64).sum::<f64>() / n;
        (round(m * 10000.0), round((sq - m * m).max(0.0).sqrt() * 10000.0))
    };
    let reward_sum = rewards.iter().sum::<i64>() * 10000;
    match s.header.scenario_kind {
        1 => points += 100,
        2 => points += 200,
        _ => {}
    }
    let s1 = points;

    // Terrain variety.
    let var = variety(s, &marks);
    let area = ((w - 6) * (w - 6)) as f64;
    let group = |c: [u16; 3]| c.iter().map(|v| format!("{:>5}", format!("{:.1}", *v as f64 * 100.0 / area).replace('.', ","))).collect::<String>();
    let report = [group(var.terrain), group(var.land), group(var.water)];
    let scale = (w / 50) * (w / 50);
    let l = var.land.map(|v| v as i64 / scale);
    let wt = var.water.map(|v| v as i64 / scale);
    for (count, above, loss) in [(l[0], 400, 10), (l[1], 100, 3), (l[2], 50, 1), (wt[0], 800, 20), (wt[1], 400, 6), (wt[2], 200, 2)] {
        if count > above {
            points -= (count - above) / loss;
        }
    }
    let s2 = points;

    // Building spacing, between footprint centres.
    let spaced = |k: u8| matches!(k, 1..=5 | 7..=12);
    let centre = |b: &crate::dt::dtm::Building| (b.x as i64 + (b.size_x / 2) as i64, b.y as i64 + (b.size_y / 2) as i64);
    let (mut sum, mut count) = (0i64, 0i64);
    for (i, b) in s.buildings.iter().enumerate().filter(|(_, b)| spaced(b.kind)) {
        let (x, y) = centre(b);
        let nearest = s
            .buildings
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && spaced(o.kind))
            .map(|(_, o)| {
                let (ox, oy) = centre(o);
                let (dx, dy) = ((ox - x).abs(), (oy - y).abs());
                (2 * dx.max(dy) + dx.min(dy)) / 2
            })
            .fold(w + h, i64::min);
        sum += nearest;
        count += 1;
    }
    let average = if count < 1 { w } else { sum / count };
    if average > 10 {
        points -= (w / 50) * (average - 10) * (average - 10);
    }
    let s3 = points;

    // Reward spread.
    if mean + spread > 50000 {
        points -= round((mean + spread - 50000) as f64 * reward_sum as f64 / 1e8);
    }
    // Buildings.
    for b in &s.buildings {
        match b.kind {
            3 if b.gold_per_day == 0 => points -= 10,
            2 if b.gold_per_day == 0 => points -= 5,
            12 if b.garrison[0].unit == 0 && b.owner_army == 0 => points -= 3,
            1 | 6 | 7 if b.random_artifacts_for_sale == 0 => points -= 5,
            _ => {}
        }
        if b.start_for.iter().any(|v| *v != 0) && b.gold_per_day > 150 {
            let over = b.gold_per_day as i64 - 150;
            points -= over;
            for slot in b.barracks.iter().filter(|t| t.unit != 0) {
                if facts.unit_cost(slot.unit as u32) > 150 {
                    points -= over / 2;
                }
            }
        }
    }
    // Armies.
    for a in &s.armies {
        if impassable(s, &marks, a.x as i64, a.y as i64) {
            points -= 50;
        }
        if a.leader_unit == 0 {
            points -= 15;
        }
    }
    // Quests nothing completes.
    for (i, e) in s.events.iter().enumerate() {
        if e.kind == 3 && !s.events.iter().any(|o| o.results.completes_quest == (i + 1) as u16) {
            points -= 50;
        }
    }
    // Points: global events attached, impassable cells, and the chains from their events.
    if !s.points.is_empty() {
        let (mut loops, mut deadly) = (0i64, 0i64);
        let event = |id: u16| (id as usize).checked_sub(1).and_then(|i| s.events.get(i));
        for p in &s.points {
            if p.event_count == 0 {
                continue;
            }
            // The original's list has five slots; a longer count stops it with a range
            // error, so the walk ends at the fifth.
            let slots = &p.event_slots[..(p.event_count as usize).min(5)];
            for &id in slots {
                if event(id).is_some_and(|e| e.kind == 1) {
                    points -= 25;
                }
            }
            if impassable(s, &marks, p.x as i64, p.y as i64) {
                points -= 25 * p.event_count as i64;
            }
            for &start in slots {
                // The visited ids as text, `~` after each; an id found anywhere in it, even
                // as part of a longer id, ends the walk as a loop.
                let (mut id, mut walked, mut looped) = (start, String::new(), false);
                while !looped {
                    let next = event(id).map_or(0, |e| e.results.chained_event);
                    if next == 0 {
                        break;
                    }
                    walked.push_str(&format!("{id}~"));
                    id = next;
                    if walked.contains(&id.to_string()) {
                        looped = true;
                        let spell = event(id).map_or(0, |e| e.results.cast_spell);
                        if spell != 0 && facts.spell_fixed_hits(spell as u32) < 0 {
                            deadly += 1;
                        } else {
                            loops += 1;
                        }
                    }
                }
                // A walk ending on the defeat event is deadly; so is an empty slot (id 0)
                // when the map has no defeat event, which the original reads as event 0.
                if s.header.defeat_event == id {
                    deadly += 1;
                }
            }
        }
        points -= 25 * deadly;
        points -= round(500.0 * loops as f64 / s.points.len() as f64);
    }
    // A standalone map's hero presets.
    if s.header.scenario_kind == 0 {
        for hero in s.header.heroes.iter().filter(|h| h.x != 0) {
            let gold = hero.gold as u16 as i16 as i64;
            if gold == 0 {
                points -= 100;
            } else if gold > 1000 {
                points -= (gold - 1000) / 50;
            }
        }
    }
    let events = s.events.len() as i64;
    if events == 0 {
        points -= w;
    }
    if events < quests * 7 {
        points -= 50 * (quests - events / 7);
    }
    let s4 = points;

    let score = u16::try_from(curve(points, w)).map_err(|_| ScoreError::OutOfRange)?;
    let quests = u8::try_from(quests).map_err(|_| ScoreError::OutOfRange)?;
    let title: String = s.title.chars().chain(std::iter::repeat(' ')).take(s.title.chars().count().max(30)).collect();
    let line = format!("{title}|{s1:>5}|{}|{}|{}|{s2:>5}|{s3:>5}|{s4:>5}|", report[0], report[1], report[2]);
    Ok(Score { stages: [s1, s2, s3, s4], variety: var, score, quests, line })
}

/// The curve of §6.7: 0 up to W points; then √((s − W) / √W) × 11, kept with four
/// decimals, above 100 bent to (v − 100) × 0.75 + 100, rounded.
pub fn curve(points: i64, w: i64) -> i64 {
    let mut v = if points > w { round(((points - w) as f64 / (w as f64).sqrt()).sqrt() * 11.0 * 10000.0) } else { 0 };
    if v > 1_000_000 {
        v = round((v - 1_000_000) as f64 * 0.75 + 1_000_000.0);
    }
    round(v as f64 / 10000.0)
}

/// The colour band of a score in the status line (§6.8): 0, then from 1, 50, 100, 150, 200,
/// 300 and 400.
pub fn band(score: u16) -> usize {
    [1, 50, 100, 150, 200, 300, 400].iter().filter(|t| score >= **t).count()
}

/// Appends a scoring's line to `MapData.Txt` in `dir` (created when missing), in cp1251
/// with a CR LF line end.
pub fn append_map_data(dir: &std::path::Path, line: &str) -> std::io::Result<std::path::PathBuf> {
    use std::io::Write;
    std::fs::create_dir_all(dir)?;
    let path = dir.join("MapData.Txt");
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&path)?;
    f.write_all(&text::encode(line))?;
    f.write_all(b"\r\n")?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Point};
    use crate::editor::palette::Facts;

    fn names() -> Names {
        Names {
            facts: Facts {
                artefacts: vec![ArtefactFact { id: 1, cost: 2600, kind: 0 }, ArtefactFact { id: 2, cost: -9000, kind: 9 }, ArtefactFact { id: 3, cost: 40000, kind: 3 }],
                unit_costs: vec![(5, 200), (6, 100)],
                spell_fixed_hits: vec![Some(-5), None],
                recruit_div: 2,
            },
            ..Names::default()
        }
    }

    fn map(w: u32) -> Scenario {
        let mut s = Scenario::default();
        s.header.width = w;
        s.header.height = w;
        s.terrain = vec![6; (w * w) as usize];
        s
    }

    #[test]
    fn words_and_their_distance() {
        assert_eq!(words("Да, это  так!\r\nИди."), ["Да", "это", "так", "Иди"]);
        assert_eq!(words("..."), [""]);
        assert_eq!(word_distance("a b c", "a x c"), 1);
        assert_eq!(word_distance("a b c", "a c"), 1);
        assert_eq!(word_distance("Go now.", "go now"), 1, "words compare with case");
        assert_eq!(word_distance("one two", "three four five"), 3);
    }

    #[test]
    fn event_values() {
        let n = names();
        let mut quests = 0;
        let mut rewards = vec![];
        let mut e = Event { kind: 3, title: "Q%+A=/B".into(), ..Event::default() };
        e.conditions.meet_army = 2;
        e.conditions.happened_yes_check = 1;
        e.conditions.happened_yes = [4, 0];
        e.conditions.not_happened_check = 1;
        e.conditions.not_happened = [4, 5];
        e.conditions.confirm_question = 1;
        e.conditions.artifacts_check = 1;
        e.results.relative_event = 3;
        e.results.artifacts_add = [1, 2, 3, 0];
        e.results.gold = 4500;
        // 40 + 10 + 10 + 15 + 15 (=/B) + 20 + 10 + 15 + items (2600 div 250 = 10, 9000 div
        // 500 div 1500 = 0, 40000 div 250 capped at 15) + 2 + 5.
        assert_eq!(event_value(&n, &[], &e, &mut quests, &mut rewards), 40 + 10 + 10 + 15 + 15 + 20 + 10 + 15 + 25 + 2 + 5);
        assert_eq!((quests, rewards), (1, vec![10, 0, 15, 3]));
        let set_only = Event { title: "T%+A".into(), ..Event::default() };
        assert_eq!(event_value(&n, &[], &set_only, &mut quests, &mut vec![]), 0, "only a flag test counts");
    }

    #[test]
    fn the_curve_and_the_bands() {
        // An empty 50 × 50 map: all 44² inner cells are uniform land at every level, losing
        // (1936 − 400) div 10 + (1936 − 100) div 3 + (1936 − 50); with no buildings the
        // average spacing is W, losing (50 − 10)²; no events costs W. The score is 0.
        let s = map(50);
        let r = score(&s, &names(), &[]).unwrap();
        assert_eq!((r.score, r.quests), (0, 0));
        assert_eq!(r.variety.land, [1936; 3]);
        assert_eq!(r.stages, [0, -153 - 612 - 1886, -2651 - 1600, -4251 - 50]);
        // The curve: (s − W) / √W = 50 gives √50 × 11 = 77.78; 1000 on a 100 map gives
        // √100 × 11 = 110, bent to 107.5, rounded to even: 108; 1 point over W = 50 gives
        // √(1 / √50) × 11 = 4.1.
        assert_eq!(curve(100, 100), 0);
        assert_eq!(curve(100 + (50.0 * 100f64.sqrt()) as i64, 100), 78);
        assert_eq!(curve(100 + 1000, 100), 108);
        assert_eq!(curve(51, 50), 4);
        assert!(score(&map(49), &names(), &[]).is_err(), "narrower than 50: the original divides by zero");
        assert_eq!((band(0), band(1), band(49), band(50), band(399), band(400)), (0, 1, 1, 2, 6, 7));
    }

    #[test]
    fn a_small_map_scores_as_the_original() {
        let mut s = map(50);
        s.title = "Проба".into();
        // Two castles 8 apart (their centres), one with income; a lone village.
        let castle = |x, gold| Building { x, y: 10, kind: 3, size_x: 2, size_y: 2, gold_per_day: gold, ..Building::default() };
        s.buildings = vec![castle(10, 100), castle(18, 0)];
        s.armies = vec![Army { leader_unit: 9, unknown_80: 120, artifacts: [1, 0, 0], ..Army::default() }];
        let msg = "Добро пожаловать в наши земли, путник, здесь тебя ждут".to_string();
        s.events = vec![
            Event { kind: 1, message: msg.clone(), ..Event::default() },
            Event { kind: 2, message: format!("{msg} снова"), ..Event::default() },
        ];
        s.points = vec![Point { x: 20, y: 20, ..Point::default() }];
        s.header.heroes[0].x = 5;
        s.header.heroes[0].gold = 1500;
        let r = score(&s, &names(), &[]).unwrap();
        // Content: army 120 div 50 + 2600 div 500 = 2 + 5; castles 4 + 4. Events: 0 and 15,
        // each a near duplicate of the other (one word apart): 0 div 1, 15 div 1; text
        // length: √54 rounds to 7, t = −2, so the first (0, not above 10) is weak and becomes
        // −10; √60 rounds to 8, t = −1, and the second (15, above 10) gets −1 div 2 = 0.
        // Then 5 × 1 army + 5 × (2 − 1 weak) + 1 point.
        assert_eq!(r.stages[0], 7 + 8 + (-10) + 15 + 5 + 5 + 1);
        // The plain map is uniform everywhere: all 44² cells at every level, by marks, as
        // land, and by terrain; only the building cells break it up.
        assert!(r.variety.terrain[0] as i64 == 44 * 44 && r.variety.land[2] > 0 && r.variety.water == [0; 3]);
        // Spacing: the castles' centres (11, 11) and (19, 11) are 8 apart: no loss.
        assert_eq!(r.stages[2], r.stages[1]);
        // Penalties: the castle without income −10, the preset's gold (1500 − 1000) div 50.
        assert_eq!(r.stages[3], r.stages[2] - 10 - 10);
        assert!(r.line.starts_with("Проба                         |"));
        assert_eq!(r.line.matches('|').count(), 8);
        assert!(r.line.contains("100,0"), "{}", r.line);
    }

    #[test]
    fn map_data_lines_are_appended() {
        let dir = crate::editor::files::tests::temp_dir("mapdata");
        append_map_data(&dir, "Карта|    1|").unwrap();
        let path = append_map_data(&dir, "B|    2|").unwrap();
        assert_eq!(path, dir.join("MapData.Txt"));
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(text::decode(&bytes), "Карта|    1|\r\nB|    2|\r\n");
        assert_eq!(bytes.len(), 12 + 8 + 4, "one byte a letter in cp1251");
    }

    #[test]
    fn chains_from_points() {
        let mut s = map(50);
        let chain = |next: u16| {
            let mut e = Event { kind: 2, title: "-x".into(), ..Event::default() };
            e.results.chained_event = next;
            e
        };
        // 1 → 12 → 1: "1~12~" contains "1": a loop. 2 → 3 → 2, where 2 casts spell 1
        // (fixed hits −5): deadly. 4 → 5, the defeat event: deadly.
        // The repeated event's spell decides: event 2 is found again and casts spell 1.
        let mut deadly = chain(3);
        deadly.results.cast_spell = 1;
        let mut events: Vec<Event> = (1..=12).map(|_| chain(0)).collect();
        events[0] = chain(12);
        events[11] = chain(1);
        events[1] = deadly;
        events[2] = chain(2);
        events[3] = chain(5);
        s.events = events;
        s.header.defeat_event = 5;
        let mut p = Point { radius: 1, event_count: 3, ..Point::default() };
        p.event_slots[..3].copy_from_slice(&[1, 2, 4]);
        s.points = vec![p];
        let base = {
            let mut q = s.clone();
            q.points[0].event_count = 0;
            score(&q, &names(), &[]).unwrap().stages[3]
        };
        let r = score(&s, &names(), &[]).unwrap();
        // One loop (500 × 1 / 1 point) and two deadly chains (25 each).
        assert_eq!(r.stages[3], base - 500 - 50);
    }
}
