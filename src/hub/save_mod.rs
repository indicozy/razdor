//! Which hub mod a refused save needs (1552): the mod the save records, else the one installed
//! mod whose content has every unit and item the save lacks. Pure: the App reads the installed
//! mods and their content, and asks what to offer.

use super::{plan_install, Catalog, Installed, Kind};
use crate::rules::content::{Content, ItemId, UnitId};
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

/// Whether `content` has every unit and item of `missing`.
pub fn covers(content: &Content, missing: &Unknown) -> bool {
    missing.units.iter().all(|&u| content.try_unit(UnitId(u)).is_some()) && missing.items.iter().all(|&i| content.try_item(ItemId(i)).is_some())
}

/// The installed mods (id, content) whose content covers `missing`, in the order given.
pub fn matching<'a>(missing: &Unknown, mods: impl IntoIterator<Item = (&'a str, &'a Content)>) -> Vec<String> {
    mods.into_iter().filter(|(_, c)| covers(c, missing)).map(|(id, _)| id.to_string()).collect()
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
    use crate::rules::content::testkit;
    use crate::rules::map_refs::Unknown;

    fn content(units: &[u32], items: &[u32]) -> Content {
        let us = units.iter().map(|&u| testkit::warrior(u, 3, 3)).collect();
        let is = items.iter().map(|&i| testkit::item(i, crate::dt::data::ArtefactType::Ring)).collect();
        testkit::content(us, is)
    }

    fn saved(id: &str) -> SaveMod {
        SaveMod { id: id.into(), version: "1.0".into() }
    }

    fn missing(units: &[u32], items: &[u32]) -> Unknown {
        Unknown { units: units.to_vec(), items: items.to_vec() }
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
    fn a_mod_matches_when_its_content_has_every_missing_id() {
        let m = missing(&[104, 105], &[35]);
        assert!(covers(&content(&[104, 105], &[35]), &m));
        assert!(!covers(&content(&[104], &[35]), &m), "a missing unit");
        assert!(!covers(&content(&[104, 105], &[]), &m), "a missing item");
        let evo = content(&[104, 105], &[35]);
        let base = content(&[1], &[7]);
        assert_eq!(matching(&m, [("evo", &evo), ("base", &base)]), vec!["evo".to_string()]);
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
}
