//! The Inspector dock: the selected table's form (`forms`), the list of what names it («Used by»)
//! and Delete. The form reads the table from the map file's text (the open document's, else the disk's),
//! so it shows what is typed; a text that does not parse shows the last good package's values.
//! Every change the form makes is one undo step.

use std::collections::BTreeMap;
use std::fs;

use egui::Ui;
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::pack::aliases;
use razdor::pack::load::Package;
use razdor::pack::map::MapFile;
use razdor::pack::refs::DtKind;
use razdor::pack::uses::{self, Target, Use};
use razdor::studio::map::{locator_of, Selection};
use razdor::studio::Workspace;
use razdor::trf;

use super::forms::{self, Ctx};
use super::scenarios::Focus;
use super::{Jump, Viewer, ERROR_RED};

/// The map file's spec as the text says now, else the last good package's.
pub fn spec_now(ws: &Workspace, pkg: Option<&Package>, map: &str) -> Option<MapFile> {
    let file = format!("maps/{map}/map.toml");
    let text = ws.text_of(&file).ok().or_else(|| fs::read_to_string(ws.root.join(&file)).ok());
    if let Some(parsed) = text.and_then(|t| toml::from_str::<MapFile>(&t).ok()) {
        return Some(parsed);
    }
    pkg?.maps.get(map)?.spec.clone()
}

/// Whether the table of `sel` is in the map's spec.
fn present(spec: &MapFile, sel: &Selection) -> bool {
    match sel {
        Selection::Hero => true,
        Selection::Building(id) => spec.building.iter().any(|b| &b.get_ref().id == id),
        Selection::Army(id) => spec.army.iter().any(|a| &a.get_ref().id == id),
        Selection::Point(i) => *i < spec.point.len(),
        Selection::Object(i) => *i < spec.object.len(),
    }
}

/// The unit choices of a form: the package's units by id, then the `dt:` aliases of the base game.
fn unit_choices(pkg: Option<&Package>) -> Vec<String> {
    let mut out: Vec<String> = pkg.map(|p| p.units.iter().map(|u| u.value.id.clone()).collect()).unwrap_or_default();
    out.extend(aliases::names(DtKind::Unit).map(|a| format!("dt:{a}")));
    out
}

/// The texts of the language the studio shows (any language when that one is missing).
fn texts_of<'p>(pkg: Option<&'p Package>, lang: &str) -> Option<&'p BTreeMap<String, String>> {
    let p = pkg?;
    p.texts.get(lang).or_else(|| p.texts.values().next())
}

/// Deletes the table of `sel` on map `map` (one undo step). The references it has are counted and
/// logged first: the delete goes on anyway. Returns whether the table was deleted.
pub fn delete_selected(ws: &mut Workspace, log: &mut Vec<String>, pkg: Option<&Package>, map: &str, sel: &Selection) -> bool {
    if matches!(sel, Selection::Hero) {
        log.push(tr("The hero cannot be removed: every map has one").to_string());
        return false;
    }
    let file = format!("maps/{map}/map.toml");
    let Some(spec) = spec_now(ws, pkg, map) else { return false };
    let Some(at) = locator_of(&file, &spec, sel) else { return false };
    let name = at.name();
    let refs = match sel {
        Selection::Army(id) => pkg.map(|p| uses::uses(p, map, Target::Army(id))).unwrap_or_default(),
        Selection::Building(id) => pkg.map(|p| uses::uses(p, map, Target::Building(id))).unwrap_or_default(),
        Selection::Point(i) => spec
            .point
            .get(*i)
            .and_then(|p| p.get_ref().id.as_deref())
            .and_then(|id| pkg.map(|p| uses::uses(p, map, Target::Point(id))))
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if !refs.is_empty() {
        log.push(trf!("{name} is named by {count} places (deleted anyway): {first}", name, count = refs.len(), first = describe(&refs[0])));
    }
    match ws.remove(at) {
        Ok(()) => {
            log.push(trf!("Deleted {name}", name));
            true
        }
        Err(e) => {
            log.push(e);
            false
        }
    }
}

/// `file:line, what` for a log line.
fn describe(u: &Use) -> String {
    format!("{}:{}, {}", u.file, u.line, u.what)
}

impl Viewer<'_> {
    pub(super) fn inspector(&mut self, ui: &mut Ui) {
        // The scenario forms take the Inspector while they are in focus (`scenarios`).
        match self.scen.focus.clone() {
            Focus::Package => return egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.package_form(ui)).inner,
            Focus::Header => return egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.header_form(ui)).inner,
            Focus::Campaign => return egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.campaign_inspector(ui)).inner,
            Focus::Table => {}
        }
        let Some(sel) = self.selection.clone() else {
            ui.weak(tr("Nothing selected: pick an object on the map or in the scene."));
            return;
        };
        let Some(map) = self.maps.map.clone() else {
            ui.weak(tr("No map to show."));
            return;
        };
        let file = format!("maps/{map}/map.toml");
        let Some(spec) = spec_now(self.ws, self.pkg.as_deref(), &map) else {
            ui.colored_label(ERROR_RED, tr("The map file could not be read."));
            return;
        };
        // A selection whose table is gone (an undo took it back) is dropped.
        if !present(&spec, &sel) {
            *self.selection = None;
            ui.weak(tr("Nothing selected: pick an object on the map or in the scene."));
            return;
        }
        let Some(at) = locator_of(&file, &spec, &sel) else {
            ui.colored_label(ERROR_RED, tr("This object has no table in the file"));
            return;
        };
        let title = match &sel {
            Selection::Hero => tr("Hero").to_string(),
            Selection::Building(id) => format!("{}: {id}", tr("Building")),
            Selection::Army(id) => format!("{}: {id}", tr("Army")),
            Selection::Point(i) => format!("{}: {}", tr("Point"), i + 1),
            Selection::Object(i) => format!("{}: {}", tr("Object"), i + 1),
        };
        ui.strong(title);
        ui.weak(tr("Each change is one undo step (Ctrl+Z)."));
        ui.separator();

        let size = self.maps.current().map_or((200, 200), |g| (g.compiled.scenario.width(), g.compiled.scenario.height()));
        let units = unit_choices(self.pkg.as_deref());
        let empty = BTreeMap::new();
        let texts = texts_of(self.pkg.as_deref(), razdor::pack::open::lang()).unwrap_or(&empty);
        let cx = Ctx { map: &map, size, texts, units: &units };
        let edit = match &sel {
            Selection::Hero => forms::hero(ui, &cx, &at, &spec.hero),
            Selection::Building(id) => spec.building.iter().map(|b| b.get_ref()).find(|b| &b.id == id).and_then(|b| forms::building(ui, &cx, &at, b)),
            Selection::Army(id) => spec.army.iter().map(|a| a.get_ref()).find(|a| &a.id == id).and_then(|a| forms::army(ui, &cx, &at, a)),
            Selection::Point(i) => spec.point.get(*i).and_then(|p| forms::point(ui, &cx, &at, p.get_ref())),
            Selection::Object(i) => spec.object.get(*i).and_then(|o| forms::object(ui, &cx, &at, o.get_ref())),
        };
        if let Some(edit) = edit {
            match self.ws.apply(edit) {
                Ok(()) => self.changed = true,
                Err(e) => self.log.push(e),
            }
        }

        ui.add_space(8.0);
        ui.separator();
        self.used_by(ui, &map, &sel, &spec);
        ui.add_space(6.0);
        let deletable = !matches!(sel, Selection::Hero);
        let clicked = ui.add_enabled(deletable, egui::Button::new(tr("Delete"))).on_hover_text(tr("Delete key: removes the table; the comment above it stays")).clicked();
        if clicked && delete_selected(self.ws, self.log, self.pkg.as_deref(), &map, &sel) {
            *self.selection = None;
            self.changed = true;
        }
    }

    /// The references to the selected army, building or lantern: each one clickable, opening its
    /// file at its line in the Script.
    fn used_by(&mut self, ui: &mut Ui, map: &str, sel: &Selection, spec: &MapFile) {
        let target = match sel {
            Selection::Army(id) => Some(Target::Army(id)),
            Selection::Building(id) => Some(Target::Building(id)),
            Selection::Point(i) => spec.point.get(*i).and_then(|p| p.get_ref().id.as_deref()).map(Target::Point),
            Selection::Hero | Selection::Object(_) => None,
        };
        ui.strong(tr("Used by"));
        let Some(target) = target else {
            ui.weak(tr("Nothing names this table."));
            return;
        };
        let Some(pkg) = self.pkg.clone() else {
            ui.weak(tr("The package could not be read: see Output."));
            return;
        };
        let refs = uses::uses(&pkg, map, target);
        if refs.is_empty() {
            ui.weak(tr("Nothing names this table."));
            return;
        }
        ui.weak(tr("Click a use to open it in Script."));
        for u in &refs {
            ui.horizontal_wrapped(|ui| {
                if ui.link(u.what.as_str()).clicked() {
                    self.open = Some(u.file.clone());
                    *self.jump = Some(Jump { line: u.line, range: None });
                }
                ui.weak(format!("{}:{}", u.file, u.line));
            });
        }
    }
}
