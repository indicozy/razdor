//! Android: the Load window's «Перенос…» (Razdor's own, a player's request). The app's folder is
//! `Android/data/...`, which the Files app cannot open on Android 11 and later, so the saves and the
//! log go out through the system's document creator (ACTION_CREATE_DOCUMENT), and a save or a saves
//! archive comes back in through the picker, copied to the cache first. The files are written by
//! `razdor::transfer`; the picker answers on the activity's thread and is polled each frame of the
//! window (`poll`), like the map and archive pickers (`ui::map_import`, `ui::game_import`).

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};

use macroquad::prelude::*;
use razdor::i18n::tr;
use razdor::local;
use razdor::platform;
use razdor::rules::save;
use razdor::transfer;
use razdor::trf;

use super::android;
use super::chrome;
use super::map_import;
use super::widgets::{button_state, key, wrap};

/// A phone's button is this many dp high, as the phone windows' (`map_import::buttons`).
const PHONE_BUTTON_DP: f32 = 44.0;

/// What a finished action reports: the message box's text, and whether the saves changed (the list
/// is read again).
pub struct Done {
    pub message: String,
    pub saves_changed: bool,
}

/// The menu's choices.
#[derive(Clone, Copy)]
enum Choice {
    ExportSaves,
    ImportSave,
    ExportLog,
}

/// The picker's action under way.
enum Step {
    Idle,
    /// An export's picker is open; `temp` is the file it copies, removed when it answers.
    Exporting { temp: PathBuf, log: bool },
    /// The import's picker is open.
    Importing,
}

local! {
    /// The menu is open (drawn by [`menu`]).
    static MENU: Cell<bool> = const { Cell::new(false) };
    /// The picker's action under way (polled by [`poll`]).
    static STEP: RefCell<Step> = const { RefCell::new(Step::Idle) };
    /// The result of a transfer started from Settings («Прочее»), for its note window (`note_window`).
    static NOTE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// The «Перенос…» button was pressed: the menu opens (the window draws it this frame).
pub fn open_menu() {
    MENU.with(|m| m.set(true));
}

/// The menu or a picker is up: the Load window takes no clicks meanwhile.
pub fn asking() -> bool {
    MENU.with(|m| m.get()) || STEP.with(|s| !matches!(*s.borrow(), Step::Idle))
}

/// The menu's window, drawn while it is open: its three actions and «Отмена» (Escape, its close
/// box). A choice starts its action and closes the menu. Returns what the action reports at once.
pub fn menu() -> Option<Done> {
    if !MENU.with(|m| m.get()) {
        return None;
    }
    let answer = draw_menu()?;
    MENU.with(|m| m.set(false));
    match answer {
        Some(Choice::ExportSaves) => start_export(false),
        Some(Choice::ExportLog) => start_export(true),
        Some(Choice::ImportSave) => start_import(),
        None => None,
    }
}

/// The menu's window: `None` while it is open, else the answer (`Some(None)` for «Отмена»).
fn draw_menu() -> Option<Option<Choice>> {
    let k = chrome::k();
    let g = super::display::button_grow();
    let body = tr("Save the saves or the log to a file, or bring a save back from one.");
    let text_size = 13.0 * k;
    let lines = wrap(body, 340.0 * k, text_size);
    // On a phone the buttons are 44 dp high with a 14 sp label, as the other phone windows' buttons.
    let phone = super::display::dp_px().map(|dp| (dp, map_import::phone_factor()));
    let (bw, bh, gap) = match phone {
        Some((dp, f)) => ((260.0 * k * f).min(380.0 * k - 48.0 * k * f), PHONE_BUTTON_DP * dp, 12.0 * k * f),
        None => (260.0 * k * g, 30.0 * k * g, 8.0 * k),
    };
    let label_size = phone.map(|(_, f)| 14.0 * k * f);
    let choices = [
        (tr("Export saves"), Some(Choice::ExportSaves)),
        (tr("Import a save"), Some(Choice::ImportSave)),
        (tr("Export the log"), Some(Choice::ExportLog)),
        (tr("Cancel"), None),
    ];
    let w = 380.0 * k;
    let h = 40.0 * k + lines.len() as f32 * 16.0 * k + 12.0 * k + choices.len() as f32 * (bh + gap) + 12.0 * k + 28.0 * k * (g - 1.0);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let (inner, closed) = chrome::window(r, tr("Transfer"), chrome::Skin::Marble, true);
    let mut y = inner.y + 22.0 * k;
    for line in &lines {
        chrome::shadow_centered(line, inner.center().x, y, text_size, chrome::CREAM);
        y += 16.0 * k;
    }
    y += 12.0 * k;
    let mut answer = None;
    for (label, choice) in choices {
        let b = Rect::new(inner.center().x - bw / 2.0, y, bw, bh);
        let (_, down, pressed) = button_state(b, true);
        match label_size {
            Some(size) => chrome::marble_button_sized(b, label, true, down, size),
            None => chrome::marble_button(b, label, true, down),
        }
        if pressed {
            answer = Some(choice);
        }
        y += bh + gap;
    }
    if let Some(choice) = answer {
        return Some(choice);
    }
    (closed || key(KeyCode::Escape)).then_some(None)
}

/// Starts an export: the file is built in the cache first (the log copied, the saves zipped), then
/// the document creator takes the name and the player's place for it. Returns what fails at once.
fn start_export(log: bool) -> Option<Done> {
    let (temp, name, mime) = match prepare_export(log) {
        Ok(prepared) => prepared,
        Err(done) => return Some(done),
    };
    if android::start_document_export(&name, mime, &temp.to_string_lossy()) {
        STEP.with(|s| *s.borrow_mut() = Step::Exporting { temp, log });
        None
    } else {
        let _ = fs::remove_file(&temp);
        Some(export_failed(tr("the file picker did not open")))
    }
}

/// The copy an export writes (the log, or the saves zip) in the cache's `export` folder: its path,
/// the name the document creator suggests and its MIME type.
fn prepare_export(log: bool) -> Result<(PathBuf, String, &'static str), Done> {
    let cache = platform::cache_dir().ok_or_else(|| export_failed(tr("the app has no cache folder")))?;
    let dir = cache.join("export");
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).map_err(|e| io_failed(export_failed, e))?;
    let stamp = transfer::date_stamp(now_secs());
    if log {
        let src = razdor::diag::log_path().ok_or_else(|| export_failed(tr("there is no log file")))?;
        let name = format!("razdor-log-{stamp}.txt");
        let temp = dir.join(&name);
        write_log_bundle(&src, &temp).map_err(|e| io_failed(export_failed, e))?;
        return Ok((temp, name, "text/plain"));
    }
    let root = platform::app_dir().map(|d| d.join("saves")).ok_or_else(|| export_failed(tr("the app has no folder")))?;
    let name = format!("razdor-saves-{stamp}.zip");
    let temp = dir.join(&name);
    match transfer::export_saves(&root, &temp) {
        Ok(0) => Err(Done { message: tr("There are no saves to export yet.").to_string(), saves_changed: false }),
        Ok(_) => Ok((temp, name, "application/zip")),
        Err(why) => {
            let _ = fs::remove_file(&temp);
            Err(export_failed(&why))
        }
    }
}

/// The log export's file: `razdor.log`, then the play log and the newest performance report (`razdor-bench-*.txt`
/// in the app's folder), when there are, each under a heading. One text file for the document creator.
fn write_log_bundle(log: &Path, temp: &Path) -> std::io::Result<()> {
    let mut text = fs::read(log)?;
    let mut parts: Vec<PathBuf> = Vec::new();
    parts.extend(razdor::diag::play_log_path().filter(|p| p.is_file()));
    parts.extend(newest_bench_report());
    for part in parts {
        text.extend_from_slice(format!("\n\n===== {} =====\n", part.file_name().unwrap_or_default().to_string_lossy()).as_bytes());
        text.extend(fs::read(&part)?);
    }
    fs::write(temp, text)
}

/// The newest performance report the app's folder holds (`bench::save`: `razdor-bench-<date>.txt`, the names sort by date).
fn newest_bench_report() -> Option<PathBuf> {
    let mut reports: Vec<PathBuf> = fs::read_dir(platform::app_dir()?)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.starts_with("razdor-bench-") && n.ends_with(".txt")))
        .collect();
    reports.sort();
    reports.pop()
}

/// Settings → «Прочее» → «Отправить журнал…» (Android): the log goes to the document creator, as the Load
/// window's «Перенос…» does. A failure to start is kept for the note window (`note_window`); the answer
/// comes through `poll` (called each frame by the app).
pub fn start_log_export() {
    if let Some(done) = start_export(true) {
        set_note(done.message);
    }
}

/// The note of the settings' transfer (`start_log_export`), until its OK.
fn set_note(text: String) {
    NOTE.with_borrow_mut(|n| *n = Some(text));
}

/// Takes the answer of a settings transfer that ended (`poll`), for the note window. Called each frame.
pub fn settings_poll() {
    if let Some(done) = poll() {
        set_note(done.message);
    }
}

/// The note of a settings transfer: a notice window over the settings, until its OK.
pub fn note_window() {
    let Some(text) = NOTE.with_borrow(|n| n.clone()) else { return };
    if map_import::notice_window(tr("Transfer"), std::slice::from_ref(&text)) {
        NOTE.with_borrow_mut(|n| *n = None);
    }
}

/// Starts an import: the picker opens; its answer is polled.
fn start_import() -> Option<Done> {
    if android::start_pick() {
        STEP.with(|s| *s.borrow_mut() = Step::Importing);
        None
    } else {
        Some(import_failed(tr("the file picker did not open")))
    }
}

/// The picker's and the document creator's answers, polled each frame. Nothing while they are
/// open; a cancel says nothing.
pub fn poll() -> Option<Done> {
    let step = STEP.with(|s| std::mem::replace(&mut *s.borrow_mut(), Step::Idle));
    match step {
        Step::Idle => None,
        Step::Exporting { temp, log } => {
            let outcome = android::export_outcome();
            if outcome.is_empty() {
                STEP.with(|s| *s.borrow_mut() = Step::Exporting { temp, log });
                return None;
            }
            let _ = fs::remove_file(&temp);
            match outcome.as_str() {
                "ok" => {
                    let message = if log { tr("The log is exported.") } else { tr("Saves exported.") };
                    Some(Done { message: message.to_string(), saves_changed: false })
                }
                "cancel" => None,
                text => {
                    // The Java side names the step that failed (a short code); the player gets the fixed text.
                    razdor::diag!("export failed: {text}");
                    Some(export_failed(tr("the file cannot be written")))
                }
            }
        }
        Step::Importing => {
            let outcome = android::pick_outcome();
            match outcome.as_str() {
                "" => {
                    STEP.with(|s| *s.borrow_mut() = Step::Importing);
                    None
                }
                "cancel" => None,
                text => match text.strip_prefix("ok\n").and_then(|rest| rest.split_once('\n')) {
                    Some((name, path)) => Some(import_picked(name, Path::new(path))),
                    None => {
                        razdor::diag!("the picked file cannot be read: {text}");
                        Some(import_failed(tr("the file cannot be read")))
                    }
                },
            }
        }
    }
}

/// Brings the picked file in: a `.zip` (an export of the saves) into the saves folder, a `.rzsave`
/// into the manual saves of the install played now. The copy in the cache is removed.
fn import_picked(name: &str, path: &Path) -> Done {
    let lower = name.to_ascii_lowercase();
    let result = if lower.ends_with(".zip") {
        match platform::app_dir().map(|d| d.join("saves")) {
            Some(root) => transfer::import_archive(path, &root),
            None => Err(tr("the app has no folder").to_string()),
        }
    } else if lower.ends_with(".rzsave") {
        match save::manual_dir() {
            Some(dir) => transfer::import_save(path, name, &dir),
            None => Err(tr("the app has no folder").to_string()),
        }
    } else {
        Err(tr("The file is not a Razdor save or saves archive.").to_string())
    };
    let _ = fs::remove_file(path);
    match result {
        Ok(imported) => {
            let mut message = trf!("Imported saves: {n}, already there: {m}.", n = imported.saves, m = imported.already);
            if imported.settings_kept {
                message = format!("{message} {}", tr("Settings were not replaced: they are already there."));
            }
            Done { message, saves_changed: true }
        }
        Err(why) => import_failed(&why),
    }
}

/// A failed file step: the detail goes to the log, the player gets the fixed text of `done`'s kind.
fn io_failed(kind: fn(&str) -> Done, e: std::io::Error) -> Done {
    razdor::diag!("export step failed: {e}");
    kind(tr("the file cannot be written"))
}

fn export_failed(why: &str) -> Done {
    Done { message: trf!("Could not export: {why}.", why = why), saves_changed: false }
}

fn import_failed(why: &str) -> Done {
    Done { message: trf!("Could not import: {why}.", why = why), saves_changed: false }
}

/// Seconds since 1970 now.
fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}
