//! The unit editor and the artefact editor windows (the original's TInfoUnit and
//! TInfoArtefact): a list on the left, the selected record's fields on the right. They edit
//! a draft that is stored into the session's tables (`razdor::editor::catalog`), never into
//! the map; the export writes new ini files into Razdor's editor folder.

use macroquad::prelude::*;

use razdor::dt::data::{ArtefactDef, ArtefactType, Bonus, MagicDirection, MagicSchool, Nature, Stat, UnitDef, Upgrade};
use razdor::editor::catalog::{self, PriceSkip};
use razdor::editor::palette::Names;
use razdor::i18n::{n_, tr};
use razdor::rules::content::Content;
use razdor::trf;

use super::form::*;
use crate::ui::widgets::*;

/// What the windows ask for.
pub enum CatalogAction {
    None,
    /// Replace the session's tables with this content.
    Store(Box<Content>),
    /// Export the session's tables (after storing the draft).
    Export(Box<Content>),
    Status(String),
    /// Close (after storing the draft).
    Close(Option<Box<Content>>),
}

#[derive(Default)]
pub struct CatalogState {
    selected: usize,
    list_scroll: usize,
    scroll: f32,
    /// The record being edited (stored when another is picked).
    unit: Option<UnitDef>,
    item: Option<ArtefactDef>,
    /// The automatic price's last label: units that can wear it and those that gain.
    price_note: Option<String>,
    /// The unit window's gold field, the price a store writes ([`catalog::window_gold`]).
    window_gold: i64,
}

const STATS: [(Stat, &str); 13] = [
    (Stat::Hits, n_("Hits")),
    (Stat::AttackBlow, n_("Attack (blows)")),
    (Stat::DefenceBlow, n_("Defence (blows)")),
    (Stat::AttackShot, n_("Attack (shots)")),
    (Stat::DefenceShot, n_("Defence (shots)")),
    (Stat::MagicPower, n_("Magic power")),
    (Stat::Initiative, n_("Initiative")),
    (Stat::Manevres, n_("Manoeuvres")),
    (Stat::ProtectLife, n_("Protection: life")),
    (Stat::ProtectDeath, n_("Protection: death")),
    (Stat::ProtectElemental, n_("Protection: elements")),
    (Stat::Regen, n_("Regeneration")),
    (Stat::Vampirizm, n_("Vampirism")),
];

fn school_options() -> Options {
    list_options(&[tr("(none)"), tr("Life"), tr("Elemental"), tr("Death")], 0)
}

fn school_index(m: Option<MagicSchool>) -> i64 {
    match m {
        None => 0,
        Some(MagicSchool::Life) => 1,
        Some(MagicSchool::Elemental) => 2,
        Some(MagicSchool::Death) => 3,
    }
}

fn school_of(i: i64) -> Option<MagicSchool> {
    [None, Some(MagicSchool::Life), Some(MagicSchool::Elemental), Some(MagicSchool::Death)][i.clamp(0, 3) as usize]
}

fn bonus_options() -> Options {
    let mut o: Options = vec![(0, tr("(none)").into())];
    o.extend(Bonus::known().enumerate().map(|(i, b)| (i as i64 + 1, b.token().to_string())));
    o
}

fn bonus_index(b: &Option<Bonus>) -> i64 {
    b.as_ref().and_then(|b| Bonus::known().position(|x| x == b)).map_or(0, |i| i as i64 + 1)
}

fn bonus_of(i: i64) -> Option<Bonus> {
    (i > 0).then(|| Bonus::known().nth(i as usize - 1).cloned()).flatten()
}

/// The window's frame: dims the map, draws the panel; returns it.
fn frame(title: &str) -> Rect {
    let (sw, sh) = (screen_width(), screen_height());
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.0, 0.0, 0.0, 0.55));
    let w = (sw - 40.0).clamp(300.0, 1100.0);
    let h = (sh - 60.0).max(300.0);
    let r = Rect::new((sw - w) / 2.0, 30.0, w, h);
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 0.98));
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, ACCENT);
    text(title, r.x + 14.0, r.y + 28.0, 24.0, ACCENT);
    r
}

/// A list of rows; returns the row clicked.
fn list(r: Rect, rows: &[(String, Color)], selected: usize, scroll: &mut usize) -> Option<usize> {
    let n = (r.h / 24.0).floor().max(1.0) as usize;
    if mouse_in(r.x, r.y, r.w, r.h) && !popup_open() {
        let wh = wheel();
        if wh > 0.0 {
            *scroll = scroll.saturating_sub(3);
        } else if wh < 0.0 {
            *scroll += 3;
        }
    }
    *scroll = (*scroll).min(rows.len().saturating_sub(n));
    draw_rectangle(r.x, r.y, r.w, r.h, FIELD_BG);
    let mut hit = None;
    for (i, (label, ink)) in rows.iter().enumerate().skip(*scroll).take(n) {
        let y = r.y + (i - *scroll) as f32 * 24.0;
        let hover = mouse_in(r.x, y, r.w, 23.0);
        if i == selected || hover {
            draw_rectangle(r.x, y, r.w, 23.0, if i == selected { Color::new(0.45, 0.34, 0.16, 1.0) } else { Color::new(0.25, 0.2, 0.13, 1.0) });
        }
        text(&ellipsize(label, r.w - 10.0, 16.0), r.x + 5.0, y + 17.0, 16.0, *ink);
        if hover && clicked() {
            hit = Some(i);
        }
    }
    hit
}

fn scroll_form(state: &mut CatalogState, area: Rect, content: f32) {
    if mouse_in(area.x, area.y, area.w, area.h) && !popup_open() {
        let wh = wheel();
        if wh != 0.0 {
            state.scroll -= wh * 40.0;
        }
    }
    state.scroll = state.scroll.clamp(0.0, (content - area.h + 20.0).max(0.0));
}

/// The colour of a unit's row by its nature (the original colours its list so).
fn nature_ink(n: Nature) -> Color {
    match n {
        Nature::Undead => Color::new(0.7, 0.6, 0.9, 1.0),
        Nature::Elemental => Color::new(0.5, 0.8, 0.95, 1.0),
        Nature::Rogue => Color::new(0.9, 0.55, 0.45, 1.0),
        Nature::Animal => Color::new(0.6, 0.85, 0.5, 1.0),
        Nature::Hero => ACCENT,
        _ => INK,
    }
}

/// The unit editor (records.md §5).
pub fn units_window(state: &mut CatalogState, c: &Content, names: &Names) -> CatalogAction {
    let r = frame(tr("Units"));
    let rows: Vec<(String, Color)> = c.units.iter().map(|u| (format!("{:>3} {}", u.id, u.name), nature_ink(u.nature))).collect();
    let lr = Rect::new(r.x + 10.0, r.y + 40.0, 260.0f32.min(r.w * 0.3), r.h - 100.0);
    state.selected = state.selected.min(c.units.len().saturating_sub(1));
    // Storing writes the window's gold field as the price (0x559c20).
    let store = |state: &CatalogState, d: &UnitDef| Box::new(catalog::store_unit(c, d, state.window_gold));
    let shown = |state: &CatalogState| state.unit.clone().or_else(|| c.units.get(state.selected).cloned());
    // Choosing another unit stores the shown one first, edited or not (0x55a268), so its
    // price becomes the formula's: the original's behaviour, kept (the export too).
    if let Some(i) = list(lr, &rows, state.selected, &mut state.list_scroll) {
        if i != state.selected {
            let stored = shown(state).map(|d| store(state, &d));
            state.selected = i;
            state.unit = None;
            state.scroll = 0.0;
            if let Some(s) = stored {
                return CatalogAction::Store(s);
            }
        }
    }
    let by = r.bottom() - 52.0;
    if button(r.x + 10.0, by, 150.0, 40.0, tr("Store"), true) {
        if let Some(d) = shown(state) {
            let s = store(state, &d);
            state.unit = None;
            return CatalogAction::Store(s);
        }
    }
    if button(r.x + 170.0, by, 190.0, 40.0, tr("Export the list"), true) {
        let s = shown(state).map(|d| store(state, &d)).unwrap_or_else(|| Box::new(c.clone()));
        state.unit = None;
        return CatalogAction::Export(s);
    }
    if button(r.right() - 130.0, by, 116.0, 40.0, tr("Close"), true) || (!typing() && !popup_open() && is_key_pressed(KeyCode::Escape)) {
        let s = state.unit.clone().map(|d| store(state, &d));
        state.unit = None;
        return CatalogAction::Close(s);
    }
    let Some(base) = c.units.get(state.selected) else { return CatalogAction::None };
    let mut d = state.unit.clone().unwrap_or_else(|| base.clone());
    let area = Rect::new(lr.right() + 14.0, r.y + 40.0, r.right() - lr.right() - 26.0, r.h - 100.0);
    let mut f = Form::new(&format!("unit{}", d.id), area, state.scroll);
    f.label_w = (area.w * 0.4).min(220.0);
    let cost = catalog::unit_cost(c, &d);
    // The cost routine runs after every change (0x55c860), updating the gold field.
    state.window_gold = catalog::window_gold(c, &d, state.window_gold);
    match cost.gold {
        Some(g) => f.note(&trf!("Tactical cost {t}, price {g}", t = cost.tactical, g), ACCENT),
        None => f.note(&trf!("Tactical cost {t}; the price passes 65,535 (the original stops with a range error)", t = cost.tactical), RED),
    }
    let table = catalog::level_table(c, &d);
    f.note(&trf!("Levels 1-5: {table}", table = table.iter().map(|p| format!("{p}%")).collect::<Vec<_>>().join(" ")), DIM);
    f.text("name", tr("Name"), &mut d.name);
    f.memo("descr", tr("Description"), &mut d.description, 3);
    let icons = c.units.len().max(1) as i32;
    if let Some(k) = f.buttons(&[tr("Previous icon"), tr("Next icon")]) {
        d.icon_index = (d.icon_index + if k == 1 { 1 } else { -1 }).rem_euclid(icons);
    }
    f.note(&trf!("Icon {n}", n = d.icon_index), DIM);
    let natures: Options = Nature::ALL.iter().map(|n| (n.index() as i64, format!("{n:?}"))).collect();
    let mut nature = d.nature.index() as i64;
    f.pick("nature", tr("Nature"), &mut nature, &natures);
    d.nature = Nature::ALL[nature.clamp(0, 6) as usize];
    let mut school = school_index(d.magic);
    f.pick("school", tr("Magic school"), &mut school, &school_options());
    d.magic = school_of(school);
    let dirs = list_options(&[tr("At everyone"), tr("At enemies"), tr("At allies")], 0);
    let mut dir: i64 = match d.magic_direction {
        Some(MagicDirection::ToEnemy) => 1,
        Some(MagicDirection::ToAlly) => 2,
        _ => 0,
    };
    f.pick("direction", tr("Magic direction"), &mut dir, &dirs);
    d.magic_direction = Some([MagicDirection::ToAll, MagicDirection::ToEnemy, MagicDirection::ToAlly][dir.clamp(0, 2) as usize]);
    let mut bonus = bonus_index(&d.bonus);
    f.pick("bonus", tr("Bonus"), &mut bonus, &bonus_options());
    if bonus != bonus_index(&d.bonus) {
        d.bonus = bonus_of(bonus);
    }
    f.heading(tr("Battle stats and per-level gains"));
    for (st, label) in STATS {
        let (lo, hi, step) = catalog::stat_range(st);
        let mut v = d.stat(st);
        f.num_step(&format!("s{st:?}"), label, &mut v, lo as i64, hi as i64, step as i64);
        if v != d.stat(st) {
            catalog::set_unit_stat(&mut d, st, v);
        }
        let mut gain = d.level_up.get(&st).copied().unwrap_or(0);
        f.num(&format!("d{st:?}"), tr("... per level"), &mut gain, -1000, 1000);
        if gain == 0 {
            d.level_up.remove(&st);
        } else {
            d.level_up.insert(st, gain);
        }
    }
    f.heading(tr("Cost and experience"));
    f.num("mult", tr("Cost multiplier (%)"), &mut d.cost_multiplier, 0, 10_000);
    f.num("div", tr("Gold divisor"), &mut d.cost_gold_div, 1, 9);
    f.num("surrender", tr("Surrender"), &mut d.surrender, 0, 1000);
    f.num("xp", tr("Starting experience"), &mut d.start_experience, 0, 100_000);
    f.num("lmul", tr("Level multiplier (%)"), &mut d.level_multiplier, 0, 10_000);
    f.heading(tr("Upgrades"));
    let units = unit_options(names);
    // The three NextUnit slots.
    for k in 1..=3u8 {
        let current = d.upgrades.iter().find(|u| u.slot == k).cloned();
        let mut t = current.as_ref().and_then(|u| u.target).unwrap_or(0) as i64;
        let mut lv = current.as_ref().map_or(0, |u| u.level) as i64;
        f.pick(&format!("up{k}"), &trf!("Upgrade {n}", n = k), &mut t, &units);
        f.num(&format!("upl{k}"), tr("... at level"), &mut lv, 0, 255);
        let before = current.as_ref().map(|u| (u.target.unwrap_or(0) as i64, u.level as i64)).unwrap_or((0, 0));
        if (t, lv) != before {
            d.upgrades.retain(|u| u.slot != k);
            if t > 0 {
                let name = c.try_unit(razdor::rules::content::UnitId(t as u32)).map(|u| u.name.clone()).unwrap_or_default();
                d.upgrades.push(Upgrade { target_name: name, target: Some(t as u32), level: lv as i32, slot: k });
                d.upgrades.sort_by_key(|u| u.slot);
            }
        }
    }
    let content = f.content_height();
    scroll_form(state, area, content);
    if d != *base || state.unit.is_some() {
        state.unit = Some(d);
    }
    CatalogAction::None
}

/// The artefact editor (records.md §6).
pub fn artefacts_window(state: &mut CatalogState, c: &Content) -> CatalogAction {
    let r = frame(tr("Artefacts"));
    let rows: Vec<(String, Color)> = c.items.iter().map(|a| (format!("{:>3} {}  {}", a.id, a.name, a.cost), INK)).collect();
    let lr = Rect::new(r.x + 10.0, r.y + 40.0, 300.0f32.min(r.w * 0.34), r.h - 100.0);
    state.selected = state.selected.min(c.items.len().saturating_sub(1));
    let store_draft = |state: &CatalogState| -> Option<Box<Content>> {
        let d = state.item.clone()?;
        Some(Box::new(catalog::with_items(c, catalog::store_artefact(&c.items, &d))))
    };
    if let Some(i) = list(lr, &rows, state.selected, &mut state.list_scroll) {
        if i != state.selected {
            let store = store_draft(state);
            (state.selected, state.item, state.scroll, state.price_note) = (i, None, 0.0, None);
            if let Some(s) = store {
                return CatalogAction::Store(s);
            }
        }
    }
    let by = r.bottom() - 52.0;
    let labels = [tr("Store"), tr("Copy"), tr("Delete"), tr("Price"), tr("Export the list")];
    let bw = ((r.w - 160.0) / labels.len() as f32 - 6.0).min(150.0);
    let mut hit = None;
    for (k, l) in labels.iter().enumerate() {
        if button(r.x + 10.0 + k as f32 * (bw + 6.0), by, bw, 40.0, l, true) {
            hit = Some(k);
        }
    }
    let close = button(r.right() - 130.0, by, 116.0, 40.0, tr("Close"), true) || (!typing() && !popup_open() && is_key_pressed(KeyCode::Escape));
    if close {
        let s = store_draft(state);
        state.item = None;
        return CatalogAction::Close(s);
    }
    // A copy not stored yet is the draft alone.
    let Some(base) = c.items.get(state.selected).cloned().or_else(|| state.item.clone()) else { return CatalogAction::None };
    let mut d = state.item.clone().unwrap_or(base.clone());
    match hit {
        Some(0) => {
            if let Some(s) = store_draft(state) {
                state.item = None;
                return CatalogAction::Store(s);
            }
        }
        Some(1) => {
            match catalog::copy_artefact(&c.items, &d) {
                Some(copy) => {
                    state.selected = c.items.len();
                    state.item = Some(copy);
                }
                None => return CatalogAction::Status(tr("At most 255 artefacts.").into()),
            }
            return CatalogAction::None;
        }
        Some(2) => {
            let mut items = c.items.clone();
            catalog::delete_artefact(&mut items, d.id);
            state.item = None;
            return CatalogAction::Store(Box::new(catalog::with_items(c, items)));
        }
        Some(3) => match catalog::auto_price(c, &d) {
            Ok((p, n, m)) => {
                // The original stores the artefact first, then puts the price into the
                // field (0x555bc8): the stored record keeps the old price until stored again.
                let stored = Box::new(catalog::with_items(c, catalog::store_artefact(&c.items, &d)));
                d.cost = p as i32;
                state.price_note = Some(if n == 0 { tr("No unit can wear it.").to_string() } else { trf!("Worn by {n} unit types, {pct}% of them gain", n, pct = m * 100 / n) });
                state.item = Some(d);
                return CatalogAction::Store(stored);
            }
            Err(PriceSkip::NotPriced) => state.price_note = Some(tr("Not priced: its price is 1, or it is a potion or an item.").into()),
            Err(PriceSkip::NoGain) => state.price_note = Some(tr("No unit gains from it: the original's price is undefined here (it divides by a counter it never sets).").into()),
        },
        Some(_) => {
            let s = store_draft(state).unwrap_or_else(|| Box::new(c.clone()));
            state.item = None;
            return CatalogAction::Export(s);
        }
        None => {}
    }
    let area = Rect::new(lr.right() + 14.0, r.y + 40.0, r.right() - lr.right() - 26.0, r.h - 100.0);
    let mut f = Form::new(&format!("art{}", d.id), area, state.scroll);
    f.label_w = (area.w * 0.4).min(220.0);
    if let Some(n) = &state.price_note {
        f.note(n, ACCENT);
    }
    f.text("name", tr("Name"), &mut d.name);
    f.memo("descr", tr("Description"), &mut d.description, 3);
    let digits: String = d.icon.chars().filter(|c| c.is_ascii_digit()).collect();
    let icon: i32 = digits.parse().unwrap_or(0);
    let top = c.items.iter().filter_map(|a| a.icon.chars().filter(|c| c.is_ascii_digit()).collect::<String>().parse::<i32>().ok()).max().unwrap_or(0) + 1;
    if let Some(k) = f.buttons(&[tr("Previous icon"), tr("Next icon")]) {
        d.icon = format!("A{:03}.Tga", (icon + if k == 1 { 1 } else { -1 }).rem_euclid(top.max(1)));
    }
    f.note(&trf!("Icon {name}", name = d.icon), DIM);
    f.num_step("cost", tr("Price"), &mut d.cost, -1_000_000, 1_000_000, 25);
    let types: Options = ArtefactType::ALL.iter().map(|t| (catalog::artefact_type_code(*t) as i64, format!("{t:?}"))).collect();
    let mut code = catalog::artefact_type_code(d.kind) as i64;
    f.pick("type", tr("Type"), &mut code, &types);
    d.kind = ArtefactType::ALL.into_iter().find(|t| catalog::artefact_type_code(*t) as i64 == code).unwrap_or(d.kind);
    let mut school = school_index(d.magic);
    f.pick("school", tr("Magic school"), &mut school, &school_options());
    d.magic = school_of(school);
    let mut bonus = bonus_index(&d.bonus);
    f.pick("bonus", tr("Bonus"), &mut bonus, &bonus_options());
    if bonus != bonus_index(&d.bonus) {
        d.bonus = bonus_of(bonus);
    }
    for (title, block) in [(n_("Fixed values (f-)"), 0), (n_("Additions (d-)"), 1), (n_("Percent changes (p-)"), 2)] {
        f.heading(title);
        let mods = match block {
            0 => &mut d.fixed,
            1 => &mut d.add,
            _ => &mut d.percent,
        };
        // The third block also has the protections, regeneration and vampirism.
        let n = if block == 2 { 13 } else { 8 };
        for (st, label) in STATS.iter().take(n) {
            let mut v = mods.get(st).copied().unwrap_or(0);
            f.num(&format!("{block}{st:?}"), label, &mut v, -100_000, 100_000);
            if v == 0 {
                mods.remove(st);
            } else {
                mods.insert(*st, v);
            }
        }
    }
    let content = f.content_height();
    scroll_form(state, area, content);
    if d != base || state.item.is_some() {
        state.item = Some(d);
    }
    CatalogAction::None
}
