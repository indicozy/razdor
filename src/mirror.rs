//! The built-in addresses Razdor reads and downloads from, each list in the order it is tried:
//! GitVerse first (players in Russia reach it), GitHub as the fallback. They are the defaults of the
//! remote config (`remote_config::Config::builtin`): the lists in use are the config's, which the
//! app takes from `config.json` (see `remote_config`). See `docs/specs/2026-11-13-gitverse-mirrors.md`.
//!
//! Learned primary (the owner's decision, 2026-11-13): the host that works for a player becomes that
//! player's primary. Every list is ordered with the primary host's addresses first ([`ordered_by`]);
//! the config's order is the tie-break within a host and the order when there is no primary yet. A
//! primary moves only when a request's own fallback chain shows it: the primary host failed, then
//! another host answered ([`Chain`]). The primary is saved in the cache folder (`mirror-pref.json`,
//! [`learn`]); a file that does not read is no primary. `RAZDOR_HUB` is one explicit source and is
//! never reordered. Nothing here needs macroquad.

use std::fs;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;
#[cfg(not(test))]
use std::sync::Mutex;

use serde::Deserialize;

/// The built-in update feed: the release list in the GitHub API's shape. GitVerse's copy is a static
/// file on the community's Pages; GitHub's is the API.
pub const UPDATE_FEED: [&str; 2] = [
    "https://indicozy.gitverse.site/razdor-community/releases.json",
    "https://api.github.com/repos/indicozy/razdor/releases?per_page=30",
];

/// The built-in hub folders: the catalog's `index.json` names the files, resolved against the folder
/// that gave the catalog. GitHub's is `hub::DEFAULT_SOURCE`.
pub const HUB: [&str; 2] = ["https://indicozy.gitverse.site/razdor-community", crate::hub::DEFAULT_SOURCE];

/// The built-in game archive (`gdrive::game` gives the one in use): GitVerse's attachment, then
/// GitHub's release file.
pub const GAME: [&str; 2] = [
    "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5",
    "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip",
];

/// The file the learned primary is saved in, in the cache folder (`platform::cache_dir`).
const PREF_FILE: &str = "mirror-pref.json";

/// The two hosts Razdor downloads from. GitVerse is `gitverse.ru` and `*.gitverse.site` (the Pages);
/// GitHub is `github.com`, `api.github.com`, `*.github.io` and the release file hosts.
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

    /// The list in this chain's order ([`ordered_by`] with the chain's primary).
    pub fn order<'a, S: AsRef<str>>(&self, list: &'a [S]) -> Vec<&'a str> {
        ordered_by(self.primary, list)
    }

    /// An address of this chain failed (for whatever reason).
    pub fn failed(&mut self, url: &str) {
        if self.primary.is_some() && Host::of(url) == self.primary {
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
                chain.failed(address);
                last = why;
            }
        }
    }
    Err(last)
}

/// The primary host a saved file gives: `{"host": "gitverse" | "github", ...}`. Anything else (a
/// damaged file, another word) is no primary.
pub fn parse_pref(bytes: &[u8]) -> Option<Host> {
    #[derive(Deserialize)]
    struct Saved {
        host: String,
    }
    let saved: Saved = serde_json::from_slice(bytes).ok()?;
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
    let since = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let json = serde_json::json!({ "host": to.name(), "since": since }).to_string();
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
    fn the_addresses_are_gitverse_first_and_github_last() {
        assert!(UPDATE_FEED[0].starts_with("https://indicozy.gitverse.site/"));
        assert!(UPDATE_FEED[1].starts_with("https://api.github.com/"));
        assert!(HUB[0].starts_with("https://indicozy.gitverse.site/"));
        assert_eq!(HUB[1], crate::hub::DEFAULT_SOURCE);
        assert!(GAME[0].starts_with("https://gitverse.ru/api/attachments/"));
        assert!(GAME[1].starts_with("https://github.com/"));
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
        c.failed(GH);
        assert_eq!(c.succeeded(GH2), None, "another address of the primary's host keeps it");
        c.failed(GV);
        assert_eq!(c.succeeded(GV2), Some(Host::GitVerse), "the primary failed, then GitVerse answered");

        let mut other = Chain::new(Some(Host::GitHub));
        other.failed(GV);
        assert_eq!(other.succeeded(GV2), None, "a failure on the other host alone does not move the primary");
        assert_eq!(other.succeeded(GH), None);

        let mut none = Chain::new(None);
        none.failed(GH);
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
        assert_eq!(parse_pref(br#"{"host":"gitverse","since":1763000000}"#), Some(Host::GitVerse));
        assert_eq!(parse_pref(br#"{"host":"github"}"#), Some(Host::GitHub));
        let bad: [&[u8]; 6] = [b"", b"garbage", b"{\"host\":\"mars\"}", b"{\"host\":5}", b"[1,2]", b"\xff\xfe"];
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
        fs::write(&path, b"not json").unwrap();
        assert_eq!(parse_pref(&fs::read(&path).unwrap()), None, "a damaged file is no primary");
        let _ = fs::remove_dir_all(&dir);
    }
}
