//! The Тест workspace's Оценка view («Оценка», UX6): Richness and Health side by side, each at the full
//! height with its number and its breakdown. The scores are computed on the scores thread after each rebuild
//! (`studio::scores`), and the view shows the last ones of the shown map while a newer job runs («считается…»).
//! A line of a breakdown jumps to its targets (`studio::nav::of_target`): an object, army or building in Карта
//! (the Inspector shows it), a cell highlighted there, or a file's line in Данные › Файлы.

use std::collections::BTreeMap;
use std::time::Duration;

use egui::{Color32, RichText, ScrollArea, Sense, Ui};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::studio::health::{Health, Severity};
use razdor::studio::nav;
use razdor::studio::scores::{Item, Job, Scores, Target, Worker};

use super::{Viewer, ERROR_RED, WARNING_AMBER};

/// How long a snapshot waits for the scores (a map's fights take a few seconds).
const WAIT: Duration = Duration::from_secs(120);

const GOOD_GREEN: Color32 = Color32::from_rgb(0x6f, 0xc2, 0x7a);

/// The scores thread and its last results: the requests by generation (the map each one is of), and
/// the last result applied.
#[derive(Default)]
pub struct State {
    worker: Option<Worker>,
    asked: u64,
    applied: u64,
    maps: BTreeMap<u64, String>,
    result: Option<(String, Result<Scores, String>)>,
}

impl State {
    /// A state with its thread running (the window's; the tests and the MCP tools need none).
    pub fn start() -> State {
        State { worker: Some(Worker::start()), ..State::default() }
    }

    /// Asks for the scores of a job (the newest one wins on the thread).
    pub fn request(&mut self, job: Option<Job>) {
        let (Some(worker), Some(job)) = (&self.worker, job) else { return };
        self.asked += 1;
        self.maps.insert(self.asked, job.map.clone());
        worker.send(self.asked, job);
    }

    /// Takes the finished results that are ready.
    pub fn poll(&mut self) {
        while let Some((gen, result)) = self.worker.as_ref().and_then(Worker::try_recv) {
            self.apply(gen, result);
        }
    }

    /// Waits for the newest request (a snapshot draws the scores of the map it shows).
    pub fn wait(&mut self) {
        while self.applied < self.asked {
            let Some((gen, result)) = self.worker.as_ref().and_then(|w| w.recv_timeout(WAIT)) else { break };
            self.apply(gen, result);
        }
    }

    fn apply(&mut self, gen: u64, result: Result<Scores, String>) {
        if gen > self.applied {
            self.applied = gen;
            let map = self.maps.remove(&gen).unwrap_or_default();
            self.maps.retain(|g, _| *g > gen);
            self.result = Some((map, result));
        }
    }

    /// Whether a job is still running (its result is not applied yet).
    pub fn pending(&self) -> bool {
        self.applied < self.asked
    }

    /// The last result of `map`, if there is one (it may be older than the job running now).
    pub fn of(&self, map: &str) -> Option<&Result<Scores, String>> {
        self.result.as_ref().filter(|(m, _)| m == map).map(|(_, r)| r)
    }
}

/// The big number's colour: green from 80, amber from 50, red below.
fn health_colour(summary: i64) -> Color32 {
    if summary >= 80 {
        GOOD_GREEN
    } else if summary >= 50 {
        WARNING_AMBER
    } else {
        ERROR_RED
    }
}

/// One breakdown row: the points, the term and its note, the name, the figures. Returns whether it
/// was clicked (a row with targets selects them).
fn row(ui: &mut Ui, id: egui::Id, points: Option<i64>, head: &str, note: &str, name: &str, figures: &str) -> bool {
    let resp = ui.horizontal_wrapped(|ui| {
        if let Some(p) = points {
            let colour = match p.signum() {
                1 => GOOD_GREEN,
                -1 => ERROR_RED,
                _ => ui.visuals().weak_text_color(),
            };
            ui.colored_label(colour, RichText::new(format!("{p:+}")).monospace());
        }
        ui.strong(head);
        if !note.is_empty() {
            ui.label(note);
        }
        if !name.is_empty() {
            ui.monospace(name);
        }
        if !figures.is_empty() {
            ui.weak(figures);
        }
    });
    ui.add_space(1.0);
    ui.interact(resp.response.rect, id, Sense::click()).clicked()
}

impl Viewer<'_> {
    /// The Оценка view: the two scores side by side, each at the full height.
    pub(super) fn scores_dock(&mut self, ui: &mut Ui) {
        let Some(map) = self.maps.map.clone() else {
            ui.weak(tr("No map to show."));
            return;
        };
        if self.install.is_none() {
            ui.weak(tr("The scores need the game's files: set RAZDOR_DT_DIR."));
            return;
        }
        let pending = self.scores.pending();
        let shown = self.scores.of(&map);
        let mut clicked: Option<Vec<Target>> = None;
        if pending {
            ui.weak(tr("Counting…"));
        }
        match shown {
            None => {
                if !pending {
                    ui.weak(tr("No scores yet: the package is being read."));
                }
            }
            Some(Err(e)) => {
                ui.colored_label(ERROR_RED, e);
            }
            Some(Ok(scores)) => {
                // The snapshot's `jump=score` clicks the first target of the Richness breakdown, once.
                if std::mem::take(&mut *self.snap_jump) {
                    clicked = scores.richness.as_ref().ok().and_then(|r| r.items.iter().find(|i| !i.targets.is_empty())).map(|i| i.targets.clone());
                }
                ui.columns(2, |cols| {
                    richness_column(&mut cols[0], scores, &mut clicked);
                    health_column(&mut cols[1], scores, &mut clicked);
                });
            }
        }
        if let Some(targets) = clicked {
            self.score_select(targets);
        }
    }

    /// A breakdown line's targets: the cells are outlined on the canvas (the first is centred on), and the first
    /// other target is where the jump goes (`studio::nav::of_target`): its object in Карта, or a file's line.
    fn score_select(&mut self, targets: Vec<Target>) {
        let map = self.maps.map.clone().unwrap_or_default();
        let mut cells = Vec::new();
        let mut first: Option<nav::Destination> = None;
        for t in &targets {
            match t {
                Target::Cell(x, y) => cells.push((*x, *y)),
                other => {
                    first.get_or_insert_with(|| nav::of_target(&map, other));
                }
            }
        }
        if let (Some(&(x, y)), None) = (cells.first(), &first) {
            first = Some(nav::Destination::Map { map: map.clone(), object: None, cell: Some((x, y)) });
        }
        if let Some(&(x, y)) = cells.first() {
            self.canvas.centre = Some((x as f32 + 0.5, y as f32 + 0.5));
        }
        self.canvas.score_cells = cells;
        self.goto = first;
    }
}

/// The Richness column: its number and its points, then the breakdown (each item's row jumps to its targets).
fn richness_column(ui: &mut Ui, scores: &Scores, clicked: &mut Option<Vec<Target>>) {
    ui.weak(tr("Richness"));
    match &scores.richness {
        Ok(r) => {
            ui.label(RichText::new(r.curve_value.to_string()).size(40.0).strong());
            ui.weak(format!("{} {}", tr("points"), r.total));
            ui.separator();
            ScrollArea::vertical().id_salt("scores-richness-list").auto_shrink([false, false]).show(ui, |ui| {
                ui.strong(tr("Richness breakdown"));
                for (i, item) in r.items.iter().enumerate() {
                    item_row(ui, i, item, clicked);
                }
            });
        }
        Err(e) => {
            ui.colored_label(WARNING_AMBER, e);
        }
    }
}

/// The Health column: its number and what it measures, then the findings (each line jumps to its targets).
fn health_column(ui: &mut Ui, scores: &Scores, clicked: &mut Option<Vec<Target>>) {
    ui.weak(tr("Health"));
    ui.label(RichText::new(scores.health.summary.to_string()).size(40.0).strong().color(health_colour(scores.health.summary)));
    ui.weak(tr("Razdor's own measure: 100 less the findings"));
    ui.separator();
    ScrollArea::vertical().id_salt("scores-health-list").auto_shrink([false, false]).show(ui, |ui| {
        health_breakdown(ui, &scores.health, clicked);
    });
}

/// One richness item as a row; clicking it selects its targets.
fn item_row(ui: &mut Ui, i: usize, item: &Item, clicked: &mut Option<Vec<Target>>) {
    let id = ui.id().with(("score-item", i));
    let name = if item.name.is_empty() { String::new() } else { format!(" {}", item.name) };
    if row(ui, id, Some(item.points), tr(item.term), &format!(" · {}", tr(item.note)), &name, &item.figures) && !item.targets.is_empty() {
        *clicked = Some(item.targets.clone());
    }
}

/// The health findings, grouped by check: each line selects its targets.
fn health_breakdown(ui: &mut Ui, health: &Health, clicked: &mut Option<Vec<Target>>) {
    let warnings = health.findings.iter().filter(|f| f.severity == Severity::Warning).count();
    let notes = health.findings.len() - warnings;
    ui.strong(format!("{} ({warnings} · {notes})", tr("Health findings")));
    if health.findings.is_empty() {
        ui.weak(tr("No findings."));
    }
    for (i, f) in health.findings.iter().enumerate() {
        let (colour, word) = match f.severity {
            Severity::Warning => (WARNING_AMBER, tr("Warning")),
            Severity::Info => (ui.visuals().weak_text_color(), tr("Note")),
        };
        let clickable = !f.targets.is_empty();
        let id = ui.id().with(("score-finding", i));
        let name = if f.name.is_empty() { String::new() } else { format!(" {}", f.name) };
        let resp = ui.horizontal_wrapped(|ui| {
            ui.colored_label(colour, word);
            ui.label(tr(f.note));
            if !name.is_empty() {
                ui.monospace(name.trim_start());
            }
            if !f.figures.is_empty() {
                ui.weak(&f.figures);
            }
        });
        ui.add_space(1.0);
        if clickable && ui.interact(resp.response.rect, id, Sense::click()).clicked() {
            *clicked = Some(f.targets.clone());
        }
    }
}
