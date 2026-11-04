//! The story forms' rows (P4): one list of rows (a trigger's `when` is a single row, its `if` and
//! `then` are lists; a dialog's choices have their own `if` and `then`). A row's kind is a dropdown
//! over the vocabulary's kinds (`pack::triggers`); the arguments of the kind come from
//! `studio::story::spec`, one widget per type ([`Arg`]). A row that the format's parser refuses is
//! not taken: the error is returned and the file keeps its row.

use std::collections::BTreeMap;

use egui::Ui;
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::pack::triggers as pt;
use razdor::studio::story::{self as st, Arg, ArgSpec, Options, Row, Section};
use toml_edit::{InlineTable, Value};

use super::forms;

/// What the rows' widgets read from the package: the names the forms offer, and the texts.
pub(super) struct Cx<'a> {
    pub opts: &'a Options,
    /// The texts of the language shown.
    pub texts: &'a BTreeMap<String, String>,
}

/// What the user did to one row.
pub(super) enum RowAct {
    Keep,
    Changed(Row),
    Remove,
    Up,
    Down,
}

/// Renders one row (its kind and arguments) and returns what the user did with it. `movable` and
/// `deletable` add the row's up, down and delete buttons.
pub(super) fn row_editor(ui: &mut Ui, salt: &str, section: Section, row: &Row, cx: &Cx, movable: bool, deletable: bool) -> Result<RowAct, String> {
    let kind = section.kind_key(row).unwrap_or_default();
    let mut act = RowAct::Keep;
    let mut error: Option<String> = None;
    let mut edited = row.clone();
    let mut touched = false;
    ui.horizontal(|ui| {
        if let Some(k) = forms::choice(ui, &format!("{salt}-kind"), &kind, section.kinds()) {
            match st::new_row(section, &k, cx.opts, None) {
                Some(r) => {
                    edited = r;
                    touched = true;
                }
                None => error = Some(format!("`{k}` is not a kind")),
            }
        }
        if movable {
            if ui.small_button(tr("Up")).on_hover_text(tr("Up: runs earlier")).clicked() {
                act = RowAct::Up;
            }
            if ui.small_button(tr("Down")).on_hover_text(tr("Down: runs later")).clicked() {
                act = RowAct::Down;
            }
        }
        if deletable && ui.small_button(tr("Remove")).on_hover_text(tr("Delete this row")).clicked() {
            act = RowAct::Remove;
        }
    });
    if let Some(spec) = st::spec(section, &kind) {
        let mut row_now = if touched { edited.clone() } else { row.clone() };
        let mut row_changed = touched;
        let mut arg_error: Option<String> = None;
        egui::Grid::new(ui.id().with((salt, "args"))).num_columns(2).spacing([8.0, 4.0]).show(ui, |ui| {
            if let Some(p) = spec.positional {
                caption(ui, &kind);
                match value_widget(ui, &format!("{salt}-pos"), p, &kind, row_now.get(&kind), &row_now, section, cx) {
                    Ok(Some(v)) => {
                        row_now.insert(kind.as_str(), v);
                        row_changed = true;
                    }
                    Ok(None) => {}
                    Err(e) => arg_error = Some(e),
                }
                ui.end_row();
            }
            let mut done_groups: Vec<&str> = Vec::new();
            for a in spec.args {
                if let Some(g) = a.group {
                    if done_groups.contains(&g) {
                        continue;
                    }
                    done_groups.push(g);
                    match group_ui(ui, &format!("{salt}-{g}"), g, spec.args.iter().filter(|m| m.group == Some(g)).copied().collect(), &mut row_now, section, cx) {
                        Ok(changed) => row_changed |= changed,
                        Err(e) => arg_error = Some(e),
                    }
                    ui.end_row();
                    continue;
                }
                if a.optional {
                    let present = row_now.contains_key(a.key);
                    if let Some(on) = optional_ui(ui, &format!("{salt}-{}", a.key), a.key, present) {
                        if on {
                            row_now.insert(a.key, st::default_value(a.arg, a.key, cx.opts, None));
                        } else {
                            row_now.remove(a.key);
                        }
                        row_changed = true;
                    }
                    if row_now.contains_key(a.key) {
                        if let Err(e) = arg_row(ui, &format!("{salt}-{}", a.key), a, &mut row_now, section, cx, &mut row_changed) {
                            arg_error = Some(e);
                        }
                    }
                    ui.end_row();
                    continue;
                }
                if let Err(e) = arg_row(ui, &format!("{salt}-{}", a.key), a, &mut row_now, section, cx, &mut row_changed) {
                    arg_error = Some(e);
                }
                ui.end_row();
            }
        });
        if let Some(e) = arg_error {
            error.get_or_insert(e);
        }
        if row_changed {
            if let Some(kind_now) = section.kind_key(&row_now) {
                if kind_now == kind {
                    st::fix_garrison(section, &mut row_now, cx.opts.buildings.first().map_or("", String::as_str));
                }
            }
            match st::validate(section, &row_now) {
                Ok(()) => {
                    if !matches!(act, RowAct::Remove | RowAct::Up | RowAct::Down) {
                        act = RowAct::Changed(row_now);
                    }
                }
                Err(p) => {
                    let hint = if p.hint.is_empty() { String::new() } else { format!(" ({})", p.hint) };
                    error.get_or_insert(format!("{}{hint}", p.message));
                }
            }
        }
    }
    match error {
        Some(e) => Err(e),
        None => Ok(act),
    }
}

/// A caption: the key, in monospace.
fn caption(ui: &mut Ui, key: &str) {
    ui.weak(egui::RichText::new(key).monospace());
}

/// A group's selector (the member in the row, or none when the group may be empty) and the member's
/// widget. Picking another member removes the others and gives the new one its default.
fn group_ui(ui: &mut Ui, salt: &str, group: &str, members: Vec<ArgSpec>, row: &mut Row, section: Section, cx: &Cx) -> Result<bool, String> {
    let mut changed = false;
    let present = members.iter().find(|m| row.contains_key(m.key)).copied();
    let optional = members.iter().any(|m| m.optional);
    let mut labels: Vec<&str> = members.iter().map(|m| m.key).collect();
    if optional {
        labels.push("none");
    }
    let current = present.map_or("none", |m| m.key);
    caption(ui, group);
    if let Some(picked) = forms::choice(ui, &format!("{salt}-pick"), current, &labels) {
        for m in &members {
            row.remove(m.key);
        }
        if let Some(m) = members.iter().find(|m| m.key == picked) {
            row.insert(m.key, st::default_value(m.arg, m.key, cx.opts, None));
        }
        changed = true;
    }
    if let Some(m) = members.iter().find(|m| row.contains_key(m.key)).copied() {
        let mut inner_changed = false;
        arg_row(ui, &format!("{salt}-{}", m.key), &m, row, section, cx, &mut inner_changed)?;
        changed |= inner_changed;
    }
    let _ = present;
    Ok(changed)
}

/// An optional argument's check box (on: the argument is in the row).
fn optional_ui(ui: &mut Ui, salt: &str, key: &str, present: bool) -> Option<bool> {
    let _ = salt;
    let mut on = present;
    if ui.checkbox(&mut on, key).changed() {
        Some(on)
    } else {
        None
    }
}

/// One argument's caption and widget, applied to the row.
fn arg_row(ui: &mut Ui, salt: &str, a: &ArgSpec, row: &mut Row, section: Section, cx: &Cx, changed: &mut bool) -> Result<(), String> {
    caption(ui, a.key);
    let cur = row.get(a.key).cloned();
    let new = value_widget(ui, salt, a.arg, a.key, cur.as_ref(), row, section, cx)?;
    if let Some(v) = new {
        row.insert(a.key, v);
        *changed = true;
    }
    Ok(())
}

/// The widget of one type of value. `Ok(Some)` is the new value when the user changed it.
#[allow(clippy::too_many_arguments)]
fn value_widget(ui: &mut Ui, salt: &str, arg: Arg, key: &str, cur: Option<&Value>, row: &Row, section: Section, cx: &Cx) -> Result<Option<Value>, String> {
    let opts = cx.opts;
    let text = cur.and_then(Value::as_str).unwrap_or("").to_string();
    let pick = |ui: &mut Ui, options: Vec<String>| -> Option<Value> {
        if options.is_empty() {
            return forms::text_field(ui, salt, &text, 150.0).map(Value::from);
        }
        let refs: Vec<&str> = options.iter().map(String::as_str).collect();
        forms::choice(ui, salt, &text, &refs).map(Value::from)
    };
    Ok(match arg {
        Arg::Int(lo, hi) => {
            let n = cur.and_then(Value::as_integer).unwrap_or(0);
            forms::int_field(ui, salt, n, lo, hi).map(Value::from)
        }
        Arg::Bool => {
            let mut b = cur.and_then(Value::as_bool).unwrap_or(false);
            if ui.checkbox(&mut b, "").changed() {
                Some(Value::from(b))
            } else {
                None
            }
        }
        Arg::True => {
            ui.weak("true");
            None
        }
        Arg::Cell => {
            let c = ints(cur);
            let (x, y) = (c.first().copied().unwrap_or(0), c.get(1).copied().unwrap_or(0));
            let mut out = None;
            ui.horizontal(|ui| {
                if let Some(nx) = forms::int_field(ui, &format!("{salt}-x"), x, 0, 999) {
                    out = Some(st::cell_value(&[nx, y]));
                }
                if let Some(ny) = forms::int_field(ui, &format!("{salt}-y"), y, 0, 999) {
                    out = Some(st::cell_value(&[x, ny]));
                }
            });
            out
        }
        Arg::Place => {
            let typed = join_ints(&ints(cur));
            match forms::text_field(ui, salt, &typed, 150.0) {
                Some(s) => Some(parse_ints(&s, &[2, 4]).map(|v| st::cell_value(&v))?),
                None => None,
            }
        }
        Arg::Cells => {
            let typed = cells_text(cur);
            match forms::text_field(ui, salt, &typed, 180.0) {
                Some(s) => Some(parse_cells(&s)?),
                None => None,
            }
        }
        Arg::Var => pick(ui, opts.vars.iter().map(|(n, _)| n.clone()).collect()),
        Arg::VarValue => {
            let var = section.kind_key(row).and_then(|k| row.get(&k).and_then(Value::as_str).map(str::to_string));
            let var_is_int = var.as_deref().and_then(|v| opts.var_is_int(v)).unwrap_or(cur.is_none_or(|c| c.is_integer()));
            if var_is_int {
                let n = cur.and_then(Value::as_integer).unwrap_or(0);
                forms::int_field(ui, salt, n, -1_000_000, 1_000_000).map(Value::from)
            } else {
                let mut b = cur.and_then(Value::as_bool).unwrap_or(false);
                if ui.checkbox(&mut b, "").changed() {
                    Some(Value::from(b))
                } else {
                    None
                }
            }
        }
        Arg::Army => pick(ui, opts.armies.clone()),
        Arg::Building => pick(ui, opts.buildings.clone()),
        Arg::Dialog => pick(ui, opts.dialogs.clone()),
        Arg::Choice => {
            let dialog = row.get("dialog").and_then(Value::as_str).unwrap_or("").to_string();
            pick(ui, opts.choices_of(&dialog))
        }
        Arg::Item => pick(ui, opts.items.clone()),
        Arg::Unit => pick(ui, opts.units.clone()),
        Arg::Units => {
            let typed = cur.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(", ")).unwrap_or_default();
            match forms::text_field(ui, salt, &typed, 180.0) {
                Some(s) => {
                    let units: Vec<String> = s.split(',').map(str::trim).filter(|u| !u.is_empty()).map(str::to_string).collect();
                    Some(string_array(&units))
                }
                None => None,
            }
        }
        Arg::Spell => pick(ui, opts.spells.clone()),
        Arg::Map => pick(ui, opts.maps.clone()),
        Arg::Named => pick(ui, opts.named.clone()),
        Arg::Faction | Arg::Side => forms::choice(ui, salt, &text, &["player", "ally", "neighbour", "enemy"]).map(Value::from),
        Arg::Cmp => forms::choice(ui, salt, &text, &["==", "!=", "<", "<=", ">", ">="]).map(Value::from),
        Arg::Model => {
            let names: Vec<&str> = pt::MODELS.iter().map(|(n, _)| *n).collect();
            forms::choice(ui, salt, &text, &names).map(Value::from)
        }
        Arg::EntityKind => forms::choice(ui, salt, &text, &["army", "building"]).map(Value::from),
        Arg::Entity => {
            let of_building = row.get("of").and_then(Value::as_str) == Some("building");
            let mut options = if of_building { opts.buildings.clone() } else { opts.armies.clone() };
            if section == Section::Action && section.kind_key(row).as_deref() == Some("add_units") {
                options.push("garrison".into());
            }
            pick(ui, options)
        }
        Arg::Holder => {
            let mut options = vec!["player".to_string()];
            if key == "to" {
                options.push("pack".into());
            }
            options.extend(opts.armies.iter().cloned());
            pick(ui, options)
        }
        Arg::PosHolder => {
            let mut options = vec!["player".to_string()];
            options.extend(opts.armies.iter().cloned());
            options.push("garrison".into());
            pick(ui, options)
        }
        Arg::Figure => {
            let mut options = vec!["default".to_string()];
            options.extend(opts.units.iter().cloned());
            options.extend(pt::MODELS.iter().map(|(n, _)| format!("model {n}")));
            let shown = match cur {
                Some(Value::InlineTable(t)) => format!("model {}", t.get("model").and_then(Value::as_str).unwrap_or("")),
                _ => text.clone(),
            };
            let refs: Vec<&str> = options.iter().map(String::as_str).collect();
            forms::choice(ui, salt, &shown, &refs).map(|picked| match picked.strip_prefix("model ") {
                Some(m) => {
                    let mut t = InlineTable::new();
                    t.insert("model", Value::from(m));
                    Value::InlineTable(t)
                }
                None => Value::from(picked.as_str()),
            })
        }
        Arg::Target => {
            let mut options = vec!["hero".to_string()];
            options.extend(opts.armies.iter().cloned());
            pick(ui, options)
        }
        Arg::Text | Arg::NewArmy => forms::text_field(ui, salt, &text, 150.0).map(Value::from),
        Arg::TextKey => {
            let suggestions: Vec<String> = cx.texts.keys().cloned().collect();
            forms::text_key(ui, salt, &text, &suggestions, cx.texts).map(Value::from)
        }
    })
}

/// The integers of an array value (none for another value).
fn ints(v: Option<&Value>) -> Vec<i64> {
    v.and_then(Value::as_array).map(|a| a.iter().filter_map(Value::as_integer).collect()).unwrap_or_default()
}

fn join_ints(v: &[i64]) -> String {
    v.iter().map(i64::to_string).collect::<Vec<_>>().join(", ")
}

/// The text of a list of cells: `x, y; x, y`.
fn cells_text(cur: Option<&Value>) -> String {
    cur.and_then(Value::as_array)
        .map(|a| a.iter().map(|c| join_ints(&ints(Some(c)))).collect::<Vec<_>>().join("; "))
        .unwrap_or_default()
}

/// Integers typed as `a, b` or `a, b, c, d`; the count must be one of `counts`.
fn parse_ints(s: &str, counts: &[usize]) -> Result<Vec<i64>, String> {
    let v: Vec<i64> = s.split(',').map(str::trim).filter(|p| !p.is_empty()).map(|p| p.parse::<i64>().map_err(|_| format!("`{p}` is not a whole number"))).collect::<Result<_, _>>()?;
    if counts.contains(&v.len()) {
        Ok(v)
    } else {
        Err(format!("write {} numbers, e.g. `12, 7`", counts.iter().map(usize::to_string).collect::<Vec<_>>().join(" or ")))
    }
}

/// Cells typed as `x, y; x, y` (at least one cell).
fn parse_cells(s: &str) -> Result<Value, String> {
    let mut outer = toml_edit::Array::new();
    for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let v = parse_ints(part, &[2])?;
        outer.push(st::cell_value(&v));
    }
    Ok(Value::Array(outer))
}

/// The list of rows of a section, each row with its own widgets; the rows' buttons and the add
/// button. Returns the new list when a row changed; `Err` names what the format refuses.
pub(super) fn list_editor(ui: &mut Ui, salt: &str, section: Section, rows: &[Row], cx: &Cx) -> Result<Option<Vec<Row>>, String> {
    let mut out: Vec<Row> = rows.to_vec();
    let mut changed = false;
    let mut error: Option<String> = None;
    let last = rows.len().saturating_sub(1);
    for (i, row) in rows.iter().enumerate() {
        let row_salt = format!("{salt}-{i}");
        ui.group(|ui| match row_editor(ui, &row_salt, section, row, cx, rows.len() > 1, true) {
            Ok(RowAct::Keep) => {}
            Ok(RowAct::Changed(r)) => {
                out[i] = r;
                changed = true;
            }
            Ok(RowAct::Remove) => {
                out.remove(i);
                changed = true;
            }
            Ok(RowAct::Up) if i > 0 => {
                out.swap(i, i - 1);
                changed = true;
            }
            Ok(RowAct::Down) if i < last => {
                out.swap(i, i + 1);
                changed = true;
            }
            Ok(_) => {}
            Err(e) => error = Some(e),
        });
    }
    ui.horizontal(|ui| {
        if ui.button(tr("Add a row")).clicked() {
            let first = section.kinds()[0];
            if let Some(r) = st::new_row(section, first, cx.opts, None) {
                out.push(r);
                changed = true;
            }
        }
    });
    if let Some(e) = error {
        return Err(e);
    }
    if !changed {
        return Ok(None);
    }
    for r in &out {
        st::validate(section, r).map_err(|p| if p.hint.is_empty() { p.message } else { format!("{} ({})", p.message, p.hint) })?;
    }
    Ok(Some(out))
}

/// The text of a key in every language: one field per language. Returns the language and the new
/// text when one changed.
pub(super) fn text_cells(ui: &mut Ui, salt: &str, key: &str, texts: &BTreeMap<String, BTreeMap<String, String>>) -> Option<(String, String)> {
    let mut out = None;
    for (lang, table) in texts {
        let current = table.get(key).cloned().unwrap_or_default();
        ui.horizontal(|ui| {
            ui.weak(lang.as_str());
            // The field takes the Inspector's width (a fixed one overflows a narrow panel).
            let width = (ui.available_width() - 56.0).max(100.0);
            if let Some(v) = forms::text_field(ui, &format!("{salt}-{lang}"), &current, width) {
                out = Some((lang.clone(), v));
            }
        });
    }
    out
}

/// A TOML array of strings.
fn string_array(items: &[String]) -> Value {
    let mut a = toml_edit::Array::new();
    for s in items {
        a.push(s.as_str());
    }
    Value::Array(a)
}
