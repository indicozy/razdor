//! The world map: terrain, objects, buildings, armies, the party, its clock and money.
//!
//! With a Discord Times install the original terrain textures, map objects, buildings and
//! map figures are drawn (decoded at runtime by [`DtArt`]); otherwise coloured cells and
//! simple shapes. Only the visible cells are drawn, so 200×200 maps stay fast.

use razdor::local;
use std::collections::VecDeque;

use crate::ui::input::{get_keys_pressed, is_mouse_button_down};
use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::frame_probe::{self, Kind, Stage};
use super::gpu_probe::gpu_wait;
use super::photo;
use razdor::rules::ai::{Clash, Combatant};
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::clock::duration_label;
use razdor::rules::game::{Event, Foe, Game};
use razdor::rules::idle;
use razdor::rules::magic::{spell_lines, CastOutcome, CastTarget, SpellLine};
use razdor::dt::cards::Card;
use razdor::rules::map::{object_class, Decoration, Grid, Tile, TileMap, ROAD};
use razdor::rules::world::{Army, Location, LocationKind, Owner, Troop};

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::building_view::BuildingView;
use super::gestures::SwipeKind;
use super::cursor::{self, BuildingHover, CellArmy, MapHover, Shape};
use super::dialog::Dialog;
use super::dt_art::{Atlas, DtArt};
use super::daylight;
use super::game_bar::{self, BarButton, Look, TimeButton};
use super::keymap::{self, Action};
use super::map_scale;
use super::minimap;
use super::mist;
use super::terrain;
use super::water;
use super::saves::{self, Back, LoadView, SaveView};
use super::story;
use super::widgets::*;
use super::view25::{self, TiltCamera};
use super::Screen;
use macroquad::camera::set_camera;
use macroquad::window::get_internal_gl;

/// Screen pixels per world unit (one cell width) at zoom 1 on the original's 1024×768
/// screen: its 32 px cells.
const PX: f32 = 32.0;

/// The time a frame may spend on the AI's moves of a walk or a wait tick (`Game::tick_within`).
/// A slow step (a big map, many armies) takes several frames; the screen keeps drawing meanwhile.
const AI_FRAME_BUDGET: std::time::Duration = std::time::Duration::from_millis(8);

/// The map's scale at zoom 1, against the original's 32 px cells: the interface scale the
/// player chose in the settings, so a bigger interface gives a closer map (Razdor's choice,
/// the owner's request; the original shows a fixed 1024×682 view).
fn map_scale() -> f32 {
    map_scale_for(super::display::chosen_scale(), super::chrome::fit_k())
}

/// [`map_scale`] for the settings' interface scale `chosen` (`0`: Auto) in a window whose own
/// fit is `fit` (`chrome::fit_k`). The cells are drawn as large as the original's pictures
/// in the windows (`chrome::k` is against the 960×720 footage, 0.9375 of the original's
/// 1024 px). Unlike the interface, a chosen scale is not cut down to the window's fit: the
/// map needs no room for windows and follows the setting, not the window's shape. Auto is
/// the fit, the interface's own scale then.
fn map_scale_for(chosen: f32, fit: f32) -> f32 {
    (if chosen > 0.0 { chosen } else { fit }) * 0.9375
}
/// Sail colour of the hero's own ship.
const HERO_SAIL: Color = Color::new(0.35, 0.8, 0.45, 1.0);
fn bar_h() -> f32 {
    // A trailer run draws no bar: the map takes the whole window (`ui::trailer`). Photo mode none either.
    if super::trailer::hides_ui() || photo::active() {
        return 0.0;
    }
    super::chrome::bar_top_height()
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

/// The player's own map zoom (the settings, Razdor's): the zoom the wheel steps from and the
/// 0 key comes back to, and whether the wheel and the +/- keys may change it.
static HOME_ZOOM: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x3f80_0000);
static ZOOM_LOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// «Продолжать карту за краем» (`Settings::map_extended`): off, the black past the map's edges as
/// in the original; on, the ground goes on there ([`extend_edge`]).
static EXTEND_EDGE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The zoom range the settings' slider keeps to, and the home zoom is clamped to.
pub const ZOOM_RANGE: (f32, f32) = (0.4, 2.0);

/// The wheel's, pinch's and +/- keys' highest zoom (Razdor's, the owner's request 2026-10-30:
/// closer for tapping on phones), above the settings' range.
pub const ZOOM_WHEEL_MAX: f32 = 4.0;

/// Takes the setting «Продолжать карту за краем» in (every frame).
pub fn set_edge_pref(on: bool) {
    EXTEND_EDGE.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the map's ground goes on past its edges (a short strip, then the animated fog), instead
/// of black (the original's). Razdor's setting, off by default: the visual only, no rule or path.
fn extend_edge() -> bool {
    EXTEND_EDGE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Takes the settings' map zoom and lock in (every frame).
pub fn set_zoom_prefs(home: f32, locked: bool) {
    let home = if home.is_finite() { home.clamp(ZOOM_RANGE.0, ZOOM_RANGE.1) } else { 1.0 };
    HOME_ZOOM.store(home.to_bits(), std::sync::atomic::Ordering::Relaxed);
    ZOOM_LOCKED.store(locked, std::sync::atomic::Ordering::Relaxed);
}

fn home_zoom() -> f32 {
    f32::from_bits(HOME_ZOOM.load(std::sync::atomic::Ordering::Relaxed))
}

/// The lowest wheel zoom (Razdor's, for fun): far enough out that the whole map fits the
/// view, and never above ZOOM_RANGE.0. `map_w` and `map_h_px` are the map's size in screen
/// pixels at zoom 1; a zero or non-finite size, or an empty view, gives ZOOM_RANGE.0.
fn zoom_floor(map_w: f32, map_h_px: f32, view: Rect) -> f32 {
    let sized = map_w > 0.0 && map_h_px > 0.0 && map_w.is_finite() && map_h_px.is_finite();
    let viewed = view.w > 0.0 && view.h > 0.0 && view.w.is_finite() && view.h.is_finite();
    if !sized || !viewed {
        return ZOOM_RANGE.0;
    }
    (view.w / map_w).min(view.h / map_h_px).min(ZOOM_RANGE.0).max(0.01)
}

/// The zoom `steps` wheel notches from `home` (×1.1 each), within `lo` (the wheel's lowest,
/// see [`zoom_floor`]) and ZOOM_WHEEL_MAX; locked, always `home`.
fn zoom_at(home: f32, steps: i32, locked: bool, lo: f32) -> f32 {
    if locked {
        home
    } else {
        (home * 1.1f32.powi(steps)).clamp(lo, ZOOM_WHEEL_MAX)
    }
}

/// `steps` brought into the range where every wheel step still changes the zoom (Razdor's):
/// from the lowest `s` with `home * 1.1^s >= lo` to the highest with `home * 1.1^s <=
/// ZOOM_WHEEL_MAX`, the same limits [`step_zoom`] keeps. The floor moves with the view (a
/// rotated phone) and the home zoom with the settings, so the steps are refitted each frame;
/// a stale count would sit past the new limit and the zoom would stop following the steps.
fn fit_steps(home: f32, steps: i32, lo: f32) -> i32 {
    if !(home.is_finite() && home > 0.0 && lo.is_finite() && lo > 0.0) {
        return steps;
    }
    let at = |s: i32| home * 1.1f32.powi(s);
    let mut low = ((lo / home).ln() / 1.1f32.ln()).ceil() as i32;
    while at(low) < lo {
        low += 1;
    }
    while at(low - 1) >= lo {
        low -= 1;
    }
    let mut high = ((ZOOM_WHEEL_MAX / home).ln() / 1.1f32.ln()).floor() as i32;
    while at(high) > ZOOM_WHEEL_MAX {
        high -= 1;
    }
    while at(high + 1) <= ZOOM_WHEEL_MAX {
        high += 1;
    }
    if low > high {
        return steps;
    }
    steps.clamp(low, high)
}

/// `steps` moved by `by`, kept where it still changes the zoom within `lo` and ZOOM_WHEEL_MAX.
fn step_zoom(home: f32, steps: i32, by: i32, lo: f32) -> i32 {
    let next = steps + by;
    let z = home * 1.1f32.powi(next);
    if (z < lo && by < 0) || (z > ZOOM_WHEEL_MAX && by > 0) {
        steps
    } else {
        next
    }
}

/// A camera's zoom and where it looked (`None`: the hero).
type CamState = (f32, Option<(f32, f32)>);

local! {
    /// The map's camera as last drawn (its zoom and where it looked, `None` the hero): the
    /// windows over the map draw it so, not re-centred and re-zoomed under them.
    static LAST_CAM: std::cell::Cell<Option<CamState>> = const { std::cell::Cell::new(None) };
    /// The screen's side of the armies' idle animation ([`razdor::rules::idle`]), by uid.
    static IDLE: std::cell::RefCell<std::collections::HashMap<u32, IdleSprite>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// An army's idle stamp (the frame clock, ms, when its idle animation last restarted: the
/// load, 0x4b4b38, or a stop, 0x4ad8a0), the restart it belongs to, and the frame the idle
/// pass last set (its sprite keeps it while the hero walks or waits).
#[derive(Clone, Copy)]
struct IdleSprite {
    restarts: u32,
    stamp_ms: i64,
    frame: u32,
}

/// The frame clock the idle pass reads (0x4f1c34), in ms.
fn clock_ms() -> i64 {
    (crate::ui::clock::now() * 1000.0) as i64
}

/// Stamps every army the first time the screen draws the map (a load stamps them all at
/// once) and an army again when a stop restarted its animation.
fn stamp_idle(game: &Game, now: i64) {
    IDLE.with(|m| {
        let mut m = m.borrow_mut();
        for a in &game.world.armies {
            let restarts = a.mind.idle_restarts;
            let e = m.entry(a.uid).or_insert(IdleSprite { restarts, stamp_ms: now, frame: 0 });
            if e.restarts != restarts {
                *e = IdleSprite { restarts, stamp_ms: now, ..*e };
            }
        }
    });
}

/// The frame of a standing army (`None`: its walk frame stays). While the map is idle (the
/// hero neither walks nor waits, 0x68dc62) the world frame's idle pass (0x4c868c) sets the
/// sprite of every army in no building from its stamp and offset; otherwise the sprite keeps
/// the frame the pass last set, unless the army walks.
fn idle_sprite_frame(game: &Game, a: &Army, now: i64) -> Option<u32> {
    if a.mind.standing.is_some() {
        return None;
    }
    let idle = !game.moving() && !game.waiting();
    IDLE.with(|m| {
        let mut m = m.borrow_mut();
        let e = m.get_mut(&a.uid)?;
        if idle {
            let since = now - e.stamp_ms;
            e.frame = if a.sails() { idle::ship_idle_frame(since, a.mind.idle_offset) } else { idle::idle_frame(a.figure, since, a.mind.idle_offset) };
            Some(e.frame)
        } else {
            game.army_walk_frame(a).is_none().then_some(e.frame)
        }
    })
}

/// World-map view state kept between frames.
pub struct MapView {
    pub zoom: f32,
    /// Wheel notches from the settings' zoom (0: that zoom, the 0 key's).
    zoom_steps: i32,
    /// The minimap window is open.
    pub minimap: bool,
    /// Razdor's debug overlay (F3): event points, lanterns and the events of each place.
    pub debug: bool,
    /// Where the camera looks when moved by the minimap (world units); `None` follows the hero.
    pub look: Option<(f32, f32)>,
    /// Places the scenario's events have shown (lanterns, shown armies), first in line: the
    /// camera flies to each in turn and its uncovered cells fade in from the fog.
    shows: VecDeque<Showing>,
    /// Presentation time for place reveals; a long frame must not skip the camera glide.
    show_clock: f64,
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
    /// The map's pointer (interface.md §7.2), kept between frames as the original keeps it
    /// (0xae1d7c): the hover changes it only on entering another cell.
    pointer: Shape,
    /// The cell the pointer was last worked out for (0x68dca0/a4); `None` looks again.
    pointer_cell: Option<Tile>,
    /// The map was busy last frame (the clock was up).
    was_busy: bool,
    /// What the hover marks on the cell it last worked out ([`MapHoverLook`]).
    hover: Option<MapHoverLook>,
    /// The building last outlined and when the pointer was last near it: the outline stays
    /// while the pointer is within a cell of its footprint, and [`OUTLINE_GRACE`] after
    /// that, so moving over its picture does not make it blink.
    outlined: Option<(usize, f64)>,
    /// The battles between map parties shown on the map (Razdor's): their cell and when each
    /// began; the mark fades out over [`CLASH_SECS`].
    clash_marks: VecDeque<(Tile, f64)>,
}

/// How long a building stays outlined after the pointer leaves it, in seconds.
const OUTLINE_GRACE: f64 = 0.3;

/// The hover's marks on the map: the original's animated ring on the hovered cell
/// ([`cursor::hover_ring`]) and, Razdor's choice (a player's wish), the outline of the
/// building under the pointer. The original only rings the cell, under the building's
/// picture.
#[derive(Clone, Copy, Debug, PartialEq)]
struct MapHoverLook {
    cell: Tile,
    ring: Option<(&'static str, Color)>,
    building: Option<usize>,
}

impl Default for MapView {
    fn default() -> Self {
        MapView { zoom: 1.0, zoom_steps: 0, minimap: false, debug: false, look: None, shows: VecDeque::new(), show_clock: 0.0, returning: None, opening: None, spell_fx: VecDeque::new(), preview: None, last_frame_ms: None, back_to: None, centring: None, grab: None, toasts: VecDeque::new(), pointer: Shape::Arrow, pointer_cell: None, was_busy: false, hover: None, outlined: None, clash_marks: VecDeque::new() }
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
    /// The press with the pointer at `at` and the button `down` or not; `to_world` turns a
    /// screen move into world units ([`Camera::screen_to_world_delta`]).
    fn step(&mut self, at: Vec2, down: bool, to_world: impl Fn(Vec2) -> Vec2) -> GrabStep {
        if !down {
            return if self.moved { GrabStep::Dropped } else { GrabStep::Click };
        }
        let d = at - self.start;
        self.moved |= d.length() > GRAB_START;
        if self.moved {
            GrabStep::Drag(self.from - to_world(d))
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
    use macroquad::miniquad::CursorIcon;
    let m = Vec2::from(pointer());
    let Some(mut grab) = view.grab else {
        if right_clicked() && !off_map && cam.over_map(m) && view.shows.is_empty() && !game.moving() {
            view.grab = Some(Grab { start: m, from: view.look.unwrap_or(game.display_pos()).into(), moved: false });
        }
        return false;
    };
    let down = is_mouse_button_down(MouseButton::Right) && !input_blocked();
    match grab.step(m, down, |d| cam.screen_to_world_delta(d)) {
        GrabStep::Held => view.grab = Some(grab),
        GrabStep::Drag(at) => {
            // The drag is Razdor's: the system's hand, not one of the original's pointers.
            super::cursor::system(CursorIcon::Pointer);
            view.look = Some(Camera::looking_at(game, view.zoom, at.into()).centre());
            view.grab = Some(grab);
        }
        GrabStep::Click => {
            view.grab = None;
            return !input_blocked();
        }
        GrabStep::Dropped => view.grab = None,
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
    const KEPT: [Action; 6] = [Action::Minimap, Action::CentreHero, Action::Music, Action::Help, Action::Language, Action::QuickSave];
    // Q and E turn the 2.5D view only (visual): on the flat map they cut a walk as any other key.
    let turns = tilt_view() && matches!(k, KeyCode::Q | KeyCode::E);
    !(turns || matches!(k, KeyCode::Equal | KeyCode::Minus | KeyCode::KpAdd | KeyCode::KpSubtract) || KEPT.into_iter().any(|a| keymap::key(a) == k))
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

/// Where the mouse at `at` on a screen of `size` scrolls the map (4cd0af): within `edge` of a
/// side that way, a corner both ways; off the screen not at all.
fn edge_dir(at: (f32, f32), size: (f32, f32), edge: f32) -> Vec2 {
    let ((mx, my), (w, h)) = (at, size);
    if !(0.0..=w).contains(&mx) || !(0.0..=h).contains(&my) {
        return Vec2::ZERO;
    }
    let x = if mx < edge { -1.0 } else if mx > w - edge { 1.0 } else { 0.0 };
    let y = if my < edge { -1.0 } else if my > h - edge { 1.0 } else { 0.0 };
    vec2(x, y)
}

/// A one-finger swipe over the idle map moves the view with the finger (gestures, Razdor's):
/// `slide` is the finger's move in screen pixels, the map following it 1:1 (finger right, the
/// map right: the view goes left), clamped to the map as the scroll is.
fn pan(game: &Game, view: &mut MapView, slide: (f32, f32)) {
    if slide == (0.0, 0.0) || input_blocked() || !view.shows.is_empty() {
        return;
    }
    let look = view.look.unwrap_or(game.display_pos());
    let cam = Camera::looking_at(game, view.zoom, look);
    let at = Vec2::from(look) - cam.screen_to_world_delta(vec2(slide.0, slide.1));
    view.look = Some(Camera::looking_at(game, view.zoom, at.into()).centre());
}

/// The original's scrolling of the idle map (interface.md §7.6): the held arrow key (only one
/// at a time: the original keeps only the last key down) and the mouse within 5 px of an
/// edge of the screen (the bar's lower edge included; a corner both ways) each move the
/// view by the frame's step. The camera then stays there until something moves it. A
/// gamepad's right stick (`pad`, Razdor's) scrolls it too, as far as it is pushed.
fn scroll(game: &Game, view: &mut MapView, dt_ms: i64, pad: (f32, f32)) {
    if input_blocked() || !view.shows.is_empty() {
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
    dir += Vec2::from(pad);
    let (mx, my) = crate::ui::widgets::pointer();
    let (w, h) = (screen_width(), screen_height());
    let edge = EDGE * map_scale();
    // The open minimap does not stop the edges (the original's minimap is always on screen
    // and its map still scrolls, 4cd0af); only the pointer on the minimap itself does, so it
    // can be clicked at the screen's edge.
    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    let on_minimap = on_minimap_pointer(game, view, cam.view);
    if !on_minimap {
        let edge_dir = edge_dir((mx, my), (w, h), edge);
        // The pointer left the window (Razdor's: macroquad keeps its last place, at the edge it
        // crossed): asked of the system only while that place is at an edge.
        if edge_dir != Vec2::ZERO && !super::display::pointer_outside() {
            dir += edge_dir;
        }
    }
    if dir == Vec2::ZERO {
        return;
    }
    let (sx, sy) = scroll_step(scroll_speed(), dt_ms as f64);
    // Original pixels to world units: a cell is 32 px across, its row 22 px down.
    let rh = game.world.map.grid.row_height();
    let step = cam.turn_ground(dir * vec2(sx as f32 / PX, sy as f32 / 22.0 * rh) / view.zoom);
    let at = Vec2::from(view.look.unwrap_or(game.display_pos())) + step;
    // Where the camera can really look: at the map's border it stops, and so does the
    // scroll, so turning back moves the view at once.
    view.look = Some(Camera::looking_at(game, view.zoom, at.into()).centre());
}

/// Seconds the camera takes to reach a shown place, then the uncovered area takes to fade
/// in, then the view rests there before the next place.
const SHOW_PAN: f64 = 0.8;
const SHOW_FADE: f64 = 1.2;

/// A place an event showed: the cells it uncovered stay dark until the camera is there
/// (after the event's message is read), then fade in.
struct Showing {
    /// World position of the place.
    at: (f32, f32),
    /// The event that showed it: shown once its window is closed.
    event: Option<u16>,
    /// The fog as it was before the place was uncovered (soft edges and all), one darkness
    /// per cell, and the texture drawn over the fog layer until the reveal, so the place pops
    /// out of the dark without any hint before. The texture holds only the darkness the fog
    /// layer lacks (`minimap::reveal_overlay` against `after`): clear wherever the place's
    /// cells did not change the fog, so the rest of the map keeps its shade (0x49c8f0).
    before: Vec<u8>,
    /// The fog layer's darkness once the place was uncovered.
    after: Vec<u8>,
    mask: Option<Texture2D>,
    /// Reused upload buffer and distances for only the texels the reveal can change.
    mask_image: Option<std::cell::RefCell<Image>>,
    reveal_cells: Vec<(usize, f32)>,
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
        let mut after = Vec::new();
        let reach = shown.cells.iter().map(|&t| (Vec2::from(map.center(t)) - Vec2::from(at)).length()).fold(0.0, f32::max) + SHOW_RIM + 1.0;
        let mut mask_image = None;
        let mut reveal_cells = Vec::new();
        let mask = (!shown.cells.is_empty() && fog.w > 0 && fog.h > 0).then(|| {
            let fresh: std::collections::HashSet<Tile> = shown.cells.iter().copied().collect();
            before = minimap::darkness_of(fog.w, fog.h, |t| fog.explored(t) && !fresh.contains(&t));
            after = minimap::darkness(fog);
            let rgba: Vec<u8> = before.iter().zip(&after).flat_map(|(&b, &a)| [0, 0, 0, minimap::reveal_overlay(b, a)]).collect();
            let t = Texture2D::from_rgba8(fog.w as u16, fog.h as u16, &rgba);
            t.set_filter(FilterMode::Linear);
            for (i, &alpha) in rgba.iter().skip(3).step_by(4).enumerate() {
                if alpha == 0 {
                    continue;
                }
                let tile = (i as i32 % fog.w, i as i32 / fog.w);
                let distance = (Vec2::from(map.center(tile)) - Vec2::from(at)).length();
                if distance <= reach + SHOW_RIM {
                    reveal_cells.push((i, distance));
                }
            }
            mask_image = Some(std::cell::RefCell::new(Image { bytes: rgba, width: fog.w as u16, height: fog.h as u16 }));
            t
        });
        Showing { at, event: shown.event, before, after, mask, mask_image, reveal_cells, reach, started: None, radius: shown.radius as f32 + 0.5 }
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

    /// The darkness the reveal still draws over cell `t` of a `w`-wide map at `now` (0 to 1), as
    /// [`Camera::draw_showing_flat`] has it in its mask; None once the place is open or with no
    /// mask.
    fn alpha_at(&self, w: i32, t: Tile, now: f64) -> Option<f32> {
        if self.mask.is_none() || self.progress(now) >= 1.0 {
            return None;
        }
        let image = self.mask_image.as_ref()?.borrow();
        let i = t.1 as usize * w as usize + t.0 as usize;
        Some(image.bytes.get(i * 4 + 3).copied().unwrap_or(0) as f32 / 255.0)
    }
}

impl MapView {
    pub fn reset(&mut self) {
        self.look = None;
        LAST_CAM.with(|c| c.set(None));
    }

    /// The camera looks at `at` (world units) and stays there: the opening fog view, the event
    /// views and the glide back to the hero are dropped. Only the trailer's stills
    /// (`RAZDOR_TRAILER_TARGET`).
    pub fn aim_at(&mut self, at: (f32, f32)) {
        self.look = Some(at);
        self.forget_shows();
        self.centring = None;
    }

    /// The map is left (a window, a battle): a right-button press under way ends with it, so
    /// the press is no right click (a stop) when the map comes back.
    pub fn drop_grab(&mut self) {
        self.grab = None;
    }

    /// A game was loaded: the places the old one was about to show are dropped.
    pub fn forget_shows(&mut self) {
        IDLE.with(|m| m.borrow_mut().clear());
        // A game was loaded or started: its fog is shown as it is, with no fade of the whole map.
        minimap::snap_fog();
        self.shows.clear();
        self.returning = None;
        self.opening = None;
        self.spell_fx.clear();
        self.preview = None;
        self.back_to = None;
        WAKES.with_borrow_mut(|w| w.clear());
    }

    /// A map starts: the fog opens around the hero (0x4af83c), the places of the last game
    /// forgotten.
    pub fn open_around_hero(&mut self, game: &Game) {
        self.forget_shows();
        let (at, r) = (game.tile(), game.sight_radius() + 1);
        let cells = (at.1 - r..=at.1 + r).flat_map(|y| (at.0 - r..=at.0 + r).map(move |x| (x, y))).filter(|&t| game.fog.explored(t)).collect();
        let mut s = Showing::new(game, &razdor::rules::game::Shown { at, cells, radius: r, event: None });
        // No flight: the fog opens at once.
        s.started = Some((self.show_clock - SHOW_PAN, s.at));
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
#[derive(Clone, Debug)]
enum SpellFx {
    /// To this army (`None`: the hero's), from where the camera was when it set off.
    Look { army: Option<u32>, started: Option<(f64, (f32, f32))> },
    /// The effect `art` over this army (`None`: the hero's).
    Effect { army: Option<u32>, art: &'static str, started: Option<f64> },
    /// The spell's own effects (`Effect1..3`) over this army (`None`: the hero's), as the
    /// original plays them (0x4af2f8).
    Layers { army: Option<u32>, layers: Vec<FxLayer>, started: Option<f64> },
}

/// One of a spell's `EffectN`: its animation, when it starts and how long it runs (ms), its
/// tint, its frame's bottom below the cell's centre and its scale (× 1000).
#[derive(Clone, Debug)]
struct FxLayer {
    /// Which of `Effect1..3` (1 under every sprite, 2 behind the figure, 3 in front of it).
    layer: u8,
    art: String,
    start_ms: f64,
    duration_ms: f64,
    tint: Color,
    y_offset: f32,
    scale: f32,
}

/// The side of a spell layer's quad in the original's pixels, before its scale: the loader
/// 0x4e0c60 builds every quad 128×128 (0x48c4c0) whatever the frames' size.
const FX_QUAD: f32 = 128.0;

impl FxLayer {
    fn of(layer: u8, e: &razdor::dt::data::SpellEffect) -> FxLayer {
        let c = |v: i32| v.clamp(0, 255) as f32 / 255.0;
        FxLayer {
            layer,
            art: format!("Spells/{}.ugs", e.file),
            start_ms: e.start_ms.max(0) as f64,
            duration_ms: e.duration_ms.max(1) as f64,
            tint: Color::new(c(e.rgb[0]), c(e.rgb[1]), c(e.rgb[2]), 1.0),
            y_offset: e.y_offset as f32,
            scale: e.scale_milli.max(1) as f32 / 1000.0,
        }
    }

    /// The frame shown `ms` after the cast, if the layer shows then: from its start to its
    /// end, both included, frame `(ms − start)·49 div length` of the 50 (0x4af2f8).
    fn frame(&self, ms: f64) -> Option<usize> {
        let (start, len) = (self.start_ms as i64, self.duration_ms as i64);
        let t = ms as i64;
        (t >= start && t <= start + len).then(|| ((t - start) * 49 / len.max(1)).clamp(0, 49) as usize)
    }

    /// The layer's quad, in the original's pixels from the target cell's centre: the loader
    /// 0x4e0c60 moves the 128×128 quad by (−64, Y − 128) (0x48c6b0, Y = the 6th field), scales
    /// it by the 7th field / 1000 and adds (16, 11), and the renderer adds the cell's top-left
    /// (0x4c9368, 0x4c9dd7, 0x4ca115). So Y puts the frame's bottom row Y·scale px below the
    /// centre: a larger Y draws lower.
    fn quad(&self) -> Rect {
        let side = FX_QUAD * self.scale;
        Rect::new(-FX_QUAD / 2.0 * self.scale, (self.y_offset - FX_QUAD) * self.scale, side, side)
    }
}

/// How long a spell's effects run: the latest start plus its length (0x4af2f8).
fn layers_ms(layers: &[FxLayer]) -> f64 {
    layers.iter().map(|l| l.start_ms + l.duration_ms).fold(0.0, f64::max)
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

local! {
    /// What the spell book and the events asked the map to show, taken by its next frame.
    static SPELL_FX: std::cell::RefCell<Vec<SpellFx>> = const { std::cell::RefCell::new(Vec::new()) };
}

local! {
    /// A world spell picked in the book, waiting for its target on the map: the book's click
    /// closes it and sets the cast flag (0x4c2e34, [66c0b9] = 1).
    static TARGETING: std::cell::Cell<Option<u32>> = const { std::cell::Cell::new(None) };
}

/// The map asks for the target of enemy spell `spell` (0x4c2f39).
pub(super) fn begin_targeting(spell: u32) {
    TARGETING.with(|t| t.set(Some(spell)));
}

fn targeting() -> Option<u32> {
    TARGETING.with(|t| t.get())
}

fn end_targeting() {
    TARGETING.with(|t| t.set(None));
}

/// What the targeting hint says (0x47ea80): no target yet, the army aimed at, or a friend
/// that cannot be cursed.
fn spell_target_hint(game: &Game, spell: u32, aim: Option<Result<u32, ()>>) {
    let k = super::chrome::k();
    let info = |key: &str, ours: &'static str| super::chrome::ui_text("Info", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
    let Some(s) = game.spell(spell) else { return };
    let mut lines = vec![(s.name.clone(), WHITE)];
    match aim {
        None => lines.push((info("SpellTargetHint", n_("Pick a target for the spell!")), super::chrome::GOLD)),
        Some(Ok(uid)) => {
            lines.push((info("SpellHint", n_("The spell's target is picked -")), super::chrome::BLUE_TEXT));
            let name = game.world.armies.iter().find(|a| a.uid == uid).map_or(String::new(), |a| a.shown_name().to_string());
            lines.push((name, super::chrome::BLUE_TEXT));
        }
        Some(Err(())) => {
            // The original's text breaks its line with `#\`.
            let refusal = info("SpellFriendHint", n_("Friendly characters cannot be the target of evil spells!"));
            lines.extend(refusal.split("#\\").map(|l| (l.trim().to_string(), super::chrome::RED_TEXT)));
        }
    }
    lines.push((info("SpellCancelHint", n_("Cancel: right click or ESC")), Color::new(1.0, 0.92, 0.6, 1.0)));
    // Shown whatever option 6 says: the game always shows the spell-target hints (interface.md §10).
    hint_box_always(&lines, 510.0 * k);
}

/// The camera goes to `target` (a spell's): queued for the map.
pub(super) fn look_at(target: CastTarget) {
    let army = match target {
        CastTarget::Own => None,
        CastTarget::Army(uid) => Some(uid),
    };
    SPELL_FX.with(|q| q.borrow_mut().push(SpellFx::Look { army, started: None }));
}

/// Spell `s` lands on `target`: its effect over the army, queued for the map. The spell's
/// own `Effect1..3` when it has them, else a stand-in by its school.
pub(super) fn spell_effect(s: &razdor::rules::content::SpellDef, target: CastTarget) {
    let layers: Vec<FxLayer> = (1u8..)
        .zip(&s.effects)
        .filter_map(|(n, e)| Some((n, e.as_ref()?)))
        .filter(|(_, e)| !e.file.trim().is_empty())
        .map(|(n, e)| FxLayer::of(n, e))
        .collect();
    if !layers.is_empty() && layers.iter().all(|l| super::chrome::animation(&l.art).is_some()) {
        let army = match target {
            CastTarget::Own => None,
            CastTarget::Army(uid) => Some(uid),
        };
        SPELL_FX.with(|q| q.borrow_mut().push(SpellFx::Layers { army, layers, started: None }));
        return;
    }
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
        SpellFx::Layers { layers, started, .. } => (now - *started.get_or_insert(now)) * 1000.0 >= layers_ms(layers),
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

/// The 2.5D view (`view25`): the setting (`audio::Settings::map_tilt`, pushed by
/// [`set_tilt_view`]) and the tilt in degrees (PageUp and PageDown change it).
static TILT_VIEW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The first-person view within the tilted one (`audio::Settings::map_first_person`), and its far
/// ground's horizon («Горизонт», `audio::Settings::map_horizon`); both off by default.
static FIRST_PERSON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static HORIZON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The sky's colours in the first-person view with a horizon: light blue at the top, pale at the
/// horizon.
const SKY_TOP: Color = Color::new(0.30, 0.55, 0.80, 1.0);
const SKY_HORIZON: Color = Color::new(0.80, 0.89, 0.95, 1.0);
/// The tilt's bits; NaN until it is set (then `RAZDOR_VIEW_PITCH`, or the default, starts it).
static TILT_PITCH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x7fc0_0000);
/// Degrees per PageUp or PageDown press.
const TILT_STEP: f32 = 5.0;
/// The sky above the ground in the 2.5D view (the part of the view with no map).
const SKY: Color = Color::new(0.06, 0.08, 0.11, 1.0);

/// The view override, `RAZDOR_VIEW`: `25d` (the 2.5D view), `flat`, the snapshots' `first` and `eyes`
/// (the first-person view forced on, the eyes camera with `eyes`), or the developer overrides `fp-chase`
/// (the first-person view behind the hero) and `fp-eyes` (the same with the eyes camera). None for the
/// settings.
fn view_env() -> Option<&'static str> {
    static ENV: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    ENV.get_or_init(|| std::env::var("RAZDOR_VIEW").ok()).as_deref()
}

/// Whether the world map is drawn tilted (the 2.5D view or the first-person one): the setting,
/// unless `RAZDOR_VIEW` says `25d` or `first` (on) or `flat` (off), which the debug snapshots use.
pub fn tilt_view() -> bool {
    // A trailer run is always 2.5D (`ui::trailer`).
    if super::trailer::active() {
        return true;
    }
    match view_env() {
        Some("25d") | Some("first") | Some("eyes") | Some("fp-chase") | Some("fp-eyes") => true,
        Some("flat") => false,
        _ => TILT_VIEW.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Puts the 2.5D view on or off (from the settings, at once).
pub fn set_tilt_view(on: bool) {
    TILT_VIEW.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the tilted view is the first-person one («От первого лица», `view25::TiltCamera::first_person`):
/// the setting (`audio::Settings::map_first_person`, pushed by [`set_first_person`]) within the tilted view.
/// `RAZDOR_VIEW=first` or `eyes` (a snapshot) and `fp-chase` or `fp-eyes` (the developer overrides) force it
/// on; `25d` and `flat` do not.
pub fn first_person() -> bool {
    tilt_view()
        && match view_env() {
            Some("first") | Some("eyes") | Some("fp-chase") | Some("fp-eyes") => true,
            Some(_) => false,
            None => FIRST_PERSON.load(std::sync::atomic::Ordering::Relaxed),
        }
}

/// Puts the first-person view on or off (from the settings, at once).
pub fn set_first_person(on: bool) {
    FIRST_PERSON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the first-person view's far ground curves down to a horizon with the sky above
/// («Горизонт»), rather than rolling up to top-down («Вверх»).
pub fn horizon() -> bool {
    // `RAZDOR_VIEW_HORIZON=down` (or `1`) makes it the horizon and `up` the roll, as a developer override
    // and for the snapshots; unset, the setting.
    static ENV: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    match ENV.get_or_init(|| std::env::var("RAZDOR_VIEW_HORIZON").ok()).as_deref().map(str::trim) {
        Some("down") | Some("1") => true,
        Some("up") => false,
        _ => HORIZON.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Puts the far ground's shape on or off the horizon (from the settings, at once).
pub fn set_horizon(on: bool) {
    HORIZON.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// The eyes view («Глазами героя», `audio::Settings::map_eyes`, pushed by [`set_eyes`]): the camera
/// stands at the hero's eyes and looks along his walk. Off by default; it is within the first-person
/// view (`first_person`).
static EYES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the eyes setting is on: `RAZDOR_VIEW=eyes` (a snapshot) and `fp-eyes` (a developer override)
/// turn it on, `fp-chase` and the other overrides turn it off, and with the env unset it is the setting.
pub fn eyes_setting() -> bool {
    match view_env() {
        Some("eyes") | Some("fp-eyes") => true,
        Some(_) => false,
        None => EYES.load(std::sync::atomic::Ordering::Relaxed),
    }
}

/// Puts the eyes view on or off (from the settings, at once).
pub fn set_eyes(on: bool) {
    EYES.store(on, std::sync::atomic::Ordering::Relaxed);
}

/// Whether the map is drawn from the hero's eyes now: a trailer shot's `eye` in a run, else the
/// setting within the first-person view.
pub fn eyes_view() -> bool {
    match trailer() {
        Some(t) => t.eye,
        None => first_person() && eyes_setting(),
    }
}

/// Whether `cam` is the first-person view's camera: a bent tilt that is not the eyes camera (the eyes have
/// their own fade). Photo mode's free camera has no bend, so it is not. With [`first_person`], the view's
/// near fade applies to it.
fn bent_first_person_cam(cam: &Camera) -> bool {
    !eyes_cam(cam) && cam.tilt.is_some_and(|t| t.cam.bend.is_some())
}

/// Whether the camera stands at the eyes (`view25::TiltCamera::eyes`): the hero's figure and his mark
/// are not drawn, a sprite behind the eye is not, and the near fade is the eyes' fixed one.
fn eyes_cam(cam: &Camera) -> bool {
    cam.tilt.is_some_and(|t| t.cam.eye_at.is_some())
}

/// The trailer's camera from `RAZDOR_TRAILER_CAM` (dev-only, read once; see `view25::Trailer`).
/// A value that does not parse is logged and ignored.
fn trailer() -> Option<view25::Trailer> {
    // A trailer shot or fly mode gives its camera frame by frame (`ui::trailer`).
    if let Some(t) = super::trailer::camera() {
        return Some(t);
    }
    static TRAILER: std::sync::OnceLock<Option<view25::Trailer>> = std::sync::OnceLock::new();
    *TRAILER.get_or_init(|| {
        let text = std::env::var("RAZDOR_TRAILER_CAM").ok()?;
        Some(view25::parse_trailer(&text).unwrap_or_else(|e| {
            razdor::diag!("RAZDOR_TRAILER_CAM: {e}");
            view25::Trailer::default()
        }))
    })
}

/// `RAZDOR_TRAILER_NOFOG=1` (dev-only, the trailer's stills): no fog is drawn and every sprite is
/// fully visible. Read once.
fn trailer_nofog() -> bool {
    static NOFOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    // A trailer shot or fly mode implies it: the fog is hidden there as the UI is.
    super::trailer::hides_ui() || *NOFOG.get_or_init(|| std::env::var("RAZDOR_TRAILER_NOFOG").is_ok_and(|v| v.trim() == "1"))
}

/// The camera the trailer's keys give over the player's `pitch` and `yaw`, centred on `target`
/// and fitted to `scale` in `view`. With a bend, the camera carries it (`TiltCamera::bend`), and
/// the region returned is the ground the sheet covers (`width`, `ahead` and `behind` about the
/// target), which the cull and the plan take.
fn trailer_camera(t: &view25::Trailer, target: Vec2, scale: f32, view: Rect, pitch: f32, yaw: f32) -> (TiltCamera, Option<Rect>) {
    let yaw = t.yaw.unwrap_or(yaw);
    let mut cam = TiltCamera::free(target, scale, view, t.pitch.unwrap_or(pitch), yaw, t.fov.unwrap_or(view25::FOV_DEG));
    // The distance the view fits to the zoom: a shot's distances start from it (`--plan` prints it).
    super::trailer::note_fitted_distance(cam.distance);
    if let Some(k) = t.fit {
        cam.distance *= k;
    }
    if let Some(d) = t.dist {
        cam.distance = d;
    }
    if t.bend <= 0.0 {
        return (cam, None);
    }
    let bend = view25::Bend::new(target, yaw, t.flat, t.radius, t.bend);
    cam.bend = Some(bend);
    (cam, Some(bend.region(t.width, t.ahead, t.behind)))
}

/// The photo mode's tilt (`ui::photo`): the free camera at `pitch` and `yaw` looking at `target`, fitted
/// to `scale` in `view`, its footprints as the 2.5D view's (the frame and the strip under the bar).
fn photo_tilt(target: Vec2, scale: f32, view: Rect, draw: Rect, pitch: f32, yaw: f32) -> Tilt {
    let cam = TiltCamera::free(target, scale, view, pitch, yaw, view25::FOV_DEG);
    Tilt { cam, plan: cam.ground_footprint(view, view), cull: cam.ground_footprint(view, draw) }
}

/// The first-person tilt of a view (`view25::TiltCamera::first_person`, the player's): the camera
/// looks at `look` (the map view's centre, the hero's place) and its bent sheet's ground is the
/// footprint of the frame `view`, culled to the strip under the bar `draw` as the 2.5D one is.
fn first_person_tilt(look: Vec2, scale: f32, view: Rect, draw: Rect, yaw: f32, horizon: bool) -> Tilt {
    let cam = TiltCamera::first_person(look, scale, view, yaw, horizon);
    Tilt { cam, plan: cam.ground_footprint(view, view), cull: cam.ground_footprint(view, draw) }
}

/// The eyes view's tilt (`view25::TiltCamera::eyes`): the eye stands on `ground` (the hero's displayed
/// place, or a shot's target in a run), looks along `yaw` (a shot's own yaw if it sets one) and has the
/// far ground of `horizon`: curved down to the horizon, else rolled up. A shot sets its own pitch, field,
/// height and bend. Its footprints are the frame `view` and the strip under the bar `draw`, as the
/// first-person view's.
fn eyes_tilt(ground: Vec2, scale: f32, view: Rect, draw: Rect, yaw: f32, horizon: bool, shot: Option<view25::Trailer>) -> Tilt {
    let (pitch, fov, height, yaw, bend) = match shot {
        Some(t) => (
            t.pitch.unwrap_or(view25::EYE_PITCH),
            t.fov.unwrap_or(view25::FP_FOV),
            t.height,
            t.yaw.unwrap_or(yaw),
            (t.bend > 0.0).then_some((t.flat, t.radius, if t.horizon { -t.bend } else { t.bend })),
        ),
        None if horizon => (view25::EYE_PITCH, view25::FP_FOV, view25::EYE_HEIGHT, yaw, Some((view25::EYE_FLAT, view25::EYE_RADIUS_DOWN, view25::EYE_BEND_DOWN))),
        None => (view25::EYE_PITCH, view25::FP_FOV, view25::EYE_HEIGHT, yaw, Some((view25::EYE_FLAT, view25::EYE_RADIUS_UP, view25::EYE_BEND_UP))),
    };
    let mut cam = TiltCamera::eyes(ground, height, scale, view, pitch, yaw, fov);
    if let Some((flat, radius, max_deg)) = bend {
        cam.bend = Some(cam.eye_bend(flat, radius, max_deg));
    }
    Tilt { cam, plan: cam.ground_footprint(view, view), cull: cam.ground_footprint(view, draw) }
}

/// The tilt in degrees: the start's (`RAZDOR_VIEW_PITCH`, else the default) until PageUp or
/// PageDown sets it.
fn tilt_pitch() -> f32 {
    let deg = f32::from_bits(TILT_PITCH.load(std::sync::atomic::Ordering::Relaxed));
    if deg.is_nan() {
        return std::env::var("RAZDOR_VIEW_PITCH").ok().and_then(|v| v.parse().ok()).unwrap_or(view25::PITCH_DEFAULT).clamp(view25::PITCH_MIN, view25::PITCH_MAX);
    }
    deg
}

fn set_tilt_pitch(deg: f32) {
    TILT_PITCH.store(deg.clamp(view25::PITCH_MIN, view25::PITCH_MAX).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The yaw's target in degrees (Q and E turn it by [`TILT_YAW_STEP`]), wrapped to (-180, 180];
/// NaN until it is set. The shown yaw eases toward it ([`ease_tilt_yaw`]).
static TILT_YAW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x7fc0_0000);
/// The shown yaw in degrees, the one the camera uses; NaN until the first frame.
static TILT_YAW_SHOWN: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0x7fc0_0000);
/// Degrees per Q or E press (and per two-finger twist step).
const TILT_YAW_STEP: f32 = 45.0;
/// The size factor of the standing sprites in the 2.5D view (1 is their own size): the setting
/// (`audio::Settings::map_sprite_size`, pushed by [`set_sprite_size`]).
static SPRITE_VIEW: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(DEFAULT_SPRITE_SIZE.to_bits());
/// The sprite size the game starts with (the owner's decision, 2026-11-05).
pub const DEFAULT_SPRITE_SIZE: f32 = 1.5;
/// The sizes the settings button cycles through (1×, 1.5×, 2×).
pub const SPRITE_SIZES: [f32; 3] = [1.0, 1.5, 2.0];

/// The standing sprites' size factor: `RAZDOR_VIEW_SPRITE`, when set (a look experiment for the
/// snapshots), else the setting.
pub fn sprite_size() -> f32 {
    static ENV: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();
    let env = *ENV.get_or_init(|| std::env::var("RAZDOR_VIEW_SPRITE").ok().and_then(|v| v.parse::<f32>().ok()).map(|v| v.clamp(0.5, 3.0)));
    env.unwrap_or_else(|| f32::from_bits(SPRITE_VIEW.load(std::sync::atomic::Ordering::Relaxed)))
}

/// Puts the sprites' size from the settings (a value outside 0.5..=3 or not finite is the default).
pub fn set_sprite_size(f: f32) {
    let f = if f.is_finite() { f.clamp(0.5, 3.0) } else { DEFAULT_SPRITE_SIZE };
    SPRITE_VIEW.store(f.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The size the settings button shows after `f`: 1× → 1.5× → 2× → 1×. A size not in
/// [`SPRITE_SIZES`] (a file's own) goes to 1×.
pub fn next_sprite_size(f: f32) -> f32 {
    match SPRITE_SIZES.iter().position(|&s| (s - f).abs() < 1e-3) {
        Some(i) => SPRITE_SIZES[(i + 1) % SPRITE_SIZES.len()],
        None => SPRITE_SIZES[0],
    }
}

/// The world map's drawing resolution (`map_scale`, 1.0 = 100%): pushed by [`set_map_render_scale`].
static MAP_SCALE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1.0f32.to_bits());

/// Puts the map's drawing resolution from the settings (a value not in `map_scale::SCALES` is 100%).
pub fn set_map_render_scale(f: f32) {
    MAP_SCALE.store(super::map_scale::valid(f).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The map's drawing resolution the settings set (`map_scale::in_use` applies the profiling switch).
pub fn map_render_scale() -> f32 {
    f32::from_bits(MAP_SCALE.load(std::sync::atomic::Ordering::Relaxed))
}

/// `deg` wrapped to (-180, 180].
fn wrap_yaw(deg: f32) -> f32 {
    let w = deg.rem_euclid(360.0);
    if w > 180.0 { w - 360.0 } else { w }
}

/// The start yaw: `RAZDOR_VIEW_YAW` (any angle, for the snapshots), else 0 (north up).
fn tilt_yaw_start() -> f32 {
    std::env::var("RAZDOR_VIEW_YAW").ok().and_then(|v| v.parse().ok()).map_or(0.0, wrap_yaw)
}

/// The yaw the camera shows, in degrees (see [`ease_tilt_yaw`]).
fn tilt_yaw() -> f32 {
    let deg = f32::from_bits(TILT_YAW_SHOWN.load(std::sync::atomic::Ordering::Relaxed));
    if deg.is_nan() { tilt_yaw_start() } else { deg }
}

/// The yaw the player turned to, in degrees.
fn tilt_yaw_target() -> f32 {
    let deg = f32::from_bits(TILT_YAW.load(std::sync::atomic::Ordering::Relaxed));
    if deg.is_nan() { tilt_yaw_start() } else { deg }
}

/// Turns the yaw target by `deg` (wrapped).
fn turn_tilt_yaw(deg: f32) {
    TILT_YAW.store(wrap_yaw(tilt_yaw_target() + deg).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Turns the view back to the default angle (north up): the compass button's tap.
fn reset_tilt_yaw() {
    TILT_YAW.store(0.0f32.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The yaw after `dt_ms` of easing from `shown` toward `target`: the shortest way round, at a
/// rate of 12 a second, and onto the target within 0.05°.
fn ease_yaw(shown: f32, target: f32, dt_ms: i64) -> f32 {
    let d = wrap_yaw(target - shown);
    if d.abs() < 0.05 {
        return target;
    }
    wrap_yaw(shown + d * (1.0 - (-(dt_ms as f32 / 1000.0) * 12.0).exp()))
}

/// Eases the shown yaw toward its target over the frame's `dt_ms` ([`ease_yaw`]).
fn ease_tilt_yaw(dt_ms: i64) {
    let next = ease_yaw(tilt_yaw(), tilt_yaw_target(), dt_ms);
    TILT_YAW_SHOWN.store(next.to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// The eyes view's yaw target while the hero walks: the direction of his next step, from where he is
/// drawn, so the view turns with his walk (eased by [`ease_tilt_yaw`]). Standing, the target stays:
/// Q, E, the two-finger twist and the compass turn it.
fn follow_walk(game: &Game) {
    let (px, py) = game.display_pos();
    let Some((hx, hy)) = game.display_heading() else { return };
    let (dx, dy) = (hx - px, hy - py);
    if dx.hypot(dy) < 1e-3 {
        return;
    }
    // The yaw whose forward is the step: `forward(yaw)` is (-sin yaw, -cos yaw).
    TILT_YAW.store(wrap_yaw((-dx).atan2(-dy).to_degrees()).to_bits(), std::sync::atomic::Ordering::Relaxed);
}

/// Whether the compass button shows: the 2.5D view is turned away from the default angle, by the
/// player's turns or while it eases back (`shown`, `target` in degrees). Hidden once it is back.
fn compass_shows(tilted: bool, shown: f32, target: f32) -> bool {
    tilted && (wrap_yaw(shown).abs() > 1e-3 || wrap_yaw(target).abs() > 1e-3)
}

/// A 3D view for `set_camera`: depth off (the map is painted back to front, as the flat view
/// is), no viewport (not every backend has one), the matrix of [`view25::window_matrix`].
struct View3d(Mat4);

impl macroquad::camera::Camera for View3d {
    fn matrix(&self) -> Mat4 {
        self.0
    }
    fn depth_enabled(&self) -> bool {
        false
    }
    fn render_pass(&self) -> Option<RenderPass> {
        // The screen, the target of a cached picture being filled, or the map's scaled target (`render_cache`).
        super::render_cache::draw_target().map(|rt| rt.render_pass)
    }
    fn viewport(&self) -> Option<(i32, i32, i32, i32)> {
        None
    }
}

/// Which part of a bent view's closure runs ([`bent_view`]): the closure runs once for the
/// ground (drawn flat into the sheet's picture, `ground` draws, `standing` does not) and once for
/// the sprites (`ground` does not draw, `standing` stands them on the sheet). Plain otherwise.
#[derive(Clone, Copy, PartialEq)]
enum Pass {
    Plain,
    Ground,
    Sprites,
}

local! {
    static PASS: std::cell::Cell<Pass> = const { std::cell::Cell::new(Pass::Plain) };
}

/// Runs `f` with the pass `pass` set, the plain pass after.
fn with_pass(pass: Pass, f: impl FnOnce()) {
    PASS.with(|p| p.set(pass));
    f();
    PASS.with(|p| p.set(Pass::Plain));
}

/// Runs `f` on the 2D space of `cam`. A flat camera's is its own; a tilted one's is its cull
/// footprint ([`Camera::tilted_space`]), drawn with the 3D view set, where [`ground`] and
/// [`standing`] place each pass. The 2D camera is back after. With a bend ([`bent_view`]) `f`
/// runs twice, see [`Pass`].
fn in_view(cam: &Camera, f: impl Fn(&Camera)) {
    let Some(tilt) = cam.tilt else { return f(cam) };
    // The window's view of this camera: the sprites' screen rectangles project through it ([`screen_view`]).
    let prev = SCREEN_VIEW.with(|v| v.replace(Some(cam.view)));
    if let Some(bend) = tilt.cam.bend.filter(|_| super::render_cache::target().is_none()) {
        bent_view(cam, &tilt, bend, &f);
    } else {
        let (w, h) = (screen_width(), screen_height());
        set_camera(&View3d(view25::window_matrix(cam.view, w, h) * tilt.cam.view_proj(cam.view)));
        f(&cam.tilted_space());
        super::render_cache::back_to_2d();
    }
    SCREEN_VIEW.with(|v| v.set(prev));
}

/// The bent view of [`in_view`] (`RAZDOR_TRAILER_CAM`, dev-only): the ground pass is drawn flat
/// into a picture of the cull's ground, the picture is laid on a grid that follows the bend
/// ([`draw_sheet`]), and the sprites are stood on that sheet in a second run of `f`. The picture
/// is at the screen's pixel density at the pivot (`k` pixels per world pixel), so the ground
/// stays as sharp as the flat view's at the middle of the frame.
fn bent_view(cam: &Camera, tilt: &Tilt, bend: view25::Bend, f: &impl Fn(&Camera)) {
    let space = cam.tilted_space();
    let (sw, sh) = (space.view.w, space.view.h);
    // The picture's pixels per 2D pixel: the screen's at the pivot, shrunk to fit the largest
    // target the GPU takes (`MAX_BENT_SIDE`).
    let k = (tilt.cam.pivot_scale(cam.view) / cam.scale).max(0.05);
    let k = k.min(MAX_BENT_SIDE / sw.max(sh).max(1.0));
    let (pw, ph) = ((sw * k).ceil().max(1.0) as u32, (sh * k).ceil().max(1.0) as u32);
    let target = bent_target(pw, ph);
    super::render_cache::fill_space(&target, space.view, || with_pass(Pass::Ground, || f(&space)));
    let (w, h) = (screen_width(), screen_height());
    set_camera(&View3d(view25::window_matrix(cam.view, w, h) * tilt.cam.view_proj(cam.view)));
    draw_sheet(&target, &bend, tilt.cull);
    with_pass(Pass::Sprites, || f(&space));
    super::render_cache::back_to_2d();
}

/// The largest side of a bent view's picture in pixels.
const MAX_BENT_SIDE: f32 = 8192.0;

/// The target of a bent view's picture of `w` × `h` pixels, kept by size: the views fill and draw
/// their picture one after the other, so one target serves them all.
fn bent_target(w: u32, h: u32) -> RenderTarget {
    local! {
        static TARGETS: std::cell::RefCell<Vec<((u32, u32), RenderTarget)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    // The size goes up to a multiple of 64 px, so the footprint's size, which moves with the turn,
    // keeps to a few targets; the picture is drawn over the whole target (`render_cache::fill_space`).
    let round = |v: u32| v.div_ceil(TARGET_STEP) * TARGET_STEP;
    let size = (round(w), round(h));
    TARGETS.with(|t| {
        let mut t = t.borrow_mut();
        if let Some((_, rt)) = t.iter().find(|(s, _)| *s == size) {
            return rt.clone();
        }
        let rt = render_target(size.0, size.1);
        rt.texture.set_filter(FilterMode::Nearest);
        if t.len() >= TARGETS_KEPT {
            t.remove(0);
        }
        t.push((size, rt.clone()));
        rt
    })
}

/// The steps a bent view's picture is sized by, and how many sizes are kept (the newest).
const TARGET_STEP: u32 = 64;
const TARGETS_KEPT: usize = 4;

/// Draws the picture `target` on the bent ground: a grid of `SHEET_ACROSS` × `SHEET_ALONG` cells
/// over `region` (the ground in world units), each vertex lifted by [`view25::Bend::point`] and
/// its texture coordinate the point's place in the region. The picture's rows come out upside
/// down from its fill, so `v` runs the other way (as [`render_cache::Cached::show`] flips it).
fn draw_sheet(target: &RenderTarget, bend: &view25::Bend, region: Rect) {
    const ACROSS: usize = 64;
    const ALONG: usize = 128;
    // Macroquad clamps a draw to 5000 indices (10000 vertices): 8 rows of 64 cells are 3072.
    const ROWS_PER_DRAW: usize = 8;
    let vertex = |i: usize, j: usize| {
        let (u, v) = (i as f32 / ACROSS as f32, j as f32 / ALONG as f32);
        let p = bend.point(vec2(region.x + region.w * u, region.y + region.h * v));
        macroquad::models::Vertex::new(p.x, p.y, p.z, u, 1.0 - v, WHITE)
    };
    for j0 in (0..ALONG).step_by(ROWS_PER_DRAW) {
        let rows = ROWS_PER_DRAW.min(ALONG - j0);
        let mut vertices = Vec::with_capacity((ACROSS + 1) * (rows + 1));
        for j in j0..=j0 + rows {
            for i in 0..=ACROSS {
                vertices.push(vertex(i, j));
            }
        }
        let row = (ACROSS + 1) as u16;
        let mut indices = Vec::with_capacity(rows * ACROSS * 6);
        for j in 0..rows as u16 {
            for i in 0..ACROSS as u16 {
                let a = j * row + i;
                indices.extend_from_slice(&[a, a + 1, a + row, a + 1, a + row + 1, a + row]);
            }
        }
        macroquad::models::draw_mesh(&macroquad::models::Mesh { vertices, indices, texture: Some(target.texture.clone()) });
    }
}

/// The mesh of a raised mist sheet `lift` cells above a bent sheet (the first-person view): the fog
/// quad `dest` (its texels `src` of `tex`, as [`Camera::mist_rects`]) on a grid of 64 × 64 cells, each
/// vertex lifted along the sheet's normal ([`view25::Bend::lift`]). Its colour's alpha is 0 past the
/// horizon, so the mist does not float in the sky. Drawn in chunks of 8 rows (a draw takes 5000 indices).
fn bent_mist_meshes(bend: &view25::Bend, origin: Vec2, scale: f32, dest: Rect, src: Option<Rect>, tex: &Texture2D, lift: f32) -> Vec<Mesh> {
    const ACROSS: usize = 64;
    const ALONG: usize = 64;
    const ROWS_PER_DRAW: usize = 8;
    let (tw, th) = (tex.width(), tex.height());
    let s = src.unwrap_or(Rect::new(0.0, 0.0, tw, th));
    let vertex = |i: usize, j: usize| {
        let (fu, fv) = (i as f32 / ACROSS as f32, j as f32 / ALONG as f32);
        let g = (vec2(dest.x + dest.w * fu, dest.y + dest.h * fv) + origin) / scale;
        // Near the horizon the sheet's normal leans toward the far side, and a lifted point would rise above
        // the horizon line: the lift and the alpha fade out over the last cells before it (none past it).
        let fade = if bend.limit.is_finite() { ((bend.limit - (g - bend.pivot).dot(view25::forward(bend.yaw_deg))) / HORIZON_FADE).clamp(0.0, 1.0) } else { 1.0 };
        let fade = fade * fade * (3.0 - 2.0 * fade);
        let at = bend.lift(g, lift * fade);
        let alpha = if bend.hidden(g) { 0.0 } else { fade };
        macroquad::models::Vertex::new(at.x, at.y, at.z, (s.x + s.w * fu) / tw, (s.y + s.h * fv) / th, Color::new(1.0, 1.0, 1.0, alpha))
    };
    let mut meshes = Vec::new();
    for j0 in (0..ALONG).step_by(ROWS_PER_DRAW) {
        let rows = ROWS_PER_DRAW.min(ALONG - j0);
        let mut vertices = Vec::with_capacity((ACROSS + 1) * (rows + 1));
        for j in j0..=j0 + rows {
            for i in 0..=ACROSS {
                vertices.push(vertex(i, j));
            }
        }
        let row = (ACROSS + 1) as u16;
        let mut indices = Vec::with_capacity(rows * ACROSS * 6);
        for j in 0..rows as u16 {
            for i in 0..ACROSS as u16 {
                let a = j * row + i;
                indices.extend_from_slice(&[a, a + 1, a + row, a + 1, a + row + 1, a + row]);
            }
        }
        meshes.push(Mesh { vertices, indices, texture: Some(tex.clone()) });
    }
    meshes
}

/// The width and height of a bank of mist in cells (its billboard stands up from the frontier).
const BANK_WIDTH: f32 = 1.4;
const BANK_HEIGHT: f32 = 1.5;
/// The banks per frontier edge: spread along the edge at these offsets (cells), overlapping so that
/// they read as one wall.
const BANK_SAMPLES: [f32; 3] = [-0.45, 0.0, 0.45];
/// The cells before a bent view's horizon over which a raised mist sheet sinks to the sheet.
const HORIZON_FADE: f32 = 3.0;

/// The banks of mist along the frontier (2.5D and first-person, `mist::frontier`): each frontier edge
/// stands three banks on the line between its unexplored cell and the lit neighbour, sorted like a
/// standing sprite on the unexplored cell. The nearest to the view's centre come first, up to
/// `mist::banks_cap`. The unexplored side's shown darkness sets each bank's alpha.
fn push_banks(cam: &Camera, game: &Game, cols: (i32, i32), rows: (i32, i32), items: &mut Vec<(i64, Drawable)>) {
    let map = &game.world.map;
    let fog = &game.fog;
    let known = |t: (i32, i32)| (t.0 >= 0 && t.1 >= 0 && t.0 < map.w && t.1 < map.h).then(|| minimap::shown_darkness(fog, t));
    let edges = mist::frontier(known, (cols.0 - 1, rows.0 - 1), (cols.1 + 1, rows.1 + 1));
    if edges.is_empty() {
        return;
    }
    let grid = cam.grid;
    let centre = Vec2::from(cam.centre());
    let mut edges: Vec<(f32, mist::Edge)> = edges.into_iter().map(|e| (Vec2::from(grid.center(e.cell)).distance_squared(centre), e)).collect();
    edges.sort_by(|a, b| a.0.total_cmp(&b.0));
    let cap = mist::banks_cap();
    let mut n = 0usize;
    for (_, e) in edges {
        // The wall stands on the frontier line, on the lit side's cell (`toward`): it rises from the explored
        // land, where it shows against the land (over the unexplored mist it would be lost in it).
        let (a, b) = (Vec2::from(grid.center(e.cell)), Vec2::from(grid.center(e.toward)));
        let mid = (a + b) * 0.5;
        // Along the edge: the perpendicular of the step to the lit cell, a cell long.
        let d = b - a;
        let along = vec2(-d.y, d.x).normalize_or_zero();
        for t in BANK_SAMPLES {
            if n >= cap {
                return;
            }
            let at = mid + along * t;
            let foot = cam.to_screen((at.x, at.y));
            let seed = (n as f32 * 0.618_034 + e.cell.0 as f32 * 0.37 + e.cell.1 as f32 * 0.13).fract();
            items.push((sprite_key(cam, e.toward, foot, LAYER_BANK), Drawable::Bank { foot, darkness: e.darkness, seed }));
            n += 1;
        }
    }
}

/// Draws a bank of mist standing with its foot at `foot` (in the billboard [`standing`] makes of it):
/// [`BANK_WIDTH`] × [`BANK_HEIGHT`] cells. `standing` grows the billboard by the sprites' size, so the
/// rectangle is the cells over that size.
fn draw_bank(cam: &Camera, foot: Vec2, darkness: f32, seed: f32) {
    let k = cam.scale / sprite_size();
    let (w, h) = (BANK_WIDTH * k, BANK_HEIGHT * k);
    mist::bank(Rect::new(foot.x - w / 2.0, foot.y - h, w, h), darkness, seed);
}

/// Draws the ground pass: under a tilted camera, the 2D space laid flat on the ground
/// ([`view25::ground_model`]); under a flat one, as drawn. In a bent view's sprite run it draws
/// nothing (the sheet holds the ground), and in its ground run it draws flat into the picture.
fn ground(cam: &Camera, draw: impl FnOnce()) {
    if cam.tilt.is_none() {
        return draw();
    }
    match PASS.with(std::cell::Cell::get) {
        Pass::Plain => {
            push_model(view25::ground_model(cam.origin, cam.scale));
            draw();
            pop_model();
        }
        Pass::Ground => draw(),
        Pass::Sprites => {}
    }
}

local! {
    /// The fog's visibility of the standing sprite being drawn, 1 = plain ([`sprite_visibility`]).
    /// Only [`standing`] sets it, and only in the 2.5D view: the flat view's sprites are always 1.
    static SPRITE_VIS: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// A straight-alpha sprite colour `c` faded by the visibility of the standing sprite being drawn:
/// the colour kept, the alpha times the visibility (white becomes (1, 1, 1, v)), so what is behind
/// the sprite shows through it.
fn faded(c: Color) -> Color {
    let v = SPRITE_VIS.with(std::cell::Cell::get);
    Color::new(c.r, c.g, c.b, c.a * v)
}

/// [`faded`] for a premultiplied sprite (the figures, `chrome::premultiplied`): every channel
/// scaled by the visibility, so white becomes (v, v, v, v).
fn faded_premultiplied(c: Color) -> Color {
    let v = SPRITE_VIS.with(std::cell::Cell::get);
    Color::new(c.r * v, c.g * v, c.b * v, c.a * v)
}

/// The visibility of a standing sprite on a cell: 1 less the darkness the ground shows there
/// (`dark`, 0 to 1: the fog and the reveal composed), eased by a smoothstep from 0.15 to 0.85, so
/// an edge sprite is not ghostly for long and a deep one is gone.
fn sprite_visibility(dark: f32) -> f32 {
    let x = (1.0 - dark).clamp(0.0, 1.0);
    let t = ((x - 0.15) / 0.7).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Draws a standing sprite whose foot is at `foot` (a point of the 2D space) as a billboard
/// ([`view25::billboard_model_about`]); under a flat camera, as drawn. The sprite turns about
/// `foot` and grows about `base`, which is the foot for every standing sprite (decorations,
/// buildings and figures alike: a grown picture keeps its ground centre where it stands). `vis` is its
/// visibility ([`Shade::at`]): the sprite fades into the fog on its foot's cell (`faded`), and is
/// not drawn at all at 0.
///
/// The billboard is a CPU-side model transform (macroquad's `push_cpu_model_matrix`, a fork
/// patch): a GPU model would start a new draw call per sprite, and on the owner's OnePlus 13
/// (Adreno) each draw call's buffer upload then cost a copy and an orphaned buffer (91% of the
/// frame in the render passes; the 2.5D frame 17.7 ms against 8.3 ms flat). With the transform
/// on the CPU the sprites share their draw calls.
/// `picture` is the sprite's drawn rectangle in the 2D space of `cam` (see [`covering_gap`]).
fn standing(cam: &Camera, foot: Vec2, base: Vec2, vis: f32, picture: Rect, draw: impl FnOnce()) {
    let Some(tilt) = cam.tilt else { return draw() };
    if vis <= 0.0 {
        return;
    }
    let g = (foot + cam.origin) / cam.scale;
    // Past a down bend's horizon the sheet hides the ground, and what stands on it.
    if tilt.cam.bend.is_some_and(|b| b.hidden(g)) {
        return;
    }
    // The sheet holds the ground: no sprite in the ground run.
    let pass = PASS.with(std::cell::Cell::get);
    if pass == Pass::Ground {
        return;
    }
    let (model, on_ground) = standing_model(cam, &tilt, foot, base, pass == Pass::Sprites);
    let yaw = tilt.cam.yaw_deg;
    // The eyes view: a sprite behind the eye is not seen (its billboard stands at its foot's depth).
    if let Some(eye) = tilt.cam.eye_at {
        let f = view25::forward(yaw);
        if (on_ground.x - eye.x) * f.x + (on_ground.z - eye.z) * f.y <= 0.0 {
            return;
        }
    }
    // The trailer's camera (dev-only) and the first-person view: a sprite whose foot is nearer the
    // eye than the near fade's distance fades out (the first-person view: only one that covers the
    // hero, [`first_person_fade`]), and the blend band of its sharp draws follows the magnification
    // of a nearer sprite. The 2.5D view is as before.
    let mut vis = vis;
    let mut magnify = 1.0;
    if trailer().is_some() || first_person() {
        let d = (on_ground - tilt.cam.eye()).length().max(1e-3);
        magnify = tilt.cam.pivot_scale(cam.view) * tilt.cam.distance / (d * cam.scale);
        let limit = near_fade_limit(cam);
        if limit > 0.0 && NEAR_FADE.with(std::cell::Cell::get) {
            vis *= if eyes_cam(cam) {
                // The eyes view's band is narrow (`EYE_FADE_BAND`): a tree stays translucent over the sky for
                // half a cell, not for the band of the trailer's and 2.5D's fade.
                smooth_band(d, limit, view25::EYE_FADE_BAND)
            } else if trailer().is_some() {
                near_fade(d, limit)
            } else {
                let depth = g.dot(view25::forward(yaw));
                first_person_fade(covering_gap(cam, &tilt, model, picture, depth), FP_NEAR_BAND * limit)
            };
        }
        if vis <= 0.0 {
            return;
        }
    }
    push_cpu_model(model);
    let prev = SPRITE_VIS.with(|s| s.replace(vis));
    super::sharp::with_perspective(magnify, draw);
    SPRITE_VIS.with(|s| s.set(prev));
    pop_cpu_model();
}

local! {
    /// The distance from the eye of the hero's foot this frame: the trailer's near fade never reaches
    /// past it, so the sprites between the camera and him fade and he stays in view (infinite off
    /// the sprite run).
    static NEAR_LIMIT: std::cell::Cell<f32> = const { std::cell::Cell::new(f32::INFINITY) };
    /// Whether the trailer's near fade applies to the standing sprite being drawn: off for the hero,
    /// who is the frame's point of interest (the trees in front of him fade instead).
    static NEAR_FADE: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
    /// The first-person view's hero as drawn this frame (`draw_world_in`): the sprites that cover him are
    /// faded for him ([`covering_gap`]). None when he is not drawn.
    static HERO_ON_WINDOW: std::cell::Cell<Option<HeroOnWindow>> = const { std::cell::Cell::new(None) };
    /// The window's view of the tilted camera being drawn (`in_view`): the sprites' rectangles project
    /// through it, as the GPU draws them ([`screen_view`]).
    static SCREEN_VIEW: std::cell::Cell<Option<Rect>> = const { std::cell::Cell::new(None) };
}

/// The hero as drawn this frame: his rectangle on the window and his depth along the view (the draw order's
/// depth, [`sprite_key`]).
#[derive(Clone, Copy)]
struct HeroOnWindow {
    rect: Rect,
    depth: f32,
}

/// Runs `draw` with the near fade off (the hero's figure).
fn without_near_fade(draw: impl FnOnce()) {
    let prev = NEAR_FADE.with(|f| f.replace(false));
    draw();
    NEAR_FADE.with(|f| f.set(prev));
}

/// The distance inside which a standing sprite fades (world units; 0 is off): the trailer's `near`,
/// and in the first-person view the hero's distance from the eye (a sprite nearer than him fades only
/// if it covers him, [`first_person_fade`]; he never fades). Never past the hero.
fn near_fade_limit(cam: &Camera) -> f32 {
    // The eyes view: a fixed distance (the hero stands at the eye, so his distance would fade nothing).
    if eyes_cam(cam) {
        return view25::EYE_NEAR;
    }
    let hero = NEAR_LIMIT.with(std::cell::Cell::get);
    match trailer() {
        Some(t) => t.near.min(hero),
        // Only the first-person view fades by the hero's distance: the 2.5D sprites never do.
        None if hero.is_finite() && first_person() && bent_first_person_cam(cam) => hero,
        None => 0.0,
    }
}

/// The first-person view's near fade of a standing sprite: a sprite drawn over the hero (`gap` is how far in
/// front of him it stands along the view, [`covering_gap`]) is gone once it stands `ramp` in front of him,
/// whole `ramp` behind him, and fades across his depth in between. Every other sprite stays whole, the trees
/// and buildings between the eye and him included (the owner's report, 2026-11-10: the whole lower half of
/// the view went when the fade keyed on his distance alone).
fn first_person_fade(gap: Option<f32>, ramp: f32) -> f32 {
    match gap {
        Some(gap) => 1.0 - smooth_band(gap, -ramp, 2.0 * ramp),
        None => 1.0,
    }
}

/// The ramp across the hero's depth, as a share of his distance from the eye (see [`first_person_fade`]). A
/// covering sprite that stands only a little in front of him (a building at his depth, 0.3 of a cell) still
/// covered him at a wider ramp: the ramp is kept narrow, a sprite in front of him is gone.
const FP_NEAR_BAND: f32 = 0.01;

/// The margin, as a share of the hero's drawn width, that a sprite's picture must clear his by to stay
/// whole (see [`covering_gap`]).
const FP_HERO_MARGIN: f32 = 0.1;

/// How far a standing sprite stands in front of the hero along the view when its picture covers his drawn
/// rectangle on the window, else None: the picture (`picture`, in the 2D space of `cam`, under its billboard
/// `model`) overlaps his widened by [`FP_HERO_MARGIN`], or has a corner behind the eye. The depth is the one
/// the draw order sorts by ([`sprite_key`]), so a sprite that is drawn over him is one in front of him. None
/// when he is not drawn this frame.
fn covering_gap(cam: &Camera, tilt: &Tilt, model: Mat4, picture: Rect, depth: f32) -> Option<f32> {
    let hero = HERO_ON_WINDOW.with(std::cell::Cell::get)?;
    let covers = match picture_on_screen(tilt, screen_view(cam), model, picture) {
        Some(r) => overlaps(r, hero.rect, FP_HERO_MARGIN * hero.rect.w),
        None => true,
    };
    covers.then_some(hero.depth - depth)
}

/// Whether the rectangles `a` and `b` overlap, `b` widened by `margin` on every side.
fn overlaps(a: Rect, b: Rect, margin: f32) -> bool {
    a.x < b.x + b.w + margin && a.x + a.w > b.x - margin && a.y < b.y + b.h + margin && a.y + a.h > b.y - margin
}

/// The window's view of the tilted camera being drawn: the frame its projection maps to (set by
/// [`in_view`]; the 2D space's own view is its cull, not the window's).
fn screen_view(cam: &Camera) -> Rect {
    SCREEN_VIEW.with(std::cell::Cell::get).unwrap_or(cam.view)
}

/// The screen rectangle of a standing sprite's `picture` (in the 2D space of its camera) under its billboard
/// `model`: the picture's four corners projected through the window's view, and the box about them. None
/// if a corner is behind the eye.
fn picture_on_screen(tilt: &Tilt, view: Rect, model: Mat4, picture: Rect) -> Option<Rect> {
    let (x0, y0, x1, y1) = (picture.x, picture.y, picture.x + picture.w, picture.y + picture.h);
    let (mut lo, mut hi) = (vec2(f32::INFINITY, f32::INFINITY), vec2(f32::NEG_INFINITY, f32::NEG_INFINITY));
    for (x, y) in [(x0, y0), (x1, y0), (x1, y1), (x0, y1)] {
        let s = tilt.cam.project(model.transform_point3(vec3(x, y, 0.0)), view)?;
        lo = lo.min(s);
        hi = hi.max(s);
    }
    Some(Rect::new(lo.x, lo.y, hi.x - lo.x, hi.y - lo.y))
}

/// A standing sprite's billboard model and the point its foot stands on in space: about the bent sheet's
/// point of the foot when `bent` (the sprites' run of a bent view), else the flat ground's.
fn standing_model(cam: &Camera, tilt: &Tilt, foot: Vec2, base: Vec2, bent: bool) -> (Mat4, Vec3) {
    let (pitch, yaw, size) = (tilt.cam.pitch_deg, tilt.cam.yaw_deg, sprite_size());
    let g = (foot + cam.origin) / cam.scale;
    match tilt.cam.bend {
        // A sprite stands where its foot is on the sheet (the bend's point of its ground).
        Some(bend) if bent => {
            let at = bend.point(g);
            (view25::billboard_model_about_at(foot, base, cam.scale, pitch, yaw, size, at), at)
        }
        _ => (view25::billboard_model_about(cam.origin, foot, base, cam.scale, pitch, yaw, size), vec3(g.x, 0.0, g.y)),
    }
}

/// The fade of a standing sprite whose foot is `d` from the eye, for the near fade's distance
/// ([`near_fade_limit`]): 0 at `near`, smoothly 1 at 1.5 `near` and beyond.
fn near_fade(d: f32, near: f32) -> f32 {
    smooth_band(d, near, 0.5 * near)
}

/// The smoothstep from 0 at `near` to 1 at `near + band`.
fn smooth_band(d: f32, near: f32, band: f32) -> f32 {
    let t = ((d - near) / band).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The distance from the eye to a standing sprite's foot (2D point `foot` of the tilted space): the
/// bent sheet's point where it is, else the flat ground's.
fn foot_distance(cam: &Camera, foot: Vec2) -> Option<f32> {
    let tilt = cam.tilt?;
    let g = (foot + cam.origin) / cam.scale;
    let on_ground = match tilt.cam.bend {
        Some(bend) => bend.point(g),
        None => vec3(g.x, 0.0, g.y),
    };
    Some((on_ground - tilt.cam.eye()).length())
}

/// Whether the trailer's stills are on (`RAZDOR_TRAILER_CAM`, dev-only): the terrain is drawn with
/// nearest sampling then, and the frame shows the world alone.
pub fn trailer_active() -> bool {
    trailer().is_some()
}

/// What photo mode's frame did (`photo_frame`): it drew the frame, or it was left before drawing and
/// the map's own frame goes on as in play.
enum PhotoStep {
    Drawn,
    Left,
}

/// Turns photo mode on for the map (`map_frame`): the camera starts where the play's looks, at the
/// play's pitch and turn; the map's look is kept for leaving (`photo::Photo::saved_look`). The zoom
/// and the rest of the play's view are not touched, so leaving gives them back.
fn enter_photo(game: &Game, view: &mut MapView) {
    let look = view.look.unwrap_or(game.display_pos());
    let centre = Camera::looking_at(game, view.zoom, look).centre();
    let saved = view.look;
    view.look = Some(centre);
    let mut p = photo::Photo::new(tilt_pitch(), tilt_yaw(), view.zoom, saved);
    photo::snapshot_start(&mut p);
    photo::enter(p);
}

/// Pans the map by a pointer or finger move of `slide` screen pixels, the map following it (as
/// `pan`, but with no check on the windows: photo mode has none open).
fn photo_pan(game: &Game, view: &mut MapView, slide: (f32, f32)) {
    if slide == (0.0, 0.0) {
        return;
    }
    let look = view.look.unwrap_or(game.display_pos());
    let cam = Camera::looking_at(game, view.zoom, look);
    let at = Vec2::from(look) - cam.screen_to_world_delta(vec2(slide.0, slide.1));
    view.look = Some(Camera::looking_at(game, view.zoom, at.into()).centre());
}

/// The shutter (`photo_frame`): the screen as drawn so far (the map, no strip, no notice) saved as
/// a PNG, and a notice of where it went (the folder too on the desktop).
fn take_shot(view: &mut MapView, now: f64) {
    let img = get_screen_data();
    match photo::save_shot(&img.bytes, img.width as u32, img.height as u32) {
        Ok(path) => {
            let file = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            view.toasts.push_back((trf!("Shot saved: {file}", file = file), now));
            if super::display::dp_px().is_none() {
                let folder = path.parent().map(|d| d.display().to_string()).unwrap_or_default();
                view.toasts.push_back((trf!("Photos folder: {folder}", folder = folder), now));
            }
        }
        Err(error) => view.toasts.push_back((trf!("The shot was not saved: {error}", error = error), now)),
    }
    while view.toasts.len() > TOAST_MAX {
        view.toasts.pop_front();
    }
}

/// The map in photo mode (`ui::photo`, `map_frame`): the game stands still (no tick, no event, no
/// AI move, no clock), the camera is free (the mouse and fingers, the wheel, the keys), and the frame is
/// the map alone. The shutter's picture is taken before the control strip and the notices are drawn.
/// Leaving (the strip's «Выход», the photo key, Esc with the strip shown) gives the view back and returns
/// `Left` before anything is drawn.
fn photo_frame(game: &mut Game, assets: &Assets, view: &mut MapView, entering: bool) -> PhotoStep {
    let (sw, sh) = (screen_width(), screen_height());
    let layout = photo::layout((sw, sh), super::display::dp_px(), super::chrome::k(), super::display::safe_area().bottom);
    let shown = photo::with(|p| p.strip).unwrap_or(true);
    let (pressed, held) = if shown { photo::strip_input(&layout) } else { ([false; 3], [false; 3]) };
    let at = pointer();
    let on_strip = shown && photo::on_strip(&layout, at);
    let esc = esc_pressed();
    // The key that turned the mode on (this frame, `entering`) does not turn it off again.
    let toggle = !entering && key(keymap::key(Action::Photo));
    let leave = toggle || pressed[2] || (esc && shown);
    if leave {
        if let Some(p) = photo::leave() {
            view.look = p.saved_look;
        }
        return PhotoStep::Left;
    }
    // A drag: a left one pans, a right one turns and tilts; a left press and release without moving
    // is a tap, which brings a hidden strip back.
    let mut drag = photo::with(|p| p.drag).flatten();
    if drag.is_none() && !input_blocked() && !on_strip {
        if crate::ui::input::is_mouse_button_pressed(MouseButton::Left) {
            drag = Some(photo::Drag::start(photo::Button::Left, at));
        } else if crate::ui::input::is_mouse_button_pressed(MouseButton::Right) {
            drag = Some(photo::Drag::start(photo::Button::Right, at));
        }
    }
    let (mut pan_px, mut turn_px, mut tapped) = ((0.0f32, 0.0f32), (0.0f32, 0.0f32), false);
    if let Some(mut d) = drag {
        let button = if d.button == photo::Button::Left { MouseButton::Left } else { MouseButton::Right };
        let down = is_mouse_button_down(button);
        match d.step(at, down) {
            photo::DragOut::Pan(v) => pan_px = v,
            photo::DragOut::Turn(v) => turn_px = v,
            photo::DragOut::None => {}
        }
        if down {
            drag = Some(d);
        } else {
            tapped = d.button == photo::Button::Left && !d.moved;
            drag = None;
        }
    }
    // The keys held turn, tilt and pan at their rates; the wheel, pinch and +/- zoom.
    let dt = crate::ui::clock::frame_dt().clamp(0.0, 0.1);
    let axis = |neg: KeyCode, pos: KeyCode| (crate::ui::input::is_key_down(pos) as i32 - crate::ui::input::is_key_down(neg) as i32) as f32;
    let yaw_keys = axis(KeyCode::Q, KeyCode::E) * photo::KEY_YAW * dt;
    let pitch_keys = axis(KeyCode::PageUp, KeyCode::PageDown) * photo::KEY_PITCH * dt;
    let view_dir = (axis(KeyCode::Left, KeyCode::Right), axis(KeyCode::Up, KeyCode::Down));
    let keys_pan = (-view_dir.0 * photo::KEY_PAN * dt, -view_dir.1 * photo::KEY_PAN * dt);
    // Two fingers: the twist turns (clockwise, as E), their slide tilts; one finger's swipe pans.
    let turn = crate::ui::input::map_turn().to_degrees();
    let pair = crate::ui::input::map_pair();
    let swipe = crate::ui::input::map_pan();
    let mut notches = wheel();
    if key(KeyCode::Equal) || key(KeyCode::KpAdd) {
        notches += 1.0;
    }
    if key(KeyCode::Minus) || key(KeyCode::KpSubtract) {
        notches -= 1.0;
    }
    let floor = {
        let map = &game.world.map;
        let ms = map_scale();
        zoom_floor(map.w as f32 * PX * ms, map.h as f32 * map.grid.row_height() * PX * ms, map_area())
    };
    let bounds = photo::zoom_bounds(floor);
    let shutter = pressed[0] || key(KeyCode::Space) || key(KeyCode::Enter);
    let strip = if pressed[1] && shown {
        false
    } else if !shown && (esc || tapped) {
        true
    } else {
        shown
    };
    let shot = photo::with(|p| {
        p.strip = strip;
        p.drag = drag;
        p.down = held;
        p.yaw = photo::wrap_yaw(p.yaw + yaw_keys + turn_px.0 * photo::DRAG_YAW + turn);
        p.pitch = photo::clamp_pitch(p.pitch + pitch_keys + turn_px.1 * photo::DRAG_PITCH + pair.1 * photo::DRAG_PITCH);
        p.zoom = photo::zoom_by(p.zoom, notches, bounds);
        std::mem::take(&mut p.shot) || shutter
    })
    .unwrap_or(false);
    view.zoom = photo::with(|p| p.zoom).unwrap_or(view.zoom);
    photo_pan(game, view, (pan_px.0 + swipe.0 + keys_pan.0, pan_px.1 + swipe.1 + keys_pan.1));
    // A one-finger swipe pans over the whole screen; the strip's panel is no pan, and a tap there is the
    // button's (`ui::gestures`, `SwipeKind::Off`).
    crate::ui::input::swipe_area(Rect::new(0.0, 0.0, sw, sh), SwipeKind::Pan);
    if strip {
        crate::ui::input::swipe_area(layout.panel, SwipeKind::Off);
    }
    clear_background(rgb(10, 12, 10));
    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    let now = crate::ui::clock::now();
    frame_probe::time(Stage::World, || map_scale::draw(map_render_scale(), rgb(10, 12, 10), || draw_world(game, assets, &cam, None, None, None, true, Veil { shows: &[], now }, None)));
    if shot {
        take_shot(view, now);
    }
    if strip {
        photo::draw_strip(&layout, held, super::display::dp_px());
    }
    // Photo mode has no pointer on screen except over the strip's buttons, where the arrow
    // shows so a click has a target (the owner's request, 2026-11-10); the shot never has one
    // (it is read before the pointer is drawn).
    let over_button = shown && layout.buttons.iter().any(|r| r.contains(vec2(at.0, at.1)));
    cursor::set(if over_button { Shape::Arrow } else { Shape::Hidden });
    let base = if strip { layout.panel.y - 8.0 * super::chrome::k() } else { sh - 14.0 * super::chrome::k() };
    draw_toasts(&mut view.toasts, now, base, super::display::dp_px().map(|d| 16.0 * d));
    PhotoStep::Drawn
}

/// The trailer's stills (`RAZDOR_TRAILER_CAM`, dev-only) show the world alone: no bar, pointer,
/// minimap, compass, hover outline, tooltips, messages, marks or debug overlay, and no border line.
fn trailer_clean() -> bool {
    trailer().is_some()
}

fn push_model(m: Mat4) {
    // SAFETY: called on the main thread while macroquad runs, as every draw.
    unsafe { get_internal_gl() }.quad_gl.push_model_matrix(m);
}

fn pop_model() {
    // SAFETY: as `push_model`.
    unsafe { get_internal_gl() }.quad_gl.pop_model_matrix();
}

fn push_cpu_model(m: Mat4) {
    // SAFETY: called on the main thread while macroquad runs, as every draw.
    unsafe { get_internal_gl() }.quad_gl.push_cpu_model_matrix(m);
}

fn pop_cpu_model() {
    // SAFETY: as `push_cpu_model`.
    unsafe { get_internal_gl() }.quad_gl.pop_cpu_model_matrix();
}

/// A camera's 2.5D view: the tilted camera, the ground the view's frame shows (`plan`, in world
/// units: the minimap's rectangle) and the ground the drawing culls to (`cull`: the frame and
/// the strip under the bar).
#[derive(Clone, Copy)]
struct Tilt {
    cam: TiltCamera,
    plan: Rect,
    cull: Rect,
}

struct Camera {
    /// World pixel (at this zoom) at the top-left of the map view.
    origin: Vec2,
    /// The view above the bottom bar: the scroll range and the minimap's window are kept to it.
    view: Rect,
    /// What is drawn: the view and the strip under it, beside the bar (Razdor's choice: the map
    /// shows where the bar does not cover it, down to the screen's edge).
    draw: Rect,
    /// Screen pixels per world unit.
    scale: f32,
    grid: Grid,
    /// The 2.5D view (`tilt_view`), or None for the flat view. The fields above are the 2D space
    /// the drawing works in ([`Camera::tilted_space`]); [`Camera::to_screen`] is that space, and
    /// [`Camera::screen_at`] the window's.
    tilt: Option<Tilt>,
}

impl Camera {
    /// Centred on world position `at` (clamped to the map), in the map view left of the
    /// side panel.
    fn looking_at(game: &Game, zoom: f32, at: (f32, f32)) -> Camera {
        Camera::looking_in(game, zoom, at, map_area())
    }

    fn looking_in(game: &Game, zoom: f32, at: (f32, f32), view: Rect) -> Camera {
        // A trailer shot's target (`ui::trailer`) replaces the centre the view looks at.
        let at = super::trailer::target().unwrap_or(at);
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
        if extend_edge() {
            // «Продолжать карту за краем»: the view may reach past each edge as far as the fog
            // goes (`terrain::EDGE_FULL`), and a map narrower than the view is centred, so the
            // extension shows on every side. Off, the view stays on the map as it always did.
            let over = vec2(terrain::EDGE_FULL * scale, terrain::EDGE_FULL * rh * scale);
            origin.x = if world.x <= view.w { (world.x - view.w) / 2.0 } else { origin.x.clamp(-over.x, world.x - view.w + over.x) };
            origin.y = if world.y <= view.h { (world.y - view.h) / 2.0 } else { origin.y.clamp(-over.y, world.y - view.h + over.y) };
        } else {
            origin.x = origin.x.clamp(0.0, (world.x - view.w).max(0.0));
            origin.y = origin.y.clamp(0.0, (world.y - view.h).max(0.0));
        }
        let draw = Rect::new(view.x, view.y, view.w, view.h + bar_h());
        let mut cam = Camera { origin: origin - pad, view, draw, scale, grid: map.grid, tilt: None };
        // A view with no height (a minimised window) has no perspective: it stays flat. Photo mode
        // is always tilted, with its own camera whatever the view setting (`ui::photo`).
        let photo_angles = photo::angles();
        if (tilt_view() || photo_angles.is_some()) && view.w >= 1.0 && view.h >= 1.0 {
            let target = Vec2::from(cam.centre());
            if let Some((pitch, yaw)) = photo_angles {
                cam.tilt = Some(photo_tilt(target, scale, view, draw, pitch, yaw));
            } else if eyes_view() {
                // The eyes stand on the hero's displayed place (a shot's target in a run), not on the view's
                // centre, which the map's edges move.
                let ground = if super::trailer::active() { Vec2::from(at) } else { Vec2::from(game.display_pos()) };
                cam.tilt = Some(eyes_tilt(ground, scale, view, draw, tilt_yaw(), horizon(), trailer()));
            } else if trailer().is_none() && first_person() {
                cam.tilt = Some(first_person_tilt(target, scale, view, draw, tilt_yaw(), horizon()));
            } else {
                let (tilt, region) = match trailer() {
                    None => (TiltCamera::new(target, scale, view, tilt_pitch(), tilt_yaw()), None),
                    Some(t) => trailer_camera(&t, target, scale, view, tilt_pitch(), tilt_yaw()),
                };
                cam.tilt = Some(match region {
                    Some(region) => Tilt { cam: tilt, plan: region, cull: region },
                    None => Tilt { cam: tilt, plan: tilt.ground_footprint(view, view), cull: tilt.ground_footprint(view, draw) },
                });
            }
        }
        cam
    }

    /// The world point at the middle of the view (where the camera really looks, the map's
    /// border taken into account).
    fn centre(&self) -> (f32, f32) {
        ((self.origin + vec2(self.view.w, self.view.h) / 2.0) / self.scale).into()
    }

    /// The 2D space of a tilted camera: the cull footprint's ground in world pixels, from its
    /// corner, culled and drawn by the flat functions as the view is ([`in_view`]).
    fn tilted_space(&self) -> Camera {
        let cull = self.tilt.map_or(self.draw, |t| t.cull);
        let size = vec2(cull.w, cull.h) * self.scale;
        let space = Rect::new(0.0, 0.0, size.x, size.y);
        Camera { origin: vec2(cull.x, cull.y) * self.scale, view: space, draw: space, scale: self.scale, grid: self.grid, tilt: self.tilt }
    }

    /// Where a world point is in the window: the projection of the tilted view, else its flat
    /// position ([`Camera::to_screen`]). For what is drawn on top of the map, not in its 2D space.
    fn screen_at(&self, p: (f32, f32)) -> Vec2 {
        match self.tilt {
            None => self.to_screen(p),
            Some(t) => t.cam.project(vec3(p.0, 0.0, p.1), self.view).unwrap_or(vec2(-1e5, -1e5)),
        }
    }

    /// Where a cell's centre is in the window ([`Camera::screen_at`]).
    fn screen_cell(&self, t: Tile) -> Vec2 {
        self.screen_at(self.grid.center(t))
    }

    /// Pixels per world unit along the window's x and y: the flat scale, and tilted, the rows'
    /// foreshortened one (a drag's pixels to world units).
    fn screen_scale(&self) -> Vec2 {
        match self.tilt {
            None => Vec2::splat(self.scale),
            Some(t) => vec2(self.scale, self.scale * t.cam.pitch_deg.to_radians().sin()),
        }
    }

    /// World units moved by a screen move of `d` pixels (y down) with the look point: the ground
    /// under the pointer follows the move. Turned, the move is turned into the ground too.
    fn screen_to_world_delta(&self, d: Vec2) -> Vec2 {
        let s = self.screen_scale();
        let c = vec2(d.x / s.x, d.y / s.y);
        self.turn_ground(c)
    }

    /// A ground step given in the screen's plane (x right, y down the screen, in world units),
    /// turned by the view's yaw; the step itself at yaw 0 and in the flat view.
    fn turn_ground(&self, g: Vec2) -> Vec2 {
        match self.tilt {
            None => g,
            Some(t) => t.cam.ground_of_screen(g),
        }
    }

    /// A ground direction (2D space) as the screen sees it (x right, y down): the sprites' facing
    /// rows follow the screen, not the map.
    fn facing(&self, g: Vec2) -> Vec2 {
        match self.tilt {
            None => g,
            Some(t) => t.cam.screen_of_ground(g),
        }
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
        if let Some(t) = self.tilt {
            return t.plan;
        }
        let o = self.origin / self.scale;
        Rect::new(o.x, o.y, self.view.w / self.scale, self.view.h / self.scale)
    }

    /// The part of the world drawn, in world units (the view and the strip beside the bar).
    fn drawn_world_rect(&self) -> Rect {
        let o = self.origin / self.scale;
        Rect::new(o.x, o.y, self.draw.w / self.scale, self.draw.h / self.scale)
    }

    /// Whether a map click or hover at `m` lands on the map: on what is drawn and not on the
    /// bar, whose buttons and time panel take their own clicks.
    fn over_map(&self, m: Vec2) -> bool {
        self.draw.contains(m) && !game_bar::bar_rect().contains(m)
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
    fn draw_fog(&self, game: &Game, shows: &[&Showing]) {
        in_view(self, |c| ground(c, || c.draw_fog_flat(game, shows)));
    }

    /// The drawn ground as a map texture's rectangles: where it is on screen (`dest`) and which
    /// texels of a one-texel-per-cell map texture it shows (`src`, in texels, past the map's own
    /// where the ground goes on; texel `i` is cell `i`, its centre at `i + ½`, rows over the row
    /// height). In the 2D space of a tilted camera too ([`Camera::tilted_space`]).
    fn fog_texels(&self) -> (Rect, Rect) {
        let r = self.drawn_world_rect();
        let rh = self.grid.row_height();
        (self.draw, Rect::new(r.x + 0.5, r.y / rh + 0.5, r.w, r.h / rh))
    }

    /// The mist over the fog's texture (`ui::mist`, «Туман вместо черноты»). The tilted views' ground is its
    /// lowest sheet alone, and the others are raised ([`Camera::draw_raised_mist`]). The flat view draws every
    /// sheet on the ground; with `RAZDOR_MIST_RAISED=0` the tilted views do too, with the 2.5D parallax.
    fn draw_mist(&self, tex: &Texture2D, dest: Rect, src: Option<Rect>) {
        if self.tilt.is_some() && mist::raised_count() > 0 {
            return mist::draw_ground(tex, dest, src);
        }
        let parallax = match &self.tilt {
            Some(t) => mist::parallax(t.cam.yaw_deg, t.cam.pitch_deg),
            None => Vec2::ZERO,
        };
        mist::draw(tex, dest, src, parallax);
    }

    /// The mist's rectangles over the fog texture: where it is drawn (`dest`) and which texels (`src`, None
    /// for all of it), as [`Camera::draw_fog_flat`] draws them.
    fn mist_rects(&self, game: &Game) -> (Rect, Option<Rect>) {
        if extend_edge() {
            let (dest, src) = self.fog_texels();
            (dest, Some(src))
        } else {
            let (tl, br) = self.fog_corners(game);
            (Rect::new(tl.x, tl.y, br.x - tl.x, br.y - tl.y), None)
        }
    }

    /// The raised sheets of the mist (`mist::raised_count`, «Туман вместо черноты» in the tilted views): each a
    /// copy of the fog quad lifted off the ground. In 2.5D it is drawn on the ground's pass, lifted straight up
    /// ([`view25::raised_ground_model`]). In a bent view the picture holds the ground, so the raised sheet is a
    /// mesh of its own, lifted along the sheet's normal ([`bent_mist_meshes`]), in the sprites' run. Drawn over
    /// the ground's mist and under the reveals, as the mist itself is.
    fn draw_raised_mist(&self, game: &Game, shows: &[&Showing]) {
        let Some(tilt) = self.tilt else { return };
        let n = mist::raised_count();
        if n == 0 || !mist::on() || trailer_nofog() {
            return;
        }
        let settle = shows.iter().filter(|s| s.mask.is_some()).flat_map(|s| s.reveal_cells.iter().map(|&(i, _)| i));
        let (held, origin) = (game.unshown_cells(), game.tile());
        let Some(tex) = minimap::fog_layer(&game.fog, &held, origin, settle) else { return };
        let (dest, src) = self.mist_rects(game);
        let pass = PASS.with(std::cell::Cell::get);
        for sheet in 1..=n {
            let lift = mist::raised_height(sheet);
            match (pass, tilt.cam.bend) {
                // The ground's pass of a bent view draws the picture (no raised sheet in it).
                (Pass::Ground, _) => {}
                (Pass::Sprites, Some(bend)) => {
                    let meshes = bent_mist_meshes(&bend, self.origin, self.scale, dest, src, &tex, lift);
                    mist::raised(&tex, sheet, lift, || meshes.iter().for_each(macroquad::models::draw_mesh));
                }
                (Pass::Sprites, None) => {}
                (Pass::Plain, _) => {
                    push_model(view25::raised_ground_model(self.origin, self.scale, lift));
                    let params = DrawTextureParams { dest_size: Some(dest.size()), source: src, ..Default::default() };
                    mist::raised(&tex, sheet, lift, || draw_texture_ex(&tex, dest.x, dest.y, WHITE, params.clone()));
                    pop_model();
                }
            }
        }
    }

    fn draw_fog_flat(&self, game: &Game, shows: &[&Showing]) {
        if trailer_nofog() {
            return;
        }
        // The reveal cells of the places being shown jump to their target: the mask draws their
        // circle, so the fog under it does not fade a second time (`minimap::fog_texture`).
        let settle = shows.iter().filter(|s| s.mask.is_some()).flat_map(|s| s.reveal_cells.iter().map(|&(i, _)| i));
        let (held, origin) = (game.unshown_cells(), game.tile());
        // The same texture, drawn as mist instead of black when the setting is on (the minimap's stays black).
        let mist = mist::on();
        // «Продолжать карту за краем»: the fog goes on past the edges too (its texels clamp there).
        if extend_edge() {
            let (dest, src) = self.fog_texels();
            if mist {
                if let Some(tex) = minimap::fog_layer(&game.fog, &held, origin, settle) {
                    self.draw_mist(&tex, dest, Some(src));
                }
            } else {
                minimap::draw_fog_extended(&game.fog, &held, origin, settle, dest, src);
            }
            return;
        }
        let (tl, br) = self.fog_corners(game);
        if mist {
            if let Some(tex) = minimap::fog_layer(&game.fog, &held, origin, settle) {
                self.draw_mist(&tex, Rect::new(tl.x, tl.y, br.x - tl.x, br.y - tl.y), None);
            }
        } else if self.tilt.is_some() {
            // The 2.5D view (a tilted camera) draws only the fog's dark box (`minimap::draw_fog_boxed`);
            // the flat map view draws the whole quad.
            minimap::draw_fog_boxed(&game.fog, &held, origin, settle, tl, br);
        } else {
            minimap::draw_fog(&game.fog, &held, origin, settle, tl, br);
        }
        let v = self.draw;
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
        in_view(self, |c| ground(c, || c.draw_known_place_flat(s, now)));
    }

    fn draw_known_place_flat(&self, s: &Showing, now: f64) {
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
        let shows: Vec<&'s Showing> = shows.into_iter().collect();
        in_view(self, |c| ground(c, || c.draw_showing_flat(game, shows.clone(), now)));
    }

    fn draw_showing_flat(&self, game: &Game, shows: Vec<&Showing>, now: f64) {
        if trailer_nofog() {
            return;
        }
        let (tl, br) = self.fog_corners(game);
        for s in shows {
            let Some(mask) = &s.mask else { continue };
            let p = s.progress(now);
            if p >= 1.0 {
                continue;
            }
            if p > 0.0 {
                // Ease out: quick at first, slowing as it reaches the edges.
                let r = (1.0 - (1.0 - p) * (1.0 - p)) * s.reach;
                let Some(image) = &s.mask_image else { continue };
                let mut image = image.borrow_mut();
                for &(i, d) in &s.reveal_cells {
                    let keep = ((d - r) / SHOW_RIM + 0.5).clamp(0.0, 1.0);
                    image.bytes[i * 4 + 3] = minimap::reveal_overlay((s.before[i] as f32 * keep) as u8, s.after[i]);
                }
                mask.update(&image);
            }
            if extend_edge() {
                let (dest, src) = self.fog_texels();
                minimap::draw_extended(mask, dest, src);
            } else {
                super::sharp::plain_draw();
                draw_texture_ex(mask, tl.x, tl.y, WHITE, DrawTextureParams { dest_size: Some(br - tl), ..Default::default() });
            }
        }
    }

    fn tile_under_mouse(&self) -> Option<Tile> {
        let m = Vec2::from(crate::ui::widgets::pointer());
        if !self.over_map(m) {
            return None;
        }
        self.tile_under_pixel(m)
    }

    /// The cell under the window's pixel `m`, flat or tilted (the ground under it). Tilted, a
    /// pixel over the sky sees no ground: None.
    fn tile_under_pixel(&self, m: Vec2) -> Option<Tile> {
        let m = map_point_at(self, m);
        if self.tilt.is_some() && !m.is_finite() {
            return None;
        }
        let w = (m - vec2(self.draw.x, self.draw.y) + self.origin) / self.scale;
        Some(self.grid.tile_at((w.x, w.y)))
    }

    /// Visible cell ranges (cols, rows), half-open, clamped to the map.
    fn visible(&self, map: &TileMap) -> ((i32, i32), (i32, i32)) {
        let top_left = self.origin / self.scale;
        let rh = self.grid.row_height();
        let c0 = (top_left.x - 1.0).floor() as i32;
        let r0 = (top_left.y / rh - 1.0).floor() as i32;
        let c1 = c0 + (self.draw.w / self.scale) as i32 + 3;
        let r1 = r0 + (self.draw.h / (self.scale * rh)) as i32 + 3;
        ((c0.max(0), c1.min(map.w)), (r0.max(0), r1.min(map.h)))
    }
}

/// The map's pointer in the camera's 2D space ([`Camera::to_screen`]). On the flat view it is the
/// pointer itself. On the 2.5D view it is the point of the ground under the pointer, NaN where
/// the pointer sees no ground (above the horizon).
fn map_pointer(cam: &Camera) -> Vec2 {
    map_point_at(cam, Vec2::from(crate::ui::widgets::pointer()))
}

/// [`map_pointer`] for the window's pixel `m`.
fn map_point_at(cam: &Camera, m: Vec2) -> Vec2 {
    match cam.tilt {
        None => m,
        Some(t) => t.cam.pick(m, cam.view).map_or(Vec2::NAN, |g| cam.to_screen((g.x, g.y))),
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
            super::sharp::draw(
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

/// The terrain of the map as the blended layer (`terrain::TerrainLayer`): the quad over the drawn
/// ground, in `mode` (see [`terrain::Mode`]). `cam` is the 2D space ([`in_view`]).
fn terrain_quad(game: &Game, layer: &terrain::TerrainLayer, cam: &Camera, mode: terrain::Mode, flow: &[(f32, f32)], relief: Option<daylight::Relief>) {
    let map = &game.world.map;
    // The map area in view, in world units (cells span ±½ around their centres).
    let rh = cam.grid.row_height();
    let (map_tl, map_br) = (vec2(-0.5, -0.5 * rh), vec2(map.w as f32 - 0.5, (map.h as f32 - 0.5) * rh));
    let w = cam.drawn_world_rect();
    // «Продолжать карту за краем»: the whole drawn ground, and past the map's edges the ground
    // goes on (a short strip, then the fog); otherwise the map's area in view.
    let extend = extend_edge();
    let (tl, br) = if extend { (w.point(), w.point() + w.size()) } else { (map_tl.max(w.point()), map_br.min(w.point() + w.size())) };
    if tl.x < br.x && tl.y < br.y {
        let (a, b) = (cam.to_screen(tl.into()), cam.to_screen(br.into()));
        if matches!(mode, terrain::Mode::Full | terrain::Mode::Ground) {
            frame_probe::terrain(b.x - a.x, b.y - a.y);
        }
        let view = vec4(tl.x, tl.y / rh, br.x, br.y / rh);
        // The ground lies in the map's axes: the light (screen axes) turns with the camera's yaw.
        let relief = relief.map(|r| r.turned(cam.tilt.map_or(0.0, |t| t.cam.yaw_deg)));
        // The edge's mist is fog: the trailer's no-fog switch leaves the ground going on past the edge.
        layer.draw(map.surface_codes(), (map.w as u32, map.h as u32), Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), view, vec2(PX, PX * rh), Some((crate::ui::clock::now() * 1000.0) as i64), extend && !trailer_nofog(), mode, flow, relief);
    }
}

/// The map's ground (the terrain layer, or square cells without it), drawn under the camera `cam`
/// of the map frame. Option 2 (the terrain cache) applies to the map frame only, not to a
/// backdrop, and not inside a picture being filled: the ground is drawn once into the cache when
/// its key ([`ground_key`]) changes; then the cached ground is copied and the water and the
/// border's fog drawn over it.
fn draw_ground(game: &Game, art: Option<&DtArt>, cam: &Camera, waits: bool, relief: Option<daylight::Relief>) {
    let Some(layer) = art.and_then(|a| a.terrain_layer()).filter(|_| cam.grid == Grid::Square8) else {
        return in_view(cam, |c| ground(c, || draw_terrain(game, art, c)));
    };
    // «Живая вода»: the rivers' flow of the map, while the water is alive (else still).
    let flow = map_flow(game);
    let flow: &[(f32, f32)] = flow.as_deref().map_or(&[], |f| f.as_slice());
    super::gpu_probe::timed(|| {
        // A bent view draws its ground in `draw_world`'s picture (`ground_in_bent`), never here;
        // the cache's fill would nest inside that picture, so a bent view is not cached.
        let plain = cam.tilt.is_none_or(|t| t.cam.bend.is_none());
        let cached = (waits && layer.cache_on() && super::render_cache::target().is_none() && plain).then(|| layer.ground_target(ground_key(game, cam, relief))).flatten();
        match cached {
            Some((rt, fill)) => {
                if fill {
                    super::render_cache::fill(&rt, || in_view(cam, |c| ground(c, || terrain_quad(game, layer, c, terrain::Mode::Ground, flow, relief))));
                }
                in_view(cam, |c| ground(c, || terrain_quad(game, layer, c, terrain::Mode::Cached, flow, relief)));
                in_view(cam, |c| ground(c, || terrain_quad(game, layer, c, terrain::Mode::Overlay, flow, relief)));
            }
            None => in_view(cam, |c| ground(c, || terrain_quad(game, layer, c, terrain::Mode::Full, flow, relief))),
        }
    });
}

local! {
    /// The rivers' flow of the map (`rules::flow`) and the key it was made for: the map's size, its
    /// surface codes and its hills (the objects that stand on the land).
    static FLOW: std::cell::RefCell<Option<(u64, std::rc::Rc<Vec<(f32, f32)>>)>> = std::cell::RefCell::new(None);
}

/// The rivers' flow of the game's map, kept while the map stays; `None` while the water is still
/// (the setting off), when the flow is not drawn.
fn map_flow(game: &Game) -> Option<std::rc::Rc<Vec<(f32, f32)>>> {
    if !water::live() {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let map = &game.world.map;
    let key = {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (map.w, map.h, map.surface_codes()).hash(&mut h);
        for o in map.objects.iter().filter(|o| (object_class::HILLS..=object_class::DARK_MOUNTAINS).contains(&o.class)) {
            (o.tile, o.class).hash(&mut h);
        }
        h.finish()
    };
    Some(FLOW.with_borrow_mut(|c| {
        if let Some((k, f)) = c.as_ref() {
            if *k == key {
                return f.clone();
            }
        }
        let f = std::rc::Rc::new(razdor::rules::flow::flow_field(map));
        *c = Some((key, f.clone()));
        f
    }))
}

/// The key of the cached ground: everything the ground pass reads (the screen, the camera and
/// its 2.5D tilt, the map's size and surface codes, and the edge's mode). The clock and the water
/// are not in it: they are drawn over the cache each frame.
fn ground_key(game: &Game, cam: &Camera, relief: Option<daylight::Relief>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let f = |v: f32| v.to_bits();
    let rect = |r: Rect| [r.x, r.y, r.w, r.h].map(f);
    [screen_width(), screen_height(), screen_dpi_scale()].map(f).hash(&mut h);
    [cam.origin.x, cam.origin.y, cam.scale].map(f).hash(&mut h);
    rect(cam.view).hash(&mut h);
    rect(cam.draw).hash(&mut h);
    if let Some(t) = cam.tilt {
        (t.cam.pitch_deg.to_bits(), t.cam.yaw_deg.to_bits(), rect(t.plan), rect(t.cull)).hash(&mut h);
    }
    let map = &game.world.map;
    (map.w, map.h, cam.grid.row_height().to_bits(), extend_edge(), mist::on(), map.surface_codes()).hash(&mut h);
    // The relief («Освещение») is in the ground: it moves with the hour, so the key takes it to
    // 1/256 (a cycle's light refills the cache a few times a game hour, not every frame).
    if let Some(r) = relief {
        let q = |v: f32| (v * 256.0).round() as i32;
        [q(r.light[0]), q(r.light[1]), q(r.light[2]), q(r.strength), q(r.cavity)].hash(&mut h);
    }
    h.finish()
}

fn draw_terrain(game: &Game, art: Option<&DtArt>, cam: &Camera) {
    let map = &game.world.map;
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
    /// Card `k` of a building drawn as cards (2.5D, [`card_building`]): location, card.
    BuildingCard(usize, usize),
    Army(usize),
    /// The hero's ship, waiting where he left it.
    Ship,
    Hero,
    /// Layer 2 or 3 of the spell being played ([`SpellShow`]).
    Spell(u8),
    /// A bank of mist on the frontier (2.5D and first-person, [`push_banks`]): its foot, the unexplored side's
    /// shown darkness and its noise seed.
    Bank { foot: Vec2, darkness: f32, seed: f32 },
}

/// A spell's layers playing over the figure they landed on, for [`draw_world`].
struct SpellShow<'a> {
    layers: &'a [FxLayer],
    /// The target's place in world units (its cell's centre while it stands).
    at: (f32, f32),
    /// The cell of the target figure: its layers are drawn in that cell's pass (see [`draw_world`]).
    cell: Tile,
    /// Since the cast.
    ms: f64,
}

/// Draws `show`'s layer `n` (1..3) as the original does: its frame added to what is below,
/// each texel times the layer's red/green/blue (the quad's diffuse colour 0x00RRGGBB,
/// 0x48c5b4; stage 0 keeps D3D's default MODULATE, specular 0), with SRCBLEND = DESTBLEND =
/// ONE (layer 1 0x4c92ec/0x4c93e0, 2 0x4c9e63, 3 0x4ca1a1). Black adds nothing and a dark
/// tint only dims the light: the frames never darken the map. The frames are the additive
/// art made to glow ([`super::chrome::Fx::Glow`]), which [`super::chrome::additive`] adds
/// back to the same sum.
fn draw_spell_layer(show: &SpellShow, n: u8, cam: &Camera) {
    let c = cam.to_screen(show.at);
    let zoom = cam.scale / PX;
    for l in show.layers.iter().filter(|l| l.layer == n) {
        let Some(i) = l.frame(show.ms) else { continue };
        let Some(frames) = super::chrome::animation(&l.art) else { continue };
        let Some(f) = frames.get(i.min(frames.len().saturating_sub(1))) else { continue };
        let q = l.quad();
        let dest = Rect::new(c.x + q.x * zoom, c.y + q.y * zoom, q.w * zoom, q.h * zoom);
        super::chrome::additive(|| super::chrome::tex(f, dest, faded(l.tint)));
    }
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
/// moves the plants of classes 9–11 by their jitter ([`plant_shift`]).
fn object_sprite_origin(o: &Decoration, w: f32, h: f32, cam: &Camera) -> Vec2 {
    let (x, y) = cam.grid.center(o.tile);
    let corner = cam.to_screen((x + 0.5, y + cam.grid.row_height() / 2.0));
    let (dx, dy) = match o.class {
        1..=8 => (16.0, 11.0),
        _ => plant_shift(o),
    };
    corner + vec2(dx - w - 0.5, dy - h - 0.5) * (cam.scale / PX)
}

/// The jitter of a plant's picture in pixels (classes 9–11, [`razdor::rules::rng::plant_offset`]),
/// nothing for the other classes.
fn plant_shift(o: &Decoration) -> (f32, f32) {
    match o.class {
        object_class::TREES..=object_class::THICKET => {
            let (dx, dy, _) = super::memo::plant_jitter_memo(o.tile.0, o.tile.1, u16::from(o.class) << 8 | u16::from(o.sprite));
            (dx as f32, dy as f32)
        }
        _ => (0.0, 0.0),
    }
}

/// The foot of a map object's sprite in the 2D space: the centre of the ground it stands on. A
/// billboard turns about its foot, so the foot must be the point the picture is on the ground at:
/// a hill or mountain stands on the centre of the cells it covers (`object_cells`, `object_side`
/// cells square up and left of its own; its picture is 32·(side + 1) px wide, centred on them); a
/// plant on its cell's centre moved by its jitter, as its picture is ([`plant_shift`]); anything
/// else on its cell's centre. The sort key uses it too ([`sprite_key`]).
fn object_foot(o: &Decoration, cam: &Camera) -> Vec2 {
    match o.class {
        1..=8 => {
            let (sum, n) = razdor::rules::map::object_cells(o).fold((Vec2::ZERO, 0u32), |(s, n), t| (s + cam.cell_centre(t), n + 1));
            if n == 0 {
                return cam.cell_centre(o.tile);
            }
            sum / n as f32
        }
        _ => {
            let (dx, dy) = plant_shift(o);
            cam.cell_centre(o.tile) + vec2(dx, dy) * (cam.scale / PX)
        }
    }
}

/// The foot of a building's sprite in the 2D space: the centre of its footprint (the mean of its
/// cells' centres), the point it stands on. A billboard turns about its foot, so a corner would
/// make a big building slide across the ground when the view turns; the sort key uses it too
/// ([`sprite_key`]). Without cells (none in a real map) the anchor's centre. The picture's bottom
/// edge lies half the footprint's height below this point, on the footprint's south edge, for
/// every picture of the install (tested over all of them in `tilt_tests`).
fn building_foot(l: &Location, cam: &Camera) -> Vec2 {
    let (sum, n) = l.cells().fold((Vec2::ZERO, 0u32), |(s, n), t| (s + cam.cell_centre(t), n + 1));
    if n == 0 {
        return cam.cell_centre(l.anchor);
    }
    sum / n as f32
}

/// The part `src` of `texture` drawn at `at`, `size` big, its top edge moved `lean` px right.
fn draw_leaning(texture: &Texture2D, src: Rect, at: Vec2, size: Vec2, lean: f32) {
    let texel = super::sharp::texels(texture);
    let (tw, th) = (texel.x, texel.y);
    let (u0, v0, u1, v1) = (src.x / tw, src.y / th, (src.x + src.w) / tw, (src.y + src.h) / th);
    let white = faded(WHITE);
    let vertex = |x: f32, y: f32, u: f32, v: f32| Vertex::new(x, y, 0.0, u, v, white);
    // One quad mesh, reused by every swaying tree (its vertex buffer keeps its capacity).
    LEAN_MESH.with(|m| {
        let mut m = m.borrow_mut();
        m.vertices.clear();
        m.vertices.extend([
            vertex(at.x + lean, at.y, u0, v0),
            vertex(at.x + size.x + lean, at.y, u1, v0),
            vertex(at.x + size.x, at.y + size.y, u1, v1),
            vertex(at.x, at.y + size.y, u0, v1),
        ]);
        m.texture = Some(texture.clone());
        super::sharp::draw_mesh_sharp(&m, vec2(size.x / src.w, size.y / src.h));
    });
}

local! {
    /// The swaying trees' quad (`draw_leaning`): indices fixed, vertices and texture refilled.
    static LEAN_MESH: std::cell::RefCell<Mesh> = std::cell::RefCell::new(Mesh { vertices: Vec::new(), indices: vec![0, 1, 2, 0, 2, 3], texture: None });
    /// The map's painter's list (`draw_world`): cleared and refilled each frame.
    static WORLD_ITEMS: std::cell::RefCell<Vec<(i64, Drawable)>> = std::cell::RefCell::new(Vec::new());
}

fn draw_object(o: &Decoration, art: Option<&DtArt>, cam: &Camera) {
    let c = cam.cell_centre(o.tile);
    let base = vec2(c.x, c.y + cam.cell_size().y / 2.0);
    let zoom = cam.scale / PX;
    if let Some((atlas, r)) = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.decoration(o.class, o.sprite)?))) {
        let at = object_sprite_origin(o, r.w, r.h, cam);
        let size = vec2(r.w * zoom, r.h * zoom);
        if o.class == object_class::TREES {
            // The small trees and bushes sway: the quad's top corners move by the lean of
            // their phase, the bottom stays (0x4c99d0, engine.md §7).
            let (_, _, phase) = super::memo::plant_jitter_memo(o.tile.0, o.tile.1, u16::from(o.class) << 8 | u16::from(o.sprite));
            let lean = razdor::rules::rng::tree_sway(clock_ms(), phase) * zoom;
            draw_leaning(&atlas.texture, r, at, size, lean);
            return;
        }
        frame_probe::sprite(size.x, size.y);
        super::sharp::draw(&atlas.texture, at.x, at.y, faded(WHITE), DrawTextureParams { dest_size: Some(size), source: Some(r), ..Default::default() });
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

/// The outline of the building under the pointer (Razdor's choice, see [`MapHoverLook`]).
const BUILDING_OUTLINE: Color = Color::new(1.0, 0.82, 0.42, 0.9);

/// The eight steps of a building's outline, each a multiple of its width (`draw_building`).
const OUTLINE_STEPS: [(f32, f32); 8] = [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0), (-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)];

/// A building drawn as cards in the 2.5D view: its sprite's rectangle in the atlas and its cards,
/// back to front. `None` for a bridge (it lies on the ground) and for a picture with no cut data
/// (one billboard, as before).
fn card_building<'a>(l: &Location, art: Option<&'a DtArt>) -> Option<(&'a Atlas, Rect, &'a [Card])> {
    if l.kind.is_bridge() {
        return None;
    }
    let atlas = art?.map_atlas()?;
    let cards = atlas.building_cards(l.picture.0, l.picture.1)?;
    Some((atlas, atlas.building(l.picture.0, l.picture.1)?, cards))
}

/// A card's foot in the 2D space: its ground point in the building's sprite whose top-left is at
/// `origin`, drawn `zoom` screen pixels to a sprite pixel.
fn card_foot(origin: Vec2, zoom: f32, card: &Card) -> Vec2 {
    origin + vec2(card.foot.0, card.foot.1) * zoom
}

/// The mesh of a card of a building's sprite whose top-left is at `origin_px` on screen, `zoom`
/// screen pixels to a sprite pixel: the outline's vertices at their screen places, their uvs in the
/// atlas texture (`atlas_size`) where the sprite's rectangle `src` lies, all of one `color`.
fn card_mesh(card: &Card, origin_px: Vec2, zoom: f32, src: Rect, atlas_size: Vec2, color: Color) -> (Vec<Vertex>, Vec<u16>) {
    let vertices = card
        .verts
        .iter()
        .map(|&(x, y)| {
            let p = origin_px + vec2(x, y) * zoom;
            let uv = vec2(src.x + x, src.y + y) / atlas_size;
            Vertex::new(p.x, p.y, 0.0, uv.x, uv.y, color)
        })
        .collect();
    (vertices, card.tris.clone())
}

/// Draws a card of a building as its mesh, with the sharp sampling of the other sprites. Outlined
/// (the pointer on the building), each card's gold silhouette goes under it, as `draw_building`
/// outlines the whole picture.
fn draw_card(card: &Card, origin: Vec2, zoom: f32, r: Rect, atlas: &Atlas, outlined: bool) {
    let size = vec2(atlas.texture.width(), atlas.texture.height());
    let mesh = |origin: Vec2, color: Color| {
        let (vertices, indices) = card_mesh(card, origin, zoom, r, size, color);
        Mesh { vertices, indices, texture: Some(atlas.texture.clone()) }
    };
    if outlined {
        let d = (1.5 * zoom).max(1.0);
        super::chrome::silhouette(|| {
            for (dx, dy) in OUTLINE_STEPS {
                super::sharp::draw_mesh_sharp(&mesh(origin + vec2(dx * d, dy * d), faded(BUILDING_OUTLINE)), vec2(zoom, zoom));
            }
        });
    }
    super::sharp::draw_mesh_sharp(&mesh(origin, faded(WHITE)), vec2(zoom, zoom));
}

/// A card building's shadow on the ground (2.5D): the whole sprite, placed as `draw_building`
/// places it, with only its shadow pixels drawn (`chrome::shadow_only`). It is drawn in the ground
/// pass before the fog, which then covers it where the ground is dark.
fn draw_building_shadow(l: &Location, atlas: &Atlas, r: Rect, cam: &Camera, shadow_k: f32) {
    let zoom = cam.scale / PX;
    let at = building_sprite_origin(l, r.w, r.h, cam);
    let params = DrawTextureParams { dest_size: Some(vec2(r.w * zoom, r.h * zoom)), source: Some(r), ..Default::default() };
    super::chrome::shadow_only(shadow_k, || super::sharp::draw(&atlas.texture, at.x, at.y, WHITE, params));
}

fn draw_building(l: &Location, art: Option<&DtArt>, cam: &Camera, outlined: bool) {
    let base = cam.to_screen(footprint_base(cam.grid, l));
    let zoom = cam.scale / PX;
    let sprite = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.building(l.picture.0, l.picture.1)?)));
    // The original draws just the sprite (RenderWorld_BuildingAndRest 0x4c9b5b), its owner
    // shown only on the minimap.
    if let Some((atlas, r)) = sprite {
        let at = building_sprite_origin(l, r.w, r.h, cam);
        let params = DrawTextureParams { dest_size: Some(vec2(r.w * zoom, r.h * zoom)), source: Some(r), ..Default::default() };
        frame_probe::sprite(r.w * zoom, r.h * zoom);
        if outlined {
            // The picture's solid shape in gold, a little out every way, under the picture.
            let d = (1.5 * zoom).max(1.0);
            super::chrome::silhouette(|| {
                for (dx, dy) in OUTLINE_STEPS {
                    super::sharp::draw(&atlas.texture, at.x + dx * d, at.y + dy * d, faded(BUILDING_OUTLINE), params.clone());
                }
            });
        }
        super::sharp::draw(&atlas.texture, at.x, at.y, faded(WHITE), params);
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
        let (edge, thick) = if outlined { (BUILDING_OUTLINE, 2.5) } else { (rgb(70, 60, 50), 1.5) };
        draw_rectangle_lines(base.x - w / 2.0, base.y - h, w, h, thick, edge);
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
        (crate::ui::clock::now() * 10.0) as u32 % 8
    } else {
        0
    }
}

/// The screen square a figure's sprite is drawn in, from its feet at world `pos`. None if the
/// install has no such sprite.
fn figure_rect(art: Option<&DtArt>, stem: &str, pos: (f32, f32), cam: &Camera, stand: Stand) -> Option<Rect> {
    let sheet = art.and_then(|a| a.figure_sheet(stem))?;
    let size = sheet.width() / 8.0 * cam.scale / PX;
    let p = cam.to_screen(pos);
    let dest = match stand {
        // The figure's own alpha holds its shadow (the .ugs frames); nothing is added under it.
        Stand::Feet => vec2(p.x - size / 2.0, p.y - size * 0.8),
        Stand::Afloat => vec2(p.x - size / 2.0, p.y - size * 0.55),
    };
    Some(Rect::new(dest.x, dest.y, size, size))
}

fn draw_figure(art: Option<&DtArt>, stem: &str, pos: (f32, f32), next: Option<(f32, f32)>, frame: u32, cam: &Camera, stand: Stand) -> bool {
    let Some(sheet) = art.and_then(|a| a.figure_sheet(stem)) else { return false };
    let Some(rect) = figure_rect(art, stem, pos, cam, stand) else { return false };
    let n = sheet.width() / 8.0;
    let p = cam.to_screen(pos);
    // The sheet's rows are the screen's directions, so a turned view turns the heading too.
    let heading = cam.facing(next.map_or(Vec2::ZERO, |n| cam.to_screen(n) - p));
    let row = facing_row(heading);
    let frame = frame.min(7) as f32;
    let (dest, size) = (vec2(rect.x, rect.y), rect.w);
    frame_probe::sprite(size, size);
    // The sheets hold premultiplied colour and the original draws every figure with
    // SRCBLEND ONE, DESTBLEND INVSRCALPHA (0x4c9ef6, 0x4ca0d9): the Ghost's light where its
    // alpha is 0 is added to the map, its glow.
    super::chrome::premultiplied(|| {
        super::sharp::draw(
            &sheet,
            dest.x,
            dest.y,
            faded_premultiplied(WHITE),
            DrawTextureParams { dest_size: Some(vec2(size, size)), source: Some(Rect::new(frame * n, row * n, n, n)), ..Default::default() },
        )
    });
    true
}

/// The figure under a screen point, of `(cell, sprite rect, feet)` entries: the one whose body
/// contains the point, its transparent margins left out (the middle 60% across, the top 15%
/// ignored), and among several the one whose feet are nearest. The body is drawn above its
/// feet, so a point can lie in the cell above a figure's cell.
fn figure_under(pointer: Vec2, figures: &[(Tile, Rect, Vec2)]) -> Option<Tile> {
    figures
        .iter()
        .filter(|(_, r, _)| Rect::new(r.x + r.w * 0.2, r.y + r.h * 0.15, r.w * 0.6, r.h * 0.85).contains(pointer))
        .min_by(|a, b| a.2.distance_squared(pointer).total_cmp(&b.2.distance_squared(pointer)))
        .map(|(t, _, _)| *t)
}

/// The cell a tap on `t` aims at. On a phone there is no hover, so a finger on an army's
/// figure lands on the cell above its feet. When no army stands on `t`, the army whose figure
/// is under the finger (among the cells the hero can target) is the target. Razdor's choice for
/// phones (a player's report): the original picks an army by its cell (interface.md §7.2,
/// world.md §1.3, 0x4cc583), and on PC the hover pointer shows the army under the mouse.
fn figure_aim(game: &Game, art: Option<&DtArt>, cam: &Camera, t: Tile, pointer: Vec2) -> Tile {
    let map = &game.world.map;
    // Tilted (2.5D), the figures' sprite rectangles are not where they are drawn: the cell under
    // the pointer decides (a simplification of this step).
    if cam.tilt.is_some() || crate::ui::display::dp_px().is_none() || game.world.armies.iter().any(|a| a.tile(map) == t) {
        return t;
    }
    let figures: Vec<(Tile, Rect, Vec2)> = game
        .world
        .armies
        .iter()
        .filter(|a| game.fog.explored(a.tile(map)) && !game.army_hidden(a) && game.can_target(a.tile(map)))
        .filter_map(|a| {
            let pos = game.army_display_pos(a);
            let feet = cam.to_screen(pos);
            let (stem, stand) = if a.sails() { (ship_stem(a.ship), Stand::Afloat) } else { (*FIGURES.get(a.figure as usize)?, Stand::Feet) };
            // Without the install's sprite: a cell wide and 1.6 cells tall above the feet.
            let rect = figure_rect(art, stem, pos, cam, stand).unwrap_or_else(|| Rect::new(feet.x - cam.scale / 2.0, feet.y - 1.6 * cam.scale, cam.scale, 1.6 * cam.scale));
            Some((a.tile(map), rect, feet))
        })
        .collect();
    figure_under(pointer, &figures).unwrap_or(t)
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

/// The mark under the hero (yellow), under an army (red) and on the route's end (green), as
/// the original's screen shows them.
const MARK_HERO: Color = Color::new(1.0, 0.92, 0.1, 1.0);
const MARK_ARMY: Color = Color::new(0.95, 0.15, 0.1, 0.55);
const MARK_ROUTE_END: Color = Color::new(0.2, 0.95, 0.2, 0.55);

/// A thin dashed ring on the ground at `pos` (the original's `Selection-*.lit` marks,
/// 0x4ce30c), the ring's light added to the ground (its blend
/// 0x4c9459, SRCCOLOR / INVSRCCOLOR). Without the picture, a thin ellipse. *(The original
/// turns it 5.625° every 100 ms.)*
fn draw_mark(cam: &Camera, pos: (f32, f32), texture: &str, color: Color) {
    let p = cam.to_screen(pos);
    let zoom = cam.scale / PX;
    // Sized as the original's screen shows it (about 30 × 20 at 1:1).
    let (w, h) = (32.0 * zoom, 22.0 * zoom);
    let angle = (crate::ui::clock::now() * 10.0).floor() as f32 * (std::f32::consts::PI / 32.0);
    match super::chrome::win_fx(texture, super::chrome::Fx::Glow) {
        Some(t) => super::chrome::ground_ring(&t, Rect::new(p.x - w / 2.0, p.y - h / 2.0, w, h), color, angle),
        None => draw_ellipse_lines(p.x, p.y, 15.0 * zoom, 10.0 * zoom, 0.0, 1.0, color),
    }
}

fn draw_army(game: &Game, a: &Army, assets: &Assets, art: Option<&DtArt>, cam: &Camera) {
    // Inside a building its figure is not drawn, only its ring under the building
    // (0x497abc, `World::hides_figure`).
    if game.army_hidden(a) {
        return;
    }
    let next = a.path.first().map(|&t| game.world.map.center(t));
    let pos = game.army_display_pos(a);
    // Its walk frames run by game time (engine.md §7), not by the clock on the wall; standing,
    // its idle animation by the wall clock (0x4c868c).
    let frame = idle_sprite_frame(game, a, clock_ms()).or_else(|| game.army_walk_frame(a)).unwrap_or(0);
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
    // The eyes view looks through his eyes: his figure is not drawn.
    if eyes_cam(cam) {
        return;
    }
    // His figure is his army record's, not his current unit type's: an event that makes him
    // another unit leaves it (0x4ad4cc, 0x4ad5ac).
    let model = game.hero_figure();
    // Inside a building his figure is not drawn, only his ring under the building
    // (0x497abc, `World::hides_figure`).
    if game.hero_hidden() {
        return;
    }
    // The frames play while he has a step to take; his row is the step's direction, also at a
    // stop (he keeps facing it, 0x75c050), the viewer's row before any step.
    let walking = game.display_heading();
    let heading = walking.or_else(|| game.standing_heading());
    let c = cam.to_screen(game.display_pos());
    if game.aboard() {
        // At rest aboard, his galley rocks through its 8 frames by the frame clock (0x4c868c).
        let frame = if !game.moving() && !game.waiting() && game.location.is_none() { idle::hero_ship_idle_frame(clock_ms()) } else { hero_frame(walking) };
        if !draw_figure(art, ship_stem(razdor::rules::ships::kind::HERO), game.display_pos(), heading, frame, cam, Stand::Afloat) {
            draw_ship(cam, game.display_pos(), HERO_SAIL);
        }
        return;
    }
    if !draw_figure(art, figure_stem(model), game.display_pos(), heading, hero_frame(walking), cam, Stand::Feet) {
        assets.draw_unit(game.hero().def, Team::Player, c.x, c.y - 10.0, 30.0);
    }
}

/// The drawn rectangle of a massif object or plant (its atlas picture, or the rough box of the shapes drawn
/// without one), in the 2D space of `cam`: the first-person view's near fade keys on it ([`covering_gap`]).
fn object_picture(o: &Decoration, art: Option<&DtArt>, cam: &Camera) -> Rect {
    if let Some(r) = art.and_then(|a| a.map_atlas()).and_then(|at| at.decoration(o.class, o.sprite)) {
        let at = object_sprite_origin(o, r.w, r.h, cam);
        let zoom = cam.scale / PX;
        return Rect::new(at.x, at.y, r.w * zoom, r.h * zoom);
    }
    let base = cam.cell_centre(o.tile);
    let s = cam.scale;
    Rect::new(base.x - s, base.y - 2.0 * s, 2.0 * s, 2.0 * s)
}

/// The drawn rectangle of a building (its atlas picture, or the box of the shape drawn without one), in the
/// 2D space of `cam`.
fn building_picture(l: &Location, art: Option<&DtArt>, cam: &Camera) -> Rect {
    if let Some(r) = art.and_then(|a| a.map_atlas()).and_then(|at| at.building(l.picture.0, l.picture.1)) {
        let at = building_sprite_origin(l, r.w, r.h, cam);
        let zoom = cam.scale / PX;
        return Rect::new(at.x, at.y, r.w * zoom, r.h * zoom);
    }
    let base = cam.to_screen(footprint_base(cam.grid, l));
    let (w, h) = (l.size.0 as f32 * cam.scale, (l.size.1 as f32 * cam.cell_size().y).max(cam.scale * 0.8));
    Rect::new(base.x - w / 2.0, base.y - h - h * 0.5, w, h * 1.5)
}

/// The drawn rectangle of an army's figure (its ship at sea), in the 2D space of `cam`.
fn army_picture(game: &Game, a: &Army, art: Option<&DtArt>, cam: &Camera) -> Rect {
    let pos = game.army_display_pos(a);
    let figure = if a.sails() { Some((ship_stem(a.ship), Stand::Afloat)) } else { FIGURES.get(a.figure as usize).map(|stem| (*stem, Stand::Feet)) };
    figure.and_then(|(stem, stand)| figure_rect(art, stem, pos, cam, stand)).unwrap_or_else(|| {
        let c = cam.to_screen(pos);
        Rect::new(c.x - 13.0, c.y - 21.0, 26.0, 26.0)
    })
}

/// The drawn rectangle of the hero's figure (his ship aboard), in the 2D space of `cam`.
fn hero_picture(game: &Game, art: Option<&DtArt>, cam: &Camera) -> Rect {
    let pos = game.display_pos();
    let figure = if game.aboard() {
        figure_rect(art, ship_stem(razdor::rules::ships::kind::HERO), pos, cam, Stand::Afloat)
    } else {
        figure_rect(art, figure_stem(game.hero_figure()), pos, cam, Stand::Feet)
    };
    figure.unwrap_or_else(|| {
        let c = cam.to_screen(pos);
        Rect::new(c.x - 15.0, c.y - 25.0, 30.0, 30.0)
    })
}

/// The hero as drawn on the window this frame, for the first-person view's near fade ([`covering_gap`]). None
/// when he is not drawn (the eyes view, inside a building, no tilt) or his picture has a corner behind the eye.
fn hero_on_screen(game: &Game, art: Option<&DtArt>, cam: &Camera) -> Option<HeroOnWindow> {
    let tilt = cam.tilt?;
    if eyes_cam(cam) || game.hero_hidden() {
        return None;
    }
    let f = cam.to_screen(game.display_pos());
    let (model, _) = standing_model(cam, &tilt, f, f, PASS.with(std::cell::Cell::get) == Pass::Sprites);
    let rect = picture_on_screen(&tilt, screen_view(cam), model, hero_picture(game, art, cam))?;
    let depth = ((f + cam.origin) / cam.scale).dot(view25::forward(tilt.cam.yaw_deg));
    Some(HeroOnWindow { rect, depth })
}

/// The drawn rectangle of a spell's layer `n` (the union of its quads), in the 2D space of `cam`; empty when
/// it has none.
fn spell_picture(show: &SpellShow, n: u8, cam: &Camera) -> Rect {
    let c = cam.to_screen(show.at);
    let zoom = cam.scale / PX;
    show.layers
        .iter()
        .filter(|l| l.layer == n)
        .map(|l| {
            let q = l.quad();
            Rect::new(c.x + q.x * zoom, c.y + q.y * zoom, q.w * zoom, q.h * zoom)
        })
        .reduce(|a, b| {
            let (x, y) = (a.x.min(b.x), a.y.min(b.y));
            Rect::new(x, y, (a.x + a.w).max(b.x + b.w) - x, (a.y + a.h).max(b.y + b.h) - y)
        })
        .unwrap_or(Rect::new(c.x, c.y, 0.0, 0.0))
}

/// A massif object's layer in a cell's turn of the pass (+0, classes 1–8), a plant's (+2,
/// classes 9–12), the building's (+0xC) last (0x4c8864, 0x4c9b5b). Then a spell's layer 2,
/// the cell's figures (armies, the ship, the hero: the drawn-occupant byte, 0x4c9dca), and a
/// spell's layer 3 after them (0x4ca108).
const LAYER_BUILDING: i64 = 2;
const LAYER_SPELL_BELOW: i64 = 3;
const LAYER_FIGURE: i64 = 4;
const LAYER_SPELL_ABOVE: i64 = 5;
/// A bank of mist (drawn before the buildings of its cell, after the cell's ground marks).
const LAYER_BANK: i64 = 1;

fn object_layer(class: u8) -> i64 {
    i64::from(class >= object_class::TREES)
}

/// Where a cell's drawing comes in the map's one pass over the cells: row by row, left to
/// right, then by layer within the cell (0x4c8864). A figure is drawn in its cell's turn, so
/// a building or a tree in a later cell covers it (its tall picture reaches up into the
/// cells north of it), while a figure in a later cell stands in front.
fn cell_pass_key((x, y): Tile, layer: i64) -> i64 {
    ((i64::from(y) + 64) * 65536 + (i64::from(x) + 64)) * LAYERS + layer
}

/// The layers of a cell's turn in the pass (below 8: [`cell_pass_key`] and [`sprite_key`]).
const LAYERS: i64 = 8;

/// Whether the 2.5D view is turned (its shown yaw beyond 0.01°): then the standing sprites are
/// sorted by depth and every side takes the larger margin ([`draw_world_in`]).
fn is_turned(cam: &Camera) -> bool {
    cam.tilt.is_some_and(|t| t.cam.yaw_deg.abs() > 0.01)
}

/// The sort key of a standing sprite of cell `tile` whose foot is at `foot` (in the 2D space of
/// `cam`). Flat, or with the view not turned, its cell's pass key ([`cell_pass_key`]); turned,
/// back to front by the foot's depth along the view's forward (farther first, in 1/64 of a cell),
/// the layer breaking ties.
fn sprite_key(cam: &Camera, tile: Tile, foot: Vec2, layer: i64) -> i64 {
    match cam.tilt {
        Some(t) if is_turned(cam) => {
            let world = (foot + cam.origin) / cam.scale;
            let depth = world.dot(view25::forward(t.cam.yaw_deg));
            -((depth * 64.0).floor() as i64) * LAYERS + layer
        }
        _ => cell_pass_key(tile, layer),
    }
}

/// The first cell of a building's footprint in the pass's order: the top row's left cell
/// ([`razdor::rules::world::Location::cells`]: `size` cells up and left of the anchor, one
/// row more when wider than tall).
fn first_footprint_cell(l: &razdor::rules::world::Location) -> Tile {
    l.cells().min_by_key(|&(x, y)| (y, x)).unwrap_or(l.anchor)
}

/// The compass button's side: 48 dp on a phone (a finger's target, at least 44), 40 px at the
/// interface scale `k` on the desktop.
fn compass_size(k: f32) -> f32 {
    match super::display::dp_px() {
        Some(dp) => 48.0 * dp,
        None => 40.0 * k,
    }
}

/// Where the compass button stands: beside the bottom bar (right, then left), above it at its
/// right or left end, or in the screen's top right corner. The first of these that is on the
/// screen and clear of every rectangle in `avoid` (the minimap's window and its army list) is its
/// place; `None` when none is clear. The bar's buttons are never covered: all of these lie
/// outside the bar.
fn compass_place(screen: Vec2, bar: Rect, size: f32, avoid: &[Rect]) -> Option<Rect> {
    let gap = size * 0.2;
    let beside_y = bar.y + (bar.h - size) / 2.0;
    let above_y = bar.y - gap - size;
    let candidates = [
        Rect::new(bar.x + bar.w + gap, beside_y, size, size),
        Rect::new(bar.x - gap - size, beside_y, size, size),
        Rect::new(bar.x + bar.w - size, above_y, size, size),
        Rect::new(bar.x, above_y, size, size),
        Rect::new(screen.x - gap - size, gap, size, size),
    ];
    candidates.into_iter().find(|r| r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= screen.x && r.y + r.h <= screen.y && !avoid.iter().any(|a| a.overlaps(r)))
}

/// The compass button's rectangle this frame, or None when it does not show ([`compass_shows`]).
fn compass_rect(game: &Game, view: &MapView, cam: &Camera) -> Option<Rect> {
    if !compass_shows(cam.tilt.is_some(), tilt_yaw(), tilt_yaw_target()) {
        return None;
    }
    let mut avoid = Vec::new();
    if view.minimap {
        let o = minimap::outer(&game.world.map, cam.view);
        avoid.push(o);
        if game.army_list {
            avoid.extend(minimap::list_area(game, o, screen_height() - bar_h()));
        }
    }
    compass_place(vec2(screen_width(), screen_height()), game_bar::bar_rect(), compass_size(super::chrome::k()), &avoid)
}

/// The compass: a dark disc with a silver ring, its needle red to north (the map's default up,
/// `d` the screen's direction of it) and grey to the south.
fn draw_compass(rect: Rect, d: Vec2) {
    let c = vec2(rect.x + rect.w / 2.0, rect.y + rect.h / 2.0);
    let r = rect.w / 2.0;
    draw_circle(c.x, c.y, r, Color::new(0.05, 0.06, 0.08, 0.85));
    draw_circle_lines(c.x, c.y, r - 1.0, 1.5, super::chrome::SILVER);
    let n = vec2(-d.y, d.x);
    let (tip, tail, half) = (c + d * r * 0.74, c - d * r * 0.74, r * 0.22);
    draw_triangle(tip, c + n * half, c - n * half, Color::new(0.85, 0.22, 0.2, 1.0));
    draw_triangle(tail, c - n * half, c + n * half, Color::new(0.9, 0.9, 0.92, 1.0));
    draw_circle(c.x, c.y, r * 0.1, Color::new(0.9, 0.9, 0.92, 1.0));
}

/// The map's border in the 2.5D view, drawn on the ground before the standing sprites and the fog
/// (the fog darkens it where the ground is unexplored): a wall of black lines along the fog grid's
/// outer edge ([`Camera::fog_corners`]), so the playable map ends where the ground past it begins.
/// The main line sits on the edge; a thinner one runs 0.35 cell outside it.
fn draw_border(game: &Game, cam: &Camera) {
    let (tl, br) = cam.fog_corners(game);
    let (w, h) = (br.x - tl.x, br.y - tl.y);
    // The offsets are in cells: a cell is `cam.scale` wide and `cam.scale * row_height` high in the
    // ground pass's pixels.
    let (dx, dy) = (BORDER_OUTER_AT * cam.scale, BORDER_OUTER_AT * cam.scale * cam.grid.row_height());
    draw_rectangle_lines(tl.x, tl.y, w, h, border_width(BORDER_LINE_CELLS, cam), BORDER_LINE);
    draw_rectangle_lines(tl.x - dx, tl.y - dy, w + 2.0 * dx, h + 2.0 * dy, border_width(BORDER_OUTER_CELLS, cam), BORDER_OUTER);
}

/// The border's main line, in cells wide (the owner's decision, 2026-11-05: a wall, not a hairline).
const BORDER_LINE_CELLS: f32 = 0.15;
/// The outer line's width in cells, and how far outside the edge it runs (in cells).
const BORDER_OUTER_CELLS: f32 = 0.07;
const BORDER_OUTER_AT: f32 = 0.35;
/// The border's colours: opaque black on the edge, a fainter one outside it.
const BORDER_LINE: Color = Color::new(0.0, 0.0, 0.0, 0.95);
const BORDER_OUTER: Color = Color::new(0.0, 0.0, 0.0, 0.8);

/// A border line `cells` wide at this zoom, at least 1.5 px so it stays visible when zoomed out.
fn border_width(cells: f32, cam: &Camera) -> f32 {
    (cells * cam.scale).max(1.5)
}

/// The world map: the flat view as it always was, or the 2.5D view (`tilt_view`), where the
/// same passes are laid on the ground and stand up as billboards.
///
/// `waits`: the map frame itself, whose passes the frame probe's GPU waits follow (Android,
/// probe on); a backdrop under a window or a battle does not wait.
/// The fog layers of a frame: the war fog, and the event reveals (`shows`, in the order drawn)
/// at `now`. The flat view draws them after the world; the 2.5D view on the ground under the
/// standing sprites ([`draw_world_in`]).
#[derive(Clone, Copy)]
struct Veil<'a> {
    shows: &'a [&'a Showing],
    now: f64,
}

/// No fog layers: a backdrop under a window (the fog is drawn by [`draw_world`] in the 2.5D
/// view only), or any frame with none.
const NO_VEIL: Veil<'static> = Veil { shows: &[], now: 0.0 };

/// The fog shade of the standing sprites in the 2.5D view: the war fog's darkness on a cell (its
/// texture's value there, the unshown cells dark as the texture has them) and the reveals' over
/// it, the darker of the two.
struct Shade<'a> {
    fog: &'a razdor::rules::fog::Fog,
    veil: Veil<'a>,
}

impl Shade<'_> {
    /// The shade of a sprite whose foot is at `foot` (a point of the 2D space of `cam`), on the
    /// cell under it. The fog's darkness there is the one the ground shows this frame
    /// (`minimap::shown_darkness`, drawn before the sprites), composed with the reveal's mask
    /// on that cell (`Showing::alpha_at`, updated this frame too): the same darkness the ground
    /// has, so a sprite opens as the circle passes it.
    fn at(&self, cam: &Camera, foot: Vec2) -> f32 {
        let world = (foot + cam.origin) / cam.scale;
        let t = cam.grid.tile_at((world.x, world.y));
        let fog = self.fog;
        // A disabled fog draws nothing (`minimap::draw_fog`), so it darkens nothing; nor does the
        // trailer's no-fog switch.
        if !fog.enabled || fog.w <= 0 || fog.h <= 0 || trailer_nofog() {
            return 1.0;
        }
        // The cell whose fog the foot stands on. Past the map's edges the ground is black (the
        // fog's rectangles), unless the map goes on past them («Продолжать карту за краем»): then
        // the edge cell's fog, as the texture clamps there.
        let inside = t.0 >= 0 && t.1 >= 0 && t.0 < fog.w && t.1 < fog.h;
        let on = if inside {
            Some(t)
        } else if extend_edge() {
            Some((t.0.clamp(0, fog.w - 1), t.1.clamp(0, fog.h - 1)))
        } else {
            None
        };
        let dark = match on {
            Some(c) => {
                let fog_dark = minimap::shown_darkness(fog, c);
                let reveal = self.veil.shows.iter().filter_map(|s| s.alpha_at(fog.w, c, self.veil.now)).fold(0.0, f32::max);
                // The two layers composed, as the GPU blends them: 1 − (1 − fog)(1 − reveal).
                1.0 - (1.0 - fog_dark) * (1.0 - reveal)
            }
            None => 1.0,
        };
        sprite_visibility(dark)
    }
}

/// The world map: the flat view as it always was, or the 2.5D view (`tilt_view`), where the
/// same passes are laid on the ground and stand up as billboards.
///
/// `waits`: the map frame itself, whose passes the frame probe's GPU waits follow (Android,
/// probe on); a backdrop under a window or a battle does not wait. `veil`: the fog layers, drawn
/// here only in the 2.5D view (the flat caller draws them after this).
#[allow(clippy::too_many_arguments)]
/// The sky behind a tilted view's ground. The flat dark [`SKY`], or, in the first-person view
/// with a horizon («Горизонт»), a gradient from light blue at the top of the frame to pale at the
/// horizon's line, which the ground covers below it.
fn draw_sky(cam: &Camera) {
    let Some(tilt) = cam.tilt else { return };
    let r = cam.draw;
    let horizon = tilt.cam.bend.filter(|b| b.limit.is_finite()).and_then(|b| {
        let g = b.pivot + view25::forward(b.yaw_deg) * b.limit;
        tilt.cam.project(b.point(g), cam.view)
    });
    let Some(h) = horizon else {
        // A frame's whole target: the clear is the same picture, and cheaper. Every caller of
        // `draw_world` clears the target just before it, with nothing drawn in between (`world_frame`
        // and `photo_frame` clear, then the camera and the input only; `draw_map_backdrop` and
        // `window_backdrop_at_scale` clear inside `unpanned`, then the camera). `clear_background`
        // drops the draws queued so far, so the clear also leaves nothing of them behind.
        if r.x <= 0.0 && r.y <= 0.0 && r.x + r.w >= screen_width() && r.y + r.h >= screen_height() {
            return clear_background(SKY);
        }
        return draw_rectangle(r.x, r.y, r.w, r.h, SKY);
    };
    let (y, bottom) = (h.y.clamp(r.y, r.y + r.h), r.y + r.h);
    let v = |x: f32, y: f32, c: Color| macroquad::models::Vertex::new(x, y, 0.0, 0.0, 0.0, c);
    let vertices = vec![
        v(r.x, r.y, SKY_TOP),
        v(r.x + r.w, r.y, SKY_TOP),
        v(r.x, y, SKY_HORIZON),
        v(r.x + r.w, y, SKY_HORIZON),
        v(r.x, bottom, SKY_HORIZON),
        v(r.x + r.w, bottom, SKY_HORIZON),
    ];
    let indices = vec![0, 1, 2, 1, 3, 2, 2, 3, 4, 3, 5, 4];
    macroquad::models::draw_mesh(&macroquad::models::Mesh { vertices, indices, texture: None });
}

#[allow(clippy::too_many_arguments)]
fn draw_world(game: &Game, assets: &Assets, cam: &Camera, preview: Option<&[Tile]>, spell: Option<&SpellShow>, hover: Option<MapHoverLook>, waits: bool, veil: Veil, relief: Option<daylight::Relief>) {
    // The depth model's workers (2.5D, buildings with no cut): they run beside the frame.
    if let Some(art) = assets.dt.as_ref() {
        art.depth_tick(cam.tilt.is_some());
    }
    draw_sky(cam);
    if waits {
        gpu_wait(Stage::GpuSky);
    }
    stamp_idle(game, clock_ms());
    // A bent view (the trailer's, the first-person one) draws its ground and the ground's overlays
    // into one picture, laid on one sheet, and the sprites stand on it: one `in_view` for both.
    if cam.tilt.is_some_and(|t| t.cam.bend.is_some()) {
        let art = assets.dt.as_ref();
        return in_view(cam, |c| {
            ground_in_bent(game, art, c, relief);
            draw_world_in(game, assets, c, preview, spell, hover, waits, veil, relief);
        });
    }
    // The ground first: it sets its own camera (`in_view`), so the cache can fill through it.
    frame_probe::time(Stage::Terrain, || draw_ground(game, assets.dt.as_ref(), cam, waits, relief));
    if waits {
        gpu_wait(Stage::GpuTerrain);
    }
    in_view(cam, |c| draw_world_in(game, assets, c, preview, spell, hover, waits, veil, relief));
}

/// The ground of a bent view ([`draw_world`]), drawn in its picture's 2D space `c` (the ground's
/// pass of [`bent_view`]; the sprites' pass draws none of it): the terrain, as [`draw_ground`] draws
/// it without the cache, which a bent view does not use.
fn ground_in_bent(game: &Game, art: Option<&DtArt>, c: &Camera, relief: Option<daylight::Relief>) {
    // «Живая вода»: the rivers' flow of the map, while the water is alive (else still).
    let flow = map_flow(game);
    let flow: &[(f32, f32)] = flow.as_deref().map_or(&[], |f| f.as_slice());
    match art.and_then(|a| a.terrain_layer()).filter(|_| c.grid == Grid::Square8) {
        Some(layer) => ground(c, || terrain_quad(game, layer, c, terrain::Mode::Full, flow, relief)),
        None => ground(c, || draw_terrain(game, art, c)),
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_world_in(game: &Game, assets: &Assets, cam: &Camera, preview: Option<&[Tile]>, spell: Option<&SpellShow>, hover: Option<MapHoverLook>, waits: bool, veil: Veil, relief: Option<daylight::Relief>) {
    let art = assets.dt.as_ref();
    let map = &game.world.map;
    let ((c0, c1), (r0, r1)) = cam.visible(map);
    // Sprites stand on their cell and reach up to ~8 cells above it. Turned, a sprite reaches any
    // way up the screen, so every side takes the larger margin.
    let (below, side) = (10, 8);
    let (up, side) = if is_turned(cam) { (below, below) } else { (1, side) };
    // The list is the thread's buffer taken for this frame (its capacity is kept).
    let mut items = WORLD_ITEMS.with(|c| std::mem::take(&mut *c.borrow_mut()));
    items.clear();
    let fog = &game.fog;
    let tilted = cam.tilt.is_some();
    // The 2.5D view's fog shade of each standing sprite (none in the flat view: 1 there).
    let shade = tilted.then_some(Shade { fog, veil });
    let shade_of = |foot: Vec2| shade.as_ref().map_or(1.0, |s| s.at(cam, foot));
    // The trailer's near fade stops at the hero's distance, and the first-person view's covering sprites
    // fade in front of him (see `NEAR_LIMIT`): set before any sprite of the pass, the hills included.
    // Reset first, so that no limit from an earlier pass that stopped early can reach this one.
    NEAR_LIMIT.with(|l| l.set(f32::INFINITY));
    if trailer().is_some() || first_person() {
        if let Some(d) = foot_distance(cam, cam.to_screen(game.display_pos())) {
            NEAR_LIMIT.with(|l| l.set(d));
        }
    }
    // The hero as drawn, before any sprite of the pass (the hills' too): the covering test needs him.
    HERO_ON_WINDOW.with(|h| h.set(if first_person() && bent_first_person_cam(cam) { hero_on_screen(game, art, cam) } else { None }));
    // An object of the drawn columns and rows with a cell explored near the feather is seen, and
    // the hills are the classes 1-3 (`hill`).
    let seen = |o: &Decoration| o.tile.0 >= c0 - side && o.tile.0 < c1 + side && razdor::rules::map::object_cells(o).any(|t| fog.explored_near(t, minimap::FEATHER));
    let hill = |o: &Decoration| (object_class::HILLS..=object_class::ROCKY_HILLS).contains(&o.class);
    // The original (0x4c8864) draws the hills of classes 1-3 in a pass of their own before
    // anything else, so they lie under every tree, mountain, building and army, the ones
    // above them included. Its test is `0x100 < class·256 + sprite < 0x401`: class 4's
    // sprites are 10 and up, so the yellow hills are drawn with the mountains, row by row.
    // 2.5D: the hills are drawn after the fog, below (they stand in the fog's shade).
    for o in map.objects_in_rows(r0 - up, r1 + below) {
        if seen(o) {
            if hill(o) {
                if !tilted {
                    standing(cam, object_foot(o, cam), object_foot(o, cam), shade_of(object_foot(o, cam)), object_picture(o, art, cam), || draw_object(o, art, cam));
                }
            } else {
                items.push((sprite_key(cam, o.tile, object_foot(o, cam), object_layer(o.class)), Drawable::Object(*o)));
            }
        }
    }
    // A spell's layer 1 lies on the ground over the hills, under the marks and every
    // standing sprite (0x4c935b: the cell loop before the marks' 0x4c9459).
    if let Some(s) = spell {
        ground(cam, || draw_spell_layer(s, 1, cam));
    }
    // The marks under the hero and every army (cell byte +0xA, 0x48eeb0: the hero's kind 1,
    // an army's 3) lie on the ground in the same pass, under every tree, building and
    // figure (0x4c9459), so the figures' shadows fall over them. The hover's ring takes the
    // place of the mark of the cell it is on (one mark byte a cell, 0x48eeb0).
    let ring = hover.and_then(|h| Some((h.cell, h.ring?)));
    ground(cam, || {
        for a in game.world.armies.iter().filter(|a| fog.explored(a.tile(map)) && !a.sails()) {
            if ring.is_none_or(|(t, _)| a.tile(map) != t) {
                draw_mark(cam, game.army_display_pos(a), "Selection-1", MARK_ARMY);
            }
        }
        if let Some((t, (texture, color))) = ring {
            draw_mark(cam, map.center(t), texture, color);
        }
        if !game.aboard() && !eyes_cam(cam) {
            draw_mark(cam, game.display_pos(), "Selection-1", MARK_HERO);
        }
        // The route being walked, or the one a first click shows, lies on the ground under the
        // figures (the original's second pass, over the hills). The eyes view drops it: its route
        // starts under the camera, and its marks would fill the frame.
        if !eyes_cam(cam) {
            if game.moving() {
                draw_route(game, &game.path, cam);
            } else if let Some(path) = preview {
                draw_route(game, path, cam);
            }
        }
    });
    // «Живая вода» (`water`): the wakes lie on the ground behind the sailing figures, over the
    // marks and the route and under the border and every standing sprite.
    if water::live() {
        ground(cam, || draw_wakes(game, cam));
    }
    // 2.5D: the border lies on the ground, over the ground's marks and route and under every
    // standing sprite; the fog darkens it where the ground is unexplored.
    if tilted && !trailer_clean() {
        ground(cam, || draw_border(game, cam));
    }
    // A location is drawn when a cell of its footprint is explored near the feather and it lies in
    // the drawn rows and columns.
    let location_seen = |l: &Location| {
        let (ax, ay) = l.anchor;
        l.cells().any(|t| fog.explored_near(t, minimap::FEATHER)) && ay >= r0 - up && ay < r1 + below && ax >= c0 - side && ax - l.size.0 < c1 + side
    };
    // 2.5D: a card building's shadow lies on the ground too, under the fog below.
    let shadow_k = relief.map_or(1.0, |r| r.shadow_k);
    if tilted {
        ground(cam, || {
            for l in game.world.locations.iter().filter(|l| location_seen(l)) {
                if let Some((atlas, r, _)) = card_building(l, art) {
                    draw_building_shadow(l, atlas, r, cam, shadow_k);
                }
            }
        });
    }
    if waits {
        gpu_wait(Stage::GpuMarks);
    }
    // 2.5D: the war fog and the reveals lie on the ground now, over the border and under every
    // standing sprite (the flat view draws them after the world): a building behind the fog
    // keeps its upper part, and a sprite on a dark cell is darkened by it (`shade_of`). The
    // hills follow, standing in that shade.
    if tilted {
        frame_probe::time(Stage::Fog, || {
            ground(cam, || cam.draw_fog_flat(game, veil.shows));
            // The raised sheets of the mist lie over the ground's mist and under the reveals, as the mist does.
            cam.draw_raised_mist(game, veil.shows);
            ground(cam, || cam.draw_showing_flat(game, veil.shows.to_vec(), veil.now));
        });
        if waits {
            gpu_wait(Stage::GpuFog);
        }
        // One sharp session for the hills, as for the pass below: outside one, every sharp draw
        // switches the material on and off, so each hill was its own draw call.
        super::sharp::session(|| {
            for o in map.objects_in_rows(r0 - up, r1 + below) {
                if seen(o) && hill(o) {
                    standing(cam, object_foot(o, cam), object_foot(o, cam), shade_of(object_foot(o, cam)), object_picture(o, art, cam), || draw_object(o, art, cam));
                }
            }
        });
    }
    // The standing scenery and the buildings share one pass over the cells, row by row and
    // left to right (0x4c8864's cell loop): each cell draws its massif object (+0), then its
    // plant (+2), then the building whose footprint covers it (+0xC) if that building is not
    // drawn yet this frame (0x4c9b5b, its once-a-frame flag at record −0x43). So a building
    // is drawn at the first cell of its footprint in that order, its top row's left cell, and
    // every object after it in the scan (south of that row, or east in the same row) covers
    // it. Bridges stay on the ground, under everything (Razdor's).
    for (i, l) in game.world.locations.iter().enumerate() {
        if location_seen(l) {
            if l.kind.is_bridge() {
                items.push((i64::MIN / 2 + cell_pass_key(l.anchor, 0), Drawable::Building(i)));
            } else if let Some((_, r, cards)) = tilted.then(|| card_building(l, art)).flatten() {
                // 2.5D with cut data: each card goes in the pass on its own foot, back to front.
                let origin = building_sprite_origin(l, r.w, r.h, cam);
                let zoom = cam.scale / PX;
                for (k, card) in cards.iter().enumerate() {
                    items.push((sprite_key(cam, first_footprint_cell(l), card_foot(origin, zoom, card), LAYER_BUILDING), Drawable::BuildingCard(i, k)));
                }
            } else {
                items.push((sprite_key(cam, first_footprint_cell(l), building_foot(l, cam), LAYER_BUILDING), Drawable::Building(i)));
            }
        }
    }
    // 2.5D and first-person with the mist: the banks stand along the frontier of the unexplored land, sorted
    // with the sprites (the figures and buildings in front of them draw over them).
    if tilted && mist::on() && !trailer_nofog() && fog.enabled {
        push_banks(cam, game, (c0, c1), (r0, r1), &mut items);
    }
    // The figures (armies, the waiting ship, the hero) are drawn in their cell's turn of the
    // pass, after the cell's scenery and buildings (0x4c9b5b, 0x4c9dca). The original keys a
    // figure by its drawn cell (cell +9), so a figure north of a building is covered by it
    // (the castle's walls hide a hero behind them, a player's report).
    // Armies in the dark keep moving but are not shown.
    for (i, a) in game.world.armies.iter().enumerate().filter(|(_, a)| fog.explored(a.tile(map))) {
        items.push((sprite_key(cam, a.tile(map), cam.to_screen(game.army_display_pos(a)), LAYER_FIGURE), Drawable::Army(i)));
    }
    if let Some(ship) = game.ship.filter(|s| !s.aboard && fog.explored(s.tile)) {
        items.push((sprite_key(cam, ship.tile, cam.to_screen(map.center(ship.tile)), LAYER_FIGURE), Drawable::Ship));
    }
    items.push((sprite_key(cam, game.tile(), cam.to_screen(game.display_pos()), LAYER_FIGURE), Drawable::Hero));
    // A spell's layer 2 just behind its target's figure, layer 3 just in front (0x4c9b5b
    // draws them in the cell's pass before its armies, 0x4c9dca, and after, 0x4ca108).
    if let Some(s) = spell {
        items.push((sprite_key(cam, s.cell, cam.to_screen(s.at), LAYER_SPELL_BELOW), Drawable::Spell(2)));
        items.push((sprite_key(cam, s.cell, cam.to_screen(s.at), LAYER_SPELL_ABOVE), Drawable::Spell(3)));
    }
    // Stable: figures sharing a cell keep the order they were pushed in.
    items.sort_by_key(|a| a.0);
    // One sharp session for the pass: its art shares the material switch (see `sharp::session`).
    super::sharp::session_lit(relief, || {
        for d in items.iter().map(|(_, d)| d) {
            match d {
                Drawable::Object(o) => standing(cam, object_foot(o, cam), object_foot(o, cam), shade_of(object_foot(o, cam)), object_picture(o, art, cam), || draw_object(o, art, cam)),
                Drawable::Building(i) => {
                    let l = &game.world.locations[*i];
                    let outlined = hover.is_some_and(|h| h.building == Some(*i));
                    if l.kind.is_bridge() {
                        // A bridge lies on the ground (Razdor's), so it is drawn in the ground pass.
                        ground(cam, || draw_building(l, art, cam, outlined));
                    } else {
                        standing(cam, building_foot(l, cam), building_foot(l, cam), shade_of(building_foot(l, cam)), building_picture(l, art, cam), || draw_building(l, art, cam, outlined));
                    }
                }
                Drawable::BuildingCard(i, k) => {
                    let l = &game.world.locations[*i];
                    let Some((atlas, r, cards)) = card_building(l, art) else { continue };
                    let card = &cards[*k];
                    let zoom = cam.scale / PX;
                    let origin = building_sprite_origin(l, r.w, r.h, cam);
                    let f = card_foot(origin, zoom, card);
                    let outlined = hover.is_some_and(|h| h.building == Some(*i));
                    standing(cam, f, f, shade_of(f), Rect::new(origin.x, origin.y, r.w * zoom, r.h * zoom), || {
                        if *k == 0 {
                            frame_probe::sprite(r.w * zoom, r.h * zoom);
                        }
                        draw_card(card, origin, zoom, r, atlas, outlined);
                    });
                }
                Drawable::Bank { foot, darkness, seed } => standing(cam, *foot, *foot, 1.0, || draw_bank(cam, *foot, *darkness, *seed)),
                Drawable::Army(i) => {
                    let a = &game.world.armies[*i];
                    let f = cam.to_screen(game.army_display_pos(a));
                    standing(cam, f, f, shade_of(f), army_picture(game, a, art, cam), || draw_army(game, a, assets, art, cam));
                }
                Drawable::Ship => {
                    if let Some(ship) = game.ship {
                        let at = map.center(ship.tile);
                        let f = cam.to_screen(at);
                        let picture = figure_rect(art, ship_stem(razdor::rules::ships::kind::HERO), at, cam, Stand::Afloat).unwrap_or(Rect::new(f.x - 20.0, f.y - 20.0, 40.0, 40.0));
                        standing(cam, f, f, shade_of(f), picture, || {
                            if !draw_figure(art, ship_stem(razdor::rules::ships::kind::HERO), at, None, 0, cam, Stand::Afloat) {
                                draw_ship(cam, at, HERO_SAIL);
                            }
                        });
                    }
                }
                Drawable::Hero => {
                    let f = cam.to_screen(game.display_pos());
                    without_near_fade(|| standing(cam, f, f, shade_of(f), hero_picture(game, art, cam), || draw_hero(game, assets, art, cam)))
                }
                Drawable::Spell(n) => {
                    if let Some(s) = spell {
                        let f = cam.to_screen(s.at);
                        standing(cam, f, f, shade_of(f), spell_picture(s, *n, cam), || draw_spell_layer(s, *n, cam));
                    }
                }
            }
        }
    });
    NEAR_LIMIT.with(|l| l.set(f32::INFINITY));
    HERO_ON_WINDOW.with(|h| h.set(None));
    if waits {
        gpu_wait(Stage::GpuSprites);
    }
    items.clear();
    WORLD_ITEMS.with(|c| *c.borrow_mut() = items);
}

/// The route being walked, as the original draws it (0x49e520): a white arrow on every cell
/// ahead but the last (`Windows/Way_Arrows.ugs`, 32×22, one frame per direction in the exe's
/// order: up-left, up, up-right, right, down-right, down, down-left, left). Inside a building
/// only the cell the route leaves it from has one. The last cell has a green ring instead
/// (0x48eeb0 with kind 2), unless an army stands there to be met: its own ring stays. The
/// arrows' brightness runs along the route ([`route_arrow_light`]). The time left is in the
/// bar.
fn draw_route(game: &Game, path: &[Tile], cam: &Camera) {
    let w = &game.world;
    let building = |t: Tile| w.location_covering(t).is_some();
    if let Some(&end) = path.last() {
        let target = !building(end) && w.armies.iter().any(|a| a.tile(&w.map) == end);
        if !building(end) && !target {
            draw_mark(cam, w.map.center(end), "Selection-0", MARK_ROUTE_END);
        }
    }
    let arrows = super::chrome::animation("Windows/Way_Arrows.ugs").filter(|a| a.len() == 8);
    let zoom = cam.scale / PX;
    let ms = crate::ui::clock::now() * 1000.0;
    let mut from = game.tile();
    for (i, &t) in path.iter().enumerate() {
        let (dx, dy) = ((t.0 - from.0).signum(), (t.1 - from.1).signum());
        from = t;
        if !route_arrow(i, path.len(), building(t), path.get(i + 1).is_some_and(|&n| building(n))) {
            continue;
        }
        let c = cam.cell_centre(t);
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
        // The original numbers its phases from the route's start; counted from the end here,
        // so they stay put while the hero walks it down (only the wave's start time differs).
        let v = route_arrow_light(ms, (path.len() - 1 - i) % 10);
        match &arrows {
            Some(a) => {
                let (w, h) = (a[dir].width() * zoom, a[dir].height() * zoom);
                super::sharp::draw(&a[dir], c.x - w / 2.0, c.y - h / 2.0, Color::new(v, v, v, 1.0), DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
            }
            None => {
                draw_circle(c.x, c.y, 3.5, Color::new(0.0, 0.0, 0.0, 0.5));
                draw_circle(c.x, c.y, 2.5, Color::new(v, v, v, 1.0));
            }
        }
    }
}

/// Whether the route's step `i` of `len` has an arrow (0x49e520): not the last one, and in a
/// building only the one the next step leaves it from.
fn route_arrow(i: usize, len: usize, in_building: bool, next_in_building: bool) -> bool {
    i + 1 < len && (!in_building || !next_in_building)
}

/// A route arrow's brightness at `ms` for its phase 0..9 (map renderer, 0x4c612c chunk):
/// `x = (ms / 10 + 18 · phase) mod 181`, folded at 90, plus 150, as a grey of 255.
fn route_arrow_light(ms: f64, phase: usize) -> f32 {
    let x = ((ms / 10.0) as i64 + 18 * phase as i64).rem_euclid(181);
    let x = if x > 90 { 180 - x } else { x };
    (x + 150) as f32 / 255.0
}

/// The tooltips' cells (0x4ca9f0, 0x4cb18c): 48 px squares 56 px apart, from 10 px in; a
/// portrait 46 px at (1, 1) inside, framed by a 1 px line.
const TIP_CELL: f32 = 48.0;
const TIP_PITCH: f32 = 56.0;

/// The height of the tooltips' cells, `lines` rows of [`TIP_PITCH`] less the last gap.
fn tip_grid_h(lines: usize) -> f32 {
    lines as f32 * TIP_PITCH - (TIP_PITCH - TIP_CELL)
}

/// The width of the tooltips' cells, `cols` columns less the last gap.
fn tip_grid_w(cols: usize) -> f32 {
    cols as f32 * TIP_PITCH - (TIP_PITCH - TIP_CELL)
}

/// The scale (screen px per original px) of an army tooltip's cells. Off a phone, and for a
/// building's garrison, the interface scale `k`, as before. On a phone an army's cells take the
/// text's scale `tk` (Razdor's, a player's report, backlog 1554: the text grew with the phone's
/// dp and the pictures stayed at the window's scale, so they looked tiny beside it), capped at
/// what the box holds: `room` px of height under the text (`grid_h` and `grid_w` in the cells'
/// own units) and the box's width `box_w` px. Never under `k`.
fn army_cells_scale(phone_army: bool, k: f32, tk: f32, grid_h: f32, grid_w: f32, room: f32, box_w: f32) -> f32 {
    if !phone_army {
        return k;
    }
    tk.min(room / grid_h).min(box_w / grid_w).max(k)
}

/// A 2×6 (or 3×4) mini formation of portraits, the front row at the bottom as the enemy's
/// in battle; empty cells show their row's icon. Its cells are `cs` px per original px. Returns
/// its height.
fn formation_grid(game: &Game, assets: &Assets, troops: &[Troop], team: Team, x: f32, y: f32, cs: f32) -> f32 {
    let (cell, pitch) = (TIP_CELL * cs, TIP_PITCH * cs);
    let f = game.content.formation;
    let lines = f.display_lines();
    for r in 0..lines {
        for col in 0..f.display_cols() {
            let Some(slot) = f.at_display(r, col) else { continue };
            let (cx, cy) = (x + col as f32 * pitch, y + (lines - 1 - r) as f32 * pitch);
            let sq = Rect::new(cx, cy, cell, cell);
            let troop = troops.iter().find(|t| f.display(t.slot) == (r, col));
            match troop {
                None => super::chrome::cell_icon(super::chrome::CellIcon::of(f, slot), sq),
                Some(t) => {
                    let face = Rect::new(cx + cs, cy + cs, cell - 2.0 * cs, cell - 2.0 * cs);
                    assets.draw_portrait(t.unit, team, face);
                    // Wounds left by world spells.
                    let max = razdor::rules::units::Stats::of_level(&game.content, t.unit, t.level).max_hp();
                    super::chrome::wounds(face, max - t.hurt, max);
                }
            }
            // Portrait, wounds and frame only: the original writes no level (0x4ca9f0).
            draw_rectangle_lines(cx, cy, cell, cell, 1.0, super::chrome::SILVER);
        }
    }
    tip_grid_h(lines) * cs
}

/// What a tooltip line is drawn in (0x4ca9f0, 0x4cb18c): the style's label font (SanSerif),
/// its name font (Benguiat), the orange SanSerif of an army's description, the orange
/// Benguiat of "(дань уже собрана)", or Razdor's blue magic lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ink {
    Label,
    Name,
    Orange,
    Note,
    Mana,
}

impl Ink {
    /// Names and the note are in Benguiat, larger; the rest in SanSerif.
    fn big(self) -> bool {
        matches!(self, Ink::Name | Ink::Note)
    }
}

struct Tooltip {
    title: String,
    /// The text block, centred: each entry a line or a paragraph to wrap, "" an empty line.
    text: Vec<(String, Ink)>,
    troops: Vec<Troop>,
    /// A building's garrison caption, over its cells at the bottom.
    garrison: Option<String>,
    /// An army's: its cells right under the title, the text after them.
    army: bool,
    team: Team,
    style: TipStyle,
}

/// A text of the install (`[Info] <key>`) in Russian, else ours.
fn info(key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text("Info", key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// A font's tint: white with `minus` (r, g, b) taken off as it is loaded (0x48d0d4 at
/// 0x4e5c9c and 0x4dbc26).
const fn tint(minus: [u8; 3]) -> Color {
    Color::new((255 - minus[0]) as f32 / 255.0, (255 - minus[1]) as f32 / 255.0, (255 - minus[2]) as f32 / 255.0, 1.0)
}

/// The title, in every style: Benguiat less (0, 0, 100) (font 0xae24a0), over its shadow.
const TIP_TITLE: Color = tint([0, 0, 100]);
/// Normal style: names in Benguiat less (100, 0, 100) (0xae249c), labels and a building's
/// description in SanSerif less (100, 0, 100) (0xae24d4): both green.
const TIP_GREEN: Color = tint([100, 0, 100]);
/// SanSerif less (20, 90, 180) (0xae24c4): an army's description in every style, labels and
/// a building's description in the hostile one.
const TIP_ORANGE: Color = tint([20, 90, 180]);
/// Benguiat less (0, 80, 170) (0xae24a4): names in the hostile style, and the village's note.
const TIP_ORANGE_NAME: Color = tint([0, 80, 170]);
/// Neutral style: names in Benguiat less (80, 80, 80) (0xae24b8), labels in SanSerif less
/// (100, 100, 100) (0xae24d0).
const TIP_GREY_NAME: Color = tint([80, 80, 80]);
const TIP_GREY_LABEL: Color = tint([100, 100, 100]);

/// The colour of `ink` in `style` (0x4ca9f0, 0x4cb18c pick the fonts by style).
fn tip_color(style: TipStyle, ink: Ink) -> Color {
    match (style, ink) {
        (_, Ink::Orange) => TIP_ORANGE,
        (_, Ink::Note) => TIP_ORANGE_NAME,
        (_, Ink::Mana) => MANA,
        (TipStyle::Normal, _) => TIP_GREEN,
        (TipStyle::Hostile, Ink::Name) => TIP_ORANGE_NAME,
        (TipStyle::Hostile, _) => TIP_ORANGE,
        (TipStyle::Neutral, Ink::Name) => TIP_GREY_NAME,
        (TipStyle::Neutral, _) => TIP_GREY_LABEL,
    }
}

/// An army's tooltip names its leader only when the army has more than one unit (0x4ca9f0:
/// the record's unit count, +0, above 1).
fn tip_shows_leader(leader: &str, units: usize) -> bool {
    !leader.is_empty() && units > 1
}

/// The original's army tooltip (0x4ca9f0): its name, its 2×6 cards, then "Предводитель" and
/// the leader's name, the description in orange; world spells on it under that, then the wounds
/// (Razdor's line, the original's tooltip has none): all the troops' wounds, battle ones
/// included, so the line is not called magic.
fn army_tooltip(game: &Game, a: &Army) -> Tooltip {
    let title = if a.shown_name().is_empty() { info("NoNameArmy", n_("Unknown army")) } else { a.shown_name().to_string() };
    let mut text = Vec::new();
    if tip_shows_leader(&a.leader_name, a.troops.len()) {
        text.push((info("Commander", n_("Leader")), Ink::Label));
        text.push((a.leader_name.clone(), Ink::Name));
    }
    if !a.description.trim().is_empty() {
        // The original fills #HERONAME in building/army descriptions at map load (0x4b2930, 0x4b2a46).
        text.push((game.fill_text(&a.description), Ink::Orange));
    }
    let now = game.clock.total_minutes() as u64;
    let groups: Vec<_> = a.troops.iter().map(|t| &t.spells[..]).collect();
    let spells: Vec<String> = spell_lines(&groups, now).iter().filter_map(|l| Some(spell_entry(&game.spell(l.spell)?.name, l, now))).collect();
    if !spells.is_empty() {
        // Wrapped as the description: an army of several units can list more names than fit.
        text.push((trf!("Under spells: {spells}", spells = spells.join(", ")), Ink::Mana));
    }
    let hurt: i32 = a.troops.iter().map(|t| t.hurt).sum();
    if hurt > 0 {
        text.push((trf!("Wounded: -{hurt} hits", hurt), Ink::Mana));
    }
    // 0x4ca9f0: the hostile frame and colours for an army whose own attitude to the player
    // (+0x16af, not the relation) is below 1, neutral ones (0) included.
    let style = if a.own_attitude() < 1 { TipStyle::Hostile } else { TipStyle::Normal };
    let team = if a.hostile() { Team::Enemy } else { Team::Player };
    Tooltip { title, text, troops: a.troops.clone(), garrison: None, army: true, team, style }
}

/// The owner line of a building's tooltip. The holder is named only for a castle, fort or
/// ruins (0x4cb18c): the hero, an army's leader or the map's owner. Any other building, a
/// captured village too, always names the map's owner. An army's leader comes from the map's
/// army record (0x4cb2d3), so it is named while the army is off the map or beaten too.
fn tooltip_owner(game: &Game, l: &Location) -> String {
    match (l.kind, l.owner) {
        (LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins, Owner::Player) => game.hero_name.clone().unwrap_or_else(|| tr("you").to_string()),
        (LocationKind::Castle | LocationKind::Fort | LocationKind::Ruins, Owner::Army(k)) => {
            (k as usize).checked_sub(1).and_then(|i| game.world.army_leaders.get(i)).cloned().unwrap_or_default()
        }
        _ => l.owner_name.clone(),
    }
}

/// The original's building tooltip (0x4cb18c): its name cut at `#` (0x4973a0, called at
/// 0x4cb62c), «Неизвестное здание» when nothing is left (0x4cb631); a text block of "Владелец"
/// and the owner's name, the description in the label font, "(дань уже собрана)" for a village
/// already emptied; a garrison's cells at the bottom under "Состав гарнизона защитников:".
/// Ruins never show their cells: their text block ends with an empty line, that caption and
/// "Неизвестная армия" in the name font (only when the block has something else in it).
fn location_tooltip(game: &Game, l: &Location) -> Tooltip {
    let title = if l.shown_name().is_empty() { info("NoNameBuilding", n_("Unknown building")) } else { l.shown_name().to_string() };
    let mut text = Vec::new();
    let owner = tooltip_owner(game, l);
    if !owner.trim().is_empty() {
        text.push((info("Owner", n_("Owner")), Ink::Label));
        text.push((owner, Ink::Name));
    }
    if !l.description.trim().is_empty() {
        // The original fills #HERONAME in building/army descriptions at map load (0x4b2930, 0x4b2a46).
        text.push((game.fill_text(&l.description), Ink::Label));
    }
    if l.kind == LocationKind::Village && l.tribute_gold <= 0 && l.tribute_mana <= 0 {
        text.push((info("VillageEmptyGold", n_("(tribute already collected)")), Ink::Note));
    }
    let mut troops = garrison_shown(game, l);
    let mut garrison = None;
    if !troops.is_empty() {
        let caption = info("Defenders", n_("The garrison's defenders:"));
        if l.kind == LocationKind::Ruins {
            if !text.is_empty() {
                text.push((String::new(), Ink::Label));
                text.push((caption, Ink::Label));
                text.push((info("NoNameArmy", n_("Unknown army")), Ink::Name));
            }
            troops.clear();
        } else {
            garrison = Some(caption);
        }
    }
    let team = if l.owned() { Team::Player } else { Team::Enemy };
    Tooltip { title, text, troops, garrison, army: false, team, style: location_tip_style(l) }
}

/// The garrison of a building's tooltip, whoever holds the building (0x4cb18c): the player's
/// units left there too. A town shows none. A building taken shows only the units the player
/// left there: the garrison beaten for it is gone.
fn garrison_shown(game: &Game, l: &Location) -> Vec<Troop> {
    if l.kind == LocationKind::Town {
        return Vec::new();
    }
    let mut troops: Vec<Troop> = if l.cleared { Vec::new() } else { l.garrison.iter().filter(|t| t.alive()).cloned().collect() };
    for s in l.stationed.iter().filter(|s| s.unit.alive()) {
        let u = &s.unit;
        let mut t = Troop::new(u.def, u.level, u.slot);
        t.hurt = (u.max_hp(&game.content) - u.hp).max(0);
        troops.push(t);
    }
    troops
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

/// The tooltip's width, its text block's width, its title's room and the text block's top
/// (0x4ca8ac: 0x15c wide; 0x47cc90 boxes 0x14c wide at x = 8; 0x48df50 cuts the title to
/// 0x154; a building's block at y = 0x25).
const TIP_W: f32 = 348.0;
const TIP_TEXT_W: f32 = 332.0;
const TIP_TITLE_W: f32 = 340.0;
const TIP_TOP: f32 = 37.0;

/// A tooltip's height in the original's pixels (0x4ca9f0, 0x4cb18c), from its text block's
/// height, its cells' height and the garrison caption's (a building with cells):
/// - an army: its cells from y = 37, its text 8 px under them and 10 px of margin; 2 px of
///   margin without text; the text at most 0x161 high;
/// - a building: its text from y = 37 and 10 px of margin, or the title strip alone (0x1d);
///   with cells, the caption and the cells under the text (+ 0x79 for 2 rows), or under the
///   title (0x9f for 2 rows);
/// - at most 0x200.
fn tip_height(army: bool, text_h: f32, grid_h: f32, caption_h: Option<f32>) -> f32 {
    tip_height_raw(army, text_h, grid_h, caption_h).min(512.0)
}

/// [`tip_height`] before its cap of 0x200.
fn tip_height_raw(army: bool, text_h: f32, grid_h: f32, caption_h: Option<f32>) -> f32 {
    if army {
        let top = TIP_TOP + grid_h + 8.0;
        if text_h > 0.0 { top + text_h.min(353.0) + 10.0 } else { top + 2.0 }
    } else {
        match (text_h > 0.0, caption_h) {
            (true, None) => TIP_TOP + text_h + 10.0,
            (false, None) => 29.0,
            (true, Some(c)) => TIP_TOP + text_h + 10.0 + c + grid_h + 17.0,
            (false, Some(c)) => c + grid_h + 55.0,
        }
    }
}

/// A tooltip row's font size at the text scale `s`: the big Benguiat's 16, else 12.
fn tip_size(ink: Ink, s: f32) -> f32 {
    if ink.big() { 16.0 * s } else { 12.0 * s }
}

/// A tooltip row's advance at the text scale `s`: its font size and 3 more.
fn tip_lh(ink: Ink, s: f32) -> f32 {
    tip_size(ink, s) + 3.0 * s
}

/// The tooltip's box width in px at the text scale `u`: the original's `TIP_W` at `u`, no wider
/// than the screen less 8 px (Razdor's: a phone's text base makes the box grow with its text).
fn tip_box_w(u: f32) -> f32 {
    (TIP_W * u).min(screen_width() - 8.0)
}

fn draw_tooltip(game: &Game, assets: &Assets, t: &Tooltip) {
    // The glyphs take the sizes asked for (`glyph`), not the interface scale's native ones,
    // unless the text is the original's own (`display::native_text`).
    if super::display::native_text() {
        draw_tooltip_at(game, assets, t);
    } else {
        super::dt_font::with_legacy_metrics(|| draw_tooltip_at(game, assets, t));
    }
}

fn draw_tooltip_at(game: &Game, assets: &Assets, t: &Tooltip) {
    use super::chrome::shadow_centered;
    use super::dt_font::{with_face, Face};
    let k = super::chrome::k();
    let face = |ink: Ink| if ink.big() { Face::Subtitle } else { Face::Body };
    // A glyph's size at the text scale `u` in its face: the sheet's size there (the native one
    // at the interface scale).
    let glyph = |f: Face, u: f32| super::dt_font::sheet_size(f, u);
    // Razdor's text size and phone base (the owner's request): the text, the title and the
    // garrison's caption share one text scale `tk` (the phone's text base times the text size,
    // else the interface scale), and the box is `TIP_W` original px at `tk`. The formation's
    // cells keep the interface scale. Every entry centred, a paragraph wrapped in the box's text
    // width (0x47e944, 0x47e46c).
    let rows_at = |tk: f32| -> Vec<(String, Ink)> {
        let text_w = tip_box_w(tk) * TIP_TEXT_W / TIP_W;
        t.text
            .iter()
            .flat_map(|(s, ink)| {
                let wrapped = if s.trim().is_empty() { vec![String::new()] } else { with_face(face(*ink), || wrap(s, text_w, glyph(face(*ink), tk))) };
                wrapped.into_iter().map(|r| (r, *ink))
            })
            .collect()
    };
    let text_h_of = |rows: &[(String, Ink)], tk: f32| -> f32 { rows.iter().map(|(_, ink)| tip_lh(*ink, tk)).sum() };
    let lines = game.content.formation.display_lines();
    let grid_h = tip_grid_h(lines);
    let cells = t.army || t.garrison.is_some();
    let grid = if cells { grid_h } else { 0.0 };
    // The layout's heights in units of `u` px: the caption and the text at `u`, the cells at
    // the interface scale.
    let caption = |u: f32| t.garrison.as_ref().map(|_| tip_lh(Ink::Label, u) / u);
    let fits = |u: f32| {
        let text_h = text_h_of(&rows_at(u), u) / u;
        let raw = tip_height_raw(t.army, text_h, grid * k / u, caption(u));
        (!t.army || text_h <= 353.0) && raw <= 512.0 && raw * u <= screen_height() - bar_h()
    };
    let tk = super::display::text_o_fitting(super::display::text_base(k, super::dialog::TEXT_REF), fits);
    let rows = rows_at(tk);
    let text_h = text_h_of(&rows, tk) / tk;
    let w = tip_box_w(tk);
    // The cells' scale: a phone's army takes the text's (capped to what the box holds, see
    // `army_cells_scale`); the desktop and a garrison keep the interface scale.
    let cols = game.content.formation.display_cols() as usize;
    let room = screen_height() - bar_h() - tip_height(true, text_h, 0.0, None) * tk;
    let cs = army_cells_scale(t.army && super::display::dp_px().is_some(), k, tk, grid_h, tip_grid_w(cols), room, w);
    let h = tip_height(t.army, text_h, grid * cs / tk, caption(tk)) * tk;
    // Centred on the mouse and kept inside the map's view, above the bottom bar (interface.md
    // §7.4; 0x4ca8ac).
    let (mx, my) = crate::ui::widgets::pointer();
    let desktop = Rect::new((mx - w / 2.0).min(screen_width() - w).max(0.0), (my - h / 2.0).min(screen_height() - bar_h() - h).max(0.0), w, h);
    // On a phone it goes above the finger, clear of it (`popup::placed`).
    let placed = super::popup::placed(desktop, (mx, my), (w, h));
    let (x, y) = (placed.x, placed.y);
    tooltip_panel_styled(placed, t.style, tk);
    // The title in the yellow Benguiat over its shadow, centred in the strip (27 px at `tk`).
    let title_size = glyph(Face::Title, tk);
    with_face(Face::Title, || {
        let title = ellipsize(&t.title, w * TIP_TITLE_W / TIP_W, title_size);
        shadow_centered(&title, x + w / 2.0, y + 13.5 * tk + title_size * 0.36, title_size, TIP_TITLE);
    });
    let grid_x = x + (w - tip_grid_w(cols) * cs) / 2.0;
    let mut ly = y + TIP_TOP * tk;
    if t.army {
        ly += formation_grid(game, assets, &t.troops, t.team, grid_x, ly, cs) + 8.0 * cs;
    }
    let bottom = y + h - 2.0 * tk;
    for (s, ink) in &rows {
        if ly + tip_lh(*ink, tk) > bottom {
            break;
        }
        if !s.is_empty() {
            with_face(face(*ink), || shadow_centered(s, x + w / 2.0, ly + tip_size(*ink, tk), glyph(face(*ink), tk), tip_color(t.style, *ink)));
        }
        ly += tip_lh(*ink, tk);
    }
    if let Some(caption_text) = &t.garrison {
        // The cells 10 px off the bottom, the caption 8 px over them, and a line 5 px over
        // the caption when there is text above (0x4cb18c).
        let grid_y = y + h - (10.0 + grid_h) * k;
        let caption_y = grid_y - 8.0 * k - tip_lh(Ink::Label, tk);
        if !rows.is_empty() {
            draw_line(x + 2.0 * tk, caption_y - 5.0 * k, x + w - 2.0 * tk, caption_y - 5.0 * k, 1.0, super::chrome::SILVER);
        }
        let ink = Ink::Label;
        with_face(face(ink), || shadow_centered(caption_text, x + w / 2.0, caption_y + tip_size(ink, tk), glyph(face(ink), tk), tip_color(t.style, ink)));
        formation_grid(game, assets, &t.troops, t.team, grid_x, grid_y, k);
    }
}

/// What the mouse is over: an army, else a building.
fn hover_tooltip(game: &Game, cam: &Camera) -> Option<Tooltip> {
    let m = Vec2::from(crate::ui::widgets::pointer());
    if !cam.over_map(m) {
        return None;
    }
    let near = 22.0 * (cam.scale / PX).max(0.6);
    let over = |pos: (f32, f32)| (cam.screen_at(pos) - vec2(0.0, 12.0 * cam.scale / PX) - m).length() < near;
    // Occupant code 1, the hero, is army 0 to the tooltip (0x4cbf20 → 0x4cc274).
    if over(game.display_pos()) {
        return Some(hero_tooltip(game));
    }
    let map = &game.world.map;
    if let Some(a) = game.world.armies.iter().filter(|a| game.fog.explored(a.tile(map))).find(|a| over(game.army_display_pos(a))) {
        return Some(army_tooltip(game, a));
    }
    let t = cam.tile_under_mouse().filter(|&t| game.fog.explored(t))?;
    let l = game.world.location_covering(t).or_else(|| game.world.location_at(t))?;
    if let Some(a) = tooltip_guard(game, l) {
        return Some(army_tooltip(game, a));
    }
    Some(location_tooltip(game, &game.world.locations[l]))
}

/// The pointer's place over army `i` for an off-screen snapshot (`RAZDOR_SCENE_ARMY_HOVER`):
/// the point the map's hover test takes for it (its cell, 12 px above its foot, as
/// [`army_tooltip`]'s hit test), at the default zoom.
pub fn army_pointer(game: &Game, i: usize) -> Option<(f32, f32)> {
    let a = game.world.armies.get(i)?;
    let cam = Camera::looking_at(game, 1.0, game.display_pos());
    Some((cam.screen_at(game.army_display_pos(a)) - vec2(0.0, 12.0 * cam.scale / PX)).into())
}

/// The original's tooltip of the hero's own army (0x4ca9f0 with army 0): "Отряд героя",
/// its 2×6 cards, "Предводитель" and the hero's name, in the normal frame.
fn hero_tooltip(game: &Game) -> Tooltip {
    let troops = game
        .squad
        .iter()
        .map(|u| {
            let mut t = Troop::new(u.def, u.level, u.slot);
            t.hurt = (u.max_hp(&game.content) - u.hp).max(0);
            t
        })
        .collect();
    let text = vec![(info("Commander", n_("Leader")), Ink::Label), (game.hero_name(), Ink::Name)];
    Tooltip { title: info("HeroArmy", n_("Hero's army")), text, troops, garrison: None, army: true, team: Team::Player, style: TipStyle::Normal }
}

/// The army a building's tooltip shows instead of the building's (0x4cc2a5–0x4cc31a): for a
/// village, castle, fort, ruins or bridge, the last army in the scenario's order that is on
/// the map and stands in it (army +0x3788, the building it is in, as the step's guard),
/// whatever its attitude (the king in his castle).
fn tooltip_guard(game: &Game, l: usize) -> Option<&Army> {
    use LocationKind as K;
    let kind = game.world.locations[l].kind;
    if !matches!(kind, K::Village | K::Castle | K::Fort | K::Ruins | K::StoneBridge | K::WoodenBridge) {
        return None;
    }
    game.world.armies.iter().filter(|a| a.mind.standing == Some(l)).max_by_key(|a| a.id)
}

/// The map file's number of a building type (0x4ccc7a reads it): 0 palace … 12 ruins, 13
/// and 14 the bridges, 15 the obelisk. The demo's camp is taken for a fort.
fn building_type(k: LocationKind) -> u8 {
    use LocationKind as K;
    match k {
        K::Palace => 0,
        K::Town => 1,
        K::Village => 2,
        K::Castle => 3,
        K::Fort | K::Camp => 4,
        K::Tavern => 5,
        K::Market => 6,
        K::Church => 7,
        K::Smithy => 8,
        K::Shipyard => 9,
        K::Altar => 10,
        K::Entrance => 11,
        K::Ruins => 12,
        K::StoneBridge => 13,
        K::WoodenBridge => 14,
        K::Obelisk => 15,
    }
}

/// What the world's hit test (0x4cbf20) finds at cell `t`: unexplored (the explored map's
/// pixel 0), a target ([`Game::can_target`]), and on explored ground off the minimap the
/// cell's army (the hero, an army, the hero's parked ship) and the building over it; while
/// the hero sails a bridge is neither (0x4cc108).
fn map_hover(game: &Game, t: Tile, on_minimap: bool) -> MapHover {
    let w = &game.world;
    let map = &w.map;
    let unexplored = !map.in_bounds(t) || !game.fog.explored(t);
    let mut h = MapHover { on_minimap, unexplored, valid: !on_minimap && game.can_target(t), ..Default::default() };
    if on_minimap || unexplored {
        return h;
    }
    let meeting = |a: &Army| game.meeting_marked(a.id);
    h.army = if t == game.tile() {
        CellArmy::Hero
    } else if let Some(a) = w.armies.iter().find(|a| a.tile(map) == t) {
        CellArmy::Army { attitude: a.own_attitude(), meeting: meeting(a) }
    } else if game.parked_ship() == Some(t) {
        CellArmy::Ship
    } else {
        CellArmy::None
    };
    if let Some(l) = w.location_covering(t) {
        let loc = &w.locations[l];
        if game.aboard() && loc.kind.is_bridge() {
            return MapHover { army: CellArmy::None, valid: false, ..h };
        }
        h.building = Some(BuildingHover {
            kind: building_type(loc.kind),
            standing: w.armies.iter().filter(|a| a.mind.standing == Some(l)).map(|a| (a.own_attitude(), meeting(a))).collect(),
            garrison: !loc.cleared && loc.garrison.iter().any(|t| t.alive()),
            attitude: loc.attitude,
            ownerless: loc.owner == Owner::Neutral,
        });
    }
    h
}

/// The world map's pointer this frame (0x4cc148). While the map is busy (a wait or a
/// spell's reading 0x4ae311, a glide 0x4af99a, a place or the start's fog opening 0x4af86a,
/// a spell's effect 0x4af32a) the clock; once it is done the arrow (0x4b9060, 0x4ae24c →
/// 0x48ecbc). As the hero sets off the arrow (0x4cd184) and no hover. On the idle map the
/// pointer is worked out again only when it enters another cell (0x4ccab3-0x4ccacb):
/// [`cursor::map_pointer`]; below the map (the bar) the arrow (0x4cceeb). A wait or a walk
/// makes the next hover look again (0x4ae302, 0x4adc26).
fn update_pointer(game: &Game, view: &mut MapView, cam: &Camera, on_minimap: bool) {
    let waits = game.waiting() || game.endless_waiting() || game.reading().is_some();
    let showing = view.shows.front().is_some_and(|s| s.started.is_some());
    if waits || showing || view.centring.is_some() || view.returning.is_some() || view.opening.is_some() || !view.spell_fx.is_empty() {
        view.pointer = Shape::Clock;
        view.was_busy = true;
        if waits {
            view.pointer_cell = None;
        }
        return;
    }
    if std::mem::take(&mut view.was_busy) {
        view.pointer = Shape::Arrow;
    }
    if game.moving() {
        view.pointer = Shape::Arrow;
        view.pointer_cell = None;
        view.hover = None;
        return;
    }
    let Some(t) = cam.tile_under_mouse() else {
        // Below the map the cell's ring stays (0x4cceeb restores nothing).
        view.pointer = Shape::Arrow;
        return;
    };
    if view.pointer_cell != Some(t) {
        view.pointer_cell = Some(t);
        let h = map_hover(game, t, on_minimap);
        view.pointer = cursor::map_pointer(view.pointer, &h);
        view.hover = Some(hover_look(game, t, &h));
    }
}

/// The hover's marks for cell `t`, from its hit test `h`: the ring in the colour of the
/// original's mark ([`cursor::hover_ring`]: 2 the route end's green, 3 the armies' red) and
/// the building over it, if any.
fn hover_look(game: &Game, t: Tile, h: &MapHover) -> MapHoverLook {
    let ring = cursor::hover_ring(h).map(|c| if c == 3 { ("Selection-1", MARK_ARMY) } else { ("Selection-0", MARK_ROUTE_END) });
    // A bridge is a piece of the land, nothing to enter: never outlined. Another kind of
    // building drawn with a bridge's picture is.
    let building = h.building.as_ref().and_then(|_| game.world.location_at(t));
    MapHoverLook { cell: t, ring, building }
}

/// A click on the building the party stands in (`t` one of its cells): he enters it again
/// (0x4cd0aa → 0x4bbc84: a village's offer, or its window paying what has refilled), or the
/// battle with a garrison still to beat. Nothing for a burnt camp. `None` when the click was
/// not on that building.
fn reopen_here(game: &mut Game, t: Tile, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, session: Option<&mut super::session::Keeper>) -> Option<Option<Screen>> {
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
        return Some(Some(saves::battle(game, session)));
    }
    let events = game.reenter_building();
    Some(handle_events(game, events, message, dialogs))
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
        Event::Script(O::QuestCompleted { quest, .. }) => format!("QUEST {quest} «{}» completed", title(*quest)),
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
        // An event's spell writes nothing: the original shows only its effect.
        Event::NewDay(_) | Event::Captured(_) | Event::Script(_) | Event::EventSpell { .. } => None,
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
            let name = game.world.armies.get(*i).map_or("", |a| a.shown_name());
            Some(if name.is_empty() { tr("An army attacks!").to_string() } else { trf!("{name} attacks!", name) })
        }
        Event::Met(i) => {
            let name = game.world.armies.get(*i).map_or("", |a| a.shown_name());
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
    let mut tribute = None;
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
                } else if let Some(o) = game.village_offer() {
                    // A village's offer is a question in the event window, before any village
                    // window (0x4aca80).
                    *message = None;
                    dialogs.push_back(super::building_view::offer_dialog(game, o, false));
                } else if let Some(first) = game.window_at(l) {
                    *message = None;
                    next = Some(Screen::Building(BuildingView::new(first)));
                }
            }
            Event::NewDay(r) => dialogs.push_back(Dialog::day_report(game, &r)),
            // A building taken on the way opens no window and does not stop the walk
            // (0x4ad94c); the village's own window opens when he ends his walk in it.
            // The tribute's gold sound plays as the village window closes (`building_view`).
            Event::Tribute { paid, mana, .. } => {
                let gold = match paid {
                    razdor::rules::game::Tribute::Gold(g) => g,
                    razdor::rules::game::Tribute::Item(_) => 0,
                };
                tribute = Some((gold, mana));
            }
            // A spell read to its end: the camera to an enemy target, then the effect.
            Event::SpellCast { spell, target, outcome } => {
                if let (Some(s), CastOutcome::Done { .. }) = (game.spell(spell).cloned(), outcome) {
                    if !matches!(target, CastTarget::Own) {
                        look_at(target);
                    }
                    spell_effect(&s, target);
                }
            }
            // An event's spell on the hero: its effect over him, its sound by the spell's own
            // target (0x4af392 / 0x4af3a2: a spell for an enemy plays `Spell-Evil` even here).
            Event::EventSpell { spell } => {
                if let Some(s) = game.spell(spell).cloned() {
                    super::audio::cue(if razdor::rules::magic::targets_enemy(&s) { super::audio::Cue::SpellEvil } else { super::audio::Cue::SpellGood });
                    spell_effect(&s, CastTarget::Own);
                }
            }
            Event::Captured(_) | Event::Met(_) | Event::Battle(_) => {}
            // A level gained is silent: `Unit-Upgrade` is the promotion screen's (0x4b1af8).
            Event::LevelUp(..) => {}
            Event::Script(o) => story::show(game, &o, dialogs),
        }
    }
    if let Some(Screen::Building(v)) = next.as_mut() {
        if let Some(t) = tribute {
            v.tribute_paid = true;
            v.tribute = t;
        }
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

/// The camera of the map under a window: as the map was last drawn (its zoom, where it
/// looked), so opening a window neither moves nor re-zooms it; the hero at zoom 1 before.
fn backdrop_camera(game: &Game, full: Rect) -> Camera {
    let (zoom, look) = LAST_CAM.with(|c| c.get()).unwrap_or((1.0, None));
    Camera::looking_in(game, zoom, look.unwrap_or(game.display_pos()), full)
}

/// The map under a building window or a dialog: drawn over the whole screen, not
/// interactive, with the bar's buttons greyed (`lit`: the open screen's button).
pub fn backdrop_lit(game: &Game, assets: &Assets, lit: Option<BarButton>) {
    draw_map_backdrop(game, assets);
    game_bar::draw(game, |b| if Some(b) == lit { Look::Lit } else { Look::Grey });
    // The window or dialog drawn next stays above the bar (`ui::pan`).
    super::pan::cover_bar();
}

/// The map under a window: its world and fog drawn at the map's resolution (`map_scale`), then dimmed.
/// The caller has cleared the screen and holds the pan's offset off (`ui::pan::unpanned`).
fn backdrop_world(game: &Game, assets: &Assets) {
    let full = map_area();
    let cam = backdrop_camera(game, full);
    map_scale::draw(map_render_scale(), rgb(10, 12, 10), || {
        draw_world(game, assets, &cam, None, None, None, false, NO_VEIL, None);
        if cam.tilt.is_none() {
            cam.draw_fog(game, &[]);
        }
        draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.2));
    });
}

/// The map under a window: clear, drawn, fogged and dimmed (the bar is drawn by the caller).
fn draw_map_backdrop(game: &Game, assets: &Assets) {
    // The map stays where it is when the window over it is panned (`ui::pan`).
    super::pan::unpanned(|| {
        clear_background(rgb(10, 12, 10));
        backdrop_world(game, assets);
    });
}

/// The map under the settings window: the bar's settings button is live (lit), the others
/// grey and not pressable, as `backdrop_lit`. Returns whether the settings button was pressed
/// this frame (the caller closes the window with its changes kept).
pub fn settings_backdrop(game: &Game, assets: &Assets) -> bool {
    draw_map_backdrop(game, assets);
    let pressed = game_bar::draw(game, |b| if b == BarButton::Settings { Look::Lit } else { Look::Grey });
    super::pan::cover_bar();
    pressed == Some(BarButton::Settings)
}

pub fn backdrop(game: &Game, assets: &Assets) {
    backdrop_lit(game, assets, None);
}

/// The map behind a battle (the window's backdrop), drawn once and shown again: the picture of
/// [`draw_map_backdrop`] is filled into `cache` when its key changes, and the game bar is drawn
/// live over it, as [`backdrop`] does. The animations behind the battle (trees, water, marks,
/// idle figures) stand still while the picture is kept: that is the battle's by design.
pub fn battle_backdrop(game: &Game, assets: &Assets, cache: &mut super::render_cache::Cached) {
    cache.show(battle_backdrop_key(game), || draw_map_backdrop(game, assets));
    game_bar::draw(game, |_| Look::Grey);
    super::pan::cover_bar();
}

/// The key of the battle's cached map picture: everything the picture reads except the
/// animations (the clock): the screen's pixels, the interface scale and DPI, the backdrop
/// camera (zoom, look, the hero, the 2.5D tilt and yaw), the fog, the places being shown, and
/// each army's and the hero's place and look. A battle changes none of these (its hit points
/// are not drawn on the map).
fn battle_backdrop_key(game: &Game) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    let bits = |(x, y): (f32, f32)| (x.to_bits(), y.to_bits());
    (screen_width().to_bits(), screen_height().to_bits(), screen_dpi_scale().to_bits()).hash(&mut h);
    super::chrome::k().to_bits().hash(&mut h);
    super::display::dp_px().map(f32::to_bits).hash(&mut h);
    let area = map_area();
    (area.x.to_bits(), area.y.to_bits(), area.w.to_bits(), area.h.to_bits()).hash(&mut h);
    let (zoom, look) = LAST_CAM.with(|c| c.get()).unwrap_or((1.0, None));
    zoom.to_bits().hash(&mut h);
    look.map(bits).hash(&mut h);
    bits(game.display_pos()).hash(&mut h);
    (tilt_view(), first_person(), horizon(), eyes_setting(), tilt_pitch().to_bits(), tilt_yaw().to_bits(), extend_edge(), mist::on()).hash(&mut h);
    (game.fog.enabled, &game.fog.bits).hash(&mut h);
    game.unshown_cells().hash(&mut h);
    (game.aboard(), game.hero_hidden()).hash(&mut h);
    if let Some(s) = game.ship {
        (s.tile, s.aboard).hash(&mut h);
    }
    for a in &game.world.armies {
        (a.uid, a.tile(&game.world.map), a.figure, a.ship, a.sails(), a.hostile(), a.chasing).hash(&mut h);
        bits(game.army_display_pos(a)).hash(&mut h);
        (game.army_hidden(a), a.path.first().copied()).hash(&mut h);
    }
    h.finish()
}

/// The map under a window the bar stays live for (army, spell book, journal, a building),
/// as in the original: its buttons are blue, the window's own one green; one pressed gives
/// the screen it opens (the lit one, or the map button, closes the window).
pub fn window_backdrop(game: &Game, assets: &Assets, lit: Option<BarButton>) -> Option<Screen> {
    // The map and the bar are drawn at the interface scale, whatever window is over them.
    super::display::with_window_scale(None, || window_backdrop_at_scale(game, assets, lit))
}

fn window_backdrop_at_scale(game: &Game, assets: &Assets, lit: Option<BarButton>) -> Option<Screen> {
    // The map stays where it is when the window over it is panned (`ui::pan`).
    super::pan::unpanned(|| {
        clear_background(rgb(10, 12, 10));
        backdrop_world(game, assets);
    });
    let idle = game.foe.is_none();
    let modal = input_blocked();
    let pressed = game_bar::draw(game, |b| match b {
        _ if modal => Look::Grey,
        _ if Some(b) == lit => Look::Lit,
        BarButton::Save | BarButton::Spells if !idle => Look::Grey,
        _ => Look::Normal,
    });
    // The window drawn next stays above the bar, which keeps its own presses (`ui::pan`).
    super::pan::cover_bar();
    let pressed = pressed?;
    Some(match pressed {
        b if Some(b) == lit => Screen::WorldMap,
        BarButton::Menu => Screen::Menu(false),
        BarButton::Settings => Screen::Settings,
        BarButton::Save => Screen::Save(SaveView::new(game, Back::Map)),
        BarButton::Load => Screen::Load(LoadView::new(Back::Map)),
        BarButton::Hero => Screen::Hero(Default::default()),
        BarButton::Squad => Screen::Squad { selected: Default::default(), scroll: 0, back: None },
        BarButton::Spells => Screen::Spellbook { selected: 0, page: 0, filter: Default::default() },
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
    // J opens Razdor's journal (its own window; the bar's flag is the original's hero window).
    let journal = pressed.is_none() && key(keymap::key(Action::Journal));
    // The bar's keys (Razdor's) act as its buttons, also while the hero walks or waits: the
    // key held has cut his walk (`frame`), as the original's any key does.
    if pressed.is_none() && !journal {
        // Esc closes the minimap first (Razdor's); the menu only when nothing else is open.
        pressed = if key(KeyCode::Escape) {
            Some(esc_on_map(minimap_open))
        } else if idle && key(keymap::key(Action::SpellBook)) {
            Some(BarButton::Spells)
        } else if key(keymap::key(Action::HeroArmy)) {
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
        Some(BarButton::Hero) => Some(Screen::Hero(Default::default())),
        Some(BarButton::Squad) => {
            *message = None;
            Some(Screen::Squad { selected: Default::default(), scroll: 0, back: None })
        }
        Some(BarButton::Spells) => {
            *message = None;
            Some(Screen::Spellbook { selected: 0, page: 0, filter: Default::default() })
        }
        Some(BarButton::Map) => None,
        // J: the hero window, whose diary holds the quests and Razdor's history.
        None if journal => Some(Screen::Hero(Default::default())),
        None => None,
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

pub fn frame(game: &mut Game, assets: &Assets, view: &mut MapView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, session: Option<&mut super::session::Keeper>) -> Option<Screen> {
    let next = map_frame(game, assets, view, message, dialogs, session);
    // Opening any window ends the spell's targeting (0x4b8d28).
    if next.is_some() {
        end_targeting();
    }
    next
}

fn map_frame(game: &mut Game, assets: &Assets, view: &mut MapView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, session: Option<&mut super::session::Keeper>) -> Option<Screen> {
    // Photo mode is asked for by the game menu's «Фото» (`photo::request`) or by its key, on the map
    // with no dialog open. Once on, its frame draws the map alone and no screen comes from it.
    let entering = !photo::active() && (photo::take_request() || (dialogs.is_empty() && !input_blocked() && key(keymap::key(Action::Photo))));
    if entering {
        enter_photo(game, view);
    }
    if photo::active() {
        if let PhotoStep::Drawn = photo_frame(game, assets, view, entering) {
            return None;
        }
    }
    world_frame(game, assets, view, message, dialogs, session)
}

fn world_frame(game: &mut Game, assets: &Assets, view: &mut MapView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>, session: Option<&mut super::session::Keeper>) -> Option<Screen> {
    clear_background(rgb(10, 12, 10));
    // The minimap's meeting marks are recomputed only while the event display is idle (the
    // original's 0x4abf44 runs when the scan is idle, after the dialogs): an army met keeps its
    // ally colour while the meeting's dialog or a showing is on screen.
    // An unfinished step (`Game::tick_within`) has not happened yet for the marks either.
    if dialogs.is_empty() && !view.flying() && view.spell_fx.is_empty() && !game.event_display_busy() && !game.ai_pending() {
        game.refresh_meeting_marks();
    }

    // The original's world frame acts on the map only while it is idle (interface.md §7.3,
    // 0x4cc1ff): while the hero walks or waits, it only watches for the stop.
    // The glide back to the hero takes no input either (0x4af96c).
    let idle = !game.moving() && !game.waiting() && game.reading().is_none() && view.centring.is_none();
    // The whole milliseconds since the map's last frame (the original's timeGetTime).
    let now_ms = (crate::ui::clock::now() * 1000.0) as i64;
    let dt_ms = now_ms - view.last_frame_ms.unwrap_or(now_ms);
    view.last_frame_ms = Some(now_ms);
    view.show_clock += crate::ui::clock::frame_dt().clamp(0.0, 1.0 / 30.0) as f64;
    // A gamepad's right stick scrolls the map here (Razdor's), and turns no wheel.
    let pad_scroll = crate::ui::input::map_scroll();

    // Zoom: mouse wheel or +/- (Razdor's, also while he walks or waits), in notches from the
    // settings' zoom; 0 comes back to it. Locked in the settings, the zoom stays theirs.
    let (home, locked) = (home_zoom(), ZOOM_LOCKED.load(std::sync::atomic::Ordering::Relaxed));
    // The wheel zooms out to the whole map in the view (Razdor's), past the settings' range.
    let lo = {
        let map = &game.world.map;
        let ms = map_scale();
        zoom_floor(map.w as f32 * PX * ms, map.h as f32 * map.grid.row_height() * PX * ms, map_area())
    };
    if !locked {
        // The steps follow the view's floor and the settings' zoom (see `fit_steps`).
        view.zoom_steps = fit_steps(home, view.zoom_steps, lo);
        let wheel = wheel();
        if wheel != 0.0 {
            view.zoom_steps = step_zoom(home, view.zoom_steps, if wheel > 0.0 { 1 } else { -1 }, lo);
        }
        if key(KeyCode::Equal) || key(KeyCode::KpAdd) {
            view.zoom_steps = step_zoom(home, step_zoom(home, view.zoom_steps, 1, lo), 1, lo);
        }
        if key(KeyCode::Minus) || key(KeyCode::KpSubtract) {
            view.zoom_steps = step_zoom(home, step_zoom(home, view.zoom_steps, -1, lo), -1, lo);
        }
        if key(KeyCode::Key0) || key(KeyCode::Kp0) {
            view.zoom_steps = 0;
        }
    }
    view.zoom = zoom_at(home, view.zoom_steps, locked, lo);
    // PageUp and PageDown tilt the 2.5D view, 5° a press; Q and E turn it 45° (the flat view
    // ignores them). The shown yaw eases toward its target every frame.
    // The eyes view turns with the hero's walk first, so the easing carries it.
    if eyes_view() {
        follow_walk(game);
    }
    if tilt_view() {
        ease_tilt_yaw(dt_ms);
    }
    if tilt_view() && dialogs.is_empty() {
        // The first-person view's pitch is fixed (`view25::FP_PITCH`): only the 2.5D view tilts.
        if !first_person() && key(KeyCode::PageUp) {
            set_tilt_pitch(tilt_pitch() - TILT_STEP);
        }
        if !first_person() && key(KeyCode::PageDown) {
            set_tilt_pitch(tilt_pitch() + TILT_STEP);
        }
        if key(KeyCode::Q) {
            turn_tilt_yaw(-TILT_YAW_STEP);
        }
        if key(KeyCode::E) {
            turn_tilt_yaw(TILT_YAW_STEP);
        }
        // A two-finger twist turns it a step a turn (clockwise, as E).
        let turns = crate::ui::input::map_rotate();
        if turns != 0 {
            turn_tilt_yaw(turns as f32 * TILT_YAW_STEP);
        }
    }

    // M toggles the minimap; F3 Razdor's debug overlay (both as the player has bound them).
    if key(keymap::key(Action::Minimap)) {
        view.minimap = !view.minimap;
    }
    if key(keymap::key(Action::Debug)) {
        view.debug = !view.debug;
    }
    // Places the scenario has just shown wait in line (dark until their turn). Tab or a
    // click on the map skips the showing; Tab: the camera back on the hero (after the
    // minimap or a showing moved it).
    for shown in std::mem::take(&mut game.shown) {
        view.shows.push_back(Showing::new(game, &shown));
    }
    if (!view.shows.is_empty() || view.returning.is_some()) && !input_blocked() && ((idle && clicked()) || key(keymap::key(Action::CentreHero))) {
        view.shows.clear();
        view.returning = None;
        view.look = None;
    }
    // Tab: the centre button's glide (Razdor's key), when the view is off the hero; while
    // he walks (Razdor's key works then too) the view is on him already.
    if view.centring.is_none() && view.look.is_some() && key(keymap::key(Action::CentreHero)) {
        if idle {
            view.centring = Some((crate::ui::clock::now(), view.look.unwrap_or(game.display_pos())));
        } else {
            view.look = None;
        }
    }
    if let Some((t0, from)) = view.centring {
        let ms = ((crate::ui::clock::now() - t0) * 1000.0) as i64;
        let to = game.display_pos();
        let e = glide_ease(ms);
        view.look = Some((from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e));
        if ms >= GLIDE_MS {
            view.centring = None;
            view.look = None;
        }
    }
    if idle && dialogs.is_empty() && view.grab.is_none() && !view.flying() && view.spell_fx.is_empty() {
        scroll(game, view, dt_ms, pad_scroll);
        pan(game, view, crate::ui::input::map_pan());
    }
    // While he walks the view is locked on him (interface.md §8).
    view.look = camera_look(view.look, game.moving(), !view.shows.is_empty());
    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    // The compass button (2.5D, turned away from the default angle) takes its own taps: a tap
    // turns the view back to north up, and a pointer over it is not the map's (`on_minimap`).
    let compass = compass_rect(game, view, &cam);
    let on_compass = compass.is_some_and(|r| r.contains(Vec2::from(crate::ui::widgets::pointer())));
    let on_minimap = on_minimap_pointer(game, view, cam.view) || on_compass;
    if let Some(r) = compass.filter(|_| dialogs.is_empty() && !input_blocked()) {
        if crate::ui::widgets::tapped_in(r) {
            reset_tilt_yaw();
        }
    }
    // A one-finger swipe over the map pans it (gestures, a phone): the map view, with the open
    // minimap as a blocker (its drag stays a drag), and only with no dialog open on the map.
    if dialogs.is_empty() && !input_blocked() {
        crate::ui::input::swipe_area(map_area(), SwipeKind::Pan);
        // Beside the bar the map is drawn down to the screen's edge: a swipe there pans it too.
        let bar = game_bar::bar_rect();
        for side in [Rect::new(0.0, bar.y, bar.x, bar.h), Rect::new(bar.x + bar.w, bar.y, screen_width() - bar.x - bar.w, bar.h)] {
            if side.w > 0.0 {
                crate::ui::input::swipe_area(side, SwipeKind::Pan);
            }
        }
        if view.minimap {
            crate::ui::input::swipe_area(minimap::outer(&game.world.map, cam.view), SwipeKind::Off);
        }
    }
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
        if game.endless_waiting() && held_key() == Some(keymap::key(Action::EndEndlessWait)) {
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
    } else if let Some(spell) = targeting() {
        // An enemy spell's targeting (0x4cc148): a left click on the army aimed at casts it,
        // a right click or Esc cancels (0x4cd016); a click anywhere else does nothing.
        if right_clicked() || key(KeyCode::Escape) {
            end_targeting();
            swallow_input();
        } else if clicked() && !on_minimap {
            if let Some(Some(Ok(uid))) = hovered.map(|t| game.curse_aim(t)) {
                end_targeting();
                super::spellbook::cast(game, spell, CastTarget::Army(uid), message);
            }
        }
    } else if clicked() && !on_minimap && !game.holds_events() {
        // Not while the last step is drawn with its events held: they come at its end, and a
        // click on the building it entered would enter it again before them (a village's
        // window "already taken" over its tribute).
        if let Some(screen) = hovered.and_then(|t| reopen_here(game, t, message, dialogs, session)) {
            // A click on the building the party stands in opens it again.
            reopened = screen;
        } else if let Some(target) = hovered.map(|t| figure_aim(game, assets.dt.as_ref(), &cam, t, map_pointer(&cam))).filter(|&t| game.can_target(t)) {
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
        right_clicked() && !on_minimap && !on_clock && cam.over_map(Vec2::from(pointer()))
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
    let tile0 = game.tile();
    let ticked = !input_blocked() && events.is_empty() && photo::may_tick();
    if ticked {
        // The AI's moves of a step may take more than the frame has: they stop at the budget and
        // go on in the next frames, which keep drawing (`Game::tick_within`).
        let frame_start = std::time::Instant::now();
        let mut within_budget = || frame_start.elapsed() < AI_FRAME_BUDGET;
        events = frame_probe::time(Stage::Tick, || game.tick_shown_within(crate::ui::clock::frame_dt().min(0.1), &mut within_budget));
    }
    // The frame probe's kind: a step (the hero moved to another cell), a walk, or idle. Only a
    // frame whose tick ran is one of these; a frame with a window open or events held stays
    // `Other` (a window over a walk is not a walk frame).
    if ticked {
        let tilt = tilt_view();
        frame_probe::set_kind(if game.tile() != tile0 {
            Kind::WalkStep { tilt }
        } else if game.moving() {
            Kind::Walk { tilt }
        } else {
            Kind::Idle { tilt }
        });
    }
    let mut next = handle_events(game, events, message, dialogs).or(reopened);
    // Battles between map parties (Razdor's): taken every frame, shown where the player sees
    // the cell and the option is on; the notice replaces the battle's plain line.
    let battles_shown = game.battle_marks;
    // The battles of an unfinished step wait for it (`Game::tick_within`).
    let clashes = if game.ai_pending() { Vec::new() } else { game.take_clashes() };
    for clash in clashes {
        if battles_shown && game.fog.explored(clash.tile) {
            *message = Some(clash_notice(&clash, &info("NoNameArmy", n_("Unknown army"))));
            view.clash_marks.push_back((clash.tile, crate::ui::clock::now()));
            while view.clash_marks.len() > CLASH_MAX {
                view.clash_marks.pop_front();
            }
        }
    }

    // The place being shown: once its message is read, the camera flies there and the
    // uncovered area fades in; then the next place, if any. After the last one the camera
    // flies back to the hero.
    let now = crate::ui::clock::now();
    let show_now = view.show_clock;
    for shown in std::mem::take(&mut game.shown) {
        view.shows.push_back(Showing::new(game, &shown));
    }
    // An event's places are shown once its own window is closed, before the next window
    // opens (`App` holds it meanwhile). Continue directly to the next ready place;
    // return to the hero once the queue ends or the next place waits for its dialog.
    if let Some((t0, from)) = view.returning {
        let p = ((show_now - t0) / SHOW_PAN).clamp(0.0, 1.0) as f32;
        let ease = p * p * (3.0 - 2.0 * p);
        let to = game.display_pos();
        view.look = Some((from.0 + (to.0 - from.0) * ease, from.1 + (to.1 - from.1) * ease));
        if p >= 1.0 {
            view.returning = None;
            view.look = None;
        }
    } else if let Some(front) = view.shows.front_mut().filter(|s| s.free(dialogs)) {
        let here = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos())).centre();
        let (t0, from) = *front.started.get_or_insert((show_now, here));
        let p = ((show_now - t0) / SHOW_PAN).clamp(0.0, 1.0) as f32;
        let ease = p * p * (3.0 - 2.0 * p);
        view.look = Some((from.0 + (front.at.0 - from.0) * ease, from.1 + (front.at.1 - from.1) * ease));
        if show_now - t0 > SHOW_PAN + SHOW_FADE {
            view.shows.pop_front();
            if view.shows.front().is_none_or(|s| !s.free(dialogs)) {
                let here = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos())).centre();
                view.returning = Some((show_now, here));
            }
        }
    } else {
        play_spell_fx(game, view, now);
    }
    if view.opening.as_ref().is_some_and(|s| s.progress(show_now) >= 1.0) {
        view.opening = None;
    }

    let cam = Camera::looking_at(game, view.zoom, view.look.unwrap_or(game.display_pos()));
    LAST_CAM.with(|c| c.set(Some((view.zoom, view.look))));
    // A spell's own effects, each layer while it runs (0x4af2f8), drawn among the figures.
    let show = match view.spell_fx.front() {
        Some(SpellFx::Layers { army, layers, started: Some(t0) }) => {
            let target = match army {
                None => Some((game.display_pos(), game.tile())),
                Some(uid) => game.world.armies.iter().find(|a| a.uid == *uid).map(|a| (game.army_display_pos(a), a.tile(&game.world.map))),
            };
            target.map(|(at, cell)| SpellShow { layers, at, cell, ms: (now - t0) * 1000.0 })
        }
        _ => None,
    };
    // The hovered building is outlined only while the pointer is on it and the map takes
    // input; the cell's ring stays as the original leaves it.
    let pointed = !input_blocked() && view.pointer != Shape::Clock && view.hover.is_some_and(|h| cam.tile_under_mouse() == Some(h.cell));
    let pointed_at = view.hover.and_then(|h| h.building.filter(|_| pointed));
    let taking = !input_blocked() && view.pointer != Shape::Clock;
    let now_s = crate::ui::clock::now();
    let mouse_cell = cam.tile_under_mouse();
    let near = |b: usize| mouse_cell.is_some_and(|(x, y)| game.world.locations[b].cells().any(|(cx, cy)| (cx - x).abs() <= 1 && (cy - y).abs() <= 1));
    view.outlined = match (pointed_at, view.outlined) {
        (Some(b), _) => Some((b, now_s)),
        // Off it but still within a cell of it, and not on another building.
        (None, Some((b, _))) if taking && view.hover.is_none_or(|h| h.building.is_none()) && near(b) => Some((b, now_s)),
        (None, Some((b, at))) if taking && now_s - at < OUTLINE_GRACE => Some((b, at)),
        _ => None,
    };
    let mut hover = view.hover.map(|h| MapHoverLook { building: view.outlined.map(|(b, _)| b), ..h });
    // Targeting rings the cell under the pointer with `Selection-2` (0x4ccb8f, 0x4cce24):
    // blue (5, 0x00c4ff) on an army it can curse, red (4, 0xdf0000) on any other target
    // cell; no building outline meanwhile.
    if targeting().is_some() {
        hover = cam.tile_under_mouse().filter(|_| idle && !on_minimap).map(|t| {
            let ring = match game.curse_aim(t) {
                Some(Ok(_)) => Some(("Selection-2", rgb(0x00, 0xc4, 0xff))),
                // The hero's own cell is no target (the original's hit test skips his army): no ring.
                _ if game.spell_can_reach(t) && t != game.tile() => Some(("Selection-2", rgb(0xdf, 0x00, 0x00))),
                _ => None,
            };
            MapHoverLook { cell: t, ring, building: None }
        });
    }
    // The fog layers: the 2.5D view draws them on the ground inside the world, under the standing
    // sprites; the flat view draws them after the world, as before.
    let shows: Vec<&Showing> = view.opening.iter().chain(&view.shows).collect();
    let flat = cam.tilt.is_none();
    let hover = hover.filter(|_| !trailer_clean());
    // The light is read once, before the world (its relief lights the sprites and the ground).
    let light = frame_light_of(game);
    let relief = light.and_then(|_| daylight::frame_relief());
    frame_probe::time(Stage::World, || {
        map_scale::draw(map_render_scale(), rgb(10, 12, 10), || {
            draw_world(game, assets, &cam, view.preview.as_ref().map(|p| p.1.as_slice()), show.as_ref(), hover, true, Veil { shows: &shows, now: show_now }, relief);
            // Razdor's «Освещение» (off by default): the light over the world, before the flat fog, so the
            // fog's black stays black. With the mist on («Туман вместо черноты») the flat fog is a colour,
            // so the light goes over it too (the mist takes the night's tint). Off, the order is as before.
            let mist_light = flat && mist::on();
            if !mist_light {
                draw_daylight(game, &cam, light);
            }
            if flat {
                frame_probe::time(Stage::Fog, || cam.draw_fog(game, &shows));
                gpu_wait(Stage::GpuFog);
            }
            if mist_light {
                draw_daylight(game, &cam, light);
            }
        })
    });
    if !trailer_clean() {
        draw_clash_marks(view, &cam, crate::ui::clock::now());
    }
    if flat {
        cam.draw_showing(game, view.opening.iter().chain(&view.shows), show_now);
    }
    if let Some(s) = view.shows.front().filter(|_| !trailer_clean()) {
        cam.draw_known_place(s, show_now);
    }
    // A spell's effect over the army it landed on.
    if let Some(SpellFx::Effect { army, art, started: Some(t0) }) = view.spell_fx.front() {
        if let Some(at) = army_pos(game, *army) {
            let c = cam.screen_at(at);
            let t = ((now - t0) / SPELL_FX_SECS) as f32;
            super::chrome::effect(art, c, cam.cell_size().x * 3.0, t, WHITE);
        }
    }
    // The shown route's travel time, at its end.
    if let Some(end) = view.preview.as_ref().and_then(|p| p.1.last().map(|&e| (e, game.travel_minutes(&p.1)))) {
        let c = cam.screen_cell(end.0);
        let label = duration_label(end.1 as f64);
        let size = (15.0 * super::chrome::k()).round();
        super::chrome::shadow_centered(&label, c.x, c.y - 14.0 * cam.scale / PX, size, ACCENT);
    }

    // Waiting: the three buttons over the message box (shown while the pointer is on it, the
    // panel's centre; interface.md §6) and keys 1 / 4. A click elsewhere on the panel does
    // nothing, as the original's (0x4b930c, 0x4b93a8; interface.md §6, §7). Waits play in real
    // time, a 30-minute tick every 150 ms (`Game::tick`).
    let can_wait = idle && game.foe.is_none();
    // The three buttons show on the idle map only: no walk, wait, flight or glide.
    let time_buttons = can_wait && view.shows.is_empty() && view.returning.is_none() && view.spell_fx.is_empty();
    // The original's wait buttons play the button sound (interface.md §14).
    if can_wait && key(keymap::key(Action::Wait1)) {
        cue(Cue::Button);
        game.begin_wait(1);
    }
    if can_wait && key(keymap::key(Action::Wait4)) {
        cue(Cue::Button);
        game.begin_wait(4);
    }
    // Community F4 (the held key, 0xc277d2): a wait without end; F5 ends it (`frame`'s
    // walking branch).
    if can_wait && held_key() == Some(keymap::key(Action::EndlessWait)) {
        game.begin_endless_wait();
    }
    // A wait drops the route drawn; the planned cell stays (0x4ae280).
    if game.waiting() {
        if let Some(p) = view.preview.as_mut() {
            p.1.clear();
        }
    }

    // The F3 overlay is drawn in the flat view only: its labels are placed on the flat map.
    if view.debug && cam.tilt.is_none() {
        draw_debug(game, &cam);
    }
    let (bar, toggle_map, timed) = if trailer_clean() { (None, false, None) } else { bottom_bar(game, message, view.minimap, time_buttons && !game.waiting()) };
    next = next.or(bar);
    // A time button pressed (its sound played): a wait, or the view's glide back to the hero
    // (0x4b9448: 2 or 8 ticks, 0x4af96c).
    match timed {
        Some(TimeButton::Wait1) => game.begin_wait(1),
        Some(TimeButton::Wait4) => game.begin_wait(4),
        Some(TimeButton::ShowHero) => view.centring = Some((crate::ui::clock::now(), view.look.unwrap_or(game.display_pos()))),
        None => {}
    }
    if toggle_map {
        view.minimap = !view.minimap;
    }
    if view.minimap && !trailer_clean() {
        if let Some(at) = frame_probe::time(Stage::Minimap, || minimap::window(game, assets.dt.as_ref(), cam.view, cam.world_rect(), surface_color)) {
            view.look = Some(at);
        }
        // Razdor's army list under the minimap (advanced option): a click on a row goes there.
        if game.army_list && view.minimap {
            let o = minimap::outer(&game.world.map, cam.view);
            if let Some(at) = minimap::army_list(game, o, screen_height() - bar_h(), &info("NoNameArmy", n_("Unknown army"))) {
                view.look = Some(at);
            }
        }
    }
    if let (Some(r), Some(t), false) = (compass, cam.tilt, trailer_clean()) {
        if let Some(d) = view25::north_on_screen(&t.cam, cam.view) {
            draw_compass(r, d);
        }
    }
    // The pointer (interface.md §7.2); a window or message on top takes the arrow.
    update_pointer(game, view, &cam, on_minimap);
    // Targeting never shows the swords or the house: the arrow, with the question mark or
    // the "no entry" over cells that are no target (0x4cce5b).
    let aiming = targeting().filter(|_| idle && view.pointer != Shape::Clock);
    if aiming.is_some() {
        view.pointer = match cam.tile_under_mouse().filter(|_| !on_minimap) {
            Some(t) => cursor::map_pointer(Shape::Arrow, &MapHover { army: CellArmy::None, building: None, ..map_hover(game, t, on_minimap) }),
            None => Shape::Arrow,
        };
    }
    cursor::set(if trailer_clean() {
        Shape::Hidden
    } else if input_blocked() {
        Shape::Arrow
    } else {
        view.pointer
    });
    // The right button held (the left one up) shows the tooltip of the army or building
    // under it, as in the original (interface.md §7.4), unless it drags the map. The pointer
    // is not drawn meanwhile (0x4cc269, 0x474a81).
    let dragging = view.grab.is_some_and(|g| g.moved);
    let right_held = !input_blocked() && !dragging && is_mouse_button_down(MouseButton::Right) && !is_mouse_button_down(MouseButton::Left);
    // The touchpad mode (Razdor's, a player's suggestion): the cursor resting still for
    // [`super::rest::REST_SECS`] with no button down shows it too. The tooltip is centred on the
    // pointer, so the pointer is hidden here as in the held case.
    let rest_tip = super::touchpad::enabled() && super::rest::rested() && !input_blocked() && !dragging;
    if let Some(t) = hover_tooltip(game, &cam).filter(|_| idle && (right_held || rest_tip) && !on_minimap && !trailer_clean()) {
        draw_tooltip(game, assets, &t);
        cursor::set(Shape::Hidden);
    }
    if let Some(spell) = aiming.filter(|_| !input_blocked()) {
        let aim = cam.tile_under_mouse().filter(|_| !on_minimap).and_then(|t| game.curse_aim(t));
        spell_target_hint(game, spell, aim);
    }
    // A message for the screen this frame opens (a building's window, a battle) is that
    // screen's to show.
    if let Some(m) = message.take_if(|_| next.is_none()) {
        razdor::diag::play(&game.clock.label(), &format!("MESSAGE {m}"));
        view.toasts.push_back((m, crate::ui::clock::now()));
        while view.toasts.len() > TOAST_MAX {
            view.toasts.pop_front();
        }
    }
    if !trailer_clean() {
        draw_toasts(&mut view.toasts, crate::ui::clock::now(), screen_height() - bar_h() - 14.0 * super::chrome::k(), None);
    }
    next
}

/// The pointer is on the map's minimap window or on its army list (Razdor's, the option
/// «Список армий у миникарты»), which the map does not take input through.
fn on_minimap_pointer(game: &Game, view: &MapView, map_view: Rect) -> bool {
    view.minimap
        && (minimap::under_pointer(&game.world.map, map_view)
            || (game.army_list && minimap::list_under_pointer(game, minimap::outer(&game.world.map, map_view), screen_height() - bar_h())))
}

/// Seconds a battle's mark stays on the map; it fades out over the last second.
const CLASH_SECS: f64 = 3.0;
/// Battle marks shown at once; a new one pushes the oldest out.
const CLASH_MAX: usize = 4;

local! {
    /// The soft round light of a glow (white, its alpha falling to 0 at the rim), made on first use.
    static GLOW_TEX: std::cell::RefCell<Option<Texture2D>> = const { std::cell::RefCell::new(None) };
    /// Razdor's «Живая вода» wakes of the sailing figures (`water`), per army index.
    static WAKES: std::cell::RefCell<water::Wakes> = std::cell::RefCell::new(water::Wakes::default());
}

/// The glow's texture (`glow_texture`), made on first use.
fn glow_tex() -> Texture2D {
    GLOW_TEX.with_borrow_mut(|t| t.get_or_insert_with(glow_texture).clone())
}

/// A 64×64 white disc whose alpha falls off to 0 at its rim, linear-filtered.
fn glow_texture() -> Texture2D {
    const N: usize = 64;
    let mut px = vec![0u8; N * N * 4];
    for y in 0..N {
        for x in 0..N {
            let dx = (x as f32 + 0.5) / (N as f32 / 2.0) - 1.0;
            let dy = (y as f32 + 0.5) / (N as f32 / 2.0) - 1.0;
            let r = (dx * dx + dy * dy).sqrt().min(1.0);
            let i = (y * N + x) * 4;
            px[i..i + 4].copy_from_slice(&[255, 255, 255, ((1.0 - r) * (1.0 - r) * 255.0) as u8]);
        }
    }
    let tex = Texture2D::from_rgba8(N as u16, N as u16, &px);
    tex.set_filter(FilterMode::Linear);
    tex
}

/// The frame's light (`daylight::frame_light`) from the game's clock, once per map frame: before the
/// world (its relief is read there, `daylight::frame_relief`) and the tint after it.
fn frame_light_of(game: &Game) -> Option<daylight::Light> {
    let hour = (game.clock.total_minutes() % razdor::rules::clock::MINUTES_PER_DAY as f64 / 60.0) as f32;
    let now = crate::ui::clock::now();
    daylight::frame_light(hour, now, get_frame_time())
}

/// Razdor's «Освещение» (`daylight`, off by default): the map's tint over the drawn world, then the
/// glows of the towns and of the armies (the hero's too), in screen space. Style only: nothing is
/// drawn when it is off, and the rules never read it.
fn draw_daylight(game: &Game, cam: &Camera, light: Option<daylight::Light>) {
    let Some(light) = light else {
        return;
    };
    let now = crate::ui::clock::now();
    let t = light.tint;
    super::chrome::multiply(|| draw_rectangle(cam.draw.x, cam.draw.y, cam.draw.w, cam.draw.h, Color::new(t[0], t[1], t[2], 1.0)));
    if light.glow <= 0.01 {
        return;
    }
    let tex = glow_tex();
    let map = &game.world.map;
    let fog = &game.fog;
    let cell_w = cam.cell_size().x;
    // What the fog shows on a cell this frame (1 lit, 0 black): a glow fades with it, as the
    // 2.5D sprites do (`Shade::at`), since the fog is drawn inside the world there. A disabled fog
    // darkens nothing. The glows fade with its square: a half-seen town must not spill a warm halo
    // over the solid fog beside it.
    let visible = |t: Tile| if !fog.enabled || fog.w <= 0 || fog.h <= 0 { 1.0 } else { 1.0 - minimap::shown_darkness(fog, t) };
    let glow_at = |at: Vec2, size: f32, rgb: [f32; 3], alpha: f32| {
        let color = Color::new(rgb[0], rgb[1], rgb[2], alpha.clamp(0.0, 1.0));
        draw_texture_ex(&tex, at.x - size / 2.0, at.y - size / 2.0, color, DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() });
    };
    super::chrome::additive(|| {
        // The towns and the villages: a warm window glow over the middle of each footprint.
        for l in game.world.locations.iter() {
            let town = matches!(l.kind, LocationKind::Palace | LocationKind::Town | LocationKind::Village | LocationKind::Castle | LocationKind::Fort | LocationKind::Tavern | LocationKind::Church);
            if !town {
                continue;
            }
            let (sum, n, seen) = l.cells().fold(((0.0f32, 0.0f32), 0u32, 0.0f32), |((x, y), n, v), t| {
                let (cx, cy) = map.center(t);
                ((x + cx, y + cy), n + 1, v + visible(t))
            });
            let (centre, vis) = if n == 0 {
                (map.center(l.anchor), visible(l.anchor))
            } else {
                ((sum.0 / n as f32, sum.1 / n as f32), seen / n as f32)
            };
            if vis < 0.01 {
                continue;
            }
            glow_at(cam.screen_at(centre), 2.2 * l.size.0.max(1) as f32 * cell_w, [1.0, 0.72, 0.38], 0.55 * light.glow * vis * vis);
        }
        // The armies on the map, the hero's among them: a torch glow, flickering a little.
        for (i, a) in game.world.armies.iter().enumerate() {
            if !fog.explored(a.tile(map)) || game.army_hidden(a) {
                continue;
            }
            let flicker = 0.92 + 0.08 * (now * 9.0 + i as f64).sin() as f32;
            let vis = visible(a.tile(map));
            glow_at(cam.screen_at(game.army_display_pos(a)), 1.6 * cell_w, [1.0, 0.65, 0.30], 0.45 * light.glow * flicker * vis * vis);
        }
        // The eyes camera stands on the hero: his torch would float in front of the lens.
        if !game.hero_hidden() && !game.aboard() && !eyes_cam(cam) {
            let flicker = 0.92 + 0.08 * (now * 9.0 + game.world.armies.len() as f64).sin() as f32;
            let vis = visible(game.tile());
            glow_at(cam.screen_at(game.display_pos()), 1.6 * cell_w, [1.0, 0.65, 0.30], 0.45 * light.glow * flicker * vis * vis);
        }
    });
}

/// Razdor's «Живая вода» wakes (`water`, off by default): a sailing army in motion (and the hero's
/// galley) leaves its last positions on the ground behind it, fading and growing (`water::wake_fade`).
/// Only on explored cells. Style only: nothing is drawn when the water is not alive.
fn draw_wakes(game: &Game, cam: &Camera) {
    let now = crate::ui::clock::now();
    let map = &game.world.map;
    let fog = &game.fog;
    WAKES.with_borrow_mut(|wakes| {
        for (i, a) in game.world.armies.iter().enumerate() {
            if a.sails() && !a.path.is_empty() && !game.army_hidden(a) && fog.explored(a.tile(map)) {
                wakes.record(i, game.army_display_pos(a), now);
            }
        }
        if game.aboard() && game.moving() && !game.hero_hidden() && fog.explored(game.tile()) {
            wakes.record(water::HERO, game.display_pos(), now);
        }
        wakes.prune(now);
        let tex = glow_tex();
        let cell_w = cam.cell_size().x;
        super::chrome::additive(|| {
            for (_, wake) in wakes.iter() {
                for (p, alpha, size) in wake.visible(now) {
                    let c = cam.to_screen(p.at);
                    let (w, h) = (size * cell_w, size * cell_w * 0.6);
                    draw_texture_ex(&tex, c.x - w / 2.0, c.y - h / 2.0, Color::new(1.0, 1.0, 1.0, alpha), DrawTextureParams { dest_size: Some(vec2(w, h)), ..Default::default() });
                }
            }
        });
    });
}

/// The crossed-swords mark of the battles on the map (Razdor's, «Битвы армий на карте»): the
/// install's swords strip (the cursor's `Swords.ugs`), drawn over its cell above the map's
/// objects and armies, fading out over [`CLASH_SECS`].
fn draw_clash_marks(view: &mut MapView, cam: &Camera, now: f64) {
    view.clash_marks.retain(|(_, at)| now - at < CLASH_SECS);
    for (tile, at) in &view.clash_marks {
        let age = now - at;
        let a = (CLASH_SECS - age).clamp(0.0, 1.0) as f32;
        let frame = ((age * 1000.0 / 30.0) as usize) % 50;
        let Some(tex) = super::chrome::animation("Windows/Swords.ugs").and_then(|f| f.get(frame % f.len().max(1)).cloned()) else {
            return;
        };
        let c = cam.screen_cell(*tile);
        let s = cam.cell_size().x * 1.5;
        super::sharp::draw(&tex, c.x - s / 2.0, c.y - s / 2.0, Color { a, ..WHITE }, DrawTextureParams { dest_size: Some(vec2(s, s)), ..Default::default() });
    }
}

/// The notice of a battle on the map (Razdor's): the names as the hover cards show them;
/// `unnamed` for an army with no name.
fn clash_notice(clash: &Clash, unnamed: &str) -> String {
    let name = |p: &Combatant| match p {
        Combatant::Army(n) if n.is_empty() => unnamed.to_string(),
        Combatant::Army(n) | Combatant::Garrison(n) => n.clone(),
    };
    match (&clash.winner, &clash.loser) {
        (Combatant::Army(_), Combatant::Army(_)) => trf!("The army «{winner}» defeated the army «{loser}».", winner = name(&clash.winner), loser = name(&clash.loser)),
        (Combatant::Army(_), Combatant::Garrison(_)) => trf!("The army «{winner}» defeated the garrison «{loser}».", winner = name(&clash.winner), loser = name(&clash.loser)),
        (Combatant::Garrison(_), _) => trf!("The garrison «{winner}» defeated the army «{loser}».", winner = name(&clash.winner), loser = name(&clash.loser)),
    }
}

/// Seconds a message stays above the bar; it fades out over its last [`TOAST_FADE`].
const TOAST_SECONDS: f64 = 5.0;
const TOAST_FADE: f64 = 0.6;
/// Messages shown at once; a new one pushes the oldest out.
const TOAST_MAX: usize = 5;

/// The messages above the bar, the newest at the bottom, the older ones stacked above it;
/// the run-out ones are dropped.
fn draw_toasts(toasts: &mut VecDeque<(String, f64)>, now: f64, base: f32, size_override: Option<f32>) {
    toasts.retain(|(_, at)| now - at < TOAST_SECONDS);
    let k = super::chrome::k();
    // The photo mode's notices (a phone's 15 sp) are taller than the play's; the play's are as before.
    let (size, h, gap) = match size_override {
        Some(s) => (s, (s * 1.6).max(30.0 * k), 6.0 * k),
        None => ((18.0 * k).max(14.0), (30.0 * k).max(24.0), 6.0 * k),
    };
    let cx = screen_width() / 2.0;
    // The newest is drawn at the base, the older ones stack above it.
    let mut y = base - h;
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

/// The tint of a cell in Razdor's debug overlay (F3) for the hero's way of moving: `cost` is
/// the foot cost (the land map) when on foot, or the ship cost (the water map) when aboard.
/// On foot passable is green, brighter on a road; aboard sailable is blue; blocked is red.
fn walk_tint(cost: Option<u16>, aboard: bool, road: bool) -> Color {
    match (cost, aboard) {
        (None, _) => Color::new(0.9, 0.1, 0.1, 0.14),
        (Some(_), true) => Color::new(0.2, 0.5, 1.0, 0.09),
        (Some(_), false) if road => Color::new(0.1, 0.9, 0.2, 0.12),
        (Some(_), false) => Color::new(0.1, 0.9, 0.2, 0.06),
    }
}

/// Razdor's debug overlay (F3): tints every visible cell by whether the hero can step onto it
/// (on foot: green, brighter on roads; aboard: blue if the ship can sail it; red if blocked).
/// Drawn first, under the labels.
fn draw_walkable(game: &Game, cam: &Camera) {
    let map = &game.world.map;
    let aboard = game.aboard();
    let cell = cam.cell_size();
    let ((c0, c1), (r0, r1)) = cam.visible(map);
    for y in r0..r1 {
        for x in c0..c1 {
            let t = (x, y);
            let cost = if aboard { map.water_cost(t) } else { map.cost(t) };
            let road = !aboard && cost == Some(ROAD);
            let c = cam.cell_centre(t);
            draw_rectangle(c.x - cell.x / 2.0, c.y - cell.y / 2.0, cell.x, cell.y, walk_tint(cost, aboard, road));
        }
    }
}

/// Razdor's debug overlay (F3): every event point and lantern with its id, radius and
/// events, the buildings' events, and a panel with the hero's and the pointer's cells and the
/// events of the place under the pointer. Drawn through the fog.
fn draw_debug(game: &Game, cam: &Camera) {
    use razdor::rules::events::Place;
    draw_walkable(game, cam);
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

/// An entry of an army's "Under spells" line: the spell's name and its time left, "∞" when
/// endless, and how many of the army's units it is on when that is not all of them: "Name
/// (∞, 1 из 12)".
fn spell_entry(name: &str, l: &SpellLine, now: u64) -> String {
    let time = if l.endless { "∞".to_string() } else { duration_label(l.until.saturating_sub(now) as f64) };
    if l.units < l.of {
        format!("{name} ({time}, {})", trf!("{n} of {total}", n = l.units, total = l.of))
    } else {
        format!("{name} ({time})")
    }
}

#[cfg(test)]
mod tests {
    use razdor::rules::clock::duration_label;
    use razdor::rules::content::HeroClass;
    use razdor::rules::magic::SpellLine;
    use razdor::trf;

    #[test]
    fn an_army_spell_shows_endless_as_infinity_and_the_units_under_a_part_of_the_army() {
        let now = 1_000;
        let timed = SpellLine { spell: 2, until: now + 600, endless: false, units: 12, of: 12 };
        assert_eq!(spell_entry("Bless", &timed, now), format!("Bless ({})", duration_label(600.0)));
        let part = SpellLine { units: 1, ..timed };
        assert_eq!(spell_entry("Bless", &part, now), format!("Bless ({}, {})", duration_label(600.0), trf!("{n} of {total}", n = 1, total = 12)));
        let endless = SpellLine { until: razdor::rules::world::MAP_ARMY_SPELL_END, endless: true, units: 1, ..timed };
        let line = spell_entry("Bless", &endless, now);
        assert!(line.starts_with("Bless (∞, "), "{line}");
        assert!(line.ends_with(&format!("{})", trf!("{n} of {total}", n = 1, total = 12))), "{line}");
        assert_eq!(spell_entry("Bless", &SpellLine { endless: true, ..timed }, now), "Bless (∞)");
    }

    /// A turn of a phone (Razdor's extra): a pointer left in the new screen's edge zone scrolls
    /// the map, the centre the turn puts it at does not (`input::cursor_after_resize`).
    #[test]
    fn a_turn_of_the_phone_does_not_scroll_the_map_from_the_old_pointer() {
        use crate::ui::input::cursor_after_resize;
        let (old, new) = ((800.0, 400.0), (400.0, 800.0));
        // Where the pointer was on the old landscape screen: at the new portrait screen's right edge.
        let stale = (398.0, 400.0);
        assert_eq!(edge_dir(stale, new, EDGE), vec2(1.0, 0.0));
        let centred = cursor_after_resize(old, new, true, (790.0, 395.0));
        assert_eq!(edge_dir(centred, new, EDGE), Vec2::ZERO);
    }

    /// A figure is drawn in its cell's turn of the map's pass (0x4c9b5b, 0x4c9dca, drawn-occupant
    /// byte cell +9): a building whose top-left footprint cell is (5, 10) covers a figure one row
    /// north of it (the castle's walls hide a hero behind them), and a figure one row south
    /// stands in front of it. Within one cell the scenery and the building come first.
    #[test]
    fn a_figure_is_drawn_in_its_cells_turn_of_the_pass() {
        use super::{cell_pass_key, LAYER_BUILDING, LAYER_FIGURE, LAYER_SPELL_ABOVE};
        let castle = cell_pass_key((5, 10), LAYER_BUILDING);
        assert!(cell_pass_key((5, 9), LAYER_FIGURE) < castle, "behind the walls: covered");
        assert!(cell_pass_key((5, 11), LAYER_FIGURE) > castle, "in front of the walls: seen");
        assert!(cell_pass_key((5, 10), LAYER_FIGURE) > castle, "its own cell: after the building");
        assert!(cell_pass_key((5, 9), LAYER_BUILDING) < cell_pass_key((5, 9), LAYER_FIGURE));
        assert!(cell_pass_key((5, 9), LAYER_FIGURE) < cell_pass_key((5, 9), LAYER_SPELL_ABOVE));
    }

    /// A figure whose feet are at `feet` and whose sprite square is 40 px (feet 0.8 up it).
    fn figure_at(cell: (i32, i32), feet: (f32, f32)) -> ((i32, i32), macroquad::prelude::Rect, macroquad::prelude::Vec2) {
        use macroquad::prelude::{vec2, Rect};
        (cell, Rect::new(feet.0 - 20.0, feet.1 - 32.0, 40.0, 40.0), vec2(feet.0, feet.1))
    }

    #[test]
    fn a_tap_on_a_figures_body_finds_it_though_the_cell_above_its_feet() {
        use macroquad::prelude::vec2;
        // Feet at (100, 100); the finger at (100, 85) is on the body, in the cell above.
        let figures = [figure_at((5, 5), (100.0, 100.0))];
        assert_eq!(super::figure_under(vec2(100.0, 85.0), &figures), Some((5, 5)));
    }

    #[test]
    fn a_tap_where_two_figures_overlap_picks_the_nearest_feet() {
        use macroquad::prelude::vec2;
        let figures = [figure_at((5, 5), (100.0, 100.0)), figure_at((5, 4), (100.0, 90.0))];
        assert_eq!(super::figure_under(vec2(100.0, 96.0), &figures), Some((5, 5)), "4 px from the first's feet, 6 from the second's");
        assert_eq!(super::figure_under(vec2(100.0, 91.0), &figures), Some((5, 4)), "1 px from the second's feet");
    }

    #[test]
    fn a_tap_outside_every_figure_or_in_its_transparent_margin_finds_none() {
        use macroquad::prelude::vec2;
        let figures = [figure_at((5, 5), (100.0, 100.0))];
        assert_eq!(super::figure_under(vec2(10.0, 10.0), &figures), None, "far from the figure");
        assert_eq!(super::figure_under(vec2(100.0, 70.0), &figures), None, "the top 15% of the sprite is transparent");
        assert_eq!(super::figure_under(vec2(82.0, 90.0), &figures), None, "the side margin is transparent");
        assert_eq!(super::figure_under(vec2(100.0, 90.0), &[]), None, "no figures");
    }

    #[test]
    fn the_debug_overlay_tints_by_how_the_hero_moves() {
        use super::walk_tint;
        let red = Color::new(0.9, 0.1, 0.1, 0.14);
        let green = Color::new(0.1, 0.9, 0.2, 0.06);
        let road = Color::new(0.1, 0.9, 0.2, 0.12);
        let blue = Color::new(0.2, 0.5, 1.0, 0.09);
        assert_eq!(walk_tint(None, false, false), red, "blocked on foot");
        assert_eq!(walk_tint(None, true, false), red, "blocked by ship");
        assert_eq!(walk_tint(Some(2), false, false), green, "passable on foot");
        assert_eq!(walk_tint(Some(3), false, true), road, "a road is brighter");
        assert_eq!(walk_tint(Some(2), true, false), blue, "sailable aboard");
    }

    #[test]
    fn the_edges_scroll_the_map() {
        let e = |x, y| super::edge_dir((x, y), (1024.0, 768.0), 5.0);
        assert_eq!(e(2.0, 300.0), vec2(-1.0, 0.0));
        assert_eq!(e(1022.0, 766.0), vec2(1.0, 1.0), "a corner both ways");
        assert_eq!(e(500.0, 300.0), Vec2::ZERO);
        assert_eq!(e(-3.0, 300.0), Vec2::ZERO, "off the window");
    }

    use super::*;

    #[test]
    fn the_map_starts_at_the_interface_scale_chosen() {
        // Auto: the interface's own scale, the window's fit (1920×1080: 1.5).
        assert_eq!(map_scale_for(0.0, 1.5), 1.5 * 0.9375);
        // A chosen scale is the map's, whatever the window's shape or size.
        assert_eq!(map_scale_for(1.25, 1.5), 1.25 * 0.9375);
        assert_eq!(map_scale_for(1.25, 1.1), map_scale_for(1.25, 3.0));
        assert_eq!(map_scale_for(2.0, 1.1), 2.0 * 0.9375, "not cut down to the fit as the interface is");
        // A bigger interface, a closer map.
        assert!(map_scale_for(2.0, 2.0) > map_scale_for(1.0, 2.0));
    }

    #[test]
    fn the_zoom_steps_from_the_settings_zoom_and_locks() {
        let lo = ZOOM_RANGE.0;
        assert_eq!(zoom_at(1.2, 0, false, lo), 1.2, "0 is the settings' zoom");
        assert!((zoom_at(1.0, 2, false, lo) - 1.21).abs() < 1e-5);
        assert_eq!(zoom_at(1.2, 5, true, lo), 1.2, "locked");
        // The steps stop where the zoom stops changing.
        let mut s = 0;
        for _ in 0..40 {
            s = step_zoom(1.0, s, -1, lo);
        }
        assert!(zoom_at(1.0, s, false, lo) >= lo && step_zoom(1.0, s, 1, lo) == s + 1);
        assert_eq!(zoom_at(1.0, s, false, lo), (1.1f32.powi(s)).max(lo));
        // In, the steps stop at the wheel's ×4 ceiling, above the settings' ×2 (the owner's request).
        let mut t = 0;
        for _ in 0..60 {
            t = step_zoom(1.0, t, 1, lo);
        }
        // The top notch is the highest one still at most ×4 (×3.8 here: a notch is ×1.1).
        let top = zoom_at(1.0, t, false, lo);
        assert!(top <= ZOOM_WHEEL_MAX && top * 1.1 > ZOOM_WHEEL_MAX, "the top notch: {top}");
        assert_eq!(step_zoom(1.0, t, 1, lo), t, "no step past ×4");
        // The settings' slider keeps its own range (×0.4 to ×2).
        assert_eq!(ZOOM_RANGE, (0.4, 2.0));
    }

    #[test]
    fn a_raised_zoom_floor_brings_the_steps_back() {
        // Stepped out to a floor of 0.1 (about 25 notches), then the view turns and the floor
        // rises to 0.3: the steps come back to where the zoom moves again.
        let mut s = 0;
        for _ in 0..60 {
            s = step_zoom(1.0, s, -1, 0.1);
        }
        let s = fit_steps(1.0, s, 0.3);
        let before = zoom_at(1.0, s, false, 0.3);
        let up = step_zoom(1.0, s, 1, 0.3);
        assert!(up != s, "a step up is taken");
        assert!(zoom_at(1.0, up, false, 0.3) > before, "the zoom moves: {before}");
        // The settings' zoom changes it too.
        assert_ne!(zoom_at(1.5, s, false, 0.3), zoom_at(1.0, s, false, 0.3));
    }

    #[test]
    fn a_lowered_zoom_ceiling_brings_the_steps_back() {
        // Stepped in to the ×4 ceiling at home 0.5, then the settings' zoom rises to 2.0:
        // the steps come back so that the zoom is at most ×4 and a step out lowers it.
        let mut s = 0;
        for _ in 0..40 {
            s = step_zoom(0.5, s, 1, ZOOM_RANGE.0);
        }
        let s = fit_steps(2.0, s, ZOOM_RANGE.0);
        assert!(zoom_at(2.0, s, false, ZOOM_RANGE.0) <= ZOOM_WHEEL_MAX);
        let before = zoom_at(2.0, s, false, ZOOM_RANGE.0);
        let down = step_zoom(2.0, s, -1, ZOOM_RANGE.0);
        assert!(zoom_at(2.0, down, false, ZOOM_RANGE.0) < before, "one step down lowers the zoom");
    }

    #[test]
    fn fit_steps_leaves_steps_in_range_alone() {
        // Home 1.0 and a floor of 0.4: the steps run from -9 (0.4 is at most 9 notches out)
        // up to 14 (×4 is at most 14 notches in); 0 is always in range.
        assert_eq!(fit_steps(1.0, 0, 0.4), 0);
        assert_eq!(fit_steps(1.0, -5, 0.4), -5);
        assert_eq!(fit_steps(1.0, -9, 0.4), -9);
        assert_eq!(fit_steps(1.0, 7, 0.4), 7);
        assert_eq!(fit_steps(1.0, 14, 0.4), 14);
        assert_eq!(fit_steps(0.4, 0, 0.4), 0);
        assert_eq!(fit_steps(2.0, 0, 0.4), 0);
        // Out of range: brought to the nearest limit.
        assert_eq!(fit_steps(1.0, -30, 0.4), -9);
        assert_eq!(fit_steps(1.0, 30, 0.4), 14);
        assert!(zoom_at(1.0, fit_steps(1.0, 30, 0.4), false, 0.4) <= ZOOM_WHEEL_MAX);
    }

    #[test]
    fn the_wheel_zooms_out_past_the_settings_range_to_a_lower_floor() {
        // Razdor's: with a lower floor, stepping out goes below 0.4 and stops at the floor.
        let lo = 0.1;
        let mut s = 0;
        for _ in 0..60 {
            s = step_zoom(1.0, s, -1, lo);
        }
        let z = zoom_at(1.0, s, false, lo);
        assert!(z < ZOOM_RANGE.0, "past the settings' 0.4: {z}");
        assert!(z >= lo - 1e-6, "stops at the floor: {z}");
        assert!(step_zoom(1.0, s, -1, lo) == s, "no step below the floor");
        // The same steps with the settings' floor stop at 0.4.
        let mut t = 0;
        for _ in 0..60 {
            t = step_zoom(1.0, t, -1, ZOOM_RANGE.0);
        }
        assert!(zoom_at(1.0, t, false, ZOOM_RANGE.0) >= ZOOM_RANGE.0);
    }

    #[test]
    fn the_zoom_floor_fits_the_whole_map_in_the_view() {
        let view = Rect::new(0.0, 0.0, 800.0, 600.0);
        // A big map (4000×3000 px at zoom 1): its floor is below 0.4, and at that zoom it fits.
        let (w, h) = (4000.0, 3000.0);
        let lo = zoom_floor(w, h, view);
        assert!(lo < ZOOM_RANGE.0, "floor {lo}");
        assert!(w * lo <= view.w + 1e-3 && h * lo <= view.h + 1e-3, "the map fits at the floor");
        // A small map never goes above the settings' floor.
        assert_eq!(zoom_floor(200.0, 150.0, view), ZOOM_RANGE.0);
        // An empty view, or a zero or non-finite map: the settings' floor.
        assert_eq!(zoom_floor(w, h, Rect::new(0.0, 0.0, 0.0, 600.0)), ZOOM_RANGE.0);
        assert_eq!(zoom_floor(w, h, Rect::new(0.0, 0.0, 800.0, 0.0)), ZOOM_RANGE.0);
        assert_eq!(zoom_floor(0.0, h, view), ZOOM_RANGE.0);
        assert_eq!(zoom_floor(f32::NAN, h, view), ZOOM_RANGE.0);
        assert_eq!(zoom_floor(w, f32::INFINITY, view), ZOOM_RANGE.0);
    }

    #[test]
    fn the_route_s_last_cell_has_no_arrow() {
        let arrows = |b: &[bool]| (0..b.len()).filter(|&i| route_arrow(i, b.len(), b[i], b.get(i + 1).copied().unwrap_or(false))).collect::<Vec<_>>();
        assert_eq!(arrows(&[false, false, false]), [0, 1]);
        // Through a building: only the cell it is left from.
        assert_eq!(arrows(&[false, true, true, false, false]), [0, 2, 3]);
        assert!(arrows(&[false]).is_empty());
    }

    #[test]
    fn route_arrows_glow_from_150_to_240() {
        assert_eq!(route_arrow_light(0.0, 0), 150.0 / 255.0);
        assert_eq!(route_arrow_light(900.0, 0), 240.0 / 255.0);
        assert_eq!(route_arrow_light(1000.0, 0), 230.0 / 255.0);
        assert_eq!(route_arrow_light(0.0, 5), 240.0 / 255.0);
        assert_eq!(route_arrow_light(1810.0, 0), 150.0 / 255.0);
    }

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
        // Guarded ruins (a player's report: «Врата Вуальшпиля»): the text block ends with an
        // empty line, the caption in the label font and «Неизвестная армия» in the name font;
        // no cells (0x4cb18c). A castle's garrison gets its caption over its cells instead.
        let ruins = g.world.locations.iter().find(|l| l.name == "Развалины").unwrap();
        let t = location_tooltip(&g, ruins);
        assert!(t.troops.is_empty() && t.garrison.is_none() && !t.army);
        let inks: Vec<Ink> = t.text.iter().map(|(_, i)| *i).collect();
        assert_eq!(inks[inks.len() - 3..], [Ink::Label, Ink::Label, Ink::Name]);
        assert_eq!(t.text[t.text.len() - 3].0, "");
        if !ruins.description.trim().is_empty() {
            assert!(t.text.contains(&(ruins.description.clone(), Ink::Label)), "a building's description in the label font");
        }
        let castle = g.world.locations.iter().find(|l| l.name == "Замок Бонитур").unwrap();
        let t = location_tooltip(&g, castle);
        assert!(!t.troops.is_empty() && t.garrison.is_some());
    }

    /// The tooltips' fonts by style (0x4ca9f0, 0x4cb18c; loaded at 0x4e5c9c and 0x4dbc26):
    /// in the red one, as the player's screenshot of the original shows, labels and the
    /// description in the orange SanSerif, names in the orange Benguiat; the title yellow in
    /// all; an army's description orange in all.
    #[test]
    fn the_tooltip_fonts_follow_the_style() {
        let rgb = |c: Color| [c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8);
        assert_eq!(rgb(TIP_TITLE), [255, 255, 155]);
        assert_eq!(rgb(tip_color(TipStyle::Hostile, Ink::Label)), [235, 165, 75]);
        assert_eq!(rgb(tip_color(TipStyle::Hostile, Ink::Name)), [255, 175, 85]);
        assert_eq!(rgb(tip_color(TipStyle::Normal, Ink::Label)), [155, 255, 155]);
        assert_eq!(rgb(tip_color(TipStyle::Normal, Ink::Name)), [155, 255, 155]);
        assert_eq!(rgb(tip_color(TipStyle::Normal, Ink::Orange)), [235, 165, 75]);
        assert_eq!(rgb(tip_color(TipStyle::Neutral, Ink::Label)), [155, 155, 155]);
        assert_eq!(rgb(tip_color(TipStyle::Neutral, Ink::Name)), [175, 175, 175]);
        assert_eq!(rgb(tip_color(TipStyle::Normal, Ink::Note)), [255, 175, 85]);
    }

    /// The original fills `#HERONAME` in a building's and an army's description at map load
    /// (0x4b2930, 0x4b2a46): the tooltips show the hero's name, never the escape.
    #[test]
    fn tooltips_fill_the_hero_name_as_the_original_does() {
        use std::sync::Arc;
        use razdor::rules::content::Content;
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.set_hero_name("Ivo");
        g.world.locations[0].description = "Принадлежит #HERONAME.".into();
        let t = location_tooltip(&g, &g.world.locations[0]);
        assert!(t.text.iter().any(|(s, _)| s == "Принадлежит Ivo."), "the building's description");
        assert!(!t.text.iter().any(|(s, _)| s.contains("#HERONAME")));
        let a = g.world.armies.len() - 1;
        g.world.armies[a].description = "Армия #HERONAME.".into();
        let t = army_tooltip(&g, &g.world.armies[a]);
        assert!(t.text.iter().any(|(s, _)| s == "Армия Ivo."), "the army's description");
        assert!(!t.text.iter().any(|(s, _)| s.contains("#HERONAME")));
    }

    /// An army's wounds line counts every wound, battle ones included (Razdor's line), so it is
    /// not called magic in the catalog either.
    #[test]
    fn an_army_s_wounds_are_not_called_magic() {
        use std::sync::Arc;
        use razdor::rules::content::Content;
        use razdor::i18n::{fill, tr, tr_in, Lang};
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let a = g.world.armies.len() - 1;
        g.world.armies[a].troops[0].hurt = 118;
        let t = army_tooltip(&g, &g.world.armies[a]);
        let want = fill(tr("Wounded: -{hurt} hits"), &[("hurt", &118)]);
        assert!(t.text.iter().any(|(s, _)| *s == want), "the wounds line: {want}");
        assert!(!tr_in(Lang::Ru, "Wounded: -{hurt} hits").contains("магии"));
    }

    /// An army's leader is named only when it has more than one unit (0x4ca9f0).
    #[test]
    fn a_lone_unit_s_tooltip_names_no_leader() {
        assert!(!tip_shows_leader("Бран", 1));
        assert!(tip_shows_leader("Бран", 2));
        assert!(!tip_shows_leader("", 5));
    }

    /// The tooltips' heights (0x4ca9f0, 0x4cb18c) with the 2×6 cells (104 px high).
    #[test]
    fn the_tooltip_heights_are_the_original_s() {
        let grid = tip_grid_h(2);
        assert_eq!(grid, 104.0);
        assert_eq!(tip_height(true, 0.0, grid, None), 151.0, "an army without text: 0x97");
        assert_eq!(tip_height(true, 30.0, grid, None), 30.0 + 159.0, "with text: + 0x9f");
        assert_eq!(tip_height(true, 400.0, grid, None), 353.0 + 159.0, "its text at most 0x161");
        assert_eq!(tip_height(false, 0.0, 0.0, None), 29.0, "the title strip alone: 0x1d");
        assert_eq!(tip_height(false, 40.0, 0.0, None), 40.0 + 47.0, "+ 0x2f");
        assert_eq!(tip_height(false, 0.0, grid, Some(15.0)), 15.0 + 159.0, "cells only: caption + 0x9f");
        assert_eq!(tip_height(false, 40.0, grid, Some(15.0)), 40.0 + 47.0 + 15.0 + 121.0, "+ caption + 0x79");
        assert_eq!(tip_height(false, 600.0, grid, Some(15.0)), 512.0, "at most 0x200");
    }

    /// A phone's army cells take the text's scale (a player's report 1554: the text was dp-scaled
    /// and the 2×6 pictures stayed small), capped by the room under the text and the box's width.
    /// The desktop and a garrison keep the interface scale, whatever the text's.
    #[test]
    fn a_phone_s_army_cells_take_the_text_s_scale_within_the_box() {
        let (grid_h, grid_w) = (tip_grid_h(2), tip_grid_w(6));
        assert_eq!((grid_h, grid_w), (104.0, 328.0));
        assert_eq!(army_cells_scale(false, 1.0, 2.5, grid_h, grid_w, 1000.0, 900.0), 1.0, "the desktop: the interface scale");
        assert_eq!(army_cells_scale(true, 1.0, 2.5, grid_h, grid_w, 1000.0, 900.0), 2.5, "a phone's army: the text's scale");
        assert_eq!(army_cells_scale(true, 1.0, 2.5, grid_h, grid_w, 150.0, 900.0), 150.0 / 104.0, "capped by the room under the text");
        assert_eq!(army_cells_scale(true, 1.0, 2.5, grid_h, grid_w, 1000.0, 492.0), 1.5, "capped by the box's width");
        assert_eq!(army_cells_scale(true, 1.0, 2.5, grid_h, grid_w, 10.0, 900.0), 1.0, "never under the interface scale");
    }

    /// A castle taken shows the units the player left in it, not the garrison beaten for it.
    #[test]
    fn a_castle_taken_shows_the_units_left_in_it() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap();
        let content = std::sync::Arc::new(razdor::rules::content::Content::from_dt(&dt));
        let g = Game::from_scenario(content, &m.load().unwrap(), HeroClass::Knight);
        let mut castle = g.world.locations.iter().find(|l| l.name == "Замок Бонитур").unwrap().clone();
        assert!(castle.garrison.iter().any(|t| t.alive()));
        castle.cleared = true;
        castle.owner = razdor::rules::world::Owner::Player;
        let left = g.squad.last().unwrap().clone();
        castle.stationed.push(razdor::rules::world::Stationed { unit: left.clone() });
        assert_eq!(garrison_shown(&g, &castle).iter().map(|t| t.unit).collect::<Vec<_>>(), vec![left.def]);
    }

    /// A building with an army standing in it shows that army's panel (0x4cc2a5), the last
    /// in order of those standing there: on «Другой берег» Castle Waston shows von Waston's
    /// troop (and the capital King August once he is in it).
    #[test]
    fn a_building_shows_the_army_standing_in_it() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let content = std::sync::Arc::new(razdor::rules::content::Content::from_dt(&dt));
        let m = dt.maps.iter().find(|m| m.name.starts_with("Другой берег")).unwrap();
        let mut g = Game::from_scenario(content, &m.load().unwrap(), HeroClass::Knight);
        let castle = g.world.locations.iter().position(|l| l.name == "Замок Вастон").unwrap();
        let guard = tooltip_guard(&g, castle).map(|a| a.name.clone());
        assert_eq!(guard.as_deref(), Some("Отряд фон Вастон"));
        // The king walks into his castle: it shows him.
        let lakwood = g.world.locations.iter().position(|l| l.name == "Замок Лаквуд").unwrap();
        assert!(tooltip_guard(&g, lakwood).is_none_or(|a| !a.name.starts_with("Король")), "not in it yet");
        let king = g.world.armies.iter().position(|a| a.name.starts_with("Король Август")).unwrap();
        g.world.armies[king].mind.standing = Some(lakwood);
        assert!(tooltip_guard(&g, lakwood).is_some_and(|a| a.name.starts_with("Король Август")));
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

    /// A castle held by an army names that army's leader from the map's army record
    /// (0x4cb2d3 reads 0x68ed04), also once the army is off the map or beaten.
    #[test]
    fn a_castle_names_its_armys_leader_with_the_army_gone() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let content = std::sync::Arc::new(razdor::rules::content::Content::from_dt(&dt));
        let mut checked = 0;
        for m in dt.maps.iter().filter(|m| m.name.starts_with("РК")) {
            let scenario = m.load().unwrap();
            let mut g = Game::from_scenario(content.clone(), &scenario, HeroClass::Knight);
            let held: Vec<(usize, u8)> = g
                .world
                .locations
                .iter()
                .enumerate()
                .filter(|(_, l)| matches!(l.kind, LocationKind::Castle | LocationKind::Fort))
                .filter_map(|(i, l)| match l.owner {
                    Owner::Army(k) => Some((i, k)),
                    _ => None,
                })
                .collect();
            for (i, k) in held {
                let leader = razdor::dt::dtm::collapse_spaces(&scenario.armies[k as usize - 1].leader_name);
                if leader.trim().is_empty() {
                    continue;
                }
                g.world.armies.clear();
                g.world.inactive.clear();
                assert_eq!(tooltip_owner(&g, &g.world.locations[i]), leader, "{}: {}", m.name, g.world.locations[i].name);
                checked += 1;
            }
        }
        assert!(checked > 0, "no castle held by an army");
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

    /// «Молния»'s layers: `Effect1=P-Flare,120,0,100,1000,65,1200,600` and
    /// `Effect3=S-Lighting,0,135,155,1000,35,1400,0`.
    fn lightning(n: u8) -> FxLayer {
        let line = if n == 1 { "P-Flare,120,0,100,1000,65,1200,600" } else { "S-Lighting,0,135,155,1000,35,1400,0" };
        let parts: Vec<i32> = line.split(',').skip(1).map(|x| x.parse().unwrap()).collect();
        let e = razdor::dt::data::SpellEffect {
            file: line.split(',').next().unwrap().into(),
            rgb: [parts[0], parts[1], parts[2]],
            duration_ms: parts[3],
            y_offset: parts[4],
            scale_milli: parts[5],
            start_ms: parts[6],
        };
        FxLayer::of(n, &e)
    }

    #[test]
    fn a_spell_layer_shows_from_its_start_to_its_end_in_49_steps() {
        let flare = lightning(1);
        assert_eq!(flare.frame(599.0), None);
        assert_eq!(flare.frame(600.0), Some(0));
        // (t − start)·49 div length: frame 49 only at the very end.
        assert_eq!(flare.frame(620.0), Some(0));
        assert_eq!(flare.frame(621.0), Some(1));
        assert_eq!(flare.frame(1599.0), Some(48));
        assert_eq!(flare.frame(1600.0), Some(49));
        assert_eq!(flare.frame(1601.0), None);
        assert_eq!(layers_ms(&[flare, lightning(3)]), 1600.0);
    }

    #[test]
    fn a_spell_layers_quad_hangs_from_the_cells_centre_by_its_y() {
        // 128 px × 1.2 wide, centred; its bottom 65 × 1.2 px below the centre.
        let q = lightning(1).quad();
        assert!((q.x + 76.8).abs() < 1e-4 && (q.w - 153.6).abs() < 1e-4 && (q.h - 153.6).abs() < 1e-4);
        assert!((q.y - (65.0 - 128.0) * 1.2).abs() < 1e-4);
        assert!((q.y + q.h - 78.0).abs() < 1e-4);
        // The bolt: 1.4 times, its bottom 49 px below the centre, its top 130 px above.
        let q = lightning(3).quad();
        assert!((q.y + q.h - 49.0).abs() < 1e-4 && (q.y + 130.2).abs() < 1e-4);
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
        assert_eq!(g.step(vec2(102.0, 101.0), true, |d| d / 32.0), GrabStep::Held);
        assert_eq!(g.step(vec2(102.0, 101.0), false, |d| d / 32.0), GrabStep::Click);
        // Moved past the threshold it drags: the ground follows the mouse, the camera goes
        // the other way, and it stays a drag back at the start.
        let mut g = Grab { start: vec2(100.0, 100.0), from: vec2(10.0, 10.0), moved: false };
        assert_eq!(g.step(vec2(164.0, 100.0), true, |d| d / 32.0), GrabStep::Drag(vec2(8.0, 10.0)));
        assert_eq!(g.step(vec2(100.0, 100.0), true, |d| d / 32.0), GrabStep::Drag(vec2(10.0, 10.0)));
        assert_eq!(g.step(vec2(100.0, 100.0), false, |d| d / 32.0), GrabStep::Dropped, "a drag is no click");
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
        for k in [KeyCode::Equal, KeyCode::Minus, KeyCode::KpAdd, KeyCode::KpSubtract] {
            assert!(!cuts_walk(k), "{k:?}");
        }
        // Q and E turn the 2.5D view only: they leave a walk alone there, and cut it on the flat map.
        set_tilt_view(true);
        assert!(!cuts_walk(KeyCode::Q) && !cuts_walk(KeyCode::E), "2.5D: the turns cut nothing");
        set_tilt_view(false);
        assert!(cuts_walk(KeyCode::Q) && cuts_walk(KeyCode::E), "flat: as any other key");
        // The keys the player has bound to them, as their defaults.
        for a in [Action::Minimap, Action::CentreHero, Action::Music, Action::Help, Action::Language, Action::QuickSave] {
            assert!(!cuts_walk(keymap::key(a)), "{a:?}");
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

/// The 2.5D view's pure parts: the picking at every turn, the sprites' size setting, the easing
/// back to north up, when the compass shows and where it stands.
#[cfg(test)]
mod card_tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).abs().max_element() < 1e-4
    }

    #[test]
    fn card_mesh_places_a_triangle_on_its_sprite() {
        // A one-triangle card in a 10 × 10 sprite at (20, 30) of a 100 × 200 atlas, drawn at zoom 2
        // with the sprite's top-left at (100, 50).
        let card = Card { foot: (0.0, 0.0), verts: vec![(0.0, 0.0), (10.0, 0.0), (0.0, 10.0)], tris: vec![0, 1, 2] };
        let (v, i) = card_mesh(&card, vec2(100.0, 50.0), 2.0, Rect::new(20.0, 30.0, 10.0, 10.0), vec2(100.0, 200.0), WHITE);
        assert_eq!(i, vec![0, 1, 2]);
        let pos: Vec<Vec2> = v.iter().map(|v| vec2(v.position.x, v.position.y)).collect();
        assert!(pos.iter().zip([vec2(100.0, 50.0), vec2(120.0, 50.0), vec2(100.0, 70.0)]).all(|(a, b)| close(*a, b)), "{pos:?}");
        let uv: Vec<Vec2> = v.iter().map(|v| v.uv).collect();
        assert!(uv.iter().zip([vec2(0.2, 0.15), vec2(0.3, 0.15), vec2(0.2, 0.2)]).all(|(a, b)| close(*a, b)), "{uv:?}");
        assert!(v.iter().all(|v| v.color == [255, 255, 255, 255]));
    }

    #[test]
    fn card_foot_is_on_the_sprite_at_its_zoom() {
        let card = Card { foot: (4.0, 8.0), verts: vec![], tris: vec![] };
        assert!(close(card_foot(vec2(10.0, 20.0), 0.5, &card), vec2(12.0, 24.0)));
    }
}

#[cfg(test)]
mod tilt_tests {
    use super::*;
    use razdor::rules::content::{Content, HeroClass};
    use std::sync::Arc;

    /// A camera over the middle of `game`'s map, at `pitch` and `yaw` (tilted), or flat.
    fn camera(game: &Game, tilt: Option<(f32, f32)>) -> Camera {
        let view = Rect::new(0.0, 0.0, 1280.0, 760.0);
        let scale = PX;
        let grid = game.world.map.grid;
        let at = (game.world.map.w as f32 / 2.0, game.world.map.h as f32 / 2.0 * grid.row_height());
        let origin = vec2(at.0 * scale, at.1 * scale) - vec2(view.w, view.h) / 2.0;
        let mut cam = Camera { origin, view, draw: view, scale, grid, tilt: None };
        if let Some((pitch, yaw)) = tilt {
            let tilt = view25::TiltCamera::new(Vec2::from(cam.centre()), scale, view, pitch, yaw);
            cam.tilt = Some(Tilt { cam: tilt, plan: tilt.ground_footprint(view, view), cull: tilt.ground_footprint(view, view) });
        }
        cam
    }

    /// Every cell near the middle of the map, and points a quarter cell off its centre, pick the
    /// cell they are on, at every turn and pitch of the 2.5D view and in the flat one: the
    /// pointer's cell is the one drawn under it.
    #[test]
    fn the_cell_under_a_pixel_is_the_cell_drawn_there_at_every_turn() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let (cx, cy) = (m.w / 2, m.h / 2);
        let rh = m.grid.row_height();
        let mut views = vec![("flat".to_string(), camera(&game, None))];
        for yaw in [0.0, 45.0, 90.0, 135.0, 180.0, -135.0, -90.0, -45.0] {
            for pitch in [view25::PITCH_MIN, view25::PITCH_DEFAULT, view25::PITCH_MAX] {
                views.push((format!("yaw {yaw} pitch {pitch}"), camera(&game, Some((pitch, yaw)))));
            }
        }
        for (name, cam) in &views {
            for dc in -3..=3 {
                for dr in -3..=3 {
                    let t = (cx + dc, cy + dr);
                    let (x, y) = m.grid.center(t);
                    for (ox, oy) in [(0.0, 0.0), (0.25, 0.0), (-0.25, 0.0), (0.0, 0.25), (0.0, -0.25)] {
                        let at = cam.screen_at((x + ox, y + oy * rh));
                        assert_eq!(cam.tile_under_pixel(at), Some(t), "{name}: the cell {t:?} at {at:?}");
                    }
                }
            }
        }
    }

    /// The first-person near fade applies to the first-person camera only: the bent first-person tilt has it,
    /// the eyes camera has its own, and photo mode's free camera (no bend) has none, whatever the view setting.
    #[test]
    fn only_the_bent_first_person_camera_takes_the_first_person_near_fade() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let fp = first_person_camera(&game, 0.0, false);
        assert!(bent_first_person_cam(&fp), "the first-person camera");
        assert!(!bent_first_person_cam(&eyes_camera(&game, 0.0, false)), "the eyes camera has its own fade");
        let view = Rect::new(0.0, 0.0, 1280.0, 760.0);
        let mut photo = fp;
        photo.tilt = Some(photo_tilt(Vec2::from(photo.centre()), PX, view, view, 30.0, 0.0));
        assert!(!bent_first_person_cam(&photo), "photo mode's free camera has no bend");
    }

    /// The 2.5D view's sprites never fade by the hero's distance, even with it set for the pass, at any yaw
    /// (only the first-person view keys on it). The tests run with `RAZDOR_VIEW` unset, as a player's game does.
    #[test]
    fn the_25d_sprites_do_not_fade_by_the_heros_distance() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        set_tilt_view(true);
        set_first_person(false);
        assert!(!first_person());
        // The pass's hero distance is set, as `draw_world_in` sets it in the first-person view.
        NEAR_LIMIT.with(|l| l.set(12.0));
        for yaw in [0.0, 90.0, 180.0, 270.0] {
            let cam = camera(&game, Some((view25::PITCH_DEFAULT, yaw)));
            assert_eq!(near_fade_limit(&cam), 0.0, "yaw {yaw}: the 2.5D sprites do not fade by the hero's distance");
        }
        NEAR_LIMIT.with(|l| l.set(f32::INFINITY));
        set_tilt_view(false);
    }

    /// The first-person near fade (the owner's report, 2026-11-10: every tree and building between the camera
    /// and the hero vanished): only a sprite that covers him, in front of him, fades. The rest stay whole; a
    /// covering one is gone in front of him and whole behind him, fading across his depth in between.
    #[test]
    fn the_first_person_near_fade_takes_only_the_sprites_that_cover_the_hero() {
        let ramp = 0.3;
        assert_eq!(first_person_fade(None, ramp), 1.0, "a sprite that covers no part of him stays whole, however near");
        assert_eq!(first_person_fade(Some(-1.0), ramp), 1.0, "a covering sprite behind him is whole");
        let mid = first_person_fade(Some(0.0), ramp);
        assert!(mid > 0.0 && mid < 1.0, "a covering sprite at his depth fades across it: {mid}");
        assert_eq!(first_person_fade(Some(0.3), ramp), 0.0, "a covering sprite a ramp in front of him is gone");
        assert_eq!(first_person_fade(Some(4.0), ramp), 0.0, "a covering sprite well in front of him is gone");
    }

    /// The window's overlap test: rectangles overlap when they share a pixel, and a margin widens the second
    /// one on every side.
    #[test]
    fn two_screen_rectangles_overlap_with_a_margin_about_the_second() {
        let hero = Rect::new(100.0, 100.0, 40.0, 60.0);
        assert!(overlaps(Rect::new(120.0, 120.0, 10.0, 10.0), hero, 0.0), "inside");
        assert!(overlaps(Rect::new(90.0, 90.0, 20.0, 20.0), hero, 0.0), "the corner");
        assert!(!overlaps(Rect::new(0.0, 0.0, 50.0, 50.0), hero, 0.0));
        assert!(!overlaps(Rect::new(141.0, 100.0, 10.0, 10.0), hero, 0.0), "beside him, one pixel clear");
        assert!(overlaps(Rect::new(141.0, 100.0, 10.0, 10.0), hero, 4.0), "the margin reaches it");
        assert!(!overlaps(Rect::new(100.0, 170.0, 10.0, 10.0), hero, 4.0), "below him, clear of the margin");
    }

    /// The covering test on the window, with the real projection: a sprite standing on the hero's cell covers
    /// his drawn rectangle; one standing six cells east, clear of it, does not. Nothing covers him when he is
    /// not drawn.
    #[test]
    fn a_sprite_on_the_heros_cell_covers_him_on_the_window_and_one_far_off_does_not() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let (cx, cy) = (m.w / 2, m.h / 2);
        let cam = first_person_camera(&game, 0.0, false);
        let tilt = cam.tilt.expect("the first-person tilt");
        let hero_at = cam.screen_at(m.grid.center((cx, cy)));
        let (fx, fy) = m.grid.center((cx + 6, cy));
        let far_at = cam.screen_at((fx, fy));
        assert!(cam.view.contains(hero_at) && cam.view.contains(far_at), "both in the frame: {hero_at:?} {far_at:?}");
        // His drawn rectangle: 40 by 60 px, its foot at his cell.
        HERO_ON_WINDOW.with(|h| h.set(Some(HeroOnWindow { rect: Rect::new(hero_at.x - 20.0, hero_at.y - 60.0, 40.0, 60.0), depth: 10.0 })));
        // A picture of two world units about the ground point of a cell (a model that lays it on the ground).
        let picture = Rect::new(-1.0, -1.0, 2.0, 2.0);
        let on_hero = Mat4::from_translation(vec3(m.grid.center((cx, cy)).0, 0.0, m.grid.center((cx, cy)).1));
        let on_far = Mat4::from_translation(vec3(fx, 0.0, fy));
        assert_eq!(covering_gap(&cam, &tilt, on_hero, picture, 4.0), Some(6.0), "a sprite on his cell covers him, six in front of him");
        assert_eq!(covering_gap(&cam, &tilt, on_far, picture, 4.0), None, "a sprite six cells east does not");
        HERO_ON_WINDOW.with(|h| h.set(None));
        assert_eq!(covering_gap(&cam, &tilt, on_hero, picture, 4.0), None, "nothing covers an undrawn hero");
    }

    /// The first-person camera over the middle of `game`'s map, built as [`Camera::looking_in`]
    /// builds it (the bend and the footprints included), at `yaw`, with the far ground's horizon or not.
    fn first_person_camera(game: &Game, yaw: f32, horizon: bool) -> Camera {
        let view = Rect::new(0.0, 0.0, 1280.0, 760.0);
        let scale = PX;
        let grid = game.world.map.grid;
        let at = (game.world.map.w as f32 / 2.0, game.world.map.h as f32 / 2.0 * grid.row_height());
        let origin = vec2(at.0 * scale, at.1 * scale) - vec2(view.w, view.h) / 2.0;
        let mut cam = Camera { origin, view, draw: view, scale, grid, tilt: None };
        cam.tilt = Some(first_person_tilt(Vec2::from(cam.centre()), scale, view, view, yaw, horizon));
        cam
    }

    /// The hero's cell is the one under the pixel the hero is drawn at, in both bends and at every
    /// yaw: the pointer at his feet picks him, and so do the cells about him (the first-person
    /// view's click test; the bent sheet is picked by its inverse).
    #[test]
    fn a_click_on_the_hero_in_the_first_person_view_picks_his_cell_at_both_bends() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let (cx, cy) = (m.w / 2, m.h / 2);
        for horizon in [false, true] {
            for yaw in [0.0, 45.0, 90.0, 180.0, -135.0] {
                let cam = first_person_camera(&game, yaw, horizon);
                let hero = (cx, cy);
                let (x, y) = m.grid.center(hero);
                let at = cam.screen_at((x, y));
                assert!(at.is_finite() && cam.view.contains(at), "horizon {horizon} yaw {yaw}: the hero at {at:?}");
                assert_eq!(cam.tile_under_pixel(at), Some(hero), "horizon {horizon} yaw {yaw}: the hero's pixel");
                for (dc, dr) in [(-1, 0), (0, -1), (1, 1), (0, 2)] {
                    let t = (cx + dc, cy + dr);
                    let (x, y) = m.grid.center(t);
                    let at = cam.screen_at((x, y));
                    if at.is_finite() && cam.view.contains(at) {
                        assert_eq!(cam.tile_under_pixel(at), Some(t), "horizon {horizon} yaw {yaw}: the cell {t:?} at {at:?}");
                    }
                }
            }
        }
    }

    /// The eyes camera over the middle of `game`'s map at `yaw`, built as `looking_in` builds it.
    fn eyes_camera(game: &Game, yaw: f32, horizon: bool) -> Camera {
        let view = Rect::new(0.0, 0.0, 1280.0, 760.0);
        let scale = PX;
        let grid = game.world.map.grid;
        let at = (game.world.map.w as f32 / 2.0, game.world.map.h as f32 / 2.0 * grid.row_height());
        let origin = vec2(at.0 * scale, at.1 * scale) - vec2(view.w, view.h) / 2.0;
        let mut cam = Camera { origin, view, draw: view, scale, grid, tilt: None };
        cam.tilt = Some(eyes_tilt(vec2(at.0, at.1), scale, view, view, yaw, horizon, None));
        cam
    }

    /// The cells about the eyes, on the flat ground and on the bent sheet, at both bends and several
    /// yaws: each cell's centre is projected where it stands and the pixel picks that cell back.
    #[test]
    fn a_click_on_the_ground_in_the_eyes_view_picks_its_cell_at_both_bends() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let (cx, cy) = (m.w / 2, m.h / 2);
        let mut checked = 0;
        for horizon in [false, true] {
            for yaw in [0.0, 45.0, 90.0, 180.0, -135.0] {
                let cam = eyes_camera(&game, yaw, horizon);
                let tilt = cam.tilt.expect("the eyes tilt");
                let bend = tilt.cam.bend.expect("the eyes' bend");
                for dc in -6..=6 {
                    for dr in -6..=6 {
                        let t = (cx + dc, cy + dr);
                        let g = Vec2::from(m.grid.center(t));
                        if bend.hidden(g) {
                            continue;
                        }
                        let Some(px) = tilt.cam.project(bend.point(g), cam.view) else { continue };
                        if !cam.view.contains(px) {
                            continue;
                        }
                        assert_eq!(cam.tile_under_pixel(px), Some(t), "horizon {horizon} yaw {yaw}: the cell {t:?} at {px:?}");
                        checked += 1;
                    }
                }
            }
        }
        assert!(checked > 50, "only {checked} cells checked");
    }

    /// The sprites' size is the setting: 1.5 by default, a value outside 0.5..=3 falls back to it,
    /// and the settings button cycles 1× → 1.5× → 2× → 1×.
    #[test]
    fn the_sprite_size_is_the_setting_and_the_button_cycles_it() {
        assert_eq!(next_sprite_size(1.0), 1.5);
        assert_eq!(next_sprite_size(1.5), 2.0);
        assert_eq!(next_sprite_size(2.0), 1.0);
        assert_eq!(next_sprite_size(1.3), 1.0, "a size not in the list goes to 1×");
        assert_eq!(next_sprite_size(f32::NAN), 1.0);
        set_sprite_size(2.0);
        assert_eq!(f32::from_bits(SPRITE_VIEW.load(std::sync::atomic::Ordering::Relaxed)), 2.0);
        set_sprite_size(9.0);
        assert_eq!(f32::from_bits(SPRITE_VIEW.load(std::sync::atomic::Ordering::Relaxed)), 3.0);
        set_sprite_size(f32::NAN);
        assert_eq!(f32::from_bits(SPRITE_VIEW.load(std::sync::atomic::Ordering::Relaxed)), DEFAULT_SPRITE_SIZE);
    }

    /// The view eases back to north up the short way round, snapping onto it within 0.05°, and
    /// stays put when no time passes or it is there already.
    #[test]
    fn the_view_eases_back_to_north_up_the_short_way() {
        assert_eq!(ease_yaw(30.0, 30.0, 16), 30.0);
        assert_eq!(ease_yaw(10.0, 0.0, 0), 10.0, "no time, no move");
        // From 170° to -170° is 20° the short way, through 180°: it goes up past 170°.
        assert!(ease_yaw(170.0, -170.0, 16) > 170.0, "the short way round");
        let mut s = 90.0;
        for _ in 0..600 {
            let next = ease_yaw(s, 0.0, 16);
            assert!(wrap_yaw(next).abs() <= wrap_yaw(s).abs() + 1e-3, "it never turns away: {s} -> {next}");
            s = next;
        }
        assert_eq!(s, 0.0, "snapped onto north up");
        assert_eq!(ease_yaw(0.01, 0.0, 16), 0.0, "within 0.05° it snaps");
    }

    /// The compass shows only in the 2.5D view, while the view is turned away from north up (by a
    /// turn, or as it eases back), and not once it is back.
    #[test]
    fn the_compass_shows_when_the_view_is_turned_away_and_hides_when_it_is_back() {
        assert!(!compass_shows(false, 45.0, 45.0), "the flat view has no turn");
        assert!(!compass_shows(true, 0.0, 0.0), "north up: no compass");
        assert!(compass_shows(true, 0.0, 45.0), "turned: the compass shows");
        assert!(compass_shows(true, 45.0, 0.0), "easing back: it still shows");
        assert!(!compass_shows(true, 0.0, 360.0), "a full turn is north up");
        assert!(compass_shows(true, 180.0, 180.0), "due south is turned away");
    }

    /// The compass beside the bottom bar, the first place clear of the minimap; never over the bar,
    /// and on a phone (the bar across the screen's width) above it or in the top corner.
    #[test]
    fn the_compass_stands_beside_the_bar_and_keeps_clear_of_the_minimap() {
        let screen = vec2(1280.0, 800.0);
        let bar = Rect::new(160.0, 740.0, 960.0, 60.0);
        assert_eq!(compass_place(screen, bar, 40.0, &[]), Some(Rect::new(1128.0, 750.0, 40.0, 40.0)), "right of the bar");
        let minimap = Rect::new(1100.0, 700.0, 180.0, 100.0);
        assert_eq!(compass_place(screen, bar, 40.0, &[minimap]), Some(Rect::new(112.0, 750.0, 40.0, 40.0)), "left of the bar when the right is taken");
        // A phone held upright: the bar spans the screen, so the places beside it are off screen.
        let phone = vec2(1080.0, 2400.0);
        let phone_bar = Rect::new(0.0, 2300.0, 1080.0, 100.0);
        let above_right = compass_place(phone, phone_bar, 132.0, &[]).unwrap();
        assert_eq!(above_right.x, 948.0, "above the bar's right end");
        assert!(above_right.y + above_right.h <= phone_bar.y, "above the bar, not over it");
        let both_bottom = [Rect::new(900.0, 2100.0, 180.0, 200.0), Rect::new(0.0, 2100.0, 200.0, 200.0)];
        let top = compass_place(phone, phone_bar, 132.0, &both_bottom).unwrap();
        assert_eq!(top.y, 132.0 * 0.2, "the top right corner when both places above the bar are taken");
        assert!((top.x + top.w - (1080.0 - 132.0 * 0.2)).abs() < 1e-3, "in the top right corner: {top:?}");
        // Every place is clear of its bar's rectangle.
        let clear = |r: Rect, b: Rect| r.y + r.h <= b.y || r.y >= b.y + b.h || r.x >= b.x + b.w || r.x + r.w <= b.x;
        assert!(clear(compass_place(screen, bar, 40.0, &[]).unwrap(), bar));
        assert!(clear(above_right, phone_bar) && clear(top, phone_bar));
        assert_eq!(compass_place(screen, bar, 40.0, &[Rect::new(0.0, 0.0, 1280.0, 800.0)]), None, "no clear place: no compass");
    }

    /// A building stands on the centre of its footprint, not on a corner: its foot is the mean of
    /// its cells' centres, at every turn (a billboard turns about its foot).
    #[test]
    fn a_building_stands_on_the_centre_of_its_footprint() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let mut l = game.world.locations[0].clone();
        l.anchor = (m.w / 2, m.h / 2);
        l.size = (2, 2);
        let cells: Vec<Tile> = l.cells().collect();
        assert_eq!(cells.len(), 4);
        for cam in [camera(&game, None), camera(&game, Some((55.0, 0.0))), camera(&game, Some((55.0, 135.0)))] {
            let (sx, sy) = cells.iter().fold((0.0, 0.0), |(x, y), &t| {
                let (cx, cy) = m.grid.center(t);
                (x + cx, y + cy)
            });
            let n = cells.len() as f32;
            let want = cam.to_screen((sx / n, sy / n));
            let got = building_foot(&l, &cam);
            assert!((got - want).length() < 1e-3, "foot {got:?}, want {want:?}");
        }
    }

    /// A plant stands on its cell's centre moved by its jitter, as its picture is, and grows about
    /// that foot (its base is its foot).
    #[test]
    fn a_plant_stands_on_its_cell_moved_by_its_jitter() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let t = (m.w / 2 + 1, m.h / 2 - 1);
        let o = Decoration { tile: t, class: object_class::TREES, sprite: 0 };
        let (dx, dy) = razdor::rules::rng::plant_offset(t.0, t.1, u16::from(o.class) << 8);
        for cam in [camera(&game, None), camera(&game, Some((55.0, 30.0)))] {
            let k = cam.scale / PX;
            let want = cam.to_screen(m.grid.center(t)) + vec2(dx as f32, dy as f32) * k;
            assert!((object_foot(&o, &cam) - want).length() < 1e-3, "foot {:?} want {want:?}", object_foot(&o, &cam));
        }
    }

    /// A hill or mountain stands on the centre of the cells it covers, `object_side` square up and
    /// left of its own cell, not on that cell.
    #[test]
    fn a_hill_stands_on_the_centre_of_the_cells_it_covers() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let t = (m.w / 2, m.h / 2);
        for (sprite, side) in [(10u8, 1.0f32), (20, 2.0), (50, 5.0)] {
            let o = Decoration { tile: t, class: 1, sprite };
            let cam = camera(&game, None);
            let f = side as i32;
            let cells: Vec<Vec2> = (0..f).flat_map(|j| (0..f).map(move |i| (t.0 - i, t.1 - j))).map(|c| cam.cell_centre(c)).collect();
            let want = cells.iter().fold(Vec2::ZERO, |s, &c| s + c) / cells.len() as f32;
            assert!((object_foot(&o, &cam) - want).length() < 1e-3, "side {side}: foot {:?} want {want:?}", object_foot(&o, &cam));
        }
    }

    /// The turning drift of every standing sprite's ground centre, at every turn of the view, for
    /// every decoration class and sprite and every building picture the install has (and synthetic
    /// ones without an install: each class with sides 1–6, every footprint size up to 9×9). The
    /// ground centre is a hill's covered cells, a plant's cell moved by its jitter, a building's
    /// footprint centre, anything else its cell's centre. The picture's point there must stay on
    /// that ground point (under 2 px) at yaws 0..315 by 45, with the foot and the base both there.
    #[test]
    fn every_standing_sprite_keeps_its_ground_centre_at_every_turn() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let (ax, ay) = (m.w / 2, m.h / 2);
        let mut decos: Vec<Decoration> = Vec::new();
        let mut pictures: Vec<(u8, u8)> = Vec::new();
        if let Ok(install) = razdor::dt::install::DtInstall::from_env() {
            if let Ok(objs) = install.map_objects() {
                for s in &objs.sprites {
                    if s.section == 0 {
                        decos.push(Decoration { tile: (ax, ay), class: s.cat as u8, sprite: s.idx as u8 });
                    } else if s.section == 1 && !matches!(s.cat, 13 | 14) {
                        // Bridges lie on the ground (their own pass), not standing.
                        pictures.push((s.cat as u8, s.idx as u8));
                    }
                }
            }
        } else {
            eprintln!("RAZDOR_DT_DIR unset: the synthetic cases only");
        }
        for class in [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12] {
            for side in 1..=6u8 {
                decos.push(Decoration { tile: (ax, ay), class, sprite: side * 10 });
            }
            for sprite in [0u8, 1, 5, 9] {
                decos.push(Decoration { tile: (ax, ay), class, sprite });
            }
        }
        // Buildings: every footprint of up to 9×9 (wide ones too), for every picture.
        let sizes: Vec<(i32, i32)> = (1..=9).flat_map(|w| (1..=9).map(move |h| (w, h))).collect();
        let mut turns = 0;
        let mut worst: std::collections::BTreeMap<String, f32> = Default::default();
        let mut check = |kind: String, name: String, foot: Vec2, base: Vec2, truth: Vec2, cam: &Camera| {
            let d = drift(cam, foot, base, truth);
            assert!((foot - truth).length() < 1e-2, "{name}: foot {foot:?} is not its ground centre {truth:?}");
            let w = worst.entry(kind).or_insert(0.0);
            *w = w.max(d);
            assert!(d < 2.0, "{name}: drift {d:.2} px at a turn");
            turns += 1;
        };
        for o in &decos {
            for i in 0..8 {
                let cam = camera(&game, Some((view25::PITCH_DEFAULT, i as f32 * 45.0)));
                let k = cam.scale / PX;
                let truth = match o.class {
                    1..=8 => {
                        // A hill of side 0 (no such sprite in the data) stands on its own cell.
                        let f = razdor::rules::map::object_side(o.class, o.sprite);
                        let cells: Vec<Vec2> = (0..f).flat_map(|j| (0..f).map(move |i| (o.tile.0 - i, o.tile.1 - j))).map(|c| cam.cell_centre(c)).collect();
                        if cells.is_empty() { cam.cell_centre(o.tile) } else { cells.iter().fold(Vec2::ZERO, |s, &c| s + c) / cells.len() as f32 }
                    }
                    9..=11 => {
                        let (dx, dy) = razdor::rules::rng::plant_offset(o.tile.0, o.tile.1, u16::from(o.class) << 8 | u16::from(o.sprite));
                        cam.cell_centre(o.tile) + vec2(dx as f32, dy as f32) * k
                    }
                    _ => cam.cell_centre(o.tile),
                };
                let kind = match o.class {
                    1..=8 => format!("hill class {} side {}", o.class, razdor::rules::map::object_side(o.class, o.sprite)),
                    _ => format!("decoration class {}", o.class),
                };
                check(kind, format!("decoration class {} sprite {} yaw {}", o.class, o.sprite, i * 45), object_foot(o, &cam), object_foot(o, &cam), truth, &cam);
            }
        }
        for &(t, v) in &pictures {
            for &(sx, sy) in &sizes {
                let l = Location { anchor: (ax, ay), size: (sx, sy), picture: (t, v), ..game.world.locations[0].clone() };
                for i in 0..8 {
                    let cam = camera(&game, Some((view25::PITCH_DEFAULT, i as f32 * 45.0)));
                    let rows = sy + i32::from(sx > sy);
                    let cells: Vec<Vec2> = (0..rows).flat_map(|j| (0..sx).map(move |i| (ax - i, ay - j))).map(|c| cam.cell_centre(c)).collect();
                    let truth = cells.iter().fold(Vec2::ZERO, |s, &c| s + c) / cells.len() as f32;
                    check(format!("building picture {t}/{v}"), format!("building picture {t}/{v} size {sx}x{sy} yaw {}", i * 45), building_foot(&l, &cam), building_foot(&l, &cam), truth, &cam);
                }
            }
        }
        for (kind, d) in &worst {
            eprintln!("DRIFT {kind}: worst {d:.3} px");
        }
        eprintln!("standing sprites: {turns} turns checked");
    }

    /// The drift in px of a standing sprite's ground point `point` (2D): the screen position of the
    /// picture's point at `point` (grown at the sprites' size about `foot`, `base`) against the
    /// screen position of the world ground under `point`.
    fn drift(cam: &Camera, foot: Vec2, base: Vec2, point: Vec2) -> f32 {
        let t = cam.tilt.expect("a 2.5D camera");
        let m = view25::billboard_model_about(cam.origin, foot, base, cam.scale, t.cam.pitch_deg, t.cam.yaw_deg, DEFAULT_SPRITE_SIZE);
        let on_picture = t.cam.project(m.transform_point3(vec3(point.x, point.y, 0.0)), cam.view);
        let on_ground = t.cam.project(vec3((point.x + cam.origin.x) / cam.scale, 0.0, (point.y + cam.origin.y) / cam.scale), cam.view);
        match (on_picture, on_ground) {
            (Some(a), Some(b)) => (a - b).length(),
            _ => f32::INFINITY,
        }
    }

    /// Turned 180°, a tree in the row just north of a 2×2 building (nearer the eye) is drawn
    /// after the building, and one just south of it before: the sort uses the foot, so a tree
    /// no longer draws over a building it stands behind. Every turn of the sweep must build
    /// without a panic, and the 180° case is checked.
    #[test]
    fn a_tree_beside_a_building_sorts_by_depth_when_turned_about() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let m = &game.world.map;
        let (ax, ay) = (m.w / 2, m.h / 2);
        let mut l = game.world.locations[0].clone();
        l.anchor = (ax, ay);
        l.size = (2, 2);
        for yaw in [0.0, 45.0, 90.0, 135.0, 180.0, -90.0] {
            let cam = camera(&game, Some((55.0, yaw)));
            let building = sprite_key(&cam, first_footprint_cell(&l), building_foot(&l, &cam), LAYER_BUILDING);
            let tree = |tile: Tile| {
                let o = Decoration { tile, class: object_class::TREES, sprite: 0 };
                sprite_key(&cam, o.tile, object_foot(&o, &cam), object_layer(o.class))
            };
            // The footprint is the rows ay-1 and ay (`Location::cells`): a tree in row ay-2 is
            // just north of it, one in row ay+1 just south.
            let north = tree((ax - 1, ay - 2));
            let south = tree((ax - 1, ay + 1));
            if yaw == 180.0 {
                assert!(north > building, "turned 180: the tree north of the building is nearer, drawn later");
                assert!(south < building, "turned 180: the tree south of the building is farther, drawn earlier");
            }
        }
    }

    /// The visibility is 1 less the darkness the ground shows, eased by a smoothstep from 0.15 to
    /// 0.85 and clamped to 0 to 1.
    #[test]
    fn a_sprite_fades_with_the_darkness_of_its_ground() {
        assert_eq!(sprite_visibility(0.0), 1.0, "clear ground: plain");
        assert_eq!(sprite_visibility(1.0), 0.0, "deep in the dark: gone");
        assert_eq!(sprite_visibility(0.5), 0.5, "half dark: half there");
        assert!(sprite_visibility(0.25) > sprite_visibility(0.4), "darker, less visible");
        assert_eq!(sprite_visibility(1.5), 0.0, "clamped above");
        assert_eq!(sprite_visibility(-0.5), 1.0, "clamped below");
    }

    /// Plain unless the 2.5D view's standing sprite sets it: a flat camera's sprite is drawn as it is.
    #[test]
    fn the_flat_view_never_fades_a_sprite() {
        assert_eq!(faded(WHITE).a, 1.0, "no sprite drawn: plain");
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let flat = camera(&game, None);
        let mut got = None;
        standing(&flat, vec2(10.0, 10.0), vec2(10.0, 10.0), 0.25, Rect::new(0.0, 0.0, 1.0, 1.0), || got = Some(faded(WHITE)));
        assert_eq!(got.map(|c| (c.r, c.g, c.b, c.a)), Some((1.0, 1.0, 1.0, 1.0)), "a flat sprite is never faded");
    }

    /// A sprite's shade is read from the fog on its own cell, as the ground shows it this frame:
    /// clear where it is explored, dark where it is not, and dark where an event's reveal still
    /// holds its cell unshown.
    #[test]
    fn a_sprite_on_an_unexplored_cell_is_dark_and_one_on_an_explored_cell_is_not() {
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let cam = camera(&game, Some((45.0, 0.0)));
        let mut fog = razdor::rules::fog::Fog::new(40, 30);
        fog.reveal(20, 15, 4);
        let (lit, dark) = ((20, 15), (2, 2));
        assert!(fog.explored(lit) && !fog.explored(dark), "the test fog: lit inside, dark far away");
        let foot = |t: Tile| cam.cell_centre(t);
        let shade = Shade { fog: &fog, veil: NO_VEIL };
        minimap::snap_fog();
        minimap::track(&fog, &[], lit, std::iter::empty(), 0.0);
        assert_eq!(shade.at(&cam, foot(lit)), 1.0, "explored and clear");
        assert_eq!(shade.at(&cam, foot(dark)), 0.0, "unexplored");
        // A reveal's unshown block (its cells dark, the feather at its rim), taken at once.
        let block: Vec<Tile> = (17..=23).flat_map(|x| (12..=18).map(move |y| (x, y))).collect();
        minimap::snap_fog();
        minimap::track(&fog, &block, lit, std::iter::empty(), 0.0);
        assert_eq!(shade.at(&cam, foot(lit)), 0.0, "an unshown cell is dark even when explored");
    }

    /// Visibility 0 draws nothing: a straight-alpha sprite's alpha goes to 0, a premultiplied
    /// one's every channel with it. At 0.25 the two scale as their blend needs.
    #[test]
    fn a_fading_sprite_scales_its_alpha_or_its_premultiplied_colour() {
        SPRITE_VIS.with(|s| s.set(0.25));
        let (straight, premultiplied) = (faded(WHITE), faded_premultiplied(WHITE));
        SPRITE_VIS.with(|s| s.set(0.0));
        let (gone, gone_premultiplied) = (faded(WHITE), faded_premultiplied(WHITE));
        SPRITE_VIS.with(|s| s.set(1.0));
        assert_eq!((straight.r, straight.g, straight.b, straight.a), (1.0, 1.0, 1.0, 0.25), "straight: colour kept, alpha faded");
        assert_eq!((premultiplied.r, premultiplied.g, premultiplied.b, premultiplied.a), (0.25, 0.25, 0.25, 0.25), "premultiplied: all channels faded");
        assert_eq!(gone.a, 0.0, "invisible");
        assert_eq!((gone_premultiplied.r, gone_premultiplied.a), (0.0, 0.0), "invisible premultiplied");
    }
}
