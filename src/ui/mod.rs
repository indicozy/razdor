//! macroquad presentation layer. Reads rules state and calls rules methods.
pub mod assets;
pub mod audio;
pub mod battle_view;
pub mod building_view;
pub mod chrome;
pub mod dialog;
pub mod dt_art;
pub mod dt_font;
pub mod editor;
pub mod game_bar;
pub mod hotkeys;
pub mod items_view;
pub mod jukebox;
pub mod language;
pub mod main_menu;
pub mod minimap;
pub mod new_game;
pub mod saves;
pub mod screens;
pub mod snapshot;
pub mod spellbook;
pub mod story;
pub mod terrain;
pub mod unit_drag;
pub mod unit_sheet;
pub mod widgets;
pub mod world_view;

use std::collections::VecDeque;

use macroquad::prelude::{is_key_down, is_key_pressed, KeyCode};
use std::path::PathBuf;
use std::sync::Arc;

use razdor::dt::dtm::Scenario;
use razdor::i18n::tr;
use razdor::rules::content::Content;
use razdor::rules::events::EventOutcome;
use razdor::rules::game::{Foe, Game};
use razdor::rules::save::{self, Install};
use razdor::rules::script::ScriptEnd;
use razdor::rules::world::LocationKind;

use assets::Assets;
use audio::{Audio, Cue, Mood};
use battle_view::BattleView;
use building_view::BuildingView;
use dialog::{Close, Dialog};
use world_view::MapView;

pub enum Screen {
    /// The original's main menu (the first screen).
    MainMenu,
    /// The authors' window over the main menu, opened at this time (`get_time`).
    Authors(f64),
    /// The settings window over the main menu.
    Options,
    /// The built-in demo or a map of the install.
    ScenarioSelect,
    /// "Обучающий сценарий": the offer to play the tutorial, before the first new game.
    TutorialOffer,
    /// Hero class for the demo (`None`) or for `scenarios[i]`.
    ClassSelect { scenario: Option<usize> },
    WorldMap,
    /// The window of the building the hero stands in.
    Building(BuildingView),
    /// Hero and army screen: selected squad member, backpack scroll, and the building window
    /// "Back" returns to (the map if none).
    Squad { selected: usize, scroll: usize, back: Option<BuildingView> },
    Battle(Box<BattleView>),
    /// The journal: its tab, selected line and scrolling.
    Journal(story::JournalView),
    /// The spell book, with the selected cell.
    Spellbook { selected: usize },
    /// "Выход из игры" over the map (the bar's X, Esc); `true` while the restart question
    /// is open.
    Menu(bool),
    /// The settings window over the map (the bar's gears).
    Settings,
    Save(saves::SaveView),
    Load(saves::LoadView),
    GameOver,
    Victory,
    /// The map editor (`ui::editor`).
    Editor,
}

/// A scenario of the player's install, loaded for the select screen.
/// The play log's line for a game that starts: how, the map, the hero, the money.
pub(super) fn play_game_line(game: &Game, how: &str) -> String {
    let map = match &game.origin {
        Some(save::ScenarioRef::Map { file, .. }) => file.clone(),
        Some(save::ScenarioRef::Demo) | None => "demo".into(),
    };
    format!(
        "GAME {how}: map «{map}», hero {} «{}» at {:?}, gold {}, mana {}, squad {}",
        game.hero().name(&game.content),
        game.hero_name.clone().unwrap_or_default(),
        game.tile(),
        game.gold,
        game.mana,
        game.squad.len()
    )
}

pub struct ScenarioEntry {
    /// File name without the extension.
    pub file: String,
    pub path: PathBuf,
    pub scenario: Scenario,
}

/// The map music's rotation as the original runs it (interface.md §13): the last pick, when
/// the next change is due (in macroquad's clock, seconds), and whether a won battle's triumph
/// is playing (the world map waits for it; closing a dialog changes the track at once).
#[derive(Default)]
struct MapMusic {
    pick: usize,
    due: f64,
    triumph: bool,
}

pub struct App {
    pub assets: Assets,
    /// The built-in demo content.
    pub demo: Arc<Content>,
    /// Content of the player's install, if present.
    pub dt_content: Option<Arc<Content>>,
    /// Maps of the install (read at start, never written).
    pub scenarios: Vec<ScenarioEntry>,
    pub game: Option<Game>,
    pub screen: Screen,
    /// One-line notice shown on the world map / town screens.
    pub message: Option<String>,
    /// Modal windows waiting to be read (noon reports, victories), first on top.
    pub dialogs: VecDeque<Dialog>,
    pub map_view: MapView,
    /// A save file the load window picked: loaded after the frame.
    pub pending_load: Option<PathBuf>,
    /// Why the last load failed (shown in the load window).
    pub load_error: Option<String>,
    pub audio: Audio,
    /// The map music's rotation (`rules::music`).
    map_music: MapMusic,
    /// The screen of the last frame, to hear windows open and battles begin.
    last_screen: Option<std::mem::Discriminant<Screen>>,
    /// Gold at the end of the last frame of this game (`None` right after a new game or load).
    last_gold: Option<i32>,
    /// The play log's last screen and message, to write each change once.
    play_last: (&'static str, Option<String>),
    /// The map editor, kept while a test play runs.
    editor: Option<Box<editor::EditorScreen>>,
    /// The game is a test play of the editor's map: leaving it returns to the editor.
    test_play: bool,
    /// The F1 key list is open over the screen.
    help: bool,
    /// The interface language the demo content was built in.
    lang: razdor::i18n::Lang,
    /// "Выход" was picked in the main menu: the process ends after this frame.
    pub quit: bool,
}

impl App {
    pub fn new(assets: Assets, demo: Arc<Content>) -> Self {
        let dt_content = assets.dt.as_ref().map(|d| Arc::new(Content::from_dt(&d.install)));
        let scenarios = assets
            .dt
            .iter()
            .flat_map(|d| d.install.maps.iter())
            .filter_map(|m| match m.load() {
                Ok(scenario) => Some(ScenarioEntry { file: m.name.clone(), path: m.path.clone(), scenario }),
                Err(e) => {
                    razdor::diag!("{}: {e}", m.name);
                    None
                }
            })
            .collect();
        let audio = Audio::new(assets.dt.as_ref().map(|d| &d.install));
        App {
            assets,
            demo,
            dt_content,
            scenarios,
            game: None,
            screen: Screen::MainMenu,
            message: None,
            dialogs: VecDeque::new(),
            map_view: MapView::default(),
            pending_load: None,
            load_error: None,
            audio,
            map_music: MapMusic::default(),
            last_screen: None,
            last_gold: None,
            play_last: ("", None),
            editor: None,
            test_play: false,
            help: false,
            lang: razdor::i18n::lang(),
            quit: false,
        }
    }

    /// Opens the map editor (the title screen's button and `--editor`).
    pub fn open_editor(&mut self) {
        if self.editor.is_none() {
            let mut ed = editor::EditorScreen::new(&self.assets, self.dt_content.clone(), self.demo.clone());
            // The original opens the last map at start-up (not in a debug snapshot, whose
            // scene says what to show).
            if snapshot::target().is_none() {
                ed.reopen_last();
            }
            self.editor = Some(Box::new(ed));
        }
        self.screen = Screen::Editor;
    }

    /// After an EN / RU switch: the demo's names and descriptions in the new language (for
    /// the next new game or load; a running game keeps its content).
    fn follow_language(&mut self) {
        let now = razdor::i18n::lang();
        if now != self.lang {
            self.lang = now;
            self.demo = Arc::new(Content::builtin());
        }
    }

    /// A frame of the editor; test play starts a game on the edited map.
    fn editor_frame(&mut self) {
        // F2 is the editor's quick save there (the original's); the language switches on the
        // title screen.
        let Some(ed) = self.editor.as_mut() else {
            self.open_editor();
            return;
        };
        match ed.frame(&self.assets) {
            editor::EditorAction::None => {}
            editor::EditorAction::Exit => {
                self.editor = None;
                self.screen = Screen::MainMenu;
            }
            editor::EditorAction::TestPlay { scenario, content, class } => {
                let mut game = Game::from_scenario(content, &scenario, class);
                game.set_hero_name("");
                self.dialogs.clear();
                self.message = Some(tr("Test play: Esc > Main menu returns to the editor.").to_string());
                self.map_view.reset();
                self.map_view.forget_shows();
                self.last_gold = None;
                razdor::diag::play(&game.clock.label(), &play_game_line(&game, "editor test play"));
                self.game = Some(game);
                self.test_play = true;
                self.screen = Screen::WorldMap;
            }
        }
    }

    /// Loads save file `path`: the demo from the built-in data, a map from the install (the
    /// same map file only). A pending battle starts again; a pending question is asked again.
    fn load(&mut self, path: &std::path::Path) {
        let install = self.assets.dt.as_ref().zip(self.dt_content.clone()).map(|(d, content)| Install { dir: &d.install.dir, content });
        match save::load(path, self.demo.clone(), install.as_ref()) {
            Ok(mut game) => {
                self.dialogs.clear();
                self.message = None;
                self.load_error = None;
                self.map_view.reset();
                self.map_view.forget_shows();
                self.last_gold = None;
                razdor::diag::play(&game.clock.label(), &play_game_line(&game, "loaded"));
                self.dt_content = saves::session_content(self.dt_content.take(), &game);
                if let Some(q) = game.pending_question() {
                    story::show(&game, &EventOutcome::Question(q), &mut self.message, &mut self.dialogs);
                }
                self.screen = if game.foe.is_some() {
                    Screen::Battle(Box::new(BattleView::new(game.start_battle())))
                } else {
                    Screen::WorldMap
                };
                self.game = Some(game);
            }
            Err(e) => self.load_error = Some(razdor::trf!("Cannot load: {e}.", e)),
        }
    }

    /// "Рестарт": the scenario under way from its beginning (the map is found again by its
    /// file name), as the original's restart snapshot holds it ([`Game::restart`]).
    fn restart(&mut self) {
        let Some(old) = self.game.as_ref() else { return };
        let game = match &old.origin {
            Some(save::ScenarioRef::Map { file, .. }) => {
                let (Some(e), Some(c)) = (self.scenarios.iter().find(|e| &e.file == file), self.dt_content.clone()) else {
                    self.message = Some(tr("The map of this game is not in the install.").into());
                    self.screen = Screen::WorldMap;
                    return;
                };
                // The original's restart snapshot: a campaign map starts again with what
                // the map before carried over (0x4b5ff8).
                let g = old.restart(c, &e.scenario);
                razdor::diag::play(&g.clock.label(), &play_game_line(&g, "restart"));
                g
            }
            _ => {
                let name = old.hero_name.clone().unwrap_or_default();
                screens::start_game(&self.demo, None, old.start_class(), &name)
            }
        };
        self.game = Some(game);
        self.dialogs.clear();
        self.message = None;
        self.map_view.reset();
        self.last_gold = None;
        self.screen = Screen::WorldMap;
    }

    /// Before the process ends: the music stops and the settings are written.
    pub fn shutdown(&mut self) {
        self.audio.shutdown();
        // The editor writes its settings as the original's does when it closes.
        if let (Some(ed), None) = (self.editor.as_mut(), snapshot::target()) {
            ed.close();
        }
    }

    /// The music the current screen wants.
    fn mood(&self) -> Mood {
        match &self.screen {
            Screen::Authors(_) => Mood::Credits,
            Screen::MainMenu | Screen::Options | Screen::ScenarioSelect | Screen::TutorialOffer | Screen::ClassSelect { .. } | Screen::Editor => Mood::Menu,
            Screen::Load(v) if v.back == saves::Back::Title || self.game.is_none() => Mood::Menu,
            Screen::Battle(_) => Mood::Battle { garrison: matches!(self.game.as_ref().and_then(|g| g.foe.as_ref()), Some(Foe::Garrison(_))) },
            Screen::GameOver => Mood::Lost,
            Screen::Victory => Mood::Won,
            _ if self.game.is_some() => Mood::Map,
            _ => Mood::Menu,
        }
    }

    /// The map music's change when it is due (interface.md §13): the pick and its time are
    /// draws of the game's generator, as in the original, so they shift the rolls that follow.
    fn rotate_music(&mut self) {
        let Some(game) = self.game.as_mut() else { return };
        let (pick, ms) = game.music_rotate(self.map_music.pick);
        self.map_music = MapMusic { pick, due: macroquad::prelude::get_time() + ms as f64 / 1000.0, triumph: false };
        self.audio.set_map_track(razdor::rules::music::ROTATION[pick]);
    }

    /// Sounds that follow from what changed this frame (a window opened, a battle began, gold
    /// came in, a dialog appeared), then the audio frame.
    fn sounds(&mut self) {
        // A map start or a load starts the world theme; its first change was drawn then.
        let clock = macroquad::prelude::get_time();
        if let Some(wait) = self.game.as_mut().and_then(|g| g.take_music_wait()) {
            let pick = razdor::rules::music::WORLD_THEME;
            self.map_music = MapMusic { pick, due: clock + wait as f64 / 1000.0, triumph: false };
            self.audio.set_map_track(razdor::rules::music::ROTATION[pick]);
        }
        // The change is checked by the map and its windows, not in battle or the menus; the
        // world map alone waits while the triumph plays.
        // A battle's theme replaces the triumph of an earlier win; a win starts it.
        if let Screen::Battle(v) = &self.screen {
            self.map_music.triumph = v.won();
        }
        let waits = self.map_music.triumph && matches!(self.screen, Screen::WorldMap);
        if self.mood() == Mood::Map && clock >= self.map_music.due && !waits {
            self.rotate_music();
        }
        let now = std::mem::discriminant(&self.screen);
        if self.last_screen != Some(now) {
            // The village and shipyard windows open with an event chord (a draw).
            let chord_window = match (&self.screen, self.game.as_ref()) {
                (Screen::Building(_), Some(g)) => g.location.is_some_and(|l| matches!(g.world.locations[l].kind, LocationKind::Village | LocationKind::Shipyard)),
                _ => false,
            };
            match self.screen {
                Screen::Building(_) if chord_window => {
                    let k = self.game.as_mut().map_or(0, |g| g.event_chord());
                    audio::cue(Cue::Event(k as u8));
                }
                Screen::Battle(_) => audio::cue(Cue::BattleHorn),
                Screen::Building(_)
                | Screen::Squad { .. }
                | Screen::Journal(_)
                | Screen::Spellbook { .. }
                | Screen::Menu(_)
                | Screen::Settings
                | Screen::Save(_)
                | Screen::Load(_) => audio::cue(Cue::Panel),
                _ => {}
            }
        }
        self.last_screen = Some(now);
        let new_game = matches!(self.screen, Screen::ScenarioSelect | Screen::TutorialOffer | Screen::ClassSelect { .. });
        let gold = self.game.as_ref().filter(|_| !new_game).map(|g| g.gold);
        if let (Some(before), Some(after)) = (self.last_gold, gold) {
            if after > before {
                audio::cue(Cue::Gold);
            }
        }
        self.last_gold = gold;
        if let Some(d) = self.dialogs.front_mut().filter(|d| !d.cued) {
            d.cued = true;
            if d.event.is_some() {
                let k = self.game.as_mut().map_or(0, |g| g.event_chord());
                audio::cue(Cue::Event(k as u8));
            } else {
                audio::cue(Cue::Panel);
            }
        }
        // N: music on/off (not while typing or answering a question: there any key answers;
        // not with Ctrl, the editor's Ctrl+N).
        let ctrl = is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl);
        if !self.help && hotkeys::music_key_allowed(self.guard(), ctrl) && is_key_pressed(KeyCode::N) {
            self.audio.settings.music_muted = !self.audio.settings.music_muted;
        }
        let mood = self.mood();
        self.audio.frame(mood);
    }

    /// The current screen, by what its keys do.
    fn place(&self) -> hotkeys::Place {
        use hotkeys::Place;
        match &self.screen {
            Screen::MainMenu | Screen::Authors(_) | Screen::Options | Screen::ScenarioSelect | Screen::TutorialOffer => Place::Title,
            Screen::ClassSelect { .. } => Place::ClassSelect,
            Screen::WorldMap => Place::WorldMap,
            Screen::Building(_) => Place::Building,
            Screen::Squad { .. } => Place::Army,
            Screen::Battle(_) => Place::Battle,
            Screen::Journal(_) => Place::Journal,
            Screen::Spellbook { .. } => Place::Spellbook,
            Screen::Menu(_) | Screen::Settings => Place::Menu,
            Screen::Save(_) => Place::Save,
            Screen::Load(_) => Place::Load,
            Screen::GameOver | Screen::Victory => Place::End,
            Screen::Editor => Place::Editor,
        }
    }

    /// What stands in the way of shortcut keys this frame.
    fn guard(&self) -> hotkeys::Guard {
        hotkeys::Guard {
            typing: hotkeys::typing(self.place(), widgets::typing()),
            dialog: !self.dialogs.is_empty(),
            game: self.game.is_some(),
            foe: self.game.as_ref().is_some_and(|g| g.foe.is_some()),
            endless: self.game.as_ref().is_some_and(|g| g.endless_waiting()),
        }
    }

    /// F5: writes the quick save (a manual save named "Quick save", replacing the last).
    fn quick_save(&mut self) {
        let Some(game) = &self.game else { return };
        self.message = Some(match save::default_dir() {
            None => tr("No data folder for saves: set RAZDOR_SAVE_DIR.").to_string(),
            Some(dir) => match save::quick_save(&dir, game) {
                Ok(_) => tr("Quick save written (F9 loads it).").to_string(),
                Err(e) => razdor::trf!("Not saved: {e}.", e),
            },
        });
    }

    /// F9: loads the quick save, if there is one.
    fn quick_load(&mut self) {
        match save::default_dir().and_then(|d| save::quick_save_path(&d)) {
            Some(path) => {
                self.load(&path);
                self.message = Some(self.load_error.take().unwrap_or_else(|| tr("Quick save loaded.").to_string()));
            }
            None => self.message = Some(tr("No quick save yet: F5 writes one.").to_string()),
        }
    }

    /// The frame rate in the top right corner, when the settings ask for it: frames counted
    /// over each half second, so the number stays readable.
    pub fn draw_fps(&self) {
        thread_local! {
            /// (frames so far, when counting began, the last rate shown)
            static COUNT: std::cell::Cell<(u32, f64, u32)> = const { std::cell::Cell::new((0, 0.0, 0)) };
        }
        let now = macroquad::prelude::get_time();
        let (mut n, mut since, mut shown) = COUNT.with(|c| c.get());
        n += 1;
        if now - since >= 0.5 {
            shown = (n as f64 / (now - since)).round() as u32;
            (n, since) = (0, now);
        }
        COUNT.with(|c| c.set((n, since, shown)));
        if !self.audio.settings.show_fps {
            return;
        }
        let k = chrome::k();
        let label = format!("FPS {shown}");
        let size = (14.0 * k).round();
        let w = widgets::measure(&label, size).width;
        chrome::shadow_text(&label, macroquad::prelude::screen_width() - w - 8.0 * k, 8.0 * k + size, size, chrome::GOLD);
    }

    /// The play log (`diag::play`): the screen when it changes and every new message line.
    fn play_log_frame(&mut self) {
        let when = self.game.as_ref().map_or_else(|| "menu".to_string(), |g| g.clock.label());
        let screen = self.screen_name();
        if self.play_last.0 != screen {
            razdor::diag::play(&when, &format!("SCREEN {screen}"));
            self.play_last.0 = screen;
        }
        if self.message.is_some() && self.message != self.play_last.1 {
            razdor::diag::play(&when, &format!("MESSAGE {}", self.message.as_deref().unwrap_or_default()));
        }
        self.play_last.1 = self.message.clone();
    }

    /// The current screen's name, for the frame timer (`RAZDOR_PROFILE`).
    pub fn screen_name(&self) -> &'static str {
        match self.screen {
            Screen::MainMenu => "main menu",
            Screen::Authors(_) => "authors",
            Screen::Options => "options",
            Screen::ScenarioSelect => "scenario select",
            Screen::TutorialOffer => "tutorial offer",
            Screen::ClassSelect { .. } => "class select",
            Screen::WorldMap => "world map",
            Screen::Building(_) => "building",
            Screen::Squad { .. } => "army",
            Screen::Battle(_) => "battle",
            Screen::Journal(_) => "journal",
            Screen::Spellbook { .. } => "spell book",
            Screen::Menu(_) => "menu",
            Screen::Settings => "settings",
            Screen::Save(_) => "save",
            Screen::Load(_) => "load",
            Screen::GameOver => "game over",
            Screen::Victory => "victory",
            Screen::Editor => "editor",
        }
    }

    pub fn frame(&mut self) {
        chrome::begin_frame();
        widgets::track_held_key();
        self.follow_language();
        self.sounds();
        // The battle AI's level from the settings: the next battle uses it.
        if let Some(g) = self.game.as_mut() {
            g.improved_ai = main_menu::expert_ai(&self.audio.settings);
        }
        if matches!(self.screen, Screen::Editor) {
            self.editor_frame();
            return;
        }
        // A dialog or the key list on top: the screen below is drawn but takes no input.
        let place = self.place();
        let guard = self.guard();
        widgets::set_input_blocked(!self.dialogs.is_empty() || self.help);
        let mut restart = false;
        let mut next = match (&mut self.screen, &mut self.game) {
            (Screen::MainMenu, _) => match main_menu::frame() {
                Some(main_menu::Pick::NewGame) if new_game::tutorial_map(&self.scenarios).is_some() => Some(Screen::TutorialOffer),
                Some(main_menu::Pick::NewGame) => Some(Screen::ScenarioSelect),
                Some(main_menu::Pick::Load) => Some(Screen::Load(saves::LoadView::new(saves::Back::Title))),
                Some(main_menu::Pick::Editor) => Some(Screen::Editor),
                Some(main_menu::Pick::Exit) => {
                    self.quit = true;
                    None
                }
                Some(main_menu::Pick::Options) => Some(Screen::Options),
                Some(main_menu::Pick::Authors) => Some(Screen::Authors(macroquad::prelude::get_time())),
                None => None,
            },
            (Screen::Authors(started), _) => main_menu::authors(*started).then_some(Screen::MainMenu),
            (Screen::Options, _) => main_menu::options(&mut self.audio.settings).then_some(Screen::MainMenu),
            (Screen::ScenarioSelect, _) => new_game::scenario_select(&self.scenarios, self.dt_content.is_some()),
            (Screen::TutorialOffer, _) => new_game::tutorial_offer(&self.scenarios),
            (Screen::ClassSelect { scenario }, game) => {
                let pick = scenario.and_then(|i| Some((self.scenarios.get(i)?, self.dt_content.clone()?)));
                new_game::class_select(game, &self.demo, pick, &self.assets)
            }
            (Screen::WorldMap, Some(game)) => {
                world_view::frame(game, &self.assets, &mut self.map_view, &mut self.message, &mut self.dialogs)
            }
            (Screen::Building(view), Some(game)) => {
                building_view::frame(game, &self.assets, view, &mut self.message, &mut self.dialogs)
            }
            (Screen::Squad { selected, scroll, back }, Some(game)) => {
                items_view::squad(game, &self.assets, selected, scroll, back, &mut self.message)
            }
            (Screen::Battle(view), Some(game)) => view.frame(game, &self.assets, &mut self.message, &mut self.dialogs),
            (Screen::Journal(view), Some(game)) => story::journal(game, &self.assets, view),
            (Screen::Spellbook { selected }, Some(game)) => {
                spellbook::frame(game, &self.assets, selected, &mut self.message)
            }
            (Screen::Menu(asking), Some(game)) => match saves::exit_window(game, &self.assets, asking) {
                (Some(saves::ExitChoice::Quit), _) => {
                    self.quit = true;
                    None
                }
                (Some(saves::ExitChoice::MainMenu), _) => Some(Screen::MainMenu),
                (Some(saves::ExitChoice::Restart), _) => {
                    restart = true;
                    None
                }
                (None, next) => next,
            },
            (Screen::Settings, Some(game)) => {
                world_view::backdrop_lit(game, &self.assets, Some(game_bar::BarButton::Settings));
                main_menu::options_window(&mut self.audio.settings).then_some(Screen::WorldMap)
            }
            (Screen::Save(view), Some(game)) => saves::save_screen(game, &self.assets, view, &mut self.message),
            (Screen::Load(view), game) => saves::load_screen(game.as_ref(), &self.assets, view, &mut self.pending_load, &self.load_error),
            (Screen::GameOver, game) => match screens::game_over(game) {
                (next, Some(screens::EndChoice::Load(path))) => {
                    self.pending_load = Some(path);
                    next
                }
                (_, Some(screens::EndChoice::Restart)) => {
                    restart = true;
                    None
                }
                (next, None) => next,
            },
            (Screen::Victory, game) => screens::victory(game, &self.scenarios, self.dt_content.clone()),
            (Screen::Editor, _) => None,
            (_, None) => Some(Screen::MainMenu),
        };
        widgets::set_input_blocked(false);
        // "Варианты выхода из битвы" chose.
        if let Screen::Battle(v) = &mut self.screen {
            match v.exit.take() {
                Some(saves::ExitChoice::Quit) => self.quit = true,
                Some(saves::ExitChoice::MainMenu) => next = Some(Screen::MainMenu),
                Some(saves::ExitChoice::Restart) => restart = true,
                None => {}
            }
        }
        if restart {
            self.restart();
            return;
        }
        // F1: the key list; F5 / F9: quick save and load (when the screen did not move on).
        let pressed = |k: hotkeys::Global| next.is_none() && hotkeys::allowed(place, k, guard) && is_key_pressed(k.key());
        if self.help {
            if hotkeys::help_overlay(place) {
                self.help = false;
            }
        } else if pressed(hotkeys::Global::Help) {
            self.help = true;
        } else if pressed(hotkeys::Global::Language) {
            language::toggle();
        } else if pressed(hotkeys::Global::QuickSave) {
            self.quick_save();
        } else if pressed(hotkeys::Global::QuickLoad) {
            self.quick_load();
            return;
        }
        if let Some(d) = self.dialogs.front() {
            if let Some(close) = dialog::draw(d, &self.assets) {
                let asked = self.dialogs.pop_front().is_some_and(|d| d.question);
                // Closing a dialog while the triumph plays changes the map track at once
                // (0x4c20b3).
                if self.map_music.triumph {
                    self.rotate_music();
                }
                // A scenario question: the answer goes to the event engine.
                if let (true, Some(game)) = (asked, self.game.as_mut()) {
                    let events = game.answer_question(close == Close::Yes);
                    let after = world_view::handle_events(game, events, &mut self.message, &mut self.dialogs);
                    next = next.or(after);
                }
            }
        }
        // A fight decided on the map or in a building begins once the messages of that moment
        // are read (the original shows a meeting's words over the map, then the battle).
        if next.is_none() && self.dialogs.is_empty() && matches!(self.screen, Screen::WorldMap | Screen::Building(_)) {
            if let Some(game) = self.game.as_mut().filter(|g| g.foe.is_some()) {
                next = Some(saves::battle(game));
            }
        }
        // A victory or defeat event ends the game once its window is read.
        if next.is_none() && self.dialogs.is_empty() {
            let end = self.game.as_ref().and_then(Game::script_end);
            match end {
                Some(ScriptEnd::Victory(_)) if !matches!(self.screen, Screen::Victory) => next = Some(Screen::Victory),
                Some(ScriptEnd::Defeat(_)) if !matches!(self.screen, Screen::GameOver) => next = Some(Screen::GameOver),
                _ => {}
            }
            // A world spell that killed the whole army loses the game (0x4900fc → 0x4af658).
            if next.is_none() && !matches!(self.screen, Screen::GameOver | Screen::Battle(..)) && self.game.as_ref().is_some_and(Game::army_fallen) {
                next = Some(Screen::GameOver);
            }
        }
        // The noon report asks for an autosave, named by the date.
        if let Some(g) = self.game.as_mut() {
            if let Some(name) = g.autosave_due.take() {
                saves::autosave(g, &name, false);
            }
        }
        if let Some(path) = self.pending_load.take() {
            self.load(&path);
            return;
        }
        // Leaving a test play (main menu, or "new game" on an end screen) returns to the editor.
        if self.test_play && matches!(next, Some(Screen::MainMenu | Screen::ScenarioSelect)) {
            self.test_play = false;
            self.game = None;
            self.dialogs.clear();
            self.message = None;
            self.screen = Screen::Editor;
            return;
        }
        if let Some(next) = next {
            if matches!(next, Screen::Load(_)) {
                self.load_error = None;
            }
            if matches!(next, Screen::MainMenu | Screen::ScenarioSelect) {
                self.dialogs.clear();
            }
            if matches!(next, Screen::WorldMap) && !matches!(self.screen, Screen::WorldMap) {
                self.map_view.reset();
            }
            if matches!(self.screen, Screen::ClassSelect { .. }) {
                self.message = None;
            }
            self.screen = next;
        }
        self.play_log_frame();
        // A new game may run on other content: draw its pictures.
        if let Some(g) = &self.game {
            if !Arc::ptr_eq(&g.content, self.assets.content()) {
                self.assets.set_content(g.content.clone());
            }
        }
    }
}
