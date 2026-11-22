//! The tables of the two databases. `CREATE ... IF NOT EXISTS`, so opening an old file is safe.
//! Days are integers (days since 1970, UTC). Raw rows carry `quarantined` (a banned sender's data
//! from the bad period): the dashboard and the aggregates skip those.

pub const STATS: &str = "
CREATE TABLE IF NOT EXISTS pings (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL,
    day INTEGER NOT NULL,
    received_at INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    platform TEXT NOT NULL,
    game TEXT NOT NULL,
    lang TEXT NOT NULL,
    quarantined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS pings_install ON pings(install_id, day);
CREATE INDEX IF NOT EXISTS pings_day ON pings(day);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    day INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    kind TEXT NOT NULL,
    map TEXT NOT NULL DEFAULT '',
    outcome TEXT NOT NULL DEFAULT '',
    last_screen TEXT NOT NULL DEFAULT '',
    payload TEXT NOT NULL,
    quarantined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS events_install ON events(install_id, received_at);
CREATE INDEX IF NOT EXISTS events_day ON events(day, kind);

CREATE TABLE IF NOT EXISTS perf (
    id INTEGER PRIMARY KEY,
    install_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    day INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    gpu TEXT NOT NULL,
    phone TEXT NOT NULL,
    screen TEXT NOT NULL,
    median_ms REAL NOT NULL,
    p99_ms REAL NOT NULL,
    payload TEXT NOT NULL,
    quarantined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS perf_install ON perf(install_id);
CREATE INDEX IF NOT EXISTS perf_day ON perf(day);

CREATE TABLE IF NOT EXISTS crashes (
    id TEXT PRIMARY KEY,
    install_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    day INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    platform TEXT NOT NULL,
    game TEXT NOT NULL,
    screen TEXT NOT NULL,
    panic_loc TEXT NOT NULL,
    panic_msg TEXT NOT NULL,
    has_save INTEGER NOT NULL DEFAULT 0,
    report_file TEXT NOT NULL,
    save_file TEXT,
    save_at INTEGER,
    quarantined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS crashes_install ON crashes(install_id);
CREATE INDEX IF NOT EXISTS crashes_group ON crashes(panic_loc, panic_msg);

CREATE TABLE IF NOT EXISTS battles (
    id TEXT PRIMARY KEY,
    install_id TEXT NOT NULL,
    received_at INTEGER NOT NULL,
    day INTEGER NOT NULL,
    started_at INTEGER NOT NULL,
    ended_at INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    platform TEXT NOT NULL,
    game TEXT NOT NULL,
    map TEXT NOT NULL,
    map_hash TEXT NOT NULL,
    mod_hash TEXT,
    rules INTEGER NOT NULL,
    experiments TEXT NOT NULL,
    tags TEXT NOT NULL,
    winner TEXT NOT NULL,
    turns INTEGER NOT NULL,
    player_losses INTEGER NOT NULL,
    foe_losses INTEGER NOT NULL,
    record_sha256 TEXT NOT NULL,
    record_len INTEGER NOT NULL,
    file TEXT NOT NULL,
    verdict TEXT,
    verdict_reason TEXT,
    quarantined INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS battles_install ON battles(install_id, received_at);
CREATE INDEX IF NOT EXISTS battles_time ON battles(install_id, started_at, ended_at);

CREATE TABLE IF NOT EXISTS bans (
    kind TEXT NOT NULL,
    key TEXT NOT NULL,
    strikes INTEGER NOT NULL,
    until INTEGER NOT NULL,
    reason TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    PRIMARY KEY (kind, key)
);

-- Aggregates of the raw rows older than their retention (written by the retention job).
CREATE TABLE IF NOT EXISTS daily_players (
    day INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    platform TEXT NOT NULL,
    game TEXT NOT NULL,
    lang TEXT NOT NULL,
    installs INTEGER NOT NULL,
    PRIMARY KEY (day, razdor, platform, game, lang)
);
CREATE TABLE IF NOT EXISTS daily_events (
    day INTEGER NOT NULL,
    kind TEXT NOT NULL,
    map TEXT NOT NULL,
    outcome TEXT NOT NULL,
    last_screen TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, kind, map, outcome, last_screen)
);
CREATE TABLE IF NOT EXISTS daily_perf (
    day INTEGER NOT NULL,
    gpu TEXT NOT NULL,
    phone TEXT NOT NULL,
    screen TEXT NOT NULL,
    sessions INTEGER NOT NULL,
    median_sum REAL NOT NULL,
    p99_sum REAL NOT NULL,
    PRIMARY KEY (day, gpu, phone, screen)
);
CREATE TABLE IF NOT EXISTS daily_cloud (
    day INTEGER NOT NULL,
    op TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (day, op)
);
";

pub const CLOUD: &str = "
CREATE TABLE IF NOT EXISTS accounts (
    id TEXT PRIMARY KEY,
    key_hash TEXT NOT NULL UNIQUE,
    created_at INTEGER NOT NULL,
    last_active INTEGER NOT NULL DEFAULT 0
);
-- A link code's key is kept here only until the code is redeemed or expires (at most 10 minutes).
CREATE TABLE IF NOT EXISTS links (
    code TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    expires_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS saves (
    id TEXT PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    game TEXT NOT NULL,
    map TEXT NOT NULL,
    kind TEXT NOT NULL,
    device TEXT NOT NULL,
    name TEXT NOT NULL,
    saved_at INTEGER NOT NULL,
    razdor TEXT NOT NULL,
    size INTEGER NOT NULL,
    sha256 TEXT NOT NULL,
    pinned INTEGER NOT NULL,
    created_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS saves_account ON saves(account_id, game, map, kind);
CREATE TABLE IF NOT EXISTS marks (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS hub (
    account_id TEXT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS bench (
    id INTEGER PRIMARY KEY,
    account_id TEXT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    at INTEGER NOT NULL,
    json TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS bench_account ON bench(account_id, at);
";

/// Start-up migration of the cloud database. A file from before `last_active` gets the column, and
/// its accounts count as used now (the expiry clock starts at the upgrade, not years earlier).
pub fn migrate_cloud(c: &rusqlite::Connection, now: i64) -> rusqlite::Result<()> {
    let has: bool = c
        .prepare("SELECT 1 FROM pragma_table_info('accounts') WHERE name = 'last_active'")?
        .exists([])?;
    if !has {
        c.execute_batch("ALTER TABLE accounts ADD COLUMN last_active INTEGER NOT NULL DEFAULT 0;")?;
    }
    c.execute("UPDATE accounts SET last_active = ?1 WHERE last_active = 0", rusqlite::params![now])?;
    Ok(())
}
