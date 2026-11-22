//! The server's settings: an optional TOML file, then environment overrides (`RAZDOR_*`).
//! Secrets are never in git: the admin password is an argon2 hash, the admin token its SHA-256.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use razdor_wire::limits;
use serde::Deserialize;

/// The rate and ban numbers. The per-install and per-IP request rates are the server's own (the wire
/// crate states only the spec's ban rules). Every field can be overridden in the TOML `[limits]` table.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct Limits {
    /// Requests per IP per minute (generous: many players share one IP behind CGNAT).
    pub ip_per_minute: u32,
    /// Minutes over the IP limit in an hour before the IP is banned.
    pub ip_flood_strikes: u32,
    /// Junk requests (not JSON, no install id, unknown framing) per IP per ten minutes before a ban.
    pub junk_per_ten_minutes: u32,
    /// Requests per install id per minute (a 429, not a ban).
    pub install_per_minute: u32,
    /// Requests per account key per minute (a 429).
    pub account_per_minute: u32,
    /// Link redeems per IP per minute (a 429); wrong codes count too.
    pub redeem_per_ip_per_minute: u32,
    /// New cloud accounts per IP per day (a 429).
    pub accounts_per_ip_per_day: u32,
    /// Admin logins per IP per ten minutes (a 429).
    pub login_per_ip_per_ten_minutes: u32,
    pub pings_per_day: u32,
    pub events_per_hour: u32,
    pub battles_per_hour: u32,
    /// Share of invalid items (percent) over an hour that bans an install.
    pub invalid_percent: u32,
    /// Bad requests in a row that ban an install.
    pub bad_in_a_row: u32,
    /// Saves an account may keep (all maps and kinds); a save past it is a 409, nothing is deleted.
    pub saves_per_account: u32,
    /// Crash reports per install per UTC day; past it a 429 (no ban).
    pub crash_per_day: u32,
    /// Performance reports per install per UTC day; past it a 429 (no ban).
    pub perf_per_day: u32,
    /// Bytes of the stored files (battles, crash reports and saves, the cloud's saves). Past it: no new
    /// uploads (507 for saves and accounts, 429 for the analytics writes). The databases are not counted.
    pub max_data_bytes: u64,
    /// Free bytes the disk must keep (statvfs). Under it: the same refusals as `max_data_bytes`.
    pub min_free_bytes: u64,
    /// Accounts idle for this many days are deleted with their files (0: never).
    pub account_expiry_days: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            ip_per_minute: 1200,
            ip_flood_strikes: 5,
            junk_per_ten_minutes: 300,
            install_per_minute: 300,
            account_per_minute: 120,
            redeem_per_ip_per_minute: 10,
            accounts_per_ip_per_day: 5,
            login_per_ip_per_ten_minutes: 10,
            pings_per_day: limits::PINGS_PER_DAY,
            events_per_hour: limits::EVENTS_PER_HOUR,
            battles_per_hour: limits::BATTLES_PER_HOUR,
            invalid_percent: limits::INVALID_PERCENT,
            bad_in_a_row: limits::BAD_REQUESTS_IN_A_ROW,
            saves_per_account: 160,
            crash_per_day: 10,
            perf_per_day: 48,
            max_data_bytes: 20 << 30,
            min_free_bytes: 2 << 30,
            account_expiry_days: 90,
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Where the databases and the zstd files live (`stats.sqlite`, `cloud.sqlite`, `battles/`, ...).
    pub data_dir: PathBuf,
    /// The address the service listens on (behind nginx on the same host).
    pub bind: SocketAddr,
    /// Read the client's address from `X-Real-IP` (set by nginx). Only true behind a proxy.
    pub trust_proxy: bool,
    /// Mark the admin cookie `Secure` (true over HTTPS; false only for a local test).
    pub secure_cookies: bool,
    /// The admin dashboard's password as an argon2 PHC string (`razdor-server hash-password`).
    pub admin_password_hash: Option<String>,
    /// SHA-256 (hex) of the admin token the verifier and the digest send (`razdor-server token-hash`).
    pub admin_token_sha256: Option<String>,
    /// If set, only these versions are accepted (exact strings). Empty: any `X.Y.Z` release.
    pub allowed_versions: Vec<String>,
    pub limits: Limits,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            data_dir: PathBuf::from("data"),
            bind: "127.0.0.1:8080".parse().expect("a valid default address"),
            trust_proxy: false,
            secure_cookies: true,
            admin_password_hash: None,
            admin_token_sha256: None,
            allowed_versions: Vec::new(),
            limits: Limits::default(),
        }
    }
}

impl Config {
    /// Reads the TOML file (if any), then applies the environment. `env` is injected for tests.
    pub fn load(file: Option<&Path>, env: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let mut cfg = match file {
            Some(path) => {
                let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
                toml::from_str::<Config>(&text).map_err(|e| format!("{}: {e}", path.display()))?
            }
            None => Config::default(),
        };
        if let Some(v) = env("RAZDOR_DATA_DIR") {
            cfg.data_dir = PathBuf::from(v);
        }
        if let Some(v) = env("RAZDOR_BIND") {
            cfg.bind = v.parse().map_err(|e| format!("RAZDOR_BIND: {e}"))?;
        }
        if let Some(v) = env("RAZDOR_TRUST_PROXY") {
            cfg.trust_proxy = v == "1" || v.eq_ignore_ascii_case("true");
        }
        if let Some(v) = env("RAZDOR_SECURE_COOKIES") {
            cfg.secure_cookies = !(v == "0" || v.eq_ignore_ascii_case("false"));
        }
        if let Some(v) = env("RAZDOR_ADMIN_PASSWORD_HASH") {
            cfg.admin_password_hash = Some(v);
        }
        if let Some(v) = env("RAZDOR_ADMIN_TOKEN_SHA256") {
            cfg.admin_token_sha256 = Some(v);
        }
        if let Some(v) = env("RAZDOR_ALLOWED_VERSIONS") {
            cfg.allowed_versions = v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
        }
        cfg.validate()?;
        Ok(cfg)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(h) = &self.admin_token_sha256 {
            if h.len() != 64 || !h.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err("admin_token_sha256 must be 64 lowercase hex characters".into());
            }
        }
        if self.limits.ip_per_minute == 0 || self.limits.install_per_minute == 0 {
            return Err("the request limits must be above zero".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_overrides_defaults() {
        let env = |k: &str| match k {
            "RAZDOR_BIND" => Some("0.0.0.0:9000".to_string()),
            "RAZDOR_ALLOWED_VERSIONS" => Some("1.5.4, 1.5.5".to_string()),
            "RAZDOR_TRUST_PROXY" => Some("1".to_string()),
            _ => None,
        };
        let cfg = Config::load(None, env).expect("loads");
        assert_eq!(cfg.bind.port(), 9000);
        assert_eq!(cfg.allowed_versions, vec!["1.5.4", "1.5.5"]);
        assert!(cfg.trust_proxy);
    }

    #[test]
    fn bad_token_hash_is_refused() {
        let env = |k: &str| (k == "RAZDOR_ADMIN_TOKEN_SHA256").then(|| "XYZ".to_string());
        assert!(Config::load(None, env).is_err());
    }

    #[test]
    fn toml_limits_override_one_number() {
        let cfg: Config = toml::from_str("[limits]\npings_per_day = 5\n").expect("parses");
        assert_eq!(cfg.limits.pings_per_day, 5);
        assert_eq!(cfg.limits.events_per_hour, limits::EVENTS_PER_HOUR);
    }
}
