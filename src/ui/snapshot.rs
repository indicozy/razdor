//! Debug snapshots, for comparing screens with the original without playing to them:
//! `RAZDOR_SCENE` sets the screen up, `RAZDOR_SNAPSHOT=<file.png>` saves the frame after
//! `RAZDOR_SNAPSHOT_FRAMES` frames (default 20) and quits. Meant for an invisible display,
//! e.g. `xvfb-run -s "-screen 0 1024x768x24" razdor` with `RAZDOR_SIZE=1024x768`.
//!
//! Scenes (`<map>` is a map file name of the install without `.DTm`, e.g. `РК3-Столица`):
//! `title`, `authors`, `options`, `scenarios`, `tutorial`, `load`, `editor[:<what>[:<map>]]`
//! (`<what>`: `units`, `artefacts`, `options`, `settings`, `events`, `grid`, `fog`, `records`
//! (the buildings submenu), `page<k>` (tool page 0–4), or a record `a<n>`, `b<n>`, `p<n>` of
//! `<map>` opened in the editor), `classes:<map>`, `map:<map>[:x,y]`, `minimap:<map>`, `walk:<map>:dx,dy` (the
//! hero sets off that many cells away), `building:<map>:<n>`
//! (the hero in the n-th building), `army:<map>`, `journal:<map>`, `spells:<map>`,
//! `menu:<map>`, `battle:<map>:<n>` (against the n-th army). `RAZDOR_SCENE_SHOW=x,y,r` shows
//! a place as a lantern event does; `RAZDOR_SCENE_QUIET=1` drops
//! the scenario's messages every frame, to see the screen under them; `RAZDOR_MOUSE=x,y`
//! puts the pointer there.

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

/// Sets up the scene of `RAZDOR_SCENE`, if any; a scene that cannot be set up is reported.
pub fn stage(app: &mut App) {
    super::widgets::set_pointer(pointer());
    let Ok(scene) = std::env::var("RAZDOR_SCENE") else { return };
    if let Err(e) = try_stage(app, &scene) {
        razdor::diag!("RAZDOR_SCENE={scene}: {e}");
    }
}

fn try_stage(app: &mut App, scene: &str) -> Result<(), String> {
    let mut parts = scene.split(':');
    let kind = parts.next().unwrap_or_default();
    match kind {
        "title" => return Ok(()),
        "authors" => {
            app.screen = Screen::Authors(-25.0);
            return Ok(());
        }
        "options" => {
            app.screen = Screen::Options;
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
            app.screen = Screen::Load(LoadView::new(Back::Title));
            return Ok(());
        }
        "editor" => {
            app.open_editor();
            let what = parts.next().unwrap_or_default();
            let map = parts.next().map(|m| app.scenarios.iter().find(|e| e.file == m).map(|e| e.path.clone()).ok_or_else(|| format!("no map {m} in the install"))).transpose()?;
            if let Some(ed) = app.editor.as_mut() {
                ed.stage(what, map)?;
            }
            return Ok(());
        }
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
    // `RAZDOR_SCENE_SHOW=x,y,r`: an event shows that place (as a lantern does), to see the
    // camera fly there and the area fade in.
    if let Some((x, y, r)) = std::env::var("RAZDOR_SCENE_SHOW").ok().and_then(|v| {
        let n: Vec<i32> = v.split(',').filter_map(|p| p.trim().parse().ok()).collect();
        (n.len() == 3).then(|| (n[0], n[1], n[2]))
    }) {
        game.reveal_area((x, y), r);
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
                _ => {}
            }
            game.pos = game.world.map.center(loc.anchor);
            game.location = Some(l);
            Screen::Building(BuildingView::new(tab))
        }
        "army" => Screen::Squad { selected: 0, scroll: 0, back: None },
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
    app.assets.set_content(game.content.clone());
    app.game = Some(game);
    Ok(())
}
