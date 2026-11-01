//! "Custom battle" (a Razdor extra, issue #1): the setup window over the main menu's ruins.
//! On the left two tabs: the unit types of the install (or the demo's), a click adds one to
//! the army picked; and the items the picked unit may wear, by kind, by price, a click puts
//! one on. Each list has a filter line over it (a click or Ctrl+F; any case, any language
//! shown), and a letter typed over it jumps to the next name starting with it. In the middle
//! the player's army (below on the battle screen) and the enemy's, each unit with its level
//! and items; under them the picked unit's worn items, the battle code (the whole setup as a
//! line of text to copy into a chat and paste back, `rules::custom_code`), the formation
//! (wide row or vanilla), the battle AI's level (the settings' own) and who plays each side.
//! "Fight!" starts the round on the normal battle screen (`BattleView::custom`). The setup
//! is kept for the whole run of the program ([`razdor::rules::custom::Session`]). The layout
//! is ours: the original has no such window.

use macroquad::miniquad::window::{clipboard_get, clipboard_set};
use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::rules::content::{Content, ItemId};
use razdor::rules::custom::{self, Category, Control, PriceOrder, Session, MAX_LEVEL};
use razdor::rules::custom_code::{self, CodeError};
use razdor::search::Match;
use razdor::trf;

use super::assets::Assets;
use super::audio::{cue, Cue, Settings};
use super::chrome::{self, CREAM, GOLD};
use crate::ui::input::is_key_pressed;
use super::item_filter;
use super::kit;
use super::main_menu::AiStrength;
use super::widgets::*;

/// What the setup window chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    Fight,
    Back,
}

/// The left list's tab.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Tab {
    #[default]
    Units,
    Items,
}

/// One left list's state: its filter line, its scroll (rows) and the row a typed letter
/// jumped to.
#[derive(Default)]
struct ListState {
    query: String,
    scroll: usize,
    cursor: Option<usize>,
}

/// The setup and what the window shows of it: the army new units go to, the unit picked in
/// it, the left tab and its lists, and the last word of the battle code's buttons.
pub struct SetupView {
    pub session: Session,
    side: Team,
    picked: Option<usize>,
    tab: Tab,
    units: ListState,
    items: ListState,
    category: Category,
    order: PriceOrder,
    /// The code buttons' message: its text, an error or not, and when it was shown.
    note: Option<(String, bool, f64)>,
}

impl SetupView {
    pub fn new(content: &Content) -> SetupView {
        SetupView {
            session: Session::new(content),
            side: Team::Player,
            picked: None,
            tab: Tab::Units,
            units: ListState::default(),
            items: ListState::default(),
            category: Category::All,
            order: PriceOrder::Content,
            note: None,
        }
    }
}

/// Rows of each army.
const ROWS: usize = 12;
/// The filter lines' keys (`widgets::take_focus`).
const UNIT_FILTER: &str = "custom-battle:units";
const ITEM_FILTER: &str = "custom-battle:items";
/// Seconds the code buttons' message stays.
const NOTE_SECONDS: f64 = 10.0;

fn side_name(team: Team) -> &'static str {
    match team {
        Team::Player => tr("Your army (below)"),
        Team::Enemy => tr("Enemy army (above)"),
    }
}

fn control_label(c: Control) -> &'static str {
    match c {
        Control::Player => tr("Played by: you"),
        Control::Ai => tr("Played by: the AI"),
    }
}

fn category_label(c: Category) -> &'static str {
    match c {
        Category::All => tr("All"),
        Category::Weapons => tr("Weapons"),
        Category::Armour => tr("Armour"),
        Category::Helms => tr("Helms"),
        Category::Shields => tr("Shields"),
        Category::Rings => tr("Rings"),
        Category::Amulets => tr("Amulets"),
    }
}

fn order_label(o: PriceOrder) -> &'static str {
    match o {
        PriceOrder::Content => tr("Price: any"),
        PriceOrder::Cheapest => tr("Price ↑"),
        PriceOrder::Dearest => tr("Price ↓"),
    }
}

/// The battle AI's strength as the code holds it.
fn ai_code(s: AiStrength) -> u8 {
    match s {
        AiStrength::Original => 0,
        AiStrength::Improved => 1,
        AiStrength::Max => 2,
    }
}

fn ai_of_code(n: u8) -> AiStrength {
    match n {
        0 => AiStrength::Original,
        1 => AiStrength::Improved,
        _ => AiStrength::Max,
    }
}

/// A row of a list that can be picked: the blue bar when picked, a warm wash under the
/// mouse (or on the row a letter jumped to). True when clicked.
fn row(r: Rect, picked: bool) -> bool {
    row_lit(r, picked, false)
}

fn row_lit(r: Rect, picked: bool, lit: bool) -> bool {
    let (over, down, pressed) = button_state(r, true);
    if picked {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.12, 0.2, 0.62, 0.75));
    } else if down {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.85, 0.5, 0.12));
    } else if lit {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.85, 0.5, 0.16));
    } else if over {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.85, 0.5, 0.06));
    }
    pressed
}

/// `name` from `x` within `room`, its matched part lit when it fits whole.
fn name_marked(name: &str, m: &Match, x: f32, y: f32, room: f32, size: f32, color: Color) {
    let fit = fit_size(name, room, size);
    let shown = ellipsize(name, room, fit);
    let mark = m.name_range.clone().filter(|r| r.end <= shown.len() && shown == name);
    text_marked(&shown, mark, x, y, fit, color, GOLD);
}

/// The unit picked in its army, if still there.
fn picked_unit(view: &SetupView) -> Option<usize> {
    view.picked.filter(|&i| i < view.session.setup.army(view.side).len())
}

/// Draws the window; returns what was chosen. `audio` holds the battle AI's level.
pub fn frame(view: &mut SetupView, content: &Content, assets: &Assets, audio: &mut Settings) -> Option<Pick> {
    super::main_menu::backdrop();
    view.session.setup.fit(content);
    // On a phone (`kit::phone_window`): its own layout (`frame_phone`), the window filling the room.
    if let Some((k, rect)) = kit::phone_window(false, (900.0, 600.0), true) {
        return super::display::with_window_scale(Some(k), || frame_phone(view, content, assets, audio, rect));
    }
    // Razdor's own (the owner's decision, 2026-10-31): the buttons grow with the text size
    // (`button_grow`, `g`), and the window with them: 24 video px for the tabs and 60 for the battle
    // code's and the bottom row's buttons, so the lists keep their places.
    let g = super::display::button_grow();
    let video_h = 600.0 + 84.0 * (g - 1.0);
    let _scale = super::window_layout::title_scale((900.0, video_h));
    let k = chrome::k();
    let (w, h) = (900.0 * k, video_h * k);
    let r = super::window_layout::title_rect((900.0, video_h)).unwrap_or_else(|| Rect::new((screen_width() - w) / 2.0, (super::chrome::canvas_top() + 130.0 * k).min(screen_height() - h).max(0.0), w, h));
    let (inner, closed) = chrome::window(r, tr("Custom battle"), chrome::Skin::Marble, true);
    let size = (14.0 * k).round();
    let row_h = 26.0 * k;
    let tab_h = 24.0 * k * g;
    let list_y = inner.y + 40.0 * k + 24.0 * k * (g - 1.0);
    let list_h = ROWS as f32 * row_h;

    // This frame's typed letters: over a list they jump; the filter line, when it has the
    // keyboard, reads its own.
    let letters = if typing() { Vec::new() } else { typed_chars() };

    // The tabs and the list on the left.
    let lx = inner.x + 10.0 * k;
    let lw = 270.0 * k;
    let tw = (lw - 6.0 * k) / 2.0;
    if toggle_button(lx, inner.y + 8.0 * k, tw, tab_h, tr("Unit types"), view.tab == Tab::Units) {
        view.tab = Tab::Units;
    }
    if toggle_button(lx + tw + 6.0 * k, inner.y + 8.0 * k, tw, tab_h, tr("Items"), view.tab == Tab::Items) {
        view.tab = Tab::Items;
    }
    let left = Rect::new(lx, list_y, lw, list_h);
    let filter = match view.tab {
        Tab::Units => units_list(view, content, assets, left, size, row_h, &letters, 22.0 * k, ROWS - 1),
        Tab::Items => items_list(view, content, assets, left, size, row_h, &letters, 22.0 * k, ROWS - 3),
    };

    // The two armies.
    let aw = 290.0 * k;
    for (col, team) in Team::BOTH.into_iter().enumerate() {
        let ax = lx + lw + 12.0 * k + col as f32 * (aw + 12.0 * k);
        army_column(view, content, assets, team, Rect::new(ax, list_y, aw, list_h), inner.y, size, row_h);
    }

    // The picked unit's items.
    let iy = list_y + list_h + 28.0 * k;
    items_row(view, content, Rect::new(lx + lw + 12.0 * k, iy, 2.0 * aw + 12.0 * k, 52.0 * k), size);

    // The battle code.
    // The bottom row stays 10 px above the window's bottom, grown up from there (`button_grow`).
    let bh = 30.0 * k * g;
    let by = inner.y + inner.h - 40.0 * k + (30.0 * k - bh);
    let cy = by - bh - 10.0 * k;
    let strength = super::main_menu::ai_strength(audio);
    let keys_free = !typing() && !input_blocked();
    if button(lx, cy, 220.0 * k, bh, tr("Copy the battle code"), true) || (keys_free && shortcut_held() && is_key_pressed(KeyCode::C)) {
        copy_code(view, content, strength);
    }
    if button(lx + 230.0 * k, cy, 200.0 * k, bh, tr("Paste the battle code"), true) || (keys_free && shortcut_held() && is_key_pressed(KeyCode::V)) {
        paste_code(view, content, audio);
    }
    if let Some((msg, err, at)) = &view.note {
        if crate::ui::clock::now() - at < NOTE_SECONDS {
            let x = lx + 440.0 * k;
            let room = inner.x + inner.w - 10.0 * k - x;
            let lines = wrap(msg, room, (12.0 * k).round());
            // Its second line would meet the "both armies" warning over the buttons.
            let shown = if view.session.setup.ready() { 2 } else { 1 };
            for (n, line) in lines.iter().take(shown).enumerate() {
                chrome::shadow_text(line, x, cy + 9.0 * k + n as f32 * 13.0 * k, (12.0 * k).round(), if *err { chrome::RED_TEXT } else { CREAM });
            }
        }
    }

    // The battle's settings and the way on.
    if button(lx, by, 220.0 * k, bh, formation_label(view.session.setup.wide), true) {
        toggle_formation(view, content);
    }
    if button(lx + 230.0 * k, by, 200.0 * k, bh, ai_label(strength), true) {
        super::main_menu::set_ai_strength(audio, strength.next());
    }
    let ready = view.session.setup.ready();
    let back = button(inner.x + inner.w - 310.0 * k, by, 140.0 * k, bh, tr("Back"), true);
    let fight = button(inner.x + inner.w - 160.0 * k, by, 150.0 * k, bh, tr("Fight! (Enter)"), ready);
    if !ready {
        chrome::shadow_text(tr("Both armies need a unit."), inner.x + inner.w - 310.0 * k, by - 8.0 * k, (12.0 * k).round(), chrome::RED_TEXT);
    }
    // The filter line's Enter and Esc are its own.
    let keys = !filter.keys_taken;
    if fight || (keys && ready && (key(KeyCode::Enter) || key(KeyCode::KpEnter))) {
        return Some(Pick::Fight);
    }
    if back || closed || (keys && key(KeyCode::Escape)) {
        return Some(Pick::Back);
    }
    None
}

/// The formation button's label.
fn formation_label(wide: bool) -> &'static str {
    if wide { tr("Formation: wide row (6)") } else { tr("Formation: vanilla (4)") }
}

/// The battle AI button's label.
fn ai_label(strength: AiStrength) -> &'static str {
    match strength {
        AiStrength::Original => tr("Battle AI: normal"),
        AiStrength::Improved => tr("Battle AI: improved"),
        AiStrength::Max => tr("Battle AI: maximum"),
    }
}

/// Switches the formation (wide row or vanilla), refits both armies and drops the pick.
fn toggle_formation(view: &mut SetupView, content: &Content) {
    let setup = &mut view.session.setup;
    setup.wide = !setup.wide;
    setup.fit(content);
    view.picked = None;
}

/// Copies the battle code of the setup (with the battle AI's level) and says so.
fn copy_code(view: &mut SetupView, content: &Content, strength: AiStrength) {
    let code = custom_code::encode(&view.session.setup, content, ai_code(strength));
    clipboard_set(&code);
    let msg = trf!("Battle code copied ({n} characters): paste it in a chat. Ctrl+V here loads one.", n = code.chars().count());
    view.note = Some((msg, false, crate::ui::clock::now()));
}

/// Loads the battle code on the clipboard into the setup and says what happened.
fn paste_code(view: &mut SetupView, content: &Content, audio: &mut Settings) {
    let text = clipboard_get().unwrap_or_default();
    let (msg, err) = load_code(view, content, audio, &text);
    if !err {
        cue(Cue::Button);
    }
    view.note = Some((msg, err, crate::ui::clock::now()));
}

/// The stepper's width: 48 dp on a phone (a finger), 20 px on the desktop.
fn stepper_w(k: f32) -> f32 {
    if kit::phone().is_some() { 48.0 * k } else { 20.0 * k }
}

/// Razdor's own (the owner's decision, 2026-11-01): the phone layout. Upright, the setup is one
/// scrolling column: the tabs and the list, the two armies, the picked unit's worn items, the
/// formation, the battle AI and the battle code's buttons. Sideways it is two columns: the list on
/// the left, the rest on the right. The rows are finger tall, the steppers 40 dp; the lists keep
/// their own scroll. «Назад» and «В бой!» are the footer, in halves.
fn frame_phone(view: &mut SetupView, content: &Content, assets: &Assets, audio: &mut Settings, rect: Rect) -> Option<Pick> {
    let k = chrome::k();
    let (inner, closed) = chrome::window(rect, tr("Custom battle"), chrome::Skin::Marble, true);
    let pad = 8.0 * k;
    let bh = kit::touch_h(32.0);
    let end_y = inner.y + inner.h - pad - bh;
    let half = (inner.w - 2.0 * pad - 8.0 * k) / 2.0;
    let back_r = Rect::new(inner.x + pad, end_y, half, bh);
    let fight_r = Rect::new(back_r.x + half + 8.0 * k, end_y, half, bh);
    let body = Rect::new(inner.x + pad, inner.y + 8.0 * k, inner.w - 2.0 * pad, (end_y - pad - inner.y - 8.0 * k).max(0.0));
    let letters = if typing() { Vec::new() } else { typed_chars() };
    let strength = super::main_menu::ai_strength(audio);
    let keys_free = !typing() && !input_blocked();
    let keys_taken = if kit::upright() {
        let mut flow = kit::Flow::begin(body, "custom-battle");
        let taken = lists_phone(view, content, assets, &mut flow, &letters);
        armies_phone(view, content, assets, &mut flow);
        setup_phone(view, content, audio, strength, keys_free, &mut flow);
        flow.finish();
        taken
    } else {
        let lw = (body.w * 0.44).floor();
        let mut left = kit::Flow::begin(Rect::new(body.x, body.y, lw, body.h), "custom-battle-left");
        let taken = lists_phone(view, content, assets, &mut left, &letters);
        left.finish();
        let mut right = kit::Flow::begin(Rect::new(body.x + lw + 8.0 * k, body.y, body.w - lw - 8.0 * k, body.h), "custom-battle-right");
        armies_phone(view, content, assets, &mut right);
        setup_phone(view, content, audio, strength, keys_free, &mut right);
        right.finish();
        taken
    };
    let ready = view.session.setup.ready();
    let back = kit::button(back_r, tr("Back"), true);
    let fight = kit::button(fight_r, tr("Fight! (Enter)"), ready);
    let keys = !keys_taken;
    if fight || (keys && ready && (key(KeyCode::Enter) || key(KeyCode::KpEnter))) {
        return Some(Pick::Fight);
    }
    if back || closed || (keys && key(KeyCode::Escape)) {
        return Some(Pick::Back);
    }
    None
}

/// The phone's tabs and the list of the tab picked. Returns whether the list's filter took the keys.
fn lists_phone(view: &mut SetupView, content: &Content, assets: &Assets, flow: &mut kit::Flow, letters: &[char]) -> bool {
    let k = chrome::k();
    let size = (14.0 * k).round();
    const ROWS_PHONE: usize = 6;
    let ui_h = kit::finger_h(22.0 * k);
    let row_h = kit::finger_h(26.0 * k);
    let items_top = (ui_h + 4.0 * k) + 2.0 * (ui_h + 2.0 * k) + 2.0 * k;
    let block = items_top + ROWS_PHONE as f32 * row_h + 24.0 * k;
    let tabs = flow.row_h(ui_h, |_| {});
    let tw = (tabs.w - 6.0 * k) / 2.0;
    if toggle_button(tabs.x, tabs.y, tw, ui_h, tr("Unit types"), view.tab == Tab::Units) {
        view.tab = Tab::Units;
    }
    if toggle_button(tabs.x + tw + 6.0 * k, tabs.y, tw, ui_h, tr("Items"), view.tab == Tab::Items) {
        view.tab = Tab::Items;
    }
    let r = flow.row_h(block, |_| {});
    let area = Rect::new(r.x, r.y, r.w, r.h - 24.0 * k);
    let reply = match view.tab {
        Tab::Units => units_list(view, content, assets, area, size, row_h, letters, ui_h, ROWS_PHONE),
        Tab::Items => items_list(view, content, assets, area, size, row_h, letters, ui_h, ROWS_PHONE),
    };
    reply.keys_taken
}

/// The two armies on a phone, each a head row (its name and count, the control button) over its
/// units, then the picked unit's worn items.
fn armies_phone(view: &mut SetupView, content: &Content, assets: &Assets, flow: &mut kit::Flow) {
    let k = chrome::k();
    let size = (14.0 * k).round();
    let g = super::display::button_grow();
    let cr_h = kit::finger_h(26.0 * k * g);
    let row_h = kit::finger_h(26.0 * k);
    let n = view.session.setup.capacity().max(1);
    for team in Team::BOTH {
        // The head: the name (24 px), the control button (`cr_h`) under it, then the units.
        let r = flow.row_h(36.0 * k + cr_h + n as f32 * row_h + 6.0 * k, |_| {});
        let list = Rect::new(r.x, r.y + 36.0 * k + cr_h, r.w, n as f32 * row_h);
        army_column(view, content, assets, team, list, r.y - 6.0 * k, size, row_h);
    }
    worn_items_phone(view, content, flow);
}

/// The picked unit's worn items on a phone: each a finger-tall button (a click takes it off).
fn worn_items_phone(view: &mut SetupView, content: &Content, flow: &mut kit::Flow) {
    let k = chrome::k();
    let team = view.side;
    let (x, w) = (flow.area().x, flow.width());
    let Some(i) = picked_unit(view) else {
        let t = kit::Text::new(tr("Pick a unit of an army to give it items."), 14.0, CREAM);
        flow.text_at(&t, x, t.size(), w, 0.0);
        return;
    };
    let pick = view.session.setup.army(team)[i].clone();
    let title = trf!("Items of «{name}» (a click takes one off):", name = content.unit(pick.unit).name);
    let t = kit::Text::new(&title, 14.0, GOLD);
    flow.text_at(&t, x, t.size(), w, 0.0);
    if pick.items.is_empty() {
        let t = kit::Text::new(tr("none"), 14.0, CREAM);
        flow.text_at(&t, x, t.size(), w, 0.0);
    }
    let mut off = None;
    for (n, &item) in pick.items.iter().enumerate() {
        let name = content.try_item(item).map_or_else(|| "?".to_string(), |d| d.name.clone());
        let r = flow.row_h(kit::finger_h(26.0 * k), |_| {});
        if kit::button_in(flow, r, &name, true) {
            off = Some(n);
        }
    }
    if let Some(n) = off {
        view.session.setup.take_off(team, i, n);
    }
    if view.tab != Tab::Items {
        let t = kit::Text::new(tr("More in the «Items» tab on the left."), 13.0, CREAM);
        flow.text_at(&t, x, t.size(), w, 0.0);
    }
}

/// The phone's settings and the battle code's buttons, then the note of the last code and the
/// warning while an army is empty.
fn setup_phone(view: &mut SetupView, content: &Content, audio: &mut Settings, strength: AiStrength, keys_free: bool, flow: &mut kit::Flow) {
    let (x, w) = (flow.area().x, flow.width());
    let bh = kit::touch_h(32.0);
    flow.gap(8.0);
    let r = flow.row_h(bh, |_| {});
    if kit::button_in(flow, r, tr("Copy the battle code"), true) || (keys_free && shortcut_held() && is_key_pressed(KeyCode::C)) {
        copy_code(view, content, strength);
    }
    let r = flow.row_h(bh, |_| {});
    if kit::button_in(flow, r, tr("Paste the battle code"), true) || (keys_free && shortcut_held() && is_key_pressed(KeyCode::V)) {
        paste_code(view, content, audio);
    }
    if let Some((msg, err, at)) = &view.note {
        if crate::ui::clock::now() - at < NOTE_SECONDS {
            let t = kit::Text::new(msg, 13.0, if *err { chrome::RED_TEXT } else { CREAM });
            flow.text_at(&t, x, t.size(), w, 0.0);
        }
    }
    flow.gap(8.0);
    let r = flow.row_h(bh, |_| {});
    if kit::button_in(flow, r, formation_label(view.session.setup.wide), true) {
        toggle_formation(view, content);
    }
    let r = flow.row_h(bh, |_| {});
    if kit::button_in(flow, r, ai_label(strength), true) {
        super::main_menu::set_ai_strength(audio, strength.next());
    }
    if !view.session.setup.ready() {
        let t = kit::Text::new(tr("Both armies need a unit."), 14.0, chrome::RED_TEXT);
        flow.text_at(&t, x, t.size(), w, 0.0);
    }
}

/// Reads the battle code in `text` into the setup (and the battle AI's strength); returns
/// the message to show and whether it is an error. An error changes nothing.
fn load_code(view: &mut SetupView, content: &Content, audio: &mut Settings, text: &str) -> (String, bool) {
    if text.trim().is_empty() {
        return (tr("The clipboard is empty: copy a battle code first.").to_string(), true);
    }
    match custom_code::decode(text, content) {
        Err(CodeError::NotACode) => (tr("The clipboard holds no battle code (it starts with RZB1:).").to_string(), true),
        Err(CodeError::Newer(_)) => (tr("This battle code was made by a newer Razdor: update the game.").to_string(), true),
        Err(CodeError::Damaged) => (tr("The battle code is damaged: copy it whole and try again.").to_string(), true),
        Ok(d) => {
            view.session.setup = d.setup;
            view.session.setup.fit(content);
            super::main_menu::set_ai_strength(audio, ai_of_code(d.ai));
            view.picked = None;
            view.side = Team::Player;
            let msg = if d.unknown_units + d.unknown_items > 0 {
                trf!("Battle loaded without {units} units and {items} items this game lacks: the code was made on another install or mod.", units = d.unknown_units, items = d.unknown_items)
            } else if d.other_content {
                tr("Battle loaded. The code was made on another install or mod: units may differ.").to_string()
            } else {
                tr("Battle loaded from the code.").to_string()
            };
            (msg, d.unknown_units + d.unknown_items > 0)
        }
    }
}

/// The filter line over a left list, in `r`, with the count of what it keeps.
fn filter_line(key: &str, query: &mut String, r: Rect, found: usize, of: usize) -> item_filter::Reply {
    item_filter::field_hinted(key, query, r, &[], found, of, true, tr("Filter (click, Ctrl+F); a letter jumps"))
}

/// A letter typed over a list of `names`: the list jumps to the next name starting with it.
fn jump_to(state: &mut ListState, names: &[&str], letters: &[char], rows: usize) {
    for &c in letters.iter().filter(|c| c.is_alphanumeric()) {
        if let Some(i) = custom::jump(names.iter().copied(), c, state.cursor) {
            state.cursor = Some(i);
            if i < state.scroll || i >= state.scroll + rows {
                state.scroll = i.saturating_sub(rows / 2);
            }
        }
    }
}

/// The wheel and the scrollbar of a list of `total` rows, `rows` shown, in `list`. Returns
/// the scrollbar's width.
fn scroll_list(state: &mut ListState, list: Rect, rows: usize, total: usize) -> f32 {
    let k = chrome::k();
    let max_scroll = total.saturating_sub(rows);
    swipe_rows(list, list.h / rows.max(1) as f32);
    if mouse_in(list.x, list.y, list.w, list.h) {
        let wh = wheel();
        if wh < 0.0 {
            state.scroll = (state.scroll + 1).min(max_scroll);
        } else if wh > 0.0 {
            state.scroll = state.scroll.saturating_sub(1);
        }
    }
    state.scroll = state.scroll.min(max_scroll);
    // Razdor's scrollbar on the right when the rows do not all fit (a touch screen has no wheel).
    scrollbar(Rect::new(list.x + list.w - 14.0 * k, list.y + 2.0 * k, 12.0 * k, list.h - 4.0 * k), &mut state.scroll, rows, total);
    if max_scroll > 0 {
        14.0 * k
    } else {
        0.0
    }
}

/// "first–last of all" under a list that does not fit.
fn list_note(state: &ListState, x: f32, y: f32, rows: usize, total: usize) {
    if total > rows {
        let k = chrome::k();
        let note = trf!("{first}–{last} of {all} (wheel scrolls)", first = state.scroll + 1, last = (state.scroll + rows).min(total), all = total);
        chrome::shadow_text(&note, x, y, (12.0 * k).round(), CREAM);
    }
}

/// The unit types tab: the filter line, then the types it keeps; a click adds one to the
/// army picked.
#[allow(clippy::too_many_arguments)]
fn units_list(view: &mut SetupView, content: &Content, assets: &Assets, area: Rect, size: f32, row_h: f32, letters: &[char], ui_h: f32, rows: usize) -> item_filter::Reply {
    let k = chrome::k();
    let all = custom::choices(content).len();
    let kept = custom::unit_list(content, &view.units.query);
    let reply = filter_line(UNIT_FILTER, &mut view.units.query, Rect::new(area.x, area.y, area.w, ui_h), kept.len(), all);
    // The query may have changed: the list as it now stands.
    let kept = custom::unit_list(content, &view.units.query);
    if reply.pick {
        if let Some(&(id, _)) = kept.first() {
            add_unit(view, id);
        }
    }
    let list = Rect::new(area.x, area.y + ui_h + 4.0 * k, area.w, rows as f32 * row_h);
    let names: Vec<&str> = kept.iter().map(|(u, _)| content.unit(*u).name.as_str()).collect();
    jump_to(&mut view.units, &names, letters, rows);
    chrome::text_box(list);
    let bar_w = scroll_list(&mut view.units, list, rows, kept.len());
    let full = view.session.setup.army(view.side).len() >= view.session.setup.capacity();
    for (n, (id, m)) in kept.iter().enumerate().skip(view.units.scroll).take(rows) {
        let y = list.y + (n - view.units.scroll) as f32 * row_h;
        let rr = Rect::new(list.x + 2.0 * k, y + 1.0 * k, list.w - 4.0 * k - bar_w, row_h - 2.0 * k);
        if row_lit(rr, false, view.units.cursor == Some(n)) && !full {
            view.units.cursor = Some(n);
            add_unit(view, *id);
        }
        assets.draw_portrait(*id, view.side, Rect::new(rr.x + 2.0 * k, rr.y + 1.0 * k, rr.h - 2.0 * k, rr.h - 2.0 * k));
        let def = content.unit(*id);
        name_marked(&def.name, m, rr.x + rr.h + 6.0 * k, rr.y + rr.h * 0.7, rr.w - rr.h - 50.0 * k, size, CREAM);
        let hp = trf!("{hp} HP", hp = def.hits);
        shadow_right_fit(&hp, rr.x + rr.w - 4.0 * k, rr.y + rr.h * 0.7, (12.0 * k).round());
    }
    list_note(&view.units, area.x, area.y + area.h + 16.0 * k, rows, kept.len());
    reply
}

/// Adds a unit of type `id` to the army picked and picks it.
fn add_unit(view: &mut SetupView, id: razdor::rules::content::UnitId) {
    if view.session.setup.add(view.side, id) {
        cue(Cue::Button);
        view.picked = Some(view.session.setup.army(view.side).len() - 1);
    }
}

/// The items tab: the filter line, the kinds and the price order, then the items the picked
/// unit may put on (with none picked, every item that can be worn, to look at); a click puts
/// one on.
#[allow(clippy::too_many_arguments)]
fn items_list(view: &mut SetupView, content: &Content, assets: &Assets, area: Rect, size: f32, row_h: f32, letters: &[char], ui_h: f32, rows: usize) -> item_filter::Reply {
    let k = chrome::k();
    let team = view.side;
    let picked = picked_unit(view);
    let pick = picked.map(|i| view.session.setup.army(team)[i].clone());
    let bonus = super::unit_sheet::bonus_title;
    let all = custom::item_list(content, pick.as_ref(), view.category, view.order, "", &bonus).len();
    let found = custom::item_list(content, pick.as_ref(), view.category, view.order, &view.items.query, &bonus).len();
    let reply = filter_line(ITEM_FILTER, &mut view.items.query, Rect::new(area.x, area.y, area.w, ui_h), found, all);
    // The rows of the head: the filter, then the kinds and the order (`pitch` apart).
    let head = ui_h + 4.0 * k;
    let pitch = ui_h + 2.0 * k;

    // The kinds, then the price order, four to a row.
    let bw = (area.w - 3.0 * 4.0 * k) / 4.0;
    for (n, c) in Category::EVERY.into_iter().enumerate() {
        let (x, y) = (area.x + (n % 4) as f32 * (bw + 4.0 * k), area.y + head + (n / 4) as f32 * pitch);
        if toggle_button(x, y, bw, ui_h, category_label(c), view.category == c) {
            view.category = c;
            view.items.scroll = 0;
            view.items.cursor = None;
        }
    }
    let (x, y) = (area.x + 3.0 * (bw + 4.0 * k), area.y + head + pitch);
    if toggle_button(x, y, bw, ui_h, order_label(view.order), view.order != PriceOrder::Content) {
        view.order = view.order.next();
        view.items.cursor = None;
    }

    let kept = custom::item_list(content, pick.as_ref(), view.category, view.order, &view.items.query, &bonus);
    let wear = |view: &mut SetupView, item: ItemId| {
        if let Some(i) = picked {
            if view.session.setup.wear(content, team, i, item).is_ok() {
                cue(Cue::Button);
            }
        }
    };
    if reply.pick {
        if let Some(&(item, _)) = kept.first() {
            wear(view, item);
        }
    }
    let top = area.y + head + 2.0 * pitch + 2.0 * k;
    let list = Rect::new(area.x, top, area.w, rows as f32 * row_h);
    let names: Vec<&str> = kept.iter().map(|(i, _)| content.item(*i).name.as_str()).collect();
    jump_to(&mut view.items, &names, letters, rows);
    chrome::text_box(list);
    let bar_w = scroll_list(&mut view.items, list, rows, kept.len());
    if kept.is_empty() {
        let why = if pick.is_some() { tr("Nothing here this unit can put on.") } else { tr("No such items.") };
        text_fit(why, list.x + 8.0 * k, list.y + 20.0 * k, list.w - 16.0 * k, (12.0 * k).round(), CREAM);
    }
    let ink = if pick.is_some() { CREAM } else { DIM };
    let mut hover = None;
    for (n, (item, m)) in kept.iter().enumerate().skip(view.items.scroll).take(rows) {
        let y = list.y + (n - view.items.scroll) as f32 * row_h;
        let rr = Rect::new(list.x + 2.0 * k, y + 1.0 * k, list.w - 4.0 * k - bar_w, row_h - 2.0 * k);
        if row_lit(rr, false, view.items.cursor == Some(n)) {
            view.items.cursor = Some(n);
            wear(view, *item);
        }
        if mouse_in(rr.x, rr.y, rr.w, rr.h) {
            hover = Some(*item);
        }
        assets.draw_item(*item, rr.x + 2.0 * k, rr.y + 1.0 * k, rr.h - 2.0 * k);
        let def = content.item(*item);
        name_marked(&def.name, m, rr.x + rr.h + 6.0 * k, rr.y + rr.h * 0.7, rr.w - rr.h - 58.0 * k, size, ink);
        let price = custom::price(content, *item);
        if price > 0 {
            shadow_right_fit(&trf!("{gold} gold", gold = price), rr.x + rr.w - 4.0 * k, rr.y + rr.h * 0.7, (12.0 * k).round());
        }
    }
    list_note(&view.items, area.x, area.y + area.h + 16.0 * k, rows, kept.len());
    if let Some(item) = hover {
        let def = content.item(item);
        let mut lines = vec![(def.name.clone(), ACCENT), (razdor::rules::items::describe_with(content, item, &bonus), INK)];
        if pick.is_none() {
            lines.push((tr("Pick a unit of an army to give it items.").to_string(), DIM));
        }
        tooltip(&lines);
    }
    reply
}

/// `s` right-aligned at `rx`, in the dim ink of a detail.
fn shadow_right_fit(s: &str, rx: f32, y: f32, size: f32) {
    chrome::shadow_right(s, rx, y, size, Color::new(0.85, 0.80, 0.68, 1.0));
}

/// One army: its name and count (a click picks it for new units), who plays it, and its
/// units with their level steppers, items and the × that removes them. A click on a unit
/// picks it and opens the items tab.
#[allow(clippy::too_many_arguments)]
fn army_column(view: &mut SetupView, content: &Content, assets: &Assets, team: Team, list: Rect, top: f32, size: f32, row_h: f32) {
    let k = chrome::k();
    let setup = &view.session.setup;
    let head = format!("{}: {}/{}", side_name(team), setup.army(team).len(), setup.capacity());
    let chosen = view.side == team;
    // On a phone the head's name takes the row and the control button its own row under it (its
    // label is never cut); on the desktop the control sits beside the name.
    let phone = kit::phone().is_some();
    let hr = Rect::new(list.x, top + 8.0 * k, if phone { list.w } else { list.w * 0.55 }, 24.0 * k);
    if mouse_in(hr.x, hr.y, hr.w, hr.h) && clicked() {
        view.side = team;
        view.picked = None;
    }
    text_fit(&head, hr.x, hr.y + 18.0 * k, hr.w, size, if chosen { GOLD } else { CREAM });
    let control = setup.control[team.index()];
    // The control button grows with the text size (`button_grow`); the lists start below it.
    let cr = if phone {
        Rect::new(list.x, top + 34.0 * k, list.w, kit::finger_h(26.0 * k * super::display::button_grow()))
    } else {
        Rect::new(list.x + list.w * 0.57, top + 6.0 * k, list.w * 0.43, kit::finger_h(26.0 * k * super::display::button_grow()))
    };
    if button(cr.x, cr.y, cr.w, cr.h, control_label(control), true) {
        view.session.setup.control[team.index()] = control.other();
    }
    chrome::text_box(list);
    if chosen {
        chrome::glow_frame(list, GOLD, false);
    }
    let mut remove = None;
    for (i, p) in view.session.setup.army(team).to_vec().into_iter().enumerate() {
        let y = list.y + i as f32 * row_h;
        let rr = Rect::new(list.x + 2.0 * k, y + 1.0 * k, list.w - 4.0 * k, row_h - 2.0 * k);
        let picked = chosen && view.picked == Some(i);
        // The steppers and the × take their clicks before the row does.
        let (bw, bx) = (stepper_w(k), rr.x + rr.w - 4.0 * k);
        let minus = Rect::new(bx - 3.0 * bw - 46.0 * k, rr.y + 2.0 * k, bw, rr.h - 4.0 * k);
        let plus = Rect::new(bx - 2.0 * bw - 4.0 * k, minus.y, bw, minus.h);
        let cross = Rect::new(bx - bw, minus.y, bw, minus.h);
        let on_buttons = [minus, plus, cross].iter().any(|b| mouse_in(b.x, b.y, b.w, b.h));
        if !on_buttons && row(rr, picked) {
            view.side = team;
            view.picked = Some(i);
            view.tab = Tab::Items;
            view.items.cursor = None;
        } else if on_buttons {
            row(rr, picked);
        }
        assets.draw_portrait(p.unit, team, Rect::new(rr.x + 2.0 * k, rr.y + 1.0 * k, rr.h - 2.0 * k, rr.h - 2.0 * k));
        let name = match p.items.len() {
            0 => content.unit(p.unit).name.clone(),
            n => format!("{} +{n}", content.unit(p.unit).name),
        };
        text_fit(&name, rr.x + rr.h + 6.0 * k, rr.y + rr.h * 0.7, minus.x - rr.x - rr.h - 10.0 * k, size, CREAM);
        let lvl = trf!("Lv {level}", level = p.level);
        text_fit(&lvl, minus.x + bw + 3.0 * k, rr.y + rr.h * 0.7, 40.0 * k, (12.0 * k).round(), GOLD);
        if small_button(minus.x, minus.y, minus.w, minus.h, "-", p.level > 1) {
            view.session.setup.set_level(team, i, p.level - 1);
        }
        if small_button(plus.x, plus.y, plus.w, plus.h, "+", p.level < MAX_LEVEL) {
            view.session.setup.set_level(team, i, p.level + 1);
        }
        if small_button(cross.x, cross.y, cross.w, cross.h, "×", true) {
            remove = Some(i);
        }
    }
    if let Some(i) = remove {
        cue(Cue::Button);
        view.session.setup.remove(team, i);
        if view.side == team {
            view.picked = match view.picked {
                Some(p) if p == i => None,
                Some(p) if p > i => Some(p - 1),
                other => other,
            };
        }
    }
}

/// The picked unit's worn items (a click takes one off); new ones come from the items tab.
fn items_row(view: &mut SetupView, content: &Content, r: Rect, size: f32) {
    let k = chrome::k();
    let team = view.side;
    let Some(i) = picked_unit(view) else {
        chrome::shadow_text(tr("Pick a unit of an army to give it items."), r.x, r.y + 16.0 * k, size, CREAM);
        return;
    };
    let pick = view.session.setup.army(team)[i].clone();
    let title = trf!("Items of «{name}» (a click takes one off):", name = content.unit(pick.unit).name);
    text_fit(&title, r.x, r.y + 14.0 * k, r.w, size, GOLD);
    let mut x = r.x;
    let mut off = None;
    for (n, &item) in pick.items.iter().enumerate() {
        let name = content.try_item(item).map_or_else(|| "?".to_string(), |d| d.name.clone());
        let w = (measure(&name, (12.0 * k).round()).width + 14.0 * k).min(160.0 * k);
        if small_button(x, r.y + 20.0 * k, w, 22.0 * k, &name, true) {
            off = Some(n);
        }
        x += w + 4.0 * k;
    }
    if pick.items.is_empty() {
        chrome::shadow_text(tr("none"), x, r.y + 36.0 * k, (12.0 * k).round(), CREAM);
        x += 50.0 * k;
    }
    if let Some(n) = off {
        view.session.setup.take_off(team, i, n);
    }
    if view.tab != Tab::Items && x < r.x + r.w - 200.0 * k {
        let hint = tr("More in the «Items» tab on the left.");
        text_fit(hint, x + 8.0 * k, r.y + 36.0 * k, r.x + r.w - x - 8.0 * k, (12.0 * k).round(), CREAM);
    }
}
