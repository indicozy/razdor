//! The world map: terrain, objects, buildings, armies, the party, its clock and money.
//!
//! With a Discord Times install the original terrain textures, map objects, buildings and
//! map figures are drawn (decoded at runtime by [`DtArt`]); otherwise coloured cells and
//! simple shapes. Only the visible cells are drawn, so 200×200 maps stay fast.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::clock::duration_label;
use razdor::rules::content::HeroClass;
use razdor::rules::game::{Event, Foe, Game};
use razdor::rules::magic::{CastOutcome, CastTarget};
use razdor::rules::map::{object_class, Decoration, Grid, Tile, TileMap};
use razdor::rules::world::{Army, Location, LocationKind, Owner, Troop};

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::building_view::BuildingView;
use super::dialog::Dialog;
use super::dt_art::DtArt;
use super::game_bar::{self, BarButton, Look, TimeButton};
use super::minimap;
use super::saves::{self, Back, LoadView, SaveView};
use super::story;
use super::widgets::*;
use super::Screen;

/// Screen pixels per world unit (one cell width) at zoom 1 on the original's 1024×768
/// screen: its 32 px cells.
const PX: f32 = 32.0;

/// How much larger than the original's 1024×768 the screen is (the interface's scale,
/// `chrome::k`, is against the 960×720 footage): at zoom 1 the map shows as much ground as
/// the original's, about 32 cells across, whatever the window size.
fn map_scale() -> f32 {
    super::chrome::k() * 0.9375
}
/// Sail colour of the hero's own ship.
const HERO_SAIL: Color = Color::new(0.35, 0.8, 0.45, 1.0);
fn bar_h() -> f32 {
    super::chrome::bar_height()
}
/// The map view: the window above the bottom bar.
fn map_area() -> Rect {
    map_area_in(screen_width(), screen_height(), bar_h())
}
/// Never less than nothing: a minimized window on Windows is 1 pixel high, under the bar.
fn map_area_in(w: f32, h: f32, bar: f32) -> Rect {
    Rect::new(0.0, 0.0, w.max(0.0), (h - bar).max(0.0))
}
use super::dialog::MANA;

/// World-map view state kept between frames.
pub struct MapView {
    pub zoom: f32,
    /// The minimap window is open.
    pub minimap: bool,
    /// Razdor's debug overlay (F3): event points, lanterns and the events of each place.
    pub debug: bool,
    /// Where the camera looks when moved by the minimap (world units); `None` follows the hero.
    pub look: Option<(f32, f32)>,
    /// Places the scenario's events have shown (lanterns, shown armies), first in line: the
    /// camera flies to each in turn and its uncovered cells fade in from the fog.
    shows: VecDeque<Showing>,
    /// After an event's last shown place: when the camera set off back to the hero, and from
    /// where.
    returning: Option<(f64, (f32, f32))>,
    /// The fog opening around the hero at a map start (0x4af83c), over everything else.
    opening: Option<Showing>,
    /// The camera's glides to a spell's target and the spells' effects on the armies, in
    /// order (0x4afa98, 0x4af2f8).
    spell_fx: VecDeque<SpellFx>,
    /// The route a first click on the map shows, as in the original (interface.md §7.3): the
    /// spot clicked, the route, and where the hero stood. A second click on the same spot
    /// sets off; a move of the hero drops it.
    preview: Option<(Tile, Vec<Tile>, Tile)>,
    /// The clock (whole ms) at the last frame of the map: the scroll step is the time since
    /// then (0x4cc18f), so the first frame back from a window counts the window's time too,
    /// as in the original.
    last_frame_ms: Option<i64>,
    /// A right-button drag of the map (a Razdor extra), from its press until its release.
    grab: Option<Grab>,
    /// The building window that stepped aside for the flights of an event read in it: it
    /// comes back as it was once the camera is back on the hero.
    pub(super) back_to: Option<super::building_view::BuildingView>,
    /// The messages above the bar, oldest first, with when each came (a Razdor extra, the
    /// player's wish: they stack and fade out, [`TOAST_SECONDS`]).
    toasts: VecDeque<(String, f64)>,
    /// The view gliding back to the hero (the centre button or Tab, 0x4af96c): when it set
    /// off and from where. The map takes no input meanwhile.
    centring: Option<(f64, (f32, f32))>,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { zoom: 1.0, minimap: false, debug: false, look: None, shows: VecDeque::new(), returning: None, opening: None, spell_fx: VecDeque::new(), preview: None, last_frame_ms: None, back_to: None, centring: None, grab: None, toasts: VecDeque::new() }
    }
}

/// Pixels the mouse must move with the right button held before it drags the map.
const GRAB_START: f32 = 4.0;

/// A right-button press on the map: where the mouse and the camera (world units) were when
/// it went down, and whether it has moved past [`GRAB_START`] (then it drags the map).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Grab {
    start: Vec2,
    from: Vec2,
    moved: bool,
}

/// What a frame of a right-button press does ([`Grab::step`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum GrabStep {
    /// Held still: the original's tooltip shows.
    Held,
    /// Held and moved: the camera looks at this world point, the ground following the mouse.
    Drag(Vec2),
    /// Let go without moving: a right click (Razdor's stop).
    Click,
    /// Let go after a drag: the camera stays where it was dragged.
    Dropped,
}

impl Grab {
    /// The press with the pointer at `at` and the button `down` or not, at `scale` screen
    /// pixels per world unit.
    fn step(&mut self, at: Vec2, down: bool, scale: f32) -> GrabStep {
        if !down {
            return if self.moved { GrabStep::Dropped } else { GrabStep::Click };
        }
        let d = at - self.start;
        self.moved |= d.length() > GRAB_START;
        if self.moved {
            GrabStep::Drag(self.from - d / scale)
        } else {
            GrabStep::Held
        }
    }
}

/// The right button on the map (Razdor extras kept from before the parity pass): pressed,
/// held and moved, it drags the map with the mouse under a hand cursor, and the camera stays
/// there, as after the minimap, until a click on the map or Tab; let go without moving, it is
/// a right click, returned true (Razdor's stop). Held still it shows the original's tooltip
/// (`frame`). Not while the hero walks: the view is locked on him then.
fn grab_map(game: &Game, view: &mut MapView, cam: &Camera, off_map: bool) -> bool {
    use macroquad::miniquad::{window::set_mouse_cursor, CursorIcon};
    let m = Vec2::from(pointer());
    let Some(mut grab) = view.grab else {
        if right_clicked() && !off_map && cam.view.contains(m) && view.shows.is_empty() && !game.moving() {
            view.grab = Some(Grab { start: m, from: view.look.unwrap_or(game.display_pos()).into(), moved: false });
        }
        return false;
    };
    let down = is_mouse_button_down(MouseButton::Right) && !input_blocked();
    match grab.step(m, down, cam.scale) {
        GrabStep::Held => view.grab = Some(grab),
        GrabStep::Drag(at) => {
            set_mouse_cursor(CursorIcon::Pointer);
            view.look = Some(Camera::looking_at(game, view.zoom, at.into()).centre());
            view.grab = Some(grab);
        }
        GrabStep::Click => {
            view.grab = None;
            return !input_blocked();
        }
        GrabStep::Dropped => {
            view.grab = None;
            set_mouse_cursor(CursorIcon::Default);
        }
    }
    false
}

/// Razdor's stop (Space or a right click on the map, as before the parity pass): a walk is
/// cut at the step under way, as the original's own stop does, and a wait (the endless one
/// too) or a spell being read ends at once.
fn razdor_stop(game: &mut Game) {
    if game.moving() {
        game.cut_walk();
    } else if game.waiting() || game.endless_waiting() || game.reading().is_some() {
        game.stop();
    }
}

/// Whether `k` held while the hero walks cuts his walk (the original: any key). Razdor's keys
/// that only change the view, the sound or the help (zoom, the minimap, Tab, the music, F1,
/// F2) and the quick save leave him walking, as they did before the parity pass.
fn cuts_walk(k: KeyCode) -> bool {
    !matches!(
        k,
        KeyCode::Equal | KeyCode::Minus | KeyCode::KpAdd | KeyCode::KpSubtract | KeyCode::M | KeyCode::Tab | KeyCode::N | KeyCode::F1 | KeyCode::F2 | KeyCode::F5
    )
}

/// The original's scroll speed setting (`[Options] ScrollSpeed` of the interface ini,
/// 0–100); 100, the shipped value, when the install has none.
fn scroll_speed() -> i32 {
    super::chrome::ui_text("Options", "ScrollSpeed").map_or(100, |v| razdor::dt::ini::loose_int(&v))
}

/// The original's scroll step of a frame (interface.md §7.6, 0x4cc18f): `round(dt / F)` px
/// across and `round(dt × 0.6875 / F)` px down, with `F = (1 − ScrollSpeed/100) × 1.5 + 0.5`
/// and dt the milliseconds since the last frame, rounded half to even as the FPU does. In
/// the original's pixels (32 × 22 px cells).
fn scroll_step(speed: i32, dt_ms: f64) -> (f64, f64) {
    let f = (1.0 - speed as f64 / 100.0) * 1.5 + 0.5;
    ((dt_ms / f).round_ties_even(), (dt_ms * 0.6875 / f).round_ties_even())
}

/// Pixels (of the original's 1024 × 768 screen) from a screen edge where the mouse scrolls.
const EDGE: f32 = 5.0;

/// The original's scrolling of the idle map (interface.md §7.6): the held arrow key (only one
/// at a time: the original keeps only the last key down) and the mouse within 5 px of an
/// edge of the screen (the bar's lower edge included; a corner both ways) each move the
/// view by the frame's step. The camera then stays there until something moves it.
fn scroll(game: &Game, view: &mut MapView, dt_ms: i64) {
    if input_blocked() || !view.shows.is_empty() || view.minimap {
        return;
    }
    let mut dir = Vec2::ZERO;
    match held_key() {
        Some(KeyCode::Left) => dir.x -= 1.0,
        Some(KeyCode::Right) => dir.x += 1.0,
        Some(KeyCode::Up) => dir.y -= 1.0,
        Some(KeyCode::Down) => dir.y += 1.0,
        _ => {}
    }
    let (mx, my) = crate::ui::widgets::pointer();
    let (w, h) = (screen_width(), screen_height());
    let edge = EDGE * map_scale();
    if (0.0..=w).contains(&mx) && (0.0..=h).contains(&my) {
        dir.x += if mx < edge { -1.0 } else if mx > w - edge { 1.0 } else { 0.0 };
        dir.y += if my < edge { -1.0 } else if my > h - edge { 1.0 } else { 0.0 };
    }
    if dir == Vec2::ZERO {
        return;
    }
    let (sx, sy) = scroll_step(scroll_speed(), dt_ms as f64);
    // Original pixels to world units: a cell is 32 px across, its row 22 px down.
    let rh = game.world.map.grid.row_height();
    let step = dir * vec2(sx as f32 / PX, sy as f32 / 22.0 * rh) / view.zoom;
    let at = Vec2::from(view.look.unwrap_or(game.display_pos())) + step;
    // Where the camera can really look: at the map's border it stops, and so does the
    // scroll, so turning back moves the view at once.
    view.look = Some(Camera::looking_at(game, view.zoom, at.into()).centre());
}

/// Seconds the camera takes to reach a shown place, then the uncovered area takes to fade
/// in, then the view rests there before the next place.
const SHOW_PAN: f64 = 0.8;
const SHOW_FADE: f64 = 1.2;
const SHOW_REST: f64 = 0.4;

/// A place an event showed: the cells it uncovered stay dark until the camera is there
/// (after the event's message is read), then fade in.
struct Showing {
    /// World position of the place.
    at: (f32, f32),
    /// The event that showed it: shown once its window is closed.
    event: Option<u16>,
    /// The fog as it was before the place was uncovered (soft edges and all), one darkness
    /// per cell, and the texture it is drawn from: over the map until the reveal, so the
    /// place pops out of the dark without any hint before.
    before: Vec<u8>,
    mask: Option<Texture2D>,
    /// World distance from the place's centre to its farthest uncovered cell, plus the rim.
    reach: f32,
    /// When the camera set off, and from where.
    started: Option<(f64, (f32, f32))>,
    /// The radius shown, in world units.
    radius: f32,
}

/// Width of the soft rim of the opening circle, in world units (cells).
const SHOW_RIM: f32 = 2.5;

impl Showing {
    fn new(game: &Game, shown: &razdor::rules::game::Shown) -> Showing {
        let fog = &game.fog;
        let map = &game.world.map;
        let at = map.center(shown.at);
        let mut before = Vec::new();
        let mask = (!shown.cells.is_empty() && fog.w > 0 && fog.h > 0).then(|| {
            let fresh: std::collections::HashSet<Tile> = shown.cells.iter().copied().collect();
            before = minimap::darkness_of(fog.w, fog.h, |t| fog.explored(t) && !fresh.contains(&t));
            let rgba: Vec<u8> = before.iter().flat_map(|&a| [0, 0, 0, a]).collect();
            let t = Texture2D::from_rgba8(fog.w as u16, fog.h as u16, &rgba);
            t.set_filter(FilterMode::Linear);
            t
        });
        let reach = shown.cells.iter().map(|&t| (Vec2::from(map.center(t)) - Vec2::from(at)).length()).fold(0.0, f32::max) + SHOW_RIM + 1.0;
        Showing { at, event: shown.event, before, mask, reach, started: None, radius: shown.radius as f32 + 0.5 }
    }

    /// Its event's window is closed (a place shown with no window waits for all of them).
    fn free(&self, dialogs: &VecDeque<Dialog>) -> bool {
        shown_free(self.event, dialogs.iter().map(|d| d.event))
    }

    /// How far the reveal has come: 0 until the camera arrives, 1 when it is open.
    fn progress(&self, now: f64) -> f32 {
        match self.started {
            Some((t0, _)) => ((now - t0 - SHOW_PAN) / SHOW_FADE).clamp(0.0, 1.0) as f32,
            None => 0.0,
        }
    }
}

impl MapView {
    pub fn reset(&mut self) {
        self.look = None;
    }

    /// The map is left (a window, a battle): a right-button press under way ends with it, so
    /// the hand cursor does not stay over the next screen and the press is no right click
    /// (a stop) when the map comes back.
    pub fn drop_grab(&mut self) {
        if self.grab.take().is_some_and(|g| g.moved) {
            macroquad::miniquad::window::set_mouse_cursor(macroquad::miniquad::CursorIcon::Default);
        }
    }

    /// A game was loaded: the places the old one was about to show are dropped.
    pub fn forget_shows(&mut self) {
        self.shows.clear();
        self.returning = None;
        self.opening = None;
        self.spell_fx.clear();
        self.preview = None;
        self.back_to = None;
    }

    /// A map starts: the fog opens around the hero (0x4af83c), the places of the last game
    /// forgotten.
    pub fn open_around_hero(&mut self, game: &Game) {
        self.forget_shows();
        let (at, r) = (game.tile(), game.sight_radius() + 1);
        let cells = (at.1 - r..=at.1 + r).flat_map(|y| (at.0 - r..=at.0 + r).map(move |x| (x, y))).filter(|&t| game.fog.explored(t)).collect();
        let mut s = Showing::new(game, &razdor::rules::game::Shown { at, cells, radius: r, event: None });
        // No flight: the fog opens at once.
        s.started = Some((get_time() - SHOW_PAN, s.at));
        self.opening = Some(s);
    }

    /// The camera is on its way to an event's places (or back): the next window waits until
    /// it is there (the original queues the glides at the event's OK, before its next step).
    pub fn holds_dialogs(&self, dialogs: &VecDeque<Dialog>) -> bool {
        self.returning.is_some() || self.shows.front().is_some_and(|s| s.free(dialogs)) || !self.spell_fx.is_empty()
    }

    /// A place an event showed is due now (its window is closed), in `game.shown` or in
    /// line: a building window steps aside for its flight.
    pub fn shows_due(&self, game: &Game, dialogs: &VecDeque<Dialog>) -> bool {
        let due = |event: Option<u16>| shown_free(event, dialogs.iter().map(|d| d.event));
        self.shows.front().is_some_and(|s| due(s.event)) || game.shown.first().is_some_and(|s| due(s.event))
    }

    /// The camera is flying to the shown places or back to the hero.
    pub fn flying(&self) -> bool {
        !self.shows.is_empty() || self.returning.is_some()
    }
}

/// A place shown by `event` may be flown to: that event's window is no longer among the
/// open windows `open` (their events); a place shown with no event waits for every window.
fn shown_free(event: Option<u16>, mut open: impl Iterator<Item = Option<u16>>) -> bool {
    match event {
        Some(id) => !open.any(|e| e == Some(id)),
        None => open.next().is_none(),
    }
}

/// What the building window does about an event's flights (`App`): it steps aside for the
/// world map when a shown place is due, and comes back once the camera is back on the hero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum BuildingFlight {
    Stay,
    StepAside,
    ComeBack,
}

/// `in_building`: the building window is the screen; `aside`: it stepped aside for the
/// flights; `due`: a shown place is due; `flying`: the camera is on its way.
pub(super) fn building_flight(in_building: bool, aside: bool, due: bool, flying: bool) -> BuildingFlight {
    if in_building && due {
        BuildingFlight::StepAside
    } else if !in_building && aside && !flying {
        BuildingFlight::ComeBack
    } else {
        BuildingFlight::Stay
    }
}

impl MapView {
}

/// A world spell's part on the map: the camera's glide to the target army when it is more
/// than 300 px away (0x4afa98, 900 ms cosine), then the spell's effect over it (0x4af2f8).
#[derive(Clone, Copy, Debug)]
enum SpellFx {
    /// To this army (`None`: the hero's), from where the camera was when it set off.
    Look { army: Option<u32>, started: Option<(f64, (f32, f32))> },
    /// The effect `art` over this army (`None`: the hero's).
    Effect { army: Option<u32>, art: &'static str, started: Option<f64> },
}

/// The glide's length, and the spell effect's.
const GLIDE_SECS: f64 = 0.9;
/// The glide's length in the original's whole ms (0x4af96c).
const GLIDE_MS: i64 = 900;

/// How far a glide has come after `ms` whole ms, 0 to 1: the original's cosine ease in its
/// whole steps, `round(900 × (1 − cos(π t / 900)) / 2) / 900` (0x4af96c).
fn glide_ease(ms: i64) -> f32 {
    let t = ms.clamp(0, GLIDE_MS) as f64;
    let e = (GLIDE_MS as f64 * (1.0 - (std::f64::consts::PI * t / GLIDE_MS as f64).cos()) / 2.0).round();
    (e / GLIDE_MS as f64) as f32
}
const SPELL_FX_SECS: f64 = 0.8;

thread_local! {
    /// What the spell book and the events asked the map to show, taken by its next frame.
    static SPELL_FX: std::cell::RefCell<Vec<SpellFx>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The camera goes to `target` (a spell's): queued for the map.
pub(super) fn look_at(target: CastTarget) {
    let army = match target {
        CastTarget::Own => None,
        CastTarget::Army(uid) => Some(uid),
    };
    SPELL_FX.with(|q| q.borrow_mut().push(SpellFx::Look { army, started: None }));
}

/// Spell `s` lands on `target`: its effect over the army, queued for the map.
pub(super) fn spell_effect(s: &razdor::rules::content::SpellDef, target: CastTarget) {
    let (army, art) = match target {
        CastTarget::Own => (None, "Spells/S-Swirl.ugs"),
        CastTarget::Army(uid) => (
            Some(uid),
            match s.school {
                Some(razdor::rules::content::MagicSchool::Life) => "Spells/S-Light-Front.ugs",
                Some(razdor::rules::content::MagicSchool::Death) => "Spells/S-Fog.ugs",
                _ => "Spells/S-Fire.ugs",
            },
        ),
    };
    SPELL_FX.with(|q| q.borrow_mut().push(SpellFx::Effect { army, art, started: None }));
}

/// Where army `uid` (the hero for `None`) stands now, in world units.
fn army_pos(game: &Game, army: Option<u32>) -> Option<(f32, f32)> {
    match army {
        None => Some(game.display_pos()),
        Some(uid) => game.world.armies.iter().find(|a| a.uid == uid).map(|a| a.pos),
    }
}

/// The original's distance in its pixels between two world positions (0x4826f8): the larger
/// difference plus half the smaller.
fn glide_distance(game: &Game, a: (f32, f32), b: (f32, f32)) -> f32 {
    let rh = game.world.map.grid.row_height();
    let dx = ((a.0 - b.0) * PX).abs();
    let dy = ((a.1 - b.1) / rh * 22.0).abs();
    dx.max(dy) + dx.min(dy) / 2.0
}

/// Plays the front of the spells' queue: the camera's glide, then the effect.
fn play_spell_fx(game: &Game, view: &mut MapView, now: f64) {
    SPELL_FX.with(|q| view.spell_fx.extend(q.borrow_mut().drain(..)));
    let here = view.look.unwrap_or(game.display_pos());
    let Some(front) = view.spell_fx.front_mut() else { return };
    let done = match front {
        SpellFx::Look { army, started } => match army_pos(game, *army) {
            None => true,
            Some(to) => {
                let (t0, from) = match started {
                    Some(s) => *s,
                    // Close enough: no glide (0x4afa98).
                    None if glide_distance(game, here, to) <= 300.0 => {
                        view.spell_fx.pop_front();
                        return;
                    }
                    None => *started.insert((now, here)),
                };
                let p = ((now - t0) / GLIDE_SECS).clamp(0.0, 1.0);
                let e = ((1.0 - (std::f64::consts::PI * p).cos()) / 2.0) as f32;
                view.look = Some((from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e));
                if p >= 1.0 && army.is_none() {
                    view.look = None;
                }
                p >= 1.0
            }
        },
        SpellFx::Effect { started, .. } => now - *started.get_or_insert(now) >= SPELL_FX_SECS,
    };
    if done {
        view.spell_fx.pop_front();
    }
}

fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::from_rgba(r, g, b, 255)
}

/// Placeholder colour of a terrain code (our own palette).
pub(super) fn surface_color(code: u8) -> Color {
    match code {
        0 => rgb(120, 170, 200),
        1 => rgb(60, 110, 180),
        2 => rgb(28, 60, 130),
        3 => rgb(190, 70, 30),
        4 => rgb(176, 150, 104),
        5 => rgb(96, 146, 64),
        6 => rgb(112, 160, 70),
        7 => rgb(176, 170, 100),
        8 => rgb(86, 112, 80),
        9 => rgb(52, 70, 52),
        10 => rgb(222, 204, 140),
        11 => rgb(160, 116, 80),
        12 => rgb(140, 132, 122),
        13 => rgb(92, 66, 54),
        14 => rgb(230, 234, 240),
        _ => rgb(250, 250, 255),
    }
}

/// Faction colour (player green, ally blue, neighbour yellow, enemy red, as the editor).
pub(super) fn faction_color(faction: u8) -> Color {
    match faction {
        1 => rgb(70, 200, 90),
        2 => rgb(70, 130, 230),
        3 => rgb(230, 200, 60),
        4 => rgb(220, 60, 50),
        _ => rgb(200, 200, 200),
    }
}

struct Camera {
    /// World pixel (at this zoom) at the top-left of the map view.
    origin: Vec2,
    view: Rect,
    /// Screen pixels per world unit.
    scale: f32,
    grid: Grid,
}

impl Camera {
    /// Centred on world position `at` (clamped to the map), in the map view left of the
    /// side panel.
    fn looking_at(game: &Game, zoom: f32, at: (f32, f32)) -> Camera {
        Camera::looking_in(game, zoom, at, map_area())
    }

    fn looking_in(game: &Game, zoom: f32, at: (f32, f32), view: Rect) -> Camera {
        let map = &game.world.map;
        let scale = PX * zoom * map_scale();
        let rh = map.grid.row_height();
        // The map's extent is its fog grid's: half a cell either side of the cell centres.
        // Odd rows of a staggered map reach half a cell further right; that overhang stays
        // out of view.
        let world = vec2((map.w as f32) * scale, (map.h as f32) * rh * scale);
        let pad = vec2(0.5 * scale, 0.5 * rh * scale);
        let centre = Vec2::from(at) * scale + pad;
        let mut origin = centre - vec2(view.w, view.h) / 2.0;
        origin.x = origin.x.clamp(0.0, (world.x - view.w).max(0.0));
        origin.y = origin.y.clamp(0.0, (world.y - view.h).max(0.0));
        Camera { origin: origin - pad, view, scale, grid: map.grid }
    }

    /// The world point at the middle of the view (where the camera really looks, the map's
    /// border taken into account).
    fn centre(&self) -> (f32, f32) {
        ((self.origin + vec2(self.view.w, self.view.h) / 2.0) / self.scale).into()
    }

    /// Screen position of a world-space point.
    fn to_screen(&self, p: (f32, f32)) -> Vec2 {
        Vec2::from(p) * self.scale - self.origin + vec2(self.view.x, self.view.y)
    }

    fn cell_centre(&self, t: Tile) -> Vec2 {
        self.to_screen(self.grid.center(t))
    }

    /// Cell size on screen.
    fn cell_size(&self) -> Vec2 {
        vec2(self.scale, self.scale * self.grid.row_height())
    }

    /// The part of the world in view, in world units.
    fn world_rect(&self) -> Rect {
        let o = self.origin / self.scale;
        Rect::new(o.x, o.y, self.view.w / self.scale, self.view.h / self.scale)
    }

    /// Screen corners of the fog grid over the whole map.
    fn fog_corners(&self, game: &Game) -> (Vec2, Vec2) {
        let map = &game.world.map;
        let rh = map.grid.row_height();
        (self.to_screen((-0.5, -0.5 * rh)), self.to_screen((map.w as f32 - 0.5, (map.h as f32 - 0.5) * rh)))
    }

    /// The fog layer over the whole map (`ui::minimap`), and black beyond the map's limits:
    /// nothing drawn past its edges shows (a map smaller than the window, the overhang of
    /// staggered rows).
    fn draw_fog(&self, game: &Game) {
        let (tl, br) = self.fog_corners(game);
        minimap::draw_fog(&game.fog, tl, br);
        let v = self.view;
        let (l, r) = (tl.x.clamp(v.x, v.x + v.w), br.x.clamp(v.x, v.x + v.w));
        let (t, b) = (tl.y.clamp(v.y, v.y + v.h), br.y.clamp(v.y, v.y + v.h));
        for (x, y, w, h) in [(v.x, v.y, l - v.x, v.h), (r, v.y, v.x + v.w - r, v.h), (l, v.y, r - l, t - v.y), (l, b, r - l, v.y + v.h - b)] {
            if w > 0.0 && h > 0.0 {
                draw_rectangle(x, y, w, h, BLACK);
            }
        }
    }

    /// The fog still over places being shown, as dark as each one's fade has left it.
    /// The fog kept over places being shown. The one being revealed opens like an iris: a
    /// circle from its centre out to its edges, with a soft rim, clears the old fog.
    /// Razdor's mark of a place an event shows that was explored already (nothing opens
    /// there): a pulsing red circle of the radius shown, once the camera has arrived.
    fn draw_known_place(&self, s: &Showing, now: f64) {
        let Some((t0, _)) = s.started else { return };
        if s.mask.is_some() {
            return;
        }
        let t = now - t0 - SHOW_PAN;
        if t < 0.0 {
            return;
        }
        let fade = (t / 0.3).min(1.0) as f32;
        let pulse = 0.75 + 0.25 * (t * std::f64::consts::TAU * 1.5).sin() as f32;
        let c = self.to_screen(s.at);
        // The radius in cells: rows are closer together on screen than columns.
        let (rx, ry) = (s.radius * self.scale, s.radius * self.scale * self.grid.row_height());
        let red = Color::new(0.95, 0.15, 0.1, 0.9 * fade * pulse);
        let ring = |width: f32, colour: Color| {
            let n = 72;
            let at = |i: usize| {
                let a = i as f32 / n as f32 * std::f32::consts::TAU;
                c + vec2(rx * a.cos(), ry * a.sin())
            };
            for i in 0..n {
                let (p, q) = (at(i), at(i + 1));
                draw_line(p.x, p.y, q.x, q.y, width, colour);
            }
        };
        ring(4.0, Color::new(0.0, 0.0, 0.0, 0.5 * fade));
        ring(2.5, red);
    }

    fn draw_showing<'s>(&self, game: &Game, shows: impl IntoIterator<Item = &'s Showing>, now: f64) {
        let (tl, br) = self.fog_corners(game);
        let map = &game.world.map;
        let (w, h) = (game.fog.w, game.fog.h);
        for s in shows {
            let Some(mask) = &s.mask else { continue };
            let p = s.progress(now);
            if p >= 1.0 {
                continue;
            }
            if p > 0.0 {
                // Ease out: quick at first, slowing as it reaches the edges.
                let r = (1.0 - (1.0 - p) * (1.0 - p)) * s.reach;
                let centre = Vec2::from(s.at);
                let mut rgba = vec![0u8; s.before.len() * 4];
                for y in 0..h {
                    for x in 0..w {
                        let i = (y * w + x) as usize;
                        let d = (Vec2::from(map.center((x, y))) - centre).length();
                        let keep = ((d - r) / SHOW_RIM + 0.5).clamp(0.0, 1.0);
                        rgba[i * 4 + 3] = (s.before[i] as f32 * keep) as u8;
                    }
                }
                mask.update(&Image { bytes: rgba, width: w as u16, height: h as u16 });
            }
            draw_texture_ex(mask, tl.x, tl.y, WHITE, DrawTextureParams { dest_size: Some(br - tl), ..Default::default() });
        }
    }

    fn tile_under_mouse(&self) -> Option<Tile> {
        let m = Vec2::from(crate::ui::widgets::pointer());
        if !self.view.contains(m) {
            return None;
        }
        let w = (m - vec2(self.view.x, self.view.y) + self.origin) / self.scale;
        Some(self.grid.tile_at((w.x, w.y)))
    }

    /// Visible cell ranges (cols, rows), half-open, clamped to the map.
    fn visible(&self, map: &TileMap) -> ((i32, i32), (i32, i32)) {
        let top_left = self.origin / self.scale;
        let rh = self.grid.row_height();
        let c0 = (top_left.x - 1.0).floor() as i32;
        let r0 = (top_left.y / rh - 1.0).floor() as i32;
        let c1 = c0 + (self.view.w / self.scale) as i32 + 3;
        let r1 = r0 + (self.view.h / (self.scale * rh)) as i32 + 3;
        ((c0.max(0), c1.min(map.w)), (r0.max(0), r1.min(map.h)))
    }
}

/// Draws `tex` into `dest`, sampling it from world pixel `src` (unscaled) with wrap-around,
/// so neighbouring cells continue the texture seamlessly.
pub(super) fn draw_wrapped(tex: &Texture2D, dest: Rect, src: Vec2, src_size: Vec2) {
    let (tw, th) = (tex.width(), tex.height());
    let u0 = src.x.rem_euclid(tw);
    let v0 = src.y.rem_euclid(th);
    let k = vec2(dest.w / src_size.x, dest.h / src_size.y);
    let mut v = v0;
    let mut dy = 0.0;
    while dy < src_size.y - 0.01 {
        let hgt = (th - v).min(src_size.y - dy);
        let mut u = u0;
        let mut dx = 0.0;
        while dx < src_size.x - 0.01 {
            let wid = (tw - u).min(src_size.x - dx);
            draw_texture_ex(
                tex,
                dest.x + dx * k.x,
                dest.y + dy * k.y,
                WHITE,
                DrawTextureParams {
                    dest_size: Some(vec2(wid * k.x + 0.6, hgt * k.y + 0.6)),
                    source: Some(Rect::new(u, v, wid, hgt)),
                    ..Default::default()
                },
            );
            dx += wid;
            u = 0.0;
        }
        dy += hgt;
        v = 0.0;
    }
}

fn draw_terrain(game: &Game, art: Option<&DtArt>, cam: &Camera) {
    let map = &game.world.map;
    if let Some(layer) = art.and_then(|a| a.terrain_layer()).filter(|_| cam.grid == Grid::Square8) {
        // The map area in view, in world units (cells span ±½ around their centres).
        let rh = cam.grid.row_height();
        let (map_tl, map_br) = (vec2(-0.5, -0.5 * rh), vec2(map.w as f32 - 0.5, (map.h as f32 - 0.5) * rh));
        let w = cam.world_rect();
        let (tl, br) = (map_tl.max(w.point()), map_br.min(w.point() + w.size()));
        if tl.x < br.x && tl.y < br.y {
            let (a, b) = (cam.to_screen(tl.into()), cam.to_screen(br.into()));
            let view = vec4(tl.x, tl.y / rh, br.x, br.y / rh);
            layer.draw(map.surface_codes(), (map.w as u32, map.h as u32), Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), view, vec2(PX, PX * rh));
        }
        return;
    }
    let ((c0, c1), (r0, r1)) = cam.visible(map);
    let size = cam.cell_size();
    let unscaled = vec2(PX, PX * cam.grid.row_height());
    // One pass per terrain code keeps texture switches (and draw calls) few.
    let mut present = [false; 16];
    for y in r0..r1 {
        for x in c0..c1 {
            present[(map.surface_code((x, y)) & 15) as usize] = true;
        }
    }
    for code in (0..16u8).filter(|c| present[*c as usize]) {
        let tex = art.and_then(|a| a.terrain(code));
        for y in r0..r1 {
            for x in c0..c1 {
                if map.surface_code((x, y)) != code {
                    continue;
                }
                let c = cam.cell_centre((x, y));
                let dest = Rect::new(c.x - size.x / 2.0, c.y - size.y / 2.0, size.x, size.y);
                match &tex {
                    Some(tex) => {
                        // Texture space is world space: cell (x, y) samples at its own pixels.
                        let (wx, wy) = cam.grid.center((x, y));
                        draw_wrapped(tex, dest, vec2(wx * PX, wy * PX), unscaled);
                    }
                    None => draw_rectangle(dest.x, dest.y, dest.w + 0.5, dest.h + 0.5, surface_color(code)),
                }
            }
        }
    }
}

/// Something drawn in painter's order (by the bottom edge on screen).
enum Drawable {
    Object(Decoration),
    Building(usize),
    Army(usize),
    /// The hero's ship, waiting where he left it.
    Ship,
    Hero,
}

/// Bottom-centre of a location's footprint, in world units.
fn footprint_base(grid: Grid, l: &Location) -> (f32, f32) {
    let rh = grid.row_height();
    let (x, y) = grid.center(l.anchor);
    (x - (l.size.0 - 1) as f32 / 2.0, y + rh / 2.0)
}

/// The offsets the original adds to the sprites of picture types 13 (stone road and bridge
/// pieces) and 14 (wooden bridges), in pixels, by variant: the exe's table at 0x4ecd88
/// (`type·0x50 + variant·8`, two floats), read by 0x4c9b5b for picture types ≥ 13 only.
const BRIDGE_OFFSETS: [[(f32, f32); 10]; 2] = [
    [(2.0, 27.0), (11.0, 22.0), (0.0, 22.0), (11.0, 22.0), (11.0, 26.0), (1.0, 22.0), (13.0, 23.0), (18.0, 27.0), (22.0, 22.0), (16.0, 24.0)],
    [(11.0, 24.0), (10.0, 25.0), (11.0, 24.0), (10.0, 24.0), (8.0, 22.0), (9.0, 27.0), (9.0, 23.0), (9.0, 32.0), (35.0, 21.0), (36.0, 34.0)],
];

/// Screen top-left of a building's `w`×`h` px sprite, placed as the original places it: the
/// loader 0x4ce30c puts the quad at `(32·sx − w, 22·sy − h)` from the footprint's top-left
/// cell, 11 px higher when the footprint is wider than tall, less half a pixel, and 0x4c9b5b
/// adds [`BRIDGE_OFFSETS`] to the bridges. So the sprite's bottom-right corner is the anchor
/// cell's.
fn building_sprite_origin(l: &Location, w: f32, h: f32, cam: &Camera) -> Vec2 {
    let (x, y) = cam.grid.center(l.anchor);
    let corner = cam.to_screen((x + 0.5, y + cam.grid.row_height() / 2.0));
    let raise = if l.size.0 > l.size.1 { 11.0 } else { 0.0 };
    let (dx, dy) = bridge_offset(l.picture);
    corner + vec2(dx - w - 0.5, dy - h - raise - 0.5) * (cam.scale / PX)
}

/// [`BRIDGE_OFFSETS`] of a building picture (type, variant): nothing for other pictures.
pub(super) fn bridge_offset(picture: (u8, u8)) -> (f32, f32) {
    match picture {
        (t @ 13..=14, v) => BRIDGE_OFFSETS[usize::from(t - 13)].get(usize::from(v)).copied().unwrap_or_default(),
        _ => (0.0, 0.0),
    }
}

/// Screen top-left of a map object's `w`×`h` px sprite, placed as the original places it: the
/// loader 0x4ce30c puts the quad of classes 1–8 (hills, mountains, rocks) at `(48 − w, 33 − h)`
/// from the cell's top-left, its bottom-right corner 16 px right of and 11 px below the cell's,
/// and that of the plants at `(32 − w, 22 − h)`, less half a pixel; the renderer 0x4c8864 then
/// moves the plants of classes 9–11 by their jitter ([`razdor::rules::rng::plant_offset`]).
fn object_sprite_origin(o: &Decoration, w: f32, h: f32, cam: &Camera) -> Vec2 {
    let (x, y) = cam.grid.center(o.tile);
    let corner = cam.to_screen((x + 0.5, y + cam.grid.row_height() / 2.0));
    let (dx, dy) = match o.class {
        1..=8 => (16.0, 11.0),
        object_class::TREES..=object_class::THICKET => {
            let (dx, dy) = razdor::rules::rng::plant_offset(o.tile.0, o.tile.1, u16::from(o.class) << 8 | u16::from(o.sprite));
            (dx as f32, dy as f32)
        }
        _ => (0.0, 0.0),
    };
    corner + vec2(dx - w - 0.5, dy - h - 0.5) * (cam.scale / PX)
}

fn draw_object(o: &Decoration, art: Option<&DtArt>, cam: &Camera) {
    let c = cam.cell_centre(o.tile);
    let base = vec2(c.x, c.y + cam.cell_size().y / 2.0);
    let zoom = cam.scale / PX;
    if let Some((atlas, r)) = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.decoration(o.class, o.sprite)?))) {
        let at = object_sprite_origin(o, r.w, r.h, cam);
        draw_texture_ex(&atlas.texture, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w * zoom, r.h * zoom)), source: Some(r), ..Default::default() });
        return;
    }
    let s = cam.scale;
    use object_class::*;
    match o.class {
        TREES | DEAD_TREES => {
            let col = if o.class == TREES { rgb(40, 100, 44) } else { rgb(110, 90, 50) };
            draw_triangle(vec2(base.x, base.y - s * 1.1), vec2(base.x - s * 0.35, base.y - s * 0.1), vec2(base.x + s * 0.35, base.y - s * 0.1), col);
            draw_line(base.x, base.y, base.x, base.y - s * 0.15, 2.0, rgb(80, 60, 40));
        }
        THICKET => draw_circle(base.x, base.y - s * 0.4, s * 0.42, rgb(24, 64, 30)),
        MOUNTAINS | DARK_MOUNTAINS => {
            let k = 1.0 + (o.sprite / 10) as f32 * 0.5;
            let top = vec2(base.x, base.y - s * 0.9 * k);
            draw_triangle(top, vec2(base.x - s * 0.6 * k, base.y), vec2(base.x + s * 0.6 * k, base.y), rgb(120, 116, 112));
            draw_triangle(top, vec2(top.x - s * 0.15 * k, top.y + s * 0.2 * k), vec2(top.x + s * 0.15 * k, top.y + s * 0.2 * k), rgb(235, 235, 240));
        }
        ROCKS => draw_circle(base.x, base.y - s * 0.2, s * 0.25, rgb(150, 150, 150)),
        _ => {
            let k = 1.0 + (o.sprite / 10) as f32 * 0.4;
            draw_ellipse(base.x, base.y - s * 0.2 * k, s * 0.55 * k, s * 0.25 * k, 0.0, rgb(120, 140, 70));
        }
    }
}

fn draw_building(l: &Location, art: Option<&DtArt>, cam: &Camera) {
    let base = cam.to_screen(footprint_base(cam.grid, l));
    let zoom = cam.scale / PX;
    let sprite = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.building(l.picture.0, l.picture.1)?)));
    // The original draws just the sprite (RenderWorld_BuildingAndRest 0x4c9b5b), its owner
    // shown only on the minimap.
    if let Some((atlas, r)) = sprite {
        let at = building_sprite_origin(l, r.w, r.h, cam);
        draw_texture_ex(&atlas.texture, at.x, at.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w * zoom, r.h * zoom)), source: Some(r), ..Default::default() });
    } else {
        let (w, h) = (l.size.0 as f32 * cam.scale, (l.size.1 as f32 * cam.cell_size().y).max(cam.scale * 0.8));
        let wall = match l.kind {
            LocationKind::Castle | LocationKind::Fort | LocationKind::Palace => rgb(196, 192, 184),
            LocationKind::Ruins | LocationKind::Camp => rgb(110, 100, 90),
            LocationKind::StoneBridge => rgb(150, 145, 140),
            LocationKind::WoodenBridge => rgb(140, 100, 60),
            LocationKind::Church | LocationKind::Altar | LocationKind::Obelisk => rgb(236, 232, 224),
            _ => rgb(206, 180, 140),
        };
        draw_rectangle(base.x - w / 2.0, base.y - h, w, h, wall);
        draw_rectangle_lines(base.x - w / 2.0, base.y - h, w, h, 1.5, rgb(70, 60, 50));
        if !l.kind.is_bridge() {
            let roof = if l.kind == LocationKind::Camp && l.cleared { rgb(60, 56, 50) } else { rgb(170, 64, 48) };
            draw_triangle(vec2(base.x, base.y - h - h * 0.5), vec2(base.x - w / 2.0, base.y - h), vec2(base.x + w / 2.0, base.y - h), roof);
            // The first letter (a char: Russian letters take two bytes).
            let letter: String = l.kind.label().chars().take(1).collect();
            text_centered(&letter, base.x, base.y - h * 0.3, (h * 0.6).clamp(10.0, 30.0), BLACK);
        }
    }
}

/// The game's 13 map figures (`Graphics/Units/*.ugs`), by an army's figure (army +0x169d,
/// [`razdor::rules::world::Army::figure`]): the names at 0x4ed238, loaded in this order by
/// 0x4ce30c and drawn by 0x4ad314 (`0x71c430 + figure·0x2c00`).
pub(super) const FIGURES: [&str; 13] = [
    "Hero-Knight",
    "Hero-Mage",
    "Hero-Ranger",
    "Hero-Ship-Vesla",
    "Rogue",
    "Peasant",
    "Knight",
    "Necromant",
    "Zombie",
    "Ghost",
    "Mage",
    "Ship-Merchant",
    "Ship-Pirat",
];

/// `Graphics/Units/*.ugs` figure for the editor's army picture (`.DTm` byte 5) or the hero's
/// class (1–3). The game draws an army by its own figure ([`FIGURES`]), not by byte 5.
pub(super) fn figure_stem(model: u8) -> &'static str {
    match model {
        1 => "Hero-Knight",
        2 => "Hero-Mage",
        3 => "Hero-Ranger",
        5 => "Rogue",
        6 => "Peasant",
        10 => "Necromant",
        11 => "Ghost",
        12 => "Zombie",
        _ => "Knight",
    }
}

/// Sheet row for a heading (screen dx, dy): rows run clockwise from north-west.
fn facing_row(d: Vec2) -> f32 {
    if d.length_squared() < 1e-6 {
        return 5.0; // facing the viewer
    }
    let dir = ((d.x.atan2(-d.y) / std::f32::consts::FRAC_PI_4).round() as i32).rem_euclid(8);
    ((dir + 1) % 8) as f32
}

/// The original's ship sprites (`Graphics/Units`) by ship type (army byte 72,
/// `rules::ships::kind`): the hero's galley, pirates and merchants.
fn ship_stem(kind: u8) -> &'static str {
    match kind {
        razdor::rules::ships::kind::PIRATE => "Ship-Pirat",
        razdor::rules::ships::kind::MERCHANT => "Ship-Merchant",
        _ => "Hero-Ship-Vesla",
    }
}

/// How a sprite stands on its point: a figure's feet near the frame's bottom, with a
/// shadow; a ship's waterline across the frame's middle (its reflection is in the art).
#[derive(Clone, Copy)]
enum Stand {
    Feet,
    Afloat,
}

/// Draws a map figure (or ship) at world `pos`, heading towards `next`, drawn at one
/// sprite pixel per map pixel whatever its frame size. Returns false if the install has no
/// such sprite.
/// The hero's walk frame: 8 frames at 10 a second while he has a step to take, else 0.
fn hero_frame(next: Option<(f32, f32)>) -> u32 {
    if next.is_some() {
        (get_time() * 10.0) as u32 % 8
    } else {
        0
    }
}

fn draw_figure(art: Option<&DtArt>, stem: &str, pos: (f32, f32), next: Option<(f32, f32)>, frame: u32, cam: &Camera, stand: Stand) -> bool {
    let Some(sheet) = art.and_then(|a| a.figure_sheet(stem)) else { return false };
    let n = sheet.width() / 8.0;
    let p = cam.to_screen(pos);
    let heading = next.map_or(Vec2::ZERO, |n| cam.to_screen(n) - p);
    let row = facing_row(heading);
    let frame = frame.min(7) as f32;
    let size = n * cam.scale / PX;
    let dest = match stand {
        // The figure's own alpha holds its shadow (the .ugs frames); nothing is added under it.
        Stand::Feet => vec2(p.x - size / 2.0, p.y - size * 0.8),
        Stand::Afloat => vec2(p.x - size / 2.0, p.y - size * 0.55),
    };
    draw_texture_ex(
        &sheet,
        dest.x,
        dest.y,
        WHITE,
        DrawTextureParams { dest_size: Some(vec2(size, size)), source: Some(Rect::new(frame * n, row * n, n, n)), ..Default::default() },
    );
    true
}

/// A ship on the water without the install's sprites (a placeholder shape: hull, mast and a
/// sail of `sail` colour).
fn draw_ship(cam: &Camera, pos: (f32, f32), sail: Color) {
    let c = cam.to_screen(pos);
    let k = cam.scale / PX;
    let (hw, hh) = (18.0 * k, 7.0 * k);
    let hull = Color::new(0.42, 0.26, 0.12, 1.0);
    let (top, bottom) = (c.y - hh * 0.2, c.y + hh);
    draw_ellipse(c.x, bottom + 2.0 * k, hw * 1.1, 4.0 * k, 0.0, Color::new(0.0, 0.1, 0.2, 0.35));
    draw_rectangle(c.x - hw * 0.7, top, hw * 1.4, bottom - top, hull);
    draw_triangle(vec2(c.x - hw, top), vec2(c.x - hw * 0.7, top), vec2(c.x - hw * 0.7, bottom), hull);
    draw_triangle(vec2(c.x + hw, top), vec2(c.x + hw * 0.7, top), vec2(c.x + hw * 0.7, bottom), hull);
    draw_line(c.x, top, c.x, top - 30.0 * k, 2.0 * k.max(0.5), Color::new(0.3, 0.2, 0.1, 1.0));
    draw_triangle(vec2(c.x + 1.0, top - 28.0 * k), vec2(c.x + 1.0, top - 6.0 * k), vec2(c.x + 16.0 * k, top - 8.0 * k), sail);
}

/// The mark under the hero (yellow) and under an army (red), as the original's screen
/// shows them.
const MARK_HERO: Color = Color::new(1.0, 0.92, 0.1, 1.0);
const MARK_ARMY: Color = Color::new(0.95, 0.15, 0.1, 1.0);

/// A thin dashed ring on the ground at `pos` (the original's `Selection-*.lit` marks,
/// 0x4ce30c), the ring's light added to the ground (its blend
/// 0x4c9459, SRCCOLOR / INVSRCCOLOR). Without the picture, a thin ellipse. *(The original
/// turns it 5.625° every 100 ms; Razdor's stays still.)*
fn draw_mark(cam: &Camera, pos: (f32, f32), color: Color) {
    let p = cam.to_screen(pos);
    let zoom = cam.scale / PX;
    // Sized as the original's screen shows it (about 30 × 20 at 1:1).
    let (w, h) = (32.0 * zoom, 22.0 * zoom);
    match super::chrome::win_fx("Selection-1", super::chrome::Fx::Glow) {
        Some(t) => draw_texture_ex(&t, p.x - w / 2.0, p.y - h / 2.0, color, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() }),
        None => draw_ellipse_lines(p.x, p.y, 15.0 * zoom, 10.0 * zoom, 0.0, 1.0, color),
    }
}

fn draw_army(game: &Game, a: &Army, assets: &Assets, art: Option<&DtArt>, cam: &Camera) {
    let next = a.path.first().map(|&t| game.world.map.center(t));
    let pos = game.army_display_pos(a);
    // Its walk frames run by game time (engine.md §7), not by the clock on the wall.
    let frame = game.army_walk_frame(a).unwrap_or(0);
    if a.sails() {
        if !draw_figure(art, ship_stem(a.ship), pos, next, frame, cam, Stand::Afloat) {
            let sail = if a.hostile() { Color::new(0.15, 0.12, 0.12, 1.0) } else { Color::new(0.92, 0.9, 0.82, 1.0) };
            draw_ship(cam, pos, sail);
        }
    } else if !FIGURES.get(a.figure as usize).is_some_and(|stem| draw_figure(art, stem, pos, next, frame, cam, Stand::Feet)) {
        // A figure past the table (an event's opcode 17 can set any byte) reads past the
        // original's sprites; Razdor draws the leader instead *(guess)*.
        let c = cam.to_screen(pos);
        if let Some(leader) = a.leader() {
            assets.draw_unit(leader, if a.hostile() { Team::Enemy } else { Team::Player }, c.x, c.y - 8.0, 26.0);
        }
    }
    let c = cam.to_screen(pos);
    if a.chasing {
        text_centered("!", c.x + 14.0, c.y - 30.0, 26.0, RED);
    }
}

fn draw_hero(game: &Game, assets: &Assets, art: Option<&DtArt>, cam: &Camera) {
    let model = match game.hero_class() {
        Some(HeroClass::Archmage) => 2,
        Some(HeroClass::Ranger) => 3,
        _ => 1,
    };
    let next = game.display_heading();
    let c = cam.to_screen(game.display_pos());
    if game.aboard() {
        if !draw_figure(art, ship_stem(razdor::rules::ships::kind::HERO), game.display_pos(), next, hero_frame(next), cam, Stand::Afloat) {
            draw_ship(cam, game.display_pos(), HERO_SAIL);
        }
        return;
    }
    if !draw_figure(art, figure_stem(model), game.display_pos(), next, hero_frame(next), cam, Stand::Feet) {
        assets.draw_unit(game.hero().def, Team::Player, c.x, c.y - 10.0, 30.0);
    }
}

fn draw_world(game: &Game, assets: &Assets, cam: &Camera, preview: Option<&[Tile]>) {
    let art = assets.dt.as_ref();
    draw_terrain(game, art, cam);
    let map = &game.world.map;
    let ((c0, c1), (r0, r1)) = cam.visible(map);
    let rh = cam.grid.row_height();
    // Sprites stand on their cell and reach up to ~8 cells above it.
    let (below, side) = (10, 8);
    let mut items: Vec<(f32, Drawable)> = Vec::new();
    let fog = &game.fog;
    // The original (0x4c8864) draws the hills of classes 1-3 in a pass of their own before
    // anything else, so they lie under every tree, mountain, building and army, the ones
    // above them included. Its test is `0x100 < class·256 + sprite < 0x401`: class 4's
    // sprites are 10 and up, so the yellow hills are drawn with the mountains, row by row.
    for o in map.objects_in_rows(r0 - 1, r1 + below) {
        if o.tile.0 >= c0 - side && o.tile.0 < c1 + side && razdor::rules::map::object_cells(o).any(|t| fog.explored_near(t, minimap::FEATHER)) {
            if (object_class::HILLS..=object_class::ROCKY_HILLS).contains(&o.class) {
                draw_object(o, art, cam);
            } else {
                items.push((o.tile.1 as f32 * rh, Drawable::Object(*o)));
            }
        }
    }
    // The marks under the hero and every army (cell byte +0xA, 0x48eeb0: the hero's kind 1,
    // an army's 3) lie on the ground in the same pass, under every tree, building and
    // figure (0x4c9459), so the figures' shadows fall over them.
    for a in game.world.armies.iter().filter(|a| fog.explored(a.tile(map)) && !a.sails()) {
        draw_mark(cam, game.army_display_pos(a), MARK_ARMY);
    }
    if !game.aboard() {
        draw_mark(cam, game.display_pos(), MARK_HERO);
    }
    // The route being walked, or the one a first click shows, lies on the ground under the
    // figures (the original's second pass, over the hills).
    if game.moving() {
        draw_route(game, &game.path, cam);
    } else if let Some(path) = preview {
        draw_route(game, path, cam);
    }
    // Buildings stand in front of the scenery: hills, rocks and trees south of one would
    // hide it, so they are drawn after every object, sorted among themselves. Bridges lie
    // flat, under everything standing on them.
    let mut buildings: Vec<(f32, Drawable)> = Vec::new();
    for (i, l) in game.world.locations.iter().enumerate() {
        let (ax, ay) = l.anchor;
        let seen = l.cells().any(|t| fog.explored_near(t, minimap::FEATHER));
        if seen && ay >= r0 - 1 && ay < r1 + below && ax >= c0 - side && ax - l.size.0 < c1 + side {
            if l.kind.is_bridge() {
                items.push((ay as f32 * rh - 1000.0, Drawable::Building(i)));
            } else {
                buildings.push((ay as f32 * rh, Drawable::Building(i)));
            }
        }
    }
    // Figures (armies, the waiting ship, the hero) always stand in front of the scenery: they
    // are sorted among themselves and drawn after every object and building.
    let mut figures: Vec<(f32, Drawable)> = Vec::new();
    // Armies in the dark keep moving but are not shown.
    for (i, a) in game.world.armies.iter().enumerate().filter(|(_, a)| fog.explored(a.tile(map))) {
        figures.push((game.army_display_pos(a).1 + 0.02, Drawable::Army(i)));
    }
    if let Some(ship) = game.ship.filter(|s| !s.aboard && fog.explored(s.tile)) {
        figures.push((map.center(ship.tile).1 + 0.02, Drawable::Ship));
    }
    figures.push((game.display_pos().1 + 0.03, Drawable::Hero));
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    buildings.sort_by(|a, b| a.0.total_cmp(&b.0));
    figures.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (_, d) in items.iter().chain(&buildings).chain(&figures) {
        match d {
            Drawable::Object(o) => draw_object(o, art, cam),
            Drawable::Building(i) => draw_building(&game.world.locations[*i], art, cam),
            Drawable::Army(i) => draw_army(game, &game.world.armies[*i], assets, art, cam),
            Drawable::Ship => {
                if let Some(ship) = game.ship {
                    let at = map.center(ship.tile);
                    if !draw_figure(art, ship_stem(razdor::rules::ships::kind::HERO), at, None, 0, cam, Stand::Afloat) {
                        draw_ship(cam, at, HERO_SAIL);
                    }
                }
            }
            Drawable::Hero => draw_hero(game, assets, art, cam),
        }
    }
}

/// Route dots and, at the end, the travel time.
/// The route being walked, as the original draws it: a white arrow on every cell ahead
/// (`Windows/Way_Arrows.ugs`, 32×22, one frame per direction in the exe's order: up-left,
/// up, up-right, right, down-right, down, down-left, left). The time left is in the bar.
fn draw_route(game: &Game, path: &[Tile], cam: &Camera) {
    let arrows = super::chrome::animation("Windows/Way_Arrows.ugs").filter(|a| a.len() == 8);
    let zoom = cam.scale / PX;
    let mut from = game.tile();
    for &t in path {
        let c = cam.cell_centre(t);
        let (dx, dy) = ((t.0 - from.0).signum(), (t.1 - from.1).signum());
        from = t;
        let dir = match (dx, dy) {
            (-1, -1) => 0,
            (0, -1) => 1,
            (1, -1) => 2,
            (1, 0) => 3,
            (1, 1) => 4,
            (0, 1) => 5,
            (-1, 1) => 6,
            _ => 7,
        };
        match &arrows {
            Some(a) => {
                let (w, h) = (a[dir].width() * zoom, a[dir].height() * zoom);
                draw_texture_ex(&a[dir], c.x - w / 2.0, c.y - h / 2.0, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
            }
            None => {
                draw_circle(c.x, c.y, 3.5, Color::new(0.0, 0.0, 0.0, 0.5));
                draw_circle(c.x, c.y, 2.5, WHITE);
            }
        }
    }
}

/// A 2×6 (or 3×4) mini formation of portraits, the front row at the bottom as the enemy's
/// in battle; empty cells show their row's icon.
fn formation_grid(game: &Game, assets: &Assets, troops: &[Troop], team: Team, x: f32, y: f32, cell: f32) -> f32 {
    let f = game.content.formation;
    let lines = f.display_lines();
    for r in 0..lines {
        for col in 0..f.display_cols() {
            let Some(slot) = f.at_display(r, col) else { continue };
            let (cx, cy) = (x + col as f32 * (cell + 3.0), y + (lines - 1 - r) as f32 * (cell + 3.0));
            let sq = Rect::new(cx, cy, cell, cell);
            draw_rectangle(cx, cy, cell, cell, Color::new(0.0, 0.0, 0.0, 0.45));
            draw_rectangle_lines(cx, cy, cell, cell, 1.0, Color::new(0.6, 0.6, 0.62, 0.8));
            let troop = troops.iter().find(|t| f.display(t.slot) == (r, col));
            if troop.is_none() {
                super::chrome::cell_icon(super::chrome::CellIcon::of(f, slot), sq);
            }
            if let Some(t) = troop {
                assets.draw_portrait(t.unit, team, sq);
                // Wounds left by world spells.
                let max = razdor::rules::units::Stats::of_level(&game.content, t.unit, t.level).max_hp();
                super::chrome::wounds(sq, max - t.hurt, max);
                draw_rectangle_lines(cx, cy, cell, cell, 1.0, Color::new(0.8, 0.8, 0.8, 0.9));
                // The troop's level in the corner.
                let lv = t.level.to_string();
                let tw = measure(&lv, 14.0).width;
                draw_rectangle(cx + cell - tw - 4.0, cy + cell - 14.0, tw + 4.0, 14.0, Color::new(0.0, 0.0, 0.0, 0.6));
                text(&lv, cx + cell - tw - 2.0, cy + cell - 2.0, 14.0, XP_COLOR);
            }
        }
    }
    f.display_lines() as f32 * (cell + 3.0)
}

struct Tooltip {
    title: String,
    lines: Vec<(String, Color)>,
    troops: Vec<Troop>,
    team: Team,
    footer: Vec<(String, Color)>,
    style: TipStyle,
}

/// A text of the install (`[Info] <key>`) in Russian, else ours.
fn info(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Info", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The name colour of the original's tooltips (the leader, the owner).
const TIP_NAME: Color = Color::new(0.45, 1.0, 0.5, 1.0);
/// Its orange notes ("(дань уже собрана)"), and the names in the hostile frame.
const TIP_NOTE: Color = Color::new(1.0, 0.62, 0.25, 1.0);
/// The labels ("Владелец") in the hostile frame: white less (20, 90, 180) (font 0xae24c4).
const TIP_LABEL_HOSTILE: Color = Color::new(0.92, 0.65, 0.29, 1.0);
/// Names and labels in the neutral frame: white less (80, 80, ...) and (100, 100, 100).
const TIP_NAME_NEUTRAL: Color = Color::new(0.69, 0.69, 0.69, 1.0);
const TIP_LABEL_NEUTRAL: Color = Color::new(0.61, 0.61, 0.61, 1.0);

/// The original's army tooltip: its name, its 2×6 cards, "Предводитель" and the leader's
/// name, the description; world spells on it and their wounds (Razdor's) under that.
fn army_tooltip(game: &Game, a: &Army) -> Tooltip {
    let title = if a.name.is_empty() { info("NoNameArmy", n_("Unknown army")) } else { a.name.clone() };
    let mut footer = Vec::new();
    if !a.leader_name.is_empty() {
        footer.push((info("Commander", n_("Leader")), DIM));
        footer.push((a.leader_name.clone(), TIP_NAME));
    }
    for line in wrap(&a.description, 320.0 * super::chrome::k(), 12.0 * super::chrome::k()) {
        footer.push((line, INK));
    }
    let now = game.clock.total_minutes() as u64;
    let mut spells: Vec<&str> = Vec::new();
    for s in a.troops.iter().flat_map(|t| t.spells.iter().flatten()).filter(|s| s.until > now).filter_map(|s| game.spell(s.spell)) {
        if !spells.contains(&s.name.as_str()) {
            spells.push(s.name.as_str());
        }
    }
    if !spells.is_empty() {
        footer.push((trf!("Under spells: {spells}", spells = spells.join(", ")), MANA));
    }
    let hurt: i32 = a.troops.iter().map(|t| t.hurt).sum();
    if hurt > 0 {
        footer.push((trf!("Wounded by magic: -{hurt} hits", hurt), MANA));
    }
    // 0x4ca9f0: the hostile frame and colours for an army whose attitude to the player is
    // below 1, neutral ones (0) included.
    let style = if a.attitude < 1 { TipStyle::Hostile } else { TipStyle::Normal };
    Tooltip { title, lines: Vec::new(), troops: a.troops.clone(), team: if a.hostile() { Team::Enemy } else { Team::Player }, footer, style }
}

/// The owner line of a building's tooltip. The holder is named only for a castle, fort or
/// ruins (0x4cb18c): the hero, an army's leader or the map's owner. Any other building, a
/// captured village too, always names the map's owner.
fn tooltip_owner(game: &Game, l: &Location) -> String {
    match (l.kind, l.owner) {
        (LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins, Owner::Player) => game.hero_name.clone().unwrap_or_else(|| tr("you").to_string()),
        (LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins, Owner::Army(k)) => game.world.armies.iter().find(|a| a.id == k).map_or_else(String::new, |a| a.leader_name.clone()),
        _ => l.owner_name.clone(),
    }
}

/// The original's building tooltip: its name, "Владелец" and the owner's name, the
/// description, "(дань уже собрана)" for a village already emptied; a garrison under
/// "Состав гарнизона защитников:".
fn location_tooltip(game: &Game, l: &Location) -> Tooltip {
    let title = if l.name.is_empty() { info("NoNameBuilding", n_("Unknown building")) } else { l.name.clone() };
    let mut lines = Vec::new();
    let owner = tooltip_owner(game, l);
    if !owner.trim().is_empty() {
        lines.push((info("Owner", n_("Owner")), DIM));
        lines.push((owner, TIP_NAME));
    }
    for line in wrap(&l.description, 320.0 * super::chrome::k(), 12.0 * super::chrome::k()) {
        lines.push((line, INK));
    }
    if l.kind == LocationKind::Village && l.tribute_gold <= 0 && l.tribute_mana <= 0 {
        lines.push((info("VillageEmptyGold", n_("(tribute already collected)")), TIP_NOTE));
    }
    // The garrison, whoever holds the building (0x4cb18c): the player's units left there too.
    // A town shows none; ruins say they are guarded but hide by whom.
    let mut troops: Vec<Troop> = l.garrison.iter().filter(|t| t.alive()).cloned().collect();
    for s in l.stationed.iter().filter(|s| s.unit.alive()) {
        let u = &s.unit;
        let mut t = Troop::new(u.def, u.level, u.slot);
        t.hurt = (u.max_hp(&game.content) - u.hp).max(0);
        troops.push(t);
    }
    if l.kind == LocationKind::Town || l.cleared {
        troops.clear();
    }
    if !troops.is_empty() {
        lines.push((info("Defenders", n_("The garrison's defenders:")), DIM));
        if l.kind == LocationKind::Ruins {
            lines.push((info("NoNameArmy", n_("Unknown army")), INK));
            troops.clear();
        }
    }
    let team = if l.owned() { Team::Player } else { Team::Enemy };
    Tooltip { title, lines, troops, team, footer: Vec::new(), style: location_tip_style(l) }
}

/// The frame of a building's tooltip (0x4cb18c): neutral for bridges; for a castle, fort or
/// ruins the player does not own, hostile when its attitude to him is below 1 and always for
/// ruins, but neutral for ruins with no one left in them. Every other building (villages,
/// churches, taverns, towns...) keeps the normal frame, whoever holds it.
fn location_tip_style(l: &Location) -> TipStyle {
    use LocationKind as K;
    match l.kind {
        K::StoneBridge | K::WoodenBridge => TipStyle::Neutral,
        K::Castle | K::Fort | K::Ruins if !l.owned() => {
            let guarded = !l.cleared && l.garrison.iter().any(|t| t.alive());
            if l.kind == K::Ruins && !guarded {
                TipStyle::Neutral
            } else if l.attitude < 1 || l.kind == K::Ruins {
                TipStyle::Hostile
            } else {
                TipStyle::Normal
            }
        }
        _ => TipStyle::Normal,
    }
}

fn draw_tooltip(game: &Game, assets: &Assets, t: &Tooltip) {
    use super::chrome::{shadow_centered, CREAM};
    use super::dt_font::{with_face, Face};
    let k = super::chrome::k();
    let cell = (46.0 * k).round();
    let f = game.content.formation;
    // `formation_grid` spaces its cells 3 px apart.
    let grid_w = f.display_cols() as f32 * (cell + 3.0) - 3.0;
    let grid_h = if t.troops.is_empty() { 0.0 } else { f.display_lines() as f32 * (cell + 3.0) + 8.0 * k };
    // Names in Benguiat, larger; the rest small.
    let big = |c: Color| c == TIP_NAME || c == TIP_NOTE;
    let size = |c: Color| if big(c) { 16.0 * k } else { 12.0 * k };
    let face = |c: Color| if big(c) { Face::Title } else { Face::Body };
    // The style's colours (0x4ca9f0, 0x4cb18c): in the hostile frame names and labels are
    // orange (fonts 0xae24a4, 0xae24c4), in the neutral one grey (0xae24b8, 0xae24d0).
    let shown = |c: Color| match (t.style, c) {
        (_, c) if c == INK => CREAM,
        (TipStyle::Hostile, c) if c == TIP_NAME => TIP_NOTE,
        (TipStyle::Hostile, c) if c == DIM => TIP_LABEL_HOSTILE,
        (TipStyle::Neutral, c) if c == TIP_NAME => TIP_NAME_NEUTRAL,
        (TipStyle::Neutral, c) if c == DIM => TIP_LABEL_NEUTRAL,
        (_, c) => c,
    };
    let width = |s: &str, c: Color| with_face(face(c), || measure(s, size(c)).width);
    let title_size = 16.0 * k;
    let w = [with_face(Face::Title, || measure(&t.title, title_size).width) + 90.0 * k, grid_w + 24.0 * k, 250.0 * k]
        .into_iter()
        .chain(t.lines.iter().chain(&t.footer).map(|(s, c)| width(s, *c) + 24.0 * k))
        .fold(0.0, f32::max)
        .min(420.0 * k);
    let lh = |c: Color| size(c) + 3.0 * k;
    let lines_h: f32 = t.lines.iter().chain(&t.footer).map(|(_, c)| lh(*c)).sum();
    let bar = 24.0 * k;
    let h = bar + 8.0 * k + lines_h + grid_h + 8.0 * k;
    let (mx, my) = crate::ui::widgets::pointer();
    let x = (mx + 18.0).min(screen_width() - w - 4.0);
    let y = (my + 18.0).min(screen_height() - bar_h() - h - 4.0).max(2.0);
    tooltip_panel_styled(Rect::new(x, y, w, h), t.style);
    // The title strip with the ornaments at its ends.
    draw_rectangle(x + 2.0, y + 2.0, w - 4.0, bar - 2.0, Color::new(0.0, 0.0, 0.0, 0.25));
    if let Some(orn) = super::chrome::win_fx("Corner-Left", super::chrome::Fx::KeyBlack) {
        let oh = bar * 0.8;
        let ow = orn.width() * oh / orn.height();
        // The hostile frame has its own pair of end pieces (0xae255c/60), tinted as in
        // Razdor's red windows; the neutral one keeps the normal pair (0xae2554/58).
        let tint = if t.style == TipStyle::Hostile { Color::new(1.0, 0.45, 0.1, 0.9) } else { Color::new(0.55, 0.8, 0.7, 0.8) };
        super::chrome::tex(&orn, Rect::new(x + 4.0 * k, y + (bar - oh) / 2.0, ow, oh), tint);
        if let Some(r) = super::chrome::win_fx("Corner-Right", super::chrome::Fx::KeyBlack) {
            super::chrome::tex(&r, Rect::new(x + w - ow - 4.0 * k, y + (bar - oh) / 2.0, ow, oh), tint);
        }
    }
    draw_line(x + 2.0, y + bar, x + w - 2.0, y + bar, 1.0, super::chrome::SILVER);
    with_face(Face::Title, || shadow_centered(&t.title, x + w / 2.0, y + bar * 0.5 + title_size * 0.36, title_size, CREAM));
    let mut ly = y + bar + 6.0 * k;
    let line = |s: &str, c: Color, ly: &mut f32| {
        with_face(face(c), || shadow_centered(s, x + w / 2.0, *ly + size(c), size(c), shown(c)));
        *ly += lh(c);
    };
    for (s, c) in &t.lines {
        line(s, *c, &mut ly);
    }
    if !t.troops.is_empty() {
        ly += formation_grid(game, assets, &t.troops, t.team, x + (w - grid_w) / 2.0, ly + 4.0 * k, cell) + 8.0 * k;
    }
    for (s, c) in &t.footer {
        line(s, *c, &mut ly);
    }
}

/// What the mouse is over: an army, else a building.
fn hover_tooltip(game: &Game, cam: &Camera) -> Option<Tooltip> {
    let m = Vec2::from(crate::ui::widgets::pointer());
    if !cam.view.contains(m) {
        return None;
    }
    let near = 22.0 * (cam.scale / PX).max(0.6);
    let map = &game.world.map;
    if let Some(a) = game.world.armies.iter().filter(|a| game.fog.explored(a.tile(map))).find(|a| (cam.to_screen(game.army_display_pos(a)) - vec2(0.0, 12.0 * cam.scale / PX) - m).length() < near) {
        return Some(army_tooltip(game, a));
    }
    let t = cam.tile_under_mouse().filter(|&t| game.fog.explored(t))?;
    let l = game.world.location_covering(t).or_else(|| game.world.location_at(t))?;
    Some(location_tooltip(game, &game.world.locations[l]))
}

/// A click on the building the party stands in (`t` one of its cells): its window again, or
/// the battle with a garrison still to beat. Nothing for a burnt camp.
fn reopen_here(game: &mut Game, t: Tile) -> Option<Screen> {
    let l = game.location?;
    if game.world.location_covering(t).or_else(|| game.world.location_at(t)) != Some(l) {
        return None;
    }
    let loc = &game.world.locations[l];
    if loc.kind == LocationKind::Camp && loc.cleared {
        return None;
    }
    if loc.defended() {
        game.foe = Some(Foe::Garrison(l));
        return Some(saves::battle(game));
    }
    game.window_at(l).map(|first| Screen::Building(BuildingView::new(first)))
}


/// The play log's line for a world event: scenario events by number and title.
fn play_event(game: &Game, event: &Event) {
    use razdor::rules::events::EventOutcome as O;
    let title = |id: u16| story::event_title(game, id);
    let line = match event {
        Event::Script(O::Fired { event, message }) => format!("EVENT {event} «{}» fired{}", title(*event), if *message { " (message)" } else { "" }),
        Event::Script(O::Question(id)) => format!("EVENT {id} «{}» asks", title(*id)),
        Event::Script(O::Declined(id)) => format!("EVENT {id} «{}» declined", title(*id)),
        Event::Script(O::QuestAdded(id)) => format!("QUEST {id} «{}» added", title(*id)),
        Event::Script(O::QuestCompleted(id)) => format!("QUEST {id} «{}» completed", title(*id)),
        Event::Script(other) => format!("EVENT {other:?}"),
        Event::Encounter(i) => format!("ENCOUNTER army {} «{}» at {:?}", game.world.armies.get(*i).map_or(0, |a| a.id), game.world.armies.get(*i).map_or("", |a| &a.name), game.tile()),
        Event::Met(i) => format!("MET army {} «{}»", game.world.armies.get(*i).map_or(0, |a| a.id), game.world.armies.get(*i).map_or("", |a| &a.name)),
        Event::Arrived(l) => format!("ARRIVED at building {} «{}»", game.world.locations[*l].id, game.world.locations[*l].name),
        Event::Captured(l) => format!("CAPTURED building {} «{}»", game.world.locations[*l].id, game.world.locations[*l].name),
        Event::NewDay(r) => format!("NOON income {} wages {} unpaid {} deserted {} gold {}", r.income, r.wages, r.unpaid, r.deserted.len(), r.gold),
        other => format!("{other:?}"),
    };
    razdor::diag::play(&game.clock.label(), &line);
}

fn describe(event: &Event, game: &Game) -> Option<String> {
    match event {
        Event::NewDay(_) | Event::Captured(_) | Event::Script(_) => None,
        Event::SpellCast { spell, target, outcome } => game.spell(*spell).map(|s| super::spellbook::landed(s, *target, *outcome)),
        Event::Tribute { paid, mana, .. } => Some(match paid {
            razdor::rules::game::Tribute::Gold(g) => trf!("The village pays its tribute: {g} gold and {mana} mana.", g, mana),
            razdor::rules::game::Tribute::Item(item) => trf!("The village pays with a {item} and {mana} mana.", item = game.content.item(*item).name, mana),
        }),
        Event::LevelUp(i, level) => game.squad.get(*i).map(|u| trf!("{name} reaches level {level}!", name = u.name(&game.content), level)),
        Event::Battle(news) => Some(news.text.clone()),
        Event::Arrived(l) => {
            let loc = &game.world.locations[*l];
            game.foe.is_some().then(|| trf!("{place}: the garrison bars your way!", place = loc.name))
        }
        // The army may have left the map since the meeting was recorded (an event took it
        // away): then it is "an army" (Razdor 0.3.10 crashed on such a meeting).
        Event::Encounter(i) => {
            let name = game.world.armies.get(*i).map_or("", |a| a.name.as_str());
            Some(if name.is_empty() { tr("An army attacks!").to_string() } else { trf!("{name} attacks!", name) })
        }
        Event::Met(i) => {
            let name = game.world.armies.get(*i).map_or("", |a| a.name.as_str());
            let who = if name.is_empty() { tr("An army") } else { name };
            Some(trf!("A meeting on the road: {who} lets you pass.", who))
        }
    }
}

/// Applies the events of a tick or a wait: noon reports open the report window, stepping
/// into a building opens its window, the scenario's events open their dialogs. Returns the
/// next screen, if any.
pub(super) fn handle_events(game: &mut Game, events: Vec<Event>, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let mut next = None;
    let mut tribute = false;
    for event in events {
        play_event(game, &event);
        if let Some(m) = describe(&event, game) {
            *message = Some(m);
        }
        match event {
            // The battle waits for the messages of the same moment (a meeting's words) to be
            // read: the app starts it once no dialog is open (`App::frame`).
            Event::Encounter(_) => {}
            Event::Arrived(l) => {
                if game.foe.is_some() {
                } else if let Some(first) = game.window_at(l) {
                    *message = None;
                    next = Some(Screen::Building(BuildingView::new(first)));
                }
            }
            Event::NewDay(r) => dialogs.push_back(Dialog::day_report(game, &r)),
            // A building taken on the way opens no window and does not stop the walk
            // (0x4ad94c); the village's own window opens when he ends his walk in it.
            // The tribute's gold sound plays as the village window closes (`building_view`).
            Event::Tribute { .. } => tribute = true,
            // A spell read to its end: the camera to an enemy target, then the effect.
            Event::SpellCast { spell, target, outcome } => {
                if let (Some(s), CastOutcome::Done { .. }) = (game.spell(spell).cloned(), outcome) {
                    if !matches!(target, CastTarget::Own) {
                        look_at(target);
                    }
                    spell_effect(&s, target);
                }
            }
            Event::Captured(_) | Event::Met(_) | Event::Battle(_) => {}
            // A level gained is silent: `Unit-Upgrade` is the promotion screen's (0x4b1af8).
            Event::LevelUp(..) => {}
            Event::Script(o) => story::show(game, &o, message, dialogs),
        }
    }
    if let Some(Screen::Building(v)) = next.as_mut() {
        v.tribute_paid |= tribute;
    }
    // The tutorial's end mark (0x4ac9fc): kept in the settings, so it is not offered again.
    if game.script().is_some_and(|s| s.tutorial_done()) {
        let settings = super::language::Settings::load();
        if !settings.tutorial_completed {
            super::language::Settings { tutorial_completed: true, ..settings }.save();
        }
    }
    next
}

/// The map under a building window or a dialog: drawn over the whole screen, not
/// interactive, with the bar's buttons greyed (`lit`: the open screen's button).
pub fn backdrop_lit(game: &Game, assets: &Assets, lit: Option<BarButton>) {
    clear_background(rgb(10, 12, 10));
    let full = map_area();
    let cam = Camera::looking_in(game, 1.0, game.display_pos(), full);
    draw_world(game, assets, &cam, None);
    cam.draw_fog(game);
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.2));
    game_bar::draw(game, |b| if Some(b) == lit { Look::Lit } else { Look::Grey });
}

pub fn backdrop(game: &Game, assets: &Assets) {
    backdrop_lit(game, assets, None);
}

/// The map under a window the bar stays live for (army, spell book, journal, a building),
/// as in the original: its buttons are blue, the window's own one green; one pressed gives
/// the screen it opens (the lit one, or the map button, closes the window).
pub fn window_backdrop(game: &Game, assets: &Assets, lit: Option<BarButton>) -> Option<Screen> {
    clear_background(rgb(10, 12, 10));
    let full = map_area();
    let cam = Camera::looking_in(game, 1.0, game.display_pos(), full);
    draw_world(game, assets, &cam, None);
    cam.draw_fog(game);
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.2));
    let idle = game.foe.is_none();
    let modal = input_blocked();
    let pressed = game_bar::draw(game, |b| match b {
        _ if modal => Look::Grey,
        _ if Some(b) == lit => Look::Lit,
        BarButton::Save | BarButton::Spells if !idle => Look::Grey,
        _ => Look::Normal,
    })?;
    Some(match pressed {
        b if Some(b) == lit => Screen::WorldMap,
        BarButton::Menu => Screen::Menu(false),
        BarButton::Settings => Screen::Settings,
        BarButton::Save => Screen::Save(SaveView::new(game, Back::Map)),
        BarButton::Load => Screen::Load(LoadView::new(Back::Map)),
        BarButton::Journal => Screen::Journal(Default::default()),
        BarButton::Squad => Screen::Squad { selected: Default::default(), scroll: 0, back: None },
        BarButton::Spells => Screen::Spellbook { selected: 0 },
        BarButton::Map => Screen::WorldMap,
    })
}

/// The bottom bar of the map: its buttons and keys. Returns the next screen and whether the
/// minimap was toggled.
fn bottom_bar(game: &mut Game, message: &mut Option<String>, minimap_open: bool, time_buttons: bool) -> (Option<Screen>, bool, Option<TimeButton>) {
    let idle = game.foe.is_none();
    let modal = input_blocked();
    let look = |b: BarButton| match b {
        _ if modal => Look::Grey,
        BarButton::Save | BarButton::Spells if !idle => Look::Grey,
        BarButton::Map if minimap_open => Look::Glow,
        _ => Look::Normal,
    };
    let (mut pressed, timed) = game_bar::draw_with_time(game, look, time_buttons);
    // The bar's keys (Razdor's) act as its buttons, also while the hero walks or waits: the
    // key held has cut his walk (`frame`), as the original's any key does.
    if pressed.is_none() {
        // Esc closes the minimap first (Razdor's); the menu only when nothing else is open.
        pressed = if key(KeyCode::Escape) {
            Some(esc_on_map(minimap_open))
        } else if idle && key(KeyCode::B) {
            Some(BarButton::Spells)
        } else if key(KeyCode::J) {
            Some(BarButton::Journal)
        } else if key(KeyCode::A) {
            Some(BarButton::Squad)
        } else {
            None
        };
    }
    let next = match pressed {
        Some(BarButton::Menu) => Some(Screen::Menu(false)),
        Some(BarButton::Settings) => Some(Screen::Settings),
        Some(BarButton::Save) => Some(Screen::Save(SaveView::new(game, Back::Map))),
        Some(BarButton::Load) => Some(Screen::Load(LoadView::new(Back::Map))),
        Some(BarButton::Journal) => Some(Screen::Journal(Default::default())),
        Some(BarButton::Squad) => {
            *message = None;
            Some(Screen::Squad { selected: Default::default(), scroll: 0, back: None })
        }
        Some(BarButton::Spells) => {
            *message = None;
            Some(Screen::Spellbook { selected: 0 })
        }
        Some(BarButton::Map) | None => None,
    };
    // A press while the hero walks has cut his route (`frame`): as in the original, where
    // a window opens over the walk and the wait without stopping them, he finishes the step
    // under way, and a wait goes on, when the map is back. A save holds him where he stands
    // *(Razdor's saves keep the route; the original's do not)*.
    if matches!(next, Some(Screen::Save(_))) && game.moving() {
        game.stop();
    }
    (next, pressed == Some(BarButton::Map), timed)
}

/// Where the camera looks (`None`: on the hero) after this frame's input: the walk locks it
/// on the hero (0x4ae8a8) unless an event's places are being shown; otherwise it stays where
/// the player put it, a planning click included (the original's click moves no camera).
fn camera_look(look: Option<(f32, f32)>, walking: bool, showing: bool) -> Option<(f32, f32)> {
    if walking && !showing {
        None
    } else {
        look
    }
}

/// What Esc on the map presses: Razdor's Esc closes the minimap first (the original opens the
/// exit menu over it); with nothing open, the exit menu.
fn esc_on_map(minimap_open: bool) -> BarButton {
    if minimap_open {
        BarButton::Map
    } else {
        BarButton::Menu
    }
}

/// What a left click on a target cell of the idle map does (interface.md §7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MapClick {
    /// The planned cell with its route drawn: the hero sets off.
    SetOff,
    /// Another cell in reach: its route is drawn and it becomes the planned cell.
    Planned,
    /// Another cell out of reach: the route drawn is dropped, the planned cell stays.
    OutOfReach,
}

/// The original's planning click (0x4cc426–0x4cc9bc) on `target` (not the hero's cell),
/// with `planned` the planned cell, its route and where the hero stood. A click on the
/// planned cell whose route was dropped (by a click out of reach, or a wait) plans it
/// again. Razdor fixes the original's bug: there such a cell stayed planned and its clicks
/// did nothing until a click elsewhere planned again.
fn plan_click(planned: &mut Option<(Tile, Vec<Tile>, Tile)>, target: Tile, here: Tile, route: impl FnOnce() -> Vec<Tile>) -> MapClick {
    if planned.as_ref().is_some_and(|p| p.0 == target && !p.1.is_empty()) {
        return MapClick::SetOff;
    }
    let route = route();
    if route.is_empty() {
        if let Some(p) = planned.as_mut() {
            p.1.clear();
        }
        return MapClick::OutOfReach;
    }
    *planned = Some((target, route, here));
    MapClick::Planned
}

pub fn frame(game: &mut Game, assets: &Assets, view: &mut MapView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    clear_background(rgb(10, 12, 10));

    // The original's world frame acts on the map only while it is idle (interface.md §7.3,
    // 0x4cc1ff): while the hero walks or waits, it only watches for the stop.
    // The glide back to the hero takes no input either (0x4af96c).
    let idle = !game.moving() && !game.waiting() && game.reading().is_none() && view.centring.is_none();
    // The whole milliseconds since the map's last frame (the original's timeGetTime).
    let now_ms = (get_time() * 1000.0) as i64;
    let dt_ms = now_ms - view.last_frame_ms.unwrap_or(now_ms);
    view.last_frame_ms = Some(now_ms);

    // Zoom: mouse wheel or +/- (Razdor's, also while he walks or waits).
    let wheel = wheel();
    if wheel != 0.0 {
        view.zoom = (view.zoom * if wheel > 0.0 { 1.1 } else { 1.0 / 1.1 }).clamp(0.4, 2.0);
    }
    if key(KeyCode::Equal) || key(KeyCode::KpAdd) {
        view.zoom = (view.zoom * 1.2).min(2.0);
    }
    if key(KeyCode::Minus) || key(KeyCode::KpSubtract) {
        view.zoom = (view.zoom / 1.2).max(0.4);
    }

    // M toggles the minimap; F3 Razdor's debug overlay.
    if key(KeyCode::M) {
        view.minimap = !view.minimap;
    }
    if key(KeyCode::F3) {
        view.debug = !view.debug;
    }
    // Places the scenario has just shown wait in line (dark until their turn). Tab or a
    // click on the map skips the showing; Tab: the camera back on the hero (after the
    // minimap or a showing moved it).
    for shown in std::mem::take(&mut game.shown) {
        view.shows.push_back(Showing::new(game, &shown));
    }
    if (!view.shows.is_empty() || view.returning.is_some()) && !input_blocked() && ((idle && clicked()) || key(KeyCode::Tab)) {
        view.shows.clear();
        view.returning = None;
        view.look = None;
    }
    // Tab: the centre button's glide (Razdor's key), when the view is off the hero; while
    // he walks (Razdor's key works then too) the view is on him already.
    if view.centring.is_none() && view.look.is_some() && key(KeyCode::Tab) {
        if idle {
            view.centring = Some((get_time(), view.look.unwrap_or(game.display_pos())));
        } else {
            view.look = None;
        }
    }
    if let Some((t0, from)) = view.centring {
        let ms = ((get_time() - t0) * 1000.0) as i64;
        let to = game.display_pos();
        let e = glide_ease(ms);
        view.look = Some((from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e));
        if ms >= GLIDE_MS {
            view.centring = None;
            view.look = None;
        }
    }
    if idle && dialogs.is_empty() && view.grab.is_none() {
        scroll(game, view, dt_ms);
    }
    // While he walks the view is locked on him (interface.md §8).
    view.look = camera_look(view.look, game.moving(), !view.shows.is_empty());
    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    let on_minimap = view.minimap && minimap::under_pointer(&game.world.map, cam.view);
    let clock = game_bar::time_panel();
    let on_clock = !input_blocked() && clock.contains(crate::ui::widgets::pointer().into());
    let hovered = cam.tile_under_mouse().filter(|_| !on_minimap);
    let mut reopened = None;
    if !idle {
        // A left click or any key held while he walks cuts his route: he finishes the step
        // under way and stops (0x4cd132). Razdor's view keys leave him walking
        // ([`cuts_walk`]); a wait goes on.
        if game.moving() && (clicked() || held_key().is_some_and(cuts_walk)) {
            game.cut_walk();
            view.preview = None;
        }
        // F5 held ends the Community endless wait (0xc27802).
        if game.endless_waiting() && held_key() == Some(KeyCode::F5) {
            game.end_endless_wait();
        }
        // A left click anywhere or a key press during a wait ends it after the half hour
        // under way, and does nothing else *(Razdor's choice: the original's waits run to
        // their end)*. Razdor's view keys leave it running, as they leave a walk, and Space
        // is Razdor's stop, which ends it at once (below).
        if game.waiting() && (clicked() || (any_key_pressed() && get_keys_pressed().into_iter().any(|k| cuts_walk(k) && k != KeyCode::Space))) {
            game.cut_wait();
            swallow_input();
        }
    } else if clicked() && !on_minimap {
        if let Some(screen) = hovered.and_then(|t| reopen_here(game, t)) {
            // A click on the building the party stands in opens it again.
            *message = None;
            reopened = Some(screen);
        } else if let Some(target) = hovered.filter(|&t| game.can_target(t)) {
            // Only a target cell counts (explored, open on his map, or his ship): a click
            // anywhere else does nothing, as in the original. The first click shows the
            // route; a second one on the same spot sets off (0x4cc99f).
            if target != game.tile() {
                match plan_click(&mut view.preview, target, game.tile(), || game.route_to(target)) {
                    MapClick::SetOff => {
                        if game.set_destination(target) {
                            view.preview = None;
                            razdor::diag::play(&game.clock.label(), &format!("WALK from {:?} to {:?}: {} steps, {:.0} min", game.tile(), target, game.path.len(), game.minutes_left()));
                        }
                    }
                    MapClick::OutOfReach => *message = Some(tr("No way through.").into()),
                    MapClick::Planned => {}
                }
            }
            // The view stays where it is while the route is chosen; it goes back to the
            // hero once he sets off (below), as the original's click (0x4cc3ad-0x4cca39)
            // leaves the camera alone and only the walk locks it (0x4ae8a8).
        }
    }
    // Razdor's stop: Space, or a right click on the map (the right button dragged moves the
    // map instead); either also drops the route shown.
    let right_click = if game.moving() {
        right_clicked() && !on_minimap && !on_clock && cam.view.contains(Vec2::from(pointer()))
    } else {
        grab_map(game, view, &cam, on_minimap || on_clock || view.centring.is_some())
    };
    if key(KeyCode::Space) || right_click {
        razdor_stop(game);
        view.preview = None;
    }
    // A planned route belongs to where the hero stood: the end of a walk forgets it
    // (0x4ae5d8).
    if view.preview.as_ref().is_some_and(|p| p.2 != game.tile() || game.moving()) {
        view.preview = None;
    }

    // Time stands still while a window is open.
    let mut events = game.drain_events();
    if !input_blocked() && events.is_empty() {
        events = game.tick_shown(get_frame_time().min(0.1));
    }
    let mut next = handle_events(game, events, message, dialogs).or(reopened);

    // The place being shown: once its message is read, the camera flies there and the
    // uncovered area fades in; then the next place, if any. After the last one the camera
    // flies back to the hero.
    let now = get_time();
    for shown in std::mem::take(&mut game.shown) {
        view.shows.push_back(Showing::new(game, &shown));
    }
    // An event's places are shown once its own window is closed, before the next window
    // opens (`App` holds it meanwhile); after its last one the camera flies back.
    if let Some((t0, from)) = view.returning {
        let p = ((now - t0) / SHOW_PAN).clamp(0.0, 1.0) as f32;
        let ease = p * p * (3.0 - 2.0 * p);
        let to = game.display_pos();
        view.look = Some((from.0 + (to.0 - from.0) * ease, from.1 + (to.1 - from.1) * ease));
        if p >= 1.0 {
            view.returning = None;
            view.look = None;
        }
    } else if let Some(front) = view.shows.front_mut().filter(|s| s.free(dialogs)) {
        let event = front.event;
        let here = view.look.unwrap_or(game.display_pos());
        let (t0, from) = *front.started.get_or_insert((now, here));
        let p = ((now - t0) / SHOW_PAN).clamp(0.0, 1.0) as f32;
        let ease = p * p * (3.0 - 2.0 * p);
        view.look = Some((from.0 + (front.at.0 - from.0) * ease, from.1 + (front.at.1 - from.1) * ease));
        if now - t0 > SHOW_PAN + SHOW_FADE + SHOW_REST {
            view.shows.pop_front();
            if view.shows.front().is_none_or(|s| s.event != event) {
                view.returning = view.look.map(|at| (now, at));
            }
        }
    } else {
        play_spell_fx(game, view, now);
    }
    if view.opening.as_ref().is_some_and(|s| s.progress(now) >= 1.0) {
        view.opening = None;
    }

    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    draw_world(game, assets, &cam, view.preview.as_ref().map(|p| p.1.as_slice()));
    cam.draw_fog(game);
    cam.draw_showing(game, view.opening.iter().chain(&view.shows), now);
    if let Some(s) = view.shows.front() {
        cam.draw_known_place(s, now);
    }
    // A spell's effect over the army it landed on.
    if let Some(SpellFx::Effect { army, art, started: Some(t0) }) = view.spell_fx.front() {
        if let Some(at) = army_pos(game, *army) {
            let c = cam.to_screen(at);
            let t = ((now - t0) / SPELL_FX_SECS) as f32;
            super::chrome::effect(art, c, cam.cell_size().x * 3.0, t, WHITE);
        }
    }
    // The shown route's travel time, at its end.
    if let Some(end) = view.preview.as_ref().and_then(|p| p.1.last().map(|&e| (e, game.travel_minutes(&p.1)))) {
        let c = cam.cell_centre(end.0);
        let label = duration_label(end.1 as f64);
        let size = (15.0 * super::chrome::k()).round();
        super::chrome::shadow_centered(&label, c.x, c.y - 14.0 * cam.scale / PX, size, ACCENT);
    }

    // The original has no side panel: the map fills the screen above the bar. Lasting
    // world spells stand small in the top left corner.
    let now = game.clock.total_minutes() as u64;
    let notes: Vec<(String, Color)> = game
        .active_spells()
        .iter()
        .filter_map(|e| {
            let name = &game.spell(e.spell)?.name;
            Some(format!("{name} ({})", duration_label(e.until.saturating_sub(now) as f64)))
        })
        .map(|l| (l, MANA))
        .collect();
    for (i, (line, color)) in notes.iter().enumerate() {
        super::chrome::shadow_text(line, 10.0, 22.0 + i as f32 * 18.0, 16.0, *color);
    }
    // Waiting: the buttons over the message box (interface.md §6), 1 / 4, or a click on the
    // time panel off the buttons (left 1 h, right 4 h). Waits play in real time, a 30-minute
    // tick every 150 ms (`Game::tick`).
    let can_wait = idle && game.foe.is_none();
    // The three buttons show on the idle map only: no walk, wait, flight or glide.
    let time_buttons = can_wait && view.shows.is_empty() && view.returning.is_none() && view.spell_fx.is_empty();
    let (bar_top, bar_mid, bar_scale) = game_bar::time_layout();
    let on_button = game_bar::time_button_at(Vec2::from(pointer()), time_buttons, bar_top, bar_mid, bar_scale).is_some();
    let on_clock = on_clock && !on_button;
    // The original's wait buttons play the button sound (interface.md §14).
    if can_wait && (key(KeyCode::Key1) || (on_clock && clicked())) {
        cue(Cue::Button);
        game.begin_wait(1);
    }
    if can_wait && (key(KeyCode::Key4) || (on_clock && right_clicked())) {
        cue(Cue::Button);
        game.begin_wait(4);
    }
    // Community F4 (the held key, 0xc277d2): a wait without end; F5 ends it (`frame`'s
    // walking branch).
    if can_wait && held_key() == Some(KeyCode::F4) {
        game.begin_endless_wait();
    }
    // A wait drops the route drawn; the planned cell stays (0x4ae280).
    if game.waiting() {
        if let Some(p) = view.preview.as_mut() {
            p.1.clear();
        }
    }

    if view.debug {
        draw_debug(game, &cam);
    }
    let (bar, toggle_map, timed) = bottom_bar(game, message, view.minimap, time_buttons && !game.waiting());
    next = next.or(bar);
    // A time button pressed (its sound played): a wait, or the view's glide back to the hero
    // (0x4b9448: 2 or 8 ticks, 0x4af96c).
    match timed {
        Some(TimeButton::Wait1) => game.begin_wait(1),
        Some(TimeButton::Wait4) => game.begin_wait(4),
        Some(TimeButton::ShowHero) => view.centring = Some((get_time(), view.look.unwrap_or(game.display_pos()))),
        None => {}
    }
    if toggle_map {
        view.minimap = !view.minimap;
    }
    if view.minimap {
        if let Some(at) = minimap::window(game, assets.dt.as_ref(), cam.view, cam.world_rect(), surface_color) {
            view.look = Some(at);
        }
    }
    // The right button held (the left one up) shows the tooltip of the army or building
    // under it, as in the original (interface.md §7.4), unless it drags the map.
    let dragging = view.grab.is_some_and(|g| g.moved);
    let right_held = !input_blocked() && !dragging && is_mouse_button_down(MouseButton::Right) && !is_mouse_button_down(MouseButton::Left);
    if let Some(t) = hover_tooltip(game, &cam).filter(|_| idle && right_held && !on_minimap) {
        draw_tooltip(game, assets, &t);
    }
    if on_clock && can_wait {
        let hint = |key: &str, ours: &'static str| super::chrome::ui_text("GameMenu", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
        let left = hint("cp_Wait1Hour", n_("Wait 1 hour (the hero stands still)"));
        let right = hint("cp_Wait4Hour", n_("Wait 4 hours (the hero stands still)"));
        tooltip(&[(trf!("Left click: {left}", left), INK), (trf!("Right click: {right}", right), INK)]);
    }
    // A message for the screen this frame opens (a building's window, a battle) is that
    // screen's to show.
    if let Some(m) = message.take_if(|_| next.is_none()) {
        razdor::diag::play(&game.clock.label(), &format!("MESSAGE {m}"));
        view.toasts.push_back((m, get_time()));
        while view.toasts.len() > TOAST_MAX {
            view.toasts.pop_front();
        }
    }
    draw_toasts(&mut view.toasts, get_time());
    next
}

/// Seconds a message stays above the bar; it fades out over its last [`TOAST_FADE`].
const TOAST_SECONDS: f64 = 5.0;
const TOAST_FADE: f64 = 0.6;
/// Messages shown at once; a new one pushes the oldest out.
const TOAST_MAX: usize = 5;

/// The messages above the bar, the newest at the bottom, the older ones stacked above it;
/// the run-out ones are dropped.
fn draw_toasts(toasts: &mut VecDeque<(String, f64)>, now: f64) {
    toasts.retain(|(_, at)| now - at < TOAST_SECONDS);
    let k = super::chrome::k();
    let (size, h, gap) = ((18.0 * k).max(14.0), (30.0 * k).max(24.0), 6.0 * k);
    let cx = screen_width() / 2.0;
    let mut y = screen_height() - bar_h() - 14.0 * k - h;
    for (m, at) in toasts.iter().rev() {
        let left = TOAST_SECONDS - (now - at);
        let a = (left / TOAST_FADE).clamp(0.0, 1.0) as f32;
        let w = (measure(m, size).width + 40.0 * k).min(screen_width() - 16.0);
        draw_rectangle(cx - w / 2.0, y, w, h, Color { a: PANEL.a * a, ..PANEL });
        draw_rectangle_lines(cx - w / 2.0, y, w, h, 1.0, Color { a: 0.5 * a, ..super::chrome::SILVER });
        text_centered(m, cx, y + h * 0.5 + size * 0.36, size, Color { a, ..ACCENT });
        y -= h + gap;
    }
}

/// The events listed at `place` as lines "E12 «title» ×2" (`×n`: times fired).
fn debug_events(game: &Game, place: razdor::rules::events::Place) -> Vec<String> {
    let Some(e) = game.script() else { return Vec::new() };
    e.events_at(place)
        .iter()
        .map(|&id| {
            let title = e.event(id).map_or("", |ev| ev.title_text().trim());
            let times = e.times_fired(id);
            if times > 0 {
                format!("E{id} «{title}» ×{times}")
            } else {
                format!("E{id} «{title}»")
            }
        })
        .collect()
}

/// Razdor's debug overlay (F3): every event point and lantern with its id, radius and
/// events, the buildings' events, and a panel with the hero's and the pointer's cells and the
/// events of the place under the pointer. Drawn through the fog.
fn draw_debug(game: &Game, cam: &Camera) {
    use razdor::rules::events::Place;
    let size = 13.0;
    let label_rect = |lines: &[String], at: Vec2| {
        let w = lines.iter().map(|l| measure(l, size).width).fold(0.0, f32::max) + 8.0;
        Rect::new(at.x, at.y, w, lines.len() as f32 * (size + 2.0) + 4.0)
    };
    let label = |lines: &[String], at: Vec2, colour: Color| {
        let r = label_rect(lines, at);
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.7));
        for (i, l) in lines.iter().enumerate() {
            text(l, at.x + 4.0, at.y + (i as f32 + 1.0) * (size + 2.0) - 2.0, size, if i == 0 { colour } else { WHITE });
        }
    };
    // Labels in the way of one drawn already shrink to their first line, or are left out
    // (the panel shows the place under the pointer in full).
    let mut taken: Vec<Rect> = Vec::new();
    let mut place_label = |lines: &[String], at: Vec2, colour: Color| {
        for n in [lines.len(), 1] {
            let r = label_rect(&lines[..n], at);
            if !taken.iter().any(|t| t.overlaps(&r)) {
                label(&lines[..n], at, colour);
                taken.push(r);
                return;
            }
        }
    };
    let cell = cam.cell_size();
    let under = cam.tile_under_mouse();
    let mut place_under = None;
    for p in &game.world.points {
        let c = cam.cell_centre(p.tile);
        if !cam.view.contains(c) {
            continue;
        }
        let colour = if p.radius > 0 { Color::new(1.0, 0.85, 0.2, 1.0) } else { Color::new(0.3, 0.9, 1.0, 1.0) };
        if p.radius > 0 {
            draw_circle_lines(c.x, c.y, (p.radius as f32 + 0.5) * cell.x, 1.0, Color::new(colour.r, colour.g, colour.b, 0.5));
        }
        draw_rectangle_lines(c.x - cell.x / 2.0, c.y - cell.y / 2.0, cell.x, cell.y, 2.0, colour);
        draw_circle(c.x, c.y, 4.0, colour);
        let kind = if p.radius > 0 { trf!("lantern r{r}{lit}", r = p.radius, lit = if p.lit { tr(", lit") } else { "" }) } else { tr("event point").to_string() };
        let mut lines = vec![format!("#{} {kind} ({}, {})", p.id, p.tile.0, p.tile.1)];
        lines.extend(debug_events(game, Place::Point(p.id)));
        place_label(&lines, c + vec2(6.0, 6.0), colour);
        if under == Some(p.tile) {
            place_under = Some((lines, colour));
        }
    }
    for (l, loc) in game.world.locations.iter().enumerate() {
        if loc.id == 0 {
            continue;
        }
        let events = debug_events(game, Place::Building(loc.id));
        let over = under.is_some_and(|t| game.world.location_covering(t) == Some(l));
        if events.is_empty() && !over {
            continue;
        }
        let c = cam.cell_centre(loc.anchor);
        let mut lines = vec![format!("B{} {} ({}, {})", loc.id, loc.name.trim(), loc.anchor.0, loc.anchor.1)];
        lines.extend(events);
        let colour = Color::new(1.0, 0.55, 0.85, 1.0);
        if cam.view.contains(c) && lines.len() > 1 {
            place_label(&lines, c + vec2(-40.0, -cell.y), colour);
        }
        if over {
            place_under = Some((lines, colour));
        }
    }
    // The panel.
    let mut lines = vec![tr("Debug (F3)").to_string()];
    lines.push(trf!("Hero at ({x}, {y}), {time}", x = game.tile().0, y = game.tile().1, time = game.clock.label()));
    if let Some(t) = under {
        lines.push(trf!("Pointer at ({x}, {y})", x = t.0, y = t.1));
    }
    let panel = vec2(cam.view.x + 8.0, cam.view.y + 8.0);
    label(&lines, panel, Color::new(1.0, 0.4, 0.4, 1.0));
    if let Some((under, colour)) = place_under {
        label(&under, panel + vec2(0.0, lines.len() as f32 * (size + 2.0) + 10.0), colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// РК1's buildings get the original's tooltip frames (0x4cb18c): its two ruins, guarded
    /// and hostile, the red one, as the player saw «Развалины» should be; Bonitur's castle,
    /// friendly (2), the normal one; the bridges the neutral one; the villages and the church
    /// the normal one whoever holds them.
    #[test]
    fn rk1_buildings_take_the_originals_tooltip_frames() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap();
        let content = std::sync::Arc::new(razdor::rules::content::Content::from_dt(&dt));
        let g = Game::from_scenario(content, &m.load().unwrap(), HeroClass::Knight);
        let style = |name: &str| location_tip_style(g.world.locations.iter().find(|l| l.name == name).unwrap());
        assert_eq!(style("Развалины"), TipStyle::Hostile);
        assert_eq!(style("Древние руины"), TipStyle::Hostile);
        assert_eq!(style("Замок Бонитур"), TipStyle::Normal);
        assert_eq!(style("Замок Черной скалы"), TipStyle::Normal, "the hero's own");
        assert_eq!(style("Каменный мост"), TipStyle::Neutral);
        for name in ["Деревня Упокоище", "Церковь Трех Святых", "Деревня Васильки"] {
            assert_eq!(style(name), TipStyle::Normal, "{name}");
        }
        // Ruins nobody guards any more: neutral.
        let mut ruins = g.world.locations.iter().find(|l| l.name == "Развалины").unwrap().clone();
        ruins.cleared = true;
        assert_eq!(location_tip_style(&ruins), TipStyle::Neutral);
        // A castle not owned and at attitude 0 is hostile; owned, normal whatever its attitude.
        let mut castle = g.world.locations.iter().find(|l| l.name == "Замок Бонитур").unwrap().clone();
        castle.attitude = 0;
        assert_eq!(location_tip_style(&castle), TipStyle::Hostile);
        castle.owner = razdor::rules::world::Owner::Player;
        assert_eq!(location_tip_style(&castle), TipStyle::Normal);
    }

    /// A village the hero stepped on is his (0x4ad94c), but its tooltip still names the
    /// map's owner (0x4cb18c names the holder only for castles, forts and ruins); a castle
    /// he holds names him.
    #[test]
    fn a_captured_village_still_names_the_maps_owner() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap();
        let content = std::sync::Arc::new(razdor::rules::content::Content::from_dt(&dt));
        let mut g = Game::from_scenario(content, &m.load().unwrap(), HeroClass::Knight);
        g.set_hero_name("Иво");
        let owner = |g: &Game, name: &str| {
            let l = g.world.locations.iter().find(|l| l.name == name).unwrap();
            (l.owner_name.clone(), tooltip_owner(g, l))
        };
        for l in g.world.locations.iter_mut().filter(|l| l.name == "Деревня Васильки" || l.name == "Замок Бонитур") {
            l.owner = Owner::Player;
        }
        let (map, shown) = owner(&g, "Деревня Васильки");
        assert!(!map.is_empty());
        assert_eq!(shown, map);
        assert_eq!(owner(&g, "Замок Бонитур").1, "Иво");
    }

    /// The centre button's glide (0x4af96c): 900 ms, the cosine ease in the original's
    /// whole steps of 1/900.
    #[test]
    fn the_glide_is_the_originals_900_ms_cosine() {
        assert_eq!(glide_ease(0), 0.0);
        assert_eq!(glide_ease(-5), 0.0);
        assert_eq!(glide_ease(450), 0.5);
        assert_eq!(glide_ease(900), 1.0);
        assert_eq!(glide_ease(2000), 1.0);
        // round(900 × (1 − cos(π/9)) / 2) = round(27.14) = 27.
        assert_eq!(glide_ease(100), 27.0 / 900.0);
        // Slow at both ends, symmetric, never backwards.
        assert!((glide_ease(800) - (1.0 - glide_ease(100))).abs() < 1e-6);
        assert!((1..=900).all(|t| glide_ease(t) >= glide_ease(t - 1)));
    }

    /// A planning click keeps the view where it was scrolled; the walk brings it back to
    /// the hero, unless an event's places are being shown.
    #[test]
    fn the_view_follows_the_hero_only_while_he_walks() {
        let away = Some((100.0, 200.0));
        assert_eq!(camera_look(away, false, false), away);
        assert_eq!(camera_look(away, true, false), None);
        assert_eq!(camera_look(away, true, true), away);
        assert_eq!(camera_look(None, false, false), None);
    }

    #[test]
    fn a_shown_place_waits_for_its_own_window() {
        assert!(!shown_free(Some(6), [Some(6)].into_iter()), "the quest's message is still open");
        assert!(shown_free(Some(6), [Some(7), None].into_iter()), "other windows do not hold it");
        assert!(!shown_free(None, [None].into_iter()), "a place with no event waits for all");
        assert!(shown_free(None, std::iter::empty()));
    }

    #[test]
    fn a_building_window_steps_aside_for_the_flights_and_comes_back() {
        use BuildingFlight::*;
        // The quest's message open in the building: nothing due yet.
        assert_eq!(building_flight(true, false, false, false), Stay);
        // Its OK: the place is due, the window steps aside for the map (0x4af96c).
        assert_eq!(building_flight(true, false, true, false), StepAside);
        // On the map, the camera on its way: the window waits.
        assert_eq!(building_flight(false, true, false, true), Stay);
        // Back on the hero: the window comes back as it was.
        assert_eq!(building_flight(false, true, false, false), ComeBack);
        // A map with no window aside stays the map.
        assert_eq!(building_flight(false, false, false, false), Stay);
    }

    #[test]
    fn the_scroll_step_follows_the_scroll_speed() {
        // F = 0.5 at 100: 2 px per ms across, 1.375 down (62.5 cells a second both ways).
        assert_eq!(scroll_step(100, 16.0), (32.0, 22.0));
        assert_eq!(scroll_step(100, 1000.0), (2000.0, 1375.0));
        // F = 2 at 0: 0.5 px per ms; 5.5 rounds to the even 6, 2.5 to 2.
        assert_eq!(scroll_step(0, 16.0), (8.0, 6.0));
        assert_eq!(scroll_step(0, 5.0), (2.0, 2.0));
        // F = 1.25 at 50.
        assert_eq!(scroll_step(50, 10.0), (8.0, 6.0));
    }

    #[test]
    fn the_right_button_drags_the_map_or_clicks() {
        // Held still: the tooltip; let go: a click (Razdor's stop).
        let mut g = Grab { start: vec2(100.0, 100.0), from: vec2(10.0, 10.0), moved: false };
        assert_eq!(g.step(vec2(102.0, 101.0), true, 32.0), GrabStep::Held);
        assert_eq!(g.step(vec2(102.0, 101.0), false, 32.0), GrabStep::Click);
        // Moved past the threshold it drags: the ground follows the mouse, the camera goes
        // the other way, and it stays a drag back at the start.
        let mut g = Grab { start: vec2(100.0, 100.0), from: vec2(10.0, 10.0), moved: false };
        assert_eq!(g.step(vec2(164.0, 100.0), true, 32.0), GrabStep::Drag(vec2(8.0, 10.0)));
        assert_eq!(g.step(vec2(100.0, 100.0), true, 32.0), GrabStep::Drag(vec2(10.0, 10.0)));
        assert_eq!(g.step(vec2(100.0, 100.0), false, 32.0), GrabStep::Dropped, "a drag is no click");
    }

    /// A minimized window (1 pixel high on Windows) left the map view −39 high, and the
    /// fog's border panicked on `clamp` (crash report, 0.3.2).
    #[test]
    fn a_minimized_window_leaves_an_empty_map_view() {
        let v = map_area_in(1920.0, 1.0, 40.0);
        assert_eq!((v.w, v.h), (1920.0, 0.0));
        assert_eq!(map_area_in(1280.0, 800.0, 40.0).h, 760.0);
    }

    #[test]
    fn leaving_the_map_drops_a_right_press() {
        let mut view = MapView { grab: Some(Grab { start: vec2(1.0, 1.0), from: vec2(0.0, 0.0), moved: false }), ..MapView::default() };
        view.drop_grab();
        assert_eq!(view.grab, None, "no right click (a stop) waits for the map's return");
    }

    #[test]
    fn razdor_keys_on_the_map_are_back() {
        // Esc closes the minimap first.
        assert_eq!(esc_on_map(true), BarButton::Map);
        assert_eq!(esc_on_map(false), BarButton::Menu);
        // Zoom, minimap, Tab, music, help, language and the quick save leave a walk alone;
        // other keys cut it as in the original.
        for k in [KeyCode::Equal, KeyCode::Minus, KeyCode::KpAdd, KeyCode::KpSubtract, KeyCode::M, KeyCode::Tab, KeyCode::N, KeyCode::F1, KeyCode::F2, KeyCode::F5] {
            assert!(!cuts_walk(k), "{k:?}");
        }
        for k in [KeyCode::Space, KeyCode::J, KeyCode::A, KeyCode::B, KeyCode::Escape, KeyCode::Left, KeyCode::Key1] {
            assert!(cuts_walk(k), "{k:?}");
        }
    }

    #[test]
    fn razdor_stop_cuts_a_walk_and_ends_a_wait() {
        use std::sync::Arc;
        use razdor::rules::content::Content;
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let here = g.tile();
        let target = (here.0 + 4, here.1);
        assert!(g.set_destination(target), "the demo has open ground east of the start");
        razdor_stop(&mut g);
        assert_eq!(g.path.len(), 1, "he finishes the step under way");
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.begin_wait(4);
        assert!(g.waiting());
        razdor_stop(&mut g);
        assert!(!g.waiting());
        g.begin_endless_wait();
        razdor_stop(&mut g);
        assert!(!g.endless_waiting());
    }

    /// A meeting or an attack whose army has left the map by the time it is read (Razdor
    /// 0.3.10 crashed on one): it speaks of "an army".
    #[test]
    fn a_meeting_with_an_army_gone_from_the_map_is_told_without_its_name() {
        use std::sync::Arc;
        use razdor::rules::content::Content;
        let g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let gone = g.world.armies.len();
        assert_eq!(describe(&Event::Met(gone), &g), Some(trf!("A meeting on the road: {who} lets you pass.", who = tr("An army"))));
        assert_eq!(describe(&Event::Encounter(gone), &g), Some(tr("An army attacks!").to_string()));
    }

    #[test]
    fn a_planned_cell_whose_route_was_dropped_plans_again() {
        let (here, a, b, c) = ((1, 1), (3, 1), (9, 9), (4, 1));
        let mut planned = None;
        assert_eq!(plan_click(&mut planned, a, here, || vec![(2, 1), a]), MapClick::Planned);
        assert_eq!(plan_click(&mut planned, a, here, || unreachable!("no planning")), MapClick::SetOff);
        // A cell out of reach drops the route; a click on the planned cell plans it again
        // (the original's bug left its clicks doing nothing until a click elsewhere).
        assert_eq!(plan_click(&mut planned, b, here, Vec::new), MapClick::OutOfReach);
        assert_eq!(planned, Some((a, vec![], here)));
        assert_eq!(plan_click(&mut planned, a, here, || vec![(2, 1), a]), MapClick::Planned);
        assert_eq!(plan_click(&mut planned, a, here, Vec::new), MapClick::SetOff);
        // A click elsewhere plans too.
        assert_eq!(plan_click(&mut planned, c, here, || vec![(2, 1), (3, 1), c]), MapClick::Planned);
    }
}
