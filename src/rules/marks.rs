//! The run's mark (Razdor's, backlog 1532, the owner's decisions): the worst thing a run has used,
//! shown as a coloured checkmark on the scenario list. A run is a game from the map's start; its
//! mark rises and never falls: a console cheat (red), a «Читы» switch (yellow), a gameplay setting
//! a «Исправления» switch or «Максимум» (green with a «Р»), else green. Grey is retired (2026-11-23): an old grey mark is re-graded on read (`regrade`). The run keeps it in its save (`Game::run_mark`),
//! and a victory writes the map's best mark ever into the player's `marks.json` (`Records`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use web_time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::save;

/// A level name this build does not know (one from a newer build, in a file or a cloud payload)
/// reads as yellow, the safe «not clean» level, so one unknown mark never drops the other marks.
impl<'de> Deserialize<'de> for Level {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Any JSON value, not only a string: an odd level must not fail the whole file.
        let name = serde_json::Value::deserialize(deserializer)?;
        Ok(match name.as_str() {
            Some("green") => Level::Green,
            Some("fixes") => Level::Fixes,
            Some("grey") => Level::Grey,
            Some("red") => Level::Red,
            _ => Level::Yellow,
        })
    }
}

/// The mark's colour, best first. The order is how bad it is: the worst mark is the greatest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// No cheat, no experimental rule, every gameplay setting the original's. A run whose history
    /// is unknown (a save from before the marks) is green too, unless its save shows otherwise.
    Green,
    /// Only Razdor's fixes are on (the «Исправления» group, any subset): the run is green with a
    /// «Р» and counts as completed. Added 2026-11-22; a run with a cheat is yellow, not this.
    Fixes,
    /// Retired (2026-11-23): nothing produces it. An old grey mark is re-graded on read by
    /// [`regrade`]; the level stays in the code and on the wire for old files and clients.
    Grey,
    /// A cheat (an experimental «Читы» switch) is on. Also what an unknown level name reads as
    /// (see the `Deserialize` impl above).
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
    /// Every gameplay setting that is not the original's, as the run used it (backlog 1678): the
    /// setting's catalog name and its value's catalog key, one entry per setting, its most recent
    /// non-original value. Empty in a save or a record from before it; the tooltip then names only
    /// `reason`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<(String, String)>,
}

impl RunMark {
    /// A run at its start: nothing used yet.
    pub fn start() -> RunMark {
        RunMark { level: Level::Green, reason: Reason::Honest, settings: Vec::new() }
    }

    /// A save from before the mark: its history is unknown. The owner's decision (2026-11-13): it
    /// is green, as a fair run; only what the run uses after that, or what its save shows (a cheat,
    /// an experimental rule, a changed setting), lowers it.
    pub fn unknown_history() -> RunMark {
        RunMark { level: Level::Green, reason: Reason::Unknown, settings: Vec::new() }
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
        for (name, value) in &earlier.settings {
            if !self.settings.iter().any(|(n, _)| n == name) {
                self.settings.push((name.clone(), value.clone()));
            }
        }
    }

    /// Notes a gameplay setting that is not the original's, with its value now (`value`, a catalog
    /// key): one entry per setting, the most recent value (`settings`). The run's own list wins over
    /// an earlier one in [`RunMark::absorb`], since the run's values are read again every frame.
    pub fn note_setting(&mut self, name: &str, value: &str) {
        match self.settings.iter_mut().find(|(n, _)| n == name) {
            Some(entry) => entry.1 = value.to_string(),
            None => self.settings.push((name.to_string(), value.to_string())),
        }
    }
}

/// The gameplay settings a grey mark may name (`ui::audio::Settings::gameplay_differs_from_original`
/// before 2026-11-23, by their catalog names): DT's own options, the convenience «Сохранять расстановку»
/// and the retired «Unit battle participation». DT's options never count, so these re-grade to green.
const DT_SETTINGS: [&str; 5] = [
    "Wide front row in battle (for new games)",
    "Impossible difficulty (needs a restart)",
    "Automatically pursue the chosen army",
    "Unit battle participation",
    "Keep formation after battle",
];
/// Kept XP on promotion: the setting's catalog name, and the name an older build saved. It is a fix now.
const KEPT_XP_SETTINGS: [&str; 2] = ["Experience on promotion", "Experience on promotion: kept"];
/// The battle AI's strength, and «Максимум» (Razdor's; a fix, so green with a «Р»).
const AI_SETTING: &str = "Battle AI strength";
const MAXIMUM: &str = "Maximum (Razdor)";

/// Re-grades a grey mark of a run or of a record (grey is retired, 2026-11-23; nothing new produces it).
/// Pure, so the saves and `marks.json` read the same. Only grey changes; the dates are the caller's.
///
/// With the list of settings (`RunMark::settings`): «Максимум» or kept XP makes it `Fixes`, named by that
/// setting; a list of DT's own settings only (and the AI at «Улучшенный» or «Как в оригинале») makes it
/// green, `Honest`. Without a list (a record from before it) the reason's name decides: kept XP and the
/// AI (its value unknown: the cautious reading, «Максимум» may have been used) are `Fixes`, DT's names
/// are green, and an unknown history stays green. A name nothing knows is `Fixes`, never green.
pub fn regrade(mut mark: RunMark) -> RunMark {
    if mark.level != Level::Grey {
        return mark;
    }
    let kept_xp = |name: &str| KEPT_XP_SETTINGS.contains(&name);
    let dt = |name: &str| DT_SETTINGS.contains(&name) || name == AI_SETTING;
    let (level, reason) = if mark.settings.is_empty() {
        match &mark.reason {
            Reason::Setting(name) if kept_xp(name) || name == AI_SETTING => (Level::Fixes, mark.reason.clone()),
            Reason::Setting(name) if dt(name) => (Level::Green, Reason::Honest),
            Reason::Unknown => (Level::Green, Reason::Unknown),
            _ => (Level::Fixes, mark.reason.clone()),
        }
    } else if let Some((name, _)) = mark.settings.iter().find(|(name, value)| (name == AI_SETTING && value == MAXIMUM) || kept_xp(name)) {
        (Level::Fixes, Reason::Setting(name.clone()))
    } else if mark.settings.iter().all(|(name, _)| dt(name)) {
        (Level::Green, Reason::Honest)
    } else {
        (Level::Fixes, mark.reason.clone())
    };
    mark.level = level;
    mark.reason = reason;
    mark
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
    /// Every map's record, by its key (the file name lower-cased).
    pub fn entries(&self) -> impl Iterator<Item = (&String, &Record)> {
        self.maps.iter()
    }

    /// Sets map `key`'s record (the cloud's merged marks, `cloud_store`).
    pub fn put_key(&mut self, key: String, record: Record) {
        self.maps.insert(key, record);
    }

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
            record.mark = regrade(record.mark.clone());
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
        RunMark { level, reason, settings: Vec::new() }
    }

    /// A mark of the run that noted «Максимум» (Razdor's, a fix: green with a «Р»).
    fn max_ai(level: Level) -> RunMark {
        RunMark {
            level,
            reason: Reason::Setting("Battle AI strength".into()),
            settings: vec![("Battle AI strength".into(), "Maximum (Razdor)".into())],
        }
    }

    /// A mark as an old build or the regrade reads it: the grey level with its reason and list.
    fn grey(reason: Reason, settings: &[(&str, &str)]) -> RunMark {
        RunMark { level: Level::Grey, reason, settings: settings.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect() }
    }

    /// The level and reason `regrade` gives a mark.
    fn regraded(m: RunMark) -> (Level, Reason) {
        let r = regrade(m);
        (r.level, r.reason)
    }

    #[test]
    fn a_grey_mark_of_dt_s_own_settings_is_green_when_re_graded() {
        let list = [("Wide front row in battle (for new games)", "on (setting value)"), ("Impossible difficulty (needs a restart)", "on (setting value)"), ("Battle AI strength", "As in the original")];
        assert_eq!(regraded(grey(Reason::Setting("Wide front row in battle (for new games)".into()), &list)), (Level::Green, Reason::Honest));
        // Without a list (a record from before it): the reason's name decides.
        assert_eq!(regraded(grey(Reason::Setting("Unit battle participation".into()), &[])), (Level::Green, Reason::Honest));
        assert_eq!(regraded(grey(Reason::Setting("Automatically pursue the chosen army".into()), &[])), (Level::Green, Reason::Honest));
    }

    #[test]
    fn a_grey_mark_that_names_maximum_or_kept_xp_is_a_fix() {
        let max = grey(Reason::Setting("Wide front row in battle (for new games)".into()), &[("Wide front row in battle (for new games)", "off (setting value)"), ("Battle AI strength", "Maximum (Razdor)")]);
        assert_eq!(regraded(max), (Level::Fixes, Reason::Setting("Battle AI strength".into())));
        let xp = grey(Reason::Setting("Experience on promotion".into()), &[("Impossible difficulty (needs a restart)", "on (setting value)"), ("Experience on promotion", "Kept (Razdor)")]);
        assert_eq!(regraded(xp), (Level::Fixes, Reason::Setting("Experience on promotion".into())));
    }

    #[test]
    fn an_old_record_without_its_list_is_read_by_its_reason_name() {
        assert_eq!(regraded(grey(Reason::Setting("Experience on promotion".into()), &[])), (Level::Fixes, Reason::Setting("Experience on promotion".into())));
        assert_eq!(regraded(grey(Reason::Setting("Battle AI strength".into()), &[])), (Level::Fixes, Reason::Setting("Battle AI strength".into())), "no value: the cautious reading");
        assert_eq!(regraded(grey(Reason::Unknown, &[])), (Level::Green, Reason::Unknown));
        assert_eq!(regraded(grey(Reason::Setting("A name nothing knows".into()), &[])), (Level::Fixes, Reason::Setting("A name nothing knows".into())), "never green");
    }

    #[test]
    fn only_grey_changes_and_a_regraded_mark_stays() {
        let yellow = mark(Level::Yellow, Reason::Rule("Six equipment cells".into()));
        assert_eq!(regrade(yellow.clone()), yellow);
        let red = mark(Level::Red, Reason::Cheat("god".into()));
        assert_eq!(regrade(red.clone()), red);
        let fixes = max_ai(Level::Fixes);
        assert_eq!(regrade(fixes.clone()), fixes);
        assert_eq!(regrade(RunMark::start()), RunMark::start());
        let once = regrade(grey(Reason::Setting("Unit battle participation".into()), &[]));
        assert_eq!(regrade(once.clone()), once, "idempotent");
    }

    #[test]
    fn a_record_on_disk_is_re_graded_and_keeps_its_date() {
        let text = r#"{"maps":{"one":{"mark":{"level":"grey","reason":{"Setting":"Battle AI strength"},"settings":[["Battle AI strength","Maximum (Razdor)"]]},"date":"2026-11-02"},"two":{"mark":{"level":"grey","reason":{"Setting":"Automatically pursue the chosen army"}},"date":"2026-11-03"}}}"#;
        let records = Records::from_json(text);
        let one = records.get("one").unwrap();
        assert_eq!((one.mark.level, one.date.as_str()), (Level::Fixes, "2026-11-02"));
        let two = records.get("two").unwrap();
        assert_eq!((two.mark.level, two.mark.reason.clone(), two.date.as_str()), (Level::Green, Reason::Honest, "2026-11-03"));
    }

    /// The order: green, then green with a «Р» (fixes only), grey, yellow, red; a fixes run is
    /// completed (not worse than a clean one) and a cheat is worse than any fix.
    #[test]
    fn the_levels_run_green_fixes_grey_yellow_red() {
        assert!(Level::Green < Level::Fixes && Level::Fixes < Level::Grey && Level::Grey < Level::Yellow && Level::Yellow < Level::Red);
        let mut m = RunMark::start();
        assert!(m.worsen(Level::Fixes, Reason::Rule("Faster AI".into())));
        assert!(!m.worsen(Level::Green, Reason::Honest), "a fixes run never goes back to green");
        assert!(m.worsen(Level::Yellow, Reason::Rule("Free units".into())));
        assert!(!m.worsen(Level::Fixes, Reason::Rule("Faster AI".into())), "a fix never lowers a cheat's yellow");
        assert_eq!(m.reason, Reason::Rule("Free units".into()));
    }

    /// Marks written before the «Исправления» level still load (their levels keep their names), and
    /// a fixes mark round-trips under its own name.
    #[test]
    fn an_old_marks_file_still_loads_and_the_fixes_level_round_trips() {
        let old = r#"{"maps":{"one":{"mark":{"level":"green","reason":"Honest"},"date":"2026-01-01"},
            "two":{"mark":{"level":"yellow","reason":{"Rule":"Free units"}},"date":"2026-01-02"},
            "three":{"mark":{"level":"grey","reason":{"Setting":"Unit battle participation"}},"date":"2026-01-03"},
            "four":{"mark":{"level":"red","reason":{"Cheat":"god"}},"date":"2026-01-04"}}}"#;
        let records = Records::from_json(old);
        assert_eq!(records.get("one").unwrap().mark.level, Level::Green);
        assert_eq!(records.get("two").unwrap().mark.level, Level::Yellow, "a stored yellow stays yellow");
        assert_eq!(records.get("three").unwrap().mark.level, Level::Green, "a grey record re-grades on read: Unit battle participation is DT's");
        assert_eq!(records.get("four").unwrap().mark.level, Level::Red);
        let mut records = records;
        records.record_victory("one", &mark(Level::Fixes, Reason::Rule("Bless once a turn".into())), "2026-11-22");
        assert_eq!(records.get("one").unwrap().mark.level, Level::Green, "a clean record beats a fixes one");
        records.record_victory("four", &mark(Level::Fixes, Reason::Rule("Bless once a turn".into())), "2026-11-22");
        assert_eq!(records.get("four").unwrap().mark.level, Level::Fixes, "a fixes win replaces a red record");
        let back = Records::from_json(&records.to_json());
        assert_eq!(back.get("four").unwrap().mark.level, Level::Fixes);
        assert!(records.to_json().contains("\"fixes\""));
    }

    /// A level name this build does not know (from a newer build) reads as yellow, the safe «not
    /// clean» level, and the other maps' marks still load: one unknown name drops no mark.
    #[test]
    fn an_unknown_level_reads_yellow_and_keeps_the_other_marks() {
        let text = r#"{"maps":{"one":{"mark":{"level":"green","reason":"Honest"},"date":"2026-01-01"},
            "two":{"mark":{"level":"sparkly","reason":{"Rule":"Future rule"}},"date":"2026-01-02"},
            "three":{"mark":{"level":"fixes","reason":{"Rule":"Bless once a turn"}},"date":"2026-01-03"}}}"#;
        let records = Records::from_json(text);
        assert_eq!(records.get("one").unwrap().mark.level, Level::Green, "a clean mark survives");
        assert_eq!(records.get("two").unwrap().mark.level, Level::Yellow, "an unknown level is not clean");
        assert_eq!(records.get("three").unwrap().mark.level, Level::Fixes);
        assert!(Records::from_json("not json").maps.is_empty(), "a broken file is still no marks");
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
    fn absorbing_merges_the_settings_lists_and_keeps_the_run_s_own_values() {
        let mut now = mark(Level::Grey, Reason::Setting("Battle AI strength".into()));
        now.note_setting("Battle AI strength", "Maximum (Razdor)");
        let mut earlier = mark(Level::Grey, Reason::Setting("Automatically pursue the chosen army".into()));
        earlier.note_setting("Battle AI strength", "Improved");
        earlier.note_setting("Automatically pursue the chosen army", "Off");
        now.absorb(&earlier);
        assert_eq!(
            now.settings,
            vec![
                ("Battle AI strength".to_string(), "Maximum (Razdor)".to_string()),
                ("Automatically pursue the chosen army".to_string(), "Off".to_string()),
            ],
            "one entry per setting: the run's value, plus the earlier setting it did not have"
        );
        assert_eq!(now.reason, Reason::Setting("Battle AI strength".into()), "the first cause stays");
    }

    #[test]
    fn a_setting_is_listed_once_with_its_latest_value() {
        let mut m = RunMark::start();
        m.note_setting("Battle AI strength", "Improved");
        m.note_setting("Experience on promotion", "Kept (Razdor)");
        m.note_setting("Battle AI strength", "Maximum (Razdor)");
        assert_eq!(
            m.settings,
            vec![
                ("Battle AI strength".to_string(), "Maximum (Razdor)".to_string()),
                ("Experience on promotion".to_string(), "Kept (Razdor)".to_string()),
            ]
        );
    }

    #[test]
    fn a_mark_from_before_the_settings_list_loads_with_none() {
        let old = r#"{"level":"grey","reason":{"Setting":"Unit battle participation"}}"#;
        let m: RunMark = serde_json::from_str(old).expect("an old mark loads");
        assert_eq!(m, mark(Level::Grey, Reason::Setting("Unit battle participation".into())));
        assert!(m.settings.is_empty());
        assert!(!serde_json::to_string(&RunMark::start()).unwrap().contains("settings"), "an empty list is not written");
        let back: RunMark = serde_json::from_str(&serde_json::to_string(&m).unwrap()).unwrap();
        assert_eq!(back, m);
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
        kept.record_victory("РК2-Дикие пустоши", &mark(Level::Fixes, Reason::Setting("Battle AI strength".into())), "2026-11-02");
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
        g.note_settings(&[]);
        assert_eq!(g.run_mark().level, Level::Green, "the original's settings: still green");
        g.note_settings(&[("Battle AI strength", "Maximum (Razdor)")]);
        assert_eq!(g.run_mark(), max_ai(Level::Fixes));
        g.note_settings(&[]);
        assert_eq!(g.run_mark().level, Level::Fixes, "a setting changed back does not clean the run");
        g.note_cheat("god");
        assert_eq!(g.run_mark(), RunMark { settings: max_ai(Level::Fixes).settings, ..mark(Level::Red, Reason::Cheat("god".into())) });
        g.note_settings(&[("Keep formation after battle", "Off")]);
        assert_eq!(g.run_mark().level, Level::Red, "red stays red");
    }

    #[test]
    fn the_experimental_rules_of_the_playthrough_make_it_yellow() {
        let g = {
            let mut g = game(ExperimentalRules { six_cells: true, ..Default::default() });
            g.note_settings(&[]);
            g
        };
        assert_eq!(g.run_mark(), mark(Level::Yellow, Reason::Rule("Six equipment cells".into())));
        let mut g2 = game(ExperimentalRules { free_units: true, two_potions: true, ..Default::default() });
        g2.note_settings(&[("Battle AI strength", "Maximum (Razdor)")]);
        assert_eq!(g2.run_mark(), RunMark { settings: max_ai(Level::Fixes).settings, ..mark(Level::Yellow, Reason::Rule("At most two potions per character".into())) }, "the first rule on in the panel's order names it; yellow beats grey");
    }

    #[test]
    fn an_old_save_s_run_reads_grey_at_most_and_keeps_known_evidence() {
        let plain = game(ExperimentalRules::default());
        let mut old = serde_json::to_value(&plain).unwrap();
        old.as_object_mut().unwrap().remove("run_mark");
        let mut loaded: Game = serde_json::from_value(old.clone()).unwrap();
        assert!(loaded.run_mark.is_none());
        assert_eq!(loaded.run_mark(), RunMark::unknown_history());
        loaded.note_settings(&[]);
        assert_eq!(loaded.run_mark().level, Level::Green, "unknown history alone is green");
        let mut changed: Game = serde_json::from_value(old.clone()).unwrap();
        changed.note_settings(&[("Battle AI strength", "Maximum (Razdor)")]);
        assert_eq!(changed.run_mark(), max_ai(Level::Fixes), "a changed setting still makes it a fix");
        // A save that records a cheat or a rule keeps that, as its history is known.
        let mut cheated = old.clone();
        cheated["cheats"]["used"] = serde_json::json!(true);
        let mut cheated: Game = serde_json::from_value(cheated).unwrap();
        cheated.note_settings(&[]);
        assert_eq!(cheated.run_mark(), mark(Level::Red, Reason::Cheat(String::new())));
        let mut ruled = old.clone();
        ruled["experimental"]["six_cells"] = serde_json::json!(true);
        let mut ruled: Game = serde_json::from_value(ruled).unwrap();
        ruled.note_settings(&[]);
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
        clean.note_settings(&[]);
        assert_eq!(clean.run_mark(), RunMark::unknown_history());
        assert_eq!(clean.run_mark().level, Level::Green);
        let mut cheated = old;
        cheated["cheats"]["used"] = serde_json::json!(true);
        let mut cheated: Game = serde_json::from_value(cheated).unwrap();
        cheated.note_settings(&[]);
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
        loaded.note_settings(&[]);
        assert_eq!(loaded.run_mark(), RunMark::unknown_history(), "an inherited switch is not the player's choice");
        let mut chosen = old;
        chosen["experimental"]["bless_once"] = serde_json::json!(true);
        let mut chosen: Game = serde_json::from_value(chosen).unwrap();
        chosen.note_settings(&[]);
        assert_eq!(chosen.run_mark(), mark(Level::Fixes, Reason::Rule("Bless once a turn".into())), "a chosen fix is the green “Р” mark");
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
