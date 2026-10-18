//! The language: the player's choice kept in `settings.json` (next to `audio.json` in the
//! save folder), the language list (a drop-down) and its F2 key. Russian when nothing is
//! saved (English when no font with Cyrillic was found). The same file remembers that the
//! tutorial is done, and the scenario list's marks and view (Razdor's): the scenarios won,
//! the player's favourites, and how the list is filtered and sorted.
//!
//! The list (Razdor's own, asked for by the owner): «Русский» (the game as it is), «English»,
//! then each translation of the game data found in the install (`razdor::dt::english::discover`:
//! `<Prefix>_Units.ini` files made by razdor-translate, named by their first line). Razdor's
//! own interface exists in English and Russian only: Russian for «Русский», English for every
//! other choice; the game's data texts (unit, item and spell names and descriptions, the
//! game's interface texts) come from the chosen language's files.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use macroquad::prelude::*;
use razdor::dt::english::{DataLanguage, ENGLISH};
use razdor::i18n::{self, tr, Lang};
use serde::{Deserialize, Serialize};

use super::widgets::*;

/// The key that steps through the language list on every screen.
pub const KEY: KeyCode = KeyCode::F2;

/// The drop-down list's key ([`super::widgets::open_list`]).
const LIST: &str = "razdor:language";

/// Settings of the game that are not about sound, saved in `settings.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "ru", "en", or the prefix of a translation's files ("Hindi-native"); empty: never chosen.
    pub language: String,
    /// The tutorial's last event finished (its title test holds `end_tutorial`): the original
    /// writes `[Tutorial] Completed=1` into its language ini (0x4ac9fc); Razdor keeps it here.
    pub tutorial_completed: bool,
    /// The maps whose victory was reached (file names without the extension), for the
    /// scenario list's marks (Razdor's, a players' wish; the original keeps no such record).
    pub completed_scenarios: Vec<String>,
    /// The maps the player marked as favourites in the scenario list (Razdor's).
    pub favourite_scenarios: Vec<String>,
    /// The scenario list's filter and order (`ui::new_game`; 0, 0: all, as the original).
    pub scenario_show: i64,
    pub scenario_order: i64,
}

impl Settings {
    fn path() -> Option<PathBuf> {
        razdor::rules::save::default_dir().map(|d| d.join("settings.json"))
    }

    pub fn load() -> Settings {
        let read = Settings::path().and_then(|p| std::fs::read(p).ok());
        read.and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
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
        json["completed_scenarios"] = serde_json::json!(self.completed_scenarios);
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

/// Notes that the victory of map `file` (its name without the extension) was reached, for the
/// scenario list's mark; written at once.
pub fn mark_completed(file: &str) {
    let mut s = Settings::load();
    if add_completed(&mut s, file) {
        s.save();
    }
}

/// Adds `file` to the maps won, unless it is there already (any case). True when added.
fn add_completed(s: &mut Settings, file: &str) -> bool {
    if file.is_empty() || s.completed_scenarios.iter().any(|f| f.to_lowercase() == file.to_lowercase()) {
        return false;
    }
    s.completed_scenarios.push(file.to_string());
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

/// The data language to start in from the saved code: "ru" Russian, "en" English (the codes
/// of the EN / RU switch before the list), another code that translation; nothing saved:
/// Russian (English without a Cyrillic font, which would only show transliterations).
pub fn initial(saved: &Settings, cyrillic_font: bool) -> Option<String> {
    let code = saved.language.trim();
    match Lang::from_code(code) {
        Some(Lang::Ru) => None,
        Some(Lang::En) => Some(ENGLISH.into()),
        None if code.is_empty() => (!cyrillic_font).then(|| ENGLISH.into()),
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

/// At startup: sets the saved language (a translation is checked once the install is known,
/// [`set_install`]).
pub fn init() {
    set(initial(&Settings::load(), has_font()));
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

    fn found() -> Vec<DataLanguage> {
        vec![DataLanguage { prefix: "Hindi-native".into(), name: "Hindi (native letters)".into() }]
    }

    #[test]
    fn russian_unless_chosen_otherwise() {
        let s = |code: &str| Settings { language: code.into(), ..Settings::default() };
        assert_eq!(initial(&Settings::default(), true), None, "the default: Russian");
        assert_eq!(initial(&Settings::default(), false).as_deref(), Some("Eng"), "no Cyrillic font");
        assert_eq!(initial(&s("en"), true).as_deref(), Some("Eng"), "the old EN of the switch");
        assert_eq!(initial(&s("ru"), false), None, "the old RU: a choice is kept");
        assert_eq!(initial(&s("Hindi-native"), true).as_deref(), Some("Hindi-native"));
        let s: Settings = serde_json::from_str(r#"{"language":"en","other":1,"tutorial_offered":true}"#).unwrap();
        assert_eq!(s.language, "en");
        assert!(!s.tutorial_completed, "older files: the tutorial is not done (an offer seen does not count)");
        assert!(s.completed_scenarios.is_empty() && s.favourite_scenarios.is_empty(), "older files: nothing won, no favourites");
    }

    #[test]
    fn a_won_map_is_noted_once() {
        let mut s = Settings::default();
        assert!(add_completed(&mut s, "Тихая пристань"));
        assert!(!add_completed(&mut s, "тихая пристань"), "once");
        assert!(!add_completed(&mut s, ""));
        assert_eq!(s.completed_scenarios, ["Тихая пристань"]);
        let back: Settings = serde_json::from_value(serde_json::to_value(&s).unwrap()).unwrap();
        assert_eq!(back, s);
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
