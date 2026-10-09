//! The building window (town, castle, fort, village, church, market, tavern …), laid out as
//! the original's (video notes §2): a title bar, a column of tabs on the left, the tab's
//! content on the right, and the building's description at the bottom.

use std::collections::VecDeque;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::content::{ArtefactType, ItemId, SpellDef};
use razdor::rules::formation::{Formation, Slot};
use razdor::rules::game::{Currency, Game, HireError, TradeError, SPELL_BOOK_SIZE};
use razdor::rules::items::describe;
use razdor::rules::script::HallEntry;
use razdor::rules::town::{ServiceError, Tab};
use razdor::rules::units::Unit;
use razdor::rules::world::LocationKind;

use super::assets::Assets;
use super::chrome;
use super::audio::{cue, cued, Cue};
use super::dialog::{resource_icon, Dialog, Resource, MANA};
use super::items_view::{level_gains, unit_stat_lines};
use super::screens::stat_lines;
use super::story;
use super::widgets::*;
use super::world_view;
use super::Screen;

const PARCHMENT_INK: Color = Color::new(0.45, 0.28, 0.14, 1.0);
const BOX_INK: Color = Color::new(1.0, 0.86, 0.58, 1.0);
const SILVER: Color = Color::new(0.78, 0.78, 0.82, 1.0);
const TAB_RED: Color = Color::new(0.75, 0.18, 0.12, 1.0);

/// State of the building window between frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildingView {
    pub tab: Tab,
    /// Selected row of the market or sanctuary list.
    pub pick: Option<usize>,
    pub scroll: usize,
    /// The market shows the "sell" shop (the pack) instead of the goods.
    pub selling: bool,
    /// The garrison tab's selected unit: (in the garrison, index).
    pub garrison_sel: Option<(bool, usize)>,
    /// The garrison unit being bought back: (index, cell, price), waiting for the answer.
    pub garrison_buy: Option<(usize, Slot, i32)>,
    /// The village window took the tribute: its close plays `Item-Gold` (0x4c604a).
    pub tribute_paid: bool,
    /// A card animation of the hire tab under way.
    anim: Option<CardAnim>,
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
        BuildingView { tab, pick: None, scroll: 0, selling: false, garrison_sel: None, garrison_buy: None, tribute_paid: false, anim: None }
    }

    fn switch(&mut self, tab: Tab) {
        *self = BuildingView { tribute_paid: self.tribute_paid, ..BuildingView::new(tab) };
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
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let w = (836.0 * k).min(sw - 8.0).round();
    let h = (600.0 * k).min(sh - chrome::bar_height() - 4.0).round();
    let (x, y) = (((sw - w) / 2.0).round(), ((sh - chrome::bar_height() - h) / 2.0).max(2.0).round());
    Frame { x, y, w, h, cx: x + 256.0 * k, cy: y + 34.0 * k, cw: w - 264.0 * k }
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

/// A parchment tab in the left column (`tab` `None` is the exit). Returns true when clicked.
fn tab_button(label: &str, tab: Option<Tab>, r: Rect, active: bool) -> bool {
    let hover = mouse_in(r.x, r.y, r.w, r.h);
    let state = if active {
        "TBd"
    } else if hover {
        "TBo"
    } else {
        "TB-"
    };
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
    hover && clicked()
}

/// The red-brown text box with the building's description.
fn description_box(desc: &str, x: f32, y: f32, w: f32, h: f32) {
    let k = chrome::k();
    chrome::text_box(Rect::new(x, y, w, h));
    let lines = wrap(desc, w - 60.0 * k, 19.0 * k);
    let top = y + (h - lines.len() as f32 * 23.0 * k) / 2.0 + 16.0 * k;
    for (i, line) in lines.iter().enumerate() {
        chrome::shadow_centered(line, x + w / 2.0, top + i as f32 * 23.0 * k, 19.0 * k, BOX_INK);
    }
}

/// Main hall: the building's picture, the rumours on offer (heard for free; a rumour's own event may cost gold) and this
/// building's quests, the description.
fn main_hall(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    let k = chrome::k();
    let l = game.location?;
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let pic_h = 250.0 * k;
    {
        let loc = &game.world.locations[l];
        draw_rectangle(x, y, w, pic_h, Color::new(0.35, 0.5, 0.65, 1.0));
        draw_rectangle(x, y + pic_h * 0.62, w, pic_h * 0.38, Color::new(0.35, 0.5, 0.3, 1.0));
        // The original's picture of this kind of building, else its map sprite.
        let scene = match loc.kind {
            LocationKind::Town | LocationKind::Palace => Some("S_Town"),
            LocationKind::Castle | LocationKind::Fort => Some("S_Castle"),
            LocationKind::Church => Some("S_Church"),
            LocationKind::Market | LocationKind::Smithy => Some("S_Market"),
            LocationKind::Tavern => Some("S_Tavern"),
            LocationKind::Village => Some("S_Village"),
            LocationKind::Ruins => Some("S_Ruin"),
            _ => None,
        };
        if let Some(t) = scene.and_then(chrome::win) {
            let src_h = (t.width() * pic_h / w).min(t.height());
            chrome::tex_src(&t, Rect::new(0.0, (t.height() - src_h) / 2.0, t.width(), src_h), Rect::new(x, y, w, pic_h), WHITE);
        } else if let Some(tex) = assets.dt.as_ref().and_then(|a| a.building(loc.picture.0, loc.picture.1)) {
            let sc = ((pic_h - 20.0 * k) / tex.height()).min((w - 20.0 * k) / tex.width()).min(2.5 * k);
            let (tw, th) = (tex.width() * sc, tex.height() * sc);
            draw_texture_ex(&tex, x + (w - tw) / 2.0, y + pic_h - th - 6.0 * k, WHITE, DrawTextureParams { dest_size: Some(vec2(tw, th)), ..Default::default() });
        } else {
            text_centered(loc.kind.label(), x + w / 2.0, y + pic_h / 2.0, 40.0 * k, INK);
        }
        chrome::silver_frame(Rect::new(x, y, w, pic_h), 1.0);
    }
    let entries = game.hall_entries();
    if view.pick.is_some_and(|k| k >= entries.len()) {
        view.pick = None;
    }
    let picked = view.pick.and_then(|k| entries.get(k).copied());
    let ly = y + pic_h + 12.0 * k;
    draw_rectangle(x, ly, w, 36.0 * k, Color::new(0.0, 0.0, 0.0, 0.3));
    chrome::silver_frame(Rect::new(x, ly, w, 36.0 * k), 1.0);
    chrome::shadow_text(tr("Quests and rumours:"), x + 14.0 * k, ly + 25.0 * k, 20.0 * k, chrome::GOLD);
    let mut next = None;
    let label = if matches!(picked, Some(HallEntry::Quest(_))) { tr("Take quest") } else { tr("Hear rumour") };
    if button(x + w - 250.0 * k, ly + 3.0 * k, 240.0 * k, 30.0 * k, label, picked.is_some()) {
        if let Some(HallEntry::Rumour(id) | HallEntry::Quest(id)) = picked {
            match game.take_hall_entry(id) {
                Ok(events) => {
                    *message = None;
                    view.pick = None;
                    next = world_view::handle_events(game, events, message, dialogs);
                }
                Err(e) => *message = Some(service_error(e)),
            }
        }
    }
    let list_y = ly + 42.0 * k;
    let (row_h, rows) = (24.0 * k, 5);
    let list_h = rows as f32 * row_h + 12.0 * k;
    chrome::parchment(Rect::new(x, list_y, w, list_h), false);
    if entries.is_empty() {
        let none = if game.script().is_some() { tr("Nothing is on offer here.") } else { tr("No quests in the demo.") };
        text_centered(none, x + w / 2.0, list_y + list_h / 2.0 + 6.0 * k, 19.0 * k, PARCHMENT_INK);
    }
    let max_scroll = entries.len().saturating_sub(rows);
    if mouse_in(x, list_y, w, list_h) {
        let wh = wheel();
        if wh < 0.0 {
            view.scroll = (view.scroll + 1).min(max_scroll);
        } else if wh > 0.0 {
            view.scroll = view.scroll.saturating_sub(1);
        }
    }
    view.scroll = view.scroll.min(max_scroll);
    for (n, entry) in entries.iter().enumerate().skip(view.scroll).take(rows) {
        let ry = list_y + 6.0 * k + (n - view.scroll) as f32 * row_h;
        let (id, note, color) = match *entry {
            HallEntry::Rumour(id) => (id, tr("rumour"), Color::new(0.55, 0.1, 0.1, 1.0)),
            HallEntry::Quest(id) => (id, tr("quest"), Color::new(0.1, 0.3, 0.55, 1.0)),
        };
        if view.pick == Some(n) {
            draw_rectangle(x + 4.0 * k, ry, w - 8.0 * k, row_h - 2.0 * k, Color::new(0.72, 0.6, 0.4, 1.0));
        }
        let title: String = story::event_title(game, id).chars().take(60).collect();
        text_fit(&title, x + 16.0 * k, ry + 18.0 * k, w - 48.0 * k - measure(note, 16.0 * k).width, 19.0 * k, color);
        text(note, x + w - 16.0 * k - measure(note, 16.0 * k).width, ry + 17.0 * k, 16.0 * k, PARCHMENT_INK);
        if mouse_in(x, ry, w, row_h) && clicked() {
            view.pick = Some(n);
        }
    }
    let dy = list_y + list_h + 10.0 * k;
    let desc = game.world.locations[l].description.clone();
    description_box(&desc, x, dy, w, f.y + f.h - dy - 10.0 * k);
    next
}

/// Barracks: recruits for hire along the top, the counters, the army with heal and
/// resurrect buttons below.
/// A point of the building window in pixels of the 960×720 video (the window at 60, 20).
fn at(f: &Frame, x: f32, y: f32, w: f32, h: f32) -> Rect {
    let k = chrome::k();
    Rect::new(f.x + x * k, f.y + y * k, w * k, h * k)
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
        super::dt_font::with_face(super::dt_font::Face::Title, || {
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
    let back = at(f, 250.0, 28.0, 584.0, 270.0);
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
    if recruits.is_empty() {
        chrome::shadow_centered(tr("No recruits here."), back.center().x, back.y + 90.0 * k, 18.0 * k, chrome::CREAM);
    }
    for (i, r) in recruits.iter().take(6).enumerate() {
        let face = at(f, 258.0 + i as f32 * 96.0, 74.0, 88.0, 88.0);
        draw_rectangle(face.x + 3.0 * k, face.y + 3.0 * k, face.w, face.h, Color::new(0.0, 0.0, 0.0, 0.45));
        assets.draw_portrait(r.unit, Team::Player, face);
        draw_rectangle_lines(face.x, face.y, face.w, face.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.9));
        if mouse_in(face.x, face.y, face.w, face.h) {
            let def = c.unit(r.unit);
            hover_lines.push((def.name.clone(), ACCENT));
            hover_lines.push((level_label(1, 0, c.xp_to_next(r.unit, 1)), XP_COLOR));
            hover_lines.extend(stat_lines(&c, r.unit).into_iter().map(|s| (s, INK)));
            hover_lines.push((trf!("Per level: {gains}", gains = level_gains(&c, r.unit)), INK));
            hover_lines.push((trf!("Daily wage {wage}", wage = c.wage_for(r.unit, razdor::rules::content::WageKind::of(def))), Color::new(0.95, 0.6, 0.25, 1.0)));
            match r.stock {
                Some(n) => hover_lines.push((trf!("{n} of {max} left", n, max = r.max), INK)),
                None => hover_lines.push((tr("always").to_string(), INK)),
            }
        }
        let price = game.hire_price(r.unit);
        let stock_left = r.stock != Some(0);
        let can = hires && stock_left && game.can_afford(price) && game.squad.len() < game.max_squad();
        let pill = Rect::new(face.x + 4.0 * k, face.y + face.h + 2.0 * k, face.w - 8.0 * k, 17.0 * k);
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
    barracks_counters(game, at(f, 258.0, 242.0, 572.0, 48.0));
    chrome::divider(at(f, 250.0, 302.0, 588.0, 16.0));

    // The army: the army screen's cards, a heal or raise button with its price on the strip.
    let heals = game.heals_here();
    let raises = game.resurrects_here();
    let form = c.formation;
    let lines = form.display_lines() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / form.display_cols() as f32);
    let (card, pitch) = (vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k));
    let grid = at(f, 250.0, 330.0, 588.0, 0.0);
    let gx = (grid.x + (grid.w - (form.display_cols() as f32 * pitch.x - 8.0 * cs * k)) / 2.0).round();
    let cell_at = |slot: Slot| {
        let (line, col) = form.display(slot);
        vec2(gx + col as f32 * pitch.x, grid.y + line as f32 * pitch.y).round()
    };
    // A press on a card or an empty cell of the hero's grid (0x4c653c), acted on below.
    let mut pressed = None;
    for slot in form.slots() {
        if !game.squad.iter().any(|u| u.slot == slot) {
            let p = cell_at(slot);
            chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(form, slot), true);
            if mouse_in(p.x, p.y, card.x, card.y) && clicked() {
                pressed = Some((None, slot));
            }
        }
    }
    let mut action = None;
    let anim = view.anim_at();
    let selected = view.garrison_sel.filter(|&(g, i)| !g && i < game.squad.len()).map(|s| s.1);
    for i in 0..game.squad.len() {
        let u = game.squad[i].clone();
        let mut p = cell_at(u.slot);
        if let Some((CardAnimKind::Hired { unit, recruit }, t)) = anim {
            if unit == i {
                let from = at(f, 258.0 + recruit as f32 * 96.0, 74.0, 88.0, 88.0);
                p = vec2(from.x, from.y).lerp(p, t).round();
            }
        }
        if let Some((CardAnimKind::Slid { unit, from, .. }, t)) = anim {
            if unit == i {
                p = vec2(from.0 as f32, from.1 as f32).lerp(p, t).round();
            }
        }
        let sq = Rect::new(p.x, p.y, card.x, card.x);
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.45));
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
                if chrome::pill_button(pill, &label, game.can_pay_service(i, pr), false) {
                    action = Some((i, raise));
                }
                let cost = trf!("Price {price}", price = pr.amount);
                chrome::shadow_centered(&cost, strip.center().x, pill.y + pill.h + 13.0 * k, 12.0 * k, chrome::GOLD);
            }
            _ => super::unit_sheet::stat_strip(strip, &vs, &vs, vs[razdor::rules::content::Stat::MagicPower], super::unit_sheet::caster(&c, u.def), super::unit_sheet::strip_place(form, u.slot), u.hp, back_row_def(&c, u.slot), false, super::unit_sheet::StripPanel::of_squad(i, u.named)),
        }
        if !u.alive() {
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.0, 0.0, 0.0, 0.55));
            draw_line(sq.x + 10.0, sq.y + 10.0, sq.x + sq.w - 10.0, sq.y + sq.h - 10.0, 3.0, RED);
            draw_line(sq.x + sq.w - 10.0, sq.y + 10.0, sq.x + 10.0, sq.y + sq.h - 10.0, 3.0, RED);
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
                chrome::effect("Battle/--CURE.ugs", sq.center(), sq.w * 1.6, t, WHITE);
            }
        }
        if selected == Some(i) {
            chrome::glow_frame(sq, Color::new(0.35, 1.0, 0.35, 1.0), true);
        }
        if mouse_in(sq.x, sq.y, sq.w, sq.h) {
            if clicked() {
                pressed = Some((Some(i), u.slot));
            }
            chrome::glow_frame(sq, Color::new(0.35, 0.55, 1.0, 0.9), false);
            hover_lines = vec![(game.squad_label(i), ACCENT)];
            hover_lines.extend(unit_stat_lines(&c, &u, game.wage(i)).into_iter().map(|s| (s, INK)));
        }
    }
    // The press, as the original's hero grid within the hero's army (0x4c653c); none while a
    // card moves (busy 0x68dc63).
    if let (Some((on, slot)), None) = (pressed, anim) {
        let press = super::unit_drag::grid_press(selected, on);
        view.garrison_sel = match press {
            super::unit_drag::GridPress::Select(i) => Some((false, i)),
            super::unit_drag::GridPress::Nothing => view.garrison_sel,
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

/// The signs from a card's top left (493a64): the promotion for a unit of the hero's army
/// (`own`, not the hero) that can take one, then a drunk potion.
fn top_left_signs(c: &razdor::rules::content::Content, sq: Rect, u: &Unit, own: bool) {
    let upgrade = own && u.upgrade_tree(c).iter().any(|&(_, _, ok)| ok);
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
fn card_grid(game: &Game, assets: &Assets, f: &Frame, rel_y: f32, units: &[&Unit], own: bool, selected: Option<usize>) -> Option<Hit> {
    let k = chrome::k();
    let c = &game.content;
    let form = c.formation;
    let lines = form.display_lines() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / form.display_cols() as f32);
    let (card, pitch) = (vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k));
    let grid = at(f, 250.0, rel_y, 584.0, 0.0);
    let gx = (grid.x + (grid.w - (form.display_cols() as f32 * pitch.x - 8.0 * cs * k)) / 2.0).round();
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
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.45));
        assets.draw_portrait(u.def, Team::Player, sq);
        chrome::wounds(sq, u.hp, u.max_hp(c));
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let vs = u.stats(c);
        super::unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[razdor::rules::content::Stat::MagicPower], super::unit_sheet::caster(c, u.def), super::unit_sheet::strip_place(form, u.slot), u.hp, back_row_def(c, u.slot), false, if own { super::unit_sheet::StripPanel::of_squad(i, u.named) } else { super::unit_sheet::StripPanel::Plain });
        if !u.alive() {
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.0, 0.0, 0.0, 0.55));
        } else if u.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        top_left_signs(c, sq, u, own && i > 0);
        super::spell_badges::draw(sq, &u.spells, u.drain, game.clock.total_minutes() as u64, c);
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
/// guard asks first), the same grid changes its cell.
fn garrison(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) {
    let k = chrome::k();
    let c = game.content.clone();
    let guards: Vec<_> = game.garrison_here().to_vec();
    let sel = view.garrison_sel.filter(|&(g, n)| if g { n < guards.len() } else { n < game.squad.len() });
    let mut hover = Vec::new();
    let guard_units: Vec<&Unit> = guards.iter().map(|s| &s.unit).collect();
    let top = card_grid(game, assets, f, 32.0, &guard_units, false, sel.filter(|s| s.0).map(|s| s.1));
    if let Some(Hit::Unit(j)) = top {
        let u = &guards[j].unit;
        let lv = level_label(u.level, u.xp, u.xp_to_next(&c));
        let paid = if u.unpaid { tr("unpaid") } else { tr("paid") };
        hover = vec![(u.name(&c).to_string(), ACCENT), (lv, XP_COLOR), (trf!("{hp}/{max} HP, {paid}", hp = u.hp.max(0), max = u.max_hp(&c), paid), INK)];
        hover.push((trf!("Units on guard draw no wage and heal {pct}% a day.", pct = c.options.garrison_auto_heal), DIM));
    }
    chrome::divider(at(f, 250.0, 302.0, 584.0, 16.0));
    let squad = game.squad.clone();
    let army: Vec<&Unit> = squad.iter().collect();
    let bottom = card_grid(game, assets, f, 330.0, &army, true, sel.filter(|s| !s.0).map(|s| s.1));
    if let Some(Hit::Unit(i)) = bottom {
        let u = &squad[i];
        let lv = level_label(u.level, u.xp, u.xp_to_next(&c));
        hover = vec![(game.squad_label(i), ACCENT), (lv, XP_COLOR), (trf!("{hp}/{max} HP, wage {wage}", hp = u.hp.max(0), max = u.max_hp(&c), wage = game.wage(i)), INK)];
    }
    // The hero and named characters never go into a garrison: with one selected the
    // garrison's grid says so (0x4c6964, `[Army]` line 9); with a garrison unit selected the
    // hero or a named character under the pointer cannot change places with it (0x4c612c,
    // line 11).
    let unique = |i: usize| i == 0 || squad.get(i).is_some_and(|u| u.named > 0);
    let refusal = match (sel, top, bottom) {
        (Some((false, n)), Some(_), _) if unique(n) => Some((9, n_("A unique character cannot be moved into a garrison!"))),
        (Some((true, _)), _, Some(Hit::Unit(i))) if unique(i) => Some((11, n_("You cannot change places with the selected character!"))),
        _ => None,
    };
    if let Some((line, ours)) = refusal {
        let t = chrome::ui_line("Army", line).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
        hover.insert(0, (t, chrome::RED_TEXT));
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
    if clicked() {
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
        // the original (0x4c667e, 0x4c70d6, 0x4c7233 jump past the deselection).
        let refused = matches!(result, Some(Err(ServiceError::Hero | ServiceError::Named)));
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
    tooltip(&hover);
}

/// A list with a selection and a scroll bar. Rows are (icon item, name, price, the price in
/// red). Returns the clicked row.
#[allow(clippy::too_many_arguments)]
fn price_list(assets: Option<&Assets>, rows: &[(Option<ItemId>, String, String, bool)], pick: Option<usize>, scroll: &mut usize, x: f32, y: f32, w: f32, visible: usize) -> Option<usize> {
    let k = chrome::k();
    let row_h = 30.0 * k;
    draw_rectangle(x, y, w, 26.0 * k + visible as f32 * row_h, Color::new(0.0, 0.04, 0.02, 0.45));
    chrome::silver_frame(Rect::new(x, y, w, 26.0 * k + visible as f32 * row_h), 1.0);
    text(tr("Name"), x + 50.0 * k, y + 19.0 * k, 17.0 * k, ACCENT);
    text(tr("Price"), x + w - 80.0 * k, y + 19.0 * k, 17.0 * k, ACCENT);
    let max_scroll = rows.len().saturating_sub(visible);
    let over = mouse_in(x, y, w, 26.0 * k + visible as f32 * row_h);
    let wheel = if over { wheel() } else { 0.0 };
    if wheel < 0.0 {
        *scroll = (*scroll + 1).min(max_scroll);
    } else if wheel > 0.0 {
        *scroll = scroll.saturating_sub(1);
    }
    *scroll = (*scroll).min(max_scroll);
    let mut hit = None;
    for (n, (icon, name, price, red)) in rows.iter().enumerate().skip(*scroll).take(visible) {
        let ry = y + 26.0 * k + (n - *scroll) as f32 * row_h;
        let sel = pick == Some(n);
        if sel {
            draw_rectangle(x + 2.0 * k, ry, w - 20.0 * k, row_h - 2.0 * k, Color::new(0.3, 0.38, 0.3, 1.0));
        }
        if let (Some(item), Some(assets)) = (icon, assets) {
            assets.draw_item(*item, x + 8.0 * k, ry + 1.0, row_h - 4.0 * k);
        }
        let shown: String = name.chars().take(26).collect();
        text(&shown, x + 50.0 * k, ry + 21.0 * k, 18.0 * k, if sel { WHITE } else { ACCENT });
        text(price, x + w - 80.0 * k, ry + 21.0 * k, 18.0 * k, if *red { RED } else { ACCENT });
        if mouse_in(x, ry, w - 18.0 * k, row_h) && clicked() {
            hit = Some(n);
        }
    }
    if rows.len() > visible {
        let bh = visible as f32 * row_h;
        draw_rectangle(x + w - 14.0 * k, y + 26.0 * k, 10.0 * k, bh, Color::new(0.05, 0.08, 0.07, 1.0));
        let th = bh * visible as f32 / rows.len() as f32;
        let ty = y + 26.0 * k + (bh - th) * *scroll as f32 / max_scroll.max(1) as f32;
        draw_rectangle(x + w - 13.0 * k, ty, 8.0 * k, th, SILVER);
    }
    hit
}

pub(super) fn item_description(game: &Game, assets: &Assets, item: ItemId, x: f32, y: f32, w: f32, h: f32) {
    let k = chrome::k();
    let c = &game.content;
    let d = c.item(item);
    chrome::text_box(Rect::new(x, y, w, h));
    draw_rectangle_lines(x, y, w, h, 2.0 * k, Color::new(0.6, 0.42, 0.25, 1.0));
    assets.draw_item(item, x + w / 2.0 - 28.0 * k, y + 10.0 * k, 56.0 * k);
    text_centered(&d.name, x + w / 2.0, y + 90.0 * k, 20.0 * k, BOX_INK);
    let limit = match d.kind {
        ArtefactType::BlowWeapon => Some(tr("warriors only")),
        ArtefactType::ShotWeapon => Some(tr("shooters only")),
        ArtefactType::Staff => Some(tr("mages only")),
        ArtefactType::Item => Some(tr("trade goods: cannot be worn")),
        _ => None,
    };
    let mut ly = y + 112.0 * k;
    if let Some(limit) = limit {
        text_centered(limit, x + w / 2.0, ly, 16.0 * k, Color::new(1.0, 0.6, 0.4, 1.0));
        ly += 20.0 * k;
    }
    for line in wrap(&d.description, w - 24.0 * k, 15.0 * k).into_iter().take(4) {
        text_centered(&line, x + w / 2.0, ly, 15.0 * k, BOX_INK);
        ly += 18.0 * k;
    }
    for line in wrap(&describe(c, item), w - 24.0 * k, 16.0 * k).into_iter().take(3) {
        text_centered(&line, x + w / 2.0, ly + 4.0 * k, 16.0 * k, MANA);
        ly += 19.0 * k;
    }
}

/// The row to select after row `k` left a list that now has `left` rows: the one that
/// moved up into it, else the one above, else none.
fn next_pick(k: usize, left: usize) -> Option<usize> {
    (left > 0).then(|| k.min(left - 1))
}

/// Market: the goods (or, in the sell shop, the pack) with prices, the selected item's
/// description, and the buy / sell buttons.
fn market(game: &mut Game, assets: &Assets, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) -> Option<Screen> {
    let k = chrome::k();
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let c = game.content.clone();
    let dw = w * 0.42;
    let (lx, lw) = (x + dw + 10.0 * k, w - dw - 10.0 * k);
    // The sell list holds only the pack items worth more than 1 (personal items and the
    // cheapest are not bought); a buy price above the gold is red (0x4bca8c).
    let sellable: Vec<usize> = (0..game.pack.len()).filter(|&k| game.can_sell(game.pack[k])).collect();
    let goods = game.market_here().unwrap_or_default();
    let rows: Vec<(Option<ItemId>, String, String, bool)> = if view.selling {
        sellable.iter().map(|&k| game.pack[k]).map(|i| (Some(i), c.item(i).name.clone(), game.sell_price(i).to_string(), false)).collect()
    } else {
        goods.iter().map(|&i| (Some(i), c.item(i).name.clone(), game.buy_price(i).to_string(), game.buy_price(i) > game.gold)).collect()
    };
    text_centered(if view.selling { tr("Your pack: what the market pays") } else { tr("Goods for sale") }, lx + lw / 2.0, y + 18.0 * k, 18.0 * k, ACCENT);
    if let Some(k) = price_list(Some(assets), &rows, view.pick, &mut view.scroll, lx, y + 26.0 * k, lw, 8) {
        view.pick = Some(k);
    }
    if view.pick.is_some_and(|k| k >= rows.len()) {
        view.pick = None;
    }
    let dh = 26.0 * k + 8.0 * 30.0 * k + 26.0 * k;
    match view.pick.and_then(|k| rows.get(k)).and_then(|r| r.0) {
        Some(item) => item_description(game, assets, item, x, y, dw, dh),
        None => {
            chrome::text_box(Rect::new(x, y, dw, dh));
            text_centered(tr("Pick an item from the list."), x + dw / 2.0, y + dh / 2.0, 18.0 * k, BOX_INK);
        }
    }
    let by = y + dh + 10.0 * k;
    let mut next = None;
    if button(x, by, 150.0 * k, 40.0 * k, tr("Inventory"), true) {
        next = Some(Screen::Squad { selected: Default::default(), scroll: 0, back: Some(view.clone()) });
    }
    resource_icon(Resource::Gold, x + 180.0 * k, by + 20.0 * k, 34.0 * k);
    text(&trf!("Gold {gold}", gold = game.gold), x + 202.0 * k, by + 27.0 * k, 20.0 * k, ACCENT);
    let label = if view.selling { tr("Sell") } else { tr("Buy") };
    // Sell is always on; Buy only when the price is at most the gold, the purchase's test.
    let can = match (view.selling, view.pick) {
        (true, Some(k)) => k < sellable.len(),
        (false, Some(k)) => rows.get(k).and_then(|r| r.0).is_some_and(|i| game.gold >= game.buy_price(i)),
        _ => false,
    };
    // The trade button plays the gold sound (interface.md §14).
    if button_sounding(lx, by, 130.0 * k, 40.0 * k, label, can, Cue::Gold) {
        let k = view.pick.unwrap_or(0);
        let mut done = false;
        *message = Some(if view.selling {
            let pack_index = sellable[k];
            let name = c.item(game.pack[pack_index]).name.clone();
            match game.sell(pack_index) {
                Ok(g) => {
                    done = true;
                    trf!("Sold {name} for {g} gold.", name, g)
                }
                Err(e) => trade_error(e),
            }
        } else {
            match game.buy(k) {
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
            let left = if view.selling { game.pack.iter().filter(|&&i| game.can_sell(i)).count() } else { game.market_here().map_or(0, |g| g.len()) };
            view.pick = next_pick(k, left);
        }
    }
    // The way back to the goods is offered only while the shop has some.
    let toggle = if view.selling { tr("Back to the goods") } else { tr("Sell shop") };
    if button(lx + lw - 190.0 * k, by, 190.0 * k, 40.0 * k, toggle, !view.selling || !goods.is_empty()) {
        view.selling = !view.selling;
        view.pick = None;
        view.scroll = 0;
    }
    if let Some(l) = game.location {
        let dy = by + 52.0 * k;
        description_box(&game.world.locations[l].description, x, dy, w, f.y + f.h - dy - 10.0 * k);
    }
    next
}

/// Sanctuary: spells for sale; a spell bought goes into the hero's book.
fn sanctuary(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) {
    let k = chrome::k();
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let spells: Vec<SpellDef> = game.spells_here().into_iter().cloned().collect();
    let dw = w * 0.45;
    let (lx, lw) = (x + dw + 10.0 * k, w - dw - 10.0 * k);
    let rows: Vec<_> = spells.iter().map(|s| (None, s.name.clone(), s.cost_gold.to_string(), false)).collect();
    text_centered(tr("Spells"), lx + lw / 2.0, y + 18.0 * k, 18.0 * k, ACCENT);
    if let Some(k) = price_list(None, &rows, view.pick, &mut view.scroll, lx, y + 26.0 * k, lw, 7) {
        view.pick = Some(k);
    }
    let dh = 150.0 * k;
    chrome::text_box(Rect::new(x, y, dw, dh));
    let chosen = view.pick.and_then(|k| spells.get(k));
    match chosen {
        Some(s) => {
            chrome::spell_icon(&s.icons, Rect::new(x + 14.0 * k, y + 14.0 * k, 96.0 * k, 96.0 * k));
            text_centered(&s.name, x + dw / 2.0, y + 30.0 * k, 21.0 * k, BOX_INK);
            for (i, line) in super::spellbook::spell_lines(game, s).iter().enumerate() {
                text_centered(line, x + dw / 2.0, y + 60.0 * k + i as f32 * 22.0 * k, 16.0 * k, MANA);
            }
        }
        None => text_centered(tr("Pick a spell from the list."), x + dw / 2.0, y + dh / 2.0, 18.0 * k, BOX_INK),
    }
    let by = y + dh + 14.0 * k;
    if let Some(s) = chosen {
        if game.knows_spell(s.id) {
            text_centered(tr("This spell is already in your book!"), x + dw / 2.0, by + 20.0 * k, 18.0 * k, MANA);
        } else if button_sounding(x + dw - 130.0 * k, by + 50.0 * k, 130.0 * k, 40.0 * k, tr("Buy"), game.gold >= s.cost_gold, Cue::Gold) {
            *message = Some(match game.learn_spell(s.id) {
                Ok(()) => trf!("{spell} is written into your book.", spell = s.name),
                Err(e) => service_error(e),
            });
        }
    }
    resource_icon(Resource::Gold, x + 26.0 * k, by + 70.0 * k, 34.0 * k);
    text(&trf!("Gold {gold}", gold = game.gold), x + 50.0 * k, by + 77.0 * k, 20.0 * k, ACCENT);
    text_fit(&trf!("Book {n}/{max}. Cast from the spell book on the map (B).", n = game.spells.len(), max = SPELL_BOOK_SIZE), x, by + 118.0 * k, w, 16.0 * k, DIM);
    if let Some(l) = game.location {
        let dy = y + 26.0 * k + 26.0 * k + 7.0 * 30.0 * k + 50.0 * k;
        description_box(&game.world.locations[l].description, x, dy, w, f.y + f.h - dy - 10.0 * k);
    }
}

/// A village: its waiting tribute and what may be asked instead.
fn tribute(game: &mut Game, f: &Frame, view: &mut BuildingView, message: &mut Option<String>) {
    let k = chrome::k();
    let Some(l) = game.location else { return };
    let (x, y, w) = (f.cx, f.cy, f.cw);
    let v = game.world.locations[l].clone();
    draw_rectangle(x, y, w, 120.0 * k, Color::new(0.2, 0.12, 0.07, 1.0));
    text_fit(tr("The headman keeps the tribute for whoever protects the village."), x + 16.0 * k, y + 28.0 * k, w - 32.0 * k, 19.0 * k, INK);
    resource_icon(Resource::Gold, x + 40.0 * k, y + 76.0 * k, 40.0 * k);
    text(&trf!("Gold {gold} (up to {max})", gold = v.tribute_gold, max = v.gold_max.max(v.gold_income)), x + 70.0 * k, y + 84.0 * k, 20.0 * k, ACCENT);
    resource_icon(Resource::Mana, x + 340.0 * k, y + 76.0 * k, 40.0 * k);
    text(&trf!("Mana {mana} (up to {max})", mana = v.tribute_mana, max = v.mana_max.max(v.mana_income)), x + 370.0 * k, y + 84.0 * k, 20.0 * k, MANA);
    // The tribute is taken on entering (economy.md §3); only an offer waits for an answer.
    let mut by = y + 140.0 * k;
    let status = if game.village_offer().is_some() {
        tr("The villagers ask you something before paying their tribute.")
    } else if v.hostile() {
        tr("The village pays no tribute to you.")
    } else {
        tr("Tribute already collected.")
    };
    text_fit(status, x, by + 26.0 * k, w, 19.0 * k, DIM);
    by += 52.0 * k;
    // The one offer this visit may bring (instead of the tribute: it empties the village).
    if let Some(offer) = game.village_offer() {
        use razdor::rules::economy::{OfferResult, VillageOffer, BLESSING_SPELLS, FURS_ITEM, PRIEST_SPELL};
        let spell_name = |id: u32| game.spell(id).map_or(String::new(), |s| s.name.clone());
        let label = match offer {
            VillageOffer::Innkeeper => tr("Instead: the innkeeper pays your army").to_string(),
            VillageOffer::Priest => trf!("Instead: the priest heals ({spell})", spell = spell_name(PRIEST_SPELL)),
            VillageOffer::Blessing => {
                let names: Vec<String> = BLESSING_SPELLS.iter().map(|&s| spell_name(s)).filter(|n| !n.is_empty()).collect();
                trf!("Instead: a long blessing ({spells})", spells = names.join(" / "))
            }
            VillageOffer::Furs => trf!("Instead: furs ({item})", item = game.content.try_item(razdor::rules::content::ItemId(FURS_ITEM)).map_or("", |i| i.name.as_str())),
            VillageOffer::Witch => tr("Instead: the witch's gift of mana").to_string(),
        };
        if button(x, by, (460.0 * k).min(w), 42.0 * k, &label, true) {
            // The furs, the witch and the innkeeper show their result in the event window,
            // with its chord (0x4c2100).
            let window = offer.result_window();
            let result = game.accept_offer();
            if window {
                let k = game.event_chord();
                cue(Cue::Event(k as u8));
            }
            let spell_name = |id: u32| game.spell(id).map_or(String::new(), |s| s.name.clone());
            *message = result.map(|r| match r {
                OfferResult::Paid(n) => trf!("The innkeeper pays off your {n} men.", n),
                OfferResult::Healed(h) => trf!("The priest tends to your wounded: {h} hits.", h = format!("{h:+}")),
                OfferResult::Blessing(id) => trf!("The villagers pray for you: {spell}.", spell = spell_name(id)),
                OfferResult::Furs(item) => trf!("You get {item}.", item = game.content.item(item).name),
                OfferResult::Mana(m) => trf!("The witch gives {m} mana.", m),
            });
        }
        by += 52.0 * k;
        if button(x, by, (460.0 * k).min(w), 42.0 * k, tr("No thanks: take the tribute"), true) {
            let (gold, mana) = (v.tribute_gold, v.tribute_mana);
            let paid = game.decline_offer();
            view.tribute_paid |= paid.is_some();
            *message = paid.map(|t| match t {
                razdor::rules::game::Tribute::Gold(_) => trf!("The village pays {gold} gold and {mana} mana.", gold, mana),
                razdor::rules::game::Tribute::Item(item) => trf!("The village pays with a {item}.", item = game.content.item(item).name),
            });
        }
        by += 52.0 * k;
    }
    by += 4.0 * k;
    text_fit(tr("The tribute grows every midnight, slower as it nears the village's maximum."), x, by, w, 16.0 * k, DIM);
    let dy = by + 24.0 * k;
    description_box(&v.description, x, dy, w, f.y + f.h - dy - 10.0 * k);
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
    let bar = world_view::window_backdrop(game, assets, None);
    // Events that happened meanwhile (an answer's follow-ups …).
    let pending = game.drain_events();
    let early = world_view::handle_events(game, pending, message, dialogs);
    let tabs = game.tabs_here();
    if tabs.is_empty() {
        return Some(Screen::WorldMap);
    }
    // A shipyard opens its own small window, not the building window.
    if tabs == [Tab::Shipyard] {
        let bar_next = bar.map(|b| match b {
            Screen::Squad { selected, scroll, .. } => Screen::Squad { selected, scroll, back: Some(view.clone()) },
            other => other,
        });
        return early.or(ship_window(game)).or(bar_next);
    }
    if !tabs.contains(&view.tab) {
        view.switch(tabs[0]);
    }
    let f = window();
    let k = chrome::k();
    let (_, close) = chrome::window_plain(Rect::new(f.x, f.y, f.w, f.h), &title(game), chrome::Skin::Marble, true);

    // Tabs, on light parchment.
    let col = Rect::new(f.x + 2.0 * k, f.y + 27.0 * k, 244.0 * k, f.h - 29.0 * k);
    chrome::surface(col, chrome::Skin::Paper);
    draw_line(col.x + col.w + 1.0, col.y, col.x + col.w + 1.0, col.y + col.h, 1.5 * k, SILVER);
    let (tw, th) = (228.0 * k, 88.0 * k);
    let tx = col.x + (col.w - tw) / 2.0;
    let pitch = (th + 6.0 * k).min((col.h - th - 20.0 * k) / tabs.len().max(1) as f32);
    for (i, &t) in tabs.iter().enumerate() {
        let r = Rect::new(tx, col.y + 12.0 * k + i as f32 * pitch, tw, th);
        if tab_button(tab_label(t), Some(t), r, view.tab == t) && view.tab != t {
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
    let exit = tab_button(tr("Exit"), None, Rect::new(tx, col.y + col.h - th - 10.0 * k, tw, th), false);

    let mut next = early;
    match view.tab {
        Tab::MainHall => next = next.or(main_hall(game, assets, &f, view, message, dialogs)),
        Tab::Barracks => next = next.or(barracks(game, assets, &f, view, message, dialogs)),
        Tab::Garrison => garrison(game, assets, &f, view, message),
        Tab::Market => next = market(game, assets, &f, view, message),
        Tab::Sanctuary => sanctuary(game, &f, view, message),
        Tab::Tribute => tribute(game, &f, view, message),
        Tab::Shipyard => {}
    }
    if let Some(m) = message {
        let w = measure(m, 20.0).width + 40.0;
        let (cx, y) = (f.cx + f.cw / 2.0, f.y + f.h + 6.0);
        draw_rectangle(cx - w / 2.0, y, w, 30.0, PANEL);
        text_centered(m, cx, y + 21.0, 20.0, ACCENT);
    }
    if close || exit || key(KeyCode::Escape) {
        *message = None;
        // The village window closes with the button sound, with the gold sound too when its
        // tribute was taken (0x4c604a); the building window's close is silent.
        let chord = game.location.is_some_and(|l| game.world.locations[l].kind == LocationKind::Village);
        if chord {
            cue(Cue::Button);
            if view.tribute_paid {
                cue(Cue::Gold);
            }
        }
        return Some(Screen::WorldMap);
    }
    // The bar's army button opens the army screen with the way back here.
    next.or(match bar {
        Some(Screen::Squad { selected, scroll, .. }) => Some(Screen::Squad { selected, scroll, back: Some(view.clone()) }),
        other => other,
    })
}

#[cfg(test)]
mod tests {
    use super::{next_pick, screen_line};
    use razdor::rules::formation::{Formation, Row, Slot};

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
}
