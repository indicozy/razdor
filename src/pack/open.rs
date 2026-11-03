//! Opening a package's map as a game: the package read from its folder or `.rzpack`, checked, its
//! map compiled on the install, its story compiled for that map, and the package's hash. The game
//! (`ui::pack_play`, `rules::save`) plays the result; a save names the map by [`Opened::origin`] and
//! finds the package again among the packages the player opened (`trust`), by its hash, never from
//! the save's path as it is.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::campaign::{self, Campaign};
use super::check::check;
use super::compile::{self, Compiled};
use super::files::Files;
use super::load::Package;
use crate::dt::data::{ArtefactDef, UnitDef};
use crate::dt::dtm::Scenario;
use crate::dt::install::DtInstall;
use crate::rules::campaign::{CampaignState, Route};
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
    /// The package's campaign (`campaign.toml`), if it has one: the whole graph, not this map only.
    pub campaign: Option<Arc<Campaign>>,
    /// The routes out of this map (its campaign `next` entries, their conditions compiled), in
    /// order. Empty outside a campaign.
    pub routes: Vec<Route>,
    /// This map's points that have an id: a campaign's `travel` enters at one of them.
    pub points: BTreeMap<String, (i32, i32)>,
    /// The campaign's name: its title text in the package's language (the key when the text is
    /// missing, which `check` reports).
    pub campaign_title: Option<String>,
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
        None => match package.campaign.as_ref() {
            Some(c) => c.start.clone(),
            None => package.maps.keys().next().cloned().ok_or("the package has no map (maps/<id>/map.toml)")?,
        },
    };
    let compiled = compile::compile(&package, install, lang, &map_id).map_err(|r| r.to_text())?;
    build(&package, hash, path, &map_id, compiled, lang)
}

/// The package's scripts (`scripts/<name>.lua`) by path. A file that is not UTF-8 text is left out
/// (`check` reports it).
pub fn package_scripts(files: &Files) -> BTreeMap<String, String> {
    files
        .paths()
        .filter(|p| p.starts_with("scripts/") && p.ends_with(".lua"))
        .filter_map(|p| files.get(p).and_then(|b| std::str::from_utf8(b).ok()).map(|t| (p.to_string(), t.to_string())))
        .collect()
}

/// The opened map from a compiled map of `package`: its story is compiled against the map's ids.
/// `hash` is the package's [`package_hash`]. The tests open packages this way without an install.
pub fn build(package: &Package, hash: u64, path: &Path, map: &str, compiled: Compiled, lang: &str) -> Result<Opened, String> {
    let manifest = package.manifest.as_ref().ok_or("the package has no package.toml")?;
    let mut story = triggers::compile(package, lang, map, &compiled.names, story_seed(hash)).map_err(|errors| errors.join("\n"))?;
    story.battles = compiled.names.battles.clone();
    story.scripts = Some(Arc::new(package_scripts(&package.files)));
    let routes = campaign::routes(package, map).map_err(|errors| errors.join("\n"))?;
    let points = campaign::points_of(package, map).into_iter().collect();
    let texts = package.texts.get(lang).or_else(|| package.texts.values().next());
    let campaign_title = package.campaign.as_ref().and_then(|c| c.title.as_ref()).map(|key| texts.and_then(|t| t.get(key)).cloned().unwrap_or_else(|| key.clone()));
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
        campaign: package.campaign.clone().map(Arc::new),
        routes,
        points,
        campaign_title,
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
        if self.campaign.is_some() {
            g.campaign = Some(Box::new(CampaignState::start(&self.map)));
        }
        g.attach_story(self.story.clone());
        g.start_story();
    }

    /// Whether this map is a campaign's revisit map: leaving it keeps its state (`rules::campaign`).
    pub fn revisit(&self) -> bool {
        self.campaign.as_ref().and_then(|c| c.map(&self.map)).is_some_and(|m| m.revisit)
    }

    /// Whether this is the package a save names: the same id, version, content hash and map. The
    /// path does not count: a save's path is a hint, and the package is found again by its hash
    /// (`trust`).
    pub fn same_package(&self, origin: &ScenarioRef) -> bool {
        matches!(origin, ScenarioRef::Pack { id, version, hash, map, .. } if *id == self.id && *version == self.version && *hash == self.hash && *map == self.map)
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

    /// The story seed the chapter tests play with. A package's own seed comes from its hash (every
    /// file), so any content edit would re-roll chapter 1's dread and omens; the tests pin this one
    /// (the value the package had when they were written), so only a change to the story itself can
    /// move them.
    const CHAPTER_ONE_SEED: u32 = 2517464231;

    /// Chapter 1 «Брод» of the reference package, on the install (skipped without `RAZDOR_DT_DIR`).
    /// The moments are raised the way the hero's steps and the battles raise them: `story_events`
    /// for a step onto a cell, `story_battle_end` for a battle the army lost (its `beaten_armies`
    /// and its place on the map, as `resolve_battle` leaves them), and `pass_time` for the weeks.
    fn chapter() -> Option<(Game, Opened, DtInstall)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let install = DtInstall::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{e}"));
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let mut opened = open(&root, Some("brod"), "en", &install).unwrap_or_else(|e| panic!("{e}"));
        opened.story.seed = CHAPTER_ONE_SEED;
        let content = Arc::new(Content::from_dt(&install).with_package(opened.units.clone(), opened.items.clone()));
        let mut g = Game::from_scenario(content, &opened.scenario, HeroClass::Knight);
        g.triggers.trace = Some(Vec::new());
        opened.start(&mut g);
        Some((g, opened, install))
    }

    /// Varga's id: the package numbers its named characters by first mention (Ilm is first, in
    /// chapter 3а's triggers), so she is found by her name.
    fn varga_id(g: &Game) -> u8 {
        let story = g.story().expect("the story is attached");
        crate::rules::triggers::PACKAGE_BASE + story.named.iter().position(|n| n == "Varga").expect("Varga is a named character") as u8
    }

    /// The id of the package army `host` (armies are numbered by their first spawn: chapter 1's is
    /// not the first in file order).
    fn host_id(g: &Game) -> u8 {
        let story = g.story().expect("the story is attached");
        story.army_names.iter().find(|(_, n)| n.as_str() == "host").map(|(id, _)| *id).expect("the host is a package army")
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

    /// Whether the dialog `id` is among `events`.
    fn said(events: Vec<Event>, id: &str) -> bool {
        dialogs(events).iter().any(|d| d == id)
    }

    /// The id of the package army called `name` (a spawned army is numbered by its spawn).
    fn army_named(g: &Game, name: &str) -> u8 {
        let story = g.story().expect("the story is attached");
        story.army_names.iter().find(|(_, n)| n.as_str() == name).map(|(id, _)| *id).unwrap_or_else(|| panic!("no army {name}"))
    }

    /// The scenario id of the building called `name` on the map.
    fn building_named(g: &Game, name: &str) -> u16 {
        let story = g.story().expect("the story is attached");
        story.building_names.iter().find(|(_, n)| n.as_str() == name).map(|(id, _)| *id).unwrap_or_else(|| panic!("no building {name}"))
    }

    /// A new game of the chapter on `opened`, its arrival dialog answered, the trace on.
    fn fresh(opened: &Opened, install: &DtInstall) -> Game {
        let content = Arc::new(Content::from_dt(install).with_package(opened.units.clone(), opened.items.clone()));
        let mut g = Game::from_scenario(content, &opened.scenario, HeroClass::Knight);
        g.triggers.trace = Some(Vec::new());
        opened.start(&mut g);
        assert_eq!(dialogs(g.drain_events()), ["arrival"]);
        g.answer_story_dialog("arrival", None);
        g
    }

    /// Raises the arrival of raid army `name` in building `building` (the AI's walk is not run here: the
    /// moment is raised as the hero's steps and battles are, see `chapter`).
    fn arrive(g: &mut Game, name: &str, building: u16) -> Vec<Event> {
        let army = army_named(g, name);
        g.story_events(When::ArmyArrives { army, at: None, building: Some(building) })
    }

    /// Steps the hero onto `cell` (raising `hero_enters`) and answers the dialogs that opens, and those they
    /// open in turn: the listed choice, else the dialog's first one («Далее» when it has none).
    fn step(g: &mut Game, cell: (i32, i32), choices: &[(&str, &str)]) -> Vec<String> {
        let events = g.story_events(When::HeroEnters(Place::Cell(cell.0, cell.1)));
        answer_all(g, events, choices)
    }

    /// Answers the dialogs among `events` (and those they open), as [`step`] does.
    fn answer_all(g: &mut Game, events: Vec<Event>, choices: &[(&str, &str)]) -> Vec<String> {
        let mut queue: std::collections::VecDeque<Event> = events.into();
        let mut out = Vec::new();
        while let Some(e) = queue.pop_front() {
            if let Event::Package(d) = e {
                let choice = choices.iter().find(|(id, _)| *id == d.id).map(|(_, c)| c.to_string()).or_else(|| d.choices.first().cloned());
                out.push(d.id.clone());
                queue.extend(g.answer_story_dialog(&d.id, choice.as_deref()));
            }
        }
        out
    }

    /// The arrival of a chapter game: its opening dialog answered.
    fn start(g: &mut Game) {
        assert_eq!(dialogs(g.drain_events()), ["arrival"]);
        g.answer_story_dialog("arrival", None);
    }

    /// Path A: the mill spared. The hero takes the watchtower and walks the roads (every beat of the route is
    /// reached), Varga joins, the toll is fought, the marsh folk are helped and the wraiths beaten; the raids
    /// come to the tower on Dread 2 and 4, and the host rises at Dread 5 (the weekly backstop) and is beaten at
    /// the ford: the mercy ending.
    fn path_a(g: &mut Game) {
        start(g);
        assert!(said(g.story_events(When::BuildingCaptured(2)), "stones"), "the watchtower taken");
        assert!(step(g, (37, 23), &[]).contains(&"deserter_note".to_string()), "the hanged man's note");
        assert!(step(g, (41, 21), &[]).contains(&"farm".to_string()), "the farmstead");
        assert!(step(g, (37, 12), &[]).contains(&"scout".to_string()), "the scout's letter");
        assert!(step(g, (34, 17), &[("varga_meet", "join")]).contains(&"varga_meet".to_string()), "the ruins by the road");
        assert!(g.squad.iter().any(|u| u.named == varga_id(g)), "Varga joins the hero's army");
        assert!(step(g, (33, 23), &[("toll", "fight")]).contains(&"toll".to_string()), "the toll");
        beat(g, 1);
        for cell in [(34, 23), (37, 19), (34, 8), (28, 8)] {
            assert_eq!(step(g, cell, &[]).len(), 1, "a beat on the road at {cell:?}");
        }
        assert!(step(g, (19, 5), &[("deserters", "hand")]).contains(&"deserters".to_string()), "the deserters in the north woods");
        assert!(step(g, (14, 10), &[]).contains(&"grave_marker".to_string()), "the grave marker");
        assert!(step(g, (12, 16), &[]).contains(&"cairn".to_string()), "the sign on the road");
        assert!(step(g, (11, 25), &[("refugees", "take")]).contains(&"refugees".to_string()), "the overturned cart");
        let reedmere = building_named(g, "reedmere");
        assert!(said(g.story_events(When::HeroEntersBuilding(reedmere)), "refugees_thanks"), "the family thanked at Reedmere");
        assert!(step(g, (13, 31), &[("herb", "buy")]).contains(&"herb".to_string()), "the herb-wife");
        assert!(step(g, (21, 23), &[]).contains(&"ferry".to_string()), "the sunken boat");
        assert!(step(g, (6, 40), &[("marsh_plea", "help")]).contains(&"marsh_plea".to_string()), "the marsh folk ask for help");
        assert!(step(g, (6, 43), &[]).contains(&"bog_edge".to_string()), "the bog's edge");
        assert!(dialogs(beat(g, 2)).contains(&"marsh_thanks".to_string()), "Tess is named");
        // Helped +2 and the family brought home +1.
        assert_eq!(g.story_var("marsh_trust"), Some(crate::pack::triggers::VarValue::Int(3)));
        let mut out = Vec::new();
        g.pass_time(14.0 * 24.0 * 60.0, &mut out);
        assert!(dialogs(out).contains(&"host_rises".to_string()), "the host rises at the ford");
        let ended = beat(g, host_id(g));
        assert_eq!(dialogs(ended), ["end_mercy", "rider"], "the mill was spared: the mercy ending, and the rider");
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
        assert!(!opened.story.triggers.is_empty());
    }

    /// Path B: the mill burned (Varga's ruins, Dread 2: the raid goes on Reedmere), the toll talked away, the
    /// deserters served, the barrow and the refugees left alone, the marsh folk ridden past. The raids burn
    /// Reedmere on Dread 2 and 4, the host rises at Dread 5 and is beaten: the fire ending.
    fn path_b(g: &mut Game) {
        assert!(step(g, (34, 17), &[("varga_meet", "burn")]).contains(&"varga_meet".to_string()), "the burnt ruins: Dread +2");
        assert_eq!(g.story_var("mill_burned"), Some(crate::pack::triggers::VarValue::Bool(true)));
        assert!(step(g, (33, 23), &[("toll", "talk")]).contains(&"toll".to_string()), "the toll talked away");
        assert!(step(g, (19, 5), &[("deserters", "serve")]).contains(&"deserters".to_string()), "the deserters in the north woods");
        assert_eq!(g.story_var("deserters"), Some(crate::pack::triggers::VarValue::Int(1)));
        assert!(step(g, (41, 6), &[("barrow", "leave")]).contains(&"barrow".to_string()), "the barrow, left alone");
        assert!(step(g, (11, 25), &[("refugees", "leave")]).contains(&"refugees".to_string()), "the cart, left");
        assert!(step(g, (6, 40), &[("marsh_plea", "ride_on")]).contains(&"marsh_plea".to_string()), "the marsh folk, ridden past");
        assert_eq!(g.story_var("marsh_trust"), Some(crate::pack::triggers::VarValue::Int(-1)));
        let mut out = Vec::new();
        g.pass_time(7.0 * 24.0 * 60.0, &mut out);
        let raid_2 = army_named(g, "raid_2");
        assert!(g.triggers.spawned.contains(&raid_2), "Dread 4 at the first week's backstop sends raid 2");
        let reedmere = building_named(g, "reedmere");
        // The raids walk on Reedmere while the days pass (the AI's own walk): raid 1 burns it in the week; raid 2
        // is raised by the week's Dread 4 and is raised here at its arrival if the walk has not brought it.
        let _ = arrive(g, "raid_1", reedmere);
        assert!(fired(g).iter().any(|t| t == "raid_1_burns"), "raid 1 burns Reedmere: {:?}", fired(g));
        let _ = arrive(g, "raid_2", reedmere);
        assert!(fired(g).iter().any(|t| t == "raid_2_burns"), "raid 2 burns it again: {:?}", fired(g));
        assert!(g.story_var("villages_burned").is_some_and(|v| v == crate::pack::triggers::VarValue::Int(2)), "the ash counts twice");
        // Dread 5 at the second week's midnight, the host the midnight after it.
        g.pass_time(8.0 * 24.0 * 60.0, &mut out);
        assert!(dialogs(out).contains(&"host_rises".to_string()), "the host rises at Dread 5");
        let ended = beat(g, host_id(g));
        assert_eq!(dialogs(ended), ["end_fire", "rider"], "the mill burned: the fire ending");
        assert_eq!(g.story_end(), Some(Ending::Victory));
    }

    /// Path C: Varga sent away, the toll paid, the herb-wife's salve left, the refugees taken home, the deserters
    /// let go, the barrow sealed with a rite, the wraiths beaten without Varga (the charm goes to the pack),
    /// and the new moons' rolls (the omens). Raid 1 goes on Reedmere (the tower never held) and burns it.
    fn path_c(g: &mut Game) {
        assert!(step(g, (41, 21), &[]).contains(&"farm".to_string()), "the farmstead's coins");
        assert!(step(g, (34, 17), &[("varga_meet", "refuse")]).contains(&"varga_meet".to_string()), "Varga sent away");
        assert!(step(g, (33, 23), &[("toll", "pay")]).contains(&"toll".to_string()), "the toll paid");
        assert_eq!(g.story_var("dread"), Some(crate::pack::triggers::VarValue::Int(2)), "Varga's and the toll's deeds: Dread 2");
        let reedmere = building_named(g, "reedmere");
        let _ = arrive(g, "raid_1", reedmere);
        assert!(fired(g).iter().any(|t| t == "raid_1_burns"), "the first raid burns Reedmere (the tower is not held)");
        assert!(step(g, (11, 25), &[("refugees", "take")]).contains(&"refugees".to_string()), "the family by the road");
        assert!(said(g.story_events(When::HeroEntersBuilding(reedmere)), "refugees_thanks"), "the family is thanked");
        assert!(step(g, (19, 5), &[("deserters", "pass")]).contains(&"deserters".to_string()), "the deserters let go");
        assert!(step(g, (13, 31), &[("herb", "leave")]).contains(&"herb".to_string()), "the herb-wife, left");
        assert!(step(g, (41, 6), &[("barrow", "seal")]).contains(&"barrow".to_string()), "the barrow sealed with a rite");
        assert_eq!(g.story_var("barrow_sealed"), Some(crate::pack::triggers::VarValue::Bool(true)));
        assert!(step(g, (6, 40), &[("marsh_plea", "help")]).contains(&"marsh_plea".to_string()), "the marsh folk ask for help");
        assert!(dialogs(beat(g, 2)).contains(&"marsh_thanks".to_string()), "the wraiths beaten without Varga: the charm goes to the pack");
        let mut omens = Vec::new();
        for _ in 0..60 {
            let events = g.story_events(When::NewMoon);
            omens.extend(answer_all(g, events, &[]));
        }
        assert!(g.story_var("omen").is_some_and(|v| v != crate::pack::triggers::VarValue::Int(0)), "the new moons rolled an omen");
        let mut out = Vec::new();
        g.pass_time(45.0 * 24.0 * 60.0, &mut out);
        assert!(dialogs(out.clone()).contains(&"host_warning".to_string()), "the warning comes at Dread 5");
        let raid_2 = army_named(g, "raid_2");
        assert!(g.triggers.spawned.contains(&raid_2), "the second raid comes at Dread 4");
        let _ = arrive(g, "raid_2", reedmere);
        assert!(fired(g).iter().any(|t| t == "raid_2_burns"), "the second raid burns Reedmere");
        let ended = beat(g, host_id(g));
        assert_eq!(dialogs(ended), ["end_mercy", "rider"], "the mercy ending");
    }

    /// Path D: the tower held and the barrow broken; the first raid beaten at the tower (Dread 2 to 3), the toll
    /// talked away (Dread 4: raid 2), raid 2 beaten (Dread 5: the warning), and the weakened host rises the next
    /// midnight: the mercy ending.
    fn path_d(g: &mut Game) {
        assert!(said(g.story_events(When::BuildingCaptured(2)), "stones"), "the watchtower taken");
        assert!(step(g, (41, 6), &[("barrow", "break")]).contains(&"barrow".to_string()), "the barrow's offer");
        let mound = army_named(g, "barrow_mound");
        assert!(said(beat(g, mound), "barrow_broken"), "the mound broken");
        assert!(step(g, (34, 17), &[("varga_meet", "join")]).contains(&"varga_meet".to_string()), "Varga joins");
        let r1 = army_named(g, "raid_1");
        assert!(said(beat(g, r1), "raid_beaten"), "raid 1 beaten at the tower");
        assert!(step(g, (33, 23), &[("toll", "talk")]).contains(&"toll".to_string()), "the toll talked away");
        let r2 = army_named(g, "raid_2");
        assert!(said(beat(g, r2), "raid_beaten"), "raid 2 beaten");
        assert!(g.story_var("dread").is_some_and(|v| v == crate::pack::triggers::VarValue::Int(5)), "the raids' blood: Dread 5");
        let mut out = Vec::new();
        g.pass_time(24.0 * 60.0, &mut out);
        assert!(dialogs(out).contains(&"host_rises".to_string()), "the host rises the midnight after the warning");
        let host = army_named(g, "host");
        let troops = g.world.armies.iter().find(|a| a.id == host).map(|a| a.troops.len());
        assert_eq!(troops, Some(1), "the host is one unit weaker for the broken mound");
        let ended = beat(g, host);
        assert_eq!(dialogs(ended), ["end_mercy", "rider"], "the mercy ending");
    }

    /// Path E: the tower lost to raid 1 and taken back, raid 2 takes it again: the loss is noticed each time
    /// (`tower_lost`), the raids' arrivals and losses (`raid_*_takes_tower`).
    fn path_e(g: &mut Game) {
        assert!(said(g.story_events(When::BuildingCaptured(2)), "stones"), "the watchtower taken");
        assert!(step(g, (34, 17), &[("varga_meet", "join")]).contains(&"varga_meet".to_string()), "Varga joins: Dread 2, raid 1 for the tower");
        assert!(said(arrive(g, "raid_1", 2), "raid_tower"), "raid 1 takes the tower");
        g.story_events(When::BuildingLost(2));
        assert!(g.story_var("tower_taken") == Some(crate::pack::triggers::VarValue::Bool(false)), "the loss is noticed");
        g.story_events(When::BuildingCaptured(2));
        assert!(g.story_var("tower_taken") == Some(crate::pack::triggers::VarValue::Bool(true)), "the hero takes it back");
        assert!(step(g, (6, 40), &[("marsh_plea", "help")]).contains(&"marsh_plea".to_string()), "the marsh folk");
        assert!(dialogs(beat(g, 2)).contains(&"marsh_thanks".to_string()), "the wraiths beaten: Dread 3");
        assert!(step(g, (33, 23), &[("toll", "talk")]).contains(&"toll".to_string()), "the toll: Dread 4, raid 2");
        assert!(said(arrive(g, "raid_2", 2), "raid_tower"), "raid 2 takes the retaken tower");
        g.story_events(When::BuildingLost(2));
        let trace = fired(g);
        assert!(trace.iter().any(|t| t == "tower_lost") && trace.iter().any(|t| t == "raid_2_takes_tower"), "{trace:?}");
    }

    /// Every trigger of the chapter fires on some path: A to E above.
    #[test]
    fn every_trigger_of_chapter_one_fires_on_some_path() {
        let Some((mut a, opened, install)) = chapter() else { return };
        path_a(&mut a);
        let mut all = fired(&a);
        let mut b = fresh(&opened, &install);
        path_b(&mut b);
        all.extend(fired(&b));
        let mut c = fresh(&opened, &install);
        path_c(&mut c);
        all.extend(fired(&c));
        let mut d = fresh(&opened, &install);
        path_d(&mut d);
        all.extend(fired(&d));
        let mut e = fresh(&opened, &install);
        path_e(&mut e);
        all.extend(fired(&e));

        let mut expected: Vec<String> = opened.story.triggers.iter().map(|t| t.id.clone()).collect();
        expected.sort();
        expected.dedup();
        all.sort();
        all.dedup();
        let missing: Vec<&String> = expected.iter().filter(|t| !all.contains(t)).collect();
        assert!(missing.is_empty(), "triggers that never fire on any path: {missing:?}");
    }
}
