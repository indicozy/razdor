//! Saving and loading (video notes §6): the save window with a name, the load window with
//! the original's two tabs (manual saves and autosaves, newest first, with the scenario, the
//! hero and the in-game date), the Esc menu, and the autosaves before every battle and at the
//! 12:00 report. Files live in the player's data folder ([`save::games_dir`]).

use razdor::local;
use std::path::PathBuf;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::content::Content;
use razdor::rules::game::{Foe, Game};
use razdor::rules::save::{self, ListedSave, MetaState, SaveKind};

use super::assets::Assets;
use super::battle_view::BattleView;
use super::widgets::*;
use super::world_view;
use super::Screen;


/// Where a save or load window returns to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Back {
    Map,
    Title,
}

impl Back {
    fn screen(self) -> Screen {
        match self {
            Back::Map => Screen::WorldMap,
            Back::Title => Screen::MainMenu,
        }
    }

    /// Where Esc goes: the map in a game (Esc closes every window), else the title.
    fn escape(self) -> Screen {
        match self {
            Back::Title => Screen::MainMenu,
            Back::Map => Screen::WorldMap,
        }
    }
}

pub struct SaveView {
    pub name: String,
    /// The name's caret and selection.
    pub edit: super::text_edit::TextEdit,
    pub entries: Vec<ListedSave>,
    /// The first row shown when the missing metas were last asked for.
    asked: Option<usize>,
    /// Every meta came and the rows were put in their final order.
    settled: bool,
    /// The first row shown (the wheel scrolls the list).
    pub scroll: usize,
    /// Why the last save was refused (shown in the window).
    pub error: Option<String>,
    pub back: Back,
}

impl SaveView {
    /// The name starts empty (0x4c0dd0: a new save has no suggested name); the game is not
    /// needed for that any more, the row shows the scenario's title while the window is open.
    pub fn new(_game: &Game, back: Back) -> SaveView {
        SaveView { name: String::new(), edit: Default::default(), entries: listed(SaveKind::Manual), asked: None, settled: false, scroll: 0, error: None, back }
    }
}

pub struct LoadView {
    /// "Clear all" asked about (Razdor's): every save of the tab but the pinned ones.
    pub confirm_clear: bool,
    /// File and display name captured when its delete sign was released.
    confirm_delete: Option<(PathBuf, String)>,
    pub tab: SaveKind,
    pub entries: Vec<ListedSave>,
    /// The first row shown when the missing metas were last asked for.
    asked: Option<usize>,
    /// Every meta came and the rows were put in their final order.
    settled: bool,
    pub selected: usize,
    pub scroll: usize,
    pub back: Back,
}

local! {
    /// The tab the load window was last left on: it opens there again, the autosaves the
    /// first time (players mostly load an autosave; the original always opens on its own
    /// saves — a choice of the user's, 2026-10-09).
    static LAST_TAB: std::cell::Cell<SaveKind> = const { std::cell::Cell::new(SaveKind::Auto) };
}

/// Final per-tab placement and artwork choices from the development tuner.
const MANUAL_TAB_POSITION: (f32, f32) = (11.117_645, 408.0);
const MANUAL_TAB_TEXTURES: (u8, u8) = (0, 0);
const AUTO_TAB_POSITION: (f32, f32) = (166.852_94, 408.0);
const AUTO_TAB_TEXTURES: (u8, u8) = (1, 1);

impl LoadView {
    pub fn new(back: Back) -> LoadView {
        let first = LAST_TAB.with(|t| t.get());
        let other = if first == SaveKind::Auto { SaveKind::Manual } else { SaveKind::Auto };
        let mut v = LoadView { confirm_clear: false, confirm_delete: None, tab: first, entries: Vec::new(), asked: None, settled: false, selected: 0, scroll: 0, back };
        v.refresh();
        if v.entries.is_empty() {
            v.tab = other;
            v.refresh();
            if v.entries.is_empty() {
                v.tab = first;
                v.refresh();
            }
        }
        v
    }

    pub fn refresh(&mut self) {
        self.entries = listed(self.tab);
        self.asked = None;
        self.settled = false;
        self.selected = 0;
        self.scroll = 0;
    }
}

/// The save files of a kind, listed without being read (their metas come in later:
/// [`fill_metas`]), so the window opens at once however many there are *(Razdor's)*.
fn listed(kind: SaveKind) -> Vec<ListedSave> {
    save::games_dir().map_or_else(Vec::new, |d| save::list_files(&d, kind))
}

/// Takes in the metas read since the last frame and drops the files that are no saves
/// (`selected` follows its row); when the rows shown changed, asks for theirs first, then
/// the rest. `first`: the list's first row shown. Once every meta is in, the rows take the
/// order of the times saved ([`save::sort_listed`]), once (`settled`); with the metas read
/// ahead at the start, that is before the window is first drawn.
fn fill_metas(entries: &mut Vec<ListedSave>, asked: &mut Option<usize>, settled: &mut bool, first: usize, mut selected: Option<&mut usize>) {
    if *settled {
        return;
    }
    let mut gone = Vec::new();
    let mut waiting = false;
    for (i, e) in entries.iter_mut().enumerate() {
        match e.fill() {
            MetaState::Ready => {}
            MetaState::Waiting => waiting = true,
            MetaState::Unreadable => gone.push(i),
        }
    }
    if !gone.is_empty() {
        for &i in gone.iter().rev() {
            entries.remove(i);
        }
        if let Some(sel) = selected.as_deref_mut() {
            let before = gone.iter().filter(|&&i| i < *sel).count();
            *sel = sel.saturating_sub(before).min(entries.len().saturating_sub(1));
        }
    }
    if !waiting {
        *settled = true;
        // A row picked stays picked; the first row (nothing picked yet) stays the first.
        let chosen = selected.as_deref().filter(|&&s| s > 0).and_then(|&s| entries.get(s)).map(|e| e.path.clone());
        save::sort_listed(entries);
        if let (Some(sel), Some(path)) = (selected, chosen) {
            *sel = entries.iter().position(|e| e.path == path).unwrap_or(0);
        }
        return;
    }
    if *asked != Some(first) {
        *asked = Some(first);
        let shown = first.min(entries.len())..(first + Book::ROWS).min(entries.len());
        save::want_metas(entries[shown.clone()].iter().chain(entries[shown.end..].iter()).chain(entries[..shown.start].iter()));
    }
}

/// The autosave name before a battle: "Battle - <foe>" (the footage: "Битва - Замок …").
fn battle_name(game: &Game) -> String {
    // The army's or the building's name only, cut at its first `#` (0x4973a0).
    let who = match game.foe {
        Some(Foe::Army(i)) => game.world.armies.get(i).map(|a| a.name.as_str()),
        Some(Foe::Garrison(l)) => game.world.locations.get(l).map(|l| l.name.as_str()),
        None => None,
    };
    match who.map(save::autosave_foe).filter(|w| !w.trim().is_empty()) {
        Some(w) => razdor::trf!("Battle - {foe}", foe = w.trim_start()),
        None => tr("Battle").to_string(),
    }
}

/// Writes an autosave named `name` (`in_battle`: the one before a battle, see
/// [`save::write_autosave`]); a failure is reported on stderr (the game goes on).
pub fn autosave(game: &Game, name: &str, in_battle: bool) {
    if !autosaves_on(super::chrome::has_texts().then(|| super::chrome::options_value("OptValue8")).flatten().as_deref()) {
        return;
    }
    let Some(dir) = save::games_dir() else { return };
    if let Err(e) = save::write_autosave(&dir, name, game, in_battle) {
        razdor::diag!("autosave: {e}");
    }
}

/// Whether autosaves are written, by the install's `[Options] OptValue8` (`None`: no install
/// texts, or no such key): an install that sets it follows it as the original does (on when it
/// reads 1, 0x4b7410); without it Razdor autosaves, as it always did before the parity pass.
pub fn autosaves_on(opt_value8: Option<&str>) -> bool {
    opt_value8.is_none_or(|v| razdor::dt::ini::loose_int(v) == 1)
}

/// The install's content for the rest of the session after `game` was loaded: a save's row
/// width holds for the session (0x4b771c sets the option in memory, not in the ini), so the
/// next new game, restart or campaign map on the install's maps is played in it too, until
/// Razdor is started again.
pub fn session_content(install: Option<Arc<Content>>, game: &Game) -> Option<Arc<Content>> {
    match install {
        Some(c) if matches!(game.origin, Some(save::ScenarioRef::Map { .. })) && c.formation != game.content.formation => Some(Arc::new(c.with_formation(game.content.formation))),
        other => other,
    }
}

/// The battle against the pending foe, after the autosave the original makes before every
/// battle. The autosave is the moment just before it: without the pending foe, so loading
/// it puts the hero on the map next to the enemy (potions, spells and the army can still be
/// seen to) instead of straight into the fight, which starts again when he moves or waits.
pub fn battle(game: &mut Game) -> Screen {
    let name = battle_name(game);
    let foe = game.foe.take();
    autosave(game, &name, true);
    game.foe = foe;
    // The moment just before it, its foe pending, kept for «Рестарт» (this battle again).
    let state = save::meta_of(game, SaveKind::Auto, &name).ok().and_then(|m| save::encode(&m, game).ok().map(|b| (m, b)));
    let mut view = BattleView::new(game.start_battle());
    view.restart_state = state;
    Screen::Battle(Box::new(view))
}

/// "2026-09-25 13:08" (UTC) from seconds since 1970.
fn real_time(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let (h, m) = (secs % 86_400 / 3600, secs % 3600 / 60);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y}-{mo:02}-{d:02} {h:02}:{m:02}")
}

/// One row of the book: the save's name, its scenario and when it was saved.
struct Row {
    name: String,
    scenario: String,
    time: String,
    pinned: bool,
    /// The save's meta is still being read: the file's time only, dimmed.
    waiting: bool,
    /// The name's selected bytes, lit (the name being typed).
    mark: Option<std::ops::Range<usize>>,
    /// The new save's grey placeholder (nothing typed yet), not a name.
    placeholder: bool,
    /// The caret's character in `name` while a name is typed: a name too long for its
    /// column keeps the caret's side in sight.
    caret: Option<usize>,
}

/// The time text of a row (saved rows and the new save's row alike): the computer's date and
/// time of `secs`, in the original's words (0x49c94c).
fn time_line(secs: u64) -> String {
    razdor::trf!("Time: {date}", date = saved_label(secs))
}

fn row_of(e: &ListedSave) -> Row {
    let Some(m) = &e.meta else {
        return Row { name: "…".to_string(), scenario: String::new(), time: time_line(e.written), pinned: e.pinned, waiting: true, mark: None, placeholder: false, caret: None };
    };
    let name = if m.name == save::QUICK_SAVE { tr(save::QUICK_SAVE).to_string() } else { m.name.clone() };
    // A game in which the cheat console worked says so after its scenario.
    let scenario = if m.cheats { razdor::trf!("{title} (cheats)", title = m.title) } else { m.title.clone() };
    Row { name, scenario, time: time_line(m.saved_at), pinned: e.pinned, waiting: false, mark: None, placeholder: false, caret: None }
}

/// The save lists' fonts (0x4b61bc, 0x4b6338): the save's name in ae24a0 (Benguiat less
/// 0, 0, 100), the scenario's name in ae24f0 (less 25, 25, 125, loaded at 0x4dc01a), the
/// time in ae24c4 (SanSerif less 20, 90, 180). No highlight font is given (0x47e46c's last
/// argument 0), so the picked row keeps them, and the lists' highlight mode 4 (0x4d67ac)
/// adds (0, 30, 90) over its lines, text included (0x47d71c): the yellows come out almost
/// white, the orange pale.
const NAME_INK: [u8; 3] = [255, 255, 155];
const SCENARIO_INK: [u8; 3] = [230, 230, 130];
const TIME_INK: [u8; 3] = [235, 165, 75];
const PICKED_ADDED: [u8; 3] = [0, 30, 90];

/// A save row's text colour `c`, with the picked row's (0, 30, 90) added when `picked`.
fn row_ink(c: [u8; 3], picked: bool) -> Color {
    let add = if picked { PICKED_ADDED } else { [0; 3] };
    Color::from_rgba(c[0].saturating_add(add[0]), c[1].saturating_add(add[1]), c[2].saturating_add(add[2]), 255)
}

/// Razdor's pin of a save: a round head with a needle, gold when pinned, grey when not.
fn draw_pin(r: Rect, on: bool, hover: bool) {
    let gold = Color::new(0.95, 0.78, 0.3, 1.0);
    let grey = Color::new(0.75, 0.75, 0.75, if hover { 1.0 } else { 0.7 });
    let c = if on { gold } else { grey };
    let head = vec2(r.x + r.w * 0.62, r.y + r.h * 0.38);
    let tip = vec2(r.x + r.w * 0.18, r.y + r.h * 0.85);
    draw_line(head.x, head.y, tip.x, tip.y, (r.w * 0.09).max(1.5), Color::new(0.85, 0.85, 0.85, 1.0));
    draw_circle(head.x, head.y, r.w * 0.27, Color::new(0.0, 0.0, 0.0, 0.6));
    draw_circle(head.x, head.y, r.w * 0.22, c);
}

/// "2023 год, 1 месяц, 31 день, 13:01": when a file was saved, in the original's words.
fn saved_label(secs: u64) -> String {
    let t = real_time(secs);
    let (date, clock) = t.split_once(' ').unwrap_or((&t, ""));
    let mut p = date.split('-');
    let (y, m, d) = (p.next().unwrap_or(""), p.next().unwrap_or(""), p.next().unwrap_or(""));
    let n = |s: &str| s.trim_start_matches('0').to_string();
    razdor::trf!("{year} y, {month} m, {day} d, {clock}", year = y, month = n(m), day = n(d), clock)
}

/// A text of the install (`[<section>] <key>`) in Russian, else ours.
fn own(section: &str, key: &str, ours: &'static str) -> String {
    let t = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| super::chrome::ui_text(section, key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The top of the load or save window of height `h` on a screen `sh` tall with a bottom bar
/// `bar` tall: over the map centred over the view above the bar, else at the video's y 190.
fn book_top(sh: f32, bar: f32, h: f32, k: f32, over_map: bool) -> f32 {
    if over_map {
        ((sh - bar - h) / 2.0).max(0.0)
    } else {
        (190.0 * k).min(sh - h)
    }
}

/// Layout of the book window: the video's load window (18:12), 594×482 pixels of the
/// 960×720 frame, scaled by `chrome::k`.
struct Book {
    win: Rect,
    k: f32,
}

impl Book {
    const W: f32 = 594.0;
    const H: f32 = 482.0;
    const ROWS: usize = 12;

    fn at(title: &str, over_map: bool) -> (Book, bool) {
        let k = super::chrome::k();
        let (w, h) = (Book::W * k, Book::H * k);
        // Over the menu the video's window stands low (y 190 of 720), clear of the logo; over
        // the map the original centres it over the view above the bottom bar, as its other
        // map windows (its in-game load and save at y 104 of 768).
        let y = book_top(screen_height(), super::chrome::bar_height(), h, k, over_map);
        let win = Rect::new((screen_width() - w) / 2.0, y, w, h);
        let (_, closed) = super::chrome::window(win, title, super::chrome::Skin::Marble, true);
        // The 634×516 window with its inner frames (ae2520, 0x48d710 from 4dc10e).
        super::chrome::window_inner_frames(win, super::chrome::opx());
        (Book { win, k }, closed)
    }

    /// A point of the window in video pixels.
    fn p(&self, x: f32, y: f32) -> Vec2 {
        vec2(self.win.x + x * self.k, self.win.y + y * self.k)
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        let p = self.p(x, y);
        Rect::new(p.x, p.y, w * self.k, h * self.k)
    }

    /// The column titles and the open book behind the rows.
    fn page(&self, left: &str, right: &str) {
        let k = self.k;
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            let y = self.p(0.0, 53.0).y;
            let heading = Color::new(155.0 / 255.0, 1.0, 155.0 / 255.0, 1.0);
            super::chrome::shadow_centered(left, self.p(157.0, 0.0).x, y, 16.0 * k, heading);
            super::chrome::shadow_centered(right, self.p(437.0, 0.0).x, y, 16.0 * k, heading);
        });
        let r = self.rect(17.0, 60.0, 560.0, 350.0);
        match super::chrome::win("S_Save") {
            Some(t) => super::chrome::tex(&t, r, WHITE),
            None => super::chrome::text_box(r),
        }
        super::chrome::silver_frame(r, k);
    }

    fn row_rect(&self, i: usize) -> Rect {
        self.rect(17.0, 64.0 + i as f32 * 29.0, 560.0, 27.0)
    }

    /// The rows from `scroll` on; returns the row clicked, the delete icon clicked and the pin
    /// icon clicked (`deletable`: the selected row has both icons).
    fn rows(&self, rows: &[Row], selected: Option<usize>, scroll: &mut usize, deletable: bool) -> (Option<usize>, Option<usize>, Option<usize>) {
        let k = self.k;
        // The rows and the scrollbar right of them.
        let list = self.rect(17.0, 60.0, 574.0, 350.0);
        swipe_rows(list, 29.0 * k);
        if list.contains(crate::ui::widgets::pointer().into()) && !input_blocked() {
            let wh = wheel();
            if wh < 0.0 && *scroll + Book::ROWS < rows.len() {
                *scroll += 1;
            } else if wh > 0.0 && *scroll > 0 {
                *scroll -= 1;
            }
        }
        // Razdor's scrollbar right of the page, when the rows do not all fit.
        scrollbar(self.rect(579.0, 60.0, 12.0, 350.0), scroll, Book::ROWS, rows.len());
        let (mut picked, mut delete, mut pin) = (None, None, None);
        for (i, row) in rows.iter().enumerate().skip(*scroll).take(Book::ROWS) {
            let r = self.row_rect(i - *scroll);
            let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
            let sign = 22.0 * k;
            let row_hit = if deletable && selected == Some(i) {
                Rect::new(r.x, r.y, r.w - 2.0 * sign - 10.0 * k, r.h)
            } else {
                r
            };
            let (_, row_down, row_pressed) = super::widgets::button_state(row_hit, true);
            if selected == Some(i) {
                // The video's blue bar, the load sign on its left, the delete sign on its right.
                let steps = 8;
                for s in 0..steps {
                    let t = s as f32 / (steps - 1) as f32;
                    let a = 0.85 - 0.5 * (t - 0.5).abs();
                    draw_rectangle(r.x, r.y + r.h * s as f32 / steps as f32, r.w, r.h / steps as f32 + 0.5, Color::new(0.12, 0.2, 0.62, a));
                }
                if deletable {
                    // Razdor's pin, left of the delete sign.
                    let p = Rect::new(r.x + r.w - 2.0 * sign - 10.0 * k, r.y + (r.h - sign) / 2.0, sign, sign);
                    let (over, down, pressed) = super::widgets::button_state(p, true);
                    draw_pin(p, row.pinned, down);
                    if over {
                        tooltip(&[(if row.pinned { tr("Unpin the save (P)") } else { tr("Pin the save (P): it is never overwritten, stays on top and is not counted in the save limit") }.to_string(), INK)]);
                    }
                    if pressed {
                        pin = Some(i);
                    }
                    let d = Rect::new(r.x + r.w - sign - 4.0 * k, r.y + (r.h - sign) / 2.0, sign, sign);
                    let (over, down, pressed) = super::widgets::button_state(d, true);
                    if let Some(t) = super::chrome::win(if down { "LSign-Delete-Down" } else { "LSign-Delete" }) {
                        super::chrome::tex(&t, d, WHITE);
                    }
                    if over {
                        tooltip(&[(own("LoadGame", "DeleteHint", n_("Delete the saved game")), INK)]);
                    }
                    if pressed {
                        delete = Some(i);
                    }
                }
            } else if row_down {
                draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.16, 0.24, 0.54, 0.55));
            } else if hover {
                draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.15, 0.45, 0.35));
            }
            if row.pinned && !(deletable && selected == Some(i)) {
                let sign = 18.0 * k;
                draw_pin(Rect::new(r.x + 6.0 * k, r.y + (r.h - sign) / 2.0, sign, sign), true, false);
            }
            let lh = r.h;
            let dim = |c: Color| if row.waiting { Color { a: c.a * 0.55, ..c } } else { c };
            // The name is centred on its column, never wider than the room right of the load
            // sign (its right edge at 43): 2 × (157 − 47).
            let lit = |c: [u8; 3]| dim(row_ink(c, selected == Some(i)));
            let name_color = lit(if row.placeholder { PLACEHOLDER_INK } else { NAME_INK });
            super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
                let size = fit_size(&row.name, NAME_ROOM * k, 16.0 * k);
                let shown = match row.caret {
                    Some(at) => caret_window(&row.name, at, NAME_ROOM * k, |t| measure(t, size).width),
                    None => ellipsize(&row.name, NAME_ROOM * k, size),
                };
                let (cx, y) = (self.p(157.0, 0.0).x, r.y + lh * 0.5 + size * 0.36);
                // The selection of the name being typed, lit (when it is shown whole).
                if let Some(m) = row.mark.clone().filter(|m| shown == row.name && m.end <= shown.len()) {
                    let x0 = cx - measure(&shown, size).width / 2.0;
                    let (a, b) = (measure(&shown[..m.start], size).width, measure(&shown[..m.end], size).width);
                    draw_rectangle(x0 + a, y - size * 0.82, b - a, size * 1.05, SELECTION);
                }
                super::chrome::shadow_centered(&shown, cx, y, size, name_color);
            });
            let cx = self.p(437.0, 0.0).x;
            super::dt_font::with_face(super::dt_font::Face::Bold, || {
                super::chrome::shadow_centered(&ellipsize(&row.scenario, 250.0 * k, 11.0 * k), cx, r.y + lh * 0.42, 11.0 * k, lit(SCENARIO_INK));
            });
            super::dt_font::with_face(super::dt_font::Face::Body, || {
                super::chrome::shadow_centered(&ellipsize(&row.time, 250.0 * k, 11.0 * k), cx, r.y + lh * 0.9, 11.0 * k, lit(TIME_INK));
            });
            // Keep the play mark above long save names and the selected row highlight.
            if selected == Some(i) {
                if let Some(t) = super::chrome::win("LSign-Load") {
                    super::chrome::tex(&t, Rect::new(r.x + 4.0 * k, r.y + (r.h - sign) / 2.0, sign, sign), WHITE);
                }
            }
            // A held click on the selected row shows its load sign as pressed.
            if row_hit.contains(crate::ui::widgets::pointer().into()) && row_down && selected == Some(i) {
                draw_rectangle(r.x + 4.0 * k, r.y + (r.h - sign) / 2.0, sign, sign, Color::new(0.0, 0.0, 0.0, 0.28));
            }
            if row_pressed && delete.is_none() && pin.is_none() {
                picked = Some(i);
            }
        }
        (picked, delete, pin)
    }

    /// A torn-parchment tab hanging under the book (`load-button-1/2.lit`: grey, and orange
    /// for the open one), on a steel line running along the window.
    fn tab(&self, x: f32, y: f32, label: &str, open: bool, textures: (u8, u8)) -> bool {
        let k = self.k;
        // Convert the original 165x45 artwork to our 960x720 design coordinates.
        let native = vec2(165.0, 45.0) * 0.9375;
        let hit = self.rect(x, y, native.x, native.y);
        let (_, down, pressed) = super::widgets::button_state(hit, !open);
        // The original switches frames immediately: a held press is halfway between the
        // idle and selected size; releasing over the tab selects its full-size state.
        let scale = if open { 1.0 } else if down { 0.9 } else { 0.8 };
        // Anchor the upper left to the book's lower edge, so the selected spine covers
        // both its silver outline and the steel line and joins the parchment above.
        let r = self.rect(x, y, native.x * scale, native.y * scale);
        let selected = open;
        let texture = if selected { textures.1 } else { textures.0 };
        let name = if texture == 0 { "load-button-1" } else { "load-button-2" };
        let fx = if selected { super::chrome::Fx::Plain } else { super::chrome::Fx::Grey };
        if let Some(t) = super::chrome::win_fx(name, fx) {
            super::chrome::tex(&t, r, WHITE);
        } else {
            super::chrome::surface(r, super::chrome::Skin::Paper);
        }
        super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
            let size = 15.0 * k;
            // The right end is a torn tail; centre the caption over the solid spine.
            super::chrome::shadow_centered(label, r.x + r.w * 0.4, r.y + r.h * 0.55, size, WHITE);
        });
        pressed
    }

    /// The steel line the tabs hang from, running along the window (the original draws it
    /// right of the tabs, under the book).
    fn tab_line(&self) {
        // The window's line over the buttons (0x48d710: (2, h − 56) to (w − 2, h − 54)),
        // drawn again over the hanging tabs.
        let px = super::chrome::opx();
        let w = self.win;
        super::chrome::steel_frame(Rect::new(w.x + 2.0 * px, w.bottom() - 56.0 * px, w.w - 4.0 * px, 2.0 * px), px, 2);
    }

    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, 443.0, w, 28.0);
        let (_, down, pressed) = super::widgets::button_state(r, enabled);
        super::chrome::marble_button(r, label, enabled, down);
        pressed
    }
}

/// The name being typed as its row shows it, and the bytes of it to light: the selection,
/// or else the caret put in at its place (`|`, a space while it blinks off).
fn typed_name(name: &str, edit: super::text_edit::TextEdit, caret_on: bool) -> (String, Option<std::ops::Range<usize>>) {
    let mut edit = edit;
    edit.fit(name);
    if edit.has_selection() {
        return (name.to_string(), Some(edit.selected_bytes(name)));
    }
    let at = name.char_indices().nth(edit.caret).map_or(name.len(), |(b, _)| b);
    (format!("{}{}{}", &name[..at], if caret_on { '|' } else { ' ' }, &name[at..]), None)
}

/// The new save's row (the first row of the window): the name being typed, or the grey
/// placeholder while nothing is typed (0x4c0dd0: an empty slot shows "new save" in grey, a
/// new save starts with a blank name). Its time is the computer's date `now`, the same text
/// as the saved rows (0x49c94c), not the game's clock.
fn new_save_row(name: &str, edit: super::text_edit::TextEdit, caret_on: bool, title: &str, now: u64) -> Row {
    let time = time_line(now);
    if name.is_empty() {
        return Row { name: own("Buttons", "NewSave", n_("(save anew)")), scenario: title.to_string(), time, pinned: false, waiting: false, mark: None, placeholder: true, caret: None };
    }
    let (typed, mark) = typed_name(name, edit, caret_on);
    let mut edit = edit;
    edit.fit(name);
    Row { name: typed, scenario: title.to_string(), time, pinned: false, waiting: false, mark, placeholder: false, caret: Some(edit.caret) }
}

/// The part of `name` (`caret`: the caret's character, drawn as its own glyph) that fits
/// `room` wide, `width` measuring a text: the start moves on until the caret's glyph is in
/// sight (`text_edit::first_shown`), the characters past what fits are left out.
fn caret_window(name: &str, caret: usize, room: f32, width: impl Fn(&str) -> f32) -> String {
    let chars: Vec<char> = name.chars().collect();
    let xs = super::text_edit::caret_xs(name, |c| width(&c.to_string()));
    let first = super::text_edit::first_shown(&xs, caret + 1, room);
    let mut last = first;
    while last < chars.len() && xs[last + 1] - xs[first] <= room {
        last += 1;
    }
    chars[first..last].iter().collect()
}

/// A save may be written under a name with some text in it, into a folder that exists.
fn can_save(name: &str, folder: bool) -> bool {
    !name.trim().is_empty() && folder
}

/// The computer's clock in seconds since 1970 (the save window's new-save date).
fn now_secs() -> u64 {
    // A snapshot shows a fixed date, so its pictures do not change from day to day.
    if crate::ui::clock::is_snapshot() {
        return SNAPSHOT_SECS;
    }
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// The date a snapshot shows for the computer's time (2025-06-15 12:00 UTC).
const SNAPSHOT_SECS: u64 = 1_749_988_800;

/// The name is typed in whenever the window is open and not blocked (its row draws the caret
/// at all times), so it asks for the on-screen keyboard as the other text fields do
/// (`widgets::edit_line`); the row is drawn by `new_save_row`, not by `edit_line`.
fn ask_keyboard(name: &str, edit: super::text_edit::TextEdit, blocked: bool) {
    if !blocked {
        want_keyboard(name, edit.caret);
    }
}

/// The widest a save's name is drawn in its column, in the book's pixels: centred at 157,
/// it stays right of the selected row's load sign (21..43) with 4 px to spare.
const NAME_ROOM: f32 = 220.0;

/// The grey of the new save's placeholder name (0x4c0dd0).
const PLACEHOLDER_INK: [u8; 3] = [150, 150, 150];

/// The save window ("Сохранение активной игры"): the book of manual saves under "(save
/// anew)"; a click on a save takes its name to replace it, typing edits the name.
pub fn save_screen(game: &Game, assets: &Assets, view: &mut SaveView, message: &mut Option<String>) -> Option<Screen> {
    world_view::backdrop(game, assets);
    let (book, closed) = Book::at(&own("SaveGame", "Title", n_("Save the game")), true);
    book.page(&own("SaveGame", "GameName", n_("Name of the saved game")), &own("SaveGame", "ScenarioName", n_("Name of the scenario")));
    // Held Backspace repeats, as in the original's edit box (0x47a624, 0x475748); the caret,
    // the selection and the clipboard are Razdor's own (`widgets::edit_keys`).
    if !input_blocked() {
        edit_keys(&mut view.name, &mut view.edit, &typed_chars(), EditRules::line(60));
    }
    ask_keyboard(&view.name, view.edit, input_blocked());
    fill_metas(&mut view.entries, &mut view.asked, &mut view.settled, view.scroll.saturating_sub(1), None);
    let same = view.entries.iter().position(|e| e.meta.as_ref().is_some_and(|m| m.name == view.name));
    // The first row is the name being typed (a new save, or the one it replaces).
    let typed = new_save_row(&view.name, view.edit, (crate::ui::clock::now() * 2.0) as i64 % 2 == 0, &game.world.title, now_secs());
    let mut rows = vec![typed];
    rows.extend(view.entries.iter().map(row_of));
    if let (Some(k), _, _) = book.rows(&rows, Some(same.map_or(0, |i| i + 1)), &mut view.scroll, false) {
        // A save whose meta is not read yet gives no name to take.
        match k.checked_sub(1).map(|i| view.entries[i].meta.as_ref()) {
            None => view.name = String::new(),
            Some(Some(m)) => view.name = m.name.clone(),
            Some(None) => {}
        }
        view.edit = Default::default();
    }
    let dir = save::games_dir();
    let warning = if dir.is_none() { Some(tr("No data folder for saves: set RAZDOR_SAVE_DIR.")) } else { view.error.as_deref() };
    if let Some(w) = warning {
        let p = book.p(22.0, 435.0);
        text(&ellipsize(w, 330.0 * book.k, 13.0 * book.k), p.x, p.y, 13.0 * book.k, RED);
    }
    let ok = can_save(&view.name, dir.is_some());
    if book.button(369.0, 116.0, &own("SaveGame", "Save", n_("Save")), ok) || (ok && key(KeyCode::Enter)) {
        let dir = dir.expect("checked");
        match save::write(&dir, SaveKind::Manual, view.name.trim(), game) {
            Ok(_) => {
                *message = Some(razdor::trf!("Saved: {name}", name = view.name.trim()));
                return Some(Screen::WorldMap);
            }
            // The window stays: a save to replace can be picked.
            Err(e) => view.error = Some(razdor::trf!("Not saved: {e}.", e)),
        }
    }
    if book.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed {
        return Some(view.back.screen());
    }
    if key(KeyCode::Escape) {
        return Some(view.back.escape());
    }
    None
}

/// The load window ("Загрузка сохраненной игры"): the book of saves with the "Личные" and
/// "Авто-сохр." tabs; a row's delete sign opens a confirmation. "Clear all" deletes the tab's
/// saves but the pinned ones after a question. Loading itself is
/// the app's (`pending` receives the file).
pub fn load_screen(game: Option<&Game>, assets: &Assets, view: &mut LoadView, pending: &mut Option<PathBuf>, error: &mut Option<String>) -> Option<Screen> {
    match game {
        Some(g) if view.back != Back::Title => world_view::backdrop(g, assets),
        _ => super::main_menu::backdrop(),
    }
    let asking_delete = view.confirm_delete.is_some();
    // A failed load is told in its own message box, which takes the window until its OK.
    let failed = error.is_some();
    let asking = view.confirm_clear || asking_delete || failed;
    let previously_blocked = input_blocked();
    if asking { set_input_blocked(true); }
    let (book, closed) = Book::at(&own("LoadGame", "Title", n_("Load a saved game")), game.is_some() && view.back != Back::Title);
    book.page(&own("LoadGame", "GameName", n_("Name of the saved game")), &own("LoadGame", "ScenarioName", n_("Name of the scenario")));
    fill_metas(&mut view.entries, &mut view.asked, &mut view.settled, view.scroll, Some(&mut view.selected));
    let rows: Vec<Row> = view.entries.iter().map(row_of).collect();
    if rows.is_empty() {
        let p = book.p(297.0, 230.0);
        super::chrome::shadow_centered(tr("No saves here yet."), p.x, p.y, 16.0 * book.k, super::chrome::CREAM);
    }
    let (picked, delete, pin) = book.rows(&rows, Some(view.selected), &mut view.scroll, true);
    // Razdor's pin: the icon, or P for the selected save.
    let pin = pin.or_else(|| key(KeyCode::P).then_some(view.selected));
    if let Some(e) = pin.filter(|_| !asking).and_then(|k| view.entries.get(k)) {
        let (path, on) = (e.path.clone(), !e.pinned);
        if let Err(err) = save::set_pinned(&path, on) {
            razdor::diag!("{}: {err}", path.display());
        }
        view.refresh();
        fill_metas(&mut view.entries, &mut view.asked, &mut view.settled, 0, None);
        // The save stays selected where the list now puts it.
        if let Some(k) = view.entries.iter().position(|e| e.path == path) {
            view.selected = k;
            view.scroll = k.saturating_sub(Book::ROWS - 1);
        }
    }
    if let Some(k) = picked.filter(|_| !asking) {
        if k == view.selected {
            *pending = view.entries.get(k).map(|e| e.path.clone());
        }
        view.selected = k;
    }
    if let Some(e) = delete.filter(|_| !asking).and_then(|k| view.entries.get(k)) {
        let name = e.meta.as_ref().map(|_| row_of(e).name)
            .unwrap_or_else(|| e.path.file_stem().unwrap_or_default().to_string_lossy().into_owned());
        view.confirm_delete = Some((e.path.clone(), name));
    }
    book.tab_line();
    let tabs = [
        (MANUAL_TAB_POSITION.0, MANUAL_TAB_POSITION.1, SaveKind::Manual, own("LoadGame", "PrivateSave", n_("Private")), MANUAL_TAB_TEXTURES),
        (AUTO_TAB_POSITION.0, AUTO_TAB_POSITION.1, SaveKind::Auto, own("LoadGame", "AutoSave", n_("Autosaves")), AUTO_TAB_TEXTURES),
    ];
    // Paint the inactive spine first and the selected spine last, above the window edge.
    let mut switched = None;
    for open in [false, true] {
        for (x, y, tab, label, textures) in &tabs {
            if (view.tab == *tab) != open { continue; }
            if book.tab(*x, *y, label, open, *textures) && !asking { switched = Some(*tab); }
        }
    }
    if let Some(tab) = switched {
        super::audio::cue(super::audio::Cue::CastSpell);
        view.tab = tab;
        LAST_TAB.with(|t| t.set(tab));
        view.refresh();
    }
    let clearable = view.entries.iter().any(|e| !e.pinned);
    if book.button(17.0, 140.0, tr("Clear all"), clearable) && !asking {
        view.confirm_clear = true;
        return None;
    }
    let chosen = view.entries.get(view.selected);
    if (book.button(369.0, 116.0, &own("LoadGame", "Load", n_("Load")), chosen.is_some()) || key(KeyCode::Enter)) && !asking && chosen.is_some() {
        *pending = chosen.map(|e| e.path.clone());
    }
    if (book.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed) && !asking {
        return Some(view.back.screen());
    }
    set_input_blocked(previously_blocked);
    if asking { super::chrome::grey_out(book.win); }
    if let Some(message) = error.as_deref() {
        if load_failed_box(message) { *error = None; }
        return None;
    }
    if asking_delete {
        let (path, name) = view.confirm_delete.as_ref().expect("delete confirmation");
        match delete_save_question(&book, name) {
            Some(true) => {
                if let Err(err) = save::delete(path) {
                    razdor::diag!("{}: {err}", path.display());
                }
                let selected = view.selected;
                view.confirm_delete = None;
                view.refresh();
                view.selected = selected.min(view.entries.len().saturating_sub(1));
            }
            Some(false) => view.confirm_delete = None,
            None => {}
        }
        return None;
    }
    // "Clear all"'s question. It opens on the next frame, so the click that asked it cannot
    // also answer it.
    if view.confirm_clear {
        let tab = if view.tab == SaveKind::Manual { own("LoadGame", "PrivateSave", n_("Private")) } else { own("LoadGame", "AutoSave", n_("Autosaves")) };
        let q = format!("^{}", razdor::trf!("Delete every save of the \"{tab}\" tab? The pinned ones stay.", tab));
        match marked_question(&own("MessageBox", "DeleteSave_Title", n_("Delete the save")), &q) {
            Some(true) => {
                for e in view.entries.iter().filter(|e| !e.pinned) {
                    if let Err(err) = save::delete(&e.path) {
                        razdor::diag!("{}: {err}", e.path.display());
                    }
                }
                view.confirm_clear = false;
                view.refresh();
            }
            Some(false) => view.confirm_clear = false,
            None => {}
        }
        return None;
    }
    if key(KeyCode::Escape) {
        return Some(view.back.escape());
    }
    None
}

/// The exit window's version line: the original's blue (ae24a8, Benguiat with red − 200,
/// green − 75).
const VERSION_INK: Color = Color::new(0.22, 0.7, 1.0, 1.0);
/// Its warning: the ae24a0 tint.
const WARNING_INK: Color = Color::new(1.0, 1.0, 0.61, 1.0);
/// Razdor's message box for a failed load: the message wrapped over as many lines as it takes,
/// and one OK (or Enter, Escape). True once it is answered.
fn load_failed_box(message: &str) -> bool {
    let k = super::chrome::k();
    let title = own("LoadGame", "Title", n_("Load a saved game"));
    let size = 13.0 * k;
    let step = 17.0 * k;
    let lines = wrap(message, 330.0 * k, size);
    let h = (60.0 * k + lines.len() as f32 * step).max(150.0 * k);
    let w = 380.0 * k;
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let (inner, _) = super::chrome::window(r, &title, super::chrome::Skin::Marble, false);
    for (i, line) in lines.iter().enumerate() {
        super::chrome::shadow_centered(line, inner.center().x, inner.y + 24.0 * k + i as f32 * step, size, super::chrome::CREAM);
    }
    let ok = Rect::new(inner.center().x - 45.0 * k, inner.y + inner.h - 38.0 * k, 90.0 * k, 28.0 * k);
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let (_, down, pressed) = super::widgets::button_state(ok, true);
    super::chrome::marble_button(ok, &ok_label, true, down);
    pressed || answer_key().is_some()
}

/// The original's delete confirmation: a parchment panel and two green buttons.
fn delete_save_question(book: &Book, name: &str) -> Option<bool> {
    let k = book.k;
    let r = Rect::new(book.win.center().x - 190.0 * k, book.win.center().y - 110.0 * k, 380.0 * k, 220.0 * k);
    let title = own("MessageBox", "DeleteSave_Title", n_("Delete the save"));
    super::chrome::window(r, &title, super::chrome::Skin::Marble, false);
    let panel = Rect::new(r.x + 17.0 * k, r.y + 43.0 * k, r.w - 34.0 * k, 110.0 * k);
    super::chrome::text_box(panel);
    let size = 15.0 * k;
    super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
        let lines = wrap(tr("Do you really want to delete the saved game"), panel.w - 36.0 * k, size);
        let name_lines = wrap(&format!("\"{name}\"?"), panel.w - 36.0 * k, size);
        let step = 17.0 * k;
        let total = (lines.len() + name_lines.len()) as f32 * step;
        let mut y = panel.center().y - total * 0.5 + size;
        for line in &lines {
            super::chrome::shadow_centered(line, panel.center().x, y, size, ACCENT);
            y += step;
        }
        for line in &name_lines {
            super::chrome::shadow_centered(line, panel.center().x, y, size, Color::from_rgba(55, 180, 255, 255));
            y += step;
        }
    });
    super::chrome::silver_frame(Rect::new(r.x + 3.0 * k, r.y + 29.0 * k, r.w - 6.0 * k, 140.0 * k), k);
    let yes = Rect::new(r.x + 55.0 * k, r.y + 179.0 * k, 90.0 * k, 32.0 * k);
    let no = Rect::new(r.x + 235.0 * k, yes.y, yes.w, yes.h);
    let (yes_label, no_label) = (own("Buttons", "Yes", n_("Yes")), own("Buttons", "No", n_("No")));
    let (_, down, yes_pressed) = button_state(yes, true);
    super::chrome::marble_button(yes, &yes_label, true, down);
    let (_, down, no_pressed) = button_state(no, true);
    super::chrome::marble_button(no, &no_label, true, down);
    let answer = answer_key();
    if yes_pressed || answer == Some(true) { Some(true) }
    else if no_pressed || answer == Some(false) { Some(false) }
    else { None }
}

/// What the "Выход из игры" window chose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExitChoice {
    Quit,
    MainMenu,
    Restart,
    /// Razdor's «Клавиши» (the game menu only): the key list over the map.
    Keys,
}

/// The bar's X button (and Esc on the map): "Выход из игры", the original's warning that the
/// game will end, and "Выйти из игры", "Выйти в меню", "Рестарт" (after a question) or
/// "Отмена". `asking` is true while the restart question is open.
pub fn exit_window(game: &Game, assets: &Assets, asking: &mut bool) -> (Option<ExitChoice>, Option<Screen>) {
    world_view::backdrop_lit(game, assets, Some(super::game_bar::BarButton::Menu));
    let title = own("ExitGame", "Title", n_("Leave the game"));
    let warning = own("ExitGame", "Warning", n_("Attention!\n\nThe game under way will end.\nTo go on with it another time, save it from the map's bar first."));
    match exit_dialog(&title, &warning, asking, false, true) {
        (_, true) => (None, Some(Screen::WorldMap)),
        (choice, false) => (choice, None),
    }
}

/// Esc in a battle: "Варианты выхода из битвы", the original's warning that a battle cannot
/// be left for the map, and the same choices as the exit window. Returns the choice and
/// whether it was cancelled.
pub fn battle_exit_dialog(asking: &mut bool) -> (Option<ExitChoice>, bool) {
    let title = own("ExitBattle", "Title", n_("Ways out of the battle"));
    // Razdor's «Рестарт» here plays this battle again (the user's choice; the original's
    // restarts the scenario, and says so): its own text.
    let warning = tr(n_("Attention!\n\nYou cannot go back to the map without finishing the battle!\nTo stop the game under way, leave to the main menu (the game is not saved), or start this battle again (restart)."));
    if super::chrome::win("S_Battle").is_none() {
        return exit_dialog(&title, warning, asking, true, false);
    }
    super::dt_font::with_legacy_metrics(|| battle_exit_window(&title, warning, asking))
}

/// The original's leave-battle window (4d3620, drawn by 4d0ccc), in its pixels times the
/// interface scale: 634×516 in red marble centred over the map view above the bar, the
/// `S_Battle` picture at (5, 32), a text box at (18, 264) 598×179 darkened by 0x30 with its
/// silver lines and corners, the warning in the plain Benguiat tint (ae24a0) centred in it,
/// and the red buttons (Btn* turned and darkened, 4dba90) 10 px from the bottom: «Выйти из
/// игры» (Btn3) at x 10, «Выйти в меню» (Btn3) 8 px after it, «Рестарт» (Btn2) 8 px after
/// that (4d5214), «Отмена» (Btn1) 10 px from the right (4d3620).
fn battle_exit_window(title: &str, warning: &str, asking: &mut bool) -> (Option<ExitChoice>, bool) {
    use super::chrome;
    const W: f32 = 634.0;
    const H: f32 = 516.0;
    let (sw, sh) = (screen_width(), screen_height());
    let o = (chrome::k() * 0.9375).min((sw - 12.0) / W).min((sh - chrome::bar_height() - 4.0) / H);
    let (wx, wy) = (((sw - W * o) / 2.0).floor(), ((sh - chrome::bar_height() - H * o) / 2.0).floor().max(2.0));
    let r = Rect::new(wx, wy, W * o, H * o);
    let closed = chrome::battle_exit_frame(r, title, o);
    if let Some(t) = chrome::win("S_Battle") {
        let (pw, ph) = (t.width().min(W - 10.0), t.height().min(H - 32.0 - 5.0));
        chrome::tex(&t, Rect::new(wx + 5.0 * o, wy + 32.0 * o, pw * o, ph * o), WHITE);
    }
    // The window is made by 0x48d710 with its inner frames (4d3664): the 2 px frame and
    // title line (0x48d3f8), the line over the buttons and the 1 px frame round the picture.
    chrome::window_frames(r, o);
    chrome::window_inner_frames(r, o);
    let tb = Rect::new(wx + 18.0 * o, wy + 264.0 * o, 598.0 * o, 179.0 * o);
    let grey = 0x30 as f32 / 255.0;
    chrome::subtractive(|| draw_rectangle(tb.x, tb.y, tb.w, tb.h, Color::new(grey, grey, grey, 1.0)));
    chrome::silver_frame(tb, o);
    for (name, right, bottom) in [("Corner_Frame-LU", false, false), ("Corner_Frame-RU", true, false), ("Corner_Frame-LD", false, true), ("Corner_Frame-RD", true, true)] {
        if let Some(t) = chrome::win(name) {
            let (w, h) = (t.width() * o, t.height() * o);
            chrome::tex(&t, Rect::new(if right { tb.right()-w } else { tb.x }, if bottom { tb.bottom()-h } else { tb.y }, w, h), WHITE);
        }
    }
    let face = super::dt_font::Face::Subtitle;
    let size = super::dt_font::sheet_size(face, o);
    // The warning's first sentence occupies one line in the reference window.
    let size = super::dt_font::with_face(face, || fit_size(warning.split('\n').nth(2).unwrap_or_default(), tb.w - 40.0 * o, size));
    let line = 18.0 * o;
    // Wrapped in the face it is drawn in (the original's Benguiat sheet).
    let lines: Vec<String> = super::dt_font::with_face(face, || {
        warning.split('\n').flat_map(|l| if l.is_empty() { vec![String::new()] } else { wrap(l, tb.w - 40.0 * o, size) }).collect()
    });
    let text_h = lines.len() as f32 * line;
    let mut y = tb.y + ((tb.h - text_h) / 2.0).max(4.0 * o) + size * 0.8;
    for l in &lines {
        super::dt_font::with_face(face, || chrome::shadow_centered(l, tb.center().x, y, size, Color::from_rgba(255, 255, 155, 255)));
        y += line;
    }
    let art = |name: &str| chrome::win(&format!("{name}Up")).map(|t| (t.width(), t.height()));
    let (b3, b2, b1) = (art("Btn3").unwrap_or((160.0, 36.0)), art("Btn2").unwrap_or((128.0, 36.0)), art("Btn1").unwrap_or((96.0, 36.0)));
    let by = 460.0 + (H - 460.0 - b1.1) / 2.0;
    let exit_x = 10.0;
    let menu_x = exit_x + b3.0 + 8.0;
    let restart_x = menu_x + b3.0 + 8.0;
    let cancel_x = W - 10.0 - b1.0;
    let buttons = [
        (exit_x, b3, "Btn3", own("ExitGame", "Exit", n_("Quit the game")), Some(ExitChoice::Quit)),
        (menu_x, b3, "Btn3", own("ExitGame", "ExitToMenu", n_("To the menu")), Some(ExitChoice::MainMenu)),
        (restart_x, b2, "Btn2", own("ExitGame", "Restart", n_("Restart")), Some(ExitChoice::Restart)),
        (cancel_x, b1, "Btn1", own("Buttons", "Cancel", n_("Cancel")), None),
    ];
    let mut choice = None;
    let mut cancel = closed;
    let was_asking = *asking;
    for (x, (bw, bh), art, label, what) in buttons.iter() {
        let b = Rect::new(wx + x * o, wy + by * o, bw * o, bh * o);
        let over = !*asking && b.contains(crate::ui::widgets::pointer().into()) && !input_blocked();
        chrome::defeat_button_art(b, art, label, over && super::input::is_mouse_button_down(MouseButton::Left));
        if over && clicked() {
            match what {
                Some(ExitChoice::Restart) => *asking = true,
                Some(c) => choice = Some(*c),
                None => cancel = true,
            }
        }
    }
    if was_asking {
        let (t, q) = (tr(n_("Restart the battle")).to_string(), format!("^{}", tr(n_("Do you really want to start this battle again?"))));
        match marked_question(&t, &q) {
            Some(true) => {
                *asking = false;
                return (Some(ExitChoice::Restart), false);
            }
            Some(false) => *asking = false,
            None => {}
        }
        return (None, false);
    }
    if *asking {
        return (None, false);
    }
    (choice, cancel || key(KeyCode::Escape))
}

/// The window of [`exit_window`] and [`battle_exit_dialog`]: the warning and "Выйти из
/// игры", "Выйти в меню", "Рестарт" (after a question), "Отмена".
///
/// The map's window (`art`) is the original's "Выход из игры" (0x4d5214): the standard
/// window centred over the 1024×682 view, the castle picture with the game's name
/// (`S_Exit_RUS`) and the version line (`[SpecialTexts] VerInfo`) over it, the warning in a
/// framed box under its middle (18 px in from the sides, `(H − 117) / 2 + 65` down,
/// `(H − 117) / 2 − 20` high, the text centred in it), three buttons from the left and
/// «Отмена» on the right, 10 px from the window's foot.
fn exit_dialog(title: &str, warning: &str, asking: &mut bool, battle: bool, art: bool) -> (Option<ExitChoice>, bool) {
    let k = super::chrome::k();
    // The map's window is ae2520, 634×516 in the original's pixels (4d524b).
    let px = super::chrome::opx();
    let footer_h = 56.0 * px;
    let (w, h) = if art { (634.0 * px, 516.0 * px) } else { (560.0 * k, 250.0 * k) };
    let r = Rect::new((screen_width() - w) / 2.0, ((screen_height() - super::chrome::bar_height() - h) / 2.0).max(0.0), w, h);
    let (inner, closed) = super::chrome::window(r, title, super::chrome::Skin::Marble, true);
    if art {
        if let Some(t) = super::chrome::win("S_Exit_RUS") {
            let pw = (inner.w - 4.0 * k).min(585.0 * k);
            let ph = pw * t.height() / t.width();
            let pic = Rect::new(inner.center().x - pw / 2.0, inner.y + 2.0 * k, pw, ph.min(inner.h - 4.0 * k));
            let src = Rect::new(0.0, 0.0, t.width(), t.height() * pic.h / ph);
            super::chrome::tex_src(&t, src, pic, WHITE);
        }
        let ver = super::chrome::ui_text("SpecialTexts", "VerInfo").unwrap_or_default();
        if !ver.is_empty() {
            super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
                super::chrome::shadow_centered(&ver, inner.center().x, inner.y + 95.0 * k, 15.0 * k, VERSION_INK);
            });
        }
        // The warning's list (665470, 4d5214): 18 px in from the sides, (H − 117) / 2 + 65
        // down, (H − 117) / 2 − 20 high, its back RGB(0x30, 0x30, 0x30) taken off.
        let bh = ((516.0 - 117.0) / 2.0f32).floor();
        let tb = Rect::new(r.x + 18.0 * px, r.y + (bh + 65.0) * px, r.w - 36.0 * px, (bh - 20.0) * px);
        let shade = 0x30 as f32 / 255.0;
        super::chrome::subtractive(|| draw_rectangle(tb.x, tb.y, tb.w, tb.h, Color::new(shade, shade, shade, 1.0)));
        super::chrome::text_box_frame(tb);
        let size = 16.0 * k;
        let lines: Vec<String> = super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
            warning.split('\n').flat_map(|l| if l.is_empty() { vec![String::new()] } else { wrap(l, tb.w - 30.0 * k, size) }).collect()
        });
        let line_h = 18.0 * k;
        let mut y = tb.y + ((tb.h - lines.len() as f32 * line_h) / 2.0).max(4.0 * k) + size * 0.8;
        super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
            for line in &lines {
                super::chrome::shadow_centered(line, tb.center().x, y, size, WARNING_INK);
                y += line_h;
            }
        });
    } else {
        let size = 13.0 * k;
        let mut y = inner.y + 22.0 * k;
        for line in warning.split('\n').flat_map(|l| if l.is_empty() { vec![String::new()] } else { wrap(l, inner.w - 30.0 * k, size) }) {
            super::chrome::shadow_centered(&line, inner.center().x, y, size, super::chrome::CREAM);
            y += 16.0 * k;
        }
    }
    let mut labels = vec![
        (own("ExitGame", "Exit", n_("Quit the game")), Some(ExitChoice::Quit)),
        (own("ExitGame", "ExitToMenu", n_("To the menu")), Some(ExitChoice::MainMenu)),
        (own("ExitGame", "Restart", n_("Restart")), Some(ExitChoice::Restart)),
    ];
    // Razdor's «Клавиши» in the game menu: on phones only (the key list over the map; phones
    // have no F1). The desktop menu is the original's five-less row, as before.
    let phone_menu = art && super::display::dp_px().is_some();
    let mut widths = vec![150.0 * k, 150.0 * k, if phone_menu { 110.0 * k } else { 120.0 * k }];
    // The phone menu's five buttons take a little less between them (4 px, not 7.5) and
    // «Клавиши» and «Отмена» are narrower, so the row keeps the labels whole.
    let gap = if phone_menu { 4.0 * k } else { 7.5 * k };
    if phone_menu {
        labels.push((tr(n_("Keys")).to_string(), Some(ExitChoice::Keys)));
        widths.push(104.0 * k);
    }
    labels.push((own("Buttons", "Cancel", n_("Cancel")), None));
    widths.push(if phone_menu { 84.0 * k } else { 90.0 * k });
    let n = labels.len();
    // The phone's row fits the window: the buttons shrink together when they would not.
    let fit = if phone_menu { ((r.w - 18.75 * k - gap * (n as f32 - 1.0)) / widths.iter().sum::<f32>()).min(1.0) } else { 1.0 };
    let bw = (inner.w - 50.0 * k) / 4.0;
    let button_h = 36.0 * 0.9375 * k;
    let by = if art { r.bottom() - (footer_h + button_h) / 2.0 } else { inner.y + inner.h - 42.0 * k };
    // The original's buttons: three from the left 8 apart, «Отмена» on the right.
    let mut choice = None;
    let mut cancel = closed;
    // The restart question opens on the next frame, so the click on «Рестарт» cannot also
    // answer it (its «Нет» lies over that button).
    let was_asking = *asking;
    for (i, (label, what)) in labels.iter().enumerate() {
        let b = if art {
            let x = if i == n - 1 { r.right() - 9.375 * k - widths[i] * fit }
                else { r.x + 9.375 * k + widths[..i].iter().map(|w| w * fit + gap).sum::<f32>() };
            Rect::new(x, by, widths[i] * fit, button_h)
        } else {
            Rect::new(inner.x + 10.0 * k + i as f32 * (bw + 10.0 * k), by, bw, 30.0 * k)
        };
        let (_, down, pressed) = super::widgets::button_state(b, !*asking);
        super::chrome::marble_button(b, label, true, down);
        if pressed {
            match what {
                Some(ExitChoice::Restart) => *asking = true,
                Some(c) => choice = Some(*c),
                None => cancel = true,
            }
        }
    }
    if art {
        super::chrome::window_inner_frames(r, px);
    }
    if was_asking {
        let (t, q) = if battle {
            (tr(n_("Restart the battle")).to_string(), format!("^{}", tr(n_("Do you really want to start this battle again?"))))
        } else {
            let t = own("MessageBox", "Restart_Title", n_("Restart the game"));
            // The restart box reads its text as markup (0x4bf748 → 0x48e438); Razdor's own
            // text is centred as the install's (`^`).
            let q = (razdor::i18n::lang() == razdor::i18n::Lang::Ru)
                .then(|| super::chrome::ui_text("MessageBox", "Restart_Text"))
                .flatten()
                .unwrap_or_else(|| format!("^{}", tr(n_("Do you really want to start the scenario under way again from the beginning?"))));
            (t, q)
        };
        match marked_question(&t, &q) {
            Some(true) => {
                *asking = false;
                return (Some(ExitChoice::Restart), false);
            }
            Some(false) => *asking = false,
            None => {}
        }
        return (None, false);
    }
    if *asking {
        return (None, false);
    }
    (choice, cancel || key(KeyCode::Escape))
}

/// A question with Yes / No, `None` until answered (Esc or N: no, any other key: yes, as
/// the original's box but for Razdor's N: `answer_key`).
pub(super) fn question(title: &str, text: &str) -> Option<bool> {
    question_box(title, text, false)
}

/// [`question`] for the original's boxes whose text is markup (the restart and delete-save
/// boxes, 0x48e438): each line in its font, centred or justified, no shadow.
fn marked_question(title: &str, text: &str) -> Option<bool> {
    question_box(title, text, true)
}

fn question_box(title: &str, text: &str, marked: bool) -> Option<bool> {
    let k = super::chrome::k();
    let (w, h) = (380.0 * k, 150.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let (inner, _) = super::chrome::window(r, title, super::chrome::Skin::Marble, false);
    let size = 13.0 * k;
    let (room, step) = (inner.w - 20.0 * k, 17.0 * k);
    if marked {
        // `ui_text` gave the install's `#\` breaks as new lines: back to a break for 0x48e438.
        let rows = markup_rows(&text.replace('\n', "#\\"), room, size);
        for (i, row) in rows.iter().take(3).enumerate() {
            draw_markup_row(row, inner.x + 10.0 * k, inner.y + 24.0 * k + i as f32 * step, room, size);
        }
    } else {
        for (i, line) in wrap(text, room, size).iter().take(3).enumerate() {
            super::chrome::shadow_centered(line, inner.center().x, inner.y + 24.0 * k + i as f32 * step, size, super::chrome::CREAM);
        }
    }
    let bw = 90.0 * k;
    let by = inner.y + inner.h - 38.0 * k;
    let yes = Rect::new(inner.center().x - bw - 8.0 * k, by, bw, 28.0 * k);
    let no = Rect::new(inner.center().x + 8.0 * k, by, bw, 28.0 * k);
    let (yes_label, no_label) = (own("Buttons", "Yes", n_("Yes")), own("Buttons", "No", n_("No")));
    let (_, yes_down, yes_pressed) = super::widgets::button_state(yes, true);
    super::chrome::marble_button(yes, &yes_label, true, yes_down);
    let (_, no_down, no_pressed) = super::widgets::button_state(no, true);
    super::chrome::marble_button(no, &no_label, true, no_down);
    let answer = answer_key();
    if yes_pressed || answer == Some(true) {
        return Some(true);
    }
    if no_pressed || answer == Some(false) {
        return Some(false);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 0x4b6338, 0x47d71c: the picked save's lines keep their fonts with (0, 30, 90) added.
    #[test]
    fn the_picked_save_s_lines_are_their_fonts_with_the_highlight_added() {
        let rgb = |c: Color| [(c.r * 255.0).round() as u8, (c.g * 255.0).round() as u8, (c.b * 255.0).round() as u8];
        assert_eq!(rgb(row_ink(NAME_INK, false)), [255, 255, 155]);
        assert_eq!(rgb(row_ink(NAME_INK, true)), [255, 255, 245], "the name almost white");
        assert_eq!(rgb(row_ink(SCENARIO_INK, true)), [230, 255, 220]);
        assert_eq!(rgb(row_ink(TIME_INK, true)), [235, 195, 165], "the time pale, not orange");
    }

    #[test]
    fn over_the_map_the_book_stands_centred_over_the_view() {
        // 1024×768 with the 86 px bar: the original's in-game load and save stand at y ~104.
        let top = book_top(768.0, 86.0, 482.0 * 1.0667, 1.0667, true);
        assert!((top - 84.0).abs() < 25.0, "{top}");
        // Over the menu: the video's y 190.
        assert_eq!(book_top(768.0, 86.0, 482.0, 1.0, false), 190.0);
    }

    #[test]
    fn a_loaded_map_games_row_width_holds_for_the_session() {
        use razdor::rules::content::HeroClass;
        use razdor::rules::formation::Formation;
        let wide = Arc::new(Content::builtin().with_formation(Formation::WIDE));
        let mut game = Game::new(Arc::new(wide.with_formation(Formation::VANILLA)), HeroClass::Knight);
        // The demo is not the install's: its width does not carry.
        assert!(Arc::ptr_eq(&session_content(Some(wide.clone()), &game).unwrap(), &wide));
        game.origin = Some(save::ScenarioRef::Map { file: "m".into(), hash: 0 });
        assert_eq!(session_content(Some(wide.clone()), &game).unwrap().formation, Formation::VANILLA);
        assert!(session_content(None, &game).is_none());
    }

    #[test]
    fn autosaves_are_on_unless_the_install_turns_them_off() {
        assert!(autosaves_on(None), "no install, or no OptValue8: Razdor autosaves");
        assert!(autosaves_on(Some("1")));
        assert!(!autosaves_on(Some("0")));
        assert!(!autosaves_on(Some("")));
    }

    /// 0x4c0dd0, 0x49c94c: the new save's row shows the computer's date, in the saved rows' text.
    #[test]
    fn the_new_save_row_shows_the_computer_s_date_like_the_saved_rows() {
        let now = 1_790_000_000;
        let row = new_save_row("", Default::default(), true, "Map", now);
        assert_eq!(row.time, time_line(now));
        assert_eq!(row.time, razdor::trf!("Time: {date}", date = saved_label(now)));
        assert_eq!(row.scenario, "Map");
        let typed = new_save_row("Mine", Default::default(), false, "Map", now);
        assert_eq!(typed.time, row.time, "the date does not change while typing");
    }

    /// 0x4c0dd0: a new save starts blank with the grey placeholder, which goes once typed.
    #[test]
    fn the_placeholder_shows_only_while_the_name_is_empty() {
        let blank = new_save_row("", Default::default(), true, "Map", 0);
        assert!(blank.placeholder && blank.caret.is_none() && blank.mark.is_none());
        assert!(!blank.name.is_empty());
        let typed = new_save_row("A", super::super::text_edit::TextEdit { caret: 1, anchor: 1 }, true, "Map", 0);
        assert!(!typed.placeholder);
        assert_eq!(typed.name, "A|", "the caret blinks on at the end");
        assert_eq!(typed.caret, Some(1));
    }

    /// 0x4c0b68: Save with an empty name (or only spaces) writes nothing.
    #[test]
    fn an_empty_name_refuses_to_save() {
        assert!(!can_save("", true));
        assert!(!can_save("   ", true));
        assert!(!can_save("Mine", false), "no save folder");
        assert!(can_save(" Mine ", true));
    }

    /// A long name keeps the caret's side in sight, the far end cut.
    #[test]
    fn a_long_typed_name_keeps_the_caret_side_in_sight() {
        // Ten pixels a character.
        let w = |t: &str| t.chars().count() as f32 * 10.0;
        let name = "Магическое подземелье Магическое подземелье|";
        let end = caret_window(name, name.chars().count() - 1, 120.0, w);
        assert_eq!(end, " подземелье|", "the caret's end in sight, twelve characters");
        let start = caret_window(name, 0, 120.0, w);
        assert_eq!(start, "Магическое п", "the start in sight, the far end left out");
    }

    #[test]
    fn real_time_labels() {
        assert_eq!(real_time(0), "1970-01-01 00:00");
        assert_eq!(real_time(1_790_000_000), "2026-09-21 14:13");
    }

    #[test]
    fn the_save_name_asks_for_the_keyboard_unless_input_is_blocked() {
        use crate::ui::widgets::{keyboard_request, reset_keyboard_request};
        reset_keyboard_request();
        ask_keyboard("Сейв", Default::default(), true);
        assert_eq!(keyboard_request(), None, "a blocked window asks for nothing");
        // The caret of the typed name goes to the strip, as with the other fields.
        reset_keyboard_request();
        let edit = crate::ui::text_edit::TextEdit::all("Сейв");
        ask_keyboard("Сейв", edit, false);
        assert_eq!(keyboard_request(), Some(("Сейв".to_string(), edit.caret)));
    }
}
