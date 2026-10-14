//! The map hub (Razdor's own; a mock for now): a catalog of community maps, campaigns and
//! mods that the player browses and installs from inside the game (`ui::hub_view`).
//!
//! - **Catalog**: an `index.json` ([`Catalog`]) next to the files it names. Each entry is a
//!   map, a campaign (its chapters in order, linked by their next-map names) or a mod, with
//!   its titles and descriptions in Russian and English, the languages its maps come in
//!   (Russian as written, English from the import's translation) and the mods it needs.
//! - **Source** ([`Source`]): `RAZDOR_HUB` names an `https://` address or a folder; unset, a
//!   mock hub is written once under the hub folder ([`mock::build`]) and read from there.
//! - **Install**: a map or campaign needs its mods first ([`plan_install`] orders them,
//!   dependencies before what needs them). Every file is checked against the catalog's size
//!   and SHA-256, then written atomically: maps into `<hub>/maps/` (flat, so a campaign's
//!   next-map names find each other and the new-game list can read them), mods into
//!   `<hub>/mods/<id>/`. `<hub>/installed.json` records what went where.
//! - **Remove**: refused while an installed entry still needs the mod ([`needed_by`]).
//!
//! The hub folder is `RAZDOR_HUB_DIR`, else `razdor/hub` in the platform data folder. Mods are
//! only recorded for now: the game does not switch to a mod's data yet.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::i18n::Lang;

/// Environment variable naming the hub: an `https://` address or a folder.
pub const SOURCE_ENV: &str = "RAZDOR_HUB";
/// Environment variable naming the local hub folder.
pub const DIR_ENV: &str = "RAZDOR_HUB_DIR";
/// The catalog's file name at the hub's root.
pub const INDEX: &str = "index.json";

/// A text in both interface languages.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Text {
    pub ru: String,
    #[serde(default)]
    pub en: String,
}

impl Text {
    /// The text in `lang`, else the Russian.
    pub fn get(&self, lang: Lang) -> &str {
        match lang {
            Lang::En if !self.en.trim().is_empty() => &self.en,
            _ => &self.ru,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Map,
    Campaign,
    Mod,
}

/// One file of an entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// Where it is in the hub, relative to the catalog.
    pub path: String,
    /// The name it is installed under (a campaign's chapters keep the names their next-map
    /// fields use, whatever the language).
    pub name: String,
    /// The language of its texts (`ru`, `en`); none for a file that has no text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    pub size: u64,
    pub sha256: String,
}

/// An entry of the catalog.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: String,
    pub kind: Kind,
    pub title: Text,
    pub author: String,
    pub version: String,
    #[serde(default)]
    pub description: Text,
    /// The languages its maps come in; `en` beyond the author's `ru` came from the import's
    /// machine translation when `machine_translated` is set.
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub machine_translated: bool,
    /// Ids of the mods it needs.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Where it was posted (a Discord message, a forum thread).
    #[serde(default)]
    pub source_url: String,
    /// Date it was added, `YYYY-MM-DD`.
    #[serde(default)]
    pub added: String,
    pub files: Vec<FileRef>,
}

impl Entry {
    /// The files to install for `lang`: those in that language or with no language; when it
    /// has no file in `lang`, the Russian ones.
    pub fn files_for(&self, lang: &str) -> Vec<&FileRef> {
        let has = self.files.iter().any(|f| f.lang.as_deref() == Some(lang));
        let want = if has { lang } else { "ru" };
        self.files.iter().filter(|f| f.lang.as_deref().is_none_or(|l| l == want)).collect()
    }

    /// The number of maps it installs (a campaign's chapters).
    pub fn map_count(&self) -> usize {
        self.files_for("ru").iter().filter(|f| is_map_name(&f.name)).count()
    }
}

/// The hub's `index.json`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub format: u32,
    #[serde(default)]
    pub entries: Vec<Entry>,
}

impl Catalog {
    pub fn parse(bytes: &[u8]) -> Result<Catalog, String> {
        let c: Catalog = serde_json::from_slice(bytes).map_err(|e| format!("index.json: {e}"))?;
        if c.format != 1 {
            return Err(format!("index.json: format {} is not known", c.format));
        }
        for e in &c.entries {
            // An id names a mod's folder (`mods/<id>/`): letters, digits, `-` and `_` only.
            if !safe_id(&e.id) || !e.requires.iter().all(|r| safe_id(r)) {
                return Err(format!("index.json: bad id {:?}", e.id));
            }
            for f in &e.files {
                if !safe_name(&f.name) || !safe_path(&f.path) {
                    return Err(format!("index.json: {}: bad file name {:?}", e.id, f.name));
                }
            }
        }
        Ok(c)
    }

    pub fn entry(&self, id: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.id == id)
    }
}

/// An entry id: it becomes a folder name, so nothing that could leave the hub folder.
fn safe_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// A file name the hub may write: no folders, no dots in front, nothing odd.
fn safe_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && name.chars().all(|c| c.is_alphanumeric() || "-_. ".contains(c))
}

/// A path inside the hub: relative, no `..`.
fn safe_path(path: &str) -> bool {
    !path.is_empty() && !path.starts_with('/') && !path.contains('\\') && path.split('/').all(|p| p != ".." && !p.is_empty())
}

fn is_map_name(name: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, ext)| ext.eq_ignore_ascii_case("DTm"))
}

/// What is installed: per entry id, its version, language and the files written.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Installed {
    #[serde(default)]
    pub entries: BTreeMap<String, InstalledEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstalledEntry {
    pub version: String,
    pub kind: Kind,
    #[serde(default)]
    pub lang: String,
    /// Relative to the hub folder.
    pub files: Vec<String>,
}

impl Installed {
    pub fn has(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }
}

/// Why an entry cannot be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PlanError {
    /// The catalog has no entry of this id (a needed mod the hub does not carry).
    Missing(String),
    /// Mods needing each other in a circle.
    Cycle(String),
}

/// What installing `id` takes, in order: the mods it needs that are not installed yet
/// (dependencies first, each once), then `id` itself (also when it is installed: a reinstall).
pub fn plan_install(catalog: &Catalog, installed: &Installed, id: &str) -> Result<Vec<String>, PlanError> {
    fn visit(catalog: &Catalog, installed: &Installed, id: &str, top: bool, path: &mut Vec<String>, out: &mut Vec<String>) -> Result<(), PlanError> {
        if out.iter().any(|o| o == id) || (!top && installed.has(id)) {
            return Ok(());
        }
        if path.iter().any(|p| p == id) {
            return Err(PlanError::Cycle(id.to_string()));
        }
        let e = catalog.entry(id).ok_or_else(|| PlanError::Missing(id.to_string()))?;
        path.push(id.to_string());
        for r in &e.requires {
            visit(catalog, installed, r, false, path, out)?;
        }
        path.pop();
        out.push(id.to_string());
        Ok(())
    }
    let mut out = Vec::new();
    visit(catalog, installed, id, true, &mut Vec::new(), &mut out)?;
    Ok(out)
}

/// The installed entries that need `id`.
pub fn needed_by(catalog: &Catalog, installed: &Installed, id: &str) -> Vec<String> {
    installed
        .entries
        .keys()
        .filter(|other| *other != id && catalog.entry(other).is_some_and(|e| e.requires.iter().any(|r| r == id)))
        .cloned()
        .collect()
}

/// Where the hub is read from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Url(String),
    Dir(PathBuf),
    /// The mock hub, written under the hub folder on first use.
    Mock,
}

impl Source {
    /// `RAZDOR_HUB`, else the mock.
    pub fn from_env() -> Source {
        match std::env::var(SOURCE_ENV) {
            Ok(s) if s.starts_with("https://") => Source::Url(s.trim_end_matches('/').to_string()),
            Ok(s) if !s.trim().is_empty() => Source::Dir(PathBuf::from(s)),
            _ => Source::Mock,
        }
    }

    /// Short words for the window's corner.
    pub fn label(&self) -> String {
        match self {
            Source::Url(u) => u.clone(),
            Source::Dir(d) => d.display().to_string(),
            Source::Mock => "mock".into(),
        }
    }
}

/// The local hub folder: `RAZDOR_HUB_DIR`, else `razdor/hub` in the platform data folder.
pub fn local_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    dirs::data_dir().map(|d| d.join("razdor").join("hub"))
}

/// The folder installed maps go to.
pub fn maps_dir(local: &Path) -> PathBuf {
    local.join("maps")
}

/// A hub and the local folder it installs into.
#[derive(Clone, Debug)]
pub struct Hub {
    pub source: Source,
    pub local: PathBuf,
}

impl Hub {
    fn root(&self) -> Result<Root, String> {
        match &self.source {
            Source::Url(u) => Ok(Root::Url(u.clone())),
            Source::Dir(d) => Ok(Root::Dir(d.clone())),
            Source::Mock => {
                let dir = self.local.join("mock-hub");
                if !dir.join(INDEX).is_file() {
                    mock::build(&dir).map_err(|e| format!("mock hub: {e}"))?;
                }
                Ok(Root::Dir(dir))
            }
        }
    }

    fn fetch(&self, rel: &str) -> Result<Vec<u8>, String> {
        match self.root()? {
            Root::Url(u) => crate::update::fetch(&format!("{u}/{rel}"), 60),
            Root::Dir(d) => std::fs::read(d.join(rel)).map_err(|e| format!("{rel}: {e}")),
        }
    }

    pub fn catalog(&self) -> Result<Catalog, String> {
        Catalog::parse(&self.fetch(INDEX)?)
    }

    pub fn installed(&self) -> Installed {
        std::fs::read(self.local.join("installed.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    fn save_installed(&self, i: &Installed) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(i).map_err(|e| e.to_string())?;
        crate::editor::files::write_atomically(&self.local.join("installed.json"), &json).map_err(|e| format!("installed.json: {e}"))
    }

    /// Installs `id` and the mods it needs (see [`plan_install`]); returns the ids installed.
    pub fn install(&self, catalog: &Catalog, id: &str, lang: Lang) -> Result<Vec<String>, String> {
        let mut installed = self.installed();
        let plan = plan_install(catalog, &installed, id).map_err(|e| match e {
            PlanError::Missing(m) => format!("{m}: not in the hub"),
            PlanError::Cycle(m) => format!("{m}: mods need each other"),
        })?;
        let code = match lang {
            Lang::En => "en",
            Lang::Ru => "ru",
        };
        for pid in &plan {
            let e = catalog.entry(pid).expect("planned from the catalog");
            let files = e.files_for(code);
            // All files are fetched and checked before any is written.
            let mut got = Vec::new();
            for f in &files {
                let bytes = self.fetch(&f.path)?;
                if bytes.len() as u64 != f.size || crate::update::sha256_hex(&bytes) != f.sha256.to_lowercase() {
                    return Err(format!("{}: the file does not match the catalog", f.path));
                }
                got.push((f, bytes));
            }
            let mut written = Vec::new();
            for (f, bytes) in got {
                let rel = match e.kind {
                    Kind::Mod => format!("mods/{}/{}", e.id, f.name),
                    Kind::Map | Kind::Campaign => format!("maps/{}", f.name),
                };
                crate::editor::files::write_atomically(&self.local.join(&rel), &bytes).map_err(|err| format!("{rel}: {err}"))?;
                written.push(rel);
            }
            let lang = files.iter().find_map(|f| f.lang.clone()).unwrap_or_default();
            installed.entries.insert(e.id.clone(), InstalledEntry { version: e.version.clone(), kind: e.kind, lang, files: written });
            self.save_installed(&installed)?;
        }
        Ok(plan)
    }

    /// Removes `id`'s files, unless an installed entry needs it.
    pub fn remove(&self, catalog: &Catalog, id: &str) -> Result<(), String> {
        let mut installed = self.installed();
        let users = needed_by(catalog, &installed, id);
        if !users.is_empty() {
            return Err(format!("{id}: needed by {}", users.join(", ")));
        }
        let Some(e) = installed.entries.remove(id) else { return Ok(()) };
        for rel in &e.files {
            if safe_path(rel) {
                let _ = std::fs::remove_file(self.local.join(rel));
            }
        }
        if e.kind == Kind::Mod && safe_id(id) {
            let _ = std::fs::remove_dir(self.local.join("mods").join(id));
        }
        self.save_installed(&installed)
    }
}

enum Root {
    Url(String),
    Dir(PathBuf),
}

/// The mock hub: small empty maps made here (nothing from the game), with catalog texts of our
/// own, covering each case the window shows: maps in one or two languages, campaigns, mods,
/// a map needing a mod, a campaign needing a mod that needs another, and a map needing a mod
/// the hub does not carry.
pub mod mock {
    use super::*;
    use crate::dt::container;
    use crate::editor::defaults::{new_scenario, NewMap};

    struct MapSpec {
        file: &'static str,
        title: (&'static str, &'static str),
        about: (&'static str, &'static str),
        size: u32,
        fill: u8,
    }

    /// The bytes of a mock map in `lang`: an empty map with its texts. `kind` is header
    /// 0x10F (0 standalone, 1 a campaign's first map, 2 a later one).
    fn map_bytes(s: &MapSpec, lang: &str, campaign: &str, kind: u8, next: &str) -> Vec<u8> {
        let mut sc = new_scenario(NewMap { width: s.size, height: s.size, fill: s.fill });
        let en = lang == "en";
        sc.title = if en { s.title.1 } else { s.title.0 }.into();
        sc.description = if en { s.about.1 } else { s.about.0 }.into();
        sc.campaign_name = campaign.into();
        sc.next_map = next.into();
        sc.header.scenario_kind = kind;
        container::encode(19, &sc.to_payload())
    }

    struct Out<'a> {
        dir: &'a Path,
        entries: Vec<Entry>,
    }

    impl Out<'_> {
        fn file(&self, path: String, name: &str, lang: Option<&str>, bytes: &[u8]) -> std::io::Result<FileRef> {
            crate::editor::files::write_atomically(&self.dir.join(&path), bytes)?;
            Ok(FileRef { path, name: name.into(), lang: lang.map(Into::into), size: bytes.len() as u64, sha256: crate::update::sha256_hex(bytes) })
        }

        /// A map or campaign: its chapters in `langs`, linked by their next-map names.
        #[allow(clippy::too_many_arguments)]
        fn maps(&mut self, id: &str, kind: Kind, title: (&str, &str), author: &str, about: (&str, &str), langs: &[&str], requires: &[&str], added: &str, chapters: &[MapSpec]) -> std::io::Result<()> {
            let mut files = Vec::new();
            for lang in langs {
                for (i, ch) in chapters.iter().enumerate() {
                    let (campaign, k, next) = match kind {
                        Kind::Campaign => (if *lang == "en" { title.1 } else { title.0 }, if i == 0 { 1 } else { 2 }, chapters.get(i + 1).map_or(String::new(), |n| format!("{}.DTm", n.file))),
                        _ => ("", 0, String::new()),
                    };
                    let bytes = map_bytes(ch, lang, campaign, k, &next);
                    let name = format!("{}.DTm", ch.file);
                    files.push(self.file(format!("maps/{id}/{lang}/{name}"), &name, Some(lang), &bytes)?);
                }
            }
            self.entries.push(Entry {
                id: id.into(),
                kind,
                title: Text { ru: title.0.into(), en: title.1.into() },
                author: author.into(),
                version: "1.0".into(),
                description: Text { ru: about.0.into(), en: about.1.into() },
                languages: langs.iter().map(|l| l.to_string()).collect(),
                machine_translated: langs.contains(&"en"),
                requires: requires.iter().map(|r| r.to_string()).collect(),
                source_url: String::new(),
                added: added.into(),
                files,
            });
            Ok(())
        }

        /// A mod: a manifest only (a mock mod carries no game data).
        #[allow(clippy::too_many_arguments)]
        fn a_mod(&mut self, id: &str, title: (&str, &str), author: &str, version: &str, about: (&str, &str), requires: &[&str], added: &str) -> std::io::Result<()> {
            let manifest = serde_json::json!({ "id": id, "name": title.0, "version": version, "mock": true });
            let bytes = serde_json::to_vec_pretty(&manifest).expect("json");
            let f = self.file(format!("mods/{id}/mod.json"), "mod.json", None, &bytes)?;
            self.entries.push(Entry {
                id: id.into(),
                kind: Kind::Mod,
                title: Text { ru: title.0.into(), en: title.1.into() },
                author: author.into(),
                version: version.into(),
                description: Text { ru: about.0.into(), en: about.1.into() },
                languages: vec!["ru".into()],
                machine_translated: false,
                requires: requires.iter().map(|r| r.to_string()).collect(),
                source_url: String::new(),
                added: added.into(),
                files: vec![f],
            });
            Ok(())
        }
    }

    /// Writes the mock hub (its files and `index.json`) into `dir`.
    pub fn build(dir: &Path) -> std::io::Result<()> {
        let mut o = Out { dir, entries: Vec::new() };
        let map = |file, title, about, size, fill| MapSpec { file, title, about, size, fill };
        o.maps(
            "ford-outpost",
            Kind::Map,
            ("Застава у брода", "The Outpost at the Ford"),
            "Ратибор",
            ("Удержите брод, пока не подойдёт подмога из города.", "Hold the ford until help comes from the town."),
            &["ru", "en"],
            &[],
            "2026-09-02",
            &[map("Hub_Ford", ("Застава у брода", "The Outpost at the Ford"), ("Удержите брод.", "Hold the ford."), 40, 6)],
        )?;
        o.maps(
            "black-wood",
            Kind::Map,
            ("Тени Чернолесья", "Shadows of the Black Wood"),
            "Велимира",
            ("Лесные разбойники грабят обозы. Найдите их логово.", "Forest robbers raid the carts. Find their lair."),
            &["ru"],
            &[],
            "2026-09-11",
            &[map("Hub_BlackWood", ("Тени Чернолесья", "Shadows of the Black Wood"), ("Найдите логово разбойников.", "Find the robbers' lair."), 50, 7)],
        )?;
        o.maps(
            "north-march",
            Kind::Campaign,
            ("Сага о Северной марке", "The Saga of the North March"),
            "Гостомысл",
            ("Три главы: от пограничной крепости до столицы марки.", "Three chapters, from a border fort to the march's capital."),
            &["ru", "en"],
            &[],
            "2026-09-20",
            &[
                map("Hub_North1", ("Пограничная крепость", "The Border Fort"), ("Глава первая.", "Chapter one."), 40, 6),
                map("Hub_North2", ("Перевал", "The Pass"), ("Глава вторая.", "Chapter two."), 40, 9),
                map("Hub_North3", ("Столица марки", "The March's Capital"), ("Глава третья.", "Chapter three."), 50, 6),
            ],
        )?;
        o.a_mod(
            "evolution-mock",
            ("Эволюция (макет)", "Evolution (mock)"),
            "Сообщество",
            "9.0",
            ("Новые отряды, предметы и заклинания. В макете файлов игры нет.", "New units, items and spells. The mock carries no game files."),
            &[],
            "2026-08-15",
        )?;
        o.a_mod(
            "evolution-mercs",
            ("Эволюция: наёмники (макет)", "Evolution: Mercenaries (mock)"),
            "Сообщество",
            "1.2",
            ("Дополнение к «Эволюции»: гильдии наёмников.", "An add-on to Evolution: mercenary guilds."),
            &["evolution-mock"],
            "2026-09-01",
        )?;
        o.maps(
            "rotten-castle",
            Kind::Map,
            ("Осада Гнилого замка", "Siege of the Rotten Castle"),
            "Ратибор",
            ("Нужен мод «Эволюция»: в гарнизоне его новые отряды.", "Needs the Evolution mod: its new units hold the garrison."),
            &["ru", "en"],
            &["evolution-mock"],
            "2026-09-25",
            &[map("Hub_Rotten", ("Осада Гнилого замка", "Siege of the Rotten Castle"), ("Возьмите замок.", "Take the castle."), 40, 8)],
        )?;
        o.maps(
            "merc-road",
            Kind::Campaign,
            ("Наёмничья тропа", "The Mercenary Road"),
            "Велимира",
            ("Две главы для «Эволюции» с дополнением о наёмниках.", "Two chapters for Evolution with the mercenaries add-on."),
            &["ru"],
            &["evolution-mercs"],
            "2026-10-03",
            &[
                map("Hub_Merc1", ("Вербовка", "The Muster"), ("Глава первая.", "Chapter one."), 40, 6),
                map("Hub_Merc2", ("Расплата", "The Reckoning"), ("Глава вторая.", "Chapter two."), 40, 7),
            ],
        )?;
        o.maps(
            "lost-update",
            Kind::Map,
            ("Забытый остров", "The Forgotten Isle"),
            "Неизвестный",
            ("Сделана для мода, которого в хабе нет.", "Made for a mod the hub does not carry."),
            &["ru"],
            &["community-update"],
            "2026-10-08",
            &[map("Hub_Isle", ("Забытый остров", "The Forgotten Isle"), ("Найдите остров.", "Find the isle."), 30, 2)],
        )?;
        let catalog = Catalog { format: 1, entries: o.entries };
        let json = serde_json::to_vec_pretty(&catalog).expect("json");
        crate::editor::files::write_atomically(&dir.join(INDEX), &json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::Scenario;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("razdor-hub-{tag}-{}-{nanos}", std::process::id()))
    }

    fn mock_hub(tag: &str) -> (Hub, Catalog) {
        let hub = Hub { source: Source::Mock, local: temp(tag) };
        let c = hub.catalog().unwrap();
        (hub, c)
    }

    #[test]
    fn the_mock_catalog_reads_back_and_its_maps_parse() {
        let (hub, c) = mock_hub("parse");
        assert!(c.entries.len() >= 8);
        for e in c.entries.iter().filter(|e| e.kind != Kind::Mod) {
            for f in &e.files {
                let bytes = std::fs::read(hub.local.join("mock-hub").join(&f.path)).unwrap();
                let s = Scenario::from_file_bytes(&bytes).unwrap();
                assert!(!s.title.is_empty(), "{}", f.path);
            }
        }
        let north = c.entry("north-march").unwrap();
        assert_eq!(north.map_count(), 3);
        let first = std::fs::read(hub.local.join("mock-hub").join(&north.files_for("ru")[0].path)).unwrap();
        let s = Scenario::from_file_bytes(&first).unwrap();
        assert_eq!((s.header.scenario_kind, s.next_map.as_str()), (1, "Hub_North2.DTm"));
    }

    #[test]
    fn files_follow_the_language_and_fall_back_to_russian() {
        let (_, c) = mock_hub("lang");
        let ford = c.entry("ford-outpost").unwrap();
        assert!(ford.files_for("en").iter().all(|f| f.lang.as_deref() == Some("en")));
        let wood = c.entry("black-wood").unwrap();
        assert!(wood.files_for("en").iter().all(|f| f.lang.as_deref() == Some("ru")));
        assert_eq!(wood.files_for("en").len(), 1);
    }

    #[test]
    fn mods_come_first_in_the_plan_and_once() {
        let (_, c) = mock_hub("plan");
        let none = Installed::default();
        assert_eq!(plan_install(&c, &none, "merc-road").unwrap(), ["evolution-mock", "evolution-mercs", "merc-road"]);
        assert_eq!(plan_install(&c, &none, "ford-outpost").unwrap(), ["ford-outpost"]);
        assert_eq!(plan_install(&c, &none, "lost-update"), Err(PlanError::Missing("community-update".into())));
        let mut some = Installed::default();
        some.entries.insert("evolution-mock".into(), InstalledEntry { version: "9.0".into(), kind: Kind::Mod, lang: String::new(), files: vec![] });
        assert_eq!(plan_install(&c, &some, "merc-road").unwrap(), ["evolution-mercs", "merc-road"]);
    }

    #[test]
    fn a_circle_of_mods_is_refused() {
        let (_, mut c) = mock_hub("cycle");
        c.entries.iter_mut().find(|e| e.id == "evolution-mock").unwrap().requires = vec!["evolution-mercs".into()];
        assert!(matches!(plan_install(&c, &Installed::default(), "merc-road"), Err(PlanError::Cycle(_))));
    }

    #[test]
    fn install_writes_the_maps_and_mods_and_remove_respects_needs() {
        let (hub, c) = mock_hub("install");
        let done = hub.install(&c, "merc-road", Lang::En).unwrap();
        assert_eq!(done.len(), 3);
        let maps = maps_dir(&hub.local);
        assert!(maps.join("Hub_Merc1.DTm").is_file());
        assert!(hub.local.join("mods/evolution-mock/mod.json").is_file());
        let inst = hub.installed();
        assert_eq!(inst.entries["merc-road"].lang, "ru", "no English files: the Russian ones");
        assert!(hub.remove(&c, "evolution-mock").is_err(), "the add-on needs it");
        hub.remove(&c, "merc-road").unwrap();
        hub.remove(&c, "evolution-mercs").unwrap();
        hub.remove(&c, "evolution-mock").unwrap();
        assert!(hub.installed().entries.is_empty());
        assert!(!hub.local.join("mods/evolution-mock").exists());
    }

    #[test]
    fn english_maps_install_under_the_chapter_names() {
        let (hub, c) = mock_hub("english");
        hub.install(&c, "north-march", Lang::En).unwrap();
        let s = Scenario::load(&maps_dir(&hub.local).join("Hub_North1.DTm")).unwrap();
        assert_eq!(s.title, "The Border Fort");
        assert_eq!(s.next_map, "Hub_North2.DTm");
    }

    #[test]
    fn a_damaged_file_is_not_installed() {
        let (hub, c) = mock_hub("damaged");
        let f = &c.entry("ford-outpost").unwrap().files[0];
        std::fs::write(hub.local.join("mock-hub").join(&f.path), b"broken").unwrap();
        assert!(hub.install(&c, "ford-outpost", Lang::Ru).is_err());
        assert!(!hub.installed().has("ford-outpost"));
    }

    #[test]
    fn the_catalog_refuses_paths_out_of_the_hub() {
        let bad = br#"{"format":1,"entries":[{"id":"x","kind":"map","title":{"ru":"x"},"author":"a","version":"1","files":[{"path":"../x","name":"x.DTm","size":1,"sha256":""}]}]}"#;
        assert!(Catalog::parse(bad).is_err());
        let bad_name = br#"{"format":1,"entries":[{"id":"x","kind":"map","title":{"ru":"x"},"author":"a","version":"1","files":[{"path":"x","name":"../x.DTm","size":1,"sha256":""}]}]}"#;
        assert!(Catalog::parse(bad_name).is_err());
        let bad_id = br#"{"format":1,"entries":[{"id":"../../x","kind":"mod","title":{"ru":"x"},"author":"a","version":"1","files":[{"path":"x","name":"mod.json","size":1,"sha256":""}]}]}"#;
        assert!(Catalog::parse(bad_id).is_err());
        let bad_need = br#"{"format":1,"entries":[{"id":"x","kind":"map","title":{"ru":"x"},"author":"a","version":"1","requires":["a/b"],"files":[]}]}"#;
        assert!(Catalog::parse(bad_need).is_err());
    }
}
