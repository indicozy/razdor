//! Moving the player's files from the shared storage into the app's own folder on Android
//! (release N: the storage permission still reads the old folders, so the copy can run; the
//! release after drops the permission). Pure Rust, compiled on every target: the thread and the
//! screen are `ui::migrate`.
//!
//! Each folder is copied into `<dest>.part` and every file's name and size are compared with the
//! source. The absolute legacy paths stored in the copy's text files are rewritten to the new
//! ones (on the copy, never on the source). Only then are the folders renamed into place, and the
//! marker ([`platform::migrated_marker`]) is written. The caller flips the paths after that. The
//! legacy folders are only read: nothing is written, moved or deleted there.

use crate::dt::game_import::SPACE_MARGIN;
use crate::platform::{self, LEGACY_ANDROID_GAME, LEGACY_ANDROID_ROOT};
use std::cell::Cell;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Text files bigger than this are not searched for legacy paths (saves, settings and the hub's
/// small records are far smaller; the game's own files are never searched).
const REWRITE_LIMIT: u64 = 8 << 20;
/// Folders nested deeper than this are refused (a link loop, say).
const MAX_DEPTH: u32 = 32;

/// Copies the first `len` bytes of `from` to a new file `to` (`len` is the size the scan listed:
/// a live file such as the log may grow meanwhile). [`copy_file`] is the real one.
pub type CopyFile<'a> = dyn FnMut(&Path, &Path, u64) -> io::Result<()> + 'a;

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
/// room (the archive is asked for when the legacy folder is not readable any more).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Moved {
    pub game_skipped: bool,
}

/// The real copy of one file (`len` bytes, see [`CopyFile`]), then its modification time (the
/// hub's records compare times, so the copy keeps them).
pub fn copy_file(from: &Path, to: &Path, len: u64) -> io::Result<()> {
    let mut source = fs::File::open(from)?;
    let mut copy = fs::File::create_new(to)?;
    io::copy(&mut io::Read::take(&mut source, len), &mut copy)?;
    drop(copy);
    let modified = fs::metadata(from)?.modified()?;
    fs::OpenOptions::new().write(true).open(to)?.set_modified(modified)
}

/// Moves the legacy folders of `plan` into the app's folder (see the module docs). `free` gives
/// the free bytes of a folder ([`crate::dt::game_import::free_space`], or a test's); `copy`
/// copies one file; `progress` gets the bytes copied so far and the total. A second call after
/// the marker copies nothing.
pub fn migrate(plan: &Plan, free: &dyn Fn(&Path) -> Option<u64>, copy: &mut CopyFile<'_>, progress: &dyn Fn(u64, u64)) -> Result<Moved, Failure> {
    let dest = platform::android_paths(&plan.files, true);
    let result = move_in(plan, &dest, free, copy, progress);
    if result.is_err() {
        // Only what this move made: the parts, and the folders renamed into place.
        for path in [part_of(&dest.root), part_of(&dest.game), dest.root.clone(), dest.game.clone()] {
            let _ = fs::remove_dir_all(path);
        }
    }
    result
}

fn move_in(plan: &Plan, dest: &platform::AndroidPaths, free: &dyn Fn(&Path) -> Option<u64>, copy: &mut CopyFile<'_>, progress: &dyn Fn(u64, u64)) -> Result<Moved, Failure> {
    if platform::migrated_marker(&plan.files).exists() {
        return Ok(Moved { game_skipped: false });
    }
    let root = scan(&plan.legacy_root).map_err(io_failure)?;
    let game = scan(&plan.legacy_game).map_err(io_failure)?;
    let game_there = !game.files.is_empty();
    // Left by an interrupted move. With no marker nothing in the app's folder is the player's
    // yet (the paths are the legacy ones until the marker), so these go.
    for path in [part_of(&dest.root), part_of(&dest.game), dest.root.clone(), dest.game.clone()] {
        let _ = fs::remove_dir_all(path);
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
    if game_fits {
        copy_tree(&plan.legacy_game, &part_of(&dest.game), &game, copy, &note)?;
    }

    fs::rename(&part_root, &dest.root).map_err(io_failure)?;
    if game_fits {
        fs::rename(part_of(&dest.game), &dest.game).map_err(io_failure)?;
    }
    let game_skipped = game_there && !game_fits;
    let how = if game_fits { format!("from {LEGACY_ANDROID_ROOT} and {LEGACY_ANDROID_GAME}") } else { format!("from {LEGACY_ANDROID_ROOT}") };
    platform::write_migrated_marker(&plan.files, &how);
    if !platform::migrated_marker(&plan.files).exists() {
        return Err(Failure::Failed("the marker could not be written".into()));
    }
    Ok(Moved { game_skipped })
}

/// `<path>.part`, the folder a copy is written into before its rename.
fn part_of(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

fn io_failure(e: io::Error) -> Failure {
    Failure::Failed(e.to_string())
}

/// The folders and files of a tree (paths relative to its root), with the files' bytes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Tree {
    dirs: Vec<PathBuf>,
    files: Vec<(PathBuf, u64)>,
    bytes: u64,
}

/// What `root` holds. A missing folder is an empty tree.
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
        let meta = fs::metadata(&path)?;
        let rel = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
        if meta.is_dir() {
            tree.dirs.push(rel);
            walk(root, &path, depth + 1, tree)?;
        } else if meta.is_file() {
            tree.bytes += meta.len();
            tree.files.push((rel, meta.len()));
        }
    }
    Ok(())
}

/// Copies `src` (as `tree` lists it) into the new folder `part`, then compares what is there
/// with the list: every file's name and size, and the count. `copied` gets each file's size.
fn copy_tree(src: &Path, part: &Path, tree: &Tree, copy: &mut CopyFile<'_>, copied: &dyn Fn(u64)) -> Result<(), Failure> {
    fs::create_dir_all(part).map_err(io_failure)?;
    for dir in &tree.dirs {
        fs::create_dir_all(part.join(dir)).map_err(io_failure)?;
    }
    for (rel, len) in &tree.files {
        copy(&src.join(rel), &part.join(rel), *len).map_err(io_failure)?;
        copied(*len);
    }
    verify(tree, &scan(part).map_err(io_failure)?)
}

/// The copy's list against the source's: the same count, names and sizes.
fn verify(expected: &Tree, got: &Tree) -> Result<(), Failure> {
    if expected.files.len() != got.files.len() {
        return Err(Failure::Failed(format!("the copy has {} files, the source {}", got.files.len(), expected.files.len())));
    }
    for ((rel, len), (got_rel, got_len)) in expected.files.iter().zip(&got.files) {
        if rel != got_rel {
            return Err(Failure::Failed(format!("{} is not in the copy", rel.display())));
        }
        if len != got_len {
            return Err(Failure::Failed(format!("{}: {got_len} bytes copied, {len} in the source", rel.display())));
        }
    }
    Ok(())
}

/// Rewrites the legacy paths in the text files under `dir`, each `(legacy, new)` pair.
fn rewrite_tree(dir: &Path, pairs: &[(&str, &str)]) -> io::Result<()> {
    for (rel, len) in scan(dir)?.files {
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

/// `text` with each whole legacy path replaced by its new one: a match must not be followed by
/// a letter or a digit (`/storage/emulated/0/DiscordTimes2` is another folder). None when
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
        let hit = pairs.iter().find(|(from, _)| {
            text[i..].starts_with(from) && text[i + from.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric())
        });
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

    /// Every file under `dir` with its bytes: the legacy folders' state before and after a move.
    fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else { return };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    out.insert(path.strip_prefix(root).unwrap().to_path_buf(), fs::read(&path).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
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

    /// The real copy, for the tests' moves.
    fn real_copy(from: &Path, to: &Path, len: u64) -> io::Result<()> {
        copy_file(from, to, len)
    }

    fn room(bytes: u64) -> impl Fn(&Path) -> Option<u64> {
        move |_: &Path| Some(bytes)
    }

    fn plenty() -> u64 {
        10 << 30
    }

    /// The paths the app uses: the legacy ones while the marker is absent, then the app's own.
    fn in_use(plan: &Plan) -> platform::StartState {
        platform::start_state(platform::migrated_marker(&plan.files).exists(), platform::legacy_state(&plan.legacy_root), platform::legacy_state(&plan.legacy_game))
    }

    #[test]
    fn a_full_copy_writes_the_marker_flips_the_paths_and_rewrites_the_stored_paths() {
        let (base, plan) = plan("full");
        let before = (snapshot(&plan.legacy_root), snapshot(&plan.legacy_game));
        assert_eq!(in_use(&plan), platform::StartState::Legacy);

        let moved = migrate(&plan, &room(plenty()), &mut real_copy, &|_, _| {}).expect("moves");
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
        // A file without a stored path is copied as it was.
        assert_eq!(fs::read(dest.root.join("saves/settings.json")).unwrap(), b"{\"volume\":3}");
        // No part is left behind.
        assert!(!part_of(&dest.root).exists() && !part_of(&dest.game).exists());
        // The legacy folders are byte for byte as they were.
        assert_eq!((snapshot(&plan.legacy_root), snapshot(&plan.legacy_game)), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn an_interrupted_copy_leaves_no_marker_the_legacy_paths_and_no_part_and_the_next_run_completes() {
        let (base, plan) = plan("interrupted");
        let before = (snapshot(&plan.legacy_root), snapshot(&plan.legacy_game));
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
        let failed = migrate(&plan, &room(plenty()), &mut killed_after_two, &|_, _| {});
        assert!(matches!(failed, Err(Failure::Failed(_))), "{failed:?}");
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert!(!plan.files.join("Razdor").exists() && !part_of(&plan.files.join("Razdor")).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!((snapshot(&plan.legacy_root), snapshot(&plan.legacy_game)), before);

        // The next start: the stale parts are gone and the copy goes through.
        let moved = migrate(&plan, &room(plenty()), &mut real_copy, &|_, _| {}).expect("the next run moves");
        assert_eq!(moved, Moved { game_skipped: false });
        assert_eq!(in_use(&plan), platform::StartState::Migrated);
        assert!(!part_of(&plan.files.join("Razdor")).exists());
        assert!(!plan.files.join("Razdor/stale.txt").exists());
        assert_eq!((snapshot(&plan.legacy_root), snapshot(&plan.legacy_game)), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_copy_whose_size_differs_from_the_source_is_refused() {
        let (base, plan) = plan("size");
        let before = (snapshot(&plan.legacy_root), snapshot(&plan.legacy_game));
        // The copy writes half of the game's executable.
        let mut short = |from: &Path, to: &Path, len: u64| {
            if from.ends_with("DiscordTimes.exe") {
                let bytes = fs::read(from)?;
                return fs::write(to, &bytes[..bytes.len() / 2]);
            }
            copy_file(from, to, len)
        };
        let result = migrate(&plan, &room(plenty()), &mut short, &|_, _| {});
        match result {
            Err(Failure::Failed(why)) => assert!(why.contains("DiscordTimes.exe"), "{why}"),
            other => panic!("expected a refusal, got {other:?}"),
        }
        assert!(!platform::migrated_marker(&plan.files).exists());
        assert_eq!(in_use(&plan), platform::StartState::Legacy);
        assert_eq!((snapshot(&plan.legacy_root), snapshot(&plan.legacy_game)), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_razdor_folder_that_does_not_fit_stays_legacy_and_nothing_is_copied() {
        let (base, plan) = plan("room-root");
        let before = (snapshot(&plan.legacy_root), snapshot(&plan.legacy_game));
        let mut copied = 0;
        let result = migrate(&plan, &room(1024), &mut |a: &Path, b: &Path, n: u64| {
            copied += 1;
            copy_file(a, b, n)
        }, &|_, _| {});
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
        assert_eq!((snapshot(&plan.legacy_root), snapshot(&plan.legacy_game)), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_game_that_does_not_fit_is_skipped_and_the_razdor_folder_still_moves() {
        let (base, plan) = plan("room-game");
        let before = (snapshot(&plan.legacy_root), snapshot(&plan.legacy_game));
        let root_bytes: u64 = scan(&plan.legacy_root).unwrap().bytes;
        // Room for the Razdor folder with its margin, and less than the game needs on top of it.
        let free = root_bytes + SPACE_MARGIN + 1;
        let moved = migrate(&plan, &room(free), &mut real_copy, &|_, _| {}).expect("the Razdor folder moves");
        assert_eq!(moved, Moved { game_skipped: true });
        assert!(platform::migrated_marker(&plan.files).is_file());
        let dest = platform::android_paths(&plan.files, true);
        assert!(!dest.game.exists(), "the game folder is not copied");
        assert_eq!(fs::read(dest.root.join("saves/Autosave.rzsave")).unwrap(), b"save bytes");
        // The game is still at its legacy place, so the remembered path keeps naming it.
        assert_eq!(fs::read_to_string(dest.root.join("install")).unwrap(), LEGACY_ANDROID_GAME);
        assert_eq!((snapshot(&plan.legacy_root), snapshot(&plan.legacy_game)), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_second_start_with_the_marker_copies_nothing() {
        let (base, plan) = plan("second");
        migrate(&plan, &room(plenty()), &mut real_copy, &|_, _| {}).expect("first move");
        let before = snapshot(&plan.files);
        let mut copied = 0;
        let moved = migrate(&plan, &room(plenty()), &mut |a: &Path, b: &Path, n: u64| {
            copied += 1;
            copy_file(a, b, n)
        }, &|_, _| {});
        assert_eq!(moved, Ok(Moved { game_skipped: false }));
        assert_eq!(copied, 0);
        assert_eq!(snapshot(&plan.files), before);
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn progress_reports_the_bytes_of_both_folders() {
        let (base, plan) = plan("progress");
        let seen = std::cell::RefCell::new(Vec::new());
        migrate(&plan, &room(plenty()), &mut real_copy, &|done, total| seen.borrow_mut().push((done, total))).expect("moves");
        let seen = seen.into_inner();
        let total = seen.last().unwrap().1;
        assert_eq!(seen.last().unwrap().0, total);
        assert!(seen.windows(2).all(|w| w[0].0 <= w[1].0));
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn a_file_that_grows_while_it_is_copied_is_copied_to_its_size_at_the_scan() {
        // The live log is appended to by the running app: the copy takes the size the scan listed.
        let (base, plan) = plan("grows");
        let log = plan.legacy_root.join("saves/Autosave.rzsave");
        let mut grow = |from: &Path, to: &Path, len: u64| {
            if from.ends_with("Autosave.rzsave") {
                use std::io::Write as _;
                fs::OpenOptions::new().append(true).open(from).unwrap().write_all(b" and more").unwrap();
            }
            copy_file(from, to, len)
        };
        migrate(&plan, &room(plenty()), &mut grow, &|_, _| {}).expect("moves");
        let dest = platform::android_paths(&plan.files, true);
        assert_eq!(fs::read(dest.root.join("saves/Autosave.rzsave")).unwrap(), b"save bytes");
        assert!(fs::read(&log).unwrap().len() > b"save bytes".len());
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn only_whole_legacy_paths_are_rewritten() {
        let pairs = [(LEGACY_ANDROID_ROOT, "/new/Razdor"), (LEGACY_ANDROID_GAME, "/new/DiscordTimes")];
        let text = format!("{{\"a\":\"{LEGACY_ANDROID_GAME}\",\"b\":\"{LEGACY_ANDROID_ROOT}/saves\",\"c\":\"{LEGACY_ANDROID_GAME}2\"}}");
        assert_eq!(rewrite_text(&text, &pairs).unwrap(), "{\"a\":\"/new/DiscordTimes\",\"b\":\"/new/Razdor/saves\",\"c\":\"/storage/emulated/0/DiscordTimes2\"}");
        assert_eq!(rewrite_text("install: /games/DiscordTimes", &pairs), None);
        assert_eq!(rewrite_text("ünïcode /storage/emulated/0/DiscordTimes/ü", &pairs).unwrap(), "ünïcode /new/DiscordTimes/ü");
    }
}
