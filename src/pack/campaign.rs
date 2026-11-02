//! Campaigns of a package: `campaign.toml`, the graph of its maps. A map's victory (or a
//! `travel`) leads to the next map: the first `next` whose `if` holds, else the campaign ends.
//! `read` reads the file, `check` runs the rules of the graph, and `routes` compiles the
//! conditions of a map's `next` entries for the game (`rules::campaign`). The format is in
//! `docs/reference/rzpack-format.md` («Campaigns»).

use std::collections::{BTreeMap, BTreeSet};

use serde::Deserialize;
use toml::{Spanned, Table};

use super::compile::{item_ids, unit_ids};
use super::diag::Report;
use super::load::{line_of, parse_toml, Package};
use super::triggers::{self as pt, Action, Cmp, VarKind, VarValue};
use crate::rules::campaign::Route;
use crate::rules::content::{ItemId, UnitId};
use crate::rules::triggers::{self as rt, Resolve};

/// The file that holds the graph, at the package's top.
pub const FILE: &str = "campaign.toml";

/// What a campaign carries from one map to the next (`[carry]`, all on by default).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Carry {
    /// Level, XP, spells and worn items of the hero.
    pub hero: bool,
    /// The squad, with its named units, their levels and items.
    pub army: bool,
    /// The pack (unworn items).
    pub pack: bool,
    /// Gold (and the god's favour, mana, which goes with it).
    pub gold: bool,
    /// Every variable, and the fired-once triggers that name no map.
    pub vars: bool,
}

impl Default for Carry {
    fn default() -> Self {
        Carry { hero: true, army: true, pack: true, gold: true, vars: true }
    }
}

/// The campaign: its first map, its name, what carries, and its maps in file order.
#[derive(Clone, Debug, PartialEq)]
pub struct Campaign {
    /// The map the campaign starts on.
    pub start: String,
    /// A text key: the campaign's name (the new-game list shows it).
    pub title: Option<String>,
    pub carry: Carry,
    pub maps: Vec<CampaignMap>,
}

/// One map of the campaign: whether leaving it keeps its state, and where it goes on a victory.
#[derive(Clone, Debug, PartialEq)]
pub struct CampaignMap {
    pub id: String,
    /// The line of its `[[map]]` table in `campaign.toml`.
    pub line: u32,
    /// Leaving the map keeps its state (armies, buildings, fog, its fired triggers), and coming
    /// back restores it.
    pub revisit: bool,
    /// The victory's routes, in order: the first whose `if` holds is taken.
    pub next: Vec<Next>,
}

/// A route out of a map: the map it goes to and the conditions (all must hold; none: always).
#[derive(Clone, Debug, PartialEq)]
pub struct Next {
    pub to: String,
    pub line: u32,
    pub cond: Vec<pt::Cond>,
}

impl Campaign {
    /// The entry of map `id`, if the campaign has one.
    pub fn map(&self, id: &str) -> Option<&CampaignMap> {
        self.maps.iter().find(|m| m.id == id)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCampaign {
    start: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    carry: Option<RawCarry>,
    #[serde(default)]
    map: Vec<Spanned<RawMap>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct RawCarry {
    hero: Option<bool>,
    army: Option<bool>,
    pack: Option<bool>,
    gold: Option<bool>,
    vars: Option<bool>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawMap {
    id: String,
    #[serde(default)]
    revisit: bool,
    #[serde(default)]
    next: Vec<Spanned<RawNext>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawNext {
    to: String,
    #[serde(rename = "if", default)]
    cond: Vec<Table>,
}

/// Reads `campaign.toml`. A table that does not read is reported at its line and left out;
/// a file that does not parse at all gives no campaign.
pub fn read(text: &str, report: &mut Report) -> Option<Campaign> {
    let raw: RawCampaign = parse_toml(FILE, text, report)?;
    let carry = match raw.carry {
        None => Carry::default(),
        Some(c) => {
            let d = Carry::default();
            Carry { hero: c.hero.unwrap_or(d.hero), army: c.army.unwrap_or(d.army), pack: c.pack.unwrap_or(d.pack), gold: c.gold.unwrap_or(d.gold), vars: c.vars.unwrap_or(d.vars) }
        }
    };
    let mut maps = Vec::new();
    for m in raw.map {
        let line = line_of(text, m.span().start);
        let m = m.into_inner();
        let mut next = Vec::new();
        for n in m.next {
            let nline = line_of(text, n.span().start);
            let n = n.into_inner();
            let mut cond = Vec::new();
            for t in &n.cond {
                match pt::parse_cond(t) {
                    Ok(c) => cond.push(c),
                    Err(p) => report.error_hint(FILE, Some(nline), p.message, p.hint),
                }
            }
            next.push(Next { to: n.to, line: nline, cond });
        }
        maps.push(CampaignMap { id: m.id, line, revisit: m.revisit, next });
    }
    Some(Campaign { start: raw.start, title: raw.title, carry, maps })
}

/// The package's names a campaign's conditions can use: its units and items (a campaign names no
/// army or building: those belong to a map, and such conditions are refused by `check`).
struct PackageNames {
    units: BTreeMap<String, u32>,
    items: BTreeMap<String, u32>,
}

impl PackageNames {
    fn of(pkg: &Package) -> PackageNames {
        PackageNames {
            units: unit_ids(pkg).into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            items: item_ids(pkg).into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
        }
    }
}

impl Resolve for PackageNames {
    fn unit(&self, local: &str) -> Option<UnitId> {
        self.units.get(local).map(|&id| UnitId(id))
    }
    fn item(&self, local: &str) -> Option<ItemId> {
        self.items.get(local).map(|&id| ItemId(id))
    }
    fn army(&self, _: &str) -> Option<u8> {
        None
    }
    fn building(&self, _: &str) -> Option<u16> {
        None
    }
}

/// The routes out of map `map` of the package's campaign, their conditions compiled for the game.
/// Empty when the package has no campaign or `map` is not in it. `Err` names what does not compile
/// (`check` reports the same).
pub fn routes(pkg: &Package, map: &str) -> Result<Vec<Route>, Vec<String>> {
    let Some(entry) = pkg.campaign.as_ref().and_then(|c| c.map(map)) else { return Ok(Vec::new()) };
    let names = PackageNames::of(pkg);
    let mut out = Vec::new();
    let mut errors = Vec::new();
    for n in &entry.next {
        match rt::compile_conditions(pkg, &names, &n.cond) {
            Ok(cond) => out.push(Route { to: n.to.clone(), cond }),
            Err(e) => errors.extend(e),
        }
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// The trigger and dialog actions of the package, with their place (file and line) and the maps
/// they apply to (`None`: every map of the campaign).
fn actions(pkg: &Package) -> Vec<(String, u32, Option<String>, &Action)> {
    let mut out = Vec::new();
    for t in &pkg.triggers {
        for a in &t.value.then {
            out.push((t.file.clone(), t.line, t.value.map.clone(), a));
        }
    }
    for d in &pkg.dialogs {
        for c in &d.value.choices {
            for a in &c.then {
                out.push((d.file.clone(), d.line, None, a));
            }
        }
    }
    out
}

/// Checks the campaign's graph against the package's maps, variables, dialogs and points. Only a
/// package with a `campaign.toml` is checked.
pub fn check(pkg: &Package, r: &mut Report) {
    let Some(c) = pkg.campaign.as_ref() else { return };
    let maps_in_package: Vec<&str> = pkg.maps.keys().map(String::as_str).collect();
    let in_campaign: BTreeSet<&str> = c.maps.iter().map(|m| m.id.as_str()).collect();

    let mut seen = BTreeSet::new();
    for m in &c.maps {
        if !seen.insert(m.id.as_str()) {
            r.error_hint(FILE, Some(m.line), format!("map `{}` is listed twice", m.id), "each map is one [[map]] table");
        }
        if !pkg.maps.contains_key(&m.id) {
            r.error_hint(FILE, Some(m.line), format!("no map `{}` in the package", m.id), format!("its maps are: {}", maps_in_package.join(", ")));
        }
    }
    for m in pkg.maps.keys() {
        if !in_campaign.contains(m.as_str()) {
            r.warning_hint(FILE, None, format!("map `{m}` is not in the campaign"), "list it in a [[map]] table, or it is never played in this campaign");
        }
    }
    if !in_campaign.contains(c.start.as_str()) {
        r.error_hint(FILE, None, format!("the start `{}` is not a campaign map", c.start), "start names one of the [[map]] ids");
    }
    if let Some(key) = &c.title {
        if !pkg.texts.values().any(|t| t.contains_key(key)) {
            r.error_hint(FILE, None, format!("the title text `{key}` is missing"), "add it to texts/<lang>.toml");
        }
    }

    for m in &c.maps {
        for n in &m.next {
            if !in_campaign.contains(n.to.as_str()) {
                r.error_hint(FILE, Some(n.line), format!("no campaign map `{}`", n.to), format!("the campaign's maps are: {}", c.maps.iter().map(|m| m.id.as_str()).collect::<Vec<_>>().join(", ")));
            }
            for cond in &n.cond {
                check_cond(pkg, cond, n.line, r);
            }
        }
    }
    if let Err(errors) = routes_all(pkg, c) {
        for e in errors {
            r.error(FILE, None, e);
        }
    }

    // The map a story's travel or victory names, and the point it enters at.
    for (file, line, _, a) in actions(pkg) {
        match a {
            Action::VictoryTo(to) => {
                if !in_campaign.contains(to.as_str()) {
                    r.error_hint(&file, Some(line), format!("no campaign map `{to}`"), "a victory's `next` names a [[map]] of campaign.toml");
                }
            }
            Action::Travel { map, at } => {
                if !in_campaign.contains(map.as_str()) {
                    r.error_hint(&file, Some(line), format!("no campaign map `{map}`"), "a travel names a [[map]] of campaign.toml");
                    continue;
                }
                if let Some(at) = at {
                    if !points_of(pkg, map).iter().any(|(id, _)| id == at) {
                        r.error_hint(&file, Some(line), format!("map `{map}` has no point `{at}`"), "a [[point]] of the map takes `id = \"…\"`");
                    }
                }
            }
            _ => {}
        }
    }

    // Reachability: from the start, along the routes and the travels and victories the stories
    // name (over-approximated: a story without `map` may act on any map).
    let mut edges: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    let mut ends: BTreeSet<&str> = BTreeSet::new();
    for m in &c.maps {
        edges.entry(m.id.as_str()).or_default().extend(m.next.iter().map(|n| n.to.as_str()));
        if m.next.is_empty() {
            ends.insert(m.id.as_str());
        }
    }
    for t in &pkg.triggers {
        let from: Vec<&str> = match &t.value.map {
            Some(m) => vec![m.as_str()],
            None => c.maps.iter().map(|m| m.id.as_str()).collect(),
        };
        for a in &t.value.then {
            let to = match a {
                Action::VictoryTo(to) => Some(to.as_str()),
                Action::Travel { map, .. } => Some(map.as_str()),
                _ => None,
            };
            for &f in &from {
                if let Some(to) = to {
                    edges.entry(f).or_default().insert(to);
                }
                if matches!(a, Action::Ending(_)) {
                    ends.insert(f);
                }
            }
        }
    }
    for d in &pkg.dialogs {
        for ch in &d.value.choices {
            for a in &ch.then {
                let to = match a {
                    Action::VictoryTo(to) => Some(to.as_str()),
                    Action::Travel { map, .. } => Some(map.as_str()),
                    _ => None,
                };
                for m in &c.maps {
                    if let Some(to) = to {
                        edges.entry(m.id.as_str()).or_default().insert(to);
                    }
                    if matches!(a, Action::Ending(_)) {
                        ends.insert(m.id.as_str());
                    }
                }
            }
        }
    }
    let mut reached: BTreeSet<&str> = BTreeSet::new();
    let mut todo = vec![c.start.as_str()];
    while let Some(m) = todo.pop() {
        if reached.insert(m) {
            todo.extend(edges.get(m).into_iter().flatten().copied());
        }
    }
    for m in &c.maps {
        if !reached.contains(m.id.as_str()) {
            r.warning_hint(FILE, Some(m.line), format!("map `{}` is never reached from `{}`", m.id, c.start), "a route, a travel or a victory must lead to it");
        }
    }
    if !ends.iter().any(|e| reached.contains(e)) {
        r.error_hint(FILE, None, format!("no path from `{}` to an end", c.start), "a map with no `next`, or an `ending` of a story, must be reachable");
    }
}

/// Every route's conditions compiled (the same as `routes`, for each map).
fn routes_all(pkg: &Package, c: &Campaign) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    for m in &c.maps {
        if let Err(e) = routes(pkg, &m.id) {
            errors.extend(e);
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The points of map `map` with an id: their names and cells.
pub fn points_of(pkg: &Package, map: &str) -> Vec<(String, (i32, i32))> {
    let Some(spec) = pkg.maps.get(map).and_then(|m| m.spec.as_ref()) else { return Vec::new() };
    spec.point
        .iter()
        .filter_map(|p| {
            let p = p.get_ref();
            p.id.clone().map(|id| (id, (p.cell[0] as i32, p.cell[1] as i32)))
        })
        .collect()
}

/// A campaign condition: a variable (declared, with a value of its kind), or an item, unit or
/// spell of the package. Armies, buildings and places belong to a map.
fn check_cond(pkg: &Package, cond: &pt::Cond, line: u32, r: &mut Report) {
    match cond {
        pt::Cond::Var { name, value, cmp } => match pkg.vars.iter().find(|v| &v.value.name == name) {
            None => r.error_hint(FILE, Some(line), format!("variable `{name}` is not declared"), "declare it in a triggers/*.toml [vars] table"),
            Some(v) => {
                let kind_ok = matches!((v.value.kind, value), (VarKind::Int, VarValue::Int(_)) | (VarKind::Bool, VarValue::Bool(_)));
                if !kind_ok {
                    r.error_hint(FILE, Some(line), format!("`{name}` does not hold that kind of value"), "compare an int with a whole number, a bool with true or false");
                }
                if v.value.kind == VarKind::Bool && !matches!(cmp, Cmp::Eq | Cmp::Ne) {
                    r.error_hint(FILE, Some(line), format!("`{name}` is true or false: only == and != compare it"), "use == or !=");
                }
            }
        },
        pt::Cond::ArmyAt { .. } | pt::Cond::ArmyAlive { .. } | pt::Cond::BuildingOwner { .. } | pt::Cond::HeroAt(_) => {
            r.error_hint(FILE, Some(line), "a campaign condition names no army, building or place", "use a variable, gold, an item or a spell: the map's own conditions are in its triggers");
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    const MANIFEST: &str = "id = \"camp-test\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"t.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
    const MAP: &str = "title = \"t.title\"\nstart_date = \"1300-04-01\"\nrelations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-3],[-3,-3,-3,3]]\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\"]\ngold = 100\ntroops = [{ unit = \"dt:militiaman\", level = 1, count = 3 }]\n\n[victory]\nkind = \"reach\"\ncell = [45, 45]\n";

    /// A package with maps `a`, `b`, `c` (no terrain: the campaign rules do not read it), and the
    /// given campaign.toml and trigger file.
    fn package(campaign: &str, triggers: &str) -> Package {
        let mut f = Files::default();
        f.insert("package.toml", MANIFEST);
        f.insert("texts/en.toml", "[t]\ntitle = \"T\"\n");
        f.insert("campaign.toml", campaign);
        f.insert("triggers/main.toml", triggers);
        for m in ["a", "b", "c"] {
            f.insert(&format!("maps/{m}/map.toml"), MAP);
        }
        Package::parse(f).0
    }

    fn errors_of(pkg: &Package) -> Vec<String> {
        let mut r = Report::default();
        check(pkg, &mut r);
        r.diagnostics.iter().filter(|d| d.severity == crate::pack::Severity::Error).map(|d| d.message.clone()).collect()
    }

    const GRAPH: &str = "start = \"a\"\n[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\nif = [ { var = \"ally\", op = \"==\", value = 1 } ]\n[[map.next]]\nto = \"c\"\n[[map]]\nid = \"b\"\nrevisit = true\n[[map]]\nid = \"c\"\n";

    #[test]
    fn a_graph_reads_with_its_routes_and_carry() {
        let mut r = Report::default();
        let c = read(GRAPH, &mut r).expect("reads");
        assert!(!r.has_errors());
        assert_eq!(c.start, "a");
        assert_eq!(c.carry, Carry::default(), "everything carries when [carry] is not written");
        assert_eq!(c.maps.len(), 3);
        assert!(c.map("b").unwrap().revisit);
        assert_eq!(c.map("a").unwrap().next[0].to, "b");
        assert_eq!(c.map("a").unwrap().next[1].cond.len(), 0, "a route with no if always holds");
        let c = read("start = \"a\"\n[carry]\ngold = false\n[[map]]\nid = \"a\"\n", &mut r).unwrap();
        assert_eq!(c.carry, Carry { gold: false, ..Carry::default() });
    }

    #[test]
    fn a_graph_with_a_good_route_and_a_declared_variable_checks() {
        let pkg = package(GRAPH, "[vars]\nally = 0\n\n[[trigger]]\nid = \"t\"\nwhen = { event = \"game_start\" }\nthen = [ { victory = true } ]\n");
        assert_eq!(errors_of(&pkg), Vec::<String>::new());
    }

    #[test]
    fn unknown_maps_and_undeclared_variables_are_errors() {
        let pkg = package("start = \"a\"\n[[map]]\nid = \"a\"\n[[map.next]]\nto = \"zz\"\n[[map]]\nid = \"q\"\n", "[vars]\n");
        let e = errors_of(&pkg).join("\n");
        assert!(e.contains("no map `q` in the package"), "{e}");
        assert!(e.contains("no campaign map `zz`"), "{e}");
        let pkg = package("start = \"a\"\n[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\nif = [ { var = \"ally\", value = 1 } ]\n[[map]]\nid = \"b\"\n", "[vars]\n");
        assert!(errors_of(&pkg).join("\n").contains("variable `ally` is not declared"));
    }

    #[test]
    fn a_route_names_no_army_and_a_victory_names_a_campaign_map() {
        let pkg = package("start = \"a\"\n[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\nif = [ { army_alive = \"x\" } ]\n[[map]]\nid = \"b\"\n", "[vars]\n");
        assert!(errors_of(&pkg).join("\n").contains("names no army, building or place"));
        let pkg = package("start = \"a\"\n[[map]]\nid = \"a\"\n[[map]]\nid = \"b\"\n", "[vars]\n\n[[trigger]]\nid = \"t\"\nmap = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { victory = true, next = \"c\" } ]\n");
        assert!(errors_of(&pkg).join("\n").contains("no campaign map `c`"));
    }

    #[test]
    fn a_travel_names_a_point_the_map_has() {
        let pkg = package("start = \"a\"\n[[map]]\nid = \"a\"\n[[map]]\nid = \"b\"\n", "[vars]\n\n[[trigger]]\nid = \"t\"\nmap = \"a\"\nwhen = { event = \"game_start\" }\nthen = [ { travel = \"b\", at = \"gate\" } ]\n");
        assert!(errors_of(&pkg).join("\n").contains("map `b` has no point `gate`"));
    }

    #[test]
    fn an_unreachable_map_warns_and_no_end_is_an_error() {
        let pkg = package("start = \"a\"\n[[map]]\nid = \"a\"\n[[map.next]]\nto = \"b\"\n[[map]]\nid = \"b\"\n[[map.next]]\nto = \"a\"\n[[map]]\nid = \"c\"\n", "[vars]\n");
        let mut r = Report::default();
        check(&pkg, &mut r);
        assert!(r.diagnostics.iter().any(|d| d.message.contains("map `c` is never reached")), "{}", r.to_text());
        assert!(errors_of(&pkg).join("\n").contains("no path from `a` to an end"), "a and b loop, neither ends");
    }

    #[test]
    fn the_routes_of_a_map_compile_for_the_game() {
        let pkg = package(GRAPH, "[vars]\nally = 0\n");
        let routes = routes(&pkg, "a").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(routes.len(), 2);
        assert_eq!(routes[0].to, "b");
        assert_eq!(routes[0].cond.len(), 1);
        assert!(routes[1].cond.is_empty());
        assert!(super::routes(&pkg, "c").unwrap().is_empty(), "a map with no next has no routes");
    }
}
