//! The scenario settings window: title and description, start date, victory and defeat
//! events, the three hero presets, the faction relations, campaign settings and the named
//! characters.

use macroquad::prelude::*;

use razdor::dt::dtm::Scenario;
use razdor::editor::records;
use razdor::editor::palette::{self, Names};
use razdor::editor::{Command, Settings};
use razdor::i18n::{n_, tr};
use razdor::trf;

use super::form::*;
use crate::ui::widgets::*;

#[derive(Default)]
pub struct SettingsState {
    pub tab: usize,
    pub scroll: f32,
    /// The scenario picture file to import.
    picture_path: String,
}

/// What the window asks for.
pub enum SettingsAction {
    None,
    Apply(Command, String),
    /// Close the window and let the user click the start of hero preset `k`.
    PickStart(usize),
    /// A message for the status line.
    Status(String),
    Close,
}

const TABS: [&str; 7] = [n_("Scenario"), n_("Knight"), n_("Archmage"), n_("Ranger"), n_("Factions"), n_("Campaign"), n_("Characters")];


pub fn window(state: &mut SettingsState, s: &Scenario, names: &Names) -> SettingsAction {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.55));
    let w = 720.0f32.min(sw - 40.0);
    let h = (sh - 80.0).max(300.0);
    let r = Rect::new((sw - w) / 2.0, 40.0, w, h);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 0.98));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
    text(tr("Scenario settings"), r.x + 14.0, r.y + 28.0, 24.0, ACCENT);
    let before = state.tab;
    tabs(r.x + 10.0, r.y + 40.0, r.w - 20.0, &TABS.map(tr), &mut state.tab);
    if state.tab != before {
        state.scroll = 0.0;
    }
    let area = Rect::new(r.x + 20.0, r.y + 80.0, r.w - 40.0, r.h - 140.0);
    let orig = Settings {
        header: s.header.clone(),
        title: s.title.clone(),
        description: s.description.clone(),
        campaign_name: s.campaign_name.clone(),
        next_map: s.next_map.clone(),
        named_characters: s.named_characters.clone(),
        scenario_picture: s.scenario_picture.clone(),
    };
    let mut st = orig.clone();
    let mut f = Form::new("settings", area, state.scroll);
    let mut action = SettingsAction::None;
    let units = unit_options(names);
    match state.tab {
        0 => {
            f.text("title", tr("Title"), &mut st.title);
            st.title = records::cut(&st.title, records::TITLE_LEN);
            f.memo("description", tr("Description"), &mut st.description, 6);
            f.note(&trf!("Map size {w} x {h} cells.", w = s.width(), h = s.height()), DIM);
            f.heading(tr("The clock starts at"));
            f.date("start", &mut st.header.start_time);
            f.note(tr("A new start date moves every event's start by as much (but for relative-only events)."), DIM);
            f.heading(tr("End of the scenario"));
            let events = event_options_none(s);
            f.pick("victory", tr("Victory event"), &mut st.header.victory_event, &events);
            f.pick("defeat", tr("Defeat event"), &mut st.header.defeat_event, &events);
            f.note(tr("The scenario ends when one of these events fires. Events are edited with the Events button."), DIM);
            let (castles, villages) = records::income_sums(s);
            f.note(&trf!("Daily gold of castles and forts: {castles}; of villages: {villages}", castles, villages), DIM);
        }
        k @ 1..=3 => {
            let before = st.header.heroes[k - 1].clone();
            let hero = &mut st.header.heroes[k - 1];
            f.heading(&trf!("Start of the {class}", class = tr(palette::HERO_CLASSES[k - 1])));
            // The start is set by placing the hero on the map, as in the original.
            f.note(&trf!("At ({x}, {y})", x = hero.x, y = hero.y), INK);
            if f.button(tr("Pick the start on the map"), true) {
                action = SettingsAction::PickStart(k - 1);
            }
            let mut xp = records::preset_experience(hero);
            f.num("xp", tr("Experience"), &mut xp, 0, records::PRESET_MAX);
            records::set_preset_experience(hero, xp);
            let (mut gold, mut mana) = (records::preset_word(hero.gold), records::preset_word(hero.mana));
            f.num("gold", tr("Gold"), &mut gold, 0, records::PRESET_MAX);
            f.num("mana", tr("Mana"), &mut mana, 0, records::PRESET_MAX);
            records::set_preset_word(&mut hero.gold, gold);
            records::set_preset_word(&mut hero.mana, mana);
            f.pick("building", tr("Start building"), &mut hero.start_building, &building_options_of(s, &records::START_TYPES));
            f.heading(tr("Troops"));
            f.troops_raw("troops", &units, &mut hero.troops, 9);
            records::limit_preset_troops(&before, hero);
            f.note(tr("At most 11 units besides the hero."), DIM);
            f.heading(tr("Artefacts"));
            let arts = artefact_options(names);
            for i in 0..3 {
                f.pick(&format!("art{i}"), &trf!("Artefact {n}", n = i + 1), &mut hero.artifacts[i], &arts);
            }
            f.heading(tr("Spells and prayers"));
            let spells = spell_options(names);
            for i in 0..records::PRESET_SPELLS {
                f.pick(&format!("spell{i}"), &trf!("Spell {n}", n = i + 1), &mut hero.spells[i], &spells);
            }
        }
        4 => {
            f.note(tr("How each faction (row) feels about each faction, -3 (war) to 3 (friends)."), DIM);
            if let Some(k) = f.buttons(&[tr("Default"), tr("Allied"), tr("Neutral"), tr("War")]) {
                st.header.relations = records::ALLIANCE_PRESETS[k];
                f.changed = Some("relations".into());
            }
            for (i, row) in palette::FACTIONS.iter().enumerate() {
                f.heading(row);
                f.relations(&format!("r{i}"), &mut st.header.relations[i]);
            }
        }
        5 => {
            f.pick("kind", tr("Scenario kind"), &mut st.header.scenario_kind, &list_options(&palette::SCENARIO_KINDS, 0));
            f.text("campaign", tr("Campaign name"), &mut st.campaign_name);
            f.text("next", tr("Next map file"), &mut st.next_map);
            st.next_map = records::file_name_only(&st.next_map);
            f.heading(tr("Picture"));
            let index = st.header.scenario_picture_index;
            if let Some(b) = f.buttons(&[tr("Previous picture"), tr("Next picture")]) {
                st.header.scenario_picture_index = records::cycle_picture(index, b == 1);
            }
            f.note(&if index == 0 { tr("Built-in picture: none").to_string() } else { trf!("Built-in picture {n}", n = index) }, INK);
            match st.scenario_picture.as_ref() {
                Some(p) => f.note(&trf!("Own picture: {n} bytes", n = p.len()), INK),
                None => f.note(tr("No own picture."), DIM),
            }
            f.text("pic_path", tr("Picture file (path)"), &mut state.picture_path);
            match f.buttons(&[tr("Import picture"), tr("Remove own picture")]) {
                Some(0) => match std::fs::read(state.picture_path.trim()).ok().and_then(records::scenario_picture) {
                    Some(p) => {
                        st.scenario_picture = Some(p);
                        f.changed = Some("picture_file".into());
                    }
                    // The original keeps a file only if it decodes as a picture.
                    None => action = SettingsAction::Status(trf!("Cannot read a picture from \"{path}\".", path = state.picture_path.trim())),
                },
                Some(_) => {
                    st.scenario_picture = None;
                    f.changed = Some("picture_file".into());
                }
                None => {}
            }
            f.heading(tr("The hero keeps from the previous map"));
            for (i, what) in palette::CARRY_OVER.iter().enumerate() {
                f.flag(&format!("carry{i}"), what, &mut st.header.carry_over[i]);
            }
        }
        _ => {
            f.note(tr("Named characters are unit types with a name; armies, events and the hero's squad can use them. Removing one renumbers nothing, as in the original."), DIM);
            let mut remove = None;
            for (i, nc) in st.named_characters.iter_mut().enumerate() {
                f.heading(&trf!("Character {n}", n = i + 1));
                f.text(&format!("name{i}"), tr("Character's name"), &mut nc.name);
                nc.name = records::cut(&nc.name, records::TITLE_LEN);
                f.pick(&format!("unit{i}"), tr("Unit"), &mut nc.unit, &units);
                if f.button(tr("Remove"), true) {
                    remove = Some(i);
                }
            }
            if let Some(i) = remove {
                return SettingsAction::Apply(Command::RemoveNamedCharacter { index: i as u8 + 1 }, String::new());
            }
            if f.button(tr("Add a named character"), st.named_characters.len() < 32) {
                return SettingsAction::Apply(Command::AddNamedCharacter { unit: 1, name: tr("New character").into() }, String::new());
            }
        }
    }
    let content = f.content_height();
    if mouse_in(area.x, area.y, area.w, area.h) && !popup_open() {
        let wh = wheel();
        if wh != 0.0 {
            state.scroll -= wh * 40.0;
        }
    }
    state.scroll = state.scroll.clamp(0.0, (content - area.h + 20.0).max(0.0));
    if button(r.right() - 150.0, r.bottom() - 52.0, 130.0, 40.0, tr("Close"), true) || (!typing() && is_key_pressed(KeyCode::Escape)) {
        return SettingsAction::Close;
    }
    if st != orig {
        let key = f.changed.clone().unwrap_or_else(|| "settings".into());
        return SettingsAction::Apply(Command::SetSettings(Box::new(st)), key);
    }
    action
}
