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
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use serde::{Deserialize, Serialize};

#[cfg(not(any(target_os = "android", target_os = "ios")))]
use crate::archive;
use crate::mirror;
use crate::parts;

/// How long a download may go without a byte (or a connection) before it stops: every download
/// here, the game's, the hub's and the updates', fails with [`STALLED`] instead of waiting out its
/// whole cap. Curl's `--connect-timeout` and `--speed-*` options and the phones' read timeout.
pub const STALL_SECONDS: u32 = 30;

/// The reason of a download that stalled. The technical reason goes to the log; the game's window
/// reads this one as a text of its own (`ui::game_import`).
pub const STALLED: &str = "stalled";

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
    /// The settings button's next choice. On a phone (`phone`) there is no "Always" (see
    /// [`effective_on`]): Ask and Off only, and a stored "Always" moves on as "Ask" does.
    pub fn next_on(self, phone: bool) -> Mode {
        match (self, phone) {
            (Mode::Ask, false) => Mode::Always,
            (Mode::Always, false) => Mode::Off,
            (Mode::Ask | Mode::Always, true) => Mode::Off,
            (Mode::Off, _) => Mode::Ask,
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
    /// This platform's program: its file, or an archive (`member` set) that holds it.
    pub program_url: String,
    /// The name of that file in the release: its versioned name when the release has one
    /// (see [`versioned_asset`]), else the old fixed name. For an archive, the archive's own name.
    pub asset: String,
    /// The program's name inside the archive (`program_url` is then a zip); `None` for a file.
    pub member: Option<String>,
    /// `SHA256SUMS` (or `SHA256SUMS.txt`).
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

/// The versioned name of a program's release file, for `legacy` (a name [`asset_for`] gives):
/// `razdor` at 1.4.1 is `razdor-1.4.1-linux`, `Razdor.exe` is `Razdor-1.4.1.exe`. `None` for
/// any other name. Releases carry both names: copies already installed run an updater that
/// only knows the old fixed names, so the old names stay in every release as well, and an
/// old release has no versioned names at all (`parse_releases` falls back to them).
pub fn versioned_asset(legacy: &str, v: Version) -> Option<String> {
    Some(match legacy {
        "razdor" => format!("razdor-{v}-linux"),
        "Razdor.exe" => format!("Razdor-{v}.exe"),
        "razdor-macos" => format!("razdor-{v}-macos"),
        "razdor.apk" => format!("razdor-{v}.apk"),
        "razdor.ipa" => format!("razdor-{v}.ipa"),
        _ => return None,
    })
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

/// [`effective`] for a system given as `phone`: the settings show and act on this one.
pub fn effective_on(mode: Mode, phone: bool) -> Mode {
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

/// `curl` with its window hidden on Windows (Razdor has no console there). It does not fail on an
/// HTTP error status by itself (see [`curl`]).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn curl_base() -> Command {
    let mut c = Command::new("curl");
    // The tests' range server is plain HTTP on 127.0.0.1; the app asks https only.
    let proto = if cfg!(test) { "=http,https" } else { "=https" };
    c.args(["-sSL", "--proto", proto, "-A", concat!("razdor/", env!("CARGO_PKG_VERSION"))]);
    c.args(stall_args());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

/// `curl` that fails on an HTTP error status (its body is then not written).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn curl() -> Command {
    let mut c = curl_base();
    c.arg("-f");
    c
}

/// The options that stop a stalled transfer: no connection within [`STALL_SECONDS`], and fewer
/// than one byte a second for [`STALL_SECONDS`]. Curl then exits with code 28 ([`curl_why`]).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn stall_args() -> Vec<String> {
    let s = STALL_SECONDS.to_string();
    ["--connect-timeout", &s, "--speed-limit", "1", "--speed-time", &s].iter().map(|a| a.to_string()).collect()
}

/// Why a `curl` run failed: a stall (exit code 28: a timeout or a transfer too slow) is
/// [`STALLED`], with curl's own words in the log; anything else is curl's text, else its status.
/// (The whole-transfer cap `--max-time` also exits 28: a download that long has stalled as well.)
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn curl_why(status: std::process::ExitStatus, stderr: &str) -> String {
    let why = stderr.trim();
    if status.code() == Some(28) {
        crate::diag!("download stalled: {why}");
        return STALLED.into();
    }
    if why.is_empty() {
        format!("curl: {status}")
    } else {
        why.to_string()
    }
}

/// A temp file name for one phone fetch (`phone::get`), unique in this process: `razdor-fetch-<pid>-<n>`
/// with `n` counting the calls. Razdor's own: on the phones the launch's catalog and the update check
/// fetch at the same time, and a shared file name let one read the other's body (the catalog failed
/// to parse, the check found no file, and the game was not downloaded by itself).
#[cfg_attr(not(any(target_os = "android", target_os = "ios")), allow(dead_code))]
pub(crate) fn fetch_temp_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!("razdor-fetch-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed))
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
        return Err(curl_why(out.status, &String::from_utf8_lossy(&out.stderr)));
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
    /// `"zip"` for an archive (GitVerse's mirror packs the programs that its file names refuse).
    #[serde(default)]
    archive: Option<String>,
    /// The file inside a zip archive that is the program.
    #[serde(default)]
    member: Option<String>,
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

/// The API's list of releases: a [`Release`] for `asset` (an old fixed name, see
/// [`asset_for`]) when the newest one (drafts and pre-releases aside) is newer than
/// `current`, `None` when it is not. The file is the newest release's versioned name for
/// `asset` when it has one, else `asset` itself (old releases). Its notes are those of every
/// release newer than `current`.
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
    let find = |name: &str| newest.assets.iter().find(|a| a.name == name);
    // Copies on old updaters know only the fixed name; new releases also carry the versioned one.
    // A zip (the GitVerse mirror's) is found by the program's fixed name, which it holds as `member`.
    let file = |name: &str| find(name).map(|a| (name.to_string(), a.browser_download_url.clone(), None));
    let zip = || {
        newest
            .assets
            .iter()
            .find(|a| a.archive.as_deref() == Some("zip") && a.member.as_deref() == Some(asset))
            .map(|a| (a.name.clone(), a.browser_download_url.clone(), Some(asset.to_string())))
    };
    let (name, program_url, member) = versioned_asset(asset, *version)
        .and_then(|n| file(&n))
        .or_else(|| file(asset))
        .or_else(zip)
        .ok_or_else(|| format!("{} has no {asset}", newest.tag_name))?;
    let sums_url = ["SHA256SUMS", "SHA256SUMS.txt"]
        .iter()
        .find_map(|n| find(n))
        .map(|a| a.browser_download_url.clone())
        .ok_or_else(|| format!("{} has no SHA256SUMS", newest.tag_name))?;
    let notes = newer
        .iter()
        .flat_map(|(v, r)| std::iter::once(Note::Release(*v)).chain(parse_notes(r.body.as_deref().unwrap_or_default())))
        .collect();
    Ok(Some(Release { version: *version, page: newest.html_url.clone(), program_url, asset: name, member, sums_url, notes }))
}

/// The update feed as `addresses` give it (see [`mirror::UPDATE_FEED`]): the first one that reads
/// as a release list answers (`parse_releases`); a reply that does not read is the next address's.
pub fn feed_from(addresses: &[&str], mut get: impl FnMut(&str) -> Result<Vec<u8>, String>, current: Version, asset: &str) -> Result<Option<Release>, String> {
    mirror::first_ok("update feed", addresses, |url| get(url).and_then(|json| parse_releases(&json, current, asset)))
}

/// Asks GitHub for the latest release on a background thread (nothing when a check or a
/// download is under way, or an update is already in place).
pub fn check() {
    let busy = matches!(status(), Status::Checking | Status::Downloading(_) | Status::Installed(_) | Status::Handed(_));
    let Some(asset) = asset_name().filter(|_| !busy) else { return };
    set(Status::Checking);
    let spawned = std::thread::Builder::new().name("update check".into()).spawn(move || {
        let feed = crate::remote_config::update_feed();
        let found = feed_from(&crate::remote_config::refs(&feed), |url| fetch(url, 20), current(), asset);
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

/// Downloads `url` into the file `path`, streamed, following redirects: in pieces when the server
/// answers ranges (`parts`), else as one stream of at most `max_bytes` (curl stops there on the
/// desktop; the phones take what the system's downloader gives).
pub(crate) fn download_to(url: &str, path: &Path, seconds: u32, max_bytes: u64) -> Result<(), String> {
    download_parted(url, path, seconds, max_bytes, &|_: u64| {}, &|| false, || download_one(url, path, seconds, max_bytes).map(|()| 200)).map(|_| ())
}

/// One stream into `path`, the whole file: `curl` on the desktop (a failed status is its error).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn download_one(url: &str, path: &Path, seconds: u32, max_bytes: u64) -> Result<(), String> {
    let out = curl()
        // curl's size limit is a signed 64-bit number.
        .args(["--max-time", &seconds.to_string(), "--max-filesize", &max_bytes.min(i64::MAX as u64).to_string(), "-o"])
        .arg(path)
        .arg(url)
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(curl_why(out.status, &String::from_utf8_lossy(&out.stderr)))
}

/// Android: the system's downloader (a refused status is `HTTP <code>`).
#[cfg(target_os = "android")]
fn download_one(url: &str, path: &Path, seconds: u32, _max_bytes: u64) -> Result<(), String> {
    phone::download(url, path, seconds)
}

/// iOS: the body (`NSData`), then written.
#[cfg(target_os = "ios")]
fn download_one(url: &str, path: &Path, seconds: u32, _max_bytes: u64) -> Result<(), String> {
    let body = phone::get(url, seconds)?;
    std::fs::write(path, body).map_err(|e| format!("{}: {e}", path.display()))
}

/// The pieces of `url` into `path` when the server answers ranges (see `parts`): `Ok(200)` when
/// they are joined, else `single` (one stream, with its own status). A failed piece is the error.
#[cfg(not(target_os = "ios"))]
fn download_parted(url: &str, path: &Path, seconds: u32, cap: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool, single: impl FnOnce() -> Result<u16, String>) -> Result<u16, String> {
    match parts::fetch(&Ranged { seconds }, url, path, cap, progress, cancelled)? {
        parts::Parted::Joined => Ok(200),
        parts::Parted::Single => single(),
    }
}

/// iOS takes one stream: its download is one `NSData` call, with no range to ask for.
#[cfg(target_os = "ios")]
fn download_parted(_: &str, _: &Path, _: u32, _: u64, _: &dyn Fn(u64), _: &dyn Fn() -> bool, single: impl FnOnce() -> Result<u16, String>) -> Result<u16, String> {
    single()
}

/// The desktop's and the Android's range requests for `parts` (iOS has none: see `download_parted`).
#[cfg(not(target_os = "ios"))]
struct Ranged {
    seconds: u32,
}

#[cfg(not(target_os = "ios"))]
impl parts::RangeNet for Ranged {
    fn range(&self, url: &str, path: &Path, start: u64, end: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<parts::RangeReply, String> {
        range_request(url, path, start, end, self.seconds, progress, cancelled)
    }
}

/// One range request (`parts::RangeNet::range`): `curl -r` on the desktop, watched. The status and
/// the file's size come from curl's write-out. A 200 is an answer, not an error: the server ignored
/// the range, and curl stops at `--max-filesize` (exit 63) before it takes the whole file.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn range_request(url: &str, path: &Path, start: u64, end: u64, seconds: u32, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<parts::RangeReply, String> {
    let mut cmd = curl_base();
    cmd.args(["--max-time", &seconds.to_string(), "--max-filesize", &(end - start + 1).to_string()])
        .args(["-r", &format!("{start}-{end}"), "-w", "%{http_code} %header{content-range}|%header{retry-after}", "-o"])
        .arg(path)
        .arg(url);
    let (status, out, err) = run_watched(cmd, path, progress, cancelled)?;
    let reply = parse_range_reply(&out);
    // Only a 206's body is the piece's bytes (a piece resumes after them, see `parts`): any other
    // reply leaves no file, so a 429's or a 200's body is never taken for the file's.
    if reply.is_none_or(|r| r.status != 206) {
        let _ = std::fs::remove_file(path);
    }
    match reply {
        Some(reply) if status.success() || (status.code() == Some(63) && reply.status == 200) => Ok(reply),
        _ => Err(curl_why(status, &err)),
    }
}

/// The status and the file's size from curl's write-out (`%{http_code} %header{content-range}`,
/// e.g. `206 bytes 0-0/44619982`; the size is None when the header is not there or is `*`), and the
/// `Retry-After` in seconds after the bar (None when it is not a number).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn parse_range_reply(out: &str) -> Option<parts::RangeReply> {
    // `<code> <content-range>|<retry-after>`: the bar keeps the two headers' words apart.
    let (head, retry) = out.trim().split_once('|').unwrap_or((out.trim(), ""));
    let mut words = head.splitn(2, ' ');
    let status = words.next()?.parse().ok()?;
    let total = words.next().and_then(|range| range.rsplit_once('/')).and_then(|(_, size)| size.trim().parse().ok());
    let retry_after = retry.trim().parse().ok();
    Some(parts::RangeReply { status, total, retry_after })
}

/// The phone's range request: the system's downloader, watched (`watched`).
#[cfg(target_os = "android")]
fn range_request(url: &str, path: &Path, start: u64, end: u64, seconds: u32, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<parts::RangeReply, String> {
    watched(path, progress, cancelled, || phone::download_range(url, path, start, end, seconds))
}

/// Downloads `url` into `path` for the game's download (`gdrive`), from byte `from` on (`from` > 0
/// continues a partial file with a Range request; the phones cannot, so they only take 0). From 0
/// the download is in pieces when the server answers ranges (`parts`), else one stream.
/// `progress` gets the file's size so far (the sum of the pieces), and `cancelled` is asked every
/// fraction of a second; true stops the transfer at once (an error, the file kept). Returns the HTTP
/// status (the body is written whatever it is: the caller checks it; a pieced file is 200).
pub(crate) fn download_watched(url: &str, path: &Path, from: u64, seconds: u32, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<u16, String> {
    if from > 0 {
        return stream_watched(url, path, from, seconds, progress, cancelled);
    }
    download_parted(url, path, seconds, u64::MAX, progress, cancelled, || stream_watched(url, path, 0, seconds, progress, cancelled))
}

/// The size of the file at `path` (0 when there is none yet).
fn file_size(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// Runs `cmd` (a curl) to its end, watched: `progress` gets the size of `path` every fraction of a
/// second, and `cancelled` kills the run (`Err("cancelled")`). Returns the exit status and the
/// stdout and stderr of the run.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn run_watched(mut cmd: Command, path: &Path, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<(std::process::ExitStatus, String, String), String> {
    use std::io::Read;
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            progress(file_size(path));
            let mut out = String::new();
            let mut err = String::new();
            if let Some(mut o) = child.stdout.take() {
                let _ = o.read_to_string(&mut out);
            }
            if let Some(mut e) = child.stderr.take() {
                let _ = e.read_to_string(&mut err);
            }
            return Ok((status, out, err));
        }
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("cancelled".into());
        }
        progress(file_size(path));
        std::thread::sleep(std::time::Duration::from_millis(150));
    }
}

/// One stream of `url` into `path` from byte `from` (desktop: `curl`, watched; a partial file is
/// continued with `-C`). Returns the HTTP status (see `download_watched`).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn stream_watched(url: &str, path: &Path, from: u64, seconds: u32, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<u16, String> {
    let mut cmd = curl_base();
    cmd.args(["--retry", "2", "--max-time", &seconds.to_string(), "-w", "%{http_code}", "-o"]).arg(path);
    if from > 0 {
        cmd.args(["-C", &from.to_string()]);
    }
    cmd.arg(url);
    let (status, out, err) = run_watched(cmd, path, progress, cancelled)?;
    if !status.success() {
        return Err(curl_why(status, &err));
    }
    Ok(out.trim().parse().unwrap_or(0))
}

/// One stream of `url` into `path` (phones and iOS): the system's downloader cannot continue a file,
/// so only from 0. A refused status comes back as its code (see `watched`).
#[cfg(any(target_os = "android", target_os = "ios"))]
fn stream_watched(url: &str, path: &Path, from: u64, seconds: u32, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<u16, String> {
    if from > 0 {
        return Err("the phone's downloader cannot continue a file".into());
    }
    match watched(path, progress, cancelled, || download_one(url, path, seconds, u64::MAX).map(|()| 200)) {
        Ok(code) => Ok(code),
        Err(e) => match e.strip_prefix("HTTP ").and_then(|code| code.trim().parse().ok()) {
            Some(code) => Ok(code),
            None => Err(e),
        },
    }
}

/// A phone's blocking download into `path`, watched: `work` runs on a helper thread while this one
/// polls the size of `path` as progress, and asks the transfer of `path` to stop
/// (`phone::download_cancel`) while `cancelled` is true; a cancelled transfer is `Err("cancelled")`.
/// Each piece and each hedge of a download has its own file, so a cancel stops that one transfer.
#[cfg(any(target_os = "android", target_os = "ios"))]
fn watched<R: Send>(path: &Path, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool, work: impl FnOnce() -> Result<R, String> + Send) -> Result<R, String> {
    if cancelled() {
        return Err("cancelled".into());
    }
    let result = std::thread::scope(|s| {
        let worker = s.spawn(work);
        while !worker.is_finished() {
            if cancelled() {
                phone::download_cancel(path);
            }
            progress(file_size(path));
            std::thread::sleep(std::time::Duration::from_millis(150));
        }
        worker.join().unwrap_or_else(|_| Err("the download thread failed".into()))
    });
    progress(file_size(path));
    match result {
        Err(_) if cancelled() => Err("cancelled".into()),
        other => other,
    }
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
    // The sum is the program's: for an archive, the file inside it (its member), else the file itself.
    let want = expected_sum(&sums, r.member.as_deref().unwrap_or(&r.asset))?;
    let part = part_path(&program, asset);
    let bytes = match &r.member {
        None => download_to(&r.program_url, &part, 900, archive::MAX_MEMBER).and_then(|()| std::fs::read(&part).map_err(|e| format!("{}: {e}", part.display()))),
        Some(member) => {
            // The archive is kept beside the program only while its member is taken out of it.
            let zip = program.with_file_name(format!(".{asset}.zip.update"));
            let got = download_to(&r.program_url, &zip, 900, archive::MAX_MEMBER).and_then(|()| archive::member(&zip, member));
            let _ = std::fs::remove_file(&zip);
            got
        }
    };
    // Unix: `swap` makes the new file executable (0755) before it takes the program's place.
    let checked = bytes.and_then(|bytes| check_sum(&bytes, &want).map(|()| bytes)).and_then(|bytes| {
        std::fs::write(&part, &bytes).map_err(|e| format!("{}: {e}", part.display()))?;
        swap(&part, &program).map_err(|e| format!("{}: {e}", program.display()))
    });
    if checked.is_err() {
        let _ = std::fs::remove_file(&part);
    }
    if checked.is_ok() {
        mirror::learn_from(&r.program_url, "the update came from it");
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
    asset_name().ok_or("no release for this system")?;
    if r.member.is_some() {
        // The phones' releases are plain files (the APK); an archive is not unpacked here.
        return Err(format!("{} is an archive: the phone takes the APK itself", r.asset));
    }
    let sums = fetch(&r.sums_url, 60)?;
    let want = expected_sum(&sums, &r.asset)?;
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
        mirror::learn_from(&r.program_url, "the update came from it");
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

/// Opens `url` in the player's browser (or a folder in the file manager). True when the opener started.
pub fn open_in_browser(url: &str) -> bool {
    let mut c = if cfg!(windows) {
        Command::new("explorer")
    } else if cfg!(target_os = "macos") {
        Command::new("open")
    } else {
        Command::new("xdg-open")
    };
    match c.arg(url).spawn() {
        Ok(_) => true,
        Err(e) => {
            crate::diag!("{url}: {e}");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn every_curl_stops_a_stalled_transfer() {
        let args: Vec<String> = curl().get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let s = STALL_SECONDS.to_string();
        for (flag, value) in [("--connect-timeout", s.as_str()), ("--speed-limit", "1"), ("--speed-time", s.as_str())] {
            let at = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} is missing: {args:?}"));
            assert_eq!(args[at + 1], value, "{flag}");
        }
    }

    #[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
    #[test]
    fn curl_exit_28_is_a_stall_and_other_failures_keep_curls_words() {
        use std::os::unix::process::ExitStatusExt;
        let stalled = std::process::ExitStatus::from_raw(28 << 8);
        assert_eq!(curl_why(stalled, "curl: (28) Operation too slow. Less than 1 bytes/sec\n"), STALLED);
        let refused = std::process::ExitStatus::from_raw(6 << 8);
        assert_eq!(curl_why(refused, "curl: (6) Could not resolve host\n"), "curl: (6) Could not resolve host");
        assert_eq!(curl_why(refused, " "), format!("curl: {refused}"));
    }

    #[test]
    fn each_phone_fetch_gets_its_own_temp_name() {
        let threads: Vec<_> = (0..8).map(|_| std::thread::spawn(|| (0..50).map(|_| fetch_temp_name()).collect::<Vec<_>>())).collect();
        let names: std::collections::HashSet<String> = threads.into_iter().flat_map(|t| t.join().unwrap()).collect();
        assert_eq!(names.len(), 400, "no two fetches, on any thread, share a file");
        let one = fetch_temp_name();
        assert!(one.starts_with(&format!("razdor-fetch-{}-", std::process::id())), "{one}");
        assert_ne!(one, fetch_temp_name());
    }

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
        assert_eq!(Mode::Ask.next_on(false), Mode::Always);
        assert_eq!(Mode::Always.next_on(false), Mode::Off);
        assert_eq!(Mode::Off.next_on(false), Mode::Ask);
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

    const BOTH_NAMES: &str = r#"[
        {"tag_name":"v1.4.1","html_url":"https://x/v1.4.1","draft":false,"prerelease":false,"body":"\n### Fixed\n- A.",
         "assets":[{"name":"Razdor.exe","browser_download_url":"https://x/Razdor.exe"},
                   {"name":"Razdor-1.4.1.exe","browser_download_url":"https://x/Razdor-1.4.1.exe"},
                   {"name":"Razdor-1.4.0.exe","browser_download_url":"https://x/Razdor-1.4.0.exe"},
                   {"name":"SHA256SUMS","browser_download_url":"https://x/SHA256SUMS"}]}
    ]"#;

    #[test]
    fn a_release_with_both_names_gives_the_versioned_one_first() {
        let r = parse_releases(BOTH_NAMES.as_bytes(), Version(1, 4, 0), "Razdor.exe").unwrap().unwrap();
        assert_eq!(r.program_url, "https://x/Razdor-1.4.1.exe");
        assert_eq!(r.asset, "Razdor-1.4.1.exe");
    }

    #[test]
    fn an_old_release_with_only_the_old_name_still_works() {
        let r = parse_releases(REPLY.as_bytes(), Version(0, 3, 13), "Razdor.exe").unwrap().unwrap();
        assert_eq!(r.program_url, "https://x/Razdor.exe");
        assert_eq!(r.asset, "Razdor.exe");
    }

    #[test]
    fn a_versioned_name_of_another_version_is_not_picked() {
        // The newest release has a file named for 1.4.0 only: the old name is the one picked.
        let other = BOTH_NAMES.replace("Razdor-1.4.1.exe", "Razdor-1.4.0.exe");
        let r = parse_releases(other.as_bytes(), Version(1, 4, 0), "Razdor.exe").unwrap().unwrap();
        assert_eq!(r.asset, "Razdor.exe");
        // Without the old name either, it is no program for this release.
        let only_other = other.replace(r#"{"name":"Razdor.exe","browser_download_url":"https://x/Razdor.exe"},"#, "");
        assert!(parse_releases(only_other.as_bytes(), Version(1, 4, 0), "Razdor.exe").is_err());
    }

    #[test]
    fn sums_find_the_versioned_line() {
        let sums = "033d  Razdor.exe\n9a1c  Razdor-1.4.1.exe\n";
        assert_eq!(sum_for(sums, "Razdor-1.4.1.exe").as_deref(), Some("9a1c"));
        assert_eq!(sum_for(sums, "Razdor.exe").as_deref(), Some("033d"));
    }

    #[test]
    fn versioned_names_for_the_five_programs() {
        let v = Version(1, 4, 1);
        assert_eq!(versioned_asset("razdor", v).as_deref(), Some("razdor-1.4.1-linux"));
        assert_eq!(versioned_asset("Razdor.exe", v).as_deref(), Some("Razdor-1.4.1.exe"));
        assert_eq!(versioned_asset("razdor-macos", v).as_deref(), Some("razdor-1.4.1-macos"));
        assert_eq!(versioned_asset("razdor.apk", v).as_deref(), Some("razdor-1.4.1.apk"));
        assert_eq!(versioned_asset("razdor.ipa", v).as_deref(), Some("razdor-1.4.1.ipa"));
        assert_eq!(versioned_asset("razdor-translate", v), None);
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    #[test]
    fn a_range_reply_gives_its_status_and_the_files_size() {
        let got = parse_range_reply("206 bytes 0-0/44619982").unwrap();
        assert_eq!(got, parts::RangeReply { status: 206, total: Some(44619982), retry_after: None });
        // No Content-Range header (curl writes nothing after the code), or a size of "*".
        assert_eq!(parse_range_reply("200 ").unwrap(), parts::RangeReply { status: 200, total: None, retry_after: None });
        assert_eq!(parse_range_reply("206 bytes 5-9/*").unwrap(), parts::RangeReply { status: 206, total: None, retry_after: None });
        // A 429 with its Retry-After (seconds); a date, which is not a number, is no wait.
        assert_eq!(parse_range_reply("429 |120").unwrap(), parts::RangeReply { status: 429, total: None, retry_after: Some(120) });
        assert_eq!(parse_range_reply("503 |Wed, 21 Oct 2026 07:28:00 GMT").unwrap().retry_after, None);
        // curl failed before any answer: its code is 000, which is no status a range can have.
        assert_eq!(parse_range_reply("000 ").unwrap().status, 0);
        assert_eq!(parse_range_reply(""), None);
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
            asset: String::new(),
            member: None,
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
    fn phone_cycle_is_ask_and_off() {
        // On a phone the row cycles Ask <-> Off; a stored Always reads as Ask, so it moves to Off.
        assert_eq!(Mode::Ask.next_on(true), Mode::Off);
        assert_eq!(Mode::Off.next_on(true), Mode::Ask);
        assert_eq!(Mode::Always.next_on(true), Mode::Ask.next_on(true));
        for m in [Mode::Ask, Mode::Always, Mode::Off] {
            assert_ne!(m.next_on(true), Mode::Always);
            assert_eq!(effective_on(m.next_on(true), true), m.next_on(true));
        }
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

    const ZIP_FEED: &str = r#"[
        {"tag_name":"v1.5.2","html_url":"https://x/v1.5.2","draft":false,"prerelease":false,"body":"\n### Added\n- A mirror.",
         "assets":[{"name":"Razdor-1.5.2-windows.zip","browser_download_url":"https://g/win.zip","archive":"zip","member":"Razdor.exe"},
                   {"name":"razdor-1.5.2-linux.zip","browser_download_url":"https://g/linux.zip","archive":"zip","member":"razdor"},
                   {"name":"SHA256SUMS.txt","browser_download_url":"https://g/SHA256SUMS.txt"}]}
    ]"#;

    #[test]
    fn an_archive_asset_gives_its_member_and_the_txt_sums() {
        let r = parse_releases(ZIP_FEED.as_bytes(), Version(1, 5, 1), "Razdor.exe").unwrap().unwrap();
        assert_eq!(r.program_url, "https://g/win.zip");
        assert_eq!(r.asset, "Razdor-1.5.2-windows.zip");
        assert_eq!(r.member.as_deref(), Some("Razdor.exe"));
        assert_eq!(r.sums_url, "https://g/SHA256SUMS.txt");
        let linux = parse_releases(ZIP_FEED.as_bytes(), Version(1, 5, 1), "razdor").unwrap().unwrap();
        assert_eq!(linux.program_url, "https://g/linux.zip");
        assert_eq!(linux.member.as_deref(), Some("razdor"));
        // A plain release keeps no member, and its sums may be named SHA256SUMS.txt.
        let plain = parse_releases(REPLY.replace("SHA256SUMS", "SHA256SUMS.txt").as_bytes(), Version(0, 3, 13), "Razdor.exe").unwrap().unwrap();
        assert_eq!(plain.member, None);
        assert_eq!(plain.sums_url, "https://x/SHA256SUMS.txt");
    }

    #[test]
    fn a_zip_member_is_taken_out_and_its_own_sum_checked() {
        let dir = std::env::temp_dir().join(format!("razdor-zip-member-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let zip_path = dir.join("pack.zip");
        let program = b"#!/bin/sh\necho razdor\n";
        {
            let file = std::fs::File::create(&zip_path).unwrap();
            let mut w = zip::ZipWriter::new(file);
            let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
            w.start_file("razdor", opts).unwrap();
            std::io::Write::write_all(&mut w, program).unwrap();
            w.start_file("razdor-translate", opts).unwrap();
            std::io::Write::write_all(&mut w, b"the translator").unwrap();
            w.finish().unwrap();
        }
        let bytes = archive::member(&zip_path, "razdor").unwrap();
        assert_eq!(bytes, program);
        // The sums name the member, as the release's SHA256SUMS.txt lists the programs inside.
        let sums = format!("{}  razdor\n{}  razdor-translate\n", sha256_hex(program), sha256_hex(b"the translator"));
        let want = expected_sum(sums.as_bytes(), "razdor").unwrap();
        assert!(check_sum(&bytes, &want).is_ok());
        assert!(check_sum(b"tampered", &want).is_err());
        assert!(archive::member(&zip_path, "missing").is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_html_page_is_no_feed_and_the_next_address_answers() {
        let html = b"<html><body>Page not found</body></html>".to_vec();
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let got = feed_from(
            &["https://gitverse.example/releases.json", "https://api.example/releases"],
            |url| {
                asked.borrow_mut().push(url.to_string());
                Ok(if url.starts_with("https://gitverse") { html.clone() } else { REPLY.as_bytes().to_vec() })
            },
            Version(0, 3, 13),
            "Razdor.exe",
        );
        assert_eq!(*asked.borrow(), ["https://gitverse.example/releases.json", "https://api.example/releases"]);
        assert_eq!(got.unwrap().unwrap().version, Version(0, 4, 0));
    }

    #[test]
    fn a_feed_that_is_unreachable_or_does_not_check_out_moves_on_and_its_reason_is_the_error() {
        let asked = std::cell::RefCell::new(0usize);
        let got = feed_from(
            &["a", "b", "c"],
            |url| {
                *asked.borrow_mut() += 1;
                match url {
                    "a" => Err("stalled".into()),
                    // A list without this platform's program does not check out.
                    _ => Ok(REPLY.as_bytes().to_vec()),
                }
            },
            Version(0, 3, 0),
            "razdor-macos",
        );
        assert_eq!(*asked.borrow(), 3, "a stall, then a list without the program, then the last address");
        assert!(got.unwrap_err().contains("has no razdor-macos"), "the last reason is the error");
    }

    #[test]
    fn the_first_feed_that_reads_answers_even_when_nothing_is_newer() {
        let asked = std::cell::RefCell::new(Vec::<String>::new());
        let got = feed_from(
            &["gitverse", "github"],
            |url| {
                asked.borrow_mut().push(url.to_string());
                Ok(REPLY.as_bytes().to_vec())
            },
            Version(0, 4, 0),
            "Razdor.exe",
        );
        assert_eq!(got, Ok(None));
        assert_eq!(*asked.borrow(), ["gitverse"]);
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
