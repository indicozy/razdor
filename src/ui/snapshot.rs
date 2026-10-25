//! Debug snapshots, for comparing screens with the original without playing to them:
//! `RAZDOR_SCENE` sets the screen up, `RAZDOR_SNAPSHOT=<file.png>` saves the frame after
//! `RAZDOR_SNAPSHOT_FRAMES` frames (default 20) and quits. Meant for an invisible display,
//! e.g. `xvfb-run -s "-screen 0 1024x768x24" razdor` with `RAZDOR_SIZE=1024x768`.
//!
//! Scenes (`<map>` is a map file name of the install without `.DTm`, e.g. `РК3-Столица`):
//! `title`, `fonts` (sample lines in many scripts), `update` (a made-up release on offer), `authors[:<seconds>]`, `options[:advanced]`, `scenarios[:<map>]`, `tutorial`, `load[:manual|auto[:<row>]]`, `editor[:<map>[:<text to find>]]`, `classes:<map>`, `map:<map>[:x,y]`, `minimap:<map>`, `clash:<map>` (the first two armies fight on the explored map), `walk:<map>:dx,dy` (the
//! hero sets off that many cells away), `building:<map>:<n>`
//! (the hero in the n-th building; `:barracks`, `:garrison`, `:market`, `:sanctuary` open that tab; `:market-pack` the market's pack list), `army:<map>[:<n>[:<unit id>[:<level>]]]` (squad member n selected, made that unit at that level), `journal:<map>` / `hero:<map>[:<tab>]`, `spells:<map>` (`spells-pages:<map>`: 20 spells, page 2 open),
//! `menu:<map>`, `defeat:<map>` (the lost battle's report), `event:<map>:<n>` (the map's event n's window; `RAZDOR_SCENE_LOST=<id>,…` adds its lost-items row),`learnt:<map>[:<id>,…]` (the event window with new spells, the hero with no mana), `report:<map>[:<gold>]` (the noon report), `settings:<map>[:advanced|keys]` (`keys`: Razdor's keys window over the advanced settings), `battle:<map>:<n>` (against the n-th army), `custom` (the custom battle
//! setup), `custom-battle` (its first round; `custom-battle:watch` watches the AI play it). `RAZDOR_SCENE_DEBUG=1` turns the map's debug overlay (F3) on; `RAZDOR_SCENE_SHOW=x,y,r` shows
//! a place as a lantern event does; `RAZDOR_SCENE_CONSOLE=help;gold 100` opens the cheat
//! console after those commands; `RAZDOR_SCENE_FILTER=text` fills the inventory filter
//! (army screen, market); `RAZDOR_SCENE_QUIET=1` drops
//! the scenario's messages every frame, to see the screen under them; `RAZDOR_MOUSE=x,y`
//! puts the pointer there (`RAZDOR_MOUSE_RIGHT=1` also holds the right button there: the map's tooltips); `RAZDOR_SCENE_SPELLS=<id>,…` puts those spells on every unit; `RAZDOR_SCENE_TARGET=<id>` starts the targeting of that enemy spell on the map; `RAZDOR_SCENE_MANA=<n>` sets the hero's mana in the `spells` scene; `RAZDOR_SCENE_PREVIEW=1` shows the first spell's card at the pointer in the `learnt` scene; `RAZDOR_SCENE_POTION=1` gives every unit of the army a
//! drunk potion. `RAZDOR_SCENE_LANGUAGE=1` opens the language list (`title`, `options:advanced`, `settings:<map>:advanced`). `replay:<step>` with `RAZDOR_REPLAY=<actions.jsonl>`: the diff
//! `RAZDOR_SCENE_EXPERIMENTAL=bonuses,rings,amulets` enables selected playthrough rules;
//! `RAZDOR_SCENE_RULES=1` opens their menu over `scenarios`. `RAZDOR_SCENE_BONUS_ITEMS=1`
//! equips the hero with available distinct bonus items to exercise the trait scrollbar.
//! test's action list played to that step. `RAZDOR_SCENE_BATTLE_EXIT=1` opens a battle
//! scene's leave-battle window. `RAZDOR_SCENE_PACK_AT=<cell>:<id>,…` puts items in those pack
//! cells; `RAZDOR_SCENE_HELD=<id>` puts an item on the pointer (hero and army windows);
//! `RAZDOR_SCENE_WEAR=<id>,…` wears those items on the squad member of `army:<map>:<n>:<unit>`.
//! `RAZDOR_SCENE_PAN=dx,dy` holds the interface's pan at that offset (windows larger than the screen).
//! `RAZDOR_TEXT_SIZE=<1|1.25|1.5>` sets the text size and `RAZDOR_LARGE_WINDOWS=1` turns the large
//! pop-up message windows on (the original's size and off otherwise; the saved settings are ignored).
//! `RAZDOR_DP=<f32>` is a phone's density (screen px per dp): the phone layout of the pop-ups and
//! the event window (their size in dp, the window's width following the screen), on the desktop.
//!
//! Every map scene starts its map as a new game (`Game::from_scenario`): nothing is carried
//! over from a campaign's map before, so a later map whose opening events check for what it
//! should have brought ends at once by its own defeat event (РК2 does without the herald,
//! so `battle:РК2-…:0` shows the defeat screen). The game is not at fault there.

use razdor::local;
use razdor::rules::content::HeroClass;
use razdor::rules::game::{Foe, Game};
use razdor::rules::town::first_tab;

use super::battle_view::BattleView;
use super::building_view::BuildingView;
use super::saves::{Back, LoadView};
use super::{App, Screen};

local! {
    /// The `fonts` scene is on.
    static FONTS_SCENE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The `fonts` scene's lines: one per script a translated map may carry.
const FONT_SAMPLES: &[&str] = &[
    "English: The hero enters the town of Ravenhold.",
    "Русский: Герой входит в город «Вороний холм» — 120 золота.",
    "Deutsch: Größere Übermacht, schöne Grüße aus Köln.",
    "Ελληνικά: Ο ήρωας μπαίνει στην πόλη.",
    "हिन्दी: नायक शहर में प्रवेश करता है।",
    "中文: 英雄进入了城镇。",
    "日本語: 勇者は町に入った。カタカナ",
    "Tiếng Việt: Người anh hùng vào thành phố.",
];

/// Draws the `fonts` scene over the frame: the samples in each of the interface's faces,
/// with a line under each as wide as `widgets::measure` says (drawing and measuring must
/// agree), centred and shortened copies, and the fallback chain.
pub fn draw_overlay() {
    use super::dt_font::{with_face, Face};
    use super::widgets;
    use macroquad::prelude::*;
    if !FONTS_SCENE.with(|f| f.get()) {
        return;
    }
    draw_rectangle(0.0, 0.0, screen_width(), screen_height(), Color::new(0.08, 0.07, 0.06, 1.0));
    let mut y = 22.0;
    let mode = format!("{:?}", super::dt_font::mode());
    // A developer's caption (snapshots only), not an interface text.
    let caption = format!("mode {mode}; chain: {}", super::script_font::chain_names().join(", "));
    widgets::text(&caption, 10.0, y, 13.0, widgets::DIM);
    y += 8.0;
    for (face, size) in [(Face::Body, 18.0), (Face::Title, 18.0), (Face::Subtitle, 16.0)] {
        with_face(face, || {
            for (i, s) in FONT_SAMPLES.iter().enumerate() {
                y += size * 1.35;
                let x = if i % 2 == 0 { 10.0 } else { 20.0 };
                let w = widgets::measure(s, size).width;
                draw_line(x, y + 3.0, x + w, y + 3.0, 1.0, Color::new(0.9, 0.3, 0.2, 0.8));
                if face == Face::Title {
                    super::chrome::shadow_text(s, x, y, size, widgets::INK);
                } else {
                    widgets::text(s, x, y, size, widgets::INK);
                }
            }
        });
    }
    // Centred and shortened to a box: the box's edges must hold the text.
    y += 30.0;
    for (i, s) in FONT_SAMPLES.iter().enumerate().skip(4).take(3) {
        let bx = 10.0;
        let by = y + (i - 4) as f32 * 26.0;
        draw_rectangle_lines(bx, by - 18.0, 260.0, 24.0, 1.0, widgets::DIM);
        widgets::text_fit(s, bx + 2.0, by, 256.0, 18.0, widgets::ACCENT);
        draw_rectangle_lines(300.0, by - 18.0, 400.0, 24.0, 1.0, widgets::DIM);
        widgets::text_centered(s, 500.0, by, 18.0, widgets::ACCENT);
    }
}

/// Where to save the snapshot and after how many frames.
pub fn target() -> Option<(String, u64)> {
    let path = std::env::var("RAZDOR_SNAPSHOT").ok()?;
    let frames = std::env::var("RAZDOR_SNAPSHOT_FRAMES").ok().and_then(|f| f.trim().parse().ok()).unwrap_or(20);
    Some((path, frames))
}

/// The window size asked for by `RAZDOR_SIZE=<w>x<h>`.
pub fn size() -> Option<(i32, i32)> {
    let s = std::env::var("RAZDOR_SIZE").ok()?;
    let (w, h) = s.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// The scenario's messages are dropped every frame (`RAZDOR_SCENE_QUIET`).
pub fn quiet() -> bool {
    std::env::var("RAZDOR_SCENE_QUIET").is_ok_and(|v| !v.is_empty())
}

local! {
    /// `RAZDOR_SCENE_LANGUAGE` has opened the language list.
    static LANGUAGE_OPENED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// `RAZDOR_SCENE_LANGUAGE=1`: the first language button drawn (the main menu's, the advanced
/// settings') opens its list, once.
pub fn open_language_list() -> bool {
    if LANGUAGE_OPENED.with(|o| o.get()) || !std::env::var("RAZDOR_SCENE_LANGUAGE").is_ok_and(|v| v == "1") {
        return false;
    }
    LANGUAGE_OPENED.with(|o| o.set(true));
    true
}

/// `RAZDOR_SCENE_PACK_VIEW=<group>,<order>`: the item group (`all`, `weapons`, `armour`,
/// `helms`, `shields`, `rings`, `amulets`, `potions`, `other`) and order (`shown`, `name-up`, `name-down`,
/// `price-down`, `price-up`) the backpack and the market show; an unknown word leaves it out.
fn pack_view_from_env() -> Option<(razdor::rules::pack_order::ItemGroup, razdor::rules::pack_order::PackSort)> {
    use razdor::rules::pack_order::{ItemGroup, PackSort};
    let v = std::env::var("RAZDOR_SCENE_PACK_VIEW").ok()?;
    let (group, sort) = v.split_once(',').unwrap_or((v.as_str(), "shown"));
    let group = match group {
        "all" => ItemGroup::All,
        "weapons" => ItemGroup::Weapons,
        "armour" => ItemGroup::Armour,
        "helms" => ItemGroup::Helms,
        "shields" => ItemGroup::Shields,
        "rings" => ItemGroup::Rings,
        "amulets" => ItemGroup::Amulets,
        "potions" => ItemGroup::Potions,
        "other" => ItemGroup::Other,
        _ => return None,
    };
    let sort = match sort {
        "shown" => PackSort::Shown,
        "name-up" => PackSort::NameUp,
        "name-down" => PackSort::NameDown,
        "price-down" => PackSort::PriceDown,
        "price-up" => PackSort::PriceUp,
        _ => return None,
    };
    Some((group, sort))
}

/// `RAZDOR_MOUSE=x,y`: the pointer stands there (tooltips, hover looks).
fn pointer() -> Option<(f32, f32)> {
    let s = std::env::var("RAZDOR_MOUSE").ok()?;
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// `RAZDOR_SCENE_PAN=dx,dy`: the interface's pan holds that offset (a window panned over the
/// screen at an interface scale above the fit, `ui::pan`).
fn pan_offset() -> Option<(f32, f32)> {
    let s = std::env::var("RAZDOR_SCENE_PAN").ok()?;
    let (x, y) = s.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

/// `replay:<step>`: the action list `RAZDOR_REPLAY` (the diff test's, `razdor --replay`)
/// played up to and including action `<step>` (all of it without a step), and its screen:
/// the map, the building window or the battle. Dialogs still waiting are not shown.
fn stage_replay(app: &mut App, step: Option<&str>) -> Result<(), String> {
    use razdor::difftest::{read_action_list, Runner, Source};
    let list = std::env::var("RAZDOR_REPLAY").map_err(|_| "RAZDOR_REPLAY names no action list")?;
    let actions = read_action_list(std::path::Path::new(&list), None, None)?;
    let last = match step {
        Some(s) => s.parse::<usize>().map_err(|_| "bad step")?,
        None => actions.len().saturating_sub(1),
    };
    let dt = app.assets.dt.as_ref().ok_or("no install")?;
    let mut r = Runner::new(Source::Install(&dt.install));
    for a in actions.iter().take(last + 1) {
        r.apply(a)?;
    }
    let (mut game, battle, building) = r.into_view().ok_or("the list has no new_game")?;
    game.pos = game.world.map.center(game.tile());
    app.screen = match (battle, building) {
        (Some(b), _) => Screen::Battle(Box::new(BattleView::new(*b))),
        (None, true) => {
            let loc = game.location.map(|l| &game.world.locations[l]).ok_or("no building")?;
            Screen::Building(BuildingView::new(first_tab(loc, &game.content).ok_or("the building has no window")?))
        }
        (None, false) => Screen::WorldMap,
    };
    game.look_around();
    app.assets.set_content(game.content.clone());
    app.dt_content = Some(game.content.clone());
    app.game = Some(game);
    Ok(())
}

/// `RAZDOR_SCENE_OPEN=<frame>:<window>[,<frame>:<window>…]`: at that frame the window
/// opens over the scene, as its button opens it (`load`, `army`, `spells`, `journal`,
/// `menu`, `building:<n>`; `map` closes it), for the frame timer (`RAZDOR_PROFILE`) to time
/// its opening. Returns true at such a frame.
pub fn open_later(app: &mut App, frame: u64) -> bool {
    let Ok(specs) = std::env::var("RAZDOR_SCENE_OPEN") else { return false };
    let Some((spec, what)) = specs.split(',').find_map(|spec| {
        let (at, what) = spec.split_once(':')?;
        (at.trim().parse::<u64>().ok() == Some(frame)).then_some((spec, what))
    }) else {
        return false;
    };
    let started = std::time::Instant::now();
    let mut parts = what.split(':');
    let next = match parts.next().unwrap_or_default() {
        "load" => Some(Screen::Load(LoadView::new(if app.game.is_some() { Back::Map } else { Back::Title }))),
        "army" => Some(Screen::Squad { selected: Default::default(), scroll: 0, back: None }),
        "spells" => Some(Screen::Spellbook { selected: 0, page: 0, filter: Default::default() }),
        "journal" => Some(Screen::Hero(Default::default())),
        "hero" => Some(Screen::Hero(Default::default())),
        "menu" => Some(Screen::Menu(false)),
        "map" => Some(Screen::WorldMap),
        "building" => app.game.as_mut().and_then(|game| {
            let l: usize = parts.next()?.parse().ok()?;
            let tab = first_tab(game.world.locations.get(l)?, &game.content)?;
            game.location = Some(l);
            Some(Screen::Building(BuildingView::new(tab)))
        }),
        _ => None,
    };
    match next {
        Some(next) => {
            app.screen = next;
            razdor::diag!("opened {what}: {:.1} ms to set up", started.elapsed().as_secs_f64() * 1000.0);
            true
        }
        None => {
            razdor::diag!("RAZDOR_SCENE_OPEN={spec}: cannot open that here");
            false
        }
    }
}

/// Sets up the scene of `RAZDOR_SCENE`, if any; a scene that cannot be set up is reported.
pub fn stage(app: &mut App) {
    super::widgets::set_pointer(pointer());
    super::pan::force_for_snapshot(pan_offset());
    // `RAZDOR_MOUSE_RIGHT=1`: the right button held where `RAZDOR_MOUSE` stands (the map's
    // tooltips, as the original shows them while it is held).
    super::input::hold_right_for_snapshot(std::env::var("RAZDOR_MOUSE_RIGHT").is_ok_and(|v| v == "1"));
    let Ok(scene) = std::env::var("RAZDOR_SCENE") else { return };
    // Reproducible menu, save and stacked-trait screenshots without changing defaults.
    if let Ok(flags) = std::env::var("RAZDOR_SCENE_EXPERIMENTAL") {
        let has = |key| flags.split(',').any(|f| f.trim() == key);
        super::new_game::snapshot_experimental(razdor::rules::content::ExperimentalRules {
            simultaneous_bonuses: has("bonuses"), multiple_rings: has("rings"), multiple_amulets: has("amulets"), allow_duplicate_items: !has("unique"), two_potions: has("potions"), six_cells: has("cells"),
        });
    }
    if let Err(e) = try_stage(app, &scene) {
        razdor::diag!("RAZDOR_SCENE={scene}: {e}");
    }
    // `RAZDOR_SCENE_ARCHIVES=a.7z,b.zip`: the not-found block with those game archives found;
    // `RAZDOR_SCENE_UNPACK=<pct>` with it: the unpacking at that percentage (`ui::mobile`).
    if let Ok(names) = std::env::var("RAZDOR_SCENE_ARCHIVES") {
        super::mobile::snapshot_archives(&names, std::env::var("RAZDOR_SCENE_UNPACK").ok().and_then(|p| p.parse().ok()));
    }
    if std::env::var("RAZDOR_SCENE_RULES").is_ok_and(|v| v == "1") { super::new_game::open_experimental_menu(); }
    // `RAZDOR_SCENE_BATTLE_EXIT=1`: a battle scene with its leave-battle window open (Esc).
    if std::env::var("RAZDOR_SCENE_BATTLE_EXIT").is_ok_and(|v| v == "1") {
        if let Screen::Battle(v) | Screen::CustomBattle(v) = &mut app.screen {
            v.open_exit_window();
        }
    }
    // `RAZDOR_SCENE_FILTER=меч`: the inventory filter (army screen, market) holds that.
    if let Ok(q) = std::env::var("RAZDOR_SCENE_FILTER") {
        super::items_view::set_pack_filter(&q);
        if let Screen::Building(v) = &mut app.screen {
            v.filter = q;
        }
    }
    // `RAZDOR_SCENE_PACK_VIEW=weapons,price-up`: the backpack's group and order (army screen)
    // and the market's group, as the player chose them (`rules::pack_order`).
    if let Some((group, sort)) = pack_view_from_env() {
        super::items_view::set_pack_view(group, sort);
        if let Screen::Building(v) = &mut app.screen {
            v.group = group;
        }
    }
    // `RAZDOR_SCENE_CONSOLE=help;gold 100`: the cheat console open, after those commands.
    if let Ok(lines) = std::env::var("RAZDOR_SCENE_CONSOLE") {
        app.console.open = true;
        for line in lines.split(';').map(str::trim).filter(|l| !l.is_empty()) {
            app.console.print(format!("> {line}"), super::console::Kind::Typed);
            if let Some(next) = app.run_cheat(line) {
                app.screen = next;
            }
        }
    }
}

fn try_stage(app: &mut App, scene: &str) -> Result<(), String> {
    let mut parts = scene.split(':');
    let kind = parts.next().unwrap_or_default();
    match kind {
        "title" => return Ok(()),
        // `fonts`: sample lines in many scripts over the title screen (`draw_overlay`).
        "fonts" => {
            FONTS_SCENE.with(|f| f.set(true));
            return Ok(());
        }
        // `update`: the title screen with a made-up newer release on offer.
        "update" => {
            super::update_view::stage_offer();
            return Ok(());
        }
        "authors" => {
            // `authors:<seconds>`: that far into the credits' scroll (25 by default).
            let into = parts.next().and_then(|t| t.parse::<f64>().ok()).unwrap_or(25.0);
            app.screen = Screen::Authors(crate::ui::clock::now() - into);
            return Ok(());
        }
        "options" => {
            app.screen = Screen::Options;
            // `options:advanced`: Razdor's advanced settings over it.
            if parts.next() == Some("advanced") {
                super::main_menu::open_advanced();
            }
            return Ok(());
        }
        "scenarios" => {
            // `scenarios:<map>`: that map's row picked.
            if let Some(name) = parts.next() {
                super::new_game::pick_map(name);
            }
            app.screen = Screen::ScenarioSelect;
            return Ok(());
        }
        "tutorial" => {
            app.screen = Screen::TutorialOffer;
            return Ok(());
        }
        "load" => {
            let mut view = LoadView::new(Back::Title);
            // `load:auto`: the autosaves' tab; `load:<tab>:<row>`: scrolled to that row.
            if parts.next() == Some("auto") {
                view.tab = razdor::rules::save::SaveKind::Auto;
                view.refresh();
            }
            if let Some(n) = parts.next().and_then(|n| n.parse().ok()) {
                view.scroll = n;
            }
            app.screen = Screen::Load(view);
            return Ok(());
        }
        "editor" => {
            app.open_editor();
            // `editor:<map>[:find text]`: that map of the install open, the find window
            // searching for the text.
            if let (Some(map), Some(ed)) = (parts.next(), app.editor.as_mut()) {
                let path = app.scenarios.iter().find(|e| e.file == map).map(|e| e.path.clone()).ok_or_else(|| format!("no map {map} in the install"))?;
                ed.open_for_snapshot(path, parts.next());
            }
            return Ok(());
        }
        "custom" => {
            app.open_custom();
            return Ok(());
        }
        "custom-battle" => {
            app.open_custom();
            let Some(Screen::CustomBattle(mut view)) = app.custom_round() else { return Err("no custom battle".into()) };
            if parts.next() == Some("watch") {
                view.watch_for_snapshot();
            }
            app.screen = Screen::CustomBattle(view);
            return Ok(());
        }
        "replay" => return stage_replay(app, parts.next()),
        _ => {}
    }
    let map = parts.next().ok_or("no map named")?;
    let index = app.scenarios.iter().position(|e| e.file == map).ok_or_else(|| format!("no map {map} in the install"))?;
    if kind == "classes" {
        app.screen = Screen::ClassSelect { scenario: Some(index) };
        return Ok(());
    }
    let content = app.dt_content.clone().ok_or("no install content")?;
    let content = std::sync::Arc::new(content.with_experimental(super::new_game::experimental_rules()));
    let mut game = Game::from_scenario(content, &app.scenarios[index].scenario, HeroClass::Knight);
    // The event generator's fixed seed (its clock seed would differ on every run).
    game.seed_events(0x5EED);
    // `RAZDOR_SCENE_MAX=1`: the battles of the scene are «Максимум»'s (`Game::max_ai`, as a new game takes it).
    if std::env::var("RAZDOR_SCENE_MAX").is_ok_and(|v| v == "1") {
        game.max_ai = true;
    }
    if let Some(index)=std::env::var("RAZDOR_SCENE_BENCHED").ok().and_then(|s| s.parse::<usize>().ok()).filter(|&i| i>0) {
        if let Some(unit)=game.squad.get_mut(index) { unit.battle_disabled=true; }
    }
    if std::env::var("RAZDOR_SCENE_BONUS_ITEMS").is_ok_and(|v| v == "1") {
        let c=game.content.clone();
        let mut seen=game.hero().stats(&c).bonuses;
        for item in &c.items {
            let Some(bonus)=item.bonus.as_ref().filter(|b| !seen.contains(b)) else { continue };
            if let Ok(slot)=razdor::rules::items::slot_for(&c,&game.squad[0],razdor::rules::content::ItemId(item.id)) {
                game.squad[0].items[slot]=Some(razdor::rules::content::ItemId(item.id));
                seen.push(bonus.clone());
            }
        }
    }
    // `RAZDOR_SCENE_QUIET=1`: the map's opening windows are not shown.
    if std::env::var_os("RAZDOR_SCENE_QUIET").is_some() {
        game.drain_events();
    }
    // `RAZDOR_SCENE_HURT=<percent>`: the hero's army keeps that share of its hits (wounds).
    if let Some(pct) = std::env::var("RAZDOR_SCENE_HURT").ok().and_then(|v| v.trim().parse::<i32>().ok()) {
        let content = game.content.clone();
        for (i, u) in game.squad.iter_mut().enumerate() {
            // A different share for each, to see the fill vary.
            u.hp = (u.max_hp(&content) * (pct - 15 * i as i32).clamp(0, 100) / 100).max(1);
        }
    }
    // `RAZDOR_SCENE_TARGET=<spell id>`: the map is in the targeting of that enemy spell, as after
    // a click in the book (`world_view::begin_targeting`), to see the aim's ring and hint.
    if let Some(id) = std::env::var("RAZDOR_SCENE_TARGET").ok().and_then(|v| v.trim().parse::<u32>().ok()) {
        super::world_view::begin_targeting(id);
    }
    // `RAZDOR_SCENE_SPELLS=<id>,<id>…`: those spells running on every unit of the hero's army
    // and of the map's armies (the n-th for 10 h + n days), to see the cards' spell badges.
    if let Ok(v) = std::env::var("RAZDOR_SCENE_SPELLS") {
        let ids: Vec<u32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        let now = game.clock.total_minutes() as u64;
        let slots: Vec<_> = ids.iter().enumerate().map(|(n, &spell)| razdor::rules::units::SpellSlot { spell, until: now + 600 + n as u64 * 1440 }).collect();
        let fill = |spells: &mut [Option<razdor::rules::units::SpellSlot>]| {
            for (slot, s) in spells.iter_mut().zip(&slots) {
                *slot = Some(*s);
            }
        };
        for u in &mut game.squad {
            fill(&mut u.spells);
            u.drain = 20;
        }
        for t in game.world.armies.iter_mut().flat_map(|a| a.troops.iter_mut()) {
            fill(&mut t.spells);
        }
    }
    // `RAZDOR_SCENE_POTION=1`: every unit of the hero's army has drunk the install's first
    // potion (the cards' potion sign; its effect is not applied).
    if std::env::var("RAZDOR_SCENE_POTION").is_ok_and(|v| !v.is_empty()) {
        let potion = game.content.items.iter().find(|d| d.kind == razdor::dt::data::ArtefactType::Potion).map(|d| razdor::rules::content::ItemId(d.id));
        for u in game.squad.iter_mut() {
            u.potions.extend(potion);
        }
    }
    // `RAZDOR_SCENE_ITEMS=<id>,<id>…`: those items in the hero's pack (the army window's
    // pack, its item hover and pick).
    if let Ok(v) = std::env::var("RAZDOR_SCENE_ITEMS") {
        for item in v.split(',').filter_map(|p| p.trim().parse::<u32>().ok()).map(razdor::rules::content::ItemId) {
            game.pack.add(item);
        }
    }
    // `RAZDOR_SCENE_PACK_AT=<cell>:<id>,…`: those items in those pack cells (holes between
    // them); `RAZDOR_SCENE_HELD=<id>`: that item on the pointer.
    if let Ok(v) = std::env::var("RAZDOR_SCENE_PACK_AT") {
        for (cell, id) in v.split(',').filter_map(|p| p.split_once(':')) {
            if let (Ok(cell), Ok(id)) = (cell.trim().parse::<usize>(), id.trim().parse::<u32>()) {
                game.pack.set(cell.min(razdor::rules::game::PACK_SIZE - 1), Some(razdor::rules::content::ItemId(id)));
            }
        }
    }
    if let Some(id) = std::env::var("RAZDOR_SCENE_HELD").ok().and_then(|v| v.trim().parse::<u32>().ok()) {
        game.held_item = Some(razdor::rules::content::ItemId(id));
    }
    // `RAZDOR_SCENE_DEBUG=1`: the map's debug overlay (F3) on.
    if std::env::var("RAZDOR_SCENE_DEBUG").is_ok_and(|v| !v.is_empty()) {
        app.map_view.debug = true;
    }
    let arg = parts.next();
    let n = || arg.and_then(|a| a.parse::<usize>().ok()).ok_or("no index given");
    app.screen = match kind {
        "minimap" => {
            // The whole map explored, to see the minimap's colours and symbols.
            for y in 0..game.world.map.h {
                for x in 0..game.world.map.w {
                    game.fog.mark((x, y));
                }
            }
            app.map_view.minimap = true;
            Screen::WorldMap
        }
        "clash" => {
            // The whole map explored and the first two armies fight (the AI's battle): the map
            // shows the crossed-swords mark and the notice of Battles on the map.
            for y in 0..game.world.map.h {
                for x in 0..game.world.map.w {
                    game.fog.mark((x, y));
                }
            }
            if game.world.armies.len() >= 2 {
                game.ai_battle(0, razdor::rules::ai::Defender::Army(1));
            }
            Screen::WorldMap
        }
        "map" => {
            if let Some((x, y)) = arg.and_then(|a| a.split_once(',')) {
                let t = (x.parse().map_err(|_| "bad x")?, y.parse().map_err(|_| "bad y")?);
                game.pos = game.world.map.center(t);
            }
            Screen::WorldMap
        }
        "walk" => {
            let (x, y) = arg.and_then(|a| a.split_once(',')).ok_or("no offset dx,dy")?;
            let (dx, dy): (i32, i32) = (x.parse().map_err(|_| "bad dx")?, y.parse().map_err(|_| "bad dy")?);
            let t = (game.tile().0 + dx, game.tile().1 + dy);
            game.look_around();
            if !game.set_destination(t) {
                return Err("no way there".into());
            }
            Screen::WorldMap
        }
        "building" => {
            let l = n()?;
            let loc = game.world.locations.get(l).ok_or("no such building")?;
            let mut tab = first_tab(loc, &game.content).ok_or("the building has no window")?;
            // `building:<map>:<n>:barracks`: that tab.
            match parts.next() {
                Some("barracks") => tab = razdor::rules::town::Tab::Barracks,
                Some("garrison") => tab = razdor::rules::town::Tab::Garrison,
                Some("market") | Some("market-pack") => tab = razdor::rules::town::Tab::Market,
                Some("sanctuary") => tab = razdor::rules::town::Tab::Sanctuary,
                _ => {}
            }
            // `…:market-pack`: the market showing the pack ("Инвентарь").
            let pack = scene.ends_with(":market-pack");
            game.pos = game.world.map.center(loc.anchor);
            game.location = Some(l);
            // The map's opening windows are not shown over the building. Their places are
            // cleared too: a place still due (the start's flight to the capital) makes the
            // building window step aside for the map (`world_view::building_flight`), which
            // left the scene a black map with no window.
            game.drain_events();
            game.shown.clear();
            let mut view = BuildingView::new(tab);
            view.selling |= pack;
            // `…:sanctuary:<row>` (or any tab): that row of its list picked; the main hall's
            // list is its quests and rumours.
            let row = parts.next().and_then(|p| p.parse().ok());
            if tab == razdor::rules::town::Tab::MainHall {
                view.hall_pick = row;
                // `RAZDOR_SCENE_TAKE=1`: that row taken («Взять задание»), its windows shown.
                if std::env::var_os("RAZDOR_SCENE_TAKE").is_some() {
                    if let Some(id) = row.and_then(|r| game.hall_here().get(r).copied()) {
                        let events = game.take_hall_entry(id).map_err(|e| format!("{e:?}"))?;
                        let mut message = None;
                        super::world_view::handle_events(&mut game, events, &mut message, &mut app.dialogs);
                        view.hall_pick = None;
                    }
                }
            } else {
                view.pick = row;
            }
            Screen::Building(view)
        }
        "army" => {
            // `army:<map>:<n>[:<unit id>[:<level>]]`: squad member n pressed (its promotion
            // tree up), first made a unit of that `GlobalIndex` at that level (default 2).
            let mut selected = super::items_view::ArmySel::default();
            if let Some(i) = arg.and_then(|a| a.parse::<usize>().ok()) {
                if i >= game.squad.len() {
                    return Err("no such squad member".into());
                }
                if let Some(id) = parts.next().and_then(|v| v.parse::<u32>().ok()) {
                    let def = razdor::rules::content::UnitId(id);
                    game.content.try_unit(def).ok_or("no such unit")?;
                    let slot = game.squad[i].slot;
                    let mut u = razdor::rules::units::Unit::new(&game.content, def, slot);
                    u.level = parts.next().and_then(|v| v.parse().ok()).unwrap_or(2);
                    game.squad[i] = u;
                }
                // `RAZDOR_SCENE_WEAR=<id>,<id>…`: those items worn by the squad member n.
                if let Ok(v) = std::env::var("RAZDOR_SCENE_WEAR") {
                    let c = game.content.clone();
                    for id in v.split(',').filter_map(|p| p.trim().parse::<u32>().ok()).map(razdor::rules::content::ItemId) {
                        if let Ok(slot) = razdor::rules::items::slot_for(&c, &game.squad[i], id) {
                            game.squad[i].items[slot] = Some(id);
                        }
                    }
                }
                selected = super::items_view::ArmySel { selected: Some(i), shown: Some(i) };
            }
            Screen::Squad { selected, scroll: 0, back: None }
        }
        // `hero:<map>[:<tab>]`: the diary on tab 0..3 (quests, completed, rumours, messages).
        "journal" | "hero" => {
            let tab = arg.and_then(|t| t.parse::<usize>().ok()).and_then(|i| razdor::rules::journal::Tab::ALL.get(i).copied());
            Screen::Hero(super::items_view::HeroView { tab: tab.unwrap_or(razdor::rules::journal::Tab::Active), ..Default::default() })
        }
        "spells" | "spells-pages" => {
            // A book full of the install's spells, to see their pictures. `spells-pages`: 20 of
            // them, with the second page open (Razdor's paged book, the owner's 2026-10-22).
            let (count, page) = if kind == "spells-pages" { (20, 1) } else { (15, 0) };
            game.spells = game.content.spells.iter().filter_map(|s| u8::try_from(s.id).ok()).take(count).collect();
            // `RAZDOR_SCENE_MANA=<n>` sets the hero's mana (default 500: every card affordable).
            game.mana = std::env::var("RAZDOR_SCENE_MANA").ok().and_then(|v| v.trim().parse().ok()).unwrap_or(500);
            Screen::Spellbook { selected: 0, page, filter: Default::default() }
        }
        "menu" => Screen::Menu(false),
        "save" => {
            // `save:<map>`: the save window over the map (the bar's save button).
            Screen::Save(super::saves::SaveView::new(&game, Back::Map))
        }
        "victory" => {
            // `victory:<map>[:<gold>]`: the report after a won battle over the map
            // (Dialog::victory), the map's first army beaten, `gold` won (default 160).
            let gold = arg.and_then(|a| a.parse::<i32>().ok()).unwrap_or(160);
            let name = game.world.armies.first().map(|a| a.name.clone()).unwrap_or_default();
            let result = razdor::rules::game::BattleResult::Victory {
                reward: gold,
                mana: 0,
                lost: 0,
                loot: Vec::new(),
                left_behind: 0,
                level_ups: Vec::new(),
                captured: None,
                over: razdor::rules::game::Beat::Army(name),
            };
            app.dialogs.extend(super::dialog::Dialog::victory(&result));
            Screen::WorldMap
        }
        "report" => {
            // `report:<map>[:<gold>]`: the noon report over the map (Dialog::day_report), the
            // gold `gold` (default 19260), mana 2000, income 500 and wages 595.
            let gold = arg.and_then(|a| a.parse::<i32>().ok()).unwrap_or(19260);
            let report = razdor::rules::game::DayReport { day: 0, income: 500, wages: 595, mana_wages: 0, unpaid: 0, deserted: Vec::new(), gold, mana_total: 2000 };
            app.dialogs.push_back(super::dialog::Dialog::day_report(&game, &report));
            Screen::WorldMap
        }
        "defeat" => {
            // `defeat:<map>`: the report after a lost battle over the map (Dialog::defeat).
            app.dialogs.push_back(super::dialog::Dialog::defeat());
            Screen::WorldMap
        }
        "message" => {
            // `message:<map>`: a plain message window with the text of `RAZDOR_SCENE_TEXT` (a
            // short or a long one), title `RAZDOR_SCENE_TITLE`, over the map.
            let mut d = super::dialog::Dialog::new(std::env::var("RAZDOR_SCENE_TITLE").unwrap_or_else(|_| "Сообщение".into()));
            d.text = vec![std::env::var("RAZDOR_SCENE_TEXT").unwrap_or_default()];
            app.dialogs.push_back(d);
            Screen::WorldMap
        }
        "event" => {
            // `event:<map>:<n>`: the map's event n as its window opens (story::event_dialog),
            // with the lost row of the items in `RAZDOR_SCENE_LOST=<id>,…` (what the engine
            // reports as taken, `EventOutcome::ItemsLost`).
            let id = n()? as u16;
            let mut d = super::story::event_dialog(&game, id, false);
            if let Ok(v) = std::env::var("RAZDOR_SCENE_LOST") {
                d.lost = v.split(',').filter_map(|p| p.trim().parse::<u32>().ok()).map(razdor::rules::content::ItemId).collect();
            }
            app.dialogs.push_back(d);
            Screen::WorldMap
        }
        "learnt" => {
            // `learnt:<map>[:<id>,…]`: the event window «Выучены новые заклинания» with those
            // spells (default: the first two of the install), the hero with no mana, over the map.
            // The spell icons' preview is the card at the pointer (RAZDOR_MOUSE).
            let ids: Vec<u32> = match arg {
                Some(a) => a.split(',').filter_map(|v| v.trim().parse().ok()).collect(),
                None => game.content.spells.iter().take(2).map(|s| s.id).collect(),
            };
            game.mana = 0;
            let mut d = super::dialog::Dialog::new("Hermit's gift");
            // `RAZDOR_SCENE_PREVIEW=1`: the card of the first spell is up at RAZDOR_MOUSE, as after
            // a press on its icon (the press's edge is never seen in a snapshot).
            if let (Some((x, y)), Some(&first)) = (pointer(), ids.first()) {
                if std::env::var("RAZDOR_SCENE_PREVIEW").is_ok_and(|v| v == "1") {
                    d.preview.set(Some((true, first, macroquad::math::Vec2::new(x, y))));
                }
            }
            d.spells = ids;
            app.dialogs.push_back(d);
            Screen::WorldMap
        }
        "settings" => {
            // `settings:<map>[:advanced]`: the settings over the map (the bar's gears), with
            // Razdor's advanced settings over them.
            if arg == Some("advanced") {
                super::main_menu::open_advanced();
            } else if arg == Some("keys") {
                super::main_menu::open_keys();
            }
            Screen::Settings
        }
        "battle" => {
            let i = n()?;
            if i >= game.world.armies.len() {
                return Err("no such army".into());
            }
            game.foe = Some(Foe::Army(i));
            Screen::Battle(Box::new(BattleView::new(game.start_battle())))
        }
        _ => return Err(format!("unknown scene {kind}")),
    };
    game.look_around();
    // `RAZDOR_SCENE_SHOW=x,y,r`: an event shows that place (as a lantern does), to see the
    // camera fly there and the area fade in (or, explored already, Razdor's red ring).
    if let Some((x, y, r)) = std::env::var("RAZDOR_SCENE_SHOW").ok().and_then(|v| {
        let n: Vec<i32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (n.len() == 3).then(|| (n[0], n[1], n[2]))
    }) {
        game.reveal_area((x, y), r);
    }
    app.assets.set_content(game.content.clone());
    app.game = Some(game);
    Ok(())
}
