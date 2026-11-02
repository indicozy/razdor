//! `razdor-pack sim`: plays a package's map headless with the AI for some days, the way the
//! community map smoke test plays the install's maps (`rules::sim`). A panic is caught and
//! reported; a softlock (the clock stops) is reported too.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use super::compile::{self, Compiled};
use super::diag::Report;
use super::load::Package;
use crate::dt::install::DtInstall;
use crate::rules::content::{Content, HeroClass};
use crate::rules::sim;

/// What a simulation of one map saw.
#[derive(Clone, Debug, PartialEq)]
pub struct SimResult {
    pub map: String,
    pub days: u64,
    /// The panic's message, if the simulation panicked.
    pub panic: Option<String>,
    /// The clock did not reach the end: a softlock.
    pub stalled: bool,
    /// Days after the start when the victory fired, if it did.
    pub victory_day: Option<f64>,
    /// Days after the start when the hero fell or the defeat fired, if it did.
    pub defeat_day: Option<f64>,
    pub ai_battles: u32,
    pub player_battles: usize,
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

    /// The result as JSON (`razdor-pack sim --json`).
    pub fn json(&self) -> serde_json::Value {
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
            "seconds": self.seconds,
        })
    }
}

/// Simulates `map` of `pkg` for `days` days on `install`. A compile error is returned as the
/// report (the package is checked first).
pub fn sim_map(pkg: &Package, install: &DtInstall, map: &str, days: u64) -> Result<SimResult, Report> {
    let Compiled { scenario, units, items, .. } = compile::compile(pkg, install, "en", map)?;
    let class = scenario.header.first_offered_class().map_or(HeroClass::Knight, |k| HeroClass::ALL[k]);
    let content = Arc::new(Content::from_dt(install).with_package(units, items));
    let start = std::time::Instant::now();
    let run = std::panic::catch_unwind(AssertUnwindSafe(|| sim::play_days(&content, &scenario, class, days)));
    let seconds = start.elapsed().as_secs_f64();
    Ok(match run {
        Ok(run) => SimResult {
            map: map.to_string(),
            days,
            panic: None,
            stalled: run.stalled,
            victory_day: run.victory_day,
            defeat_day: run.defeat_day,
            ai_battles: run.ai.battles,
            player_battles: run.player_battles,
            seconds,
        },
        Err(payload) => SimResult {
            map: map.to_string(),
            days,
            panic: Some(panic_text(payload.as_ref())),
            stalled: false,
            victory_day: None,
            defeat_day: None,
            ai_battles: 0,
            player_battles: 0,
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

    /// The example plays 30 days on the install with no panic and no softlock (skipped without
    /// `RAZDOR_DT_DIR`, as the other tests on the real files).
    #[test]
    fn the_example_plays_thirty_days() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let install = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let r = sim_map(&example(), &install, "first-light", 30).unwrap_or_else(|rep| panic!("{}", rep.to_text()));
        println!("{}", r.text());
        assert!(r.ok(), "{}", r.text());
        assert_eq!(r.days, 30);
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
