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
use crate::ui::widgets::*;

/// The places of a market test: (artefact, price) or empty.
type MarketTest = Vec<Option<(u32, i32)>>;

/// Panel state kept between frames.
#[derive(Default)]
pub struct PanelState {
    pub tab: usize,
    pub scroll: f32,
    /// What the panel showed last (a new selection starts at the top).
    shown: Option<String>,
    /// The army window's cost figures of the army last rated.
    army_cost: Option<(Army, Option<records::ArmyCost>)>,
    /// The last market test: the building and the twelve places with their prices.
    market: Option<(u16, MarketTest)>,
}

impl PanelState {
    /// The army window's cost figures ([`records::army_cost`]), kept while the army is
    /// unchanged.
    fn cost(&mut self, a: &Army, c: &Content, recruit_div: i32) -> Option<records::ArmyCost> {
        if self.army_cost.as_ref().is_none_or(|(x, _)| x != a) {
            self.army_cost = Some((a.clone(), records::army_cost(a, c, recruit_div)));
        }
        self.army_cost.as_ref().and_then(|x| x.1)
    }

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
    /// The same content, shared (the market test builds a game on it).
    pub shared: Option<std::sync::Arc<Content>>,
}

/// The panel's frame and title; returns the area under the title and tabs for the form.
fn frame(rect: Rect, title: &str, tabs_labels: &[&str], state: &mut PanelState) -> Rect {
    draw_rectangle(rect.x, rect.y, rect.w, rect.h, Color::new(0.1, 0.095, 0.09, 0.96));
    draw_rectangle_lines(rect.x, rect.y, rect.w, rect.h, 1.0, DIM);
    text_fit(title, rect.x + 10.0, rect.y + 24.0, rect.w - 20.0, 20.0, ACCENT);
    let mut y = rect.y + 34.0;
    if !tabs_labels.is_empty() {
        let before = state.tab;
        y += tabs(rect.x + 6.0, y, rect.w - 12.0, tabs_labels, &mut state.tab);
        if state.tab != before {
            state.scroll = 0.0;
        }
    }
    Rect::new(rect.x + 10.0, y + 4.0, rect.w - 20.0, rect.bottom() - y - 50.0)
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
fn footer(rect: Rect, label: &str) -> bool {
    small_button(rect.x + 10.0, rect.bottom() - 40.0, 170.0, 30.0, label, true)
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
    // The original's pages for the type (records.md §4.1).
    let pages = records::BuildingPages::of(b.kind);
    let mut tabs: Vec<(usize, &str)> = vec![(0, tr("General"))];
    if pages.barracks || pages.garrison {
        tabs.push((1, tr("Troops")));
    }
    if pages.treasure {
        tabs.push((2, tr("Treasure")));
    }
    if pages.market || pages.library {
        tabs.push((3, tr("Trade")));
    }
    tabs.push((4, tr("Faction")));
    tabs.push((5, tr("Events")));
    state.tab = state.tab.min(tabs.len() - 1);
    let title = format!("{} #{id}  {}", tr(building_type_label(b.kind)), b.name.trim());
    let labels: Vec<&str> = tabs.iter().map(|t| t.1).collect();
    let area = frame(rect, &title, &labels, state);
    let mut f = Form::new(&format!("b{id}"), area, state.scroll);
    let n = ctx.names;
    let (w, h) = (s.width() as i64, s.height() as i64);
    match tabs[state.tab].0 {
        0 => {
            f.text("name", tr("Name"), &mut b.name);
            f.text("owner_name", tr("Neutral owner"), &mut b.owner_name);
            f.memo("description", tr("Description"), &mut b.description, 4);
            let kinds = list_options(&(1..16).map(|k| tr(building_type_label(k))).collect::<Vec<_>>(), 1);
            f.pick("kind", tr("Building type"), &mut b.kind, &kinds);
            let variants: Options = ctx
                .palette
                .pictures_of(b.picture_type)
                .map(|p| (p.variant as i64, trf!("Picture {n} ({w}x{h})", n = p.variant, w = p.size.0, h = p.size.1)))
                .collect();
            if variants.is_empty() {
                f.num("variant", tr("Picture"), &mut b.picture_variant, 0, 255);
            } else {
                f.pick("variant", tr("Picture"), &mut b.picture_variant, &variants);
            }
            f.num("picture_type", tr("Picture type"), &mut b.picture_type, 1, 14);
            f.note(&trf!("Footprint {w} x {h}, from the picture", w = b.size_x, h = b.size_y), DIM);
            f.num("x", tr("X (bottom-right)"), &mut b.x, 0, w - 1);
            f.num("y", tr("Y (bottom-right)"), &mut b.y, 0, h - 1);
            f.pick("owner", tr("Owner army"), &mut b.owner_army, &owner_options(s));
            f.heading(tr("Starts as the player's for"));
            for (k, class) in palette::HERO_CLASSES.iter().enumerate() {
                f.flag(&format!("start{k}"), tr(class), &mut b.start_for[k]);
            }
        }
        1 => {
            let units = unit_options(n);
            if pages.barracks {
                f.heading(tr("Barracks"));
                f.flag("all_types", tr("Recruits all types (bandits too)"), &mut b.recruit_all_types);
                f.slot_header(tr("Unit"), tr("At start"), tr("Most"));
                for (i, r) in b.barracks.iter_mut().enumerate() {
                    let (mut u, mut a, mut m) = (r.unit, r.start_count, r.max_count);
                    f.slot(&format!("bar{i}"), &units, &mut u, &mut a, 9, &mut m, 9);
                    if (u, a, m) != (r.unit, r.start_count, r.max_count) {
                        *r = if u == 0 { Default::default() } else { razdor::dt::dtm::RecruitSlot { unit: u, start_count: a, max_count: m } };
                    }
                }
            }
            if pages.garrison {
                f.heading(tr("Garrison"));
                f.troops_raw("gar", &units, &mut b.garrison, 9);
                let mut defence = b.garrison_extra_defence.min(records::DEFENCE_SLIDER);
                f.num("defence", tr("Garrison defence (0-50)"), &mut defence, 0, records::DEFENCE_SLIDER as i64);
                if defence != b.garrison_extra_defence.min(records::DEFENCE_SLIDER) {
                    b.garrison_extra_defence = records::defence_of_slider(records::defence_slider(defence));
                }
                f.flag("ai_only", tr("Garrison serves the AI only"), &mut b.garrison_ai_only);
                if let Some(c) = ctx.content {
                    match records::garrison_rating(&b, c) {
                        Some((t, side)) => f.note(&trf!("Tactical cost / strength: {t} / {side}", t, side), INK),
                        None => f.note(tr("More than 12 units: the garrison cannot be rated."), RED),
                    }
                }
            }
        }
        2 => {
            let arts = artefact_options(n);
            f.heading(tr("Treasure (artefacts)"));
            for k in 0..records::TREASURE {
                f.pick(&format!("goods{k}"), &trf!("Slot {n}", n = k + 1), &mut b.artifact_slots[k], &arts);
            }
            f.num_step("treasure", tr("Treasure gold"), &mut b.price_max, 0, 50_000, 50);
        }
        3 => {
            if pages.market {
                let arts = artefact_options(n);
                f.heading(tr("Goods always for sale"));
                for k in 0..records::GOODS {
                    f.pick(&format!("goods{k}"), &trf!("Slot {n}", n = k + 1), &mut b.artifact_slots[k], &arts);
                }
                f.heading(tr("Random goods"));
                f.num("random", tr("How many"), &mut b.random_artifacts_for_sale, 0, 12);
                f.num_step("price_min", tr("Lowest price"), &mut b.price_min, 0, 50_000, 500);
                f.num_step("price_max", tr("Highest price"), &mut b.price_max, 0, 50_000, 500);
                if let Some(c) = &ctx.shared {
                    if f.button(tr("Test the market"), records::market_test_ready(&b)) {
                        state.market = Some((id, records::market_test(s, id, &b, c.clone())));
                    }
                    if let Some((_, goods)) = state.market.as_ref().filter(|m| m.0 == id) {
                        for g in goods.iter().flatten() {
                            f.note(&format!("{}: {}", n.artefact(g.0), g.1), INK);
                        }
                    }
                }
            }
            if pages.library {
                f.heading(tr("Spells to learn"));
                let spells = spell_options(n);
                for k in 0..6 {
                    f.pick(&format!("spell{k}"), &trf!("Spell {n}", n = k + 1), &mut b.spells_for_sale[k], &spells);
                }
            }
        }
        4 => {
            let factions = list_options(&palette::FACTIONS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 1);
            let mut faction = b.faction;
            f.pick("faction", tr("Faction"), &mut faction, &factions);
            if faction != b.faction {
                // As the original: a new faction brings its row of attitudes.
                b.faction = faction;
                if let Some(row) = s.header.relations.get((faction as usize).wrapping_sub(1)) {
                    b.relations = *row;
                }
            }
            f.heading(tr("Attitude towards"));
            f.relations("rel", &mut b.relations);
            f.heading(tr("Gold"));
            f.num_step("gold", tr("Income per day"), &mut b.gold_per_day, 0, 250, 10);
            f.num_step("gold_max", tr("Most kept"), &mut b.gold_max, 0, 2500, 10);
            f.heading(tr("Mana"));
            f.num_step("mana", tr("Income per day"), &mut b.mana_per_day, 0, 250, 10);
            // The original's spin goes to 2,500, but the byte holds 255: more stops its save
            // with a range error.
            f.num_step("mana_max", tr("Most kept"), &mut b.mana_max, 0, 255, 10);
            let links = records::link_types(b.kind);
            if !links.is_empty() {
                f.pick("linked", tr("Linked building"), &mut b.linked_building, &building_options_of(s, links));
                f.note(tr("A village's castle; a dungeon's other end."), DIM);
            }
        }
        _ => {
            f.heading(tr("Local events"));
            f.note(tr("Local events, quests and rumours checked here, in this order; one may be listed twice (edit them with the Events button)."), DIM);
            let used = records::used_events(&b.event_slots, b.event_count).to_vec();
            match f.event_list("events", &used, &event_options(s), true) {
                EventListEdit::Add(e) => {
                    if records::add_building_event(&mut b, e, None) {
                        f.changed = Some("events".into());
                    }
                }
                EventListEdit::Remove(i) => {
                    records::remove_building_event(&mut b, i);
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
    // Saved as the original's window saves it (records.md §4.3).
    let footprint = ctx.palette.picture(b.picture_type, b.picture_variant).map(|p| p.size);
    let b = records::save_building(&b, s, footprint);
    (b != *orig).then(|| (Command::SetBuilding { id, building: Box::new(b) }, key))
}

pub fn army_panel(state: &mut PanelState, s: &Scenario, id: u8, ctx: &Ctx, rect: Rect) -> Option<(Command, String)> {
    let orig = s.army(id)?;
    let mut a: Army = orig.clone();
    state.show(&format!("a{id}"));
    // The extra leader entry hides the troop and AI pages (records.md §3.3).
    let special = a.leader_unit == records::SPECIAL_LEADER;
    let pages: Vec<usize> = if special { vec![0, 3] } else { vec![0, 1, 2, 3] };
    let labels = [tr("Leader"), tr("Troops"), tr("AI"), tr("Faction")];
    let shown: Vec<&str> = pages.iter().map(|p| labels[*p]).collect();
    state.tab = state.tab.min(shown.len() - 1);
    let area = frame(rect, &trf!("Army {army}", army = army_label(s, id)), &shown, state);
    let mut f = Form::new(&format!("a{id}"), area, state.scroll);
    let n = ctx.names;
    let (w, h) = (s.width() as i64, s.height() as i64);
    match pages[state.tab] {
        0 => {
            f.text("name", tr("Army name"), &mut a.name);
            if a.named_character == 0 {
                f.text("leader_name", tr("Leader's name"), &mut a.leader_name);
            } else {
                f.note(&trf!("Leader's name: {name} (the named character's)", name = a.leader_name), DIM);
            }
            let mut leaders = unit_options(n);
            leaders.push((records::SPECIAL_LEADER as i64, tr("(the extra empty entry)").into()));
            let mut leader = a.leader_unit;
            f.pick("leader", tr("Leader"), &mut leader, &leaders);
            if leader != a.leader_unit {
                if leader == records::SPECIAL_LEADER {
                    records::pick_special_leader(&mut a);
                } else {
                    a.leader_unit = leader;
                }
            }
            if special {
                f.note(tr("The extra leader entry: the army is inactive and patrols on the spot; what the game does with it is unknown."), DIM);
            } else {
                f.num("leader_level", tr("Leader's level (as stored, from 0)"), &mut a.leader_level, 0, 255);
                let mut named = a.named_character;
                f.pick("named", tr("Named character"), &mut named, &named_options(s));
                if named != a.named_character {
                    records::pick_named_character(&mut a, s, named);
                }
                let mut home = a.home_building;
                f.pick("home", tr("Home building"), &mut home, &building_options_of(s, &records::HOME_TYPES));
                if home != a.home_building {
                    records::pick_home(&mut a, s, home);
                }
                if let Some(b) = s.building(a.home_building as u16) {
                    let (cx, cy) = (b.x as i32 - (b.size_x as i32 - 1) / 2, b.y as i32 - (b.size_y as i32 - 1) / 2);
                    f.note(&trf!("Home at ({x}, {y})", x = cx, y = cy), DIM);
                }
            }
            f.pick("ship", tr("Ship"), &mut a.ship, &list_options(&palette::SHIPS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 0));
            f.num("x", "X", &mut a.x, 0, w - 1);
            f.num("y", "Y", &mut a.y, 0, h - 1);
            if !special {
                let mut gold = a.gold_income as i16;
                f.num_step("gold", tr("Starting gold"), &mut gold, i16::MIN as i64, i16::MAX as i64, 50);
                a.gold_income = gold as u16;
                f.heading(tr("Carries"));
                let arts = artefact_options(n);
                for k in 0..3 {
                    f.pick(&format!("art{k}"), &trf!("Artefact {n}", n = k + 1), &mut a.artifacts[k], &arts);
                }
                f.pick("spell", tr("Spell on the army"), &mut a.spell, &spell_options(n));
            }
        }
        1 => {
            f.memo("description", tr("Description"), &mut a.description, 3);
            f.heading(tr("Troops"));
            f.troops_raw("t", &unit_options(n), &mut a.troops, 255);
            if let Some(c) = ctx.content {
                f.note(&trf!("Units: {n} of {max} (the leader counts)", n = records::army_units(&a, c), max = records::ARMY_UNITS), DIM);
                match state.cost(&a, c, n.facts.recruit_div) {
                    Some(cost) => {
                        f.note(&trf!("Cost / upkeep: {gold} / {upkeep}", gold = cost.gold, upkeep = cost.upkeep), INK);
                        f.note(&trf!("Tactical cost / side strength: {t} / {side}", t = cost.tactical, side = cost.side), INK);
                    }
                    None => f.note(tr("More than 12 units: the cost cannot be computed."), RED),
                }
            }
            f.note(&trf!("Stored: {a} / {b} (recomputed when the army is saved)", a = a.tactical_cost_1, b = a.tactical_cost_2), DIM);
            f.heading(tr("Hiring and garrison"));
            let mut income = a.unknown_80 as i64 * 10;
            f.num_step("income", tr("Base gold income"), &mut income, 0, 2500, 10);
            a.unknown_80 = (income / 10) as u8;
            f.num_step("hire_xp", tr("Experience for hired units"), &mut a.hire_bonus_exp, 0, 1000, 10);
            f.flag("xp_like", tr("Hired units start like the player's"), &mut a.exp_like_player);
            f.num_step("garrison", tr("Garrison strength"), &mut a.garrison_strength, 0, 90, 5);
        }
        2 => {
            let behaviours = list_options(&palette::BEHAVIOURS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 0);
            f.pick("behaviour", tr("Behaviour"), &mut a.behaviour, &behaviours);
            f.pick("target", tr("Target choice"), &mut a.target_model, &list_options(&palette::TARGET_MODELS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 0));
            f.num_step("aggression", tr("Aggression"), &mut a.aggression, -100, 100, 10);
            f.heading(tr("Movement"));
            f.flag("inactive", tr("Inactive at the start"), &mut a.inactive);
            f.flag("patrols", tr("Patrols"), &mut a.patrols);
            f.num_step("radius", tr("Patrol radius"), &mut a.patrol_radius, 0, 250, 5);
            f.num("speed", tr("Speed correction"), &mut a.speed_correction, -3, 5);
            f.heading(tr("Targets"));
            f.flag("ignored", tr("Ignored by the AI"), &mut a.ignored_by_ai);
            f.flag("hunts", tr("Hunts only the player"), &mut a.hunts_player_only);
            f.flag("no_random", tr("No random targets"), &mut a.no_random_targets);
            f.flag("no_social", tr("Does not meet other armies"), &mut a.no_socialising);
            f.flag("no_buildings", tr("No interest in buildings"), &mut a.no_building_interest);
            f.heading(tr("Respawn and spoils"));
            f.num("respawn", tr("Respawn after (days)"), &mut a.respawn_days, 0, 30);
            f.flag("respawn_all", tr("Respawn the whole army"), &mut a.respawn_all);
            f.num_step("exp", tr("Experience correction (%)"), &mut a.exp_correction, 10, 250, 5);
            f.flag("no_money", tr("Units carry no money"), &mut a.no_money);
        }
        _ => {
            let mut faction = a.faction;
            f.pick("faction", tr("Faction"), &mut faction, &list_options(&palette::FACTIONS.iter().map(|l| tr(l)).collect::<Vec<_>>(), 1));
            if faction != a.faction {
                // As the original: a new faction brings its row of attitudes.
                records::pick_army_faction(&mut a, &s.header, faction);
            }
            if !(1..=4).contains(&a.faction) {
                f.note(tr("No faction yet: saving the army makes it the enemy."), DIM);
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
    // Saved as the original's window saves it: the 12-unit limit, then the derived bytes.
    if let Some(c) = ctx.content {
        records::limit_army(orig, &mut a, c);
    }
    let a = records::save_army(&a, ctx.content, n.facts.recruit_div);
    (a != *orig).then(|| (Command::SetArmy { id, army: Box::new(a) }, key))
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
    match f.event_list("events", &used, &event_options(s), false) {
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
