use razdor::local;
use macroquad::prelude::*;

use std::sync::Arc;

use razdor::dt::data::MagicSchool;
#[cfg(not(any(target_os = "android", target_os = "ios")))]
use razdor::dt::install::Problem;
use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::content::{Content, HeroClass, Stat, UnitId};
use razdor::rules::game::Game;
use razdor::rules::save::ScenarioRef;
use razdor::rules::script::{NextMap, ScriptEnd};
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::text_edit::TextEdit;
use super::widgets::*;
use super::{ScenarioEntry, Screen};

/// Longest hero name the class screen takes.
const NAME_MAX: usize = 24;

local! {
    /// The hero's name typed on the class screen (kept between frames and games).
    pub(super) static HERO_NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    /// The name field's caret and selection.
    pub(super) static HERO_EDIT: std::cell::Cell<TextEdit> = const { std::cell::Cell::new(TextEdit::at_end()) };
    /// The fallback scenario screen's last explicitly selected entry (demo is zero).
    static SCENARIO_PICKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The hero's name field: this frame's keys edit the name (printable characters up to
/// [`NAME_MAX`]; held Backspace repeats, as the original's edit box 0x47a624 takes each
/// repeated WM_KEYDOWN, 0x475748), then it is drawn in `r` on the baseline `y`, the mouse
/// placing the caret and selecting. Razdor's own, beyond the original's box (caret at the
/// end only): the caret, the selection, the clipboard ([`edit_keys`]). Returns the name.
pub(super) fn hero_name_field(r: Rect, y: f32, look: LineLook) -> String {
    HERO_NAME.with(|n| {
        let mut n = n.borrow_mut();
        let mut ed = HERO_EDIT.with(|e| e.get());
        if !input_blocked() {
            edit_keys(&mut n, &mut ed, &typed_chars(), EditRules::line(NAME_MAX));
        }
        edit_line(field_id("hero:name"), &n, &mut ed, true, true, r, y, look);
        HERO_EDIT.with(|e| e.set(ed));
        n.clone()
    })
}

/// The name field of the class screen: what was typed, or the class names as a hint.
fn name_field(x: f32, y: f32, w: f32) -> String {
    text_fit(tr("Hero's name (type it; empty: the class's name):"), x, y - 8.0, w, 18.0, DIM);
    draw_rectangle(x, y, w, 36.0, PANEL);
    draw_rectangle_lines(x, y, w, 36.0, 2.0, ACCENT);
    let name = hero_name_field(Rect::new(x + 10.0, y, w - 20.0, 36.0), y + 25.0, LineLook { size: 20.0, color: INK, centred: false, shadow: false });
    if name.is_empty() {
        text(tr("(the class's name)"), x + 14.0, y + 25.0, 20.0, DIM);
    }
    name
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
    if button(60.0, 20.0, 48.0, 32.0, "...", true) {
        super::new_game::open_experimental_menu();
        return None;
    }
    super::language::picker_box(Rect::new(screen_width() - 196.0, 20.0, 176.0, 36.0));
    let (x, w) = (60.0, screen_width() - 120.0);
    let row_h = 52.0;
    let top = 140.0;
    let cols = if scenarios.len() > 10 { 2 } else { 1 };
    let col_w = (w - 20.0 * (cols - 1) as f32) / cols as f32;
    let mut hovered = None;
    let mut picked = SCENARIO_PICKED.with(|p| p.get()).min(scenarios.len());
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
        let selected = picked == k;
        draw_rectangle_lines(ex, ey, col_w, row_h, 2.0, if selected { ACCENT } else { DIM });
        if selected {
            draw_rectangle(ex + 2.0, ey + 2.0, col_w - 4.0, row_h - 4.0, Color::new(0.8, 0.35, 0.08, 0.25));
        }
        text_fit(title, ex + 12.0, ey + 22.0, col_w - 24.0, 20.0, if idx.is_none() { ACCENT } else { INK });
        let first = wrap(desc, col_w - 24.0, 16.0).into_iter().next().unwrap_or_default();
        text(&first, ex + 12.0, ey + 42.0, 16.0, DIM);
        if hover {
            hovered = Some((*idx, desc.clone()));
            if clicked() {
                picked = k;
                SCENARIO_PICKED.with(|p| p.set(picked));
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
        #[cfg(target_os = "android")]
        {
            android_not_found(x, w);
        }
        #[cfg(target_os = "ios")]
        {
            ios_not_found(x, w);
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
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
    }
    None
}

/// Android: the install is not there. Tells the player what to do, and offers the two buttons:
/// give Razdor the storage access (the settings page; a dialog on Android 8 to 10), and look
/// again. The permission is in `ui::android`, the look in `ui::mobile`; the look runs on the
/// title screen. The access page is opened at the first frame of the run without an install
/// (`App::mobile_install_poll`). The block is scaled to the screen ([`PhoneBlock`]).
#[cfg(target_os = "android")]
fn android_not_found(x: f32, w: f32) {
    use super::android::request_storage;
    use super::mobile::ask_recheck;
    let b = PhoneBlock::new(w, 2);
    text_fit(tr("Discord Times not found."), x, b.y, w, 20.0 * b.k, INK);
    text_fit(tr("1. Give Razdor access to all files: the «Grant access» button, or Settings > Apps > Special app access > All files access > Razdor."), x, b.y + 26.0 * b.k, w, 16.0 * b.k, DIM);
    text_fit(tr("2. Put the game folder in internal storage, in a folder named DiscordTimes (or in Download)."), x, b.y + 46.0 * b.k, w, 16.0 * b.k, DIM);
    if button(x, b.buttons_y, b.btn_w, b.btn_h, tr("Grant access"), true) {
        request_storage();
    }
    if button(x + b.btn_w + 16.0 * b.k, b.buttons_y, b.btn_w, b.btn_h, tr("Check again"), true) {
        ask_recheck();
    }
}

/// iOS: the install is not there. Tells the player how to copy the game folder into the app's
/// Files folder (no permission to grant), and offers «Check again», the look that runs on the
/// title screen (`ui::mobile`). Scaled and placed as [`android_not_found`].
#[cfg(target_os = "ios")]
fn ios_not_found(x: f32, w: f32) {
    use super::mobile::ask_recheck;
    let b = PhoneBlock::new(w, 1);
    text_fit(tr("Discord Times not found."), x, b.y, w, 20.0 * b.k, INK);
    text_fit(tr("1. Copy the game folder to the Files app: On My iPhone (or iPad) > Razdor, in a folder named DiscordTimes."), x, b.y + 26.0 * b.k, w, 16.0 * b.k, DIM);
    text_fit(tr("2. From a computer: Finder (or iTunes) > the device > Files > Razdor."), x, b.y + 46.0 * b.k, w, 16.0 * b.k, DIM);
    if button(x, b.buttons_y, b.btn_w, b.btn_h, tr("Check again"), true) {
        ask_recheck();
    }
}

/// Android and iOS, on the main menu while no install is found: the not-found block in the
/// column left of the menu's buttons (the left half of the screen, in landscape and portrait),
/// over a dark panel, with the same texts and buttons as the scenario list's block. Its buttons
/// stack. The scale is the interface's (`chrome::k`, which fits the width and the height), so
/// the column is not scaled by the height alone.
#[cfg(any(target_os = "android", target_os = "ios"))]
pub fn menu_not_found(no_install: bool) {
    if !no_install {
        return;
    }
    let sh = screen_height();
    let k = super::chrome::k();
    let x = 16.0 * k;
    let w = (super::main_menu::left_edge() - 2.0 * x).max(120.0 * k);
    let btn_h = (sh * 0.12).max(34.0 * k);
    let btn_w = (240.0 * k).min(w);
    let gap = 12.0 * k;
    let bottom = sh - 60.0 * k;
    #[cfg(target_os = "android")]
    let (steps, buttons) = (
        [
            tr("1. Give Razdor access to all files: the «Grant access» button, or Settings > Apps > Special app access > All files access > Razdor."),
            tr("2. Put the game folder in internal storage, in a folder named DiscordTimes (or in Download)."),
        ],
        2,
    );
    #[cfg(target_os = "ios")]
    let (steps, buttons) = (
        [
            tr("1. Copy the game folder to the Files app: On My iPhone (or iPad) > Razdor, in a folder named DiscordTimes."),
            tr("2. From a computer: Finder (or iTunes) > the device > Files > Razdor."),
        ],
        1,
    );
    let (title_size, step_size, lead) = (20.0 * k, 16.0 * k, 22.0 * k);
    let step_lines: Vec<String> = steps.iter().flat_map(|s| wrap(s, w, step_size)).collect();
    let text_h = title_size + 10.0 * k + step_lines.len() as f32 * lead;
    let buttons_top = bottom - buttons as f32 * btn_h - (buttons - 1) as f32 * gap;
    let text_top = buttons_top - 18.0 * k - text_h;
    draw_rectangle(x - 10.0 * k, text_top - 10.0 * k, w + 20.0 * k, bottom - text_top + 20.0 * k, Color::new(0.0, 0.0, 0.0, 0.6));
    let title_base = text_top + title_size;
    text(tr("Discord Times not found."), x, title_base, title_size, INK);
    for (i, line) in step_lines.iter().enumerate() {
        text(line, x, title_base + 10.0 * k + (i + 1) as f32 * lead, step_size, DIM);
    }
    #[cfg(target_os = "android")]
    {
        if button(x, buttons_top, btn_w, btn_h, tr("Grant access"), true) {
            super::android::request_storage();
        }
        if button(x, buttons_top + btn_h + gap, btn_w, btn_h, tr("Check again"), true) {
            super::mobile::ask_recheck();
        }
    }
    #[cfg(target_os = "ios")]
    {
        if button(x, buttons_top, btn_w, btn_h, tr("Check again"), true) {
            super::mobile::ask_recheck();
        }
    }
}

/// Other platforms: the install is searched for in the game's folder; no menu block.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn menu_not_found(_no_install: bool) {}

/// The layout of a phone's not-found block, scaled to the screen: the scale `k` (at least 1),
/// the buttons' height (a twelfth of the screen at least) and width (`buttons` of them across
/// the block, 16 scaled pixels apart), the buttons' top `buttons_y`, and the block's first text
/// line `y` (66 scaled pixels above the buttons).
#[cfg(any(target_os = "android", target_os = "ios"))]
struct PhoneBlock {
    k: f32,
    btn_h: f32,
    btn_w: f32,
    buttons_y: f32,
    y: f32,
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl PhoneBlock {
    fn new(w: f32, buttons: u32) -> PhoneBlock {
        let sh = screen_height();
        let k = (sh / 600.0).max(1.0);
        let btn_h = (sh * 0.12).max(34.0 * k);
        let btn_w = (200.0 * k).min((w - 16.0 * k * (buttons - 1) as f32) / buttons as f32);
        let buttons_y = sh - 20.0 * k - btn_h;
        PhoneBlock { k, btn_h, btn_w, buttons_y, y: buttons_y - 66.0 * k }
    }
}

/// A new game as `hero` named `name`: of the demo on `demo`, or of the install's map.
pub(super) fn start_game(demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, &Arc<Content>)>, hero: HeroClass, name: &str) -> Game {
    start_game_with_rules(demo, scenario, hero, name, super::new_game::experimental_rules())
}

pub(super) fn start_game_with_rules(demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, &Arc<Content>)>, hero: HeroClass, name: &str, rules: razdor::rules::content::ExperimentalRules) -> Game {
    let mut g = match scenario {
        Some((e, c)) => {
            let content = Arc::new(c.with_experimental(rules));
            let mut g = Game::from_scenario(content, &e.scenario, hero);
            // Saves name the map file, a hash of its bytes, and carry the bytes.
            match ScenarioRef::of_map(&e.path, &e.file) {
                Ok((origin, bytes)) => g.set_map(origin, bytes),
                Err(err) => razdor::diag!("{}: {err}; this game cannot be saved", e.file),
            }
            g
        }
        None => Game::new(Arc::new(demo.with_experimental(rules)), hero),
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
    let content = Arc::new(content.with_experimental(super::new_game::experimental_rules()));
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
    let name = name_field((screen_width() - 420.0) / 2.0, 600.0, 420.0);
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

/// After a campaign victory whose next map is in the install: what carries over (header
/// 0x110), with the flags, and that map's entry.
fn next_campaign_map<'a>(game: &Option<Game>, scenarios: &'a [ScenarioEntry]) -> Option<(NextMap, &'a ScenarioEntry)> {
    let next = game.as_ref().and_then(Game::next_map)?;
    let stem = next.name.trim();
    let stem = stem.strip_suffix(".DTm").or_else(|| stem.strip_suffix(".dtm")).unwrap_or(stem).to_lowercase();
    let entry = scenarios.iter().find(|e| e.file.to_lowercase() == stem)?;
    Some((next, entry))
}

/// The next campaign map's game, started from `prev`'s hand-over.
fn campaign_game(content: Arc<Content>, prev: &NextMap, e: &ScenarioEntry) -> Game {
    let mut g = Game::from_campaign(content, &e.scenario, prev);
    match ScenarioRef::of_map(&e.path, &e.file) {
        Ok((origin, bytes)) => g.set_map(origin, bytes),
        Err(err) => razdor::diag!("{}: {err}; this game cannot be saved", e.file),
    }
    razdor::diag::play(&g.clock.label(), &super::play_game_line(&g, "next campaign map"));
    g
}

/// The original's campaign hand-over (events.md §11, 0x4b5b64): once the victory event's
/// window is read, the next map starts at once, with no screen between. `None` when the map
/// names no next map or the install lacks it (then the victory screen).
pub fn hand_over(game: &Option<Game>, scenarios: &[ScenarioEntry], content: Option<Arc<Content>>) -> Option<Game> {
    let (prev, e) = next_campaign_map(game, scenarios)?;
    Some(campaign_game(content?, &prev, e))
}

/// The victory screen. After a campaign map whose next map is in the install, "Next map"
/// starts it with what carries over (header 0x110) and the flags, as the original does.
pub fn victory(game: &mut Option<Game>, scenarios: &[ScenarioEntry], content: Option<Arc<Content>>) -> Option<Screen> {
    let days = days_played(game);
    let next = next_campaign_map(game, scenarios);
    let shown = match end_event(game) {
        Some(title) => end_screen(tr("Victory!"), &trf!("{title}, after {days} days.", title, days), ACCENT, game),
        None => end_screen(tr("The bandits are broken"), &trf!("Peace returns to the land after {days} days.", days), ACCENT, game),
    };
    if shown.is_some() {
        return shown;
    }
    if let (Some((prev, e)), Some(c)) = (next, content) {
        if button(screen_width() / 2.0 - 160.0, 450.0, 320.0, 50.0, &trf!("Next map: {title}", title = e.scenario.title), true) {
            cue(Cue::MenuPress);
            *game = Some(campaign_game(c, &prev, e));
            return Some(Screen::WorldMap);
        }
    }
    None
}

#[cfg(test)]
mod experimental_tests {
    use super::*;

    #[test]
    fn experimental_menu_choices_only_affect_the_new_party() {
        let demo = Arc::new(Content::builtin());
        let rules = razdor::rules::content::ExperimentalRules { simultaneous_bonuses: true, multiple_rings: true, multiple_amulets: false, allow_duplicate_items: false, two_potions: true, six_cells: false };
        super::super::new_game::snapshot_experimental(rules);
        let game = start_game(&demo, None, HeroClass::Knight, "Test");
        assert_eq!(game.experimental, rules);
        assert_eq!(game.content.experimental, rules);
        super::super::new_game::open_list();
        assert_eq!(super::super::new_game::experimental_rules(), razdor::rules::content::ExperimentalRules::for_new_game());
        assert_eq!(game.experimental, rules, "new menu choices cannot change a running party");
        let restarted = start_game_with_rules(&demo, None, game.start_class(), "Test", game.experimental);
        assert_eq!(restarted.content.experimental, rules);
        assert_eq!(demo.experimental, Default::default());
    }
}
