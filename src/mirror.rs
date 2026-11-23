//! The built-in addresses Razdor reads and downloads from, each list in the order it is tried:
//! GitHub first, GitVerse as the fallback mirror for players in Russia. They are the defaults of the
//! remote config (`remote_config::Config::builtin`): the lists in use are the config's, which the
//! app takes from `config.json` (see `remote_config`). See `docs/specs/2026-11-13-gitverse-mirrors.md`.
//!
//! The rule (the owner's decision, 2026-11-23): GitHub is tried first on every request, and GitVerse
//! is only that request's fallback. Nothing is learned or remembered between requests or launches: a
//! GitHub failure in one request does not change the order of the next. The list is always the
//! config's order ([`order`]). A host paused by a throttle (429, 503) goes last for its requests while
//! the pause lasts (`throttled`, `wait_host`); the pauses are per session. A `mirror-pref.json` left
//! by an older build (it saved a learned primary) is deleted at start ([`remove_old_pref`]).
//! `RAZDOR_HUB` is one explicit source and is never reordered. Nothing here needs macroquad.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

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

/// The file an older build saved its learned primary in, in the cache folder (`platform::cache_dir`).
/// Nothing writes it now; a leftover one is deleted at start ([`remove_old_pref`]).
const PREF_FILE: &str = "mirror-pref.json";

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

/// The list in the order it is tried: the config's order, with the addresses of a paused (throttled)
/// host last ([`held_last_by`]). Nothing is learned, so the order does not depend on earlier requests.
pub fn order<S: AsRef<str>>(list: &[S]) -> Vec<&str> {
    held_last_by(list.iter().map(|s| s.as_ref()).collect(), paused)
}

/// The addresses whose host is held (paused) go after the others, each part in its order: a throttled
/// host is tried when the other addresses have failed, and then it waits its pause (see [`wait_host`]).
fn held_last_by(list: Vec<&str>, held: impl Fn(&str) -> bool) -> Vec<&str> {
    let (live, held): (Vec<&str>, Vec<&str>) = list.into_iter().partition(|u| !held(u));
    live.into_iter().chain(held).collect()
}

/// Tries the addresses in order ([`order`]): `attempt` reads what one of them gives, or says why not.
/// The first `Ok` is the answer. Each failure is logged with its address and reason; when none
/// answers, the last reason is the error.
pub fn first_ok<T, S: AsRef<str>>(what: &str, addresses: &[S], mut attempt: impl FnMut(&str) -> Result<T, String>) -> Result<T, String> {
    let mut last = String::from("no address");
    for address in order(addresses) {
        match attempt(address) {
            Ok(value) => {
                crate::diag!("{what}: from {address}");
                return Ok(value);
            }
            Err(why) => {
                crate::diag!("{what}: {address}: {why}");
                last = why;
            }
        }
    }
    Err(last)
}

/// The list ordered as [`order`], as owned strings (for a `Vec<String>` list).
pub fn ordered_owned(list: &[String]) -> Vec<String> {
    order(list).into_iter().map(String::from).collect()
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

/// Deletes the leftover `mirror-pref.json` of an older build, which saved a learned primary host. This
/// build never reads it. Best effort: an error is ignored.
pub fn remove_old_pref() {
    #[cfg(not(test))]
    {
        if let Some(path) = crate::platform::cache_dir().map(|d| d.join(PREF_FILE)) {
            remove_pref_file(&path);
        }
    }
}

/// Deletes the file at `path` if it is there; any error is ignored.
fn remove_pref_file(path: &Path) {
    let _ = fs::remove_file(path);
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
    fn the_order_is_the_configs_whatever_came_before() {
        let list = [GH, GV, OTHER, GH2, GV2];
        assert_eq!(order(&list), list.to_vec(), "the config's order: no learned primary");
        let owned: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        assert_eq!(ordered_owned(&owned), owned, "the owned list is ordered the same");
    }

    #[test]
    fn a_github_failure_in_one_request_does_not_change_the_next_requests_order() {
        let list = [GH, GV];
        let got = first_ok("test", &list, |url| if url == GH { Err("stalled".into()) } else { Ok(9) });
        assert_eq!(got, Ok(9), "GitVerse answers this request as its fallback");
        let asked = RefCell::new(Vec::new());
        let got = first_ok("test", &list, |url| {
            asked.borrow_mut().push(url.to_string());
            Ok(10)
        });
        assert_eq!(got, Ok(10));
        assert_eq!(asked.borrow().as_slice(), &[GH], "the next request asks GitHub first and stops there");
    }

    #[test]
    fn a_throttled_host_goes_last_while_its_pause_lasts_and_the_rest_keep_their_order() {
        let now = Instant::now();
        let mut pauses = Pauses::default();
        pauses.throttled(Host::GitHub, now, Some(30), 0.5);
        let held = |u: &str| Host::of(u).is_some_and(|h| !pauses.remaining(h, now).is_zero());
        let list = [GH, GV, OTHER, GH2, GV2];
        assert_eq!(held_last_by(list.to_vec(), held), vec![GV, OTHER, GV2, GH, GH2], "GitHub is paused: last, in its order");
        pauses.answered(Host::GitHub);
        let held = |u: &str| Host::of(u).is_some_and(|h| !pauses.remaining(h, now).is_zero());
        assert_eq!(held_last_by(list.to_vec(), held), list.to_vec(), "the answer ends the pause: GitHub first again");
    }

    #[test]
    fn a_leftover_saved_primary_is_deleted_and_a_missing_one_is_no_error() {
        let dir = std::env::temp_dir().join(format!("razdor-mirror-old-pref-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join(PREF_FILE);
        fs::write(&path, br#"{"host":"gitverse","since":1763000000,"order":2}"#).unwrap();
        remove_pref_file(&path);
        assert!(!path.exists(), "the old file is removed");
        remove_pref_file(&path);
        let _ = fs::remove_dir_all(&dir);
    }

}
