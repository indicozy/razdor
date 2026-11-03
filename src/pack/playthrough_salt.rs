//! Chapter 3б «Солончаки» (`salt`) of «Пепельная Марка», played by the harness of `playthrough`:
//! the marsh folk's branch, entered from the hub by the campaign's hand-over. The hero rides in
//! disguise as a bandit (the Crown's patrol and bounty men are friendly to him, see triggers/salt.toml),
//! walks to Reedmere, the Tess trail and the Salt Gate, and the chapter ends back in the hub. The
//! skipped tests need `RAZDOR_DT_DIR`.

use std::sync::Arc;

use super::open::Opened;
use super::playthrough::{install, open_map, Run, Stuck};
use super::playthrough_ch23::drink_for_wounded;
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::battle::Outcome;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::content::Content;
use crate::rules::map::Tile;
use crate::rules::world::Army;

/// The cell where the marsh folk's village Reedmere stands (its anchor).
const REEDMERE: Tile = (22, 60);
/// The Salt Gate's anchor cell: the castle's footprint is the four cells up and left of it.
const SALT_GATE: Tile = (70, 40);

/// The hub's hand-over to the marsh folk's branch: chapter 1 won on the prepared route, the hub's
/// quests done and the council answered `marsh`, then the campaign's own hand-over to `to` (the
/// branch). Mirrors `playthrough_ch23`'s helpers, which are private to its test module; the hub
/// visits what the chapter 2 playthrough visits (the marsh quest, the wraiths, the shrine).
fn branch(install: &DtInstall, seed: u32, to: &str, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    branch_by(install, seed, to, choices, false)
}

/// [`branch`], or with `as_played`: the hub as the campaign plays it (the walk round the armies, the
/// walk to Ashford, the healing and the hiring within means), so the squad is the one the route has at
/// the salt (ten units, not the four of the hub without the hire).
fn branch_by(install: &DtInstall, seed: u32, to: &str, choices: &[(&str, &str)], as_played: bool) -> Result<Run, Stuck> {
    use super::playthrough_campaign::{heal_within_means, hire_within_means};
    // Chapter one as the campaign plays it (its side quests, the raids, the hiring at the tower): the
    // gold it carries is the tower's income over its days.
    let (brod, _) = open_map(install, "brod");
    let mut brod_run = super::playthrough_campaign::chapter_one(install, seed)?;
    let first = after_victory(&mut brod_run.g, &brod.routes).ok_or_else(|| Stuck::Broken("chapter one has no next map".into()))?;
    let (marka, marka_content) = open_map(install, &first);
    let g = enter(&brod_run.g, &brod, &marka, None, marka_content).map_err(Stuck::Broken)?;
    let council = [("council", "marsh"), ("shrine_inside", "cleanse"), ("shrine_offer", "accept"), ("marsh_offer", "accept"), ("marka_arrival", "take")];
    let mut hub = Run::resume(g, &council)?;
    if as_played {
        hub.g.avoid_armies = true;
    }
    drink_for_wounded(&mut hub);
    if as_played {
        hub.walk_to((58, 44), 2)?;
        heal_within_means(&mut hub);
        hire_within_means(&mut hub);
    }
    hub.walk_to((15, 69), 10)?;
    hub.walk_to((30, 84), 12)?;
    hub.walk_to((28, 36), 10)?;
    hub.wait_until_done(40)?;
    let (hub_opened, _) = open_map(install, "marka");
    let next = after_victory(&mut hub.g, &hub_opened.routes).ok_or_else(|| Stuck::Broken("the hub has no route".into()))?;
    if next != to {
        return Err(Stuck::Broken(format!("the hub sends the hero to {next}, not {to}")));
    }
    let (salt, salt_content): (Opened, Arc<Content>) = open_map(install, to);
    let g = enter(&hub.g, &hub_opened, &salt, None, salt_content).map_err(Stuck::Broken)?;
    let mut run = Run::resume(g, choices)?;
    drink_for_wounded(&mut run);
    Ok(run)
}

/// The army the story calls `name` (its name in the story's army names), while it is on the map.
fn army_named<'a>(run: &'a Run, name: &str) -> Option<&'a Army> {
    let story = run.g.story()?;
    let (&id, _) = story.army_names.iter().find(|(_, n)| n.as_str() == name)?;
    run.g.world.armies.iter().find(|a| a.id == id)
}

/// The Crown's attitude to the hero of the army `name` (below 0 is hostile).
fn attitude_of(run: &Run, name: &str) -> i8 {
    army_named(run, name).unwrap_or_else(|| panic!("no army {name}: {}", run.log.join("\n"))).attitude
}

/// The chapter's route: Reedmere (the marsh folk's dialog), then the Salt Gate, walked over until the
/// gate falls or the walk is stuck. The walk ends at each battle or teleport, so it is repeated.
fn salt_route(run: &mut Run) -> Result<(), Stuck> {
    run.walk_to(REEDMERE, 20)?;
    for _ in 0..6 {
        if run.done() {
            break;
        }
        run.walk_to(SALT_GATE, 4)?;
    }
    Ok(())
}

#[test]
fn the_branch_opens_quiet_with_its_arrival_dialog() {
    let Some(install) = install() else { return };
    let mut run = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    assert!(run.log.iter().any(|l| l.contains("dialog salt_arrival")), "{:?}", run.log);
    run.wait_days(20).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.battles.is_empty(), "nothing attacks before the story means it to: {:?}", run.battles);
    assert!(attitude_of(&run, "patrol_north") < 0 && attitude_of(&run, "bounty") < 0, "the Crown is hostile to the hero before the disguise");
}

#[test]
fn reedmere_disguise_calms_the_crown_and_openly_it_does_not() {
    let Some(install) = install() else { return };
    let mut run = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(REEDMERE, 20).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog salt_reedmere -> disguise")), "{:?}", run.log);
    assert_eq!(run.var("salt_disguise"), Some(VarValue::Bool(true)));
    assert!(attitude_of(&run, "patrol_north") > 0, "the disguised hero is friendly to the Crown's patrol");
    assert!(attitude_of(&run, "bounty") > 0, "and to the bounty men");
    let mut open = branch(&install, 1, "salt", &[("salt_reedmere", "openly")]).unwrap_or_else(|e| panic!("{e}"));
    open.walk_to(REEDMERE, 20).unwrap_or_else(|e| panic!("{e}\n{}", open.log.join("\n")));
    assert_eq!(open.var("salt_disguise"), Some(VarValue::Bool(false)));
    assert!(attitude_of(&open, "patrol_north") < 0 && attitude_of(&open, "bounty") < 0, "openly, the Crown stays hostile");
}

/// The Crown's patrol reacts to the hero near its road: openly (its attack goal, set by the Reedmere dialog's
/// «openly») it hunts him; disguised, it is friendly and leaves him alone. A hero who walks onto the
/// patrol's cell is a deliberate click, a fight whatever the attitude (the original).
#[test]
fn the_patrol_chases_the_open_hero_and_leaves_the_disguised_one() {
    let Some(install) = install() else { return };
    for (choice, hostile) in [("disguise", false), ("openly", true)] {
        let mut run = branch(&install, 1, "salt", &[("salt_reedmere", choice)]).unwrap_or_else(|e| panic!("{e}"));
        run.walk_to(REEDMERE, 20).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        // Openly, the patrol hunts him and beats this squad (a Stuck::Lost): the open road is the hard one.
        let walked = run.walk_to((46, 26), 15);
        let waited = if walked.is_ok() { run.wait_days(3) } else { walked };
        let fought = run.battles.iter().any(|b| b.enemy == "patrol_north");
        assert_eq!(fought, hostile, "{choice}: battles {:?}\n{}", run.battles, run.log.join("\n"));
        assert_eq!(waited.is_ok(), !hostile, "{choice}: {waited:?}");
    }
}

#[test]
fn the_trail_the_witch_and_the_bog_fire_on_the_way() {
    let Some(install) = install() else { return };
    let mut run = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    run.walk_to(REEDMERE, 20).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    run.walk_to((10, 76), 10).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog salt_bog")), "the bog's fog warns the hero: {:?}", run.log);
    run.g.triggers.vars.insert("taint".into(), VarValue::Int(1));
    run.walk_to((10, 40), 10).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog salt_witch")), "the witch takes the tainted hero: {:?}", run.log);
    let mut trail = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    trail.walk_to(REEDMERE, 20).unwrap_or_else(|e| panic!("{e}\n{}", trail.log.join("\n")));
    trail.walk_to(SALT_GATE, 10).unwrap_or_else(|e| panic!("{e}\n{}", trail.log.join("\n")));
    assert!(trail.log.iter().any(|l| l.contains("dialog salt_trail")), "Tess's trail takes the hero through: {:?}", trail.log);
}

#[test]
fn the_disguised_hero_is_caught_at_the_gate_and_takes_it() {
    let Some(install) = install() else { return };
    let mut run = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    salt_route(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.log.iter().any(|l| l.contains("dialog salt_caught -> draw")), "{}", run.log.join("\n"));
    assert!(run.battles.iter().any(|b| b.enemy == "The Salt Gate" && b.outcome == Outcome::Victory), "{:?}", run.battles);
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert_eq!(run.var("salt_done"), Some(VarValue::Bool(true)));
}

/// The route's win over twenty seeds (the game's generator and the events' draws; the battles are
/// deterministic, the walk and the rolls change with the seed).
#[test]
fn the_salt_route_is_won_over_twenty_seeds() {
    let Some(install) = install() else { return };
    let started = std::time::Instant::now();
    let (mut wins, mut problems) = (0, Vec::new());
    for seed in 1..=20u32 {
        match branch(&install, seed, "salt", &[]) {
            Ok(mut run) => match salt_route(&mut run) {
                Ok(()) if run.victory() => wins += 1,
                r => problems.push(format!("seed {seed}: {r:?}; {}", run.log.join(" | "))),
            },
            Err(e) => problems.push(format!("seed {seed}: hand-over {e}")),
        }
    }
    eprintln!("salt route over 20 seeds: {wins}/20 in {:.2}s; {problems:?}", started.elapsed().as_secs_f64());
    assert!(wins * 100 >= 70 * 20, "won {wins}/20: {problems:?}");
}

/// The route ends on `salt_done`; the campaign goes back to the hub; the hub's revisit ends the chapter
/// and the campaign goes on to the gates.
#[test]
fn the_chapter_ends_back_in_marka_and_on_to_the_gates() {
    let Some(install) = install() else { return };
    let mut run = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    salt_route(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    let (salt, _) = open_map(&install, "salt");
    let (marka, marka_content) = open_map(&install, "marka");
    assert_eq!(after_victory(&mut run.g, &salt.routes).as_deref(), Some("marka"), "the chapter goes back to the hub");
    let g = enter(&run.g, &salt, &marka, None, marka_content).unwrap_or_else(|e| panic!("{e}"));
    let mut hub = Run::resume(g, &[]).unwrap_or_else(|e| panic!("{e}"));
    hub.wait_until_done(5).unwrap_or_else(|e| panic!("{e}"));
    assert!(hub.log.iter().any(|l| l.contains("dialog revisit_salt")), "{:?}", hub.log);
    assert!(hub.done(), "the hub's revisit ends the chapter: {:?}", hub.log);
    assert_eq!(after_victory(&mut hub.g, &marka.routes).as_deref(), Some("gates"), "salt_done sends the campaign to the gates");
}

/// The gold and the squad over the route: the day-by-day curve, printed for the report. No unit
/// deserts on the route (the squad at the gate is the squad at the hand-over).
#[test]
fn the_gold_curve_on_the_route() {
    let Some(install) = install() else { return };
    let mut run = branch(&install, 1, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
    let start = (run.g.gold, run.g.squad.len());
    eprintln!("GOLD start: gold {} squad {} ({:?})", start.0, start.1, run.squad_names());
    let mut days = Vec::new();
    run.walk_to(REEDMERE, 20).unwrap_or_else(|e| panic!("{e}"));
    days.push((run.day(), run.g.gold, run.g.squad.len()));
    for _ in 0..6 {
        if run.done() {
            break;
        }
        let _ = run.walk_to(SALT_GATE, 4);
        days.push((run.day(), run.g.gold, run.g.squad.len()));
    }
    eprintln!("GOLD curve (day, gold, squad): {days:?}");
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert_eq!(run.g.squad.len(), start.1, "no unit deserts on the route");
}

/// The route with the default options: «Let pass» off, so a step onto a friendly army is a fight (the
/// original's rule). The route bumps into no army (the bounty men and the ghosts stand off the road).
#[test]
fn the_route_is_won_with_the_default_options() {
    let Some(install) = install() else { return };
    let (mut wins, mut lost) = (0, Vec::new());
    for seed in 1..=10u32 {
        let mut run = branch(&install, seed, "salt", &[]).unwrap_or_else(|e| panic!("{e}"));
        run.g.friends_let_pass = false;
        let r = salt_route(&mut run);
        if run.victory() {
            wins += 1;
        } else {
            lost.push(format!("seed {seed}: {r:?}; {}", run.log.join(" | ")));
        }
    }
    assert!(wins * 100 >= 70 * 10, "won {wins}/10: {lost:?}");
}


/// The two optional fights of the salt, with the squad the route has there (ten units, as played): the
/// Crown's watch post is taken by a walk into it, and the patrol that hunts him when he rides openly is
/// met on the road. Each is won in most of fifteen seeds (the battles have no randomness; the walk does).
#[test]
fn the_route_squad_wins_the_watch_and_the_openly_patrol_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let (mut watch_wins, mut patrol_wins) = (0, 0);
    let mut problems = Vec::new();
    for seed in 1..=15u32 {
        match branch_by(&install, seed, "salt", &[("salt_reedmere", "disguise")], true) {
            Ok(mut run) => {
                let _ = run.walk_to((56, 18), 6);
                if run.battles.iter().any(|b| b.enemy == "The watch post" && b.outcome == Outcome::Victory) {
                    watch_wins += 1;
                } else {
                    problems.push(format!("watch, seed {seed}: {}", run.log.join(" | ")));
                }
            }
            Err(e) => problems.push(format!("watch, seed {seed}: {e}")),
        }
        match branch_by(&install, seed, "salt", &[("salt_reedmere", "openly")], true) {
            Ok(mut run) => {
                let _ = run.walk_to((46, 26), 15);
                for _ in 0..12 {
                    if run.battles.iter().any(|b| b.enemy == "patrol_north") || run.done() {
                        break;
                    }
                    if let Some(t) = army_named(&run, "patrol_north").map(|a| a.tile(&run.g.world.map)) {
                        let _ = run.walk_to(t, 1);
                    }
                    if run.battles.iter().any(|b| b.enemy == "patrol_north") {
                        break;
                    }
                    let _ = run.wait_days(1);
                }
                if run.battles.iter().any(|b| b.enemy == "patrol_north" && b.outcome == Outcome::Victory) {
                    patrol_wins += 1;
                } else {
                    problems.push(format!("openly, seed {seed}: {}", run.log.join(" | ")));
                }
            }
            Err(e) => problems.push(format!("openly, seed {seed}: {e}")),
        }
    }
    assert!(watch_wins * 100 >= 50 * 15, "the watch won {watch_wins}/15: {problems:?}");
    assert!(patrol_wins * 100 >= 50 * 15, "the openly patrol won {patrol_wins}/15: {problems:?}");
}
