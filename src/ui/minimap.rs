//! The fog layer over the world map and the minimap window (`rules::fog`,
//! `docs/reference/video-notes.md` §1).
//!
//! - Fog: unexplored cells are black. The layer is a texture with one texel per cell, drawn
//!   scaled over the map with linear filtering; explored cells next to the dark are shaded by
//!   how much dark lies around them, so the edge is a soft feathered band like the original's.
//! - Minimap: a toggle window in the top-right corner of the map view (bottom-bar "Map" button
//!   or M). The whole map scaled down, explored cells in their terrain colour and the rest
//!   black, locations as small icons in the colours the original gives their types, the
//!   armies and the hero as shields, and a light rectangle for the view. A click on it moves the camera there (as in the video); walking still
//!   needs a click on the map.
//!
//! Both textures are rebuilt only when the explored set changes.

use std::cell::RefCell;

use macroquad::prelude::*;

use razdor::rules::fog::Fog;
use razdor::rules::game::Game;
use razdor::rules::map::TileMap;
use razdor::rules::world::LocationKind;

use super::widgets::*;

/// Explored cells within this many cells of the dark are shaded (the feathered edge).
pub const FEATHER: i32 = 2;

struct Cached {
    key: u64,
    tex: Texture2D,
}

thread_local! {
    static FOG_TEX: RefCell<Option<Cached>> = const { RefCell::new(None) };
    static MINI_TEX: RefCell<Option<Cached>> = const { RefCell::new(None) };
}

/// A texture from `cache`, rebuilt by `make` (w, h, rgba) when `key` changes.
fn cached(cache: &'static std::thread::LocalKey<RefCell<Option<Cached>>>, key: u64, filter: FilterMode, make: impl FnOnce() -> (u16, u16, Vec<u8>)) -> Texture2D {
    cache.with(|c| {
        let mut c = c.borrow_mut();
        if c.as_ref().is_none_or(|c| c.key != key) {
            let (w, h, rgba) = make();
            let tex = Texture2D::from_rgba8(w, h, &rgba);
            tex.set_filter(filter);
            *c = Some(Cached { key, tex });
        }
        c.as_ref().unwrap().tex.clone()
    })
}

/// Most darkness an explored cell at the edge of the dark keeps.
const EDGE_DARK: u8 = 170;

/// Darkness (0 lit … 255 black) of every cell: the share of unexplored cells within
/// [`FEATHER`] cells (a box blur through a summed-area table), eased by a smoothstep. The
/// 0.5 contour runs along the border of the explored ground with its corners rounded, so
/// explored cells at the edge are shaded and dark cells at the edge let a little through:
/// the soft edge of the original. Deep in the dark it is black, well inside it is clear.
pub fn darkness(fog: &Fog) -> Vec<u8> {
    darkness_of(fog.w, fog.h, |t| fog.explored(t))
}

/// [`darkness`] of a `w` × `h` fog whose explored cells `explored` tells (the fog as it was
/// before an event uncovered some of it, for the map's reveal).
pub fn darkness_of(w: i32, h: i32, explored: impl Fn((i32, i32)) -> bool) -> Vec<u8> {
    let (w, h) = (w.max(0) as usize, h.max(0) as usize);
    let mut sum = vec![0u32; (w + 1) * (h + 1)];
    for y in 0..h {
        let mut row = 0;
        for x in 0..w {
            row += (!explored((x as i32, y as i32))) as u32;
            sum[(y + 1) * (w + 1) + x + 1] = sum[y * (w + 1) + x + 1] + row;
        }
    }
    let f = FEATHER as usize;
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            // Outside the map counts as neither: the window shrinks at the map's edges.
            let (x0, x1, y0, y1) = (x.saturating_sub(f), (x + f + 1).min(w), y.saturating_sub(f), (y + f + 1).min(h));
            let dark = sum[y1 * (w + 1) + x1] + sum[y0 * (w + 1) + x0] - sum[y0 * (w + 1) + x1] - sum[y1 * (w + 1) + x0];
            let share = dark as f32 / ((x1 - x0) * (y1 - y0)) as f32;
            let t = ((share - 0.2) / 0.5).clamp(0.0, 1.0);
            let mut v = (t * t * (3.0 - 2.0 * t) * 255.0).round() as u8;
            if explored((x as i32, y as i32)) {
                // An explored cell is at least a third lit (world.md §3: the explored mask
                // takes cells whose half-cells add up to over a third of full brightness),
                // and one with no dark neighbour is clear: a small lantern shows too.
                let edge = (y.saturating_sub(1)..(y + 2).min(h)).any(|ny| (x.saturating_sub(1)..(x + 2).min(w)).any(|nx| !explored((nx as i32, ny as i32))));
                v = v.min(if edge { EDGE_DARK } else { 0 });
            }
            out[y * w + x] = v;
        }
    }
    out
}

/// Draws the fog over the map. `tl` and `br` are the screen positions of world points
/// `(-0.5, -0.5·row)` and `(w − 0.5, (h − 0.5)·row)`: the outer corners of the edge cells,
/// so texel centres fall on cell centres.
pub fn draw_fog(fog: &Fog, tl: Vec2, br: Vec2) {
    if !fog.enabled || fog.w <= 0 || fog.h <= 0 {
        return;
    }
    let tex = cached(&FOG_TEX, fog.fingerprint(), FilterMode::Linear, || {
        let rgba = darkness(fog).into_iter().flat_map(|a| [0, 0, 0, a]).collect();
        (fog.w as u16, fog.h as u16, rgba)
    });
    draw_texture_ex(&tex, tl.x, tl.y, WHITE, DrawTextureParams { dest_size: Some(br - tl), ..Default::default() });
}


/// The minimap window's side in pixels of the 960×720 video (`MiniMap_Frame_400x400.lit` is
/// 430 px at the original's 1024×768), and its frame's border.
const WINDOW: f32 = 403.0;
const BORDER: f32 = 13.0 * 403.0 / 430.0;

/// Screen rectangle of the minimap picture (inside its frame): a square in the top-right
/// corner of the map view `view`, the whole map in it, one cell per texel (the original's
/// minimap is square for its square maps).
pub fn rect(map: &TileMap, view: Rect) -> Rect {
    rect_at(map, view, super::chrome::k())
}

/// [`rect`] at interface scale `k`.
fn rect_at(map: &TileMap, view: Rect, k: f32) -> Rect {
    let o = outer_at(view, k);
    let b = BORDER * k;
    let inner = Rect::new(o.x + b, o.y + b, o.w - 2.0 * b, o.h - 2.0 * b);
    let (w, h) = (map.w.max(1) as f32, map.h.max(1) as f32);
    let s = (inner.w / w).min(inner.h / h);
    let (pw, ph) = (w * s, h * s);
    Rect::new(inner.x + (inner.w - pw) / 2.0, inner.y + (inner.h - ph) / 2.0, pw, ph)
}

/// Frame included.
pub fn outer(_map: &TileMap, view: Rect) -> Rect {
    outer_at(view, super::chrome::k())
}

fn outer_at(view: Rect, k: f32) -> Rect {
    let side = (WINDOW * k).min(view.w - 4.0).min(view.h - 4.0);
    Rect::new(view.x + view.w - side - 10.0 * k, view.y + 2.0 * k, side, side)
}

/// The original's colours (`Rus_DiscordTimes.ini [Options]`, 0xRRGGBB), else ours.
fn option_color(key: &str, ours: [u8; 3]) -> Color {
    let v = super::chrome::options_value(key).and_then(|v| v.trim().parse::<u32>().ok());
    let [r, g, b] = v.map_or(ours, |v| [(v >> 16) as u8, (v >> 8) as u8, v as u8]);
    Color::from_rgba(r, g, b, 255)
}

/// A minimap symbol of `MM_Icons.ugs`: column and row in its size's grid.
#[derive(Clone, Copy)]
enum Symbol {
    Grid(u32, u32),
    Castle,
}

/// Where a symbol sits in `MM_Icons.ugs` for icons of `size` (0 small 12 px, 1 medium 18,
/// 2 large 24): five rows of two symbols per size, the castles down the left in three sizes.
fn symbol_rect(sym: Symbol, size: usize) -> Rect {
    match sym {
        Symbol::Grid(c, r) => {
            let (x0, cell) = [(0.0, 12.0), (24.0, 18.0), (60.0, 24.0)][size];
            Rect::new(x0 + c as f32 * cell, r as f32 * cell, cell, cell)
        }
        Symbol::Castle => [Rect::new(0.0, 72.0, 26.0, 12.0), Rect::new(0.0, 84.0, 26.0, 20.0), Rect::new(0.0, 104.0, 26.0, 24.0)][size],
    }
}

/// The symbol of a location on the minimap, if it has one, for icons of `size`: the tables
/// 0x4ed520 (x) and 0x4ed5d4 (y) by building type that Minimap_BuildBuildingMarkers 0x49da20
/// reads. Bridges and obelisks (types 13–15) get no marker. Towns have the castle; castles
/// and forts the tower; taverns, markets and smithies the village's house; altars the skull,
/// the gravestone for picture variant 2 (0x4ed5bc / 0x4ed670). The church's small icon is the
/// house with a cross (x 0, y 48), its medium and large ones the arch.
fn symbol(kind: LocationKind, variant: u8, size: usize) -> Option<Symbol> {
    use LocationKind as K;
    Some(match kind {
        K::Palace | K::Town => Symbol::Castle,
        K::Village | K::Tavern | K::Market | K::Smithy => Symbol::Grid(0, 2),
        K::Castle | K::Fort => Symbol::Grid(0, 3),
        K::Ruins => Symbol::Grid(1, 3),
        K::Church if size == 0 => Symbol::Grid(0, 4),
        K::Church => Symbol::Grid(1, 4),
        K::Shipyard => Symbol::Grid(1, 1),
        K::Entrance => Symbol::Grid(1, 2),
        K::Altar if variant == 2 => Symbol::Grid(1, 0),
        K::Altar => Symbol::Grid(0, 1),
        // The demo's bandit camp has no original type: the skull.
        K::Camp => Symbol::Grid(0, 1),
        K::StoneBridge | K::WoodenBridge | K::Obelisk => return None,
    })
}

/// The army and hero marker of `MM_Icons.ugs`: the 9 × 9 px quad of 0x49e458 at UV
/// (1/128, 61/128)–(10/128, 70/128) of the 128 px texture, the first shield.
const ARMY_MARK: Rect = Rect { x: 1.0, y: 61.0, w: 9.0, h: 9.0 };

/// Which of the options' colours a building marker takes (Minimap_Refresh 0x49e28c with the
/// Community hooks 0xc276eb–0xc27762).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Neutral,
    VillageEmpty,
    VillageFull,
    Harbor,
    Ruin,
    Player,
    Ally,
    Enemy,
}

/// The colour of a building marker: a village by its gold stock (+0x11e = 0 empty; the
/// mana stock is not read), a shipyard the harbour's, smithies, altars, dungeon entrances and
/// ruins (types 8, 10–12) the ruin grey, castles and forts (3–4) the player's when he owns
/// them (+0x124 = 0), else the ally's when their attitude to the player (+0x152) is above 0,
/// else the enemy's. Every other building (palace, town, tavern, market, church) keeps the
/// neutral colour 0xc276eb sets first, whoever owns it.
fn building_mark(kind: LocationKind, owned: bool, attitude: i8, gold_stock: i32) -> Mark {
    use LocationKind as K;
    match kind {
        K::Village if gold_stock == 0 => Mark::VillageEmpty,
        K::Village => Mark::VillageFull,
        K::Shipyard => Mark::Harbor,
        K::Smithy | K::Altar | K::Entrance | K::Ruins => Mark::Ruin,
        K::Castle | K::Fort if owned => Mark::Player,
        K::Castle | K::Fort if attitude > 0 => Mark::Ally,
        K::Castle | K::Fort => Mark::Enemy,
        _ => Mark::Neutral,
    }
}

/// The colour of a location on the minimap (`Rus_DiscordTimes.ini [Options]`, else ours).
fn location_color(l: &razdor::rules::world::Location) -> Color {
    match building_mark(l.kind, l.owned(), l.attitude, l.tribute_gold) {
        Mark::VillageEmpty => option_color("ColorVillageEmpty", [255, 160, 0]),
        Mark::VillageFull => option_color("ColorVillageFull", [255, 255, 0]),
        Mark::Ruin => option_color("ColorRuin", [195, 195, 195]),
        Mark::Harbor => option_color("ColorHarbor", [40, 160, 255]),
        Mark::Player => option_color("ColorBuildingPlayer", [64, 223, 64]),
        Mark::Ally => option_color("ColorBuildingAlly", [0, 160, 255]),
        Mark::Enemy => option_color("ColorBuildingEnemy", [255, 66, 0]),
        Mark::Neutral => option_color("ColorNeutral", [255, 255, 255]),
    }
}

/// An army's marker is in the ally colour only when a meeting event waits for it (+0x3826)
/// and its attitude to the player (+0x16af) is above 0; every other army, friends included,
/// has the enemy colour (0x49e3c9 with the hooks 0xc27773 / 0xc27784).
fn army_is_ally(attitude: i8, meeting_waiting: bool) -> bool {
    meeting_waiting && attitude > 0
}

/// Draws the minimap window. `view_world` is the part of the world the map view shows (world
/// units). Returns the world position clicked, if any.
pub fn window(game: &Game, art: Option<&super::dt_art::DtArt>, view: Rect, view_world: Rect, surface_color: fn(u8) -> Color) -> Option<(f32, f32)> {
    let map = &game.world.map;
    let fog = &game.fog;
    let r = rect(map, view);
    let o = outer(map, view);
    // The window: black behind the map, the original's silver frame over it (or a stone-grey
    // one).
    let frame_art = super::chrome::win_fx("MiniMap_Frame_400x400", super::chrome::Fx::KeyBlack);
    draw_rectangle(o.x, o.y, o.w, o.h, BLACK);
    if frame_art.is_none() {
        draw_rectangle_lines(o.x + 1.0, o.y + 1.0, o.w - 2.0, o.h - 2.0, 2.0, Color::new(0.62, 0.64, 0.62, 1.0));
    }
    let tex = cached(&MINI_TEX, fog.fingerprint() ^ (map.w as u64) << 20 ^ map.h as u64 ^ art.is_some() as u64, FilterMode::Linear, || {
        // The ground: the terrain texture's own colours; the map objects on it in their
        // sprites' colours; building footprints (bridges, roads through towns) light.
        let (w, h) = (map.w as usize, map.h as usize);
        let mut rgb = vec![[0u8; 3]; w * h];
        for y in 0..map.h {
            for x in 0..map.w {
                let code = map.surface_code((x, y));
                rgb[y as usize * w + x as usize] = art.and_then(|a| a.minimap_ground(code, x, y)).unwrap_or_else(|| {
                    let [r, g, b, _]: [u8; 4] = surface_color(code).into();
                    [r, g, b]
                });
            }
        }
        // Objects tint the ground a third of the way to their colour: specks of trees,
        // lighter hills, grey mountains (as the original's minimap shows them).
        for o in map.objects_in_rows(0, map.h) {
            let Some(c) = art.and_then(|a| a.minimap_object(o.class, o.sprite)) else { continue };
            for t in razdor::rules::map::object_cells(o) {
                if t.0 >= 0 && t.1 >= 0 && (t.0 as usize) < w && (t.1 as usize) < h {
                    let p = &mut rgb[t.1 as usize * w + t.0 as usize];
                    *p = [0, 1, 2].map(|i| ((2 * p[i] as u32 + c[i] as u32) / 3) as u8);
                }
            }
        }
        // A little darker than the ground's textures (the video: grass (42, 82, 13)
        // against the texture's (53, 94, 11)).
        if art.is_some() {
            rgb.iter_mut().for_each(|p| *p = p.map(|v| (v as u32 * 85 / 100) as u8));
        }
        for l in game.world.locations.iter().filter(|l| l.kind.is_bridge()) {
            for t in l.cells() {
                if t.0 >= 0 && t.1 >= 0 && (t.0 as usize) < w && (t.1 as usize) < h {
                    rgb[t.1 as usize * w + t.0 as usize] = [230, 230, 230];
                }
            }
        }
        // The dark with the map's soft edge.
        let dark = darkness(fog);
        let mut rgba = Vec::with_capacity(w * h * 4);
        for (i, c) in rgb.iter().enumerate() {
            let lit = 255 - dark.get(i).copied().unwrap_or(255) as u32;
            rgba.extend([c[0], c[1], c[2]].map(|v| (v as u32 * lit / 255) as u8));
            rgba.push(255);
        }
        (map.w as u16, map.h as u16, rgba)
    });
    draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });

    // Cells → minimap pixels (world units: rows are `row_height` apart).
    let rh = map.grid.row_height();
    let k = vec2(r.w / map.w as f32, r.h / map.h as f32);
    let to_mini = |p: (f32, f32)| vec2(r.x + (p.0 + 0.5) * k.x, r.y + (p.1 / rh + 0.5) * k.y);

    // The symbols, in the size that suits the map (small ones for the big maps).
    let icons = super::chrome::win_ugs("MM_Icons");
    let size = if map.w >= 150 { 0 } else if map.w >= 75 { 1 } else { 2 };
    let zoom = super::chrome::k() * 0.9375;
    for l in game.world.locations.iter().filter(|l| !l.kind.is_bridge()) {
        if !l.cells().any(|t| fog.explored(t)) && !fog.explored(l.tile) {
            continue;
        }
        let (ax, ay) = map.center(l.anchor);
        let c = to_mini((ax - (l.size.0 - 1) as f32 / 2.0, ay - (l.size.1 - 1) as f32 * rh / 2.0));
        let col = location_color(l);
        match (&icons, symbol(l.kind, l.picture.1, size)) {
            (Some(t), Some(sym)) => {
                let src = symbol_rect(sym, size);
                let (w, h) = (src.w * zoom, src.h * zoom);
                super::chrome::tex_src(t, src, Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h), col);
            }
            _ => {
                draw_circle(c.x, c.y, 3.5, Color::new(0.0, 0.0, 0.0, 0.85));
                draw_circle(c.x, c.y, 2.5, col);
            }
        }
    }

    // The hero, then every army on the map and outside a building (+0x16a1 set, +0x3788 = 0),
    // as the army loop of 0x49e381 draws them: the 9 px shield at the centre of its cell
    // (+0x1724/+0x1728 + 0.5), the hero in `ColorMarkPlayer`, an army in `ColorMarkAlly` or
    // `ColorMarkEnemy` ([`army_is_ally`]). The original draws them all and lays the fog
    // over them (0x49c700); ours leaves out the ones on unexplored ground.
    let (player, enemy, ally) = (option_color("ColorMarkPlayer", [255, 255, 255]), option_color("ColorMarkEnemy", [255, 66, 0]), option_color("ColorMarkAlly", [0, 160, 255]));
    let mut marks = vec![(game.tile(), player)];
    for a in game.world.armies.iter() {
        let t = a.tile(map);
        if !fog.explored(t) || game.world.location_at(t).is_some() {
            continue;
        }
        let waits = game.script().is_some_and(|e| e.meeting_waiting(game, a.id));
        marks.push((t, if army_is_ally(a.attitude, waits) { ally } else { enemy }));
    }
    for (t, col) in marks {
        let p = to_mini(map.center(t));
        match &icons {
            Some(tex) => {
                let s = ARMY_MARK.w * zoom;
                super::chrome::tex_src(tex, ARMY_MARK, Rect::new(p.x - s / 2.0, p.y - s / 2.0, s, s), col);
            }
            None => {
                draw_circle(p.x, p.y, 3.5 * zoom.max(1.0), BLACK);
                draw_circle(p.x, p.y, 2.5 * zoom.max(1.0), col);
            }
        }
    }

    // The view: a light grey box, as the original's.
    let a = to_mini((view_world.x, view_world.y));
    let b = to_mini((view_world.x + view_world.w, view_world.y + view_world.h));
    let (x0, y0) = (a.x.max(r.x), a.y.max(r.y));
    let (x1, y1) = (b.x.min(r.x + r.w), b.y.min(r.y + r.h));
    if x1 > x0 && y1 > y0 {
        draw_rectangle(x0, y0, x1 - x0, y1 - y0, Color::new(1.0, 1.0, 1.0, 0.18));
        draw_rectangle_lines(x0, y0, x1 - x0, y1 - y0, 1.0, Color::new(0.85, 0.85, 0.85, 0.6));
    }

    if let Some(t) = frame_art {
        super::chrome::tex(&t, o, WHITE);
    }

    let m = Vec2::from(crate::ui::widgets::pointer());
    (clicked() && r.contains(m)).then(|| ((m.x - r.x) / k.x - 0.5, ((m.y - r.y) / k.y - 0.5) * rh))
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::rules::map::Grid;

    #[test]
    fn darkness_is_black_in_the_dark_and_feathered_at_the_edge() {
        // Explored: the left half.
        let (w, h) = (40, 20);
        let mut fog = Fog::new(w, h);
        for y in 0..h {
            for x in 0..20 {
                fog.mark((x, y));
            }
        }
        let d = darkness(&fog);
        let at = |x: i32, y: i32| d[(y * w + x) as usize];
        assert_eq!(at(39, 10), 255, "deep in the dark");
        assert_eq!(at(5, 10), 0, "well inside is clear");
        assert!(at(19, 10) > 0 && at(19, 10) < 128, "explored edge cell shaded: {}", at(19, 10));
        assert!(at(20, 10) >= 128 && at(20, 10) < 255, "dark edge cell mostly dark: {}", at(20, 10));
        assert!(at(17, 10) <= at(19, 10) && at(20, 10) <= at(22, 10), "darker outwards");
        assert!(darkness(&Fog::disabled(3, 3)).iter().all(|&a| a == 0));
    }

    #[test]
    fn a_small_lantern_shows_through_the_dark() {
        // A radius-1 lantern opens 9 cells; the blur alone kept their centre near black.
        let mut fog = Fog::new(30, 30);
        fog.reveal(15, 15, 1);
        let d = darkness(&fog);
        let at = |x: i32, y: i32| d[(y * 30 + x) as usize];
        assert_eq!(at(15, 15), 0, "its centre is clear");
        assert!(at(16, 15) <= EDGE_DARK, "an explored edge cell is at least a third lit: {}", at(16, 15));
        assert_eq!(at(25, 25), 255);
    }

    #[test]
    fn minimap_is_square_for_square_maps_and_keeps_other_aspects() {
        let codes = vec![0u8; 200 * 100];
        let map = TileMap::from_codes(Grid::Square8, 200, 100, &codes, vec![]);
        let view = Rect::new(0.0, 0.0, 1000.0, 700.0);
        let r = rect_at(&map, view, 1.0);
        assert!((r.w / r.h - 2.0).abs() < 1e-3, "one cell per texel: {r:?}");
        let o = outer_at(view, 1.0);
        assert!(o.contains(r.point()) && r.x + r.w <= o.x + o.w && o.x + o.w <= 1000.0 && o.y >= 0.0);
        assert!((o.w - o.h).abs() < 1e-3, "the frame is square");
        let square = TileMap::from_codes(Grid::Square8, 50, 50, &vec![0u8; 2500], vec![]);
        let r = rect_at(&square, view, 1.0);
        assert!((r.w - r.h).abs() < 1e-3);
    }

    #[test]
    fn building_markers_take_the_originals_colours_by_type() {
        // 0x49e28c: only castles and forts are coloured by owner and attitude; taverns,
        // markets, churches and towns stay neutral whoever owns them.
        use LocationKind as K;
        assert_eq!(building_mark(K::Castle, true, -3, 0), Mark::Player);
        assert_eq!(building_mark(K::Fort, false, 1, 0), Mark::Ally);
        assert_eq!(building_mark(K::Castle, false, 0, 0), Mark::Enemy, "attitude 0 is the enemy's");
        for k in [K::Tavern, K::Market, K::Church, K::Town, K::Palace] {
            assert_eq!(building_mark(k, false, -3, 0), Mark::Neutral, "{k:?}");
            assert_eq!(building_mark(k, true, 3, 0), Mark::Neutral, "{k:?}");
        }
        for k in [K::Smithy, K::Altar, K::Entrance, K::Ruins] {
            assert_eq!(building_mark(k, false, -3, 0), Mark::Ruin, "{k:?}");
        }
        assert_eq!(building_mark(K::Shipyard, false, -3, 0), Mark::Harbor);
        assert_eq!(building_mark(K::Village, false, 0, 0), Mark::VillageEmpty);
        assert_eq!(building_mark(K::Village, false, 0, 40), Mark::VillageFull);
    }

    #[test]
    fn only_a_friend_with_a_meeting_waiting_is_marked_as_an_ally() {
        // 0x49e3c9: +0x3826 and +0x16af > 0; a friend without a meeting is red.
        assert!(army_is_ally(1, true));
        assert!(!army_is_ally(3, false));
        assert!(!army_is_ally(0, true) && !army_is_ally(-2, true));
    }

    #[test]
    fn minimap_symbols_follow_the_originals_type_tables() {
        // 0x4ed520 / 0x4ed5d4: (x, y) of the small icons by type.
        use LocationKind as K;
        let at = |k, v, size| symbol(k, v, size).map(|s| symbol_rect(s, size).point());
        assert_eq!(at(K::Castle, 0, 0), Some(vec2(0.0, 36.0)), "a castle has the tower");
        assert_eq!(at(K::Tavern, 0, 0), Some(vec2(0.0, 24.0)), "a tavern has the house");
        assert_eq!(at(K::Smithy, 0, 0), Some(vec2(0.0, 24.0)));
        assert_eq!(at(K::Altar, 0, 0), Some(vec2(0.0, 12.0)), "an altar has the skull");
        assert_eq!(at(K::Altar, 2, 0), Some(vec2(12.0, 0.0)), "variant 2 the gravestone");
        assert_eq!(at(K::Church, 0, 0), Some(vec2(0.0, 48.0)));
        assert_eq!(at(K::Church, 0, 1), Some(vec2(42.0, 72.0)));
        assert_eq!(at(K::Church, 0, 2), Some(vec2(84.0, 96.0)));
        assert_eq!(at(K::Town, 0, 0), Some(vec2(0.0, 72.0)));
        assert_eq!(at(K::Obelisk, 0, 0), None, "types 13-15 have no marker");
    }

    #[test]
    fn minimap_symbols_lie_inside_the_atlas() {
        for size in 0..3 {
            for sym in [Symbol::Castle, Symbol::Grid(0, 0), Symbol::Grid(1, 4)] {
                let r = symbol_rect(sym, size);
                assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 108.0 && r.y + r.h <= 128.0, "{size} {r:?}");
            }
        }
        let m = ARMY_MARK;
        assert!(m.x >= 0.0 && m.x + m.w <= 12.0 && m.y >= 60.0 && m.y + m.h <= 72.0, "inside the first shield");
    }
}
