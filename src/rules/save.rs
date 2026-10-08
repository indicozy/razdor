//! Saved games: the whole [`Game`] with serde, as bzip2-compressed JSON.
//!
//! A save holds the game's state only. What comes from the scenario (the map, the texts of
//! buildings, armies and events, the event scripts) and the content (units, items, spells)
//! is left out and rebuilt on load: the save names its scenario ([`ScenarioRef`]: the demo,
//! or a map file of the install with an FNV-1a hash of its bytes), and loading reads that map
//! again from `RAZDOR_DT_DIR`. A missing or changed map is refused ([`SaveError`]).
//!
//! Saves are the player's data. They live in the platform data folder
//! (`$XDG_DATA_HOME/razdor/saves`, `~/.local/share/razdor/saves`, `%APPDATA%\razdor\saves`,
//! …; [`default_dir`]), or where `RAZDOR_SAVE_DIR` points; never in the repo or the game's
//! folder. Manual saves go to `manual/`, autosaves to `auto/`: one before every battle and
//! one at every 12:00 report, named by the in-game date as in the original ("1204.06.03,
//! 12 h"). There are at most [`AUTOSAVES_KEPT`] autosaves, reused as the original reuses
//! them ([`write_autosave`]).

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::dt::dtm::Scenario;
use crate::i18n::tr;
use crate::trf;

use super::clock::Clock;
use super::content::{Content, ItemId, UnitId};
use super::events::EventEngine;
use super::game::Game;
use super::rng::{self, EventRng, Rng};
use super::world::World;

/// Bumped when the saved state changes shape. 2: the random generator is no longer saved
/// (as in the original, a load starts it afresh); version 1 saves still load, their saved
/// generator ignored. 3: armies keep a talk counter towards the hero and the hero a ship just
/// bought and an event's speed; older saves load with none. 4: the AI's own state (unit
/// records with worn items, deaths and pay; scores, talk counters, wander points, noon;
/// buildings' attitudes to every faction); older saves load with the AI set up afresh. 5: an
/// AI army's place on its path (the original's path index) and a free first step, and the
/// time of death a unit raised again keeps; older saves load at the path's start, with
/// none kept. 6: the player's stored income, the mana-short flag, a unit's garrison stamp,
/// and the markets' 12 places and restock timer; older saves load with none stored, the
/// flag down, no stamps, and their goods in order with the timer due. 7: world spells in
/// each unit's 4 slots, a unit's drain and HP carry, the map's start minute; older saves
/// load with their army-wide spells moved into the units' slots, no drain (a draining curse
/// becomes a slot like any other) and the start day's midnight as the map start. 8: the
/// event engine's flags as the original's one string (older saves' list of names is read
/// into it), its *last fired* a minute ahead until the scan goes idle, the ask and once
/// bytes a Yes rewrites, opcode 18's waiting digit and the tutorial's end mark; older saves
/// load with their flags joined, no digit waiting and the mark down. 9: the wide front row
/// the game was started with; older saves load wide, as they were all made.
pub const FORMAT_VERSION: u32 = 9;
/// The oldest format still read.
pub const OLDEST_VERSION: u32 = 1;
pub const EXTENSION: &str = "rzsave";
/// Overrides the save folder (tests, portable installs).
pub const DIR_ENV: &str = "RAZDOR_SAVE_DIR";
/// Autosaves kept: 30, Razdor's (the original keeps 12), reused as the original reuses them.
pub const AUTOSAVES_KEPT: usize = 30;
const MANUAL_DIR: &str = "manual";
const AUTO_DIR: &str = "auto";

/// Which scenario a game plays.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScenarioRef {
    /// The built-in demo (our own content, always available).
    Demo,
    /// A map of the install: its file name without the extension and a hash of its bytes.
    Map { file: String, hash: u64 },
}

impl ScenarioRef {
    /// The reference of map file `path` (named `file` without the extension).
    pub fn of_map(path: &Path, file: &str) -> std::io::Result<ScenarioRef> {
        Ok(ScenarioRef::Map { file: file.to_string(), hash: fnv1a(&std::fs::read(path)?) })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveKind {
    Manual,
    Auto,
}

/// What the load screen shows of a save.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SaveMeta {
    pub version: u32,
    pub kind: SaveKind,
    /// The save's name: the player's for manual saves, "Battle - …" or the date for autosaves.
    pub name: String,
    pub scenario: ScenarioRef,
    /// The scenario's title (the demo's, or the map's) and the hero's class.
    pub title: String,
    pub hero: String,
    /// In-game date and time ([`Clock::label`]).
    pub date: String,
    /// Real time of saving, seconds since 1970.
    pub saved_at: u64,
    /// The cheat console was used in this game (`rules::cheats`); older saves: no.
    #[serde(default)]
    pub cheats: bool,
}

#[derive(Debug)]
pub enum SaveError {
    Io(std::io::Error),
    /// Not a save, or a damaged one.
    Corrupt(String),
    /// Written by another version of the save format.
    Version(u32),
    /// The game does not know which scenario it plays.
    NoScenario,
    /// A map save, but `RAZDOR_DT_DIR` is not set or has no maps folder.
    NoInstall,
    /// The map file is gone from the install.
    MapMissing(String),
    /// The map file's bytes differ from the saved game's.
    MapChanged(String),
    /// The saved game does not fit the map or the install's data any more.
    Mismatch(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(e) => f.write_str(&trf!("cannot read or write the save: {e}", e)),
            SaveError::Corrupt(e) => f.write_str(&trf!("the save is damaged: {e}", e)),
            SaveError::Version(v) => f.write_str(&trf!("the save is from another version (format {v}, this one reads {current})", v, current = FORMAT_VERSION)),
            SaveError::NoScenario => f.write_str(tr("this game's scenario is unknown, it cannot be saved")),
            SaveError::NoInstall => f.write_str(tr("this save needs your Discord Times install: set RAZDOR_DT_DIR")),
            SaveError::MapMissing(m) => f.write_str(&trf!("the map \"{m}\" is not in your install any more", m)),
            SaveError::MapChanged(m) => f.write_str(&trf!("the map \"{m}\" has changed since the game was saved", m)),
            SaveError::Mismatch(e) => f.write_str(&trf!("the save does not fit the map or the data: {e}", e)),
        }
    }
}

impl std::error::Error for SaveError {}

impl From<std::io::Error> for SaveError {
    fn from(e: std::io::Error) -> SaveError {
        SaveError::Io(e)
    }
}

/// 64-bit FNV-1a: a stable hash of a map's bytes (stable across Rust versions, unlike std's).
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ b as u64).wrapping_mul(0x0100_0000_01b3))
}

/// The original's autosave name for a moment: "1204.06.03, 12 h" ("…, 12 час" in Russian),
/// the month 1-based, the day 0-based, both of two digits. Razdor fixes the original's
/// padding bug: it padded the day only when its index was below 9 (0x49ce20 tests the index
/// against 9, not 10), so day 9 printed as `9`.
pub fn date_name(clock: &Clock) -> String {
    // Under an hour since year 0 (a map whose header time is 0) the original names it by its
    // "less than an hour" text instead.
    if clock.total_minutes() < 60.0 {
        return tr(LESS_THAN_AN_HOUR).to_string();
    }
    let date = format!("{}.{:02}.{:02}", clock.year(), clock.month(), clock.day());
    crate::trf!("{date}, {hour} h", date, hour = clock.hour())
}

/// The autosave name of a moment under an hour since year 0 (`[Time] cLessAtHour`).
pub const LESS_THAN_AN_HOUR: &str = crate::i18n::n_("Less than an hour");

/// The opponent's part of a battle autosave's name (0x4b7410 → 0x4973a0): the army's or the
/// building's name (an army's leader name is not used), cut at its first `#` and its
/// trailing spaces trimmed. Two foes whose names differ only after a `#` share an autosave.
pub fn autosave_foe(name: &str) -> &str {
    name.split_once('#').map_or(name, |(head, _)| head).trim_end_matches(' ')
}

/// The save folder: `RAZDOR_SAVE_DIR`, else `razdor/saves` in the platform data folder.
pub fn default_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    dirs::data_dir().map(|d| d.join("razdor").join("saves"))
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn now_millis() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// The meta of `game` saved now as `name`.
pub fn meta_of(game: &Game, kind: SaveKind, name: &str) -> Result<SaveMeta, SaveError> {
    let scenario = game.origin.clone().ok_or(SaveError::NoScenario)?;
    Ok(SaveMeta {
        version: FORMAT_VERSION,
        kind,
        name: name.to_string(),
        scenario,
        title: game.world.title.clone(),
        hero: game.hero().name(&game.content).to_string(),
        date: game.clock.label(),
        saved_at: now_secs(),
        cheats: game.cheats.used,
    })
}

#[derive(Serialize)]
struct SaveOut<'a> {
    meta: &'a SaveMeta,
    game: &'a Game,
}

#[derive(Deserialize)]
struct SaveIn {
    meta: SaveMeta,
    game: Game,
}

#[derive(Deserialize)]
struct MetaIn {
    meta: SaveMeta,
}

/// The save file's bytes: bzip2-compressed JSON of the meta and the game.
pub fn encode(meta: &SaveMeta, game: &Game) -> Result<Vec<u8>, SaveError> {
    let json = serde_json::to_vec(&SaveOut { meta, game }).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    enc.write_all(&json)?;
    Ok(enc.finish()?)
}

fn unpack(bytes: &[u8]) -> Result<Vec<u8>, SaveError> {
    let mut json = Vec::new();
    bzip2::read::BzDecoder::new(bytes).read_to_end(&mut json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    Ok(json)
}

fn check_version(meta: &SaveMeta) -> Result<(), SaveError> {
    if !(OLDEST_VERSION..=FORMAT_VERSION).contains(&meta.version) {
        return Err(SaveError::Version(meta.version));
    }
    Ok(())
}

/// Reads a save's bytes: its meta and the game state, not yet restored (see [`restore`]).
pub fn decode(bytes: &[u8]) -> Result<(SaveMeta, Game), SaveError> {
    let json = unpack(bytes)?;
    let meta: MetaIn = serde_json::from_slice(&json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    check_version(&meta.meta)?;
    let s: SaveIn = serde_json::from_slice(&json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    Ok((s.meta, s.game))
}

/// Only the meta of a save file (for the load screen).
pub fn read_meta(path: &Path) -> Result<SaveMeta, SaveError> {
    let json = unpack(&std::fs::read(path)?)?;
    let m: MetaIn = serde_json::from_slice(&json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    Ok(m.meta)
}

/// The player's data folder for a kind of save.
fn kind_dir(dir: &Path, kind: SaveKind) -> PathBuf {
    dir.join(match kind {
        SaveKind::Manual => MANUAL_DIR,
        SaveKind::Auto => AUTO_DIR,
    })
}

/// A file name from a save name: letters and digits kept, the rest `_`.
fn slug(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { '_' }).take(60).collect();
    if s.trim_matches('_').is_empty() {
        "save".to_string()
    } else {
        s
    }
}

/// Writes `game` into save folder `dir` as `name`. A manual save of the same name is
/// replaced; an autosave goes where [`write_autosave`] puts one outside a battle. Returns the
/// file written.
pub fn write(dir: &Path, kind: SaveKind, name: &str, game: &Game) -> Result<PathBuf, SaveError> {
    match kind {
        SaveKind::Manual => {
            let folder = kind_dir(dir, kind);
            std::fs::create_dir_all(&folder)?;
            put(&folder.join(format!("{}.{EXTENSION}", slug(name))), kind, name, game)
        }
        SaveKind::Auto => write_autosave(dir, name, game, false),
    }
}

/// Writes an autosave as the original picks its slot (0x4b7410): in a battle the autosave
/// of the same name is reused, otherwise the one of the same name **and** the same map title
/// (when several match, the last in the list, newest first, so the oldest of them); with no
/// match a new one is made while there are fewer than [`AUTOSAVES_KEPT`], else the last of
/// the list, the oldest, is overwritten.
pub fn write_autosave(dir: &Path, name: &str, game: &Game, in_battle: bool) -> Result<PathBuf, SaveError> {
    let folder = kind_dir(dir, SaveKind::Auto);
    std::fs::create_dir_all(&folder)?;
    let saves = list(dir, SaveKind::Auto);
    let same = |e: &&SaveEntry| e.meta.name == name && (in_battle || e.meta.title == game.world.title);
    let reused = saves.iter().filter(same).last().or_else(|| saves.get(AUTOSAVES_KEPT - 1));
    let path = match reused {
        Some(e) => e.path.clone(),
        None => {
            // Millisecond stamps keep the names apart; a counter, two in the same millisecond.
            let stamp = now_millis();
            (0..).map(|k| folder.join(format!("{stamp:015}-{k:02}.{EXTENSION}"))).find(|p| !p.exists()).expect("a free name")
        }
    };
    put(&path, SaveKind::Auto, name, game)
}

/// Writes the save file `path`: aside first, then moved into place, so a crash never leaves
/// half a save.
fn put(path: &Path, kind: SaveKind, name: &str, game: &Game) -> Result<PathBuf, SaveError> {
    let meta = meta_of(game, kind, name)?;
    let bytes = encode(&meta, game)?;
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    Ok(path.to_path_buf())
}

/// The name of the quick save (F5): a manual save that each quick save replaces.
pub const QUICK_SAVE: &str = crate::i18n::n_("Quick save");

/// Writes the quick save (F5), replacing the last one.
pub fn quick_save(dir: &Path, game: &Game) -> Result<PathBuf, SaveError> {
    write(dir, SaveKind::Manual, QUICK_SAVE, game)
}

/// The quick save's file (F9), if there is one.
pub fn quick_save_path(dir: &Path) -> Option<PathBuf> {
    let path = kind_dir(dir, SaveKind::Manual).join(format!("{}.{EXTENSION}", slug(QUICK_SAVE)));
    path.is_file().then_some(path)
}

/// A save found in the folder.
#[derive(Clone, Debug)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub meta: SaveMeta,
}

/// Save files of one kind, newest first. Unreadable files are skipped.
pub fn list(dir: &Path, kind: SaveKind) -> Vec<SaveEntry> {
    let Ok(read) = std::fs::read_dir(kind_dir(dir, kind)) else { return Vec::new() };
    let mut v: Vec<SaveEntry> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|path| Some(SaveEntry { meta: read_meta(&path).ok()?, path }))
        .collect();
    // Within a second, the file written last first (a reused autosave keeps an old name).
    let written = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    v.sort_by(|a, b| b.meta.saved_at.cmp(&a.meta.saved_at).then_with(|| written(&b.path).cmp(&written(&a.path))).then_with(|| b.path.cmp(&a.path)));
    v
}

/// Where a map save finds its scenario: the install folder and its content.
pub struct Install<'a> {
    pub dir: &'a Path,
    pub content: Arc<Content>,
}

/// Reads and restores a save file (see [`restore`]).
pub fn load(path: &Path, demo: Arc<Content>, install: Option<&Install>) -> Result<Game, SaveError> {
    let (meta, game) = decode(&std::fs::read(path)?)?;
    restore(&meta, game, demo, install)
}

/// Rebuilds what the save left out: the content, and the world's and the event engine's
/// statics from the scenario (the demo, or the map file of the install, which must be the
/// very file the game was saved with).
pub fn restore(meta: &SaveMeta, mut game: Game, demo: Arc<Content>, install: Option<&Install>) -> Result<Game, SaveError> {
    check_version(meta)?;
    // What the load sequence draws from (`rng::Rng::save_load`): the plant layer and the
    // number of armies of the map file.
    let (content, fresh, engine, plants, armies) = match &meta.scenario {
        ScenarioRef::Demo => {
            let world = World::standard(&demo);
            let m = &world.map;
            let plants = rng::plant_layer(m.w, m.h, m.objects.iter().map(|o| (o.tile.0, o.tile.1, o.class, o.sprite)));
            let armies = world.armies.len() + world.inactive.len();
            (demo, world, None, plants, armies)
        }
        ScenarioRef::Map { file, hash } => {
            let install = install.ok_or(SaveError::NoInstall)?;
            let maps = crate::dt::install::list_maps(install.dir).map_err(|_| SaveError::NoInstall)?;
            let entry = maps.iter().find(|m| &m.name == file).ok_or_else(|| SaveError::MapMissing(file.clone()))?;
            let bytes = std::fs::read(&entry.path)?;
            if fnv1a(&bytes) != *hash {
                return Err(SaveError::MapChanged(file.clone()));
            }
            let scenario = Scenario::from_file_bytes(&bytes).map_err(|e| SaveError::Mismatch(e.to_string()))?;
            let world = World::from_scenario(&scenario, &install.content);
            let (w, h) = (scenario.header.width as i32, scenario.header.height as i32);
            let plants = rng::plant_layer(w, h, scenario.objects.iter().map(|o| (o.x as i32, o.y as i32, o.class, o.sprite)));
            (install.content.clone(), world, Some(EventEngine::new(&scenario)), plants, scenario.armies.len())
        }
    };
    if (game.fog.w, game.fog.h) != (fresh.map.w, fresh.map.h) {
        return Err(SaveError::Mismatch(tr("the map's size differs").into()));
    }
    game.world.restore_statics(fresh).map_err(SaveError::Mismatch)?;
    match (game.script.as_deref_mut(), engine) {
        (Some(e), Some(fresh)) => e.restore_statics(fresh).map_err(SaveError::Mismatch)?,
        (None, _) => {}
        (Some(_), None) => return Err(SaveError::Mismatch(tr("events saved for the demo").into())),
    }
    let (rng, music) = Rng::save_load_with_music(game.world.map.w, &plants, armies);
    game.rng = rng;
    game.music_wait = Some(90_000 + music as u32);
    game.event_rng = EventRng::from_clock();
    // The row width the game was saved with holds for it (0x4b771c), whatever the option
    // says now.
    let wide = super::formation::Formation::WIDE;
    game.content = if game.wide_row == (content.formation == wide) {
        content
    } else {
        Arc::new(content.with_formation(if game.wide_row { wide } else { super::formation::Formation::VANILLA }))
    };
    game.origin = Some(meta.scenario.clone());
    // The AI is set up again on every load (0x4a1ff0 from the save loader).
    game.ai_init(meta.version >= 4);
    // Saves from before the units' spell slots: the army-wide spells go into the slots.
    if meta.version < 7 {
        migrate_spells(&mut game);
    }
    // Saves from before the last-pay minute count everyone as paid now.
    let now = game.clock.total_minutes() as u64;
    for u in game.squad.iter_mut().filter(|u| u.last_paid == 0) {
        u.last_paid = now;
    }
    check_content(&game).map_err(SaveError::Mismatch)?;
    Ok(game)
}

/// Moves the army-wide spells of a save before format 7 into the units' slots (a spell that
/// never ended runs to the Community opcode's end).
fn migrate_spells(game: &mut Game) {
    let end = game.opcode_spell_end();
    let old = std::mem::take(&mut game.old_effects);
    super::magic::migrate_old_spells(&old, &mut game.squad, end);
    let c = game.content.clone();
    let w = &mut game.world;
    for a in w.armies.iter_mut().chain(w.inactive.iter_mut()) {
        let old = std::mem::take(&mut a.old_effects);
        let mut units: Vec<_> = a.troops.iter().map(|t| super::game::troop_unit(&c, t)).collect();
        super::magic::migrate_old_spells(&old, &mut units, end);
        for (t, u) in a.troops.iter_mut().zip(&units) {
            t.spells = u.spells;
        }
    }
}

/// Every unit, item and spell the game refers to exists in its content.
fn check_content(g: &Game) -> Result<(), String> {
    let c = &g.content;
    let unit = |id: UnitId| c.try_unit(id).map(|_| ()).ok_or_else(|| trf!("unknown unit {id}", id = id.0));
    let item = |id: ItemId| c.try_item(id).map(|_| ()).ok_or_else(|| trf!("unknown item {id}", id = id.0));
    let w = &g.world;
    let stationed = w.locations.iter().flat_map(|l| l.stationed.iter().map(|s| &s.unit));
    for u in g.squad.iter().chain(stationed) {
        unit(u.def)?;
        u.items.iter().flatten().chain(&u.potions).try_for_each(|&i| item(i))?;
    }
    g.pack.iter().try_for_each(|&i| item(i))?;
    let armies = w.armies.iter().chain(w.inactive.iter()).chain(w.respawns.iter().map(|r| &r.army));
    let troops = w.locations.iter().flat_map(|l| l.garrison.iter()).chain(armies.clone().flat_map(|a| a.troops.iter())).chain(w.gang.iter());
    for t in troops {
        unit(t.unit)?;
    }
    for l in &w.locations {
        l.recruits.iter().try_for_each(|r| unit(r.unit))?;
        l.treasure.iter().try_for_each(|&i| item(i))?;
        if let Some(s) = &l.shop {
            s.goods().iter().try_for_each(|&i| item(i))?;
        }
    }
    armies.flat_map(|a| a.items.iter()).try_for_each(|&i| item(i))?;
    if g.squad.is_empty() {
        return Err(tr("no hero").into());
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::rules::battle::Outcome;
    use crate::rules::content::HeroClass;
    use crate::rules::game::Foe;

    /// A fresh, empty folder under the system temp dir for one test (never the player's).
    pub(crate) fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("razdor-test-{}-{name}-{}", std::process::id(), now_millis()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn json(g: &Game) -> String {
        serde_json::to_string(g).unwrap()
    }

    /// The game's state without what a load sets up again for the AI (the armies' minds
    /// and talk counters towards the hero).
    fn without_ai(g: &Game) -> serde_json::Value {
        let mut v = serde_json::to_value(g).unwrap();
        for key in ["armies", "inactive"] {
            for a in v["world"][key].as_array_mut().into_iter().flatten() {
                a.as_object_mut().map(|o| (o.remove("mind"), o.remove("talk")));
            }
        }
        for r in v["world"]["respawns"].as_array_mut().into_iter().flatten() {
            r["army"].as_object_mut().map(|o| (o.remove("mind"), o.remove("talk")));
        }
        v
    }

    /// Save, load, and compare: the state serialises the same, and what was rebuilt matches.
    pub(crate) fn roundtrip(g: &Game, demo: Arc<Content>, install: Option<&Install>) -> Game {
        let meta = meta_of(g, SaveKind::Manual, "test").unwrap();
        let bytes = encode(&meta, g).unwrap();
        let (m, loaded) = decode(&bytes).unwrap();
        assert_eq!(m, meta);
        let loaded = restore(&m, loaded, demo, install).unwrap();
        // A load sets the AI up again (0x4a1ff0): its scores, talk counters and the income
        // its castles add are worked out afresh, so they are left out of the comparison.
        assert_eq!(without_ai(&loaded), without_ai(g), "state differs after a load");
        assert_eq!(loaded.world.title, g.world.title);
        assert_eq!((loaded.world.map.w, loaded.world.map.h), (g.world.map.w, g.world.map.h));
        assert_eq!(loaded.world.events, g.world.events);
        assert_eq!(loaded.world.points, g.world.points);
        let names = |g: &Game| g.world.locations.iter().map(|l| (l.name.clone(), l.description.clone())).collect::<Vec<_>>();
        assert_eq!(names(&loaded), names(g));
        let armies = |g: &Game| g.world.armies.iter().map(|a| (a.name.clone(), a.leader_name.clone())).collect::<Vec<_>>();
        assert_eq!(armies(&loaded), armies(g));
        for t in [(0, 0), (5, 5), (g.world.map.w - 1, g.world.map.h - 1)] {
            assert_eq!(loaded.world.map.minutes(t), g.world.map.minutes(t));
            assert_eq!(loaded.world.location_at(t), g.world.location_at(t));
        }
        assert!(Arc::ptr_eq(&loaded.content, &g.content) || loaded.content.units.len() == g.content.units.len());
        loaded
    }

    fn demo() -> Arc<Content> {
        Arc::new(Content::builtin())
    }

    fn walk(g: &mut Game, frames: usize) {
        for _ in 0..frames {
            if !g.moving() {
                break;
            }
            g.tick(0.05);
            if g.foe.is_some() {
                break;
            }
        }
    }

    fn fight(g: &mut Game) {
        let mut b = g.start_battle();
        b.begin();
        let mut steps = 0;
        while b.outcome() == Outcome::Ongoing && steps < 5000 {
            b.ai_step();
            steps += 1;
        }
        g.resolve_battle(&b);
        g.drain_events();
    }

    #[test]
    fn fnv_and_date_names() {
        assert_eq!(fnv1a(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(date_name(&Clock::at(1204, 6, 3, 12)), "1204.06.03, 12 h");
        // Day 9 is padded too (the original's bug, 0x49ce20, printed `9`).
        assert_eq!(date_name(&Clock::at(1204, 6, 8, 7)), "1204.06.08, 7 h");
        assert_eq!(date_name(&Clock::at(1204, 10, 9, 12)), "1204.10.09, 12 h");
        assert_eq!(date_name(&Clock::at(1204, 9, 29, 0)), "1204.09.29, 0 h");
        // Under 60 minutes since year 0 the "less than an hour" text (a map starting at 0).
        assert_eq!(date_name(&Clock::at_minutes(59)), "Less than an hour");
        assert_eq!(date_name(&Clock::at_minutes(60)), "0.01.00, 1 h");
        assert_eq!(autosave_foe("Bandits #the second"), "Bandits");
        assert_eq!(autosave_foe(" Old fort  "), " Old fort");
        assert_eq!(autosave_foe("#hidden"), "");
        assert_eq!(slug("My game: day 3!"), "My_game__day_3_");
        assert_eq!(slug("Битва - Замок"), "Битва_-_Замок");
        assert_eq!(slug("///"), "save");
    }

    #[test]
    fn demo_roundtrip_after_walking_and_a_battle() {
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Archmage);
        roundtrip(&g, c.clone(), None);
        g.hire(crate::rules::world::demo_unit(&g.content, "spearman")).unwrap();
        g.set_destination(g.world.locations[g.world.index_of("Millbrook")].tile);
        walk(&mut g, 400);
        g.wait(4);
        g.foe = Some(Foe::Garrison(g.world.index_of("Bandit camp")));
        fight(&mut g);
        g.gold += 5;
        let loaded = roundtrip(&g, c.clone(), None);
        // The generator is not saved: every load of the save starts it the same way, from
        // the load sequence. With that state, the loaded game goes on exactly like the
        // original.
        assert_eq!(roundtrip(&g, c.clone(), None).rng.state(), loaded.rng.state());
        let (mut a, mut b) = (g, loaded);
        a.rng = b.rng.clone();
        // A load sets the AI up again (0x4a1ff0); so does the game played on.
        a.ai_init(true);
        for g in [&mut a, &mut b] {
            g.wait(30);
        }
        assert_eq!(json(&a), json(&b));
    }

    #[test]
    fn saves_are_written_listed_and_read_back() {
        let dir = temp_dir("list");
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        let p = write(&dir, SaveKind::Manual, "First", &g).unwrap();
        assert!(p.starts_with(dir.join("manual")));
        g.gold = 999;
        write(&dir, SaveKind::Manual, "First", &g).unwrap();
        write(&dir, SaveKind::Manual, "Second", &g).unwrap();
        let saves = list(&dir, SaveKind::Manual);
        assert_eq!(saves.len(), 2, "the same name is replaced");
        assert_eq!(saves[0].meta.hero, g.hero().name(&g.content));
        assert_eq!(saves[0].meta.title, "Demo kingdom");
        assert_eq!(saves[0].meta.date, g.clock.label());
        let first = saves.iter().find(|s| s.meta.name == "First").unwrap();
        let loaded = load(&first.path, c, None).unwrap();
        assert_eq!(loaded.gold, 999);
        assert!(list(&dir, SaveKind::Auto).is_empty());
        std::fs::write(dir.join("manual").join("junk.rzsave"), b"not a save").unwrap();
        assert_eq!(list(&dir, SaveKind::Manual).len(), 2, "unreadable files are skipped");
        assert!(matches!(load(&dir.join("manual").join("junk.rzsave"), demo(), None), Err(SaveError::Corrupt(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_quick_save_replaces_the_last_and_is_found_again() {
        let dir = temp_dir("quick");
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        assert_eq!(quick_save_path(&dir), None);
        write(&dir, SaveKind::Manual, "Mine", &g).unwrap();
        let first = quick_save(&dir, &g).unwrap();
        g.gold = 4321;
        let second = quick_save(&dir, &g).unwrap();
        assert_eq!(first, second, "the same file");
        let saves = list(&dir, SaveKind::Manual);
        assert_eq!(saves.len(), 2, "one quick save next to the player's own");
        assert_eq!(saves.iter().filter(|s| s.meta.name == QUICK_SAVE).count(), 1);
        let path = quick_save_path(&dir).unwrap();
        assert_eq!(path, second);
        assert_eq!(load(&path, c, None).unwrap().gold, 4321, "the newest quick save");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_journal_history_survives_a_save_and_old_saves_load_without_one() {
        use crate::rules::journal::EntryKind;
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        g.journal.record(EntryKind::Quest, 2, 100, "A quest", "Its text");
        g.journal.record(EntryKind::Rumour, 5, 160, "A rumour", "Whispers");
        g.journal.next_chapter();
        let loaded = roundtrip(&g, c.clone(), None);
        assert_eq!(loaded.journal, g.journal);
        // A save written before the history existed.
        let meta = meta_of(&g, SaveKind::Manual, "old").unwrap();
        let mut value = serde_json::to_value(&g).unwrap();
        value.as_object_mut().unwrap().remove("journal");
        let old: Game = serde_json::from_value(value).unwrap();
        let loaded = restore(&meta, old, c, None).unwrap();
        assert!(loaded.journal.entries.is_empty());
    }

    #[test]
    fn autosaves_reuse_slots_as_the_original() {
        // 0x4b7410: the same name (and map) is reused; with no match a new one while there
        // are fewer than AUTOSAVES_KEPT (Razdor 30, the original 12), else the oldest is overwritten.
        let dir = temp_dir("auto");
        let g = Game::new(demo(), HeroClass::Ranger);
        let names = |dir: &Path| list(dir, SaveKind::Auto).iter().map(|s| s.meta.name.clone()).collect::<Vec<_>>();
        for k in 0..AUTOSAVES_KEPT {
            write(&dir, SaveKind::Auto, &format!("auto {k}"), &g).unwrap();
        }
        assert_eq!(names(&dir).len(), AUTOSAVES_KEPT);
        write(&dir, SaveKind::Auto, "auto 3", &g).unwrap();
        let now = names(&dir);
        assert_eq!((now.len(), now[0].as_str(), now.iter().filter(|n| *n == "auto 3").count()), (AUTOSAVES_KEPT, "auto 3", 1), "reused");
        write(&dir, SaveKind::Auto, "new", &g).unwrap();
        let now = names(&dir);
        assert_eq!((now.len(), now[0].as_str()), (AUTOSAVES_KEPT, "new"));
        assert!(!now.contains(&"auto 0".to_string()), "the oldest was overwritten");
        // Outside a battle the map title must match too; before a battle the name alone.
        let mut other = Game::new(demo(), HeroClass::Ranger);
        other.world.title = "Another map".into();
        write_autosave(&dir, "auto 5", &other, false).unwrap();
        let now = names(&dir);
        assert_eq!(now.iter().filter(|n| *n == "auto 5").count(), 2, "a new slot, over the oldest (auto 1)");
        assert!(!now.contains(&"auto 1".to_string()));
        write_autosave(&dir, "auto 6", &other, true).unwrap();
        let now = names(&dir);
        assert_eq!((now.len(), now.iter().filter(|n| *n == "auto 6").count(), now.contains(&"auto 2".to_string())), (AUTOSAVES_KEPT, 1, true));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn noon_asks_for_an_autosave_named_by_the_date() {
        let mut g = Game::new(demo(), HeroClass::Knight);
        g.first_noon_today();
        g.world.armies.clear();
        g.wait(3);
        assert_eq!(g.autosave_due, None);
        g.wait(1);
        assert_eq!(g.autosave_due.as_deref(), Some("1200.04.00, 12 h"));
    }

    #[test]
    fn a_load_keeps_the_row_width_the_game_was_saved_with() {
        // 0x4b771c: header byte 0x121 sets the wide row for the loaded game, not the option.
        use crate::rules::formation::Formation;
        let narrow = Arc::new(demo().with_formation(Formation::VANILLA));
        let g = Game::new(narrow, HeroClass::Knight);
        let loaded = roundtrip(&g, demo(), None);
        assert_eq!((loaded.content.formation, demo().formation), (Formation::VANILLA, Formation::WIDE));
        // A save before format 9 was made wide.
        let mut v = serde_json::to_value(&g).unwrap();
        v.as_object_mut().unwrap().remove("wide_row");
        let old: Game = serde_json::from_value(v).unwrap();
        assert!(old.wide_row);
    }

    #[test]
    fn a_noon_without_a_report_asks_for_no_autosave() {
        // 0x4abfbc: the flag is set as the noon report opens; no wages and no income, no
        // report, no autosave.
        use crate::rules::world::testkit;
        let mut s = testkit::scenario(10, 10);
        s.header.heroes[0] = testkit::hero(1, 1, 100, &[]);
        let mut g = Game::from_scenario(Arc::new(testkit::content()), &s, HeroClass::Knight);
        g.drain_events();
        let events = g.wait(30);
        assert!(!events.iter().any(|e| matches!(e, crate::rules::game::Event::NewDay(_))), "{events:?}");
        assert_eq!(g.autosave_due, None);
    }

    #[test]
    fn a_save_of_another_version_or_without_a_scenario_is_refused() {
        let mut g = Game::new(demo(), HeroClass::Knight);
        let mut meta = meta_of(&g, SaveKind::Manual, "x").unwrap();
        meta.version = 99;
        let bytes = encode(&meta, &g).unwrap();
        assert!(matches!(decode(&bytes), Err(SaveError::Version(99))));
        g.origin = None;
        assert!(matches!(meta_of(&g, SaveKind::Manual, "x"), Err(SaveError::NoScenario)));
    }

    #[test]
    fn a_version_1_save_with_its_generator_still_loads() {
        let c = demo();
        let g = Game::new(c.clone(), HeroClass::Knight);
        let mut meta = meta_of(&g, SaveKind::Manual, "old").unwrap();
        meta.version = 1;
        let mut game = serde_json::to_value(&g).unwrap();
        game.as_object_mut().unwrap().insert("rng".into(), serde_json::json!(123_456_789u64));
        let bytes = serde_json::to_vec(&serde_json::json!({ "meta": meta, "game": game })).unwrap();
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        enc.write_all(&bytes).unwrap();
        let (m, loaded) = decode(&enc.finish().unwrap()).unwrap();
        let loaded = restore(&m, loaded, c, None).unwrap();
        assert_eq!(json(&loaded), json(&g));
        let fresh = World::standard(&loaded.content);
        let plants = rng::plant_layer(fresh.map.w, fresh.map.h, fresh.map.objects.iter().map(|o| (o.tile.0, o.tile.1, o.class, o.sprite)));
        assert_eq!(loaded.rng.state(), Rng::save_load(fresh.map.w, &plants, fresh.armies.len() + fresh.inactive.len()).state(), "the old saved generator is ignored");
    }

    #[test]
    fn a_version_6_save_moves_its_army_wide_spells_into_the_units_slots() {
        let c = demo();
        let g = Game::new(c.clone(), HeroClass::Knight);
        let mut meta = meta_of(&g, SaveKind::Manual, "old").unwrap();
        meta.version = 6;
        let mut game = serde_json::to_value(&g).unwrap();
        let obj = game.as_object_mut().unwrap();
        obj.remove("map_start");
        obj.insert("effects".into(), serde_json::json!([{ "spell": 2, "until": 900_000_000u64 }, { "spell": 3, "until": null, "leader": true }]));
        let bytes = serde_json::to_vec(&serde_json::json!({ "meta": meta, "game": game })).unwrap();
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
        enc.write_all(&bytes).unwrap();
        let (m, loaded) = decode(&enc.finish().unwrap()).unwrap();
        let loaded = restore(&m, loaded, c, None).unwrap();
        use crate::rules::units::SpellSlot;
        let end = loaded.start_day * crate::rules::clock::MINUTES_PER_DAY + crate::rules::magic::OPCODE_SPELL_END;
        assert_eq!(loaded.squad[0].spells[..2], [Some(SpellSlot { spell: 2, until: 900_000_000 }), Some(SpellSlot { spell: 3, until: end })]);
        assert!(loaded.squad[1..].iter().all(|u| u.spells[0] == Some(SpellSlot { spell: 2, until: 900_000_000 }) && u.spells[1].is_none()));
        assert!(loaded.old_effects.is_empty());
    }

    #[test]
    fn map_saves_need_the_same_map_file() {
        use crate::rules::world::testkit as tk;
        // A fake install: a maps folder with one hand-built map.
        let dir = temp_dir("maps");
        let maps = dir.join(crate::dt::install::MAPS_DIR);
        std::fs::create_dir_all(&maps).unwrap();
        let mut s = tk::scenario(24, 6);
        s.header.heroes[0] = tk::hero(2, 2, 200, &[tk::troop(4, 0, 2)]);
        let path = maps.join("Test.DTm");
        std::fs::write(&path, s.to_payload()).unwrap();
        let content = Arc::new(tk::content());
        let scenario = Scenario::load(&path).unwrap();
        let mut g = Game::from_scenario(content.clone(), &scenario, HeroClass::Knight);
        g.set_origin(ScenarioRef::of_map(&path, "Test").unwrap());
        g.set_destination((10, 3));
        walk(&mut g, 200);
        let install = Install { dir: &dir, content: content.clone() };
        roundtrip(&g, demo(), Some(&install));

        let meta = meta_of(&g, SaveKind::Manual, "x").unwrap();
        let bytes = encode(&meta, &g).unwrap();
        let again = || decode(&bytes).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), None), Err(SaveError::NoInstall)));
        // Changed: one more gold in the preset.
        s.header.heroes[0].gold += 1;
        std::fs::write(&path, s.to_payload()).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), Some(&install)), Err(SaveError::MapChanged(m)) if m == "Test"));
        std::fs::rename(&path, maps.join("Other.DTm")).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), Some(&install)), Err(SaveError::MapMissing(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn rk1() -> Option<(crate::dt::install::DtInstall, crate::dt::install::MapEntry)> {
        let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
        let dt = crate::dt::install::DtInstall::load(Path::new(&dir)).expect("install loads");
        let m = dt.maps.iter().find(|m| m.name.starts_with("РК1")).expect("РК1 in the install").clone();
        Some((dt, m))
    }

    #[test]
    fn rk1_roundtrip_after_walking_and_a_battle() {
        let Some((dt, entry)) = rk1() else { return };
        let content = Arc::new(Content::from_dt(&dt));
        let scenario = entry.load().unwrap();
        let mut g = Game::from_scenario(content.clone(), &scenario, HeroClass::Knight);
        g.set_origin(ScenarioRef::of_map(&entry.path, &entry.name).unwrap());
        g.drain_events();
        let install = Install { dir: &dt.dir, content: content.clone() };
        roundtrip(&g, demo(), Some(&install));
        // Walk towards the nearest building, then fight the nearest army.
        let here = g.tile();
        let target = g
            .world
            .locations
            .iter()
            .map(|l| l.tile)
            .filter(|&t| t != here && g.fog.explored(t))
            .min_by_key(|&t| g.world.map.distance(t, here));
        if let Some(t) = target {
            g.set_destination(t);
            walk(&mut g, 300);
            g.drain_events();
        }
        if !g.world.armies.is_empty() {
            g.foe = Some(Foe::Army(0));
            fight(&mut g);
        }
        g.foe = None;
        assert!(g.battles > 0 && g.clock.total_minutes() > scenario.header.start_time as f64, "walked and fought");
        let loaded = roundtrip(&g, demo(), Some(&install));
        // The load starts the generator from the map's last plant and its armies.
        let plants = rng::plant_layer(g.world.map.w, g.world.map.h, scenario.objects.iter().map(|o| (o.x as i32, o.y as i32, o.class, o.sprite)));
        assert!(plants.iter().any(|&p| (9..=11).contains(&(p >> 8))), "РК1 has plants");
        assert_eq!(loaded.rng.state(), Rng::save_load(g.world.map.w, &plants, scenario.armies.len()).state());
        let music = Rng::save_load_with_music(g.world.map.w, &plants, scenario.armies.len()).1;
        assert_eq!(loaded.music_wait, Some(90_000 + music as u32), "the world theme's first change");
        let (mut a, mut b) = (g, loaded);
        a.rng = b.rng.clone();
        // A load sets the AI up again (0x4a1ff0); so does the game played on.
        a.ai_init(true);
        for g in [&mut a, &mut b] {
            g.wait(12);
        }
        assert_eq!(json(&a), json(&b), "the loaded game goes on like the original");
    }
}
