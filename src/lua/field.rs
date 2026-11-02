//! The field battle hooks of a package's scripts (`rules::field`), over the sandboxed Lua state
//! of [`super::Runtime`]. A battle's script is `scripts/<name>.lua`; the battle calls its global
//! functions, each optional:
//!
//! - `setup(s)` → nil or `{ tiles = {{x,y,tile[,hp]}…}, place = {{unit,x,y}…}, max_turns = n }`
//! - `on_turn_start(s)` → nil or `{ tiles = {…}, log = {"text.key", …} }`
//! - `on_damage(s, d)` → nil or the damage (a whole number)
//! - `can_enter(s, m)` → nil or a boolean (false refuses the step)
//! - `check_end(s)` → nil or `"defenders"` / `"attackers"`
//! - `ai_turn(s)` → nil or `{ move = {x,y}, attack = {unit = id} | {gate = {x,y}} }`
//!
//! `s` is the battle's view (read only, see `docs/reference/rzpack-format.md`, «Field
//! battles»). Any table a hook returns may also carry `vars = {name = value}`: the battle keeps
//! those package variables. A bad return, or a Lua error, is an error of the battle: it ends as
//! a defenders' win with the error in its log (`rules::field`).
//!
//! The view's read-only tables are proxies: a write to them is a Lua error.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;

use mlua::{Function, Lua, Table, Value};

use super::{Runtime, Scripts};
use crate::rules::field::board::{Board, Tile};
use crate::rules::field::{Action, Damage, FieldHost, FieldScript, Placement, Side, Snapshot, Step, Target, TurnEdit, UnitView};

/// What the helpers `razdor.path`, `razdor.distance` and `razdor.line_of_fire` read: the board
/// now, and the cells units stand on (a path goes around them).
#[derive(Default)]
struct View {
    board: Option<Board>,
    blocked: Vec<(i32, i32)>,
}

type Shared = Rc<RefCell<View>>;

/// The hooks of the package's scripts, opened per battle (`Game::field_host`).
pub struct Host {
    scripts: Arc<Scripts>,
}

impl Host {
    pub fn new(scripts: Arc<Scripts>) -> Host {
        Host { scripts }
    }
}

impl FieldHost for Host {
    fn open(&self, script: &str, rng: u64) -> Result<Box<dyn FieldScript>, String> {
        let path = format!("scripts/{script}.lua");
        if !self.scripts.contains_key(&path) {
            return Err(format!("the package has no {path}"));
        }
        LuaField::open(&path, (*self.scripts).clone(), rng).map(|f| Box::new(f) as Box<dyn FieldScript>)
    }
}

/// The hooks of one battle's script, in its own Lua state.
pub struct LuaField {
    rt: Runtime,
    path: String,
    view: Shared,
}

impl LuaField {
    /// The script at `path` loaded in a new state seeded with `rng` (its generator's state).
    pub fn open(path: &str, scripts: Scripts, rng: u64) -> Result<LuaField, String> {
        let rt = Runtime::new(rng, scripts).map_err(|e| e.to_string())?;
        let view: Shared = Rc::new(RefCell::new(View::default()));
        install_helpers(rt.lua(), view.clone()).map_err(|e| e.to_string())?;
        rt.load_script(path).map_err(|e| e.to_string())?;
        Ok(LuaField { rt, path: path.to_string(), view })
    }

    /// Calls `func` with the view and `extra`, and returns what it gave (nil as `Value::Nil`).
    /// The view's package variables are the snapshot's, and the view of the helpers is set.
    fn call(&mut self, s: &mut Snapshot, func: &str, extra: impl FnOnce(&Lua) -> mlua::Result<Value>) -> Result<Value, String> {
        if !self.rt.defines(func) {
            return Ok(Value::Nil);
        }
        {
            let mut v = self.view.borrow_mut();
            v.board = Some(s.board.clone());
            v.blocked = s.units.iter().filter(|u| u.alive).filter_map(|u| u.at).collect();
        }
        let lua = self.rt.lua().clone();
        let fail = |e: mlua::Error| e.to_string();
        let ctx = ctx_table(&lua, s).map_err(fail)?;
        let vars = vars_table(&lua, &s.vars).map_err(fail)?;
        lua.globals().get::<Table>("razdor").and_then(|r| r.set("vars", vars)).map_err(fail)?;
        let extra = extra(&lua).map_err(fail)?;
        let out: Option<Value> = self.rt.call(&self.path, func, (ctx, extra)).map_err(|e| e.to_string())?;
        Ok(out.unwrap_or(Value::Nil))
    }

    /// The table a hook returned, or `None` for nil; anything else is an error.
    fn table_of(&self, func: &str, v: Value) -> Result<Option<Table>, String> {
        match v {
            Value::Nil => Ok(None),
            Value::Table(t) => Ok(Some(t)),
            other => Err(format!("{}: {func} returned a {}, not a table or nil", self.path, other.type_name())),
        }
    }
}

impl FieldScript for LuaField {
    fn setup(&mut self, s: &mut Snapshot) -> Result<Option<Placement>, String> {
        let v = self.call(s, "setup", |_| Ok(Value::Nil))?;
        let Some(t) = self.table_of("setup", v)? else { return Ok(None) };
        let mut p = Placement::default();
        for e in seq(&t, "tiles")? {
            p.tiles.push(tile_entry(&e, &self.path)?);
        }
        for e in seq(&t, "place")? {
            let Value::Table(e) = e else { return Err(format!("{}: setup: a place is {{unit, x, y}}", self.path)) };
            let unit = int_field(&e, "unit", &self.path)?;
            if unit < 0 {
                return Err(format!("{}: setup: place.unit must not be negative", self.path));
            }
            p.place.push((unit as usize, int_field(&e, "x", &self.path)?, int_field(&e, "y", &self.path)?));
        }
        p.max_turns = match t.get::<Value>("max_turns").map_err(|e| e.to_string())? {
            Value::Nil => None,
            _ => Some(int_field(&t, "max_turns", &self.path)?.max(1) as u32),
        };
        take_vars(&t, s, &self.path)?;
        Ok(Some(p))
    }

    fn on_turn_start(&mut self, s: &mut Snapshot) -> Result<Option<TurnEdit>, String> {
        let v = self.call(s, "on_turn_start", |_| Ok(Value::Nil))?;
        let Some(t) = self.table_of("on_turn_start", v)? else { return Ok(None) };
        let mut edit = TurnEdit::default();
        for e in seq(&t, "tiles")? {
            edit.tiles.push(tile_entry(&e, &self.path)?);
        }
        for e in seq(&t, "log")? {
            match e {
                Value::String(k) => edit.log.push(k.to_str().map_err(|e| e.to_string())?.to_string()),
                other => return Err(format!("{}: on_turn_start: log lines are text keys, not a {}", self.path, other.type_name())),
            }
        }
        take_vars(&t, s, &self.path)?;
        Ok(Some(edit))
    }

    fn on_damage(&mut self, s: &mut Snapshot, d: &Damage) -> Result<Option<i32>, String> {
        let d = *d;
        let v = self.call(s, "on_damage", move |lua| {
            let t = lua.create_table()?;
            t.set("attacker", d.attacker as i64)?;
            t.set("target", d.target as i64)?;
            t.set("shot", d.shot)?;
            t.set("amount", d.amount)?;
            Ok(Value::Table(t))
        })?;
        match v {
            Value::Nil => Ok(None),
            other => match whole(&other) {
                Some(n) => Ok(Some(n)),
                None => Err(format!("{}: on_damage returned a {}, not a whole number", self.path, other.type_name())),
            },
        }
    }

    fn can_enter(&mut self, s: &mut Snapshot, m: &Step) -> Result<Option<bool>, String> {
        let m = *m;
        let v = self.call(s, "can_enter", move |lua| {
            let t = lua.create_table()?;
            t.set("unit", m.unit as i64)?;
            t.set("from", cell_table(lua, m.from)?)?;
            t.set("to", cell_table(lua, m.to)?)?;
            t.set("tile", tile_name(m.tile))?;
            Ok(Value::Table(t))
        })?;
        match v {
            Value::Nil => Ok(None),
            Value::Boolean(b) => Ok(Some(b)),
            other => Err(format!("{}: can_enter returned a {}, not a boolean or nil", self.path, other.type_name())),
        }
    }

    fn check_end(&mut self, s: &mut Snapshot) -> Result<Option<Side>, String> {
        let v = self.call(s, "check_end", |_| Ok(Value::Nil))?;
        match v {
            Value::Nil => Ok(None),
            Value::String(w) => match w.to_str().map_err(|e| e.to_string())?.as_ref() {
                "defenders" => Ok(Some(Side::Defenders)),
                "attackers" => Ok(Some(Side::Attackers)),
                other => Err(format!("{}: check_end returned \"{other}\": use \"defenders\", \"attackers\" or nil", self.path)),
            },
            other => Err(format!("{}: check_end returned a {}, not a string or nil", self.path, other.type_name())),
        }
    }

    fn ai_turn(&mut self, s: &mut Snapshot) -> Result<Option<Action>, String> {
        let v = self.call(s, "ai_turn", |_| Ok(Value::Nil))?;
        let Some(t) = self.table_of("ai_turn", v)? else { return Ok(None) };
        let mut a = Action::default();
        if let Some(m) = opt_table(&t, "move", &self.path)? {
            a.move_to = Some((int_field(&m, "x", &self.path)?, int_field(&m, "y", &self.path)?));
        }
        if let Some(at) = opt_table(&t, "attack", &self.path)? {
            a.attack = Some(match (opt_table(&at, "gate", &self.path)?, at.get::<Value>("unit").map_err(|e| e.to_string())?) {
                (Some(g), _) => Target::Tile(int_field(&g, "x", &self.path)?, int_field(&g, "y", &self.path)?),
                (None, Value::Nil) => return Err(format!("{}: ai_turn: attack needs a unit or a gate", self.path)),
                (None, _) => {
                    let u = int_field(&at, "unit", &self.path)?;
                    if u < 0 {
                        return Err(format!("{}: ai_turn: attack.unit must not be negative", self.path));
                    }
                    Target::Unit(u as usize)
                }
            });
        }
        take_vars(&t, s, &self.path)?;
        Ok(Some(a))
    }

    fn rng_state(&self) -> u64 {
        self.rt.rng().state()
    }

    fn take_log(&mut self) -> Vec<String> {
        self.rt.take_log()
    }
}

/// Checks a package's script without calling anything (`razdor-pack check`): it loads, and
/// defines the functions a battle needs (`setup` and `check_end`). Errors name the line.
pub fn check_battle_script(scripts: &Scripts, script: &str) -> Result<(), String> {
    let path = format!("scripts/{script}.lua");
    if !scripts.contains_key(&path) {
        return Err(format!("the package has no {path}"));
    }
    let rt = Runtime::new(0, scripts.clone()).map_err(|e| e.to_string())?;
    rt.load_script(&path).map_err(|e| e.to_string())?;
    let missing: Vec<&str> = ["setup", "check_end"].into_iter().filter(|f| !rt.defines(f)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!("{path} does not define {}", missing.join(" and ")))
    }
}

/// Installs `razdor.path`, `razdor.distance` and `razdor.line_of_fire`, which read the view.
fn install_helpers(lua: &Lua, view: Shared) -> mlua::Result<()> {
    let razdor: Table = lua.globals().get("razdor")?;
    let v = view.clone();
    razdor.set(
        "path",
        lua.create_function(move |lua, (from, to): (Value, Value)| {
            let (from, to) = (cell_of(&from)?, cell_of(&to)?);
            let v = v.borrow();
            let Some(board) = v.board.as_ref() else { return Ok(Value::Nil) };
            let Some(path) = board.path(from, to, &v.blocked) else { return Ok(Value::Nil) };
            let out = lua.create_table()?;
            for (i, c) in path.into_iter().enumerate() {
                out.raw_set(i + 1, cell_table(lua, c)?)?;
            }
            Ok(Value::Table(out))
        })?,
    )?;
    razdor.set(
        "distance",
        lua.create_function(|_, (a, b): (Value, Value)| Ok(Board::distance(cell_of(&a)?, cell_of(&b)?) as i64))?,
    )?;
    let v = view;
    razdor.set(
        "line_of_fire",
        lua.create_function(move |_, (a, b): (Value, Value)| {
            let (a, b) = (cell_of(&a)?, cell_of(&b)?);
            let v = v.borrow();
            Ok(v.board.as_ref().is_some_and(|board| board.line_of_fire(a, b)))
        })?,
    )?;
    Ok(())
}

/// A cell from a Lua value: `{x = n, y = n}`, whole numbers on the board's scale.
fn cell_of(v: &Value) -> mlua::Result<(i32, i32)> {
    let Value::Table(t) = v else {
        return Err(mlua::Error::runtime(format!("a cell is {{x = n, y = n}}, not a {}", v.type_name())));
    };
    let get = |k: &str| -> mlua::Result<i32> {
        whole(&t.get::<Value>(k)?).ok_or_else(|| mlua::Error::runtime(format!("a cell's {k} is not a whole number")))
    };
    Ok((get("x")?, get("y")?))
}

/// A whole number from a Lua value (an integer, or a float with no fraction in range); NaN and
/// infinities are not.
fn whole(v: &Value) -> Option<i32> {
    match v {
        Value::Integer(i) => i32::try_from(*i).ok(),
        Value::Number(n) if n.is_finite() && n.fract() == 0.0 && n.abs() <= i32::MAX as f64 => Some(*n as i32),
        _ => None,
    }
}

fn int_field(t: &Table, key: &str, path: &str) -> Result<i32, String> {
    let v = t.get::<Value>(key).map_err(|e| e.to_string())?;
    whole(&v).ok_or_else(|| format!("{path}: `{key}` is not a whole number"))
}

/// The entries of a sequence field (nil: none).
fn seq(t: &Table, key: &str) -> Result<Vec<Value>, String> {
    match t.get::<Value>(key).map_err(|e| e.to_string())? {
        Value::Nil => Ok(Vec::new()),
        Value::Table(list) => {
            let mut out = Vec::new();
            for i in 1..=list.raw_len() {
                out.push(list.raw_get::<Value>(i).map_err(|e| e.to_string())?);
            }
            Ok(out)
        }
        other => Err(format!("`{key}` is a {}, not a list", other.type_name())),
    }
}

fn opt_table(t: &Table, key: &str, path: &str) -> Result<Option<Table>, String> {
    match t.get::<Value>(key).map_err(|e| e.to_string())? {
        Value::Nil => Ok(None),
        Value::Table(x) => Ok(Some(x)),
        other => Err(format!("{path}: `{key}` is a {}, not a table", other.type_name())),
    }
}

/// A tile entry `{x, y, tile = "name", hp = n}` of a hook's list.
fn tile_entry(v: &Value, path: &str) -> Result<(i32, i32, Tile), String> {
    let Value::Table(t) = v else { return Err(format!("{path}: a tile is {{x, y, tile}}, not a {}", v.type_name())) };
    let (x, y) = (int_field(t, "x", path)?, int_field(t, "y", path)?);
    let name = match t.get::<Value>("tile").map_err(|e| e.to_string())? {
        Value::String(s) => s.to_str().map_err(|e| e.to_string())?.to_string(),
        _ => return Err(format!("{path}: a tile needs its `tile` name")),
    };
    let tile = match name.as_str() {
        "open" => Tile::Open,
        "wall" => Tile::Wall,
        "battlement" => Tile::Battlement,
        "rubble" => Tile::Rubble,
        "gate" => {
            let hp = int_field(t, "hp", path)?;
            if hp < 1 {
                return Err(format!("{path}: a gate needs hp of at least 1"));
            }
            Tile::Gate { hp }
        }
        other => return Err(format!("{path}: no tile \"{other}\" (open, wall, gate, battlement, rubble)")),
    };
    Ok((x, y, tile))
}

fn tile_name(t: Tile) -> &'static str {
    match t {
        Tile::Open => "open",
        Tile::Wall => "wall",
        Tile::Gate { .. } => "gate",
        Tile::Battlement => "battlement",
        Tile::Rubble => "rubble",
    }
}

/// A `{x = n, y = n}` table.
fn cell_table(lua: &Lua, c: (i32, i32)) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("x", c.0 as i64)?;
    t.set("y", c.1 as i64)?;
    Ok(t)
}

/// The `vars` a hook returned, merged into the snapshot's (kept by the battle).
fn take_vars(t: &Table, s: &mut Snapshot, path: &str) -> Result<(), String> {
    let Value::Table(v) = t.get::<Value>("vars").map_err(|e| e.to_string())? else { return Ok(()) };
    for pair in v.pairs::<Value, Value>() {
        let (k, val) = pair.map_err(|e| e.to_string())?;
        let Value::String(k) = k else { return Err(format!("{path}: vars are named by strings")) };
        let json = match val {
            Value::Boolean(b) => serde_json::Value::Bool(b),
            Value::Integer(i) => serde_json::Value::from(i),
            Value::Number(n) if n.is_finite() => serde_json::Number::from_f64(n).map(serde_json::Value::Number).unwrap_or(serde_json::Value::Null),
            Value::String(s) => serde_json::Value::String(s.to_str().map_err(|e| e.to_string())?.to_string()),
            other => return Err(format!("{path}: a variable is a number, a string or a boolean, not a {}", other.type_name())),
        };
        s.vars.insert(k.to_str().map_err(|e| e.to_string())?.to_string(), json);
    }
    Ok(())
}

/// The battle's view for a hook: `{round, max_turns, active, board, units, vars}`, read only.
fn ctx_table(lua: &Lua, s: &Snapshot) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("round", s.round as i64)?;
    t.set("max_turns", s.max_turns as i64)?;
    if let Some(a) = s.active {
        t.set("active", a as i64)?;
    }
    let b = lua.create_table()?;
    b.set("w", s.board.w as i64)?;
    b.set("h", s.board.h as i64)?;
    let tiles = lua.create_table()?;
    let gates = lua.create_table()?;
    for (i, tile) in s.board.tiles.iter().enumerate() {
        tiles.raw_set(i + 1, tile_name(*tile))?;
        if let Tile::Gate { hp } = tile {
            let g = lua.create_table()?;
            g.set("x", (i as i32 % s.board.w) as i64)?;
            g.set("y", (i as i32 / s.board.w) as i64)?;
            g.set("hp", *hp as i64)?;
            gates.raw_set(gates.raw_len() + 1, g)?;
        }
    }
    b.set("tiles", tiles)?;
    b.set("gates", gates)?;
    t.set("board", b)?;
    let units = lua.create_table()?;
    for (i, u) in s.units.iter().enumerate() {
        units.raw_set(i + 1, unit_table(lua, u)?)?;
    }
    t.set("units", units)?;
    t.set("vars", vars_table(lua, &s.vars)?)?;
    read_only(lua, t)
}

fn unit_table(lua: &Lua, u: &UnitView) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    t.set("id", u.id as i64)?;
    t.set("side", match u.side {
        Side::Defenders => "defenders",
        Side::Attackers => "attackers",
    })?;
    t.set("name", u.name.as_str())?;
    t.set("level", u.level as i64)?;
    t.set("hp", u.hp as i64)?;
    t.set("max_hp", u.max_hp as i64)?;
    t.set("attack_blow", u.attack_blow as i64)?;
    t.set("attack_shot", u.attack_shot as i64)?;
    t.set("defence_blow", u.defence_blow as i64)?;
    t.set("defence_shot", u.defence_shot as i64)?;
    t.set("initiative", u.initiative as i64)?;
    t.set("manevres", u.manevres as i64)?;
    t.set("shot_range", u.shot_range as i64)?;
    if let Some(at) = u.at {
        t.set("at", cell_table(lua, at)?)?;
    }
    t.set("alive", u.alive)?;
    Ok(t)
}

fn vars_table(lua: &Lua, vars: &BTreeMap<String, serde_json::Value>) -> mlua::Result<Table> {
    let t = lua.create_table()?;
    for (k, v) in vars {
        let val = match v {
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => match n.as_i64() {
                Some(i) => Value::Integer(i),
                None => Value::Number(n.as_f64().unwrap_or(0.0)),
            },
            serde_json::Value::String(s) => Value::String(lua.create_string(s)?),
            _ => continue,
        };
        t.set(k.as_str(), val)?;
    }
    read_only(lua, t)
}

/// A read-only proxy of `t`: nested tables are proxies too, reads and `pairs` and `#` work, a
/// write is a Lua error.
fn read_only(lua: &Lua, t: Table) -> mlua::Result<Table> {
    for pair in t.pairs::<Value, Value>() {
        let (k, v) = pair?;
        if let Value::Table(inner) = v {
            t.raw_set(k, read_only(lua, inner)?)?;
        }
    }
    let mt = lua.create_table()?;
    mt.set("__index", t.clone())?;
    mt.set("__newindex", lua.create_function(|_, _: mlua::MultiValue| -> mlua::Result<()> {
        Err(mlua::Error::runtime("the battle's view is read only"))
    })?)?;
    let raw = t.clone();
    mt.set("__len", lua.create_function(move |_, _: Value| Ok(raw.raw_len() as i64))?)?;
    let next: Function = lua.globals().get("next")?;
    let inner = t.clone();
    mt.set("__pairs", lua.create_function(move |_, _: Value| Ok((next.clone(), inner.clone(), Value::Nil)))?)?;
    let proxy = lua.create_table()?;
    proxy.set_metatable(Some(mt))?;
    Ok(proxy)
}

#[cfg(test)]
mod tests;
