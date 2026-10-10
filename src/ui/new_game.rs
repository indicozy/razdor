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
use razdor::rules::content::{Content, HeroClass, UnitId};
use razdor::rules::game::Game;
use razdor::trf;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::chrome::{self, CREAM, GOLD};
use super::dt_font::{with_face, Face};
use super::widgets::*;
use super::{screens, ScenarioEntry, Screen};

thread_local! {
    /// The scenario group picked (index into the list shown), the list's scroll in pixels
    /// at k = 1, and the hero picked.
    static PICKED: Cell<usize> = const { Cell::new(0) };
    static SCROLL: Cell<f32> = const { Cell::new(0.0) };
    static HERO: Cell<usize> = const { Cell::new(0) };
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
        (Win { r, k }, closed)
    }

    fn rect(&self, x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(self.r.x + x * self.k, self.r.y + y * self.k, w * self.k, h * self.k)
    }

    fn button(&self, x: f32, w: f32, label: &str, enabled: bool) -> bool {
        let r = self.rect(x, 442.0, w, 28.0);
        let hover = !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        chrome::marble_button(r, label, enabled, hover);
        enabled && hover && clicked()
    }
}

/// The blue bar of the picked row (as the load window's).
fn picked_bar(r: Rect) {
    let steps = 8;
    for s in 0..steps {
        let t = s as f32 / (steps - 1) as f32;
        let a = 0.85 - 0.5 * (t - 0.5).abs();
        draw_rectangle(r.x, r.y + r.h * s as f32 / steps as f32, r.w, r.h / steps as f32 + 0.5, Color::new(0.12, 0.2, 0.62, a));
    }
}

/// The round scenario icon: `SI_Castle`, `SI_Helm`, `SI_Swords`, `SI_Skull`, `SI_Tutorial`
/// by the map's picture index (header byte 0x120, 1..5), in the silver ring `si-border`.
fn scenario_icon(picture: u8, c: Vec2, size: f32) {
    const ICONS: [&str; 5] = ["SI_Castle", "SI_Helm", "SI_Swords", "SI_Skull", "SI_Tutorial"];
    let Some(name) = (picture as usize).checked_sub(1).and_then(|i| ICONS.get(i)) else { return };
    let r = Rect::new(c.x - size / 2.0, c.y - size / 2.0, size, size);
    draw_circle(c.x, c.y, size * 0.46, Color::new(0.0, 0.0, 0.0, 0.8));
    if let Some(t) = chrome::win(name) {
        let inner = Rect::new(r.x + size * 0.12, r.y + size * 0.12, size * 0.76, size * 0.76);
        chrome::tex(&t, inner, WHITE);
    }
    if let Some(t) = chrome::win_fx("si-border", chrome::Fx::KeyBlack) {
        chrome::tex(&t, r, WHITE);
    }
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
    groups(&maps)
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

/// "Сценарий для Новой Игры".
/// The list's scroll after the wheel turned by `turned` this frame: 30 px a notch. A frame
/// without a turn leaves it (`signum` of 0.0 is 1.0, which pulled the list back to its top
/// every frame, so it could not be scrolled).
fn wheel_scroll(scroll: f32, turned: f32) -> f32 {
    if turned == 0.0 { scroll } else { scroll - turned.signum() * 30.0 }
}

pub fn scenario_select(scenarios: &[ScenarioEntry], has_install: bool) -> Option<Screen> {
    if !has_install || chrome::win("Win-marble").is_none() {
        return screens::scenario_select(scenarios, has_install);
    }
    super::main_menu::backdrop();
    let (win, closed) = Win::open(&own("NewGame", "Title", n_("Scenario for a new game")));
    let k = win.k;
    let list = scenario_groups(scenarios);
    let mut picked = PICKED.with(|p| p.get()).min(list.len().saturating_sub(1));
    // The list: per group a round icon and the name, and a campaign's chapters under it in
    // small type; the picked group's block is lit. The wheel scrolls it.
    let rows = win.rect(12.0, 36.0, 270.0, 396.0);
    chrome::text_box(rows);
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
    SCROLL.with(|s| s.set(scroll));
    let mut next = None;
    let mut y = rows.y + 6.0 * k - scroll * k;
    // The rows are clipped to the box, so a block only partly in view shows its part (a
    // campaign taller than the room left was hidden, with the gap it left).
    let inside = Rect::new(rows.x, rows.y + 2.0 * k, rows.w, rows.h - 4.0 * k);
    let clip = crate::ui::widgets::Clip::new(inside);
    for (row, g) in list.iter().enumerate() {
        let r = Rect::new(rows.x + 4.0 * k, y, rows.w - 8.0 * k, block_h(g) - 2.0 * k);
        y += block_h(g);
        if r.y >= inside.y + inside.h || r.y + r.h <= inside.y {
            continue;
        }
        let hover = over_list && r.contains(crate::ui::widgets::pointer().into());
        if row == picked {
            picked_bar(r);
        } else if hover {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.15, 0.45, 0.35));
        }
        let e = &scenarios[g.first];
        let head = Rect::new(r.x, r.y, r.w, head_h - 2.0 * k);
        scenario_icon(e.scenario.header.scenario_picture_index, vec2(head.x + 14.0 * k, head.y + head.h / 2.0), 26.0 * k);
        let name = group_name(scenarios, g);
        with_face(Face::Subtitle, || {
            let size = fit_size(name, r.w - 40.0 * k, 15.0 * k);
            chrome::shadow_text(&ellipsize(name, r.w - 40.0 * k, size), head.x + 32.0 * k, head.y + head.h * 0.5 + size * 0.36, size, CREAM);
        });
        let small = 12.0 * k;
        for (n, &c) in listed_chapters(scenarios, g).iter().enumerate() {
            let cy = r.y + head_h + n as f32 * chapter_h + chapter_h * 0.5 + small * 0.36;
            chrome::shadow_text(&ellipsize(title(&scenarios[c]), r.w - 60.0 * k, small), r.x + 52.0 * k, cy, small, CREAM);
        }
        // A row is picked silently (the original's list makes no sound).
        if hover && clicked() {
            if row == picked {
                next = Some(Screen::ClassSelect { scenario: Some(g.first) });
            }
            picked = row;
        }
    }
    drop(clip);
    PICKED.with(|p| p.set(picked));
    // The map and what it is.
    if let Some(g) = list.get(picked) {
        let (i, e) = (g.first, &scenarios[g.first]);
        let frame = win.rect(290.0, 36.0, 295.0, 162.0);
        draw_rectangle(frame.x, frame.y, frame.w, frame.h, BLACK);
        // The frame's opening: the map's own picture (it is made for it), else the terrain.
        let inner = Rect::new(frame.x + 8.0 * k, frame.y + 8.0 * k, frame.w - 16.0 * k, frame.h - 16.0 * k);
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
        if let Some(t) = chrome::win_fx("MapImageFrame", chrome::Fx::KeyBlack) {
            chrome::tex(&t, frame, WHITE);
        }
        // With the picture in the frame, the terrain goes next to the name, status and size.
        let mut beside = 0.0;
        if pic.is_some() {
            if let Some(t) = preview(i, e) {
                let side = 56.0 * k;
                let r = Rect::new(frame.x + frame.w - side - 2.0 * k, frame.y + frame.h + 8.0 * k, side, side);
                let edge = Rect::new(r.x - 2.0 * k, r.y - 2.0 * k, r.w + 4.0 * k, r.h + 4.0 * k);
                draw_rectangle(edge.x, edge.y, edge.w, edge.h, BLACK);
                chrome::tex(&t, r, WHITE);
                chrome::silver_frame(edge, 1.0);
                beside = side + 10.0 * k;
            }
        }
        let status = match e.scenario.header.scenario_kind {
            1 => own("NewGame", "Campaign", n_("Campaign")),
            _ => own("NewGame", "OneScenario", n_("Single scenario")),
        };
        let lines = [
            (own("NewGame", "Name", n_("Name:")), group_name(scenarios, g).to_string()),
            (own("NewGame", "Status", n_("Status:")), status),
            (own("NewGame", "MapSize", n_("Map size")), format!("{}×{}", e.scenario.width(), e.scenario.height())),
        ];
        let size = 13.0 * k;
        let mut y = frame.y + frame.h + 20.0 * k;
        for (label, value) in lines {
            let label = label.trim_end_matches(':').to_string() + ":";
            chrome::shadow_text(&label, frame.x + 4.0 * k, y, size, GOLD);
            let lw = measure(&label, size).width + 6.0 * k;
            chrome::shadow_text(&ellipsize(&value, frame.w - lw - 8.0 * k - beside, size), frame.x + 4.0 * k + lw, y, size, CREAM);
            y += 17.0 * k;
        }
        let descript = own("NewGame", "Descript", n_("Description:"));
        chrome::shadow_text(&descript, frame.x + 4.0 * k, y, size, GOLD);
        let bottom = win.rect(0.0, 432.0, 0.0, 0.0).y;
        let desc = Rect::new(frame.x, y + 6.0 * k, frame.w, (bottom - y - 6.0 * k).max(0.0));
        chrome::text_box(desc);
        let small = 12.0 * k;
        for (n, line) in wrap(&e.scenario.description, desc.w - 16.0 * k, small).iter().enumerate() {
            let ly = desc.y + 16.0 * k + n as f32 * 14.0 * k;
            if ly > desc.y + desc.h - 4.0 * k {
                break;
            }
            chrome::shadow_text(line, desc.x + 8.0 * k, ly, small, CREAM);
        }
    }
    let can = !list.is_empty();
    if (win.button(369.0, 116.0, &own("Buttons", "Next", n_("Next")), can) || (can && key(KeyCode::Enter))) && next.is_none() {
        cue(Cue::Button);
        next = list.get(picked).map(|g| Screen::ClassSelect { scenario: Some(g.first) });
    }
    if win.button(492.0, 95.0, &own("Buttons", "Cancel", n_("Cancel")), true) || closed || key(KeyCode::Escape) {
        return Some(Screen::MainMenu);
    }
    next
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
    let saved = razdor::rules::save::default_dir().is_some_and(|d| {
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
    super::main_menu::backdrop();
    let (win, closed) = Win::open(&own("NewHero", "Title", n_("The hero's starting characteristics")));
    let k = win.k;
    // As the original (0x4c1804, 0x4743c8): only a class whose preset has a start cell is
    // offered; the window opens on the first one, or is not opened at all when there is
    // none. A greyed portrait takes no click, so a class the map leaves out cannot be
    // started (there is no key for it either).
    let offered = e.scenario.header.offered_classes();
    if !HERO_OPEN.with(|o| o.replace(true)) {
        match e.scenario.header.first_offered_class() {
            Some(c) => HERO.with(|h| h.set(c)),
            None => return leave_hero_window(Screen::MainMenu),
        }
    }
    let mut pick = HERO.with(|h| h.get()).min(2);
    // The three heroes.
    for (i, hero) in HeroClass::ALL.into_iter().enumerate() {
        let r = win.rect(27.0 + i as f32 * 185.0, 38.0, 170.0, 150.0);
        let hover = offered[i] && !input_blocked() && r.contains(crate::ui::widgets::pointer().into());
        draw_rectangle(r.x + 3.0 * k, r.y + 3.0 * k, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.5));
        match chrome::win(["Hero0", "hero1", "hero2"][i]) {
            Some(t) => chrome::tex_src(&t, Rect::new(0.0, (t.height() - t.height() * 150.0 / 170.0) / 2.0, t.width(), t.height() * 150.0 / 170.0), r, WHITE),
            None => assets.draw_portrait(hero.unit(), razdor::rules::battle::Team::Player, r),
        }
        if !offered[i] {
            draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.6));
        }
        if i == pick {
            chrome::glow_frame(r, Color::new(0.35, 1.0, 0.35, 1.0), true);
        } else if hover {
            chrome::glow_frame(r, Color::new(0.35, 0.55, 1.0, 0.9), false);
        }
        let name = &content.unit(hero.unit()).name;
        let ink = if i == pick { GOLD } else if offered[i] { CREAM } else { Color::new(0.5, 0.48, 0.44, 1.0) };
        with_face(Face::Subtitle, || chrome::shadow_centered(name, r.center().x, r.y + r.h + 18.0 * k, 16.0 * k, ink));
        if hover && clicked() {
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
    // The hero's name, typed.
    let name = screens::HERO_NAME.with(|n| {
        let mut n = n.borrow_mut();
        screens::edit_name(&mut n);
        n.clone()
    });
    // The name plate (`nameframe`, 162×22, silver): the name in dark ink on it.
    let field = win.rect(172.0, 212.0, 250.0, 250.0 * 22.0 / 162.0);
    let caret = if (get_time() * 2.0) as i64 % 2 == 0 { "|" } else { "" };
    let shown = if name.is_empty() { format!("{}{caret}", own("NewHero", "PrivateHeroName", n_("(no name)"))) } else { format!("{name}{caret}") };
    let size = 14.0 * k;
    match chrome::win("nameframe") {
        Some(t) => {
            chrome::tex(&t, field, WHITE);
            let ink = if name.is_empty() { Color::new(0.25, 0.25, 0.27, 1.0) } else { Color::new(0.05, 0.04, 0.03, 1.0) };
            with_face(Face::Bold, || {
                let w = measure(&shown, size).width;
                let (x, y) = (field.center().x - w / 2.0, field.y + field.h * 0.5 + size * 0.36);
                // A light halo keeps the ink readable on the chain.
                for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
                    text(&shown, x + dx * k, y + dy * k, size, Color::new(0.92, 0.92, 0.95, 0.9));
                }
                text(&shown, x, y, size, ink);
            });
        }
        None => {
            chrome::text_box(field);
            chrome::shadow_centered(&shown, field.center().x, field.y + field.h * 0.5 + size * 0.36, size, CREAM);
        }
    }
    // The book: the hero on the left page, his bonus and what the map gives him on the right.
    let book = win.rect(15.0, 250.0, 564.0, 186.0);
    match chrome::win("NewHero_Paper") {
        Some(t) => chrome::tex(&t, book, WHITE),
        None => chrome::text_box(book),
    }
    let n = pick + 1;
    let small = 12.0 * k;
    let page_w = book.w / 2.0 - 44.0 * k;
    let (left, right) = (book.x + 26.0 * k, book.x + book.w / 2.0 + 18.0 * k);
    let ours = [
        n_("A veteran of many cruel battles, the Knight has superb experience of war. Under the Church's protection he can withstand hostile spells."),
        n_("The Archmage can call on the mighty forces of the world that ordinary people cannot command. In battle he binds the enemy."),
        n_("The Ranger is a fast and accurate shot who finds his way anywhere. Watching nature, he has learnt the secrets of healing wounds fast."),
    ];
    let descript = own("NewHero", &format!("Descript{n}"), ours[pick]);
    for (j, line) in wrap(&descript, page_w, small).iter().take(9).enumerate() {
        chrome::shadow_text(line, left, book.y + 28.0 * k + j as f32 * 14.0 * k, small, CREAM);
    }
    let icon = 24.0 * k;
    chrome::trait_icon(&format!("HeroBonus{n}"), right, book.y + 18.0 * k, icon);
    let bonus_ours = [
        n_("The army of this hero takes 10% less damage from enemy attacks (magic excepted)."),
        n_("The Archmage casts spells twice as fast for 50% less mana, but his army gets no bonuses."),
        n_("The army of this hero travels 20% faster, and the wounded heal 20% of their hits every day."),
    ];
    let bonus = own("NewHero", &format!("Bonus{n}"), bonus_ours[pick]);
    let mut y = book.y + 28.0 * k;
    for (j, line) in wrap(&bonus, page_w - icon - 6.0 * k, small).iter().take(5).enumerate() {
        chrome::shadow_text(line, right + icon + 6.0 * k, y, small, chrome::BLUE_TEXT);
        y = book.y + 28.0 * k + (j + 1) as f32 * 14.0 * k;
    }
    let preset = &e.scenario.header.heroes[pick];
    y += 8.0 * k;
    chrome::shadow_text(&trf!("Gold: {gold}", gold = preset.gold), right, y, small, GOLD);
    let army: Vec<String> = preset
        .troops
        .iter()
        .filter(|t| t.unit != 0 && t.count > 0)
        .filter_map(|t| content.try_unit(UnitId(t.unit as u32)).map(|u| format!("{} {}", t.count, u.name)))
        .collect();
    let army = if army.is_empty() { tr("alone").to_string() } else { army.join(", ") };
    for (j, line) in wrap(&army, page_w, small).iter().take(4).enumerate() {
        chrome::shadow_text(line, right, y + (j + 1) as f32 * 14.0 * k, small, CREAM);
    }
    if win.button(369.0, 116.0, &own("NewHero", "Start", n_("Start")), true) || key(KeyCode::Enter) {
        cue(Cue::Button);
        *game = Some(screens::start_game(demo, Some((e, &content)), hero, &name));
        return leave_hero_window(Screen::WorldMap);
    }
    // Back or Esc returns to the main menu, not to the scenario list (0x4c0fd4).
    if win.button(492.0, 95.0, &own("Buttons", "Prev", n_("Back")), true) || closed || key(KeyCode::Escape) {
        return leave_hero_window(Screen::MainMenu);
    }
    None
}

/// Closes the hero window: the next opening starts again on the map's first offered class.
fn leave_hero_window(next: Screen) -> Option<Screen> {
    HERO_OPEN.with(|o| o.set(false));
    Some(next)
}

/// The class picked after a click on portrait `clicked`: a class the map does not offer
/// keeps the current pick (the original's hit test skips a disabled portrait, 0x4743c8).
fn choose_class(offered: [bool; 3], pick: usize, clicked: usize) -> usize {
    if offered[clicked] { clicked } else { pick }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_scenario_list_keeps_its_scroll_between_turns_of_the_wheel() {
        assert_eq!(wheel_scroll(60.0, 0.0), 60.0, "no turn, no move");
        assert_eq!(wheel_scroll(60.0, -1.0), 90.0, "down a notch");
        assert_eq!(wheel_scroll(60.0, 2.5), 30.0, "up a notch, whatever the wheel's step");
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
}
