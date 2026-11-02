//! The Hub's package actions (`razdor::hub::packages`) in its window: the «Импорт пакета…» file
//! picker (the desktop dialog, the Android document picker; iPhone says it cannot), the notice
//! that says why a package is refused (a scrollable list of `razdor-pack check`'s diagnostics), and
//! the report window of a package's card. A report has no endpoint: its text goes to the
//! clipboard and the player's Discord invite opens, to paste it into the reports thread.

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use macroquad::miniquad::window::clipboard_set;
use macroquad::prelude::*;

use razdor::hub::{self, ImportError};
use razdor::i18n::tr;
use razdor::pack::{Diagnostic, Severity};
use razdor::{platform, trf};

use super::chrome::{self, CREAM, GOLD, RED_TEXT};
use super::main_menu;
use super::widgets::*;

/// The report window's text field key (`widgets::text_field`).
const REPORT_KEY: &str = "hub-package-report";

/// A file picker on its way: the desktop dialog's thread (its answer comes on the channel), or the
/// Android document picker (its answer through `ui::android::pick_outcome`).
pub enum Pick {
    Desktop(Receiver<Option<PathBuf>>),
    #[cfg(target_os = "android")]
    Android,
}

/// What the picker says this frame.
pub enum Answer {
    Cancelled,
    /// The picked file, and whether it is a copy in the app's cache (to delete when it is used).
    Picked(PathBuf, bool),
    /// The Android picker reported an error (its reason); the other platforms never say so.
    #[cfg_attr(not(target_os = "android"), allow(dead_code))]
    Failed(String),
}

/// The desktop's open dialog for a `.rzpack`.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn desktop_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Import a package…"))
        .add_filter(tr("Razdor packages"), &["rzpack"])
        .add_filter(tr("All files"), &["*"])
        .pick_file()
}

/// Opens the platform's file picker. On the desktop the dialog runs on a thread (on macOS on the
/// main thread, as its dialogs must); `Err` says why it cannot be opened.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub fn start_pick() -> Result<Pick, String> {
    let (tx, rx) = channel();
    if cfg!(target_os = "macos") {
        let _ = tx.send(desktop_dialog());
        return Ok(Pick::Desktop(rx));
    }
    std::thread::Builder::new()
        .name("package-picker".into())
        .spawn(move || {
            let _ = tx.send(desktop_dialog());
        })
        .map_err(|e| trf!("The file picker cannot be opened: {e}.", e = e))?;
    Ok(Pick::Desktop(rx))
}

#[cfg(target_os = "android")]
pub fn start_pick() -> Result<Pick, String> {
    if super::android::start_pick() {
        Ok(Pick::Android)
    } else {
        Err(tr("The file picker cannot be opened on this phone.").into())
    }
}

#[cfg(target_os = "ios")]
pub fn start_pick() -> Result<Pick, String> {
    Err(tr("Importing a package is not possible on iPhone yet.").into())
}

/// The picker's answer this frame. `pick` stays while the picker is open.
pub fn poll_pick(pick: &mut Option<Pick>) -> Option<Answer> {
    match pick.take()? {
        Pick::Desktop(rx) => match rx.try_recv() {
            Ok(Some(path)) => Some(Answer::Picked(path, false)),
            Ok(None) | Err(TryRecvError::Disconnected) => Some(Answer::Cancelled),
            Err(TryRecvError::Empty) => {
                *pick = Some(Pick::Desktop(rx));
                None
            }
        },
        #[cfg(target_os = "android")]
        Pick::Android => {
            let outcome = super::android::pick_outcome();
            match outcome.as_str() {
                "" => {
                    *pick = Some(Pick::Android);
                    None
                }
                "cancel" => Some(Answer::Cancelled),
                text if text.starts_with("error: ") => Some(Answer::Failed(text["error: ".len()..].to_string())),
                text => match text.strip_prefix("ok\n").and_then(|rest| rest.split_once('\n')) {
                    Some((_, path)) => Some(Answer::Picked(PathBuf::from(path), true)),
                    None => Some(Answer::Failed(trf!("The picked file cannot be read: {why}.", why = text.to_string()))),
                },
            }
        }
    }
}

/// The lines that say why a package is refused: each diagnostic as `error: file:line: message
/// (hint: …)`, errors first, then the warnings.
pub fn refused_lines(diags: &[Diagnostic]) -> Vec<String> {
    let mut ordered: Vec<&Diagnostic> = diags.iter().collect();
    ordered.sort_by_key(|d| d.severity != razdor::pack::Severity::Error);
    ordered
        .iter()
        .map(|d| {
            let sev = if d.severity == razdor::pack::Severity::Error { tr("error") } else { tr("warning") };
            let at = match (d.file.is_empty(), d.line) {
                (true, _) => String::new(),
                (false, Some(line)) => format!("{}:{line}: ", d.file),
                (false, None) => format!("{}: ", d.file),
            };
            let hint = d.hint.as_ref().map(|h| format!(" ({}: {h})", tr("hint"))).unwrap_or_default();
            format!("{sev}: {at}{}{hint}", d.message)
        })
        .collect()
}

/// The text a failed import says: the reason, or the refused package's diagnostics.
pub fn import_notice(err: &ImportError) -> Notice {
    match err {
        ImportError::Failed(why) => Notice::new(tr("The package is not imported"), vec![why.clone()]),
        ImportError::Refused(diags) => {
            let n = diags.iter().filter(|d| d.severity == razdor::pack::Severity::Error).count();
            let mut lines = vec![trf!("The package does not pass the check (errors: {n}), so it is not imported.", n = n)];
            lines.extend(refused_lines(diags));
            Notice::new(tr("The package is not imported"), lines)
        }
    }
}

/// Diagnostics as a refused package's check gives them: a sample for the off-screen shot (`hub-refused`).
pub fn sample_refusal() -> Vec<Diagnostic> {
    let d = |severity, file: &str, line, message: &str, hint: Option<&str>| Diagnostic {
        severity,
        file: file.into(),
        line,
        message: message.into(),
        hint: hint.map(Into::into),
    };
    vec![
        d(Severity::Error, "units/heroes.toml", Some(12), "unknown field `cst`", Some("did you mean `cost`?")),
        d(Severity::Error, "maps/first-light/map.toml", Some(31), "a troop slot's count is 0", None),
        d(Severity::Error, "maps/first-light/map.toml", None, "the victory event names a building that is not on the map", Some("add the [[building]] or change `army`")),
        d(Severity::Warning, "texts/en.toml", None, "the text key `unit.dawn_knight.description` is missing in English", None),
    ]
}

/// A notice in the window: its title and its lines, wrapped and scrolled by the wheel and the bar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub lines: Vec<String>,
    pub scroll: usize,
}

impl Notice {
    pub fn new(title: &str, lines: Vec<String>) -> Notice {
        Notice { title: title.to_string(), lines, scroll: 0 }
    }
}

/// Draws a notice; true when it is closed (OK, the close box, Enter or Escape).
pub fn notice_window(n: &mut Notice) -> bool {
    let k = chrome::k();
    let (w, h) = (min_w(560.0 * k), min_h(420.0 * k));
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let (inner, closed) = chrome::window(r, &n.title, chrome::Skin::Marble, true);
    let size = (14.0 * k).round();
    let pad = 12.0 * k;
    let bh = 34.0 * k;
    let btn = Rect::new(inner.x + inner.w - pad - 120.0 * k, inner.y + inner.h - pad - bh, 120.0 * k, bh);
    let area = Rect::new(inner.x + pad, inner.y + pad, inner.w - 2.0 * pad - 14.0 * k, btn.y - inner.y - 2.0 * pad);
    let row = size * 1.35;
    let mut rows: Vec<(String, Color)> = Vec::new();
    for (i, line) in n.lines.iter().enumerate() {
        let ink = if line.starts_with(tr("error")) { RED_TEXT } else if i == 0 { GOLD } else { CREAM };
        for part in wrap(line, area.w, size) {
            rows.push((part, ink));
        }
    }
    let shown = (area.h / row).floor().max(1.0) as usize;
    let most = rows.len().saturating_sub(shown);
    if !input_blocked() && mouse_in(area.x, area.y, area.w, area.h) {
        let wh = wheel();
        if wh < 0.0 {
            n.scroll = (n.scroll + 1).min(most);
        } else if wh > 0.0 {
            n.scroll = n.scroll.saturating_sub(1);
        }
    }
    n.scroll = n.scroll.min(most);
    for (i, (text, ink)) in rows.iter().enumerate().skip(n.scroll).take(shown) {
        chrome::shadow_text(text, area.x, area.y + size + (i - n.scroll) as f32 * row, size, *ink);
    }
    if most > 0 {
        let bar = Rect::new(inner.x + inner.w - pad - 10.0 * k, area.y, 10.0 * k, area.h);
        scrollbar(bar, &mut n.scroll, shown, rows.len());
    }
    let ok = button(btn.x, btn.y, btn.w, btn.h, tr("OK"), true);
    let enter = key(KeyCode::Enter) || key(KeyCode::Escape);
    if ok || closed || enter {
        return true;
    }
    false
}

/// The report window's form: the package it is about and the player's words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportForm {
    pub id: String,
    pub version: String,
    pub title: String,
    pub text: String,
}

/// What the report window did this frame.
pub enum ReportAnswer {
    Open,
    Cancel,
    Send(String),
}

/// The text the report carries to the reports thread: the package and the player's words.
pub fn report_text(id: &str, version: &str, text: &str) -> String {
    format!("Пакет {id} {version}: {}", text.trim())
}

/// Puts `text` on the clipboard and opens the reports link in the browser (the player pastes it).
pub fn send_report(text: &str) {
    clipboard_set(text);
    main_menu::open_link(hub::REPORTS_LINK);
}

/// Draws the report window; Send needs some words. The text is the player's, the link is the
/// reports thread's (`hub::REPORTS_LINK`).
pub fn report_window(form: &mut ReportForm) -> ReportAnswer {
    let k = chrome::k();
    let (w, h) = (min_w(560.0 * k), min_h(260.0 * k));
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let title = trf!("Report a problem: {title}", title = form.title);
    let (inner, closed) = chrome::window(r, &title, chrome::Skin::Marble, true);
    let size = (14.0 * k).round();
    let pad = 12.0 * k;
    let bh = 34.0 * k;
    let by = inner.y + inner.h - pad - bh;
    chrome::shadow_text(tr("What is wrong? The text is copied, and Discord opens: paste it into the reports thread."), inner.x + pad, inner.y + pad + size, size, CREAM);
    text_field(REPORT_KEY, inner.x + pad, inner.y + pad + size * 1.6, inner.w - 2.0 * pad, bh, &mut form.text, false);
    let send_w = 150.0 * k;
    let cancel = button(inner.x + pad, by, 130.0 * k, bh, tr("Cancel"), true);
    let send = button(inner.x + inner.w - pad - send_w, by, send_w, bh, tr("Send"), !form.text.trim().is_empty());
    if cancel || closed || key(KeyCode::Escape) {
        return ReportAnswer::Cancel;
    }
    if send {
        return ReportAnswer::Send(report_text(&form.id, &form.version, &form.text));
    }
    ReportAnswer::Open
}

/// A window's width: `want`, less the screen's margins.
fn min_w(want: f32) -> f32 {
    want.min(screen_width() - 20.0)
}

/// A window's height: `want`, less the screen's margins.
fn min_h(want: f32) -> f32 {
    want.min(screen_height() - 20.0)
}

/// Imports the packages named on the command line (`razdor FILE.rzpack`, and an «open with» that
/// passes a file): each is checked and kept in the Hub's folder (`Hub::import_package`). A refusal
/// is a notice on the game's screens. Returns the notices, for the caller to show.
pub fn import_args(args: &[String]) -> Vec<Notice> {
    let Some(local) = hub::local_dir() else { return Vec::new() };
    let hub = hub::Hub { source: hub::Source::from_env(), local, trust_dir: platform::app_dir() };
    let mut notices = Vec::new();
    let mut skip = false;
    for a in args {
        if std::mem::take(&mut skip) {
            continue;
        }
        match a.as_str() {
            "--pack" | "--map" => skip = true,
            path if path.to_ascii_lowercase().ends_with(".rzpack") => match hub.import_package(std::path::Path::new(path)) {
                Ok(id) => razdor::diag!("hub: imported the package {id} from {path}"),
                Err(e) => notices.push(import_notice(&e)),
            },
            _ => {}
        }
    }
    notices
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::pack::Severity;

    fn diag(sev: Severity, file: &str, line: Option<u32>, msg: &str, hint: Option<&str>) -> Diagnostic {
        Diagnostic { severity: sev, file: file.into(), line, message: msg.into(), hint: hint.map(Into::into) }
    }

    #[test]
    fn the_refusal_lists_errors_first_with_file_line_and_hint() {
        let lines = refused_lines(&[
            diag(Severity::Warning, "texts/en.toml", None, "a key is missing", None),
            diag(Severity::Error, "units/a.toml", Some(12), "unknown field `cst`", Some("did you mean `cost`")),
        ]);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with(&format!("{}: units/a.toml:12: ", tr("error"))), "{}", lines[0]);
        assert!(lines[0].contains("did you mean"), "{}", lines[0]);
        assert!(lines[1].starts_with(&format!("{}: texts/en.toml: ", tr("warning"))), "{}", lines[1]);
    }

    #[test]
    fn a_report_carries_the_package_and_the_words() {
        assert_eq!(report_text("elves-pack", "1.2.0", "  второй бой не начинается  "), "Пакет elves-pack 1.2.0: второй бой не начинается");
    }

    #[test]
    fn a_failed_import_is_a_notice_with_its_reason() {
        let n = import_notice(&ImportError::Failed("x.rzpack: not a .rzpack file".into()));
        assert_eq!(n.lines, vec!["x.rzpack: not a .rzpack file".to_string()]);
        let n = import_notice(&ImportError::Refused(vec![diag(Severity::Error, "package.toml", Some(1), "bad", None)]));
        assert!(n.lines[0].contains("errors: 1"), "{:?}", n.lines);
        assert_eq!(n.lines.len(), 2);
    }
}
