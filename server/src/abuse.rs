//! Abuse: the rate windows (in memory), the automatic bans (in `stats.sqlite`) and the quarantine.
//!
//! Bans (`2026-11-22-analytics-design.md`, Abuse):
//! - an install id: 24 h, then 7 days, then for good; what it sent since the offence window opened
//!   is quarantined (kept, never counted). Triggers: over the ping, event or battle limit of an hour
//!   or a day, overlapping battles, ten bad requests in a row, or over 20% invalid items in an hour.
//! - an IP (hashed with a salt, never stored raw): 1 h, doubling up to 24 h. Triggers: a flood (over
//!   the request limit in five minutes of an hour) or junk (too many bodies that are not our JSON).
//!
//! A banned sender gets `ApiError::Silent`. The windows live in memory: a restart forgets them (the
//! bans themselves persist).

use std::collections::HashMap;
use std::net::IpAddr;

use razdor_wire::limits;
use rusqlite::{params, OptionalExtension};

use crate::civil::DAY;
use crate::http::ApiError;
use crate::state::{sha256_hex, AppState};

/// Each rate map holds at most this many windows; the oldest are evicted past it (memory bound).
pub const MAX_WINDOWS: usize = 100_000;
/// A ban's strike count restarts after this long without a new IP ban (strikes decay).
pub const IP_STRIKE_DECAY_SECS: i64 = 7 * DAY;
/// The windows are pruned at most this often (the request path checks it).
const PRUNE_EVERY_SECS: i64 = 60;

/// The address a rate window is keyed by: an IPv6 address is cut to its /64 (one household or one
/// carrier's customer), an IPv4 address stays whole.
pub fn window_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(_) => ip,
        IpAddr::V6(v6) => {
            let mut o = v6.octets();
            o[8..].fill(0);
            IpAddr::V6(std::net::Ipv6Addr::from(o))
        }
    }
}

/// Keeps at most `max` entries of `m`: when over, drops the oldest (by `ts`) down to 90% of `max`.
pub fn bound<K: Clone + Eq + std::hash::Hash, V>(m: &mut HashMap<K, V>, max: usize, ts: impl Fn(&V) -> i64) {
    if m.len() <= max {
        return;
    }
    let mut by_age: Vec<(i64, K)> = m.iter().map(|(k, v)| (ts(v), k.clone())).collect();
    by_age.sort_by_key(|(t, _)| *t);
    let drop = m.len() - max * 9 / 10;
    for (_, k) in by_age.into_iter().take(drop) {
        m.remove(&k);
    }
}

/// The strikes of a new IP ban: one more than the last one's, unless the last one is older than
/// the decay (then it starts again at one).
pub fn next_ip_strikes(prev: Option<(i64, i64)>, now: i64) -> i64 {
    match prev {
        Some((strikes, created)) if now - created < IP_STRIKE_DECAY_SECS => strikes + 1,
        _ => 1,
    }
}

/// A fixed window: `(start, count)`. Returns the count in the current window.
fn bump(w: &mut (i64, u32), now: i64, window: i64) -> u32 {
    if now - w.0 >= window {
        *w = (now, 0);
    }
    w.1 += 1;
    w.1
}

#[derive(Default)]
pub struct IpWin {
    minute: (i64, u32),
    strike_window: i64,
    strikes: u32,
    junk: (i64, u32),
}

#[derive(Default)]
pub struct InstallWin {
    minute: (i64, u32),
    /// Start of the hour the invalid-share check counts over.
    window: i64,
    total: u32,
    invalid: u32,
    bad_run: u32,
    bad_since: i64,
    /// Items refused for volume in the hour that `refused_window` starts.
    refused_window: i64,
    refused: u32,
}

#[derive(Default)]
pub struct Abuse {
    ips: HashMap<IpAddr, IpWin>,
    installs: HashMap<String, InstallWin>,
    accounts: HashMap<String, (i64, u32)>,
    redeems: HashMap<IpAddr, (i64, u32)>,
    logins: HashMap<IpAddr, (i64, u32)>,
    creates: HashMap<IpAddr, (i64, u32)>,
    last_prune: i64,
}

impl Abuse {
    /// Drops the windows nobody has used for two hours and bounds every map. Called by the retention
    /// job, and by the request path at most once a minute ([`Abuse::maybe_prune`]).
    pub fn prune(&mut self, now: i64) {
        let old = now - 7200;
        self.last_prune = now;
        self.ips.retain(|_, w| w.minute.0 > old || w.junk.0 > old || w.strike_window > old);
        self.installs.retain(|_, w| w.minute.0 > old || w.window > old);
        self.accounts.retain(|_, w| w.0 > old);
        self.redeems.retain(|_, w| w.0 > old);
        self.logins.retain(|_, w| w.0 > old);
        self.creates.retain(|_, w| w.0 > now - DAY);
        bound(&mut self.creates, MAX_WINDOWS, |w| w.0);
        bound(&mut self.ips, MAX_WINDOWS, |w| w.minute.0.max(w.junk.0).max(w.strike_window));
        bound(&mut self.installs, MAX_WINDOWS, |w| w.minute.0.max(w.window));
        bound(&mut self.accounts, MAX_WINDOWS, |w| w.0);
        bound(&mut self.redeems, MAX_WINDOWS, |w| w.0);
        bound(&mut self.logins, MAX_WINDOWS, |w| w.0);
    }

    pub fn maybe_prune(&mut self, now: i64) {
        if now - self.last_prune >= PRUNE_EVERY_SECS {
            self.prune(now);
        }
    }
}

/// The salted hash of an IP's window key (the ban key): never the raw address.
pub fn ip_key(st: &AppState, ip: IpAddr) -> String {
    sha256_hex(&[st.ip_salt.as_bytes(), window_ip(ip).to_string().as_bytes()])
}

/// Whether a ban of `kind` on `key` is active now.
pub async fn is_banned(st: &AppState, kind: &'static str, key: &str) -> Result<bool, ApiError> {
    let now = st.clock.now();
    let key = key.to_string();
    st.stats
        .call(move |c| {
            let until: Option<i64> = c
                .query_row("SELECT until FROM bans WHERE kind = ?1 AND key = ?2", params![kind, key], |r| r.get(0))
                .optional()?;
            Ok(until.is_some_and(|u| u > now))
        })
        .await
}

/// The request-level gate of an IP: a ban (silent), a flood (429, and a strike a minute).
pub async fn ip_gate(st: &AppState, ip: IpAddr) -> Result<(), ApiError> {
    let now = st.clock.now();
    let key = ip_key(st, ip);
    if is_banned(st, "ip", &key).await? {
        return Err(ApiError::Silent);
    }
    let limit = st.cfg.limits.ip_per_minute;
    let strikes_to_ban = st.cfg.limits.ip_flood_strikes;
    let flood = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.ips.entry(window_ip(ip)).or_default();
        let n = bump(&mut w.minute, now, 60);
        if n <= limit {
            None
        } else {
            if n == limit + 1 {
                if now - w.strike_window >= 3600 {
                    w.strike_window = now;
                    w.strikes = 0;
                }
                w.strikes += 1;
            }
            Some(w.strikes >= strikes_to_ban)
        }
    };
    match flood {
        None => Ok(()),
        Some(false) => Err(ApiError::Limited),
        Some(true) => {
            ban_ip(st, key, "request flood", now).await?;
            Err(ApiError::Silent)
        }
    }
}

/// Counts a junk body from an IP (not JSON, no install id). Returns the answer: a 400, or a silent
/// 200 when this junk brought the ban.
pub async fn junk(st: &AppState, ip: IpAddr, msg: &str) -> ApiError {
    let now = st.clock.now();
    let limit = st.cfg.limits.junk_per_ten_minutes;
    let banned = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.ips.entry(window_ip(ip)).or_default();
        if now - w.junk.0 >= 600 {
            w.junk = (now, 0);
        }
        w.junk.1 += 1;
        if w.junk.1 > limit {
            w.junk = (now, 0);
            true
        } else {
            false
        }
    };
    if !banned {
        return ApiError::BadRequest(msg.to_string());
    }
    match ban_ip(st, ip_key(st, ip), "junk requests", now).await {
        Ok(()) => ApiError::Silent,
        Err(e) => e,
    }
}

/// The install's gate: a ban (silent, unless `allow_banned`: deleting one's data always works),
/// then the per-minute rate (429, no ban).
pub async fn install_gate(st: &AppState, install: &str, allow_banned: bool) -> Result<(), ApiError> {
    if !allow_banned && is_banned(st, "install", install).await? {
        return Err(ApiError::Silent);
    }
    let now = st.clock.now();
    let limit = st.cfg.limits.install_per_minute;
    let n = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.installs.entry(install.to_string()).or_default();
        bump(&mut w.minute, now, 60)
    };
    if n > limit {
        Err(ApiError::Limited)
    } else {
        Ok(())
    }
}

/// Counts an install's items: `valid` accepted, `invalid` refused. `whole_bad` marks a request that
/// was refused as a whole (it counts toward the ten-in-a-row rule). Bans the install when a rule trips.
pub async fn tally(st: &AppState, install: &str, valid: u32, invalid: u32, whole_bad: bool) -> Result<(), ApiError> {
    let now = st.clock.now();
    let bad_in_a_row = st.cfg.limits.bad_in_a_row;
    let invalid_percent = st.cfg.limits.invalid_percent;
    let decision = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.installs.entry(install.to_string()).or_default();
        if now - w.window >= 3600 {
            w.window = now;
            w.total = 0;
            w.invalid = 0;
        }
        w.total += valid + invalid;
        w.invalid += invalid;
        if whole_bad {
            if w.bad_run == 0 {
                w.bad_since = now;
            }
            w.bad_run += 1;
        } else if invalid == 0 {
            w.bad_run = 0;
        }
        if whole_bad && w.bad_run >= bad_in_a_row {
            Some((w.bad_since, "ten bad requests in a row"))
        } else if w.total >= 10 && u64::from(w.invalid) * 100 > u64::from(w.total) * u64::from(invalid_percent) {
            Some((w.window, "over 20% of the data invalid in an hour"))
        } else {
            None
        }
    };
    match decision {
        None => Ok(()),
        Some((since, reason)) => {
            ban_install(st, install, reason, since).await?;
            Err(ApiError::Silent)
        }
    }
}

/// A refused request of an install: the 400, or a silent 200 when this refusal brought a ban.
pub async fn reject(st: &AppState, install: &str, items: u32, msg: String) -> ApiError {
    match tally(st, install, 0, items.max(1), true).await {
        Err(e) => e,
        Ok(()) => ApiError::BadRequest(msg),
    }
}

/// Per-account and per-IP windows of the cloud (429s, no bans).
pub fn account_gate(st: &AppState, key_hash: &str) -> Result<(), ApiError> {
    let now = st.clock.now();
    let limit = st.cfg.limits.account_per_minute;
    let n = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.accounts.entry(key_hash.to_string()).or_insert((0, 0));
        bump(w, now, 60)
    };
    if n > limit {
        Err(ApiError::Limited)
    } else {
        Ok(())
    }
}

/// New cloud accounts per IP per day (a 429). Free accounts cost disk, so one address may open only
/// a few; the window is a day, so a carrier's shared address still fits a household's worth.
pub async fn account_create_gate(st: &AppState, ip: IpAddr) -> Result<(), ApiError> {
    let now = st.clock.now();
    let limit = st.cfg.limits.accounts_per_ip_per_day;
    let n = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.creates.entry(window_ip(ip)).or_insert((0, 0));
        bump(w, now, DAY)
    };
    if n > limit {
        Err(ApiError::Limited)
    } else {
        Ok(())
    }
}

/// Counts the items of an install that were refused for volume (over the honest limit: a 429, nothing
/// stored) in the hour. True when they are a gross volume (over `gross` in an hour): the caller bans.
pub fn gross_refusals(st: &AppState, install: &str, items: u32, gross: u32) -> bool {
    let now = st.clock.now();
    let mut a = st.abuse();
    a.maybe_prune(now);
    let w = a.installs.entry(install.to_string()).or_default();
    if now - w.refused_window >= 3600 {
        w.refused_window = now;
        w.refused = 0;
    }
    w.refused = w.refused.saturating_add(items);
    w.refused > gross
}

pub fn redeem_gate(st: &AppState, ip: IpAddr) -> Result<(), ApiError> {
    let now = st.clock.now();
    let limit = st.cfg.limits.redeem_per_ip_per_minute;
    let n = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.redeems.entry(window_ip(ip)).or_insert((0, 0));
        bump(w, now, 60)
    };
    if n > limit {
        Err(ApiError::Limited)
    } else {
        Ok(())
    }
}

pub fn login_gate(st: &AppState, ip: IpAddr) -> Result<(), ApiError> {
    let now = st.clock.now();
    let limit = st.cfg.limits.login_per_ip_per_ten_minutes;
    let n = {
        let mut a = st.abuse();
        a.maybe_prune(now);
        let w = a.logins.entry(window_ip(ip)).or_insert((0, 0));
        bump(w, now, 600)
    };
    if n > limit {
        Err(ApiError::Limited)
    } else {
        Ok(())
    }
}

/// The ban's length: the first 24 h, the second 7 days, then for good.
pub fn install_ban_until(strikes: i64, now: i64) -> i64 {
    match strikes {
        1 => now + DAY,
        2 => now + 7 * DAY,
        _ => i64::MAX,
    }
}

/// An IP's ban: 1 h, doubling with each strike, capped at 24 h.
pub fn ip_ban_secs(strikes: i64) -> i64 {
    (3600i64 << (strikes - 1).clamp(0, 5)).min(limits::IP_BAN_MAX_SECS as i64)
}

async fn ban_ip(st: &AppState, key: String, reason: &'static str, now: i64) -> Result<(), ApiError> {
    st.stats
        .call(move |c| {
            let tx = c.transaction()?;
            let prev: Option<(i64, i64)> = tx
                .query_row("SELECT strikes, created_at FROM bans WHERE kind = 'ip' AND key = ?1", params![key], |r| {
                    Ok((r.get(0)?, r.get(1)?))
                })
                .optional()?;
            let strikes = next_ip_strikes(prev, now);
            let until = now + ip_ban_secs(strikes);
            tx.execute(
                "INSERT INTO bans (kind, key, strikes, until, reason, created_at) VALUES ('ip', ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (kind, key) DO UPDATE SET strikes = excluded.strikes, until = excluded.until,
                 reason = excluded.reason, created_at = excluded.created_at",
                params![key, strikes, until, reason, now],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
}

/// Bans an install and quarantines its raw rows received since `since`. The strike count decides
/// the length (24 h, 7 days, for good).
pub async fn ban_install(st: &AppState, install: &str, reason: &str, since: i64) -> Result<(), ApiError> {
    let now = st.clock.now();
    let install = install.to_string();
    let reason = reason.to_string();
    st.stats
        .call(move |c| {
            let tx = c.transaction()?;
            let prev: Option<i64> = tx
                .query_row("SELECT strikes FROM bans WHERE kind = 'install' AND key = ?1", params![install], |r| {
                    r.get(0)
                })
                .optional()?;
            let strikes = prev.unwrap_or(0) + 1;
            let until = install_ban_until(strikes, now);
            tx.execute(
                "INSERT INTO bans (kind, key, strikes, until, reason, created_at) VALUES ('install', ?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT (kind, key) DO UPDATE SET strikes = excluded.strikes, until = excluded.until,
                 reason = excluded.reason, created_at = excluded.created_at",
                params![install, strikes, until, reason, now],
            )?;
            for table in ["pings", "events", "perf", "crashes", "battles"] {
                tx.execute(
                    &format!("UPDATE {table} SET quarantined = 1 WHERE install_id = ?1 AND received_at >= ?2"),
                    params![install, since],
                )?;
            }
            tx.commit()?;
            tracing::warn!("install banned ({reason}), strike {strikes}");
            Ok(())
        })
        .await
}

/// Lifts a ban now (the dashboard's unban). The strikes stay: the next offence is escalated.
pub async fn unban(st: &AppState, kind: &'static str, key: String) -> Result<bool, ApiError> {
    let now = st.clock.now();
    st.stats
        .call(move |c| {
            let n = c.execute("UPDATE bans SET until = ?1 WHERE kind = ?2 AND key = ?3", params![now, kind, key])?;
            Ok(n > 0)
        })
        .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalation_of_install_bans() {
        assert_eq!(install_ban_until(1, 100), 100 + DAY);
        assert_eq!(install_ban_until(2, 100), 100 + 7 * DAY);
        assert_eq!(install_ban_until(3, 100), i64::MAX);
    }

    #[test]
    fn ip_v6_windows_are_per_slash_64() {
        let a: IpAddr = "2001:db8:1:2:aaaa:bbbb:cccc:1".parse().unwrap();
        let b: IpAddr = "2001:db8:1:2:ffff::9".parse().unwrap();
        let c: IpAddr = "2001:db8:1:3::1".parse().unwrap();
        assert_eq!(window_ip(a), window_ip(b));
        assert_ne!(window_ip(a), window_ip(c));
        let v4: IpAddr = "203.0.113.7".parse().unwrap();
        assert_eq!(window_ip(v4), v4);
    }

    #[test]
    fn bounded_maps_drop_the_oldest() {
        let mut m: HashMap<u32, i64> = (0..100u32).map(|i| (i, i64::from(i))).collect();
        bound(&mut m, 50, |t| *t);
        assert!(m.len() <= 50 && m.len() >= 40, "down to 90% of the cap: {}", m.len());
        assert!(!m.contains_key(&0), "the oldest went first");
        assert!(m.contains_key(&99), "the newest stayed");
        let mut small: HashMap<u32, i64> = (0..5u32).map(|i| (i, 0)).collect();
        bound(&mut small, 50, |t| *t);
        assert_eq!(small.len(), 5, "under the cap nothing goes");
    }

    #[test]
    fn ip_strikes_decay_after_a_week() {
        assert_eq!(next_ip_strikes(None, 1000), 1);
        assert_eq!(next_ip_strikes(Some((3, 1000)), 1000 + DAY), 4, "a recent ban escalates");
        assert_eq!(next_ip_strikes(Some((3, 1000)), 1000 + IP_STRIKE_DECAY_SECS), 1, "an old one has decayed");
    }

    #[test]
    fn ip_bans_double_and_cap_at_a_day() {
        assert_eq!(ip_ban_secs(1), 3600);
        assert_eq!(ip_ban_secs(2), 7200);
        assert_eq!(ip_ban_secs(5), 57_600);
        assert_eq!(ip_ban_secs(6), 86_400);
        assert_eq!(ip_ban_secs(9), 86_400);
    }
}
