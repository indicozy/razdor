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

use razdor::hub::{self, Catalog, Entry, FileRef, Hub, ImportError, Installed, Kind, PlanError, Source, Status};
use razdor::i18n::{self, lang, tr, Lang};
use razdor::platform;
use razdor::trf;

use super::audio::{cue, Cue};
use super::chrome::{self, CREAM, GOLD, RED_TEXT};
use super::hub_pack::{self, Answer, Notice, Pick, ReportAnswer, ReportForm};
use super::input::{is_mouse_button_down, is_mouse_button_pressed};
use super::widgets::*;

const GREEN_TEXT: Color = Color::new(0.55, 0.88, 0.45, 1.0);

/// The search field's key (`widgets::text_field`).
const SEARCH_KEY: &str = "hub-search";

/// What a job on the thread came back with.
enum Done {
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
    /// The catalog fetched in the background (`reload`): it replaces the list on the screen when it
    /// differs (`landed`); the thread writes it to the cache.
    fetching: Option<Receiver<Result<Catalog, String>>>,
    /// The fetch failed while a catalog was on the screen (the saved one): the window says so.
    offline: bool,
    /// The last job's word: the text and whether it went wrong.
    note: Option<(String, bool)>,
    /// Something was installed or removed while the window was open (the new-game list is
    /// read again on the way out).
    pub changed: bool,
    /// The picked entry's picture (`ensure_preview`).
    preview: Option<Preview>,
    /// «Импорт пакета…»: the file picker while it is open, and the import on its thread.
    picker: Option<Pick>,
    import: Option<Receiver<Result<String, ImportError>>>,
    /// A notice over the window: a refused import, or a picker that cannot be opened.
    notice: Option<Notice>,
    /// The report window of a package's card, while it is open.
    report: Option<ReportForm>,
    /// A package to play: the window closes and the game opens its map (`App::open_pack`).
    pub play: Option<PathBuf>,
    /// The packages whose update mark is on (`Hub::has_update`), read with `installed`.
    marks: Vec<String>,
}

/// The picture of an entry: its texture once it is in the cache, and the download on its way
/// when it is not.
struct Preview {
    /// `preview_key`: the entry and its picture, so a refreshed catalog's new picture is loaded.
    key: String,
    texture: Option<Texture2D>,
    job: Option<Receiver<Result<PathBuf, String>>>,
}

/// A press on a scrolled area, held until the button comes up: where it began, the scroll it
/// began at, and whether the pointer has moved past the slop since (then it is a drag).
#[derive(Clone, Copy, Debug)]
pub(super) struct Drag {
    at: (f32, f32),
    start: f32,
    moved: bool,
}

/// The most a preview is drawn tall (window units); a phone held upright draws it up to 45% of
/// the pane instead, the full width.
const PREVIEW_H: f32 = 150.0;

/// The picture shown for an entry: the map's own picture when the catalog has one, else the
/// post's image (`preview`).
fn shown_picture(e: &Entry) -> Option<&FileRef> {
    e.picture.as_ref().or(e.preview.as_ref())
}

/// Keeps the picked entry's picture: loaded from the cache, or downloaded on a thread (once,
/// into the cache) when it is not there yet. A download that fails leaves no picture.
fn ensure_preview(view: &mut HubView, e: &Entry) {
    let key = preview_key(e);
    if let Some(p) = view.preview.as_mut().filter(|p| p.key == key) {
        if let Some(done) = p.job.as_ref().and_then(|rx| rx.try_recv().ok()) {
            p.job = None;
            match done {
                Ok(path) => p.texture = load_texture(&path),
                Err(why) => razdor::diag!("hub: the picture of {} is not shown: {why}", e.id),
            }
        }
        return;
    }
    let mut p = Preview { key, texture: None, job: None };
    if let (Some(f), Some(hub)) = (shown_picture(e), view.hub.clone()) {
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

/// The key of an entry's picture: the entry's id and the picture's SHA-256.
fn preview_key(e: &Entry) -> String {
    shown_picture(e).map_or_else(|| e.id.clone(), |f| format!("{}:{}", e.id, f.sha256))
}

/// A cached PNG as a texture (macroquad decodes PNG and TGA; `None` when it does not read).
fn load_texture(path: &Path) -> Option<Texture2D> {
    let bytes = std::fs::read(path).ok()?;
    let img = Image::from_file_with_format(&bytes, None).ok()?;
    Some(Texture2D::from_image(&img))
}

impl HubView {
    pub fn new() -> HubView {
        // A snapshot run never writes the trusted list: its installs stay in the snapshot's own hub folder.
        let trust_dir = if std::env::var_os("RAZDOR_SNAPSHOT").is_some() { None } else { platform::app_dir() };
        let hub = hub::local_dir().map(|local| Hub { source: Source::from_env(), local, trust_dir });
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
            fetching: None,
            offline: false,
            note: None,
            changed: false,
            preview: None,
            picker: None,
            import: None,
            notice: None,
            report: None,
            play: None,
            marks: Vec::new(),
        };
        v.reload();
        v
    }

    /// A snapshot's setup: a notice over the window (`ui::snapshot`).
    pub fn stage_notice(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    /// A snapshot's setup: the report window open.
    pub fn stage_report(&mut self, form: ReportForm) {
        self.report = Some(form);
    }


    /// A snapshot's setup (`ui::snapshot`): the tab and the picked entry.
    /// Types `q` into the search field (the off-screen scene `hub:<tab>:<id>:<search>`).
    pub fn search_for_snapshot(&mut self, q: &str) {
        self.query = q.into();
    }

    pub fn stage(&mut self, tab: usize, pick: Option<&str>) {
        self.tab = tab.min(3);
        self.picked = pick.map(Into::into);
        if let Some(Ok(c)) = &self.catalog {
            self.picked = keep_pick(self.picked.as_deref(), c);
        }
        self.detail = pick.is_some();
    }

    /// Shows the cached catalog at once (`show_cache`) and reads the current one on a thread
    /// (`fetching`); a catalog already on the screen stays until the new one lands (`landed`).
    /// Also reads what is installed.
    fn reload(&mut self) {
        let Some(hub) = self.hub.clone() else {
            self.catalog = Some(Err(tr("No folder for the hub.").into()));
            return;
        };
        self.read_installed(&hub);
        self.offline = false;
        self.show_cache(&hub);
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let _ = tx.send(hub.fetch_catalog());
        });
        self.fetching = Some(rx);
    }

    /// Reads the installed list and which of its packages carry the update mark (`Hub::has_update`),
    /// so the rows and the card need no file reads of their own.
    fn read_installed(&mut self, hub: &Hub) {
        self.installed = hub.installed();
        self.marks = self.installed.entries.keys().filter(|id| hub.has_update(id)).cloned().collect();
    }

    /// Puts the cached catalog on the screen when there is none yet. A corrupt cache, or one of
    /// another address, is no catalog: the window reads the hub as it does without one.
    fn show_cache(&mut self, hub: &Hub) {
        if matches!(self.catalog, Some(Ok(_))) {
            return;
        }
        match hub.cached_catalog() {
            Some(c) => {
                self.picked = keep_pick(self.picked.as_deref(), &c);
                self.catalog = Some(Ok(c));
            }
            None => self.catalog = None,
        }
    }

    /// A fetch of the catalog came back. The same catalog as the one shown changes nothing; a
    /// different one replaces it, keeping the pick by id. A failed fetch keeps a catalog already on
    /// the screen (the cache or an earlier fetch) and sets `offline`; with none, the error is the list.
    fn landed(&mut self, fetched: Result<Catalog, String>) {
        match fetched {
            Ok(c) => {
                self.offline = false;
                if matches!(&self.catalog, Some(Ok(shown)) if *shown == c) {
                    return;
                }
                self.picked = keep_pick(self.picked.as_deref(), &c);
                self.catalog = Some(Ok(c));
            }
            Err(why) if matches!(self.catalog, Some(Ok(_))) => {
                razdor::diag!("hub: the catalog is not fetched, the saved list stays: {why}");
                self.offline = true;
            }
            Err(why) => self.catalog = Some(Err(why)),
        }
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

    /// Takes a finished job's result, and a finished catalog fetch.
    fn poll(&mut self) {
        if let Some(fetched) = self.fetching.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.fetching = None;
            self.landed(fetched);
        }
        // «Импорт пакета…»: the picker's answer starts the import; the import's answer is a notice
        // when it is refused, else the package is listed and picked.
        match hub_pack::poll_pick(&mut self.picker) {
            Some(Answer::Picked(path, cached)) => self.start_import(path, cached),
            Some(Answer::Failed(why)) => self.notice = Some(Notice::new(tr("Import a package…"), vec![why])),
            Some(Answer::Cancelled) | None => {}
        }
        if let Some(done) = self.import.as_ref().and_then(|rx| rx.try_recv().ok()) {
            self.import = None;
            match done {
                Ok(id) => {
                    cue(Cue::Button);
                    if let Some(h) = self.hub.clone() {
                        self.read_installed(&h);
                    }
                    self.changed = true;
                    let names = self.installed.entries.get(&id).map_or_else(|| id.clone(), |r| r.title.get(lang()).to_string());
                    self.note = Some((trf!("Imported: {names}", names), false));
                    self.picked = Some(id);
                    self.detail_for = None;
                }
                Err(e) => self.notice = Some(hub_pack::import_notice(&e)),
            }
        }
        let Some(done) = self.job.as_ref().and_then(|rx| rx.try_recv().ok()) else { return };
        self.job = None;
        let refresh = |v: &mut HubView| {
            if let Some(h) = v.hub.clone() {
                v.read_installed(&h);
            }
            v.changed = true;
        };
        match done {
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

    /// Imports the picked file on a thread (`Hub::import_package`); a cached copy (Android's) goes
    /// once it is read.
    fn start_import(&mut self, path: PathBuf, cached: bool) {
        let Some(hub) = self.hub.clone() else { return };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let done = hub.import_package(&path);
            if cached {
                let _ = std::fs::remove_file(&path);
            }
            let _ = tx.send(done);
        });
        self.import = Some(rx);
    }

    fn titles(&self, ids: &[String]) -> String {
        let c = self.catalog.as_ref().and_then(|c| c.as_ref().ok());
        ids.iter().map(|id| c.and_then(|c| c.entry(id)).map_or(id.as_str(), |e| e.title.get(lang()))).collect::<Vec<_>>().join(", ")
    }
}

/// A snapshot's setup (`ui::snapshot`, `hub-versions`): the example package imported at 0.1.0 and then
/// at 0.2.0, the first marked as played, so its card shows the version picker and the «New version»
/// mark. The mock catalog offers 0.3.0 to install. It runs before the window is made, like
/// [`install_for_snapshot`].
pub fn versions_for_snapshot() {
    let Some(local) = hub::local_dir() else { return };
    let dir = local.join("snapshot-versions");
    let hub = Hub { source: Source::Mock, local: local.clone(), trust_dir: None };
    for (n, version) in ["0.1.0", "0.2.0"].into_iter().enumerate() {
        let Ok(bytes) = hub::mock::package_zip(version) else { return };
        let file = dir.join(format!("hello-story-{version}.rzpack"));
        if std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&file, &bytes)).is_err() {
            return;
        }
        if let Err(e) = hub.import_package(&file) {
            razdor::diag!("hub: the snapshot's version {version} is not imported: {e:?}");
            return;
        }
        if n == 0 {
            let _ = hub.mark_played("hello-story", version);
        }
    }
}

/// A snapshot's setup (`ui::snapshot`): the entry installed first, from the mock hub of the
/// snapshot's hub folder. It runs before the window is made, so the mock hub is built here, once,
/// and not by the window's catalog thread at the same time.
pub fn install_for_snapshot(id: &str) {
    let Some(local) = hub::local_dir() else { return };
    let hub = Hub { source: Source::from_env(), local, trust_dir: None };
    if let Ok(c) = hub.catalog() {
        let _ = hub.install(&c, id, lang());
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
        Kind::Package => tr("Package"),
    }
}

/// The entry to pick in `c` after its catalog changed: the one picked before when it is still
/// there, else the first.
pub fn keep_pick(picked: Option<&str>, c: &Catalog) -> Option<String> {
    match picked {
        Some(p) if c.entry(p).is_some() => Some(p.to_string()),
        _ => c.entries.first().map(|e| e.id.clone()),
    }
}

/// The tab's entries that match the search, best rated first (`hub::by_rating`).
fn shown<'a>(c: &'a Catalog, tab: usize, query: &str, installed: &Installed) -> Vec<&'a Entry> {
    let want = [None, Some(Kind::Map), Some(Kind::Campaign), Some(Kind::Mod)][tab.min(3)];
    // An unlisted entry is hidden, unless the player has it (`hub::listed`).
    let mut list: Vec<&Entry> = c
        .entries
        .iter()
        .filter(|e| want.is_none_or(|k| e.kind == k))
        .filter(|e| hub::listed(e, installed))
        .filter(|e| matches(c, e, query))
        .collect();
    list.sort_by(|a, b| hub::by_rating(a, b));
    list
}

/// Whether an entry matches the search `query`: its Russian or English title, its author, or the
/// title or id of a mod it requires (so «evolu» finds every map made for Evolution) contains it,
/// ignoring case. An empty query (or only spaces) matches everything.
pub fn matches(c: &Catalog, e: &Entry, query: &str) -> bool {
    let mut titles: Vec<&str> = vec![e.title.ru.as_str(), e.title.en.as_str()];
    for id in &e.requires {
        titles.push(id.as_str());
        if let Some(m) = c.entry(id) {
            titles.push(m.title.ru.as_str());
            titles.push(m.title.en.as_str());
        }
    }
    matches_text(query, &titles, &e.author)
}

/// [`matches`] on the texts: `titles` are the entry's titles in each language, `author` its author.
pub fn matches_text(query: &str, titles: &[&str], author: &str) -> bool {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return true;
    }
    titles.iter().any(|t| t.to_lowercase().contains(&q)) || author.to_lowercase().contains(&q)
}

/// The words that name the mods an entry still needs: «Нужен мод: A» for one, «Нужны моды: A, B»
/// for several (`names` in install order, the catalog's own titles).
pub fn needs_words(lang: Lang, names: &[&str]) -> String {
    let list = names.join(", ");
    let template = if names.len() == 1 { i18n::n_("Needs mod: {names}") } else { i18n::n_("Needs mods: {names}") };
    i18n::fill(i18n::tr_in(lang, template), &[("names", &list as &dyn std::fmt::Display)])
}

/// «Для мода: A» for one mod, «Для модов: A, B» for several (`names` in install order, the
/// catalog's own titles): the mod an entry is made for, shown once its mods are installed.
pub fn for_mods_words(lang: Lang, names: &[&str]) -> String {
    let list = names.join(", ");
    let template = if names.len() == 1 { i18n::n_("For mod: {names}") } else { i18n::n_("For mods: {names}") };
    i18n::fill(i18n::tr_in(lang, template), &[("names", &list as &dyn std::fmt::Display)])
}

/// The words of a row whose entry needs mods: the mods still to install by name (`missing`) while
/// there are some, else the mods it is made for (`names`, all its mods). None when it needs no mod.
pub fn mod_row_words(lang: Lang, missing: &[&str], names: &[&str]) -> Option<String> {
    if names.is_empty() {
        None
    } else if !missing.is_empty() {
        Some(needs_words(lang, missing))
    } else {
        Some(for_mods_words(lang, names))
    }
}

/// A row's state: its words and ink. An entry that needs mods always says which (its count
/// while some are missing, else the mod it is made for), so it still tells after the install. An
/// installed package with a newer version to play shows the «New version» mark (`update`) in the
/// accent ink instead.
fn state(c: &Catalog, inst: &Installed, e: &Entry, update: bool) -> (String, Color) {
    if update && inst.has(&e.id) {
        return (tr("New version").into(), GOLD);
    }
    if e.requires.is_empty() {
        return if inst.has(&e.id) { (tr("Installed").into(), GREEN_TEXT) } else { (String::new(), CREAM) };
    }
    let title = |id: &str| c.entry(id).map_or_else(|| id.to_string(), |m| m.title.get(lang()).to_string());
    let names: Vec<String> = e.requires.iter().map(|id| title(id)).collect();
    // The mods still to install, by their titles: the plan's, which ends with the entry itself (none
    // when the entry is installed).
    let missing: Vec<String> = if inst.has(&e.id) {
        Vec::new()
    } else {
        match hub::plan_install(c, inst, &e.id) {
            Err(_) => return (tr("Can't be installed").into(), RED_TEXT),
            Ok(plan) => plan.split_last().map_or_else(Vec::new, |(_, rest)| rest.iter().map(|id| title(id)).collect()),
        }
    };
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let missing_refs: Vec<&str> = missing.iter().map(String::as_str).collect();
    let words = mod_row_words(lang(), &missing_refs, &refs).unwrap_or_default();
    (words, if missing.is_empty() { CREAM } else { GOLD })
}

/// The controls a package's card shows, from its installed versions (newest first), its update mark
/// (`Hub::has_update`) and the catalog's version the player lacks (`Hub::catalog_update`).
#[derive(Debug, PartialEq, Eq)]
pub struct CardControls {
    /// The version picker: more than one version is installed.
    pub picker: bool,
    /// The «New version» mark.
    pub mark: bool,
    /// The «Install X.Y.Z» button: the version to install.
    pub install: Option<String>,
}

/// The card's controls for `versions` (installed, newest first), `update` (the mark) and `catalog`
/// (the catalog version to install). A mark needs an installed version.
pub fn card_controls(versions: &[String], update: bool, catalog: Option<&str>) -> CardControls {
    CardControls { picker: versions.len() > 1, mark: update && !versions.is_empty(), install: catalog.map(str::to_string) }
}

/// The card's version picker in `field` (a field as tall as a button): it shows `versions[current]`
/// with its arrow, and a tap opens the list of the versions, newest first (`open_list`). Returns the
/// index of the version picked since the last frame.
fn version_picker(field: Rect, size: f32, k: f32, versions: &[String], current: usize, enabled: bool) -> Option<usize> {
    let picked = take_pick(VERSION_KEY).and_then(|v| usize::try_from(v).ok());
    // The list opens on the release of a click (`button_state`), as every drop-down does: opened on
    // the press, `draw_popup` would close it at once, the press being outside the list it opens.
    let (hover, _, pressed) = button_state(field, enabled);
    draw_rectangle(field.x, field.y, field.w, field.h, FIELD_BG);
    draw_rectangle_lines(field.x, field.y, field.w, field.h, 2.0, if hover { INK } else { DIM });
    let label = versions.get(current).map_or("", String::as_str);
    text_fit(label, field.x + 8.0 * k, field.y + field.h / 2.0 + size * 0.35, field.w - 34.0 * k, size, INK);
    let (cx, cy) = (field.x + field.w - 14.0 * k, field.y + field.h / 2.0);
    draw_triangle(vec2(cx - 5.0 * k, cy - 3.0 * k), vec2(cx + 5.0 * k, cy - 3.0 * k), vec2(cx, cy + 3.0 * k), DIM);
    if pressed {
        let options: Vec<(i64, String)> = versions.iter().enumerate().map(|(i, v)| (i as i64, v.clone())).collect();
        open_list(VERSION_KEY, field, current as i64, &options, k);
    }
    picked.filter(|&i| i != current && i < versions.len())
}

/// The key of the card's version picker (its drop-down list).
const VERSION_KEY: &str = "hub-version";

/// Starts the install of package `id`'s catalog version (`Hub::install`, which keeps the installed ones).
fn start_install(view: &mut HubView, id: &str) {
    let id = id.to_string();
    let lang_code = lang();
    view.start(move |hub, c| Done::Installed(hub.install(&c, &id, lang_code)));
}

/// The entry's state as its author or the hub's checks say it: the words and their ink.
fn status_line(e: &Entry) -> (String, Color) {
    let note = e.status_note.get(lang());
    match e.status {
        Status::Full => (tr("Works fully").into(), GREEN_TEXT),
        Status::Partial => (trf!("Works partly: {note}", note = note), GOLD),
        Status::Broken => (trf!("Does not start: {note}", note = note), RED_TEXT),
        Status::Unknown => (tr("Not checked yet").into(), Color::new(0.85, 0.80, 0.68, 1.0)),
        Status::Unlisted => (tr("Taken off the list").into(), Color::new(0.85, 0.80, 0.68, 1.0)),
    }
}

/// A row's wash: the blue bar when picked, a warm wash under the mouse. The click is
/// `drag_frame`'s, so a drag over the row picks nothing.
pub(super) fn row_paint(r: Rect, picked: bool) {
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
    /// A phone held upright: the details pane takes the list's place, so its picture and text are drawn larger.
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
pub(super) fn wrap_long(s: &str, width: f32, size: f32) -> Vec<String> {
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
    let (bh, th, mut row_h, gap) = if phone { (44.0 * k * g, 44.0 * k * g, 46.0 * k * g, 8.0 * k) } else { (30.0 * k * g, 26.0 * k * g, 34.0 * k * g, 4.0 * k) };
    // A phone held upright: two readable lines per row (title and the line under it).
    if narrow {
        row_h = 58.0 * k * g;
    }
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
    // The bottom row beside the list (not held upright): Back, Refresh, the import button, the search
    // field, then Install. The import button takes its label's width (190 units) or what the row has left
    // before Install; the search field is left out when less than 80 units would be left for it.
    let step_d = 150.0 * k * g;
    let bx_d = inner.x + inner.w - 10.0 * k - bw;
    let import_w = if narrow { 0.0 } else { (190.0 * k * g).min(bx_d - gap - (lx + 2.0 * step_d)).max(60.0 * k * g) };
    let search_room = narrow || bx_d - gap - (lx + 2.0 * step_d + import_w + gap) >= 80.0 * k * g;
    // A notice or the report window is over the window: the buttons under it take no click.
    let modal = view.notice.is_some() || view.report.is_some();
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
            let clicked = (if phone { tab_button(x, y, w, th, t, view.tab == i) } else { toggle_button(x, y, w, th, t, view.tab == i) }) && !modal;
            if clicked && view.tab != i {
                view.tab = i;
                view.scroll = 0;
            }
        }
    }

    // The search field (a tap in it types; the phone's keyboard comes only then, as the
    // dropdowns' filter does) and its clear button: above the list in portrait, in the bottom row
    // between «Обновить» and «Установить» elsewhere.
    let search_esc = if !detail && search_room {
        let step = 150.0 * k * g;
        let bx = inner.x + inner.w - 10.0 * k - bw;
        let x0 = lx + 2.0 * step + import_w + gap;
        let (fx, fy, fh, avail) = if narrow { (lx, field_y, th, lw) } else { (x0, by, bh, bx - gap - x0) };
        // The clear control is a square as tall as the field (44 dp on a phone, grown with the
        // text size), drawn with the window's own close cross (`chrome::close_button`).
        let sq = fh;
        let field_w = avail - sq - gap;
        let was_typing = has_focus(SEARCH_KEY);
        let before = view.query.clone();
        // The typed text and its hint: the list's row-title size on a phone (16 units times the
        // scale), the desktop's 17 as before. On a phone they are drawn in Razdor's own metrics
        // (`legacy_metrics_if`, as the rows are): the original font's native metrics shrink them.
        let field_px = if phone { (16.0 * k * g).round() } else { 17.0 };
        super::dt_font::legacy_metrics_if(phone, || text_field_sized(SEARCH_KEY, fx, fy, field_w, fh, &mut view.query, false, field_px));
        if view.query != before {
            view.scroll = 0;
        }
        if view.query.is_empty() && !has_focus(SEARCH_KEY) {
            super::dt_font::legacy_metrics_if(phone, || text_fit(tr("Search…"), fx + 8.0 * k, fy + fh / 2.0 + field_px * 0.35, field_w - 16.0 * k, field_px, Color::new(0.85, 0.80, 0.68, 0.7)));
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

    // The list shows the catalog and the packages imported from files (`hub::with_imported`).
    let catalog = view.catalog.clone().map(|r| r.map(|c| hub::with_imported(&c, &view.installed)));
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
            let entries = shown(c, view.tab, &view.query, &view.installed);
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
                let click = if modal { None } else { click };
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
                    super::dt_font::legacy_metrics_if(narrow, || {
                    let (words, ink) = state(c, &view.installed, e, view.marks.contains(&e.id));
                    // Upright: the title takes the whole row and the line under it says the state
                    // (what is missing, the install's word) or else the kind, author and rating.
                    // Elsewhere the state sits at the title's end, shortened to half the row.
                    let (title_px, line_px, t_y, l_y) = if narrow { ((16.0 * k * g).round(), (14.0 * k * g).round(), 23.0, 44.0) } else { (size, small, 15.0, 29.0) };
                    let state_here = !words.is_empty() && narrow;
                    let words = if words.is_empty() || narrow { words } else { ellipsize(&words, rr.w * 0.5, small) };
                    let sw = if words.is_empty() || narrow { 0.0 } else { measure(&words, small).width + 8.0 * k };
                    text_fit(e.title.get(lang()), rr.x + 6.0 * k, rr.y + t_y * g * k, rr.w - sw - 12.0 * k, title_px, if picked { GOLD } else { CREAM });
                    if state_here {
                        chrome::shadow_text(&ellipsize(&words, rr.w - 12.0 * k, line_px), rr.x + 6.0 * k, rr.y + l_y * g * k, line_px, ink);
                    } else {
                        let mut by_line = format!("{} · {}", kind_label(e.kind), e.author);
                        if let Some(rating) = e.rating {
                            by_line = format!("{by_line} · +{} / -{}", rating.up, rating.down);
                        }
                        text_fit(&by_line, rr.x + 6.0 * k, rr.y + l_y * g * k, rr.w - 12.0 * k, line_px, Color::new(0.85, 0.80, 0.68, 1.0));
                    }
                    if !words.is_empty() && !narrow {
                        chrome::shadow_right(&words, rr.x + rr.w - 6.0 * k, rr.y + t_y * g * k, small, ink);
                    }
                    });
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
    // Esc closes the window only when no notice or report is over it (their own Esc closes them).
    let mut close = closed || (key(KeyCode::Escape) && !search_esc && !modal);
    if close {
        // Closing the window hides the keyboard the search field may have.
        clear_focus();
    }
    if detail {
        if button(lx, by, back_w, bh, tr("To the list"), !modal) {
            view.detail = false;
        }
    } else {
        let third = (lw - 2.0 * gap) / 3.0;
        let (w_b, step) = if narrow { (third, third + gap) } else { (back_w, 150.0 * k * g) };
        if button(lx, by, w_b, bh, tr("Back"), !modal) {
            close = true;
        }
        if button(lx + step, by, w_b, bh, tr("Refresh"), view.job.is_none() && view.fetching.is_none() && !modal) {
            view.reload();
        }
        let importing = view.picker.is_some() || view.import.is_some();
        // A phone held upright has a third of the row: its label is the short one.
        let imp_w = if narrow { third } else { import_w };
        let imp_label = if narrow { tr("Import…") } else { tr("Import a package…") };
        if button(lx + 2.0 * step, by, imp_w, bh, imp_label, view.job.is_none() && !importing && !modal) {
            match hub_pack::start_pick() {
                Ok(pick) => view.picker = Some(pick),
                Err(why) => view.notice = Some(Notice::new(tr("Import a package…"), vec![why])),
            }
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
    } else if view.offline && narrow {
        text_fit(tr("No connection: showing the saved list."), status_x, note_y, status_w, size, GOLD);
    }
    // The line under the list: the saved list's notice when the hub could not be reached (a phone
    // held upright has it above the bottom row instead), else the hub's place on the desktop only (a
    // phone's space goes to the list; a hub that can't be read says so in the list).
    let under = list.y + list.h + 18.0 * k;
    if view.offline && !narrow {
        text_fit(tr("No connection: showing the saved list."), lx, under, lw, small, GOLD);
    } else if let (false, false, Some(h)) = (narrow, phone, &view.hub) {
        let from = trf!("Hub: {place}", place = h.source.label());
        text_fit(&from, lx, under, lw, small, Color::new(0.85, 0.80, 0.68, 0.8));
    }
    // A package's play closes the window: the game opens it.
    if view.play.is_some() {
        close = true;
    }
    if let Some(form) = view.report.as_mut() {
        match hub_pack::report_window(form) {
            ReportAnswer::Open => {}
            ReportAnswer::Cancel => view.report = None,
            ReportAnswer::Send(text) => {
                hub_pack::send_report(&text);
                view.report = None;
                view.note = Some((tr("Copied: paste it into the reports thread on Discord.").into(), false));
            }
        }
    }
    if let Some(n) = view.notice.as_mut() {
        if hub_pack::notice_window(n) {
            view.notice = None;
        }
    }
    close
}

/// A card's hero on a phone held upright (the hub's details, the scenario card): the picture as
/// wide as `r` and at most `max_h` tall, its centre band cropped to that height (a dark frame while
/// it loads, or with no picture), a dark fade over its bottom half, and the title over the fade in the
/// title font: at most two lines, the last 16 units above the picture's bottom and the first 32 above
/// that, a longer title cut with an ellipsis. `k` is the window's scale (lines), `s` the text's (`k`
/// times the text size). Returns the picture's height.
pub(super) fn hero(r: Rect, picture: Option<&Texture2D>, max_h: f32, title: &str, k: f32, s: f32) -> f32 {
    let (w, h) = picture.map_or((16.0, 9.0), |t| (t.width(), t.height()));
    let hh = (r.w * h / w).min(max_h);
    match picture {
        Some(t) => {
            let sh = w * hh / r.w;
            let sy = (h - sh) / 2.0;
            draw_texture_ex(t, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, hh)), source: Some(Rect::new(0.0, sy, w, sh)), ..Default::default() });
        }
        None => draw_rectangle(r.x, r.y, r.w, hh, Color::new(0.05, 0.04, 0.03, 0.8)),
    }
    // A dark fade over the bottom half, from clear to 85% black: one pixel row at a time on
    // whole pixels, so no seam between bands shows through the title.
    let (top, bottom) = ((r.y + hh / 2.0).floor(), (r.y + hh).floor());
    let rows = (bottom - top).max(1.0);
    let mut py = top;
    while py < bottom {
        let a = 0.85 * (py - top + 1.0) / rows;
        draw_rectangle(r.x, py, r.w, 1.0, Color::new(0.02, 0.03, 0.02, a));
        py += 1.0;
    }
    draw_rectangle_lines(r.x, r.y, r.w, hh, 2.0 * k, Color::new(0.75, 0.72, 0.62, 0.8));
    // The title in the title font, centered over the fade.
    let title_px = (28.0 * s).round();
    let width = r.w - 24.0 * s;
    super::dt_font::with_face(super::dt_font::Face::Title, || {
        // The largest size that keeps the title on one line (28, 24, then 20 units), else the
        // largest that keeps it on two, else three lines at 20; a longer one is cut with an ellipsis.
        let sizes = [title_px, (24.0 * s).round(), (20.0 * s).round()];
        let fits = |n: usize| sizes.iter().map(|&z| (z, wrap_long(title, width, z))).find(|(_, l)| l.len() <= n);
        let (tz, mut lines) = fits(1).or_else(|| fits(2)).unwrap_or_else(|| (sizes[2], wrap_long(title, width, sizes[2])));
        if lines.len() > 3 {
            let rest = lines.split_off(2).join(" ");
            lines.push(ellipsize(&rest, width, tz));
        }
        let last = r.y + hh - 16.0 * s;
        for (i, l) in lines.iter().enumerate() {
            let up = (lines.len() - 1 - i) as f32;
            let tx = r.x + (r.w - measure(l, tz).width) / 2.0;
            chrome::shadow_text(l, tx, last - up * 32.0 * s * tz / title_px, tz, GOLD);
        }
    });
    hh
}

/// A pane's scroll, as the hub's details pane has it: a swipe or the wheel over it (40 px a notch),
/// a drag with the finger, and the bar on its right. `most` is the content's height above the pane's
/// (0 when it fits). Returns the room the bar takes on the pane's right: the content is drawn that
/// much narrower.
pub(super) fn pane_scroll(scroll: &mut f32, drag: &mut Option<Drag>, pane: Rect, most: f32, k: f32, phone: bool) -> f32 {
    let room = if most > 0.0 { bar_room(k, phone) } else { 0.0 };
    let body = Rect::new(pane.x, pane.y, pane.w - room, pane.h);
    swipe_rows(body, 40.0 * k);
    if !input_blocked() && mouse_in(body.x, body.y, body.w, body.h) {
        *scroll -= wheel() * 40.0 * k;
    }
    let (moved, _) = drag_frame(drag, body, *scroll, 1.0, most, drag_slop());
    *scroll = clamp_scroll(moved, most);
    scrollbar_px(bar_rect(pane.x + pane.w, pane.y, pane.h, k, phone), scroll, most, pane.h, 40.0 * k);
    room
}

/// The picked entry in the details pane `pane`, scrolled by the wheel, a swipe, a drag or the
/// bar on its right, and clipped to the pane so no text reaches the bottom row. Install or
/// Remove sits at the bottom right of the window (`lay`), below the pane.
fn details(view: &mut HubView, c: &Catalog, e: &Entry, pane: Rect, lay: &Lay) {
    let (k, size, small, by, bh, bw) = (lay.k, lay.size, lay.small, lay.by, lay.bh, lay.bw);
    let s = k * lay.g;
    // A phone held upright draws the pane's text larger; elsewhere the sizes are the original's.
    let (title_px, info_px, body_px, info_step, body_step) = if lay.narrow {
        ((28.0 * s).round(), (15.0 * s).round(), (16.0 * s).round(), 22.0 * s, 24.0 * s)
    } else {
        ((18.0 * s).round(), small, size, 18.0 * s, 19.0 * s)
    };
    let dim = Color::new(0.85, 0.80, 0.68, 1.0);
    if view.detail_for.as_deref() != Some(e.id.as_str()) {
        view.detail_for = Some(e.id.clone());
        view.detail_scroll = 0.0;
        view.detail_h = 0.0;
    }
    // The bar and the text's width follow the content's height as the last frame drew it.
    let most = (view.detail_h - pane.h).max(0.0);
    let room = pane_scroll(&mut view.detail_scroll, &mut view.pane_drag, pane, most, k, lay.phone);
    let scroll = view.detail_scroll;

    // The content, from the pane's top less the scroll, drawn clipped to the pane.
    let r = Rect::new(pane.x, pane.y - scroll, pane.w - room, pane.h);
    let installed = view.installed.has(&e.id);
    let unchecked = view.installed.entries.get(&e.id).is_some_and(|rec| rec.unchecked);
    // A package's installed versions, newest first, and the one its card plays: the picked one (or the
    // last played), else the newest. The dropdown shows that one; Play starts its file.
    let versions = view.hub.as_ref().filter(|_| installed && e.kind == Kind::Package).map(|h| h.package_versions(&e.id)).unwrap_or_default();
    let shown = view.hub.as_ref().filter(|_| !versions.is_empty()).and_then(|h| h.default_version(&e.id));
    let shown_at = shown.as_ref().and_then(|v| versions.iter().position(|x| x == v)).unwrap_or(0);
    let play_path = match (&view.hub, &shown) {
        (Some(h), Some(v)) => h.play_path_of(&e.id, v),
        _ => None,
    };
    let update = view.marks.contains(&e.id);
    let install_to = view.hub.as_ref().and_then(|h| h.catalog_update(c, &e.id));
    let controls = card_controls(&versions, update, install_to.as_deref());
    let idle = view.job.is_none() && view.import.is_none() && view.report.is_none() && view.notice.is_none();
    let users = if installed { hub::needed_by(c, &view.installed, &e.id) } else { Vec::new() };
    let plan = (!installed).then(|| hub::plan_install(c, &view.installed, &e.id));
    let clip = Clip::new(pane);
    // A phone held upright: the pane's text takes its own sizes (Razdor's), not the original
    // font's native size, as item_text does (`dt_font::legacy_metrics_if`).
    let content_h = super::dt_font::legacy_metrics_if(lay.narrow, || {
        let mut y = r.y + 22.0 * s;
        let picture = shown_picture(e).is_some();
        let loaded = view.preview.as_ref().filter(|p| p.key == preview_key(e)).and_then(|p| p.texture.clone());
        if picture && !lay.narrow {
            // The picture at the top of the pane: as wide as the pane, at most PREVIEW_H tall (and
            // 40% of the pane), its aspect kept. Room for it is kept while it loads (a dark frame of
            // 16:9 until then).
            let (w, h) = loaded.as_ref().map_or((16.0, 9.0), |t| (t.width(), t.height()));
            let max_h = (PREVIEW_H * k).min(pane.h * 0.4);
            let scale = (r.w / w).min(max_h / h);
            let (dw, dh) = (w * scale, h * scale);
            match &loaded {
                Some(t) => draw_texture_ex(t, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(dw, dh)), ..Default::default() }),
                None => draw_rectangle(r.x, r.y, dw, dh, Color::new(0.05, 0.04, 0.03, 0.8)),
            }
            y = r.y + dh + 16.0 * k;
        }
        if lay.narrow {
            if picture {
                // A phone held upright: the picture is the hero, as wide as the pane and at most 45% of
                // the pane tall, its title over its bottom half (`hero`).
                let hh = hero(r, loaded.as_ref(), pane.h * 0.45, e.title.get(lang()), k, s);
                y = r.y + hh + 34.0 * s;
            } else {
                // No picture: the title is centered at the top, each line of it, in the title font.
                super::dt_font::with_face(super::dt_font::Face::Title, || {
                    for l in wrap_long(e.title.get(lang()), r.w, title_px).iter() {
                        let tx = r.x + (r.w - measure(l, title_px).width) / 2.0;
                        chrome::shadow_text(l, tx, y, title_px, GOLD);
                        y += 30.0 * s;
                    }
                });
            }
            // Upright: the author is centered between gold rules.
            let author = trf!("Author: {author}", author = e.author);
            let aw = measure(&author, info_px).width.min(r.w - 2.0 * (34.0 * s));
            let ax = r.x + (r.w - aw) / 2.0;
            text_fit(&author, ax, y, aw, info_px, dim);
            let mid = y - 0.35 * info_px;
            let rule = Color::new(0.85, 0.70, 0.35, 0.7);
            draw_line(ax - 10.0 * s - 24.0 * s, mid, ax - 10.0 * s, mid, 1.0 * k, rule);
            draw_line(ax + aw + 10.0 * s, mid, ax + aw + 10.0 * s + 24.0 * s, mid, 1.0 * k, rule);
            y += info_step + 6.0 * s;
            // The chips: kind, version, date, rating and the status, in rows centered under the author.
            let mut chips: Vec<(String, Color)> = vec![(kind_label(e.kind).to_string(), dim)];
            if !e.version.trim().is_empty() {
                chips.push((format!("v{}", e.version), dim));
            }
            if !e.added.is_empty() {
                chips.push((e.added.clone(), dim));
            }
            if let Some(rating) = e.rating {
                chips.push((format!("+{} / -{}", rating.up, rating.down), GREEN_TEXT));
            }
            chips.push(match e.status {
                Status::Full => (tr("Works fully").into(), GREEN_TEXT),
                Status::Partial => (tr("Works partly").into(), GOLD),
                Status::Broken => (tr("Does not start").into(), RED_TEXT),
                Status::Unknown => (tr("Not checked yet").into(), dim),
                Status::Unlisted => (tr("Taken off the list").into(), dim),
            });
            if e.kind == Kind::Package {
                if unchecked {
                    chips.push((tr("Not checked by the Hub").into(), GOLD));
                }
                if controls.mark {
                    chips.push((tr("New version").into(), GOLD));
                }
                if !e.base.trim().is_empty() {
                    chips.insert(1, (trf!("Base: {base}", base = e.base), dim));
                }
            }
            let ch = (info_px * 1.8).round();
            let pad = 10.0 * s;
            let gap = 8.0 * s;
            // Each chip's width: its label and a pad either side, never wider than the pane.
            let widths: Vec<f32> = chips.iter().map(|(l, _)| (measure(l, info_px).width + 2.0 * pad).min(r.w)).collect();
            let mut rows: Vec<Vec<usize>> = Vec::new();
            let mut used = 0.0;
            for (i, w) in widths.iter().enumerate() {
                match rows.last_mut() {
                    Some(row) if used + gap + w <= r.w => {
                        row.push(i);
                        used += gap + w;
                    }
                    _ => {
                        rows.push(vec![i]);
                        used = *w;
                    }
                }
            }
            let mut top = y - info_px;
            for row in &rows {
                let total: f32 = row.iter().map(|&i| widths[i]).sum::<f32>() + gap * (row.len() - 1) as f32;
                let mut cx = r.x + (r.w - total) / 2.0;
                for &i in row {
                    let (label, ink) = &chips[i];
                    let w = widths[i];
                    draw_rectangle(cx, top, w, ch, Color::new(0.0, 0.0, 0.0, 0.35));
                    draw_rectangle_lines(cx, top, w, ch, 2.0 * k, Color::new(0.85, 0.70, 0.35, 0.55));
                    if measure(label, info_px).width + 2.0 * pad <= w {
                        chrome::shadow_text(label, cx + pad, top + ch / 2.0 + info_px * 0.35, info_px, *ink);
                    } else {
                        text_fit(label, cx + pad, top + ch / 2.0 + info_px * 0.35, w - 2.0 * pad, info_px, *ink);
                    }
                    cx += w + gap;
                }
                top += ch + gap;
            }
            // The next line is drawn on its baseline: its capitals start 14 units under the chips.
            y = top - gap + info_px + 14.0 * s;
            // A status that is not full says why, under the chips.
            if matches!(e.status, Status::Partial | Status::Broken) {
                let note = e.status_note.get(lang());
                if !note.trim().is_empty() {
                    let ink = if e.status == Status::Partial { GOLD } else { RED_TEXT };
                    for l in wrap_long(note, r.w, info_px).iter() {
                        chrome::shadow_text(l, r.x, y, info_px, ink);
                        y += info_step;
                    }
                }
            }
        } else {
            text_fit(e.title.get(lang()), r.x, y, r.w, title_px, GOLD);
            y += 22.0 * s;
            let mut by_line = if e.version.trim().is_empty() {
                trf!("{kind} by {author}", kind = kind_label(e.kind), author = e.author)
            } else {
                trf!("{kind} by {author}, version {version}", kind = kind_label(e.kind), author = e.author, version = e.version)
            };
            if !e.added.is_empty() {
                by_line = format!("{by_line} · {}", e.added);
            }
            text_fit(&by_line, r.x, y, r.w, info_px, dim);
            y += info_step;
            if let Some(rating) = e.rating {
                text_fit(&trf!("Rating on Discord: +{up} / -{down}", up = rating.up, down = rating.down), r.x, y, r.w, info_px, dim);
                y += info_step;
            }
            // The status is read whole: its lines wrap.
            let (words, ink) = status_line(e);
            for l in wrap_long(&words, r.w, info_px).iter() {
                chrome::shadow_text(l, r.x, y, info_px, ink);
                y += info_step;
            }
        }
        if e.kind == Kind::Campaign {
            text_fit(&trf!("Chapters: {n}", n = e.map_count()), r.x, y, r.w, info_px, dim);
            y += info_step;
        }
        // The base and the unchecked note are chips on a phone held upright (above), lines here.
        if e.kind == Kind::Package && !lay.narrow {
            if !e.base.trim().is_empty() {
                text_fit(&trf!("Base: {base}", base = e.base), r.x, y, r.w, info_px, dim);
                y += info_step;
            }
            if unchecked {
                text_fit(tr("Not checked by the Hub: it came from a file, not from the list."), r.x, y, r.w, info_px, GOLD);
                y += info_step;
            }
            if controls.mark {
                text_fit(tr("New version"), r.x, y, r.w, info_px, GOLD);
                y += info_step;
            }
        }
        if !matches!(e.kind, Kind::Mod | Kind::Package) {
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
            text_fit(&trf!("Languages: {list}", list = langs.join(", ")), r.x, y, r.w, info_px, dim);
            y += info_step;
        }
        // Why a mod can't be installed, in the text (the bottom row keeps its disabled Install).
        if let Some(Err(why)) = &plan {
            y += 4.0 * s;
            let why_text = match why {
                PlanError::Missing(m) => trf!("Can't be installed: the hub has no mod «{m}».", m),
                PlanError::Cycle(m) => trf!("Can't be installed: mods need each other («{m}»).", m),
            };
            for l in wrap_long(&why_text, r.w, info_px).iter() {
                chrome::shadow_text(l, r.x, y, info_px, RED_TEXT);
                y += if lay.narrow { info_step } else { 16.0 * s };
            }
        }

        // The mods it needs, each with its state.
        if !e.requires.is_empty() {
            y += 8.0 * s;
            chrome::shadow_text(tr("Needs:"), r.x, y, body_px, CREAM);
            y += body_step;
            for id in &e.requires {
                let (name, words, ink) = match c.entry(id) {
                    None => (id.as_str(), tr("not in the hub"), RED_TEXT),
                    Some(m) if view.installed.has(id) => (m.title.get(lang()), tr("installed"), GREEN_TEXT),
                    Some(m) => (m.title.get(lang()), tr("will be installed too"), GOLD),
                };
                text_fit(&format!("• {name}: {words}"), r.x + 8.0 * k, y, r.w - 8.0 * k, info_px, ink);
                y += if lay.narrow { info_step } else { 19.0 * s };
            }
        }

        // A package's own actions: play its installed copy, and report a problem with it. Each is drawn
        // only where it is inside the pane (a scrolled-away one takes no click).
        if e.kind == Kind::Package {
            y += 10.0 * s;
            let pw = ((r.w - 8.0 * s) / 2.0).min(220.0 * k * lay.g);
            let visible = |y: f32| y >= pane.y && y + bh <= pane.y + pane.h;
            // The version picker (more than one version installed): a label and the field, on a row of its own.
            if controls.picker {
                let label_w = measure(tr("Version"), size).width + 10.0 * k;
                if visible(y) {
                    text_fit(tr("Version"), r.x, y + bh / 2.0 + size * 0.35, label_w, size, CREAM);
                    let field = Rect::new(r.x + label_w, y, (r.w - label_w).min(220.0 * k * lay.g), bh);
                    if let Some(i) = version_picker(field, size, k, &versions, shown_at, idle) {
                        if let Some(h) = view.hub.as_ref() {
                            if let Err(why) = h.set_picked(&e.id, &versions[i]) {
                                view.note = Some((why, true));
                            }
                        }
                    }
                }
                y += bh + 10.0 * s;
            }
            let mut x = r.x;
            if installed && visible(y) && button(x, y, pw, bh, tr("Play"), idle) {
                view.play = play_path.clone();
            }
            if installed {
                x += pw + 8.0 * s;
            }
            if visible(y) && button(x, y, pw, bh, tr("Report a problem"), idle) {
                view.report = Some(ReportForm { id: e.id.clone(), version: e.version.clone(), title: e.title.get(lang()).to_string(), text: String::new() });
            }
            y += bh + 10.0 * s;
            // Install X.Y.Z: a row under the actions. The bottom row has no room beside Remove (Import and
            // the search field take it), and a phone held upright has none either.
            if let Some(v) = controls.install.as_deref() {
                if visible(y) && button(r.x, y, pw, bh, &trf!("Install {version}", version = v), idle) {
                    start_install(view, &e.id);
                }
                y += bh + 10.0 * s;
            }
        }
        // The description, whole (scrolled, not cut): its long words wrap at the pane's width.
        // The map's own story (`about`) comes first; the post's text (`description`) follows it under a
        // heading. Without a story the description is drawn alone, as before.
        y += 10.0 * s;
        let story = e.about.get(lang()).trim();
        let post = e.description.get(lang()).trim();
        if lay.narrow && (!story.is_empty() || !post.is_empty()) {
            // A phone held upright: a thin gold rule with a small diamond in its middle sets the text apart.
            let (cx, cy, half) = (r.x + r.w / 2.0, y - 4.0 * s, 5.0 * s);
            let rule = Color::new(0.85, 0.70, 0.35, 0.6);
            let ink = Color::new(0.85, 0.70, 0.35, 0.9);
            draw_line(r.x, cy, cx - half - 6.0 * s, cy, 1.0 * k, rule);
            draw_line(cx + half + 6.0 * s, cy, r.x + r.w, cy, 1.0 * k, rule);
            draw_triangle(vec2(cx - half, cy), vec2(cx, cy - half), vec2(cx, cy + half), ink);
            draw_triangle(vec2(cx, cy - half), vec2(cx + half, cy), vec2(cx, cy + half), ink);
            y += 18.0 * s;
        }
        if !story.is_empty() {
            for l in wrap_long(story, r.w, body_px).iter() {
                chrome::shadow_text(l, r.x, y, body_px, CREAM);
                y += body_step;
            }
            if !post.is_empty() {
                let heading = tr("From the author's post");
                if lay.narrow {
                    y += 14.0 * s;
                    text_fit(heading, r.x, y, r.w, info_px, GOLD);
                    y += info_step;
                    for l in wrap_long(post, r.w, info_px).iter() {
                        chrome::shadow_text(l, r.x, y, info_px, dim);
                        y += info_step;
                    }
                } else {
                    y += 8.0 * s;
                    text_fit(heading, r.x, y, r.w, small, GOLD);
                    y += 18.0 * s;
                    for l in wrap_long(post, r.w, small).iter() {
                        chrome::shadow_text(l, r.x, y, small, dim);
                        y += 16.0 * s;
                    }
                }
            }
        } else {
            for l in wrap_long(e.description.get(lang()), r.w, body_px).iter() {
                chrome::shadow_text(l, r.x, y, body_px, CREAM);
                y += body_step;
            }
        }
        if e.kind == Kind::Mod {
            y += 8.0 * s;
            for l in wrap_long(tr("A mod is played from the new game window's Mod list, over the base game."), r.w, info_px).iter() {
                chrome::shadow_text(l, r.x, y, info_px, dim);
                y += if lay.narrow { info_step } else { 16.0 * s };
            }
        }
        if matches!(e.kind, Kind::Map | Kind::Campaign) && installed {
            y += 8.0 * s;
            text_fit(tr("Its maps are in the new game's list."), r.x, y, r.w, info_px, GREEN_TEXT);
            y += info_step;
        }
        if e.kind == Kind::Package && installed {
            y += 8.0 * s;
            for l in wrap_long(tr("Play starts the package's map as a new game."), r.w, info_px).iter() {
                chrome::shadow_text(l, r.x, y, info_px, dim);
                y += if lay.narrow { info_step } else { 16.0 * s };
            }
        }
        if !users.is_empty() {
            y += 8.0 * s;
            let names = view.titles(&users);
            text_fit(&trf!("Needed by: {names}", names), r.x, y, r.w, info_px, GOLD);
            y += info_step;
        }
        y - r.y + 8.0 * s
    });
    drop(clip);
    view.detail_h = content_h;

    // Install or remove: the bottom row, below the pane.
    let busy = view.job.is_some();
    let bx = pane.x + pane.w - bw;
    if installed {
        if button(bx, by, bw, bh, tr("Remove"), !busy && users.is_empty() && idle) {
            let id = e.id.clone();
            view.start(move |hub, c| Done::Removed(hub.remove(&c, &id).map(|_| id)));
        }
        return;
    }
    match plan {
        Some(Ok(plan)) => {
            let label = if plan.len() > 1 { trf!("Install (+{n})", n = plan.len() - 1) } else { tr("Install").into() };
            if button(bx, by, bw, bh, &label, !busy && idle) {
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

    /// The mods an entry needs are named: one («Нужен мод»), or several («Нужны моды», comma-separated).
    #[test]
    fn needs_words_name_the_mods() {
        assert_eq!(needs_words(Lang::Ru, &["Evolution"]), "Нужен мод: Evolution");
        assert_eq!(needs_words(Lang::Ru, &["A", "B"]), "Нужны моды: A, B");
        assert_eq!(needs_words(Lang::En, &["Evolution"]), "Needs mod: Evolution");
        assert_eq!(needs_words(Lang::En, &["A", "B", "C"]), "Needs mods: A, B, C");
    }

    /// A row names its mod all the time: the missing ones while some are missing (as before), else
    /// the mods it is made for.
    #[test]
    fn a_row_of_a_map_that_needs_mods_always_names_them() {
        assert_eq!(mod_row_words(Lang::Ru, &["Эволюция (макет)"], &["Эволюция (макет)"]).as_deref(), Some("Нужен мод: Эволюция (макет)"), "one missing: its name");
        assert_eq!(mod_row_words(Lang::Ru, &["A", "B"], &["A", "B"]).as_deref(), Some("Нужны моды: A, B"));
        assert_eq!(mod_row_words(Lang::Ru, &["Mercs"], &["Evolution", "Mercs"]).as_deref(), Some("Нужен мод: Mercs"), "only the missing are named");
        assert_eq!(mod_row_words(Lang::Ru, &[], &["Evolution"]).as_deref(), Some("Для мода: Evolution"), "installed: the mod's name");
        assert_eq!(mod_row_words(Lang::Ru, &[], &["A", "B"]).as_deref(), Some("Для модов: A, B"));
        assert_eq!(mod_row_words(Lang::En, &["Evolution"], &["Evolution"]).as_deref(), Some("Needs mod: Evolution"));
        assert_eq!(mod_row_words(Lang::En, &[], &["Evolution"]).as_deref(), Some("For mod: Evolution"));
        assert_eq!(mod_row_words(Lang::Ru, &[], &[]), None, "no mod needed: no words");
    }

    /// The search finds an entry by the title or the id of a mod it requires, not only its own.
    #[test]
    fn the_search_finds_the_maps_made_for_a_mod() {
        let c = mock_catalog("evolu-search");
        let castle = c.entry("rotten-castle").unwrap();
        let merc = c.entry("merc-road").unwrap();
        let ford = c.entry("ford-outpost").unwrap();
        assert!(matches(&c, castle, "evolu"), "its mod's English title");
        assert!(matches(&c, merc, "evolu"), "its mod's title and the mercenaries' add-on");
        assert!(matches(&c, merc, "mercenaries"), "the add-on's own title");
        assert!(matches(&c, castle, "evolution-mock"), "the mod's id");
        assert!(!matches(&c, ford, "evolu"), "a map with no mod");
        assert!(matches(&c, castle, "Осада"), "its own title still counts");
    }

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

    /// The mock hub's catalog (nothing from the game), in a folder of its own.
    fn mock_catalog(tag: &str) -> Catalog {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let local = std::env::temp_dir().join(format!("razdor-hubview-{tag}-{}-{nanos}", std::process::id()));
        Hub { source: Source::Mock, local, trust_dir: None }.catalog().unwrap()
    }

    /// A window with its catalog and pick set; no thread, no drawing (the tests call the pure parts).
    fn window(hub: Option<Hub>, catalog: Option<Result<Catalog, String>>, picked: Option<&str>) -> HubView {
        HubView {
            hub,
            catalog,
            installed: Installed::default(),
            tab: 0,
            picked: picked.map(Into::into),
            detail: false,
            scroll: 0,
            list_drag: None,
            pane_drag: None,
            detail_scroll: 0.0,
            detail_h: 0.0,
            detail_for: None,
            query: String::new(),
            job: None,
            fetching: None,
            offline: false,
            note: None,
            changed: false,
            preview: None,
            picker: None,
            import: None,
            notice: None,
            report: None,
            play: None,
            marks: Vec::new(),
        }
    }

    const URL: &str = "https://example.org/hub/latest/download";

    fn hub_at(source: Source, tag: &str) -> Hub {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        Hub { source, local: std::env::temp_dir().join(format!("razdor-hubview-{tag}-{}-{nanos}", std::process::id())), trust_dir: None }
    }

    /// A swap keeps the entry picked when the new catalog still has it, and takes the first when not.
    #[test]
    fn a_swap_keeps_the_pick_by_id_or_takes_the_first() {
        let c = mock_catalog("pick");
        let (first, other) = (c.entries[0].id.clone(), c.entries[1].id.clone());
        assert_eq!(keep_pick(Some(&other), &c), Some(other.clone()));
        assert_eq!(keep_pick(Some("gone"), &c), Some(first.clone()));
        assert_eq!(keep_pick(None, &c), Some(first.clone()));
        assert_eq!(keep_pick(Some(&other), &Catalog { format: 1, entries: vec![] }), None);

        // The same entries in another order, and one title changed: the pick is the same entry.
        let mut fresh = c.clone();
        fresh.entries.reverse();
        fresh.entries.iter_mut().find(|e| e.id == other).unwrap().title.ru = "Новое название".into();
        let mut v = window(None, Some(Ok(c.clone())), Some(&other));
        v.landed(Ok(fresh.clone()));
        assert_eq!(v.picked.as_deref(), Some(other.as_str()));
        assert_eq!(v.catalog, Some(Ok(fresh)));
        assert!(!v.offline);
    }

    /// A failed fetch with a list on the screen keeps the list and sets the offline state; the
    /// next good fetch clears it.
    #[test]
    fn a_failed_fetch_keeps_the_saved_list_and_says_so() {
        let c = mock_catalog("offline");
        let other = c.entries[1].id.clone();
        let mut v = window(None, Some(Ok(c.clone())), Some(&other));
        v.landed(Err("curl: could not resolve host".into()));
        assert_eq!(v.catalog, Some(Ok(c.clone())), "the saved list stays");
        assert_eq!(v.picked.as_deref(), Some(other.as_str()));
        assert!(v.offline, "the status line says the list is saved");
        v.landed(Ok(c.clone()));
        assert!(!v.offline, "a fetch that works clears it");
    }

    /// A failed fetch with no list on the screen shows the error in the list, as before the cache.
    #[test]
    fn a_failed_fetch_with_no_list_shows_the_error() {
        let mut v = window(None, None, None);
        v.landed(Err("boom".into()));
        assert_eq!(v.catalog, Some(Err("boom".to_string())));
        assert!(!v.offline);
    }

    /// The cache is on the screen at once, for the address it was fetched from only; a corrupt
    /// cache leaves the window reading the hub.
    #[test]
    fn the_cache_is_shown_at_once_for_its_own_address_only() {
        let c = mock_catalog("cache-view");
        let first = c.entries[0].id.clone();
        let mine = hub_at(Source::Url(URL.into()), "mine");
        hub::write_cache(&mine.local, URL, &c).unwrap();

        let mut v = window(Some(mine.clone()), None, None);
        v.show_cache(&mine);
        assert_eq!(v.catalog, Some(Ok(c.clone())));
        assert_eq!(v.picked.as_deref(), Some(first.as_str()), "the first entry is picked");

        let other = Hub { source: Source::Url("https://example.org/other".into()), local: mine.local.clone(), trust_dir: None };
        let mut v = window(Some(other.clone()), None, None);
        v.show_cache(&other);
        assert_eq!(v.catalog, None, "another address's cache is not shown");

        std::fs::write(mine.local.join(hub::CACHE_DIR).join(hub::INDEX), b"{ corrupt").unwrap();
        let mut v = window(Some(mine.clone()), None, None);
        v.show_cache(&mine);
        assert_eq!(v.catalog, None, "a corrupt cache is ignored");
        let _ = std::fs::remove_dir_all(&mine.local);
    }

    /// The card's controls: one version shows no picker and no mark; two show the picker; a newer
    /// version nobody played shows the mark; a catalog version not installed is the Install button.
    #[test]
    fn the_card_shows_the_controls_its_versions_call_for() {
        let one = vec!["0.1.0".to_string()];
        let two = vec!["0.2.0".to_string(), "0.1.0".to_string()];
        assert_eq!(card_controls(&one, false, None), CardControls { picker: false, mark: false, install: None });
        assert_eq!(card_controls(&two, false, None), CardControls { picker: true, mark: false, install: None });
        assert_eq!(card_controls(&two, true, None), CardControls { picker: true, mark: true, install: None });
        assert_eq!(card_controls(&one, true, Some("0.2.0")), CardControls { picker: false, mark: true, install: Some("0.2.0".into()) });
        assert_eq!(card_controls(&two, false, Some("0.3.0")).install.as_deref(), Some("0.3.0"));
        assert!(!card_controls(&[], true, None).mark, "a mark needs an installed version");
    }
}
