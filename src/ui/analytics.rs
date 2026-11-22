//! The play events' hooks in the UI (`razdor::telemetry::events`): the session, the map runs, the
//! progress, the battles, the saves, the settings and the interface counters. The rules never call
//! this (they stay free of telemetry). Every hook does nothing when the analytics are off
//! (`telemetry::hub()` is None: under the tests, in the snapshots, with `RAZDOR_TELEMETRY=off`).

use std::cell::RefCell;
use std::path::Path;

use razdor::rules::battle::{Battle, Team};
use razdor::rules::content::ExperimentalRules;
use razdor::rules::game::{BattleResult, Game};
use razdor::rules::recording::header::{install_hash, ReplayHeader};
use razdor::rules::recording::hook;
use razdor::rules::save::{self, ScenarioRef, SaveKind};
use razdor::telemetry::battles::{self, Facts};
use razdor::telemetry::events::{self, MapInfo, Tracker};
use razdor::telemetry::state::Consent;
use razdor::telemetry::{self, Hub};
use razdor_wire::analytics::{BattleOutcome, BattleTag, Outcome, SaveSource, Setting, Side};

use super::quality_preset::{self, Preset};
use super::{audio, main_menu, App, Screen};

/// A map run that started in the rules' game start, waiting for the app (its AI setting is the app's).
struct Start {
    map: String,
    hash: String,
    difficulty: String,
    experiments: Vec<String>,
}

#[derive(Default)]
struct Glue {
    /// The start of a campaign game, taken by the next frame.
    pending: Option<Start>,
    /// The second of the last poll (`telemetry::now_secs`).
    last_second: u64,
    /// The screen and the dialog count of the last frame: their changes are the windows opened.
    screen: String,
    dialogs: usize,
    /// The second the battle screen was entered (the battle record's start), and the battle's flags:
    /// the player let the autobattle play («Q»), or loaded a save while the battle was open.
    battle_since: Option<u64>,
    autobattle: bool,
    reloaded: bool,
}

razdor::local! {
    static TRACKER: RefCell<Option<Tracker>> = const { RefCell::new(None) };
    static GLUE: RefCell<Glue> = RefCell::new(Glue::default());
}

/// Runs `f` with the hub and the tracker, when the analytics are on.
fn with_tracker(f: impl FnOnce(&Hub, &mut Tracker)) {
    let Some(hub) = telemetry::hub() else { return };
    TRACKER.with(|t| {
        let mut t = t.borrow_mut();
        f(hub, t.get_or_insert_with(|| Tracker::new(hub.dir())));
    });
}

/// The session starts: called once the app is built (`main.rs`). The window's size, the graphics
/// preset and the settings that differ from the defaults go with it.
pub fn session_start(app: &App) {
    let (w, h) = (macroquad::window::screen_width() as u32, macroquad::window::screen_height() as u32);
    let graphics = graphics_key(app);
    let changed = changed_settings(app);
    with_tracker(|hub, t| {
        let now = telemetry::now_secs();
        t.session_start(hub, now, w, h, graphics);
        t.settings_changed(hub, now, changed);
    });
}

/// The session ends: called as the app exits (`main.rs`, `exit_now`).
pub fn session_end() {
    with_tracker(|hub, t| t.session_end(hub, telemetry::now_secs()));
    super::perf_analytics::session_end();
}

/// Called every frame by the app (cheap: a screen and a dialog count, and one poll a second).
pub fn frame(app: &App) {
    let Some(hub) = telemetry::hub() else { return };
    let now = telemetry::now_secs();
    let screen = app.screen_name();
    let dialogs = app.dialogs.len();
    // The screen the last frame showed, when it is not this one (the name is kept only on a change).
    let (start, previous, previous_dialogs, second_due) = GLUE.with(|g| {
        let mut g = g.borrow_mut();
        let second_due = g.last_second != now;
        g.last_second = now;
        let previous = (g.screen != screen).then(|| std::mem::replace(&mut g.screen, screen.to_string()));
        let previous_dialogs = std::mem::replace(&mut g.dialogs, dialogs);
        (g.pending.take(), previous, previous_dialogs, second_due)
    });
    let changed_screen = previous.is_some();
    if changed_screen && screen == "battle" {
        GLUE.with(|g| {
            let mut g = g.borrow_mut();
            g.battle_since = Some(now);
            g.autobattle = false;
            g.reloaded = false;
        });
    }
    let in_map = app.game.is_some() && !is_menu(&app.screen);
    let closed_settings = matches!(previous.as_deref(), Some("options" | "settings"));
    let settings = closed_settings.then(|| changed_settings(app));
    TRACKER.with(|tr| {
        let mut tr = tr.borrow_mut();
        let t = tr.get_or_insert_with(|| Tracker::new(hub.dir()));
        if let Some(start) = start {
            let info = MapInfo { map: &start.map, map_hash: &start.hash, difficulty: &start.difficulty, ai_strength: ai_key(app), experiments: &start.experiments };
            t.map_start(hub, now, info);
        }
        if changed_screen && !is_base_screen(screen) {
            t.count_window(hub, screen);
        }
        for _ in previous_dialogs..dialogs {
            t.count_window(hub, "dialog");
        }
        if let Some(changed) = settings {
            t.settings_changed(hub, now, changed);
        }
        t.set_screen(screen);
        if !second_due {
            return;
        }
        t.tick(hub, now, in_map);
        let Some(game) = app.game.as_ref().filter(|_| in_map) else { return };
        let hero_level = game.hero().level.max(0) as u32;
        let army = game.squad.len() as u32;
        t.progress(hub, now, game.clock.day_index() as u32, hero_level, army);
        if let Some(outcome) = map_outcome(game) {
            t.map_end(hub, now, outcome);
        }
    });
}

/// A campaign game starts (`screens::start_game_with_rules`): the run starts at the next frame, when
/// the app's settings are at hand. Nothing for the demo (it has no map file to name or hash).
pub fn campaign_started(game: &Game) {
    super::perf_analytics::map_requested();
    arm_battles(game);
    if telemetry::hub().is_none() {
        return;
    }
    let Some(ScenarioRef::Map { file, hash }) = game.origin.clone() else { return };
    let start = Start {
        map: file,
        hash: format!("{hash:016x}"),
        difficulty: game.content.options.difficulty_factor.to_string(),
        experiments: experiment_names(&game.experimental),
    };
    GLUE.with(|g| g.borrow_mut().pending = Some(start));
}

/// A save is loaded (`App::load`). Its source, its format (when older) and its map: an open run on
/// another map ends (as abandoned) and the save's map starts a new run; on the same map the run goes on.
pub fn save_loaded(app: &App, path: &Path, game: &Game) {
    super::perf_analytics::map_requested();
    if hook::battle_tap_open() {
        GLUE.with(|g| g.borrow_mut().reloaded = true);
    }
    arm_battles(game);
    let Ok(meta) = save::read_meta(path) else { return };
    let source = if meta.name == save::QUICK_SAVE {
        SaveSource::Quick
    } else if meta.name == save::SESSION {
        SaveSource::Session
    } else if meta.kind == SaveKind::Auto {
        SaveSource::Auto
    } else {
        SaveSource::Manual
    };
    let ai = ai_key(app);
    let experiments = experiment_names(&game.experimental);
    let difficulty = game.content.options.difficulty_factor.to_string();
    with_tracker(|hub, t| {
        let now = telemetry::now_secs();
        t.older_save_loaded(hub, now, source, meta.version, save::FORMAT_VERSION);
        if let ScenarioRef::Map { file, hash } = &meta.scenario {
            if t.run().is_none_or(|r| r.map != *file) {
                let hash = format!("{hash:016x}");
                t.map_start(hub, now, MapInfo { map: file, map_hash: &hash, difficulty: &difficulty, ai_strength: ai, experiments: &experiments });
            }
        }
    });
}

/// The battles of a campaign game are captured for the battle records (`hook::arm_battle_tap`) when the
/// consent is yes; any other game, and no consent, turns the capture off.
fn arm_battles(game: &Game) {
    let campaign = matches!(game.origin, Some(ScenarioRef::Map { .. }));
    let on = telemetry::hub().is_some_and(|h| h.consent_now() == Consent::Yes) && campaign;
    hook::arm_battle_tap(on);
}

/// The player let the autobattle play in the battle (`battle_view`'s quick battle: «Q»).
pub fn battle_autobattle() {
    GLUE.with(|g| g.borrow_mut().autobattle = true);
}

/// The battle's tags in its record: the autobattle, and a load before the battle ended.
fn battle_tags(autobattle: bool, reloaded: bool) -> Vec<BattleTag> {
    let mut tags = Vec::new();
    if autobattle {
        tags.push(BattleTag::Autobattle);
    }
    if reloaded {
        tags.push(BattleTag::ReloadBeforeEnd);
    }
    tags
}

/// A battle ended (`battle_view`, after `resolve_battle`): the run's last battle. The turns are the
/// battle's round; the losses count the fighters that are down at the end. The battle's record is
/// packed and queued (`battle_record`).
pub fn battle_ended(game: &Game, battle: &Battle, result: &BattleResult) {
    let winner = match result {
        BattleResult::Victory { .. } => Side::Player,
        BattleResult::Withdrew { .. } | BattleResult::Defeat => Side::Foe,
    };
    let down = |team: Team| battle.fighters.iter().filter(|f| f.team == team && !f.alive()).count() as u32;
    let outcome = BattleOutcome { winner, turns: battle.round, player_losses: down(Team::Player), foe_losses: down(Team::Enemy) };
    with_tracker(|hub, t| t.battle_ended(hub, telemetry::now_secs(), outcome));
    battle_record(game, outcome);
}

/// Packs the battle that just ended from the rules' capture (`hook::take_battle_tap`) and queues it for
/// the next hour (`battles::submit`). Nothing without the hub, the consent or a captured battle.
fn battle_record(game: &Game, outcome: BattleOutcome) {
    let (since, autobattle, reloaded) = GLUE.with(|g| {
        let mut g = g.borrow_mut();
        let flags = (g.battle_since.take(), g.autobattle, g.reloaded);
        g.autobattle = false;
        g.reloaded = false;
        flags
    });
    let Some(hub) = telemetry::hub() else { return };
    let Some(captured) = hook::take_battle_tap() else { return };
    let Some(install) = save::install_dir().and_then(|dir| install_hash(&dir).ok()) else { return };
    let header = ReplayHeader::of(game, install);
    let install_id = hub.install_id();
    let game_id = telemetry::game_id();
    let now = telemetry::now_secs();
    let facts = Facts {
        install_id: &install_id,
        game: &game_id,
        mod_hash: None,
        experiments: experiment_names(&game.experimental),
        tags: battle_tags(autobattle, reloaded),
        outcome,
        started_at: since.unwrap_or(now),
        ended_at: now,
    };
    if let Ok(packed) = battles::pack(&header, &captured.start, &captured.entries, &facts) {
        let _ = battles::submit(hub, &packed, now);
    }
}

/// The «?» hint opened (`main_menu`'s help button): counted for the session.
pub fn hint_opened() {
    with_tracker(|hub, t| t.count_hint(hub));
}

/// The game's end, as the run sees it: a scenario's victory or defeat, the victory screen (all camps
/// cleared) or the army fallen.
fn map_outcome(game: &Game) -> Option<Outcome> {
    match game.script_end() {
        Some(razdor::rules::script::ScriptEnd::Victory(_)) => Some(Outcome::Win),
        Some(razdor::rules::script::ScriptEnd::Defeat(_)) => Some(Outcome::Loss),
        None if game.won() => Some(Outcome::Win),
        None if game.army_fallen() => Some(Outcome::Loss),
        None => None,
    }
}

/// The screens that are not windows: the menus, the map and the battle (counted apart).
fn is_base_screen(name: &str) -> bool {
    matches!(name, "main menu" | "authors" | "options" | "scenario select" | "tutorial offer" | "class select" | "world map" | "battle" | "victory" | "defeat")
}

/// The screens with no game in them (the map run is not in progress there).
fn is_menu(screen: &Screen) -> bool {
    matches!(screen, Screen::MainMenu | Screen::Options | Screen::ScenarioSelect | Screen::Authors(_) | Screen::TutorialOffer | Screen::ClassSelect { .. })
}

/// The settings that differ from the defaults, as names and keys.
fn changed_settings(app: &App) -> Vec<Setting> {
    let current = serde_json::to_value(&app.audio.settings).unwrap_or(serde_json::Value::Null);
    let default = serde_json::to_value(audio::Settings::default()).unwrap_or(serde_json::Value::Null);
    events::differing_settings(&current, &default)
}

/// The graphics preset's key («Качество графики»); `custom` when the settings match no preset.
fn graphics_key(app: &App) -> &'static str {
    match quality_preset::current(&app.audio.settings) {
        Some(Preset::Original) => "original",
        Some(Preset::Low) => "low",
        Some(Preset::Medium) => "medium",
        Some(Preset::High) => "high",
        None => "custom",
    }
}

/// The enemy's battle AI's key («Максимум» and the rest).
fn ai_key(app: &App) -> &'static str {
    match main_menu::ai_strength(&app.audio.settings) {
        main_menu::AiStrength::Original => "original",
        main_menu::AiStrength::Improved => "improved",
        main_menu::AiStrength::Max => "max",
    }
}

/// The names of the experiments that are on (the switches set to true).
fn experiment_names(rules: &ExperimentalRules) -> Vec<String> {
    let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(rules) else { return Vec::new() };
    fields.into_iter().filter(|(_, on)| *on == serde_json::Value::Bool(true)).map(|(name, _)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::telemetry::events::fnv1a64_hex;

    #[test]
    fn with_no_hub_the_hooks_do_nothing() {
        // Under the tests the analytics are off: the hooks return at once and keep no state.
        assert!(telemetry::hub().is_none());
        hint_opened();
        session_end();
        GLUE.with(|g| assert!(g.borrow().pending.is_none()));
    }

    #[test]
    fn the_map_hash_is_the_save_hash_as_hex() {
        let bytes = b"a map's bytes";
        assert_eq!(fnv1a64_hex(bytes), format!("{:016x}", save::fnv1a(bytes)));
    }

    #[test]
    fn an_autobattle_or_a_reload_tags_the_battle_record() {
        assert_eq!(battle_tags(false, false), Vec::<BattleTag>::new());
        assert_eq!(battle_tags(true, false), vec![BattleTag::Autobattle]);
        assert_eq!(battle_tags(false, true), vec![BattleTag::ReloadBeforeEnd]);
    }

    #[test]
    fn the_base_screens_are_not_counted_as_windows() {
        assert!(is_base_screen("world map"));
        assert!(!is_base_screen("hero"));
        assert!(!is_base_screen("save"));
    }
}
