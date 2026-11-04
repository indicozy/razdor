//! The formula on DT inputs: the rebuild's own cases (`src/editor/playability.rs` of the paused
//! `dt-editor` branch, its tests of §6.2 to §6.7), with the same numbers, plus the package path's
//! invariants on the example packages (skipped without `RAZDOR_DT_DIR`).

use super::*;
use crate::dt::dtm::{Army, Building, Point};
use crate::studio::scores::Target;

fn names() -> Facts {
    Facts {
        artefacts: BTreeMap::from([(1, (2600, 0)), (2, (-9000, 9)), (3, (40000, 3))]),
        unit_costs: BTreeMap::from([(5, 200), (6, 100)]),
        spell_hits: BTreeMap::from([(1, -5)]),
    }
}

fn map(w: u32) -> Scenario {
    let mut s = Scenario::default();
    s.header.width = w;
    s.header.height = w;
    s.terrain = vec![6; (w * w) as usize];
    s
}

/// An event with no source line, for the tests.
fn src(event: Event) -> Source {
    Source { event, name: "event".into(), target: Target::Hero }
}

fn sources(list: Vec<Event>) -> Vec<Source> {
    list.into_iter().map(src).collect()
}

fn score_of(s: &Scenario, events: &[Source], facts: &Facts) -> Result<Score, ScoreError> {
    score(&Input { s, events, facts, campaign: 0, names: &Names::default() })
}

#[test]
fn words_and_their_distance() {
    assert_eq!(words("Да, это  так!\r\nИди."), ["Да", "это", "так", "Иди"]);
    assert_eq!(words("..."), [""]);
    assert_eq!(word_distance("a b c", "a x c"), 1);
    assert_eq!(word_distance("a b c", "a c"), 1);
    assert_eq!(word_distance("Go now.", "go now"), 1, "words compare with case");
    assert_eq!(word_distance("one two", "three four five"), 3);
}

/// §6.2 event values: the rebuild's case, with its flag test as the event's script (`=/B`).
#[test]
fn event_values() {
    let facts = names();
    let mut quests = 0;
    let mut rewards = vec![];
    let mut e = Event { kind: 3, ..Event::default() };
    e.flags = Some(FlagScript { require_unset: Some("B".into()), ..FlagScript::default() });
    e.conditions.meet_army = 2;
    e.conditions.happened_yes_check = 1;
    e.conditions.happened_yes = [4, 0];
    e.conditions.not_happened_check = 1;
    e.conditions.not_happened = [4, 5];
    e.conditions.confirm_question = 1;
    e.conditions.artifacts_check = 1;
    e.results.relative_event = 3;
    e.results.artifacts_add = [1, 2, 3, 0];
    e.results.gold = 4500;
    // 40 + 10 + 10 + 15 + 15 (=/B) + 20 + 10 + 15 + items (2600 div 250 = 10, 9000 div 500 div 1500
    // = 0, 40000 div 250 capped at 15) + 2 + 5.
    let (v, _) = event_value(&facts, &e, &mut quests, &mut rewards);
    assert_eq!(v, 40 + 10 + 10 + 15 + 15 + 20 + 10 + 15 + 25 + 2 + 5);
    assert_eq!((quests, rewards), (1, vec![10, 0, 15, 3]));
    let mut plain = Event { kind: 1, ..Event::default() };
    plain.flags = Some(FlagScript { require_set: Some("A".into()), ..FlagScript::default() });
    assert_eq!(event_value(&facts, &plain, &mut 0, &mut vec![]).0, 15, "a flag test counts once");
    assert_eq!(event_value(&facts, &Event { kind: 1, ..Event::default() }, &mut 0, &mut vec![]).0, 0);
}

/// §6.3 to §6.7 on the empty 50 × 50 map: all 44² inner cells uniform land at every level; no
/// buildings, so the spacing averages W; no events cost W. The score is 0.
#[test]
fn the_curve_and_the_empty_map() {
    let s = map(50);
    let r = score_of(&s, &[], &names()).unwrap();
    assert_eq!((r.curve_value, r.quests), (0, 0));
    assert_eq!(r.variety.land, [1936; 3]);
    assert_eq!(r.stages, [0, -153 - 612 - 1886, -2651 - 1600, -4251 - 50]);
    assert_eq!(r.total, -4301);
    // The curve: (s − W) ÷ √W = 50 gives √50 × 11 = 77.78; 1000 on a 100 map gives √100 × 11 = 110,
    // bent to 107.5, rounded to even: 108; 1 point over W = 50 gives √(1 / √50) × 11 = 4.1.
    assert_eq!(curve(100, 100), 0);
    assert_eq!(curve(100 + (50.0 * 100f64.sqrt()) as i64, 100), 78);
    assert_eq!(curve(100 + 1000, 100), 108);
    assert_eq!(curve(51, 50), 4);
    assert_eq!(score_of(&map(49), &[], &names()), Err(ScoreError::TooNarrow), "narrower than 50: the original divides by zero");
    assert_eq!((band(0), band(1), band(49), band(50), band(399), band(400)), (0, 1, 1, 2, 6, 7));
}

/// The items of a breakdown add up to its total, and the empty map's items name what they cost.
#[test]
fn the_items_add_up_to_the_total() {
    let r = score_of(&map(50), &[], &names()).unwrap();
    assert_eq!(r.items.iter().map(|i| i.points).sum::<i64>(), r.total);
    assert!(r.items.iter().any(|i| i.term == "Penalty" && i.note.contains("No events")));
    assert!(r.items.iter().any(|i| i.term == "Terrain variety" && i.points == -153 && !i.targets.is_empty()), "the land level 1 penalty selects its cells");
}

/// §6.2 to §6.6 on a small map: two castles 8 apart (one with income), a lone army with an artefact,
/// two events one word apart, a point and a preset hero's gold.
#[test]
fn a_small_map_scores_as_the_original() {
    let mut s = map(50);
    let castle = |x, gold| Building { x, y: 10, kind: 3, size_x: 2, size_y: 2, gold_per_day: gold, ..Building::default() };
    s.buildings = vec![castle(10, 100), castle(18, 0)];
    s.armies = vec![Army { leader_unit: 9, unknown_80: 120, artifacts: [1, 0, 0], ..Army::default() }];
    let msg = "Добро пожаловать в наши земли, путник, здесь тебя ждут".to_string();
    let events = sources(vec![Event { kind: 1, message: msg.clone(), ..Event::default() }, Event { kind: 2, message: format!("{msg} снова"), ..Event::default() }]);
    s.points = vec![Point { x: 20, y: 20, ..Point::default() }];
    s.header.heroes[0].x = 5;
    s.header.heroes[0].gold = 1500;
    let r = score_of(&s, &events, &names()).unwrap();
    // Content: army 120 ÷ 50 + 2600 ÷ 500 = 7 (+5 for the army); castles 4 + 4. Events: 0 and 15, each
    // a near duplicate of the other: 0 ÷ 1, 15 ÷ 1; text lengths give √54 → 7 and √60 → 8, t = −2 and −1:
    // the first (0) is weak and becomes −10, the second gets −1 ÷ 2 = 0. Then 5 × (2 − 1 weak) and 1 point.
    assert_eq!(r.stages[0], 7 + 5 + 8 + (-10) + 15 + 5 + 1);
    assert!(r.variety.terrain[0] as i64 == 44 * 44 && r.variety.land[2] > 0 && r.variety.water == [0; 3]);
    assert_eq!(r.stages[2], r.stages[1], "the castles' centres are 8 apart: no spacing loss");
    // The castle without income −10, and the preset's gold (1500 − 1000) ÷ 50 = 10.
    assert_eq!(r.stages[3], r.stages[2] - 10 - 10);
    assert_eq!(r.items.iter().map(|i| i.points).sum::<i64>(), r.total);
    assert!(r.items.iter().any(|i| i.term == "Penalty" && i.targets == vec![Target::Hero]), "the hero's preset selects the hero");
}

/// §6.6 chains from points: a loop (1 → 12 → 1) and a deadly chain (2 → 3 → 2, the repeated event
/// casting a spell with negative fixed hits) and a chain to the defeat event.
#[test]
fn chains_from_points() {
    let mut s = map(50);
    let chain = |next: u16| {
        let mut e = Event { kind: 2, title: "-x".into(), ..Event::default() };
        e.results.chained_event = next;
        e
    };
    let mut deadly = chain(3);
    deadly.results.cast_spell = 1;
    let mut list: Vec<Event> = (1..=12).map(|_| chain(0)).collect();
    list[0] = chain(12);
    list[11] = chain(1);
    list[1] = deadly;
    list[2] = chain(2);
    list[3] = chain(5);
    let events = sources(list);
    s.header.defeat_event = 5;
    let mut p = Point { radius: 1, event_count: 3, ..Point::default() };
    p.event_slots[..3].copy_from_slice(&[1, 2, 4]);
    s.points = vec![p];
    let base = {
        let mut q = s.clone();
        q.points[0].event_count = 0;
        score_of(&q, &events, &names()).unwrap().stages[3]
    };
    let r = score_of(&s, &events, &names()).unwrap();
    // One loop (500 × 1 ÷ 1 point) and two deadly chains (25 each).
    assert_eq!(r.stages[3], base - 500 - 50);
    let mut q = s.clone();
    q.points[0].event_slots[1] = 0;
    assert_eq!(score_of(&q, &events, &names()), Err(ScoreError::PointEvents));
    let mut q = s.clone();
    q.points[0].event_count = 6;
    assert_eq!(score_of(&q, &events, &names()), Err(ScoreError::PointEvents));
    let mut events2 = events.clone();
    events2[3].event.results.chained_event = 5001;
    assert_eq!(score_of(&s, &events2, &names()), Err(ScoreError::PointEvents));
}

/// The package path on the example packages: the install's maps score, and every item adds up.
#[test]
fn the_example_maps_score_on_the_install() {
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
    let pkg = crate::pack::load::Package::parse(crate::pack::Files::from_dir(&root).unwrap()).0;
    for map in ["brod", "marka"] {
        let (score, names) = richness(map, &pkg, &dt, "ru").unwrap_or_else(|e| panic!("{map}: {e}"));
        assert_eq!(score.items.iter().map(|i| i.points).sum::<i64>(), score.total, "{map}: the items add up");
        assert!(score.items.iter().any(|i| i.term == "Events"), "{map} has events");
        assert!(!names.armies.is_empty() || !names.buildings.is_empty(), "{map} names its armies and buildings");
        println!("RICH {map}: total {} curve {} stages {:?} quests {}", score.total, score.curve_value, score.stages, score.quests);
        for i in &score.items { if i.points != 0 { println!("RICH   {} / {} {} {:+} [{}] {}", i.term, i.note, i.name, i.points, i.figures, i.targets.len()); } }
    }
}
