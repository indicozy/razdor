//! The cloud against the real API: `razdor-server`'s router on a local port, the desktop curl
//! transport, two devices (the second one linked with a code). Each test starts its own server on
//! a temp data folder.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use razdor_server::clock::Clock;
use razdor_server::config::Config;
use razdor_server::state::AppState;
use razdor_wire::cloud::{MarkLevel, SaveKind};

use super::account;
use super::api::{Api, CloudError};
use super::http::Curl;
use super::select::SaveState;
use super::sync::{self, Local};
use super::testing::MemLocal;

/// Starts the server on a free port; returns its base address and its data folder.
fn server(name: &str) -> (String, PathBuf) {
    let dir = std::env::temp_dir().join(format!("razdor-cloud-e2e-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let cfg = Config {
        data_dir: dir.clone(),
        trust_proxy: true,
        secure_cookies: false,
        ..Config::default()
    };
    let state = AppState::open(cfg, Clock::system()).expect("the server opens");
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            tx.send(listener.local_addr().expect("address")).expect("send");
            let app = razdor_server::router(state).into_make_service_with_connect_info::<SocketAddr>();
            axum::serve(listener, app).await.expect("serves");
        });
    });
    let addr = rx.recv().expect("the server's address");
    (format!("http://{addr}"), dir)
}

fn net() -> Arc<Curl> {
    Arc::new(Curl { seconds: 30 })
}

/// Bytes that do not compress, so a save's size on the wire is its real size.
fn noise(len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    getrandom::fill(&mut out).expect("random");
    out
}

/// A device: its cloud folder, its key (once linked) and its local store.
struct Device {
    dir: PathBuf,
    local: MemLocal,
}

impl Device {
    fn new(root: &std::path::Path, name: &str, device: &str) -> Device {
        let dir = root.join(name);
        let _ = std::fs::remove_dir_all(&dir);
        Device { dir, local: MemLocal::new(device) }
    }

    fn api(&self, base: &str) -> Api {
        Api::new(net(), base, account::load_key(&self.dir))
    }
}

#[test]
fn an_account_links_a_second_device_and_the_saves_come_back() {
    let (base, root) = server("link");
    let mut a = Device::new(&root, "a", "ПК");
    let key = Api::new(net(), &base, None).create_account().expect("creates an account");
    account::store_key(&a.dir, &key).expect("keeps the key");
    let api_a = a.api(&base);

    let manual1 = noise(3000);
    a.local.add("dt", "ruins", SaveKind::Manual, "первое", 100, &manual1);
    a.local.add("dt", "ruins", SaveKind::Manual, "второе", 200, &noise(2000));
    a.local.add("dt", "ruins", SaveKind::Manual, "третье", 300, &noise(2000));
    a.local.add("dt", "ruins", SaveKind::Manual, "четвертое", 400, &noise(2000));
    a.local.add("dt", "ruins", SaveKind::Quick, "быстрое", 150, &noise(1000));
    a.local.add("dt", "ruins", SaveKind::Session, "Продолжить", 500, &noise(1500));
    a.local.set_mark("ruins", MarkLevel::Green);
    a.local.set_setting("language", "ru", 10);

    let pushed = sync::push(&api_a, &mut a.local).expect("pushes");
    assert_eq!(pushed.pushed, 5, "three manual of four, the quick save and the session");
    assert!(pushed.errors.is_empty(), "{:?}", pushed.errors);
    assert_eq!(pushed.states["local-1"], SaveState::LocalOnly, "the oldest manual stays on this device");
    assert_eq!(pushed.states["local-2"], SaveState::InCloud);
    assert!(pushed.used_bytes > 0);

    // The second device: a code from the first, typed on the second.
    let link = api_a.link_issue().expect("issues a code");
    assert!(link.expires_at > 0);
    let mut b = Device::new(&root, "b", "телефон");
    let key_b = Api::new(net(), &base, None).link_redeem(link.code.as_str()).expect("redeems the code");
    assert_eq!(key_b, key, "the same account");
    account::store_key(&b.dir, &key_b).expect("keeps the key");
    assert!(Api::new(net(), &base, None).link_redeem(link.code.as_str()).is_err(), "a code works once");

    let api_b = b.api(&base);
    let pulled = sync::sync(&api_b, &mut b.local).expect("syncs");
    assert_eq!(pulled.pulled, 5);
    assert_eq!(b.local.bytes_named("первое"), None, "the save beyond the window is not downloaded twice");
    assert_eq!(b.local.bytes_named("второе").expect("downloaded").len(), 2000);
    assert_eq!(b.local.bytes_named("Продолжить").expect("the session").len(), 1500);
    assert!(b.local.has_mark("ruins"), "the marks come over");
    assert_eq!(b.local.setting("language").as_deref(), Some("ru"));
    assert!(pulled.newer.is_empty());

    // A second push from the second device sends nothing new.
    let again = sync::push(&api_b, &mut b.local).expect("pushes again");
    assert_eq!(again.pushed, 0);
    assert_eq!(again.deleted, 0);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_session_slot_keeps_the_newer_and_the_older_becomes_a_save() {
    let (base, root) = server("session");
    let mut a = Device::new(&root, "a", "ПК");
    let key = Api::new(net(), &base, None).create_account().expect("creates an account");
    account::store_key(&a.dir, &key).expect("keeps the key");
    a.local.add("dt", "ruins", SaveKind::Session, "Продолжить", 500, &noise(800));
    sync::push(&a.api(&base), &mut a.local).expect("pushes the first session");

    let mut b = Device::new(&root, "b", "телефон");
    let key_b = Api::new(net(), &base, None).link_redeem(
        Api::new(net(), &base, Some(key.clone())).link_issue().expect("code").code.as_str(),
    ).expect("redeems");
    account::store_key(&b.dir, &key_b).expect("keeps the key");
    sync::pull(&b.api(&base), &mut b.local).expect("pulls");
    // The phone plays on and its session is newer than the PC's.
    b.local.add("dt", "ruins", SaveKind::Session, "Продолжить", 900, &noise(900));
    let pushed = sync::push(&b.api(&base), &mut b.local).expect("pushes the newer session");
    assert!(pushed.errors.is_empty(), "{:?}", pushed.errors);
    assert_eq!(pushed.deleted, 1, "the cloud's older session is replaced");
    assert!(b.local.named("Продолжить (ПК, 00:08)").is_some(), "the older becomes a save named after its device");

    // The PC pulls: its older session becomes a save, the newer one takes the slot.
    let mut pulled = sync::pull(&a.api(&base), &mut a.local).expect("pulls on the PC");
    assert!(pulled.errors.is_empty(), "{:?}", pulled.errors);
    pulled = sync::pull(&a.api(&base), &mut a.local).expect("a second pull changes nothing");
    assert_eq!(pulled.pulled, 0);
    let session = a.local.saves().into_iter().filter(|s| s.kind == SaveKind::Session).count();
    assert_eq!(session, 1, "one session on the PC");
    assert!(a.local.named("Продолжить (ПК, 00:08)").is_some(), "the PC's own older session became a save");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_window_removes_only_the_stale_save_and_a_full_quota_stops_the_round() {
    let (base, root) = server("window");
    let mut a = Device::new(&root, "a", "ПК");
    let key = Api::new(net(), &base, None).create_account().expect("creates an account");
    account::store_key(&a.dir, &key).expect("keeps the key");
    for (name, at) in [("n1", 100u64), ("n2", 200), ("n3", 300)] {
        a.local.add("dt", "ruins", SaveKind::Manual, name, at, &noise(500));
    }
    sync::push(&a.api(&base), &mut a.local).expect("pushes three");
    // A fourth, newer save: the oldest one in the cloud goes, the new one takes its place.
    a.local.add("dt", "ruins", SaveKind::Manual, "n4", 400, &noise(500));
    let round = sync::push(&a.api(&base), &mut a.local).expect("pushes the fourth");
    assert_eq!(round.pushed, 1);
    assert_eq!(round.deleted, 1);
    let listed = a.api(&base).list_saves().expect("lists");
    let mut times: Vec<u64> = listed.saves.iter().map(|e| e.meta.saved_at).collect();
    times.sort();
    assert_eq!(times, vec![200, 300, 400], "the window of three");

    // Eight megabytes of noise on three other maps: the third is over the account's quota.
    let mut c = Device::new(&root, "c", "ПК-2");
    let key_c = Api::new(net(), &base, None).create_account().expect("creates an account");
    account::store_key(&c.dir, &key_c).expect("keeps the key");
    for (i, map) in ["m1", "m2", "m3"].iter().enumerate() {
        c.local.add("dt", map, SaveKind::Manual, &format!("big{i}"), 10 + i as u64, &noise(7 * 1024 * 1024));
    }
    let full = sync::push(&c.api(&base), &mut c.local).expect("the round ends");
    assert!(full.full, "the round reports the full quota");
    assert_eq!(full.pushed, 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// Polls `done` for up to ten seconds.
fn wait_for(what: &str, done: impl Fn() -> bool) {
    for _ in 0..100 {
        if done() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("timed out waiting for {what}");
}

#[test]
fn the_background_worker_creates_syncs_and_deletes_the_account() {
    let (base, root) = server("worker");
    let mut local = MemLocal::new("ПК");
    local.add("dt", "ruins", SaveKind::Manual, "фоновое", 77, &noise(900));
    let cloud = super::Cloud::start(Box::new(local), net(), &base, root.join("worker"));
    assert!(!cloud.linked(), "no account before one is made");
    cloud.request(super::Job::CreateAccount);
    wait_for("the account", || cloud.linked());
    cloud.request(super::Job::Sync);
    wait_for("the first sync", || cloud.snapshot().last_sync.is_some());
    let snap = cloud.snapshot();
    assert_eq!(snap.phase, super::Phase::Idle);
    assert!(snap.last_error.is_none());
    assert_eq!(snap.states.values().copied().collect::<Vec<_>>(), vec![SaveState::InCloud]);
    assert_eq!(cloud.state_of("local-1"), SaveState::InCloud);
    cloud.request(super::Job::DeleteAccount);
    wait_for("the account's deletion", || !cloud.linked());
    assert!(account::load_key(&root.join("worker")).is_none(), "the key is forgotten on this device");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn marks_merge_and_a_deleted_account_is_refused() {
    let (base, root) = server("marks");
    let mut a = Device::new(&root, "a", "ПК");
    let key = Api::new(net(), &base, None).create_account().expect("creates an account");
    account::store_key(&a.dir, &key).expect("keeps the key");
    a.local.set_mark("one", MarkLevel::Grey);
    a.local.set_setting("language", "ru", 10);
    sync::sync(&a.api(&base), &mut a.local).expect("syncs");

    let mut b = Device::new(&root, "b", "телефон");
    let key_b = Api::new(net(), &base, None).link_redeem(
        Api::new(net(), &base, Some(key.clone())).link_issue().expect("code").code.as_str(),
    ).expect("redeems");
    account::store_key(&b.dir, &key_b).expect("keeps the key");
    b.local.set_mark("two", MarkLevel::Red);
    b.local.set_setting("language", "en", 20);
    sync::sync(&b.api(&base), &mut b.local).expect("syncs");
    sync::sync(&a.api(&base), &mut a.local).expect("syncs again");
    assert!(a.local.has_mark("one") && a.local.has_mark("two"), "a union of the marks");
    assert_eq!(a.local.setting("language").as_deref(), Some("en"), "the newest change wins");

    let api_b = b.api(&base);
    api_b.delete_account().expect("the account is deleted");
    account::forget_key(&b.dir).expect("forgets");
    let refused = api_b.list_saves();
    assert_eq!(refused, Err(CloudError::Unauthorized), "the deleted key is refused");
    let _ = std::fs::remove_dir_all(&root);
}
