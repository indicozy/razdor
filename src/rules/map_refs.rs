//! The unit types and items a scenario names, and those the content lacks.
//!
//! `Content::unit` and `Content::item` panic on an id they do not have, which a map made for a
//! mod the install lacks does ("no unit 104"). So a map is checked before it starts (the
//! new-game window refuses it with the ids) and when it is imported (the import warns). The
//! install's own maps name only the install's content (a test checks this with the real files).
//! A save is checked the same way before it is restored ([`unknown_in_game`] for the save's state,
//! and [`unknown`] for its map): a save made with another mod is refused with the ids, not loaded.
//! That is Razdor's crash guard, not the original's.

use super::content::{Content, ItemId, UnitId};
use super::game::Game;
use crate::dt::dtm::{Scenario, Troop};
use crate::trf;

/// The ids a scenario names that the content does not have, sorted, without repeats.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Unknown {
    pub units: Vec<u32>,
    pub items: Vec<u32>,
}

impl Unknown {
    pub fn is_empty(&self) -> bool {
        self.units.is_empty() && self.items.is_empty()
    }

    /// The words for the ids, as the notices say them: «units 104, 105; items 35».
    pub fn text(&self) -> String {
        let units = self.units.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        let items = self.items.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
        match (self.units.is_empty(), self.items.is_empty()) {
            (false, false) => trf!("units {units}; items {items}", units, items),
            (false, true) => trf!("units {units}", units),
            _ => trf!("items {items}", items),
        }
    }
}

/// The unit types and items of `s` that `content` does not have. An empty slot (0) names nothing.
pub fn unknown(s: &Scenario, content: &Content) -> Unknown {
    let (units, items) = references(s);
    collect(units, items, content)
}

/// The unit types and items a game names that `content` does not have: its state (what a save
/// holds) and not its map (which [`unknown`] checks).
pub fn unknown_in_game(g: &Game, content: &Content) -> Unknown {
    let (units, items) = game_references(g);
    collect(units, items, content)
}

fn collect(units: Vec<u32>, items: Vec<u32>, content: &Content) -> Unknown {
    let mut out = Unknown {
        units: units.into_iter().filter(|&id| id != 0 && content.try_unit(UnitId(id)).is_none()).collect(),
        items: items.into_iter().filter(|&id| id != 0 && content.try_item(ItemId(id)).is_none()).collect(),
    };
    out.units.sort_unstable();
    out.units.dedup();
    out.items.sort_unstable();
    out.items.dedup();
    out
}

/// Every unit type and item id a game's state names: the squad and the units on the stations
/// with their item cells and potions, the pack, the armies, the garrisons, the gang, the
/// respawns, the recruits, the treasure and the shops' goods. Each place has a case in
/// `game_references_cover_every_place`.
fn game_references(g: &Game) -> (Vec<u32>, Vec<u32>) {
    let mut units: Vec<u32> = Vec::new();
    let mut items: Vec<u32> = Vec::new();
    let w = &g.world;
    let stationed = w.locations.iter().flat_map(|l| l.stationed.iter().map(|s| &s.unit));
    for u in g.squad.iter().chain(stationed) {
        units.push(u.def.0);
        items.extend(u.items.iter().flatten().chain(&u.potions).map(|i| i.0));
    }
    items.extend(g.pack.iter().map(|i| i.0));
    let armies = || w.armies.iter().chain(w.inactive.iter()).chain(w.respawns.iter().map(|r| &r.army));
    units.extend(w.locations.iter().flat_map(|l| l.garrison.iter()).chain(armies().flat_map(|a| a.troops.iter())).chain(w.gang.iter()).map(|t| t.unit.0));
    for a in armies() {
        items.extend(a.items.iter().map(|i| i.0));
    }
    for l in &w.locations {
        units.extend(l.recruits.iter().map(|r| r.unit.0));
        items.extend(l.treasure.iter().map(|i| i.0));
        if let Some(s) = &l.shop {
            items.extend(s.goods().iter().map(|i| i.0));
        }
    }
    (units, items)
}

/// Every unit type and item id the scenario names (empty slots included: [`unknown`] drops them).
/// The places the rules read: armies and their troops and items, the heroes' presets, named
/// characters, the buildings' items, barracks and garrisons, and the events' conditions, the units
/// and items they add or remove, the new hero class and the picture. Each source has a test in
/// `every_id_field_of_a_map_is_checked`.
fn references(s: &Scenario) -> (Vec<u32>, Vec<u32>) {
    let mut units: Vec<u32> = Vec::new();
    let mut items: Vec<u32> = Vec::new();
    let troops = |ts: &[Troop], out: &mut Vec<u32>| out.extend(ts.iter().filter(|t| !t.is_empty()).map(|t| u32::from(t.unit)));
    for army in &s.armies {
        units.push(u32::from(army.leader_unit));
        troops(&army.troops, &mut units);
        items.extend(army.artifacts.iter().map(|&a| u32::from(a)));
    }
    for hero in &s.header.heroes {
        troops(&hero.troops, &mut units);
        items.extend(hero.artifacts.iter().map(|&a| u32::from(a)));
    }
    units.extend(s.named_characters.iter().map(|n| u32::from(n.unit)));
    for building in &s.buildings {
        items.extend(building.artifact_slots.iter().map(|&a| u32::from(a)));
        units.extend(building.barracks.iter().map(|r| u32::from(r.unit)));
        troops(&building.garrison, &mut units);
    }
    for event in &s.events {
        // Conditions: a unit type or an artifact the event checks for.
        units.extend(event.conditions.units.iter().map(|&u| u32::from(u)));
        items.extend(event.conditions.artifacts.iter().map(|&a| u32::from(a)));
        // 0xFE and 0xFF are not types: "added by an event" and "any unit" (dtm.rs, units_remove).
        units.extend(event.results.units_add.iter().map(|&u| u32::from(u)));
        units.extend(event.results.units_remove.iter().filter(|&&u| u < 0xFE).map(|&u| u32::from(u)));
        items.extend(event.results.artifacts_add.iter().map(|&a| u32::from(a)));
        items.extend(event.results.artifacts_remove.iter().map(|&a| u32::from(a)));
        // The new hero class is a unit type (dtm.rs, new_hero_class); the picture is a unit type
        // unless it is one of the two end pictures (200 defeat, 201 victory) or none (0).
        units.push(u32::from(event.results.new_hero_class));
        if !matches!(event.results.picture, 0 | 200 | 201) {
            units.push(u32::from(event.results.picture));
        }
    }
    (units, items)
}

#[cfg(test)]
mod tests {
    use super::super::content::testkit;
    use super::*;
    use crate::dt::dtm::Scenario;

    fn scenario() -> Scenario {
        Scenario::default()
    }

    fn content() -> Content {
        testkit::content(vec![testkit::warrior(1, 3, 3), testkit::warrior(2, 4, 4)], vec![testkit::item(7, crate::dt::data::ArtefactType::Ring)])
    }

    #[test]
    fn a_map_of_the_install_content_names_nothing_unknown() {
        let mut s = scenario();
        s.armies.push(crate::dt::dtm::Army { leader_unit: 1, troops: [Troop { unit: 2, level: 1, count: 3 }, Troop::default(), Troop::default(), Troop::default(), Troop::default(), Troop::default()], artifacts: [7, 0, 0], ..Default::default() });
        assert!(unknown(&s, &content()).is_empty());
    }

    #[test]
    fn units_and_items_the_content_lacks_are_listed_once_each() {
        let mut s = scenario();
        s.armies.push(crate::dt::dtm::Army { leader_unit: 104, troops: [Troop { unit: 104, level: 1, count: 2 }, Troop { unit: 105, level: 1, count: 1 }, Troop::default(), Troop::default(), Troop::default(), Troop::default()], artifacts: [35, 0, 0], ..Default::default() });
        let u = unknown(&s, &content());
        assert_eq!(u.units, vec![104, 105]);
        assert_eq!(u.items, vec![35]);
    }

    #[test]
    fn an_empty_troop_or_slot_names_nothing() {
        let mut s = scenario();
        s.armies.push(crate::dt::dtm::Army { troops: [Troop { unit: 0, level: 0, count: 0 }, Troop::default(), Troop::default(), Troop::default(), Troop::default(), Troop::default()], ..Default::default() });
        assert!(unknown(&s, &content()).is_empty());
    }

    #[test]
    fn event_results_name_their_units_and_items_but_not_the_special_units() {
        let mut s = scenario();
        let mut e = crate::dt::dtm::Event::default();
        e.results.units_add = [9, 0, 0, 0];
        e.results.units_remove = [0xFE, 0xFF, 0, 0];
        e.results.artifacts_remove = [8, 0, 0, 0];
        s.events.push(e);
        let u = unknown(&s, &content());
        assert_eq!(u.units, vec![9]);
        assert_eq!(u.items, vec![8]);
    }

    /// Every place a map can name a unit type or an item, each with an id the content lacks: the
    /// unknown list must name it. (A field missed here would reach `Content::unit` and panic.)
    #[test]
    fn every_id_field_of_a_map_is_checked() {
        // (the field, a scenario that fills it with an id the content lacks, whether it is a unit, the id)
        type Fill = fn(&mut Scenario);
        let cases: Vec<(&str, Fill, bool, u32)> = vec![
            ("army leader", |s| s.armies.push(crate::dt::dtm::Army { leader_unit: 200, ..Default::default() }), true, 200),
            ("army troop", |s| s.armies.push(crate::dt::dtm::Army { troops: [Troop { unit: 200, level: 1, count: 1 }, Troop::default(), Troop::default(), Troop::default(), Troop::default(), Troop::default()], ..Default::default() }), true, 200),
            ("army item", |s| s.armies.push(crate::dt::dtm::Army { artifacts: [200, 0, 0], ..Default::default() }), false, 200),
            ("hero troop", |s| s.header.heroes[0].troops[0] = Troop { unit: 200, level: 1, count: 1 }, true, 200),
            ("hero item", |s| s.header.heroes[0].artifacts[0] = 200, false, 200),
            ("named character", |s| s.named_characters.push(crate::dt::dtm::NamedCharacter { unit: 200, name: String::new() }), true, 200),
            ("building item slot", |s| s.buildings.push(crate::dt::dtm::Building { artifact_slots: [202; 64], ..Default::default() }), false, 202),
            ("building barracks", |s| s.buildings.push(crate::dt::dtm::Building { barracks: [crate::dt::dtm::RecruitSlot { unit: 200, start_count: 1, max_count: 1 }, Default::default(), Default::default(), Default::default(), Default::default(), Default::default()], ..Default::default() }), true, 200),
            ("building garrison", |s| s.buildings.push(crate::dt::dtm::Building { garrison: [Troop { unit: 200, level: 1, count: 1 }, Troop::default(), Troop::default(), Troop::default(), Troop::default(), Troop::default()], ..Default::default() }), true, 200),
            ("event condition unit", |s| s.events.push(crate::dt::dtm::Event { conditions: crate::dt::dtm::EventConditions { units: [200, 0, 0], ..Default::default() }, ..Default::default() }), true, 200),
            ("event condition item", |s| s.events.push(crate::dt::dtm::Event { conditions: crate::dt::dtm::EventConditions { artifacts: [200, 0, 0], ..Default::default() }, ..Default::default() }), false, 200),
            ("event unit added", |s| s.events.push(crate::dt::dtm::Event { results: crate::dt::dtm::EventResults { units_add: [200, 0, 0, 0], ..Default::default() }, ..Default::default() }), true, 200),
            ("event unit removed", |s| s.events.push(crate::dt::dtm::Event { results: crate::dt::dtm::EventResults { units_remove: [200, 0, 0, 0], ..Default::default() }, ..Default::default() }), true, 200),
            ("event item given", |s| s.events.push(crate::dt::dtm::Event { results: crate::dt::dtm::EventResults { artifacts_add: [200, 0, 0, 0], ..Default::default() }, ..Default::default() }), false, 200),
            ("event item taken", |s| s.events.push(crate::dt::dtm::Event { results: crate::dt::dtm::EventResults { artifacts_remove: [200, 0, 0, 0], ..Default::default() }, ..Default::default() }), false, 200),
            ("event new hero class", |s| s.events.push(crate::dt::dtm::Event { results: crate::dt::dtm::EventResults { new_hero_class: 200, ..Default::default() }, ..Default::default() }), true, 200),
            ("event picture", |s| s.events.push(crate::dt::dtm::Event { results: crate::dt::dtm::EventResults { picture: 202, ..Default::default() }, ..Default::default() }), true, 202),
        ];
        for (field, fill, is_unit, id) in cases {
            let mut s = scenario();
            fill(&mut s);
            let u = unknown(&s, &content());
            let named = if is_unit { &u.units } else { &u.items };
            assert!(named.contains(&id), "{field}: {id} must be named, got {u:?}");
        }
    }

    /// The install's own maps name only the install's content (skipped without `RAZDOR_DT_DIR`).
    #[test]
    fn the_install_maps_name_only_the_install_content() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let Ok(dt) = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)) else { return };
        let content = Content::from_dt(&dt);
        for map in &dt.maps {
            let Ok(s) = map.load() else { continue };
            let u = unknown(&s, &content);
            assert!(u.is_empty(), "{}: units {:?}, items {:?}", map.name, u.units, u.items);
        }
    }

    /// The Evolution Mod's maps name only its content (skipped without `RAZDOR_EVO_DIR`).
    #[test]
    fn the_evolution_mod_maps_name_only_its_content() {
        let Some(dt) = crate::dt::install::evolution_mod::evo_install() else { return };
        let content = Content::from_dt(&dt);
        assert!(!dt.maps.is_empty());
        for map in &dt.maps {
            let s = map.load().unwrap_or_else(|e| panic!("{}: {e}", map.name));
            let u = unknown(&s, &content);
            assert!(u.is_empty(), "{}: units {:?}, items {:?}", map.name, u.units, u.items);
        }
    }

    /// A game of the install content names nothing unknown, whatever its state.
    #[test]
    fn an_intact_game_names_nothing_unknown() {
        use crate::rules::content::HeroClass;
        let c = std::sync::Arc::new(Content::builtin());
        let g = Game::new(c.clone(), HeroClass::Knight);
        assert!(unknown_in_game(&g, &c).is_empty());
    }

    /// Every place a game's state names a unit type or an item, each with an id the content lacks:
    /// the unknown list must name it. (A field missed here would reach `Content::unit` and panic.)
    #[test]
    fn game_references_cover_every_place() {
        use crate::rules::content::HeroClass;
        use crate::rules::world::Troop as WorldTroop;
        let known = std::sync::Arc::new(Content::builtin());
        let content = testkit::content(vec![testkit::warrior(1, 3, 3)], vec![testkit::item(7, crate::dt::data::ArtefactType::Ring)]);
        let fresh = || Game::new(known.clone(), HeroClass::Knight);
        // (the place, a game with that place holding an id the content lacks, whether it is a unit, the id)
        type Fill = fn(&mut Game);
        let cases: Vec<(&str, Fill, bool)> = vec![
            ("squad unit", |g| g.squad[0].def = UnitId(200), true),
            ("squad item", |g| g.squad[0].items[0] = Some(ItemId(200)), false),
            ("squad potion", |g| g.squad[0].potions.push(ItemId(200)), false),
            ("station unit", |g| {
                let unit = g.squad[0].clone();
                g.world.locations[0].stationed.push(crate::rules::world::Stationed { unit: crate::rules::units::Unit { def: UnitId(200), ..unit } });
            }, true),
            ("army troop", |g| g.world.armies[0].troops[0].unit = UnitId(200), true),
            ("army item", |g| g.world.armies[0].items.push(ItemId(200)), false),
            ("garrison troop", |g| {
                let t = g.world.armies[0].troops[0].clone();
                g.world.locations[0].garrison.push(WorldTroop { unit: UnitId(200), ..t });
            }, true),
            ("gang troop", |g| {
                let t = g.world.armies[0].troops[0].clone();
                g.world.gang.push(WorldTroop { unit: UnitId(200), ..t });
            }, true),
            ("recruit", |g| {
                if let Some(r) = g.world.locations.iter_mut().find(|l| !l.recruits.is_empty()) {
                    r.recruits[0].unit = UnitId(200);
                }
            }, true),
            ("treasure", |g| g.world.locations[0].treasure.push(ItemId(200)), false),
        ];
        for (place, fill, is_unit) in cases {
            let mut g = fresh();
            fill(&mut g);
            let u = unknown_in_game(&g, &content);
            let named = if is_unit { &u.units } else { &u.items };
            assert!(named.contains(&200), "{place}: 200 must be named, got {u:?}");
        }
    }
}
