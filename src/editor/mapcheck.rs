//! The original editor's map check (DTMapEdit 0x556c68,
//! docs/reference/editor/mapcheck-files.md §2): 20 design rules, listed in a window that never
//! blocks saving. Each row names the record it is about, and "go to" opens that record.
//!
//! Razdor's own integrity check ([`super::validate`]) is separate: it refuses to write a file
//! that would not read back.

use crate::dt::dtm::{Scenario, RELATIVE_START};
use crate::i18n::{n_, tr};
use crate::trf;

use super::grid::{impassable, Marks};
use super::palette::Names;
use super::validate::Place;

/// The kind of record a row is about, in the window's first column.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckKind {
    Army,
    Building,
    Event,
    Point,
}

impl CheckKind {
    pub fn label(self) -> &'static str {
        tr(match self {
            CheckKind::Army => n_("Army"),
            CheckKind::Building => n_("Building"),
            CheckKind::Event => n_("Event"),
            CheckKind::Point => n_("Point"),
        })
    }
}

/// One row of the check window: the record's kind, id and name, and the message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckRow {
    pub kind: CheckKind,
    pub id: u16,
    pub name: String,
    pub message: String,
}

impl CheckRow {
    /// The record "go to" opens.
    pub fn place(&self) -> Place {
        match self.kind {
            CheckKind::Army => Place::Army(self.id as u8),
            CheckKind::Building => Place::Building(self.id),
            CheckKind::Event => Place::Event(self.id),
            CheckKind::Point => Place::Point(self.id),
        }
    }
}

/// The event title shown in lists: up to its first `%` (0x4dc7b8).
fn event_name(title: &str) -> String {
    title.split_once('%').map_or(title, |(t, _)| t).to_string()
}

/// Runs the 20 rules on `s`, in the original's order: the armies by id, then the
/// buildings, the events and the points, each record's rules in turn. `names` gives the
/// install's artefacts (for the dearest and cheapest price), unit prices and
/// `CostRecrutDiv`.
pub fn check_map(s: &Scenario, names: &Names) -> Vec<CheckRow> {
    let facts = &names.facts;
    // The install's artefact list (not the map's custom artefacts): the dearest price, at
    // least 0, and the cheapest absolute price among those that are not of type 9 or 10.
    let dearest = facts.artefacts.iter().map(|a| a.cost).max().unwrap_or(0).max(0);
    let cheapest = facts.artefacts.iter().filter(|a| a.kind != 9 && a.kind != 10).map(|a| a.cost.unsigned_abs() as i64).min().unwrap_or(0x7fff_ffff);
    let marks = Marks::build(s);
    let mut out = Vec::new();

    for (i, a) in s.armies.iter().enumerate() {
        let id = (i + 1) as u8;
        let mut row = |message: String| out.push(CheckRow { kind: CheckKind::Army, id: id as u16, name: a.name.clone(), message });
        // 1. On an impassable cell.
        if impassable(s, &marks, a.x as i64, a.y as i64) {
            row(tr("the army stands on an impassable place").into());
        }
        // 2. Inactive at start and nothing calls it up. A reserve (an army some event takes
        // added units from) skips rules 3 to 6; only this rule finds reserves, so an
        // active army is never one.
        let mut reserve = false;
        if a.inactive != 0 {
            let mut called = false;
            for e in &s.events {
                let r = &e.results;
                if r.activate_armies.contains(&id) || r.start_battle_with == id || r.units_from_army == id {
                    called = true;
                }
                if r.units_from_army == id {
                    reserve = true;
                }
            }
            if !called {
                row(tr("the army is never activated: no event activates it, starts a battle with it or takes units from it").into());
            }
        }
        // 3. Friendly or neutral towards the player and no event that may fire many times
        // meets it.
        if a.relations[0] >= 0 && !reserve && !s.events.iter().any(|e| e.conditions.meet_army == id && e.once == 0) {
            row(tr("there is no event for meeting the army").into());
        }
        if a.behaviour == 0 && !reserve {
            // 4. A feudal army's upkeep above its average income. The leader is not counted.
            // A `CostRecrutDiv` of 0 stops the original with a division by zero; the rule is
            // skipped then.
            if facts.recruit_div != 0 {
                let upkeep: i64 = a.troops.iter().filter(|t| t.unit != 0).map(|t| (facts.unit_cost(t.unit as u32) / facts.recruit_div) as i64 * t.count as i64).sum();
                let mut income = a.unknown_80 as i64 * 10;
                let (mut incomes, mut maxima, mut n) = (0i64, 0i64, 0i64);
                for b in &s.buildings {
                    if matches!(b.kind, 3 | 4) && b.owner_army == id {
                        income += b.gold_per_day as i64;
                    }
                    if b.kind == 2 {
                        let d = (a.x as i64 - b.x as i64).abs().max((a.y as i64 - b.y as i64).abs());
                        if d <= a.patrol_radius as i64 || a.patrols == 0 {
                            incomes += b.gold_per_day as i64;
                            maxima += b.gold_max as i64;
                            n += 1;
                        }
                    }
                }
                if n > 0 {
                    income += (incomes + maxima) / (2 * n) * n.min(3);
                }
                if income < upkeep {
                    row(trf!("the upkeep {upkeep} is above the average income {income}", upkeep, income));
                }
            }
            // 5. The patrol reaching the bottom edge, compared with the width (the original's
            // code; the shipped maps are square).
            if s.header.width as i64 <= a.y as i64 + a.patrol_radius as i64 {
                row(tr("the patrol reaches past the bottom edge of the map, which may crash the game").into());
            }
        }
        // 6. No description.
        if a.description.is_empty() && !reserve {
            row(tr("there is no description text").into());
        }
    }

    for (i, b) in s.buildings.iter().enumerate() {
        let mut row = |message: &str| out.push(CheckRow { kind: CheckKind::Building, id: (i + 1) as u16, name: b.name.clone(), message: message.into() });
        // 7. A village, castle or fort without money income.
        if matches!(b.kind, 2..=4) && b.gold_per_day == 0 {
            row(tr("no money income is set"));
        }
        // 8. No description, but for bridges.
        if b.description.is_empty() && !matches!(b.kind, 13 | 14) {
            row(tr("there is no description text"));
        }
        let market = matches!(b.kind, 1 | 6 | 7);
        // 9. A market's highest random price above the dearest artefact.
        if market && b.price_max as i32 > dearest {
            row(tr("the market's highest price is above the dearest artefact"));
        }
        // 10. A castle or ruins whose garrison holds no units.
        if matches!(b.kind, 3 | 12) && b.garrison.iter().filter(|t| t.unit != 0).map(|t| t.count as u32).sum::<u32>() == 0 {
            row(tr("the garrison is empty"));
        }
        // 11. A town or castle with no recruits.
        if matches!(b.kind, 1 | 3) && b.barracks.iter().filter(|t| t.unit != 0).map(|t| t.max_count as u32).sum::<u32>() == 0 {
            row(tr("no recruitment is set"));
        }
        // 12. A market with no fixed goods and no workable random goods.
        if market
            && b.artifact_slots.iter().all(|g| *g == 0)
            && (b.random_artifacts_for_sale == 0 || b.price_max as i32 > dearest || (b.price_min as i64) < cheapest)
        {
            row(tr("the market is not set up"));
        }
    }

    for (i, e) in s.events.iter().enumerate() {
        let id = (i + 1) as u16;
        let mut row = |message: String| out.push(CheckRow { kind: CheckKind::Event, id, name: event_name(&e.title), message });
        let in_buildings = || s.buildings.iter().any(|b| b.event_slots[..(b.event_count as usize).min(64)].contains(&id));
        let in_points = || s.points.iter().any(|p| p.event_slots[..(p.event_count as usize).min(10)].contains(&id));
        // 13–15, by type.
        match e.kind {
            2 if e.subordinate == 0 => {
                if !in_points() && !in_buildings() {
                    row(tr("a local event that no point and no building lists").into());
                }
            }
            3 => {
                if !s.events.iter().any(|o| o.results.completes_quest == id) {
                    row(tr("a quest that no event completes").into());
                }
            }
            4 if e.subordinate == 0 => {
                if !in_buildings() {
                    row(tr("a rumour that no building lists").into());
                }
            }
            _ => {}
        }
        // 16. A subordinate event no event chains to.
        if e.subordinate != 0 && !s.events.iter().any(|o| o.results.chained_event == id) {
            row(tr("a subordinate event that no event chains to").into());
        }
        // 17. A "relative only" event no event names as its relative event.
        if e.start_time == RELATIVE_START && !s.events.iter().any(|o| o.results.relative_event == id) {
            row(tr("a relative-only event that no event starts").into());
        }
        // 18. A reward of units or artefacts without a message.
        let rewards = e.results.artifacts_add.iter().chain(&e.results.units_add).filter(|x| **x != 0).count();
        if rewards > 0 && e.message.is_empty() {
            row(tr("there is no text for the reward").into());
        }
        // 19. An artefact the player must have that nothing on the map gives (hero presets,
        // building spells and garrison items are not searched).
        if e.conditions.artifacts_check != 0 {
            for k in 0..3 {
                let item = e.conditions.artifacts[k];
                if item == 0 || item == 135 || e.conditions.artifacts_owner[k] != 1 {
                    continue;
                }
                let sources = s.events.iter().filter(|o| o.results.artifacts_add.contains(&item)).count()
                    + s.armies.iter().filter(|a| a.artifacts.contains(&item)).count()
                    + s.buildings.iter().filter(|b| b.artifact_slots.contains(&(item as u16))).count();
                if sources == 0 {
                    row(trf!("the artefact {item} is not on the map, so the event can never fire", item = names.artefact(item as u32)));
                }
            }
        }
    }

    for (i, p) in s.points.iter().enumerate() {
        // 20. No events and no radius.
        if p.event_count == 0 && p.radius == 0 {
            out.push(CheckRow { kind: CheckKind::Point, id: (i + 1) as u16, name: format!("{} - {}", p.x, p.y), message: tr("an empty point: no events and no radius").into() });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, Event, MapObject, Point, Troop};
    use crate::editor::palette::{ArtefactFact, Choice, Facts};

    fn names() -> Names {
        Names {
            artefacts: vec![Choice { id: 7, name: "Меч".into() }],
            facts: Facts {
                artefacts: vec![
                    ArtefactFact { id: 1, cost: 300, kind: 0 },
                    ArtefactFact { id: 2, cost: -1200, kind: 6 },
                    ArtefactFact { id: 3, cost: 20, kind: 9 },
                    ArtefactFact { id: 7, cost: 50, kind: 1 },
                ],
                unit_costs: vec![(10, 101), (11, 40)],
                spell_fixed_hits: vec![],
                recruit_div: 2,
            },
            ..Names::default()
        }
    }

    fn map() -> Scenario {
        let mut s = Scenario::default();
        s.header.width = 20;
        s.header.height = 20;
        s.terrain = vec![6; 400];
        s
    }

    fn messages(rows: &[CheckRow], kind: CheckKind, id: u16) -> Vec<String> {
        rows.iter().filter(|r| r.kind == kind && r.id == id).map(|r| r.message.clone()).collect()
    }

    #[test]
    fn army_rules() {
        let mut s = map();
        s.terrain[5 * 20 + 5] = 2; // deep sea
        // Army 1: on deep sea, friendly, no meeting event, no description; feudal with
        // upkeep (101 div 2) × 3 + (40 div 2) × 2 = 190 above an income of 10 × 10 + 50
        // (its fort) + villages: (60 + 100 + 30 + 10) div 4 × 2 = 100, so 250 > 190: no
        // row; with more troops it shows.
        let mut a1 = Army { x: 5, y: 5, patrols: 1, patrol_radius: 3, unknown_80: 10, ..Army::default() };
        a1.troops[0] = Troop { unit: 10, level: 0, count: 3 };
        a1.troops[1] = Troop { unit: 11, level: 0, count: 2 };
        // Army 2: inactive, only a "take units from" reserve: rules 3–6 skipped.
        let a2 = Army { inactive: 1, relations: [1, 0, 0, 0], ..Army::default() };
        // Army 3: inactive and called by nothing; hostile; a rogue whose patrol reaches the
        // bottom edge (no row: only feudal armies are tested).
        let a3 = Army { inactive: 1, relations: [-1, 0, 0, 0], behaviour: 1, y: 18, patrol_radius: 2, description: "d".into(), ..Army::default() };
        // Army 4: feudal at y 17 with radius 3 reaches the width.
        let a4 = Army { x: 1, y: 17, patrol_radius: 3, relations: [-2, 0, 0, 0], description: "d".into(), ..Army::default() };
        s.armies = vec![a1, a2, a3, a4];
        let fort = Building { x: 9, y: 9, kind: 4, owner_army: 1, gold_per_day: 50, description: "x".into(), ..Building::default() };
        let v1 = Building { x: 7, y: 7, kind: 2, gold_per_day: 60, gold_max: 100, description: "x".into(), ..Building::default() };
        let v2 = Building { x: 8, y: 2, kind: 2, gold_per_day: 30, gold_max: 10, description: "x".into(), ..Building::default() };
        let far = Building { x: 19, y: 19, kind: 2, gold_per_day: 500, gold_max: 500, description: "x".into(), ..Building::default() };
        s.buildings = vec![fort, v1, v2, far];
        let mut take = Event::default();
        take.results.units_from_army = 2;
        // A once-only meeting does not count for army 1.
        let mut meet = Event { once: 1, ..Event::default() };
        meet.conditions.meet_army = 1;
        s.events = vec![take, meet];
        let rows = check_map(&s, &names());
        assert_eq!(
            messages(&rows, CheckKind::Army, 1),
            [tr("the army stands on an impassable place"), tr("there is no event for meeting the army"), tr("there is no description text")]
        );
        assert!(messages(&rows, CheckKind::Army, 2).is_empty());
        assert_eq!(messages(&rows, CheckKind::Army, 3), [tr("the army is never activated: no event activates it, starts a battle with it or takes units from it")]);
        assert_eq!(messages(&rows, CheckKind::Army, 4), [tr("the patrol reaches past the bottom edge of the map, which may crash the game")]);
        s.armies[0].troops[2] = Troop { unit: 10, level: 0, count: 2 };
        let rows = check_map(&s, &names());
        assert!(messages(&rows, CheckKind::Army, 1).contains(&trf!("the upkeep {upkeep} is above the average income {income}", upkeep = 290, income = 250)));
        // The name column is the army's name; go to opens the army.
        assert_eq!(rows[0].place(), Place::Army(1));
    }

    #[test]
    fn building_rules() {
        let mut s = map();
        let castle = Building { kind: 3, ..Building::default() };
        let mut market = Building { kind: 6, price_max: 400, description: "d".into(), random_artifacts_for_sale: 2, price_min: 10, ..Building::default() };
        market.name = "Рынок".into();
        let bridge = Building { kind: 13, ..Building::default() };
        let mut town = Building { kind: 1, description: "d".into(), gold_per_day: 5, random_artifacts_for_sale: 1, price_min: 20, price_max: 300, ..Building::default() };
        town.barracks[0].unit = 9; // a unit with no maximum count
        let mut stocked = Building { kind: 7, description: "d".into(), ..Building::default() };
        stocked.artifact_slots[40] = 5; // any of the 64 words counts
        s.buildings = vec![castle, market, bridge, town, stocked];
        let rows = check_map(&s, &names());
        assert_eq!(
            messages(&rows, CheckKind::Building, 1),
            [tr("no money income is set"), tr("there is no description text"), tr("the garrison is empty"), tr("no recruitment is set")]
        );
        // 400 is above the dearest artefact (300); the cheapest (types 9 and 10 left out) is
        // |−1200| > 50 > 10: so the market is not set up either way.
        assert_eq!(messages(&rows, CheckKind::Building, 2), [tr("the market's highest price is above the dearest artefact"), tr("the market is not set up")]);
        assert_eq!(rows.iter().find(|r| r.id == 2).unwrap().name, "Рынок");
        assert!(messages(&rows, CheckKind::Building, 3).is_empty(), "bridges need no description");
        // The town's random goods are fine (20 ≥ 50 is false: below the cheapest 50).
        assert_eq!(messages(&rows, CheckKind::Building, 4), [tr("no recruitment is set"), tr("the market is not set up")]);
        assert!(messages(&rows, CheckKind::Building, 5).is_empty());
    }

    #[test]
    fn event_and_point_rules() {
        let mut s = map();
        let local = Event { kind: 2, title: "Встреча%+Ф".into(), ..Event::default() };
        let listed = Event { kind: 2, ..Event::default() };
        let quest = Event { kind: 3, ..Event::default() };
        let rumour = Event { kind: 4, ..Event::default() };
        let sub = Event { kind: 1, subordinate: 1, ..Event::default() };
        let rel = Event { kind: 1, start_time: RELATIVE_START, ..Event::default() };
        let mut reward = Event { kind: 1, ..Event::default() };
        reward.results.units_add[2] = 4;
        let mut needs = Event { kind: 1, message: "m".into(), ..Event::default() };
        needs.conditions.artifacts_check = 1;
        needs.conditions.artifacts = [7, 135, 8];
        needs.conditions.artifacts_owner = [1, 1, 6];
        s.events = vec![local, listed, quest, rumour, sub, rel, reward, needs];
        let mut b = Building { description: "d".into(), ..Building::default() };
        b.event_slots[0] = 2;
        b.event_slots[1] = 4; // past the count: not listed
        b.event_count = 1;
        s.buildings = vec![b];
        let empty = Point { x: 3, y: 4, ..Point::default() };
        let lit = Point { radius: 5, ..Point::default() };
        s.points = vec![empty, lit];
        let rows = check_map(&s, &names());
        let ev = |id| messages(&rows, CheckKind::Event, id);
        assert_eq!(ev(1), [tr("a local event that no point and no building lists")]);
        assert_eq!(rows.iter().find(|r| r.kind == CheckKind::Event && r.id == 1).unwrap().name, "Встреча");
        assert!(ev(2).is_empty());
        assert_eq!(ev(3), [tr("a quest that no event completes")]);
        assert_eq!(ev(4), [tr("a rumour that no building lists")]);
        assert_eq!(ev(5), [tr("a subordinate event that no event chains to")]);
        assert_eq!(ev(6), [tr("a relative-only event that no event starts")]);
        assert_eq!(ev(7), [tr("there is no text for the reward")]);
        // Artefact 7 is nowhere; 135 and a slot not owned by the player are skipped.
        assert_eq!(ev(8), [trf!("the artefact {item} is not on the map, so the event can never fire", item = "Меч")]);
        s.armies.push(Army { artifacts: [0, 7, 0], description: "d".into(), relations: [-1; 4], ..Army::default() });
        assert!(messages(&check_map(&s, &names()), CheckKind::Event, 8).is_empty());
        assert_eq!(
            rows.iter().filter(|r| r.kind == CheckKind::Point).map(|r| (r.id, r.name.as_str())).collect::<Vec<_>>(),
            [(1, "3 - 4")]
        );
        assert_eq!(rows.last().unwrap().place(), Place::Point(1));
    }

    #[test]
    fn rows_come_in_record_order() {
        let mut s = map();
        s.points = vec![Point::default()];
        s.events = vec![Event { kind: 3, ..Event::default() }];
        s.buildings = vec![Building { kind: 5, ..Building::default() }];
        s.armies = vec![Army { relations: [-1; 4], behaviour: 1, ..Army::default() }];
        s.objects = vec![MapObject { x: 0, y: 0, sprite: 1, class: 11 }];
        let kinds: Vec<CheckKind> = check_map(&s, &names()).iter().map(|r| r.kind).collect();
        assert_eq!(kinds, [CheckKind::Army, CheckKind::Army, CheckKind::Building, CheckKind::Event, CheckKind::Point]);
    }
}
