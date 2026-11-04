//! Health: whether a package's map can be played as its story says, from play tests and from the
//! design traps of the editor guide (`docs/reference/rzpack-editor-guide.md` §8). Findings, each
//! with where it is, and a 0 to 100 summary. Health warns; it never blocks saving or anything else.
//!
//! The checks:
//! - reachability: every story place (a trigger's cell or area, a building, an army, a travel point)
//!   is reached on foot from the hero's start (`pack::reach`, the engine's walk);
//! - the scripted fights (an army a trigger needs beaten or starts a battle with, the garrison of a
//!   building a trigger needs captured) are fought once by the hero's starting squad on the battle
//!   tester's engine (`testers`): a lost fight is a warning, a won one that the enemy's strength
//!   brings near the squad's (above 90%) is a note. The battle has no random draws, so one run is
//!   the fight; the outcomes are cached by the content of the map and the units and items it uses;
//! - the gold budget of the map: the hero's gold, the income of the buildings the hero reaches over
//!   [`EXPECTED_DAYS`], and the gold the triggers give, against the wages of the starting squad over
//!   the same days;
//! - dead ends: a once-only offer gated on gold, a «later» answer whose dialog never comes back, a
//!   named character required after a `remove_unit` or who may die in a fight of the map;
//! - the design traps of the guide's §8 that the files show (one check each, named after the trap).
//!
//! The summary is Razdor's own measure, not DT's: 100 less 10 for each warning and 2 for each note,
//! at least 0. It says how much there is to look at, not how good the map is.

use std::collections::{BTreeMap, BTreeSet};

use crate::dt::install::DtInstall;
use crate::i18n::{n_, tr};
use crate::pack::compile;
use crate::pack::diag::Report;
use crate::pack::files::Files;
use crate::pack::load::{Located, Package};
use crate::pack::map::{ArmySpec, BuildingSpec, MapFile, Victory};
use crate::pack::reach::{self, Named};
use crate::pack::terrain::{self, Palette};
use crate::pack::triggers::{Action, Cond, Event, GoalSpec, Place, Target as GoalTarget, Trigger};
use crate::rules::battle::Team;
use crate::studio::characters;
use crate::studio::richness::Names;
use crate::studio::scores::Target;
use crate::studio::testers::{self, Matchup, Run, Source};

/// The days a map's budget is worked out over (Razdor's measure: a map has no day count of its own).
pub const EXPECTED_DAYS: i64 = 30;

/// A fight is risky when the enemy's strength is above this share of the squad's.
pub const RISKY_SHARE: f64 = 0.9;

/// The models that hunt the hero from the start (the guide's §8 examples: bandits, ghosts, …).
pub const HUNTING_MODELS: [&str; 4] = ["bandits", "ghosts", "necromancer", "zombies"];

/// How much a finding takes from the summary.
const WARNING_COST: i64 = 10;
const NOTE_COST: i64 = 2;

/// What a finding says about the map.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Warning,
    Info,
}

/// One finding: its check (`kind`, the check's name), its severity, what it says (a catalogue key),
/// the name it is about, the numbers, and where it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub kind: &'static str,
    pub severity: Severity,
    pub note: &'static str,
    pub name: String,
    pub figures: String,
    pub targets: Vec<Target>,
}

impl Finding {
    fn new(kind: &'static str, severity: Severity, note: &'static str, name: impl Into<String>, figures: impl Into<String>, targets: Vec<Target>) -> Finding {
        Finding { kind, severity, note, name: name.into(), figures: figures.into(), targets }
    }
}

/// The findings of a map and their summary.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Health {
    pub findings: Vec<Finding>,
    pub summary: i64,
}

/// The outcome of one scripted fight by the squad, one run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Won,
    Lost,
    NoEnd,
}

/// A fight's result as cached: the outcome, whether it is risky, and the squad's dead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FightRes {
    pub outcome: Outcome,
    pub risky: bool,
    pub squad_dead: u32,
}

/// The fights of the maps, kept while their content does not change: per map, the hash of the
/// content it was fought with and the results by fight.
#[derive(Default)]
pub struct Cache {
    maps: BTreeMap<String, (u64, BTreeMap<String, FightRes>)>,
}

/// A `hero_enters` area of a trigger, with the trigger.
type HeroArea<'a> = (&'a Located<Trigger>, (i32, i32, i32, i32));

/// A scripted fight: the army or the building garrison it is with, and where it is scripted.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Fight {
    key: String,
    side: Side,
    at: Target,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Side {
    Army(String),
    Garrison(String),
}

/// A FNV-1a hash of the files under `prefixes` (path and bytes, in path order).
fn content_hash(files: &Files, prefixes: &[String]) -> u64 {
    let mut paths: BTreeSet<&str> = BTreeSet::new();
    for p in files.paths() {
        if prefixes.iter().any(|pre| p.starts_with(pre.as_str())) {
            paths.insert(p);
        }
    }
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for p in paths {
        feed(p.as_bytes());
        feed(&[0]);
        feed(files.get(p).unwrap_or_default());
        feed(&[0]);
    }
    h
}

/// Whether a trigger applies to map `map` (a trigger with no map applies to every map).
fn applies(t: &Trigger, map: &str) -> bool {
    t.map.as_deref().is_none_or(|m| m == map)
}

/// The armies a trigger names as fought, beaten or sent into (for the traps and the fights).
fn fought_armies(t: &Trigger) -> Vec<String> {
    let mut out = Vec::new();
    if let Event::ArmyDefeated(a) = &t.when {
        out.push(a.clone());
    }
    for a in &t.then {
        if let Action::StartBattle { army, .. } = a {
            out.push(army.clone());
        }
    }
    out
}

/// The map's file as the terrain and the story of the walk read it.
fn walk_of(pkg: &Package, map: &str, spec: &MapFile) -> Option<(reach::Story, crate::rules::map::TileMap, Terrain)> {
    let palette = Palette::with_overrides(&spec.terrain_palette).ok()?;
    let png = pkg.files.get(&format!("maps/{map}/terrain.png"))?;
    let t = terrain::decode(png, &palette).ok()?;
    let story = reach::story(&pkg.files, map).ok()?;
    let foot = reach::foot_map(&pkg.files, map, t.width as i32, t.height as i32, &t.cells).ok()?;
    Some((story, foot, Terrain { width: t.width as i32 }))
}

/// The terrain's width (what the walk needs of it).
struct Terrain {
    #[allow(dead_code)]
    width: i32,
}

/// The cell a story site names, as a target (an area's first cell).
fn site_cell(named: &Named) -> (i64, i64) {
    match *named {
        Named::Cell((x, y)) => (i64::from(x), i64::from(y)),
        Named::Area(x, y, _, _) => (i64::from(x), i64::from(y)),
    }
}

/// Reachability: each story place the hero's start cannot reach on foot.
fn reachability(pkg: &Package, map: &str, spec: &MapFile, findings: &mut Vec<Finding>) -> BTreeSet<String> {
    let mut unreached_buildings = BTreeSet::new();
    let Some((story, foot, _)) = walk_of(pkg, map, spec) else { return unreached_buildings };
    for site in reach::unreached(&foot, story.hero, &story) {
        let from = if site.from.is_some() { tr("from its army's cell") } else { tr("from the hero's start") };
        let (x, y) = site_cell(&site.named);
        findings.push(Finding::new("unreached", Severity::Warning, n_("Cannot be reached on foot from the hero's start"), &site.what, format!("{from}; {}", site.file), vec![Target::Cell(x, y)]));
        if let Some(id) = site.what.strip_prefix("building `").and_then(|s| s.strip_suffix('`')) {
            unreached_buildings.insert(id.to_string());
        }
    }
    unreached_buildings
}

/// The dead ends of the story (the guide's §8 «once-only offer» and «later» traps; a «later» answer
/// whose dialog never comes back is a softlock).
fn dead_ends(pkg: &Package, map: &str, findings: &mut Vec<Finding>) {
    let triggers: Vec<&Located<Trigger>> = pkg.triggers.iter().filter(|t| applies(&t.value, map)).collect();
    // A once-only trigger gated on gold that opens a dialog: short of gold when it fires, the offer is gone.
    for t in &triggers {
        let v = &t.value;
        let gated = v.once && v.cond.iter().any(|c| matches!(c, Cond::Gold { .. }));
        let opens = v.then.iter().any(|a| matches!(a, Action::Dialog(_)));
        if gated && opens {
            findings.push(Finding::new("once_offer_gold", Severity::Warning, n_("A once-only offer gated on gold: short of gold when it fires, the offer is lost"), &v.id, tr("Repeat the trigger (once = false) while the offer is open"), vec![Target::Line { file: t.file.clone(), line: t.line }]));
        }
    }
    // A «later» answer whose dialog is never offered again: a dialog comes back when a trigger that is
    // not once opens it, or another dialog's answer (other than «later») does.
    let mut reopened: BTreeSet<String> = BTreeSet::new();
    for t in &triggers {
        if !t.value.once {
            for a in &t.value.then {
                if let Action::Dialog(d) = a {
                    reopened.insert(d.clone());
                }
            }
        }
    }
    for d in &pkg.dialogs {
        for c in &d.value.choices {
            if c.id == "later" {
                continue;
            }
            for a in &c.then {
                if let Action::Dialog(n) = a {
                    reopened.insert(n.clone());
                }
            }
        }
    }
    for d in &pkg.dialogs {
        let opened_here = triggers.iter().any(|t| t.value.then.iter().any(|a| matches!(a, Action::Dialog(x) if *x == d.value.id)));
        if opened_here && d.value.choices.iter().any(|c| c.id == "later") && !reopened.contains(&d.value.id) {
            findings.push(Finding::new("later_never_returns", Severity::Warning, n_("A «later» answer whose dialog never comes back: the map has no way forward from it"), &d.value.id, tr("Re-offer the dialog each week until it is answered (a trigger with once = false)"), vec![Target::Line { file: d.file.clone(), line: d.line }]));
        }
    }
}

/// Whether a character's site is on map `map` (a site of a trigger with no map is on every map).
fn on_map(site: &characters::Site, map: &str) -> bool {
    site.map.as_deref().is_none_or(|m| m == map)
}

/// The named characters, as this map uses them: one removed here (`remove_unit`) and required
/// anywhere may be gone when the scene needs him.
fn named(pkg: &Package, map: &str, findings: &mut Vec<Finding>) {
    for c in characters::characters(pkg) {
        let removed: Vec<&characters::Site> = c.removes.iter().filter(|s| on_map(s, map)).collect();
        if !c.needs.is_empty() && !removed.is_empty() {
            let at: Vec<Target> = removed.iter().map(|s| Target::Line { file: s.file.clone(), line: s.line }).collect();
            findings.push(Finding::new("named_removed", Severity::Warning, n_("A named character is removed and also required: he may be gone when a scene needs him"), &c.id, format!("{} {}, {} {}", removed.len(), tr("remove"), c.needs.len(), tr("need")), at));
        }
    }
}

/// The traps of the guide's §8 that the files show. Each is one check, named after the trap.
fn traps(pkg: &Package, map: &str, spec: &MapFile, unreached_buildings: &BTreeSet<String>, findings: &mut Vec<Finding>) {
    let triggers: Vec<&Located<Trigger>> = pkg.triggers.iter().filter(|t| applies(&t.value, map)).collect();
    let army = |id: &str| spec.army.iter().map(|a| a.get_ref()).find(|a| a.id == id);
    let building = |id: &str| spec.building.iter().map(|b| b.get_ref()).find(|b| b.id == id);
    let line = |t: &Located<Trigger>| Target::Line { file: t.file.clone(), line: t.line };
    // An enemy army that hunts, named by a story trigger, with no goal at game start.
    let goal_at_start: BTreeSet<String> = triggers
        .iter()
        .filter(|t| matches!(t.value.when, Event::GameStart))
        .flat_map(|t| t.value.then.iter().filter_map(|a| if let Action::ArmyGoal { army, .. } = a { Some(army.clone()) } else { None }))
        .collect();
    for a in &spec.army {
        let a: &ArmySpec = a.get_ref();
        let named_by_story = triggers.iter().any(|t| !matches!(t.value.when, Event::GameStart) && fought_armies(&t.value).contains(&a.id));
        if HUNTING_MODELS.contains(&a.model.as_str()) && named_by_story && !goal_at_start.contains(&a.id) {
            findings.push(Finding::new("hunting_no_goal", Severity::Warning, n_("A hunting army named by a story trigger has no goal at game start: it hunts the hero from day one"), &a.id, format!("{} {}", tr("model"), a.model), vec![Target::Army(a.id.clone())]));
        }
        // A neutral army (the neighbours' faction) on the map: a step onto it is a fight.
        if a.faction == "neighbour" {
            findings.push(Finding::new("neutral_army", Severity::Info, n_("A neutral army: a step onto it is a fight, even with «let pass»"), &a.id, String::new(), vec![Target::Army(a.id.clone())]));
        }
    }
    for t in &triggers {
        let v = &t.value;
        for act in &v.then {
            match act {
                // An escort stops the hero's walk on each step unless the route goes around it.
                Action::ArmyGoal { army: id, goal: GoalSpec::Follow(GoalTarget::Hero) } => {
                    findings.push(Finding::new("escort_blocks", Severity::Info, n_("An escort stops the hero's walk on each step unless the route goes around it: say so in the dialog"), id, "", vec![Target::Army(id.clone())]));
                }
                // An army sent to attack a standing hero waits beside him: the fight starts on his next step.
                Action::ArmyGoal { army: id, goal: GoalSpec::Attack(GoalTarget::Hero) } => {
                    findings.push(Finding::new("attack_hero_waits", Severity::Info, n_("An attack on the hero waits beside him: the fight starts on his next step, or use start_battle"), id, "", vec![Target::Army(id.clone())]));
                }
                _ => {}
            }
        }
        // A victory by beating an army that shelters in a town or a tavern: a sheltered army is never engaged.
        let beaten = match &v.when {
            Event::ArmyDefeated(a) => Some(a.clone()),
            _ => None,
        };
        if let Some(id) = beaten {
            let goal = v.then.iter().find_map(|a| match a {
                Action::ArmyGoal { army, goal: GoalSpec::GoToBuilding(b) } if *army == id => Some(b.clone()),
                _ => None,
            });
            if let Some(b) = goal {
                if building(&b).is_some_and(|x| matches!(x.kind.as_str(), "town" | "tavern")) {
                    findings.push(Finding::new("sheltered_victory", Severity::Warning, n_("A victory by beating an army that shelters in a town or tavern: a sheltered army is never engaged"), &id, format!("{} {b}", tr("building")), vec![line(t), Target::Building(b)]));
                }
            }
        }
        // A capture of a town (towns are never captured on a step), or of an altar or a church (never a contact target).
        if let Event::BuildingCaptured(b) = &v.when {
            match building(b).map(|x| x.kind.as_str()) {
                Some("town") => findings.push(Finding::new("capture_town", Severity::Warning, n_("A town is not captured on a step: use a castle or a fort for a capture goal"), b, tr("town"), vec![line(t), Target::Building(b.clone())])),
                Some(k @ ("altar" | "church")) => findings.push(Finding::new("altar_target", Severity::Warning, n_("An altar or a church is never a contact target: it can't be fought or captured"), b, if k == "altar" { tr("altar") } else { tr("church") }, vec![line(t), Target::Building(b.clone())])),
                _ => {}
            }
        }
        // A hero_enters area that does not cover the army it meets: the meeting comes before the area fires.
        if let Event::HeroEnters(Place::Area(x, y, w, h)) = &v.when {
            for id in fought_armies(v) {
                if let Some(a) = army(&id) {
                    let (ax, ay) = (a.cell[0] as i32, a.cell[1] as i32);
                    if !(ax >= *x && ax < x + w && ay >= *y && ay < y + h) {
                        findings.push(Finding::new("hero_enters_misses_army", Severity::Warning, n_("A hero_enters area that does not cover its army: the army meets the hero before the area fires"), &id, format!("{} {x},{y} {w}×{h}", tr("area")), vec![Target::Army(id.clone()), line(t)]));
                    }
                }
            }
        }
    }
    // Two hero_enters areas that overlap: both dialogs open at once, and a meeting can be skipped.
    let areas: Vec<HeroArea> = triggers.iter().filter_map(|t| if let Event::HeroEnters(Place::Area(x, y, w, h)) = t.value.when { Some((*t, (x, y, w, h))) } else { None }).collect();
    for (i, (ta, a)) in areas.iter().enumerate() {
        for (tb, b) in areas.iter().skip(i + 1) {
            let x0 = a.0.max(b.0);
            let y0 = a.1.max(b.1);
            let x1 = (a.0 + a.2).min(b.0 + b.2);
            let y1 = (a.1 + a.3).min(b.1 + b.3);
            if x0 < x1 && y0 < y1 {
                findings.push(Finding::new("hero_enters_overlap", Severity::Warning, n_("Two hero_enters areas overlap: both dialogs open at once"), format!("{} / {}", ta.value.id, tb.value.id), format!("{} {x0},{y0}", tr("overlap")), vec![Target::Cell(i64::from(x0), i64::from(y0)), line(ta), line(tb)]));
            }
        }
    }
    // Victory by capturing a building the victory names: a town is not captured on a step (above).
    if let Victory::Capture { building: b, .. } = &spec.victory.get_ref() {
        if building(b).is_some_and(|x| x.kind == "town") {
            findings.push(Finding::new("capture_town", Severity::Warning, n_("A town is not captured on a step: use a castle or a fort for a capture goal"), b, tr("victory"), vec![Target::Building(b.clone())]));
        }
    }
    // Income from a building the hero cannot reach is never held.
    for b in &spec.building {
        let b: &BuildingSpec = b.get_ref();
        if b.income > 0 && unreached_buildings.contains(&b.id) {
            findings.push(Finding::new("income_not_held", Severity::Warning, n_("Income from a building the hero cannot reach: no gold arrives"), &b.id, format!("{} {}", tr("income"), b.income), vec![Target::Building(b.id.clone())]));
        }
    }
}

/// The gold the triggers give, and the hero's gold, the income of the buildings the hero reaches, and
/// the wages of the starting squad over the expected days (see the budget finding).
fn budget(map: &str, pkg: &Package, spec: &MapFile, unreached_buildings: &BTreeSet<String>, wages_per_day: i64, findings: &mut Vec<Finding>) {
    let rewards: i64 = pkg
        .triggers
        .iter()
        .filter(|t| applies(&t.value, map))
        .flat_map(|t| t.value.then.iter())
        .filter_map(|a| if let Action::GiveGold(n) = a { Some((*n).max(0)) } else { None })
        .sum();
    let income: i64 = spec.building.iter().map(|b| b.get_ref()).filter(|b| !unreached_buildings.contains(&b.id)).map(|b| i64::from(b.income)).sum();
    let gold = i64::from(spec.hero.gold);
    let available = gold + income * EXPECTED_DAYS + rewards;
    let wages = wages_per_day * EXPECTED_DAYS;
    if wages > available {
        findings.push(Finding::new(
            "gold_budget",
            Severity::Warning,
            n_("The starting squad's wages over the expected days are more than the gold the map gives: the squad deserts"),
            map,
            format!("{} {wages}, {} {available} ({} {gold}, {} {income} / {}, {} {rewards})", tr("wages"), tr("gold"), tr("hero"), tr("income"), tr("day"), tr("rewards")),
            vec![Target::Hero],
        ));
    }
}

/// The scripted fights: the armies a trigger fights and the garrisons of the buildings it needs
/// captured, each once, with where it is scripted.
fn scripted(map: &str, pkg: &Package) -> Vec<Fight> {
    let mut out: Vec<Fight> = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut push = |key: String, side: Side, at: Target| {
        if seen.insert(key.clone()) {
            out.push(Fight { key, side, at });
        }
    };
    for t in pkg.triggers.iter().filter(|t| applies(&t.value, map)) {
        let at = Target::Line { file: t.file.clone(), line: t.line };
        for id in fought_armies(&t.value) {
            push(format!("army:{id}"), Side::Army(id.clone()), at.clone());
        }
        if let Event::BuildingCaptured(b) = &t.value.when {
            push(format!("garrison:{b}"), Side::Garrison(b.clone()), at.clone());
        }
    }
    out
}

/// Fights the scripted fights on the battle tester's engine (one run each), reusing the cache when
/// the map's content is unchanged.
fn fights(map: &str, pkg: &Package, install: &DtInstall, lang: &str, cache: &mut Cache, names: &Names, fights: &[Fight]) -> Result<BTreeMap<String, FightRes>, String> {
    let hash = content_hash(&pkg.files, &[format!("maps/{map}/"), "units/".into(), "items/".into()]);
    if let Some((h, res)) = cache.maps.get(map) {
        if *h == hash && fights.iter().all(|f| res.contains_key(&f.key)) {
            return Ok(res.clone());
        }
    }
    let mg = testers::open_map(pkg, install, map).map_err(|r| r.to_text().lines().next().unwrap_or_default().to_string())?;
    let _ = lang;
    let mut out = BTreeMap::new();
    for f in fights {
        let other = match &f.side {
            Side::Army(id) => names.armies.iter().position(|n| n == id).map(Source::Army),
            Side::Garrison(id) => names.buildings.iter().position(|n| n == id).map(Source::Garrison),
        };
        let Some(other) = other else { continue };
        let m = Matchup { a: Source::Squad, b: other };
        let Ok(battle) = testers::build(&mg.content, Some(&mg.game), &m, false) else { continue };
        let squad = battle.strength_now(Team::Player) as f64;
        let enemy = battle.strength_now(Team::Enemy) as f64;
        let run: Run = testers::fight(battle, 1);
        let outcome = match run.winner {
            Some(Team::Player) => Outcome::Won,
            Some(Team::Enemy) => Outcome::Lost,
            None => Outcome::NoEnd,
        };
        let risky = outcome == Outcome::Won && squad > 0.0 && enemy > RISKY_SHARE * squad;
        out.insert(f.key.clone(), FightRes { outcome, risky, squad_dead: run.a.units });
    }
    cache.maps.insert(map.to_string(), (hash, out.clone()));
    Ok(out)
}

/// The fight findings: a lost fight, a fight that does not end, and a won fight near the squad's strength.
fn fight_findings(fights: &[Fight], res: &BTreeMap<String, FightRes>, findings: &mut Vec<Finding>) {
    for f in fights {
        let Some(r) = res.get(&f.key) else { continue };
        let name = match &f.side {
            Side::Army(id) => format!("{} {id}", tr("army")),
            Side::Garrison(id) => format!("{} {id}", tr("garrison")),
        };
        match r.outcome {
            Outcome::Lost => findings.push(Finding::new("fight_lost", Severity::Warning, n_("The starting squad loses this scripted fight: it ends the game"), name, format!("{} {}", r.squad_dead, tr("dead")), vec![f.at.clone()])),
            Outcome::NoEnd => findings.push(Finding::new("fight_no_end", Severity::Warning, n_("The scripted fight does not end within the battle's limit"), name, String::new(), vec![f.at.clone()])),
            Outcome::Won if r.risky => findings.push(Finding::new("fight_risky", Severity::Info, n_("The starting squad wins, but the enemy has more than 90% of its strength"), name, format!("{} {}", r.squad_dead, tr("dead")), vec![f.at.clone()])),
            Outcome::Won => {}
        }
    }
}

/// The health of a map (see the module doc). `cache` keeps the fights while the map's content is
/// unchanged. An error when the map's files or the install do not allow the checks.
pub fn health(map: &str, pkg: &Package, install: &DtInstall, lang: &str, cache: &mut Cache) -> Result<Health, String> {
    let spec = pkg.maps.get(map).and_then(|m| m.spec.as_ref()).ok_or_else(|| format!("the map {map} does not read"))?;
    let mut findings = Vec::new();
    let unreached_buildings = reachability(pkg, map, spec, &mut findings);
    dead_ends(pkg, map, &mut findings);
    named(pkg, map, &mut findings);
    traps(pkg, map, spec, &unreached_buildings, &mut findings);
    let compiled = compile::compile(pkg, install, lang, map).map_err(|r: Report| r.to_text().lines().next().unwrap_or_default().to_string())?;
    let names = Names::of(&compiled.names);
    let scripted = scripted(map, pkg);
    let res = fights(map, pkg, install, lang, cache, &names, &scripted)?;
    fight_findings(&scripted, &res, &mut findings);
    let wages = {
        let mg = testers::open_map(pkg, install, map).map_err(|r| r.to_text().lines().next().unwrap_or_default().to_string())?;
        i64::from(mg.game.daily_wages())
    };
    budget(map, pkg, spec, &unreached_buildings, wages, &mut findings);
    // A named character who joins the hero and is needed later may die in a fight that the map has,
    // where the squad loses a unit in the tester's run.
    let squad_losses = res.values().any(|r| r.squad_dead > 0);
    if squad_losses {
        for c in characters::characters(pkg) {
            let needed: Vec<&characters::Site> = c.needs.iter().filter(|s| on_map(s, map)).collect();
            if !c.gives.is_empty() && !needed.is_empty() {
                let at: Vec<Target> = needed.iter().map(|s| Target::Line { file: s.file.clone(), line: s.line }).collect();
                findings.push(Finding::new("named_dies", Severity::Warning, n_("A named character who joins the hero is needed later, and the map has a fight that costs the squad units"), &c.id, format!("{} {}", needed.len(), tr("need")), at));
            }
        }
    }
    let summary = summary(&findings);
    Ok(Health { findings, summary })
}

/// The summary of findings: 100, less [`WARNING_COST`] for each warning and [`NOTE_COST`] for each
/// note, at least 0.
pub fn summary(findings: &[Finding]) -> i64 {
    let penalty: i64 = findings
        .iter()
        .map(|f| match f.severity {
            Severity::Warning => WARNING_COST,
            Severity::Info => NOTE_COST,
        })
        .sum();
    (100 - penalty).max(0)
}

#[cfg(test)]
#[path = "health/tests.rs"]
mod tests;
