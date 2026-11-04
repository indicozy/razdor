//! The inspector's forms, one per kind of table: the hero, a building, an army, a lantern and an
//! object. Each field is a widget that makes one `Edit` when its value is committed: a number on
//! the release of its drag (or its typed Enter), a text on Enter or when the field loses focus, a
//! choice at once. The values come from the table's text (`toml` read), so the form shows what the
//! file says, the edit is format-preserving (`studio::edit`), and the comments survive.
//!
//! The layout is the Inspector widget set (`inspector_kit`, UX2): each form is sections of rows, a row
//! is a label in a fixed column and its value, and a list of troops drags to reorder. The rows are
//! filtered by the field search (`Ctx::query`); a filtered-out row makes no edit.
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

use super::inspector_kit::{self as kit, Tool};

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
    /// The field search (empty: every field shows).
    pub query: &'a str,
}

/// The widget's id, unique within the frame's form (`salt` names the field).
fn field_id(ui: &Ui, salt: &str) -> Id {
    ui.id().with(("studio-form", salt))
}

/// A whole number in `lo..=hi`, typed in a text field and committed on Enter or when the field
/// loses focus (one edit per value). A typed value outside the range is clamped to it. Returns the
/// value when it is committed and differs from `model`.
pub(super) fn int_field(ui: &mut Ui, salt: &str, model: i64, lo: i64, hi: i64) -> Option<i64> {
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
pub(super) fn choice(ui: &mut Ui, salt: &str, current: &str, options: &[&str]) -> Option<String> {
    choice_w(ui, salt, current, options, None)
}

/// A choice of a given width (the list rows fix theirs so the columns line up).
fn choice_w(ui: &mut Ui, salt: &str, current: &str, options: &[&str], width: Option<f32>) -> Option<String> {
    let mut picked = None;
    let mut combo = ComboBox::from_id_salt(field_id(ui, salt)).selected_text(current);
    if let Some(w) = width {
        combo = combo.width(w);
    }
    combo.show_ui(ui, |ui| {
        for o in options {
            if ui.selectable_label(current == *o, *o).clicked() && *o != current {
                picked = Some(o.to_string());
            }
        }
    });
    picked
}

/// A text, committed on Enter or when the field loses focus.
pub(super) fn text_field(ui: &mut Ui, salt: &str, current: &str, width: f32) -> Option<String> {
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
pub(super) fn text_key(ui: &mut Ui, salt: &str, current: &str, suggestions: &[String], texts: &BTreeMap<String, String>) -> Option<String> {
    let mut out = None;
    // A grid cell lays its contents out left to right: the field and the resolved text are stacked
    // in a column, so the text gets its own line under the field.
    ui.vertical(|ui| {
        ui.horizontal(|ui| {
            // The picker first: at the inspector's edge the field is what gets cut, not the picker.
            let mut picked = None;
            ComboBox::from_id_salt(field_id(ui, &format!("{salt}-keys"))).selected_text(tr("Text keys")).width(64.0).show_ui(ui, |ui| {
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
        // The resolved text wraps in the inspector's width, from the field's column to the panel's
        // edge (the column is as wide as its widest row, which is what pushed the text past it).
        let resolved = match texts.get(current) {
            Some(text) => text.clone(),
            None if !current.is_empty() => format!("{} {current}", tr("No text for this key in the language:")),
            None => String::new(),
        };
        if !resolved.is_empty() {
            let width = (ui.clip_rect().right() - ui.cursor().left() - 4.0).max(120.0);
            ui.allocate_ui(egui::vec2(width, 0.0), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(resolved).weak()).wrap());
            });
        }
    });
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

/// A row list (troops, garrison, recruit): unit, and two numbers. A row is selected by its number,
/// the list's icon tools add, duplicate and remove the selected row, and a grip drags a row to a new
/// place. Returns the new rows when the list changed (one edit).
fn rows_field(
    ui: &mut Ui,
    salt: &str,
    rows: &[(String, i64, i64)],
    units: &[String],
    max: usize,
    (a_label, a_range): (&str, (i64, i64)),
    (b_label, b_range): (&str, (i64, i64)),
) -> Option<Vec<(String, i64, i64)>> {
    // A number field is 56 wide with its frame: the caption column is as wide as the field.
    const NUM_W: f32 = 64.0;
    const INDEX_W: f32 = 24.0;
    // The unit takes what the row leaves: grip, index, two numbers and their gaps are fixed (the row fits the
    // Inspector's column at any width, where a fixed unit width overflowed it).
    let unit_w = (ui.available_width() - (kit::GRIP_W + INDEX_W + NUM_W * 2.0 + 6.0 * 5.0)).clamp(90.0, 260.0);
    let sel = kit::selected(ui, salt, rows.len());
    if !rows.is_empty() {
        // The captions sit in a frame of the rows' size and spacing, so each one is over its column.
        egui::Frame::NONE.stroke(egui::Stroke::new(1.0, egui::Color32::TRANSPARENT)).inner_margin(egui::Margin::symmetric(4, 3)).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.add_space(kit::GRIP_W + INDEX_W + 6.0);
                kit::caption(ui, unit_w, tr("Unit"));
                kit::caption(ui, NUM_W, a_label);
                kit::caption(ui, NUM_W, b_label);
            });
        });
    }
    let unit_refs: Vec<&str> = units.iter().map(String::as_str).collect();
    let mut edits: Vec<(usize, (String, i64, i64))> = Vec::new();
    let mut picked: Option<usize> = None;
    let moved = kit::reorder(ui, salt, rows.len(), sel, |ui, i| {
        let (unit, a, b) = &rows[i];
        ui.horizontal(|ui| {
            kit::grip(ui, salt, i);
            if ui.add_sized([INDEX_W, 20.0], egui::SelectableLabel::new(sel == Some(i), format!("{}", i + 1))).clicked() {
                picked = Some(i);
            }
            let mut next = (unit.clone(), *a, *b);
            if let Some(v) = choice_w(ui, &format!("{salt}-unit-{i}"), unit, &unit_refs, Some(unit_w)) {
                next.0 = v;
            }
            if let Some(v) = int_field(ui, &format!("{salt}-a-{i}"), *a, a_range.0, a_range.1) {
                next.1 = v;
            }
            if let Some(v) = int_field(ui, &format!("{salt}-b-{i}"), *b, b_range.0, b_range.1) {
                next.2 = v;
            }
            if next != (unit.clone(), *a, *b) {
                edits.push((i, next));
            }
        });
    });
    if let Some(i) = picked {
        kit::set_selected(ui, salt, Some(i));
    }
    let mut out: Option<Vec<(String, i64, i64)>> = None;
    for (i, r) in edits {
        let mut v = rows.to_vec();
        v[i] = r;
        out = Some(v);
    }
    if let Some((from, to)) = moved {
        let base = out.take().unwrap_or_else(|| rows.to_vec());
        out = Some(kit::moved(&base, from, to));
        if sel == Some(from) {
            kit::set_selected(ui, salt, Some(to));
        }
    }
    let room = rows.len() < max;
    let tool = kit::list_tools(ui, room, room && sel.is_some(), sel.is_some());
    let mut v = out.unwrap_or_else(|| rows.to_vec());
    match (tool, sel) {
        (Some(Tool::Add), _) if room => {
            v.push((units.first().cloned().unwrap_or_default(), a_range.0.max(1), b_range.0.max(1)));
            kit::set_selected(ui, salt, None);
        }
        (Some(Tool::Duplicate), Some(i)) if room && i < v.len() => {
            let copy = v[i].clone();
            v.insert(i + 1, copy);
            kit::set_selected(ui, salt, Some(i + 1));
        }
        (Some(Tool::Remove), Some(i)) if i < v.len() => {
            v.remove(i);
            kit::set_selected(ui, salt, None);
        }
        _ => {}
    }
    // The list is written only when it differs from the table's rows.
    (v != rows).then_some(v)
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
pub(super) fn optional(ui: &mut Ui, label: &str, present: bool) -> Option<bool> {
    let mut on = present;
    (ui.checkbox(&mut on, label).changed() && on != present).then_some(on)
}

/// The cell of a table as the edit it makes.
fn cell_edit(at: &Locator, cell: [u32; 2]) -> Edit {
    Edit::SetCell { at: at.clone(), cell: (cell[0] as i32, cell[1] as i32) }
}

/// The hero's form.
pub fn hero(ui: &mut Ui, cx: &Ctx, at: &Locator, h: &HeroSpec) -> Option<Edit> {
    let mut out = None;
    let q = cx.query;
    kit::section(ui, "hero", "main", tr("Main"), true, q, |ui| {
        keep(&mut out, kit::row(ui, q, tr("Cell"), "hero.cell", None, |ui| cell_field(ui, "hero-cell", h.cell, cx.size).map(|c| cell_edit(at, c))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Gold"), "hero.gold", None, |ui| int_field(ui, "hero-gold", i64::from(h.gold), 0, 1_000_000).map(|v| set(at, "gold", Value::from(v)))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Mana"), "hero.mana", None, |ui| int_field(ui, "hero-mana", i64::from(h.mana), 0, 1_000_000).map(|v| set(at, "mana", Value::from(v)))).value.flatten());
        let mut classes: Vec<String> = h.classes.clone();
        let mut changed = false;
        kit::row(ui, q, tr("Classes"), "hero.classes", None, |ui| {
            ui.horizontal_wrapped(|ui| {
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
        });
        if changed {
            let items: Vec<Value> = CLASSES.iter().filter(|c| classes.iter().any(|x| x == *c)).map(|c| Value::from(*c)).collect();
            keep(&mut out, Some(set_rows(at, "classes", items)));
        }
    });
    kit::section(ui, "hero", "troops", tr("Troops"), true, q, |ui| {
        let rows = kit::block(ui, q, tr("Troops"), "hero.troops", |ui| {
            rows_field(ui, "hero-troops", &troop_rows(&h.troops), cx.units, 6, (tr("Level"), (1, 7)), (tr("Count"), (1, 255)))
        })
        .flatten();
        if let Some(rows) = rows {
            keep(&mut out, Some(set_rows(at, "troops", rows_items(&rows, "level", "count"))));
        }
    });
    out
}

/// A building's form.
pub fn building(ui: &mut Ui, cx: &Ctx, at: &Locator, b: &BuildingSpec) -> Option<Edit> {
    let mut out = None;
    let q = cx.query;
    let names: [&str; 2] = [b.id.as_str(), cx.map];
    let keys = suggestions(cx.texts, &names);
    kit::section(ui, "building", "main", tr("Main"), true, q, |ui| {
        kit::row(ui, q, tr("Id"), "building.id", None, |ui| ui.monospace(b.id.as_str()));
        keep(&mut out, kit::row(ui, q, tr("Type"), "building.type", None, |ui| choice(ui, "building-type", &b.kind, &BUILDING_NAMES).map(|v| set(at, "type", Value::from(v.as_str())))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Name"), "building.name", None, |ui| text_key(ui, "building-name", &b.name, &keys, cx.texts).map(|v| set(at, "name", Value::from(v.as_str())))).value.flatten());
        keep(
            &mut out,
            kit::row(ui, q, tr("Description"), "building.description", None, |ui| {
                let mut edit = None;
                ui.horizontal(|ui| {
                    if let Some(on) = optional(ui, "", b.description.is_some()) {
                        edit = Some(if on {
                            set(at, "description", Value::from(format!("{}.building.{}.description", cx.map, b.id)))
                        } else {
                            Edit::RemoveKey { at: at.clone(), key: "description".into() }
                        });
                    }
                    if let Some(d) = &b.description {
                        if let Some(v) = text_key(ui, "building-description", d, &keys, cx.texts) {
                            edit = Some(set(at, "description", Value::from(v.as_str())));
                        }
                    }
                });
                edit
            })
            .value
            .flatten(),
        );
        keep(&mut out, kit::row(ui, q, tr("Faction"), "building.faction", None, |ui| choice(ui, "building-faction", &b.faction, &FACTIONS).map(|v| set(at, "faction", Value::from(v.as_str())))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Cell"), "building.cell", None, |ui| cell_field(ui, "building-cell", b.cell, cx.size).map(|c| cell_edit(at, c))).value.flatten());
        keep(
            &mut out,
            kit::row(ui, q, tr("Size"), "building.size", None, |ui| {
                let mut edit = None;
                let (w, h) = razdor::pack::compile::footprint_of(b);
                let default = razdor::pack::compile::default_footprint(&b.kind).unwrap_or((1, 1));
                ui.horizontal(|ui| {
                    if let Some(on) = optional(ui, "", b.size.is_some()) {
                        edit = Some(if on {
                            set(at, "size", int_array(&[default.0, default.1]))
                        } else {
                            Edit::RemoveKey { at: at.clone(), key: "size".into() }
                        });
                    }
                    if b.size.is_some() {
                        let nw = int_field(ui, "building-size-w", i64::from(w), 1, 9);
                        let nh = int_field(ui, "building-size-h", i64::from(h), 1, 9);
                        if nw.is_some() || nh.is_some() {
                            edit = Some(set(at, "size", int_array(&[nw.unwrap_or(i64::from(w)) as u32, nh.unwrap_or(i64::from(h)) as u32])));
                        }
                    } else {
                        ui.weak(format!("{}×{} {}", default.0, default.1, tr("by the type")));
                    }
                });
                edit
            })
            .value
            .flatten(),
        );
        ui.add_space(4.0);
        kit::note(ui, tr("rename comes later"));
    });
    kit::section(ui, "building", "income", tr("Income"), true, q, |ui| {
        keep(&mut out, kit::row(ui, q, tr("Income"), "building.income", None, |ui| int_field(ui, "building-income", i64::from(b.income), 0, 65_535).map(|v| set(at, "income", Value::from(v)))).value.flatten());
        keep(
            &mut out,
            kit::row(ui, q, tr("Gold max"), "building.gold_max", None, |ui| {
                let mut edit = None;
                ui.horizontal(|ui| {
                    if let Some(on) = optional(ui, "", b.gold_max.is_some()) {
                        edit = Some(if on {
                            set(at, "gold_max", Value::from(i64::from(b.income) * 4))
                        } else {
                            Edit::RemoveKey { at: at.clone(), key: "gold_max".into() }
                        });
                    }
                    match b.gold_max {
                        Some(g) => {
                            if let Some(v) = int_field(ui, "building-gold-max", i64::from(g), 0, 65_535) {
                                edit = Some(set(at, "gold_max", Value::from(v)));
                            }
                        }
                        None => {
                            ui.weak(tr("four days' income"));
                        }
                    }
                });
                edit
            })
            .value
            .flatten(),
        );
    });
    kit::section(ui, "building", "garrison", tr("Garrison"), true, q, |ui| {
        let rows = kit::block(ui, q, tr("Garrison"), "building.garrison", |ui| {
            rows_field(ui, "building-garrison", &troop_rows(&b.garrison), cx.units, 6, (tr("Level"), (1, 7)), (tr("Count"), (1, 255)))
        })
        .flatten();
        if let Some(rows) = rows {
            keep(&mut out, Some(set_rows(at, "garrison", rows_items(&rows, "level", "count"))));
        }
    });
    kit::section(ui, "building", "recruit", tr("Recruit"), true, q, |ui| {
        let recruit: Vec<(String, i64, i64)> = b.recruit.iter().map(|r: &RecruitSpec| (r.unit.clone(), i64::from(r.start), i64::from(r.max))).collect();
        let rows = kit::block(ui, q, tr("Recruit"), "building.recruit", |ui| rows_field(ui, "building-recruit", &recruit, cx.units, 6, (tr("Start"), (0, 255)), (tr("Max"), (0, 255)))).flatten();
        if let Some(rows) = rows {
            keep(&mut out, Some(set_rows(at, "recruit", rows_items(&rows, "start", "max"))));
        }
    });
    out
}

/// An army's form: the main fields, the troops and the leader, the gold, and the battle.
pub fn army(ui: &mut Ui, cx: &Ctx, at: &Locator, a: &ArmySpec) -> Option<Edit> {
    let mut out = None;
    let q = cx.query;
    let keys = suggestions(cx.texts, &[a.id.as_str(), cx.map]);
    kit::section(ui, "army", "main", tr("Main"), true, q, |ui| {
        kit::row(ui, q, tr("Id"), "army.id", None, |ui| ui.monospace(a.id.as_str()));
        keep(&mut out, kit::row(ui, q, tr("Name"), "army.name", None, |ui| text_key(ui, "army-name", &a.name, &keys, cx.texts).map(|v| set(at, "name", Value::from(v.as_str())))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Cell"), "army.cell", None, |ui| cell_field(ui, "army-cell", a.cell, cx.size).map(|c| cell_edit(at, c))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Faction"), "army.faction", None, |ui| choice(ui, "army-faction", &a.faction, &FACTIONS).map(|v| set(at, "faction", Value::from(v.as_str())))).value.flatten());
        let models: Vec<&str> = ARMY_MODELS.iter().map(|m| m.0).collect();
        keep(&mut out, kit::row(ui, q, tr("Model"), "army.model", None, |ui| choice(ui, "army-model", &a.model, &models).map(|v| set(at, "model", Value::from(v.as_str())))).value.flatten());
        let ship = a.ship.clone().unwrap_or_else(|| SHIPS[0].to_string());
        keep(&mut out, kit::row(ui, q, tr("Ship"), "army.ship", None, |ui| choice(ui, "army-ship", &ship, &SHIPS).map(|v| set(at, "ship", Value::from(v.as_str())))).value.flatten());
        ui.add_space(4.0);
        kit::note(ui, tr("rename comes later"));
    });
    kit::section(ui, "army", "troops", tr("Troops"), true, q, |ui| {
        let rows = kit::block(ui, q, tr("Troops"), "army.troops", |ui| {
            rows_field(ui, "army-troops", &troop_rows(&a.troops), cx.units, 6, (tr("Level"), (1, 7)), (tr("Count"), (1, 255)))
        })
        .flatten();
        if let Some(rows) = rows {
            keep(&mut out, Some(set_rows(at, "troops", rows_items(&rows, "level", "count"))));
        }
        keep(
            &mut out,
            kit::row(ui, q, tr("Leader"), "army.leader", None, |ui| {
                let mut edit = None;
                ui.horizontal(|ui| {
                    if let Some(on) = optional(ui, "", a.leader.is_some()) {
                        edit = Some(if on {
                            let fresh = LeaderSpec { unit: cx.units.first().cloned().unwrap_or_default(), level: 1, name: format!("{}.army.{}.leader.name", cx.map, a.id) };
                            set(at, "leader", leader_value(Some(&fresh)))
                        } else {
                            Edit::RemoveKey { at: at.clone(), key: "leader".into() }
                        });
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
                        edit = Some(set(at, "leader", leader_value(Some(&c))));
                    }
                }
                edit
            })
            .value
            .flatten(),
        );
    });
    kit::section(ui, "army", "rewards", tr("Rewards"), true, q, |ui| {
        keep(&mut out, kit::row(ui, q, tr("Gold"), "army.gold", None, |ui| int_field(ui, "army-gold", i64::from(a.gold), 0, 1_000_000).map(|v| set(at, "gold", Value::from(v)))).value.flatten());
    });
    kit::section(ui, "army", "behaviour", tr("Behaviour"), true, q, |ui| {
        keep(
            &mut out,
            kit::row(ui, q, tr("Battle"), "army.battle", None, |ui| {
                let mut edit = None;
                ui.horizontal(|ui| {
                    if let Some(on) = optional(ui, "", a.battle.is_some()) {
                        edit = Some(if on {
                            set(at, "battle", Value::from("dt"))
                        } else {
                            Edit::RemoveKey { at: at.clone(), key: "battle".into() }
                        });
                    }
                    if let Some(b) = &a.battle {
                        if let Some(v) = text_field(ui, "army-battle", b, 180.0) {
                            edit = Some(set(at, "battle", Value::from(v.as_str())));
                        }
                    }
                });
                edit
            })
            .value
            .flatten(),
        );
    });
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
    let q = cx.query;
    kit::section(ui, "point", "main", tr("Main"), true, q, |ui| {
        kit::row(ui, q, tr("Id"), "point.id", None, |ui| ui.weak(p.id.clone().unwrap_or_else(|| tr("none: a lantern").to_string())));
        keep(&mut out, kit::row(ui, q, tr("Cell"), "point.cell", None, |ui| cell_field(ui, "point-cell", p.cell, cx.size).map(|c| cell_edit(at, c))).value.flatten());
    });
    kit::section(ui, "point", "light", tr("Light"), true, q, |ui| {
        keep(&mut out, kit::row(ui, q, tr("Radius"), "point.radius", None, |ui| int_field(ui, "point-radius", i64::from(p.radius), 0, 24).map(|v| set(at, "radius", Value::from(v)))).value.flatten());
        keep(
            &mut out,
            kit::row(ui, q, tr("Active"), "point.active", None, |ui| {
                let mut on = p.active;
                if ui.checkbox(&mut on, tr("lit from the start")).changed() {
                    Some(set(at, "active", Value::from(on)))
                } else {
                    None
                }
            })
            .value
            .flatten(),
        );
    });
    out
}

/// An object's form.
pub fn object(ui: &mut Ui, cx: &Ctx, at: &Locator, o: &ObjectSpec) -> Option<Edit> {
    let mut out = None;
    let q = cx.query;
    kit::section(ui, "object", "main", tr("Main"), true, q, |ui| {
        let kinds: Vec<&str> = OBJECT_KINDS.iter().map(|k| k.0).collect();
        keep(&mut out, kit::row(ui, q, tr("Kind"), "object.kind", None, |ui| choice(ui, "object-kind", &o.kind, &kinds).map(|v| set(at, "kind", Value::from(v.as_str())))).value.flatten());
        keep(&mut out, kit::row(ui, q, tr("Cell"), "object.cell", None, |ui| cell_field(ui, "object-cell", o.cell, cx.size).map(|c| cell_edit(at, c))).value.flatten());
        keep(
            &mut out,
            kit::row(ui, q, tr("Sprite"), "object.sprite", None, |ui| {
                let mut edit = None;
                let default = OBJECT_KINDS.iter().find(|k| k.0 == o.kind).map_or(0, |k| k.2);
                ui.horizontal(|ui| {
                    if let Some(on) = optional(ui, "", o.sprite.is_some()) {
                        edit = Some(if on {
                            set(at, "sprite", Value::from(i64::from(default)))
                        } else {
                            Edit::RemoveKey { at: at.clone(), key: "sprite".into() }
                        });
                    }
                    match o.sprite {
                        Some(s) => {
                            if let Some(v) = int_field(ui, "object-sprite", i64::from(s), 0, 255) {
                                edit = Some(set(at, "sprite", Value::from(v)));
                            }
                        }
                        None => {
                            ui.weak(format!("{} {default}", tr("by the kind:")));
                        }
                    }
                });
                edit
            })
            .value
            .flatten(),
        );
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::studio::map::{locator_of, Selection};
    use razdor::studio::records::{self as rec, Kind};
    use razdor::studio::story as st;
    use razdor::studio::Workspace;
    use toml_edit::DocumentMut;

    /// A temporary copy of the example package, opened as a workspace (the copy is removed with the test).
    fn example() -> (razdor::studio::gen::apply::TempCopy, Workspace) {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packs/pepelnaya-marka");
        let copy = razdor::studio::gen::apply::copy_package(&src).expect("the example package copies");
        let ws = Workspace::open(copy.path()).expect("the copy opens");
        (copy, ws)
    }

    #[test]
    fn a_rows_list_edit_keeps_the_other_rows_in_order() {
        // The troops as the form holds them: the reorder through the kit's move is what the form writes.
        let rows = vec![("a".to_string(), 1, 2), ("b".to_string(), 3, 4), ("c".to_string(), 5, 6)];
        let moved = kit::moved(&rows, 2, 0);
        assert_eq!(moved.iter().map(|r| r.0.as_str()).collect::<Vec<_>>(), vec!["c", "a", "b"]);
        let items = rows_items(&moved, "level", "count");
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].as_inline_table().and_then(|t| t.get("unit")).and_then(Value::as_str), Some("c"));
    }

    #[test]
    fn reordering_an_armys_troops_is_one_undo_step_and_undo_restores_the_bytes() {
        let (_copy, mut ws) = example();
        let file = "maps/brod/map.toml";
        let before = ws.text_of(file).expect("the map's text");
        let spec = super::super::inspector::spec_now(&ws, None, "brod").expect("the map's spec");
        let army = spec.army.iter().map(|a| a.get_ref()).find(|a| a.id == "toll").expect("the army toll");
        let rows = troop_rows(&army.troops);
        assert!(rows.len() >= 2, "toll has two troop rows to swap");
        let at = locator_of(file, &spec, &Selection::Army("toll".into())).expect("toll's locator");
        let moved = kit::moved(&rows, 0, 1);
        ws.apply(set_rows(&at, "troops", rows_items(&moved, "level", "count"))).expect("the reorder applies");
        let after = super::super::inspector::spec_now(&ws, None, "brod").expect("the spec after");
        let army = after.army.iter().map(|a| a.get_ref()).find(|a| a.id == "toll").expect("toll still there");
        assert_eq!(troop_rows(&army.troops), moved, "the troops are in the new order");
        assert!(ws.undo().expect("undo").is_some(), "the reorder is an undo step");
        assert_eq!(ws.text_of(file).expect("the text after undo"), before, "undo restores the bytes");
    }

    #[test]
    fn reordering_a_dialogs_choices_is_one_undo_step_and_undo_restores_the_bytes() {
        let (_copy, mut ws) = example();
        let (pkg, _) = ws.read_package().expect("the package reads");
        let (file, id) = pkg
            .dialogs
            .iter()
            .filter_map(|d| {
                let text = ws.text_of(&d.file).ok()?;
                let draft = st::dialog_draft(&text, &d.value.id)?;
                (draft.choices.len() >= 2).then(|| (d.file.clone(), d.value.id.clone()))
            })
            .next()
            .expect("a dialog with two choices");
        let before = ws.text_of(&file).expect("the dialog file's text");
        let draft = st::dialog_draft(&before, &id).expect("the dialog parses");
        let (first, second) = (draft.choices[0].id.clone(), draft.choices[1].id.clone());
        // The kit's drop from slot 2 onto the first row's position is the move 0 → 1.
        let to = kit::move_target(0, 2).expect("a real move");
        ws.apply_doc(&file, "move choice", &[st::move_choice(&id, 0, to)]).expect("the move applies");
        let after = st::dialog_draft(&ws.text_of(&file).expect("the text after"), &id).expect("parses after");
        assert_eq!(after.choices[0].id, second, "the second choice is first now");
        assert_eq!(after.choices[1].id, first, "the first choice is second now");
        assert!(ws.undo().expect("undo").is_some(), "the move is an undo step");
        assert_eq!(ws.text_of(&file).expect("the text after undo"), before, "undo restores the bytes");
    }

    #[test]
    fn resetting_a_changed_unit_field_is_one_undo_step_and_undo_restores_the_bytes() {
        let (_copy, mut ws) = example();
        let (pkg, _) = ws.read_package().expect("the package reads");
        let id = "ash_knight";
        assert_eq!(rec::override_of(&pkg, Kind::Unit, id, "cost"), Some(90), "the unit has a cost override to reset");
        let (file, edit) = rec::set_edit(&pkg, Kind::Unit, id, "cost", None).expect("the reset edit");
        let before = ws.text_of(&file).expect("the unit file's text");
        ws.apply_doc(&file, "reset cost", &[edit]).expect("the reset applies");
        let doc: DocumentMut = ws.text_of(&file).expect("the text after").parse().expect("parses");
        let unit = doc["unit"].as_array_of_tables().and_then(|a| a.iter().find(|t| t.get("id").and_then(|v| v.as_str()) == Some(id))).expect("the unit's table");
        assert!(unit.get("cost").is_none(), "the override is gone: the base's value holds");
        assert!(ws.undo().expect("undo").is_some(), "the reset is an undo step");
        assert_eq!(ws.text_of(&file).expect("the text after undo"), before, "undo restores the bytes");
    }
}
