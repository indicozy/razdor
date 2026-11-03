//! `razdor --pack <folder-or-.rzpack> [--map <id>]`, or the `RAZDOR_PACK` and `RAZDOR_PACK_MAP`
//! variables (for the off-screen snapshots): a content package's map as a new game.
//!
//! The package is checked, read and its map compiled on the player's install (`pack::open`). Its
//! map joins the scenario list, the game's content gains the package's units and items
//! (`Content::with_package`), and the pictures of the package's units are those of their base
//! units. The package's story (triggers and dialogs) starts with the game. The game opens on the
//! hero window, as a map of the install does; `ScenarioSelect` lists the package's map too.
//!
//! A package game saves like the others: its save names the package (id, version, hash and the
//! path it was read from) and the map. A load finds the package again among the packages the
//! player opened (`pack::trust`, recorded by [`App::open_pack`]), by its hash, never from the
//! save's path as it is ([`App::ensure_pack`]). A package that is gone or changed refuses the
//! load, naming it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use razdor::pack::{self, compile, trust, Opened};
use razdor::rules::campaign::{after_victory, enter};
use razdor::rules::save::{ScenarioRef, SaveError};
use razdor::{hub, platform};

use super::{hub_here, hub_package_entries, items_view, keeps_listed, package_entry, App, HubPack, Screen, ScenarioEntry};
use super::new_game;

/// The hero class of a snapshot run of a package's map (`RAZDOR_PACK_CLASS`), if any.
pub fn snapshot_class() -> Option<razdor::rules::content::HeroClass> {
    use razdor::rules::content::HeroClass;
    match std::env::var("RAZDOR_PACK_CLASS").ok()?.trim().to_ascii_lowercase().as_str() {
        "knight" => Some(HeroClass::Knight),
        "archmage" => Some(HeroClass::Archmage),
        "ranger" => Some(HeroClass::Ranger),
        _ => None,
    }
}

/// The story variables a snapshot run presets (`RAZDOR_PACK_VARS=name=value,…`, numbers or
/// true/false), as the sim's `--set` does before the story starts.
fn snapshot_vars() -> Vec<(String, razdor::pack::triggers::VarValue)> {
    std::env::var("RAZDOR_PACK_VARS")
        .unwrap_or_default()
        .split(',')
        .filter_map(|spec| razdor::pack::sim::parse_set(spec.trim()).ok())
        .collect()
}

/// The package's map the run plays: the opened package (`pack::open`).
pub struct PackPlay {
    pub opened: Arc<Opened>,
}

/// The package and map named on the command line (`--pack <path> [--map <id>]`), else by
/// `RAZDOR_PACK` and `RAZDOR_PACK_MAP`.
pub fn requested() -> Option<(PathBuf, Option<String>)> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--pack") {
        let path = PathBuf::from(args.get(i + 1)?);
        let map = args.iter().position(|a| a == "--map").and_then(|j| args.get(j + 1).cloned());
        return Some((path, map));
    }
    let path = std::env::var_os("RAZDOR_PACK").filter(|p| !p.is_empty())?;
    let map = std::env::var("RAZDOR_PACK_MAP").ok().filter(|m| !m.is_empty());
    Some((PathBuf::from(path), map))
}

impl App {
    /// Opens `map` of the package at `path` (the first map when none is named) as a new game: the
    /// hero window comes up. An error names what is wrong; nothing changes then. The package
    /// is then remembered (`trust`): a save of it finds it again by its hash, from this path.
    pub fn open_pack(&mut self, path: &Path, map: Option<&str>) -> Result<(), String> {
        let index = self.adopt_package(path, map)?;
        // A snapshot run (`RAZDOR_PACK_CLASS=knight|archmage|ranger`): the map starts with that
        // hero, past the hero window, as its «Start» does (the off-screen shots of a package's maps).
        if let Some(hero) = snapshot_class() {
            let content = self.dt_content.clone().ok_or("no install content")?;
            let content = std::sync::Arc::new(content.with_experimental(super::new_game::experimental_rules()));
            // The hero has no name typed in: the window's default (its «no name» text). The story's
            // variables are preset first (`RAZDOR_PACK_VARS=name=value,…`, as the sim's `--set`).
            let name = razdor::i18n::tr("(no name)").to_string();
            let mut game = razdor::rules::game::Game::from_scenario(content.clone(), &self.scenarios[index].scenario, hero);
            for (var, value) in snapshot_vars() {
                game.triggers.vars.insert(var, value);
            }
            if let Some(p) = self.scenarios[index].pack.clone() {
                p.start(&mut game);
            }
            game.set_hero_name(&name);
            // A disguised hero (`salt_disguise`, as the salt trigger's `hero_figure`) rides as a bandit.
            if matches!(game.triggers.vars.get("salt_disguise"), Some(razdor::pack::triggers::VarValue::Bool(true))) {
                game.hero_figure = razdor::pack::triggers::model_code("bandits").ok();
            }
            // `RAZDOR_PACK_NODIALOG=1`: the story's opening events are dropped, so no dialog is up (the 2.5D
            // view is drawn only with no dialog open). `RAZDOR_PACK_DIALOG=<id>`: that dialog is open, with
            // all its choices (a shot of a dialog the story reaches later, e.g. the council).
            if std::env::var("RAZDOR_PACK_NODIALOG").is_ok_and(|v| v == "1") {
                let _ = game.drain_events();
            }
            if let Ok(id) = std::env::var("RAZDOR_PACK_DIALOG") {
                if let Some(def) = game.story_dialog(&id) {
                    let choices = def.choices.iter().map(|c| c.id.clone()).collect();
                    let pd = razdor::rules::triggers::PendingDialog { id: id.clone(), choices };
                    self.dialogs.clear();
                    self.dialogs.push_back(super::story::package_dialog(&game, &pd));
                }
            }
            // `RAZDOR_PACK_REVEAL=1`: the map is revealed (the cheat «reveal»), to judge its layout in a shot.
            if std::env::var("RAZDOR_PACK_REVEAL").is_ok_and(|v| v == "1") {
                let _ = razdor::rules::cheats::run(&razdor::rules::cheats::Cheat::Reveal, Some(&mut game), None);
            }
            self.game = Some(game);
            self.map_view.reset();
            self.screen = Screen::WorldMap;
            return Ok(());
        }
        self.screen = Screen::ClassSelect { scenario: Some(index) };
        Ok(())
    }

    /// Opens the package at `path` (its `map`, else its first) and makes it the package the game plays
    /// (`adopt_pack`), remembered (`trust`). Returns the index of its map's entry in the list.
    fn adopt_package(&mut self, path: &Path, map: Option<&str>) -> Result<usize, String> {
        let path = trust::canonical_local(path)?;
        let opened = {
            let install = self.assets.dt.as_ref().map(|d| &d.install).ok_or("no Discord Times install (set RAZDOR_DT_DIR)")?;
            pack::open::open(&path, map, pack::open::lang(), install)?
        };
        if let Some(dir) = platform::app_dir() {
            // Without the record a save of this package cannot find it again: the game still
            // plays, and such a save names the package as missing.
            if let Err(e) = trust::remember(&dir, trust::Entry::from_opened(&opened)) {
                razdor::diag!("packages: {e}");
            }
        }
        let id = opened.id.clone();
        self.adopt_pack(opened);
        self.scenarios.iter().position(|e| e.pack.as_ref().is_some_and(|p| p.id == id)).ok_or_else(|| "the package's map is not in the list".to_string())
    }

    /// The new-game list reads the Hub's installed packages again (it opens with the list, so a package
    /// installed, updated or played since shows as it is now). The open package keeps its entry.
    pub(super) fn relist_packages(&mut self) {
        let fresh = hub_package_entries(self.assets.dt.as_ref().map(|d| &d.install));
        self.scenarios.retain(|e| e.pack.is_none());
        self.scenarios.extend(fresh);
        self.keep_pack_map();
    }

    /// The version picked on a package's page in the new-game list: remembered (`set_picked`) and the
    /// package opened on it, so its units are the game's; the list stays and picks the package. A
    /// package that does not open is named in a notice, and the list keeps the version it had.
    pub(super) fn choose_version(&mut self, id: &str, version: &str) {
        let Some(hub) = hub_here() else { return };
        if let Err(e) = hub.set_picked(id, version) {
            razdor::diag!("packages: {e}");
        }
        let opened = hub.play_path_of(id, version).ok_or_else(|| format!("{id} {version}: not installed")).and_then(|path| self.adopt_package(&path, None));
        match opened {
            Ok(_) => new_game::pick_pack(id),
            Err(e) => {
                razdor::diag!("packages: {e}");
                super::map_import::notice(razdor::i18n::tr("The package cannot be played"), vec![e]);
            }
        }
    }

    /// The list's «Next» (or Enter) on a package's entry: the package is opened on the version the entry
    /// shows (`adopt_package`), so its units and pictures are the game's, and the hero window comes up
    /// on its map. Any other choice passes on as it is. A package that does not open stays on the list.
    pub(super) fn open_listed(&mut self, next: Option<Screen>) -> Option<Screen> {
        let Some(Screen::ClassSelect { scenario: Some(i) }) = next else { return next };
        let Some(hp) = self.scenarios.get(i).and_then(|e| e.hub_pack.clone()) else { return next };
        let opened = hub_here()
            .and_then(|hub| hub.play_path_of(&hp.id, &hp.version))
            .ok_or_else(|| format!("{} {}: not installed", hp.id, hp.version))
            .and_then(|path| self.adopt_package(&path, None));
        match opened {
            Ok(index) => Some(Screen::ClassSelect { scenario: Some(index) }),
            Err(e) => {
                razdor::diag!("packages: {e}");
                super::map_import::notice(razdor::i18n::tr("The package cannot be played"), vec![e]);
                None
            }
        }
    }

    /// Makes `opened` the package the game plays: its pictures join the install's, its units and
    /// items the content, and its map the scenario list (in place of the package's last map).
    fn adopt_pack(&mut self, opened: Opened) {
        if let Some(dt) = self.assets.dt.as_mut() {
            // The base game's pictures stay; the package's are those of the last package opened.
            dt.install.unit_pictures.retain(|&(id, _)| id < compile::UNIT_BASE);
            dt.install.unit_pictures.extend(opened.pictures.iter().copied());
            dt.install.unit_pictures.sort_unstable();
        }
        self.pack = Some(PackPlay { opened: Arc::new(opened) });
        self.keep_pack_map();
        self.dt_content = self.install_content();
    }

    /// A save names a package game's package: it is open as the game plays. The package is found
    /// only among the packages the player opened (`trust`) and the Hub's, by the save's id,
    /// version and hash; the save's path is never opened as it is. A package that cannot be found
    /// or does not match is the error [`SaveError::PackageMissing`] gives, and the game keeps the
    /// package it had.
    pub(super) fn ensure_pack(&mut self, origin: &ScenarioRef) -> Result<(), String> {
        let ScenarioRef::Pack { id, version, map, .. } = origin else { return Ok(()) };
        if self.pack.as_ref().is_some_and(|p| p.opened.same_package(origin)) {
            return Ok(());
        }
        let install = self.assets.dt.as_ref().map(|d| &d.install).ok_or_else(|| SaveError::NoInstall.to_string())?;
        let missing = || SaveError::PackageMissing(format!("{id} {version}")).to_string();
        let list = platform::app_dir().map(|dir| trust::read(&dir)).unwrap_or_default();
        let hub_packages = hub::local_dir().map(|local| trust::hub_packages(&local)).unwrap_or_default();
        let opened = trust::candidates(&list, origin, &hub_packages)
            .into_iter()
            .find_map(|path| pack::open::open(&path, Some(map.as_str()), pack::open::lang(), install).ok().filter(|o| o.same_package(origin)))
            .ok_or_else(missing)?;
        self.adopt_pack(opened);
        Ok(())
    }

    /// The content the game plays on: the install's, with the package's units and items added.
    pub(super) fn install_content(&self) -> Option<Arc<razdor::rules::content::Content>> {
        let base = super::dt_content_of(&self.assets, self.audio.settings.impossible)?;
        Some(match &self.pack {
            Some(p) => Arc::new(base.with_package(p.opened.units.clone(), p.opened.items.clone())),
            None => base,
        })
    }

    /// Keeps the open package's map in the scenario list (the install's maps are read again on a
    /// reload, which drops it); a package opened before is replaced. A campaign's maps show the
    /// campaign's name.
    pub(super) fn keep_pack_map(&mut self) {
        let Some(opened) = self.pack.as_ref().map(|p| p.opened.clone()) else { return };
        let hub_pack = hub_here().and_then(|hub| HubPack::of(&hub, &opened.id, &opened.version));
        // The package's own entry (the Hub's, on the version it plays) is replaced, not listed twice.
        self.scenarios.retain(|e| keeps_listed(e.pack.as_ref().map(|p| p.id.as_str()), e.hub_pack.is_some(), &opened.id));
        self.scenarios.push(package_entry(opened, hub_pack));
    }

    /// Whether a campaign package's game is on screen.
    pub(super) fn campaign_playing(&self) -> bool {
        self.game.is_some() && self.pack.as_ref().is_some_and(|p| p.opened.campaign.is_some())
    }

    /// The campaign's victory on the map on screen: the next map the graph names, entered with what
    /// carries (`rules::campaign::enter`). When no route holds, the campaign ends with the victory
    /// screen.
    pub(super) fn campaign_victory(&mut self) -> Option<Screen> {
        let routes = self.pack.as_ref()?.opened.routes.clone();
        let next = after_victory(self.game.as_mut()?, &routes);
        match next {
            Some(map) => match self.campaign_go(&map, None) {
                Ok(()) => Some(Screen::WorldMap),
                Err(e) => {
                    razdor::diag!("campaign: {e}; the campaign ends here");
                    Some(Screen::Victory)
                }
            },
            None => Some(Screen::Victory),
        }
    }

    /// A story's request once its dialogs are read: a `travel` goes to its map (at its point), an
    /// `ending` ends the campaign on the victory screen, which shows the ending's dialog title.
    pub(super) fn campaign_request(&mut self) -> Option<Screen> {
        let travel = self.game.as_mut()?.triggers.travel.take();
        if let Some(t) = travel {
            return match self.campaign_go(&t.map, t.at.as_deref()) {
                Ok(()) => Some(Screen::WorldMap),
                Err(e) => {
                    razdor::diag!("campaign: travel to {}: {e}", t.map);
                    None
                }
            };
        }
        let ending = self.game.as_mut()?.triggers.ending.take()?;
        self.game.as_mut()?.set_campaign_ended(&ending);
        Some(Screen::Victory)
    }

    /// The campaign moves to map `map` (entered at point `at`, else its start): the package's map
    /// is opened from its file, the game of it is made by `enter`, and the map on screen is the new
    /// one. Nothing changes when the map cannot be opened or restored.
    fn campaign_go(&mut self, map: &str, at: Option<&str>) -> Result<(), String> {
        let install = self.assets.dt.as_ref().map(|d| &d.install).ok_or("no Discord Times install (set RAZDOR_DT_DIR)")?;
        let path = self.pack.as_ref().ok_or("no package open")?.opened.path.clone();
        let to = pack::open::open(&path, Some(map), pack::open::lang(), install)?;
        let content = self.install_content().ok_or("no game content")?;
        let from = &self.pack.as_ref().ok_or("no package open")?.opened;
        let game = enter(self.game.as_ref().ok_or("no game")?, from, &to, at, content)?;
        self.adopt_pack(to);
        self.map_view.reset();
        self.map_view.open_around_hero(&game);
        items_view::reset_pack_view();
        self.message = None;
        self.game = Some(game);
        Ok(())
    }
}

/// The hero window's Start began a game of a package's entry: the Hub remembers the version played
/// (`mark_played`), so the «New version» mark goes once the newest is played. A failure is logged;
/// the game starts all the same.
pub(super) fn note_played(e: &ScenarioEntry) {
    let Some(hp) = &e.hub_pack else { return };
    if let Some(Err(err)) = hub_here().map(|hub| hub.mark_played(&hp.id, &hp.version)) {
        razdor::diag!("packages: {err}");
    }
}
