//! The hooks of a field battle, as plain data. A package's Lua (`src/lua/field.rs`) implements
//! [`FieldScript`]; the rules never see Lua. Every hook is optional: `None` (or the trait's
//! default) keeps the Rust rule. A hook that fails stops the script for the rest of the battle
//! and the Rust rules go on (the battle never stalls on a script).

use std::collections::BTreeMap;

use super::board::{Board, Tile};
use super::Side;

/// A unit's view for the scripts (read only: a hook returns its changes, it does not write them).
/// `id` is the unit's index in the battle: 0 to n-1, defenders and attackers together.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitView {
    pub id: usize,
    pub side: Side,
    pub name: String,
    pub level: i32,
    pub hp: i32,
    pub max_hp: i32,
    pub attack_blow: i32,
    pub attack_shot: i32,
    pub defence_blow: i32,
    pub defence_shot: i32,
    pub initiative: i32,
    pub manevres: i32,
    /// The farthest shot in tiles (0: no shots), without a battlement's bonus.
    pub shot_range: i32,
    /// The cell, `None` while the unit is not on the board.
    pub at: Option<(i32, i32)>,
    pub alive: bool,
}

/// What a hook sees: the board, the units, the turn, and the script's variables (`razdor.vars`).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub board: Board,
    pub round: u32,
    pub max_turns: u32,
    pub units: Vec<UnitView>,
    /// The unit whose action is being decided, when there is one.
    pub active: Option<usize>,
    /// The package's variables: numbers, strings and booleans. A hook may change them; the
    /// battle keeps the result and the save holds it.
    pub vars: BTreeMap<String, serde_json::Value>,
}

/// An action of one unit: move (within its manevres), then attack.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Action {
    pub move_to: Option<(i32, i32)>,
    pub attack: Option<Target>,
}

/// What a unit attacks: a unit (by its id) or a gate on a cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Unit(usize),
    Tile(i32, i32),
}

/// `setup`: the board's tiles and the units' cells. Anything not placed gets the default cell.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    pub tiles: Vec<(i32, i32, Tile)>,
    pub place: Vec<(usize, i32, i32)>,
    /// The turn limit, when the script sets its own (default 12).
    pub max_turns: Option<u32>,
}

/// `on_turn_start`: tiles that change at the start of a turn, and lines for the battle log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TurnEdit {
    pub tiles: Vec<(i32, i32, Tile)>,
    pub log: Vec<String>,
}

/// `on_damage`: the blow or shot about to land, with the damage the DT formula gives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Damage {
    pub attacker: usize,
    pub target: usize,
    pub shot: bool,
    pub amount: i32,
}

/// `can_enter`: a unit about to step from one cell to the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Step {
    pub unit: usize,
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub tile: Tile,
}

/// The package's hooks. Each returns `Ok(None)` (or the default) when it has nothing to say;
/// `Err` carries the script's error text.
pub trait FieldScript {
    fn setup(&mut self, _s: &mut Snapshot) -> Result<Option<Placement>, String> {
        Ok(None)
    }
    fn on_turn_start(&mut self, _s: &mut Snapshot) -> Result<Option<TurnEdit>, String> {
        Ok(None)
    }
    /// The damage a blow or shot will do, or `None` to keep the DT formula's.
    fn on_damage(&mut self, _s: &mut Snapshot, _d: &Damage) -> Result<Option<i32>, String> {
        Ok(None)
    }
    /// `Some(false)` refuses the step.
    fn can_enter(&mut self, _s: &mut Snapshot, _m: &Step) -> Result<Option<bool>, String> {
        Ok(None)
    }
    /// Which side has won, or `None` to play on. Called after each action.
    fn check_end(&mut self, _s: &mut Snapshot) -> Result<Option<Side>, String> {
        Ok(None)
    }
    /// The action of an AI unit (`s.active`), or `None` for the Rust AI.
    fn ai_turn(&mut self, _s: &mut Snapshot) -> Result<Option<Action>, String> {
        Ok(None)
    }
    /// The seed state of the script's generator, saved with the battle.
    fn rng_state(&self) -> u64 {
        0
    }
    /// The lines the script printed since the last call.
    fn take_log(&mut self) -> Vec<String> {
        Vec::new()
    }
}

/// No script: every hook keeps the Rust rule.
pub struct NoScript;

impl FieldScript for NoScript {}

/// Where a battle's hooks come from: the package's Lua (`src/lua/field.rs`, feature `lua`). The
/// rules name no Lua; a game holds a host (`Game::field_host`) and opens the hooks of each
/// battle, and again on a load. Lua globals do not survive a load: a script keeps its state in
/// `vars` (saved) or in the generator.
pub trait FieldHost: Send + Sync {
    /// The hooks of `script` (`scripts/<script>.lua`), with its generator at `rng`. `Err` when the
    /// script is missing or does not load: the battle then does not open (the caller says so).
    fn open(&self, script: &str, rng: u64) -> Result<Box<dyn FieldScript>, String>;
}
