//! Richness: DT's playability score (the original editor's score button, DTMapEdit 0x5a0724),
//! ported to Razdor Studio's packages. Points for the map's content, penalties for monotonous
//! terrain, scattered buildings, uneven rewards and design slips, mapped through a curve to a score,
//! with a breakdown whose items select their objects, cells and lines.
//!
//! The formula is the original's (`docs/reference/editor/mapcheck-files.md` §6, and the rebuild in
//! `src/editor/playability.rs` of the paused `dt-editor` branch): its numbers, its rounding, its
//! order of corrections and its marks (§1). Only the inputs change, as a package has them:
//!
//! - the armies, buildings, terrain, objects and points are the compiled map's (`pack::compile`),
//!   read as the DT records they become;
//! - the events are the package's: each trigger is one event and each dialog the map opens is one
//!   (see [`events_of`] for the table); the victory is the compiled map's own event;
//! - the artefact costs, unit costs and spells come from the install with the package's units and
//!   items in it (`rules::content::Content`).
//!
//! A term the package has no equivalent for gets no points and is listed as «not in the format»; no
//! rule is invented for it. The score is Razdor's port of the original's measure, not a judgement of
//! the story (Health is the story's check).

use std::collections::BTreeMap;

use crate::dt::dtm::{Building, Event, EventConditions, EventResults, FlagScript, MapObject, Scenario};
use crate::i18n::{n_, tr};
use crate::dt::install::DtInstall;
use crate::dt::text;
use crate::pack::aliases;
use crate::pack::refs::DtKind;
use crate::pack::compile::{self, MapNames};
use crate::pack::load::{key_line, Located, Package};
use crate::pack::refs::Ref;
use crate::pack::triggers::{Action, Cond, Event as When, Trigger};
use crate::rules::content::Content;
use crate::rules::experience::round_half_even;
use crate::studio::cost::artefact_type_code;
use crate::studio::scores::{Item, Target};

/// The most cells one breakdown item selects (a uniform map has thousands).
pub const MAX_CELLS: usize = 200;

/// The install's and the package's facts the formula reads by id. An unknown id reads as 0, the
/// original's empty record.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Facts {
    /// Artefact id to (cost, type code).
    pub artefacts: BTreeMap<u32, (i64, u8)>,
    pub unit_costs: BTreeMap<u32, i64>,
    /// Spell id to its fixed hit change.
    pub spell_hits: BTreeMap<u32, i64>,
}

impl Facts {
    /// The facts of a content (the install's, with the package's units and items in it).
    pub fn of(c: &Content) -> Facts {
        Facts {
            artefacts: c.items.iter().map(|a| (a.id, (i64::from(a.cost), artefact_type_code(a.kind)))).collect(),
            unit_costs: c.units.iter().map(|u| (u.id, i64::from(u.cost))).collect(),
            spell_hits: c.spells.iter().filter_map(|s| s.delta_fixed_hits.map(|h| (s.id, i64::from(h)))).collect(),
        }
    }

    fn artefact(&self, id: u32) -> (i64, u8) {
        self.artefacts.get(&id).copied().unwrap_or((0, 0))
    }

    fn unit_cost(&self, id: u32) -> i64 {
        self.unit_costs.get(&id).copied().unwrap_or(0)
    }

    fn spell_fixed(&self, id: u32) -> i64 {
        self.spell_hits.get(&id).copied().unwrap_or(0)
    }
}

/// The ids of the map's armies and buildings by their index (the DT id minus 1); empty strings when
/// unknown, so an item then selects nothing.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Names {
    pub armies: Vec<String>,
    pub buildings: Vec<String>,
}

impl Names {
    /// The names of a compiled map's armies and buildings.
    pub fn of(n: &MapNames) -> Names {
        let by_index = |m: &BTreeMap<String, u32>| {
            let mut v = vec![String::new(); m.values().map(|&i| i as usize).max().unwrap_or(0)];
            for (name, &id) in m {
                if id >= 1 {
                    v[id as usize - 1] = name.clone();
                }
            }
            v
        };
        let armies: BTreeMap<String, u32> = n.armies.iter().map(|(k, v)| (k.clone(), u32::from(*v))).collect();
        let buildings: BTreeMap<String, u32> = n.buildings.iter().map(|(k, v)| (k.clone(), u32::from(*v))).collect();
        Names { armies: by_index(&armies), buildings: by_index(&buildings) }
    }

    fn army(&self, i: usize) -> Option<Target> {
        self.armies.get(i).filter(|s| !s.is_empty()).map(|s| Target::Army(s.clone()))
    }

    fn building(&self, i: usize) -> Option<Target> {
        self.buildings.get(i).filter(|s| !s.is_empty()).map(|s| Target::Building(s.clone()))
    }
}

/// One event of the map as the formula reads it, with where it is written.
#[derive(Clone, Debug)]
pub struct Source {
    pub event: Event,
    /// The event's name for a person: «trigger `x`», «dialog `y`», «victory».
    pub name: String,
    pub target: Target,
}

/// What the formula reads.
pub struct Input<'a> {
    pub s: &'a Scenario,
    /// The map's events in id order (id = position + 1); the victory first when the map has one.
    pub events: &'a [Source],
    pub facts: &'a Facts,
    /// The campaign place: 0 standalone, 1 the campaign's first map, 2 a later one (0x10F).
    pub campaign: u8,
    pub names: &'a Names,
}

/// The result of one scoring.
#[derive(Clone, Debug, PartialEq)]
pub struct Score {
    /// The points before the curve (the original's `s`): the items add up to it.
    pub total: i64,
    /// The score (header 0x122): 0 up to the map's width in points, then the curve of §6.7.
    pub curve_value: i64,
    /// The points after the content (§6.2), the terrain (§6.3), the spacing (§6.4) and the penalties
    /// (§6.6), as the original's four stages.
    pub stages: [i64; 4],
    /// The terrain counters of §6.3: T1–T3, L1–L3, W1–W3.
    pub variety: Variety,
    /// The quests (header 0x126).
    pub quests: i64,
    pub items: Vec<Item>,
}

/// The terrain counters of §6.3 (16-bit, as in the original).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Variety {
    pub terrain: [u16; 3],
    pub land: [u16; 3],
    pub water: [u16; 3],
}

/// Why the original would store no score: a map narrower than 50 (§6.3's division), a value outside
/// its field (§6.7), or a point's events out of range (§6.6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScoreError {
    TooNarrow,
    OutOfRange,
    PointEvents,
}

impl std::fmt::Display for ScoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            ScoreError::TooNarrow => "The map is narrower than 50 cells: the original gives no score.",
            ScoreError::OutOfRange => "The score or the quest count does not fit its field.",
            ScoreError::PointEvents => "A point lists an empty event slot, more than five events or an event past 5000.",
        })
    }
}

/// The terms the original has that a package has no equivalent for: listed, with no points.
pub const NOT_IN_FORMAT: [(&str, &str); 4] = [
    (n_("Event conditions"), n_("Event happened (56, 61, 69): the package has no «event happened» condition")),
    (n_("Event results"), n_("Relative event (77) and activated army (121): the package has no delayed events or army activation")),
    (n_("Event kinds"), n_("Rumour events (kind 4): the package has no rumours")),
    (n_("Event chains"), n_("Chained events (138) from points: the package has no event chains")),
];

/// Delphi's `Round`: halves to even.
fn round(x: f64) -> i64 {
    round_half_even(x)
}

/// The words of a text for the near-duplicate test (0x5303e8): `,./?!:"` and line breaks become
/// spaces, runs of spaces one, the ends trimmed; an empty result is one empty word.
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

/// The marks of §1 (DTMapEdit 0x5a2ed4): every cell's terrain code, replaced by minus the object's
/// class where an object stands (the first slot's objects over their n × n cells, the second's on
/// their own cell unless a deeper mark keeps it), and building footprints at −127, −126 on their
/// first row and column, −125 on their last. Row by row, `width` cells a row.
pub fn marks(s: &Scenario) -> Vec<i8> {
    let (w, h) = (i64::from(s.width()), i64::from(s.height()));
    let mut m: Vec<i8> = s.terrain.iter().map(|c| *c as i8).collect();
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let inside = |x: i64, y: i64| x >= 0 && y >= 0 && x < w && y < h;
    // The objects of each cell by slot: classes 1–8 the first, the others the second; a later object
    // of a slot replaces an earlier one, and classes 13 or more and the empty word are dropped.
    let mut grid: BTreeMap<(i64, i64), [Option<&MapObject>; 2]> = BTreeMap::new();
    for o in &s.objects {
        let empty = o.sprite == 0 && o.class == 0;
        if o.class > 12 || empty || i64::from(o.x) >= w || i64::from(o.y) >= h {
            continue;
        }
        let slot = if (1..=8).contains(&o.class) { 0 } else { 1 };
        grid.entry((i64::from(o.x), i64::from(o.y))).or_default()[slot] = Some(o);
    }
    for y in 0..h {
        for x in 0..w {
            let Some(o) = grid.get(&(x, y)).and_then(|c| c[0]) else { continue };
            let n = i64::from(o.sprite / 10);
            for dx in (1 - n)..=0 {
                for dy in (1 - n)..=0 {
                    if inside(x + dx, y + dy) && m[idx(x + dx, y + dy)] >= 0 {
                        m[idx(x + dx, y + dy)] = -(o.class as i8);
                    }
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let Some(o) = grid.get(&(x, y)).and_then(|c| c[1]) else { continue };
            let i = idx(x, y);
            let mark = m[i];
            let keeps = mark < 0 || (mark > 2 && !(4..=8).contains(&mark) && !(10..=14).contains(&mark));
            let overwritable = (-4..=-1).contains(&mark) || mark == -12;
            let word = u16::from(o.sprite) | u16::from(o.class) << 8;
            if word == 0x000B || !keeps || overwritable {
                m[i] = -(o.class as i8);
            }
        }
    }
    for b in &s.buildings {
        for dx in 0..i64::from(b.size_x) {
            for dy in 0..i64::from(b.size_y) {
                let value = if dx == i64::from(b.size_x) - 1 || dy == i64::from(b.size_y) - 1 {
                    -125
                } else if dx == 0 || dy == 0 {
                    -126
                } else {
                    -127
                };
                let (x, y) = (i64::from(b.x) - dx, i64::from(b.y) - dy);
                if inside(x, y) {
                    m[idx(x, y)] = value;
                }
            }
        }
    }
    m
}

/// The mark of a cell; 0 outside the map.
fn mark_at(s: &Scenario, marks: &[i8], x: i64, y: i64) -> i8 {
    let (w, h) = (i64::from(s.width()), i64::from(s.height()));
    if x < 0 || y < 0 || x >= w || y >= h {
        0
    } else {
        marks[(y * w + x) as usize]
    }
}

/// The terrain code of a cell; 0 outside the map.
fn terrain_or_zero(s: &Scenario, x: i64, y: i64) -> u8 {
    if x < 0 || y < 0 {
        return 0;
    }
    s.terrain_at(x as u32, y as u32).unwrap_or(0)
}

/// The original's impassable cell (0x556d70): deep sea, lava, impassable swamp or snowdrifts, or a
/// mark of −11 (thicket) or −8..−5 (mountains, rocks).
pub fn impassable(s: &Scenario, marks: &[i8], x: i64, y: i64) -> bool {
    let mark = mark_at(s, marks, x, y);
    matches!(terrain_or_zero(s, x, y), 2 | 3 | 9 | 15) || mark == -11 || (-8..=-5).contains(&mark)
}

/// The three square rings around a cell: 8 cells at distance 1, 16 at 2, 24 at 3 (0x5becdc, 0x5bed10).
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

/// The cells of §6.3 at least 3 from each edge, with how many of their rings differ: the loops take
/// the x range from the height and the y range from the width, as the original's do. `visit` gets
/// each cell with its cumulative count of differing mark (`by_mark`) and terrain code (`by_code`)
/// per ring.
fn each_cell(s: &Scenario, marks: &[i8], mut visit: impl FnMut((i64, i64), [usize; 3], [usize; 3])) {
    let (w, h) = (i64::from(s.width()), i64::from(s.height()));
    let rings = rings();
    for x in 3..h - 3 {
        for y in 3..w - 3 {
            let centre = mark_at(s, marks, x, y);
            let code = terrain_or_zero(s, x, y);
            let (mut by_mark, mut by_code) = ([0usize; 3], [0usize; 3]);
            let (mut sm, mut sc) = (0usize, 0usize);
            for (k, ring) in rings.iter().enumerate() {
                sm += ring.iter().filter(|(dx, dy)| mark_at(s, marks, x + dx, y + dy) != centre).count();
                sc += ring.iter().filter(|(dx, dy)| terrain_or_zero(s, x + dx, y + dy) != code).count();
                by_mark[k] = sm;
                by_code[k] = sc;
            }
            visit((x, y), by_mark, by_code);
        }
    }
}

/// The counters of §6.3: a cell is uniform at level k (1 to 3) when fewer than 2, 3 and 4 cells of
/// its first one, two and three rings differ. Land and water by the cell's mark, the terrain by its
/// code (reported only).
pub fn variety(s: &Scenario, marks: &[i8]) -> Variety {
    const LIMITS: [usize; 3] = [2, 3, 4];
    let mut v = Variety::default();
    each_cell(s, marks, |(x, y), by_mark, by_code| {
        let land = !(0..=2).contains(&mark_at(s, marks, x, y));
        for k in 0..3 {
            if by_mark[k] < LIMITS[k] {
                let c = if land { &mut v.land[k] } else { &mut v.water[k] };
                *c = c.wrapping_add(1);
            }
            if by_code[k] < LIMITS[k] {
                v.terrain[k] = v.terrain[k].wrapping_add(1);
            }
        }
    });
    v
}

/// The uniform cells of one level and kind, up to [`MAX_CELLS`], as the breakdown selects them.
fn uniform_cells(s: &Scenario, marks: &[i8], level: usize, land: bool) -> Vec<(i64, i64)> {
    const LIMITS: [usize; 3] = [2, 3, 4];
    let mut out = Vec::new();
    each_cell(s, marks, |cell, by_mark, _| {
        let is_land = !(0..=2).contains(&mark_at(s, marks, cell.0, cell.1));
        if is_land == land && by_mark[level] < LIMITS[level] && out.len() < MAX_CELLS {
            out.push(cell);
        }
    });
    out
}

/// The points of a building by its type (the jump table 0x5a0945).
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

/// Whether a building type counts for the spacing (§6.4): types 1–5 and 7–12.
fn spaced(kind: u8) -> bool {
    matches!(kind, 1..=5 | 7..=12)
}

/// A building's footprint centre: x + size x div 2, y + size y div 2.
fn centre(b: &Building) -> (i64, i64) {
    (i64::from(b.x) + i64::from(b.size_x / 2), i64::from(b.y) + i64::from(b.size_y / 2))
}

/// The value of an event (§6.2) and the rewards it adds to the statistic, with the figures of
/// what it counts.
fn event_value(facts: &Facts, e: &Event, quests: &mut i64, rewards: &mut Vec<i64>) -> (i64, Vec<String>) {
    let c = &e.conditions;
    let r = &e.results;
    let mut parts: Vec<String> = Vec::new();
    let mut v: i64 = match e.kind {
        2 => 15,
        3 => {
            *quests += 1;
            40
        }
        4 => 5,
        _ => 0,
    };
    parts.push(format!("{} {} {v}", tr("kind"), e.kind));
    if c.meet_army != 0 {
        v += 10;
        parts.push(format!("+10 {}", tr("meets an army")));
    }
    for (check, ids) in [(c.happened_yes_check, c.happened_yes), (c.happened_no_check, c.happened_no), (c.not_happened_check, c.not_happened)] {
        if check != 0 && ids[0] != 0 {
            let p = if ids[1] == 0 { 10 } else { 15 };
            v += p;
            parts.push(format!("+{p} {}", tr("event happened")));
        }
    }
    if e.flags.as_ref().is_some_and(|f| f.require_set.is_some() || f.require_unset.is_some()) {
        v += 15;
        parts.push(format!("+15 {}", tr("flag test")));
    }
    if c.confirm_question == 1 {
        v += 20;
        parts.push(format!("+20 {}", tr("question")));
    }
    for (on, p, what) in [(c.artifacts_check, 10, tr("artefact")), (c.buildings_check, 5, tr("building")), (c.defeated_check, 10, tr("defeated armies")), (c.units_check, 5, tr("named squad"))] {
        if on != 0 {
            v += p;
            parts.push(format!("+{p} {what}"));
        }
    }
    if r.relative_event != 0 {
        v += 15;
        parts.push(format!("+15 {}", tr("relative event")));
    }
    if r.activate_armies[0] != 0 {
        v += 5;
        parts.push(format!("+5 {}", tr("activates an army")));
    }
    if r.cast_spell != 0 {
        v += 10;
        parts.push(format!("+10 {}", tr("spell")));
    }
    let mut gained = 0;
    for &item in r.artifacts_add.iter().filter(|i| **i != 0) {
        let (cost0, kind) = facts.artefact(u32::from(item));
        let cost = if cost0 < 0 { -cost0 / 500 } else { cost0 };
        let value = (cost / if kind < 8 { 250 } else { 1500 }).min(15);
        v += value;
        rewards.push(value);
        gained += 1;
        parts.push(format!("+{value} {}", tr("artefact")));
    }
    if gained > 0 {
        v += 2;
        parts.push(format!("+2 {}", tr("gained artefacts")));
    }
    if r.gold > 0 {
        v += 5;
        rewards.push(i64::from(r.gold) / 2000 + 1);
        parts.push(format!("+5 {}", tr("gold")));
    } else if r.gold < 0 {
        v += 2;
        parts.push(format!("+2 {}", tr("gold lost")));
    }
    (v, parts)
}

/// Scores the map: §6.2 (the content), §6.3 (the terrain), §6.4 (the spacing), §6.5 (the rewards),
/// §6.6 (the penalties) and §6.7 (the curve). Every term is one item of the breakdown; they add up to
/// the total.
pub fn score(input: &Input) -> Result<Score, ScoreError> {
    let s = input.s;
    let facts = input.facts;
    let w = i64::from(s.width());
    let h = i64::from(s.height());
    if w / 50 == 0 {
        return Err(ScoreError::TooNarrow);
    }
    let marks = marks(s);
    let names = input.names;
    let mut items: Vec<Item> = Vec::new();
    let mut points: i64 = 0;

    // §6.2 armies: strength byte 80 div 50, the carried artefacts (cost div 500, or 2000 for the
    // types from 8, at most 10 each), and 5 points for each army (below).
    for (i, a) in s.armies.iter().enumerate() {
        let mut p = i64::from(a.unknown_80) / 50;
        let mut figures = format!("{} ÷ 50 = {p}", a.unknown_80);
        for &item in a.artifacts.iter().filter(|i| **i != 0) {
            let (cost0, kind) = facts.artefact(u32::from(item));
            let cost = if cost0 < 0 { -cost0 / 1000 } else { cost0 };
            let v = (cost / if kind < 8 { 500 } else { 2000 }).min(10);
            p += v;
            figures.push_str(&format!(", +{v}"));
        }
        p += 5;
        figures.push_str(", +5");
        // Byte 80 (the strength of §6.2) is not in a package's army: Razdor writes 0, so it scores 0.
        items.push(Item::new(n_("Armies"), n_("Carried artefacts and 5 points an army; strength (byte 80) is not in the format"), p, format!("{} {}", tr("army"), i + 1), figures, names.army(i).into_iter().collect()));
        points += p;
    }
    // §6.2 buildings: by type, and the random goods for sale (2 + the highest price div 1500).
    for (i, b) in s.buildings.iter().enumerate() {
        let mut p = building_points(b.kind);
        let mut figures = format!("{} {} = {p}", tr("type"), b.kind);
        if b.random_artifacts_for_sale != 0 {
            let goods = 2 + i64::from(b.price_max) / 1500;
            p += goods;
            figures.push_str(&format!(", {} +{goods}", tr("goods")));
        }
        if p != 0 {
            items.push(Item::new(n_("Buildings"), n_("Points by building type, and goods for sale"), p, format!("{} {}", tr("building"), i + 1), figures, names.building(i).into_iter().collect()));
        }
        points += p;
    }

    // §6.2 events: each event's value, its near duplicates (÷ their number), and its text length.
    let (mut quests, mut weak) = (0i64, 0i64);
    let mut rewards: Vec<i64> = Vec::new();
    let events = input.events;
    for (i, src) in events.iter().enumerate() {
        let id = (i + 1) as u16;
        let e = &src.event;
        let (mut v, mut parts) = event_value(facts, e, &mut quests, &mut rewards);
        let dups = events
            .iter()
            .enumerate()
            .filter(|(j, o)| *j != i && {
                let mut d = -1;
                if !e.message.is_empty() && !o.event.message.is_empty() {
                    d += word_distance(&o.event.message, &e.message);
                }
                if !e.question.is_empty() && !o.event.question.is_empty() {
                    d += word_distance(&o.event.question, &e.question);
                }
                d != -1 && d < 2
            })
            .count() as i64;
        if dups > 0 {
            parts.push(format!("÷ {dups} {}", tr("near duplicates")));
            v /= dups;
        }
        // Text length: t = Round(√(question + message length)) − 9, unless the title starts with `-`.
        // A short event that is not the victory or the defeat is weak.
        if !e.title.starts_with('-') {
            let len = text::encode(&e.question).len() + text::encode(&e.message).len();
            let t = round((len as f64).sqrt()) - 9;
            if t <= 0 && v <= 10 && s.header.victory_event != id && s.header.defeat_event != id {
                v = if v >= 1 { t } else { -10 };
                weak += 1;
                parts.push(format!("{}: {len}", tr("weak text")));
            } else {
                v += t / 2;
                parts.push(format!("{} {len} → {:+}", tr("text"), t / 2));
            }
        }
        items.push(Item::new(n_("Events"), n_("Event value (6.2)"), v, src.name.clone(), parts.join(", "), vec![src.target.clone()]));
        points += v;
    }
    let not_weak = 5 * (events.len() as i64 - weak);
    items.push(Item::new(n_("Events not weak"), n_("5 points an event that is not weak"), not_weak, "", format!("{} {}, {weak} {}", events.len(), tr("events"), tr("weak")), Vec::new()));
    points += not_weak;
    items.push(Item::new(n_("Points"), n_("1 point a map point"), s.points.len() as i64, "", format!("{} {}", s.points.len(), tr("points")), s.points.iter().enumerate().map(|(i, _)| Target::Point(i)).collect()));
    points += s.points.len() as i64;
    let campaign_points = match input.campaign {
        1 => 100,
        2 => 200,
        _ => 0,
    };
    items.push(Item::new(n_("Campaign place"), n_("100 for the first campaign map, 200 for a later one"), campaign_points, "", format!("{} {}", tr("place"), input.campaign), Vec::new()));
    points += campaign_points;
    // The reward statistic, in units of 1/10000: the mean and the standard deviation (§6.5).
    let n = rewards.len() as f64;
    let (mean, spread) = if rewards.is_empty() {
        (0, 0)
    } else {
        let m = rewards.iter().sum::<i64>() as f64 / n;
        let sq = rewards.iter().map(|v| (v * v) as f64).sum::<f64>() / n;
        (round(m * 10000.0), round((sq - m * m).max(0.0).sqrt() * 10000.0))
    };
    let reward_sum = rewards.iter().sum::<i64>() * 10000;
    let s1 = points;

    // §6.3 terrain variety: each count is a percentage of (W − 6)² in the report, and divided by
    // (W div 50)² for the penalty, the penalty being the part above its limit.
    let var = variety(s, &marks);
    let area = ((w - 6) * (w - 6)) as f64;
    let pct = |v: u16| format!("{:.1}", f64::from(v) * 100.0 / area);
    let scale = (w / 50) * (w / 50);
    let land = var.land.map(|v| i64::from(v) / scale);
    let water = var.water.map(|v| i64::from(v) / scale);
    for (k, (count, above, loss, is_land)) in [(land[0], 400, 10, true), (land[1], 100, 3, true), (land[2], 50, 1, true), (water[0], 800, 20, false), (water[1], 400, 6, false), (water[2], 200, 2, false)]
        .into_iter()
        .enumerate()
    {
        let level = k % 3;
        let p = if count > above { (count - above) / loss } else { 0 };
        let targets: Vec<Target> = if p > 0 { uniform_cells(s, &marks, level, is_land).into_iter().map(|(x, y)| Target::Cell(x, y)).collect() } else { Vec::new() };
        let kind = if is_land { "L" } else { "W" };
        items.push(Item::new(n_("Terrain variety"), n_("Uniform cells above the limit (6.3)"), -p, format!("{kind}{}", level + 1), format!("{count} {} {above}, ÷ {loss}; {} %", tr("over"), pct(if is_land { var.land[level] } else { var.water[level] })), targets));
        points -= p;
    }
    items.push(Item::new(n_("Terrain report"), n_("Terrain code variety, reported only (6.3)"), 0, "", format!("T1 {} %, T2 {} %, T3 {} %", pct(var.terrain[0]), pct(var.terrain[1]), pct(var.terrain[2])), Vec::new()));
    let s2 = points;

    // §6.4 building spacing: each spaced building's nearest other one (distance (2 max + min) div 2
    // between footprint centres; alone, W + H); an average above 10 costs (W div 50) × (avg − 10)².
    let spaced_idx: Vec<usize> = s.buildings.iter().enumerate().filter(|(_, b)| spaced(b.kind)).map(|(i, _)| i).collect();
    let mut sum = 0i64;
    for &i in &spaced_idx {
        let (x, y) = centre(&s.buildings[i]);
        let nearest = spaced_idx
            .iter()
            .filter(|&&j| j != i)
            .map(|&j| {
                let (ox, oy) = centre(&s.buildings[j]);
                let (dx, dy) = ((ox - x).abs(), (oy - y).abs());
                (2 * dx.max(dy) + dx.min(dy)) / 2
            })
            .fold(w + h, i64::min);
        sum += nearest;
    }
    let count = spaced_idx.len() as i64;
    let average = if count < 1 { w } else { sum / count };
    let spacing_loss = if average > 10 { (w / 50) * (average - 10) * (average - 10) } else { 0 };
    let targets: Vec<Target> = if spacing_loss > 0 { spaced_idx.iter().filter_map(|&i| names.building(i)).collect() } else { Vec::new() };
    items.push(Item::new(n_("Building spacing"), n_("Average nearest distance above 10 (6.4)"), -spacing_loss, "", format!("{} {average}", tr("average")), targets));
    points -= spacing_loss;
    let s3 = points;

    // §6.5 reward spread: when the mean plus the spread is above 5, the sum of the rewards times the
    // excess is lost.
    let spread_loss = if mean + spread > 50_000 { round(((mean + spread - 50_000) as f64) * reward_sum as f64 / 1e8) } else { 0 };
    items.push(Item::new(n_("Reward spread"), n_("Mean plus spread above 5 (6.5)"), -spread_loss, "", format!("{} {:.2}, {} {:.2}", tr("mean"), mean as f64 / 1e4, tr("spread"), spread as f64 / 1e4), Vec::new()));
    points -= spread_loss;

    // §6.6 penalties on buildings: the castle and village without income, ruins without a garrison,
    // a town, market or church without goods, and a starting building's income above 150.
    for (i, b) in s.buildings.iter().enumerate() {
        let target: Vec<Target> = names.building(i).into_iter().collect();
        let penalty = match b.kind {
            3 if b.gold_per_day == 0 => Some((10, n_("Castle without income (6.6)"))),
            2 if b.gold_per_day == 0 => Some((5, n_("Village without income (6.6)"))),
            12 if b.garrison[0].unit == 0 && b.owner_army == 0 => Some((3, n_("Ruins without a garrison (6.6)"))),
            1 | 6 | 7 if b.random_artifacts_for_sale == 0 => Some((5, n_("Town, market or church without goods (6.6)"))),
            _ => None,
        };
        if let Some((p, note)) = penalty {
            items.push(Item::new(n_("Penalty"), note, -p, format!("{} {}", tr("building"), i + 1), format!("{} {}", tr("type"), b.kind), target.clone()));
            points -= p;
        }
        if b.start_for.iter().any(|v| *v != 0) && b.gold_per_day > 150 {
            let over = i64::from(b.gold_per_day) - 150;
            let mut loss = over;
            for slot in b.barracks.iter().filter(|t| t.unit != 0) {
                if facts.unit_cost(u32::from(slot.unit)) > 150 {
                    loss += over / 2;
                }
            }
            items.push(Item::new(n_("Penalty"), n_("Starting income above 150 (6.6)"), -loss, format!("{} {}", tr("building"), i + 1), format!("{} {}", tr("income"), b.gold_per_day), target));
            points -= loss;
        }
    }
    // §6.6 armies: on an impassable cell (50), without a leader (15).
    for (i, a) in s.armies.iter().enumerate() {
        let target: Vec<Target> = names.army(i).into_iter().collect();
        if impassable(s, &marks, i64::from(a.x), i64::from(a.y)) {
            items.push(Item::new(n_("Penalty"), n_("Army on an impassable cell (6.6)"), -50, format!("{} {}", tr("army"), i + 1), format!("{} {}, {}", tr("cell"), a.x, a.y), target.clone()));
            points -= 50;
        }
        if a.leader_unit == 0 {
            items.push(Item::new(n_("Penalty"), n_("Army without a leader (6.6)"), -15, format!("{} {}", tr("army"), i + 1), String::new(), target));
            points -= 15;
        }
    }
    // §6.6 quests that no event completes (50 each).
    for (i, src) in events.iter().enumerate() {
        if src.event.kind == 3 && !events.iter().any(|o| o.event.results.completes_quest == (i + 1) as u16) {
            items.push(Item::new(n_("Penalty"), n_("Quest no event completes (6.6)"), -50, src.name.clone(), String::new(), vec![src.target.clone()]));
            points -= 50;
        }
    }
    // §6.6 points: global events attached (25 each), an impassable point (25 per event), and the
    // chains from the points' events: 25 per deadly chain and 500 × loops ÷ points.
    if !s.points.is_empty() {
        let (mut loops, mut deadly) = (0i64, 0i64);
        let event = |id: u16| (id as usize).checked_sub(1).and_then(|i| events.get(i)).map(|src| &src.event);
        for (pi, p) in s.points.iter().enumerate() {
            if p.event_count == 0 {
                continue;
            }
            if p.event_count > 5 {
                return Err(ScoreError::PointEvents);
            }
            let slots = &p.event_slots[..p.event_count as usize];
            if slots.iter().any(|id| !(1..=5000).contains(id)) {
                return Err(ScoreError::PointEvents);
            }
            let target = vec![Target::Point(pi)];
            for &id in slots {
                if event(id).is_some_and(|e| e.kind == 1) {
                    items.push(Item::new(n_("Penalty"), n_("Global event attached to a point (6.6)"), -25, format!("{} {id}", tr("event")), format!("{} {}", tr("point"), pi + 1), target.clone()));
                    points -= 25;
                }
            }
            if impassable(s, &marks, i64::from(p.x), i64::from(p.y)) {
                let loss = 25 * i64::from(p.event_count);
                items.push(Item::new(n_("Penalty"), n_("Point on an impassable cell (6.6)"), -loss, format!("{} {}", tr("point"), pi + 1), String::new(), target.clone()));
                points -= loss;
            }
            for &start in slots {
                let (mut id, mut walked, mut looped) = (start, String::new(), false);
                while !looped {
                    let next = event(id).map_or(0, |e| e.results.chained_event);
                    if next == 0 {
                        break;
                    }
                    walked.push_str(&format!("{id}~"));
                    id = next;
                    if id > 5000 {
                        return Err(ScoreError::PointEvents);
                    }
                    if walked.contains(&id.to_string()) {
                        looped = true;
                        let spell = event(id).map_or(0, |e| e.results.cast_spell);
                        if spell != 0 && facts.spell_fixed(u32::from(spell)) < 0 {
                            deadly += 1;
                        } else {
                            loops += 1;
                        }
                    }
                }
                if s.header.defeat_event == id {
                    deadly += 1;
                }
            }
        }
        items.push(Item::new(n_("Penalty"), n_("Deadly chains from points (6.6)"), -25 * deadly, "", format!("{deadly}"), Vec::new()));
        points -= 25 * deadly;
        let loop_loss = round(500.0 * loops as f64 / s.points.len() as f64);
        items.push(Item::new(n_("Penalty"), n_("Chain loops from points (6.6)"), -loop_loss, "", format!("{loops}"), Vec::new()));
        points -= loop_loss;
    }
    // §6.6 a standalone map's hero presets: gold 0 costs 100, above 1000 costs (gold − 1000) ÷ 50.
    if input.campaign == 0 {
        for hero in s.header.heroes.iter().filter(|h| h.x != 0) {
            let gold = i64::from(hero.gold as u16 as i16);
            let loss = if gold == 0 {
                100
            } else if gold > 1000 {
                (gold - 1000) / 50
            } else {
                0
            };
            items.push(Item::new(n_("Penalty"), n_("Hero gold preset (6.6)"), -loss, "hero", format!("{} {gold}", tr("gold")), vec![Target::Hero]));
            points -= loss;
        }
    }
    let event_count = events.len() as i64;
    if event_count == 0 {
        items.push(Item::new(n_("Penalty"), n_("No events at all (6.6)"), -w, "", String::new(), Vec::new()));
        points -= w;
    }
    if event_count < quests * 7 {
        let loss = 50 * (quests - event_count / 7);
        items.push(Item::new(n_("Penalty"), n_("More than one quest per seven events (6.6)"), -loss, "", format!("{quests} {}, {event_count} {}", tr("quests"), tr("events")), Vec::new()));
        points -= loss;
    }
    let s4 = points;

    for (term, note) in NOT_IN_FORMAT {
        items.push(Item::new(term, note, 0, "", String::new(), Vec::new()));
    }

    Ok(Score { total: points, curve_value: curve(points, w), stages: [s1, s2, s3, s4], variety: var, quests, items })
}

/// The curve of §6.7: 0 up to W points; then √((s − W) ÷ √W) × 11, kept with four decimals, above 100
/// bent to (v − 100) × 0.75 + 100, rounded.
pub fn curve(points: i64, w: i64) -> i64 {
    let mut v = if points > w { round(((points - w) as f64 / (w as f64).sqrt()).sqrt() * 11.0 * 10000.0) } else { 0 };
    if v > 1_000_000 {
        v = round((v - 1_000_000) as f64 * 0.75 + 1_000_000.0);
    }
    round(v as f64 / 10000.0)
}

/// The colour band of a score (§6.8): 0, then from 1, 50, 100, 150, 200, 300 and 400.
pub fn band(score: i64) -> usize {
    [1, 50, 100, 150, 200, 300, 400].iter().filter(|t| score >= **t).count()
}

/// The events of map `map` as the formula reads them, the compiled victory first when there is one.
///
/// | package | DT event (§6.2) |
/// |---|---|
/// | a trigger with a `quest` action | a quest (kind 3, 40 points) |
/// | a trigger on `hero_enters`, `hero_enters_building`, `army_defeated`, `building_captured`, `building_lost`, `army_arrives`, `dialog_choice`, `var_changed` | local (kind 2, 15) |
/// | any other trigger (the day, the week, the game's start) | global (kind 1, 0) |
/// | `hero_enters` | a met army (74, +10) |
/// | `army_defeated`, or `army_alive = false` | defeated armies (53, +10) |
/// | a `var` condition (a flag test) | the flag test in the script (+15) |
/// | a `has_item` condition | an artefact condition (46, +10) |
/// | a `building_owner` condition | a building condition (29, +5) |
/// | a `has_named` condition | a named squad (36, +5) |
/// | a `cast` action | a spell on the player (81, +10) |
/// | a `give_item` action | a gained artefact (113–116) |
/// | a `give_gold` action above 0 | a positive gold change (85, +5) |
/// | a dialog the map opens (a trigger's or a choice's `dialog`) | a local event: its text is the message, its choices' texts the question; two or more choices are a question (76, +20); the choices' conditions and actions are its conditions and results |
/// | a `complete_quest` action | the quest it completes (the event's id, so the quest is not uncompleted) |
pub fn events_of(pkg: &Package, map: &str, lang: &str, victory: Option<(Event, u32)>, map_file: &str) -> Vec<Source> {
    let texts = pkg.texts.get(lang);
    let text_of = |key: &str| texts.and_then(|m| m.get(key)).cloned().unwrap_or_else(|| key.to_string());
    let item_ids = compile::item_ids(pkg);
    let mut out: Vec<Source> = Vec::new();
    if let Some((event, line)) = victory {
        out.push(Source { event, name: tr("victory").into(), target: Target::Line { file: map_file.to_string(), line } });
    }
    let triggers: Vec<&Located<Trigger>> = pkg.triggers.iter().filter(|t| t.value.map.as_deref().is_none_or(|m| m == map)).collect();
    // The dialogs the map opens, in the order they are opened, with the dialogs their choices open.
    let mut opened: Vec<String> = Vec::new();
    for t in &triggers {
        for a in &t.value.then {
            if let Action::Dialog(d) = a {
                if !opened.contains(d) {
                    opened.push(d.clone());
                }
            }
        }
    }
    let mut k = 0;
    while k < opened.len() {
        if let Some(d) = pkg.dialogs.iter().find(|d| d.value.id == opened[k]) {
            for c in &d.value.choices {
                for a in &c.then {
                    if let Action::Dialog(n) = a {
                        if !opened.contains(n) {
                            opened.push(n.clone());
                        }
                    }
                }
            }
        }
        k += 1;
    }
    // Each event's quest creations and completions by name; resolved once the ids are known.
    let mut creates: BTreeMap<String, usize> = BTreeMap::new();
    let mut completes: Vec<(usize, String)> = Vec::new();
    for t in &triggers {
        // A trigger has no text of its own: the text-length rule (§6.2) is left out by its title, which
        // starts with `-` (the original's own exemption; its title is a name here).
        let mut e = Event { kind: trigger_kind(&t.value), title: "-".into(), ..Event::default() };
        conditions(&t.value.cond, &mut e);
        if matches!(t.value.when, When::HeroEnters(_)) {
            e.conditions.meet_army = 1;
        }
        if matches!(t.value.when, When::ArmyDefeated(_)) {
            e.conditions.defeated_check = 1;
        }
        let at = out.len();
        results(&t.value.then, &mut e, &item_ids, &mut creates, &mut completes, at);
        out.push(Source { event: e, name: format!("{} `{}`", tr("trigger"), t.value.id), target: Target::Line { file: t.file.clone(), line: t.line } });
    }
    for d in pkg.dialogs.iter().filter(|d| opened.contains(&d.value.id)) {
        let dlg = &d.value;
        let mut e = Event { kind: 2, ..Event::default() };
        e.message = text_of(&dlg.text);
        e.question = dlg.choices.iter().map(|c| text_of(&c.text)).collect::<Vec<_>>().join("\n");
        if dlg.choices.len() >= 2 {
            e.conditions.confirm_question = 1;
        }
        let at = out.len();
        for c in &dlg.choices {
            conditions(&c.cond, &mut e);
            results(&c.then, &mut e, &item_ids, &mut creates, &mut completes, at);
        }
        out.push(Source { event: e, name: format!("{} `{}`", tr("dialog"), dlg.id), target: Target::Line { file: d.file.clone(), line: d.line } });
    }
    // A quest's event is the one that creates it (its id is the position + 1); a completion names it.
    for (at, quest) in completes {
        if let Some(&creator) = creates.get(&quest) {
            out[at].event.results.completes_quest = (creator + 1) as u16;
        }
    }
    for (i, src) in out.iter_mut().enumerate() {
        if creates.values().any(|&c| c == i) {
            src.event.kind = 3;
        }
    }
    out
}

/// The kind of a trigger's event (see [`events_of`]).
fn trigger_kind(t: &Trigger) -> u8 {
    if t.then.iter().any(|a| matches!(a, Action::Quest { .. })) {
        return 3;
    }
    match t.when {
        When::HeroEnters(_) | When::HeroEntersBuilding(_) | When::ArmyDefeated(_) | When::BuildingCaptured(_) | When::BuildingLost(_) | When::ArmyArrives { .. } | When::DialogChoice { .. } | When::VarChanged(_) => 2,
        _ => 1,
    }
}

/// The conditions of a trigger or a choice on its DT event (a flag test is the event's script).
fn conditions(conds: &[Cond], e: &mut Event) {
    for cond in conds {
        match cond {
            Cond::Var { name, .. } => {
                let f: &mut FlagScript = e.flags.get_or_insert_with(FlagScript::default);
                f.require_set.get_or_insert_with(|| name.clone());
            }
            Cond::HasItem { .. } => e.conditions.artifacts_check = 1,
            Cond::BuildingOwner { .. } => e.conditions.buildings_check = 1,
            Cond::HasNamed { .. } => e.conditions.units_check = 1,
            Cond::ArmyAlive { alive: false, .. } => e.conditions.defeated_check = 1,
            _ => {}
        }
    }
    let _: &EventConditions = &e.conditions;
}

/// The results of a trigger or a choice on its DT event: spells, gold, gained artefacts, and the
/// quests it creates or completes (named, resolved by [`events_of`]).
fn results(acts: &[Action], e: &mut Event, items: &BTreeMap<&str, u32>, creates: &mut BTreeMap<String, usize>, completes: &mut Vec<(usize, String)>, at: usize) {
    let r: &mut EventResults = &mut e.results;
    for a in acts {
        match a {
            Action::Cast { spell, .. } => {
                if let Some(id) = spell_id(spell) {
                    r.cast_spell = id.min(255) as u8;
                } else {
                    r.cast_spell = 1;
                }
            }
            Action::GiveGold(n) if *n > 0 => r.gold = (*n).min(i64::from(i16::MAX)) as i16,
            Action::GiveItem { item, .. } => {
                if let (Some(id), Some(slot)) = (item_id(item, items), r.artifacts_add.iter_mut().find(|v| **v == 0)) {
                    *slot = id.min(255) as u8;
                }
            }
            Action::Quest { id, .. } => {
                creates.entry(id.clone()).or_insert(at);
            }
            Action::CompleteQuest(id) => completes.push((at, id.clone())),
            _ => {}
        }
    }
}

/// The DT spell id of a base spell (by its alias); a package spell has none here.
fn spell_id(spell: &Ref) -> Option<u32> {
    match spell {
        Ref::Dt { kind: DtKind::Spell, alias } => aliases::resolve(DtKind::Spell, alias),
        _ => None,
    }
}

/// The item id of a reference: a package item by its id, a base item by its alias.
pub fn item_id(item: &Ref, items: &BTreeMap<&str, u32>) -> Option<u32> {
    match item {
        Ref::Local { id } => items.get(id.as_str()).copied(),
        Ref::Dt { kind: DtKind::Item, alias } => aliases::resolve(DtKind::Item, alias),
        _ => None,
    }
}

/// A map's campaign place: 1 the campaign's first map, 2 a later one, 0 standalone (0x10F).
pub fn campaign_place(pkg: &Package, map: &str) -> u8 {
    match &pkg.campaign {
        Some(c) if c.start == map => 1,
        Some(c) if c.maps.iter().any(|m| m.id == map) => 2,
        _ => 0,
    }
}

/// The richness of a package map, on the install: the map is compiled, the facts come from the
/// install with the package's units and items, and the events from the package.
pub fn richness(map: &str, pkg: &Package, install: &DtInstall, lang: &str) -> Result<(Score, Names), String> {
    let compiled = compile::compile(pkg, install, lang, map).map_err(|r| r.to_text().lines().next().unwrap_or_default().to_string())?;
    let content = Content::from_dt(install).with_package(compiled.units.clone(), compiled.items.clone());
    let facts = Facts::of(&content);
    let names = Names::of(&compiled.names);
    let map_file = format!("maps/{map}/map.toml");
    let file = pkg.files.get(&map_file).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    let victory = compiled.scenario.events.first().cloned().map(|e| (e, key_line(&file, "victory").unwrap_or(1)));
    let events = events_of(pkg, map, lang, victory, &map_file);
    let input = Input { s: &compiled.scenario, events: &events, facts: &facts, campaign: campaign_place(pkg, map), names: &names };
    let score = score(&input).map_err(|e| e.to_string())?;
    Ok((score, names))
}

#[cfg(test)]
#[path = "richness/tests.rs"]
mod tests;
