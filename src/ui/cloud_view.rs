//! The cloud saves' settings (`razdor::cloud`, `ui::cloud_store`): the state line of the «Облако»
//! tab's rows and the four windows they open (the link code, the code from another device, the recovery
//! phrase, the delete question). The windows use the map import's frame and buttons, as the
//! analytics' panels do. Off-screen, `RAZDOR_SNAPSHOT` with `RAZDOR_SCENE_CLOUD=off|synced|offline|
//! full|working|link|enter|phrase|delete` shows a made-up state instead of the worker's, and
//! `hub-offer` the «Установить то же самое?» window with made-up items. `RAZDOR_SCENE_CLOUD_ALLOWED=1`
//! turns the cloud switch on for a snapshot (the «Облако» tab shows; no player's run sets it).
//!
//! The Hub offer: the account's installed Hub items (the worker's `HUB_OFFER`) that this device lacks
//! are offered once per set, in a window with «Установить» and «Нет». The answered set's fingerprint
//! is kept in `hub_offer.txt` in the cloud folder.

use std::sync::Mutex;

use macroquad::miniquad::window::clipboard_set;
use razdor::cloud::{account, Job, Phase};
use razdor::hub::Installed;
use razdor::i18n::tr;
use razdor::telemetry;
use razdor::cloud::SaveState;
use razdor_wire::cloud::{HubList, SaveEntry, SaveKind, SaveMeta};

use super::cloud_store;
use super::map_import;
use super::widgets::esc_pressed;

/// A settings window that is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// «Привязать устройство»: the code to type on the other device (valid 10 minutes).
    Link,
    /// «Ввести код»: the code from another device.
    Enter,
    /// «Фраза восстановления»: shows the phrase; a phrase typed here restores the account.
    Phrase,
    /// «Удалить аккаунт»: asks, then the server deletes it.
    Delete,
}

static PANEL: Mutex<Option<Panel>> = Mutex::new(None);
/// The text typed into the code or phrase field.
static TYPED: Mutex<String> = Mutex::new(String::new());

/// What the settings rows show: whether the cloud is on, the state line, a code being shown.
pub struct View {
    pub on: bool,
    pub state: String,
    /// The cloud space in use («Занято 3.0 из 20 МБ»), empty while the cloud is off.
    pub space: String,
    /// The link code and when it stops working (unix seconds), while one is issued.
    pub link: Option<(String, u64)>,
    /// The account exists on this device (on or off): the phrase and the delete rows need it.
    pub has_account: bool,
}

/// The snapshot's made-up state (`RAZDOR_SCENE_CLOUD`), when the snapshots run.
fn canned() -> Option<String> {
    std::env::var_os("RAZDOR_SNAPSHOT")?;
    std::env::var("RAZDOR_SCENE_CLOUD").ok()
}

/// The rows' state now.
pub fn view() -> View {
    if let Some(scene) = canned() {
        return canned_view(&scene);
    }
    let on = cloud_store::on();
    let shared = cloud_store::shared();
    let state = state_line(on, shared.as_ref().map(|s| (s.phase, s.last_sync)));
    let space = space_line(on, shared.as_ref().map_or(0, |s| s.used_bytes));
    View {
        on,
        state,
        space,
        link: shared.as_ref().and_then(|s| s.link.as_ref()).map(|l| (l.code.as_str().to_string(), l.expires_at)),
        has_account: cloud_store::has_account(),
    }
}

fn canned_view(scene: &str) -> View {
    let recent = Some(1_700_000_000);
    let (on, state, used) = match scene {
        "off" => (false, state_line(false, None), 0),
        "offline" => (true, state_line(true, Some((Phase::Offline, None))), 3 << 20),
        "full" => (true, state_line(true, Some((Phase::Full, recent))), 20 << 20),
        "working" => (true, state_line(true, Some((Phase::Working, recent))), 3 << 20),
        _ => (true, state_line(true, Some((Phase::Idle, recent))), 3 << 20),
    };
    View {
        on,
        state,
        space: space_line(on, used),
        link: (scene == "link").then(|| ("4F7K-2Q9M".to_string(), telemetry::now_secs() + 600)),
        has_account: true,
    }
}

/// The state line: off, syncing, offline, full, or the time of the last sync.
/// `shared` is (phase, last sync) from the worker; none before it starts.
pub fn state_line(on: bool, shared: Option<(Phase, Option<u64>)>) -> String {
    if !on {
        return tr("Off").to_string();
    }
    let Some((phase, last)) = shared else { return tr("Not synced yet").to_string() };
    match phase {
        Phase::Full => tr("The cloud is full").to_string(),
        Phase::Offline => tr("No connection").to_string(),
        Phase::Failed => tr("Sync failed, it will try again").to_string(),
        Phase::Working => tr("Syncing…").to_string(),
        Phase::Off | Phase::Idle => match last {
            Some(t) => razdor::trf!("Synced {time}", time = hhmm(t)),
            None => tr("Not synced yet").to_string(),
        },
    }
}

/// The cloud space in use, «Занято 3.0 из 20 МБ» («Выключено» while the cloud is off: an empty button
/// looked broken on the settings row).
pub fn space_line(on: bool, used: u64) -> String {
    if !on {
        return tr("Off").to_string();
    }
    razdor::trf!("Using {used} of 20 MB", used = mb(used))
}

/// Megabytes with one decimal, as the state line shows them.
fn mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}

/// The local clock's «hh:mm» of unix time `t`.
pub fn hhmm(t: u64) -> String {
    // A snapshot shows UTC, as the load list's dates do (`saves::saved_label`).
    let offset = if super::clock::is_snapshot() { 0 } else { super::saves::local_offset(t) };
    let local = (t as i64 + offset).rem_euclid(86_400);
    format!("{:02}:{:02}", local / 3600, local % 3600 / 60)
}

/// Opens a window (its row was clicked). The link and the phrase ask the worker for what they show.
pub fn open(panel: Panel) {
    *TYPED.lock().unwrap_or_else(|e| e.into_inner()) = String::new();
    *PANEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(panel);
    if panel == Panel::Link && canned().is_none() {
        cloud_store::request(Job::Link);
    }
}

/// «Облачные сохранения» flips: on creates the account (or syncs with the one there), off stops.
pub fn toggle() {
    if cloud_store::on() {
        cloud_store::turn_off();
    } else {
        cloud_store::turn_on();
    }
}

/// «Синхронизировать сейчас».
pub fn sync_now() {
    if cloud_store::on() {
        cloud_store::request(Job::Sync);
    }
}

/// Draws the window that is open, over the screen (called from `App::frame`).
pub fn modal() {
    match canned().as_deref() {
        Some("hub-offer") => {
            let _ = offer_window(&["demo-map".to_string(), "demo-mod".to_string()]);
        }
        Some(_) => {}
        None => {
            check_offer();
            offer_step();
        }
    }
    let panel = match canned() {
        Some(scene) if scene == "link" => Some(Panel::Link),
        Some(scene) if scene == "enter" => Some(Panel::Enter),
        Some(scene) if scene == "phrase" => Some(Panel::Phrase),
        Some(scene) if scene == "delete" => Some(Panel::Delete),
        Some(_) => None,
        None => *PANEL.lock().unwrap_or_else(|e| e.into_inner()),
    };
    let Some(panel) = panel else { return };
    match panel {
        Panel::Link => link_step(),
        Panel::Enter => enter_step(),
        Panel::Phrase => phrase_step(),
        Panel::Delete => delete_step(),
    }
}

/// The offer's set last checked (its fingerprint), so a set is checked once.
static OFFER_CHECKED: Mutex<Option<String>> = Mutex::new(None);
/// The set whose window is up, with the items this device lacks.
static OFFER_ASKING: Mutex<Option<Offer>> = Mutex::new(None);
/// The answered set's fingerprint, in the cloud folder.
const OFFER_FILE: &str = "hub_offer.txt";

/// A set of Hub items the window offers: the items this device lacks, and the set's fingerprint.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Offer {
    missing: Vec<String>,
    fingerprint: String,
}

/// The set's fingerprint: its items as «id version», sorted, one per line. The same set, in any
/// order, gives the same fingerprint.
pub fn fingerprint(list: &HubList) -> String {
    let mut lines: Vec<String> = list.items.iter().map(|i| format!("{} {}", i.id, i.version)).collect();
    lines.sort();
    lines.join("\n")
}

/// The offered items this device lacks, in the account's order.
pub fn missing_items(list: &HubList, installed: &Installed) -> Vec<String> {
    list.items.iter().filter(|i| !installed.has(&i.id)).map(|i| i.id.clone()).collect()
}

/// Whether to ask: something is missing here, and this set (`fingerprint`) was not answered before
/// (`answered`: the fingerprint kept in the cloud folder).
pub fn should_ask(missing: &[String], fingerprint: &str, answered: Option<&str>) -> bool {
    !missing.is_empty() && answered.map(str::trim_end) != Some(fingerprint)
}

/// Whether the cloud's settings rows, the load list's cloud marks and the Hub offer show: the remote
/// config's `cloud` switch (`remote_config::cloud_allowed`), nothing else.
pub fn rows_visible(switch: bool) -> bool {
    switch
}

/// Looks at the worker's set once per change: when it is new and asks, the window is queued.
fn check_offer() {
    if !rows_visible(razdor::remote_config::cloud_allowed()) {
        return;
    }
    let Some(list) = cloud_store::HUB_OFFER.lock().ok().and_then(|o| o.clone()) else { return };
    let fingerprint = fingerprint(&list);
    {
        let Ok(mut checked) = OFFER_CHECKED.lock() else { return };
        if checked.as_deref() == Some(fingerprint.as_str()) {
            return;
        }
        *checked = Some(fingerprint.clone());
    }
    let Some(local) = razdor::hub::local_dir() else { return };
    let missing = missing_items(&list, &razdor::hub::read_installed(&local));
    let answered = cloud_store::cloud_dir().and_then(|d| std::fs::read_to_string(d.join(OFFER_FILE)).ok());
    if should_ask(&missing, &fingerprint, answered.as_deref()) {
        if let Ok(mut asking) = OFFER_ASKING.lock() {
            *asking = Some(Offer { missing, fingerprint });
        }
    }
}

/// The queued offer's window: «Установить» starts the install, «Нет» (or the cross, Esc) only
/// records the set as answered.
fn offer_step() {
    let Some(offer) = OFFER_ASKING.lock().ok().and_then(|o| o.clone()) else { return };
    let Some(install) = offer_window(&offer.missing) else { return };
    if let Ok(mut asking) = OFFER_ASKING.lock() {
        *asking = None;
    }
    if let Some(dir) = cloud_store::cloud_dir() {
        let _ = std::fs::create_dir_all(&dir);
        if let Err(why) = std::fs::write(dir.join(OFFER_FILE), &offer.fingerprint) {
            razdor::diag!("cloud: the Hub offer is not remembered: {why}");
        }
    }
    if install {
        start_install(offer.missing);
    }
}

/// The offer's window this frame: `Some(true)` on «Установить», `Some(false)` on «Нет» or a close.
fn offer_window(missing: &[String]) -> Option<bool> {
    let names = missing.join(", ");
    let line = razdor::trf!("Your other devices have Hub items not installed here: {names}", names = names).to_string();
    let (inner, closed) = map_import::frame(tr("Install the same?"), &[line], true);
    match map_import::buttons(inner, &[tr("Install"), tr("No")]) {
        Some(0) => Some(true),
        Some(_) => Some(false),
        None if closed || esc_pressed() => Some(false),
        None => None,
    }
}

/// Installs each item through the Hub, on a thread of its own (as the lacking-mod download does).
/// The items come from the Hub's own catalog, never from the cloud.
fn start_install(ids: Vec<String>) {
    let Some(local) = razdor::hub::local_dir() else { return };
    let lang = razdor::i18n::lang();
    let spawned = std::thread::Builder::new().name("hub-offer".into()).spawn(move || {
        let hub = razdor::hub::Hub { source: razdor::hub::Source::from_env(), local };
        let Some(catalog) = hub.cached_catalog() else {
            razdor::diag!("hub offer: the catalog is not loaded");
            return;
        };
        for id in ids {
            if let Err(why) = hub.install(&catalog, &id, lang) {
                razdor::diag!("hub offer: {id}: {why}");
            }
        }
    });
    if spawned.is_err() {
        razdor::diag!("hub offer: the install thread did not start");
    }
}

fn link_step() {
    let link = view().link;
    let code = link.as_ref().map_or_else(|| tr("Making a code…").to_string(), |(c, _)| c.clone());
    let until = link.map(|(_, at)| format!("{} {}", tr("Valid for 10 minutes, once. Until"), hhmm(at)));
    let mut lines = vec![format!("{}: {code}", tr("Code for the other device"))];
    lines.push(until.unwrap_or_default());
    // Two lines: the path's own line does not leave a lone word on the desktop's window.
    lines.push(tr("On the other device:").to_string());
    lines.push(tr("Settings, Other, Cloud saves, Enter a code.").to_string());
    let (inner, closed) = map_import::frame(tr("Link a device"), &lines, true);
    match map_import::buttons(inner, &[tr("Copy"), tr("Close")]) {
        Some(0) => {
            if !code.is_empty() {
                clipboard_set(&code);
            }
            close();
        }
        Some(_) => close(),
        None if closed || esc_pressed() => close(),
        None => {}
    }
}

fn enter_step() {
    let lines = vec![tr("Type the code shown on the other device.").to_string(), String::new()];
    let (inner, closed) = map_import::frame(tr("Enter a code"), &lines, true);
    let field = map_import::field_at(inner, 1);
    let mut typed = TYPED.lock().unwrap_or_else(|e| e.into_inner());
    let _ = super::widgets::text_field("cloud-code", field.x, field.y, field.w, field.h, &mut typed, false);
    let code = typed.trim().to_string();
    drop(typed);
    match map_import::buttons(inner, &[tr("Link"), tr("Cancel")]) {
        Some(0) => {
            if !code.is_empty() {
                cloud_store::request(Job::Redeem(code));
            }
            close();
        }
        Some(_) => close(),
        None if closed || esc_pressed() => close(),
        None => {}
    }
}

fn phrase_step() {
    // The snapshots show a made-up key's phrase (no account is on the snapshot's device).
    let (has, key) = if canned().is_some() {
        (true, Some("ab".repeat(32)))
    } else {
        (cloud_store::has_account(), cloud_store::cloud_dir().and_then(|d| account::load_key(&d)))
    };
    let words = match key.as_deref().map(account::phrase) {
        Some(Ok(p)) => phrase_lines(&p),
        _ => vec![tr("No account on this device.").to_string()],
    };
    // Two short lines, so the frame wraps nothing and the field below them is where it is drawn.
    let mut lines = vec![tr("Write it down.").to_string(), tr("On a new device it brings the account back.").to_string()];
    lines.extend(words);
    lines.push(String::new());
    let (inner, closed) = map_import::frame(tr("Recovery phrase"), &lines, true);
    let field = map_import::field_at(inner, lines.len() - 1);
    let mut typed = TYPED.lock().unwrap_or_else(|e| e.into_inner());
    let _ = super::widgets::text_field("cloud-phrase", field.x, field.y, field.w, field.h, &mut typed, false);
    let phrase = typed.trim().to_string();
    drop(typed);
    match map_import::buttons(inner, &[tr("Restore"), tr("Close")]) {
        Some(0) if has || !phrase.is_empty() => {
            restore(&phrase);
            close();
        }
        Some(_) => close(),
        None if closed || esc_pressed() => close(),
        None => {}
    }
}

/// Restores the account from a typed phrase: the key goes into this device's cloud folder, and
/// the cloud is switched on. A wrong phrase is refused by its check digit and nothing changes.
fn restore(phrase: &str) {
    let Some(dir) = cloud_store::cloud_dir() else { return };
    let Ok(key) = account::key_from_phrase(phrase) else {
        razdor::diag!("cloud: the recovery phrase does not check out");
        return;
    };
    if account::store_key(&dir, &key).is_ok() {
        let _ = std::fs::remove_file(dir.join("off"));
        cloud_store::request(Job::Sync);
    }
}

fn delete_step() {
    let lines = vec![
        tr("The server deletes the account and every save of it. The saves on this device stay.").to_string(),
        tr("This cannot be undone.").to_string(),
    ];
    let (inner, closed) = map_import::frame(tr("Delete the account?"), &lines, true);
    match map_import::buttons(inner, &[tr("Delete"), tr("Cancel")]) {
        Some(0) => {
            cloud_store::request(Job::DeleteAccount);
            close();
        }
        Some(_) => close(),
        None if closed || esc_pressed() => close(),
        None => {}
    }
}

/// A recovery phrase's lines: its 4-character groups joined by dashes, five groups to a line (the
/// phrase has no spaces, so the word wrap cannot break it and it ran past the window's edge).
fn phrase_lines(phrase: &str) -> Vec<String> {
    phrase.split('-').collect::<Vec<_>>().chunks(5).map(|c| c.join("-")).collect()
}

fn close() {
    *PANEL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
/// The load window's «Облако» tab: shown while the remote config's cloud switch is on and an account
/// exists on this device (the snapshots always show it).
pub fn cloud_tab_shown() -> bool {
    canned().is_some() || (rows_visible(razdor::remote_config::cloud_allowed()) && cloud_store::has_account())
}

/// One line of the «Облако» tab: a device's header, or a save of another device. `locked`: a save of
/// a newer Razdor («Нужна новая версия»), never downloaded.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CloudLine {
    Device(String),
    Save { id: String, name: String, place: String, saved_at: u64, locked: bool },
}

/// The tab's lines: the other devices' saves and the newer Razdor's, grouped by device id (two phones
/// both named «телефон» stay apart), the device with the newest save first, the newest save first in
/// each group.
pub fn cloud_lines(others: &[SaveEntry], newer: &[SaveEntry]) -> Vec<CloudLine> {
    let mut all: Vec<(&SaveEntry, bool)> = others.iter().map(|e| (e, false)).chain(newer.iter().map(|e| (e, true))).collect();
    all.sort_by(|a, b| b.0.meta.saved_at.cmp(&a.0.meta.saved_at).then_with(|| a.0.id.cmp(&b.0.id)));
    let mut devices: Vec<&str> = Vec::new();
    for (e, _) in &all {
        if !devices.contains(&e.meta.device_id.as_str()) {
            devices.push(e.meta.device_id.as_str());
        }
    }
    let mut lines = Vec::new();
    for device in devices {
        let group: Vec<(&SaveEntry, bool)> = all.iter().filter(|(e, _)| e.meta.device_id == device).copied().collect();
        lines.push(CloudLine::Device(device_title(&group[0].0.meta.device)));
        for (e, locked) in group {
            let name = if e.meta.name.is_empty() { kind_label(e.meta.kind) } else { e.meta.name.clone() };
            let place = e.meta.map.clone();
            lines.push(CloudLine::Save { id: e.id.clone(), name, place, saved_at: e.meta.saved_at, locked });
        }
    }
    lines
}

/// A device's header: its name with a capital letter, «Другое устройство» for a device without one.
fn device_title(name: &str) -> String {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => tr("Another device").to_string(),
    }
}

/// A save's kind as the cloud tab names it.
fn kind_label(kind: SaveKind) -> String {
    match kind {
        SaveKind::Manual => tr("Private save").to_string(),
        SaveKind::Quick => tr("Quick save").to_string(),
        SaveKind::Session => tr("Continue").to_string(),
    }
}

/// The mark of a local save in the load list: the cloud mark, the hourglass while a push runs, or
/// the «не в облаке» note for a save past the device's cap. Nothing else.
pub fn save_mark(state: Option<SaveState>, phase: Phase) -> (Option<SaveState>, bool) {
    match state {
        Some(SaveState::InCloud) => (Some(SaveState::InCloud), false),
        Some(SaveState::WaitingUpload) if phase == Phase::Working => (Some(SaveState::WaitingUpload), false),
        Some(SaveState::LocalOnly) => (None, true),
        _ => (None, false),
    }
}

/// What the «Облако» tab shows now: the lines and the state of the list and of a download.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CloudTab {
    pub lines: Vec<CloudLine>,
    /// The list is being read (it has not come yet).
    pub loading: bool,
    /// No answer from the server for the list.
    pub offline: bool,
    /// The save whose download runs.
    pub downloading: Option<String>,
    /// The last download that landed: (the remote id, the local save's id).
    pub landed: Option<(String, String)>,
    /// The download that failed (its remote id).
    pub failed: Option<String>,
}

/// The tab's state. `seen` is the worker's list count when the tab was opened: until it moves, the
/// list is loading (unless the server did not answer).
pub fn cloud_tab(seen: u64) -> CloudTab {
    if let Some(scene) = canned() {
        return canned_tab(&scene);
    }
    let Some(sh) = cloud_store::shared() else { return CloudTab { loading: true, ..CloudTab::default() } };
    let offline = sh.phase == Phase::Offline;
    CloudTab {
        lines: cloud_lines(&sh.others, &sh.newer),
        loading: sh.listed == seen && !offline && sh.phase != Phase::Failed,
        offline: offline && sh.listed == seen,
        downloading: sh.downloading.clone(),
        landed: sh.landed.clone(),
        failed: sh.download_failed.clone(),
    }
}

/// A made-up save of the snapshots' «Облако» tab.
fn canned_entry(id: &str, device: (&str, &str), name: &str, map: &str, kind: SaveKind, saved_at: u64, razdor: &str) -> SaveEntry {
    SaveEntry {
        id: id.to_string(),
        meta: SaveMeta {
            schema: razdor_wire::SCHEMA,
            game: "dt".to_string(),
            map: map.to_string(),
            kind,
            device_id: device.0.to_string(),
            device: device.1.to_string(),
            name: name.to_string(),
            saved_at,
            razdor: razdor.to_string(),
            size: 0,
            sha256: String::new(),
            raw_sha256: String::new(),
            pinned: false,
        },
    }
}

/// The snapshots' «Облако» tab (`RAZDOR_SCENE_CLOUD=cloud|cloud-empty|cloud-offline|cloud-loading|
/// cloud-downloading`): two other devices and a locked save of a newer Razdor.
fn canned_tab(scene: &str) -> CloudTab {
    let phone = canned_entry("phone-1", ("b2c4", "телефон"), "Перед осадой", "Пепельная Марка", SaveKind::Manual, 1_700_003_600, "1.5.2");
    let phone_old = canned_entry("phone-2", ("b2c4", "телефон"), "", "Тихая долина", SaveKind::Session, 1_699_900_000, "1.5.2");
    let pc = canned_entry("pc-1", ("9f01", "ПК"), "Поход на север", "Дорога в Сарн", SaveKind::Manual, 1_699_800_000, "1.5.2");
    let newer = canned_entry("newer-1", ("9f01", "ПК"), "Из будущего", "Пепельная Марка", SaveKind::Manual, 1_700_100_000, "99.0.0");
    let mut tab = CloudTab::default();
    match scene {
        "cloud-empty" => {}
        "cloud-offline" => tab.offline = true,
        "cloud-loading" => tab.loading = true,
        "cloud-downloading" => {
            tab.lines = cloud_lines(&[phone, phone_old, pc], &[newer]);
            tab.downloading = Some("phone-1".to_string());
        }
        _ => tab.lines = cloud_lines(&[phone, phone_old, pc], &[newer]),
    }
    tab
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recovery_phrase_wraps_by_group_and_keeps_every_character() {
        let phrase = account::phrase(&"ab".repeat(32)).expect("a 32-byte key has a phrase");
        let lines = phrase_lines(&phrase);
        assert!(lines.len() > 1, "the phrase is wrapped, not one long line");
        assert!(lines.iter().all(|l| l.chars().count() <= 24), "five groups of four and their dashes");
        assert_eq!(lines.join("-"), phrase);
    }

    #[test]
    fn the_cloud_rows_show_only_with_the_remote_switch_on() {
        assert!(!rows_visible(false));
        assert!(rows_visible(true));
        assert!(!rows_visible(razdor::remote_config::Config::builtin().cloud), "the built-in config hides the cloud");
    }

    #[test]
    fn the_space_row_says_off_while_the_cloud_is_off() {
        assert_eq!(space_line(false, 3 << 20), tr("Off"));
        assert!(space_line(true, 3 << 20).contains("3.0"), "the used megabytes are shown");
    }

    #[test]
    fn the_quota_is_shown_in_megabytes_with_one_decimal() {
        assert_eq!(mb(3 << 20), "3.0");
        assert_eq!(mb((3 << 20) + (1 << 19)), "3.5");
    }

    #[test]
    fn a_cloud_without_a_worker_has_no_sync_state() {
        assert_eq!(state_line(true, None), tr("Not synced yet"));
    }

    fn hub(items: &[(&str, &str)]) -> HubList {
        let items = items
            .iter()
            .map(|(id, version)| razdor_wire::cloud::HubItem { id: id.to_string(), kind: razdor_wire::cloud::HubKind::Map, version: version.to_string() })
            .collect();
        HubList { schema: razdor_wire::SCHEMA, updated_at: 0, items }
    }

    #[test]
    fn the_same_set_in_any_order_has_one_fingerprint_and_a_new_version_another() {
        let a = hub(&[("b", "1"), ("a", "2")]);
        let b = hub(&[("a", "2"), ("b", "1")]);
        assert_eq!(fingerprint(&a), fingerprint(&b));
        assert_ne!(fingerprint(&a), fingerprint(&hub(&[("a", "3"), ("b", "1")])));
    }

    #[test]
    fn only_the_items_this_device_lacks_are_offered_in_the_account_order() {
        let installed: Installed = serde_json::from_str(r#"{"entries":{"a":{"version":"2","kind":"map","files":[]}}}"#).expect("installed.json");
        let list = hub(&[("b", "1"), ("a", "2"), ("c", "1")]);
        assert_eq!(missing_items(&list, &installed), ["b", "c"]);
        assert!(missing_items(&hub(&[("a", "2")]), &installed).is_empty());
    }

    #[test]
    fn a_set_with_missing_items_is_asked_once_and_nothing_is_asked_when_all_are_here() {
        let missing = ["b".to_string()];
        assert!(should_ask(&missing, "b 1", None));
        assert!(!should_ask(&missing, "b 1", Some("b 1\n")), "the answered set is not asked again");
        assert!(should_ask(&missing, "b 2", Some("b 1")), "a changed set is asked anew");
        assert!(!should_ask(&[], "b 1", None), "nothing missing, nothing asked");
    }
}

#[cfg(test)]
mod cloud_tab_tests {
    use super::*;

    fn entry(id: &str, device_id: &str, device: &str, saved_at: u64, razdor: &str) -> SaveEntry {
        canned_entry(id, (device_id, device), "", "Map", SaveKind::Manual, saved_at, razdor)
    }

    #[test]
    fn the_tab_groups_by_device_id_newest_device_first_and_locks_the_newer() {
        let others = [entry("a1", "p1", "телефон", 100, "1.5.2"), entry("b1", "pc", "ПК", 300, "1.5.2"), entry("a2", "p1", "телефон", 200, "1.5.2")];
        let other_phone = entry("c1", "p2", "телефон", 250, "1.5.2");
        let newer = [entry("n1", "pc", "ПК", 400, "99.0.0")];
        let mut all = others.to_vec();
        all.push(other_phone);
        let lines = cloud_lines(&all, &newer);
        let ids: Vec<String> = lines
            .iter()
            .map(|l| match l {
                CloudLine::Device(name) => format!("[{name}]"),
                CloudLine::Save { id, locked, .. } => format!("{id}{}", if *locked { "!" } else { "" }),
            })
            .collect();
        // The PC has the newest save (400, locked); the two phones stay apart by id, newest first.
        assert_eq!(ids, ["[ПК]", "n1!", "b1", "[Телефон]", "c1", "[Телефон]", "a2", "a1"]);
    }

    #[test]
    fn a_save_without_a_name_is_named_by_its_kind_and_a_device_without_one_by_its_place() {
        let lines = cloud_lines(&[entry("a1", "", "", 1, "1.5.2")], &[]);
        assert_eq!(lines[0], CloudLine::Device(tr("Another device").to_string()));
        assert!(matches!(&lines[1], CloudLine::Save { name, locked: false, .. } if name == tr("Private save")));
    }

    #[test]
    fn the_marks_are_the_cloud_the_hourglass_during_a_push_and_the_cap_note() {
        assert_eq!(save_mark(Some(SaveState::InCloud), Phase::Idle), (Some(SaveState::InCloud), false));
        assert_eq!(save_mark(Some(SaveState::WaitingUpload), Phase::Working), (Some(SaveState::WaitingUpload), false));
        assert_eq!(save_mark(Some(SaveState::WaitingUpload), Phase::Idle), (None, false), "no hourglass while nothing uploads");
        assert_eq!(save_mark(Some(SaveState::LocalOnly), Phase::Idle), (None, true), "past the cap: the note");
        assert_eq!(save_mark(None, Phase::Working), (None, false), "a save the mirror does not hold shows nothing");
    }

    #[test]
    fn the_canned_scenes_list_two_devices_and_a_locked_newer_save() {
        let tab = canned_tab("cloud");
        let locked = tab.lines.iter().filter(|l| matches!(l, CloudLine::Save { locked: true, .. })).count();
        assert_eq!(locked, 1);
        assert_eq!(tab.lines.iter().filter(|l| matches!(l, CloudLine::Device(_))).count(), 2);
        assert!(canned_tab("cloud-empty").lines.is_empty());
        assert!(canned_tab("cloud-offline").offline);
        assert_eq!(canned_tab("cloud-downloading").downloading.as_deref(), Some("phone-1"));
    }
}
