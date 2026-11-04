//! Health's checks: the findings of the example maps on the install, and the dead ends and traps
//! the checks read from the files (a trigger and a dialog added in memory to a copy of the example
//! package, never written to disk). The fights and the budget need the install (`RAZDOR_DT_DIR`).

use super::*;
use crate::pack::files::Files;
use crate::pack::load::Package;
use std::path::Path;

fn example(name: &str) -> Package {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs").join(name);
    Package::parse(Files::from_dir(&root).expect("the example reads")).0
}

/// The example package with extra files over it (path, text).
fn with(name: &str, extra: &[(&str, &str)]) -> Package {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs").join(name);
    let mut files = Files::from_dir(&root).expect("the example reads");
    for (path, text) in extra {
        files.overlay(path, text.as_bytes().to_vec()).expect("a package path");
    }
    Package::parse(files).0
}

fn install() -> Option<DtInstall> {
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    Some(DtInstall::load(Path::new(&dir)).expect("install loads"))
}

fn kinds(h: &Health) -> Vec<&'static str> {
    h.findings.iter().map(|f| f.kind).collect()
}

#[test]
fn the_summary_takes_ten_for_a_warning_and_two_for_a_note() {
    assert_eq!(WARNING_COST, 10);
    assert_eq!(NOTE_COST, 2);
    let f = |severity| Finding::new("x", severity, "x", "", "", Vec::new());
    let findings = vec![f(Severity::Warning), f(Severity::Warning), f(Severity::Info)];
    assert_eq!(summary(&findings), 78);
    let many: Vec<Finding> = (0..11).map(|_| f(Severity::Warning)).collect();
    assert_eq!(summary(&many), 0, "never below 0");
    assert_eq!(summary(&[]), 100);
}

#[test]
fn the_cache_key_changes_with_the_content_it_names() {
    let a = with("pepelnaya-marka", &[]);
    let b = with("pepelnaya-marka", &[("units/zz_test.toml", "# a unit file\n")]);
    let prefixes = ["maps/brod/".to_string(), "units/".to_string()];
    assert_ne!(content_hash(&b.files, &prefixes), content_hash(&a.files, &prefixes), "a unit file changes it");
    assert_eq!(content_hash(&a.files, &prefixes), content_hash(&a.files, &prefixes), "the same content gives the same key");
    assert_eq!(content_hash(&b.files, &["maps/brod/".to_string()]), content_hash(&a.files, &["maps/brod/".to_string()]), "a file outside the prefixes does not");
}

/// A once-only trigger that opens a dialog with a «later» answer, and no trigger opens it again: the
/// dialog is never offered again (the guide's «a council with a later answer that fires once»).
#[test]
fn a_later_answer_that_never_comes_back_is_a_dead_end() {
    let pkg = with(
        "pepelnaya-marka",
        &[
            ("triggers/zz_test.toml", "[[trigger]]\nid = \"zz_once\"\nmap = \"brod\"\nwhen = { event = \"game_start\" }\nonce = true\nthen = [ { dialog = \"zz_lonely\" } ]\n"),
            ("dialogs/zz_test.toml", "[[dialog]]\nid = \"zz_lonely\"\ntitle = \"t\"\ntext = \"t\"\n\n[[dialog.choice]]\nid = \"now\"\ntext = \"t\"\n\n[[dialog.choice]]\nid = \"later\"\ntext = \"t\"\n"),
        ],
    );
    let mut findings = Vec::new();
    dead_ends(&pkg, "brod", &mut findings);
    assert!(findings.iter().any(|f| f.kind == "later_never_returns" && f.name == "zz_lonely"), "{findings:?}");
    // A dialog a non-once trigger opens again is offered each day: no dead end.
    let pkg = with(
        "pepelnaya-marka",
        &[
            ("triggers/zz_test.toml", "[[trigger]]\nid = \"zz_once\"\nmap = \"brod\"\nwhen = { event = \"game_start\" }\nonce = true\nthen = [ { dialog = \"zz_lonely\" } ]\n\n[[trigger]]\nid = \"zz_again\"\nmap = \"brod\"\nwhen = { event = \"day_start\" }\nonce = false\nthen = [ { dialog = \"zz_lonely\" } ]\n"),
            ("dialogs/zz_test.toml", "[[dialog]]\nid = \"zz_lonely\"\ntitle = \"t\"\ntext = \"t\"\n\n[[dialog.choice]]\nid = \"now\"\ntext = \"t\"\n\n[[dialog.choice]]\nid = \"later\"\ntext = \"t\"\n"),
        ],
    );
    let mut findings = Vec::new();
    dead_ends(&pkg, "brod", &mut findings);
    assert!(!findings.iter().any(|f| f.name == "zz_lonely"), "{findings:?}");
}

/// A once-only trigger gated on gold that opens an offer: short of gold when it fires, it is lost.
#[test]
fn a_once_offer_gated_on_gold_is_a_dead_end() {
    let pkg = with("pepelnaya-marka", &[("triggers/zz_test.toml", "[[trigger]]\nid = \"zz_gold\"\nmap = \"brod\"\nwhen = { event = \"hero_enters\", at = [1, 1, 2, 2] }\nif = [ { gold = 100, op = \">=\" } ]\nonce = true\nthen = [ { dialog = \"zz_offer\" } ]\n")]);
    let mut findings = Vec::new();
    dead_ends(&pkg, "brod", &mut findings);
    assert!(findings.iter().any(|f| f.kind == "once_offer_gold" && f.name == "zz_gold"), "{findings:?}");
}

/// Each example map gets findings with where they are, a summary in 0 to 100, and the same result
/// from the cache (the fights are not run again). Skipped without `RAZDOR_DT_DIR`.
#[test]
fn the_example_maps_are_checked_on_the_install() {
    let Some(dt) = install() else { return };
    let pkg = example("pepelnaya-marka");
    let mut cache = Cache::default();
    for map in ["brod", "marka"] {
        let h = health(map, &pkg, &dt, "ru", &mut cache).unwrap_or_else(|e| panic!("{map}: {e}"));
        println!("HEALTH {map}: summary {} findings {}", h.summary, h.findings.len());
        for f in &h.findings {
            println!("HEALTH   {:?} {} {} [{}] {:?}", f.severity, f.kind, f.name, f.figures, f.targets.first());
        }
        assert!((0..=100).contains(&h.summary));
        assert!(h.findings.iter().all(|f| !f.kind.is_empty()));
        let again = health(map, &pkg, &dt, "ru", &mut cache).unwrap();
        assert_eq!(again, h, "the cached fights give the same findings");
        assert!(kinds(&h).iter().all(|k| !k.is_empty()));
    }
}

/// A map that is not on the package is refused with its name, not checked.
#[test]
fn an_unknown_map_is_refused() {
    let Some(dt) = install() else { return };
    let pkg = example("pepelnaya-marka");
    assert!(health("no-such-map", &pkg, &dt, "ru", &mut Cache::default()).unwrap_err().contains("no-such-map"));
}

