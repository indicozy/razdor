//! Razdor Studio's start page (UX1, spec §11): what the studio shows with no package folder open (the
//! main menu's «Редактор», and after «Закрыть пакет»). The recent packages (up to 8, each with its
//! name, its map count and when it was opened), the actions (a new package, a folder to open, the
//! example «Пепельная Марка»), the wiki's guide and the version. The folder pickers are the desktop's
//! own dialogs (`rfd`), run on a thread like the map import's (on macOS on the main thread).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::time::{SystemTime, UNIX_EPOCH};

use egui_macroquad::egui;
use egui::{Align, Button, Color32, Frame, Layout, RichText, TextEdit};
use egui_phosphor::regular as ic;
use razdor::i18n::tr;
use razdor::studio::newpkg;

use super::theme;
use crate::ui::audio::{RecentPackage, StudioPrefs};

/// The packages the start page lists (the most recent first).
pub const RECENT_MAX: usize = 8;
/// The studio's guide in the player wiki (opened from the page).
const GUIDE_URL: &str = "https://indicozy.github.io/razdor/wiki/tools/map-editor/";
/// The example package the page offers (`examples/packs`).
const EXAMPLE: &str = "examples/packs/pepelnaya-marka";

/// What the start page asks of the game after a frame.
pub enum Action {
    None,
    /// Back to the main menu.
    Back,
    /// The package folder to open.
    Open(PathBuf),
}

/// Which folder a picker is for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum For {
    /// The package to open.
    Open,
    /// The folder the new package goes into.
    Parent,
}

/// A recent package as the page lists it: its folder, name, map count and when it was opened.
struct Entry {
    path: PathBuf,
    name: String,
    maps: usize,
    opened: u64,
}

/// The start page's state.
pub struct StartPage {
    picker: Option<(For, Receiver<Option<PathBuf>>)>,
    new_name: String,
    new_parent: Option<PathBuf>,
    new_open: bool,
    error: Option<String>,
    recent: Vec<Entry>,
    example: Option<PathBuf>,
}

impl Default for StartPage {
    fn default() -> StartPage {
        StartPage {
            picker: None,
            new_name: "my-package".into(),
            new_parent: dirs::home_dir(),
            new_open: false,
            error: None,
            recent: Vec::new(),
            example: example_folder(),
        }
    }
}

/// The example package's folder: the working directory's `examples/packs`, else the source tree's.
fn example_folder() -> Option<PathBuf> {
    let here = PathBuf::from(EXAMPLE);
    if here.join("package.toml").is_file() {
        return Some(here);
    }
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join(EXAMPLE);
    src.join("package.toml").is_file().then_some(src)
}

/// Opens a folder dialog for `title`. The answer comes on the receiver (`None` when cancelled).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn pick_folder(title: String) -> Result<Receiver<Option<PathBuf>>, String> {
    let (tx, rx) = channel();
    let dialog = move || rfd::FileDialog::new().set_title(title).pick_folder();
    if cfg!(target_os = "macos") {
        let _ = tx.send(dialog());
        return Ok(rx);
    }
    std::thread::Builder::new()
        .name("studio-folder-picker".into())
        .spawn(move || {
            let _ = tx.send(dialog());
        })
        .map_err(|e| format!("{e}"))?;
    Ok(rx)
}

/// One frame of the start page, at the studio's scale (`scale` in per cent).
pub fn frame(page: &mut StartPage, prefs: &StudioPrefs) -> Action {
    macroquad::prelude::clear_background(macroquad::prelude::Color::new(0.086, 0.09, 0.106, 1.0));
    crate::ui::cursor::egui_pointer(true);
    let mut action = Action::None;
    egui_macroquad::ui(|ctx| {
        theme::apply(ctx, prefs.scale);
        action = page.show(ctx);
    });
    egui_macroquad::draw();
    action
}

/// Records a package folder as opened now: it goes first in the recent list, once, at most `RECENT_MAX`.
pub fn remember(prefs: &mut StudioPrefs, folder: &Path) {
    let path = folder.display().to_string();
    prefs.recent.retain(|r| r.path != path);
    let opened = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    prefs.recent.insert(0, RecentPackage { path, opened });
    prefs.recent.truncate(RECENT_MAX);
}

/// The number of maps a package folder has (`maps/<id>/map.toml`).
fn map_count(folder: &Path) -> usize {
    std::fs::read_dir(folder.join("maps"))
        .map(|rd| rd.flatten().filter(|e| e.path().join("map.toml").is_file()).count())
        .unwrap_or(0)
}

/// «2026-11-04 17:50» (UTC) of a time in seconds since the Unix epoch.
fn when(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days, for the proleptic Gregorian calendar.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}", rem / 3600, rem % 3600 / 60)
}

impl StartPage {
    /// The start page with the recent packages (the ones still on disk, most recent first).
    pub fn with_recent(recent: &[RecentPackage]) -> StartPage {
        let entries = recent
            .iter()
            .filter_map(|r| {
                let path = PathBuf::from(&r.path);
                path.join("package.toml").is_file().then(|| Entry {
                    name: path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned()),
                    maps: map_count(&path),
                    opened: r.opened,
                    path,
                })
            })
            .take(RECENT_MAX)
            .collect();
        StartPage { recent: entries, ..StartPage::default() }
    }

    /// The picker's answer, if it has come.
    fn poll(&mut self) -> Option<(For, Option<PathBuf>)> {
        let (purpose, rx) = self.picker.as_ref()?;
        let answer = match rx.try_recv() {
            Ok(answer) => Some((*purpose, answer)),
            Err(TryRecvError::Empty) => return None,
            // The dialog's thread ended without an answer: taken as cancelled.
            Err(TryRecvError::Disconnected) => Some((*purpose, None)),
        };
        self.picker = None;
        answer
    }

    fn show(&mut self, ctx: &egui::Context) -> Action {
        let mut action = Action::None;
        if let Some((purpose, answer)) = self.poll() {
            match (purpose, answer) {
                (For::Open, Some(folder)) => {
                    if folder.join("package.toml").is_file() {
                        action = Action::Open(folder);
                    } else {
                        self.error = Some(format!("{}: {}", folder.display(), tr("not a package folder")));
                    }
                }
                (For::Parent, Some(folder)) => self.new_parent = Some(folder),
                _ => {}
            }
        }
        egui::CentralPanel::default().frame(Frame::NONE.fill(theme::BG)).show(ctx, |ui| {
            let width = (ui.available_width() - 2.0 * 16.0).min(760.0);
            ui.vertical_centered(|ui| {
                ui.set_max_width(width);
                ui.add_space(24.0);
                ui.label(RichText::new("Razdor Studio").strong().size(theme::HEADING + 4.0));
                ui.label(RichText::new(tr("The editor for Razdor 2.0 packages.")).color(theme::DIM));
                ui.add_space(20.0);
                let next = self.body(ui);
                if let Some(next) = next {
                    action = next;
                }
                ui.add_space(20.0);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button(format!("{} {}", ic::ARROW_LEFT, tr("Back to the menu"))).clicked() {
                        action = Action::Back;
                    }
                    ui.hyperlink_to(format!("{} {}", ic::BOOK_OPEN, tr("Studio guide")), GUIDE_URL);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("Razdor {}", env!("CARGO_PKG_VERSION"))).color(theme::FAINT).text_style(egui::TextStyle::Small));
                    });
                });
            });
        });
        action
    }

    /// The two columns: the recent packages on the left, the actions on the right; the new package's
    /// form under the actions when it is open. A recent package is opened by its click.
    fn body(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let mut action = None;
        ui.columns(2, |cols| {
            let left = &mut cols[0];
            left.label(RichText::new(tr("Recent packages").to_uppercase()).color(theme::FAINT).text_style(egui::TextStyle::Small).strong());
            left.add_space(4.0);
            if self.recent.is_empty() {
                left.label(RichText::new(tr("No package opened yet.")).color(theme::DIM));
            }
            for e in &self.recent {
                let text = format!("{}\n{} · {}: {}", e.name, e.path.display(), tr("Maps"), e.maps);
                let resp = left.add(Button::new(RichText::new(text).size(theme::TEXT_SIZE)).min_size(egui::vec2(left.available_width(), 46.0)));
                let resp = resp.on_hover_text(format!("{}\n{}", e.path.display(), when(e.opened)));
                if resp.clicked() {
                    action = Some(Action::Open(e.path.clone()));
                }
            }
            let right = &mut cols[1];
            right.label(RichText::new(tr("Actions").to_uppercase()).color(theme::FAINT).text_style(egui::TextStyle::Small).strong());
            right.add_space(4.0);
            let wide = egui::vec2(right.available_width(), theme::BTN_H + 8.0);
            if right.add(Button::new(format!("{} {}", ic::FILE_PLUS, tr("New package"))).min_size(wide)).clicked() {
                self.new_open = !self.new_open;
            }
            if right.add(Button::new(format!("{} {}", ic::FOLDER_OPEN, tr("Open package folder…"))).min_size(wide)).clicked() {
                self.start_pick(For::Open, tr("Open package folder…"));
            }
            let example = self.example.clone();
            let example_text = format!("{} {} «Пепельная Марка»", ic::STACK, tr("Open the example"));
            let resp = right.add_enabled(example.is_some(), Button::new(example_text).min_size(wide));
            let resp = if example.is_none() { resp.on_disabled_hover_text(tr("The example package is not in this folder.")) } else { resp };
            if resp.clicked() {
                action = example.map(Action::Open);
            }
            if self.new_open {
                right.add_space(8.0);
                right.separator();
                right.add_space(6.0);
                self.new_package_form(right, &mut action);
            }
            if let Some(e) = &self.error {
                right.add_space(8.0);
                right.colored_label(Color32::from_rgb(0xe0, 0x5a, 0x5a), e);
            }
        });
        action
    }

    /// The new package's form: its folder name, its parent folder, and «Создать».
    fn new_package_form(&mut self, ui: &mut egui::Ui, action: &mut Option<Action>) {
        ui.label(tr("Folder name"));
        ui.add(TextEdit::singleline(&mut self.new_name).desired_width(ui.available_width()));
        ui.add_space(4.0);
        ui.label(tr("Where"));
        let shown = self.new_parent.as_ref().map_or_else(|| "—".to_string(), |p| p.display().to_string());
        ui.label(RichText::new(shown).color(theme::DIM).text_style(egui::TextStyle::Small));
        if ui.button(tr("Choose folder…")).clicked() {
            self.start_pick(For::Parent, tr("Choose folder…"));
        }
        ui.add_space(6.0);
        let ready = !self.new_name.trim().is_empty() && self.new_parent.is_some();
        if ui.add_enabled(ready, Button::new(tr("Create package")).fill(theme::ACCENT)).clicked() {
            if let Some(parent) = self.new_parent.clone() {
                match newpkg::create(&parent, &self.new_name) {
                    Ok(dir) => *action = Some(Action::Open(dir)),
                    Err(e) => self.error = Some(e),
                }
            }
        }
    }

    /// Shows `e` under the page (a failed open).
    pub fn set_error(&mut self, e: String) {
        self.error = Some(e);
    }

    fn start_pick(&mut self, purpose: For, title: &str) {
        match pick_folder(title.to_string()) {
            Ok(rx) => self.picker = Some((purpose, rx)),
            Err(e) => self.error = Some(format!("{}: {e}", tr("The file picker cannot be opened."))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_is_written_as_its_utc_date() {
        assert_eq!(when(0), "1970-01-01 00:00");
        // 2026-11-04 17:50 UTC.
        assert_eq!(when(1_793_814_600), "2026-11-04 17:50");
    }

    #[test]
    fn the_recent_list_keeps_the_latest_first_once_and_at_most_eight() {
        let mut prefs = StudioPrefs::default();
        for i in 0..10 {
            remember(&mut prefs, Path::new(&format!("/pkgs/p{i}")));
        }
        assert_eq!(prefs.recent.len(), RECENT_MAX);
        assert_eq!(prefs.recent[0].path, "/pkgs/p9");
        remember(&mut prefs, Path::new("/pkgs/p3"));
        assert_eq!(prefs.recent[0].path, "/pkgs/p3");
        assert_eq!(prefs.recent.iter().filter(|r| r.path == "/pkgs/p3").count(), 1);
    }
}
