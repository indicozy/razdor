//! The built-in addresses Razdor reads and downloads from, each list in the order it is tried:
//! GitHub first (the owner's decision, 2026-11-22), GitVerse as the fallback mirror for players in
//! Russia. They are the defaults of the
//! remote config (`remote_config::Config::builtin`): the lists in use are the config's, which the
//! app takes from `config.json` (see `remote_config`). See `docs/specs/2026-11-13-gitverse-mirrors.md`.
//!
//! Learned primary (the owner's decision, 2026-11-13): the host that works for a player becomes that
//! player's primary. Every list is ordered with the primary host's addresses first ([`ordered_by`]);
//! the config's order is the tie-break within a host and the order when there is no primary yet. A
//! primary moves only when a request's own fallback chain shows it: the primary host failed, then
//! another host answered ([`Chain`]). The primary is saved in the cache folder (`mirror-pref.json`,
//! [`learn`]); a file that does not read is no primary. `RAZDOR_HUB` is one explicit source and is
//! never reordered.
//!
//! Throttling (429, 503): a throttle never moves the learned primary (`is_throttle`, `Chain::failed`). It
//! pauses that host for its requests (`throttled`, `wait_host`), and a paused host's addresses go last in
//! their list. Nothing here needs macroquad.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde::Deserialize;
use web_time::Instant;

/// The built-in update feed: the release list in the GitHub API's shape. GitHub's Pages copy is a
/// static file of the community's Pages, tried first; the API is the next fallback (it allows 60
/// anonymous requests an hour per IP); GitVerse's copy of the same file comes last. A copy that is not
/// there (404) gives way to the next.
pub const UPDATE_FEED: [&str; 3] = [
    "https://indicozy.github.io/razdor-community/releases.json",
    "https://api.github.com/repos/indicozy/razdor/releases?per_page=30",
    "https://indicozy.gitverse.site/razdor-community/releases.json",
];

/// The built-in hub folders: the catalog's `index.json` names the files, resolved against the folder
/// that gave the catalog. GitHub's is `hub::DEFAULT_SOURCE`, then GitVerse's Pages.
pub const HUB: [&str; 2] = [crate::hub::DEFAULT_SOURCE, "https://indicozy.gitverse.site/razdor-community"];

/// The built-in game archive (`gdrive::game` gives the one in use): GitHub's release file, then
/// GitVerse's attachment.
pub const GAME: [&str; 2] = [
    "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip",
    "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5",
];

/// The file the learned primary is saved in, in the cache folder (`platform::cache_dir`).
const PREF_FILE: &str = "mirror-pref.json";

/// The order a saved primary was learned under: 2 is the GitHub-first order (2026-11-22). A saved
/// file without it (learned under the GitVerse-first order) is no primary, so every player starts on
/// GitHub once, and a GitVerse primary is learned again only when GitHub fails.
const PREF_ORDER: u64 = 2;

/// The two hosts Razdor downloads from. GitHub is `github.com`, `api.github.com`, `*.github.io` and the
/// release file hosts; GitVerse is `gitverse.ru` and `*.gitverse.site` (the Pages), the fallback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Host {
    GitVerse,
    GitHub,
}

impl Host {
    /// The word the saved file and the log use.
    pub fn name(self) -> &'static str {
        match self {
            Host::GitVerse => "gitverse",
            Host::GitHub => "github",
        }
    }

    /// The host of an `https://` address, or `None` for a host that is neither (a folder, a test server).
    pub fn of(url: &str) -> Option<Host> {
        let rest = url.strip_prefix("https://")?;
        let host = rest.split(['/', '?', '#']).next()?.rsplit('@').next()?.split(':').next()?.to_ascii_lowercase();
        let gitverse = host == "gitverse.ru" || host.ends_with(".gitverse.ru") || host.ends_with(".gitverse.site");
        let github = host == "github.com"
            || host == "api.github.com"
            || host.ends_with(".github.io")
            || host == "objects.githubusercontent.com"
            || host == "release-assets.githubusercontent.com";
        match (gitverse, github) {
            (true, _) => Some(Host::GitVerse),
            (_, true) => Some(Host::GitHub),
            _ => None,
        }
    }
}

/// The list in the order it is tried: the primary host's addresses first, then the rest, each part in
/// the list's own order. Without a primary, the list as it is.
pub fn ordered_by<S: AsRef<str>>(primary: Option<Host>, list: &[S]) -> Vec<&str> {
    let items: Vec<&str> = list.iter().map(|s| s.as_ref()).collect();
    let Some(primary) = primary else { return items };
    let (mut first, rest): (Vec<&str>, Vec<&str>) = items.into_iter().partition(|u| Host::of(u) == Some(primary));
    first.extend(rest);
    first
}

/// The addresses whose host is held (paused) go after the others, each part in its order: a throttled
/// host is tried when the other addresses have failed, and then it waits its pause (see [`wait_host`]).
fn held_last_by(list: Vec<&str>, held: impl Fn(&str) -> bool) -> Vec<&str> {
    let (live, held): (Vec<&str>, Vec<&str>) = list.into_iter().partition(|u| !held(u));
    live.into_iter().chain(held).collect()
}

/// One request's fallback chain: the addresses in order, and whether the primary host failed in it.
/// A primary moves only when this chain shows it (the primary failed, then another host answered).
#[derive(Debug)]
pub struct Chain {
    primary: Option<Host>,
    failed_on_primary: bool,
}

impl Chain {
    /// A chain for the given primary host.
    pub fn new(primary: Option<Host>) -> Chain {
        Chain { primary, failed_on_primary: false }
    }

    /// The list in this chain's order ([`ordered_by`] with the chain's primary, the addresses of a
    /// paused host last: see [`held_last_by`]).
    pub fn order<'a, S: AsRef<str>>(&self, list: &'a [S]) -> Vec<&'a str> {
        held_last_by(ordered_by(self.primary, list), paused)
    }

    /// An address of this chain failed, with its reason. A throttle (429 or 503) is no failure of the
    /// primary host: the primary is never moved away by it (the host is paused instead, see [`throttled`]).
    pub fn failed(&mut self, url: &str, why: &str) {
        if self.primary.is_some() && Host::of(url) == self.primary && !is_throttle(why) {
            self.failed_on_primary = true;
        }
    }

    /// An address of this chain answered. `Some(host)` when the answer moves the primary: the primary
    /// failed earlier in this chain and this address is on another known host.
    pub fn succeeded(&self, url: &str) -> Option<Host> {
        let to = Host::of(url)?;
        (self.failed_on_primary && Some(to) != self.primary).then_some(to)
    }
}

/// The primary host as the app has it: the saved one, read once; `None` when there is none.
pub fn current() -> Option<Host> {
    #[cfg(test)]
    {
        TEST_PRIMARY.with(std::cell::Cell::get)
    }
    #[cfg(not(test))]
    {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        *state.get_or_insert_with(|| pref_path().and_then(|p| fs::read(p).ok()).and_then(|b| parse_pref(&b)))
    }
}

/// A new chain for the current primary host.
pub fn chain() -> Chain {
    Chain::new(current())
}

/// The list ordered by the current primary host, as owned strings (for a `Vec<String>` list).
pub fn ordered_owned(list: &[String]) -> Vec<String> {
    chain().order(list).into_iter().map(String::from).collect()
}

/// Makes `to` the primary host and saves it. A change is logged with its reason; the same host is
/// kept as it is.
pub fn learn(to: Host, why: &str) {
    let from = current();
    if from == Some(to) {
        return;
    }
    crate::diag!("mirror: the primary host is {} (was {}): {why}", to.name(), from.map_or("none", Host::name));
    store(to);
}

/// Makes the host of `url` the primary (the updater's file came from it, the first start's config
/// came first from it). An address of no known host changes nothing.
pub fn learn_from(url: &str, why: &str) {
    if let Some(to) = Host::of(url) {
        learn(to, why);
    }
}

/// Tries the addresses in order (ordered by the primary host, [`chain`]): `attempt` reads what one of
/// them gives, or says why not. The first `Ok` is the answer, and a primary that failed in this
/// chain and is followed by an answer from another host moves ([`Chain::succeeded`]). Each failure
/// is logged with its address and reason; when none answers, the last reason is the error.
pub fn first_ok<T, S: AsRef<str>>(what: &str, addresses: &[S], mut attempt: impl FnMut(&str) -> Result<T, String>) -> Result<T, String> {
    let mut chain = chain();
    let mut last = String::from("no address");
    for address in chain.order(addresses) {
        match attempt(address) {
            Ok(value) => {
                crate::diag!("{what}: from {address}");
                if let Some(to) = chain.succeeded(address) {
                    learn(to, &format!("{what}: its primary failed, {address} answered"));
                }
                return Ok(value);
            }
            Err(why) => {
                crate::diag!("{what}: {address}: {why}");
                chain.failed(address, &why);
                last = why;
            }
        }
    }
    Err(last)
}

/// The status and the Retry-After (seconds, when the server gave a number) of the Java side's reply
/// `HTTP <code>` or `HTTP <code> retry-after <seconds>`; None for any other reply.
pub fn http_reply(text: &str) -> Option<(u16, Option<u64>)> {
    let rest = text.strip_prefix("HTTP ")?;
    let (code, retry_after) = match rest.split_once(" retry-after ") {
        Some((code, retry)) => (code, retry.trim().parse().ok()),
        None => (rest, None),
    };
    Some((code.trim().parse().ok()?, retry_after))
}

/// True when a failure's reason is a throttle: HTTP 429 (rate limit) or 503 (busy), as the app's
/// requests and curl word them.
pub fn is_throttle(why: &str) -> bool {
    ["HTTP 429", "HTTP 503", "error: 429", "error: 503"]
        .iter()
        .any(|s| why.match_indices(s).any(|(i, _)| !why[i + s.len()..].starts_with(|c: char| c.is_ascii_digit())))
}

/// The host part of an `https://` address, for the log (`indicozy.gitverse.site`); "" for none.
fn host_of(url: &str) -> &str {
    let rest = url.strip_prefix("https://").unwrap_or(url);
    rest.split(['/', '?', '#']).next().unwrap_or("").rsplit('@').next().unwrap_or("").split(':').next().unwrap_or("")
}

/// The pauses per host (one shared pause for every request to that host, none for another host).
/// `level` counts the throttles in a row: the next pause without a `Retry-After` is longer.
#[derive(Debug, Clone, Copy)]
struct Pause {
    until: Instant,
    level: u32,
}

/// The pauses of all hosts. Pure: the caller gives the clock and the random unit (0..1) for jitter.
#[derive(Debug, Default)]
pub struct Pauses {
    hosts: BTreeMap<&'static str, Pause>,
}

impl Pauses {
    /// A throttle from `host` at `now`: its requests wait a while. The wait is the server's
    /// `Retry-After` (capped at [`crate::parts::MAX_RETRY_AFTER`]) or else 2, 4, 8 ... 60 s by the
    /// level, jittered to 75-125 %. Returns the wait given.
    pub fn throttled(&mut self, host: Host, now: Instant, retry_after: Option<u64>, unit: f64) -> Duration {
        let level = self.hosts.get(host.name()).map_or(0, |p| p.level);
        let base = match retry_after {
            Some(s) => Duration::from_secs(s.min(crate::parts::MAX_RETRY_AFTER)),
            None => Duration::from_secs((2u64 << level.min(5)).min(crate::parts::MAX_RETRY_AFTER)),
        };
        let wait = base.mul_f64(0.75 + 0.5 * unit.clamp(0.0, 1.0));
        let entry = self.hosts.entry(host.name()).or_insert(Pause { until: now, level: 0 });
        entry.until = entry.until.max(now + wait);
        entry.level = level + 1;
        wait
    }

    /// How long `host`'s requests still wait at `now`.
    pub fn remaining(&self, host: Host, now: Instant) -> Duration {
        self.hosts.get(host.name()).map_or(Duration::ZERO, |p| p.until.saturating_duration_since(now))
    }

    /// `host` answered: its pause and its level are gone.
    pub fn answered(&mut self, host: Host) {
        self.hosts.remove(host.name());
    }
}

/// The app's pauses (see [`Pauses`]).
static PAUSES: Mutex<Pauses> = Mutex::new(Pauses { hosts: BTreeMap::new() });

fn pauses() -> std::sync::MutexGuard<'static, Pauses> {
    PAUSES.lock().unwrap_or_else(|e| e.into_inner())
}

/// A random unit (0..1) for the jitter: a xorshift stream seeded from the clock.
fn unit_random() -> f64 {
    static SEED: AtomicU64 = AtomicU64::new(0);
    let mut x = SEED.load(Ordering::Relaxed);
    if x == 0 {
        x = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    SEED.store(x, Ordering::Relaxed);
    (x >> 11) as f64 / (1u64 << 53) as f64
}

/// The address's host is throttled (429 or 503): its requests wait a while ([`Pauses::throttled`]).
/// Other hosts are not held back. An address of no known host changes nothing.
pub fn throttled(url: &str, retry_after: Option<u64>) {
    let Some(host) = Host::of(url) else { return };
    let wait = pauses().throttled(host, Instant::now(), retry_after, unit_random());
    crate::diag!("http: {}: throttled, its requests wait {:.1} s", host_of(url), wait.as_secs_f64());
}

/// The address's host answered: its level of throttles starts again.
pub fn answered(url: &str) {
    if let Some(host) = Host::of(url) {
        pauses().answered(host);
    }
}

/// True while the address's host is paused.
pub fn paused(url: &str) -> bool {
    Host::of(url).is_some_and(|host| !pauses().remaining(host, Instant::now()).is_zero())
}

/// Waits until the address's host is not paused. `stopped` ends the wait early (a cancel).
pub fn wait_host(url: &str, stopped: &dyn Fn() -> bool) {
    let Some(host) = Host::of(url) else { return };
    loop {
        if stopped() {
            return;
        }
        let left = pauses().remaining(host, Instant::now());
        if left.is_zero() {
            return;
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// A failed HTTP request, logged with its host, reason and `Retry-After`. A throttle (429 or 503)
/// pauses that host ([`throttled`]).
pub fn note_failure(url: &str, why: &str, retry_after: Option<u64>) {
    let retry = retry_after.map_or_else(|| "none".to_string(), |s| s.to_string());
    crate::diag!("http: {}: {why} (retry-after {retry})", host_of(url));
    if is_throttle(why) {
        throttled(url, retry_after);
    }
}

/// Extra tries a request gets after a throttle (429 or 503), each one after the host's pause.
pub const THROTTLE_RETRIES: u32 = 2;

/// Runs `attempt`, and runs it again (up to [`THROTTLE_RETRIES`] times) while it fails with a
/// throttle. Any other failure, or the last one, is the result.
pub fn retrying<T>(what: &str, mut attempt: impl FnMut() -> Result<T, String>) -> Result<T, String> {
    let mut left = THROTTLE_RETRIES;
    loop {
        match attempt() {
            Err(why) if is_throttle(&why) && left > 0 => {
                left -= 1;
                crate::diag!("{what}: throttled, another try after the host's pause: {why}");
            }
            other => return other,
        }
    }
}

/// The primary host a saved file gives: `{"host": "gitverse" | "github", "order": 2, ...}`. Anything
/// else (a damaged file, another word, a file learned under an older order) is no primary.
pub fn parse_pref(bytes: &[u8]) -> Option<Host> {
    #[derive(Deserialize)]
    struct Saved {
        host: String,
        order: Option<u64>,
    }
    let saved: Saved = serde_json::from_slice(bytes).ok()?;
    if saved.order != Some(PREF_ORDER) {
        return None;
    }
    match saved.host.as_str() {
        "gitverse" => Some(Host::GitVerse),
        "github" => Some(Host::GitHub),
        _ => None,
    }
}

#[cfg(not(test))]
/// Where the primary is saved: `mirror-pref.json` in the cache folder.
fn pref_path() -> Option<PathBuf> {
    crate::platform::cache_dir().map(|d| d.join(PREF_FILE))
}

/// Saves `to` at `path` (written whole under a temporary name, then renamed). A failure is logged.
fn write_pref(path: &Path, to: Host) {
    let since = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let json = serde_json::json!({ "host": to.name(), "since": since, "order": PREF_ORDER }).to_string();
    let tmp = path.with_extension("json.part");
    let written = path.parent().map_or(Ok(()), fs::create_dir_all).and_then(|_| fs::write(&tmp, json)).and_then(|_| fs::rename(&tmp, path));
    if let Err(e) = written {
        crate::diag!("mirror: the primary host is not saved: {e}");
    }
}

#[cfg(not(test))]
static STATE: Mutex<Option<Option<Host>>> = Mutex::new(None);

#[cfg(test)]
crate::local! {
    /// The tests' primary: per thread (each test has its own), never the file.
    static TEST_PRIMARY: std::cell::Cell<Option<Host>> = const { std::cell::Cell::new(None) };
}

/// Sets the primary in memory and saves it to the cache folder (the tests keep it per thread).
fn store(to: Host) {
    #[cfg(test)]
    TEST_PRIMARY.with(|c| c.set(Some(to)));
    #[cfg(not(test))]
    {
        if let Some(path) = pref_path() {
            write_pref(&path, to);
        }
        *STATE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Some(to));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const GV: &str = "https://indicozy.gitverse.site/razdor-community/releases.json";
    const GV2: &str = "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5";
    const GH: &str = "https://api.github.com/repos/indicozy/razdor/releases?per_page=30";
    const GH2: &str = "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip";
    const OTHER: &str = "https://other.example/x.json";

    #[test]
    fn the_first_answer_wins_and_later_addresses_are_not_asked() {
        let asked = RefCell::new(Vec::new());
        let got = first_ok("test", &["a", "b", "c"], |a| {
            asked.borrow_mut().push(a.to_string());
            if a == "a" { Err("connection refused".into()) } else { Ok(a.to_uppercase()) }
        });
        assert_eq!(got, Ok("B".to_string()));
        assert_eq!(asked.borrow().as_slice(), &["a", "b"], "c is never asked");
    }

    #[test]
    fn every_fallback_reason_moves_on_and_the_last_reason_is_the_error() {
        let reasons = ["curl: (6) could not resolve", "HTTP 404", "stalled", "reply: expected value at line 1", "size is wrong"];
        let addresses = ["a1", "a2", "a3", "a4", "a5"];
        let asked = RefCell::new(0usize);
        let got: Result<(), String> = first_ok("test", &addresses, |_| {
            let n = *asked.borrow();
            *asked.borrow_mut() += 1;
            Err(reasons[n].to_string())
        });
        assert_eq!(*asked.borrow(), 5, "every address is tried");
        assert_eq!(got, Err("size is wrong".to_string()));
    }

    #[test]
    fn the_addresses_are_github_first_and_gitverse_the_fallback() {
        assert_eq!(UPDATE_FEED[0], "https://indicozy.github.io/razdor-community/releases.json", "the Pages copy first");
        assert!(UPDATE_FEED[1].starts_with("https://api.github.com/"), "the API is the next fallback");
        assert!(UPDATE_FEED[2].starts_with("https://indicozy.gitverse.site/"), "GitVerse's copy is the last");
        assert!(HUB[0].starts_with("https://github.com/"));
        assert!(HUB[1].starts_with("https://indicozy.gitverse.site/"));
        assert_eq!(HUB[0], crate::hub::DEFAULT_SOURCE);
        assert!(GAME[0].starts_with("https://github.com/"));
        assert!(GAME[1].starts_with("https://gitverse.ru/api/attachments/"));
    }

    #[test]
    fn each_address_belongs_to_its_host_and_others_to_none() {
        assert_eq!(Host::of(GV), Some(Host::GitVerse));
        assert_eq!(Host::of(GV2), Some(Host::GitVerse));
        assert_eq!(Host::of("https://gitverse.ru/x"), Some(Host::GitVerse));
        assert_eq!(Host::of(GH), Some(Host::GitHub));
        assert_eq!(Host::of(GH2), Some(Host::GitHub));
        assert_eq!(Host::of("https://indicozy.github.io/razdor/config.json"), Some(Host::GitHub));
        assert_eq!(Host::of("https://objects.githubusercontent.com/x"), Some(Host::GitHub));
        assert_eq!(Host::of("https://release-assets.githubusercontent.com/x"), Some(Host::GitHub));
        assert_eq!(Host::of("https://INDICOZY.GITVERSE.SITE:443/x"), Some(Host::GitVerse), "the host is not case sensitive");
        assert_eq!(Host::of(OTHER), None);
        assert_eq!(Host::of("http://indicozy.github.io/x"), None, "https only");
        assert_eq!(Host::of("https://github.com.evil.example/x"), None);
    }

    #[test]
    fn ordered_puts_the_primary_hosts_addresses_first_and_keeps_each_part_in_its_order() {
        let list = [GV, GH, OTHER, GV2, GH2];
        assert_eq!(ordered_by(Some(Host::GitHub), &list), vec![GH, GH2, GV, OTHER, GV2]);
        assert_eq!(ordered_by(Some(Host::GitVerse), &list), vec![GV, GV2, GH, OTHER, GH2]);
        assert_eq!(ordered_by(None, &list), list.to_vec(), "no primary: the list as it is");
        let owned: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        assert_eq!(ordered_by(Some(Host::GitHub), &owned), vec![GH, GH2, GV, OTHER, GV2]);
    }

    #[test]
    fn a_chain_moves_only_when_the_primary_failed_and_another_host_answered() {
        let mut c = Chain::new(Some(Host::GitHub));
        assert_eq!(c.succeeded(GH), None, "a success on the primary keeps it");
        c.failed(GH, "stalled");
        assert_eq!(c.succeeded(GH2), None, "another address of the primary's host keeps it");
        c.failed(GV, "stalled");
        assert_eq!(c.succeeded(GV2), Some(Host::GitVerse), "the primary failed, then GitVerse answered");

        let mut other = Chain::new(Some(Host::GitHub));
        other.failed(GV, "stalled");
        assert_eq!(other.succeeded(GV2), None, "a failure on the other host alone does not move the primary");
        assert_eq!(other.succeeded(GH), None);

        let mut none = Chain::new(None);
        none.failed(GH, "stalled");
        assert_eq!(none.succeeded(GV), None, "with no primary nothing moves");
    }

    #[test]
    fn first_ok_moves_the_primary_when_it_fails_and_the_other_host_answers() {
        learn(Host::GitHub, "test: start");
        let asked = RefCell::new(Vec::new());
        let got = first_ok("test", &[GV, GH], |url| {
            asked.borrow_mut().push(url.to_string());
            if url == GH { Ok(7) } else { Err("stalled".into()) }
        });
        assert_eq!(got, Ok(7));
        assert_eq!(asked.borrow().as_slice(), &[GH], "the primary is asked first and answers");
        assert_eq!(current(), Some(Host::GitHub), "an answer from the primary keeps it");

        let got = first_ok("test", &[GH, GV], |url| if url == GH { Err("HTTP 404".into()) } else { Ok(8) });
        assert_eq!(got, Ok(8));
        assert_eq!(current(), Some(Host::GitVerse), "GitHub failed, GitVerse answered: GitVerse is the primary now");
    }

    #[test]
    fn a_429_or_503_on_the_primary_never_learns_another_primary() {
        learn(Host::GitVerse, "test: start");
        let got = first_ok("test", &[GV, GH], |url| if url == GV { Err("HTTP 429".into()) } else { Ok(3) });
        assert_eq!(got, Ok(3), "the next mirror answers at once");
        assert_eq!(current(), Some(Host::GitVerse), "a throttle is no reason to move the primary");
        let got = first_ok("test", &[GV, GH], |url| if url == GV { Err("error: 503".into()) } else { Ok(4) });
        assert_eq!(got, Ok(4));
        assert_eq!(current(), Some(Host::GitVerse));
        let mut c = Chain::new(Some(Host::GitVerse));
        c.failed(GV, "The requested URL returned error: 429");
        assert_eq!(c.succeeded(GH), None, "curl's 429 words count as a throttle too");
    }

    #[test]
    fn the_java_side_reply_gives_its_status_and_retry_after() {
        assert_eq!(http_reply("HTTP 429 retry-after 30"), Some((429, Some(30))));
        assert_eq!(http_reply("HTTP 503"), Some((503, None)));
        assert_eq!(http_reply("HTTP 429 retry-after Wed, 21 Oct 2026 07:28:00 GMT"), Some((429, None)), "a date is no number of seconds");
        assert_eq!(http_reply("206 1234"), None);
        assert_eq!(http_reply("stalled: timeout"), None);
    }

    #[test]
    fn a_throttle_is_recognised_in_every_words_it_comes_in() {
        for why in ["HTTP 429", "the server answered HTTP 503 to a range request", "the server refused the file (HTTP 429)", "curl: (22) The requested URL returned error: 429"] {
            assert!(is_throttle(why), "{why}");
        }
        for why in ["stalled", "HTTP 404", "HTTP 4290", "reply: expected value at line 1", "curl: (6) could not resolve"] {
            assert!(!is_throttle(why), "{why}");
        }
    }

    #[test]
    fn a_throttled_host_is_held_last_and_the_others_keep_their_order() {
        let list = [GV, GH, OTHER, GV2];
        let held = |u: &str| Host::of(u) == Some(Host::GitVerse);
        assert_eq!(held_last_by(list.to_vec(), held), vec![GH, OTHER, GV, GV2]);
        assert_eq!(held_last_by(list.to_vec(), |_| false), list.to_vec(), "nothing held: the list as it is");
    }

    #[test]
    fn a_pause_is_shared_by_one_host_grows_without_a_retry_after_and_is_reset_by_an_answer() {
        let now = Instant::now();
        let mut pauses = Pauses::default();
        let first = pauses.throttled(Host::GitVerse, now, None, 0.5);
        assert_eq!(first, Duration::from_secs(2), "no Retry-After: 2 s at the middle of the jitter");
        let second = pauses.throttled(Host::GitVerse, now, None, 0.5);
        assert_eq!(second, Duration::from_secs(4), "the next throttle in a row is longer");
        assert_eq!(pauses.remaining(Host::GitVerse, now), Duration::from_secs(4), "one pause for the whole host");
        assert_eq!(pauses.remaining(Host::GitHub, now), Duration::ZERO, "another host is not held back");
        assert_eq!(pauses.throttled(Host::GitVerse, now, Some(7), 0.5), Duration::from_secs(7), "Retry-After is honoured");
        assert_eq!(pauses.throttled(Host::GitVerse, now, Some(999), 0.5), Duration::from_secs(60), "and capped");
        pauses.answered(Host::GitVerse);
        assert_eq!(pauses.remaining(Host::GitVerse, now), Duration::ZERO);
        assert_eq!(pauses.throttled(Host::GitVerse, now, None, 0.5), Duration::from_secs(2), "the level starts again");
    }

    #[test]
    fn the_jitter_keeps_a_pause_within_75_to_125_percent() {
        let now = Instant::now();
        let low = Pauses::default().throttled(Host::GitHub, now, Some(20), 0.0);
        let high = Pauses::default().throttled(Host::GitHub, now, Some(20), 1.0);
        assert_eq!(low, Duration::from_secs(15));
        assert_eq!(high, Duration::from_secs(25));
        let long = Pauses::default().throttled(Host::GitHub, now, None, 1.0);
        assert!(long <= Duration::from_secs(75), "the cap holds with the jitter: {long:?}");
    }

    #[test]
    fn retrying_tries_again_after_a_throttle_only_and_gives_up_at_the_limit() {
        let calls = RefCell::new(0u32);
        let got = retrying("test", || {
            *calls.borrow_mut() += 1;
            if *calls.borrow() == 1 { Err("HTTP 503".to_string()) } else { Ok(5) }
        });
        assert_eq!(got, Ok(5));
        assert_eq!(*calls.borrow(), 2);

        let calls = RefCell::new(0u32);
        let got: Result<(), String> = retrying("test", || {
            *calls.borrow_mut() += 1;
            Err("HTTP 429".to_string())
        });
        assert_eq!(got, Err("HTTP 429".to_string()));
        assert_eq!(*calls.borrow(), 1 + THROTTLE_RETRIES, "the first try and the retries");

        let calls = RefCell::new(0u32);
        let got: Result<(), String> = retrying("test", || {
            *calls.borrow_mut() += 1;
            Err("stalled".to_string())
        });
        assert_eq!(got, Err("stalled".to_string()));
        assert_eq!(*calls.borrow(), 1, "a stall is not retried here");
    }

    #[test]
    fn a_failure_on_the_other_host_does_not_move_the_primary() {
        learn(Host::GitVerse, "test: start");
        let got = first_ok("test", &[GV, GH], |url| if url == GV { Ok(1) } else { Err("no".into()) });
        assert_eq!(got, Ok(1));
        assert_eq!(current(), Some(Host::GitVerse));
    }

    #[test]
    fn the_updater_download_host_becomes_the_primary() {
        learn(Host::GitVerse, "test: start");
        learn_from(GH2, "test: the update came from it");
        assert_eq!(current(), Some(Host::GitHub));
        learn_from(OTHER, "test: an unknown host changes nothing");
        assert_eq!(current(), Some(Host::GitHub));
    }

    #[test]
    fn the_saved_primary_reads_back_and_a_garbage_file_is_no_primary() {
        assert_eq!(parse_pref(br#"{"host":"gitverse","since":1763000000,"order":2}"#), Some(Host::GitVerse));
        assert_eq!(parse_pref(br#"{"host":"github","order":2}"#), Some(Host::GitHub));
        let bad: [&[u8]; 8] = [
            b"",
            b"garbage",
            b"{\"host\":\"mars\",\"order\":2}",
            b"{\"host\":5,\"order\":2}",
            b"[1,2]",
            b"\xff\xfe",
            br#"{"host":"gitverse","since":1763000000}"#,
            br#"{"host":"gitverse","order":1}"#,
        ];
        for bad in bad {
            assert_eq!(parse_pref(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_saved_primary_is_written_and_read_from_its_file() {
        let dir = std::env::temp_dir().join(format!("razdor-mirror-pref-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join(PREF_FILE);
        write_pref(&path, Host::GitHub);
        assert_eq!(parse_pref(&fs::read(&path).unwrap()), Some(Host::GitHub));
        write_pref(&path, Host::GitVerse);
        assert_eq!(parse_pref(&fs::read(&path).unwrap()), Some(Host::GitVerse), "written with the order it was learned under");
        fs::write(&path, b"not json").unwrap();
        assert_eq!(parse_pref(&fs::read(&path).unwrap()), None, "a damaged file is no primary");
        let _ = fs::remove_dir_all(&dir);
    }
}
