//! The service info the app reads from `config.json` on both Pages sites (GitVerse first, GitHub
//! after): the update feed's addresses, the hub's sources and the game archive. Its source is
//! `data/remote/config.json` (on both sites, the same bytes), so a change there needs no app
//! release. The built-in values (`mirror`) are used until a fresh config arrives, or when none does;
//! the last good config is kept in the cache folder and used at the next start. See
//! `docs/specs/2026-11-13-gitverse-mirrors.md` (Remote config). Nothing here needs macroquad.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Deserialize;

use crate::mirror;

/// The schema this app reads. A config of another schema is ignored whole (the built-ins are used).
pub const SCHEMA: u32 = 1;

/// Where the config is, GitVerse first. GitVerse's Pages answer an unknown path with HTTP 200 and an
/// HTML page, so only a config that parses counts.
pub const ADDRESSES: [&str; 2] = [
    "https://indicozy.gitverse.site/razdor-community/config.json",
    "https://indicozy.github.io/razdor/config.json",
];

/// The fetch's stall limit in seconds (the update feed's).
const SECONDS: u32 = 20;

/// The last good config, in the cache folder (`platform::cache_dir`).
const CACHE_FILE: &str = "remote-config.json";

/// The game archive as the config gives it (the built-in one when the config's is not good).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub links: Vec<String>,
    pub size: u64,
    pub sha256: String,
    pub name: String,
}

/// What the app reads from the config. Every list is non-empty: a list the config leaves out, or
/// whose addresses are all bad, is the built-in one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub update_feed: Vec<String>,
    pub hub: Vec<String>,
    pub game: Game,
}

/// The config as the file spells it. Unknown fields are ignored (a newer config keeps working).
#[derive(Deserialize)]
struct Raw {
    schema: u32,
    #[serde(default)]
    update_feed: Vec<String>,
    #[serde(default)]
    hub: Vec<String>,
    #[serde(default)]
    game: Option<RawGame>,
}

#[derive(Deserialize)]
struct RawGame {
    #[serde(default)]
    links: Vec<String>,
    size: u64,
    sha256: String,
    name: String,
}

/// A config that was picked, its source for the log and, when it was fetched, its bytes (to keep).
#[derive(Debug)]
pub struct Pick {
    pub config: Config,
    pub source: String,
    pub fresh: Option<Vec<u8>>,
}

impl Config {
    /// The built-in config: today's addresses (`mirror`).
    pub fn builtin() -> Config {
        Config {
            update_feed: mirror::UPDATE_FEED.iter().map(|s| s.to_string()).collect(),
            hub: mirror::HUB.iter().map(|s| s.to_string()).collect(),
            game: Game {
                links: mirror::GAME.iter().map(|s| s.to_string()).collect(),
                size: 44_619_982,
                sha256: "dff554e22a0485f0ed06b3459d3e7e868224d49e264bba0ecf40db852514568d".into(),
                name: "discord-times-cu.zip".into(),
            },
        }
    }

    /// The config the bytes give, checked: a web page, a bad JSON or another schema is an error;
    /// a bad address is dropped (logged), and a list or the game that is left with nothing good is
    /// the built-in one.
    pub fn parse(bytes: &[u8]) -> Result<Config, String> {
        if crate::gdrive::is_html(bytes) {
            return Err("a web page, not the config".into());
        }
        let raw: Raw = serde_json::from_slice(bytes).map_err(|e| format!("not a config: {e}"))?;
        if raw.schema != SCHEMA {
            return Err(format!("schema {} (this Razdor reads schema {SCHEMA})", raw.schema));
        }
        let built = Config::builtin();
        let update_feed = addresses("update_feed", raw.update_feed).unwrap_or(built.update_feed);
        let hub = addresses("hub", raw.hub).unwrap_or(built.hub);
        let game = match raw.game.map(game) {
            Some(Ok(g)) => g,
            Some(Err(why)) => {
                crate::diag!("remote config: game: {why}; the built-in game is used");
                built.game
            }
            None => built.game,
        };
        Ok(Config { update_feed, hub, game })
    }
}

/// An address the app may fetch: https, with no spaces, control characters or backslashes.
pub fn good_address(address: &str) -> bool {
    address.len() > "https://".len() && address.starts_with("https://") && !address.chars().any(|c| c.is_whitespace() || c.is_control() || c == '\\')
}

/// The list's good addresses, or `None` when there are none (the caller keeps the built-in list).
fn addresses(what: &str, list: Vec<String>) -> Option<Vec<String>> {
    let kept: Vec<String> = list
        .into_iter()
        .filter(|a| {
            let ok = good_address(a);
            if !ok {
                crate::diag!("remote config: {what}: {a:?} is dropped (not an https address)");
            }
            ok
        })
        .collect();
    (!kept.is_empty()).then_some(kept)
}

/// The config's game, checked like the hub catalog's: a file name, https links, a SHA-256 (lower-cased),
/// a sane size. Any failure refuses the game (the built-in one is used).
fn game(g: RawGame) -> Result<Game, String> {
    if g.name.contains('/') || !crate::hub::safe_rel_name(&g.name) {
        return Err(format!("bad file name {:?}", g.name));
    }
    if g.sha256.len() != 64 || !g.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("the SHA-256 is not 64 hex digits".into());
    }
    if g.size == 0 || g.size > crate::hub::MAX_ARCHIVE_DOWNLOAD {
        return Err(format!("bad size {}", g.size));
    }
    let links = addresses("game", g.links).ok_or_else(|| "no https address".to_string())?;
    Ok(Game { links, size: g.size, sha256: g.sha256.to_ascii_lowercase(), name: g.name })
}

/// Picks the config: the first of `addresses` whose fetched bytes parse (`fetch` gives the bytes of
/// one address or why not; a failure is logged and the next address is tried). When none does, the
/// `cached` config (the last good one) if it parses, else the built-in config.
pub fn resolve(addresses: &[&str], mut fetch: impl FnMut(&str) -> Result<Vec<u8>, String>, cached: Option<&[u8]>) -> Pick {
    let got = mirror::first_ok("remote config", addresses, |url| {
        let bytes = fetch(url)?;
        let config = Config::parse(&bytes)?;
        Ok((url.to_string(), config, bytes))
    });
    match got {
        Ok((url, config, bytes)) => Pick { config, source: format!("fresh from {url}"), fresh: Some(bytes) },
        Err(why) => match cached.map(Config::parse) {
            Some(Ok(config)) => Pick { config, source: format!("cached (no fresh config: {why})"), fresh: None },
            _ => Pick { config: Config::builtin(), source: format!("built-in (no config: {why})"), fresh: None },
        },
    }
}

/// The config in use. `None` until [`start`] installs one: the built-in config.
static CURRENT: Mutex<Option<Config>> = Mutex::new(None);

fn install(pick: Pick) {
    crate::diag!("remote config: using the {}", pick.source);
    if let Ok(mut current) = CURRENT.lock() {
        *current = Some(pick.config);
    }
}

fn current() -> Config {
    CURRENT.lock().ok().and_then(|c| c.clone()).unwrap_or_else(Config::builtin)
}

/// The update feed's addresses, in the order they are tried (`update::check`).
pub fn update_feed() -> Vec<String> {
    current().update_feed
}

/// The hub's source folders, in the order they are tried (the first one is the hub's label).
pub fn hub() -> Vec<String> {
    current().hub
}

/// The game archive as the config gives it (`gdrive::game`).
pub fn game_info() -> Game {
    current().game
}

/// A list as the `&str` slices the fetchers take.
pub fn refs(list: &[String]) -> Vec<&str> {
    list.iter().map(String::as_str).collect()
}

fn cache_path() -> Option<PathBuf> {
    crate::platform::cache_dir().map(|d| d.join(CACHE_FILE))
}

fn read_cache() -> Option<Vec<u8>> {
    std::fs::read(cache_path()?).ok()
}

/// Keeps the fetched bytes as the cache (written whole under a temporary name, then renamed).
fn write_cache(bytes: &[u8]) {
    let Some(path) = cache_path() else { return };
    let tmp = path.with_extension("json.part");
    let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&tmp, bytes)).and_then(|_| std::fs::rename(&tmp, &path));
    if let Err(e) = written {
        crate::diag!("remote config: the cache is not written: {e}");
    }
}

/// Start-up: the cached config is used at once when it parses; a thread then fetches the fresh one
/// from [`ADDRESSES`] and keeps it. Nothing waits for the network.
pub fn start() {
    let cached = read_cache();
    if let Some(Ok(config)) = cached.as_deref().map(Config::parse) {
        install(Pick { config, source: "cached config (the last start's)".into(), fresh: None });
    }
    let spawned = std::thread::Builder::new().name("remote config".into()).spawn(move || {
        let pick = resolve(&ADDRESSES, |url| crate::update::fetch(url, SECONDS), cached.as_deref());
        if let Some(bytes) = &pick.fresh {
            write_cache(bytes);
        }
        install(pick);
    });
    if let Err(e) = spawned {
        crate::diag!("remote config: the fetch does not start: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const GOOD: &str = r#"{
      "schema": 1,
      "update_feed": ["https://feed.example/releases.json", "https://api.example/releases"],
      "hub": ["https://hub.example/razdor", "https://github.example/latest/download"],
      "game": {
        "links": ["https://gitverse.example/game.zip", "https://github.example/game.zip"],
        "size": 1234,
        "sha256": "ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789",
        "name": "game.zip"
      }
    }"#;

    fn good() -> Config {
        Config::parse(GOOD.as_bytes()).unwrap()
    }

    #[test]
    fn a_good_config_gives_its_lists_and_game() {
        let c = good();
        assert_eq!(c.update_feed, vec!["https://feed.example/releases.json", "https://api.example/releases"]);
        assert_eq!(c.hub, vec!["https://hub.example/razdor", "https://github.example/latest/download"]);
        assert_eq!(c.game.links, vec!["https://gitverse.example/game.zip", "https://github.example/game.zip"]);
        assert_eq!(c.game.size, 1234);
        assert_eq!(c.game.sha256, "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789", "the SHA-256 is kept in lower case");
        assert_eq!(c.game.name, "game.zip");
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let text = GOOD.replace("\"schema\": 1,", "\"schema\": 1, \"news\": {\"x\": [1, 2]},").replace("\"size\": 1234,", "\"size\": 1234, \"extra\": true,");
        assert_eq!(Config::parse(text.as_bytes()).unwrap(), good());
    }

    #[test]
    fn another_schema_is_ignored_whole() {
        for schema in ["2", "0"] {
            let text = GOOD.replace("\"schema\": 1", &format!("\"schema\": {schema}"));
            let err = Config::parse(text.as_bytes()).unwrap_err();
            assert!(err.contains("schema"), "{err}");
        }
    }

    #[test]
    fn a_bad_address_is_dropped_and_the_rest_stays() {
        let text = GOOD.replace(
            "\"update_feed\": [\"https://feed.example/releases.json\", \"https://api.example/releases\"]",
            "\"update_feed\": [\"http://plain.example/x.json\", \"https://feed.example/releases.json\", \"relative/x.json\", \"https://sp ace.example/x\", \"https://back\\\\slash.example/x\", \"https://\"]",
        );
        assert_eq!(Config::parse(text.as_bytes()).unwrap().update_feed, vec!["https://feed.example/releases.json"]);
    }

    #[test]
    fn a_list_with_no_good_address_is_the_built_in_one() {
        let text = GOOD.replace("\"hub\": [\"https://hub.example/razdor\", \"https://github.example/latest/download\"]", "\"hub\": [\"http://plain.example\"]");
        assert_eq!(Config::parse(text.as_bytes()).unwrap().hub, Config::builtin().hub);
        let none = GOOD.replace("\"update_feed\": [\"https://feed.example/releases.json\", \"https://api.example/releases\"]", "\"update_feed\": []");
        assert_eq!(Config::parse(none.as_bytes()).unwrap().update_feed, Config::builtin().update_feed);
    }

    #[test]
    fn a_web_page_is_refused() {
        let html = b"<!doctype html><html><body>Page not found</body></html>";
        assert!(Config::parse(html).unwrap_err().contains("web page"));
        assert!(Config::parse(b"  <html>").is_err());
        assert!(Config::parse(b"not json").is_err());
    }

    #[test]
    fn a_bad_game_is_the_built_in_game() {
        let base = GOOD.to_string();
        let bad = [
            base.replace("\"name\": \"game.zip\"", "\"name\": \"../game.zip\""),
            base.replace("\"name\": \"game.zip\"", "\"name\": \"\""),
            base.replace("ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789ABCDEF0123456789", "abc"),
            base.replace("\"size\": 1234", "\"size\": 0"),
            base.replace("\"size\": 1234", &format!("\"size\": {}", crate::hub::MAX_ARCHIVE_DOWNLOAD + 1)),
            base.replace("\"links\": [\"https://gitverse.example/game.zip\", \"https://github.example/game.zip\"]", "\"links\": [\"http://plain.example/g.zip\"]"),
        ];
        for text in bad {
            assert_eq!(Config::parse(text.as_bytes()).unwrap().game, Config::builtin().game, "{text}");
        }
    }

    #[test]
    fn the_built_in_config_has_today_s_addresses() {
        let b = Config::builtin();
        assert!(b.update_feed[0].starts_with("https://indicozy.gitverse.site/"));
        assert!(b.hub[0].starts_with("https://indicozy.gitverse.site/"));
        assert_eq!(b.game.links[0], "https://gitverse.ru/api/attachments/36c22adf-f3f3-4286-a474-9b6a00a5abc5");
    }

    #[test]
    fn the_shipped_config_is_a_good_config() {
        let text = include_bytes!("../data/remote/config.json");
        let c = Config::parse(text).unwrap();
        assert_eq!(c, Config::builtin(), "the shipped config matches the built-in one until it is changed on purpose");
    }

    /// A fake network: each address gives its reply (or why not), and the addresses asked are kept.
    fn net(replies: Vec<(&'static str, Result<Vec<u8>, String>)>) -> (impl FnMut(&str) -> Result<Vec<u8>, String>, std::rc::Rc<RefCell<Vec<String>>>) {
        let asked = std::rc::Rc::new(RefCell::new(Vec::new()));
        let seen = asked.clone();
        let fetch = move |url: &str| {
            seen.borrow_mut().push(url.to_string());
            replies.iter().find(|(u, _)| *u == url).map(|(_, r)| r.clone()).unwrap_or_else(|| Err("no route".into()))
        };
        (fetch, asked)
    }

    const A: &str = "https://a.example/config.json";
    const B: &str = "https://b.example/config.json";

    #[test]
    fn a_fresh_config_wins_over_the_cache_and_the_built_in() {
        let cached = GOOD.replace("game.zip", "old.zip");
        let (fetch, asked) = net(vec![(A, Ok(GOOD.as_bytes().to_vec()))]);
        let pick = resolve(&[A, B], fetch, Some(cached.as_bytes()));
        assert_eq!(pick.config, good());
        assert_eq!(pick.source, format!("fresh from {A}"));
        assert_eq!(pick.fresh.as_deref(), Some(GOOD.as_bytes()), "the bytes to keep");
        assert_eq!(asked.borrow().as_slice(), &[A], "the second address is not asked");
    }

    #[test]
    fn a_web_page_from_the_first_address_gives_way_to_the_second() {
        let page = b"<!doctype html><html>not found</html>".to_vec();
        let (fetch, asked) = net(vec![(A, Ok(page)), (B, Ok(GOOD.as_bytes().to_vec()))]);
        let pick = resolve(&[A, B], fetch, None);
        assert_eq!(pick.source, format!("fresh from {B}"));
        assert_eq!(pick.config, good());
        assert_eq!(asked.borrow().as_slice(), &[A, B]);
    }

    #[test]
    fn with_no_fresh_config_the_cache_is_used() {
        let cached = GOOD.replace("game.zip", "old.zip");
        let (fetch, _) = net(vec![(A, Err("curl: (6) could not resolve".into()))]);
        let pick = resolve(&[A, B], fetch, Some(cached.as_bytes()));
        assert_eq!(pick.config.game.name, "old.zip");
        assert!(pick.fresh.is_none(), "nothing new to keep");
        assert!(pick.source.starts_with("cached"), "{}", pick.source);
    }

    #[test]
    fn with_no_fresh_config_and_a_bad_cache_the_built_in_is_used() {
        let (fetch, _) = net(vec![]);
        let pick = resolve(&[A, B], fetch, Some(b"<html>"));
        assert_eq!(pick.config, Config::builtin());
        assert!(pick.source.starts_with("built-in"), "{}", pick.source);
        let (fetch, _) = net(vec![]);
        let none = resolve(&[A, B], fetch, None);
        assert_eq!(none.config, Config::builtin());
        assert!(none.fresh.is_none());
    }

    #[test]
    fn consumers_read_the_lists_of_the_config() {
        // The fetchers take the config's lists as they are: the hub's catalog is asked at the
        // config's addresses, in its order, and the game archive has the config's links.
        let c = good();
        let mut asked = Vec::new();
        let _ = crate::hub::catalog_from_mirrors(&refs(&c.hub), |url| {
            asked.push(url.to_string());
            Err("no route".into())
        });
        assert_eq!(asked, vec!["https://hub.example/razdor/index-v2.json", "https://hub.example/razdor/index.json", "https://github.example/latest/download/index-v2.json", "https://github.example/latest/download/index.json"]);
        let archive = crate::gdrive::archive_from(&c.game);
        assert_eq!(archive.links.iter().map(|l| l.to_string()).collect::<Vec<_>>(), c.game.links);
        assert_eq!(archive.size, 1234);
        assert_eq!(refs(&c.update_feed), vec!["https://feed.example/releases.json", "https://api.example/releases"]);
    }

    #[test]
    fn before_start_the_built_in_config_is_used() {
        assert_eq!(update_feed(), Config::builtin().update_feed);
        assert_eq!(hub(), Config::builtin().hub);
        assert_eq!(game_info(), Config::builtin().game);
    }
}
