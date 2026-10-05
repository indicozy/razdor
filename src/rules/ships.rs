//! Ships (`docs/reference/mechanics.md` §5.2, `original-mechanics/world.md` §8): a shipyard
//! sells the hero a ship for `ShipCost` gold (`_Global.ini [Costs]`); with it he sails the
//! shallows and coastal water (deep sea blocks ships too) and lands on the shore. One ship
//! at a time: buying another loses the old one.
//!
//! As the original:
//! - Buying puts no ship on the water: the hero's planner switches to the MIXED map (water
//!   at its own cost — coastal 1, shallows 2 — land at 5× its cost, building footprints 6),
//!   so from the shipyard he can route onto the water next to it. Stepping out of a building
//!   onto water puts him at sea; leaving the shipyard on land loses the purchase.
//! - At sea he plans on MIXED and a step takes the water's cost times his speed (coastal 5
//!   minutes, shallows 10; the ranger 4 and 8). Bridges close to him only when he clicks
//!   land or stands on one.
//! - Land or a building ahead ends his route on it: he lands and the ship is parked on the
//!   water he left, where a click on it takes him back aboard. The land test reads a cell
//!   further south (the original's bug, kept: [`World::landing_terrain_is_land`]).
//!
//! The scenario's own ships are the armies placed on water (ai.md §13): the same AI as on
//! land, on the SHIP map (water and building footprints); army byte 72 (hero, pirate and
//! merchant ships) only picks the picture. [`World::mooring`] (the water nearest a shipyard
//! on foot) serves only the reachability check of the editor's tools.

use serde::{Deserialize, Serialize};

use super::game::Game;
use super::map::{is_water, Tile, ROAD};
use super::world::{Army, LocationKind, World};

/// At sea the original's MIXED map prices land at this many times its cost.
pub const MIXED_LAND_FACTOR: u16 = 5;
/// How many steps on foot from a shipyard its ship may wait *(guess)*.
pub const MOORING_RADIUS: i32 = 24;

/// Ship types (`.DTm` army byte 72).
pub mod kind {
    pub const HERO: u8 = 1;
    pub const PIRATE: u8 = 2;
    pub const MERCHANT: u8 = 3;
}

/// The hero's rented ship.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ship {
    /// Where it is: under the hero while he is aboard, else where he left it.
    pub tile: Tile,
    pub aboard: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShipError {
    /// The hero is not in a shipyard.
    NoShipyard,
    NotEnoughGold,
}

impl World {
    /// Open water a ship can sail on.
    pub fn is_sea(&self, t: Tile) -> bool {
        self.map.mask_index(t).and_then(|i| self.sea.get(i).copied()).unwrap_or(false)
    }

    /// The water cell nearest to `t` within `radius` steps (`t` itself if it is water).
    pub fn nearest_sea(&self, t: Tile, radius: i32) -> Option<Tile> {
        let g = self.map.grid;
        (0..=radius).find_map(|r| {
            let ring = g.disk(t, r).into_iter().filter(|&n| g.distance(t, n) == r && self.is_sea(n));
            ring.min_by(|&a, &b| g.step_length(t, a).total_cmp(&g.step_length(t, b)).then((a.1, a.0).cmp(&(b.1, b.0))))
        })
    }

    /// Where the ship of shipyard `l` waits: the water next to the land nearest the entry on
    /// foot (within [`MOORING_RADIUS`] steps; a few shipyards stand well inland), the water
    /// cell nearest the entry among equals.
    pub fn mooring(&self, l: usize) -> Option<Tile> {
        let map = &self.map;
        let g = map.grid;
        let entry = self.locations.get(l)?.tile;
        let mut seen = std::collections::HashSet::from([entry]);
        let mut layer = vec![entry];
        for _ in 0..=MOORING_RADIUS {
            let best = layer
                .iter()
                .flat_map(|&t| g.neighbours(t))
                .filter(|&n| self.is_sea(n))
                .min_by(|&a, &b| g.step_length(entry, a).total_cmp(&g.step_length(entry, b)).then((a.1, a.0).cmp(&(b.1, b.0))));
            if best.is_some() {
                return best;
            }
            let mut next = Vec::new();
            for &t in &layer {
                for n in g.neighbours(t) {
                    if map.passable(n) && seen.insert(n) {
                        next.push(n);
                    }
                }
            }
            layer = next;
        }
        None
    }

    /// Cost units of cell `t` on the original's MIXED map (world.md §1.1): water at its ship
    /// cost (coastal 1, shallows 2), land at [`MIXED_LAND_FACTOR`]× its cost, every building
    /// footprint twice a road; 0 where it is blocked.
    pub fn mixed_cost(&self, t: Tile) -> u16 {
        let map = &self.map;
        if !map.in_bounds(t) {
            0
        } else if self.location_covering(t).is_some() {
            2 * ROAD
        } else if is_water(map.surface(t)) {
            map.water_cost(t).unwrap_or(0)
        } else {
            map.cost(t).map_or(0, |c| MIXED_LAND_FACTOR * c)
        }
    }

    /// The landing test's terrain (world.md §4.2 d): the original reads row `(y·(W+8) + x)
    /// div (H+2)` of the bordered terrain, column `x`, and calls it land when its code is 3 or
    /// more (its bug: the row should be `y`). Row `H` is the copied bottom border (row `H−1`);
    /// rows beyond the buffer count as water *(guess: the original reads past its terrain
    /// there, into data that is mostly zero, the shallows' code)*.
    pub fn landing_terrain_is_land(&self, (x, y): Tile) -> bool {
        let (w, h) = (self.map.w, self.map.h);
        let row = (y * (w + 8) + x) / (h + 2);
        let row = if row == h { h - 1 } else { row };
        row < h && !is_water(self.map.surface((x, row)))
    }

    /// Cost units of a ship army's step onto `to`: the original's SHIP map (shallows 2,
    /// coastal water 1, building footprints road), `None` elsewhere.
    pub fn sea_step(&self, to: Tile) -> Option<u16> {
        self.map.water_cost(to)
    }

    /// Where a waiting army comes onto the map: its post, or the nearest cell of its kind
    /// (water for a ship, walkable land otherwise) within [`super::world::PLACE_RADIUS`].
    pub fn placement(&self, a: &Army) -> Option<Tile> {
        let r = super::world::PLACE_RADIUS;
        if a.sails() {
            self.nearest_sea(a.post, r)
        } else if self.map.passable(a.post) {
            Some(a.post)
        } else {
            self.map.nearest_passable(a.post, r)
        }
    }

    /// Cells the hero can reach from `start` on foot and by ship, ignoring the fog, armies and
    /// money: walking, renting a ship at every shipyard he reaches (it waits at its
    /// [`World::mooring`]), sailing that ship's waters and landing on any coast, as a `w*h`
    /// mask. Scripted events (a teleport, a bridge built) are not considered.
    pub fn reachable_with_ships(&self, start: Tile) -> Vec<bool> {
        let map = &self.map;
        let mut reach = vec![false; (map.w * map.h).max(0) as usize];
        let mut used = vec![false; self.locations.len()];
        let mut seeds = vec![start];
        loop {
            let mut stack = Vec::new();
            for s in seeds.drain(..) {
                if let Some(i) = map.mask_index(s) {
                    if !reach[i] {
                        reach[i] = true;
                        stack.push(s);
                    }
                }
            }
            while let Some(t) = stack.pop() {
                let at_sea = self.is_sea(t);
                for n in map.grid.neighbours(t) {
                    let Some(j) = map.mask_index(n) else { continue };
                    // Land is walked onto from anywhere; water only from water (the ship).
                    if !reach[j] && (map.passable(n) || (at_sea && self.is_sea(n))) {
                        reach[j] = true;
                        stack.push(n);
                    }
                }
            }
            for (l, loc) in self.locations.iter().enumerate() {
                let entry = map.mask_index(loc.tile).is_some_and(|i| reach[i]);
                if loc.kind == LocationKind::Shipyard && entry && !used[l] {
                    used[l] = true;
                    seeds.extend(self.mooring(l));
                }
            }
            if seeds.is_empty() {
                return reach;
            }
        }
    }
}

impl Game {
    /// Gold a shipyard asks for a ship (`ShipCost`).
    pub fn ship_price(&self) -> i32 {
        self.content.options.ship_cost.max(0)
    }

    /// The shipyard the hero stands in. Every shipyard serves him: the original opens its
    /// ship window for any building of type 9 (0x4bbc84) and buys with no attitude or owner
    /// test (0x4c60ac).
    pub fn shipyard_here(&self) -> Option<usize> {
        let l = self.location?;
        (self.world.locations[l].kind == LocationKind::Shipyard).then_some(l)
    }

    /// Buys a ship at the shipyard here for [`Game::ship_price`] (world.md §8, 0x4c60ac): any
    /// ship he had is gone (one ship only) and no ship appears yet; his planner switches to
    /// the MIXED map, so from the shipyard he can route onto the water next to it. Stepping
    /// out onto the water puts him at sea; leaving the shipyard on land loses the purchase.
    pub fn rent_ship(&mut self) -> Result<(), ShipError> {
        self.shipyard_here().ok_or(ShipError::NoShipyard)?;
        let price = self.ship_price();
        if self.gold < price {
            return Err(ShipError::NotEnoughGold);
        }
        self.gold -= price;
        self.ship = None;
        self.ship_bought = true;
        Ok(())
    }

    /// The hero is at sea (on his ship).
    pub fn aboard(&self) -> bool {
        self.ship.is_some_and(|s| s.aboard)
    }

    /// Where his ship waits for him, when he is ashore.
    pub fn parked_ship(&self) -> Option<Tile> {
        self.ship.filter(|s| !s.aboard).map(|s| s.tile)
    }

    /// The hero's planner works on the MIXED map: at sea, or with a ship just bought and not
    /// yet a step taken (0x4c60ac, 0x497c68).
    pub fn plans_at_sea(&self) -> bool {
        self.aboard() || self.ship_bought
    }

    /// Cost units of cell `t` on the hero's planner map (world.md §1.2): LAND on foot, MIXED
    /// at sea ([`World::mixed_cost`]); 0 where he cannot go.
    pub fn planner_cost(&self, t: Tile) -> u16 {
        if self.plans_at_sea() {
            self.world.mixed_cost(t)
        } else {
            self.world.map.cost(t).unwrap_or(0)
        }
    }

    /// The hero's cell changes from `old` to `new` (world.md §7.2, §8; 0x497c68): the
    /// building he is in (entered when he arrives on one of its cells from another building
    /// cell, or stays on it: `old == new`), and whether he is at sea: out of a building, on
    /// water he is, on land he is not (a ship he had under him is then lost: it is parked
    /// only by a landing, [`Game::landing`]); moving inside a shipyard with a ship just
    /// bought puts him at sea there.
    pub(crate) fn move_to_cell(&mut self, old: Tile, new: Tile) {
        let w = &self.world;
        let (ob, nb) = (w.location_covering(old), w.location_covering(new));
        let building = |b: Option<usize>| b.filter(|&l| !w.locations[l].kind.is_bridge());
        let mut at_sea = self.aboard();
        if nb.is_some() && nb == ob && building(nb).is_some_and(|l| w.locations[l].kind == LocationKind::Shipyard) && self.plans_at_sea() {
            at_sea = true;
        }
        // His next step is priced now, on the map the at-sea flag chooses before the water
        // below him updates it (0x497c68): stepping off land onto the water, that is LAND,
        // where water costs 0.
        let cost = if at_sea { w.mixed_cost(new) } else { w.map.cost(new).unwrap_or(0) };
        self.step_base = Some(u32::from(cost) * self.hero_speed());
        let w = &self.world;
        if nb.is_none() {
            at_sea = is_water(w.map.surface(new));
        }
        match building(nb) {
            Some(l) if ob.is_some() => self.location = Some(l),
            // The first footprint cell, stepped on from outside: not entered yet.
            Some(_) => {}
            None => self.location = None,
        }
        if at_sea != self.aboard() {
            self.sea_changed(at_sea);
        }
        self.ship = if at_sea { Some(Ship { tile: new, aboard: true }) } else { self.ship.filter(|s| !s.aboard) };
        self.ship_bought = false;
    }

    /// The hero goes to sea (`true`) or leaves it (0x496d28): the scenario's flag `Sea` is
    /// added (when it does not occur yet) or removed; going to sea also removes the flag
    /// `EnterShipyard`. The AI armies on the medium he is now on (ships, or land armies) lose
    /// their banked time and plan again at their next arrival.
    fn sea_changed(&mut self, at_sea: bool) {
        if let Some(engine) = self.script.as_mut() {
            engine.set_engine_flag("Sea", at_sea);
            if at_sea {
                engine.set_engine_flag("EnterShipyard", false);
            }
        }
        for a in self.world.armies.iter_mut().filter(|a| a.sails() == at_sea) {
            a.budget = 0.0;
            a.mind.countdown = 0;
        }
    }

    /// At sea, the hero's next step lands him (world.md §4.2 d, 0x4ad94c) when the cell is
    /// land or a building other than a bridge. The original's land test reads the terrain
    /// row as `cell index div (H+2)` instead of `div (W+8)`, a cell further south (its bug,
    /// reproduced: [`World::landing_terrain_is_land`]); the building test is right.
    pub(crate) fn landing(&self, next: Tile) -> bool {
        let w = &self.world;
        self.aboard() && (w.landing_terrain_is_land(next) || w.location_at(next).is_some())
    }

    /// The hero lands from `from` (world.md §8): he is no longer at sea and his ship is
    /// parked on the water cell he leaves, unless he leaves from a shipyard (then it is lost).
    pub(crate) fn land(&mut self, from: Tile) {
        let in_yard = self.world.location_at(from).is_some_and(|l| self.world.locations[l].kind == LocationKind::Shipyard);
        self.ship = (!in_yard).then_some(Ship { tile: from, aboard: false });
        self.sea_changed(false);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::dt::dtm::{BuildingType, Scenario, Surface};
    use crate::rules::content::HeroClass;
    use crate::rules::game::{Event, Foe};
    use crate::rules::world::testkit::{self as tk, army, building, hero, scenario, troop};

    /// A 30×30 map: land x 0..=9, a strait of coastal water x 10..=19 (shallows in the
    /// middle), land x 20..=29. A shipyard at (9, 5) on the shore; the knight starts at
    /// (2, 5) with 600 gold. (Square, so that the original's landing test reads the right
    /// kind of cell on the rows used here.)
    fn strait() -> Scenario {
        let mut s = scenario(30, 30);
        for y in 0..30 {
            for x in 10..20 {
                tk::set(&mut s, x, y, if (13..17).contains(&x) { Surface::ShallowsFords } else { Surface::CoastalWater });
            }
        }
        let mut yard = building(BuildingType::Shipyard, 9, 5, (1, 1));
        yard.relations = [1, 0, 0, 0];
        s.buildings = vec![yard];
        s.header.heroes[0] = hero(2, 5, 600, &[troop(4, 0, 1)]);
        s
    }

    fn start(s: &Scenario) -> Game {
        let mut g = Game::from_scenario(Arc::new(tk::content()), s, HeroClass::Knight);
        // No fog: these tests are about the ship.
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        g
    }

    fn walk_until_stopped(g: &mut Game) -> Vec<Event> {
        let mut events = Vec::new();
        for _ in 0..20_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        events
    }

    fn at_yard(s: &Scenario) -> Game {
        let mut g = start(s);
        assert!(g.set_destination((9, 5)));
        assert_eq!(walk_until_stopped(&mut g).last(), Some(&Event::Arrived(0)));
        g
    }

    /// Bought at the yard and sailed out to `(x, 5)`.
    fn at_sea(s: &Scenario, x: i32) -> Game {
        let mut g = at_yard(s);
        g.rent_ship().unwrap();
        assert!(g.set_destination((x, 5)));
        let ev = walk_until_stopped(&mut g);
        assert!(g.aboard() && g.tile() == (x, 5), "{:?} {:?} {ev:?} {:?}", g.tile(), g.ship, g.path);
        g
    }

    #[test]
    fn shallows_and_coastal_water_are_sea_and_block_on_foot() {
        let g = start(&strait());
        assert!(g.world.is_sea((10, 5)) && g.world.is_sea((15, 0)) && !g.world.is_sea((9, 5)) && !g.world.is_sea((20, 5)));
        assert!(!g.world.map.passable((10, 5)) && !g.world.map.passable((15, 5)), "no wading through the shallows");
        assert!(!g.world.is_sea((9, 5)), "the shipyard is land");
        assert!(!g.can_target((15, 5)), "no ship: water is not a target on foot");
        let mut g = g;
        assert!(!g.set_destination((25, 5)), "the far shore is out of reach on foot");
    }

    #[test]
    fn deep_sea_blocks_ships_too() {
        let mut s = strait();
        for y in 0..30 {
            tk::set(&mut s, 15, y, Surface::DeepSea);
        }
        let mut g = at_yard(&s);
        assert!(!g.world.is_sea((15, 5)));
        g.rent_ship().unwrap();
        assert!(!g.can_target((15, 5)));
        assert!(!g.set_destination((25, 5)), "no crossing over deep sea");
        assert!(g.set_destination((14, 5)), "the near waters are sailed");
    }

    #[test]
    fn buying_a_ship_puts_none_on_the_water_and_plans_on_the_mixed_map() {
        let s = strait();
        let mut g = start(&s);
        assert_eq!(g.rent_ship(), Err(ShipError::NoShipyard), "not in a shipyard");
        let mut g2 = at_yard(&s);
        use crate::rules::town::{first_tab, Tab};
        assert_eq!(first_tab(&g2.world.locations[0], &g2.content), Some(Tab::Shipyard));
        assert_eq!(g2.ship_price(), 250);
        g2.gold = 249;
        assert_eq!(g2.rent_ship(), Err(ShipError::NotEnoughGold));
        g2.gold = 600;
        assert_eq!(g2.rent_ship(), Ok(()));
        assert_eq!((g2.gold, g2.ship), (350, None), "no ship object appears (0x4c60ac)");
        assert!(g2.plans_at_sea() && !g2.aboard());
        // The MIXED map: water at its own cost, land 5×, buildings 6.
        assert_eq!([(10, 5), (14, 5), (5, 5), (9, 5)].map(|t| g2.planner_cost(t)), [1, 2, 25, 6]);
        assert!(g2.can_target((15, 5)));
        g.gold = 0;
        assert_eq!(g.ship, None);
    }

    #[test]
    fn a_shipyard_opens_its_ship_window_on_land_and_nothing_at_sea() {
        // 0x4bbc84: type 9 with the at-sea flag clear opens the ship window (0x4d3ec0), with
        // it set nothing at all.
        let s = strait();
        let mut g = at_yard(&s);
        use crate::rules::town::Tab;
        assert_eq!(g.window_at(0), Some(Tab::Shipyard));
        assert_eq!(g.tabs_here(), [Tab::Shipyard], "no main hall, no other tab");
        g.ship = Some(Ship { tile: g.tile(), aboard: true });
        assert!(g.aboard());
        assert_eq!(g.window_at(0), None);
        assert!(g.tabs_here().is_empty());
    }

    #[test]
    fn an_ill_disposed_shipyard_rents_a_ship_too() {
        // Twelve of the shipped maps' 30 shipyards start ill-disposed (ДС1's «Старый причал» at
        // −2, Проклятое озеро's two ports at −1 …); the original's window has no attitude
        // test (0x4bbc84, 0x4c60ac).
        let mut s = strait();
        s.buildings[0].relations = [-3, 0, 0, 0];
        s.buildings[0].faction = 4;
        let mut g = at_yard(&s);
        assert!(g.world.locations[0].hostile());
        use crate::rules::town::{first_tab, Tab};
        assert_eq!(first_tab(&g.world.locations[0], &g.content), Some(Tab::Shipyard));
        assert_eq!(g.rent_ship(), Ok(()));
        assert_eq!(g.gold, 350);
        assert!(g.set_destination((12, 5)));
        walk_until_stopped(&mut g);
        assert!(g.aboard() && g.tile() == (12, 5));
    }

    #[test]
    fn stepping_out_onto_the_water_puts_him_at_sea() {
        let g = at_sea(&strait(), 12);
        assert_eq!(g.ship, Some(Ship { tile: (12, 5), aboard: true }));
        // Time: the cell left on the MIXED map, times his speed: coastal 5 min, shallows
        // 10, a building 30.
        assert_eq!(g.step_time((12, 5), (11, 5)), 5.0);
        assert_eq!(g.step_time((14, 5), (15, 5)), 10.0);
        assert_eq!(g.step_time((14, 5), (15, 6)), 15.0, "diagonal ×1.5");
        assert_eq!(g.step_time((9, 5), (10, 5)), 30.0);
    }

    /// The step time is set as he comes onto a cell (0x497c68), priced on the map his at-sea
    /// flag chose before that cell updated it: the first water cell is priced on LAND, where
    /// water costs 0, so the step after it takes no time; from there on MIXED.
    #[test]
    fn the_first_step_after_going_to_sea_takes_no_time() {
        let mut g = at_yard(&strait());
        g.rent_ship().unwrap();
        let yard = u32::from(g.world.map.cost((9, 5)).unwrap()) * g.hero_speed();
        assert!(g.set_destination((12, 5)));
        let t0 = g.clock.total_minutes();
        walk_until_stopped(&mut g);
        // Off the yard (its LAND cost, as when he came in), (10,5) → (11,5) free, then a
        // coastal step of 5.
        assert_eq!(g.clock.total_minutes() - t0, (yard + 5) as f64);
        // At the walk's end he is put on his cell again: the next step is priced on MIXED.
        assert!(g.set_destination((13, 5)));
        let t1 = g.clock.total_minutes();
        walk_until_stopped(&mut g);
        assert_eq!(g.clock.total_minutes() - t1, 5.0);
    }

    #[test]
    fn leaving_the_shipyard_on_foot_loses_the_ship() {
        let mut g = at_yard(&strait());
        g.rent_ship().unwrap();
        assert!(g.set_destination((5, 5)));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (5, 5));
        assert!(g.ship.is_none() && !g.plans_at_sea(), "the purchase is lost");
        assert!(!g.can_target((12, 5)));
    }

    /// Going to sea (0x496d28) adds `Sea` and removes `EnterShipyard`, should a script have
    /// set it.
    #[test]
    fn going_to_sea_sets_sea_and_drops_enter_shipyard() {
        let mut g = at_yard(&strait());
        g.script.as_mut().unwrap().set_flag_string("EnterShipyard\u{a0}A\u{a0}");
        g.rent_ship().unwrap();
        assert!(g.set_destination((12, 5)));
        walk_until_stopped(&mut g);
        assert!(g.aboard());
        assert_eq!(g.script().unwrap().flag_string(), "A\u{a0}Sea\u{a0}");
    }

    #[test]
    fn landing_parks_the_ship_on_the_water_he_left() {
        let mut g = at_sea(&strait(), 12);
        // A click on the far land: the route is priced on MIXED and he stops on the first
        // land cell, the ship on the water behind him.
        assert!(g.set_destination((25, 5)));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (20, 5), "the route ends where he lands");
        assert_eq!(g.ship, Some(Ship { tile: (19, 5), aboard: false }));
        assert!(!g.plans_at_sea() && g.planner_cost((19, 5)) == 0);
        assert!(!g.script().unwrap().flag("Sea"), "landing removes the flag Sea (0x496d28)");
        // On foot again; the parked ship is a target although water costs nothing on LAND.
        assert!(g.can_target((19, 5)) && !g.can_target((18, 5)));
        assert!(g.set_destination((25, 5)));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (25, 5));
        assert!(g.set_destination((19, 5)));
        walk_until_stopped(&mut g);
        assert_eq!(g.ship, Some(Ship { tile: (19, 5), aboard: true }), "walking onto it, he is at sea again");
        assert_eq!(g.script().unwrap().flag_string(), "Sea\u{a0}", "boarding adds it");
        assert!(g.can_target((15, 5)));
    }

    #[test]
    fn going_to_sea_or_ashore_empties_the_banks_of_the_armies_there() {
        // 0x496d28: boarding resets the ships' banks, landing the land armies'.
        let mut s = strait();
        let mut pirate = army(1, 15, 20, -2, &[troop(4, 0, 1)]);
        pirate.ship = kind::PIRATE;
        s.armies = vec![pirate, army(2, 3, 20, 1, &[troop(4, 0, 1)])];
        let mut g = at_yard(&s);
        g.rent_ship().unwrap();
        for a in &mut g.world.armies {
            a.budget = 100.0;
        }
        g.move_to_cell((9, 5), (10, 5));
        assert!(g.aboard());
        assert_eq!(g.world.armies.iter().map(|a| a.budget).collect::<Vec<_>>(), [0.0, 100.0]);
        g.world.armies[0].budget = 100.0;
        g.land((10, 5));
        assert_eq!(g.world.armies.iter().map(|a| a.budget).collect::<Vec<_>>(), [100.0, 0.0]);
    }

    #[test]
    fn at_sea_bridges_close_only_for_a_click_on_land_or_from_a_bridge() {
        let mut s = strait();
        // A bridge across the strait on row 8.
        for x in 10..20 {
            s.buildings.push(building(BuildingType::WoodenBridge, x, 8, (1, 1)));
        }
        let g = at_sea(&s, 12);
        assert!(!g.can_target((15, 8)), "a bridge is no target at sea");
        // To the water beyond: the route may pass the bridge (6 on MIXED).
        let p = g.plan((15, 10));
        assert!(p.iter().any(|&t| t.1 == 8), "{p:?}");
        // To land: every bridge is closed.
        let p = g.plan((25, 10));
        assert!(!p.is_empty() && p.iter().all(|&t| g.world.location_covering(t).is_none()), "{p:?}");
        // AI ships sail the SHIP map, where bridges are footprints paved as road.
        assert_eq!(g.world.sea_step((15, 8)), Some(crate::rules::map::ROAD));
    }

    /// A 10×10 sea with the hero aboard at `at`, land at the `land` cells.
    fn bay(at: Tile, land: &[Tile]) -> Game {
        let mut s = scenario(10, 10);
        for y in 0..10 {
            for x in 0..10 {
                let ground = land.contains(&(x as i32, y as i32));
                tk::set(&mut s, x, y, if ground { Surface::GrassPlain } else { Surface::CoastalWater });
            }
        }
        s.header.heroes[0] = hero(at.0 as u16, at.1 as u16, 0, &[]);
        let g = start(&s);
        assert!(g.aboard(), "a start on the water is at sea");
        g
    }

    #[test]
    fn the_landing_test_reads_a_cell_further_south() {
        // The original's bug (0x4ad94c): the row read is (y·(W+8) + x) div (H+2). On a 10×10
        // map, (5, 2) reads row 41 div 12 = 3.
        let mut g = bay((4, 2), &[(5, 2), (6, 2)]);
        assert!(!g.world.landing_terrain_is_land((5, 2)), "(5, 3) is water");
        assert!(!g.world.landing_terrain_is_land((5, 8)), "row 149 div 12 = 12: past the map, water (guess)");
        // So he is not stopped on the shore: he steps onto the land, which takes him off the
        // sea without parking the ship (it is lost), and walks on.
        assert!(g.set_destination((6, 2)));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (6, 2));
        assert_eq!(g.ship, None, "the ship is lost");
        // The other way round: land read under water stops him on the water.
        let mut g = bay((3, 2), &[(5, 3)]);
        assert!(g.world.landing_terrain_is_land((5, 2)));
        assert!(g.set_destination((7, 2)));
        assert!(g.path.contains(&(5, 2)));
        walk_until_stopped(&mut g);
        assert_eq!(g.tile(), (5, 2), "stopped on the water as if landing");
        assert!(g.aboard(), "the water there puts him at sea again");
    }

    #[test]
    fn pirates_sail_and_attack_the_hero_at_sea() {
        let mut s = strait();
        let mut pirate = army(1, 15, 0, -2, &[troop(4, 0, 1)]);
        pirate.ship = kind::PIRATE;
        // Gold for its wages: an unpaid crew does not attack (ai.md §4).
        pirate.gold_income = 500;
        let mut merchant = army(2, 11, 11, -2, &[troop(4, 0, 1)]);
        merchant.ship = kind::MERCHANT;
        s.armies = vec![pirate, merchant];
        let mut g = at_yard(&s);
        let ids: Vec<u8> = g.world.armies.iter().map(|a| a.id).collect();
        assert_eq!(ids, [1, 2]);
        assert!(g.world.armies[0].hostile() && g.world.armies[0].sails());
        assert!(!g.world.armies[1].hostile(), "merchants never attack");
        // No patrol of their own: they wander the whole sea like any army that does not
        // patrol (ai.md §13).
        assert!(!g.world.armies[0].patrols);
        // The ships cruise, on water only; nobody attacks a waiting hero.
        let before: Vec<_> = g.world.armies.iter().map(|a| a.pos).collect();
        g.wait(24);
        assert!(g.foe.is_none());
        for a in &g.world.armies {
            assert!(g.world.is_sea(a.tile(&g.world.map)), "army {} left the water", a.id);
        }
        assert_ne!(before, g.world.armies.iter().map(|a| a.pos).collect::<Vec<_>>(), "they moved");
        // Out at sea, the pirates come for the hero as he sails up and down.
        g.world.armies.retain(|a| a.id == 1);
        let a = &mut g.world.armies[0];
        a.pos = g.world.map.center((15, 1));
        a.path.clear();
        // Bold enough to attack him (the AI attacks only battles it wins), no wandering.
        a.ai.aggression = 100;
        a.ai.no_random = true;
        a.mind.clean.clear();
        g.rent_ship().unwrap();
        let mut events = Vec::new();
        for target in [(14, 5), (11, 5), (14, 5), (11, 5), (14, 5), (11, 5)] {
            if g.foe.is_some() {
                break;
            }
            g.set_destination(target);
            events.extend(walk_until_stopped(&mut g));
        }
        assert!(matches!(events.last(), Some(Event::Encounter(0))), "{events:?}");
        assert_eq!(g.foe, Some(Foe::Army(0)));
        assert!(g.world.is_sea(g.world.armies[0].tile(&g.world.map)));
    }

    #[test]
    fn ship_state_survives_a_save() {
        let s = strait();
        let g = at_sea(&s, 14);
        let json = serde_json::to_string(&g).unwrap();
        let mut h: Game = serde_json::from_str(&json).unwrap();
        h.content = g.content.clone();
        h.world.restore_statics(World::from_scenario(&s, &g.content)).unwrap();
        assert_eq!(h.ship, Some(Ship { tile: (14, 5), aboard: true }));
        assert!(h.world.is_sea((14, 5)));
        assert!(h.set_destination((25, 5)));
        walk_until_stopped(&mut h);
        assert_eq!(h.tile(), (20, 5));
        // Older saves have no ship.
        let old = json.replace(&format!(",\"ship\":{}", serde_json::to_string(&g.ship).unwrap()), "");
        assert_ne!(old, json);
        let o: Game = serde_json::from_str(&old).unwrap();
        assert_eq!(o.ship, None);
    }

    #[test]
    fn reachability_counts_rented_ships() {
        let s = strait();
        let g = start(&s);
        let w = &g.world;
        let on_foot = w.map.reachable((2, 5));
        let with_ships = w.reachable_with_ships((2, 5));
        let i = w.map.mask_index((25, 5)).unwrap();
        assert!(!on_foot[i] && with_ships[i]);
        // Without the shipyard, the far shore stays out of reach.
        let mut s = strait();
        s.buildings.clear();
        let w = World::from_scenario(&s, &tk::content());
        assert!(!w.reachable_with_ships((2, 5))[i]);
    }

    #[test]
    fn hero_starts_at_his_preset_and_gets_his_start_buildings() {
        let mut s = scenario(20, 20);
        let a = building(BuildingType::Castle, 5, 5, (2, 2));
        let mut b = building(BuildingType::Town, 15, 15, (2, 2));
        b.start_for = [0, 0, 1];
        let mut c = building(BuildingType::Tavern, 15, 4, (1, 1));
        c.start_for = [0, 1, 0];
        s.buildings = vec![a, b, c];
        // The knight's preset names building 1, far from his x/y: he stays at his x/y.
        s.header.heroes[0] = hero(18, 1, 100, &[]);
        s.header.heroes[0].start_building = 1;
        // The ranger stands in his flagged town.
        s.header.heroes[2] = hero(15, 15, 100, &[]);
        s.header.heroes[1] = hero(2, 17, 100, &[]);
        let ct = tk::content();
        let w = World::from_scenario(&s, &ct);
        let k = w.hero_start(&s, &ct, HeroClass::Knight);
        assert_eq!((k.tile, k.location, k.owned.clone()), ((18, 1), None, vec![0]));
        let r = w.hero_start(&s, &ct, HeroClass::Ranger);
        assert_eq!((r.tile, r.location, r.owned.clone()), ((15, 15), Some(1), vec![1]));
        let m = w.hero_start(&s, &ct, HeroClass::Archmage);
        assert_eq!((m.tile, m.owned.clone()), ((2, 17), vec![2]), "a flagged building anywhere is his");
        let g = Game::from_scenario(Arc::new(ct), &s, HeroClass::Knight);
        assert_eq!((g.tile(), g.location), ((18, 1), None));
        let castle = &g.world.locations[0];
        assert!(castle.owned() && castle.faction == 1 && castle.attitude == 3, "his start building is his");
        assert!(!g.world.locations[1].owned() && !g.world.locations[2].owned(), "other classes' buildings are not");
    }
}

#[cfg(test)]
mod real_maps {
    //! Ships on the player's maps; skipped without `RAZDOR_DT_DIR`. Numbers only.
    use std::sync::Arc;

    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::game::Event;

    #[test]
    fn every_shipyard_has_a_mooring() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let c = Content::from_dt(&dt);
        for m in &dt.maps {
            let s = m.load().unwrap();
            let w = World::from_scenario(&s, &c);
            for (i, l) in w.locations.iter().enumerate().filter(|(_, l)| l.kind == LocationKind::Shipyard) {
                // A shipyard with no sailable water near it (shallows count as water now) is reported.
                let water_near = w.nearest_sea(l.tile, MOORING_RADIUS).is_some();
                assert_eq!(w.mooring(i).is_some(), water_near, "{} shipyard {}", m.name, l.id);
                if !water_near {
                    eprintln!("{} shipyard {} has no sailable water near it", m.name, l.id);
                }
            }
            for a in w.armies.iter().filter(|a| a.sails()) {
                assert!(w.is_sea(a.tile(&w.map)), "{} ship {}", m.name, a.id);
            }
        }
    }

    #[test]
    fn ds1_sails_from_a_shipyard_to_a_building_out_of_reach_on_foot() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let c = Arc::new(Content::from_dt(&dt));
        let s = dt.maps.iter().find(|m| m.name.starts_with("ДС1")).unwrap().load().unwrap();
        let mut g = Game::from_scenario(c, &s, HeroClass::Knight);
        g.fog = crate::rules::fog::Fog::disabled(g.world.map.w, g.world.map.h);
        g.world.armies.clear();
        let foot = g.world.map.reachable(g.tile());
        let reached = |t: Tile| g.world.map.mask_index(t).is_some_and(|i| foot[i]);
        let yard = (0..g.world.locations.len()).find(|&i| g.world.locations[i].kind == LocationKind::Shipyard && reached(g.world.locations[i].tile)).expect("a shipyard on foot");
        let far: Vec<usize> = (0..g.world.locations.len()).filter(|&i| !g.world.locations[i].kind.is_bridge() && !reached(g.world.locations[i].tile)).collect();
        assert!(!far.is_empty(), "ДС1 has buildings beyond the water");
        // Stand in the shipyard and rent.
        g.pos = g.world.map.center(g.world.locations[yard].tile);
        g.location = Some(yard);
        g.gold = 1000;
        g.rent_ship().unwrap();
        let target = far.iter().copied().find(|&i| !g.plan(g.world.locations[i].tile).is_empty()).expect("a building across the water");
        let entry = g.world.locations[target].tile;
        assert!(g.set_destination(entry));
        let mut events = Vec::new();
        for _ in 0..100_000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
            // Scripted messages, and buildings on the way, stop the walk; keep going.
            if !g.moving() && g.location != Some(target) && g.foe.is_none() {
                g.set_destination(entry);
            }
        }
        // In it, or met by its garrison at its gate.
        assert!(g.location == Some(target) || g.foe.is_some(), "{:?} {:?}", g.tile(), g.foe);
        assert!(events.contains(&Event::Arrived(target)));
        assert!(g.ship.is_none_or(|s| !s.aboard), "landed: the ship waits, not boarded");
    }
}
