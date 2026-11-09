//! Moving the player's files from the shared storage into the app's own folder on Android
//! (release N: the storage permission still reads the old folders, so the copy can run; the
//! release after drops the permission). Pure Rust, compiled on every target: the thread and the
//! screen are `ui::migrate`.
//!
//! Each folder is copied into `<dest>.part` and every file's name and size are compared with the
//! source. The absolute legacy paths stored in the copy's text files are rewritten to the new
//! ones (on the copy, never on the source). Then the legacy trees are scanned again: if anything
//! in them changed since the copy's scan, the move is discarded and starts again at the next
//! start. Only then the copy is put in place and the marker ([`platform::migrated_marker`]) is
//! written. The caller flips the paths after that.
//!
//! The app's folder may already hold files without a marker (a run without the storage grant
//! plays from it): then the copy is merged in, and the app's own file wins where both have one.
//! Whatever this move created is the only thing a failure removes. The legacy folders are only
//! read: nothing is written, moved or deleted there.

use crate::dt::game_import::SPACE_MARGIN;
use crate::platform::{self, LEGACY_ANDROID_GAME, LEGACY_ANDROID_ROOT};
use std::cell::Cell;
use std::fs;
use std::io::{self, Read as _};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Text files bigger than this are not searched for legacy paths (saves, settings and the hub's
/// small records are far smaller; the game's own files are never searched).
const REWRITE_LIMIT: u64 = 8 << 20;
/// Folders nested deeper than this are refused (a link loop, say).
const MAX_DEPTH: u32 = 32;
/// The logs of the running app, at the top of the Razdor folder: the diagnostic log and the play
/// log, with the copies kept before them. This process writes to them (and renames them, when
/// the play log is first written) during the move, so a change to them is not a reason to start
/// again. A copy takes the size the scan listed; a play log that is renamed away meanwhile is
/// left out of the copy.
const LIVE_LOGS: [&str; 4] = ["razdor.log", "razdor.previous.log", "razdor-play.log", "razdor-play.previous.log"];

fn is_live_log(rel: &Path) -> bool {
    LIVE_LOGS.iter().any(|name| rel == Path::new(name))
}

/// Copies the first `len` bytes of `from` to a new file `to` (`len` is the size the scan listed:
/// a live file may grow meanwhile). [`copy_file`] is the real one.
pub type CopyFile<'a> = dyn FnMut(&Path, &Path, u64) -> io::Result<()> + 'a;

/// Puts `from` (a file or a folder) at `to`, where nothing is yet. [`std::fs::rename`] is the real one.
pub type PlaceFile<'a> = dyn FnMut(&Path, &Path) -> io::Result<()> + 'a;

/// Where the data comes from and goes to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The app's own folder (`getExternalFilesDir`): `Razdor` and `DiscordTimes` go inside it.
    pub files: PathBuf,
    /// The legacy folders, read only.
    pub legacy_root: PathBuf,
    pub legacy_game: PathBuf,
}

impl Plan {
    /// The phones' plan: the legacy shared folders into the app's own folder `files`.
    pub fn android(files: &Path) -> Plan {
        let legacy = platform::legacy_android_paths();
        Plan { files: files.to_path_buf(), legacy_root: legacy.root, legacy_game: legacy.game }
    }
}

/// Why the move did not happen. Nothing legacy was changed, and nothing new is left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The room the folder needs with [`SPACE_MARGIN`], and the room free, in bytes.
    NoRoom { needed: u64, free: u64 },
    /// Anything else: what went wrong.
    Failed(String),
}

/// The move happened. `game_skipped`: the game folder was left in the legacy place for lack of
/// room (the install is remembered at the new place, so the archive is asked for when it is not there).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub game_skipped: bool,
}

/// The real copy of one file (`len` bytes, see [`CopyFile`]), then its modification time (the
/// hub's records compare times, so the copy keeps them).
pub fn copy_file(from: &Path, to: &Path, len: u64) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let mut copy = fs::File::create_new(to)?;
    io::copy(&mut source.by_ref().take(len), &mut copy)?;
    drop(copy);
    let modified = fs::metadata(from)?.modified()?;
    fs::OpenOptions::new().write(true).open(to)?.set_modified(modified)
}

/// Moves the legacy folders of `plan` into the app's folder (see the module docs). `free` gives
/// the free bytes of a folder ([`crate::dt::game_import::free_space`], or a test's); `copy` copies
/// one file; `place` puts a copied file or folder where it goes; `progress` gets the bytes copied
/// so far and the total. A second call after the marker copies nothing.
pub fn migrate(
    plan: &Plan,
    free: &dyn Fn(&Path) -> Option<u64>,
    copy: &mut CopyFile<'_>,
    place: &mut PlaceFile<'_>,
    progress: &dyn Fn(u64, u64),
) -> Result<Moved, Failure> {
    let dest = platform::android_paths(&plan.files, true);
    let mut made = Made::default();
    let result = move_in(plan, &dest, free, copy, place, progress, &mut made);
    if result.is_err() {
        made.undo();
        for part in [part_of(&dest.root), part_of(&dest.game)] {
            let _ = fs::remove_dir_all(part);
        }
    }
    result
}

/// What this move put into the app's folder: a failure removes exactly these.
#[derive(Default)]
struct Made {
    /// Folders this move created in place (renamed in whole).
    trees: Vec<PathBuf>,
    /// Folders created inside an existing folder, in creation order.
    dirs: Vec<PathBuf>,
    /// Files placed inside an existing folder.
    files: Vec<PathBuf>,
}

impl Made {
    fn undo(&self) {
        for file in self.files.iter().rev() {
            let _ = fs::remove_file(file);
        }
        for dir in self.dirs.iter().rev() {
            let _ = fs::remove_dir(dir);
        }
        for tree in &self.trees {
            let _ = fs::remove_dir_all(tree);
        }
    }
}

fn move_in(
    plan: &Plan,
    dest: &platform::AndroidPaths,
    free: &dyn Fn(&Path) -> Option<u64>,
    copy: &mut CopyFile<'_>,
    place: &mut PlaceFile<'_>,
    progress: &dyn Fn(u64, u64),
    made: &mut Made,
) -> Result<Moved, Failure> {
    if platform::migrated_marker(&plan.files).exists() {
        return Ok(Moved { game_skipped: false });
    }
    let root = scan(&plan.legacy_root).map_err(io_failure)?;
    let game = scan(&plan.legacy_game).map_err(io_failure)?;
    let game_there = !game.files.is_empty();
    // Parts a killed run left behind. The folders themselves are never removed here: the app's
    // folder may hold files of its own (see the module docs).
    for part in [part_of(&dest.root), part_of(&dest.game)] {
        let _ = fs::remove_dir_all(part);
    }
    // The room, decided before anything is written: the Razdor folder first, then the game in
    // what is left of the room.
    let free_now = free(&plan.files);
    let root_needed = root.bytes + SPACE_MARGIN;
    if let Some(free_now) = free_now {
        if root_needed > free_now {
            return Err(Failure::NoRoom { needed: root_needed, free: free_now });
        }
    }
    let game_fits = match free_now {
        _ if !game_there => false,
        None => true,
        Some(free_now) => game.bytes + SPACE_MARGIN <= free_now.saturating_sub(root.bytes),
    };
    let total = root.bytes + if game_fits { game.bytes } else { 0 };
    let done = Cell::new(0u64);
    let note = |n: u64| {
        done.set(done.get() + n);
        progress(done.get(), total);
    };

    let part_root = part_of(&dest.root);
    copy_tree(&plan.legacy_root, &part_root, &root, copy, &note)?;
    let root_to = dest.root.to_string_lossy().into_owned();
    let game_to = dest.game.to_string_lossy().into_owned();
    let mut pairs = vec![(LEGACY_ANDROID_ROOT, root_to.as_str())];
    if game_fits {
        pairs.push((LEGACY_ANDROID_GAME, game_to.as_str()));
    }
    rewrite_tree(&part_root, &pairs).map_err(io_failure)?;
    if game_there && !game_fits {
        // The game stays where it is, but the remembered install names the new folder, so the
        // not-found screen asks for the archive once the legacy folder cannot be read.
        remember_game(&part_root, &game_to).map_err(io_failure)?;
    }
    let part_game = part_of(&dest.game);
    if game_fits {
        copy_tree(&plan.legacy_game, &part_game, &game, copy, &note)?;
    }

    // Anything that changed in the legacy folders since their scan makes this copy stale.
    unchanged(&plan.legacy_root, &root)?;
    unchanged(&plan.legacy_game, &game)?;

    place_folder(&part_root, &dest.root, place, made)?;
    if game_fits {
        place_folder(&part_game, &dest.game, place, made)?;
    }
    let game_skipped = game_there && !game_fits;
    let how = if game_fits { format!("from {LEGACY_ANDROID_ROOT} and {LEGACY_ANDROID_GAME}") } else { format!("from {LEGACY_ANDROID_ROOT}") };
    platform::write_migrated_marker(&plan.files, &how);
    if !platform::migrated_marker(&plan.files).exists() {
        return Err(Failure::Failed("the marker could not be written".into()));
    }
    Ok(Moved { game_skipped })
}

/// `<path>.part`, the folder a copy is written into before it is put in place.
fn part_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn io_failure(e: io::Error) -> Failure {
    Failure::Failed(e.to_string())
}

/// The folders and files of a tree (paths relative to its root): the files with their sizes and
/// modification times, and the bytes in all.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tree {
    dirs: Vec<PathBuf>,
    files: Vec<(PathBuf, u64, Option<SystemTime>)>,
    bytes: u64,
}

/// What `root` holds. A missing folder is an empty tree. Links and special files are skipped
/// (logged); a link is never followed, so a dangling one does not fail the move.
fn scan(root: &Path) -> io::Result<Tree> {
    let mut tree = Tree::default();
    match fs::read_dir(root) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(tree),
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    walk(root, root, 0, &mut tree)?;
    tree.dirs.sort();
    tree.files.sort();
    Ok(tree)
}

fn walk(root: &Path, dir: &Path, depth: u32, tree: &mut Tree) -> io::Result<()> {
    if depth > MAX_DEPTH {
        return Err(io::Error::other(format!("{}: folders nested too deep", dir.display())));
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        let meta = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) => {
                skip(&path, &e.to_string());
                continue;
            }
        };
        let kind = meta.file_type();
        if kind.is_symlink() {
            skip(&path, "a link");
        } else if kind.is_dir() {
            tree.dirs.push(rel);
            walk(root, &path, depth + 1, tree)?;
        } else if kind.is_file() {
            tree.bytes += meta.len();
            tree.files.push((rel, meta.len(), meta.modified().ok()));
        } else {
            skip(&path, "not a file or a folder");
        }
    }
    Ok(())
}

fn skip(path: &Path, why: &str) {
    crate::diag::log(&format!("move: {} is skipped ({why})", path.display()));
}

/// Copies `src` (as `tree` lists it) into the new folder `part`, then compares what is there
/// with the list: every file's name and size, and the count. `copied` gets each file's size.
fn copy_tree(src: &Path, part: &Path, tree: &Tree, copy: &mut CopyFile<'_>, copied: &dyn Fn(u64)) -> Result<(), Failure> {
    fs::create_dir_all(part).map_err(io_failure)?;
    for dir in &tree.dirs {
        fs::create_dir_all(part.join(dir)).map_err(io_failure)?;
    }
    // What the copy is checked against: a live log renamed away since the scan is not in it.
    let mut expected = tree.clone();
    for (rel, len, _) in &tree.files {
        match copy(&src.join(rel), &part.join(rel), *len) {
            Ok(()) => copied(*len),
            Err(e) if e.kind() == io::ErrorKind::NotFound && is_live_log(rel) => {
                crate::diag::log(&format!("move: {} was renamed during the move and is left out", rel.display()));
                expected.files.retain(|(gone, _, _)| gone != rel);
            }
            Err(e) => return Err(io_failure(e)),
        }
    }
    verify(&expected, &scan(part).map_err(io_failure)?)
}

/// The copy's list against the source's: the same count, names and sizes.
fn verify(expected: &Tree, got: &Tree) -> Result<(), Failure> {
    if expected.files.len() != got.files.len() {
        return Err(Failure::Failed(format!("the copy has {} files, the source {}", got.files.len(), expected.files.len())));
    }
    for ((rel, len, _), (got_rel, got_len, _)) in expected.files.iter().zip(&got.files) {
        if rel != got_rel {
            return Err(Failure::Failed(format!("{} is not in the copy", rel.display())));
        }
        if len != got_len {
            return Err(Failure::Failed(format!("{}: {got_len} bytes copied, {len} in the source", rel.display())));
        }
    }
    Ok(())
}

/// Scans the legacy folder `dir` again and compares it with `before` (the scan the copy used):
/// names, sizes and times. The live logs are left out (see [`LIVE_LOGS`]).
fn unchanged(dir: &Path, before: &Tree) -> Result<(), Failure> {
    let now = scan(dir).map_err(io_failure)?;
    if listing(&now) == listing(before) {
        Ok(())
    } else {
        Err(Failure::Failed(format!("{} changed while it was copied: the move starts again at the next start", dir.display())))
    }
}

/// The folders, and the files with their sizes and times, of a tree (see [`listing`]).
type Listing = (Vec<PathBuf>, Vec<(PathBuf, u64, Option<SystemTime>)>);

/// The folders and files of `tree` without the live logs, for [`unchanged`].
fn listing(tree: &Tree) -> Listing {
    let files = tree.files.iter().filter(|(rel, _, _)| !is_live_log(rel)).cloned().collect();
    (tree.dirs.clone(), files)
}

/// Puts the copy `part` at `dest`. Where `dest` does not exist yet, `part` is renamed in whole.
/// Otherwise the copy's files go in where `dest` has no file at that path: a regular file already
/// there is the app's own and wins (logged). Anything else in the way (a folder where the copy has
/// a file, a file where the copy has a folder, a link) stops the move, so nothing is overwritten
/// and the undo runs. Each placed file is checked by its size; `part` is then removed.
fn place_folder(part: &Path, dest: &Path, place: &mut PlaceFile<'_>, made: &mut Made) -> Result<(), Failure> {
    if fs::symlink_metadata(dest).is_err() {
        place(part, dest).map_err(io_failure)?;
        made.trees.push(dest.to_path_buf());
        return Ok(());
    }
    if !is_real_folder(dest) {
        return Err(Failure::Failed(format!("{} is in the app's folder and is not a folder: the move stops", dest.display())));
    }
    let tree = scan(part).map_err(io_failure)?;
    for dir in &tree.dirs {
        let target = dest.join(dir);
        match fs::symlink_metadata(&target) {
            Ok(meta) if meta.is_dir() => {}
            Ok(_) => return Err(Failure::Failed(format!("{} is a file in the app's folder, but the legacy copy has a folder there: the move stops", target.display()))),
            Err(_) => {
                fs::create_dir(&target).map_err(io_failure)?;
                made.dirs.push(target);
            }
        }
    }
    for (rel, len, _) in &tree.files {
        let target = dest.join(rel);
        match fs::symlink_metadata(&target) {
            Ok(meta) if meta.is_file() => {
                crate::diag::log(&format!("move: {} is kept: the app's own file", target.display()));
                continue;
            }
            Ok(_) => return Err(Failure::Failed(format!("{} is not a file in the app's folder, but the legacy copy has a file there: the move stops", target.display()))),
            Err(_) => {}
        }
        place(&part.join(rel), &target).map_err(io_failure)?;
        made.files.push(target.clone());
        match fs::metadata(&target) {
            Ok(meta) if meta.len() == *len => {}
            _ => return Err(Failure::Failed(format!("{} is not in place", target.display()))),
        }
    }
    let _ = fs::remove_dir_all(part);
    Ok(())
}

/// A folder that is not a link.
fn is_real_folder(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Rewrites the legacy paths in the text files under `dir`, each `(legacy, new)` pair.
fn rewrite_tree(dir: &Path, pairs: &[(&str, &str)]) -> io::Result<()> {
    for (rel, len, _) in scan(dir)?.files {
        if len > REWRITE_LIMIT {
            continue;
        }
        let path = dir.join(rel);
        let bytes = fs::read(&path)?;
        let Ok(text) = std::str::from_utf8(&bytes) else { continue };
        if let Some(new) = rewrite_text(text, pairs) {
            fs::write(&path, new)?;
        }
    }
    Ok(())
}

/// The remembered install (`install` in the copy) names the new game folder, when it named the
/// legacy one.
fn remember_game(part_root: &Path, game_to: &str) -> io::Result<()> {
    let install = part_root.join("install");
    let Ok(text) = fs::read_to_string(&install) else { return Ok(()) };
    if text.trim() == LEGACY_ANDROID_GAME {
        fs::write(install, game_to)?;
    }
    Ok(())
}

/// Whether a legacy path ends at `next` (the character after it): the end of the text, a path
/// separator, a quote or a space. `DiscordTimes-Evolution`, `DiscordTimes_old` and
/// `DiscordTimes.bak` are other names.
fn ends_path(next: Option<char>) -> bool {
    match next {
        None => true,
        Some(c) => matches!(c, '/' | '\\' | '"' | '\'') || c.is_whitespace(),
    }
}

/// `text` with each whole legacy path replaced by its new one (see [`ends_path`]). None when
/// nothing matched.
pub fn rewrite_text(text: &str, pairs: &[(&str, &str)]) -> Option<String> {
    let mut out = String::new();
    let mut changed = false;
    let mut last = 0;
    let mut i = 0;
    while i < text.len() {
        if !text.is_char_boundary(i) {
            i += 1;
            continue;
        }
        let hit = pairs.iter().find(|(from, _)| text[i..].starts_with(from) && ends_path(text[i + from.len()..].chars().next()));
        match hit {
            Some((from, to)) => {
                out.push_str(&text[last..i]);
                out.push_str(to);
                i += from.len();
                last = i;
                changed = true;
            }
            None => i += 1,
        }
    }
    if !changed {
        return None;
    }
    out.push_str(&text[last..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// A fresh folder for one test (removed by the test that makes it).
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-migration-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn put(dir: &Path, rel: &str, bytes: &[u8]) {
        let path = dir.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    /// Everything under a folder: each folder (None) and file (its bytes).
    type Snapshot = BTreeMap<PathBuf, Option<Vec<u8>>>;

    /// Everything under `dir` (see [`Snapshot`]): the state before and after a move.
    fn everything(dir: &Path) -> Snapshot {
        fn walk(root: &Path, dir: &Path, out: &mut Snapshot) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                let rel = path.strip_prefix(root).unwrap().to_path_buf();
                let kind = fs::symlink_metadata(&path).unwrap().file_type();
                if kind.is_symlink() {
                    out.insert(rel, Some(b"a link".to_vec()));
                } else if kind.is_dir() {
                    out.insert(rel, None);
                    walk(root, &path, out);
                } else {
                    out.insert(rel, Some(fs::read(&path).unwrap()));
                }
            }
        }
        let mut out = Snapshot::new();
        walk(dir, dir, &mut out);
        out
    }

    /// The legacy folders as the old releases left them: saves, settings, the remembered install
    /// (an absolute path), a hub mod's merged folder whose stamp stores the base's path, and the game.
    fn legacy(root: &Path, game: &Path) {
        put(root, "saves/Autosave.rzsave", b"save bytes");
        put(root, "saves/settings.json", b"{\"volume\":3}");
        put(root, ".nomedia", b"");
        put(root, "install", LEGACY_ANDROID_GAME.as_bytes());
        put(root, "hub/mods/mod1/game/stamp.json", format!("{{\"base\":\"{LEGACY_ANDROID_GAME}\",\"layers\":[]}}").as_bytes());
        put(root, "hub/mods/mod1/game/Rus_Units.ini", b"units");
        put(game, "DiscordTimes.exe", &[7u8; 4096]);
        put(game, "Rus_Units.ini", b"units in the game");
        put(game, "Maps_Rus/Map1.DTm", &[1u8; 1000]);
    }

    fn plan(name: &str) -> (PathBuf, Plan) {
        let base = scratch(name);
        let (root, game, files) = (base.join("legacy-razdor"), base.join("legacy-game"), base.join("app-files"));
        legacy(&root, &game);
        fs::create_dir_all(&files).unwrap();
        (base, Plan { files, legacy_root: root, legacy_game: game })
    }

    fn room(bytes: u64) -> impl Fn(&Path) -> Option<u64> {
        move |_: &Path| Some(bytes)
    }

    fn plenty() -> u64 {
        10 << 30
    }

    /// The real copy and the real placing, for the tests' moves.
    fn real_copy(from: &Path, to: &Path, len: u64) -> io::Result<()> {
        copy_file(from, to, len)
    }

    fn real_place(from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }

    /// Runs a move with the real copy, placing and room.
    fn run(plan: &Plan) -> Result<Moved, Failure> {
        migrate(plan, &room(plenty()), &mut real_copy, &mut real_place, &|_, _| {})
    }

    /// The paths the app uses: the legacy ones while the marker is absent, then the app's own.
    fn in_use(plan: &Plan) -> platform::StartState {
        platform::start_state(platform::migrated_marker(&plan.files).exists(), platform::legacy_state(&plan.legacy_root), platform::legacy_state(&plan.legacy_game))
    }

    fn legacy_state_of(plan: &Plan) -> (Snapshot, Snapshot) {
        (everything(&plan.legacy_root), everything(&plan.legacy_game))
    }

    #[test]
    fn a_full_copy_writes_the_marker_flips_the_paths_and_rewrites_the_stored_paths() {
        let (base, plan) = plan("full");
        let before = legacy_state_of(&plan);
        assert_eq!(in_use(&plan), platform::StartState::Legacy);

        let moved = run(&plan).expect("moves");
        assert_eq!(moved, Moved { game_skipped: false });

        assert!(platform::migrated_marker(&plan.files).is_file());
        assert_eq!(in_use(&plan), platform::StartState::Migrated);
        let dest = platform::android_paths(&plan.files, true);
        assert_eq!(dest.root, plan.files.join("Razdor"));
        assert_eq!(fs::read(dest.root.join("saves/Autosave.rzsave")).unwrap(), b"save bytes");
        assert_eq!(fs::read(dest.game.join("Maps_Rus/Map1.DTm")).unwrap().len(), 1000);
        // The remembered install and the hub's stamp now name the app's game folder.
        let want = dest.game.to_string_lossy().into_owned();
        assert_eq!(fs::read_to_string(dest.root.join("install")).unwrap(), want);
        let stamp = fs::read_to_string(dest.root.join("hub/mods/mod1/game/stamp.json")).unwrap();
        assert_eq!(stamp, format!("{{\"base\":\"{want}\",\"layers\":[]}}"));
        assert_eq!(fs::read(dest.root.join("saves/settings.json")).unwrap(), b"{\"volume\":3}");
        assert!(!part_of(&dest.root).exists() && !part_of(&dest.game).exists());
        assert_eq!(legacy_state_of(&plan), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn an_interrupted_copy_leaves_no_marker_the_legacy_paths_and_no_part_and_the_next_run_completes() {
        let (base, plan) = plan("interrupted");
        let before = legacy_state_of(&plan);
        // A part a killed run left behind.
        put(&part_of(&plan.files.join("Razdor")), "stale.txt", b"left by a killed run");

        let mut copied = 0;
        let mut killed_after_two = |from: &Path, to: &Path, len: u64| {
            copied += 1;
            if copied > 2 {
                return Err(io::Error::other("killed"));
            }
            copy_file(from, to, len)
        };
        let failed = migrate(&plan, &room(plenty()), &mut killed_after_two, &mut real_place, &|_, _| {});
        assert!(matches!(failed, Err(Failure::Failed(_))), "{failed:?}");
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert!(!plan.files.join("Razdor").exists() && !part_of(&plan.files.join("Razdor")).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!(legacy_state_of(&plan), before);

        let moved = run(&plan).expect("the next run moves");
        assert_eq!(moved, Moved { game_skipped: false });
        assert_eq!(in_use(&plan), platform::StartState::Migrated);
        assert!(!part_of(&plan.files.join("Razdor")).exists());
        assert!(!plan.files.join("Razdor/stale.txt").exists());
        assert_eq!(legacy_state_of(&plan), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_copy_whose_size_differs_from_the_source_is_refused() {
        let (base, plan) = plan("size");
        let before = legacy_state_of(&plan);
        // The copy writes half of the game's executable.
        let mut short = |from: &Path, to: &Path, len: u64| {
            if from.ends_with("DiscordTimes.exe") {
                let bytes = fs::read(from)?;
                return fs::write(to, &bytes[..bytes.len() / 2]);
            }
            copy_file(from, to, len)
        };
        match migrate(&plan, &room(plenty()), &mut short, &mut real_place, &|_, _| {}) {
            Err(Failure::Failed(why)) => assert!(why.contains("DiscordTimes.exe"), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert!(!plan.files.join("Razdor").exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!(legacy_state_of(&plan), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_razdor_folder_that_does_not_fit_stays_legacy_and_nothing_is_copied() {
        let (base, plan) = plan("room-root");
        let before = legacy_state_of(&plan);
        let mut copied = 0;
        let result = migrate(
            &plan,
            &room(1024),
            &mut |a: &Path, b: &Path, n: u64| {
                copied += 1;
                copy_file(a, b, n)
            },
            &mut real_place,
            &|_, _| {},
        );
        match result {
            Err(Failure::NoRoom { needed, free }) => {
                assert_eq!(free, 1024);
                assert!(needed > SPACE_MARGIN, "{needed}");
            }
            other => panic!("expected no room, got {other:?}"),
        }
        assert_eq!(copied, 0);
        assert!(!plan.files.join("Razdor").exists());
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!(legacy_state_of(&plan), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_game_that_does_not_fit_is_skipped_and_the_razdor_folder_still_moves() {
        let (base, plan) = plan("room-game");
        let before = legacy_state_of(&plan);
        let root_bytes: u64 = scan(&plan.legacy_root).unwrap().bytes;
        // Room for the Razdor folder with its margin, and less than the game needs on top of it.
        let free = root_bytes + SPACE_MARGIN + 1;
        let moved = migrate(&plan, &room(free), &mut real_copy, &mut real_place, &|_, _| {}).expect("the Razdor folder moves");
        assert_eq!(moved, Moved { game_skipped: true });
        assert!(platform::migrated_marker(&plan.files).is_file());
        let dest = platform::android_paths(&plan.files, true);
        assert!(!dest.game.exists(), "the game folder is not copied");
        assert_eq!(fs::read(dest.root.join("saves/Autosave.rzsave")).unwrap(), b"save bytes");
        // The remembered install names the new folder anyway: the not-found screen then asks for the archive.
        assert_eq!(fs::read_to_string(dest.root.join("install")).unwrap(), dest.game.to_string_lossy());
        // The hub's stamp keeps the legacy base: the game is still there.
        assert_eq!(legacy_state_of(&plan), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_second_start_with_the_marker_copies_nothing() {
        let (base, plan) = plan("second");
        run(&plan).expect("first move");
        let before = everything(&plan.files);
        let mut copied = 0;
        let moved = migrate(
            &plan,
            &room(plenty()),
            &mut |a: &Path, b: &Path, n: u64| {
                copied += 1;
                copy_file(a, b, n)
            },
            &mut real_place,
            &|_, _| {},
        );
        assert_eq!(moved, Ok(Moved { game_skipped: false }));
        assert_eq!(copied, 0);
        assert_eq!(everything(&plan.files), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn progress_reports_the_bytes_of_both_folders() {
        let (base, plan) = plan("progress");
        let seen = std::cell::RefCell::new(Vec::new());
        migrate(&plan, &room(plenty()), &mut real_copy, &mut real_place, &|done, total| seen.borrow_mut().push((done, total))).expect("moves");
        let seen = seen.into_inner();
        let total = seen.last().unwrap().1;
        assert_eq!(seen.last().unwrap().0, total);
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn an_app_folder_with_a_save_and_no_marker_is_merged_and_its_own_file_wins() {
        let (base, plan) = plan("merge");
        let legacy_before = legacy_state_of(&plan);
        // A run without the grant played from the app's folder and saved there.
        put(&plan.files.join("Razdor"), "saves/Autosave.rzsave", b"app save");
        put(&plan.files.join("Razdor"), "hub/own.json", b"app hub");
        let app_before = everything(&plan.files);

        run(&plan).expect("merges");
        assert!(platform::migrated_marker(&plan.files).is_file());
        let root = plan.files.join("Razdor");
        assert_eq!(fs::read(root.join("saves/Autosave.rzsave")).unwrap(), b"app save", "the app's own save survives");
        assert_eq!(fs::read(root.join("saves/settings.json")).unwrap(), b"{\"volume\":3}", "the legacy-only file is added");
        assert_eq!(fs::read(root.join("hub/own.json")).unwrap(), b"app hub");
        assert_eq!(fs::read(root.join("hub/mods/mod1/game/Rus_Units.ini")).unwrap(), b"units");
        assert!(!part_of(&root).exists());
        for (path, bytes) in &app_before {
            assert_eq!(everything(&plan.files).get(path), Some(bytes), "{} changed", path.display());
        }
        assert_eq!(legacy_state_of(&plan), legacy_before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn an_error_in_the_middle_of_a_merge_leaves_the_app_folder_exactly_as_it_was() {
        let (base, plan) = plan("merge-error");
        let legacy_before = legacy_state_of(&plan);
        put(&plan.files.join("Razdor"), "saves/Autosave.rzsave", b"app save");
        let app_before = everything(&plan.files);
        // The third placing fails: some files and folders of the copy are already in place.
        let mut placed = 0;
        let mut failing = |from: &Path, to: &Path| {
            placed += 1;
            if placed == 3 {
                return Err(io::Error::other("disk gone"));
            }
            real_place(from, to)
        };
        let result = migrate(&plan, &room(plenty()), &mut real_copy, &mut failing, &|_, _| {});
        assert!(matches!(result, Err(Failure::Failed(_))), "{result:?}");
        assert!(placed >= 3, "the failure was reached");
        assert_eq!(everything(&plan.files), app_before);
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!(legacy_state_of(&plan), legacy_before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_legacy_file_that_changes_while_the_copy_runs_discards_the_move() {
        let (base, plan) = plan("rescan");
        // Once the first file is copied, a legacy save is written to (an app's save, say, in the
        // legacy folder): the scan the copy used is out of date.
        let mut changed = false;
        let mut write_late = |from: &Path, to: &Path, len: u64| {
            if !changed {
                changed = true;
                use std::io::Write as _;
                fs::OpenOptions::new().append(true).open(plan.legacy_root.join("saves/settings.json")).unwrap().write_all(b"!").unwrap();
            }
            copy_file(from, to, len)
        };
        let result = migrate(&plan, &room(plenty()), &mut write_late, &mut real_place, &|_, _| {});
        match result {
            Err(Failure::Failed(why)) => assert!(why.contains("changed"), "{why}"),
            other => panic!("expected the move to start again, got {other:?}"),
        }
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert!(!plan.files.join("Razdor").exists());
        assert!(!part_of(&plan.files.join("Razdor")).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        // The next start copies the file as it is now.
        let moved = run(&plan).expect("the next start moves");
        assert_eq!(moved, Moved { game_skipped: false });
        assert_eq!(fs::read(plan.files.join("Razdor/saves/settings.json")).unwrap(), b"{\"volume\":3}!");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn the_live_log_may_grow_while_it_is_copied() {
        let (base, plan) = plan("live-log");
        put(&plan.legacy_root, "razdor.log", b"line one\n");
        let mut grow = |from: &Path, to: &Path, len: u64| {
            if from.ends_with("razdor.log") {
                use std::io::Write as _;
                fs::OpenOptions::new().append(true).open(from).unwrap().write_all(b"line two\n").unwrap();
            }
            copy_file(from, to, len)
        };
        migrate(&plan, &room(plenty()), &mut grow, &mut real_place, &|_, _| {}).expect("the log does not stop the move");
        let copied = fs::read(plan.files.join("Razdor").join("razdor.log")).unwrap();
        assert_eq!(copied, b"line one\n", "the copy takes the size the scan listed");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_play_log_written_during_the_scan_and_the_rescan_does_not_discard_the_move() {
        let (base, plan) = plan("play-log");
        put(&plan.legacy_root, "razdor-play.log", b"[1.0] menu | opened\n");
        // A line of the play log, written after the copy's scan and before the rescan.
        let mut write_play = |from: &Path, to: &Path, len: u64| {
            if from.ends_with("install") {
                use std::io::Write as _;
                fs::OpenOptions::new().append(true).open(plan.legacy_root.join("razdor-play.log")).unwrap().write_all(b"[2.0] menu | more\n").unwrap();
            }
            copy_file(from, to, len)
        };
        migrate(&plan, &room(plenty()), &mut write_play, &mut real_place, &|_, _| {}).expect("the play log does not stop the move");
        assert!(platform::migrated_marker(&plan.files).is_file());
        let copied = fs::read(plan.files.join("Razdor/razdor-play.log")).unwrap();
        assert_eq!(copied, b"[1.0] menu | opened\n", "the copy takes the size the scan listed");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_play_log_renamed_away_during_the_copy_is_left_out_and_the_move_goes_on() {
        let (base, plan) = plan("play-rename");
        put(&plan.legacy_root, "razdor-play.log", b"[1.0] menu | opened\n");
        // The play log is first written (renamed to its previous copy) before its turn to be copied.
        let mut rotate = |from: &Path, to: &Path, len: u64| {
            if from.ends_with("install") {
                fs::rename(plan.legacy_root.join("razdor-play.log"), plan.legacy_root.join("razdor-play.previous.log")).unwrap();
            }
            copy_file(from, to, len)
        };
        migrate(&plan, &room(plenty()), &mut rotate, &mut real_place, &|_, _| {}).expect("the rename does not stop the move");
        assert!(platform::migrated_marker(&plan.files).is_file());
        assert!(!plan.files.join("Razdor/razdor-play.log").exists());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_legacy_file_where_the_app_folder_has_a_folder_stops_the_move_and_nothing_is_overwritten() {
        let (base, plan) = plan("file-vs-folder");
        let legacy_before = legacy_state_of(&plan);
        // The app's folder has a folder where the legacy one has a file.
        put(&plan.files.join("Razdor"), "saves/settings.json/keep.txt", b"app");
        let app_before = everything(&plan.files);
        let result = run(&plan);
        match result {
            Err(Failure::Failed(why)) => assert!(why.contains("settings.json"), "{why}"),
            other => panic!("expected the move to stop, got {other:?}"),
        }
        assert_eq!(everything(&plan.files), app_before, "the app folder is unchanged");
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!(legacy_state_of(&plan), legacy_before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_legacy_folder_where_the_app_folder_has_a_file_stops_the_move_with_the_path_named() {
        let (base, plan) = plan("folder-vs-file");
        let legacy_before = legacy_state_of(&plan);
        // The app's folder has a file where the legacy one has the hub's folder.
        put(&plan.files, "Razdor/hub", b"a file, not a folder");
        let app_before = everything(&plan.files);
        match run(&plan) {
            Err(Failure::Failed(why)) => assert!(why.contains("hub"), "{why}"),
            other => panic!("expected the move to stop, got {other:?}"),
        }
        assert_eq!(everything(&plan.files), app_before, "the app folder is unchanged");
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!(legacy_state_of(&plan), legacy_before);
        let _ = fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_skipped_and_a_dangling_one_does_not_fail_the_move() {
        let (base, plan) = plan("links");
        let legacy_before = legacy_state_of(&plan);
        std::os::unix::fs::symlink(plan.legacy_root.join("saves/settings.json"), plan.legacy_root.join("saves/link.json")).unwrap();
        std::os::unix::fs::symlink(base.join("nowhere"), plan.legacy_root.join("saves/dangling.json")).unwrap();
        run(&plan).expect("the move goes on past the links");
        let root = plan.files.join("Razdor");
        assert!(fs::symlink_metadata(root.join("saves/link.json")).is_err());
        assert!(fs::symlink_metadata(root.join("saves/dangling.json")).is_err());
        assert_eq!(fs::read(root.join("saves/settings.json")).unwrap(), b"{\"volume\":3}");
        // The links themselves are left as they were.
        assert!(fs::symlink_metadata(plan.legacy_root.join("saves/link.json")).unwrap().file_type().is_symlink());
        assert_eq!(legacy_state_of(&plan).0.len(), legacy_before.0.len() + 2);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn only_whole_legacy_paths_are_rewritten() {
        let pairs = [(LEGACY_ANDROID_ROOT, "/new/Razdor"), (LEGACY_ANDROID_GAME, "/new/DiscordTimes")];
        let g = LEGACY_ANDROID_GAME;
        // Whole paths: followed by a separator, a quote, a space or the end of the text.
        assert_eq!(rewrite_text(&format!("{g}/Maps_Rus"), &pairs).unwrap(), "/new/DiscordTimes/Maps_Rus");
        assert_eq!(rewrite_text(&format!("{{\"a\":\"{g}\"}}"), &pairs).unwrap(), "{\"a\":\"/new/DiscordTimes\"}");
        assert_eq!(rewrite_text(&format!("path {g} here"), &pairs).unwrap(), "path /new/DiscordTimes here");
        assert_eq!(rewrite_text(&format!("{LEGACY_ANDROID_ROOT}\\saves"), &pairs).unwrap(), "/new/Razdor\\saves");
        // Other names that start the same way are other folders: untouched.
        assert_eq!(rewrite_text(&format!("{g}-Evolution"), &pairs), None);
        assert_eq!(rewrite_text(&format!("{g}_old/x"), &pairs), None);
        assert_eq!(rewrite_text(&format!("{g}.bak"), &pairs), None);
        assert_eq!(rewrite_text(&format!("{g}2"), &pairs), None);
        assert_eq!(rewrite_text("install: /games/DiscordTimes", &pairs), None);
        // Mixed: the whole path changes, the other name next to it does not.
        assert_eq!(rewrite_text(&format!("{g}-Evolution {g}"), &pairs).unwrap(), format!("{g}-Evolution /new/DiscordTimes"));
        assert_eq!(rewrite_text("ünïcode /storage/emulated/0/DiscordTimes/ü", &pairs).unwrap(), "ünïcode /new/DiscordTimes/ü");
    }
}
