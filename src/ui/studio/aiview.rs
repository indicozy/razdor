//! The Тест workspace's ИИ view («Просмотр ИИ», UX6): the shown map's AI run for some days by the same sim
//! as `razdor-pack sim` (`sim_map_observed`, on a worker thread, with Cancel), then the armies on the map
//! at the day the slider shows, their trails over the last few days, and the story's moments that have
//! fired by that day (a trigger's cell marked with its id). The log under the map lists the moments by
//! day; a click on a line moves the slider there and jumps to the moment's object (`studio::nav`).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use egui::Ui;
use egui_macroquad::egui;
use macroquad::prelude as mq;
use razdor::dt::install::DtInstall;
use razdor::i18n::tr;
use razdor::pack::load::Package;
use razdor::pack::sim::{sim_map_observed, Choices, Frame, Recording, SimOptions, SimResult};
use razdor::rules::triggers::{Ending, Trace};
use razdor::trf;

use super::canvas::Cam;

/// The days the viewer runs when the box is not changed.
pub const DEFAULT_DAYS: u64 = 30;
/// The longest the most recent trails reach back, in days.
const TRAIL_DAYS: u64 = 5;
/// The most days the box takes.
const MAX_DAYS: u64 = 365;

/// Army colours by faction (the faction's byte modulo the list).
const FACTION_COLOURS: [mq::Color; 6] = [
    mq::Color::new(0.90, 0.30, 0.30, 1.0),
    mq::Color::new(0.35, 0.60, 0.95, 1.0),
    mq::Color::new(0.45, 0.85, 0.45, 1.0),
    mq::Color::new(0.95, 0.80, 0.30, 1.0),
    mq::Color::new(0.80, 0.50, 0.95, 1.0),
    mq::Color::new(0.95, 0.60, 0.25, 1.0),
];

enum Msg {
    Day(u64),
    Done(Result<Option<(SimResult, Recording)>, String>),
}

/// The run on its worker: what it has reached, and the channel its messages come on.
struct Job {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

/// The AI viewer's state: the mode, the day box and slider, the run and its recording.
pub struct AiView {
    /// Days to run.
    pub days: u64,
    /// The day the slider shows.
    pub day: u64,
    job: Option<Job>,
    /// The last day the run reached.
    pub progress: u64,
    pub result: Option<SimResult>,
    pub recording: Option<Recording>,
    /// Why the last run failed (the map does not compile, the install is missing).
    pub error: Option<String>,
    /// The map the recording is of.
    pub map: Option<String>,
}

/// What the strip asked for this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Req {
    None,
    Start,
    Cancel,
}

impl Default for AiView {
    fn default() -> AiView {
        AiView { days: DEFAULT_DAYS, day: 0, job: None, progress: 0, result: None, recording: None, error: None, map: None }
    }
}

impl AiView {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// Starts the run of `map` for `self.days` days on a worker thread. The recording is replaced
    /// when it comes.
    pub fn start(&mut self, pkg: Arc<Package>, install: Arc<DtInstall>, map: &str) {
        self.cancel();
        self.recording = None;
        self.result = None;
        self.error = None;
        self.progress = 0;
        self.day = 0;
        self.map = Some(map.to_string());
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (days, map) = (self.days, map.to_string());
        std::thread::spawn(move || {
            let opts = SimOptions { days, choices: Choices::First, trace: true, preset: Vec::new(), seed: None };
            let progress = tx.clone();
            let run = sim_map_observed(&pkg, &install, &map, &opts, &stop, &mut |d| {
                let _ = progress.send(Msg::Day(d));
            });
            let _ = tx.send(Msg::Done(run.map_err(|r| r.to_text())));
        });
        self.job = Some(Job { rx, cancel });
    }

    /// Stops the run (its result is dropped).
    pub fn cancel(&mut self) {
        if let Some(job) = self.job.take() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Takes what the worker has sent since the last frame.
    pub fn poll(&mut self) {
        loop {
            let Some(job) = &self.job else { return };
            match job.rx.try_recv() {
                Ok(Msg::Day(d)) => self.progress = d,
                Ok(Msg::Done(Ok(Some((result, recording))))) => {
                    self.job = None;
                    self.result = Some(result);
                    self.recording = Some(recording);
                    self.day = self.day.min(self.last_day());
                    return;
                }
                Ok(Msg::Done(Ok(None))) => {
                    self.job = None;
                    return;
                }
                Ok(Msg::Done(Err(text))) => {
                    self.job = None;
                    self.error = Some(text);
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.job = None;
                    return;
                }
            }
        }
    }

    /// The last day of the recording (0 with none).
    pub fn last_day(&self) -> u64 {
        self.recording.as_ref().and_then(|r| r.frames.last()).map_or(0, |f| f.day)
    }

    /// The frame the slider shows: the last frame at or before its day.
    pub fn frame(&self) -> Option<&Frame> {
        let rec = self.recording.as_ref()?;
        rec.frames.iter().rev().find(|f| f.day <= self.day)
    }

    /// Runs the map for `self.days` days in the calling thread (the snapshot's picture).
    pub fn run_now(&mut self, pkg: &Package, install: &DtInstall, map: &str) -> Result<(), String> {
        let opts = SimOptions { days: self.days, choices: Choices::First, trace: true, preset: Vec::new(), seed: None };
        let never = AtomicBool::new(false);
        match sim_map_observed(pkg, install, map, &opts, &never, &mut |_| {}) {
            Ok(Some((result, recording))) => {
                self.result = Some(result);
                self.recording = Some(recording);
                self.map = Some(map.to_string());
                self.error = None;
                Ok(())
            }
            Ok(None) => Err("cancelled".into()),
            Err(r) => Err(r.to_text()),
        }
    }

    /// The AI tab's controls: the days box, Run or Cancel (with the progress), the run's result and its error.
    /// `ready` is why the run cannot start, if it cannot. Returns what the controls asked for.
    pub fn controls(&mut self, ui: &mut Ui, ready: Result<(), &'static str>) -> Req {
        let mut req = Req::None;
        ui.horizontal(|ui| {
            ui.label(tr("Days"));
            ui.add(egui::DragValue::new(&mut self.days).range(1..=MAX_DAYS));
            if self.running() {
                if ui.button(tr("Cancel")).clicked() {
                    req = Req::Cancel;
                }
                ui.weak(trf!("day {day} of {total}", day = self.progress, total = self.days));
            } else if ui.add_enabled(ready.is_ok(), egui::Button::new(tr("Run"))).clicked() {
                req = Req::Start;
            }
            if let Err(why) = ready {
                ui.weak(why);
            }
            if let Some(result) = &self.result {
                let mut line = trf!("{battles} AI battles", battles = result.ai_battles);
                if let Some(day) = result.victory_day {
                    line.push_str(" · ");
                    line.push_str(&trf!("victory on day {day}", day = day.floor() as u64));
                }
                if let Some(day) = result.defeat_day {
                    line.push_str(" · ");
                    line.push_str(&trf!("defeat on day {day}", day = day.floor() as u64));
                }
                ui.weak(line);
            }
        });
        if let Some(error) = &self.error {
            ui.colored_label(egui::Color32::from_rgb(0xe0, 0x5a, 0x5a), error.lines().next().unwrap_or(""));
        }
        req
    }

    /// The day slider and the log of the story's moments, under the map. A click on a line moves the slider
    /// to its day and returns the moment's index (the caller jumps to it). `None` when nothing was clicked or
    /// there is no recording.
    pub fn timeline(&mut self, ui: &mut Ui) -> Option<usize> {
        self.recording.as_ref()?;
        let last = self.last_day();
        ui.add(egui::Slider::new(&mut self.day, 0..=last).text(tr("Day")));
        let day = self.day;
        let mut clicked = None;
        if let Some(rec) = &self.recording {
            egui::ScrollArea::vertical().id_salt("aiview-log").max_height(120.0).auto_shrink([false, true]).show(ui, |ui| {
                if rec.moments.is_empty() {
                    ui.weak(tr("No story moments."));
                }
                for (i, m) in rec.moments.iter().enumerate() {
                    if ui.selectable_label(m.day == day, format!("{}: {}", m.day, moment_text(&m.what))).clicked() {
                        clicked = Some(i);
                    }
                }
            });
        }
        if let Some(i) = clicked {
            self.day = self.recording.as_ref().map_or(0, |r| r.moments[i].day);
        }
        clicked
    }
}

/// The layer over the map at the slider's day: trails of the last days, the armies, and the
/// moments fired by then with their cells marked. The caller draws it only in the AI tab.
pub fn draw(cam: &Cam, view: &AiView) {
    let (Some(rec), Some(frame)) = (view.recording.as_ref(), view.frame()) else { return };
    let at = |c: (i32, i32)| cam.to_screen(mq::vec2(c.0 as f32 + 0.5, c.1 as f32 + 0.5));
    // Trails: each army's cells over the last days, joined in day order.
    let from = frame.day.saturating_sub(TRAIL_DAYS);
    let days: Vec<&Frame> = rec.frames.iter().filter(|f| f.day >= from && f.day <= frame.day).collect();
    for a in &frame.armies {
        let colour = FACTION_COLOURS[a.faction as usize % FACTION_COLOURS.len()];
        let mut prev: Option<(i32, i32)> = None;
        for f in &days {
            if let Some(c) = f.armies.iter().find(|x| x.index == a.index).map(|x| x.cell) {
                if let Some(p) = prev {
                    if p != c {
                        let (s, e) = (at(p), at(c));
                        mq::draw_line(s.x, s.y, e.x, e.y, 2.0, mq::Color::new(colour.r, colour.g, colour.b, 0.55));
                    }
                }
                prev = Some(c);
            }
        }
    }
    // Armies at the day, numbered from 1 in the world's order.
    for a in &frame.armies {
        let colour = FACTION_COLOURS[a.faction as usize % FACTION_COLOURS.len()];
        let p = at(a.cell);
        mq::draw_circle(p.x, p.y, 6.0, colour);
        mq::draw_circle_lines(p.x, p.y, 6.5, 1.5, mq::BLACK);
        mq::draw_text((a.index + 1).to_string(), p.x + 7.0, p.y - 5.0, 16.0, mq::WHITE);
    }
    // The moments fired by the day, at their cells.
    for m in rec.moments.iter().filter(|m| m.day <= frame.day) {
        let (Some(c), Some(id)) = (m.cell, m.trigger.as_deref()) else { continue };
        let p = at(c);
        let r = 7.0;
        mq::draw_poly_lines(p.x, p.y, 4, r, 45.0, 2.0, mq::Color::new(1.0, 0.85, 0.2, 1.0));
        mq::draw_text(id, p.x + r + 3.0, p.y + 4.0, 14.0, mq::Color::new(1.0, 0.85, 0.2, 1.0));
    }
}

/// A story moment as the log words it (the studio's language).
pub fn moment_text(what: &Trace) -> String {
    match what {
        Trace::Fired { trigger, moment } => trf!("trigger {trigger} fired ({moment})", trigger = trigger.as_str(), moment = moment.as_str()),
        Trace::Var { name, value } => trf!("variable {name} set to {value}", name = name.as_str(), value = var_text(value)),
        Trace::Dialog { id, choices } if choices.is_empty() => trf!("dialog {id} opens (Далее)", id = id.as_str()),
        Trace::Dialog { id, choices } => trf!("dialog {id} opens: {choices}", id = id.as_str(), choices = choices.join(", ")),
        Trace::Answer { dialog, choice: Some(c) } => trf!("the AI answers {dialog}: {choice}", dialog = dialog.as_str(), choice = c.as_str()),
        Trace::Answer { dialog, choice: None } => trf!("the AI answers {dialog}: Далее", dialog = dialog.as_str()),
        Trace::Spawn { army } => trf!("army {army} comes onto the map", army = army.as_str()),
        Trace::Goal { army, goal } if goal == "clear" => trf!("army {army} goes back to the usual AI", army = army.as_str()),
        Trace::Goal { army, goal } => trf!("army {army} takes the goal {goal}", army = army.as_str(), goal = goal.as_str()),
        Trace::Arrive { army, cell: (x, y), building: Some(b) } => trf!("army {army} arrives in {building} at [{x}, {y}]", army = army.as_str(), building = b.as_str(), x = x, y = y),
        Trace::Arrive { army, cell: (x, y), building: None } => trf!("army {army} arrives at [{x}, {y}]", army = army.as_str(), x = x, y = y),
        Trace::End(Ending::Victory) => tr("the story ends: victory").to_string(),
        Trace::End(Ending::Defeat) => tr("the story ends: defeat").to_string(),
        Trace::Travel(map) => trf!("the story travels to {map}", map = map.as_str()),
        Trace::Ending(dialog) => trf!("the story ends the campaign: {dialog}", dialog = dialog.as_str()),
    }
}

fn var_text(v: &razdor::pack::triggers::VarValue) -> String {
    use razdor::pack::triggers::VarValue;
    match v {
        VarValue::Int(n) => n.to_string(),
        VarValue::Bool(b) => b.to_string(),
    }
}
