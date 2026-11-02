//! The field battle screen (Razdor 2.0): a board of up to 12 × 10 tiles with walls, a gate,
//! battlements and rubble, fought by DT units. The rules are [`razdor::rules::field`]; this file
//! only draws them and turns taps into actions.
//!
//! [`FieldSource`] is the seam to the game: the screen reads a [`Snapshot`], asks for the
//! [`Action`]s a unit may take, plays one, and lets the AI play. [`Game`] implements it over its
//! field battle (the package's hooks are the game's); [`DemoField`] implements it over a battle of
//! its own, which the snapshot scene `field:siege` runs ([`siege_screen`]).
//!
//! Taps: a reachable cell highlights blue. A tap there moves the unit; if an enemy can be hit
//! from that cell, the first tap only plans the move (the cell goes gold, its targets red), and a
//! tap on a red target commits move and blow or shot together, a tap on the cell again commits the
//! move alone. A tap on a target in reach from where the unit stands attacks at once. «Ждать»
//! (wait) ends the unit's turn; «Авто» plays the whole side on the rules' AI.
//!
//! Units are drawn in the interface's units (1 unit = 1 dp on a phone, `chrome::k`), the board
//! fits the space left of the panel (landscape or desktop) or sits above it (a phone held
//! upright).

use std::collections::VecDeque;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::pack::aliases;
use razdor::pack::refs::DtKind;
use razdor::rules::battle::Team;
use razdor::rules::content::{Content, UnitId};
use razdor::rules::game::{BattleResult, Game};
use razdor::rules::field::board::{Board, Tile};
use razdor::rules::field::{shadow, Action, FieldBattle, FieldScript, Side, Snapshot, Target, UnitView};
use razdor::rules::formation::{Row, Slot};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::dialog::Dialog;
use super::Screen;
use super::chrome::{self, CREAM, GOLD, RED_TEXT};
use super::display;
use super::widgets::{self, DIM, INK};

/// What a field battle needs from the game: the screen reads a [`Snapshot`], asks for the
/// [`Action`]s a unit may take, plays one, and lets the AI play. Each call is the rules' own, one
/// for one; the screen never computes a rule.
pub trait FieldSource {
    /// The board, the units and the turn; `None` when no battle is under way.
    fn snapshot(&mut self) -> Option<Snapshot>;
    /// The DT unit a battle unit is drawn as.
    fn kind(&mut self, unit: usize) -> Option<UnitId>;
    /// The unit due to act, starting or ending rounds as needed; `None` once the battle is over.
    fn acting(&mut self) -> Option<usize>;
    /// Every action unit `unit` may take now (empty unless it is the one due).
    fn legal(&mut self, unit: usize) -> Vec<Action>;
    /// Plays `action` for the unit due now.
    fn act(&mut self, action: Action) -> Result<(), String>;
    /// Plays the unit due now by the AI. `false` once the battle is over.
    fn auto_step(&mut self) -> bool;
    /// The coming units: the rest of this round, then the next round's order.
    fn queue(&mut self) -> Vec<usize>;
    /// The last `lines` lines of the battle's log, in `lang`, oldest first.
    fn log(&mut self, lines: usize, lang: &str) -> Vec<String>;
    /// The winner, once the battle is over.
    fn outcome(&mut self) -> Option<Side>;
    /// The round of the battle (0 when none).
    fn round(&mut self) -> u32;
    /// The terrain surface code of the ground the battle is on (the DT battle's background is the
    /// map's terrain there); `None` for a battle with no ground of its own (the screen's green).
    fn ground(&self) -> Option<u8> {
        None
    }
}

impl FieldSource for Game {
    fn ground(&self) -> Option<u8> {
        self.field_ground()
    }

    fn snapshot(&mut self) -> Option<Snapshot> {
        self.field_snapshot()
    }

    fn kind(&mut self, unit: usize) -> Option<UnitId> {
        self.field_kind(unit)
    }

    fn acting(&mut self) -> Option<usize> {
        self.field_acting()
    }

    fn legal(&mut self, unit: usize) -> Vec<Action> {
        self.field_legal(unit)
    }

    fn act(&mut self, action: Action) -> Result<(), String> {
        self.field_act(action)
    }

    fn auto_step(&mut self) -> bool {
        self.field_auto_step().is_ok()
    }

    fn queue(&mut self) -> Vec<usize> {
        self.field_queue()
    }

    fn log(&mut self, lines: usize, lang: &str) -> Vec<String> {
        self.field_log(lines, lang)
    }

    fn outcome(&mut self) -> Option<Side> {
        self.field_outcome()
    }

    fn round(&mut self) -> u32 {
        self.field_round().unwrap_or(0)
    }
}

/// A battle the screen holds itself, with its hooks: the snapshot scene's demo siege.
pub struct DemoField {
    pub fb: FieldBattle,
    pub hooks: Box<dyn FieldScript>,
}

impl FieldSource for DemoField {
    fn snapshot(&mut self) -> Option<Snapshot> {
        Some(self.fb.snapshot(self.fb.acting()))
    }

    fn kind(&mut self, unit: usize) -> Option<UnitId> {
        self.fb.shadow.fighters.get(unit).map(|f| f.unit)
    }

    fn acting(&mut self) -> Option<usize> {
        self.fb.next_actor(self.hooks.as_mut())
    }

    fn legal(&mut self, unit: usize) -> Vec<Action> {
        self.fb.legal(unit)
    }

    fn act(&mut self, action: Action) -> Result<(), String> {
        let f = self.acting().ok_or_else(|| "no unit is due to act".to_string())?;
        self.fb.act(f, action, self.hooks.as_mut())?;
        self.fb.next_actor(self.hooks.as_mut());
        Ok(())
    }

    fn auto_step(&mut self) -> bool {
        let played = self.fb.auto_step(self.hooks.as_mut());
        self.fb.next_actor(self.hooks.as_mut());
        played
    }

    fn queue(&mut self) -> Vec<usize> {
        self.fb.queue()
    }

    fn log(&mut self, lines: usize, lang: &str) -> Vec<String> {
        let none = std::collections::BTreeMap::new();
        let from = self.fb.state.log.len().saturating_sub(lines);
        self.fb.state.log[from..].iter().map(|l| razdor::rules::field::text::render(l, lang, &none)).collect()
    }

    fn outcome(&mut self) -> Option<Side> {
        self.fb.outcome()
    }

    fn round(&mut self) -> u32 {
        self.fb.round()
    }
}

/// The end of a field battle, as a DT battle's close settles it (`Game::field_finish`): a defeat's
/// report, a victory's report on the map (or the victory screen when the game is won), the
/// withdrawal's notice. The screen the app goes to.
pub fn finish(game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Screen {
    let Some(result) = game.field_finish() else { return Screen::WorldMap };
    let due = crate::ui::clock::now() + razdor::av::BATTLE_REPORT_GAP_MS as f64 / 1000.0;
    match result {
        BattleResult::Defeat => {
            dialogs.push_back(Dialog { not_before: Some(due), ..Dialog::defeat(super::last_save_path().is_some()) });
            *message = None;
            Screen::WorldMap
        }
        BattleResult::Victory { .. } if game.won() => Screen::Victory,
        victory @ BattleResult::Victory { .. } => {
            dialogs.extend(Dialog::victory(&victory).map(|d| Dialog { not_before: Some(due), ..d }));
            *message = None;
            Screen::WorldMap
        }
        BattleResult::Withdrew { lost } => {
            *message = Some(if lost > 0 {
                razdor::trf!("Nobody breaks. You withdraw, {lost} fell; no experience without a victory.", lost)
            } else {
                tr("Nobody breaks. You withdraw; no experience without a victory.").to_string()
            });
            Screen::WorldMap
        }
    }
}

/// The board on the screen: a grid of square cells, `cell` pixels each, from `(x0, y0)`. A
/// board turned a quarter (`turn`, a phone held upright: its long side runs down the screen)
/// has its columns and rows swapped: board `(x, y)` is shown at `(bh - 1 - y, x)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Grid {
    pub x0: f32,
    pub y0: f32,
    pub cell: f32,
    /// The cells across and down the screen.
    pub w: i32,
    pub h: i32,
    pub turn: bool,
    /// The board's own size.
    pub bw: i32,
    pub bh: i32,
}

impl Grid {
    /// The largest square cells that fit `area` for a `bw × bh` board (turned or not), centred
    /// in it (turned: at the top).
    pub fn fit(area: Rect, bw: i32, bh: i32, turn: bool) -> Grid {
        let (w, h) = if turn { (bh, bw) } else { (bw, bh) };
        let cell = (area.w / w as f32).min(area.h / h as f32).max(1.0);
        let x0 = area.x + (area.w - cell * w as f32) / 2.0;
        // Centred across and down, unless turned (a phone upright: the board sits at the top).
        let y0 = if turn { area.y } else { area.y + (area.h - cell * h as f32) / 2.0 };
        Grid { x0, y0, cell, w, h, turn, bw, bh }
    }

    pub fn rect(&self) -> Rect {
        Rect::new(self.x0, self.y0, self.cell * self.w as f32, self.cell * self.h as f32)
    }

    /// The screen cell of board tile `(x, y)`.
    fn screen_of(&self, x: i32, y: i32) -> (i32, i32) {
        if self.turn {
            (self.bh - 1 - y, x)
        } else {
            (x, y)
        }
    }

    /// The board tile on screen cell `(sx, sy)`.
    fn board_of(&self, sx: i32, sy: i32) -> (i32, i32) {
        if self.turn {
            (sy, self.bh - 1 - sx)
        } else {
            (sx, sy)
        }
    }

    /// The screen rectangle of board tile `(x, y)`.
    pub fn tile_rect(&self, x: i32, y: i32) -> Rect {
        let (sx, sy) = self.screen_of(x, y);
        Rect::new(self.x0 + sx as f32 * self.cell, self.y0 + sy as f32 * self.cell, self.cell, self.cell)
    }

    /// The centre of board tile `(x, y)` on the screen.
    #[cfg(test)]
    pub fn centre(&self, x: i32, y: i32) -> (f32, f32) {
        let r = self.tile_rect(x, y);
        (r.x + r.w / 2.0, r.y + r.h / 2.0)
    }

    /// The board tile under the screen point `p`; `None` outside the board.
    pub fn tile_at(&self, p: (f32, f32)) -> Option<(i32, i32)> {
        let fx = (p.0 - self.x0) / self.cell;
        let fy = (p.1 - self.y0) / self.cell;
        if fx < 0.0 || fy < 0.0 {
            return None;
        }
        let (sx, sy) = (fx.floor() as i32, fy.floor() as i32);
        (sx < self.w && sy < self.h).then(|| self.board_of(sx, sy))
    }
}

/// What a tap on the board hits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pick {
    /// A tile with no living unit on it.
    Tile(i32, i32),
    /// A living unit, by its id.
    Unit(usize),
}

/// What a tap at `p` on `grid` hits: a living unit on its tile first, else the tile. `None`
/// outside the board.
pub fn pick(grid: &Grid, units: &[UnitView], p: (f32, f32)) -> Option<Pick> {
    let (x, y) = grid.tile_at(p)?;
    let unit = units.iter().find(|u| u.alive && u.at == Some((x, y)));
    Some(match unit {
        Some(u) => Pick::Unit(u.id),
        None => Pick::Tile(x, y),
    })
}

/// Pixels per unit: on a phone one unit is one dp (the touch targets and the text sizes of the
/// phone rules), else the interface scale.
fn unit_px() -> f32 {
    display::dp_px().unwrap_or_else(chrome::k)
}

/// A text's size in pixels for a base size in units (one unit is one dp on a phone), so a 14-unit
/// text is 14 dp there.
fn text_px(base: f32) -> f32 {
    base * unit_px()
}

/// The screen's parts, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    pub grid: Grid,
    /// The panel with the queue, the log and the buttons.
    pub panel: Rect,
    /// A phone held upright: the panel sits below the board.
    pub stacked: bool,
}

/// The panel's width in units on the desktop and in landscape.
const PANEL_W: f32 = 270.0;
/// The margin round the screen, in units.
const MARGIN: f32 = 10.0;
/// A button's height in units: 40 dp or more on a phone.
const BUTTON_H: f32 = 42.0;
/// How long the AI's pause is between two actions, in seconds.
const STEP_SECS: f64 = 0.6;
/// The log lines the panel tries to show (newest last).
const LOG_LINES: usize = 40;

/// The layout of a `sw × sh` screen in pixels, with `k` pixels per unit. `phone_upright`: a phone
/// held upright, the board turned on top (its long side down the screen) and the panel below.
pub fn layout(sw: f32, sh: f32, k: f32, board: (i32, i32), phone_upright: bool) -> Layout {
    let m = MARGIN * k;
    if phone_upright {
        // The board takes the widest cells that fit 60% of the height, at the top margin.
        let area = Rect::new(m, m, sw - 2.0 * m, sh * 0.6);
        let grid = Grid::fit(area, board.0, board.1, true);
        let top = grid.y0 + grid.rect().h + m;
        let panel = Rect::new(m, top, sw - 2.0 * m, sh - top - m);
        return Layout { grid, panel, stacked: true };
    }
    let panel_w = (PANEL_W * k).min(sw * 0.36);
    let area = Rect::new(m, m, sw - panel_w - 3.0 * m, sh - 2.0 * m);
    let grid = Grid::fit(area, board.0, board.1, false);
    let panel = Rect::new(sw - panel_w - m, m, panel_w, sh - 2.0 * m);
    Layout { grid, panel, stacked: false }
}

/// Which cells a unit may stand on, and what it can hit from each: read off the legal actions.
#[derive(Default, Debug, PartialEq)]
pub struct Options {
    /// The cells the unit may move to (not its own cell).
    pub cells: Vec<(i32, i32)>,
    /// The targets reachable from the planned cell (or from where it stands), as actions.
    pub targets: Vec<Action>,
}

impl Options {
    /// The options of `legal` for the unit's plan: `planned` is the cell it plans to stand on
    /// (`None`: where it stands).
    pub fn of(legal: &[Action], planned: Option<(i32, i32)>) -> Options {
        let mut cells: Vec<(i32, i32)> = legal.iter().filter(|a| a.attack.is_none()).filter_map(|a| a.move_to).collect();
        cells.sort_unstable();
        cells.dedup();
        let targets = legal.iter().filter(|a| a.move_to == planned && a.attack.is_some()).copied().collect();
        Options { cells, targets }
    }
}

/// The action a tap on `hit` plays, from the legal ones: an attack on that target, from the planned
/// cell when it has one, else from where the unit stands. `None` when nothing in reach.
pub fn attack_action(legal: &[Action], planned: Option<(i32, i32)>, target: Target) -> Option<Action> {
    let mut options = legal.iter().filter(|a| a.attack == Some(target));
    options.clone().find(|a| a.move_to == planned).or_else(|| options.find(|a| a.move_to.is_none())).copied()
}

/// The screen's state between frames: the plan of the unit due, the AI's pace, the end.
#[derive(Debug, Default)]
pub struct FieldView {
    /// The unit the plan is for; a new unit due starts a new plan.
    plan_for: Option<usize>,
    /// The cell the unit is planning to stand on, while a tap there waits for its target.
    planned: Option<(i32, i32)>,
    /// «Авто»: the computer plays the player's side too.
    pub auto: bool,
    /// When the AI last acted (`clock::now`, seconds).
    last_step: f64,
}

/// The field screen as the app holds it: its view, and for the snapshot scene its own demo
/// battle. A game's field battle is the game's (the app passes the game to [`FieldView::frame`]).
#[derive(Default)]
pub struct FieldScreen {
    pub view: FieldView,
    pub demo: Option<DemoField>,
}

/// What the board highlights this frame.
#[derive(Default)]
struct Marks {
    /// Cells the unit due may move to.
    cells: Vec<(i32, i32)>,
    /// The planned cell, gold.
    planned: Option<(i32, i32)>,
    /// Targets in reach from the planned cell (or where the unit stands), red.
    targets: Vec<Target>,
}

impl FieldView {
    /// One frame: the AI's pace, the player's taps and wait, then the drawing. The whole frame
    /// runs at the unit scale (`display::with_window_scale`), so the text sizes follow it. Returns
    /// `true` on the frame the end's «Дальше» is pressed: the battle is over and the app settles it
    /// (`Game::field_finish`).
    pub fn frame(&mut self, src: &mut dyn FieldSource, assets: &Assets) -> bool {
        let k = unit_px();
        let ground = src.ground();
        display::with_window_scale(Some(k), || self.frame_at_scale(src, assets, ground))
    }

    fn frame_at_scale(&mut self, src: &mut dyn FieldSource, assets: &Assets, ground: Option<u8>) -> bool {
        let k = unit_px();
        let (sw, sh) = (screen_width(), screen_height());
        let phone_upright = display::dp_px().is_some() && sh > sw;
        let Some(mut snap) = src.snapshot() else { return false };
        let outcome = src.outcome();
        let actor = if outcome.is_none() { src.acting() } else { None };
        let kinds: Vec<Option<UnitId>> = (0..snap.units.len()).map(|i| src.kind(i)).collect();
        let lay = layout(sw, sh, k, (snap.board.w, snap.board.h), phone_upright);
        let now = crate::ui::clock::now();

        if self.plan_for != actor {
            self.plan_for = actor;
            self.planned = None;
        }
        let player_due = actor.filter(|&a| snap.units[a].side == Side::Defenders && !self.auto && outcome.is_none());
        if actor.is_some() && player_due.is_none() && outcome.is_none() && now - self.last_step >= STEP_SECS {
            src.auto_step();
            self.last_step = now;
        }
        // The snapshot after the AI's step, so the board shows the state the player acts on.
        if let Some(after) = src.snapshot() {
            snap = after;
        }
        let actor = snap.active;
        let player_due = actor.filter(|&a| snap.units[a].side == Side::Defenders && !self.auto && outcome.is_none());

        let legal = player_due.map(|a| src.legal(a)).unwrap_or_default();
        let opts = Options::of(&legal, self.planned);
        let marks = Marks { cells: if player_due.is_some() { opts.cells.clone() } else { Vec::new() }, planned: self.planned, targets: opts.targets.iter().filter_map(|a| a.attack).collect() };

        draw_background(sw, sh, assets, ground);
        draw_board(&snap, &lay, assets, &kinds, actor, &marks, ground);

        if player_due.is_some() && widgets::tapped_in(lay.grid.rect()) {
            let p = widgets::pointer();
            self.tap(src, &snap, &lay.grid, &legal, p);
        }
        let wait = draw_panel(src, &snap, &lay, assets, &kinds, actor, player_due.is_some(), outcome, &mut self.auto);
        if wait {
            self.planned = None;
            let _ = src.act(Action::default());
        }
        match outcome {
            Some(winner) => draw_end(&lay, winner),
            None => false,
        }
    }

    /// A tap on the board for the unit due (at `legal`): a tap on a target plays its action, a
    /// tap on a reachable cell moves there (or plans the move when a target is in reach from it),
    /// a tap on its own cell cancels a plan or waits.
    fn tap(&mut self, src: &mut dyn FieldSource, snap: &Snapshot, grid: &Grid, legal: &[Action], p: (f32, f32)) {
        let Some(hit) = pick(grid, &snap.units, p) else { return };
        let here = snap.active.and_then(|a| snap.units[a].at);
        let play: Option<Action> = match hit {
            Pick::Unit(t) => attack_action(legal, self.planned, Target::Unit(t)),
            Pick::Tile(x, y) if matches!(snap.board.get(x, y), Some(Tile::Gate { .. })) => attack_action(legal, self.planned, Target::Tile(x, y)),
            Pick::Tile(x, y) if Some((x, y)) == here => {
                if self.planned.is_some() {
                    self.planned = None;
                    None
                } else {
                    Some(Action::default())
                }
            }
            Pick::Tile(x, y) => {
                let reachable = Options::of(legal, None).cells.contains(&(x, y));
                let has_target = legal.iter().any(|a| a.move_to == Some((x, y)) && a.attack.is_some());
                if !reachable {
                    None
                } else if self.planned == Some((x, y)) || !has_target {
                    Some(Action { move_to: Some((x, y)), attack: None })
                } else {
                    self.planned = Some((x, y));
                    None
                }
            }
        };
        if let Some(act) = play {
            self.planned = None;
            let _ = src.act(act);
        }
    }
}

/// The board: ground, tiles, highlights, units.
fn draw_board(snap: &Snapshot, lay: &Layout, assets: &Assets, kinds: &[Option<UnitId>], actor: Option<usize>, marks: &Marks, ground: Option<u8>) {
    let grid = lay.grid;
    let board = &snap.board;
    let rect = grid.rect();
    let k = unit_px();
    draw_rectangle(rect.x - 3.0, rect.y - 3.0, rect.w + 6.0, rect.h + 6.0, Color::new(0.10, 0.08, 0.06, 1.0));
    for y in 0..board.h {
        for x in 0..board.w {
            let r = grid.tile_rect(x, y);
            draw_ground(assets, r, ground);
            draw_tile(assets, board.get(x, y).unwrap_or(Tile::Open), r);
        }
    }
    for &(x, y) in &marks.cells {
        let r = grid.tile_rect(x, y);
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.35, 0.6, 1.0, 0.28));
        draw_rectangle_lines(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 2.0, Color::new(0.45, 0.7, 1.0, 0.9));
    }
    for t in &marks.targets {
        match *t {
            Target::Unit(id) => {
                if let Some((x, y)) = snap.units.get(id).and_then(|u| u.at) {
                    let r = grid.tile_rect(x, y);
                    draw_rectangle_lines(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 3.0, Color::new(1.0, 0.25, 0.2, 0.95));
                }
            }
            Target::Tile(x, y) => {
                let r = grid.tile_rect(x, y);
                draw_rectangle_lines(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 3.0, Color::new(1.0, 0.25, 0.2, 0.95));
            }
        }
    }
    if let Some((x, y)) = marks.planned {
        let r = grid.tile_rect(x, y);
        draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 3.0 * k, GOLD);
    }
    for u in snap.units.iter().filter(|u| u.alive) {
        let Some((x, y)) = u.at else { continue };
        let r = grid.tile_rect(x, y);
        let inset = r.w * 0.06;
        let pr = Rect::new(r.x + inset, r.y + inset, r.w - 2.0 * inset, r.h - 2.0 * inset);
        let team = if u.side == Side::Defenders { Team::Player } else { Team::Enemy };
        match kinds.get(u.id).copied().flatten() {
            Some(kind) => draw_unit(assets, kind, team, pr),
            None => draw_rectangle(pr.x, pr.y, pr.w, pr.h, Color::new(0.3, 0.3, 0.3, 1.0)),
        }
        draw_rectangle_lines(pr.x, pr.y, pr.w, pr.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        draw_hp_bar(Rect::new(pr.x, pr.y + pr.h - 6.0 * k, pr.w, 5.0 * k), u.hp, u.max_hp);
        if Some(u.id) == actor {
            let pulse = (crate::ui::clock::now() * 6.0).sin() as f32 * 0.5 + 0.5;
            draw_rectangle_lines(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0, 3.0, Color::new(1.0, 0.85, 0.2, 0.6 + 0.4 * pulse));
        }
    }
}

/// The right-hand panel (or the one below the board): the round, the queue, the log, the
/// buttons. Returns `true` when «Ждать» was pressed.
#[allow(clippy::too_many_arguments)]
fn draw_panel(src: &mut dyn FieldSource, snap: &Snapshot, lay: &Layout, assets: &Assets, kinds: &[Option<UnitId>], actor: Option<usize>, player_due: bool, outcome: Option<Side>, auto: &mut bool) -> bool {
    let k = unit_px();
    let p = lay.panel;
    draw_rectangle(p.x, p.y, p.w, p.h, Color::new(0.10, 0.08, 0.06, 0.92));
    draw_rectangle_lines(p.x, p.y, p.w, p.h, 1.0, Color::new(0.55, 0.45, 0.30, 0.9));
    let pad = 10.0 * k;
    let x = p.x + pad;
    let w = p.w - 2.0 * pad;
    let mut y = p.y + pad + 16.0 * k;
    widgets::text_fit(tr("Field battle"), x, y, w, text_px(18.0), GOLD);
    y += 22.0 * k;
    let turn = razdor::trf!("Turn {n} of {max}", n = src.round(), max = snap.max_turns);
    widgets::text_fit(&turn, x, y, w, text_px(15.0), INK);
    y += 22.0 * k;
    let who = match (actor, outcome) {
        (_, Some(_)) => tr("The battle is over").to_string(),
        (Some(a), None) if player_due => razdor::trf!("Your move: {name}", name = snap.units[a].name),
        (Some(a), None) if *auto => razdor::trf!("Auto: {name}", name = snap.units[a].name),
        (Some(a), None) => razdor::trf!("Their move: {name}", name = snap.units[a].name),
        (None, None) => tr("Waiting").to_string(),
    };
    let color = if player_due { CREAM } else { RED_TEXT };
    widgets::text_fit(&who, x, y, w, text_px(15.0), color);
    y += 24.0 * k;

    // The queue: the next 8 units as small portraits, a bar in their side's colour.
    widgets::text_fit(tr("Initiative"), x, y, w, text_px(14.0), DIM);
    y += 6.0 * k;
    let queue = src.queue();
    let n = 8usize;
    let gap = 4.0 * k;
    let chip = ((w - gap * (n as f32 - 1.0)) / n as f32).min(44.0 * k).max(8.0);
    for (i, &u) in queue.iter().take(n).enumerate() {
        let r = Rect::new(x + i as f32 * (chip + gap), y, chip, chip);
        draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.05, 0.04, 0.03, 1.0));
        let view = snap.units.get(u);
        if let (Some(view), Some(kind)) = (view, kinds.get(u).copied().flatten()) {
            let team = if view.side == Side::Defenders { Team::Player } else { Team::Enemy };
            draw_unit(assets, kind, team, Rect::new(r.x + 1.0, r.y + 1.0, r.w - 2.0, r.h - 2.0));
        }
        let bar = if view.is_some_and(|v| v.side == Side::Defenders) { Color::new(0.3, 0.55, 1.0, 1.0) } else { Color::new(0.9, 0.3, 0.2, 1.0) };
        draw_rectangle(r.x, r.y + r.h - 3.0 * k, r.w, 3.0 * k, bar);
        if Some(u) == actor {
            draw_rectangle_lines(r.x, r.y, r.w, r.h, 2.0, GOLD);
        }
    }
    y += chip + 16.0 * k;

    // The log: the newest lines that fit above the buttons.
    let buttons_y = p.y + p.h - pad - BUTTON_H * k;
    let line_h = 18.0 * k;
    let log_top = y + 18.0 * k;
    let fit = (((buttons_y - 8.0 * k - log_top) / line_h).max(0.0) as usize).min(LOG_LINES);
    widgets::text_fit(tr("Log"), x, y, w, text_px(14.0), DIM);
    let lines = src.log(LOG_LINES, razdor::i18n::lang().code());
    let mut ly = log_top + 12.0 * k;
    for line in &lines[lines.len().saturating_sub(fit)..] {
        widgets::text_fit(line, x, ly, w, text_px(14.0), CREAM);
        ly += line_h;
    }

    // The buttons: wait, auto.
    let bw = (w - 8.0 * k) / 2.0;
    let wait_on = player_due && outcome.is_none();
    let waited = widgets::button(x, buttons_y, bw, BUTTON_H * k, tr("Wait"), wait_on) && wait_on;
    if widgets::toggle_button(x + bw + 8.0 * k, buttons_y, bw, BUTTON_H * k, tr("Auto"), *auto) {
        *auto = !*auto;
    }
    waited
}

/// The result banner over the board; `true` when «Дальше» was pressed.
fn draw_end(lay: &Layout, winner: Side) -> bool {
    let k = unit_px();
    let r = lay.grid.rect();
    let bw_box = (r.w * 0.8).min(360.0 * k);
    let bh_box = 150.0 * k;
    let bx = r.x + (r.w - bw_box) / 2.0;
    let by = r.y + (r.h - bh_box) / 2.0;
    draw_rectangle(bx, by, bw_box, bh_box, Color::new(0.08, 0.06, 0.04, 0.95));
    draw_rectangle_lines(bx, by, bw_box, bh_box, 2.0, GOLD);
    let cx = r.x + r.w / 2.0;
    let cy = by + 40.0 * k;
    let title = if winner == Side::Defenders { tr("Victory") } else { tr("Defeat") };
    chrome::shadow_centered(title, cx, cy, text_px(28.0), GOLD);
    let line = match winner {
        Side::Defenders => tr("The defenders hold the field"),
        Side::Attackers => tr("The attackers hold the field"),
    };
    widgets::text_centered(line, cx, by + text_px(78.0), text_px(16.0), INK);
    let bw = 200.0 * k;
    widgets::button(cx - bw / 2.0, by + bh_box - BUTTON_H * k - 12.0 * k, bw, BUTTON_H * k, tr("Next"), true)
}

/// The screen's background: the battle's terrain texture tiled over the screen (the DT battle's
/// background, the map's terrain there), else a dark plain.
fn draw_background(sw: f32, sh: f32, assets: &Assets, ground: Option<u8>) {
    draw_rectangle(0.0, 0.0, sw, sh, Color::new(0.06, 0.05, 0.04, 1.0));
    let Some(tex) = ground.and_then(|code| assets.dt.as_ref()?.terrain(code)) else { return };
    let (tw, th) = (tex.width().max(1.0), tex.height().max(1.0));
    let mut y = 0.0;
    while y < sh {
        let mut x = 0.0;
        while x < sw {
            draw_texture_ex(&tex, x, y, WHITE, DrawTextureParams { dest_size: Some(vec2(tw, th)), ..Default::default() });
            x += tw;
        }
        y += th;
    }
}

/// The ground of a tile: the battle's terrain texture (`ground`), else the DT plain's, else a
/// plain green.
fn draw_ground(assets: &Assets, r: Rect, ground: Option<u8>) {
    let tex = assets.dt.as_ref().and_then(|d| d.terrain(ground.unwrap_or(5)));
    match tex {
        Some(tex) => draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() }),
        None => draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.24, 0.32, 0.18, 1.0)),
    }
    draw_rectangle_lines(r.x, r.y, r.w, r.h, 1.0, Color::new(0.0, 0.0, 0.0, 0.18));
}

/// A DT picture drawn into a tile: scaled to the tile's width, bottom aligned (the pictures are
/// taller than a tile's share of ground, as the map draws them).
fn draw_sprite(tex: &Texture2D, r: Rect) {
    let h = r.w * tex.height() / tex.width();
    draw_texture_ex(tex, r.x, r.y + r.h - h, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, h)), ..Default::default() });
}

/// A tile's overlay: DT art from the install (a wall piece, the gate's arch, a tower on a
/// battlement, rocks as rubble), or Razdor's tint when the install lacks the picture.
/// `Objects.ugs` keys: buildings by (picture type, variant), decorations by (class, sprite).
fn draw_tile(assets: &Assets, tile: Tile, r: Rect) {
    let dt = assets.dt.as_ref();
    match tile {
        Tile::Open => {}
        Tile::Wall => match dt.and_then(|d| d.building(4, 4)) {
            Some(tex) => draw_sprite(&tex, r),
            None => {
                draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.46, 0.42, 0.37, 1.0));
                draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 2.0, Color::new(0.25, 0.22, 0.19, 1.0));
            }
        },
        Tile::Gate { hp } => {
            match dt.and_then(|d| d.building(11, 0)) {
                Some(tex) => draw_sprite(&tex, r),
                None => {
                    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.42, 0.26, 0.12, 1.0));
                    draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 2.0, Color::new(0.20, 0.12, 0.05, 1.0));
                }
            }
            // The gate's HP, on a small plate at the foot of the tile.
            let plate = Rect::new(r.x + r.w * 0.3, r.y + r.h - r.h * 0.26, r.w * 0.4, r.h * 0.22);
            draw_rectangle(plate.x, plate.y, plate.w, plate.h, Color::new(0.0, 0.0, 0.0, 0.7));
            widgets::text_centered(&hp.to_string(), plate.x + plate.w / 2.0, plate.y + plate.h * 0.8, (r.h * 0.16).max(8.0), CREAM);
        }
        Tile::Battlement => match dt.and_then(|d| d.building(4, 3)) {
            Some(tex) => draw_sprite(&tex, r),
            None => {
                draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.62, 0.58, 0.52, 1.0));
                draw_rectangle_lines(r.x + 2.0, r.y + 2.0, r.w - 4.0, r.h - 4.0, 1.0, Color::new(0.35, 0.32, 0.28, 1.0));
            }
        },
        Tile::Rubble => match dt.and_then(|d| d.object(8, 17)) {
            Some(tex) => draw_sprite(&tex, r),
            None => {
                draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.36, 0.34, 0.30, 0.9));
                for i in 0..5 {
                    let fx = r.x + r.w * (0.2 + 0.15 * i as f32);
                    let fy = r.y + r.h * (0.3 + 0.12 * ((i * 3) % 4) as f32);
                    draw_circle(fx, fy, r.w * 0.07, Color::new(0.55, 0.52, 0.48, 1.0));
                }
            }
        },
    }
}

/// A unit's portrait in `r`: the DT bust, mirrored for the attackers so the two sides face each
/// other, as the battle view draws them. Without the install, the placeholder.
fn draw_unit(assets: &Assets, kind: UnitId, team: Team, r: Rect) {
    match assets.dt_portrait(kind) {
        Some(tex) => draw_texture_ex(&tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), flip_x: team == Team::Enemy, ..Default::default() }),
        None => assets.draw_portrait(kind, team, r),
    }
}

fn draw_hp_bar(r: Rect, hp: i32, max: i32) {
    let frac = if max > 0 { (hp.max(0) as f32 / max as f32).min(1.0) } else { 0.0 };
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, 0.0, 0.0, 0.7));
    let fill = if frac > 0.5 { Color::new(0.25, 0.8, 0.3, 1.0) } else if frac > 0.25 { Color::new(0.9, 0.7, 0.2, 1.0) } else { Color::new(0.9, 0.25, 0.2, 1.0) };
    draw_rectangle(r.x, r.y, r.w * frac, r.h, fill);
}

/// The demo siege: the defenders' side on the left (the hero's), the attackers' on the right, a
/// wall down column 7 with one gate, two battlements behind it, rubble on both sides.
pub struct SiegeDemo;

/// The gate of the demo, and the cell in front of it on the attackers' side.
pub const SIEGE_GATE: (i32, i32) = (7, 3);
const SIEGE_FRONT: (i32, i32) = (8, 3);

impl FieldScript for SiegeDemo {
    fn setup(&mut self, _s: &mut Snapshot) -> Result<Option<razdor::rules::field::Placement>, String> {
        let mut tiles: Vec<(i32, i32, Tile)> = (0..8).filter(|&y| y != SIEGE_GATE.1).map(|y| (7, y, Tile::Wall)).collect();
        tiles.push((SIEGE_GATE.0, SIEGE_GATE.1, Tile::Gate { hp: 6 }));
        tiles.push((6, 1, Tile::Battlement));
        tiles.push((6, 6, Tile::Battlement));
        tiles.push((4, 2, Tile::Rubble));
        tiles.push((9, 5, Tile::Rubble));
        Ok(Some(razdor::rules::field::Placement { tiles, place: vec![], max_turns: Some(12) }))
    }

    /// The attackers' AI of the demo: strike a defender next to the planned cell, else walk
    /// toward the gate (or, once it is broken, the nearest defender) and strike the gate from
    /// there. The defenders keep the rules' AI.
    fn ai_turn(&mut self, s: &mut Snapshot) -> Result<Option<Action>, String> {
        let Some(me) = s.active else { return Ok(None) };
        let u = s.units[me].clone();
        if u.side != Side::Attackers {
            return Ok(None);
        }
        let Some(at) = u.at else { return Ok(None) };
        let defenders: Vec<UnitView> = s.units.iter().filter(|t| t.alive && t.side == Side::Defenders && t.at.is_some()).cloned().collect();
        let gate_standing = matches!(s.board.get(SIEGE_GATE.0, SIEGE_GATE.1), Some(Tile::Gate { hp: 1.. }));
        let goal = if gate_standing {
            SIEGE_FRONT
        } else {
            defenders.iter().filter_map(|d| d.at).min_by_key(|&c| (Board::distance(at, c), c)).unwrap_or(at)
        };
        let blocked: Vec<(i32, i32)> = s.units.iter().filter(|t| t.alive && t.id != me).filter_map(|t| t.at).collect();
        let costs = s.board.costs(at, &blocked);
        let mut best: Option<(i32, (i32, i32))> = None;
        for y in 0..s.board.h {
            for x in 0..s.board.w {
                let c = (x, y);
                let within = costs[(y * s.board.w + x) as usize].is_some_and(|cost| cost <= u.manevres);
                if within {
                    let key = (Board::distance(c, goal), c);
                    if best.is_none_or(|b| key < b) {
                        best = Some(key);
                    }
                }
            }
        }
        let dest = best.map(|(_, c)| c).unwrap_or(at);
        let attack = defenders
            .iter()
            .filter(|d| d.at.is_some_and(|c| Board::distance(c, dest) == 1))
            .min_by_key(|d| (d.hp, d.id))
            .map(|d| Target::Unit(d.id))
            .or_else(|| (gate_standing && Board::distance(dest, SIEGE_GATE) == 1).then_some(Target::Tile(SIEGE_GATE.0, SIEGE_GATE.1)));
        Ok(Some(Action { move_to: (dest != at).then_some(dest), attack }))
    }
}

/// The demo's defenders, by DT alias (the hero first), and its attackers.
pub const SIEGE_DEFENDERS: &[&str] = &["knight", "militiaman", "halberdier", "archer", "crossbowman", "cannoneer"];
pub const SIEGE_ATTACKERS: &[&str] = &["deadman", "dead_warrior", "ghoul", "necrolith", "ghost", "lich"];

/// A DT unit by its alias, resolved through `content`; `None` when the content lacks it.
pub fn unit_by_alias(content: &Content, alias: &str) -> Option<UnitId> {
    let id = UnitId(aliases::resolve(DtKind::Unit, alias)?);
    content.try_unit(id).map(|_| id)
}

/// The units of a side: the aliases the content resolves, then its first ids to fill up.
fn side_units(content: &Content, names: &[&str]) -> Vec<UnitId> {
    let mut ids: Vec<UnitId> = names.iter().filter_map(|n| unit_by_alias(content, n)).collect();
    let spare: Vec<UnitId> = content.unit_ids().filter(|id| !ids.contains(id)).collect();
    let mut spare = spare.into_iter();
    while ids.len() < names.len() {
        match spare.next() {
            Some(id) => ids.push(id),
            None => break,
        }
    }
    ids
}

/// The demo siege battle on `content`: the defenders on the left, the attackers on the right,
/// the demo's walls and gate; the defenders play the rules' AI and the attackers [`SiegeDemo`].
pub fn siege_battle(content: Arc<Content>) -> DemoField {
    let squad: Vec<Unit> = side_units(&content, SIEGE_DEFENDERS).into_iter().enumerate().map(|(i, id)| Unit::new(&content, id, Slot::new(Row::Front, i as u8))).collect();
    let enemies: Vec<Unit> = side_units(&content, SIEGE_ATTACKERS).into_iter().enumerate().map(|(i, id)| Unit::new(&content, id, Slot::new(Row::Front, i as u8))).collect();
    let battle_shadow = shadow(content, &squad, &enemies);
    let mut hooks: Box<dyn FieldScript> = Box::new(SiegeDemo);
    let fb = FieldBattle::new(battle_shadow, "siege-demo", 1, hooks.as_mut());
    DemoField { fb, hooks }
}

/// The snapshot scene `field:siege[:n|end]`: the demo siege at the start, after `n` unit actions
/// by the AI, or played to its end.
pub fn siege_screen(content: Arc<Content>, when: Option<&str>) -> Result<FieldScreen, String> {
    let mut demo = siege_battle(content);
    match when {
        None => {}
        Some("end") => {
            demo.fb.play_auto(demo.hooks.as_mut());
        }
        Some(n) => {
            let n: u32 = n.parse().map_err(|_| format!("field:siege takes a number of actions or end, not {n}"))?;
            for _ in 0..n {
                if !demo.auto_step() {
                    break;
                }
            }
        }
    }
    Ok(FieldScreen { view: FieldView::default(), demo: Some(demo) })
}

#[cfg(test)]
mod tests {
    use super::*;
    use razdor::rules::content::Content;

    fn grid() -> Grid {
        Grid::fit(Rect::new(10.0, 20.0, 480.0, 320.0), 12, 8, false)
    }

    #[test]
    fn a_turned_board_maps_its_tiles_both_ways() {
        let g = Grid::fit(Rect::new(0.0, 0.0, 320.0, 600.0), 12, 8, true);
        assert_eq!((g.w, g.h), (8, 12));
        for y in 0..8 {
            for x in 0..12 {
                assert_eq!(g.tile_at(g.centre(x, y)), Some((x, y)), "tile {x},{y}");
            }
        }
        // Defenders on the left columns sit at the top of the turned board.
        assert!(g.tile_rect(0, 0).y < g.tile_rect(11, 0).y);
    }

    #[test]
    fn a_tile_and_its_centre_map_back_to_each_other() {
        let g = grid();
        assert_eq!(g.cell, 40.0);
        for y in 0..g.h {
            for x in 0..g.w {
                assert_eq!(g.tile_at(g.centre(x, y)), Some((x, y)));
                let r = g.tile_rect(x, y);
                assert_eq!(g.tile_at((r.x + 0.5, r.y + 0.5)), Some((x, y)));
            }
        }
    }

    #[test]
    fn the_board_is_centred_and_points_off_it_hit_nothing() {
        let g = Grid::fit(Rect::new(0.0, 0.0, 600.0, 320.0), 12, 8, false);
        assert_eq!(g.cell, 40.0);
        assert!((g.x0 - 60.0).abs() < 1e-3, "centred across the spare width");
        assert_eq!(g.tile_at((g.x0 - 1.0, g.y0 + 1.0)), None);
        assert_eq!(g.tile_at((g.x0 + 12.0 * 40.0, g.y0 + 1.0)), None);
        assert_eq!(g.tile_at((g.x0 + 1.0, g.y0 + 8.0 * 40.0)), None);
    }

    #[test]
    fn a_tap_picks_a_living_unit_on_its_tile_then_the_tile() {
        let g = grid();
        let unit = |id, at, alive| UnitView {
            id,
            side: Side::Defenders,
            name: String::new(),
            level: 1,
            hp: 5,
            max_hp: 5,
            attack_blow: 1,
            attack_shot: 0,
            defence_blow: 0,
            defence_shot: 0,
            initiative: 1,
            manevres: 1,
            shot_range: 0,
            at,
            alive,
        };
        let units = [unit(0, Some((2, 1)), true), unit(1, Some((3, 1)), false)];
        let centre = |x, y| g.centre(x, y);
        assert_eq!(pick(&g, &units, centre(2, 1)), Some(Pick::Unit(0)));
        assert_eq!(pick(&g, &units, centre(3, 1)), Some(Pick::Tile(3, 1)), "a dead unit no longer holds its tile");
        assert_eq!(pick(&g, &units, centre(0, 0)), Some(Pick::Tile(0, 0)));
        assert_eq!(pick(&g, &units, (0.0, 0.0)), None);
    }

    #[test]
    fn the_layout_puts_the_board_beside_the_panel_or_above_it() {
        let wide = layout(1280.0, 800.0, 1.0, (12, 8), false);
        assert!(!wide.stacked);
        assert!(wide.grid.rect().x + wide.grid.rect().w <= wide.panel.x, "the board is left of the panel");
        let upright = layout(1080.0 / 2.75, 2400.0 / 2.75, 2.75, (12, 8), true);
        assert!(upright.stacked);
        assert!(upright.grid.rect().y + upright.grid.rect().h <= upright.panel.y, "the panel is under the board");
        assert!(upright.panel.y + upright.panel.h <= 2400.0 / 2.75 + 0.5, "the panel stays on the screen");
        // A phone held upright turns the board: its cells reach 40 dp (the touch target).
        assert!(upright.grid.turn);
        assert!(upright.grid.cell >= 40.0, "cell {}", upright.grid.cell);
    }

    #[test]
    fn options_and_attack_actions_follow_the_plan() {
        let stand = Action { move_to: Some((4, 4)), attack: None };
        let hit = Action { move_to: Some((4, 4)), attack: Some(Target::Unit(3)) };
        let here = Action { move_to: None, attack: Some(Target::Unit(3)) };
        let legal = [Action::default(), stand, hit, here];
        let o = Options::of(&legal, None);
        assert_eq!(o.cells, vec![(4, 4)]);
        assert_eq!(o.targets, vec![here]);
        let planned = Options::of(&legal, Some((4, 4)));
        assert_eq!(planned.targets, vec![hit]);
        assert_eq!(attack_action(&legal, Some((4, 4)), Target::Unit(3)), Some(hit));
        assert_eq!(attack_action(&legal, None, Target::Unit(3)), Some(here));
        assert_eq!(attack_action(&legal, None, Target::Unit(9)), None);
    }

    #[test]
    fn the_demo_plays_the_siege_to_an_outcome_by_auto_steps() {
        let content = Arc::new(Content::builtin());
        let mut d = siege_battle(content);
        let mut steps = 0;
        while d.acting().is_some() {
            assert!(d.auto_step(), "a due unit steps");
            steps += 1;
            assert!(steps < 2000, "the siege ends");
        }
        assert!(d.outcome().is_some(), "a side holds the field");
        assert!(!d.auto_step(), "nothing is played after the end");
        assert!(d.log(40, "en").iter().any(|l| l.contains("the battle is over")));
    }

    #[test]
    fn the_demo_lists_the_actions_of_the_unit_due_and_plays_one() {
        let content = Arc::new(Content::builtin());
        let mut d = siege_battle(content);
        let due = d.acting().expect("a unit is due at the start");
        let snap = d.snapshot().expect("a battle is under way");
        assert_eq!(snap.active, Some(due));
        let legal = d.legal(due);
        assert!(legal.contains(&Action::default()), "waiting is always legal");
        assert!(legal.iter().any(|a| a.move_to.is_some()), "a unit can move on the open board");
        let before = d.round();
        // Waiting ends the unit's turn; the next unit is due, or the round has moved on.
        d.act(Action::default()).expect("the due unit waits");
        assert!(d.round() >= before);
        assert_ne!(d.snapshot().and_then(|s| s.active), Some(due), "the unit that waited is not due again this round");
    }

    #[test]
    fn the_queue_lists_the_living_units_by_initiative() {
        let content = Arc::new(Content::builtin());
        let mut d = siege_battle(content);
        d.acting();
        let q = d.queue();
        let living = d.snapshot().expect("a battle is under way").units.iter().filter(|u| u.alive && u.at.is_some()).count();
        assert!(q.len() >= living, "the rest of the round and the next one");
    }
}
