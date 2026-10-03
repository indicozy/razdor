//! The AI viewer's window (the original's TViewAI, docs/reference/editor/testers.md §3): the
//! map's overview with the armies as dots, the clock and its buttons, the hero choice, the
//! two subjects with their panels, the pair's scores and predicted battles, the overlays of
//! the first subject's army and the 9 × 9 value grid. The model is
//! `razdor::editor::viewer`; a new one is made at every opening, as the original reloads
//! the map's records.

use macroquad::prelude::*;

use razdor::editor::viewer::{self, Subject, Viewer, ViewerStop};
use razdor::i18n::tr;
use razdor::rules::content::HeroClass;
use razdor::rules::map::Tile;
use razdor::rules::world::Owner;
use razdor::trf;

use crate::ui::widgets::*;

pub enum ViewerAction {
    None,
    Close,
    /// Open the battle tester with these two armies (side 1, side 2).
    Battle(usize, usize),
}

/// Which overlay of the first subject's army is shown.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Overlay {
    None,
    Route,
    Density,
    Flood,
}

pub struct ViewerState {
    pub viewer: Viewer,
    /// The picks of the two lists (kept between openings, as the original's).
    pub picks: (usize, usize),
    running: bool,
    overlay: Overlay,
    maps: Option<(Vec<u16>, Vec<u16>)>,
    overlay_tex: Option<Texture2D>,
    grid: Option<[[Option<u16>; 9]; 9]>,
    /// The seed box: the next opening (or this one, before time runs) seeds from the clock.
    pub rnd: bool,
    note: Option<String>,
}

impl ViewerState {
    pub fn new(viewer: Viewer, picks: (usize, usize), rnd: bool) -> ViewerState {
        ViewerState { viewer, picks, running: false, overlay: Overlay::None, maps: None, overlay_tex: None, grid: None, rnd, note: None }
    }

    fn clear_overlay(&mut self) {
        self.overlay = Overlay::None;
        self.maps = None;
        self.overlay_tex = None;
        self.grid = None;
    }
}

fn hue(h: f32) -> Color {
    let h = (h.rem_euclid(360.0)) / 60.0;
    let x = 1.0 - (h % 2.0 - 1.0).abs();
    let (r, g, b) = match h as u32 {
        0 => (1.0, x, 0.0),
        1 => (x, 1.0, 0.0),
        2 => (0.0, 1.0, x),
        3 => (0.0, x, 1.0),
        4 => (x, 0.0, 1.0),
        _ => (1.0, 0.0, x),
    };
    Color::new(r, g, b, 1.0)
}

/// The overlay's picture of a W × H map: density (0 dark red, below 16 green, else blue) or
/// flood distance (0 dark red, else a hue by distance / 10).
fn overlay_texture(values: &[u16], w: i32, h: i32, density: bool) -> Texture2D {
    let max = values.iter().copied().filter(|&v| v != u16::MAX).max().unwrap_or(1).max(1) as f32;
    let mut rgba = Vec::with_capacity(values.len() * 4);
    for &v in values {
        let c = if v == 0 || v == u16::MAX {
            Color::new(0.5, 0.0, 0.0, 1.0)
        } else if density {
            if v < 16 {
                Color::new(0.0, (250.0 - v as f32 * 200.0 / 30.0) / 255.0, 0.0, 1.0)
            } else {
                Color::new(0.0, 0.0, (250.0 - v as f32 * 200.0 / max) / 255.0, 1.0)
            }
        } else {
            hue(v as f32 / 10.0)
        };
        let px: [u8; 4] = c.into();
        rgba.extend_from_slice(&px);
    }
    let tex = Texture2D::from_rgba8(w.max(1) as u16, h.max(1) as u16, &rgba);
    tex.set_filter(FilterMode::Nearest);
    tex
}

fn subject_name(v: &Viewer, s: Subject) -> String {
    let w = &v.game.world;
    match s {
        Subject::Hero => tr("Player's army").to_string(),
        Subject::Army(i) => w.armies.get(i).map_or_else(String::new, |a| format!("{} {}", a.id, a.name)),
        Subject::Building(l) => w.locations.get(l).map_or_else(String::new, |b| format!("{} {}", b.id, b.name)),
    }
}

fn stop_text(s: ViewerStop) -> String {
    match s {
        ViewerStop::Victory => tr("The victory event fired: the clock stops.").to_string(),
        ViewerStop::Defeat => tr("The defeat event fired: the clock stops.").to_string(),
        ViewerStop::RepeatUnderADay { event } => trf!("Event {event} repeats more often than once a day: the original divides by zero there; Razdor stopped the clock.", event),
        ViewerStop::EventLoop { event } => trf!("Event {event} kept firing in one scan: the original would never finish it; Razdor stopped the clock.", event),
    }
}

/// The panel of subject `s` from (x, y), `w` wide.
fn panel(v: &Viewer, s: Subject, x: f32, mut y: f32, w: f32) {
    let g = &v.game;
    let c = &g.content;
    let line = |t: &str, y: &mut f32, col: Color| {
        text_fit(t, x, *y, w, 14.0, col);
        *y += 17.0;
    };
    match s {
        Subject::Building(l) => {
            let b = &g.world.locations[l];
            let owner = match b.owner {
                Owner::Player => tr("player").to_string(),
                Owner::Army(id) => g.world.armies.iter().find(|a| a.id == id).map_or_else(|| id.to_string(), |a| a.name.clone()),
                Owner::Neutral => tr("none").to_string(),
            };
            line(&trf!("Gold {gold} +{income}, defence +{defence}", gold = b.treasure_gold, income = b.gold_income, defence = b.garrison_defence), &mut y, INK);
            line(&trf!("Owner: {owner}", owner), &mut y, INK);
            for t in &b.garrison {
                line(&format!("{} {}", c.unit(t.unit).name, t.level), &mut y, DIM);
            }
        }
        _ => {
            let Some(i) = v.army_of(s) else {
                line(tr("No hero."), &mut y, DIM);
                return;
            };
            let a = &g.world.armies[i];
            let lost: i32 = a.troops.iter().map(|t| t.hurt).sum();
            let now = g.clock.total_minutes();
            let busy = if a.mind.busy_until > now { format!("{:.0}", a.mind.busy_until - now) } else { tr("none").to_string() };
            line(&trf!("Active: {active}; HP lost {lost}; busy for {busy}", active = if razdor::rules::ai::managed(a) { tr("yes") } else { tr("no") }, lost, busy), &mut y, INK);
            line(&trf!("Gold {gold}; bank {bank} min", gold = a.gold, bank = a.budget), &mut y, INK);
            for t in &a.troops {
                let items: Vec<String> = t.worn.iter().flatten().map(|i| c.item(*i).name.clone()).collect();
                let mut l = format!("{} {} ({} XP)", c.unit(t.unit).name, t.level, t.xp);
                if !items.is_empty() {
                    l.push_str(&format!(": {}", items.join(", ")));
                }
                line(&l, &mut y, if t.alive() { DIM } else { Color::new(0.5, 0.3, 0.3, 1.0) });
            }
        }
    }
}

/// The window in rectangle `r`; `overview` is the map's terrain picture.
pub fn window(st: &mut ViewerState, r: Rect, overview: Option<&Texture2D>) -> ViewerAction {
    let mut action = ViewerAction::None;
    let (x0, y0) = (r.x + 16.0, r.y + 30.0);
    text(tr("AI viewer"), x0, y0, 22.0, ACCENT);
    let (w, h) = (st.viewer.game.world.map.w, st.viewer.game.world.map.h);
    let side = (r.h - 80.0).min(470.0);
    let k = side / w.max(h).max(1) as f32;
    let map = Rect::new(x0, y0 + 14.0, w as f32 * k, h as f32 * k);
    draw_rectangle(map.x, map.y, map.w, map.h, BLACK);
    if let Some(t) = overview {
        draw_texture_ex(t, map.x, map.y, WHITE, DrawTextureParams { dest_size: Some(vec2(map.w, map.h)), ..Default::default() });
    }
    if let Some(t) = &st.overlay_tex {
        draw_texture_ex(t, map.x, map.y, WHITE, DrawTextureParams { dest_size: Some(vec2(map.w, map.h)), ..Default::default() });
    }
    let cell = |t: Tile| (map.x + (t.0 as f32 + 0.5) * k, map.y + (t.1 as f32 + 0.5) * k);
    let firsts = st.viewer.first_list();
    let seconds = st.viewer.second_list();
    st.picks.0 = st.picks.0.min(firsts.len().saturating_sub(1));
    st.picks.1 = st.picks.1.min(seconds.len().saturating_sub(1));
    let (s1, s2) = (firsts[st.picks.0], seconds.get(st.picks.1).copied());
    let (a1, a2) = (st.viewer.army_of(s1), s2.and_then(|s| st.viewer.army_of(s)));
    {
        let g = &st.viewer.game;
        // Towns, castles and forts in their occupier's colour.
        for b in &g.world.locations {
            let col = match b.owner {
                Owner::Player => crate::ui::world_view::faction_color(1),
                _ => crate::ui::world_view::faction_color(b.faction),
            };
            let (x, y) = (map.x + (b.anchor.0 - b.size.0 + 1) as f32 * k, map.y + (b.anchor.1 - b.size.1 + 1) as f32 * k);
            draw_rectangle_lines(x, y, b.size.0 as f32 * k, b.size.1 as f32 * k, 1.0, col);
        }
        if st.overlay == Overlay::Route {
            if let Some(a) = a1.map(|i| &g.world.armies[i]) {
                for &t in &a.path {
                    let (x, y) = cell(t);
                    draw_circle(x, y, (k * 0.3).max(1.0), BLACK);
                }
            }
        }
        for (i, a) in g.world.armies.iter().enumerate() {
            if !razdor::rules::ai::managed(a) {
                continue;
            }
            let col = if Some(i) == a1 {
                BLACK
            } else if Some(i) == a2 {
                WHITE
            } else {
                crate::ui::world_view::faction_color(a.faction)
            };
            let (x, y) = cell(a.tile(&g.world.map));
            draw_circle(x, y, (k * 0.6).max(2.0), col);
        }
    }
    // The mouse readout and the 9 × 9 grid.
    let (mx, my) = pointer();
    let hover = mouse_in(map.x, map.y, map.w, map.h).then(|| (((mx - map.x) / k) as i32, ((my - map.y) / k) as i32));
    let values = st.maps.as_ref().map(|m| if st.overlay == Overlay::Density { &m.0 } else { &m.1 });
    if let Some((cx, cy)) = hover {
        let v = values.and_then(|vs| vs.get((cy * w + cx) as usize)).copied().unwrap_or(0);
        text_fit(&format!("X={cx} Y={cy} V={v}"), map.x, map.bottom() + 18.0, map.w, 15.0, INK);
        if clicked() {
            if let Some(vs) = values {
                st.grid = Some(viewer::grid9(vs, w, h, cx, cy));
            }
        }
    }
    // The right panel.
    let px = map.right() + 16.0;
    let pw = r.right() - px - 16.0;
    let mut y = y0 + 14.0;
    let v = &mut st.viewer;
    let now = v.now();
    text_fit(&trf!("{minutes} min since the start, {time}", minutes = v.elapsed(), time = viewer::format_time(now)), px, y + 14.0, pw, 16.0, INK);
    y += 24.0;
    let bw = (pw - 16.0) / 5.0;
    let labels = [tr("6 min"), tr("1 hour"), tr("6 hours")];
    for (n, (label, steps)) in labels.iter().zip(viewer::STEPS).enumerate() {
        if small_button(px + n as f32 * (bw + 4.0), y, bw, 26.0, label, v.stop.is_none() && !st.running) {
            v.steps(steps);
        }
    }
    if small_button(px + 3.0 * (bw + 4.0), y, bw, 26.0, tr("Run"), v.stop.is_none() && !st.running) {
        st.running = true;
    }
    if small_button(px + 4.0 * (bw + 4.0), y, bw, 26.0, tr("Stop"), st.running) {
        st.running = false;
    }
    y += 32.0;
    // The hero choice, locked once time has run.
    let heroes = [(None, tr("No hero")), (Some(HeroClass::Knight), tr("Knight")), (Some(HeroClass::Archmage), tr("Archmage")), (Some(HeroClass::Ranger), tr("Ranger"))];
    let hw = (pw - 12.0) / 4.0;
    for (n, (class, label)) in heroes.iter().enumerate() {
        let open = !v.ran && class.is_none_or(|c| v.offered(c));
        let on = v.hero == *class;
        let hx = px + n as f32 * (hw + 4.0);
        if open {
            if toggle_button(hx, y, hw, 24.0, label, on) {
                v.set_hero(*class);
                st.overlay = Overlay::None;
            }
        } else {
            small_button(hx, y, hw, 24.0, label, false);
        }
    }
    y += 30.0;
    if let Some(on) = checkbox(px, y, pw / 2.0, tr("Events"), v.events_on) {
        v.events_on = on;
    }
    if let Some(on) = checkbox(px + pw / 2.0, y, pw / 2.0, tr("Seed from the clock"), st.rnd) {
        st.rnd = on;
        st.note = Some(tr("Takes effect at the next opening of the viewer.").to_string());
    }
    y += 26.0;
    let [towns, castles, villages] = v.incomes();
    text_fit(&trf!("Daily income: towns {towns}, castles and forts {castles}, villages {villages}", towns, castles, villages), px, y + 14.0, pw, 14.0, DIM);
    y += 24.0;
    let entries1: Vec<(i64, String)> = firsts.iter().enumerate().map(|(n, s)| (n as i64, subject_name(v, *s))).collect();
    let entries2: Vec<(i64, String)> = seconds.iter().enumerate().map(|(n, s)| (n as i64, subject_name(v, *s))).collect();
    let half = (pw - 8.0) / 2.0;
    if let Some(n) = dropdown("viewer:first", px, y, half, st.picks.0 as i64, &entries1) {
        st.picks.0 = n as usize;
        st.clear_overlay();
    }
    if let Some(n) = dropdown("viewer:second", px + half + 8.0, y, half, st.picks.1 as i64, &entries2) {
        st.picks.1 = n as usize;
    }
    y += 30.0;
    let v = &st.viewer;
    panel(v, s1, px, y + 14.0, half);
    if let Some(s2) = s2 {
        panel(v, s2, px + half + 8.0, y + 14.0, half);
    }
    y += 200.0;
    // The pair: cached scores, predicted battles, the battle and debug buttons.
    if let (Some(a), Some(s2)) = (a1, s2) {
        if let Some((score, talk)) = v.scores(a, s2) {
            text_fit(&trf!("Scores: {score}, talk {talk}", score, talk), px, y + 14.0, pw, 15.0, INK);
            y += 20.0;
        }
        if let Some(b) = a2.filter(|&b| b != a) {
            let [ab, ba] = v.predictions(a, b);
            text_fit(&trf!("Predicted: attacking {own} / {theirs}; attacked {own2} / {theirs2}", own = ab.own_left, theirs = ab.theirs_left, own2 = ba.theirs_left, theirs2 = ba.own_left), px, y + 14.0, pw, 15.0, INK);
            y += 22.0;
            if small_button(px, y, bw * 1.6, 26.0, tr("Battle 1 against 2"), true) {
                action = ViewerAction::Battle(a, b);
            }
            if small_button(px + bw * 1.6 + 4.0, y, bw * 1.6, 26.0, tr("Battle 2 against 1"), true) {
                action = ViewerAction::Battle(b, a);
            }
            if small_button(px + 2.0 * (bw * 1.6 + 4.0), y, bw * 1.2, 26.0, tr("Test"), true) {
                let _ = v.test_score(a, b);
            }
            y += 32.0;
        }
    }
    // The first subject's overlays.
    if let Some(a) = a1 {
        let ow = (pw - 8.0) / 3.0;
        let picks = [(Overlay::Route, tr("Route")), (Overlay::Density, tr("Density")), (Overlay::Flood, tr("Flood"))];
        for (n, (o, label)) in picks.iter().enumerate() {
            if toggle_button(px + n as f32 * (ow + 4.0), y, ow, 24.0, label, st.overlay == *o) {
                if st.overlay == *o {
                    st.clear_overlay();
                } else {
                    st.overlay = *o;
                    st.grid = None;
                    st.overlay_tex = None;
                    if *o != Overlay::Route {
                        st.maps = st.viewer.plan(a);
                        if let Some(m) = &st.maps {
                            let vals = if *o == Overlay::Density { &m.0 } else { &m.1 };
                            st.overlay_tex = Some(overlay_texture(vals, w, h, *o == Overlay::Density));
                        }
                    }
                }
            }
        }
        y += 30.0;
    }
    if let Some(g) = &st.grid {
        for (rr, row) in g.iter().enumerate() {
            let l: Vec<String> = row.iter().map(|v| v.map_or_else(|| "·".to_string(), |v| v.to_string())).collect();
            text_fit(&l.join(" "), px, y + 12.0 + rr as f32 * 14.0, pw, 12.0, DIM);
        }
    }
    // Messages and notes along the bottom.
    let mut lines: Vec<String> = st.viewer.messages.iter().rev().take(3).cloned().collect();
    if let Some(s) = st.viewer.stop {
        lines.insert(0, stop_text(s));
    }
    if let Some(n) = &st.note {
        lines.push(n.clone());
    }
    for (n, l) in lines.iter().take(4).enumerate() {
        text_fit(l, x0, r.bottom() - 64.0 + n as f32 * 15.0, r.w - 180.0, 13.0, DIM);
    }
    if button(r.right() - 140.0, r.bottom() - 54.0, 120.0, 40.0, tr("Close"), true) || (!typing() && is_key_pressed(KeyCode::Escape)) {
        action = ViewerAction::Close;
    }
    // While running: a few steps a frame, until stopped.
    if st.running {
        let v = &mut st.viewer;
        for _ in 0..10 {
            if v.stop.is_some() {
                st.running = false;
                break;
            }
            v.step();
            if v.elapsed().is_multiple_of(viewer::RUN_REFRESH) {
                break;
            }
        }
    }
    action
}
