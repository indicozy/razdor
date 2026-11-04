//! The whole campaign of «Пепельная Марка» played end to end (`pack::playthrough`): chapter 1 «Брод», the
//! hub «Марка», one branch (the abbey, the marsh, the Companies), the hub's revisit, the gates and the
//! barrows. Each map is entered from the one before by the campaign's own hand-over
//! (`rules::campaign::enter`), so the gold, the squad and the variables are the ones the campaign
//! carries; nothing is carried by hand. The hero hires as a player does (`hire_within_means`), and every
//! map's boundary is logged with the gold and the squad. Two things must hold on each map: no unit
//! deserts (no squad member leaves unpaid) and the gold does not sit at 0 for more than a few days.
//! Skipped without `RAZDOR_DT_DIR`.

use std::sync::Arc;

use super::open::Opened;
use super::playthrough::{install, open_map, side_quests_route, Carried, Run, Stuck, PREPARED};
use super::playthrough_camp::CAMP;
use super::playthrough_ch23::drink_for_wounded;
use super::playthrough_camp::camp_route;
use super::playthrough_ch45::{barrows_by_the_grey_throne, barrows_by_the_king, ending_of, gates_by_the_companies, gates_by_the_marsh, gates_by_the_order};
use super::triggers::VarValue;
use crate::dt::install::DtInstall;
use crate::rules::campaign::{after_victory, enter};
use crate::rules::content::Content;

/// The branch a campaign takes at the hub, and the gates' and barrows' endings it ends on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Plan {
    /// The Order: Ilm the novice's pilgrim burns the village (the inquisitor's path).
    Order,
    /// The Order's cleansed inquisitor (the shrine's cleanse, the council's Order) spares the village at the
    /// abbey's trial: the heresy, the grey bargain at the gates.
    Heretic,
    /// The marsh folk: Tess and the disguise, the Salt Gate.
    Marsh,
    /// The Free Companies: Kord hired, the camp's relief fought.
    Companies,
}

pub(crate) use super::runner::{heal_within_means, hire_within_means, hire_within_means_keeping, raise_within_means};

/// The hub's town: the walk to Ashford (its recruits and its service), the healing, then the hiring.
pub(crate) fn ashford(run: &mut Run) -> Result<(), Stuck> {
    run.walk_to((58, 44), 2)?;
    heal_within_means(run);
    raise_within_means(run);
    hire_within_means(run);
    Ok(())
}

/// One map of the campaign as the run saw it: the gold and the squad when it was entered and left, the
/// days played, the desertions, the longest run of days with no gold, and the bills and income at its end.
#[derive(Clone, Debug)]
pub(crate) struct Leg {
    pub map: String,
    pub entered: (i32, usize),
    pub left: (i32, usize),
    pub days: f64,
    pub deserted: usize,
    /// The most days in a row the gold was 0 at a day's start.
    pub zero_days: usize,
    pub wages: i32,
    pub income: i32,
    pub squad: Vec<String>,
    /// The battles the map fought, with the squad's HP at each start, and the healing.
    pub battles: Vec<String>,
    /// The story's variables that decide the campaign's roads and endings, as they stood.
    pub vars: String,
    /// The day-by-day gold, squad and income (the ledger), for the report.
    pub ledger: String,
}

/// The living squad's size and names.
fn alive(run: &Run) -> (usize, Vec<String>) {
    let names: Vec<String> = run.g.squad.iter().filter(|u| u.alive()).map(|u| run.g.content.unit(u.def).name.clone()).collect();
    (names.len(), names)
}

/// The squad as a line for a report: each unit with its hp, and the dead and the unpaid marked.
fn squad_state(run: &Run) -> String {
    let units: Vec<String> = run
        .g
        .squad
        .iter()
        .map(|u| {
            let name = run.g.content.unit(u.def).name.clone();
            let mut mark = String::new();
            if !u.alive() {
                mark.push_str(" dead");
            }
            if u.unpaid {
                mark.push_str(" unpaid");
            }
            format!("{name} {}/{}{mark}", u.hp, u.max_hp(&run.g.content))
        })
        .collect();
    format!("gold {}, spells {}, squad [{}]", run.g.gold, run.g.spells.len(), units.join(", "))
}

fn leg(map: &str, run: &Run) -> Leg {
    let (n, names) = alive(run);
    let entered = run.ledger.first().map_or((run.g.gold, n), |s| (s.gold, s.squad));
    let (mut zero_days, mut streak) = (0, 0);
    for s in &run.ledger {
        streak = if s.gold == 0 { streak + 1 } else { 0 };
        zero_days = zero_days.max(streak);
    }
    Leg {
        map: map.to_string(),
        entered,
        left: (run.g.gold, n),
        days: run.day(),
        deserted: run.deserted,
        zero_days,
        wages: run.g.daily_wages(),
        income: run.ledger.last().map_or(0, |s| s.income),
        squad: names,
        ledger: run.ledger.iter().map(|s| format!("d{:.0}:{}g/{}s/i{}", s.day, s.gold, s.squad, s.income)).collect::<Vec<_>>().join(" "),
        vars: ["ally", "ilm", "crown", "order", "companies", "marsh_trust", "varga_alive", "grey_bargain_offered", "bw_free_ok", "dread"]
            .iter()
            .map(|v| format!("{v}={}", run.var(v).map_or("-".to_string(), |x| format!("{x:?}"))))
            .collect::<Vec<_>>()
            .join(" "),
        battles: run.log.iter().filter(|l| l.contains("battle with") || l.contains("battle vs") || l.contains("healed")).map(|l| l.replace("battle with ", "").replace("battle vs ", "=> ")).collect(),
    }
}

/// The player's hiring for chapter one's route: [`hire_within_means_keeping`], the gold kept for a rite.
fn hire_for_chapter_one(run: &mut Run, keep: i32) {
    hire_within_means_keeping(run, keep);
}

/// Chapter 1 «Брод» on the side-quest route (`side_quests_route`): the tower and its recruits, the barrow
/// sealed with a rite (the break is a fight that leaves the squad too hurt for the raids), the deserters
/// handed to the Crown, the refugees taken to Reedmere, the raids met on days 4, 8 and 12, the deeds that
/// raise the host, the host beaten.
pub(crate) fn chapter_one(install: &DtInstall, seed: u32) -> Result<Run, Stuck> {
    let (brod, content) = open_map(install, "brod");
    let mut run = Run::new(&brod, content, seed, PREPARED)?;
    side_quests_route(&mut run, "leave", hire_for_chapter_one)?;
    if !run.victory() {
        return Err(Stuck::Broken("chapter one was not won".into()));
    }
    Ok(run)
}

/// The hub's answers on the first visit, by plan: the council's ally, the Order's shrine, and the
/// Companies' captain and Kord's offer (the hire waits until the purse holds it).
fn hub_choices(plan: Plan) -> Vec<(&'static str, &'static str)> {
    let council = match plan {
        Plan::Order | Plan::Heretic => "order",
        Plan::Marsh => "marsh",
        Plan::Companies => "later",
    };
    let mut c = vec![("marka_arrival", "take"), ("shrine_inside", "cleanse"), ("shrine_offer", "accept"), ("marsh_offer", "accept"), ("council", council)];
    if plan == Plan::Companies {
        c.extend([("captain", "serve"), ("kord_offer", "hire")]);
    }
    c
}

/// The hub's first visit: the quests on the way to the council, then the council's answer (for the
/// Companies, the answer when Kord's company is hired).
fn hub_first_visit(run: &mut Run, plan: Plan) -> Result<(), Stuck> {
    drink_for_wounded(run);
    ashford(run)?;
    run.walk_to((15, 69), 10)?;
    run.walk_to((30, 84), 12)?;
    run.walk_to((28, 36), 10)?;
    if plan != Plan::Companies {
        return run.wait_until_done(40);
    }
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
    Ok(())
}

/// The branch's answers on its arrival.
fn branch_choices(plan: Plan) -> Vec<(&'static str, &'static str)> {
    match plan {
        Plan::Heretic => vec![("abbey_trial", "spare"), ("abbey_heresy", "accept")],
        Plan::Companies => CAMP.to_vec(),
        _ => Vec::new(),
    }
}

/// Plays the branch to its end (the chapter's victory). The abbey's siege, the salt road, the camp's relief.
fn branch_play(plan: Plan, run: &mut Run) -> Result<(), Stuck> {
    match plan {
        Plan::Order => {
            run.walk_to((50, 50), 6)?;
            run.walk_to((50, 40), 6)?;
            run.wait_until_done(40)
        }
        Plan::Heretic => {
            run.walk_to((50, 50), 6)?;
            run.walk_to((16, 60), 10)?;
            run.walk_to((50, 40), 12)?;
            run.wait_until_done(40)
        }
        Plan::Marsh => {
            run.walk_to((22, 60), 20)?;
            for _ in 0..6 {
                if run.done() {
                    break;
                }
                run.walk_to((70, 40), 4)?;
            }
            Ok(())
        }
        Plan::Companies => {
            camp_route(run)?;
            Ok(())
        }
    }
}

/// The gates' answers, by plan: the Marsh's Varga for the south gate, the heretic listens to the wraith.
fn gates_choices(plan: Plan) -> Vec<(&'static str, &'static str)> {
    match plan {
        Plan::Marsh => vec![("gt_closed_s", "varga")],
        Plan::Heretic => vec![("gt_messenger", "listen")],
        _ => Vec::new(),
    }
}

fn gates_play(plan: Plan, run: &mut Run) -> Result<(), Stuck> {
    match plan {
        Plan::Order | Plan::Heretic => gates_by_the_order(run),
        Plan::Marsh => {
            // Varga's halberd opens the south gate while she lives; else the Crown's toll of 100 gold.
            let varga = if run.var("varga_alive") == Some(VarValue::Bool(true)) { "varga" } else { "pay" };
            run.choices.insert("gt_closed_s".into(), varga.into());
            gates_by_the_marsh(run)
        }
        Plan::Companies => gates_by_the_companies(run),
    }
}

/// The barrows: the chapel's rite on the way (the army healed for the gold), then the heretic kneels at
/// the grey throne, the others walk to the King.
fn barrows_play(plan: Plan, run: &mut Run) -> Result<(), Stuck> {
    run.walk_to((48, 170), 3)?;
    heal_within_means(run);
    if plan == Plan::Heretic {
        barrows_by_the_grey_throne(run)
    } else {
        barrows_by_the_king(run)
    }
}

/// The campaign's whole run for `seed` on `plan`, every map entered by the hand-over. The legs are
/// pushed as each map is left (so a failure keeps the legs before it). Returns the ending reached.
pub(crate) fn play(install: &DtInstall, seed: u32, plan: Plan, legs: &mut Vec<Leg>) -> Result<Option<String>, String> {
    let fail = |what: &str, e: Stuck| format!("{what}: {e}");
    let mut brod_run = chapter_one(install, seed).map_err(|e| fail("brod", e))?;
    let (brod, _) = open_map(install, "brod");
    legs.push(leg("brod", &brod_run));
    let first = after_victory(&mut brod_run.g, &brod.routes).ok_or("chapter one has no next map")?;
    if first != "marka" {
        return Err(format!("chapter one goes to {first}, not the hub"));
    }

    // The hub, first visit.
    let mut hub = hand(install, &brod_run, &brod, "marka", &hub_choices(plan)).map_err(|e| fail("marka", e))?;
    hub.g.avoid_armies = true;
    hub_first_visit(&mut hub, plan).map_err(|e| fail("marka (first visit)", e))?;
    let (marka, _) = open_map(install, "marka");
    legs.push(leg("marka", &hub));

    // The branch the council sent the hero to.
    let branch = after_victory(&mut hub.g, &marka.routes).ok_or("the hub sends the hero nowhere")?;
    let mut br = hand(install, &hub, &marka, &branch, &branch_choices(plan)).map_err(|e| fail(&branch, e))?;
    br.g.avoid_armies = true;
    drink_for_wounded(&mut br);
    branch_play(plan, &mut br).map_err(|e| fail(&branch, e))?;
    legs.push(leg(&branch, &br));
    if !br.victory() {
        return Err(format!("the branch {branch} was not won: {}", br.log.join(" | ")));
    }
    let (branch_open, _) = open_map(install, &branch);
    let back = after_victory(&mut br.g, &branch_open.routes).ok_or("the branch has no way back")?;
    if back != "marka" {
        return Err(format!("the branch {branch} goes to {back}, not the hub"));
    }

    // The hub's revisit: the branch's chapter is done, the revisit ends the hub.
    let mut hub2 = hand(install, &br, &branch_open, "marka", &[]).map_err(|e| fail("marka (revisit)", e))?;
    drink_for_wounded(&mut hub2);
    ashford(&mut hub2).map_err(|e| fail("marka (revisit)", e))?;
    hub2.wait_until_done(5).map_err(|e| fail("marka (revisit)", e))?;
    if !hub2.done() {
        return Err(format!("the hub's revisit did not end: {}", hub2.log.join(" | ")));
    }
    legs.push(leg("marka (revisit)", &hub2));
    let gates_to = after_victory(&mut hub2.g, &marka.routes).ok_or("the hub has no way on")?;
    if gates_to != "gates" {
        return Err(format!("the hub goes to {gates_to} after the branch, not the gates"));
    }

    // The gates.
    let mut gates = hand(install, &hub2, &marka, "gates", &gates_choices(plan)).map_err(|e| fail("gates", e))?;
    gates.g.avoid_armies = true;
    drink_for_wounded(&mut gates);
    let entered_gates = squad_state(&gates);
    gates_play(plan, &mut gates).map_err(|e| format!("gates: {e}\n  entered with: {entered_gates}\n  log: {}", gates.log.join(" | ")))?;
    if !gates.victory() {
        return Err(format!("the gates were not won: {}", gates.log.join(" | ")));
    }
    legs.push(leg("gates", &gates));
    let (gates_open, _) = open_map(install, "gates");
    let next = after_victory(&mut gates.g, &gates_open.routes).ok_or("the gates have no way on")?;
    if next != "barrows" {
        return Err(format!("the gates go to {next}, not the barrows"));
    }

    // The barrows: the finale.
    let choices: &[(&str, &str)] = match plan {
        Plan::Heretic => &[("bw_grey_throne", "kneel")],
        Plan::Order => &[("bw_council", "iron")],
        Plan::Marsh | Plan::Companies => &[("bw_council", "free")],
    };
    let mut bw = hand(install, &gates, &gates_open, "barrows", choices).map_err(|e| fail("barrows", e))?;
    bw.g.avoid_armies = true;
    let played = barrows_play(plan, &mut bw);
    legs.push(leg("barrows", &bw));
    played.map_err(|e| fail("barrows", e))?;
    Ok(ending_of(&bw))
}

/// The campaign's hand-over from the run `from` (on map `from_open`) to map `to`: its game entered as the
/// campaign does, and the run on it with the choices.
fn hand(install: &DtInstall, from: &Run, from_open: &Opened, to: &str, choices: &[(&str, &str)]) -> Result<Run, Stuck> {
    let (opened, content): (Opened, Arc<Content>) = open_map(install, to);
    let g = enter(&from.g, from_open, &opened, None, content).map_err(Stuck::Broken)?;
    Run::resume(g, choices)
}

/// Plays `plan` for `seed` and returns the legs and the ending (or the failure with the legs before it).
pub(crate) fn campaign(install: &DtInstall, seed: u32, plan: Plan) -> (Vec<Leg>, Result<Option<String>, String>) {
    let mut legs = Vec::new();
    let ending = play(install, seed, plan, &mut legs);
    (legs, ending)
}

/// The legs as a report: one line per map, the gold and the squad at its boundaries.
pub(crate) fn report(plan: Plan, seed: u32, legs: &[Leg], ending: &Result<Option<String>, String>) -> String {
    let mut out = format!("{plan:?} seed {seed}:\n");
    for l in legs {
        out.push_str(&format!(
            "  {:<16} enter gold {:>5} squad {:>2} | leave gold {:>5} squad {:>2} | days {:>5.1} | deserted {} zero-gold days {} | wages {:>4}/day income {:>4}/day | {}\n",
            l.map,
            l.entered.0,
            l.entered.1,
            l.left.0,
            l.left.1,
            l.days,
            l.deserted,
            l.zero_days,
            l.wages,
            l.income,
            l.squad.join(", ")
        ));
        out.push_str(&format!("      vars: {}\n", l.vars));
        if l.map == "camp" {
            out.push_str(&format!("      ledger: {}\n", l.ledger));
        }
        if !l.battles.is_empty() {
            out.push_str(&format!("      battles: {}\n", l.battles.join("; ")));
        }
    }
    match ending {
        Ok(Some(e)) => out.push_str(&format!("  ending: {e}\n")),
        Ok(None) => out.push_str("  ending: none\n"),
        Err(e) => out.push_str(&format!("  FAILED: {e}\n")),
    }
    out
}

/// The campaign's checks on its legs: no desertion, the gold not at 0 for more than a few days.
pub(crate) fn problems(legs: &[Leg]) -> Vec<String> {
    let mut out = Vec::new();
    for l in legs {
        if l.deserted > 0 {
            out.push(format!("{}: {} unit(s) deserted", l.map, l.deserted));
        }
        if l.zero_days > 3 {
            out.push(format!("{}: the gold sat at 0 for {} days", l.map, l.zero_days));
        }
    }
    out
}

/// A `player` fort pays its income at each noon (the stock grows back to `gold_max` at the midnight):
/// the barrows' Ford Fort, 20 a day, with no squad to pay, adds 20 a day to a purse at 0. Before the
/// compile gave the stock a maximum, a fort paid its first noon and nothing after it.
#[test]
fn a_players_fort_pays_its_income_at_each_noon() {
    let Some(install) = install() else { return };
    let (opened, content) = open_map(&install, "barrows");
    let carry = Carried { gold: Some(0), vars: Vec::new(), squad: Vec::new(), named: Vec::new(), avoid_armies: true };
    let mut run = Run::carried(&opened, content, 1, &[], &carry).unwrap_or_else(|e| panic!("{e}"));
    run.g.squad.truncate(1);
    run.wait_days(4).unwrap_or_else(|e| panic!("{e}\n{}", run.log.join("\n")));
    let paid = run.ledger.last().map_or(0, |s| s.income);
    assert_eq!(paid, 20, "the Ford Fort is the hero's and gives 20 a day");
    assert!(run.g.gold >= 60, "four noons of 20: the purse holds them, {} gold", run.g.gold);
}

#[test]
fn the_campaign_plays_the_order_to_its_ending() {
    let Some(install) = install() else { return };
    let (legs, ending) = campaign(&install, 1, Plan::Order);
    eprintln!("{}", report(Plan::Order, 1, &legs, &ending));
    assert!(ending.is_ok(), "{}", report(Plan::Order, 1, &legs, &ending));
    assert_eq!(ending.clone().unwrap().as_deref(), Some("iron"), "{}", report(Plan::Order, 1, &legs, &ending));
    assert!(problems(&legs).is_empty(), "{:?}", problems(&legs));
}

#[test]
fn the_campaign_plays_the_heretic_to_the_grey_bargain() {
    let Some(install) = install() else { return };
    let (legs, ending) = campaign(&install, 1, Plan::Heretic);
    eprintln!("{}", report(Plan::Heretic, 1, &legs, &ending));
    assert_eq!(ending.clone().ok().flatten().as_deref(), Some("grey"), "{}", report(Plan::Heretic, 1, &legs, &ending));
    assert!(problems(&legs).is_empty(), "{:?}", problems(&legs));
}

#[test]
fn the_campaign_plays_the_marsh_to_the_free_march() {
    let Some(install) = install() else { return };
    let (legs, ending) = campaign(&install, 1, Plan::Marsh);
    eprintln!("{}", report(Plan::Marsh, 1, &legs, &ending));
    assert_eq!(ending.clone().ok().flatten().as_deref(), Some("free"), "{}", report(Plan::Marsh, 1, &legs, &ending));
    assert!(problems(&legs).is_empty(), "{:?}", problems(&legs));
}

#[test]
fn the_campaign_plays_the_companies_to_the_free_march() {
    let Some(install) = install() else { return };
    let (legs, ending) = campaign(&install, 1, Plan::Companies);
    eprintln!("{}", report(Plan::Companies, 1, &legs, &ending));
    assert_eq!(ending.clone().ok().flatten().as_deref(), Some("free"), "{}", report(Plan::Companies, 1, &legs, &ending));
    assert!(problems(&legs).is_empty(), "{:?}", problems(&legs));
}

/// Varga, the halberdier who joins at the ruins, is alive at the end of chapter one in at least 70% of
/// fifteen seeds on the campaign's own route (the hired recruits, the host met). Measured at 15 of 15 when
/// it was written: the host's fight does not reach her (no blow lands on her in the sweep).
#[test]
fn varga_survives_chapter_one_over_fifteen_seeds() {
    let Some(install) = install() else { return };
    let mut alive = 0;
    for seed in 1..=15u32 {
        if let Ok(run) = chapter_one(&install, seed) {
            if run.g.squad.iter().any(|u| u.alive() && u.named != 0) {
                alive += 1;
            }
        }
    }
    assert!(alive * 100 >= 70 * 15, "Varga lives in {alive}/15 seeds");
}

