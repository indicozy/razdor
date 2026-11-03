//! The inspector's forms, one per kind of table: the hero, a building, an army, a lantern and an
//! object. Each field is a widget that makes one `Edit` when its value is committed: a number on
//! the release of its drag (or its typed Enter), a text on Enter or when the field loses focus, a
//! choice at once. The values come from the table's text (`toml` read), so the form shows what the
//! file says, the edit is format-preserving (`studio::edit`), and the comments survive.
//!
//! The lists: the format's own (`pack::compile`: the building types, the army models, the object
//! kinds, the factions, the ships, the hero classes) and the unit choices (the package's units and
//! the `dt:` aliases, `pack::aliases`). Text keys offer the keys of the language's texts that name
//! the table, and show the text they resolve to under the field.

use std::collections::BTreeMap;

use egui::{ComboBox, Id, Ui};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::pack::compile::{ARMY_MODELS, BUILDING_NAMES, CLASSES, FACTIONS, OBJECT_KINDS, SHIPS};
use razdor::pack::map::{ArmySpec, BuildingSpec, HeroSpec, LeaderSpec, ObjectSpec, PointSpec, RecruitSpec, TroopSpec};
use razdor::studio::{Edit, Locator};
use toml_edit::{InlineTable, Value};

/// What a form needs of the package around its table.
pub struct Ctx<'a> {
    /// The map's id.
    pub map: &'a str,
    /// The map's size in cells (the cell fields clamp to it).
    pub size: (u32, u32),
    /// The texts of the language the studio shows, key to text (empty when there are none).
    pub texts: &'a BTreeMap<String, String>,
    /// The unit choices: the package's units, then the `dt:` aliases.
    pub units: &'a [String],
}

/// The widget's id, unique within the frame's form (`salt` names the field).
fn field_id(ui: &Ui, salt: &str) -> Id {
    ui.id().with(("studio-form", salt))
}

/// A caption for a field: its translated label, then its raw key in a weaker monospace.
fn caption(ui: &mut Ui, label: &str, key: &str) {
    ui.horizontal(|ui| {
        ui.label(label);
        ui.weak(egui::RichText::new(key).monospace());
    });
}

/// A whole number in `lo..=hi`, typed in a text field and committed on Enter or when the field
/// loses focus (one edit per value). A typed value outside the range is clamped to it. Returns the
/// value when it is committed and differs from `model`.
fn int_field(ui: &mut Ui, salt: &str, model: i64, lo: i64, hi: i64) -> Option<i64> {
    let id = field_id(ui, salt);
    let mut buf: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| model.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(56.0));
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    let typed = buf.trim().parse::<i64>().ok()?.clamp(lo, hi);
    (r.lost_focus() && typed != model).then_some(typed)
}

/// A choice among `options`, applied at once. Returns the picked option when it differs.
fn choice(ui: &mut Ui, salt: &str, current: &str, options: &[&str]) -> Option<String> {
    let mut picked = None;
    ComboBox::from_id_salt(field_id(ui, salt)).selected_text(current).show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(current == *o, *o).clicked() && *o != current {
                picked = Some(o.to_string());
            }
        }
    });
    picked
}

/// A text, committed on Enter or when the field loses focus.
fn text_field(ui: &mut Ui, salt: &str, current: &str, width: f32) -> Option<String> {
    let id = field_id(ui, salt);
    let mut buf: String = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| current.to_string());
    let r = ui.add(egui::TextEdit::singleline(&mut buf).desired_width(width));
    if r.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, buf.clone()));
    } else {
        ui.data_mut(|d| d.remove::<String>(id));
    }
    (r.lost_focus() && buf != current).then_some(buf)
}

/// A text key: a free field, a list of the keys that name the table, and the text it resolves to.
fn text_key(ui: &mut Ui, salt: &str, current: &str, suggestions: &[String], texts: &BTreeMap<String, String>) -> Option<String> {
    let mut out = None;
    ui.horizontal(|ui| {
        // The picker first: at the inspector's edge the field is what gets cut, not the picker.
        let mut picked = None;
        ComboBox::from_id_salt(field_id(ui, &format!("{salt}-keys"))).selected_text("▾").width(24.0).show_ui(ui, |ui| {
            for key in suggestions {
                if ui.selectable_label(key == current, key.as_str()).clicked() && key != current {
                    picked = Some(key.clone());
                }
            }
        });
        if picked.is_some() {
            out = picked;
        }
        if let Some(v) = text_field(ui, salt, current, 96.0) {
            out = Some(v);
        }
    });
    // The resolved text wraps in the field's column (the inspector is narrow).
    let resolved = match texts.get(current) {
        Some(text) => text.clone(),
        None if !current.is_empty() => format!("{} {current}", tr("No text for this key in the language:")),
        None => String::new(),
    };
    if !resolved.is_empty() {
        ui.add(egui::Label::new(egui::RichText::new(resolved).weak()).wrap());
    }
    out
}

/// The keys of the texts that name a table: those with a segment of its id or of its map's name.
pub fn suggestions(texts: &BTreeMap<String, String>, names: &[&str]) -> Vec<String> {
    texts
        .keys()
        .filter(|k| k.split('.').any(|seg| names.contains(&seg)))
        .cloned()
        .collect()
}

/// A row list (troops, garrison, recruit): unit, and two numbers, with a remove button per row and
/// an add button under them up to `max`. Returns the new rows when one of them changed.
fn rows_field(
    ui: &mut Ui,
    salt: &str,
    rows: &[(String, i64, i64)],
    units: &[String],
    max: usize,
    (a_label, a_range): (&str, (i64, i64)),
    (b_label, b_range): (&str, (i64, i64)),
) -> Option<Vec<(String, i64, i64)>> {
    let mut out: Option<Vec<(String, i64, i64)>> = None;
    if !rows.is_empty() {
        ui.weak(format!("{a_label} · {b_label}"));
    }
    for (i, (unit, a, b)) in rows.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut next = unit.clone();
            if let Some(v) = choice(ui, &format!("{salt}-unit-{i}"), unit, &units.iter().map(String::as_str).collect::<Vec<_>>()) {
                next = v;
            }
            if let Some(v) = int_field(ui, &format!("{salt}-a-{i}"), *a, a_range.0, a_range.1) {
                let mut r = rows.to_vec();
                r[i] = (unit.clone(), v, *b);
                out = Some(r);
            }
            if let Some(v) = int_field(ui, &format!("{salt}-b-{i}"), *b, b_range.0, b_range.1) {
                let mut r = rows.to_vec();
                r[i] = (unit.clone(), *a, v);
                out = Some(r);
            }
            if ui.small_button("✕").on_hover_text(tr("Remove")).clicked() {
                let mut r = rows.to_vec();
                r.remove(i);
                out = Some(r);
            }
            if next != *unit {
                let mut r = rows.to_vec();
                r[i] = (next, *a, *b);
                out = Some(r);
            }
        });
    }
    if rows.len() < max && ui.small_button(tr("Add")).clicked() {
        let mut r = rows.to_vec();
        r.push((units.first().cloned().unwrap_or_default(), a_range.0.max(1), b_range.0.max(1)));
        out = Some(r);
    }
    out
}

/// The troop rows of a list in the form's order, as inline tables with `level` and `count`.
fn troop_rows(list: &[TroopSpec]) -> Vec<(String, i64, i64)> {
    list.iter().map(|t| (t.unit.clone(), i64::from(t.level), i64::from(t.count))).collect()
}

/// The rows as the file's array: one inline table per row (`unit`, `a_key`, `b_key`).
fn rows_items(rows: &[(String, i64, i64)], a_key: &str, b_key: &str) -> Vec<Value> {
    rows.iter()
        .map(|(unit, a, b)| {
            let mut t = InlineTable::new();
            t.insert("unit", Value::from(unit.as_str()));
            t.insert(a_key, Value::from(*a));
            t.insert(b_key, Value::from(*b));
            Value::InlineTable(t)
        })
        .collect()
}

/// Keeps the first edit of a frame (a form makes at most one per frame; the others wait for the next).
fn keep(out: &mut Option<Edit>, edit: Option<Edit>) {
    if out.is_none() {
        *out = edit;
    }
}

fn set(at: &Locator, key: &str, value: Value) -> Edit {
    Edit::SetValue { at: at.clone(), key: key.to_string(), value }
}

fn set_rows(at: &Locator, key: &str, items: Vec<Value>) -> Edit {
    Edit::SetArray { at: at.clone(), key: key.to_string(), items }
}

/// The cell fields of a table: its x and y clamped to the map. Returns the new cell when committed.
fn cell_field(ui: &mut Ui, salt: &str, cell: [u32; 2], size: (u32, u32)) -> Option<[u32; 2]> {
    let mut out = None;
    ui.horizontal(|ui| {
        let max_x = i64::from(size.0.max(1) - 1);
        let max_y = i64::from(size.1.max(1) - 1);
        let x = int_field(ui, &format!("{salt}-x"), i64::from(cell[0]), 0, max_x);
        let y = int_field(ui, &format!("{salt}-y"), i64::from(cell[1]), 0, max_y);
        if x.is_some() || y.is_some() {
            out = Some([x.map_or(cell[0], |v| v as u32), y.map_or(cell[1], |v| v as u32)]);
        }
    });
    out
}

/// A checkbox that says whether an optional key is in the table (its state when toggled).
fn optional(ui: &mut Ui, label: &str, present: bool) -> Option<bool> {
    let mut on = present;
    (ui.checkbox(&mut on, label).changed() && on != present).then_some(on)
}

/// The hero's form.
pub fn hero(ui: &mut Ui, cx: &Ctx, at: &Locator, h: &HeroSpec) -> Option<Edit> {
    let mut out = None;
    egui::Grid::new(field_id(ui, "hero-grid")).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        caption(ui, tr("Cell"), "cell");
        if let Some(c) = cell_field(ui, "hero-cell", h.cell, cx.size) {
            out = Some(Edit::SetCell { at: at.clone(), cell: (c[0] as i32, c[1] as i32) });
        }
        ui.end_row();
        caption(ui, tr("Gold"), "gold");
        if let Some(v) = int_field(ui, "hero-gold", i64::from(h.gold), 0, 1_000_000) {
            keep(&mut out, Some(set(at, "gold", Value::from(v))));
        }
        ui.end_row();
        caption(ui, tr("Mana"), "mana");
        if let Some(v) = int_field(ui, "hero-mana", i64::from(h.mana), 0, 1_000_000) {
            keep(&mut out, Some(set(at, "mana", Value::from(v))));
        }
        ui.end_row();
    });
    ui.add_space(4.0);
    caption(ui, tr("Classes"), "classes");
    let mut classes: Vec<String> = h.classes.clone();
    let mut changed = false;
    ui.horizontal(|ui| {
        for c in CLASSES {
            let mut on = classes.iter().any(|x| x == c);
            if ui.checkbox(&mut on, c).changed() {
                changed = true;
                if on {
                    classes.push(c.to_string());
                } else {
                    classes.retain(|x| x != c);
                }
            }
        }
    });
    if changed {
        let items: Vec<Value> = CLASSES.iter().filter(|c| classes.iter().any(|x| x == *c)).map(|c| Value::from(*c)).collect();
        keep(&mut out, Some(set_rows(at, "classes", items)));
    }
    ui.add_space(4.0);
    caption(ui, tr("Troops"), "troops");
    if let Some(rows) = rows_field(ui, "hero-troops", &troop_rows(&h.troops), cx.units, 6, (tr("Level"), (1, 7)), (tr("Count"), (1, 255))) {
        keep(&mut out, Some(set_rows(at, "troops", rows_items(&rows, "level", "count"))));
    }
    out
}

/// A building's form.
pub fn building(ui: &mut Ui, cx: &Ctx, at: &Locator, b: &BuildingSpec) -> Option<Edit> {
    let mut out = None;
    let names: [&str; 2] = [b.id.as_str(), cx.map];
    let keys = suggestions(cx.texts, &names);
    egui::Grid::new(field_id(ui, "building-grid")).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        caption(ui, tr("Id"), "id");
        ui.monospace(b.id.as_str());
        ui.end_row();
        caption(ui, tr("Type"), "type");
        if let Some(v) = choice(ui, "building-type", &b.kind, &BUILDING_NAMES) {
            keep(&mut out, Some(set(at, "type", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Name"), "name");
        if let Some(v) = text_key(ui, "building-name", &b.name, &keys, cx.texts) {
            keep(&mut out, Some(set(at, "name", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Description"), "description");
        ui.horizontal(|ui| {
            if let Some(on) = optional(ui, "", b.description.is_some()) {
                keep(&mut out, Some(if on {
                    set(at, "description", Value::from(format!("{}.building.{}.description", cx.map, b.id)))
                } else {
                    Edit::RemoveKey { at: at.clone(), key: "description".into() }
                }));
            }
            if let Some(d) = &b.description {
                if let Some(v) = text_key(ui, "building-description", d, &keys, cx.texts) {
                    keep(&mut out, Some(set(at, "description", Value::from(v.as_str()))));
                }
            }
        });
        ui.end_row();
        caption(ui, tr("Cell"), "cell");
        if let Some(c) = cell_field(ui, "building-cell", b.cell, cx.size) {
            keep(&mut out, Some(Edit::SetCell { at: at.clone(), cell: (c[0] as i32, c[1] as i32) }));
        }
        ui.end_row();
        caption(ui, tr("Size"), "size");
        ui.horizontal(|ui| {
            let (w, h) = razdor::pack::compile::footprint_of(b);
            let default = razdor::pack::compile::default_footprint(&b.kind).unwrap_or((1, 1));
            if let Some(on) = optional(ui, "", b.size.is_some()) {
                keep(&mut out, Some(if on {
                    set(at, "size", int_array(&[default.0, default.1]))
                } else {
                    Edit::RemoveKey { at: at.clone(), key: "size".into() }
                }));
            }
            if b.size.is_some() {
                let nw = int_field(ui, "building-size-w", i64::from(w), 1, 9);
                let nh = int_field(ui, "building-size-h", i64::from(h), 1, 9);
                if nw.is_some() || nh.is_some() {
                    keep(&mut out, Some(set(at, "size", int_array(&[nw.unwrap_or(i64::from(w)) as u32, nh.unwrap_or(i64::from(h)) as u32]))));
                }
            } else {
                ui.weak(format!("{}×{} {}", default.0, default.1, tr("by the type")));
            }
        });
        ui.end_row();
        caption(ui, tr("Faction"), "faction");
        if let Some(v) = choice(ui, "building-faction", &b.faction, &FACTIONS) {
            keep(&mut out, Some(set(at, "faction", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Income"), "income");
        if let Some(v) = int_field(ui, "building-income", i64::from(b.income), 0, 65_535) {
            keep(&mut out, Some(set(at, "income", Value::from(v))));
        }
        ui.end_row();
        caption(ui, tr("Gold max"), "gold_max");
        ui.horizontal(|ui| {
            if let Some(on) = optional(ui, "", b.gold_max.is_some()) {
                keep(&mut out, Some(if on {
                    set(at, "gold_max", Value::from(i64::from(b.income) * 4))
                } else {
                    Edit::RemoveKey { at: at.clone(), key: "gold_max".into() }
                }));
            }
            match b.gold_max {
                Some(g) => {
                    if let Some(v) = int_field(ui, "building-gold-max", i64::from(g), 0, 65_535) {
                        keep(&mut out, Some(set(at, "gold_max", Value::from(v))));
                    }
                }
                None => {
                    ui.weak(tr("four days' income"));
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.weak(tr("rename comes later"));
    ui.add_space(4.0);
    caption(ui, tr("Garrison"), "garrison");
    if let Some(rows) = rows_field(ui, "building-garrison", &troop_rows(&b.garrison), cx.units, 6, (tr("Level"), (1, 7)), (tr("Count"), (1, 255))) {
        keep(&mut out, Some(set_rows(at, "garrison", rows_items(&rows, "level", "count"))));
    }
    caption(ui, tr("Recruit"), "recruit");
    let recruit: Vec<(String, i64, i64)> = b.recruit.iter().map(|r: &RecruitSpec| (r.unit.clone(), i64::from(r.start), i64::from(r.max))).collect();
    if let Some(rows) = rows_field(ui, "building-recruit", &recruit, cx.units, 6, (tr("Start"), (0, 255)), (tr("Max"), (0, 255))) {
        keep(&mut out, Some(set_rows(at, "recruit", rows_items(&rows, "start", "max"))));
    }
    out
}

/// An army's form.
pub fn army(ui: &mut Ui, cx: &Ctx, at: &Locator, a: &ArmySpec) -> Option<Edit> {
    let mut out = None;
    let keys = suggestions(cx.texts, &[a.id.as_str(), cx.map]);
    egui::Grid::new(field_id(ui, "army-grid")).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        caption(ui, tr("Id"), "id");
        ui.monospace(a.id.as_str());
        ui.end_row();
        caption(ui, tr("Name"), "name");
        if let Some(v) = text_key(ui, "army-name", &a.name, &keys, cx.texts) {
            keep(&mut out, Some(set(at, "name", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Cell"), "cell");
        if let Some(c) = cell_field(ui, "army-cell", a.cell, cx.size) {
            keep(&mut out, Some(Edit::SetCell { at: at.clone(), cell: (c[0] as i32, c[1] as i32) }));
        }
        ui.end_row();
        caption(ui, tr("Faction"), "faction");
        if let Some(v) = choice(ui, "army-faction", &a.faction, &FACTIONS) {
            keep(&mut out, Some(set(at, "faction", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Model"), "model");
        let models: Vec<&str> = ARMY_MODELS.iter().map(|m| m.0).collect();
        if let Some(v) = choice(ui, "army-model", &a.model, &models) {
            keep(&mut out, Some(set(at, "model", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Ship"), "ship");
        let ship = a.ship.clone().unwrap_or_else(|| SHIPS[0].to_string());
        if let Some(v) = choice(ui, "army-ship", &ship, &SHIPS) {
            keep(&mut out, Some(set(at, "ship", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Gold"), "gold");
        if let Some(v) = int_field(ui, "army-gold", i64::from(a.gold), 0, 1_000_000) {
            keep(&mut out, Some(set(at, "gold", Value::from(v))));
        }
        ui.end_row();
        caption(ui, tr("Battle"), "battle");
        ui.horizontal(|ui| {
            if let Some(on) = optional(ui, "", a.battle.is_some()) {
                keep(&mut out, Some(if on {
                    set(at, "battle", Value::from("dt"))
                } else {
                    Edit::RemoveKey { at: at.clone(), key: "battle".into() }
                }));
            }
            if let Some(b) = &a.battle {
                if let Some(v) = text_field(ui, "army-battle", b, 180.0) {
                    keep(&mut out, Some(set(at, "battle", Value::from(v.as_str()))));
                }
            }
        });
        ui.end_row();
    });
    ui.add_space(4.0);
    ui.weak(tr("rename comes later"));
    ui.add_space(4.0);
    caption(ui, tr("Troops"), "troops");
    if let Some(rows) = rows_field(ui, "army-troops", &troop_rows(&a.troops), cx.units, 6, (tr("Level"), (1, 7)), (tr("Count"), (1, 255))) {
        keep(&mut out, Some(set_rows(at, "troops", rows_items(&rows, "level", "count"))));
    }
    ui.add_space(4.0);
    caption(ui, tr("Leader"), "leader");
    ui.horizontal(|ui| {
        if let Some(on) = optional(ui, "", a.leader.is_some()) {
            keep(&mut out, Some(if on {
                let fresh = LeaderSpec { unit: cx.units.first().cloned().unwrap_or_default(), level: 1, name: format!("{}.army.{}.leader.name", cx.map, a.id) };
                set(at, "leader", leader_value(Some(&fresh)))
            } else {
                Edit::RemoveKey { at: at.clone(), key: "leader".into() }
            }));
        }
    });
    if let Some(l) = &a.leader {
        let mut changed = None;
        ui.horizontal(|ui| {
            if let Some(v) = choice(ui, "army-leader-unit", &l.unit, &cx.units.iter().map(String::as_str).collect::<Vec<_>>()) {
                changed = Some(LeaderSpec { unit: v, ..l.clone() });
            }
            if let Some(v) = int_field(ui, "army-leader-level", i64::from(l.level), 1, 7) {
                changed = Some(LeaderSpec { level: v as u8, ..l.clone() });
            }
        });
        if let Some(text) = text_key(ui, "army-leader-name", &l.name, &keys, cx.texts) {
            changed = Some(LeaderSpec { name: text, ..l.clone() });
        }
        if let Some(c) = changed {
            keep(&mut out, Some(set(at, "leader", leader_value(Some(&c)))));
        }
    }
    out
}

fn leader_value(l: Option<&LeaderSpec>) -> Value {
    let Some(l) = l else { return Value::from("") };
    let mut t = InlineTable::new();
    t.insert("unit", Value::from(l.unit.as_str()));
    t.insert("level", Value::from(i64::from(l.level)));
    t.insert("name", Value::from(l.name.as_str()));
    Value::InlineTable(t)
}

fn int_array(values: &[u32]) -> Value {
    let mut a = toml_edit::Array::new();
    for v in values {
        a.push(i64::from(*v));
    }
    Value::Array(a)
}

/// A lantern's form.
pub fn point(ui: &mut Ui, cx: &Ctx, at: &Locator, p: &PointSpec) -> Option<Edit> {
    let mut out = None;
    egui::Grid::new(field_id(ui, "point-grid")).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        caption(ui, tr("Id"), "id");
        ui.weak(p.id.clone().unwrap_or_else(|| tr("none: a lantern").to_string()));
        ui.end_row();
        caption(ui, tr("Cell"), "cell");
        if let Some(c) = cell_field(ui, "point-cell", p.cell, cx.size) {
            keep(&mut out, Some(Edit::SetCell { at: at.clone(), cell: (c[0] as i32, c[1] as i32) }));
        }
        ui.end_row();
        caption(ui, tr("Radius"), "radius");
        if let Some(v) = int_field(ui, "point-radius", i64::from(p.radius), 0, 24) {
            keep(&mut out, Some(set(at, "radius", Value::from(v))));
        }
        ui.end_row();
        caption(ui, tr("Active"), "active");
        let mut on = p.active;
        if ui.checkbox(&mut on, tr("lit from the start")).changed() {
            keep(&mut out, Some(set(at, "active", Value::from(on))));
        }
        ui.end_row();
    });
    out
}

/// An object's form.
pub fn object(ui: &mut Ui, cx: &Ctx, at: &Locator, o: &ObjectSpec) -> Option<Edit> {
    let mut out = None;
    egui::Grid::new(field_id(ui, "object-grid")).num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
        caption(ui, tr("Kind"), "kind");
        let kinds: Vec<&str> = OBJECT_KINDS.iter().map(|k| k.0).collect();
        if let Some(v) = choice(ui, "object-kind", &o.kind, &kinds) {
            keep(&mut out, Some(set(at, "kind", Value::from(v.as_str()))));
        }
        ui.end_row();
        caption(ui, tr("Cell"), "cell");
        if let Some(c) = cell_field(ui, "object-cell", o.cell, cx.size) {
            keep(&mut out, Some(Edit::SetCell { at: at.clone(), cell: (c[0] as i32, c[1] as i32) }));
        }
        ui.end_row();
        caption(ui, tr("Sprite"), "sprite");
        ui.horizontal(|ui| {
            let default = OBJECT_KINDS.iter().find(|k| k.0 == o.kind).map_or(0, |k| k.2);
            if let Some(on) = optional(ui, "", o.sprite.is_some()) {
                keep(&mut out, Some(if on {
                    set(at, "sprite", Value::from(i64::from(default)))
                } else {
                    Edit::RemoveKey { at: at.clone(), key: "sprite".into() }
                }));
            }
            match o.sprite {
                Some(s) => {
                    if let Some(v) = int_field(ui, "object-sprite", i64::from(s), 0, 255) {
                        keep(&mut out, Some(set(at, "sprite", Value::from(v))));
                    }
                }
                None => {
                    ui.weak(format!("{} {default}", tr("by the kind:")));
                }
            }
        });
        ui.end_row();
    });
    out
}
