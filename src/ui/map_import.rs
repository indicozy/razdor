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
use razdor::rules::map_refs;
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

/// Refuses the start of `entry` when it names units or items the content lacks: the reason is
/// shown in a notice, and true is returned (a map of a mod the install does not have).
pub fn refused(entry: &ScenarioEntry, content: &Content) -> bool {
    let unknown = map_refs::unknown(&entry.scenario, content);
    if unknown.is_empty() {
        return false;
    }
    let title = if entry.scenario.title.trim().is_empty() { entry.file.as_str() } else { entry.scenario.title.as_str() };
    let text = trf!("The map «{title}» uses units or items that are not in your game ({ids}). It needs a mod, so it cannot be started.", title, ids = unknown.text());
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
                    ids = unknown.text()
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
    // The base install's maps (a mod's merged folder is built from it, so a map goes there).
    let base = razdor::dt::install::base_dir().unwrap_or_else(|| install.dir.clone());
    let folder = map_import::maps_folder(&base);
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
    let (inner, closed, _) = frame_bar(title, lines, None, with_buttons);
    (inner, closed)
}

/// [`frame`] with a progress bar at `share` (0 to 1) when `share` is given. On a phone the bar is
/// drawn here, full width under the text (the text, the bar, then the buttons from
/// [`buttons`], one below the other); on the desktop the caller draws the bar at its place, which
/// is returned (`Some`).
pub(super) fn frame_bar(title: &str, lines: &[String], share: Option<f32>, with_buttons: bool) -> (Rect, bool, Option<Rect>) {
    let k = chrome::k();
    let f = phone_factor();
    let phone = f > 1.0;
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.0, 0.0, 0.0, 0.35));
    let w = (460.0 * k * f).min(screen_width() - 24.0 * k);
    let size = 14.0 * k * f;
    let text_w = w - 48.0 * k * f;
    let wrapped: Vec<String> = lines.iter().flat_map(|l| wrap(l, text_w, size)).collect();
    let button_area = if with_buttons { button_area(k, f) } else { 0.0 };
    // The bar's own space and the gap above the buttons (a phone's centred button sits under it).
    let bar_area = match super::display::dp_px() {
        Some(dp) if share.is_some() => 24.0 * k * f + 24.0 * dp,
        _ => 0.0,
    };
    let h = (92.0 * k * f + wrapped.len() as f32 * 20.0 * k * f + button_area + bar_area).min(screen_height() - 16.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    // A phone's title grows with the text (the window grows with it); the desktop's is as it was.
    let (inner, closed) = if phone {
        chrome::window_grown(r, title, chrome::Skin::Marble, true)
    } else {
        chrome::window(r, title, chrome::Skin::Marble, true)
    };
    let text_bottom = 34.0 * k * f + wrapped.len() as f32 * 20.0 * k * f;
    for (i, line) in wrapped.iter().enumerate() {
        chrome::shadow_text(line, inner.x + 24.0 * k * f, inner.y + 34.0 * k * f + i as f32 * 20.0 * k * f, size, chrome::CREAM);
    }
    match share {
        Some(share) if phone => {
            let bar = Rect::new(inner.x + 24.0 * k * f, inner.y + text_bottom + 4.0 * k * f, inner.w - 48.0 * k * f, 14.0 * k * f);
            draw_bar(bar, share);
            (inner, closed, None)
        }
        Some(_) => (inner, closed, Some(Rect::new(inner.x + 24.0 * k, inner.y + 60.0 * k, inner.w - 48.0 * k, 14.0 * k))),
        None => (inner, closed, None),
    }
}

/// A progress bar: a dark trough with the amber share of it on top.
pub(super) fn draw_bar(bar: Rect, share: f32) {
    draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::new(0.0, 0.0, 0.0, 0.45));
    draw_rectangle(bar.x, bar.y, bar.w * share.clamp(0.0, 1.0), bar.h, Color::new(0.85, 0.65, 0.25, 1.0));
    draw_rectangle_lines(bar.x, bar.y, bar.w, bar.h, 1.0, Color::new(0.0, 0.0, 0.0, 0.8));
}

/// The phones' text and touch targets, in these windows: the text is this factor times the
/// desktop's (about 14 sp, the phone rule), and a button is at least 44 dp.
const PHONE_TEXT: f32 = 2.5;
/// A phone button's height, in dp.
const PHONE_TOUCH_DP: f32 = 44.0;

/// The text factor of these windows: [`PHONE_TEXT`] on a phone, 1 on the desktop.
pub(super) fn phone_factor() -> f32 {
    if super::display::dp_px().is_some() { PHONE_TEXT } else { 1.0 }
}

/// The height the buttons take at a window's bottom, with the gap above them.
fn button_area(k: f32, f: f32) -> f32 {
    match super::display::dp_px() {
        Some(dp) => PHONE_TOUCH_DP * dp + 16.0 * k * f,
        None => 44.0 * k,
    }
}

/// The window's buttons at its bottom right (as the update window's): the pressed one's index.
/// On a phone a button is 44 dp high, its label at the phone's text size, and the buttons share
/// the window's width when they must.
pub(super) fn buttons(inner: Rect, labels: &[&str]) -> Option<usize> {
    let k = chrome::k();
    let f = phone_factor();
    let n = labels.len() as f32;
    let (w, h, gap, y) = match super::display::dp_px() {
        Some(dp) => {
            let gap = 12.0 * k * f;
            let h = PHONE_TOUCH_DP * dp;
            let w = (150.0 * k * f).min((inner.w - 48.0 * k * f - (n - 1.0) * gap) / n);
            (w, h, gap, inner.y + inner.h - h - 12.0 * k * f)
        }
        None => (150.0 * k, 28.0 * k, 12.0 * k, inner.y + inner.h - 40.0 * k),
    };
    let total = n * w + (n - 1.0) * gap;
    // A phone centres its row (the one button under the text); the desktop keeps it at the right.
    let mut x = match super::display::dp_px() {
        Some(_) => inner.x + (inner.w - total) / 2.0,
        None => inner.x + inner.w - 24.0 * k * f - total,
    };
    let mut picked = None;
    for (i, label) in labels.iter().enumerate() {
        let r = Rect::new(x, y, w, h);
        let (_, down, pressed) = button_state(r, true);
        if f > 1.0 {
            chrome::marble_button_sized(r, label, true, down, 14.0 * k * f);
        } else {
            chrome::marble_button(r, label, true, down);
        }
        if pressed {
            cue(Cue::Button);
            picked = Some(i);
        }
        x += w + gap;
    }
    picked
}
