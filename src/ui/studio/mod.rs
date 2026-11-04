//! Razdor Studio's screen: the docks of a package folder. The file tree, the script view, the
//! problems and the output (step 1); the Map tab with the game's own map renderer, the Scene and
//! the Inspector (step 2); the linked selection, the undo and redo, the stale banner, the outside
//! changes, the drag to move and the save (step 3); and in step 4 the Inspector forms, the placing
//! tools, the terrain brush, the live problems on the canvas, the reach overlay and the grid and
//! footprints switches. egui over the game's own window (`egui-macroquad`), docked with
//! `egui_dock`. The model is `razdor::studio`.
//!
//! Timing. A change asks for a rebuild of the package after 300 ms without another change (the
//! debounce); a run of typing in the Script is one undo step after the same pause. The rebuild
//! runs on the studio's build thread (`razdor::studio::worker`): the window keeps its last good
//! state until the result comes, and a result older than the one applied is dropped. Outside
//! changes settle for 250 ms before they are read.
//!
//! Desktop only: the whole module is compiled out of Android and iOS (see `ui/mod.rs`).

mod aiview;
mod battletest;
mod canvas;
mod data;
mod outline;
mod palette;
mod claude;
mod forms;
mod inspector;
mod inspector_kit;
mod mcp;
mod map_tab;
mod selection;
mod scores;
mod newmap;
mod records;
mod scene;
mod story;
mod story_forms;
mod vars;
pub(crate) mod scenarios;
pub mod start;
mod watch;
mod commands;
mod settings;
mod theme;
mod workspaces;

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange};
use egui_macroquad::egui;
use egui::{Align2, Button, Color32, ScrollArea, Sense, TextEdit, Ui, WidgetText};
use egui_dock::{DockArea, DockState, Style, TabViewer};
use egui_macroquad::egui::{Align, Id, Layout, Margin, RichText, Stroke};
use egui_phosphor::regular as ic;
use serde::{Deserialize, Serialize};
use razdor::dt::install::DtInstall;
use razdor::i18n::{n_, tr};
use razdor::pack::diag::{Diagnostic, Severity};
use razdor::pack::load::{line_of, Package};
use razdor::pack::open::lang;
use razdor::studio::build::Job;
use razdor::studio::nav;
use razdor::studio::play;
use razdor::studio::story_graph as sg;
use story::StorySel;
use razdor::studio::mcp::server::{runtime_file, Server};
use razdor::studio::mcp::{Frame, SnapTarget};
use razdor::studio::map::{choose_map, map_of_file, marks, selection_of, targets, MapView, Selection};
use razdor::studio::worker::Worker;
use razdor::studio::{is_text, unified, Edit, Installed, Kind, Locator, Reconcile, Workspace};
use razdor::trf;

use crate::ui::audio::StudioPrefs;
use commands::{Clip, Scope};
use workspaces::{default_dock, Drawer, WsKind};

use crate::ui::dt_art::DtArt;
use map_tab::{draw as draw_map_tab, switch_map, Canvas, Tool};
use scenarios::{Focus, ScenarioUi};
use mcp::{Request, UiHost};

/// The example package `--studio` opens when no folder is named.
pub const DEFAULT_PACKAGE: &str = "examples/packs/hello-story";

const ERROR_RED: Color32 = Color32::from_rgb(0xe0, 0x5a, 0x5a);
const WARNING_AMBER: Color32 = Color32::from_rgb(0xe8, 0xb0, 0x4f);

/// Quiet time after a change before the package is rebuilt, and after a keystroke before its
/// undo step is recorded.
const DEBOUNCE: Duration = Duration::from_millis(300);
/// Quiet time after an outside change before the file is read.
const SETTLE: Duration = Duration::from_millis(250);
/// The longest a snapshot waits for its build (a build is a fraction of a second).
const SNAPSHOT_WAIT: Duration = Duration::from_secs(60);

/// What the studio asks of the game after a frame.
pub enum Action {
    None,
    /// Back to the main menu.
    Exit,
    /// The game's battle view of one battle of the tester (watch mode), shown in place of the
    /// studio; the studio stays open behind it.
    Watch(Box<crate::ui::battle_view::BattleView>),
}

/// One dock tab. Godot 4's docks, after the studio's design. Saved layouts name a tab by its variant
/// (serde) and its `key`: neither is translated, so a change of language keeps every layout.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tab {
    FileSystem,
    Scene,
    Map,
    Script,
    Inspector,
    Problems,
    Output,
    /// The Claude dock: the author's own `claude` in a terminal (`ui::studio::claude`).
    Claude,
    /// The whole map small, with the view frame (`map_tab::Viewer::minimap`).
    Minimap,
    /// The package's saved regions (`map_tab::Viewer::stamps_dock`).
    Stamps,
    /// The Map palette: terrain swatches and object thumbnails (`map_tab::Viewer::palette_dock`).
    Palette,
    /// The Map layers: eyes, locks and the overlay switches (`map_tab::Viewer::layers_dock`).
    Layers,
    /// The campaign graph (the Campaign main screen).
    Campaign,
    /// The texts table: key by language (the Texts main screen).
    Texts,
    /// The named characters: where each is given, required and removed.
    Characters,
    /// The flow graph of the shown map: dialogs, triggers, variables (the Story main screen).
    Story,
    /// Every variable: where it is set and read, rename (`vars.rs`).
    Vars,
    /// The package's units: the list, the new unit, the form in the Inspector (`records.rs`).
    Units,
    /// The package's items: the same as the units, with the artefact price (`records.rs`).
    Items,
    /// The battle tester: two sides, N battles on the game's engine (`battletest.rs`).
    Battle,
    /// The Тест workspace's ИИ view: the AI run on a read-only map, the day slider and the log (`aiview.rs`, `Viewer::ai_tab`).
    AiTest,
    /// The two scores: Richness and Health, each with its breakdown (`scores.rs`).
    Scores,
    /// The Сюжет outline (UX5, `outline.rs`): the triggers and dialogs by map, the characters, the variables.
    Outline,
    /// The Данные navigation (UX5, `data.rs`): the five views, and the file tree on Файлы.
    DataNav,
    /// The Данные centre (UX5, `data.rs`): the view the navigation picked.
    Data,
}

impl Tab {
    /// The tab's fixed key: its dock id, which is not its title (the titles are translated).
    pub fn key(self) -> &'static str {
        match self {
            Tab::FileSystem => "file-system",
            Tab::Scene => "scene",
            Tab::Map => "map",
            Tab::Script => "script",
            Tab::Inspector => "inspector",
            Tab::Problems => "problems",
            Tab::Output => "output",
            Tab::Claude => "claude",
            Tab::Minimap => "minimap",
            Tab::Stamps => "stamps",
            Tab::Palette => "palette",
            Tab::Layers => "layers",
            Tab::Campaign => "campaign",
            Tab::Texts => "texts",
            Tab::Characters => "characters",
            Tab::Story => "story",
            Tab::Vars => "vars",
            Tab::Units => "units",
            Tab::Items => "items",
            Tab::Battle => "battle",
            Tab::AiTest => "ai-test",
            Tab::Scores => "scores",
            Tab::Outline => "outline",
            Tab::DataNav => "data-nav",
            Tab::Data => "data-view",
        }
    }

    fn title(self) -> &'static str {
        match self {
            Tab::FileSystem => tr("File system"),
            Tab::Scene => tr("Scene"),
            Tab::Map => tr("Map"),
            Tab::Script => tr("Script"),
            Tab::Inspector => tr("Inspector"),
            Tab::Problems => tr("Problems"),
            Tab::Output => tr("Output"),
            Tab::Claude => "Claude",
            Tab::Minimap => tr("Minimap"),
            Tab::Stamps => tr("Stamps"),
            Tab::Palette => tr("Palette"),
            Tab::Layers => tr("Layers"),
            Tab::Campaign => tr("Campaign"),
            Tab::Texts => tr("Texts"),
            Tab::Characters => tr("Characters"),
            Tab::Story => tr("Story"),
            Tab::Vars => tr("Variables"),
            Tab::Units => tr("Units"),
            Tab::Items => tr("Items"),
            Tab::Battle => tr("Battle"),
            Tab::AiTest => tr("AI"),
            Tab::Scores => tr("Scores"),
            Tab::Outline => tr("Outline"),
            Tab::DataNav => tr("Data"),
            Tab::Data => tr("Data"),
        }
    }
}

/// Where the Script goes next: its caret to a line (1-based), or a table's bytes selected.
#[derive(Clone, Debug)]
pub struct Jump {
    pub line: u32,
    pub range: Option<Range<usize>>,
}

/// A text file that changed on disk while it has unsaved changes: the question, and the diff
/// (its lines) when it is shown.
struct Conflict {
    path: String,
    disk: String,
    diff: Option<Vec<String>>,
}

/// The studio's state for one open package folder.
pub struct StudioScreen {
    ws: Workspace,
    /// The four workspaces' docks, in `WsKind::ALL` order, and the shown one (`ws_kind`).
    docks: [DockState<Tab>; 4],
    ws_kind: WsKind,
    /// The layouts are made (the first frame, at the window's width): before that the docks are nominal.
    laid_out: bool,
    width: f32,
    /// Each workspace's default layout as JSON (the layout is saved only when it differs from it).
    layout_default: [String; 4],
    /// The snapshot options: `side` (the Script beside the map), and that a snapshot runs (its layouts
    /// are the defaults and are not saved).
    side: bool,
    snapshot: bool,
    /// The bottom drawer: open or closed, its tab, and the error count seen (a new error opens it).
    drawer: Drawer,
    drawer_open: bool,
    errors_seen: usize,
    /// The snapshot's `scale=` (not saved).
    scale_override: Option<u32>,
    /// The Settings window (scale, shortcuts).
    settings: settings::Window,
    /// Index into `ws.docs` of the document the Script tab shows and Ctrl+S saves.
    selected: Option<usize>,
    /// The output log: one line per action and its result.
    log: Vec<String>,
    /// The package's name: its folder's name.
    name: String,
    /// The map the Map tab shows and its last good compile.
    maps: MapView,
    /// The diagnostics of the last failed compile (listed in Problems after the checks').
    map_problems: Vec<Diagnostic>,
    /// Whether the game's install is there (the map is drawn only then), as of the last frame.
    installed: bool,
    /// The game's install, shared with the build thread (made at the first frame that has one).
    install: Option<Arc<DtInstall>>,
    /// The build thread, and the generations: the last asked, the last applied.
    worker: Option<Worker>,
    gen: u64,
    applied: u64,
    /// A rebuild is due (a change settled, a map was shown, a file was saved).
    want_build: bool,
    /// The snapshot's build waits for its result before the first frame is drawn.
    wait_build: bool,
    /// The snapshot's scores wait for their result after the build (`scores`, `scores-generated`).
    wait_scores: bool,
    /// The scores thread and the last scores (`scores.rs`).
    scores: scores::State,
    /// The copy of the package a generator snapshot works on (removed with the screen).
    snapshot_copy: Option<razdor::studio::gen::apply::TempCopy>,
    /// The Check button was pressed: its counts are logged when the build comes back.
    check_asked: bool,
    /// The canvas's marks are to be made again (the text or the diagnostics changed).
    marks_dirty: bool,
    /// The file last opened: it picks the map when it is a map's file.
    opened: Option<String>,
    /// What the Map, Scene and Inspector docks have selected.
    selection: Option<Selection>,
    /// The Map tab's view: the camera, the tools, the overlays.
    canvas: Canvas,
    /// The Script's next jump (set by the Scene, the Inspector and a problem; taken when drawn).
    jump: Option<Jump>,
    /// When the last change asked for a rebuild.
    debounce: Option<Instant>,
    /// When the last keystroke or visual edit happened (a text burst ends after a pause).
    burst: Option<Instant>,
    /// The Script's caret as last seen: its file and character index. Moving it selects the table.
    cursor_seen: Option<(String, usize)>,
    /// The watcher of the folder (`None` when it could not start; the reason is in the log).
    watch: Option<watch::Watch>,
    /// Outside changes seen and not read yet: the path, and when it was seen.
    disk_pending: BTreeMap<String, Instant>,
    /// A dirty file changed on disk: the question.
    conflict: Option<Conflict>,
    /// Leaving with unsaved changes: the prompt is shown.
    exit_prompt: bool,
    /// Package › New map (generator): its window and the worker it runs.
    newmap: newmap::NewMap,
    /// The Claude dock (its program runs only when the author starts it).
    claude: claude::ClaudeDock,
    /// The MCP server (`None` when it did not start: the reason is in the log), kept for its drop (it
    /// stops and removes its runtime file), its requests, and the pictures asked for, answered after
    /// the frame is drawn.
    _mcp: Option<Server>,
    mcp_rx: Receiver<Request>,
    mcp_snaps: Vec<(SnapTarget, Sender<Result<Frame, String>>)>,
    /// The scenario screens' state (the Inspector's focus, the campaign selection, the texts).
    scen: ScenarioUi,
    /// A main screen the menu asked for; shown after the dock is drawn.
    pending_tab: Option<Tab>,
    /// The battle tester (`battletest.rs`), and the request its screen made this frame.
    battle: battletest::BattleTest,
    battle_req: Option<battletest::Req>,
    /// Snapshot options whose picture needs the install (`battletest`, `aiview`), run once it is there.
    snap_todo: Vec<&'static str>,
    /// The `inspector=army-keys` snapshot: the Inspector shows the file keys under the labels.
    snap_keys: bool,
    /// The `inspector-find=<text>` snapshot: the field search's text, set on the first frame.
    snap_find: Option<String>,
    /// The `battlewatch` snapshot: the tester's first battle opens in the battle view.
    snap_watch: bool,
    /// Tab hides the side panels (the tool strip, the docks but the Map tab): the canvas takes the width.
    side_hidden: bool,
    /// The snapshot's `ws=` workspace, kept for every frame of the snapshot (a jump to the Script would
    /// otherwise move the shown workspace to the Данные one).
    snap_ws: Option<WsKind>,
    /// The command palette (UX6, `palette.rs`): Ctrl+Shift+P, Ctrl+P and the top bar's field.
    palette: palette::Palette,
    /// The game started by «Играть» (F5), while it runs; `play_shown` is the snapshot's stand-in for it.
    play: Option<std::process::Child>,
    play_shown: bool,
    /// Ctrl+F in the Inspector was asked for (by the palette too): the field search takes the focus this frame.
    find_asked: bool,
    /// The snapshot's `jump=score`: the Оценка view clicks its first target once (`scores.rs`).
    snap_jump: bool,
}

impl StudioScreen {
    /// Opens a package folder. Its first rebuild is due at the first frame, so the Problems tab
    /// fills in a moment.
    pub fn open(path: &Path) -> Result<StudioScreen, String> {
        let ws = Workspace::open(path)?;
        let name = ws.root.file_name().map_or_else(|| ws.root.display().to_string(), |n| n.to_string_lossy().into_owned());
        let mut log = Vec::new();
        let (host, mcp_rx) = UiHost::new();
        let mcp = match runtime_file() {
            Some(file) => match Server::start(Arc::new(host), file) {
                Ok(server) => Some(server),
                Err(e) => {
                    log.push(trf!("The MCP server is not started: {reason}", reason = e));
                    None
                }
            },
            None => {
                log.push(tr("The MCP server is not started: no folder for its runtime file").to_string());
                None
            }
        };
        let watch = match watch::Watch::start(&ws.root) {
            Ok(w) => Some(w),
            Err(e) => {
                log.push(trf!("Outside changes are not seen: {reason}", reason = e));
                None
            }
        };
        Ok(StudioScreen {
            ws,
            docks: WsKind::ALL.map(|k| default_dock(k, workspaces::NOMINAL_W, false)),
            ws_kind: WsKind::Map,
            laid_out: false,
            width: workspaces::NOMINAL_W,
            layout_default: Default::default(),
            side: false,
            snapshot: false,
            drawer: Drawer::Problems,
            drawer_open: false,
            errors_seen: 0,
            scale_override: None,
            settings: settings::Window::default(),
            selected: None,
            log,
            name,
            maps: MapView::default(),
            map_problems: Vec::new(),
            installed: false,
            install: None,
            worker: Some(Worker::start()),
            gen: 0,
            applied: 0,
            want_build: true,
            wait_build: false,
            wait_scores: false,
            scores: scores::State::start(),
            snapshot_copy: None,
            check_asked: false,
            marks_dirty: true,
            opened: None,
            selection: None,
            canvas: Canvas::default(),
            jump: None,
            debounce: None,
            burst: None,
            cursor_seen: None,
            watch,
            disk_pending: BTreeMap::new(),
            conflict: None,
            exit_prompt: false,
            newmap: newmap::NewMap::new(),
            claude: claude::ClaudeDock::new(),
            _mcp: mcp,
            mcp_rx,
            mcp_snaps: Vec::new(),
            scen: ScenarioUi::default(),
            pending_tab: None,
            battle: battletest::BattleTest::default(),
            battle_req: None,
            snap_todo: Vec::new(),
            snap_keys: false,
            snap_find: None,
            snap_watch: false,
            side_hidden: false,
            snap_ws: None,
            palette: palette::Palette::default(),
            play: None,
            play_shown: false,
            find_asked: false,
            snap_jump: false,
        })
    }

    /// Shows map `map` and selects `army` on it, for the snapshot's `studio:<folder>:<map>:<army>`.
    /// `opts` are the snapshot's options (listed in `ui::snapshot`): `side`, `stale`, `conflict`,
    /// `overlap`, `reach`, `grid`, `footprints`, `tool=<name>`, `building=<id>`. None keeps the
    /// Script as it is.
    pub fn preselect(&mut self, map: Option<&str>, army: Option<&str>, opts: &[&str]) -> Result<(), String> {
        self.snapshot = true;
        // `scores-generated` adds a map, so it runs on a copy of the package, removed with the screen
        // (`generated` is copied by the scene, `ui::snapshot`), and the package the scene names is never written.
        if opts.contains(&"scores-generated") {
            let copy = razdor::studio::gen::apply::copy_package(&self.ws.root)?;
            self.ws = Workspace::open(copy.path())?;
            self.snapshot_copy = Some(copy);
        }
        if let Some(id) = map {
            // The package is built in the background, so the map is looked for in the folder.
            if !self.ws.root.join("maps").join(id).join("map.toml").is_file() {
                return Err(format!("no map {id} in the package"));
            }
            switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, id);
        }
        if let Some(id) = army {
            self.selection = Some(Selection::Army(id.to_string()));
        }
        // The first frames wait for the build of what is shown (the snapshot's picture is of it).
        self.wait_build = true;
        if opts.is_empty() {
            return Ok(());
        }
        let Some(id) = self.maps.map.clone() else { return Ok(()) };
        let path = format!("maps/{id}/map.toml");
        self.open_file(&path);
        // `inspector=<what>` picks the Inspector's form: the same options as the forms' own (`package`,
        // `header`, `campaign`, `trigger`, `dialog`, `units`, `items`), or a selection of its own.
        let expanded: Vec<&str> = opts.iter().map(|o| o.strip_prefix("inspector=").map_or(*o, |what| match what {
            "unit" => "units",
            "item" => "items",
            other => other,
        })).collect();
        for opt in &expanded {
            match *opt {
                "army" => {}
                "army-keys" => self.snap_keys = true,
                other if other.starts_with("inspector-find=") => self.snap_find = Some(other["inspector-find=".len()..].to_string()),
                "building" | "point" | "object" | "hero" => {
                    let map = id.clone();
                    let spec = inspector::spec_now(&self.ws, None, &map).ok_or("the map file could not be read")?;
                    self.selection = Some(match *opt {
                        "building" => Selection::Building(spec.building.first().map(|b| b.get_ref().id.clone()).ok_or("no building on the map")?),
                        "point" => Selection::Point(if spec.point.is_empty() { return Err("no lantern on the map".into()) } else { 0 }),
                        "object" => Selection::Object(if spec.object.is_empty() { return Err("no object on the map".into()) } else { 0 }),
                        _ => Selection::Hero,
                    });
                }
                "campaign-edge" => {
                    self.scen.focus = Focus::Campaign;
                    self.scen.story_view = outline::StoryView::Campaign;
                    self.pending_tab = Some(Tab::Story);
                    let text = self.ws.text_of(razdor::studio::campaign_graph::FILE).ok();
                    if let Ok((pkg, _)) = self.ws.read_package() {
                        let graph = razdor::studio::campaign_graph::build(&pkg, text.as_deref(), lang());
                        if let Some(e) = graph.edges.first() {
                            self.scen.camp = scenarios::CampSel::Edge(e.from.clone(), e.index);
                        }
                    }
                }
                "newmap" => self.newmap.open = true,
                "battletest" | "battlewatch" => {
                    self.snap_watch |= *opt == "battlewatch";
                    self.snap_todo.push("battletest");
                }
                "aiview" => self.snap_todo.push("aiview"),
                // `test=battle|ai|scores`: the Тест workspace's view; `palette[=<text>]` and `goto[=<text>]` open the
                // command palette (Ctrl+Shift+P) and the Ctrl+P list, typed with the text; `play` shows the game
                // running (the status bar), without starting it.
                other if other.starts_with("test=") => {
                    self.pending_tab = Some(match &other["test=".len()..] {
                        "battle" => Tab::Battle,
                        "ai" => Tab::AiTest,
                        "scores" => Tab::Scores,
                        _ => return Err(format!("{other}: battle, ai or scores")),
                    });
                }
                // `jump=score`: the first score line with a target is clicked (the jump to Карта or Файлы, as a click does).
                "jump=score" => self.snap_jump = true,
                "play" => self.play_shown = true,
                other if other == "palette" || other.starts_with("palette=") => {
                    self.palette.request(palette::Mode::Commands, other.strip_prefix("palette=").unwrap_or(""));
                }
                other if other == "goto" || other.starts_with("goto=") => {
                    self.palette.request(palette::Mode::Go, other.strip_prefix("goto=").unwrap_or(""));
                }
                "generated" => self.snapshot_generated()?,
                "scores" => self.snap_scores(),
                "scores-generated" => {
                    self.snapshot_generated()?;
                    self.snap_scores();
                }
                "side" => self.side = true,
                "drawer" => self.drawer_open = true,
                "keys" => self.settings.show_keys(),
                "settings" => self.settings.open = true,
                "keys-conflict" => self.settings.show_conflict(&StudioPrefs::default()),
                "stale" => self.snapshot_stale()?,
                "conflict" => self.snapshot_conflict(&path)?,
                "overlap" => self.snapshot_overlap(&id)?,
                "reach" => self.canvas.reach = true,
                "grid" => self.canvas.grid = true,
                "footprints" => self.canvas.footprints = true,
                "replace" => self.canvas.replace_trees = true,
                "erase" => self.canvas.erase = true,
                "move" => self.canvas.move_mode = true,
                "warn-mountains" => self.canvas.ignore_mountains = false,
                "passable" => self.canvas.passable = true,
                "patrols" => self.canvas.patrols = true,
                "fog" => self.canvas.fog = true,
                "minimap" => {
                    self.pending_tab = Some(Tab::Minimap);
                }
                "stamps" => {
                    self.pending_tab = Some(Tab::Stamps);
                }
                "burn" => {
                    let map = self.maps.map.clone().ok_or("no map shown")?;
                    let file = format!("maps/{map}/map.toml");
                    let text = self.ws.text_of(&file)?;
                    let edits = razdor::studio::brush::burn_tree_edits(&file, &text);
                    let i = self.ws.open_terrain(&map)?;
                    let w = self.ws.terrains[i].terrain.width;
                    let cells = razdor::studio::brush::burn_cells(&self.ws.terrains[i].terrain.cells, w);
                    razdor::studio::brush::commit(&mut self.ws, "burn everything", &map, edits, cells)?;
                    self.reload();
                }
                "copy" => {
                    let set = selection::set_of(&self.canvas, &self.maps);
                    selection::copy(&mut self.ws, &self.maps, &mut self.canvas, &set, &mut self.log);
                }
                // `plant=<x>,<y>` and `hill=<x>,<y>`: the forests and hills tools' stroke at that brush centre
                // (the brush size, the family and `replace` as set), in memory.
                other if other.starts_with("plant=") || other.starts_with("hill=") => {
                    let (kind, at) = other.split_once('=').ok_or("bad option")?;
                    let (x, y) = at.split_once(',').ok_or_else(|| format!("{other}: x,y"))?;
                    let (x, y): (i64, i64) = (x.parse().map_err(|_| format!("{other}: x"))?, y.parse().map_err(|_| format!("{other}: y"))?);
                    let map = self.maps.map.clone().ok_or("no map shown")?;
                    let file = format!("maps/{map}/map.toml");
                    let text = self.ws.text_of(&file)?;
                    let size = self.canvas.brush;
                    // As the Map tab does: no object on water (the package's rule), read from the terrain picture
                    // (the map may not be compiled yet when the snapshot's options run).
                    let i = self.ws.open_terrain(&map)?;
                    let t = &self.ws.terrains[i];
                    let water = |c: (i64, i64)| c.0 >= 0 && c.1 >= 0 && t.code_at(c.0 as u32, c.1 as u32).is_some_and(|k| k <= 2);
                    let cells: Vec<(i64, i64)> = razdor::studio::brush::square(x, y, size).into_iter().filter(|&c| !water(c)).collect();
                    let edits = if kind == "hill" {
                        razdor::studio::brush::hill_edits(&file, &text, razdor::studio::brush::hill_centre(x, y, size), size)
                    } else {
                        razdor::studio::brush::forest_edits(&file, &text, &cells, self.canvas.forest, self.canvas.replace_trees, size)
                    };
                    razdor::studio::brush::commit(&mut self.ws, "snapshot stroke", &map, edits, Vec::new())?;
                    self.reload();
                }
                "paste" => selection::arm_paste(&mut self.canvas, &mut self.log),
                "package" => self.scen.focus = Focus::Package,
                "header" => self.scen.focus = Focus::Header,
                "campaign" => {
                    self.scen.focus = Focus::Campaign;
                    self.scen.story_view = outline::StoryView::Campaign;
                    self.pending_tab = Some(Tab::Story);
                }
                "story" | "vars" => {
                    // The story screens show their own selection: the army the scene named is not theirs.
                    if *opt == "story" {
                        self.selection = None;
                        self.scen.focus = Focus::Story;
                    }
                    if *opt == "vars" {
                        self.scen.outline_tab = outline::OutlineTab::Vars;
                    }
                    self.pending_tab = Some(if *opt == "story" { Tab::Story } else { Tab::Outline });
                }
                // `outline=story|characters|vars`: the Сюжет outline's tab; `story-view=campaign` the centre's switch.
                other if other.starts_with("outline=") => {
                    self.scen.outline_tab = match &other["outline=".len()..] {
                        "story" => outline::OutlineTab::Story,
                        "characters" => outline::OutlineTab::Characters,
                        "vars" => outline::OutlineTab::Vars,
                        _ => return Err(format!("{other}: story, characters or vars")),
                    };
                    self.pending_tab = Some(Tab::Outline);
                }
                // `data=texts|package|files`: the Данные view (`units`, `items` are the spreadsheets).
                other if other.starts_with("data=") => {
                    self.scen.data_view = match &other["data=".len()..] {
                        "texts" => data::DataView::Texts,
                        "package" => data::DataView::Package,
                        "files" => data::DataView::Files,
                        _ => return Err(format!("{other}: texts, package or files")),
                    };
                    self.pending_tab = Some(Tab::Data);
                }
                // `sort=price`: the spreadsheets sorted by the suggested price, highest first.
                "sort=price" => {
                    self.scen.sheet.sort = [Some((8, true)), Some((4, true))];
                }
                // `file=<path>` opens the file in Данные › Файлы, and `at=<line>` (after it) puts the caret at the line.
                other if other.starts_with("file=") => {
                    self.open_file(&other["file=".len()..]);
                    self.scen.data_view = data::DataView::Files;
                    self.pending_tab = Some(Tab::Script);
                }
                other if other.starts_with("at=") => {
                    let line: u32 = other["at=".len()..].parse().map_err(|_| format!("{other}: the line"))?;
                    self.jump = Some(Jump { line, range: None });
                }
                "trigger" | "dialog" => {
                    // The package is built after the first frames: the Story screen picks the node then.
                    self.selection = None;
                    self.scen.story.pending = Some(opt.to_string());
                    self.pending_tab = Some(Tab::Story);
                }
                "texts" | "characters" => {
                    let tab = if *opt == "texts" { Tab::Texts } else { Tab::Characters };
                    self.pending_tab = Some(tab);
                }
                "units" | "items" => {
                    // The first record of the package is selected, its price steps open (records.rs).
                    let (pkg, _) = self.ws.read_package()?;
                    let (kind, tab, focus) = if *opt == "units" {
                        (razdor::studio::records::Kind::Unit, Tab::Units, Focus::Units)
                    } else {
                        (razdor::studio::records::Kind::Item, Tab::Items, Focus::Items)
                    };
                    let first = razdor::studio::records::rows(&pkg, kind).into_iter().next().map(|r| r.id);
                    match kind {
                        razdor::studio::records::Kind::Unit => self.scen.unit = first,
                        razdor::studio::records::Kind::Item => self.scen.item = first,
                    }
                    self.scen.focus = focus;
                    self.scen.price_open = true;
                    self.selection = None;
                    self.scen.data_view = if kind == razdor::studio::records::Kind::Unit { data::DataView::Units } else { data::DataView::Items };
                    self.pending_tab = Some(tab);
                }
                "claude" => {
                    self.pending_tab = Some(Tab::Claude);
                }
                "claude-start" => self.claude.start(&self.ws.root.clone()),
                "map" => {
                    self.pending_tab = Some(Tab::Map);
                }
                // The Слои tab shows one layer hidden (buildings) and one locked (armies) on the shown map.
                "layers" => {
                    let map = self.maps.map.clone().unwrap_or_default();
                    let set = self.canvas.layer_sets.entry(map).or_default();
                    set.toggle_visible("buildings");
                    set.toggle_locked("armies");
                    self.pending_tab = Some(Tab::Layers);
                }
                "palette" => {
                    self.pending_tab = Some(Tab::Palette);
                }
                "round" => self.canvas.shape = razdor::studio::brush::Shape::Round,
                "army-tool" => self.set_tool(Tool::Army),
                other => {
                    if let Some(key) = other.strip_prefix("ws=") {
                        // The workspace shown is its default view: the file the snapshot opened (the Script) does not take it.
                        // The option names are the workspaces' short names (the layout key of Карта has a version).
                        let kind = match key {
                            "map" => WsKind::Map,
                            "story" => WsKind::Story,
                            "data" => WsKind::Data,
                            "test" => WsKind::Test,
                            _ => return Err(format!("no workspace {key:?}: map, story, data or test")),
                        };
                        self.ws_kind = kind;
                        self.snap_ws = Some(kind);
                        self.pending_tab = None;
                    } else if let Some(pct) = other.strip_prefix("scale=") {
                        let pct: u32 = pct.parse().map_err(|_| format!("scale {pct:?} is not a number"))?;
                        self.scale_override = Some(theme::scale_pct(pct));
                    } else if let Some(name) = other.strip_prefix("tool=") {
                        self.canvas.tool = Tool::named(name).ok_or_else(|| format!("no tool {name:?}"))?;
                    } else if let Some(id) = other.strip_prefix("building=") {
                        self.selection = Some(Selection::Building(id.to_string()));
                    } else if let Some(c) = other.strip_prefix("hover=") {
                        let v: Vec<i32> = c.split(',').map(|n| n.parse().map_err(|_| format!("hover {c:?}: x,y"))).collect::<Result<_, _>>()?;
                        let [x, y] = v[..] else { return Err(format!("hover {c:?}: x,y")) };
                        self.canvas.snap_hover = Some((x, y));
                    } else if let Some(d) = other.strip_prefix("density=") {
                        let d: u32 = d.parse().map_err(|_| format!("density {d:?} is not a number"))?;
                        self.canvas.density = d.min(100);
                    } else if let Some(n) = other.strip_prefix("brush=") {
                        let n: u32 = n.parse().map_err(|_| format!("brush {n:?} is not a number"))?;
                        if !razdor::studio::brush::BRUSH_SIZES.contains(&n) {
                            return Err(format!("brush {n} is not one of 1-6 or 9"));
                        }
                        self.canvas.brush = n;
                    } else if let Some(f) = other.strip_prefix("forest=") {
                        self.canvas.forest = if f == "thicket" {
                            razdor::studio::brush::Forest::Thicket
                        } else {
                            razdor::studio::brush::Forest::Family(f.parse().map_err(|_| format!("forest {f:?}: a family 0-9 or thicket"))?)
                        };
                    } else if let Some(kind) = other.strip_prefix("sel=") {
                        self.canvas.snap_sel = Some(kind.to_string());
                    } else if let Some(name) = other.strip_prefix("stamp-save=") {
                        self.canvas.snap_stamp = Some(name.to_string());
                    } else if other == "stamp-prompt" {
                        self.canvas.stamp_prompt = true;
                    } else if let Some(name) = other.strip_prefix("stamp=") {
                        let list = razdor::studio::region::list_stamps(&self.ws.root);
                        let stamp = list.into_iter().find(|s| s.name == name).ok_or_else(|| format!("no stamp {name:?}"))?;
                        self.canvas.clip = Some(stamp.clip);
                        self.canvas.paste = true;
                        self.canvas.float = false;
                    } else {
                        return Err(format!("no studio option {other:?}"));
                    }
                }
            }
        }
        // The caret starts at the selection's table, unless the snapshot named a line (`at=`).
        if let (Some(sel), None) = (self.selection.clone(), self.jump.as_ref()) {
            self.jump = jump_for(&self.ws, self.ws.package().as_deref(), &path, &sel);
        }
        Ok(())
    }

    /// The snapshot's `stale` option: a syntax error at the end of the shown file's text, in
    /// memory only (nothing is written), so the banner shows and the screens keep the last good state.
    fn snapshot_stale(&mut self) -> Result<(), String> {
        let i = self.selected.ok_or("no file open")?;
        let text = format!("{}\n[[army\n", self.ws.docs[i].text);
        self.ws.docs[i].text = text;
        self.touch();
        Ok(())
    }

    /// The snapshot's `conflict` option: the shown file has a visual edit in memory (the hero's
    /// mana), and the disk got another one (its mana changed): the question, with the diff shown.
    fn snapshot_conflict(&mut self, path: &str) -> Result<(), String> {
        let disk = self.ws.read_text(path)?.replacen("mana = 10", "mana = 12", 1);
        let hero = Locator { file: path.to_string(), kind: Kind::Hero, id: None, index: 0 };
        self.ws.apply(Edit::SetValue { at: hero, key: "mana".into(), value: toml_edit::Value::from(30_i64) })?;
        match self.ws.on_disk_change(path, &disk) {
            Reconcile::Asks { path, disk } => {
                let diff = diff_lines(&self.mine(&path), &disk);
                self.conflict = Some(Conflict { path, disk, diff: Some(diff) });
                Ok(())
            }
            other => Err(format!("the outside change was not a question: {other:?}")),
        }
    }

    /// The snapshot's `overlap` option: the second building of the map is moved, in memory, onto
    /// the first one's cell, so the two footprints overlap (the checks say so, the canvas marks both).
    fn snapshot_overlap(&mut self, map: &str) -> Result<(), String> {
        let file = format!("maps/{map}/map.toml");
        let spec = inspector::spec_now(&self.ws, self.ws.package().as_deref(), map).ok_or("the map does not read")?;
        let (first, second) = match (spec.building.first(), spec.building.get(1)) {
            (Some(a), Some(b)) => (a.get_ref().clone(), b.get_ref().clone()),
            _ => return Err("the map has two buildings to overlap".into()),
        };
        let at = Locator { file, kind: Kind::Building, id: Some(second.id.clone()), index: 0 };
        self.ws.apply(Edit::SetCell { at, cell: (first.cell[0] as i32, first.cell[1] as i32) })?;
        self.touch();
        Ok(())
    }

    /// The text of the open document `path` (empty when it is not open).
    fn mine(&self, path: &str) -> String {
        self.ws.docs.iter().find(|d| d.path == path).map(|d| d.text.clone()).unwrap_or_default()
    }

    /// One frame: the timers and the outside changes, the finished rebuilds (or, for a snapshot, the
    /// one it waits for), the egui interface, then the map drawn into the Map tab's rectangle, then
    /// the egui meshes over the game's frame. `art` is the game's art and install (`None` when the
    /// install is not there).
    pub fn frame(&mut self, art: Option<&DtArt>, prefs: &mut StudioPrefs) -> Action {
        let mut action = Action::None;
        crate::ui::cursor::egui_pointer(true);
        macroquad::prelude::clear_background(macroquad::prelude::Color::new(0.125, 0.145, 0.19, 1.0));
        self.installed = art.is_some();
        if self.install.is_none() {
            self.install = art.map(|a| Arc::new(a.install.clone()));
        }
        self.settle();
        self.poll_build();
        if self.wait_build {
            self.wait_build = false;
            self.wait_for_build();
        }
        self.scores.poll();
        if self.wait_scores {
            self.wait_scores = false;
            self.scores.wait();
        }
        self.refresh_marks();
        self.canvas.ai.poll();
        self.battle.poll();
        self.snapshot_todo();
        self.claude.pump();
        self.serve_mcp();
        self.canvas.shown = false;
        self.canvas.ai_shown = false;
        self.poll_play();
        self.canvas.mini = None;
        self.canvas.rect = None;
        // The palette's object thumbnails are read from the install's sprites before egui draws the frame.
        self.canvas.prepare_thumbs(art);
        // egui may run the frame's UI twice (a relayout pass): a pass's request is kept, a pass
        // without one does not clear it.
        egui_macroquad::ui(|ctx| {
            let asked = self.show(ctx, prefs);
            if !matches!(asked, Action::None) {
                action = asked;
            }
        });
        let pkg = self.ws.package();
        draw_map_tab(&mut self.canvas, &self.maps, pkg.as_deref(), self.selection.as_ref(), art);
        egui_macroquad::draw();
        self.answer_snapshots();
        action
    }

    /// What the battle tester's screen asked for: a run starts or stops, or one battle's view opens
    /// (the studio's action; the host shows it in place of the studio).
    fn battle_request(&mut self, req: battletest::Req) -> Option<Action> {
        let (Some(pkg), Some(install), Some(map)) = (self.ws.package(), self.install.clone(), self.maps.map.clone()) else { return None };
        match req {
            battletest::Req::Start => self.battle.start(pkg, install, &map),
            battletest::Req::Cancel => self.battle.cancel(),
            battletest::Req::Watch(_) => match self.battle.watch(&pkg, &install, &map) {
                Ok(view) => return Some(Action::Watch(view)),
                Err(e) => self.battle.error = Some(e),
            },
        }
        None
    }

    /// The snapshot options that need the install (`battletest`: two armies fought, the results shown;
    /// `aiview`: the AI run for ten days, the slider at day 7). Run once the install and the package
    /// are there; each runs in this frame.
    fn snapshot_todo(&mut self) {
        if self.snap_todo.is_empty() {
            return;
        }
        let (Some(pkg), Some(install), Some(map)) = (self.ws.package(), self.install.clone(), self.maps.map.clone()) else { return };
        for opt in std::mem::take(&mut self.snap_todo) {
            match opt {
                "battletest" => {
                    self.pending_tab = Some(Tab::Battle);
                    // `battlewatch`: then the first battle's view opens, as «Посмотреть бой» does.
                    let watch = self.snap_watch;
                    match razdor::studio::testers::open_map(&pkg, &install, &map) {
                        Ok(mg) => {
                            self.battle.load(&mg, &map);
                            // Two of the map's armies, the first two it offers.
                            let armies: Vec<usize> = self.battle.options.iter().enumerate().filter(|(_, o)| o.kind == razdor::studio::testers::Kind::Army).map(|(i, _)| i).collect();
                            if let [a, b, ..] = armies[..] {
                                self.battle.a.choice = a;
                                self.battle.b.choice = b;
                            }
                            if let Err(e) = self.battle.run_now(&mg, 20) {
                                self.battle.error = Some(e);
                            }
                            if watch {
                                self.battle_req = Some(battletest::Req::Watch(1));
                            }
                        }
                        Err(r) => self.battle.error = Some(r.to_text().lines().next().unwrap_or_default().to_string()),
                    }
                }
                _ => {
                    self.pending_tab = Some(Tab::AiTest);
                    self.canvas.ai.days = 10;
                    if let Err(e) = self.canvas.ai.run_now(&pkg, &install, &map) {
                        self.canvas.ai.error = Some(e);
                    }
                    self.canvas.ai.day = 7;
                }
            }
        }
    }

    /// The timers: a text burst ends after its pause, outside changes that settled are read, and a
    /// change that settled asks for a rebuild.
    fn settle(&mut self) {
        let now = Instant::now();
        if self.burst.is_some_and(|t| now.duration_since(t) >= DEBOUNCE) {
            self.ws.end_text_edit();
            self.burst = None;
        }
        self.poll_disk(now);
        if self.debounce.is_some_and(|t| now.duration_since(t) >= DEBOUNCE) {
            self.debounce = None;
        }
        if self.want_build && self.debounce.is_none() {
            self.request_build();
        }
    }

    /// A change happened: the package is rebuilt once things settle, and the canvas's marks made again.
    /// A map the generator added: the package is built again and the map is shown on the Map tab.
    fn added_map(&mut self, id: &str) {
        self.touch();
        // The map's own file is opened: the package shows the map of the file it has open.
        self.open_file(&format!("maps/{id}/map.toml"));
        switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, id);
        self.pending_tab = Some(Tab::Map);
        self.pending_tab = Some(Tab::Map);
    }

    /// The snapshot's generated map: the fixed seed on the 100-cell land, all three steps, added
    /// to the (copied) package and shown.
    /// The snapshot of the Scores tab: its picture waits for the scores of the shown map.
    fn snap_scores(&mut self) {
        self.wait_scores = true;
        self.pending_tab = Some(Tab::Scores);
    }

    fn snapshot_generated(&mut self) -> Result<(), String> {
        // The snapshot runs before the first frame, which is where the install is read: read it here.
        let install = match self.install.clone() {
            Some(install) => install,
            None => Arc::new(DtInstall::from_env().map_err(|e| format!("the generator needs the game's files: {e}"))?),
        };
        let req = razdor::studio::gen::Request {
            map: razdor::studio::gen::newmap::Options { size: 100, kind: razdor::studio::gen::newmap::LAND, keep: true, rebuild: true, ..Default::default() },
            seed: 4242,
            clock: razdor::studio::gen::clock_of(4242),
            world: Some(razdor::studio::gen::World { buildings: true, economy: true, armies: true, options: razdor::studio::gen::worldgen::Options::new(100) }),
        };
        let map = newmap::NewMap::make(&install, &req, "generated", "Generated map")?;
        razdor::studio::gen::apply::add_map(&mut self.ws, "generated", &map, false)?;
        self.added_map("generated");
        Ok(())
    }

    fn touch(&mut self) {
        let now = Instant::now();
        self.debounce = Some(now);
        self.burst = Some(now);
        self.want_build = true;
        self.marks_dirty = true;
    }

    /// Asks for a rebuild now (the save, the map switch, the check).
    fn reload(&mut self) {
        self.debounce = None;
        self.want_build = true;
        self.marks_dirty = true;
    }

    /// Sends the rebuild of the package to the build thread: the open texts and the painted terrains
    /// over the disk, the shown map compiled and walked.
    fn request_build(&mut self) {
        self.want_build = false;
        let Some(worker) = &self.worker else { return };
        self.gen += 1;
        let lang = lang();
        let job: Job = self.ws.job(self.maps.map.clone(), lang, self.install.clone());
        worker.send(self.gen, job);
    }

    /// Takes the finished rebuilds that are ready.
    fn poll_build(&mut self) {
        while let Some((gen, result)) = self.worker.as_ref().and_then(Worker::try_recv) {
            self.apply_build(gen, result);
        }
    }

    /// Waits for the build asked last (a snapshot draws the state of the build, not the one before).
    fn wait_for_build(&mut self) {
        self.debounce = None;
        self.request_build();
        loop {
            let Some((gen, result)) = self.worker.as_ref().and_then(|w| w.recv_timeout(SNAPSHOT_WAIT)) else { break };
            let reached = gen >= self.gen;
            self.apply_build(gen, result);
            if reached {
                break;
            }
        }
    }

    /// Asks the scores thread for the shown map's scores of the package as it now is.
    fn request_scores(&mut self) {
        let job = match (self.ws.package(), self.maps.map.clone()) {
            (Some(pkg), Some(map)) => Some(razdor::studio::scores::Job { pkg, map, lang: lang().to_string(), install: self.install.clone() }),
            _ => None,
        };
        self.scores.request(job);
    }

    /// Applies one finished rebuild, if it is newer than the last one applied.
    fn apply_build(&mut self, gen: u64, result: Result<razdor::studio::build::Built, String>) {
        match result {
            Ok(built) if gen > self.applied => {
                self.applied = gen;
                let done = self.ws.install(built);
                self.apply_installed(done);
                self.keep_a_map();
                self.request_scores();
            }
            Ok(_) => {}
            Err(e) => self.log.push(e),
        }
        if self.check_asked {
            self.check_asked = false;
            let errors = self.ws.problems.iter().filter(|d| d.severity == Severity::Error).count();
            let warnings = self.ws.problems.len() - errors;
            self.log.push(trf!("Check: {errors} errors, {warnings} warnings", errors, warnings));
        }
        self.marks_dirty = true;
    }

    /// The map's compile and walk of an installed rebuild (for the shown map only).
    fn apply_installed(&mut self, done: Installed) {
        let Some(map) = done.map else { return };
        if let Some(compiled) = done.compiled {
            self.map_problems = self.maps.install(&map, compiled);
            self.canvas.moving = None;
        }
        if self.maps.map.as_deref() == Some(map.as_str()) {
            if let Some(unreached) = done.unreached {
                self.canvas.unreached = unreached;
            }
        }
    }

    /// Shows a map of the package when the one shown is not in it (the first build, a deleted map):
    /// the rule of `choose_map`, then a rebuild for it.
    fn keep_a_map(&mut self) {
        let Some(pkg) = self.ws.package() else { return };
        let kept = self.maps.map.as_ref().is_some_and(|m| pkg.maps.contains_key(m));
        if kept {
            return;
        }
        if let Some(id) = choose_map(&pkg, self.opened.as_deref()) {
            switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, &id);
            self.reload();
        }
    }

    /// The problems the Problems tab lists: the checks', then the failed compile's (those the checks
    /// already listed are not repeated).
    fn all_problems(&self) -> Vec<Diagnostic> {
        self.ws
            .problems
            .iter()
            .chain(self.map_problems.iter().filter(|d| !self.ws.problems.contains(d)))
            .cloned()
            .collect()
    }

    /// Makes the canvas's marks again when they are due: the shown map's tables, each with the
    /// diagnostics of its lines (`studio::map::marks`).
    fn refresh_marks(&mut self) {
        if !self.marks_dirty {
            return;
        }
        self.marks_dirty = false;
        let Some(map) = self.maps.map.clone() else {
            self.canvas.marks.clear();
            return;
        };
        let file = format!("maps/{map}/map.toml");
        let text = self.ws.text_of(&file).unwrap_or_default();
        let spans = self.ws.spans(&file).unwrap_or_default();
        self.canvas.marks = marks(&file, &text, &spans, &self.all_problems());
    }

    /// Reads the outside changes the watcher reported, once they settle, and asks the model what
    /// each one means for an open file (or a painted terrain).
    fn poll_disk(&mut self, now: Instant) {
        if let Some(w) = &self.watch {
            for path in w.changed() {
                if path == watch::ALL {
                    for doc in &self.ws.docs {
                        self.disk_pending.insert(doc.path.clone(), now);
                    }
                    for t in &self.ws.terrains {
                        self.disk_pending.insert(t.path.clone(), now);
                    }
                } else {
                    self.disk_pending.insert(path, now);
                }
            }
        }
        let due: Vec<String> = self.disk_pending.iter().filter(|(_, t)| now.duration_since(**t) >= SETTLE).map(|(p, _)| p.clone()).collect();
        if due.is_empty() {
            return;
        }
        for path in &due {
            self.disk_pending.remove(path);
        }
        if let Err(e) = self.ws.refresh_entries() {
            self.log.push(e);
        }
        for path in due {
            if self.ws.terrains.iter().any(|t| t.path == path) {
                match self.ws.read_bytes(&path).and_then(|disk| self.ws.on_disk_terrain(&path, &disk)) {
                    Ok(true) => {
                        self.log.push(trf!("{path} changed on disk: reloaded (Ctrl+Z takes the old picture back)", path));
                        self.touch();
                    }
                    Ok(false) => {}
                    Err(e) => self.log.push(e),
                }
                continue;
            }
            if !self.ws.docs.iter().any(|d| d.path == path) {
                continue;
            }
            let Ok(disk) = self.ws.read_text(&path) else { continue };
            match self.ws.on_disk_change(&path, &disk) {
                Reconcile::Ignored => {}
                Reconcile::Reloaded => {
                    self.log.push(trf!("{path} changed on disk: reloaded (Ctrl+Z takes the old text back)", path));
                    self.touch();
                }
                Reconcile::Asks { path, disk } => self.conflict = Some(Conflict { path, disk, diff: None }),
            }
        }
    }

    /// The frame's chrome and the four workspaces: the top bar, the status bar, the drawer, the
    /// workspace's dock, the Settings window. The commands of the frame run first (`commands_frame`).
    fn show(&mut self, ctx: &egui::Context, prefs: &mut StudioPrefs) -> Action {
        // The zoom this pass's screen rectangle was measured at, before the scale is applied.
        let zoom_before = ctx.zoom_factor();
        theme::apply(ctx, self.scale(prefs));
        if !self.laid_out {
            self.lay_out(ctx, prefs, zoom_before);
        }
        let mut action = Action::None;
        // The file keys under the labels: the setting, or a snapshot's option.
        inspector_kit::set_inline_keys(ctx, prefs.field_keys || self.snap_keys);
        if let Some(text) = self.snap_find.take() {
            inspector_kit::set_query(ctx, &text);
        }
        // The terminal of the Claude dock takes the keys first while it has the keyboard.
        self.claude.begin_frame();
        self.claude.take_input(ctx);
        // The shortcuts wait while the Settings window captures a key, and while the palette has the keyboard.
        if !self.settings.capturing() && !self.palette.is_open() {
            if let Some(asked) = self.commands_frame(ctx, prefs) {
                action = asked;
            }
        }
        let pkg = self.ws.package();
        let problems = self.all_problems();
        let errors = problems.iter().filter(|d| matches!(d.severity, Severity::Error)).count();
        if errors > self.errors_seen {
            // A new error opens the drawer on Проблемы by itself.
            self.drawer_open = true;
            self.drawer = Drawer::Problems;
        }
        self.errors_seen = errors;
        if self.top_bar(ctx, prefs) {
            action = self.leave();
        }
        self.status_bar(ctx);
        let style = Style::from_egui(ctx.style().as_ref());
        let idx = self.ws_kind.index();
        let key = self.ws_kind.key();
        // The Map workspace's tool strip, a column of its own at the left edge (spec §5.1), unless Tab hid it.
        let on_map = self.ws_kind == WsKind::Map;
        let show_side = on_map && !self.side_hidden;
        let mut strip_cmd: Option<&'static str> = None;
        if show_side {
            let active = self.canvas.tool;
            let keys = &prefs.keys;
            egui::SidePanel::left("studio_tools")
                .exact_width(theme::TOOL_W)
                .resizable(false)
                .frame(chrome_frame(5, 6))
                .show(ctx, |ui| {
                    strip_cmd = map_tab::tool_strip(ui, active, &|id| commands::command(id).map(|c| commands::tip(c, keys)).unwrap_or_default());
                });
        }
        let keys = &prefs.keys;
        let mut viewer = Viewer {
            keys,
            ws: &mut self.ws,
            pkg: pkg.clone(),
            selected: &mut self.selected,
            log: &mut self.log,
            map_problems: &problems,
            maps: &mut self.maps,
            canvas: &mut self.canvas,
            selection: &mut self.selection,
            has_install: self.installed,
            jump: &mut self.jump,
            cursor_seen: &mut self.cursor_seen,
            claude: &mut self.claude,
            open: None,
            focus_map: false,
            changed: false,
            scen: &mut self.scen,
            install: self.install.clone(),
            battle: &mut self.battle,
            battle_req: &mut self.battle_req,
            scores: &self.scores,
            goto: None,
            snap_jump: &mut self.snap_jump,
        };
        let mut drawer_open = self.drawer_open;
        let mut drawer = self.drawer;
        let drawer_h = if drawer_open { theme::DRAWER_OPEN_H } else { theme::DRAWER_CLOSED_H };
        egui::TopBottomPanel::bottom("studio_drawer")
            .exact_height(drawer_h)
            .resizable(false)
            .frame(chrome_frame(8, 3))
            .show(ctx, |ui| drawer_panel(ui, &mut viewer, &mut drawer_open, &mut drawer, problems.len()));
        self.drawer_open = drawer_open;
        self.drawer = drawer;
        // With the side panels hidden, the Map tab alone fills the window (the layout is kept for later).
        let mut solo = DockState::new(vec![Tab::Map]);
        let (dock, dock_id) = if on_map && self.side_hidden { (&mut solo, "studio-workspace-solo") } else { (&mut self.docks[idx], "studio-workspace") };
        DockArea::new(dock)
            .id(Id::new((dock_id, key)))
            .style(style)
            .show_close_buttons(false)
            .show_leaf_close_all_buttons(false)
            .show(ctx, &mut viewer);
        let (open, focus_map, changed) = (viewer.open.take(), viewer.focus_map, viewer.changed);
        let goto = viewer.goto.take();
        if let Some(id) = strip_cmd {
            self.run_command(id, prefs);
        }
        if let Some(dest) = goto {
            self.jump_to(dest);
        }
        if changed {
            self.touch();
        }
        if let Some(path) = open {
            self.open_file(&path);
        }
        if let Some(req) = self.battle_req.take() {
            if let Some(ask) = self.battle_request(req) {
                action = ask;
            }
        }
        if focus_map {
            self.pending_tab = Some(Tab::Map);
        }
        if let Some(tab) = self.pending_tab.take().or_else(|| self.scen.want_tab.take()) {
            self.show_tab(tab);
        }
        if let Some(id) = self.scen.want_map.take() {
            switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, &id);
            self.reload();
        }
        if let Some(kind) = self.snap_ws {
            self.ws_kind = kind;
        }
        self.save_layout(prefs);
        let scale = self.scale(prefs);
        // The function keys are read only while the Settings window captures a key.
        let mq = if self.settings.capturing() { commands::mq_presses() } else { Vec::new() };
        self.settings.show(ctx, prefs, scale, &mq);
        if std::mem::take(&mut self.settings.reset_layout) {
            self.reset_layout(prefs);
        }
        // The command palette (Ctrl+Shift+P, Ctrl+P, the top bar's field): its pick runs like the command or jumps.
        let pkg_for_goto = self.ws.package();
        let files: Vec<String> = self.ws.entries.iter().filter(|e| is_text(e)).cloned().collect();
        let goto_list = move || match pkg_for_goto {
            Some(pkg) => razdor::studio::nav::goto_index(&pkg, &files),
            None => Vec::new(),
        };
        if let Some(pick) = self.palette.show(ctx, &prefs.keys, goto_list) {
            match pick {
                palette::Pick::Command(id) => {
                    if let Some(asked) = self.run_command(id, prefs) {
                        action = asked;
                    }
                }
                palette::Pick::Go(g) => self.jump_to(g.dest),
            }
        }
        if std::mem::take(&mut self.find_asked) {
            inspector_kit::request_find(ctx);
        }
        self.conflict_ui(ctx);
        if let Some(id) = self.newmap.show(ctx, &mut self.ws, self.install.as_ref()) {
            self.added_map(&id);
        }
        if self.exit_prompt {
            let leave = self.exit_prompt_ui(ctx);
            if leave {
                action = Action::Exit;
            }
        }
        action
    }

    /// The interface scale of this frame: the snapshot's, else the setting's.
    fn scale(&self, prefs: &StudioPrefs) -> u32 {
        theme::scale_pct(self.scale_override.unwrap_or(prefs.scale))
    }

    /// The top bar: the workspace switcher, Save, Undo, Redo, Проверка, Играть, and on the right Exit,
    /// Settings, the Package menu and the command field. Returns whether Exit was clicked.
    fn top_bar(&mut self, ctx: &egui::Context, prefs: &StudioPrefs) -> bool {
        let keys = &prefs.keys;
        let mut exit = false;
        egui::TopBottomPanel::top("studio_top").exact_height(theme::TOP_H).frame(chrome_frame(8, 4)).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(RichText::new(self.name.as_str()).strong());
                ui.add_space(8.0);
                for kind in WsKind::ALL {
                    let tip = commands::command(kind.command()).map(|c| commands::tip(c, keys)).unwrap_or_default();
                    let text = RichText::new(format!("{}  {}", kind.icon(), tr(kind.name())));
                    if ui.add(Button::new(text).selected(self.ws_kind == kind)).on_hover_text(tip).clicked() {
                        self.ws_kind = kind;
                    }
                }
                ui.separator();
                if cmd_button(ui, "save", keys, true, false, "").clicked() {
                    self.save_selected();
                }
                let undo = self.ws.history.undo_label().map(str::to_string);
                let redo = self.ws.history.redo_label().map(str::to_string);
                if cmd_button(ui, "undo", keys, undo.is_some(), false, &label_note(undo.as_deref())).clicked() {
                    self.undo();
                }
                if cmd_button(ui, "redo", keys, redo.is_some(), false, &label_note(redo.as_deref())).clicked() {
                    self.redo();
                }
                ui.separator();
                if cmd_button(ui, "check", keys, true, false, "").clicked() {
                    self.check();
                }
                // Играть starts the game on this map (F5); a refusal shows as the button's note and as a message.
                let note = self.play_refusal().map(|r| self.refusal_text(r)).unwrap_or_default();
                if cmd_button(ui, "play", keys, !self.playing(), false, &note).clicked() {
                    self.play_game();
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if cmd_button(ui, "exit", keys, true, false, "").clicked() {
                        exit = true;
                    }
                    if cmd_button(ui, "settings", keys, true, self.settings.open, "").clicked() {
                        self.settings.open = !self.settings.open;
                    }
                    ui.menu_button(RichText::new(ic::PACKAGE).size(theme::ICON), |ui| {
                        if ui.button(tr("Package settings")).clicked() {
                            self.scen.focus = Focus::Package;
                            ui.close_menu();
                        }
                        if ui.button(tr("New map (generator)…")).clicked() {
                            self.newmap.open = true;
                            ui.close_menu();
                        }
                        if ui.button(tr("Map settings")).clicked() {
                            self.scen.focus = Focus::Header;
                            ui.close_menu();
                        }
                        for (label, tab) in [("Story", Tab::Story), ("Campaign", Tab::Campaign), ("Texts", Tab::Texts), ("Characters", Tab::Characters), ("Variables", Tab::Vars), ("Units", Tab::Units), ("Items", Tab::Items)] {
                            if ui.button(tr(label)).clicked() {
                                self.pending_tab = Some(tab);
                                ui.close_menu();
                            }
                        }
                        if ui.button(tr("Battle test")).clicked() {
                            self.pending_tab = Some(Tab::Battle);
                            ui.close_menu();
                        }
                    })
                    .response
                    .on_hover_text(tr("Package"));
                    // The command field: a click opens the palette with commands and objects (`palette.rs`).
                    let tip = commands::command("palette").map(|c| commands::tip(c, keys)).unwrap_or_default();
                    let field = chrome_frame(6, 2).fill(theme::WELL).stroke(egui::Stroke::new(1.0, theme::LINE)).show(ui, |ui| {
                        ui.set_width(230.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(ic::MAGNIFYING_GLASS).color(theme::DIM));
                            ui.label(RichText::new(tr("Commands and objects")).color(theme::DIM));
                        });
                    });
                    let click = ui.interact(field.response.rect, Id::new("studio-command-field"), Sense::click());
                    if click.clicked() {
                        self.palette.request(palette::Mode::All, "");
                    }
                    click.on_hover_text(tip);
                });
            });
        });
        exit
    }

    /// The status bar: the cell under the pointer and the Map tool's hint, and the save state.
    fn status_bar(&self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("studio_status").exact_height(theme::STATUS_H).frame(chrome_frame(8, 2)).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if self.playing() {
                    ui.label(RichText::new(format!("{} {}", ic::PLAY, tr("The game is running"))).text_style(egui::TextStyle::Small).color(theme::TEXT));
                    ui.label(RichText::new("·").color(theme::FAINT));
                }
                if self.ws_kind == WsKind::Map {
                    if let Some((x, y)) = self.canvas.hover {
                        ui.label(RichText::new(format!("{}: {x}, {y}", tr("Cell"))).text_style(egui::TextStyle::Small));
                        ui.label(RichText::new("·").color(theme::FAINT));
                    }
                    // The terrain under the pointer and the top object on its cell (spec §5.9).
                    if let Some((terrain, object)) = &self.canvas.hover_info {
                        ui.label(RichText::new(terrain.as_str()).text_style(egui::TextStyle::Small));
                        if let Some(object) = object {
                            ui.label(RichText::new(format!("· {object}")).text_style(egui::TextStyle::Small).color(theme::DIM));
                        }
                        ui.label(RichText::new("·").color(theme::FAINT));
                    }
                    ui.label(RichText::new(tr(tool_hint(self.canvas.tool))).text_style(egui::TextStyle::Small).color(theme::DIM));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let state = if self.ws.any_dirty() { tr("Unsaved changes").to_string() } else { format!("{} {}", ic::CHECK, tr("Saved")) };
                    ui.label(RichText::new(state).color(theme::DIM).text_style(egui::TextStyle::Small));
                });
            });
        });
    }

    /// The commands of this frame: the clipboard events, the shortcuts (the registry's, with the
    /// author's bindings) and the function keys. Returns the action a command asked for.
    fn commands_frame(&mut self, ctx: &egui::Context, prefs: &mut StudioPrefs) -> Option<Action> {
        let ws = self.ws_kind;
        inspector_kit::clear_find(ctx);
        let inspector_hot = inspector_kit::hot(ctx);
        let wants = ctx.wants_keyboard_input();
        let active = move |scope: Scope| match scope {
            Scope::Global => true,
            // A Map shortcut with Ctrl (Ctrl+A, Ctrl+D) waits while a text field has the keyboard.
            Scope::Map => ws == WsKind::Map && !wants,
            Scope::Inspector => inspector_hot,
            // The graph's and the sheet's keys wait while the Inspector has the pointer (its Ctrl+F is the field search).
            Scope::Story => ws == WsKind::Story && !wants && !inspector_hot,
            Scope::Data => ws == WsKind::Data && !wants && !inspector_hot,
        };
        // The function keys come from macroquad (egui-miniquad does not deliver them). A relayout pass of the
        // same frame may read them again: a command that runs twice (Проверка) only rebuilds twice.
        let ids = commands::pressed(ctx, &prefs.keys, active, &commands::mq_presses());
        // A paste's text is read before the clipboard events are taken: the sheet types it into its cell.
        let pasted = ctx.input(|i| {
            i.events.iter().find_map(|e| match e {
                egui::Event::Paste(t) => Some(t.clone()),
                egui::Event::Text(t) if i.modifiers.command => Some(t.clone()),
                _ => None,
            })
        });
        let clips = ctx.input_mut(|i| {
            let held = i.modifiers.command;
            let shift = i.modifiers.shift;
            commands::clipboard(&mut i.events, held, shift, wants)
        });
        if ws == WsKind::Data {
            for clip in &clips {
                match clip {
                    Clip::Copy => self.scen.sheet.copy = true,
                    Clip::Paste | Clip::PasteInPlace => self.scen.sheet.paste = pasted.clone(),
                    Clip::Cut => {}
                }
            }
        }
        if ws == WsKind::Map {
            for clip in clips {
                self.clip(clip);
            }
        }
        let mut action = None;
        for id in ids {
            if let Some(asked) = self.run_command(id, prefs) {
                action = Some(asked);
            }
        }
        action
    }

    /// Runs one command of the registry.
    fn run_command(&mut self, id: &str, prefs: &mut StudioPrefs) -> Option<Action> {
        match id {
            "save" => self.save_selected(),
            "save-all" => self.save_all(),
            "play" => self.play_game(),
            "palette" => self.palette.request(palette::Mode::Commands, ""),
            "goto" => self.palette.request(palette::Mode::Go, ""),
            "find" => self.find_asked = true,
            "copy" => self.clip(Clip::Copy),
            "cut" => self.clip(Clip::Cut),
            "paste" => self.clip(Clip::Paste),
            "paste-in-place" => self.clip(Clip::PasteInPlace),
            "undo" => self.undo(),
            "redo" => self.redo(),
            "check" => self.check(),
            "drawer" => self.drawer_open = !self.drawer_open,
            "cancel" => {
                self.scen.sheet.cancel = true;
                self.canvas.paste = false;
                self.canvas.float = false;
                // Esc leaves a placement too: the tool goes back to Select.
                if self.canvas.tool.places() {
                    self.canvas.tool = Tool::Select;
                }
            }
            "settings" => self.settings.open = true,
            "open-in-file" => self.open_in_file(),
            "find-node" => {
                self.scen.outline_tab = outline::OutlineTab::Story;
                self.scen.want_tab = Some(Tab::Outline);
                self.scen.focus_find = true;
            }
            "find-data" => self.scen.focus_find = true,
            // The graph's keys: the frame goes back to the readable zoom centred on the node, Home fits all,
            // Delete and Ctrl+D act on the selected trigger or dialog (in the story screen's own frame).
            "frame-node" => self.scen.story.zoom = None,
            "fit-nodes" => self.scen.story.fit = true,
            "dup-node" => self.scen.story_req = Some(story::StoryReq::Duplicate),
            "delete" if self.ws_kind == WsKind::Story => {
                if self.scen.story.sel != story::StorySel::None {
                    self.scen.story_req = Some(story::StoryReq::Delete);
                }
            }
            "delete" => {
                // Delete clears the selection's contents on the visible unlocked layers; without one it
                // removes the selected object, as before.
                if self.ws_kind == WsKind::Map && self.canvas.sel.is_some() {
                    let set = selection::set_of(&self.canvas, &self.maps);
                    if selection::delete(&mut self.ws, &mut self.maps, &mut self.canvas, &set, &mut self.log) {
                        self.touch();
                    }
                } else {
                    self.delete_selection();
                }
            }
            "reset-layout" => self.reset_layout(prefs),
            "exit" => return Some(self.leave()),
            "side-panels" => self.side_hidden = !self.side_hidden,
            "swap-picks" => {
                let (now, before) = (self.canvas.pick, self.canvas.pick_prev);
                self.canvas.pick_prev = now;
                self.canvas.pick = before;
                before.apply(&mut self.canvas);
            }
            "brush-smaller" => self.canvas.brush = self.canvas.brush.saturating_sub(1).max(1),
            "brush-larger" => self.canvas.brush = (self.canvas.brush + 1).min(15),
            "density-less" => self.canvas.density = self.canvas.density.saturating_sub(10),
            "density-more" => self.canvas.density = (self.canvas.density + 10).min(100),
            "size-1" => self.canvas.brush = 1,
            "size-2" => self.canvas.brush = 2,
            "size-3" => self.canvas.brush = 3,
            "size-4" => self.canvas.brush = 4,
            "size-5" => self.canvas.brush = 5,
            "size-6" => self.canvas.brush = 6,
            "size-9" => self.canvas.brush = 9,
            "zoom-in" => self.canvas.zoom_by(1.25),
            "zoom-out" => self.canvas.zoom_by(0.8),
            "zoom-100" => {
                let zoom = self.canvas.zoom();
                self.canvas.zoom_by(1.0 / zoom);
            }
            "zoom-fit" => self.canvas.fit = true,
            "tool-select" => self.set_tool(Tool::Select),
            // B takes the palette's last choice: a terrain kind, the hills, a forest or an object.
            "tool-brush" => {
                let pick = self.canvas.pick;
                pick.apply(&mut self.canvas);
            }
            "tool-eraser" => self.set_tool(Tool::Eraser),
            "tool-fill" => self.set_tool(Tool::Fill),
            "tool-eyedrop" => self.set_tool(Tool::Eyedrop),
            "tool-marquee" => self.set_tool(Tool::Marquee),
            "tool-lasso" => self.set_tool(Tool::Lasso),
            "tool-wand" => self.set_tool(Tool::Wand),
            "select-all" => selection::select_all(&mut self.canvas, &self.maps),
            "deselect" => selection::deselect(&mut self.canvas),
            "invert" => selection::invert(&mut self.canvas, &self.maps),
            "save-stamp" => {
                if self.canvas.sel.is_some() {
                    self.canvas.stamp_prompt = true;
                } else {
                    self.log.push(tr("Select cells first: M, L or W").to_string());
                }
            }
            // Enter commits a floating paste at the hovered cell (or where the clip was copied from).
            "commit-paste" => {
                if self.canvas.float {
                    let at = self.canvas.hover.map_or(self.canvas.clip_origin, |(x, y)| (i64::from(x), i64::from(y)));
                    let set = selection::set_of(&self.canvas, &self.maps);
                    if selection::commit_float(&mut self.ws, &mut self.maps, &mut self.canvas, &set, at, &mut self.log) {
                        self.touch();
                    }
                }
            }
            "tool-stamp" => self.set_tool(Tool::Stamp),
            "tool-army" => self.set_tool(Tool::Army),
            "tool-building" => self.set_tool(Tool::Building),
            "tool-point" => self.set_tool(Tool::Point),
            "tool-hand" => self.set_tool(Tool::Hand),
            other => {
                if let Some(kind) = WsKind::of_command(other) {
                    self.ws_kind = kind;
                }
            }
        }
        None
    }

    /// A jump (Ctrl+P, a score line, an AI log entry): the workspace of the destination is shown, and its
    /// object is selected, or its cell centred, or its file's line is the caret (`studio::nav`).
    fn jump_to(&mut self, dest: nav::Destination) {
        match dest {
            nav::Destination::Map { map, object, cell } => {
                if self.maps.map.as_deref() != Some(map.as_str()) {
                    switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, &map);
                    self.reload();
                }
                // The object's cell is centred on; a cell is outlined too (the Оценка line's cells).
                let at = match (&object, cell) {
                    (_, Some(c)) => Some((c.0 as f32 + 0.5, c.1 as f32 + 0.5)),
                    (Some(sel), None) => self
                        .ws
                        .package()
                        .and_then(|pkg| pkg.maps.get(&map).and_then(|m| m.spec.as_ref()).and_then(|spec| razdor::studio::map::cell_of(spec, sel)))
                        .map(|[x, y]| (x as f32 + 0.5, y as f32 + 0.5)),
                    (None, None) => None,
                };
                if let Some(sel) = object {
                    self.selection = Some(sel);
                }
                if let Some((x, y)) = at {
                    self.canvas.centre = Some((x, y));
                }
                self.pending_tab = Some(Tab::Map);
            }
            nav::Destination::Story(node) => {
                let (sel, centre, owner) = match &node {
                    nav::StoryNode::Trigger(id) => {
                        let owner = self.ws.package().and_then(|pkg| pkg.triggers.iter().find(|t| &t.value.id == id).and_then(|t| t.value.map.clone()));
                        (StorySel::Trigger(id.clone()), sg::trigger_id(id), owner)
                    }
                    nav::StoryNode::Dialog(id) => (StorySel::Dialog(id.clone()), sg::dialog_id(id), None),
                };
                // The node's map is shown when it is another one (the outline does the same).
                if let Some(m) = owner.filter(|m| self.maps.map.as_deref() != Some(m.as_str())) {
                    self.scen.want_map = Some(m);
                }
                self.scen.story.sel = sel;
                self.scen.story.centre = Some(centre);
                self.scen.story_view = outline::StoryView::Story;
                self.scen.focus = Focus::Story;
                self.pending_tab = Some(Tab::Story);
            }
            nav::Destination::Var(name) => {
                self.scen.outline_tab = outline::OutlineTab::Vars;
                self.scen.story.sel = StorySel::Var(name.clone());
                self.scen.story.centre = Some(sg::var_id(&name));
                self.scen.story_view = outline::StoryView::Story;
                self.scen.focus = Focus::Story;
                self.pending_tab = Some(Tab::Story);
            }
            nav::Destination::Character(id) => {
                self.scen.outline_tab = outline::OutlineTab::Characters;
                self.scen.character = Some(id);
                self.pending_tab = Some(Tab::Outline);
            }
            nav::Destination::Unit(id) => {
                self.scen.unit = Some(id);
                self.scen.focus = Focus::Units;
                self.scen.data_view = data::DataView::Units;
                self.pending_tab = Some(Tab::Units);
            }
            nav::Destination::Item(id) => {
                self.scen.item = Some(id);
                self.scen.focus = Focus::Items;
                self.scen.data_view = data::DataView::Items;
                self.pending_tab = Some(Tab::Items);
            }
            nav::Destination::File { path, line } => {
                self.open_file(&path);
                self.scen.data_view = data::DataView::Files;
                self.jump = Some(Jump { line, range: None });
                self.pending_tab = Some(Tab::Script);
            }
        }
    }

    /// One clipboard action on the Map (Copy, Cut, Paste, Paste in place) on the selection, as the keys and the
    /// palette run it. On the Данные workspace Copy copies the sheet's cells (the sheet takes it).
    fn clip(&mut self, clip: Clip) {
        if self.ws_kind == WsKind::Data {
            if clip == Clip::Copy {
                self.scen.sheet.copy = true;
            }
            return;
        }
        let set = selection::set_of(&self.canvas, &self.maps);
        match clip {
            Clip::Copy => selection::copy(&mut self.ws, &self.maps, &mut self.canvas, &set, &mut self.log),
            Clip::Cut => {
                if selection::cut(&mut self.ws, &mut self.maps, &mut self.canvas, &set, &mut self.log) {
                    self.touch();
                }
            }
            Clip::Paste => selection::arm_paste(&mut self.canvas, &mut self.log),
            Clip::PasteInPlace => {
                if selection::paste_in_place(&mut self.ws, &mut self.maps, &mut self.canvas, &set, &mut self.log) {
                    self.touch();
                }
            }
        }
    }

    /// Whether the game started by «Играть» is running (the snapshot's `play` shows it so as well).
    fn playing(&self) -> bool {
        self.play.is_some() || self.play_shown
    }

    /// Why «Играть» cannot start now, if it cannot: no install, errors in the package (Проверка's), no map.
    fn play_refusal(&self) -> Option<play::Refusal> {
        let errors = self.all_problems().iter().filter(|d| d.severity == Severity::Error).count();
        play::refusal(self.install.is_some(), errors, self.maps.map.is_some())
    }

    /// The words of a refusal, as the status and the log say them.
    fn refusal_text(&self, why: play::Refusal) -> String {
        match why {
            play::Refusal::NoInstall => tr("The game does not start: its files are needed (set RAZDOR_DT_DIR)").to_string(),
            play::Refusal::Errors(errors) => trf!("The game does not start: the package has {errors} errors (see Problems)", errors = errors),
            play::Refusal::NoMap => tr("The game does not start: no map is shown").to_string(),
        }
    }

    /// Logs a refusal and opens the drawer on what explains it (Проблемы for errors, else Вывод).
    fn refuse(&mut self, text: String, why: Option<play::Refusal>) {
        self.log.push(text);
        self.drawer = if matches!(why, Some(play::Refusal::Errors(_))) { Drawer::Problems } else { Drawer::Output };
        self.drawer_open = true;
    }

    /// «Играть» (F5, UX6): saves the open files, then starts the game on the shown map in its own process, from
    /// the package's own start (`--pack <folder> --map <id>`, `studio::play`). A refusal is logged. The studio keeps
    /// running; the game is watched by `poll_play`.
    fn play_game(&mut self) {
        if self.playing() {
            self.refuse(tr("The game is already running").to_string(), None);
            return;
        }
        if let Some(why) = self.play_refusal() {
            let text = self.refusal_text(why);
            self.refuse(text, Some(why));
            return;
        }
        if self.ws.any_dirty() {
            self.save_all();
            if self.ws.any_dirty() {
                return;
            }
        }
        let (Some(install), Some(map)) = (self.install.clone(), self.maps.map.clone()) else { return };
        // The game keeps its saves, settings and log in its own folder inside Razdor's (never the player's).
        let Some(play_dir) = razdor::platform::app_dir().map(|d| d.join(razdor::platform::PLAY_DIR)) else {
            self.refuse(tr("The game does not start: Razdor's folder is unknown").to_string(), None);
            return;
        };
        if let Err(e) = std::fs::create_dir_all(&play_dir) {
            let text = trf!("The game could not start: {reason}", reason = e.to_string());
            self.refuse(text, None);
            return;
        }
        let launch = match std::env::current_exe() {
            Ok(exe) => play::launch(&exe, &self.ws.root, &map, &install.dir, &play_dir),
            Err(e) => {
                let text = trf!("The game could not start: {reason}", reason = e.to_string());
                self.refuse(text, None);
                return;
            }
        };
        let mut command = std::process::Command::new(&launch.program);
        command.args(&launch.args).envs(launch.envs.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        match command.spawn() {
            Ok(child) => {
                self.play = Some(child);
                self.log.push(trf!("The game starts on {map}", map = map));
            }
            Err(e) => {
                let text = trf!("The game could not start: {reason}", reason = e.to_string());
                self.refuse(text, None);
            }
        }
    }

    /// Takes the game's exit when it comes (the status bar stops saying it runs).
    fn poll_play(&mut self) {
        let Some(child) = self.play.as_mut() else { return };
        if let Ok(None) = child.try_wait() {
            return;
        }
        self.play = None;
        self.log.push(tr("The game has closed").to_string());
    }

    /// Makes a Map tool the current one (the tool strip and its shortcut). The stamp tool needs an armed
    /// stamp: the Stamps tab arms one.
    fn set_tool(&mut self, tool: Tool) {
        self.canvas.tool = tool;
        self.canvas.paste = false;
        self.canvas.float = false;
        self.canvas.sel_shape = None;
        self.canvas.lasso.clear();
        self.canvas.moving = None;
        self.canvas.paint_last = None;
        if tool == Tool::Stamp {
            if self.canvas.clip.is_some() {
                self.canvas.paste = true;
            } else {
                self.log.push(tr("Pick a stamp in the Stamps tab first").to_string());
            }
        }
    }

    /// Delete: removes the selected object (the Inspector's delete).
    fn delete_selection(&mut self) {
        if let (Some(sel), Some(map)) = (self.selection.clone(), self.maps.map.clone()) {
            let pkg = self.ws.package();
            if inspector::delete_selected(&mut self.ws, &mut self.log, pkg.as_deref(), &map, &sel) {
                self.selection = None;
                self.touch();
            }
        }
    }

    /// Exit (the button, Проверка's neighbour): the unsaved changes ask first.
    fn leave(&mut self) -> Action {
        if self.ws.any_dirty() {
            self.exit_prompt = true;
            Action::None
        } else {
            Action::Exit
        }
    }

    /// Shows a tab: in the current workspace when it has the tab, else in the first workspace that has it.
    /// The screens of UX5 have their own places: the old tabs of the Story and Data screens route to them
    /// (the Campaign to the Story centre's switch, the Characters and Variables to the outline's tabs, the
    /// units, items and texts to the Данные views, the Script to the Файлы view unless a Script dock shows).
    fn show_tab(&mut self, tab: Tab) {
        let tab = self.route(tab);
        let order = std::iter::once(self.ws_kind).chain(WsKind::ALL.into_iter().filter(|k| *k != self.ws_kind));
        for kind in order.collect::<Vec<_>>() {
            let dock = &mut self.docks[kind.index()];
            if let Some(at) = dock.find_tab(&tab) {
                dock.set_active_tab(at);
                self.ws_kind = kind;
                return;
            }
        }
    }

    /// The dock tab a screen tab is shown in, after the screen's own place (`show_tab`) is set.
    fn route(&mut self, tab: Tab) -> Tab {
        match tab {
            Tab::Campaign => {
                self.scen.story_view = outline::StoryView::Campaign;
                Tab::Story
            }
            Tab::Characters | Tab::Vars => {
                self.scen.outline_tab = if tab == Tab::Vars { outline::OutlineTab::Vars } else { outline::OutlineTab::Characters };
                Tab::Outline
            }
            Tab::Units | Tab::Items | Tab::Texts => {
                self.scen.data_view = match tab {
                    Tab::Units => data::DataView::Units,
                    Tab::Items => data::DataView::Items,
                    _ => data::DataView::Texts,
                };
                Tab::Data
            }
            Tab::Script if !self.docks.iter().any(|d| d.find_tab(&Tab::Script).is_some()) => {
                self.scen.data_view = data::DataView::Files;
                Tab::Data
            }
            other => other,
        }
    }

    /// The first frame: each workspace's layout is its saved one (a snapshot uses the defaults), made for
    /// the window's width.
    fn lay_out(&mut self, ctx: &egui::Context, prefs: &StudioPrefs, zoom_before: f32) {
        // The window's width in points at the studio's scale (the rectangle is in points at `zoom_before`).
        self.width = ctx.screen_rect().width() * zoom_before / theme::zoom(self.scale(prefs));
        for kind in WsKind::ALL {
            let fresh = default_dock(kind, self.width, self.side && kind == WsKind::Map);
            let saved = if self.snapshot { None } else { prefs.layouts.get(kind.key()).and_then(|j| serde_json::from_str::<DockState<Tab>>(j).ok()) };
            self.docks[kind.index()] = saved.unwrap_or(fresh);
            self.layout_default[kind.index()] = workspaces::to_json(&default_dock(kind, self.width, false)).unwrap_or_default();
        }
        self.laid_out = true;
    }

    /// Keeps the shown workspace's layout in the settings when it differs from the default (and drops it
    /// when it is the default again). A snapshot keeps nothing.
    fn save_layout(&self, prefs: &mut StudioPrefs) {
        if self.snapshot || !self.laid_out {
            return;
        }
        let kind = self.ws_kind;
        let Some(json) = workspaces::to_json(&self.docks[kind.index()]) else { return };
        if json == self.layout_default[kind.index()] {
            prefs.layouts.remove(kind.key());
        } else if prefs.layouts.get(kind.key()) != Some(&json) {
            prefs.layouts.insert(kind.key().to_string(), json);
        }
    }

    /// «Сбросить раскладку»: the shown workspace's default layout, and its saved one forgotten.
    fn reset_layout(&mut self, prefs: &mut StudioPrefs) {
        let kind = self.ws_kind;
        self.docks[kind.index()] = default_dock(kind, self.width, false);
        prefs.layouts.remove(kind.key());
    }

    /// Undo: one history for the whole studio (the Script's own undo does not run).
    fn undo(&mut self) {
        match self.ws.undo() {
            Ok(Some(label)) => {
                self.log.push(trf!("Undone: {label}", label));
                self.touch();
            }
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    fn redo(&mut self) {
        match self.ws.redo() {
            Ok(Some(label)) => {
                self.log.push(trf!("Redone: {label}", label));
                self.touch();
            }
            Ok(None) => {}
            Err(e) => self.log.push(e),
        }
    }

    /// Opens a text file for the Script tab and shows that tab. A map's file (`maps/<id>/…`)
    /// also makes its map the one the Map tab shows.
    fn open_file(&mut self, path: &str) {
        match self.ws.open_doc(path) {
            Ok(i) => {
                if path == "package.toml" {
                    self.scen.focus = Focus::Package;
                }
                self.selected = Some(i);
                self.opened = Some(path.to_string());
                if let (Some(pkg), Some(id)) = (self.ws.package(), map_of_file(path)) {
                    if pkg.maps.contains_key(id) {
                        switch_map(&mut self.maps, &mut self.canvas, &mut self.selection, id);
                        self.reload();
                    }
                }
                self.pending_tab = Some(Tab::Script);
            }
            Err(e) => self.log.push(e),
        }
    }

    fn save_selected(&mut self) {
        let Some(i) = self.selected.filter(|&i| i < self.ws.docs.len()) else { return };
        let path = self.ws.docs[i].path.clone();
        match self.ws.save(i) {
            Ok(()) => {
                self.log.push(trf!("Saved {path}", path));
                self.reload();
            }
            Err(e) => self.log.push(e),
        }
    }

    fn save_all(&mut self) {
        match self.ws.save_all() {
            Ok(()) => self.log.push(tr("All files saved.").to_string()),
            Err(e) => self.log.push(e),
        }
        self.reload();
    }

    /// The Check button: the rebuild now, and its counts logged when it comes back.
    fn check(&mut self) {
        self.check_asked = true;
        self.reload();
    }

    /// The question about a file changed on disk: Reload, Keep mine, or the diff first.
    fn conflict_ui(&mut self, ctx: &egui::Context) {
        let Some(c) = self.conflict.as_mut() else { return };
        let mut answer: Option<bool> = None;
        let title = trf!("{path} changed on disk", path = c.path);
        let mine = self.ws.docs.iter().find(|d| d.path == c.path).map(|d| d.text.clone()).unwrap_or_default();
        egui::Window::new(title).collapsible(false).resizable(true).default_width(560.0).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("The file changed on disk, and it has unsaved changes here."));
            ui.horizontal(|ui| {
                if ui.button(tr("Reload")).clicked() {
                    answer = Some(true);
                }
                if ui.button(tr("Keep mine")).clicked() {
                    answer = Some(false);
                }
                let toggle = if c.diff.is_some() { tr("Hide diff") } else { tr("Show diff") };
                if ui.button(toggle).clicked() {
                    c.diff = match c.diff {
                        Some(_) => None,
                        None => Some(diff_lines(&mine, &c.disk)),
                    };
                }
            });
            if let Some(lines) = &c.diff {
                ui.separator();
                ScrollArea::vertical().max_height(380.0).auto_shrink([false, true]).show(ui, |ui| {
                    for line in lines {
                        ui.add(egui::Label::new(diff_text(line)).wrap_mode(egui::TextWrapMode::Extend));
                    }
                });
            }
        });
        let Some(answer) = answer else { return };
        let Some(c) = self.conflict.take() else { return };
        let result = if answer { self.ws.take_disk(&c.path, &c.disk) } else { self.ws.keep_mine(&c.path, &c.disk) };
        match result {
            Ok(()) if answer => {
                self.log.push(trf!("{path} reloaded from disk (Ctrl+Z takes the old text back)", path = c.path));
                self.touch();
            }
            Ok(()) => self.log.push(trf!("{path} kept: the disk text is the saved one now", path = c.path)),
            Err(e) => self.log.push(e),
        }
    }

    /// The prompt on leaving with unsaved files. True when the studio should close.
    fn exit_prompt_ui(&mut self, ctx: &egui::Context) -> bool {
        let mut leave = false;
        egui::Window::new(tr("Unsaved changes")).collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            ui.label(tr("Save the changed files before leaving?"));
            ui.horizontal(|ui| {
                if ui.button(tr("Save all")).clicked() {
                    self.save_all();
                    self.exit_prompt = false;
                    leave = !self.ws.any_dirty();
                }
                if ui.button(tr("Discard")).clicked() {
                    leave = true;
                }
                if ui.button(tr("Cancel")).clicked() {
                    self.exit_prompt = false;
                }
            });
        });
        leave
    }
}

/// The lines of a unified diff, coloured: additions green, removals red, the hunk headers weak.
fn diff_text(line: &str) -> egui::RichText {
    let text = egui::RichText::new(line).monospace();
    match line.chars().next() {
        Some('+') if !line.starts_with("+++") => text.color(Color32::from_rgb(0x7f, 0xc8, 0x7f)),
        Some('-') if !line.starts_with("---") => text.color(ERROR_RED),
        _ => text.color(Color32::from_gray(0x9a)),
    }
}

/// The diff of the document's text (`mine`) and the disk text, as lines.
fn diff_lines(mine: &str, disk: &str) -> Vec<String> {
    unified(mine, disk).lines().map(str::to_string).collect()
}

/// The Script's jump to the table of `sel` in the map file `path`: the table's bytes selected and
/// its line. `None` when the selection has no table in the file.
pub fn jump_for(ws: &Workspace, pkg: Option<&Package>, path: &str, sel: &Selection) -> Option<Jump> {
    let spec = pkg?.maps.get(map_of_file(path)?)?.spec.as_ref()?;
    let loc = razdor::studio::map::locator_of(path, spec, sel)?;
    let text = ws.text_of(path).ok()?;
    let (_, range) = ws.spans(path).ok()?.into_iter().find(|(l, _)| *l == loc)?;
    Some(Jump { line: line_of(&text, range.start), range: Some(range) })
}

/// The byte offset where a 1-based line starts.
fn line_start(text: &str, line: u32) -> usize {
    text.split_inclusive('\n').take(line.saturating_sub(1) as usize).map(str::len).sum()
}

/// The character index of a byte offset (egui's cursors count characters).
fn char_index(text: &str, byte: usize) -> usize {
    text[..byte.min(text.len())].chars().count()
}

/// The default layout, as Godot 4's: FileSystem over Scene on the left, Map and Script in the
/// centre (Script shown), Inspector on the right, Problems and Output under the centre. `side`
/// puts the Script beside the Map instead of in its tabs (the snapshot's `side` option).
/// A frame of the chrome (the bars and the drawer): the panel fill and the margins, a 1 px line.
fn chrome_frame(x: i8, y: i8) -> egui::Frame {
    egui::Frame::NONE.fill(theme::PANEL).stroke(Stroke::new(1.0, theme::LINE)).inner_margin(Margin::symmetric(x, y))
}

/// A toolbar button: the command's icon, its tooltip (the name and the shortcut, `commands::tip`), a
/// note after it (the undo's label), greyed when disabled.
fn cmd_button(ui: &mut Ui, id: &str, keys: &commands::Prefs, enabled: bool, on: bool, note: &str) -> egui::Response {
    let c = commands::command(id).expect("a studio command");
    let mut tip = commands::tip(c, keys);
    if !note.is_empty() {
        tip.push(' ');
        tip.push_str(note);
    }
    let button = Button::new(RichText::new(c.icon).size(theme::ICON)).min_size(egui::vec2(theme::BTN_H, theme::BTN_H)).selected(on);
    ui.add_enabled(enabled, button).on_hover_text(tip)
}

/// The note of an undo or redo button: its label, in brackets, when there is one.
fn label_note(label: Option<&str>) -> String {
    label.map_or_else(String::new, |l| format!("({l})"))
}

/// The one-line hint of the Map tool, shown in the status bar.
fn tool_hint(tool: Tool) -> &'static str {
    match tool {
        Tool::Select => n_("Click selects an object, drag moves it"),
        Tool::Army => n_("Click places an army"),
        Tool::Point => n_("Click places a point"),
        Tool::Object => n_("Click places an object"),
        Tool::Building => n_("Click places a building"),
        Tool::Terrain => n_("Drag paints terrain"),
        Tool::Hills => n_("Drag paints hills"),
        Tool::Forests => n_("Drag paints forests"),
        Tool::Items => n_("Not in the package format yet"),
        Tool::Rect => n_("Drag takes a rectangle to copy"),
        Tool::Fill => n_("Click fills an area"),
        Tool::Marquee => n_("Drag takes a rectangle of cells"),
        Tool::Lasso => n_("Drag draws a lasso round cells"),
        Tool::Wand => n_("Click selects the cells of its terrain"),
        Tool::Eraser => n_("Drag erases plants, then hills"),
        Tool::Eyedrop => n_("Click takes the cell's terrain"),
        Tool::Stamp => n_("Click places the armed stamp"),
        Tool::Hand => n_("Drag pans the view"),
    }
}

/// The drawer's strip and, when open, its content: Проблемы (the problems list), Вывод (the log) or
/// Claude (the terminal). Its tabs are always shown; a click on the open tab closes the drawer.
fn drawer_panel(ui: &mut Ui, viewer: &mut Viewer<'_>, open: &mut bool, which: &mut Drawer, problems: usize) {
    ui.horizontal(|ui| {
        for d in Drawer::ALL {
            let label = match d {
                Drawer::Problems if problems > 0 => format!("{} {problems}", tr(d.name())),
                _ => tr(d.name()).to_string(),
            };
            let on = *open && *which == d;
            if ui.add(Button::new(RichText::new(label).text_style(egui::TextStyle::Small)).selected(on)).clicked() {
                if on {
                    *open = false;
                } else {
                    *which = d;
                    *open = true;
                }
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let c = commands::command("drawer").expect("the drawer command");
            let icon = if *open { ic::CARET_DOWN } else { ic::CARET_UP };
            let toggle = Button::new(RichText::new(icon).size(theme::ICON)).min_size(egui::vec2(theme::SMALL_BTN, theme::SMALL_BTN)).frame(false);
            if ui.add(toggle).on_hover_text(commands::tip(c, &commands::Prefs::new())).clicked() {
                *open = !*open;
            }
        });
    });
    if *open {
        ui.separator();
        match *which {
            Drawer::Problems => viewer.problems(ui),
            Drawer::Output => viewer.output(ui),
            Drawer::Claude => viewer.claude.show(ui, &viewer.ws.root),
        }
    }
}

/// The tabs' contents. Each tab is drawn from the same borrowed state; a file asked for (a click
/// in the tree or on a problem) is opened after the dock is drawn.
struct Viewer<'a> {
    /// The shortcuts the author set (the tooltips of the bar show them).
    keys: &'a commands::Prefs,
    ws: &'a mut Workspace,
    /// The package as last rebuilt (the screens read it; the model keeps the last good one).
    pkg: Option<Arc<Package>>,
    selected: &'a mut Option<usize>,
    log: &'a mut Vec<String>,
    /// The diagnostics of the failed compile (the Problems tab lists them after the checks').
    map_problems: &'a [Diagnostic],
    maps: &'a mut MapView,
    canvas: &'a mut Canvas,
    selection: &'a mut Option<Selection>,
    has_install: bool,
    /// The Script's next jump (taken by the Script tab when it is drawn).
    jump: &'a mut Option<Jump>,
    cursor_seen: &'a mut Option<(String, usize)>,
    claude: &'a mut claude::ClaudeDock,
    /// Requests for after the dock is drawn: a file to open, the Map tab to show.
    open: Option<String>,
    focus_map: bool,
    /// A document or the package changed in this frame (the rebuild and the burst timers start).
    changed: bool,
    /// The scenario screens' state.
    scen: &'a mut ScenarioUi,
    /// The game's install, for the records' prices (`None` without one).
    install: Option<Arc<DtInstall>>,
    /// The battle tester's state, and its request for after the dock (start, cancel, watch).
    battle: &'a mut battletest::BattleTest,
    battle_req: &'a mut Option<battletest::Req>,
    /// The scores of the shown map (`scores.rs`), read by the Scores tab.
    scores: &'a scores::State,
    /// A jump asked for by a view (a score line, an AI log entry): taken after the dock is drawn.
    goto: Option<nav::Destination>,
    /// The snapshot's `jump=score` flag (taken by the Оценка view).
    snap_jump: &'a mut bool,
}

impl Viewer<'_> {
    /// The battle tester's screen: its options come from the shown map (opened here, once per map).
    fn battle_tab(&mut self, ui: &mut Ui) {
        let ready = match (&self.pkg, &self.install, &self.maps.map) {
            (_, None, _) => Err(tr("The battle tester needs the game's files: set RAZDOR_DT_DIR")),
            (Some(_), Some(_), Some(_)) => Ok(()),
            _ => Err(tr("No map to show.")),
        };
        if let (Ok(()), Some(pkg), Some(install), Some(map)) = (ready, self.pkg.as_ref(), self.install.as_ref(), self.maps.map.clone()) {
            if self.battle.map.as_deref() != Some(map.as_str()) {
                match razdor::studio::testers::open_map(pkg, install, &map) {
                    Ok(mg) => self.battle.load(&mg, &map),
                    Err(r) => {
                        self.battle.map = Some(map.clone());
                        self.battle.error = Some(r.to_text().lines().next().unwrap_or_default().to_string());
                    }
                }
            }
        }
        if let Some(req) = self.battle.show(ui, ready) {
            *self.battle_req = Some(req);
        }
    }

    /// The Тест workspace's ИИ view (UX6): the AI run's controls, the map as a read-only canvas with the armies,
    /// their trails and the story's marks at the day the slider shows, and the day slider and the log under it.
    /// A click on a log line sets the day and jumps to that moment's object (`studio::nav::of_moment`).
    fn ai_tab(&mut self, ui: &mut Ui) {
        ui.heading(tr("AI viewer"));
        let ready = match (&self.pkg, &self.install, &self.maps.map) {
            (_, None, _) => Err(tr("The AI viewer needs the game's files: set RAZDOR_DT_DIR")),
            (Some(_), Some(_), Some(_)) => Ok(()),
            _ => Err(tr("No map to show.")),
        };
        match self.canvas.ai.controls(ui, ready) {
            aiview::Req::Start => {
                if let (Some(pkg), Some(install), Some(map)) = (self.pkg.clone(), self.install.clone(), self.maps.map.clone()) {
                    self.canvas.ai.start(pkg, install, &map);
                }
            }
            aiview::Req::Cancel => self.canvas.ai.cancel(),
            aiview::Req::None => {}
        }
        let recorded = self.canvas.ai.recording.is_some();
        if !recorded {
            ui.weak(tr("Run the AI for the armies and the story on the map."));
        }
        // The map takes the height the slider and the log leave.
        let under = if recorded { 200.0 } else { 0.0 };
        self.ai_canvas(ui, ui.available_height() - under);
        if recorded {
            if let Some(i) = self.canvas.ai.timeline(ui) {
                let moment = self.canvas.ai.recording.as_ref().and_then(|r| r.moments.get(i));
                let dest = match (&self.pkg, &self.maps.map, moment) {
                    (Some(pkg), Some(map), Some(moment)) => nav::of_moment(pkg, map, moment),
                    _ => None,
                };
                // The moment's cell is outlined on the canvas, as a score line's cells are.
                let cell = moment.and_then(|m| m.cell).map(|(x, y)| (i64::from(x), i64::from(y)));
                self.canvas.score_cells = cell.into_iter().collect();
                self.goto = dest;
            }
        }
    }
}

impl TabViewer for Viewer<'_> {
    type Tab = Tab;

    /// The tab's id is its fixed key, not its translated title: a saved layout survives a language change.
    fn id(&mut self, tab: &mut Tab) -> Id {
        Id::new(("studio-tab", tab.key()))
    }

    fn title(&mut self, tab: &mut Tab) -> WidgetText {
        tab.title().into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::FileSystem => self.file_system(ui),
            Tab::Scene => self.scene(ui),
            Tab::Map => self.map_tab(ui),
            Tab::Script => self.script(ui),
            Tab::Inspector => self.inspector(ui),
            Tab::Problems => self.problems(ui),
            Tab::Output => self.output(ui),
            Tab::Claude => self.claude.show(ui, &self.ws.root),
            Tab::Minimap => self.minimap(ui),
            Tab::Stamps => self.stamps_dock(ui),
            Tab::Palette => self.palette_dock(ui),
            Tab::Layers => self.layers_dock(ui),
            Tab::Campaign => self.story_centre(ui),
            Tab::Texts => self.texts_screen(ui),
            Tab::Characters => self.characters_dock(ui),
            Tab::Story => self.story_centre(ui),
            Tab::Outline => self.outline_panel(ui),
            Tab::DataNav => self.data_nav(ui),
            Tab::Data => self.data_centre(ui),
            Tab::Vars => self.vars_dock(ui),
            Tab::Units => self.units_screen(ui),
            Tab::Items => self.items_screen(ui),
            Tab::Battle => self.battle_tab(ui),
            Tab::AiTest => self.ai_tab(ui),
            Tab::Scores => self.scores_dock(ui),
        }
    }

    /// The map is drawn over the Map tab's body by the macroquad pass, so egui paints nothing there.
    fn clear_background(&self, tab: &Tab) -> bool {
        // The map, the minimap and the ИИ view's canvas are drawn by macroquad after egui: egui must not paint over them.
        !matches!(tab, Tab::Map | Tab::Minimap | Tab::AiTest)
    }
}

impl Viewer<'_> {
    fn file_system(&mut self, ui: &mut Ui) {
        ui.weak(tr("Package folder"));
        let tree = build_tree(&self.ws.entries);
        let open = &mut self.open;
        ScrollArea::both().auto_shrink([false, false]).show(ui, |ui| show_dir(ui, &tree, open));
    }

    /// Selects `sel` from the Scene or the canvas: the Script goes to its table when the Script
    /// shows the shown map's file.
    pub(super) fn select(&mut self, sel: Selection) {
        let shown = self.selected.and_then(|i| self.ws.docs.get(i)).map(|d| d.path.clone());
        if let Some(path) = shown.filter(|p| map_of_file(p) == self.maps.map.as_deref()) {
            *self.jump = jump_for(&*self.ws, self.pkg.as_deref(), &path, &sel);
        }
        *self.selection = Some(sel);
        self.scen.focus = Focus::Table;
    }

    fn script(&mut self, ui: &mut Ui) {
        if self.ws.docs.is_empty() {
            ui.weak(tr("No file open: pick a text file in the file system."));
            return;
        }
        ui.horizontal_wrapped(|ui| {
            for (i, doc) in self.ws.docs.iter().enumerate() {
                let name = file_name(&doc.path);
                let label = if doc.is_dirty() { format!("{name} (*)") } else { name.to_string() };
                if ui.selectable_label(*self.selected == Some(i), label).on_hover_text(&doc.path).clicked() {
                    *self.selected = Some(i);
                }
            }
        });
        ui.separator();
        let Some(i) = self.selected.filter(|&i| i < self.ws.docs.len()) else {
            ui.weak(tr("No file open: pick a text file in the file system."));
            return;
        };
        let path = self.ws.docs[i].path.clone();
        let before = self.ws.docs[i].text.clone();
        let size = ui.available_size();
        let id = ui.id().with("studio-script");
        let row = ui.text_style_height(&egui::TextStyle::Monospace);
        let mut area = ScrollArea::vertical().auto_shrink([false, false]);
        // A jump's scroll is kept for a few frames: egui clamps an offset to the text's height, which is known only
        // once the text is laid out (a file opened by a jump is new to the view).
        let scroll_id = Id::new("studio-script-scroll");
        if let Some(j) = self.jump.take() {
            // The caret goes to the table's start, the table is selected, and the view scrolls to
            // a few lines above it. A line alone puts the caret at its start.
            let text = &self.ws.docs[i].text;
            let (start, end) = match &j.range {
                Some(r) => (r.start, r.end),
                None => (line_start(text, j.line), line_start(text, j.line)),
            };
            let line = line_of(text, start);
            let mut state = TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
            state.cursor.set_char_range(Some(CCursorRange::two(CCursor::new(char_index(text, start)), CCursor::new(char_index(text, end)))));
            state.store(ui.ctx(), id);
            ui.memory_mut(|m| m.request_focus(id));
            ui.ctx().data_mut(|d| d.insert_temp(scroll_id, (line.saturating_sub(4) as f32 * row, 4u8)));
        }
        if let Some((at, frames)) = ui.ctx().data(|d| d.get_temp::<(f32, u8)>(scroll_id)) {
            area = area.vertical_scroll_offset(at);
            ui.ctx().data_mut(|d| if frames <= 1 { d.remove::<(f32, u8)>(scroll_id) } else { d.insert_temp(scroll_id, (at, frames - 1)) });
        }
        let resp = area
            .show(ui, |ui| {
                ui.add(TextEdit::multiline(&mut self.ws.docs[i].text).id(id).code_editor().desired_width(f32::INFINITY).min_size(size))
            })
            .inner;
        if resp.changed() {
            self.ws.begin_text_edit(&path, &before);
            self.changed = true;
        }
        // The caret moved: the table it is in selects its object (the shown map's file only).
        let Some(state) = TextEdit::load_state(ui.ctx(), id) else { return };
        let Some(range) = state.cursor.char_range() else { return };
        let at = range.primary.index;
        if self.cursor_seen.as_ref() == Some(&(path.clone(), at)) {
            return;
        }
        *self.cursor_seen = Some((path.clone(), at));
        if map_of_file(&path).is_some() && map_of_file(&path) == self.maps.map.as_deref() {
            let text = &self.ws.docs[i].text;
            let byte = text.char_indices().nth(at).map_or(text.len(), |(b, _)| b);
            let sel = self.ws.locator_at(&path, byte).and_then(|loc| selection_of(&loc));
            if let Some(sel) = sel {
                if self.selection.as_ref() != Some(&sel) {
                    *self.selection = Some(sel);
                }
            }
        }
    }

    fn problems(&mut self, ui: &mut Ui) {
        // The checks' problems, then the failed compile's (those the checks already listed are not repeated).
        let all: Vec<&Diagnostic> = self
            .ws
            .problems
            .iter()
            .chain(self.map_problems.iter().filter(|d| !self.ws.problems.contains(d)))
            .collect();
        if all.is_empty() {
            ui.weak(tr("No problems. Press Check to look again."));
            return;
        }
        let mut clicked: Option<Diagnostic> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            for (i, d) in all.iter().enumerate() {
                if problem_row(ui, i, d) && !d.file.is_empty() {
                    clicked = Some((*d).clone());
                }
            }
        });
        if let Some(d) = clicked {
            // A problem on a table of the shown map selects that table and centres the canvas on it;
            // the Script goes to the line either way.
            if let Some(map) = self.maps.map.clone() {
                let file = format!("maps/{map}/map.toml");
                if d.file == file {
                    let text = self.ws.text_of(&file).unwrap_or_default();
                    let spans = self.ws.spans(&file).unwrap_or_default();
                    if let Some(sel) = targets(&d, &file, &text, &spans).into_iter().next() {
                        let spec = self.pkg.as_deref().and_then(|p| p.maps.get(&map)).and_then(|m| m.spec.as_ref());
                        if let Some(c) = spec.and_then(|s| razdor::studio::map::centre_of(s, &sel)) {
                            self.canvas.centre = Some(c);
                        }
                        *self.selection = Some(sel);
                    }
                }
            }
            self.open = Some(d.file.clone());
            *self.jump = d.line.map(|line| Jump { line, range: None });
        }
    }

    fn output(&mut self, ui: &mut Ui) {
        if self.log.is_empty() {
            ui.weak(tr("Nothing yet."));
            return;
        }
        ScrollArea::vertical().stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            for line in self.log.iter() {
                ui.monospace(line);
            }
        });
    }
}

/// One problem: its severity, `file:line`, the message and the hint. Returns whether it was clicked.
fn problem_row(ui: &mut Ui, i: usize, d: &Diagnostic) -> bool {
    let (color, word) = match d.severity {
        Severity::Error => (ERROR_RED, tr("Error")),
        Severity::Warning => (WARNING_AMBER, tr("Warning")),
    };
    let location = match (d.file.is_empty(), d.line) {
        (true, _) => String::new(),
        (false, Some(line)) => format!("{}:{line}", d.file),
        (false, None) => d.file.clone(),
    };
    // The severity and the location on the first line; the message wraps under them (the
    // Problems tab is narrow), then the hint.
    let row = ui.vertical(|ui| {
        ui.horizontal(|ui| {
            ui.colored_label(color, word);
            if !location.is_empty() {
                ui.monospace(location);
            }
        });
        ui.label(d.message.as_str());
        if let Some(hint) = &d.hint {
            ui.weak(trf!("Hint: {hint}", hint));
        }
    });
    ui.add_space(2.0);
    ui.separator();
    let id = ui.id().with(("studio-problem", i));
    let response = ui.interact(row.response.rect, id, Sense::click());
    response.clicked()
}

/// The package's files as a folder tree.
#[derive(Default)]
struct Dir {
    dirs: BTreeMap<String, Dir>,
    /// Package-relative paths of the files in this folder.
    files: Vec<String>,
}

fn build_tree(entries: &[String]) -> Dir {
    let mut root = Dir::default();
    for path in entries {
        let mut dir = &mut root;
        let mut parts = path.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                dir = dir.dirs.entry(part.to_string()).or_default();
            } else {
                dir.files.push(path.clone());
            }
        }
    }
    root
}

/// Draws a folder: its subfolders as collapsible headers, then its files. Text files are
/// clickable (`open` gets the path); other files are shown as plain labels.
fn show_dir(ui: &mut Ui, dir: &Dir, open: &mut Option<String>) {
    for (name, sub) in &dir.dirs {
        egui::CollapsingHeader::new(name.as_str()).default_open(true).show(ui, |ui| show_dir(ui, sub, open));
    }
    for path in &dir.files {
        let name = file_name(path);
        if is_text(path) {
            if ui.selectable_label(false, name).clicked() {
                *open = Some(path.clone());
            }
        } else {
            ui.weak(name);
        }
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// `--studio [<package folder>]` on the command line: the folder to open. The example package
/// when none is named. `None` when the flag is not there.
pub fn requested() -> Option<Result<PathBuf, String>> {
    let args: Vec<String> = std::env::args().collect();
    let at = args.iter().position(|a| a == "--studio")?;
    let folder = args.get(at + 1).filter(|a| !a.starts_with("--"));
    Some(match folder {
        Some(f) => Ok(PathBuf::from(f)),
        None if Path::new(DEFAULT_PACKAGE).is_dir() => Ok(PathBuf::from(DEFAULT_PACKAGE)),
        None => Err(format!("--studio: name a package folder ({DEFAULT_PACKAGE} is not here)")),
    })
}
