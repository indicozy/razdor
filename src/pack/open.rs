//! Opening a package's map as a game: the package read from its folder or `.rzpack`, checked, its
//! map compiled on the install, its story compiled for that map, and the package's hash. The game
//! (`ui::pack_play`, `rules::save`) plays the result; a save names the map by [`Opened::origin`] and
//! finds the package again by its path, which it checks against the hash.

use std::path::{Path, PathBuf};

use super::check::check;
use super::compile::{self, Compiled};
use super::files::Files;
use super::load::Package;
use crate::dt::data::{ArtefactDef, UnitDef};
use crate::dt::dtm::Scenario;
use crate::dt::install::DtInstall;
use crate::rules::game::Game;
use crate::rules::save::{fnv1a, ScenarioRef};
use crate::rules::triggers::{self, Story};

/// A package's map, opened: what a game of it needs.
#[derive(Clone, Debug)]
pub struct Opened {
    /// The package's id and version (`package.toml`).
    pub id: String,
    pub version: String,
    /// The package's content hash ([`package_hash`]).
    pub hash: u64,
    /// The path the package was read from: its folder or its `.rzpack`.
    pub path: PathBuf,
    /// The map's id (its folder name).
    pub map: String,
    pub scenario: Scenario,
    /// The package's units and items, with their new ids (`Content::with_package`).
    pub units: Vec<UnitDef>,
    pub items: Vec<ArtefactDef>,
    /// `(new unit id, picture frame)` of each package unit with a picture.
    pub pictures: Vec<(u32, usize)>,
    /// The package's triggers and dialogs for this map.
    pub story: Story,
}

/// The language the package's texts are read in: Russian when the game is, else English.
pub fn lang() -> &'static str {
    if matches!(crate::i18n::lang(), crate::i18n::Lang::Ru) {
        "ru"
    } else {
        "en"
    }
}

/// The content hash of a package: FNV-1a over each file's path and bytes, in path order.
pub fn package_hash(files: &Files) -> u64 {
    let mut bytes = Vec::new();
    for path in files.paths() {
        bytes.extend_from_slice(path.as_bytes());
        bytes.push(0);
        bytes.extend_from_slice(files.get(path).unwrap_or_default());
        bytes.push(0);
    }
    fnv1a(&bytes)
}

/// The seed of a package's story generator: from its hash, never zero.
pub fn story_seed(hash: u64) -> u32 {
    (hash as u32) | 1
}

/// Reads the package at `path` and opens its map `map` (the first map when none is named) on
/// `install`. The errors are for a person: the package does not read or check, or the map does
/// not compile.
pub fn open(path: &Path, map: Option<&str>, lang: &str, install: &DtInstall) -> Result<Opened, String> {
    let files = Files::load(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let hash = package_hash(&files);
    let (package, _) = Package::parse(files);
    let report = check(&package);
    if report.has_errors() {
        return Err(format!("{} does not check:\n{}", path.display(), report.to_text()));
    }
    let map_id = match map {
        Some(m) => m.to_string(),
        None => package.maps.keys().next().cloned().ok_or("the package has no map (maps/<id>/map.toml)")?,
    };
    let compiled = compile::compile(&package, install, lang, &map_id).map_err(|r| r.to_text())?;
    build(&package, hash, path, &map_id, compiled, lang)
}

/// The opened map from a compiled map of `package`: its story is compiled against the map's ids.
/// `hash` is the package's [`package_hash`]. The tests open packages this way without an install.
pub fn build(package: &Package, hash: u64, path: &Path, map: &str, compiled: Compiled, lang: &str) -> Result<Opened, String> {
    let manifest = package.manifest.as_ref().ok_or("the package has no package.toml")?;
    let story = triggers::compile(package, lang, map, &compiled.names, story_seed(hash)).map_err(|errors| errors.join("\n"))?;
    Ok(Opened {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        hash,
        path: path.to_path_buf(),
        map: map.to_string(),
        scenario: compiled.scenario,
        units: compiled.units,
        items: compiled.items,
        pictures: compiled.pictures,
        story,
    })
}

impl Opened {
    /// The reference a save names this map by.
    pub fn origin(&self) -> ScenarioRef {
        ScenarioRef::Pack { id: self.id.clone(), version: self.version.clone(), hash: self.hash, path: self.path.display().to_string(), map: self.map.clone() }
    }

    /// Starts the game of this map: its story is attached and its `game_start` moment raised (the
    /// events wait in `Game::drain_events`, as the scenario's opening events do).
    pub fn start(&self, g: &mut Game) {
        g.set_origin(self.origin());
        g.attach_story(self.story.clone());
        g.start_story();
    }

    /// The package's name for a person: its id and version.
    pub fn name(&self) -> String {
        format!("{} {}", self.id, self.version)
    }
}

/// Test support: a package with a story on one map, opened on the built-in demo content (no
/// install), for the save and the story tests.
#[cfg(test)]
pub(crate) mod test_support {
    use super::*;
    use crate::pack::compile::compile_with;
    use crate::pack::compile::Base;
    use crate::pack::terrain::{encode_rgb, DEFAULT_PALETTE};
    use crate::rules::content::Content;

    /// The package `story-test` with one map `m`: a 50×50 meadow, a knight hero, a bandit army and
    /// a story. At the start a dialog «intro» (take, refuse); «take» sets `dread` to 2 and gives 50
    /// gold; each week with `dread` at 2 or more spawns the host `host` (militiamen, stand-ins for the dead) at (10, 10), once;
    /// beating the host is a victory.
    pub fn story_package() -> Package {
        let manifest = "id = \"story-test\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"story.title\"\nauthor = \"Razdor\"\nlicense = \"MIT\"\n";
        let map = "title = \"map.title\"\nstart_date = \"1300-04-01\"\nrelations = [[3,3,0,-3],[3,3,0,-3],[0,0,3,-3],[-3,-3,-3,3]]\n\n[hero]\ncell = [5, 5]\nclasses = [\"knight\"]\ngold = 100\ntroops = [{ unit = \"dt:militiaman\", level = 1, count = 3 }]\n\n[[army]]\nid = \"e\"\nname = \"army.e\"\ncell = [30, 30]\nfaction = \"enemy\"\nmodel = \"bandits\"\ntroops = [{ unit = \"dt:bandit\", level = 1, count = 2 }]\n\n[victory]\nkind = \"reach\"\ncell = [45, 45]\nmessage = \"win.message\"\n";
        let triggers = "[vars]\ndread = 0\n\n[[trigger]]\nid = \"start\"\nwhen = { event = \"game_start\" }\nthen = [ { dialog = \"intro\" } ]\n\n[[trigger]]\nid = \"take\"\nwhen = { event = \"dialog_choice\", dialog = \"intro\", choice = \"take\" }\nthen = [ { set = \"dread\", value = 2 }, { give_gold = 50 } ]\n\n[[trigger]]\nid = \"host\"\nwhen = { event = \"week_start\" }\nif = [ { var = \"dread\", op = \">=\", value = 2 } ]\nthen = [ { spawn_army = \"host\", at = [10, 10], faction = \"enemy\", model = 11, troops = [\"dt:militiaman\"] } ]\n\n[[trigger]]\nid = \"won\"\nwhen = { event = \"army_defeated\", army = \"host\" }\nthen = [ { victory = true } ]\n";
        let dialog = "[[dialog]]\nid = \"intro\"\ntitle = \"intro.title\"\ntext = \"intro.text\"\n\n[[dialog.choice]]\nid = \"take\"\ntext = \"intro.take\"\n\n[[dialog.choice]]\nid = \"refuse\"\ntext = \"intro.refuse\"\n";
        let texts = "[story]\ntitle = \"Story test\"\n\n[map]\ntitle = \"Map\"\n\n[intro]\ntitle = \"Intro\"\ntext = \"A rider waits.\"\ntake = \"Take the purse\"\nrefuse = \"Refuse\"\n\n[win]\nmessage = \"Won\"\n\n[army]\ne = \"Bandits\"\n";
        let mut f = Files::default();
        f.insert("package.toml", manifest);
        f.insert("texts/en.toml", texts);
        f.insert("maps/m/map.toml", map);
        f.insert("maps/m/terrain.png", encode_rgb(50, 50, |_, _| DEFAULT_PALETTE[6].1));
        f.insert("triggers/main.toml", triggers);
        f.insert("dialogs/intro.toml", dialog);
        Package::parse(f).0
    }

    /// The package's map `m` opened on the demo content.
    pub fn opened(pkg: &Package) -> Opened {
        let content = Content::builtin();
        let picture = |_: u32| -> Option<usize> { None };
        let base = Base { units: &content.units, items: &content.items, picture: &picture };
        let compiled = compile_with(pkg, &base, "en", "m").unwrap_or_else(|r| panic!("{}", r.to_text()));
        build(pkg, package_hash(&pkg.files), Path::new("story-test"), "m", compiled, "en").unwrap_or_else(|e| panic!("{e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pack::files::Files;

    #[test]
    fn the_hash_follows_the_content_not_the_order() {
        let mut a = Files::default();
        a.insert("package.toml", "x");
        a.insert("texts/en.toml", "y");
        let mut b = Files::default();
        b.insert("texts/en.toml", "y");
        b.insert("package.toml", "x");
        assert_eq!(package_hash(&a), package_hash(&b));
        b.insert("package.toml", "z");
        assert_ne!(package_hash(&a), package_hash(&b));
        assert_ne!(story_seed(7), 0);
    }

    use crate::dt::install::DtInstall;
    use crate::pack::triggers::Place;
    use crate::rules::content::{Content, HeroClass};
    use crate::rules::game::Event;
    use crate::rules::script::ScriptEnd;
    use crate::rules::triggers::{Ending, Trace, When};
    use std::sync::Arc;

    /// Chapter 1 «Брод» of the reference package, on the install (skipped without `RAZDOR_DT_DIR`).
    /// The moments are raised the way the hero's steps and the battles raise them: `story_events`
    /// for a step onto a cell, `story_battle_end` for a battle the army lost (its `beaten_armies`
    /// and its place on the map, as `resolve_battle` leaves them), and `pass_time` for the weeks.
    fn chapter() -> Option<(Game, Opened, DtInstall)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let install = DtInstall::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{e}"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let opened = open(&root, Some("brod"), "en", &install).unwrap_or_else(|e| panic!("{e}"));
        let content = Arc::new(Content::from_dt(&install).with_package(opened.units.clone(), opened.items.clone()));
        let mut g = Game::from_scenario(content, &opened.scenario, HeroClass::Knight);
        g.triggers.trace = Some(Vec::new());
        opened.start(&mut g);
        Some((g, opened, install))
    }

    /// The dialogs among `events`, by id.
    fn dialogs(events: Vec<Event>) -> Vec<String> {
        events.into_iter().filter_map(|e| if let Event::Package(d) = e { Some(d.id) } else { None }).collect()
    }

    /// The battle's end for army `id`: beaten, and gone from the map.
    fn beat(g: &mut Game, id: u8) -> Vec<Event> {
        let before = g.story_battle_snapshot().expect("a story is attached");
        g.beaten_armies.insert(id);
        g.world.armies.retain(|a| a.id != id);
        g.story_battle_end(before)
    }

    /// The triggers that fired so far in the game's trace.
    fn fired(g: &Game) -> Vec<String> {
        g.triggers.trace.as_ref().map_or_else(Vec::new, |log| log.iter().filter_map(|(_, t)| if let Trace::Fired { trigger, .. } = t { Some(trigger.clone()) } else { None }).collect())
    }

    /// Path A: Varga joins, the toll is fought, the marsh folk's wraiths are beaten, the tower is
    /// taken, the dread rises to the host, and the host is beaten with the mill spared.
    fn path_a(g: &mut Game) {
        assert_eq!(dialogs(g.drain_events()), ["arrival"]);
        g.answer_story_dialog("arrival", None);
        assert_eq!(dialogs(g.story_events(When::HeroEnters(Place::Cell(33, 19)))), ["varga_meet"], "the ruins by the road");
        g.answer_story_dialog("varga_meet", Some("join"));
        assert!(g.squad.iter().any(|u| u.named == 200), "Varga joins the hero's army");
        let gold = g.gold;
        assert_eq!(dialogs(g.story_events(When::HeroEnters(Place::Cell(35, 23)))), ["toll"], "the toll, as the hero nears the bandits");
        g.answer_story_dialog("toll", Some("fight"));
        beat(g, 1);
        assert_eq!(g.gold, gold + 25, "the toll's bandits beaten: 25 gold");
        assert_eq!(dialogs(g.story_events(When::HeroEnters(Place::Cell(6, 40)))), ["marsh_plea"], "the marsh folk ask for help");
        g.answer_story_dialog("marsh_plea", Some("help"));
        let thanks = beat(g, 2);
        assert_eq!(dialogs(thanks), ["marsh_thanks"], "Tess is named");
        assert!(g.squad.iter().any(|u| u.named == 200 && u.items.contains(&Some(crate::rules::content::ItemId(200)))), "the charm goes to Varga");
        assert_eq!(g.story_var("marsh_trust"), Some(crate::pack::triggers::VarValue::Int(2)));
        assert_eq!(dialogs(g.story_events(When::BuildingCaptured(2))), ["stones"], "the watchtower taken");
        let mut out = Vec::new();
        g.pass_time(14.0 * 24.0 * 60.0, &mut out);
        assert_eq!(g.story_var("dread"), Some(crate::pack::triggers::VarValue::Int(2)), "two weeks of dread");
        assert!(dialogs(out).contains(&"host_rises".to_string()), "the host rises at the ford");
        assert!(g.triggers.spawned.contains(&200), "the host is on the map");
        let ended = beat(g, 200);
        assert_eq!(dialogs(ended), ["end_mercy"], "the mill was spared: the mercy ending");
    }

    #[test]
    fn chapter_one_is_won_through_the_api() {
        let Some((mut g, opened, _)) = chapter() else { return };
        path_a(&mut g);
        assert_eq!(g.story_end(), Some(Ending::Victory));
        assert_eq!(g.script_end(), Some(ScriptEnd::Package(Ending::Victory)));
        assert_eq!(g.story_var("mill_burned"), Some(crate::pack::triggers::VarValue::Bool(false)));
        assert_eq!(g.story_var("varga_joined"), Some(crate::pack::triggers::VarValue::Bool(true)));
        assert_eq!(g.story_var("toll_settled"), Some(crate::pack::triggers::VarValue::Bool(true)));
        assert_eq!(g.story_var("tower_taken"), Some(crate::pack::triggers::VarValue::Bool(true)));
        assert!(!opened.story.triggers.is_empty());
    }

    /// Every trigger of the chapter fires on some path: A above, B (the mill burned, the toll talked
    /// away, the marsh folk ridden past, the host beaten: the fire ending), C (Varga sent away, the
    /// toll paid, Varga absent from the fight, the new moon's omens, the mercy ending).
    #[test]
    fn every_trigger_of_chapter_one_fires_on_some_path() {
        let Some((mut a, opened, install)) = chapter() else { return };
        path_a(&mut a);
        let mut all = fired(&a);

        let content = Arc::new(Content::from_dt(&install).with_package(opened.units.clone(), opened.items.clone()));
        let mut b = Game::from_scenario(content.clone(), &opened.scenario, HeroClass::Knight);
        b.triggers.trace = Some(Vec::new());
        opened.start(&mut b);
        assert_eq!(dialogs(b.drain_events()), ["arrival"]);
        b.answer_story_dialog("arrival", None);
        assert_eq!(dialogs(b.story_events(When::HeroEnters(Place::Cell(33, 19)))), ["varga_meet"]);
        b.answer_story_dialog("varga_meet", Some("burn"));
        assert_eq!(b.story_var("dread"), Some(crate::pack::triggers::VarValue::Int(2)), "the burnt mill: +2 dread");
        assert_eq!(dialogs(b.story_events(When::HeroEnters(Place::Cell(35, 23)))), ["toll"]);
        b.answer_story_dialog("toll", Some("talk"));
        assert_eq!(dialogs(b.story_events(When::HeroEnters(Place::Cell(6, 40)))), ["marsh_plea"]);
        b.answer_story_dialog("marsh_plea", Some("ride_on"));
        assert_eq!(b.story_var("marsh_trust"), Some(crate::pack::triggers::VarValue::Int(-1)));
        let mut out = Vec::new();
        b.pass_time(7.0 * 24.0 * 60.0, &mut out);
        assert!(b.triggers.spawned.contains(&200), "the host rises at the first week with dread 2 or more");
        assert_eq!(dialogs(beat(&mut b, 200)), ["end_fire"], "the mill burned: the fire ending");
        assert_eq!(b.story_end(), Some(Ending::Victory));
        all.extend(fired(&b));

        let mut c = Game::from_scenario(content, &opened.scenario, HeroClass::Knight);
        c.triggers.trace = Some(Vec::new());
        opened.start(&mut c);
        assert_eq!(dialogs(c.drain_events()), ["arrival"]);
        c.answer_story_dialog("arrival", None);
        assert_eq!(dialogs(c.story_events(When::HeroEnters(Place::Cell(33, 19)))), ["varga_meet"]);
        c.answer_story_dialog("varga_meet", Some("refuse"));
        assert_eq!(dialogs(c.story_events(When::HeroEnters(Place::Cell(35, 23)))), ["toll"]);
        let gold = c.gold;
        c.answer_story_dialog("toll", Some("pay"));
        assert_eq!(c.gold, gold - 20, "the toll paid");
        assert_eq!(dialogs(c.story_events(When::HeroEnters(Place::Cell(6, 40)))), ["marsh_plea"]);
        c.answer_story_dialog("marsh_plea", Some("help"));
        assert_eq!(dialogs(beat(&mut c, 2)), ["marsh_thanks"], "Varga is not in the fight: the charm goes to the pack");
        let mut omens = Vec::new();
        for _ in 0..60 {
            omens.extend(c.story_events(When::NewMoon));
        }
        assert!(c.story_var("omen").is_some_and(|v| v != crate::pack::triggers::VarValue::Int(0)), "the new moons rolled an omen");
        all.extend(fired(&c));

        let mut expected: Vec<String> = opened.story.triggers.iter().map(|t| t.id.clone()).collect();
        expected.sort();
        expected.dedup();
        all.sort();
        all.dedup();
        assert_eq!(all, expected, "every trigger of the chapter is reachable");
    }
}
