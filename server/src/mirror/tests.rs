//! The mirror's hermetic tests: a fake `Net` (fixture JSON and file bodies), a temporary root and stage.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{json, Value};

use super::config::MirrorConfig;
use super::github::{self, Asset, Release};
use super::net::{Net, Reply};
use super::store;
use super::*;

/// Fixed replies (the API, the config), files that can be downloaded, and a log of what was asked.
#[derive(Default)]
struct Fake {
    gets: RefCell<HashMap<String, Reply>>,
    files: RefCell<HashMap<String, Vec<u8>>>,
    redirects: RefCell<HashMap<String, Reply>>,
    /// Attempts that write half of the rest and then fail.
    fail: RefCell<HashSet<String>>,
    api_seen: RefCell<Vec<String>>,
    downloads: RefCell<Vec<(String, u64)>>,
}

impl Fake {
    fn file(&self, url: &str, body: &[u8]) {
        self.files.borrow_mut().insert(url.to_string(), body.to_vec());
    }
    fn json(&self, url: &str, v: &Value) {
        self.gets.borrow_mut().insert(url.to_string(), Reply { status: 200, headers: vec![], body: v.to_string().into_bytes() });
    }
    fn downloads_of(&self, url: &str) -> Vec<u64> {
        self.downloads.borrow().iter().filter(|(u, _)| u == url).map(|(_, o)| *o).collect()
    }
}

impl Net for Fake {
    fn get(&self, url: &str, headers: &[(String, String)]) -> Result<Reply, String> {
        if url.starts_with("https://api.github.com/") {
            self.api_seen.borrow_mut().push(url.to_string());
        }
        if let Some(r) = self.gets.borrow().get(url) {
            let etag = r.header("etag");
            let asked = headers.iter().find(|(k, _)| k == "If-None-Match").map(|(_, v)| v.as_str());
            if etag.is_some() && etag == asked {
                return Ok(Reply { status: 304, headers: vec![], body: vec![] });
            }
            return Ok(r.clone());
        }
        if let Some(b) = self.files.borrow().get(url) {
            return Ok(Reply { status: 200, headers: vec![], body: b.clone() });
        }
        Ok(Reply { status: 404, headers: vec![], body: vec![] })
    }

    fn redirect(&self, url: &str) -> Result<Reply, String> {
        self.redirects.borrow().get(url).cloned().ok_or_else(|| "no redirect".to_string())
    }

    fn download(&self, url: &str, offset: u64, dest: &Path) -> Result<u16, String> {
        self.downloads.borrow_mut().push((url.to_string(), offset));
        let body = self.files.borrow().get(url).cloned().ok_or("no such file")?;
        let (status, start) = if offset > 0 { (206, offset as usize) } else { (200, 0) };
        let rest = &body[start.min(body.len())..];
        let mut f = if offset > 0 {
            OpenOptions::new().append(true).create(true).open(dest).map_err(|e| e.to_string())?
        } else {
            OpenOptions::new().create(true).write(true).truncate(true).open(dest).map_err(|e| e.to_string())?
        };
        if self.fail.borrow().contains(url) {
            f.write_all(&rest[..rest.len() / 2]).map_err(|e| e.to_string())?;
            return Err("stalled".into());
        }
        f.write_all(rest).map_err(|e| e.to_string())?;
        Ok(status)
    }
}

/// A temporary base folder, removed on drop.
struct Base(PathBuf);

impl Drop for Base {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn setup(name: &str) -> (Base, MirrorConfig) {
    let base = std::env::temp_dir().join(format!("razdor-mirror-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    // The free-space floor is off here: the test disk's free space is not the point of these tests.
    let cfg = MirrorConfig {
        root: base.join("hub"),
        stage: base.join("stage"),
        min_free_bytes: 0,
        ..MirrorConfig::default()
    };
    (Base(base), cfg)
}

fn now() -> SystemTime {
    SystemTime::now()
}

/// A GitHub release object for the fake: its assets are served from `files`, with their digests. A
/// `SHA256SUMS` asset is added for app releases.
fn release(fake: &Fake, repo: &str, tag: &str, assets: &[(&str, &[u8])], sums: bool) -> Value {
    let mut all: Vec<(String, Vec<u8>)> = assets.iter().map(|(n, b)| (n.to_string(), b.to_vec())).collect();
    if sums {
        let body: String = assets.iter().map(|(n, b)| format!("{}  {n}\n", store::sha256_bytes(b))).collect();
        all.push(("SHA256SUMS".into(), body.into_bytes()));
    }
    let items: Vec<Value> = all
        .iter()
        .map(|(name, body)| {
            let url = format!("https://github.com/indicozy/{repo}/releases/download/{tag}/{name}");
            fake.file(&url, body);
            json!({
                "name": name,
                "browser_download_url": url,
                "size": body.len(),
                "digest": format!("sha256:{}", store::sha256_bytes(body)),
            })
        })
        .collect();
    json!({"tag_name": tag, "draft": false, "prerelease": false, "assets": items})
}

fn catalog_index() -> Vec<u8> {
    br#"{
  "format": 2,
  "game": {
    "url": "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip",
    "sha256": "00"
  },
  "entries": [],
  "ad": {"image": "https://github.com/indicozy/razdor-community/releases/download/catalog-2026-11-24-1/capsule.png"}
}
"#
    .to_vec()
}

/// The fixture world: the config, one app release with its files, a catalog with game, ad and depth model.
fn world(fake: &Fake) {
    fake.gets.borrow_mut().insert(CONFIG_URL.into(), Reply { status: 200, headers: vec![], body: br#"{"schema":1}"#.to_vec() });
    fake.redirects.borrow_mut().insert(
        CATALOG_LATEST.into(),
        Reply {
            status: 302,
            headers: vec![(
                "location".into(),
                "https://github.com/indicozy/razdor-community/releases/download/catalog-2026-11-24-1/index.json".into(),
            )],
            body: vec![],
        },
    );
    let app = release(
        fake,
        "razdor",
        "v1.6.0",
        &[
            ("razdor-1.6.0-linux", b"linux bytes"),
            ("razdor", b"linux bytes"),
            ("razdor-translate-1.6.0-linux", b"translate"),
            ("razdor-ads-1.6.0.zip", b"ads zip"),
            ("razdor.apk", b"apk"),
            ("razdor-1.6.0.apk", b"apk"),
        ],
        true,
    );
    let index = catalog_index();
    let catalog = release(
        fake,
        "razdor-community",
        "catalog-2026-11-24-1",
        &[("index.json", &index), ("capsule.png", b"capsule")],
        false,
    );
    let game = release(fake, "razdor-community", "game-cu-1.2", &[("discord-times-cu.zip", b"game archive")], false);
    let depth = release(fake, "razdor-community", DEPTH_TAG, &[("depth.onnx", b"model")], false);
    fake.json(APP_API, &json!([app]));
    fake.json(HUB_API, &json!([catalog, game, depth]));
}

fn feed_tags(cfg: &MirrorConfig) -> Vec<String> {
    let bytes = fs::read(cfg.root.join("pages/razdor-community/releases.json")).unwrap();
    let v: Value = serde_json::from_slice(&bytes).unwrap();
    v.as_array().unwrap().iter().map(|r| r["tag_name"].as_str().unwrap().to_string()).collect()
}

#[test]
fn a_complete_release_is_published_whole_with_the_files_it_keeps() {
    let (_base, cfg) = setup("whole");
    let fake = Fake::default();
    world(&fake);
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    let dir = store::tag_dir(&cfg.root, "razdor", "v1.6.0");
    let mut names: Vec<String> = fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into()).collect();
    names.sort();
    assert_eq!(names, vec!["SHA256SUMS", "razdor-1.6.0-linux", "razdor-1.6.0.apk", "razdor-ads-1.6.0.zip"]);
    assert_eq!(fs::read(dir.join("razdor-ads-1.6.0.zip")).unwrap(), b"ads zip");
    assert!(store::tag_dir(&cfg.root, "razdor-community", "game-cu-1.2").join("discord-times-cu.zip").exists());
    assert!(store::tag_dir(&cfg.root, "razdor-community", DEPTH_TAG).join("depth.onnx").exists());
    assert_eq!(out.app, vec!["v1.6.0".to_string()]);
    assert_eq!(out.catalog.as_deref(), Some("catalog-2026-11-24-1"));
}

#[test]
fn the_sync_makes_two_api_calls_a_run_and_writes_status() {
    let (_base, cfg) = setup("budget");
    let fake = Fake::default();
    world(&fake);
    let out = run(&cfg, &fake, now()).unwrap();
    assert_eq!(out.api_calls, 2);
    assert_eq!(fake.api_seen.borrow().len(), 2);
    let status: Value = serde_json::from_slice(&fs::read(cfg.root.join("status.json")).unwrap()).unwrap();
    assert_eq!(status["app"], json!(["v1.6.0"]));
    assert_eq!(status["catalog"], json!("catalog-2026-11-24-1"));
    assert_eq!(status["ok"], json!(true));
    assert!(status["synced_at"].as_str().unwrap().ends_with('Z'));
    // A second run finds everything on disk: no download, the lists only (a 304 costs no bytes).
    let before = fake.downloads.borrow().len();
    run(&cfg, &fake, now()).unwrap();
    assert_eq!(fake.downloads.borrow().len(), before);
}

#[test]
fn a_wrong_digest_publishes_nothing() {
    let (_base, cfg) = setup("wrong");
    let fake = Fake::default();
    world(&fake);
    let url = "https://github.com/indicozy/razdor/releases/download/v1.6.0/razdor-1.6.0-linux";
    fake.file(url, b"tampered");
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(!out.errors.is_empty());
    assert!(!store::tag_dir(&cfg.root, "razdor", "v1.6.0").exists());
    assert!(out.app.is_empty());
    assert!(!cfg.stage.join("razdor/v1.6.0/razdor-1.6.0-linux").exists(), "the bad file is not kept");
}

#[test]
fn a_file_without_a_hash_is_not_served() {
    let (_base, cfg) = setup("nohash");
    let fake = Fake::default();
    world(&fake);
    let app = release(&fake, "razdor", "v1.6.0", &[("razdor-1.6.0-linux", b"linux bytes")], false);
    // Drop the digest and the SHA256SUMS entry: nothing says what the file must be.
    let mut app = app;
    app["assets"][0].as_object_mut().unwrap().remove("digest");
    fake.json(APP_API, &json!([app]));
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(out.errors.iter().any(|e| e.contains("no SHA-256")), "{:?}", out.errors);
    assert!(!store::tag_dir(&cfg.root, "razdor", "v1.6.0").exists());
    assert!(out.app.is_empty());
}

#[test]
fn the_feed_lists_only_complete_releases() {
    let (_base, cfg) = setup("feed");
    let fake = Fake::default();
    world(&fake);
    let bad = release(&fake, "razdor", "v1.5.0", &[("razdor-1.5.0-linux", b"old")], true);
    fake.file("https://github.com/indicozy/razdor/releases/download/v1.5.0/razdor-1.5.0-linux", b"changed");
    let good = {
        let gets = fake.gets.borrow();
        let r: Value = serde_json::from_slice(&gets[APP_API].body).unwrap();
        r[0].clone()
    };
    fake.json(APP_API, &json!([good, bad]));
    let cfg = MirrorConfig { keep_app_releases: 3, ..cfg };
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(!out.errors.is_empty());
    assert_eq!(feed_tags(&cfg), vec!["v1.6.0".to_string()]);
}

#[test]
fn latest_moves_only_when_the_catalog_and_its_files_are_on_disk() {
    let (_base, cfg) = setup("latest");
    let fake = Fake::default();
    world(&fake);
    // The game archive is withdrawn from GitHub's file server: the catalog names it, so it is not published.
    fake.fail.borrow_mut().insert("https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip".into());
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(!out.errors.is_empty());
    assert!(!store::latest_link(&cfg.root, "razdor-community").exists(), "latest stays until the catalog is complete");
    assert!(!store::tag_dir(&cfg.root, "razdor-community", "catalog-2026-11-24-1").exists());
    assert_eq!(out.catalog, None);
}

#[test]
fn a_killed_run_leaves_nothing_visible_and_resumes() {
    let (_base, cfg) = setup("resume");
    let fake = Fake::default();
    world(&fake);
    let url = "https://github.com/indicozy/razdor/releases/download/v1.6.0/razdor-1.6.0-linux";
    fake.fail.borrow_mut().insert(url.into());
    run(&cfg, &fake, now()).unwrap();
    assert!(!store::tag_dir(&cfg.root, "razdor", "v1.6.0").exists(), "nothing half served");
    assert!(feed_tags(&cfg).is_empty());
    let part = cfg.stage.join("razdor/v1.6.0/razdor-1.6.0-linux.part");
    assert!(part.exists(), "the partial file waits in the stage");

    fake.fail.borrow_mut().clear();
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert!(store::tag_dir(&cfg.root, "razdor", "v1.6.0").join("razdor-1.6.0-linux").exists());
    assert!(fake.downloads_of(url).iter().any(|o| *o > 0), "the second run resumed with a Range request");
}

#[test]
fn same_digest_files_are_hard_linked() {
    let (_base, cfg) = setup("link");
    let fake = Fake::default();
    world(&fake);
    let app = release(&fake, "razdor", "v1.6.0", &[("razdor-1.6.0-linux", b"same"), ("razdor-1.6.0-copy", b"same")], true);
    fake.json(APP_API, &json!([app]));
    run(&cfg, &fake, now()).unwrap();
    let dir = store::tag_dir(&cfg.root, "razdor", "v1.6.0");
    let a = fs::metadata(dir.join("razdor-1.6.0-linux")).unwrap();
    let b = fs::metadata(dir.join("razdor-1.6.0-copy")).unwrap();
    assert_eq!(a.ino(), b.ino(), "one file, two names");
    assert!(fake.downloads_of("https://github.com/indicozy/razdor/releases/download/v1.6.0/razdor-1.6.0-copy").is_empty());
}

#[test]
fn prune_keeps_n_releases_and_the_previous_catalog() {
    let (_base, cfg) = setup("prune");
    let fake = Fake::default();
    world(&fake);
    let cfg = MirrorConfig { keep_app_releases: 1, ..cfg };
    let old_app = store::tag_dir(&cfg.root, "razdor", "v1.5.0");
    fs::create_dir_all(&old_app).unwrap();
    fs::write(old_app.join("razdor-1.5.0-linux"), b"old").unwrap();
    let previous = store::tag_dir(&cfg.root, "razdor-community", "catalog-2026-11-23-1");
    fs::create_dir_all(&previous).unwrap();
    let older = store::tag_dir(&cfg.root, "razdor-community", "catalog-2026-11-20-1");
    fs::create_dir_all(&older).unwrap();
    fs::create_dir_all(cfg.root.join("pages")).unwrap();
    store::write_atomic(&cfg.root.join("status.json"), br#"{"catalog_history":["catalog-2026-11-23-1"]}"#).unwrap();
    let app_list = {
        let gets = fake.gets.borrow();
        let r: Value = serde_json::from_slice(&gets[APP_API].body).unwrap();
        json!([r[0].clone(), {"tag_name": "v1.5.0", "draft": false, "prerelease": false, "assets": []}])
    };
    fake.json(APP_API, &app_list);
    run(&cfg, &fake, now()).unwrap();
    assert!(store::tag_dir(&cfg.root, "razdor", "v1.6.0").exists());
    assert!(!old_app.exists(), "the release beyond keep_app_releases is pruned");
    assert!(store::tag_dir(&cfg.root, "razdor-community", "catalog-2026-11-24-1").exists());
    assert!(previous.exists(), "the previous catalog stays");
    assert!(!older.exists(), "a catalog beyond the history is pruned");
    assert!(store::tag_dir(&cfg.root, "razdor-community", DEPTH_TAG).exists(), "the depth model stays");
}

#[test]
fn the_disk_cap_downloads_nothing() {
    let (_base, cfg) = setup("cap");
    let fake = Fake::default();
    world(&fake);
    let cfg = MirrorConfig { max_bytes: 4, ..cfg };
    let err = run(&cfg, &fake, now()).unwrap_err();
    assert!(err.contains("disk cap"), "{err}");
    // Only the catalog's index.json (needed to know the game and the ad) came down before the check.
    let index_url = "https://github.com/indicozy/razdor-community/releases/download/catalog-2026-11-24-1/index.json";
    assert!(fake.downloads.borrow().iter().all(|(u, _)| u == index_url));
    assert!(!store::tag_dir(&cfg.root, "razdor", "v1.6.0").exists());
}

#[test]
fn a_rate_limited_api_ends_the_run_quietly() {
    let (_base, cfg) = setup("rate");
    let fake = Fake::default();
    world(&fake);
    fake.gets.borrow_mut().insert(
        APP_API.into(),
        Reply { status: 403, headers: vec![("x-ratelimit-remaining".into(), "0".into())], body: vec![] },
    );
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(out.rate_limited);
    assert!(fake.downloads.borrow().is_empty());
    assert!(!cfg.root.join("status.json").exists(), "a run that did nothing writes no status");
}

#[test]
fn a_withdrawn_release_leaves_the_feed_and_the_disk() {
    let (_base, cfg) = setup("withdrawn");
    let fake = Fake::default();
    world(&fake);
    run(&cfg, &fake, now()).unwrap();
    assert_eq!(feed_tags(&cfg), vec!["v1.6.0".to_string()]);
    fake.json(APP_API, &json!([]));
    run(&cfg, &fake, now()).unwrap();
    assert!(feed_tags(&cfg).is_empty());
    assert!(!store::tag_dir(&cfg.root, "razdor", "v1.6.0").exists());
}

#[test]
fn an_etag_answers_304_from_the_cached_list() {
    let (_base, cfg) = setup("etag");
    let fake = Fake::default();
    world(&fake);
    let mut app = fake.gets.borrow()[APP_API].clone();
    app.headers.push(("etag".into(), "\"v1\"".into()));
    fake.gets.borrow_mut().insert(APP_API.into(), app);
    run(&cfg, &fake, now()).unwrap();
    let out = run(&cfg, &fake, now()).unwrap();
    assert!(out.errors.is_empty(), "{:?}", out.errors);
    assert_eq!(out.app, vec!["v1.6.0".to_string()]);
}

#[test]
fn the_stage_must_share_the_root_filesystem() {
    let other = Path::new("/dev/shm");
    let tmp = std::env::temp_dir();
    if !other.is_dir() || fs::metadata(other).unwrap().dev() == fs::metadata(&tmp).unwrap().dev() {
        return; // no second filesystem to test against
    }
    let (_base, mut cfg) = setup("device");
    cfg.stage = other.join(format!("razdor-mirror-test-{}", std::process::id()));
    let fake = Fake::default();
    world(&fake);
    let err = run(&cfg, &fake, now()).unwrap_err();
    let _ = fs::remove_dir_all(&cfg.stage);
    assert!(err.contains("same filesystem"), "{err}");
}

#[test]
fn a_stage_run_that_is_locked_does_nothing() {
    let (_base, cfg) = setup("lock");
    fs::create_dir_all(&cfg.stage).unwrap();
    let held = store::lock(&cfg.stage.join(".lock")).unwrap().expect("first lock");
    assert!(store::lock(&cfg.stage.join(".lock")).unwrap().is_none(), "a second run gets no lock");
    drop(held);
    assert!(store::lock(&cfg.stage.join(".lock")).unwrap().is_some());
}

// --- The pure parts ---

#[test]
fn the_wanted_names_of_1_6_0_are_the_real_ones() {
    // The asset names of GitHub's razdor v1.6.0 (public release data).
    let names = [
        "razdor", "razdor-1.6.0-linux", "razdor-1.6.0-macos", "razdor-1.6.0-windows7.zip", "razdor-1.6.0.apk",
        "Razdor-1.6.0.exe", "razdor-1.6.0.ipa", "razdor-ads-1.6.0.zip", "razdor-macos", "razdor-translate",
        "razdor-translate-1.6.0-linux", "razdor-translate-1.6.0-macos", "Razdor-translate-1.6.0.exe",
        "razdor-translate-macos", "Razdor-translate.exe", "Razdor-win7.exe", "razdor.apk", "Razdor.exe",
        "razdor.ipa", "SHA256SUMS",
    ];
    let assets: Vec<Asset> = names
        .iter()
        .map(|n| Asset { name: n.to_string(), url: format!("https://x/{n}"), size: 1, digest: None })
        .collect();
    let rel = Release { tag: "v1.6.0".into(), draft: false, prerelease: false, assets, raw: Value::Null };
    let cfg = MirrorConfig::default();
    let mut kept: Vec<&str> = github::wanted(&rel, &cfg).iter().map(|a| a.name.as_str()).collect();
    kept.sort();
    assert_eq!(
        kept,
        vec![
            "Razdor-1.6.0.exe",
            "Razdor-win7.exe",
            "SHA256SUMS",
            "razdor-1.6.0-linux",
            "razdor-1.6.0-macos",
            "razdor-1.6.0-windows7.zip",
            "razdor-1.6.0.apk",
            "razdor-ads-1.6.0.zip",
        ]
    );
}

#[test]
fn own_copies_are_for_the_owners_two_repositories_only() {
    let own = "https://razdor.indicozy.dev/hub";
    assert_eq!(
        github::own_copy("https://github.com/indicozy/razdor/releases/download/v1.6.0/x.apk", own).as_deref(),
        Some("https://razdor.indicozy.dev/hub/indicozy/razdor/releases/download/v1.6.0/x.apk")
    );
    assert_eq!(
        github::own_copy("https://indicozy.github.io/razdor/config.json", own).as_deref(),
        Some("https://razdor.indicozy.dev/hub/pages/razdor/config.json")
    );
    assert_eq!(github::own_copy("https://github.com/indicozy/other/releases/x", own), None);
    assert_eq!(github::own_copy("https://github.com/someone/razdor/x", own), None);
    assert_eq!(github::own_copy("https://gitverse.ru/api/attachments/1", own), None);
}

#[test]
fn the_catalog_rewrite_changes_only_the_game_url_and_its_mirrors() {
    let own = "https://razdor.indicozy.dev/hub";
    let src = "{\n  \"game\": {\n    \"url\": \"https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip\",\n    \"sha256\": \"00\"\n  },\n  \"entries\": [1, {\"a\": \"b\"}]\n}\n";
    let out = github::rewrite_catalog(src.as_bytes(), own).unwrap();
    let gh = "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip";
    let mine = "https://razdor.indicozy.dev/hub/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip";
    // The url value is replaced, and the mirror list follows it on the same line; nothing else moves.
    let expected = src.replacen(&format!("\"{gh}\""), &format!("\"{mine}\",\"mirrors\":[\"{gh}\"]"), 1);
    assert_eq!(String::from_utf8(out).unwrap(), expected);
}

#[test]
fn the_catalog_rewrite_keeps_an_existing_mirror_list_and_refuses_a_foreign_game() {
    let own = "https://razdor.indicozy.dev/hub";
    let gh = "https://github.com/indicozy/razdor-community/releases/download/game-cu-1.2/discord-times-cu.zip";
    let src = format!("{{\"game\":{{\"url\":\"{gh}\",\"mirrors\":[\"https://gitverse.ru/x\"]}}}}");
    let out: Value = serde_json::from_slice(&github::rewrite_catalog(src.as_bytes(), own).unwrap()).unwrap();
    assert_eq!(out["game"]["mirrors"], json!([gh, "https://gitverse.ru/x"]));
    let foreign = br#"{"game":{"url":"https://example.org/game.zip"}}"#;
    assert!(github::rewrite_catalog(foreign, own).is_err());
    assert!(github::rewrite_catalog(br#"{"entries":[]}"#, own).is_err());
}

#[test]
fn a_catalog_names_its_releases_by_github_or_own_address() {
    let own = "https://razdor.indicozy.dev/hub";
    let idx = br#"{"game":{"url":"https://razdor.indicozy.dev/hub/indicozy/razdor-community/releases/download/game-cu-1.2/x.zip"},"ad":{"image":"https://github.com/indicozy/razdor-community/releases/download/catalog-1/capsule.png"}}"#;
    let refs = github::referenced_tags(idx, own);
    assert_eq!(refs, vec![("razdor-community".to_string(), "game-cu-1.2".to_string()), ("razdor-community".to_string(), "catalog-1".to_string())]);
}

#[test]
fn the_helpers_read_digests_sums_and_locations() {
    let hex = "ab".repeat(32);
    assert_eq!(github::digest_hex(&format!("sha256:{hex}")), Some(hex.clone()));
    assert_eq!(github::digest_hex("md5:00"), None);
    let sums = format!("{hex}  razdor-1.6.0-linux\n{hex} *Razdor.exe\n");
    let parsed = github::parse_sums(sums.as_bytes());
    assert_eq!(parsed, vec![("razdor-1.6.0-linux".to_string(), hex.clone()), ("Razdor.exe".to_string(), hex.clone())]);
    assert_eq!(
        github::tag_from_location("https://github.com/indicozy/razdor-community/releases/download/catalog-2026-11-24-1/index.json").as_deref(),
        Some("catalog-2026-11-24-1")
    );
    assert_eq!(github::tag_from_location("https://example.org/"), None);
    assert_eq!(
        github::release_of_url("https://github.com/indicozy/razdor/releases/download/v1.6.0/x"),
        Some(("razdor".to_string(), "v1.6.0".to_string()))
    );
    assert_eq!(github::release_of_url("https://github.com/indicozy/other/releases/download/v1/x"), None);
}

#[test]
fn the_config_must_be_schema_one() {
    assert!(github::check_config(br#"{"schema":1,"x":2}"#).is_ok());
    assert!(github::check_config(br#"{"schema":2}"#).is_err());
    assert!(github::check_config(b"<html>").is_err());
}

#[test]
fn rfc3339_is_utc_with_a_z() {
    assert_eq!(rfc3339(UNIX_EPOCH), "1970-01-01T00:00:00Z");
    assert_eq!(rfc3339(UNIX_EPOCH + Duration::from_secs(1_764_000_000)), "2025-11-24T16:00:00Z");
}
