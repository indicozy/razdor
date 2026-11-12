use std::cell::{Cell, RefCell};
use razdor::local;
use macroquad::prelude::*;

use std::sync::Arc;

use razdor::dt::data::MagicSchool;
use razdor::dt::install::map_stem;
use razdor::dt::install::Problem;
use razdor::i18n::tr;
use razdor::rules::battle::Team;
use razdor::trf;
use razdor::rules::content::{Content, HeroClass, Stat, UnitId};
use razdor::rules::game::Game;
use razdor::rules::save::ScenarioRef;
use razdor::rules::script::{NextMap, ScriptEnd};
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::mobile;
use super::text_edit::TextEdit;
use super::widgets::*;
use super::game_import;
use super::{ScenarioEntry, Screen};
use razdor::gdrive;

/// Longest hero name the class screen takes.
const NAME_MAX: usize = 24;

local! {
    /// The hero's name typed on the class screen (kept between frames and games).
    pub(super) static HERO_NAME: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    /// The name field's caret and selection.
    pub(super) static HERO_EDIT: std::cell::Cell<TextEdit> = const { std::cell::Cell::new(TextEdit::at_end()) };
    /// The name field has the on-screen keyboard (a phone: after a tap in it, see `name_keyboard`).
    pub(super) static HERO_TYPING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The fallback scenario screen's last explicitly selected entry (demo is zero).
    static SCENARIO_PICKED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// The hero's name field: this frame's keys edit the name (printable characters up to
/// [`NAME_MAX`]; held Backspace repeats, as the original's edit box 0x47a624 takes each
/// repeated WM_KEYDOWN, 0x475748), then it is drawn in `r` on the baseline `y`, the mouse
/// placing the caret and selecting. Razdor's own, beyond the original's box (caret at the
/// end only): the caret, the selection, the clipboard ([`edit_keys`]). Returns the name.
pub(super) fn hero_name_field(r: Rect, y: f32, look: LineLook) -> String {
    // A phone asks for the keyboard only after a tap in the field (its tap target, 48 dp tall
    // and grown down into the paper below): opening the window does not pop it up.
    let tap = tap_rect(r, false);
    let touch = name_touch(tapped_in(tap));
    let typing = name_keyboard(super::display::dp_px().is_some(), HERO_TYPING.with(|t| t.get()), touch);
    HERO_TYPING.with(|t| t.set(typing));
    HERO_NAME.with(|n| {
        let mut n = n.borrow_mut();
        let mut ed = HERO_EDIT.with(|e| e.get());
        if !input_blocked() {
            edit_keys(&mut n, &mut ed, &typed_chars(), EditRules::line(NAME_MAX));
        }
        edit_line_kb(field_id("hero:name"), &n, &mut ed, true, true, tap, y, look, typing);
        HERO_EDIT.with(|e| e.set(ed));
        n.clone()
    })
}

/// The name field of the class screen: what was typed, or the class names as a hint.
fn name_field(x: f32, y: f32, w: f32) -> String {
    text_fit(tr("Hero's name (type it; empty: the class's name):"), x, y - 8.0, w, 18.0, DIM);
    draw_rectangle(x, y, w, 36.0, PANEL);
    draw_rectangle_lines(x, y, w, 36.0, 2.0, ACCENT);
    let name = hero_name_field(Rect::new(x + 10.0, y, w - 20.0, 36.0), y + 25.0, LineLook { size: 20.0, color: INK, centred: false, shadow: false });
    if name.is_empty() {
        text(tr("(the class's name)"), x + 14.0, y + 25.0, 20.0, DIM);
    }
    name
}

/// The attack line of a card: melee `A`, ranged `S` or magic `Pwr` with the school.
pub(super) fn attack_line(s: &Stats) -> String {
    let mut parts = Vec::new();
    if s.is_warrior() {
        parts.push(trf!("attack {n}", n = s[Stat::AttackBlow]));
    }
    if s.is_shooter() {
        parts.push(trf!("shot {n}", n = s[Stat::AttackShot]));
    }
    if s.is_mage() {
        let n = s[Stat::MagicPower];
        parts.push(match s.magic {
            Some(MagicSchool::Life) => trf!("Life magic {n}", n),
            Some(MagicSchool::Elemental) => trf!("Elemental magic {n}", n),
            Some(MagicSchool::Death) => trf!("Death magic {n}", n),
            None => trf!("magic {n}", n),
        });
    }
    if parts.is_empty() {
        parts.push(tr("no attack").into());
    }
    parts.join(", ")
}

pub(super) fn stat_lines(content: &Content, kind: UnitId) -> [String; 3] {
    let s = Stats::of_level(content, kind, 1);
    [
        trf!("Hits {hp}   Defence {blow}/{shot}", hp = s.max_hp(), blow = s[Stat::DefenceBlow], shot = s[Stat::DefenceShot]),
        format!("{}: {}", s.role(), attack_line(&s)),
        trf!("Initiative {ini}   Actions {act}", ini = s[Stat::Initiative], act = s[Stat::Manevres]),
    ]
}

/// First screen: the built-in demo or one of the install's maps (title and description are
/// read from the player's files at runtime).
pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool) -> Option<Screen> {
    clear_background(Color::from_rgba(24, 22, 20, 255));
    text_centered("RAZDOR", screen_width() / 2.0, 80.0, 64.0, ACCENT);
    text_centered(tr("Choose a scenario"), screen_width() / 2.0, 114.0, 24.0, DIM);
    // Razdor's own (the owner's decision, 2026-10-31): the buttons grow with the text size
    // (`button_grow`); at Normal each keeps its size and place.
    let g = super::display::button_grow();
    if button(60.0, 20.0, 48.0 * g, 32.0 * g, "...", true) {
        super::new_game::open_experimental_menu();
        return None;
    }
    super::language::picker_box(Rect::new(screen_width() - 196.0, 20.0, 176.0, 36.0));
    let (x, w) = (60.0, screen_width() - 120.0);
    let row_h = 52.0;
    let top = 140.0;
    let cols = if scenarios.len() > 10 { 2 } else { 1 };
    let col_w = (w - 20.0 * (cols - 1) as f32) / cols as f32;
    let mut hovered = None;
    let mut picked = SCENARIO_PICKED.with(|p| p.get()).min(scenarios.len());
    let mut entries: Vec<(Option<usize>, String, String)> =
        vec![(None, tr("Built-in demo: the bandit kingdom").into(), tr("Our own small map and units. Clear both bandit camps.").into())];
    entries.extend(scenarios.iter().enumerate().map(|(i, e)| {
        let title = if e.scenario.title.trim().is_empty() { e.file.clone() } else { e.scenario.title.clone() };
        let size = format!("{}×{}", e.scenario.width(), e.scenario.height());
        (Some(i), format!("{title}  ({size})"), e.scenario.description.clone())
    }));
    let per_col = entries.len().div_ceil(cols);
    for (k, (idx, title, desc)) in entries.iter().enumerate() {
        let (c, r) = (k / per_col, k % per_col);
        let (ex, ey) = (x + c as f32 * (col_w + 20.0), top + r as f32 * (row_h + 6.0));
        let hover = mouse_in(ex, ey, col_w, row_h);
        draw_rectangle(ex, ey, col_w, row_h, PANEL);
        let selected = picked == k;
        draw_rectangle_lines(ex, ey, col_w, row_h, 2.0, if selected { ACCENT } else { DIM });
        if selected {
            draw_rectangle(ex + 2.0, ey + 2.0, col_w - 4.0, row_h - 4.0, Color::new(0.8, 0.35, 0.08, 0.25));
        }
        text_fit(title, ex + 12.0, ey + 22.0, col_w - 24.0, 20.0, if idx.is_none() { ACCENT } else { INK });
        let first = wrap(desc, col_w - 24.0, 16.0).into_iter().next().unwrap_or_default();
        text(&first, ex + 12.0, ey + 42.0, 16.0, DIM);
        if hover {
            hovered = Some((*idx, desc.clone()));
            if clicked() {
                picked = k;
                SCENARIO_PICKED.with(|p| p.set(picked));
                cue(Cue::MenuPress);
                return Some(Screen::ClassSelect { scenario: *idx });
            }
        }
    }
    // Kept 40 px from the right edge at Normal; grown to the left.
    if button(screen_width() - 40.0 - 180.0 * g, 40.0, 180.0 * g, 44.0 * g, tr("Load a game"), true) {
        if super::saves_move::busy() {
            super::saves_move::refuse();
        } else {
            return Some(Screen::Load(super::saves::LoadView::new(super::saves::Back::Title)));
        }
    }
    let y = screen_height() - 110.0;
    if let Some((Some(_), desc)) = hovered {
        for (i, line) in wrap(&desc, w, 18.0).iter().take(4).enumerate() {
            text(line, x, y + i as f32 * 22.0, 18.0, INK);
        }
    } else if !has_install {
        #[cfg(target_os = "android")]
        {
            android_not_found(x, w);
        }
        #[cfg(target_os = "ios")]
        {
            ios_not_found(x, w);
        }
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        {
            let hint = tr("Discord Times not found: put Razdor into the game folder, next to DiscordTimes.exe.");
            text_centered(hint, screen_width() / 2.0, y + 20.0, 20.0, DIM);
            if let Some(why) = install_problem() {
                text_fit(&why, x, y + 48.0, w, 16.0, DIM);
            }
        }
    }
    None
}

/// Android: the install is not there. Tells the player what to do, and offers «Check again», the
/// look that runs on the title screen (`ui::mobile`). No storage permission is asked for here: a
/// new player's game folder is the app's own. The block is scaled to the screen ([`PhoneBlock`]).
/// Its line about the game and «Check again» keep their places (no grant line or button above them).
/// The line names only what works without the grant: the title screen's download and archive
/// picker (this screen has no such buttons).
#[cfg(target_os = "android")]
fn android_not_found(x: f32, w: f32) {
    use super::mobile::ask_recheck;
    let b = PhoneBlock::new(w, 2);
    text_fit(tr("Discord Times not found."), x, b.y, w, 20.0 * b.k, INK);
    text_fit(tr("Download the game with «Download the game», or choose its archive (.zip or .7z) with «Choose the archive…» on the title screen."), x, b.y + 26.0 * b.k, w, 16.0 * b.k, DIM);
    if button(x + b.btn_w + 16.0 * b.k, b.buttons_y, b.btn_w, b.btn_h, tr("Check again"), true) {
        ask_recheck();
    }
}

/// iOS: the install is not there. Tells the player how to copy the game folder into the app's
/// Files folder (no permission to grant), and offers «Check again», the look that runs on the
/// title screen (`ui::mobile`). Scaled and placed as [`android_not_found`].
#[cfg(target_os = "ios")]
fn ios_not_found(x: f32, w: f32) {
    use super::mobile::ask_recheck;
    let b = PhoneBlock::new(w, 1);
    text_fit(tr("Discord Times not found."), x, b.y, w, 20.0 * b.k, INK);
    text_fit(tr("1. Copy the game folder to the Files app: On My iPhone (or iPad) > Razdor, in a folder named DiscordTimes."), x, b.y + 26.0 * b.k, w, 16.0 * b.k, DIM);
    text_fit(tr("2. From a computer: Finder (or iTunes) > the device > Files > Razdor."), x, b.y + 46.0 * b.k, w, 16.0 * b.k, DIM);
    if button(x, b.buttons_y, b.btn_w, b.btn_h, tr("Check again"), true) {
        ask_recheck();
    }
}

/// The phones' not-found block: its text about 14 sp (`BLOCK_TEXT` times the desktop's size), its
/// buttons 44 dp high (`BLOCK_TOUCH_DP`).
const BLOCK_TEXT: f32 = 2.1;
/// A phone button's height on the not-found block, in dp.
const BLOCK_TOUCH_DP: f32 = 44.0;

/// The block's layout on a `w`-wide column at scale `k`: its texts, its rows of buttons and the
/// sizes they take. `bottom`: where the block ends on the desktop and the landscape phones (the
/// block grows up from there); `sh`: the screen's height. Shared by the drawing and the upright
/// menu's reserve (`upright_reserve`), so the two cannot disagree.
struct Block {
    k: f32,
    tf: f32,
    title_size: f32,
    body_size: f32,
    lead: f32,
    label_px: f32,
    btn_w: f32,
    btn_h: f32,
    gap: f32,
    body_lines: Vec<String>,
    text_h: f32,
    rows: Vec<Vec<NotFoundButton>>,
    /// The tallest the block may be (`main_menu::upright_block_budget`): above it the body scrolls.
    cap: f32,
}

impl Block {
    /// The height of `n` rows of buttons with their gaps.
    fn stack_of(&self, n: usize) -> f32 {
        n as f32 * self.btn_h + (n as f32 - 1.0).max(0.0) * self.gap
    }

    /// The block's height with its body in full: the panel's margins, the texts, the gap, the
    /// buttons and the margin.
    fn full_height(&self) -> f32 {
        10.0 * self.k + self.text_h + 18.0 * self.k + self.stack_of(self.rows.len()) + 10.0 * self.k
    }

    /// The block's height, at most its cap.
    fn height(&self) -> f32 {
        self.full_height().min(self.cap)
    }

    /// The height of the body's view: the block less the parts around the body (the margins, the
    /// gap, the buttons and the title with its gap).
    fn body_view(&self) -> f32 {
        let around = 38.0 * self.k + self.stack_of(self.rows.len()) + self.title_size + 10.0 * self.k * self.tf;
        (self.height() - around).max(2.0 * self.lead)
    }

    /// The body's text in full, in px.
    fn body_in_full(&self) -> f32 {
        self.body_lines.len() as f32 * self.lead
    }

    /// Whether the body is taller than its view, so it scrolls.
    fn scrolls(&self) -> bool {
        self.body_in_full() > self.body_view()
    }
}

local! {
    /// The upright not-found block's body scroll (px) and the finger's drag on it.
    static NOT_FOUND_SCROLL: Cell<f32> = const { Cell::new(0.0) };
    static NOT_FOUND_DRAG: RefCell<Option<super::hub_view::Drag>> = const { RefCell::new(None) };
}

fn block(k: f32, w: f32, bottom: f32, sh: f32, phone: Option<f32>) -> Block {
    // Razdor's own: the buttons grow with the text size (`button_grow`).
    let g = super::display::button_grow();
    // A phone: the text about 14 sp (the phone rule), a button 44 dp, and two buttons a row when
    // the column takes two (the phones' landscape is short). The desktop keeps its layout.
    let tf = if phone.is_some() { BLOCK_TEXT } else { 1.0 };
    let btn_h = match phone {
        Some(dp) => BLOCK_TOUCH_DP * dp,
        None => button_height(sh, k) * g,
    };
    let btn_w = (240.0 * k * g).min(w);
    // A phone's gap: each button's touch area takes half of it to the neighbour (`finger_hit_at`),
    // so 16 dp lets a 44 dp button keep its 44 dp. The desktop keeps its gap.
    let gap = match phone {
        Some(dp) => 16.0 * dp,
        None => 12.0 * k,
    };
    let (title_size, body_size, lead) = (20.0 * k * tf, 16.0 * k * tf, 22.0 * k * tf);
    // A phone's first line is one line long (the block must fit above the menu).
    let first = if phone.is_some() {
        tr("Version 1.8 with Community Update 1.2.")
    } else {
        tr("Razdor plays your own copy of Discord Times: version 1.8 with Community Update 1.2.")
    };
    let mut body = vec![first.to_string()];
    body.extend(download_lines());
    body.extend(unpack_lines());
    body.extend(install_problem());
    let wrap_body = |width: f32| -> Vec<String> { body.iter().flat_map(|s| wrap(s, width, body_size)).flat_map(|l| break_wide(&l, width, body_size)).collect() };
    let body_lines = wrap_body(w);
    let text_h = title_size + 10.0 * k * tf + body_lines.len() as f32 * lead;
    // The rows of buttons, from the top: the tutorial's (the download, the picker, the repair),
    // then the two links.
    let (how, there) = ((tr("How to install"), open_guide as fn()), (tr("Help on Discord"), open_discord as fn()));
    let mut rows = not_found_rows(manual_block(game_import::download_failure().as_deref(), game_import::download_available()));
    let stack_of = |n: usize| n as f32 * btn_h + (n as f32 - 1.0).max(0.0) * gap;
    let stacked = stack_of(rows.len() + 2);
    if phone.is_some() {
        // A phone's column has no room for the links: they are left out there (Discord stays on
        // the bottom bar; the install guide is in the wiki).
    } else if bottom - stacked - 18.0 * k - text_h - 10.0 * k >= 16.0 * k {
        rows.push(vec![how]);
        rows.push(vec![there]);
    } else {
        rows.push(vec![how, there]);
    }
    if let Some(dp) = phone {
        let per_row = if w >= 2.0 * 150.0 * dp + gap { 2 } else { 1 };
        let flat: Vec<NotFoundButton> = rows.into_iter().flatten().collect();
        rows = flat.chunks(per_row).map(|c| c.to_vec()).collect();
    }
    let cap = match phone {
        Some(_) => super::main_menu::upright_block_budget(k),
        None => f32::MAX,
    };
    let mut b = Block { k, tf, title_size, body_size, lead, label_px: 16.0 * k * tf, btn_w, btn_h, gap, body_lines, text_h, rows, cap };
    // A body taller than the block allows scrolls, and is wrapped narrower for the scroll bar.
    if phone.is_some() && b.full_height() > cap {
        b.body_lines = wrap_body(w - super::hub_view::bar_room(k, true));
        b.text_h = title_size + 10.0 * k * tf + b.body_lines.len() as f32 * lead;
    }
    b
}

/// The phone held upright: its dp size, or none (the block then grows up from the column's bottom).
fn upright_phone() -> Option<f32> {
    super::display::dp_px().filter(|_| super::window_layout::portrait())
}

/// The height the upright phone's menu keeps above its logo for the not-found block: the block's
/// top margin, the block (at most its budget) and the gap to the logo. Zero when the block is not
/// drawn upright.
pub fn upright_reserve(k: f32) -> f32 {
    if upright_phone().is_none() {
        return 0.0;
    }
    let b = block(k, screen_width() - 32.0 * k, 0.0, screen_height(), upright_phone());
    8.0 * k + b.height() + 16.0 * k
}

/// On the main menu while no install is found, on every platform: the not-found block in the
/// column left of the menu's buttons (the left half of the screen, in landscape and portrait),
/// over a dark panel. It says that Razdor plays the player's own copy of the game, gives the
/// steps to put it in place: a short numbered tutorial (download the game archive from the
/// community's Google Drive folder, then import it with the platform's importer button, which is
/// on the block too), and «How to install» (the install guide on the website) and «Help on Discord»
/// (the players' server, a secondary link). The phones also have their access button or «Check
/// again» (their steps' texts). The buttons stack; the two links share a row when the stack does
/// not fit the screen's height. The scale is the interface's (`chrome::k`, which fits the width
/// and the height), so the column is not scaled by the height alone. Held upright on a phone the
/// block hangs from the top, below the safe area, and the menu sits under it (`upright_reserve`);
/// its body scrolls when the menu would not fit with it in full.
pub fn menu_not_found(no_install: bool) {
    if !no_install {
        return;
    }
    // The automatic start (`game_import::auto_tick`): the check, the download and the unpacking run in
    // the loading window, and the block (the prompt) stays away until the start has failed or was not offered.
    if game_import::checking() || game_import::automatic() {
        return;
    }
    // A phone's block keeps the face and sizes it had before the menu art: Razdor's own text sizes (the
    // phone rule), and the TrueType face, whose letters are narrower than the bitmap one that cut the
    // labels of «Download the game» and «Choose the archive…». The desktop block keeps its face.
    if super::display::dp_px().is_some() {
        super::dt_font::with_plain_font(|| super::dt_font::with_legacy_metrics(draw_not_found));
    } else {
        draw_not_found();
    }
}

/// The block itself, drawn by [`menu_not_found`].
fn draw_not_found() {
    let sh = screen_height();
    let k = super::chrome::k();
    let (x, w, bottom) = super::main_menu::not_found_box(k);
    let phone = super::display::dp_px();
    let b = block(k, w, bottom, sh, phone);
    let height = b.height();
    let panel_top = match upright_phone() {
        Some(_) => super::display::safe_area().top + 8.0 * k,
        None => bottom - height + 10.0 * k,
    };
    let text_top = panel_top + 10.0 * k;
    draw_rectangle(x - 10.0 * k, panel_top, w + 20.0 * k, height, Color::new(0.0, 0.0, 0.0, 0.6));
    let title_base = text_top + b.title_size;
    text(tr("Discord Times not found."), x, title_base, b.title_size, INK);
    let body_top = title_base + 10.0 * k * b.tf;
    if b.scrolls() {
        let pane = Rect::new(x, body_top, w, b.body_view());
        let most = (b.body_in_full() - b.body_view()).max(0.0);
        let mut scroll = NOT_FOUND_SCROLL.with(Cell::get);
        NOT_FOUND_DRAG.with(|d| super::hub_view::pane_scroll(&mut scroll, &mut d.borrow_mut(), pane, most, k, true));
        NOT_FOUND_SCROLL.with(|s| s.set(scroll));
        // The lines drawn are the ones that fit the view whole: a line cut by its edge would run
        // into the title or the buttons.
        for (i, line) in b.body_lines.iter().enumerate() {
            let base = body_top + (i + 1) as f32 * b.lead - scroll;
            if base - b.lead * 0.75 >= pane.y && base <= pane.y + pane.h {
                text(line, x, base, b.body_size, DIM);
            }
        }
    } else {
        for (i, line) in b.body_lines.iter().enumerate() {
            text(line, x, body_top + (i + 1) as f32 * b.lead, b.body_size, DIM);
        }
    }
    let buttons_top = panel_top + height - 10.0 * k - b.stack_of(b.rows.len());
    for (i, row) in b.rows.iter().enumerate() {
        let y = buttons_top + i as f32 * (b.btn_h + b.gap);
        let n = row.len() as f32;
        let bw = b.btn_w.min((w - b.gap * (n - 1.0)) / n);
        for (j, (label, action)) in row.iter().enumerate() {
            let bx = x + j as f32 * (bw + b.gap);
            let pressed = match phone {
                Some(_) => button_sized(bx, y, bw, b.btn_h, label, true, b.label_px),
                None => button(bx, y, bw, b.btn_h, label, true),
            };
            if pressed {
                action();
            }
        }
    }
}

/// A line of the not-found block that is still wider than the column (a folder's path has no
/// spaces to wrap at) is cut by characters, each piece as wide as the column allows.
fn break_wide(line: &str, w: f32, size: f32) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = line;
    while measure(rest, size).width > w {
        let mut end = 0;
        for (i, c) in rest.char_indices() {
            let next = i + c.len_utf8();
            if end > 0 && measure(&rest[..next], size).width > w {
                break;
            }
            end = next;
        }
        out.push(rest[..end].to_string());
        rest = &rest[end..];
    }
    out.push(rest.to_string());
    out
}

/// A button of the not-found block: its label and what it does.
type NotFoundButton = (&'static str, fn());

/// A button's height on the menu's not-found block: a finger's size on the phones, a desktop
/// button's on the desktop.
fn button_height(sh: f32, k: f32) -> f32 {
    if cfg!(any(target_os = "android", target_os = "ios")) { (sh * 0.12).max(34.0 * k) } else { 40.0 * k }
}

/// Whether the block shows the manual path: the automatic download failed (its one-line error
/// comes first), or the platform has no download (iPhone). Pure, for the tests.
fn manual_block(failure: Option<&str>, available: bool) -> bool {
    failure.is_some() || !available
}

/// The block's download lines: the community's source when the download is offered, else (the
/// manual path) the error, the folder's link as text and the steps by hand.
fn download_lines() -> Vec<String> {
    // Missing files: the block offers the repair, not the download, so no download text.
    if matches!(razdor::dt::install::problem(), Some(Problem::Missing { .. })) {
        return Vec::new();
    }
    let failure = game_import::download_failure();
    if !manual_block(failure.as_deref(), game_import::download_available()) {
        return vec![tr("Press «Download the game»: Razdor downloads the game archive (45 MB) from the server's moderators' Google Drive and installs it.").to_string()];
    }
    let mut lines: Vec<String> = failure.into_iter().collect();
    lines.push(tr("Download it by hand:").to_string());
    lines.push(trf!("The file: {link}", link = gdrive::file_page(&gdrive::GAME)));
    lines.extend(manual_steps());
    lines
}

/// The steps by hand: the one .zip file (not unpacked: Razdor reads it). The step 3 names the
/// platform's way to hand it over: the picker («Pick the game archive…»), or on the phones the
/// unpacking of an archive found in the phone's folders (the Files app on iPhone).
fn manual_steps() -> Vec<String> {
    #[cfg(target_os = "android")]
    return vec![
        tr("1. Open the link («Open the link» button) and download the file: one .zip, saved to Download.").to_string(),
        tr("2. Do not unpack it: Razdor reads the .zip itself.").to_string(),
        trf!("3. Press «{label}» and choose the game's .zip or .7z.", label = picker_label()),
        tr("4. The game is in place when the main menu opens without «Discord Times not found».").to_string(),
    ];
    #[cfg(target_os = "ios")]
    return vec![
        tr("1. Open the link («Open the link» button) and download the file: one .zip.").to_string(),
        tr("2. Do not unpack it: Razdor reads the .zip itself.").to_string(),
        tr("3. Save the .zip in the Files app: On iPhone > Razdor, and press «Unpack the game archive».").to_string(),
        tr("4. The game is in place when the main menu opens without «Discord Times not found».").to_string(),
    ];
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    return vec![
        tr("1. Open the link («Open the link» button) and download the file: one .zip.").to_string(),
        tr("2. Do not unpack it: Razdor reads the .zip itself.").to_string(),
        trf!("3. Press «{label}» and choose the .zip.", label = picker_label()),
        tr("4. The game is in place when the main menu opens without «Discord Times not found».").to_string(),
    ];
}

/// The game archives found, and the unpacking's progress or error, as the block's lines.
fn unpack_lines() -> Vec<String> {
    let names = mobile::game_archive_names();
    if names.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![trf!("Found: {names}", names = names.join(", "))];
    if let Some(pct) = mobile::unpack_progress() {
        lines.push(trf!("Unpacking… {pct}%", pct = pct));
    } else if let Some(error) = mobile::unpack_error() {
        lines.push(match error {
            razdor::dt::unpack::UnpackError::NoRoomOrAccess(dir) => trf!("Could not unpack: no room or no access to the folder {dir}", dir = dir.display()),
            other => trf!("Could not unpack: {error}", error = other),
        });
    }
    lines
}

/// The buttons of the menu's not-found block above the two links: the download («Скачать игру»,
/// the retry too; the first option), the picker of a game archive
/// («Pick the game archive…»: the desktop's dialog, Android's document picker; a secondary option
/// below the download, in the failure state too), on the manual path the link's two buttons, the
/// phones' unpacking of an archive found in their folders (offered once one is found there) and
/// the phones' «Check again».
fn not_found_rows(manual: bool) -> Vec<Vec<NotFoundButton>> {
    let mut rows = Vec::new();
    // A folder with missing game files is repaired from the archive (the offer), not downloaded over.
    let damaged = matches!(razdor::dt::install::problem(), Some(Problem::Missing { .. }));
    if damaged {
        rows.push(vec![(restore_label(), game_import::request_repair as fn())]);
    } else if game_import::download_available() {
        rows.push(vec![(tr("Download the game"), game_import::request_download as fn())]);
    }
    if game_import::pick_available() {
        rows.push(vec![(picker_label(), game_import::request as fn())]);
    }
    if manual {
        rows.push(vec![(tr("Open the link"), open_folder_link as fn())]);
        rows.push(vec![(tr("Copy the link"), copy_folder_link as fn())]);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    if mobile::unpack_offered() {
        rows.push(vec![(tr("Unpack the game archive"), mobile::start_unpack as fn())]);
    }
    #[cfg(any(target_os = "android", target_os = "ios"))]
    rows.push(vec![(tr("Check again"), super::mobile::ask_recheck as fn())]);
    rows
}

/// The repair button's label: a phone's is short (two buttons a row, no ellipsis).
fn restore_label() -> &'static str {
    if super::display::dp_px().is_some() {
        tr("Restore…")
    } else {
        tr("Restore the game files")
    }
}

/// The picker's button label: the same words as the picker's own steps (`manual_steps`).
fn picker_label() -> &'static str {
    // A phone's label is shorter: the two buttons of a row must fit it (no ellipsis).
    if super::display::dp_px().is_some() {
        tr("Choose the archive…")
    } else {
        tr("Pick the game archive…")
    }
}

/// Why the install was not found, for the player: the folder that was looked at, what is missing
/// there, or the load error. The desktop only (the phones say what to copy, not why).
#[cfg(not(any(target_os = "android", target_os = "ios")))]
fn install_problem() -> Option<String> {
    match razdor::dt::install::problem() {
        Some(Problem::Missing { files, .. }) => Some(missing_files_line(&files)),
        // A phone (its density) has only the missing files' line, as on the phones themselves.
        _ if super::display::dp_px().is_some() => None,
        Some(Problem::NotInstall { dir, missing }) => Some(trf!("This folder has no {missing}: {dir}", missing = missing.join(", "), dir = dir.display())),
        Some(Problem::Load(e)) => Some(e),
        None => None,
    }
}

/// The phones: the lines about missing game files only (the rest of the problem is not shown).
#[cfg(any(target_os = "android", target_os = "ios"))]
fn install_problem() -> Option<String> {
    match razdor::dt::install::problem() {
        Some(Problem::Missing { files, .. }) => Some(missing_files_line(&files)),
        _ => None,
    }
}

/// «Не хватает файлов игры: …», with the files named (a long list is cut to a few, the rest counted).
fn missing_files_line(files: &[String]) -> String {
    let shown = files.iter().take(5).cloned().collect::<Vec<_>>().join(", ");
    let rest = files.len().saturating_sub(5);
    let names = if rest == 0 { shown } else { format!("{shown} …") };
    trf!("Missing game files: {names}", names = names)
}

/// «How to install»: the install guide of the interface language, in the browser.
fn open_guide() {
    super::main_menu::open_link(super::main_menu::install_guide());
}

/// «Help on Discord»: the Razdor players' Discord, in the browser.
fn open_discord() {
    help_action(&super::main_menu::open_link);
}

/// The help link's action: the players' Discord invite, through `open`.
fn help_action(open: &dyn Fn(&str)) {
    open(super::main_menu::DISCORD_INVITE);
}

/// «Открыть ссылку»: the community's game archive's page on Google Drive, in the browser.
fn open_folder_link() {
    open_link_with(&super::main_menu::open_link);
}

/// The open button's action: the file's page, through `open`.
fn open_link_with(open: &dyn Fn(&str)) {
    open(&gdrive::file_page(&gdrive::GAME));
}

/// «Копировать ссылку»: the file's page on the clipboard (Razdor's clipboard, as the battle code's).
fn copy_folder_link() {
    copy_link_with(&macroquad::miniquad::window::clipboard_set);
}

/// The copy button's action: the file's page, through `set`.
fn copy_link_with(set: &dyn Fn(&str)) {
    set(&gdrive::file_page(&gdrive::GAME));
}

#[cfg(test)]
mod not_found_tests {
    use super::*;

    fn recorded(action: impl Fn(&dyn Fn(&str))) -> Vec<String> {
        let seen = std::cell::RefCell::new(Vec::new());
        action(&|text| seen.borrow_mut().push(text.to_string()));
        seen.into_inner()
    }

    #[test]
    fn the_open_and_copy_buttons_use_the_file_page() {
        let page = gdrive::file_page(&gdrive::GAME);
        assert_eq!(recorded(|f| open_link_with(f)), std::slice::from_ref(&page));
        assert_eq!(recorded(|f| copy_link_with(f)), std::slice::from_ref(&page));
        assert_eq!(page, "https://drive.google.com/file/d/1xKIpVbDIuCWh7LFgtFn59D9t5RPkiH0y/view");
    }

    #[test]
    fn the_help_link_opens_the_discord_invite() {
        assert_eq!(recorded(|f| help_action(f)), [super::super::main_menu::DISCORD_INVITE]);
        assert_eq!(super::super::main_menu::DISCORD_INVITE, "https://discord.gg/wmMksmWFDM");
    }

    #[test]
    fn the_manual_path_shows_on_a_failure_or_without_a_download() {
        assert!(!manual_block(None, true));
        assert!(manual_block(Some("Download failed: no network."), true));
        assert!(manual_block(None, false));
    }

    #[test]
    fn the_steps_are_one_zip_and_name_the_platforms_way_in_the_third_step() {
        let steps = manual_steps();
        assert_eq!(steps.len(), 4, "{steps:?}");
        assert!(steps[0].starts_with("1. Open the link") && steps[0].contains(".zip"), "{steps:?}");
        assert!(steps[1].starts_with("2. Do not unpack"), "{steps:?}");
        assert!(steps[2].starts_with("3. ") && steps[2].contains(".zip"), "{steps:?}");
        #[cfg(not(any(target_os = "android", target_os = "ios")))]
        assert!(steps[2].contains("Pick the game archive…"), "{steps:?}");
        #[cfg(target_os = "android")]
        assert!(steps[2].contains("Unpack the game archive") && steps[2].contains("Pick the game archive…"), "{steps:?}");
        assert!(!steps.iter().any(|s| s.contains("folder") || s.contains("splits")), "the file is one archive: {steps:?}");
    }

    #[test]
    fn the_block_has_the_download_then_the_picker_then_the_manual_links() {
        let labels = |manual: bool| -> Vec<&'static str> { not_found_rows(manual).iter().flatten().map(|(label, _)| *label).collect() };
        // The picker is a secondary option right after the download, with or without the manual path.
        for manual in [false, true] {
            let labels = labels(manual);
            let download = labels.iter().position(|l| *l == "Download the game").expect("the download is offered");
            assert_eq!(labels[download + 1], "Pick the game archive…", "{labels:?}");
            assert!(labels.contains(&"Pick the game archive…"), "{labels:?}");
        }
        assert!(!labels(false).contains(&"Open the link"));
        assert!(labels(true).contains(&"Open the link") && labels(true).contains(&"Copy the link"));
        let manual = labels(true);
        let picker = manual.iter().position(|l| *l == "Pick the game archive…").unwrap();
        assert!(manual.iter().position(|l| *l == "Open the link").unwrap() > picker, "{manual:?}");
    }
}

/// The layout of a phone's not-found block, scaled to the screen: the scale `k` (at least 1),
/// the buttons' height (a twelfth of the screen at least) and width (`buttons` of them across
/// the block, 16 scaled pixels apart), the buttons' top `buttons_y`, and the block's first text
/// line `y` (66 scaled pixels above the buttons).
#[cfg(any(target_os = "android", target_os = "ios"))]
struct PhoneBlock {
    k: f32,
    btn_h: f32,
    btn_w: f32,
    buttons_y: f32,
    y: f32,
}

#[cfg(any(target_os = "android", target_os = "ios"))]
impl PhoneBlock {
    fn new(w: f32, buttons: u32) -> PhoneBlock {
        let sh = screen_height();
        let k = (sh / 600.0).max(1.0);
        // Razdor's own: the buttons grow with the text size (`button_grow`); at Normal as before.
        let g = super::display::button_grow();
        let btn_h = (sh * 0.12).max(34.0 * k) * g;
        let btn_w = (200.0 * k * g).min((w - 16.0 * k * (buttons - 1) as f32) / buttons as f32);
        let buttons_y = sh - 20.0 * k - btn_h;
        PhoneBlock { k, btn_h, btn_w, buttons_y, y: buttons_y - 66.0 * k }
    }
}

/// A new game as `hero` named `name`: of the demo on `demo`, or of the install's map.
pub(super) fn start_game(demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, &Arc<Content>)>, hero: HeroClass, name: &str) -> Game {
    start_game_with_rules(demo, scenario, hero, name, super::new_game::experimental_rules())
}

pub(super) fn start_game_with_rules(demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, &Arc<Content>)>, hero: HeroClass, name: &str, rules: razdor::rules::content::ExperimentalRules) -> Game {
    let mut g = match scenario {
        Some((e, c)) => {
            let content = Arc::new(c.with_experimental(rules));
            let mut g = Game::from_scenario(content, &e.scenario, hero);
            // Saves name the map file, a hash of its bytes, and carry the bytes.
            match ScenarioRef::of_map(&e.path, &e.file) {
                Ok((origin, bytes)) => g.set_map(origin, bytes),
                Err(err) => razdor::diag!("{}: {err}; this game cannot be saved", e.file),
            }
            // The playthrough's map: its save slot is this map's (`save::map_slot_key`).
            g.campaign = Some(e.file.clone());
            g
        }
        None => Game::new(Arc::new(demo.with_experimental(rules)), hero),
    };
    g.set_hero_name(name);
    razdor::diag::play(&g.clock.label(), &super::play_game_line(&g, "new game"));
    g
}

/// Hero class: for the demo its own classes, for a scenario the map's three presets.
pub fn class_select(
    game: &mut Option<Game>,
    demo: &Arc<Content>,
    scenario: Option<(&ScenarioEntry, Arc<Content>)>,
    assets: &Assets,
) -> Option<Screen> {
    if let Some((e, content)) = &scenario {
        if super::map_import::refused(e, content) {
            return Some(Screen::ScenarioSelect);
        }
    }
    clear_background(Color::from_rgba(24, 22, 20, 255));
    let content = scenario.as_ref().map_or(demo.clone(), |(_, c)| c.clone());
    let content = Arc::new(content.with_experimental(super::new_game::experimental_rules()));
    let title = scenario.as_ref().map_or(tr("A time of discord").to_string(), |(e, _)| e.scenario.title.clone());
    text_centered(&title, screen_width() / 2.0, 100.0, 44.0, ACCENT);
    text_centered(tr("Choose who you are."), screen_width() / 2.0, 140.0, 26.0, DIM);
    if let Some((e, _)) = &scenario {
        if e.scenario.header.scenario_kind == 2 {
            let note = tr("A later campaign map: the original carries gold and army over from the previous one.");
            text_centered(note, screen_width() / 2.0, 168.0, 18.0, DIM);
        }
    }

    // Only the classes the map gives a start cell (0x4c1804); none: back to the menu.
    let offered = scenario.as_ref().map_or([true; 3], |(e, _)| e.scenario.header.offered_classes());
    if !offered.contains(&true) {
        return Some(Screen::MainMenu);
    }
    let (w, h, gap) = (300.0, 380.0, 30.0);
    let x0 = (screen_width() - (3.0 * w + 2.0 * gap)) / 2.0;
    let name = name_field((screen_width() - 420.0) / 2.0, 600.0, 420.0);
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let kind = hero.unit();
        let x = x0 + i as f32 * (w + gap);
        let y = 180.0;
        let hover = offered[i] && mouse_in(x, y, w, h);
        draw_rectangle(x, y, w, h, PANEL);
        draw_rectangle_lines(x, y, w, h, 2.0, if hover { ACCENT } else { DIM });
        assets.draw_unit(kind, Team::Player, x + w / 2.0, y + 80.0, 96.0);
        text_centered(&content.unit(kind).name, x + w / 2.0, y + 170.0, 30.0, INK);
        for (j, line) in stat_lines(&content, kind).iter().enumerate() {
            text_centered(line, x + w / 2.0, y + 205.0 + j as f32 * 24.0, 18.0, DIM);
        }
        match &scenario {
            Some((e, c)) => {
                let preset = &e.scenario.header.heroes[i];
                text_centered(&trf!("Gold: {gold}", gold = preset.gold), x + w / 2.0, y + 290.0, 24.0, ACCENT);
                let army: Vec<String> = preset
                    .troops
                    .iter()
                    .filter(|t| t.unit != 0 && t.count > 0)
                    .filter_map(|t| c.try_unit(UnitId(t.unit as u32)).map(|u| format!("{} {}", t.count, u.name)))
                    .collect();
                let army = if army.is_empty() { tr("alone").to_string() } else { army.join(", ") };
                for (j, line) in wrap(&army, w - 20.0, 17.0).iter().take(3).enumerate() {
                    text_centered(line, x + w / 2.0, y + 318.0 + j as f32 * 20.0, 17.0, INK);
                }
            }
            None => text_centered(&trf!("Gold: {gold}", gold = content.start_gold(hero)), x + w / 2.0, y + 300.0, 24.0, ACCENT),
        }
        if !offered[i] {
            draw_rectangle(x, y, w, h, Color::new(0.0, 0.0, 0.0, 0.6));
        }
        if hover && clicked() {
            if super::saves_move::busy() {
                super::saves_move::refuse();
            } else {
                cue(Cue::MenuPress);
                super::items_view::reset_pack_view();
                *game = Some(start_game(&content, scenario.as_ref().map(|(e, c)| (*e, c)), hero, &name));
                return Some(Screen::WorldMap);
            }
        }
    }
    // Razdor's own: grows with the text size (`button_grow`), its bottom edge where it is at Normal.
    let g = super::display::button_grow();
    if button(30.0, screen_height() - 26.0 - 44.0 * g, 160.0 * g, 44.0 * g, tr("Back"), true) {
        return Some(Screen::ScenarioSelect);
    }
    None
}


fn end_screen(title: &str, subtitle: &str, color: Color, game: &mut Option<Game>) -> Option<Screen> {
    clear_background(Color::from_rgba(20, 18, 16, 255));
    text_centered(title, screen_width() / 2.0, 260.0, 64.0, color);
    text_centered(subtitle, screen_width() / 2.0, 310.0, 26.0, DIM);
    // Razdor's own: grows with the text size (`button_grow`), centred as at Normal.
    let (w, h) = (220.0 * super::display::button_grow(), 50.0 * super::display::button_grow());
    if button(screen_width() / 2.0 - w / 2.0, 380.0, w, h, tr("New game"), true) {
        *game = None;
        return Some(Screen::MainMenu);
    }
    None
}

/// Whole days since the game started.
fn days_played(game: &Option<Game>) -> u64 {
    game.as_ref().map_or(0, |g| g.clock.day_index().saturating_sub(g.world.start.day_index()))
}

/// The scenario's victory or defeat event, if it ended the game: its title.
fn end_event(game: &Option<Game>) -> Option<String> {
    let g = game.as_ref()?;
    let id = match g.script_end()? {
        ScriptEnd::Victory(id) | ScriptEnd::Defeat(id) => id,
    };
    Some(super::story::event_title(g, id))
}

/// After a campaign victory whose next map is in the install: what carries over (header
/// 0x110), with the flags, and that map's entry.
fn next_campaign_map<'a>(game: &Option<Game>, scenarios: &'a [ScenarioEntry]) -> Option<(NextMap, &'a ScenarioEntry)> {
    let next = game.as_ref().and_then(Game::next_map)?;
    let stem = map_stem(&next.name).to_lowercase();
    let entry = scenarios.iter().find(|e| e.file.to_lowercase() == stem)?;
    Some((next, entry))
}

/// The next campaign map's game, started from `prev`'s hand-over. `campaign` is the playthrough's
/// first map (`save::campaign_of` of the game before), which keeps its save slot.
fn campaign_game(content: Arc<Content>, prev: &NextMap, e: &ScenarioEntry, campaign: Option<String>) -> Game {
    let mut g = Game::from_campaign(content, &e.scenario, prev);
    match ScenarioRef::of_map(&e.path, &e.file) {
        Ok((origin, bytes)) => g.set_map(origin, bytes),
        Err(err) => razdor::diag!("{}: {err}; this game cannot be saved", e.file),
    }
    g.campaign = campaign;
    razdor::diag::play(&g.clock.label(), &super::play_game_line(&g, "next campaign map"));
    g
}

/// The original's campaign hand-over (events.md §11, 0x4b5b64): once the victory event's
/// window is read, the next map starts at once, with no screen between. `None` when the map
/// names no next map or the install lacks it (then the victory screen).
pub fn hand_over(game: &Option<Game>, scenarios: &[ScenarioEntry], content: Option<Arc<Content>>) -> Option<Game> {
    let (prev, e) = next_campaign_map(game, scenarios)?;
    let content = content?;
    // A next map that needs a mod's units or items is not started (the victory screen shows instead).
    if !razdor::rules::map_refs::unknown(&e.scenario, &content).is_empty() {
        return None;
    }
    Some(campaign_game(content, &prev, e, game.as_ref().and_then(razdor::rules::save::campaign_of)))
}

/// The victory screen. After a campaign map whose next map is in the install, "Next map"
/// starts it with what carries over (header 0x110) and the flags, as the original does.
pub fn victory(game: &mut Option<Game>, scenarios: &[ScenarioEntry], content: Option<Arc<Content>>) -> Option<Screen> {
    let days = days_played(game);
    let next = next_campaign_map(game, scenarios);
    let shown = match end_event(game) {
        Some(title) => end_screen(tr("Victory!"), &trf!("{title}, after {days} days.", title, days), ACCENT, game),
        None => end_screen(tr("The bandits are broken"), &trf!("Peace returns to the land after {days} days.", days), ACCENT, game),
    };
    if shown.is_some() {
        return shown;
    }
    if let (Some((prev, e)), Some(c)) = (next, content) {
        // The button only when the next map can start: a map that needs a mod's units or items is
        // not offered (`hand_over` refuses it the same way).
        let startable = razdor::rules::map_refs::unknown(&e.scenario, &c).is_empty();
        let campaign = game.as_ref().and_then(razdor::rules::save::campaign_of);
        // Below «New game», which is grown: the 20 px gap is kept (as at Normal, 450).
        let g = super::display::button_grow();
        let (w, h) = (320.0 * g, 50.0 * g);
        let y = 380.0 + 50.0 * g + 20.0;
        if startable && button(screen_width() / 2.0 - w / 2.0, y, w, h, &trf!("Next map: {title}", title = e.scenario.title), true) {
            cue(Cue::MenuPress);
            *game = Some(campaign_game(c, &prev, e, campaign));
            return Some(Screen::WorldMap);
        }
    }
    None
}

#[cfg(test)]
mod experimental_tests {
    use super::*;

    #[test]
    fn experimental_menu_choices_only_affect_the_new_party() {
        let demo = Arc::new(Content::builtin());
        let rules = razdor::rules::content::ExperimentalRules { simultaneous_bonuses: true, unlimited_items: true, two_potions: true, six_cells: false, free_units: false, auto_promotion: false, undying_units: false, fast_ai: false, bless_once: false, undead_poison_immune: false, unlimited_spells: false, merchant_item: false, rear_service_flat: false, building_store: false, death_curse_counter: false, no_corpse_garrison: false, inherited: 0 };
        super::super::new_game::snapshot_experimental(rules);
        let game = start_game(&demo, None, HeroClass::Knight, "Test");
        assert_eq!(game.experimental, rules);
        assert_eq!(game.content.experimental, rules);
        super::super::new_game::open_list();
        assert_eq!(super::super::new_game::experimental_rules(), razdor::rules::content::ExperimentalRules::for_new_game());
        assert_eq!(game.experimental, rules, "new menu choices cannot change a running party");
        let restarted = start_game_with_rules(&demo, None, game.start_class(), "Test", game.experimental);
        assert_eq!(restarted.content.experimental, rules);
        assert_eq!(demo.experimental, Default::default());
    }
}
