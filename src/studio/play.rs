//! «Играть» (F5, UX6, spec §4.2 and §9.3): the real game started from the studio on the package's current
//! map, in its own process (`razdor --pack <folder> --map <id>`, the game's own package start). The studio
//! keeps running and shows that the game runs; a refusal names what stops it (no install, errors in the
//! package). Nothing is pinned for the child: it inherits the studio's CPU affinity.
//!
//! The child keeps its own files apart from the player's: its save folder (`RAZDOR_SAVE_DIR`, which also
//! holds its settings), its log and its lock all sit in [`crate::platform::PLAY_DIR`] inside Razdor's folder,
//! so a play-test never writes the player's saves, settings or log (see `platform::app_dir`).

use std::path::{Path, PathBuf};

/// The variable the child is started with. The child is a play-test game: it keeps its files in its own
/// folder ([`crate::platform::PLAY_DIR`]) and takes its own lock there, not the studio's or the player's.
pub const CHILD_ENV: &str = "RAZDOR_STUDIO_PLAY";

/// How the game is started: the program, its arguments and the variables it gets on top of the studio's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Launch {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub envs: Vec<(String, String)>,
}

/// The launch of the game on `map` of the package in `folder`, with the install at `install`. The folder is
/// made absolute (the child may run from another directory). `play_dir` is the child's own folder
/// ([`crate::platform::PLAY_DIR`] in Razdor's folder): its saves and settings (`RAZDOR_SAVE_DIR`) and its log
/// go there, whatever the studio's environment says (an inherited `RAZDOR_SAVE_DIR` or `RAZDOR_LOG` is replaced).
pub fn launch(exe: &Path, folder: &Path, map: &str, install: &Path, play_dir: &Path) -> Launch {
    let folder = std::fs::canonicalize(folder).unwrap_or_else(|_| folder.to_path_buf());
    Launch {
        program: exe.to_path_buf(),
        args: vec!["--pack".into(), folder.display().to_string(), "--map".into(), map.to_string()],
        envs: vec![
            (CHILD_ENV.into(), "1".into()),
            ("RAZDOR_DT_DIR".into(), install.display().to_string()),
            ("RAZDOR_SAVE_DIR".into(), play_dir.display().to_string()),
            ("RAZDOR_LOG".into(), play_dir.join("razdor.log").display().to_string()),
        ],
    }
}

/// Why the game cannot start from the studio.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// No install is set (`RAZDOR_DT_DIR`): the game's files are needed.
    NoInstall,
    /// The package has errors (Проверка): the game would refuse it.
    Errors(usize),
    /// No map is shown: there is no map to start on.
    NoMap,
}

/// The refusal for a start, if there is one. The first reason that holds is the one given.
pub fn refusal(has_install: bool, errors: usize, has_map: bool) -> Option<Refusal> {
    if !has_install {
        Some(Refusal::NoInstall)
    } else if errors > 0 {
        Some(Refusal::Errors(errors))
    } else if !has_map {
        Some(Refusal::NoMap)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_game_starts_on_the_map_of_the_package_through_its_own_pack_entry() {
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let play = Path::new("/data/razdor").join(crate::platform::PLAY_DIR);
        let launch = launch(Path::new("/opt/razdor"), &folder, "marka", Path::new("/games/dt"), &play);
        assert_eq!(launch.program, PathBuf::from("/opt/razdor"));
        assert_eq!(launch.args[0], "--pack");
        assert_eq!(launch.args[2], "--map");
        assert_eq!(launch.args[3], "marka");
        assert!(Path::new(&launch.args[1]).is_absolute(), "the folder is absolute: {}", launch.args[1]);
        assert!(launch.envs.contains(&(CHILD_ENV.to_string(), "1".to_string())));
        assert!(launch.envs.contains(&("RAZDOR_DT_DIR".to_string(), "/games/dt".to_string())));
    }

    #[test]
    fn the_play_test_keeps_its_saves_settings_and_log_apart_from_the_players() {
        let root = Path::new("/data/razdor");
        let play = root.join(crate::platform::PLAY_DIR);
        let folder = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let launch = launch(Path::new("/opt/razdor"), &folder, "marka", Path::new("/games/dt"), &play);
        let env = |key: &str| {
            let (_, value) = launch.envs.iter().find(|(k, _)| k == key).unwrap_or_else(|| panic!("no {key}"));
            PathBuf::from(value)
        };
        let saves = env("RAZDOR_SAVE_DIR");
        assert_eq!(saves, play, "the child's saves folder is its own");
        assert_ne!(saves, root.join("saves"), "not the player's save folder");
        assert_ne!(saves.join("settings.json"), root.join("saves").join("settings.json"), "not the player's settings");
        assert_eq!(env("RAZDOR_LOG"), play.join("razdor.log"));
        assert_ne!(env("RAZDOR_LOG"), root.join("razdor.log"), "not the player's log");
    }

    #[test]
    fn a_start_is_refused_without_an_install_then_with_errors_then_without_a_map() {
        assert_eq!(refusal(false, 0, true), Some(Refusal::NoInstall));
        assert_eq!(refusal(false, 3, false), Some(Refusal::NoInstall), "the install comes first");
        assert_eq!(refusal(true, 2, true), Some(Refusal::Errors(2)));
        assert_eq!(refusal(true, 0, false), Some(Refusal::NoMap));
        assert_eq!(refusal(true, 0, true), None, "a clean package with a map starts");
    }
}
