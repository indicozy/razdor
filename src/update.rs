//! Updates from the GitHub releases: whether a newer Razdor is out, and putting it in place
//! of this program.
//!
//! Everything runs on a background thread and only changes [`status`]; the game never waits
//! on it. The requests go through the system's `curl` (Windows 10 and later, macOS and
//! almost every Linux have it): without one the check fails quietly. A download counts only
//! when its SHA-256 matches the release's `SHA256SUMS`.
//!
//! The new program replaces this one by a rename: on Unix over the running file (which keeps
//! running from its old inode), on Windows after moving the running `Razdor.exe` aside to
//! `Razdor.exe.old`, deleted at the next start. It runs from the next start on.
//!
//! A program under a `target` folder (a `cargo` build) checks nothing, unless
//! `RAZDOR_UPDATE_AS=<X.Y.Z>` is set: it then counts as that version, to try the whole flow
//! against the real releases.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

/// The newest releases on GitHub, as the API lists them (newest first).
const RELEASES: &str = "https://api.github.com/repos/indicozy/razdor/releases?per_page=30";

/// What the settings let Razdor do about updates.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// Check at start; offer a newer release in a window.
    #[default]
    Ask,
    /// Check at start; download and put a newer release in place without a word.
    Always,
    /// No request unless the player asks for one (the settings' "Check now").
    Off,
}

impl Mode {
    /// The settings button's next choice.
    pub fn next(self) -> Mode {
        match self {
            Mode::Ask => Mode::Always,
            Mode::Always => Mode::Off,
            Mode::Off => Mode::Ask,
        }
    }
}

/// A version `X.Y.Z`, ordered as numbers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl Version {
    /// `X.Y.Z` or a tag `vX.Y.Z`.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let mut parts = s.strip_prefix('v').unwrap_or(s).split('.').map(|p| p.parse::<u32>().ok());
        let v = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(v)
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A release newer than this program, with what this platform downloads.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: Version,
    /// The release's page (its notes).
    pub page: String,
    /// This platform's program.
    pub program_url: String,
    /// `SHA256SUMS`.
    pub sums_url: String,
    /// What changed: the notes of every release newer than this program, newest first.
    pub notes: Vec<Note>,
}

/// A line of the release notes, read from their Markdown (the CHANGELOG section).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Note {
    /// A release: "Razdor X.Y.Z".
    Release(Version),
    /// "Added", "Changed", "Fixed" (`### …`).
    Section(String),
    /// A change (`- …`, with its continued lines).
    Item(String),
    /// Any other paragraph.
    Text(String),
}

/// Where the updates stand.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    /// Nothing asked yet.
    Idle,
    Checking,
    /// The newest release is this program's version (or older).
    UpToDate,
    /// No answer: no curl, no network, an odd reply. Shown only after "Check now".
    CheckFailed(String),
    /// A newer release is out.
    Found(Release),
    Downloading(Release),
    /// The release is in place of this program and runs from the next start.
    Installed(Release),
    InstallFailed(Release, String),
}

static STATUS: Mutex<Status> = Mutex::new(Status::Idle);
/// This program's file, taken at start (before a swap could rename it).
static PROGRAM: OnceLock<Option<PathBuf>> = OnceLock::new();
static RESTART: Mutex<bool> = Mutex::new(false);

pub fn status() -> Status {
    STATUS.lock().map(|s| s.clone()).unwrap_or(Status::Idle)
}

fn set(s: Status) {
    if let Ok(mut st) = STATUS.lock() {
        *st = s;
    }
}

/// Sets the status by hand (the snapshot scene of the update window).
pub fn pretend(s: Status) {
    set(s);
}

/// The program's file name in the releases on this platform; `None` where none is built.
pub fn asset_name() -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some("razdor-macos")
    } else if cfg!(all(windows, target_arch = "x86_64")) {
        Some("Razdor.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("razdor")
    } else {
        None
    }
}

/// The version this program counts as: `RAZDOR_UPDATE_AS`, else its own.
pub fn current() -> Version {
    std::env::var("RAZDOR_UPDATE_AS")
        .ok()
        .and_then(|v| Version::parse(&v))
        .or_else(|| Version::parse(env!("CARGO_PKG_VERSION")))
        .unwrap_or(Version(0, 0, 0))
}

/// At start: notes the program's file and clears what an update left behind (the old
/// Windows program, a download cut short).
pub fn init() {
    let program = PROGRAM.get_or_init(|| std::env::current_exe().ok()).clone();
    let (Some(program), Some(name)) = (program, asset_name()) else { return };
    for leftover in [old_path(&program), part_path(&program, name)] {
        if leftover.exists() {
            match std::fs::remove_file(&leftover) {
                Ok(()) => crate::diag!("update: removed {}", leftover.display()),
                Err(e) => crate::diag!("update: {}: {e}", leftover.display()),
            }
        }
    }
}

fn program() -> Option<PathBuf> {
    PROGRAM.get_or_init(|| std::env::current_exe().ok()).clone()
}

/// Updates apply to this program: a release build of this platform, not one of `cargo`'s
/// (unless `RAZDOR_UPDATE_AS` asks to try it).
pub fn enabled() -> bool {
    if asset_name().is_none() {
        return false;
    }
    if std::env::var_os("RAZDOR_UPDATE_AS").is_some() {
        return true;
    }
    program().is_some_and(|p| !p.components().any(|c| c.as_os_str() == "target"))
}

fn old_path(program: &Path) -> PathBuf {
    let mut name = program.file_name().unwrap_or_default().to_os_string();
    name.push(".old");
    program.with_file_name(name)
}

fn part_path(program: &Path, asset: &str) -> PathBuf {
    program.with_file_name(format!(".{asset}.update"))
}

/// `curl` with its window hidden on Windows (Razdor has no console there).
fn curl() -> Command {
    let mut c = Command::new("curl");
    c.args(["-fsSL", "--proto", "=https", "-A", concat!("razdor/", env!("CARGO_PKG_VERSION"))]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// The body of `url`, or why not.
fn fetch(url: &str, seconds: u32) -> Result<Vec<u8>, String> {
    let out = curl()
        .args(["--max-time", &seconds.to_string(), url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if why.is_empty() { format!("curl: {}", out.status) } else { why });
    }
    Ok(out.stdout)
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
}

/// A release's notes as lines: its CHANGELOG section, up to the pipeline's own part (the
/// first `## ` heading: downloads, SHA-256), without the Markdown marks.
pub fn parse_notes(body: &str) -> Vec<Note> {
    let plain = |s: &str| s.replace("**", "").replace('`', "").trim().to_string();
    let mut notes = Vec::new();
    // The paragraph being read: an item or plain text.
    let mut open: Option<Note> = None;
    let close = |open: &mut Option<Note>, notes: &mut Vec<Note>| notes.extend(open.take());
    for line in body.lines() {
        let t = line.trim();
        if t.starts_with("## ") {
            break;
        }
        if t.is_empty() {
            close(&mut open, &mut notes);
        } else if let Some(h) = t.strip_prefix("### ") {
            close(&mut open, &mut notes);
            notes.push(Note::Section(plain(h)));
        } else if let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) {
            close(&mut open, &mut notes);
            open = Some(Note::Item(plain(item)));
        } else if let Some(Note::Item(s) | Note::Text(s)) = open.as_mut() {
            s.push(' ');
            s.push_str(&plain(t));
        } else {
            open = Some(Note::Text(plain(t)));
        }
    }
    close(&mut open, &mut notes);
    notes
}

/// The API's list of releases: a [`Release`] for `asset` when the newest one (drafts and
/// pre-releases aside) is newer than `current`, `None` when it is not. Its notes are those
/// of every release newer than `current`.
pub fn parse_releases(json: &[u8], current: Version, asset: &str) -> Result<Option<Release>, String> {
    let list: Vec<ApiRelease> = serde_json::from_slice(json).map_err(|e| format!("reply: {e}"))?;
    let mut newer: Vec<(Version, ApiRelease)> = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease)
        .filter_map(|r| Some((Version::parse(&r.tag_name)?, r)))
        .filter(|(v, _)| *v > current)
        .collect();
    newer.sort_by(|a, b| b.0.cmp(&a.0));
    let Some((version, newest)) = newer.first() else { return Ok(None) };
    let url = |name: &str| newest.assets.iter().find(|a| a.name == name).map(|a| a.browser_download_url.clone());
    let program_url = url(asset).ok_or_else(|| format!("{} has no {asset}", newest.tag_name))?;
    let sums_url = url("SHA256SUMS").ok_or_else(|| format!("{} has no SHA256SUMS", newest.tag_name))?;
    let notes = newer
        .iter()
        .flat_map(|(v, r)| std::iter::once(Note::Release(*v)).chain(parse_notes(r.body.as_deref().unwrap_or_default())))
        .collect();
    Ok(Some(Release { version: *version, page: newest.html_url.clone(), program_url, sums_url, notes }))
}

/// Asks GitHub for the latest release on a background thread (nothing when a check or a
/// download is under way, or an update is already in place).
pub fn check() {
    let busy = matches!(status(), Status::Checking | Status::Downloading(_) | Status::Installed(_));
    let Some(asset) = asset_name().filter(|_| !busy) else { return };
    set(Status::Checking);
    let spawned = std::thread::Builder::new().name("update check".into()).spawn(move || {
        let found = fetch(RELEASES, 20).and_then(|json| parse_releases(&json, current(), asset));
        let next = match found {
            Ok(Some(r)) => {
                crate::diag!("update: {} is out (this is {})", r.version, current());
                Status::Found(r)
            }
            Ok(None) => {
                crate::diag!("update: up to date ({})", current());
                Status::UpToDate
            }
            Err(e) => {
                crate::diag!("update: no check: {e}");
                Status::CheckFailed(e)
            }
        };
        set(next);
    });
    if let Err(e) = spawned {
        set(Status::CheckFailed(e.to_string()));
    }
}

/// The SHA-256 `SHA256SUMS` (`sha256sum`'s lines) gives for file `name`.
pub fn sum_for(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hash, file) = l.trim().split_once(char::is_whitespace)?;
        (file.trim().trim_start_matches('*') == name).then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

/// Puts file `new` in place of `program`: a rename over it on Unix (made executable first);
/// on Windows the running program moves aside to `<name>.old` first, and back if the new
/// one cannot take its place.
pub fn swap(new: &Path, program: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(new, std::fs::Permissions::from_mode(0o755))?;
        std::fs::rename(new, program)
    }
    #[cfg(not(unix))]
    {
        let old = old_path(program);
        let _ = std::fs::remove_file(&old);
        std::fs::rename(program, &old)?;
        std::fs::rename(new, program).inspect_err(|_| {
            let _ = std::fs::rename(&old, program);
        })
    }
}

/// Downloads `r`, checks it and puts it in place of this program.
fn install_now(r: &Release) -> Result<(), String> {
    let asset = asset_name().ok_or("no release for this system")?;
    let program = program().ok_or("the program's file is unknown")?;
    let sums = fetch(&r.sums_url, 60)?;
    let want = sum_for(&String::from_utf8_lossy(&sums), asset).ok_or_else(|| format!("SHA256SUMS has no {asset}"))?;
    let part = part_path(&program, asset);
    let out = curl()
        .args(["--max-time", "900", "-o"])
        .arg(&part)
        .arg(&r.program_url)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let checked = if !out.status.success() {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    } else {
        let bytes = std::fs::read(&part).map_err(|e| format!("{}: {e}", part.display()))?;
        let got = sha256_hex(&bytes);
        if got == want {
            swap(&part, &program).map_err(|e| format!("{}: {e}", program.display()))
        } else {
            Err(format!("SHA-256 {got} is not the release's {want}"))
        }
    };
    if checked.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    checked
}

/// Downloads release `r` and puts it in place, on a background thread.
pub fn install(r: Release) {
    if matches!(status(), Status::Downloading(_) | Status::Installed(_)) {
        return;
    }
    set(Status::Downloading(r.clone()));
    let spawned = std::thread::Builder::new().name("update download".into()).spawn(move || {
        set(match install_now(&r) {
            Ok(()) => {
                crate::diag!("update: {} is in place; it runs from the next start", r.version);
                Status::Installed(r)
            }
            Err(e) => {
                crate::diag!("update: {} not installed: {e}", r.version);
                Status::InstallFailed(r, e)
            }
        });
    });
    if let Err(e) = spawned {
        crate::diag!("update: {e}");
    }
}

/// Asks for the program to start again when this one ends ([`restart`]).
pub fn request_restart() {
    if let Ok(mut r) = RESTART.lock() {
        *r = true;
    }
}

/// When asked for ([`request_restart`]): starts the program (the updated one, in the same
/// place) again with the same arguments. Called as the process ends.
pub fn restart() {
    if !RESTART.lock().is_ok_and(|r| *r) {
        return;
    }
    let Some(program) = program() else { return };
    match Command::new(&program).args(std::env::args_os().skip(1)).spawn() {
        Ok(_) => crate::diag!("update: started {} again", program.display()),
        Err(e) => crate::diag!("update: {}: {e}", program.display()),
    }
}

/// Opens `url` in the player's browser.
pub fn open_in_browser(url: &str) {
    let mut c = if cfg!(windows) {
        Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    if let Err(e) = c.arg(url).spawn() {
        crate::diag!("{url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_parse_and_order_as_numbers() {
        assert_eq!(Version::parse("v0.3.13"), Some(Version(0, 3, 13)));
        assert_eq!(Version::parse("1.2.3"), Some(Version(1, 2, 3)));
        assert_eq!(Version::parse("1.2"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(Version::parse("v1.x.3"), None);
        assert!(Version(0, 3, 13) > Version(0, 3, 9));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert_eq!(Version(0, 3, 13).to_string(), "0.3.13");
    }

    #[test]
    fn modes_cycle_and_are_saved_by_name() {
        assert_eq!(Mode::default(), Mode::Ask);
        assert_eq!(Mode::Ask.next().next().next(), Mode::Ask);
        assert_eq!(serde_json::to_string(&Mode::Always).unwrap(), "\"always\"");
        assert_eq!(serde_json::from_str::<Mode>("\"off\"").unwrap(), Mode::Off);
    }

    const REPLY: &str = r#"[
        {"tag_name":"v0.4.1","html_url":"https://x/v0.4.1","draft":true,"prerelease":false,"body":"draft","assets":[]},
        {"tag_name":"v0.4.0","html_url":"https://github.com/indicozy/razdor/releases/tag/v0.4.0","draft":false,"prerelease":false,
         "body":"\n### Added\n- **Updates:** Razdor finds\n  a newer release.\n\n### Fixed\n- A `crash`.\n\n## Downloads\n- **Razdor.exe**: Windows",
         "assets":[{"name":"razdor","browser_download_url":"https://x/razdor"},
                   {"name":"Razdor.exe","browser_download_url":"https://x/Razdor.exe"},
                   {"name":"SHA256SUMS","browser_download_url":"https://x/SHA256SUMS"}]},
        {"tag_name":"v0.3.14","html_url":"https://x/v0.3.14","draft":false,"prerelease":false,"body":"\n### Changed\n- Saves.","assets":[]},
        {"tag_name":"v0.3.13","html_url":"https://x/v0.3.13","draft":false,"prerelease":false,"body":"\n### Changed\n- Old.","assets":[]}
    ]"#;

    #[test]
    fn the_newest_release_gives_this_platforms_program_and_every_newer_ones_notes() {
        let r = parse_releases(REPLY.as_bytes(), Version(0, 3, 13), "Razdor.exe").unwrap().unwrap();
        assert_eq!(r.version, Version(0, 4, 0));
        assert_eq!(r.program_url, "https://x/Razdor.exe");
        assert_eq!(r.sums_url, "https://x/SHA256SUMS");
        assert!(r.page.ends_with("/v0.4.0"));
        assert_eq!(
            r.notes,
            vec![
                Note::Release(Version(0, 4, 0)),
                Note::Section("Added".into()),
                Note::Item("Updates: Razdor finds a newer release.".into()),
                Note::Section("Fixed".into()),
                Note::Item("A crash.".into()),
                Note::Release(Version(0, 3, 14)),
                Note::Section("Changed".into()),
                Note::Item("Saves.".into()),
            ]
        );
    }

    #[test]
    fn the_same_or_an_older_release_is_no_update() {
        assert_eq!(parse_releases(REPLY.as_bytes(), Version(0, 4, 0), "razdor").unwrap(), None);
        assert_eq!(parse_releases(REPLY.as_bytes(), Version(0, 5, 0), "razdor").unwrap(), None);
    }

    #[test]
    fn a_release_without_the_program_or_odd_reply_fails() {
        assert!(parse_releases(REPLY.as_bytes(), Version(0, 3, 0), "razdor-macos").is_err());
        assert!(parse_releases(b"{\"message\":\"API rate limit exceeded\"}", Version(0, 3, 0), "razdor").is_err());
    }

    #[test]
    fn sums_are_read_as_sha256sum_writes_them() {
        let sums = "0EE5  razdor\n033d Razdor.exe\n48fe *razdor-macos\n";
        assert_eq!(sum_for(sums, "razdor").as_deref(), Some("0ee5"));
        assert_eq!(sum_for(sums, "Razdor.exe").as_deref(), Some("033d"));
        assert_eq!(sum_for(sums, "razdor-macos").as_deref(), Some("48fe"));
        assert_eq!(sum_for(sums, "razdor.exe"), None);
    }

    #[test]
    fn sha256_of_known_text() {
        assert_eq!(sha256_hex(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn the_new_program_takes_the_old_ones_place() {
        let dir = std::env::temp_dir().join(format!("razdor-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let program = dir.join("razdor");
        let new = part_path(&program, "razdor");
        std::fs::write(&program, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        swap(&new, &program).unwrap();
        assert_eq!(std::fs::read(&program).unwrap(), b"new");
        assert!(!new.exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&program).unwrap().permissions().mode() & 0o777, 0o755);
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
