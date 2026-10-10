//! The original's own mouse pointers (interface.md §7.2): the game draws its pointer itself,
//! last in the frame, over everything (0x474a81). Each frame the screens say which shape they
//! want ([`set`]); [`draw`] puts it at the mouse once the frame is drawn.
//!
//! The pictures (LoadMouseCursors 0x4db278): the stills `Graphics/Windows/Cursor-Normal.lit`,
//! `Cursor-Ask.lit` and `Cursor-Denied.lit`, and the 50-frame strips `Clock.ugs`, `House.ugs`,
//! `Swords.ugs` and `Ask.ugs` (0x4db07c). The stills are the arrow's record (0x4db35e: its
//! hotspot 0,0), whose picture the map's hover swaps for the question mark or the "no entry"
//! (0x4cce7c, 0x4cce92); the strips are drawn centred on the mouse: 42 px clock and house
//! at (−21,−21) (0x4db3a8, 0x4db3da), 48 px swords at (−24,−24) (0x4db412), 40 px question
//! mark at (−20,−20) (0x4db44a). Without the install's art the system pointer stays.

use std::cell::Cell;

use macroquad::miniquad::{self, CursorIcon};
use macroquad::prelude::*;

use super::widgets::pointer;

/// A pointer the original draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `Cursor-Normal`, state 0 (0x48eca0).
    Arrow,
    /// `Cursor-Ask`, still: the arrow's state over an unexplored cell (0x4cce7c).
    Ask,
    /// `Cursor-Denied`, still: the arrow's state over an explored cell that is no target
    /// (0x4cce92).
    Denied,
    /// `Clock.ugs`, state 1: busy (0x48ecf0).
    Clock,
    /// `House.ugs`, state 2: over a building (0x48ed14).
    House,
    /// `Swords.ugs`, state 3: over an army, a guard or a hostile garrison (0x48edc8).
    Swords,
    /// `Ask.ugs`, state 4, animated: a friend with a meeting waiting (0x48edc8), ruins with
    /// no army for owner or an obelisk (0x48ed14).
    Query,
    /// None: the right-button tooltip of the map is up (the pointer is frozen and not drawn,
    /// 0x474a81 on 0x4ecd88), or what is held is drawn in its place (an item: 0x4c24f4
    /// makes the item's picture the pointer, record 0xae1da0).
    Hidden,
}

impl Shape {
    /// The original's state 0 (0xae1d7c): the arrow with one of its three pictures. The
    /// map's hover gives it its still (0x4cce5b) only in that state.
    pub fn is_arrow(self) -> bool {
        matches!(self, Shape::Arrow | Shape::Ask | Shape::Denied)
    }
}

/// Where a shape's picture comes from: a still or a 50-frame strip with its frame time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Art {
    Still(&'static str),
    Strip(&'static str, u64),
}

/// The picture and hotspot (pixels of the original, added to the mouse) of `s`.
fn art(s: Shape) -> Option<(Art, (f32, f32))> {
    Some(match s {
        Shape::Arrow => (Art::Still("Cursor-Normal"), (0.0, 0.0)),
        Shape::Ask => (Art::Still("Cursor-Ask"), (0.0, 0.0)),
        Shape::Denied => (Art::Still("Cursor-Denied"), (0.0, 0.0)),
        Shape::Clock => (Art::Strip("Clock.ugs", 30), (-21.0, -21.0)),
        Shape::House => (Art::Strip("House.ugs", 50), (-21.0, -21.0)),
        Shape::Swords => (Art::Strip("Swords.ugs", 30), (-24.0, -24.0)),
        Shape::Query => (Art::Strip("Ask.ugs", 25), (-20.0, -20.0)),
        Shape::Hidden => return None,
    })
}

/// The frame of a 50-frame strip at clock `ms` (Cursor_Animate 0x48eb98: `(ms / t) mod 50`,
/// t = 30 for the clock and the swords, 50 for the house, 25 for the question mark).
fn strip_frame(ms: u64, frame_ms: u64) -> usize {
    ((ms / frame_ms.max(1)) % 50) as usize
}

thread_local! {
    /// The shape the screens asked for this frame.
    static WANT: Cell<Shape> = const { Cell::new(Shape::Arrow) };
    /// A system pointer asked for this frame (Razdor's own drags, the editor).
    static SYSTEM: Cell<Option<CursorIcon>> = const { Cell::new(None) };
    /// What the window shows now: the system pointer (with its icon) or none.
    static SHOWN: Cell<Option<Option<CursorIcon>>> = const { Cell::new(None) };
    /// The window has the keyboard focus, and its input subscriber.
    static FOCUS: Cell<(bool, Option<usize>)> = const { Cell::new((true, None)) };
}

/// A new frame: the arrow unless a screen asks otherwise.
pub fn begin_frame() {
    WANT.with(|w| w.set(Shape::Arrow));
    SYSTEM.with(|s| s.set(None));
}

/// This frame's pointer is `s` (the last call wins).
pub fn set(s: Shape) {
    WANT.with(|w| w.set(s));
}

/// This frame's pointer is the system's `icon` (Razdor's minimap resize and map drag, which
/// the original does not have, and the map editor); it wins over [`set`].
pub fn system(icon: CursorIcon) {
    SYSTEM.with(|s| s.set(Some(icon)));
}

/// Watches the focus: lost (macroquad's "minimized", sent on focus changes), it comes back
/// with "restored" or a key or button pressed in the window.
struct FocusWatch(bool);

impl miniquad::EventHandler for FocusWatch {
    fn update(&mut self) {}
    fn draw(&mut self) {}
    fn window_minimized_event(&mut self) {
        self.0 = false;
    }
    fn window_restored_event(&mut self) {
        self.0 = true;
    }
    fn mouse_button_down_event(&mut self, _: miniquad::MouseButton, _: f32, _: f32) {
        self.0 = true;
    }
    fn key_down_event(&mut self, _: miniquad::KeyCode, _: miniquad::KeyMods, _: bool) {
        self.0 = true;
    }
}

fn focused() -> bool {
    let (was, sub) = FOCUS.with(|f| f.get());
    let sub = sub.unwrap_or_else(macroquad::input::utils::register_input_subscriber);
    let mut watch = FocusWatch(was);
    macroquad::input::utils::repeat_all_miniquad_input(&mut watch, sub);
    FOCUS.with(|f| f.set((watch.0, Some(sub))));
    watch.0
}

/// The window had the keyboard focus when the last frame's pointer was drawn (a gamepad is
/// read only then: it would play a game in the background).
pub fn has_focus() -> bool {
    FOCUS.with(|f| f.get().0)
}

/// The window shows the system pointer `icon`, or none.
fn show_system(icon: Option<CursorIcon>) {
    if SHOWN.with(|s| s.get()) == Some(icon) {
        return;
    }
    SHOWN.with(|s| s.set(Some(icon)));
    match icon {
        Some(i) => {
            miniquad::window::set_mouse_cursor(i);
            show_mouse(true);
        }
        None => show_mouse(false),
    }
}

/// Draws this frame's pointer at the mouse, over everything drawn so far. Out of focus, or
/// without the install's picture, the system pointer shows instead; outside the window
/// nothing is drawn.
pub fn draw() {
    let focus = focused();
    let want = WANT.with(|w| w.get());
    // A gamepad's pointer (`ui::input`): the window's pointer cannot be put there, so where
    // the system's would show Razdor draws a plain arrow of its own.
    let pad = super::input::pad_pointer().filter(|_| focus);
    let system = |icon: CursorIcon| match pad {
        Some(at) => {
            show_system(None);
            if want != Shape::Hidden {
                plain_arrow(at);
            }
        }
        None => show_system(Some(icon)),
    };
    if let Some(icon) = SYSTEM.with(|s| s.get()).or((!focus).then_some(CursorIcon::Default)) {
        system(icon);
        return;
    }
    let Some((what, hot)) = art(want) else {
        show_system(None);
        return;
    };
    let tex = match what {
        Art::Still(name) => super::chrome::win(name),
        Art::Strip(name, t) => super::chrome::animation(&format!("Windows/{name}")).and_then(|f| {
            let ms = (get_time() * 1000.0) as u64;
            f.get(strip_frame(ms, t) % f.len().max(1)).cloned()
        }),
    };
    let Some(tex) = tex else {
        system(CursorIcon::Default);
        return;
    };
    show_system(None);
    let (mx, my) = pointer();
    if mx < 0.0 || my < 0.0 || mx >= screen_width() || my >= screen_height() {
        return;
    }
    // The original's pixels at the interface's scale (the 1024×768 screen against the
    // 960×720 footage of `chrome::k`).
    let px = super::chrome::k() * 0.9375;
    let (x, y) = ((mx + hot.0 * px).round(), (my + hot.1 * px).round());
    let size = vec2(tex.width(), tex.height()) * px;
    draw_texture_ex(&tex, x, y, WHITE, DrawTextureParams { dest_size: Some(size), ..Default::default() });
}

/// Razdor's stand-in for the system arrow at the gamepad's pointer: white, outlined in black,
/// its tip at `at`.
fn plain_arrow(at: (f32, f32)) {
    let k = super::chrome::k();
    let tip = vec2(at.0, at.1).round();
    let (a, b) = (tip + vec2(0.0, 19.0) * k, tip + vec2(13.0, 13.0) * k);
    draw_triangle(tip, a, b, WHITE);
    for (p, q) in [(tip, a), (a, b), (b, tip)] {
        draw_line(p.x, p.y, q.x, q.y, 1.5 * k.max(1.0), BLACK);
    }
}

// ------------------------------------------------------------------------------------------
// The world map's hover (0x4cc148: 0x4ccad1-0x4ccec1)
// ------------------------------------------------------------------------------------------

/// The army byte of the hovered cell (0x4cc01e): none, the hero (1), the hero's parked ship
/// (N + 2) or an army (2..N + 1).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CellArmy {
    #[default]
    None,
    Hero,
    Ship,
    /// An army, with its attitude to the player (+0x16af) and whether a meeting event waits
    /// for it (+0x3826).
    Army { attitude: i8, meeting: bool },
}

/// The building whose footprint covers the hovered cell (0x4cc03e).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildingHover {
    /// Its type, as the map file numbers them (0 palace … 12 ruins, 13 and 14 bridges,
    /// 15 obelisk).
    pub kind: u8,
    /// The armies on the map standing in it (+0x3788), in army order: attitude, meeting
    /// waiting.
    pub standing: Vec<(i8, bool)>,
    /// It has a garrison (building −0x42; for ruins also its list, −0x10).
    pub garrison: bool,
    /// Its attitude to the player (building −0x14).
    pub attitude: i8,
    /// Its owner byte (+0x124) is above the armies' count: no army holds it.
    pub ownerless: bool,
}

/// What the world's hit test (0x4cbf20) finds under the pointer, in the map view.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MapHover {
    /// Over the minimap (−2).
    pub on_minimap: bool,
    /// The cell is unexplored (−3).
    pub unexplored: bool,
    /// The cell is a target a click walks to (−0xe).
    pub valid: bool,
    /// The army byte (−8), read only on an explored cell off the minimap.
    pub army: CellArmy,
    /// The building (−0xc), likewise; none for a bridge while the hero sails (0x4cc108).
    pub building: Option<BuildingHover>,
}

/// Cursor_OverArmy (0x48edc8): an army with a meeting waiting and an attitude of 1 or more
/// gets the question mark, any other the swords.
fn over_army(attitude: i8, meeting: bool) -> Shape {
    if meeting && attitude >= 1 {
        Shape::Query
    } else {
        Shape::Swords
    }
}

/// The guard of a hovered building (0x4ccc8a-0x4ccd46), as the pointer over it: of a
/// village, castle, fort, ruins or bridge the last army standing in it that is ill-disposed
/// (any on a bridge); a castle, fort or ruins with a garrison and an attitude of 0 or less
/// (ruins whatever it is) is guarded by its garrison (the swords); ruins with no garrison
/// have none.
fn building_guard(b: &BuildingHover) -> Option<Shape> {
    let mut guard = None;
    if matches!(b.kind, 2..=4 | 12..=14) {
        guard = b.standing.iter().rev().find(|&&(att, _)| att <= 0 || b.kind >= 13).map(|&(att, meeting)| over_army(att, meeting));
    }
    if matches!(b.kind, 3 | 4 | 12) && b.garrison && (b.attitude <= 0 || b.kind == 12) {
        guard = Some(Shape::Swords);
    }
    if b.kind == 12 && !b.garrison {
        guard = None;
    }
    guard
}

/// The ring the hover puts on a newly entered cell (0x48eeb0 from 0x4ccdf8 and 0x4cce51,
/// size 1), off spell targeting (Razdor has none on the map): with an army or a building
/// in it colour 3 when the building has a guard (0x4ccd37, 0x4ccd88), else colour 2; on an
/// empty cell colour 2 when it is a target, nothing when it is not. The hero's own cell
/// keeps his mark (0x48eee5 refuses colour 2 there, 0x4ccee4 marks him again). The cell's
/// mark before comes back when the pointer enters another one (0x4ccb1a, 0x4ccb39).
pub fn hover_ring(h: &MapHover) -> Option<u8> {
    if h.army == CellArmy::Hero {
        return None;
    }
    if h.army != CellArmy::None || h.building.is_some() {
        return Some(if h.building.as_ref().and_then(building_guard).is_some() { 3 } else { 2 });
    }
    h.valid.then_some(2)
}

/// The pointer after the map's hover of a newly entered cell, from the one before (`prev`):
/// the hover of 0x4ccb85-0x4ccdfd, then the still of 0x4cce5b.
///
/// - Nothing under it: the arrow.
/// - A building: of a village, castle, fort, ruins or bridge the last army on the map
///   standing in it that is ill-disposed (attitude 0 or less; any on a bridge) is its guard
///   (0x4ccc8a-0x4cccf7); a castle, fort or ruins with a garrison and an attitude of 0 or
///   less (ruins whatever it is) counts as guarded by its garrison (0x4cccf9-0x4ccd43);
///   ruins with no garrison have no guard (0x4ccd46). Guarded: as an army (the garrison:
///   the swords); else a bridge gives the arrow, any other building the house, or the
///   question mark for ruins no army holds and for an obelisk (Cursor_OverBuilding
///   0x48ed14), which keeps the house or the question mark already shown.
/// - An army outside a building: [`over_army`].
/// - The hero or his ship: the arrow (0x4ccdc7).
///
/// Then, while it is the arrow: off the minimap a cell that is no target gets the question
/// mark when unexplored, the "no entry" when explored.
pub fn map_pointer(prev: Shape, h: &MapHover) -> Shape {
    let mut s = prev;
    let hovered = h.army != CellArmy::None || h.building.is_some();
    if !hovered {
        s = Shape::Arrow;
    } else {
        match &h.building {
            Some(b) => {
                s = match building_guard(b) {
                    Some(g) => g,
                    None if matches!(b.kind, 13 | 14) => Shape::Arrow,
                    None if matches!(s, Shape::House | Shape::Query) => s,
                    None if (b.kind == 12 && b.ownerless) || b.kind == 15 => Shape::Query,
                    None => Shape::House,
                };
            }
            None => {
                if let CellArmy::Army { attitude, meeting } = h.army {
                    s = over_army(attitude, meeting);
                }
            }
        }
        if matches!(h.army, CellArmy::Hero | CellArmy::Ship) {
            s = Shape::Arrow;
        }
    }
    if s.is_arrow() {
        s = match (h.valid || h.on_minimap, h.unexplored) {
            (true, _) => Shape::Arrow,
            (false, true) => Shape::Ask,
            (false, false) => Shape::Denied,
        };
    }
    s
}

/// The pointer of the battle screen: the clock while the enemy acts (Battle_Step 0x4c57bc:
/// 0x4c5fee on its turn, the arrow at 0x4c5ab0 on the player's), during a pass's pause
/// (0x4afb7d) and the won battle's hold (0x4b0a17); else the arrow.
pub fn battle_pointer(enemy_turn: bool, pausing: bool, won_hold: bool) -> Shape {
    if enemy_turn || pausing || won_hold {
        Shape::Clock
    } else {
        Shape::Arrow
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(valid: bool, unexplored: bool) -> MapHover {
        MapHover { valid, unexplored, ..Default::default() }
    }

    fn building(kind: u8) -> BuildingHover {
        BuildingHover { kind, ..Default::default() }
    }

    fn over(b: BuildingHover) -> MapHover {
        MapHover { valid: true, building: Some(b), ..Default::default() }
    }

    #[test]
    fn strips_turn_at_the_original_rates() {
        // 0x48eb98: (ms / t) mod 50.
        assert_eq!(strip_frame(0, 30), 0);
        assert_eq!(strip_frame(29, 30), 0);
        assert_eq!(strip_frame(30, 30), 1);
        assert_eq!(strip_frame(1500, 30), 0, "50 frames of 30 ms");
        assert_eq!(strip_frame(2499, 50), 49);
        assert_eq!(strip_frame(1249, 25), 49);
        assert_eq!(strip_frame(1250, 25), 0);
    }

    #[test]
    fn hotspots_centre_the_strips_and_keep_the_arrow_tip() {
        // 0x4db35e-0x4db46c: the arrow's record at 0,0; the 42 px clock and house at −21,
        // the 48 px swords at −24, the 40 px question mark at −20.
        assert_eq!(art(Shape::Arrow).map(|a| a.1), Some((0.0, 0.0)));
        assert_eq!(art(Shape::Ask).map(|a| a.1), Some((0.0, 0.0)));
        assert_eq!(art(Shape::Denied).map(|a| a.1), Some((0.0, 0.0)));
        assert_eq!(art(Shape::Clock).map(|a| a.1), Some((-21.0, -21.0)));
        assert_eq!(art(Shape::House).map(|a| a.1), Some((-21.0, -21.0)));
        assert_eq!(art(Shape::Swords).map(|a| a.1), Some((-24.0, -24.0)));
        assert_eq!(art(Shape::Query).map(|a| a.1), Some((-20.0, -20.0)));
        assert_eq!(art(Shape::Hidden), None);
    }

    #[test]
    fn the_installs_strips_have_50_frames_centred_on_their_hotspots() {
        // Skipped without `RAZDOR_DT_DIR`. Sizes only.
        use razdor::dt::{gfx, install};
        let Some(dir) = std::env::var_os(install::ENV_VAR) else { return };
        let dir = std::path::Path::new(&dir);
        for s in [Shape::Arrow, Shape::Ask, Shape::Denied, Shape::Clock, Shape::House, Shape::Swords, Shape::Query] {
            let Some((what, hot)) = art(s) else { panic!("{s:?}") };
            let rel = match what {
                Art::Still(name) => format!("Graphics/Windows/{name}.lit"),
                Art::Strip(name, _) => format!("Graphics/Windows/{name}"),
            };
            let frames = gfx::decode_file(&install::find_path(dir, &rel).unwrap()).unwrap();
            let first = &frames[0];
            match what {
                Art::Still(_) => assert!(first.width <= 48 && first.height <= 48, "{rel}: within the arrow's 48 px record"),
                Art::Strip(..) => {
                    assert_eq!(frames.len(), 50, "{rel}");
                    assert_eq!((first.width as f32, first.height as f32), (-2.0 * hot.0, -2.0 * hot.1), "{rel}");
                }
            }
        }
    }

    #[test]
    fn plain_ground_is_the_arrow_the_question_mark_or_no_entry() {
        assert_eq!(map_pointer(Shape::Swords, &cell(true, false)), Shape::Arrow);
        assert_eq!(map_pointer(Shape::Arrow, &cell(false, true)), Shape::Ask);
        assert_eq!(map_pointer(Shape::House, &cell(false, false)), Shape::Denied);
        // Over the minimap: the arrow, whatever lies under it.
        let mini = MapHover { on_minimap: true, unexplored: true, ..Default::default() };
        assert_eq!(map_pointer(Shape::Swords, &mini), Shape::Arrow);
    }

    #[test]
    fn an_army_gets_the_swords_or_for_a_friend_with_a_meeting_the_question_mark() {
        let army = |attitude, meeting| MapHover { valid: true, army: CellArmy::Army { attitude, meeting }, ..Default::default() };
        assert_eq!(map_pointer(Shape::Arrow, &army(-2, false)), Shape::Swords);
        assert_eq!(map_pointer(Shape::Arrow, &army(2, false)), Shape::Swords, "a friend with nothing to say");
        assert_eq!(map_pointer(Shape::Arrow, &army(0, true)), Shape::Swords, "a meeting with a neutral");
        assert_eq!(map_pointer(Shape::Arrow, &army(1, true)), Shape::Query);
        // An army on a cell that is no target keeps its swords.
        let shut = MapHover { valid: false, army: CellArmy::Army { attitude: -1, meeting: false }, ..Default::default() };
        assert_eq!(map_pointer(Shape::Arrow, &shut), Shape::Swords);
    }

    #[test]
    fn the_hovered_cell_gets_the_originals_ring() {
        // 0x4ccb85-0x4cce51: colour 2 on a target or an army, 3 on a guarded building.
        assert_eq!(hover_ring(&cell(true, false)), Some(2));
        assert_eq!(hover_ring(&cell(false, false)), None, "no target, no ring");
        assert_eq!(hover_ring(&cell(false, true)), None);
        let army = MapHover { valid: false, army: CellArmy::Army { attitude: -1, meeting: false }, ..Default::default() };
        assert_eq!(hover_ring(&army), Some(2), "an army's cell, a target or not");
        assert_eq!(hover_ring(&over(building(5))), Some(2), "an unguarded building");
        let guarded = BuildingHover { kind: 3, garrison: true, attitude: 0, ..Default::default() };
        assert_eq!(hover_ring(&over(guarded)), Some(3));
        let held = BuildingHover { kind: 2, standing: vec![(-1, false)], ..Default::default() };
        assert_eq!(hover_ring(&over(held)), Some(3), "an ill-disposed army stands in it");
        let empty_ruins = BuildingHover { kind: 12, standing: vec![(-1, false)], ..Default::default() };
        assert_eq!(hover_ring(&over(empty_ruins)), Some(2));
        let ship = MapHover { valid: true, army: CellArmy::Ship, ..Default::default() };
        assert_eq!(hover_ring(&ship), Some(2));
        // The hero's cell keeps his own mark, in a building too.
        let hero = MapHover { valid: true, army: CellArmy::Hero, building: Some(building(3)), ..Default::default() };
        assert_eq!(hover_ring(&hero), None);
    }

    #[test]
    fn the_hero_and_his_ship_are_the_arrow() {
        let hero = MapHover { valid: true, army: CellArmy::Hero, ..Default::default() };
        assert_eq!(map_pointer(Shape::Swords, &hero), Shape::Arrow);
        let ship = MapHover { valid: true, army: CellArmy::Ship, ..Default::default() };
        assert_eq!(map_pointer(Shape::House, &ship), Shape::Arrow);
        // The building he stands in too (0x4ccdc7 after the building's pointer).
        let inside = MapHover { valid: true, army: CellArmy::Hero, building: Some(building(5)), ..Default::default() };
        assert_eq!(map_pointer(Shape::Arrow, &inside), Shape::Arrow);
    }

    #[test]
    fn a_building_is_the_house_unless_guarded() {
        assert_eq!(map_pointer(Shape::Arrow, &over(building(5))), Shape::House, "a tavern");
        // A village with an ill-disposed army standing in it: its guard's swords.
        let mut v = building(2);
        v.standing = vec![(1, false), (-1, false)];
        assert_eq!(map_pointer(Shape::Arrow, &over(v.clone())), Shape::Swords);
        // Only friends in it: the house; the army in the cell itself does not count.
        v.standing = vec![(1, true)];
        assert_eq!(map_pointer(Shape::Arrow, &over(v.clone())), Shape::House);
        let in_cell = MapHover { army: CellArmy::Army { attitude: -3, meeting: false }, ..over(v) };
        assert_eq!(map_pointer(Shape::Arrow, &in_cell), Shape::House);
        // A tavern is never guarded.
        let mut t = building(5);
        t.standing = vec![(-3, false)];
        assert_eq!(map_pointer(Shape::Arrow, &over(t)), Shape::House);
    }

    #[test]
    fn a_hostile_garrison_gives_the_swords() {
        let mut c = building(3);
        c.garrison = true;
        c.attitude = 0;
        assert_eq!(map_pointer(Shape::Arrow, &over(c.clone())), Shape::Swords);
        c.attitude = 1;
        assert_eq!(map_pointer(Shape::Arrow, &over(c.clone())), Shape::House, "a friendly fort");
        c.garrison = false;
        c.attitude = -2;
        assert_eq!(map_pointer(Shape::Arrow, &over(c)), Shape::House, "no garrison");
        // Ruins are hostile whatever their attitude, and unguarded once empty, even with an
        // ill-disposed army standing in them.
        let mut r = building(12);
        r.garrison = true;
        r.attitude = 3;
        assert_eq!(map_pointer(Shape::Arrow, &over(r.clone())), Shape::Swords);
        r.garrison = false;
        r.standing = vec![(-2, false)];
        assert_eq!(map_pointer(Shape::Arrow, &over(r)), Shape::House);
    }

    #[test]
    fn ownerless_ruins_and_the_obelisk_ask_and_a_shown_house_or_question_stays() {
        let mut r = building(12);
        r.ownerless = true;
        assert_eq!(map_pointer(Shape::Arrow, &over(r.clone())), Shape::Query);
        assert_eq!(map_pointer(Shape::Swords, &over(building(15))), Shape::Query);
        // Cursor_OverBuilding does nothing while the house or the question mark is up
        // (0x48ed1d): from a house straight onto the obelisk the house stays, and back.
        assert_eq!(map_pointer(Shape::House, &over(building(15))), Shape::House);
        assert_eq!(map_pointer(Shape::Query, &over(building(7))), Shape::Query);
        r.ownerless = false;
        assert_eq!(map_pointer(Shape::Arrow, &over(r)), Shape::House, "ruins an army holds");
    }

    #[test]
    fn a_bridge_is_the_arrow_unless_an_army_stands_on_it() {
        assert_eq!(map_pointer(Shape::House, &over(building(13))), Shape::Arrow);
        // On a bridge any army standing in it guards it, a friend too.
        let mut b = building(14);
        b.standing = vec![(2, false)];
        assert_eq!(map_pointer(Shape::Arrow, &over(b.clone())), Shape::Swords);
        b.standing = vec![(2, true)];
        assert_eq!(map_pointer(Shape::Arrow, &over(b)), Shape::Query);
        // A bridge that is no target (he sails: the hit test drops it) is no entry.
        assert_eq!(map_pointer(Shape::Arrow, &cell(false, false)), Shape::Denied);
    }

    #[test]
    fn the_battle_waits_under_the_clock() {
        assert_eq!(battle_pointer(false, false, false), Shape::Arrow);
        assert_eq!(battle_pointer(true, false, false), Shape::Clock);
        assert_eq!(battle_pointer(false, true, false), Shape::Clock);
        assert_eq!(battle_pointer(false, false, true), Shape::Clock);
    }
}
