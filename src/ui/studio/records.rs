//! The Units and Items screens (P5): the package's records as lists with a filter and a new record
//! on a base, and each record's form in the Inspector: its fields with the base value each one
//! overrides (a reset per field), the suggested price from DT's own formula with its breakdown and
//! «Apply» (`studio::cost`, on the game's install), the level table, what the package format cannot
//! hold, the places that name it, rename and delete. Every change is one undo step: a field, a price
//! (`Workspace::apply_doc`), a rename across files (`set_texts`), a new record (a group).

use std::sync::Arc;

use egui::{Align2, CollapsingHeader, ComboBox, FontId, Grid, Id, Pos2, Rect, Sense, Stroke, TextEdit, Ui, Vec2};
use egui_macroquad::egui;
use egui_phosphor::regular as ic;
use razdor::dt::data::Stat;
use razdor::i18n::tr;
use razdor::pack::aliases;
use razdor::pack::load::Package;
use razdor::pack::refs::Ref;
use razdor::studio::cost::{ItemPrice, UnitPrice};
use razdor::studio::records::{self as rec, ItemFail, Kind, Priced, Refused};
use razdor::studio::sheet::{self, Cell, Column};
use razdor::trf;

use super::forms;
use super::inspector_kit as kit;
use super::scenarios::Focus;
use super::theme;
use super::{Jump, Viewer, ERROR_RED, WARNING_AMBER};

/// The label of an override field (the Russian names of the original's windows, records.md §5.1).
fn field_label(key: &str) -> &str {
    match key {
        "cost" => tr("Price, gold"),
        "hits" => tr("Hits"),
        "attack_blow" => tr("Attack, blows"),
        "attack_shot" => tr("Attack, shots"),
        "magic_power" => tr("Magic power"),
        "defence_blow" => tr("Defence, blows"),
        "defence_shot" => tr("Defence, shots"),
        "protect_life" => tr("Protection, life"),
        "protect_death" => tr("Protection, death"),
        "protect_elemental" => tr("Protection, elements"),
        "initiative" => tr("Initiative"),
        "manevres" => tr("Manoeuvres"),
        "regen" => tr("Regeneration"),
        "vampirism" => tr("Vampirism"),
        "evasion" => tr("Evasion"),
        _ => "",
    }
}

/// What a record's Inspector form changes this frame; applied after the form is drawn.
#[derive(Default)]
struct Asks {
    sets: Vec<(String, Option<i64>)>,
    rename: Option<String>,
    delete: bool,
    apply: bool,
    keep: bool,
    jump: Option<(String, u32)>,
}

/// The unit's price as the form shows it: the formula's steps, or why there is none.
enum UnitQuote {
    Price(UnitPrice),
    Unresolved(String),
}

/// The item's price as the form shows it.
enum ItemQuote {
    Price(ItemPrice),
    Skip(razdor::studio::cost::PriceSkip),
    Unresolved(String),
}

/// The sheet's row height and header height (points), and the frozen id column's width.
const ROW_H: f32 = 22.0;
const HEAD_H: f32 = 24.0;
const ID_W: f32 = 104.0;

/// A column's width on the sheet (points): sized so that the id, the stats and the suggested price fit a 1280 px
/// window beside the navigation and the Inspector (the rest scrolls).
fn col_w(c: Column) -> f32 {
    match c {
        Column::Id => ID_W,
        Column::Suggested => 136.0,
        Column::Base | Column::Type => 90.0,
        Column::Xp => 88.0,
        Column::Hits => 54.0,
        Column::Cost => 74.0,
        Column::Attack => 58.0,
        Column::Speed => 70.0,
        Column::Defence => 66.0,
    }
}

/// A column's header label (the catalogue's text).
fn column_label(c: Column) -> &'static str {
    match c {
        Column::Id => tr("Id"),
        Column::Base => tr("Base"),
        Column::Xp => tr("XP to level 2"),
        Column::Hits => tr("Hits"),
        Column::Attack => tr("Attack"),
        Column::Defence => tr("Defence"),
        Column::Speed => tr("Speed"),
        Column::Type => tr("Type"),
        Column::Cost => tr("Gold"),
        Column::Suggested => tr("Suggested"),
    }
}

/// What the sheet asks this frame, applied after its painter: a pick (the record and its cell), a sort by a
/// column, a cell to type in, a typed value to write, a paste, a suggested price to use.
#[derive(Default)]
struct SheetAsks {
    pick: Option<(String, (usize, usize))>,
    sort: Option<usize>,
    begin: Option<(String, usize, String)>,
    commit: Option<(String, usize, String)>,
    paste: Option<(String, usize, String)>,
    use_price: Option<String>,
}

/// A numeric cell being typed in: the record's id, the column, the text, and whether it takes the focus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SheetEdit {
    pub id: String,
    pub col: usize,
    pub text: String,
    pub focus: bool,
}

impl Viewer<'_> {
    /// The package's prices, built on the install for this package (rebuilt when the package is).
    /// `None` without the install.
    fn priced(&mut self, pkg: &Arc<Package>) -> Option<&mut Priced> {
        let install = self.install.clone()?;
        let fresh = matches!(&self.scen.priced, Some((p, _)) if Arc::ptr_eq(p, pkg));
        if !fresh {
            let lang = razdor::pack::open::lang();
            self.scen.priced = Some((pkg.clone(), Priced::new(&install, pkg.as_ref(), lang)));
        }
        self.scen.priced.as_mut().map(|(_, p)| p)
    }

    /// The selected record's id of `kind`.
    fn record_sel(&mut self, kind: Kind) -> &mut Option<String> {
        match kind {
            Kind::Unit => &mut self.scen.unit,
            Kind::Item => &mut self.scen.item,
        }
    }

    /// Selects a record: its form shows in the Inspector.
    fn record_pick(&mut self, kind: Kind, id: String) {
        *self.record_sel(kind) = Some(id);
        self.scen.focus = match kind {
            Kind::Unit => Focus::Units,
            Kind::Item => Focus::Items,
        };
        self.scen.error = None;
    }

    /// Shows an error under the form and in the log.
    fn record_fail(&mut self, e: String) {
        self.log.push(e.clone());
        self.scen.error = Some(e);
    }

    /// The Units screen.
    pub(super) fn units_screen(&mut self, ui: &mut Ui) {
        self.records_screen(ui, Kind::Unit);
    }

    /// The Items screen.
    pub(super) fn items_screen(&mut self, ui: &mut Ui) {
        self.records_screen(ui, Kind::Item);
    }

    /// The Inspector's form of the selected unit.
    pub(super) fn unit_inspector(&mut self, ui: &mut Ui) {
        self.record_inspector(ui, Kind::Unit);
    }

    /// The Inspector's form of the selected item.
    pub(super) fn item_inspector(&mut self, ui: &mut Ui) {
        self.record_inspector(ui, Kind::Item);
    }

    fn records_screen(&mut self, ui: &mut Ui, kind: Kind) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        ui.horizontal(|ui| {
            ui.label(tr("Filter"));
            ui.add(TextEdit::singleline(&mut self.scen.record_filter).desired_width(160.0));
        });
        // The new record: its id, its base (a base of the game) and the button.
        let base_list: Vec<&str> = aliases::names(kind.dt_kind()).collect();
        if self.scen.new_base.is_empty() {
            if let Some(first) = base_list.first() {
                self.scen.new_base = first.to_string();
            }
        }
        let mut create = false;
        ui.horizontal_wrapped(|ui| {
            ui.label(if kind == Kind::Unit { tr("New unit") } else { tr("New item") });
            ui.add(TextEdit::singleline(&mut self.scen.new_id).hint_text(tr("id")).desired_width(110.0));
            ComboBox::from_id_salt(("record-new-base", kind.key())).selected_text(self.scen.new_base.clone()).height(300.0).show_ui(ui, |ui| {
                for a in &base_list {
                    if ui.selectable_label(self.scen.new_base == *a, *a).clicked() {
                        self.scen.new_base = a.to_string();
                    }
                }
            });
            if ui.button(tr("Create")).on_hover_text(tr("Makes the record's file and its name text in every language, on the chosen base")).clicked() {
                create = true;
            }
        });
        if let Some(e) = self.scen.error.clone() {
            ui.colored_label(ERROR_RED, e);
        }
        if create {
            self.record_create(&pkg, kind);
        }
        ui.separator();
        self.sheet(ui, &pkg, kind);
    }

    /// The spreadsheet of `kind` (spec §4.3): one row per record, the columns of `sheet::columns`, sorted and filtered
    /// by the toolbar. The id column and the header stay in place while the rest scrolls. A click picks the record (its
    /// form shows in the Inspector and the cell is the cursor); a double click, Enter or F2 types into a numeric cell
    /// (Enter commits, Esc cancels, one undo step); «Use» writes the suggested price; Ctrl+C copies the cursor's cell
    /// and Ctrl+V pastes a number into it; the arrows move the cursor.
    fn sheet(&mut self, ui: &mut Ui, pkg: &Arc<Package>, kind: Kind) {
        let si = if kind == Kind::Unit { 0 } else { 1 };
        let cols: Vec<Column> = sheet::columns(kind).to_vec();
        let mut rows = {
            let priced = self.priced(pkg);
            sheet::rows(pkg, priced, kind)
        };
        let total = rows.len();
        let filter = self.scen.record_filter.clone();
        rows.retain(|r| sheet::matches(r, &filter));
        if let Some((col, desc)) = self.scen.sheet.sort[si] {
            sheet::sort(&mut rows, col, desc);
        }
        let selected_id = self.record_sel(kind).clone();
        ui.weak(trf!("{shown} of {total} records", shown = rows.len(), total = total));

        // Esc (the cancel command) drops a cell being typed in, before the cell is drawn.
        if std::mem::take(&mut self.scen.sheet.cancel) {
            self.scen.sheet.edit = None;
        }
        let typing = ui.ctx().wants_keyboard_input();
        let (down, up, left, right, enter) = if typing || self.scen.sheet.edit.is_some() {
            (false, false, false, false, false)
        } else {
            ui.input(|i| (i.key_pressed(egui::Key::ArrowDown), i.key_pressed(egui::Key::ArrowUp), i.key_pressed(egui::Key::ArrowLeft), i.key_pressed(egui::Key::ArrowRight), i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::F2)))
        };
        let mut asks = SheetAsks::default();
        if !rows.is_empty() {
            let (r, c) = self.scen.sheet.cursor;
            let (r, c) = (r.min(rows.len() - 1), c.min(cols.len() - 1));
            let r2 = if down { (r + 1).min(rows.len() - 1) } else if up { r.saturating_sub(1) } else { r };
            let c2 = if right { (c + 1).min(cols.len() - 1) } else if left { c.saturating_sub(1) } else { c };
            self.scen.sheet.cursor = (r2, c2);
            if (r2, c2) != (r, c) && r2 != r {
                asks.pick = Some((rows[r2].id.clone(), (r2, c2)));
            }
            if enter && sheet::field_of(cols[c2]).is_some() {
                let cell = &rows[r2].cells[c2];
                self.scen.sheet.edit = Some(SheetEdit { id: rows[r2].id.clone(), col: c2, text: cell.text(), focus: true });
            }
            if std::mem::take(&mut self.scen.sheet.copy) {
                ui.ctx().copy_text(rows[r2].cells[c2].text());
            }
            if let Some(text) = self.scen.sheet.paste.take() {
                asks.paste = Some((rows[r2].id.clone(), c2, text));
            }
        } else {
            self.scen.sheet.paste = None;
            self.scen.sheet.copy = false;
        }
        let cursor = self.scen.sheet.cursor;
        let sort = self.scen.sheet.sort[si];
        let mut edit = self.scen.sheet.edit.take();

        // The geometry: the id column is frozen, the others scroll; the header is the top row.
        let widths: Vec<f32> = cols.iter().map(|c| col_w(*c)).collect();
        let mut xs: Vec<f32> = Vec::with_capacity(widths.len());
        let mut acc = 0.0;
        for w in &widths {
            xs.push(acc);
            acc += w;
        }
        let content_w: f32 = widths.iter().sum();
        let content_h = HEAD_H + ROW_H * rows.len() as f32;
        let area = egui::ScrollArea::both().id_salt(("sheet", si)).auto_shrink([false, false]);
        area.show_viewport(ui, |ui, viewport| {
            let (rect, _) = ui.allocate_exact_size(Vec2::new(content_w.max(viewport.width()), content_h.max(viewport.height())), Sense::hover());
            let origin = rect.min;
            let at = |x: f32, y: f32| origin + Vec2::new(x, y);
            let painter = ui.painter().clone();
            let body_font = FontId::proportional(theme::TEXT_SIZE);
            // The body: each row's background and cells (the id column is drawn last, frozen).
            for (ri, row) in rows.iter().enumerate() {
                let y = HEAD_H + ri as f32 * ROW_H;
                let is_sel = selected_id.as_deref() == Some(row.id.as_str());
                let bg = if is_sel { theme::ACCENT.gamma_multiply(0.22) } else if ri % 2 == 1 { theme::PANEL } else { theme::BG };
                painter.rect_filled(Rect::from_min_size(at(0.0, y), Vec2::new(content_w, ROW_H)), 0.0, bg);
                for ci in 1..cols.len() {
                    let cell_rect = Rect::from_min_size(at(xs[ci], y), Vec2::new(widths[ci], ROW_H));
                    let editing = edit.as_ref().is_some_and(|e| e.id == row.id && e.col == ci);
                    if editing {
                        let e = edit.as_mut().expect("an edit");
                        let eid = Id::new(("sheet-edit", si, ri, ci));
                        let r = ui.put(cell_rect.shrink(1.0), TextEdit::singleline(&mut e.text).id(eid).desired_width(cell_rect.width() - 6.0));
                        if std::mem::take(&mut e.focus) {
                            r.request_focus();
                        }
                        if r.lost_focus() {
                            asks.commit = Some((row.id.clone(), ci, std::mem::take(&mut e.text)));
                        }
                        continue;
                    }
                    let cell = &row.cells[ci];
                    let numeric = matches!(cell, Cell::Num(_));
                    let colour = if row.overridden[ci] { theme::ACCENT } else { theme::TEXT };
                    let (align, pos) = if numeric { (Align2::RIGHT_CENTER, cell_rect.right_center() - Vec2::new(6.0, 0.0)) } else { (Align2::LEFT_CENTER, cell_rect.left_center() + Vec2::new(6.0, 0.0)) };
                    if cols[ci] == Column::Suggested {
                        // The suggested price's number, then the «Use» button at the right of the cell.
                        let btn = Rect::from_min_size(at(xs[ci] + widths[ci] - 88.0, y + 2.0), Vec2::new(84.0, ROW_H - 4.0));
                        painter.with_clip_rect(cell_rect).text(cell_rect.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, cell.text(), body_font.clone(), theme::TEXT);
                        // The button is there only when the formula gives a price.
                        if numeric && ui.put(btn, egui::Button::new(tr("Use")).small().wrap_mode(egui::TextWrapMode::Extend)).on_hover_text(tr("Write the suggested price into the price field (one undo step)")).clicked() {
                            asks.use_price = Some(row.id.clone());
                        }
                        continue;
                    }
                    // The text is clipped to its cell, so a long base does not run into the next column.
                    painter.with_clip_rect(cell_rect).text(pos, align, cell.text(), body_font.clone(), colour);
                    let resp = ui.interact(cell_rect, Id::new(("sheet-cell", si, ri, ci)), Sense::click());
                    if resp.double_clicked() && sheet::field_of(cols[ci]).is_some() {
                        asks.begin = Some((row.id.clone(), ci, cell.text()));
                    } else if resp.clicked() {
                        asks.pick = Some((row.id.clone(), (ri, ci)));
                    }
                }
            }
            // The cursor's frame.
            if rows.get(cursor.0).is_some() {
                let y = HEAD_H + cursor.0 as f32 * ROW_H;
                let rect = Rect::from_min_size(at(xs[cursor.1], y), Vec2::new(widths[cursor.1], ROW_H));
                painter.rect_stroke(rect, 0.0, Stroke::new(1.0, theme::ACCENT), egui::StrokeKind::Inside);
            }
            // The frozen column and the header: they stay at the viewport's left and top.
            let left = rect.min.x + viewport.min.x;
            let top = rect.min.y + viewport.min.y;
            for (ri, row) in rows.iter().enumerate() {
                let y = origin.y + HEAD_H + ri as f32 * ROW_H;
                let is_sel = selected_id.as_deref() == Some(row.id.as_str());
                let bg = if is_sel { theme::ACCENT.gamma_multiply(0.22) } else { theme::PANEL };
                let r = Rect::from_min_size(Pos2::new(left, y), Vec2::new(widths[0], ROW_H));
                painter.rect_filled(r, 0.0, bg);
                painter.with_clip_rect(r).text(r.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, row.id.as_str(), FontId::monospace(theme::MONO), theme::TEXT);
                let resp = ui.interact(r, Id::new(("sheet-id", si, ri)), Sense::click());
                if resp.clicked() {
                    asks.pick = Some((row.id.clone(), (ri, 0)));
                }
            }
            let head = Rect::from_min_size(Pos2::new(rect.min.x, top), Vec2::new(viewport.width().max(content_w), HEAD_H));
            painter.rect_filled(head, 0.0, theme::RAISED);
            for (ci, col) in cols.iter().enumerate() {
                let x = if ci == 0 { left } else { origin.x + xs[ci] };
                let r = Rect::from_min_size(Pos2::new(x, top), Vec2::new(widths[ci], HEAD_H));
                let mut label = column_label(*col).to_string();
                if let Some((sc, desc)) = sort {
                    if sc == ci {
                        label.push_str(if desc { ic::CARET_DOWN } else { ic::CARET_UP });
                    }
                }
                painter.with_clip_rect(r).text(r.left_center() + Vec2::new(6.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(theme::SMALL + 1.0), theme::DIM);
                let resp = ui.interact(r, Id::new(("sheet-head", si, ci)), Sense::click());
                if resp.clicked() {
                    asks.sort = Some(ci);
                }
            }
        });
        self.scen.sheet.edit = edit;
        self.apply_sheet_asks(pkg, kind, asks);
    }

    /// Applies what the sheet asked this frame: a pick, a sort, a cell's edit (one undo step), a paste, the price.
    fn apply_sheet_asks(&mut self, pkg: &Arc<Package>, kind: Kind, asks: SheetAsks) {
        let si = if kind == Kind::Unit { 0 } else { 1 };
        if let Some(ci) = asks.sort {
            let cur = self.scen.sheet.sort[si];
            self.scen.sheet.sort[si] = match cur {
                Some((c, false)) if c == ci => Some((ci, true)),
                Some((c, true)) if c == ci => None,
                _ => Some((ci, false)),
            };
        }
        if let Some((id, (r, c))) = asks.pick {
            self.scen.sheet.cursor = (r, c);
            if self.record_sel(kind).as_deref() != Some(id.as_str()) {
                self.record_pick(kind, id);
            }
        }
        if let Some((id, col, text)) = asks.begin {
            self.scen.sheet.edit = Some(SheetEdit { id, col, text, focus: true });
        }
        if let Some((id, col, text)) = asks.commit {
            self.scen.sheet.edit = None;
            self.sheet_set(pkg, kind, &id, col, &text);
        }
        if let Some((id, col, text)) = asks.paste {
            let first = text.lines().next().unwrap_or("").to_string();
            self.sheet_set(pkg, kind, &id, col, &first);
        }
        if let Some(id) = asks.use_price {
            self.record_pick(kind, id.clone());
            self.record_apply_price(pkg, kind, &id);
        }
    }

    /// Writes a typed number into a numeric cell of the sheet: refused when the text is not a number or the field's
    /// range does not hold it, and a no-op when it is the value the cell has.
    fn sheet_set(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str, col: usize, text: &str) {
        let cols = sheet::columns(kind);
        let Some(key) = cols.get(col).and_then(|c| sheet::field_of(*c)) else { return };
        let Ok(value) = text.trim().parse::<i64>() else {
            return self.record_fail(trf!("{text} is not a number", text = text.trim()));
        };
        let (lo, hi) = rec::field_range(key);
        if value < lo || value > hi {
            return self.record_fail(trf!("{key} must be between {lo} and {hi}", key = key, lo = lo, hi = hi));
        }
        if rec::override_of(pkg, kind, id, key) == Some(value) {
            return;
        }
        self.record_set(pkg, kind, id, key, Some(value));
    }

    /// Makes the new record of the form's id and base: its file and its name text in every language,
    /// as one undo step.
    fn record_create(&mut self, pkg: &Arc<Package>, kind: Kind) {
        let id = self.scen.new_id.trim().to_string();
        let alias = self.scen.new_base.clone();
        let name = self.base_name(pkg, kind, &alias);
        let n = match rec::new_record(pkg, kind, &id, &alias, &name) {
            Ok(n) => n,
            Err(e) => return self.record_fail(e),
        };
        let label = format!("add {} {id}", kind.key());
        self.ws.begin_group();
        let mut result = self.ws.create_text_file(&n.file, &n.text, &label);
        for (file, edit) in n.texts {
            if result.is_ok() {
                result = self.ws.apply_doc(&file, &label, &[edit]);
            }
        }
        match result {
            Ok(()) => {
                self.ws.end_group(&label);
                self.scen.new_id.clear();
                self.scen.error = None;
                self.changed = true;
                self.record_pick(kind, id);
            }
            Err(e) => {
                self.ws.cancel_group();
                self.record_fail(e);
            }
        }
    }

    /// The name of the base record, as the game names it (the alias when the install is not there).
    fn base_name(&mut self, pkg: &Arc<Package>, kind: Kind, alias: &str) -> String {
        let r = match kind {
            Kind::Unit => Ref::parse(&format!("dt:{alias}")),
            Kind::Item => Ref::parse(&format!("dt:item/{alias}")),
        };
        let name = match (self.priced(pkg), r) {
            (Some(p), Ok(r)) => match kind {
                Kind::Unit => p.base_unit(&r).map(|d| d.name),
                Kind::Item => p.base_item(&r).map(|a| a.name),
            },
            _ => None,
        };
        name.filter(|n| !n.is_empty()).unwrap_or_else(|| alias.to_string())
    }

    /// Sets a record's field `key` to `value` (`None`: the override is removed), one undo step.
    fn record_set(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str, key: &str, value: Option<i64>) {
        let label = match value {
            Some(_) => format!("set {key} of {id}"),
            None => format!("reset {key} of {id}"),
        };
        match rec::set_edit(pkg, kind, id, key, value) {
            Ok((file, edit)) => match self.ws.apply_doc(&file, &label, &[edit]) {
                Ok(()) => {
                    self.scen.error = None;
                    self.changed = true;
                }
                Err(e) => self.record_fail(e),
            },
            Err(e) => self.record_fail(e),
        }
    }

    /// Writes the suggested price into the record's cost field, one undo step. A unit whose formula
    /// gives no price (a tactical cost below 1, or gold over 65,535) and an item the formula leaves
    /// alone are refused with the reason.
    fn record_apply_price(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str) {
        let price: Result<i64, String> = match self.priced(pkg) {
            None => Err(tr("The price needs the game's install: open the studio with the game installed.").to_string()),
            Some(p) => match kind {
                Kind::Unit => match p.unit_price(id) {
                    Err(e) => Err(e),
                    Ok(q) if q.tactical < 1 => Err(tr("The tactical cost is below 1: the formula gives no price for this unit.").to_string()),
                    Ok(q) => q.price.ok_or_else(|| tr("The gold is over 65,535: the formula gives no price for this unit.").to_string()),
                },
                Kind::Item => match p.item_price(id) {
                    Ok(q) => Ok(q.banded),
                    Err(ItemFail::Skip(_)) => Err(tr("The formula leaves this artefact unpriced: see the note above the price.").to_string()),
                    Err(ItemFail::Unresolved(e)) => Err(e),
                },
            },
        };
        match price {
            Ok(v) => self.record_set(pkg, kind, id, "cost", Some(v)),
            Err(e) => self.record_fail(e),
        }
    }

    /// Deletes the record, refused (with its first use) while the package names it.
    fn record_delete(&mut self, pkg: &Arc<Package>, kind: Kind, id: &str) {
        match rec::delete_edit(pkg, kind, id) {
            Ok((file, edit)) => match self.ws.apply_doc(&file, &format!("delete {} {id}", kind.key()), &[edit]) {
                Ok(()) => {
                    *self.record_sel(kind) = None;
                    self.scen.error = None;
                    self.changed = true;
                }
                Err(e) => self.record_fail(e),
            },
            Err(Refused::NotFound) => self.record_fail(tr("No such record in the package.").to_string()),
            Err(Refused::Used(uses)) => {
                let first = format!("{}:{}, {}", uses[0].file, uses[0].line, uses[0].what);
                let count = uses.len();
                self.record_fail(trf!("Cannot delete: named by {count} places, the first at {first}", count = count, first = first));
            }
        }
    }

    /// Renames the record in every file that names it, one undo step.
    fn record_rename(&mut self, pkg: &Arc<Package>, kind: Kind, old: &str, new: &str) {
        let plan = {
            let ws = &*self.ws;
            rec::rename_plan(pkg, kind, old, new, &|f: &str| ws.text_of(f).ok())
        };
        match plan {
            Ok(files) => match self.ws.set_texts(&format!("rename {old} to {new}"), files) {
                Ok(()) => {
                    self.record_pick(kind, new.to_string());
                    self.scen.record_rename.clear();
                    self.changed = true;
                }
                Err(e) => self.record_fail(e),
            },
            Err(e) => self.record_fail(e),
        }
    }

    /// The Inspector's form of a record of `kind`.
    fn record_inspector(&mut self, ui: &mut Ui, kind: Kind) {
        let Some(pkg) = self.pkg.clone() else {
            ui.colored_label(ERROR_RED, tr("The package could not be read: see Problems."));
            return;
        };
        let Some(id) = self.record_sel(kind).clone() else {
            ui.weak(if kind == Kind::Unit { tr("Nothing selected: pick a unit in the Units tab.") } else { tr("Nothing selected: pick an item in the Items tab.") });
            return;
        };
        let Some(row) = rec::rows(&pkg, kind).into_iter().find(|r| r.id == id) else {
            *self.record_sel(kind) = None;
            ui.weak(tr("This record is gone: its file no longer names it."));
            return;
        };
        let fields: &[&str] = if kind == Kind::Unit { &rec::UNIT_FIELDS } else { &rec::ITEM_FIELDS };
        let price_open = self.scen.price_open;
        let has_install = self.install.is_some();

        // The values the form shows, read from the prices (the game's content) before the form is drawn.
        let (bases, unit_quote, item_quote, levels) = match self.priced(&pkg) {
            None => (vec![None; fields.len()], None, None, Vec::new()),
            Some(p) => {
                let bases: Vec<Option<i64>> = fields.iter().map(|k| rec::base_value(&pkg, p, kind, &id, k)).collect();
                match kind {
                    Kind::Unit => {
                        let quote = match p.unit_price(&id) {
                            Ok(q) => UnitQuote::Price(q),
                            Err(e) => UnitQuote::Unresolved(e),
                        };
                        let levels: Vec<[i64; 4]> = (1..=5)
                            .map(|l| {
                                let pct = p.level_percent(&id).map_or(0, |t| t[(l - 1) as usize]);
                                match p.level_stats(&id, l) {
                                    Some(s) => [s[Stat::Hits] as i64, s[Stat::AttackBlow] as i64, s[Stat::DefenceBlow] as i64, pct],
                                    None => [0, 0, 0, pct],
                                }
                            })
                            .collect();
                        (bases, Some(quote), None, levels)
                    }
                    Kind::Item => {
                        let quote = match p.item_price(&id) {
                            Ok(q) => ItemQuote::Price(q),
                            Err(ItemFail::Skip(s)) => ItemQuote::Skip(s),
                            Err(ItemFail::Unresolved(e)) => ItemQuote::Unresolved(e),
                        };
                        (bases, None, Some(quote), Vec::new())
                    }
                }
            }
        };

        let mut asks = Asks::default();
        let (icon, kind_word) = if kind == Kind::Unit { (ic::SHIELD_STAR, tr("Unit type")) } else { (ic::SCROLL, tr("Item")) };
        let q = super::inspector::head(ui, icon, &format!("{kind_word}: {id}"), &row.base);
        let kind_key = kind.key();

        // The price: the formula's number, «Apply» and «Keep» (the suggestion goes away for this record), and
        // its steps in a collapsible. The card is hidden when the author has kept the price.
        let price_kept = self.scen.price_kept.as_deref() == Some(id.as_str());
        kit::section(ui, kind_key, "price", tr("Price"), true, &q, |ui| {
            if price_kept {
                kit::note(ui, tr("The suggested price is kept as it is (the price field holds it)."));
                return;
            }
            kit::card(ui, |ui| {
                ui.strong(tr("Suggested price"));
                if !has_install {
                    ui.colored_label(WARNING_AMBER, tr("The price needs the game's install: open the studio with the game installed."));
                }
                let priced = match (&unit_quote, &item_quote) {
                    (Some(UnitQuote::Price(p)), _) if p.tactical >= 1 && p.price.is_some() => Some(p.price.unwrap_or(0)),
                    (_, Some(ItemQuote::Price(p))) => Some(p.banded),
                    _ => None,
                };
                match (&unit_quote, &item_quote) {
                    (Some(UnitQuote::Price(p)), _) => {
                        if p.tactical < 1 || p.price.is_none() {
                            ui.colored_label(WARNING_AMBER, tr("The formula gives no price here (see the steps)."));
                        }
                    }
                    (Some(UnitQuote::Unresolved(e)), _) => {
                        ui.colored_label(ERROR_RED, trf!("Cannot price: {e}", e = e));
                    }
                    (_, Some(ItemQuote::Skip(razdor::studio::cost::PriceSkip::NotPriced))) => {
                        ui.weak(tr("Not priced: the price is 1, or the artefact is a potion or an item (the button leaves it alone)."));
                    }
                    (_, Some(ItemQuote::Skip(razdor::studio::cost::PriceSkip::NoGain))) => {
                        ui.colored_label(WARNING_AMBER, tr("No unit type gains from it: the original's price there is not known. Set it by hand."));
                    }
                    (_, Some(ItemQuote::Unresolved(e))) => {
                        ui.colored_label(ERROR_RED, trf!("Cannot price: {e}", e = e));
                    }
                    _ => {}
                }
                if let Some(gold) = priced {
                    ui.label(trf!("{gold} gold, by the game's formula", gold = gold));
                    ui.horizontal(|ui| {
                        if ui.button(tr("Apply the price")).on_hover_text(tr("Write this price into the price field (one undo step)")).clicked() {
                            asks.apply = true;
                        }
                        if ui.button(tr("Keep")).on_hover_text(tr("Leave the price field as it is")).clicked() {
                            asks.keep = true;
                        }
                    });
                }
                CollapsingHeader::new(tr("How the price is made")).id_salt(("studio-inspector", kind_key, "price-steps")).default_open(price_open).show(ui, |ui| {
                    if let Some(UnitQuote::Price(p)) = &unit_quote {
                        ui.label(trf!("Strength S, the battle core's evaluator on the base stats: {s}", s = p.strength));
                        ui.label(trf!("Tactical cost T: S times {mult} / 100 gives {t}", mult = p.multiplier, t = p.tactical));
                        if p.tactical >= 1 {
                            match p.gold_raw {
                                Some(g) => ui.label(trf!("Gold: (T / 2.2 + 21) / {div} gives {g}", div = p.gold_div, g = g)),
                                None => ui.colored_label(ERROR_RED, tr("Gold over 65,535: a range error in the original, no price.")),
                            };
                            if let Some(b) = p.banded {
                                ui.label(trf!("Rounded down in bands: {b}", b = b));
                            }
                            if p.hero {
                                ui.label(trf!("Hero class, doubled: {price}", price = p.price.unwrap_or(0)));
                            } else {
                                ui.weak(tr("Not a hero class: no doubling."));
                            }
                        }
                    }
                    if let Some(ItemQuote::Price(p)) = &item_quote {
                        ui.label(trf!("Gains summed over the units that gain: {sum} (wearers {n}, gainers {m})", sum = p.sum, n = p.wearers, m = p.gainers));
                        ui.label(trf!("Mean of the gains, plus 10, at least 20: {mean} plus 10 gives {raw}", mean = p.mean, raw = p.raw));
                        ui.label(trf!("Rounded down in bands: {b}", b = p.banded));
                    }
                    if item_quote.is_none() && unit_quote.is_none() {
                        ui.weak(tr("No price steps: the game's install is needed."));
                    }
                });
            });
        });

        // The fields: the override (or the base's value), the base next to it, the changed dot and the reset.
        kit::section(ui, kind_key, "fields", tr("Fields"), true, &q, |ui| {
            kit::note(ui, tr("A field with no override takes the base's value. Reset removes the override."));
            for (i, key) in fields.iter().enumerate() {
                let ov = rec::override_of(&pkg, kind, &id, key);
                let base = bases[i];
                let (lo, hi) = rec::field_range(key);
                let model = ov.or(base).unwrap_or(0);
                let shown = kit::row(ui, &q, field_label(key), &format!("{kind_key}.{key}"), Some(ov.is_some()), |ui| {
                    ui.horizontal(|ui| {
                        let v = forms::int_field(ui, &format!("{id}-{key}"), model, lo, hi);
                        match base {
                            Some(b) => ui.weak(format!("{} {b}", tr("base"))),
                            None => ui.weak(tr("base: no install")),
                        };
                        v
                    })
                    .inner
                });
                if let Some(Some(v)) = shown.value {
                    asks.sets.push((key.to_string(), Some(v)));
                }
                if shown.reset {
                    asks.sets.push((key.to_string(), None));
                }
            }
            kit::note(ui, if kind == Kind::Unit {
                tr("Not in the package format, so the base's values hold: per-level gains, nature, magic school and direction, bonus, icon, upgrade units, surrender, starting experience, level multiplier, cost multiplier and gold divisor.")
            } else {
                tr("Not in the package format, so the base's values hold: the stat changes, type, magic school and bonus. The package holds the price.")
            });
        });

        if kind == Kind::Unit && !levels.is_empty() {
            kit::section(ui, kind_key, "levels", tr("Levels 1 to 5"), false, &q, |ui| {
                Grid::new("record-levels").num_columns(5).spacing([10.0, 4.0]).striped(true).show(ui, |ui| {
                    ui.strong(tr("Level"));
                    ui.strong(tr("Hits"));
                    ui.strong(tr("Attack, blows"));
                    ui.strong(tr("Defence, blows"));
                    ui.strong(tr("Tactical cost, % of base"));
                    ui.end_row();
                    for (k, row) in levels.iter().enumerate() {
                        ui.label(format!("{}", k + 1));
                        ui.label(format!("{}", row[0]));
                        ui.label(format!("{}", row[1]));
                        ui.label(format!("{}", row[2]));
                        ui.label(format!("{}%", row[3]));
                        ui.end_row();
                    }
                });
                kit::note(ui, tr("The base's per-level gains: the package has none of its own."));
            });
        }

        kit::section(ui, kind_key, "record", tr("Record"), true, &q, |ui| {
            kit::row(ui, &q, tr("Base"), &format!("{kind_key}.base"), None, |ui| {
                ui.monospace(row.base.as_str());
                ui.weak(format!("{}:{}", row.file, row.line));
            });
            kit::row(ui, &q, tr("Name"), &format!("{kind_key}.name"), None, |ui| {
                ui.weak(egui::RichText::new(row.name.as_str()).monospace());
            });
            if let Some(text) = pkg.texts.get(razdor::pack::open::lang()).and_then(|t| t.get(&row.name)).or_else(|| pkg.texts.values().find_map(|t| t.get(&row.name))) {
                kit::row(ui, &q, tr("Text"), &format!("{kind_key}.text"), None, |ui| ui.label(text.as_str()));
            }
            let rename = kit::row(ui, &q, tr("Rename to"), &format!("{kind_key}.id"), None, |ui| {
                ui.add(TextEdit::singleline(&mut self.scen.record_rename).desired_width(120.0));
                ui.button(tr("Rename")).on_hover_text(tr("Renames the id in every file that names it, and its name keys (one undo step)")).clicked()
            });
            if rename.value == Some(true) {
                let new = self.scen.record_rename.trim().to_string();
                if !new.is_empty() {
                    asks.rename = Some(new);
                }
            }
        });

        kit::section(ui, kind_key, "uses", tr("Used by"), true, &q, |ui| {
            let refs = rec::uses(&pkg, kind, &id);
            if refs.is_empty() {
                kit::note(ui, tr("Nothing names this record."));
            } else {
                kit::note(ui, tr("Click a use to open it in Script."));
                for u in &refs {
                    ui.horizontal_wrapped(|ui| {
                        if ui.link(u.what.as_str()).clicked() {
                            asks.jump = Some((u.file.clone(), u.line));
                        }
                        ui.weak(format!("{}:{}", u.file, u.line));
                    });
                }
            }
        });
        ui.add_space(6.0);
        if ui.button(tr("Delete")).on_hover_text(tr("Removes the record from its file; refused while the package names it")).clicked() {
            asks.delete = true;
        }

        // Apply what the form asked, in order: a delete ends the record's form.
        if asks.delete {
            self.record_delete(&pkg, kind, &id);
            return;
        }
        if asks.keep {
            self.scen.price_kept = Some(id.clone());
        }
        for (key, value) in asks.sets {
            self.record_set(&pkg, kind, &id, &key, value);
        }
        if asks.apply {
            self.record_apply_price(&pkg, kind, &id);
        }
        if let Some(new) = asks.rename {
            self.record_rename(&pkg, kind, &id, &new);
        }
        if let Some((file, line)) = asks.jump {
            self.open = Some(file);
            *self.jump = Some(Jump { line, range: None });
        }
    }
}
