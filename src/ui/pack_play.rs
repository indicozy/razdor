//! `razdor --pack <folder-or-.rzpack> [--map <id>]`, or the `RAZDOR_PACK` and `RAZDOR_PACK_MAP`
//! variables (for the off-screen snapshots): a content package's map as a new game.
//!
//! The package is checked and compiled on the player's install (`pack::compile`). Its map joins the
//! scenario list, the game's content gains the package's units and items (`Content::with_package`),
//! and the pictures of the package's units are those of their base units. The game opens on the
//! hero window, as a map of the install does; `ScenarioSelect` lists the package's map too.
//!
//! A package game cannot be saved yet: a save names its map's file and hash, and a package has no
//! such file (see `docs/reference/rzpack-format.md`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use razdor::dt::data::{ArtefactDef, UnitDef};
use razdor::dt::dtm::Scenario;
use razdor::pack::{self, compile, Files, Package};

use super::{App, Screen, ScenarioEntry};

/// The package's map as the run plays it.
pub struct PackPlay {
    /// The map's id: the scenario's file name in the list.
    pub map: String,
    pub scenario: Scenario,
    pub units: Vec<UnitDef>,
    pub items: Vec<ArtefactDef>,
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
    /// hero window comes up. An error names what is wrong; nothing changes then.
    pub fn open_pack(&mut self, path: &Path, map: Option<&str>) -> Result<(), String> {
        let files = Files::load(path)?;
        let (package, _) = Package::parse(files);
        let report = pack::check(&package);
        if report.has_errors() {
            return Err(format!("{} does not check:\n{}", path.display(), report.to_text()));
        }
        let map_id = match map {
            Some(m) => m.to_string(),
            None => package.maps.keys().next().cloned().ok_or("the package has no map (maps/<id>/map.toml)")?,
        };
        let lang = if matches!(razdor::i18n::lang(), razdor::i18n::Lang::Ru) { "ru" } else { "en" };
        let compiled = {
            let install = self.assets.dt.as_ref().map(|d| &d.install).ok_or("no Discord Times install (set RAZDOR_DT_DIR)")?;
            compile::compile(&package, install, lang, &map_id).map_err(|r| r.to_text())?
        };
        if let Some(dt) = self.assets.dt.as_mut() {
            dt.install.unit_pictures.extend(compiled.pictures.iter().copied());
            dt.install.unit_pictures.sort_unstable();
        }
        self.pack = Some(PackPlay { map: map_id.clone(), scenario: compiled.scenario, units: compiled.units, items: compiled.items });
        self.keep_pack_map();
        self.dt_content = self.install_content();
        let index = self.scenarios.iter().position(|e| e.file == map_id).ok_or("the package's map is not in the list")?;
        self.screen = Screen::ClassSelect { scenario: Some(index) };
        Ok(())
    }

    /// The content the game plays on: the install's, with the package's units and items added.
    pub(super) fn install_content(&self) -> Option<Arc<razdor::rules::content::Content>> {
        let base = super::dt_content_of(&self.assets, self.audio.settings.impossible)?;
        Some(match &self.pack {
            Some(p) => Arc::new(base.with_package(p.units.clone(), p.items.clone())),
            None => base,
        })
    }

    /// Keeps the package's map in the scenario list (the install's maps are read again on a reload).
    pub(super) fn keep_pack_map(&mut self) {
        let Some(p) = &self.pack else { return };
        if self.scenarios.iter().any(|e| e.file == p.map) {
            return;
        }
        self.scenarios.push(ScenarioEntry { file: p.map.clone(), path: PathBuf::new(), scenario: p.scenario.clone(), base_game: false });
    }
}
