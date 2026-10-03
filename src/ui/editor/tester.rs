//! The battle tester's window (the original's TTestBattle, docs/reference/editor/testers.md
//! §2): the AI's army on top, the player's below, each on its 3 × 6 grid with its figures;
//! the unit types with a stat card, the buttons, the three check boxes and the rule switches
//! on the right. The model is `razdor::editor::tester`; the window is made once per session.
//!
//! The original moves the system pointer onto each actor and the AI's target; Razdor leaves
//! the pointer alone and marks those cells with an arrow instead.

use macroquad::prelude::*;

use razdor::editor::tester::{self, Closing, Debug, Run, Tester};
use razdor::i18n::tr;
use razdor::rules::battle::{Switches, Team};
use razdor::rules::content::Stat;
use razdor::rules::formation::Slot;
use razdor::rules::rng::Rng;
use razdor::trf;

use crate::ui::widgets::*;

pub enum TesterAction {
    None,
    Close,
}

/// What a finished battle left to show.
struct Results {
    remaining: [u16; 2],
    debug: [Debug; 2],
}

pub struct TesterState {
    pub tester: Tester,
    /// The catalogue entry picked in the list.
    selected: Option<usize>,
    scroll: usize,
    run: Option<Run>,
    /// When the AI's pending step runs (the 500 ms delay), with its target cell.
    pending: Option<(f64, Option<(Team, Slot)>)>,
    results: Option<Results>,
    /// The option panel's copy of the switches while it is open.
    options: Option<Switches>,
    /// The closing messages of the last battle, shown until clicked away.
    messages: Vec<String>,
    note: Option<String>,
}

impl TesterState {
    pub fn new(tester: Tester) -> TesterState {
        TesterState { tester, selected: None, scroll: 0, run: None, pending: None, results: None, options: None, messages: Vec::new(), note: None }
    }

    /// Armies from the AI viewer (0x5770f8): the start button enabled, ready to fight.
    pub fn set_viewer_armies(&mut self, side1: tester::Army, side2: tester::Army) {
        self.stop();
        self.tester.set_viewer_armies(side1, side2);
        self.results = None;
        self.messages.clear();
    }

    fn stop(&mut self) {
        self.run = None;
        self.pending = None;
    }

    fn running(&self) -> bool {
        self.run.as_ref().is_some_and(|r| !r.over())
    }
}

const CW: f32 = 96.0;
const CH: f32 = 50.0;
const GAP: f32 = 4.0;

/// The text of a closing message.
fn closing_text(c: Closing) -> String {
    match c {
        Closing::Boast { band, turns } => match band {
            0 => tr("As I expected: your army was no match for mine.").to_string(),
            1 => tr("Not bad, but my army was the stronger one.").to_string(),
            2 => trf!("Armies alike, and I still beat yours in {turns} turns.", turns),
            3 => trf!("Your army was stronger than mine, and it fell in {turns} turns.", turns),
            _ => trf!("Your army was far stronger than mine, and I crushed it in {turns} turns. Shame!", turns),
        },
        Closing::Praise { band, turns } => match band {
            0 => tr("You won, but my army was far weaker than yours.").to_string(),
            1 => tr("You won, but my army was weaker than yours.").to_string(),
            2 => trf!("Armies alike, and you won in {turns} turns. Not bad.", turns),
            3 => trf!("My army was stronger, and you still won in {turns} turns. Well fought.", turns),
            _ => trf!("My army was far stronger, and you won in {turns} turns. I bow to you.", turns),
        },
        Closing::Refusal => tr("I refuse to fight with such an army.").to_string(),
    }
}

/// Where cell `s` of army `side` (0 bottom, 1 top) is drawn: the top army's rows go upward
/// from the middle, the bottom army's downward.
fn cell_rect(origin: (f32, f32), side: usize, s: Slot) -> Rect {
    let r = (s.row.number() - 1) as f32;
    let line = if side == 1 { 2.0 - r } else { r };
    Rect::new(origin.0 + s.col as f32 * (CW + GAP), origin.1 + line * (CH + GAP), CW, CH)
}

/// The window in rectangle `r`.
pub fn window(st: &mut TesterState, r: Rect) -> TesterAction {
    let mut action = TesterAction::None;
    let (x0, y0) = (r.x + 16.0, r.y + 30.0);
    text(tr("Battle tester"), x0, y0, 22.0, ACCENT);
    let content = st.tester.content().clone();
    let grid_w = 6.0 * (CW + GAP);
    let bar_h = 62.0;
    // The top army (side 2, the AI's) and its bar, then the bottom army (side 1).
    let top = (x0, y0 + 14.0);
    let top_bar = top.1 + 3.0 * (CH + GAP);
    let bottom = (x0, top_bar + bar_h + 10.0);
    let bottom_bar = bottom.1 + 3.0 * (CH + GAP);
    let running = st.running();
    let actor = st.run.as_ref().filter(|_| running).and_then(|run| run.battle.active());
    let marker_cells: Vec<(Team, Slot)> = {
        let mut v = Vec::new();
        if st.tester.delay {
            if let (Some(run), Some(a)) = (&st.run, actor) {
                let f = &run.battle.fighters[a];
                v.push((f.team, f.slot));
            }
            if let Some((_, Some(t))) = st.pending {
                v.push(t);
            }
        }
        v
    };
    let mut grid_click: Option<(usize, Slot)> = None;
    for side in [1usize, 0] {
        let origin = if side == 1 { top } else { bottom };
        let team = if side == 0 { Team::Player } else { Team::Enemy };
        for s in tester::cells() {
            let cr = cell_rect(origin, side, s);
            let hover = mouse_in(cr.x, cr.y, cr.w, cr.h);
            // In a battle the fighters show; outside it, the army.
            let shown: Option<(String, i32, i32, bool)> = match &st.run {
                Some(run) => run.battle.fighters.iter().enumerate().find(|(_, f)| f.team == team && f.slot == s && f.listed()).map(|(i, f)| (f.name.clone(), f.hp, f.max_hp(), Some(i) == actor)),
                None => st.tester.armies[side].at(s).map(|k| {
                    let p = &st.tester.armies[side].units[k];
                    (p.unit.name(&content).to_string(), p.unit.hp, p.unit.stats(&content).max_hp(), false)
                }),
            };
            let bg = match &shown {
                Some((_, _, _, true)) => Color::new(0.5, 0.5, 0.5, 1.0),
                Some(_) => Color::new(0.24, 0.2, 0.15, 1.0),
                None => FIELD_BG,
            };
            draw_rectangle(cr.x, cr.y, cr.w, cr.h, bg);
            let legal = running && st.run.as_ref().is_some_and(|run| run.waits_for_click()) && hover;
            draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, if hover { 2.0 } else { 1.0 }, if legal { ACCENT } else if hover { INK } else { DIM });
            if let Some((name, hp, max, _)) = &shown {
                text_fit(name, cr.x + 4.0, cr.y + 18.0, cr.w - 8.0, 15.0, INK);
                text_fit(&format!("{hp}/{max}"), cr.x + 4.0, cr.y + 40.0, cr.w - 8.0, 14.0, DIM);
            }
            if marker_cells.contains(&(team, s)) {
                // The pointer's jump, as an arrow on the cell.
                let (ax, ay) = (cr.right() - 18.0, cr.y + 6.0);
                draw_triangle(vec2(ax, ay), vec2(ax + 12.0, ay + 14.0), vec2(ax + 3.0, ay + 16.0), ACCENT);
            }
            if hover && clicked() {
                grid_click = Some((side, s));
            }
        }
        // The army's bar: cost, side strength, remaining value, diagnostic line.
        let by = if side == 1 { top_bar } else { bottom_bar };
        draw_rectangle(x0, by, grid_w - GAP, bar_h - 6.0, Color::new(0.08, 0.075, 0.07, 1.0));
        let army = &st.tester.armies[side];
        let who = if side == 1 { tr("AI army") } else { tr("Your army") };
        let strength = st.run.as_ref().map_or_else(|| army.strength(&content), |run| run.setup[side]);
        let mut line = trf!("{who}: cost {cost}, strength {strength}", who, cost = army.cost, strength);
        if let Some(res) = &st.results {
            line.push_str(&trf!(", left {left}", left = res.remaining[side]));
        }
        text_fit(&line, x0 + 6.0, by + 20.0, grid_w - 16.0, 16.0, INK);
        if let Some(res) = &st.results {
            let d = res.debug[side];
            text_fit(&trf!("Turn {turn}; XP pool {pool}; HP lost {lost}, predicted {predicted}", turn = d.turn, pool = d.pool, lost = d.lost, predicted = d.predicted), x0 + 6.0, by + 42.0, grid_w - 16.0, 14.0, DIM);
        }
    }
    // The right panel.
    let px = x0 + grid_w + 14.0;
    let pw = r.right() - px - 16.0;
    let mut y = y0 + 14.0;
    let bw = (pw - 8.0) / 3.0;
    let idle = !running;
    let one = small_button(px, y, bw, 28.0, tr("Random 1100"), idle);
    let two = small_button(px + bw + 4.0, y, bw, 28.0, tr("Random 2100"), idle);
    if one || two {
        let tag = if one { 1 } else { 2 };
        // The original seeds from the CPU clock: the armies cannot be repeated.
        let seed = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.subsec_nanos() ^ d.as_secs() as u32);
        let stop = st.tester.random(tag, &mut Rng::new(seed));
        st.run = None;
        st.results = None;
        st.note = stop.map(|s| trf!("Army {side}: no unit type of its natures costs 10 to {left}; the original would never finish, Razdor stopped that army.", side = s.side, left = s.left - 1));
    }
    if small_button(px + 2.0 * (bw + 4.0), y, bw, 28.0, tr("Clear"), idle) {
        st.tester.clear();
        st.run = None;
        st.results = None;
    }
    y += 32.0;
    let dir = razdor::editor::options::editor_dir();
    if small_button(px, y, bw, 28.0, tr("Load"), idle) {
        match &dir {
            Some(d) if st.tester.load(d) => {
                st.run = None;
                st.results = None;
                st.note = None;
            }
            _ => st.note = Some(trf!("There is no {file} to load.", file = tester::FILE)),
        }
    }
    if small_button(px + bw + 4.0, y, bw, 28.0, tr("Save"), idle) {
        st.note = Some(match dir.as_deref().map(|d| st.tester.save(d)) {
            Some(Ok(p)) => trf!("Saved {path}.", path = p.display()),
            Some(Err(e)) => trf!("Not saved: {e}", e),
            None => tr("Not saved: no data folder.").to_string(),
        });
    }
    if small_button(px + 2.0 * (bw + 4.0), y, bw, 28.0, tr("Swap"), idle) {
        st.tester.swap();
        st.run = None;
        st.results = None;
    }
    y += 32.0;
    let start = small_button(px, y, bw, 28.0, tr("Start"), st.tester.armed && idle);
    let stop = small_button(px + bw + 4.0, y, bw, 28.0, if running { tr("Stop") } else { tr("Exit") }, true);
    if small_button(px + 2.0 * (bw + 4.0), y, bw, 28.0, tr("Rules"), idle && st.options.is_none()) {
        st.options = Some(st.tester.switches);
    }
    y += 36.0;
    let t = &mut st.tester;
    if let Some(v) = checkbox(px, y, pw, tr("Super AI"), t.super_ai) {
        t.set_super_ai(v);
    }
    if let Some(v) = checkbox(px, y + 24.0, pw, tr("Both sides played by the AI"), t.all_ai) {
        t.all_ai = v;
    }
    if let Some(v) = checkbox(px, y + 48.0, pw, tr("Delay"), t.delay) {
        t.delay = v;
    }
    y += 80.0;
    if let Some(sw) = &mut st.options {
        // The option panel: the five switches, OK keeps them, Cancel the old ones.
        let rows: [(&str, &mut bool); 5] = [
            (tr("Counterblow"), &mut sw.counterblow),
            (tr("Short-range shots and spells"), &mut sw.short_range),
            (tr("Rows step forward"), &mut sw.collapse),
            (tr("Long strike"), &mut sw.long_strike),
            (tr("Actions cost initiative"), &mut sw.initiative_cost),
        ];
        for (k, (label, v)) in rows.into_iter().enumerate() {
            if let Some(n) = checkbox(px, y + k as f32 * 24.0, pw, label, *v) {
                *v = n;
            }
        }
        y += 124.0;
        if small_button(px, y, bw, 26.0, tr("OK"), true) {
            st.tester.switches = *sw;
            st.options = None;
        } else if small_button(px + bw + 4.0, y, bw, 26.0, tr("Cancel"), true) {
            st.options = None;
        }
        y += 32.0;
    }
    // The unit types, then the picked one's card.
    let list_h = (r.bottom() - 150.0 - y).max(60.0);
    let rows = (list_h / 20.0).floor() as usize;
    let n = st.tester.catalogue.len();
    if mouse_in(px, y, pw, list_h) {
        let wh = wheel();
        if wh < 0.0 {
            st.scroll = (st.scroll + 3).min(n.saturating_sub(rows));
        } else if wh > 0.0 {
            st.scroll = st.scroll.saturating_sub(3);
        }
    }
    draw_rectangle(px, y, pw, list_h, FIELD_BG);
    for (row, k) in (st.scroll..n).take(rows).enumerate() {
        let ry = y + row as f32 * 20.0;
        let id = st.tester.catalogue[k];
        if st.selected == Some(k) {
            draw_rectangle(px, ry, pw, 20.0, Color::new(0.4, 0.3, 0.15, 1.0));
        }
        text_fit(&format!("{} ({})", content.unit(id).name, content.unit(id).cost), px + 4.0, ry + 15.0, pw - 8.0, 15.0, INK);
        if mouse_in(px, ry, pw, 20.0) && clicked() {
            st.selected = Some(k);
        }
    }
    y += list_h + 8.0;
    if let Some(&id) = st.selected.and_then(|k| st.tester.catalogue.get(k)) {
        let d = content.unit(id);
        let lines = [
            trf!("Attack {blow} / {shot}, defence {dblow} / {dshot}", blow = d.stat(Stat::AttackBlow), shot = d.stat(Stat::AttackShot), dblow = d.stat(Stat::DefenceBlow), dshot = d.stat(Stat::DefenceShot)),
            trf!("Hits {hits}, actions {actions}, magic {magic}, initiative {init}", hits = d.hits, actions = d.manevres, magic = d.magic_power, init = d.initiative),
            trf!("Cost {cost}", cost = d.cost),
        ];
        for (k, l) in lines.iter().enumerate() {
            text_fit(l, px, y + 14.0 + k as f32 * 18.0, pw, 14.0, DIM);
        }
    }
    if let Some(note) = &st.note {
        for (k, l) in wrap(note, r.w - 32.0, 14.0).iter().take(2).enumerate() {
            text(l, x0, r.bottom() - 30.0 + k as f32 * 16.0, 14.0, DIM);
        }
    }
    // The closing messages, over the grids until clicked.
    if !st.messages.is_empty() {
        let mr = Rect::new(x0 + 40.0, top_bar + 4.0, grid_w - 80.0, 30.0 + 22.0 * st.messages.len() as f32);
        draw_rectangle(mr.x, mr.y, mr.w, mr.h, Color::new(0.1, 0.08, 0.06, 0.97));
        draw_rectangle_lines(mr.x, mr.y, mr.w, mr.h, 2.0, ACCENT);
        for (k, m) in st.messages.iter().enumerate() {
            text_fit(m, mr.x + 12.0, mr.y + 24.0 + 22.0 * k as f32, mr.w - 24.0, 16.0, INK);
        }
        if clicked() || key(KeyCode::Enter) {
            st.messages.clear();
        }
        return action;
    }
    // The buttons' effects and the battle's steps.
    let esc = !typing() && is_key_pressed(KeyCode::Escape);
    if start {
        st.results = None;
        st.messages.clear();
        st.run = Some(st.tester.start());
        st.pending = None;
    } else if stop || esc {
        // Stop (button, Esc or closing): the battle ends at once with no result.
        if running {
            st.stop();
        } else {
            st.stop();
            action = TesterAction::Close;
        }
    } else if let Some((side, s)) = grid_click {
        if running {
            if let Some(run) = &mut st.run {
                if run.waits_for_click() {
                    run.click(if side == 0 { Team::Player } else { Team::Enemy }, s);
                }
            }
        } else if st.run.is_none() || st.results.is_some() {
            st.run = None;
            st.results = None;
            let sel = st.selected.and_then(|k| st.tester.catalogue.get(k).copied());
            st.tester.click(side, s, sel);
        }
    }
    step(st);
    action
}

/// The battle's loop for this frame: an AI actor acts (after the 500 ms with the delay box
/// on, the pointer marked on it and its target), a player's actor waits for a click; at the
/// end, the figures and, with the delay box on, the closing messages.
fn step(st: &mut TesterState) {
    let delay = st.tester.delay;
    let Some(run) = &mut st.run else { return };
    if st.results.is_some() {
        return;
    }
    let now = get_time();
    // Without the delay the AI plays on until the player's turn or the end (a frame's worth).
    for _ in 0..if delay { 1 } else { 200 } {
        if run.over() || run.waits_for_click() {
            break;
        }
        if delay {
            match st.pending {
                None => st.pending = Some((now + tester::STEP_DELAY_MS as f64 / 1000.0, run.battle.ai_target_cell())),
                Some((at, _)) if now >= at => {
                    run.ai_step();
                    st.pending = None;
                }
                Some(_) => {}
            }
        } else {
            run.ai_step();
        }
    }
    if run.over() {
        st.results = Some(Results { remaining: [run.remaining(0), run.remaining(1)], debug: [run.debug(0), run.debug(1)] });
        st.messages = run.closing().into_iter().map(closing_text).collect();
        st.pending = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::rules::formation::Row;

    #[test]
    fn the_top_army_grows_upward_from_the_middle() {
        let front = cell_rect((0.0, 0.0), 1, Slot::new(Row::Front, 0));
        let reserve = cell_rect((0.0, 0.0), 1, Slot::new(Row::Reserve, 2));
        assert!(reserve.y < front.y);
        let front = cell_rect((0.0, 0.0), 0, Slot::new(Row::Front, 0));
        let reserve = cell_rect((0.0, 0.0), 0, Slot::new(Row::Reserve, 2));
        assert!(reserve.y > front.y);
    }
}
