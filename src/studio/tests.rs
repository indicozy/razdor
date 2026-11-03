use std::fs;
use std::path::{Path, PathBuf};

use super::*;

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs").join(name)
}

/// A copy of an example package in a temporary folder, removed when dropped.
struct Copy(PathBuf);

impl Copy {
    fn of(name: &str) -> Copy {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("razdor-studio-test-{}-{nanos}", std::process::id()));
        copy_dir(&example(name), &dir);
        Copy(dir)
    }
}

impl Drop for Copy {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dst);
        } else {
            fs::copy(&src, &dst).unwrap();
        }
    }
}

#[test]
fn opening_hello_story_lists_its_files() {
    let ws = Workspace::open(example("hello-story")).unwrap();
    assert!(ws.entries.iter().any(|e| e == "package.toml"));
    assert!(ws.entries.iter().any(|e| e == "maps/first-light/map.toml"));
    assert!(ws.entries.iter().all(|e| !e.contains('\\') && !e.starts_with('/')));
    assert!(ws.entries.windows(2).all(|w| w[0] < w[1]), "sorted");
    assert!(ws.docs.is_empty() && ws.problems.is_empty());
}

#[test]
fn a_folder_without_package_toml_is_an_error() {
    let dir = std::env::temp_dir().join(format!("razdor-studio-nopkg-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let err = Workspace::open(&dir).unwrap_err();
    assert!(err.contains("package.toml"), "{err}");
    let _ = fs::remove_dir_all(&dir);
    assert!(Workspace::open(dir.join("missing")).is_err());
}

#[test]
fn open_edit_save_round_trip() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let idx = ws.open_doc("package.toml").unwrap();
    assert_eq!(ws.open_doc("package.toml").unwrap(), idx, "an open document is reused");
    assert_eq!(ws.docs.len(), 1);
    assert!(!ws.docs[idx].is_dirty());

    let original = ws.docs[idx].text.clone();
    ws.docs[idx].text.push_str("\n# edited by the test\n");
    assert!(ws.docs[idx].is_dirty());
    assert!(ws.any_dirty());
    // Not on disk yet.
    assert_eq!(fs::read_to_string(copy.0.join("package.toml")).unwrap(), original);

    ws.save(idx).unwrap();
    assert!(!ws.docs[idx].is_dirty());
    assert!(!ws.any_dirty());
    let on_disk = fs::read_to_string(copy.0.join("package.toml")).unwrap();
    assert!(on_disk.ends_with("# edited by the test\n"));

    // save_all writes only what is dirty.
    ws.docs[idx].text = original.clone();
    assert!(ws.any_dirty());
    ws.save_all().unwrap();
    assert_eq!(fs::read_to_string(copy.0.join("package.toml")).unwrap(), original);
}

#[test]
fn only_text_files_open() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let err = ws.open_doc("maps/first-light/terrain.png").unwrap_err();
    assert!(err.contains("not a text file"), "{err}");
    assert!(ws.open_doc("../escape.toml").is_err());
    assert!(ws.open_doc("maps/first-light/map.toml").is_ok());
}

#[test]
fn check_on_hello_story_has_no_errors() {
    let mut ws = Workspace::open(example("hello-story")).unwrap();
    let (errors, _warnings) = ws.check().unwrap();
    assert_eq!(errors, 0, "{:?}", ws.problems);
    assert!(ws.problems.iter().all(|d| d.severity != crate::pack::diag::Severity::Error));
}

#[test]
fn check_reports_a_broken_file_with_its_line() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let idx = ws.open_doc("items/relics.toml").unwrap();
    ws.docs[idx].text = "[[item]\nid = \"broken\"\n".to_string();
    ws.save(idx).unwrap();
    let (errors, _) = ws.check().unwrap();
    assert!(errors > 0);
    let hit = ws.problems.iter().find(|d| d.file == "items/relics.toml").expect("a diagnostic for the file");
    assert_eq!(hit.line, Some(1));
}

#[test]
fn map_ids_of_pepelnaya_marka_lists_its_maps() {
    let ws = Workspace::open(example("pepelnaya-marka")).unwrap();
    let (pkg, _) = ws.read_package().unwrap();
    let ids = map::map_ids(&pkg);
    for id in ["abbey", "barrows", "brod", "camp", "gates", "marka", "salt", "siege-test"] {
        assert!(ids.iter().any(|i| i == id), "{id} in {ids:?}");
    }
    assert!(ids.windows(2).all(|w| w[0] < w[1]), "sorted");
}

#[test]
fn the_map_shown_is_the_opened_files_then_the_campaign_start_then_the_first() {
    let ws = Workspace::open(example("pepelnaya-marka")).unwrap();
    let (mut pkg, _) = ws.read_package().unwrap();
    assert_eq!(map::map_of_file("maps/barrows/map.toml"), Some("barrows"));
    assert_eq!(map::map_of_file("maps/barrows"), None);
    assert_eq!(map::map_of_file("package.toml"), None);
    // A file under maps/<id>/ picks that map.
    assert_eq!(map::choose_map(&pkg, Some("maps/barrows/terrain.png")).as_deref(), Some("barrows"));
    // Not a map's file, or a map the package lacks: the campaign's start map.
    assert_eq!(map::choose_map(&pkg, Some("texts/en.toml")).as_deref(), Some("brod"));
    assert_eq!(map::choose_map(&pkg, Some("maps/nope/map.toml")).as_deref(), Some("brod"));
    assert_eq!(map::choose_map(&pkg, None).as_deref(), Some("brod"));
    // Without a campaign: the first map.
    pkg.campaign = None;
    assert_eq!(map::choose_map(&pkg, None), map::map_ids(&pkg).into_iter().next());
}

#[test]
fn inspect_finds_a_table_its_fields_and_its_line() {
    let text = "title = \"x\"\n\n[hero]\ncell = [1, 2]\n\n[[building]]\nid = \"a\"\ntype = \"town\"\ncell = [5, 6]\n\n[[building]]\nid = \"b\"\ntype = \"village\"\ncell = [9, 9]\n";
    let b = map::inspect(text, &map::Selection::Building("b".into())).unwrap();
    assert_eq!(b.line, 11);
    assert_eq!(b.fields.iter().map(|f| f.0.as_str()).collect::<Vec<_>>(), ["id", "type", "cell"]);
    assert_eq!(b.fields[2].1, "[9, 9]");
    let hero = map::inspect(text, &map::Selection::Hero).unwrap();
    assert_eq!((hero.line, hero.fields.len()), (3, 1));
    assert!(map::inspect(text, &map::Selection::Point(0)).is_err());
    assert!(map::inspect(text, &map::Selection::Army("a".into())).is_err());
}

#[test]
fn map_compiles_on_the_install() {
    if std::env::var_os(crate::dt::install::ENV_VAR).is_none() {
        return;
    }
    let install = crate::dt::install::DtInstall::from_env().unwrap();
    let ws = Workspace::open(example("pepelnaya-marka")).unwrap();
    let (pkg, _) = ws.read_package().unwrap();
    let mut view = map::MapView::default();
    view.show("brod");
    let problems = view.compile(&pkg, &install, "en");
    assert!(problems.is_empty(), "{problems:?}");
    let good = view.current().expect("brod compiles");
    let spec = pkg.maps["brod"].spec.as_ref().unwrap();
    if let Some(army) = spec.army.first().map(|a| a.get_ref()) {
        let cell = (army.cell[0] as i32, army.cell[1] as i32);
        assert_eq!(map::hit(&good.compiled, spec, cell), Some(map::Selection::Army(army.id.clone())));
        let outline = map::outline_of(&good.compiled, spec, &map::Selection::Army(army.id.clone()));
        assert_eq!(outline.len(), 1);
    }
    assert_eq!(map::centre_of(spec, &map::Selection::Hero), Some((spec.hero.cell[0] as f32 + 0.5, spec.hero.cell[1] as f32 + 0.5)));
}

#[test]
fn a_package_path_with_dot_dot_or_absolute_is_refused() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let err = ws.open_doc("../secret.toml").unwrap_err();
    assert!(err.contains("not a package path"), "{err}");
    assert!(ws.open_doc("/etc/secret.toml").is_err());
    assert!(ws.open_doc("maps/../../secret.toml").is_err());
    assert!(ws.docs.is_empty());
}

#[cfg(unix)]
mod links {
    use std::os::unix::fs::symlink;

    use super::*;

    /// A file outside the package, in a temporary folder of its own (removed when dropped).
    struct Outside(PathBuf);

    impl Outside {
        fn new() -> Outside {
            let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
            let dir = std::env::temp_dir().join(format!("razdor-studio-outside-{}-{nanos}", std::process::id()));
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("secret.toml"), "secret = true\n").unwrap();
            Outside(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join("secret.toml")
        }
    }

    impl Drop for Outside {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_symlink_out_of_the_package_is_not_listed_and_not_opened() {
        let copy = Copy::of("hello-story");
        let out = Outside::new();
        symlink(out.file(), copy.0.join("link.toml")).unwrap();
        symlink(&out.0, copy.0.join("linkdir")).unwrap();
        let mut ws = Workspace::open(&copy.0).unwrap();
        assert!(!ws.entries.iter().any(|e| e == "link.toml"), "{:?}", ws.entries);
        assert!(!ws.entries.iter().any(|e| e.starts_with("linkdir/")), "{:?}", ws.entries);
        assert!(ws.open_doc("link.toml").is_err());
        assert!(ws.open_doc("linkdir/secret.toml").is_err());
    }

    #[test]
    fn a_symlink_inside_the_package_to_a_file_inside_is_listed_and_opened() {
        let copy = Copy::of("hello-story");
        symlink(copy.0.join("package.toml"), copy.0.join("alias.toml")).unwrap();
        let mut ws = Workspace::open(&copy.0).unwrap();
        assert!(ws.entries.iter().any(|e| e == "alias.toml"));
        assert!(ws.open_doc("alias.toml").is_ok());
    }

    #[test]
    fn save_never_writes_through_a_symlink() {
        let copy = Copy::of("hello-story");
        let out = Outside::new();
        let mut ws = Workspace::open(&copy.0).unwrap();
        let idx = ws.open_doc("package.toml").unwrap();
        ws.docs[idx].text.push_str("# changed\n");
        // The file is swapped for a link to the outside file after it was opened.
        fs::remove_file(copy.0.join("package.toml")).unwrap();
        symlink(out.file(), copy.0.join("package.toml")).unwrap();
        assert!(ws.save(idx).is_err());
        assert_eq!(fs::read_to_string(out.file()).unwrap(), "secret = true\n");
    }
}
