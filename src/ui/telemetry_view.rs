//! The analytics' windows (`telemetry`): the two questions («Помочь Раздору?» at the first start and
//! once after an update; the crash question after a crash) and the three panels of Settings («Мой
//! идентификатор», «Удалить мои данные», «Новый идентификатор»). All use the map import's frame and
//! buttons (the same look as the other questions). Nothing shows when the analytics are off
//! (`telemetry::active`), except in the snapshots' own switches: `RAZDOR_SCENE_CONSENT=1`,
//! `RAZDOR_SCENE_CRASH=save|none` and `RAZDOR_SCENE_TELEMETRY=id|delete|newid`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use macroquad::miniquad::window::clipboard_set;
use razdor::i18n::tr;
use razdor::telemetry::{self, crash, state::Consent, Hub};

use super::map_import;
use super::widgets::esc_pressed;

/// The version the questions are asked in (asked again after an update).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The consent window was closed without an answer: not shown again in this run (it is asked at the next start).
static DISMISSED: AtomicBool = AtomicBool::new(false);

/// A Settings panel that is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Panel {
    /// «Мой идентификатор»: the id, with a copy button.
    Id,
    /// «Удалить мои данные»: asks, then the server deletes the id's data and a new id is made.
    Delete,
    /// «Новый идентификатор»: asks, then a new id is made.
    NewId,
}

static PANEL: Mutex<Option<Panel>> = Mutex::new(None);

/// What the consent window says this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Yes,
    No,
    /// Closed (the cross or Esc): no answer is recorded.
    Closed,
    Open,
}

/// Draws the question or panel that is due, over the screen (called from `App::frame`).
pub fn modal() {
    if let Some(name) = snapshot_scene("RAZDOR_SCENE_TELEMETRY") {
        let panel = match name.as_str() {
            "delete" => Panel::Delete,
            "newid" => Panel::NewId,
            _ => Panel::Id,
        };
        panel_step(telemetry::hub(), panel);
        return;
    }
    if snapshot_scene("RAZDOR_SCENE_CONSENT").is_some() {
        consent_window();
        return;
    }
    if let Some(with_save) = snapshot_scene("RAZDOR_SCENE_CRASH") {
        crash_window(with_save == "save");
        return;
    }
    let panel = *PANEL.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(panel) = panel {
        panel_step(telemetry::hub(), panel);
        return;
    }
    let Some(hub) = telemetry::hub() else { return };
    if !DISMISSED.load(Ordering::Relaxed) && hub.question_due(VERSION) {
        consent_step(hub);
        return;
    }
    if let Some(ask) = crash::question() {
        crash_step(hub, ask.save.is_some());
    }
}

/// The snapshot's switch (`RAZDOR_SCENE_*`, with `RAZDOR_SNAPSHOT`): its value, when set.
fn snapshot_scene(name: &str) -> Option<String> {
    std::env::var_os("RAZDOR_SNAPSHOT")?;
    std::env::var(name).ok()
}

/// Opens a Settings panel (its row was clicked).
pub fn open(panel: Panel) {
    *PANEL.lock().unwrap_or_else(|e| e.into_inner()) = Some(panel);
}

/// Whether the player has said yes (false while the analytics are off).
pub fn consent_yes() -> bool {
    telemetry::hub().is_some_and(|h| h.consent() == Consent::Yes)
}

/// The Settings row «Помогать Раздору»: flips the answer (a no purges the queued statistics).
pub fn toggle_consent() {
    let Some(hub) = telemetry::hub() else { return };
    let next = if hub.consent() == Consent::Yes { Consent::No } else { Consent::Yes };
    hub.set_consent(next, VERSION);
}

fn consent_step(hub: &Hub) {
    match consent_window() {
        Answer::Yes => hub.set_consent(Consent::Yes, VERSION),
        Answer::No => hub.set_consent(Consent::No, VERSION),
        Answer::Closed => DISMISSED.store(true, Ordering::Relaxed),
        Answer::Open => {}
    }
}

/// The consent window: its answer this frame.
fn consent_window() -> Answer {
    let text = tr("Razdor can send anonymous play statistics, performance figures, crash reports and battle records, to make it better. No names, no chat, no typed text. You can change this in Settings any time.").to_string();
    let link = format!("{}: {}", tr("What we send"), telemetry::WHAT_WE_SEND_URL);
    let (inner, closed) = map_import::frame(tr("Help Razdor?"), &[text, link], true);
    match map_import::buttons(inner, &[tr("Yes"), tr("No")]) {
        Some(0) => Answer::Yes,
        Some(_) => Answer::No,
        None if closed || esc_pressed() => Answer::Closed,
        None => Answer::Open,
    }
}

fn crash_step(hub: &Hub, with_save: bool) {
    if let Some(send) = crash_window(with_save) {
        if let Some(Err(why)) = crash::answer(hub, send) {
            razdor::diag!("telemetry: the crash save is not queued: {why:?}");
        }
    }
}

/// The crash question: `Some(send)` on a button or a closed window (a close is «Нет»), `None` while open.
fn crash_window(with_save: bool) -> Option<bool> {
    let text = if with_save {
        tr("The game crashed last time. Send the report with the save, so the developer can repeat it?")
    } else {
        tr("The game crashed last time. Send the report, so the developer can look into it?")
    };
    let (inner, closed) = map_import::frame(tr("The game crashed"), &[text.to_string()], true);
    match map_import::buttons(inner, &[tr("Send"), tr("No")]) {
        Some(0) => Some(true),
        Some(_) => Some(false),
        None if closed || esc_pressed() => Some(false),
        None => None,
    }
}

/// A Settings panel for this frame: the id with its copy button, or a confirmation. It closes on any
/// button, the cross or Esc.
fn panel_step(hub: Option<&Hub>, panel: Panel) {
    match panel {
        Panel::Id => {
            let id = hub.map_or_else(|| tr("No id yet").to_string(), |h| h.install_id());
            let (inner, closed) = map_import::frame(tr("My id"), &[id.clone()], true);
            match map_import::buttons(inner, &[tr("Copy"), tr("Close")]) {
                Some(0) => {
                    clipboard_set(&id);
                    close_panel();
                }
                Some(_) => close_panel(),
                None if closed || esc_pressed() => close_panel(),
                None => {}
            }
        }
        Panel::Delete | Panel::NewId => {
            let (title, text, confirm) = match panel {
                Panel::Delete => (
                    tr("Delete my data?"),
                    tr("The server deletes everything sent under this id: statistics, crash reports, battle records. A new id is made, and nothing more is sent under the old one."),
                    tr("Delete"),
                ),
                _ => (
                    tr("New id?"),
                    tr("A new id is made. Data queued under the old id is not sent, and the old id's data stays on the server."),
                    tr("New id"),
                ),
            };
            let (inner, closed) = map_import::frame(title, &[text.to_string()], true);
            match map_import::buttons(inner, &[confirm, tr("Cancel")]) {
                Some(0) => {
                    if let Some(hub) = hub {
                        if panel == Panel::Delete {
                            if let Err(why) = hub.delete_my_data() {
                                razdor::diag!("telemetry: the delete is not queued: {why:?}");
                            }
                        } else {
                            hub.new_install_id();
                        }
                    }
                    close_panel();
                }
                Some(_) => close_panel(),
                None if closed || esc_pressed() => close_panel(),
                None => {}
            }
        }
    }
}

fn close_panel() {
    *PANEL.lock().unwrap_or_else(|e| e.into_inner()) = None;
}
