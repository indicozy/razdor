//! macroquad presentation layer. Reads rules state and calls rules methods.
pub mod assets;
pub mod audio;
pub mod battle_view;
pub mod building_view;
pub mod chrome;
pub mod console;
pub mod custom_battle;
pub mod dialog;
pub mod display;
pub mod dt_art;
pub mod dt_font;
pub mod editor;
pub mod game_bar;
pub mod hotkeys;
pub mod item_filter;
pub mod items_view;
pub mod jukebox;
pub mod language;
pub mod main_menu;
pub mod minimap;
pub mod new_game;
pub mod saves;
pub mod screens;
pub mod snapshot;
pub mod spell_badges;
pub mod spellbook;
pub mod story;
pub mod terrain;
pub mod unit_drag;
pub mod unit_sheet;
pub mod update_view;
pub mod widgets;
pub mod world_view;

use std::collections::VecDeque;

use macroquad::prelude::{is_key_pressed, KeyCode};
use std::path::PathBuf;
use std::sync::Arc;

use razdor::dt::dtm::Scenario;
use razdor::i18n::tr;
use razdor::rules::content::Content;
use razdor::rules::formation::Formation;
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
    Squad { selected: items_view::ArmySel, scroll: usize, back: Option<BuildingView> },
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
    /// The custom battle setup over the main menu (`ui::custom_battle`).
    CustomSetup,
    /// A round of a custom battle.
    CustomBattle(Box<BattleView>),
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
    /// The play log's last screen and message, to write each change once.
    play_last: (&'static str, Option<String>),
    /// The map editor, kept while a test play runs.
    editor: Option<Box<editor::EditorScreen>>,
    /// The game is a test play of the editor's map: leaving it returns to the editor.
    test_play: bool,
    /// The F1 key list is open over the screen.
    help: bool,
    /// The custom battles of this run: their setup and score, kept from round to round.
    custom: Option<custom_battle::SetupView>,
    /// The cheat console (`~` on the map and in battle).
    console: console::Console,
    /// The console holds the keys this frame (open, or just closed by them).
    console_held: bool,
    /// The interface language the demo content was built in.
    lang: razdor::i18n::Lang,
    /// "Выход" was picked in the main menu: the process ends after this frame.
    pub quit: bool,
    /// The update window (`update_view`) is over the screen this frame.
    update_open: bool,
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
        minimap::set_size(audio.settings.minimap_size);
        let mut app = App {
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
            play_last: ("", None),
            editor: None,
            test_play: false,
            help: false,
            custom: None,
            console: console::Console::default(),
            console_held: false,
            lang: razdor::i18n::lang(),
            quit: false,
            update_open: false,
        };
        app.follow_row_setting();
        app
    }

    /// Opens the map editor (the title screen's button and `--editor`).
    pub fn open_editor(&mut self) {
        if self.editor.is_none() {
            self.editor = Some(Box::new(editor::EditorScreen::new(&self.assets, self.dt_content.clone(), self.demo.clone())));
        }
        self.screen = Screen::Editor;
    }

    /// The content custom battles are fought with: the install's, else the demo's.
    fn custom_content(&self) -> Arc<Content> {
        self.dt_content.clone().unwrap_or_else(|| self.demo.clone())
    }

    /// Opens the custom battle setup (the main menu's link), as it was left last time.
    pub fn open_custom(&mut self) {
        let content = self.custom_content();
        self.custom.get_or_insert_with(|| custom_battle::SetupView::new(&content));
        self.screen = Screen::CustomSetup;
    }

    /// A round of the custom battle, with the setup as it stands (`None` while an army is
    /// empty). The round before it has put the battle globals back.
    pub fn custom_round(&mut self) -> Option<Screen> {
        let content = self.custom_content();
        let expert = main_menu::expert_ai(&self.audio.settings);
        let view = self.custom.get_or_insert_with(|| custom_battle::SetupView::new(&content));
        view.session.end_round();
        let battle = view.session.start_round(&content, expert)?;
        Some(Screen::CustomBattle(Box::new(BattleView::custom(battle, view.session.setup.control))))
    }

    /// A new game may run on other content: its pictures are drawn (a custom battle's are
    /// its own).
    fn show_content(&mut self) {
        let shown = if self.in_custom() { Some(self.custom_content()) } else { self.game.as_ref().map(|g| g.content.clone()) };
        if let Some(c) = shown {
            if !Arc::ptr_eq(&c, self.assets.content()) {
                self.assets.set_content(c);
            }
        }
    }

    /// The setup or a round of a custom battle is on screen.
    fn in_custom(&self) -> bool {
        matches!(self.screen, Screen::CustomSetup | Screen::CustomBattle(_))
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
        if hotkeys::allowed(hotkeys::Place::Editor, hotkeys::Global::Language, self.guard())
            && !widgets::popup_open()
            && is_key_pressed(language::KEY)
        {
            language::toggle();
        }
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
                self.map_view.open_around_hero(&game);
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
        self.map_view.open_around_hero(&game);
        self.game = Some(game);
        self.dialogs.clear();
        self.message = None;
        self.map_view.reset();
        self.screen = Screen::WorldMap;
    }

    /// Before the process ends: the music stops and the settings are written.
    pub fn shutdown(&mut self) {
        self.audio.shutdown();
    }

    /// The music the current screen wants.
    fn mood(&self) -> Mood {
        match &self.screen {
            Screen::Authors(_) => Mood::Credits,
            Screen::MainMenu | Screen::Options | Screen::ScenarioSelect | Screen::TutorialOffer | Screen::ClassSelect { .. } | Screen::Editor | Screen::CustomSetup => Mood::Menu,
            Screen::CustomBattle(_) => Mood::Battle { garrison: false },
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
            // The side windows sound only as their panel icon is pressed (`game_bar`).
            match self.screen {
                Screen::Building(_) if chord_window => {
                    let k = self.game.as_mut().map_or(0, |g| g.event_chord());
                    audio::cue(Cue::Event(k as u8));
                }
                // The building window opens on its first tab, highlighted (interface.md §14).
                Screen::Building(_) => audio::cue(Cue::CastSpell),
                Screen::Battle(_) | Screen::CustomBattle(_) => audio::cue(Cue::BattleHorn),
                _ => {}
            }
        }
        self.last_screen = Some(now);
        // A window waits while the camera flies to the places of the event before it.
        let held = matches!(self.screen, Screen::WorldMap) && self.map_view.holds_dialogs(&self.dialogs);
        if let Some(d) = self.dialogs.front_mut().filter(|d| !d.cued && !held && !d.waiting(clock)) {
            d.cued = true;
            if d.event.is_some() || d.chord {
                let k = self.game.as_mut().map_or(0, |g| g.event_chord());
                audio::cue(Cue::Event(k as u8));
            } else {
                audio::cue(Cue::Panel);
            }
        }
        // A stop's snap of the armies and its idle draws, after the chords of the windows the
        // stop opened (0x4ad8a0).
        if let Some(g) = self.game.as_mut() {
            g.armies_snap();
        }
        // N: music on/off (not while typing or answering a question: there any key answers).
        // A key that cuts a wait on the map does nothing else (`world_view::frame`).
        let cuts_wait = matches!(self.screen, Screen::WorldMap) && self.dialogs.is_empty() && self.game.as_ref().is_some_and(|g| g.waiting());
        if !self.help && !cuts_wait && hotkeys::shortcuts_allowed(self.guard()) && is_key_pressed(KeyCode::N) {
            self.audio.settings.music_muted = !self.audio.settings.music_muted;
        }
        let mood = self.mood();
        self.audio.frame(mood);
    }

    /// An event read in a building window (a quest taken in the main hall) shows its places
    /// at once, as the original does: its OK queues the camera's glides (0x4ab1ec → 0x4af96c,
    /// 0x4af83c) and they play over the world map, then the building window comes back as it
    /// was, without a sound (checked live on РК1, interface.md §9.8). `leaving`: the screen
    /// changes this frame anyway.
    fn fly_from_building(&mut self, leaving: bool) {
        if leaving {
            self.map_view.back_to = None;
            return;
        }
        let Some(game) = self.game.as_ref() else { return };
        let in_building = matches!(self.screen, Screen::Building(_));
        let on_map = matches!(self.screen, Screen::WorldMap);
        let aside = self.map_view.back_to.is_some();
        let due = in_building && self.map_view.shows_due(game, &self.dialogs);
        match world_view::building_flight(in_building, aside && on_map, due, self.map_view.flying()) {
            world_view::BuildingFlight::StepAside => {
                if let Screen::Building(view) = std::mem::replace(&mut self.screen, Screen::WorldMap) {
                    self.map_view.back_to = Some(view);
                }
                self.map_view.reset();
            }
            world_view::BuildingFlight::ComeBack => {
                if let Some(view) = self.map_view.back_to.take() {
                    self.screen = Screen::Building(view);
                }
            }
            world_view::BuildingFlight::Stay => return,
        }
        // Silent both ways: the map has no sound of its own, the window is not opened anew.
        self.last_screen = Some(std::mem::discriminant(&self.screen));
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
            Screen::Battle(_) | Screen::CustomBattle(_) => Place::Battle,
            Screen::CustomSetup => Place::Custom,
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
            typing: hotkeys::typing(self.place(), widgets::typing()) || self.console.open || self.console_held,
            dialog: !self.dialogs.is_empty() || self.map_view.back_to.is_some() || self.update_open,
            game: self.game.is_some(),
            foe: self.game.as_ref().is_some_and(|g| g.foe.is_some()),
            endless: self.game.as_ref().is_some_and(|g| g.endless_waiting()),
        }
    }

    /// Runs a command of the cheat console on the game and the battle on screen (a custom
    /// battle has no game behind it); its answer goes to the console and the play log. The
    /// screen the time it let pass leads to, if any.
    fn run_cheat(&mut self, line: &str) -> Option<Screen> {
        use razdor::rules::cheats;
        let when = self.game.as_ref().map_or_else(|| "menu".to_string(), |g| g.clock.label());
        let cheat = match cheats::parse(line) {
            Ok(c) => c,
            Err(e) => {
                self.console.print(e.to_string(), console::Kind::Error);
                return None;
            }
        };
        let custom = self.in_custom();
        let battle = match &mut self.screen {
            Screen::Battle(v) | Screen::CustomBattle(v) => Some(v.battle_mut()),
            _ => None,
        };
        let game = self.game.as_mut().filter(|_| !custom);
        match cheats::run(&cheat, game, battle) {
            Ok(done) => {
                if cheat != cheats::Cheat::Help {
                    razdor::diag::play(&when, &format!("CHEAT {line}: {}", done.lines.join(" / ")));
                }
                for l in done.lines {
                    self.console.print(l, console::Kind::Said);
                }
                let game = self.game.as_mut()?;
                world_view::handle_events(game, done.events, &mut self.message, &mut self.dialogs)
            }
            Err(e) => {
                razdor::diag::play(&when, &format!("CHEAT {line}: refused: {e}"));
                self.console.print(e, console::Kind::Error);
                None
            }
        }
    }

    /// F5: writes a quick save (one of five, over the oldest; `save::quick_save`).
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

    /// F9: loads the newest quick save, if there is one.
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
            Screen::CustomSetup => "custom battle setup",
            Screen::CustomBattle(_) => "custom battle",
        }
    }

    /// The install's "wide front row in battle" (`OptValue11`; wide without an install).
    fn install_wide_row(&self) -> bool {
        self.assets.dt.as_ref().is_none_or(|d| d.install.settings.wide_row)
    }

    /// The front row's width of the settings, at once: new games take it, and so does the game
    /// under way from its next battle (not during one; its saves then record the new width).
    fn follow_row_setting(&mut self) {
        let wide = main_menu::wide_row(&self.audio.settings, self.install_wide_row());
        let want = if wide { Formation::WIDE } else { Formation::VANILLA };
        if let Some(c) = self.dt_content.as_mut().filter(|c| c.formation != want) {
            *c = Arc::new(c.with_formation(want));
        }
        if !matches!(self.screen, Screen::Battle(_)) {
            if let Some(g) = self.game.as_mut() {
                g.set_formation(want);
            }
        }
    }

    pub fn frame(&mut self) {
        display::follow_settings(&self.audio.settings);
        // A minimap resized by the player is kept once the drag ends (saved with the settings).
        if !minimap::resizing() && minimap::size() != self.audio.settings.minimap_size {
            self.audio.settings.minimap_size = minimap::size();
        }
        chrome::begin_frame();
        widgets::track_held_key();
        self.follow_language();
        // A screen opened from outside the frame (a snapshot scene) draws its content.
        self.show_content();
        // A custom round left in any way (its buttons, F9) puts the battle globals back.
        if !matches!(self.screen, Screen::CustomBattle(_)) {
            if let Some(c) = self.custom.as_mut() {
                c.session.end_round();
            }
        }
        self.sounds();
        // The battle AI's level from the settings: the next battle uses it.
        if let Some(g) = self.game.as_mut() {
            g.improved_ai = main_menu::expert_ai(&self.audio.settings);
            g.friends_let_pass = self.audio.settings.friends_let_pass;
        }
        // The front row's width from the settings: the next new game uses it.
        self.follow_row_setting();
        // Updates: the check at start, and "Always" putting a new release in place.
        update_view::tick(self.audio.settings.updates);
        if matches!(self.screen, Screen::Editor) {
            self.editor_frame();
            return;
        }
        // The cheat console on the map and in battle: while it is open the screen takes no keys.
        let place = self.place();
        let console_here = matches!(place, hotkeys::Place::WorldMap | hotkeys::Place::Battle) && (self.game.is_some() || self.in_custom());
        // The update window opens only at a calm moment: nothing else on top, no battle due.
        let calm = match &self.screen {
            Screen::MainMenu | Screen::Options | Screen::Settings => true,
            Screen::WorldMap => self.game.as_ref().is_some_and(|g| g.foe.is_none() && !g.step_playing()),
            _ => false,
        } && self.dialogs.is_empty()
            && !self.help
            && !self.console.open
            && self.map_view.back_to.is_none()
            && !widgets::popup_open();
        self.update_open = update_view::wanted(self.audio.settings.updates, calm);
        let (held, entered) = self.console.input(console_here && self.dialogs.is_empty() && !self.help && !self.update_open && !widgets::typing());
        self.console_held = held;
        if held {
            // A key the console took is not held for the map once it closes.
            widgets::forget_held_key();
        }
        let console_next = entered.and_then(|line| self.run_cheat(&line));
        // A dialog or the key list on top: the screen below is drawn but takes no input.
        let guard = self.guard();
        // The flights of an event read in a building window take no input either.
        widgets::set_input_blocked(!self.dialogs.is_empty() || self.help || held || self.map_view.back_to.is_some() || self.update_open);
        let mut restart = false;
        let mut custom_round = false;
        let custom_content = self.custom_content();
        let install_wide = self.install_wide_row();
        let mut next = match (&mut self.screen, &mut self.game) {
            (Screen::MainMenu, _) => match main_menu::frame() {
                Some(main_menu::Pick::NewGame) if new_game::tutorial_map(&self.scenarios).is_some() => Some(Screen::TutorialOffer),
                Some(main_menu::Pick::NewGame) => Some(Screen::ScenarioSelect),
                Some(main_menu::Pick::Load) => Some(Screen::Load(saves::LoadView::new(saves::Back::Title))),
                Some(main_menu::Pick::Editor) => Some(Screen::Editor),
                Some(main_menu::Pick::Custom) => {
                    self.custom.get_or_insert_with(|| custom_battle::SetupView::new(&custom_content));
                    Some(Screen::CustomSetup)
                }
                Some(main_menu::Pick::Exit) => {
                    self.quit = true;
                    None
                }
                Some(main_menu::Pick::Options) => Some(Screen::Options),
                Some(main_menu::Pick::Authors) => Some(Screen::Authors(macroquad::prelude::get_time())),
                None => None,
            },
            (Screen::Authors(started), _) => main_menu::authors(*started).then_some(Screen::MainMenu),
            (Screen::Options, _) => main_menu::options(&mut self.audio.settings, install_wide).then_some(Screen::MainMenu),
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
                main_menu::options_window(&mut self.audio.settings, install_wide).then_some(Screen::WorldMap)
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
            (Screen::CustomSetup, _) => {
                let view = self.custom.get_or_insert_with(|| custom_battle::SetupView::new(&custom_content));
                match custom_battle::frame(view, &custom_content, &self.assets, &mut self.audio.settings) {
                    Some(custom_battle::Pick::Fight) => {
                        custom_round = true;
                        None
                    }
                    Some(custom_battle::Pick::Back) => Some(Screen::MainMenu),
                    None => None,
                }
            }
            (Screen::CustomBattle(view), _) => {
                let session = &mut self.custom.get_or_insert_with(|| custom_battle::SetupView::new(&custom_content)).session;
                match view.frame_custom(&self.assets, session) {
                    Some(battle_view::CustomEnd::Again) => {
                        custom_round = true;
                        None
                    }
                    Some(battle_view::CustomEnd::Setup) => Some(Screen::CustomSetup),
                    Some(battle_view::CustomEnd::MainMenu) => Some(Screen::MainMenu),
                    None => None,
                }
            }
            (_, None) => Some(Screen::MainMenu),
        };
        // A spell badge's hint over the screen that drew it (under the dialogs).
        spell_badges::flush();
        widgets::set_input_blocked(false);
        next = next.or(console_next);
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
        if custom_round {
            if matches!(self.screen, Screen::CustomBattle(_)) {
                // "Again": the horn as for any battle that begins.
                audio::cue(Cue::BattleHorn);
            }
            next = self.custom_round();
        }
        // F1: the key list; F5 / F9: quick save and load (when the screen did not move on).
        let pressed = |k: hotkeys::Global| next.is_none() && !widgets::input_swallowed() && hotkeys::allowed(place, k, guard) && is_key_pressed(k.key());
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
        let held = matches!(self.screen, Screen::WorldMap) && self.map_view.holds_dialogs(&self.dialogs);
        let clock = macroquad::prelude::get_time();
        if let Some(d) = self.dialogs.front().filter(|d| !held && !d.waiting(clock)) {
            if let Some(close) = dialog::draw(d, &self.assets) {
                let closed = self.dialogs.pop_front();
                let asked = closed.as_ref().is_some_and(|d| d.question);
                let read = closed.as_ref().is_some_and(|d| d.event.is_some() && !d.question);
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
                // A scenario event's window: the events after it run now (0x4ab1ec).
                if let (true, Some(game)) = (read, self.game.as_mut()) {
                    let events = game.event_window_closed();
                    if !events.is_empty() {
                        let after = world_view::handle_events(game, events, &mut self.message, &mut self.dialogs);
                        next = next.or(after);
                    }
                }
                // The windows are read: the building he walked into while one opened is
                // entered now (0x4bbc84).
                if let (true, Some(game)) = (self.dialogs.is_empty(), self.game.as_mut()) {
                    let events = game.enter_waiting_building();
                    if !events.is_empty() {
                        let after = world_view::handle_events(game, events, &mut self.message, &mut self.dialogs);
                        next = next.or(after);
                    }
                }
            }
        }
        // A scenario event's window the screen does not show (none queued) holds the
        // event scan no longer.
        let shown = self.dialogs.iter().any(|d| d.event.is_some() && !d.question);
        if let Some(game) = self.game.as_mut() {
            let events = game.release_unshown_window(shown);
            if !events.is_empty() {
                let after = world_view::handle_events(game, events, &mut self.message, &mut self.dialogs);
                next = next.or(after);
            }
        }
        self.fly_from_building(next.is_some());
        self.console.draw();
        // A newer release on offer, or how the update the player chose went.
        if self.update_open && next.is_none() && update_view::frame(matches!(self.screen, Screen::MainMenu | Screen::Options), &mut self.audio.settings.updates) {
            self.quit = true;
        }
        // A fight decided on the map or in a building begins once the messages of that moment
        // are read (the original shows a meeting's words over the map, then the battle), and
        // on the map once the step that brought it has played: the attacker is seen arriving.
        if next.is_none() && self.dialogs.is_empty() && matches!(self.screen, Screen::WorldMap | Screen::Building(_)) {
            if let Some(game) = self.game.as_mut().filter(|g| g.foe.is_some() && !(matches!(self.screen, Screen::WorldMap) && g.step_playing())) {
                next = Some(saves::battle(game));
            }
        }
        // A victory or defeat event ends the game once its window is read (not while a custom
        // battle, which has no game behind it, is on screen). The window of an event a step
        // brought is held until the step has played (`Game::tick_shown`), so it is waited for
        // too: the end comes after it, never before (events.md §11).
        let unshown = self.game.as_ref().is_some_and(Game::holds_events);
        if next.is_none() && self.dialogs.is_empty() && !unshown && !self.in_custom() {
            let end = self.game.as_ref().and_then(Game::script_end);
            match end {
                // A campaign map: the next map starts at once (0x4b5b64), as in the original;
                // Razdor's victory screen only when there is none (or in the editor's test play).
                Some(ScriptEnd::Victory(_)) if !matches!(self.screen, Screen::Victory) => {
                    let handed = (!self.test_play).then(|| screens::hand_over(&self.game, &self.scenarios, self.dt_content.clone())).flatten();
                    match handed {
                        Some(g) => {
                            self.map_view.reset();
                            self.map_view.open_around_hero(&g);
                            self.message = None;
                            self.game = Some(g);
                            next = Some(Screen::WorldMap);
                        }
                        None => next = Some(Screen::Victory),
                    }
                }
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
            if matches!(self.screen, Screen::WorldMap) && !matches!(next, Screen::WorldMap) {
                self.map_view.drop_grab();
            }
            // A window over the map closed: after a heal, a raise or a trade in the building
            // window the events are checked now (0x4b8d28(0) → 0x4b8f63).
            let side_window = matches!(
                self.screen,
                Screen::Building(_) | Screen::Squad { .. } | Screen::Journal(_) | Screen::Spellbook { .. } | Screen::Menu(_) | Screen::Settings | Screen::Save(_) | Screen::Load(_)
            );
            if let (true, true, Some(g)) = (matches!(next, Screen::WorldMap), side_window, self.game.as_mut()) {
                let events = g.window_closed();
                if !events.is_empty() {
                    let _ = world_view::handle_events(g, events, &mut self.message, &mut self.dialogs);
                }
            }
            // The army window opens: it recounts the hero's army (0x4d1814).
            if let (Screen::Squad { .. }, false, Some(g)) = (&next, matches!(self.screen, Screen::Squad { .. }), self.game.as_mut()) {
                g.army_window_opened();
            }
            if matches!(self.screen, Screen::ClassSelect { .. }) {
                self.message = None;
                // A map starts: the fog opens around the hero.
                if let (Screen::WorldMap, Some(g)) = (&next, self.game.as_ref()) {
                    self.map_view.open_around_hero(g);
                }
            }
            // A field that had the keyboard (the inventory filter) lets it go with its screen,
            // and the backpack's filter does not stay for the next game or the next visit.
            widgets::clear_focus();
            if matches!(self.screen, Screen::Squad { .. }) && !matches!(next, Screen::Squad { .. }) {
                items_view::clear_pack_filter();
            }
            self.screen = next;
        }
        self.play_log_frame();
        self.show_content();
    }
}
