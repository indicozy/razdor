//! The world generator's window (the original's TMakeWorld, docs/reference/editor/worldgen.md
//! §2): three tabs — buildings and roads, economy, armies and garrisons — each with the
//! original's options; "Run" runs the step of the tab shown on the open map (one undo step).
//! The window is made anew at every opening, as the original's is, so its budgets follow
//! the open map's width.

use macroquad::prelude::*;

use razdor::editor::worldgen::{budget, Chance, Counters, Options, Report, Stop, CHANCE_ENTRIES};
use razdor::editor::WorldStep;
use razdor::i18n::{n_, tr};
use razdor::trf;

use crate::ui::widgets::*;

pub enum WorldGenAction {
    None,
    Run(WorldStep),
    Close,
}

pub struct WorldGenState {
    pub options: Options,
    pub tab: usize,
    /// The counters shown: the open map's buildings at the opening, then the last run's.
    pub counters: Counters,
    /// What the last run said.
    pub note: Option<String>,
}

impl WorldGenState {
    pub fn new(width: u32, existing: Counters) -> WorldGenState {
        WorldGenState { options: Options::new(width), tab: 0, counters: existing, note: None }
    }

    /// A run's report: its counters (step 1 counts what it made) and why it stopped.
    pub fn ran(&mut self, step: WorldStep, r: &Report) {
        if step == WorldStep::BuildingsAndRoads {
            self.counters = r.counters;
        }
        self.note = Some(match r.stop {
            Some(s) => stop_text(s),
            None => trf!("{step}: done.", step = tr(step.label())),
        });
    }
}

/// Why a step stopped, for the window and the status line.
pub fn stop_text(s: Stop) -> String {
    match s {
        Stop::FootprintOffMap { building } => trf!("Building {building} reaches past the map's left or top edge: the original's clearing stops the step there with a range error, and so did Razdor.", building),
        Stop::NarrowMap => tr("The map is narrower than 50 cells: the original divides by its sector count, 0, so the step stopped after clearing the buildings.").into(),
        Stop::TownTable => tr("A town of the extra sector visit fell outside the original's town table (a map 800 wide and taller): the step stopped there with a range error, as the original's does.").into(),
        Stop::TownRecord => tr("A town found its place but was refused (check the brush size) before any building stood: the original's road pass stops with a range error, and so did Razdor.").into(),
        Stop::JunctionAtEdge { x, y } => trf!("The junction building at ({x}, {y}) reaches past the map's top or left edge: the original stops the step there with a range error, and so did Razdor.", x, y),
        Stop::EconomyValue { building } => trf!("A value of building {building} does not fit its field (check the grids): the original stops the step there with a range error, and so did Razdor.", building),
        Stop::ArmyIncome { building, value } => trf!("The army of building {building} would earn {value} tens a day, more than its byte holds: the original stops the step there with a range error, and so did Razdor.", building, value),
        Stop::ArmyGold { building, value } => trf!("The army of building {building} would get {value} gold, more than its field holds: the original stops the step there with a range error, and so did Razdor.", building, value),
        Stop::UnitId { building } => trf!("A theme lists a unit above 255 (building {building}): the original stops the step there with a range error, and so did Razdor.", building),
        Stop::Hang { building, slot, lo, hi } => {
            trf!("No unit of the theme costs {lo} to {hi} (building {building}, slot {slot}) and the window cannot widen: the original would never finish; Razdor stopped the step there.", building, slot, lo, hi)
        }
    }
}

const TABS: [&str; 3] = [n_("Buildings and roads"), n_("Economy"), n_("Armies and garrisons")];

/// The drop-downs of the first and third tabs.
const CHANCES_1: [(Chance, &str); 5] = [(Chance::Towns, n_("Towns")), (Chance::Castles, n_("Castles")), (Chance::Villages, n_("Villages")), (Chance::Ruins, n_("Ruins")), (Chance::Other, n_("Other buildings"))];
const CHANCES_3: [(Chance, &str); 5] = [
    (Chance::TownArmies, n_("Armies in towns")),
    (Chance::CastleArmies, n_("Armies in castles")),
    (Chance::VillageArmies, n_("Armies in villages")),
    (Chance::RuinArmies, n_("Armies in ruins")),
    (Chance::OtherArmies, n_("Armies in other buildings")),
];

/// The budget ranges: label and the low and high spin boxes.
const RANGES: [(&str, usize, usize); 8] = [
    (n_("Town armies"), budget::T1, budget::T2),
    (n_("Castle armies"), budget::C1, budget::C2),
    (n_("Village armies"), budget::V1, budget::V2),
    (n_("Ruin armies"), budget::R1, budget::R2),
    (n_("Fort, market and church armies"), budget::O1, budget::O2),
    (n_("Town garrisons"), budget::TG1, budget::TG2),
    (n_("Castle garrisons"), budget::CG1, budget::CG2),
    (n_("Ruin garrisons"), budget::RG1, budget::RG2),
];

fn chance_row(key: &str, x: f32, y: f32, label: &str, value: &mut u8) {
    text_fit(label, x, y + 18.0, 190.0, 16.0, INK);
    let entries: Vec<(i64, String)> = (0..CHANCE_ENTRIES).map(|k| (k as i64, format!("{} %", razdor::editor::worldgen::chance(k)))).collect();
    if let Some(v) = dropdown(key, x + 196.0, y, 100.0, *value as i64, &entries) {
        *value = v as u8;
    }
}

/// A text cell of a grid.
fn cell(key: &str, x: f32, y: f32, w: f32, value: &mut String) {
    text_field(key, x, y, w, 26.0, value, false);
}

/// The window in rectangle `r`.
pub fn window(st: &mut WorldGenState, r: Rect) -> WorldGenAction {
    let (x, mut y) = (r.x + 20.0, r.y + 32.0);
    text(tr("World generator"), x, y, 22.0, ACCENT);
    y += 14.0;
    let labels = TABS.map(tr);
    y += tabs(x, y, r.w - 40.0, &labels, &mut st.tab) + 14.0;
    let o = &mut st.options;
    match st.tab {
        0 => {
            for (k, (c, label)) in CHANCES_1.iter().enumerate() {
                chance_row(&format!("wg:c{k}"), x, y, tr(label), &mut o.chances[*c as usize]);
                y += 34.0;
            }
            y += 10.0;
            let c = st.counters;
            let rows = [
                (tr("Towns"), c.towns),
                (tr("Villages"), c.villages),
                (tr("Castles"), c.castles),
                (tr("Forts"), c.forts),
                (tr("Shipyards"), c.shipyards),
                (tr("Bridges"), c.bridges),
                (tr("Taverns"), c.taverns),
                (tr("Churches"), c.churches),
                (tr("Markets"), c.markets),
                (tr("Ruins"), c.ruins),
            ];
            for (k, (label, n)) in rows.iter().enumerate() {
                let (cx, cy) = (x + 330.0 + (k / 5) as f32 * 190.0, r.y + 110.0 + (k % 5) as f32 * 26.0);
                text_fit(&format!("{label}: {n}"), cx, cy, 180.0, 16.0, DIM);
            }
            text_fit(tr("Clears every building and road, then builds new ones (armies stay)."), x, y + 16.0, r.w - 40.0, 15.0, DIM);
        }
        1 => {
            text(tr("Income"), x, y + 16.0, 17.0, ACCENT);
            y += 26.0;
            let heads = [tr("Town gold"), tr("Castle gold"), tr("Village gold"), tr("Village mana")];
            for (k, head) in heads.iter().enumerate() {
                let cx = x + k as f32 * 130.0;
                text_fit(head, cx, y + 14.0, 124.0, 15.0, DIM);
                cell(&format!("wg:inc{k}"), cx, y + 20.0, 120.0, &mut o.income[k]);
            }
            y += 62.0;
            text(tr("Trade"), x, y + 16.0, 17.0, ACCENT);
            y += 26.0;
            let cols = [tr("Lowest price"), tr("Highest price"), tr("Goods")];
            for (k, head) in cols.iter().enumerate() {
                text_fit(head, x + 110.0 + k as f32 * 130.0, y + 14.0, 124.0, 15.0, DIM);
            }
            y += 20.0;
            for (row, label) in [tr("Town"), tr("Market"), tr("Church")].iter().enumerate() {
                text_fit(label, x, y + 18.0, 104.0, 16.0, INK);
                for col in 0..3 {
                    cell(&format!("wg:tr{row}{col}"), x + 110.0 + col as f32 * 130.0, y, 120.0, &mut o.trade[row][col]);
                }
                y += 32.0;
            }
            y += 10.0;
            text(tr("Library"), x, y + 16.0, 17.0, ACCENT);
            y += 26.0;
            let cols = [tr("Lowest price"), tr("Highest price"), tr("Spells")];
            for (k, head) in cols.iter().enumerate() {
                text_fit(head, x + 110.0 + k as f32 * 130.0, y + 14.0, 124.0, 15.0, DIM);
            }
            y += 20.0;
            for (row, label) in [tr("Town"), tr("Church")].iter().enumerate() {
                text_fit(label, x, y + 18.0, 104.0, 16.0, INK);
                for col in 0..3 {
                    cell(&format!("wg:lib{row}{col}"), x + 110.0 + col as f32 * 130.0, y, 120.0, &mut o.library[row][col]);
                }
                y += 32.0;
            }
            text_fit(tr("A cell that is not a number takes the original's own fallback value."), x, y + 16.0, r.w - 40.0, 15.0, DIM);
        }
        _ => {
            let top = y;
            for (k, (c, label)) in CHANCES_3.iter().enumerate() {
                chance_row(&format!("wg:a{k}"), x, y, tr(label), &mut o.chances[*c as usize]);
                y += 32.0;
            }
            y += 6.0;
            text_fit(tr("Budgets (low, high)"), x, y + 16.0, 300.0, 16.0, ACCENT);
            y += 24.0;
            for (k, (label, lo, hi)) in RANGES.iter().enumerate() {
                text_fit(tr(label), x, y + 18.0, 190.0, 15.0, INK);
                for (j, idx) in [*lo, *hi].into_iter().enumerate() {
                    let key = format!("wg:b{k}{j}");
                    if let Some(v) = number_field_step(&key, x + 196.0 + j as f32 * 110.0, y, 104.0, o.budgets[idx] as i64, 0, budget::MAX as i64, budget::STEP as i64) {
                        o.budgets[idx] = v as i32;
                    }
                }
                y += 30.0;
            }
            // The minimum-point keypad: 7 8 9 on top, 1 2 3 at the bottom, and "none".
            let (kx, ky) = (x + 440.0, top);
            text_fit(tr("Minimum point"), kx, ky + 16.0, 200.0, 16.0, ACCENT);
            for (row, keys) in [[7u8, 8, 9], [4, 5, 6], [1, 2, 3]].iter().enumerate() {
                for (col, key) in keys.iter().enumerate() {
                    if toggle_button(kx + col as f32 * 44.0, ky + 26.0 + row as f32 * 44.0, 40.0, 40.0, &key.to_string(), o.min_point == *key) {
                        o.min_point = *key;
                    }
                }
            }
            if toggle_button(kx, ky + 26.0 + 3.0 * 44.0, 128.0, 30.0, tr("None"), o.min_point == 0) {
                o.min_point = 0;
            }
            let cy = ky + 26.0 + 3.0 * 44.0 + 44.0;
            if let Some(v) = checkbox(kx, cy, r.right() - kx - 20.0, tr("Only buildings without an owner (keeps the armies)"), o.unowned_only) {
                o.unowned_only = v;
            }
            if let Some(v) = checkbox(kx, cy + 30.0, r.right() - kx - 20.0, tr("Town and castle armies are enemies"), o.enemies_only) {
                o.enemies_only = v;
            }
        }
    }
    // What the last run said, and the buttons.
    let (bx, by) = (r.right() - 270.0, r.bottom() - 54.0);
    if let Some(note) = &st.note {
        for (i, line) in wrap(note, bx - x - 20.0, 15.0).iter().take(3).enumerate() {
            text(line, x, by + 4.0 + i as f32 * 17.0, 15.0, INK);
        }
    }
    if button(bx, by, 120.0, 40.0, tr("Run"), true) {
        return WorldGenAction::Run(WorldStep::ALL[st.tab.min(2)]);
    }
    let esc = !typing() && is_key_pressed(KeyCode::Escape);
    if button(bx + 130.0, by, 120.0, 40.0, tr("Close"), true) || esc {
        return WorldGenAction::Close;
    }
    WorldGenAction::None
}
