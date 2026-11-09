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
use super::widgets::{button_state, key, wrap};

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
    let (bw, bh, gap) = (260.0 * k * g, 30.0 * k * g, 8.0 * k);
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
        chrome::marble_button(b, label, true, down);
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
    fs::create_dir_all(&dir).map_err(|e| export_failed(&e.to_string()))?;
    let stamp = transfer::date_stamp(now_secs());
    if log {
        let src = razdor::diag::log_path().ok_or_else(|| export_failed(tr("there is no log file")))?;
        let name = format!("razdor-log-{stamp}.txt");
        let temp = dir.join(&name);
        fs::copy(&src, &temp).map_err(|e| export_failed(&e.to_string()))?;
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
                text => Some(export_failed(text.strip_prefix("error: ").unwrap_or(text))),
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
                    None => Some(import_failed(text.strip_prefix("error: ").unwrap_or(text))),
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
            Some(dir) => transfer::import_save(path, name, &dir).map(|_| 1),
            None => Err(tr("the app has no folder").to_string()),
        }
    } else {
        Err(tr("The file is not a Razdor save or saves archive.").to_string())
    };
    let _ = fs::remove_file(path);
    match result {
        Ok(n) => Done { message: trf!("Imported saves: {n}.", n = n), saves_changed: true },
        Err(why) => import_failed(&why),
    }
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
