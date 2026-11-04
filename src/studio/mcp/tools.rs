//! The nine tools of the studio's MCP server: `get_state`, `check`, `schema`, `scores`, `sim`, `run_test`, `query_refs`,
//! `focus` and `snapshot`. Their schemas (`list`) and their calls (`call`). The checks and the sim are the CLI's
//! own: `check` is `build::run` and `Report::to_json` (what `razdor-pack check --json` prints), `sim`
//! is `sim::sim_map` with `--trace --json`'s options and reply. Nothing here edits a file.

use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::host::{FocusTarget, Host, SnapTarget};
use super::image;
use crate::pack::playtest::{self, Options as TestOptions};
use crate::pack::sim::{check_presets, parse_set, sim_map, Choices, SimOptions};
use crate::pack::vocab;
use crate::pack::uses::{uses, Target};
use crate::studio::build::{self, Built, Job};
use crate::studio::health::{self, Severity};
use crate::studio::map::Selection;
use crate::studio::scores::{self, Scores, Target as ScoreTarget};
use crate::studio::vars::variable_uses;

/// The longest a sim may run, in story days.
pub const MAX_DAYS: u64 = 365;

/// The most seeds a `run_test` may play for one test.
pub const MAX_SEEDS: u64 = playtest::MAX_SEEDS as u64;

/// The uses a `query_refs` answer covers, for the agent to read with the list.
const COVERS_MAP: &str = "triggers (when, if, then), dialogs' choices (if, then), the map's own victory";
const COVERS_VARIABLE: &str = "the variable's declaration in triggers/*.toml [vars]; triggers (var_changed, if, set, add, random_var) and dialog choices (if, then); not a map's victory or the campaign, which hold no variables";

/// A tool's reply: its content blocks, and whether it failed (`isError`).
struct Reply {
    content: Vec<Value>,
    is_error: bool,
}

impl Reply {
    fn ok(content: Vec<Value>) -> Reply {
        Reply { content, is_error: false }
    }

    fn json(value: &Value) -> Reply {
        Reply::ok(vec![text(serde_json::to_string_pretty(value).unwrap_or_default())])
    }

    fn fail(message: impl Into<String>) -> Reply {
        Reply { content: vec![text(message.into())], is_error: true }
    }

    fn into_value(self) -> Value {
        json!({ "content": self.content, "isError": self.is_error })
    }
}

/// A text content block.
fn text(s: impl Into<String>) -> Value {
    json!({ "type": "text", "text": s.into() })
}

/// The tools' descriptors for `tools/list`.
pub fn list() -> Value {
    json!([
        {
            "name": "get_state",
            "description": "The studio's state: the package folder, the map open on the canvas, the selected table (kind, id, cell, file, line), the files with unsaved changes, and the file that does not parse (stale), if any. Call it first, and after a focus.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "check",
            "description": "Checks the package as it is now, including unsaved edits in the studio: the errors and warnings of the package format, each with its file, line, message and hint. Run it after every edit. Errors block play; warnings do not. Same JSON as `razdor-pack check --json`.",
            "inputSchema": { "type": "object", "properties": {}, "additionalProperties": false }
        },
        {
            "name": "schema",
            "description": "The vocabulary of trigger files: every event (when), condition (if) and action (then) with its keys, their types and defaults, one sentence and one TOML example, and the names agents tried for it (aliases). Read it before writing an action you are not sure of. Same data as `razdor-pack schema --json`; a name keeps its one entry.",
            "inputSchema": {
                "type": "object",
                "properties": { "name": { "type": "string", "description": "An event, condition or action's name, e.g. `restore_army`. Default: all of them." } },
                "additionalProperties": false
            }
        },
        {
            "name": "scores",
            "description": "The map's two scores: Richness (DT's playability score, 0 and up: the curve value and its points) and Health (findings and a 0 to 100 summary; Razdor's own measure, not DT's). Each breakdown item names its targets (an army, building, point, object, hero, a cell, or a file line). Warnings are problems to look at; they do not block. Needs the game's install.",
            "inputSchema": {
                "type": "object",
                "properties": { "map": { "type": "string", "description": "The map's id (maps/<id>/). Default: the map open on the canvas." } },
                "additionalProperties": false
            }
        },
        {
            "name": "sim",
            "description": "Plays a map of the package for N story days with its triggers, dialogs and variables, headless (the AI plays, the hero does not walk), and returns the trace: each day's fired triggers, variable changes and dialogs, and the choice taken. Use it to test a story, not to test a route. Same JSON as `razdor-pack sim --trace --json`. Needs the game's install.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "map": { "type": "string", "description": "The map's id (maps/<id>/). Default: the map open on the canvas." },
                    "days": { "type": "integer", "minimum": 1, "maximum": MAX_DAYS, "default": 30, "description": "Story days to play, 1 to 365." },
                    "choices": { "type": "string", "description": "How dialogs are answered: `first` (default), `random`, or `dialog=choice` pairs separated by commas, the first choice elsewhere." },
                    "set": { "type": "array", "items": { "type": "string" }, "description": "Variables forced before the story starts, as `var=value` (a whole number, true or false)." },
                    "seed": { "type": "integer", "minimum": 0, "description": "The random seed of the story's draws. Default: the package's own." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "run_test",
            "description": "Plays the package's tests (tests/<name>.toml) headless over their seeds, as `razdor-pack test` does: a playthrough of a map or of the campaign from its start, with its steps' expectations. One result per test: its pass rate against its min_rate and, for a failing test, the first failing seed's step, line, what went wrong and the log's tail. `name` runs one test; `seeds` replaces each test's seed count. Same JSON as `razdor-pack test --json`; isError when a test misses its min_rate. Needs the game's install. A seed has 60 s and 400 game days; one past them fails with the limit it hit, and the call ends within 300 s. A failing seed's `replay` is the `razdor-pack sim` line that plays the same story.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "The test's name: its file's name without .toml. Default: every test." },
                    "seeds": { "type": "integer", "minimum": 1, "maximum": MAX_SEEDS, "description": "Seeds to play for each test, 1 to 200. Default: each test's own count." }
                },
                "additionalProperties": false
            }
        },
        {
            "name": "query_refs",
            "description": "Who refers to a name: the triggers, dialogs and victory that use an army, building or point of a map by its id (with the line of each use), or the triggers and dialogs that declare, read or write a story variable by its name.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["army", "building", "point", "variable"] },
                    "id": { "type": "string", "description": "The army, building or point id in the map, or the variable's name." },
                    "map": { "type": "string", "description": "The map's id, for army, building and point. Default: the map open on the canvas." }
                },
                "required": ["kind", "id"],
                "additionalProperties": false
            }
        },
        {
            "name": "focus",
            "description": "Points the studio at a table of a map (its selection, the canvas centred on it, the Script at its table) or opens a file at a line in the Script. The author sees what you point at.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": { "type": "string", "enum": ["hero", "army", "building", "point", "object", "file"] },
                    "id": { "type": "string", "description": "The army's or building's id." },
                    "index": { "type": "integer", "minimum": 0, "description": "The point's or object's position in its list in map.toml, from 0." },
                    "map": { "type": "string", "description": "The map's id. Default: the map open on the canvas." },
                    "path": { "type": "string", "description": "For kind `file`: the package path of the file." },
                    "line": { "type": "integer", "minimum": 1, "description": "For kind `file`: the line to put the caret on." }
                },
                "required": ["kind"],
                "additionalProperties": false
            }
        },
        {
            "name": "snapshot",
            "description": "A PNG picture of what the studio draws: the Map tab's canvas (default; the Map tab must be on screen) or the whole studio window. Use it to look at a map after a change. Scaled to 1600 px on its long side.",
            "inputSchema": {
                "type": "object",
                "properties": { "target": { "type": "string", "enum": ["canvas", "window"], "default": "canvas" } },
                "additionalProperties": false
            }
        }
    ])
}

/// Calls a tool. `None` for a tool that does not exist; otherwise its result (`content`, `isError`).
pub fn call(host: &dyn Host, name: &str, args: &Value) -> Option<Value> {
    let reply = match name {
        "get_state" => match host.state() {
            Ok(v) => Reply::json(&v),
            Err(e) => Reply::fail(e),
        },
        "check" => check(host).unwrap_or_else(Reply::fail),
        "schema" => schema(args).unwrap_or_else(Reply::fail),
        "scores" => scores(host, args).unwrap_or_else(Reply::fail),
        "sim" => sim(host, args).unwrap_or_else(Reply::fail),
        "run_test" => run_test(host, args).unwrap_or_else(Reply::fail),
        "query_refs" => query_refs(host, args).unwrap_or_else(Reply::fail),
        "focus" => focus(host, args).unwrap_or_else(Reply::fail),
        "snapshot" => snapshot(host, args).unwrap_or_else(Reply::fail),
        _ => return None,
    };
    Some(reply.into_value())
}

/// The package of a job, built with no compile (the checks and the uses read the files).
fn built(job: Job) -> Result<Built, String> {
    build::run(Job { map: None, ..job })
}

fn check(host: &dyn Host) -> Result<Reply, String> {
    let built = built(host.job()?)?;
    Ok(Reply::ok(vec![text(built.report.to_json())]))
}

/// The vocabulary of trigger files (`pack::vocab`), all of it or the entry of `name`.
fn schema(args: &Value) -> Result<Reply, String> {
    let name = optional(args, "name")?;
    Ok(Reply::json(&vocab::json(name)?))
}

/// The two scores of a map (`map`, default the shown one), as the Scores tab shows them.
fn scores(host: &dyn Host, args: &Value) -> Result<Reply, String> {
    let job = host.job()?;
    let map = match optional(args, "map")? {
        Some(m) => m.to_string(),
        None => job.map.clone().ok_or("no map is open on the canvas: pass `map`")?,
    };
    let (lang, install) = (job.lang.clone(), job.install.clone());
    let pkg = Arc::new(built(job)?.package);
    let scored = scores::compute(&scores::Job { pkg, map, lang, install }, &mut health::Cache::default())?;
    Ok(Reply::json(&scores_json(&scored)))
}

/// A score target as the agent reads it.
fn target_json(t: &ScoreTarget) -> Value {
    match t {
        ScoreTarget::Hero => json!({ "kind": "hero" }),
        ScoreTarget::Army(id) => json!({ "kind": "army", "id": id }),
        ScoreTarget::Building(id) => json!({ "kind": "building", "id": id }),
        ScoreTarget::Point(i) => json!({ "kind": "point", "index": i }),
        ScoreTarget::Object(i) => json!({ "kind": "object", "index": i }),
        ScoreTarget::Cell(x, y) => json!({ "kind": "cell", "x": x, "y": y }),
        ScoreTarget::Line { file, line } => json!({ "kind": "line", "file": file, "line": line }),
    }
}

/// Both scores as JSON: the richness (its total, the curve value, the four stages, the quests and its
/// items) or its error, and the health (the summary and its findings).
pub(super) fn scores_json(s: &Scores) -> Value {
    let richness = match &s.richness {
        Ok(r) => json!({
            "points": r.total,
            "score": r.curve_value,
            "stages": r.stages,
            "quests": r.quests,
            "items": r.items.iter().map(|i| json!({
                "term": i.term, "note": i.note, "name": i.name, "figures": i.figures,
                "points": i.points, "targets": i.targets.iter().map(target_json).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        }),
        Err(e) => json!({ "error": e }),
    };
    let findings: Vec<Value> = s
        .health
        .findings
        .iter()
        .map(|f| json!({
            "check": f.kind,
            "severity": match f.severity { Severity::Warning => "warning", Severity::Info => "info" },
            "note": f.note, "name": f.name, "figures": f.figures,
            "targets": f.targets.iter().map(target_json).collect::<Vec<_>>(),
        }))
        .collect();
    json!({ "map": s.map, "richness": richness, "health": { "summary": s.health.summary, "findings": findings } })
}

/// A string argument, required.
fn required<'a>(args: &'a Value, key: &str) -> Result<&'a str, String> {
    args.get(key).and_then(Value::as_str).filter(|s| !s.trim().is_empty()).ok_or_else(|| format!("`{key}` is required"))
}

/// A string argument, if given.
fn optional<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_str().map(Some).ok_or_else(|| format!("`{key}` must be a string")),
    }
}

/// A whole-number argument, if given.
fn number(args: &Value, key: &str) -> Result<Option<u64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v.as_u64().map(Some).ok_or_else(|| format!("`{key}` must be a whole number")),
    }
}

fn sim(host: &dyn Host, args: &Value) -> Result<Reply, String> {
    let days = number(args, "days")?.unwrap_or(30);
    if !(1..=MAX_DAYS).contains(&days) {
        return Err(format!("`days` must be between 1 and {MAX_DAYS}"));
    }
    let job = host.job()?;
    let install = job.install.clone();
    let shown = job.map.clone();
    let map = match optional(args, "map")? {
        Some(m) => m.to_string(),
        None => shown.ok_or("no map is open on the canvas: name one with `map`")?,
    };
    let built = built(job)?;
    if built.report.has_errors() {
        // The CLI stops here too: a package with errors does not play.
        return Ok(Reply { content: vec![text(built.report.to_json())], is_error: true });
    }
    let install = install.ok_or("the game's install is not found: set RAZDOR_DT_DIR, the sim plays the map on it")?;
    let package = built.package;
    let set = match args.get("set") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items.iter().map(|v| v.as_str().ok_or("`set` takes strings as var=value".to_string()).and_then(parse_set)).collect::<Result<_, _>>()?,
        Some(_) => return Err("`set` must be an array of var=value strings".into()),
    };
    check_presets(&package, &set)?;
    let choices = match optional(args, "choices")? {
        Some(spec) => Choices::parse(spec)?,
        None => Choices::First,
    };
    let opts = SimOptions { days, choices, trace: true, preset: set, seed: number(args, "seed")?.map(|s| s as u32) };
    // The story runs on its own thread: a panic in it is an answer, not a dead server.
    let worker = thread::Builder::new()
        .name("studio-sim".into())
        .spawn(move || sim_map(&package, &install, &map, &opts).map_err(|r| (map, r.to_text())))
        .map_err(|e| format!("cannot start the sim: {e}"))?;
    let outcome = worker.join().map_err(|_| "the story's sim panicked".to_string())?;
    Ok(match outcome {
        Ok(result) => Reply::ok(vec![text(Value::Array(vec![result.json()]).to_string())]),
        Err((map, errors)) => Reply {
            content: vec![text(Value::Array(vec![json!({ "map": map, "ok": false, "errors": errors })]).to_string())],
            is_error: true,
        },
    })
}

/// The package's tests, played as the CLI plays them (`playtest::run_package`) on a thread of their own.
fn run_test(host: &dyn Host, args: &Value) -> Result<Reply, String> {
    let name = optional(args, "name")?.map(str::to_string);
    let seeds = number(args, "seeds")?;
    if let Some(n) = seeds {
        if !(1..=MAX_SEEDS).contains(&n) {
            return Err(format!("`seeds` must be between 1 and {MAX_SEEDS}"));
        }
    }
    let job = host.job()?;
    let install = job.install.clone();
    let root = job.root.clone();
    let built = built(job)?;
    if built.report.has_errors() {
        // The CLI stops here too: a package with errors does not play its tests.
        return Ok(Reply { content: vec![text(built.report.to_json())], is_error: true });
    }
    let install = install.ok_or("the game's install is not found: set RAZDOR_DT_DIR, the tests play their maps on it")?;
    let package = built.package;
    // The call ends within `CALL_SECONDS`: a seed still running then fails with the reason (no hang).
    let call = Instant::now() + Duration::from_secs(playtest::CALL_SECONDS);
    let opts = TestOptions { only: name, seeds: seeds.map(|n| n as u32), deadline: Some(call) };
    // The tests run on their own thread: a panic in the engine is an answer, not a dead server.
    let worker = thread::Builder::new()
        .name("studio-tests".into())
        .spawn(move || playtest::run_package(&package, &root, &install, &opts))
        .map_err(|e| format!("cannot start the tests: {e}"))?;
    let (results, _wall) = worker.join().map_err(|_| "the tests panicked".to_string())??;
    let ok = playtest::all_ok(&results);
    Ok(Reply { content: vec![text(playtest::json(&results).to_string())], is_error: !ok })
}

fn query_refs(host: &dyn Host, args: &Value) -> Result<Reply, String> {
    let kind = required(args, "kind")?;
    let id = required(args, "id")?;
    let job = host.job()?;
    let shown = job.map.clone();
    let built = built(job)?;
    let pkg = &built.package;
    if kind == "variable" {
        let found: Vec<Value> = variable_uses(pkg, id)
            .into_iter()
            .map(|u| json!({ "role": u.role.name(), "file": u.file, "line": u.line, "what": u.what }))
            .collect();
        return Ok(Reply::json(&json!({ "kind": kind, "id": id, "uses": found, "covers": COVERS_VARIABLE })));
    }
    let target = match kind {
        "army" => Target::Army(id),
        "building" => Target::Building(id),
        "point" => Target::Point(id),
        other => return Err(format!("`kind` is army, building, point or variable, not {other}")),
    };
    let map = match optional(args, "map")? {
        Some(m) => m.to_string(),
        None => shown.ok_or("no map is open on the canvas: name one with `map`")?,
    };
    if !pkg.maps.contains_key(&map) {
        return Err(format!("the package has no map `{map}`"));
    }
    let found: Vec<Value> = uses(pkg, &map, target).into_iter().map(|u| json!({ "file": u.file, "line": u.line, "what": u.what })).collect();
    Ok(Reply::json(&json!({ "kind": kind, "id": id, "map": map, "uses": found, "covers": COVERS_MAP })))
}

fn focus(host: &dyn Host, args: &Value) -> Result<Reply, String> {
    let kind = required(args, "kind")?;
    let target = match kind {
        "file" => FocusTarget::File {
            path: required(args, "path")?.to_string(),
            line: number(args, "line")?.map(|l| l as u32),
        },
        _ => {
            let map = optional(args, "map")?.map(str::to_string);
            let index = || number(args, "index")?.map(|i| i as usize).ok_or_else(|| format!("kind `{kind}` needs `index`"));
            let sel = match kind {
                "hero" => Selection::Hero,
                "army" => Selection::Army(required(args, "id")?.to_string()),
                "building" => Selection::Building(required(args, "id")?.to_string()),
                "point" => Selection::Point(index()?),
                "object" => Selection::Object(index()?),
                other => return Err(format!("`kind` is hero, army, building, point, object or file, not {other}")),
            };
            FocusTarget::Object { map, sel }
        }
    };
    Ok(Reply::json(&host.focus(target)?))
}

fn snapshot(host: &dyn Host, args: &Value) -> Result<Reply, String> {
    let (target, name) = match optional(args, "target")?.unwrap_or("canvas") {
        "canvas" => (SnapTarget::Canvas, "canvas"),
        "window" => (SnapTarget::Window, "window"),
        other => return Err(format!("`target` is canvas or window, not {other}")),
    };
    let frame = host.snapshot(target)?;
    let (w, h, rgba) = image::fit(frame.width, frame.height, &frame.rgba, image::MAX_SIDE)?;
    let png = image::png(w, h, &rgba)?;
    Ok(Reply::ok(vec![
        text(format!("{name}: {}x{} px as a PNG of {w}x{h} px", frame.width, frame.height)),
        json!({ "type": "image", "data": image::base64(&png), "mimeType": "image/png" }),
    ]))
}
