//! `/etc/razdor-mirror.toml`: where the mirror writes, what it keeps and the disk limits.

use std::path::{Path, PathBuf};

use serde::Deserialize;

pub const GIB: u64 = 1024 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MirrorConfig {
    /// The web root nginx serves at `/hub/` (`/var/www/razdor-hub`).
    pub root: PathBuf,
    /// Where downloads wait until they are verified. Must be on the same filesystem as `root`.
    pub stage: PathBuf,
    /// The owner's own address for the mirror (`https://razdor.indicozy.dev/hub`), used for the
    /// own copies of the catalog's addresses.
    pub own_base: String,
    /// App releases kept (the newest published ones).
    pub keep_app_releases: usize,
    /// Catalogs kept: the current one and the previous one, so a client that read the old `index.json`
    /// just before a swap still finds its files.
    pub keep_catalogs: usize,
    /// The most the mirror may hold in `root`.
    pub max_bytes: u64,
    /// The free space `root`'s disk keeps after a download.
    pub min_free_bytes: u64,
    /// File-name globs (a `*` may stand anywhere) that are never mirrored.
    pub exclude: Vec<String>,
    /// Old fixed names (`razdor`, `Razdor.exe`, ...) are skipped when the release has versioned files.
    pub skip_unversioned: bool,
}

impl Default for MirrorConfig {
    fn default() -> Self {
        MirrorConfig {
            root: PathBuf::from("/var/www/razdor-hub"),
            stage: PathBuf::from("/var/lib/razdor-mirror/stage"),
            own_base: "https://razdor.indicozy.dev/hub".into(),
            keep_app_releases: 3,
            keep_catalogs: 2,
            max_bytes: 8 * GIB,
            min_free_bytes: 20 * GIB,
            exclude: vec!["razdor-translate*".into(), "Razdor-translate*".into(), "*.ipa".into()],
            skip_unversioned: true,
        }
    }
}

impl MirrorConfig {
    pub fn load(path: &Path) -> Result<MirrorConfig, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let cfg: MirrorConfig = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        if cfg.keep_app_releases == 0 || cfg.keep_catalogs == 0 {
            return Err("keep_app_releases and keep_catalogs must be at least 1".into());
        }
        if !cfg.own_base.starts_with("https://") {
            return Err("own_base must be an https address".into());
        }
        Ok(cfg)
    }
}

/// Matches a file name against a glob whose only wildcard is `*` (any run of characters, possibly empty).
pub fn glob_match(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let first = parts[0];
    let last = parts[parts.len() - 1];
    if !name.starts_with(first) || !name.ends_with(last) || name.len() < first.len() + last.len() {
        return false;
    }
    let mut rest = &name[first.len()..name.len() - last.len()];
    for mid in &parts[1..parts.len() - 1] {
        match rest.find(mid) {
            Some(i) => rest = &rest[i + mid.len()..],
            None => return false,
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs_match_prefix_suffix_and_exact_names() {
        assert!(glob_match("razdor-translate*", "razdor-translate-1.6.0-linux"));
        assert!(glob_match("*.ipa", "razdor-1.6.0.ipa"));
        assert!(!glob_match("*.ipa", "razdor-1.6.0.apk"));
        assert!(glob_match("razdor", "razdor"));
        assert!(!glob_match("razdor", "razdor.apk"));
        assert!(!glob_match("razdor-translate*", "razdor"));
    }

    #[test]
    fn the_defaults_keep_the_ads_zip_and_drop_translate_and_ipa() {
        let cfg = MirrorConfig::default();
        let kept = |n: &str| !cfg.exclude.iter().any(|p| glob_match(p, n));
        assert!(kept("razdor-ads-1.6.0.zip"));
        assert!(kept("razdor-1.6.0.apk"));
        assert!(!kept("razdor-translate-1.6.0-linux"));
        assert!(!kept("razdor-1.6.0.ipa"));
    }

    #[test]
    fn a_file_with_bad_keeps_is_refused() {
        let dir = std::env::temp_dir().join(format!("razdor-mirror-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.toml");
        std::fs::write(&path, "keep_catalogs = 0\n").unwrap();
        assert!(MirrorConfig::load(&path).is_err());
        std::fs::write(&path, "own_base = \"http://x\"\n").unwrap();
        assert!(MirrorConfig::load(&path).is_err());
        std::fs::write(&path, "keep_app_releases = 2\nbogus = 1\n").unwrap();
        assert!(MirrorConfig::load(&path).is_err(), "unknown keys are refused");
        std::fs::write(&path, "keep_app_releases = 2\n").unwrap();
        assert_eq!(MirrorConfig::load(&path).unwrap().keep_app_releases, 2);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
