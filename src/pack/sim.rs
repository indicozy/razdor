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
use crate::rules::content::{Content, HeroClass};
use crate::rules::sim::{self, StoryPlay};
use crate::rules::triggers::{Ending, PendingDialog, Trace};

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
        format!(
            "{}: {status}; {} days, {outcome}; AI battles {}, player battles {}; {:.2} s",
            self.map, self.days, self.ai_battles, self.player_battles, self.seconds
        )
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
            "story": self.story,
            "trace": trace,
            "seconds": self.seconds,
        })
    }
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
    let play = StoryPlay { story: opened.story.clone(), trace: opts.trace, pick: &mut pick };
    let run = std::panic::catch_unwind(AssertUnwindSafe(|| sim::play_days_with(&content, &opened.scenario, class, opts.days, Some(play))));
    let seconds = start.elapsed().as_secs_f64();
    Ok(match run {
        Ok(run) => SimResult {
            map: map.to_string(),
            days: opts.days,
            panic: None,
            stalled: run.stalled,
            victory_day: run.victory_day,
            defeat_day: run.defeat_day,
            ai_battles: run.ai.battles,
            player_battles: run.player_battles,
            story: story_on,
            trace: run.trace,
            seconds,
        },
        Err(payload) => SimResult {
            map: map.to_string(),
            days: opts.days,
            panic: Some(panic_text(payload.as_ref())),
            stalled: false,
            victory_day: None,
            defeat_day: None,
            ai_battles: 0,
            player_battles: 0,
            story: story_on,
            trace: Vec::new(),
            seconds,
        },
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::Files;

    /// The example package, read from the repository.
    fn example() -> Package {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/hello-story");
        Package::parse(Files::from_dir(&dir).unwrap()).0
    }

    fn options(days: u64) -> SimOptions {
        SimOptions { days, choices: Choices::First, trace: false }
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
}
