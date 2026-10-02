//! The map editor window: toolbar, map canvas, tool palette, minimap, property panels and
//! dialogs. All editing goes through the pure model in `razdor::editor`; this module turns
//! mouse and keys into its calls and draws.
//!
//! Layout: toolbar on top, the tool palette with the minimap on the right, the selected
//! record's panel over the left of the map, a status line at the bottom.

mod canvas;
mod catalog;
mod events;
mod form;
mod newmap;
mod palette_panel;
mod props;
mod settings;

use std::path::PathBuf;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::dt::dtm::Scenario;
use razdor::dt::install::{self, MapEntry};
use razdor::editor::files::{self, Consent, Destination, SaveBlock};
use razdor::editor::mapcheck::{self, CheckRow};
use razdor::editor::mapfile::{OpenFormat, SaveFormat};
use razdor::editor::playability::{self, ScoreError};
use razdor::editor::menus;
use razdor::editor::naming::NamePools;
use razdor::editor::options::Session;
use razdor::editor::palette::{object_class_label, SURFACE_LABELS};
use razdor::editor::tools::Open;
use razdor::editor::validate::has_errors;
use razdor::editor::{Command, EditorDoc, Issue, Kit, Names, Origin, Page, Palette, Place, Press, SaveError, Severity, Target, ToolState};
use razdor::i18n::tr;
use razdor::rules::content::{Content, HeroClass};
use razdor::trf;

use crate::ui::assets::Assets;
use crate::ui::widgets::*;

use canvas::{Cam, Overlays, Overview};
use palette_panel::PaletteState;
use props::{Ctx, PanelState};
use events::{EventsAction, EventsState};
use settings::{SettingsAction, SettingsState};

const TOP: f32 = 44.0;
const RIGHT_W: f32 = 300.0;
const STATUS_H: f32 = 26.0;
const PANEL_W: f32 = 390.0;
const MINIMAP_H: f32 = 170.0;

/// What the editor asks the app to do.
pub enum EditorAction {
    None,
    /// Back to the title screen.
    Exit,
    /// Play the edited scenario with this content and hero class; come back afterwards.
    TestPlay { scenario: Box<Scenario>, content: Arc<Content>, class: HeroClass },
}

/// What a confirmation leads to.
#[derive(Clone)]
enum Then {
    /// The original's New: an empty map of the current size.
    Clear,
    /// The new-map dialog's map.
    Generated(Box<Generated>),
    Open(PathBuf),
    Exit,
    Save { name: String, format: SaveFormat, dest: Destination, consent: Consent },
    /// Delete an event that is still referred to (then back to the event window).
    DeleteEvent(u16),
}

/// A map the new-map dialog made, with the marks it left.
#[derive(Clone)]
struct Generated {
    scenario: Scenario,
    marks: Vec<i8>,
    status: String,
}

enum Modal {
    /// The new-map dialog (its state is [`EditorScreen::newmap`]).
    NewMap,
    Open { path: String, scroll: usize, format: OpenFormat },
    SaveAs { name: String, format: SaveFormat },
    Confirm { message: String, then: Then },
    Issues { scroll: usize },
    Settings,
    Events,
    TestPlay,
    /// The original's number dialog for a lantern's radius (records.md §2): a new lantern's
    /// (`placed`) or one clicked in Info mode; cancel keeps `initial`.
    Radius { id: u16, value: i64, initial: i64, placed: bool },
    /// The buildings or armies submenu (main-window.md §18).
    Records { buildings: bool, scroll: usize },
    /// The original's options window.
    Options(razdor::editor::options::Options),
    /// The unit editor and the artefact editor.
    Units,
    Artefacts,
}

pub struct EditorScreen {
    doc: EditorDoc,
    tools: ToolState,
    pal: PaletteState,
    cam: Option<Cam>,
    overview: Overview,
    overlays: Overlays,
    panel: PanelState,
    settings: SettingsState,
    events: EventsState,
    modal: Option<Modal>,
    status: Option<String>,
    issues: Vec<Issue>,
    /// The rows of the original editor's map check (they never block saving).
    check_rows: Vec<CheckRow>,
    palette: Palette,
    /// Names from the install, as the game reads them (the file check); `None` without one.
    install_names: Option<Names>,
    /// Names of the content test play uses (the install's, else the demo's).
    play_names: Names,
    /// Names of the session's tables (the pickers): the install's, else the demo's, as the
    /// unit and artefact editors leave them. The checks keep the game's: an artefact deleted
    /// in the session is still the game's, and the map still names it.
    session_names: Names,
    play_content: Arc<Content>,
    user_dir: Option<PathBuf>,
    game_dir: Option<PathBuf>,
    /// The left button went down on the map (a press, drags and a release follow).
    pressing: bool,
    last_cell: Option<(i32, i32)>,
    panning: Option<Vec2>,
    /// The building name lists of the install's editor ini.
    name_pools: Option<Arc<NamePools>>,
    /// The original's `[Option]` keys of its own: the last map, the building-place check.
    session: Session,
    /// When a held arrow key moves the view one more cell.
    arrow_repeat: f64,
    /// The original editor's options (records.md §11).
    options: razdor::editor::options::Options,
    /// The session's unit and artefact tables, as the unit and artefact editors leave them:
    /// every cost the editor shows reads them (test play keeps the install's).
    catalog: Arc<Content>,
    catalog_state: catalog::CatalogState,
    /// The new-map dialog while it is open.
    newmap: Option<newmap::NewMapState>,
}

fn ctrl() -> bool {
    is_key_down(KeyCode::LeftControl) || is_key_down(KeyCode::RightControl)
}

fn shift() -> bool {
    is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift)
}

impl EditorScreen {
    /// A new 50×50 map. `dt_content` is the install's content (names and test play), `demo`
    /// the built-in content test play falls back to.
    pub fn new(assets: &Assets, dt_content: Option<Arc<Content>>, demo: Arc<Content>) -> EditorScreen {
        let art = assets.dt.as_ref();
        let palette = art.map(|a| Palette::from_sprites(a.objects())).filter(|p| !p.buildings.is_empty()).unwrap_or_else(Palette::fallback);
        let game_dir = art.and_then(|a| install::find_path(&a.install.dir, install::MAPS_DIR).ok());
        let play_content = dt_content.clone().unwrap_or(demo);
        let options = razdor::editor::options::Options::load(razdor::editor::options::editor_dir().as_deref(), art.map(|a| a.install.dir.as_path()));
        let session = Session::load(razdor::editor::options::editor_dir().as_deref());
        let mut tools = ToolState::with_check(session.find_building_place);
        tools.choose_page(Page::Terrain, &palette);
        let mut doc = EditorDoc::new_map(razdor::editor::NewMap::default());
        // The original seeds its generator from the clock at start-up.
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        doc.rng = razdor::rules::rng::Rng::new(seed);
        EditorScreen {
            doc,
            tools,
            pal: PaletteState::default(),
            cam: None,
            overview: Overview::default(),
            overlays: Overlays::default(),
            panel: PanelState::default(),
            settings: SettingsState::default(),
            events: EventsState::with_options(&options),
            modal: None,
            status: Some(tr("New 50 x 50 map. Maps are saved to your own folder; see Save as.").into()),
            issues: Vec::new(),
            check_rows: Vec::new(),
            palette,
            install_names: dt_content.as_deref().map(Names::from_content),
            play_names: Names::from_content(&play_content),
            session_names: Names::from_content(&play_content),
            play_content: play_content.clone(),
            user_dir: files::user_maps_dir(),
            game_dir,
            pressing: false,
            last_cell: None,
            panning: None,
            name_pools: art.and_then(|a| NamePools::load(&a.install.dir)).map(Arc::new),
            session,
            arrow_repeat: 0.0,
            options,
            catalog: play_content.clone(),
            catalog_state: catalog::CatalogState::default(),
            newmap: None,
        }
    }

    fn names(&self) -> &Names {
        &self.session_names
    }

    fn install_maps(&self, assets: &Assets) -> Vec<MapEntry> {
        assets.dt.as_ref().map(|a| a.install.maps.clone()).unwrap_or_default()
    }

    fn view_rect() -> Rect {
        Rect::new(0.0, TOP, screen_width() - RIGHT_W, screen_height() - TOP - STATUS_H)
    }

    fn set_doc(&mut self, mut doc: EditorDoc) {
        // One generator for the whole session, as the original's.
        doc.rng = self.doc.rng.clone();
        self.doc = doc;
        self.tools.held = None;
        self.tools.selected = None;
        self.tools.opened.clear();
        self.cam = None;
        self.issues.clear();
        self.check_rows.clear();
        self.events = EventsState::with_options(&self.options);
    }

    fn open_file(&mut self, path: PathBuf) {
        let palette = self.palette.from_install.then_some(&self.palette);
        let base_artefacts = self.play_names.artefacts.iter().map(|a| a.id as usize).max().unwrap_or(0);
        match EditorDoc::open_with(&path, self.game_dir.as_deref(), palette, base_artefacts) {
            Ok(d) => {
                let (Origin::Game(read) | Origin::File(read)) = &d.origin else { unreachable!("opened from a file") };
                let path = read.display();
                let mut status = if matches!(d.origin, Origin::Game(_)) {
                    trf!("Opened {path} (a game map: saving goes to your own folder).", path)
                } else {
                    trf!("Opened {path}.", path)
                };
                for note in &d.load_notes {
                    status.push(' ');
                    status.push_str(note);
                }
                self.status = Some(status);
                self.set_doc(d);
            }
            Err(e) => self.status = Some(trf!("Cannot open {path}: {e}", path = path.display(), e)),
        }
    }

    /// Runs a confirmed (or unneeded) follow-up.
    fn run(&mut self, then: Then) -> EditorAction {
        match then {
            Then::Clear => {
                let s = &self.doc.scenario;
                let (w, h, seed) = (s.width(), s.height(), self.doc.rng.state());
                self.set_doc(EditorDoc::cleared(w, h, seed));
                self.status = Some(trf!("New {w} x {h} map.", w, h));
            }
            Then::Generated(g) => {
                self.set_doc(EditorDoc::generated(g.scenario, &g.marks));
                self.status = Some(g.status);
            }
            Then::Open(p) => self.open_file(p),
            Then::Exit => {
                self.close();
                return EditorAction::Exit;
            }
            Then::Save { name, format, dest, consent } => self.save(&name, format, dest, consent),
            Then::DeleteEvent(id) => {
                let before = self.doc.scenario.events.len();
                self.apply(Command::DeleteEvent { id }, "");
                if self.doc.scenario.events.len() < before {
                    self.status = Some(trf!("Deleted event {id}; later events moved up one, and the lists that held it keep an empty slot, as in the original.", id));
                }
                self.modal = Some(Modal::Events);
            }
        }
        EditorAction::None
    }

    /// `then`, after asking about unsaved changes if there are any.
    fn guarded(&mut self, then: Then) -> EditorAction {
        if self.doc.dirty() {
            self.modal = Some(Modal::Confirm { message: tr("The map has unsaved changes. Discard them?").into(), then });
            EditorAction::None
        } else {
            self.run(then)
        }
    }

    /// Saves under `name` as `format` to `dest`, asking for every confirmation the rules need
    /// first.
    fn save(&mut self, name: &str, format: SaveFormat, dest: Destination, consent: Consent) {
        let current = self.doc.saved_path.clone();
        match files::plan_save_as(name, format, dest, self.user_dir.as_deref(), self.game_dir.as_deref(), current.as_deref(), consent) {
            Ok(path) => match self.doc.save_to(&path, self.install_names.as_ref(), Some(&self.palette)) {
                Ok(written) => self.status = Some(trf!("Saved {path}.", path = written.display())),
                Err(SaveError::Invalid(issues)) => {
                    self.issues = issues;
                    self.check_rows.clear();
                    self.modal = Some(Modal::Issues { scroll: 0 });
                    self.status = Some(tr("Not saved: the map has errors.").into());
                }
                Err(e) => self.status = Some(trf!("Not saved: {e}", e)),
            },
            Err(block) => {
                let mut next = consent;
                match &block {
                    SaveBlock::ConfirmGameFolder(_) => next.game_folder = true,
                    SaveBlock::ConfirmReplaceGameMap(_) => next.replace_game_map = true,
                    SaveBlock::ConfirmReplaceOwnMap(_) => next.replace_own_map = true,
                    SaveBlock::BadName(_) | SaveBlock::NoFolder => {
                        self.status = Some(trf!("Not saved: {block}.", block));
                        return;
                    }
                }
                let message = block.to_string();
                self.modal = Some(Modal::Confirm { message, then: Then::Save { name: name.to_string(), format, dest, consent: next } });
            }
        }
    }

    /// Ctrl+S: the file saved to before (the game folder asks again), else "save as".
    fn quick_save(&mut self) {
        match self.doc.saved_path.clone() {
            Some(p) => {
                let name = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
                let in_game = self.game_dir.as_deref().is_some_and(|g| files::is_inside(&p, g));
                if in_game {
                    self.save(&name, SaveFormat::Normal, Destination::GameFolder, Consent::default());
                } else if self.user_dir.as_deref().is_some_and(|u| files::is_inside(&p, u)) {
                    self.save(&name, SaveFormat::Normal, Destination::UserFolder, Consent::default());
                } else {
                    // A file opened from elsewhere: saved where it is.
                    match self.doc.save_to(&p, self.install_names.as_ref(), Some(&self.palette)) {
                        Ok(written) => self.status = Some(trf!("Saved {path}.", path = written.display())),
                        Err(SaveError::Invalid(issues)) => {
                            self.issues = issues;
                            self.check_rows.clear();
                            self.modal = Some(Modal::Issues { scroll: 0 });
                        }
                        Err(e) => self.status = Some(trf!("Not saved: {e}", e)),
                    }
                }
            }
            None => self.modal = Some(Modal::SaveAs { name: self.doc.suggested_name(), format: SaveFormat::Normal }),
        }
    }

    fn apply(&mut self, cmd: Command, key: &str) {
        let key = (!key.is_empty()).then_some(key);
        if let Err(e) = self.doc.apply_merging(cmd, key) {
            self.status = Some(trf!("Refused: {e}.", e));
        }
        self.tools.check_selection(&mut self.doc);
    }

    fn check(&mut self) {
        self.issues = self.doc.issues(self.install_names.as_ref(), Some(&self.palette));
        self.check_rows = mapcheck::check_map(&self.doc.scenario, self.names());
        self.modal = Some(Modal::Issues { scroll: 0 });
    }

    /// Saves the map to `ErrorSave.DTm` in its folder (the user's maps folder for a map of
    /// the game's folder or a new one), then lets the drawing failure end the program.
    fn emergency_exit(&mut self, failure: Box<dyn std::any::Any + Send>) -> ! {
        let own = self.doc.saved_path.as_deref().and_then(|p| p.parent()).filter(|d| !self.game_dir.as_deref().is_some_and(|g| files::is_inside(d, g)));
        if let Some(dir) = own.map(PathBuf::from).or_else(|| self.user_dir.clone()) {
            match self.doc.emergency_save(&dir, self.install_names.as_ref(), Some(&self.palette)) {
                Ok(p) => eprintln!("{}", trf!("The map could not be drawn; it was saved to {path}.", path = p.display())),
                Err(e) => eprintln!("{}", trf!("The map could not be drawn, and the emergency save failed: {e}", e)),
            }
        }
        std::panic::resume_unwind(failure)
    }

    /// The original's score button: the score and quest count go into the header, and a line
    /// into `MapData.Txt` in the user's maps folder (the original writes it next to itself).
    fn score(&mut self) {
        let names = self.names().clone();
        match self.doc.score_playability(&names) {
            Ok(r) => {
                let written = self.user_dir.as_deref().map(|d| playability::append_map_data(d, &r.line));
                self.status = Some(match written {
                    Some(Ok(path)) => trf!("Playability {score}, quests {quests}; the scoring is in {path}.", score = r.score, quests = r.quests, path = path.display()),
                    _ => trf!("Playability {score}, quests {quests}.", score = r.score, quests = r.quests),
                });
            }
            Err(ScoreError::TooNarrow) => self.status = Some(tr("No playability for a map narrower than 50 cells (the original stops with a division by zero).").into()),
            Err(ScoreError::OutOfRange) => self.status = Some(tr("No playability: the score does not fit its field (the original stops with a range error).").into()),
            Err(ScoreError::PointEvents) => {
                self.status = Some(tr("No playability: a point lists an empty event slot or more than five events (the original stops with a range error).").into())
            }
        }
    }

    /// Centres the view on what an issue is about and selects it.
    fn go_to(&mut self, place: Place) {
        let s = &self.doc.scenario;
        let (target, cell) = match place {
            Place::Building(id) => (Some(Target::Building(id)), s.building(id).map(|b| (b.x, b.y))),
            Place::Army(id) => (Some(Target::Army(id)), s.army(id).map(|a| (a.x, a.y))),
            Place::Point(id) => (Some(Target::Point(id)), s.points.get(id as usize - 1).map(|p| (p.x, p.y))),
            Place::Object(i) => (None, s.objects.get(i).map(|o| (o.x, o.y))),
            Place::Hero(k) => (None, s.header.heroes.get(k).map(|h| (h.x, h.y))),
            Place::Settings => {
                self.modal = Some(Modal::Settings);
                return;
            }
            Place::Event(id) => {
                self.events.select(id);
                self.modal = Some(Modal::Events);
                return;
            }
            Place::Map => (None, None),
        };
        if target.is_some() {
            self.tools.selected = target;
        }
        if let (Some((x, y)), Some(cam)) = (cell, self.cam.as_mut()) {
            cam.centre = vec2(x as f32 + 0.5, y as f32 + 0.5);
        }
        self.modal = None;
    }

    /// A debug snapshot's scene (`ui::snapshot`): `map` opened, then a window or a record
    /// selected.
    pub fn stage(&mut self, what: &str, map: Option<PathBuf>) -> Result<(), String> {
        if let Some(p) = map {
            self.open_file(p);
        }
        let id = |s: &str| s[1..].parse::<u16>().map_err(|e| e.to_string());
        match what {
            "" => {}
            "units" => self.modal = Some(Modal::Units),
            "artefacts" => self.modal = Some(Modal::Artefacts),
            "options" => self.modal = Some(Modal::Options(self.options)),
            "settings" => self.modal = Some(Modal::Settings),
            "events" => self.modal = Some(Modal::Events),
            "grid" => {
                self.overlays.grid = true;
                self.doc.cells.rebuild_marks(&self.doc.scenario);
            }
            "fog" => self.overlays.fog = true,
            "records" => self.modal = Some(Modal::Records { buildings: true, scroll: 0 }),
            // The new-map dialog; with a map type (0–7), a 200 × 200 run of it started.
            w if w.starts_with("newmap") => {
                let shares = newmap::load_shares();
                let mut st = newmap::NewMapState::new(shares);
                if let Some(kind) = w.strip_prefix("newmap").filter(|k| !k.is_empty()) {
                    st.options.kind = kind.parse::<u8>().map_err(|e| e.to_string())?.min(7);
                    st.options.size = 200;
                    let cells = razdor::editor::newmap::Cells::of_map(&self.doc.scenario, self.doc.cells.marks(), 200);
                    st.start(razdor::editor::newmap::Sprites::from_palette(&self.palette), cells);
                }
                self.newmap = Some(st);
                self.modal = Some(Modal::NewMap);
            }
            w if w.starts_with("page") => {
                let k = w[4..].parse::<usize>().map_err(|e| e.to_string())?;
                let page = palette_panel::PAGES.get(k).ok_or("no such page")?.0;
                self.tools.choose_page(page, &self.palette);
            }
            w if w.starts_with('a') => self.tools.selected = Some(Target::Army(id(w)? as u8)),
            w if w.starts_with('b') => self.tools.selected = Some(Target::Building(id(w)?)),
            w if w.starts_with('p') => self.tools.selected = Some(Target::Point(id(w)?)),
            w => return Err(format!("no editor window {w}")),
        }
        Ok(())
    }

    /// The unit or artefact editor stored its draft: the session's tables change, and so do
    /// the names the pickers offer.
    fn set_catalog(&mut self, c: Content) {
        self.session_names = Names::from_content(&c);
        self.catalog = Arc::new(c);
    }

    /// The export of the unit or artefact list into Razdor's editor folder.
    fn export_catalog(&mut self, units: bool) {
        use razdor::editor::catalog as cat;
        let Some(dir) = razdor::editor::options::editor_dir() else {
            self.status = Some(tr("Not exported: no data folder.").into());
            return;
        };
        let written = if units { cat::export_units(&dir, &self.catalog.units) } else { cat::export_artefacts(&dir, &self.catalog.items) };
        self.status = Some(match written {
            Ok([ini, texts]) => trf!("Exported to {ini} and {texts}.", ini = ini.display(), texts = texts.display()),
            Err(e) => trf!("Not exported: {e}", e),
        });
    }

    /// One frame of the event window (it is the open modal).
    fn events_window(&mut self) {
        let names = self.names().clone();
        self.modal = Some(Modal::Events);
        match events::window(&mut self.events, &self.doc.scenario, &names) {
            EventsAction::None => {}
            EventsAction::Apply(cmd, key) => {
                let key = (!key.is_empty()).then_some(key);
                match self.doc.apply_merging(cmd, key.as_deref()) {
                    Ok(a) => {
                        if let Some(id) = a.new_id {
                            self.events.select(id as u16);
                            self.status = Some(trf!("Event {id} made.", id));
                        }
                    }
                    Err(e) => self.status = Some(trf!("Refused: {e}.", e)),
                }
                self.tools.check_selection(&mut self.doc);
            }
            EventsAction::Delete(id) => {
                self.run(Then::DeleteEvent(id));
            }
            EventsAction::Status(m) => self.status = Some(m),
            EventsAction::Close => self.modal = None,
        }
    }

    fn start_test_play(&mut self, class: HeroClass) -> EditorAction {
        let issues = self.doc.issues(Some(&self.play_names), None);
        if has_errors(&issues) {
            self.issues = issues;
            self.check_rows.clear();
            self.modal = Some(Modal::Issues { scroll: 0 });
            self.status = Some(tr("Fix the errors before test play.").into());
            return EditorAction::None;
        }
        self.modal = None;
        EditorAction::TestPlay { scenario: Box::new(self.doc.scenario.clone()), content: self.play_content.clone(), class }
    }

    /// One frame of the editor.
    pub fn frame(&mut self, assets: &Assets) -> EditorAction {
        clear_background(Color::from_rgba(12, 12, 11, 255));
        fields_begin_frame();
        let art = assets.dt.as_ref();
        let view = Self::view_rect();
        let s = &self.doc.scenario;
        let cam = self.cam.get_or_insert_with(|| {
            let mut c = Cam::new(view, s.width(), s.height());
            c.zoom = 0.75;
            c
        });
        cam.view = view;
        let modal_open = self.modal.is_some();
        // A dialog or an open list takes all input.
        set_input_blocked(modal_open || popup_open());

        let mut action = EditorAction::None;
        if !modal_open && !typing() && !popup_open() {
            action = self.shortcuts();
        }
        let overview = self.overview.texture(&self.doc);
        let panel_rect = self.tools.selected.map(|_| Rect::new(8.0, TOP + 8.0, PANEL_W, view.h - 16.0));
        self.canvas_input(view, panel_rect);
        let cam = self.cam.expect("set above");
        // As the original's emergency save: a failure while drawing the map or the minimap
        // saves the map to ErrorSave.DTm and ends the program.
        let held = self.tools.held;
        let drawn = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| canvas::draw_map(&self.doc, art, &cam, overview.as_ref(), held)));
        if let Err(e) = drawn {
            self.emergency_exit(e);
        }
        let hover = (!modal_open && view.contains(Vec2::from(crate::ui::widgets::pointer()))).then(|| cam.cell_at(Vec2::from(crate::ui::widgets::pointer())));
        canvas::draw_overlays(&self.doc, &self.tools, &self.palette, art, &cam, hover, cam.origin(), &self.overlays);

        // Right column: minimap and tools.
        let rx = screen_width() - RIGHT_W;
        let mini = Rect::new(rx + 6.0, TOP + 6.0, RIGHT_W - 12.0, MINIMAP_H);
        let picked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| canvas::minimap(&self.doc, overview.as_ref(), &cam, mini)));
        let picked = picked.unwrap_or_else(|e| self.emergency_exit(e));
        if let Some(at) = picked {
            if let Some(c) = self.cam.as_mut() {
                c.centre = at;
            }
        }
        let tools_rect = Rect::new(rx, mini.bottom() + 6.0, RIGHT_W, screen_height() - mini.bottom() - 6.0 - STATUS_H);
        let panel = palette_panel::tool_panel(&mut self.pal, &mut self.tools, &self.palette, art, tools_rect);
        if panel.burn {
            self.burn();
        }

        // The selected record's panel.
        if let (Some(t), Some(pr)) = (self.tools.selected, panel_rect) {
            let ctx = Ctx { names: &self.session_names, palette: &self.palette, content: Some(&*self.catalog), shared: Some(self.catalog.clone()) };
            let s = &self.doc.scenario;
            let edit = match t {
                Target::Building(id) => props::building_panel(&mut self.panel, s, id, &ctx, pr),
                Target::Army(id) => props::army_panel(&mut self.panel, s, id, &ctx, pr),
                Target::Point(id) => props::point_panel(&mut self.panel, s, id, pr),
            };
            if small_button(pr.right() - 34.0, pr.y + 6.0, 26.0, 24.0, "x", true) {
                self.tools.selected = None;
            } else if let Some((cmd, key)) = edit {
                self.apply(cmd, &key);
            }
        }

        let bar = self.toolbar(assets);
        if !matches!(bar, EditorAction::None) {
            action = bar;
        }
        self.status_line(hover);
        // The Info mode's hint over the map (outside move mode).
        if let Some(cell) = hover.filter(|_| !modal_open && !popup_open() && !panel_rect.is_some_and(|p| p.contains(Vec2::from(crate::ui::widgets::pointer())))) {
            let lines = self.tools.hint(&self.doc, &self.session_names, cell);
            if !lines.is_empty() {
                tooltip(&lines.into_iter().map(|l| (l, INK)).collect::<Vec<_>>());
            }
        }

        // Dialogs on top.
        set_input_blocked(popup_open());
        if self.modal.is_some() {
            let m = self.modal_frame(assets);
            if !matches!(m, EditorAction::None) {
                action = m;
            }
        }
        set_input_blocked(false);
        draw_popup();
        fields_end_frame();
        action
    }

    /// "Burn everything" (main-window.md §15): no question, and the modified flag stays as it
    /// was.
    fn burn(&mut self) {
        let clean = !self.doc.dirty();
        let facts = self.palette.forest_facts();
        self.apply(Command::Burn { facts }, "");
        if clean {
            self.doc.mark_unmodified();
        }
        self.status = Some(tr("Everything burnt (the building names keep their words).").into());
    }

    /// F2 (main-window.md §17.1): the map saved straight to its folder under its current
    /// name, without a dialog even for a new map. Razdor's checks and confirmations still
    /// apply.
    fn f2_save(&mut self) {
        let path = self.doc.saved_path.clone();
        let name = path.as_ref().and_then(|p| p.file_stem()).map_or("New".to_string(), |s| s.to_string_lossy().into_owned());
        let in_game = path.as_deref().zip(self.game_dir.as_deref()).is_some_and(|(p, g)| files::is_inside(p, g));
        let outside = path.as_deref().is_some_and(|p| !in_game && !self.user_dir.as_deref().is_some_and(|u| files::is_inside(p, u)));
        if outside {
            self.quick_save();
        } else {
            let dest = if in_game { Destination::GameFolder } else { Destination::UserFolder };
            self.save(&name, SaveFormat::Normal, dest, Consent::default());
        }
    }

    /// Moves the view by whole cells (the original's arrow keys).
    fn step_view(&mut self, dx: f32, dy: f32) {
        let (w, h) = (self.doc.scenario.width(), self.doc.scenario.height());
        if let Some(cam) = self.cam.as_mut() {
            cam.centre += vec2(dx, dy);
            cam.clamp(w, h);
        }
    }

    fn shortcuts(&mut self) -> EditorAction {
        let c = ctrl();
        if c && is_key_pressed(KeyCode::Z) {
            if shift() {
                self.doc.redo();
            } else {
                self.doc.undo();
            }
            self.tools.check_selection(&mut self.doc);
        } else if c && is_key_pressed(KeyCode::Y) {
            self.doc.redo();
            self.tools.check_selection(&mut self.doc);
        } else if c && is_key_pressed(KeyCode::S) {
            if shift() {
                self.modal = Some(Modal::SaveAs { name: self.doc.suggested_name(), format: SaveFormat::Normal });
            } else {
                self.quick_save();
            }
        } else if c && is_key_pressed(KeyCode::O) {
            self.modal = Some(Modal::Open { path: String::new(), scroll: 0, format: OpenFormat::Normal });
        } else if c && is_key_pressed(KeyCode::N) {
            return self.guarded(Then::Clear);
        } else if c && is_key_pressed(KeyCode::Q) {
            return self.guarded(Then::Exit);
        } else if !c {
            if is_key_pressed(KeyCode::F2) {
                self.f2_save();
            }
            if is_key_pressed(KeyCode::Space) {
                self.tools.space();
            }
            if is_key_pressed(KeyCode::Delete) {
                self.tools.delete_selected(&mut self.doc);
            }
            if is_key_pressed(KeyCode::Escape) {
                self.tools.selected = None;
            }
            // The arrows move the view one cell (held down, again and again).
            let now = get_time();
            let mut d = Vec2::ZERO;
            for (k, v) in [(KeyCode::Left, vec2(-1.0, 0.0)), (KeyCode::Right, vec2(1.0, 0.0)), (KeyCode::Up, vec2(0.0, -1.0)), (KeyCode::Down, vec2(0.0, 1.0))] {
                if is_key_pressed(k) {
                    d += v;
                    self.arrow_repeat = now + 0.4;
                } else if is_key_down(k) && now >= self.arrow_repeat {
                    d += v;
                }
            }
            if d != Vec2::ZERO {
                if now >= self.arrow_repeat {
                    self.arrow_repeat = now + 0.05;
                }
                self.step_view(d.x, d.y);
            }
            let s = &self.doc.scenario;
            let (w, h) = (s.width(), s.height());
            if let Some(cam) = self.cam.as_mut() {
                if is_key_pressed(KeyCode::Equal) || is_key_pressed(KeyCode::KpAdd) {
                    cam.zoom_at(1.25, cam.view.center());
                }
                if is_key_pressed(KeyCode::Minus) || is_key_pressed(KeyCode::KpSubtract) {
                    cam.zoom_at(0.8, cam.view.center());
                }
                if is_key_pressed(KeyCode::Home) {
                    cam.fit(w, h);
                }
            }
        }
        EditorAction::None
    }

    /// Mouse on the map (main-window.md §6): the left button works the tool, the right one
    /// picks up in Info mode; Razdor's own: the right or middle button drags the view where
    /// nothing is picked up, the wheel zooms.
    fn canvas_input(&mut self, view: Rect, panel: Option<Rect>) {
        let m = Vec2::from(crate::ui::widgets::pointer());
        let over = view.contains(m) && !panel.is_some_and(|p| p.contains(m)) && !input_blocked();
        let (w, h) = (self.doc.scenario.width(), self.doc.scenario.height());
        let Some(cam) = self.cam.as_mut() else { return };
        if over {
            let wh = mouse_wheel().1;
            if wh != 0.0 {
                cam.zoom_at(if wh > 0.0 { 1.12 } else { 1.0 / 1.12 }, m);
            }
        }
        let cam = *cam;
        let cell = cam.cell_at(m);
        let modifiers = shift() || ctrl() || is_key_down(KeyCode::LeftAlt) || is_key_down(KeyCode::RightAlt);
        let kit = Kit { palette: &self.palette, names: self.name_pools.as_ref() };
        let press = |right| Press { cell, view_origin: cam.origin(), right, modifiers };
        // The right button: a pick-up when there is one, else the view drags.
        if over && is_mouse_button_pressed(MouseButton::Right) {
            let before = self.tools.held;
            self.tools.press(&mut self.doc, kit, press(true));
            if self.tools.held == before {
                self.panning = Some(m);
            }
        } else if over && is_mouse_button_pressed(MouseButton::Middle) {
            self.panning = Some(m);
        }
        if is_mouse_button_down(MouseButton::Right) || is_mouse_button_down(MouseButton::Middle) {
            if let (Some(last), Some(cam)) = (self.panning, self.cam.as_mut()) {
                cam.pan(m - last);
                cam.clamp(w, h);
                self.panning = Some(m);
            }
        } else {
            self.panning = None;
        }
        if over && is_mouse_button_pressed(MouseButton::Left) {
            clear_focus();
            self.pressing = true;
            self.last_cell = Some(cell);
            self.tools.press(&mut self.doc, kit, press(false));
            self.after_tool();
        } else if self.pressing {
            if is_mouse_button_down(MouseButton::Left) {
                if self.last_cell != Some(cell) {
                    self.last_cell = Some(cell);
                    self.tools.drag_to(&mut self.doc, kit, cell);
                }
            } else {
                self.pressing = false;
                self.tools.release(&mut self.doc, cell);
                self.after_tool();
            }
        }
    }

    /// What a press asked to open: a record's panel, the lantern dialog, the scenario window.
    fn after_tool(&mut self) {
        if let Some(m) = self.tools.message.take() {
            self.status = Some(trf!("Refused: {m}.", m));
        }
        for open in std::mem::take(&mut self.tools.opened) {
            match open {
                Open::Building(id) => self.select(Target::Building(id)),
                Open::Army(id) => self.select(Target::Army(id)),
                Open::Point(id) | Open::Target(id) => self.select(Target::Point(id)),
                Open::LanternRadius { id, placed } => {
                    if let Some(p) = self.doc.scenario.points.get(id as usize - 1) {
                        let r = p.radius as i64;
                        self.modal = Some(Modal::Radius { id, value: r, initial: r, placed });
                    }
                }
                Open::HeroSettings(k) => {
                    self.settings.tab = k + 1;
                    self.modal = Some(Modal::Settings);
                }
            }
        }
    }

    /// Opens a record's panel at its first tab.
    fn select(&mut self, t: Target) {
        self.tools.selected = Some(t);
        self.panel.tab = 0;
    }

    /// The submenus' choice (0x5af960, 0x5afadc): the view centred on the record (a building
    /// on its footprint) and its editor opened.
    fn centre_on(&mut self, t: Target) {
        let s = &self.doc.scenario;
        let at = match t {
            Target::Building(id) => s.building(id).map(|b| vec2(b.x as f32 + 1.0 - b.size_x as f32 / 2.0, b.y as f32 + 1.0 - b.size_y as f32 / 2.0)),
            Target::Army(id) => s.army(id).map(|a| vec2(a.x as f32 + 0.5, a.y as f32 + 0.5)),
            Target::Point(id) => s.points.get(id as usize - 1).map(|p| vec2(p.x as f32 + 0.5, p.y as f32 + 0.5)),
        };
        if let (Some(at), Some(cam)) = (at, self.cam.as_mut()) {
            cam.centre = at;
        }
        self.select(t);
    }

    /// The map of the last session (`WorkMap`), as the original opens it at start-up; a new
    /// map when it cannot be opened.
    pub fn reopen_last(&mut self) {
        if let Some(p) = self.session.work_map.clone() {
            if p.is_file() {
                self.open_file(p);
            }
        }
    }

    /// Closing (main-window.md §1.3): the last map's file and the building-place check go to
    /// Razdor's editor settings.
    pub fn close(&mut self) {
        let file = self.doc.saved_path.clone().or(match &self.doc.origin {
            Origin::Game(p) | Origin::File(p) => Some(p.clone()),
            Origin::New => None,
        });
        if file.is_some() {
            self.session.work_map = file;
        }
        self.session.find_building_place = self.tools.building_check;
        if let Some(dir) = razdor::editor::options::editor_dir() {
            let _ = self.session.save(&dir);
        }
    }

    fn toolbar(&mut self, _assets: &Assets) -> EditorAction {
        let w = screen_width();
        draw_rectangle(0.0, 0.0, w, TOP, Color::new(0.14, 0.12, 0.1, 1.0));
        draw_line(0.0, TOP, w, TOP, 1.0, DIM);
        let (cu, cr) = (self.doc.can_undo(), self.doc.can_redo());
        let dirty = self.doc.dirty();
        let has_b = !self.doc.scenario.buildings.is_empty();
        let has_a = !self.doc.scenario.armies.is_empty();
        // (label, enabled, lit: a toggle's state).
        let buttons: Vec<(&str, bool, Option<bool>)> = vec![
            (tr("New"), true, None),
            (tr("Generate"), true, None),
            (tr("Open"), true, None),
            // As the original's, enabled only while the map is modified.
            (tr("Save"), dirty, None),
            (tr("Save as"), true, None),
            (tr("Save to game folder"), self.game_dir.is_some(), None),
            (tr("Undo"), cu, None),
            (tr("Redo"), cr, None),
            (tr("Settings"), true, None),
            (tr("Events"), true, None),
            (tr("Buildings"), has_b, None),
            (tr("Armies"), has_a, None),
            (tr("Check"), true, None),
            (tr("Playability"), true, None),
            (tr("Editor options"), true, None),
            (tr("Units"), true, None),
            (tr("Artefacts"), true, None),
            (tr("Grid"), true, Some(self.overlays.grid)),
            (tr("Patrol zones"), true, Some(self.overlays.patrols)),
            (tr("Fog"), true, Some(self.overlays.fog)),
            (tr("Test play"), true, None),
            (tr("Exit"), true, None),
        ];
        // Each button as wide as its label; all narrower (smaller labels) if the row would
        // not fit the window.
        let natural: Vec<f32> = buttons.iter().map(|(l, _, _)| measure(l, 17.0).width + 16.0).collect();
        let room = w - 12.0 - 4.0 * (buttons.len() - 1) as f32;
        let k = (room / natural.iter().sum::<f32>()).min(1.0);
        let mut x = 6.0;
        let mut hit = None;
        for (i, ((label, enabled, lit), nw)) in buttons.iter().zip(&natural).enumerate() {
            let bw = nw * k;
            let pressed = match lit {
                Some(on) => toggle_button(x, 8.0, bw, 28.0, label, *on),
                None => small_button(x, 8.0, bw, 28.0, label, *enabled),
            };
            if pressed {
                hit = Some(i);
            }
            // Undo / Redo: what they would undo or redo.
            let what = match i {
                6 => self.doc.undo_label(),
                7 => self.doc.redo_label(),
                _ => None,
            };
            if let Some(what) = what.filter(|_| mouse_in(x, 8.0, bw, 28.0)) {
                tooltip(&[(tr(what).to_string(), INK)]);
            }
            x += bw + 4.0;
        }
        let mut action = EditorAction::None;
        match hit {
            Some(0) => action = self.guarded(Then::Clear),
            Some(1) => {
                let shares = newmap::load_shares();
                self.newmap = Some(newmap::NewMapState::new(shares));
                self.modal = Some(Modal::NewMap);
            }
            Some(2) => self.modal = Some(Modal::Open { path: String::new(), scroll: 0, format: OpenFormat::Normal }),
            Some(3) => self.quick_save(),
            Some(4) => self.modal = Some(Modal::SaveAs { name: self.doc.suggested_name(), format: SaveFormat::Normal }),
            Some(5) => {
                let name = self.doc.suggested_name();
                self.save(&name, SaveFormat::Normal, Destination::GameFolder, Consent::default());
            }
            Some(6) => {
                self.doc.undo();
                self.tools.check_selection(&mut self.doc);
            }
            Some(7) => {
                self.doc.redo();
                self.tools.check_selection(&mut self.doc);
            }
            Some(8) => {
                self.settings.tab = 0;
                self.modal = Some(Modal::Settings);
            }
            Some(9) => self.modal = Some(Modal::Events),
            Some(10) => self.modal = Some(Modal::Records { buildings: true, scroll: 0 }),
            Some(11) => self.modal = Some(Modal::Records { buildings: false, scroll: 0 }),
            Some(12) => self.check(),
            Some(13) => self.score(),
            Some(14) => self.modal = Some(Modal::Options(self.options)),
            Some(15) => self.modal = Some(Modal::Units),
            Some(16) => self.modal = Some(Modal::Artefacts),
            Some(17) => {
                self.overlays.grid = !self.overlays.grid;
                // Switching the grid on builds the marks again (0x5a37a8).
                if self.overlays.grid {
                    self.doc.cells.rebuild_marks(&self.doc.scenario);
                }
            }
            Some(18) => self.overlays.patrols = !self.overlays.patrols,
            Some(19) => self.overlays.fog = !self.overlays.fog,
            Some(20) => self.modal = Some(Modal::TestPlay),
            Some(21) => action = self.guarded(Then::Exit),
            _ => {}
        }
        let s = &self.doc.scenario;
        let title = format!("{}{}  ({} x {})", if self.doc.dirty() { "* " } else { "" }, self.doc.suggested_name(), s.width(), s.height());
        let tw = measure(&title, 18.0).width;
        if x + tw + 12.0 < w {
            text(&title, w - tw - 10.0, 28.0, 18.0, INK);
        }
        action
    }

    fn status_line(&self, hover: Option<(i32, i32)>) {
        let (w, h) = (screen_width(), screen_height());
        let y = h - STATUS_H;
        draw_rectangle(0.0, y, w, STATUS_H, Color::new(0.08, 0.08, 0.075, 1.0));
        let s = &self.doc.scenario;
        let mut parts = Vec::new();
        if let Some((x, cy)) = hover.filter(|(x, y)| *x >= 0 && *y >= 0 && (*x as u32) < s.width() && (*y as u32) < s.height()) {
            let code = s.terrain_at(x as u32, cy as u32).unwrap_or(0);
            parts.push(format!("({x}, {cy}) {}", tr(SURFACE_LABELS[code as usize & 15])));
            let objs: Vec<String> = self.doc.objects_at(x as u16, cy as u16).map(|o| format!("{} {}", object_class_label(o.class), o.sprite)).collect();
            if !objs.is_empty() {
                parts.push(objs.join(", "));
            }
        }
        parts.push(trf!("buildings {b}, armies {a}, points {p}, events {e}", b = s.buildings.len(), a = s.armies.len(), p = s.points.len(), e = s.events.len()));
        let line = parts.join("   |   ");
        text(&line, 8.0, y + 18.0, 16.0, DIM);
        // The quest count and score of the last scoring, coloured by the original's bands.
        let score = s.header.playability();
        let band = [
            Color::new(0.9, 0.25, 0.2, 1.0),
            Color::new(0.95, 0.55, 0.15, 1.0),
            Color::new(0.65, 0.65, 0.2, 1.0),
            Color::new(0.45, 0.75, 0.3, 1.0),
            Color::new(0.25, 0.8, 0.45, 1.0),
            Color::new(0.35, 0.55, 0.95, 1.0),
            Color::new(0.65, 0.4, 0.9, 1.0),
            Color::new(0.9, 0.35, 0.8, 1.0),
        ][playability::band(score)];
        let label = trf!("quests {q}, playability {p}", q = s.header.quest_count(), p = score);
        // At the foot of the tool column, clear of the messages.
        text_fit(&label, w - RIGHT_W + 8.0, y - 8.0, RIGHT_W - 16.0, 16.0, band);
        // The message right of the counts, ending at the window's edge, shrunk to the room left.
        if let Some(m) = &self.status {
            let from = 8.0 + measure(&line, 16.0).width + 24.0;
            let room = (w - 10.0 - from).max(0.0);
            let tw = measure(m, 16.0).width.min(room);
            text_fit(m, w - 10.0 - tw, y + 18.0, room, 16.0, ACCENT);
            // Shortened: the whole message on hover.
            let (mx, my) = crate::ui::widgets::pointer();
            if measure(m, 16.0).width > room && my >= y && mx >= w - 10.0 - tw {
                tooltip(&wrap(m, 560.0, 17.0).into_iter().map(|l| (l, INK)).collect::<Vec<_>>());
            }
        }
    }

    /// The new-map dialog in `r`; `Some` when it leads somewhere (its map, after asking about
    /// unsaved changes).
    fn newmap_window(&mut self, r: Rect) -> Option<EditorAction> {
        let mut st = self.newmap.take()?;
        let palette = &self.palette;
        let (doc, cells) = (&self.doc.scenario, &self.doc.cells);
        let a = newmap::window(&mut st, r, || razdor::editor::newmap::Sprites::from_palette(palette), |size| razdor::editor::newmap::Cells::of_map(doc, cells.marks(), size));
        match a {
            newmap::NewMapAction::None => {}
            newmap::NewMapAction::Ran(rng) => self.doc.rng = rng,
            newmap::NewMapAction::Close(rng) => {
                if let Some(rng) = rng {
                    self.doc.rng = rng;
                }
                self.status = Some(tr("Cancelled.").into());
                return None;
            }
            newmap::NewMapAction::Accept(out, shares) => {
                // The exit button writes the widths (0x528438).
                let saved = match razdor::editor::options::editor_dir().map(|d| shares.save(&d)) {
                    Some(Err(e)) => format!(" {}", trf!("The share widths were not saved: {e}", e)),
                    _ => String::new(),
                };
                let scenario = razdor::editor::newmap::new_scenario(&out, &self.doc.scenario, tr("New scenario"));
                let mut status = trf!("New {w} x {h} map from seed {seed}.", w = out.cells.w, h = out.cells.h, seed = out.seed);
                if let Some(stop) = out.stop {
                    status.push(' ');
                    status.push_str(&newmap::stop_text(stop));
                }
                status.push_str(&saved);
                let g = Generated { scenario, marks: out.cells.mark.clone(), status };
                self.modal = None;
                return Some(self.guarded(Then::Generated(Box::new(g))));
            }
        }
        self.newmap = Some(st);
        None
    }

    fn modal_frame(&mut self, assets: &Assets) -> EditorAction {
        let (sw, sh) = (screen_width(), screen_height());
        let mut action = EditorAction::None;
        let Some(modal) = self.modal.take() else { return action };
        if matches!(modal, Modal::Settings) {
            let names = self.names().clone();
            match settings::window(&mut self.settings, &self.doc.scenario, &names) {
                SettingsAction::Apply(cmd, key) => {
                    self.apply(cmd, &key);
                    self.modal = Some(Modal::Settings);
                }
                SettingsAction::PickStart(k) => {
                    // The original places a start from the items palette.
                    self.tools.choose_page(Page::Items, &self.palette);
                    self.tools.click_palette(k, &self.palette);
                    self.status = Some(tr("Click the hero's start cell on the map.").into());
                }
                SettingsAction::Close => {}
                SettingsAction::Status(m) => {
                    self.status = Some(m);
                    self.modal = Some(Modal::Settings);
                }
                SettingsAction::None => self.modal = Some(Modal::Settings),
            }
            return action;
        }
        if matches!(modal, Modal::Events) {
            self.events_window();
            return action;
        }
        if matches!(modal, Modal::Units | Modal::Artefacts) {
            let names = self.names().clone();
            let a = if matches!(modal, Modal::Units) {
                catalog::units_window(&mut self.catalog_state, &self.catalog, &names)
            } else {
                catalog::artefacts_window(&mut self.catalog_state, &self.catalog)
            };
            let keep = !matches!(a, catalog::CatalogAction::Close(_));
            match a {
                catalog::CatalogAction::None => {}
                catalog::CatalogAction::Store(c) | catalog::CatalogAction::Close(Some(c)) => self.set_catalog(*c),
                catalog::CatalogAction::Close(None) => {}
                catalog::CatalogAction::Export(c) => {
                    self.set_catalog(*c);
                    self.export_catalog(matches!(modal, Modal::Units));
                }
                catalog::CatalogAction::Status(m) => self.status = Some(m),
            }
            if keep {
                self.modal = Some(modal);
            }
            return action;
        }
        draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.55));
        let (w, h) = match &modal {
            Modal::Confirm { .. } => (560.0, 200.0),
            Modal::Radius { .. } => (420.0, 190.0),
            Modal::Records { .. } => (560.0f32.min(sw - 40.0), (sh - 100.0).max(300.0)),
            Modal::Options(_) => (520.0, 300.0),
            Modal::TestPlay => (560.0, 230.0),
            Modal::SaveAs { .. } => (620.0, 300.0),
            Modal::NewMap => (900.0f32.min(sw - 20.0), 680.0f32.min(sh - 20.0)),
            _ => (720.0f32.min(sw - 40.0), (sh - 100.0).max(300.0)),
        };
        let r = Rect::new((sw - w) / 2.0, ((sh - h) / 2.0).max(10.0), w, h);
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 0.98));
        draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
        let esc = !typing() && is_key_pressed(KeyCode::Escape);
        let (x, mut y) = (r.x + 20.0, r.y + 32.0);
        let cancel = |r: &Rect| button(r.right() - 140.0, r.bottom() - 54.0, 120.0, 40.0, tr("Cancel"), true);
        let mut keep = true;
        let mut next = modal;
        match &mut next {
            Modal::Confirm { message, then } => {
                text(tr("Please confirm"), x, y, 22.0, ACCENT);
                for (i, line) in wrap(message, r.w - 40.0, 18.0).iter().take(4).enumerate() {
                    text(line, x, y + 32.0 + i as f32 * 22.0, 18.0, INK);
                }
                if button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, tr("Yes"), true) {
                    let then = then.clone();
                    self.modal = None;
                    return self.run(then);
                }
                if cancel(&r) || esc {
                    keep = false;
                    self.status = Some(tr("Cancelled.").into());
                }
            }
            Modal::TestPlay => {
                text(tr("Test play: choose the hero"), x, y, 22.0, ACCENT);
                let note = if self.install_names.is_some() { tr("With your install's units and rules.") } else { tr("Without an install: the built-in demo's units.") };
                text_fit(note, x, y + 28.0, r.w - 40.0, 16.0, DIM);
                text_fit(tr("Esc > Main menu in the game comes back here."), x, y + 48.0, r.w - 40.0, 16.0, DIM);
                y += 70.0;
                for (k, class) in HeroClass::ALL.into_iter().enumerate() {
                    if button(x + k as f32 * 170.0, y, 160.0, 44.0, tr(razdor::editor::palette::HERO_CLASSES[k]), true) {
                        self.modal = None;
                        return self.start_test_play(class);
                    }
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::SaveAs { name, format } => {
                text(tr("Save as"), x, y, 22.0, ACCENT);
                let folder = self.user_dir.as_ref().map_or(tr("(no data folder)").to_string(), |d| d.display().to_string());
                text_fit(&trf!("Into your maps folder: {folder}", folder), x, y + 26.0, r.w - 40.0, 15.0, DIM);
                text_fit(tr("Map name:"), x, y + 60.0, 116.0, 17.0, INK);
                text_field("saveas:name", x + 120.0, y + 44.0, r.w - 160.0, 26.0, name, false);
                // The original's four file types.
                let bw = (r.w - 40.0 - 3.0 * 6.0) / 4.0;
                for (k, f) in SaveFormat::ALL.into_iter().enumerate() {
                    if toggle_button(x + k as f32 * (bw + 6.0), y + 82.0, bw, 28.0, f.label(), *format == f) {
                        *format = f;
                    }
                }
                for (i, line) in wrap(tr("Use \"Save to game folder\" in the toolbar to put it where the game finds it."), r.w - 40.0, 15.0).iter().enumerate() {
                    text(line, x, y + 136.0 + i as f32 * 18.0, 15.0, DIM);
                }
                let go = button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, tr("Save"), true) || (is_key_pressed(KeyCode::Enter) && !popup_open());
                if go {
                    let (n, f) = (name.clone(), *format);
                    self.modal = None;
                    self.save(&n, f, Destination::UserFolder, Consent::default());
                    return action;
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::NewMap => {
                if let Some(a) = self.newmap_window(r) {
                    return a;
                }
                keep = self.newmap.is_some();
            }
            Modal::Open { path, scroll, format } => {
                text(tr("Open a map"), x, y, 22.0, ACCENT);
                // The original's three file types: a demo map lists the .DTs files.
                let bw = (r.w - 40.0 - 2.0 * 6.0) / 3.0;
                for (k, f) in OpenFormat::ALL.into_iter().enumerate() {
                    if toggle_button(x + k as f32 * (bw + 6.0), y + 10.0, bw, 28.0, f.label(), *format == f) {
                        *format = f;
                    }
                }
                y += 50.0;
                let mut entries: Vec<(String, PathBuf)> = Vec::new();
                if *format != OpenFormat::Demo {
                    for m in self.install_maps(assets) {
                        entries.push((trf!("Game: {name}", name = m.name), m.path));
                    }
                }
                if let Some(d) = &self.user_dir {
                    let ext = if *format == OpenFormat::Demo { "DTs" } else { "DTm" };
                    for m in files::list_dir_with(d, ext) {
                        entries.push((trf!("Yours: {name}", name = m.name), m.path));
                    }
                }
                let list = Rect::new(x, y, r.w - 40.0, r.h - 224.0);
                let rows = (list.h / 26.0).floor() as usize;
                if mouse_in(list.x, list.y, list.w, list.h) {
                    let wh = mouse_wheel().1;
                    if wh > 0.0 {
                        *scroll = scroll.saturating_sub(2);
                    } else if wh < 0.0 {
                        *scroll += 2;
                    }
                }
                *scroll = (*scroll).min(entries.len().saturating_sub(rows));
                if entries.is_empty() {
                    for (i, line) in wrap(tr("No maps found. Set RAZDOR_DT_DIR for the game's maps, or type a path below."), list.w, 16.0).iter().enumerate() {
                        text(line, x, y + 20.0 + i as f32 * 20.0, 16.0, DIM);
                    }
                }
                let mut pick = None;
                for (i, (label, p)) in entries.iter().enumerate().skip(*scroll).take(rows) {
                    let ry = y + (i - *scroll) as f32 * 26.0;
                    if small_button(list.x, ry, list.w, 24.0, label, true) {
                        pick = Some(p.clone());
                    }
                }
                let fy = r.bottom() - 110.0;
                text_fit(tr("Or a file:"), x, fy + 18.0, 86.0, 17.0, INK);
                text_field("open:path", x + 90.0, fy, r.w - 130.0, 26.0, path, false);
                if button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, tr("Open file"), !path.trim().is_empty()) {
                    pick = Some(PathBuf::from(path.trim()));
                }
                if let Some(p) = pick {
                    let p = format.request(&p);
                    self.modal = None;
                    return self.guarded(Then::Open(p));
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::Issues { scroll } => {
                let errors = self.issues.iter().filter(|i| i.severity == Severity::Error).count();
                let warnings = self.issues.len() - errors;
                let head = if self.issues.is_empty() && self.check_rows.is_empty() {
                    tr("No problems found.").to_string()
                } else {
                    trf!("Errors: {errors}, warnings: {warnings}. Errors block saving. Click one to go there.", errors, warnings)
                };
                text(tr("Map check"), x, y, 22.0, ACCENT);
                text_fit(&head, x, y + 26.0, r.w - 40.0, 16.0, if errors > 0 { RED } else { INK });
                y += 40.0;
                if !self.check_rows.is_empty() {
                    let n = self.check_rows.len();
                    text_fit(&trf!("The original editor's check: {n} remarks; they do not block saving.", n), x, y + 2.0, r.w - 40.0, 15.0, DIM);
                    y += 22.0;
                }
                // The original's rows (kind, id, name, message), then Razdor's file checks.
                let mut lines: Vec<(String, Color, Place)> = self
                    .check_rows
                    .iter()
                    .map(|c| (format!("{} {}  {}: {}", c.kind.label(), c.id, c.name, c.message), Color::new(0.95, 0.85, 0.55, 1.0), c.place()))
                    .collect();
                let error_ink = Color::new(1.0, 0.5, 0.45, 1.0);
                lines.extend(self.issues.iter().map(|i| (i.to_string(), if i.severity == Severity::Error { error_ink } else { INK }, i.place)));
                let list = Rect::new(x, y, r.w - 40.0, r.bottom() - 74.0 - y);
                let rows = (list.h / 24.0).floor() as usize;
                if mouse_in(list.x, list.y, list.w, list.h) {
                    let wh = mouse_wheel().1;
                    if wh > 0.0 {
                        *scroll = scroll.saturating_sub(2);
                    } else if wh < 0.0 {
                        *scroll += 2;
                    }
                }
                *scroll = (*scroll).min(lines.len().saturating_sub(rows));
                let mut go = None;
                for (i, (line, ink, place)) in lines.iter().enumerate().skip(*scroll).take(rows) {
                    let ry = y + (i - *scroll) as f32 * 24.0;
                    let mut line = line.clone();
                    while measure(&line, 15.0).width > list.w - 10.0 && !line.is_empty() {
                        line.pop();
                    }
                    let hover = mouse_in(list.x, ry, list.w, 22.0);
                    if hover {
                        draw_rectangle(list.x, ry, list.w, 22.0, Color::new(0.3, 0.25, 0.15, 1.0));
                    }
                    text(&line, list.x + 4.0, ry + 16.0, 15.0, *ink);
                    if hover && clicked() {
                        go = Some(*place);
                    }
                }
                if let Some(p) = go {
                    self.go_to(p);
                    return action;
                }
                if button(r.right() - 140.0, r.bottom() - 54.0, 120.0, 40.0, tr("Close"), true) || esc {
                    keep = false;
                }
            }
            Modal::Radius { id, value, initial, placed } => {
                text(tr("Lantern radius"), x, y, 22.0, ACCENT);
                text_fit(tr("Radius (0-24)"), x, y + 40.0, 160.0, 17.0, INK);
                if let Some(v) = number_field("radius:value", x + 170.0, y + 22.0, 150.0, *value, 0, razdor::editor::records::LANTERN_MAX as i64) {
                    *value = v;
                }
                let ok = button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, tr("OK"), true) || (is_key_pressed(KeyCode::Enter) && !popup_open());
                // Cancel gives the dialog's starting value; either way the area is revealed
                // with the radius (an existing lantern's old area removed first).
                let cancelled = cancel(&r) || esc;
                if ok || cancelled {
                    let radius = if ok { *value } else { *initial } as u8;
                    self.apply(Command::LanternRadius { id: *id, radius, placed: *placed }, "");
                    keep = false;
                }
            }
            Modal::Records { buildings, scroll } => {
                text(if *buildings { tr("Buildings") } else { tr("Armies") }, x, y, 22.0, ACCENT);
                y += 16.0;
                let s = &self.doc.scenario;
                // (caption, target): group headers have no target; a separator is a blank row.
                let mut rows: Vec<(String, Option<Target>)> = Vec::new();
                let group_label = |b: bool, key: u8| -> String {
                    if b {
                        razdor::editor::palette::building_type_label(key).to_string()
                    } else if key == menus::ARMY_UNDEAD {
                        tr("Undead").to_string()
                    } else {
                        tr(razdor::editor::palette::BEHAVIOURS[key as usize]).to_string()
                    }
                };
                if *buildings {
                    for (k, sec) in menus::building_menu(s).into_iter().enumerate() {
                        if k > 0 {
                            rows.push((String::new(), None));
                        }
                        for g in sec {
                            rows.push((group_label(true, g.key), None));
                            rows.extend(g.items.into_iter().map(|(id, name)| (format!("    #{id} {name}"), Some(Target::Building(id)))));
                        }
                    }
                } else {
                    let undead = &self.session_names.undead;
                    for (k, sec) in menus::army_menu(s, |u| undead.contains(&(u as u32))).into_iter().enumerate() {
                        if k > 0 {
                            rows.push((String::new(), None));
                        }
                        for g in sec {
                            rows.push((group_label(false, g.key), None));
                            rows.extend(g.items.into_iter().map(|(id, name)| (format!("    #{id} {name}"), Some(Target::Army(id)))));
                        }
                    }
                }
                let list = Rect::new(x, y + 10.0, r.w - 40.0, r.bottom() - 74.0 - y - 10.0);
                let fit = (list.h / 24.0).floor() as usize;
                if mouse_in(list.x, list.y, list.w, list.h) {
                    let wh = mouse_wheel().1;
                    if wh > 0.0 {
                        *scroll = scroll.saturating_sub(2);
                    } else if wh < 0.0 {
                        *scroll += 2;
                    }
                }
                *scroll = (*scroll).min(rows.len().saturating_sub(fit));
                let mut go = None;
                for (i, (label, t)) in rows.iter().enumerate().skip(*scroll).take(fit) {
                    let ry = list.y + (i - *scroll) as f32 * 24.0;
                    match t {
                        Some(t) => {
                            let hover = mouse_in(list.x, ry, list.w, 22.0);
                            if hover {
                                draw_rectangle(list.x, ry, list.w, 22.0, Color::new(0.3, 0.25, 0.15, 1.0));
                            }
                            text_fit(label, list.x + 4.0, ry + 16.0, list.w - 8.0, 15.0, INK);
                            if hover && clicked() {
                                go = Some(*t);
                            }
                        }
                        None => text_fit(label, list.x + 4.0, ry + 16.0, list.w - 8.0, 16.0, ACCENT),
                    }
                }
                if let Some(t) = go {
                    self.modal = None;
                    self.centre_on(t);
                    return action;
                }
                if button(r.right() - 140.0, r.bottom() - 54.0, 120.0, 40.0, tr("Close"), true) || esc {
                    keep = false;
                }
            }
            Modal::Options(o) => {
                use razdor::editor::options::{self, SIZES};
                text(tr("Editor options"), x, y, 22.0, ACCENT);
                text_fit(tr("Text size of the event window's message and question"), x, y + 30.0, r.w - 40.0, 16.0, INK);
                for (k, size) in SIZES.iter().enumerate() {
                    if toggle_button(x + k as f32 * 90.0, y + 44.0, 84.0, 28.0, &size.to_string(), o.text_size == *size) {
                        o.text_size = *size;
                    }
                }
                if let Some(on) = checkbox(x, y + 84.0, r.w - 40.0, tr("Bold text"), o.bold) {
                    o.bold = on;
                }
                if let Some(on) = checkbox(x, y + 114.0, r.w - 40.0, tr("New events can happen many times"), o.new_events_repeat) {
                    o.new_events_repeat = on;
                }
                if button(r.right() - 270.0, r.bottom() - 54.0, 120.0, 40.0, tr("OK"), true) {
                    self.options = *o;
                    self.events.set_options(o);
                    self.status = Some(match options::editor_dir().map(|d| o.save(&d)) {
                        Some(Ok(path)) => trf!("Options saved to {path}.", path = path.display()),
                        Some(Err(e)) => trf!("Options not saved: {e}", e),
                        None => tr("Options kept for this session (no data folder).").into(),
                    });
                    keep = false;
                }
                if cancel(&r) || esc {
                    keep = false;
                }
            }
            Modal::Settings | Modal::Events | Modal::Units | Modal::Artefacts => unreachable!("handled above"),
        }
        if keep {
            self.modal = Some(next);
        }
        action = EditorAction::None;
        action
    }
}
