//! The game folder against the community's archive (`data/game-manifest.json`, made from the Drive
//! archive by `scripts/game-manifest.py`: every file's path, size and SHA-256, no game content).
//!
//! Only a Community Update build is checked: its exe is the manifest's exe, or most of the files
//! are there at the archive's sizes. Another build (Evolution's own exe, an old install) is
//! skipped and logged. Extra files (translations, user maps, saves) are ignored. A MISSING file is
//! damage: the loader refuses to start on it (`DtError::MissingGameFiles`, the screen offers the
//! repair). A CHANGED file may be the player's own edit: it is only reported, and it is replaced
//! only by the repair the player starts. Nothing here needs macroquad.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::install::EXE_FILE;

/// The manifest the build carries.
pub const MANIFEST: &str = include_str!("../../data/game-manifest.json");

/// The archive the manifest describes, and the folder it is under.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ArchiveInfo {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    pub top: String,
}

/// One file of the archive: its path under the top folder (`/` separated), size and SHA-256.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub archive: ArchiveInfo,
    pub files: Vec<FileEntry>,
}

/// What the check found in a Community Update install. `community` false: nothing is reported.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub community: bool,
    /// Files the archive has and the folder lacks (damage: the repair restores them).
    pub missing: Vec<String>,
    /// Files whose size (or, with a full check, SHA-256) differs: maybe the player's own edit.
    pub changed: Vec<String>,
}

/// Parses a manifest, refusing paths that could leave the folder and malformed hashes.
pub fn parse(text: &str) -> Result<Manifest, String> {
    let m: Manifest = serde_json::from_str(text).map_err(|e| format!("the game manifest is unreadable: {e}"))?;
    if m.files.is_empty() {
        return Err("the game manifest lists no files".into());
    }
    if m.archive.top.is_empty() || m.archive.top.contains(['/', '\\']) {
        return Err("the game manifest's archive folder is not a name".into());
    }
    for f in &m.files {
        if !safe_rel(&f.path) {
            return Err(format!("the game manifest has an unsafe path: {}", f.path));
        }
        if f.sha256.len() != 64 || !f.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("the game manifest has a bad SHA-256 for {}", f.path));
        }
    }
    Ok(m)
}

/// The manifest the build carries ([`MANIFEST`]).
pub fn builtin() -> Result<Manifest, String> {
    parse(MANIFEST)
}

/// A relative path of the archive that stays inside its folder: no absolute part, no `..`, no
/// backslash, not empty.
pub fn safe_rel(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Compares the folder `dir` with the manifest. `full` also hashes the files that are there (an
/// explicit check); without it a file is changed when its size differs. A folder that is not a
/// Community Update build gives an empty report with `community` false.
pub fn check(dir: &Path, m: &Manifest, full: bool) -> Report {
    let exe_matches = m
        .files
        .iter()
        .find(|f| f.path == EXE_FILE)
        .is_some_and(|exe| file_sha(&dir.join(EXE_FILE)).is_some_and(|sha| sha == exe.sha256));
    let mut report = Report::default();
    let mut same_size = 0usize;
    for f in &m.files {
        let path = dir.join(&f.path);
        match fs::metadata(&path) {
            Ok(meta) if meta.is_file() => {
                if meta.len() != f.size {
                    report.changed.push(f.path.clone());
                } else {
                    same_size += 1;
                    if full && file_sha(&path).is_some_and(|sha| sha != f.sha256) {
                        report.changed.push(f.path.clone());
                    }
                }
            }
            _ => report.missing.push(f.path.clone()),
        }
    }
    report.community = exe_matches || same_size * 2 > m.files.len();
    if !report.community {
        return Report::default();
    }
    report
}

/// The files of the archive that `dir` must hold to be a whole game: the missing ones, as
/// [`Report::missing`] (the loader's guard). Empty for a folder that is not a Community build.
pub fn missing_files(dir: &Path, m: &Manifest) -> Vec<String> {
    check(dir, m, false).missing
}

/// Whether Razdor reads the archive's file `path` when it plays. Razdor's own filter: the
/// original checks no files at all, so a file it never reads must not stop the game (a folder
/// without the modder's notepads or the exe still plays). Not needed: the notepads folder, the
/// help folder, the root's programs and libraries (`.exe`, `.dll`, `.ico`), `License.txt`, and the
/// ini files of the map editor and the DirectDraw shim. Everything else (the data, maps, pictures,
/// sounds, logo) is needed. The exe is optional for the game: `dt::install::drain_rule` falls back
/// to `_Global.ini`. Case-insensitive.
pub fn needed_to_play(path: &str) -> bool {
    match path.split_once('/') {
        Some((folder, _)) => !(folder.to_lowercase() == NOTEPADS_FOLDER.to_lowercase() || folder.eq_ignore_ascii_case("Help")),
        None => !unused_root_file(path),
    }
}

/// The modder's notepads (`Дополнительные файлы`): the Community Update's extras, never read.
const NOTEPADS_FOLDER: &str = "Дополнительные файлы";

/// The root ini files of the map editor and the DirectDraw shim (lowercase).
const UNUSED_ROOT_INI: [&str; 4] = ["dtmapedit.ini", "dtmapedit_rus.ini", "rus_mapedit.ini", "ddrawcompat.ini"];

/// A file at the archive's root that the game never reads: a program or a library, its icon, the
/// license, or an editor's or the shim's ini.
fn unused_root_file(name: &str) -> bool {
    let lower = name.to_lowercase();
    let ext = lower.rsplit_once('.').map(|(_, ext)| ext);
    matches!(ext, Some("exe" | "dll" | "ico")) || lower == "license.txt" || UNUSED_ROOT_INI.contains(&lower.as_str())
}

/// Restores the files `rels` of the archive into `dir`, from the archive at `zip_path` (the
/// archive's own folder is `m.archive.top`). Each file is written beside its place, checked
/// against its size and SHA-256, and then renamed into place; a file that does not check is not
/// written. Returns how many were written. Only the listed files are touched.
pub fn repair(dir: &Path, m: &Manifest, zip_path: &Path, rels: &[String]) -> Result<usize, String> {
    if rels.is_empty() {
        return Ok(0);
    }
    let file = fs::File::open(zip_path).map_err(|e| format!("{}: {e}", zip_path.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("the archive cannot be read: {e}"))?;
    let mut written = 0;
    for rel in rels {
        let Some(entry) = m.files.iter().find(|f| &f.path == rel) else {
            return Err(format!("{rel} is not in the game manifest"));
        };
        if !safe_rel(rel) {
            return Err(format!("{rel} is not a safe path"));
        }
        let name = format!("{}/{}", m.archive.top, rel);
        let mut src = zip.by_name(&name).map_err(|e| format!("{rel}: {e}"))?;
        let dest = dir.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut part = dest.clone().into_os_string();
        part.push(".razdor-part");
        let part = PathBuf::from(part);
        {
            let mut out = fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
            // Never more than the manifest's size: a bigger entry fails the check below.
            io::copy(&mut (&mut src).take(entry.size + 1), &mut out).map_err(|e| format!("{rel}: {e}"))?;
        }
        let ok = fs::metadata(&part).map(|meta| meta.len() == entry.size).unwrap_or(false) && file_sha(&part).is_some_and(|sha| sha == entry.sha256);
        if !ok {
            let _ = fs::remove_file(&part);
            return Err(format!("the archive's copy of {rel} is not the expected file"));
        }
        fs::rename(&part, &dest).map_err(|e| {
            let _ = fs::remove_file(&part);
            format!("{}: {e}", dest.display())
        })?;
        written += 1;
    }
    Ok(written)
}

/// The SHA-256 of a file, or None when it cannot be read.
fn file_sha(path: &Path) -> Option<String> {
    crate::update::sha256_file(path).ok()
}

/// The folder's files as the manifest names them, for a test (a helper: the bytes are read whole).
#[cfg(test)]
pub fn read_all(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?.read_to_end(&mut bytes)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    /// A small game in a temp folder: the files, a zip of them under the top folder, and the
    /// manifest that describes them.
    struct Fixture {
        dir: PathBuf,
        zip: PathBuf,
        manifest: Manifest,
    }

    const TOP: &str = "Discord Times Community Update (Unstable)";

    fn files() -> Vec<(&'static str, Vec<u8>)> {
        vec![
            ("DiscordTimes.exe", b"MZ community exe".to_vec()),
            ("_Global.ini", b"[Global]\r\nName=1\r\n".to_vec()),
            ("Rus_Units.ini", b"[Units]\r\nunits=1\r\n".to_vec()),
            ("Maps_Rus/a.DTm", b"map a".to_vec()),
            ("Graphics/knight.lit", b"picture".to_vec()),
        ]
    }

    fn manifest_of(files: &[(&str, Vec<u8>)]) -> Manifest {
        Manifest {
            archive: ArchiveInfo { name: "game.zip".into(), size: 0, sha256: "0".repeat(64), top: TOP.into() },
            files: files
                .iter()
                .map(|(path, bytes)| FileEntry { path: path.to_string(), size: bytes.len() as u64, sha256: crate::update::sha256_hex(bytes) })
                .collect(),
        }
    }

    fn fixture(name: &str) -> Fixture {
        let root = std::env::temp_dir().join(format!("razdor-integrity-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let dir = root.join("DiscordTimes");
        let good = files();
        for (path, bytes) in &good {
            let p = dir.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(&p, bytes).unwrap();
        }
        let zip = root.join("game.zip");
        let mut w = zip::ZipWriter::new(io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (path, bytes) in &good {
            w.start_file(format!("{TOP}/{path}"), opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        fs::write(&zip, w.finish().unwrap().into_inner()).unwrap();
        let manifest = manifest_of(&good);
        Fixture { dir, zip, manifest }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            if let Some(root) = self.dir.parent() {
                let _ = fs::remove_dir_all(root);
            }
        }
    }

    #[test]
    fn the_built_in_manifest_parses_and_names_the_exe() {
        let m = builtin().expect("the manifest in data/ parses");
        assert_eq!(m.archive.size, 46_726_043);
        assert_eq!(m.archive.sha256, "fce6d8aa22f8932cddf0dffaf02d3c013d15b298148fc2c50154306d24f9f37e");
        assert_eq!(m.archive.top, "Discord Times Community Update (Unstable)");
        assert!(m.files.iter().any(|f| f.path == EXE_FILE), "the exe is listed");
        assert!(m.files.len() > 500, "the whole folder is listed: {}", m.files.len());
    }

    #[test]
    fn a_manifest_with_an_unsafe_path_or_a_bad_hash_is_refused() {
        let good = r#"{"archive":{"name":"g.zip","size":1,"sha256":"00","top":"T"},"files":[{"path":"a.ini","size":1,"sha256":"%H%"}]}"#;
        assert!(parse(&good.replace("%H%", &"a".repeat(64))).is_ok());
        assert!(parse(&good.replace("%H%", "xyz")).is_err(), "a bad hash");
        let escape = good.replace("a.ini", "../a.ini").replace("%H%", &"a".repeat(64));
        assert!(parse(&escape).unwrap_err().contains("unsafe path"));
        assert!(parse("{").is_err());
        assert!(parse(r#"{"archive":{"name":"g","size":1,"sha256":"00","top":"T"},"files":[]}"#).is_err());
        assert!(safe_rel("Maps_Rus/a.DTm") && !safe_rel("/a") && !safe_rel("a/../b") && !safe_rel("a\\b") && !safe_rel(""));
    }

    #[test]
    fn needed_to_play_drops_only_what_the_game_never_reads() {
        for unused in [
            "Дополнительные файлы/Блокноты с ID/Армия.txt",
            "дополнительные файлы/x.txt",
            "Help/Документация к Редактору Сценариев.doc",
            "help/x.doc",
            "DiscordTimes.exe",
            "discordtimes.EXE",
            "ddraw.dll",
            "detoured.dll",
            "ogg.dll",
            "vorbis.dll",
            "vorbisfile.dll",
            "help.ico",
            "Help.ICO",
            "License.txt",
            "license.txt",
            "DTMapEdit.Ini",
            "DTMapEdit_Rus.Ini",
            "Rus_MapEdit.ini",
            "DDrawCompat.ini",
            "ddrawcompat.INI",
        ] {
            assert!(!needed_to_play(unused), "{unused} is not read by the game");
        }
        for needed in [
            "Graphics/knight.lit",
            "Maps_Rus/a.DTm",
            "Rus_Units.ini",
            "Rus_Artefacts.ini",
            "Rus_Spells.ini",
            "Rus_DiscordTimes.ini",
            "_Global.ini",
            "_Sounds.ini",
            "Sounds/battle1.raw",
            "Logo/AE_Logo.lit",
        ] {
            assert!(needed_to_play(needed), "{needed} is read by the game");
        }
    }

    #[test]
    fn the_built_in_manifest_needs_exactly_the_root_files_the_game_reads() {
        let m = builtin().unwrap();
        let mut root_needed: Vec<&str> = m.files.iter().map(|f| f.path.as_str()).filter(|p| !p.contains('/') && needed_to_play(p)).collect();
        root_needed.sort_unstable();
        assert_eq!(
            root_needed,
            ["Rus_Artefacts.ini", "Rus_DiscordTimes.ini", "Rus_Spells.ini", "Rus_Units.ini", "_Global.ini", "_Sounds.ini"],
        );
    }

    #[test]
    fn a_whole_community_install_has_nothing_missing_or_changed() {
        let f = fixture("whole");
        let report = check(&f.dir, &f.manifest, true);
        assert!(report.community);
        assert!(report.missing.is_empty() && report.changed.is_empty(), "{report:?}");
    }

    #[test]
    fn a_missing_file_is_restored_from_the_cached_zip_and_nothing_else_is_touched() {
        let f = fixture("missing");
        fs::remove_file(f.dir.join("Maps_Rus/a.DTm")).unwrap();
        fs::write(f.dir.join("extra_user_map.DTm"), b"a user map").unwrap();
        let before = read_all(&f.dir.join("Graphics/knight.lit")).unwrap();

        assert_eq!(missing_files(&f.dir, &f.manifest), vec!["Maps_Rus/a.DTm".to_string()]);
        let n = repair(&f.dir, &f.manifest, &f.zip, &["Maps_Rus/a.DTm".to_string()]).unwrap();
        assert_eq!(n, 1);
        assert_eq!(read_all(&f.dir.join("Maps_Rus/a.DTm")).unwrap(), b"map a");
        assert_eq!(read_all(&f.dir.join("Graphics/knight.lit")).unwrap(), before, "other files untouched");
        assert_eq!(read_all(&f.dir.join("extra_user_map.DTm")).unwrap(), b"a user map", "extra files are kept");
        assert!(check(&f.dir, &f.manifest, true).missing.is_empty());
    }

    #[test]
    fn a_changed_file_is_reported_and_not_overwritten_until_asked() {
        let f = fixture("changed");
        let edited = b"[Units]\r\nunits=9\r\n".to_vec();
        assert_eq!(edited.len(), files()[2].1.len(), "the same size: only the hash tells them apart");
        fs::write(f.dir.join("Rus_Units.ini"), &edited).unwrap();

        let quick = check(&f.dir, &f.manifest, false);
        assert!(quick.community && quick.changed.is_empty(), "a quick check reads sizes only: {quick:?}");
        let full = check(&f.dir, &f.manifest, true);
        assert_eq!(full.changed, vec!["Rus_Units.ini".to_string()]);
        assert_eq!(read_all(&f.dir.join("Rus_Units.ini")).unwrap(), edited, "reporting does not overwrite");

        // A repair of other files leaves the edit alone.
        fs::remove_file(f.dir.join("_Global.ini")).unwrap();
        repair(&f.dir, &f.manifest, &f.zip, &["_Global.ini".to_string()]).unwrap();
        assert_eq!(read_all(&f.dir.join("Rus_Units.ini")).unwrap(), edited);

        // Asked for by the player: the archive's bytes come back.
        repair(&f.dir, &f.manifest, &f.zip, &["Rus_Units.ini".to_string()]).unwrap();
        assert_eq!(read_all(&f.dir.join("Rus_Units.ini")).unwrap(), files()[2].1);
    }

    #[test]
    fn another_build_is_skipped_and_reports_nothing() {
        let f = fixture("other-build");
        // An Evolution-like exe, and the other files cut down to a few: not the community's folder.
        fs::write(f.dir.join("DiscordTimes.exe"), b"an other exe of another size").unwrap();
        fs::remove_file(f.dir.join("Maps_Rus/a.DTm")).unwrap();
        fs::remove_file(f.dir.join("Graphics/knight.lit")).unwrap();
        fs::write(f.dir.join("_Global.ini"), b"changed").unwrap();
        let report = check(&f.dir, &f.manifest, true);
        assert_eq!(report, Report::default(), "another build is not checked");
        assert!(missing_files(&f.dir, &f.manifest).is_empty(), "the loader does not refuse it");
    }

    #[test]
    fn repair_refuses_an_archive_copy_that_does_not_check_and_writes_nothing() {
        let f = fixture("bad-copy");
        fs::remove_file(f.dir.join("Maps_Rus/a.DTm")).unwrap();
        // The manifest now expects other bytes than the archive holds.
        let mut m = f.manifest.clone();
        for e in &mut m.files {
            if e.path == "Maps_Rus/a.DTm" {
                e.sha256 = crate::update::sha256_hex(b"another map");
            }
        }
        let err = repair(&f.dir, &m, &f.zip, &["Maps_Rus/a.DTm".to_string()]).unwrap_err();
        assert!(err.contains("not the expected file"), "{err}");
        assert!(!f.dir.join("Maps_Rus/a.DTm").exists());
        assert!(!f.dir.join("Maps_Rus/a.DTm.razdor-part").exists());
    }

    #[test]
    fn repair_names_a_path_the_manifest_does_not_hold() {
        let f = fixture("unknown");
        let err = repair(&f.dir, &f.manifest, &f.zip, &["nope.ini".to_string()]).unwrap_err();
        assert!(err.contains("not in the game manifest"), "{err}");
    }
}
