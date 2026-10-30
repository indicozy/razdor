//! "Maps and mods" (a Razdor extra, a mock for now): the hub's window over the main menu's
//! ruins (`razdor::hub`). Tabs above the list (all, maps, campaigns, mods); per row the
//! entry's kind, title, author and state (installed, needs mods, can't be installed). On the
//! right the picked entry: who made it, its languages (an English made by the import's
//! machine translation says so), the mods it needs with their state, its description, and
//! Install or Remove. Reading the catalog and installing run on a thread; the window only
//! shows what they say. The layout is ours: the original has no such window.

use std::sync::mpsc::{channel, Receiver};

use macroquad::prelude::*;

use razdor::hub::{self, Catalog, Entry, Hub, Installed, Kind, PlanError, Source, Status};
use razdor::i18n::{lang, tr};
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome::{self, CREAM, GOLD, RED_TEXT};
use super::widgets::*;

const GREEN_TEXT: Color = Color::new(0.55, 0.88, 0.45, 1.0);

/// Rows of the list.
const ROWS: usize = 12;

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
    scroll: usize,
    job: Option<Receiver<Done>>,
    /// The last job's word: the text and whether it went wrong.
    note: Option<(String, bool)>,
    /// Something was installed or removed while the window was open (the new-game list is
    /// read again on the way out).
    pub changed: bool,
}

impl HubView {
    pub fn new() -> HubView {
        let hub = hub::local_dir().map(|local| Hub { source: Source::from_env(), local });
        let mut v = HubView { hub, catalog: None, installed: Installed::default(), tab: 0, picked: None, scroll: 0, job: None, note: None, changed: false };
        v.reload();
        v
    }

    /// A snapshot's setup (`ui::snapshot`): the tab and the picked entry.
    pub fn stage(&mut self, tab: usize, pick: Option<&str>) {
        self.tab = tab.min(3);
        self.picked = pick.map(Into::into);
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

/// The tab's entries.
fn shown(c: &Catalog, tab: usize) -> Vec<&Entry> {
    let want = [None, Some(Kind::Map), Some(Kind::Campaign), Some(Kind::Mod)][tab.min(3)];
    c.entries.iter().filter(|e| want.is_none_or(|k| e.kind == k)).collect()
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

/// Draws the window; true when it is closed.
pub fn frame(view: &mut HubView) -> bool {
    view.poll();
    super::main_menu::backdrop();
    let k = chrome::k();
    let (w, h) = (900.0 * k, 560.0 * k);
    let r = Rect::new((screen_width() - w) / 2.0, (150.0 * k).min(screen_height() - h).max(0.0), w, h);
    let (inner, closed) = chrome::window(r, tr("Maps and mods"), chrome::Skin::Marble, true);
    let size = (14.0 * k).round();
    let small = (12.0 * k).round();
    let row_h = 34.0 * k;
    let lx = inner.x + 10.0 * k;
    let lw = 380.0 * k;
    let by = inner.y + inner.h - 40.0 * k;
    let bh = 30.0 * k;

    let tabs = [tr("All"), tr("Maps"), tr("Campaigns"), tr("Mods")];
    let tw = lw / tabs.len() as f32;
    for (i, t) in tabs.iter().enumerate() {
        if toggle_button(lx + i as f32 * tw, inner.y + 10.0 * k, tw - 4.0 * k, 26.0 * k, t, view.tab == i) && view.tab != i {
            view.tab = i;
            view.scroll = 0;
        }
    }
    let list = Rect::new(lx, inner.y + 44.0 * k, lw, ROWS as f32 * row_h);
    chrome::text_box(list);

    let catalog = view.catalog.clone();
    match &catalog {
        None => chrome::shadow_text(tr("Reading the hub…"), lx + 12.0 * k, list.y + 26.0 * k, size, CREAM),
        Some(Err(e)) => {
            for (i, l) in wrap(&trf!("The hub can't be read: {e}", e), lw - 24.0 * k, size).iter().enumerate() {
                chrome::shadow_text(l, lx + 12.0 * k, list.y + 26.0 * k + i as f32 * 20.0 * k, size, RED_TEXT);
            }
        }
        Some(Ok(c)) => {
            let entries = shown(c, view.tab);
            let max_scroll = entries.len().saturating_sub(ROWS);
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
            scrollbar(Rect::new(lx + lw - 14.0 * k, list.y + 2.0 * k, 12.0 * k, list.h - 4.0 * k), &mut view.scroll, ROWS, entries.len());
            for (n, e) in entries.iter().enumerate().skip(view.scroll).take(ROWS) {
                let y = list.y + (n - view.scroll) as f32 * row_h;
                let rr = Rect::new(lx + 2.0 * k, y + 1.0 * k, lw - 4.0 * k - bar_w, row_h - 2.0 * k);
                let picked = view.picked.as_deref() == Some(e.id.as_str());
                if row(rr, picked) && !picked {
                    cue(Cue::Button);
                    view.picked = Some(e.id.clone());
                }
                let (words, ink) = state(c, &view.installed, e);
                let sw = if words.is_empty() { 0.0 } else { measure(&words, small).width + 8.0 * k };
                text_fit(e.title.get(lang()), rr.x + 6.0 * k, rr.y + 15.0 * k, rr.w - sw - 12.0 * k, size, if picked { GOLD } else { CREAM });
                let by_line = format!("{} · {}", kind_label(e.kind), e.author);
                text_fit(&by_line, rr.x + 6.0 * k, rr.y + 29.0 * k, rr.w - 12.0 * k, small, Color::new(0.85, 0.80, 0.68, 1.0));
                if !words.is_empty() {
                    chrome::shadow_right(&words, rr.x + rr.w - 6.0 * k, rr.y + 15.0 * k, small, ink);
                }
            }
            if entries.is_empty() {
                chrome::shadow_text(tr("Nothing here yet."), lx + 12.0 * k, list.y + 26.0 * k, size, CREAM);
            }
            let picked = view.picked.clone().and_then(|p| c.entry(&p).cloned());
            if let Some(e) = picked {
                let dx = lx + lw + 16.0 * k;
                details(view, c, &e, Rect::new(dx, inner.y + 10.0 * k, inner.x + inner.w - dx - 10.0 * k, by - inner.y - 20.0 * k), size, small, by, bh);
            }
        }
    }

    // The way out, the job's word and where the hub is.
    let back = button(lx, by, 140.0 * k, bh, tr("Back"), true);
    if button(lx + 150.0 * k, by, 140.0 * k, bh, tr("Refresh"), view.job.is_none()) {
        view.reload();
    }
    let status_x = lx + 300.0 * k;
    if view.job.is_some() && view.catalog.is_some() {
        chrome::shadow_text(tr("Working…"), status_x, by + 20.0 * k, size, GOLD);
    } else if let Some((s, bad)) = &view.note {
        text_fit(s, status_x, by + 20.0 * k, 260.0 * k, size, if *bad { RED_TEXT } else { GREEN_TEXT });
    }
    if let Some(h) = &view.hub {
        let from = trf!("Hub: {place}", place = h.source.label());
        text_fit(&from, lx, list.y + list.h + 18.0 * k, lw, small, Color::new(0.85, 0.80, 0.68, 0.8));
    }
    back || closed || key(KeyCode::Escape)
}

/// The picked entry, and Install or Remove at the bottom right.
#[allow(clippy::too_many_arguments)]
fn details(view: &mut HubView, c: &Catalog, e: &Entry, r: Rect, size: f32, small: f32, by: f32, bh: f32) {
    let k = chrome::k();
    let dim = Color::new(0.85, 0.80, 0.68, 1.0);
    let mut y = r.y + 22.0 * k;
    text_fit(e.title.get(lang()), r.x, y, r.w, (18.0 * k).round(), GOLD);
    y += 22.0 * k;
    let mut by_line = trf!("{kind} by {author}, version {version}", kind = kind_label(e.kind), author = e.author, version = e.version);
    if !e.added.is_empty() {
        by_line = format!("{by_line} · {}", e.added);
    }
    text_fit(&by_line, r.x, y, r.w, small, dim);
    y += 18.0 * k;
    let (words, ink) = status_line(e);
    text_fit(&words, r.x, y, r.w, small, ink);
    if e.kind == Kind::Campaign {
        y += 18.0 * k;
        text_fit(&trf!("Chapters: {n}", n = e.map_count()), r.x, y, r.w, small, dim);
    }
    if e.kind != Kind::Mod {
        y += 18.0 * k;
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
        y += 26.0 * k;
        chrome::shadow_text(tr("Needs:"), r.x, y, size, CREAM);
        for id in &e.requires {
            y += 19.0 * k;
            let (name, words, ink) = match c.entry(id) {
                None => (id.as_str(), tr("not in the hub"), RED_TEXT),
                Some(m) if view.installed.has(id) => (m.title.get(lang()), tr("installed"), GREEN_TEXT),
                Some(m) => (m.title.get(lang()), tr("will be installed too"), GOLD),
            };
            text_fit(&format!("• {name}: {words}"), r.x + 8.0 * k, y, r.w - 8.0 * k, small, ink);
        }
    }

    // The description.
    y += 28.0 * k;
    for l in wrap(e.description.get(lang()), r.w, size).iter().take(8) {
        chrome::shadow_text(l, r.x, y, size, CREAM);
        y += 19.0 * k;
    }
    if e.kind == Kind::Mod {
        y += 8.0 * k;
        for l in wrap(tr("A mod is played from the new game window's Mod list, over the base game."), r.w, small).iter() {
            chrome::shadow_text(l, r.x, y, small, dim);
            y += 16.0 * k;
        }
    }
    if e.kind != Kind::Mod && view.installed.has(&e.id) {
        y += 8.0 * k;
        text_fit(tr("Its maps are in the new game's list."), r.x, y, r.w, small, GREEN_TEXT);
    }

    // Install or remove.
    let busy = view.job.is_some();
    let bw = 220.0 * k;
    let bx = r.x + r.w - bw;
    let lang_code = lang();
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
                view.start(move |hub, c| Done::Installed(hub.install(&c, &id, lang_code)));
            }
        }
        Err(why) => {
            let s = match why {
                PlanError::Missing(m) => trf!("Can't be installed: the hub has no mod «{m}».", m),
                PlanError::Cycle(m) => trf!("Can't be installed: mods need each other («{m}»).", m),
            };
            for (i, l) in wrap(&s, r.w, small).iter().enumerate() {
                chrome::shadow_text(l, r.x, by - 26.0 * k + i as f32 * 16.0 * k, small, RED_TEXT);
            }
            button(bx, by, bw, bh, tr("Install"), false);
        }
    }
}
