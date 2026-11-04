//! The Данные workspace (UX5, spec §4.2 and §4.3): the navigation at the left (Юниты | Предметы | Тексты | Пакет |
//! Файлы; on Файлы the package's file tree), the view in the centre, and «Открыть в файле» (Ctrl+Shift+O, and the
//! link at the top of the Inspector) which opens an object's line in the Файлы view. The spreadsheets are in
//! `records.rs`; the texts table in `scenarios.rs`; the package form in `scenarios.rs`; the code view in `mod.rs`.

use egui::{ScrollArea, Ui};
use egui_macroquad::egui;
use egui_phosphor::regular as ic;
use razdor::i18n::tr;
use razdor::studio::map::Selection;
use razdor::studio::nav::{open_target, Object};
use razdor::studio::texts as tx;

use super::records::SheetEdit;
use super::scenarios::Focus;
use super::story::StorySel;
use super::{Jump, Tab, Viewer, StudioScreen};
use super::workspaces::WsKind;

/// The Данные views (the navigation's tabs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DataView {
    #[default]
    Units,
    Items,
    Texts,
    Package,
    Files,
}

impl DataView {
    pub const ALL: [DataView; 5] = [DataView::Units, DataView::Items, DataView::Texts, DataView::Package, DataView::Files];
}

/// The spreadsheets' state: per kind (units, items) the sort column and whether it is descending; the cursor's
/// row and column; the cell being typed in; and the keys the frame's shortcuts asked for (Esc, Ctrl+C, Ctrl+V).
#[derive(Default)]
pub struct SheetUi {
    pub sort: [Option<(usize, bool)>; 2],
    pub cursor: (usize, usize),
    pub edit: Option<SheetEdit>,
    pub cancel: bool,
    pub copy: bool,
    pub paste: Option<String>,
}

/// The object the current selection names, for «Открыть в файле»: a trigger or dialog of the Story, a unit or an
/// item of the Данные views, or the shown map's object (the Map's selection). `None` when nothing is selected.
pub fn object_now(focus: &Focus, story: &StorySel, unit: &Option<String>, item: &Option<String>, selection: &Option<Selection>) -> Option<Object> {
    match focus {
        Focus::Story => match story {
            StorySel::Trigger(id) => Some(Object::Trigger(id.clone())),
            StorySel::Dialog(id) => Some(Object::Dialog(id.clone())),
            StorySel::Var(_) | StorySel::None => None,
        },
        Focus::Units => unit.clone().map(Object::Unit),
        Focus::Items => item.clone().map(Object::Item),
        Focus::Table => selection.clone().map(Object::Map),
        _ => None,
    }
}

impl Viewer<'_> {
    /// The navigation column: the five views with their counts, and on Файлы the package's file tree.
    pub(super) fn data_nav(&mut self, ui: &mut Ui) {
        let Some(pkg) = self.pkg.clone() else {
            ui.weak(tr("The package could not be read: see Problems."));
            return;
        };
        let texts = tx::table_of(&pkg).rows.len();
        for view in DataView::ALL {
            let (icon, label, count) = match view {
                DataView::Units => (ic::SHIELD_STAR, tr("Units"), pkg.units.len()),
                DataView::Items => (ic::SCROLL, tr("Items"), pkg.items.len()),
                DataView::Texts => (ic::TRANSLATE, tr("Texts"), texts),
                DataView::Package => (ic::PACKAGE, tr("Package"), 0),
                DataView::Files => (ic::FILE_TEXT, tr("Files"), self.ws.entries.len()),
            };
            let text = if count > 0 { format!("{icon}  {label} · {count}") } else { format!("{icon}  {label}") };
            if ui.selectable_label(self.scen.data_view == view, text).clicked() {
                self.scen.data_view = view;
            }
        }
        if self.scen.data_view == DataView::Files {
            ui.separator();
            self.file_system(ui);
        }
    }

    /// The Данные centre: the view the navigation picked.
    pub(super) fn data_centre(&mut self, ui: &mut Ui) {
        match self.scen.data_view {
            DataView::Units => self.units_screen(ui),
            DataView::Items => self.items_screen(ui),
            DataView::Texts => self.texts_screen(ui),
            DataView::Package => {
                ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| self.package_form(ui));
            }
            DataView::Files => self.script(ui),
        }
    }

    /// The «Открыть в файле» link at the top of the Inspector, for the selection's object (when the package names it).
    pub(super) fn open_link(&mut self, ui: &mut Ui) {
        let Some(obj) = object_now(&self.scen.focus, &self.scen.story.sel, &self.scen.unit, &self.scen.item, self.selection) else { return };
        let Some(pkg) = self.pkg.clone() else { return };
        let map = self.maps.map.clone().unwrap_or_default();
        let target = {
            let ws = &*self.ws;
            open_target(&pkg, &|f: &str| ws.text_of(f).ok(), &map, &obj)
        };
        let Some((file, line)) = target else { return };
        ui.horizontal(|ui| {
            if ui.link(tr("Open in file")).on_hover_text(tr("Open this object's line in Данные › Файлы (Ctrl+Shift+O)")).clicked() {
                self.go_to(file, line);
            }
            ui.weak(format!("{}:{}", short(&obj), line));
        });
    }

    /// Opens `file` at `line` in the Файлы view (through the Script's jump, shown when the view is drawn).
    pub(super) fn go_to(&mut self, file: String, line: u32) {
        self.open = Some(file);
        *self.jump = Some(Jump { line, range: None });
        self.scen.want_tab = Some(Tab::Script);
    }
}

/// The name of an object for the link's caption (the line is shown beside it).
fn short(obj: &Object) -> String {
    match obj {
        Object::Map(_) => "map.toml".to_string(),
        Object::Trigger(id) | Object::Dialog(id) | Object::Unit(id) | Object::Item(id) => id.clone(),
    }
}

impl StudioScreen {
    /// Ctrl+Shift+O: opens the selection's object in its file, at its line, in Данные › Файлы.
    pub(super) fn open_in_file(&mut self) {
        let Some(pkg) = self.ws.package() else { return };
        let Some(obj) = object_now(&self.scen.focus, &self.scen.story.sel, &self.scen.unit, &self.scen.item, &self.selection) else {
            self.log.push(tr("Nothing to open in a file: select an object first.").to_string());
            return;
        };
        let map = self.maps.map.clone().unwrap_or_default();
        let target = {
            let ws = &self.ws;
            open_target(&pkg, &|f: &str| ws.text_of(f).ok(), &map, &obj)
        };
        let Some((file, line)) = target else {
            self.log.push(tr("The package does not name this object in a file.").to_string());
            return;
        };
        self.open_file(&file);
        self.jump = Some(Jump { line, range: None });
        self.ws_kind = WsKind::Data;
        self.scen.data_view = DataView::Files;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::studio::records::{self as rec, Kind};
    use razdor::studio::Workspace;

    /// A temporary copy of the example package, opened as a workspace (the copy is removed with the test).
    fn example() -> (razdor::studio::gen::apply::TempCopy, Workspace) {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let copy = razdor::studio::gen::apply::copy_package(&src).expect("the example package copies");
        let ws = Workspace::open(copy.path()).expect("the copy opens");
        (copy, ws)
    }

    /// The value of `key` in the table of unit `id` in `file`, read from the file's text (the workspace's document,
    /// which an edit changes at once; `read_package` reads the saved package).
    fn unit_field(ws: &Workspace, file: &str, id: &str, key: &str) -> Option<i64> {
        let doc: toml_edit::DocumentMut = ws.text_of(file).expect("the unit file's text").parse().expect("parses");
        let unit = doc["unit"].as_array_of_tables()?.iter().find(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))?;
        unit.get(key).and_then(|v| v.as_integer())
    }


    #[test]
    fn the_selection_names_its_object_by_the_view_that_has_focus() {
        let sel = Selection::Hero;
        assert_eq!(object_now(&Focus::Story, &StorySel::Trigger("go".into()), &None, &None, &None), Some(Object::Trigger("go".into())));
        assert_eq!(object_now(&Focus::Units, &StorySel::None, &Some("ash_knight".into()), &None, &None), Some(Object::Unit("ash_knight".into())));
        assert_eq!(object_now(&Focus::Table, &StorySel::None, &None, &None, &Some(sel.clone())), Some(Object::Map(sel)));
        assert_eq!(object_now(&Focus::Table, &StorySel::None, &None, &None, &None), None, "nothing selected names nothing");
    }

    #[test]
    fn a_numeric_cell_edit_is_one_undo_step_and_undo_restores_the_bytes() {
        // The sheet writes a cell through `records::set_edit` and `Workspace::apply_doc`, as the form does.
        let (_copy, mut ws) = example();
        let (pkg, _) = ws.read_package().expect("the package reads");
        let id = "ash_knight";
        let before_value = rec::override_of(&pkg, Kind::Unit, id, "hits");
        let (file, edit) = rec::set_edit(&pkg, Kind::Unit, id, "hits", Some(77)).expect("the cell's edit");
        let before = ws.text_of(&file).expect("the unit file's text");
        ws.apply_doc(&file, "set hits", &[edit]).expect("the cell applies");
        assert_eq!(unit_field(&ws, &file, id, "hits"), Some(77), "the cell holds the typed value (was {before_value:?})");
        assert!(ws.undo().expect("undo").is_some(), "the cell's edit is an undo step");
        assert_eq!(ws.text_of(&file).expect("the text after undo"), before, "undo restores the bytes");
    }

    #[test]
    fn use_suggested_price_writes_the_formula_s_price_as_one_undo_step() {
        // The «Use» button writes the price the formula gives into the cost field (`record_apply_price`). The
        // price itself comes from the game's install; here the write of that price is checked, with the price the
        // formula gives for this record (read from the install when `RAZDOR_DT_DIR` is set).
        let (_copy, mut ws) = example();
        let (pkg, _) = ws.read_package().expect("the package reads");
        let id = "ash_knight";
        let price = match std::env::var("RAZDOR_DT_DIR").ok().and_then(|d| razdor::dt::install::DtInstall::load(std::path::Path::new(&d)).ok()) {
            Some(install) => {
                let mut priced = rec::Priced::new(&install, &pkg, razdor::pack::open::lang());
                priced.unit_price(id).expect("the unit prices").price.expect("the formula gives a price")
            }
            None => 150,
        };
        let (file, edit) = rec::set_edit(&pkg, Kind::Unit, id, "cost", Some(price)).expect("the price edit");
        let before = ws.text_of(&file).expect("the file");
        ws.apply_doc(&file, "use suggested price", &[edit]).expect("the price applies");
        assert_eq!(unit_field(&ws, &file, id, "cost"), Some(price as i64), "the price field holds the formula's price");
        assert!(ws.undo().expect("undo").is_some());
        assert_eq!(ws.text_of(&file).expect("after undo"), before);
    }

    #[test]
    fn the_view_names_its_five_places_in_order() {
        assert_eq!(DataView::ALL.len(), 5);
        assert_eq!(DataView::ALL[0], DataView::Units);
        assert_eq!(DataView::ALL[4], DataView::Files);
    }
}
