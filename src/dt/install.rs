//! Locate and load the files of a Discord Times install (the player's own copy).
//!
//! Only reads. Nothing is copied or written anywhere.

use super::data::{self, ArtefactDef, GlobalOptions, SpellDef, UnitDef};
use super::dtm::Scenario;
use super::ini::Ini;
use super::DtError;
use std::path::{Path, PathBuf};

/// Environment variable pointing to the install directory.
pub const ENV_VAR: &str = "RAZDOR_DT_DIR";
pub const UNITS_FILE: &str = "Rus_Units.ini";
pub const ARTEFACTS_FILE: &str = "Rus_Artefacts.ini";
pub const SPELLS_FILE: &str = "Rus_Spells.ini";
pub const GLOBAL_FILE: &str = "_Global.ini";
/// The interface texts and the player's settings (`[Options]`).
pub const SETTINGS_FILE: &str = "Rus_DiscordTimes.ini";
pub const MAPS_DIR: &str = "Maps_Rus";
pub const MAP_EXTENSION: &str = "DTm";

/// A scenario file found in the maps folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MapEntry {
    /// File name without the extension.
    pub name: String,
    pub path: PathBuf,
}

impl MapEntry {
    pub fn load(&self) -> Result<Scenario, DtError> {
        Scenario::load(&self.path)
    }
}

/// The data definitions of an install plus the list of its maps (loaded on demand).
#[derive(Clone, Debug)]
pub struct DtInstall {
    pub dir: PathBuf,
    pub units: Vec<UnitDef>,
    pub artefacts: Vec<ArtefactDef>,
    pub spells: Vec<SpellDef>,
    pub options: GlobalOptions,
    /// The player's settings that change play.
    pub settings: PlayOptions,
    /// Sorted by file name.
    pub maps: Vec<MapEntry>,
}

fn io_err(path: &Path) -> impl FnOnce(std::io::Error) -> DtError + '_ {
    move |source| DtError::Io { path: path.to_path_buf(), source }
}

/// `dir/name`, matching the name ignoring case (the game comes from Windows).
fn find(dir: &Path, name: &str) -> Result<PathBuf, DtError> {
    let exact = dir.join(name);
    if exact.exists() {
        return Ok(exact);
    }
    let entries = std::fs::read_dir(dir).map_err(io_err(dir))?;
    for entry in entries {
        let entry = entry.map_err(io_err(dir))?;
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Ok(entry.path());
        }
    }
    Err(DtError::Io { path: exact, source: std::io::ErrorKind::NotFound.into() })
}

/// `dir/a/b/c` from a `/`-separated relative path, matching each component ignoring case.
pub fn find_path(dir: &Path, rel: &str) -> Result<PathBuf, DtError> {
    rel.split('/').filter(|c| !c.is_empty()).try_fold(dir.to_path_buf(), |p, c| find(&p, c))
}

fn read_ini(dir: &Path, name: &str) -> Result<Ini, DtError> {
    let path = find(dir, name)?;
    let bytes = std::fs::read(&path).map_err(io_err(&path))?;
    Ok(Ini::from_cp1251(&bytes))
}

/// The player's settings in `[Options]` of [`SETTINGS_FILE`] that change play, read as the
/// original reads them at start (0x4b8974): a flag is on when its value reads 1 (loosely,
/// [`super::ini::loose_int`]); a missing file, section or key reads 0, so off. The wide row
/// is the exception: Razdor's default since its first release, it is on unless the install
/// sets `OptValue11` (to anything but 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PlayOptions {
    /// `OptValue9`: improved enemy AI in battle.
    pub expert_ai: bool,
    /// `OptValue10`: "impossible" difficulty, factor F = 100 instead of 120.
    pub impossible: bool,
    /// `OptValue11`: the wide front row, 6 units per row instead of 4.
    pub wide_row: bool,
}

impl Default for PlayOptions {
    fn default() -> Self {
        PlayOptions { expert_ai: false, impossible: false, wide_row: true }
    }
}

impl PlayOptions {
    pub fn from_ini(ini: &Ini) -> PlayOptions {
        let options = ini.section("Options");
        let on = |key: &str| options.is_some_and(|s| s.int(key) == 1);
        let wide_row = options.and_then(|s| s.get("OptValue11")).is_none_or(|_| on("OptValue11"));
        PlayOptions { expert_ai: on("OptValue9"), impossible: on("OptValue10"), wide_row }
    }

    fn read(dir: &Path) -> PlayOptions {
        read_ini(dir, SETTINGS_FILE).map(|ini| PlayOptions::from_ini(&ini)).unwrap_or_default()
    }
}

/// All `*.DTm` files of the maps folder, sorted by name.
pub fn list_maps(dir: &Path) -> Result<Vec<MapEntry>, DtError> {
    let maps_dir = find(dir, MAPS_DIR)?;
    let mut maps = Vec::new();
    for entry in std::fs::read_dir(&maps_dir).map_err(io_err(&maps_dir))? {
        let path = entry.map_err(io_err(&maps_dir))?.path();
        let is_map = path.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(MAP_EXTENSION));
        if is_map && path.is_file() {
            let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            maps.push(MapEntry { name, path });
        }
    }
    maps.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(maps)
}

impl DtInstall {
    /// Load the unit, artefact and spell definitions and the global options from `dir`,
    /// and list its maps. Only a missing or unreadable file fails; values the readers had to
    /// ignore or entries they skipped (common in mods) are written to the log.
    pub fn load(dir: &Path) -> Result<DtInstall, DtError> {
        fn logged<T>(file: &str, loaded: data::Loaded<T>) -> T {
            for w in &loaded.warnings {
                crate::diag!("install: {file}: {w}");
            }
            loaded.value
        }
        let mut options = logged(GLOBAL_FILE, GlobalOptions::from_ini(&read_ini(dir, GLOBAL_FILE)?));
        let settings = PlayOptions::read(dir);
        if settings.impossible {
            options.difficulty_factor = 100;
        }
        Ok(DtInstall {
            dir: dir.to_path_buf(),
            units: logged(UNITS_FILE, data::parse_units(&read_ini(dir, UNITS_FILE)?)),
            artefacts: logged(ARTEFACTS_FILE, data::parse_artefacts(&read_ini(dir, ARTEFACTS_FILE)?)),
            spells: logged(SPELLS_FILE, data::parse_spells(&read_ini(dir, SPELLS_FILE)?)),
            options,
            settings,
            maps: list_maps(dir)?,
        })
    }

    /// Load the install named by `RAZDOR_DT_DIR`.
    pub fn from_env() -> Result<DtInstall, DtError> {
        let Some(dir) = locate() else {
            let own = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf));
            set_problem(own.map(|dir| Problem::NotInstall { missing: missing(&dir), dir }));
            return Err(DtError::NoInstallDir);
        };
        let loaded = DtInstall::load(&dir);
        match &loaded {
            Ok(dt) => crate::diag!("install loaded: {} units, {} items, {} spells, {} maps", dt.units.len(), dt.artefacts.len(), dt.spells.len(), dt.maps.len()),
            Err(e) => {
                crate::diag!("install failed to load: {e}");
                set_problem(Some(Problem::Load(e.to_string())));
            }
        }
        loaded
    }

    /// Unit type by `GlobalIndex`.
    pub fn unit(&self, id: u32) -> Option<&UnitDef> {
        self.units.iter().find(|u| u.id == id)
    }

    /// Artefact by `GlobalIndex`.
    pub fn artefact(&self, id: u32) -> Option<&ArtefactDef> {
        self.artefacts.iter().find(|a| a.id == id)
    }

    /// Spell by id (its 1-based section position).
    pub fn spell(&self, id: u32) -> Option<&SpellDef> {
        self.spells.iter().find(|s| s.id == id)
    }

    /// Map entry by file name without extension.
    pub fn map(&self, name: &str) -> Option<&MapEntry> {
        self.maps.iter().find(|m| m.name == name)
    }
}

/// Why no install could be used, for the player.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// The program's folder is not an install: what it lacks.
    NotInstall { dir: PathBuf, missing: Vec<String> },
    /// The install was found but a file of it failed to load.
    Load(String),
}

static PROBLEM: std::sync::Mutex<Option<Problem>> = std::sync::Mutex::new(None);

fn set_problem(why: Option<Problem>) {
    if let Ok(mut p) = PROBLEM.lock() {
        *p = why;
    }
}

/// Why the last [`DtInstall::from_env`] found no install or could not load it.
pub fn problem() -> Option<Problem> {
    PROBLEM.lock().ok().and_then(|p| p.clone())
}

/// A Discord Times install: a folder holding the maps folder and the unit list.
pub fn is_install(dir: &Path) -> bool {
    dir.join(MAPS_DIR).is_dir() && dir.join(UNITS_FILE).is_file()
}

/// What an install needs and `dir` lacks, for the log.
pub fn why_not_install(dir: &Path) -> String {
    if !dir.is_dir() {
        return "no such folder".into();
    }
    format!("missing {}", missing(dir).join(", "))
}

/// The folder and file of an install that `dir` lacks.
pub fn missing(dir: &Path) -> Vec<String> {
    let mut missing = Vec::new();
    if !dir.join(MAPS_DIR).is_dir() {
        missing.push(format!("{MAPS_DIR}/"));
    }
    if !dir.join(UNITS_FILE).is_file() {
        missing.push(UNITS_FILE.to_string());
    }
    missing
}

/// Where Razdor remembers the install folder: `$RAZDOR_CONFIG_DIR`, else the platform's
/// config folder (`$XDG_CONFIG_HOME` or `~/.config` on Linux, `%APPDATA%` on Windows) +
/// `razdor`; the file `install` holds the path.
pub fn config_file() -> Option<PathBuf> {
    let dir = std::env::var_os("RAZDOR_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| dirs::config_dir().map(|d| d.join("razdor")))?;
    Some(dir.join("install"))
}

/// Folders searched for an install when none is set or remembered: `~/Games`, `~/Downloads`
/// and the home folder.
pub fn search_roots() -> Vec<PathBuf> {
    let Some(home) = dirs::home_dir() else { return Vec::new() };
    vec![home.join("Games"), home.join("Downloads"), home]
}

/// The folder holding the running program, when it is an install: Razdor put into the game
/// folder next to `DiscordTimes.exe` plays that copy.
pub fn exe_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.canonicalize().unwrap_or(exe).parent()?.to_path_buf();
    is_install(&dir).then_some(dir)
}

/// The install folder: `RAZDOR_DT_DIR` when set (also from a `.env` file, see
/// [`load_dotenv`]), else the program's own folder when it is an install, else the remembered
/// one, else the first install found under `roots` (3 levels deep, hidden folders skipped).
/// A folder found by the variable or the search is remembered in `config` for next time.
pub fn locate_with(
    env: Option<std::ffi::OsString>,
    exe: Option<PathBuf>,
    config: Option<&Path>,
    roots: &[PathBuf],
) -> Option<PathBuf> {
    let remember = |dir: &Path| {
        if let Some(c) = config {
            let _ = c.parent().map(std::fs::create_dir_all);
            let _ = std::fs::write(c, dir.to_string_lossy().as_bytes());
        }
    };
    if let Some(dir) = env.map(PathBuf::from) {
        if is_install(&dir) {
            remember(&dir);
            return Some(dir);
        }
        crate::diag!("install: {ENV_VAR}={} is not an install ({})", dir.display(), why_not_install(&dir));
    }
    if let Some(dir) = exe {
        return Some(dir);
    }
    let saved = config.and_then(|c| std::fs::read_to_string(c).ok()).map(|t| PathBuf::from(t.trim()));
    if let Some(dir) = saved {
        if is_install(&dir) {
            return Some(dir);
        }
        crate::diag!("install: the remembered {} is not an install ({})", dir.display(), why_not_install(&dir));
    }
    fn search(dir: &Path, depth: u32) -> Option<PathBuf> {
        if is_install(dir) {
            return Some(dir.to_path_buf());
        }
        if depth == 0 {
            return None;
        }
        let mut subdirs: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| e.path())
            .collect();
        subdirs.sort();
        subdirs.iter().find_map(|d| search(d, depth - 1))
    }
    let found = roots.iter().find_map(|r| search(r, 3))?;
    remember(&found);
    Some(found)
}

/// [`locate_with`] with the real environment, config file and search folders.
pub fn locate() -> Option<PathBuf> {
    let exe = exe_dir();
    if exe.is_none() {
        if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
            crate::diag!("install: the program's folder {} is not an install ({})", dir.display(), why_not_install(&dir));
        }
    }
    let found = locate_with(std::env::var_os(ENV_VAR), exe, config_file().as_deref(), &search_roots());
    match &found {
        Some(dir) => crate::diag!("install: {}", dir.display()),
        None => crate::diag!("install: none found (set {ENV_VAR}, or put Razdor into the game folder)"),
    }
    found
}

/// Sets the variables of a `.env` file (`KEY=value` lines, `#` comments, optional quotes,
/// an optional `export `) that are not set already. Read from the current folder, then the
/// program's folder; the first file to set a variable wins. Call before anything reads them.
pub fn load_dotenv() {
    let mut files = vec![PathBuf::from(".env")];
    if let Some(dir) = std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)) {
        files.push(dir.join(".env"));
    }
    for file in files {
        let Ok(text) = std::fs::read_to_string(&file) else { continue };
        crate::diag!(".env: {}", file.canonicalize().unwrap_or(file.clone()).display());
        for (key, value) in parse_dotenv(&text) {
            if std::env::var_os(&key).is_none() {
                crate::diag!("  {key}={value:?}");
                std::env::set_var(key, value);
            } else {
                crate::diag!("  {key} already set, the file's value is not used");
            }
        }
    }
}

/// The `KEY=value` pairs of a `.env` file's text.
pub fn parse_dotenv(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let line = line.trim();
            let line = line.strip_prefix("export ").unwrap_or(line);
            if line.starts_with('#') {
                return None;
            }
            let (key, value) = line.split_once('=')?;
            let key = key.trim();
            let value = value.trim();
            let value = [('"', '"'), ('\'', '\'')]
                .iter()
                .find_map(|&(a, b)| value.strip_prefix(a).and_then(|v| v.strip_suffix(b)))
                .unwrap_or(value);
            (!key.is_empty()).then(|| (key.to_string(), value.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {

    fn fake_install(dir: &Path) {
        std::fs::create_dir_all(dir.join(MAPS_DIR)).unwrap();
        std::fs::write(dir.join(UNITS_FILE), b"").unwrap();
    }

    #[test]
    fn the_install_is_found_remembered_and_reused() {
        let tmp = std::env::temp_dir().join(format!("razdor-locate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        let game = tmp.join("home/Games/Discord Times Community Update");
        fake_install(&game);
        let config = tmp.join("config/razdor/install");
        let roots = vec![tmp.join("home/Games"), tmp.join("home")];
        assert!(!is_install(&tmp));
        // Nothing set or remembered: the search finds it and remembers it.
        assert_eq!(locate_with(None, None, Some(&config), &roots), Some(game.clone()));
        assert_eq!(std::fs::read_to_string(&config).unwrap(), game.to_string_lossy());
        // Next time, without searching (no roots), the remembered folder.
        assert_eq!(locate_with(None, None, Some(&config), &[]), Some(game.clone()));
        // The variable wins and is remembered.
        let other = tmp.join("elsewhere/DT");
        fake_install(&other);
        assert_eq!(locate_with(Some(other.clone().into()), None, Some(&config), &[]), Some(other.clone()));
        assert_eq!(locate_with(None, None, Some(&config), &[]), Some(other.clone()));
        // The program's own folder beats the remembered one, and is not remembered.
        assert_eq!(locate_with(None, Some(game.clone()), Some(&config), &[]), Some(game.clone()));
        assert_eq!(locate_with(None, None, Some(&config), &[]), Some(other));
        // A variable pointing at no install is ignored; nothing anywhere gives None.
        let empty = tmp.join("nothing");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(locate_with(Some(empty.clone().into()), None, None, std::slice::from_ref(&empty)), None);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn dotenv_lines() {
        let text = "# install\nexport RAZDOR_DT_DIR=\"/home/a/Games/Discord Times\"\n\nA = 'b'\nC=d=e\nnot a pair\n=x\n";
        let pairs = parse_dotenv(text);
        let want = [("RAZDOR_DT_DIR", "/home/a/Games/Discord Times"), ("A", "b"), ("C", "d=e")];
        assert_eq!(pairs, want.map(|(k, v)| (k.to_string(), v.to_string())));
    }
    use super::*;
    use crate::dt::dtm::{Archetype, BuildingType, GameDate};

    #[test]
    fn play_options_read_as_the_original() {
        // 0x4b8974: on when the value reads 1, loosely; anything else, or nothing, is off.
        let ini = Ini::parse("[Options]\nOptValue9=1\nOptValue10= 1 \nOptValue11=1x\n");
        assert_eq!(PlayOptions::from_ini(&ini), PlayOptions { expert_ai: true, impossible: true, wide_row: true });
        let ini = Ini::parse("[Options]\nOptValue9=01\nOptValue10=2\nOptValue11=on\n");
        assert_eq!(PlayOptions::from_ini(&ini), PlayOptions { expert_ai: true, impossible: false, wide_row: false });
        assert_eq!(PlayOptions::from_ini(&Ini::default()), PlayOptions::default());
        // Razdor's wide row default: with no OptValue11 the row is wide.
        assert_eq!(PlayOptions::default(), PlayOptions { expert_ai: false, impossible: false, wide_row: true });
        assert!(PlayOptions::from_ini(&Ini::parse("[Options]\nOptValue9=1\n")).wide_row);
        assert!(!PlayOptions::from_ini(&Ini::parse("[Options]\nOptValue11=0\n")).wide_row);
    }

    #[test]
    fn missing_dir_is_an_io_error() {
        let err = DtInstall::load(Path::new("/nonexistent/razdor-dt")).unwrap_err();
        assert!(matches!(err, DtError::Io { .. }), "{err}");
    }

    fn install() -> Option<DtInstall> {
        let dir = std::env::var_os(ENV_VAR)?;
        Some(DtInstall::load(Path::new(&dir)).expect("install loads"))
    }

    #[test]
    fn real_data_definitions() {
        let Some(dt) = install() else { return };
        assert_eq!(dt.units.len(), 102);
        assert_eq!(dt.artefacts.len(), 167);
        // 34 `[Spell]` sections, the last being the empty editor template.
        assert_eq!(dt.spells.len(), 33);
        assert!(dt.spells.iter().enumerate().all(|(i, s)| s.id == i as u32 + 1));
        assert!(dt.spells.iter().all(|s| s.school.is_some() && s.target.is_some()));
        let ids: Vec<u32> = dt.units.iter().map(|u| u.id).collect();
        assert_eq!(ids, (1..=102).collect::<Vec<_>>());
        // Hero classes (mechanics.md 7).
        let knight = dt.unit(1).unwrap();
        assert_eq!((knight.hits, knight.attack_blow, knight.defence_blow, knight.defence_shot), (80, 45, 15, 10));
        assert_eq!((knight.initiative, knight.manevres, knight.cost, knight.level_multiplier), (9, 1, 280, 150));
        let mage = dt.unit(2).unwrap();
        assert_eq!((mage.hits, mage.magic_power, mage.manevres), (50, 25, 2));
        assert_eq!(mage.magic, Some(data::MagicSchool::Elemental));
        assert_eq!(mage.magic_direction, Some(data::MagicDirection::ToEnemy));
        let ranger = dt.unit(3).unwrap();
        assert_eq!((ranger.hits, ranger.attack_shot, ranger.regen, ranger.manevres), (65, 30, 5, 2));
        // Every upgrade target resolves to a unit.
        let ups: Vec<_> = dt.units.iter().flat_map(|u| &u.upgrades).collect();
        assert_eq!(ups.len(), 40 + 12 + 4);
        assert!(ups.iter().all(|u| u.target.is_some()), "unresolved upgrade");
        // Vanilla data uses only vanilla bonuses.
        assert!(dt.units.iter().filter_map(|u| u.bonus.as_ref()).all(|b| b.vanilla_index().is_some()));
        assert_eq!(dt.units.iter().filter(|u| u.bonus.is_some()).count(), 59);
        assert_eq!(dt.units.iter().filter(|u| u.nature == data::Nature::Undead).count(), 23);
        assert!(dt.units.iter().all(|u| u.extra.is_empty()), "unknown unit keys");
        // Artefacts.
        let aids: Vec<u32> = dt.artefacts.iter().map(|a| a.id).collect();
        assert!(aids.windows(2).all(|w| w[0] < w[1]));
        assert_eq!(dt.artefacts.iter().filter(|a| a.kind == data::ArtefactType::Potion).count(), 8);
        assert_eq!(dt.artefacts.iter().filter(|a| a.kind == data::ArtefactType::Item).count(), 27);
        assert!(dt.artefacts.iter().all(|a| a.extra.is_empty()), "unknown artefact keys");
        // Commented-out lines are keys like any other to the reader (`//Effect1`); they never
        // match.
        assert!(dt.spells.iter().flat_map(|s| s.extra.keys()).all(|k| k.starts_with("//")), "unknown spell keys");
        // Global options match the documented vanilla values; only the misspelled key is extra.
        let o = &dt.options;
        let d = GlobalOptions::default();
        // The player's settings turn on "impossible difficulty" (OptValue10=1): F = 100.
        assert_eq!(o.difficulty_factor, 100);
        let same = GlobalOptions { ai_targets: d.ai_targets.clone(), army_generation: vec![], extra: d.extra.clone(), difficulty_factor: d.difficulty_factor, ..o.clone() };
        assert_eq!(same, d);
        assert_eq!(o.extra.keys().collect::<Vec<_>>(), ["MixHealingTarget"]);
        assert_eq!(o.ai_targets.min_attack_army, Some([1, 1, 100, 50, 50]));
        assert_eq!(o.ai_targets.min_healing, None);
        assert_eq!(o.army_generation.len(), 8);
    }

    #[test]
    fn real_maps_offer_the_classes_with_a_start_cell() {
        let Some(dt) = install() else { return };
        // Of the shipped maps only Устье Трейна leaves a class without a start cell: its
        // archmage, though that preset still names a start building and a troop. The original
        // greys his portrait and opens the window on the knight (checked live under Wine).
        let mut short: Vec<(String, [bool; 3])> = Vec::new();
        for m in &dt.maps {
            let h = m.load().expect("map loads").header;
            assert!(h.first_offered_class().is_some(), "{}: no class offered", m.name);
            if h.offered_classes() != [true; 3] {
                short.push((m.name.clone(), h.offered_classes()));
            }
        }
        assert_eq!(short.len(), 1, "{short:?}");
        assert!(short[0].0.starts_with("Устье"), "{short:?}");
        assert_eq!(short[0].1, [true, false, true]);
    }

    #[test]
    fn real_maps_parse_and_roundtrip() {
        let Some(dt) = install() else { return };
        assert_eq!(dt.maps.len(), 15);
        for m in &dt.maps {
            let bytes = std::fs::read(&m.path).unwrap();
            let payload = crate::dt::container::decode(&bytes).unwrap_or_else(|e| panic!("{}: {e}", m.name)).payload;
            let s = Scenario::parse_payload(&payload).unwrap_or_else(|e| panic!("{}: {e}", m.name));
            assert!(s.to_payload() == payload, "{}: payload does not roundtrip", m.name);
            assert_eq!(s.terrain.len(), (s.width() * s.height()) as usize);
            assert!(s.terrain.iter().all(|c| *c < 16), "{}", m.name);
            assert!(s.armies.iter().enumerate().all(|(i, a)| a.id as usize == i + 1), "{}", m.name);
            assert!(s.points.iter().enumerate().all(|(i, p)| p.id as usize == i + 1), "{}", m.name);
            assert!(s.buildings.iter().all(|b| b.building_type().is_some()), "{}", m.name);
            assert!(s.events.iter().all(|e| e.kind().is_some()), "{}", m.name);
            for a in &s.armies {
                assert!(a.troops().all(|t| dt.unit(t.unit as u32).is_some()), "{}", m.name);
            }
        }
    }

    #[test]
    fn real_map_spot_checks() {
        let Some(dt) = install() else { return };
        let load = |prefix: &str| {
            let m = dt.maps.iter().find(|m| m.name.starts_with(prefix)).expect("map present");
            m.load().unwrap()
        };
        // The reference map of the gameplay video.
        let rk3 = load("РК3");
        assert_eq!((rk3.width(), rk3.height()), (200, 200));
        assert_eq!(rk3.header.start_date(), GameDate { year: 1204, month: 5, day: 20, hour: 9, minute: 0 });
        assert_eq!((rk3.buildings.len(), rk3.armies.len(), rk3.points.len(), rk3.events.len()), (147, 26, 23, 63));
        assert_eq!(rk3.objects.len(), 15783);
        assert_eq!(rk3.named_characters.iter().map(|n| n.unit).collect::<Vec<_>>(), [74, 72, 42, 39, 15, 11, 70]);
        assert_eq!((rk3.header.victory_event, rk3.header.defeat_event, rk3.header.scenario_kind), (32, 35, 2));
        assert_eq!(rk3.header.relations, [[3, 2, 1, -2], [2, 3, 1, -2], [1, 1, 3, 1], [-2, -2, 1, 3]]);
        assert_eq!(rk3.header.carry_over, [1; 7]);
        assert_eq!((rk3.header.hero(Archetype::Knight).x, rk3.header.hero(Archetype::Knight).y), (9, 198));
        assert!(rk3.scenario_picture.is_none());
        let count = |t: BuildingType| rk3.buildings.iter().filter(|b| b.building_type() == Some(t)).count();
        assert_eq!((count(BuildingType::Village), count(BuildingType::StoneBridge), count(BuildingType::Castle)), (39, 31, 22));
        assert_eq!((count(BuildingType::Ruins), count(BuildingType::Town), count(BuildingType::Market)), (18, 2, 2));
        let b1 = &rk3.buildings[0];
        assert_eq!(b1.building_type(), Some(BuildingType::Castle));
        assert_eq!((b1.x, b1.y, b1.size_x, b1.size_y, b1.gold_per_day, b1.gold_max), (52, 15, 4, 4, 55, 325));
        assert_eq!((b1.owner(), b1.faction, b1.garrison_extra_defence, b1.relations), (Some(7), 3, 11, [1, 1, 3, 1]));
        assert_eq!(b1.garrison.iter().filter(|t| !t.is_empty()).count(), 5);
        let b2 = &rk3.buildings[1];
        assert_eq!((b2.building_type(), b2.mana_per_day, b2.mana_max, b2.owner()), (Some(BuildingType::Village), 75, 180, None));
        let a1 = &rk3.armies[0];
        assert_eq!((a1.x, a1.y, a1.model, a1.home_building, a1.leader_unit), (62, 145, 7, 60, 72));
        assert_eq!((a1.gold_income, a1.aggression, a1.patrol_radius, a1.named_character), (1200, -20, 10, 2));
        assert_eq!((a1.tactical_cost_1, a1.tactical_cost_2, a1.unknown_80), (7102, 9174, 144));
        let e1 = &rk3.events[0];
        assert_eq!((e1.kind, e1.start_time, e1.repeat, e1.duration, e1.once), (1, 624_354_300, 1440, 1440, 1));
        assert_eq!(e1.results.light_lanterns, [1, 0, 0, 0]);
        let terrain_count = |c: u8| rk3.terrain.iter().filter(|t| **t == c).count();
        assert_eq!((terrain_count(6), terrain_count(4), terrain_count(0)), (21938, 1923, 2674));

        let rk1 = load("РК1");
        assert_eq!((rk1.width(), rk1.buildings.len(), rk1.armies.len(), rk1.points.len(), rk1.events.len()), (50, 12, 15, 24, 54));
        assert_eq!(rk1.header.start_time, 624_296_700);
        assert_eq!(rk1.scenario_picture.as_ref().map(Vec::len), Some(59152));
        assert!(rk1.scenario_picture.as_ref().unwrap().starts_with(b"LIT\0"));

        // All maps: sizes, section counts and embedded pictures.
        let stats: Vec<(u32, usize, usize, usize, usize, usize)> = dt
            .maps
            .iter()
            .map(|m| {
                let s = m.load().unwrap();
                (s.width(), s.buildings.len(), s.armies.len(), s.points.len(), s.events.len(), s.objects.len())
            })
            .collect();
        assert_eq!(stats.iter().map(|s| s.1).sum::<usize>(), 1082);
        assert_eq!(stats.iter().map(|s| s.2).sum::<usize>(), 403);
        assert_eq!(stats.iter().filter(|s| s.0 == 200).count(), 6);
        assert_eq!(stats.iter().map(|s| s.4).sum::<usize>(), 1300);
        let pictures: usize = dt.maps.iter().map(|m| m.load().unwrap().events.iter().filter(|e| e.custom_picture.is_some()).count()).sum();
        assert_eq!(pictures, 1);
    }
}
