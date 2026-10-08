//! Debug snapshots, for comparing screens with the original without playing to them:
//! `RAZDOR_SCENE` sets the screen up, `RAZDOR_SNAPSHOT=<file.png>` saves the frame after
//! `RAZDOR_SNAPSHOT_FRAMES` frames (default 20) and quits. Meant for an invisible display,
//! e.g. `xvfb-run -s "-screen 0 1024x768x24" razdor` with `RAZDOR_SIZE=1024x768`.
//!
//! Scenes (`<map>` is a map file name of the install without `.DTm`, e.g. `РК3-Столица`):
//! `title`, `update` (a made-up release on offer), `authors[:<seconds>]`, `options[:advanced]`, `scenarios`, `tutorial`, `load[:manual|auto[:<row>]]`, `editor[:<map>[:<text to find>]]`, `classes:<map>`, `map:<map>[:x,y]`, `minimap:<map>`, `walk:<map>:dx,dy` (the
//! hero sets off that many cells away), `building:<map>:<n>`
//! (the hero in the n-th building; `:barracks`, `:garrison`, `:market` open that tab), `army:<map>[:<n>[:<unit id>[:<level>]]]` (squad member n selected, made that unit at that level), `journal:<map>`, `spells:<map>`,
//! `menu:<map>`, `battle:<map>:<n>` (against the n-th army), `custom` (the custom battle
//! setup), `custom-battle` (its first round; `custom-battle:watch` watches the AI play it). `RAZDOR_SCENE_DEBUG=1` turns the map's debug overlay (F3) on; `RAZDOR_SCENE_SHOW=x,y,r` shows
//! a place as a lantern event does; `RAZDOR_SCENE_CONSOLE=help;gold 100` opens the cheat
//! console after those commands; `RAZDOR_SCENE_FILTER=text` fills the inventory filter
//! (army screen, market); `RAZDOR_SCENE_QUIET=1` drops
//! the scenario's messages every frame, to see the screen under them; `RAZDOR_MOUSE=x,y`
//! puts the pointer there; `RAZDOR_SCENE_SPELLS=<id>,…` puts those spells on every unit; `RAZDOR_SCENE_POTION=1` gives every unit of the army a
//! drunk potion. `replay:<step>` with `RAZDOR_REPLAY=<actions.jsonl>`: the diff
//! test's action list played to that step.
//!
//! Every map scene starts its map as a new game (`Game::from_scenario`): nothing is carried
//! over from a campaign's map before, so a later map whose opening events check for what it
//! should have brought ends at once by its own defeat event (РК2 does without the herald,
//! so `battle:РК2-…:0` shows the defeat screen). The game is not at fault there.

use razdor::rules::content::HeroClass;
use razdor::rules::game::{Foe, Game};
use razdor::rules::town::first_tab;

use super::battle_view::BattleView;
use super::building_view::BuildingView;
use super::saves::{Back, LoadView};
use super::story::JournalView;
use super::{App, Screen};

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

/// `RAZDOR_MOUSE=x,y`: the pointer stands there (tooltips, hover looks).
fn pointer() -> Option<(f32, f32)> {
    let s = std::env::var("RAZDOR_MOUSE").ok()?;
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

/// Sets up the scene of `RAZDOR_SCENE`, if any; a scene that cannot be set up is reported.
pub fn stage(app: &mut App) {
    super::widgets::set_pointer(pointer());
    let Ok(scene) = std::env::var("RAZDOR_SCENE") else { return };
    if let Err(e) = try_stage(app, &scene) {
        razdor::diag!("RAZDOR_SCENE={scene}: {e}");
    }
    // `RAZDOR_SCENE_FILTER=меч`: the inventory filter (army screen, market) holds that.
    if let Ok(q) = std::env::var("RAZDOR_SCENE_FILTER") {
        super::items_view::set_pack_filter(&q);
        if let Screen::Building(v) = &mut app.screen {
            v.filter = q;
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
        // `update`: the title screen with a made-up newer release on offer.
        "update" => {
            super::update_view::stage_offer();
            return Ok(());
        }
        "authors" => {
            // `authors:<seconds>`: that far into the credits' scroll (25 by default).
            let into = parts.next().and_then(|t| t.parse::<f64>().ok()).unwrap_or(25.0);
            app.screen = Screen::Authors(macroquad::prelude::get_time() - into);
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
    let mut game = Game::from_scenario(content, &app.scenarios[index].scenario, HeroClass::Knight);
    // `RAZDOR_SCENE_HURT=<percent>`: the hero's army keeps that share of its hits (wounds).
    if let Some(pct) = std::env::var("RAZDOR_SCENE_HURT").ok().and_then(|v| v.trim().parse::<i32>().ok()) {
        let content = game.content.clone();
        for (i, u) in game.squad.iter_mut().enumerate() {
            // A different share for each, to see the fill vary.
            u.hp = (u.max_hp(&content) * (pct - 15 * i as i32).clamp(0, 100) / 100).max(1);
        }
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
                Some("market") => tab = razdor::rules::town::Tab::Market,
                _ => {}
            }
            game.pos = game.world.map.center(loc.anchor);
            game.location = Some(l);
            Screen::Building(BuildingView::new(tab))
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
                selected = super::items_view::ArmySel { selected: Some(i), shown: Some(i) };
            }
            Screen::Squad { selected, scroll: 0, back: None }
        }
        "journal" => Screen::Journal(JournalView::default()),
        "spells" => {
            // A book full of the install's spells, to see their pictures.
            game.spells = game.content.spells.iter().filter_map(|s| u8::try_from(s.id).ok()).take(15).collect();
            game.mana = 500;
            Screen::Spellbook { selected: 0 }
        }
        "menu" => Screen::Menu(false),
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
