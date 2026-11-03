//! The Free Companies of «Пепельная Марка»: the hub's third route (Kord hired for 40 gold, the council's
//! «companies»), chapter 3в `camp`, and the hub's return from a branch to the gates. Played through the
//! harness of `pack::playthrough`: each map is entered by the campaign's own hand-over
//! (`rules::campaign::enter`), so the squad, the gold and the variables are the ones the campaign carries.
//! Skipped without `RAZDOR_DT_DIR`.

use std::sync::Arc;

use super::open::Opened;
use super::playthrough::{install, meet_host, open_map, prepared_to_wraiths, Carried, Run, Stuck, PREPARED};
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::battle::Outcome;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::content::Content;
use crate::rules::map::Tile;

/// The camp's dialog answers: the Crown's column lets pass (its strike is an open balance question), the red
/// company is left, the waygate's stone is lit, the spoils are split.
pub(crate) const CAMP: &[(&str, &str)] = &[("camp_raid", "pass"), ("camp_loot", "split"), ("camp_red", "leave"), ("camp_gate", "light")];

/// Chapter 1 won on the prepared route, handed over to `marka` (the hub's first visit).
fn into_marka(install: &DtInstall, seed: u32, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let (brod, brod_content) = open_map(install, "brod");
    let mut run = Run::new(&brod, brod_content, seed, PREPARED)?;
    prepared_to_wraiths(&mut run)?;
    meet_host(&mut run, 30)?;
    if !run.victory() {
        return Err(Stuck::Broken("chapter one was not won".into()));
    }
    let to = after_victory(&mut run.g, &brod.routes).ok_or_else(|| Stuck::Broken("chapter one has no next map".into()))?;
    hand_over(install, &brod, &mut run, &to, choices)
}

/// The game of `to`, entered from `from` (the run's game) by the campaign's hand-over.
fn hand_over(install: &DtInstall, from: &Opened, run: &mut Run, to: &str, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let (opened, content): (Opened, Arc<Content>) = open_map(install, to);
    let g = enter(&run.g, from, &opened, None, content).map_err(Stuck::Broken)?;
    Run::resume(g, choices)
}

/// Drinks a potion of the pack for each wounded living unit (a player drinks from the army screen).
fn drink_for_wounded(run: &mut Run) {
    loop {
        let wounded = (0..run.g.squad.len()).find(|&i| run.g.squad[i].alive() && run.g.squad[i].hp < run.g.squad[i].max_hp(&run.g.content));
        let Some(unit) = wounded else { return };
        let drunk = (0..24).find_map(|k| run.g.drink(unit, k).ok().map(|healed| (k, healed)));
        let Some((_, healed)) = drunk else { return };
        run.log.push(format!("drank a potion: {} +{healed} HP", run.squad_names()[unit]));
    }
}

/// The hub's Companies route, as a player plays it: the quests on the way, Kord hired as soon as the purse
/// holds the hire (his offer is shown only then), the council answered «later» until his men are hired, then
/// «companies» at the next weekly offer.
fn hub_companies(install: &DtInstall, seed: u32) -> Result<Run, Stuck> {
    let choices = [
        ("marka_arrival", "take"),
        ("captain", "serve"),
        ("marsh_offer", "accept"),
        ("shrine_offer", "accept"),
        ("shrine_inside", "cleanse"),
        ("kord_offer", "hire"),
        ("council", "later"),
    ];
    let mut run = into_marka(install, seed, &choices)?;
    // The option «Обход армий на пути»: the walk goes round the armies (an army stepped onto stops it).
    run.g.avoid_armies = true;
    drink_for_wounded(&mut run);
    run.walk_to((15, 69), 10)?;
    run.walk_to((30, 84), 12)?;
    run.walk_to((28, 36), 10)?;
    for _ in 0..60 {
        let kord = run.var("kord");
        if kord == Some(VarValue::Int(1)) || kord == Some(VarValue::Int(3)) {
            run.choices.insert("council".into(), "companies".into());
        } else if kord == Some(VarValue::Int(0)) && run.g.gold >= 40 {
            run.walk_to((84, 76), 3)?;
        }
        run.wait_days(1)?;
        if run.done() {
            break;
        }
    }
    Ok(run)
}

/// The camp, entered from the hub's Companies by the campaign's hand-over (the hub must send the hero there).
fn into_camp(install: &DtInstall, seed: u32, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let mut hub = hub_companies(install, seed)?;
    let (marka, _) = open_map(install, "marka");
    let next = after_victory(&mut hub.g, &marka.routes).ok_or_else(|| Stuck::Broken("the hub has no route".into()))?;
    if next != "camp" {
        return Err(Stuck::Broken(format!("the hub sends the hero to {next}, not camp")));
    }
    let mut run = hand_over(install, &marka, &mut hub, "camp", choices)?;
    run.g.avoid_armies = true;
    drink_for_wounded(&mut run);
    Ok(run)
}

/// The cell of the army the story calls `name`, while it is on the map.
fn army_tile(run: &Run, name: &str) -> Option<Tile> {
    let story = run.g.story()?;
    let (&id, _) = story.army_names.iter().find(|(_, n)| n.as_str() == name)?;
    run.g.world.armies.iter().find(|a| a.id == id).map(|a| a.tile(&run.g.world.map))
}

/// The camp's route: the Crown's column comes to the ambush point by the first day (its raid dialog, passed),
/// the red company's offer, the waygate's stone, then the relief fought where it settles in the hall's yard.
pub(crate) fn camp_route(run: &mut Run) -> Result<Option<Outcome>, Stuck> {
    run.wait_days(1)?;
    run.walk_to((74, 44), 6)?;
    run.walk_to((48, 60), 6)?;
    // The relief comes to the hall's yard (60, 58) from the third week; the hero waits for it to settle.
    let mut settled = 0;
    let mut last = None;
    for _ in 0..40 {
        if run.done() {
            return Ok(None);
        }
        let now = army_tile(run, "camp_relief");
        settled = if now.is_some() && now == last { settled + 1 } else { 0 };
        last = now;
        if settled >= 2 {
            break;
        }
        run.wait_days(1)?;
    }
    let Some(cell) = last else { return Ok(None) };
    let before = run.battles.len();
    run.walk_to(cell, 3)?;
    Ok(run.battles[before..].iter().find(|b| b.enemy == "camp_relief").map(|b| b.outcome))
}

/// Walks into the army `name` as it moves, a day at a time, until its battle comes (or `days` pass). Its
/// outcome, None when no battle with it came.
fn fight_army(run: &mut Run, name: &str, days: u64) -> Result<Option<Outcome>, Stuck> {
    for _ in 0..days {
        let before = run.battles.len();
        if let Some(cell) = army_tile(run, name) {
            let _ = run.walk_to(cell, 1);
        }
        if let Some(b) = run.battles[before..].iter().find(|b| b.enemy == name) {
            return Ok(Some(b.outcome));
        }
        run.wait_days(1)?;
        if run.done() {
            break;
        }
    }
    Ok(None)
}

/// The hub's Companies route reaches the camp: over 15 seeds, the council picks the Companies once Kord is
/// hired (the hire costs 40 gold, found on the way), and the campaign routes the hero to the camp.
#[test]
fn the_council_picks_the_companies_once_kord_is_hired() {
    let Some(install) = install() else { return };
    let mut won = 0;
    let mut problems = Vec::new();
    for seed in 1..=15u32 {
        match hub_companies(&install, seed) {
            Ok(mut hub) => {
                let (marka, _) = open_map(&install, "marka");
                let next = after_victory(&mut hub.g, &marka.routes);
                if hub.victory() && hub.var("ally") == Some(VarValue::Int(3)) && next.as_deref() == Some("camp") {
                    won += 1;
                } else {
                    problems.push(format!("seed {seed}: victory {} next {next:?} ally {:?}", hub.victory(), hub.var("ally")));
                }
            }
            Err(e) => problems.push(format!("seed {seed}: {e}")),
        }
    }
    eprintln!("the Companies' council over 15 seeds: {won}/15 {problems:?}");
    assert!(won * 100 >= 70 * 15, "won {won}/15: {problems:?}");
}

/// The council's «later» comes back each week: a hub without the hire is not a dead end.
#[test]
fn the_council_comes_back_after_later() {
    let Some(install) = install() else { return };
    let mut run = into_marka(&install, 1, &[("council", "later"), ("kord_offer", "walk"), ("captain", "serve")]).unwrap_or_else(|e| panic!("{e}"));
    run.g.avoid_armies = true;
    drink_for_wounded(&mut run);
    run.walk_to((15, 69), 10).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to((30, 84), 12).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to((28, 36), 10).unwrap_or_else(|e| panic!("{e}"));
    run.wait_days(30).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    let offered = run.log.iter().filter(|l| l.contains("dialog council")).count();
    assert!(offered >= 4, "the council comes back each week: {offered} times\n{}", run.log.join("\n"));
    assert!(!run.done(), "the hub waits for an ally");
}

/// Kord's offer is shown only when the purse holds the hire: at the first visit he is not offered. Chapter
/// 1's tower now pays its income, so the purse is set here (20 gold; the caravan's 10 on the way keeps it under 40).
#[test]
fn kords_offer_waits_for_the_price() {
    let Some(install) = install() else { return };
    let mut run = into_marka(&install, 1, &[]).unwrap_or_else(|e| panic!("{e}"));
    run.g.avoid_armies = true;
    run.g.gold = 20;
    drink_for_wounded(&mut run);
    run.walk_to((84, 76), 3).unwrap_or_else(|e| panic!("{e}"));
    assert!(run.g.gold < 40, "the purse at the visit: {}", run.g.gold);
    assert!(!run.log.iter().any(|l| l.contains("dialog kord_offer")), "{}", run.log.join("\n"));
    assert_eq!(run.var("kord"), Some(VarValue::Int(0)));
}

/// The camp opens with its arrival dialog, and nothing attacks the hero before the story's relief (day 19):
/// he stands still for eighteen days.
#[test]
fn the_camp_opens_with_its_dialog_and_nothing_attacks_before_the_relief() {
    let Some(install) = install() else { return };
    let mut run = into_camp(&install, 1, CAMP).unwrap_or_else(|e| panic!("{e}"));
    assert!(run.log.iter().any(|l| l.contains("dialog camp_arrival")), "{}", run.log.join("\n"));
    run.wait_days(18).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.battles.is_empty(), "nothing attacks before the relief: {:?}", run.battles);
    assert!(!run.done());
}

/// The camp's offers fire by walking: the Crown's column comes to the ambush point (its raid dialog), the red
/// company's offer (at its camp), the waygate's stone (the gates open). Kord's meeting place is quiet for a
/// company hired in the hub (his men are in the squad).
#[test]
fn the_camp_offers_fire_by_walking() {
    let Some(install) = install() else { return };
    let mut run = into_camp(&install, 1, CAMP).unwrap_or_else(|e| panic!("{e}"));
    run.wait_days(1).unwrap_or_else(|e| panic!("{e}"));
    assert!(run.log.iter().any(|l| l.contains("dialog camp_raid -> pass")), "{}", run.log.join("\n"));
    run.walk_to((74, 44), 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog camp_red -> leave")), "{}", run.log.join("\n"));
    run.walk_to((48, 60), 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog camp_gate -> light")), "{}", run.log.join("\n"));
    assert_eq!(run.var("gates_open"), Some(VarValue::Int(1)), "{}", run.log.join("\n"));
    assert!(!run.log.iter().any(|l| l.contains("dialog camp_kord")), "Kord is hired: {}", run.log.join("\n"));
}

/// The camp's duel: Varga, halberdier of the hero's army, fights the captain of the Crown's guard. She falls
/// in chapter 1's host on this route, so the test carries her in (as chapter 4's tests do). Over 15 seeds.
#[test]
fn the_camp_duel_is_won_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let (camp, _) = open_map(&install, "camp");
    let mut won = 0;
    let mut problems = Vec::new();
    for seed in 1..=15u32 {
        let mut run = match into_camp(&install, seed, &[("camp_duel", "accept"), ("camp_raid", "pass"), ("camp_red", "leave"), ("camp_gate", "light")]) {
            Ok(r) => r,
            Err(e) => {
                problems.push(format!("seed {seed}: {e}"));
                continue;
            }
        };
        Carried { named: vec![("varga", "halberdier", 1)], ..Carried::default() }.apply(&mut run.g, &camp);
        if let Err(e) = run.walk_to((80, 80), 10) {
            problems.push(format!("seed {seed}: {e}"));
            continue;
        }
        match fight_army(&mut run, "duel_captain", 6) {
            Ok(Some(Outcome::Victory)) => won += 1,
            other => problems.push(format!("seed {seed}: {other:?}; {}", run.log.join(" | "))),
        }
    }
    eprintln!("the camp's duel over 15 seeds: {won}/15 {problems:?}");
    assert!(won * 100 >= 70 * 15, "won {won}/15: {problems:?}");
}

/// The camp's route over 15 seeds: the relief is fought where it settles, the chapter's victory fires, and
/// `camp_done` is set (the campaign's route then goes back to the hub).
#[test]
fn the_camp_is_won_on_its_relief_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let mut won = 0;
    let mut problems = Vec::new();
    for seed in 1..=15u32 {
        let mut run = into_camp(&install, seed, CAMP).unwrap_or_else(|e| panic!("{e}"));
        let out = camp_route(&mut run);
        if matches!(out, Ok(Some(Outcome::Victory))) && run.victory() && run.var("camp_done") == Some(VarValue::Bool(true)) {
            won += 1;
        } else {
            problems.push(format!("seed {seed}: {out:?}; {}", run.log.join(" | ")));
        }
    }
    eprintln!("the camp over 15 seeds: {won}/15 {problems:?}");
    assert!(won * 100 >= 70 * 15, "won {won}/15: {problems:?}");
}

/// The camp ends and the campaign takes the hero back to the hub, which keeps its state: the hub's revisit
/// (`m_return_camp`) ends its chapter, and the campaign routes it to the gates.
#[test]
fn the_hub_takes_the_hero_back_from_the_camp_to_the_gates() {
    let Some(install) = install() else { return };
    let mut camp_run = into_camp(&install, 1, CAMP).unwrap_or_else(|e| panic!("{e}"));
    camp_route(&mut camp_run).unwrap_or_else(|e| panic!("{e}\n{}", camp_run.log.join("\n")));
    assert!(camp_run.victory(), "{}", camp_run.log.join("\n"));
    let (camp, _) = open_map(&install, "camp");
    let (marka, marka_content) = open_map(&install, "marka");
    assert_eq!(after_victory(&mut camp_run.g, &camp.routes).as_deref(), Some("marka"), "the camp returns to the hub");
    let g = enter(&camp_run.g, &camp, &marka, None, marka_content).unwrap_or_else(|e| panic!("{e}"));
    let mut hub = Run::resume(g, &[]).unwrap_or_else(|e| panic!("{e}"));
    hub.wait_until_done(3).unwrap_or_else(|e| panic!("{e}\n{}", hub.log.join("\n")));
    assert!(hub.done(), "the hub's revisit ends its chapter: {}", hub.log.join("\n"));
    assert_eq!(hub.var("camp_done"), Some(VarValue::Bool(true)));
    assert_eq!(after_victory(&mut hub.g, &marka.routes).as_deref(), Some("gates"), "the campaign goes on to the gates");
}

/// Each branch's flag ends the hub's chapter and routes it to the gates (a constructed first visit of the hub
/// with the flag set; the revisit with kept state is the test above).
#[test]
fn each_branchs_done_flag_routes_the_hub_to_the_gates() {
    let Some(install) = install() else { return };
    for (flag, ally) in [("abbey_done", 1), ("salt_done", 2), ("camp_done", 3)] {
        let (marka, content) = open_map(&install, "marka");
        let carry = Carried { gold: Some(100), vars: vec![("ally".into(), VarValue::Int(ally)), (flag.into(), VarValue::Bool(true))], ..Carried::default() };
        let mut run = Run::carried(&marka, content, 1, &[], &carry).unwrap_or_else(|e| panic!("{e}"));
        run.wait_until_done(3).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert!(run.done(), "{flag}: the hub ends its chapter: {}", run.log.join("\n"));
        assert_eq!(after_victory(&mut run.g, &marka.routes).as_deref(), Some("gates"), "{flag} routes to the gates");
    }
}
