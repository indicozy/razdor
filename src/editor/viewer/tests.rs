//! The AI viewer on small synthetic maps, from the numbers of testers.md §3 and §5.
use super::*;
use crate::dt::dtm::{Event, FlagScript};
use crate::rules::content::testkit as ck;
use crate::rules::content::UnitDef;
use crate::rules::world::testkit::{army, hero, scenario, troop};

/// Units 1–3 the heroes, 4 a warrior, 5 a shooter.
fn content() -> Arc<Content> {
    let units = vec![ck::warrior(1, 20, 5), ck::warrior(2, 20, 5), ck::warrior(3, 20, 5), ck::warrior(4, 10, 2), UnitDef { cost: 80, ..ck::shooter(5, 8) }];
    Arc::new(ck::content(units, vec![ck::item(7, crate::rules::content::ArtefactType::Ring)]))
}

/// A 40 × 20 map: the knight's preset at (5, 5) with a warrior and item 7; army 1 (hostile)
/// at (30, 10) whose speed byte is `speed`.
fn map(speed: i8) -> Scenario {
    let mut s = scenario(40, 20);
    let mut p = hero(5, 5, 300, &[troop(4, 2, 1)]);
    p.artifacts = [7, 0, 0];
    s.header.heroes[0] = p;
    let mut a = army(1, 30, 10, -2, &[troop(4, 0, 2)]);
    a.speed_correction = speed;
    s.armies.push(a);
    s
}

fn open(s: &Scenario) -> Viewer {
    Viewer::open(content(), s, Switches::default(), None)
}

/// A global event with `message`, starting at the map's start, lasting `hours`.
fn event(s: &Scenario, message: &str, hours: u16) -> Event {
    Event { kind: 1, start_time: s.header.start_time, duration: hours, message: message.into(), ..Event::default() }
}

#[test]
fn it_opens_seeded_to_1_with_no_hero_and_speed_byte_plus_4() {
    let v = open(&map(1));
    assert_eq!(v.game.rng.state(), 1);
    assert_eq!(v.hero, None);
    assert_eq!(v.game.world.armies[0].speed, 5, "byte 13 + 4; the game's would be 5 − 1");
    assert_eq!(v.first_list(), vec![Subject::Hero, Subject::Army(0)]);
    assert_eq!(v.hero_index(), None);
    assert!(Viewer::open(content(), &map(1), Switches::default(), Some(77)).game.rng.state() == 77);
}

#[test]
fn six_minute_steps_bank_24_on_each_24_minute_tick_up_to_200() {
    // Speed 124: no step is ever affordable, so the bank only fills.
    let mut v = open(&map(120));
    let start = v.now();
    let mut banked = Vec::new();
    for _ in 0..48 {
        let ticks = v.now().is_multiple_of(24);
        v.step();
        banked.push((ticks, v.game.world.armies[0].budget));
    }
    assert_eq!(v.now(), start + 48 * 6);
    let mut want = 0.0f32;
    for (tick, b) in banked {
        if tick {
            want = (want + 24.0).min(200.0);
        }
        assert_eq!(b, want);
    }
    assert_eq!(want, 200.0);
}

#[test]
fn the_hero_is_an_army_the_ai_steers_and_its_choice_locks_once_time_runs() {
    let mut s = map(1);
    assert!(!open(&s).offered(HeroClass::Archmage), "no preset position");
    s.header.heroes[2] = hero(7, 7, 0, &[]);
    let mut v = open(&s);
    assert!(v.offered(HeroClass::Ranger));
    assert!(!v.set_hero(Some(HeroClass::Archmage)));
    assert!(v.set_hero(Some(HeroClass::Knight)));
    let i = v.hero_index().unwrap();
    let h = &v.game.world.armies[i];
    assert_eq!((h.speed, h.faction, h.ai.aggression, h.gold), (4, 1, -20, 300));
    assert_eq!(h.troops.len(), 2);
    assert_eq!(h.troops[0].worn[0], Some(ItemId(7)), "worn, not packed");
    assert_eq!(v.first_list()[0], Subject::Hero);
    assert_eq!(v.army_of(Subject::Hero), Some(i));
    assert!(!v.second_list().contains(&Subject::Army(i)));
    v.step();
    assert!(!v.set_hero(None), "time has run");
    assert_eq!(v.hero, Some(HeroClass::Knight));
}

#[test]
fn global_events_fire_on_the_ticks_with_the_60_minute_rule() {
    let mut s = map(1);
    s.events.push(event(&s, "hello", 24));
    let mut local = event(&s, "local", 24);
    local.kind = 2;
    s.events.push(local);
    let mut v = open(&s);
    v.events_on = false;
    v.steps(8);
    assert!(v.messages.is_empty(), "the events box is off");
    v.events_on = true;
    v.steps(4);
    assert_eq!(v.messages, vec!["hello".to_string()], "global only");
    v.steps(8);
    assert_eq!(v.messages.len(), 1, "not again within 60 minutes");
    v.steps(4);
    assert_eq!(v.messages.len(), 2);
}

#[test]
fn a_repeat_under_a_day_stops_the_clock_where_the_original_divides_by_zero() {
    let mut s = map(1);
    let mut e = event(&s, "x", 24);
    e.repeat = 600;
    s.events.push(e);
    let mut v = open(&s);
    let start = v.now();
    v.steps(10);
    assert_eq!(v.stop, Some(ViewerStop::RepeatUnderADay { event: 1 }));
    let at = v.now();
    v.steps(5);
    assert_eq!(v.now(), at, "the clock stays stopped");
    assert!(at < start + 60);
}

#[test]
fn victory_stops_the_clock_and_chains_fire_their_event() {
    let mut s = map(1);
    let mut first = event(&s, "first", 24);
    first.results.chained_event = 2;
    let mut second = event(&s, "second", 24);
    second.start_time = s.header.start_time + 10_000;
    s.events.push(first);
    s.events.push(second);
    s.header.victory_event = 2;
    let mut v = open(&s);
    v.steps(4);
    assert_eq!(v.messages, vec!["first".to_string(), "second".to_string()], "a chain fires whatever its own conditions");
    assert_eq!(v.stop, Some(ViewerStop::Victory));
}

#[test]
fn flags_relative_events_and_activation() {
    let mut s = map(1);
    let mut set = event(&s, "set", 24);
    set.title = "a%+GATE".into();
    set.flags = FlagScript::from_title(&set.title);
    set.results.relative_event = 3;
    set.results.relative_delay_hours = 2;
    set.results.deactivate_army = 1;
    set.once = 1;
    let mut test = event(&s, "gated", 240);
    test.title = "b%=GATE".into();
    test.flags = FlagScript::from_title(&test.title);
    test.once = 1;
    let mut later = event(&s, "later", 1);
    later.start_time = u32::MAX; // never by its own start
    s.events.extend([set, test, later]);
    let mut v = open(&s);
    v.steps(4);
    assert_eq!(v.flags(), vec!["GATE".to_string()]);
    assert!(v.game.world.armies.is_empty(), "army 1 deactivated");
    v.steps(40);
    assert!(v.messages.contains(&"gated".to_string()));
    assert!(v.messages.contains(&"later".to_string()), "its start moved to 2 hours after the first");
    assert_eq!(v.messages.iter().filter(|m| *m == "gated").count(), 1, "once");
}

#[test]
fn a_meet_event_passes_only_on_the_step_of_the_meeting_and_marks_the_army_met() {
    let mut s = map(1);
    let mut e = event(&s, "met", 240);
    e.conditions.meet_army = 1;
    s.events.push(e);
    let mut v = open(&s);
    v.ran = true;
    assert_eq!(v.passes(0), Ok(false));
    assert!(v.game.met_armies.contains(&1), "marked met though it did not meet");
    v.met = Some(1);
    assert_eq!(v.passes(0), Ok(true));
}

#[test]
fn the_signed_threshold() {
    assert!(threshold(0, -5));
    assert!(threshold(3, 3) && !threshold(3, 2));
    assert!(threshold(-3, 3) && !threshold(-3, 4));
}

#[test]
fn the_9_by_9_grid_and_its_top_edge_bug() {
    let (w, h) = (20, 20);
    let values: Vec<u16> = (0..(w * h) as u16).collect();
    let g = grid9(&values, w, h, 10, 10);
    assert_eq!(g[0][0], Some(6 * 20 + 6));
    // Near the left edge the column start is clamped to 0.
    assert_eq!(grid9(&values, w, h, 1, 10)[0][0], Some(6 * 20));
    // Near the top edge the original zeroes the column start, not the row start: the first
    // rows lie above the map.
    let g = grid9(&values, w, h, 10, 1);
    assert_eq!(g[0][0], None);
    assert_eq!(g[3][0], Some(0), "row 0, column 0: the column start went to 0");
    // Near the right and bottom edges the window moves inside.
    assert_eq!(grid9(&values, w, h, 19, 19)[8][8], Some(19 * 20 + 19));
}

#[test]
fn game_time_text() {
    assert_eq!(format_time(0), "00h 01.01.0000");
    assert_eq!(format_time(518_400 + 43_200 + 1440 * 2 + 60 * 13), "13h 03.02.0001");
}

#[test]
fn the_battle_buttons_build_the_attacker_weakened_and_the_defender_whole() {
    let mut s = map(1);
    s.armies.push(army(2, 31, 10, -2, &[troop(5, 0, 1)]));
    let mut v = open(&s);
    v.game.world.armies[0].troops[1].unpaid = true;
    let (a, b) = v.battle_armies(0, 1);
    assert_eq!(a.units.len(), 2, "the unpaid fight");
    assert_eq!(a.units.iter().map(|p| p.weak).collect::<Vec<_>>(), vec![false, true]);
    assert!(b.units.iter().all(|p| !p.weak));
    let t = &v.game.world.armies[0].troops[0];
    assert_eq!(a.units[0].value, ai::tactical_modes(&v.game.content, t, 0).1);
    // The predictions run both ways.
    let [ab, ba] = v.predictions(0, 1);
    assert_eq!((ab.own, ba.theirs), (ba.theirs, ab.own));
    v.game.world.armies[1].troops[0].unpaid = true;
    assert!(v.predictions(1, 0)[0].own > 0);
    let _ = v.test_score(0, 1);
}

#[test]
fn the_overlays_plan_the_army() {
    let mut v = open(&map(1));
    let (density, dist) = v.plan(0).unwrap();
    assert_eq!(density.len(), 40 * 20);
    assert_eq!(dist.len(), 40 * 20);
}

#[test]
fn every_shipped_map_runs_a_day_in_the_viewer() {
    // Against the player's install; skipped without `RAZDOR_DT_DIR`.
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let c = Arc::new(Content::from_dt(&dt));
    for m in dt.maps.iter().take(3) {
        let payload = crate::dt::container::decode(&std::fs::read(&m.path).unwrap()).unwrap().payload;
        let s = Scenario::parse_payload(&payload).unwrap();
        let mut v = Viewer::open(c.clone(), &s, Switches::default(), None);
        if let Some(class) = HeroClass::ALL.into_iter().find(|&h| v.offered(h)) {
            assert!(v.set_hero(Some(class)));
        }
        v.steps(240);
        assert!(v.stop.is_some() || v.elapsed() == 240 * 6, "{}", m.name);
    }
}
