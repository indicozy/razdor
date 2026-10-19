//! The original's new-game windows over the main menu's ruins: "Сценарий для Новой Игры"
//! (the scenario list with its round icons, the map in `MapImageFrame`, the name, status,
//! size and description) and "Стартовые характеристики Героя" (the three heroes' portraits,
//! the name field, and the chosen hero's description and bonus in the open book of
//! `NewHero_Paper`). The texts are the install's (`[NewGame]`, `[NewHero]`, `[Buttons]`).
//! No footage shows these two windows: their layout is ours, built from the original's art
//! (L). Without an install the plain screens of `ui::screens` stand in.

use std::cell::{Cell, RefCell};
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::i18n::{self, n_, tr, Lang};
use razdor::rules::content::{Content, ExperimentalRules, HeroClass};
use razdor::rules::game::Game;


use super::assets::Assets;
use super::audio::{cue, Cue};
use super::chrome;
use super::dt_font::{with_face, Face};
use super::widgets::*;
use super::{screens, ScenarioEntry, Screen};

thread_local! {
    static EXPERIMENTAL: Cell<ExperimentalRules> = const { Cell::new(ExperimentalRules {
        simultaneous_bonuses: false, multiple_rings: false, multiple_amulets: false, no_duplicate_items: true, two_potions: false,
    }) };
    static RULES_OPEN: Cell<bool> = const { Cell::new(false) };
    /// The scenario group picked (index into the list shown), the list's scroll in pixels
    /// at k = 1, and the hero picked.
    static PICKED: Cell<usize> = const { Cell::new(0) };
    /// The specific chapter selected in a campaign, when one is picked.
    static PICKED_MAP: Cell<Option<usize>> = const { Cell::new(None) };
    static SCROLL: Cell<f32> = const { Cell::new(0.0) };
    /// The description's first shown line, and the group it was scrolled for.
    static DESC_SCROLL: Cell<(usize, usize)> = const { Cell::new((usize::MAX, 0)) };
    static HERO: Cell<usize> = const { Cell::new(0) };
    /// A map to pick when the list opens (the `scenarios:<map>` snapshot scene).
    static PICK: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Whether the hero window is open (its class was set on opening).
    static HERO_OPEN: Cell<bool> = const { Cell::new(false) };
    /// The map preview of the scenario shown: (index into `scenarios`, texture).
    static PREVIEW: RefCell<Option<(usize, Texture2D)>> = const { RefCell::new(None) };
    /// The scenario's own picture, decoded once per scenario shown (`None`: it has none).
    static PICTURE: RefCell<Option<(usize, Option<Texture2D>)>> = const { RefCell::new(None) };
}

/// A text of the install (`[<section>] <key>`) in Russian, else ours.
fn own(section: &str, key: &str, ours: &'static str) -> String {
    let t = (i18n::lang() == Lang::Ru).then(|| chrome::ui_text(section, key)).flatten();
    t.unwrap_or_else(|| tr(ours).to_string())
}

/// The window, 594×482 video pixels as the load window, low on the screen under the logo.
struct Win {
    r: Rect,
    k: f32,
}

impl Win {
    fn open(title: &str) -> (Win, bool) {
        let k = chrome::k();
        let (w, h) = (594.0 * k, 482.0 * k);
        let r = Rect::new((screen_width() - w) / 2.0, (190.0 * k).min(screen_height() - h), w, h);
        let (_, closed) = chrome::window(r, title, chrome::Skin::Marble, true);
        // The 634×516 window with its inner frames (ae2520, read by 4d2a30; 0x48d710).
        chrome::window_inner_frames(r, chrome::opx());
        (Win { r, k }, closed)
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.r.x + x * self.k, self.r.y + y * self.k, w * self.k, h * self.k)
    }

    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, 443.0, w, 28.0);
        let (_, down, pressed) = button_state(r, enabled);
        chrome::marble_button(r, label, enabled, down);
        pressed
    }
}

/// The picked item's light: (0, 30, 90) added over its lines (the list's highlight colour
/// set at 0x4d2e0c, added in the text list's default mode 4, 0x47c848 / 0x47d71c).
fn picked_bar(r: Rect) {
    chrome::additive(|| draw_rectangle(r.x, r.y, r.w, r.h, Color::from_rgba(0, 30, 90, 255)));
}

/// The scenario icon: `SI_Castle`, `SI_Helm`, `SI_Swords`, `SI_Skull`, `SI_Tutorial` by the
/// map's picture index (header byte 0x120, 1..5), centred on `c` at its own size. The original
/// loads each as a picture with its mask (0x4dc118 → 0x48cd90) and puts it into the row's icon
/// cell as it is (0x4c1968 → 0x48e8f0): no disc and no ring under it (`si-border` belongs to
/// another window, 0x49ac64).
fn scenario_icon(picture: u8, c: Vec2, k: f32) {
    const ICONS: [&str; 5] = ["SI_Castle", "SI_Helm", "SI_Swords", "SI_Skull", "SI_Tutorial"];
    let Some(name) = (picture as usize).checked_sub(1).and_then(|i| ICONS.get(i)) else { return };
    if let Some(t) = chrome::win(name) {
        chrome::tex(&t, icon_rect(t.size(), c, k), WHITE);
    }
}

/// Where a scenario icon of `size` pixels goes: its own size at scale `k`, centred on `c`.
fn icon_rect(size: Vec2, c: Vec2, k: f32) -> Rect {
    let (w, h) = (size.x * k, size.y * k);
    Rect::new(c.x - w / 2.0, c.y - h / 2.0, w, h)
}

/// The map frame and the picture in it, from the frame's top left `at`, `px` screen pixels
/// to one of the original's: `MapImageFrame` at its 295×162 and the map's 267×134 picture
/// 14 px in (0x4c1c24 puts the frame, 0x4c1c63 the picture at +0xe, +0xe).
fn map_frame_rects(at: Vec2, px: f32) -> (Rect, Rect) {
    (Rect::new(at.x, at.y, 295.0 * px, 162.0 * px), Rect::new(at.x + 14.0 * px, at.y + 14.0 * px, 267.0 * px, 134.0 * px))
}

/// The map's terrain, one texel per cell (the minimap's colours).
fn preview(index: usize, e: &ScenarioEntry) -> Option<Texture2D> {
    PREVIEW.with(|p| {
        let mut p = p.borrow_mut();
        if let Some((i, t)) = p.as_ref() {
            if *i == index {
                return Some(t.clone());
            }
        }
        let s = &e.scenario;
        let (w, h) = (u16::try_from(s.width()).ok()?, u16::try_from(s.height()).ok()?);
        if w == 0 || h == 0 || s.terrain.len() != w as usize * h as usize {
            return None;
        }
        let rgba: Vec<u8> = s.terrain.iter().flat_map(|&c| <[u8; 4]>::from(super::world_view::surface_color(c))).collect();
        let t = Texture2D::from_rgba8(w, h, &rgba);
        t.set_filter(FilterMode::Linear);
        *p = Some((index, t.clone()));
        Some(t)
    })
}

/// The picture the map carries (a 267×134 LIT image after the header, dtm-format.md §11),
/// shown in the map frame as the original does.
fn picture(index: usize, e: &ScenarioEntry) -> Option<Texture2D> {
    PICTURE.with(|p| {
        let mut p = p.borrow_mut();
        if let Some((i, t)) = p.as_ref() {
            if *i == index {
                return t.clone();
            }
        }
        let t = e.scenario.scenario_picture.as_deref().and_then(|b| razdor::dt::gfx::decode_lit(b).ok()).and_then(|img| {
            let (w, h) = (u16::try_from(img.width).ok()?, u16::try_from(img.height).ok()?);
            (img.rgba.len() == w as usize * h as usize * 4).then(|| Texture2D::from_rgba8(w, h, &img.rgba))
        });
        *p = Some((index, t.clone()));
        t
    })
}

/// A row of the scenario list, as the original groups it: a single scenario, or a campaign
/// under its name with its chapters listed below. A new game starts on `first`: the single
/// scenario or the campaign's first map (a later chapter is reached by winning the one
/// before, so the chapters are shown, not picked).
#[derive(Debug, PartialEq)]
struct Group {
    first: usize,
    /// The campaign's maps in play order, `first` included; empty for a single scenario.
    chapters: Vec<usize>,
}

/// The groups of the list from each map's file name, header kind (0 single, 1 campaign
/// start, 2 later campaign map) and next map. A campaign's chapters follow the next-map
/// links from its first map (the later maps' own campaign names are not trusted: one map of
/// the Community Update names its campaign «Компания»). Later maps no campaign reaches are
/// left out, as before; a broken or looping chain stops where it breaks.
fn groups(maps: &[(&str, u8, &str)]) -> Vec<Group> {
    let by_file = |next: &str| {
        let next = next.trim();
        let stem = next.rsplit_once('.').filter(|(_, ext)| ext.eq_ignore_ascii_case("DTm")).map_or(next, |(stem, _)| stem);
        maps.iter().position(|(file, _, _)| !stem.is_empty() && file.to_lowercase() == stem.to_lowercase())
    };
    maps.iter()
        .enumerate()
        .filter(|(_, (_, kind, _))| *kind != 2)
        .map(|(first, &(_, kind, _))| {
            let mut chapters = Vec::new();
            if kind == 1 {
                let mut at = Some(first);
                while let Some(i) = at.filter(|i| !chapters.contains(i)) {
                    chapters.push(i);
                    at = by_file(maps[i].2);
                }
            }
            Group { first, chapters }
        })
        .collect()
}

fn scenario_groups(scenarios: &[ScenarioEntry]) -> Vec<Group> {
    let maps: Vec<(&str, u8, &str)> = scenarios.iter().map(|e| (e.file.as_str(), e.scenario.header.scenario_kind, e.scenario.next_map.as_str())).collect();
    let tutorial = chrome::ui_text("Tutorial", "Tutorial_MapName").unwrap_or_default();
    let mut list = groups(&maps);
    list.sort_by_key(|g| list_key(&scenarios[g.first].file, scenarios[g.first].scenario.width(), tutorial.trim()));
    list
}

/// The list's order (0x4e21d8): the map whose file name holds the tutorial's name first
/// (key 0), then the rest by their map's width, smaller first; a stable sort keeps the
/// file order between maps of one size.
fn list_key(file: &str, width: u32, tutorial: &str) -> u32 {
    if !tutorial.is_empty() && file.to_lowercase().contains(&tutorial.to_lowercase()) {
        0
    } else {
        width
    }
}

/// The maps the game ships with (the Community Update's install, file names without the
/// extension): the list's "original" filter. Any other map (a mod's, the player's own) is
/// fan-made. Razdor's list, for the filter only (a players' wish).
const ORIGINAL_MAPS: [&str; 15] = [
    "Другой берег",
    "ДС1-С чего все начиналось",
    "ДС2-Страшная история",
    "Обучающий1",
    "Обучающий2",
    "Проклятое озеро",
    "РК1-Начало пути",
    "РК2-Дикие пустоши",
    "РК3-Столица",
    "РК4-Восточная провинция",
    "РК5-Столица в огне",
    "РК6-Северная битва",
    "РК7-Конец",
    "Тихая пристань",
    "Устье Трейна",
];

/// A map the game ships with ([`ORIGINAL_MAPS`]).
fn is_original_map(file: &str) -> bool {
    ORIGINAL_MAPS.iter().any(|m| m.to_lowercase() == file.to_lowercase())
}

/// Which rows the list shows (Razdor's, a players' wish; `All` is the original's list).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Show {
    All,
    Campaigns,
    Singles,
    Original,
    FanMade,
    Completed,
    NotCompleted,
    Favourites,
}

impl Show {
    const ALL: [Show; 8] = [Show::All, Show::Campaigns, Show::Singles, Show::Original, Show::FanMade, Show::Completed, Show::NotCompleted, Show::Favourites];

    fn of(v: i64) -> Show {
        usize::try_from(v).ok().and_then(|i| Show::ALL.get(i)).copied().unwrap_or(Show::All)
    }

    fn label(self) -> &'static str {
        match self {
            Show::All => tr("All scenarios"),
            Show::Campaigns => tr("Campaigns"),
            Show::Singles => tr("Single scenarios"),
            Show::Original => tr("The game's own"),
            Show::FanMade => tr("Fan-made"),
            Show::Completed => tr("Completed scenarios"),
            Show::NotCompleted => tr("Not completed"),
            Show::Favourites => tr("Favourites"),
        }
    }

    fn keeps(self, f: &Facts) -> bool {
        match self {
            Show::All => true,
            Show::Campaigns => f.campaign,
            Show::Singles => !f.campaign,
            Show::Original => f.original,
            Show::FanMade => !f.original,
            Show::Completed => f.completed,
            Show::NotCompleted => !f.completed,
            Show::Favourites => f.favourite,
        }
    }
}

/// The list's order (Razdor's choices besides the original's).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Order {
    /// The original's: the tutorial first, then by the map's width ([`list_key`]).
    Original,
    /// Single scenarios by size, small first, then the campaigns in the original's order
    /// (a campaign has maps of several sizes: it is not sorted by size).
    SmallFirst,
    /// The same, large first.
    LargeFirst,
    /// By the name the row shows.
    Name,
}

impl Order {
    const ALL: [Order; 4] = [Order::Original, Order::SmallFirst, Order::LargeFirst, Order::Name];

    fn of(v: i64) -> Order {
        usize::try_from(v).ok().and_then(|i| Order::ALL.get(i)).copied().unwrap_or(Order::Original)
    }

    fn label(self) -> &'static str {
        match self {
            Order::Original => tr("As in the game"),
            Order::SmallFirst => tr("Small maps first"),
            Order::LargeFirst => tr("Large maps first"),
            Order::Name => tr("By name"),
        }
    }
}

/// What the filter and the order read of a row.
#[derive(Clone, Debug, Default, PartialEq)]
struct Facts {
    campaign: bool,
    original: bool,
    completed: bool,
    favourite: bool,
    width: u32,
    name: String,
}

/// The rows `list` (in the original's order) as `show` and `order` want them, with each
/// row's facts from `facts`.
fn arrange(list: Vec<Group>, facts: impl Fn(&Group) -> Facts, show: Show, order: Order) -> Vec<Group> {
    let mut rows: Vec<(Group, Facts)> = list.into_iter().map(|g| {
        let f = facts(&g);
        (g, f)
    }).filter(|(_, f)| show.keeps(f)).collect();
    match order {
        Order::Original => {}
        Order::SmallFirst => rows.sort_by_key(|(_, f)| (f.campaign, if f.campaign { 0 } else { f.width })),
        Order::LargeFirst => rows.sort_by_key(|(_, f)| (f.campaign, if f.campaign { 0 } else { u32::MAX - f.width })),
        Order::Name => rows.sort_by_cached_key(|(_, f)| razdor::search::fold(&f.name)),
    }
    rows.into_iter().map(|(g, _)| g).collect()
}

/// The marks the player keeps on the list (`settings.json`), read when the list opens.
#[derive(Clone, Debug, Default)]
struct Marks {
    completed: Vec<String>,
    favourites: Vec<String>,
    show: i64,
    order: i64,
}

thread_local! {
    static MARKS: RefCell<Option<Marks>> = const { RefCell::new(None) };
}

fn marks() -> Marks {
    MARKS.with(|m| {
        m.borrow_mut()
            .get_or_insert_with(|| {
                let s = super::language::Settings::load();
                Marks { completed: s.completed_scenarios, favourites: s.favourite_scenarios, show: s.scenario_show, order: s.scenario_order }
            })
            .clone()
    })
}

/// Keeps `m` and writes it into the settings.
fn keep_marks(m: Marks) {
    let s = super::language::Settings::load();
    super::language::Settings { favourite_scenarios: m.favourites.clone(), scenario_show: m.show, scenario_order: m.order, ..s }.save();
    MARKS.with(|c| *c.borrow_mut() = Some(m));
}

fn listed(names: &[String], file: &str) -> bool {
    names.iter().any(|n| n.to_lowercase() == file.to_lowercase())
}

/// A row's facts: a campaign counts as completed once its last map is won.
fn facts_of(scenarios: &[ScenarioEntry], g: &Group, m: &Marks) -> Facts {
    let e = &scenarios[g.first];
    let last = g.chapters.last().map_or(g.first, |&c| c);
    Facts {
        campaign: !g.chapters.is_empty(),
        original: is_original_map(&e.file),
        completed: listed(&m.completed, &scenarios[last].file),
        favourite: listed(&m.favourites, &e.file),
        width: e.scenario.width(),
        name: group_name(scenarios, g).to_string(),
    }
}

/// The scenario list opens on its first row, scrolled to the top (the original's list
/// starts on row 0 each time). The player's marks are read again (a map won since).
pub fn open_list() {
    EXPERIMENTAL.with(|p| p.set(ExperimentalRules::for_new_game()));
    RULES_OPEN.with(|p| p.set(false));
    PICKED.with(|p| p.set(0));
    PICKED_MAP.with(|p| p.set(None));
    SCROLL.with(|s| s.set(0.0));
    MARKS.with(|m| *m.borrow_mut() = None);
}

/// A five-pointed star centred on `c` (the favourite mark): filled, or its outline.
fn star(c: Vec2, r: f32, color: Color, filled: bool) {
    let p: Vec<Vec2> = (0..10)
        .map(|i| {
            let a = std::f32::consts::PI * (i as f32 / 5.0 - 0.5);
            let rr = if i % 2 == 0 { r } else { r * 0.45 };
            vec2(c.x + rr * a.cos(), c.y + rr * a.sin())
        })
        .collect();
    for i in 0..10 {
        let (a, b) = (p[i], p[(i + 1) % 10]);
        if filled {
            draw_triangle(c, a, b, color);
        } else {
            draw_line(a.x, a.y, b.x, b.y, (r * 0.18).max(1.0), color);
        }
    }
}

/// A tick (the completed mark) in a box of side `s` at `(x, y)`.
fn tick(x: f32, y: f32, s: f32, color: Color) {
    let w = (s * 0.18).max(1.5);
    draw_line(x + s * 0.1, y + s * 0.55, x + s * 0.4, y + s * 0.85, w, color);
    draw_line(x + s * 0.4, y + s * 0.85, x + s * 0.95, y + s * 0.15, w, color);
}

const TICK_GREEN: Color = Color::new(0.45, 0.95, 0.35, 1.0);
const STAR_GOLD: Color = Color::new(1.0, 0.82, 0.25, 1.0);

/// Picks the row of the map whose file name starts with `name` when the list next shows
/// (the `scenarios:<map>` snapshot scene).
pub fn pick_map(name: &str) {
    PICK.with(|p| *p.borrow_mut() = Some(name.to_string()));
}

/// A map's title, or its file name when it has none.
fn title(e: &ScenarioEntry) -> &str {
    if e.scenario.title.trim().is_empty() { e.file.as_str() } else { e.scenario.title.as_str() }
}

/// The name a group's row shows: the campaign's name, else the map's title.
fn group_name<'a>(scenarios: &'a [ScenarioEntry], g: &Group) -> &'a str {
    let e = &scenarios[g.first];
    let campaign = e.scenario.campaign_name.trim();
    if g.chapters.is_empty() || campaign.is_empty() { title(e) } else { campaign }
}

/// The chapters a campaign's row lists under its name. The original lists none for the
/// tutorial, the one campaign named as its first map («Обучающий сценарий»), so a campaign
/// named like its first map lists none here either *(guess, from one screenshot)*.
fn listed_chapters<'a>(scenarios: &[ScenarioEntry], g: &'a Group) -> &'a [usize] {
    let e = &scenarios[g.first];
    if e.scenario.campaign_name.trim() == title(e).trim() { &[] } else { &g.chapters }
}

/// Next always starts the selected campaign at its first scenario. Selecting a chapter only
/// changes the highlighted row; it does not change the campaign entry point.
fn campaign_start(groups: &[Group], picked: usize) -> Option<usize> {
    groups.get(picked).map(|g| g.first)
}

/// How far each line of the description box moves the next one down: the name's lines 20 px,
/// the last with 12 px more under it, the description's 14 px.
fn desc_steps(title_rows: usize, rows: usize, k: f32) -> Vec<f32> {
    (0..rows)
        .map(|n| match n {
            n if n + 1 == title_rows => 32.0 * k,
            n if n < title_rows => 20.0 * k,
            _ => 14.0 * k,
        })
        .collect()
}

/// The first line the description can be scrolled to: the lines from it on fit in `room`.
fn desc_max_scroll(steps: &[f32], room: f32) -> usize {
    let mut left: f32 = steps.iter().sum();
    let mut first = 0;
    while first < steps.len() && left > room {
        left -= steps[first];
        first += 1;
    }
    first
}

/// "Сценарий для Новой Игры".
/// The list's scroll after the wheel turned by `turned` this frame: 30 px a notch. A frame
/// without a turn leaves it (`signum` of 0.0 is 1.0, which pulled the list back to its top
/// every frame, so it could not be scrolled).
fn wheel_scroll(scroll: f32, turned: f32) -> f32 {
    if turned == 0.0 { scroll } else { scroll - turned.signum() * 30.0 }
}

pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool) -> Option<Screen> {
    if RULES_OPEN.with(Cell::get) {
        super::main_menu::backdrop();
        experimental_menu();
        return None;
    }
    if !has_install || chrome::win("Win-marble").is_none() {
        return screens::scenario_select(scenarios, has_install);
    }
    super::main_menu::backdrop();
    let (win, closed) = Win::open(&own("NewGame", "Title", n_("Scenario for a new game")));
    let k = win.k;
    let mut m = marks();
    let (show, order) = (Show::of(m.show), Order::of(m.order));
    let list = arrange(scenario_groups(scenarios), |g| facts_of(scenarios, g, &m), show, order);
    if let Some(name) = PICK.with(|p| p.borrow_mut().take()) {
        if let Some(i) = list.iter().position(|g| std::iter::once(&g.first).chain(&g.chapters).any(|&m| scenarios[m].file.starts_with(&name))) {
            PICKED.with(|p| p.set(i));
            PICKED_MAP.with(|p| p.set(None));
        }
    }
    let mut picked = PICKED.with(|p| p.get()).min(list.len().saturating_sub(1));
    // The list: per group a round icon and the name, and a campaign's chapters under it in
    // small type; the picked group's block is lit. The wheel and the scrollbar scroll it.
    let rows = win.rect(12.0, 36.0, 270.0, 396.0);
    chrome::text_box_base(rows);
    let (head_h, chapter_h) = (30.0 * k, 18.0 * k);
    let block_h = |g: &Group| head_h + listed_chapters(scenarios, g).len() as f32 * chapter_h + if listed_chapters(scenarios, g).is_empty() { 0.0 } else { 4.0 * k };
    let content_h: f32 = list.iter().map(block_h).sum::<f32>() + 12.0 * k;
    let max_scroll = (content_h - rows.h).max(0.0) / k;
    let mut scroll = SCROLL.with(|s| s.get());
    let over_list = !input_blocked() && rows.contains(crate::ui::widgets::pointer().into());
    if over_list {
        scroll = wheel_scroll(scroll, wheel());
    }
    scroll = scroll.clamp(0.0, max_scroll);
    // Razdor's scrollbar on the right when the list does not all fit (a touch screen has no
    // wheel); the rows give it room.
    let barred = max_scroll > 0.0;
    scrollbar_px(Rect::new(rows.x + rows.w - 16.0 * k, rows.y + 4.0 * k, 12.0 * k, rows.h - 8.0 * k), &mut scroll, max_scroll, rows.h / k, 15.0);
    SCROLL.with(|s| s.set(scroll));
    let row_w = rows.w - if barred { 22.0 * k } else { 8.0 * k };
    chrome::text_box_decoration(rows);
    let inside = Rect::new(rows.x, rows.y + 2.0 * k, rows.w, rows.h - 4.0 * k);
    let mut selection_y = rows.y + 6.0 * k - scroll * k;
    for (row, g) in list.iter().enumerate() {
        let r = Rect::new(rows.x + 4.0 * k, selection_y, row_w, block_h(g) - 2.0 * k);
        selection_y += block_h(g);
        if row == PICKED.with(|p| p.get()).min(list.len().saturating_sub(1)) && r.y < inside.y + inside.h && r.y + r.h > inside.y {
            picked_bar(r);
        }
    }
    let mut next = None;
    let mut y = rows.y + 6.0 * k - scroll * k;
    // The rows are clipped to the box, so a block only partly in view shows its part (a
    // campaign taller than the room left was hidden, with the gap it left).
    let clip = crate::ui::widgets::Clip::new(inside);
    for (row, g) in list.iter().enumerate() {
        let r = Rect::new(rows.x + 4.0 * k, y, row_w, block_h(g) - 2.0 * k);
        y += block_h(g);
        if r.y >= inside.y + inside.h || r.y + r.h <= inside.y {
            continue;
        }
        let e = &scenarios[g.first];
        let chapters = listed_chapters(scenarios, g);
        let head = Rect::new(r.x, r.y, r.w, head_h - 2.0 * k);
        // Razdor's marks at the head's right end: the favourite star (shown on the row under
        // the pointer, lit when marked; a click on it marks or unmarks) and the tick of a
        // scenario won (a campaign: its last map).
        let star_r = Rect::new(head.x + head.w - 22.0 * k, head.y + head.h / 2.0 - 9.0 * k, 18.0 * k, 18.0 * k);
        let on_star = over_list && star_r.contains(crate::ui::widgets::pointer().into());
        let (row_hover, _, _) = button_state(r, over_list && !on_star);
        let head_pressed = head.contains(pointer().into()) && clicked() && over_list && !on_star;
        let (_, _, star_pressed) = button_state(star_r, over_list);
        let f = facts_of(scenarios, g, &m);
        if f.favourite || row_hover || on_star {
            let ink = if f.favourite { STAR_GOLD } else { Color::new(0.85, 0.85, 0.85, if on_star { 0.9 } else { 0.45 }) };
            star(star_r.center(), 8.0 * k, ink, f.favourite);
        }
        if star_pressed {
            let file = e.file.clone();
            if f.favourite {
                m.favourites.retain(|n| n.to_lowercase() != file.to_lowercase());
            } else {
                m.favourites.push(file);
            }
            keep_marks(m.clone());
        }
        if f.completed {
            tick(head.x + head.w - 42.0 * k, head.y + head.h / 2.0 - 7.0 * k, 14.0 * k, TICK_GREEN);
        }
        scenario_icon(e.scenario.header.scenario_picture_index, vec2(head.x + 14.0 * k, head.y + head.h / 2.0), k);
        let name = group_name(scenarios, g);
        // The name's Benguiat variant (0x4e266e..0x4e27a3, colours from 0x4dbb0c): the tutorial
        // ae249c (green), a campaign ae24a0 (yellow), a single scenario ae24ac (white with
        // 120 taken off red: cyan).
        // The picked item's lines take their highlight fonts (0x47d71c): the name the white
        // Benguiat ae2498, the chapters the plain white SanSerif ae24bc (else ae24c4, orange).
        let lit = row == picked;
        let name_ink = if lit {
            super::typography::Ink::Plain.color()
        } else if e.scenario.header.scenario_picture_index == 5 {
            super::typography::Ink::Green.color()
        } else if !chapters.is_empty() {
            super::typography::Ink::Yellow.color()
        } else {
            // Single scenarios use the original list's cyan, rather than the blue markup tint.
            Color::from_rgba(90, 231, 255, 255)
        };
        // The name leaves room for the marks it has (none: the original's width).
        let name_w = r.w - (40.0 + if f.completed { 44.0 } else if f.favourite { 24.0 } else { 0.0 }) * k;
        with_face(Face::Subtitle, || {
            let size = fit_size(name, name_w, 15.0 * k);
            chrome::shadow_text(&ellipsize(name, name_w, size), head.x + 32.0 * k, head.y + head.h * 0.5 + size * 0.36, size, name_ink);
        });
        let small = 12.0 * k;
        with_face(Face::Body, || for (n, &c) in chapters.iter().enumerate() {
            let cy = r.y + head_h + n as f32 * chapter_h + chapter_h * 0.5 + small * 0.36;
            let ink = if lit { super::typography::Ink::Plain } else { super::typography::Ink::Orange };
            // Each map of a campaign won has its tick too.
            let won = listed(&m.completed, &scenarios[c].file);
            let room = r.w - (if won { 84.0 } else { 60.0 }) * k;
            chrome::shadow_text(&ellipsize(title(&scenarios[c]), room, small), r.x + 52.0 * k, cy, small, ink.color());
            if won {
                tick(head.x + head.w - 40.0 * k, cy - small * 0.36 - 5.0 * k, 10.0 * k, TICK_GREEN);
            }
        });
        // A campaign heading or individual chapter is selected only on click.
        if head_pressed {
            if row == picked && PICKED_MAP.with(|p| p.get()).is_none() {
                next = Some(Screen::ClassSelect { scenario: Some(g.first) });
            }
            picked = row;
            PICKED_MAP.with(|p| p.set(None));
        } else {
            for (n, &chapter_index) in chapters.iter().enumerate() {
                let chapter = Rect::new(r.x + 28.0 * k, r.y + head_h + n as f32 * chapter_h, r.w - 28.0 * k, chapter_h);
                if over_list && chapter.contains(crate::ui::widgets::pointer().into()) && clicked() {
                    if PICKED_MAP.with(|p| p.get()) == Some(chapter_index) {
                        next = Some(Screen::ClassSelect { scenario: Some(g.first) });
                    }
                    picked = row;
                    PICKED_MAP.with(|p| p.set(Some(chapter_index)));
                    break;
                }
            }
        }
    }
    drop(clip);
    if list.is_empty() {
        with_face(Face::Body, || {
            let size = 13.0 * k;
            let t = tr("No scenario fits this choice.");
            chrome::shadow_text(&ellipsize(t, rows.w - 24.0 * k, size), rows.x + 12.0 * k, rows.y + 24.0 * k, size, super::typography::Ink::Orange.color());
        });
    }
    PICKED.with(|p| p.set(picked));
    // The map and what it is.
    if let Some(g) = list.get(picked) {
        let (i, e) = (g.first, &scenarios[g.first]);
        // (Razdor's scenario window draws the frame at the interface scale, 295 × 162 of its
        // own pixels, the picture at the same scale so the two stay in register.)
        let (frame, inner) = map_frame_rects(win.rect(290.0, 36.0, 0.0, 0.0).point(), k);
        // The original copies MapImageFrame (295×162) as it is, then the map's own 267×134
        // picture at its size 14 px in, over the frame's opening (0x4c1c09..0x4c1c63); no
        // line is drawn round either. Without a picture Razdor shows the terrain there.
        match chrome::win("MapImageFrame") {
            Some(t) => chrome::tex(&t, frame, WHITE),
            None => draw_rectangle(frame.x, frame.y, frame.w, frame.h, BLACK),
        }
        let pic = picture(i, e);
        match (&pic, preview(i, e)) {
            (Some(p), _) => chrome::tex(p, inner, WHITE),
            (None, Some(t)) => {
                // Square maps, centred in the opening.
                let side = inner.w.min(inner.h);
                chrome::tex(&t, Rect::new(inner.center().x - side / 2.0, inner.y + (inner.h - side) / 2.0, side, side), WHITE);
            }
            (None, None) => {}
        }
        // Under it the description box (0x4d2a30, 65ae00): the name centred in white (ae2498,
        // 0x4c1b4b), the description in the bold SanSerif's orange (ae24f4 over ae2510,
        // 0x4c1bb4), from the box's top.
        let bottom = win.rect(0.0, 432.0, 0.0, 0.0).y;
        let desc = Rect::new(frame.x, frame.y + frame.h + 8.0 * k, frame.w, (bottom - frame.y - frame.h - 8.0 * k).max(0.0));
        chrome::text_box(desc);
        let size = 12.0 * k;
        let room = desc.w - 34.0 * k;
        let title_text = format!("^*{}", group_name(scenarios, g).replace(['^', '*', '|', '@'], ""));
        let mut rows = markup_rows(&title_text, room, 16.0 * k);
        let title_rows = rows.len();
        // The description is added as plain lines (0x4c1bb4 → 0x47ea80): marks stay as typed,
        // no indent, justified, in ae24f4 over the shadow font ae2510.
        rows.extend(plain_rows_face(&e.scenario.description, room, size, Face::Bold));
        let line_h = 14.0 * k;
        let steps = desc_steps(title_rows, rows.len(), k);
        // The box is a text list (0x65ae00): the wheel over it scrolls it a line a frame
        // (0x47ede0), down to its last line; a new map starts it at the top (0x47cc90).
        let max_scroll = desc_max_scroll(&steps, desc.h - 20.0 * k);
        let (for_group, mut first) = DESC_SCROLL.with(|d| d.get());
        if for_group != picked {
            first = 0;
        }
        if !input_blocked() && desc.contains(crate::ui::widgets::pointer().into()) {
            let turned = wheel();
            if turned > 0.0 {
                first = first.saturating_sub(1);
            } else if turned < 0.0 {
                first += 1;
            }
        }
        let first = first.min(max_scroll);
        DESC_SCROLL.with(|d| d.set((picked, first)));
        // A text list lays its lines from its top (0x47e46c).
        let mut y = desc.y + 10.0 * k + 14.0 * k;
        let clip = crate::ui::widgets::Clip::new(desc);
        for (n, row) in rows.iter().enumerate() {
            if n < first {
                continue;
            }
            if n < title_rows {
                // The name in ae2498 over its shadow font ae24b0 (0x4c1b33).
                draw_markup_row_shadowed(row, desc.x + 17.0 * k, y, room, 16.0 * k, Face::Subtitle, super::typography::Ink::Plain.color());
                y += 20.0 * k;
                if n + 1 == title_rows {
                    y += 12.0 * k;
                }
            } else {
                let orange = with_face(Face::Bold, || super::typography::Ink::Orange.color());
                with_face(Face::Bold, || draw_markup_row_shadowed(row, desc.x + 17.0 * k, y, room, size, Face::Bold, orange));
                y += line_h;
            }
        }
        drop(clip);
    }
    // Razdor's filter and order (a players' wish), under the list where the original's window
    // has nothing: each opens a drop-down list; the choice is kept in the settings.
    let pick_show = view_choice(&win, 12.0, "razdor:scenario-show", show as i64, &Show::ALL.map(|s| s.label()));
    let pick_order = view_choice(&win, 158.0, "razdor:scenario-order", order as i64, &Order::ALL.map(|o| o.label()));
    if pick_show.is_some() || pick_order.is_some() {
        m.show = pick_show.unwrap_or(m.show);
        m.order = pick_order.unwrap_or(m.order);
        keep_marks(m);
        PICKED.with(|p| p.set(0));
        PICKED_MAP.with(|p| p.set(None));
        SCROLL.with(|s| s.set(0.0));
    }
    let can = !list.is_empty();
    if experimental_rules() != ExperimentalRules::default() {
        let label=win.rect(12.0,470.0,306.0,12.0);
        chrome::shadow_text(tr("Experimental rules enabled"),label.x,label.y+9.0*k,9.0*k,chrome::ORANGE_TEXT);
    }
    if win.button(328.0, 34.0, "...", can) {
        RULES_OPEN.with(|p| p.set(true));
        return None;
    }
    let selected_map = campaign_start(&list, picked);
    if (win.button(369.0, 116.0, &own("Buttons", "Next", n_("Next")), can) || (can && key(KeyCode::Enter))) && next.is_none() {
        cue(Cue::Button);
        next = selected_map.map(|i| Screen::ClassSelect { scenario: Some(i) });
    }
    if win.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed || key(KeyCode::Escape) {
        return Some(Screen::MainMenu);
    }
    next
}

pub(super) fn experimental_rules() -> ExperimentalRules { EXPERIMENTAL.with(Cell::get) }

pub(super) fn open_experimental_menu() { RULES_OPEN.with(|p| p.set(true)); }

pub(super) fn snapshot_experimental(rules: ExperimentalRules) { EXPERIMENTAL.with(|p| p.set(rules)); }

/// Kept modal so clicks and Enter cannot start a scenario behind the options.
fn experimental_menu() {
    let k = chrome::k();
    let r = Rect::new((screen_width() - 550.0*k)/2.0, (screen_height() - 360.0*k)/2.0, 550.0*k, 360.0*k);
    let (_, close) = chrome::window(r, tr("Experimental scenario rules"), chrome::Skin::Marble, true);
    let mut rules = experimental_rules();
    let check = |index: usize, label: &str, value: &mut bool| {
        let row = Rect::new(r.x+20.0*k, r.y+(54.0+index as f32*38.0)*k, r.w-40.0*k, 28.0*k);
        let box_r = Rect::new(row.x, row.y+3.0*k, 20.0*k, 20.0*k);
        chrome::silver_frame(box_r, k.max(1.0));
        if *value {
            draw_line(box_r.x+4.0*k,box_r.y+10.0*k,box_r.x+8.0*k,box_r.y+15.0*k,2.0*k,chrome::GOLD);
            draw_line(box_r.x+8.0*k,box_r.y+15.0*k,box_r.x+17.0*k,box_r.y+5.0*k,2.0*k,chrome::GOLD);
        }
        chrome::shadow_text(label,row.x+30.0*k,row.y+20.0*k,16.0*k,chrome::CREAM);
        if row.contains(pointer().into()) && clicked() { *value = !*value; }
    };
    check(0,tr("All special bonuses simultaneously"),&mut rules.simultaneous_bonuses);
    check(1,tr("Allow multiple rings"),&mut rules.multiple_rings);
    check(2,tr("Allow multiple amulets"),&mut rules.multiple_amulets);
    check(3,tr("No duplicate items"),&mut rules.no_duplicate_items);
    check(4,tr("At most two potions per character"),&mut rules.two_potions);
    EXPERIMENTAL.with(|p| p.set(rules));
    // Six cells with the multiple rings or amulets rule, else the original's four.
    let slots = if rules.multiple_rings || rules.multiple_amulets {
        tr("Six equipment slots. Duplicate abilities do not stack.")
    } else {
        tr("Four equipment slots. Duplicate abilities do not stack.")
    };
    let lines = [slots, tr("Applies to all armies; saved with this playthrough."), tr("The potion limit resets after a battle.")];
    for (i,line) in lines.iter().enumerate() {
        chrome::shadow_text(line,r.x+20.0*k,r.y+(264.0+i as f32*19.0)*k,12.0*k,chrome::CREAM);
    }
    let b = Rect::new(r.center().x-50.0*k,r.bottom()-44.0*k,100.0*k,28.0*k);
    let (_, down, pressed) = button_state(b,true);
    chrome::marble_button(b,tr("OK"),true,down);
    if close || pressed || key(KeyCode::Escape) || key(KeyCode::Enter) { RULES_OPEN.with(|p| p.set(false)); }
}

/// A marble button at `x` (window pixels) on the buttons' line showing the choice `current`
/// of `labels`, with a small arrow; a click opens the drop-down list `key`. The value picked.
fn view_choice(win: &Win, x: f32, key: &str, current: i64, labels: &[&str]) -> Option<i64> {
    let picked = take_pick(key);
    let r = win.rect(x, 442.0, 140.0, 28.0);
    let (_, down, pressed) = button_state(r, true);
    let label = labels.get(current as usize).copied().unwrap_or_default();
    chrome::marble_button(r, label, true, down || list_open(key));
    let (ax, ay) = (r.x + r.w - 14.0 * win.k, r.y + r.h / 2.0);
    let s = 4.0 * win.k;
    draw_triangle(vec2(ax - s, ay - s * 0.6), vec2(ax + s, ay - s * 0.6), vec2(ax, ay + s * 0.8), Color::new(0.85, 0.85, 0.8, 1.0));
    if pressed {
        cue(Cue::Button);
        let options: Vec<(i64, String)> = labels.iter().enumerate().map(|(i, l)| (i as i64, l.to_string())).collect();
        super::widgets::open_list(key, r, current, &options, win.k);
    }
    picked.filter(|&v| v != current)
}

/// The tutorial map to offer before a new game, as an index into `scenarios`: the install's
/// `[Tutorial] Tutorial_MapName` (Обучающий1), at every new game while the tutorial is not
/// done (0x4e2970): the install says so (`Completed=1`, written by the original), Razdor's
/// settings say so (the tutorial's last event finished), or any save exists.
pub fn tutorial_map(scenarios: &[ScenarioEntry]) -> Option<usize> {
    if chrome::win("Win-marble").is_none() || chrome::ui_text("Tutorial", "Completed").is_some_and(|c| razdor::dt::ini::loose_int(&c) == 1) {
        return None;
    }
    if super::language::Settings::load().tutorial_completed {
        return None;
    }
    let saved = razdor::rules::save::games_dir().is_some_and(|d| {
        use razdor::rules::save::{list, SaveKind};
        !list(&d, SaveKind::Manual).is_empty() || !list(&d, SaveKind::Auto).is_empty()
    });
    if saved {
        return None;
    }
    let name = chrome::ui_text("Tutorial", "Tutorial_MapName")?;
    scenarios.iter().position(|e| e.file.eq_ignore_ascii_case(name.trim()) || e.file == name.trim())
}

/// "Обучающий сценарий", before the first new game: the install's picture (`Как Играть`) on
/// the left and its text on the right. «Да» starts the tutorial map (the hero choice next),
/// «Нет» opens the scenario list; it comes again at the next new game until the tutorial is
/// done.
pub fn tutorial_offer(scenarios: &[ScenarioEntry]) -> Option<Screen> {
    let Some(map) = tutorial_map(scenarios) else { return Some(Screen::ScenarioSelect) };
    super::main_menu::backdrop();
    let (win, closed) = Win::open(&own("Tutorial", "Title", n_("Tutorial scenario")));
    let k = win.k;
    let pic = win.rect(18.0, 44.0, 152.0, 386.0);
    let picture = chrome::ui_text("Tutorial", "Picture")
        .and_then(|p| p.rsplit(['\\', '/']).next().map(|f| f.trim_end_matches(".lit").trim_end_matches(".LIT").to_string()))
        .and_then(|name| chrome::win(&name));
    match picture {
        Some(t) => {
            // The whole picture, as tall as the box allows, centred.
            let h = pic.h.min(t.height() * pic.w / t.width());
            let w = t.width() * h / t.height();
            chrome::tex(&t, Rect::new(pic.x + (pic.w - w) / 2.0, pic.y + (pic.h - h) / 2.0, w, h), WHITE);
        }
        None => chrome::text_box(pic),
    }
    let text = win.rect(182.0, 44.0, 396.0, 386.0);
    chrome::text_box(text);
    let size = (14.0 * k).round();
    let (x0, width) = (text.x + 20.0 * k, text.w - 40.0 * k);
    let mut y = text.y + 26.0 * k;
    let body = own("Tutorial", "Text", n_("Welcome to the world of \"A Time of Discord\"!\n\nThe tutorial scenario shows you the game's interface and how to play it with each of the heroes.\n\n\n\nPress \"No\" to open the list of all scenarios and campaigns.\n\nPress \"Yes\" to start the tutorial and choose your hero."));
    // The original asks this in the event window (0x4ac748 makes it an event's question,
    // 0x4a8ae8 shows it), so its text is markup (0x48e438): each line in its font, justified
    // behind the indent or centred. `ui_text` gave the `#\` breaks as new lines.
    for row in markup_rows(&body.replace('\n', "#\\"), width, size) {
        if y > text.y + text.h - 8.0 * k {
            break;
        }
        draw_markup_row(&row, x0, y, width, size);
        y += size * 1.3;
    }
    let yes = win.button(92.0, 116.0, &own("Buttons", "Yes", n_("Yes")), true) || key(KeyCode::Enter) || key(KeyCode::Y);
    let no = win.button(386.0, 116.0, &own("Buttons", "No", n_("No")), true) || key(KeyCode::N) || key(KeyCode::Escape) || closed;
    if yes || no {
        cue(Cue::MenuPress);
        return Some(if yes { Screen::ClassSelect { scenario: Some(map) } } else { Screen::ScenarioSelect });
    }
    None
}

/// "Стартовые характеристики Героя".
pub fn class_select(game: &mut Option<Game>, demo: &Arc<Content>, scenario: Option<(&ScenarioEntry, Arc<Content>)>, assets: &Assets) -> Option<Screen> {
    if scenario.is_none() || chrome::win("Win-marble").is_none() {
        return screens::class_select(game, demo, scenario, assets);
    }
    let (e, content) = scenario.expect("checked");
    let content = Arc::new(content.with_experimental(experimental_rules()));
    super::main_menu::backdrop();
    let scale = chrome::k();
    let r = Rect::new((screen_width() - 632.0 * scale) / 2.0, (202.0 * scale).min(screen_height() - 516.0 * scale), 632.0 * scale, 516.0 * scale);
    let title = own("NewHero", "Title", n_("The hero's starting characteristics"));
    let (_, closed) = chrome::window(r, &title, chrome::Skin::Marble, true);
    let win = Win { r, k: scale };
    let k = win.k;
    // As the original (0x4c1804, 0x4743c8): only a class whose preset has a start cell is
    // offered; the window opens on the first one, or is not opened at all when there is
    // none. A greyed portrait takes no click, so a class the map leaves out cannot be
    // started (there is no key for it either).
    let offered = e.scenario.header.offered_classes();
    if !HERO_OPEN.with(|o| o.replace(true)) {
        fill_name_field();
        match e.scenario.header.first_offered_class() {
            Some(c) => HERO.with(|h| h.set(c)),
            None => return leave_hero_window(Screen::MainMenu),
        }
    }
    let mut pick = HERO.with(|h| h.get()).min(2);
    // The three heroes (0x4d7d30, 0x4c1458): the portraits 160 px square, 37 apart, 43 under
    // the window's top. The picked one is drawn from the brightened picture; another offered
    // class from its blue copy, a class the map leaves out from its dark red one (65bcd8 /
    // 65bce4; how they are made: `chrome::hero_portrait`, 0x4d7d30). Under each the window's
    // silver chain; under the picked one the name field instead.
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let r = win.rect(38.0 + i as f32 * 197.0, 42.0, 160.0, 160.0);
        let (hover, down, pressed) = button_state(r, offered[i]);
        let look = if !offered[i] {
            chrome::Fx::HeroLocked
        } else if i == pick {
            chrome::Fx::HeroPicked
        } else {
            chrome::Fx::HeroOffered
        };
        match chrome::win_fx(["Hero0", "hero1", "hero2"][i], look) {
            Some(t) => chrome::tex(&t, r, WHITE),
            None => assets.draw_portrait(hero.unit(), razdor::rules::battle::Team::Player, r),
        }
        chrome::silver_frame(r, 1.0);
        if down || (hover && i != pick) { draw_rectangle_lines(r.x, r.y, r.w, r.h, k, chrome::SILVER); }
        if i != pick {
            if let Some(t) = chrome::win("nameframe") { chrome::tex(&t, win.rect(38.0 + i as f32 * 197.0, 207.0, 160.0, 22.0), WHITE); }
        }
        if pressed {
            // Choosing another class plays the menu press (interface.md §14); the class
            // already picked, nothing.
            let was = pick;
            pick = choose_class(offered, pick, i);
            if pick != was {
                cue(Cue::MenuPress);
            }
        }
    }
    HERO.with(|h| h.set(pick));
    let hero = HeroClass::ALL[pick];
    // The hero's name, typed (centred, as the original draws its edit box's text, 0x47a624).
    let field = win.rect(38.0 + pick as f32 * 197.0, 207.0, 160.0, 22.0);
    draw_rectangle(field.x, field.y, field.w, field.h, BLACK);
    chrome::silver_frame(field, 1.0);
    let name = with_face(Face::Subtitle, || {
        let size = super::typography::Role::Name.size(16.0 * k);
        let look = LineLook { size, color: super::typography::Ink::Yellow.color(), centred: true, shadow: true };
        let room = Rect::new(field.x + 4.0 * k, field.y, field.w - 8.0 * k, field.h);
        screens::hero_name_field(room, field.center().y + size * 0.36, look)
    });
    let book = win.rect(18.0, 240.0, 596.0, 210.0);
    match chrome::win("NewHero_Paper") {
        Some(t) => chrome::tex(&t, book, WHITE),
        None => chrome::text_box(book),
    }
    let n = pick + 1;
    let small = 12.0 * k;
    let page_w = 260.0 * k;
    let (left, right) = (book.x + 28.0 * k, book.x + 314.0 * k);
    let unit = &content.unit(hero.unit());
    let ours = [
        n_("A veteran of many cruel battles, the Knight has superb experience of war. Under the Church's protection he can withstand hostile spells."),
        n_("The Archmage can call on the mighty forces of the world that ordinary people cannot command. In battle he binds the enemy."),
        n_("The Ranger is a fast and accurate shot who finds his way anywhere. Watching nature, he has learnt the secrets of healing wounds fast."),
    ];
    let descript = own("NewHero", &format!("Descript{n}"), ours[pick]);
    let bonus_ours = [
        n_("The army of this hero takes 10% less damage from enemy attacks (magic excepted)."),
        n_("The Archmage casts spells twice as fast for 50% less mana, but his army gets no bonuses."),
        n_("The army of this hero travels 20% faster, and the wounded heal 20% of their hits every day."),
    ];
    let bonus = own("NewHero", &format!("Bonus{n}"), bonus_ours[pick]);
    // The left page, centred in its height: the name centred, the description justified,
    // then the bonus beside its icon.
    let icon = 22.0 * k;
    let desc_rows = wrap(&descript, page_w, small);
    let bonus_rows = wrap(&bonus, page_w - icon - 6.0 * k, small);
    let line_h = 14.0 * k;
    let total = 20.0 * k + desc_rows.len() as f32 * line_h + 6.0 * k + bonus_rows.len().max(2) as f32 * line_h;
    let mut y = book.y + ((book.h - total) / 2.0).max(10.0 * k) + 12.0 * k;
    with_face(Face::Subtitle, || chrome::shadow_centered(&unit.name, left + page_w / 2.0, y, 15.0 * k, WHITE));
    y += 20.0 * k;
    for row in &desc_rows {
        chrome::shadow_text(row, left, y, small, DESC_INK);
        y += line_h;
    }
    y += 6.0 * k;
    chrome::trait_icon(&format!("HeroBonus{n}"), left, y - small, icon);
    for (j, row) in bonus_rows.iter().enumerate() {
        chrome::shadow_text(row, left + icon + 6.0 * k, y + j as f32 * line_h, small, chrome::BLUE_TEXT);
    }
    // The starting characteristics (0x4c1334 → 0x491fa4); labels and values in their inks.
    let rows = hero_stats(&content, hero);
    with_face(Face::Subtitle, || {
        let size = super::typography::Role::Name.size(16.0 * k);
        for (j, (label, value)) in rows.iter().enumerate() {
            let y = book.y + 50.0 * k + j as f32 * 18.0 * k;
            chrome::shadow_text(&ellipsize(label, 225.0 * k, size), right, y, size, super::typography::Ink::Yellow.color());
            chrome::shadow_right(value, book.right() - 24.0 * k, y, size, super::typography::Ink::Plain.color());
        }
    });
    draw_line(win.r.x, win.r.y + 460.0 * k, win.r.right(), win.r.y + 460.0 * k, k, chrome::SILVER);
    let hero_button = |x, label: &str| {
        let r = win.rect(x, 470.0, 94.0, 34.0);
        let (_, down, pressed) = button_state(r, true);
        chrome::marble_button(r, label, true, down);
        pressed
    };
    if hero_button(424.0, &own("NewHero", "Start", n_("Start"))) || key(KeyCode::Enter) {
        cue(Cue::Button);
        *game = Some(screens::start_game(demo, Some((e, &content)), hero, &name));
        return leave_hero_window(Screen::WorldMap);
    }
    // Back or Esc returns to the main menu, not to the scenario list (0x4c0fd4).
    if hero_button(528.0, &own("Buttons", "Cancel", n_("Cancel"))) || closed || key(KeyCode::Escape) {
        return leave_hero_window(Screen::MainMenu);
    }
    None
}

/// The class description: the ae24c0 tint, a light yellow.
const DESC_INK: Color = Color::new(1.0, 0.86, 0.55, 1.0);

/// A label of the unit panel (`[Skills] key` of the install, else ours).
fn skill(key: &str, ours: &'static str) -> String {
    own("Skills", key, ours)
}

/// The starting characteristics of `hero` as the hero window lists them (0x4c1334 →
/// 0x491fa4): the hits, the melee attack, the melee defence, the ranged attack, the ranged
/// defence, the magic power of his school, one "immunity to magic" line when the three
/// protections are equal (else the three of life, death and the elements), the initiative
/// and the actions. A value of 0 is left out, the hits never.
fn hero_stats(content: &Content, hero: HeroClass) -> Vec<(String, String)> {
    use razdor::rules::content::{MagicSchool, Stat};
    let s = razdor::rules::units::Stats::of_level(content, hero.unit(), 1);
    let mut rows = vec![(skill("SHit", n_("Hits")), s.max_hp().to_string())];
    for (st, key, ours) in [
        (Stat::AttackBlow, "SAttackBlow", n_("Melee attack")),
        (Stat::DefenceBlow, "SDefenceBlow", n_("Melee defence")),
        (Stat::AttackShot, "SAttackShot", n_("Ranged attack")),
        (Stat::DefenceShot, "SDefenceShot", n_("Ranged defence")),
    ] {
        if s[st] > 0 {
            rows.push((skill(key, ours), s[st].to_string()));
        }
    }
    if let Some(school) = s.magic.filter(|_| s[Stat::MagicPower] > 0) {
        let (key, ours) = match school {
            MagicSchool::Life => ("SMagicLife", n_("Life magic power")),
            MagicSchool::Death => ("SMagicDeath", n_("Death magic power")),
            MagicSchool::Elemental => ("SMagicElemental", n_("Elemental magic power")),
        };
        rows.push((skill(key, ours), s[Stat::MagicPower].to_string()));
    }
    let (life, elemental, death) = (s[Stat::ProtectLife], s[Stat::ProtectElemental], s[Stat::ProtectDeath]);
    if life == elemental && elemental == death && death > 0 {
        rows.push((skill("SProtectAllMagic", n_("Immunity to magic")), format!("{death}%")));
    } else {
        for (v, key, ours) in [
            (life, "SProtectLife", n_("Life magic protection")),
            (death, "SProtectDeath", n_("Death magic protection")),
            (elemental, "SProtectElemental", n_("Elemental magic protection")),
        ] {
            if v > 0 {
                rows.push((skill(key, ours), format!("{v}%")));
            }
        }
    }
    rows.push((skill("SInitiative", n_("Initiative")), s[Stat::Initiative].to_string()));
    rows.push((skill("SManevres", n_("Actions")), s[Stat::Manevres].to_string()));
    rows
}

/// Closes the hero window: the next opening starts again on the map's first offered class.
fn leave_hero_window(next: Screen) -> Option<Screen> {
    HERO_OPEN.with(|o| o.set(false));
    Some(next)
}

/// Fills the name field as the original does on every opening of the window (0x4cff70):
/// with `[NewHero] PrivateHeroName` as text, not as a hint. Start takes the field as it
/// stands (0x4c10d9), so a name left untouched is that text («(нет имени)», or the name a
/// mod writes there). As the original, the caret stands at its end and nothing is selected:
/// its edit box (0x47ae5c) draws the text and the caret in the field's yellow, with no
/// selection band (a player's report; it once opened with the text selected, in blue).
fn fill_name_field() {
    let name = own("NewHero", "PrivateHeroName", n_("(no name)"));
    screens::HERO_EDIT.with(|e| e.set(super::text_edit::TextEdit::at_end()));
    screens::HERO_NAME.with(|n| *n.borrow_mut() = name);
}

/// The class picked after a click on portrait `clicked`: a class the map does not offer
/// keeps the current pick (the original's hit test skips a disabled portrait, 0x4743c8).
fn choose_class(offered: [bool; 3], pick: usize, clicked: usize) -> usize {
    if offered[clicked] { clicked } else { pick }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The map frame and its picture at one scale (0x4c1c09..0x4c1c63; issue #17): the 267×134
    /// picture 14 px in fills the 295×162 frame's opening with the same 14 px all round.
    #[test]
    fn the_map_picture_sits_14_px_inside_its_frame() {
        for px in [1.0, 1.0667, 1.5] {
            let (frame, inner) = map_frame_rects(vec2(10.0, 20.0), px);
            assert_eq!((frame.w, frame.h), (295.0 * px, 162.0 * px));
            assert_eq!((inner.x - frame.x, inner.y - frame.y), (14.0 * px, 14.0 * px));
            assert!((frame.right() - inner.right() - 14.0 * px).abs() < 1e-3);
            assert!((frame.bottom() - inner.bottom() - 14.0 * px).abs() < 1e-3);
        }
    }

    /// A description longer than its box scrolls (a player's report: "нельзя прокрутить
    /// описание"): the original's box is a text list the wheel scrolls (0x47ede0) down to the
    /// first line from which the rest fits.
    #[test]
    fn a_long_description_scrolls_until_its_end_is_in_the_box() {
        // Two name lines (20, then 20 + 12) and ten description lines of 14.
        let steps = desc_steps(2, 12, 1.0);
        assert_eq!(&steps[..3], &[20.0, 32.0, 14.0]);
        assert_eq!(steps.iter().sum::<f32>(), 192.0);
        assert_eq!(desc_max_scroll(&steps, 200.0), 0, "it fits: no scrolling");
        assert_eq!(desc_max_scroll(&steps, 140.0), 2, "the name scrolls away first");
        assert_eq!(desc_max_scroll(&steps, 100.0), 5);
    }

    #[test]
    fn scenario_icons_keep_their_own_size_without_a_ring() {
        // 0x4c1968 puts the picture into the cell as it is: a 27×25 icon at k = 2 is 54×50,
        // centred (Razdor drew it shrunk to 76 % inside a 26 px ring before).
        let r = icon_rect(vec2(27.0, 25.0), vec2(100.0, 50.0), 2.0);
        assert_eq!((r.x, r.y, r.w, r.h), (73.0, 25.0, 54.0, 50.0));
        let src = include_str!("new_game.rs");
        let body = &src[src.find("fn scenario_icon(").unwrap()..src.find("fn icon_rect(").unwrap()];
        assert!(!body.contains("win_fx(\"si-border\"") && !body.contains("draw_circle"), "no ring and no disc under the icon");
    }

    #[test]
    fn the_scenario_list_keeps_its_scroll_between_turns_of_the_wheel() {
        assert_eq!(wheel_scroll(60.0, 0.0), 60.0, "no turn, no move");
        assert_eq!(wheel_scroll(60.0, -1.0), 90.0, "down a notch");
        assert_eq!(wheel_scroll(60.0, 2.5), 30.0, "up a notch, whatever the wheel's step");
    }

    #[test]
    fn the_name_field_opens_with_the_install_s_no_name_as_text() {
        screens::HERO_NAME.with(|n| *n.borrow_mut() = "Иво".to_string());
        fill_name_field();
        let name = screens::HERO_NAME.with(|n| n.borrow().clone());
        assert_eq!(name, own("NewHero", "PrivateHeroName", n_("(no name)")), "the last game's name is gone; the field holds the text itself");
        assert!(!name.is_empty(), "a name left untouched is that text, not an empty one");
        let mut ed = screens::HERO_EDIT.with(|e| e.get());
        ed.fit(&name);
        assert!(!ed.has_selection(), "nothing selected, no blue band (0x47ae5c)");
        assert_eq!(ed.caret, name.chars().count(), "the caret after the text");
        let mut typed = name.clone();
        assert!(ed.type_chars(&mut typed, &['И'], 24));
        assert_eq!(typed, format!("{name}И"), "a key typed goes after the default name, as the original");
    }

    /// The list's order (0x4e21d8): the tutorial first, then by map width; one size keeps
    /// the file order.
    #[test]
    fn the_scenario_list_goes_tutorial_first_then_by_map_size() {
        let maps = [("Другой берег", 200), ("Обучающий1", 50), ("Проклятое озеро", 100), ("РК1-Начало пути", 50), ("Тихая пристань", 50)];
        let mut order: Vec<&str> = maps.iter().map(|m| m.0).collect();
        order.sort_by_key(|f| list_key(f, maps.iter().find(|m| m.0 == *f).unwrap().1, "Обучающий1"));
        assert_eq!(order, ["Обучающий1", "РК1-Начало пути", "Тихая пристань", "Проклятое озеро", "Другой берег"]);
        assert_eq!(list_key("обучающий1", 50, "Обучающий1"), 0, "the name is found whatever its case");
        assert_eq!(list_key("Обучающий1", 50, ""), 50, "no tutorial named: by size");
    }

    #[test]
    fn the_list_filters_and_sorts_as_chosen() {
        // Rows in the original's order: a 50 single, a campaign, a 200 fan map, a 100 single.
        let g = |first: usize, campaign: bool| Group { first, chapters: if campaign { vec![first, first + 10] } else { Vec::new() } };
        let facts = |g: &Group| match g.first {
            0 => Facts { original: true, completed: true, width: 50, name: "Тихая пристань".into(), ..Facts::default() },
            1 => Facts { campaign: true, original: true, width: 50, name: "Кампания".into(), ..Facts::default() },
            2 => Facts { favourite: true, width: 200, name: "Ахтунг".into(), ..Facts::default() },
            _ => Facts { original: true, width: 100, name: "Другой берег".into(), ..Facts::default() },
        };
        let list = || vec![g(0, false), g(1, true), g(2, false), g(3, false)];
        let firsts = |show: Show, order: Order| arrange(list(), facts, show, order).iter().map(|g| g.first).collect::<Vec<_>>();
        assert_eq!(firsts(Show::All, Order::Original), [0, 1, 2, 3], "the original's list as it is");
        assert_eq!(firsts(Show::Campaigns, Order::Original), [1]);
        assert_eq!(firsts(Show::Singles, Order::Original), [0, 2, 3]);
        assert_eq!(firsts(Show::Original, Order::Original), [0, 1, 3]);
        assert_eq!(firsts(Show::FanMade, Order::Original), [2]);
        assert_eq!(firsts(Show::Completed, Order::Original), [0]);
        assert_eq!(firsts(Show::NotCompleted, Order::Original), [1, 2, 3]);
        assert_eq!(firsts(Show::Favourites, Order::Original), [2]);
        assert_eq!(firsts(Show::All, Order::SmallFirst), [0, 3, 2, 1], "campaigns after, unsorted by size");
        assert_eq!(firsts(Show::All, Order::LargeFirst), [2, 3, 0, 1]);
        assert_eq!(firsts(Show::All, Order::Name), [2, 3, 1, 0]);
        assert_eq!((Show::of(99), Order::of(-1)), (Show::All, Order::Original), "a bad setting: the original's view");
        assert!(is_original_map("обучающий1") && !is_original_map("Изумрудная шахта"));
    }

    #[test]
    fn a_class_the_map_does_not_offer_cannot_be_clicked() {
        let offered = [true, false, true];
        assert_eq!(choose_class(offered, 0, 1), 0, "the greyed archmage keeps the knight");
        assert_eq!(choose_class(offered, 2, 1), 2);
        assert_eq!(choose_class(offered, 0, 2), 2);
        assert_eq!(choose_class(offered, 2, 0), 0);
    }

    #[test]
    fn campaigns_are_grouped_by_their_next_map_chain() {
        let maps = [
            ("Другой берег", 0, ""),
            ("ДС1-Начало", 1, "ДС2-Продолжение.DTm"),
            ("ДС2-Продолжение", 2, ""),
            ("РК1-Первая", 1, "рк2-вторая.dtm"),
            ("РК2-Вторая", 2, "РК3-Третья.DTm"),
            ("РК3-Третья", 2, "РК2-Вторая.DTm"),
            ("Сирота", 2, ""),
            ("Обрыв", 1, "Нет такой.DTm"),
        ];
        assert_eq!(
            groups(&maps),
            vec![
                Group { first: 0, chapters: vec![] },
                Group { first: 1, chapters: vec![1, 2] },
                Group { first: 3, chapters: vec![3, 4, 5] },
                Group { first: 7, chapters: vec![7] },
            ],
            "names match without case or extension; a loop and a missing map end the chain; an unreached later map is left out"
        );
    }

    #[test]
    fn next_starts_a_campaign_at_its_first_scenario_even_if_a_chapter_was_selected() {
        let campaign = Group { first: 3, chapters: vec![3, 4, 5] };
        assert_eq!(campaign_start(&[campaign], 0), Some(3));
        assert_eq!(campaign_start(&[], 0), None);
    }

    /// A chapter line is not a list item of its own in the original: the text list keeps the
    /// item of each line (+0x5f, 0x47e46c), selects and lights by item (0x47d71c), and Start
    /// copies the sorted map array's entry of the selected item (0x4c1804, [0x65adab]), so
    /// any line of «Раменское королевство» starts РК1, not the chapter under the pointer (a
    /// player's report: a game seen starting on РК6). On the install: one item for the
    /// campaign, РК1 its first map, the other six its chapters only (translated copies left out).
    #[test]
    fn a_campaign_on_the_install_is_one_item_started_on_its_first_map() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)).unwrap();
        let sc: Vec<ScenarioEntry> =
            dt.maps.iter().filter(|m| !razdor::translate::job::is_translated_file(&m.name)).map(|m| ScenarioEntry { file: m.name.clone(), path: m.path.clone(), scenario: m.load().unwrap() }).collect();
        let maps: Vec<(&str, u8, &str)> = sc.iter().map(|e| (e.file.as_str(), e.scenario.header.scenario_kind, e.scenario.next_map.as_str())).collect();
        let list = groups(&maps);
        let rk: Vec<&Group> = list.iter().filter(|g| g.chapters.iter().any(|&c| sc[c].file.starts_with("РК6"))).collect();
        assert_eq!(rk.len(), 1, "one item holds РК6");
        assert!(sc[rk[0].first].file.starts_with("РК1-"), "its start is {}", sc[rk[0].first].file);
        let files: Vec<String> = rk[0].chapters.iter().map(|&c| sc[c].file.chars().take(3).collect()).collect();
        assert_eq!(files, ["РК1", "РК2", "РК3", "РК4", "РК5", "РК6", "РК7"]);
        assert!(list.iter().all(|g| sc[g.first].scenario.header.scenario_kind != 2), "no later campaign map is an item");
    }
}
