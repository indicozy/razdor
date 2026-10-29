//! Saved games: the whole [`Game`] with serde, as bzip2-compressed JSON.
//!
//! A save holds the game's state only. What comes from the scenario (the map, the texts of
//! buildings, armies and events, the event scripts) and the content (units, items, spells)
//! is left out and rebuilt on load: the save names its scenario ([`ScenarioRef`]: the demo,
//! or a map file of the install with an FNV-1a hash of its bytes). Loading reads that map
//! from `RAZDOR_DT_DIR`, or, when the install lacks it, from the map's bytes the save carries
//! ([`SaveFile::map`], the original's self-contained save, saves-data §12.2). Only a map
//! whose bytes match the hash is used; otherwise the load is refused ([`SaveError`]).
//!
//! Saves are the player's data. They live in the platform data folder
//! (`$XDG_DATA_HOME/razdor/saves`, `~/.local/share/razdor/saves`, `%APPDATA%\razdor\saves`,
//! …; [`default_dir`]), or where `RAZDOR_SAVE_DIR` points; never in the repo or the game's
//! folder; each install's in its own folder there ([`games_dir`]). Manual saves go to
//! `manual/`, autosaves to `auto/`: one before every battle and
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
/// the game was started with; older saves load wide, as they were all made. 10: per-party
/// experimental rules, also carried between campaign maps; older saves use classic rules.
/// 11: duplicate-item restriction, potion limit/counters and each unit's noncombatant mark.
/// 12: six item cells per unit (the multiple rings or amulets rule); older saves hold four,
/// read with two empty cells.
pub const FORMAT_VERSION: u32 = 12;
/// The oldest format still read.
pub const OLDEST_VERSION: u32 = 1;
pub const EXTENSION: &str = "rzsave";
/// Overrides the save folder (tests, portable installs).
pub const DIR_ENV: &str = "RAZDOR_SAVE_DIR";
/// Autosaves kept: 30, Razdor's (the original keeps 12), reused as the original reuses them.
pub const AUTOSAVES_KEPT: usize = 30;
/// The player's own unpinned saves (quick saves not counted): Razdor's limit (the original
/// has 12 slots). A save of a new name is refused when they are full; replacing one always
/// works. Pinned saves are not counted and have no limit: a Razdor deviation the owner asked
/// for (a player's report, 2026-10-17); pinning never fails for lack of room.
pub const MANUAL_SAVES_KEPT: usize = 50;
/// Quick saves (F5), reused oldest first: Razdor's (the original has none).
pub const QUICK_SAVES_KEPT: usize = 5;
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
    /// The reference of map file `path` (named `file` without the extension), with the
    /// file's bytes: the game keeps them so its saves carry the map ([`super::game::Game::set_map`]).
    pub fn of_map(path: &Path, file: &str) -> std::io::Result<(ScenarioRef, Arc<[u8]>)> {
        let bytes: Arc<[u8]> = std::fs::read(path)?.into();
        Ok((ScenarioRef::Map { file: file.to_string(), hash: fnv1a(&bytes) }, bytes))
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
    /// [`MANUAL_SAVES_KEPT`] unpinned saves already: replace, delete or pin one.
    Full,
    /// The save to replace is pinned.
    Pinned,
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Io(e) => f.write_str(&trf!("cannot read or write the save: {e}", e)),
            SaveError::Corrupt(e) => f.write_str(&trf!("the save is damaged: {e}", e)),
            SaveError::Version(v) => f.write_str(&trf!("the save is from another version (format {v}, this one reads {current})", v, current = FORMAT_VERSION)),
            SaveError::NoScenario => f.write_str(tr("this game's scenario is unknown, it cannot be saved")),
            SaveError::NoInstall => f.write_str(tr("this save needs your Discord Times install: set RAZDOR_DT_DIR")),
            // The scenario's map file (its name without the extension) is gone from Maps_Rus.
            SaveError::MapMissing(m) => f.write_str(&trf!("the scenario's map file \"{m}.DTm\" is missing from the game folder (Maps_Rus)", m)),
            SaveError::MapChanged(m) => f.write_str(&trf!("the map \"{m}\" has changed since the game was saved", m)),
            SaveError::Mismatch(e) => f.write_str(&trf!("the save does not fit the map or the data: {e}", e)),
            SaveError::Pinned => f.write_str(tr("that save is pinned: unpin it first")),
            SaveError::Full => f.write_str(&trf!("there are {n} unpinned saves already: save over one, delete one or pin one", n = MANUAL_SAVES_KEPT)),
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
    crate::platform::app_dir().map(|d| d.join("saves"))
}

/// The install the games are played on ([`set_install`]), whose saves [`games_dir`] keeps.
static INSTALL: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

/// The install played from now on: its saves are kept apart from another install's
/// ([`games_dir`]).
pub fn set_install(dir: &Path) {
    *INSTALL.lock().unwrap_or_else(|e| e.into_inner()) = Some(dir.to_path_buf());
}

/// The folder of the games' saves *(Razdor's)*: one per install in [`default_dir`], named
/// by the install's folder and a hash of its path ("Discord Times-1a2b3c4d"), so a mod and
/// the plain game installed side by side do not mix their saves. Without an install,
/// [`default_dir`] itself. The settings stay in [`default_dir`], shared.
pub fn games_dir() -> Option<PathBuf> {
    let root = default_dir()?;
    let Some(install) = INSTALL.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return Some(root) };
    let own = root.join(install_key(&install));
    if !own.exists() {
        claim_old_saves(&root, &own, &install);
    }
    Some(own)
}

/// The name of install `dir`'s save folder: its folder name and the FNV-1a hash of its path.
fn install_key(dir: &Path) -> String {
    let path = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let name: String = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' { c } else { '_' }).collect();
    format!("{}-{:08x}", name.trim(), fnv1a(path.to_string_lossy().as_bytes()) as u32)
}

/// Before saves were kept per install they all lay in `root`: the first time install
/// `install` is played, its folder `own` takes those whose map it has, the same bytes, and
/// the demo's, with their pins. The others wait for their own install.
fn claim_old_saves(root: &Path, own: &Path, install: &Path) {
    let maps: Vec<(String, u64)> = crate::dt::install::list_maps(install)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| Some((m.name, fnv1a(&std::fs::read(&m.path).ok()?))))
        .collect();
    let ours = |s: &ScenarioRef| match s {
        ScenarioRef::Demo => true,
        ScenarioRef::Map { file, hash } => maps.iter().any(|(f, h)| f == file && h == hash),
    };
    for kind in [SaveKind::Manual, SaveKind::Auto] {
        let (from, to) = (kind_dir(root, kind), kind_dir(own, kind));
        if std::fs::create_dir_all(&to).is_err() {
            continue;
        }
        let pinned = pins(&from);
        for e in by_time(root, kind).into_iter().filter(|e| ours(&e.meta.scenario)) {
            let Some(name) = file_name(&e.path) else { continue };
            if std::fs::rename(&e.path, to.join(&name)).is_ok() && pinned.contains(&name) {
                let _ = set_pinned(&to.join(&name), true);
            }
        }
    }
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

/// A save file's JSON: the meta, the game, and the map's bytes (base64) for a map save, as
/// the original's save carries its map (saves-data §12.2). Older saves have no `map`.
#[derive(Serialize)]
struct SaveOut<'a> {
    meta: &'a SaveMeta,
    game: &'a Game,
    #[serde(skip_serializing_if = "Option::is_none")]
    map: Option<String>,
}

#[derive(Deserialize)]
struct SaveIn {
    meta: SaveMeta,
    game: Game,
    #[serde(default)]
    map: Option<String>,
}

#[derive(Deserialize)]
struct MetaIn {
    meta: SaveMeta,
}

/// The save file's bytes: bzip2-compressed JSON of the meta, the game and, for a map save
/// whose map bytes the game holds and that hash to the scenario's, the map's bytes
/// (base64): the save loads without the map file in the install (saves-data §12.2).
pub fn encode(meta: &SaveMeta, game: &Game) -> Result<Vec<u8>, SaveError> {
    pack(&encode_json(meta, game)?)
}

/// The save's JSON, before [`pack`]: the cheap part of [`encode`] (about a millisecond for a
/// map game), which the noon autosave does on the frame while the packing runs on its thread.
fn encode_json(meta: &SaveMeta, game: &Game) -> Result<Vec<u8>, SaveError> {
    let map = embedded_map(game).map(base64_encode);
    serde_json::to_vec(&SaveOut { meta, game, map }).map_err(|e| SaveError::Corrupt(e.to_string()))
}

/// The map bytes a save of `game` carries: those of its map, if they hash to the stored hash.
fn embedded_map(game: &Game) -> Option<&[u8]> {
    match (&game.origin, &game.map_bytes) {
        (Some(ScenarioRef::Map { hash, .. }), Some(bytes)) if fnv1a(bytes) == *hash => Some(bytes),
        _ => None,
    }
}

fn pack(json: &[u8]) -> Result<Vec<u8>, SaveError> {
    let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
    enc.write_all(json)?;
    Ok(enc.finish()?)
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Standard base64 with padding (RFC 4648), for the map bytes in the save's JSON.
fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, &b)| n | (b as u32) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(BASE64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The bytes of [`base64_encode`]'s text; `None` when it is not base64.
fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let text = text.as_bytes();
    if !text.len().is_multiple_of(4) {
        return None;
    }
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    for (k, quad) in text.chunks(4).enumerate() {
        let last = k + 1 == text.len() / 4;
        let pad = quad.iter().rev().take_while(|&&c| c == b'=').count();
        if pad > 2 || (pad > 0 && !last) {
            return None;
        }
        let mut n = 0u32;
        for &c in &quad[..4 - pad] {
            n = n << 6 | BASE64.iter().position(|&b| b == c)? as u32;
        }
        n <<= 6 * pad as u32;
        out.extend_from_slice(&n.to_be_bytes()[1..4 - pad]);
    }
    Some(out)
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
/// The game holds the map's bytes the save carries ([`Game::map_bytes`]), if any.
pub fn decode(bytes: &[u8]) -> Result<(SaveMeta, Game), SaveError> {
    let json = unpack(bytes)?;
    let meta: MetaIn = serde_json::from_slice(&json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    check_version(&meta.meta)?;
    let s: SaveIn = serde_json::from_slice(&json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    let mut game = s.game;
    if let Some(text) = s.map {
        let map = base64_decode(&text).ok_or_else(|| SaveError::Corrupt("the save's map is not base64".into()))?;
        game.map_bytes = Some(map.into());
    }
    Ok((s.meta, game))
}

/// Only the meta of a save file (for the load screen).
pub fn read_meta(path: &Path) -> Result<SaveMeta, SaveError> {
    let stamp = file_stamp(path)?;
    if let Some(meta) = cached_meta(path, stamp) {
        return Ok(meta);
    }
    let meta = read_meta_uncached(path)?;
    remember_meta(path, stamp, &meta);
    Ok(meta)
}

/// The meta read from the file itself: the whole file unpacked, the meta parsed out of it
/// (bzip2 has no way to unpack the first bytes alone: most of the load window's time).
fn read_meta_uncached(path: &Path) -> Result<SaveMeta, SaveError> {
    let json = unpack(&std::fs::read(path)?)?;
    let m: MetaIn = serde_json::from_slice(&json).map_err(|e| SaveError::Corrupt(e.to_string()))?;
    Ok(m.meta)
}

/// What tells a save file's versions apart: its length and its time of writing.
type FileStamp = (u64, Option<std::time::SystemTime>);

fn file_stamp(path: &Path) -> std::io::Result<FileStamp> {
    let m = std::fs::metadata(path)?;
    Ok((m.len(), m.modified().ok()))
}

/// The metas read so far, by file, with the stamp of the file they were read from *(Razdor's)*:
/// the load window, the autosaves and the quick saves list the folder again and again, and
/// unpacking every save each time made the window open late (a player's report).
static META_CACHE: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, (FileStamp, SaveMeta)>>> =
    std::sync::LazyLock::new(Default::default);

fn cached_meta(path: &Path, stamp: FileStamp) -> Option<SaveMeta> {
    let cache = META_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.get(path).filter(|(s, _)| *s == stamp).map(|(_, m)| m.clone())
}

fn remember_meta(path: &Path, stamp: FileStamp, meta: &SaveMeta) {
    META_CACHE.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), (stamp, meta.clone()));
}

/// The files whose meta could not be read (not a save, or a damaged one), with the stamp
/// they had: listed files left out until they change.
static META_FAILED: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<PathBuf, FileStamp>>> =
    std::sync::LazyLock::new(Default::default);

fn failed_meta(path: &Path, stamp: FileStamp) -> bool {
    META_FAILED.lock().unwrap_or_else(|e| e.into_inner()).get(path) == Some(&stamp)
}

/// Reads the meta of a file into the cache (or notes it unreadable).
fn read_into_cache(path: &Path, stamp: FileStamp) {
    if cached_meta(path, stamp).is_some() || failed_meta(path, stamp) {
        return;
    }
    match read_meta_uncached(path) {
        Ok(meta) => remember_meta(path, stamp, &meta),
        Err(_) => {
            META_FAILED.lock().unwrap_or_else(|e| e.into_inner()).insert(path.to_path_buf(), stamp);
        }
    }
}

/// The files waiting for their metas to be read by the [`META_READERS`], first first.
type MetaQueue = (std::sync::Mutex<std::collections::VecDeque<(PathBuf, FileStamp)>>, std::sync::Condvar);

static META_QUEUE: std::sync::LazyLock<MetaQueue> =
    std::sync::LazyLock::new(|| {
        let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, META_READERS);
        for k in 0..threads {
            let _ = std::thread::Builder::new().name(format!("save metas {k}")).spawn(meta_reader);
        }
        Default::default()
    });

/// The threads reading the metas of listed saves at most.
const META_READERS: usize = 4;

fn meta_reader() {
    let (queue, ready) = &*META_QUEUE;
    loop {
        let next = {
            let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                match q.pop_front() {
                    Some(job) => break job,
                    None => q = ready.wait(q).unwrap_or_else(|e| e.into_inner()),
                }
            }
        };
        read_into_cache(&next.0, next.1);
    }
}

/// Asks the reader threads for the metas of `files`, these first (before anything asked
/// earlier); files read already are skipped. Never waits.
fn request_metas<'a>(files: impl IntoIterator<Item = &'a (PathBuf, FileStamp)>) {
    let mut first: Vec<(PathBuf, FileStamp)> = files.into_iter().filter(|(p, s)| cached_meta(p, *s).is_none() && !failed_meta(p, *s)).cloned().collect();
    if first.is_empty() {
        return;
    }
    let (queue, ready) = &*META_QUEUE;
    let mut q = queue.lock().unwrap_or_else(|e| e.into_inner());
    q.retain(|job| !first.contains(job));
    for job in first.drain(..).rev() {
        q.push_front(job);
    }
    ready.notify_all();
}

/// Has the metas of the saves of the install's folder ([`games_dir`]) read on the reader
/// threads, so the load window finds them read when it opens.
pub fn warm_meta_cache() {
    let _ = std::thread::Builder::new().name("save list".into()).spawn(move || {
        let Some(dir) = games_dir() else { return };
        // The autosaves last, so first: the load window opens on them the first time.
        for kind in [SaveKind::Manual, SaveKind::Auto] {
            request_metas(files_of(&dir, kind).iter());
        }
    });
}

/// A save file as the load window lists it before its meta is read *(Razdor's)*: the folder is
/// listed at once and the metas come in from the reader threads, the rows on screen first
/// ([`ListedSave::fill`]).
#[derive(Clone, Debug)]
pub struct ListedSave {
    pub path: PathBuf,
    stamp: FileStamp,
    /// When the file was written, seconds since 1970 (what the row shows until its meta comes).
    pub written: u64,
    pub pinned: bool,
    /// `None` while the meta is being read.
    pub meta: Option<SaveMeta>,
}

/// What [`ListedSave::fill`] found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MetaState {
    Ready,
    Waiting,
    /// Not a save, or a damaged one: the row goes.
    Unreadable,
}

impl ListedSave {
    /// Takes the meta from the cache when the readers have read it.
    pub fn fill(&mut self) -> MetaState {
        if self.meta.is_some() {
            return MetaState::Ready;
        }
        if let Some(m) = cached_meta(&self.path, self.stamp) {
            self.meta = Some(m);
            return MetaState::Ready;
        }
        if failed_meta(&self.path, self.stamp) {
            MetaState::Unreadable
        } else {
            MetaState::Waiting
        }
    }
}

/// The save files of one kind without reading them: pinned first, then the file written last
/// first. The metas known already are filled in; the others are asked of the reader threads
/// ([`want_metas`]).
pub fn list_files(dir: &Path, kind: SaveKind) -> Vec<ListedSave> {
    finish_pending_writes();
    let folder = kind_dir(dir, kind);
    let pins = pins(&folder);
    let mut v: Vec<ListedSave> = files_of(dir, kind)
        .into_iter()
        .filter(|(p, s)| !failed_meta(p, *s))
        .map(|(path, stamp)| {
            let written = stamp.1.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            let pinned = file_name(&path).is_some_and(|n| pins.contains(&n));
            let meta = cached_meta(&path, stamp);
            ListedSave { path, stamp, written, pinned, meta }
        })
        .collect();
    v.sort_by_key(|e| !e.pinned);
    v
}

/// Puts listed saves whose metas are all read in the order of [`list`]: pinned first, then
/// by the time saved (a copied file's own time may differ), the file written last first
/// within a second. Rows still waiting keep their place after the others.
pub fn sort_listed(saves: &mut [ListedSave]) {
    saves.sort_by(|a, b| {
        let at = |e: &ListedSave| e.meta.as_ref().map_or(e.written, |m| m.saved_at);
        b.pinned.cmp(&a.pinned).then_with(|| at(b).cmp(&at(a))).then_with(|| b.stamp.1.cmp(&a.stamp.1)).then_with(|| b.path.cmp(&a.path))
    });
}

/// Asks the reader threads for the metas of `saves` not read yet, in their order.
pub fn want_metas<'a>(saves: impl IntoIterator<Item = &'a ListedSave>) {
    let files: Vec<(PathBuf, FileStamp)> = saves.into_iter().filter(|e| e.meta.is_none()).map(|e| (e.path.clone(), e.stamp)).collect();
    request_metas(files.iter());
}

/// The save files of one kind with their stamps, the file written last first (by name when
/// two were written at once).
fn files_of(dir: &Path, kind: SaveKind) -> Vec<(PathBuf, FileStamp)> {
    let Ok(read) = std::fs::read_dir(kind_dir(dir, kind)) else { return Vec::new() };
    let mut files: Vec<(PathBuf, FileStamp)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|p| Some((file_stamp(&p).ok()?, p)))
        .map(|(s, p)| (p, s))
        .collect();
    files.sort_by(|(a, sa), (b, sb)| sb.1.cmp(&sa.1).then_with(|| b.cmp(a)));
    files
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
/// replaced; a new one is refused when there are [`MANUAL_SAVES_KEPT`] unpinned ones (the
/// pinned ones, however many, are not counted); the quick save's
/// name goes to [`quick_save`]; an autosave goes where [`write_autosave`] puts one outside a
/// battle. Returns the file written.
pub fn write(dir: &Path, kind: SaveKind, name: &str, game: &Game) -> Result<PathBuf, SaveError> {
    match kind {
        SaveKind::Manual if name == QUICK_SAVE => quick_save(dir, game),
        SaveKind::Manual => {
            let folder = kind_dir(dir, kind);
            std::fs::create_dir_all(&folder)?;
            let path = folder.join(format!("{}.{EXTENSION}", slug(name)));
            if is_pinned(&path) {
                return Err(SaveError::Pinned);
            }
            // Pinned saves are not counted (Razdor's deviation, see MANUAL_SAVES_KEPT).
            let own = list(dir, kind).iter().filter(|e| e.meta.name != QUICK_SAVE && !e.pinned).count();
            if !path.exists() && own >= MANUAL_SAVES_KEPT {
                return Err(SaveError::Full);
            }
            put(&path, kind, name, game)
        }
        SaveKind::Auto => write_autosave(dir, name, game, false),
    }
}

/// Writes an autosave as the original picks its slot (0x4b7410): in a battle the autosave
/// of the same name is reused, otherwise the one of the same name **and** the same map title
/// (when several match, the last in the list, newest first, so the oldest of them); with no
/// match a new one is made while there are fewer than [`AUTOSAVES_KEPT`], else the last of
/// the list, the oldest, is overwritten. Pinned autosaves are left out of all of it.
pub fn write_autosave(dir: &Path, name: &str, game: &Game, in_battle: bool) -> Result<PathBuf, SaveError> {
    finish_pending_writes();
    let meta = meta_of(game, SaveKind::Auto, name)?;
    let bytes = encode(&meta, game)?;
    store_autosave(dir, &meta, &bytes, in_battle)
}

/// The noon autosave, written on a writer thread (Razdor's): the frame only takes the save's
/// JSON ([`encode_json`]); the thread packs it, rotates the autosaves as [`write_autosave`]
/// does and writes the file, the same bytes the synchronous path writes. The next autosave
/// joins the writer first, as do [`finish_pending_writes`] (before the game quits, before a
/// save is loaded or the save folder listed) and the listings.
pub fn write_autosave_in_background(dir: PathBuf, name: &str, game: &Game) -> Result<(), SaveError> {
    finish_pending_writes();
    let meta = meta_of(game, SaveKind::Auto, name)?;
    let json = encode_json(&meta, game)?;
    spawn_autosave(dir, meta, json);
    Ok(())
}

/// The writer thread of [`write_autosave_in_background`]; its errors are logged, as the
/// frame's autosave logs its own.
fn spawn_autosave(dir: PathBuf, meta: SaveMeta, json: Vec<u8>) {
    let writer = std::thread::Builder::new().name("autosave".into()).spawn(move || {
        if let Err(e) = pack(&json).and_then(|bytes| store_autosave(&dir, &meta, &bytes, false)) {
            crate::diag!("autosave: {e}");
        }
    });
    match writer {
        Ok(handle) => *PENDING_WRITE.lock().unwrap_or_else(|e| e.into_inner()) = Some(handle),
        Err(e) => crate::diag!("autosave: {e}"),
    }
}

/// The autosave writer in progress, if any (joined by [`finish_pending_writes`]).
static PENDING_WRITE: std::sync::Mutex<Option<std::thread::JoinHandle<()>>> = std::sync::Mutex::new(None);

/// Waits for the background autosave in progress, if any (Razdor's). Called before the game
/// quits, before a save is loaded and before the save folder is listed, so each sees every
/// autosave written. With nothing pending it returns at once.
pub fn finish_pending_writes() {
    let pending = PENDING_WRITE.lock().unwrap_or_else(|e| e.into_inner()).take();
    if let Some(handle) = pending {
        if handle.join().is_err() {
            crate::diag!("autosave: the writer thread panicked");
        }
    }
}

/// Stores a packed autosave (`bytes`) as [`write_autosave`] picks its slot: the rotation reads
/// the autosaves of `dir`. Runs on the writer thread or the frame, never both at once.
fn store_autosave(dir: &Path, meta: &SaveMeta, bytes: &[u8], in_battle: bool) -> Result<PathBuf, SaveError> {
    let folder = kind_dir(dir, SaveKind::Auto);
    std::fs::create_dir_all(&folder)?;
    let saves: Vec<SaveEntry> = by_time(dir, SaveKind::Auto).into_iter().filter(|e| !e.pinned).collect();
    let same = |e: &&SaveEntry| e.meta.name == meta.name && (in_battle || e.meta.title == meta.title);
    let reused = saves.iter().filter(same).next_back().or_else(|| saves.get(AUTOSAVES_KEPT - 1));
    let path = match reused {
        Some(e) => e.path.clone(),
        None => {
            // Millisecond stamps keep the names apart; a counter, two in the same millisecond.
            let stamp = now_millis();
            (0..).map(|k| folder.join(format!("{stamp:015}-{k:02}.{EXTENSION}"))).find(|p| !p.exists()).expect("a free name")
        }
    };
    put_bytes(&path, meta, bytes)
}

/// Writes the save file `path`: aside first, then moved into place, so a crash never leaves
/// half a save.
fn put(path: &Path, kind: SaveKind, name: &str, game: &Game) -> Result<PathBuf, SaveError> {
    let meta = meta_of(game, kind, name)?;
    let bytes = encode(&meta, game)?;
    put_bytes(path, &meta, &bytes)
}

/// Writes the packed save `bytes` of `meta` to `path` (see [`put`]).
fn put_bytes(path: &Path, meta: &SaveMeta, bytes: &[u8]) -> Result<PathBuf, SaveError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)?;
    if let Ok(stamp) = file_stamp(path) {
        remember_meta(path, stamp, meta);
    }
    Ok(path.to_path_buf())
}

/// The name of the quick saves (F5): manual saves kept apart from the player's own.
pub const QUICK_SAVE: &str = crate::i18n::n_("Quick save");

/// The quick saves, newest first, pinned ones too.
fn quick_saves(dir: &Path) -> Vec<SaveEntry> {
    by_time(dir, SaveKind::Manual).into_iter().filter(|e| e.meta.name == QUICK_SAVE).collect()
}

/// Writes a quick save (F5): a new one while there are fewer than [`QUICK_SAVES_KEPT`],
/// else over the oldest. Pinned quick saves are left out of it.
pub fn quick_save(dir: &Path, game: &Game) -> Result<PathBuf, SaveError> {
    let folder = kind_dir(dir, SaveKind::Manual);
    std::fs::create_dir_all(&folder)?;
    let all = quick_saves(dir);
    let quick: Vec<&SaveEntry> = all.iter().filter(|e| !e.pinned).collect();
    let path = match quick.get(QUICK_SAVES_KEPT - 1) {
        Some(oldest) => oldest.path.clone(),
        None => {
            let taken: Vec<&Path> = all.iter().map(|e| e.path.as_path()).collect();
            (1..)
                .map(|k| folder.join(format!("{}-{k}.{EXTENSION}", slug(QUICK_SAVE))))
                .find(|p| !taken.contains(&p.as_path()) && !p.exists())
                .expect("a free name")
        }
    };
    // Quick saves beyond the limit (an older Razdor's) go.
    for extra in quick.iter().skip(QUICK_SAVES_KEPT) {
        let _ = std::fs::remove_file(&extra.path);
    }
    put(&path, SaveKind::Manual, QUICK_SAVE, game)
}

/// The newest quick save's file (F9), if there is one.
pub fn quick_save_path(dir: &Path) -> Option<PathBuf> {
    finish_pending_writes();
    quick_saves(dir).into_iter().next().map(|e| e.path)
}

/// A save found in the folder.
#[derive(Clone, Debug)]
pub struct SaveEntry {
    pub path: PathBuf,
    pub meta: SaveMeta,
    /// Pinned by the player: never overwritten, first in the list (Razdor's).
    pub pinned: bool,
}

/// Save files of one kind, pinned first, then newest first. Unreadable files are skipped.
pub fn list(dir: &Path, kind: SaveKind) -> Vec<SaveEntry> {
    finish_pending_writes();
    let mut v = by_time(dir, kind);
    v.sort_by_key(|e| !e.pinned);
    v
}

/// Save files of one kind, newest first, pinned or not.
fn by_time(dir: &Path, kind: SaveKind) -> Vec<SaveEntry> {
    let folder = kind_dir(dir, kind);
    let Ok(read) = std::fs::read_dir(&folder) else { return Vec::new() };
    let pins = pins(&folder);
    let files: Vec<(PathBuf, FileStamp)> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == EXTENSION))
        .filter_map(|p| Some((file_stamp(&p).ok()?, p)))
        .map(|(s, p)| (p, s))
        .collect();
    // The files not read before are unpacked on all the cores at once.
    let fresh: Vec<&(PathBuf, FileStamp)> = files.iter().filter(|(p, s)| cached_meta(p, *s).is_none()).collect();
    let threads = std::thread::available_parallelism().map_or(1, |n| n.get()).clamp(1, 8);
    if fresh.len() > 1 && threads > 1 {
        std::thread::scope(|scope| {
            for chunk in fresh.chunks(fresh.len().div_ceil(threads)) {
                scope.spawn(move || {
                    for (p, s) in chunk {
                        if let Ok(meta) = read_meta_uncached(p) {
                            remember_meta(p, *s, &meta);
                        }
                    }
                });
            }
        });
    }
    let mut v: Vec<(SaveEntry, FileStamp)> = files
        .into_iter()
        .filter_map(|(path, stamp)| {
            let meta = cached_meta(&path, stamp).map_or_else(|| read_meta(&path).ok(), Some)?;
            let pinned = file_name(&path).is_some_and(|n| pins.contains(&n));
            Some((SaveEntry { meta, path, pinned }, stamp))
        })
        .collect();
    // Within a second, the file written last first (a reused autosave keeps an old name).
    v.sort_by(|(a, sa), (b, sb)| b.meta.saved_at.cmp(&a.meta.saved_at).then_with(|| sb.1.cmp(&sa.1)).then_with(|| b.path.cmp(&a.path)));
    v.into_iter().map(|(e, _)| e).collect()
}

/// The list of pinned saves of a kind's folder: one file name per line.
const PINS_FILE: &str = "pinned.txt";

fn file_name(path: &Path) -> Option<String> {
    path.file_name().and_then(|n| n.to_str()).map(str::to_string)
}

/// The pinned file names of `folder` whose files are there (a pin of a file gone, deleted
/// by hand, is forgotten so a new file of that name is not pinned).
fn pins(folder: &Path) -> std::collections::BTreeSet<String> {
    std::fs::read_to_string(folder.join(PINS_FILE))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|n| !n.is_empty() && folder.join(n).is_file())
        .map(str::to_string)
        .collect()
}

/// The save file is pinned.
pub fn is_pinned(path: &Path) -> bool {
    let (Some(folder), Some(name)) = (path.parent(), file_name(path)) else { return false };
    pins(folder).contains(&name)
}

/// Pins or unpins a save file.
pub fn set_pinned(path: &Path, on: bool) -> std::io::Result<()> {
    let (Some(folder), Some(name)) = (path.parent(), file_name(path)) else { return Ok(()) };
    let mut pins = pins(folder);
    if on {
        pins.insert(name);
    } else {
        pins.remove(&name);
    }
    let text: String = pins.iter().map(|n| format!("{n}\n")).collect();
    std::fs::write(folder.join(PINS_FILE), text)
}

/// Deletes a save file, and its pin.
pub fn delete(path: &Path) -> std::io::Result<()> {
    std::fs::remove_file(path)?;
    set_pinned(path, false)
}

/// Where a map save finds its scenario: the install folder and its content.
pub struct Install<'a> {
    pub dir: &'a Path,
    pub content: Arc<Content>,
}

/// Reads and restores a save file (see [`restore`]).
pub fn load(path: &Path, demo: Arc<Content>, install: Option<&Install>) -> Result<Game, SaveError> {
    finish_pending_writes();
    let (meta, game) = decode(&std::fs::read(path)?)?;
    restore(&meta, game, demo, install)
}

/// Rebuilds what the save left out: the content, and the world's and the event engine's
/// statics from the scenario (the demo, or the map: the install's file when it is the one
/// the game was saved with, else the map's bytes the save carries, see [`map_bytes`]).
pub fn restore(meta: &SaveMeta, mut game: Game, demo: Arc<Content>, install: Option<&Install>) -> Result<Game, SaveError> {
    check_version(meta)?;
    let embedded = game.map_bytes.take();
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
            let bytes = map_bytes(install.dir, file, *hash, embedded.as_deref())?;
            let scenario = Scenario::from_file_bytes(&bytes).map_err(|e| SaveError::Mismatch(e.to_string()))?;
            game.map_bytes = Some(bytes.into());
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
    let (rng, idle_offsets, music) = Rng::save_load_draws(game.world.map.w, &plants, armies);
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
    game.content = Arc::new(game.content.with_experimental(game.experimental));
    game.origin = Some(meta.scenario.clone());
    // A cell the row width lacks (a save from before a width change, or from a build that let
    // one through) goes to a free cell of its row: the rules' grid has no other places.
    game.reseat_all();
    // The AI is set up again on every load (0x4a1ff0 from the save loader).
    game.ai_init(meta.version >= 4);
    // The offsets the load drew (after the AI's set-up, which clears the records).
    game.set_idle_offsets(&idle_offsets);
    // Saves from before the units' spell slots: the army-wide spells go into the slots.
    if meta.version < 7 {
        migrate_spells(&mut game);
    }
    // Saves from before the last-pay minute count everyone as paid now.
    let now = game.clock.total_minutes() as u64;
    for u in game.squad.iter_mut().filter(|u| u.last_paid == 0) {
        u.last_paid = now;
    }
    // A save made with an item on the pointer (a quick save in the hero or army window): the
    // game opens on the map, so the item goes back into the pack as the window's closing
    // would put it (0x4b8d28 → 0x49a85c).
    game.drop_held();
    check_content(&game).map_err(SaveError::Mismatch)?;
    Ok(game)
}

/// The bytes of map `file`, which a save names with `hash`: the install's copy when it has
/// the hash, else the copy the save carries when that has it (the original's self-contained
/// save, saves-data §12.2). Otherwise the install's copy is refused as changed, or the map is
/// missing (a save without the bytes, as before).
fn map_bytes(dir: &Path, file: &str, hash: u64, embedded: Option<&[u8]>) -> Result<Vec<u8>, SaveError> {
    let maps = crate::dt::install::list_maps(dir).unwrap_or_default();
    let entry = maps.iter().find(|m| m.name == file);
    let in_install = match entry.map(|e| std::fs::read(&e.path)).transpose()? {
        Some(bytes) if fnv1a(&bytes) == hash => return Ok(bytes),
        other => other.is_some(),
    };
    match embedded.filter(|b| fnv1a(b) == hash) {
        Some(bytes) => Ok(bytes.to_vec()),
        None if in_install => Err(SaveError::MapChanged(file.to_string())),
        None => Err(SaveError::MapMissing(file.to_string())),
    }
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
        // An army's character was its own field before each troop kept its name.
        let named = std::mem::take(&mut a.named);
        if let Some(leader) = a.troops.first_mut().filter(|t| named != 0 && t.named == 0) {
            leader.named = named;
        }
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
    g.pack.iter().try_for_each(item)?;
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

    /// The battle's restart keeps the game as the battle began, its foe pending: restored,
    /// it comes back with the same foe, so the battle opens again.
    #[test]
    fn a_game_kept_as_its_battle_began_comes_back_with_its_foe() {
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        g.foe = Some(crate::rules::game::Foe::Garrison(g.world.index_of("Bandit lair")));
        let loaded = roundtrip(&g, c, None);
        assert_eq!(loaded.foe, g.foe);
    }

    #[test]
    fn experimental_rules_survive_save_load_without_changing_install_defaults() {
        let install = demo();
        let rules = super::super::content::ExperimentalRules { simultaneous_bonuses: true, multiple_rings: true, multiple_amulets: true, allow_duplicate_items: false, two_potions: true, six_cells: true };
        let mut g = Game::new(Arc::new(install.with_experimental(rules)), HeroClass::Knight);
        g.squad[0].potions_drunk = 2;
        g.squad[0].battle_disabled = true;
        let loaded = roundtrip(&g, install.clone(), None);
        assert_eq!(loaded.experimental, rules);
        assert_eq!(loaded.content.experimental, rules);
        assert_eq!(loaded.squad[0].potions_drunk, 2);
        assert!(loaded.squad[0].battle_disabled);
        assert_eq!(install.experimental, Default::default());
    }

    #[test]
    fn experimental_missing_save_rules_default_to_classic() {
        let g = Game::new(demo(), HeroClass::Knight);
        let mut old = serde_json::to_value(&g).unwrap();
        old.as_object_mut().unwrap().remove("experimental");
        for u in old["squad"].as_array_mut().unwrap() {
            u.as_object_mut().unwrap().remove("battle_disabled");
            u.as_object_mut().unwrap().remove("potions_drunk");
        }
        let loaded: Game = serde_json::from_value(old).unwrap();
        assert_eq!(loaded.experimental, Default::default());
        assert!(loaded.squad.iter().all(|u| !u.battle_disabled && u.potions_drunk == 0));
    }

    /// Format 11 stored four item cells per unit: they load with two empty cells added, and a
    /// unit with six cells (the multiple rings or amulets rule) roundtrips whole.
    #[test]
    fn four_item_cells_from_format_11_load_padded_and_six_cells_roundtrip() {
        let install = demo();
        let mut g = Game::new(install.clone(), HeroClass::Knight);
        let mut old = serde_json::to_value(&g).unwrap();
        old["squad"][0]["items"] = serde_json::json!([7, null, null, 9]);
        let loaded: Game = serde_json::from_value(old).unwrap();
        assert_eq!(loaded.squad[0].items, [Some(ItemId(7)), None, None, Some(ItemId(9)), None, None]);

        g.squad[0].items[4] = Some(ItemId(7));
        g.squad[0].items[5] = Some(ItemId(9));
        let back = roundtrip(&g, install, None);
        assert_eq!(back.squad[0].items, g.squad[0].items);
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
    fn the_packs_holes_survive_a_save_and_an_item_held_goes_back_into_the_pack() {
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        let potion = g.content.item_by_key("heal_potion").unwrap();
        g.pack = vec![potion, potion, potion].into();
        g.press_pack(0);
        g.press_pack(7);
        g.press_pack(1);
        assert_eq!(g.held_item, Some(potion));
        let meta = meta_of(&g, SaveKind::Manual, "test").unwrap();
        let (m, saved) = decode(&encode(&meta, &g).unwrap()).unwrap();
        let loaded = restore(&m, saved, c, None).unwrap();
        // Cell 7 kept; the item held went into the first empty cell (0x4b8d28 → 0x49a85c).
        assert_eq!(loaded.held_item, None);
        assert_eq!((0..8).map(|i| loaded.pack.get(i)).collect::<Vec<_>>(), [Some(potion), None, Some(potion), None, None, None, None, Some(potion)]);
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
    fn the_listed_metas_follow_the_files() {
        let dir = temp_dir("meta-cache");
        let mut g = Game::new(demo(), HeroClass::Knight);
        let mut paths = Vec::new();
        for k in 0..12 {
            paths.push(write(&dir, SaveKind::Manual, &format!("Save {k}"), &g).unwrap());
        }
        let names = || list(&dir, SaveKind::Manual).into_iter().map(|e| e.meta.name).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(names().len(), 12);
        // Read once, the metas are remembered; a file changed outside Razdor is read again.
        META_CACHE.lock().unwrap().retain(|p, _| !p.starts_with(&dir));
        assert_eq!(names().len(), 12, "read on several threads at once");
        g.gold = 7;
        let other = write(&dir, SaveKind::Manual, "Other name", &g).unwrap();
        std::fs::copy(&other, &paths[0]).unwrap();
        assert_eq!(read_meta(&paths[0]).unwrap().name, "Other name");
        assert!(!names().contains("Save 0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Fills `v` until no meta is waiting (the readers are threads), or fails after 10 s.
    fn fill_all(v: &mut Vec<ListedSave>) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(10);
        loop {
            let states: Vec<MetaState> = v.iter_mut().map(ListedSave::fill).collect();
            let mut k = 0;
            v.retain(|_| {
                k += 1;
                states[k - 1] != MetaState::Unreadable
            });
            if !states.contains(&MetaState::Waiting) {
                return;
            }
            assert!(std::time::Instant::now() < until, "the metas never came");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn the_load_list_comes_at_once_and_its_metas_after() {
        let dir = temp_dir("listed");
        let mut g = Game::new(demo(), HeroClass::Knight);
        let mut paths = Vec::new();
        for k in 0..6 {
            paths.push(write(&dir, SaveKind::Manual, &format!("Save {k}"), &g).unwrap());
            // Apart in time, so the order is the order of writing.
            std::thread::sleep(std::time::Duration::from_millis(15));
        }
        set_pinned(&paths[1], true).unwrap();
        std::fs::write(kind_dir(&dir, SaveKind::Manual).join(format!("junk.{EXTENSION}")), b"not a save").unwrap();
        // Nothing read yet: every row waits, in its place (pinned first, then newest first).
        META_CACHE.lock().unwrap().retain(|p, _| !p.starts_with(&dir));
        let mut v = list_files(&dir, SaveKind::Manual);
        let order: Vec<PathBuf> = v.iter().map(|e| e.path.clone()).collect();
        assert_eq!(v.len(), 7, "the damaged file is listed until read");
        assert!(v.iter().all(|e| e.meta.is_none() && e.written > 0));
        assert_eq!(v[0].path, paths[1], "pinned first");
        assert!(v[1].path.ends_with(format!("junk.{EXTENSION}")), "then the newest file");
        assert_eq!(v[2].path, paths[5]);
        want_metas(v.iter().rev());
        fill_all(&mut v);
        let names: Vec<&str> = v.iter().map(|e| e.meta.as_ref().unwrap().name.as_str()).collect();
        assert_eq!(names, ["Save 1", "Save 5", "Save 4", "Save 3", "Save 2", "Save 0"], "the rows stay in place, the damaged one gone");
        assert_eq!(v.iter().map(|e| &e.path).collect::<Vec<_>>(), order.iter().filter(|p| !p.ends_with(format!("junk.{EXTENSION}"))).collect::<Vec<_>>());
        // Listed again, the metas known come with the list; the damaged file stays out.
        let again = list_files(&dir, SaveKind::Manual);
        assert_eq!(again.len(), 6);
        assert!(again.iter().all(|e| e.meta.is_some()));
        // A save written over and one deleted: the list follows the files.
        g.gold = 7;
        let other = write(&dir, SaveKind::Manual, "Other name", &g).unwrap();
        std::fs::copy(&other, &paths[0]).unwrap();
        delete(&paths[2]).unwrap();
        let mut v = list_files(&dir, SaveKind::Manual);
        want_metas(v.iter());
        fill_all(&mut v);
        let names: std::collections::BTreeSet<String> = v.iter().map(|e| e.meta.as_ref().unwrap().name.clone()).collect();
        assert_eq!(v.len(), 6);
        assert_eq!(names.len(), 5, "the copy and its source share a name");
        assert!(names.contains("Other name") && !names.contains("Save 0") && !names.contains("Save 2"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn quick_saves_take_five_slots_and_f9_finds_the_newest() {
        let dir = temp_dir("quick");
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        assert_eq!(quick_save_path(&dir), None);
        write(&dir, SaveKind::Manual, "Mine", &g).unwrap();
        let mut paths = Vec::new();
        for k in 0..QUICK_SAVES_KEPT + 2 {
            g.gold = 1000 + k as i32;
            paths.push(quick_save(&dir, &g).unwrap());
            // Within a second, the order is the files' write times.
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let saves = list(&dir, SaveKind::Manual);
        assert_eq!(saves.len(), QUICK_SAVES_KEPT + 1, "five quick saves next to the player's own");
        assert_eq!(paths[QUICK_SAVES_KEPT], paths[0], "the sixth reuses the oldest");
        let path = quick_save_path(&dir).unwrap();
        assert_eq!(path, paths[QUICK_SAVES_KEPT + 1]);
        assert_eq!(load(&path, c, None).unwrap().gold, 1000 + QUICK_SAVES_KEPT as i32 + 1, "the newest quick save");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_install_takes_the_old_saves_of_its_maps_with_their_pins() {
        let (root, install) = (temp_dir("claim-root"), temp_dir("claim-install"));
        std::fs::create_dir_all(install.join(crate::dt::install::MAPS_DIR)).unwrap();
        std::fs::write(install.join(crate::dt::install::MAPS_DIR).join("Ours.DTm"), b"our map").unwrap();
        let mut g = Game::new(demo(), HeroClass::Knight);
        let demo_save = write(&root, SaveKind::Manual, "demo", &g).unwrap();
        set_pinned(&demo_save, true).unwrap();
        g.origin = Some(ScenarioRef::Map { file: "Ours".into(), hash: fnv1a(b"our map") });
        write(&root, SaveKind::Auto, "ours", &g).unwrap();
        g.origin = Some(ScenarioRef::Map { file: "Ours".into(), hash: fnv1a(b"the mod's map") });
        write(&root, SaveKind::Manual, "the mod's", &g).unwrap();
        let own = root.join(install_key(&install));
        claim_old_saves(&root, &own, &install);
        let names = |dir: &Path, kind| list(dir, kind).into_iter().map(|e| (e.meta.name, e.pinned)).collect::<Vec<_>>();
        assert_eq!(names(&own, SaveKind::Manual), [("demo".to_string(), true)]);
        assert_eq!(names(&own, SaveKind::Auto), [("ours".to_string(), false)]);
        assert_eq!(names(&root, SaveKind::Manual), [("the mod's".to_string(), false)], "another map's bytes: left for its install");
        assert!(install_key(&install).starts_with("razdor-test-"), "{}", install_key(&install));
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&install));
    }

    #[test]
    fn a_pinned_save_is_never_overwritten_and_comes_first() {
        let dir = temp_dir("pins");
        let mut g = Game::new(demo(), HeroClass::Knight);
        let tick = || std::thread::sleep(std::time::Duration::from_millis(10));
        // The oldest autosave pinned: the rotation goes around it.
        let first = write(&dir, SaveKind::Auto, "auto 0", &g).unwrap();
        tick();
        set_pinned(&first, true).unwrap();
        for k in 1..=AUTOSAVES_KEPT + 3 {
            write(&dir, SaveKind::Auto, &format!("auto {k}"), &g).unwrap();
            tick();
        }
        let autos = list(&dir, SaveKind::Auto);
        assert_eq!(autos.len(), AUTOSAVES_KEPT + 1, "the pin is not counted");
        assert_eq!((autos[0].path.clone(), autos[0].meta.name.as_str(), autos[0].pinned), (first.clone(), "auto 0", true), "first in the list");
        // Its name reused before a battle: a new autosave, the pinned one stays.
        write_autosave(&dir, "auto 0", &g, true).unwrap();
        assert_eq!(read_meta(&first).unwrap().saved_at, autos[0].meta.saved_at);
        // A pinned quick save is kept, and F9 still loads the newest.
        let q = quick_save(&dir, &g).unwrap();
        set_pinned(&q, true).unwrap();
        tick();
        for _ in 0..QUICK_SAVES_KEPT + 1 {
            g.gold += 1;
            quick_save(&dir, &g).unwrap();
            tick();
        }
        assert!(q.exists() && is_pinned(&q));
        assert_eq!(list(&dir, SaveKind::Manual).len(), QUICK_SAVES_KEPT + 1);
        let newest = quick_save_path(&dir).unwrap();
        assert_ne!(newest, q);
        // A pinned own save cannot be saved over; unpinned, it can.
        let mine = write(&dir, SaveKind::Manual, "Mine", &g).unwrap();
        set_pinned(&mine, true).unwrap();
        assert!(matches!(write(&dir, SaveKind::Manual, "Mine", &g), Err(SaveError::Pinned)));
        set_pinned(&mine, false).unwrap();
        write(&dir, SaveKind::Manual, "Mine", &g).unwrap();
        // Deleting drops the pin: a new file of that name is not pinned.
        set_pinned(&mine, true).unwrap();
        delete(&mine).unwrap();
        assert!(!is_pinned(&write(&dir, SaveKind::Manual, "Mine", &g).unwrap()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn own_saves_stop_at_fifty_but_can_be_replaced() {
        let dir = temp_dir("full");
        let g = Game::new(demo(), HeroClass::Knight);
        for k in 0..MANUAL_SAVES_KEPT {
            write(&dir, SaveKind::Manual, &format!("save {k}"), &g).unwrap();
        }
        quick_save(&dir, &g).unwrap();
        assert!(matches!(write(&dir, SaveKind::Manual, "one more", &g), Err(SaveError::Full)));
        write(&dir, SaveKind::Manual, "save 7", &g).unwrap();
        quick_save(&dir, &g).unwrap();
        assert_eq!(list(&dir, SaveKind::Manual).len(), MANUAL_SAVES_KEPT + 2, "quick saves do not count");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pinned_own_saves_have_no_limit_and_free_their_slots() {
        // The owner's deviation (a player's report, 2026-10-17): only unpinned own saves
        // count toward MANUAL_SAVES_KEPT; pinned ones are unlimited and never rotated out.
        let dir = temp_dir("pins-unlimited");
        let g = Game::new(demo(), HeroClass::Knight);
        let mut pinned = Vec::new();
        for k in 0..MANUAL_SAVES_KEPT + 10 {
            let path = write(&dir, SaveKind::Manual, &format!("kept {k}"), &g).unwrap();
            set_pinned(&path, true).unwrap();
            pinned.push(path);
        }
        // Past the limit in pins alone, a full set of unpinned saves still fits.
        for k in 0..MANUAL_SAVES_KEPT {
            write(&dir, SaveKind::Manual, &format!("free {k}"), &g).unwrap();
        }
        assert!(matches!(write(&dir, SaveKind::Manual, "one more", &g), Err(SaveError::Full)), "unpinned saves keep their limit");
        // Replacing an unpinned one still works; a quick save is not refused either.
        write(&dir, SaveKind::Manual, "free 3", &g).unwrap();
        quick_save(&dir, &g).unwrap();
        // Pinning one more when the unpinned slots are full works and frees a slot.
        let free0 = dir.join(MANUAL_DIR).join(format!("{}.{EXTENSION}", slug("free 0")));
        set_pinned(&free0, true).unwrap();
        let extra = write(&dir, SaveKind::Manual, "one more", &g).unwrap();
        // Every save is listed, pinned ones first, and none was overwritten or removed.
        let all = list(&dir, SaveKind::Manual);
        assert_eq!(all.len(), 2 * MANUAL_SAVES_KEPT + 10 + 1 + 1);
        let pins_listed = all.iter().take_while(|e| e.pinned).count();
        assert_eq!(pins_listed, MANUAL_SAVES_KEPT + 11);
        assert!(pinned.iter().all(|p| p.exists() && is_pinned(p)));
        assert!(all.iter().any(|e| e.path == extra && !e.pinned));
        // Unpinning one past the limit keeps every file; only new names are refused.
        set_pinned(&pinned[0], false).unwrap();
        assert!(pinned[0].exists());
        assert!(matches!(write(&dir, SaveKind::Manual, "and another", &g), Err(SaveError::Full)));
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
    fn a_load_seats_a_garrison_unit_off_the_grid_on_a_free_cell() {
        // A save with a unit on a cell its row width lacks (Front 6 of the 6-wide row) comes back
        // with the unit on a free cell of the grid.
        use crate::rules::formation::{Row, Slot};
        use crate::rules::units::Unit;
        use crate::rules::world::Stationed;
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        let unit = Unit::new(&c, g.squad[0].def, Slot::new(Row::Front, 6));
        g.world.locations[0].stationed.push(Stationed { unit });
        let meta = meta_of(&g, SaveKind::Manual, "test").unwrap();
        let (m, loaded) = decode(&encode(&meta, &g).unwrap()).unwrap();
        let back = restore(&m, loaded, c.clone(), None).unwrap();
        let placed = back.world.locations[0].stationed.last().unwrap().unit.slot;
        assert!(c.formation.contains(placed), "loaded on {placed:?}, off the grid");
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

    /// A fake install's maps folder with one hand-built map (`Test.DTm`), and a game walked on it.
    fn map_game(dir: &Path) -> (Game, PathBuf, Arc<Content>) {
        use crate::rules::world::testkit as tk;
        let maps = dir.join(crate::dt::install::MAPS_DIR);
        std::fs::create_dir_all(&maps).unwrap();
        let mut s = tk::scenario(24, 6);
        s.header.heroes[0] = tk::hero(2, 2, 200, &[tk::troop(4, 0, 2)]);
        let path = maps.join("Test.DTm");
        std::fs::write(&path, s.to_payload()).unwrap();
        let content = Arc::new(tk::content());
        let scenario = Scenario::load(&path).unwrap();
        let mut g = Game::from_scenario(content.clone(), &scenario, HeroClass::Knight);
        let (origin, bytes) = ScenarioRef::of_map(&path, "Test").unwrap();
        g.set_map(origin, bytes);
        g.set_destination((10, 3));
        walk(&mut g, 200);
        (g, path, content)
    }

    /// The save's bytes with its `map` field changed by `f` (a tampered save, rebuilt).
    fn with_map_bytes(bytes: &[u8], f: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
        let mut v: serde_json::Value = serde_json::from_slice(&unpack(bytes).unwrap()).unwrap();
        let mut map = base64_decode(v["map"].as_str().expect("a map save carries its map")).unwrap();
        f(&mut map);
        v["map"] = base64_encode(&map).into();
        pack(&serde_json::to_vec(&v).unwrap()).unwrap()
    }

    #[test]
    fn a_map_save_loads_from_its_own_map_bytes_when_the_install_lacks_the_map() {
        let dir = temp_dir("maps");
        let (g, path, content) = map_game(&dir);
        let original = std::fs::read(&path).unwrap();
        let install = Install { dir: &dir, content: content.clone() };
        roundtrip(&g, demo(), Some(&install));

        // The install's copy changed: the save still loads with the map it was saved with.
        std::fs::write(&path, original.iter().map(|b| b ^ 1).collect::<Vec<u8>>()).unwrap();
        roundtrip(&g, demo(), Some(&install));
        // The install has no copy at all (another player's install): the save carries it.
        std::fs::remove_file(&path).unwrap();
        let empty = temp_dir("no-maps");
        let bare = Install { dir: &empty, content: content.clone() };
        let loaded = roundtrip(&g, demo(), Some(&bare));
        assert_eq!(loaded.map_bytes.as_deref(), Some(original.as_slice()), "the map the save carries");
        // Loaded, the game keeps its map: a restart and the next save need no install copy.
        let scenario = Scenario::from_file_bytes(&original).unwrap();
        assert!(g.restart(content.clone(), &scenario).map_bytes.is_some());
        let meta = meta_of(&loaded, SaveKind::Manual, "again").unwrap();
        let (_, reloaded) = decode(&encode(&meta, &loaded).unwrap()).unwrap();
        assert!(reloaded.map_bytes.is_some(), "the loaded game's save carries the map");

        // Without the install at all, a map save still needs the install's content.
        let meta = meta_of(&g, SaveKind::Manual, "x").unwrap();
        let bytes = encode(&meta, &g).unwrap();
        let again = || decode(&bytes).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), None), Err(SaveError::NoInstall)));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn a_tampered_map_in_a_save_is_refused_and_the_install_copy_is_used_instead() {
        let dir = temp_dir("maps-tampered");
        let (g, path, content) = map_game(&dir);
        let original = std::fs::read(&path).unwrap();
        let meta = meta_of(&g, SaveKind::Manual, "x").unwrap();
        let bytes = encode(&meta, &g).unwrap();
        let tampered = with_map_bytes(&bytes, |m| {
            let middle = m.len() / 2;
            m[middle] ^= 1;
        });
        let again = || decode(&tampered).unwrap();
        let bare_dir = temp_dir("maps-tampered-bare");
        let bare = Install { dir: &bare_dir, content: content.clone() };
        // Nothing to load it from: the install lacks the map, and the save's copy is not the map's.
        assert!(matches!(restore(&again().0, again().1, demo(), Some(&bare)), Err(SaveError::MapMissing(m)) if m == "Test"));
        // The install's own copy has the hash, so that one loads, as before.
        let install = Install { dir: &dir, content: content.clone() };
        assert!(restore(&again().0, again().1, demo(), Some(&install)).is_ok());
        // A tampered save whose install copy is changed too is refused as changed.
        std::fs::write(&path, original.iter().map(|b| b ^ 1).collect::<Vec<u8>>()).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), Some(&install)), Err(SaveError::MapChanged(m)) if m == "Test"));
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&bare_dir);
    }

    #[test]
    fn a_save_without_map_bytes_still_needs_the_same_map_file() {
        let dir = temp_dir("maps-old");
        let (mut g, path, content) = map_game(&dir);
        let original = std::fs::read(&path).unwrap();
        let install = Install { dir: &dir, content: content.clone() };
        // A save as Razdor wrote them before the map was carried: no bytes in it.
        let carried = g.map_bytes.take();
        let meta = meta_of(&g, SaveKind::Manual, "x").unwrap();
        let bytes = encode(&meta, &g).unwrap();
        g.map_bytes = carried;
        assert!(!String::from_utf8_lossy(&unpack(&bytes).unwrap()).contains("\"map\":"), "no map in an old save");
        let again = || decode(&bytes).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), None), Err(SaveError::NoInstall)));
        roundtrip(&g, demo(), Some(&install));
        // Changed: every byte of the install's copy differs.
        std::fs::write(&path, original.iter().map(|b| b ^ 1).collect::<Vec<u8>>()).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), Some(&install)), Err(SaveError::MapChanged(m)) if m == "Test"));
        std::fs::rename(&path, dir.join(crate::dt::install::MAPS_DIR).join("Other.DTm")).unwrap();
        assert!(matches!(restore(&again().0, again().1, demo(), Some(&install)), Err(SaveError::MapMissing(_))));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn base64_is_standard_and_refuses_what_is_not_base64() {
        for (raw, text) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64_encode(raw.as_bytes()), text);
            assert_eq!(base64_decode(text).as_deref(), Some(raw.as_bytes()));
        }
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&all)), Some(all));
        for bad in ["Zg=", "Z===", "Zg==Zm9v", "Zm9*", "Zm9v=", "Zg=\u{e9}"] {
            assert_eq!(base64_decode(bad), None, "{bad}");
        }
    }

    /// A save whose map is gone: the message names the map file and the folder it is missing from.
    #[test]
    fn a_missing_map_error_names_the_file_and_the_game_folder() {
        let text = SaveError::MapMissing("Kiev".to_string()).to_string();
        assert!(text.contains("\"Kiev.DTm\""), "{text}");
        assert!(text.contains("missing from the game folder (Maps_Rus)"), "{text}");
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
        let (origin, bytes) = ScenarioRef::of_map(&entry.path, &entry.name).unwrap();
        g.set_map(origin, bytes);
        g.drain_events();
        let install = Install { dir: &dt.dir, content: content.clone() };
        roundtrip(&g, demo(), Some(&install));
        // The save carries its map: it loads with an install that lacks this map file.
        let without_maps = temp_dir("rk1-no-maps");
        roundtrip(&g, demo(), Some(&Install { dir: &without_maps, content: content.clone() }));
        let _ = std::fs::remove_dir_all(&without_maps);
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
            // A save is never taken with a dialog open: the walk's messages are read first.
            for _ in 0..20 {
                if !g.script().unwrap().holds_window() {
                    break;
                }
                g.event_window_closed();
            }
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

    /// The autosave file's game as JSON, with the time it was saved set aside (the one value
    /// the two paths read off the clock).
    fn autosave_json_without_time(path: &Path) -> Vec<u8> {
        let mut v: serde_json::Value = serde_json::from_slice(&unpack(&std::fs::read(path).unwrap()).unwrap()).unwrap();
        v["meta"]["saved_at"] = serde_json::Value::from(0);
        serde_json::to_vec(&v).unwrap()
    }

    /// The noon autosave's writer thread writes the same bytes as the synchronous path for the
    /// same meta: the file is the same packed save, byte for byte.
    #[test]
    fn a_background_autosave_writes_the_bytes_of_the_synchronous_one() {
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Archmage);
        g.set_destination(g.world.locations[g.world.index_of("Millbrook")].tile);
        walk(&mut g, 200);
        let (dir_sync, dir_bg) = (temp_dir("autosave-sync"), temp_dir("autosave-bg"));
        let meta = meta_of(&g, SaveKind::Auto, "1204.06.03, 12 h").unwrap();
        let sync_bytes = encode(&meta, &g).unwrap();
        let sync_path = store_autosave(&dir_sync, &meta, &sync_bytes, false).unwrap();
        spawn_autosave(dir_bg.clone(), meta.clone(), encode_json(&meta, &g).unwrap());
        finish_pending_writes();
        // The names are stamped with the time: the one file of each folder is compared.
        let bg_files = files_of(&dir_bg, SaveKind::Auto);
        assert_eq!(bg_files.len(), 1);
        assert_eq!(std::fs::read(&sync_path).unwrap(), sync_bytes);
        assert_eq!(std::fs::read(&bg_files[0].0).unwrap(), sync_bytes, "the writer thread's file is the synchronous file");
        let _ = std::fs::remove_dir_all(&dir_sync);
        let _ = std::fs::remove_dir_all(&dir_bg);
    }

    /// The public pair: the noon autosave on its thread and the synchronous autosave save the
    /// same game (the time they were saved aside), each as a file of the same name.
    #[test]
    fn the_background_autosave_is_the_same_save_as_the_synchronous_one() {
        let c = demo();
        let mut g = Game::new(c.clone(), HeroClass::Knight);
        walk(&mut g, 60);
        let (dir_sync, dir_bg) = (temp_dir("autosave-same-sync"), temp_dir("autosave-same-bg"));
        let sync_path = write_autosave(&dir_sync, "1204.06.03, 12 h", &g, false).unwrap();
        write_autosave_in_background(dir_bg.clone(), "1204.06.03, 12 h", &g).unwrap();
        finish_pending_writes();
        let listed = list(&dir_bg, SaveKind::Auto);
        assert_eq!(listed.len(), 1, "the writer's file is listed once it is written");
        assert_eq!(listed[0].meta.name, "1204.06.03, 12 h");
        assert_eq!(autosave_json_without_time(&sync_path), autosave_json_without_time(&listed[0].path));
        let _ = std::fs::remove_dir_all(&dir_sync);
        let _ = std::fs::remove_dir_all(&dir_bg);
    }

    /// The listing and the next autosave wait for the writer: its file is listed at once, and
    /// the autosaves that follow rotate as they would have synchronously.
    #[test]
    fn the_listing_and_the_next_autosave_wait_for_the_writer() {
        let g = Game::new(demo(), HeroClass::Archmage);
        let dir = temp_dir("autosave-join");
        write_autosave_in_background(dir.clone(), "first", &g).unwrap();
        assert_eq!(list_files(&dir, SaveKind::Auto).len(), 1, "the listing joins the writer");
        write_autosave_in_background(dir.clone(), "second", &g).unwrap();
        write_autosave(&dir, "third", &g, false).unwrap();
        finish_pending_writes();
        finish_pending_writes();
        assert_eq!(list(&dir, SaveKind::Auto).len(), 3, "each autosave kept under its own name");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
