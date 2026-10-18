//! The building window (town, castle, fort, village, church, market, tavern …), laid out as
//! the original's (video notes §2): a title bar, a column of tabs on the left, the tab's
//! content on the right, and the building's description at the bottom.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::content::{HeroClass, ItemId, SpellDef, Stat, UnitId};
use razdor::rules::formation::{Formation, Slot};
use razdor::rules::economy::VillageOffer;
use razdor::rules::game::{Currency, Game, HireError, TradeError, Tribute, SPELL_BOOK_SIZE};
use razdor::rules::script::HallEntry;
use razdor::rules::town::{ServiceError, Tab};
use razdor::rules::units::Unit;
use razdor::rules::world::LocationKind;

use super::assets::Assets;
use super::chrome;
use super::audio::{cue, cued, Cue};
use super::dialog::{resource_icon, Dialog, Picture, Resource, MANA};
use super::item_filter;
use super::items_view::unit_stat_lines;
use super::story;
use super::unit_sheet;
use super::widgets::*;
use super::window_layout;
use super::world_view;
use super::Screen;

const PARCHMENT_INK: Color = Color::new(0.45, 0.28, 0.14, 1.0);
const BOX_INK: Color = Color::new(1.0, 0.86, 0.58, 1.0);
const SILVER: Color = Color::new(0.78, 0.78, 0.82, 1.0);
const TAB_RED: Color = Color::new(0.75, 0.18, 0.12, 1.0);
const MARKET_HEADING: Color = Color::new(156.0 / 255.0, 1.0, 156.0 / 255.0, 1.0);
const BUILDING_AND_ITEM_NAME: Color = Color::new(211.0 / 255.0, 228.0 / 255.0, 161.0 / 255.0, 1.0);
const QUEST_HEADING: Color = Color::new(189.0 / 255.0, 190.0 / 255.0, 90.0 / 255.0, 1.0);

fn quest_text_color() -> Color {
    let saturation = window_layout::visual().quest_text_saturation;
    Color::new((97.0 / 255.0 * saturation).clamp(0.0, 1.0), 0.0, 0.0, 1.0)
}

fn quest_selection_row(r: Rect) {
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.72, 0.07, 0.04, 0.34));
}

/// The quest panel's button: `[Building] TakeQuest` («Взять задание») whatever is picked,
/// quest or rumour; the original sets its caption once when it builds the window
/// (0x4d98b0) and never changes it.
fn take_quest_button(r: Rect, picked: Option<HallEntry>) -> bool {
    let enabled = picked.is_some();
    let label = own_text("Building", "TakeQuest", n_("Take quest"));
    let label = label.as_str();
    let (_, down, pressed) = button_state(r, enabled);
    chrome::quest_button(r, label, enabled, down);
    if pressed { cue(Cue::Button); }
    pressed
}

/// The market's filter field.
const FILTER_KEY: &str = "market:filter";

thread_local! {
    /// The market's filter took this frame's keys (its Esc does not close the window).
    static FILTER_KEYS: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// State of the building window between frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingView {
    pub tab: Tab,
    /// Selected row of the market or sanctuary list.
    pub pick: Option<usize>,
    pub scroll: usize,
    /// Shared quest and rumour selection across non-army building tabs.
    pub hall_pick: Option<usize>,
    hall_scroll: usize,
    /// The market shows the "sell" shop (the pack) instead of the goods.
    pub selling: bool,
    /// The garrison tab's selected unit: (in the garrison, index).
    pub garrison_sel: Option<(bool, usize)>,
    /// The garrison unit being bought back: (index, cell, price), waiting for the answer.
    pub garrison_buy: Option<(usize, Slot, i32)>,
    /// What the market's filter line holds (`ui::item_filter`).
    pub filter: String,
    /// How the market's or the sanctuary's list is sorted (Razdor's addition, [`ListSort`]).
    pub sort: ListSort,
    /// The village window took the tribute: its close plays `Item-Gold` (0x4c604a).
    pub tribute_paid: bool,
    /// The gold and mana the village paid as the hero came in: what its window shows.
    pub tribute: (i32, i32),
    /// A card animation of the hire tab under way.
    anim: Option<CardAnim>,
    /// The unit card the hire tab's left panel shows over the tabs, if any (`HirePanel`).
    hire_panel: Option<HirePanel>,
}

/// The card the hire tab paints over its column of tabs (0x492f24), until the pointer brings
/// the tabs back (0x4bc98c).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HirePanel {
    /// A hire offer: a fresh unit of the type (0x492f24 with the building's army: level 1).
    Recruit(UnitId),
    /// A unit of the hero's army (squad index).
    Squad(usize),
}

/// What the pointer entered this frame in the hire tab.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entered {
    /// A hire offer's card (0x4c6964, the barracks branch).
    Recruit(UnitId),
    /// A cell of the hero's grid, with the unit in it if any (0x4c612c).
    SquadCell(Option<usize>),
    /// A tab button's place, the Exit included (0x4bb6c8).
    Tab,
}

/// The hire tab's left panel after the pointer enters `entered`, with `selected` the hero's
/// selected unit (None: the tabs). A hire offer always shows its card (0x4c6964 → 0x492f24
/// (−building, slot)); a cell of the hero's grid brings the tabs back while nothing is
/// selected (0x4c612c → 0x4bc98c), else shows the unit under the pointer or, over an empty
/// cell, the selected one; a tab's place brings the tabs back (0x4bb6c8 → 0x4bc98c). Leaving
/// a card changes nothing (0x4c72a4, 0x4c68d4): the card stays until something else is
/// entered.
fn hire_panel_on(now: Option<HirePanel>, entered: Option<Entered>, selected: Option<usize>) -> Option<HirePanel> {
    match entered {
        None => now,
        Some(Entered::Recruit(kind)) => Some(HirePanel::Recruit(kind)),
        Some(Entered::SquadCell(on)) => selected.map(|s| HirePanel::Squad(on.unwrap_or(s))),
        Some(Entered::Tab) => None,
    }
}

/// A card animation of the hire tab (0x4b0c04, 0x4b11cc), from its start (whole ms of the
/// clock).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CardAnim {
    kind: CardAnimKind,
    t0_ms: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardAnimKind {
    /// A hired unit (squad index) slides from its recruit's portrait (index) into the army.
    Hired { unit: usize, recruit: usize },
    /// The cure over a healed or raised unit's card.
    Cured { unit: usize },
    /// A unit (squad index) of the hero's grid slides from (x, y) to its new cell, pressed
    /// there with it selected (0x4c653c → 0x4b0c04), in `ms`.
    Slid { unit: usize, from: (i32, i32), ms: i64 },
}

/// How long a hired card slides, and the cure plays over a card.
const HIRE_SLIDE_MS: i64 = 200;
const CURE_MS: i64 = 350;

fn now_ms() -> i64 {
    (get_time() * 1000.0) as i64
}

impl BuildingView {
    pub fn new(tab: Tab) -> BuildingView {
        BuildingView { tab, pick: None, scroll: 0, hall_pick: None, hall_scroll: 0, selling: false, garrison_sel: None, garrison_buy: None, filter: String::new(), sort: ListSort::Shown, tribute_paid: false, tribute: (0, 0), anim: None, hire_panel: None }
    }

    fn switch(&mut self, tab: Tab) {
        if has_focus(FILTER_KEY) {
            clear_focus();
        }
        *self = BuildingView { tribute_paid: self.tribute_paid, tribute: self.tribute, hall_pick: None, hall_scroll: 0, ..BuildingView::new(tab) };
    }

    /// The animation under way and how far it is (0..1), if any.
    fn anim_at(&self) -> Option<(CardAnimKind, f32)> {
        let a = self.anim?;
        let len = match a.kind {
            CardAnimKind::Hired { .. } => HIRE_SLIDE_MS,
            CardAnimKind::Cured { .. } => CURE_MS,
            CardAnimKind::Slid { ms, .. } => ms.max(1),
        };
        let p = (now_ms() - a.t0_ms) as f32 / len as f32;
        (p < 1.0).then_some((a.kind, p.max(0.0)))
    }
}

pub fn tab_label(t: Tab) -> &'static str {
    match t {
        Tab::MainHall => tr("Main hall"),
        Tab::Barracks => tr("Barracks"),
        Tab::Garrison => tr("Garrison"),
        Tab::Market => tr("Market"),
        Tab::Sanctuary => tr("Sanctuary"),
        Tab::Tribute => tr("Tribute"),
        Tab::Shipyard => tr("Ships"),
    }
}

fn title(game: &Game) -> String {
    let Some(l) = game.location.map(|l| &game.world.locations[l]) else { return String::new() };
    if l.name.is_empty() {
        l.kind.label().to_string()
    } else {
        l.name.clone()
    }
}

pub fn service_error(e: ServiceError) -> String {
    match e {
        ServiceError::NotHere => tr("Not offered here.").into(),
        ServiceError::CannotAfford => tr("You cannot afford it.").into(),
        ServiceError::NotWounded => tr("Not wounded.").into(),
        ServiceError::NotDead => tr("Alive and well.").into(),
        ServiceError::SquadFull => tr("Your army is full.").into(),
        ServiceError::GarrisonFull => tr("The garrison is full.").into(),
        ServiceError::Hero => tr("The hero stays with his army.").into(),
        ServiceError::AlreadyKnown => tr("Already in your book.").into(),
        ServiceError::BookFull => tr("No room in the book.").into(),
        ServiceError::Named => tr("A named hero stays with the army.").into(),
        ServiceError::Dead => tr("A fallen unit cannot be left in a garrison!").into(),
        ServiceError::Unpaid(price) => trf!("Unpaid: {price} gold to take back.", price),
        ServiceError::NoSuchUnit => tr("Nobody there.").into(),
    }
}

pub fn trade_error(e: TradeError) -> String {
    match e {
        TradeError::NoMarket => tr("There is no market here.").into(),
        TradeError::NotEnoughGold => tr("Not enough gold.").into(),
        TradeError::NoSuchItem => tr("Nothing there.").into(),
        TradeError::NotForSale => tr("A personal item: it cannot be sold.").into(),
    }
}

struct Frame {
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    /// Content pane.
    cx: f32,
    cy: f32,
    cw: f32,
}

/// The window, as the original's building window (836×600 in the reference video): the tab
/// column on the left, the content pane on the right.
fn window() -> Frame {
    let Rect { x, y, w, h } = window_layout::window();
    let win = Rect::new(x, y, w, h);
    let content = window_layout::lower_content(win);
    let top = window_layout::at(win, 256.0, 34.0, 0.0, 0.0);
    Frame { x, y, w, h, cx: content.x, cy: top.y, cw: content.w }
}

/// The original's picture of a tab (`TB-<n>_RUS`, hovered `TBo`, open `TBd`), if it has one.
fn tab_art(tab: Option<Tab>) -> Option<usize> {
    Some(match tab {
        Some(Tab::MainHall) => 1,
        Some(Tab::Barracks) => 2,
        Some(Tab::Garrison) => 3,
        Some(Tab::Market) => 4,
        Some(Tab::Sanctuary) => 5,
        None => 6,
        Some(Tab::Tribute | Tab::Shipyard) => return None,
    })
}

/// A tab's picture, as the original's building window sets them (0x4d9060): `TB-` normal,
/// `TBo` under the mouse (its enter and leave handlers 0x4bb6c8 / 0x4bb734 swap it in and
/// out), `TBd` while held down and for the open tab (its pressed picture).
fn tab_state(active: bool, hover: bool, down: bool) -> &'static str {
    if active || down {
        "TBd"
    } else if hover {
        "TBo"
    } else {
        "TB-"
    }
}

/// A parchment tab in the left column (`tab` `None` is the exit). Returns true when clicked.
fn tab_button(label: &str, tab: Option<Tab>, r: Rect, active: bool) -> bool {
    let (hover, down, pressed) = button_state(r, true);
    let state = tab_state(active, hover, down);
    let art = tab_art(tab).and_then(|n| chrome::win(&format!("{state}{n}_RUS")));
    if let Some(t) = art {
        chrome::tex(&t, r, WHITE);
    } else {
        if active {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.45, 0.4, 0.38, 0.35));
        } else if hover {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.95, 0.8, 0.25));
        }
        let (edge, width) = if active { (SILVER, 4.0) } else { (PARCHMENT_INK, 2.0) };
        draw_rectangle_lines(r.x + 6.0, r.y + 6.0, r.w - 12.0, r.h - 12.0, width, edge);
        draw_rectangle_lines(r.x + 12.0, r.y + 12.0, r.w - 24.0, r.h - 24.0, 1.0, Color { a: 0.6, ..edge });
        let size = fit_size(label, r.w - 28.0, (r.h * 0.36).round());
        let d = measure(label, size);
        let color = if active { TAB_RED } else { PARCHMENT_INK };
        text(label, r.x + (r.w - d.width) / 2.0 + 1.5, r.y + (r.h + d.offset_y) / 2.0 - 1.5, size, Color::new(1.0, 0.95, 0.85, 0.5));
        text(label, r.x + (r.w - d.width) / 2.0, r.y + (r.h + d.offset_y) / 2.0 - 3.0, size, color);
    }
    pressed
}

/// The red-brown text box with the building's description.
/// The building's description in its box, as the original's (the castle's main hall at
/// 1024×768): a marked-up text in Benguiat's plain tint at the sheet's own size, centred,
/// with no shadow, rows 19 of the original's pixels apart (the event window's text list).
fn description_box(desc: &str, x: f32, y: f32, w: f32, h: f32) {
    super::typography::Role::Name.with(|| description_box_in_face(desc, x, y, w, h))
}

fn description_box_in_face(desc: &str, x: f32, y: f32, w: f32, h: f32) {
    let k = chrome::k();
    chrome::text_box(Rect::new(x, y, w, h));
    let o = super::dt_font::whole_scale(k * 0.9375);
    let size = super::dt_font::sheet_size(super::dt_font::Face::Subtitle, o);
    let room = w - 60.0 * k;
    let rows = markup_rows(&format!("^{desc}"), room, size);
    let line = 19.0 * o;
    let top = y + (h - rows.len() as f32 * line) / 2.0 + 14.0 * o;
    for (i, row) in rows.iter().enumerate() {
        draw_markup_row(row, x + 30.0 * k, top + i as f32 * line, room, size);
    }
}

/// The main hall's picture (0x4baa30, by the building type): the town, castle and fort,
/// tavern, market and church their own; an altar the graveyard when its map picture is
/// variant 2 (the cemetery), else the ruin; ruins the ruined house for the map pictures
/// (8, 5), (12, 3) and (12, 8), else the ruin. The palace, the smithy and the dungeon have
/// none. The village's is its own window's (`S_Village`).
fn hall_picture(kind: LocationKind, (picture_type, variant): (u8, u8)) -> Option<&'static str> {
    Some(match kind {
        LocationKind::Town => "S_Town",
        LocationKind::Castle | LocationKind::Fort => "S_Castle",
        LocationKind::Tavern => "S_Tavern",
        LocationKind::Market => "S_Market",
        LocationKind::Church => "S_Church",
        LocationKind::Village => "S_Village",
        LocationKind::Altar if variant == 2 => "S_Grave",
        LocationKind::Altar => "S_Ruin",
        LocationKind::Ruins if matches!((picture_type, variant), (8, 5) | (12, 3) | (12, 8)) => "S_RuinedHouse",
        LocationKind::Ruins => "S_Ruin",
        _ => return None,
    })
}

/// Main hall: the building's picture, the rumours on offer (heard for free; a rumour's own event may cost gold) and this
/// building's quests, the description.
fn main_hall(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let k = chrome::k();
    let l = game.location?;
    let pic_h = window_layout::upper(Rect::new(f.x, f.y, f.w, f.h)).h;
    {
        let upper = window_layout::upper(Rect::new(f.x, f.y, f.w, f.h));
        let (x, y, w) = (upper.x, upper.y, upper.w);
        let loc = &game.world.locations[l];
        // The original's picture of this kind of building. A kind without one (the palace,
        // the smithy, the dungeon entrance) shows the window's own background there: the tab
        // repaints the right panel from the window's surface (0x4bb7c4) and draws no picture
        // (0x4ba854 case 0). (Razdor drew its own sky, grass and the map sprite there until a
        // player's report.) Without the install, Razdor's placeholder names the kind.
        let scene = hall_picture(loc.kind, loc.picture);
        if let Some(t) = scene.and_then(chrome::win) {
            chrome::tex(&t, Rect::new(x, y, w, pic_h), WHITE);
        } else if assets.dt.is_none() {
            text_centered(loc.kind.label(), x + w / 2.0, y + pic_h / 2.0, 40.0 * k, INK);
        }
        chrome::silver_frame(Rect::new(x, y, w, pic_h), k.max(1.0));
    }
    hall_offers(game, f, view, message, dialogs)
}

/// One silver frame encloses the quest header and paper list; a matching line divides them.
fn quest_section_frame(panel: Rect, list: Rect) {
    let bottom = list.y + list.h;
    chrome::silver_frame(Rect::new(panel.x, panel.y, panel.w, bottom - panel.y), chrome::k().max(1.0));
    draw_line(panel.x + 1.0, list.y, panel.x + panel.w - 1.0, list.y, chrome::k().max(1.0), chrome::SILVER);
}

/// The quests and rumours under the main hall's picture; they remain available from every
/// building tab that does not manage armies. With none on offer, the original hides the
/// whole quest area: its divider stays and the building's description fills the space.
fn hall_offers(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    super::typography::Role::Name.with(|| hall_offers_in_face(game, f, view, message, dialogs))
}

fn hall_offers_in_face(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let k = chrome::k();
    let entries = game.hall_entries();
    let strip = card_panel_divider(f);
    let panel = window_layout::lower_content(Rect::new(f.x, f.y, f.w, f.h));
    if entries.is_empty() {
        chrome::divider(strip);
        if let Some(l) = game.location {
            let desc = &game.world.locations[l].description;
            description_box(desc, panel.x, panel.y, panel.w, panel.h);
        }
        return None;
    }

    view.hall_pick = view.hall_pick.filter(|&n| n < entries.len());
    chrome::divider(strip);
    let (quest_border, take) = window_layout::quest_rects(panel);
    let title = tr("Quests and rumours:");
    if let Some(t) = chrome::quest_border_texture() {
        chrome::tex(&t, quest_border, WHITE);
    } else {
        draw_rectangle(quest_border.x, quest_border.y, quest_border.w, quest_border.h, Color::new(0.0, 0.0, 0.0, 0.3));
        chrome::silver_frame(quest_border, 1.0);
    }
    let title_area = Rect::new(quest_border.x + 62.0*k, quest_border.y, quest_border.w - 70.0*k, quest_border.h);
    let title_size = fit_size(title, title_area.w, 20.0 * k);
    chrome::shadow_centered(&ellipsize(title, title_area.w, title_size), title_area.center().x, title_area.y + (title_area.h + title_size * 0.36) / 2.0, title_size, QUEST_HEADING);

    // The list fills the rest of the lower panel, its entries from the top, a picked one lit
    // across the whole row (0x4bedec); the building's description is not shown under it.
    let row_h = 20.0 * k;
    let list_y = (quest_border.y + quest_border.h).max(take.y + take.h) + 5.0*k;
    let list = Rect::new(panel.x, list_y, panel.w, (panel.y + panel.h - list_y).max(row_h + 8.0 * k));
    let rows = (((list.h - 8.0 * k) / row_h).floor() as usize).max(1);
    chrome::quest_list_parchment(list);
    let max_scroll = entries.len().saturating_sub(rows);
    if mouse_in(list.x, list.y, list.w, list.h) {
        let wh = wheel();
        if wh < 0.0 { view.hall_scroll = (view.hall_scroll + 1).min(max_scroll); }
        if wh > 0.0 { view.hall_scroll = view.hall_scroll.saturating_sub(1); }
    }
    view.hall_scroll = view.hall_scroll.min(max_scroll);
    // The bar is always there, idle while the list fits, as the original's (0x4bedec).
    scrollbar_always(Rect::new(list.x + list.w - 14.0 * k, list.y + 2.0 * k, 12.0 * k, list.h - 4.0 * k), &mut view.hall_scroll, rows, entries.len());
    let row_w = list.w - 16.0 * k;
    // From the top however few there are: the list widget lays its lines down from 0 (0x47e46c).
    let first_y = list.y + 4.0 * k;
    for (n, entry) in entries.iter().enumerate().skip(view.hall_scroll).take(rows) {
        let y = first_y + (n - view.hall_scroll) as f32 * row_h;
        let id = match *entry { HallEntry::Rumour(id) | HallEntry::Quest(id) => id };
        if view.hall_pick == Some(n) {
            quest_selection_row(Rect::new(list.x, y, row_w, row_h));
        }
        let name = story::event_title(game, id);
        let size = 16.0 * k;
        let shown = ellipsize(&name, row_w - 20.0 * k, size);
        text_centered(&shown, list.x + row_w / 2.0, y + row_h / 2.0 + size * 0.36, size, quest_text_color());
        if mouse_in(list.x, y, row_w, row_h) && clicked() { view.hall_pick = Some(n); }
    }

    let mut next = None;
    let picked = view.hall_pick.and_then(|n| entries.get(n).copied());
    if take_quest_button(take, picked) {
        if let Some(HallEntry::Rumour(id) | HallEntry::Quest(id)) = picked {
            match game.take_hall_entry(id) {
                Ok(events) => {
                    view.hall_pick = None;
                    *message = None;
                    next = world_view::handle_events(game, events, message, dialogs);
                }
                Err(e) => *message = Some(service_error(e)),
            }
        }
    }
    quest_section_frame(panel, list);
    next
}

/// Barracks: recruits for hire along the top, the counters, the army with heal and
/// resurrect buttons below.
/// A point of the building window in pixels of the 960×720 video (the window at 60, 20).
fn at(f: &Frame, x: f32, y: f32, w: f32, h: f32) -> Rect {
    window_layout::at(Rect::new(f.x, f.y, f.w, f.h), x, y, w, h)
}

// Both tabs place the hero's formation inside the same framed lower panel.
const CARD_PANEL_X: f32 = 248.0;
const CARD_PANEL_W: f32 = 586.0;
const CARD_BOTTOM_Y: f32 = 334.0;

fn card_grid_left(f: &Frame, form: Formation, cs: f32, pitch_x: f32) -> f32 {
    let grid = at(f, CARD_PANEL_X, 0.0, CARD_PANEL_W, 0.0);
    window_layout::army_grid_left(grid, form.display_cols(), pitch_x, 8.0 * cs * chrome::k())
}

fn card_panel_divider(f: &Frame) -> Rect {
    window_layout::divider(Rect::new(f.x, f.y, f.w, f.h))
}

fn card_panel_lower(f: &Frame) -> Rect {
    window_layout::lower(Rect::new(f.x, f.y, f.w, f.h))
}

/// The left panel's unit card: the hire tab's (`HirePanel`), else the selected unit's.
fn selected_unit_panel(game: &Game, assets: &Assets, f: &Frame, view: &BuildingView) {
    match view.hire_panel.filter(|_| view.tab == Tab::Barracks) {
        Some(HirePanel::Recruit(kind)) => recruit_panel(game, assets, f, kind),
        Some(HirePanel::Squad(i)) => unit_panel(game, assets, f, (false, i)),
        None => {
            if let Some(sel) = view.garrison_sel {
                unit_panel(game, assets, f, sel);
            }
        }
    }
}

/// A hire offer's card (0x492f24 with the building's army): a fresh unit of the type at
/// level 1, unhurt, with no items, no state lines and the type's recruit wage, as the
/// promotion tree's class card (`items_view::class_wage`).
/// Original's quirk, not copied: the offer's path skips setting the building-defence and
/// in-building flags (0x492f98 → 0x493106 jumps past 0x4930ab / 0x4930ff), so they keep the
/// last card's values; Razdor shows the offer with neither.
fn recruit_panel(game: &Game, assets: &Assets, f: &Frame, kind: UnitId) {
    let c = &game.content;
    if c.try_unit(kind).is_none() {
        return;
    }
    let mut unit = Unit::new(c, kind, Slot::new(razdor::rules::formation::Row::Front, 0));
    unit.heal_full(c);
    let stats = unit.stats(c);
    let name = c.unit(kind).name.clone();
    let sheet = unit_sheet::Sheet {
        kind,
        name: &name,
        named: false,
        level: unit.level,
        xp: unit.xp,
        need: unit.xp_to_next(c),
        hp: unit.hp,
        now: &stats,
        start: &stats,
        power: stats[Stat::MagicPower],
        wage: super::items_view::class_wage(c, kind),
        items: unit.items,
        back_row: false,
        unpaid: false,
        building: 0,
        hero: None,
        status: Vec::new(),
    };
    let mut hover = None;
    unit_sheet::draw(assets, c, window_layout::left(Rect::new(f.x, f.y, f.w, f.h)), &sheet, false, &mut hover);
}

/// The card of a unit of the hero's army or of the garrison (`guard`), index `i`.
fn unit_panel(game: &Game, assets: &Assets, f: &Frame, (guard, i): (bool, usize)) {
    let unit = if guard {
        game.garrison_here().get(i).map(|s| &s.unit)
    } else {
        game.squad.get(i)
    };
    let Some(unit) = unit else { return };
    let c = &game.content;
    // The panel shows the building's defence with the unit's own, "15 + 20" (492f24, as the
    // cards add army +0x378c: 0x49462c).
    let bd = if guard { game.garrison_defence_here() } else { game.hero_building_defence() };
    let mut stats = unit.stats(c);
    stats[Stat::DefenceBlow] += bd;
    stats[Stat::DefenceShot] += bd;
    let name = if guard { unit.name(c).to_string() } else { game.squad_label(i) };
    let hero = (!guard && i == 0).then(|| HeroClass::ALL.into_iter().find(|h| h.unit() == unit.def)).flatten();
    let sheet = unit_sheet::Sheet {
        kind: unit.def,
        name: &name,
        named: unit.named > 0,
        level: unit.level,
        xp: unit.xp,
        need: unit.xp_to_next(c),
        hp: unit.hp,
        now: &stats,
        start: &stats,
        power: stats[Stat::MagicPower],
        wage: if guard { 0 } else { game.wage(i) },
        items: unit.items,
        back_row: unit.slot.row == razdor::rules::formation::Row::Back,
        unpaid: unit.unpaid,
        building: bd,
        hero,
        status: super::items_view::potion_status(c, unit),
    };
    let mut hover = None;
    unit_sheet::draw(assets, c, window_layout::left(Rect::new(f.x, f.y, f.w, f.h)), &sheet, false, &mut hover);
}

/// The left edge of recruit card `i` of `n` (window pixels): the cards and the `n + 1` gaps
/// share the picture area evenly, as the original's hire tab (0x4bd3a4: gap = (area − card·n)
/// div (n + 1)).
fn recruit_x(i: usize, n: usize) -> f32 {
    let (area_x, area_w, card) = (CARD_PANEL_X, CARD_PANEL_W, 88.0);
    let gap = ((area_w - card * n as f32) / (n as f32 + 1.0)).trunc();
    area_x + gap + i as f32 * (card + gap)
}

/// The barracks' counters, as the original's row under the recruits: money, the army's
/// wages and the income, each with its picture.
fn barracks_counters(game: &Game, r: Rect) {
    let k = chrome::k();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.06, 0.03, 0.55));
    chrome::silver_frame(r, 1.0);
    let mut wages = format!("- {}", game.daily_wages());
    if game.daily_mana_wages() > 0 {
        wages += &format!(" / {}", trf!("{mana} mana", mana = game.daily_mana_wages()));
    }
    let cells = [
        (Resource::Gold, own_text("Army", "Gold", n_("Money")), format!("{}", game.gold)),
        (Resource::Wages, own_text("Army", "Payment", n_("Army wages")), wages),
        (Resource::Income, own_text("Army", "Incom", n_("Income")), format!("+ {}", game.daily_income())),
    ];
    let step = r.w / cells.len() as f32;
    for (i, (res, label, value)) in cells.iter().enumerate() {
        let cx = r.x + step * i as f32;
        if i > 0 {
            draw_line(cx, r.y + 3.0 * k, cx, r.y + r.h - 3.0 * k, 1.0, Color::new(0.6, 0.55, 0.45, 0.6));
        }
        resource_icon(*res, cx + 34.0 * k, r.y + r.h / 2.0, 40.0 * k);
        super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
            let size = fit_size(label, step - 66.0 * k, 14.0 * k);
            chrome::shadow_text(&ellipsize(label, step - 66.0 * k, size), cx + 62.0 * k, r.y + 19.0 * k, size, Color::new(0.85, 0.9, 0.45, 1.0));
            chrome::shadow_text(value, cx + 62.0 * k, r.y + 39.0 * k, 16.0 * k, chrome::CREAM);
        });
    }
}

/// A text of the install (`[<section>] <key>`) in Russian, else ours.
fn own_text(section: &str, key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text(section, key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The strip's line over a hire offer (0x4c6964): `[Army]` line 8 with the type's name and
/// the `HireArmy` caption, each in quotes (#NAME1, #BUTTON1); with none left, line 10 in the
/// warning colour (ae24f8). The flag says warning.
fn recruit_hint(name: &str, stock_left: bool) -> (String, bool) {
    let ru = razdor::i18n::lang() == razdor::i18n::Lang::Ru;
    if !stock_left {
        let t = chrome::ui_line("Army", 10).filter(|_| ru).unwrap_or_else(|| tr("No units of this type are left for hire!").to_string());
        return (t, true);
    }
    let button = own_text("Army", "HireArmy", n_("Hire"));
    let t = chrome::ui_line("Army", 8).filter(|_| ru).map_or_else(
        || trf!("Click the \"{button}\" button to hire \"{name}\" into your army", button, name),
        |t| t.replace("#BUTTON1", &format!("\"{button}\"")).replace("#NAME1", &format!("\"{name}\"")),
    );
    (t, false)
}

/// The barracks, as the original's (the video, 19:52): the recruits in front of the
/// building's sepia interior, each portrait with "Нанять" and "Цена = N"; the money, wages
/// and income; the army's 2×6 cards below, each with "Лечить" / "Воскресить" and its price
/// (else the card's stat strip). Sizes follow the window.
fn barracks(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let l = game.location?;
    let k = chrome::k();
    let c = game.content.clone();
    let recruits = game.world.locations[l].recruits.clone();
    let hires = game.world.locations[l].hires(&c);
    // The interior behind the recruits and the counters.
    let back = window_layout::upper(Rect::new(f.x, f.y, f.w, f.h));
    let interior = match game.world.locations[l].kind {
        LocationKind::Town | LocationKind::Palace => Some("BI_Town"),
        LocationKind::Castle | LocationKind::Fort => Some("BI_Castle"),
        LocationKind::Church => Some("BI_Church"),
        LocationKind::Ruins => Some("BI_Ruin"),
        _ => None,
    };
    match interior.and_then(chrome::win) {
        Some(t) => {
            let src_h = t.width() * back.h / back.w;
            chrome::tex_src(&t, Rect::new(0.0, (t.height() - src_h).max(0.0) / 2.0, t.width(), src_h.min(t.height())), back, WHITE);
        }
        None => draw_rectangle(back.x, back.y, back.w, back.h, Color::new(0.3, 0.2, 0.12, 1.0)),
    }
    chrome::silver_frame(back, 1.0);
    let mut hover_lines = Vec::new();
    // What the pointer is over this frame, for the left panel (`hire_panel_on`).
    let mut entered = None;
    let mut offer_hint = None;
    if recruits.is_empty() {
        chrome::shadow_centered(tr("No recruits here."), back.center().x, back.y + 90.0 * k, 18.0 * k, chrome::CREAM);
    }
    let shown = recruits.len().min(6);
    for (i, r) in recruits.iter().take(6).enumerate() {
        let face = at(f, recruit_x(i, shown), 74.0, 88.0, 88.0);
        // A hire offer is a card of the type (49462c, the hire view's template): its shadow
        // takes the card's height under the face.
        chrome::unit_shadow(Rect::new(face.x, face.y, face.w, face.w * 128.0 / 88.0));
        assets.draw_portrait(r.unit, Team::Player, face);
        draw_rectangle_lines(face.x, face.y, face.w, face.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.9));
        let stock_left = r.stock != Some(0);
        // An offer under the pointer (0x4c6964): its card on the left panel, the green
        // highlight pulsing on its portrait (state 2 drawn with the fifth glow set: 0x4cd94c
        // passes set 4 for the building's side of the hire tab, 0x4cda47), and the hint
        // line on the strip. No tooltip.
        if mouse_in(face.x, face.y, face.w, face.h) {
            entered = Some(Entered::Recruit(r.unit));
            chrome::card_highlight(face, chrome::Lit::Current, now_ms() as u64);
            offer_hint = Some(recruit_hint(&c.unit(r.unit).name, stock_left));
        }
        let price = game.hire_price(r.unit);
        let can = hires && stock_left && game.can_afford(price) && game.squad.len() < game.max_squad();
        let pill = Rect::new(face.x + 4.0 * k, face.y + face.h + 2.0 * k, face.w - 8.0 * k, 17.0 * k);
        // The hire pill now has a transparent black matte. Keep its card backing and the
        // price on parchment, instead of letting the scene show through the button area.
        chrome::surface(Rect::new(face.x, face.y + face.h, face.w, 36.0 * k), chrome::Skin::Strip);
        let hire = own_text("Army", "HireArmy", n_("Hire"));
        if chrome::pill_button(pill, &hire, can, true) {
            // The original plays the gold on the press and again in the click action on the
            // release (0x4c7370, 0x4c7380): the one buffer restarts.
            super::audio::cue_on_release(Cue::Gold);
            let name = c.unit(r.unit).name.clone();
            *message = Some(match game.hire(r.unit) {
                Ok(()) => {
                    // The new card slides from the recruit into the army (0x4b0c04).
                    cue(Cue::CardMove);
                    view.anim = Some(CardAnim { kind: CardAnimKind::Hired { unit: game.squad.len() - 1, recruit: i }, t0_ms: now_ms() });
                    trf!("{name} joins your army.", name)
                }
                Err(HireError::NotEnoughGold) => tr("You cannot afford it.").into(),
                Err(HireError::SquadFull) => tr("Your army is full.").into(),
                Err(HireError::NotOffered) => tr("Not offered here.").into(),
            });
        }
        let cost = trf!("Price {price}", price = price.amount);
        let color = if price.currency == Currency::Mana { MANA } else { chrome::GOLD };
        chrome::shadow_centered(&cost, face.center().x, pill.y + pill.h + 13.0 * k, 12.0 * k, color);
    }
    barracks_counters(game, at(f, 256.0, 242.0, 572.0, 48.0));
    let strip = card_panel_divider(f);
    chrome::army_divider(strip);
    if let Some((hint, warn)) = offer_hint {
        chrome::hint_text(strip, &hint, if warn { chrome::RED_TEXT } else { Color::new(0.4, 1.0, 0.8, 1.0) });
    }

    // The army: the army screen's cards, a heal or raise button with its price on the strip.
    let heals = game.heals_here();
    let raises = game.resurrects_here();
    let form = c.formation;
    let lines = form.display_lines() as f32;
    let pane = card_panel_lower(f);
    let cols = form.display_cols() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / cols)
        .min((pane.w - 16.0*k) / ((cols*96.0-8.0)*k))
        .min((pane.h - 16.0*k) / (((lines-1.0)*133.0+128.0)*k));
    let (card, pitch) = (vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k));
    let grid = at(f, CARD_PANEL_X, CARD_BOTTOM_Y, CARD_PANEL_W, 0.0);
    let gx = card_grid_left(f, form, cs, pitch.x);
    let cell_at = |slot: Slot| {
        let (line, col) = form.display(slot);
        vec2(gx + col as f32 * pitch.x, grid.y + line as f32 * pitch.y).round()
    };
    // A press on a card or an empty cell of the hero's grid (0x4c653c), acted on below.
    let mut pressed = None;
    let anim = view.anim_at();
    // The cell a card slides into (moved or hired) stays drawn empty until the card lands.
    let landing = match anim {
        Some((CardAnimKind::Slid { unit, .. } | CardAnimKind::Hired { unit, .. }, _)) => game.squad.get(unit).map(|u| u.slot),
        _ => None,
    };
    for slot in form.slots() {
        let taken = game.squad.iter().any(|u| u.slot == slot);
        if !taken || landing == Some(slot) {
            let p = cell_at(slot);
            chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(form, slot), true);
            if !taken && mouse_in(p.x, p.y, card.x, card.y) {
                entered = Some(Entered::SquadCell(None));
                if clicked() {
                    pressed = Some((None, slot));
                }
            }
        }
    }
    let mut action = None;
    let selected = view.garrison_sel.filter(|&(g, i)| !g && i < game.squad.len()).map(|s| s.1);
    for i in 0..game.squad.len() {
        let u = game.squad[i].clone();
        let mut p = cell_at(u.slot);
        if let Some((CardAnimKind::Hired { unit, recruit }, t)) = anim {
            if unit == i {
                let from = at(f, recruit_x(recruit, recruits.len().min(6)), 74.0, 88.0, 88.0);
                p = vec2(from.x, from.y).lerp(p, t).round();
            }
        }
        if let Some((CardAnimKind::Slid { unit, from, .. }, t)) = anim {
            if unit == i {
                p = vec2(from.0 as f32, from.1 as f32).lerp(p, t).round();
            }
        }
        let sq = Rect::new(p.x, p.y, card.x, card.x);
        chrome::unit_shadow(Rect::new(p.x, p.y, card.x, card.y));
        assets.draw_portrait(u.def, Team::Player, sq);
        chrome::wounds(sq, u.hp, u.max_hp(&c));
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let strip = Rect::new(p.x, p.y + card.x, card.x, card.y - card.x);
        let vs = u.stats(&c);
        let (label, price, raise) = match (game.heal_price(i).filter(|_| heals), game.resurrect_price(i).filter(|_| raises)) {
            (Some(pr), _) => (Some(own_text("Army", "CureArmy", n_("Heal"))), Some(pr), false),
            (None, Some(pr)) => (Some(own_text("Army", "ResurrectArmy", n_("Raise"))), Some(pr), true),
            _ => (None, None, false),
        };
        match (label, price) {
            (Some(label), Some(pr)) => {
                chrome::surface(strip, chrome::Skin::Strip);
                let pill = Rect::new(strip.x + 4.0 * k, strip.y + 3.0 * k, strip.w - 8.0 * k, 17.0 * k);
                if chrome::pill_button(pill, &label, game.can_pay_service(pr), false) {
                    action = Some((i, raise));
                }
                let cost = trf!("Price {price}", price = pr.amount);
                chrome::shadow_centered(&cost, strip.center().x, pill.y + pill.h + 13.0 * k, 12.0 * k, chrome::GOLD);
            }
            _ => super::unit_sheet::stat_strip(strip, &vs, &vs, vs[razdor::rules::content::Stat::MagicPower], super::unit_sheet::caster(&c, u.def), super::unit_sheet::strip_place(form, u.slot), u.hp, back_row_def(&c, u.slot), game.hero_building_defence(), false, super::unit_sheet::StripPanel::of_squad(i, u.named)),
        }
        if !u.alive() {
            chrome::death_veil(sq);
        } else if u.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        top_left_signs(&c, sq, &u, i > 0);
        super::spell_badges::draw(sq, &u.spells, u.drain, game.clock.total_minutes() as u64, &c);
        if super::unit_drag::dragged() == Some(i) {
            draw_rectangle(p.x, p.y, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.55));
        }
        if let Some((CardAnimKind::Cured { unit }, t)) = anim {
            if unit == i {
                // `[BattleEffects] Effect4`, its colours and blend as in battle (0x4b11cc).
                super::battle_view::draw_effect(&c, 4, sq, t);
            }
        }
        // The hero's grid in the hire tab (0x4cd94c): a selected card (state 5) pulses
        // green, the one under the pointer (state 2) blue, both with glow set state − 1.
        let over = mouse_in(sq.x, sq.y, sq.w, sq.h);
        if selected == Some(i) {
            chrome::card_highlight(sq, chrome::Lit::Current, now_ms() as u64);
        } else if over {
            chrome::card_highlight(sq, chrome::Lit::CanAct, now_ms() as u64);
        }
        if over {
            entered = Some(Entered::SquadCell(Some(i)));
            if clicked() {
                pressed = Some((Some(i), u.slot));
            }
            hover_lines = vec![(game.squad_label(i), ACCENT)];
            hover_lines.extend(unit_stat_lines(&c, &u, game.wage(i)).into_iter().map(|s| (s, INK)));
        }
    }
    // The press, as the original's hero grid within the hero's army (0x4c653c); none while a
    // card moves (busy 0x68dc63). With «Расстановка армии: Перетаскивание» (Razdor's default,
    // the owner's decision) it only selects and units move by dragging; the original's second
    // press swaps or slides.
    if let (Some((on, slot)), None) = (pressed, anim) {
        let press = super::unit_drag::grid_press_in(super::unit_drag::controls(), selected, on);
        view.garrison_sel = match press {
            super::unit_drag::GridPress::Select(i) => Some((false, i)),
            // Dragging only: an empty cell clears this grid's selection.
            super::unit_drag::GridPress::Nothing => view.garrison_sel.filter(|_| selected.is_none()),
            _ => None,
        };
        match press {
            super::unit_drag::GridPress::Swap { selected: s, pressed: p } => {
                cue(Cue::CardMove);
                game.move_unit(s, game.squad[p].slot);
            }
            super::unit_drag::GridPress::Slide(s) => {
                cue(Cue::CardMove);
                let from = cell_at(game.squad[s].slot);
                let ms = super::unit_drag::slide_ms(from, cell_at(slot), k);
                game.move_unit(s, slot);
                view.anim = Some(CardAnim { kind: CardAnimKind::Slid { unit: s, from: (from.x as i32, from.y as i32), ms }, t0_ms: now_ms() });
            }
            // Razdor's drag starts from a press that does not swap.
            _ => {
                if let Some(i) = on {
                    super::unit_drag::press(i, game.squad[i].def);
                }
            }
        }
    }
    let selected_now = view.garrison_sel.filter(|&(g, i)| !g && i < game.squad.len()).map(|s| s.1);
    view.hire_panel = hire_panel_on(view.hire_panel, entered, selected_now);
    let cells: Vec<(Slot, Rect)> = form.slots().map(|s| (s, Rect::new(cell_at(s).x, cell_at(s).y, card.x, card.y))).collect();
    if let Some((unit, slot)) = super::unit_drag::update(assets, &cells, card) {
        game.move_unit(unit, slot);
        view.garrison_sel = None;
    }
    let mut next = None;
    if let Some((i, raise)) = action {
        let name = game.squad[i].name(&c).to_string();
        let r = if raise { game.resurrect(i) } else { game.heal(i) };
        match r {
            Ok(events) => {
                // The cure over the card, with its sound (0x4b11cc, `Battle-Cure`).
                cue(Cue::Cure);
                view.anim = Some(CardAnim { kind: CardAnimKind::Cured { unit: i }, t0_ms: now_ms() });
                *message = Some(if raise { trf!("{name} rises again.", name) } else { trf!("{name} is healed.", name) });
                // What happened meanwhile: a noon report, the scenario's events.
                next = world_view::handle_events(game, events, message, dialogs);
            }
            Err(e) => *message = Some(service_error(e)),
        }
    }
    tooltip(&hover_lines);
    next
}

/// A 2×6 grid of the army screen's cards (portrait and stat strip) at `rel_y` of the building
/// Where the pointer is over a card grid: a unit's card, or an empty cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hit {
    Unit(usize),
    Cell(Slot),
}

/// The ranged defence the back row adds on a card's strip (`Row2Def`; 0 elsewhere).
pub fn back_row_def(c: &razdor::rules::content::Content, slot: Slot) -> i32 {
    if slot.row == razdor::rules::formation::Row::Back {
        c.options.row2_def
    } else {
        0
    }
}

/// The signs from a card's top left (493a64): the promotion for a unit that can take one
/// (`promotable`: of the hero's army, not the hero, or of a garrison), then a drunk potion.
fn top_left_signs(c: &razdor::rules::content::Content, sq: Rect, u: &Unit, promotable: bool) {
    let upgrade = promotable && u.upgrade_tree(c).iter().any(|&(_, _, ok)| ok);
    chrome::card_signs(sq, true, &[(upgrade, "Sign-Upgrade", GREEN), (!u.potions.is_empty(), "sign-potion", GREEN)]);
}

/// The line on screen, from the top, of display line `line` (0 the front): the hero's grid
/// has its front on top, the garrison's grid above it is mirrored so the two fronts face each
/// other (0x4d9060: the garrison's card k at line 1 − k div 6, the hero's at k div 6).
fn screen_line(form: Formation, line: usize, own: bool) -> usize {
    if own {
        line
    } else {
        form.display_lines() - 1 - line
    }
}

/// The army screen's cards for `units` in the formation, `rel_y` below the content's top;
/// `own`: the hero's army (else a garrison, mirrored: [`screen_line`]). `selected` is framed.
/// Returns what the pointer is over.
/// The scale, card size and pitch of the card grid at `rel_y` (screen px).
fn card_grid_size(f: &Frame, form: Formation, rel_y: f32) -> (f32, Vec2, Vec2) {
    let k = chrome::k();
    let lines = form.display_lines() as f32;
    let win = Rect::new(f.x, f.y, f.w, f.h);
    let pane = if rel_y < 300.0 { window_layout::upper(win) } else { window_layout::lower(win) };
    let cols = form.display_cols() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / cols)
        .min((pane.w - 16.0*k) / ((cols*96.0-8.0)*k))
        .min((pane.h - 16.0*k) / (((lines-1.0)*133.0+128.0)*k));
    (cs, vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k))
}

fn card_grid(game: &Game, assets: &Assets, f: &Frame, rel_y: f32, units: &[&Unit], own: bool, selected: Option<usize>) -> Option<Hit> {
    let k = chrome::k();
    let c = &game.content;
    let form = c.formation;
    let (cs, card, pitch) = card_grid_size(f, form, rel_y);
    let grid = at(f, CARD_PANEL_X, rel_y, CARD_PANEL_W, 0.0);
    let gx = card_grid_left(f, form, cs, pitch.x);
    let cell_at = |slot: Slot| {
        let (line, col) = form.display(slot);
        vec2(gx + col as f32 * pitch.x, grid.y + screen_line(form, line, own) as f32 * pitch.y).round()
    };
    let mut hovered = None;
    for slot in form.slots() {
        if !units.iter().any(|u| u.slot == slot) {
            let p = cell_at(slot);
            chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(form, slot), true);
            if mouse_in(p.x, p.y, card.x, card.y) {
                hovered = Some(Hit::Cell(slot));
            }
        }
    }
    for (i, u) in units.iter().enumerate() {
        let p = cell_at(u.slot);
        let sq = Rect::new(p.x, p.y, card.x, card.x);
        chrome::unit_shadow(Rect::new(p.x, p.y, card.x, card.y));
        assets.draw_portrait(u.def, Team::Player, sq);
        chrome::wounds(sq, u.hp, u.max_hp(c));
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let vs = u.stats(c);
        let bd = if own { game.hero_building_defence() } else { game.garrison_defence_here() };
        super::unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[razdor::rules::content::Stat::MagicPower], super::unit_sheet::caster(c, u.def), super::unit_sheet::strip_place(form, u.slot), u.hp, back_row_def(c, u.slot), bd, false, if own { super::unit_sheet::StripPanel::of_squad(i, u.named) } else { super::unit_sheet::StripPanel::Plain });
        if !u.alive() {
            chrome::death_veil(sq);
        } else if u.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        // The hero's army but its hero, and every garrison unit (493a64: army 0 or a
        // building's, < 0, outside a battle).
        top_left_signs(c, sq, u, !own || i > 0);
        super::spell_badges::draw(sq, &u.spells, u.drain, game.clock.total_minutes() as u64, c);
        // A card dragged (Razdor's) is dimmed where it stands.
        if super::unit_drag::dragged_from() == Some((!own, i)) {
            draw_rectangle(p.x, p.y, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.55));
        }
        if selected == Some(i) {
            chrome::glow_frame(sq, Color::new(1.0, 0.85, 0.3, 0.95), false);
        }
        if mouse_in(p.x, p.y, card.x, card.y) {
            chrome::glow_frame(sq, Color::new(0.35, 0.55, 1.0, 0.9), false);
            hovered = Some(Hit::Unit(i));
        }
    }
    hovered
}

/// Garrison, as the original's (0x4c653c, 0x4c6f50): the troops left here on top, the
/// hero's army below. One click selects a unit, a second acts: a unit of the other grid
/// swaps the two, an empty cell of the other grid moves it across (buying back an unpaid
/// guard asks first), the same grid changes its cell. Razdor's: a card dragged onto a card or
/// a cell does the same; with «Расстановка армии: Перетаскивание» (the default) a click only
/// selects ([`garrison_click`]).
fn garrison(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) {
    let k = chrome::k();
    let c = game.content.clone();
    let guards: Vec<_> = game.garrison_here().to_vec();
    let sel = view.garrison_sel.filter(|&(g, n)| if g { n < guards.len() } else { n < game.squad.len() });
    let mut hover = Vec::new();
    let guard_units: Vec<&Unit> = guards.iter().map(|s| &s.unit).collect();
    chrome::silver_frame(window_layout::upper(Rect::new(f.x, f.y, f.w, f.h)), k.max(1.0));
    let top = card_grid(game, assets, f, 35.0, &guard_units, false, sel.filter(|s| s.0).map(|s| s.1));
    if let Some(Hit::Unit(j)) = top {
        let u = &guards[j].unit;
        let lv = level_label(u.level, u.xp, u.xp_to_next(&c));
        let paid = if u.unpaid { tr("unpaid") } else { tr("paid") };
        hover = vec![(u.name(&c).to_string(), ACCENT), (lv, XP_COLOR), (trf!("{hp}/{max} HP, {paid}", hp = u.hp.max(0), max = u.max_hp(&c), paid), INK)];
        hover.push((trf!("Units on guard draw no wage and heal {pct}% a day.", pct = c.options.garrison_auto_heal), DIM));
    }
    let squad = game.squad.clone();
    let army: Vec<&Unit> = squad.iter().collect();
    let bottom = card_grid(game, assets, f, CARD_BOTTOM_Y, &army, true, sel.filter(|s| !s.0).map(|s| s.1));
    if let Some(Hit::Unit(i)) = bottom {
        let u = &squad[i];
        let lv = level_label(u.level, u.xp, u.xp_to_next(&c));
        hover = vec![(game.squad_label(i), ACCENT), (lv, XP_COLOR), (trf!("{hp}/{max} HP, wage {wage}", hp = u.hp.max(0), max = u.max_hp(&c), wage = game.wage(i)), INK)];
    }
    // The hero and named characters never go into a garrison: with one selected the
    // garrison's grid says so (0x4c6964, `[Army]` line 9); with a garrison unit selected the
    // hero or a named character under the pointer cannot change places with it (0x4c612c,
    // line 11).
    // Razdor's rule: a fallen unit of the army does not go in either (the original takes it);
    // its line is Razdor's own (no `[Army]` line, hence 0).
    // With «Расстановка армии: Перетаскивание» (Razdor's default) a selection does nothing
    // more, so the refusals speak only while a card is dragged; the original's selection
    // acts on the next press.
    let controls = super::unit_drag::controls();
    let acting = super::unit_drag::dragged_from().or(sel.filter(|_| controls == super::unit_drag::Controls::Original));
    let unique = |i: usize| i == 0 || squad.get(i).is_some_and(|u| u.named > 0);
    let fallen = |i: usize| i > 0 && squad.get(i).is_some_and(|u| u.named == 0 && !u.alive());
    let refusal = match (acting, top, bottom) {
        (Some((false, n)), Some(_), _) if unique(n) => Some((9, n_("A unique character cannot be moved into a garrison!"))),
        (Some((true, _)), _, Some(Hit::Unit(i))) if unique(i) => Some((11, n_("You cannot change places with the selected character!"))),
        (Some((false, n)), Some(_), _) if fallen(n) => Some((0, n_("A fallen unit cannot be left in a garrison!"))),
        (Some((true, _)), _, Some(Hit::Unit(i))) if fallen(i) => Some((0, n_("A fallen unit cannot be left in a garrison!"))),
        _ => None,
    };
    let pointed = match (top, bottom) {
        (Some(Hit::Unit(j)), _) => Some((true, j)),
        (_, Some(Hit::Unit(i))) => Some((false, i)),
        _ => None,
    };
    let unit_name = |(guard, i): (bool, usize)| {
        if guard { guards[i].unit.name(&c).to_string() } else { game.squad_label(i) }
    };
    // The refusal and the army window's hint lines (0x4c2f54) go on the strip, not the tooltip.
    let hint = if let Some((line, ours)) = refusal {
        Some(chrome::ui_line("Army", line).filter(|_| line > 0 && razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string()))
    } else if let Some(pointed) = pointed {
        let (line, named) = super::items_view::pointed_line(controls, pointed, sel);
        super::items_view::army_line(line, &unit_name(named))
    } else {
        top.or(bottom).and_then(|hit| match hit { Hit::Cell(slot) => super::items_view::row_text(c.formation, slot), _ => None })
    };
    let strip = card_panel_divider(f);
    chrome::army_divider(strip);
    if let Some(hint) = hint {
        chrome::hint_text(strip, &hint, if refusal.is_some() { chrome::RED_TEXT } else { Color::new(0.4, 1.0, 0.8, 1.0) });
    }
    if let Some((j, cell, price)) = view.garrison_buy {
        // The purchase question (the garrison move event, 0x4acff4).
        let r = at(f, 250.0, 290.0, 584.0, 44.0);
        draw_rectangle(r.x, r.y, r.w, r.h, PANEL);
        let name = guards.get(j).map_or_else(String::new, |s| s.unit.name(&c).to_string());
        text(&trf!("Pay {price} gold to take {name} back?", price, name), r.x + 10.0 * k, r.y + 28.0 * k, 18.0 * k, ACCENT);
        if button(r.x + r.w - 190.0 * k, r.y + 6.0 * k, 85.0 * k, 32.0 * k, tr("Yes"), true) {
            view.garrison_buy = None;
            *message = Some(match game.take_from_garrison(j, Some(cell), true) {
                Ok(()) => cued(Cue::CardMove, tr("Back in your army.").to_string()),
                Err(e) => service_error(e),
            });
        } else if button(r.x + r.w - 95.0 * k, r.y + 6.0 * k, 85.0 * k, 32.0 * k, tr("No"), true) {
            view.garrison_buy = None;
        }
        tooltip(&hover);
        return;
    }
    // Razdor's drag (both settings): a card let go over a card or a cell does what the
    // original's second press does with that card selected.
    let dropped = super::unit_drag::follow(assets, card_grid_size(f, c.formation, CARD_BOTTOM_Y).1);
    if let Some(from) = dropped {
        view.garrison_sel = Some(from);
        garrison_act(game, view, &guards, &squad, Some(from), top, bottom, message);
    } else if clicked() {
        match garrison_click(controls, top, bottom) {
            GarrisonClick::Select(s) => {
                view.garrison_sel = Some(s);
                start_garrison_drag(&guards, &squad, s);
            }
            GarrisonClick::Clear => view.garrison_sel = None,
            GarrisonClick::Act => {
                garrison_act(game, view, &guards, &squad, sel, top, bottom, message);
                // A unit selected by this press may be dragged from here (Razdor's).
                if let (None, Some(s)) = (sel, view.garrison_sel) {
                    start_garrison_drag(&guards, &squad, s);
                }
            }
            GarrisonClick::Nothing => {}
        }
    }
    tooltip(&hover);
}

/// What a press on the garrison tab does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GarrisonClick {
    /// Dragging only: the unit pressed (a garrison unit, its index) is selected.
    Select((bool, usize)),
    /// Dragging only: an empty cell clears the selection.
    Clear,
    /// As in the original: the press selects, swaps or moves ([`garrison_act`]).
    Act,
    Nothing,
}

/// The press on the garrison tab over `top` (the garrison's grid) or `bottom` (the hero's).
/// With «Расстановка армии: Перетаскивание» (Razdor's default, the owner's decision) a press
/// only selects and units move by dragging; the original's second press swaps or moves
/// (0x4c653c, 0x4c6f50).
fn garrison_click(controls: super::unit_drag::Controls, top: Option<Hit>, bottom: Option<Hit>) -> GarrisonClick {
    if controls == super::unit_drag::Controls::Original {
        return GarrisonClick::Act;
    }
    match (top, bottom) {
        (Some(Hit::Unit(j)), _) => GarrisonClick::Select((true, j)),
        (_, Some(Hit::Unit(i))) => GarrisonClick::Select((false, i)),
        (Some(Hit::Cell(_)), _) | (_, Some(Hit::Cell(_))) => GarrisonClick::Clear,
        (None, None) => GarrisonClick::Nothing,
    }
}

/// Razdor's drag from the garrison tab's card `s` (a garrison unit, its index).
fn start_garrison_drag(guards: &[razdor::rules::world::Stationed], squad: &[Unit], s: (bool, usize)) {
    match s {
        (true, j) => super::unit_drag::press_guard(j, guards[j].unit.def),
        (false, i) => super::unit_drag::press(i, squad[i].def),
    }
}

/// The original's second press on the garrison tab with `sel` selected (0x4c653c, 0x4c6f50):
/// selects with none selected, else swaps or moves within one army or across. Razdor's drag
/// ends here as well.
#[allow(clippy::too_many_arguments)]
fn garrison_act(game: &mut Game, view: &mut BuildingView, guards: &[razdor::rules::world::Stationed], squad: &[Unit], sel: Option<(bool, usize)>, top: Option<Hit>, bottom: Option<Hit>, message: &mut Option<String>) {
    let mut done = true;
    let result: Option<Result<(), ServiceError>> = match (sel, top, bottom) {
        (Some((true, j)), Some(Hit::Unit(n)), _) if n == j => None,
        (Some((false, i)), _, Some(Hit::Unit(n))) if n == i => None,
        (None, Some(Hit::Unit(j)), _) => {
            view.garrison_sel = Some((true, j));
            done = false;
            None
        }
        (None, _, Some(Hit::Unit(i))) => {
            view.garrison_sel = Some((false, i));
            done = false;
            None
        }
        // Within one army: a swap or a slide, both with `Card-Move` (0x4c7029, 0x4c65f8;
        // the slide's own at 0x4b0d19).
        (Some((true, j)), Some(Hit::Unit(n)), _) => {
            game.move_guard(j, guards[n].unit.slot);
            cue(Cue::CardMove);
            None
        }
        (Some((true, j)), Some(Hit::Cell(cell)), _) => {
            game.move_guard(j, cell);
            cue(Cue::CardMove);
            None
        }
        (Some((false, i)), _, Some(Hit::Unit(n))) => {
            game.move_unit(i, squad[n].slot);
            cue(Cue::CardMove);
            None
        }
        (Some((false, i)), _, Some(Hit::Cell(cell))) => {
            game.move_unit(i, cell);
            cue(Cue::CardMove);
            None
        }
        (Some((false, i)), Some(Hit::Unit(j)), _) => Some(game.swap_with_garrison(i, j, false)),
        (Some((true, j)), _, Some(Hit::Unit(i))) => Some(game.swap_with_garrison(i, j, true)),
        (Some((false, i)), Some(Hit::Cell(cell)), _) => Some(game.leave_in_garrison(i, Some(cell))),
        (Some((true, j)), _, Some(Hit::Cell(cell))) => match game.take_from_garrison(j, Some(cell), false) {
            Err(ServiceError::Unpaid(price)) if price < game.gold => {
                view.garrison_buy = Some((j, cell, price));
                None
            }
            Err(ServiceError::Unpaid(_)) => Some(Err(ServiceError::CannotAfford)),
            r => Some(r),
        },
        _ => {
            done = false;
            None
        }
    };
    // The hero and named units are refused without a word and the selection stays, as in
    // the original (0x4c667e, 0x4c70d6, 0x4c7233 jump past the deselection); a fallen one
    // (Razdor's rule) the same way, the strip saying why.
    let refused = matches!(result, Some(Err(ServiceError::Hero | ServiceError::Named | ServiceError::Dead)));
    if done && !refused {
        view.garrison_sel = None;
    }
    match result {
        // A move across plays `Card-Move` too (the swap's 0x4c66a0 / 0x4c70fe, the slide's).
        Some(Ok(())) => cue(Cue::CardMove),
        Some(Err(e)) if !refused => *message = Some(service_error(e)),
        _ => {}
    }
}

/// A list with a selection and a scroll bar. Rows are (icon item, name, price, the price in
/// red); `marks` light a filter's match in the names. Returns the clicked row.
#[allow(clippy::too_many_arguments)]
fn price_list(assets: Option<&Assets>, rows: &[(Option<ItemId>, String, String, bool)], marks: &[Option<std::ops::Range<usize>>], pick: Option<usize>, scroll: &mut usize, x: f32, y: f32, w: f32, visible: usize) -> Option<usize> {
    super::typography::Role::Name.with(|| price_list_in_face(assets, rows, marks, pick, scroll, x, y, w, visible))
}

/// The market list's columns as the original lays them out (0x4d9df3..0x4d9eaa): the icon
/// column 27 px, the names 283 − 70 − 27 = 186 px, the prices 70 px, then the scroll bar.
const MARKET_ICON_W: f32 = 27.0;
const MARKET_NAME_W: f32 = 186.0;
const MARKET_PRICE_W: f32 = 70.0;

/// A list's text inset (+0xb9 = 8, set at 0x4d9e50, 0x4d9f02, 0x4da04d, 0x4da0ff).
const LIST_MARGIN: f32 = 8.0;

/// What the market's lists take off the window's marble under them: their fill (24, 24, 24)
/// drawn in mode 5 (+0xa9 and +0xad, 0x4d9e33 and 0x4d9ee5), subtracted.
const MARKET_LIST_TAKEN: [i32; 3] = [24, 24, 24];

/// What a picked market row adds: the text list's own highlight (64, 64, 64) in its adding
/// mode 4 (0x47c848; the market does not change it).
const MARKET_PICK_ADDED: [i32; 3] = [64, 64, 64];

/// Where a list's columns stand from its left edge, scaled from the original's widths
/// (`widths`: the icon column, the names, the prices) to the room left of the scroll bar.
#[derive(Clone, Copy, Debug, PartialEq)]
struct ListColumns {
    /// The icon column's width (the names start there).
    icon: f32,
    /// Where a name starts: the list's margin in from the icon column.
    name_x: f32,
    /// The middle of the names' list, where its title is centred.
    names_cx: f32,
    /// The middle of the prices' list: the prices and their title are centred on it.
    price_cx: f32,
}

fn list_columns(w: f32, bar: f32, [icon, names, prices]: [f32; 3]) -> ListColumns {
    let s = (w - bar).max(0.0) / (icon + names + prices);
    ListColumns { icon: icon * s, name_x: (icon + LIST_MARGIN) * s, names_cx: (icon + names / 2.0) * s, price_cx: (icon + names + prices / 2.0) * s }
}

/// The market list's columns (`w` wide, its scroll bar `bar` at the right).
fn market_columns(w: f32, bar: f32) -> ListColumns {
    list_columns(w, bar, [MARKET_ICON_W, MARKET_NAME_W, MARKET_PRICE_W])
}

/// The tints of a market row (0x4bca8c): a buy price above the gold puts the name and the
/// price in font ae24a4 (Benguiat 255, 175, 85), any other row in ae24a0 (255, 255, 155).
fn market_ink(red: bool) -> Color {
    if red { benguiat(ORANGE_INK) } else { benguiat(YELLOW_INK) }
}

/// `Benguiat` with blue − 100 (font ae24a0), the lists' plain rows.
const YELLOW_INK: [u8; 3] = [255, 255, 155];
/// `Benguiat` with green − 80, blue − 170 (font ae24a4), a price above the gold.
const ORANGE_INK: [u8; 3] = [255, 175, 85];
/// `Benguiat` with red − 200, green − 75 (font ae24a8), a spell already in the book.
const BLUE_INK: [u8; 3] = [55, 180, 255];

/// A blend colour (r, g, b) as drawn.
fn blend_color(c: [i32; 3]) -> Color {
    let [r, g, b] = c.map(|v| v.clamp(0, 255) as u8);
    Color::from_rgba(r, g, b, 255)
}

/// The market's list as the original draws it (0x4bca8c, 0x4bddf0): the icon column is
/// Win-paper from (0, 0) with (24, 24, 24) taken off (0x4bddf0), the names and prices stand
/// on the window's marble with (24, 24, 24) taken off, a 1 px frame round both (0x48d1d4 at
/// 0x4bcfa4); the names left, the prices centred in their column (align 2, 0x4bcd03); the
/// picked row lighter by (64, 64, 64) over the names and prices, not the icons.
#[allow(clippy::too_many_arguments)]
fn price_list_in_face(assets: Option<&Assets>, rows: &[(Option<ItemId>, String, String, bool)], marks: &[Option<std::ops::Range<usize>>], pick: Option<usize>, scroll: &mut usize, x: f32, y: f32, w: f32, visible: usize) -> Option<usize> {
    let k = chrome::k();
    let row_h = 186.0 * k / visible.max(1) as f32;
    let frame = Rect::new(x, y, w, visible as f32 * row_h);
    let bar = 18.0 * k;
    let ListColumns { icon: icon_w, name_x, price_cx, .. } = market_columns(w, bar);
    chrome::subtractive(|| draw_rectangle(x + icon_w, y, w - icon_w, frame.h, blend_color(MARKET_LIST_TAKEN)));
    chrome::paper_minus(Rect::new(x, y, icon_w, frame.h), (0.0, 0.0), MARKET_LIST_TAKEN);
    let max_scroll = rows.len().saturating_sub(visible);
    let over = mouse_in(x, y, w, visible as f32 * row_h);
    let wheel = if over { wheel() } else { 0.0 };
    if wheel < 0.0 {
        *scroll = (*scroll + 1).min(max_scroll);
    } else if wheel > 0.0 {
        *scroll = scroll.saturating_sub(1);
    }
    *scroll = (*scroll).min(max_scroll);
    // The scroll bar (Razdor's): arrows, the track pages, the knob drags.
    scrollbar(Rect::new(x + w - 15.0 * k, y, 12.0 * k, visible as f32 * row_h), scroll, visible, rows.len());
    if let Some(n) = pick.filter(|&n| n >= *scroll && n < *scroll + visible) {
        let ry = y + (n - *scroll) as f32 * row_h;
        let right = x + w - bar;
        chrome::additive(|| draw_rectangle(x + icon_w, ry, (right - x - icon_w).max(0.0), row_h, blend_color(MARKET_PICK_ADDED)));
    }
    chrome::silver_frame(frame, k.max(1.0));
    let mut hit = None;
    for (n, (icon, name, price, red)) in rows.iter().enumerate().skip(*scroll).take(visible) {
        let ry = y + (n - *scroll) as f32 * row_h;
        if let (Some(item), Some(assets)) = (icon, assets) {
            let side = (row_h - 4.0 * k).min(icon_w - 2.0 * k);
            assets.draw_item(*item, x + (icon_w - side) / 2.0, ry + 1.0, side);
        }
        let shown = ellipsize(name, price_cx - name_x - 30.0 * k, 17.0 * k);
        let visible_end = shown.strip_suffix('…').unwrap_or(&shown).len();
        let mark = marks.get(n).cloned().flatten().filter(|r| r.end <= visible_end && shown.is_char_boundary(r.start) && shown.is_char_boundary(r.end));
        let ink = market_ink(*red);
        text_marked(&shown, mark, x + name_x, ry + 20.0 * k, 17.0 * k, ink, ink);
        text_centered(price, x + price_cx, ry + 21.0 * k, 18.0 * k, ink);
        if mouse_in(x, ry, w - bar, row_h) && clicked() {
            hit = Some(n);
        }
    }
    hit
}

/// A list column whose title sorts the list when clicked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Column {
    Name,
    Price,
}

/// How the market's or the sanctuary's list is sorted by clicks on its column titles.
/// Razdor's addition (the owner's request after a player's report), not the original's: the
/// original cannot sort these lists. `Shown` is the list's own order, which a new window or
/// tab starts with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ListSort {
    #[default]
    Shown,
    /// А to Я.
    NameUp,
    /// Я to А.
    NameDown,
    /// The dearest first.
    PriceDown,
    /// The cheapest first.
    PriceUp,
}

impl ListSort {
    /// The sort after a click on a column's title: «Название» A→Я, then Я→A, then the list's
    /// own order; «Цена» the dearest first, then the cheapest first, then the own order.
    fn clicked(self, c: Column) -> ListSort {
        match (c, self) {
            (Column::Name, ListSort::NameUp) => ListSort::NameDown,
            (Column::Name, ListSort::NameDown) => ListSort::Shown,
            (Column::Name, _) => ListSort::NameUp,
            (Column::Price, ListSort::PriceDown) => ListSort::PriceUp,
            (Column::Price, ListSort::PriceUp) => ListSort::Shown,
            (Column::Price, _) => ListSort::PriceDown,
        }
    }
}

/// A name's key in alphabetical order, case left out; «ё» right after «е».
fn alphabet_key(s: &str) -> Vec<(char, bool)> {
    s.chars().flat_map(char::to_lowercase).map(|c| if c == 'ё' { ('е', true) } else { (c, false) }).collect()
}

/// `base` (the list's own order: indices into `names` and `prices`) sorted by `sort`; rows
/// with equal keys keep their order in `base`.
fn sorted_rows<P: Ord>(base: &[usize], names: &[&str], prices: &[P], sort: ListSort) -> Vec<usize> {
    let mut order = base.to_vec();
    match sort {
        ListSort::Shown => {}
        ListSort::NameUp => order.sort_by_cached_key(|&n| alphabet_key(names[n])),
        ListSort::NameDown => order.sort_by(|&a, &b| alphabet_key(names[b]).cmp(&alphabet_key(names[a]))),
        ListSort::PriceDown => order.sort_by(|&a, &b| prices[b].cmp(&prices[a])),
        ListSort::PriceUp => order.sort_by(|&a, &b| prices[a].cmp(&prices[b])),
    }
    order
}

/// The row that shows what row `pick` of `before` showed, in the order `after`.
fn same_pick(before: &[usize], after: &[usize], pick: Option<usize>) -> Option<usize> {
    let item = before.get(pick?)?;
    after.iter().position(|n| n == item)
}

/// The column whose title (centred on `name_cx` or `price_cx`, its baseline `y`, in `size`)
/// was clicked this frame, if any; measured in the font active when called.
fn title_clicked((name, price): (&str, &str), (name_cx, price_cx): (f32, f32), y: f32, size: f32) -> Option<Column> {
    if !clicked() {
        return None;
    }
    let k = chrome::k();
    [(Column::Name, name, name_cx), (Column::Price, price, price_cx)].into_iter().find_map(|(c, s, cx)| {
        let w = measure(s, size).width + 8.0 * k;
        mouse_in(cx - w / 2.0, y - size, w, size + 6.0 * k).then_some(c)
    })
}

/// The market rows' order: indices of `prices`, cheapest first; equal prices keep their
/// stock or pack order (a stable sort).
fn cheapest_first<P: Ord>(prices: &[P]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..prices.len()).collect();
    order.sort_by_key(|&n| &prices[n]);
    order
}

/// The row a market list opens with: its first, if it has any (0x4bca8c).
fn first_pick(rows: usize) -> Option<usize> {
    (rows > 0).then_some(0)
}

/// The row to select after row `k` left a list that now has `left` rows: the one that
/// moved up into it, else the one above, else none.
fn next_pick(k: usize, left: usize) -> Option<usize> {
    (left > 0).then(|| k.min(left - 1))
}

/// One of the market's two list buttons: whether it shows the pack («Инвентарь») or the
/// goods («Торговая лавка»), and whether it is enabled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ListButton {
    shows_pack: bool,
    enabled: bool,
}

/// The market's list buttons, «Инвентарь» then «Торговая лавка», with the pack (`selling`)
/// or the goods shown (0x4bca8c sets their enabled bytes +0x0d, 0x670b3d and 0x670cb1): the
/// shown list's own button is off, and the goods' also while the shop has none (it needs a
/// goods count above 0). An off button is drawn locked, not in its normal look: Button.Draw (0x479bbc) takes
/// the disabled picture +0x53 when +0x0d is 0, and for both buttons that is 0x67045b, the
/// `Btn3Down` art greyed (0x48db5c 100/200/100) and rescaled (0x48da5c ×1300/1024 −26), built
/// at 0x4d985f and set at 0x4d9a8c / 0x4d9b5f; the caption moves by the pressed offset.
/// A disabled widget takes no clicks either.
fn market_list_buttons(selling: bool, goods: usize) -> [ListButton; 2] {
    [ListButton { shows_pack: true, enabled: !selling }, ListButton { shows_pack: false, enabled: selling && goods > 0 }]
}

/// Market (0x4bca8c): the goods (mode 1) or the pack (mode 0, "Инвентарь") with prices,
/// the selected item's description, and the trade button. The goods list stands right of the
/// description, the pack list left of it (the original moves both panes with the mode).
/// "Инвентарь" (0x4b9be0) shows the pack and "Торговая лавка" (0x4b9be8) the goods: both
/// always shown, the current list's one locked, "Торговая лавка" also while the shop is empty.
fn market(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let k = chrome::k();
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let c = game.content.clone();
    let dw = w * 0.492;
    let lw = w - dw - 8.0 * k;
    let (dx, lx) = if view.selling { (x + lw + 8.0 * k, x) } else { (x, x + dw + 8.0 * k) };
    let heading = own_text("Building", "ItemInfo", n_("Item description"));
    super::typography::Role::Title.with(|| chrome::shadow_centered(&heading, dx + dw / 2.0, y + 13.0 * k, 17.0 * k, MARKET_HEADING));
    // The sell list holds only the pack items worth more than 1 (personal items and the
    // cheapest are not bought); a buy price above the gold is red (0x4bca8c).
    let sellable: Vec<usize> = game.pack.entries().filter(|&(_, i)| game.can_sell(i)).map(|(k, _)| k).collect();
    let goods = game.market_here().unwrap_or_default();
    // Both lists cheapest first, whatever the item's kind (the owner's choice; the original
    // shows them unsorted, in stock and pack order: 0x4be9a0 jumps over its sort). `order[row]`
    // is the row's index in `sellable` or in the goods, which the trade takes.
    let prices: Vec<_> = if view.selling { sellable.iter().map(|&n| game.sell_price(game.pack[n])).collect() } else { goods.iter().map(|&i| game.buy_price(i)).collect() };
    let names: Vec<&str> = if view.selling { sellable.iter().map(|&n| c.item(game.pack[n]).name.as_str()).collect() } else { goods.iter().map(|&i| c.item(i).name.as_str()).collect() };
    let own_order = cheapest_first(&prices);
    // A click on «Название» or «Цена» sorts the list (Razdor's addition, `ListSort`; the
    // original's titles do nothing). The picked item stays picked.
    let (name_head, price_head) = (own_text("Building", "ItemName", n_("Name")), own_text("Building", "ItemCost", n_("Price")));
    let cols = market_columns(lw, 18.0 * k);
    let titles_shown = view.filter.is_empty() && !has_focus(FILTER_KEY);
    let title_hit = if titles_shown {
        super::typography::Role::Title.with(|| title_clicked((&name_head, &price_head), (lx + cols.names_cx, lx + cols.price_cx), y + 13.0 * k, 17.0 * k))
    } else {
        None
    };
    if let Some(column) = title_hit {
        let before = sorted_rows(&own_order, &names, &prices, view.sort);
        view.sort = view.sort.clicked(column);
        let after = sorted_rows(&own_order, &names, &prices, view.sort);
        view.pick = same_pick(&before, &after, view.pick);
        swallow_input();
    }
    let order = sorted_rows(&own_order, &names, &prices, view.sort);
    let rows: Vec<(Option<ItemId>, String, String, bool)> = if view.selling {
        order.iter().map(|&n| game.pack[sellable[n]]).map(|i| (Some(i), c.item(i).name.clone(), game.sell_price(i).to_string(), false)).collect()
    } else {
        order.iter().map(|&n| goods[n]).map(|i| (Some(i), c.item(i).name.clone(), game.buy_price(i).to_string(), game.buy_price(i) > game.gold)).collect()
    };
    // Over the list the original's column titles, "Название" and "Цена" (0x4bca8c). The
    // filter (Razdor's) takes their line only while it is used: a letter typed over the list,
    // Ctrl+F or a click on the titles' line beside the titles starts it; the list keeps the
    // items that match, their matched part lit; Enter picks the first.
    let all: Vec<(usize, ItemId)> = rows.iter().enumerate().filter_map(|(n, r)| r.0.map(|i| (n, i))).collect();
    let before = item_filter::keep(&c, all.iter().copied(), &view.filter).len();
    let filter_rect = Rect::new(lx, y, lw, 18.0 * k);
    let reply = item_filter::field_with(FILTER_KEY, &mut view.filter, filter_rect, &[KeyCode::N], before, rows.len(), false);
    if view.filter.is_empty() && !has_focus(FILTER_KEY) {
        // Each title centred over its own list (0x48e750 for 0x670688 and 0x670760).
        super::typography::Role::Title.with(|| {
            chrome::shadow_centered(&name_head, lx + cols.names_cx, y + 13.0 * k, 17.0 * k, MARKET_HEADING);
            chrome::shadow_centered(&price_head, lx + cols.price_cx, y + 13.0 * k, 17.0 * k, MARKET_HEADING);
        });
    }
    let kept = item_filter::keep(&c, all, &view.filter);
    let shown: Vec<(Option<ItemId>, String, String, bool)> = kept.iter().map(|(n, _)| rows[*n].clone()).collect();
    let marks: Vec<Option<std::ops::Range<usize>>> = kept.iter().map(|(_, m)| m.name_range.clone()).collect();
    // The original opens either list with its first row picked and described (0x4bca8c),
    // the market tab and each switch between the goods and the pack alike.
    if view.pick.is_none() && view.filter.trim().is_empty() {
        view.pick = first_pick(rows.len());
    }
    let at_pick = view.pick.and_then(|p| kept.iter().position(|(n, _)| *n == p));
    if let Some(k) = price_list(Some(assets), &shown, &marks, at_pick, &mut view.scroll, lx, y + 21.0 * k, lw, 7) {
        view.pick = Some(kept[k].0);
    }
    if reply.pick {
        view.pick = kept.first().map(|(n, _)| *n);
    }
    // A row the filter hides is not the one picked.
    if !view.filter.trim().is_empty() && view.pick.is_some_and(|p| !kept.iter().any(|(n, _)| *n == p)) {
        view.pick = None;
    }
    if view.pick.is_some_and(|k| k >= rows.len()) {
        view.pick = None;
    }
    let dh = 186.0 * k;
    let card_y = y + 21.0 * k;
    match view.pick.and_then(|k| rows.get(k)).and_then(|r| r.0) {
        Some(item) => super::item_text::draw(game, assets, item, Rect::new(dx, card_y, dw, dh), super::item_text::ItemBox::Market, k),
        None => {
            chrome::text_box(Rect::new(dx, card_y, dw, dh));
            text_centered(tr("Pick an item from the list."), dx + dw / 2.0, card_y + dh / 2.0, 18.0 * k, BOX_INK);
        }
    }
    let by = f.y + 249.0 * k;
    for (offset, width) in [(0.0, 166.0), (166.0, 134.0), (300.0, 108.0), (408.0, 164.0)] {
        chrome::silver_frame(Rect::new(x + offset * k, f.y + 241.0 * k, width * k, 50.0 * k), k.max(1.0));
    }
    let [invent_button, shop_button] = market_list_buttons(view.selling, goods.len());
    let (invent, shop) = (own_text("Building", "Invent", n_("Inventory")), own_text("Building", "Market", n_("Trading shop")));
    // The list button of the list shown is locked (greyed), as the original's (0x479bbc).
    if button(x + 8.0 * k, by, 150.0 * k, 34.0 * k, &invent, invent_button.enabled) {
        view.selling = invent_button.shows_pack;
        view.pick = None;
        view.scroll = 0;
    }
    // The money between "Инвентарь" and the trade button, its label over the amount.
    gold_group(game, Rect::new(x + 174.0 * k, by - 2.0 * k, 118.0 * k, 40.0 * k));
    let label = if view.selling { own_text("Building", "Trade", n_("Sell")) } else { own_text("Building", "Buy", n_("Buy")) };
    // Sell is always on; Buy only when the price is at most the gold, the purchase's test.
    let can = match (view.selling, view.pick) {
        (true, Some(k)) => k < order.len(),
        (false, Some(k)) => rows.get(k).and_then(|r| r.0).is_some_and(|i| game.gold >= game.buy_price(i)),
        _ => false,
    };
    // The trade button plays the gold sound (interface.md §14).
    if button_sounding(x + 308.0 * k, by, 90.0 * k, 34.0 * k, &label, can, Cue::Gold) {
        let k = view.pick.unwrap_or(0);
        let mut done = false;
        *message = Some(if view.selling {
            let pack_index = sellable[order[k]];
            let name = c.item(game.pack[pack_index]).name.clone();
            match game.sell(pack_index) {
                Ok(g) => {
                    done = true;
                    trf!("Sold {name} for {g} gold.", name, g)
                }
                Err(e) => trade_error(e),
            }
        } else {
            match game.buy(order[k]) {
                Ok(item) => {
                    done = true;
                    trf!("Bought {item}. It is in your pack.", item = c.item(item).name)
                }
                Err(e) => trade_error(e),
            }
        });
        // For many buys (or sales) in a row the selection stays: on the item that moved up
        // into the bought one's row, or the one above when it was the last; none when the
        // list is empty. A refused trade keeps it where it was.
        if done {
            let left = if view.selling { game.pack.iter().filter(|&i| game.can_sell(i)).count() } else { game.market_here().map_or(0, |g| g.len()) };
            view.pick = next_pick(k, left);
        }
    }
    if button(x + 416.0 * k, by, 148.0 * k, 34.0 * k, &shop, shop_button.enabled) {
        view.selling = shop_button.shows_pack;
        view.pick = None;
        view.scroll = 0;
    }
    FILTER_KEYS.with(|f| f.set(reply.keys_taken));
    hall_offers(game, f, view, message, dialogs)
}

/// Sanctuary: spells for sale; a spell bought goes into the hero's book.
fn sanctuary(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let k = chrome::k();
    let own: Vec<SpellDef> = game.spells_here().into_iter().cloned().collect();
    // The list in the shop's order, or sorted by a click on a title (Razdor's addition,
    // `ListSort`).
    let own_order: Vec<usize> = (0..own.len()).collect();
    let names: Vec<&str> = own.iter().map(|s| s.name.as_str()).collect();
    let prices: Vec<i32> = own.iter().map(|s| s.cost_gold).collect();
    let order = sorted_rows(&own_order, &names, &prices, view.sort);
    let spells: Vec<SpellDef> = order.iter().map(|&n| own[n].clone()).collect();
    view.pick = view.pick.filter(|&n| n < spells.len()).or_else(|| (!spells.is_empty()).then_some(0));
    // Laid out as the original's (0x4ba854, tab 4; measured on its screen): the list on the
    // right, a picture column, "Название заклятия" and "Цена" over six rows in one frame; a
    // spell already in the book in its own colour.
    let list = at(f, 530.0, 55.0, 296.0, 234.0);
    let (row, title) = spell_list(game, &spells, view.pick, list);
    if let Some(n) = row {
        view.pick = Some(n);
    }
    if let Some(column) = title {
        view.sort = view.sort.clicked(column);
        let after = sorted_rows(&own_order, &names, &prices, view.sort);
        view.pick = same_pick(&order, &after, view.pick);
        swallow_input();
    }
    // The description on the left: its title, the spell's card, the message box, the money
    // and "Купить" (0x4ba078).
    let title = own_text("Building", "SpellInfo", n_("Spell description"));
    super::dt_font::with_face(super::dt_font::Face::Title, || chrome::shadow_centered(&title, f.x + 388.0 * k, f.y + 46.0 * k, 15.0 * k, MARKET_HEADING));
    let card = at(f, 255.0, 55.0, 266.0, 103.0);
    let chosen = view.pick.and_then(|n| spells.get(n));
    match chosen {
        Some(s) => super::spellbook::spell_card(game, s, card, false),
        None => {
            chrome::text_box(card);
            text_centered(tr("Pick a spell from the list."), card.center().x, card.center().y, 16.0 * k, BOX_INK);
        }
    }
    let note = at(f, 255.0, 165.0, 266.0, 66.0);
    let (can, said) = match chosen {
        Some(s) if game.knows_spell(s.id) => (false, Some((own_text("Building", "AlreadySpell", n_("This spell is already in your book!")), Color::from_rgba(55, 180, 255, 255)))),
        Some(s) if game.gold < s.cost_gold => (false, Some((own_text("Building", "NoMoneyForSpell", n_("You do not have enough money for this spell!")), chrome::RED_TEXT))),
        Some(_) if game.spells.len() >= SPELL_BOOK_SIZE => (false, Some((own_text("Building", "NoPlaceForSpell", n_("There is no room for the spell in your spell book!")), chrome::RED_TEXT))),
        Some(_) => (true, None),
        None => (false, None),
    };
    if let Some((t, color)) = said {
        super::typography::Role::Name.with(|| {
            let size = 15.0 * k;
            let line_h = super::typography::Role::Name.line_height(19.0 * k);
            let lines = wrap(&t, note.w - 8.0 * k, size);
            let top = note.center().y - (lines.len().saturating_sub(1) as f32 * line_h) / 2.0 + super::typography::Role::Name.size(size) * 0.36;
            for (i, l) in lines.iter().enumerate() {
                chrome::shadow_centered(l, note.center().x, top + i as f32 * line_h, size, color);
            }
        });
    }
    chrome::silver_frame(at(f, 255.0, 241.0, 266.0, 50.0), k.max(1.0));
    chrome::silver_frame(at(f, 255.0, 241.0, 160.0, 50.0), k.max(1.0));
    gold_group(game, at(f, 255.0, 241.0, 160.0, 50.0));
    let buy = at(f, 424.0, 247.0, 92.0, 36.0);
    if button_sounding(buy.x, buy.y, buy.w, buy.h, tr("Buy"), can, Cue::Gold) {
        if let Some(s) = chosen {
            *message = game.learn_spell(s.id).err().map(service_error);
        }
    }
    hall_offers(game, f, view, message, dialogs)
}

/// The sanctuary list's columns as the original lays them out (0x4d9fe6..0x4da0ae): the
/// spells' picture strip 42 px, the names 316 − 70 − 42 = 204 px, the prices 70 px.
const SPELL_ICON_W: f32 = 42.0;
const SPELL_NAME_W: f32 = 204.0;
const SPELL_PRICE_W: f32 = 70.0;

/// What the sanctuary takes off Win-paper under its list, copied from (20, 20) (0x4bb3c3,
/// 0x4bb3e0): (148, 132, 148), the same paper as the hint strip's.
const SPELL_LIST_TAKEN: [i32; 3] = [148, 132, 148];
/// What it takes off below the last spell, the paper copied on from (20, 20 + the rows'
/// height) (0x4bb592, 0x4bb5af): (180, 140, 113), darker.
const SPELL_EMPTY_TAKEN: [i32; 3] = [180, 140, 113];
/// What a picked spell's row adds over the names and prices: (24, 32, 24), the lists' own
/// highlight in the adding mode 4 (+0xb1 set at 0x4da030 and 0x4da0e5).
const SPELL_PICK_ADDED: [i32; 3] = [24, 32, 24];

/// The tint of a spell's name and price (0x4ba854 tab 4, 0x49c100): a spell already in the
/// book in font ae24a8 (blue), any other in ae24a0 (yellow), whatever its price and the gold.
fn spell_ink(known: bool) -> Color {
    if known { benguiat(BLUE_INK) } else { benguiat(YELLOW_INK) }
}

/// Where a spell's picture stands in its row: the original paints each 100 px picture at
/// (4, 4 + 104·n) of a 108 × 628 strip keyed on (95, 59, 0), shrinks the strip to 42 × 250
/// (0x4bc3c1, 0x4bc42a) and blits it keyed at the list's left (0x4bb533). Returns the
/// picture's left and top offsets and its side, for a column `icon_w` wide and rows `row_h`.
fn spell_icon_place(icon_w: f32, row_h: f32) -> (f32, f32, f32) {
    let (sx, sy) = (icon_w / 108.0, row_h / 104.0);
    (4.0 * sx, 4.0 * sy, (100.0 * sx).min(100.0 * sy))
}

/// The sanctuary's spell list (0x4ba854, tab 4), as the original builds it: one panel under
/// the picture strip, the names and the prices, Win-paper from (20, 20) with (148, 132, 148)
/// taken off; below the last spell the paper with (180, 140, 113) taken off; a 1 px frame
/// round it (0x48d1d4 at 0x4bb508); the pictures on the left; the names left and the prices
/// centred in their lists (align 1 and 2, 0x4bb0b0 and 0x4bb0ca), both in one tint; the
/// picked row lighter by (24, 32, 24) over the names and prices, not the pictures. Six rows
/// over the frame's height; a click picks a row, a click on a column's title sorts the list
/// (Razdor's addition, `ListSort`): the row and the title clicked.
fn spell_list(game: &Game, spells: &[SpellDef], pick: Option<usize>, r: Rect) -> (Option<usize>, Option<Column>) {
    let k = chrome::k();
    let cols = list_columns(r.w, 0.0, [SPELL_ICON_W, SPELL_NAME_W, SPELL_PRICE_W]);
    let row_h = r.h / 6.0;
    let rows = spells.len().min(6);
    let used = rows as f32 * row_h;
    chrome::paper_minus(Rect::new(r.x, r.y, r.w, used), (20.0, 20.0), SPELL_LIST_TAKEN);
    chrome::paper_minus(Rect::new(r.x, r.y + used, r.w, r.h - used), (20.0, 20.0 + used / k), SPELL_EMPTY_TAKEN);
    chrome::silver_frame(r, k.max(1.0));
    // The column titles stand over the frame, each centred over its list (0x48e750).
    let (name_head, price_head) = (own_text("Building", "SpellName", n_("Spell")), own_text("Building", "ItemCost", n_("Price")));
    let title = super::dt_font::with_face(super::dt_font::Face::Title, || {
        chrome::shadow_centered(&name_head, r.x + cols.names_cx, r.y - 9.0 * k, 15.0 * k, MARKET_HEADING);
        chrome::shadow_centered(&price_head, r.x + cols.price_cx, r.y - 9.0 * k, 15.0 * k, MARKET_HEADING);
        title_clicked((&name_head, &price_head), (r.x + cols.names_cx, r.x + cols.price_cx), r.y - 9.0 * k, 15.0 * k)
    });
    let mut hit = None;
    let (icon_dx, icon_dy, side) = spell_icon_place(cols.icon, row_h);
    for (n, s) in spells.iter().enumerate().take(6) {
        let ry = r.y + n as f32 * row_h;
        if pick == Some(n) {
            chrome::additive(|| draw_rectangle(r.x + cols.icon, ry, r.w - cols.icon, row_h, blend_color(SPELL_PICK_ADDED)));
        }
        chrome::spell_icon(s, Rect::new(r.x + icon_dx, ry + icon_dy, side, side));
        let ink = spell_ink(game.knows_spell(s.id));
        super::typography::Role::Name.with(|| {
            let size = 18.0 * k;
            let baseline = ry + row_h / 2.0 + size * 0.35;
            let room = cols.price_cx - (SPELL_PRICE_W / 2.0) * (cols.icon / SPELL_ICON_W) - cols.name_x - 4.0 * k;
            text(&ellipsize(&s.name, room, size), r.x + cols.name_x, baseline, size, ink);
            text_centered(&s.cost_gold.to_string(), r.x + cols.price_cx, baseline, size, ink);
        });
        if button_state(Rect::new(r.x, ry, r.w, row_h), true).2 {
            hit = Some(n);
        }
    }
    (hit, title)
}

/// The money as the market and the sanctuary show it (0x4bd39c): the gold picture, then
/// "Деньги" with the amount under it, the group centred in `r`.
fn gold_group(game: &Game, r: Rect) {
    super::typography::Role::Name.with(|| gold_group_in_face(game, r))
}

fn gold_group_in_face(game: &Game, r: Rect) {
    let k = chrome::k();
    let label = own_text("Building", "Gold", n_("Money"));
    let icon = 34.0 * k;
    let size = fit_size(&label, r.w - icon - 6.0 * k, 17.0 * k);
    let amount = game.gold.to_string();
    let tw = measure(&label, size).width.max(measure(&amount, 16.0 * k).width);
    let x = r.x + ((r.w - icon - 6.0 * k - tw) / 2.0).max(0.0);
    resource_icon(Resource::Gold, x + icon / 2.0, r.y + r.h / 2.0, icon);
    let tx = x + icon + 6.0 * k;
    chrome::shadow_text(&label, tx, r.y + r.h / 2.0 - 4.0 * k, size, MARKET_HEADING);
    chrome::shadow_text(&amount, tx, r.y + r.h / 2.0 + 15.0 * k, 16.0 * k, ACCENT);
}

/// The original's Benguiat tints (0x4dbb0c adds a per-channel delta to the white glyphs).
fn benguiat(rgb: [u8; 3]) -> Color {
    Color::from_rgba(rgb[0], rgb[1], rgb[2], 255)
}

/// `Benguiat` with red − 120 (0x4dbb0c: font 0xae24ac), the village's mana line.
const CYAN_INK: [u8; 3] = [135, 255, 255];

/// The picture of a village stock slot: the full one while the stock is above 0, else the
/// empty one (0x4d11ef gold, 0x4d1247 mana).
fn stock_picture(stock: i32, mana: bool) -> &'static str {
    match (mana, stock > 0) {
        (false, true) => "Village_Gold",
        (false, false) => "Village_Gold_Empty",
        (true, true) => "Village_Mana",
        (true, false) => "Village_Mana_Empty",
    }
}

/// The caption over a stock slot: `[Building] Gold` / `Mana`, then " + N" while the stock is
/// above 0, else " = N" (0x4d0f7a, 0x4d0fa3).
fn stock_caption(label: &str, stock: i32) -> String {
    format!("{label}{}{stock}", if stock > 0 { " + " } else { " = " })
}

/// The village window (built by 0x4d3a38, drawn by 0x4d0e28, filled by 0x4bbc84), in the
/// original's pixels: the 634×516 frame centred over the map, the village's name as its
/// title, `S_Village` inside an inset frame; a text box at (18, 45), 598 wide and as tall as its text
/// plus 14 above and below, at most 193: `AboutVillage` (`#NAME1` = the owner's name,
/// white, centred), an empty line, then `VillageEmpty` (orange-red) when both stocks are 0,
/// else `VillageFullGold` (yellow) when there is gold and `VillageFullMana` (cyan) when there
/// is mana, an empty line between the two. Two 160×160 slots at x = W·(i+1)/3 − 80, 279
/// down (41 under the box's full 193): `Village_Gold` / `Village_Mana`, or their `_Empty`
/// pictures for a stock of 0; over each, 24 px up and centred, its caption ("Деньги + N" in
/// yellow, "Магия + N" in blue) with a 1 px shadow. `Ok` (`Btn1`) centred 10 px from the
/// bottom and the close box, both 0x4c6000. Razdor takes the tribute as the hero enters
/// (economy.md §3), so the window shows what was taken ([`BuildingView::tribute`]).
/// The village window's texts: the box and the two stock captions ([`village_window`]).
fn village_texts(game: &Game, view: &BuildingView, l: usize, (x, y, o): (f32, f32, f32), size: f32, line_h: f32) {
    use razdor::dt::markup::Ink;
    let loc = &game.world.locations[l];
    let (gold, mana) = view.tribute;

    let (bx, by, bw) = (x + 18.0 * o, y + 45.0 * o, 598.0 * o);
    let lines_of = |t: &str| t.split('\n').flat_map(|p| if p.trim().is_empty() { vec![String::new()] } else { wrap(p, bw - 32.0 * o, size) }).collect::<Vec<_>>();
    let mut lines: Vec<(String, Color)> = Vec::new();
    let mut add = |t: &str, c: Color| lines.extend(lines_of(t).into_iter().map(|t| (t, c)));
    let about = own_text("Building", "AboutVillage", n_("You enter the village, and the local #NAME1 greets you with reverence.")).replace("#NAME1", &loc.owner_name);
    add(&about, benguiat(Ink::Star.rgb()));
    add("", WHITE);
    if gold <= 0 && mana <= 0 {
        add(&own_text("Building", "VillageEmpty", n_("The tribute of this village has already been taken: there is nothing left but to bid the headman farewell.")), benguiat(Ink::At.rgb()));
    } else {
        if gold > 0 {
            add(&own_text("Building", "VillageFullGold", n_("As a nobleman you collect the village's tax, promising to hunt the robbers and shield the peasants from heretics and hostile lords.")), benguiat(Ink::Plain.rgb()));
            if mana > 0 {
                add("", WHITE);
            }
        }
        if mana > 0 {
            add(&own_text("Building", "VillageFullMana", n_("The grateful peasants pray for your magic power to grow.")), benguiat(CYAN_INK));
        }
    }
    let text_h = lines.len() as f32 * line_h;
    let bh = (text_h + 28.0 * o).min(193.0 * o);
    let top = if text_h + 28.0 * o <= 193.0 * o { 14.0 * o } else { (bh - text_h) / 2.0 };
    chrome::shaded_text_box(Rect::new(bx, by, bw, bh), 0.30);
    for (i, (t, c)) in lines.iter().enumerate() {
        let ly = by + top + i as f32 * line_h;
        if ly >= by && ly + line_h <= by + bh {
            chrome::shadow_centered(t, bx + bw / 2.0, ly + size, size, *c);
        }
    }
    for (i, (stock, is_mana)) in [(gold, false), (mana, true)].into_iter().enumerate() {
        let sx = x + (634 * (i as i32 + 1) / 3 - 80) as f32 * o;
        let sy = y + 279.0 * o;
        if let Some(t) = chrome::win(stock_picture(stock, is_mana)) {
            chrome::tex(&t, Rect::new(sx, sy, 160.0 * o, 160.0 * o), WHITE);
        }
        let (label, ink) = if is_mana { (own_text("Building", "Mana", n_("Magic")), Ink::Bar) } else { (own_text("Building", "Gold", n_("Money")), Ink::Plain) };
        chrome::shadow_centered(&stock_caption(&label, stock), sx + 80.0 * o, sy - 24.0 * o + size, size, benguiat(ink.rgb()));
    }
}

thread_local! {
    /// A key went down while the village window was up (its «Ок» is pressed).
    static VILLAGE_KEY: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The village window's «Ок» by key this frame: pressed by a key going down, clicked by its
/// release (0x4cd81c).
fn village_ok_key(down: bool, up: bool) -> bool {
    VILLAGE_KEY.with(|k| {
        if down {
            k.set(true);
        }
        let click = up && k.get();
        if click {
            k.set(false);
        }
        click
    })
}

fn village_window(game: &mut Game, view: &BuildingView, message: &mut Option<String>) -> Option<Screen> {
    let l = game.location?;
    let o = chrome::k() * 0.9375;
    let (w, h) = ((634.0 * o).round(), (516.0 * o).round());
    let (x, y) = (((screen_width() - w) / 2.0).round(), ((screen_height() - chrome::bar_height() - h) / 2.0).max(2.0).round());
    let loc = &game.world.locations[l];
    let (_, closed) = chrome::window(Rect::new(x, y, w, h), &loc.name, chrome::Skin::Marble, true);
    // The reference has a narrow marble gap around the picture and a separate framed
    // footer. Stretch the complete village artwork to these bounds, without cropping it.
    let picture = Rect::new(x + 4.0 * o, y + 31.0 * o, w - 8.0 * o, 428.0 * o);
    if let Some(t) = chrome::win("S_Village") {
        chrome::tex(&t, picture, WHITE);
    }
    chrome::silver_frame(picture, o.max(1.0));
    let footer = Rect::new(x + 2.0 * o, y + 461.0 * o, w - 4.0 * o, h - 463.0 * o);
    chrome::silver_frame(footer, o.max(1.0));
    let size = (15.0 * o).round();
    let line_h = (17.0 * o).round();
    // The original's texts here are Benguiat's tints (0x4dbb0c fonts ae24a0..ae24ac).
    super::dt_font::with_original_face(super::dt_font::Face::Subtitle, || village_texts(game, view, l, (x, y, o), size, line_h));
    let (btn_w, btn_h) = chrome::win("Btn1Up").map_or((180.0, 40.0), |t| (t.width(), t.height()));
    let (btn_w, btn_h) = (btn_w * o, btn_h * o);
    let btn_x = x + (w - btn_w) / 2.0;
    let btn_y = y + h - 10.0 * o - btn_h;
    let label = own_text("Buttons", "Ok", n_("OK"));
    // Any key but Tab, Alt and the Up and Down arrows presses «Ок» and its release clicks it,
    // Space and Enter included (the window's key handler 0x4cd81c: 0x479eb8 / 0x47a0dc on
    // 0x671a24), as in the one-button event window.
    // A release counts only after the key went down with the window up (the button must be
    // pressed first).
    let key_ok = village_ok_key(ok_key_pressed(), ok_key_released());
    let ok = button(btn_x, btn_y, btn_w, btn_h, &label, true) || key_ok;
    // The close box and Esc sound as the button does (tools/difftest/AV.md §5).
    let esc = key(KeyCode::Escape);
    if closed || esc || key_ok {
        cue(Cue::Button);
    }
    if !(ok || closed || esc) {
        return None;
    }
    VILLAGE_KEY.with(|k| k.set(false));
    // Closing plays `Item-Gold` too when the tribute was taken (0x4c604a).
    if view.tribute_paid {
        cue(Cue::Gold);
    }
    *message = None;
    Some(Screen::WorldMap)
}

/// The picture of a village offer in the event window (0x4aca80 sets the record's picture,
/// +0xa7): `Village_Bonus_1` the blessing, `_2` the priest, `_3` the furs, `_4` the witch,
/// `_5` the innkeeper. The `_3A`/`_4A`/`_5A` pictures are loaded (0x4dc118: 0x671d24,
/// 0x671d2c, 0x671d34) and never read.
pub fn offer_picture(o: VillageOffer) -> &'static str {
    match o {
        VillageOffer::Blessing => "Village_Bonus_1",
        VillageOffer::Priest => "Village_Bonus_2",
        VillageOffer::Furs => "Village_Bonus_3",
        VillageOffer::Witch => "Village_Bonus_4",
        VillageOffer::Innkeeper => "Village_Bonus_5",
    }
}

/// The offer's question (0x4aca80, `[Event]`): `VillageBonus1` / `2` carry their question;
/// 3, 4 and 5 are `VillageBonusN` + `VillageBonusNAsk`. With `result`, the text of the window
/// a Yes opens for 3, 4 and 5: `VillageBonusN` + `VillageBonusNResult`.
fn offer_text(o: VillageOffer, result: bool) -> String {
    // The install's text in Russian, else ours behind the original's marks: two blank lines
    // (`#\`) and the line's font (`@` orange for the question, `*` white, `|` blue). The
    // install's text keeps its `#\` too: the window reads it as markup.
    let ini = |key: &str| (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_markup("Event", key)).flatten();
    let own = |key: &str, mark: &str, ours: &'static str| match ini(key) {
        Some(t) => t,
        None if mark.is_empty() => tr(ours).to_string(),
        None => format!("#\\#\\#\\{mark}{}", tr(ours)),
    };
    let ask = n_("Will you take this offer instead of the usual tribute?");
    let (n, body) = match o {
        VillageOffer::Blessing => {
            let ours = n_("The local priest asks you not to take the village's tribute. Instead he will cast a good spell on your army, one that lasts very long thanks to some secrets of his craft.");
            return ini("VillageBonus1").unwrap_or_else(|| format!("{}#\\#\\#\\@{}", tr(ours), tr(ask)));
        }
        VillageOffer::Priest => {
            let ours = n_("The local priest asks you not to take the village's tribute. He sees that your soldiers need healing, and he will try to cure all he can, as far as his strength goes.");
            return ini("VillageBonus2").unwrap_or_else(|| format!("{}#\\#\\#\\@{}", tr(ours), tr(ask)));
        }
        VillageOffer::Furs => (3, own("VillageBonus3", "", n_("Some hunters of the village come up to you. They ask you not to take the village's tribute; instead they will give you furs that sell at the market for much more than the usual tribute."))),
        VillageOffer::Witch => (4, own("VillageBonus4", "", n_("The village witch asks you not to take the village's tribute. Instead she will perform a ritual that greatly increases your magic power."))),
        VillageOffer::Innkeeper => (5, own("VillageBonus5", "", n_("The village innkeeper comes up to you. He asks you not to take the village's tribute: he sees that many of your soldiers have not been paid for a long time, and he will feed them and give them drink in place of their pay, so that they all fight again."))),
    };
    let tail = match (n, result) {
        (_, false) => own(&format!("VillageBonus{n}Ask"), "@", ask),
        (3, true) => own("VillageBonus3Result", "*", n_("You agree. The hunters hand you a bag of furs.")),
        (4, true) => own("VillageBonus4Result", "|", n_("You agree. The witch performs her ritual and your magic power grows.")),
        _ => own("VillageBonus5Result", "*", n_("You agree. Your soldiers are content now and ready to fight!")),
    };
    format!("{body}{tail}")
}

/// The village's offer as the original shows it (0x4aca80 → 0x4a8ae8): the event window
/// titled with the village's name, a Yes/No question, the offer's picture left of the text.
pub fn offer_dialog(game: &Game, o: VillageOffer, result: bool) -> Dialog {
    let name = game.location.map_or(String::new(), |l| game.world.locations[l].name.clone());
    let mut d = Dialog::new(name);
    d.chord = true;
    d.question = !result;
    d.offer = !result;
    d.village_event = true;
    d.marked = Some(offer_text(o, result));
    d.picture = chrome::win(offer_picture(o)).map(Picture::Side);
    d
}

/// The answer to the village's offer (`Dialog::offer`). Yes (0x4ab1ec, 0x4ab966): the offer
/// taken, the village emptied; the blessing's and the priest's spell lands over the hero
/// with no window, the furs, the witch and the innkeeper show their result in the event
/// window again (0x4c2100: its chord, its OK). No (0x4c2378): the village is entered again
/// with no offer, its window opens and the tribute is taken. Returns the next screen.
pub fn answer_offer(game: &mut Game, yes: bool, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    use razdor::rules::economy::{OfferResult, PRIEST_SPELL};
    let o = game.village_offer()?;
    if !yes {
        let mana = game.location.map_or(0, |l| game.world.locations[l].tribute_mana);
        let paid = game.decline_offer();
        let tab = game.location.and_then(|l| game.window_at(l))?;
        let mut v = BuildingView::new(tab);
        v.tribute_paid = paid.is_some();
        v.tribute = match paid {
            Some(Tribute::Gold(g)) => (g, mana),
            Some(Tribute::Item(_)) => (0, mana),
            None => (0, 0),
        };
        return Some(Screen::Building(v));
    }
    let result = game.accept_offer();
    if o.result_window() {
        let mut dialog = offer_dialog(game, o, true);
        match result {
            Some(OfferResult::Furs(item)) => dialog.items.push(item),
            Some(OfferResult::Mana(amount)) => dialog.resources.push((Resource::Mana, trf!("Magic + {magic}", magic = amount))),
            _ => {}
        }
        dialogs.push_back(dialog);
    }
    // The blessing's and the priest's spell is cast as an event's: its effect over the hero,
    // its sound by its target.
    let cast = match result {
        Some(OfferResult::Blessing(id)) => Some(id),
        Some(OfferResult::Healed(_)) => Some(PRIEST_SPELL),
        _ => None,
    };
    if let Some(s) = cast.and_then(|id| game.spell(id).cloned()) {
        cue(if razdor::rules::magic::targets_enemy(&s) { Cue::SpellEvil } else { Cue::SpellGood });
        super::world_view::spell_effect(&s, razdor::rules::magic::CastTarget::Own);
    }
    None
}

/// The shipyard's ship window (0x4d3ec0, opened by 0x4bbc84 when the hero is on land), in
/// the original's pixels: the generated 634×516 frame centred over the map, the building's
/// name as its title, `S_Shipyard` at (5, 32), a text box at (18, 45), 598 wide and as tall
/// as its text plus 28 (at most 193), with `AboutShipyard` (`#NAME1` = the owner's name)
/// and, when the gold is short of `ShipCost`, an empty line and `NoMoneyForShip` in red; the
/// "CostShip = ShipCost" line 17 px under the box (0x4d1314); "Нанять корабль" at the bottom
/// left, enabled iff ShipCost ≤ gold, and "Отмена" at the bottom right (`Btn3`). No attitude
/// or owner test. Every button sounds as it is pressed (0x4b958c). Buying (0x4c60ac) closes
/// the window, any old ship is gone, the gold paid, `Item-Gold`; "Отмена", the close box and
/// Esc (0x4c6118, 0x4cd8d0) close it, the hero still standing in the shipyard.
fn ship_window(game: &mut Game) -> Option<Screen> {
    let l = game.location?;
    // Screen pixels per pixel of the original's 1024×768.
    let o = chrome::k() * 0.9375;
    let (w, h) = ((634.0 * o).round(), (516.0 * o).round());
    let (x, y) = (((screen_width() - w) / 2.0).round(), ((screen_height() - chrome::bar_height() - h) / 2.0).max(2.0).round());
    let loc = &game.world.locations[l];
    // The title is the building's name as it is, even an empty one (0x4bbc84).
    let (_, closed) = chrome::window(Rect::new(x, y, w, h), &loc.name, chrome::Skin::Marble, true);
    if let Some(t) = chrome::win("S_Shipyard") {
        let (pw, ph) = ((t.width() * o).min(w - 10.0 * o), (t.height() * o).min(h - 34.0 * o));
        chrome::tex_src(&t, Rect::new(0.0, 0.0, pw / o, ph / o), Rect::new(x + 5.0 * o, y + 32.0 * o, pw, ph), WHITE);
    }
    let price = game.ship_price();
    let affordable = price <= game.gold;
    let about = own_text("Building", "AboutShipyard", n_("You have entered a harbour owned by #NAME1. The harbour master tells you that a ship can be hired here.\n\nThe terms are simple: for a fixed price the ship takes you anywhere on the coast. When you go ashore, it stays and waits for you to come back."))
        .replace("#NAME1", &loc.owner_name);
    let size = (15.0 * o).round();
    let line_h = (17.0 * o).round();
    let (bx, by, bw) = (x + 18.0 * o, y + 45.0 * o, 598.0 * o);
    let lines_of = |t: &str| t.split('\n').flat_map(|p| if p.trim().is_empty() { vec![String::new()] } else { wrap(p, bw - 32.0 * o, size) }).collect::<Vec<_>>();
    let mut lines: Vec<(String, Color)> = lines_of(&about).into_iter().map(|t| (t, BOX_INK)).collect();
    if !affordable {
        lines.push((String::new(), BOX_INK));
        let short = own_text("Building", "NoMoneyForShip", n_("You have no money to hire a ship right now!"));
        lines.extend(lines_of(&short).into_iter().map(|t| (t, chrome::RED_TEXT)));
    }
    let text_h = lines.len() as f32 * line_h;
    // As tall as the text with 14 above and below, at most 193; a longer text is centred in
    // it and cut at both ends (0x4bbc84).
    let bh = (text_h + 28.0 * o).min(193.0 * o);
    let top = if text_h + 28.0 * o <= 193.0 * o { 14.0 * o } else { (bh - text_h) / 2.0 };
    chrome::text_box(Rect::new(bx, by, bw, bh));
    for (i, (t, c)) in lines.iter().enumerate() {
        let ly = by + top + i as f32 * line_h;
        if ly >= by && ly + line_h <= by + bh {
            chrome::shadow_centered(t, bx + bw / 2.0, ly + size, size, *c);
        }
    }
    let cost = format!("{} = {price}", own_text("Building", "CostShip", n_("Price of hiring a ship")));
    chrome::shadow_centered(&cost, x + w / 2.0, by + bh + 17.0 * o + size, size, chrome::GOLD);
    let (btn_w, btn_h) = chrome::win("Btn3Up").map_or((180.0, 40.0), |t| (t.width(), t.height()));
    let (btn_w, btn_h) = (btn_w * o, btn_h * o);
    let btn_y = y + h - 10.0 * o - btn_h;
    let buy = own_text("Building", "BuyShip", n_("Hire a ship"));
    let cancel = own_text("Buttons", "Cancel", n_("Cancel"));
    if button(x + 10.0 * o, btn_y, btn_w, btn_h, &buy, affordable) {
        if game.rent_ship().is_ok() {
            cue(Cue::Gold);
        }
        return Some(Screen::WorldMap);
    }
    let cancelled = button(x + w - btn_w - 10.0 * o, btn_y, btn_w, btn_h, &cancel, true);
    if closed {
        cue(Cue::Button);
    }
    // Esc closes it as "Отмена" does (0x4cd8d0), with the button sound the replay hears when
    // Esc closes the village window (tools/difftest/AV.md §5).
    if key(KeyCode::Escape) {
        cue(Cue::Button);
    }
    (cancelled || closed || key(KeyCode::Escape)).then_some(Screen::WorldMap)
}

/// The building window. `Exit` (or Escape) returns to the map.
pub fn frame(game: &mut Game, assets: &Assets, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    // The village window is modal, the bar's buttons greyed under it, as the original's
    // (the audit's capture of «Деревня Замковая»); the building window keeps the bar live.
    let bar = if game.tabs_here().contains(&Tab::Tribute) {
        world_view::backdrop(game, assets);
        None
    } else {
        world_view::window_backdrop(game, assets, None)
    };
    // Events that happened meanwhile (an answer's follow-ups …).
    let pending = game.drain_events();
    let early = world_view::handle_events(game, pending, message, dialogs);
    let tabs = game.tabs_here();
    if tabs.is_empty() {
        return Some(Screen::WorldMap);
    }
    // A shipyard and a village open their own small windows, not the building window.
    if tabs == [Tab::Shipyard] {
        return early.or(ship_window(game)).or(with_back(bar, view));
    }
    if tabs.contains(&Tab::Tribute) {
        // An offer waiting for its answer is asked in the event window first (0x4aca80).
        if let Some(o) = game.village_offer() {
            if !dialogs.iter().any(|d| d.offer) {
                dialogs.push_back(offer_dialog(game, o, false));
            }
            return early.or(Some(Screen::WorldMap));
        }
        return early.or(village_window(game, view, message)).or(with_back(bar, view));
    }
    if !tabs.contains(&view.tab) {
        view.switch(tabs[0]);
    }
    let f = window();
    let k = chrome::k();
    let (_, close) = chrome::window_with_title_color(Rect::new(f.x, f.y, f.w, f.h), &title(game), chrome::Skin::Marble, true, BUILDING_AND_ITEM_NAME);
    chrome::silver_frame(card_panel_lower(&f), k.max(1.0));

    // Tabs, on light parchment.
    let col = window_layout::left(Rect::new(f.x, f.y, f.w, f.h));
    chrome::paper_panel(col);
    chrome::silver_frame(col, k.max(1.0));
    let (tw, th) = ((col.w - 12.0 * k).min(232.0 * k), 88.0 * k);
    let tx = col.x + (col.w - tw) / 2.0;
    let pitch = (th + 6.0 * k).min((col.h - th - 20.0 * k) / tabs.len().max(1) as f32);
    // A tab's place under the pointer brings the tabs back over the hire tab's card
    // (0x4bb6c8 → 0x4bc98c), the Exit's too.
    let exit_r = Rect::new(tx, col.y + col.h - th - 5.0 * k, tw, th);
    let tab_rects: Vec<Rect> = (0..tabs.len()).map(|i| Rect::new(tx, col.y + 5.0 * k + i as f32 * pitch, tw, th)).chain([exit_r]).collect();
    if tab_rects.iter().any(|r| mouse_in(r.x, r.y, r.w, r.h)) {
        view.hire_panel = hire_panel_on(view.hire_panel, Some(Entered::Tab), None);
    }
    for (i, &t) in tabs.iter().enumerate() {
        let r = tab_rects[i];
        if tab_button(tab_label(t), Some(t), r, view.tab == t) && view.tab != t && view.garrison_sel.is_none() {
            // The tab is highlighted with the cast sound (interface.md §9.8, §14).
            cue(Cue::CastSpell);
            view.switch(t);
            *message = None;
            if t == Tab::Garrison {
                game.open_garrison();
            }
            // The market opens on the goods when the shop has some, else on the sell list.
            if t == Tab::Market {
                view.selling = game.market_here().is_none_or(|g| g.is_empty());
            }
        }
    }
    let exit = tab_button(tr("Exit"), None, exit_r, false) && view.garrison_sel.is_none();

    let mut next = early;
    match view.tab {
        Tab::MainHall => next = next.or(main_hall(game, assets, &f, view, message, dialogs)),
        Tab::Barracks => next = next.or(barracks(game, assets, &f, view, message, dialogs)),
        Tab::Garrison => garrison(game, assets, &f, view, message),
        Tab::Market => next = next.or_else(|| market(game, assets, &f, view, message, dialogs)),
        Tab::Sanctuary => next = next.or_else(|| sanctuary(game, &f, view, message, dialogs)),
        Tab::Tribute | Tab::Shipyard => {}
    }
    chrome::silver_frame(window_layout::upper(Rect::new(f.x, f.y, f.w, f.h)), k.max(1.0));
    selected_unit_panel(game, assets, &f, view);
    if let Some(m) = message {
        let w = measure(m, 20.0).width + 40.0;
        let (cx, y) = (f.cx + f.cw / 2.0, f.y + f.h + 6.0);
        draw_rectangle(cx - w / 2.0, y, w, 30.0, PANEL);
        text_centered(m, cx, y + 21.0, 20.0, ACCENT);
    }
    let filter_keys = FILTER_KEYS.with(|f| f.replace(false));
    let escape = !filter_keys && key(KeyCode::Escape);
    if escape && view.garrison_sel.is_some() {
        view.garrison_sel = None;
        return next.or(with_back(bar, view));
    }
    if close || exit || escape {
        // The building window's close is silent.
        *message = None;
        return Some(Screen::WorldMap);
    }
    next.or(with_back(bar, view))
}

/// The bar's army button opens the army screen with the way back here.
fn with_back(bar: Option<Screen>, view: &BuildingView) -> Option<Screen> {
    bar.map(|b| match b {
        Screen::Squad { selected, scroll, .. } => Screen::Squad { selected, scroll, back: Some(view.clone()) },
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::{hire_panel_on, tab_state, village_ok_key, Entered, HirePanel};
    use razdor::rules::content::UnitId;

    #[test]
    fn a_hire_offer_under_the_pointer_shows_its_card_until_the_tabs_come_back() {
        // 0x4c6964: entering an offer paints its card over the tabs; 0x4c72a4: leaving it
        // keeps the card.
        let archer = UnitId(7);
        let card = hire_panel_on(None, Some(Entered::Recruit(archer)), None);
        assert_eq!(card, Some(HirePanel::Recruit(archer)));
        assert_eq!(hire_panel_on(card, None, None), card, "the card stays after the pointer leaves");
        // 0x4c612c: the hero's grid with nothing selected brings the tabs back (0x4bc98c),
        // over a card or an empty cell alike.
        assert_eq!(hire_panel_on(card, Some(Entered::SquadCell(Some(2))), None), None);
        assert_eq!(hire_panel_on(card, Some(Entered::SquadCell(None)), None), None);
        // With a unit selected: the unit under the pointer, else the selected one.
        assert_eq!(hire_panel_on(card, Some(Entered::SquadCell(Some(2))), Some(4)), Some(HirePanel::Squad(2)));
        assert_eq!(hire_panel_on(card, Some(Entered::SquadCell(None)), Some(4)), Some(HirePanel::Squad(4)));
        // 0x4bb6c8: a tab's place brings the tabs back.
        assert_eq!(hire_panel_on(card, Some(Entered::Tab), Some(4)), None);
    }

    #[test]
    fn the_hire_hint_names_the_offer_or_warns_none_are_left() {
        // 0x4c6964: line 8 with the name and the button in quotes; line 10 in the warning
        // colour for an offer with none left.
        let (line, warn) = super::recruit_hint("Archer", true);
        assert!(!warn);
        assert!(line.contains("\"Archer\""), "{line}");
        let (_, warn) = super::recruit_hint("Archer", false);
        assert!(warn);
    }

    #[test]
    fn the_village_ok_takes_a_key_pressed_while_it_is_up() {
        // 0x4cd81c: the key-down presses «Ок», its release clicks it.
        assert!(!village_ok_key(false, true), "a release without a press while open");
        assert!(!village_ok_key(true, false));
        assert!(village_ok_key(false, true));
        assert!(!village_ok_key(false, true), "once");
    }

    #[test]
    fn a_tab_lights_under_the_mouse_as_the_original() {
        // 0x4d9060: TBo under the mouse, TBd held or open, TB- else.
        assert_eq!(tab_state(false, false, false), "TB-");
        assert_eq!(tab_state(false, true, false), "TBo");
        assert_eq!(tab_state(false, true, true), "TBd");
        assert_eq!(tab_state(true, false, false), "TBd");
    }


    #[test]
    fn market_rows_go_cheapest_first_ties_in_stock_order() {
        assert_eq!(super::cheapest_first(&[2750, 605, 110, 495, 110]), vec![2, 4, 3, 1, 0]);
        assert!(super::cheapest_first::<i32>(&[]).is_empty());
    }

    /// 0x4bca8c: a market list opens with its first row picked, an empty one with none.
    #[test]
    fn a_market_list_opens_on_its_first_row() {
        assert_eq!(super::first_pick(7), Some(0));
        assert_eq!(super::first_pick(0), None);
    }

    /// The garrison tab's press: with dragging only (Razdor's default) it selects the unit
    /// pressed or clears on an empty cell, never moving one; as in the original it acts
    /// (0x4c653c, 0x4c6f50).
    #[test]
    fn a_garrison_press_follows_the_arrangement_setting() {
        use super::{garrison_click, GarrisonClick, Hit};
        use crate::ui::unit_drag::Controls::{Drag, Original};
        let cell = Hit::Cell(Slot { row: Row::Front, col: 0 });
        assert_eq!(garrison_click(Drag, Some(Hit::Unit(2)), None), GarrisonClick::Select((true, 2)));
        assert_eq!(garrison_click(Drag, None, Some(Hit::Unit(0))), GarrisonClick::Select((false, 0)));
        assert_eq!(garrison_click(Drag, Some(cell), None), GarrisonClick::Clear);
        assert_eq!(garrison_click(Drag, None, Some(cell)), GarrisonClick::Clear);
        assert_eq!(garrison_click(Drag, None, None), GarrisonClick::Nothing);
        assert_eq!(garrison_click(Original, Some(Hit::Unit(2)), None), GarrisonClick::Act);
        assert_eq!(garrison_click(Original, None, Some(cell)), GarrisonClick::Act);
    }

    use super::{market_list_buttons, next_pick, ListButton, offer_picture, recruit_x, screen_line, stock_caption, stock_picture};

    /// 0x4ba854 case 0: pictures only for building types 1, 3-7, 10 and 12; the palace, the
    /// smithy and the dungeon entrance keep the window's background (0x4bb7c4).
    #[test]
    fn the_main_hall_has_no_picture_for_the_palace_smithy_and_entrance() {
        use super::hall_picture;
        use razdor::rules::world::LocationKind as K;
        for kind in [K::Palace, K::Smithy, K::Entrance] {
            assert_eq!(hall_picture(kind, (11, 1)), None, "{kind:?}");
        }
        assert_eq!(hall_picture(K::Town, (1, 1)), Some("S_Town"));
        assert_eq!(hall_picture(K::Altar, (10, 2)), Some("S_Grave"));
        assert_eq!(hall_picture(K::Ruins, (12, 3)), Some("S_RuinedHouse"));
    }
    use razdor::rules::economy::VillageOffer;
    use razdor::rules::formation::{Formation, Row, Slot};

    /// 0x4d11ef / 0x4d1247: a stock of 0 shows the `_Empty` picture; 0x4d0f7a: " + " above 0,
    /// " = " at 0.
    #[test]
    fn a_village_stock_of_zero_shows_the_empty_picture_and_an_equals_sign() {
        assert_eq!(stock_picture(120, false), "Village_Gold");
        assert_eq!(stock_picture(0, false), "Village_Gold_Empty");
        assert_eq!(stock_picture(7, true), "Village_Mana");
        assert_eq!(stock_picture(0, true), "Village_Mana_Empty");
        assert_eq!(stock_caption("Деньги", 120), "Деньги + 120");
        assert_eq!(stock_caption("Магия", 0), "Магия = 0");
    }

    /// 0x4aca80: the offer's kind (1 blessing … 5 innkeeper) picks `Village_Bonus_<kind>`;
    /// the `A` pictures are never shown.
    #[test]
    fn each_village_offer_shows_its_bonus_picture() {
        use VillageOffer::*;
        let names = [Blessing, Priest, Furs, Witch, Innkeeper].map(offer_picture);
        assert_eq!(names, ["Village_Bonus_1", "Village_Bonus_2", "Village_Bonus_3", "Village_Bonus_4", "Village_Bonus_5"]);
    }

    /// Two recruits stand at a third and two thirds of the picture, as on the original's
    /// screen (centres near 427 and 651); six fill it.
    #[test]
    fn recruits_spread_over_the_picture_by_their_number() {
        let centre = |i, n| recruit_x(i, n) + 44.0;
        assert_eq!((centre(0, 2), centre(1, 2)), (428.0, 652.0));
        assert!(recruit_x(0, 6) >= super::CARD_PANEL_X && recruit_x(5, 6) + 88.0 <= super::CARD_PANEL_X + super::CARD_PANEL_W);
    }

    #[test]
    fn the_garrison_grid_is_mirrored_the_heros_is_not() {
        // 0x4d9060: the hero's front on the top line of his grid, the garrison's on the bottom
        // line of its grid, the two fronts facing each other across the divider.
        for form in [Formation::WIDE, Formation::VANILLA] {
            let front = form.display(Slot::new(Row::Front, 1)).0;
            let back = form.display(Slot::new(Row::Back, 1)).0;
            assert_eq!((screen_line(form, front, true), screen_line(form, back, true)), (0, 1));
            assert_eq!((screen_line(form, front, false), screen_line(form, back, false)), (1, 0));
        }
    }

    #[test]
    fn after_a_buy_the_selection_moves_to_the_next_item_or_the_one_above() {
        assert_eq!(next_pick(2, 5), Some(2), "the item below moved up into row 2");
        assert_eq!(next_pick(4, 4), Some(3), "the last one bought: the one above");
        assert_eq!(next_pick(0, 0), None, "nothing left");
    }

    /// The original's two list buttons (0x4bca8c): "Инвентарь" shows the pack and is locked
    /// while it is shown; "Торговая лавка" shows the goods and is locked while they are shown
    /// or the shop has none, drawn with the disabled picture (0x479bbc; an owner's report).
    /// Neither opens another window.
    #[test]
    fn the_market_s_list_buttons_are_the_original_s() {
        let b = |shows_pack, enabled| ListButton { shows_pack, enabled };
        assert_eq!(market_list_buttons(false, 5), [b(true, true), b(false, false)], "goods shown: Торговая лавка locked");
        assert_eq!(market_list_buttons(true, 5), [b(true, false), b(false, true)], "pack shown: Инвентарь locked");
        assert_eq!(market_list_buttons(true, 0), [b(true, false), b(false, false)], "an empty shop: both locked");
    }

    /// 0x4d9df3..0x4d9eaa: the market's columns 27 + 186 + 70 px, the prices centred in
    /// theirs (align 2, 0x4bcd03), each title centred over its list (0x48e750).
    #[test]
    fn market_prices_stand_centred_in_their_column() {
        let c = super::market_columns(283.0 + 18.0, 18.0);
        assert_eq!(c, super::ListColumns { icon: 27.0, name_x: 35.0, names_cx: 120.0, price_cx: 248.0 });
        let half = super::market_columns(141.5 + 9.0, 9.0);
        assert_eq!(half.price_cx, 124.0, "scaled with the list");
    }

    /// 0x4d9fe6..0x4da0ae: the sanctuary's picture strip 42 px, names 204, prices 70; the
    /// pictures shrunk from the 108 × 628 strip (0x4bc42a).
    #[test]
    fn the_sanctuary_list_is_laid_out_as_the_original() {
        let c = super::list_columns(316.0, 0.0, [super::SPELL_ICON_W, super::SPELL_NAME_W, super::SPELL_PRICE_W]);
        assert_eq!(c, super::ListColumns { icon: 42.0, name_x: 50.0, names_cx: 144.0, price_cx: 281.0 });
        let (dx, dy, side) = super::spell_icon_place(108.0, 104.0);
        assert_eq!((dx, dy, side), (4.0, 4.0, 100.0));
    }

    /// The lists' paper and highlights (0x4bddf0, 0x4bb3c3, 0x4bb592, 0x4d9e33, 0x4da030)
    /// and their tints (0x4bca8c, 0x4ba854): no orange for a spell the gold does not reach.
    #[test]
    fn the_lists_take_the_original_s_colours() {
        assert_eq!(super::MARKET_LIST_TAKEN, [24, 24, 24]);
        assert_eq!(super::MARKET_PICK_ADDED, [64, 64, 64]);
        assert_eq!(super::SPELL_LIST_TAKEN, [148, 132, 148]);
        assert_eq!(super::SPELL_EMPTY_TAKEN, [180, 140, 113]);
        assert_eq!(super::SPELL_PICK_ADDED, [24, 32, 24]);
        let rgb = |c: macroquad::prelude::Color| [c.r, c.g, c.b].map(|v| (v * 255.0).round() as u8);
        assert_eq!(rgb(super::market_ink(true)), [255, 175, 85]);
        assert_eq!(rgb(super::market_ink(false)), [255, 255, 155]);
        assert_eq!(rgb(super::spell_ink(false)), [255, 255, 155]);
        assert_eq!(rgb(super::spell_ink(true)), [55, 180, 255]);
    }

    /// Razdor's addition (not the original's): a click on «Название» sorts А→Я, again Я→А,
    /// again back to the list's own order; on «Цена» the dearest first, then the cheapest,
    /// then the own order; a new window starts in the own order.
    #[test]
    fn titles_cycle_the_list_s_sort() {
        use super::{Column, ListSort};
        assert_eq!(super::BuildingView::new(razdor::rules::town::Tab::Market).sort, ListSort::Shown);
        let name = |s: ListSort| s.clicked(Column::Name);
        let price = |s: ListSort| s.clicked(Column::Price);
        assert_eq!(name(ListSort::Shown), ListSort::NameUp);
        assert_eq!(name(ListSort::NameUp), ListSort::NameDown);
        assert_eq!(name(ListSort::NameDown), ListSort::Shown);
        assert_eq!(name(ListSort::PriceUp), ListSort::NameUp);
        assert_eq!(price(ListSort::Shown), ListSort::PriceDown);
        assert_eq!(price(ListSort::PriceDown), ListSort::PriceUp);
        assert_eq!(price(ListSort::PriceUp), ListSort::Shown);
        assert_eq!(price(ListSort::NameDown), ListSort::PriceDown);
    }

    #[test]
    fn rows_sort_by_name_in_the_russian_alphabet_and_by_price() {
        use super::{sorted_rows, ListSort};
        let names = ["Ёлка", "яблоко", "Евангелие", "Ангел", "жезл"];
        let prices = [30, 10, 50, 10, 20];
        let own = [4, 3, 2, 1, 0];
        assert_eq!(sorted_rows(&own, &names, &prices, ListSort::Shown), own);
        assert_eq!(sorted_rows(&own, &names, &prices, ListSort::NameUp), [3, 2, 0, 4, 1], "А, Е, Ё, Ж, Я; case left out");
        assert_eq!(sorted_rows(&own, &names, &prices, ListSort::NameDown), [1, 4, 0, 2, 3]);
        assert_eq!(sorted_rows(&own, &names, &prices, ListSort::PriceDown), [2, 0, 4, 3, 1], "equal prices keep the own order");
        assert_eq!(sorted_rows(&own, &names, &prices, ListSort::PriceUp), [3, 1, 4, 0, 2]);
    }

    #[test]
    fn a_sort_keeps_the_picked_item() {
        assert_eq!(super::same_pick(&[4, 3, 2], &[2, 4, 3], Some(1)), Some(2));
        assert_eq!(super::same_pick(&[4, 3, 2], &[2, 4, 3], None), None);
    }
}
