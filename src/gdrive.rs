//! The community's game archive on GitHub («Скачать игру» on the no-install screen; the hub's
//! catalog offers the same kind of address to the automatic start). The owner's decision
//! (2026-11-07): the DT authors allowed the Discord server's moderators to share the game, and it
//! is published as one public .zip file in Razdor's community releases on GitHub. Razdor downloads
//! that file into the cache, checks its size and SHA-256, and hands it to the archive import
//! (`dt::game_import::import`, the same install check and swap as «Обновить игру из архива…»).
//! Razdor hosts none of it. Nothing here needs macroquad.
//!
//! A download can resume: the file is written to `<name>.part` and renamed once it verifies, so a
//! cancelled or interrupted download is continued by the next press (a Range request). A refusal
//! (403 / 429), any other failed status, or a web page instead of the file is a failure with a
//! reason; the screen then shows the manual path with the file's address.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Where an archive is downloaded from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Link {
    /// A direct https address: the community's GitHub release file, or the hub catalog's `game`
    /// (`hub::GameRef`).
    Url(Cow<'static, str>),
}

/// An archive to download: where it is, its size and SHA-256, and the name the player sees.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Archive {
    pub link: Link,
    pub size: u64,
    pub sha256: Cow<'static, str>,
    pub name: Cow<'static, str>,
}

/// The community's game on GitHub (the owner's constants; `scripts/check-game-link.sh` checks the link).
pub const GAME: Archive = Archive {
    link: Link::Url(Cow::Borrowed("https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip")),
    size: 44_619_982,
    sha256: Cow::Borrowed("dff554e22a0485f0ed06b3459d3e7e868224d49e264bba0ecf40db852514568d"),
    name: Cow::Borrowed("discord-times-cu.zip"),
};

impl Archive {
    /// The game the hub's catalog offers, at its address (the values as the catalog checked them).
    pub fn at_url(url: String, size: u64, sha256: String, name: String) -> Archive {
        Archive { link: Link::Url(Cow::Owned(url)), size, sha256: Cow::Owned(sha256), name: Cow::Owned(name) }
    }
}

/// The file's address in the browser (the manual path's link): the archive's own address.
pub fn file_page(archive: &Archive) -> String {
    let Link::Url(url) = &archive.link;
    url.to_string()
}

/// A download this small that is HTML is a web page, not the file.
const MAX_PAGE: u64 = 1 << 20;
const SECONDS: u32 = 3600;
/// The file is not the one Razdor expects (its size or SHA-256 differ).
const CHANGED: &str = "the archive on the server changed";

/// The network the download uses. A test gives a fake.
pub trait Net {
    /// Whether a partial file can be continued (a Range request). The phones' downloader cannot.
    fn resumes(&self) -> bool {
        true
    }
    /// `url` into `path` from byte `from` on (`from` 0 writes the file from its start). `progress`
    /// gets the file's size so far, and `cancelled` is asked as it goes (true: stop, with an error).
    /// Returns the HTTP status.
    fn get(&self, url: &str, path: &Path, from: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<u16, String>;
}

/// The real network: `update::download_watched`.
pub struct Http;

impl Net for Http {
    fn resumes(&self) -> bool {
        !cfg!(any(target_os = "android", target_os = "ios"))
    }

    fn get(&self, url: &str, path: &Path, from: u64, progress: &dyn Fn(u64), cancelled: &dyn Fn() -> bool) -> Result<u16, String> {
        crate::update::download_watched(url, path, from, SECONDS, progress, cancelled)
    }
}

/// What the download's thread reports to the window: the bytes so far, whether the file is in
/// (the import runs then), and a cancel.
#[derive(Default)]
pub struct Progress {
    bytes: AtomicU64,
    downloaded: AtomicBool,
    cancel: AtomicBool,
}

impl Progress {
    /// The bytes of the file so far.
    pub fn bytes(&self) -> u64 {
        self.bytes.load(Ordering::Relaxed)
    }

    /// The file is downloaded and verified: what follows is the unpacking.
    pub fn downloaded(&self) -> bool {
        self.downloaded.load(Ordering::Relaxed)
    }

    pub fn set_downloaded(&self) {
        self.downloaded.store(true, Ordering::Relaxed);
    }

    /// Asks the download to stop (it stops at once, the partial file kept for a retry).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
}

/// Why the download did not finish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    Cancelled,
    Failed(String),
}

/// The partial copy of `dest`: `<name>.part` beside it.
pub fn part_path(dest: &Path) -> PathBuf {
    let mut name = dest.as_os_str().to_owned();
    name.push(".part");
    PathBuf::from(name)
}

/// The file at `path` has the archive's size and SHA-256: the reason it does not, if not.
pub fn verify(archive: &Archive, path: &Path) -> Result<(), String> {
    let len = fs::metadata(path).map(|m| m.len()).map_err(|e| e.to_string())?;
    if len != archive.size {
        return Err(format!("the size is {len} bytes, not {}", archive.size));
    }
    let hash = crate::update::sha256_file(path).map_err(|e| e.to_string())?;
    if hash != archive.sha256 {
        return Err(format!("the SHA-256 is {hash}, not {}", archive.sha256));
    }
    Ok(())
}

/// Downloads `archive` into `dest`, verified. A `dest` that verifies already is kept (no download).
/// A partial copy is continued when the network can (a resume); if the result does not verify
/// after a resume, it is downloaded again from the start once. A result that does not verify is
/// refused ([`CHANGED`]) and removed. Progress and cancel as in [`Progress`].
pub fn download_game(net: &dyn Net, archive: &Archive, dest: &Path, progress: &Progress) -> Result<(), Failure> {
    progress.bytes.store(0, Ordering::Relaxed);
    if dest.is_file() && verify(archive, dest).is_ok() {
        progress.bytes.store(archive.size, Ordering::Relaxed);
        return Ok(());
    }
    let _ = fs::remove_file(dest);
    let part = part_path(dest);
    let mut from = if net.resumes() { fs::metadata(&part).map(|m| m.len()).unwrap_or(0) } else { 0 };
    if from == 0 {
        let _ = fs::remove_file(&part);
    }
    loop {
        let resumed = from > 0;
        fetch(net, archive, &part, from, progress)?;
        match verify(archive, &part) {
            Ok(()) => {
                fs::rename(&part, dest).map_err(|e| Failure::Failed(e.to_string()))?;
                progress.bytes.store(archive.size, Ordering::Relaxed);
                return Ok(());
            }
            Err(why) => {
                let _ = fs::remove_file(&part);
                crate::diag!("game download: {why}");
                if !resumed {
                    return Err(Failure::Failed(CHANGED.into()));
                }
                // The partial copy was not this file: the whole file again, once.
                from = 0;
            }
        }
    }
}

/// The file into `part`, from byte `from`: a refusal or a missing range restarts the file from its
/// start once. `part` holds the body on success.
fn fetch(net: &dyn Net, archive: &Archive, part: &Path, from: u64, progress: &Progress) -> Result<(), Failure> {
    let cancelled = || progress.cancelled();
    let url = file_page(archive);
    let mut from = from;
    let mut restarted = false;
    loop {
        if cancelled() {
            return Err(Failure::Cancelled);
        }
        let status = match net.get(&url, part, from, &|n| progress.bytes.store(n, Ordering::Relaxed), &cancelled) {
            Ok(status) => status,
            Err(why) => {
                if cancelled() {
                    return Err(Failure::Cancelled);
                }
                if from > 0 && !restarted {
                    let _ = fs::remove_file(part);
                    from = 0;
                    restarted = true;
                    continue;
                }
                return Err(Failure::Failed(why));
            }
        };
        match status {
            200 | 206 => {}
            416 if from > 0 && !restarted => {
                let _ = fs::remove_file(part);
                from = 0;
                restarted = true;
                continue;
            }
            403 | 429 => {
                let _ = fs::remove_file(part);
                return Err(Failure::Failed(format!("the server refused the file (HTTP {status})")));
            }
            other => {
                let _ = fs::remove_file(part);
                return Err(Failure::Failed(format!("the server answered HTTP {other}")));
            }
        }
        let len = fs::metadata(part).map(|m| m.len()).unwrap_or(0);
        if len <= MAX_PAGE {
            let body = fs::read(part).map_err(|e| Failure::Failed(e.to_string()))?;
            if is_html(&body) {
                let _ = fs::remove_file(part);
                return Err(Failure::Failed("the server did not give the file".into()));
            }
        }
        return Ok(());
    }
}

/// True when the body is HTML (its first character is `<`), not a file's bytes.
pub fn is_html(body: &[u8]) -> bool {
    body.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'<')
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// A small stand-in for the game archive: its bytes and their size and SHA-256.
    const BODY: &[u8] = b"PK\x03\x04 a small stand-in archive for the tests";
    const URL: &str = "https://example.org/releases/test.zip";

    fn archive() -> Archive {
        Archive {
            link: Link::Url(Cow::Borrowed(URL)),
            size: BODY.len() as u64,
            sha256: Cow::Owned(crate::update::sha256_hex(BODY)),
            name: Cow::Borrowed("test.zip"),
        }
    }

    /// A web page answered in place of the file (a proxy's or an error page).
    const PAGE: &str = "<html><body>Something else is served here.</body></html>";

    /// One scripted answer of the fake network.
    #[derive(Clone)]
    enum Reply {
        /// The status and the body; a `from` > 0 appends the body to the partial copy.
        Body(u16, Vec<u8>),
        Fail(&'static str),
    }

    /// A fake network: scripted answers in order, and a log of the requests (url, from).
    struct Fake {
        replies: RefCell<Vec<Reply>>,
        requests: RefCell<Vec<(String, u64)>>,
        resumes: bool,
    }

    impl Fake {
        fn new(replies: Vec<Reply>) -> Fake {
            Fake { replies: RefCell::new(replies.into_iter().rev().collect()), requests: RefCell::new(Vec::new()), resumes: true }
        }
    }

    impl Net for Fake {
        fn resumes(&self) -> bool {
            self.resumes
        }

        fn get(&self, url: &str, path: &Path, from: u64, progress: &dyn Fn(u64), _cancelled: &dyn Fn() -> bool) -> Result<u16, String> {
            self.requests.borrow_mut().push((url.to_string(), from));
            let reply = self.replies.borrow_mut().pop().ok_or("no scripted answer left")?;
            match reply {
                Reply::Fail(why) => Err(why.to_string()),
                Reply::Body(status, body) => {
                    if from > 0 {
                        use std::io::Write;
                        let mut f = fs::OpenOptions::new().append(true).create(true).open(path).map_err(|e| e.to_string())?;
                        f.write_all(&body).map_err(|e| e.to_string())?;
                    } else {
                        fs::write(path, &body).map_err(|e| e.to_string())?;
                    }
                    progress(fs::metadata(path).map(|m| m.len()).unwrap_or(0));
                    Ok(status)
                }
            }
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-gdrive-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_game_is_the_github_archive() {
        assert_eq!(file_page(&GAME), "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip");
        assert_eq!(GAME.size, 44_619_982);
        assert_eq!(GAME.name, "discord-times-cu.zip");
        assert_eq!(GAME.sha256, "dff554e22a0485f0ed06b3459d3e7e868224d49e264bba0ecf40db852514568d");
    }

    #[test]
    fn the_file_downloads_and_verifies_in_one_request() {
        let dir = scratch("plain");
        let dest = dir.join("game.zip");
        let net = Fake::new(vec![Reply::Body(200, BODY.to_vec())]);
        let progress = Progress::default();
        download_game(&net, &archive(), &dest, &progress).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), BODY);
        assert!(!part_path(&dest).exists());
        assert_eq!(progress.bytes(), BODY.len() as u64);
        assert_eq!(net.requests.borrow().as_slice(), &[(URL.to_string(), 0)]);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_verified_file_is_kept_and_not_downloaded_again() {
        let dir = scratch("kept");
        let dest = dir.join("game.zip");
        fs::write(&dest, BODY).unwrap();
        let net = Fake::new(vec![]);
        download_game(&net, &archive(), &dest, &Progress::default()).unwrap();
        assert!(net.requests.borrow().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_partial_copy_is_continued_with_a_range_request() {
        let dir = scratch("resume");
        let dest = dir.join("game.zip");
        let half = BODY.len() / 2;
        fs::write(part_path(&dest), &BODY[..half]).unwrap();
        let net = Fake::new(vec![Reply::Body(206, BODY[half..].to_vec())]);
        download_game(&net, &archive(), &dest, &Progress::default()).unwrap();
        assert_eq!(net.requests.borrow()[0].1, half as u64, "the request starts where the partial copy ends");
        assert_eq!(fs::read(&dest).unwrap(), BODY);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_partial_copy_that_is_not_the_file_is_downloaded_again_from_the_start() {
        let dir = scratch("stale");
        let dest = dir.join("game.zip");
        fs::write(part_path(&dest), b"XXXXXXXX").unwrap();
        let net = Fake::new(vec![Reply::Body(206, BODY[8..].to_vec()), Reply::Body(200, BODY.to_vec())]);
        download_game(&net, &archive(), &dest, &Progress::default()).unwrap();
        let requests = net.requests.borrow();
        assert_eq!(requests.iter().map(|r| r.1).collect::<Vec<_>>(), vec![8, 0]);
        assert_eq!(fs::read(&dest).unwrap(), BODY);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_file_of_another_size_or_hash_is_refused_and_nothing_is_kept() {
        let dir = scratch("changed");
        let dest = dir.join("game.zip");
        let other_size = Fake::new(vec![Reply::Body(200, b"a shorter file".to_vec())]);
        assert_eq!(download_game(&other_size, &archive(), &dest, &Progress::default()), Err(Failure::Failed(CHANGED.into())));
        assert!(!dest.exists() && !part_path(&dest).exists());

        let mut other_bytes = BODY.to_vec();
        other_bytes[0] = b'Q';
        let other_hash = Fake::new(vec![Reply::Body(200, other_bytes)]);
        assert_eq!(download_game(&other_hash, &archive(), &dest, &Progress::default()), Err(Failure::Failed(CHANGED.into())));
        assert!(!dest.exists() && !part_path(&dest).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_web_page_instead_of_the_file_is_an_error_and_nothing_is_kept() {
        let dir = scratch("page");
        let dest = dir.join("game.zip");
        let net = Fake::new(vec![Reply::Body(200, PAGE.as_bytes().to_vec())]);
        assert_eq!(download_game(&net, &archive(), &dest, &Progress::default()), Err(Failure::Failed("the server did not give the file".into())));
        assert!(!dest.exists() && !part_path(&dest).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_refusal_status_and_other_statuses_name_the_server_and_the_code() {
        let dir = scratch("status");
        let dest = dir.join("game.zip");
        let net = Fake::new(vec![Reply::Body(429, b"busy".to_vec())]);
        assert_eq!(download_game(&net, &archive(), &dest, &Progress::default()), Err(Failure::Failed("the server refused the file (HTTP 429)".into())));
        let net = Fake::new(vec![Reply::Body(500, b"oops".to_vec())]);
        assert_eq!(download_game(&net, &archive(), &dest, &Progress::default()), Err(Failure::Failed("the server answered HTTP 500".into())));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_network_error_is_a_failure_with_its_reason() {
        let dir = scratch("network");
        let dest = dir.join("game.zip");
        let net = Fake::new(vec![Reply::Fail("curl: timeout")]);
        assert_eq!(download_game(&net, &archive(), &dest, &Progress::default()), Err(Failure::Failed("curl: timeout".into())));
        assert!(!dest.exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_cancel_stops_at_once_and_keeps_the_partial_copy() {
        let dir = scratch("cancel");
        let dest = dir.join("game.zip");
        let progress = Progress::default();
        progress.cancel();
        let net = Fake::new(vec![]);
        assert_eq!(download_game(&net, &archive(), &dest, &progress), Err(Failure::Cancelled));
        assert!(net.requests.borrow().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn html_is_told_from_a_file() {
        assert!(is_html(b"  \n<!DOCTYPE html><html>"));
        assert!(!is_html(b"PK\x03\x04"));
        assert!(!is_html(b""));
    }

    /// The real download of the community's archive: a manual run, never part of the test runs
    /// (`#[ignore]`). It fetches the file into `RAZDOR_GDRIVE_E2E_DIR` (an empty folder), checks its
    /// size and SHA-256, and unpacks it into a fresh install folder with the archive import.
    /// `RAZDOR_GDRIVE_E2E_DIR=<empty dir> cargo test --release --lib manual_download -- --ignored --nocapture`
    #[test]
    #[ignore = "downloads the real game archive (43 MB): a manual run only"]
    fn manual_download_of_the_community_archive() {
        let Some(dir) = std::env::var_os("RAZDOR_GDRIVE_E2E_DIR") else { return };
        let dir = PathBuf::from(dir);
        let dest = dir.join("game.zip");
        let progress = Progress::default();
        let started = std::time::Instant::now();
        let result = download_game(&Http, &GAME, &dest, &progress);
        println!("e2e: {result:?}; bytes {}; seconds {:.1}", progress.bytes(), started.elapsed().as_secs_f64());
        assert!(result.is_ok(), "{result:?}");
        verify(&GAME, &dest).unwrap();
        let found = crate::dt::game_import::examine(&dest, &GAME.name).expect("the archive holds the game");
        let target = dir.join("DiscordTimes");
        let done = crate::dt::game_import::import(&found, &target, &|_, _| {});
        println!("e2e: import {done:?}");
        assert_eq!(done, Ok(crate::dt::game_import::Done::Replaced));
        assert!(target.join("_Global.ini").is_file() && target.join("DiscordTimes.exe").is_file());
        // The real install is a whole Community Update build: nothing missing, nothing changed.
        let manifest = crate::dt::integrity::builtin().unwrap();
        let report = crate::dt::integrity::check(&target, &manifest, true);
        println!("e2e: integrity {report:?}");
        assert!(report.community && report.missing.is_empty() && report.changed.is_empty(), "{report:?}");
    }

    #[test]
    fn a_direct_address_is_downloaded_from_itself_and_verified() {
        let dest = scratch("direct").join("game-download.zip");
        let url = "https://example.org/releases/game.zip";
        let archive = Archive::at_url(url.into(), BODY.len() as u64, crate::update::sha256_hex(BODY), "game.zip".into());
        let net = Fake::new(vec![Reply::Body(200, BODY.to_vec())]);
        download_game(&net, &archive, &dest, &Progress::default()).unwrap();
        assert_eq!(net.requests.borrow().as_slice(), &[(url.to_string(), 0)], "the first request is the address itself");
        assert_eq!(fs::read(&dest).unwrap(), BODY);
        assert_eq!(file_page(&archive), url);
        // A file of another hash is refused with the server's words, and nothing is kept.
        let other = Archive::at_url(url.into(), BODY.len() as u64, "0".repeat(64), "game.zip".into());
        let net = Fake::new(vec![Reply::Body(200, BODY.to_vec())]);
        let dest2 = scratch("direct-bad").join("game-download.zip");
        assert_eq!(download_game(&net, &other, &dest2, &Progress::default()), Err(Failure::Failed("the archive on the server changed".into())));
        assert!(!dest2.exists() && !part_path(&dest2).exists());
    }
}
