//! The language: the player's choice kept in `settings.json` (next to `audio.json` in the
//! save folder), the language list (a drop-down) and its F2 key. Nothing saved: the system's
//! language decides (`lang_for_locale`; English when no font with Cyrillic
//! was found). The same file remembers that the tutorial is done, and the scenario list's
//! marks and view (Razdor's): the scenarios won, the player's favourites, and how the list is
//! filtered and sorted.
//!
//! The list (Razdor's own, asked for by the owner): «Русский» (the game as it is), «English»,
//! then each translation of the game data found in the install (`razdor::dt::english::discover`:
//! `<Prefix>_Units.ini` files made by razdor-translate, named by their first line). Razdor's
//! own interface exists in English and Russian only: Russian for «Русский», English for every
//! other choice; the game's data texts (unit, item and spell names and descriptions, the
//! game's interface texts) come from the chosen language's files.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use macroquad::prelude::*;
use razdor::dt::english::{DataLanguage, ENGLISH};
use razdor::i18n::{self, tr, Lang};
use serde::{Deserialize, Serialize};

use super::widgets::*;

/// The drop-down list's key ([`super::widgets::open_list`]).
const LIST: &str = "razdor:language";

/// Settings of the game that are not about sound, saved in `settings.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "ru", "en", or the prefix of a translation's files ("Hindi-native"); empty: never chosen
    /// (the system's language is used, and nothing is written until the player picks one).
    pub language: String,
    /// The tutorial's last event finished (its title test holds `end_tutorial`): the original
    /// writes `[Tutorial] Completed=1` into its language ini (0x4ac9fc); Razdor keeps it here.
    pub tutorial_completed: bool,
    /// The maps whose victory was reached before the ticks were kept by mod (1456): the no-mod
    /// list. `Settings::load` moves it into `completed_by_mod[NO_MOD]`; `save` writes it back too,
    /// so an older version still shows its ticks.
    pub completed_scenarios: Vec<String>,
    /// The maps whose victory was reached, by the mod played then (`NO_MOD` for the base game):
    /// file names without the extension, for the scenario list's ticks (Razdor's, a players'
    /// wish; the original keeps no such record). A tick belongs to its mod: see `won_under`.
    pub completed_by_mod: Won,
    /// The maps the player marked as favourites in the scenario list (Razdor's).
    pub favourite_scenarios: Vec<String>,
    /// The scenario list's filter and order (`ui::new_game`; 0, 0: all, as the original).
    pub scenario_show: i64,
    pub scenario_order: i64,
    /// The experimental scenario rules last set in the rules window (`ui::new_game`), kept between
    /// games (Razdor's, a players' wish); all off on a file without them, and a key a file lacks
    /// is off too (see [`remembered_rules`]).
    #[serde(deserialize_with = "remembered_rules")]
    pub experimental_rules: razdor::rules::content::ExperimentalRules,
}

/// The remembered rules: a switch whose key the file lacks is off, a new game's default. (A save's
/// missing key reads ON for the five parity switches, the rule the save had then: `RulesWire`.)
fn remembered_rules<'de, D: serde::Deserializer<'de>>(d: D) -> Result<razdor::rules::content::ExperimentalRules, D::Error> {
    use serde::de::Error;
    let mut v = <serde_json::Value as serde::Deserialize>::deserialize(d)?;
    if let Some(o) = v.as_object_mut() {
        for k in ["bless_once", "undead_poison_immune", "unlimited_spells", "merchant_item", "rear_service_flat", "building_store"] {
            o.entry(k).or_insert(serde_json::Value::Bool(false));
        }
    }
    serde_json::from_value(v).map_err(D::Error::custom)
}

/// The maps won, by mod: the mod's id (`hub::active_mod`), or `NO_MOD` for the base game.
pub type Won = BTreeMap<String, Vec<String>>;

/// The key of the base game's ticks in `Won` (a hub mod's id is a safe name and never this).
pub const NO_MOD: &str = "none";

impl Settings {
    fn path() -> Option<PathBuf> {
        razdor::rules::save::default_dir().map(|d| d.join("settings.json"))
    }

    pub fn load() -> Settings {
        let read = Settings::path().and_then(|p| std::fs::read(p).ok());
        let mut s: Settings = read.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // Ticks kept before they were per mod count for the base game (1456).
        for file in std::mem::take(&mut s.completed_scenarios) {
            add_won(&mut s.completed_by_mod, None, &file);
        }
        s
    }

    /// Writes the settings, keeping fields of the file this version does not know.
    pub fn save(&self) {
        let Some(path) = Settings::path() else { return };
        let mut json = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .filter(serde_json::Value::is_object)
            .unwrap_or_else(|| serde_json::json!({}));
        json["language"] = serde_json::Value::String(self.language.clone());
        json["tutorial_completed"] = serde_json::Value::Bool(self.tutorial_completed);
        json["completed_by_mod"] = serde_json::json!(self.completed_by_mod);
        // The base game's list stays in the old field too, for an older version.
        json["completed_scenarios"] = serde_json::json!(won_under(&self.completed_by_mod, None));
        json["favourite_scenarios"] = serde_json::json!(self.favourite_scenarios);
        json["scenario_show"] = serde_json::json!(self.scenario_show);
        json["scenario_order"] = serde_json::json!(self.scenario_order);
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            std::fs::write(&path, serde_json::to_vec_pretty(&json).unwrap_or_default())
        });
        if let Err(e) = written {
            razdor::diag!("{}: {e}", path.display());
        }
    }
}

/// Notes that the victory of map `file` (its name without the extension) was reached under the
/// mod `mod_id` (`None`: the base game), for the scenario list's tick; written at once.
pub fn mark_completed(mod_id: Option<&str>, file: &str) {
    let mut s = Settings::load();
    if add_won(&mut s.completed_by_mod, mod_id, file) {
        s.save();
    }
}

/// The maps won under `mod_id` (`None`: the base game): that mod's ticks only.
pub fn won_under<'a>(won: &'a Won, mod_id: Option<&str>) -> &'a [String] {
    won.get(mod_id.unwrap_or(NO_MOD)).map_or(&[], Vec::as_slice)
}

/// Adds `file` to the maps won under `mod_id`, unless it is there already (any case). True when added.
pub fn add_won(won: &mut Won, mod_id: Option<&str>, file: &str) -> bool {
    let list = won.entry(mod_id.unwrap_or(NO_MOD).to_string()).or_default();
    if file.is_empty() || list.iter().any(|f| f.to_lowercase() == file.to_lowercase()) {
        return false;
    }
    list.push(file.to_string());
    true
}

/// One entry of the language list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    /// The data language: the prefix of its files (`Eng`, `Hindi-native`); `None`: the
    /// install's Russian.
    pub data: Option<String>,
    pub label: String,
}

/// Razdor's interface language for data language `data`: Russian only for the game as it is.
pub fn interface_of(data: Option<&str>) -> Lang {
    if data.is_none() { Lang::Ru } else { Lang::En }
}

/// The list: Russian, English, then the translations `found` in the install.
pub fn choices_with(found: &[DataLanguage]) -> Vec<Choice> {
    let mut out = vec![Choice { data: None, label: "Русский".into() }, Choice { data: Some(ENGLISH.into()), label: "English".into() }];
    out.extend(found.iter().map(|l| Choice { data: Some(l.prefix.clone()), label: l.name.clone() }));
    out
}

/// The code kept in the settings for a data language: "ru", "en", or the files' prefix.
pub fn code_of(data: Option<&str>) -> String {
    match data {
        None => Lang::Ru.code().to_string(),
        Some(p) if p.eq_ignore_ascii_case(ENGLISH) => Lang::En.code().to_string(),
        Some(p) => p.to_string(),
    }
}

/// The interface language for a system locale (a BCP 47 tag, or a POSIX one such as
/// `ru_RU.UTF-8`): Russian for Russian, Ukrainian, Belarusian and Kazakh, English for every
/// other language, and for no locale at all (`""`, `C`, `POSIX`). Only the two exist.
pub fn lang_for_locale(locale: &str) -> Lang {
    let primary = locale.trim().split(['-', '_', '.', '@']).next().unwrap_or_default();
    if ["ru", "uk", "be", "kk"].iter().any(|l| primary.eq_ignore_ascii_case(l)) { Lang::Ru } else { Lang::En }
}

/// The system's interface language; English when the system's locale cannot be read.
pub fn system_lang() -> Lang {
    system_locale().map_or(Lang::En, |l| lang_for_locale(&l))
}

/// The system's locale: on Android the JNI's `Locale.getDefault()` (`ui::android::read_locale`),
/// then `sys-locale` when that fails; `sys-locale` everywhere else.
fn system_locale() -> Option<String> {
    #[cfg(target_os = "android")]
    if let Some(tag) = super::android::read_locale() {
        return Some(tag);
    }
    sys_locale::get_locale()
}

/// The data language to start in from the saved code: "ru" Russian, "en" English (the codes
/// of the EN / RU switch before the list), another code that translation. Nothing saved: the
/// system's language (`system`), Russian for Russian; English without a Cyrillic font, which
/// would only show transliterations. Nothing is written: a later change of the system's
/// language still applies until the player picks one.
pub fn initial(saved: &Settings, cyrillic_font: bool, system: Lang) -> Option<String> {
    let code = saved.language.trim();
    match Lang::from_code(code) {
        Some(Lang::Ru) => None,
        Some(Lang::En) => Some(ENGLISH.into()),
        None if code.is_empty() => (system == Lang::En || !cyrillic_font).then(|| ENGLISH.into()),
        None => Some(code.to_string()),
    }
}

/// `data` as one of `choices` (its prefix as the files spell it); a translation that is not
/// there (its files gone) falls back to English.
pub fn resolve(data: Option<String>, choices: &[Choice]) -> Option<String> {
    let p = data?;
    choices
        .iter()
        .filter_map(|c| c.data.as_ref())
        .find(|d| d.eq_ignore_ascii_case(&p))
        .cloned()
        .or_else(|| Some(ENGLISH.into()))
}

/// The choice after `data` in `choices`, round (F2).
pub fn next_after(data: Option<&str>, choices: &[Choice]) -> usize {
    let at = choices.iter().position(|c| c.data.as_deref() == data).unwrap_or(0);
    (at + 1) % choices.len().max(1)
}

struct State {
    data: Option<String>,
    found: Vec<DataLanguage>,
}

static STATE: Mutex<State> = Mutex::new(State { data: None, found: Vec::new() });

fn state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap_or_else(|e| e.into_inner()))
}

/// The language list now.
pub fn choices() -> Vec<Choice> {
    state(|s| choices_with(&s.found))
}

/// The data language: the prefix of its files (`Eng`, `Hindi-native`); `None`: the install's
/// Russian.
pub fn data_prefix() -> Option<String> {
    state(|s| s.data.clone())
}

fn set(data: Option<String>) {
    i18n::set_lang(interface_of(data.as_deref()));
    state(|s| s.data = data);
}

/// At startup: sets the saved language, or the system's when none is saved (a translation is
/// checked once the install is known, [`set_install`]).
pub fn init() {
    set(initial(&Settings::load(), has_font(), system_lang()));
}

/// The install at `dir`: its translations join the list; a saved one it lacks becomes English.
pub fn set_install(dir: &Path) {
    let found = razdor::dt::english::discover(dir);
    let data = resolve(data_prefix(), &choices_with(&found));
    state(|s| s.found = found);
    set(data);
}

/// Picks entry `i` of the list and saves the choice.
pub fn choose(i: usize) {
    let Some(c) = choices().into_iter().nth(i) else { return };
    let code = code_of(c.data.as_deref());
    set(c.data);
    Settings { language: code, ..Settings::load() }.save();
}

/// The next language of the list (F2).
pub fn cycle() {
    let list = choices();
    choose(next_after(data_prefix().as_deref(), &list));
}

/// The current choice's name.
pub fn current_label() -> String {
    let data = data_prefix();
    choices().into_iter().find(|c| c.data == data).map_or_else(|| code_of(data.as_deref()), |c| c.label)
}

/// What the list's hint says.
pub fn hint() -> &'static str {
    tr("Language (F2). Razdor's own texts exist in English and Russian only: with another language the game's texts (units, items, spells, its windows) are in it, the rest in English.")
}

/// Opens the language list under (or over) `anchor`, sized by `scale`.
pub fn open_list(anchor: Rect, scale: f32) {
    let data = data_prefix();
    let list = choices();
    let current = list.iter().position(|c| c.data == data).unwrap_or(0) as i64;
    let options: Vec<(i64, String)> = list.into_iter().enumerate().map(|(i, c)| (i as i64, c.label)).collect();
    super::widgets::open_list(LIST, anchor, current, &options, scale);
}

/// The language list is open.
pub fn list_open() -> bool {
    super::widgets::list_open(LIST)
}

/// Takes a choice made in the list (once a frame, wherever the list was opened).
pub fn follow_pick() {
    if let Some(i) = take_pick(LIST) {
        choose(i as usize);
    }
}

/// The language box (the translator's window, the scenario list): the current language; a
/// click opens the list.
pub fn picker_box(r: Rect) {
    let (hover, down, pressed) = super::widgets::button_state(r, true);
    draw_rectangle(r.x, r.y, r.w, r.h, PANEL);
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, if down || list_open() { ACCENT } else if hover { INK } else { DIM });
    let size = (r.h * 0.55).clamp(12.0, 22.0);
    let tw = r.w - 30.0;
    text_fit(&current_label(), r.x + 8.0, r.y + r.h / 2.0 + size * 0.35, tw, size, ACCENT);
    let (cx, cy) = (r.x + r.w - 13.0, r.y + r.h / 2.0);
    draw_triangle(vec2(cx - 5.0, cy - 3.0), vec2(cx + 5.0, cy - 3.0), vec2(cx, cy + 4.0), DIM);
    if hover {
        tooltip(&[(hint().to_string(), INK)]);
    }
    if pressed {
        super::audio::cue(super::audio::Cue::Button);
        open_list(r, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_remembered_experimental_rules_round_trip_through_the_settings_file() {
        let rules = razdor::rules::content::ExperimentalRules { unlimited_items: true, six_cells: true, ..Default::default() };
        let saved = serde_json::to_string(&Settings { experimental_rules: rules, ..Settings::default() }).unwrap();
        let back: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(back.experimental_rules, rules);
    }

    #[test]
    fn a_settings_file_from_before_the_parity_switches_reads_them_off() {
        let old = r#"{"experimental_rules": {"fast_ai": true, "six_cells": true}}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert!(s.experimental_rules.fast_ai && s.experimental_rules.six_cells);
        assert!(!s.experimental_rules.bless_once && !s.experimental_rules.rear_service_flat);
        assert!(!s.experimental_rules.building_store);
    }

    #[test]
    fn a_settings_file_without_the_experimental_rules_reads_all_off() {
        let old = r#"{"language":"ru","scenario_show":2,"scenario_order":1,"favourite_scenarios":["Tut"]}"#;
        let s: Settings = serde_json::from_str(old).unwrap();
        assert_eq!(s.experimental_rules, razdor::rules::content::ExperimentalRules::default());
        assert_eq!(s.scenario_show, 2);
    }

    #[test]
    fn the_system_locale_picks_russian_or_english() {
        for ru in ["ru-RU", "ru_RU.UTF-8", "ru", "RU", "uk-UA", "uk_UA.UTF-8", "be-BY", "kk-KZ"] {
            assert_eq!(lang_for_locale(ru), Lang::Ru, "{ru}");
        }
        for en in ["en-US", "en_GB.UTF-8", "de-DE", "es", "", "C", "POSIX", "C.UTF-8", "ky-KG", "uz", "rux-XX", "ruler"] {
            assert_eq!(lang_for_locale(en), Lang::En, "{en:?}");
        }
        assert_eq!(lang_for_locale("  ru-RU  "), Lang::Ru, "blanks around it");
        assert_eq!(lang_for_locale("sr@latin"), Lang::En);
    }

    fn found() -> Vec<DataLanguage> {
        vec![DataLanguage { prefix: "Hindi-native".into(), name: "Hindi (native letters)".into() }]
    }

    #[test]
    fn the_system_language_decides_until_the_player_chooses() {
        let s = |code: &str| Settings { language: code.into(), ..Settings::default() };
        assert_eq!(initial(&Settings::default(), true, Lang::Ru), None, "a Russian system: Russian");
        assert_eq!(initial(&Settings::default(), true, Lang::En).as_deref(), Some("Eng"), "any other system: English");
        assert_eq!(initial(&Settings::default(), false, Lang::Ru).as_deref(), Some("Eng"), "no Cyrillic font");
        assert_eq!(initial(&s("en"), true, Lang::Ru).as_deref(), Some("Eng"), "the old EN of the switch");
        assert_eq!(initial(&s("ru"), false, Lang::En), None, "the old RU: a choice is kept");
        assert_eq!(initial(&s("en"), true, Lang::Ru).as_deref(), Some("Eng"), "a saved choice beats the system's");
        assert_eq!(initial(&s("Hindi-native"), true, Lang::Ru).as_deref(), Some("Hindi-native"));
        let s: Settings = serde_json::from_str(r#"{"language":"en","other":1,"tutorial_offered":true}"#).unwrap();
        assert_eq!(s.language, "en");
        assert!(!s.tutorial_completed, "older files: the tutorial is not done (an offer seen does not count)");
        assert!(s.completed_scenarios.is_empty() && s.favourite_scenarios.is_empty(), "older files: nothing won, no favourites");
    }

    #[test]
    fn a_won_map_is_noted_once() {
        let mut s = Settings::default();
        assert!(add_won(&mut s.completed_by_mod, None, "Тихая пристань"));
        assert!(!add_won(&mut s.completed_by_mod, None, "тихая пристань"), "once");
        assert!(!add_won(&mut s.completed_by_mod, None, ""));
        assert_eq!(won_under(&s.completed_by_mod, None), ["Тихая пристань"]);
        let back: Settings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }

    #[test]
    fn a_tick_is_recorded_and_read_per_mod() {
        let mut won = Won::new();
        assert!(add_won(&mut won, None, "Тихая пристань"));
        assert!(add_won(&mut won, Some("evolution"), "Тихая пристань"), "the same map under a mod is its own tick");
        assert!(add_won(&mut won, Some("evolution"), "Осада"));
        assert!(!add_won(&mut won, Some("evolution"), "осада"), "once per mod");
        assert_eq!(won_under(&won, None), ["Тихая пристань"]);
        assert_eq!(won_under(&won, Some("evolution")), ["Тихая пристань", "Осада"]);
        assert!(won_under(&won, Some("other")).is_empty(), "a mod never won shows no tick");
        let back: Won = serde_json::from_value(serde_json::to_value(&won).unwrap()).unwrap();
        assert_eq!(back, won, "the file's format keeps each mod's list");
    }

    #[test]
    fn old_ticks_read_as_the_base_game() {
        // A settings file from before 1.2: the flat list only, no per-mod field.
        let old: Settings = serde_json::from_str(r#"{"completed_scenarios":["Тихая пристань","Осада"],"language":"ru"}"#).unwrap();
        assert!(old.completed_by_mod.is_empty());
        let mut merged = old.clone();
        for file in std::mem::take(&mut merged.completed_scenarios) {
            add_won(&mut merged.completed_by_mod, None, &file);
        }
        assert_eq!(won_under(&merged.completed_by_mod, None), ["Тихая пристань", "Осада"], "old entries: no mod");
        assert!(won_under(&merged.completed_by_mod, Some("evolution")).is_empty(), "no base-game tick under a mod");
        // A file written by this version has both fields: the merge does not double the entries.
        let both: Settings = serde_json::from_str(r#"{"completed_scenarios":["Осада"],"completed_by_mod":{"none":["Осада"]}}"#).unwrap();
        let mut merged = both.clone();
        for file in std::mem::take(&mut merged.completed_scenarios) {
            add_won(&mut merged.completed_by_mod, None, &file);
        }
        assert_eq!(won_under(&merged.completed_by_mod, None), ["Осада"]);
    }

    #[test]
    fn the_list_and_its_codes() {
        let list = choices_with(&found());
        let labels: Vec<&str> = list.iter().map(|c| c.label.as_str()).collect();
        assert_eq!(labels, ["Русский", "English", "Hindi (native letters)"]);
        assert_eq!(list.iter().map(|c| interface_of(c.data.as_deref())).collect::<Vec<_>>(), [Lang::Ru, Lang::En, Lang::En], "Razdor's own texts: Russian or English");
        assert_eq!(list.iter().map(|c| code_of(c.data.as_deref())).collect::<Vec<_>>(), ["ru", "en", "Hindi-native"], "the old codes stay");
        assert_eq!(resolve(Some("hindi-NATIVE".into()), &list).as_deref(), Some("Hindi-native"));
        assert_eq!(resolve(Some("German".into()), &list).as_deref(), Some("Eng"), "a translation gone: English");
        assert_eq!(resolve(None, &list), None);
        assert_eq!(next_after(None, &list), 1);
        assert_eq!(next_after(Some("Eng"), &list), 2);
        assert_eq!(next_after(Some("Hindi-native"), &list), 0, "F2 goes round");
    }
}
