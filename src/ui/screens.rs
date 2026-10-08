use macroquad::prelude::*;

use std::sync::Arc;

use razdor::dt::data::MagicSchool;
use razdor::dt::install::Problem;
use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::content::{Content, HeroClass, Stat, UnitId};
use razdor::rules::game::Game;
use razdor::rules::save::ScenarioRef;
use razdor::rules::script::ScriptEnd;
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::widgets::*;
use super::{ScenarioEntry, Screen};

/// Longest hero name the class screen takes.
const NAME_MAX: usize = 24;

thread_local! {
    /// The hero's name typed on the class screen (kept between frames and games).
    pub(super) static HERO_NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
}

/// Applies this frame's typing to `name`: printable characters are added (up to
/// [`NAME_MAX`]), Backspace removes the last one.
pub(super) fn edit_name(name: &mut String) {
    while let Some(c) = get_char_pressed() {
        if !c.is_control() && name.chars().count() < NAME_MAX {
            name.push(c);
        }
    }
    if !input_blocked() && is_key_pressed(KeyCode::Backspace) {
        name.pop();
    }
}

/// The name field of the class screen: what was typed, or the class names as a hint.
fn name_field(name: &str, x: f32, y: f32, w: f32) {
    text_fit(tr("Hero's name (type it; empty: the class's name):"), x, y - 8.0, w, 18.0, DIM);
    draw_rectangle(x, y, w, 36.0, PANEL);
    draw_rectangle_lines(x, y, w, 36.0, 2.0, ACCENT);
    let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    if name.is_empty() {
        text(&format!("{caret}{}", tr("(the class's name)")), x + 10.0, y + 25.0, 20.0, DIM);
    } else {
        text(&format!("{name}{caret}"), x + 10.0, y + 25.0, 20.0, INK);
    }
}

/// The attack line of a card: melee `A`, ranged `S` or magic `Pwr` with the school.
pub(super) fn attack_line(s: &Stats) -> String {
    let mut parts = Vec::new();
    if s.is_warrior() {
        parts.push(trf!("attack {n}", n = s[Stat::AttackBlow]));
    }
    if s.is_shooter() {
        parts.push(trf!("shot {n}", n = s[Stat::AttackShot]));
    }
    if s.is_mage() {
        let n = s[Stat::MagicPower];
        parts.push(match s.magic {
            Some(MagicSchool::Life) => trf!("Life magic {n}", n),
            Some(MagicSchool::Elemental) => trf!("Elemental magic {n}", n),
            Some(MagicSchool::Death) => trf!("Death magic {n}", n),
            None => trf!("magic {n}", n),
        });
    }
    if parts.is_empty() {
        parts.push(tr("no attack").into());
    }
    parts.join(", ")
}

pub(super) fn stat_lines(content: &Content, kind: UnitId) -> [String; 3] {
    let s = Stats::of_level(content, kind, 1);
    [
        trf!("Hits {hp}   Defence {blow}/{shot}", hp = s.max_hp(), blow = s[Stat::DefenceBlow], shot = s[Stat::DefenceShot]),
        format!("{}: {}", s.role(), attack_line(&s)),
        trf!("Initiative {ini}   Actions {act}", ini = s[Stat::Initiative], act = s[Stat::Manevres]),
    ]
}

/// First screen: the built-in demo or one of the install's maps (title and description are
/// read from the player's files at runtime).
pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    text_centered("RAZDOR", screen_width() / 2.0, 80.0, 64.0, ACCENT);
    text_centered(tr("Choose a scenario"), screen_width() / 2.0, 114.0, 24.0, DIM);
    super::language::switch_button(screen_width() - 136.0, 20.0, 116.0, 36.0);
    let (x, w) = (60.0, screen_width() - 120.0);
    let row_h = 52.0;
    let top = 140.0;
    let cols = if scenarios.len() > 10 { 2 } else { 1 };
    let col_w = (w - 20.0 * (cols - 1) as f32) / cols as f32;
    let mut hovered = None;
    let mut entries: Vec<(Option<usize>, String, String)> =
        vec![(None, tr("Built-in demo: the bandit kingdom").into(), tr("Our own small map and units. Clear both bandit camps.").into())];
    entries.extend(scenarios.iter().enumerate().map(|(i, e)| {
        let title = if e.scenario.title.trim().is_empty() { e.file.clone() } else { e.scenario.title.clone() };
        let size = format!("{}×{}", e.scenario.width(), e.scenario.height());
        (Some(i), format!("{title}  ({size})"), e.scenario.description.clone())
    }));
    let per_col = entries.len().div_ceil(cols);
    for (k, (idx, title, desc)) in entries.iter().enumerate() {
        let (c, r) = (k / per_col, k % per_col);
        let (ex, ey) = (x + c as f32 * (col_w + 20.0), top + r as f32 * (row_h + 6.0));
        let hover = mouse_in(ex, ey, col_w, row_h);
        draw_rectangle(ex, ey, col_w, row_h, PANEL);
        draw_rectangle_lines(ex, ey, col_w, row_h, 2.0, if hover { ACCENT } else { DIM });
        text_fit(title, ex + 12.0, ey + 22.0, col_w - 24.0, 20.0, if idx.is_none() { ACCENT } else { INK });
        let first = wrap(desc, col_w - 24.0, 16.0).into_iter().next().unwrap_or_default();
        text(&first, ex + 12.0, ey + 42.0, 16.0, DIM);
        if hover {
            hovered = Some((*idx, desc.clone()));
            if clicked() {
                cue(Cue::MenuPress);
                return Some(Screen::ClassSelect { scenario: *idx });
            }
        }
    }
    if button(screen_width() - 220.0, 40.0, 180.0, 44.0, tr("Load a game"), true) {
        return Some(Screen::Load(super::saves::LoadView::new(super::saves::Back::Title)));
    }
    if button(40.0, 40.0, 180.0, 44.0, tr("Map editor"), true) {
        return Some(Screen::Editor);
    }
    let y = screen_height() - 110.0;
    if let Some((Some(_), desc)) = hovered {
        for (i, line) in wrap(&desc, w, 18.0).iter().take(4).enumerate() {
            text(line, x, y + i as f32 * 22.0, 18.0, INK);
        }
    } else if !has_install {
        let hint = tr("Discord Times not found: put Razdor into the game folder, next to DiscordTimes.exe.");
        text_centered(hint, screen_width() / 2.0, y + 20.0, 20.0, DIM);
        let why = match razdor::dt::install::problem() {
            Some(Problem::NotInstall { dir, missing }) => {
                Some(trf!("This folder has no {missing}: {dir}", missing = missing.join(", "), dir = dir.display()))
            }
            Some(Problem::Load(e)) => Some(e),
            None => None,
        };
        if let Some(why) = why {
            text_fit(&why, x, y + 48.0, w, 16.0, DIM);
        }
    }
    None
}

/// A new game as `hero` named `name`: of the demo on `demo`, or of the install's map.
pub(super) fn start_game(demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, &Arc<Content>)>, hero: HeroClass, name: &str) -> Game {
    let mut g = match scenario {
        Some((e, c)) => {
            let mut g = Game::from_scenario(c.clone(), &e.scenario, hero);
            // Saves name the map file and a hash of its bytes.
            match ScenarioRef::of_map(&e.path, &e.file) {
                Ok(origin) => g.set_origin(origin),
                Err(err) => razdor::diag!("{}: {err}; this game cannot be saved", e.file),
            }
            g
        }
        None => Game::new(demo.clone(), hero),
    };
    g.set_hero_name(name);
    razdor::diag::play(&g.clock.label(), &super::play_game_line(&g, "new game"));
    g
}

/// Hero class: for the demo its own classes, for a scenario the map's three presets.
pub fn class_select(
    game: &mut Option<Game>,
    demo: &Arc<Content>,
    scenario: Option<(&ScenarioEntry, Arc<Content>)>,
    assets: &Assets,
) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    let content = scenario.as_ref().map_or(demo.clone(), |(_, c)| c.clone());
    let title = scenario.as_ref().map_or(tr("A time of discord").to_string(), |(e, _)| e.scenario.title.clone());
    text_centered(&title, screen_width() / 2.0, 100.0, 44.0, ACCENT);
    text_centered(tr("Choose who you are."), screen_width() / 2.0, 140.0, 26.0, DIM);
    if let Some((e, _)) = &scenario {
        if e.scenario.header.scenario_kind == 2 {
            let note = tr("A later campaign map: the original carries gold and army over from the previous one.");
            text_centered(note, screen_width() / 2.0, 168.0, 18.0, DIM);
        }
    }

    // Only the classes the map gives a start cell (0x4c1804); none: back to the menu.
    let offered = scenario.as_ref().map_or([true; 3], |(e, _)| e.scenario.header.offered_classes());
    if !offered.contains(&true) {
        return Some(Screen::MainMenu);
    }
    let (w, h, gap) = (300.0, 380.0, 30.0);
    let x0 = (screen_width() - (3.0 * w + 2.0 * gap)) / 2.0;
    let name = HERO_NAME.with(|n| {
        let mut n = n.borrow_mut();
        edit_name(&mut n);
        n.clone()
    });
    name_field(&name, (screen_width() - 420.0) / 2.0, 600.0, 420.0);
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let kind = hero.unit();
        let x = x0 + i as f32 * (w + gap);
        let y = 180.0;
        let hover = offered[i] && mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, if hover { ACCENT } else { DIM });
        assets.draw_unit(kind, Team::Player, x + w / 2.0, y + 80.0, 96.0);
        text_centered(&content.unit(kind).name, x + w / 2.0, y + 170.0, 30.0, INK);
        for (j, line) in stat_lines(&content, kind).iter().enumerate() {
            text_centered(line, x + w / 2.0, y + 205.0 + j as f32 * 24.0, 18.0, DIM);
        }
        match &scenario {
            Some((e, c)) => {
                let preset = &e.scenario.header.heroes[i];
                text_centered(&trf!("Gold: {gold}", gold = preset.gold), x + w / 2.0, y + 290.0, 24.0, ACCENT);
                let army: Vec<String> = preset
                    .troops
                    .iter()
                    .filter(|t| t.unit != 0 && t.count > 0)
                    .filter_map(|t| c.try_unit(UnitId(t.unit as u32)).map(|u| format!("{} {}", t.count, u.name)))
                    .collect();
                let army = if army.is_empty() { tr("alone").to_string() } else { army.join(", ") };
                for (j, line) in wrap(&army, w - 20.0, 17.0).iter().take(3).enumerate() {
                    text_centered(line, x + w / 2.0, y + 318.0 + j as f32 * 20.0, 17.0, INK);
                }
            }
            None => text_centered(&trf!("Gold: {gold}", gold = content.start_gold(hero)), x + w / 2.0, y + 300.0, 24.0, ACCENT),
        }
        if !offered[i] {
            draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.6));
        }
        if hover && clicked() {
            cue(Cue::MenuPress);
            *game = Some(start_game(&content, scenario.as_ref().map(|(e, c)| (*e, c)), hero, &name));
            return Some(Screen::WorldMap);
        }
    }
    if button(30.0, screen_height() - 70.0, 160.0, 44.0, tr("Back"), true) {
        return Some(Screen::ScenarioSelect);
    }
    None
}


fn end_screen(title: &str, subtitle: &str, color: Color, game: &mut Option<Game>) -> Option<Screen> {
    clear_background(Color::from_rgba(20, 18, 16, 255));
    text_centered(title, screen_width() / 2.0, 260.0, 64.0, color);
    text_centered(subtitle, screen_width() / 2.0, 310.0, 26.0, DIM);
    if button(screen_width() / 2.0 - 110.0, 380.0, 220.0, 50.0, tr("New game"), true) {
        *game = None;
        return Some(Screen::MainMenu);
    }
    None
}

/// Whole days since the game started.
fn days_played(game: &Option<Game>) -> u64 {
    game.as_ref().map_or(0, |g| g.clock.day_index().saturating_sub(g.world.start.day_index()))
}

/// The scenario's victory or defeat event, if it ended the game: its title.
fn end_event(game: &Option<Game>) -> Option<String> {
    let g = game.as_ref()?;
    let id = match g.script_end()? {
        ScriptEnd::Victory(id) | ScriptEnd::Defeat(id) => id,
    };
    Some(super::story::event_title(g, id))
}

/// What the defeat screen asks the app to do besides a new game.
pub enum EndChoice {
    /// Load this save (the newest of the same map).
    Load(std::path::PathBuf),
    /// Start the same map again, with the same hero.
    Restart,
}

thread_local! {
    /// The newest save of the lost game's map, looked up at most once a second: (when,
    /// which map, the save).
    static LATEST: std::cell::RefCell<Option<(f64, ScenarioRef, Option<std::path::PathBuf>)>> = const { std::cell::RefCell::new(None) };
}

/// The newest save, manual or automatic, of the map `game` was playing.
fn latest_save(game: &Game) -> Option<std::path::PathBuf> {
    let origin = game.origin.clone()?;
    let now = get_time();
    LATEST.with(|l| {
        if let Some((at, of, path)) = l.borrow().as_ref() {
            if *of == origin && now - at < 1.0 {
                return path.clone();
            }
        }
        let dir = razdor::rules::save::default_dir()?;
        let newest = [razdor::rules::save::SaveKind::Manual, razdor::rules::save::SaveKind::Auto]
            .into_iter()
            .flat_map(|k| razdor::rules::save::list(&dir, k))
            .filter(|e| e.meta.scenario == origin)
            .max_by_key(|e| e.meta.saved_at)
            .map(|e| e.path);
        *l.borrow_mut() = Some((now, origin, newest.clone()));
        newest
    })
}

/// The defeat screen: a new game, or, for the map just lost, its newest save or a restart.
pub fn game_over(game: &mut Option<Game>) -> (Option<Screen>, Option<EndChoice>) {
    let days = days_played(game);
    let latest = game.as_ref().and_then(latest_save);
    let can_restart = game.as_ref().is_some_and(|g| g.origin.is_some());
    let shown = match end_event(game) {
        Some(title) => end_screen(tr("Defeat"), &trf!("{title}. You lasted {days} days.", title, days), RED, game),
        None => end_screen(tr("Your hero has fallen"), &trf!("The discord goes on. You lasted {days} days.", days), RED, game),
    };
    if shown.is_some() {
        return (shown, None);
    }
    let x = screen_width() / 2.0 - 110.0;
    if button(x, 445.0, 220.0, 50.0, tr("Load the latest save"), latest.is_some()) {
        if let Some(path) = latest {
            return (None, Some(EndChoice::Load(path)));
        }
    }
    if button(x, 510.0, 220.0, 50.0, tr("Restart this map"), can_restart) {
        return (None, Some(EndChoice::Restart));
    }
    (None, None)
}

/// The victory screen. After a campaign map whose next map is in the install, "Next map"
/// starts it with what carries over (header 0x110) and the flags, as the original does.
pub fn victory(game: &mut Option<Game>, scenarios: &[ScenarioEntry], content: Option<Arc<Content>>) -> Option<Screen> {
    let days = days_played(game);
    let next = game.as_ref().and_then(Game::next_map);
    let entry = next.as_ref().and_then(|n| {
        let stem = n.name.trim();
        let stem = stem.strip_suffix(".DTm").or_else(|| stem.strip_suffix(".dtm")).unwrap_or(stem).to_lowercase();
        scenarios.iter().find(|e| e.file.to_lowercase() == stem)
    });
    let shown = match end_event(game) {
        Some(title) => end_screen(tr("Victory!"), &trf!("{title}, after {days} days.", title, days), ACCENT, game),
        None => end_screen(tr("The bandits are broken"), &trf!("Peace returns to the land after {days} days.", days), ACCENT, game),
    };
    if shown.is_some() {
        return shown;
    }
    if let (Some(prev), Some(e), Some(c)) = (next, entry, content) {
        if button(screen_width() / 2.0 - 160.0, 450.0, 320.0, 50.0, &trf!("Next map: {title}", title = e.scenario.title), true) {
            cue(Cue::MenuPress);
            let mut g = Game::from_campaign(c, &e.scenario, &prev);
            match ScenarioRef::of_map(&e.path, &e.file) {
                Ok(origin) => g.set_origin(origin),
                Err(err) => razdor::diag!("{}: {err}; this game cannot be saved", e.file),
            }
            razdor::diag::play(&g.clock.label(), &super::play_game_line(&g, "next campaign map"));
            *game = Some(g);
            return Some(Screen::WorldMap);
        }
    }
    None
}
