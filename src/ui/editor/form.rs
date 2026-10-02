//! A column of labelled fields for the editor's panels: lays rows out top to bottom, skips
//! drawing rows scrolled out of view, and remembers which field changed (the undo merge key).
//!
//! Labels, headings, notes and picker lists go through `i18n::tr` here, so tables of labels
//! (`palette::FACTIONS`, …) show in the interface language; callers still write their
//! literals as `tr("…")` for the catalog.

use macroquad::prelude::*;

use razdor::dt::dtm::{Scenario, Troop};
use razdor::editor::palette::{building_type_label, Names};
use razdor::i18n::tr;

use crate::ui::widgets::*;

pub const ROW: f32 = 28.0;

/// Options of a picker: (stored value, label).
pub type Options = Vec<(i64, String)>;

/// One picker of [`Form::pick_row`]: the value, its options, its share of the row.
pub type PickCell<'a> = (&'a mut i64, &'a [(i64, String)], f32);

pub struct Form {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    top: f32,
    bottom: f32,
    start: f32,
    pub label_w: f32,
    /// Key prefix: the record (`"b12"`, `"a3"`, `"settings"`), so fields of different
    /// records never share focus or an undo step.
    prefix: String,
    /// The field that changed this frame.
    pub changed: Option<String>,
}

impl Form {
    pub fn new(prefix: &str, area: Rect, scroll: f32) -> Form {
        Form {
            x: area.x,
            y: area.y - scroll,
            w: area.w,
            top: area.y,
            bottom: area.bottom(),
            start: area.y - scroll,
            label_w: (area.w * 0.42).min(170.0),
            prefix: prefix.to_string(),
            changed: None,
        }
    }

    /// Height of everything laid out so far (for the scroll limit).
    pub fn content_height(&self) -> f32 {
        self.y - self.start
    }

    fn key(&self, k: &str) -> String {
        format!("{}:{k}", self.prefix)
    }

    /// Reserves a row of height `h`; true if it is fully in view (then draw it at `self.y`
    /// before calling [`Form::next`]).
    fn shown(&self, h: f32) -> bool {
        self.y >= self.top - 0.5 && self.y + h <= self.bottom + 0.5
    }

    fn next(&mut self, h: f32) {
        self.y += h;
    }

    fn mark(&mut self, k: &str) {
        self.changed = Some(self.key(k));
    }

    pub fn heading(&mut self, s: &str) {
        if self.shown(ROW) {
            text_fit(tr(s), self.x, self.y + 20.0, self.w, 19.0, ACCENT);
            draw_line(self.x, self.y + 25.0, self.x + self.w, self.y + 25.0, 1.0, Color::new(0.5, 0.4, 0.2, 0.6));
        }
        self.next(ROW + 2.0);
    }

    pub fn note(&mut self, s: &str, color: Color) {
        for line in wrap(tr(s), self.w, 16.0) {
            if self.shown(20.0) {
                text(&line, self.x, self.y + 15.0, 16.0, color);
            }
            self.next(20.0);
        }
    }

    fn label(&self, s: &str) {
        text_fit(tr(s), self.x, self.y + 17.0, self.label_w - 6.0, 16.0, DIM);
    }

    fn field_x(&self) -> (f32, f32) {
        (self.x + self.label_w, self.w - self.label_w)
    }

    pub fn text(&mut self, k: &str, label: &str, v: &mut String) {
        if self.shown(ROW) {
            self.label(label);
            let (fx, fw) = self.field_x();
            if text_field(&self.key(k), fx, self.y, fw, 24.0, v, false) {
                self.mark(k);
            }
        }
        self.next(ROW);
    }

    /// A multi-line text of `lines` lines, full width under its label.
    pub fn memo(&mut self, k: &str, label: &str, v: &mut String, lines: usize) {
        let h = 20.0 + lines as f32 * 19.0 + 8.0;
        if self.shown(h + 4.0) {
            text_fit(tr(label), self.x, self.y + 15.0, self.w, 16.0, DIM);
            if text_field(&self.key(k), self.x, self.y + 20.0, self.w, h - 20.0, v, true) {
                self.mark(k);
            }
        }
        self.next(h + 4.0);
    }

    /// [`Form::memo`] in the text size (pixels) and boldness of the editor's options.
    pub fn memo_styled(&mut self, k: &str, label: &str, v: &mut String, lines: usize, size: f32, bold: bool) {
        let h = 20.0 + lines as f32 * (size + 2.0) + 8.0;
        if self.shown(h + 4.0) {
            text_fit(tr(label), self.x, self.y + 15.0, self.w, 16.0, DIM);
            if text_field_styled(&self.key(k), self.x, self.y + 20.0, self.w, h - 20.0, v, true, size, bold) {
                self.mark(k);
            }
        }
        self.next(h + 4.0);
    }

    pub fn num<T: Copy + Into<i64> + TryFrom<i64>>(&mut self, k: &str, label: &str, v: &mut T, min: i64, max: i64) {
        self.num_step(k, label, v, min, max, 1);
    }

    /// [`Form::num`] whose buttons move by `step` (the original's spin increment).
    pub fn num_step<T: Copy + Into<i64> + TryFrom<i64>>(&mut self, k: &str, label: &str, v: &mut T, min: i64, max: i64, step: i64) {
        if self.shown(ROW) {
            self.label(label);
            let (fx, fw) = self.field_x();
            if let Some(n) = number_field_step(&self.key(k), fx, self.y, fw.min(150.0), (*v).into(), min, max, step) {
                if let Ok(n) = T::try_from(n) {
                    *v = n;
                    self.mark(k);
                }
            }
        }
        self.next(ROW);
    }

    /// A 0/1 flag byte as a check box.
    pub fn flag(&mut self, k: &str, label: &str, v: &mut u8) {
        if self.shown(ROW) {
            if let Some(on) = checkbox(self.x, self.y + 2.0, self.w, tr(label), *v != 0) {
                *v = on as u8;
                self.mark(k);
            }
        }
        self.next(ROW);
    }

    pub fn pick<T: Copy + Into<i64> + TryFrom<i64>>(&mut self, k: &str, label: &str, v: &mut T, options: &[(i64, String)]) {
        if self.shown(ROW) {
            self.label(label);
            let (fx, fw) = self.field_x();
            if let Some(n) = dropdown(&self.key(k), fx, self.y, fw, (*v).into(), options) {
                if let Ok(n) = T::try_from(n) {
                    *v = n;
                    self.mark(k);
                }
            }
        }
        self.next(ROW);
    }

    /// A unit slot: unit picker and two numbers (level/count or start/max).
    #[allow(clippy::too_many_arguments)]
    pub fn slot(&mut self, k: &str, units: &[(i64, String)], unit: &mut u8, a: &mut u8, a_max: i64, b: &mut u8, b_max: i64) {
        if self.shown(ROW) {
            let nw = 74.0;
            let uw = self.w - 2.0 * nw - 8.0;
            if let Some(n) = dropdown(&self.key(&format!("{k}u")), self.x, self.y, uw, *unit as i64, units) {
                *unit = n as u8;
                self.mark(&format!("{k}u"));
            }
            if let Some(n) = number_field(&self.key(&format!("{k}a")), self.x + uw + 4.0, self.y, nw, *a as i64, 0, a_max) {
                *a = n as u8;
                self.mark(&format!("{k}a"));
            }
            if let Some(n) = number_field(&self.key(&format!("{k}b")), self.x + uw + nw + 8.0, self.y, nw, *b as i64, 0, b_max) {
                *b = n as u8;
                self.mark(&format!("{k}b"));
            }
        }
        self.next(ROW);
    }

    /// Column captions over [`Form::slot`] rows.
    pub fn slot_header(&mut self, unit: &str, a: &str, b: &str) {
        if self.shown(20.0) {
            let nw = 74.0;
            let uw = self.w - 2.0 * nw - 8.0;
            text_fit(tr(unit), self.x, self.y + 15.0, uw, 15.0, DIM);
            text_fit(tr(a), self.x + uw + 8.0, self.y + 15.0, nw - 4.0, 15.0, DIM);
            text_fit(tr(b), self.x + uw + nw + 12.0, self.y + 15.0, nw - 4.0, 15.0, DIM);
        }
        self.next(20.0);
    }

    /// Six troops as the original's army window edits them: levels as stored (0-based) and
    /// counts, both up to `max`; picking a unit raises a count of 0 to 1, picking none or a
    /// count of 0 clears the slot (records.md §1).
    pub fn troops_raw(&mut self, k: &str, units: &[(i64, String)], troops: &mut [Troop; 6], max: i64) {
        self.slot_header(tr("Unit"), tr("Level"), tr("Count"));
        for (i, t) in troops.iter_mut().enumerate() {
            let (mut unit, mut level, mut count) = (t.unit, t.level, t.count);
            self.slot(&format!("{k}{i}"), units, &mut unit, &mut level, max, &mut count, max);
            if (unit, level, count) != (t.unit, t.level, t.count) {
                let count = if unit != t.unit && unit != 0 && count == 0 { 1 } else { count };
                *t = if unit == 0 || count == 0 { Troop::default() } else { Troop { unit, level, count } };
            }
        }
    }

    /// A start date as the original's masked field edits it: hour, day, month (both shown
    /// 1-based, two digits) and a four-digit year, read by [`records::date_minutes`].
    pub fn date(&mut self, k: &str, minutes: &mut u32) {
        let d = razdor::dt::dtm::GameDate::from_minutes(*minutes);
        let (mut hour, mut day, mut month, mut year) = (d.hour, d.day, d.month, d.year);
        self.num(&format!("{k}h"), tr("Hour"), &mut hour, 0, 99);
        self.num(&format!("{k}d"), tr("Day"), &mut day, 0, 99);
        self.num(&format!("{k}m"), tr("Month"), &mut month, 0, 99);
        self.num(&format!("{k}y"), tr("Year"), &mut year, 0, 9999);
        if (hour, day, month, year) != (d.hour, d.day, d.month, d.year) {
            *minutes = razdor::editor::records::date_minutes(hour, day, month, year);
        }
    }

    /// Attitudes towards the four factions, −3..3.
    pub fn relations(&mut self, k: &str, v: &mut [i8; 4]) {
        for (i, name) in razdor::editor::palette::FACTIONS.iter().enumerate() {
            self.num(&format!("{k}{i}"), name, &mut v[i], -3, 3);
        }
    }

    /// A condition threshold: a >= / <= switch and its value (0 = not checked), stored as
    /// the original stores it (the sign is the switch).
    pub fn threshold(&mut self, k: &str, label: &str, v: &mut i16, max: i64) {
        use razdor::editor::events::Threshold;
        if self.shown(ROW) {
            self.label(label);
            let (fx, _) = self.field_x();
            let t = Threshold::from_raw(*v);
            if small_button(fx, self.y, 38.0, 24.0, if t.at_least { ">=" } else { "<=" }, t.value != 0) {
                *v = Threshold { at_least: !t.at_least, ..t }.raw();
                self.mark(&format!("{k}s"));
            }
            // The signed word holds 32,767 "at least" and 32,768 "at most" (records.md §9.1).
            let max = max.min(Threshold::max(t.at_least) as i64);
            if let Some(n) = number_field(&self.key(k), fx + 42.0, self.y, 110.0, t.value as i64, 0, max) {
                *v = Threshold { value: n as u16, ..t }.raw();
                self.mark(k);
            }
        }
        self.next(ROW);
    }

    /// Several pickers side by side on one row (widths in parts of the row). The values are
    /// edited in place.
    pub fn pick_row(&mut self, k: &str, cells: &mut [PickCell]) {
        if self.shown(ROW) {
            let total: f32 = cells.iter().map(|c| c.2).sum::<f32>().max(0.01);
            let gaps = 4.0 * (cells.len() as f32 - 1.0).max(0.0);
            let mut x = self.x;
            for (i, (v, options, part)) in cells.iter_mut().enumerate() {
                let w = (self.w - gaps) * *part / total;
                let key = format!("{k}{i}");
                if let Some(n) = dropdown(&self.key(&key), x, self.y, w, **v, options) {
                    **v = n;
                    self.mark(&key);
                }
                x += w + 4.0;
            }
        }
        self.next(ROW);
    }

    /// A small button on its own row; true when clicked.
    pub fn button(&mut self, label: &str, enabled: bool) -> bool {
        let mut hit = false;
        if self.shown(ROW + 2.0) {
            let label = tr(label);
            let w = (measure(label, 17.0).width + 24.0).min(self.w);
            hit = small_button(self.x, self.y, w, 26.0, label, enabled);
        }
        self.next(ROW + 2.0);
        hit
    }

    /// Row of up to four small buttons; returns the index clicked.
    pub fn buttons(&mut self, labels: &[&str]) -> Option<usize> {
        let mut hit = None;
        if self.shown(ROW + 2.0) {
            let bw = (self.w - 4.0 * (labels.len() as f32 - 1.0)) / labels.len() as f32;
            for (i, l) in labels.iter().enumerate() {
                if small_button(self.x + i as f32 * (bw + 4.0), self.y, bw, 26.0, tr(l), true) {
                    hit = Some(i);
                }
            }
        }
        self.next(ROW + 2.0);
        hit
    }

    /// An event list with a remove button per row and an "add" picker (local events);
    /// `duplicates`: the picker also offers the events already listed.
    pub fn event_list(&mut self, k: &str, used: &[u16], events: &[(i64, String)], duplicates: bool) -> EventListEdit {
        let mut out = EventListEdit::None;
        for (i, id) in used.iter().enumerate() {
            if self.shown(ROW) {
                let label = events.iter().find(|e| e.0 == *id as i64).map_or(format!("#{id} {}", tr("(missing)")), |e| e.1.clone());
                text(&ellipsize(&label, self.w - 40.0, 16.0), self.x, self.y + 17.0, 16.0, INK);
                if small_button(self.x + self.w - 26.0, self.y, 26.0, 24.0, "x", true) {
                    out = EventListEdit::Remove(i);
                }
            }
            self.next(ROW);
        }
        let mut add: i64 = 0;
        let mut options = vec![(0, tr("Add an event...").to_string())];
        options.extend(events.iter().filter(|e| duplicates || !used.contains(&(e.0 as u16))).cloned());
        if self.shown(ROW) {
            if let Some(n) = dropdown(&self.key(&format!("{k}add")), self.x, self.y, self.w, add, &options) {
                add = n;
            }
        }
        self.next(ROW);
        if add > 0 {
            out = EventListEdit::Add(add as u16);
        }
        out
    }
}

pub enum EventListEdit {
    None,
    Add(u16),
    Remove(usize),
}

// ------------------------------------------------------------------------------------------
// Picker options from the scenario and the content names
// ------------------------------------------------------------------------------------------

fn with_none(none: &str, items: impl Iterator<Item = (i64, String)>) -> Options {
    std::iter::once((0, tr(none).to_string())).chain(items).collect()
}

pub fn unit_options(n: &Names) -> Options {
    with_none(tr("(none)"), n.units.iter().map(|c| (c.id as i64, format!("{} ({})", c.name, c.id))))
}

pub fn artefact_options(n: &Names) -> Options {
    with_none(tr("(none)"), n.artefacts.iter().map(|c| (c.id as i64, format!("{} ({})", c.name, c.id))))
}

pub fn spell_options(n: &Names) -> Options {
    with_none(tr("(none)"), n.spells.iter().map(|c| (c.id as i64, format!("{} ({})", c.name, c.id))))
}

/// "#3 Title" for every event (titles come from the map).
pub fn event_options(s: &Scenario) -> Options {
    s.events
        .iter()
        .enumerate()
        .map(|(i, e)| {
            let t = e.title_text().trim();
            let kind = razdor::editor::events::kind_label(e.kind);
            (i as i64 + 1, if t.is_empty() { format!("#{} ({kind})", i + 1) } else { format!("#{} {t} ({kind})", i + 1) })
        })
        .collect()
}

pub fn event_options_none(s: &Scenario) -> Options {
    with_none(tr("(none)"), event_options(s).into_iter())
}

pub fn building_options(s: &Scenario) -> Options {
    with_none(
        tr("(none)"),
        s.buildings.iter().enumerate().map(|(i, b)| {
            let name = if b.name.trim().is_empty() { building_type_label(b.kind).to_string() } else { b.name.trim().to_string() };
            (i as i64 + 1, format!("#{} {name}", i + 1))
        }),
    )
}

/// [`building_options`] limited to buildings of the given types (the original's lists).
pub fn building_options_of(s: &Scenario, types: &[u8]) -> Options {
    let all = building_options(s);
    all.into_iter().filter(|(id, _)| *id == 0 || s.building(*id as u16).is_some_and(|b| types.contains(&b.kind))).collect()
}

pub fn army_label(s: &Scenario, id: u8) -> String {
    match s.army(id) {
        Some(a) if !a.name.trim().is_empty() => format!("#{id} {}", a.name.trim()),
        Some(a) if !a.leader_name.trim().is_empty() => format!("#{id} {}", a.leader_name.trim()),
        _ => format!("#{id} {}", tr("army")),
    }
}

pub fn named_options(s: &Scenario) -> Options {
    with_none(tr("(none)"), s.named_characters.iter().enumerate().map(|(i, n)| (i as i64 + 1, format!("{} {}", i + 1, n.name))))
}

pub fn army_options(s: &Scenario) -> Options {
    with_none(tr("(none)"), (1..=s.armies.len().min(255) as u8).map(|id| (id as i64, army_label(s, id))))
}

pub fn point_options(s: &Scenario) -> Options {
    with_none(
        tr("(none)"),
        s.points.iter().map(|p| (p.id as i64, format!("#{} {} ({}, {})", p.id, if p.model == 8 { tr("lantern") } else { tr("event point") }, p.x, p.y))),
    )
}

pub fn list_options(labels: &[&str], first: i64) -> Options {
    labels.iter().enumerate().map(|(i, l)| (first + i as i64, tr(l).to_string())).collect()
}
