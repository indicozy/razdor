//! The run's mark (Razdor's, backlog 1532, the owner's decisions): the worst thing a run has used,
//! shown as a coloured checkmark on the scenario list. A run is a game from the map's start; its
//! mark rises and never falls: a cheat (red), an experimental rule (yellow), a gameplay setting
//! that is not the original's (grey), else green. The run keeps it in its save (`Game::run_mark`),
//! and a victory writes the map's best mark ever into the player's `marks.json` (`Records`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::save;

/// The mark's colour, best first. The order is how bad it is: the worst mark is the greatest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// No cheat, no experimental rule, every gameplay setting the original's. A run whose history
    /// is unknown (a save from before the marks) is green too, unless its save shows otherwise.
    Green,
    /// A gameplay setting that is not the original's.
    Grey,
    /// An experimental scenario rule is on.
    Yellow,
    /// A cheat was used.
    Red,
}

/// What set the mark: the first cheat, rule or setting that reached its level.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reason {
    /// A clean run.
    Honest,
    /// The cheat command's name (`god`); empty when the save does not say which cheat (a save
    /// from before the mark, with cheats used).
    Cheat(String),
    /// The experimental rule's name, as the catalog has it («Six equipment cells»).
    Rule(String),
    /// The gameplay setting's name, as the catalog has it («Unit battle participation»).
    Setting(String),
    /// The run's history is unknown: a save from before the mark. Green (the owner's decision,
    /// 2026-11-13: an old game counts as played fairly); the tooltip says the history is unknown.
    Unknown,
}

/// A run's mark: its level and what set it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunMark {
    pub level: Level,
    pub reason: Reason,
}

impl RunMark {
    /// A run at its start: nothing used yet.
    pub fn start() -> RunMark {
        RunMark { level: Level::Green, reason: Reason::Honest }
    }

    /// A save from before the mark: its history is unknown. The owner's decision (2026-11-13): it
    /// is green, as a fair run; only what the run uses after that, or what its save shows (a cheat,
    /// an experimental rule, a changed setting), lowers it.
    pub fn unknown_history() -> RunMark {
        RunMark { level: Level::Green, reason: Reason::Unknown }
    }

    /// Raises the mark to `level` for `reason` when that is worse than it is now. Sticky: a
    /// better level never lowers it, and the first cause of a level stays the reason.
    pub fn worsen(&mut self, level: Level, reason: Reason) -> bool {
        if level > self.level {
            self.level = level;
            self.reason = reason;
            true
        } else {
            false
        }
    }

    /// Keeps the worse of this mark and an earlier mark of the same run (a load of a save made
    /// earlier, or a battle's restart, must not clear what the run has used).
    pub fn absorb(&mut self, earlier: &RunMark) {
        self.worsen(earlier.level, earlier.reason.clone());
    }
}

/// A map's best mark ever (`Records`) and the date it was reached (`YYYY-MM-DD`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub mark: RunMark,
    pub date: String,
}

/// The player's best mark per map, from `marks.json` beside `settings.json`. A map is keyed by
/// its file name without the extension, lower-cased, as the favourites are matched.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Records {
    #[serde(default)]
    maps: BTreeMap<String, Record>,
}

fn key(file: &str) -> String {
    file.to_lowercase()
}

impl Records {
    /// The record of map `file`, if it was won.
    pub fn get(&self, file: &str) -> Option<&Record> {
        self.maps.get(&key(file))
    }

    /// Notes a victory on map `file` with `mark` on `date`: kept when it is the best mark so far
    /// (a tie keeps the earlier record and its date). Returns whether the record changed.
    pub fn record_victory(&mut self, file: &str, mark: &RunMark, date: &str) -> bool {
        let better = self.maps.get(&key(file)).is_none_or(|old| mark.level < old.mark.level);
        if better {
            self.maps.insert(key(file), Record { mark: mark.clone(), date: date.to_string() });
        }
        better
    }

    /// A campaign's mark: the worst of its maps' records, once every one of its maps is won
    /// (`None` before that, and for no maps). A tie keeps the first map's.
    pub fn campaign(&self, files: &[&str]) -> Option<&Record> {
        let mut worst: Option<&Record> = None;
        for file in files {
            let record = self.get(file)?;
            if worst.is_none_or(|w| record.mark.level > w.mark.level) {
                worst = Some(record);
            }
        }
        worst
    }

    /// The records as the file holds them (pretty JSON).
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// The records of a file's text; a damaged or missing file reads as none. A record of an unknown
    /// history written grey (before the owner's decision of 2026-11-13) reads green, as it now is.
    pub fn from_json(text: &str) -> Records {
        let mut records: Records = serde_json::from_str(text).unwrap_or_default();
        for record in records.maps.values_mut() {
            if record.mark.reason == Reason::Unknown && record.mark.level == Level::Grey {
                record.mark.level = Level::Green;
            }
        }
        records
    }

    /// The file `marks.json` in the saves' folder (`save::default_dir`), if there is one.
    pub fn path() -> Option<PathBuf> {
        save::default_dir().map(|d| d.join("marks.json"))
    }

    /// The player's records (none when the file is missing or damaged).
    pub fn load() -> Records {
        Records::path().and_then(|p| std::fs::read_to_string(p).ok()).map_or_else(Records::default, |t| Records::from_json(&t))
    }

    /// Writes the records to the player's file; a failure is logged, the game goes on.
    pub fn save(&self) {
        let Some(path) = Records::path() else { return };
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, self.to_json()));
        if let Err(e) = written {
            crate::diag!("{}: {e}", path.display());
        }
    }
}

/// Notes a victory on map `file` with the run's mark, in the player's file (written at once,
/// only when the record changes).
pub fn record_victory(file: &str, mark: &RunMark) {
    let mut records = Records::load();
    if records.record_victory(file, mark, &today()) {
        records.save();
    }
}

/// Today's date (UTC) as `YYYY-MM-DD`.
pub fn today() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let (y, m, d) = civil((secs / 86_400) as i64);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The civil date (year, month, day) of `days` since 1970-01-01 (Howard Hinnant's algorithm).
pub fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::{Content, ExperimentalRules, HeroClass};
    use crate::rules::game::Game;
    use std::sync::Arc;

    fn mark(level: Level, reason: Reason) -> RunMark {
        RunMark { level, reason }
    }

    #[test]
    fn a_run_goes_green_grey_yellow_red_and_never_back_up() {
        let mut m = RunMark::start();
        assert_eq!(m, mark(Level::Green, Reason::Honest));
        assert!(m.worsen(Level::Grey, Reason::Setting("Unit battle participation".into())));
        assert!(m.worsen(Level::Yellow, Reason::Rule("Six equipment cells".into())));
        assert!(m.worsen(Level::Red, Reason::Cheat("god".into())));
        assert!(!m.worsen(Level::Yellow, Reason::Rule("Free units".into())), "a better level never lowers it");
        assert!(!m.worsen(Level::Grey, Reason::Setting("Keep formation after battle".into())));
        assert_eq!(m, mark(Level::Red, Reason::Cheat("god".into())));
    }

    #[test]
    fn the_first_cause_of_a_level_stays_the_reason() {
        let mut m = RunMark::start();
        m.worsen(Level::Grey, Reason::Setting("Unit battle participation".into()));
        m.worsen(Level::Grey, Reason::Setting("Keep formation after battle".into()));
        assert_eq!(m.reason, Reason::Setting("Unit battle participation".into()));
    }

    #[test]
    fn absorbing_an_earlier_mark_keeps_the_worse_of_the_two() {
        let mut now = RunMark::start();
        now.absorb(&mark(Level::Red, Reason::Cheat("gold".into())));
        assert_eq!(now.level, Level::Red, "a load of a clean save keeps the cheat of the run");
        let mut later = mark(Level::Yellow, Reason::Rule("Faster AI".into()));
        later.absorb(&RunMark::start());
        assert_eq!(later.level, Level::Yellow);
    }

    #[test]
    fn an_old_save_is_green_until_the_run_uses_something() {
        let mut old = RunMark::unknown_history();
        assert_eq!(old.level, Level::Green, "the owner's decision: unknown history is a fair run");
        assert!(old.worsen(Level::Grey, Reason::Setting("Unit battle participation".into())));
        assert_eq!(old.level, Level::Grey);
        assert!(!old.worsen(Level::Green, Reason::Honest), "green is never reached again");
        assert_eq!(old.level, Level::Grey);
    }

    #[test]
    fn a_grey_record_of_an_unknown_history_reads_green_from_the_file() {
        let mut r = Records::default();
        r.maps.insert("рк1-начало пути".into(), Record { mark: mark(Level::Grey, Reason::Unknown), date: "2026-11-01".into() });
        let back = Records::from_json(&r.to_json());
        assert_eq!(back.get("РК1-Начало пути").map(|r| r.mark.level), Some(Level::Green));
        let mut kept = Records::default();
        kept.record_victory("РК2-Дикие пустоши", &mark(Level::Grey, Reason::Setting("Unit battle participation".into())), "2026-11-02");
        assert_eq!(Records::from_json(&kept.to_json()), kept, "other records are kept as they are");
    }

    #[test]
    fn a_victory_keeps_the_best_mark_and_its_date() {
        let mut r = Records::default();
        assert!(r.record_victory("Тихая пристань", &mark(Level::Red, Reason::Cheat("god".into())), "2026-11-01"));
        assert!(r.record_victory("Тихая пристань", &mark(Level::Grey, Reason::Setting("Unit battle participation".into())), "2026-11-02"));
        assert!(!r.record_victory("Тихая пристань", &mark(Level::Red, Reason::Cheat("win".into())), "2026-11-03"), "a worse one changes nothing");
        assert!(!r.record_victory("Тихая пристань", &mark(Level::Grey, Reason::Setting("Keep formation after battle".into())), "2026-11-04"), "a tie keeps the earlier record");
        let got = r.get("тихая пристань").expect("the key is the file name, any case");
        assert_eq!((got.mark.level, got.date.as_str()), (Level::Grey, "2026-11-02"));
    }

    #[test]
    fn the_records_survive_their_file() {
        let mut r = Records::default();
        r.record_victory("РК1-Начало пути", &RunMark::start(), "2026-11-06");
        let back = Records::from_json(&r.to_json());
        assert_eq!(back, r);
        assert_eq!(Records::from_json("not json"), Records::default(), "a damaged file reads as none");
    }

    #[test]
    fn a_campaign_shows_its_worst_map_once_all_are_won() {
        let mut r = Records::default();
        r.record_victory("РК1-Начало пути", &RunMark::start(), "2026-11-06");
        r.record_victory("РК2-Дикие пустоши", &mark(Level::Yellow, Reason::Rule("Free units".into())), "2026-11-06");
        assert!(r.campaign(&["РК1-Начало пути", "РК2-Дикие пустоши", "РК3-Столица"]).is_none(), "a map not won yet");
        r.record_victory("РК3-Столица", &mark(Level::Grey, Reason::Setting("Unit battle participation".into())), "2026-11-06");
        let worst = r.campaign(&["РК1-Начало пути", "РК2-Дикие пустоши", "РК3-Столица"]).expect("all won");
        assert_eq!(worst.mark, mark(Level::Yellow, Reason::Rule("Free units".into())), "the worst of its maps");
        assert!(r.campaign(&[]).is_none());
    }

    fn game(rules: ExperimentalRules) -> Game {
        let content = Content::builtin();
        Game::new(Arc::new(content.with_experimental(rules)), HeroClass::Knight)
    }

    #[test]
    fn a_game_s_mark_rises_with_what_it_uses_and_never_falls() {
        let mut g = game(ExperimentalRules::default());
        assert_eq!(g.run_mark(), RunMark::start(), "a run starts green");
        g.note_settings(None);
        assert_eq!(g.run_mark().level, Level::Green, "the original's settings: still green");
        g.note_settings(Some("Unit battle participation"));
        assert_eq!(g.run_mark(), mark(Level::Grey, Reason::Setting("Unit battle participation".into())));
        g.note_settings(None);
        assert_eq!(g.run_mark().level, Level::Grey, "a setting changed back does not clean the run");
        g.note_cheat("god");
        assert_eq!(g.run_mark(), mark(Level::Red, Reason::Cheat("god".into())));
        g.note_settings(Some("Keep formation after battle"));
        assert_eq!(g.run_mark().level, Level::Red, "red stays red");
    }

    #[test]
    fn the_experimental_rules_of_the_playthrough_make_it_yellow() {
        let g = {
            let mut g = game(ExperimentalRules { six_cells: true, ..Default::default() });
            g.note_settings(None);
            g
        };
        assert_eq!(g.run_mark(), mark(Level::Yellow, Reason::Rule("Six equipment cells".into())));
        let mut g2 = game(ExperimentalRules { free_units: true, two_potions: true, ..Default::default() });
        g2.note_settings(Some("Unit battle participation"));
        assert_eq!(g2.run_mark(), mark(Level::Yellow, Reason::Rule("At most two potions per character".into())), "the first rule on in the panel's order names it; yellow beats grey");
    }

    #[test]
    fn an_old_save_s_run_reads_grey_at_most_and_keeps_known_evidence() {
        let plain = game(ExperimentalRules::default());
        let mut old = serde_json::to_value(&plain).unwrap();
        old.as_object_mut().unwrap().remove("run_mark");
        let mut loaded: Game = serde_json::from_value(old.clone()).unwrap();
        assert!(loaded.run_mark.is_none());
        assert_eq!(loaded.run_mark(), RunMark::unknown_history());
        loaded.note_settings(None);
        assert_eq!(loaded.run_mark().level, Level::Green, "unknown history alone is green");
        let mut changed: Game = serde_json::from_value(old.clone()).unwrap();
        changed.note_settings(Some("Unit battle participation"));
        assert_eq!(changed.run_mark(), mark(Level::Grey, Reason::Setting("Unit battle participation".into())), "a changed setting still makes it grey");
        // A save that records a cheat or a rule keeps that, as its history is known.
        let mut cheated = old.clone();
        cheated["cheats"]["used"] = serde_json::json!(true);
        let mut cheated: Game = serde_json::from_value(cheated).unwrap();
        cheated.note_settings(None);
        assert_eq!(cheated.run_mark(), mark(Level::Red, Reason::Cheat(String::new())));
        let mut ruled = old.clone();
        ruled["experimental"]["six_cells"] = serde_json::json!(true);
        let mut ruled: Game = serde_json::from_value(ruled).unwrap();
        ruled.note_settings(None);
        assert_eq!(ruled.run_mark(), mark(Level::Yellow, Reason::Rule("Six equipment cells".into())));
    }

    /// The owner's decision (2026-11-13): an old save with nothing else is green; one that records a
    /// cheat is red.
    #[test]
    fn an_old_save_with_nothing_else_is_green_and_a_recorded_cheat_is_red() {
        let plain = game(ExperimentalRules::default());
        let mut old = serde_json::to_value(&plain).unwrap();
        old.as_object_mut().unwrap().remove("run_mark");
        let mut clean: Game = serde_json::from_value(old.clone()).unwrap();
        clean.note_settings(None);
        assert_eq!(clean.run_mark(), RunMark::unknown_history());
        assert_eq!(clean.run_mark().level, Level::Green);
        let mut cheated = old;
        cheated["cheats"]["used"] = serde_json::json!(true);
        let mut cheated: Game = serde_json::from_value(cheated).unwrap();
        cheated.note_settings(None);
        assert_eq!(cheated.run_mark().level, Level::Red);
    }

    /// A save from before the parity switches (backlog 1601): the six keys are missing, so Razdor's
    /// old rules read ON as inherited, not chosen. The run stays unknown (grey), not yellow. A
    /// switch the player chose (its key written as true) still makes the run yellow.
    #[test]
    fn switches_inherited_from_a_pre_switch_save_do_not_make_the_run_yellow() {
        let plain = game(ExperimentalRules::default());
        let mut old = serde_json::to_value(&plain).unwrap();
        old.as_object_mut().unwrap().remove("run_mark");
        for key in ["bless_once", "undead_poison_immune", "unlimited_spells", "merchant_item", "rear_service_flat", "building_store"] {
            old["experimental"].as_object_mut().unwrap().remove(key);
        }
        let mut loaded: Game = serde_json::from_value(old.clone()).unwrap();
        assert!(loaded.experimental.bless_once, "the old rule still plays");
        loaded.note_settings(None);
        assert_eq!(loaded.run_mark(), RunMark::unknown_history(), "an inherited switch is not the player's choice");
        let mut chosen = old;
        chosen["experimental"]["bless_once"] = serde_json::json!(true);
        let mut chosen: Game = serde_json::from_value(chosen).unwrap();
        chosen.note_settings(None);
        assert_eq!(chosen.run_mark(), mark(Level::Yellow, Reason::Rule("Bless once a turn".into())));
    }

    #[test]
    fn a_load_or_a_battle_s_restart_in_the_same_run_keeps_what_it_used() {
        let mut now = game(ExperimentalRules::default());
        now.note_cheat("gold");
        now.keep_run_mark(Some(&RunMark::start()));
        assert_eq!(now.run_mark().level, Level::Red, "a clean save does not clear the cheat");
        let mut other = game(ExperimentalRules::default());
        other.keep_run_mark(Some(&mark(Level::Grey, Reason::Setting("Keep formation after battle".into()))));
        assert_eq!(other.run_mark().level, Level::Grey);
        other.keep_run_mark(None);
        assert_eq!(other.run_mark().level, Level::Grey);
    }

    #[test]
    fn dates_come_from_the_day_count() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_723), (2024, 1, 1));
        assert_eq!(civil(19_782), (2024, 2, 29), "a leap day");
        assert_eq!(civil(20_454), (2026, 1, 1));
    }
}
