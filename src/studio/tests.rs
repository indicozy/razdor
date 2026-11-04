use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use super::build;
use crate::pack::diag::Severity as Sev;
use crate::studio::edit::{self as ed, Edit, Kind, Locator};
use crate::studio::map;
use crate::studio::history::{History, Step};
use toml_edit::Value;

/// The brod map of the Pepelnaya Marka example (package-relative).
const BROD: &str = "maps/brod/map.toml";

/// The `toll` army of brod.
fn toll() -> Locator {
    Locator { file: BROD.into(), kind: Kind::Army, id: Some("toll".into()), index: 0 }
}

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

fn walk_toml(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_toml(&p, out);
        } else if p.extension().is_some_and(|x| x == "toml") {
            out.push(p);
        }
    }
}

#[test]
fn every_toml_of_the_examples_prints_back_byte_for_byte() {
    let mut files = Vec::new();
    for name in ["hello-story", "pepelnaya-marka"] {
        walk_toml(&example(name), &mut files);
    }
    assert!(files.len() > 10, "{} files", files.len());
    for f in files {
        let text = fs::read_to_string(&f).unwrap();
        let doc: toml_edit::DocumentMut = text.parse().unwrap();
        assert_eq!(doc.to_string(), text, "{}", f.display());
    }
}

#[test]
fn moving_the_toll_army_changes_one_line_and_keeps_every_comment() {
    let copy = Copy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let before = fs::read_to_string(copy.0.join(BROD)).unwrap();
    ws.apply(Edit::SetCell { at: toll(), cell: (12, 13) }).unwrap();
    let idx = ws.docs.iter().position(|d| d.path == BROD).unwrap();
    let after = ws.docs[idx].text.clone();
    let (b, a): (Vec<&str>, Vec<&str>) = (before.lines().collect(), after.lines().collect());
    assert_eq!(a.len(), b.len(), "no line added or removed");
    let changed: Vec<usize> = (0..b.len()).filter(|&i| b[i] != a[i]).collect();
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(a[changed[0]], "cell = [12, 13]");
    assert_eq!(b[changed[0]], "cell = [29, 23]");
    assert_eq!(changed[0], b.iter().position(|l| *l == "id = \"toll\"").unwrap() + 2, "the toll's own cell");
    let comments = |t: &str| t.lines().filter(|l| l.trim_start().starts_with('#')).count();
    assert_eq!(comments(&before), comments(&after));
    assert!(ws.docs[idx].is_dirty());
    assert_eq!(fs::read_to_string(copy.0.join(BROD)).unwrap(), before, "nothing written to disk");
    assert_eq!(ws.history.undo_label(), Some("move army toll"));
}

#[test]
fn a_key_the_format_does_not_have_is_an_error() {
    let copy = Copy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let edit = Edit::SetValue { at: toll(), key: "colour".into(), value: Value::from("red") };
    let err = ws.apply(edit).unwrap_err();
    assert!(err.contains("colour"), "{err}");
    assert!(ws.history.undo_label().is_none());
    let text = ed::apply_to_text(&fs::read_to_string(copy.0.join(BROD)).unwrap(), &Edit::SetValue { at: toll(), key: "colour".into(), value: Value::from("red") });
    assert!(text.is_err());
}

#[test]
fn the_locator_at_the_toll_id_is_that_army_and_the_building_spans_are_disjoint() {
    let ws = Workspace::open(example("pepelnaya-marka")).unwrap();
    let text = fs::read_to_string(example("pepelnaya-marka").join(BROD)).unwrap();
    let at = text.find("id = \"toll\"").unwrap();
    let found = ws.locator_at(BROD, at).expect("a table at the id");
    assert_eq!((found.kind, found.id.as_deref()), (Kind::Army, Some("toll")));

    let spans = ws.spans(BROD).unwrap();
    let buildings: Vec<_> = spans.iter().filter(|(l, _)| l.kind == Kind::Building).collect();
    assert_eq!(buildings.len(), 12);
    for w in spans.windows(2) {
        assert!(w[0].1.end <= w[1].1.start, "{:?} and {:?} overlap", w[0].0, w[1].0);
    }
    assert!(spans.iter().all(|(_, r)| r.end <= text.len()));
    for (l, r) in &spans {
        let head = match l.kind {
            Kind::Hero => "[hero]",
            Kind::Building => "[[building]]",
            Kind::Army => "[[army]]",
            Kind::Point => "[[point]]",
            Kind::Object => "[[object]]",
        };
        assert!(text[r.start..].starts_with(head), "{:?} starts at its header", l);
    }
    // A comment above a table is not in its span.
    let comment = text.find("# The ford toll").unwrap();
    assert_eq!(ws.locator_at(BROD, comment), None);
}

#[test]
fn undo_and_redo_restore_the_exact_texts_and_a_burst_is_one_step() {
    let copy = Copy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let idx = ws.open_doc("package.toml").unwrap();
    let original = ws.docs[idx].text.clone();

    // A burst of typing: three frames, one undo step with the text before and after.
    let mut prev = original.clone();
    for c in ["x", "y", "z"] {
        ws.begin_text_edit("package.toml", &prev);
        ws.docs[idx].text.push_str(c);
        prev = ws.docs[idx].text.clone();
    }
    ws.end_text_edit();
    assert_eq!(ws.history.undo_len(), 1);
    assert_eq!(ws.history.undo_label(), Some("edit package.toml"));
    assert_eq!(ws.undo().unwrap().as_deref(), Some("edit package.toml"));
    assert_eq!(ws.docs[idx].text, original);
    assert_eq!(ws.redo().unwrap().as_deref(), Some("edit package.toml"));
    assert_eq!(ws.docs[idx].text, format!("{original}xyz"));
    assert!(ws.undo().unwrap().is_some());

    // A visual edit is one step with the army's label; a new step clears the redo stack.
    let brod_before = ws.open_doc(BROD).map(|i| ws.docs[i].text.clone()).unwrap();
    ws.apply(Edit::SetCell { at: toll(), cell: (12, 13) }).unwrap();
    assert_eq!(ws.history.undo_label(), Some("move army toll"));
    assert!(ws.undo().unwrap().is_some());
    let i = ws.docs.iter().position(|d| d.path == BROD).unwrap();
    assert_eq!(ws.docs[i].text, brod_before, "undo restores the exact text");
    assert_eq!(ws.history.redo_label(), Some("move army toll"));
    ws.apply(Edit::SetCell { at: toll(), cell: (14, 14) }).unwrap();
    assert!(ws.history.redo_label().is_none(), "a new step clears redo");
    // Nothing to undo once the stack is empty.
    while ws.undo().unwrap().is_some() {}
    assert_eq!(ws.docs[i].text, brod_before);
    assert_eq!(ws.undo().unwrap(), None);
}

#[test]
fn the_undo_stack_keeps_its_last_200_steps() {
    let mut h = History::default();
    for n in 0..210 {
        h.record(Step::text("a.toml", n.to_string(), (n + 1).to_string(), format!("step {n}")));
    }
    assert_eq!(h.undo_len(), history::LIMIT);
    assert_eq!(h.undo_label(), Some("step 209"));
    // A step that changes nothing is not recorded.
    h.record(Step::text("a.toml", "x", "x", "nothing"));
    assert_eq!(h.undo_len(), history::LIMIT);
}

#[test]
fn an_unsaved_edit_that_breaks_a_reference_shows_in_the_checks_without_a_save() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    assert_eq!(ws.check().unwrap().0, 0);
    let path = "maps/first-light/map.toml";
    let idx = ws.open_doc(path).unwrap();
    let broken = ws.docs[idx].text.replacen("faction = \"player\"", "faction = \"nobody\"", 1);
    assert_ne!(broken, ws.docs[idx].text);
    ws.docs[idx].text = broken;
    let (errors, _) = ws.check().unwrap();
    assert!(errors > 0);
    assert!(ws.problems.iter().any(|d| d.file == path && d.message.contains("nobody")), "{:?}", ws.problems);
    assert!(!fs::read_to_string(copy.0.join(path)).unwrap().contains("nobody"), "still not on disk");
}

#[test]
fn a_file_changed_outside_is_ignored_when_it_is_our_save_reloaded_when_clean_and_asked_when_dirty() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let idx = ws.open_doc("package.toml").unwrap();
    let original = ws.docs[idx].text.clone();

    // Our own save, seen again by the watcher: nothing.
    ws.docs[idx].text.push_str("# mine\n");
    ws.save(idx).unwrap();
    let saved = ws.docs[idx].text.clone();
    assert_eq!(ws.on_disk_change("package.toml", &saved), Reconcile::Ignored);
    assert!(!ws.docs[idx].is_dirty());
    assert_eq!(ws.history.undo_len(), 0);

    // A clean document takes the disk text, as one undo step.
    let outside = format!("{original}# outside\n");
    assert_eq!(ws.on_disk_change("package.toml", &outside), Reconcile::Reloaded);
    assert_eq!(ws.docs[idx].text, outside);
    assert!(!ws.docs[idx].is_dirty());
    assert_eq!(ws.history.undo_label(), Some("outside change: package.toml"));
    assert!(ws.undo().unwrap().is_some());
    assert_eq!(ws.docs[idx].text, saved, "the step restores the text before it");

    // A dirty document keeps its text and asks.
    ws.docs[idx].text.push_str("# typed\n");
    let mine = ws.docs[idx].text.clone();
    let disk = format!("{saved}# disk\n");
    assert_eq!(ws.on_disk_change("package.toml", &disk), Reconcile::Asks { path: "package.toml".into(), disk: disk.clone() });
    assert_eq!(ws.docs[idx].text, mine, "nothing replaced before the user answers");
    let changed = unified(&mine, &disk).lines().filter(|l| l.starts_with('+') && !l.starts_with("+++")).count();
    assert_eq!(changed, 1, "one line added on disk");

    // Keep mine: the text stays, dirty, and the disk text is the baseline.
    ws.keep_mine("package.toml", &disk).unwrap();
    assert!(ws.docs[idx].is_dirty());
    assert_eq!(ws.on_disk_change("package.toml", &disk), Reconcile::Ignored, "the disk is the baseline now");
    // Reload: the disk text, as one undo step that restores mine.
    ws.take_disk("package.toml", &disk).unwrap();
    assert_eq!(ws.docs[idx].text, disk);
    assert!(!ws.docs[idx].is_dirty());
    ws.undo().unwrap();
    assert_eq!(ws.docs[idx].text, mine);
}

#[test]
fn a_file_not_open_is_ignored_by_the_reconcile() {
    let copy = Copy::of("hello-story");
    let mut ws = Workspace::open(&copy.0).unwrap();
    assert_eq!(ws.on_disk_change("package.toml", "anything"), Reconcile::Ignored);
    assert!(ws.docs.is_empty());
}

#[test]
fn a_broken_toml_keeps_the_last_good_package_and_refuses_visual_edits() {
    let copy = Copy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    ws.check().unwrap();
    let good = ws.package().unwrap();
    let idx = ws.open_doc(BROD).unwrap();
    let original = ws.docs[idx].text.clone();
    ws.docs[idx].text.push_str("\n[[army\n");
    ws.rebuild().unwrap();

    assert!(Arc::ptr_eq(&good, &ws.package().unwrap()), "the last good package is kept");
    assert_eq!(ws.stale.len(), 1);
    assert_eq!(ws.stale[0].path, BROD);
    assert!(ws.stale[0].line > 1, "{:?}", ws.stale);
    assert!(ws.stale[0].banner().starts_with("stale: maps/brod/map.toml line "));
    assert!(ws.problems.iter().any(|d| d.file == BROD && d.severity == Sev::Error));

    let before = ws.docs[idx].text.clone();
    let err = ws.apply(Edit::SetCell { at: toll(), cell: (12, 13) }).unwrap_err();
    assert!(err.starts_with("stale: maps/brod/map.toml line "), "{err}");
    assert_eq!(ws.docs[idx].text, before, "the text is not changed");

    // Other files stay editable.
    assert!(ws.open_doc("triggers/brod.toml").is_ok());

    // Fixed again: the package is read again.
    ws.docs[idx].text = original;
    ws.rebuild().unwrap();
    assert!(ws.stale.is_empty());
    assert!(!Arc::ptr_eq(&good, &ws.package().unwrap()));
}

#[test]
fn the_diff_of_a_file_shows_the_changed_lines() {
    let d = unified("a\nb\nc\n", "a\nB\nc\n");
    assert!(d.contains("-b\n"), "{d}");
    assert!(d.contains("+B\n"), "{d}");
    assert_eq!(unified("same\n", "same\n").lines().filter(|l| l.starts_with('+') || l.starts_with('-')).count(), 0);
}

#[test]
fn a_selection_has_its_locator_and_back() {
    let ws = Workspace::open(example("pepelnaya-marka")).unwrap();
    let (pkg, _) = ws.read_package().unwrap();
    let spec = pkg.maps["brod"].spec.as_ref().unwrap();
    for sel in [map::Selection::Hero, map::Selection::Army("toll".into()), map::Selection::Point(0), map::Selection::Object(3)] {
        let loc = map::locator_of(BROD, spec, &sel).unwrap();
        assert_eq!(map::selection_of(&loc), Some(sel.clone()));
        assert!(map::cell_of(spec, &sel).is_some());
    }
}

#[test]
#[ignore = "timing only: cargo test --release --lib studio::tests::perf_ -- --ignored --nocapture"]
fn perf_rebuild_and_compile_marka() {
    let install = std::sync::Arc::new(crate::dt::install::DtInstall::from_env().unwrap());
    let mut ws = Workspace::open(example("pepelnaya-marka")).unwrap();
    let n = 10;
    for map in ["brod", "marka", "gates", "salt"] {
        let job = ws.job(Some(map.to_string()), "en", Some(install.clone()));
        build::run(job).unwrap();
        let t = std::time::Instant::now();
        for _ in 0..n {
            let job = ws.job(Some(map.to_string()), "en", Some(install.clone()));
            let built = build::run(job).unwrap();
            assert!(matches!(built.compiled, Some(Ok(_))), "{map} compiles");
        }
        println!("PERF job (read + parse + check + drawn compile + reach) {map}, avg of {n}: {:?}", t.elapsed() / n);
    }
    let job = ws.job(None, "en", None);
    let t = std::time::Instant::now();
    for _ in 0..n {
        build::run(ws.job(None, "en", None)).unwrap();
    }
    println!("PERF job without a map (read + parse + check), avg of {n}: {:?}", t.elapsed() / n);
    let _ = job;
    ws.rebuild().unwrap();
}

// Razdor Studio's map tools (phase 2, milestone 1): each operation is one undo step, and undoing it
// gives back the text and the terrain it changed.

mod map_tools {
    use super::*;
    use crate::studio::brush::{self, Forest};
    use crate::studio::geometry::CellRect;
    use crate::studio::region;

    const BROD_MAP: &str = "brod";

    fn brod() -> String {
        format!("maps/{BROD_MAP}/map.toml")
    }

    /// The terrain codes of the brod map, as the workspace has them.
    fn codes(ws: &mut Workspace) -> Vec<u8> {
        let i = ws.open_terrain(BROD_MAP).unwrap();
        ws.terrains[i].terrain.cells.clone()
    }

    #[test]
    fn a_forest_stroke_is_one_undo_step_and_undoes_to_the_original_text() {
        let copy = Copy::of("pepelnaya-marka");
        let mut ws = Workspace::open(&copy.0).unwrap();
        let file = brod();
        let before = ws.text_of(&file).unwrap();
        let cells = brush::square(20, 20, 3);
        let text = ws.text_of(&file).unwrap();
        let edits = brush::forest_edits(&file, &text, &cells, Forest::Family(3), false, 3);
        brush::commit(&mut ws, "forests", BROD_MAP, edits, Vec::new()).unwrap();
        let after = ws.text_of(&file).unwrap();
        let planted = brush::object_rows(&after).iter().filter(|r| cells.contains(&r.cell) && r.kind == "trees").count();
        assert_eq!(planted, 9, "one tree per cell of the 3x3 brush");
        assert!(brush::object_rows(&after).iter().filter(|r| r.kind == "trees" && cells.contains(&r.cell)).all(|r| (36..48).contains(&r.sprite.unwrap())), "family 3 sprites");
        assert_eq!(ws.history.undo_label(), Some("forests"));
        ws.undo().unwrap();
        assert_eq!(ws.text_of(&file).unwrap(), before, "one undo puts the whole stroke back");
    }

    #[test]
    fn a_hill_and_its_erase_are_undo_steps_and_the_comments_stay() {
        let copy = Copy::of("pepelnaya-marka");
        let mut ws = Workspace::open(&copy.0).unwrap();
        let file = brod();
        let before = ws.text_of(&file).unwrap();
        let comments = before.lines().filter(|l| l.starts_with('#')).count();
        let text = ws.text_of(&file).unwrap();
        brush::commit(&mut ws, "hills", BROD_MAP, brush::hill_edits(&file, &text, brush::hill_centre(30, 30, 2), 2), Vec::new()).unwrap();
        let placed = ws.text_of(&file).unwrap();
        assert!(brush::object_rows(&placed).iter().any(|r| r.kind == "hills" && r.cell == (31, 31) && r.sprite == Some(20)), "a 2x2 hill ends at (31, 31)");
        let text = ws.text_of(&file).unwrap();
        brush::commit(&mut ws, "erase", BROD_MAP, brush::hill_erase_edits(&file, &text, (30, 30)), Vec::new()).unwrap();
        let erased = ws.text_of(&file).unwrap();
        assert!(!brush::object_rows(&erased).iter().any(|r| r.kind == "hills" && r.cell == (31, 31)), "the hill covering (30, 30) is gone");
        assert_eq!(erased.lines().filter(|l| l.starts_with('#')).count(), comments, "the comment lines are kept");
        ws.undo().unwrap();
        assert!(brush::object_rows(&ws.text_of(&file).unwrap()).iter().any(|r| r.kind == "hills" && r.cell == (31, 31)), "undo brings it back");
        ws.undo().unwrap();
        assert_eq!(ws.text_of(&file).unwrap(), before);
    }

    #[test]
    fn burning_is_one_undo_step_for_the_trees_and_the_ground() {
        let copy = Copy::of("pepelnaya-marka");
        let mut ws = Workspace::open(&copy.0).unwrap();
        let file = brod();
        let before_text = ws.text_of(&file).unwrap();
        let before_codes = codes(&mut ws);
        let w = 50;
        let edits = brush::burn_tree_edits(&file, &before_text);
        let cells = brush::burn_cells(&before_codes, w);
        assert!(!cells.is_empty(), "the brod map has ground above water");
        brush::commit(&mut ws, "burn everything", BROD_MAP, edits, cells).unwrap();
        let after = codes(&mut ws);
        assert!(after.iter().zip(&before_codes).all(|(a, b)| if (4..=15).contains(b) && *b != 13 { *a == 13 } else { a == b }), "ground above water is scorched, water and lava stay");
        let burnt = ws.text_of(&file).unwrap();
        assert!(brush::object_rows(&burnt).iter().all(|r| r.kind != "trees"), "every live tree is dead now");
        assert_eq!(ws.history.undo_label(), Some("burn everything"));
        ws.undo().unwrap();
        assert_eq!(codes(&mut ws), before_codes, "the ground comes back");
        assert_eq!(ws.text_of(&file).unwrap(), before_text, "and the trees");
    }

    #[test]
    fn a_pasted_region_with_a_building_gets_new_ids_and_is_one_undo_step() {
        let copy = Copy::of("pepelnaya-marka");
        let mut ws = Workspace::open(&copy.0).unwrap();
        let file = brod();
        let before = ws.text_of(&file).unwrap();
        let spans = ed::spans(&file, &before).unwrap();
        let (_, range) = spans.iter().find(|(l, _)| l.kind == Kind::Building && l.id.as_deref() == Some("tower")).expect("the tower").clone();
        let (cx, cy) = brush::cell_of_fragment(&before[range]).unwrap();
        let rect = CellRect::spanning(((cx - 2) as i32, (cy - 2) as i32), ((cx + 2) as i32, (cy + 2) as i32));
        let clip = region::copy(&mut ws, BROD_MAP, rect).unwrap();
        assert!(clip.tables.iter().any(|t| t.kind == Kind::Building), "the tower is copied");
        let target = (1i64, 1i64);
        let added = region::paste(&mut ws, BROD_MAP, &clip, target).unwrap();
        assert!(added >= 1);
        let after = ws.text_of(&file).unwrap();
        let ids: Vec<String> = ed::spans(&file, &after).unwrap().into_iter().filter(|(l, _)| l.kind == Kind::Building).filter_map(|(l, _)| l.id).collect();
        assert!(ids.iter().any(|id| id == "tower") && ids.iter().any(|id| id == "fort_1"), "the copy is fort_1: {ids:?}");
        assert_eq!(after.lines().filter(|l| l.starts_with('#')).count(), before.lines().filter(|l| l.starts_with('#')).count(), "comments elsewhere survive");
        assert_eq!(ws.history.undo_label(), Some("paste"));
        let i = ws.open_terrain(BROD_MAP).unwrap();
        let w = ws.terrains[i].terrain.width;
        assert_eq!(ws.terrains[i].code_at(1, 1), Some(clip.codes[0]), "the top left cell is pasted at the target");
        assert!(w >= 50);
        ws.undo().unwrap();
        assert_eq!(ws.text_of(&file).unwrap(), before, "one undo takes the paste back");
    }

    #[test]
    fn a_stamp_saves_and_reads_back_with_its_terrain_and_tables() {
        let copy = Copy::of("pepelnaya-marka");
        let mut ws = Workspace::open(&copy.0).unwrap();
        let rect = CellRect::spanning((10, 10), (16, 14));
        let clip = region::copy(&mut ws, BROD_MAP, rect).unwrap();
        region::save_stamp(&copy.0, "Ford corner", &clip).unwrap();
        let listed = region::list_stamps(&copy.0);
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].name, "Ford corner");
        assert_eq!(listed[0].clip, clip, "the stamp reads back as it was copied");
        assert!(region::save_stamp(&copy.0, "../escape", &clip).is_err(), "a stamp name is a plain name");
        // The studio's own folder is not a package entry: the check does not warn about it.
        ws.check().unwrap();
        assert!(ws.problems.iter().all(|d| !d.file.starts_with(".editor")), "{:?}", ws.problems);
    }
}

#[test]
fn the_scenario_forms_edit_save_and_undo_on_a_copy_of_pepelnaya_marka() {
    // The live run's operations, through the workspace on a scratch copy: the start date, a campaign
    // edge added and reordered, a character renamed across files, a translation added, a save, and
    // undo of each step.
    use super::campaign_graph as cg;
    use super::doc::{entry_id, table, DocEdit};
    use super::scenario;
    use toml_edit::Value;
    let copy = Copy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    let map = "maps/brod/map.toml";
    let before_date = ws.text_of(map).unwrap();
    ws.apply_doc(map, "set start date", &[scenario::start_date("1301-02-03").unwrap()]).unwrap();
    assert!(ws.text_of(map).unwrap().contains("start_date = \"1301-02-03\""));
    ws.undo().unwrap();
    assert_eq!(ws.text_of(map).unwrap(), before_date, "undo restores the date");

    // The campaign: add brod → salt, then move it first.
    ws.apply_doc(cg::FILE, "add edge", &[scenario::add_edge("brod", "salt")]).unwrap();
    ws.apply_doc(cg::FILE, "move edge", &[scenario::move_edge("brod", 1, 0)]).unwrap();
    let campaign = crate::pack::campaign::read(&ws.text_of(cg::FILE).unwrap(), &mut Report::default()).unwrap();
    let targets: Vec<&str> = campaign.map("brod").unwrap().next.iter().map(|n| n.to.as_str()).collect();
    assert_eq!(targets, ["salt", "marka"], "the moved edge is first");
    ws.undo().unwrap();
    ws.undo().unwrap();
    let back = crate::pack::campaign::read(&ws.text_of(cg::FILE).unwrap(), &mut Report::default()).unwrap();
    assert_eq!(back.map("brod").unwrap().next.len(), 1, "both undone");

    // A character renamed across the files that name it, one undo step.
    let pkg = ws.read_package().unwrap().0;
    let texts = |f: &str| ws.text_of(f).ok();
    let plan = crate::studio::characters::rename_plan(&pkg, "ilm", "brother_ilm", &texts).unwrap();
    assert!(plan.len() >= 2, "triggers, dialogs and texts name ilm: {}", plan.len());
    ws.set_texts("rename ilm to brother_ilm", plan).unwrap();
    assert!(ws.text_of("dialogs/marka.toml").unwrap().contains("named = \"brother_ilm\""));
    ws.undo().unwrap();
    assert!(ws.text_of("dialogs/marka.toml").unwrap().contains("named = \"ilm\""), "one undo puts all files back");

    // A translation: a key in en, then save, then undo.
    let (file, edit) = crate::studio::texts::set_cell("ru", "story.title", "Тестовый перевод");
    ws.apply_doc(&file, "set story.title ru", &[edit]).unwrap();
    ws.save_all().unwrap();
    let saved = fs::read_to_string(copy.0.join("texts/ru.toml")).unwrap();
    assert!(saved.contains("Тестовый перевод"), "{saved}");
    ws.undo().unwrap();
    assert!(!ws.text_of("texts/ru.toml").unwrap().contains("Тестовый перевод"));
    let _ = (entry_id("map", "x"), table("x"), DocEdit::Remove { path: vec![], key: "x".into() }, Value::from(1_i64));
}

/// The story editor (P4) on a copy of Pepelnaya Marka: a trigger's action changes kind and a field,
/// a dialog gets a choice, a choice is wired to a dialog (as the graph's drag does: first a choice
/// nothing answers, then one a `dialog_choice` trigger answers), a variable is renamed across the
/// files, the layout is written, the package is saved; each step parses, adds no check error, and
/// undo restores every text byte for byte.
#[test]
fn the_story_forms_and_wiring_edit_check_and_undo_on_a_copy_of_pepelnaya_marka() {
    use super::story::{self as st, Options, Section, Step};
    use super::{campaign_graph, story_graph, vars};
    use crate::pack::triggers as pt;
    use toml_edit::Value;
    let copy = Copy::of("pepelnaya-marka");
    let mut ws = Workspace::open(&copy.0).unwrap();
    // The package as the edits leave it in memory (the disk does not have them until a save).
    fn pkg_now(ws: &mut Workspace) -> Arc<crate::pack::load::Package> {
        ws.rebuild().unwrap();
        ws.package().expect("the package parses")
    }
    let base_errors = ws.check().unwrap().0;
    let snapshot = |ws: &Workspace| -> Vec<(String, String)> {
        let mut files: Vec<String> = ws.entries.iter().filter(|p| p.ends_with(".toml")).cloned().collect();
        files.sort();
        files.into_iter().map(|p| {
            let t = ws.text_of(&p).unwrap();
            (p, t)
        }).collect()
    };
    let before = snapshot(&ws);
    let check_now = |ws: &mut Workspace, what: &str| {
        let errors = ws.check().unwrap().0;
        assert_eq!(errors, base_errors, "{what}: no new check error");
        assert!(ws.package().is_some(), "{what}: the package parses");
    };

    // 1. A trigger's first action changes kind (`dialog` to `give_gold`) and its `once` is off.
    let file = "triggers/brod.toml";
    let text = ws.text_of(file).unwrap();
    let draft = st::trigger_draft(&text, "arrival").expect("the arrival trigger reads");
    let pkg = pkg_now(&mut ws);
    let opts = Options::of(&pkg, "brod");
    let mut rows = draft.then.clone();
    let mut gold = st::new_row(Section::Action, "give_gold", &opts, None).unwrap();
    gold.insert("give_gold", Value::from(5_i64));
    rows[0] = gold;
    ws.apply_doc(file, "change action", &st::set_trigger_list("arrival", "then", &rows, true)).unwrap();
    ws.apply_doc(file, "once off", &[st::set_trigger_key("arrival", "once", Some(Value::from(false)))]).unwrap();
    let edited = ws.text_of(file).unwrap();
    assert!(edited.contains("give_gold = 5") && edited.contains("once = false"), "{edited}");
    check_now(&mut ws, "a trigger's action and field");

    // 2. A dialog gets a choice: its title and text keys in every language, one undo step.
    let pkg = pkg_now(&mut ws);
    let (cid, steps) = st::add_choice_steps(&pkg, "toll").expect("toll has room for a choice");
    ws.begin_group();
    for s in &steps {
        if let Step::Edit(f, e) = s {
            ws.apply_doc(f, "add a choice", e).unwrap();
        }
    }
    ws.end_group("add a choice");
    let pkg = pkg_now(&mut ws);
    let toll = pkg.dialogs.iter().find(|d| d.value.id == "toll").unwrap();
    assert_eq!(toll.value.choices.last().map(|c| c.id.as_str()), Some(cid.as_str()), "the choice is the last one");
    check_now(&mut ws, "a dialog's new choice");

    // 3. A choice nothing answers is wired to a dialog: its own action opens the target.
    let pkg = pkg_now(&mut ws);
    let listened = |d: &str, c: &str| pkg.triggers.iter().any(|t| matches!(&t.value.when, pt::Event::DialogChoice { dialog, choice } if dialog == d && choice == c));
    let (dialog, choice) = pkg
        .dialogs
        .iter()
        .flat_map(|d| d.value.choices.iter().map(move |c| (d.value.id.clone(), c.id.clone())))
        .find(|(d, c)| !listened(d, c))
        .expect("a choice nothing answers");
    let target = pkg.dialogs.iter().map(|d| d.value.id.clone()).find(|d| *d != dialog).unwrap();
    let steps = st::wire_choice(&pkg, &|f| ws.text_of(f).ok(), "brod", &dialog, &choice, &target).expect("wired");
    ws.begin_group();
    for s in &steps {
        if let Step::Edit(f, e) = s {
            ws.apply_doc(f, "wire a choice", e).unwrap();
        }
    }
    ws.end_group("wire a choice to a dialog");
    let pkg = pkg_now(&mut ws);
    let wired = pkg.dialogs.iter().find(|d| d.value.id == dialog).unwrap().value.choices.iter().find(|c| c.id == choice).unwrap();
    assert!(wired.then.iter().any(|a| matches!(a, pt::Action::Dialog(d) if *d == target)), "the choice opens {target}");
    check_now(&mut ws, "a choice wired to a dialog");

    // 4. A choice a trigger answers is wired: the action goes on a new `dialog_choice` trigger.
    let pkg = pkg_now(&mut ws);
    let steps = st::wire_choice(&pkg, &|f| ws.text_of(f).ok(), "brod", "varga_meet", "join", "marsh_plea").expect("wired through a trigger");
    let triggers_before = pkg.triggers.len();
    ws.begin_group();
    for s in &steps {
        if let Step::Edit(f, e) = s {
            ws.apply_doc(f, "wire a choice", e).unwrap();
        }
    }
    ws.end_group("wire a choice to a dialog");
    let pkg = pkg_now(&mut ws);
    assert_eq!(pkg.triggers.len(), triggers_before + 1, "one new trigger");
    let new = pkg.triggers.iter().find(|t| t.value.id == "varga_meet_join_to_marsh_plea").expect("the wired trigger is named by its choice and target");
    assert!(matches!(&new.value.when, pt::Event::DialogChoice { dialog, choice } if dialog == "varga_meet" && choice == "join"));
    assert!(matches!(new.value.then.as_slice(), [pt::Action::Dialog(d)] if d == "marsh_plea"));
    check_now(&mut ws, "a choice wired through a trigger");
    let wiring_steps = 1;
    let after_wire = ws.history.undo_label().map(str::to_string);
    assert_eq!(after_wire.as_deref(), Some("wire a choice to a dialog"), "the wiring is one undo step");
    let _ = wiring_steps;

    // 5. A variable is renamed in every file that names it.
    let pkg = pkg_now(&mut ws);
    let plan = vars::rename_plan(&pkg, "dread", "fear", &|f| ws.text_of(f).ok()).expect("the plan");
    assert!(plan.len() >= 2, "triggers and dialogs name dread: {}", plan.len());
    ws.set_texts("rename dread to fear", plan).unwrap();
    let pkg = pkg_now(&mut ws);
    assert!(pkg.vars.iter().any(|v| v.value.name == "fear") && !pkg.vars.iter().any(|v| v.value.name == "dread"));
    check_now(&mut ws, "a renamed variable");
    assert!(vars::variable_uses(&pkg, "fear").len() > 1, "its uses follow it");

    // 6. The layout is written through the guarded write; the package is saved and read back.
    let mut saved = std::collections::BTreeMap::new();
    saved.insert("t:arrival".to_string(), [40.0_f32, 60.0]);
    let bytes = story_graph::layout_json(None, "brod", &saved);
    ws.write_editor_file(campaign_graph::LAYOUT, &bytes).unwrap();
    assert_eq!(story_graph::read_layout(Some(&bytes), "brod")["t:arrival"], [40.0, 60.0]);
    ws.save_all().unwrap();
    let on_disk = fs::read_to_string(copy.0.join("triggers/brod.toml")).unwrap();
    assert_eq!(on_disk, ws.text_of("triggers/brod.toml").unwrap(), "the save writes the file");

    // Undo every step: each text is back, byte for byte.
    let mut steps_undone = 0;
    while ws.undo().unwrap().is_some() {
        steps_undone += 1;
    }
    assert_eq!(steps_undone, 6, "each change is one undo step (two edits, three grouped operations, the rename)");
    assert_eq!(snapshot(&ws), before, "undo restores every text");
}
