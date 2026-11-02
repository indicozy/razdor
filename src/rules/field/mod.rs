//! Field battles (Razdor 2.0): a battle on a small board, run by a package's Lua instead of the
//! DT battle. A package picks it for one army (`battle = "lua:<script>"` on `[[army]]`); the
//! rest of the game is untouched.
//!
//! The units are DT units. Their stats, the DT damage formula ([`Battle::physical_damage`]) and
//! the battle's result all come from a DT [`Battle`] built from the same squads, the *shadow*:
//! the field keeps the cells and the turns, the shadow keeps the HP, the XP share and the end.
//! When the fight is over, the shadow is handed to [`Game::resolve_battle`] like any DT battle,
//! so deaths, experience, loot and the campaign follow the usual rules.
//!
//! Turns: each round, every living unit acts once in initiative order (higher first, then the
//! lower unit id). An action is a move up to the unit's manevres (rubble costs 2, a unit cannot
//! pass another) and then an attack: melee on an adjacent unit or a gate, or a shot at a unit
//! within its shot range with a clear line of fire.
//!
//! No macroquad and no Lua here: the Lua side is [`script::FieldScript`], implemented by
//! `src/lua/field.rs`. The rules are deterministic given the seed and the script.

pub mod board;
pub mod script;
pub mod text;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::battle::{ActionKind, Battle, Team};
use super::content::{Content, Stat};
use super::units::Unit;
use board::{Board, Tile, BATTLEMENT_RANGE};
pub use script::{Action, Damage, FieldHost, FieldScript, NoScript, Placement, Snapshot, Step, Target, TurnEdit, UnitView};
pub use text::LogLine;

/// The turn limit when the script does not set one.
pub const DEFAULT_MAX_TURNS: u32 = 12;
/// Lines of the log kept in the save (the battle view shows the last ones).
const LOG_LINES: usize = 500;
/// How far a shooter reaches, in tiles, along a clear line of fire (a battlement adds
/// [`BATTLEMENT_RANGE`]).
pub const SHOT_RANGE: i32 = 6;
/// The board of a battle the script does not lay out: 12 × 8 open ground.
const DEFAULT_W: i32 = 12;
const DEFAULT_H: i32 = 8;

/// Which side of the field a unit is on. The defenders are the hero's side (the shadow's
/// `Team::Player`), the attackers the other.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Defenders,
    Attackers,
}

impl Side {
    pub fn of(team: Team) -> Side {
        match team {
            Team::Player => Side::Defenders,
            Team::Enemy => Side::Attackers,
        }
    }

    pub fn team(self) -> Team {
        match self {
            Side::Defenders => Team::Player,
            Side::Attackers => Team::Enemy,
        }
    }

    pub fn other(self) -> Side {
        match self {
            Side::Defenders => Side::Attackers,
            Side::Attackers => Side::Defenders,
        }
    }
}

/// What a unit has counted so far: what the shadow keeps, copied into the save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub hp: i32,
    pub taken: i32,
    pub useful: i32,
    pub lost: i32,
    pub actions: i32,
}

/// A field battle's state, saved with the game (`Game::field`). The shadow is rebuilt from the
/// squads on a load ([`FieldBattle::resume`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FieldState {
    /// The script (`scripts/<script>.lua`); empty for the Rust rules alone.
    pub script: String,
    pub seed: u64,
    pub board: Board,
    /// Each unit's cell by fighter index (`None`: not on the board).
    pub cells: Vec<Option<(i32, i32)>>,
    pub counts: Vec<Counts>,
    pub round: u32,
    pub max_turns: u32,
    /// This round's acting order (fighter indices) and the next one to act.
    pub order: Vec<usize>,
    pub cursor: usize,
    /// The script's variables (`razdor.vars`).
    pub vars: BTreeMap<String, serde_json::Value>,
    pub log: Vec<LogLine>,
    pub outcome: Option<Side>,
    /// A script that failed stops for the rest of the battle; the error is kept for the report.
    pub script_off: bool,
    pub script_error: Option<String>,
    /// The script's generator state at the last sync.
    pub script_rng: u64,
}

/// A field battle in play: the state and its shadow DT battle.
pub struct FieldBattle {
    pub shadow: Battle,
    pub state: FieldState,
}

/// The legal moves of one unit this turn (see [`FieldBattle::moves`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Moves {
    /// Cells the unit can walk to, its own included, row by row.
    pub cells: Vec<(i32, i32)>,
    /// Units it can hit from one of those cells (their ids).
    pub units: Vec<usize>,
    /// Standing gates next to one of those cells.
    pub gates: Vec<(i32, i32)>,
}

/// The shadow DT battle of a fight: the squad (its living units, the hero always) against the
/// enemy units, begun as a DT battle begins (the shadow's pre-simulation is off).
pub fn shadow(content: Arc<Content>, squad: &[Unit], enemies: &[Unit]) -> Battle {
    let player: Vec<(usize, &Unit)> = squad.iter().enumerate().filter(|(i, u)| *i == 0 || u.alive()).collect();
    let mut b = Battle::new(content, &player, enemies, Team::Enemy);
    b.skip_prediction();
    b.auto_arrange(Team::Enemy);
    b.begin();
    b
}

/// The shadow of a fight with explicit sides: `player` (the squad's units with their squad
/// index), `joiners` (garrison troops that fight for the hero, with no squad index: they are
/// given the index `usize::MAX` here and the caller clears it) and `enemies`. `attacker` is the
/// side that attacks (the DT initiative bonus); the enemy is arranged anew, as DT does.
pub fn shadow_of(content: Arc<Content>, player: &[(usize, &Unit)], joiners: &[Unit], enemies: &[Unit], attacker: Team) -> Battle {
    let mut all: Vec<(usize, &Unit)> = player.to_vec();
    all.extend(joiners.iter().map(|u| (usize::MAX, u)));
    let mut b = Battle::new(content, &all, enemies, attacker);
    for f in b.fighters.iter_mut() {
        if f.squad_index == Some(usize::MAX) {
            f.squad_index = None;
        }
    }
    b.skip_prediction();
    b.auto_arrange(Team::Enemy);
    b.begin();
    b
}

/// How far a unit shoots, in tiles: none without an attack of shot, else [`SHOT_RANGE`].
pub fn shot_range(b: &Battle, f: usize) -> i32 {
    if b.fighters[f].base[Stat::AttackShot] <= 0 {
        0
    } else {
        SHOT_RANGE
    }
}

impl FieldBattle {
    /// A new battle on `shadow`: the script lays out the board and the units (setup), the
    /// rest goes to the default cells.
    pub fn new(shadow: Battle, script: &str, seed: u64, hooks: &mut dyn FieldScript) -> FieldBattle {
        let n = shadow.fighters.len();
        let board = Board::open(DEFAULT_W, DEFAULT_H).expect("the default board fits");
        let state = FieldState {
            script: script.to_string(),
            seed,
            board,
            cells: vec![None; n],
            counts: vec![Counts::default(); n],
            round: 0,
            max_turns: DEFAULT_MAX_TURNS,
            order: Vec::new(),
            cursor: 0,
            vars: BTreeMap::new(),
            log: Vec::new(),
            outcome: None,
            script_off: false,
            script_error: None,
            script_rng: seed,
        };
        let mut fb = FieldBattle { shadow, state };
        fb.setup(hooks);
        fb
    }

    /// The battle of a saved state, with the shadow rebuilt from the squads as the battle began.
    pub fn resume(mut shadow: Battle, state: FieldState) -> Result<FieldBattle, String> {
        if state.counts.len() != shadow.fighters.len() || state.cells.len() != shadow.fighters.len() {
            return Err("the field battle does not fit the squads of its save".into());
        }
        for (f, c) in shadow.fighters.iter_mut().zip(&state.counts) {
            f.hp = c.hp;
            f.taken = c.taken;
            f.useful = c.useful;
            f.lost = c.lost;
            f.actions = c.actions;
        }
        Ok(FieldBattle { shadow, state })
    }

    /// Copies the shadow's counts and the script's generator into the state, for a save.
    pub fn sync(&mut self, hooks: &dyn FieldScript) {
        for (c, f) in self.state.counts.iter_mut().zip(&self.shadow.fighters) {
            *c = Counts { hp: f.hp, taken: f.taken, useful: f.useful, lost: f.lost, actions: f.actions };
        }
        self.state.script_rng = hooks.rng_state();
    }

    pub fn outcome(&self) -> Option<Side> {
        self.state.outcome
    }

    pub fn round(&self) -> u32 {
        self.state.round
    }

    /// The end: the shadow with the losing side fallen, ready for [`Game::resolve_battle`]. `None`
    /// while the battle is undecided.
    pub fn finish(mut self) -> Option<Battle> {
        let winner = self.state.outcome?;
        self.shadow.field_end(winner.team());
        self.shadow.round = self.state.round;
        self.shadow.log.extend(self.state.log.iter().map(text::english));
        Some(self.shadow)
    }

    fn fighters(&self) -> usize {
        self.shadow.fighters.len()
    }

    fn side(&self, f: usize) -> Side {
        Side::of(self.shadow.fighters[f].team)
    }

    fn alive_on_board(&self, f: usize) -> bool {
        self.shadow.fighters[f].alive() && self.state.cells[f].is_some()
    }

    fn label(&self, f: usize) -> String {
        let n = &self.shadow.fighters[f].name;
        if n.is_empty() {
            format!("unit {f}")
        } else {
            n.clone()
        }
    }

    /// Adds a line of the log: a text key and its arguments (see [`text`]).
    fn say(&mut self, key: &str, args: &[String]) {
        if self.state.log.len() < LOG_LINES {
            self.state.log.push(LogLine { key: key.to_string(), args: args.to_vec() });
        }
    }

    /// Where the others stand: the cells of the living units, except `f`'s own.
    fn blocked_except(&self, f: usize) -> Vec<(i32, i32)> {
        (0..self.fighters()).filter(|&i| i != f && self.alive_on_board(i)).filter_map(|i| self.state.cells[i]).collect()
    }

    fn occupied(&self, x: i32, y: i32) -> bool {
        (0..self.fighters()).any(|i| self.alive_on_board(i) && self.state.cells[i] == Some((x, y)))
    }

    /// Calls a hook of the script, if it still runs. An error ends the battle as a defenders'
    /// win; the log and the save name the error.
    fn hook<T>(&mut self, hooks: &mut dyn FieldScript, active: Option<usize>, call: impl FnOnce(&mut dyn FieldScript, &mut Snapshot) -> Result<Option<T>, String>) -> Option<T> {
        if self.state.script_off {
            return None;
        }
        let mut snap = self.snapshot(active);
        let out = call(hooks, &mut snap);
        self.state.vars = std::mem::take(&mut snap.vars);
        for line in hooks.take_log() {
            self.say("field.print", &[line]);
        }
        match out {
            Ok(v) => v,
            Err(e) => {
                // A failing script ends the battle as a defenders' win: the error is in the log and
                // in the save, and nothing panics.
                self.say("field.script_error", std::slice::from_ref(&e));
                self.state.script_off = true;
                self.state.script_error = Some(e);
                self.set_outcome(Side::Defenders);
                None
            }
        }
    }

    /// The hooks' view of the battle now.
    pub fn snapshot(&self, active: Option<usize>) -> Snapshot {
        let units = (0..self.fighters())
            .map(|i| {
                let f = &self.shadow.fighters[i];
                UnitView {
                    id: i,
                    side: self.side(i),
                    name: f.name.clone(),
                    level: f.level,
                    hp: f.hp,
                    max_hp: f.max_hp(),
                    attack_blow: f.base[Stat::AttackBlow],
                    attack_shot: f.base[Stat::AttackShot],
                    defence_blow: f.stats[Stat::DefenceBlow],
                    defence_shot: f.stats[Stat::DefenceShot],
                    initiative: f.stats[Stat::Initiative],
                    manevres: f.stats[Stat::Manevres],
                    shot_range: shot_range(&self.shadow, i),
                    at: self.state.cells[i],
                    alive: f.alive(),
                }
            })
            .collect();
        Snapshot { board: self.state.board.clone(), round: self.state.round, max_turns: self.state.max_turns, units, active, vars: self.state.vars.clone() }
    }

    fn setup(&mut self, hooks: &mut dyn FieldScript) {
        if let Some(p) = self.hook(hooks, None, |h, s| h.setup(s)) {
            for (x, y, t) in p.tiles {
                self.state.board.set(x, y, t);
            }
            if let Some(m) = p.max_turns {
                self.state.max_turns = m.clamp(1, 99);
            }
            for (f, x, y) in p.place {
                let ok = f < self.fighters() && self.state.board.get(x, y).is_some_and(|t| t.cost().is_some()) && !self.occupied(x, y);
                if ok {
                    self.state.cells[f] = Some((x, y));
                } else {
                    self.say("field.place_default", &[f.to_string(), format!("{x},{y}")]);
                }
            }
        }
        self.default_place();
    }

    /// The cells of the units the script did not place: defenders in the two left columns,
    /// attackers in the two right ones, top to bottom, then any free open cell.
    fn default_place(&mut self) {
        let (w, h) = (self.state.board.w, self.state.board.h);
        for side in [Side::Defenders, Side::Attackers] {
            let cols = if side == Side::Defenders { [0, 1] } else { [w - 1, w - 2] };
            let mut slots: Vec<(i32, i32)> = cols.iter().flat_map(|&x| (0..h).map(move |y| (x, y))).collect();
            slots.extend((0..h).flat_map(|y| (0..w).map(move |x| (x, y))));
            let todo: Vec<usize> = (0..self.fighters()).filter(|&f| self.side(f) == side && self.state.cells[f].is_none()).collect();
            for f in todo {
                let free = slots.iter().copied().find(|&(x, y)| self.state.board.get(x, y).is_some_and(|t| t.cost().is_some()) && !self.occupied(x, y));
                match free {
                    Some(c) => self.state.cells[f] = Some(c),
                    None => {
                        let name = self.label(f);
                        self.say("field.no_cell", &[name]);
                    }
                }
            }
        }
    }

    /// Starts the next round: the units' order, then the script's turn-start hook.
    pub fn start_round(&mut self, hooks: &mut dyn FieldScript) {
        if self.state.outcome.is_some() {
            return;
        }
        self.state.round += 1;
        self.shadow.round = self.state.round;
        let mut order: Vec<usize> = (0..self.fighters()).filter(|&f| self.alive_on_board(f)).collect();
        order.sort_by_key(|&f| (-self.shadow.fighters[f].stats[Stat::Initiative], f));
        self.state.order = order;
        self.state.cursor = 0;
        self.say("field.turn", &[self.state.round.to_string()]);
        if let Some(edit) = self.hook(hooks, None, |h, s| h.on_turn_start(s)) {
            self.apply_tiles(&edit.tiles);
            for key in edit.log {
                self.say(&key, &[]);
            }
        }
        self.check_end(hooks);
    }

    fn apply_tiles(&mut self, tiles: &[(i32, i32, Tile)]) {
        for &(x, y, t) in tiles {
            self.state.board.set(x, y, t);
        }
    }

    /// The next unit to act this round, `None` when the round is over.
    pub fn current(&mut self) -> Option<usize> {
        while self.state.cursor < self.state.order.len() {
            let f = self.state.order[self.state.cursor];
            if self.alive_on_board(f) {
                return Some(f);
            }
            self.state.cursor += 1;
        }
        None
    }

    /// The action of `f` by the script's `ai_turn`, else the Rust AI's.
    pub fn ai_action(&mut self, f: usize, hooks: &mut dyn FieldScript) -> Action {
        if let Some(a) = self.hook(hooks, Some(f), |h, s| h.ai_turn(s)) {
            return a;
        }
        self.default_action(f)
    }

    /// Plays `f`'s action: the move first (each step asked of `can_enter`), then the attack.
    /// An invalid part is logged and left out; the unit's turn is used all the same.
    pub fn act(&mut self, f: usize, action: Action, hooks: &mut dyn FieldScript) -> Result<(), String> {
        if self.current() != Some(f) {
            return Err(format!("unit {f} is not the one to act"));
        }
        self.shadow.fighters[f].actions += 1;
        let mut at = self.state.cells[f].expect("a unit on the board has a cell");
        if let Some(to) = action.move_to {
            at = self.walk(f, to, hooks);
        }
        if let Some(t) = action.attack {
            self.strike(f, at, t, hooks);
        }
        self.state.cursor += 1;
        self.check_end(hooks);
        Ok(())
    }

    /// Moves `f` along the cheapest path toward `to` as far as its manevres reach and the
    /// script allows. Returns the cell it stands on.
    fn walk(&mut self, f: usize, to: (i32, i32), hooks: &mut dyn FieldScript) -> (i32, i32) {
        let from = self.state.cells[f].expect("a unit on the board has a cell");
        if to == from {
            return from;
        }
        let blocked = self.blocked_except(f);
        let budget = self.shadow.fighters[f].stats[Stat::Manevres];
        // A cell off the board (a script's) or one with a unit on it is no move: the unit waits.
        if blocked.contains(&to) {
            let name = self.label(f);
            self.say("field.occupied", &[name, format!("{},{}", to.0, to.1)]);
            return from;
        }
        let Some(path) = self.state.board.path(from, to, &blocked) else {
            let name = self.label(f);
            self.say("field.cannot_reach", &[name, format!("{},{}", to.0, to.1)]);
            return from;
        };
        let mut spent = 0;
        let mut here = from;
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            let Some(tile) = self.state.board.get(b.0, b.1) else { break };
            let Some(cost) = tile.cost() else { break };
            if spent + cost > budget {
                break;
            }
            let step = Step { unit: f, from: a, to: b, tile };
            if self.hook(hooks, Some(f), |h, s| h.can_enter(s, &step).map(|v| v.map(|allowed| (allowed,)))).is_some_and(|(allowed,)| !allowed) {
                let name = self.label(f);
                self.say("field.stopped", &[name, format!("{},{}", a.0, a.1)]);
                break;
            }
            spent += cost;
            here = b;
        }
        if here != from {
            self.state.cells[f] = Some(here);
            let name = self.label(f);
            self.say("field.moves", &[name, format!("{},{}", here.0, here.1)]);
        }
        here
    }

    /// Whether `f`, standing on `at`, can shoot or strike the unit `t`.
    fn reaches(&self, f: usize, at: (i32, i32), t: usize) -> bool {
        let Some(tc) = self.state.cells[t] else { return false };
        if !self.alive_on_board(t) || self.side(f) == self.side(t) {
            return false;
        }
        let d = Board::distance(at, tc);
        if d == 1 {
            return true;
        }
        let range = shot_range(&self.shadow, f);
        if range == 0 {
            return false;
        }
        let bonus = if self.state.board.get(at.0, at.1) == Some(Tile::Battlement) { BATTLEMENT_RANGE } else { 0 };
        d <= range + bonus && self.state.board.line_of_fire(at, tc)
    }

    fn strike(&mut self, f: usize, at: (i32, i32), target: Target, hooks: &mut dyn FieldScript) {
        match target {
            Target::Unit(t) => {
                if t >= self.fighters() || !self.reaches(f, at, t) {
                    let name = self.label(f);
                    self.say("field.no_target", &[name]);
                    return;
                }
                let shot = Board::distance(at, self.state.cells[t].expect("a reached unit is on the board")) > 1;
                let kind = if shot { ActionKind::Shot } else { ActionKind::Melee };
                self.shadow.round = self.state.round;
                let raw = self.shadow.physical_damage(f, t, kind).max(0);
                let d = Damage { attacker: f, target: t, shot, amount: raw };
                let amount = self.hook(hooks, Some(f), |h, s| h.on_damage(s, &d)).unwrap_or(raw).max(0);
                let hp = self.shadow.fighters[t].hp.max(0);
                let dealt = amount.min(hp);
                self.shadow.fighters[t].hp = hp - dealt;
                self.shadow.fighters[t].taken += dealt;
                self.shadow.fighters[f].useful += 1;
                let (a, b) = (self.label(f), self.label(t));
                let key = if shot { "field.shoots" } else { "field.strikes" };
                self.say(key, &[a, b.clone(), dealt.to_string()]);
                if self.shadow.fighters[t].hp <= 0 {
                    self.say("field.falls", &[b]);
                }
            }
            Target::Tile(x, y) => {
                let adjacent = Board::distance(at, (x, y)) == 1;
                let Some(Tile::Gate { hp }) = self.state.board.get(x, y) else {
                    let name = self.label(f);
                    self.say("field.no_gate", &[name, format!("{x},{y}")]);
                    return;
                };
                if !adjacent || hp <= 0 {
                    let name = self.label(f);
                    self.say("field.gate_unreached", &[name, format!("{x},{y}")]);
                    return;
                }
                let blow = self.shadow.fighters[f].base[Stat::AttackBlow].max(1);
                let left = hp - blow;
                let name = self.label(f);
                if left <= 0 {
                    self.state.board.set(x, y, Tile::Rubble);
                    self.say("field.gate_broken", &[name, format!("{x},{y}")]);
                } else {
                    self.state.board.set(x, y, Tile::Gate { hp: left });
                    self.say("field.gate_hit", &[name, format!("{x},{y}"), left.to_string()]);
                }
            }
        }
    }

    /// The end of a fight by the rules alone: a side with nobody standing loses. Then the
    /// script's `check_end`.
    fn check_end(&mut self, hooks: &mut dyn FieldScript) {
        if self.state.outcome.is_some() {
            return;
        }
        let standing = |side: Side, fb: &FieldBattle| (0..fb.fighters()).any(|f| fb.side(f) == side && fb.alive_on_board(f));
        let winner = if !standing(Side::Defenders, self) {
            Some(Side::Attackers)
        } else if !standing(Side::Attackers, self) {
            Some(Side::Defenders)
        } else {
            self.hook(hooks, None, |h, s| h.check_end(s))
        };
        if let Some(w) = winner {
            self.set_outcome(w);
        }
    }

    fn set_outcome(&mut self, w: Side) {
        self.state.outcome = Some(w);
        let key = match w {
            Side::Defenders => "field.defenders_hold",
            Side::Attackers => "field.attackers_hold",
        };
        self.say(key, &[]);
    }

    /// The end of a round. At the turn limit the script's `check_end` decides (it was asked just
    /// now); if it says nothing, the defenders win.
    pub fn end_round(&mut self, hooks: &mut dyn FieldScript) {
        self.check_end(hooks);
        if self.state.outcome.is_none() && self.state.round >= self.state.max_turns {
            let max = self.state.max_turns.to_string();
            self.say("field.turn_limit", &[max]);
            self.set_outcome(Side::Defenders);
        }
    }

    /// Plays the rest of the battle with every unit on the AI (the script's or the Rust one).
    pub fn play_auto(&mut self, hooks: &mut dyn FieldScript) -> Option<Side> {
        while self.state.outcome.is_none() {
            // A round begun before a save goes on where it stopped; a new one starts otherwise.
            if self.state.round == 0 || self.state.cursor >= self.state.order.len() {
                self.start_round(hooks);
            }
            while let Some(f) = self.current() {
                let a = self.ai_action(f, hooks);
                if self.act(f, a, hooks).is_err() {
                    break;
                }
                if self.state.outcome.is_some() {
                    break;
                }
            }
            self.end_round(hooks);
        }
        self.state.outcome
    }

    /// What `f` can do this turn from where it stands: the cells it can walk to with its
    /// manevres (its own cell included), the units it can hit from one of them, and the gates
    /// next to them it can break. The rules only: a script's `can_enter` is asked when it walks.
    pub fn moves(&self, f: usize) -> Moves {
        let Some(at) = self.state.cells[f] else { return Moves::default() };
        if !self.shadow.fighters[f].alive() {
            return Moves::default();
        }
        let budget = self.shadow.fighters[f].stats[Stat::Manevres];
        let blocked = self.blocked_except(f);
        let costs = self.state.board.costs(at, &blocked);
        let mut out = Moves::default();
        for y in 0..self.state.board.h {
            for x in 0..self.state.board.w {
                if costs[(y * self.state.board.w + x) as usize].is_some_and(|c| c <= budget) {
                    out.cells.push((x, y));
                }
            }
        }
        for &c in &out.cells {
            for t in 0..self.fighters() {
                if self.reaches(f, c, t) && !out.units.contains(&t) {
                    out.units.push(t);
                }
            }
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let g = (c.0 + dx, c.1 + dy);
                if matches!(self.state.board.get(g.0, g.1), Some(Tile::Gate { hp: 1.. })) && !out.gates.contains(&g) {
                    out.gates.push(g);
                }
            }
        }
        out
    }

    /// The units that act this round, and the one to act now (`None`: none left).
    pub fn order(&self) -> &[usize] {
        &self.state.order
    }

    /// The default AI: a unit that can strike or shoot a unit from where it stands does so (the
    /// lowest HP first); a shooter with no one in reach holds; a melee unit walks to the nearest
    /// enemy and strikes; blocked by a gate, it strikes the gate.
    pub fn default_action(&self, f: usize) -> Action {
        let at = self.state.cells[f].expect("a unit on the board has a cell");
        if let Some(t) = self.best_target(f, at) {
            return Action { move_to: None, attack: Some(Target::Unit(t)) };
        }
        if shot_range(&self.shadow, f) > 0 {
            return Action::default();
        }
        let blocked = self.blocked_except(f);
        let costs = self.state.board.costs(at, &blocked);
        let (w, h) = (self.state.board.w, self.state.board.h);
        let mut goal: Option<((i32, i32), i32)> = None;
        for e in (0..self.fighters()).filter(|&e| self.side(e) != self.side(f) && self.alive_on_board(e)) {
            let ec = self.state.cells[e].expect("a living unit on the board has a cell");
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let g = (ec.0 + dx, ec.1 + dy);
                if g.0 < 0 || g.1 < 0 || g.0 >= w || g.1 >= h {
                    continue;
                }
                if let Some(c) = costs[(g.1 * w + g.0) as usize] {
                    if goal.is_none_or(|(bg, bc)| (c, g) < (bc, bg)) {
                        goal = Some((g, c));
                    }
                }
            }
        }
        let Some((g, _)) = goal else {
            // Blocked: a standing gate next to the unit is the thing to break.
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let c = (at.0 + dx, at.1 + dy);
                if matches!(self.state.board.get(c.0, c.1), Some(Tile::Gate { hp: 1.. })) {
                    return Action { move_to: None, attack: Some(Target::Tile(c.0, c.1)) };
                }
            }
            return Action::default();
        };
        let budget = self.shadow.fighters[f].stats[Stat::Manevres];
        let mut dest = at;
        if let Some(path) = self.state.board.path(at, g, &blocked) {
            let mut spent = 0;
            for pair in path.windows(2) {
                let Some(cost) = self.state.board.get(pair[1].0, pair[1].1).and_then(Tile::cost) else { break };
                if spent + cost > budget {
                    break;
                }
                spent += cost;
                dest = pair[1];
            }
        }
        Action { move_to: (dest != at).then_some(dest), attack: self.best_target(f, dest).map(Target::Unit) }
    }

    /// The enemy `f` can reach from `at` with the lowest HP (then the lowest id).
    fn best_target(&self, f: usize, at: (i32, i32)) -> Option<usize> {
        (0..self.fighters()).filter(|&t| self.reaches(f, at, t)).min_by_key(|&t| (self.shadow.fighters[t].hp, t))
    }
}

#[cfg(test)]
mod tests;
