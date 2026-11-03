//! The Inspector dock: the selected object's fields as the map file has them, in file order, from
//! the map file's text (the open document's when it is open, else the file on disk). Read-only;
//! «Show in script» takes the Script to the object's table.

use std::fs;

use egui::Ui;
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::studio::map::{inspect, Selection};

use super::{jump_for, Jump, Viewer, ERROR_RED};

impl Viewer<'_> {
    pub(super) fn inspector(&mut self, ui: &mut Ui) {
        let Some(sel) = self.selection.clone() else {
            ui.weak(tr("Nothing selected: pick an object on the map or in the scene."));
            return;
        };
        let Some(id) = self.maps.map.clone() else {
            ui.weak(tr("No map to show."));
            return;
        };
        let (kind, ident) = match &sel {
            Selection::Hero => (tr("Hero"), String::new()),
            Selection::Building(i) => (tr("Building"), i.clone()),
            Selection::Army(i) => (tr("Army"), i.clone()),
            Selection::Point(i) => (tr("Point"), (i + 1).to_string()),
            Selection::Object(i) => (tr("Object"), (i + 1).to_string()),
        };
        ui.strong(if ident.is_empty() { kind.to_string() } else { format!("{kind}: {ident}") });
        let path = format!("maps/{id}/map.toml");
        let text = self.ws.docs.iter().find(|d| d.path == path).map(|d| d.text.clone()).or_else(|| fs::read_to_string(self.ws.root.join(&path)).ok());
        let Some(text) = text else {
            ui.colored_label(ERROR_RED, tr("The map file could not be read."));
            return;
        };
        match inspect(&text, &sel) {
            Err(e) => {
                ui.colored_label(ERROR_RED, e);
            }
            Ok(found) => {
                egui::Grid::new("studio-inspector").num_columns(2).spacing([14.0, 4.0]).striped(true).show(ui, |ui| {
                    for (key, value) in &found.fields {
                        ui.weak(key.as_str());
                        ui.add(egui::Label::new(value.as_str()).wrap());
                        ui.end_row();
                    }
                });
                ui.add_space(6.0);
                if ui.button(tr("Show in script")).clicked() {
                    *self.jump = Some(jump_for(&*self.ws, self.pkg.as_deref(), &path, &sel).unwrap_or(Jump { line: found.line, range: None }));
                    self.open = Some(path.clone());
                }
            }
        }
        ui.add_space(6.0);
        ui.weak(tr("Read-only for now: the canvas moves objects by dragging them."));
    }
}
