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
    /// The name field has the on-screen keyboard (a phone: after a tap in it, `widgets::name_keyboard`).
    typing: bool,
}

impl SaveView {
    /// The name starts empty (0x4c0dd0: a new save has no suggested name); the game is not
    /// needed for that any more, the row shows the scenario's title while the window is open.
    pub fn new(_game: &Game, back: Back) -> SaveView {
        SaveView { name: String::new(), edit: Default::default(), entries: listed(SaveKind::Manual), asked: None, settled: false, scroll: 0, error: None, back, typing: false }
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
    /// The message box of a folder that could not be opened (Razdor's own: «Папка»), with
    /// the folder's path in it.
    folder_error: Option<String>,
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
        let mut v = LoadView { confirm_clear: false, confirm_delete: None, tab: first, entries: Vec::new(), asked: None, settled: false, selected: 0, scroll: 0, back, folder_error: None };
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
    let Some(dir) = save::games_dir() else { return };
    if !autosaves_on(super::chrome::has_texts().then(|| super::chrome::options_value("OptValue8")).flatten().as_deref()) {
        map_slot_only(&dir, game, name);
        return;
    }
    if let Err(e) = save::write_autosave(&dir, name, game, in_battle) {
        razdor::diag!("autosave: {e}");
    }
}

/// With the install's autosaves off, the map's slot still takes the game (Razdor's, so «Продолжить»
/// has the map's last game); the demo has none.
fn map_slot_only(dir: &std::path::Path, game: &Game, name: &str) {
    if save::map_slot_key(game).is_none() {
        return;
    }
    if let Err(e) = save::write_map_slot(dir, name, game) {
        razdor::diag!("autosave: {e}");
    }
}

/// The noon report's autosave (Razdor's): the frame takes the game's JSON, and a writer thread
/// packs and writes it ([`save::write_autosave_in_background`]). The same file and name as
/// [`autosave`], with the same rotation.
pub fn autosave_at_noon(game: &Game, name: &str) {
    save::finish_pending_writes();
    let Some(dir) = save::games_dir() else { return };
    if !autosaves_on(super::chrome::has_texts().then(|| super::chrome::options_value("OptValue8")).flatten().as_deref()) {
        map_slot_only(&dir, game, name);
        return;
    }
    if let Err(e) = save::write_autosave_in_background(dir, name, game) {
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
pub fn battle(game: &mut Game, session: Option<&mut super::session::Keeper>) -> Screen {
    let name = battle_name(game);
    let foe = game.foe.take();
    autosave(game, &name, true);
    // The session slot holds the same moment, the foe taken off too (`None`: a test play has none).
    if let Some(session) = session {
        session.save_now(game);
    }
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

/// `real_time` of `secs` moved by `offset` seconds (the local zone's, see `local_offset`), never
/// before 1970.
fn local_real_time(secs: u64, offset: i64) -> String {
    real_time(u64::try_from(secs as i64 + offset).unwrap_or(0))
}

/// The computer's clock minus UTC, in seconds, at the moment `secs` (daylight saving as it was
/// then). The original shows the computer's local time (0x49c94c), not Greenwich time; 0 when
/// the zone cannot be read.
#[cfg(unix)]
fn local_offset(secs: u64) -> i64 {
    let t = secs as libc::time_t;
    // SAFETY: `tm` is plain old data, zeroed before `localtime_r` fills it in.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&t, &mut tm) }.is_null() {
        return 0;
    }
    // A `c_long`: i64 on 64-bit targets (where the conversion is a no-op), i32 on 32-bit ones.
    #[allow(clippy::useless_conversion)]
    let off = i64::from(tm.tm_gmtoff);
    off
}

/// The same as `local_offset` on Windows, through kernel32 (no extra crate): the UTC time as a
/// FILETIME, its local SYSTEMTIME, and both back as FILETIMEs; the difference is the offset.
#[cfg(windows)]
fn local_offset(secs: u64) -> i64 {
    use std::ffi::c_void;

    #[repr(C)]
    #[derive(Default)]
    struct SystemTime {
        year: u16,
        month: u16,
        day_of_week: u16,
        day: u16,
        hour: u16,
        minute: u16,
        second: u16,
        milliseconds: u16,
    }

    extern "system" {
        fn FileTimeToSystemTime(file_time: *const u64, system_time: *mut SystemTime) -> i32;
        fn SystemTimeToTzSpecificLocalTime(zone: *const c_void, utc: *const SystemTime, local: *mut SystemTime) -> i32;
        fn SystemTimeToFileTime(system_time: *const SystemTime, file_time: *mut u64) -> i32;
    }

    // FILETIME: 100-ns ticks since 1601-01-01; the Unix epoch is 11 644 473 600 s after it.
    let utc_ft = (secs + 11_644_473_600) * 10_000_000;
    let (mut utc, mut local, mut local_ft) = (SystemTime::default(), SystemTime::default(), 0u64);
    // SAFETY: every pointer is to a live local of the right type; a null zone means this computer's.
    unsafe {
        if FileTimeToSystemTime(&utc_ft, &mut utc) == 0
            || SystemTimeToTzSpecificLocalTime(std::ptr::null(), &utc, &mut local) == 0
            || SystemTimeToFileTime(&local, &mut local_ft) == 0
        {
            return 0;
        }
    }
    (local_ft as i64 - utc_ft as i64) / 10_000_000
}

/// Any other target: UTC.
#[cfg(not(any(unix, windows)))]
fn local_offset(_secs: u64) -> i64 {
    0
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
    let name = if m.name == save::QUICK_SAVE || m.name == save::SESSION { tr(&m.name).to_string() } else { m.name.clone() };
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

/// "2023 год, 1 месяц, 31 день, 13:01": when a file was saved, in the original's words, on the
/// computer's local clock (0x49c94c).
fn saved_label(secs: u64) -> String {
    // A snapshot shows its fixed UTC date whatever the machine's zone, so its pictures stay the same.
    let offset = if crate::ui::clock::is_snapshot() { 0 } else { local_offset(secs) };
    let t = local_real_time(secs, offset);
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

/// The book window on a phone held upright, in dp (the owner's notes, 2026-11-02: the window fills
/// the width, the list is taller and its text reads at 14-15 sp, the buttons are finger-sized). A
/// phone choice, not in the original; the desktop and a phone held sideways keep the book as it is.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Upright {
    /// Physical px per dp.
    dp: f32,
    /// The window's px per video unit (its scale).
    k: f32,
}

/// The phone book's page height, a list row's pitch and the bottom row's button height, in dp.
const PAGE_DP: f32 = 360.0;
const ROW_DP: f32 = 36.0;
const BUTTON_DP: f32 = 46.0;
/// The top of the book's page in video units (the same on the desktop).
const PAGE_TOP: f32 = 60.0;

impl Upright {
    /// `dp_len` dp in video units.
    fn units(self, dp_len: f32) -> f32 {
        dp_len * self.dp / self.k
    }
    /// The page's height in video units (350 on the desktop).
    fn page(self) -> f32 {
        self.units(PAGE_DP)
    }
    /// A list row's pitch in video units.
    fn pitch(self) -> f32 {
        self.units(ROW_DP)
    }
    /// The steel line over the buttons: a gap under the page.
    fn line(self) -> f32 {
        PAGE_TOP + self.page() + self.units(8.0)
    }
    /// The bottom row's top, under the line.
    fn button_y(self) -> f32 {
        self.line() + self.units(10.0)
    }
    fn button_h(self) -> f32 {
        self.units(BUTTON_DP)
    }
    /// The whole window's height in video units.
    fn video_h(self) -> f32 {
        self.button_y() + self.button_h() + self.units(12.0)
    }
    /// A text of `sp` size in px (1 sp is 1 dp of the phone's text).
    fn sp(self, sp: f32) -> f32 {
        sp * self.dp
    }
}

/// The phone's book metrics when it is held upright, `None` elsewhere.
fn upright() -> Option<Upright> {
    let dp = super::display::dp_px()?;
    if screen_height() <= screen_width() {
        return None;
    }
    let k = super::window_layout::upright_game_k((Book::W, Book::H))?;
    Some(Upright { dp, k })
}

/// Layout of the book window: the video's load window (18:12), 594×482 pixels of the
/// 960×720 frame, scaled by `chrome::k`. On a phone held upright (`upright`) the page, the
/// bottom row and the window are taller, in dp (see [`Upright`]).
struct Book {
    win: Rect,
    k: f32,
    /// The text size's growth (`button_grow`): the rows, their text and the buttons grow with it.
    grow: f32,
    /// The footer's extra height in screen px (the window's growth, see [`book_video_h`]).
    foot: f32,
    /// The phone's metrics when held upright.
    upright: Option<Upright>,
    /// The page's height (350 on the desktop).
    page: f32,
    /// The steel line over the bottom row (426 on the desktop).
    line: f32,
    /// The bottom row's top and height (443 and 28 grown on the desktop).
    btn_y: f32,
    btn_h: f32,
}

/// The book window's height in its video pixels at the text size `g` (Razdor's own, the owner's
/// decision of 2026-10-31): 482 at Normal. The buttons' row grows with the text, and the window
/// with it; the save window's «Быстрое сохранение» takes a row of its own under the others at Large
/// and Largest (`quick_row`), since its wide label needs the room.
fn book_video_h(g: f32, quick_row: bool) -> f32 {
    if let Some(u) = upright() {
        return u.video_h();
    }
    let b = 28.0 * g;
    if quick_row && g > 1.0 { 443.0 + b + 6.0 + b + 11.0 } else { 443.0 + b + 11.0 }
}

impl Book {
    const W: f32 = 594.0;
    const H: f32 = 482.0;
    const ROWS: usize = 12;

    /// The book of window `win` with its scale `k`, growth, footer and phone metrics.
    fn layout(win: Rect, k: f32, grow: f32, foot: f32, upright: Option<Upright>) -> Book {
        let (page, line, btn_y, btn_h) = match upright {
            Some(u) => (u.page(), u.line(), u.button_y(), u.button_h()),
            None => (350.0, 426.0, 443.0, 28.0 * grow),
        };
        Book { win, k, grow, foot, upright, page, line, btn_y, btn_h }
    }

    fn at(title: &str, over_map: bool, quick_row: bool) -> (Book, bool) {
        let k = super::chrome::k();
        let grow = super::display::button_grow();
        let video_h = book_video_h(grow, quick_row);
        let foot = (video_h - Book::H) * k;
        let (w, h) = (Book::W * k, video_h * k);
        // Over the menu the video's window stands low (y 190 of 720), clear of the logo; over
        // the map the original centres it over the view above the bottom bar, as its other
        // map windows (its in-game load and save at y 104 of 768).
        let y = book_top(screen_height(), super::chrome::bar_height(), h, k, over_map);
        // On a phone the title's load window spans the width between the top inset and the footer
        // (`window_layout::title_rect`), and the window over the map the width between the inset and
        // the bar (`window_layout::over_map_rect`; sideways, the title's fit above the bar); the old placements (the
        // video's y 190 on the title, the view's centre over the map) are the fallback.
        let win = super::window_layout::window_rect(over_map, (Book::W, video_h)).unwrap_or_else(|| Rect::new((screen_width() - w) / 2.0, y, w, h));
        // Above the on-screen keyboard while its name field is typed in (`window_layout::keyboard_lifted`).
        let phone = upright();
        let win = super::window_layout::keyboard_lifted(win, Book::layout(win, k, grow, foot, phone).row_rect(0));
        let (_, closed) = super::chrome::window(win, title, super::chrome::Skin::Marble, true);
        let book = Book::layout(win, k, grow, foot, phone);
        match phone {
            // A phone's frames: over the page's bottom and the bottom row (the desktop's are the original's).
            Some(_) => {
                let px = super::chrome::opx();
                let line = book.p(0.0, book.line).y;
                super::chrome::steel_frame(Rect::new(win.x + 2.0 * px, line, win.w - 4.0 * px, 2.0 * px), px, 2);
                super::chrome::steel_frame(Rect::new(win.x + 4.0 * px, win.y + 31.0 * px, win.w - 8.0 * px, line - win.y - 31.0 * px - 4.0 * px), px, 1);
            }
            // The 634×516 window with its inner frames (ae2520, 0x48d710 from 4dc10e); the footer's growth
            // is the window's, and the frames keep their place above the buttons.
            None => super::chrome::window_inner_frames_footed(win, super::chrome::opx(), 0.0, foot),
        }
        (book, closed)
    }

    /// The rows the list shows: 12 at Normal; at Large and Largest the rows are taller, so the page
    /// shows as many as its 350 units take (Razdor's own, see [`book_video_h`]).
    fn shown(&self) -> usize {
        if let Some(u) = self.upright {
            // A phone's page: the rows that fit between its top and its bottom, two units kept free.
            return ((self.page - u.units(6.0)) / u.pitch()).floor().max(0.0) as usize;
        }
        let fit = ((410.0 - 64.0 - 27.0 * self.grow) / (29.0 * self.grow)).floor().max(0.0) as usize + 1;
        fit.min(Book::ROWS)
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
            // A phone's headings read at 14 sp (Razdor's own; the desktop's are the original's 16 px).
            let size = self.upright.map_or(16.0 * k, |u| u.sp(14.0));
            super::dt_font::legacy_metrics_if(self.upright.is_some(), || {
                super::chrome::shadow_centered(left, self.p(157.0, 0.0).x, y, size, heading);
                super::chrome::shadow_centered(right, self.p(437.0, 0.0).x, y, size, heading);
            });
        });
        let r = self.rect(17.0, 60.0, 560.0, self.page);
        match super::chrome::win("S_Save") {
            Some(t) => super::chrome::tex(&t, r, WHITE),
            None => super::chrome::text_box(r),
        }
        super::chrome::silver_frame(r, k);
    }

    fn row_rect(&self, i: usize) -> Rect {
        if let Some(u) = self.upright {
            return self.rect(17.0, PAGE_TOP + u.units(4.0) + i as f32 * u.pitch(), 560.0, u.pitch() - u.units(2.0));
        }
        // Razdor's own: the rows grow with the text size (`button_grow`), their pitch too.
        self.rect(17.0, 64.0 + i as f32 * 29.0 * self.grow, 560.0, 27.0 * self.grow)
    }

    /// The rows from `scroll` on; returns the row clicked, the delete icon clicked and the pin
    /// icon clicked (`deletable`: the selected row has both icons).
    fn rows(&self, rows: &[Row], selected: Option<usize>, scroll: &mut usize, deletable: bool) -> (Option<usize>, Option<usize>, Option<usize>) {
        let k = self.k;
        let shown = self.shown();
        let grow = self.grow;
        // The rows and the scrollbar right of them.
        let list = self.rect(17.0, 60.0, 574.0, self.page);
        swipe_rows(list, self.row_pitch());
        if list.contains(crate::ui::widgets::pointer().into()) && !input_blocked() {
            let wh = wheel();
            if wh < 0.0 && *scroll + shown < rows.len() {
                *scroll += 1;
            } else if wh > 0.0 && *scroll > 0 {
                *scroll -= 1;
            }
        }
        // Razdor's scrollbar right of the page, when the rows do not all fit.
        scrollbar(self.rect(579.0, 60.0, 12.0, self.page), scroll, shown, rows.len());
        let (mut picked, mut delete, mut pin) = (None, None, None);
        for (i, row) in rows.iter().enumerate().skip(*scroll).take(shown) {
            let r = self.row_rect(i - *scroll);
            let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
            // The signs grow with the row (`button_grow`).
            let sign = 22.0 * k * self.grow;
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
                let sign = 18.0 * k * self.grow;
                draw_pin(Rect::new(r.x + 6.0 * k, r.y + (r.h - sign) / 2.0, sign, sign), true, false);
            }
            let lh = r.h;
            let dim = |c: Color| if row.waiting { Color { a: c.a * 0.55, ..c } } else { c };
            // The name is centred on its column, never wider than the room right of the load
            // sign (its right edge at 43): 2 × (157 − 47).
            let lit = |c: [u8; 3]| dim(row_ink(c, selected == Some(i)));
            let name_color = lit(if row.placeholder { PLACEHOLDER_INK } else { NAME_INK });
            // Razdor's own: the row's text grows with the row (`button_grow`), each kept at its
            // Normal size where the grown one does not fit.
            super::dt_font::with_face(super::dt_font::Face::Subtitle, || {
                let (size, name_is_grown) = match self.upright {
                    Some(u) => (u.sp(15.0), true),
                    None => {
                        let name_normal = fit_size(&row.name, NAME_ROOM * k, 16.0 * k);
                        let name_grown = super::new_game::row_text_size(&row.name, name_normal, grow, NAME_ROOM * k);
                        let grown = name_grown > name_normal;
                        (if grown { name_grown } else { name_normal }, grown)
                    }
                };
                super::dt_font::legacy_metrics_if(name_is_grown, || {
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
            });
            let cx = self.p(437.0, 0.0).x;
            // The scenario's and the time's lines grow with the row too (their own Normal of 11 px).
            super::dt_font::with_face(super::dt_font::Face::Bold, || {
                let normal = 11.0 * k;
                let (size, grown) = match self.upright {
                    Some(u) => (u.sp(13.0), true),
                    None => {
                        let size = super::new_game::row_text_size(&row.scenario, normal, grow, 250.0 * k);
                        (size, size > normal)
                    }
                };
                super::dt_font::legacy_metrics_if(grown, || super::chrome::shadow_centered(&ellipsize(&row.scenario, 250.0 * k, size), cx, r.y + lh * 0.42, size, lit(SCENARIO_INK)));
            });
            super::dt_font::with_face(super::dt_font::Face::Body, || {
                let normal = 11.0 * k;
                let (size, grown) = match self.upright {
                    Some(u) => (u.sp(13.0), true),
                    None => {
                        let size = super::new_game::row_text_size(&row.time, normal, grow, 250.0 * k);
                        (size, size > normal)
                    }
                };
                super::dt_font::legacy_metrics_if(grown, || super::chrome::shadow_centered(&ellipsize(&row.time, 250.0 * k, size), cx, r.y + lh * 0.9, size, lit(TIME_INK)));
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

    /// A list row's pitch in px (the swipe's step).
    fn row_pitch(&self) -> f32 {
        match self.upright {
            Some(u) => u.pitch() * self.k,
            None => 29.0 * self.k,
        }
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
            // Razdor's own: the caption grows with the text size (`button_grow`), the tab's art does not.
            let g = super::display::button_grow();
            let size = super::display::grown_text(15.0 * k, g);
            // The right end is a torn tail; centre the caption over the solid spine.
            super::dt_font::legacy_metrics_if(g > 1.0, || super::chrome::shadow_centered(label, r.x + r.w * 0.4, r.y + r.h * 0.55, size, WHITE));
        });
        pressed
    }

    /// The steel line the tabs hang from, running along the window (the original draws it
    /// right of the tabs, under the book).
    fn tab_line(&self) {
        // The window's line over the buttons (0x48d710: (2, h − 56) to (w − 2, h − 54)),
        // drawn again over the hanging tabs; the footer's growth is below it. A phone's line is at its own place.
        let px = super::chrome::opx();
        let w = self.win;
        if self.upright.is_some() {
            super::chrome::steel_frame(Rect::new(w.x + 2.0 * px, self.p(0.0, self.line).y, w.w - 4.0 * px, 2.0 * px), px, 2);
            return;
        }
        super::chrome::steel_frame(Rect::new(w.x + 2.0 * px, w.bottom() - 56.0 * px - self.foot, w.w - 4.0 * px, 2.0 * px), px, 2);
    }

    /// A button of the bottom row (28 high at Normal, grown with the text size; 46 dp on a phone).
    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, self.btn_y, w, self.btn_h);
        let (_, down, pressed) = super::widgets::button_state(r, enabled);
        super::chrome::marble_button(r, label, enabled, down);
        pressed
    }

    /// A button of the save window's bottom row: drawn as [`Book::button`], but its click area is
    /// finger-sized on a phone ([`finger_hit`]).
    fn finger_button(&self, x: f32, y: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, y, w, self.btn_h);
        let (_, down, pressed) = super::widgets::button_state(finger_hit(r, super::display::dp_px()), enabled);
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
/// at all times); it asks for the on-screen keyboard only while `keyboard` says so (a phone
/// shows it after a tap in the name, `widgets::name_keyboard`). The row is drawn by
/// `new_save_row`, not by `edit_line`.
fn ask_keyboard(name: &str, edit: super::text_edit::TextEdit, blocked: bool, keyboard: bool) {
    if !blocked && keyboard {
        want_keyboard(name, edit.caret);
    }
}

/// The widest a save's name is drawn in its column, in the book's pixels: centred at 157,
/// it stays right of the selected row's load sign (21..43) with 4 px to spare.
const NAME_ROOM: f32 = 220.0;

/// The grey of the new save's placeholder name (0x4c0dd0).
const PLACEHOLDER_INK: [u8; 3] = [150, 150, 150];

/// The bottom row of the save window in its own pixels (the window is `Book::W` wide), as (x, y, width)
/// of «Быстрое сохранение», «Сохранить» and «Отмена». The original's two keep their places (369 and
/// 492); the owner's quick save (not in the original) sits 8 px left of «Сохранить», as the buttons
/// sit 7-8 px apart. The same row on a phone: the window is scaled as a whole. Razdor's own, at Large
/// and Largest (`button_grow` = `g`): the buttons are `g` times as wide, from their right ends, and the
/// quick save takes a row of its own under them at the left (`book_video_h`).
fn save_button_row(g: f32) -> [(f32, f32, f32); 3] {
    let (cancel_w, save_w) = (95.0 * g, 116.0 * g);
    let cancel_x = 587.0 - cancel_w;
    let save_x = cancel_x - 7.0 - save_w;
    let quick_w = 220.0 * g;
    if g > 1.0 {
        [(17.0, 443.0 + 28.0 * g + 6.0, quick_w), (save_x, 443.0, save_w), (cancel_x, 443.0, cancel_w)]
    } else {
        [(save_x - 8.0 - quick_w, 443.0, quick_w), (save_x, 443.0, save_w), (cancel_x, 443.0, cancel_w)]
    }
}

/// The phone's bottom row: `weights.len()` buttons in one row across the page's width (17 to 577
/// units), `gap` units apart, their widths in the weights' proportion; as (x, width) in order.
fn phone_row(weights: &[f32], gap: f32) -> Vec<(f32, f32)> {
    let total: f32 = weights.iter().sum();
    let room = 560.0 - gap * (weights.len() as f32 - 1.0);
    let mut x = 17.0;
    weights
        .iter()
        .map(|w| {
            let cell = (x, room * w / total);
            x += cell.1 + gap;
            cell
        })
        .collect()
}

/// Razdor's own «Папка» (a player's request, not the original's): its place on the desktop's
/// bottom row as (x, width), between «Очистить всё» (`clear`) and «Загрузить» (at `load_x`), 7 units
/// from each. 95 units grown with the text (as «Отмена», the same font), shrunk to the room left and centred in it.
fn folder_slot(clear: (f32, f32), load_x: f32, grow: f32) -> (f32, f32) {
    let left = clear.0 + clear.1 + 7.0;
    let right = load_x - 7.0;
    let w = (95.0 * grow).min(right - left).max(0.0);
    (left + (right - left - w) / 2.0, w)
}

/// What «Папка» shows when the folder cannot be opened: the box's message, with the folder's
/// path so the player can find it by hand. None when the system took the folder (or it was made
/// ready). The folder is made first, so it exists.
fn open_folder_notice() -> Option<String> {
    let Some(dir) = razdor::platform::app_dir() else {
        return Some(tr("Could not open the folder.").to_string());
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        razdor::diag!("{}: {e}", dir.display());
    }
    if super::main_menu::open_folder(&dir) {
        None
    } else {
        let path = folder_where(&dir);
        Some(razdor::trf!("Could not open the folder. It is here: {path}", path = path))
    }
}

/// The folder's place as the player finds it: the Files app's way on a phone («Internal storage >
/// Razdor», «On iPhone > Razdor», as the catalog writes them), the path on the desktop.
fn folder_where(dir: &std::path::Path) -> String {
    #[cfg(target_os = "android")]
    {
        let _ = dir;
        tr("Internal storage > Razdor").to_string()
    }
    #[cfg(target_os = "ios")]
    {
        let _ = dir;
        tr("On iPhone > Razdor").to_string()
    }
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        dir.display().to_string()
    }
}

/// The save window's bottom row as (x, y, width) of «Быстрое сохранение», «Сохранить» and «Отмена»:
/// on a phone held upright all three in one row (`phone_row`), else [`save_button_row`].
fn save_row(book: &Book) -> [(f32, f32, f32); 3] {
    match book.upright {
        Some(u) => {
            let row = phone_row(&[0.42, 0.29, 0.29], u.units(6.0));
            [(row[0].0, book.btn_y, row[0].1), (row[1].0, book.btn_y, row[1].1), (row[2].0, book.btn_y, row[2].1)]
        }
        None => save_button_row(book.grow),
    }
}

/// A button's click area: its drawn rect, on a phone (`dp` screen px per dp) at least 44 dp tall,
/// grown equally up and down so the row's neighbours keep their clicks at the sides; the desktop's is
/// the button itself. A phone choice, not in the original (the owner's request: finger-sized buttons).
fn finger_hit(r: Rect, dp: Option<f32>) -> Rect {
    let Some(dp) = dp.filter(|d| *d > 0.0) else { return r };
    let h = r.h.max(44.0 * dp);
    Rect::new(r.x, r.y + r.h / 2.0 - h / 2.0, r.w, h)
}

/// The message the quick save (F5, and the save window's «Быстрое сохранение») leaves on the map.
pub fn quick_save_message(game: &Game) -> String {
    match save::games_dir() {
        None => tr("No data folder for saves: set RAZDOR_SAVE_DIR.").to_string(),
        Some(dir) => match save::quick_save(&dir, game) {
            Ok(_) => tr("Quick save written (F9 loads it).").to_string(),
            Err(e) => razdor::trf!("Not saved: {e}.", e),
        },
    }
}

/// The save window ("Сохранение активной игры"): the book of manual saves under "(save
/// anew)"; a click on a save takes its name to replace it, typing edits the name. `quick` says
/// whether a quick save is allowed now (`hotkeys::allowed` for F5); the window's «Быстрое
/// сохранение» (the owner's request, not in the original) does it and closes the window.
pub fn save_screen(game: &Game, assets: &Assets, view: &mut SaveView, quick: bool, message: &mut Option<String>) -> Option<Screen> {
    world_view::backdrop(game, assets);
    // The save window at its own scale on a phone (`window_layout::over_map_scale`: upright between the
    // inset and the bar, sideways as the main menu's load window).
    let _scale = super::window_layout::over_map_scale((Book::W, book_video_h(super::display::button_grow(), true)));
    let (book, closed) = Book::at(&own("SaveGame", "Title", n_("Save the game")), true, true);
    book.page(&own("SaveGame", "GameName", n_("Name of the saved game")), &own("SaveGame", "ScenarioName", n_("Name of the scenario")));
    // Held Backspace repeats, as in the original's edit box (0x47a624, 0x475748); the caret,
    // the selection and the clipboard are Razdor's own (`widgets::edit_keys`).
    if !input_blocked() {
        edit_keys(&mut view.name, &mut view.edit, &typed_chars(), EditRules::line(60));
    }
    fill_metas(&mut view.entries, &mut view.asked, &mut view.settled, view.scroll.saturating_sub(1), None);
    let same = view.entries.iter().position(|e| e.meta.as_ref().is_some_and(|m| m.name == view.name));
    // The first row is the name being typed (a new save, or the one it replaces).
    let typed = new_save_row(&view.name, view.edit, (crate::ui::clock::now() * 2.0) as i64 % 2 == 0, &game.world.title, now_secs());
    let mut rows = vec![typed];
    rows.extend(view.entries.iter().map(row_of));
    // The name's tap target is the typed row's, grown up into the header while the list is at
    // its top (a phone: 48 dp tall; the row itself is unchanged).
    let field = super::widgets::tap_rect(book.row_rect(0), true);
    let (picked, _, _) = book.rows(&rows, Some(same.map_or(0, |i| i + 1)), &mut view.scroll, false);
    let touch = match picked {
        Some(0) => super::widgets::NameTouch::FieldTapped,
        Some(_) => super::widgets::NameTouch::RowPicked,
        None => super::widgets::name_touch(view.scroll == 0 && super::widgets::tapped_in(field)),
    };
    let phone = super::display::dp_px().is_some();
    view.typing = super::widgets::name_keyboard(phone, view.typing, touch);
    // On a phone with the keyboard up, Enter is the keyboard's key: it hides the keyboard, not OK.
    let enter = super::widgets::name_enter(phone, &mut view.typing, key(KeyCode::Enter));
    ask_keyboard(&view.name, view.edit, input_blocked(), view.typing);
    if let Some(k) = picked {
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
        let p = book.p(22.0, book.line + 9.0);
        text(&ellipsize(w, 330.0 * book.k, 13.0 * book.k), p.x, p.y, 13.0 * book.k, RED);
    }
    let [(quick_x, quick_y, quick_w), (save_x, save_y, save_w), (cancel_x, cancel_y, cancel_w)] = save_row(&book);
    if book.finger_button(quick_x, quick_y, quick_w, tr(n_("Quick save")), quick) {
        *message = Some(quick_save_message(game));
        return Some(Screen::WorldMap);
    }
    let ok = can_save(&view.name, dir.is_some());
    if book.finger_button(save_x, save_y, save_w, &own("SaveGame", "Save", n_("Save")), ok) || (ok && enter) {
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
    if book.finger_button(cancel_x, cancel_y, cancel_w, &own("Buttons", "Cancel", n_("Cancel")), true) || closed {
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
/// The mod row of a refused save's box (1552): set by the App each frame, read back by the box.
#[derive(Default)]
pub struct LackingBox {
    /// A refused save is being told (the mod row is shown).
    pub active: bool,
    /// The mod button: its label and whether it takes a click.
    pub button: Option<(String, bool)>,
    /// The message of the box when it differs from the save's refusal (the mod the window offers
    /// ends the text: what the button does, 1552).
    pub message: Option<String>,
    /// The line under the buttons: why the button is off, or what is being done.
    pub reason: Option<String>,
    /// Something is being done (checking, preparing the mod, downloading): OK and the button wait.
    pub busy: bool,
    /// The mod button was pressed this frame (taken by the App).
    pub pressed: bool,
    /// The download's question is up (its text): the box asks Yes or No instead of telling.
    pub asking: Option<String>,
    /// The Yes or No of the question, taken by the App.
    pub answer: Option<bool>,
}

pub fn load_screen(game: Option<&Game>, assets: &Assets, view: &mut LoadView, pending: &mut Option<PathBuf>, error: &mut Option<String>, lacking: &mut LackingBox) -> Option<Screen> {
    match game {
        Some(g) if view.back != Back::Title => world_view::backdrop(g, assets),
        _ => super::main_menu::backdrop(),
    }
    let asking_delete = view.confirm_delete.is_some();
    // A failed load is told in its own message box, which takes the window until its OK.
    let failed = error.is_some();
    let asking = view.confirm_clear || asking_delete || failed || view.folder_error.is_some();
    let previously_blocked = input_blocked();
    if asking { set_input_blocked(true); }
    let over_map = game.is_some() && view.back != Back::Title;
    // The load window at its own scale on a phone: the title's (`window_layout::title_scale`) or the one
    // over the map (`window_layout::over_map_scale`, the same size as the title's when held sideways).
    let grow = super::display::button_grow();
    let _scale = super::window_layout::window_scale(over_map, (Book::W, book_video_h(grow, false)));
    let (book, closed) = Book::at(&own("LoadGame", "Title", n_("Load a saved game")), over_map, false);
    book.page(&own("LoadGame", "GameName", n_("Name of the saved game")), &own("LoadGame", "ScenarioName", n_("Name of the scenario")));
    fill_metas(&mut view.entries, &mut view.asked, &mut view.settled, view.scroll, Some(&mut view.selected));
    let rows: Vec<Row> = view.entries.iter().map(row_of).collect();
    if rows.is_empty() {
        let p = book.p(297.0, 230.0 + (book.page - 350.0) / 2.0);
        let size = book.upright.map_or(16.0 * book.k, |u| u.sp(14.0));
        super::dt_font::legacy_metrics_if(book.upright.is_some(), || super::chrome::shadow_centered(tr("No saves here yet."), p.x, p.y, size, super::chrome::CREAM));
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
            view.scroll = k.saturating_sub(book.shown() - 1);
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
    // The tabs hang from the line: a phone's line is lower with its taller page (the desktop's is 426).
    let tab_drop = book.line - 426.0;
    let tabs = [
        (MANUAL_TAB_POSITION.0, MANUAL_TAB_POSITION.1 + tab_drop, SaveKind::Manual, own("LoadGame", "PrivateSave", n_("Private")), MANUAL_TAB_TEXTURES),
        (AUTO_TAB_POSITION.0, AUTO_TAB_POSITION.1 + tab_drop, SaveKind::Auto, own("LoadGame", "AutoSave", n_("Autosaves")), AUTO_TAB_TEXTURES),
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
    // Razdor's own: the buttons grow with the text (`button_grow`): «Очистить всё» keeps its left end,
    // «Загрузить» and «Отмена» their right ends (587 and 7 units apart, as at Normal). A phone's row
    // is one row across the page (`phone_row`). «Папка» (Razdor's own, a player's request)
    // takes the room between «Очистить всё» and «Загрузить».
    let (clear, folder, load, cancel) = match book.upright {
        Some(u) => {
            let row = phone_row(&[0.34, 0.2, 0.26, 0.2], u.units(6.0));
            (row[0], row[1], row[2], row[3])
        }
        None => {
            let (cancel_w, load_w) = (95.0 * grow, 116.0 * grow);
            let cancel_x = 587.0 - cancel_w;
            let load_x = cancel_x - 7.0 - load_w;
            let clear = (17.0, 140.0 * grow);
            (clear, folder_slot(clear, load_x, grow), (load_x, load_w), (cancel_x, cancel_w))
        }
    };
    let (load_x, load_w) = load;
    let (cancel_x, cancel_w) = cancel;
    if book.button(clear.0, clear.1, tr("Clear all"), clearable) && !asking {
        view.confirm_clear = true;
        return None;
    }
    if book.button(folder.0, folder.1, tr("Folder"), true) && !asking {
        view.folder_error = open_folder_notice();
    }
    let chosen = view.entries.get(view.selected);
    if (book.button(load_x, load_w, &own("LoadGame", "Load", n_("Load")), chosen.is_some()) || key(KeyCode::Enter)) && !asking && chosen.is_some() {
        *pending = chosen.map(|e| e.path.clone());
    }
    if (book.button(cancel_x, cancel_w, &own("Buttons", "Cancel", n_("Cancel")), true) || closed) && !asking {
        return Some(view.back.screen());
    }
    set_input_blocked(previously_blocked);
    if asking { super::chrome::grey_out(book.win); }
    if let Some(message) = error.as_deref() {
        if let Some(question) = lacking.asking.clone() {
            lacking_question_box(tr("Download the mod"), &question, lacking);
        } else if load_failed_box(&own("LoadGame", "Title", n_("Load a saved game")), &lacking.message.clone().unwrap_or_else(|| message.to_string()), lacking) {
            *error = None;
        }
        return None;
    }
    if let Some(message) = view.folder_error.clone() {
        if load_failed_box(tr("Folder"), &message, &mut LackingBox::default()) { view.folder_error = None; }
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
/// and one OK (or Enter, Escape). True once it is answered. A refused save (`lacking.active`,
/// 1552) has a mod button beside OK, and a line under it when it is off.
fn load_failed_box(title: &str, message: &str, lacking: &mut LackingBox) -> bool {
    let k = super::chrome::k();
    // Razdor's own (1552): on a phone the message is 13 sp in either orientation, drawn with Razdor's
    // own metrics (`legacy_metrics_if`, as the save window's phone rows are), and the box's margins
    // and line steps are in dp too (`u`), so the OK stays below the text. The interface scale
    // gave the text about 9 sp there. The desktop keeps 13 px and its own margins (`u` is `k`).
    let phone = super::display::dp_px();
    let u = phone.unwrap_or(k);
    let (size, step) = (13.0 * u, 17.0 * u);
    let lines = super::dt_font::legacy_metrics_if(phone.is_some(), || wrap(message, 330.0 * k, size));
    let reason = match (lacking.active, lacking.reason.as_deref()) {
        (true, Some(text)) => super::dt_font::legacy_metrics_if(phone.is_some(), || wrap(text, 330.0 * k, size)),
        _ => Vec::new(),
    };
    // Razdor's own: the OK grows with the text size (`button_grow`), the box with it.
    let g = super::display::button_grow();
    // The box's width: a phone's box grows to the screen (420 dp at most), so the mod button's words fit.
    let w = if !lacking.active {
        380.0 * k
    } else if phone.is_some() {
        (420.0 * u).min(screen_width() - 24.0 * k)
    } else {
        440.0 * k
    };
    // The mod row (1552): the mod button takes the box's width less a margin and OK's; its words are
    // 14 sp (their size is not cut), on one line or, when they do not fit, on two in a taller row.
    let (bw, gap, margin) = (90.0 * k * g, 10.0 * k, 30.0 * k);
    let mw = (w - 2.0 * margin - bw - gap).max(0.0);
    let label_size = 14.0 * u;
    let mod_lines: Vec<String> = match lacking.button.as_ref() {
        Some((label, _)) if lacking.active => {
            // As many lines as the words take: the row grows with them (row_h), nothing is cut.
            super::chrome::wrap_label(label, mw - 12.0 * k, label_size)
        }
        _ => Vec::new(),
    };
    let base_row_h = if phone.is_some() { 44.0 * u } else { 28.0 * k * g };
    let row_h = if mod_lines.len() > 1 { base_row_h.max(mod_lines.len() as f32 * label_size * 1.25 + 10.0 * u) } else { base_row_h };
    // The reason's lines sit between the message and the row, with a line's room above the row.
    // A refused save's box is laid out from its parts, so the row (one or more lines tall) always
    // clears the message and the reason: the title strip, the message, the reason, then the row.
    let h = if lacking.active {
        let strip = 26.0 * k + super::chrome::title_grow();
        let reason_block = if reason.is_empty() { 0.0 } else { 4.0 * u + reason.len() as f32 * step + 8.0 * u };
        strip + 24.0 * u + lines.len() as f32 * step + reason_block + 10.0 * u + row_h + 8.0 * u
    } else {
        (60.0 * u + lines.len() as f32 * step + if phone.is_some() { 20.0 * u } else { 0.0 }).max(150.0 * u) + super::chrome::title_grow()
    };
    let h = h + 28.0 * k * (g - 1.0);
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let inner = message_window(r, title, phone.is_some(), u);
    super::dt_font::legacy_metrics_if(phone.is_some(), || {
        for (i, line) in lines.iter().enumerate() {
            super::chrome::shadow_centered(line, inner.center().x, inner.y + 24.0 * u + i as f32 * step, size, super::chrome::CREAM);
        }
    });
    let bh = if lacking.active { row_h } else { 28.0 * k * g };
    let ok_y = inner.y + inner.h - 38.0 * u + (28.0 * u - bh);
    let mut mod_pressed = false;
    if lacking.active {
        let reason_top = inner.y + 24.0 * u + lines.len() as f32 * step;
        super::dt_font::legacy_metrics_if(phone.is_some(), || {
            for (i, line) in reason.iter().enumerate() {
                super::chrome::shadow_centered(line, inner.center().x, reason_top + 4.0 * u + i as f32 * step, size, super::chrome::CREAM);
            }
        });
        // The mod button and OK side by side, centred; the mod button takes the box's width left.
        let total = mw + gap + bw;
        let start = inner.center().x - total / 2.0;
        if let Some((_, enabled)) = lacking.button.clone() {
            let mr = Rect::new(start, ok_y, mw, bh);
            let on = enabled && !lacking.busy;
            let (_, down, pressed) = super::widgets::button_state(mr, on);
            super::chrome::marble_button_lines(mr, &mod_lines, on, down, label_size);
            mod_pressed = pressed;
        }
        let ok = Rect::new(start + mw + gap, ok_y, bw, bh);
        let ok_label = own("Buttons", "Ok", n_("OK"));
        let (_, down, pressed) = super::widgets::button_state(ok, !lacking.busy);
        if phone.is_some() {
            super::chrome::marble_button_sized(ok, &ok_label, !lacking.busy, down, label_size);
        } else {
            super::chrome::marble_button(ok, &ok_label, !lacking.busy, down);
        }
        lacking.pressed = mod_pressed;
        return !lacking.busy && (pressed || answer_key().is_some());
    }
    let ok = Rect::new(inner.center().x - bw / 2.0, ok_y, bw, bh);
    let ok_label = own("Buttons", "Ok", n_("OK"));
    let (_, down, pressed) = super::widgets::button_state(ok, true);
    super::chrome::marble_button(ok, &ok_label, true, down);
    pressed || answer_key().is_some()
}

/// A message window of a refused save (1552). On a phone its title is drawn at 14 sp in the title strip
/// (the shared window title is 11 sp there); on the desktop it is the window's own title. Returns the
/// window's body.
fn message_window(r: Rect, title: &str, own_title: bool, u: f32) -> Rect {
    if !own_title {
        return super::chrome::window_grown(r, title, super::chrome::Skin::Marble, false).0;
    }
    let (inner, _) = super::chrome::window_grown(r, "", super::chrome::Skin::Marble, false);
    let mid = r.y + (inner.y - r.y) / 2.0;
    super::dt_font::legacy_metrics_if(true, || super::chrome::shadow_centered(title, r.center().x, mid + 0.35 * 14.0 * u, 14.0 * u, super::chrome::CREAM));
    inner
}

/// The download's question of a refused save (1552): the question with Yes and No, in Razdor's own
/// window and sizes (the shared question box is the original's size, too small on a phone). The
/// answer goes to `lacking.answer`.
fn lacking_question_box(title: &str, question: &str, lacking: &mut LackingBox) {
    let k = super::chrome::k();
    let phone = super::display::dp_px();
    let u = phone.unwrap_or(k);
    let (size, step) = (13.0 * u, 17.0 * u);
    let lines = super::dt_font::legacy_metrics_if(phone.is_some(), || wrap(question, 330.0 * k, size));
    let g = super::display::button_grow();
    let row_h = if phone.is_some() { 44.0 * u } else { 28.0 * k * g };
    let h = (60.0 * u + lines.len() as f32 * step + row_h - 28.0 * u).max(150.0 * u) + 28.0 * k * (g - 1.0) + super::chrome::title_grow();
    let w = if phone.is_some() { (420.0 * u).min(screen_width() - 24.0 * k) } else { 440.0 * k };
    let r = Rect::new((screen_width() - w) / 2.0, (screen_height() - h) / 2.0, w, h);
    let inner = message_window(r, title, phone.is_some(), u);
    super::dt_font::legacy_metrics_if(phone.is_some(), || {
        for (i, line) in lines.iter().enumerate() {
            super::chrome::shadow_centered(line, inner.center().x, inner.y + 24.0 * u + i as f32 * step, size, super::chrome::CREAM);
        }
    });
    let (bw, bh) = (120.0 * k * g, row_h);
    let gap = 16.0 * k;
    let y = inner.y + inner.h - 38.0 * u + (28.0 * u - bh);
    let start = inner.center().x - (2.0 * bw + gap) / 2.0;
    let (yes, no) = (Rect::new(start, y, bw, bh), Rect::new(start + bw + gap, y, bw, bh));
    let (yes_label, no_label) = (own("Buttons", "Yes", n_("Yes")), own("Buttons", "No", n_("No")));
    let (_, yes_down, yes_pressed) = super::widgets::button_state(yes, true);
    let (_, no_down, no_pressed) = super::widgets::button_state(no, true);
    if phone.is_some() {
        super::chrome::marble_button_sized(yes, &yes_label, true, yes_down, 14.0 * u);
        super::chrome::marble_button_sized(no, &no_label, true, no_down, 14.0 * u);
    } else {
        super::chrome::marble_button(yes, &yes_label, true, yes_down);
        super::chrome::marble_button(no, &no_label, true, no_down);
    }
    if yes_pressed || answer_key() == Some(true) {
        lacking.answer = Some(true);
    } else if no_pressed || answer_key() == Some(false) {
        lacking.answer = Some(false);
    }
}

/// The original's delete confirmation: a parchment panel and two green buttons.
fn delete_save_question(book: &Book, name: &str) -> Option<bool> {
    let k = book.k;
    // Razdor's own: the buttons grow with the text size (`button_grow`), the box with them.
    let g = book.grow;
    let r = Rect::new(book.win.center().x - 190.0 * k, book.win.center().y - (110.0 + 16.0 * (g - 1.0)) * k, 380.0 * k, (220.0 + 32.0 * (g - 1.0)) * k);
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
    let yes = Rect::new(r.x + 55.0 * k, r.y + 179.0 * k, 90.0 * k * g, 32.0 * k * g);
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
    // The game menu's window at its own scale on a phone (`exit_dialog`): upright between the
    // top inset and the bar, sideways the large-window scale (`window_layout::exit_scale`).
    let _scale = super::window_layout::exit_scale();
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
    // The window's scale for this screen, both its variants (`exit_scale`).
    let _scale = super::window_layout::exit_scale();
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
    // Centred between the safe area's sides and between its top and the bar (on the desktop, the
    // old centring: no insets).
    let e = super::window_layout::exit_rect((sw, sh), chrome::bar_height(), super::display::safe_area(), (W * o, H * o));
    let (wx, wy) = (e.x.floor(), e.y.floor().max(2.0));
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
    // The phone's «Клавиши» (between «Рестарт» and «Отмена») takes its own slot in the row.
    let phone = super::display::dp_px().is_some();
    let row = battle_exit_row(phone, (b3.0, b2.0, b1.0));
    // (x and width, height, art, label, choice) of each button.
    type Button = ((f32, f32), f32, &'static str, String, Option<ExitChoice>);
    let mut buttons: Vec<Button> = vec![
        (row[0], b3.1, "Btn3", own("ExitGame", "Exit", n_("Quit the game")), Some(ExitChoice::Quit)),
        (row[1], b3.1, "Btn3", own("ExitGame", "ExitToMenu", n_("To the menu")), Some(ExitChoice::MainMenu)),
        (row[2], b2.1, "Btn2", own("ExitGame", "Restart", n_("Restart")), Some(ExitChoice::Restart)),
    ];
    if phone {
        buttons.push((row[3], b2.1, "Btn2", tr(n_("Keys")).to_string(), Some(ExitChoice::Keys)));
    }
    buttons.push((row[row.len() - 1], b1.1, "Btn1", own("Buttons", "Cancel", n_("Cancel")), None));
    let mut choice = None;
    let mut cancel = closed;
    let was_asking = *asking;
    for ((x, bw), bh, art, label, what) in buttons.iter() {
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

/// The leave-battle window's button row in its own pixels (the window is `W` = 634 wide), as
/// `(x, width)` in order: «Выйти из игры», «Выйти в меню», «Рестарт», then on phones «Клавиши»,
/// and «Отмена» last. `art` is the widths of the `Btn3`, `Btn2` and `Btn1` pictures.
///
/// The desktop row is the original's: 10 px from the left, 8 px between the buttons, «Отмена»
/// 10 px from the right. A phone's row takes the window less 10 px at each side, 4 px between
/// the five buttons; their widths shrink together when they would not fit (the labels are
/// shrunk to fit their buttons by `button_label`).
fn battle_exit_row(phone: bool, art: (f32, f32, f32)) -> Vec<(f32, f32)> {
    const W: f32 = 634.0;
    let (b3, b2, b1) = art;
    if !phone {
        let menu_x = 10.0 + b3 + 8.0;
        let restart_x = menu_x + b3 + 8.0;
        return vec![(10.0, b3), (menu_x, b3), (restart_x, b2), (W - 10.0 - b1, b1)];
    }
    let widths = [150.0f32, 150.0, 110.0, 104.0, 84.0];
    let gap = 4.0;
    let fit = ((W - 20.0 - gap * (widths.len() as f32 - 1.0)) / widths.iter().sum::<f32>()).min(1.0);
    let mut x = 10.0;
    widths
        .iter()
        .map(|w| {
            let row = (x, w * fit);
            x += w * fit + gap;
            row
        })
        .collect()
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
/// The game menu's art window in video px: 634×516 of the original's pixels, which are
/// `chrome::opx` (0.9375 of the interface scale) on the screen.
const EXIT_ART: (f32, f32) = (634.0 * 0.9375, 516.0 * 0.9375);

fn exit_dialog(title: &str, warning: &str, asking: &mut bool, battle: bool, art: bool) -> (Option<ExitChoice>, bool) {
    let k = super::chrome::k();
    // The map's window is ae2520, 634×516 in the original's pixels (4d524b).
    let px = super::chrome::opx();
    let footer_h = 56.0 * px;
    let (w, h) = if art { (634.0 * px, 516.0 * px) } else { (560.0 * k, 250.0 * k) };
    // The art window on a phone held upright: between the top inset and the bar (`game_rect`).
    let r = art.then(|| super::window_layout::game_rect(EXIT_ART)).flatten().unwrap_or_else(|| {
        super::window_layout::exit_rect((screen_width(), screen_height()), super::chrome::bar_height(), super::display::safe_area(), (w, h))
    });
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
    // Razdor's «Клавиши» in the game menu and in the battle's leave window: on phones only (the
    // key list over the screen; phones have no F1). The desktop windows are the original's
    // four-button rows, as before.
    let phone_menu = (art || battle) && super::display::dp_px().is_some();
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
    // The plain window's row: 10 px from the sides and between the buttons (four on the
    // desktop; a phone's five share the same row, 4 px apart).
    let plain_gap = if phone_menu { 4.0 * k } else { 10.0 * k };
    let bw = (inner.w - 20.0 * k - plain_gap * (n as f32 - 1.0)) / n as f32;
    // The buttons grow with the text size (Razdor's own, the owner's decision of 2026-10-31): their
    // bottom edge stays where it is at Normal, and they grow up (`button_grow`).
    let g = super::display::button_grow();
    let button_h0 = 36.0 * 0.9375 * k;
    let button_h = button_h0 * g;
    let by = if art { r.bottom() - (footer_h + button_h0) / 2.0 - (button_h - button_h0) } else { inner.y + inner.h - 12.0 * k - 30.0 * k * g };
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
            Rect::new(inner.x + 10.0 * k + i as f32 * (bw + plain_gap), by, bw, 30.0 * k * g)
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
    // Razdor's own: the Yes and No grow with the text size (`button_grow`), the box with them.
    let g = super::display::button_grow();
    let (w, h) = (380.0 * k, (150.0 + 28.0 * (g - 1.0)) * k);
    // Centred between the safe area's sides (no insets on the desktop: the screen's centre).
    let safe = super::display::safe_area();
    let r = Rect::new(safe.left + (screen_width() - safe.left - safe.right - w) / 2.0, (screen_height() - h) / 2.0, w, h);
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
    let (bw, bh) = (90.0 * k * g, 28.0 * k * g);
    let by = inner.y + inner.h - 38.0 * k + (28.0 * k - bh);
    let yes = Rect::new(inner.center().x - bw - 8.0 * k, by, bw, bh);
    let no = Rect::new(inner.center().x + 8.0 * k, by, bw, bh);
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

    /// A phone's bottom row is one row across the page: the buttons are 46 dp tall at 2.75 px per dp
    /// and their widths share the page's width, `gap` apart; the page's height is 360 dp.
    #[test]
    fn a_phone_book_has_one_bottom_row_of_finger_sized_buttons() {
        let u = Upright { dp: 2.75, k: 1.75 };
        assert!((u.button_h() * u.k / 2.75 - 46.0).abs() < 1e-3);
        assert!((u.page() * u.k / 2.75 - 360.0).abs() < 1e-3);
        let row = phone_row(&[0.42, 0.29, 0.29], u.units(6.0));
        assert_eq!(row.len(), 3);
        assert!((row[0].0 - 17.0).abs() < 1e-3);
        let end = row[2].0 + row[2].1;
        assert!((end - 577.0).abs() < 1e-2, "the row ends at 577 units, got {end}");
        for pair in row.windows(2) {
            assert!(pair[0].0 + pair[0].1 < pair[1].0, "buttons overlap");
        }
    }

    /// The leave-battle window's art (`Btn3`, `Btn2`, `Btn1` widths) in its own pixels.
    const ART: (f32, f32, f32) = (160.0, 128.0, 96.0);

    /// The desktop row is the original's: «Выйти из игры» 10 px in, 8 px between the buttons
    /// (Btn3, Btn3, Btn2), «Отмена» 10 px from the right; four buttons, as before.
    #[test]
    fn the_desktop_battle_exit_row_is_the_original_s_four_buttons() {
        let row = battle_exit_row(false, ART);
        assert_eq!(row, vec![(10.0, 160.0), (178.0, 160.0), (346.0, 128.0), (528.0, 96.0)]);
    }

    /// On a phone the five buttons (Quit, MainMenu, Restart, Keys, Cancel) keep their order, sit
    /// inside the window's 10 px margins with equal gaps, and never overlap.
    #[test]
    fn the_phone_battle_exit_row_has_five_buttons_that_fit_the_window() {
        let row = battle_exit_row(true, ART);
        assert_eq!(row.len(), 5, "Quit, MainMenu, Restart, Keys, Cancel");
        assert!(row[0].0 >= 10.0, "the first button starts inside the margin");
        let last = row[4];
        assert!(last.0 + last.1 <= 634.0 - 10.0 + 1e-3, "the row ends inside the margin: {last:?}");
        for pair in row.windows(2) {
            let gap = pair[1].0 - (pair[0].0 + pair[0].1);
            assert!((gap - 4.0).abs() < 1e-3, "gaps are 4 px: {row:?}");
        }
        // The art sizes do not matter on a phone: the row is the same whatever they are.
        assert_eq!(battle_exit_row(true, (300.0, 300.0, 300.0)), row);
    }

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

    /// The local label is the UTC one moved by the offset, across midnight and back before 1970.
    #[test]
    fn local_real_time_moves_the_clock_by_the_offset() {
        assert_eq!(local_real_time(1_790_000_000, 5 * 3600), "2026-09-21 19:13");
        assert_eq!(local_real_time(1_790_000_000, 10 * 3600), "2026-09-22 00:13", "past midnight");
        assert_eq!(local_real_time(1_790_000_000, -15 * 3600), "2026-09-20 23:13", "before midnight");
        assert_eq!(local_real_time(0, -3600), "1970-01-01 00:00", "never before 1970");
        assert_eq!(local_real_time(1_790_000_000, 0), real_time(1_790_000_000));
    }

    /// This computer's offset from UTC is a real zone (within ±14 h, whole quarter hours).
    #[test]
    fn the_local_offset_of_this_computer_is_a_real_zone() {
        let off = local_offset(1_790_000_000);
        assert!((-14 * 3600..=14 * 3600).contains(&off), "offset {off}");
        assert_eq!(off % 900, 0, "zones are whole quarter hours: {off}");
    }

    #[test]
    fn the_save_name_asks_for_the_keyboard_unless_input_is_blocked() {
        use crate::ui::widgets::{keyboard_request, reset_keyboard_request};
        reset_keyboard_request();
        ask_keyboard("Сейв", Default::default(), true, true);
        assert_eq!(keyboard_request(), None, "a blocked window asks for nothing");
        // The caret of the typed name goes to the strip, as with the other fields.
        reset_keyboard_request();
        let edit = crate::ui::text_edit::TextEdit::all("Сейв");
        ask_keyboard("Сейв", edit, false, true);
        assert_eq!(keyboard_request(), Some(("Сейв".to_string(), edit.caret)));
    }

    /// A phone's save window asks for nothing until the name is tapped (`keyboard` false).
    #[test]
    fn a_phone_save_window_without_a_tap_in_the_name_asks_for_no_keyboard() {
        use crate::ui::widgets::{keyboard_request, reset_keyboard_request};
        reset_keyboard_request();
        ask_keyboard("", Default::default(), false, false);
        assert_eq!(keyboard_request(), None);
    }

    /// The save window's bottom row: «Быстрое сохранение», «Сохранить» and «Отмена» fit in the window
    /// with 8 px between the first two, in order and without overlap, all inside the window's rows.
    #[test]
    fn the_save_window_button_row_holds_three_buttons_without_overlap() {
        let [quick, save, cancel] = save_button_row(1.0).map(|(x, _, w)| (x, w));
        let (left, right) = (17.0, Book::W - 7.0);
        assert!(quick.0 >= left, "the quick save is inside the window's rows: {quick:?}");
        assert!(cancel.0 + cancel.1 <= right, "«Отмена» is inside the window's rows");
        assert!(quick.0 + quick.1 + 8.0 <= save.0 + 0.5, "«Быстрое сохранение» ends 8 px before «Сохранить»: {quick:?} {save:?}");
        assert!(save.0 + save.1 + 7.0 <= cancel.0 + 0.5, "«Отмена» keeps its place after «Сохранить»");
        assert_eq!((save.0, save.1, cancel.0, cancel.1), (369.0, 116.0, 492.0, 95.0), "the original's two buttons keep their places");
        // The quick save's label is 18 characters in the button face: at least 150 px wide at this height.
        assert!(quick.1 >= 170.0, "the label «Быстрое сохранение» fits its button");
    }

    /// «Папка» on the load window's bottom row (Razdor's own) keeps 7 units from «Очистить всё»
    /// and from «Загрузить» at every text size, and is never wider than its 95 units grown.
    #[test]
    fn the_folder_button_fits_between_clear_and_load_at_every_text_size() {
        for grow in [1.0, 1.25, 1.5] {
            let (cancel_w, load_w) = (95.0 * grow, 116.0 * grow);
            let cancel_x = 587.0 - cancel_w;
            let load_x = cancel_x - 7.0 - load_w;
            let clear = (17.0, 140.0 * grow);
            let (x, w) = folder_slot(clear, load_x, grow);
            assert!(w > 0.0 && w <= 95.0 * grow + 0.001, "the folder button's width at {grow}: {w}");
            assert!(x >= clear.0 + clear.1 + 7.0 - 0.001, "«Папка» keeps 7 units from «Очистить всё» at {grow}");
            assert!(x + w <= load_x - 7.0 + 0.001, "«Папка» keeps 7 units from «Загрузить» at {grow}");
        }
    }

    /// A phone's button of the save window is finger-sized (44 dp tall at least, centred on the drawn
    /// button, so it reaches outside it at the bottom); the desktop's is the button itself.
    #[test]
    fn the_save_window_buttons_are_finger_sized_on_a_phone() {
        let (sw, sh, top) = (1080.0, 2376.0, 66.0);
        let bar = (80.0f32 * 1.125).round();
        let (k, win) = crate::ui::window_layout::portrait_game((sw, sh), Some(2.75), top, bar, 1.125, (Book::W, Book::H)).expect("held upright");
        let book = Book::layout(win, k, 1.0, 0.0, None);
        let [(quick_x, _, quick_w), (save_x, _, save_w), (cancel_x, _, cancel_w)] = save_button_row(1.0);
        for (x, w) in [(quick_x, quick_w), (save_x, save_w), (cancel_x, cancel_w)] {
            let drawn = book.rect(x, 443.0, w, 28.0);
            let hit = finger_hit(drawn, Some(2.75));
            assert!(hit.h >= 44.0 * 2.75 - 0.5, "at least 44 dp: {hit:?}");
            assert!((hit.center().y - drawn.center().y).abs() < 0.5, "centred on the button");
            assert!(hit.x == drawn.x && hit.w == drawn.w, "the sides stay the button's, so the neighbours keep theirs");
            assert!(hit.y >= book.p(0.0, 410.0).y, "the click area does not reach the list's rows above");
        }
        assert_eq!(finger_hit(book.rect(quick_x, 443.0, quick_w, 28.0), None).h, 28.0 * k, "the desktop's click area is the button");
    }

    /// The save window over the map on a phone held upright (1080×2376 at 2.75 px per dp, the
    /// interface scale 1.125, the bar 90 px): its buttons take their clicks at the window's
    /// scale, inside it, and with the on-screen keyboard up the save button is still above it.
    #[test]
    fn the_in_game_save_window_on_a_phone_clicks_at_its_own_scale() {
        let (sw, sh, top) = (1080.0, 2376.0, 66.0);
        let bar = (80.0f32 * 1.125).round();
        let (k, win) = crate::ui::window_layout::portrait_game((sw, sh), Some(2.75), top, bar, 1.125, (Book::W, Book::H)).expect("held upright");
        let book = Book::layout(win, k, 1.0, 0.0, None);
        // «Сохранить» (369, 116) and «Отмена» (492, 95), 28 design px high, as `Book::button` draws them.
        let [(_, _, _), (save_x, _, save_w), (cancel_x, _, cancel_w)] = save_button_row(1.0);
        let save = book.rect(save_x, 443.0, save_w, 28.0);
        let cancel = book.rect(cancel_x, 443.0, cancel_w, 28.0);
        assert!((save.h - 28.0 * k).abs() < 1e-3, "the button is drawn at the window's scale");
        assert!(win.contains(save.center()) && win.contains(cancel.center()), "both buttons are in the window");
        assert!(save.contains(save.center()) && !cancel.contains(save.center()), "a click on «Сохранить» hits it alone");
        // With the keyboard up (900 px), the window moves so that its name field and the save button are above it.
        let keyboard = 900.0;
        let dy = crate::ui::window_layout::keyboard_lift(win, book.row_rect(0), sh, keyboard, top);
        let lifted = Rect::new(win.x, win.y + dy, win.w, win.h);
        assert!(lifted.y >= top - 0.5 && lifted.bottom() <= sh - keyboard + 0.5, "the window is above the keyboard: {lifted:?}");
        assert!(save.y + dy + save.h <= sh - keyboard + 0.5, "the save button is above the keyboard");
    }
}
