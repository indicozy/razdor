//! Updates from the GitHub releases: whether a newer Razdor is out, and putting it in place
//! of this program.
//!
//! Everything runs on a background thread and only changes [`status`]; the game never waits
//! on it. On desktop the requests go through the system's `curl` (Windows 10 and later, macOS
//! and almost every Linux have it): without one the check fails quietly. A download counts only
//! when its SHA-256 matches the release's `SHA256SUMS`.
//!
//! The new program replaces this one by a rename: on Unix over the running file (which keeps
//! running from its old inode), on Windows after moving the running `Razdor.exe` aside to
//! `Razdor.exe.old`, deleted at the next start. It runs from the next start on.
//!
//! Phones (`update::phone`): Android downloads its `razdor.apk`, checks it the same way and
//! hands it to the system installer, which the player confirms (`Status::Handed`). The APK is
//! kept in the app's cache folder as `razdor-<X.Y.Z>.apk` (written as `.part` until its SHA-256
//! matches): a download that was already made is reused, even after a cancelled install. At
//! start the APKs of this version and older, and any partial download, are removed; the APK
//! the installer took is removed once it reports success. iOS installs nothing: its window
//! opens the release's page (`opens_page`).
//!
//! A program under a `target` folder (a `cargo` build) checks nothing, unless
//! `RAZDOR_UPDATE_AS=<X.Y.Z>` is set: it then counts as that version, to try the whole flow
//! against the real releases.

#[cfg(any(target_os = "android", target_os = "ios"))]
mod phone;

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
    /// Android: the verified APK is with the system installer, which asks the player to
    /// confirm it on the screen (the install itself is the system's, not Razdor's).
    Handed(Release),
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

/// How the system installer ended a session that Razdor handed it (Android).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// The player cancelled on the installer's screen.
    Cancelled,
    Failed(String),
}

impl Outcome {
    /// The Java side's report (`razdorInstallOutcome`): "" while the session has not ended.
    pub fn parse(text: &str) -> Option<Outcome> {
        match text {
            "" => None,
            "installed" => Some(Outcome::Installed),
            "cancelled" => Some(Outcome::Cancelled),
            other => Some(Outcome::Failed(other.strip_prefix("failed: ").unwrap_or(other).to_string())),
        }
    }
}

/// The status after the installer's report. A cancelled session goes back to the offer, so
/// the player can press «Обновить» again (on a phone "Always" is "Ask", so the offer does not
/// come straight back); a failed one shows its reason; anything else stays as it is.
pub fn after_installer(status: Status, outcome: Outcome) -> Status {
    match (status, outcome) {
        (Status::Handed(r), Outcome::Cancelled) => Status::Found(r),
        (Status::Handed(r), Outcome::Failed(why)) => Status::InstallFailed(r, why),
        (Status::Handed(r), Outcome::Installed) => Status::Installed(r),
        (other, _) => other,
    }
}

/// Once per frame: while the system installer has an update of Razdor's, takes its report
/// (Android; nothing else hands an install to a system).
pub fn poll_installer() {
    let Status::Handed(r) = status() else { return };
    if let Some(outcome) = installer_report() {
        let next = after_installer(status(), outcome.clone());
        crate::diag!("update: the installer ended: {outcome:?}");
        if outcome == Outcome::Installed {
            forget_installed(&r);
        }
        set(next);
    }
}

/// Android: the installer took its copy of the APK, so the file goes (the new version is
/// running from the next start, and its cleanup would remove it anyway).
#[cfg(target_os = "android")]
fn forget_installed(r: &Release) {
    if let Ok(dir) = phone::cache_dir() {
        remove_apk(&dir, r.version);
    }
}

#[cfg(not(target_os = "android"))]
fn forget_installed(_: &Release) {}

#[cfg(target_os = "android")]
fn installer_report() -> Option<Outcome> {
    Outcome::parse(&phone::installer_outcome())
}

#[cfg(not(target_os = "android"))]
fn installer_report() -> Option<Outcome> {
    None
}

/// The program's file name in the releases for this system; `None` where none is built (the
/// Linux and Windows builds are x86_64 only). Android's APK is one file for every ABI, iOS's
/// IPA is installed by AltStore or Sideloadly (`opens_page`).
pub fn asset_name() -> Option<&'static str> {
    asset_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// [`asset_name`] for a system (`std::env::consts::OS` and `ARCH`).
fn asset_for(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", _) => Some("razdor-macos"),
        ("windows", "x86_64") => Some("Razdor.exe"),
        ("linux", "x86_64") => Some("razdor"),
        ("android", _) => Some("razdor.apk"),
        ("ios", _) => Some("razdor.ipa"),
        _ => None,
    }
}

/// iOS: an update opens the release's page in the browser (`ui::main_menu::open_link`), where
/// the player downloads the IPA for AltStore or Sideloadly. Razdor installs nothing there.
pub fn opens_page() -> bool {
    cfg!(target_os = "ios")
}

/// The mode that applies on this system: on a phone "Always" reads as "Ask", because the
/// system's installer always needs the player's tap (and iOS installs nothing at all).
pub fn effective(mode: Mode) -> Mode {
    effective_on(mode, crate::platform::MOBILE)
}

fn effective_on(mode: Mode, phone: bool) -> Mode {
    if phone && mode == Mode::Always {
        Mode::Ask
    } else {
        mode
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
/// Windows program, a download cut short). A phone's program is not a file Razdor replaces.
pub fn init() {
    if crate::platform::MOBILE {
        clear_phone_downloads();
        return;
    }
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

#[cfg(target_os = "android")]
fn clear_phone_downloads() {
    if let Ok(dir) = phone::cache_dir() {
        clear_downloads(&dir, current());
    }
}

#[cfg(not(target_os = "android"))]
fn clear_phone_downloads() {}

fn program() -> Option<PathBuf> {
    PROGRAM.get_or_init(|| std::env::current_exe().ok()).clone()
}

/// Updates apply to this program: a release build of this platform, not one of `cargo`'s
/// (unless `RAZDOR_UPDATE_AS` asks to try it).
pub fn enabled() -> bool {
    if asset_name().is_none() {
        return false;
    }
    if std::env::var_os("RAZDOR_UPDATE_AS").is_some() || crate::platform::MOBILE {
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
#[cfg(not(any(target_os = "android", target_os = "ios")))]
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
#[cfg(any(target_os = "android", target_os = "ios"))]
pub(crate) fn fetch(url: &str, seconds: u32) -> Result<Vec<u8>, String> {
    phone::get(url, seconds)
}

/// The body of `url`, or why not.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn fetch(url: &str, seconds: u32) -> Result<Vec<u8>, String> {
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
    let busy = matches!(status(), Status::Checking | Status::Downloading(_) | Status::Installed(_) | Status::Handed(_));
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

/// The SHA-256 of a file, read in pieces (a big download is not held in memory).
pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let mut file = std::fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(hash.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Downloads `url` into the file `path`, streamed, following redirects. On the desktop at
/// most `max_bytes` (curl stops there); the phones take what the system's downloader gives.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) fn download_to(url: &str, path: &Path, seconds: u32, max_bytes: u64) -> Result<(), String> {
    let out = curl()
        .args(["--max-time", &seconds.to_string(), "--max-filesize", &max_bytes.to_string(), "-o"])
        .arg(path)
        .arg(url)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    let why = String::from_utf8_lossy(&out.stderr).trim().to_string();
    Err(if why.is_empty() { format!("curl: {}", out.status) } else { why })
}

/// Downloads `url` into the file `path` through the system's downloader (Android).
#[cfg(target_os = "android")]
pub(crate) fn download_to(url: &str, path: &Path, seconds: u32, _max_bytes: u64) -> Result<(), String> {
    phone::download(url, path, seconds)
}

/// Downloads `url` into the file `path` (iOS: the body, then written).
#[cfg(target_os = "ios")]
pub(crate) fn download_to(url: &str, path: &Path, seconds: u32, _max_bytes: u64) -> Result<(), String> {
    let body = phone::get(url, seconds)?;
    std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
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

/// The SHA-256 that `sums` (the release's `SHA256SUMS`) gives for `asset`.
pub fn expected_sum(sums: &[u8], asset: &str) -> Result<String, String> {
    sum_for(&String::from_utf8_lossy(sums), asset).ok_or_else(|| format!("SHA256SUMS has no {asset}"))
}

/// Checks that `bytes` hash to `want` (see [`expected_sum`]).
pub fn check_sum(bytes: &[u8], want: &str) -> Result<(), String> {
    let got = sha256_hex(bytes);
    if got == want {
        Ok(())
    } else {
        Err(format!("SHA-256 {got} is not the release's {want}"))
    }
}

/// The name of release `v`'s APK in the phone's downloads folder: `razdor-X.Y.Z.apk`. Its
/// download is `<name>.part` until the SHA-256 matches.
pub fn apk_file_name(v: Version) -> String {
    format!("razdor-{v}.apk")
}

/// The version of a download's file name: `razdor-X.Y.Z.apk`, or that with `.part` after it.
pub fn apk_version(name: &str) -> Option<Version> {
    let base = name.strip_suffix(".part").unwrap_or(name);
    Version::parse(base.strip_prefix("razdor-")?.strip_suffix(".apk")?)
}

/// The old-style download name (before the APK kept its version): a cut-short one is removed.
const OLD_DOWNLOAD: &str = "razdor.apk.update";

/// Which of the downloads folder's file names the start removes, for the running `running`:
/// every partial download (`razdor-*.part`) and the old-style `razdor.apk.update`, and the APKs
/// of this version or older. A newer APK stays (it is not installed yet); other files stay.
pub fn stale_downloads(names: &[String], running: Version) -> Vec<String> {
    names
        .iter()
        .filter(|n| {
            if n.as_str() == OLD_DOWNLOAD {
                return true;
            }
            if !n.starts_with("razdor-") {
                return false;
            }
            n.ends_with(".part") || apk_version(n).is_some_and(|v| v <= running)
        })
        .cloned()
        .collect()
}

/// Removes the stale downloads in `dir` (see [`stale_downloads`]). Errors are logged only.
pub fn clear_downloads(dir: &Path, running: Version) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let names: Vec<String> = entries.flatten().filter_map(|e| e.file_name().into_string().ok()).collect();
    for name in stale_downloads(&names, running) {
        let path = dir.join(&name);
        match std::fs::remove_file(&path) {
            Ok(()) => crate::diag!("update: removed {}", path.display()),
            Err(e) => crate::diag!("update: {}: {e}", path.display()),
        }
    }
}

/// Removes release `v`'s APK from `dir`, if it is there.
pub fn remove_apk(dir: &Path, v: Version) {
    let path = dir.join(apk_file_name(v));
    if path.exists() {
        match std::fs::remove_file(&path) {
            Ok(()) => crate::diag!("update: removed {}", path.display()),
            Err(e) => crate::diag!("update: {}: {e}", path.display()),
        }
    }
}

/// Whether an APK already downloaded can be installed as it is: `existing` is the SHA-256 of
/// the file (`None` when there is no file), `want` the release's. A mismatch means a download.
pub fn reuse_download(existing: Option<&str>, want: &str) -> bool {
    existing.is_some_and(|sum| sum == want)
}

/// Downloads `r`, checks it and puts it in place of this program. True when the file went to
/// the system's installer instead (Android): the player confirms it there.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn install_now(r: &Release) -> Result<bool, String> {
    let asset = asset_name().ok_or("no release for this system")?;
    let program = program().ok_or("the program's file is unknown")?;
    let sums = fetch(&r.sums_url, 60)?;
    let want = expected_sum(&sums, asset)?;
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
        std::fs::read(&part)
            .map_err(|e| format!("{}: {e}", part.display()))
            .and_then(|bytes| check_sum(&bytes, &want))
            .and_then(|()| swap(&part, &program).map_err(|e| format!("{}: {e}", program.display())))
    };
    if checked.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    checked.map(|()| false)
}

/// Android: the APK of `r` is kept in the downloads folder as `razdor-<X.Y.Z>.apk`. A file
/// there whose SHA-256 is the release's is handed over as it is; otherwise it is downloaded to
/// `.part`, checked against `SHA256SUMS` and renamed into place. The system installer copies
/// it before it returns. True: handed over. The file stays after a cancelled session (the next
/// try reuses it) and is removed once the installer reports success (`forget_installed`).
#[cfg(target_os = "android")]
fn install_now(r: &Release) -> Result<bool, String> {
    let asset = asset_name().ok_or("no release for this system")?;
    let sums = fetch(&r.sums_url, 60)?;
    let want = expected_sum(&sums, asset)?;
    let dir = phone::cache_dir()?;
    let apk = dir.join(apk_file_name(r.version));
    let existing = std::fs::read(&apk).ok().map(|bytes| sha256_hex(&bytes));
    if reuse_download(existing.as_deref(), &want) {
        crate::diag!("update: {} was downloaded already", r.version);
    } else {
        let _ = std::fs::remove_file(&apk);
        let part = dir.join(format!("{}.part", apk_file_name(r.version)));
        let _ = std::fs::remove_file(&part);
        let fetched = phone::download(&r.program_url, &part, 900)
            .and_then(|()| std::fs::read(&part).map_err(|e| format!("{}: {e}", part.display())))
            .and_then(|bytes| check_sum(&bytes, &want))
            .and_then(|()| std::fs::rename(&part, &apk).map_err(|e| format!("{}: {e}", apk.display())));
        if let Err(e) = fetched {
            let _ = std::fs::remove_file(&part);
            return Err(e);
        }
    }
    phone::install(&apk).map(|()| true)
}

/// iOS installs nothing (its window opens the release's page): nothing to do here.
#[cfg(target_os = "ios")]
fn install_now(_: &Release) -> Result<bool, String> {
    Err("iOS installs by AltStore or Sideloadly, from the release's page".into())
}

/// Downloads release `r` and puts it in place, on a background thread. Not on iOS (which opens
/// the page instead, see [`opens_page`]).
pub fn install(r: Release) {
    if opens_page() || matches!(status(), Status::Downloading(_) | Status::Installed(_) | Status::Handed(_)) {
        return;
    }
    set(Status::Downloading(r.clone()));
    let spawned = std::thread::Builder::new().name("update download".into()).spawn(move || {
        set(match install_now(&r) {
            Ok(false) => {
                crate::diag!("update: {} is in place; it runs from the next start", r.version);
                Status::Installed(r)
            }
            Ok(true) => {
                crate::diag!("update: {} is with the system installer", r.version);
                Status::Handed(r)
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
    fn each_system_gets_its_own_release_file() {
        assert_eq!(asset_for("windows", "x86_64"), Some("Razdor.exe"));
        assert_eq!(asset_for("windows", "aarch64"), None);
        assert_eq!(asset_for("linux", "x86_64"), Some("razdor"));
        assert_eq!(asset_for("linux", "aarch64"), None);
        assert_eq!(asset_for("macos", "aarch64"), Some("razdor-macos"));
        assert_eq!(asset_for("android", "aarch64"), Some("razdor.apk"));
        assert_eq!(asset_for("android", "x86_64"), Some("razdor.apk"));
        assert_eq!(asset_for("ios", "aarch64"), Some("razdor.ipa"));
        assert_eq!(asset_for("freebsd", "x86_64"), None);
    }

    fn release_for_test() -> Release {
        Release {
            version: Version(1, 0, 2),
            page: String::new(),
            program_url: String::new(),
            sums_url: String::new(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn a_cancelled_installer_session_goes_back_to_the_offer() {
        let r = release_for_test();
        // Cancelled on the installer's screen: the offer is on again, «Обновить» works again.
        assert_eq!(after_installer(Status::Handed(r.clone()), Outcome::Cancelled), Status::Found(r.clone()));
        // A failed session shows its reason.
        assert_eq!(
            after_installer(Status::Handed(r.clone()), Outcome::Failed("no space".into())),
            Status::InstallFailed(r.clone(), "no space".into())
        );
        assert_eq!(after_installer(Status::Handed(r.clone()), Outcome::Installed), Status::Installed(r.clone()));
        // Only a handed install has a session to end: any other status stays as it is.
        assert_eq!(after_installer(Status::Idle, Outcome::Cancelled), Status::Idle);
        assert_eq!(after_installer(Status::Found(r.clone()), Outcome::Cancelled), Status::Found(r));
    }

    #[test]
    fn the_java_report_reads_as_an_outcome() {
        assert_eq!(Outcome::parse(""), None);
        assert_eq!(Outcome::parse("installed"), Some(Outcome::Installed));
        assert_eq!(Outcome::parse("cancelled"), Some(Outcome::Cancelled));
        assert_eq!(Outcome::parse("failed: no space"), Some(Outcome::Failed("no space".into())));
    }

    #[test]
    fn a_phone_release_gives_its_file_and_its_page() {
        let reply = r#"[{"tag_name":"v1.1.0","html_url":"https://x/v1.1.0","draft":false,"prerelease":false,
            "body":"\n### Added\n- Phones.\n\n## SHA-256\nsums",
            "assets":[{"name":"razdor.apk","browser_download_url":"https://x/razdor.apk"},
                      {"name":"razdor.ipa","browser_download_url":"https://x/razdor.ipa"},
                      {"name":"SHA256SUMS","browser_download_url":"https://x/SHA256SUMS"}]}]"#;
        let apk = parse_releases(reply.as_bytes(), Version(1, 0, 2), "razdor.apk").unwrap().unwrap();
        assert_eq!(apk.program_url, "https://x/razdor.apk");
        assert_eq!(apk.page, "https://x/v1.1.0");
        let ipa = parse_releases(reply.as_bytes(), Version(1, 0, 2), "razdor.ipa").unwrap().unwrap();
        assert_eq!(ipa.program_url, "https://x/razdor.ipa");
        assert_eq!(apk.notes, vec![Note::Release(Version(1, 1, 0)), Note::Section("Added".into()), Note::Item("Phones.".into())]);
        // A release without the phone's file is no update for that phone.
        assert!(parse_releases(reply.as_bytes(), Version(1, 0, 2), "razdor-macos").is_err());
    }

    #[test]
    fn always_reads_as_ask_on_a_phone_only() {
        assert_eq!(effective_on(Mode::Always, true), Mode::Ask);
        assert_eq!(effective_on(Mode::Always, false), Mode::Always);
        assert_eq!(effective_on(Mode::Ask, true), Mode::Ask);
        assert_eq!(effective_on(Mode::Off, true), Mode::Off);
        assert_eq!(effective_on(Mode::Off, false), Mode::Off);
    }

    #[test]
    fn the_phone_and_the_desktop_check_the_same_sums() {
        let sums = b"0e4b2c  razdor.apk\nbeef  razdor.ipa\n";
        let want = expected_sum(sums, "razdor.apk").unwrap();
        assert_eq!(want, "0e4b2c");
        assert!(expected_sum(sums, "razdor-macos").is_err());
        assert!(check_sum(b"abc", &sha256_hex(b"abc")).is_ok());
        let wrong = check_sum(b"abc", "0000").unwrap_err();
        assert!(wrong.starts_with("SHA-256 ba7816"), "{wrong}");
        // Through the release's own SHA256SUMS line.
        let line = format!("{}  razdor.apk\n", sha256_hex(b"apk bytes"));
        let want = expected_sum(line.as_bytes(), "razdor.apk").unwrap();
        assert!(check_sum(b"apk bytes", &want).is_ok());
        assert!(check_sum(b"apk bytes!", &want).is_err());
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

    #[test]
    fn an_apk_is_named_by_its_version_and_its_download_too() {
        assert_eq!(apk_file_name(Version(1, 0, 2)), "razdor-1.0.2.apk");
        assert_eq!(apk_version("razdor-1.0.2.apk"), Some(Version(1, 0, 2)));
        assert_eq!(apk_version("razdor-1.0.2.apk.part"), Some(Version(1, 0, 2)));
        assert_eq!(apk_version("razdor.apk.update"), None);
        assert_eq!(apk_version("razdor-fetch"), None);
        assert_eq!(apk_version("razdor-x.apk"), None);
        assert_eq!(apk_version("other-1.0.2.apk"), None);
    }

    #[test]
    fn the_start_removes_old_apks_and_partial_downloads_only() {
        let names: Vec<String> = [
            "razdor-1.0.1.apk",
            "razdor-1.0.2.apk",
            "razdor-1.0.3.apk",
            "razdor-1.0.3.apk.part",
            "razdor-1.0.4.apk.part",
            "razdor.apk.update",
            "razdor-fetch",
            "pictures.db",
            "other.part",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(
            stale_downloads(&names, Version(1, 0, 2)),
            vec!["razdor-1.0.1.apk", "razdor-1.0.2.apk", "razdor-1.0.3.apk.part", "razdor-1.0.4.apk.part", "razdor.apk.update"]
        );
        // A newer APK that is already verified stays for the next start's install.
        assert_eq!(stale_downloads(&["razdor-1.0.3.apk".to_string()], Version(1, 0, 2)), Vec::<String>::new());
    }

    #[test]
    fn a_download_is_reused_only_when_its_sum_is_the_releases() {
        let want = sha256_hex(b"apk bytes");
        assert!(reuse_download(Some(&want), &want));
        assert!(!reuse_download(Some(&sha256_hex(b"apk bytes!")), &want));
        assert!(!reuse_download(None, &want));
    }

    #[test]
    fn the_folder_is_cleared_of_stale_downloads_at_start() {
        let dir = std::env::temp_dir().join(format!("razdor-downloads-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["razdor-1.0.1.apk", "razdor-1.0.2.apk.part", "razdor-1.0.3.apk", "razdor.apk.update", "pictures.db"] {
            std::fs::write(dir.join(name), b"x").unwrap();
        }
        clear_downloads(&dir, Version(1, 0, 2));
        let mut left: Vec<String> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        left.sort();
        assert_eq!(left, vec!["pictures.db", "razdor-1.0.3.apk"]);
        // The installed APK goes by its version; another version's file stays.
        remove_apk(&dir, Version(1, 0, 3));
        remove_apk(&dir, Version(9, 9, 9));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
