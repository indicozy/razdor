//! «Добавить карту…» in the new-game window: the platform's file picker, the import of the picked
//! maps into the install's maps folder (`dt::map_import`), and the windows that answer it (a
//! question when a name is taken, a notice for what was added or why not).
//!
//! The pick: the desktop's own dialog (`rfd`) on a thread, or on the main thread on macOS, where
//! its dialogs must be; Android's document picker through the JNI (`ui::android`), which the
//! Java side copies into the app's cache and reports when the player has chosen. iOS has no
//! picker yet, so the button is not offered there (`available`).
//!
//! A map that names unit types or items the install lacks (a mod's) is warned about at the import
//! and refused at the start (`refused`): `Content::unit` would panic on it.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};

use macroquad::prelude::*;
use razdor::dt::install::DtInstall;
use razdor::dt::map_import::{self, ImportError, Map};
use razdor::i18n::tr;
use razdor::local;
use razdor::rules::content::Content;
use razdor::rules::map_refs::{self, Unknown};
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome;
use super::widgets::*;
use super::ScenarioEntry;

/// The button is offered: every platform but iOS (no picker there yet).
pub fn available() -> bool {
    !cfg!(target_os = "ios")
}

/// A picked file: its name, where its bytes are, and whether the copy there is Razdor's to delete.
struct Picked {
    name: String,
    path: PathBuf,
    cached: bool,
}

/// Where the pick stands.
#[derive(Default)]
enum Picker {
    #[default]
    Idle,
    /// The desktop dialog's answer (`None`: cancelled). Its thread sends it; on macOS, where the
    /// dialog must run on the main thread, the answer is sent before the picker is polled.
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    Desktop(std::sync::mpsc::Receiver<Option<PathBuf>>),
    /// The Android picker is open: its answer comes through `ui::android::pick_outcome`.
    #[cfg(target_os = "android")]
    Android,
}

/// A window of this module, shown over the screen until the player answers it.
enum Modal {
    /// What was added, or why nothing was: one line of text per entry, then OK.
    Notice { title: String, lines: Vec<String> },
    /// Maps whose names the folder already has: replace them, or add only the new ones.
    Replace { folder: PathBuf, maps: Vec<Map>, taken: Vec<String>, warnings: Vec<String> },
}

local! {
    static REQUESTED: Cell<bool> = const { Cell::new(false) };
    static PICKER: RefCell<Picker> = const { RefCell::new(Picker::Idle) };
    static MODAL: RefCell<Option<Modal>> = const { RefCell::new(None) };
}

/// The button was pressed: the picker opens at the next frame (`tick`).
pub fn request() {
    REQUESTED.with(|r| r.set(true));
}

/// Whether the button can be pressed now: no pick under way and no window of this module open.
pub fn can_start() -> bool {
    !busy()
}

/// True while a window of this module is open (it takes the input, as a dialog does).
pub fn open() -> bool {
    MODAL.with_borrow(|m| m.is_some())
}

fn busy() -> bool {
    open() || PICKER.with_borrow(|p| !matches!(p, Picker::Idle))
}

/// Shows `lines` under `title` in a notice.
pub fn notice(title: &str, lines: Vec<String>) {
    MODAL.with_borrow_mut(|m| *m = Some(Modal::Notice { title: title.to_string(), lines }));
}

/// The words for the ids a map names that the install lacks: «воины 104, 105; предметы 35».
pub fn missing_text(u: &Unknown) -> String {
    let units = u.units.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
    let items = u.items.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
    match (u.units.is_empty(), u.items.is_empty()) {
        (false, false) => trf!("units {units}; items {items}", units, items),
        (false, true) => trf!("units {units}", units),
        _ => trf!("items {items}", items),
    }
}

/// Refuses the start of `entry` when it names units or items the content lacks: the reason is
/// shown in a notice, and true is returned (a map of a mod the install does not have).
pub fn refused(entry: &ScenarioEntry, content: &Content) -> bool {
    let unknown = map_refs::unknown(&entry.scenario, content);
    if unknown.is_empty() {
        return false;
    }
    let title = if entry.scenario.title.trim().is_empty() { entry.file.as_str() } else { entry.scenario.title.as_str() };
    let text = trf!("The map «{title}» uses units or items that are not in your game ({ids}). It needs a mod, so it cannot be started.", title, ids = missing_text(&unknown));
    notice(tr("Add a map…"), vec![text]);
    true
}

/// The warnings a map being imported earns: the same reasons `refused` gives, as the import
/// words them.
fn warnings(maps: &[Map], content: &Content) -> Vec<String> {
    maps.iter()
        .filter_map(|m| {
            let unknown = map_refs::unknown(&m.scenario, content);
            (!unknown.is_empty()).then(|| {
                trf!(
                    "«{file}» uses units or items that are not in your game ({ids}): it needs a mod, and it may work wrongly. It cannot be started.",
                    file = m.file_name.clone(),
                    ids = missing_text(&unknown)
                )
            })
        })
        .collect()
}

/// Each frame: opens a requested picker, takes its answer and imports the file picked. Returns
/// the names of the maps written this frame (the caller reads the maps list again), if any.
pub fn tick(install: Option<&DtInstall>, content: Option<&Content>) -> Option<Vec<String>> {
    let Some(install) = install else {
        REQUESTED.with(|r| r.set(false));
        return None;
    };
    if REQUESTED.with(|r| r.replace(false)) && can_start() {
        start_picker();
    }
    let picked = match poll_picker() {
        Poll::Nothing | Poll::Cancelled => return None,
        #[cfg(target_os = "android")]
        Poll::Failed(why) => {
            notice(tr("Add a map…"), vec![why]);
            return None;
        }
        Poll::Got(picked) => picked,
    };
    // The size is checked before the file is read (`read_limited`): a huge pick is refused.
    let read = map_import::read_limited(&picked.path, map_import::picked_limit(&picked.name));
    if picked.cached {
        let _ = std::fs::remove_file(&picked.path);
    }
    let bytes = match read {
        Ok(Some(bytes)) => bytes,
        Ok(None) => {
            notice(tr("Add a map…"), vec![import_error(&ImportError::TooBig(picked.name.clone()))]);
            return None;
        }
        Err(e) => {
            notice(tr("Add a map…"), vec![trf!("Cannot read «{name}»: {e}.", name = picked.name, e = e)]);
            return None;
        }
    };
    let maps = match map_import::read_picked(&picked.name, &bytes) {
        Ok(maps) => maps,
        Err(e) => {
            notice(tr("Add a map…"), vec![import_error(&e)]);
            return None;
        }
    };
    let folder = map_import::maps_folder(&install.dir);
    let warnings = content.map(|c| warnings(&maps, c)).unwrap_or_default();
    let taken = map_import::conflicts(&folder, &maps);
    if taken.is_empty() {
        return write(&folder, &maps, false, &warnings);
    }
    MODAL.with_borrow_mut(|m| *m = Some(Modal::Replace { folder, maps, taken, warnings }));
    None
}

/// The window of this module, drawn over the screen (after it, so it is on top). Returns the
/// names of the maps written when the player answered a question with a copy to make.
pub fn modal() -> Option<Vec<String>> {
    let modal = MODAL.with_borrow_mut(|m| m.take())?;
    match modal {
        Modal::Notice { title, lines } => {
            if notice_window(&title, &lines) {
                return None;
            }
            MODAL.with_borrow_mut(|m| *m = Some(Modal::Notice { title, lines }));
            None
        }
        Modal::Replace { folder, maps, taken, warnings } => match replace_window(&taken) {
            None => {
                MODAL.with_borrow_mut(|m| *m = Some(Modal::Replace { folder, maps, taken, warnings }));
                None
            }
            Some(replace) => write(&folder, &maps, replace, &warnings),
        },
    }
}

/// Copies `maps` into `folder` and says what happened (in a notice). The names written, if any.
fn write(folder: &Path, maps: &[Map], replace: bool, warnings: &[String]) -> Option<Vec<String>> {
    match map_import::write_maps(folder, maps, replace) {
        Ok(written) => {
            let mut lines = if written.is_empty() {
                vec![tr("Nothing was added: the maps are already in the game.").to_string()]
            } else {
                vec![trf!("Maps added: {n}", n = written.len()), written.join(", ")]
            };
            lines.extend(warnings.iter().cloned());
            notice(tr("Add a map…"), lines);
            Some(written).filter(|w| !w.is_empty())
        }
        Err(e) => {
            notice(tr("Add a map…"), vec![trf!("Cannot copy the map: {e}.", e = e)]);
            None
        }
    }
}

fn import_error(e: &ImportError) -> String {
    match e {
        ImportError::Unsupported => tr("Pick a .DTm file or a .zip archive.").to_string(),
        ImportError::NotAMap(name) => trf!("«{name}» is not a Discord Times map.", name = name.clone()),
        ImportError::BadArchive(why) => trf!("The archive cannot be read: {why}.", why = why.clone()),
        ImportError::NoMaps => tr("The archive holds no Discord Times maps (.DTm).").to_string(),
        ImportError::TooBig(name) => trf!("«{name}» is too big to be a Discord Times map.", name = name.clone()),
    }
}

enum Poll {
    Nothing,
    Cancelled,
    /// The Android picker reported an error (its reason).
    #[cfg(target_os = "android")]
    Failed(String),
    Got(Picked),
}

/// Opens the platform's picker (the state is then `Desktop` or `Android`).
fn start_picker() {
    #[cfg(target_os = "android")]
    {
        let state = if super::android::start_pick() {
            Picker::Android
        } else {
            notice(tr("Add a map…"), vec![tr("The file picker cannot be opened on this phone.").to_string()]);
            Picker::Idle
        };
        PICKER.with_borrow_mut(|p| *p = state);
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        if cfg!(target_os = "macos") {
            // macOS: the dialog runs here, on the main thread, as its dialogs must.
            let _ = tx.send(desktop_dialog());
            PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx));
        } else {
            let spawned = std::thread::Builder::new().name("map-picker".into()).spawn(move || {
                let _ = tx.send(desktop_dialog());
            });
            match spawned {
                Ok(_) => PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx)),
                Err(e) => notice(tr("Add a map…"), vec![trf!("The file picker cannot be opened: {e}.", e = e)]),
            }
        }
    }
}

/// The desktop's open-file dialog: a Discord Times map or an archive, or any file.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn desktop_dialog() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title(tr("Add a map…"))
        .add_filter(tr("Discord Times maps and archives"), &["DTm", "dtm", "zip"])
        .add_filter(tr("All files"), &["*"])
        .pick_file()
}

/// The picker's answer this frame, if it has one.
fn poll_picker() -> Poll {
    let state = PICKER.with_borrow_mut(std::mem::take);
    match state {
        Picker::Idle => Poll::Nothing,
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        Picker::Desktop(rx) => match rx.try_recv() {
            Ok(answer) => from_desktop(answer),
            Err(std::sync::mpsc::TryRecvError::Empty) => {
                PICKER.with_borrow_mut(|p| *p = Picker::Desktop(rx));
                Poll::Nothing
            }
            // The dialog's thread ended without an answer: taken as cancelled.
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Poll::Cancelled,
        },
        #[cfg(target_os = "android")]
        Picker::Android => {
            let outcome = super::android::pick_outcome();
            match outcome.as_str() {
                "" => {
                    PICKER.with_borrow_mut(|p| *p = Picker::Android);
                    Poll::Nothing
                }
                "cancel" => Poll::Cancelled,
                text if text.starts_with("error: ") => Poll::Failed(text["error: ".len()..].to_string()),
                text => match text.strip_prefix("ok\n").and_then(|rest| rest.split_once('\n')) {
                    Some((name, path)) => Poll::Got(Picked { name: name.to_string(), path: PathBuf::from(path), cached: true }),
                    None => Poll::Failed(trf!("The picked file cannot be read: {why}.", why = text.to_string())),
                },
            }
        }
    }
}

#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn from_desktop(answer: Option<PathBuf>) -> Poll {
    match answer {
        None => Poll::Cancelled,
        Some(path) => {
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            Poll::Got(Picked { name, path, cached: false })
        }
    }
}

/// A notice: its text, wrapped at the window's width, and OK (or the close box, Enter or Escape).
/// True when it is closed.
pub(super) fn notice_window(title: &str, lines: &[String]) -> bool {
    let (inner, closed) = frame(title, lines, true);
    let ok = buttons(inner, &[tr("OK")]).is_some();
    let enter = key(KeyCode::Enter) || key(KeyCode::Escape);
    if ok || closed || enter {
        cue(Cue::Button);
        return true;
    }
    false
}

/// The question: Replace (`Some(true)`), keep the old ones (`Some(false)`), or no answer yet.
fn replace_window(taken: &[String]) -> Option<bool> {
    let text = trf!("These maps are already in the game: {names}. Replace them?", names = taken.join(", "));
    let (inner, closed) = frame(tr("Replace?"), &[text], true);
    match buttons(inner, &[tr("Replace"), tr("Keep the old")]) {
        Some(0) => Some(true),
        Some(_) => Some(false),
        None if closed || key(KeyCode::Escape) => Some(false),
        None => None,
    }
}

/// The window's frame at its width, with its lines wrapped; returns its inside and whether its
/// close box was pressed.
pub(super) fn frame(title: &str, lines: &[String], with_buttons: bool) -> (Rect, bool) {
    let k = chrome::k();
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35));
    let w = (460.0 * k).min(screen_width() - 24.0 * k);
    let size = 14.0 * k;
    let text_w = w - 48.0 * k;
    let wrapped: Vec<String> = lines.iter().flat_map(|l| wrap(l, text_w, size)).collect();
    let h = (92.0 * k + wrapped.len() as f32 * 20.0 * k + if with_buttons { 44.0 * k } else { 0.0 }).min(screen_height() - 16.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let (inner, closed) = chrome::window(r, title, chrome::Skin::Marble, true);
    for (i, line) in wrapped.iter().enumerate() {
        chrome::shadow_text(line, inner.x + 24.0 * k, inner.y + 34.0 * k + i as f32 * 20.0 * k, size, chrome::CREAM);
    }
    (inner, closed)
}

/// The window's buttons at its bottom right (as the update window's): the pressed one's index.
pub(super) fn buttons(inner: Rect, labels: &[&str]) -> Option<usize> {
    let k = chrome::k();
    let (w, h, gap) = (150.0 * k, 28.0 * k, 12.0 * k);
    let mut x = inner.x + inner.w - 24.0 * k - labels.len() as f32 * (w + gap) + gap;
    let y = inner.y + inner.h - 40.0 * k;
    let mut picked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::new(x, y, w, h);
        let (_, down, pressed) = button_state(r, true);
        chrome::marble_button(r, label, true, down);
        if pressed {
            cue(Cue::Button);
            picked = Some(i);
        }
        x += w + gap;
    }
    picked
}
