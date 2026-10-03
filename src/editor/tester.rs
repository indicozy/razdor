//! The original editor's battle tester (TTestBattle, docs/reference/editor/testers.md §2): two
//! armies on the battle grid, built by hand from a catalogue of the unit types, generated at
//! random or loaded from `Battle.Sav`, fought under the editor's own battle engine
//! ([`Rules::Editor`]) with its five rule switches. The window is `ui::editor::tester`.
//!
//! Side 1 is the bottom army (the player's), side 2 the top army (the AI's); [`Tester::armies`]
//! holds them in that order.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::dt::ini::Ini;
use crate::rules::battle::{Battle, EndReason, Rules, Switches, Team};
use crate::rules::content::{Content, Nature, UnitId};
use crate::rules::experience::{self, SideUnit};
use crate::rules::formation::{Formation, Row, Slot};
use crate::rules::rng::Rng;
use crate::rules::units::Unit;

/// The placement file in the editor's folder.
pub const FILE: &str = "Battle.Sav";
/// The grid: 3 rows of 6 columns, the wide 6/4/2 pyramid.
pub const ROWS: u8 = 3;
pub const COLS: u8 = 6;
/// Units per army.
pub const MAX_UNITS: usize = 12;
/// The random armies' budget: Tag × 1000 + 100 (buttons of Tag 1 and 2).
pub fn budget(tag: i32) -> i32 {
    tag * 1000 + 100
}
/// A random army stops when this much or less is left.
const BUDGET_LEFT: i32 = 100;
/// The cheapest type a random army takes.
const MIN_COST: i32 = 10;
/// The step delay with the delay box on, ms (0x504280).
pub const STEP_DELAY_MS: u64 = 500;

/// A tester army's unit: the unit and its strength field (unit +0x6c), which the side
/// strength and the cost total weigh. A catalogue unit's is its type's gold `Cost`; an army
/// from the AI viewer brings its tactical costs.
#[derive(Clone, Debug)]
pub struct Placed {
    pub unit: Unit,
    pub value: i32,
}

/// One army: its units in list order, the cost label's total and the building defence its
/// units fight with (0 for the tester's own armies).
#[derive(Clone, Debug, Default)]
pub struct Army {
    pub units: Vec<Placed>,
    pub cost: i32,
    pub building_defence: i32,
}

impl Army {
    /// The unit standing on `s`.
    pub fn at(&self, s: Slot) -> Option<usize> {
        self.units.iter().position(|p| p.unit.slot == s)
    }

    /// The side strength (Side_Strength 0x4f61f4): each unit's value × HP / max HP by rows,
    /// the back row's non-warriors twice, the back row scaled by the front, a lone
    /// non-warrior a fifth.
    pub fn strength(&self, c: &Content) -> i64 {
        let side: Vec<SideUnit> = self
            .units
            .iter()
            .map(|p| {
                let s = p.unit.stats(c);
                SideUnit { tactical: p.value, hp: p.unit.hp, max_hp: s.max_hp(), row: p.unit.slot.row, role: experience::role(&s) }
            })
            .collect();
        experience::side_strength(&side)
    }

    fn sum(&self) -> i32 {
        self.units.iter().fold(0i32, |a, p| a.wrapping_add(p.value))
    }
}

/// The grid cells of the battle grid that exist (`row ≤ col ≤ 7 − row`, 1-based).
pub fn cells() -> impl Iterator<Item = Slot> {
    Formation::WIDE.slots()
}

/// Why the random armies stopped early.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomStop {
    /// The side (1 or 2) whose army stopped.
    pub side: u8,
    /// The budget it had left.
    pub left: i32,
}

/// The tester's state, kept for the session (the original's window is made once).
#[derive(Clone)]
pub struct Tester {
    content: Arc<Content>,
    /// The unit types in their order: entry k is type number k + 1, the number `Battle.Sav`
    /// stores.
    pub catalogue: Vec<UnitId>,
    pub armies: [Army; 2],
    /// The start button is enabled: only after a clear, a load, random armies or armies
    /// from the AI viewer (and only then can the grid be edited).
    pub armed: bool,
    pub switches: Switches,
    pub super_ai: bool,
    pub all_ai: bool,
    /// The delay box (checked by default).
    pub delay: bool,
}

impl Tester {
    pub fn new(content: Arc<Content>) -> Tester {
        let mut catalogue: Vec<UnitId> = content.unit_ids().collect();
        catalogue.sort_by_key(|u| u.0);
        Tester { content, catalogue, armies: Default::default(), armed: false, switches: Switches::default(), super_ai: false, all_ai: false, delay: true }
    }

    pub fn content(&self) -> &Arc<Content> {
        &self.content
    }

    /// A catalogue unit of type `id` on `slot` (0x5771c0): the type's base stats, no level
    /// gains, no items, full HP; its strength field is the gold cost.
    pub fn catalogue_unit(&self, id: UnitId, slot: Slot) -> Placed {
        Placed { unit: Unit::new(&self.content, id, slot), value: self.content.unit(id).cost }
    }

    /// Super AI (0x57ebac): ticking it ticks all-AI as well; unticking leaves all-AI on.
    pub fn set_super_ai(&mut self, on: bool) {
        self.super_ai = on;
        if on {
            self.all_ai = true;
        }
    }

    /// A click on cell `slot` of army `side` (0 bottom, 1 top) outside a battle (0x57d3f8):
    /// only while the start button is enabled, an occupied cell loses its unit (the later
    /// ones move down a place, the cost drops by its value), an empty one gets the type
    /// `selected`. True if the army changed.
    pub fn click(&mut self, side: usize, slot: Slot, selected: Option<UnitId>) -> bool {
        if !self.armed || !Formation::WIDE.contains(slot) {
            return false;
        }
        let new = selected.map(|id| self.catalogue_unit(id, slot));
        let army = &mut self.armies[side];
        if let Some(k) = army.at(slot) {
            let gone = army.units.remove(k);
            army.cost = army.cost.wrapping_sub(gone.value);
            true
        } else if let Some(p) = new {
            army.cost = army.cost.wrapping_add(p.value);
            army.units.push(p);
            true
        } else {
            false
        }
    }

    /// Clear (0x57de24): both armies empty, the start button enabled.
    pub fn clear(&mut self) {
        self.armies = Default::default();
        self.armed = true;
    }

    /// Swap (0x57dea8): the armies change places whole.
    pub fn swap(&mut self) {
        self.armies.swap(0, 1);
    }

    /// Two armies from the AI viewer, as built for battle (0x5770f8): side 1 then side 2.
    pub fn from_viewer(&mut self, side1: Army, side2: Army) {
        self.armies = [side1, side2];
        self.armed = true;
    }

    /// The random armies (0x5795ec) of the button with `tag` (1 or 2), from `rng` (the
    /// original seeds it from the CPU clock, so the armies cannot be repeated). Each side on
    /// its own: the whole budget; one roll of 9 for the natures (0–5 Normal, Rogue or Hero,
    /// 6–7 Rogue, 8 Undead); then random types, taking one whose nature is in the set and
    /// whose cost is at least 10 and below the budget left, until 100 or less is left or 12
    /// units stand; then the auto-arrange. A rejected draw is just drawn again: when no type
    /// can be taken the original never ends; Razdor stops that side there and says so.
    pub fn random(&mut self, tag: i32, rng: &mut Rng) -> Option<RandomStop> {
        self.armies = Default::default();
        let mut stop = None;
        let count = self.catalogue.len() as i32;
        for side in 0..2 {
            let mut left = budget(tag);
            let set: &[Nature] = match rng.random(9) {
                0..=5 => &[Nature::Normal, Nature::Rogue, Nature::Hero],
                6 | 7 => &[Nature::Rogue],
                _ => &[Nature::Undead],
            };
            let ok = |id: UnitId, left: i32| {
                let d = self.content.unit(id);
                set.contains(&d.editor_nature()) && d.cost >= MIN_COST && d.cost < left
            };
            let mut ids: Vec<UnitId> = Vec::new();
            while left > BUDGET_LEFT && ids.len() < MAX_UNITS {
                if count == 0 || !self.catalogue.iter().any(|&id| ok(id, left)) {
                    stop = stop.or(Some(RandomStop { side: side as u8 + 1, left }));
                    break;
                }
                let id = self.catalogue[rng.random(count) as usize];
                if ok(id, left) {
                    ids.push(id);
                    left -= self.content.unit(id).cost;
                }
            }
            self.armies[side] = self.arranged(&ids);
        }
        self.armed = true;
        stop
    }

    /// Catalogue units of `ids`, auto-arranged (0x4f5d50) by their gold costs.
    fn arranged(&self, ids: &[UnitId]) -> Army {
        let units: Vec<Placed> = ids.iter().map(|&id| self.catalogue_unit(id, Slot::new(Row::Front, 0))).collect();
        let plain: Vec<Unit> = units.iter().map(|p| p.unit.clone()).collect();
        let squad: Vec<(usize, &Unit)> = plain.iter().enumerate().collect();
        let mut bt = Battle::with_rules(self.content.clone(), &squad, &[], Team::Player, Rules::Editor(self.switches));
        bt.set_values(Team::Player, &units.iter().map(|p| p.value).collect::<Vec<_>>());
        bt.auto_arrange(Team::Player);
        let mut army = Army { units, ..Army::default() };
        for (p, f) in army.units.iter_mut().zip(&bt.fighters) {
            p.unit.slot = f.slot;
        }
        army.cost = army.sum();
        army
    }

    /// The type number (1-based catalogue place) of a unit, 0 if its type is not there.
    fn number(&self, id: UnitId) -> usize {
        self.catalogue.iter().position(|&u| u == id).map_or(0, |k| k + 1)
    }

    /// `Battle.Sav` with both armies written into `old` (0x57e3c4): a section `Army1` and
    /// `Army2`, a key `U<row><column>` for each of the 18 cells with the type number standing
    /// there, else 0. Only the types are kept: levels, items, wounds and building defence
    /// are lost.
    pub fn to_text(&self, old: &str) -> String {
        let mut text = old.to_string();
        for (s, army) in self.armies.iter().enumerate() {
            let keys: Vec<(String, String)> = (1..=ROWS)
                .flat_map(|r| (1..=COLS).map(move |c| (r, c)))
                .map(|(r, c)| {
                    let row = [Row::Front, Row::Back, Row::Reserve][r as usize - 1];
                    let v = army.at(Slot::new(row, c - 1)).map_or(0, |k| self.number(army.units[k].unit.def));
                    (format!("U{r}{c}"), v.to_string())
                })
                .collect();
            let keys: Vec<(&str, String)> = keys.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
            text = super::options::set_keys(&text, &format!("Army{}", s + 1), &keys);
        }
        text
    }

    /// The armies of a `Battle.Sav` (0x57e77c): catalogue units rebuilt row then column,
    /// the cost totals recomputed, the start button enabled. A number past the catalogue
    /// would copy an empty template in the original; Razdor leaves that cell empty.
    pub fn from_ini(&mut self, ini: &Ini) {
        for (s, army) in self.armies.iter_mut().enumerate() {
            *army = Army::default();
            let sec = ini.section(&format!("Army{}", s + 1));
            for r in 1..=ROWS {
                for c in 1..=COLS {
                    let v = sec.map_or(0, |sec| sec.get(&format!("U{r}{c}")).map_or(0, crate::dt::ini::loose_int));
                    let Some(&id) = usize::try_from(v).ok().filter(|&v| v > 0).and_then(|v| self.catalogue.get(v - 1)) else { continue };
                    let row = [Row::Front, Row::Back, Row::Reserve][r as usize - 1];
                    let p = Placed { unit: Unit::new(&self.content, id, Slot::new(row, c - 1)), value: self.content.unit(id).cost };
                    army.units.push(p);
                }
            }
            army.cost = army.sum();
        }
        self.armed = true;
    }

    /// Saves to `Battle.Sav` in `dir` (Razdor's editor folder).
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE);
        let old = std::fs::read(&path).map(|b| crate::dt::text::decode(&b)).unwrap_or_default();
        super::files::write_atomically(&path, &crate::dt::text::encode(&self.to_text(&old)))?;
        Ok(path)
    }

    /// Loads `Battle.Sav` from `dir`; `false` when there is none (the original's message).
    pub fn load(&mut self, dir: &Path) -> bool {
        match std::fs::read(dir.join(FILE)) {
            Ok(b) => {
                self.from_ini(&Ini::from_cp1251(&b));
                true
            }
            Err(_) => false,
        }
    }

    /// Starts a battle of the two armies (0x57bc7c).
    pub fn start(&self) -> Run {
        Run::new(self)
    }
}

/// How a battle ended, for the closing message (0x57c318): which army lost, the ratio band
/// and the turn count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Closing {
    /// The bottom army lost: the AI boasts, more the stronger the player's army was.
    Boast { band: u8, turns: u32 },
    /// The top army lost: the AI's reluctant praise.
    Praise { band: u8, turns: u32 },
    /// Both still stand at the turn limit: the AI refuses to fight with such an army.
    Refusal,
}

/// The message band of `r = round(2 × a / b)` (half to even): 0 and 1 the two fixed texts,
/// 2, 3–4 and 5–99 the three with the turn count; none below 0 or from 100. A side of
/// strength 0 makes the original's division fail; Razdor shows no message then.
pub fn band(a: i64, b: i64) -> Option<u8> {
    if b == 0 {
        return None;
    }
    match experience::round_half_even(2.0 * a as f64 / b as f64) {
        0 => Some(0),
        1 => Some(1),
        2 => Some(2),
        3..=4 => Some(3),
        5..=99 => Some(4),
        _ => None,
    }
}

/// The diagnostic line of an army after a battle (LabelWarItog): the turn, the side's XP
/// pool (+0x8a8) and its HP lost against what the pre-simulation predicted (+0xc, +0x8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Debug {
    pub turn: u32,
    pub pool: i64,
    pub lost: i64,
    pub predicted: i64,
}

/// A battle in the tester. The setup is never written back: a new start replays it.
pub struct Run {
    pub battle: Battle,
    values: [Vec<i32>; 2],
    /// Both sides' strengths at the setup (side +0x89c), for the closing message.
    pub setup: [i64; 2],
    all_ai: bool,
    delay: bool,
}

const TEAMS: [Team; 2] = [Team::Player, Team::Enemy];

impl Run {
    fn new(t: &Tester) -> Run {
        let units: [Vec<Unit>; 2] = [0, 1].map(|s| t.armies[s].units.iter().map(|p| p.unit.clone()).collect());
        let values: [Vec<i32>; 2] = [0, 1].map(|s| t.armies[s].units.iter().map(|p| p.value).collect());
        let squad: Vec<(usize, &Unit)> = units[0].iter().enumerate().collect();
        let mut bt = Battle::with_rules(t.content.clone(), &squad, &units[1], Team::Player, Rules::Editor(t.switches));
        for (s, team) in TEAMS.into_iter().enumerate() {
            bt.set_values(team, &values[s]);
            bt.set_building_defence(team, t.armies[s].building_defence);
        }
        // Super AI is mode 1: side 1, the bottom army, gets the full killable test.
        bt.set_ai_level(u8::from(t.super_ai));
        bt.begin();
        let setup = TEAMS.map(|team| bt.start_of(team).strength);
        Run { battle: bt, values, setup, all_ai: t.all_ai, delay: t.delay }
    }

    pub fn over(&self) -> bool {
        self.battle.active().is_none()
    }

    /// The actor waits for the player's click: it is the bottom army's and all-AI is off.
    pub fn waits_for_click(&self) -> bool {
        !self.all_ai && self.battle.active().is_some_and(|i| self.battle.fighters[i].team == Team::Player)
    }

    /// The AI's move for the actor.
    pub fn ai_step(&mut self) {
        if self.battle.ai_step().is_none() {
            self.battle.skip();
        }
    }

    /// The player's click on `slot` of `team`'s grid: a legal cell for the actor makes it act
    /// there (its own cell a pass or self-cast, an empty own cell a step, a unit an attack or
    /// spell, the empty enemy front cell of a pull). False if the cell is not legal.
    pub fn click(&mut self, team: Team, slot: Slot) -> bool {
        let bt = &mut self.battle;
        let Some(id) = bt.active() else { return false };
        let f = &bt.fighters[id];
        if team == f.team && slot == f.slot {
            bt.own_cell();
            return true;
        }
        if let Some(t) = bt.at(team, slot) {
            return bt.act(t).is_ok();
        }
        if team == f.team {
            return bt.move_active(slot).is_ok();
        }
        match bt.pull_target(id) {
            Some(t) if slot == Slot::new(Row::Front, bt.fighters[t].slot.col) => bt.pull_active(t).is_ok(),
            _ => false,
        }
    }

    /// The closing messages (only with the delay box on): the bottom army lost (nobody left,
    /// or it surrendered), the top army lost, or neither (the turn limit) — the first two
    /// tested on their own, so both can come when both armies end empty.
    pub fn closing(&self) -> Vec<Closing> {
        let bt = &self.battle;
        if !self.delay || !self.over() {
            return Vec::new();
        }
        let gone = TEAMS.map(|t| !bt.fighters.iter().any(|f| f.team == t && f.alive()));
        let turns = bt.round;
        let mut out = Vec::new();
        if gone[0] {
            out.extend(band(self.setup[0], self.setup[1]).map(|band| Closing::Boast { band, turns }));
        }
        if gone[1] {
            out.extend(band(self.setup[1], self.setup[0]).map(|band| Closing::Praise { band, turns }));
        }
        let surrendered = matches!(bt.end_reason(), Some(EndReason::Surrender(_)));
        if !gone[0] && !gone[1] && !surrendered {
            out.push(Closing::Refusal);
        }
        out
    }

    /// An army's remaining value after the battle (LabelWCost): over its surviving units,
    /// value × HP div max HP, each term rounded down, the sum kept in 16 bits.
    pub fn remaining(&self, side: usize) -> u16 {
        let team = TEAMS[side];
        let fighters = self.battle.fighters.iter().filter(|f| f.team == team);
        fighters
            .zip(&self.values[side])
            .filter(|(f, _)| f.alive())
            .fold(0u16, |a, (f, &v)| a.wrapping_add((v as i64 * f.hp as i64 / f.max_hp().max(1) as i64) as u16))
    }

    /// The diagnostic line of army `side` once the battle is over.
    pub fn debug(&self, side: usize) -> Debug {
        let (bt, team) = (&self.battle, TEAMS[side]);
        Debug { turn: bt.round, pool: bt.pool(team), lost: bt.hp_lost(team), predicted: bt.predicted_loss(team) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::UnitDef;

    /// Types 1–6: costs 50, 5, 900, 120, 60 (undead), 30 (Rogue).
    fn content() -> Arc<Content> {
        let units = vec![
            UnitDef { cost: 50, ..warrior(1, 10, 2) },
            UnitDef { cost: 5, ..warrior(2, 5, 0) },
            UnitDef { cost: 900, ..warrior(3, 40, 5) },
            UnitDef { cost: 120, ..shooter(4, 15) },
            UnitDef { cost: 60, nature: Nature::Undead, ..warrior(5, 12, 2) },
            UnitDef { cost: 30, nature: Nature::Rogue, ..warrior(6, 8, 1) },
        ];
        Arc::new(crate::rules::content::testkit::content(units, vec![]))
    }

    fn tester() -> Tester {
        let mut t = Tester::new(content());
        t.clear();
        t
    }

    #[test]
    fn the_catalogue_counts_gold_and_the_grid_edits_only_when_armed() {
        let mut t = Tester::new(content());
        assert!(!t.click(0, Slot::new(Row::Front, 2), Some(UnitId(1))), "the start button is disabled");
        t.clear();
        assert!(t.click(0, Slot::new(Row::Front, 2), Some(UnitId(1))));
        assert!(t.click(0, Slot::new(Row::Back, 1), Some(UnitId(4))));
        assert!(!t.click(0, Slot::new(Row::Back, 0), Some(UnitId(4))), "not a cell of the pyramid");
        assert_eq!(t.armies[0].cost, 170);
        let p = &t.armies[0].units[0];
        assert_eq!((p.value, p.unit.level, p.unit.hp), (50, 1, 50));
        // A click on a unit removes it; the later ones move down a place.
        assert!(t.click(0, Slot::new(Row::Front, 2), None));
        assert_eq!((t.armies[0].cost, t.armies[0].units.len(), t.armies[0].units[0].unit.def), (120, 1, UnitId(4)));
        assert!(!t.click(0, Slot::new(Row::Front, 2), None), "an empty cell and no type");
        t.swap();
        assert_eq!((t.armies[0].units.len(), t.armies[1].cost), (0, 120));
    }

    #[test]
    fn super_ai_ticks_all_ai_and_leaves_it() {
        let mut t = tester();
        t.set_super_ai(true);
        assert!(t.all_ai);
        t.set_super_ai(false);
        assert!(t.all_ai && !t.super_ai);
    }

    #[test]
    fn random_armies_spend_the_budget_by_the_rules() {
        assert_eq!((budget(1), budget(2)), (1100, 2100));
        for seed in 0..40 {
            let mut t = tester();
            let mut rng = Rng::new(seed);
            let stop = t.random(2, &mut rng);
            for (s, army) in t.armies.iter().enumerate() {
                let natures: Vec<Nature> = army.units.iter().map(|p| t.content.unit(p.unit.def).nature).collect();
                let spent: i32 = army.units.iter().map(|p| p.value).sum();
                assert_eq!(army.cost, spent);
                assert!(army.units.len() <= MAX_UNITS && spent <= 2100);
                assert!(army.units.iter().all(|p| p.value >= 10), "type 2 costs 5");
                let one_set = natures.iter().all(|n| matches!(n, Nature::Normal | Nature::Rogue | Nature::Hero)) || natures.iter().all(|&n| n == Nature::Rogue) || natures.iter().all(|&n| n == Nature::Undead);
                assert!(one_set, "{natures:?}");
                let left = 2100 - spent;
                if stop.is_none_or(|st| st.side as usize != s + 1) {
                    assert!(left <= 100 || army.units.len() == MAX_UNITS, "seed {seed} side {s}: {left} left");
                }
                assert!(army.units.iter().all(|p| Formation::WIDE.contains(p.unit.slot)));
            }
        }
    }

    #[test]
    fn random_armies_follow_the_draws() {
        // Replay side 1 by hand: one Random(9) for the set, then Random(6) for the types,
        // each taken when of the set, at least 10 and below the budget left.
        let c = content();
        for seed in [0, 7, 1234] {
            let mut t = tester();
            t.random(1, &mut Rng::new(seed));
            let mut draws = Rng::new(seed);
            let set: Vec<Nature> = match draws.random(9) {
                0..=5 => vec![Nature::Normal, Nature::Rogue, Nature::Hero],
                6 | 7 => vec![Nature::Rogue],
                _ => vec![Nature::Undead],
            };
            let mut left = 1100;
            let mut want = Vec::new();
            while left > 100 && want.len() < 12 && c.units.iter().any(|d| set.contains(&d.nature) && d.cost >= 10 && d.cost < left) {
                let id = UnitId(draws.random(6) as u32 + 1);
                let d = c.unit(id);
                if set.contains(&d.nature) && d.cost >= 10 && d.cost < left {
                    want.push(id);
                    left -= d.cost;
                }
            }
            let mut got: Vec<UnitId> = t.armies[0].units.iter().map(|p| p.unit.def).collect();
            got.sort_by_key(|u| u.0);
            want.sort_by_key(|u| u.0);
            assert_eq!(got, want, "seed {seed}");
        }
    }

    #[test]
    fn an_undead_draw_with_nothing_cheap_enough_stops_instead_of_hanging() {
        // Only one type, undead, 600 gold: the first is taken and 500 is left, below its cost.
        let c = Arc::new(crate::rules::content::testkit::content(vec![UnitDef { cost: 600, nature: Nature::Undead, ..warrior(1, 10, 0) }], vec![]));
        let mut t = Tester::new(c);
        // Find a seed whose first roll is 8 (Undead).
        let seed = (0..1000).find(|&s| Rng::new(s).random(9) == 8).unwrap();
        let stop = t.random(1, &mut Rng::new(seed));
        assert_eq!(stop, Some(RandomStop { side: 1, left: 500 }), "600 taken, 500 left, nothing below it");
        assert_eq!(t.armies[0].units.len(), 1);
    }

    #[test]
    fn battle_sav_keeps_the_types_by_cell() {
        let mut t = tester();
        t.click(0, Slot::new(Row::Front, 0), Some(UnitId(3)));
        t.click(0, Slot::new(Row::Back, 2), Some(UnitId(1)));
        t.click(1, Slot::new(Row::Reserve, 3), Some(UnitId(6)));
        t.armies[0].units[0].unit.level = 7;
        let text = t.to_text("");
        assert!(text.starts_with("[Army1]\r\nU11=3\r\nU12=0\r\n"));
        assert!(text.contains("U23=1\r\n") && text.contains("[Army2]\r\n") && text.contains("U34=6\r\n"));
        assert_eq!(text.lines().count(), 2 * 19);
        let mut back = Tester::new(content());
        back.from_ini(&Ini::parse(&text));
        assert!(back.armed);
        let row_major: Vec<UnitId> = back.armies[0].units.iter().map(|p| p.unit.def).collect();
        assert_eq!(row_major, vec![UnitId(3), UnitId(1)]);
        assert_eq!((back.armies[0].cost, back.armies[0].units[0].unit.level), (950, 1), "only the types survive");
        // A number past the catalogue leaves its cell empty.
        back.from_ini(&Ini::parse("[Army1]\nU11=99\nU12=2\n"));
        assert_eq!(back.armies[0].units.len(), 1);
        assert!(back.armies[1].units.is_empty());
        // Saving keeps the file's other lines.
        assert!(t.to_text("[Other]\r\nX=1\r\n").starts_with("[Other]\r\nX=1\r\n[Army1]"));
    }

    #[test]
    fn the_closing_message_bands() {
        assert_eq!(band(10, 100), Some(0), "0.2 → 0");
        assert_eq!(band(30, 100), Some(1), "0.6 → 1");
        assert_eq!(band(125, 100), Some(2), "2.5 → 2, half to even");
        assert_eq!(band(175, 100), Some(3), "3.5 → 4: the 3–4 band");
        assert_eq!(band(150, 100), Some(3), "3");
        assert_eq!(band(250, 100), Some(4), "5");
        assert_eq!(band(4950, 100), Some(4), "99");
        assert_eq!(band(4975, 100), None, "99.5 → 100");
        assert_eq!(band(-30, 100), None, "−0.6 → −1");
        assert_eq!(band(-20, 100), Some(0), "−0.4 → 0");
        assert_eq!(band(10, 0), None);
    }

    #[test]
    fn a_battle_runs_under_the_editors_rules_and_reports() {
        let mut t = tester();
        t.click(0, Slot::new(Row::Front, 2), Some(UnitId(3)));
        t.click(1, Slot::new(Row::Front, 2), Some(UnitId(1)));
        t.click(1, Slot::new(Row::Front, 3), Some(UnitId(6)));
        t.all_ai = true;
        let mut run = t.start();
        assert!(matches!(run.battle.rules(), Rules::Editor(_)));
        assert_eq!(run.setup, [t.armies[0].strength(t.content()), t.armies[1].strength(t.content())]);
        assert!(!run.waits_for_click());
        for _ in 0..1000 {
            if run.over() {
                break;
            }
            run.ai_step();
        }
        assert!(run.over());
        // 900 against 50 + 30: the bottom army wins; round(2 × 80 / 900) = 0.
        assert_eq!(run.closing(), vec![Closing::Praise { band: 0, turns: run.battle.round }]);
        assert_eq!(run.remaining(1), 0);
        let hp = run.battle.fighters[0].hp;
        assert_eq!(run.remaining(0), (900 * hp / 50) as u16);
        let d = run.debug(1);
        assert_eq!((d.turn, d.lost), (run.battle.round, 100), "both its units' 50 HP");
        // No messages without the delay box.
        t.delay = false;
        let mut quiet = t.start();
        while !quiet.over() {
            quiet.ai_step();
        }
        assert!(quiet.closing().is_empty());
    }

    #[test]
    fn the_player_clicks_legal_cells_only() {
        let mut t = tester();
        t.click(0, Slot::new(Row::Front, 2), Some(UnitId(1)));
        t.click(1, Slot::new(Row::Front, 2), Some(UnitId(2)));
        t.click(1, Slot::new(Row::Front, 5), Some(UnitId(2)));
        let mut run = t.start();
        assert!(run.waits_for_click());
        assert!(!run.click(Team::Enemy, Slot::new(Row::Front, 5)), "too far");
        assert!(run.click(Team::Enemy, Slot::new(Row::Front, 2)));
        assert!(!run.waits_for_click(), "its one action is spent");
    }

    #[test]
    fn the_surrendering_side_and_the_turn_limit() {
        // Two armies that cannot hurt each other: the turn limit, and the refusal.
        let c = Arc::new(crate::rules::content::testkit::content(
            vec![UnitDef { cost: 50, hits: 1000, ..warrior(1, 0, 50) }],
            vec![],
        ));
        let mut t = Tester::new(c);
        t.clear();
        t.click(0, Slot::new(Row::Front, 2), Some(UnitId(1)));
        t.click(1, Slot::new(Row::Front, 2), Some(UnitId(1)));
        t.all_ai = true;
        let mut run = t.start();
        while !run.over() {
            run.ai_step();
        }
        assert_eq!(run.closing(), vec![Closing::Refusal]);
    }
}
