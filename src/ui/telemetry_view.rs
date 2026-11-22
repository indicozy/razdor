//! The analytics' two questions (`telemetry`): «Помочь Раздору?» at the first start and once after
//! an update, and the crash question after a crash. Both are the map import's frame and buttons (the
//! same look as the other questions). Nothing shows when the analytics are off (`telemetry::active`),
//! except in the snapshots' own switches: `RAZDOR_SCENE_CONSENT=1` and `RAZDOR_SCENE_CRASH=save|none`.

use std::sync::atomic::{AtomicBool, Ordering};

use super::map_import;
use super::widgets::esc_pressed;
use razdor::i18n::tr;
use razdor::telemetry::{self, crash, state::Consent, Hub};

/// The version the questions are asked in (asked again after an update).
const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The consent window was closed without an answer: not shown again in this run (it is asked at the next start).
static DISMISSED: AtomicBool = AtomicBool::new(false);

/// What the consent window says this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Yes,
    No,
    /// Closed (the cross or Esc): no answer is recorded.
    Closed,
    Open,
}

/// Draws the question that is due, over the screen (called from `App::frame`).
pub fn modal() {
    if snapshot_scene("RAZDOR_SCENE_CONSENT").is_some() {
        consent_window();
        return;
    }
    if let Some(with_save) = snapshot_scene("RAZDOR_SCENE_CRASH") {
        crash_window(with_save == "save");
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
