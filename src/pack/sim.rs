//! `razdor-pack sim`: plays a package's map headless with the AI for some days, the way the
//! community map smoke test plays the install's maps (`rules::sim`). A panic is caught and
//! reported; a softlock (the clock stops) is reported too. The package's story runs with the map:
//! its dialogs are answered by a choice rule (`--choices`), and the run's trace is kept for
//! `--trace` (the event debugger: what fired, which variables changed, what the sim chose).

use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::sync::Arc;

use super::compile;
use super::diag::Report;
use super::load::Package;
use super::open;
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::content::{Content, HeroClass};
use crate::rules::game::Game;
use crate::rules::sim::{self, FieldFight, MapEnd, StoryPlay};
use crate::rules::triggers::{Ending, PendingDialog, Trace};

use super::open::Opened;

/// How the sim answers a dialog (`--choices`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Choices {
    /// The first choice that shows; «Далее» when there is none. The default.
    First,
    /// A seeded rule over the choices that show (the same answers every run).
    Random,
    /// The choice named for each dialog given (`dialog=choice`); the first choice elsewhere.
    Forced(Vec<(String, String)>),
}

impl Choices {
    /// Reads `--choices`: `first`, `random`, or `dialog=choice,dialog=choice,…`.
    pub fn parse(text: &str) -> Result<Choices, String> {
        match text.trim() {
            "first" => Ok(Choices::First),
            "random" => Ok(Choices::Random),
            spec => {
                let mut pairs = Vec::new();
                for part in spec.split(',').map(str::trim).filter(|p| !p.is_empty()) {
                    let (dialog, choice) = part.split_once('=').ok_or_else(|| format!("--choices: `{part}` is not dialog=choice (or first, or random)"))?;
                    pairs.push((dialog.trim().to_string(), choice.trim().to_string()));
                }
                if pairs.is_empty() {
                    return Err("--choices needs first, random, or dialog=choice,…".into());
                }
                Ok(Choices::Forced(pairs))
            }
        }
    }

    /// The choice taken in dialog `d`: a forced one if it shows, else the rule's (`state` is the
    /// rule's seeded generator for `random`). `None` for «Далее», a dialog with no choice shown.
    pub fn pick(&self, state: &mut u32, d: &PendingDialog) -> Option<String> {
        if d.choices.is_empty() {
            return None;
        }
        if let Choices::Forced(pairs) = self {
            if let Some((_, c)) = pairs.iter().find(|(dialog, _)| *dialog == d.id) {
                if d.choices.contains(c) {
                    return Some(c.clone());
                }
            }
        }
        match self {
            Choices::Random => {
                // xorshift32: a fixed seed, so a run repeats.
                *state ^= *state << 13;
                *state ^= *state >> 17;
                *state ^= *state << 5;
                Some(d.choices[*state as usize % d.choices.len()].clone())
            }
            _ => Some(d.choices[0].clone()),
        }
    }
}

/// What a sim run does: its days, its dialog choices, and whether it keeps the story's trace.
#[derive(Clone, Debug)]
pub struct SimOptions {
    pub days: u64,
    pub choices: Choices,
    pub trace: bool,
    /// Variables forced at the start (`--set var=value`).
    pub preset: Vec<(String, VarValue)>,
    /// The story's seed (`--seed`): the package generator's and the field battles' draws. None:
    /// the package's own (from its hash).
    pub seed: Option<u32>,
}

/// Reads `--set var=value`: a whole number, or `true` / `false`.
pub fn parse_set(spec: &str) -> Result<(String, VarValue), String> {
    let (name, value) = spec.split_once('=').ok_or_else(|| format!("--set: `{spec}` is not var=value"))?;
    let name = name.trim();
    let value = value.trim();
    let value = match value {
        "true" => VarValue::Bool(true),
        "false" => VarValue::Bool(false),
        v => VarValue::Int(v.parse().map_err(|_| format!("--set {name}={v}: a whole number, true or false"))?),
    };
    Ok((name.to_string(), value))
}

/// The `--set` variables must be declared by the package, with the value's type.
pub fn check_presets(pkg: &Package, presets: &[(String, VarValue)]) -> Result<(), String> {
    for (name, value) in presets {
        let declared = pkg.vars.iter().find(|v| v.value.name == *name).ok_or_else(|| format!("--set {name}: no variable `{name}` is declared"))?;
        let ok = matches!((&declared.value.default, value), (VarValue::Int(_), VarValue::Int(_)) | (VarValue::Bool(_), VarValue::Bool(_)));
        if !ok {
            return Err(format!("--set {name}: `{name}` takes {}", if matches!(declared.value.default, VarValue::Int(_)) { "a whole number" } else { "true or false" }));
        }
    }
    Ok(())
}

/// What a simulation of one map saw.
#[derive(Clone, Debug, PartialEq)]
pub struct SimResult {
    pub map: String,
    pub days: u64,
    /// The panic's message, if the simulation panicked.
    pub panic: Option<String>,
    /// The clock did not reach the end: a softlock.
    pub stalled: bool,
    /// Days after the start when the victory fired, if it did (the scenario's or the story's).
    pub victory_day: Option<f64>,
    /// Days after the start when the hero fell or the defeat fired, if it did.
    pub defeat_day: Option<f64>,
    pub ai_battles: u32,
    pub player_battles: usize,
    /// The field battles of the run (see [`crate::rules::sim::FieldFight`]).
    pub field_battles: Vec<crate::rules::sim::FieldFight>,
    /// The map has a story (triggers or dialogs).
    pub story: bool,
    /// What the story did, by day after the start, when the trace was asked for.
    pub trace: Vec<(u64, Trace)>,
    pub seconds: f64,
}

impl SimResult {
    /// No panic and no softlock: the exit status of `razdor-pack sim` for this map.
    pub fn ok(&self) -> bool {
        self.panic.is_none() && !self.stalled
    }

    /// One line for a person.
    pub fn text(&self) -> String {
        let outcome = match (self.victory_day, self.defeat_day) {
            (Some(v), _) => format!("victory on day {v:.1}"),
            (None, Some(d)) => format!("defeat on day {d:.1}"),
            (None, None) => "no victory or defeat".to_string(),
        };
        let status = match (&self.panic, self.stalled) {
            (Some(p), _) => format!("PANIC: {p}"),
            (None, true) => "STALLED (softlock)".to_string(),
            (None, false) => "ok".to_string(),
        };
        let mut out = format!(
            "{}: {status}; {} days, {outcome}; AI battles {}, player battles {}; {:.2} s",
            self.map, self.days, self.ai_battles, self.player_battles, self.seconds
        );
        for (i, f) in self.field_battles.iter().enumerate() {
            out.push_str(&format!("\n  {}", field_text(i + 1, f)));
        }
        out
    }

    /// The trace for a person: every day of the run, with what the story did that day.
    pub fn trace_text(&self) -> String {
        let mut out = String::new();
        for day in 0..self.days {
            let entries: Vec<String> = self.trace.iter().filter(|(d, _)| *d == day).map(|(_, what)| describe(what)).collect();
            if entries.is_empty() {
                out.push_str(&format!("day {day}: (nothing)\n"));
            }
            for (k, line) in entries.iter().enumerate() {
                let head = if k == 0 { format!("day {day}: ") } else { " ".repeat(format!("day {day}: ").len()) };
                out.push_str(&format!("{head}{line}\n"));
            }
        }
        out
    }

    /// The result as JSON (`razdor-pack sim --json`); the trace as data, one object per step.
    pub fn json(&self) -> serde_json::Value {
        let trace: Vec<serde_json::Value> = self.trace.iter().map(|(day, what)| serde_json::json!({"day": day, "what": what})).collect();
        serde_json::json!({
            "map": self.map,
            "ok": self.ok(),
            "days": self.days,
            "panic": self.panic,
            "stalled": self.stalled,
            "victory_day": self.victory_day,
            "defeat_day": self.defeat_day,
            "ai_battles": self.ai_battles,
            "player_battles": self.player_battles,
            "field_battles": self.field_battles.iter().map(|f| serde_json::json!({
                "rounds": f.rounds,
                "winner": f.winner.map(|w| match w { crate::rules::field::Side::Defenders => "defenders", crate::rules::field::Side::Attackers => "attackers" }),
                "defenders_hp": [f.defenders_hp.0, f.defenders_hp.1],
                "defenders_max": f.defenders_max,
                "gates": f.gates.iter().map(|((x, y), a, b)| serde_json::json!({"x": x, "y": y, "start": a, "end": b})).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "story": self.story,
            "trace": trace,
            "seconds": self.seconds,
        })
    }
}

/// One field battle, for a person: its rounds and winner, the defenders' HP lost, the gates.
pub fn field_text(n: usize, f: &FieldFight) -> String {
    let who = match f.winner {
        Some(crate::rules::field::Side::Defenders) => "defenders",
        Some(crate::rules::field::Side::Attackers) => "attackers",
        None => "no winner",
    };
    let (before, after) = f.defenders_hp;
    let lost = if before > 0 { 100 * (before - after).max(0) / before } else { 0 };
    let mut out = format!("field battle {n}: {} rounds, {who} win; defenders HP {before} -> {after} of {} ({lost}% lost)", f.rounds, f.defenders_max);
    for ((x, y), hp0, hp) in &f.gates {
        out.push_str(&format!("; gate ({x},{y}) {hp0} -> {hp}"));
    }
    out
}

/// A developer switch of the campaign (`razdor-pack sim --meet <map>:<army>[@x,y] --squad …`): on
/// `map`, the hero's squad becomes `squad` (the hero and those troops), he stands in the building
/// on the cell `at`, and the army `army` (its index in the map's `[[army]]` list) is his foe, so
/// their field battle is fought with the army he carries. A measurement, not a story.
#[derive(Clone, Debug, PartialEq)]
pub struct Meet {
    pub map: String,
    pub army: usize,
    pub at: Option<(i32, i32)>,
    /// The troops of the squad after the hero, as DT unit aliases and counts (`dt:militiaman`).
    pub squad: Vec<(String, u32)>,
}

/// Sets the hero's squad, his building and the foe of `m` (see [`Meet`]).
fn meet(g: &mut Game, content: &Content, m: &Meet) -> Result<(), String> {
    use crate::pack::aliases;
    use crate::pack::refs::DtKind;
    use crate::rules::formation::{Row, Slot};
    use crate::rules::game::Foe;
    use crate::rules::units::Unit;
    if m.army >= g.world.armies.len() {
        return Err(format!("--meet {}: the map has {} armies", m.map, g.world.armies.len()));
    }
    g.squad.truncate(1);
    let mut i = 0usize;
    for (alias, n) in &m.squad {
        let key = alias.strip_prefix("dt:").unwrap_or(alias);
        let id = aliases::resolve(DtKind::Unit, key).ok_or_else(|| format!("--squad: no unit `{alias}`"))?;
        for _ in 0..*n {
            let slot = if i < 3 { Slot::new(Row::Front, i as u8) } else { Slot::new(Row::Back, (i - 3) as u8) };
            g.squad.push(Unit::new(content, crate::rules::content::UnitId(id), slot));
            i += 1;
        }
    }
    if let Some((x, y)) = m.at {
        g.location = Some(g.world.location_covering((x, y)).ok_or_else(|| format!("--meet {}: no building covers {x},{y}", m.map))?);
    }
    g.foe = Some(Foe::Army(m.army));
    Ok(())
}

/// One step of the trace, for a person.
fn describe(what: &Trace) -> String {
    match what {
        Trace::Fired { trigger, moment } => format!("fired `{trigger}` on {moment}"),
        Trace::Var { name, value } => format!("{name} = {}", value_text(value)),
        Trace::Dialog { id, choices } if choices.is_empty() => format!("dialog `{id}` opens (Далее)"),
        Trace::Dialog { id, choices } => format!("dialog `{id}` opens: {}", choices.join(", ")),
        Trace::Answer { dialog, choice: Some(c) } => format!("the sim answers `{dialog}` with `{c}`"),
        Trace::Answer { dialog, choice: None } => format!("the sim answers `{dialog}` with Далее"),
        Trace::Spawn { army } => format!("army `{army}` comes onto the map"),
        Trace::Goal { army, goal } if goal == "clear" => format!("army `{army}` goes back to the original AI"),
        Trace::Goal { army, goal } => format!("army `{army}` takes the goal {goal}"),
        Trace::Arrive { army, cell: (x, y), building: Some(b) } => format!("army `{army}` arrives in `{b}` at [{x}, {y}]"),
        Trace::Arrive { army, cell: (x, y), building: None } => format!("army `{army}` arrives at [{x}, {y}]"),
        Trace::End(Ending::Victory) => "the story ends: victory".to_string(),
        Trace::End(Ending::Defeat) => "the story ends: defeat".to_string(),
        Trace::Travel(map) => format!("the story travels to `{map}`"),
        Trace::Ending(dialog) => format!("the story ends the campaign: `{dialog}`"),
    }
}

fn value_text(v: &VarValue) -> String {
    match v {
        VarValue::Int(n) => n.to_string(),
        VarValue::Bool(b) => b.to_string(),
    }
}

/// Simulates `map` of `pkg` on `install` as `opts` says. A compile error is returned as the
/// report (the package is checked first).
pub fn sim_map(pkg: &Package, install: &DtInstall, map: &str, opts: &SimOptions) -> Result<SimResult, Report> {
    let never = std::sync::atomic::AtomicBool::new(false);
    Ok(sim_map_observed(pkg, install, map, opts, &never, &mut |_| {})?.map(|(result, _)| result).expect("not cancelled"))
}

/// One army as the AI viewer draws it on a day: its index in the world, its name (the
/// scenario's text, empty when it has none), its cell and its faction.
#[derive(Clone, Debug, PartialEq)]
pub struct ArmyAt {
    pub index: usize,
    pub name: String,
    pub cell: (i32, i32),
    pub faction: u8,
}

/// The world at the start of a day (or at the end of the run): the hero's cell and every army's.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub day: u64,
    pub hero: (i32, i32),
    pub armies: Vec<ArmyAt>,
}

/// A story moment for the AI viewer's log: what happened on `day`, and the cell it happened on
/// when the moment names one (a trigger's `hero enters` or `army arrives` cell, an arrival).
#[derive(Clone, Debug, PartialEq)]
pub struct Moment {
    pub day: u64,
    /// The moment in English (`describe`); the studio's log words it in its own language from `what`.
    pub text: String,
    /// The moment as the trace has it.
    pub what: Trace,
    /// The trigger that fired, for a `Fired` moment.
    pub trigger: Option<String>,
    pub cell: Option<(i32, i32)>,
}

/// What the AI viewer records of a run: a frame per day, and the story's moments by day.
#[derive(Clone, Debug, PartialEq)]
pub struct Recording {
    pub frames: Vec<Frame>,
    pub moments: Vec<Moment>,
}

/// [`sim_map`] with the world recorded: `progress` gets each day as the run reaches it, and a
/// set `cancel` stops the run (`None` is returned). The recording only reads the world, so the
/// result is the one [`sim_map`] gives for the same options.
pub fn sim_map_observed(pkg: &Package, install: &DtInstall, map: &str, opts: &SimOptions, cancel: &std::sync::atomic::AtomicBool, progress: &mut dyn FnMut(u64)) -> Result<Option<(SimResult, Recording)>, Report> {
    use std::sync::atomic::Ordering;
    let compiled = compile::compile(pkg, install, "en", map)?;
    let opened = open::build(pkg, open::package_hash(&pkg.files), Path::new(""), map, compiled, "en").map_err(|e| {
        let mut r = Report::default();
        r.error("", None, e);
        r
    })?;
    let class = opened.scenario.header.first_offered_class().map_or(HeroClass::Knight, |k| HeroClass::ALL[k]);
    let content = Arc::new(Content::from_dt(install).with_package(opened.units.clone(), opened.items.clone()));
    let story_on = !opened.story.is_empty();
    let start = std::time::Instant::now();
    let mut state = 0x9E37_79B9u32;
    let mut pick = |d: &PendingDialog| opts.choices.pick(&mut state, d);
    let mut story = opened.story.clone();
    if let Some(seed) = opts.seed {
        story.seed = seed.max(1);
    }
    let play = StoryPlay { story, trace: opts.trace, preset: opts.preset.clone(), pick: &mut pick };
    let mut frames: Vec<Frame> = Vec::new();
    let mut observe = |g: &crate::rules::game::Game, day: u64| -> bool {
        let frame = Frame {
            day,
            hero: g.tile(),
            armies: g.world.armies.iter().enumerate().map(|(index, a)| ArmyAt { index, name: a.name.clone(), cell: a.tile(&g.world.map), faction: a.faction }).collect(),
        };
        match frames.last_mut() {
            Some(last) if last.day == day => *last = frame,
            _ => frames.push(frame),
        }
        progress(day);
        !cancel.load(Ordering::Relaxed)
    };
    let run = std::panic::catch_unwind(AssertUnwindSafe(|| sim::play_days_observed(&content, &opened.scenario, class, opts.days, Some(play), &mut observe)));
    let seconds = start.elapsed().as_secs_f64();
    if cancel.load(Ordering::Relaxed) {
        return Ok(None);
    }
    Ok(Some(match run {
        Ok(run) => {
            let moments = moments_of(pkg, map, &run.trace);
            let result = SimResult {
                map: map.to_string(),
                days: opts.days,
                panic: None,
                stalled: run.stalled,
                victory_day: run.victory_day,
                defeat_day: run.defeat_day,
                ai_battles: run.ai.battles,
                player_battles: run.player_battles,
                field_battles: run.field_battles,
                story: story_on,
                trace: run.trace,
                seconds,
            };
            (result, Recording { frames, moments })
        }
        Err(payload) => {
            let result = SimResult {
                map: map.to_string(),
                days: opts.days,
                panic: Some(panic_text(payload.as_ref())),
                stalled: false,
                victory_day: None,
                defeat_day: None,
                ai_battles: 0,
                player_battles: 0,
                field_battles: Vec::new(),
                story: story_on,
                trace: Vec::new(),
                seconds,
            };
            (result, Recording { frames, moments: Vec::new() })
        }
    }))
}

/// The story's trace as the AI viewer's log: each moment with the cell it names, if any. A
/// trigger's cell is its `when`'s (`hero enters` a cell, `army arrives` at one).
fn moments_of(pkg: &Package, map: &str, trace: &[(u64, Trace)]) -> Vec<Moment> {
    use crate::pack::triggers::{Event, Place};
    trace
        .iter()
        .map(|(day, what)| {
            let (trigger, cell) = match what {
                Trace::Fired { trigger, .. } => {
                    let cell = pkg.triggers.iter().find(|t| t.value.id == *trigger && t.value.map.as_deref().is_none_or(|m| m == map)).and_then(|t| match &t.value.when {
                        Event::HeroEnters(Place::Cell(x, y)) => Some((*x, *y)),
                        Event::ArmyArrives { at: Some((x, y)), .. } => Some((*x, *y)),
                        _ => None,
                    });
                    (Some(trigger.clone()), cell)
                }
                Trace::Arrive { cell, .. } => (None, Some(*cell)),
                _ => (None, None),
            };
            Moment { day: *day, text: describe(what), what: what.clone(), trigger, cell }
        })
        .collect()
}

/// The text of a caught panic's payload.
fn panic_text(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-text payload".to_string()
    }
}

/// What `razdor-pack sim --campaign` does: the days each map is played, the dialog choices, whether
/// the trace is kept, and the developer switch that fires each map's victory after some days.
#[derive(Clone, Debug)]
pub struct CampaignOptions {
    pub days: u64,
    pub choices: Choices,
    pub trace: bool,
    pub auto_win_after: Option<u64>,
    /// Variables forced before the campaign's first map starts (`--set var=value`); they carry on.
    pub preset: Vec<(String, VarValue)>,
    /// A developer switch: the hero meets an army on a map with the squad it names (see [`Meet`]).
    pub meet: Option<Meet>,
    /// Each map entered a second time is saved and restored first (`rules::save`): the sim's check
    /// that a campaign's save round-trips mid-campaign. A developer switch.
    pub save_on_revisit: bool,
}

/// One map of a campaign run: what it was entered with, how its play ended and on which day, and
/// where the campaign went from it.
#[derive(Clone, Debug, PartialEq)]
pub struct CampaignStep {
    pub map: String,
    /// The field battles the map fought (see [`FieldFight`]).
    pub fields: Vec<FieldFight>,
    /// The carried values the map was entered with (gold, squad, named characters, variables).
    pub entered_with: String,
    pub end: MapEnd,
    /// Days after the map was entered when its play ended.
    pub day: f64,
    /// The map the campaign went to from here (a route, a travel, or a victory's override).
    pub next: Option<String>,
    /// What the story did on the map, by game minute since the map was entered, when the trace
    /// was asked for.
    pub trace: Vec<(u64, Trace)>,
}

/// A campaign played headless from its start map.
#[derive(Clone, Debug, PartialEq)]
pub struct CampaignRun {
    /// The package's id and version.
    pub campaign: String,
    pub steps: Vec<CampaignStep>,
    /// The ending dialog the campaign reached, if a story's `ending` ended it.
    pub ending: Option<String>,
    /// The panic's message, if the run panicked.
    pub panic: Option<String>,
    /// What stopped the walk without a panic (a map that does not open, too many maps).
    pub error: Option<String>,
    pub seconds: f64,
}

/// The most maps one run plays: a campaign that loops (a map back to itself by victories) stops here.
const MAX_CAMPAIGN_MAPS: usize = 64;

impl CampaignRun {
    /// No panic, no error and no softlock: the exit status of `razdor-pack sim --campaign`.
    pub fn ok(&self) -> bool {
        self.panic.is_none() && self.error.is_none() && !self.steps.iter().any(|s| s.end == MapEnd::Stalled)
    }

    /// The run for a person: one line per map, its entry and its trace when asked for, and how the
    /// campaign ended.
    pub fn text(&self) -> String {
        let mut out = format!("campaign {}: {} maps, {:.2} s\n", self.campaign, self.steps.len(), self.seconds);
        for step in &self.steps {
            out.push_str(&format!("{}: {}\n", step.map, step_text(step)));
            out.push_str(&format!("  entered with {}\n", step.entered_with));
            for (i, f) in step.fields.iter().enumerate() {
                out.push_str(&format!("  {}\n", field_text(i + 1, f)));
            }
            for (minute, what) in &step.trace {
                out.push_str(&format!("  {}\n", trace_line(what, *minute)));
            }
        }
        out.push_str(&format!("{}\n", self.outcome()));
        out
    }

    /// How the campaign ended, for a person.
    fn outcome(&self) -> String {
        if let Some(p) = &self.panic {
            return format!("PANIC: {p}");
        }
        if let Some(e) = &self.error {
            return format!("ERROR: {e}");
        }
        if self.steps.iter().any(|s| s.end == MapEnd::Stalled) {
            return "STALLED (softlock)".to_string();
        }
        match &self.ending {
            Some(dialog) => format!("the campaign ends with the ending `{dialog}`"),
            None => match self.steps.last().and_then(|s| (s.end == MapEnd::Victory).then_some(())) {
                Some(()) => "the campaign ends: no route from the last map holds".to_string(),
                None => "the campaign stops before its end".to_string(),
            },
        }
    }

    /// The run as JSON (`razdor-pack sim --campaign --json`).
    pub fn json(&self) -> serde_json::Value {
        let steps: Vec<serde_json::Value> = self
            .steps
            .iter()
            .map(|s| {
                let trace: Vec<serde_json::Value> = s.trace.iter().map(|(minute, what)| serde_json::json!({"minute": minute, "what": what})).collect();
                serde_json::json!({
                    "map": s.map,
                    "entered_with": s.entered_with,
                    "end": format!("{:?}", s.end),
                    "day": s.day,
                    "next": s.next,
                    "trace": trace,
                })
            })
            .collect();
        serde_json::json!({
            "campaign": self.campaign,
            "ok": self.ok(),
            "ending": self.ending,
            "panic": self.panic,
            "error": self.error,
            "outcome": self.outcome(),
            "steps": steps,
            "seconds": self.seconds,
        })
    }
}

fn step_text(step: &CampaignStep) -> String {
    let when = format!("on day {:.1}", step.day);
    match (&step.end, &step.next) {
        (MapEnd::Victory, Some(next)) => format!("victory {when} -> {next}"),
        (MapEnd::Victory, None) => format!("victory {when} -> the campaign ends"),
        (MapEnd::Travel, Some(next)) => format!("travels {when} -> {next}"),
        (MapEnd::Travel, None) => format!("travels {when}"),
        (MapEnd::Ending, _) => format!("ending {when}"),
        (MapEnd::Defeat, _) => format!("defeat {when}"),
        (MapEnd::Days, _) => "no victory or defeat within the day limit".to_string(),
        (MapEnd::Stalled, _) => "STALLED (softlock)".to_string(),
    }
}

/// One trace step with its day (after the map was entered).
fn trace_line(what: &Trace, minute: u64) -> String {
    format!("day {:.1}: {}", minute as f64 / crate::rules::clock::MINUTES_PER_DAY as f64, describe(what))
}

/// The values a map is entered with: gold, the squad with its named characters, the pack, and the
/// variables.
fn carried_text(g: &Game) -> String {
    let named: Vec<String> = g
        .squad
        .iter()
        .filter(|u| u.named != 0)
        .map(|u| {
            let name = g.story().and_then(|s| s.named.get(usize::from(u.named) - usize::from(crate::rules::triggers::PACKAGE_BASE))).cloned();
            name.unwrap_or_else(|| u.named.to_string())
        })
        .collect();
    let named = if named.is_empty() { String::new() } else { format!(" ({})", named.join(", ")) };
    let vars: Vec<String> = g.triggers.vars.iter().map(|(k, v)| format!("{k}={}", value_text(v))).collect();
    format!("gold {}, squad {}{named}, pack {} items, vars {}", g.gold, g.squad.len(), g.pack.count(), if vars.is_empty() { "-".into() } else { vars.join(" ") })
}

/// Plays the campaign of `pkg` from its start map: each map is opened with `open`, played for
/// `opts.days`, and the campaign goes on by the victory's route (or the travel the story asked for)
/// with what carries. A panic is caught and reported. `base` is the install's content, which each
/// map's package units and items join.
pub fn run_campaign(pkg: &Package, base: &Content, open: &mut dyn FnMut(&str) -> Result<Opened, String>, opts: &CampaignOptions) -> CampaignRun {
    let t0 = std::time::Instant::now();
    let campaign = pkg.manifest.as_ref().map_or_else(String::new, |m| format!("{} {}", m.id, m.version));
    let walked = std::panic::catch_unwind(AssertUnwindSafe(|| walk(pkg, base, open, opts)));
    let seconds = t0.elapsed().as_secs_f64();
    match walked {
        Ok(Ok((steps, ending))) => CampaignRun { campaign, steps, ending, panic: None, error: None, seconds },
        Ok(Err((steps, error))) => CampaignRun { campaign, steps, ending: None, panic: None, error: Some(error), seconds },
        Err(payload) => CampaignRun { campaign, steps: Vec::new(), ending: None, panic: Some(panic_text(payload.as_ref())), error: None, seconds },
    }
}

/// The walk of [`run_campaign`]: the steps made, and the ending or the error that stopped it.
type Walk = Result<(Vec<CampaignStep>, Option<String>), (Vec<CampaignStep>, String)>;

fn walk(pkg: &Package, base: &Content, open: &mut dyn FnMut(&str) -> Result<Opened, String>, opts: &CampaignOptions) -> Walk {
    let mut steps = Vec::new();
    let Some(campaign) = pkg.campaign.as_ref() else { return Err((steps, "the package has no campaign.toml".into())) };
    let mut map = campaign.start.clone();
    // The game just left, the map it was on and the point the next map is entered at.
    let mut left: Option<(Game, Opened, Option<String>)> = None;
    let mut state = 0x9E37_79B9u32;
    let mut pick = |d: &PendingDialog| opts.choices.pick(&mut state, d);
    loop {
        if steps.len() >= MAX_CAMPAIGN_MAPS {
            return Err((steps, format!("the campaign goes on past {MAX_CAMPAIGN_MAPS} maps: a loop?")));
        }
        let opened = open(&map).map_err(|e| (steps.clone(), e))?;
        let content = Arc::new(base.with_package(opened.units.clone(), opened.items.clone()));
        // A game entered from one that is traced is traced from its start (`enter`).
        let mut g = match left.take() {
            None => {
                let class = opened.scenario.header.first_offered_class().map_or(HeroClass::Knight, |k| HeroClass::ALL[k]);
                let mut g = Game::from_scenario(content.clone(), &opened.scenario, class);
                if opts.trace {
                    g.triggers.trace = Some(Vec::new());
                }
                // Forced before the story starts, so its game_start moment sees them (`--set`).
                for (name, value) in &opts.preset {
                    g.triggers.vars.insert(name.clone(), value.clone());
                }
                opened.start(&mut g);
                g
            }
            Some((old, from, at)) => enter(&old, &from, &opened, at.as_deref(), content.clone()).map_err(|e| (steps.clone(), e))?,
        };
        if opts.save_on_revisit && steps.iter().any(|s| s.map == map) {
            g = save_round_trip(g, content.clone()).map_err(|e| (steps.clone(), format!("the save of {map} does not round-trip: {e}")))?;
        }
        if let Some(m) = opts.meet.as_ref().filter(|m| m.map == map) {
            meet(&mut g, &content, m).map_err(|e| (steps.clone(), e))?;
        }
        let entered_with = carried_text(&g);
        let t0 = g.clock.total_minutes();
        let entered_at = t0 as u64;
        let mut fields = Vec::new();
        let end = sim::play_campaign_map(&mut g, opts.days, opts.auto_win_after, &mut pick, &mut fields);
        let mut step = CampaignStep {
            map: map.clone(),
            fields,
            entered_with,
            end,
            day: (g.clock.total_minutes() - t0) / crate::rules::clock::MINUTES_PER_DAY as f64,
            next: None,
            trace: g.triggers.trace.clone().unwrap_or_default().into_iter().map(|(minute, what)| (minute.saturating_sub(entered_at), what)).collect(),
        };
        match end {
            MapEnd::Victory => {
                let next = after_victory(&mut g, &opened.routes);
                step.next = next.clone();
                steps.push(step);
                match next {
                    Some(n) => {
                        left = Some((g, opened, None));
                        map = n;
                    }
                    None => return Ok((steps, None)),
                }
            }
            MapEnd::Travel => {
                let travel = g.triggers.travel.take().unwrap_or_else(|| unreachable!("a travel ended the play"));
                step.next = Some(travel.map.clone());
                steps.push(step);
                left = Some((g, opened, travel.at));
                map = travel.map;
            }
            MapEnd::Ending => {
                let dialog = g.triggers.ending.take();
                steps.push(step);
                return Ok((steps, dialog));
            }
            MapEnd::Defeat | MapEnd::Days | MapEnd::Stalled => {
                steps.push(step);
                return Ok((steps, None));
            }
        }
    }
}

/// A game saved and loaded again, as the game does (`rules::save`): the story, the campaign's stored
/// maps and the clock come back as they were. The content is the map's own, as on a load.
fn save_round_trip(g: Game, content: Arc<Content>) -> Result<Game, String> {
    use crate::rules::save::{decode, encode, meta_of, SaveKind};
    let meta = meta_of(&g, SaveKind::Manual, "campaign").map_err(|e| format!("{e:?}"))?;
    let bytes = encode(&meta, &g).map_err(|e| format!("{e:?}"))?;
    let (_, mut back) = decode(&bytes).map_err(|e| format!("{e:?}"))?;
    back.content = content;
    if back.triggers != g.triggers {
        return Err("the story's state changed".into());
    }
    Ok(back)
}

/// `razdor-pack sim --campaign`: the package's campaign played headless (see [`run_campaign`]),
/// its maps opened on the install. A compile error is returned as the report (the package is
/// checked first).
pub fn sim_campaign(pkg: &Package, install: &DtInstall, opts: &CampaignOptions) -> Result<CampaignRun, Report> {
    let base = Content::from_dt(install);
    let hash = open::package_hash(&pkg.files);
    let mut open_map = |map: &str| -> Result<Opened, String> {
        let compiled = compile::compile(pkg, install, "en", map).map_err(|r| r.to_text())?;
        open::build(pkg, hash, Path::new(""), map, compiled, "en")
    };
    Ok(run_campaign(pkg, &base, &mut open_map, opts))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::{reach, Files};

    /// The example package, read from the repository.
    fn example() -> Package {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story");
        Package::parse(Files::from_dir(&dir).unwrap()).0
    }

    fn options(days: u64) -> SimOptions {
        SimOptions { days, choices: Choices::First, trace: false, preset: Vec::new(), seed: None }
    }

    #[test]
    fn choices_read_their_spec() {
        assert_eq!(Choices::parse("first"), Ok(Choices::First));
        assert_eq!(Choices::parse("random"), Ok(Choices::Random));
        assert_eq!(Choices::parse("ford=pay, gate = ask"), Ok(Choices::Forced(vec![("ford".into(), "pay".into()), ("gate".into(), "ask".into())])));
        assert!(Choices::parse("ford").is_err(), "a dialog needs a choice");
        assert!(Choices::parse(" , ").is_err(), "nothing to force");
    }

    #[test]
    fn a_forced_choice_is_taken_when_it_shows_and_the_first_elsewhere() {
        let d = PendingDialog { id: "gate".into(), choices: vec!["pay".into(), "ask".into()] };
        let mut state = 1;
        let forced = Choices::parse("gate=ask").unwrap();
        assert_eq!(forced.pick(&mut state, &d), Some("ask".into()));
        assert_eq!(forced.pick(&mut state, &PendingDialog { id: "other".into(), choices: vec!["x".into()] }), Some("x".into()));
        let hidden = Choices::parse("gate=nope").unwrap();
        assert_eq!(hidden.pick(&mut state, &d), Some("pay".into()), "a choice that does not show is not taken");
        assert_eq!(Choices::First.pick(&mut state, &PendingDialog { id: "note".into(), choices: vec![] }), None, "«Далее»");
    }

    #[test]
    fn the_random_rule_repeats_its_answers() {
        let d = PendingDialog { id: "g".into(), choices: vec!["a".into(), "b".into(), "c".into()] };
        let run = || {
            let mut state = 0x9E37_79B9u32;
            (0..8).map(|_| Choices::Random.pick(&mut state, &d).unwrap()).collect::<Vec<_>>()
        };
        assert_eq!(run(), run());
        assert!(run().iter().any(|c| c != &run()[0]), "not always the same choice");
    }

    /// The example plays 30 days on the install with no panic and no softlock (skipped without
    /// `RAZDOR_DT_DIR`, as the other tests on the real files).
    #[test]
    fn the_example_plays_thirty_days() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let r = sim_map(&example(), &install, "first-light", &options(30)).unwrap_or_else(|rep| panic!("{}", rep.to_text()));
        println!("{}", r.text());
        assert!(r.ok(), "{}", r.text());
        assert_eq!(r.days, 30);
        assert!(!r.story, "the example has no story");
        assert!(r.trace.is_empty());
    }

    /// The example's compiled map is the same after a payload round trip, and its package unit
    /// draws the knight's picture (the install's frame of unit 1).
    #[test]
    fn the_example_compiles_on_the_install() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let c = compile::compile(&example(), &install, "en", "first-light").unwrap_or_else(|r| panic!("{}", r.to_text()));
        let back = crate::dt::dtm::Scenario::parse_payload(&c.scenario.to_payload()).unwrap();
        assert_eq!(back, c.scenario);
        let knight = install.unit_picture_frame(1).unwrap();
        assert!(c.pictures.contains(&(200, knight)), "{:?}", c.pictures);
        assert_eq!(c.units.iter().map(|u| u.id).collect::<Vec<_>>(), vec![200]);
    }

    /// A save on the hub's revisit (marka, the second time) round-trips, and the restored game goes on
    /// to the ending: the route ally=1 with the save made in the campaign walk (`save_on_revisit`).
    #[test]
    fn a_save_on_the_hub_revisit_round_trips_and_the_campaign_goes_on_to_its_ending() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let pkg = Package::parse(Files::from_dir(&root).unwrap()).0;
        let opts = CampaignOptions {
            days: 60,
            choices: Choices::parse("council=order,bw_council=iron").unwrap(),
            trace: false,
            auto_win_after: Some(20),
            preset: vec![("quests_done".into(), VarValue::Int(2)), ("order".into(), VarValue::Int(2)), ("crown".into(), VarValue::Int(1))],
            save_on_revisit: true,
            meet: None,
        };
        let run = sim_campaign(&pkg, &install, &opts).unwrap_or_else(|r| panic!("{}", r.to_text()));
        assert!(run.ok(), "{}", run.text());
        let maps: Vec<&str> = run.steps.iter().map(|s| s.map.as_str()).collect();
        assert_eq!(maps.iter().filter(|m| **m == "marka").count(), 2, "{maps:?}");
        assert_eq!(run.ending.as_deref(), Some("end_iron"), "{}", run.text());
    }

    /// The four routes of «Пепельная Марка», each as `razdor-pack sim --campaign --auto-win-after 20`
    /// with its `--set` and `--choices`: each must end on its ending, with no panic or softlock.
    #[test]
    fn the_four_routes_of_pepelnaya_marka_end_on_their_endings() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let pkg = Package::parse(Files::from_dir(&root).unwrap()).0;
        type Preset = &'static [(&'static str, VarValue)];
        let routes: [(Preset, &str, &str); 4] = [
            (&[("quests_done", VarValue::Int(2)), ("order", VarValue::Int(2)), ("crown", VarValue::Int(1))], "council=order,bw_council=iron", "end_iron"),
            // The sim's hero does not walk: Reedmere's raid is not fought, and its burn costs the trust 1 (brod.toml
            // raid_1_burns), and the sim's Brod earns nothing. The scripted Marsh route (the_campaign_plays_the_marsh_to_the_free_march)
            // enters Marka with marsh_trust 5 and reaches Barrows with it, so the preset is that value (the Free March
            // needs 3 there).
            (&[("quests_done", VarValue::Int(2)), ("marsh_trust", VarValue::Int(5))], "council=marsh,bw_council=free", "end_free"),
            // The Companies need Kord hired (kord 1): his company is the hero's men.
            (&[("quests_done", VarValue::Int(2)), ("companies", VarValue::Int(2)), ("kord", VarValue::Int(1))], "council=companies,bw_council=free", "end_free"),
            (
                &[("quests_done", VarValue::Int(2)), ("order", VarValue::Int(1)), ("ilm", VarValue::Int(2)), ("grey_bargain_offered", VarValue::Bool(true))],
                "council=order",
                "end_grey",
            ),
        ];
        for (preset, choices, ending) in routes {
            let opts = CampaignOptions {
                days: 60,
                choices: Choices::parse(choices).unwrap(),
                trace: false,
                auto_win_after: Some(20),
                preset: preset.iter().map(|(k, v)| (k.to_string(), v.clone())).collect(),
                save_on_revisit: false,
                meet: None,
            };
            let run = sim_campaign(&pkg, &install, &opts).unwrap_or_else(|r| panic!("{}", r.to_text()));
            assert!(run.ok(), "{}", run.text());
            assert_eq!(run.ending.as_deref(), Some(ending), "{}", run.text());
        }
    }

    /// The cells of the seven campaign maps of «Пепельная Марка» that the hero's start cannot reach on
    /// foot, from the story's own cells (`pack::reach`): the entrances of the buildings, the points,
    /// the armies, the cells the triggers name (a `hero_enters` cell, its arrival, a teleport), the
    /// travel points, and each army's goal and patrol cells, reached from the army's own cell. The
    /// engine walks the compiled map. Skipped without `RAZDOR_DT_DIR`.
    #[test]
    fn the_cells_of_the_campaign_maps_of_pepelnaya_marka_are_reachable_from_the_hero() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(Path::new(&dir)).unwrap();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let pkg = Package::parse(Files::from_dir(&root).unwrap()).0;
        let mut failures = Vec::new();
        for map in ["brod", "marka", "abbey", "salt", "camp", "gates", "barrows"] {
            let compiled = compile::compile(&pkg, &install, "en", map).unwrap_or_else(|r| panic!("{}", r.to_text()));
            let opened = open::build(&pkg, open::package_hash(&pkg.files), Path::new(""), map, compiled, "en").unwrap_or_else(|e| panic!("{map}: {e}"));
            let content = Arc::new(Content::from_dt(&install).with_package(opened.units.clone(), opened.items.clone()));
            let class = opened.scenario.header.first_offered_class().map_or(HeroClass::Knight, |k| HeroClass::ALL[k]);
            let game = Game::from_scenario(content, &opened.scenario, class);
            let story = reach::story(&pkg.files, map).unwrap_or_else(|e| panic!("{map}: {e}"));
            for site in reach::unreached(&game.world.map, game.tile(), &story) {
                failures.push(format!("{map}: {} (in {})", site.what, site.file));
            }
        }
        failures.sort();
        assert!(failures.is_empty(), "the hero cannot reach these cells on foot:\n{}", failures.join("\n"));
    }

    fn marka() -> Package {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        Package::parse(Files::from_dir(&dir).unwrap()).0
    }

    /// A result without its wall-clock time (the only field two runs differ in).
    fn timeless(mut r: SimResult) -> SimResult {
        r.seconds = 0.0;
        r
    }

    /// The AI viewer's run is the sim's run: the observed result is the one `sim_map` gives, and the
    /// recording has a frame per day, in order, the same on a second run (skipped without
    /// `RAZDOR_DT_DIR`).
    #[test]
    fn the_observed_run_is_the_sim_run_and_records_every_day() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let pkg = marka();
        let opts = SimOptions { days: 10, choices: Choices::First, trace: true, preset: Vec::new(), seed: None };
        let plain = sim_map(&pkg, &install, "brod", &opts).unwrap_or_else(|r| panic!("{}", r.to_text()));
        let never = std::sync::atomic::AtomicBool::new(false);
        let (observed, rec) = sim_map_observed(&pkg, &install, "brod", &opts, &never, &mut |_| {}).unwrap().expect("not cancelled");
        assert_eq!(timeless(plain.clone()), timeless(observed.clone()), "the recording changes nothing");
        assert!(rec.frames.windows(2).all(|w| w[0].day < w[1].day), "one frame per day, in order");
        assert_eq!(rec.frames.first().map(|f| f.day), Some(0));
        assert_eq!(rec.frames.last().map(|f| f.day), Some(10), "the last frame is the run's end");
        assert!(!rec.frames.last().unwrap().armies.is_empty(), "brod has armies");
        let (_, again) = sim_map_observed(&pkg, &install, "brod", &opts, &never, &mut |_| {}).unwrap().expect("not cancelled");
        assert_eq!(rec.frames, again.frames, "the same run gives the same positions");
    }

    /// The story's firings are recorded with their day, in the trace's order, and a trigger that
    /// names a cell keeps it (`brod`'s story fires at the game's start, on day 0).
    #[test]
    fn the_story_moments_are_recorded_with_their_day() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let pkg = marka();
        let opts = SimOptions { days: 10, choices: Choices::First, trace: true, preset: Vec::new(), seed: None };
        let never = std::sync::atomic::AtomicBool::new(false);
        let (result, rec) = sim_map_observed(&pkg, &install, "brod", &opts, &never, &mut |_| {}).unwrap().expect("not cancelled");
        assert_eq!(rec.moments.len(), result.trace.len());
        for (m, (day, _)) in rec.moments.iter().zip(&result.trace) {
            assert_eq!(m.day, *day);
        }
        assert!(rec.moments.iter().any(|m| m.day == 0 && m.trigger.is_some()), "a trigger fires at the start: {:?}", rec.moments);
        let cell = rec.moments.iter().find(|m| m.trigger.is_some() && m.cell.is_some());
        assert!(cell.is_some() || rec.moments.iter().all(|m| m.trigger.is_none() || m.cell.is_none()), "a cell is kept when the trigger names one");
    }

    /// A cancelled run gives no result (the AI viewer's Cancel).
    #[test]
    fn a_cancelled_run_gives_nothing() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let opts = SimOptions { days: 10, choices: Choices::First, trace: false, preset: Vec::new(), seed: None };
        let stop = std::sync::atomic::AtomicBool::new(true);
        assert!(sim_map_observed(&marka(), &install, "brod", &opts, &stop, &mut |_| {}).unwrap().is_none());
    }
}
