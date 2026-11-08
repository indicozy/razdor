//! Which hub mod a refused save needs (1552): the mod the save records, else the one installed
//! mod whose content has every unit and item the save lacks. Pure: the App reads the installed
//! mods and their content, and asks what to offer.

use std::path::Path;

use super::{layer_bytes, layer_chain, plan_install, read_installed, Catalog, Installed, Kind};
use crate::dt::install::read_tables;
use crate::rules::map_refs::Unknown;
use crate::rules::save::SaveMod;

/// The mod a save needs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Need {
    /// The save records its mod.
    Recorded(SaveMod),
    /// An old save: the one installed mod that has everything it lacks.
    Inferred(String),
    /// No mod is known (none recorded, and none or several installed mods match).
    Unknown,
}

/// What the refusal window offers for a need.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Offer {
    /// The mod is installed: the button switches to it and loads the save.
    Installed { id: String },
    /// Not installed, but in the catalog: the button downloads it; `size` is what the download
    /// takes (the mod and the mods it needs, in bytes).
    Download { id: String, size: u64 },
    /// Recorded, but not installed and not in the catalog.
    NotInCatalog { id: String },
    /// No mod is known.
    Unknown,
}

/// The need of a save: its recorded mod, else the single installed mod in `matches`.
pub fn need(recorded: Option<&SaveMod>, matches: &[String]) -> Need {
    match (recorded, matches) {
        (Some(m), _) => Need::Recorded(m.clone()),
        (None, [one]) => Need::Inferred(one.clone()),
        _ => Need::Unknown,
    }
}

/// The unit types and item types an install has (their `GlobalIndex`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ids {
    pub units: Vec<u32>,
    pub items: Vec<u32>,
}

/// Whether an install with `ids` has every unit and item of `missing`.
pub fn covers(ids: &Ids, missing: &Unknown) -> bool {
    missing.units.iter().all(|u| ids.units.contains(u)) && missing.items.iter().all(|i| ids.items.contains(i))
}

/// The installed mods whose own tables cover `missing`, by id, in the record's order. A mod's tables
/// are read the way the game reads them with its layers (`layer_bytes`: the topmost layer's file,
/// else the base's), and nothing else: no folder is built and nothing is copied (a mod's merged
/// folder is a full copy of the install on a phone). A mod whose layers are not all installed, or
/// whose tables do not read, is not a match.
pub fn installed_matching(local: &Path, base: &Path, missing: &Unknown) -> Vec<String> {
    let installed = read_installed(local);
    let mut found = Vec::new();
    for (id, entry) in &installed.entries {
        if entry.kind != Kind::Mod {
            continue;
        }
        let Ok(chain) = layer_chain(&installed, id) else { continue };
        let bytes_of = |name: &str| layer_bytes(local, base, &chain, name);
        let Ok((units, artefacts)) = read_tables(base, &bytes_of) else { continue };
        let ids = Ids { units: units.iter().map(|u| u.id).collect(), items: artefacts.iter().map(|a| a.id).collect() };
        if covers(&ids, missing) {
            found.push(id.clone());
        }
    }
    found
}

/// What to offer for `need`: `installed` is whether the recorded mod is installed, `download` the
/// size of its download when the catalog has it (see [`download_size`]).
pub fn offer(need: &Need, installed: bool, download: Option<u64>) -> Offer {
    match need {
        Need::Unknown => Offer::Unknown,
        Need::Inferred(id) => Offer::Installed { id: id.clone() },
        Need::Recorded(m) if installed => Offer::Installed { id: m.id.clone() },
        Need::Recorded(m) => match download {
            Some(size) => Offer::Download { id: m.id.clone(), size },
            None => Offer::NotInCatalog { id: m.id.clone() },
        },
    }
}

/// The bytes a download of mod `id` takes in `lang` (`ru` or `en`): the files of the mod and of
/// the mods it needs that are not installed (`plan_install`). None when the catalog has no such
/// mod or its plan does not work.
pub fn download_size(catalog: &Catalog, installed: &Installed, id: &str, lang: &str) -> Option<u64> {
    if catalog.entry(id).is_none_or(|e| e.kind != Kind::Mod) {
        return None;
    }
    let plan = plan_install(catalog, installed, id).ok()?;
    let mut total = 0;
    for pid in &plan {
        total += catalog.entry(pid)?.files_for(lang).iter().map(|f| f.size).sum::<u64>();
    }
    Some(total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::{mod_game_dir, mod_files_dir, InstalledEntry, Text};

    fn saved(id: &str) -> SaveMod {
        SaveMod { id: id.into(), version: "1.0".into() }
    }

    fn missing(units: &[u32], items: &[u32]) -> Unknown {
        Unknown { units: units.to_vec(), items: items.to_vec() }
    }

    fn ids(units: &[u32], items: &[u32]) -> Ids {
        Ids { units: units.to_vec(), items: items.to_vec() }
    }

    #[test]
    fn a_recorded_mod_is_the_need_whatever_else_matches() {
        assert_eq!(need(Some(&saved("evo")), &[]), Need::Recorded(saved("evo")));
        assert_eq!(need(Some(&saved("evo")), &["a".into(), "b".into()]), Need::Recorded(saved("evo")));
    }

    #[test]
    fn an_old_save_infers_its_mod_only_when_exactly_one_installed_mod_matches() {
        assert_eq!(need(None, &["evo".into()]), Need::Inferred("evo".into()), "inferred-one");
        assert_eq!(need(None, &[]), Need::Unknown, "inferred-none");
        assert_eq!(need(None, &["evo".into(), "other".into()]), Need::Unknown, "inferred-several");
    }

    #[test]
    fn a_mod_covers_the_missing_ids_only_when_it_has_all_of_them() {
        let m = missing(&[104, 105], &[35]);
        assert!(covers(&ids(&[104, 105], &[35]), &m));
        assert!(!covers(&ids(&[104], &[35]), &m), "a missing unit");
        assert!(!covers(&ids(&[104, 105], &[]), &m), "a missing item");
    }

    #[test]
    fn an_installed_recorded_mod_is_offered_for_the_switch() {
        assert_eq!(offer(&Need::Recorded(saved("evo")), true, None), Offer::Installed { id: "evo".into() }, "installed");
        assert_eq!(offer(&Need::Recorded(saved("evo")), true, Some(9)), Offer::Installed { id: "evo".into() }, "installed wins over the catalog");
    }

    #[test]
    fn a_recorded_mod_in_the_catalog_is_downloadable_with_its_size() {
        assert_eq!(offer(&Need::Recorded(saved("evo")), false, Some(5_000)), Offer::Download { id: "evo".into(), size: 5_000 }, "downloadable");
    }

    #[test]
    fn a_recorded_mod_not_in_the_catalog_is_unavailable() {
        assert_eq!(offer(&Need::Recorded(saved("evo")), false, None), Offer::NotInCatalog { id: "evo".into() }, "unavailable");
    }

    #[test]
    fn an_inferred_mod_is_installed_and_no_known_mod_is_unknown() {
        assert_eq!(offer(&Need::Inferred("evo".into()), true, None), Offer::Installed { id: "evo".into() });
        assert_eq!(offer(&Need::Unknown, false, Some(1)), Offer::Unknown);
    }

    /// A scratch install and hub folder for one test: a base with three unit and item tables, and
    /// mods recorded as installed. Removed by the caller.
    struct Scratch {
        local: std::path::PathBuf,
        base: std::path::PathBuf,
    }

    impl Scratch {
        fn new(tag: &str) -> Scratch {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let local = std::env::temp_dir().join(format!("razdor-savemod-{tag}-{}-{nanos}", std::process::id()));
            let base = local.join("base");
            std::fs::create_dir_all(&base).unwrap();
            std::fs::write(base.join("_Global.ini"), "").unwrap();
            std::fs::write(base.join("Rus_Units.ini"), "[Unit1]\r\nName=Base\r\nStartExpirience=1\r\nCost=1\r\nGlobalIndex=1\r\n").unwrap();
            std::fs::write(base.join("Rus_Artefacts.ini"), "[Item7]\r\nIcon=a\r\nName=b\r\nType=Item\r\nCost=1\r\nGlobalIndex=7\r\n").unwrap();
            Scratch { local, base }
        }

        /// Records mod `id` as installed (with `requires`) and writes its own tables (`units`, `items`).
        fn mod_with(&self, id: &str, requires: &[&str], units: &[u32], items: &[u32]) {
            let files = mod_files_dir(&self.local, id);
            std::fs::create_dir_all(&files).unwrap();
            let unit_text: String = units.iter().map(|u| format!("[Unit{u}]\r\nName=U{u}\r\nStartExpirience=1\r\nCost=1\r\nGlobalIndex={u}\r\n")).collect();
            let item_text: String = items.iter().map(|i| format!("[Item{i}]\r\nIcon=a\r\nName=I{i}\r\nType=Item\r\nCost=1\r\nGlobalIndex={i}\r\n")).collect();
            std::fs::write(files.join("Rus_Units.ini"), unit_text).unwrap();
            std::fs::write(files.join("Rus_Artefacts.ini"), item_text).unwrap();
            let mut installed = read_installed(&self.local);
            installed.entries.insert(
                id.into(),
                InstalledEntry {
                    version: "2.1".into(),
                    kind: Kind::Mod,
                    lang: "ru".into(),
                    title: Text { ru: id.into(), en: String::new() },
                    requires: requires.iter().map(|r| r.to_string()).collect(),
                    files: Vec::new(),
                    sources: Vec::new(),
                },
            );
            std::fs::write(self.local.join("installed.json"), serde_json::to_vec(&installed).unwrap()).unwrap();
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.local);
        }
    }

    #[test]
    fn an_old_save_finds_the_installed_mod_that_has_the_missing_ids() {
        let s = Scratch::new("found");
        s.mod_with("evo", &[], &[104, 105], &[35]);
        s.mod_with("other", &[], &[1], &[7]);
        assert_eq!(installed_matching(&s.local, &s.base, &missing(&[104, 105], &[35])), vec!["evo".to_string()], "inferred-one");
        assert_eq!(installed_matching(&s.local, &s.base, &missing(&[1], &[7])), vec!["other".to_string()], "the base's ids count for a mod too");
        assert!(installed_matching(&s.local, &s.base, &missing(&[999], &[])).is_empty(), "inferred-none");
    }

    #[test]
    fn several_installed_mods_that_cover_the_ids_are_several_matches() {
        let s = Scratch::new("several");
        s.mod_with("evo", &[], &[104], &[]);
        s.mod_with("twin", &[], &[104], &[]);
        let found = installed_matching(&s.local, &s.base, &missing(&[104], &[]));
        assert_eq!(found.len(), 2);
        assert_eq!(need(None, &found), Need::Unknown, "inferred-several is no known mod");
    }

    #[test]
    fn a_mods_own_table_replaces_the_base_one() {
        // The layer's file is the file the game reads: the base's unit 1 is not in the mod's table.
        let s = Scratch::new("replace");
        s.mod_with("evo", &[], &[104], &[]);
        assert!(installed_matching(&s.local, &s.base, &missing(&[1], &[])).is_empty());
    }

    #[test]
    fn a_mod_with_a_missing_requirement_is_not_a_match() {
        let s = Scratch::new("requires");
        s.mod_with("needs-base-mod", &["not-installed"], &[104], &[]);
        assert!(installed_matching(&s.local, &s.base, &missing(&[104], &[])).is_empty());
    }

    /// Inferring a mod builds no game folder and copies nothing (a phone's copy of the install is
    /// gigabytes; 1550's black screen came from one).
    #[test]
    fn the_inference_creates_no_game_folder() {
        let s = Scratch::new("no-folder");
        s.mod_with("evo", &[], &[104], &[35]);
        let found = installed_matching(&s.local, &s.base, &missing(&[104], &[35]));
        assert_eq!(found, vec!["evo".to_string()]);
        assert!(!mod_game_dir(&s.local, "evo").exists(), "the mod's game folder must not be built");
        assert!(!s.local.join("mods").join("evo").join("game").exists());
        let built_anything = std::fs::read_dir(s.local.join("mods")).map(|d| d.count()).unwrap_or(0);
        assert_eq!(built_anything, 1, "only the mod's own files folder is there");
    }
}
