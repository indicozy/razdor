//! Where the editor reads and writes maps, and the rules that keep the player's shipped
//! maps safe.
//!
//! - Maps are saved to the user's own folder: `RAZDOR_MAPS_DIR`, else `razdor/maps` in the
//!   platform data folder (`~/.local/share/razdor/maps` on Linux). Never into the repo.
//! - The game's `Maps_Rus` folder is written only by an explicit "save to the game folder",
//!   after a confirmation; a file that already exists there (a shipped map or any other)
//!   needs a second confirmation before it is replaced.
//! - Files are written to a temporary name in the same folder and then renamed, so a failed
//!   write never leaves half a map behind.

use std::path::{Path, PathBuf};

use crate::dt::install::{list_maps, MapEntry, MAP_EXTENSION};
use super::mapfile::{change_ext, save_target, SaveFormat};
use crate::i18n::tr;
use crate::trf;

/// Environment variable naming the user's map folder.
pub const MAPS_DIR_ENV: &str = "RAZDOR_MAPS_DIR";

/// The user's map folder: `RAZDOR_MAPS_DIR`, else `razdor/maps` in the platform data folder.
pub fn user_maps_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(MAPS_DIR_ENV).filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    dirs::data_dir().map(|d| d.join("razdor").join("maps"))
}

/// The `.DTm` files of a folder, sorted by name (empty if the folder does not exist).
pub fn list_dir(dir: &Path) -> Vec<MapEntry> {
    list_dir_with(dir, MAP_EXTENSION)
}

/// The files of a folder with the extension `ext` (ignoring case), sorted by name: `DTm`
/// for maps, `DTs` for the original editor's demo maps.
pub fn list_dir_with(dir: &Path, ext: &str) -> Vec<MapEntry> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut maps: Vec<MapEntry> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case(ext)))
        .map(|path| MapEntry { name: path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(), path })
        .collect();
    maps.sort_by(|a, b| a.name.cmp(&b.name));
    maps
}

/// The maps of an install (its `Maps_Rus` folder), empty if there is none.
pub fn install_maps(install_dir: &Path) -> Vec<MapEntry> {
    list_maps(install_dir).unwrap_or_default()
}

/// Where a save goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// The user's map folder.
    UserFolder,
    /// The game's maps folder (explicit action only).
    GameFolder,
}

/// What the user has agreed to for this save.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Consent {
    /// "Save into the game's folder?" answered yes.
    pub game_folder: bool,
    /// "Replace the game's existing map of this name?" answered yes.
    pub replace_game_map: bool,
    /// "Replace your existing map of this name?" answered yes.
    pub replace_own_map: bool,
}

/// Why a save cannot go ahead yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveBlock {
    /// The name is empty or not a plain file name.
    BadName(String),
    /// No folder to save into (no data folder, or no install for the game folder).
    NoFolder,
    /// Saving into the game's folder needs the first confirmation.
    ConfirmGameFolder(PathBuf),
    /// A map of this name exists in the game's folder: the second confirmation.
    ConfirmReplaceGameMap(PathBuf),
    /// A different map of this name exists in the user's folder.
    ConfirmReplaceOwnMap(PathBuf),
}

impl std::fmt::Display for SaveBlock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveBlock::BadName(why) => write!(f, "{why}"),
            SaveBlock::NoFolder => f.write_str(tr("there is no folder to save into")),
            SaveBlock::ConfirmGameFolder(p) => f.write_str(&trf!("Save into the game's maps folder as {path}?", path = p.display())),
            SaveBlock::ConfirmReplaceGameMap(p) => f.write_str(&trf!("{path} already exists in the game's folder (it may be a shipped map). Replace it?", path = p.display())),
            SaveBlock::ConfirmReplaceOwnMap(p) => f.write_str(&trf!("{path} already exists. Replace it?", path = p.display())),
        }
    }
}

/// `name` as a file name: trimmed, `.DTm` added unless present; no folders, no control
/// characters, not `.` or `..`.
pub fn file_name(name: &str) -> Result<String, SaveBlock> {
    let name = name.trim();
    let stem = match name.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case(MAP_EXTENSION) => stem,
        _ => name,
    };
    let stem = stem.trim();
    if stem.is_empty() || stem == "." || stem == ".." {
        return Err(SaveBlock::BadName(tr("type a name for the map").into()));
    }
    if let Some(c) = stem.chars().find(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')) {
        return Err(SaveBlock::BadName(trf!("a map name cannot contain {c}", c = format!("{c:?}"))));
    }
    Ok(format!("{stem}.{MAP_EXTENSION}"))
}

/// Decides the file a save writes, or what must be confirmed first. `user_dir` is the
/// user's folder, `game_dir` the install's maps folder (if any), `current` the file the
/// document was last saved to (saving over it again needs no confirmation).
pub fn plan_save(
    name: &str,
    dest: Destination,
    user_dir: Option<&Path>,
    game_dir: Option<&Path>,
    current: Option<&Path>,
    consent: Consent,
) -> Result<PathBuf, SaveBlock> {
    plan_save_as(name, SaveFormat::Normal, dest, user_dir, game_dir, current, consent)
}

/// [`plan_save`] for one of the save dialog's file types: the returned name carries the
/// type's extension, which [`EditorDoc::save_to`](super::EditorDoc::save_to) turns into the
/// file it writes (a `.DTD` or `.DTZ` save writes `<name>.DTm`, a `.DTS` save `<name>.DTs`);
/// the confirmations are about that file.
pub fn plan_save_as(
    name: &str,
    format: SaveFormat,
    dest: Destination,
    user_dir: Option<&Path>,
    game_dir: Option<&Path>,
    current: Option<&Path>,
    consent: Consent,
) -> Result<PathBuf, SaveBlock> {
    let file = file_name(name)?;
    let dir = match dest {
        Destination::UserFolder => user_dir,
        Destination::GameFolder => game_dir,
    }
    .ok_or(SaveBlock::NoFolder)?;
    let request = |p: PathBuf| if format == SaveFormat::Normal { p } else { change_ext(&p, format.extension()) };
    let written = save_target(&request(dir.join(&file))).map;
    let written_name = written.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let path = existing_case(dir, &written_name).unwrap_or(written);
    match dest {
        Destination::UserFolder => {
            let same_as_current = current.is_some_and(|c| same_file(c, &path));
            if path.exists() && !same_as_current && !consent.replace_own_map {
                return Err(SaveBlock::ConfirmReplaceOwnMap(path));
            }
        }
        Destination::GameFolder => {
            if !consent.game_folder {
                return Err(SaveBlock::ConfirmGameFolder(path));
            }
            if path.exists() && !consent.replace_game_map {
                return Err(SaveBlock::ConfirmReplaceGameMap(path));
            }
        }
    }
    Ok(request(path))
}

/// The file in `dir` whose name matches `file` ignoring case (the game comes from Windows,
/// where `a.dtm` and `A.DTm` are one file).
fn existing_case(dir: &Path, file: &str) -> Option<PathBuf> {
    let exact = dir.join(file);
    if exact.exists() {
        return Some(exact);
    }
    std::fs::read_dir(dir).ok()?.filter_map(|e| e.ok()).map(|e| e.path()).find(|p| {
        p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == file.to_lowercase())
    })
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Is `path` inside `dir`?
pub fn is_inside(path: &Path, dir: &Path) -> bool {
    let (Ok(p), Ok(d)) = (path.canonicalize(), dir.canonicalize()) else { return path.starts_with(dir) };
    p.starts_with(d)
}

/// Writes `bytes` to `path`: into a temporary file next to it, then renamed over it. Creates
/// the folder if needed.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
    std::fs::create_dir_all(dir)?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = dir.join(format!(".{name}.razdor-tmp"));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A fresh folder under the system temp dir (left for the OS to clean).
    pub(crate) fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("razdor-editor-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn file_names() {
        assert_eq!(file_name("Моя карта").unwrap(), "Моя карта.DTm");
        assert_eq!(file_name(" test.dtm ").unwrap(), "test.DTm");
        assert!(matches!(file_name(""), Err(SaveBlock::BadName(_))));
        assert!(matches!(file_name(".DTm"), Err(SaveBlock::BadName(_))));
        assert!(matches!(file_name("../evil"), Err(SaveBlock::BadName(_))));
        assert!(matches!(file_name("a/b"), Err(SaveBlock::BadName(_))));
        assert!(matches!(file_name("a\\b"), Err(SaveBlock::BadName(_))));
        assert!(matches!(file_name(".."), Err(SaveBlock::BadName(_))));
    }

    #[test]
    fn user_folder_saves_ask_before_replacing_another_file() {
        let user = temp_dir("user");
        let c = Consent::default();
        let p = plan_save("one", Destination::UserFolder, Some(&user), None, None, c).unwrap();
        assert_eq!(p, user.join("one.DTm"));
        write_atomically(&p, b"x").unwrap();
        // Saving a new document over it asks first ...
        assert_eq!(plan_save("one", Destination::UserFolder, Some(&user), None, None, c), Err(SaveBlock::ConfirmReplaceOwnMap(p.clone())));
        let yes = Consent { replace_own_map: true, ..c };
        assert_eq!(plan_save("one", Destination::UserFolder, Some(&user), None, None, yes).unwrap(), p);
        // ... but saving the document that lives there again does not.
        assert_eq!(plan_save("one", Destination::UserFolder, Some(&user), None, Some(&p), c).unwrap(), p);
        // Case-insensitive names find the same file.
        assert!(matches!(plan_save("ONE", Destination::UserFolder, Some(&user), None, None, c), Err(SaveBlock::ConfirmReplaceOwnMap(_))));
        assert_eq!(plan_save("x", Destination::UserFolder, None, None, None, c), Err(SaveBlock::NoFolder));
    }

    #[test]
    fn game_folder_needs_two_confirmations_for_existing_maps() {
        let game = temp_dir("game");
        let shipped = game.join("Карта.DTm");
        write_atomically(&shipped, b"original").unwrap();
        let none = Consent::default();
        // Never without the first confirmation, even for a new name.
        assert!(matches!(plan_save("new", Destination::GameFolder, None, Some(&game), None, none), Err(SaveBlock::ConfirmGameFolder(_))));
        let first = Consent { game_folder: true, ..none };
        assert_eq!(plan_save("new", Destination::GameFolder, None, Some(&game), None, first).unwrap(), game.join("new.DTm"));
        // A shipped map's name needs the second one, even when the document came from there.
        assert_eq!(plan_save("Карта", Destination::GameFolder, None, Some(&game), Some(&shipped), first), Err(SaveBlock::ConfirmReplaceGameMap(shipped.clone())));
        let both = Consent { replace_game_map: true, ..first };
        assert_eq!(plan_save("Карта", Destination::GameFolder, None, Some(&game), None, both).unwrap(), shipped);
        // The user's own-folder consent does not count for the game folder.
        let wrong = Consent { replace_own_map: true, game_folder: true, ..none };
        assert!(matches!(plan_save("карта.dtm", Destination::GameFolder, None, Some(&game), None, wrong), Err(SaveBlock::ConfirmReplaceGameMap(_))));
        assert_eq!(std::fs::read(&shipped).unwrap(), b"original");
        assert_eq!(plan_save("x", Destination::GameFolder, None, None, None, both), Err(SaveBlock::NoFolder));
    }

    #[test]
    fn user_saves_never_land_in_the_game_folder() {
        let (user, game) = (temp_dir("u2"), temp_dir("g2"));
        let opened = game.join("Map.DTm");
        write_atomically(&opened, b"shipped").unwrap();
        // A map opened from the game folder is saved to the user's folder by default.
        let p = plan_save("Map", Destination::UserFolder, Some(&user), Some(&game), Some(&opened), Consent::default()).unwrap();
        assert_eq!(p, user.join("Map.DTm"));
        assert!(!is_inside(&p, &game));
        assert!(is_inside(&opened, &game));
    }

    #[test]
    fn save_formats_plan_the_file_they_write() {
        let user = temp_dir("formats");
        let c = Consent::default();
        let plan = |f| plan_save_as("m", f, Destination::UserFolder, Some(&user), None, None, c);
        assert_eq!(plan(SaveFormat::Normal).unwrap(), user.join("m.DTm"));
        assert_eq!(plan(SaveFormat::Dump).unwrap(), user.join("m.DTD"));
        assert_eq!(plan(SaveFormat::Uncompressed).unwrap(), user.join("m.DTZ"));
        assert_eq!(plan(SaveFormat::Demo).unwrap(), user.join("m.DTS"));
        // The confirmations are about the file written: m.DTm for the dump and the
        // uncompressed save, m.DTs for the demo.
        write_atomically(&user.join("m.DTm"), b"x").unwrap();
        assert_eq!(plan(SaveFormat::Uncompressed), Err(SaveBlock::ConfirmReplaceOwnMap(user.join("m.DTm"))));
        assert_eq!(plan(SaveFormat::Demo).unwrap(), user.join("m.DTS"));
        write_atomically(&user.join("m.DTs"), b"x").unwrap();
        assert_eq!(plan(SaveFormat::Demo), Err(SaveBlock::ConfirmReplaceOwnMap(user.join("m.DTs"))));
        assert_eq!(list_dir_with(&user, "DTs").len(), 1);
    }

    #[test]
    fn atomic_writes_and_listing() {
        let d = temp_dir("list").join("nested");
        write_atomically(&d.join("b.DTm"), b"2").unwrap();
        write_atomically(&d.join("a.dtm"), b"1").unwrap();
        write_atomically(&d.join("notes.txt"), b"-").unwrap();
        write_atomically(&d.join("b.DTm"), b"3").unwrap();
        assert_eq!(std::fs::read(d.join("b.DTm")).unwrap(), b"3");
        let names: Vec<String> = list_dir(&d).into_iter().map(|m| m.name).collect();
        assert_eq!(names, ["a", "b"]);
        assert!(list_dir(&d.join("missing")).is_empty());
        // No temporary files stay behind.
        assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().contains("razdor-tmp")));
    }
}
