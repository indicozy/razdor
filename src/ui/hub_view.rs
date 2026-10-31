//! "Maps and mods" (a Razdor extra): the hub's window over the main menu's
//! ruins (`razdor::hub`). Tabs above the list (all, maps, campaigns, mods); per row the
//! entry's kind, title, author and state (installed, needs mods, can't be installed). On the
//! right the picked entry: who made it, its languages (an English made by the import's
//! machine translation says so), the mods it needs with their state, its description, and
//! Install or Remove. Reading the catalog and installing run on a thread; the window only
//! shows what they say. The layout is ours: the original has no such window.
//!
//! Both the list and the details pane scroll: by the wheel, a swipe (`swipe_rows`), a drag
//! with the finger or the mouse (`drag_frame`; a drag never picks a row, a tap still does) and
//! the scrollbar beside them. The pane is clipped above the bottom row, and words too long for
//! its width (a link) are broken there (`wrap_long`).

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use razdor::hub::{self, Catalog, Entry, Hub, Installed, Kind, PlanError, Source, Status};
use razdor::i18n::{lang, tr};
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome::{self, CREAM, GOLD, RED_TEXT};
use super::input::{is_mouse_button_down, is_mouse_button_pressed};
use super::widgets::*;

const GREEN_TEXT: Color = Color::new(0.55, 0.88, 0.45, 1.0);

/// The search field's key (`widgets::text_field`).
const SEARCH_KEY: &str = "hub-search";

/// What a job on the thread came back with.
enum Done {
    Catalog(Result<Catalog, String>),
    Installed(Result<Vec<String>, String>),
    Removed(Result<String, String>),
}

pub struct HubView {
    hub: Option<Hub>,
    catalog: Option<Result<Catalog, String>>,
    installed: Installed,
    tab: usize,
    picked: Option<String>,
    /// A phone held upright shows the picked entry in place of the list (`frame`).
    detail: bool,
    scroll: usize,
    /// A press on the list, and on the details pane, that may be a drag (`drag_frame`).
    list_drag: Option<Drag>,
    pane_drag: Option<Drag>,
    /// The details pane's scroll in pixels, its content's height as the last frame drew it
    /// (the bar and the wrap width follow it), and the entry they are for (a new pick starts at
    /// the top).
    detail_scroll: f32,
    detail_h: f32,
    detail_for: Option<String>,
    /// The search field's text: the list shows the entries whose title or author match it
    /// (`matches`).
    query: String,
    job: Option<Receiver<Done>>,
    /// The last job's word: the text and whether it went wrong.
    note: Option<(String, bool)>,
    /// Something was installed or removed while the window was open (the new-game list is
    /// read again on the way out).
    pub changed: bool,
    /// The picked entry's picture (`ensure_preview`).
    preview: Option<Preview>,
}

/// The picture of an entry: its texture once it is in the cache, and the download on its way
/// when it is not.
struct Preview {
    id: String,
    texture: Option<Texture2D>,
    job: Option<Receiver<Result<PathBuf, String>>>,
}

/// A press on a scrolled area, held until the button comes up: where it began, the scroll it
/// began at, and whether the pointer has moved past the slop since (then it is a drag).
#[derive(Clone, Copy, Debug)]
struct Drag {
    at: (f32, f32),
    start: f32,
    moved: bool,
}

/// The most a preview is drawn tall (window units).
const PREVIEW_H: f32 = 150.0;

/// Keeps the picked entry's picture: loaded from the cache, or downloaded on a thread (once,
/// into the cache) when it is not there yet. A download that fails leaves no picture.
fn ensure_preview(view: &mut HubView, e: &Entry) {
    if let Some(p) = view.preview.as_mut().filter(|p| p.id == e.id) {
        if let Some(done) = p.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            p.job = None;
            match done {
                Ok(path) => p.texture = load_texture(&path),
                Err(why) => razdor::diag!("hub: the picture of {} is not shown: {why}", e.id),
            }
        }
        return;
    }
    let mut p = Preview { id: e.id.clone(), texture: None, job: None };
    if let (Some(f), Some(hub)) = (e.preview.as_ref(), view.hub.clone()) {
        let path = hub::preview_path(&hub.local, f);
        if path.is_file() {
            p.texture = load_texture(&path);
        } else {
            let (tx, rx) = channel();
            let f = f.clone();
            std::thread::spawn(move || {
                let _ = tx.send(hub.cache_preview(&f));
            });
            p.job = Some(rx);
        }
    }
    view.preview = Some(p);
}

/// A cached PNG as a texture (macroquad decodes PNG and TGA; `None` when it does not read).
fn load_texture(path: &Path) -> Option<Texture2D> {
    let bytes = std::fs::read(path).ok()?;
    let img = Image::from_file_with_format(&bytes, None).ok()?;
    Some(Texture2D::from_image(&img))
}

impl HubView {
    pub fn new() -> HubView {
        let hub = hub::local_dir().map(|local| Hub { source: Source::from_env(), local });
        let mut v = HubView {
            hub,
            catalog: None,
            installed: Installed::default(),
            tab: 0,
            picked: None,
            detail: false,
            scroll: 0,
            list_drag: None,
            pane_drag: None,
            detail_scroll: 0.0,
            detail_h: 0.0,
            detail_for: None,
            query: String::new(),
            job: None,
            note: None,
            changed: false,
            preview: None,
        };
        v.reload();
        v
    }

    /// A snapshot's setup (`ui::snapshot`): the tab and the picked entry.
    pub fn stage(&mut self, tab: usize, pick: Option<&str>) {
        self.tab = tab.min(3);
        self.picked = pick.map(Into::into);
        self.detail = pick.is_some();
    }

    /// Reads the catalog again (on the thread) and what is installed.
    fn reload(&mut self) {
        let Some(hub) = self.hub.clone() else {
            self.catalog = Some(Err(tr("No folder for the hub.").into()));
            return;
        };
        self.installed = hub.installed();
        self.catalog = None;
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(Done::Catalog(hub.catalog()));
        });
        self.job = Some(rx);
    }

    fn start(&mut self, work: impl FnOnce(Hub, Catalog) -> Done + Send + 'static) {
        let (Some(hub), Some(Ok(catalog))) = (self.hub.clone(), self.catalog.clone()) else { return };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(work(hub, catalog));
        });
        self.job = Some(rx);
        self.note = None;
    }

    /// Takes a finished job's result.
    fn poll(&mut self) {
        let Some(done) = self.job.as_ref().and_then(|rx| rx.try_recv().ok()) else { return };
        self.job = None;
        let refresh = |v: &mut HubView| {
            if let Some(h) = &v.hub {
                v.installed = h.installed();
            }
            v.changed = true;
        };
        match done {
            Done::Catalog(c) => {
                if let Ok(c) = &c {
                    if self.picked.as_ref().is_none_or(|p| c.entry(p).is_none()) {
                        self.picked = c.entries.first().map(|e| e.id.clone());
                    }
                }
                self.catalog = Some(c);
            }
            Done::Installed(Ok(ids)) => {
                cue(Cue::Button);
                refresh(self);
                let names = self.titles(&ids);
                self.note = Some((trf!("Installed: {names}", names), false));
            }
            Done::Removed(Ok(id)) => {
                refresh(self);
                let names = self.titles(&[id]);
                self.note = Some((trf!("Removed: {names}", names), false));
            }
            Done::Installed(Err(e)) | Done::Removed(Err(e)) => {
                refresh(self);
                self.note = Some((e, true));
            }
        }
    }

    fn titles(&self, ids: &[String]) -> String {
        let c = self.catalog.as_ref().and_then(|c| c.as_ref().ok());
        ids.iter().map(|id| c.and_then(|c| c.entry(id)).map_or(id.as_str(), |e| e.title.get(lang()))).collect::<Vec<_>>().join(", ")
    }
}

impl Default for HubView {
    fn default() -> Self {
        HubView::new()
    }
}

fn kind_label(k: Kind) -> &'static str {
    match k {
        Kind::Map => tr("Map"),
        Kind::Campaign => tr("Campaign"),
        Kind::Mod => tr("Mod"),
    }
}

/// The tab's entries that match the search, best rated first (`hub::by_rating`).
fn shown<'a>(c: &'a Catalog, tab: usize, query: &str) -> Vec<&'a Entry> {
    let want = [None, Some(Kind::Map), Some(Kind::Campaign), Some(Kind::Mod)][tab.min(3)];
    let mut list: Vec<&Entry> = c.entries.iter().filter(|e| want.is_none_or(|k| e.kind == k)).filter(|e| matches(e, query)).collect();
    list.sort_by(|a, b| hub::by_rating(a, b));
    list
}

/// Whether an entry matches the search `query`: its Russian or English title, or its author,
/// contains it, ignoring case. An empty query (or only spaces) matches everything.
pub fn matches(e: &Entry, query: &str) -> bool {
    matches_text(query, &[&e.title.ru, &e.title.en], &e.author)
}

/// [`matches`] on the texts: `titles` are the entry's titles in each language, `author` its author.
pub fn matches_text(query: &str, titles: &[&str], author: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    titles.iter().any(|t| t.to_lowercase().contains(&q)) || author.to_lowercase().contains(&q)
}

/// A row's state: its words and ink.
fn state(c: &Catalog, inst: &Installed, e: &Entry) -> (String, Color) {
    if inst.has(&e.id) {
        return (tr("Installed").into(), GREEN_TEXT);
    }
    match hub::plan_install(c, inst, &e.id) {
        Err(_) => (tr("Can't be installed").into(), RED_TEXT),
        Ok(plan) if plan.len() > 1 => (trf!("Needs {n} mods", n = plan.len() - 1), GOLD),
        Ok(_) => (String::new(), CREAM),
    }
}

/// The entry's state as its author or the hub's checks say it: the words and their ink.
fn status_line(e: &Entry) -> (String, Color) {
    let note = e.status_note.get(lang());
    match e.status {
        Status::Full => (tr("Works fully").into(), GREEN_TEXT),
        Status::Partial => (trf!("Works partly: {note}", note = note), GOLD),
        Status::Broken => (trf!("Does not start: {note}", note = note), RED_TEXT),
        Status::Unknown => (tr("Not checked yet").into(), Color::new(0.85, 0.80, 0.68, 1.0)),
    }
}

/// A row's wash: the blue bar when picked, a warm wash under the mouse. The click is
/// `drag_frame`'s, so a drag over the row picks nothing.
fn row_paint(r: Rect, picked: bool) {
    let over = !input_blocked() && mouse_in(r.x, r.y, r.w, r.h);
    if picked {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.12, 0.2, 0.62, 0.75));
    } else if over {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.85, 0.5, 0.12));
    }
}

/// The window's parts at its scale: `k` px per unit (one dp on a phone), the text sizes at the
/// text size `g` (`display::button_grow`), the bottom row's top `by`, height `bh` and the width
/// of its Install or Remove `bw`.
struct Lay {
    k: f32,
    g: f32,
    size: f32,
    small: f32,
    by: f32,
    bh: f32,
    bw: f32,
    /// A phone (one unit one dp): its text stops above the bottom row.
    phone: bool,
}

/// A tab on a phone: a marble button (its label grows with the button, `button_label`), the
/// picked tab ringed in gold. True when clicked.
fn tab_button(x: f32, y: f32, w: f32, h: f32, label: &str, on: bool) -> bool {
    let clicked = button_sounding(x, y, w, h, label, true, Cue::Button);
    if on {
        draw_rectangle_lines(x, y, w, h, 3.0, GOLD);
    }
    clicked
}

/// A scrollbar's width: a finger's target on a phone (24 dp), 12 units on the desktop.
fn bar_px(k: f32, phone: bool) -> f32 {
    if phone { 24.0 * k } else { 12.0 * k }
}

/// The room a list or pane gives its scrollbar on its right: the bar and a gap either side.
fn bar_room(k: f32, phone: bool) -> f32 {
    bar_px(k, phone) + 4.0 * k
}

/// The bar of an area whose right edge is `right`, from `y` for `h`.
fn bar_rect(right: f32, y: f32, h: f32, k: f32, phone: bool) -> Rect {
    let w = bar_px(k, phone);
    Rect::new(right - 2.0 * k - w, y + 2.0 * k, w, h - 4.0 * k)
}

/// Pixels a press may move before it is a drag, not a tap: 12 px of a 1080 px high screen (the
/// gestures' tap slop).
fn drag_slop() -> f32 {
    12.0 * screen_height() / 1080.0
}

/// Whether the pointer at `to` is more than `slop` pixels from `from`.
pub fn moved_past(from: (f32, f32), to: (f32, f32), slop: f32) -> bool {
    (to.0 - from.0).hypot(to.1 - from.1) > slop
}

/// A scroll kept between 0 and `most` (0 when there is nothing to scroll).
pub fn clamp_scroll(scroll: f32, most: f32) -> f32 {
    scroll.clamp(0.0, most.max(0.0))
}

/// The scroll of a drag begun at scroll `start` with the pointer at `from_y`, now at `now_y`:
/// the content follows the finger (dragged up, the scroll grows), `per` pixels a unit.
pub fn drag_to(start: f32, from_y: f32, now_y: f32, per: f32, most: f32) -> f32 {
    clamp_scroll(start + (from_y - now_y) / per, most)
}

/// The list's row under `at`: `area` is the rows' rectangle (the bar is not in it), `first` the
/// top row shown, `count` the entries. None outside the rows.
pub fn row_index(at: (f32, f32), area: Rect, row_px: f32, first: usize, count: usize) -> Option<usize> {
    if !(at.0 >= area.x && at.0 < area.x + area.w && at.1 >= area.y && at.1 < area.y + area.h) {
        return None;
    }
    let n = first + ((at.1 - area.y) / row_px).floor() as usize;
    (n < count).then_some(n)
}

/// `line` in pieces that each `fits`: a word too wide for the box (a link) is broken where the
/// next character would not fit; a single character too wide stands alone.
pub fn break_long(line: &str, fits: impl Fn(&str) -> bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in line.chars() {
        cur.push(c);
        if !fits(&cur) && cur.chars().count() > 1 {
            cur.pop();
            out.push(std::mem::take(&mut cur));
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// [`wrap_cached`] with the words too wide for `width` broken (`break_long`): a long link wraps
/// inside the box instead of running off its edge.
fn wrap_long(s: &str, width: f32, size: f32) -> Vec<String> {
    wrap_cached(s, width, size).iter().flat_map(|l| break_long(l, |t| measure(t, size).width <= width)).collect()
}

/// One frame of a press on a scrolled area `area`: a press inside it starts a drag; the
/// pointer's slide past `slop` moves the scroll (`drag_to`: `per` pixels a unit, `most` the
/// most). Returns the scroll now, and where the press began when the button came up without a
/// drag (a click there).
fn drag_frame(drag: &mut Option<Drag>, area: Rect, scroll: f32, per: f32, most: f32, slop: f32) -> (f32, Option<(f32, f32)>) {
    let now = pointer();
    if drag.is_none() && !input_swallowed() && is_mouse_button_pressed(MouseButton::Left) && mouse_in(area.x, area.y, area.w, area.h) {
        *drag = Some(Drag { at: now, start: scroll, moved: false });
    }
    let Some(d) = drag.as_mut() else { return (scroll, None) };
    if !is_mouse_button_down(MouseButton::Left) {
        let click = (!d.moved).then_some(d.at);
        *drag = None;
        return (scroll, click);
    }
    if !d.moved && moved_past(d.at, now, slop) {
        d.moved = true;
    }
    if d.moved {
        (drag_to(d.start, d.at.1, now.1, per, most), None)
    } else {
        (scroll, None)
    }
}

/// Draws the window; true when it is closed.
pub fn frame(view: &mut HubView) -> bool {
    view.poll();
    super::main_menu::backdrop();
    let phone = super::display::dp_px().is_some();
    let (k, r) = match super::display::dp_px() {
        Some(dp) => {
            // A phone: the screen less its safe area and the bottom bar, one unit one dp.
            let (sw, sh) = (screen_width(), screen_height());
            let safe = super::display::safe_area();
            let m = 6.0 * dp;
            (dp, Rect::new(safe.left + m, safe.top + m, sw - safe.left - safe.right - 2.0 * m, sh - safe.top - chrome::bar_height() - 2.0 * m))
        }
        None => {
            let k = chrome::k();
            let (w, h) = (900.0 * k, 560.0 * k);
            (k, Rect::new((screen_width() - w) / 2.0, (150.0 * k).min(screen_height() - h).max(0.0), w, h))
        }
    };
    // The window's own scale: its frame and everything in it read `chrome::k`.
    let _scale = super::display::enter_window_scale(Some(k));
    let g = super::display::button_grow();
    let (inner, closed) = chrome::window(r, tr("Maps and mods"), chrome::Skin::Marble, true);

    let narrow = phone && inner.w < 560.0 * k;
    let pad = 10.0 * k;
    let size = (14.0 * k * g).round();
    let small = (12.0 * k * g).round();
    // Touch targets: a phone's are 44 dp at Normal (the 40 dp rule), the desktop's the original's.
    let (bh, th, row_h, gap) = if phone { (44.0 * k * g, 44.0 * k * g, 46.0 * k * g, 8.0 * k) } else { (30.0 * k * g, 26.0 * k * g, 34.0 * k * g, 4.0 * k) };
    let by = inner.y + inner.h - pad - bh;
    let lx = inner.x + pad;
    // The list's width: the desktop's 380 units; a phone held sideways takes more of the window,
    // so its four tabs keep room for their labels at the grown size.
    let lw = if narrow {
        inner.w - 2.0 * pad
    } else if phone {
        (inner.w * 0.6).min(600.0 * k)
    } else {
        380.0 * k
    };
    let ty = inner.y + pad;
    // A phone held upright lays its four tabs out two by two, so each keeps its label whole.
    let cols = if narrow { 2 } else { 4 };
    // Portrait: the search field's row sits under the tabs (`field_y`) and the list under it.
    // Elsewhere (a phone held sideways, the desktop) the field is in the bottom row, so no list row
    // is lost there.
    let tabs_h = if narrow { 2.0 * th + gap } else { th };
    let field_y = ty + tabs_h + gap;
    let head = tabs_h + gap + if narrow { th } else { 0.0 };
    let list_y = ty + head + 8.0 * k;
    // A phone held upright keeps 40 units above the bottom row for the job's word.
    let list_bottom = by - if narrow { 40.0 * k } else { 16.0 * k };
    let rows = ((list_bottom - list_y) / row_h).floor().max(1.0) as usize;
    let list = Rect::new(lx, list_y, lw, rows as f32 * row_h);
    let detail = narrow && view.detail && view.picked.is_some();
    let back_w = 140.0 * k * g;
    let bw = if narrow { lw - back_w - gap } else { 220.0 * k * g };
    let lay = Lay { k, g, size, small, by, bh, bw, phone };
    if !detail {
        chrome::text_box(list);
    }

    let tabs = [tr("All"), tr("Maps"), tr("Campaigns"), tr("Mods")];
    let tab_w = lw / cols as f32;
    if !detail {
        for (i, t) in tabs.iter().enumerate() {
            let (x, y) = if narrow { (lx + (i % 2) as f32 * tab_w, ty + (i / 2) as f32 * (th + gap)) } else { (lx + i as f32 * tab_w, ty) };
            let w = tab_w - gap;
            let clicked = if phone { tab_button(x, y, w, th, t, view.tab == i) } else { toggle_button(x, y, w, th, t, view.tab == i) };
            if clicked && view.tab != i {
                view.tab = i;
                view.scroll = 0;
            }
        }
    }

    // The search field (a tap in it types; the phone's keyboard comes only then, as the
    // dropdowns' filter does) and its clear button: above the list in portrait, in the bottom row
    // between «Обновить» and «Установить» elsewhere.
    let search_esc = if !detail {
        let step = 150.0 * k * g;
        let bx = inner.x + inner.w - 10.0 * k - bw;
        let x0 = lx + step + back_w + gap;
        let (fx, fy, fh, avail) = if narrow { (lx, field_y, th, lw) } else { (x0, by, bh, bx - gap - x0) };
        // The clear control is a square as tall as the field (44 dp on a phone, grown with the
        // text size), drawn with the window's own close cross (`chrome::close_button`).
        let sq = fh;
        let field_w = avail - sq - gap;
        let was_typing = has_focus(SEARCH_KEY);
        let before = view.query.clone();
        text_field(SEARCH_KEY, fx, fy, field_w, fh, &mut view.query, false);
        if view.query != before {
            view.scroll = 0;
        }
        if view.query.is_empty() && !has_focus(SEARCH_KEY) {
            text_fit(tr("Search…"), fx + 8.0 * k, fy + fh / 2.0 + small * 0.35, field_w - 16.0 * k, small, Color::new(0.85, 0.80, 0.68, 0.7));
        }
        if !view.query.is_empty() && chrome::close_button(fx + field_w + gap, fy, sq, false) {
            view.query.clear();
            view.scroll = 0;
        }
        // Esc while the field has the keyboard clears the search and does not close the window.
        was_typing && esc_pressed()
    } else {
        false
    };
    if search_esc {
        view.query.clear();
        view.scroll = 0;
    }

    let catalog = view.catalog.clone();
    match &catalog {
        None if !detail => chrome::shadow_text(tr("Reading the hub…"), lx + 12.0 * k, list.y + 26.0 * k, size, CREAM),
        None => {}
        Some(Err(e)) if !detail => {
            for (i, l) in wrap(&trf!("The hub can't be read: {e}", e), lw - 24.0 * k, size).iter().enumerate() {
                chrome::shadow_text(l, lx + 12.0 * k, list.y + 26.0 * k + i as f32 * 20.0 * k, size, RED_TEXT);
            }
        }
        Some(Err(_)) => {}
        Some(Ok(c)) => {
            let entries = shown(c, view.tab, &view.query);
            let max_scroll = entries.len().saturating_sub(rows);
            if !detail {
                // The bar takes room only when the list scrolls.
                let room = if max_scroll > 0 { bar_room(k, phone) } else { 0.0 };
                let area = Rect::new(lx, list.y, lw - room, list.h);
                swipe_rows(area, row_h);
                if !input_blocked() && mouse_in(area.x, area.y, area.w, area.h) {
                    let wh = wheel();
                    if wh < 0.0 {
                        view.scroll = (view.scroll + 1).min(max_scroll);
                    } else if wh > 0.0 {
                        view.scroll = view.scroll.saturating_sub(1);
                    }
                }
                let (scroll, click) = drag_frame(&mut view.list_drag, area, view.scroll as f32, row_h, max_scroll as f32, drag_slop());
                view.scroll = (clamp_scroll(scroll, max_scroll as f32).round() as usize).min(max_scroll);
                scrollbar(bar_rect(lx + lw, list.y, list.h, k, phone), &mut view.scroll, rows, entries.len());
                // A tap (or a press released without a drag) picks the row it began on.
                if let Some(e) = click.and_then(|at| row_index(at, area, row_h, view.scroll, entries.len())).map(|n| entries[n]) {
                    if view.picked.as_deref() != Some(e.id.as_str()) {
                        cue(Cue::Button);
                        view.picked = Some(e.id.clone());
                    }
                    // A phone shows the picked entry in place of the list (`narrow`).
                    view.detail = true;
                }
                for (n, e) in entries.iter().enumerate().skip(view.scroll).take(rows) {
                    let y = list.y + (n - view.scroll) as f32 * row_h;
                    let rr = Rect::new(lx + 2.0 * k, y + 1.0 * k, lw - 4.0 * k - room, row_h - 2.0 * k);
                    let picked = view.picked.as_deref() == Some(e.id.as_str());
                    row_paint(rr, picked);
                    let (words, ink) = state(c, &view.installed, e);
                    let sw = if words.is_empty() { 0.0 } else { measure(&words, small).width + 8.0 * k };
                    text_fit(e.title.get(lang()), rr.x + 6.0 * k, rr.y + 15.0 * g * k, rr.w - sw - 12.0 * k, size, if picked { GOLD } else { CREAM });
                    let mut by_line = format!("{} · {}", kind_label(e.kind), e.author);
                    if let Some(rating) = e.rating {
                        by_line = format!("{by_line} · +{} / -{}", rating.up, rating.down);
                    }
                    text_fit(&by_line, rr.x + 6.0 * k, rr.y + 29.0 * g * k, rr.w - 12.0 * k, small, Color::new(0.85, 0.80, 0.68, 1.0));
                    if !words.is_empty() {
                        chrome::shadow_right(&words, rr.x + rr.w - 6.0 * k, rr.y + 15.0 * g * k, small, ink);
                    }
                }
                if entries.is_empty() {
                    let none = if view.query.trim().is_empty() { tr("Nothing here yet.") } else { tr("Nothing matches the search.") };
                    chrome::shadow_text(none, lx + 12.0 * k, list.y + 26.0 * k, size, CREAM);
                }
            }
            if let Some(e) = view.picked.clone().and_then(|p| c.entry(&p).cloned()) {
                if !narrow || detail {
                    ensure_preview(view, &e);
                    // The pane: beside the list on the right; a phone held upright, the list's
                    // place, above the 40 units the job's word takes.
                    let pane = if narrow {
                        Rect::new(lx, ty, lw, by - 40.0 * k - ty)
                    } else {
                        let dx = lx + lw + 16.0 * k;
                        Rect::new(dx, inner.y + 10.0 * k, inner.x + inner.w - dx - 10.0 * k, by - 10.0 * k - (inner.y + 10.0 * k))
                    };
                    details(view, c, &e, pane, &lay);
                }
            }
        }
    }

    // The bottom row: the list's Back and Refresh, or a phone's details' «To the list».
    let mut close = closed || (key(KeyCode::Escape) && !search_esc);
    if close {
        // Closing the window hides the keyboard the search field may have.
        clear_focus();
    }
    if detail {
        if button(lx, by, back_w, bh, tr("To the list"), true) {
            view.detail = false;
        }
    } else {
        let (w_b, step) = if narrow { ((lw - gap) / 2.0, (lw - gap) / 2.0 + gap) } else { (back_w, 150.0 * k * g) };
        if button(lx, by, w_b, bh, tr("Back"), true) {
            close = true;
        }
        if button(lx + step, by, w_b, bh, tr("Refresh"), view.job.is_none()) {
            view.reload();
        }
    }
    // The job's word and where the hub is: beside the buttons, or over them on a phone held upright.
    let note_y = if narrow { by - 22.0 * k } else { by + 20.0 * k * g };
    let status_x = if narrow { lx } else { lx + 300.0 * k * g };
    let status_w = if narrow { lw } else { 260.0 * k * g };
    if view.job.is_some() && view.catalog.is_some() {
        chrome::shadow_text(tr("Downloading…"), status_x, note_y, size, GOLD);
    } else if let Some((s, bad)) = &view.note {
        text_fit(s, status_x, note_y, status_w, size, if *bad { RED_TEXT } else { GREEN_TEXT });
    }
    // The hub's place under the list: on the desktop only (a phone's space goes to the list; a
    // hub that can't be read says so in the list).
    if let (false, false, Some(h)) = (narrow, phone, &view.hub) {
        let from = trf!("Hub: {place}", place = h.source.label());
        text_fit(&from, lx, list.y + list.h + 18.0 * k, lw, small, Color::new(0.85, 0.80, 0.68, 0.8));
    }
    close
}

/// The picked entry in the details pane `pane`, scrolled by the wheel, a swipe, a drag or the
/// bar on its right, and clipped to the pane so no text reaches the bottom row. Install or
/// Remove sits at the bottom right of the window (`lay`), below the pane.
fn details(view: &mut HubView, c: &Catalog, e: &Entry, pane: Rect, lay: &Lay) {
    let (k, size, small, by, bh, bw) = (lay.k, lay.size, lay.small, lay.by, lay.bh, lay.bw);
    let s = k * lay.g;
    let dim = Color::new(0.85, 0.80, 0.68, 1.0);
    if view.detail_for.as_deref() != Some(e.id.as_str()) {
        view.detail_for = Some(e.id.clone());
        view.detail_scroll = 0.0;
        view.detail_h = 0.0;
    }
    // The bar and the text's width follow the content's height as the last frame drew it.
    let most = (view.detail_h - pane.h).max(0.0);
    let room = if most > 0.0 { bar_room(k, lay.phone) } else { 0.0 };
    let body = Rect::new(pane.x, pane.y, pane.w - room, pane.h);
    swipe_rows(body, 40.0 * k);
    if !input_blocked() && mouse_in(body.x, body.y, body.w, body.h) {
        view.detail_scroll -= wheel() * 40.0 * k;
    }
    let (scroll, _) = drag_frame(&mut view.pane_drag, body, view.detail_scroll, 1.0, most, drag_slop());
    view.detail_scroll = clamp_scroll(scroll, most);
    scrollbar_px(bar_rect(pane.x + pane.w, pane.y, pane.h, k, lay.phone), &mut view.detail_scroll, most, pane.h, 40.0 * k);
    let scroll = view.detail_scroll;

    // The content, from the pane's top less the scroll, drawn clipped to the pane.
    let r = Rect::new(pane.x, pane.y - scroll, pane.w - room, pane.h);
    let installed = view.installed.has(&e.id);
    let users = if installed { hub::needed_by(c, &view.installed, &e.id) } else { Vec::new() };
    let plan = (!installed).then(|| hub::plan_install(c, &view.installed, &e.id));
    let clip = Clip::new(pane);
    let mut y = r.y + 22.0 * s;
    if e.preview.is_some() {
        // The picture at the top of the pane: as wide as the pane, at most PREVIEW_H tall (and
        // 40% of the pane, a phone held sideways), its aspect kept. Room for it is kept while it
        // loads (a dark frame of 16:9 until then).
        let loaded = view.preview.as_ref().filter(|p| p.id == e.id).and_then(|p| p.texture.clone());
        let (w, h) = loaded.as_ref().map_or((16.0, 9.0), |t| (t.width(), t.height()));
        let max_h = (PREVIEW_H * k).min(pane.h * 0.4);
        let scale = (r.w / w).min(max_h / h);
        let (dw, dh) = (w * scale, h * scale);
        match loaded {
            Some(t) => draw_texture_ex(&t, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(dw, dh)), ..Default::default() }),
            None => draw_rectangle(r.x, r.y, dw, dh, Color::new(0.05, 0.04, 0.03, 0.8)),
        }
        y = r.y + dh + 16.0 * k;
    }
    text_fit(e.title.get(lang()), r.x, y, r.w, (18.0 * s).round(), GOLD);
    y += 22.0 * s;
    let mut by_line = if e.version.trim().is_empty() {
        trf!("{kind} by {author}", kind = kind_label(e.kind), author = e.author)
    } else {
        trf!("{kind} by {author}, version {version}", kind = kind_label(e.kind), author = e.author, version = e.version)
    };
    if !e.added.is_empty() {
        by_line = format!("{by_line} · {}", e.added);
    }
    text_fit(&by_line, r.x, y, r.w, small, dim);
    y += 18.0 * s;
    if let Some(rating) = e.rating {
        text_fit(&trf!("Rating on Discord: +{up} / -{down}", up = rating.up, down = rating.down), r.x, y, r.w, small, dim);
        y += 18.0 * s;
    }
    // The status is read whole: its lines wrap.
    let (words, ink) = status_line(e);
    for l in wrap_long(&words, r.w, small).iter() {
        chrome::shadow_text(l, r.x, y, small, ink);
        y += 18.0 * s;
    }
    if e.kind == Kind::Campaign {
        text_fit(&trf!("Chapters: {n}", n = e.map_count()), r.x, y, r.w, small, dim);
        y += 18.0 * s;
    }
    if e.kind != Kind::Mod {
        let langs: Vec<&str> = e
            .languages
            .iter()
            .map(|l| match l.as_str() {
                "ru" => tr("Russian"),
                "en" if e.machine_translated => tr("English (machine translation)"),
                "en" => tr("English"),
                other => other,
            })
            .collect();
        text_fit(&trf!("Languages: {list}", list = langs.join(", ")), r.x, y, r.w, small, dim);
        y += 18.0 * s;
    }
    // Why a mod can't be installed, in the text (the bottom row keeps its disabled Install).
    if let Some(Err(why)) = &plan {
        y += 4.0 * s;
        let why_text = match why {
            PlanError::Missing(m) => trf!("Can't be installed: the hub has no mod «{m}».", m),
            PlanError::Cycle(m) => trf!("Can't be installed: mods need each other («{m}»).", m),
        };
        for l in wrap_long(&why_text, r.w, small).iter() {
            chrome::shadow_text(l, r.x, y, small, RED_TEXT);
            y += 16.0 * s;
        }
    }

    // The mods it needs, each with its state.
    if !e.requires.is_empty() {
        y += 8.0 * s;
        chrome::shadow_text(tr("Needs:"), r.x, y, size, CREAM);
        y += 19.0 * s;
        for id in &e.requires {
            let (name, words, ink) = match c.entry(id) {
                None => (id.as_str(), tr("not in the hub"), RED_TEXT),
                Some(m) if view.installed.has(id) => (m.title.get(lang()), tr("installed"), GREEN_TEXT),
                Some(m) => (m.title.get(lang()), tr("will be installed too"), GOLD),
            };
            text_fit(&format!("• {name}: {words}"), r.x + 8.0 * k, y, r.w - 8.0 * k, small, ink);
            y += 19.0 * s;
        }
    }

    // The description, whole (scrolled, not cut): its long words wrap at the pane's width.
    y += 10.0 * s;
    for l in wrap_long(e.description.get(lang()), r.w, size).iter() {
        chrome::shadow_text(l, r.x, y, size, CREAM);
        y += 19.0 * s;
    }
    if e.kind == Kind::Mod {
        y += 8.0 * s;
        for l in wrap_long(tr("A mod is played from the new game window's Mod list, over the base game."), r.w, small).iter() {
            chrome::shadow_text(l, r.x, y, small, dim);
            y += 16.0 * s;
        }
    }
    if e.kind != Kind::Mod && installed {
        y += 8.0 * s;
        text_fit(tr("Its maps are in the new game's list."), r.x, y, r.w, small, GREEN_TEXT);
        y += 18.0 * s;
    }
    if !users.is_empty() {
        y += 8.0 * s;
        let names = view.titles(&users);
        text_fit(&trf!("Needed by: {names}", names), r.x, y, r.w, small, GOLD);
        y += 18.0 * s;
    }
    let content_h = y - r.y + 8.0 * s;
    drop(clip);
    view.detail_h = content_h;

    // Install or remove: the bottom row, below the pane.
    let busy = view.job.is_some();
    let bx = pane.x + pane.w - bw;
    if installed {
        if button(bx, by, bw, bh, tr("Remove"), !busy && users.is_empty()) {
            let id = e.id.clone();
            view.start(move |hub, c| Done::Removed(hub.remove(&c, &id).map(|_| id)));
        }
        return;
    }
    match plan {
        Some(Ok(plan)) => {
            let label = if plan.len() > 1 { trf!("Install (+{n})", n = plan.len() - 1) } else { tr("Install").into() };
            if button(bx, by, bw, bh, &label, !busy) {
                let id = e.id.clone();
                let lang_code = lang();
                view.start(move |hub, c| Done::Installed(hub.install(&c, &id, lang_code)));
            }
        }
        _ => {
            button(bx, by, bw, bh, tr("Install"), false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scroll stays between nothing to scroll and the most.
    #[test]
    fn a_scroll_is_clamped_to_its_range() {
        assert_eq!(clamp_scroll(-3.0, 10.0), 0.0);
        assert_eq!(clamp_scroll(12.0, 10.0), 10.0);
        assert_eq!(clamp_scroll(4.0, 10.0), 4.0);
        assert_eq!(clamp_scroll(4.0, -1.0), 0.0);
    }

    /// The content follows the finger: dragged up two rows, two rows further down the list; a
    /// drag past either end stops at it.
    #[test]
    fn a_drag_moves_the_content_with_the_finger() {
        assert_eq!(drag_to(0.0, 300.0, 208.0, 46.0, 10.0), 2.0);
        assert_eq!(drag_to(1.0, 100.0, 400.0, 46.0, 10.0), 0.0);
        assert_eq!(drag_to(9.0, 400.0, 0.0, 46.0, 10.0), 10.0);
        // Dragged down by a row from the top: nothing (it stays at the top).
        assert_eq!(drag_to(0.0, 100.0, 146.0, 46.0, 10.0), 0.0);
    }

    /// A press that moves 12 px or less is still a tap; more is a drag.
    #[test]
    fn a_press_moved_past_the_slop_is_a_drag_and_a_small_move_a_tap() {
        assert!(!moved_past((10.0, 10.0), (14.0, 13.0), 12.0));
        assert!(!moved_past((10.0, 10.0), (10.0, 22.0), 12.0));
        assert!(moved_past((10.0, 10.0), (10.0, 30.0), 12.0));
    }

    /// A click is the row under the press, and nothing on the bar beside the rows.
    #[test]
    fn a_click_picks_the_row_pressed_and_not_the_bar() {
        let area = Rect::new(0.0, 100.0, 200.0, 92.0);
        assert_eq!(row_index((10.0, 101.0), area, 46.0, 0, 5), Some(0));
        assert_eq!(row_index((10.0, 150.0), area, 46.0, 3, 5), Some(4));
        assert_eq!(row_index((10.0, 150.0), area, 46.0, 4, 5), None);
        assert_eq!(row_index((205.0, 150.0), area, 46.0, 0, 5), None);
        assert_eq!(row_index((10.0, 99.0), area, 46.0, 0, 5), None);
    }

    /// The search matches a title in either language or the author, whatever the case; a blank
    /// search matches everything; a word found nowhere matches nothing.
    #[test]
    fn the_search_matches_titles_and_author_ignoring_case() {
        let titles = ["Эволюция", "Evolution"];
        assert!(matches_text("", &titles, "Ivan"));
        assert!(matches_text("   ", &titles, "Ivan"), "a blank search is no search");
        assert!(matches_text("эвол", &titles, "Ivan"), "a Russian title, lower case");
        assert!(matches_text("EVOLU", &titles, "Ivan"), "an English title, upper case");
        assert!(matches_text("ivan", &titles, "Ivan Тихончук"), "the author");
        assert!(matches_text("  Иван  ", &titles, "Иван"), "the query is trimmed");
        assert!(!matches_text("ragnarok", &titles, "Ivan"));
    }

    /// A word wider than the box is cut where the next character would not fit; a line that
    /// fits is kept whole, and a character too wide for the box stands alone.
    #[test]
    fn an_over_long_word_is_broken_into_pieces_that_fit() {
        let fits = |t: &str| t.chars().count() <= 5;
        assert_eq!(break_long("https://x", fits), ["https", "://x"]);
        assert_eq!(break_long("ok", fits), ["ok"]);
        assert_eq!(break_long("ЖЖЖЖЖЖЖЖ", |t| t.chars().count() <= 3), ["ЖЖЖ", "ЖЖЖ", "ЖЖ"]);
        assert_eq!(break_long("ab", |_| false), ["a", "b"]);
    }
}
