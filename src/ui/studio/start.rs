//! Razdor Studio's start page: what the studio shows when it opens with no package folder (the
//! main menu's «Редактор» with no package played before). A package folder to open, or a new
//! package made in a folder the player names. The pickers are the desktop's own folder dialogs
//! (`rfd`), run on a thread like the map import's (on macOS on the main thread).

use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, TryRecvError};

use egui_macroquad::egui;
use egui::{Button, Color32, RichText, TextEdit};
use razdor::i18n::tr;
use razdor::studio::newpkg;

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

/// The start page's state: the new package's name and folder, the picker's answer when it comes,
/// and the last error.
pub struct StartPage {
    picker: Option<(For, Receiver<Option<PathBuf>>)>,
    new_name: String,
    new_parent: Option<PathBuf>,
    error: Option<String>,
}

impl Default for StartPage {
    fn default() -> StartPage {
        StartPage { picker: None, new_name: "my-package".into(), new_parent: dirs::home_dir(), error: None }
    }
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

/// One frame of the start page.
pub fn frame(page: &mut StartPage) -> Action {
    macroquad::prelude::clear_background(macroquad::prelude::Color::new(0.125, 0.145, 0.19, 1.0));
    let mut action = Action::None;
    egui_macroquad::ui(|ctx| {
        super::theme(ctx);
        action = page.show(ctx);
    });
    egui_macroquad::draw();
    action
}

impl StartPage {
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
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(16.0);
            ui.heading("Razdor Studio");
            ui.weak(tr("Open a package folder, or make a new package."));
            ui.add_space(12.0);
            if ui.add(Button::new(tr("Open package folder…")).min_size(egui::vec2(220.0, 28.0))).clicked() {
                self.start_pick(For::Open, tr("Open package folder…"));
            }
            ui.add_space(20.0);
            ui.separator();
            ui.add_space(8.0);
            ui.strong(tr("New package"));
            ui.add_space(4.0);
            egui::Grid::new("studio-new-package").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                ui.label(tr("Folder name"));
                ui.add(TextEdit::singleline(&mut self.new_name).desired_width(240.0));
                ui.end_row();
                ui.label(tr("Where"));
                ui.horizontal(|ui| {
                    let shown = self.new_parent.as_ref().map_or_else(|| "—".to_string(), |p| p.display().to_string());
                    ui.label(RichText::new(shown).weak());
                    if ui.button(tr("Choose folder…")).clicked() {
                        self.start_pick(For::Parent, tr("Choose folder…"));
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            let ready = !self.new_name.trim().is_empty() && self.new_parent.is_some();
            if ui.add_enabled(ready, Button::new(tr("Create package")).min_size(egui::vec2(220.0, 28.0))).clicked() {
                if let Some(parent) = self.new_parent.clone() {
                    match newpkg::create(&parent, &self.new_name) {
                        Ok(dir) => action = Action::Open(dir),
                        Err(e) => self.error = Some(e),
                    }
                }
            }
            if let Some(e) = &self.error {
                ui.add_space(8.0);
                ui.colored_label(Color32::from_rgb(0xe0, 0x5a, 0x5a), e);
            }
            ui.add_space(24.0);
            if ui.button(tr("Back to the menu")).clicked() {
                action = Action::Back;
            }
        });
        action
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
