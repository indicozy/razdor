//! Versions of a content package (`kind = "package"`). Every installed version keeps its file: the
//! newest one is the record's own fields (`version`, `files`, `unchecked`, `entry`) and the others
//! are in `InstalledEntry::older`, newest first. `package_state.json`, next to `installed.json`,
//! remembers per package the version the player picked (the Hub card's and the new-game page's
//! dropdown, which is also the version last started) and the versions played.
//!
//! The "update" mark ([`Hub::has_update`]) is the one rule: an older version is installed and the
//! newest one has not been played. Several versions alone are not marked; the dropdown shows them.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{safe_path, Catalog, Entry, Hub, Installed, InstalledEntry, Kind};

/// The per-package state file, at the hub's root.
pub const STATE_FILE: &str = "package_state.json";

/// One installed version of a package: what the record keeps for it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageVersion {
    pub version: String,
    /// Relative to the hub folder.
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub unchecked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<Entry>,
}

/// What the player did with one package: the version picked (also the last one started) and the
/// versions started. A picked version that is not installed any more is not used.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageState {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub picked: Option<String>,
    #[serde(default)]
    pub played: Vec<String>,
}

/// The state of every package by id.
pub type PackageStates = BTreeMap<String, PackageState>;

/// The order of two package versions. Both are split at dots and dashes; the parts compare in turn:
/// digits as a number (a number beats text), text as text, and a version that is a prefix of
/// another comes first (`1.0` before `1.0.0`). Two different strings never compare equal, so the
/// order is total, and no version text can make it panic.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (pa, pb): (Vec<&str>, Vec<&str>) = (a.split(['.', '-']).collect(), b.split(['.', '-']).collect());
    for (x, y) in pa.iter().zip(&pb) {
        match part_order(x, y) {
            Ordering::Equal => {}
            other => return other,
        }
    }
    pa.len().cmp(&pb.len()).then_with(|| a.cmp(b))
}

fn part_order(a: &str, b: &str) -> Ordering {
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit());
    match (digits(a), digits(b)) {
        // Without leading zeros a longer number is the bigger one, however long the part is.
        (true, true) => {
            let (a, b) = (a.trim_start_matches('0'), b.trim_start_matches('0'));
            a.len().cmp(&b.len()).then_with(|| a.cmp(b))
        }
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => a.cmp(b),
    }
}

impl InstalledEntry {
    /// Every installed version of this record, newest first: the record's own, then `older`.
    pub fn versions(&self) -> Vec<PackageVersion> {
        let mut all = vec![self.top()];
        all.extend(self.older.iter().cloned());
        all.sort_by(|a, b| compare_versions(&b.version, &a.version));
        all
    }

    /// The version the record's own fields describe.
    fn top(&self) -> PackageVersion {
        PackageVersion { version: self.version.clone(), files: self.files.clone(), unchecked: self.unchecked, entry: self.entry.clone() }
    }
}

/// `new` recorded as one more version of the package whose record is `rec` (or as its first, when
/// `rec` is None). The newest version becomes the record's own fields and the others go to `older`,
/// newest first. A version the record holds already is replaced by `new`. The record's shared
/// fields (kind, lang, title, requires) are those of the newest version's record.
pub fn add_version(rec: Option<InstalledEntry>, new: InstalledEntry) -> InstalledEntry {
    let Some(rec) = rec else { return new };
    let mut all: Vec<PackageVersion> = rec.versions().into_iter().filter(|v| v.version != new.version).collect();
    all.push(new.top());
    all.sort_by(|a, b| compare_versions(&b.version, &a.version));
    let newest = all.remove(0);
    let base = if newest.version == new.version { new } else { rec };
    InstalledEntry { version: newest.version, files: newest.files, unchecked: newest.unchecked, entry: newest.entry, older: all, ..base }
}

/// The per-package state file's content (`package_state.json`); a missing or unreadable one is empty.
fn read_states(local: &Path) -> PackageStates {
    std::fs::read(local.join(STATE_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

impl Hub {
    fn package_states(&self) -> PackageStates {
        read_states(&self.local)
    }

    fn save_package_states(&self, states: &PackageStates) -> Result<(), String> {
        let json = serde_json::to_vec_pretty(states).map_err(|e| format!("{STATE_FILE}: {e}"))?;
        crate::fsutil::write_atomically(&self.local.join(STATE_FILE), &json).map_err(|e| format!("{STATE_FILE}: {e}"))
    }

    /// The installed versions of package `id`, newest first; empty when it is not an installed package.
    pub fn package_versions(&self, id: &str) -> Vec<String> {
        let installed = self.installed();
        match installed.entries.get(id).filter(|r| r.kind == Kind::Package) {
            Some(rec) => rec.versions().into_iter().map(|v| v.version).collect(),
            None => Vec::new(),
        }
    }

    /// The file of version `version` of installed package `id`, to play; None when that version is
    /// not installed.
    pub fn play_path_of(&self, id: &str, version: &str) -> Option<PathBuf> {
        let installed = self.installed();
        let rec = installed.entries.get(id).filter(|r| r.kind == Kind::Package)?;
        let v = rec.versions().into_iter().find(|v| v.version == version)?;
        let rel = v.files.first().filter(|r| safe_path(r))?;
        Some(self.local.join(rel))
    }

    /// The version a new game of package `id` starts on: the one picked (or last played) while it is
    /// installed, else the newest. None when the package is not installed.
    pub fn default_version(&self, id: &str) -> Option<String> {
        let versions = self.package_versions(id);
        let picked = self.package_states().get(id).and_then(|s| s.picked.clone()).filter(|p| versions.contains(p));
        picked.or_else(|| versions.into_iter().next())
    }

    /// Remembers `version` as the one the player picked for package `id` (the dropdown). It is not a
    /// play: the mark does not change.
    pub fn set_picked(&self, id: &str, version: &str) -> Result<(), String> {
        let mut states = self.package_states();
        states.entry(id.to_string()).or_default().picked = Some(version.to_string());
        self.save_package_states(&states)
    }

    /// The player started a new game of package `id` on `version`: it is picked from now on and
    /// counts as played. The mark goes only once the newest version is played.
    pub fn mark_played(&self, id: &str, version: &str) -> Result<(), String> {
        let mut states = self.package_states();
        let state = states.entry(id.to_string()).or_default();
        if !state.played.iter().any(|p| p == version) {
            state.played.push(version.to_string());
        }
        state.picked = Some(version.to_string());
        self.save_package_states(&states)
    }

    /// The "update" mark for package `id`: an older version is installed and its newest version has
    /// not been played yet.
    pub fn has_update(&self, id: &str) -> bool {
        let installed = self.installed();
        let Some(rec) = installed.entries.get(id).filter(|r| r.kind == Kind::Package && !r.older.is_empty()) else { return false };
        let newest = rec.versions().into_iter().next().map(|v| v.version).unwrap_or_default();
        let played = self.package_states().get(id).is_some_and(|s| s.played.contains(&newest));
        !played
    }

    /// The catalog's version of package `id` when the player has no installed version of it (the
    /// Hub card offers «Install X.Y.Z»); None otherwise.
    pub fn catalog_update(&self, catalog: &Catalog, id: &str) -> Option<String> {
        let e = catalog.entry(id).filter(|e| e.kind == Kind::Package)?;
        let versions = self.package_versions(id);
        if versions.is_empty() || versions.contains(&e.version) {
            None
        } else {
            Some(e.version.clone())
        }
    }

    /// Drops package `id`'s state (its removal).
    pub(super) fn forget_package_state(&self, id: &str) -> Result<(), String> {
        let mut states = self.package_states();
        if states.remove(id).is_some() {
            self.save_package_states(&states)
        } else {
            Ok(())
        }
    }

    /// Records `new` as a version of package `id` in `installed` (the caller saves the record). When a
    /// second version arrives and the package has no state yet, the versions it had count as played,
    /// so an existing player's single version never shows the mark.
    pub(super) fn add_package_version(&self, installed: &mut Installed, id: &str, new: InstalledEntry) -> Result<(), String> {
        if let Some(old) = installed.entries.get(id).filter(|r| r.kind == Kind::Package) {
            let known = old.versions().iter().any(|v| v.version == new.version);
            let mut states = self.package_states();
            if !known && !states.contains_key(id) {
                let state = states.entry(id.to_string()).or_default();
                state.played.extend(old.versions().into_iter().map(|v| v.version));
                self.save_package_states(&states)?;
            }
        }
        let record = add_version(installed.entries.get(id).cloned(), new);
        installed.entries.insert(id.to_string(), record);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        std::env::temp_dir().join(format!("razdor-hubversions-{tag}-{}-{nanos}", std::process::id()))
    }

    fn record(version: &str, files: &[&str]) -> InstalledEntry {
        InstalledEntry {
            version: version.into(),
            kind: Kind::Package,
            lang: String::new(),
            title: super::super::Text::default(),
            requires: vec![],
            files: files.iter().map(|f| f.to_string()).collect(),
            unchecked: false,
            entry: None,
            older: vec![],
        }
    }

    #[test]
    fn versions_order_numerically() {
        assert_eq!(compare_versions("0.10.0", "0.9.0"), Ordering::Greater);
        assert_eq!(compare_versions("1.0", "1.0.0"), Ordering::Less);
        assert_eq!(compare_versions("2", "1.0-beta"), Ordering::Greater);
        assert_eq!(compare_versions("1.0-beta", "1.0-alpha"), Ordering::Greater);
        assert_eq!(compare_versions("0.2.0", "0.2.0"), Ordering::Equal);
    }

    #[test]
    fn a_version_order_is_total_and_never_panics() {
        let all = ["", "0.1.0", "0.01.0", "0.1", "1.0", "1.0.0", "1..0", "1.0-beta", "1.0-alpha", "2", "10", "9", "x", "99999999999999999999999999.0", "v1.0.0"];
        for a in all {
            for b in all {
                let o = compare_versions(a, b);
                assert_eq!(o == Ordering::Equal, a == b, "{a} against {b}");
                assert_eq!(compare_versions(b, a), o.reverse(), "{a} against {b}");
                for c in all {
                    if o == Ordering::Greater && compare_versions(b, c) == Ordering::Greater {
                        assert_eq!(compare_versions(a, c), Ordering::Greater, "{a} > {b} > {c}");
                    }
                }
            }
        }
    }

    #[test]
    fn a_record_of_versions_that_are_not_x_y_z_lists_newest_first() {
        let mut rec = record("2", &["packages/a-2.rzpack"]);
        rec.older = vec![PackageVersion { version: "1.0-beta".into(), files: vec!["packages/a-1.0-beta.rzpack".into()], unchecked: false, entry: None }];
        let listed: Vec<String> = rec.versions().into_iter().map(|v| v.version).collect();
        assert_eq!(listed, vec!["2", "1.0-beta"]);
    }

    #[test]
    fn an_older_version_goes_below_and_the_newest_stays_on_top() {
        let top = add_version(None, record("0.2.0", &["p/0.2.0"]));
        let rec = add_version(Some(top), record("0.1.0", &["p/0.1.0"]));
        assert_eq!(rec.version, "0.2.0");
        assert_eq!(rec.files, vec!["p/0.2.0".to_string()]);
        assert_eq!(rec.older, vec![PackageVersion { version: "0.1.0".into(), files: vec!["p/0.1.0".into()], unchecked: false, entry: None }]);
        let again = add_version(Some(rec), record("0.1.0", &["p/0.1.0"]));
        assert_eq!(again.older.len(), 1, "a version the record holds is replaced, not doubled");
        let newer = add_version(Some(again), record("0.3.0", &["p/0.3.0"]));
        assert_eq!(newer.version, "0.3.0");
        let below: Vec<&str> = newer.older.iter().map(|v| v.version.as_str()).collect();
        assert_eq!(below, ["0.2.0", "0.1.0"]);
    }

    #[test]
    fn an_old_installed_json_loads_as_one_version() {
        let local = temp("old-json");
        std::fs::create_dir_all(&local).unwrap();
        let json = r#"{"entries":{"hello-story":{"version":"0.1.0","kind":"package","files":["packages/hello-story-0.1.0.rzpack"]}}}"#;
        std::fs::write(local.join("installed.json"), json).unwrap();
        let hub = Hub { source: super::super::Source::Mock, local: local.clone(), trust_dir: None };
        let installed = hub.installed();
        assert!(installed.entries["hello-story"].older.is_empty());
        assert_eq!(hub.package_versions("hello-story"), vec!["0.1.0".to_string()]);
        assert_eq!(hub.default_version("hello-story").as_deref(), Some("0.1.0"));
        assert!(!hub.has_update("hello-story"), "no state file and one version: no mark");
        let _ = std::fs::remove_dir_all(&local);
    }
}
