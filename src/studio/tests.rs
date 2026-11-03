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
