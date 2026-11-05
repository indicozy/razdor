//! Fog of war: which cells of the world map the player has explored.
//!
//! The original (`docs/reference/original-mechanics/world.md` §3): unexplored ground is black
//! and **counts as impassable to the hero's planner** until he has seen it (the AI ignores the
//! fog); the hero reveals a disc around himself after every step and at the start; explored
//! ground stays fully visible for good; lanterns reveal the same kind of disc.
//!
//! - Sight radius by class: knight 9, archmage 8, ranger 10 cells ([`sight_radius`]); the disc
//!   is a circle in **cells** (an ellipse on the 32×22 px screen).
//! - A lantern's radius (at most 24) is in cells too.
//! - The original's edge is soft: a brightness per half-cell, and a cell counts as explored
//!   when its four half-cells are bright enough ([`stamp`]); radius 8 explores 241 cells, 9
//!   293, 10 349.
//! - An unexplored cell is no target for a click, and the hero's planner treats it as closed
//!   (`Game::plan`).
//! - The built-in demo plays without fog ([`Fog::disabled`]).
//!
//! The state is plain data (`w`, `h`, a flag and a `Vec<u64>` bitset) so a save file can store
//! it as it is.

use crate::dt::dtm::Scenario;

use super::content::HeroClass;
use super::map::{Tile, TileMap};

/// Largest lantern radius the editor allows.
pub const MAX_LANTERN_RADIUS: i32 = 24;
/// Half-cells of the original's largest stamp (its masks run 0..=48).
const MAX_HALF_RADIUS: usize = 48;

/// How far the hero of `class` sees, in cells (world.md §3: 18, 16 and 20 half-cells).
pub fn sight_radius(class: HeroClass) -> i32 {
    match class {
        HeroClass::Knight => 9,
        HeroClass::Archmage => 8,
        HeroClass::Ranger => 10,
    }
}

/// The original's explored stamp for a radius of `half` half-cells (world.md §3, 0x4cf734):
/// the cells, as offsets from the centre cell, that a reveal explores. Each half-cell of a
/// 98 × 98 mask centred between its two middle pixels has the brightness
/// `Round(15 − 8·(d − half))` clamped to 0..=15 (`d` its distance in half-cells, single
/// precision, rounded half to even); a cell of the 49 × 49 explored mask is explored when the
/// darkness `15 − b` of its four half-cells, scaled by 4096 and summed, shifted right by 14
/// is below 10 (their brightness adds up to more than 20).
pub fn stamp(half: usize) -> &'static [(i32, i32)] {
    static STAMPS: std::sync::OnceLock<Vec<Vec<(i32, i32)>>> = std::sync::OnceLock::new();
    let all = STAMPS.get_or_init(|| (0..=MAX_HALF_RADIUS).map(build_stamp).collect());
    &all[half.min(MAX_HALF_RADIUS)]
}

fn build_stamp(half: usize) -> Vec<(i32, i32)> {
    let brightness = |px: i32, py: i32| -> i32 {
        let (dx, dy) = (px as f32 - 48.5, py as f32 - 48.5);
        let d = (dx * dx + dy * dy).sqrt();
        let v = 15.0 - 8.0 * (d as f64 - half as f64);
        (v.round_ties_even() as i32).clamp(0, 15)
    };
    let mut out = Vec::new();
    for j in 0..49 {
        for i in 0..49 {
            let dark: i32 = [(0, 0), (1, 0), (0, 1), (1, 1)].iter().map(|&(a, b)| (15 - brightness(2 * i + a, 2 * j + b)) << 12).sum();
            if dark >> 14 < 10 {
                out.push((i - 24, j - 24));
            }
        }
    }
    out
}

/// Cell `b` lies within the disc of radius `r` cells around `a` (the original's stamp of `2r`
/// half-cells, [`stamp`]).
pub fn within(a: Tile, b: Tile, r: i32) -> bool {
    let off = (b.0 - a.0, b.1 - a.1);
    stamp(2 * r.max(0) as usize).contains(&off)
}

/// Explored cells of a `w × h` map, one bit per cell (row by row).
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Fog {
    pub w: i32,
    pub h: i32,
    /// Off: every cell counts as explored (the demo).
    pub enabled: bool,
    /// Bit `y*w + x` is set once cell `(x, y)` has been seen.
    pub bits: Vec<u64>,
}

impl Fog {
    /// All dark.
    pub fn new(w: i32, h: i32) -> Fog {
        let n = (w.max(0) * h.max(0)) as usize;
        Fog { w, h, enabled: true, bits: vec![0; n.div_ceil(64)] }
    }

    /// No fog: everything counts as explored.
    pub fn disabled(w: i32, h: i32) -> Fog {
        Fog { enabled: false, ..Fog::new(w, h) }
    }

    fn index(&self, (x, y): Tile) -> Option<usize> {
        (x >= 0 && y >= 0 && x < self.w && y < self.h).then(|| (y * self.w + x) as usize)
    }

    /// The cell has been seen (always true with the fog off; false outside the map).
    pub fn explored(&self, t: Tile) -> bool {
        match self.index(t) {
            Some(i) => !self.enabled || self.bits[i / 64] & (1 << (i % 64)) != 0,
            None => false,
        }
    }

    /// An explored cell lies within `r` cells of `t` (a square around it): the soft edge of
    /// the dark reaches that far, so things there show faded through it.
    pub fn explored_near(&self, t: Tile, r: i32) -> bool {
        (-r..=r).any(|dy| (-r..=r).any(|dx| self.explored((t.0 + dx, t.1 + dy))))
    }

    /// Marks one cell explored. Returns true if it was dark.
    pub fn mark(&mut self, t: Tile) -> bool {
        let Some(i) = self.index(t) else { return false };
        let (word, bit) = (i / 64, 1u64 << (i % 64));
        let new = self.bits[word] & bit == 0;
        self.bits[word] |= bit;
        new && self.enabled
    }

    /// Explores the disc of radius `r` cells (at most [`MAX_LANTERN_RADIUS`]) around cell
    /// `(x, y)`: the original's stamp of `2r` half-cells ([`stamp`]), the hero's sight or a
    /// lantern. Returns true if anything new was revealed.
    pub fn reveal(&mut self, x: i32, y: i32, r: i32) -> bool {
        let half = 2 * r.clamp(0, MAX_LANTERN_RADIUS) as usize;
        let mut new = false;
        for &(dx, dy) in stamp(half) {
            new |= self.mark((x + dx, y + dy));
        }
        new
    }

    /// Number of explored cells (the whole map with the fog off).
    pub fn explored_count(&self) -> usize {
        if !self.enabled {
            return (self.w * self.h) as usize;
        }
        self.bits.iter().map(|b| b.count_ones() as usize).sum()
    }

    /// A cheap fingerprint of the explored set, for caches that redraw when it changes.
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ (self.w as u64) << 32 ^ self.h as u64 ^ self.enabled as u64;
        for &b in &self.bits {
            h = (h ^ b).wrapping_mul(0x0100_0000_01b3).rotate_left(5);
        }
        h
    }
}

/// Active lanterns at the start of a scenario: `(cell, radius)` of every point that is
/// active at start and has a radius. The map loader does not look at the model byte
/// (0x4b5a37), so an event point (model 9) marked active lights up too, as in РК1.
pub fn start_lanterns(s: &Scenario) -> Vec<(Tile, i32)> {
    s.points
        .iter()
        .filter(|p| p.active != 0 && p.radius > 0)
        .map(|p| ((p.x as i32, p.y as i32), p.radius as i32))
        .collect()
}

/// A lantern an event lights, by point id: `(cell, radius)`. An inactive lantern has the
/// event-point model but keeps its radius; one without a radius gets none *(guess)*.
pub fn lantern(s: &Scenario, point: u16) -> Option<(Tile, i32)> {
    let p = s.points.iter().find(|p| p.id as u16 == point)?;
    (p.radius > 0).then_some(((p.x as i32, p.y as i32), p.radius as i32))
}

/// The fog for a new game on `map`: the scenario's active lanterns lit. Returns a disabled
/// fog when `enabled` is false.
pub fn for_scenario(map: &TileMap, s: Option<&Scenario>, enabled: bool) -> Fog {
    if !enabled {
        return Fog::disabled(map.w, map.h);
    }
    let mut fog = Fog::new(map.w, map.h);
    for ((x, y), r) in s.map(start_lanterns).unwrap_or_default() {
        fog.reveal(x, y, r);
    }
    fog
}

/// Which side a location or army belongs to, for its colour on the minimap.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Player,
    Ally,
    Neighbour,
    Enemy,
    Neutral,
}

impl Side {
    /// By the player's ownership and the editor's faction code (1 player, 2 ally,
    /// 3 neighbour, 4 enemy; anything else neutral).
    pub fn of(owned_by_player: bool, faction: u8) -> Side {
        if owned_by_player {
            return Side::Player;
        }
        match faction {
            1 => Side::Player,
            2 => Side::Ally,
            3 => Side::Neighbour,
            4 => Side::Enemy,
            _ => Side::Neutral,
        }
    }

    /// Minimap colour (RGB): player green, ally blue, neighbour yellow, enemy red, neutral
    /// light grey, as the editor colours the factions.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Side::Player => [70, 200, 90],
            Side::Ally => [70, 130, 230],
            Side::Neighbour => [230, 200, 60],
            Side::Enemy => [220, 60, 50],
            Side::Neutral => [215, 215, 215],
        }
    }
}

/// Side of a location: the player's own, else its faction; a location without an owner is
/// neutral.
pub fn location_side(l: &super::world::Location) -> Side {
    use super::world::Owner;
    match l.owner {
        Owner::Player => Side::Player,
        Owner::Neutral if l.faction == 0 => Side::Neutral,
        _ => Side::of(false, l.faction),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Point, Surface};
    use crate::rules::map::Grid;

    #[test]
    fn near_explored_reaches_into_the_dark_edge() {
        let mut fog = Fog::new(10, 10);
        fog.mark((2, 2));
        assert!(fog.explored_near((4, 4), 2) && fog.explored_near((0, 3), 2));
        assert!(!fog.explored_near((5, 2), 2) && !fog.explored((4, 4)));
    }

    fn open_map(w: i32, h: i32) -> TileMap {
        TileMap::from_codes(Grid::Square8, w, h, &vec![Surface::GrassPlain as u8; (w * h) as usize], vec![])
    }

    #[test]
    fn sight_is_9_8_10_cells_by_class() {
        assert_eq!(sight_radius(HeroClass::Knight), 9);
        assert_eq!(sight_radius(HeroClass::Archmage), 8);
        assert_eq!(sight_radius(HeroClass::Ranger), 10);
    }

    #[test]
    fn the_stamps_explore_the_originals_cell_counts() {
        // World.md §3: radius 8 cells (16 half-cells) → 241 cells, 9 → 293, 10 → 349,
        // 24 → 1901.
        assert_eq!([16, 18, 20, 48].map(|h| stamp(h).len()), [241, 293, 349, 1901]);
        // No single disc fits every radius: the archmage's 8 includes (±7, ±5), which a disc
        // of 8.6 cells leaves out.
        assert!(stamp(16).contains(&(7, 5)) && stamp(16).contains(&(-5, -7)));
        assert!(!stamp(18).contains(&(10, 0)) && stamp(18).contains(&(9, 0)));
        assert_eq!(stamp(0), &[(0, 0)][..], "radius 0: its own cell");
        let mut f = Fog::new(40, 40);
        f.reveal(20, 20, 8);
        assert_eq!(f.explored_count(), 241);
        assert!(within((20, 20), (27, 25), 8) && !within((20, 20), (27, 26), 8));
    }

    #[test]
    fn reveal_is_a_circle_in_cells_and_stays() {
        let mut f = Fog::new(40, 40);
        assert_eq!(f.explored_count(), 0);
        assert!(f.reveal(20, 20, 9));
        // 9 cells every way, rows and columns alike (an ellipse on the 32×22 px screen).
        assert!(f.explored((29, 20)) && !f.explored((30, 20)));
        assert!(f.explored((11, 20)) && !f.explored((10, 20)));
        assert!(f.explored((20, 29)) && !f.explored((20, 30)));
        assert!(f.explored((20, 11)) && !f.explored((20, 10)));
        assert!(f.explored((26, 27)) && !f.explored((27, 27)), "a circle: the corners stay dark");
        let n = f.explored_count();
        assert!(!f.reveal(20, 20, 9), "nothing new");
        // Walking away keeps the old ground explored.
        f.reveal(5, 5, 9);
        assert!(f.explored((20, 20)) && f.explored((5, 5)));
        assert!(f.explored_count() > n);
        assert!(!f.explored((-1, 0)) && !f.explored((40, 0)));
    }

    #[test]
    fn disabled_fog_explores_everything() {
        let f = Fog::disabled(5, 5);
        assert!(f.explored((4, 4)) && !f.explored((5, 4)));
        assert_eq!(f.explored_count(), 25);
    }

    #[test]
    fn lanterns_reveal_their_radius_in_cells() {
        let m = open_map(60, 60);
        let mut s = Scenario::default();
        let point = |id, x, y, model, active, radius| Point { x, y, id, model, active, radius, ..blank_point() };
        s.points = vec![point(1, 10, 10, 8, 1, 3), point(2, 40, 40, 9, 0, 5), point(3, 50, 10, 8, 0, 4), point(4, 30, 50, 8, 1, 0), point(5, 20, 30, 9, 1, 2)];
        // The model is not checked: an active event point with a radius is lit too (the
        // original's loader, saves-data.md §10.7).
        assert_eq!(start_lanterns(&s), vec![((10, 10), 3), ((20, 30), 2)]);
        assert_eq!(lantern(&s, 2), Some(((40, 40), 5)));
        assert_eq!(lantern(&s, 4), None);
        assert_eq!(lantern(&s, 9), None);
        let mut f = for_scenario(&m, Some(&s), true);
        assert!(f.explored((10, 10)) && f.explored((13, 10)) && !f.explored((14, 10)));
        assert!(f.explored((10, 13)) && !f.explored((10, 14)), "rows count as cells too");
        assert!(f.explored((13, 12)) && !f.explored((13, 13)), "the original's stamp, not a disc");
        assert!(!f.explored((40, 40)) && !f.explored((50, 10)));
        // Lighting one later (an event).
        let ((x, y), r) = lantern(&s, 2).unwrap();
        assert!(f.reveal(x, y, r));
        assert!(f.explored((45, 40)) && !f.explored((46, 40)));
        // Radius is capped at 24.
        let mut g = Fog::new(60, 60);
        g.reveal(30, 30, 99);
        assert!(g.explored((54, 30)) && !g.explored((55, 30)));
        assert!(!for_scenario(&m, Some(&s), false).enabled);
    }

    fn blank_point() -> Point {
        Point {
            x: 0,
            y: 0,
            id: 0,
            model: 0,
            serial: 0,
            event_slots: [0; 10],
            priorities: [0; 4],
            active_duration: 0,
            radius: 0,
            event_count: 0,
            active: 0,
            unknown_41: [0; 58],
        }
    }



    #[test]
    fn sides_and_colours() {
        assert_eq!(Side::of(true, 4), Side::Player);
        assert_eq!(Side::of(false, 1), Side::Player);
        assert_eq!(Side::of(false, 2), Side::Ally);
        assert_eq!(Side::of(false, 3), Side::Neighbour);
        assert_eq!(Side::of(false, 4), Side::Enemy);
        assert_eq!(Side::of(false, 0), Side::Neutral);
        assert_eq!(Side::of(false, 9), Side::Neutral);
        let c = |s: Side| s.rgb();
        assert!(c(Side::Player)[1] > c(Side::Player)[0], "green");
        assert!(c(Side::Ally)[2] > c(Side::Ally)[0], "blue");
        assert!(c(Side::Enemy)[0] > c(Side::Enemy)[1], "red");
        assert!(c(Side::Neighbour)[0] > 200 && c(Side::Neighbour)[1] > 180 && c(Side::Neighbour)[2] < 100, "yellow");
        let n = c(Side::Neutral);
        assert!(n[0] == n[1] && n[1] == n[2], "grey");
        let all = [Side::Player, Side::Ally, Side::Neighbour, Side::Enemy, Side::Neutral].map(c);
        for i in 0..5 {
            for j in i + 1..5 {
                assert_ne!(all[i], all[j]);
            }
        }
    }

    #[test]
    fn fingerprint_changes_with_the_explored_set() {
        let mut f = Fog::new(10, 10);
        let a = f.fingerprint();
        f.reveal(3, 3, 1);
        assert_ne!(a, f.fingerprint());
    }
}

#[cfg(test)]
mod real_maps {
    //! Checks against the player's install; skipped without `RAZDOR_DT_DIR`.
    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::game::Game;
    use std::sync::Arc;

    fn install() -> Option<(DtInstall, Arc<Content>)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        Some((dt, c))
    }

    #[test]
    fn lanterns_of_the_shipped_maps() {
        let Some((dt, _)) = install() else { return };
        let (mut lit, mut points) = (0, 0);
        for m in &dt.maps {
            let s = m.load().unwrap();
            points += s.points.len();
            for ((x, y), r) in start_lanterns(&s) {
                lit += 1;
                assert!((x as u32) < s.width() && (y as u32) < s.height(), "{}", m.name);
                assert!(r <= MAX_LANTERN_RADIUS, "{}: radius {r}", m.name);
            }
        }
        eprintln!("{} maps: {points} points, {lit} lanterns lit at start", dt.maps.len());
    }

    #[test]
    fn rk1_and_rk3_start_in_a_revealed_region() {
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            for class in HeroClass::ALL {
                let g = Game::from_scenario(c.clone(), &s, class);
                let (w, h) = (g.world.map.w, g.world.map.h);
                let here = g.tile();
                let n = g.fog.explored_count();
                assert!(g.fog.enabled && g.fog.explored(here), "{prefix} {class:?}");
                // The hero's own disc of sight (the original's stamp, `within`) is explored, and
                // the map is not. A start event's lanterns are lit at its window's OK (G1), so
                // they may still be dark here: the disc is the floor, not a count.
                let r = g.sight_radius();
                let disc: Vec<Tile> = (-r..=r).flat_map(|dy| (-r..=r).map(move |dx| (here.0 + dx, here.1 + dy))).filter(|&t| within(here, t, r) && g.world.map.in_bounds(t)).collect();
                assert!(disc.iter().all(|&t| g.fog.explored(t)), "{prefix} {class:?}: the hero's disc");
                assert!(n >= disc.len() && n < (w * h / 4) as usize, "{prefix} {class:?}: {n} of {}", w * h);
                for (dx, dy) in [(r, 0), (-r, 0), (0, r), (0, -r)] {
                    let t = (here.0 + dx, here.1 + dy);
                    assert!(!g.world.map.in_bounds(t) || g.fog.explored(t), "{prefix} {class:?}: {t:?}");
                }
                eprintln!("{prefix} {class:?}: {n} of {} cells explored at start", w * h);
            }
        }
    }

    #[test]
    fn rk1_and_rk3_walk_to_the_nearest_village_through_the_fog() {
        use crate::rules::world::LocationKind;
        let Some((dt, c)) = install() else { return };
        for prefix in ["РК1", "РК3"] {
            let s = dt.maps.iter().find(|m| m.name.starts_with(prefix)).unwrap().load().unwrap();
            let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
            g.world.armies.clear();
            let (village, _) = g.world.nearest_location(g.tile(), |l| l.kind == LocationKind::Village).expect("a village");
            let target = g.world.locations[village].tile;
            let dark = !g.fog.explored(target);
            // A cell in the dark is no target; leg by leg over explored ground he gets there.
            assert_eq!(g.can_target(target), !dark, "{prefix}");
            g.walk_through_fog(target);
            assert_eq!(g.location, Some(village), "{prefix}: arrives (village in the dark at start: {dark})");
            eprintln!("{prefix}: village in the dark at start: {dark}");
        }
    }
}
