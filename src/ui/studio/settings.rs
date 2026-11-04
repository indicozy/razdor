//! Razdor Studio's Settings window (UX1, spec §9.2 and §12): the interface scale (80 to 150 %), and the
//! shortcut page «Горячие клавиши»: every command with its binding, a click to capture the next key
//! (Esc cancels), a refused binding with the command it clashes with, «Сбросить» for one command and
//! for all. The bindings and the scale are kept in the user's settings (`StudioPrefs`), not the package.

use egui_macroquad::egui::{self, Align, Align2, Button, Color32, Layout, RichText, Ui};
use razdor::i18n::{n_, tr};

use super::commands::{self, Binding, Capture, Refused, COMMANDS};
use super::theme;
use crate::ui::audio::StudioPrefs;

/// The window's pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Interface,
    Keys,
}

/// The Settings window's state.
pub struct Window {
    /// The window is shown.
    pub open: bool,
    page: Page,
    /// The command whose next key is being captured.
    capturing: Option<&'static str>,
    /// The last refused rebinding: the command and why.
    refused: Option<(&'static str, Refused)>,
    /// «Сбросить раскладку» was pressed: the studio resets the shown workspace's layout (taken by the frame).
    pub reset_layout: bool,
}

impl Default for Window {
    fn default() -> Window {
        Window { open: false, page: Page::Interface, capturing: None, refused: None, reset_layout: false }
    }
}

impl Window {
    /// Whether a key is being captured: the shortcuts then wait.
    pub fn capturing(&self) -> bool {
        self.capturing.is_some()
    }

    /// The window on the shortcut page, with a refused rebinding shown (the snapshot's `keys-conflict`):
    /// Undo is asked to take Ctrl+S, which Save has, and the refusal is the one a player would see.
    pub fn show_conflict(&mut self, prefs: &StudioPrefs) {
        let mut keys = prefs.keys.clone();
        let refused = commands::rebind(&mut keys, "undo", Binding::parse("Ctrl+S"));
        self.refused = refused.err().map(|r| ("undo", r));
        self.open = true;
        self.page = Page::Keys;
    }

    /// The window on the shortcut page (the snapshot's `keys`).
    pub fn show_keys(&mut self) {
        self.open = true;
        self.page = Page::Keys;
    }

    /// Draws the window when it is open. `scale` is the scale the studio shows now; `mq` are the
    /// function keys read from macroquad this frame (for the capture).
    pub fn show(&mut self, ctx: &egui::Context, prefs: &mut StudioPrefs, scale: u32, mq: &[Binding]) {
        if !self.open {
            self.capturing = None;
            return;
        }
        let mut open = self.open;
        egui::Window::new(tr("Settings"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
            .default_width(640.0)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    for (page, label) in [(Page::Interface, n_("Interface")), (Page::Keys, n_("Keyboard shortcuts"))] {
                        if ui.selectable_label(self.page == page, tr(label)).clicked() {
                            self.page = page;
                            self.capturing = None;
                        }
                    }
                });
                ui.separator();
                match self.page {
                    Page::Interface => interface(ui, prefs, scale, &mut self.reset_layout),
                    Page::Keys => self.keys(ui, ctx, prefs, mq),
                }
            });
        self.open = open;
        if !open {
            self.capturing = None;
        }
    }

    /// The shortcut page: the capture, the table, the clash message and the resets.
    fn keys(&mut self, ui: &mut Ui, ctx: &egui::Context, prefs: &mut StudioPrefs, mq: &[Binding]) {
        if let Some(id) = self.capturing {
            match commands::capture(ctx, mq) {
                Some(Capture::Cancel) => self.capturing = None,
                Some(Capture::Key(b)) => {
                    self.capturing = None;
                    self.apply(prefs, id, Some(b));
                }
                None => {}
            }
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr("Click a shortcut, then press the new key. Esc cancels.")).text_style(egui::TextStyle::Small).color(theme::DIM));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(!prefs.keys.is_empty(), Button::new(tr("Reset all"))).clicked() {
                    commands::reset(&mut prefs.keys, None);
                    self.refused = None;
                }
            });
        });
        ui.add_space(4.0);
        let mut reset_one = None;
        let mut start = None;
        egui::ScrollArea::vertical().max_height(420.0).auto_shrink([false, true]).show(ui, |ui| {
            egui::Grid::new("studio-keys").num_columns(4).spacing([12.0, 6.0]).striped(true).show(ui, |ui| {
                for c in COMMANDS {
                    ui.label(RichText::new(c.icon).size(theme::ICON));
                    ui.vertical(|ui| {
                        ui.set_min_width(230.0);
                        ui.label(tr(c.name));
                        if !c.later.is_empty() {
                            ui.label(RichText::new(tr(c.later)).text_style(egui::TextStyle::Small).color(theme::FAINT));
                        }
                    });
                    let text = match commands::binding_of(c, &prefs.keys) {
                        Some(b) => b.text(),
                        None => tr("Not set").to_string(),
                    };
                    if c.fixed {
                        ui.label(RichText::new(text).color(theme::DIM)).on_hover_text(tr("Copy, cut and paste keys are fixed"));
                    } else if self.capturing == Some(c.id) {
                        ui.add_enabled(false, Button::new(tr("Press a key…")));
                    } else if ui.add(Button::new(text).min_size(egui::vec2(150.0, theme::BTN_H))).clicked() {
                        start = Some(c.id);
                    }
                    let changed = prefs.keys.contains_key(c.id);
                    if ui.add_enabled(changed && !c.fixed, Button::new(tr("Reset"))).clicked() {
                        reset_one = Some(c.id);
                    }
                    ui.end_row();
                    if let Some((id, why)) = self.refused.as_ref().filter(|(id, _)| *id == c.id) {
                        let msg = match why {
                            Refused::Fixed => tr("This key is fixed").to_string(),
                            Refused::Clash(other) => {
                                let name = commands::command(other).map_or(*other, |o| o.name);
                                format!("{}: {}", tr("Taken by"), tr(name))
                            }
                        };
                        let _ = id;
                        ui.label("");
                        ui.colored_label(Color32::from_rgb(0xe0, 0x6c, 0x6c), msg);
                        ui.end_row();
                    }
                }
            });
        });
        if let Some(id) = start {
            self.capturing = Some(id);
            self.refused = None;
        }
        if let Some(id) = reset_one {
            commands::reset(&mut prefs.keys, Some(id));
            self.refused = None;
        }
    }

    /// Sets a command's binding: refused (and kept on the page) when it clashes.
    fn apply(&mut self, prefs: &mut StudioPrefs, id: &'static str, b: Option<Binding>) {
        self.refused = match commands::rebind(&mut prefs.keys, id, b) {
            Ok(()) => None,
            Err(why) => Some((id, why)),
        };
    }
}

/// The interface page: the scale, in 10 % steps from 80 to 150.
fn interface(ui: &mut Ui, prefs: &mut StudioPrefs, scale: u32, reset_layout: &mut bool) {
    ui.horizontal(|ui| {
        ui.label(tr("Interface scale"));
        egui::ComboBox::from_id_salt("studio-scale").selected_text(format!("{scale} %")).show_ui(ui, |ui| {
            for pct in theme::SCALES {
                if ui.selectable_label(pct == scale, format!("{pct} %")).clicked() {
                    prefs.scale = pct;
                }
            }
        });
    });
    ui.add_space(10.0);
    ui.checkbox(&mut prefs.field_keys, tr("File keys in labels")).on_hover_text(tr("Shows each Inspector field's key in the file under its label (otherwise it is in the tooltip)"));
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(tr("Workspace layout"));
        if ui.button(tr("Reset layout")).clicked() {
            *reset_layout = true;
        }
    });
    ui.add_space(6.0);
    ui.label(RichText::new(tr("The scale changes the whole studio. No text is drawn under 12 px.")).text_style(egui::TextStyle::Small).color(theme::DIM));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_conflict_demo_is_the_refusal_a_player_gets() {
        let mut w = Window::default();
        w.show_conflict(&StudioPrefs::default());
        assert!(w.open);
        assert_eq!(w.page, Page::Keys);
        assert_eq!(w.refused, Some(("undo", Refused::Clash("save"))));
    }

    #[test]
    fn a_refused_binding_is_not_stored() {
        let mut w = Window::default();
        let mut prefs = StudioPrefs::default();
        w.apply(&mut prefs, "undo", Binding::parse("Ctrl+S"));
        assert!(prefs.keys.is_empty());
        assert!(w.refused.is_some());
        w.apply(&mut prefs, "undo", Binding::parse("Ctrl+Alt+Z"));
        assert!(w.refused.is_none(), "a free key clears the message");
        assert_eq!(prefs.keys.get("undo").map(String::as_str), Some("Ctrl+Alt+Z"));
    }
}
