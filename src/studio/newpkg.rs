//! A new package: the folder Razdor Studio's «New package» makes. Its structure is the example
//! package `examples/packs/hello-story`'s, cut to one map: a 50×50 grass meadow with the hero in
//! its middle, a defeat-all victory, and the package's name in both languages. Nothing of the
//! game is in it; the package checks clean (the test below).

use std::path::{Path, PathBuf};

use crate::pack::terrain::{encode, Palette, Terrain};

/// The new map's side, in cells (one of the sizes the engine loads).
pub const SIZE: u32 = 50;

/// The grass the new map is filled with (the engine's code of the plain grass).
const GRASS: u8 = 6;

/// A folder name as a package id: lowercase letters, digits and single hyphens. `new-package`
/// when nothing is left of the name.
pub fn slug(name: &str) -> String {
    let mapped: String = name.trim().to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    let id = mapped.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if id.is_empty() {
        "new-package".into()
    } else {
        id
    }
}

/// Makes the package `name` in `parent` (a new folder of that name) and returns its folder.
/// Nothing is left behind when a file cannot be written.
pub fn create(parent: &Path, name: &str) -> Result<PathBuf, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(crate::i18n::tr("The package needs a folder name.").into());
    }
    if name.contains(['/', '\\']) || name == "." || name == ".." {
        return Err(format!("{name}: not a folder name."));
    }
    if !parent.is_dir() {
        return Err(format!("{}: not a folder.", parent.display()));
    }
    let dir = parent.join(name);
    if dir.exists() {
        return Err(format!("{} already exists.", dir.display()));
    }
    let made = write_package(&dir, name);
    if let Err(e) = made {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(e);
    }
    Ok(dir)
}

fn write_package(dir: &Path, name: &str) -> Result<(), String> {
    let id = slug(name);
    let title = toml::Value::from(name).to_string();
    let files = [
        ("package.toml", manifest(&id)),
        ("texts/en.toml", texts(&["Main map", "A meadow of grass. The hero stands in its middle; beat the raiders.", "Raiders"], &title)),
        ("texts/ru.toml", texts(&["Главная карта", "Луг. Герой стоит посреди него; разбейте разбойников.", "Разбойники"], &title)),
        ("maps/main/map.toml", MAP.to_string()),
    ];
    for (path, text) in files {
        crate::fsutil::write_atomically(&dir.join(path), text.as_bytes()).map_err(|e| format!("{path}: {e}"))?;
    }
    let terrain = Terrain { width: SIZE, height: SIZE, cells: vec![GRASS; (SIZE * SIZE) as usize] };
    crate::fsutil::write_atomically(&dir.join("maps/main/terrain.png"), &encode(&terrain, &Palette::default())).map_err(|e| format!("terrain.png: {e}"))
}

fn manifest(id: &str) -> String {
    format!(
        "# A new package (Razdor Studio). Its map is maps/main; its names are in texts/.\n\
         id = \"{id}\"\nversion = \"0.1.0\"\nformat = 1\ntitle = \"package.title\"\ndescription = \"package.description\"\n\
         author = \"\"\nlicense = \"MIT\"\nbase = \"community-update\"\nrequires = []\nrules = \"dt\"\n"
    )
}

/// The texts of both languages: the package's names, then the map's title, description and the
/// raiders' name (`[package]`, `[main]`, `[raiders]`).
fn texts([map_title, map_about, raiders]: &[&str; 3], title: &str) -> String {
    format!(
        "[package]\ntitle = {title}\ndescription = \"A new package made in Razdor Studio.\"\n\n[main]\ntitle = \"{map_title}\"\ndescription = \"{map_about}\"\n\n[raiders]\nname = \"{raiders}\"\n"
    )
}

const MAP: &str = "title = \"main.title\"\ndescription = \"main.description\"\nstart_date = \"1300-04-01\"\n\
relations = [[3, 3, 0, -3], [3, 3, 0, -3], [0, 0, 3, -3], [-3, -3, -3, 3]]\n\n\
[hero]\ncell = [25, 25]\nclasses = [\"knight\"]\ngold = 300\nmana = 20\n\n\
# The one enemy army: the victory is beating it.\n\
[[army]]\nid = \"raiders\"\nname = \"raiders.name\"\ncell = [40, 40]\nfaction = \"enemy\"\nmodel = \"bandits\"\ngold = 60\n\
troops = [{ unit = \"dt:bandit\", level = 1, count = 3 }]\n\n\
[victory]\nkind = \"defeat_army\"\narmy = \"raiders\"\n";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::studio::Workspace;

    fn temp(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let d = std::env::temp_dir().join(format!("razdor-newpkg-{tag}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn slugs_are_ids() {
        assert_eq!(slug("My Story 2"), "my-story-2");
        assert_eq!(slug("  a__b  "), "a-b");
        assert_eq!(slug("Новый"), "new-package");
    }

    #[test]
    fn a_new_package_opens_and_checks_clean() {
        let parent = temp("ok");
        let dir = create(&parent, "My story").unwrap();
        assert_eq!(dir, parent.join("My story"));
        let mut ws = Workspace::open(&dir).unwrap();
        let (errors, _) = ws.check().unwrap();
        assert_eq!(errors, 0, "{:?}", ws.problems);
        assert!(ws.package().is_some());
        let terrain = std::fs::read(dir.join("maps/main/terrain.png")).unwrap();
        assert!(terrain.starts_with(b"\x89PNG"));
        let _ = std::fs::remove_dir_all(&parent);
    }

    #[test]
    fn refuses_bad_names_and_existing_folders() {
        let parent = temp("refuse");
        assert!(create(&parent, "  ").is_err());
        assert!(create(&parent, "a/b").is_err());
        assert!(create(&parent, "..").is_err());
        assert!(create(&parent.join("missing"), "x").is_err());
        create(&parent, "taken").unwrap();
        assert!(create(&parent, "taken").is_err());
        let _ = std::fs::remove_dir_all(&parent);
    }
}
