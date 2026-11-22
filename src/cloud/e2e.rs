//! The cloud against the real API: `razdor-server`'s router on a local port, the desktop curl
//! transport, two devices (the second one linked with a code). Each test starts its own server on
//! a temp data folder.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use razdor_server::clock::Clock;
use razdor_server::config::Config;
use razdor_server::state::AppState;
use razdor_wire::cloud::{MarkLevel, SaveKind, SaveMeta};

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

/// Two devices of one account: `a` creates it, `b` links to it with a code.
fn linked_pair(base: &str, root: &std::path::Path) -> (Device, Device) {
    let a = Device::new(root, "a", "ПК");
    let key = Api::new(net(), base, None).create_account().expect("creates an account");
    account::store_key(&a.dir, &key).expect("keeps the key");
    let b = Device::new(root, "b", "телефон");
    let code = Api::new(net(), base, Some(key.clone())).link_issue().expect("a code").code;
    let key_b = Api::new(net(), base, None).link_redeem(code.as_str()).expect("the second device gets the key");
    account::store_key(&b.dir, &key_b).expect("keeps the key");
    (a, b)
}

/// Uploads a save straight to the server as `device_id` (a newer build's save, say).
fn upload(api: &Api, device_id: &str, device: &str, razdor: &str, name: &str, bytes: &[u8]) -> String {
    let packed = super::pack::pack(bytes).expect("packs");
    let meta = SaveMeta {
        schema: razdor_wire::SCHEMA,
        game: "dt".into(),
        map: "ruins".into(),
        kind: SaveKind::Manual,
        device_id: device_id.into(),
        device: device.into(),
        name: name.into(),
        saved_at: 500,
        razdor: razdor.into(),
        size: packed.len() as u64,
        sha256: super::pack::sha256_hex(&packed),
        raw_sha256: super::pack::sha256_hex(bytes),
        pinned: false,
    };
    api.put_save(&meta, &packed).expect("uploads").id
}

#[test]
fn a_device_mirrors_its_saves_and_another_device_lists_them_without_downloading() {
    let (base, root) = server("mirror");
    let (mut a, mut b) = linked_pair(&base, &root);
    a.local.add("dt", "ruins", SaveKind::Manual, "первое", 100, &noise(3000));
    a.local.add("dt", "ruins", SaveKind::Quick, "быстрое", 150, &noise(1000));
    a.local.add("dt", "ruins", SaveKind::Session, "Продолжить", 500, &noise(1500));
    let round = sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    assert_eq!(round.pushed, 3);
    assert_eq!(round.deleted, 0);
    assert!(round.states.values().all(|s| *s == SaveState::InCloud));

    // B has a save of the same name: two entries, labelled by device, never merged.
    b.local.add("dt", "ruins", SaveKind::Manual, "первое", 200, &noise(900));
    let listed = sync::refresh(&b.api(&base), &b.local).expect("lists");
    assert_eq!(listed.others.len(), 3, "A's three saves, listed");
    assert!(listed.others.iter().all(|e| e.meta.device == "ПК"));
    assert_eq!(b.local.saves().len(), 1, "nothing was downloaded");

    let b_round = sync::sync(&b.api(&base), &mut b.local).expect("syncs");
    assert_eq!(b_round.pushed, 1, "B uploads its own save");
    assert_eq!(b_round.deleted, 0, "and touches none of A's rows");
    assert_eq!(b_round.others.len(), 3);
    assert_eq!(b.api(&base).list_saves().expect("lists").saves.len(), 4);
}

#[test]
fn a_save_is_downloaded_on_demand_and_then_joins_the_device_mirror() {
    let (base, root) = server("download");
    let (mut a, mut b) = linked_pair(&base, &root);
    let bytes = noise(3000);
    a.local.add("dt", "ruins", SaveKind::Manual, "первое", 100, &bytes);
    sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    let entry = sync::refresh(&b.api(&base), &b.local).expect("lists").others[0].clone();

    let id = sync::download(&b.api(&base), &mut b.local, &entry.id).expect("downloads");
    let got = b.local.named("первое").expect("a local save now");
    assert_eq!(got.id, id);
    assert_eq!(got.device, "ПК", "the origin device is kept");
    assert_eq!(b.local.bytes_named("первое"), Some(bytes));
    assert!(sync::refresh(&b.api(&base), &b.local).expect("lists").others.is_empty(), "no longer on another device");

    // Downloaded again, it is the same local save.
    assert_eq!(sync::download(&b.api(&base), &mut b.local, &entry.id).expect("again"), id);
    assert_eq!(b.local.saves().len(), 1);

    // It joins B's mirror: B pushes it under its own device id.
    let round = sync::sync(&b.api(&base), &mut b.local).expect("syncs");
    assert_eq!(round.pushed, 1);
    assert_eq!(b.api(&base).list_saves().expect("lists").saves.len(), 2);
}

#[test]
fn a_deleted_save_leaves_the_cloud_at_the_next_sync_and_another_devices_rows_stay() {
    let (base, root) = server("delete");
    let (mut a, mut b) = linked_pair(&base, &root);
    a.local.add("dt", "ruins", SaveKind::Manual, "первое", 100, &noise(1000));
    a.local.add("dt", "ruins", SaveKind::Quick, "быстрое", 150, &noise(1000));
    sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    b.local.add("dt", "ruins", SaveKind::Manual, "свое", 300, &noise(1000));
    sync::sync(&b.api(&base), &mut b.local).expect("syncs");
    assert_eq!(sync::refresh(&b.api(&base), &b.local).expect("lists").others.len(), 2);

    a.local.remove("быстрое");
    let round = sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    assert_eq!(round.deleted, 1, "the deleted save leaves the cloud");
    let list = b.api(&base).list_saves().expect("lists").saves;
    assert!(!list.iter().any(|e| e.meta.name == "быстрое"));
    assert_eq!(list.iter().filter(|e| e.meta.device == "телефон").count(), 1, "B's row is untouched");
    assert_eq!(list.iter().filter(|e| e.meta.device == "ПК").count(), 1);
    assert_eq!(sync::refresh(&b.api(&base), &b.local).expect("lists").others.len(), 1, "gone from B's list");
    assert!(a.local.named("первое").is_some(), "A keeps its own save");
}

#[test]
fn the_device_cap_keeps_the_newest_and_the_rest_stays_local() {
    let (base, root) = server("cap");
    let (mut a, _b) = linked_pair(&base, &root);
    a.local.cap = 2;
    let oldest = a.local.add("dt", "ruins", SaveKind::Manual, "один", 100, &noise(500));
    a.local.add("dt", "ruins", SaveKind::Manual, "два", 200, &noise(500));
    a.local.add("dt", "ruins", SaveKind::Manual, "три", 300, &noise(500));
    let round = sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    assert_eq!(round.pushed, 2);
    assert_eq!(round.states[&oldest], SaveState::LocalOnly, "over the cap: it stays local");

    // A newer save pushes the oldest of the two out of the cap: its cloud row goes, the save stays here.
    a.local.add("dt", "ruins", SaveKind::Manual, "четыре", 400, &noise(500));
    let round = sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    assert_eq!((round.pushed, round.deleted), (1, 1));
    assert_eq!(a.api(&base).list_saves().expect("lists").saves.len(), 2);
    assert!(a.local.named("два").is_some());
}

#[test]
fn a_newer_razdors_save_is_listed_locked_and_never_downloaded_or_deleted() {
    let (base, root) = server("newer");
    let (mut a, b) = linked_pair(&base, &root);
    let id = upload(&b.api(&base), &b.local.device_id(), "телефон", "99.0.0", "из будущего", &noise(800));
    let listed = sync::refresh(&a.api(&base), &a.local).expect("lists");
    assert_eq!(listed.newer.len(), 1);
    assert!(listed.others.is_empty(), "a newer save is not offered as a download");
    assert!(matches!(sync::download(&a.api(&base), &mut a.local, &id), Err(CloudError::Refused(_))));
    sync::sync(&a.api(&base), &mut a.local).expect("syncs");
    assert!(a.api(&base).list_saves().expect("lists").saves.iter().any(|e| e.id == id), "never deleted");
}

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
