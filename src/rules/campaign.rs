//! Campaigns of a content package in the running game (`pack::campaign` reads the graph).
//!
//! Where the state lives:
//! - the hero, the army, the pack, the gold and the journal: the original's hand-over
//!   ([`Game::from_campaign`], events.md §12), masked by the graph's `[carry]` flags;
//! - the variables and the fired-once triggers that name no map: the story's own state
//!   ([`TriggerState`]), carried with `vars`;
//! - the map a campaign game is on, and the stored state of each `revisit` map it left: the world
//!   (armies, buildings, objects), the fog, the event engine, the beaten armies, the map's own fired
//!   triggers and its spawned armies. Coming back restores it. These are [`CampaignState`]'s, saved
//!   with the game (a save mid-campaign is the same as any save).
//!
//! A map change is [`enter`]: the game of the next map, started with what carries (a first visit
//! runs the map's opening events and story, as a campaign hand-over does; a revisit restores its
//! stored state and runs nothing again). [`after_victory`] picks the route.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::clock::Clock;
use super::content::Content;
use super::events::{ArmyId, EventEngine, EventWorld};
use super::fog::Fog;
use super::game::Game;
use super::script::NextMap;
use super::triggers::Cond;
use super::world::World;
use crate::dt::dtm::Scenario;
use crate::pack::campaign::Carry;
use crate::pack::open::Opened;
use crate::pack::triggers::VarValue;

/// A route out of a map: the map it leads to, and the conditions that must hold for it.
#[derive(Clone, Debug, PartialEq)]
pub struct Route {
    pub to: String,
    pub cond: Vec<Cond>,
}

/// The campaign a game is part of: the map it is on, the stored state of the revisit maps it
/// left, and the clock they share. `ended` is the ending dialog once the campaign has ended.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CampaignState {
    pub current: String,
    /// The state of each revisit map the game left, by map id; taken back when it is entered.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub stored: BTreeMap<String, StoredMap>,
    /// The game's clock when it last left a map: a revisit goes on from it.
    #[serde(default)]
    pub clock: Option<Clock>,
    /// The ending reached (its dialog's id), once a story's `ending` ran.
    #[serde(default)]
    pub ended: Option<String>,
}

/// The state a revisit map keeps while the game is elsewhere: the world part (armies, buildings,
/// objects, relations and the rest of the world's dynamic state), the fog, the event engine and
/// the armies beaten, plus the map's own fired triggers and spawned armies. The map's statics come
/// from its scenario again ([`World::restore_statics`], [`EventEngine::restore_statics`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StoredMap {
    pub world: World,
    pub fog: Fog,
    pub script: Option<Box<EventEngine>>,
    pub beaten: BTreeSet<ArmyId>,
    pub ai_beaten: BTreeSet<ArmyId>,
    pub met: BTreeSet<ArmyId>,
    /// The fired triggers that name this map (`map = "…"`).
    pub fired: BTreeSet<String>,
    pub spawned: BTreeSet<ArmyId>,
}

impl CampaignState {
    /// The state of a campaign game that has just started on map `map`.
    pub fn start(map: &str) -> CampaignState {
        CampaignState { current: map.to_string(), ..CampaignState::default() }
    }
}

/// The map a victory leads to: the story's `victory = true, next = "…"` override, else the first
/// route whose conditions hold, in order. `None`: no route holds, and the campaign ends there.
pub fn after_victory(g: &mut Game, routes: &[Route]) -> Option<String> {
    if let Some(next) = g.triggers.next.take() {
        return Some(next);
    }
    routes.iter().find(|r| r.cond.iter().all(|c| g.holds(c))).map(|r| r.to.clone())
}

/// What the state of map `from` keeps while the game is elsewhere.
fn store(g: &Game) -> StoredMap {
    let scoped: BTreeSet<String> = g.story().map(|s| s.scoped.clone()).unwrap_or_default();
    StoredMap {
        world: g.world.clone(),
        fog: g.fog.clone(),
        script: g.script.clone(),
        beaten: g.beaten_armies.clone(),
        ai_beaten: g.ai_beaten.clone(),
        met: g.met_armies.clone(),
        fired: g.triggers.fired.iter().filter(|id| scoped.contains(*id)).cloned().collect(),
        spawned: g.triggers.spawned.clone(),
    }
}

/// The hand-over with the graph's `[carry]` flags applied: a flag that is off leaves the next map's
/// own (gold and mana, the pack, the army) or its new hero (level 1, his preset book and items).
fn masked(mut prev: NextMap, carry: &Carry, content: &Arc<Content>, scenario: &Scenario) -> NextMap {
    if !carry.gold {
        prev.gold = None;
        prev.mana = None;
    }
    if !carry.pack {
        prev.inventory = None;
    }
    if !carry.army {
        prev.army = None;
    }
    if !carry.hero {
        let fresh = Game::unstarted(content.clone(), scenario, prev.class);
        prev.hero = fresh.squad[0].clone();
        prev.spells = Some(fresh.spells.clone());
        prev.hero_items = true;
        prev.figure = fresh.hero_figure;
    }
    prev
}

/// The game of map `to` (the one `g` leaves, `from`), with what the campaign carries. `at` is the
/// point of `to` the hero comes in at (a `travel`); none: the map's start. A revisit restores the
/// state the game left there; any other entry starts the map (its opening events and story run).
/// `Err` names what cannot be restored (the package changed under a save).
pub fn enter(g: &Game, from: &Opened, to: &Opened, at: Option<&str>, content: Arc<Content>) -> Result<Game, String> {
    let carry = from.campaign.as_ref().map(|c| c.carry).unwrap_or_default();
    let mut state = g.campaign.as_deref().cloned().unwrap_or_else(|| CampaignState::start(&from.map));
    if from.revisit() {
        state.stored.insert(from.map.clone(), store(g));
    }
    let clock = g.clock;
    let scoped: BTreeSet<String> = g.story().map(|s| s.scoped.clone()).unwrap_or_default();
    let prev = masked(g.hand_over(to.map.clone(), None), &carry, &content, &to.scenario);
    let vars: BTreeMap<String, VarValue> = if carry.vars { g.triggers.vars.clone() } else { BTreeMap::new() };
    let fired: BTreeSet<String> = if carry.vars { g.triggers.fired.iter().filter(|id| !scoped.contains(*id)).cloned().collect() } else { BTreeSet::new() };
    let stored = if to.revisit() { state.stored.remove(&to.map) } else { None };
    let mut next = match stored {
        None => {
            let mut next = Game::from_campaign(content.clone(), &to.scenario, &prev);
            next.triggers.trace = g.triggers.trace.as_ref().map(|_| Vec::new());
            next.triggers.vars = vars;
            next.triggers.fired = fired;
            next.triggers.rng = g.triggers.rng;
            if let Some((x, y)) = at.and_then(|a| to.points.get(a)) {
                next.teleport_player(*x, *y);
            }
            to.start(&mut next);
            next
        }
        Some(stored) => {
            let mut next = Game::unstarted(content.clone(), &to.scenario, prev.class);
            next.apply_carry_over(&prev);
            next.carried = Some(Box::new(prev.clone()));
            let mut world = stored.world;
            world.restore_statics(World::from_scenario(&to.scenario, &content))?;
            next.world = world;
            next.fog = stored.fog;
            let mut script = stored.script;
            if let Some(engine) = script.as_mut() {
                engine.restore_statics(EventEngine::new(&to.scenario))?;
                engine.set_flag_string(&prev.flags);
            }
            next.script = script;
            next.beaten_armies = stored.beaten;
            next.ai_beaten = stored.ai_beaten;
            next.met_armies = stored.met;
            next.triggers.trace = g.triggers.trace.as_ref().map(|_| Vec::new());
            next.triggers.vars = vars;
            next.triggers.fired = fired.into_iter().chain(stored.fired).collect();
            next.triggers.spawned = stored.spawned;
            next.triggers.rng = g.triggers.rng;
            if let Some((x, y)) = at.and_then(|a| to.points.get(a)) {
                next.teleport_player(*x, *y);
            }
            next.clock = clock;
            next.set_origin(to.origin());
            next.attach_story(to.story.clone());
            next
        }
    };
    state.current = to.map.clone();
    state.clock = Some(clock);
    state.ended = None;
    next.campaign = Some(Box::new(state));
    Ok(next)
}

impl Game {
    /// The campaign ending reached: its dialog's id, once a story's `ending` ran.
    pub fn campaign_ended(&self) -> Option<&str> {
        self.campaign.as_ref()?.ended.as_deref()
    }

    /// Records the campaign's ending (the ending dialog's id).
    pub fn set_campaign_ended(&mut self, dialog: &str) {
        if let Some(c) = self.campaign.as_mut() {
            c.ended = Some(dialog.to_string());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::compile::{compile_with, Base};
    use crate::pack::files::Files;
    use crate::pack::load::Package;
    use crate::pack::open::{build, package_hash};
    use crate::pack::terrain::{encode_rgb, DEFAULT_PALETTE};
    use crate::pack::triggers::VarValue;
    use crate::rules::content::HeroClass;
    use crate::rules::save::{self, Install, ScenarioRef, SaveKind};
    use std::path::Path;
    use crate::rules::triggers::Travel;

    const MANIFEST: &str = "id = \"camp\"\nversion = \"1.0.0\"\nformat = 1\ntitle = \"camp.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
    const RELATIONS: &str = "relations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-3],[-3,-3,-3,3]]";

    /// A map `id`: a 50×50 meadow, the hero at (5, 5) with 3 militiamen and 100 gold; an enemy
    /// army `raiders` at (30, 30) with 2 bandits; `points` are its `[[point]]`s (id, cell) and the
    /// victory is a reach of (45, 45) (never met: the story ends the maps).
    fn map_toml(points: &[(&str, u32, u32)]) -> String {
        let mut s = format!("title = \"camp.title\"\nstart_date = \"1300-04-01\"\n{RELATIONS}\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\"]\ngold = 100\ntroops = [{{ unit = \"dt:militiaman\", level = 1, count = 3 }}]\n\n");
        s.push_str("[[army]]\nid = \"raiders\"\nname = \"camp.title\"\ncell = [30, 30]\nfaction = \"enemy\"\nmodel = \"bandits\"\ntroops = [{ unit = \"dt:bandit\", level = 1, count = 2 }]\n\n");
        for (id, x, y) in points {
            s.push_str(&format!("[[point]]\nid = \"{id}\"\ncell = [{x}, {y}]\n\n"));
        }
        s.push_str("[victory]\nkind = \"reach\"\ncell = [45, 45]\nmessage = \"camp.title\"\n");
        s
    }

    /// The campaign package: `brod` (start) goes to `marka` (a revisit map) when `ally` is 1 and to
    /// `salt` when it is 2; `marka` travels to `salt`; `salt` travels back to `marka` at the point
    /// `gate`, or ends the campaign with the dialog `the_end`.
    fn campaign() -> Files {
        let campaign = "start = \"brod\"\n\n[[map]]\nid = \"brod\"\n[[map.next]]\nto = \"marka\"\nif = [ { var = \"ally\", value = 1 } ]\n[[map.next]]\nto = \"salt\"\nif = [ { var = \"ally\", value = 2 } ]\n\n[[map]]\nid = \"marka\"\nrevisit = true\n\n[[map]]\nid = \"salt\"\n";
        let triggers = "[vars]\nally = 0\nspare = false\n\n\
[[trigger]]\nid = \"start\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"intro\" } ]\n\n\
[[trigger]]\nid = \"pick_a\"\nwhen = { event = \"dialog_choice\", dialog = \"intro\", choice = \"a\" }\nthen = [ { set = \"ally\", value = 1 }, { give_gold = 50 }, { give_unit = \"dt:militiaman\" }, { victory = true } ]\n\n\
[[trigger]]\nid = \"pick_b\"\nwhen = { event = \"dialog_choice\", dialog = \"intro\", choice = \"b\" }\nthen = [ { set = \"ally\", value = 2 }, { give_gold = 50 }, { victory = true } ]\n\n\
[[trigger]]\nid = \"won\"\nmap = \"marka\"\nwhen = { event = \"army_defeated\", army = \"raiders\" }\nthen = [ { set = \"spare\", value = true }, { dialog = \"hub\" } ]\n\n\
[[trigger]]\nid = \"marka_leave\"\nmap = \"marka\"\nwhen = { event = \"dialog_choice\", dialog = \"hub\", choice = \"leave\" }\nthen = [ { travel = \"salt\" } ]\n\n\
[[trigger]]\nid = \"salt_recruit\"\nmap = \"salt\"\nwhen = { event = \"game_start\" }\nthen = [ { give_unit = \"dt:militiaman\", named = \"tess\" } ]\n\n\
[[trigger]]\nid = \"salt_hello\"\nmap = \"salt\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"salt_hub\" } ]\n\n\
[[trigger]]\nid = \"salt_back\"\nmap = \"salt\"\nwhen = { event = \"dialog_choice\", dialog = \"salt_hub\", choice = \"back\" }\nthen = [ { travel = \"marka\", at = \"gate\" } ]\n\n\
[[trigger]]\nid = \"salt_end\"\nmap = \"salt\"\nwhen = { event = \"dialog_choice\", dialog = \"salt_hub\", choice = \"end\" }\nthen = [ { ending = \"the_end\" } ]\n";
        let dialogs = "[[dialog]]\nid = \"intro\"\ntitle = \"camp.title\"\ntext = \"camp.title\"\n\n[[dialog.choice]]\nid = \"a\"\ntext = \"camp.title\"\n\n[[dialog.choice]]\nid = \"b\"\ntext = \"camp.title\"\n";
        let hub = "[[dialog]]\nid = \"hub\"\ntitle = \"camp.title\"\ntext = \"camp.title\"\n\n[[dialog.choice]]\nid = \"leave\"\ntext = \"camp.title\"\n";
        let salt = "[[dialog]]\nid = \"salt_hub\"\ntitle = \"camp.title\"\ntext = \"camp.title\"\n\n[[dialog.choice]]\nid = \"back\"\ntext = \"camp.title\"\n\n[[dialog.choice]]\nid = \"end\"\ntext = \"camp.title\"\n";
        let the_end = "[[dialog]]\nid = \"the_end\"\ntitle = \"camp.title\"\ntext = \"camp.title\"\n";
        let mut f = Files::default();
        f.insert("package.toml", MANIFEST);
        f.insert("texts/en.toml", "[camp]\ntitle = \"Camp\"\n");
        f.insert("campaign.toml", campaign);
        f.insert("triggers/main.toml", triggers);
        f.insert("dialogs/intro.toml", dialogs);
        f.insert("dialogs/hub.toml", hub);
        f.insert("dialogs/salt.toml", salt);
        f.insert("dialogs/end.toml", the_end);
        f.insert("maps/brod/map.toml", map_toml(&[]).as_str());
        f.insert("maps/marka/map.toml", map_toml(&[("gate", 12, 9)]).as_str());
        f.insert("maps/salt/map.toml", map_toml(&[]).as_str());
        for m in ["brod", "marka", "salt"] {
            f.insert(&format!("maps/{m}/terrain.png"), encode_rgb(50, 50, |_, _| DEFAULT_PALETTE[6].1));
        }
        f
    }

    fn package() -> Package {
        let pkg = Package::parse(campaign()).0;
        let report = crate::pack::check(&pkg);
        assert!(!report.has_errors(), "{}", report.to_text());
        pkg
    }

    /// Map `id` of `pkg`, opened on the demo content (no install).
    fn open(pkg: &Package, id: &str) -> Opened {
        let content = Content::builtin();
        let picture = |_: u32| -> Option<usize> { None };
        let base = Base { units: &content.units, items: &content.items, picture: &picture };
        let compiled = compile_with(pkg, &base, "en", id).unwrap_or_else(|r| panic!("{}", r.to_text()));
        build(pkg, package_hash(&pkg.files), Path::new("camp"), id, compiled, "en").unwrap_or_else(|e| panic!("{e}"))
    }

    fn content_for(o: &Opened) -> Arc<Content> {
        Arc::new(Content::builtin().with_package(o.units.clone(), o.items.clone()))
    }

    /// The game of the start map, begun.
    fn begin(pkg: &Package) -> (Game, Opened) {
        let o = open(pkg, "brod");
        let mut g = Game::from_scenario(content_for(&o), &o.scenario, HeroClass::Knight);
        o.start(&mut g);
        (g, o)
    }

    /// The dialogs among `events`, by id.
    fn dialogs(events: Vec<crate::rules::game::Event>) -> Vec<String> {
        events.into_iter().filter_map(|e| if let crate::rules::game::Event::Package(d) = e { Some(d.id) } else { None }).collect()
    }

    /// Defeats army `id` the way a battle does: beaten, gone from the map, the story told.
    fn beat(g: &mut Game, id: u8) -> Vec<crate::rules::game::Event> {
        let before = g.story_battle_snapshot().expect("a story is attached");
        g.beaten_armies.insert(id);
        g.world.armies.retain(|a| a.id != id);
        g.story_battle_end(before)
    }

    fn var(g: &Game, name: &str) -> Option<VarValue> {
        g.story_var(name)
    }

    /// The route the campaign takes from `from` after its victory, entered as the UI enters it.
    fn go(g: &mut Game, from: &Opened, pkg: &Package, to: &str, at: Option<&str>) -> (Game, Opened) {
        let target = open(pkg, to);
        let next = enter(g, from, &target, at, content_for(&target)).unwrap_or_else(|e| panic!("{e}"));
        (next, target)
    }

    /// The campaign's first branch: `ally` 1 takes the victory to `marka`, with its gold and unit.
    #[test]
    fn a_victory_takes_the_branch_its_variable_names_and_the_gold_and_units_carry() {
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        let gold = g.gold;
        let units = g.squad.len();
        let events = g.answer_story_dialog("intro", Some("a"));
        assert!(dialogs(events).is_empty());
        assert_eq!(g.script_end(), Some(crate::rules::script::ScriptEnd::Package(crate::rules::triggers::Ending::Victory)));
        assert_eq!(after_victory(&mut g, &brod.routes), Some("marka".to_string()), "ally 1: marka");
        let (next, marka) = go(&mut g, &brod, &pkg, "marka", None);
        assert_eq!(next.gold, gold + 50, "the gold carries");
        assert_eq!(next.squad.len(), units + 1, "the unit the victory gave carries");
        assert_eq!(var(&next, "ally"), Some(VarValue::Int(1)), "the variables carry");
        assert_eq!(next.campaign.as_ref().unwrap().current, "marka");
        assert!(marka.revisit());
        assert_eq!(next.script_end(), None, "a new map starts with no ending");
    }

    /// The other branch: `ally` 2 takes the victory to `salt`.
    #[test]
    fn the_other_branch_goes_to_salt() {
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        g.answer_story_dialog("intro", Some("b"));
        assert_eq!(after_victory(&mut g, &brod.routes), Some("salt".to_string()));
    }

    /// No route holds (ally 0): the campaign ends with no next map.
    #[test]
    fn a_victory_with_no_route_ends_the_campaign() {
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        assert_eq!(after_victory(&mut g, &brod.routes), None);
    }

    /// A revisit map keeps a defeated army defeated across a trip away and back, its fired
    /// triggers are not run again, and the hero comes in at the travel's point.
    #[test]
    fn a_revisit_map_keeps_its_defeated_army_and_its_fired_triggers() {
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        g.answer_story_dialog("intro", Some("a"));
        let (mut marka_game, marka) = go(&mut g, &brod, &pkg, "marka", None);
        drop(g);
        assert!(marka_game.world.armies.iter().any(|a| a.id == 1), "the raiders are on the map");
        let events = beat(&mut marka_game, 1);
        assert_eq!(dialogs(events), ["hub"], "the won trigger opens the hub");
        assert_eq!(var(&marka_game, "spare"), Some(VarValue::Bool(true)));
        // Away: marka travels to salt.
        assert!(marka_game.triggers.travel.is_none());
        marka_game.answer_story_dialog("hub", Some("leave"));
        let travel = marka_game.triggers.travel.take().expect("travel to salt");
        assert_eq!(travel.map, "salt");
        let (salt_game, salt) = go(&mut marka_game, &marka, &pkg, "salt", travel.at.as_deref());
        let mut salt_game = salt_game;
        assert!(salt_game.campaign.as_ref().unwrap().stored.contains_key("marka"), "marka's state is kept");
        // Back: salt travels to marka at the gate.
        salt_game.answer_story_dialog("salt_hub", Some("back"));
        let back = salt_game.triggers.travel.take().expect("travel to marka");
        assert_eq!((back.map.as_str(), back.at.as_deref()), ("marka", Some("gate")));
        let (again, marka2) = go(&mut salt_game, &salt, &pkg, "marka", back.at.as_deref());
        assert!(!again.world.armies.iter().any(|a| a.id == 1), "the defeated army stays defeated");
        assert!(again.beaten_armies.contains(&1), "its beaten mark is kept");
        assert_eq!(var(&again, "spare"), Some(VarValue::Bool(true)));
        assert!(again.triggers.fired.contains("won"), "the fired trigger of marka is kept");
        assert_eq!(again.pos, again.world.map.center(pkg_point(&marka2, "gate")), "the hero comes in at the gate");
        assert!(!again.campaign.as_ref().unwrap().stored.contains_key("salt"), "salt is not a revisit map: nothing kept");
    }

    /// The point a campaign travel enters a map at.
    fn pkg_point(o: &Opened, id: &str) -> (i32, i32) {
        *o.points.get(id).expect("the point")
    }

    /// `salt`'s «end» choice ends the campaign with its dialog.
    #[test]
    fn an_ending_is_recorded_and_its_dialog_opens() {
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        g.answer_story_dialog("intro", Some("b"));
        let (mut salt_game, _) = go(&mut g, &brod, &pkg, "salt", None);
        let events = salt_game.answer_story_dialog("salt_hub", Some("end"));
        assert_eq!(dialogs(events), ["the_end"], "the ending's dialog opens");
        assert_eq!(salt_game.triggers.ending.as_deref(), Some("the_end"));
    }

    /// A campaign save mid-campaign restores the map, the stored revisit state and the carried
    /// values: the round trip the save file makes.
    #[test]
    fn a_mid_campaign_save_round_trips() {
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        g.answer_story_dialog("intro", Some("a"));
        let (mut marka_game, marka) = go(&mut g, &brod, &pkg, "marka", None);
        beat(&mut marka_game, 1);
        marka_game.answer_story_dialog("hub", Some("leave"));
        let travel = marka_game.triggers.travel.take().unwrap();
        let (salt_game, salt) = go(&mut marka_game, &marka, &pkg, &travel.map, None);
        let meta = save::meta_of(&salt_game, SaveKind::Manual, "mid").expect("a meta");
        let bytes = save::encode(&meta, &salt_game).expect("encodes");
        let (meta2, decoded) = save::decode(&bytes).expect("decodes");
        let content = content_for(&salt);
        let install = Install { dir: Path::new(""), content: content.clone(), pack: Some(&salt) };
        let restored = save::restore(&meta2, decoded, Arc::new(Content::builtin()), Some(&install)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(restored.campaign.as_ref().unwrap().current, "salt");
        assert!(restored.campaign.as_ref().unwrap().stored.contains_key("marka"), "marka's stored state survives the save");
        assert_eq!(var(&restored, "spare"), var(&salt_game, "spare"));
        assert_eq!(restored.gold, salt_game.gold);
        assert!(matches!(meta2.scenario, ScenarioRef::Pack { ref map, .. } if map == "salt"));
        // And it goes on: back to marka, the stored state restored.
        let mut restored = restored;
        let (back, _) = go(&mut restored, &salt, &pkg, "marka", Some("gate"));
        assert!(!back.world.armies.iter().any(|a| a.id == 1), "still defeated after the reload");
    }

    /// The campaign walked by the sim (`pack::sim::run_campaign`): brod's victory goes to marka with
    /// its gold, its unit and its variables; marka's day-one victory (auto-win) has no route, and the
    /// campaign ends. The trace of brod has the trigger that won it.
    #[test]
    fn the_sim_walks_the_campaign_graph_with_what_carries() {
        use crate::pack::sim::{run_campaign, CampaignOptions, Choices};
        use crate::rules::triggers::Trace;
        let pkg = package();
        let base = Content::builtin();
        let mut open_map = |map: &str| -> Result<Opened, String> { Ok(open(&pkg, map)) };
        let opts = CampaignOptions { days: 3, choices: Choices::Forced(vec![("intro".into(), "a".into())]), trace: true, auto_win_after: Some(1), preset: Vec::new(), save_on_revisit: false };
        let run = run_campaign(&pkg, &base, &mut open_map, &opts);
        assert!(run.ok(), "{}", run.text());
        assert_eq!(run.steps.len(), 2, "{}", run.text());
        assert_eq!(run.steps[0].next.as_deref(), Some("marka"), "{}", run.text());
        assert!(run.steps[1].entered_with.contains("gold 150"), "{}", run.text());
        assert!(run.steps[1].entered_with.contains("vars ally=1 spare=false"), "{}", run.text());
        assert!(run.steps[1].entered_with.contains("squad 5"), "{}", run.text());
        assert_eq!(run.steps[1].next, None, "no route from marka: the campaign ends");
        assert!(run.steps[0].trace.iter().any(|(_, w)| matches!(w, Trace::Fired { trigger, .. } if trigger == "pick_a")), "{}", run.text());
        assert_eq!(run.ending, None);
    }

    /// A named character keeps its id over the maps: salt's recruit is named in salt only, yet the
    /// package numbers its named characters once, so brod and salt agree on them.
    #[test]
    fn named_characters_keep_their_ids_over_the_maps() {
        let pkg = package();
        let brod = open(&pkg, "brod");
        let salt = open(&pkg, "salt");
        assert_eq!(brod.story.named, ["tess"], "brod names the package's characters too");
        assert_eq!(salt.story.named, brod.story.named);
        let mut recruit = Game::from_scenario(content_for(&salt), &salt.scenario, HeroClass::Knight);
        salt.start(&mut recruit);
        assert!(recruit.squad.iter().any(|u| u.named == crate::rules::triggers::PACKAGE_BASE), "tess joins as the package's first named character");
    }

    /// The wishes' per-army goal, speed and figure live in the armies, so a revisit keeps them; the
    /// hero's sheet (`hero_figure`, the story's) resets to the default on every map change.
    #[test]
    fn a_revisit_keeps_army_goals_speeds_and_figures_and_the_hero_sheet_resets() {
        use crate::rules::ai::goals::Goal;
        let pkg = package();
        let (mut g, brod) = begin(&pkg);
        g.answer_story_dialog("intro", Some("a"));
        let (mut marka_game, marka) = go(&mut g, &brod, &pkg, "marka", None);
        assert!(marka_game.set_army_goal(1, Some(Goal::GoTo((9, 9)))));
        let i = marka_game.world.armies.iter().position(|a| a.id == 1).expect("raiders on the map");
        marka_game.world.armies[i].speed = 9;
        marka_game.world.armies[i].figure = 3;
        marka_game.triggers.hero_sheet = Some(10);
        marka_game.triggers.travel = Some(Travel { map: "salt".into(), at: None });
        let travel = marka_game.triggers.travel.take().unwrap();
        let (mut salt_game, salt) = go(&mut marka_game, &marka, &pkg, &travel.map, None);
        assert_eq!(salt_game.triggers.hero_sheet, None, "a map change resets the hero's sheet");
        salt_game.triggers.travel = Some(Travel { map: "marka".into(), at: Some("gate".into()) });
        let back = salt_game.triggers.travel.take().unwrap();
        let (again, _) = go(&mut salt_game, &salt, &pkg, &back.map, back.at.as_deref());
        let a = again.world.armies.iter().find(|a| a.id == 1).expect("raiders kept");
        assert_eq!(a.mind.goal.as_ref().map(|x| x.goal.clone()), Some(Goal::GoTo((9, 9))), "the goal is kept");
        assert_eq!((a.speed, a.figure), (9, 3), "the speed and figure are kept");
        assert_eq!(again.triggers.hero_sheet, None, "the hero's sheet is back to the default");
    }

    /// A package with no `campaign.toml` has no campaign: its game keeps no campaign state, as before.
    #[test]
    fn a_package_without_a_campaign_keeps_no_campaign_state() {
        let pkg = crate::pack::open::test_support::story_package();
        let o = crate::pack::open::test_support::opened(&pkg);
        assert!(o.campaign.is_none() && o.routes.is_empty() && !o.revisit());
        let mut g = Game::from_scenario(content_for(&o), &o.scenario, HeroClass::Knight);
        o.start(&mut g);
        assert!(g.campaign.is_none());
    }
}
