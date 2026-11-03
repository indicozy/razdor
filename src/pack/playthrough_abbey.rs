//! Chapter 3а «Аббатство» of «Пепельная Марка», played through the harness (`pack::playthrough`):
//! the hub's council takes the Order, the campaign's hand-over enters the abbey, and the hero walks
//! the map, answers its dialogs and fights its battles. The siege is a field battle (`scripts/siege.lua`)
//! started by the story when the dead stand at the south gate and the hero is in the abbey. The hub's
//! chain is copied from `playthrough_ch23` (its helpers are private there). Skipped without
//! `RAZDOR_DT_DIR`.

use std::sync::Arc;

use super::open::Opened;
use super::playthrough::{install, meet_host, open_map, prepared_to_wraiths, Carried, Run, Stuck, PREPARED};
use super::playthrough_ch23 as hub;
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::content::Content;
use crate::rules::map::Tile;

/// The hub's first visit with its quests done, the council answered `ally` and the Order's shrine
/// answered `shrine` (`cleanse`: Ilm the novice, `ilm` 1; `take`: Ilm the heretic, `ilm` 2).
fn hub_to_council(install: &DtInstall, seed: u32, ally: &str, shrine: &str) -> Result<Run, Stuck> {
    let (brod, brod_content) = open_map(install, "brod");
    let mut run = Run::new(&brod, brod_content, seed, PREPARED)?;
    prepared_to_wraiths(&mut run)?;
    meet_host(&mut run, 30)?;
    if !run.victory() {
        return Err(Stuck::Broken("chapter one was not won".into()));
    }
    let to = after_victory(&mut run.g, &brod.routes).ok_or_else(|| Stuck::Broken("chapter one has no next map".into()))?;
    let choices = [("council", ally), ("shrine_inside", shrine), ("shrine_offer", "accept"), ("marsh_offer", "accept"), ("marka_arrival", "take")];
    let (opened, content) = open_map(install, &to);
    let g = enter(&run.g, &brod, &opened, None, content).map_err(Stuck::Broken)?;
    let mut hub_run = Run::resume(g, &choices)?;
    hub::drink_for_wounded(&mut hub_run);
    hub_quests(&mut hub_run)?;
    hub_run.wait_until_done(40)?;
    Ok(hub_run)
}

/// The hub's quests on the way to the council (the walks of `playthrough_ch23`).
fn hub_quests(run: &mut Run) -> Result<(), Stuck> {
    run.walk_to((15, 69), 10)?;
    run.walk_to((30, 84), 12)?;
    run.walk_to((28, 36), 10)
}

/// The campaign's hand-over from the hub to the branch `to` (the hub's route must send the hero there),
/// with the branch's own dialog answers.
fn into_branch(install: &DtInstall, seed: u32, ally: &str, shrine: &str, to: &str, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let mut hub = hub_to_council(install, seed, ally, shrine)?;
    let (marka, _) = open_map(install, "marka");
    let next = after_victory(&mut hub.g, &marka.routes).ok_or_else(|| Stuck::Broken("the hub has no route".into()))?;
    if next != to {
        return Err(Stuck::Broken(format!("the hub sends the hero to {next}, not {to}")));
    }
    let (opened, content): (Opened, Arc<Content>) = open_map(install, to);
    let g = enter(&hub.g, &marka, &opened, None, content).map_err(Stuck::Broken)?;
    let mut run = Run::resume(g, choices)?;
    hub::drink_for_wounded(&mut run);
    Ok(run)
}

/// The abbey's south gate (Ilm's meeting) and the abbey's cell (the hero goes in).
const GATE_SOUTH: Tile = (50, 50);
const ABBEY: Tile = (50, 40);
/// The cell beside the Ольховка village (the trial's area `at = [14, 58, 8, 8]`).
const ALDER: Tile = (16, 60);

/// The siege's field fight of the run: its rounds and winner (the battle is `dead_wave`).
fn siege_fight(run: &Run) -> Option<(u32, Option<crate::rules::field::Side>)> {
    run.fields.last().map(|f| (f.rounds, f.winner))
}

/// The route of the Order: Ilm's meeting at the gate (when he is not with the hero yet), the abbey
/// entered, and the siege waited out in it.
fn the_order_route(run: &mut Run) -> Result<(), Stuck> {
    run.walk_to(GATE_SOUTH, 6)?;
    run.walk_to(ABBEY, 6)?;
    run.wait_until_done(40)
}

#[test]
fn the_abbey_opens_with_its_arrival_dialog_and_nothing_attacks_before_the_siege() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_arrival")), "{:?}", run.log);
    // The hero stands at the start for the first week: the inquisitors arrive, the dead walk their patrol.
    run.wait_days(11).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.battles.is_empty(), "nothing attacks before the story means it to: {:?}", run.log);
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_inquisitors")), "{:?}", run.log);
}

#[test]
fn the_order_route_wins_the_siege_and_hands_back_to_marka() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
    the_order_route(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert_eq!(run.var("abbey_done"), Some(VarValue::Bool(true)), "{}", run.log.join("\n"));
    assert_eq!(siege_fight(&run).map(|f| f.1), Some(Some(crate::rules::field::Side::Defenders)), "{:?}", run.fields);
    // The route never walks to Ольховка: the abbey holds (the plain ending), and no judgement is passed.
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_end_plain")), "{}", run.log.join("\n"));
}

#[test]
fn the_siege_is_a_field_battle_and_is_won_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let started = std::time::Instant::now();
    let (mut won, mut problems) = (0, Vec::new());
    let mut rounds = Vec::new();
    for seed in 1..=15u32 {
        match into_branch(&install, seed, "order", "cleanse", "abbey", &[]) {
            Ok(mut run) => match the_order_route(&mut run) {
                Ok(()) if run.victory() => {
                    won += 1;
                    rounds.extend(siege_fight(&run).map(|f| f.0));
                }
                r => problems.push(format!("seed {seed}: {r:?}; {}", run.log.join(" | "))),
            },
            Err(e) => problems.push(format!("seed {seed}: {e}")),
        }
    }
    eprintln!("abbey siege (the Order, Ilm the novice's pilgrim) over 15 seeds: won {won}/15, siege rounds {rounds:?}, in {:.2}s {problems:?}", started.elapsed().as_secs_f64());
    assert!(won * 100 >= 70 * 15, "won {won}/15: {problems:?}");
}

#[test]
fn the_siege_starts_when_the_hero_walks_into_the_abbey_after_the_dead_came() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
    // The hero waits outside the gate; the dead come to their stand, and nothing attacks.
    run.walk_to(GATE_SOUTH, 6).unwrap_or_else(|e| panic!("{e}"));
    run.wait_days(14).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.battles.is_empty(), "a waiting hero outside is not attacked: {:?}", run.log);
    run.walk_to(ABBEY, 2).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    run.wait_until_done(5).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert_eq!(run.battles.iter().filter(|b| b.enemy == "dead_wave").count(), 1, "{}", run.log.join("\n"));
    assert_eq!(run.fields.len(), 1, "the siege is a field battle: {}", run.log.join("\n"));
}

#[test]
fn the_abbey_hands_back_to_marka_and_marka_ends_the_chapter_for_gates() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
    the_order_route(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    let (abbey, _) = open_map(&install, "abbey");
    let (marka, marka_content) = open_map(&install, "marka");
    let back = after_victory(&mut run.g, &abbey.routes).expect("the abbey has a route");
    assert_eq!(back, "marka", "the abbey goes back to the hub");
    let g = enter(&run.g, &abbey, &marka, None, marka_content.clone()).unwrap_or_else(|e| panic!("{e}"));
    let mut hub = Run::resume(g, &[]).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(hub.var("ally"), Some(VarValue::Int(1)), "the hub's state is kept");
    hub.wait_until_done(3).unwrap_or_else(|e| panic!("{e}\n{}", hub.log.join("\n")));
    assert!(hub.victory(), "the revisit ends the chapter: {}", hub.log.join("\n"));
    let next = after_victory(&mut hub.g, &marka.routes).expect("the hub has a route");
    assert_eq!(next, "gates", "the hub goes on to the gates once the Order's chapter is done");
}

/// The heretic's path: Ilm the novice (joined at the shrine's cleanse), the trial's mercy (`spare`, ilm 2), the midnight's offer accepted,
/// then the siege. (The shrine's «take» does not reach it: it leaves the Order's council no Order choice.)
#[test]
fn the_heretic_route_spares_the_village_and_wins_the_siege() {
    let Some(install) = install() else { return };
    let (opened, content) = open_map(&install, "abbey");
    let carry = Carried { gold: Some(80), vars: vec![("ally".into(), VarValue::Int(1))], squad: vec![("hunter", 1, 3)], named: vec![("ilm", "pilgrim", 1)], avoid_armies: true };
    let choices = [("abbey_trial", "spare"), ("abbey_heresy", "accept")];
    let mut run = Run::carried(&opened, content, 1, &choices, &carry).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(GATE_SOUTH, 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    run.walk_to(ALDER, 10).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(run.var("ilm"), Some(VarValue::Int(2)), "the spared village plants the heresy: {}", run.log.join("\n"));
    run.walk_to(ABBEY, 12).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    run.wait_until_done(40).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_heresy -> accept")), "{}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_end_heretic")), "{}", run.log.join("\n"));
}

/// The Order's route day by day: the gold and the squad's size (the wages, the desertions), and the
/// dead wave's purse at its battle. A report (`eprintln`), with the route's gold never below 0.
#[test]
fn the_order_route_gold_curve() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
    let mut curve = vec![format!("hand-over: gold {} squad {}", run.g.gold, run.g.squad.len())];
    run.walk_to(GATE_SOUTH, 6).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(ABBEY, 6).unwrap_or_else(|e| panic!("{e}"));
    curve.push(format!("in the abbey (day {:.1}): gold {} squad {} wages/day {} income/day {}", run.day(), run.g.gold, run.g.squad.len(), run.g.daily_wages(), run.g.daily_income()));
    let mut last = run.g.gold;
    for _ in 0..40 {
        let r = run.wait_days(1);
        if run.day() < 14.0 { curve.push(format!("  day {:.1}: wages/day {} income/day {} gold {} squad {}", run.day(), run.g.daily_wages(), run.g.daily_income(), run.g.gold, run.g.squad.len())); }
        let purse = run.g.world.armies.iter().find(|a| run.g.story().and_then(|s| s.army_names.get(&a.id)).is_some_and(|n| n == "dead_wave")).map(|a| a.gold);
        if run.g.gold != last || run.g.squad.len() != 6 || run.battles.len() == 1 && curve.len() < 20 {
            curve.push(format!("day {:.1}: gold {} (was {last}) squad {} dead purse {purse:?}", run.day(), run.g.gold, run.g.squad.len()));
            last = run.g.gold;
        }
        if r.is_err() || run.done() {
            break;
        }
    }
    eprintln!("ORDER GOLD CURVE\n  {}\n  battles {:?}\n  fields {:?}\n  end: gold {} squad {:?} done {}", curve.join("\n  "), run.battles, run.fields, run.g.gold, run.squad_names(), run.done());
    assert!(run.victory(), "{}", run.log.join("\n"));
}

/// The Order's route over 15 seeds from the hub (Ilm the novice's pilgrim, the shrine's cleanse). The
/// route's fights are the siege; the rate is the route's, the target 70%.
#[test]
fn the_abbey_routes_win_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let started = std::time::Instant::now();
    let mut report = Vec::new();
    let mut rounds = Vec::new();
    let mut rate = |name: &str, play: &dyn Fn(u32) -> Result<Run, String>| {
        let (mut won, mut problems) = (0, Vec::new());
        for seed in 1..=15u32 {
            match play(seed) {
                Ok(run) if run.victory() => {
                    won += 1;
                    rounds.extend(siege_fight(&run).map(|f| f.0));
                }
                Ok(run) => problems.push(format!("seed {seed}: {}", run.log.join(" | "))),
                Err(e) => problems.push(format!("seed {seed}: {e}")),
            }
        }
        report.push(format!("{name}: won {won}/15 {problems:?}"));
        won
    };
    let order = rate("the Order (hub, cleanse)", &|seed| {
        let mut run = into_branch(&install, seed, "order", "cleanse", "abbey", &[]).map_err(|e| e.to_string())?;
        the_order_route(&mut run).map_err(|e| e.to_string())?;
        Ok(run)
    });
    eprintln!("abbey routes over 15 seeds in {:.2}s; siege rounds {rounds:?}\n  {}", started.elapsed().as_secs_f64(), report.join("\n  "));
    assert!(order * 100 >= 70 * 15, "the Order: won {order}/15");
}

/// The blight east of the abbey (its warning and curse by walking in), and the Order's rite back at the
/// abbey, which takes the curse off for 30 gold and gives the holy water. Optional for the route; the test
/// keeps its areas reachable.
#[test]
fn the_blight_curses_by_walking_and_the_rite_takes_it_off() {
    let Some(install) = install() else { return };
    let (opened, content) = open_map(&install, "abbey");
    let carry = Carried { gold: Some(80), vars: vec![("ally".into(), VarValue::Int(1))], squad: vec![("hunter", 1, 3)], named: vec![], avoid_armies: true };
    let mut run = Run::carried(&opened, content, 1, &[("abbey_blight", "")], &carry).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(GATE_SOUTH, 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    run.walk_to((58, 32), 10).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_blight")), "{}", run.log.join("\n"));
    // Every step inside the blight curses the army, at most six taints (the walk crosses several cells).
    assert!(matches!(run.var("taint"), Some(VarValue::Int(t)) if (1..=6).contains(&t)), "the blight curses the army: {}", run.log.join("\n"));
    run.walk_to(ABBEY, 10).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_rite")), "the tainted hero is offered the rite: {}", run.log.join("\n"));
    assert_eq!(run.var("taint"), Some(VarValue::Int(0)), "the rite takes the curse off: {}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_rite -> rite")), "{}", run.log.join("\n"));
}

/// The road north (the main route from the start to the abbey's south gate): a beat (a dialog, a capture or an
/// encounter) at most ten cells apart, the start included. The walk's cells are `Run::path`.
#[test]
fn the_road_north_has_a_beat_every_ten_cells() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[]).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(GATE_SOUTH, 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    let mut marks = vec![0usize];
    marks.extend(run.beats.iter().copied());
    marks.push(run.path.len() - 1);
    marks.sort_unstable();
    let mut gaps: Vec<(usize, usize, usize)> = marks.windows(2).map(|w| (w[1] - w[0], w[0], w[1])).collect();
    gaps.sort_unstable_by(|a, b| b.cmp(a));
    let worst = gaps[0];
    eprintln!("the road north: {} cells, {} beats, the longest gap {} cells (path {}..{})", run.path.len() - 1, run.beats.len(), worst.0, worst.1, worst.2);
    assert!(worst.0 <= 10, "the longest walk between beats is {} cells: {:?}", worst.0, &run.path[worst.1..=worst.2]);
}

/// Ilm walks home with the hero (his crossroads is the Order's route: he joined at the shrine), and the monks at the
/// gate know him. Ольховка ridden past is the chapter's plain ending: no judgement, no mercy, Ilm no heretic.
#[test]
fn ilm_comes_home_and_riding_past_alder_is_the_plain_ending() {
    let Some(install) = install() else { return };
    let mut run = into_branch(&install, 1, "order", "cleanse", "abbey", &[("abbey_trial", "ride")]).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(GATE_SOUTH, 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_ilm_home")), "Ilm is greeted at the gate: {}", run.log.join("\n"));
    run.walk_to(ALDER, 10).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_trial -> ride")), "{}", run.log.join("\n"));
    assert_eq!(run.var("abbey_alder"), Some(VarValue::Int(3)));
    run.walk_to(ABBEY, 12).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    run.wait_until_done(40).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_siege")), "the dead march the midnight after the judgement: {}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog abbey_end_plain")), "the abbey held: {}", run.log.join("\n"));
    assert_eq!(run.var("abbey_done"), Some(VarValue::Bool(true)));
}
