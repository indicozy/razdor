//! The battle tester's core: the same seed gives the same result, the custom sides fight as the
//! custom battle does, and the runs add up. The install's cases (the map's armies, the `dt:`
//! units) skip without `RAZDOR_DT_DIR`.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use super::*;
use crate::rules::content::{Content, UnitId};
use crate::rules::custom::{choices, Control, Setup};

fn demo() -> Arc<Content> {
    Arc::new(Content::builtin())
}

/// Two fixed custom armies from the demo's unit types.
fn fixed(c: &Content) -> Matchup {
    let ids = choices(c);
    let a = vec![Pick { unit: ids[0], level: 2, items: Vec::new() }, Pick { unit: ids[1], level: 1, items: Vec::new() }];
    let b = vec![Pick { unit: ids[2], level: 3, items: Vec::new() }, Pick { unit: ids[3], level: 1, items: Vec::new() }];
    Matchup { a: Source::Custom(a), b: Source::Custom(b) }
}

fn sweep(c: &Arc<Content>, m: &Matchup, seeds: std::ops::RangeInclusive<u64>) -> Vec<Run> {
    run_seeds(c, None, m, false, seeds, &AtomicBool::new(false), &mut |_, _| {}).expect("runs").expect("not cancelled")
}

#[test]
fn the_same_seed_gives_the_same_run_twice() {
    let c = demo();
    let m = fixed(&c);
    let first = sweep(&c, &m, 1..=3);
    let again = sweep(&c, &m, 1..=3);
    assert_eq!(first, again);
    assert_eq!(first.iter().map(|r| r.seed).collect::<Vec<_>>(), vec![1, 2, 3], "each run carries its seed");
}

#[test]
fn the_battle_has_no_random_draws_so_every_seed_fights_the_same_battle() {
    let c = demo();
    let runs = sweep(&c, &fixed(&c), 1..=8);
    let s = Summary::of(&runs);
    assert!(s.same_every_seed, "{runs:?}");
    assert_eq!(s.runs, 8);
}

#[test]
fn two_custom_sides_fight_as_the_custom_battle_does() {
    let c = demo();
    let m = fixed(&c);
    let via_tester = fight(build(&c, None, &m, false).expect("builds"), 1);
    let (Source::Custom(a), Source::Custom(b)) = (&m.a, &m.b) else { unreachable!() };
    let setup = Setup { armies: [a.clone(), b.clone()], control: [Control::Ai, Control::Ai], wide: c.formation == crate::rules::formation::Formation::WIDE };
    let direct = fight(setup.battle(&c, false).expect("the custom battle"), 1);
    assert_eq!(via_tester, direct);
    assert!(via_tester.winner.is_some(), "the battle ends: {via_tester:?}");
}

#[test]
fn a_map_side_needs_the_map_and_an_empty_side_is_refused() {
    let c = demo();
    let m = Matchup { a: Source::Squad, b: Source::Custom(vec![Pick::new(choices(&c)[0])]) };
    assert!(build(&c, None, &m, false).err().expect("refused").contains("needs the map"));
    let empty = Matchup { a: Source::Custom(Vec::new()), b: Source::Custom(vec![Pick::new(choices(&c)[0])]) };
    assert!(build(&c, None, &empty, false).is_err());
}

#[test]
fn a_cancelled_sweep_gives_nothing() {
    let c = demo();
    let cancel = AtomicBool::new(true);
    assert_eq!(run_seeds(&c, None, &fixed(&c), false, 1..=5, &cancel, &mut |_, _| {}).unwrap(), None);
}

#[test]
fn progress_counts_each_run() {
    let c = demo();
    let mut seen = Vec::new();
    run_seeds(&c, None, &fixed(&c), false, 1..=4, &AtomicBool::new(false), &mut |d, t| seen.push((d, t))).unwrap();
    assert_eq!(seen, vec![(1, 4), (2, 4), (3, 4), (4, 4)]);
}

fn run(seed: u64, winner: Option<Team>, rounds: u32, a: (u32, i64), b: (u32, i64)) -> Run {
    Run { seed, winner, rounds, a: Losses { units: a.0, hp: a.1 }, b: Losses { units: b.0, hp: b.1 } }
}

#[test]
fn the_runs_add_up_to_the_summary() {
    let runs = vec![
        run(1, Some(Team::Player), 4, (0, 10), (3, 50)),
        run(2, Some(Team::Player), 6, (1, 30), (2, 40)),
        run(3, Some(Team::Enemy), 8, (4, 90), (0, 0)),
        run(4, None, 20, (2, 60), (2, 70)),
    ];
    let s = Summary::of(&runs);
    assert_eq!((s.runs, s.a_wins, s.b_wins, s.draws), (4, 2, 1, 1));
    assert_eq!((s.a_percent(), s.b_percent(), s.draw_percent()), (50.0, 25.0, 25.0));
    assert_eq!(s.avg_rounds, 9.5);
    assert_eq!((s.a.min_units, s.a.max_units), (0, 4));
    assert_eq!((s.a.min_hp, s.a.max_hp), (10, 90));
    assert!((s.a.avg_units - 1.75).abs() < 1e-9, "{}", s.a.avg_units);
    assert!((s.b.avg_hp - 40.0).abs() < 1e-9, "{}", s.b.avg_hp);
    assert!(!s.same_every_seed);
    let empty = Summary::of(&[]);
    assert_eq!((empty.runs, empty.a_percent(), empty.avg_rounds), (0, 0.0, 0.0));
}

#[test]
fn the_custom_picks_keep_their_unit_and_level() {
    let c = demo();
    let ids: Vec<UnitId> = choices(&c);
    let m = custom_side(&[(ids[0].0, 4)]);
    let Source::Custom(picks) = m else { unreachable!() };
    assert_eq!(picks, vec![Pick { unit: ids[0], level: 4, items: Vec::new() }]);
}

/// The install's content and a pair of its unit types (skipped without `RAZDOR_DT_DIR`).
fn install_content() -> Option<Arc<Content>> {
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    Some(Arc::new(Content::from_dt(&dt)))
}

#[test]
fn a_fixed_pair_of_install_units_fights_as_the_custom_battle_does() {
    let Some(c) = install_content() else { return };
    let ids = choices(&c);
    let a: Vec<Pick> = ids[..3].iter().map(|&u| Pick { unit: u, level: 1, items: Vec::new() }).collect();
    let b: Vec<Pick> = ids[3..6].iter().map(|&u| Pick { unit: u, level: 3, items: Vec::new() }).collect();
    let m = Matchup { a: Source::Custom(a.clone()), b: Source::Custom(b.clone()) };
    let setup = Setup { armies: [a, b], control: [Control::Ai, Control::Ai], wide: c.formation == crate::rules::formation::Formation::WIDE };
    let direct = fight(setup.battle(&c, false).expect("the custom battle"), 7);
    let mine = fight(build(&c, None, &m, false).expect("builds"), 7);
    assert_eq!(mine, direct);
    let runs = sweep(&c, &m, 1..=3);
    assert!(Summary::of(&runs).same_every_seed);
}

#[test]
fn the_map_armies_of_brod_fight_on_the_install() {
    let Some(c) = install_content() else { return };
    let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
    let dt = crate::dt::install::DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let pkg_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
    let pkg = crate::pack::load::Package::parse(crate::pack::Files::from_dir(&pkg_dir).unwrap()).0;
    let mg = open_map(&pkg, &dt, "brod").unwrap_or_else(|r| panic!("{}", r.to_text()));
    let armies: Vec<usize> = map_sources(&mg.game).iter().filter_map(|s| if let Source::Army(i) = s.source { Some(i) } else { None }).collect();
    assert!(armies.len() >= 2, "brod has two armies with troops: {armies:?}");
    let m = Matchup { a: Source::Army(armies[0]), b: Source::Army(armies[1]) };
    let runs = run_seeds(&mg.content, Some(&mg.game), &m, false, 1..=2, &AtomicBool::new(false), &mut |_, _| {}).unwrap().unwrap();
    assert_eq!(runs.len(), 2);
    assert!(Summary::of(&runs).same_every_seed);
    let _ = c;
}
