//! The service info the app reads from `config.json` (GitHub's Pages first, the owner's own server
//! after): the update feed's addresses, the hub's sources and the game archive. Its source is
//! `data/remote/config.json` (published to GitHub Pages, copied to the own server by `razdor-mirror`),
//! so a change there needs no app release. The built-in values (`mirror`) are used until a fresh
//! config arrives, or when none does; the last good config is kept in the cache folder and used at the
//! next start. See `docs/specs/own-mirror-2026-11-24.md`. Nothing here needs macroquad.

use std::path::PathBuf;
use std::sync::Mutex;

use serde::Deserialize;

use crate::mirror;

/// The schema this app reads. A config of another schema is ignored whole (the built-ins are used).
pub const SCHEMA: u32 = 1;

/// Where the config is, GitHub's Pages first, the owner's own server after (the owner's decision,
/// 2026-11-24). A copy that answers an unknown path with an HTML page is no config: only a config that
/// parses counts.
pub const ADDRESSES: [&str; 2] = [
    "https://indicozy.github.io/razdor/config.json",
    "https://razdor.indicozy.dev/hub/pages/razdor/config.json",
];

/// The analytics and cloud API's built-in address: the owner's site at `/api` (one domain and a path).
/// The server's routes are `/v1/...`, so the reverse proxy strips `/api`. The address has no trailing
/// slash: the sender joins it with a path that starts with `/`. The remote config's `api` list can move
/// it without a release.
pub const API_BUILTIN: &str = "https://razdor.indicozy.dev/api";

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
    /// The API's addresses, in the order they are tried (`telemetry`).
    pub api: Vec<String>,
    /// The send switch: `false` stops every send of the analytics. Off by default: the built-in config
    /// and a config without the key both say `false`, so nothing is sent until the remote config says
    /// `"telemetry": true` (the owner turns it on after the server has run stably for 2-3 days).
    pub telemetry: bool,
    /// The cloud switch: `false` hides the cloud saves (the settings rows, the load list's marks, the
    /// Hub offer) and keeps the worker from starting, even with an account key on this device. Off by
    /// default: the built-in config and a config without the key both say `false`; the owner turns it
    /// on when the cloud server is live.
    pub cloud: bool,
    /// The cloud saves a device mirrors (its cap): `cloud_saves_per_device`, 100 by default, and never
    /// more than the server takes (`razdor_wire::limits::SAVES_PER_DEVICE`), nor less than one.
    pub cloud_saves_per_device: usize,
    /// The holiday season (`holiday`): on by default, the built-in window unless the config gives one.
    pub holiday: Holiday,
}

/// The holiday season's window: inclusive dates `(year, month, day)`, `from` no later than `to`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HolidayWindow {
    pub from: (i64, u32, u32),
    pub to: (i64, u32, u32),
}

/// The holiday season as the config gives it. `on: false` is a stop switch: no season for anyone (no start,
/// no offer, no auto-install); the player's own «Новогодний вид» setting is not changed. `window` is the
/// season's dates when the config gives a good pair; None is the built-in 1 December to 15 January.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holiday {
    pub on: bool,
    pub window: Option<HolidayWindow>,
}

impl Holiday {
    /// The built-in season: on, the built-in window.
    pub const BUILTIN: Holiday = Holiday { on: true, window: None };
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
    #[serde(default)]
    api: Vec<String>,
    #[serde(default)]
    telemetry: Option<bool>,
    #[serde(default)]
    cloud: Option<bool>,
    #[serde(default)]
    cloud_saves_per_device: Option<u32>,
    #[serde(default)]
    holiday: Option<RawHoliday>,
}

/// The `holiday` object: every field is optional (a missing `on` is true, a missing or bad window is the built-in one).
#[derive(Deserialize)]
struct RawHoliday {
    on: Option<bool>,
    from: Option<String>,
    to: Option<String>,
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
/// `from` is the address the fresh config came from (the first start's race names the primary host).
#[derive(Debug)]
pub struct Pick {
    pub config: Config,
    pub source: String,
    pub fresh: Option<Vec<u8>>,
    pub from: Option<String>,
}

/// The default cloud cap per device (the server's own cap is the same number).
pub const CLOUD_SAVES_PER_DEVICE: usize = razdor_wire::limits::SAVES_PER_DEVICE;

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
            api: vec![API_BUILTIN.to_string()],
            telemetry: false,
            cloud: false,
            cloud_saves_per_device: CLOUD_SAVES_PER_DEVICE,
            holiday: Holiday::BUILTIN,
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
        let api = addresses("api", raw.api).unwrap_or(built.api);
        let telemetry = raw.telemetry.unwrap_or(false);
        let cloud = raw.cloud.unwrap_or(false);
        let cloud_saves_per_device = match raw.cloud_saves_per_device {
            None => CLOUD_SAVES_PER_DEVICE,
            Some(n) => {
                let max = razdor_wire::limits::SAVES_PER_DEVICE;
                if n == 0 || n as usize > max {
                    crate::diag!("remote config: cloud_saves_per_device {n} is out of 1..={max}; it is clamped");
                }
                (n as usize).clamp(1, max)
            }
        };
        let holiday = holiday_of(raw.holiday);
        Ok(Config { update_feed, hub, game, api, telemetry, cloud, cloud_saves_per_device, holiday })
    }
}

/// The `holiday` object as the config gives it. A missing object is the built-in season; a window needs both
/// dates, each a `YYYY-MM-DD`, with `to` not before `from`, else the built-in window is used (logged).
fn holiday_of(h: Option<RawHoliday>) -> Holiday {
    let Some(h) = h else { return Holiday::BUILTIN };
    Holiday { on: h.on.unwrap_or(true), window: holiday_window(h.from.as_deref(), h.to.as_deref()) }
}

fn holiday_window(from: Option<&str>, to: Option<&str>) -> Option<HolidayWindow> {
    let (Some(from), Some(to)) = (from, to) else { return None };
    match (parse_date(from), parse_date(to)) {
        (Some(from), Some(to)) if from <= to => Some(HolidayWindow { from, to }),
        _ => {
            crate::diag!("remote config: holiday window {from:?}..{to:?} is not two dates in order; the built-in season is used");
            None
        }
    }
}

/// A date `YYYY-MM-DD` as `(year, month, day)`, or None when it is not one (the month 1..=12, the day 1..=31).
pub fn parse_date(text: &str) -> Option<(i64, u32, u32)> {
    let mut parts = text.trim().split('-');
    let (y, m, d) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() {
        return None;
    }
    let (y, m, d): (i64, u32, u32) = (y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((y, m, d))
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

/// Picks the config: the first of `addresses` (in the config's order, GitHub first: `mirror::first_ok`) whose
/// fetched bytes parse (`fetch` gives the bytes of one address or why not; a failure is logged and the
/// next address is tried). When none does, the `cached` config (the last good one) if it parses, else
/// the built-in config.
pub fn resolve(addresses: &[&str], mut fetch: impl FnMut(&str) -> Result<Vec<u8>, String>, cached: Option<&[u8]>) -> Pick {
    let got = mirror::first_ok("remote config", addresses, |url| {
        let bytes = fetch(url)?;
        let config = Config::parse(&bytes)?;
        Ok((url.to_string(), config, bytes))
    });
    pick_from(got, cached)
}

/// The pick for an answer of the fetch (the fresh config and its address) or the reason none came.
fn pick_from(got: Result<(String, Config, Vec<u8>), String>, cached: Option<&[u8]>) -> Pick {
    match got {
        Ok((url, config, bytes)) => Pick { config, source: format!("fresh from {url}"), fresh: Some(bytes), from: Some(url) },
        Err(why) => match cached.map(Config::parse) {
            Some(Ok(config)) => Pick { config, source: format!("cached (no fresh config: {why})"), fresh: None, from: None },
            _ => Pick { config: Config::builtin(), source: format!("built-in (no config: {why})"), fresh: None, from: None },
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

/// The API's addresses, in the order they are tried (`telemetry`).
pub fn api() -> Vec<String> {
    current().api
}

/// Whether the config lets the analytics send (the kill switch; `true` unless the config turns it off).
pub fn telemetry_allowed() -> bool {
    current().telemetry
}

/// Whether the cloud saves show and run (the `cloud` switch; `false` unless the config turns it on).
pub fn cloud_allowed() -> bool {
    // A snapshot scene can switch it on (`RAZDOR_SCENE_CLOUD_ALLOWED`, for the «Облако» tab's shots); no
    // player's run sets it.
    std::env::var_os("RAZDOR_SCENE_CLOUD_ALLOWED").is_some() || current().cloud
}

/// The cloud saves this device mirrors (`cloud_saves_per_device`, clamped to what the server takes).
pub fn cloud_saves_per_device() -> usize {
    current().cloud_saves_per_device
}

/// The holiday season as the config gives it (`holiday`): the built-in one until a config says otherwise.
pub fn holiday() -> Holiday {
    current().holiday
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
/// from [`ADDRESSES`] and keeps it. Nothing waits for the network. The addresses are tried in the
/// config's order, GitHub first on every start (`resolve`); nothing is remembered between starts. A
/// `mirror-pref.json` that an older build left is deleted here.
pub fn start() {
    mirror::remove_old_pref();
    let cached = read_cache();
    if let Some(Ok(config)) = cached.as_deref().map(Config::parse) {
        install(Pick { config, source: "cached config (the last start's)".into(), fresh: None, from: None });
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
        "links": ["https://github.example/game.zip", "https://own.example/game.zip"],
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
        assert_eq!(c.game.links, vec!["https://github.example/game.zip", "https://own.example/game.zip"]);
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

    /// The config's `holiday` value: the GOOD config with the given JSON object (or none).
    fn with_holiday(object: Option<&str>) -> Config {
        let text = match object {
            Some(o) => GOOD.replace("\"schema\": 1,", &format!("\"schema\": 1, \"holiday\": {o},")),
            None => GOOD.to_string(),
        };
        Config::parse(text.as_bytes()).unwrap()
    }

    #[test]
    fn the_holiday_key_is_optional_and_defaults_to_the_built_in_season() {
        assert_eq!(good().holiday, Holiday::BUILTIN, "without the key");
        assert_eq!(with_holiday(Some("{}")).holiday, Holiday::BUILTIN, "an empty object");
        assert_eq!(with_holiday(Some("{\"on\": true}")).holiday, Holiday::BUILTIN, "on alone");
        assert_eq!(Config::builtin().holiday, Holiday::BUILTIN, "the built-in config");
    }

    #[test]
    fn the_holiday_stop_switch_and_window_are_read() {
        assert!(!with_holiday(Some("{\"on\": false}")).holiday.on);
        let window = with_holiday(Some("{\"on\": true, \"from\": \"2026-11-23\", \"to\": \"2027-01-15\"}")).holiday;
        assert_eq!(window, Holiday { on: true, window: Some(HolidayWindow { from: (2026, 11, 23), to: (2027, 1, 15) }) });
        let one_day = with_holiday(Some("{\"from\": \"2026-12-24\", \"to\": \"2026-12-24\"}")).holiday;
        assert!(one_day.window.is_some(), "a window of one day is a window");
    }

    #[test]
    fn a_missing_or_bad_window_is_the_built_in_one() {
        let only_from = with_holiday(Some("{\"from\": \"2026-11-23\"}")).holiday;
        assert_eq!(only_from, Holiday::BUILTIN, "one date is not a window");
        let backwards = with_holiday(Some("{\"from\": \"2027-01-15\", \"to\": \"2026-11-23\"}")).holiday;
        assert_eq!(backwards, Holiday::BUILTIN, "to before from");
        let bad = with_holiday(Some("{\"from\": \"tomorrow\", \"to\": \"2027-01-15\"}")).holiday;
        assert_eq!(bad, Holiday::BUILTIN, "not a date");
        let on_off = with_holiday(Some("{\"on\": false, \"from\": \"2027-01-15\", \"to\": \"2026-11-23\"}")).holiday;
        assert_eq!(on_off, Holiday { on: false, window: None }, "a bad window keeps the switch");
    }

    #[test]
    fn a_date_reads_year_month_day() {
        assert_eq!(parse_date("2026-12-01"), Some((2026, 12, 1)));
        assert_eq!(parse_date(" 2027-01-15 "), Some((2027, 1, 15)));
        assert_eq!(parse_date("2026-13-01"), None, "no month 13");
        assert_eq!(parse_date("2026-12-32"), None, "no day 32");
        assert_eq!(parse_date("2026-12"), None);
        assert_eq!(parse_date("tomorrow"), None);
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
            base.replace("\"links\": [\"https://github.example/game.zip\", \"https://own.example/game.zip\"]", "\"links\": [\"http://plain.example/g.zip\"]"),
        ];
        for text in bad {
            assert_eq!(Config::parse(text.as_bytes()).unwrap().game, Config::builtin().game, "{text}");
        }
    }

    #[test]
    fn the_built_in_config_has_today_s_addresses() {
        let b = Config::builtin();
        assert!(b.update_feed[0].starts_with("https://indicozy.github.io/"));
        assert!(b.hub[0].starts_with("https://github.com/"));
        assert_eq!(b.game.links[0], "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip");
        assert_eq!(b.game.links[1], "https://razdor.indicozy.dev/hub/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip");
        assert!(b.update_feed[2].starts_with("https://razdor.indicozy.dev/hub/"), "the own copy is the feed's last");
        assert!(b.hub.iter().all(|u| !u.contains("gitverse")), "no GitVerse address in the built-ins");
    }

    #[test]
    fn the_api_list_comes_from_the_config_and_the_built_in_is_the_placeholder() {
        assert_eq!(Config::builtin().api, vec![API_BUILTIN.to_string()]);
        assert_eq!(API_BUILTIN, "https://razdor.indicozy.dev/api");
        assert!(!API_BUILTIN.ends_with('/'), "the sender joins a path that starts with /");
        let text = r#"{"schema":1,"api":["https://api2.example","http://no.example"]}"#;
        let c = Config::parse(text.as_bytes()).unwrap();
        assert_eq!(c.api, vec!["https://api2.example".to_string()]);
        let none = r#"{"schema":1,"api":["http://no.example"]}"#;
        assert_eq!(Config::parse(none.as_bytes()).unwrap().api, Config::builtin().api);
    }

    #[test]
    fn the_kill_switch_is_read_and_defaults_to_on() {
        assert!(!Config::builtin().telemetry, "sending stays off until the remote config turns it on");
        assert!(!Config::parse(br#"{"schema":1}"#).unwrap().telemetry);
        assert!(!Config::parse(br#"{"schema":1,"telemetry":false}"#).unwrap().telemetry);
        assert!(Config::parse(br#"{"schema":1,"telemetry":true}"#).unwrap().telemetry);
    }

    #[test]
    fn the_cloud_cap_per_device_defaults_to_100_and_is_clamped() {
        assert_eq!(Config::builtin().cloud_saves_per_device, 100);
        assert_eq!(Config::parse(br#"{"schema":1}"#).unwrap().cloud_saves_per_device, 100);
        assert_eq!(Config::parse(br#"{"schema":1,"cloud_saves_per_device":40}"#).unwrap().cloud_saves_per_device, 40);
        assert_eq!(Config::parse(br#"{"schema":1,"cloud_saves_per_device":0}"#).unwrap().cloud_saves_per_device, 1);
        assert_eq!(Config::parse(br#"{"schema":1,"cloud_saves_per_device":5000}"#).unwrap().cloud_saves_per_device, 100);
    }

    #[test]
    fn the_cloud_switch_is_read_and_defaults_to_off() {
        assert!(!Config::builtin().cloud, "the cloud stays hidden until the remote config turns it on");
        assert!(!Config::parse(br#"{"schema":1}"#).unwrap().cloud);
        assert!(!Config::parse(br#"{"schema":1,"cloud":false}"#).unwrap().cloud);
        assert!(!Config::parse(br#"{"schema":1,"cloud":null}"#).unwrap().cloud);
        assert!(Config::parse(br#"{"schema":1,"cloud":true}"#).unwrap().cloud);
        assert!(!Config::parse(br#"{"schema":1,"telemetry":true}"#).unwrap().cloud, "the two switches are separate");
        assert!(Config::parse(br#"{"schema":1,"cloud":"yes"}"#).is_err(), "a bad switch is a bad config, as telemetry's is");
    }

    #[test]
    fn the_shipped_config_is_a_good_config() {
        let text = include_bytes!("../data/remote/config.json");
        let c = Config::parse(text).unwrap();
        // The switches the owner turns on in the shipped file (the cloud and the analytics, 2026-11-23) stay off
        // in the built-in config: a game that cannot fetch the config sends nothing. The rest must match.
        let mut want = Config::builtin();
        want.telemetry = c.telemetry;
        want.cloud = c.cloud;
        want.holiday = c.holiday;
        assert_eq!(c, want, "the shipped config matches the built-in one apart from the owner's switches");
    }

    /// A fake network's answer: the bytes, or why not.
    type Reply = Result<Vec<u8>, String>;
    /// The addresses a fake network was asked, in order.
    type Asked = std::rc::Rc<RefCell<Vec<String>>>;

    /// A fake network: each address gives its reply (or why not), and the addresses asked are kept.
    fn net(replies: Vec<(&'static str, Reply)>) -> (impl FnMut(&str) -> Reply, Asked) {
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
    fn github_is_tried_first_and_its_valid_config_is_used_even_when_the_own_server_answers_too() {
        let (gh, own) = (ADDRESSES[0], ADDRESSES[1]);
        assert_eq!(mirror::Host::of(gh), Some(mirror::Host::GitHub), "the config's first address is GitHub's");
        assert_eq!(mirror::Host::of(own), Some(mirror::Host::Own));
        let asked = RefCell::new(Vec::new());
        let pick = resolve(
            &ADDRESSES,
            |url| {
                asked.borrow_mut().push(url.to_string());
                let body = if url == gh { GOOD.replace("game.zip", "github.zip") } else { GOOD.replace("game.zip", "own.zip") };
                Ok(body.into_bytes())
            },
            None,
        );
        assert_eq!(pick.config.game.name, "github.zip", "GitHub's config, not the own server's");
        assert_eq!(pick.from.as_deref(), Some(gh));
        assert_eq!(asked.borrow().as_slice(), &[gh.to_string()], "the own server is not asked when GitHub answers validly");
    }

    #[test]
    fn a_github_failure_or_web_page_falls_back_to_the_own_server_for_that_fetch() {
        let (gh, own) = (ADDRESSES[0], ADDRESSES[1]);
        let pick = resolve(&ADDRESSES, |url| if url == gh { Err("stalled".into()) } else { Ok(GOOD.as_bytes().to_vec()) }, None);
        assert_eq!(pick.from.as_deref(), Some(own), "GitHub stalled: the own server answers this fetch");
        let page = b"<!doctype html><html>Page not found</html>".to_vec();
        let pick = resolve(&ADDRESSES, |url| if url == gh { Ok(page.clone()) } else { Ok(GOOD.as_bytes().to_vec()) }, None);
        assert_eq!(pick.from.as_deref(), Some(own), "a web page is no config");
    }

    #[test]
    fn a_failed_fetch_uses_the_cache_and_names_no_address() {
        let cached = GOOD.replace("game.zip", "old.zip");
        let pick = resolve(&ADDRESSES, |_| Err("stalled".into()), Some(cached.as_bytes()));
        assert_eq!(pick.config.game.name, "old.zip");
        assert_eq!(pick.from, None, "nothing fresh, so no address");
        let none = resolve(&ADDRESSES, |_| Err("stalled".into()), None);
        assert_eq!(none.config, Config::builtin());
    }

    #[test]
    fn the_consumer_lists_keep_the_configs_order_github_first_and_the_own_copy_second() {
        let c = Config::builtin();
        assert!(mirror::order(&c.update_feed)[0].starts_with("https://indicozy.github.io/"));
        assert!(mirror::order(&c.hub)[1].starts_with("https://razdor.indicozy.dev/hub/"));
        assert!(mirror::order(&c.hub)[0].starts_with("https://github.com/"));
        assert!(mirror::order(&c.game.links)[0].starts_with("https://github.com/"));
        assert_eq!(mirror::order(&c.hub), refs(&c.hub), "nothing is learned: the config's order");
    }

    #[test]
    fn before_start_the_built_in_config_is_used() {
        assert_eq!(update_feed(), Config::builtin().update_feed);
        assert_eq!(hub(), Config::builtin().hub);
        assert_eq!(game_info(), Config::builtin().game);
    }
}
