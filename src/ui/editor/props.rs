//! Property panels of the selected building, army or point. Each works on a copy of the
//! record and returns the command that stores it (merged into one undo step per field).

use macroquad::prelude::*;

use razdor::dt::dtm::{Army, Building, Point, Scenario};
use razdor::editor::palette::{self, building_type_label, Names, Palette};
use razdor::editor::records;
use razdor::editor::Command;
use razdor::i18n::tr;
use razdor::trf;
use razdor::rules::content::Content;

use super::form::*;
use crate::ui::display::button_grow;
use crate::ui::widgets::*;

/// Panel state kept between frames.
#[derive(Default)]
pub struct PanelState {
    pub tab: usize,
    pub scroll: f32,
    /// What the panel showed last (a new selection starts at the top).
    shown: Option<String>,
}

impl PanelState {
    fn show(&mut self, what: &str) {
        if self.shown.as_deref() != Some(what) {
            self.shown = Some(what.to_string());
            self.scroll = 0.0;
        }
    }
}

/// What the panels need to know about the install.
pub struct Ctx<'a> {
    pub names: &'a Names,
    pub palette: &'a Palette,
    pub content: Option<&'a Content>,
}

/// The panel's frame and title; returns the area under the title and tabs for the form. The
/// header and the footer grow with the text size (Razdor's own): the form gives them room.
fn frame(rect: Rect, title: &str, tabs_labels: &[&str], state: &mut PanelState) -> Rect {
    let g = button_grow();
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.1, 0.095, 0.09, 0.96));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, DIM);
    text_fit(title, rect.x + 10.0, rect.y + 24.0, rect.w - 20.0, 20.0, ACCENT);
    let mut y = rect.y + 10.0 + 24.0 * g;
    if !tabs_labels.is_empty() {
        let before = state.tab;
        y += tabs(rect.x + 6.0, y, rect.w - 12.0, tabs_labels, &mut state.tab);
        if state.tab != before {
            state.scroll = 0.0;
        }
    }
    Rect::new(rect.x + 10.0, y + 4.0, rect.w - 20.0, rect.bottom() - y - 20.0 - 30.0 * g)
}

/// Mouse wheel over the form scrolls it.
fn scroll(state: &mut PanelState, area: Rect, content: f32) {
    if mouse_in(area.x, area.y, area.w, area.h) && !popup_open() {
        let wh = wheel();
        if wh != 0.0 {
            state.scroll -= wh * 40.0;
        }
    }
    state.scroll = state.scroll.clamp(0.0, (content - area.h + 20.0).max(0.0));
}

/// The footer button ("Delete building" etc., `label` already translated): true when clicked.
/// Grows with the text size (Razdor's own): its box is 170 x 30 at Normal.
fn footer(rect: Rect, label: &str) -> bool {
    let g = button_grow();
    small_button(rect.x + 10.0, rect.bottom() - 10.0 - 30.0 * g, 170.0 * g, 30.0 * g, label, true)
}

fn changed<T: PartialEq>(before: &T, after: &T, form: &Form) -> Option<String> {
    (before != after).then(|| form.changed.clone().unwrap_or_else(|| "edit".into()))
}

fn owner_options(s: &Scenario) -> Options {
    let mut o = vec![(0xFF, tr("(none: the neutral owner)").to_string())];
    o.extend((1..=s.armies.len().min(254) as u8).map(|id| (id as i64, army_label(s, id))));
    o
}

pub fn building_panel(state: &mut PanelState, s: &Scenario, id: u16, ctx: &Ctx, rect: Rect) -> Option<(Command, String)> {
    let orig = s.building(id)?;
    let mut b: Building = orig.clone();
    state.show(&format!("b{id}"));
    let title = format!("{} #{id}  {}", tr(building_type_label(b.kind)), b.name.trim());
    let area = frame(rect, &title, &[tr("General"), tr("Troops"), tr("Trade"), tr("Faction"), tr("Events")], state);
    let mut f = Form::new(&format!("b{id}"), area, state.scroll);
    let n = ctx.names;
    let (w, h) = (s.width() as i64, s.height() as i64);
    match state.tab {
        0 => {
            f.text("name", tr("Name"), &mut b.name);
            f.text("owner_name", tr("Neutral owner"), &mut b.owner_name);
            f.memo("description", tr("Description"), &mut b.description, 4);
            let kinds = list_options(&(0..16).map(|k| tr(building_type_label(k))).collect::<Vec<_>>(), 0);
            f.pick("kind", tr("Building type"), &mut b.kind, &kinds);
            let variants: Options = ctx
                .palette
                .pictures_of(b.picture_type)
                .map(|p| (p.variant as i64, trf!("Picture {n} ({w}x{h})", n = p.variant, w = p.size.0, h = p.size.1)))
                .collect();
            let before = b.picture_variant;
            if variants.is_empty() {
                f.num("variant", tr("Picture"), &mut b.picture_variant, 0, 255);
            } else {
                f.pick("variant", tr("Picture"), &mut b.picture_variant, &variants);
            }
            if b.picture_variant != before {
                // A new picture brings its footprint.
                (b.size_x, b.size_y) = ctx.palette.footprint(b.picture_type, b.picture_variant);
            }
            f.num("picture_type", tr("Picture type"), &mut b.picture_type, 0, 15);
            f.num("x", tr("X (bottom-right)"), &mut b.x, 0, w - 1);
            f.num("y", tr("Y (bottom-right)"), &mut b.y, 0, h - 1);
            f.num("size_x", tr("Footprint width"), &mut b.size_x, 1, 12);
            f.num("size_y", tr("Footprint height"), &mut b.size_y, 1, 12);
            f.pick("owner", tr("Owner army"), &mut b.owner_army, &owner_options(s));
            f.pick("linked", tr("Linked building"), &mut b.linked_building, &building_options(s));
            f.note(tr("A village's castle; a dungeon's other end."), DIM);
            f.heading(tr("Starts as the player's for"));
            for (k, class) in palette::HERO_CLASSES.iter().enumerate() {
                f.flag(&format!("start{k}"), tr(class), &mut b.start_for[k]);
            }
        }
        1 => {
            let units = unit_options(n);
            f.heading(tr("Barracks"));
            f.flag("has_barracks", tr("Has barracks"), &mut b.has_barracks);
            f.flag("all_types", tr("Recruits all types (bandits too)"), &mut b.recruit_all_types);
            f.slot_header(tr("Unit"), tr("At start"), tr("Most"));
            for (i, r) in b.barracks.iter_mut().enumerate() {
                let (mut u, mut a, mut m) = (r.unit, r.start_count, r.max_count);
                f.slot(&format!("bar{i}"), &units, &mut u, &mut a, 9, &mut m, 9);
                if (u, a, m) != (r.unit, r.start_count, r.max_count) {
                    *r = if u == 0 { Default::default() } else { razdor::dt::dtm::RecruitSlot { unit: u, start_count: a, max_count: m } };
                }
            }
            f.heading(tr("Garrison"));
            f.troops("gar", &units, &mut b.garrison);
            f.num("defence", tr("Extra defence (%)"), &mut b.garrison_extra_defence, 0, 255);
            f.flag("ai_only", tr("Garrison serves the AI only"), &mut b.garrison_ai_only);
        }
        2 => {
            let arts = artefact_options(n);
            let is_ruin = b.kind == 12;
            f.heading(if is_ruin { tr("Treasure (artefacts)") } else { tr("Goods always for sale") });
            for k in 0..records::GOODS {
                let mut a = b.artifact_slots[k];
                f.pick(&format!("goods{k}"), &trf!("Slot {n}", n = k + 1), &mut a, &arts);
                if a != b.artifact_slots[k] {
                    records::set_goods(&mut b, k, a);
                }
            }
            if is_ruin {
                f.num("treasure", tr("Treasure gold"), &mut b.price_max, 0, 50_000);
            } else {
                f.heading(tr("Random goods"));
                f.num("random", tr("How many"), &mut b.random_artifacts_for_sale, 0, 12);
                f.num("price_min", tr("Lowest price"), &mut b.price_min, 0, 50_000);
                f.num("price_max", tr("Highest price"), &mut b.price_max, 0, 50_000);
            }
            f.heading(tr("Spells to learn"));
            let spells = spell_options(n);
            for k in 0..6 {
                f.pick(&format!("spell{k}"), &trf!("Spell {n}", n = k + 1), &mut b.spells_for_sale[k], &spells);
            }
        }
        3 => {
            let factions = list_options(&palette::FACTIONS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 1);
            f.pick("faction", tr("Faction"), &mut b.faction, &factions);
            if f.button(tr("Attitudes from the faction's row"), true) {
                if let Some(row) = s.header.relations.get((b.faction as usize).wrapping_sub(1)) {
                    b.relations = *row;
                    f.changed = Some("relations".into());
                }
            }
            f.heading(tr("Attitude towards"));
            f.relations("rel", &mut b.relations);
            f.heading(tr("Gold"));
            f.num("gold", tr("Income per day"), &mut b.gold_per_day, 0, 2500);
            f.num("gold_max", tr("Most kept (villages)"), &mut b.gold_max, 0, 25_000);
            f.heading(tr("Mana"));
            f.num("mana", tr("Income per day"), &mut b.mana_per_day, 0, 255);
            f.num("mana_max", tr("Most kept"), &mut b.mana_max, 0, 255);
        }
        _ => {
            f.heading(tr("Local events"));
            f.note(tr("Local events, quests and rumours checked here, in this order (edit them with the Events button)."), DIM);
            let used = records::used_events(&b.event_slots, b.event_count).to_vec();
            match f.event_list("events", &used, &event_options(s)) {
                EventListEdit::Add(e) => {
                    records::add_event(&mut b.event_slots, &mut b.event_count, e);
                    f.changed = Some("events".into());
                }
                EventListEdit::Remove(i) => {
                    records::remove_event(&mut b.event_slots, &mut b.event_count, i);
                    f.changed = Some("events".into());
                }
                EventListEdit::None => {}
            }
        }
    }
    let content_h = f.content_height();
    scroll(state, area, content_h);
    if footer(rect, tr("Delete building")) {
        return Some((Command::DeleteBuilding { id }, String::new()));
    }
    let key = changed(orig, &b, &f)?;
    Some((Command::SetBuilding { id, building: Box::new(b) }, key))
}

pub fn army_panel(state: &mut PanelState, s: &Scenario, id: u8, ctx: &Ctx, rect: Rect) -> Option<(Command, String)> {
    let orig = s.army(id)?;
    let mut a: Army = orig.clone();
    state.show(&format!("a{id}"));
    let area = frame(rect, &trf!("Army {army}", army = army_label(s, id)), &[tr("Leader"), tr("Troops"), tr("AI"), tr("Faction")], state);
    let mut f = Form::new(&format!("a{id}"), area, state.scroll);
    let n = ctx.names;
    let (w, h) = (s.width() as i64, s.height() as i64);
    match state.tab {
        0 => {
            f.text("name", tr("Army name"), &mut a.name);
            f.text("leader_name", tr("Leader's name"), &mut a.leader_name);
            let units = unit_options(n);
            f.pick("leader", tr("Leader"), &mut a.leader_unit, &units);
            let mut lv = a.leader_level + 1;
            f.num("leader_level", tr("Leader's level"), &mut lv, 1, 10);
            a.leader_level = lv - 1;
            f.pick("named", tr("Named character"), &mut a.named_character, &named_options(s));
            f.pick("home", tr("Home building"), &mut a.home_building, &building_options(s));
            let models = list_options(&palette::ARMY_MODELS.iter().map(|m| tr(m.1)).collect::<Vec<_>>(), 1);
            f.pick("model", tr("Map figure"), &mut a.model, &models);
            f.pick("ship", tr("Ship"), &mut a.ship, &list_options(&palette::SHIPS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 0));
            f.num("x", "X", &mut a.x, 0, w - 1);
            f.num("y", "Y", &mut a.y, 0, h - 1);
            f.heading(tr("Carries"));
            let arts = artefact_options(n);
            for k in 0..3 {
                f.pick(&format!("art{k}"), &trf!("Artefact {n}", n = k + 1), &mut a.artifacts[k], &arts);
            }
            f.pick("spell", tr("Spell on the army"), &mut a.spell, &spell_options(n));
        }
        1 => {
            f.memo("description", tr("Description"), &mut a.description, 3);
            f.heading(tr("Troops"));
            f.troops("t", &unit_options(n), &mut a.troops);
            if let Some(c) = ctx.content {
                f.note(&trf!("Strength (tactical cost): {n}", n = records::army_strength(&a, c)), INK);
            }
            f.note(&trf!("Stored editor values: {a} / {b}", a = a.tactical_cost_1, b = a.tactical_cost_2), DIM);
            f.num("gold", tr("Starting gold"), &mut a.gold_income, 0, 65_535);
            f.heading(tr("Hiring and garrison"));
            f.num("hire_xp", tr("Experience for hired units"), &mut a.hire_bonus_exp, 0, 65_535);
            f.flag("xp_like", tr("Hired units get the player's experience"), &mut a.exp_like_player);
            f.num("garrison", tr("Garrison strength"), &mut a.garrison_strength, 0, 255);
        }
        2 => {
            let behaviours = list_options(&palette::BEHAVIOURS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 0);
            f.pick("behaviour", tr("Behaviour"), &mut a.behaviour, &behaviours);
            f.pick("target", tr("Target choice"), &mut a.target_model, &list_options(&palette::TARGET_MODELS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 0));
            f.num("aggression", tr("Aggression"), &mut a.aggression, -128, 127);
            f.heading(tr("Movement"));
            f.flag("inactive", tr("Inactive at the start"), &mut a.inactive);
            f.flag("patrols", tr("Patrols"), &mut a.patrols);
            f.num("radius", tr("Patrol radius"), &mut a.patrol_radius, 0, 255);
            f.num("speed", tr("Speed correction"), &mut a.speed_correction, -10, 10);
            f.heading(tr("Targets"));
            f.flag("ignored", tr("Ignored by the AI"), &mut a.ignored_by_ai);
            f.flag("hunts", tr("Hunts only the player"), &mut a.hunts_player_only);
            f.flag("no_random", tr("No random targets"), &mut a.no_random_targets);
            f.flag("no_social", tr("Does not meet other armies"), &mut a.no_socialising);
            f.flag("no_buildings", tr("No interest in buildings"), &mut a.no_building_interest);
            f.heading(tr("Respawn and spoils"));
            f.num("respawn", tr("Respawn after (days)"), &mut a.respawn_days, 0, 255);
            f.flag("respawn_all", tr("Respawn the whole army"), &mut a.respawn_all);
            f.num("exp", tr("Experience correction (%)"), &mut a.exp_correction, 0, 255);
            f.flag("no_money", tr("Units carry no money"), &mut a.no_money);
        }
        _ => {
            f.pick("faction", tr("Faction"), &mut a.faction, &list_options(&palette::FACTIONS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 1));
            if f.button(tr("Attitudes from the faction's row"), true) {
                if let Some(row) = s.header.relations.get((a.faction as usize).wrapping_sub(1)) {
                    a.relations = *row;
                    f.changed = Some("relations".into());
                }
            }
            f.heading(tr("Attitude towards"));
            f.relations("rel", &mut a.relations);
        }
    }
    let content_h = f.content_height();
    scroll(state, area, content_h);
    if footer(rect, tr("Delete army")) {
        return Some((Command::DeleteArmy { id }, String::new()));
    }
    let key = changed(orig, &a, &f)?;
    Some((Command::SetArmy { id, army: Box::new(a) }, key))
}

pub fn point_panel(state: &mut PanelState, s: &Scenario, id: u8, rect: Rect) -> Option<(Command, String)> {
    let orig = s.points.get((id as usize).checked_sub(1)?)?;
    let mut p: Point = orig.clone();
    state.show(&format!("p{id}"));
    let kind = if p.model == 8 { tr("Lantern") } else { tr("Event point") };
    let area = frame(rect, &format!("{kind} #{id}"), &[], state);
    let mut f = Form::new(&format!("p{id}"), area, state.scroll);
    let mut lantern = (p.model == 8) as u8;
    f.flag("lantern", tr("Lantern (reveals the area around it)"), &mut lantern);
    p.model = if lantern != 0 { 8 } else { 9 };
    f.flag("active", tr("Active at the start"), &mut p.active);
    f.num("radius", tr("Radius at the start"), &mut p.radius, 0, 24);
    f.num("x", "X", &mut p.x, 0, s.width() as i64 - 1);
    f.num("y", "Y", &mut p.y, 0, s.height() as i64 - 1);
    f.heading(tr("Attached events"));
    f.note(tr("Up to 5, as in the original editor."), DIM);
    let used = records::used_events(&p.event_slots, p.event_count).to_vec();
    let mut slots5 = [0u16; 5];
    slots5.copy_from_slice(&p.event_slots[..5]);
    match f.event_list("events", &used, &event_options(s)) {
        EventListEdit::Add(e) => {
            let mut n = p.event_count.min(5);
            if records::add_event(&mut slots5, &mut n, e) {
                p.event_slots[..5].copy_from_slice(&slots5);
                p.event_count = n;
                f.changed = Some("events".into());
            }
        }
        EventListEdit::Remove(i) => {
            records::remove_event(&mut p.event_slots, &mut p.event_count, i);
            f.changed = Some("events".into());
        }
        EventListEdit::None => {}
    }
    let content_h = f.content_height();
    scroll(state, area, content_h);
    if footer(rect, tr("Delete point")) {
        return Some((Command::DeletePoint { id }, String::new()));
    }
    let key = changed(orig, &p, &f)?;
    Some((Command::SetPoint { id, point: Box::new(p) }, key))
}
