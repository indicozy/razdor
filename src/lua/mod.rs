//! The Lua runtime of Razdor 2.0 (feature `lua`): Lua 5.4 (`mlua`, vendored) in a sandbox for
//! the scripts of a content package. A script is a file of the package's `scripts/` folder; the
//! host calls its global functions (the hooks of a replaceable system) and gets their results.
//!
//! The sandbox (`bootstrap.lua` and the setup here):
//! - the standard libraries `base` (minus `dofile`, `loadfile`, binary `load`, `collectgarbage`
//!   except `"count"`), `table`, `string` (minus `dump`), `math` and `utf8`; `io`, `os`,
//!   `package`, `debug` and `coroutine` are not loaded at all;
//! - `require` loads only `scripts/<name>.lua` of the package, from the files the host gives;
//! - `math.random` is the package's own generator, seeded by the host, never the clock;
//! - each call has an instruction budget ([`INSTRUCTION_BUDGET`]) and the state a memory limit
//!   ([`MEMORY_LIMIT`]); exceeding either is a [`ScriptError`] that names the script and the call,
//!   and a `pcall` cannot swallow it;
//! - `print` goes to a log the host reads ([`Runtime::take_log`]), not to the standard output.
//!
//! Determinism: `pairs` visits keys in an unspecified order (Lua seeds its string hashes at run
//! time); `razdor.sorted_pairs` gives a fixed order.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::{HookTriggers, Lua, LuaOptions, StdLib, Table, Value, VmState};

/// Instructions one call may run before it is stopped.
pub const INSTRUCTION_BUDGET: u64 = 5_000_000;
/// Memory one state may hold, in bytes.
pub const MEMORY_LIMIT: usize = 64 << 20;
/// The hook counts in steps of this many instructions (its granularity).
const HOOK_STEP: u32 = 1_000;
/// Lines of `print` kept until the host reads them.
const LOG_LINES: usize = 200;
/// `print` copies at most this much of each argument, of each line, and of the whole log.
const ARG_BYTES: usize = 2 << 10;
const LINE_BYTES: usize = 4 << 10;
const LOG_BYTES: usize = 64 << 10;
/// Wall-clock backstop per call. It is NOT a determinism input: a call that takes longer than
/// this is aborted with an error, and the same call run on a faster machine would finish. It
/// exists because a C library call (a pattern match, a huge `string.rep`) does not count
/// instructions, so the budget cannot stop it. The sandbox also bounds those calls by size
/// (`bootstrap.lua`); the hook here stops the next instruction after the deadline.
const TIME_LIMIT: std::time::Duration = std::time::Duration::from_secs(2);
/// Sizes the sandbox's string and table wrappers accept (`bootstrap.lua`).
pub const SUBJECT_BYTES: usize = 64 << 10;
pub const PATTERN_BYTES: usize = 256;
/// Backtracking items a pattern may have (see `backtracking` in `bootstrap.lua`).
pub const PATTERN_QUANTIFIERS: usize = 4;
/// The matcher steps a pattern may take on its subject: n^(items+1) must stay within it.
pub const MATCH_WORK: u64 = 200_000_000;
pub const REP_BYTES: usize = 1 << 20;
pub const CONCAT_BYTES: usize = 1 << 20;

const BOOTSTRAP: &str = include_str!("bootstrap.lua");

pub mod field;

/// The bounds of the sink of `print`, held by one state.
#[derive(Default)]
struct Sink {
    lines: Vec<String>,
    bytes: usize,
}

/// Cuts `s` to at most `max` bytes, on a character boundary.
fn cut(s: &mut String, max: usize) {
    if s.len() > max {
        let mut i = max;
        while !s.is_char_boundary(i) {
            i -= 1;
        }
        s.truncate(i);
    }
}

/// Keeps one line of `print` within every bound.
fn keep(sink: &mut Sink, mut line: String) {
    if sink.lines.len() >= LOG_LINES || sink.bytes >= LOG_BYTES {
        return;
    }
    cut(&mut line, LINE_BYTES);
    cut(&mut line, LOG_BYTES - sink.bytes);
    if !line.is_empty() {
        sink.bytes += line.len();
        sink.lines.push(line);
    }
}

/// A failed script: where it failed and what it said. `message` starts with the script's
/// `scripts/x.lua:LINE:` when Lua knows the line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    pub script: String,
    /// The hook or function that was called (`setup`, `require`, `load`), empty for a compile.
    pub call: String,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.call.is_empty() {
            write!(f, "{}", self.message)
        } else {
            write!(f, "{} (in {} of {})", self.message, self.call, self.script)
        }
    }
}

impl std::error::Error for ScriptError {}

/// The package's scripts: file path (`scripts/x.lua`) to its source text.
pub type Scripts = BTreeMap<String, String>;

/// A sandboxed Lua state for one package's scripts.
pub struct Runtime {
    lua: Lua,
    /// Instructions counted since the last call started.
    spent: Rc<Cell<u64>>,
    /// Set once a budget ran out, so `pcall` cannot go on after it.
    exhausted: Rc<Cell<bool>>,
    /// The generator behind `math.random`.
    rng: Rc<RefCell<SeedRng>>,
    log: Rc<RefCell<Sink>>,
    scripts: Rc<Scripts>,
    /// The wall-clock deadline of the call under way (see [`TIME_LIMIT`]).
    deadline: Rc<Cell<Option<std::time::Instant>>>,
    /// The limit each call is given (tests set it to zero).
    time_limit: Cell<std::time::Duration>,
}

/// A small seeded generator (splitmix64) for `math.random`: the same seed gives the same numbers
/// on every platform.
#[derive(Debug, Clone)]
pub struct SeedRng(u64);

impl SeedRng {
    pub fn new(seed: u64) -> SeedRng {
        SeedRng(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// The generator's state: a new generator from it continues the same numbers.
    pub fn state(&self) -> u64 {
        self.0
    }

    /// A float in `[0, 1)`.
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A whole number in `lo..=hi`.
    pub fn range(&mut self, lo: i64, hi: i64) -> i64 {
        let span = (hi as i128 - lo as i128 + 1) as u128;
        let pick = (self.next_u64() as u128) % span;
        (lo as i128 + pick as i128) as i64
    }
}

/// Text of a Lua error, without the wrappers mlua adds around errors that came from Rust.
fn message(e: &mlua::Error) -> String {
    match e {
        mlua::Error::CallbackError { cause, .. } => message(cause),
        // A traceback (added to errors raised in Rust callbacks) is not part of the message.
        mlua::Error::RuntimeError(text) => text.split("\nstack traceback:").next().unwrap_or_default().to_string(),
        mlua::Error::MemoryError(_) => format!("not enough memory (the limit is {} MB)", MEMORY_LIMIT >> 20),
        mlua::Error::SyntaxError { message, .. } => message.clone(),
        other => other.to_string(),
    }
}

fn is_exhausted(e: &mlua::Error) -> bool {
    matches!(e, mlua::Error::MemoryError(_)) || message(e).contains("instruction budget") || message(e).contains("time limit")
}

impl Runtime {
    /// A new state for `scripts`, its `math.random` seeded with `seed`.
    pub fn new(seed: u64, scripts: Scripts) -> Result<Runtime, ScriptError> {
        let fail = |e: mlua::Error| ScriptError { script: "the runtime".into(), call: String::new(), message: message(&e) };
        let lua = Lua::new_with(StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8, LuaOptions::default()).map_err(fail)?;
        lua.set_memory_limit(MEMORY_LIMIT).map_err(fail)?;
        let spent = Rc::new(Cell::new(0u64));
        let exhausted = Rc::new(Cell::new(false));
        let rng = Rc::new(RefCell::new(SeedRng::new(seed)));
        let log = Rc::new(RefCell::new(Sink::default()));
        let scripts = Rc::new(scripts);
        let deadline: Rc<Cell<Option<std::time::Instant>>> = Rc::new(Cell::new(None));

        let counter = spent.clone();
        let flag = exhausted.clone();
        let due = deadline.clone();
        lua.set_hook(HookTriggers::new().every_nth_instruction(HOOK_STEP), move |_, _| {
            let n = counter.get() + HOOK_STEP as u64;
            counter.set(n);
            if n > INSTRUCTION_BUDGET {
                flag.set(true);
                return Err(mlua::Error::runtime("instruction budget exceeded"));
            }
            if due.get().is_some_and(|d| std::time::Instant::now() > d) {
                flag.set(true);
                return Err(mlua::Error::runtime("time limit exceeded"));
            }
            Ok(VmState::Continue)
        })
        .map_err(fail)?;

        let g = lua.globals();
        // The standard library parts the sandbox removes.
        for name in ["dofile", "loadfile"] {
            g.set(name, Value::Nil).map_err(fail)?;
        }
        if let Ok(string) = g.get::<Table>("string") {
            string.set("dump", Value::Nil).map_err(fail)?;
        }
        // `math.random` and `math.randomseed`: the package's generator only.
        let math: Table = g.get("math").map_err(fail)?;
        let r = rng.clone();
        let random = lua
            .create_function(move |_, args: mlua::MultiValue| {
                let mut r = r.borrow_mut();
                let ints = |v: &Value| match v {
                    Value::Integer(i) => Some(*i),
                    Value::Number(n) if n.fract() == 0.0 => Some(*n as i64),
                    _ => None,
                };
                match args.len() {
                    0 => Ok(Value::Number(r.unit())),
                    1 | 2 => {
                        let (lo, hi) = if args.len() == 1 { (1, ints(&args[0])) } else { (ints(&args[0]).unwrap_or(i64::MIN), ints(&args[1])) };
                        let hi = hi.ok_or_else(|| mlua::Error::runtime("bad argument to 'random' (number has no integer representation)"))?;
                        let lo = if args.len() == 2 {
                            ints(&args[0]).ok_or_else(|| mlua::Error::runtime("bad argument #1 to 'random' (number has no integer representation)"))?
                        } else {
                            lo
                        };
                        if lo > hi {
                            return Err(mlua::Error::runtime("bad argument to 'random' (interval is empty)"));
                        }
                        Ok(Value::Integer(r.range(lo, hi)))
                    }
                    _ => Err(mlua::Error::runtime("wrong number of arguments to 'random'")),
                }
            })
            .map_err(fail)?;
        math.set("random", random).map_err(fail)?;
        let seeded = lua.create_function(|_, _: mlua::MultiValue| -> mlua::Result<()> {
            Err(mlua::Error::runtime("math.randomseed is not allowed: the seed comes from the game"))
        })
        .map_err(fail)?;
        math.set("randomseed", seeded).map_err(fail)?;

        // `print` into the host's log.
        let sink = log.clone();
        let print = lua
            .create_function(move |_, args: mlua::MultiValue| {
                // Each argument is copied in part only, and the arguments stop once a line is full.
                let mut line = String::new();
                for v in args.iter() {
                    if !line.is_empty() {
                        line.push('\t');
                    }
                    match v {
                        Value::String(s) => {
                            let b = s.as_bytes();
                            let n = b.len().min(ARG_BYTES);
                            line.push_str(&String::from_utf8_lossy(&b[..n]));
                            if b.len() > ARG_BYTES {
                                line.push_str("...");
                            }
                        }
                        other => line.push_str(&show(other)),
                    }
                    if line.len() >= LINE_BYTES {
                        break;
                    }
                }
                keep(&mut sink.borrow_mut(), line);
                Ok(())
            })
            .map_err(fail)?;
        g.set("print", print).map_err(fail)?;

        // `razdor`: the host's table (`sorted_pairs` comes from the bootstrap).
        let razdor = lua.create_table().map_err(fail)?;
        razdor.set("version", "2.0").map_err(fail)?;
        g.set("razdor", razdor).map_err(fail)?;

        // `require`: the package's own `scripts/<name>.lua`, once each.
        let loaded = lua.create_table().map_err(fail)?;
        let stack: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let (src, cache, active) = (scripts.clone(), loaded.clone(), stack.clone());
        let require = lua
            .create_function(move |lua, name: String| -> mlua::Result<Value> {
                let cached: Value = cache.get(name.as_str())?;
                if !cached.is_nil() {
                    return Ok(cached);
                }
                let valid = !name.is_empty() && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
                let path = format!("scripts/{name}.lua");
                let Some(text) = src.get(&path).filter(|_| valid) else {
                    return Err(mlua::Error::runtime(format!("module '{name}' not found: the package has no {path}")));
                };
                if active.borrow().contains(&name) {
                    return Err(mlua::Error::runtime(format!("circular require of '{name}'")));
                }
                active.borrow_mut().push(name.clone());
                let out = lua.load(text.as_str()).set_name(format!("@{path}")).eval::<Value>();
                active.borrow_mut().pop();
                let value = match out? {
                    Value::Nil => Value::Boolean(true),
                    v => v,
                };
                cache.set(name.as_str(), value.clone())?;
                Ok(value)
            })
            .map_err(fail)?;
        g.set("require", require).map_err(fail)?;

        // The bootstrap, with the host's checks (spent budget, passed deadline) and the bounds.
        let flag = exhausted.clone();
        let check = lua.create_function(move |_, ()| Ok(flag.get())).map_err(fail)?;
        let due = deadline.clone();
        let late = exhausted.clone();
        let overdue = lua
            .create_function(move |_, ()| {
                let over = due.get().is_some_and(|d| std::time::Instant::now() > d);
                if over {
                    late.set(true);
                }
                Ok(over)
            })
            .map_err(fail)?;
        let limits = lua.create_table().map_err(fail)?;
        for (k, v) in [("subject", SUBJECT_BYTES as u64), ("pattern", PATTERN_BYTES as u64), ("quantifiers", PATTERN_QUANTIFIERS as u64), ("rep", REP_BYTES as u64), ("concat", CONCAT_BYTES as u64), ("work", MATCH_WORK)] {
            limits.set(k, v).map_err(fail)?;
        }
        lua.load(BOOTSTRAP).set_name("@razdor/bootstrap.lua").call::<()>((check, overdue, limits)).map_err(fail)?;

        Ok(Runtime { lua, spent, exhausted, rng, log, scripts, deadline, time_limit: Cell::new(TIME_LIMIT) })
    }

    /// The Lua state, for the host's own functions (`razdor.path` and the like).
    pub fn lua(&self) -> &Lua {
        &self.lua
    }

    /// The seed generator behind `math.random`.
    pub fn rng(&self) -> std::cell::RefMut<'_, SeedRng> {
        self.rng.borrow_mut()
    }

    /// The lines `print` wrote since the last call of this method (at most 200 lines of 4 KB,
    /// 64 KB in all).
    pub fn take_log(&self) -> Vec<String> {
        let mut sink = self.log.borrow_mut();
        sink.bytes = 0;
        std::mem::take(&mut sink.lines)
    }

    /// Runs a script's main chunk (its top-level code: the functions it defines, its tables).
    pub fn load_script(&self, path: &str) -> Result<(), ScriptError> {
        let text = self.scripts.get(path).ok_or_else(|| ScriptError { script: path.into(), call: String::new(), message: format!("{path}: no such script in the package") })?;
        self.run(path, "load", || self.lua.load(text.as_str()).set_name(format!("@{path}")).exec())
    }

    /// Calls the global function `func` of `script`. `Ok(None)` when the script does not define it.
    pub fn call<R: mlua::FromLuaMulti>(&self, script: &str, func: &str, args: impl mlua::IntoLuaMulti) -> Result<Option<R>, ScriptError> {
        let f: Option<mlua::Function> = self.lua.globals().get::<Option<Value>>(func).ok().flatten().and_then(|v| match v {
            Value::Function(f) => Some(f),
            _ => None,
        });
        let Some(f) = f else { return Ok(None) };
        let args = args.into_lua_multi(&self.lua).map_err(|e| ScriptError { script: script.into(), call: func.into(), message: message(&e) })?;
        self.run(script, func, || f.call::<R>(args.clone()).map(Some))
    }

    /// Whether the script defines the global function `func`.
    pub fn defines(&self, func: &str) -> bool {
        matches!(self.lua.globals().get::<Value>(func), Ok(Value::Function(_)))
    }

    fn run<T>(&self, script: &str, call: &str, f: impl FnOnce() -> mlua::Result<T>) -> Result<T, ScriptError> {
        self.spent.set(0);
        self.exhausted.set(false);
        self.deadline.set(Some(std::time::Instant::now() + self.time_limit.get()));
        let out = f();
        self.deadline.set(None);
        out.map_err(|e| {
            let text = message(&e);
            if is_exhausted(&e) && text.contains("instruction budget") {
                ScriptError { script: script.into(), call: call.into(), message: format!("{script}: the instruction budget of {INSTRUCTION_BUDGET} instructions is spent") }
            } else if is_exhausted(&e) && text.contains("time limit") {
                ScriptError { script: script.into(), call: call.into(), message: format!("{script}: the time limit of {} s is spent", TIME_LIMIT.as_secs()) }
            } else if is_exhausted(&e) {
                ScriptError { script: script.into(), call: call.into(), message: format!("{script}: not enough memory (the limit is {} MB)", MEMORY_LIMIT >> 20) }
            } else {
                ScriptError { script: script.into(), call: call.into(), message: text }
            }
        })
    }

    /// Whether a budget or the memory has run out in this state (the sandbox stops every
    /// further call of it).
    pub fn exhausted(&self) -> bool {
        self.exhausted.get()
    }
}

fn show(v: &Value) -> String {
    match v {
        Value::Nil => "nil".into(),
        Value::Boolean(b) => b.to_string(),
        Value::Integer(i) => i.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.to_string_lossy().to_string(),
        other => other.type_name().to_string(),
    }
}

/// Compiles every script and calls nothing (`razdor-pack check`): each syntax error with its
/// `scripts/x.lua:LINE:` position, sorted by file.
pub fn check_scripts(scripts: &Scripts) -> Vec<ScriptError> {
    let lua = Lua::new_with(StdLib::NONE, LuaOptions::default());
    let Ok(lua) = lua else { return Vec::new() };
    let mut out = Vec::new();
    for (path, text) in scripts {
        if let Err(e) = lua.load(text.as_str()).set_name(format!("@{path}")).into_function() {
            out.push(ScriptError { script: path.clone(), call: String::new(), message: message(&e) });
        }
    }
    out
}

#[cfg(test)]
mod tests;
