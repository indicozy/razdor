//! The map canvas's derived layers, computed from the compiled map and the package's triggers: the
//! passability grid and its impassable cells (DTMapEdit's marks, `docs/reference/editor/main-window.md`
//! §8.3, §8.4 and the engine's impassable rule of `mapcheck` §1), the fog a map starts under (the
//! hero's reveal and the lit lanterns, §10.6) and the patrol loops of the triggers' `army_goal`s.
//!
//! The package format has no patrol radius and no marks: the marks are rebuilt here from the
//! objects and buildings the way the original's grid is, and a patrol is the cells its goal lists.

use crate::dt::dtm::Scenario;
use crate::pack::load::Located;
use crate::pack::triggers::{Action, GoalSpec, Trigger};
use crate::rules::world::footprint_cells;

/// The mark of a building's footprint cell (the original's −127).
pub const MARK_BUILDING: i8 = -127;

/// The grid's tiles (`main-window.md` §8.4).
pub const GRID_NORMAL: u8 = 0;
pub const GRID_HARD: u8 = 1;
pub const GRID_HARDER: u8 = 2;
pub const GRID_IMPASSABLE: u8 = 3;
pub const GRID_BUILDING: u8 = 4;
pub const GRID_SHALLOW: u8 = 5;
pub const GRID_DEEP: u8 = 6;

/// The signed mark of every cell, row by row: the terrain code by default, minus the class on the
/// cells a massif, a plant or a thicket covers, and the building mark on a footprint (the
/// original's four passes, `grid.rs` of the editor's model). Later objects of a slot win.
pub fn marks(s: &Scenario) -> Vec<i8> {
    let (w, h) = (s.width() as i64, s.height() as i64);
    let idx = |x: i64, y: i64| (y * w + x) as usize;
    let mut m: Vec<i8> = s.terrain.iter().map(|c| *c as i8).collect();
    // One object per cell and slot: the first slot holds classes 1 to 8, the second 9 to 12.
    let mut slots: Vec<[Option<(u8, u8)>; 2]> = vec![[None, None]; (w * h) as usize];
    for o in &s.objects {
        if o.class > 12 || (o.class == 0 && o.sprite == 0) || i64::from(o.x) >= w || i64::from(o.y) >= h {
            continue;
        }
        let slot = if (1..=8).contains(&o.class) { 0 } else { 1 };
        slots[idx(i64::from(o.x), i64::from(o.y))][slot] = Some((o.class, o.sprite));
    }
    for y in 0..h {
        for x in 0..w {
            let Some((class, sprite)) = slots[idx(x, y)][0] else { continue };
            let n = i64::from(sprite / 10).max(1);
            for dx in (1 - n)..=0 {
                for dy in (1 - n)..=0 {
                    let (cx, cy) = (x + dx, y + dy);
                    if cx >= 0 && cy >= 0 && cx < w && cy < h && m[idx(cx, cy)] >= 0 {
                        m[idx(cx, cy)] = -(class as i8);
                    }
                }
            }
        }
    }
    for y in 0..h {
        for x in 0..w {
            let Some((class, _)) = slots[idx(x, y)][1] else { continue };
            let i = idx(x, y);
            let mark = m[i];
            let keeps = mark < 0 || (mark > 2 && !(4..=8).contains(&mark) && !(10..=14).contains(&mark));
            let overwritable = (-4..=-1).contains(&mark) || mark == -12;
            if !keeps || overwritable {
                m[i] = -(class as i8);
            }
        }
    }
    for b in &s.buildings {
        for (cx, cy) in footprint_cells((b.x as i32, b.y as i32), (b.size_x.max(1) as i32, b.size_y.max(1) as i32)) {
            let (cx, cy) = (i64::from(cx), i64::from(cy));
            if cx >= 0 && cy >= 0 && cx < w && cy < h {
                m[idx(cx, cy)] = MARK_BUILDING;
            }
        }
    }
    m
}

/// The impassable cells (the original's rule, `grid.rs`: deep sea, lava, the impassable swamp and
/// snowdrifts, or a mark of thicket or of a mountain or rocks). A building does not make a cell
/// impassable: a bridge is walked on.
pub fn impassable(s: &Scenario, marks: &[i8]) -> Vec<bool> {
    s.terrain
        .iter()
        .zip(marks)
        .map(|(&code, &mark)| matches!(code, 2 | 3 | 9 | 15) || mark == -11 || (-8..=-5).contains(&mark))
        .collect()
}

/// The grid tile of every cell (`main-window.md` §8.4): the terrain's value, then the objects'
/// (mountains, rocks and thickets make it impassable; low hills and trees add one step below 2),
/// lava always impassable, a building's footprint overrides all.
pub fn grid(s: &Scenario, marks: &[i8]) -> Vec<u8> {
    s.terrain
        .iter()
        .zip(marks)
        .map(|(&code, &mark)| {
            if mark == MARK_BUILDING {
                return GRID_BUILDING;
            }
            if code == 3 {
                return GRID_IMPASSABLE;
            }
            let start = match code {
                0 | 1 => GRID_SHALLOW,
                2 => GRID_DEEP,
                8 | 14 => GRID_HARD,
                9 | 15 => GRID_IMPASSABLE,
                _ => GRID_NORMAL,
            };
            match mark {
                -8..=-5 | -11 => GRID_IMPASSABLE,
                -4..=-1 | -10 | -9 if start < GRID_HARDER => start + 1,
                _ => start,
            }
        })
        .collect()
}

/// The cells the hero and the lit lanterns reveal at the start: every cell whose distance from a
/// source, rounded, is at most its radius (the reveal routine, `cells.rs` of the editor's model).
/// `sources` are `(cell, radius)`; the result is row by row, `true` for a revealed cell.
pub fn revealed(width: u32, height: u32, sources: &[((i64, i64), i64)]) -> Vec<bool> {
    let mut out = vec![false; (width * height) as usize];
    for &((sx, sy), r) in sources {
        if !(0..128).contains(&r) {
            continue;
        }
        for dx in -r..=r {
            for dy in -r..=r {
                let d = ((dx * dx + dy * dy) as f64).sqrt().round() as i64;
                let (x, y) = (sx + dx, sy + dy);
                if d <= r && x >= 0 && y >= 0 && x < i64::from(width) && y < i64::from(height) {
                    out[(y * i64::from(width) + x) as usize] = true;
                }
            }
        }
    }
    out
}

/// The hero's reveal radius at the start (`cells.rs`: `HERO_REVEAL`).
pub const HERO_REVEAL: i64 = 5;

/// The sources of the start's reveal: the hero's start cell and every lit lantern of the compiled
/// map (its own radius).
pub fn start_sources(s: &Scenario, hero: (i64, i64)) -> Vec<((i64, i64), i64)> {
    let mut out = vec![(hero, HERO_REVEAL)];
    out.extend(s.points.iter().filter(|p| p.active != 0).map(|p| ((i64::from(p.x), i64::from(p.y)), i64::from(p.radius))));
    out
}

/// One patrol: the army's id and the cells of its patrol (at least two), in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Patrol {
    pub army: String,
    pub cells: Vec<(i32, i32)>,
}

/// The patrols the package's triggers give to map `map`: the `army_goal`s with a `patrol` that
/// apply to it (a trigger with no map applies to every map).
pub fn patrols(triggers: &[Located<Trigger>], map: &str) -> Vec<Patrol> {
    let mut out = Vec::new();
    for t in triggers.iter().map(|l| &l.value).filter(|t| t.map.as_deref().is_none_or(|m| m == map)) {
        for action in &t.then {
            if let Action::ArmyGoal { army, goal: GoalSpec::Patrol(cells) } = action {
                out.push(Patrol { army: army.clone(), cells: cells.clone() });
            }
        }
    }
    out
}

/// The derived layers of one compiled map, computed once per compile.
#[derive(Clone, Debug, Default)]
pub struct Layers {
    pub grid: Vec<u8>,
    pub impassable: Vec<bool>,
    pub revealed: Vec<bool>,
    pub patrols: Vec<Patrol>,
}

impl Layers {
    /// The layers of `s` (the hero's start cell `hero`) and the map's patrols.
    pub fn of(s: &Scenario, hero: (i64, i64), patrols: Vec<Patrol>) -> Layers {
        let marks = marks(s);
        Layers {
            grid: grid(s, &marks),
            impassable: impassable(s, &marks),
            revealed: revealed(s.width(), s.height(), &start_sources(s, hero)),
            patrols,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Building, MapObject};

    fn map(w: u32, h: u32, fill: u8) -> Scenario {
        Scenario { header: crate::dt::dtm::Header { width: w, height: h, ..Default::default() }, terrain: vec![fill; (w * h) as usize], ..Default::default() }
    }

    #[test]
    fn impassable_cells_follow_the_original_rule_on_a_known_layout() {
        // Row 0: road, lava, deep sea, a mountain (size 2 ends at (3, 1)), marsh, drifts.
        let mut s = map(6, 2, 4);
        s.terrain[1] = 3;
        s.terrain[2] = 2;
        s.terrain[4] = 8;
        s.terrain[5] = 15;
        s.terrain[7] = 2;
        s.objects = vec![MapObject { x: 3, y: 1, class: 5, sprite: 20 }];
        let m = marks(&s);
        let imp = impassable(&s, &m);
        assert_eq!(imp[..6], [false, true, true, true, false, true], "road walks, lava, sea, the mountain's cell, marsh walks, drifts block");
        assert!(imp[6 + 2] && imp[6 + 3], "the mountain of size 2 ends at (3, 1): its cells in the row below are blocked too");
        let g = grid(&s, &m);
        assert_eq!(g[0], GRID_NORMAL);
        assert_eq!(g[4], GRID_HARD, "marsh is hard");
        assert_eq!(g[2], GRID_IMPASSABLE, "the mountain's mark wins over the sea");
        assert_eq!(g[1], GRID_IMPASSABLE);
        assert_eq!(g[7], GRID_DEEP, "sea with no object is deep");
        assert_eq!(g[6], GRID_NORMAL, "road under nothing");
    }

    #[test]
    fn a_low_hill_or_tree_adds_a_step_and_a_building_overrides() {
        let mut s = map(4, 4, 5);
        s.terrain[5] = 8; // marsh (hard) under a tree: harder
        s.objects = vec![MapObject { x: 1, y: 1, class: 9, sprite: 3 }, MapObject { x: 0, y: 0, class: 9, sprite: 3 }, MapObject { x: 2, y: 2, class: 1, sprite: 10 }];
        s.buildings.push(Building { x: 3, y: 3, size_x: 1, size_y: 1, ..Building::default() });
        let m = marks(&s);
        let g = grid(&s, &m);
        assert_eq!(g[5], GRID_HARDER, "marsh under a tree");
        assert_eq!(g[0], GRID_HARD, "normal ground under a tree");
        assert_eq!(g[2 * 4 + 2], GRID_HARD, "normal ground under a hill");
        assert_eq!(g[3 * 4 + 3], GRID_BUILDING);
        assert!(!impassable(&s, &m)[(3 * 4 + 3) as usize], "a building cell is walkable");
    }

    #[test]
    fn the_start_fog_is_the_hero_circle_and_the_lit_lanterns() {
        let mut s = map(12, 12, 6);
        s.points.push(crate::dt::dtm::Point { x: 9, y: 9, radius: 1, active: 1, ..Default::default() });
        let src = start_sources(&s, (5, 5));
        assert_eq!(src, vec![((5, 5), 5), ((9, 9), 1)]);
        let rev = revealed(12, 12, &src);
        let at = |x: usize, y: usize| rev[y * 12 + x];
        assert!(at(5, 5) && at(7, 6) && at(10, 5) && !at(11, 5), "the hero's circle: 5 cells right is in, 6 is out");
        assert!(at(10, 9) && !at(11, 11), "the lantern's radius 1");
        assert!(!at(0, 0), "far from both");
    }

    #[test]
    fn the_hero_reveal_is_a_rounded_circle_of_radius_five() {
        let rev = revealed(20, 20, &[((10, 10), 5)]);
        let count = rev.iter().filter(|r| **r).count();
        // Rounded distance at most 5: the circle of the original's reveal (not a square).
        assert!(rev[10 * 20 + 15] && !rev[15 * 20 + 15] && !rev[14 * 20 + 14], "(5,0) inside; (5,5) is 7.07 away and (4,4) 5.66 rounds to 6");
        assert!(count > 60 && count < 100, "{count}");
    }

    #[test]
    fn patrols_are_the_army_goals_of_the_triggers_that_apply() {
        use crate::pack::triggers::{Event, Trigger};
        let mk = |map: Option<&str>, cells: Vec<(i32, i32)>| Located {
            file: "triggers/x.toml".into(),
            line: 1,
            value: Trigger {
                id: "t".into(),
                map: map.map(str::to_string),
                when: Event::GameStart,
                cond: Vec::new(),
                then: vec![Action::ArmyGoal { army: "guard".into(), goal: GoalSpec::Patrol(cells) }],
                once: true,
                chance: 100,
            },
        };
        let list = vec![mk(Some("brod"), vec![(1, 1), (5, 1)]), mk(Some("other"), vec![(2, 2), (3, 3)]), mk(None, vec![(9, 9), (9, 1)])];
        let got = patrols(&list, "brod");
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].cells, vec![(1, 1), (5, 1)]);
        assert_eq!(got[1].cells, vec![(9, 9), (9, 1)]);
    }
}
