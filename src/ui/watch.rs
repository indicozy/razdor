//! The replay's watching screen (`rules::recording::watch`): a recorded game played back on the map
//! in the player's own view settings, the camera on the hero (or where the player left it), at 1×
//! to 16×. A control strip under the map has the play and pause, the speed, the day, the timeline
//! with its marks (battles, towns taken, the hero's fall, the victory: click one or the bar to
//! seek), «Следить за героем» and «Выход». A battle begins with a choice, «Смотреть бой» or
//! «Пропустить бой» (by default after 3 s: skip at 4× and faster, watch below). Events and messages
//! show briefly and close themselves. A checkpoint that does not match stops the playback with a
//! window that names the version which recorded the replay.
//! Design: `docs/superpowers/specs/2026-11-11-replays-design.md`; the flow: `docs/flows/replays.md`.
//!
//! The state machine (`WatchView::tick`, `choose`, `seek_day`, ...) draws nothing and is tested
//! headless; the drawing and the input are in `frame`.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::Arc;

use macroquad::prelude::*;

use razdor::i18n::tr;
use razdor::rules::battle::Battle;
use razdor::rules::content::Content;
use razdor::rules::game::{Event, Game};
use razdor::rules::recording::watch::{Drift, Mark, MarkKind, Place, Pump, ReplayPlayer};
use razdor::trf;

use super::assets::Assets;
use super::battle_view::BattleView;
use super::chrome::{self, Skin, CREAM, GOLD, RED_TEXT};
use super::dialog::Dialog;
use super::display;
use super::story;
use super::widgets::*;
use super::window_layout;
use super::world_view::{self, MapView};
use super::Screen;

/// The playback's speeds (times the normal pace), in the order the buttons show them.
pub const SPEEDS: [u32; 5] = [1, 2, 4, 8, 16];
/// Seconds the battle choice waits before its default (skip at 4× and faster, watch below).
pub const CHOICE_SECS: f32 = 3.0;
/// Game seconds a real second of seeking covers («Перемотка…»).
const SEEK_RATE: f32 = 60.0;
/// A message closes after this many seconds at 1× (scaled by the speed, not under the floor).
const TOAST_SECS: f32 = 2.0;
const TOAST_FLOOR: f32 = 0.6;
/// A mark is hit within this many pixels of its centre.
const MARK_REACH: f32 = 9.0;

/// The battle choice's two answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Watch,
    Skip,
}

/// The default answer when the choice is left alone: skip at 4× and faster, watch below.
pub fn default_choice(speed: u32) -> Choice {
    if speed >= 4 {
        Choice::Skip
    } else {
        Choice::Watch
    }
}

/// Seconds a message stays up at `speed` times the pace: 2 s at 1×, a floor so it can be read.
pub fn toast_secs(speed: u32) -> f32 {
    (TOAST_SECS / speed.max(1) as f32).max(TOAST_FLOOR)
}

/// The fraction (0 to 1) of the way along the timeline from `first` to `last` that `day` is.
pub fn fraction_of(day: u64, first: u64, last: u64) -> f32 {
    if last <= first {
        return 0.0;
    }
    (day.saturating_sub(first) as f32 / (last - first) as f32).clamp(0.0, 1.0)
}

/// The day a fraction of the timeline stands for (the inverse of [`fraction_of`], rounded).
pub fn day_at(fraction: f32, first: u64, last: u64) -> u64 {
    first + (last.saturating_sub(first) as f32 * fraction.clamp(0.0, 1.0)).round() as u64
}

/// The x of a timeline fraction along `bar`.
pub fn mark_x(bar: Rect, fraction: f32) -> f32 {
    bar.x + bar.w * fraction.clamp(0.0, 1.0)
}

/// The index of the mark (at these fractions of the timeline) that `x` is on: the nearest within reach.
pub fn mark_at(bar: Rect, fractions: &[f32], x: f32) -> Option<usize> {
    fractions
        .iter()
        .enumerate()
        .map(|(i, &f)| (i, (mark_x(bar, f) - x).abs()))
        .filter(|&(_, d)| d <= MARK_REACH)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(i, _)| i)
}

/// The control strip's rectangles, in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Strip {
    pub panel: Rect,
    /// The timeline (on a phone held upright, a row of its own).
    pub bar: Rect,
    pub play: Rect,
    pub speeds: [Rect; 5],
    pub day: Rect,
    pub follow: Rect,
    pub exit: Rect,
}

/// The strip under the map on a `sw` × `sh` screen. `unit` is the pixels of one dp (or of the
/// desktop's interface scale). The desktop: one row, the timeline between the day and the follow
/// button. A phone (upright or sideways): the timeline on top, the play and the speeds under it,
/// the day, the follow button and the exit at the foot. Every button is 44 dp or more.
pub fn strip_layout(sw: f32, sh: f32, unit: f32, portrait: bool) -> Strip {
    let u = unit.max(0.1);
    let m = 6.0 * u;
    let gap = 4.0 * u;
    let btn = 44.0 * u;
    let play_w = 64.0 * u;
    let bar_h = 40.0 * u;
    // A screen narrower than 1000 dp (a phone, upright or sideways) stacks the strip: a one-row
    // timeline would be too short to read.
    if portrait || sw < 1000.0 * u {
        let h = m + bar_h + gap + btn + gap + btn + m;
        let top = sh - h;
        let inner = sw - 2.0 * m;
        let bar = Rect::new(m, top + m, inner, bar_h);
        let row_b = top + m + bar_h + gap;
        let play = Rect::new(m, row_b, play_w, btn);
        let speed_w = (inner - play_w - 5.0 * gap) / 5.0;
        let mut speeds = [Rect::new(0.0, 0.0, 0.0, 0.0); 5];
        for (i, s) in speeds.iter_mut().enumerate() {
            *s = Rect::new(m + play_w + gap + i as f32 * (speed_w + gap), row_b, speed_w, btn);
        }
        let row_c = row_b + btn + gap;
        let follow_w = 150.0 * u;
        let exit_w = 110.0 * u;
        let day_w = inner - follow_w - exit_w - 2.0 * gap;
        let day = Rect::new(m, row_c, day_w, btn);
        let follow = Rect::new(m + day_w + gap, row_c, follow_w, btn);
        let exit = Rect::new(m + day_w + gap + follow_w + gap, row_c, exit_w, btn);
        return Strip { panel: Rect::new(0.0, top, sw, h), bar, play, speeds, day, follow, exit };
    }
    let h = m + btn + m;
    let top = sh - h;
    let y = top + m;
    let play = Rect::new(m, y, play_w, btn);
    let speed_w = 48.0 * u;
    let mut speeds = [Rect::new(0.0, 0.0, 0.0, 0.0); 5];
    for (i, s) in speeds.iter_mut().enumerate() {
        *s = Rect::new(m + play_w + gap + i as f32 * (speed_w + gap), y, speed_w, btn);
    }
    let after_speeds = m + play_w + gap + 5.0 * (speed_w + gap);
    let day_w = 96.0 * u;
    let follow_w = 150.0 * u;
    let exit_w = 96.0 * u;
    let day = Rect::new(after_speeds, y, day_w, btn);
    let bar_x = after_speeds + day_w + gap;
    let bar_w = (sw - m - follow_w - exit_w - 2.0 * gap - bar_x).max(40.0 * u);
    let bar = Rect::new(bar_x, y + (btn - bar_h) / 2.0, bar_w, bar_h);
    let follow = Rect::new(bar_x + bar_w + gap, y, follow_w, btn);
    let exit = Rect::new(bar_x + bar_w + gap + follow_w + gap, y, exit_w, btn);
    Strip { panel: Rect::new(0.0, top, sw, h), bar, play, speeds, day, follow, exit }
}

/// The watching screen of the current game's record, back to `back` on «Выход». The file is the
/// live one, read as far as it is written (the day in play is written first). Err with the reason
/// when the game keeps no record (a demo, a setting turned off) or its file cannot be read.
pub fn open_record(game: &Game, back: Screen) -> Result<Screen, String> {
    let session = game.replay.clone().ok_or("this game keeps no record")?;
    let games = razdor::rules::save::games_dir().ok_or("no saves folder")?;
    let point = session.save_point();
    let path = razdor::rules::recording::session::replays_dir(&games).join(&point.file);
    let install = razdor::rules::save::install_dir();
    WatchView::open(&path, game.content.clone(), install.as_deref(), Some(back)).map(|v| Screen::Watch(Box::new(v)))
}

/// Where the playback is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mode {
    /// The game runs (when `playing`).
    Playing,
    /// A seek is under way («Перемотка…»).
    Seeking,
    /// A battle begins: the choice, with the seconds it waits before its default.
    Choosing { left: f32 },
    /// The battle is watched in its view.
    Battle,
    /// The file is played to its end.
    Ended,
    /// A checkpoint did not match: the playback stopped (see [`WatchView::drift`]).
    Drifted,
}

/// A message on the screen, closing itself.
#[derive(Clone, Debug, PartialEq)]
struct Toast {
    text: String,
    left: f32,
}

/// What the frame leaves to the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Exit {
    /// «Выход»: back to the screen the replay was opened from.
    Close,
}

/// The watching screen's state. The replay's game is `player.game()`.
pub struct WatchView {
    player: ReplayPlayer,
    map: MapView,
    /// Index into [`SPEEDS`].
    speed: usize,
    /// The game runs in `Mode::Playing` (the play button, Space).
    pub playing: bool,
    /// The camera follows the hero (the button «Следить за героем»).
    pub follow: bool,
    mode: Mode,
    toasts: Vec<Toast>,
    /// The battle the playback handed out, until the choice.
    pending: Option<Battle>,
    /// The battle being watched.
    battle: Option<Box<BattleView>>,
    /// The battle's own pause (its strip).
    battle_paused: bool,
    /// The drift the playback stopped at.
    drift: Option<Drift>,
    /// The screen «Выход» returns to.
    back: Option<Box<Screen>>,
}

impl WatchView {
    /// Opens the replay file at `path` on its first day, paused. `back` is the screen «Выход»
    /// returns to.
    pub fn open(path: &Path, content: Arc<Content>, install: Option<&Path>, back: Option<Screen>) -> Result<WatchView, String> {
        Self::with_player(ReplayPlayer::open(path, content, install)?, back)
    }

    /// The screen over a player already open (the tests open their files in memory).
    pub fn with_player(mut player: ReplayPlayer, back: Option<Screen>) -> Result<WatchView, String> {
        // The file's first place: the first segment's start (the game as the record began).
        player.begin_seek_place(Place { seg: 0, index: 0 }).map_err(|d| format!("{d:?}"))?;
        let mode = if player.game().is_some() { Mode::Playing } else { Mode::Ended };
        Ok(WatchView {
            player,
            map: MapView::default(),
            speed: 0,
            playing: false,
            follow: true,
            mode,
            toasts: Vec::new(),
            pending: None,
            battle: None,
            battle_paused: false,
            drift: None,
            back: back.map(Box::new),
        })
    }

    /// The screen's mode.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// The battle choice is up (the playback stopped at a battle's start).
    pub fn choosing(&self) -> bool {
        matches!(self.mode, Mode::Choosing { .. })
    }

    /// The game at the playback's place.
    pub fn game(&self) -> Option<&Game> {
        self.player.game()
    }

    /// The first and the last day of the timeline.
    pub fn days(&self) -> (u64, u64) {
        (self.player.first_day(), self.player.last_day())
    }

    /// The speed the playback runs at (1, 2, 4, 8 or 16).
    pub fn speed(&self) -> u32 {
        SPEEDS[self.speed]
    }

    /// Sets the speed to one of [`SPEEDS`] (an index).
    pub fn set_speed(&mut self, index: usize) {
        self.speed = index.min(SPEEDS.len() - 1);
    }

    /// Advances the screen by `real_dt` seconds: the messages close, the playback runs at its
    /// speed, a seek runs at its rate, and the battle choice counts down to its default.
    pub fn tick(&mut self, real_dt: f32) {
        for t in &mut self.toasts {
            t.left -= real_dt;
        }
        self.toasts.retain(|t| t.left > 0.0);
        match self.mode {
            Mode::Playing if self.playing => self.pump(real_dt * self.speed() as f32),
            Mode::Seeking => self.pump(real_dt * SEEK_RATE),
            Mode::Choosing { left } => {
                let left = left - real_dt;
                if left <= 0.0 {
                    self.choose(default_choice(self.speed()));
                } else {
                    self.mode = Mode::Choosing { left };
                }
            }
            _ => {}
        }
    }

    /// Plays the replay on by `budget` game seconds and takes what it reached.
    fn pump(&mut self, budget: f32) {
        match self.player.pump(budget, true) {
            Ok(Pump::Playing) => {
                if self.mode == Mode::Seeking && !self.player.seeking() {
                    self.mode = Mode::Playing;
                }
            }
            Ok(Pump::Battle) => {
                self.pending = self.player.take_battle();
                self.mode = Mode::Choosing { left: CHOICE_SECS };
            }
            Ok(Pump::Ended) => {
                self.playing = false;
                self.mode = Mode::Ended;
            }
            Err(d) => self.stop_at_drift(d),
        }
        self.take_messages();
    }

    fn stop_at_drift(&mut self, d: Drift) {
        self.playing = false;
        self.drift = Some(d);
        self.mode = Mode::Drifted;
    }

    /// The events the playback raised, as messages on the screen (a scenario's window, the rest),
    /// closing themselves at the speed.
    fn take_messages(&mut self) {
        let events = self.player.take_events();
        let Some(game) = self.player.game() else { return };
        let mut texts = Vec::new();
        for e in events {
            if let Event::Script(outcome) = &e {
                let mut shown = VecDeque::new();
                story::show(game, outcome, &mut shown);
                texts.extend(shown.iter().map(dialog_text));
            } else if let Some(t) = world_view::describe(&e, game) {
                texts.push(t);
            }
        }
        let left = toast_secs(self.speed());
        self.toasts.extend(texts.into_iter().filter(|t| !t.is_empty()).map(|text| Toast { text, left }));
    }

    /// The battle choice's answer: watch it in its view, or play it out at once.
    pub fn choose(&mut self, choice: Choice) {
        let Some(b) = self.pending.take() else { return };
        let actions = self.player.take_battle_actions();
        match choice {
            Choice::Watch => {
                self.battle = Some(Box::new(BattleView::replay(b, actions)));
                self.battle_paused = false;
                self.mode = Mode::Battle;
            }
            Choice::Skip => match self.player.return_battle(b, &actions) {
                Ok(()) => self.mode = Mode::Playing,
                Err(d) => self.stop_at_drift(d),
            },
        }
    }

    /// The watched battle's «Пропустить»: its rest is played out at once, and the replay goes on.
    pub fn skip_battle(&mut self) {
        let Some(view) = self.battle.take() else { return };
        let (b, rest) = view.take_replay();
        match self.player.return_battle(b, &rest) {
            Ok(()) => self.mode = Mode::Playing,
            Err(d) => self.stop_at_drift(d),
        }
    }

    /// Seeks to a day of the timeline («Перемотка…» while it runs).
    pub fn seek_day(&mut self, day: u64) {
        match self.player.begin_seek_day(day) {
            Ok(()) => self.begin_seek(),
            Err(d) => self.stop_at_drift(d),
        }
    }

    /// Seeks to a mark's place.
    pub fn seek_place(&mut self, place: Place) {
        match self.player.begin_seek_place(place) {
            Ok(()) => self.begin_seek(),
            Err(d) => self.stop_at_drift(d),
        }
    }

    fn begin_seek(&mut self) {
        self.toasts.clear();
        self.drift = None;
        self.mode = Mode::Seeking;
    }

    /// The drift window's «Смотреть до этого места»: the place the game stood at before the
    /// mismatch, paused there.
    pub fn watch_to_drift(&mut self) {
        let at = self.player.last_good();
        self.seek_place(at);
        self.playing = false;
    }

    /// The screen «Выход» returns to (the map when none was given).
    pub fn close(&mut self) -> Screen {
        self.back.take().map_or(Screen::WorldMap, |b| *b)
    }

    /// The battle's view has ended: its rest (none, when it ended) goes back to the playback. A
    /// battle the recording expects to go on from is a drift.
    fn finish_battle(&mut self) {
        let Some(view) = self.battle.take() else { return };
        if view.replay_stuck() {
            self.stop_at_drift(Drift { day: 0, version: String::new(), at: self.player.place(), reason: "the battle waits for a recorded action it does not have".into() });
            return;
        }
        let (b, rest) = view.take_replay();
        match self.player.return_battle(b, &rest) {
            Ok(()) => self.mode = Mode::Playing,
            Err(d) => self.stop_at_drift(d),
        }
    }

    /// One frame: the time, the map or the battle, the strip, the messages, the choice and the
    /// drift window. Returns «Выход» when it is pressed.
    pub fn frame(&mut self, assets: &Assets, real_dt: f32) -> Option<Exit> {
        let real_dt = real_dt.clamp(0.0, 0.1);
        self.tick(real_dt);
        let dp = display::dp_px();
        let portrait = dp.is_some() && window_layout::portrait();
        let unit = dp.unwrap_or_else(chrome::k);
        let (sw, sh) = (screen_width(), screen_height());
        let strip = strip_layout(sw, sh, unit, portrait);
        if self.mode == Mode::Battle {
            if self.draw_battle(assets) {
                self.finish_battle();
            }
        } else if let Some(game) = self.player.game() {
            world_view::set_watch_area(Some(Rect::new(0.0, 0.0, sw, strip.panel.y)));
            world_view::watch_map(game, assets, &mut self.map, self.follow, (real_dt * 1000.0) as i64);
            world_view::set_watch_area(None);
        } else {
            clear_background(Color::from_rgba(10, 12, 10, 255));
        }
        let mut exit = if self.mode == Mode::Battle { self.battle_strip(&strip) } else { self.strip(&strip, unit) };
        self.draw_toasts(unit, sw);
        if self.mode == Mode::Drifted {
            exit = exit.or(self.drift_window(sw, sh, unit));
        }
        if key(KeyCode::Escape) && self.mode != Mode::Battle {
            exit = Some(Exit::Close);
        }
        exit
    }

    /// Draws the watched battle through the playback (its «Максимум» moves are the recorded ones);
    /// true when the battle has ended.
    fn draw_battle(&mut self, assets: &Assets) -> bool {
        let (speed, paused) = (self.speed() as u8, self.battle_paused);
        let WatchView { player, battle, .. } = self;
        let Some(view) = battle.as_mut() else { return false };
        view.set_pace(speed);
        view.paused = paused;
        player.in_playback(|g| view.frame_replay(g, assets)).unwrap_or(true)
    }

    /// The map's strip: the play button, the speed, the day, the timeline with its marks (or the
    /// battle choice over it), the follow button and «Выход».
    fn strip(&mut self, s: &Strip, unit: f32) -> Option<Exit> {
        surface_strip(s.panel);
        if button(s.play.x, s.play.y, s.play.w, s.play.h, if self.playing { tr("Pause") } else { tr("Play") }, true) || key(KeyCode::Space) {
            self.playing = !self.playing;
        }
        for (i, r) in s.speeds.iter().enumerate() {
            if toggle_button(r.x, r.y, r.w, r.h, &format!("{}×", SPEEDS[i]), self.speed == i) {
                self.set_speed(i);
            }
        }
        let (first, last) = self.days();
        let day = self.game().map_or(first, |g| g.clock.day_index());
        chrome::shadow_centered(&trf!("Day {n}", n = day.saturating_sub(first) + 1), s.day.x + s.day.w / 2.0, s.day.y + s.day.h * 0.65, 20.0 * unit, CREAM);
        if toggle_button(s.follow.x, s.follow.y, s.follow.w, s.follow.h, tr("Follow the hero"), self.follow) {
            self.follow = !self.follow;
        }
        if matches!(self.mode, Mode::Choosing { .. }) {
            self.choice_buttons(s.bar);
        } else if self.mode == Mode::Seeking {
            chrome::shadow_centered(tr("Seeking..."), s.bar.x + s.bar.w / 2.0, s.bar.y + s.bar.h * 0.7, 16.0 * unit, GOLD);
        } else {
            self.timeline(s.bar, first, last);
        }
        button(s.exit.x, s.exit.y, s.exit.w, s.exit.h, tr("Exit"), true).then_some(Exit::Close)
    }

    /// The battle choice over the timeline: «Смотреть бой» and «Пропустить бой».
    fn choice_buttons(&mut self, bar: Rect) {
        let gap = 6.0;
        let w = (bar.w - gap) / 2.0;
        if button(bar.x, bar.y, w, bar.h, tr("Watch the battle"), true) {
            self.choose(Choice::Watch);
        }
        if button(bar.x + w + gap, bar.y, w, bar.h, tr("Skip the battle"), true) {
            self.choose(Choice::Skip);
        }
    }

    /// The timeline: the bar with its fill, the marks (a click on one seeks to its place, a click
    /// elsewhere on the bar to its day) and the hover's tooltip.
    fn timeline(&mut self, bar: Rect, first: u64, last: u64) {
        draw_rectangle(bar.x, bar.y, bar.w, bar.h, Color::from_rgba(18, 14, 10, 255));
        let now = self.game().map_or(first, |g| g.clock.day_index());
        draw_rectangle(bar.x, bar.y, bar.w * fraction_of(now, first, last), bar.h, Color::from_rgba(120, 92, 40, 255));
        draw_rectangle_lines(bar.x, bar.y, bar.w, bar.h, 1.0, GOLD);
        let fractions: Vec<f32> = self.player.marks.iter().map(|m| fraction_of(m.day, first, last)).collect();
        for (m, &fr) in self.player.marks.iter().zip(&fractions) {
            let color = match m.kind {
                MarkKind::Day => continue,
                MarkKind::Battle { .. } => RED_TEXT,
                MarkKind::Town { .. } => GOLD,
                MarkKind::HeroDown => Color::from_rgba(40, 40, 40, 255),
                MarkKind::Victory => Color::from_rgba(120, 240, 130, 255),
            };
            // A mark at either end of the bar stays on it.
            let x = mark_x(bar, fr).clamp(bar.x + bar.h * 0.35, bar.x + bar.w - bar.h * 0.35);
            draw_mark(&m.kind, x, bar.y + bar.h / 2.0, bar.h * 0.35, color);
        }
        let (mx, my) = pointer();
        if !bar.contains(vec2(mx, my)) {
            return;
        }
        match mark_at(bar, &fractions, mx) {
            Some(i) => {
                let m = self.player.marks[i].clone();
                tooltip(&mark_lines(&m, first));
                if clicked() {
                    self.seek_place(m.at);
                }
            }
            None => {
                if clicked() {
                    self.seek_day(day_at((mx - bar.x) / bar.w, first, last));
                }
            }
        }
    }

    /// The battle's strip: play or pause, the speed and «Пропустить».
    fn battle_strip(&mut self, s: &Strip) -> Option<Exit> {
        surface_strip(s.panel);
        if button(s.play.x, s.play.y, s.play.w, s.play.h, if self.battle_paused { tr("Play") } else { tr("Pause") }, true) || key(KeyCode::Space) {
            self.battle_paused = !self.battle_paused;
        }
        for (i, r) in s.speeds.iter().enumerate() {
            if toggle_button(r.x, r.y, r.w, r.h, &format!("{}×", SPEEDS[i]), self.speed == i) {
                self.set_speed(i);
            }
        }
        if button(s.exit.x, s.exit.y, s.exit.w, s.exit.h, tr("Skip"), true) {
            self.skip_battle();
        }
        None
    }

    /// The messages, one under the other at the top of the screen, centred.
    fn draw_toasts(&self, unit: f32, sw: f32) {
        let mut y = 8.0 * unit;
        for t in &self.toasts {
            let size = 16.0 * unit;
            let w = (measure(&t.text, size).width + 24.0 * unit).min(sw - 16.0 * unit);
            let h = size * 1.8;
            let x = (sw - w) / 2.0;
            draw_rectangle(x, y, w, h, Color::from_rgba(20, 16, 10, 225));
            draw_rectangle_lines(x, y, w, h, 1.0, GOLD);
            chrome::shadow_centered(&t.text, sw / 2.0, y + h * 0.7, size, CREAM);
            y += h + 4.0 * unit;
        }
    }

    /// The drift window: the version that recorded the replay, «Смотреть до этого места» and «Выход».
    fn drift_window(&mut self, sw: f32, sh: f32, unit: f32) -> Option<Exit> {
        let version = self.drift.as_ref()?.version.clone();
        let w = (520.0 * unit).min(sw - 16.0 * unit);
        let h = 200.0 * unit;
        let (inner, _) = chrome::window(Rect::new((sw - w) / 2.0, (sh - h) / 2.0, w, h), tr("Record"), Skin::Marble, false);
        let text = trf!("The record was made in version {v} and diverges from here on", v = version);
        for (i, line) in wrap(&text, inner.w - 24.0 * unit, 16.0 * unit).iter().enumerate() {
            chrome::shadow_text(line, inner.x + 12.0 * unit, inner.y + (34.0 + i as f32 * 24.0) * unit, 16.0 * unit, CREAM);
        }
        let by = inner.y + inner.h - 52.0 * unit;
        let bw = (inner.w - 36.0 * unit) / 2.0;
        if button(inner.x + 12.0 * unit, by, bw, 40.0 * unit, tr("Watch up to here"), true) {
            self.watch_to_drift();
        }
        button(inner.x + 24.0 * unit + bw, by, bw, 40.0 * unit, tr("Exit"), true).then_some(Exit::Close)
    }
}

/// A dialog's text as a message: its title and its lines.
fn dialog_text(d: &Dialog) -> String {
    let body = d.text.join(" ");
    match (d.title.is_empty(), body.is_empty()) {
        (true, _) => body,
        (false, true) => d.title.clone(),
        (false, false) => format!("{}: {body}", d.title),
    }
}

/// The hover text of a mark: what it is, and its day.
fn mark_lines(m: &Mark, first: u64) -> Vec<(String, Color)> {
    let (head, body) = match &m.kind {
        MarkKind::Battle { player, enemy, .. } => (tr("Battle").to_string(), format!("{} - {}", player.join(", "), enemy.join(", "))),
        MarkKind::Town { name } => (trf!("Taken: {name}", name = name), String::new()),
        MarkKind::HeroDown => (tr("The hero fell").to_string(), String::new()),
        MarkKind::Victory => (tr("Victory").to_string(), String::new()),
        MarkKind::Day => (trf!("Day {n}", n = m.day.saturating_sub(first) + 1), String::new()),
    };
    let mut lines = vec![(head, GOLD)];
    if !body.is_empty() {
        lines.push((body, CREAM));
    }
    if !matches!(m.kind, MarkKind::Day) {
        lines.push((trf!("Day {n}", n = m.day.saturating_sub(first) + 1), CREAM));
    }
    lines
}

/// Draws a mark: a cross for a battle, a flag for a town, a dark bar for the hero's fall, a
/// diamond for the victory.
fn draw_mark(kind: &MarkKind, x: f32, y: f32, s: f32, color: Color) {
    match kind {
        MarkKind::Battle { .. } => {
            draw_line(x - s, y - s, x + s, y + s, 3.0, color);
            draw_line(x - s, y + s, x + s, y - s, 3.0, color);
        }
        MarkKind::Town { .. } => {
            draw_line(x, y - s, x, y + s, 2.0, CREAM);
            draw_rectangle(x, y - s, s, s * 0.8, color);
        }
        MarkKind::HeroDown => draw_rectangle(x - s * 0.4, y - s, s * 0.8, s * 2.0, color),
        MarkKind::Victory => {
            draw_triangle(vec2(x, y - s), vec2(x + s, y), vec2(x, y + s), color);
            draw_triangle(vec2(x, y - s), vec2(x - s, y), vec2(x, y + s), color);
        }
        MarkKind::Day => {}
    }
}

/// The strip's background, in the game's own panel style.
fn surface_strip(r: Rect) {
    chrome::surface(r, Skin::Marble);
    chrome::silver_frame(r, 1.0);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Rect {
        Rect::new(100.0, 50.0, 400.0, 20.0)
    }

    fn inside(outer: Rect, r: Rect) -> bool {
        r.x >= outer.x - 0.01 && r.y >= outer.y - 0.01 && r.x + r.w <= outer.x + outer.w + 0.01 && r.y + r.h <= outer.y + outer.h + 0.01
    }

    #[test]
    fn the_default_choice_skips_at_four_times_and_watches_below() {
        assert_eq!(default_choice(1), Choice::Watch);
        assert_eq!(default_choice(2), Choice::Watch);
        assert_eq!(default_choice(4), Choice::Skip);
        assert_eq!(default_choice(16), Choice::Skip);
    }

    #[test]
    fn a_message_closes_sooner_as_the_speed_rises_but_not_under_its_floor() {
        assert_eq!(toast_secs(1), 2.0);
        assert_eq!(toast_secs(2), 1.0);
        assert_eq!(toast_secs(4), TOAST_FLOOR, "2 s / 4 is under the floor");
        assert_eq!(toast_secs(16), TOAST_FLOOR);
        assert!(toast_secs(2) > toast_secs(4));
    }

    #[test]
    fn a_place_on_the_bar_is_a_day_and_back() {
        assert_eq!(fraction_of(10, 10, 110), 0.0);
        assert_eq!(fraction_of(110, 10, 110), 1.0);
        assert!((fraction_of(60, 10, 110) - 0.5).abs() < 1e-6);
        assert_eq!(day_at(0.5, 10, 110), 60);
        for day in [10u64, 11, 37, 99, 110] {
            assert_eq!(day_at(fraction_of(day, 10, 110), 10, 110), day);
        }
        assert_eq!(day_at(2.0, 10, 110), 110, "past the bar is the last day");
        assert_eq!(fraction_of(5, 10, 10), 0.0, "a file of one day");
    }

    #[test]
    fn a_click_on_a_mark_is_the_nearest_mark_within_reach() {
        let b = bar();
        let fr = [0.0, 0.5, 1.0];
        assert_eq!(mark_at(b, &fr, mark_x(b, 0.5)), Some(1));
        assert_eq!(mark_at(b, &fr, mark_x(b, 0.5) + MARK_REACH), Some(1), "within reach");
        assert_eq!(mark_at(b, &fr, mark_x(b, 0.5) + MARK_REACH + 1.0), None, "out of reach");
        assert_eq!(mark_at(b, &[0.2, 0.25], mark_x(b, 0.24)), Some(1), "the nearer of two");
        assert_eq!(mark_at(b, &[], 200.0), None);
    }

    #[test]
    fn the_strip_fits_the_desktop_and_the_phones_in_both_orientations() {
        // The desktop at 1920 x 1080 (interface scale 1), a phone held upright at 360 x 780 dp and
        // the same phone landscape (3 px to a dp).
        for (sw, sh, unit, portrait) in [(1920.0, 1080.0, 1.0, false), (1080.0, 2340.0, 3.0, true), (2340.0, 1080.0, 3.0, false)] {
            let s = strip_layout(sw, sh, unit, portrait);
            assert!(inside(Rect::new(0.0, 0.0, sw, sh), s.panel), "{sw}x{sh}: the strip is on the screen");
            for r in [s.play, s.speeds[0], s.speeds[4], s.day, s.follow, s.exit] {
                assert!(inside(s.panel, r), "{sw}x{sh}: {r:?} is inside the strip");
                assert!(r.w >= 40.0 * unit - 0.01 && r.h >= 40.0 * unit - 0.01, "{sw}x{sh}: {r:?} is a 40 dp touch target");
            }
            assert!(inside(s.panel, s.bar), "{sw}x{sh}: the timeline is inside the strip");
            assert!(s.bar.w >= 120.0 * unit, "{sw}x{sh}: the timeline has room");
        }
    }

    #[test]
    fn the_phone_upright_stacks_the_timeline_over_the_buttons() {
        let s = strip_layout(1080.0, 2340.0, 3.0, true);
        assert!(s.bar.y + s.bar.h <= s.play.y, "the timeline is above the play row");
        assert_eq!(s.follow.y, s.exit.y, "the follow button and the exit share the foot row");
        assert!(s.follow.y > s.play.y, "the foot row is under the play row");
    }

    #[test]
    fn the_desktop_timeline_sits_between_the_day_and_the_follow_button() {
        let d = strip_layout(1920.0, 1080.0, 1.0, false);
        assert_eq!(d.play.y, d.speeds[0].y, "the buttons share the row");
        assert!(d.bar.x >= d.day.x + d.day.w, "the day is on the left of the timeline");
        assert!(d.bar.x + d.bar.w <= d.follow.x, "the follow button is on its right");
    }

    /// The screen's states on a real record (`RAZDOR_DT_DIR` and `RAZDOR_WATCH_RECORD`, the file made
    /// by `write_a_sample_record_for_the_screenshots`): play to the first battle's choice, skip it,
    /// seek to a mark, and stop at a drift that is put into the file in memory.
    #[test]
    fn the_screen_plays_a_record_through_its_states() {
        use razdor::rules::recording::file::read;
        use razdor::rules::recording::watch::ReplayPlayer;
        use razdor::rules::content::Content;
        use std::sync::Arc;
        razdor::dt::install::load_dotenv();
        let (Some(dir), Some(record)) = (std::env::var_os(razdor::dt::install::ENV_VAR), std::env::var_os("RAZDOR_WATCH_RECORD")) else { return };
        let Ok(install) = razdor::dt::install::DtInstall::load(std::path::Path::new(&dir)) else { return };
        let content = Arc::new(Content::from_dt(&install));
        let record = std::path::PathBuf::from(record);
        let open = || ReplayPlayer::open(&record, content.clone(), Some(&install.dir)).unwrap();
        // Play: the time runs at the speed while playing, and stops when paused.
        let mut w = WatchView::with_player(open(), None).unwrap();
        assert_eq!(w.mode(), Mode::Playing);
        let day0 = w.game().unwrap().clock.day_index();
        w.tick(0.0);
        assert_eq!(w.game().unwrap().clock.day_index(), day0, "paused: no time");
        w.playing = true;
        // Play until the first battle is chosen (no choice is taken: the default waits 3 s).
        for _ in 0..200_000 {
            if w.choosing() || w.mode() == Mode::Ended {
                break;
            }
            w.tick(1.0 / 30.0);
        }
        assert!(w.choosing(), "the battle's choice is up");
        // Choosing alone: at 1× the default is to watch, after its three seconds.
        let mut left = 0.0;
        while w.choosing() && left < CHOICE_SECS + 1.0 {
            w.tick(0.5);
            left += 0.5;
        }
        assert_eq!(w.mode(), Mode::Battle, "the default at 1× watches the battle");
        // The watched battle is skipped: the playback plays out the rest and goes on.
        w.skip_battle();
        assert_eq!(w.mode(), Mode::Playing);
        // A seek to a mark's place and a seek by day: the seek runs, then the mode is Playing again.
        let mark = w.player.marks.iter().find(|m| matches!(m.kind, MarkKind::Battle { .. })).unwrap().clone();
        w.seek_place(mark.at);
        assert_eq!(w.mode(), Mode::Seeking);
        for _ in 0..200_000 {
            if w.choosing() {
                break;
            }
            w.tick(1.0 / 30.0);
        }
        assert!(w.choosing(), "a seek to a battle's mark stops at the battle's choice");
        w.choose(Choice::Skip);
        let (first, last) = w.days();
        w.seek_day(day_at(0.5, first, last));
        while w.mode() == Mode::Seeking {
            w.tick(1.0 / 30.0);
        }
        assert!(w.game().unwrap().clock.day_index() >= day_at(0.5, first, last), "the seek reached its day");
        // Speed: a faster playback covers more game time per second, and a message closes sooner.
        w.set_speed(4);
        assert_eq!(w.speed(), 16, "the fifth speed is 16×");
        // A drift: the second check of the file changed in memory.
        let mut file = read(&record).unwrap();
        let k = file.segments[0].keyframes.get(1).unwrap().after - 1;
        if let razdor::rules::recording::stream::Input::Engine(razdor::rules::recording::stream::EngineInput::Check { hash }) = &mut file.segments[0].entries[k].input {
            *hash ^= 1;
        }
        let mut d = WatchView::with_player(ReplayPlayer::new(file, content.clone(), Some(&install.dir)), None).unwrap();
        d.playing = true;
        for _ in 0..2_000_000 {
            if d.mode() == Mode::Drifted || d.mode() == Mode::Ended {
                break;
            }
            if d.choosing() {
                d.choose(Choice::Skip);
            }
            d.tick(1.0 / 30.0);
        }
        assert_eq!(d.mode(), Mode::Drifted, "the mismatch stops the playback");
        assert!(d.drift.as_ref().is_some_and(|x| x.version == env!("CARGO_PKG_VERSION")), "the window names the recording's version");
        // «Смотреть до этого места»: back to the last good place, paused.
        d.watch_to_drift();
        assert_eq!(d.mode(), Mode::Seeking);
        while d.mode() == Mode::Seeking {
            d.tick(1.0 / 30.0);
        }
        assert_eq!(d.mode(), Mode::Playing);
        assert!(!d.playing, "paused where the record still matches");
        assert!(d.drift.is_none(), "the drift is left behind");
    }
}
