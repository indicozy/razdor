//! "Maps and mods" (a Razdor extra): the hub's window over the main menu's
//! ruins (`razdor::hub`). Tabs above the list (all, maps, campaigns, mods); per row the
//! entry's kind, title, author and state (installed, needs mods, can't be installed). On the
//! right the picked entry: who made it, its languages (an English made by the import's
//! machine translation says so), the mods it needs with their state, its description, and
//! Install or Remove. Reading the catalog and installing run on a thread; the window only
//! shows what they say. The layout is ours: the original has no such window.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use razdor::hub::{self, Catalog, Entry, Hub, Installed, Kind, PlanError, Source, Status};
use razdor::i18n::{lang, tr};
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome::{self, CREAM, GOLD, RED_TEXT};
use super::widgets::*;

const GREEN_TEXT: Color = Color::new(0.55, 0.88, 0.45, 1.0);

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
        let mut v = HubView { hub, catalog: None, installed: Installed::default(), tab: 0, picked: None, detail: false, scroll: 0, job: None, note: None, changed: false, preview: None };
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

/// The tab's entries, best rated first (`hub::by_rating`).
fn shown(c: &Catalog, tab: usize) -> Vec<&Entry> {
    let want = [None, Some(Kind::Map), Some(Kind::Campaign), Some(Kind::Mod)][tab.min(3)];
    let mut list: Vec<&Entry> = c.entries.iter().filter(|e| want.is_none_or(|k| e.kind == k)).collect();
    list.sort_by(|a, b| hub::by_rating(a, b));
    list
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

/// A row of the list: the blue bar when picked, a warm wash under the mouse. True when clicked.
fn row(r: Rect, picked: bool) -> bool {
    let over = !input_blocked() && mouse_in(r.x, r.y, r.w, r.h);
    if picked {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.12, 0.2, 0.62, 0.75));
    } else if over {
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(1.0, 0.85, 0.5, 0.12));
    }
    over && clicked()
}

/// The window's parts at its scale: `k` px per unit (one dp on a phone), the text sizes at the
/// text size `g` (`display::button_grow`), the bottom row's top `by`, height `bh` and the width
/// of its Install or Remove `bw`. `narrow`: a phone held upright, one pane (the list, or the
/// picked entry's details in its place).
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
    narrow: bool,
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
    let head = if narrow { 2.0 * th + gap } else { th };
    let list_y = ty + head + 8.0 * k;
    let list_bottom = by - if narrow { 40.0 * k } else { 16.0 * k };
    let rows = ((list_bottom - list_y) / row_h).floor().max(1.0) as usize;
    let list = Rect::new(lx, list_y, lw, rows as f32 * row_h);
    let detail = narrow && view.detail && view.picked.is_some();
    let back_w = 140.0 * k * g;
    let bw = if narrow { lw - back_w - gap } else { 220.0 * k * g };
    let lay = Lay { k, g, size, small, by, bh, bw, phone, narrow };
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
            let entries = shown(c, view.tab);
            let max_scroll = entries.len().saturating_sub(rows);
            if !detail {
                if !input_blocked() && mouse_in(list.x, list.y, list.w, list.h) {
                    let wh = wheel();
                    if wh < 0.0 {
                        view.scroll = (view.scroll + 1).min(max_scroll);
                    } else if wh > 0.0 {
                        view.scroll = view.scroll.saturating_sub(1);
                    }
                }
                view.scroll = view.scroll.min(max_scroll);
                let bar_w = if max_scroll > 0 { 14.0 * k } else { 0.0 };
                scrollbar(Rect::new(lx + lw - 14.0 * k, list.y + 2.0 * k, 12.0 * k, list.h - 4.0 * k), &mut view.scroll, rows, entries.len());
                for (n, e) in entries.iter().enumerate().skip(view.scroll).take(rows) {
                    let y = list.y + (n - view.scroll) as f32 * row_h;
                    let rr = Rect::new(lx + 2.0 * k, y + 1.0 * k, lw - 4.0 * k - bar_w, row_h - 2.0 * k);
                    let picked = view.picked.as_deref() == Some(e.id.as_str());
                    if row(rr, picked) {
                        if !picked {
                            cue(Cue::Button);
                            view.picked = Some(e.id.clone());
                        }
                        // A phone shows the picked entry in place of the list (`narrow`).
                        view.detail = true;
                    }
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
                    chrome::shadow_text(tr("Nothing here yet."), lx + 12.0 * k, list.y + 26.0 * k, size, CREAM);
                }
            }
            if let Some(e) = view.picked.clone().and_then(|p| c.entry(&p).cloned()) {
                if !narrow || detail {
                    ensure_preview(view, &e);
                    let dr = if narrow {
                        Rect::new(lx, ty, lw, by - 16.0 * k - ty)
                    } else {
                        let dx = lx + lw + 16.0 * k;
                        Rect::new(dx, inner.y + 10.0 * k, inner.x + inner.w - dx - 10.0 * k, by - inner.y - 20.0 * k)
                    };
                    details(view, c, &e, dr, &lay);
                }
            }
        }
    }

    // The bottom row: the list's Back and Refresh, or a phone's details' «To the list».
    let mut close = closed || key(KeyCode::Escape);
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
    } else if let (true, Some(h)) = (narrow, &view.hub) {
        text_fit(&trf!("Hub: {place}", place = h.source.label()), lx, note_y, lw, small, Color::new(0.85, 0.80, 0.68, 0.8));
    }
    if let (false, Some(h)) = (narrow, &view.hub) {
        let from = trf!("Hub: {place}", place = h.source.label());
        text_fit(&from, lx, list.y + list.h + 18.0 * k, lw, small, Color::new(0.85, 0.80, 0.68, 0.8));
    }
    close
}

/// The picked entry, and Install or Remove at the bottom right (`lay`).
fn details(view: &mut HubView, c: &Catalog, e: &Entry, r: Rect, lay: &Lay) {
    let (k, size, small, by, bh, bw) = (lay.k, lay.size, lay.small, lay.by, lay.bh, lay.bw);
    let s = k * lay.g;
    let dim = Color::new(0.85, 0.80, 0.68, 1.0);
    let mut y = r.y + 22.0 * s;
    if e.preview.is_some() {
        // The picture at the top of the panel: as wide as the panel, at most PREVIEW_H tall (and
        // 40% of the panel, a phone held sideways), its aspect kept. Room for it is kept while it
        // loads (a dark frame of 16:9 until then).
        let loaded = view.preview.as_ref().filter(|p| p.id == e.id).and_then(|p| p.texture.clone());
        let (w, h) = loaded.as_ref().map_or((16.0, 9.0), |t| (t.width(), t.height()));
        let max_h = (PREVIEW_H * k).min(r.h * 0.4);
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
    let (words, ink) = status_line(e);
    if lay.phone {
        // A phone's status is read whole: its lines wrap (the desktop's line is fitted).
        for l in wrap(&words, r.w, small).iter() {
            chrome::shadow_text(l, r.x, y, small, ink);
            y += 18.0 * s;
        }
        y -= 18.0 * s;
    } else {
        text_fit(&words, r.x, y, r.w, small, ink);
    }
    if e.kind == Kind::Campaign {
        y += 18.0 * s;
        text_fit(&trf!("Chapters: {n}", n = e.map_count()), r.x, y, r.w, small, dim);
    }
    if e.kind != Kind::Mod {
        y += 18.0 * s;
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
    }

    // The mods it needs, each with its state.
    if !e.requires.is_empty() {
        y += 26.0 * s;
        chrome::shadow_text(tr("Needs:"), r.x, y, size, CREAM);
        for id in &e.requires {
            y += 19.0 * s;
            let (name, words, ink) = match c.entry(id) {
                None => (id.as_str(), tr("not in the hub"), RED_TEXT),
                Some(m) if view.installed.has(id) => (m.title.get(lang()), tr("installed"), GREEN_TEXT),
                Some(m) => (m.title.get(lang()), tr("will be installed too"), GOLD),
            };
            text_fit(&format!("• {name}: {words}"), r.x + 8.0 * k, y, r.w - 8.0 * k, small, ink);
        }
    }

    // The description: a phone's stops above the bottom row (the desktop's keeps its eight lines).
    y += 28.0 * s;
    let limit = match (lay.phone, lay.narrow) {
        (false, _) => f32::MAX,
        (true, true) => by - 44.0 * k,
        (true, false) => by - 8.0 * k,
    };
    for l in wrap(e.description.get(lang()), r.w, size).iter().take(if lay.phone { usize::MAX } else { 8 }) {
        if y > limit {
            break;
        }
        chrome::shadow_text(l, r.x, y, size, CREAM);
        y += 19.0 * s;
    }
    if e.kind == Kind::Mod {
        y += 8.0 * s;
        for l in wrap(tr("A mod is played from the new game window's Mod list, over the base game."), r.w, small).iter() {
            chrome::shadow_text(l, r.x, y, small, dim);
            y += 16.0 * s;
        }
    }
    if e.kind != Kind::Mod && view.installed.has(&e.id) {
        y += 8.0 * s;
        text_fit(tr("Its maps are in the new game's list."), r.x, y, r.w, small, GREEN_TEXT);
    }

    // Install or remove.
    let busy = view.job.is_some();
    let bx = r.x + r.w - bw;
    if view.installed.has(&e.id) {
        let users = hub::needed_by(c, &view.installed, &e.id);
        if !users.is_empty() {
            let names = view.titles(&users);
            text_fit(&trf!("Needed by: {names}", names), r.x, by - 10.0 * k, r.w, small, GOLD);
        }
        if button(bx, by, bw, bh, tr("Remove"), !busy && users.is_empty()) {
            let id = e.id.clone();
            view.start(move |hub, c| Done::Removed(hub.remove(&c, &id).map(|_| id)));
        }
        return;
    }
    match hub::plan_install(c, &view.installed, &e.id) {
        Ok(plan) => {
            let label = if plan.len() > 1 { trf!("Install with {n} mods", n = plan.len() - 1) } else { tr("Install").into() };
            if button(bx, by, bw, bh, &label, !busy) {
                let id = e.id.clone();
                let lang_code = lang();
                view.start(move |hub, c| Done::Installed(hub.install(&c, &id, lang_code)));
            }
        }
        Err(why) => {
            let why_text = match why {
                PlanError::Missing(m) => trf!("Can't be installed: the hub has no mod «{m}».", m),
                PlanError::Cycle(m) => trf!("Can't be installed: mods need each other («{m}»).", m),
            };
            for (i, l) in wrap(&why_text, r.w, small).iter().enumerate() {
                chrome::shadow_text(l, r.x, by - 26.0 * s + i as f32 * 16.0 * s, small, RED_TEXT);
            }
            button(bx, by, bw, bh, tr("Install"), false);
        }
    }
}
