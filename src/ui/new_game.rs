//! The original's new-game windows over the main menu's ruins: "Сценарий для Новой Игры"
//! (the scenario list with its round icons, the map in `MapImageFrame`, the name, status,
//! size and description) and "Стартовые характеристики Героя" (the three heroes' portraits,
//! the name field, and the chosen hero's description and bonus in the open book of
//! `NewHero_Paper`). The texts are the install's (`[NewGame]`, `[NewHero]`, `[Buttons]`).
//! No footage shows these two windows: their layout is ours, built from the original's art
//! (L). Without an install the plain screens of `ui::screens` stand in.

use razdor::local;
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
use super::language;
use super::hub_view::{hero, pane_scroll, Drag};
use super::map_import;
use super::widgets::*;
use super::{screens, ScenarioEntry, Screen};

local! {
    static EXPERIMENTAL: Cell<ExperimentalRules> = const { Cell::new(ExperimentalRules {
        simultaneous_bonuses: false, multiple_rings: false, multiple_amulets: false, allow_duplicate_items: false, two_potions: false, six_cells: false,
        multiple_weapons: false, free_units: false, auto_promotion: false, undying_units: false,
    }) };
    static RULES_OPEN: Cell<bool> = const { Cell::new(false) };
    /// The rules window's scroll (px) on an upright phone, reset when it closes.
    static RULES_SCROLL: Cell<f32> = const { Cell::new(0.0) };
    /// The scenario group picked (index into the list shown), the list's scroll in pixels
    /// at k = 1, and the hero picked.
    static PICKED: Cell<usize> = const { Cell::new(0) };
    /// The specific chapter selected in a campaign, when one is picked.
    static PICKED_MAP: Cell<Option<usize>> = const { Cell::new(None) };
    static SCROLL: Cell<f32> = const { Cell::new(0.0) };
    /// Whether the base game's group under a played mod is open (1445b): shut when the list opens.
    static BASE_OPEN: Cell<bool> = const { Cell::new(false) };
    /// The description's first shown line, and the group it was scrolled for.
    static DESC_SCROLL: Cell<(usize, usize)> = const { Cell::new((usize::MAX, 0)) };
    static HERO: Cell<usize> = const { Cell::new(0) };
    /// On a phone held upright a tap on a startable entry opens its card (`card_view`): whether it is
    /// open, its scroll, the content's height as the last frame drew it, and the drag of its scroll.
    static CARD: Cell<bool> = const { Cell::new(false) };
    static CARD_SCROLL: Cell<f32> = const { Cell::new(0.0) };
    static CARD_H: Cell<f32> = const { Cell::new(0.0) };
    static CARD_DRAG: Cell<Option<Drag>> = const { Cell::new(None) };
    /// A map to pick when the list opens (the `scenarios:<map>` snapshot scene).
    static PICK: RefCell<Option<String>> = const { RefCell::new(None) };
    /// The «Мод» choice made this frame (`Some(None)`: the base game), for the app to play
    /// (`take_mod_request`).
    static MOD_REQUEST: RefCell<Option<Option<String>>> = const { RefCell::new(None) };
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

/// The window's buttons' row, 28 high at Normal (the original's), at this y in the window at
/// Normal. The footer hangs from the window's bottom: a window taller than 482 (a phone held
/// upright, or the extras row below) moves the buttons down with it (see `window_video_h`).
const FOOT_Y: f32 = 443.0;
const FOOT_H: f32 = 28.0;
/// Razdor's extras row under the buttons («Добавить карту…» and «Мод»), 26 high at Normal.
const EXTRA_ROW_H: f32 = 26.0;
/// The gap between the buttons' row (or the filter's row) and the extras row.
const EXTRA_GAP: f32 = 12.0;

/// The footer's height in video pixels, from the buttons' row's top to the window's bottom: the
/// buttons grow with the text size (`button_grow`), and at Large and Largest the filter and the
/// order take a row of their own under them, since their wide labels need the room. At Normal the
/// original's one row, 11 px below it (the window's 482); `extras` adds the extras row (Razdor's own,
/// the owner's request of 2026-10-31) under it.
fn foot_block(g: f32, extras: bool) -> f32 {
    let b = FOOT_H * g;
    let mut h = b;
    if g > 1.0 {
        h += 6.0 + b;
    }
    if extras {
        h += EXTRA_GAP + EXTRA_ROW_H * g;
    }
    h + if g > 1.0 { 14.0 } else { 11.0 }
}

/// The window's height in video pixels: the footer at its place under the list, and with `fill`
/// on a phone held upright the rest of the screen (the list then gets its rows, Razdor's own, the
/// owner's request of 2026-10-31); elsewhere that place only.
fn window_video_h(g: f32, extras: bool, fill: bool) -> f32 {
    let base = FOOT_Y + foot_block(g, extras);
    match if fill { super::window_layout::portrait_title_fill(594.0) } else { None } {
        Some(h) => base.max(h),
        None => base,
    }
}

/// The footer's extra height in the window's video pixels, over the original's footer (0 at Normal
/// without the extras row): the inner frames' divider moves up by it.
fn footer_extra(g: f32, extras: bool) -> f32 {
    foot_block(g, extras) - 39.0
}

/// The top of the extras row in video pixels, under the buttons' row (and the filter's row).
fn extra_top(foot_y: f32, g: f32) -> f32 {
    foot_y + FOOT_H * g + if g > 1.0 { 6.0 + FOOT_H * g } else { 0.0 } + EXTRA_GAP
}

/// The window, 594×482 video pixels as the load window, low on the screen under the logo.
struct Win {
    r: Rect,
    k: f32,
    /// The buttons' row's top in video pixels (the footer hangs from the window's bottom).
    foot_y: f32,
}

impl Win {
    /// Opens the window. `extras`: the extras row is under the buttons; `fill`: a phone held upright
    /// gives the window the screen's free height (the scenario list only).
    fn open(title: &str, extras: bool, fill: bool) -> (Win, bool) {
        let k = chrome::k();
        let g = super::display::button_grow();
        let video_h = window_video_h(g, extras, fill);
        let (w, h) = (594.0 * k, video_h * k);
        let r = super::window_layout::title_rect((594.0, video_h)).unwrap_or_else(|| Rect::new((screen_width() - w) / 2.0, (super::chrome::canvas_top() + 190.0 * k).min(screen_height() - h), w, h));
        let (_, closed) = chrome::window(r, title, chrome::Skin::Marble, true);
        // The 634×516 window with its inner frames (ae2520, read by 4d2a30; 0x48d710); the footer's
        // growth is the window's, and the frames keep their place above it. The upright phone's
        // list window takes its own boxes (`Portrait`), so the desktop frames would cut through them.
        if !(fill && upright_dp().is_some()) {
            chrome::window_inner_frames_footed(r, chrome::opx(), 0.0, footer_extra(g, extras) * k);
        }
        (Win { r, k, foot_y: video_h - foot_block(g, extras) }, closed)
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.r.x + x * self.k, self.r.y + y * self.k, w * self.k, h * self.k)
    }

    /// The bottom of the list and of the description box, in video pixels: 11 above the buttons'
    /// row (the original's 432 at Normal without the extras row, 432 with it too).
    fn content_bottom(&self) -> f32 {
        self.foot_y - 11.0
    }

    /// A footer button on the buttons' row (grown high with the text, see `foot_block`).
    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        self.button_at(x, self.foot_y, w, FOOT_H * super::display::button_grow(), label, enabled)
    }

    fn button_at(&self, x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
        button_rect(self.rect(x, y, w, h), label, enabled)
    }
}

/// A marble button in the screen rectangle `r`: whether it was pressed.
fn button_rect(r: Rect, label: &str, enabled: bool) -> bool {
    let (_, down, pressed) = button_state(r, enabled);
    chrome::marble_button(r, label, enabled, down);
    pressed
}

/// The dp of a phone held upright (`None` elsewhere): the scenario window's own layout there (the
/// owner's order of 2026-11-02, see [`Portrait`]).
fn upright_dp() -> Option<f32> {
    let dp = super::display::dp_px()?;
    (screen_width() < screen_height()).then_some(dp)
}

/// The scenario window on a phone held upright, in screen pixels (the owner's order of 2026-11-02). Its
/// list view: the top row «Все сценарии», «Как в игре», «Добавить карту…» (and a «Мод» row under it when
/// mods are installed), the list full width under them, and the bottom row «...» and «Отмена». Its card
/// view (a tap on a startable entry opens it, `card_view`): the map's picture as the hero with its title
/// over it, the description under it, and the bottom row «Назад» and «Далее». Sizes in dp: the texts at
/// 14 to 16 dp (14 sp and up by the mobile check), the rows 36 and 30 dp tall, the buttons 42 dp.
struct Portrait {
    /// The scale: one dp in screen pixels.
    dp: f32,
    filter: Rect,
    order: Rect,
    add: Rect,
    mod_row: Option<Rect>,
    /// The list, under the top rows down to the bottom row.
    rows: Rect,
    dots: Rect,
    cancel: Rect,
    /// The card: from the top row's place down to the bottom row, the hero and the description in it.
    card: Rect,
    back: Rect,
    next: Rect,
    head_h: f32,
    chapter_h: f32,
    name_px: f32,
    chapter_px: f32,
    /// The card's description: its text, and how far each line moves the next one down.
    card_px: f32,
    card_step: f32,
}

impl Portrait {
    fn new(win: &Win, dp: f32, grow: f32, mods: bool) -> Portrait {
        let r = win.r;
        let (m, btn_h, gap) = (8.0 * dp, 42.0 * dp, 6.0 * dp);
        let x0 = r.x + 10.0 * dp;
        let w = r.w - 20.0 * dp;
        let top = r.y + 31.0 * win.k + 2.0 * dp;
        let bw = (w - 2.0 * gap) / 3.0;
        let (filter, order, add) = (Rect::new(x0, top, bw, btn_h), Rect::new(x0 + bw + gap, top, bw, btn_h), Rect::new(x0 + 2.0 * (bw + gap), top, bw, btn_h));
        let mut y = top + btn_h + m;
        let mod_row = mods.then(|| {
            let rr = Rect::new(x0, y, w, btn_h);
            y += btn_h + m;
            rr
        });
        let foot_y = r.bottom() - 12.0 * dp - btn_h;
        let rows = Rect::new(x0, y, w, (foot_y - m - y).max(0.0));
        let dots_w = 56.0 * dp;
        let dots = Rect::new(x0, foot_y, dots_w, btn_h);
        let cancel = Rect::new(x0 + dots_w + m, foot_y, w - dots_w - m, btn_h);
        let card = Rect::new(x0, top, w, (foot_y - m - top).max(0.0));
        let half = (w - m) / 2.0;
        let back = Rect::new(x0, foot_y, half, btn_h);
        let next = Rect::new(x0 + half + m, foot_y, half, btn_h);
        Portrait {
            dp,
            filter,
            order,
            add,
            mod_row,
            rows,
            dots,
            cancel,
            card,
            back,
            next,
            head_h: 36.0 * dp * grow,
            chapter_h: 30.0 * dp * grow,
            name_px: 15.0 * dp,
            chapter_px: 14.0 * dp,
            card_px: 16.0 * dp,
            card_step: 22.0 * dp,
        }
    }
}

/// The size a text of a list row is drawn at (Razdor's own, the owner's decision of 2026-10-31): the
/// row's text `s` grows with the row, up to `grow` times its Normal size: the largest size from
/// there that fits `width`, never below Normal (`display::grown_label_size`), so a text that fits at
/// Normal is never cut by its growth. Drawn inside `legacy_metrics_if` when it is grown (see
/// `button_label`).
pub(super) fn row_text_size(s: &str, normal: f32, grow: f32, width: f32) -> f32 {
    if grow <= 1.0 {
        return normal;
    }
    super::dt_font::legacy_metrics_if(true, || super::display::grown_label_size(normal, grow, width, |size| measure(s, size).width))
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

/// Whether a group belongs to the base game's group under a played mod (1445b, 1445c): every map
/// of it, its chapters too, is `base_game`, so a campaign moves as a unit and a campaign with a
/// map the mod changed stays in the main part.
fn is_base_group(scenarios: &[ScenarioEntry], g: &Group) -> bool {
    std::iter::once(&g.first).chain(&g.chapters).all(|&m| scenarios[m].base_game)
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
    /// The ticks of every mod (1456): the list shows the chosen mod's only (`language::won_under`).
    completed: language::Won,
    favourites: Vec<String>,
    show: i64,
    order: i64,
}

local! {
    static MARKS: RefCell<Option<Marks>> = const { RefCell::new(None) };
}

fn marks() -> Marks {
    MARKS.with(|m| {
        m.borrow_mut()
            .get_or_insert_with(|| {
                let s = super::language::Settings::load();
                Marks { completed: s.completed_by_mod, favourites: s.favourite_scenarios, show: s.scenario_show, order: s.scenario_order }
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

/// The experimental rules the player last set in the rules window, kept in `settings.json`
/// (`language::Settings::experimental_rules`; all off on a file without them). `for_new_game()`
/// is all off and carries nothing per game, so the whole set is remembered.
fn remembered_rules() -> ExperimentalRules {
    super::language::Settings::load().experimental_rules
}

/// Keeps `rules` as the remembered choice (the rules window closes; a game starts).
fn keep_rules(rules: ExperimentalRules) {
    let s = super::language::Settings::load();
    if s.experimental_rules != rules {
        super::language::Settings { experimental_rules: rules, ..s }.save();
    }
}

/// The «Мод» choice as the new-game row and the title's link show it: the labels (the base game
/// first, then each mod as «Мод: <title>»), the index of the mod played and whether its folder is
/// being built (`busy`, 1441: the row and the link say so and take no choice).
pub(super) struct ModChoice {
    pub labels: Vec<String>,
    pub current: i64,
    pub busy: bool,
}

impl ModChoice {
    /// The text shown: the mod played, or the build's notice while it is being built.
    pub(super) fn shown(&self) -> &str {
        if self.busy { tr("Preparing the mod…") } else { &self.labels[self.current as usize] }
    }
}

pub(super) fn mod_choice(mods: &[(String, razdor::hub::Text)], active: Option<&str>, busy: bool) -> ModChoice {
    let mut labels = vec![format!("{}: {}", tr("Mod"), tr("No mod"))];
    labels.extend(mods.iter().map(|(_, title)| format!("{}: {}", tr("Mod"), title.get(i18n::lang()))));
    let current = active.and_then(|a| mods.iter().position(|(id, _)| id == a)).map_or(0, |i| i + 1) as i64;
    ModChoice { labels, current, busy }
}

/// The mod id of a choice's index (`None`: the base game, index 0).
pub(super) fn mod_id_at(mods: &[(String, razdor::hub::Text)], index: i64) -> Option<String> {
    (index > 0).then(|| mods.get(index as usize - 1).map(|(id, _)| id.clone())).flatten()
}

/// Asks the app to play `id` (`take_mod_request`; `None`: the base game) and starts the list
/// over, as any new choice does.
pub(super) fn request_mod(id: Option<String>) {
    MOD_REQUEST.with_borrow_mut(|r| *r = Some(id));
    PICKED.with(|p| p.set(0));
    PICKED_MAP.with(|p| p.set(None));
    SCROLL.with(|s| s.set(0.0));
}

fn listed(names: &[String], file: &str) -> bool {
    names.iter().any(|n| n.to_lowercase() == file.to_lowercase())
}

/// A row's facts: a campaign counts as completed once its last map is won.
fn facts_of(scenarios: &[ScenarioEntry], g: &Group, m: &Marks, active: Option<&str>) -> Facts {
    let e = &scenarios[g.first];
    let last = g.chapters.last().map_or(g.first, |&c| c);
    Facts {
        campaign: !g.chapters.is_empty(),
        original: is_original_map(&e.file),
        completed: listed(language::won_under(&m.completed, active), &scenarios[last].file),
        favourite: listed(&m.favourites, &e.file),
        width: e.scenario.width(),
        name: group_name(scenarios, g).to_string(),
    }
}

/// The scenario list opens on its first row, scrolled to the top (the original's list
/// starts on row 0 each time). The player's marks are read again (a map won since).
pub fn open_list() {
    EXPERIMENTAL.with(|p| p.set(remembered_rules()));
    RULES_OPEN.with(|p| p.set(false));
    PICKED.with(|p| p.set(0));
    PICKED_MAP.with(|p| p.set(None));
    SCROLL.with(|s| s.set(0.0));
    BASE_OPEN.with(|b| b.set(false));
    MARKS.with(|m| *m.borrow_mut() = None);
    open_card(false);
}

/// Opens (or shuts) the card of the picked entry on the upright phone, its scroll at the top.
fn open_card(on: bool) {
    CARD.with(|c| c.set(on));
    CARD_SCROLL.with(|s| s.set(0.0));
    CARD_H.with(|h| h.set(0.0));
    CARD_DRAG.with(|d| d.set(None));
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

/// The header row of the base game's group under a played mod (1445b): a triangle (pointing right
/// shut, down open) and the group's name. A tap on it opens or shuts the group.
fn base_header(r: Rect, k: f32, grow: f32, open: bool) {
    let ink = super::typography::Ink::Yellow.color();
    let (x, cy) = (r.x + 5.0 * k, r.y + r.h / 2.0);
    if open {
        draw_triangle(vec2(x - k, cy - 3.0 * k), vec2(x + 9.0 * k, cy - 3.0 * k), vec2(x + 4.0 * k, cy + 4.0 * k), ink);
    } else {
        draw_triangle(vec2(x, cy - 5.0 * k), vec2(x, cy + 5.0 * k), vec2(x + 8.0 * k, cy), ink);
    }
    let label = tr("Base game maps");
    // The text starts close to the triangle: its room is the row's width, so it keeps the names' size.
    let room = r.w - 17.0 * k;
    let phone = super::display::dp_px();
    with_face(Face::Subtitle, || {
        let normal = fit_size(label, room, 15.0 * k);
        let grown = row_text_size(label, normal, grow, room);
        // On a phone at least 13.2 dp of text (its capital at least 12 sp by the touch check's measure,
        // 0.7 of the size per sp), in two lines when the label is too long for one (the row is 40 dp
        // there). The desktop row is one line, cut when too long.
        let size = grown.max(phone.map_or(0.0, |dp| 13.2 * dp));
        super::dt_font::legacy_metrics_if(size > normal, || match phone {
            Some(_) => {
                let lines = wrap_lines(label, room, size);
                let line_h = size * 1.15;
                let mut y = r.y + r.h / 2.0 - (lines.len() as f32 - 1.0) * line_h / 2.0 + size * 0.36;
                for line in &lines {
                    chrome::shadow_text(line, r.x + 16.0 * k, y, size, ink);
                    y += line_h;
                }
            }
            None => chrome::shadow_text(&ellipsize(label, room, size), r.x + 16.0 * k, r.y + r.h * 0.5 + size * 0.36, size, ink),
        });
    });
}

/// `s` broken into lines of at most `width` at `size`, at its spaces (a word wider than the line
/// stays whole on its own line).
fn wrap_lines(s: &str, width: f32, size: f32) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in s.split_whitespace() {
        match lines.last_mut() {
            Some(line) if measure(&format!("{line} {word}"), size).width <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

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

/// The art's corner ornaments are 34 px at the interface scale (`chrome::text_box_frame`).
const CORNER_PX: f32 = 34.0;

/// The rows' margin from the box's top and bottom edges, at the interface scale.
const EDGE_PX: f32 = 4.0;

/// The list's left inset from the box's left edge. The top-left ornament (`Corner_Frame-LU`, a
/// dense filigree over its 34 px square) leaves clear, at 34 px in, the icon column from x 24 px
/// and from y 16 px down: a row's icon is 20-24 px and sits 20 px in from the rows' left edge
/// (`scenario_icon`, 14 px to its centre), so the inset is 16 px. Checked on the art's opaque
/// pixels (`Corner_Frame-LU.lit`, 34×34).
const LEFT_PX: f32 = 16.0;

/// The first row's top, from the rows' top edge, at scroll 0: 7 px, so its icon's top (5 px below
/// the row's top) lands at 16 px, under the ornament's clear part (see `LEFT_PX`).
const FIRST_ROW_PX: f32 = 7.0;

/// The part of the text box `rows` where a list's rows go: the box's full width less the left
/// inset (`LEFT_PX`) and the box's full height less a small margin. The right corners and the
/// bottom corners are left to the scrollbar (`list_track`) and the last visible row's clip.
fn list_view(rows: Rect, k: f32) -> Rect {
    let left = (LEFT_PX * k).min(rows.w / 2.0);
    let edge = (EDGE_PX * k).min(rows.h / 2.0);
    Rect::new(rows.x + left, rows.y + edge, rows.w - left, rows.h - 2.0 * edge)
}

/// The scrollbar's track: beside the rows on the right, from below the top-right ornament to
/// above the bottom-right one (`CORNER_PX` at each end), so its arrow buttons (each a bar's
/// width square at its ends) are never under a corner.
fn list_track(rows: Rect, view: Rect, k: f32) -> Rect {
    let corner = (CORNER_PX * k).min(rows.h / 2.0);
    Rect::new(view.right() - 16.0 * k, rows.y + corner, 12.0 * k, rows.h - 2.0 * corner)
}

/// "Сценарий для Новой Игры".
/// The list's scroll after the wheel turned by `turned` this frame: 30 px a notch. A frame
/// without a turn leaves it (`signum` of 0.0 is 1.0, which pulled the list back to its top
/// every frame, so it could not be scrolled).
fn wheel_scroll(scroll: f32, turned: f32) -> f32 {
    if turned == 0.0 { scroll } else { scroll - turned.signum() * 30.0 }
}

/// The «Мод» choice made in the new-game window since the last call: `Some(None)` for the base
/// game, `Some(Some(id))` for a mod. The app plays it (`App::choose_mod`).
pub fn take_mod_request() -> Option<Option<String>> {
    MOD_REQUEST.with_borrow_mut(Option::take)
}

/// The new-game window (`scenarios` its list, `mods` the installed mods with their titles,
/// `active` the one played).
/// Whether the new game can be started (Next, Enter) with the list `can` and a mod being built
/// (`busy`): not while the folder is built (1441), so the game never plays a half-built one.
pub fn start_allowed(can: bool, busy: bool) -> bool {
    can && !busy
}

/// The scenario list window. `busy`: a mod is being built on a worker (`App::choose_mod`).
pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool, mods: &[(String, razdor::hub::Text)], active: Option<&str>, busy: bool) -> Option<Screen> {
    // On the upright phone the screen's texts keep their sizes (the legacy metrics, as the update window's):
    // its rows and description are in dp, not snapped to the font's native sizes.
    super::dt_font::legacy_metrics_if(upright_dp().is_some(), || scenario_select_screen(scenarios, has_install, mods, active, busy))
}

fn scenario_select_screen(scenarios: &[ScenarioEntry], has_install: bool, mods: &[(String, razdor::hub::Text)], active: Option<&str>, busy: bool) -> Option<Screen> {
    if RULES_OPEN.with(Cell::get) {
        super::main_menu::backdrop();
        experimental_menu();
        return None;
    }
    if !has_install || chrome::win("Win-marble").is_none() {
        return screens::scenario_select(scenarios, has_install);
    }
    super::main_menu::backdrop();
    let grow = super::display::button_grow();
    // Razdor's «Добавить карту…» and «Мод» take the extras row under the buttons (Razdor's own, the
    // owner's request of 2026-10-31): the window takes that row only where one of them is shown.
    let extras = map_import::available() || !mods.is_empty();
    let _scale = super::window_layout::title_scale((594.0, window_video_h(grow, extras, true)));
    let (win, closed) = Win::open(&own("NewGame", "Title", n_("Scenario for a new game")), extras, true);
    let k = win.k;
    let pl = upright_dp().map(|dp| Portrait::new(&win, dp, grow, !mods.is_empty()));
    let mut m = marks();
    let (show, order) = (Show::of(m.show), Order::of(m.order));
    // The maps made for the base game under a played mod (1445b) are a group of their own, after
    // the rest, shut unless its header row is opened (`BASE_OPEN`); the snapshot scene opens it once.
    let (main, base): (Vec<Group>, Vec<Group>) = scenario_groups(scenarios).into_iter().partition(|g| !is_base_group(scenarios, g));
    let main = arrange(main, |g| facts_of(scenarios, g, &m, active), show, order);
    let base = arrange(base, |g| facts_of(scenarios, g, &m, active), show, order);
    let base_count = base.len();
    let main_len = main.len();
    if base_count > 0 {
        if let Some(open) = super::snapshot::base_maps_scene() {
            SCROLL.with(|s| s.set(f32::MAX));
            BASE_OPEN.with(|b| b.set(open));
        }
    }
    let base_open = base_count > 0 && BASE_OPEN.with(Cell::get);
    let mut list = main;
    if base_open {
        list.extend(base);
    }
    if let Some(name) = PICK.with(|p| p.borrow_mut().take()) {
        if let Some(i) = list.iter().position(|g| std::iter::once(&g.first).chain(&g.chapters).any(|&m| scenarios[m].file.starts_with(&name))) {
            PICKED.with(|p| p.set(i));
            PICKED_MAP.with(|p| p.set(None));
            if super::snapshot::card_scene() {
                open_card(true);
            }
        }
    }
    let mut picked = PICKED.with(|p| p.get()).min(list.len().saturating_sub(1));
    // A phone held upright with a card open: the card is the whole window's view (`card_view`).
    if let Some(p) = pl.as_ref().filter(|_| CARD.with(Cell::get) && !list.is_empty()) {
        PICKED.with(|q| q.set(picked));
        return card_view(p, scenarios, &list, picked, start_allowed(true, busy), closed);
    }
    // The list: per group a round icon and the name, and a campaign's chapters under it in
    // small type; the picked group's block is lit. The wheel and the scrollbar scroll it.
    // The box's top left is the window's inner pane frame's corner (4, 31), so its silver edge
    // and the pane's line up (`window_inner_frames`, 0x48d710). Its bottom is the description
    // box's (`Win::content_bottom`): the room under the list is Razdor's extras row's, moved down
    // below the buttons, so the list takes it (the owner's request, backlog 1458).
    let rows = pl.as_ref().map_or_else(|| win.rect(4.0, 31.0, 278.0, win.content_bottom() - 31.0), |p| p.rows);
    chrome::text_box_base(rows);
    // The rows, their bar, the scrollbar and the empty-list line live in `view`: the box's
    // corner ornaments take the columns beside it, so nothing under them.
    let view = list_view(rows, k);
    // Razdor's own: the rows grow with the text size (`button_grow`), so does their text.
    let (head_h, chapter_h) = pl.as_ref().map_or((30.0 * k * grow, 18.0 * k * grow), |p| (p.head_h, p.chapter_h));
    // The base game's header row (1445b) is as tall as a group's head; on a phone at least a finger's
    // size, as a touch target.
    let section_h = match super::display::dp_px() {
        Some(dp) => head_h.max(FINGER_DP * dp + 2.0 * k),
        None => head_h,
    };
    let block_h = |g: &Group| head_h + listed_chapters(scenarios, g).len() as f32 * chapter_h + if listed_chapters(scenarios, g).is_empty() { 0.0 } else { 4.0 * k };
    let content_h: f32 = list.iter().map(block_h).sum::<f32>() + if base_count > 0 { section_h } else { 0.0 } + 12.0 * k;
    let max_scroll = (content_h - view.h).max(0.0) / k;
    let mut scroll = SCROLL.with(|s| s.get());
    swipe_rows(view, 30.0 * k);
    let over_list = !input_blocked() && view.contains(crate::ui::widgets::pointer().into());
    if over_list {
        scroll = wheel_scroll(scroll, wheel());
    }
    scroll = scroll.clamp(0.0, max_scroll);
    // Razdor's scrollbar on the right when the list does not all fit (a touch screen has no
    // wheel); the rows give it room. Its track runs the view's height, inside the right corners.
    let barred = max_scroll > 0.0;
    let bar = barred.then(|| list_track(rows, view, k));
    scrollbar_px(list_track(rows, view, k), &mut scroll, max_scroll, view.h / k, 15.0);
    SCROLL.with(|s| s.set(scroll));
    let row_w = view.w - if barred { 22.0 * k } else { 8.0 * k };
    chrome::text_box_decoration(rows);
    let inside = view;
    // The rows are clipped to the list's view, so a block only partly in view shows its part (a
    // campaign taller than the room left was hidden, with the gap it left).
    let clip = crate::ui::widgets::Clip::new(inside);
    let mut selection_y = view.y + FIRST_ROW_PX * k - scroll * k;
    // The base game's header row sits before its first row (`main_len`), at the main rows' end.
    let main_h: f32 = list[..main_len].iter().map(block_h).sum();
    let header = Rect::new(view.x + 4.0 * k, view.y + FIRST_ROW_PX * k - scroll * k + main_h, row_w, section_h - 2.0 * k);
    for (row, g) in list.iter().enumerate() {
        if base_open && row == main_len {
            selection_y += section_h;
        }
        let r = Rect::new(view.x + 4.0 * k, selection_y, row_w, block_h(g) - 2.0 * k);
        selection_y += block_h(g);
        if row == PICKED.with(|p| p.get()).min(list.len().saturating_sub(1)) && r.y < inside.y + inside.h && r.y + r.h > inside.y {
            picked_bar(r);
        }
    }
    let mut next = None;
    let upright = pl.is_some();
    let mut open_card_now = false;
    let mut y = view.y + FIRST_ROW_PX * k - scroll * k;
    for (row, g) in list.iter().enumerate() {
        if base_open && row == main_len {
            y += section_h;
        }
        let r = Rect::new(view.x + 4.0 * k, y, row_w, block_h(g) - 2.0 * k);
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
        // The finger area stops at the scrollbar's left edge: its arrows and knob win the tap (1444).
        let (_, _, star_pressed) = mark_button_state(star_r, bar, over_list);
        let f = facts_of(scenarios, g, &m, active);
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
        // The name leaves room for the marks it has (none: the original's width). The row's right
        // end is 32 px past the name's start (the name ends at the row's edge, 2 px short of the
        // scrollbar), so a name fits the row as it was before the corner insets (208 px).
        let name_w = r.w - (32.0 + if f.completed { 44.0 } else if f.favourite { 24.0 } else { 0.0 }) * k;
        with_face(Face::Subtitle, || {
            // Shrunk to fit the name's room (`fit_size`, 15 px at most), as before the insets; grown
            // with its row (`row_text_size`), cut by its room when the grown name is longer.
            // On the upright phone the name is 15 dp, cut by its room (`ellipsize`), never shrunk.
            let (normal, grown) = match &pl {
                Some(p) => (p.name_px, p.name_px),
                None => {
                    let normal = fit_size(name, name_w, 15.0 * k);
                    (normal, row_text_size(name, normal, grow, name_w))
                }
            };
            let is_grown = grown > normal;
            super::dt_font::legacy_metrics_if(is_grown, || {
                let size = if is_grown { grown } else { normal };
                chrome::shadow_text(&ellipsize(name, name_w, size), head.x + 32.0 * k, head.y + head.h * 0.5 + size * 0.36, size, name_ink);
            });
        });
        // The chapters' size: the Normal 12 px, grown with the row (`row_text_size`).
        let small_normal = pl.as_ref().map_or(12.0 * k, |p| p.chapter_px);
        with_face(Face::Body, || for (n, &c) in chapters.iter().enumerate() {
            let ink = if lit { super::typography::Ink::Plain } else { super::typography::Ink::Orange };
            // Each map of a campaign won has its tick too.
            let won = listed(language::won_under(&m.completed, active), &scenarios[c].file);
            let room = r.w - (if won { 84.0 } else { 60.0 }) * k;
            let small = if pl.is_some() { small_normal } else { row_text_size(title(&scenarios[c]), small_normal, grow, room) };
            let is_grown = small > small_normal;
            let cy = r.y + head_h + n as f32 * chapter_h + chapter_h * 0.5 + small * 0.36;
            super::dt_font::legacy_metrics_if(is_grown, || chrome::shadow_text(&ellipsize(title(&scenarios[c]), room, small), r.x + 52.0 * k, cy, small, ink.color()));
            if won {
                tick(head.x + head.w - 40.0 * k, cy - small * 0.36 - 5.0 * k, 10.0 * k, TICK_GREEN);
            }
        });
        // A campaign heading or individual chapter is selected only on click.
        if head_pressed {
            match pick_tap(upright, row == picked && PICKED_MAP.with(|p| p.get()).is_none()) {
                PickTap::Start => next = Some(Screen::ClassSelect { scenario: Some(g.first) }),
                PickTap::Card => open_card_now = true,
                PickTap::Pick => {}
            }
            picked = row;
            PICKED_MAP.with(|p| p.set(None));
        } else {
            for (n, &chapter_index) in chapters.iter().enumerate() {
                let chapter = Rect::new(r.x + 28.0 * k, r.y + head_h + n as f32 * chapter_h, r.w - 28.0 * k, chapter_h);
                if over_list && chapter.contains(crate::ui::widgets::pointer().into()) && clicked() {
                    match pick_tap(upright, PICKED_MAP.with(|p| p.get()) == Some(chapter_index)) {
                        PickTap::Start => next = Some(Screen::ClassSelect { scenario: Some(g.first) }),
                        PickTap::Card => open_card_now = true,
                        PickTap::Pick => {}
                    }
                    picked = row;
                    PICKED_MAP.with(|p| p.set(Some(chapter_index)));
                    break;
                }
            }
        }
    }
    if open_card_now {
        open_card(true);
    }
    if base_count > 0 {
        let visible = header.y < inside.y + inside.h && header.y + header.h > inside.y;
        if visible {
            base_header(header, k, grow, base_open);
        }
        // A touch target (`button_state`: its finger area on a phone), tapped open or shut.
        let (_, _, tapped) = button_state(header, over_list && visible);
        if tapped {
            BASE_OPEN.with(|b| b.set(!base_open));
            // Shut, the picked row may be one of the group's: the list's first row is picked then.
            if base_open && picked >= main_len {
                picked = 0;
                PICKED_MAP.with(|p| p.set(None));
            }
        }
    }
    drop(clip);
    if list.is_empty() {
        with_face(Face::Body, || {
            let size = 13.0 * k;
            let t = tr("No scenario fits this choice.");
            chrome::shadow_text(&ellipsize(t, view.w - 24.0 * k, size), view.x + 12.0 * k, view.y + 24.0 * k, size, super::typography::Ink::Orange.color());
        });
    }
    PICKED.with(|p| p.set(picked));
    // The map and what it is.
    if let (None, Some(g)) = (&pl, list.get(picked)) {
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
        // The description's sizes: its scale `kk` (the window's k, or the phone's dp).
        let bottom = win.rect(0.0, win.content_bottom(), 0.0, 0.0).y;
        let (desc, kk) = (Rect::new(frame.x, frame.y + frame.h + 8.0 * k, frame.w, (bottom - frame.y - frame.h - 8.0 * k).max(0.0)), k);
        chrome::text_box(desc);
        let size = 12.0 * kk;
        let room = desc.w - 34.0 * k;
        let title_text = format!("^*{}", group_name(scenarios, g).replace(['^', '*', '|', '@'], ""));
        let mut rows = markup_rows(&title_text, room, 16.0 * kk);
        let title_rows = rows.len();
        // The description is added as plain lines (0x4c1bb4 → 0x47ea80): marks stay as typed,
        // no indent, justified, in ae24f4 over the shadow font ae2510.
        rows.extend(plain_rows_face(&e.scenario.description, room, size, Face::Bold));
        let line_h = 14.0 * kk;
        let steps = desc_steps(title_rows, rows.len(), kk);
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
                draw_markup_row_shadowed(row, desc.x + 17.0 * k, y, room, 16.0 * kk, Face::Subtitle, super::typography::Ink::Plain.color());
                y += 20.0 * kk;
                if n + 1 == title_rows {
                    y += 12.0 * kk;
                }
            } else {
                let orange = with_face(Face::Bold, || super::typography::Ink::Orange.color());
                with_face(Face::Bold, || draw_markup_row_shadowed(row, desc.x + 17.0 * k, y, room, size, Face::Bold, orange));
                y += line_h;
            }
        }
        drop(clip);
    }
    // The extras row under the buttons (Razdor's own, the owner's request of 2026-10-31): «Добавить
    // карту…» and «Мод» side by side, each half of the window's width; alone, the whole width.
    let both = map_import::available() && !mods.is_empty();
    let (x_mod, w_mod) = if both { (306.0, 276.0) } else { (12.0, 570.0) };
    let extras_y = extra_top(win.foot_y, grow);
    let extras_h = EXTRA_ROW_H * grow;
    let add_r = match &pl {
        Some(p) => p.add,
        None if both => win.rect(12.0, extras_y, 276.0, extras_h),
        None => win.rect(12.0, extras_y, 570.0, extras_h),
    };
    add_map_button(has_install && !busy, add_r);
    // Razdor's filter and order (a players' wish), under the list where the original's window
    // has nothing: each opens a drop-down list; the choice is kept in the settings.
    // The footer (Razdor's own, see `foot_block`): at Normal the original's row, unchanged; at
    // Large and Largest the filter and the order take a row of their own under the grown buttons.
    let two_rows = grow > 1.0;
    let b = FOOT_H * grow;
    let (choice_y, choice_h, choice_w) = if two_rows { (win.foot_y + b + 6.0, b, 140.0 * grow) } else { (win.foot_y - 1.0, FOOT_H, 140.0) };
    let (show_r, order_r) = match &pl {
        Some(p) => (p.filter, p.order),
        None => (win.rect(12.0, choice_y, choice_w, choice_h), win.rect(12.0 + choice_w + 6.0, choice_y, choice_w, choice_h)),
    };
    let pick_show = view_choice_at(&win, show_r, "razdor:scenario-show", show as i64, &Show::ALL.map(|s| s.label()));
    let pick_order = view_choice_at(&win, order_r, "razdor:scenario-order", order as i64, &Order::ALL.map(|o| o.label()));
    if pick_show.is_some() || pick_order.is_some() {
        m.show = pick_show.unwrap_or(m.show);
        m.order = pick_order.unwrap_or(m.order);
        keep_marks(m);
        PICKED.with(|p| p.set(0));
        PICKED_MAP.with(|p| p.set(None));
        SCROLL.with(|s| s.set(0.0));
    }
    // Razdor's «Мод» (a players' wish): the base game or an installed mod, which the app plays and
    // whose maps the list then shows. Only once a mod is installed.
    if !mods.is_empty() {
        let choice = mod_choice(mods, active, busy);
        let refs: Vec<&str> = choice.labels.iter().map(String::as_str).collect();
        let row = match &pl {
            Some(p) => p.mod_row.unwrap_or(Rect::new(0.0, 0.0, 0.0, 0.0)),
            None => win.rect(x_mod, extras_y, w_mod, extras_h),
        };
        // While the folder is built the row says so and takes no choice (1441).
        let picked = if choice.busy {
            chrome::marble_button(row, choice.shown(), false, false);
            None
        } else {
            view_choice_at(&win, row, "razdor:scenario-mod", choice.current, &refs)
        };
        if !choice.busy && super::snapshot::open_mod_list() {
            let options: Vec<(i64, String)> = choice.labels.iter().enumerate().map(|(i, l)| (i as i64, l.clone())).collect();
            super::widgets::open_list("razdor:scenario-mod", row, choice.current, &options, win.k);
        }
        if let Some(v) = picked {
            request_mod(mod_id_at(mods, v));
        }
    }
    let can = !list.is_empty();
    let start = start_allowed(can, busy);
    if experimental_rules() != ExperimentalRules::default() && pl.is_none() {
        let label_y = if two_rows { choice_y + b + 2.0 } else { win.foot_y + 27.0 };
        let label = win.rect(12.0, label_y, 306.0, 12.0);
        chrome::shadow_text(tr("Experimental rules enabled"), label.x, label.y + 9.0 * k, 9.0 * k, chrome::ORANGE_TEXT);
    }
    // The buttons on the row's right end, 7 units apart, grown from their right ends (at Normal the
    // original's places: «Отмена» 492 to 587, «Далее» 369, «...» 328).
    let cancel_w = 95.0 * grow;
    let cancel_x = 587.0 - cancel_w;
    let next_w = 116.0 * grow;
    let next_x = cancel_x - 7.0 - next_w;
    let dots_w = 34.0 * grow;
    let dots_x = next_x - 7.0 - dots_w;
    let (dots_r, cancel_r) = match &pl {
        Some(p) => (p.dots, p.cancel),
        None => (win.rect(dots_x, win.foot_y, dots_w, FOOT_H * grow), win.rect(cancel_x, win.foot_y, cancel_w, FOOT_H * grow)),
    };
    let next_r = win.rect(next_x, win.foot_y, next_w, FOOT_H * grow);
    if button_rect(dots_r, "...", can) {
        RULES_OPEN.with(|p| p.set(true));
        return None;
    }
    let selected_map = campaign_start(&list, picked);
    // The upright list starts nothing: its card has «Далее».
    if pl.is_none() && (button_rect(next_r, &own("Buttons", "Next", n_("Next")), start) || (start && key(KeyCode::Enter))) && next.is_none() {
        cue(Cue::Button);
        next = selected_map.map(|i| Screen::ClassSelect { scenario: Some(i) });
    }
    if button_rect(cancel_r, &own("Buttons", "Cancel", n_("Cancel")), true) || closed || key(KeyCode::Escape) {
        return Some(Screen::MainMenu);
    }
    next
}

/// What a tap on a list row does: on the desktop and a phone held sideways a first tap picks the entry
/// and a second one on the picked entry (or its chapter) starts it; on a phone held upright a tap picks
/// it and opens its card, where «Далее» starts it.
#[derive(Debug, PartialEq)]
enum PickTap {
    Start,
    Pick,
    Card,
}

fn pick_tap(upright: bool, already_picked: bool) -> PickTap {
    if upright {
        PickTap::Card
    } else if already_picked {
        PickTap::Start
    } else {
        PickTap::Pick
    }
}

/// The card of the picked entry on a phone held upright (the hub's card, the owner's order of
/// 2026-11-02): the map's picture as the hero with its title over it, its description under it at
/// 16 dp, scrolled by a swipe, the wheel or the bar, and the bottom row «Назад» (back to the list) and
/// «Далее» (starts the campaign or the map, as the list's «Далее» did). The window's close and Esc go
/// back to the list too.
fn card_view(p: &Portrait, scenarios: &[ScenarioEntry], list: &[Group], picked: usize, start: bool, closed: bool) -> Option<Screen> {
    let g = &list[picked];
    let (i, e) = (g.first, &scenarios[g.first]);
    let (k, card) = (p.dp, p.card);
    let most = (CARD_H.with(Cell::get) - card.h).max(0.0);
    let (mut scroll, mut drag) = (CARD_SCROLL.with(Cell::get), CARD_DRAG.with(Cell::get));
    let room = pane_scroll(&mut scroll, &mut drag, card, most, k, true);
    CARD_SCROLL.with(|s| s.set(scroll));
    CARD_DRAG.with(|d| d.set(drag));
    let r = Rect::new(card.x, card.y - scroll, card.w - room, card.h);
    let clip = crate::ui::widgets::Clip::new(card);
    let content_h = super::dt_font::legacy_metrics_if(true, || {
        let pic = picture(i, e);
        let hh = hero(r, pic.as_ref(), card.h * 0.45, &group_name(scenarios, g).replace(['^', '*', '|', '@'], ""), k, k);
        let mut y = r.y + hh + 16.0 * k;
        let room = r.w - 12.0 * k;
        let orange = with_face(Face::Bold, || super::typography::Ink::Orange.color());
        // Wrapped as the hub's text is, each line centred (the owner's note of 2026-11-02; justified
        // lines leave wide gaps at this size).
        let lines = with_face(Face::Bold, || super::hub_view::wrap_long(&e.scenario.description, room, p.card_px));
        for l in &lines {
            with_face(Face::Bold, || chrome::shadow_centered(l, r.x + r.w / 2.0, y, p.card_px, orange));
            y += p.card_step;
        }
        y - r.y + 10.0 * k
    });
    drop(clip);
    CARD_H.with(|h| h.set(content_h));
    if button_rect(p.back, tr("Back"), true) || closed || key(KeyCode::Escape) {
        open_card(false);
        return None;
    }
    if button_rect(p.next, &own("Buttons", "Next", n_("Next")), start) || (start && key(KeyCode::Enter)) {
        cue(Cue::Button);
        return campaign_start(list, picked).map(|i| Screen::ClassSelect { scenario: Some(i) });
    }
    None
}

pub(super) fn experimental_rules() -> ExperimentalRules { EXPERIMENTAL.with(Cell::get) }

pub(super) fn open_experimental_menu() { RULES_OPEN.with(|p| p.set(true)); }

pub(super) fn snapshot_experimental(rules: ExperimentalRules) { EXPERIMENTAL.with(|p| p.set(rules)); }

/// A snapshot scene's scroll for the rules window (`RAZDOR_SCENE_SCROLL`): it starts `px` down, and
/// the window clamps it when drawn.
pub(super) fn scene_scroll(px: f32) { RULES_SCROLL.with(|s| s.set(px)); }

/// The menu's equipment-slot hint for the six-cell switch, in the current language.
fn slots_hint(six_cells: bool) -> &'static str {
    if six_cells {
        tr("Six equipment slots. Duplicate abilities do not stack.")
    } else {
        tr("Four equipment slots. Duplicate abilities do not stack.")
    }
}

/// The touch target's least side in dp (Razdor's phone rule; the mobile-ui check's limit).
const TOUCH_DP: f32 = 40.0;

/// The OK button's height in dp on a phone held upright (the owner's note of 2026-11-02).
const OK_DP: f32 = 44.0;

/// Kept modal so clicks and Enter cannot start a scenario behind the options.
///
/// The window's scale `o` is the pop-ups' (`display::popup_o`), fitted to the screen. Its rows and
/// OK are touch targets: `big` (`o` times the text size's growth), and at least 40 dp on a phone.
/// Its texts take Razdor's text size: a phone's text is one px per dp (`tx`), grown with the text
/// size (`button_grow`), never under its Normal size. The hints wrap to the window's width and the
/// window grows with them. The rows take one, two or three columns: the fewest whose height fits
/// the screen (a phone's landscape takes two or three); the window then takes the screen's width.
fn experimental_menu() {
    /// The switches in the window (one row each).
    const ROWS: usize = 10;
    let (sw, sh) = (screen_width(), screen_height());
    let k = chrome::k();
    let o = super::display::popup_o(k, 550.0, 410.0).min((sw - 12.0) / 550.0).min((sh - 12.0) / 410.0);
    let dp = super::display::dp_px();
    let tx = dp.map_or(o, |d| d.max(o));
    let g = super::display::button_grow();
    let big = o * g;
    let touch = dp.map_or(0.0, |d| TOUCH_DP * d);
    let (row_h, gap) = ((28.0 * big).max(touch), 10.0 * big);
    // On the upright phone the OK button is 44 dp tall and wider, its label bigger (the owner's note of
    // 2026-11-02); the rest of the window stays as it was.
    let upright = dp.is_some_and(|d| d > 0.0 && sw < sh);
    let ok_h = (28.0 * big).max(if upright { OK_DP * dp.unwrap_or(0.0) } else { touch });
    let (hint_size, hint_pitch) = (14.0 * tx * g, 21.0 * tx * g);
    // The hint lines of the switches that are on (the two big ones say what they change); the
    // weapon line says which rule holds.
    let now = experimental_rules();
    let mut hints = vec![slots_hint(now.six_cells), if now.multiple_weapons { tr("Several weapons allowed.") } else { tr("One weapon per unit always.") }];
    if now.free_units {
        hints.push(tr("Units of the hero's army ask no daily wages."));
    }
    if now.undying_units {
        hints.push(tr("Dead units of the hero's army come back with 1 HP after a battle."));
    }
    if now.auto_promotion {
        hints.push(tr("Units promote on their own to a random option of their upgrade tree."));
    }
    hints.push(tr("Applies to all armies; saved with this playthrough."));
    hints.push(tr("The potion limit resets after a battle."));
    let wrapped = |iw: f32| -> Vec<String> { super::dt_font::legacy_metrics_if(true, || hints.iter().flat_map(|l| wrap_cached(l, iw, hint_size)).collect()) };
    // Every label is drawn at the same size (Normal); one too wide for its column wraps to two
    // lines. A row is as tall as two lines when any label wraps at that column count, else one.
    let normal = 16.0 * tx;
    let label_pitch = normal * 1.2;
    let labels: [&str; ROWS] = [
        tr("All special bonuses simultaneously"),
        tr("Allow multiple rings"),
        tr("Allow multiple amulets"),
        tr("Allow duplicate items"),
        tr("At most two potions per character"),
        tr("Six equipment cells"),
        tr("Allow multiple weapons"),
        tr("Free units"),
        tr("Automatic promotion"),
        tr("Units don't die"),
    ];
    // An upright phone's window takes the screen less the safe area (cutouts) and 12 px each side.
    let safe = super::display::safe_area();
    let phone_w = (sw - safe.left - safe.right - 24.0).max(0.0);
    let width_for = |cols: usize| match cols {
        1 if upright => phone_w,
        1 => (550.0 * big).min(sw - 12.0),
        2 => (1100.0 * big).min(sw - 12.0),
        _ => (1500.0 * big).min(sw - 12.0),
    };
    // On an upright phone the window is at most 60% of the screen high: its rows and hints scroll
    // between the title and the OK button, with a bar at the right, and the rows keep clear of it.
    let bar_w = if upright { 14.0 * big } else { 0.0 };
    let col_w_for = |cols: usize| {
        let iw = width_for(cols) - 40.0 * big - bar_w;
        (iw - 20.0 * big * (cols as f32 - 1.0)) / cols as f32
    };
    let wraps = |cols: usize| labels.iter().any(|l| super::dt_font::legacy_metrics_if(true, || wrap_cached(l, col_w_for(cols) - 30.0 * big, normal)).len() > 1);
    let row_h_for = |cols: usize| if wraps(cols) { row_h.max(2.0 * label_pitch + 6.0 * big) } else { row_h };
    // The window's height with `cols` columns of rows and `lines` lines of hints.
    let height = |cols: usize, lines: usize| {
        let per = ROWS.div_ceil(cols) as f32;
        54.0 * o + per * row_h_for(cols) + (per - 1.0) * gap + 8.0 * big + lines as f32 * hint_pitch + 8.0 * big + ok_h + 12.0 * big
    };
    let lines_for = |cols: usize| wrapped(width_for(cols) - 40.0 * big - bar_w);
    // An upright phone has one column: its height scrolls, so the rows are never squeezed.
    let cols = if upright { 1 } else { (1..=3).find(|&c| height(c, lines_for(c).len()) <= sh - 12.0).unwrap_or(3) };
    let (w, lines) = (width_for(cols), lines_for(cols));
    let row_h = row_h_for(cols);
    let cap = if upright { (sh * 0.6).min(sh - 12.0) } else { sh - 12.0 };
    let h = height(cols, lines.len()).min(cap);
    let r = Rect::new(((sw - w) / 2.0).round(), ((sh - h) / 2.0).round(), w, h);
    let iw = w - 40.0 * big - bar_w;
    let per_col = ROWS.div_ceil(cols);
    let col_gap = 20.0 * big;
    let col_w = (iw - col_gap * (cols as f32 - 1.0)) / cols as f32;
    // The scroll area: from under the title down to 8 units above the OK button. Its rows and
    // hints are as tall as the window's natural content, so it scrolls only when the cap bites.
    let ok_top = r.bottom() - 12.0 * big - ok_h;
    let view = Rect::new(r.x + 20.0 * big, r.y + 54.0 * o, iw + bar_w, (ok_top - 8.0 * big) - (r.y + 54.0 * o));
    let content_h = per_col as f32 * row_h + (per_col as f32 - 1.0) * gap + 8.0 * big + lines.len() as f32 * hint_pitch;
    let most = (content_h - view.h).max(0.0);
    let mut scroll = RULES_SCROLL.with(|s| s.get()).clamp(0.0, most);
    if most > 0.0 {
        swipe_rows(view, 40.0 * big);
        if !input_blocked() && view.contains(crate::ui::widgets::pointer().into()) {
            scroll = wheel_scroll(scroll, wheel()).clamp(0.0, most);
        }
    }
    let content_top = view.y - scroll;    let (_, close) = chrome::window(r, tr("Experimental scenario rules"), chrome::Skin::Marble, true);
    let mut rules = experimental_rules();
    let check = |index: usize, label: &str, value: &mut bool| {
        let (c, rr) = (index / per_col, index % per_col);
        let row = Rect::new(r.x + 20.0 * big + c as f32 * (col_w + col_gap), content_top + rr as f32 * (row_h + gap), col_w, row_h);
        let box_r = Rect::new(row.x, row.y + (row_h - 20.0 * big) / 2.0, 20.0 * big, 20.0 * big);
        chrome::silver_frame(box_r, big.max(1.0));
        if *value {
            draw_line(box_r.x+4.0*big,box_r.y+10.0*big,box_r.x+8.0*big,box_r.y+15.0*big,2.0*big,chrome::GOLD);
            draw_line(box_r.x+8.0*big,box_r.y+15.0*big,box_r.x+17.0*big,box_r.y+5.0*big,2.0*big,chrome::GOLD);
        }
        // The label's lines at the column's width: one, or two when it wraps.
        let text = super::dt_font::legacy_metrics_if(true, || wrap_cached(label, row.w - 30.0 * big, normal));
        if text.len() <= 1 {
            super::dt_font::legacy_metrics_if(true, || chrome::shadow_text(label, row.x + 30.0 * big, row.y + row_h / 2.0 + normal * 0.375, normal, chrome::CREAM));
        } else {
            let top = row.y + (row_h - text.len() as f32 * label_pitch) / 2.0;
            for (i, line) in text.iter().enumerate() {
                let base = top + i as f32 * label_pitch + normal * 0.8;
                super::dt_font::legacy_metrics_if(true, || chrome::shadow_text(line, row.x + 30.0 * big, base, normal, chrome::CREAM));
            }
        }
        // A row toggles on a press of its own, only where the pointer is in the visible view (a
        // swipe is no press), so a row scrolled out of view cannot toggle.
        let in_view = row.bottom() > view.y && row.y < view.bottom();
        let (_, _, pressed) = if in_view { button_state(row, true) } else { (false, false, false) };
        if pressed && view.contains(crate::ui::widgets::pointer().into()) { *value = !*value; }
    };
    let clip = super::widgets::Clip::new(view);
    check(0, labels[0], &mut rules.simultaneous_bonuses);
    check(1, labels[1], &mut rules.multiple_rings);
    check(2, labels[2], &mut rules.multiple_amulets);
    check(3, labels[3], &mut rules.allow_duplicate_items);
    check(4, labels[4], &mut rules.two_potions);
    check(5, labels[5], &mut rules.six_cells);
    check(6, labels[6], &mut rules.multiple_weapons);
    check(7, labels[7], &mut rules.free_units);
    check(8, labels[8], &mut rules.auto_promotion);
    check(9, labels[9], &mut rules.undying_units);
    EXPERIMENTAL.with(|p| p.set(rules));
    let hints_top = content_top + per_col as f32 * row_h + (per_col as f32 - 1.0) * gap + 8.0 * big;
    for (i, line) in lines.iter().enumerate() {
        // The baseline sits 0.8 of the size below the line's top.
        super::dt_font::legacy_metrics_if(true, || chrome::shadow_text(line, r.x + 20.0 * big, hints_top + i as f32 * hint_pitch + hint_size * 0.8, hint_size, chrome::CREAM));
    }
    drop(clip);
    if most > 0.0 {
        // The bar in the strip the rows keep clear of, its track the view's height.
        let bar = Rect::new(view.right() - 12.0 * big, view.y, 10.0 * big, view.h);
        scrollbar_px(bar, &mut scroll, most, view.h, 40.0 * big);
    }
    RULES_SCROLL.with(|s| s.set(scroll));
    let ok_w = match dp {
        Some(d) if upright => (120.0 * d).max(100.0 * big),
        _ => 100.0 * big,
    };
    let b = Rect::new(r.center().x - ok_w / 2.0, r.bottom() - 12.0 * big - ok_h, ok_w, ok_h);
    let (_, down, pressed) = button_state(b,true);
    chrome::marble_button(b,"",true,down);
    if upright {
        // Its label at 17 dp (the marble's own size at 44 dp is held to the window's scale).
        let size = 17.0 * dp.unwrap_or(1.0);
        let d = measure(tr("OK"), size);
        super::dt_font::legacy_metrics_if(true, || chrome::shadow_text(tr("OK"), b.center().x - d.width / 2.0, b.y + (b.h + d.offset_y) / 2.0 - 1.0, size, chrome::CREAM));
    } else {
        chrome::marble_button(b,tr("OK"),true,down);
    }
    if close || pressed || key(KeyCode::Escape) || key(KeyCode::Enter) {
        RULES_OPEN.with(|p| p.set(false));
        keep_rules(rules);
    }
}

/// Razdor's «Добавить карту…» on the extras row under the buttons (`at`: window units), with the
/// hint of what it takes as its tooltip (Razdor's own, the owner's request of 2026-10-31): the
/// platform's picker copies the map (or the archive's maps) into the install (`ui::map_import`).
fn add_map_button(has_install: bool, r: Rect) {
    if !map_import::available() {
        return;
    }
    let enabled = has_install && map_import::can_start();
    let (_, down, pressed) = button_state(r, enabled);
    chrome::marble_button(r, tr("Add a map…"), enabled, down);
    if pressed {
        cue(Cue::Button);
        map_import::request();
    }
    if mouse_in(r.x, r.y, r.w, r.h) {
        tooltip(&[(tr("A .DTm file or a .zip archive: an archive may hold a campaign or several maps.").to_string(), INK)]);
    }
}

/// [`view_choice`] in the rectangle `r` (screen pixels).
fn view_choice_at(win: &Win, r: Rect, key: &str, current: i64, labels: &[&str]) -> Option<i64> {
    let picked = take_pick(key);
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
    let _scale = super::window_layout::title_scale((594.0, window_video_h(super::display::button_grow(), false, false)));
    let (win, closed) = Win::open(&own("Tutorial", "Title", n_("Tutorial scenario")), false, false);
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
    // A map that needs a mod's units or items is not started (its notice says why).
    if let Some((e, content)) = &scenario {
        if map_import::refused(e, content) {
            return Some(Screen::ScenarioSelect);
        }
    }
    if scenario.is_none() || chrome::win("Win-marble").is_none() {
        return screens::class_select(game, demo, scenario, assets);
    }
    let (e, content) = scenario.expect("checked");
    let content = Arc::new(content.with_experimental(experimental_rules()));
    super::main_menu::backdrop();
    // Razdor's own (the owner's decision, 2026-10-31): the buttons' row grows with the text size
    // (`button_grow`), and the window with it: 34 units under the row at Normal, as before.
    let grow = super::display::button_grow();
    let hero_h = 516.0 + 34.0 * (grow - 1.0);
    let _scale = super::window_layout::title_scale((632.0, hero_h));
    let scale = chrome::k();
    let r = super::window_layout::title_rect((632.0, hero_h)).unwrap_or_else(|| Rect::new((screen_width() - 632.0 * scale) / 2.0, (super::chrome::canvas_top() + 202.0 * scale).min(screen_height() - hero_h * scale), 632.0 * scale, hero_h * scale));
    // Above the on-screen keyboard while the name is typed in: the name field's row (y 207 in
    // video px, the same for every hero) is the one `window_layout::keyboard_lifted` keeps in sight.
    let r = super::window_layout::keyboard_lifted(r, Win { r, k: scale, foot_y: 0.0 }.rect(38.0, 207.0, 160.0, 22.0));
    let title = own("NewHero", "Title", n_("The hero's starting characteristics"));
    let (_, closed) = chrome::window(r, &title, chrome::Skin::Marble, true);
    let win = Win { r, k: scale, foot_y: 0.0 };
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
    // «Отмена» ends at 622 and «Начать» is 10 units before it, as at Normal (528 and 424); both are
    // grown with the text size (`button_grow`), from their right ends.
    let cancel_w = 94.0 * grow;
    let cancel_x = 622.0 - cancel_w;
    let start_w = 94.0 * grow;
    let start_x = cancel_x - 10.0 - start_w;
    let hero_button = |x, w, label: &str| {
        let r = win.rect(x, 470.0, w, 34.0 * grow);
        let (_, down, pressed) = button_state(r, true);
        chrome::marble_button(r, label, true, down);
        pressed
    };
    // On a phone with the name's keyboard up, Enter is the keyboard's key: it hides the keyboard, not Start.
    let enter = screens::HERO_TYPING.with(|t| {
        let mut typing = t.get();
        let enter = name_enter(super::display::dp_px().is_some(), &mut typing, key(KeyCode::Enter));
        t.set(typing);
        enter
    });
    if hero_button(start_x, start_w, &own("NewHero", "Start", n_("Start"))) || enter {
        cue(Cue::Button);
        super::items_view::reset_pack_view();
        keep_rules(experimental_rules());
        *game = Some(screens::start_game(demo, Some((e, &content)), hero, &name));
        return leave_hero_window(Screen::WorldMap);
    }
    // Back or Esc returns to the main menu, not to the scenario list (0x4c0fd4).
    if hero_button(cancel_x, cancel_w, &own("Buttons", "Cancel", n_("Cancel"))) || closed || key(KeyCode::Escape) {
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
    // A phone's name field has no keyboard until the player taps it (`widgets::name_keyboard`).
    screens::HERO_TYPING.with(|t| t.set(false));
}

/// The class picked after a click on portrait `clicked`: a class the map does not offer
/// keeps the current pick (the original's hit test skips a disabled portrait, 0x4743c8).
fn choose_class(offered: [bool; 3], pick: usize, clicked: usize) -> usize {
    if offered[clicked] { clicked } else { pick }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The footer at Normal keeps the original's places: the buttons at 443 and the list and the
    /// description ending at 432, with or without the extras row (1458); the extras row sits under
    /// the buttons and fits the window at Large and Largest too.
    #[test]
    fn a_tap_on_a_startable_entry_opens_its_card_only_on_the_upright_phone() {
        // Upright: any pick opens the card, the second tap included (Далее starts it there).
        assert_eq!(pick_tap(true, false), PickTap::Card);
        assert_eq!(pick_tap(true, true), PickTap::Card);
        // Elsewhere, as before: the first tap picks, the second on the picked entry starts it.
        assert_eq!(pick_tap(false, false), PickTap::Pick);
        assert_eq!(pick_tap(false, true), PickTap::Start);
    }

    #[test]
    fn the_footer_keeps_the_original_places_and_the_extras_row_fits() {
        assert_eq!(window_video_h(1.0, false, false), 482.0, "the original's window without the extras");
        let h = window_video_h(1.0, true, false);
        assert_eq!(h, 520.0, "the extras row takes 38 units under the buttons");
        let win = Win { r: Rect::new(0.0, 0.0, 594.0, h), k: 1.0, foot_y: h - foot_block(1.0, true) };
        assert_eq!((win.foot_y, win.content_bottom()), (443.0, 432.0), "the buttons and the list's foot keep their places");
        assert_eq!(extra_top(win.foot_y, 1.0), 483.0);
        for g in [1.0, 1.25, 1.5] {
            let h = window_video_h(g, true, false);
            let foot_y = h - foot_block(g, true);
            let last = extra_top(foot_y, g) + EXTRA_ROW_H * g;
            assert!(last <= h - 11.0, "the extras row ends above the window's bottom at {g}");
            assert!(extra_top(foot_y, g) > foot_y + FOOT_H * g, "under the buttons at {g}");
        }
    }

    /// A tick shows for the chosen mod only (1456): a map won with no mod is not ticked under
    /// Evolution, a map won under Evolution is not ticked with no mod, and an old no-mod tick
    /// shows with no mod.
    #[test]
    fn a_tick_shows_for_the_chosen_mod_only() {
        let mut won = language::Won::new();
        language::add_won(&mut won, None, "Тихая пристань");
        language::add_won(&mut won, Some("evolution"), "Осада");
        let ticked = |active: Option<&str>, file: &str| listed(language::won_under(&won, active), file);
        assert!(ticked(None, "Тихая пристань"), "the base game's tick under no mod");
        assert!(!ticked(Some("evolution"), "Тихая пристань"), "no tick under Evolution for a map won without it");
        assert!(ticked(Some("evolution"), "Осада"), "Evolution's own tick");
        assert!(!ticked(None, "Осада"), "and not with no mod");
        assert!(!ticked(Some("other"), "Осада"), "another mod shows none");
    }

    /// The «Мод» choice the row and the title's link share: the base game first, the mod played
    /// selected, an unknown or no active mod is the base game, and the id of each index.
    #[test]
    fn the_mod_choice_lists_the_base_game_then_the_mods_and_selects_the_one_played() {
        let text = |ru: &str| razdor::hub::Text { ru: ru.into(), en: ru.into() };
        let mods = vec![("evolution".to_string(), text("Эволюция")), ("siege".to_string(), text("Осада"))];
        let c = mod_choice(&mods, Some("siege"), false);
        assert_eq!(c.labels.len(), 3);
        assert_eq!(c.labels[0], format!("{}: {}", tr("Mod"), tr("No mod")));
        assert_eq!(c.labels[2], format!("{}: Осада", tr("Mod")));
        assert_eq!(c.current, 2);
        assert_eq!(c.shown(), c.labels[2], "the link shows the mod played");
        assert_eq!(mod_choice(&mods, None, false).current, 0);
        assert_eq!(mod_choice(&mods, Some("gone"), false).current, 0, "a mod that is gone is the base game");
        assert_eq!(mod_id_at(&mods, 0), None);
        assert_eq!(mod_id_at(&mods, 1).as_deref(), Some("evolution"));
        assert_eq!(mod_id_at(&mods, 5), None, "an index past the list is the base game");
    }

    #[test]
    fn a_mod_being_built_shows_the_build_and_no_choice() {
        let mods = vec![("evolution".to_string(), razdor::hub::Text { ru: "Эволюция".into(), en: String::new() })];
        let c = mod_choice(&mods, Some("evolution"), true);
        assert!(c.busy);
        assert_eq!(c.shown(), tr("Preparing the mod…"));
    }

    /// The equipment hint follows the six-cell switch alone, in both languages.
    #[test]
    fn the_slot_hint_follows_only_the_six_cell_switch() {
        assert_eq!(slots_hint(false), tr("Four equipment slots. Duplicate abilities do not stack."));
        assert_eq!(slots_hint(true), tr("Six equipment slots. Duplicate abilities do not stack."));
        assert_ne!(slots_hint(false), slots_hint(true));
        assert!(i18n::tr_in(Lang::Ru, "Four equipment slots. Duplicate abilities do not stack.").starts_with("Четыре слота экипировки."));
        assert!(i18n::tr_in(Lang::Ru, "Six equipment slots. Duplicate abilities do not stack.").starts_with("Шесть слотов экипировки."));
    }

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
    fn the_scenario_list_lies_between_the_corner_columns() {
        // The box is 278 × 329 units; each corner is 34 px square (at k = 1). The rows take the
        // full height less a 4 px margin and the box's width less a 16 px left inset: the first
        // icon (x 24..44 from the box's left, y from 16 at scroll 0, see `LEFT_PX`) clears the
        // top-left ornament. The scrollbar's track runs from below the top-right ornament to
        // above the bottom-right one, so its arrows are never under a corner.
        let rows = Rect::new(12.0, 36.0, 540.0, 648.0);
        let v = list_view(rows, 2.0);
        assert_eq!((v.x, v.y, v.w, v.h), (12.0 + 32.0, 36.0 + 8.0, 540.0 - 32.0, 648.0 - 16.0));
        assert_eq!(v.right(), rows.right(), "the rows run to the box's right edge; the track sits inside it");
        let t = list_track(rows, v, 2.0);
        assert_eq!((t.x, t.y, t.w, t.h), (v.right() - 32.0, rows.y + 68.0, 24.0, rows.h - 136.0));
        assert!(t.y >= rows.y + CORNER_PX * 2.0 && t.bottom() <= rows.bottom() - CORNER_PX * 2.0);
        // At 1280 × 800 (k ≈ 1.11) of the box at (4, 31) of 278 × 329: the first row's head sits
        // 11 units into the box, and the name's room is the 208 units the list had before.
        let one = list_view(Rect::new(4.0, 31.0, 278.0, 329.0), 1.0);
        assert_eq!((one.x, one.y, one.w, one.h), (20.0, 35.0, 262.0, 321.0));
        assert_eq!(one.y + FIRST_ROW_PX - 31.0, 11.0);
        let row_w = one.w - 22.0;
        assert_eq!(row_w - 32.0, 208.0, "the name's room: the row less the icon column");
        let t1 = list_track(Rect::new(4.0, 31.0, 278.0, 329.0), one, 1.0);
        assert_eq!((t1.x, t1.w), (266.0, 12.0));
    }

    /// The favourite star's tap area (a player's report, 1444): a finger area grown to 48 dp
    /// reaches under the scrollbar; cut at its left edge, the arrows and knob win the tap, and the
    /// star still takes a tap on itself.
    #[test]
    fn the_favourite_star_s_tap_area_ends_left_of_the_scrollbar() {
        // A barred list at k = 2 (the box at (4, 31) of 278 × 329 at 1280 × 800 scale 2, with its
        // rows as `scenario_select` lays them).
        let k = 2.0;
        let rows = Rect::new(4.0, 31.0, 278.0, 329.0);
        let view = list_view(rows, k);
        let bar = list_track(rows, view, k);
        let row_w = view.w - 22.0 * k;
        let head = Rect::new(view.x + 4.0 * k, view.y + FIRST_ROW_PX * k, row_w, 30.0 * k - 2.0 * k);
        let star = Rect::new(head.x + head.w - 22.0 * k, head.y + head.h / 2.0 - 9.0 * k, 18.0 * k, 18.0 * k);
        let screen = Rect::new(0.0, 0.0, 1280.0 * 2.0, 800.0 * 2.0);
        let grown = finger_hit_at(star, Some(2.0), screen, screen.h, &[]);
        assert!(grown.right() > bar.x, "the unclipped area reaches the bar: the bug");
        let hit = clip_left_of(grown, Some(bar));
        assert!(hit.right() <= bar.x, "the tap area ends left of the bar");
        assert!(hit.contains(star.center()), "the star's own tap still works");
        assert!(!hit.contains(vec2(bar.x + 4.0 * k, star.center().y)), "a tap on the bar's track is not the star's");
        assert_eq!(clip_left_of(grown, None), grown, "no bar: the area as grown");
    }

    /// A mod being built starts nothing (1441): Next and Enter wait for the folder, as an empty
    /// list does, so a game never starts on a half-built folder.
    #[test]
    fn a_mod_being_built_starts_nothing() {
        assert!(start_allowed(true, false), "the list and no mod being built: Next starts");
        assert!(!start_allowed(true, true), "the folder is being built: no start");
        assert!(!start_allowed(false, false), "an empty list: no start");
    }

    #[test]
    fn the_scenario_list_keeps_its_scroll_between_turns_of_the_wheel() {
        assert_eq!(wheel_scroll(60.0, 0.0), 60.0, "no turn, no move");
        assert_eq!(wheel_scroll(60.0, -1.0), 90.0, "down a notch");
        assert_eq!(wheel_scroll(60.0, 2.5), 30.0, "up a notch, whatever the wheel's step");
    }

    /// Opening the hero window on a phone does not ask for the keyboard: the flag a previous
    /// game left is cleared, and a phone asks only after a tap in the field.
    #[test]
    fn the_hero_name_field_on_a_phone_has_no_keyboard_on_opening() {
        screens::HERO_TYPING.with(|t| t.set(true));
        fill_name_field();
        assert!(!screens::HERO_TYPING.with(|t| t.get()), "a new window starts without the keyboard");
        assert!(!crate::ui::widgets::name_keyboard(true, false, crate::ui::widgets::NameTouch::Nothing));
        assert!(crate::ui::widgets::name_keyboard(true, false, crate::ui::widgets::NameTouch::FieldTapped), "a tap in the field shows it");
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
            dt.maps.iter().filter(|m| !razdor::translate::job::is_translated_file(&m.name)).map(|m| ScenarioEntry { file: m.name.clone(), path: m.path.clone(), scenario: m.load().unwrap(), base_game: false }).collect();
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
