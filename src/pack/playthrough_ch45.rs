//! Chapters 4 «Врата» and 5 «Курганы» of «Пепельная Марка», played through the harness
//! (`pack::playthrough`): the hero walks, the battles are fought (`sim::fight_it_out`), the dialogs
//! are answered by a script. What the earlier chapters carry in is `Carried`, one state per branch
//! of the hub. Skipped without `RAZDOR_DT_DIR`.

use super::playthrough::{install, open_map, Carried, Run, Stuck};
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::battle::Outcome;
use crate::rules::map::Tile;

fn int(name: &str, v: i64) -> (String, VarValue) {
    (name.to_string(), VarValue::Int(v))
}

fn flag(name: &str, v: bool) -> (String, VarValue) {
    (name.to_string(), VarValue::Bool(v))
}

/// What every branch carries: the hub's standing, Varga's halberdier, two hunters and three
/// crossbowmen of the tower, and the hero's three militia (his start troops): twelve with the hero,
/// the formation's capacity. The gold is the purse at the chapter's start (the wages are paid at
/// every noon, and a chapter's income is what its buildings give: see the report of the balance).
fn base(ally: i64, gold: i32, vars: Vec<(String, VarValue)>) -> Carried {
    let mut all = vec![int("ally", ally), int("dread", 2), flag("varga_alive", true), flag("varga_joined", true), int("crown", 1), int("order", 0)];
    all.extend(vars);
    Carried { gold: Some(gold), vars: all, squad: vec![("crossbowman", 1, 3), ("hunter", 1, 2)], named: vec![("varga", "halberdier", 1)], avoid_armies: true }
}

/// Branch 1, the Order: Brother Ilm, the novice pilgrim, walks with the hero (`ilm`: 1 the
/// inquisitor's path, 2 the heretic's). The Order's inquisitors come to the gates as an army.
fn order_branch(ilm: i64, gold: i32) -> Carried {
    let mut c = base(1, gold, vec![int("order", 2), int("ilm", ilm), flag("abbey_done", true)]);
    c.named.push(("ilm", "pilgrim", 1));
    c
}

/// Branch 2, the marsh folk: Tess the hunter joins; the marsh trust is high.
fn marsh_branch(gold: i32) -> Carried {
    let mut c = base(2, gold, vec![int("marsh_trust", 3), flag("tess_joined", true), int("ilm", 0), flag("salt_done", true)]);
    c.squad.push(("crossbowman", 1, 1));
    c.named.push(("tess", "hunter", 1));
    c
}

/// Branch 3, the Free Companies: Kord's hired company walks with the hero, paid weekly.
fn companies_branch(gold: i32) -> Carried {
    let mut c = base(3, gold, vec![int("companies", 2), int("kord", 1), int("ilm", 0), flag("camp_done", true)]);
    c.squad = vec![("crossbowman", 1, 2), ("swordsman", 1, 1), ("bandit", 1, 1), ("archer", 1, 1)];
    c
}

/// The gates' cells: the stones, the guards at their feet.
const STONE_N: Tile = (48, 14);
const STONE_E: Tile = (84, 44);
const STONE_S: Tile = (30, 62);
const STONE_W: Tile = (14, 40);
const GUARD_N: Tile = (46, 16);
const GUARD_E: Tile = (82, 46);
const GUARD_W: Tile = (12, 42);

/// Walks to `cell` (a guard's cell is a fight: the walk ends on it), up to `days`.
pub(crate) fn to(run: &mut Run, cell: Tile, days: u64) -> Result<(), Stuck> {
    run.walk_to(cell, days)
}

/// The Order's route: the east gate by Ilm's rite, the south gate by the gatekeeper's gold, the
/// west and north guards fought; the last gate's «close» ends the chapter.
pub(crate) fn gates_by_the_order(run: &mut Run) -> Result<(), Stuck> {
    to(run, STONE_E, 12)?;
    to(run, STONE_S, 12)?;
    to(run, GUARD_W, 12)?;
    to(run, GUARD_N, 12)?;
    run.wait_until_done(5)
}

/// The marsh folk's route: the west gate by Tess's path, the south gate by Varga's halberd, the
/// east and north guards fought.
pub(crate) fn gates_by_the_marsh(run: &mut Run) -> Result<(), Stuck> {
    to(run, STONE_W, 12)?;
    to(run, STONE_S, 12)?;
    to(run, GUARD_E, 12)?;
    to(run, GUARD_N, 12)?;
    run.wait_until_done(5)
}

/// The Companies' route: the south gate by the gold, the other three guards fought.
pub(crate) fn gates_by_the_companies(run: &mut Run) -> Result<(), Stuck> {
    to(run, STONE_S, 12)?;
    to(run, GUARD_E, 12)?;
    to(run, GUARD_W, 12)?;
    to(run, GUARD_N, 12)?;
    run.wait_until_done(5)
}

fn gates_run(install: &DtInstall, carry: &Carried, seed: u32, choices: &[(&str, &str)]) -> Run {
    let (opened, content) = open_map(install, "gates");
    Run::carried(&opened, content, seed, choices, carry).unwrap_or_else(|e| panic!("{e}"))
}

fn barrows_run(install: &DtInstall, carry: &Carried, seed: u32, choices: &[(&str, &str)]) -> Run {
    let (opened, content) = open_map(install, "barrows");
    Run::carried(&opened, content, seed, choices, carry).unwrap_or_else(|e| panic!("{e}"))
}

/// The cell of the army the story calls `name`, while it is on the map.
fn army_tile(run: &Run, name: &str) -> Option<Tile> {
    let story = run.g.story()?;
    let (&id, _) = story.army_names.iter().find(|(_, n)| n.as_str() == name)?;
    run.g.world.armies.iter().find(|a| a.id == id).map(|a| a.tile(&run.g.world.map))
}

/// Waits (a day at a time, the hero standing still) until the army `name` stands on `cell`, its
/// goal, then walks into it. Returns its battle's outcome, None when no such battle came.
fn meet_army_at(run: &mut Run, name: &str, cell: Tile, days: u64) -> Result<Option<Outcome>, Stuck> {
    for _ in 0..days {
        if army_tile(run, name) == Some(cell) {
            let before = run.battles.len();
            run.walk_to(cell, 2)?;
            return Ok(run.battles[before..].iter().find(|b| b.enemy == name).map(|b| b.outcome));
        }
        run.wait_days(1)?;
        if run.done() {
            break;
        }
    }
    Ok(None)
}

/// The dread climbs a point a week while the hero stands still; each host that comes next to him
/// is fought (he steps out and back, so the host's attack comes on his step). Runs `days` days or
/// until the chapter is over or a battle is lost; the battles are in `run.battles`.
fn progression(run: &mut Run, days: u64) {
    for _ in 0..days {
        if run.wait_days(1).is_err() || run.done() || run.battles.last().is_some_and(|b| b.outcome == Outcome::Defeat) {
            break;
        }
        let here = run.g.tile();
        let near = (1..=7).any(|k| army_tile(run, &format!("bw_king_{k}")).is_some_and(|t| (t.0 - here.0).abs() <= 3 && (t.1 - here.1).abs() <= 3));
        if !near {
            continue;
        }
        let cells: Vec<Tile> = (-1..=1).flat_map(|dy| (-1..=1).map(move |dx| (here.0 + dx, here.1 + dy))).filter(|&n| n != here).collect();
        for n in cells {
            if run.walk_to(n, 1).is_ok() {
                let _ = run.walk_to(here, 1);
                break;
            }
        }
    }
}

/// The King's road: the hero walks to the King's step and fights him; the summaries and the council
/// follow, and the council's choice ends the chapter.
pub(crate) fn barrows_by_the_king(run: &mut Run) -> Result<(), Stuck> {
    to(run, (166, 36), 40)?;
    run.wait_until_done(3)
}

/// The grey throne: a heretic who heard the wraith kneels at the palace.
pub(crate) fn barrows_by_the_grey_throne(run: &mut Run) -> Result<(), Stuck> {
    to(run, (166, 26), 40)?;
    run.wait_until_done(3)
}

/// The ending the story reached, by its dialog's name without `end_` (`iron`, `free`, `grey`, `fall`).
pub(crate) fn ending_of(run: &Run) -> Option<String> {
    run.ending().map(|d| d.trim_start_matches("end_").to_string())
}

/// Plays `play` over `seeds` seeds and returns the wins, with the first failure's log.
fn sweep(seeds: u32, mut play: impl FnMut(u32) -> (bool, String)) -> (u32, Option<String>) {
    let mut wins = 0;
    let mut first = None;
    for seed in 1..=seeds {
        let (won, log) = play(seed);
        if won {
            wins += 1;
        } else if first.is_none() {
            first = Some(format!("seed {seed}: {log}"));
        }
    }
    (wins, first)
}

#[test]
fn the_order_wins_the_gates() {
    let Some(install) = install() else { return };
    let mut run = gates_run(&install, &order_branch(1, 1500), 1, &[]);
    gates_by_the_order(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert!(run.battles.iter().any(|b| b.enemy == "guard_n" && b.outcome == Outcome::Victory), "{:?}", run.battles);
}

#[test]
fn the_marsh_folk_win_the_gates() {
    let Some(install) = install() else { return };
    let mut run = gates_run(&install, &marsh_branch(1500), 1, &[("gt_closed_s", "varga")]);
    gates_by_the_marsh(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog gt_closed_s -> varga")), "{:?}", run.log);
}

#[test]
fn the_companies_win_the_gates() {
    let Some(install) = install() else { return };
    let mut run = gates_run(&install, &companies_branch(1500), 1, &[]);
    gates_by_the_companies(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert!(run.victory(), "{}", run.log.join("\n"));
}

#[test]
fn gates_routes_win_over_twenty_seeds() {
    let Some(install) = install() else { return };
    type Route = fn(&mut Run) -> Result<(), Stuck>;
    type Choices = &'static [(&'static str, &'static str)];
    let routes: [(&str, Carried, Choices, Route); 3] = [
        ("the Order", order_branch(1, 1500), &[], gates_by_the_order),
        ("the marsh folk", marsh_branch(1500), &[("gt_closed_s", "varga")], gates_by_the_marsh),
        ("the Companies", companies_branch(1500), &[], gates_by_the_companies),
    ];
    for (name, carry, choices, route) in routes {
        let (wins, first) = sweep(20, |seed| {
            let mut run = gates_run(&install, &carry, seed, choices);
            let r = route(&mut run);
            (r.is_ok() && run.victory(), format!("{r:?}; {}", run.log.join(" | ")))
        });
        assert!(wins * 100 >= 70 * 20, "{name}: won {wins}/20, first failure {first:?}");
    }
}

#[test]
fn the_heretic_hears_the_wraith_and_the_grey_bargain_is_open() {
    let Some(install) = install() else { return };
    let mut run = gates_run(&install, &order_branch(2, 1500), 1, &[("gt_messenger", "listen")]);
    to(&mut run, (56, 46), 12).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    to(&mut run, (60, 44), 2).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(run.var("grey_bargain_offered"), Some(VarValue::Bool(true)), "{}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog gt_messenger -> listen")), "{}", run.log.join("\n"));
}

#[test]
fn a_stone_teleports_the_hero_to_another_open_gate() {
    let Some(install) = install() else { return };
    let mut run = gates_run(&install, &order_branch(1, 1500), 1, &[("gt_stone_n", "to_e")]);
    to(&mut run, GUARD_N, 12).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    to(&mut run, STONE_E, 12).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    to(&mut run, STONE_N, 12).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(run.g.tile(), (84, 41), "the stone lands the hero by the east gate: {}", run.log.join("\n"));
    assert!(run.log.iter().any(|l| l.contains("dialog gt_stone_n -> to_e")), "{}", run.log.join("\n"));
}

/// Each host of the dead meets the hero fresh: the hero waits (the dread climbs a point a week),
/// the host comes to its stone, and the hero walks into it.
#[test]
fn the_hosts_of_the_dead_are_beatable() {
    let Some(install) = install() else { return };
    for (name, cell, weeks) in [("gt_host_a", (84, 46), 1u64), ("gt_host_b", (14, 38), 3), ("gt_host_c", (30, 60), 5)] {
        let mut run = gates_run(&install, &order_branch(1, 20_000), 1, &[]);
        run.wait_days(weeks * 7 + 1).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        let out = meet_army_at(&mut run, name, cell, 14).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
        assert_eq!(out, Some(Outcome::Victory), "{name} on day {:.1}: {}", run.day(), run.log.join("\n"));
    }
}

#[test]
fn the_three_council_endings_are_reached_on_the_road() {
    let Some(install) = install() else { return };
    // The Iron Peace: the Crown at 1 and the Order at 2 (the order branch's standing).
    let mut run = barrows_run(&install, &order_branch(1, 2000), 1, &[]);
    barrows_by_the_king(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(ending_of(&run).as_deref(), Some("iron"), "{}", run.log.join("\n"));
    assert!(run.done(), "the campaign ends on the ending action: {}", run.log.join("\n"));
    // The Free March: the Companies at 2 or more, the Crown at 0.
    let mut c = companies_branch(2000);
    c.vars.push(int("crown", 0));
    let mut run = barrows_run(&install, &c, 1, &[]);
    barrows_by_the_king(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(ending_of(&run).as_deref(), Some("free"), "{}", run.log.join("\n"));
    // The fall: neither standing (the Crown, the Order and the Companies at 0, the marsh folk at 2).
    let mut c = marsh_branch(2000);
    c.vars.extend([int("crown", 0), int("order", 0), int("companies", 0), int("marsh_trust", 2)]);
    let mut run = barrows_run(&install, &c, 1, &[]);
    barrows_by_the_king(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(ending_of(&run).as_deref(), Some("fall"), "{}", run.log.join("\n"));
}

#[test]
fn the_grey_bargain_is_the_third_ending_at_the_grey_throne() {
    let Some(install) = install() else { return };
    let mut c = order_branch(2, 2000);
    c.vars.push(flag("grey_bargain_offered", true));
    let mut run = barrows_run(&install, &c, 1, &[("bw_grey_throne", "kneel")]);
    barrows_by_the_grey_throne(&mut run).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    assert_eq!(ending_of(&run).as_deref(), Some("grey"), "{}", run.log.join("\n"));
    assert!(run.done(), "{}", run.log.join("\n"));
}

/// The endings' roads over fifteen seeds: the King's host is fought on each (the roads of the iron,
/// the free and the fall endings), and the grey throne's road.
#[test]
fn the_endings_win_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let mut free = companies_branch(2000);
    free.vars.push(int("crown", 0));
    let mut fall = marsh_branch(2000);
    fall.vars.extend([int("crown", 0), int("order", 0), int("companies", 0), int("marsh_trust", 2)]);
    let mut grey = order_branch(2, 2000);
    grey.vars.push(flag("grey_bargain_offered", true));
    let roads: [(&str, Carried, &str); 4] = [
        ("iron", order_branch(1, 2000), "iron"),
        ("free", free, "free"),
        ("fall", fall, "fall"),
        ("grey", grey, "grey"),
    ];
    for (name, carry, want) in roads {
        let (wins, first) = sweep(15, |seed| {
            let mut run = barrows_run(&install, &carry, seed, &[("bw_grey_throne", "kneel")]);
            let r = if want == "grey" { barrows_by_the_grey_throne(&mut run) } else { barrows_by_the_king(&mut run) };
            (r.is_ok() && ending_of(&run).as_deref() == Some(want), format!("{r:?}; {}", run.log.join(" | ")))
        });
        assert!(wins * 100 >= 70 * 15, "{name}: won {wins}/15, first failure {first:?}");
    }
}

/// The dread-scaled hosts of the King, met by a hero who waits: the first four are beaten in every
/// seed, in order. (The fifth, with a death knight, breaks the squad after four fights without
/// healing: an open balance question for the owner.)
#[test]
fn the_dread_hosts_one_to_four_are_beaten_by_a_waiting_hero() {
    let Some(install) = install() else { return };
    for seed in 1..=6u32 {
        let carry = Carried { gold: Some(100_000), vars: vec![int("ally", 0), int("dread", 2)], squad: vec![("crossbowman", 1, 3), ("hunter", 1, 2), ("militiaman", 1, 1)], named: vec![("varga", "halberdier", 1)], avoid_armies: true };
        let mut run = barrows_run(&install, &carry, seed, &[]);
        progression(&mut run, 80);
        let names: Vec<String> = run.battles.iter().map(|b| b.enemy.clone()).collect();
        let outcomes: Vec<Outcome> = run.battles.iter().map(|b| b.outcome).collect();
        assert_eq!(names, ["bw_king_1", "bw_king_2", "bw_king_3", "bw_king_4"], "seed {seed}: {}", run.log.join("\n"));
        assert!(outcomes.iter().all(|&o| o == Outcome::Victory), "seed {seed}: {:?}", run.battles);
    }
}

/// The allies' columns march from their spawn cells to the palace over routes of 166 to 183 cells. The
/// goal planner once gave up at 4000 nodes and found none, so the goal dropped and the column wandered.
#[test]
fn the_allies_routes_to_the_palace_are_planned() {
    let Some(install) = install() else { return };
    let run = barrows_run(&install, &order_branch(1, 2000), 1, &[]);
    let g = &run.g;
    let i = g.world.armies.iter().position(|a| !a.sails()).expect("a land army");
    for from in [(44, 186), (22, 186), (176, 176)] {
        let route = g.goal_route(i, from, (160, 44));
        assert!(route.len() >= 160, "from {from:?} the planner finds {} cells", route.len());
        assert_eq!(route.last(), Some(&(160, 44)), "the route ends at the palace");
    }
}

/// The Order's column walks from its spawn to the palace, and its arrival takes two points of dread.
#[test]
fn the_allies_column_reaches_the_palace_and_the_dread_falls() {
    let Some(install) = install() else { return };
    let mut run = barrows_run(&install, &order_branch(1, 2000), 1, &[]);
    let mut dread_before = None;
    let mut arrived = None;
    for _ in 0..40 {
        if run.wait_days(1).is_err() || run.done() {
            break;
        }
        if army_tile(&run, "bw_order") == Some((160, 44)) {
            arrived = run.var("dread");
            break;
        }
        if army_tile(&run, "bw_order").is_some() {
            dread_before = run.var("dread");
        }
    }
    let (Some(VarValue::Int(before)), Some(VarValue::Int(after))) = (dread_before, arrived) else {
        panic!("the column never reached the palace: {}", run.log.join("\n"));
    };
    assert!(after <= before - 2, "the arrival drops the dread by 2: {before} -> {after}\n{}", run.log.join("\n"));
}

/// Whether the army the story calls `name` was raised on the map (spawned, beaten or not).
fn spawned(run: &Run, name: &str) -> bool {
    let Some(story) = run.g.story() else { return false };
    story.army_names.iter().find(|(_, n)| n.as_str() == name).is_some_and(|(id, _)| run.g.triggers.spawned.contains(id))
}

/// Chapter 5's last host (dread 16) is one fewer when chapter one broke the barrow mound or sealed it: the
/// King sends one host fewer. Each case starts at dread 16, so the first week raises every host due.
#[test]
fn the_last_host_is_one_fewer_after_the_barrow() {
    let Some(install) = install() else { return };
    type Vars = Vec<(String, VarValue)>;
    let cases: [(&str, Vars, bool); 3] = [("neither", vec![], true), ("broken", vec![flag("barrow_broken", true)], false), ("sealed", vec![flag("barrow_sealed", true)], false)];
    for (what, extra, sixth) in cases {
        let mut vars = vec![int("ally", 0), int("dread", 16)];
        vars.extend(extra);
        let carry = Carried { gold: Some(100_000), vars, squad: vec![("crossbowman", 1, 3), ("hunter", 1, 2), ("militiaman", 1, 1)], named: vec![("varga", "halberdier", 1)], avoid_armies: true };
        let mut run = barrows_run(&install, &carry, 1, &[]);
        // The hosts may beat the hero within the week: what matters is which hosts were raised.
        let _ = run.wait_days(8);
        assert!(spawned(&run, "bw_king_1"), "{what}: the first host is raised");
        assert_eq!(spawned(&run, "bw_king_6"), sixth, "{what}: the last host: {}", run.log.join("\n"));
    }
}

/// The beat gaps of a walk along the chapter's beat areas (the `hero_enters` areas of its roads): a beat is a
/// step of the walk inside one of `areas`. Returns the worst gap in cells between two beats (or the walk's start
/// and end), and the path indices of it. The areas are the unconditional ones, so every route that plays the
/// chapter meets them.
pub(crate) fn beat_gap(run: &Run, areas: &[(i32, i32, i32, i32)]) -> (usize, usize, usize) {
    let inside = |t: &Tile| areas.iter().any(|&(x, y, w, h)| t.0 >= x && t.0 < x + w && t.1 >= y && t.1 < y + h);
    let mut marks = vec![0usize];
    marks.extend(run.path.iter().enumerate().filter(|(_, t)| inside(t)).map(|(i, _)| i));
    marks.push(run.path.len() - 1);
    marks.sort_unstable();
    marks.dedup();
    marks.windows(2).map(|w| (w[1] - w[0], w[0], w[1])).max().unwrap_or((0, 0, 0))
}

/// The gates' beat areas: the roads' points and the stones' messages (`triggers/gates.toml`).
pub(crate) const GATES_AREAS: [(i32, i32, i32, i32); 16] = [
    (54, 86, 4, 4), (62, 78, 4, 4), (64, 66, 4, 4), (66, 56, 4, 4), (66, 48, 4, 4), (58, 56, 4, 4), (46, 56, 4, 4), (38, 58, 4, 4),
    (48, 60, 4, 4), (34, 62, 4, 4), (22, 54, 4, 4), (16, 46, 4, 4), (20, 34, 4, 4), (30, 26, 4, 4), (40, 20, 4, 4), (48, 14, 4, 4),
];

/// The barrows' beat areas: the road to the wall's gap, the road beyond it, the herald, the King's words
/// (`triggers/barrows.toml`), and Varga's fate on the road (her beat while she lives in the squad).
pub(crate) const BARROWS_AREAS: [(i32, i32, i32, i32); 15] = [
    (98, 178, 4, 4), (98, 164, 4, 4), (98, 150, 4, 4), (98, 134, 4, 4), (98, 128, 4, 4), (98, 116, 4, 4), (96, 88, 10, 8), (98, 100, 4, 4),
    (108, 98, 4, 4), (118, 86, 4, 4), (128, 74, 4, 4), (138, 62, 4, 4), (150, 52, 4, 4), (156, 40, 4, 4), (160, 44, 6, 6),
];

/// The main route of the gates, from the start to the north guard, by the points of the roads (each one a beat).
const GATES_ROAD: [Tile; 15] = [(56, 88), (64, 80), (64, 68), (68, 50), (60, 58), (50, 62), (46, 58), (40, 60), (36, 62), (24, 56), (18, 48), (22, 36), (32, 28), (42, 22), (46, 16)];

/// The gates' route of the points on the roads: the gap is measured on it, and it must stay at or under 10.
#[test]
fn the_gates_roads_have_a_beat_every_ten_cells() {
    let Some(install) = install() else { return };
    let mut run = gates_run(&install, &order_branch(1, 1500), 1, &[]);
    for cell in GATES_ROAD {
        to(&mut run, cell, 6).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    }
    let (gap, from, to_i) = beat_gap(&run, &GATES_AREAS);
    eprintln!("the gates' road: {} cells, {} beats, the longest gap {gap} cells (path {from}..{to_i})", run.path.len() - 1, run.beats.len());

    assert!(gap <= 10, "the longest walk between beats on the gates' road is {gap} cells: {:?}", &run.path[from..=to_i]);
}

/// The barrows' route: the road to the gap in the wall, the road beyond it to the King's step (the beats of the
/// road and the companions), and the King's fight. The gap must stay at or under 15.
const BARROWS_ROAD: [Tile; 15] = [(100, 180), (100, 166), (100, 152), (100, 136), (100, 128), (100, 116), (100, 113), (100, 100), (100, 92), (110, 100), (120, 88), (130, 76), (140, 64), (152, 54), (160, 44)];

#[test]
fn the_barrows_roads_have_a_beat_every_fifteen_cells() {
    let Some(install) = install() else { return };
    let mut run = barrows_run(&install, &order_branch(1, 1500), 1, &[]);
    for cell in BARROWS_ROAD {
        to(&mut run, cell, 8).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    }
    let (gap, from, to_i) = beat_gap(&run, &BARROWS_AREAS);
    eprintln!("the barrows' road: {} cells, {} beats, the longest gap {gap} cells (path {from}..{to_i})", run.path.len() - 1, run.beats.len());

    assert!(gap <= 15, "the longest walk between beats on the barrows' road is {gap} cells: {:?}", &run.path[from..=to_i]);
}
