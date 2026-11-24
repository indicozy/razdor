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
-- Installs per country and day (the pings' country, folded with the rest; see `migrate_stats`).
CREATE TABLE IF NOT EXISTS daily_country (
    day INTEGER NOT NULL,
    country TEXT NOT NULL,
    installs INTEGER NOT NULL,
    PRIMARY KEY (day, country)
);
-- The own /hub/ downloads per day, file and country (`downloads::fold_dir`): whole files as a real number.
CREATE TABLE IF NOT EXISTS daily_hub (
    day INTEGER NOT NULL,
    file TEXT NOT NULL,
    country TEXT NOT NULL,
    requests INTEGER NOT NULL,
    bytes INTEGER NOT NULL,
    whole_files REAL NOT NULL,
    PRIMARY KEY (day, file, country)
);
-- GitHub's release asset counters as of the last poll (the baseline of the next one).
CREATE TABLE IF NOT EXISTS github_counts (
    repo TEXT NOT NULL,
    tag TEXT NOT NULL,
    name TEXT NOT NULL,
    count INTEGER NOT NULL,
    PRIMARY KEY (repo, tag, name)
);
-- GitHub's downloads per day: the differences between two polls.
CREATE TABLE IF NOT EXISTS daily_github (
    day INTEGER NOT NULL,
    repo TEXT NOT NULL,
    tag TEXT NOT NULL,
    name TEXT NOT NULL,
    downloads INTEGER NOT NULL,
    PRIMARY KEY (day, repo, tag, name)
);
";

/// The stats database's migrations, run after `STATS` (which creates the tables of a new file). An old
/// file has `pings` without `country`: the column is added, every old ping reads "unknown".
pub fn migrate_stats(c: &rusqlite::Connection) -> rusqlite::Result<()> {
    let has_country = c
        .prepare("SELECT 1 FROM pragma_table_info('pings') WHERE name = 'country'")?
        .exists([])?;
    if !has_country {
        c.execute_batch("ALTER TABLE pings ADD COLUMN country TEXT NOT NULL DEFAULT 'unknown';")?;
    }
    c.execute_batch("CREATE INDEX IF NOT EXISTS pings_country ON pings(day, country);")
}

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
    created_at INTEGER NOT NULL,
    device_id TEXT NOT NULL DEFAULT '',
    raw_sha256 TEXT NOT NULL DEFAULT ''
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
    // The saves of a device are counted per device (the cap). A file from before device ids gets the
    // two columns with empty defaults: those rows belong to no device, so no client deletes them.
    for (column, def) in [("device_id", "''"), ("raw_sha256", "''")] {
        let has: bool = c
            .prepare("SELECT 1 FROM pragma_table_info('saves') WHERE name = ?1")?
            .exists([column])?;
        if !has {
            c.execute_batch(&format!("ALTER TABLE saves ADD COLUMN {column} TEXT NOT NULL DEFAULT {def};"))?;
        }
    }
    c.execute_batch("CREATE INDEX IF NOT EXISTS saves_device ON saves(account_id, device_id);")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_old_pings_table_gains_the_country_and_keeps_its_rows() {
        let c = rusqlite::Connection::open_in_memory().expect("a database");
        // The table as it was before the country column existed.
        c.execute_batch(
            "CREATE TABLE pings (id INTEGER PRIMARY KEY, install_id TEXT NOT NULL, day INTEGER NOT NULL,
             received_at INTEGER NOT NULL, razdor TEXT NOT NULL, platform TEXT NOT NULL, game TEXT NOT NULL,
             lang TEXT NOT NULL, quarantined INTEGER NOT NULL DEFAULT 0);
             INSERT INTO pings (install_id, day, received_at, razdor, platform, game, lang)
             VALUES ('a', 1, 1, '1.6.0', 'linux', 'cu', 'ru');",
        )
        .expect("the old table");
        c.execute_batch(STATS).expect("the schema");
        migrate_stats(&c).expect("the migration");
        migrate_stats(&c).expect("a second run changes nothing");
        let old: String = c.query_row("SELECT country FROM pings", [], |r| r.get(0)).expect("the old row");
        assert_eq!(old, "unknown");
        c.execute(
            "INSERT INTO pings (install_id, day, received_at, razdor, platform, game, lang, country)
             VALUES ('b', 1, 1, '1.6.0', 'linux', 'cu', 'ru', 'DE')",
            [],
        )
        .expect("a new ping with a country");
        let n: i64 = c.query_row("SELECT COUNT(*) FROM pings WHERE country = 'DE'", [], |r| r.get(0)).expect("count");
        assert_eq!(n, 1);
    }
}
