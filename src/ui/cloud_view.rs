//! The cloud saves' settings (`razdor::cloud`, `ui::cloud_store`): the state line of the «Прочее»
//! rows and the four windows they open (the link code, the code from another device, the recovery
//! phrase, the delete question). The windows use the map import's frame and buttons, as the
//! analytics' panels do. Off-screen, `RAZDOR_SNAPSHOT` with `RAZDOR_SCENE_CLOUD=off|synced|offline|
//! full|working|link|enter|phrase|delete` shows a made-up state instead of the worker's.

use std::sync::Mutex;

use macroquad::miniquad::window::clipboard_set;
use razdor::cloud::{account, Job, Phase};
use razdor::i18n::tr;
use razdor::telemetry;

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

/// The cloud space in use, «Занято 3.0 из 20 МБ» (empty while the cloud is off).
pub fn space_line(on: bool, used: u64) -> String {
    if !on {
        return String::new();
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

fn link_step() {
    let link = view().link;
    let code = link.as_ref().map_or_else(|| tr("Making a code…").to_string(), |(c, _)| c.clone());
    let until = link.map(|(_, at)| format!("{} {}", tr("Valid for 10 minutes, once. Until"), hhmm(at)));
    let mut lines = vec![format!("{}: {code}", tr("Code for the other device"))];
    lines.push(until.unwrap_or_default());
    lines.push(tr("On the other device: Settings, Other, Cloud saves, Enter a code.").to_string());
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
        Some(Ok(p)) => split(&p, 18),
        _ => vec![tr("No account on this device.").to_string()],
    };
    let mut lines = vec![tr("Write it down. On a new device it brings the account back.").to_string()];
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

/// Splits a phrase into lines of about `width` characters, at its spaces.
fn split(text: &str, width: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.chars().count() + 1 + word.chars().count() > width {
            out.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        out.push(line);
    }
    out
}

fn close() {
    *PANEL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_phrase_splits_at_spaces_into_short_lines() {
        let lines = split("alpha bravo charlie delta echo foxtrot", 12);
        assert_eq!(lines, ["alpha bravo", "charlie", "delta echo", "foxtrot"]);
        assert!(split("", 12).is_empty());
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
}
