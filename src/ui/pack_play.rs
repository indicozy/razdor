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
use razdor::rules::save::{ScenarioRef, SaveError};
use razdor::{hub, platform};

use super::{App, Screen, ScenarioEntry};

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
        self.adopt_pack(opened);
        let index = self.scenarios.iter().position(|e| e.pack.is_some()).ok_or("the package's map is not in the list")?;
        self.screen = Screen::ClassSelect { scenario: Some(index) };
        Ok(())
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
    /// reload, which drops it); a package opened before is replaced.
    pub(super) fn keep_pack_map(&mut self) {
        let Some(p) = &self.pack else { return };
        self.scenarios.retain(|e| e.pack.is_none());
        self.scenarios.push(ScenarioEntry {
            file: p.opened.map.clone(),
            path: PathBuf::new(),
            scenario: p.opened.scenario.clone(),
            base_game: false,
            pack: Some(p.opened.clone()),
        });
    }
}
