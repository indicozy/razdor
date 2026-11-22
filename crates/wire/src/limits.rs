//! The limits both sides enforce. Sizes are in bytes unless a name says otherwise; text limits
//! count characters. The rates are the automatic-ban rules of the analytics spec (per install id
//! and per IP, enforced by the server; the crate only states them).

// Identities.

/// An install id: 128 random bits, lowercase hex.
pub const INSTALL_ID_LEN: usize = 32;
/// A cloud account key: a secret of at least 128 bits, lowercase hex, at most 256 bits.
pub const ACCOUNT_KEY_MIN_LEN: usize = 32;
pub const ACCOUNT_KEY_MAX_LEN: usize = 64;
/// A link code's characters: digits 2-9 and 23 letters (no 0, O, 1, I, L: a code is read and typed
/// from a screen). 8 characters, valid 10 minutes, one use: the server rate-limits the redeems.
pub const LINK_ALPHABET: &str = "23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// A link code is valid this many seconds after it is issued, and works once.
pub const LINK_TTL_SECS: u64 = 600;

// Text.

/// A short field: a name, a platform, a device, a path-free key.
pub const MAX_TEXT: usize = 256;
/// A key (a map's or a mod's name, a setting's name, a save's id).
pub const MAX_KEY: usize = 128;
/// A version string.
pub const MAX_VERSION: usize = 32;
/// A save's display name (the player's own text).
pub const MAX_SAVE_NAME: usize = 200;
/// A setting's value (a catalog key, a number, a bool as text).
pub const MAX_SETTING_VALUE: usize = 512;
/// One line of a crash report's log.
pub const MAX_LOG_LINE_CHARS: usize = 2000;
/// A crash's panic text with its backtrace.
pub const MAX_PANIC_CHARS: usize = 64 * 1024;

// Counts.

/// Items in a short list (experiments, a battle's tags, the settings a change names).
pub const MAX_LIST: usize = 64;
/// Entries in a settings map (game settings sent with a report, the synced settings).
pub const MAX_SETTINGS: usize = 256;
/// Events in one batch.
pub const MAX_EVENTS_PER_BATCH: usize = 500;
/// The top profiler parts in a performance report.
pub const MAX_PROFILE_PARTS: usize = 5;
/// Screens and load times in a performance report.
pub const MAX_SCREENS: usize = 16;
pub const MAX_LOADS: usize = 32;
/// Log lines in a crash report.
pub const MAX_LOG_LINES: usize = 200;
/// Battle tags in one record (autobattle, reload).
pub const MAX_BATTLE_TAGS: usize = 8;
/// Maps in a marks document (every map the player has finished).
pub const MAX_MARK_MAPS: usize = 5000;
/// Hub items installed on one device.
pub const MAX_HUB_ITEMS: usize = 1000;
/// Scenes in a benchmark report.
pub const MAX_BENCH_SCENES: usize = 32;
/// Saves per account in a list response (3 + 3 + 1 per map, over a few hundred maps).
pub const MAX_SAVES_LISTED: usize = 10_000;

// Analytics: the automatic-ban rules (`2026-11-22-analytics-design.md`, Abuse).

/// Pings per install in a day the server accepts before it answers 429 (an honest client sends one a
/// UTC day, plus its queued backlog after an outage: a few dozen at most).
pub const PINGS_PER_DAY: u32 = 100;
/// Events per install per hour.
pub const EVENTS_PER_HOUR: u32 = 1000;
/// Battle records per install per hour.
pub const BATTLES_PER_HOUR: u32 = 60;
/// A share of invalid data over which an install is banned (percent).
pub const INVALID_PERCENT: u32 = 20;
/// Bad requests in a row over which an install is banned.
pub const BAD_REQUESTS_IN_A_ROW: u32 = 10;
/// The first and the second ban in seconds (24 h, then 7 days); the third is for good.
pub const BAN_FIRST_SECS: u64 = 24 * 3600;
pub const BAN_SECOND_SECS: u64 = 7 * 24 * 3600;
/// An IP ban for gross abuse only lasts at most this long (many players share one IP behind CGNAT).
pub const IP_BAN_MAX_SECS: u64 = 24 * 3600;

// Client queue and cloud quota.

/// The game's on-disk queue of unsent analytics: the oldest entries are dropped above this.
pub const CLIENT_QUEUE_BYTES: u64 = 5 * 1024 * 1024;
/// The cloud quota per account, counted after zstd compression on the client.
pub const CLOUD_BYTES_PER_ACCOUNT: u64 = 20 * 1024 * 1024;
/// Cloud saves a device keeps (its mirror's cap; the remote config's `cloud_saves_per_device` default,
/// and the most a device may ask for). The account keeps four devices' worth, as a safety net.
pub const SAVES_PER_DEVICE: usize = 100;
pub const SAVES_PER_ACCOUNT: usize = 4 * SAVES_PER_DEVICE;

// Body sizes, per endpoint.

/// Small JSON bodies: the ping, the account, a link, the install delete.
pub const MAX_SMALL_BODY: usize = 4 * 1024;
pub const MAX_EVENT_BODY: usize = 256 * 1024;
pub const MAX_PERF_BODY: usize = 64 * 1024;
pub const MAX_CRASH_BODY: usize = 512 * 1024;
/// A battle record (framed: its header plus the record's bytes).
pub const MAX_BATTLE_BODY: usize = 4 * 1024 * 1024;
/// A save file (framed). Under the account quota by a wide margin.
pub const MAX_SAVE_BODY: usize = 8 * 1024 * 1024;
pub const MAX_MARKS_BODY: usize = 256 * 1024;
pub const MAX_SETTINGS_BODY: usize = 64 * 1024;
pub const MAX_HUB_BODY: usize = 64 * 1024;
pub const MAX_BENCH_BODY: usize = 128 * 1024;
/// A mark's JSON value (the game's own `RunMark`), per map.
pub const MAX_MARK_VALUE_BYTES: usize = 4 * 1024;
/// The JSON header of a framed body ([`crate::frame`]).
pub const MAX_FRAME_HEADER: usize = 64 * 1024;

// The limits must agree with each other: checked when the crate compiles.
const _: () = assert!(MAX_SAVE_BODY as u64 <= CLOUD_BYTES_PER_ACCOUNT);
const _: () = assert!(MAX_PROFILE_PARTS == 5, "the spec's top five");
const _: () = assert!(LINK_ALPHABET.len() == 31);
