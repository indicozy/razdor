//! The Scene dock: the shown map's objects by kind, and the triggers bound to the map. Clicking a
//! hero, a building, an army, a point or an object selects it, centres the Map tab on it and,
//! when the Script shows the map's file, takes the Script to its table.

use egui::{CollapsingHeader, ScrollArea, Ui};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::studio::map::{centre_of, Selection};
use razdor::trf;

use super::Viewer;

impl Viewer<'_> {
    pub(super) fn scene(&mut self, ui: &mut Ui) {
        let (Some(pkg), Some(id)) = (self.pkg.clone(), self.maps.map.clone()) else {
            ui.weak(tr("No map to show."));
            return;
        };
        let Some(spec) = pkg.maps.get(&id).and_then(|m| m.spec.as_ref()) else {
            ui.weak(tr("This map does not parse: see Problems."));
            return;
        };
        let current = self.selection.clone();
        let mut pick: Option<Selection> = None;
        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.strong(id.as_str());
            let mut row = |ui: &mut Ui, text: String, sel: Selection| {
                if ui.selectable_label(current.as_ref() == Some(&sel), text).clicked() {
                    pick = Some(sel);
                }
            };
            let [hx, hy] = spec.hero.cell;
            CollapsingHeader::new(tr("Hero")).default_open(true).show(ui, |ui| row(ui, format!("({hx}, {hy})"), Selection::Hero));
            CollapsingHeader::new(trf!("Buildings ({count})", count = spec.building.len())).default_open(true).show(ui, |ui| {
                for b in spec.building.iter().map(|b| b.get_ref()) {
                    row(ui, format!("{} · {}", b.id, b.kind), Selection::Building(b.id.clone()));
                }
            });
            CollapsingHeader::new(trf!("Armies ({count})", count = spec.army.len())).default_open(true).show(ui, |ui| {
                for a in spec.army.iter().map(|a| a.get_ref()) {
                    row(ui, format!("{} · {}", a.id, a.model), Selection::Army(a.id.clone()));
                }
            });
            CollapsingHeader::new(trf!("Points ({count})", count = spec.point.len())).default_open(true).show(ui, |ui| {
                for (i, p) in spec.point.iter().map(|p| p.get_ref()).enumerate() {
                    let text = p.id.clone().unwrap_or_else(|| format!("{} {}", tr("Point"), i + 1));
                    row(ui, text, Selection::Point(i));
                }
            });
            // Objects can be hundreds: collapsed by default.
            CollapsingHeader::new(trf!("Objects ({count})", count = spec.object.len())).default_open(false).show(ui, |ui| {
                for (i, o) in spec.object.iter().map(|o| o.get_ref()).enumerate() {
                    row(ui, format!("{} ({}, {})", o.kind, o.cell[0], o.cell[1]), Selection::Object(i));
                }
            });
            let here: Vec<&str> = pkg.triggers.iter().filter(|t| t.value.map.as_deref() == Some(id.as_str())).map(|t| t.value.id.as_str()).collect();
            CollapsingHeader::new(trf!("Triggers here ({count})", count = here.len())).default_open(true).show(ui, |ui| {
                for t in &here {
                    ui.weak(*t);
                }
            });
        });
        if let Some(sel) = pick {
            self.canvas.centre = centre_of(spec, &sel);
            self.select(sel);
            self.focus_map = true;
        }
    }
}
