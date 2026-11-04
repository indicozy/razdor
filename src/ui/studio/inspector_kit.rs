//! The Inspector widget set (UX2, spec §6): one set for every form of the studio. A header (icon,
//! name, type line), a field search that filters the rows by their label and file key (Ctrl+F
//! focuses it while the Inspector has the pointer), collapsible sections whose open state is kept per
//! object kind, rows with a fixed label column, a changed-field dot and a reset button (where the form
//! has a base value), list rows that drag to reorder, and the icon buttons of a list.
//!
//! The kit draws only; the forms make the edits. A row's value closure runs only when the row shows,
//! so a filtered-out field makes no edit. The pure parts (the filter, a drop's position, a move) are
//! unit-tested at the foot of the file.

use egui::{Align, CornerRadius, Frame, Id, Layout, Margin, Rect, RichText, Sense, Stroke, TextEdit, Ui, Vec2};
use egui_macroquad::egui;
use egui_phosphor::regular as ic;
use razdor::i18n::tr;
use razdor::trf;

use super::theme::{self, ACCENT, BTN_H, DIM, FAINT, HOVER, LABEL_W, LINE, RADIUS, RAISED, SMALL, TEXT, WELL};

/// The width a collapsing header keeps for its triangle (a filtered section keeps it too, so both align).
const TRIANGLE_W: f32 = 18.0;

/// The width of the reset slot at the end of a row (a 24 px icon button).
const RESET_W: f32 = 24.0;
/// The changed-field dot's slot, at the start of the label column.
const DOT_W: f32 = 8.0;
/// The gap between the label column and the value.
const GAP: f32 = 4.0;

/// Whether a row is shown for the field search `query` (empty: every row). A row matches when its
/// label or its file key holds the query, ignoring case.
pub fn matches(query: &str, label: &str, key: &str) -> bool {
    let q = query.trim().to_lowercase();
    q.is_empty() || label.to_lowercase().contains(&q) || key.to_lowercase().contains(&q)
}

/// The drop slot of a pointer at `y` among rows whose centres are `centres` (top to bottom): the
/// number of rows above the pointer, so the drop line goes above row `slot` (`centres.len()` is
/// below the last row).
pub fn drop_index(centres: &[f32], y: f32) -> usize {
    centres.iter().filter(|c| **c < y).count()
}

/// The index a row from `from` lands on when it is dropped in slot `slot` (a slot is the line above
/// a row). `None` when the drop puts the row where it is.
pub fn move_target(from: usize, slot: usize) -> Option<usize> {
    if slot == from || slot == from + 1 {
        return None;
    }
    Some(if slot > from { slot - 1 } else { slot })
}

/// The list with the entry `from` moved to index `to` (the entries between shift by one), as
/// `DocEdit::Move` and the array edits of the forms do it.
pub fn moved<T: Clone>(items: &[T], from: usize, to: usize) -> Vec<T> {
    let mut out = items.to_vec();
    if from < out.len() {
        let item = out.remove(from);
        out.insert(to.min(out.len()), item);
    }
    out
}

/// The per-frame flags the studio sets for the kit: the inline file keys (the setting, or a snapshot's
/// option), the Inspector's focus (the pointer was in it last frame) and a Ctrl+F request.
const INLINE_ID: &str = "studio-inspector-inline-keys";
const HOT_ID: &str = "studio-inspector-hot";
const FIND_ID: &str = "studio-inspector-find";
const SEARCH_ID: &str = "studio-inspector-search";

/// Sets whether the file keys show under the labels (the setting «Ключи файла в подписях»).
pub fn set_inline_keys(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(Id::new(INLINE_ID), on));
}

fn inline_keys(ui: &Ui) -> bool {
    ui.data(|d| d.get_temp::<bool>(Id::new(INLINE_ID))).unwrap_or(false)
}

/// Records whether the Inspector has the focus this frame (read by the next frame's shortcuts).
pub fn set_hot(ctx: &egui::Context, on: bool) {
    ctx.data_mut(|d| d.insert_temp(Id::new(HOT_ID), on));
}

/// Whether the Inspector had the focus (the pointer in it) in the last frame.
pub fn hot(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(Id::new(HOT_ID))).unwrap_or(false)
}

/// Presets the field search's text (the snapshot option `inspector-find=<text>`).
pub fn set_query(ctx: &egui::Context, query: &str) {
    ctx.data_mut(|d| d.insert_temp(Id::new(SEARCH_ID), query.to_string()));
}

/// Forgets a focus request of the last frame (called at the start of each frame).
pub fn clear_find(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<bool>(Id::new(FIND_ID)));
}

/// Asks for the search box to take the focus this frame (Ctrl+F).
pub fn request_find(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(Id::new(FIND_ID), true));
}

/// Takes the focus request (true once).
fn take_find(ctx: &egui::Context) -> bool {
    let asked = ctx.data(|d| d.get_temp::<bool>(Id::new(FIND_ID))).unwrap_or(false);
    if asked {
        ctx.data_mut(|d| d.remove::<bool>(Id::new(FIND_ID)));
    }
    asked
}

/// The header of a form: the icon in a well, the name, and the type line under it. Its response is
/// returned for a tooltip.
pub fn header(ui: &mut Ui, icon: &str, name: &str, line: &str) -> egui::Response {
    let resp = Frame::NONE.fill(RAISED).corner_radius(CornerRadius::same(RADIUS)).inner_margin(Margin::same(8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            Frame::NONE.fill(WELL).corner_radius(CornerRadius::same(6)).inner_margin(Margin::same(6)).show(ui, |ui| {
                ui.label(RichText::new(icon).size(20.0).color(TEXT));
            });
            ui.vertical(|ui| {
                ui.label(RichText::new(name).strong().size(theme::HEADING));
                ui.label(RichText::new(line).size(SMALL).color(DIM));
            });
        });
    });
    ui.add_space(4.0);
    resp.response
}

/// The field search box: it filters the rows of the form (`matches`). Returns the query, trimmed.
pub fn search(ui: &mut Ui) -> String {
    let id = Id::new(SEARCH_ID);
    let mut query: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.label(RichText::new(ic::MAGNIFYING_GLASS).color(DIM));
        let field = ui.add(TextEdit::singleline(&mut query).id(id).hint_text(tr("Search fields")).desired_width(ui.available_width() - 32.0));
        if take_find(ui.ctx()) {
            field.request_focus();
        }
        if ui.add_enabled(!query.is_empty(), icon_button(ic::X, 22.0)).on_hover_text(tr("Clear the search")).clicked() {
            query.clear();
        }
    });
    ui.data_mut(|d| d.insert_temp(id, query.clone()));
    query.trim().to_string()
}

/// The temp key of a section's count of shown rows for the current filter.
fn count_id(key: &str) -> Id {
    Id::new(("studio-inspector-count", key))
}

/// The section being drawn (its key), so that a row can count itself for the filter.
const SECTION_ID: &str = "studio-inspector-section";

/// A collapsible section. Its open state is kept per `kind` (the object's kind) and `id` (the section),
/// not per item, so a change to the number of items does not close it. With a filter, the section
/// opens by itself and shows only its matching rows; a section with none is left out until the filter
/// changes.
pub fn section(ui: &mut Ui, kind: &str, id: &str, title: &str, default_open: bool, query: &str, body: impl FnOnce(&mut Ui)) {
    if query.is_empty() {
        egui::CollapsingHeader::new(RichText::new(title).strong()).id_salt(("studio-inspector", kind, id)).default_open(default_open).show(ui, |ui| {
            ui.add_space(2.0);
            body(ui);
            ui.add_space(4.0);
        });
        return;
    }
    let key = format!("{kind}/{id}");
    let counted = count_id(&key);
    // The last count for this filter: a section with no match is left out.
    let last: Option<(String, usize)> = ui.data(|d| d.get_temp(counted));
    if matches!(&last, Some((q, 0)) if q == query) {
        return;
    }
    ui.data_mut(|d| d.insert_temp(counted, (query.to_string(), 0_usize)));
    ui.data_mut(|d| d.insert_temp(Id::new(SECTION_ID), key.clone()));
    // The title and the rows sit where a collapsing header's would (its triangle's width is kept free).
    ui.horizontal(|ui| {
        ui.add_space(TRIANGLE_W);
        ui.label(RichText::new(title).strong());
    });
    ui.add_space(2.0);
    ui.indent(("studio-inspector-filtered", key.as_str()), body);
    ui.add_space(4.0);
    ui.data_mut(|d| d.remove::<String>(Id::new(SECTION_ID)));
    let now: Option<(String, usize)> = ui.data(|d| d.get_temp(counted));
    if now != last {
        // The count changed: the section is shown or left out next frame.
        ui.ctx().request_repaint();
    }
}

/// Counts one shown row of the current section for the filter.
fn count_shown(ui: &Ui, query: &str) {
    let Some(key) = ui.data(|d| d.get_temp::<String>(Id::new(SECTION_ID))) else { return };
    let counted = count_id(&key);
    let n = ui.data(|d| d.get_temp::<(String, usize)>(counted)).map_or(0, |(_, n)| n);
    ui.data_mut(|d| d.insert_temp(counted, (query.to_string(), n + 1)));
}

/// What a row did: its value (`None` when the filter hides the row) and whether its reset was pressed.
pub struct Outcome<R> {
    pub value: Option<R>,
    pub reset: bool,
}

/// One field: the changed dot, the label (fixed width, its file key in the tooltip, or under it with
/// the inline setting), the value, and the reset button. `changed` is `None` for a field with no base
/// value (no dot, an empty reset slot), else whether it differs from the base (the reset is enabled).
pub fn row<R>(ui: &mut Ui, query: &str, label: &str, key: &str, changed: Option<bool>, value: impl FnOnce(&mut Ui) -> R) -> Outcome<R> {
    if !matches(query, label, key) {
        return Outcome { value: None, reset: false };
    }
    if !query.is_empty() {
        count_shown(ui, query);
    }
    let inline = inline_keys(ui);
    let mut out = Outcome { value: None, reset: false };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = GAP;
        ui.set_min_height(BTN_H);
        label_column(ui, label, key, changed, inline);
        let value_w = (ui.available_width() - RESET_W - GAP).max(40.0);
        // A column of its own: a scope would keep the row's horizontal layout.
        out.value = Some(
            ui.allocate_ui_with_layout(Vec2::new(value_w, 0.0), Layout::top_down(Align::LEFT), |ui| {
                ui.set_width(value_w);
                value(ui)
            })
            .inner,
        );
        match changed {
            Some(on) => {
                out.reset = ui.add_enabled(on, icon_button(ic::ARROW_COUNTER_CLOCKWISE, RESET_W)).on_hover_text(tr("Reset to the base value")).clicked();
            }
            None => {
                ui.allocate_exact_size(Vec2::splat(RESET_W), Sense::hover());
            }
        }
    });
    out
}

/// A block with no label column (a list): drawn when its label or key matches the filter.
pub fn block<R>(ui: &mut Ui, query: &str, label: &str, key: &str, body: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    if !matches(query, label, key) {
        return None;
    }
    if !query.is_empty() {
        count_shown(ui, query);
    }
    Some(body(ui))
}

/// The label column of a row: the changed dot, the label (wrapped in its width), and under it the file key
/// with the inline setting. Painted, so the label sits on the row's centre line whatever its length.
fn label_column(ui: &mut Ui, label: &str, key: &str, changed: Option<bool>, inline: bool) {
    // The column grows with the label text where the text is raised to the 12 px floor (80 %, theme::floor),
    // so a label keeps its words whole at every scale.
    let text_scale = theme::floor(theme::TEXT_SIZE, ui.ctx().zoom_factor()) / theme::TEXT_SIZE;
    let width = LABEL_W * text_scale - GAP;
    let text_w = width - DOT_W - GAP;
    let on = changed == Some(true);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let label_colour = if on { TEXT } else { DIM };
    let label_galley = ui.painter().layout(label.to_string(), font, label_colour, text_w);
    let key_galley = inline.then(|| ui.painter().layout(key.to_string(), egui::FontId::monospace(theme::MONO), FAINT, text_w));
    let key_h = key_galley.as_ref().map_or(0.0, |k| k.size().y);
    let height = (label_galley.size().y + key_h).max(BTN_H);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
    resp.on_hover_ui(|ui| {
        ui.label(trf!("{label}: key in the file", label = label));
        ui.monospace(key);
    });
    let top = rect.center().y - (label_galley.size().y + key_h) / 2.0;
    let painter = ui.painter();
    if on {
        painter.circle_filled(egui::pos2(rect.left() + DOT_W / 2.0, top + label_galley.size().y / 2.0), 3.0, ACCENT);
    }
    let x = rect.left() + DOT_W + GAP;
    let label_h = label_galley.size().y;
    painter.galley(egui::pos2(x, top), label_galley, label_colour);
    if let Some(k) = key_galley {
        painter.galley(egui::pos2(x, top + label_h), k, FAINT);
    }
}

/// A caption over a column of `width`, left-aligned, in the frame of a list row (see `reorder`).
pub fn caption(ui: &mut Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, SMALL + 3.0), Sense::hover());
    ui.painter().text(rect.left_center(), egui::Align2::LEFT_CENTER, text, egui::FontId::proportional(SMALL), DIM);
}

/// A note line of a section (weak text).
pub fn note(ui: &mut Ui, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).size(SMALL).color(DIM)).wrap());
}

/// A square icon button (the reset and the list buttons): no frame, the icon in the middle.
pub fn icon_button(icon: &str, size: f32) -> egui::Button<'_> {
    egui::Button::new(RichText::new(icon).size(14.0)).min_size(Vec2::splat(size)).frame(false)
}

/// A list's icon tool: add, duplicate or remove the selected row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Add,
    Duplicate,
    Remove,
}

/// The icon buttons under a list (add, duplicate, remove), each with its tooltip; returns the tool
/// pressed this frame.
pub fn list_tools(ui: &mut Ui, add: bool, duplicate: bool, remove: bool) -> Option<Tool> {
    let mut out = None;
    ui.horizontal(|ui| {
        if ui.add_enabled(add, icon_button(ic::PLUS, 24.0)).on_hover_text(tr("Add")).clicked() {
            out = Some(Tool::Add);
        }
        if ui.add_enabled(duplicate, icon_button(ic::COPY, 24.0)).on_hover_text(tr("Duplicate the selected row")).clicked() {
            out = Some(Tool::Duplicate);
        }
        if ui.add_enabled(remove, icon_button(ic::TRASH, 24.0)).on_hover_text(tr("Remove the selected row")).clicked() {
            out = Some(Tool::Remove);
        }
    });
    out
}

/// The selected row of the list `salt` (kept across frames), if it is still one of `n` rows.
pub fn selected(ui: &Ui, salt: &str, n: usize) -> Option<usize> {
    let s: Option<usize> = ui.data(|d| d.get_temp(Id::new(("studio-inspector-list", salt))));
    s.filter(|i| *i < n)
}

/// Sets the selected row of the list `salt`.
pub fn set_selected(ui: &Ui, salt: &str, at: Option<usize>) {
    ui.data_mut(|d| d.insert_temp(Id::new(("studio-inspector-list", salt)), at));
}

/// The payload of a dragged row: the list it is from and its row.
#[derive(Clone, Copy)]
struct Grip {
    list: u64,
    from: usize,
}

fn list_key(salt: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    salt.hash(&mut h);
    h.finish()
}

/// The grip of a list row (the drag handle, `GRIP_W` wide): drag it to move row `i` of the list `salt`.
/// The caller draws it at the start of the row's contents, for each of the list's rows.
pub fn grip(ui: &mut Ui, salt: &str, i: usize) {
    let payload = Grip { list: list_key(salt), from: i };
    ui.dnd_drag_source(Id::new(("studio-inspector-grip", salt, i)), payload, |ui| {
        ui.add_sized([GRIP_W, BTN_H - 6.0], egui::Label::new(RichText::new(ic::DOTS_SIX_VERTICAL).color(FAINT).size(14.0)))
    })
    .response
    .on_hover_text(tr("Drag to reorder"));
}

/// The width of a row's grip (the caption rows of a list keep their columns with it).
pub const GRIP_W: f32 = 14.0;

/// The frame of a list row: the row's contents from `row` (`row(ui, i)` draws the grip with `grip`).
/// The selected row has an accent border. A drop shows a 3 px line where the row lands; the drop
/// returns the move (`from`, `to`, in `moved`'s sense), once.
pub fn reorder(ui: &mut Ui, salt: &str, n: usize, selected: Option<usize>, mut row: impl FnMut(&mut Ui, usize)) -> Option<(usize, usize)> {
    let list = list_key(salt);
    let mut rects: Vec<Rect> = Vec::with_capacity(n);
    for i in 0..n {
        let on = selected == Some(i);
        let frame = list_frame(on);
        let shown = frame.show(ui, |ui| row(ui, i));
        rects.push(shown.response.rect);
    }
    let mut out = None;
    let p = egui::DragAndDrop::payload::<Grip>(ui.ctx())?;
    if p.list != list || rects.is_empty() {
        return None;
    }
    let pos = ui.input(|i| i.pointer.latest_pos())?;
    let area = rects.iter().fold(Rect::NOTHING, |a, r| a.union(*r)).expand(6.0);
    if !area.contains(pos) {
        return None;
    }
    let centres: Vec<f32> = rects.iter().map(|r| r.center().y).collect();
    let slot = drop_index(&centres, pos.y);
    let y = if slot == 0 {
        rects[0].top() - 2.0
    } else if slot >= rects.len() {
        rects[rects.len() - 1].bottom() + 2.0
    } else {
        (rects[slot - 1].bottom() + rects[slot].top()) / 2.0
    };
    ui.painter().rect_filled(Rect::from_min_max(egui::pos2(area.left(), y - 1.5), egui::pos2(area.right(), y + 1.5)), CornerRadius::same(2), ACCENT);
    if ui.input(|i| i.pointer.any_released()) {
        if let Some(g) = egui::DragAndDrop::take_payload::<Grip>(ui.ctx()) {
            if let Some(to) = move_target(g.from, slot) {
                out = Some((g.from, to));
            }
        }
    }
    out
}

/// The frame of a list row: its fill, and the accent border of the selected row.
pub fn list_frame(selected: bool) -> Frame {
    Frame::NONE
        .fill(if selected { HOVER } else { WELL })
        .stroke(Stroke::new(1.0, if selected { ACCENT } else { LINE }))
        .corner_radius(CornerRadius::same(RADIUS))
        .inner_margin(Margin::symmetric(4, 3))
}

/// A card: a raised panel with a 1 px line, for the price block and other grouped parts of a form.
pub fn card<R>(ui: &mut Ui, body: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::NONE.fill(RAISED).stroke(Stroke::new(1.0, LINE)).corner_radius(CornerRadius::same(RADIUS)).inner_margin(Margin::same(8)).show(ui, body).inner
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_filter_matches_label_or_file_key_ignoring_case() {
        assert!(matches("", "Имя", "army.name"), "an empty filter shows every row");
        assert!(matches("имя", "Имя", "army.name"), "a label, in any case");
        assert!(matches("ARMY.NA", "Имя", "army.name"), "a file key");
        assert!(!matches("gold", "Имя", "army.name"), "no match hides the row");
        assert!(matches("  name  ", "Имя", "army.name"), "the query is trimmed");
    }

    #[test]
    fn a_drop_lands_in_the_slot_under_the_pointer() {
        let centres = [10.0, 30.0, 50.0];
        assert_eq!(drop_index(&centres, 5.0), 0, "above the first row");
        assert_eq!(drop_index(&centres, 20.0), 1, "between the first and the second");
        assert_eq!(drop_index(&centres, 80.0), 3, "below the last row");
    }

    #[test]
    fn a_drop_on_the_row_itself_or_just_below_it_moves_nothing() {
        assert_eq!(move_target(1, 1), None);
        assert_eq!(move_target(1, 2), None);
        assert_eq!(move_target(1, 0), Some(0), "up one");
        assert_eq!(move_target(0, 3), Some(2), "down two: the row lands before slot 3, so index 2");
        assert_eq!(move_target(2, 0), Some(0), "to the top");
    }

    #[test]
    fn a_move_shifts_the_entries_between() {
        let v = vec!["a", "b", "c", "d"];
        assert_eq!(moved(&v, 0, 2), vec!["b", "c", "a", "d"]);
        assert_eq!(moved(&v, 3, 0), vec!["d", "a", "b", "c"]);
        assert_eq!(moved(&v, 1, 1), v);
        // The drop target of move_target is the index moved() takes.
        let from = 0;
        let slot = 3;
        let to = move_target(from, slot).unwrap_or(from);
        assert_eq!(moved(&v, from, to), vec!["b", "c", "a", "d"]);
    }
}
