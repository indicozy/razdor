//! The Scores tab («Оценка»): Richness and Health, two big numbers, each with its breakdown. The
//! scores are computed on the scores thread after each rebuild (`studio::scores`), and the tab shows
//! the last ones of the shown map while a newer job runs («считается…»). A line of a breakdown
//! selects its targets: the Scene and the Inspector get an object, army or building; a cell is
//! highlighted on the canvas; a line of a file opens the Script there.

use std::collections::BTreeMap;
use std::time::Duration;

use egui::{Color32, RichText, ScrollArea, Sense, Ui};
use egui_macroquad::egui;
use razdor::i18n::tr;
use razdor::studio::health::{Health, Severity};
use razdor::studio::map::Selection;
use razdor::studio::scores::{Item, Job, Scores, Target, Worker};

use super::{Jump, Viewer, ERROR_RED, WARNING_AMBER};

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
    /// The Scores tab: the two numbers and their breakdowns.
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
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.weak(tr("Richness"));
                        match &scores.richness {
                            Ok(r) => {
                                ui.label(RichText::new(r.curve_value.to_string()).size(40.0).strong());
                                ui.weak(format!("{} {}", tr("points"), r.total));
                            }
                            Err(e) => {
                                ui.colored_label(WARNING_AMBER, e);
                            }
                        }
                    });
                    ui.add_space(24.0);
                    ui.vertical(|ui| {
                        ui.weak(tr("Health"));
                        ui.label(RichText::new(scores.health.summary.to_string()).size(40.0).strong().color(health_colour(scores.health.summary)));
                        ui.weak(tr("Razdor's own measure: 100 less the findings"));
                    });
                });
                ui.separator();
                // Both breakdowns side by side: the dock is short, and each list scrolls on its own.
                ui.columns(2, |cols| {
                    ScrollArea::vertical().id_salt("scores-richness-list").auto_shrink([false, false]).show(&mut cols[0], |ui| {
                        if let Ok(r) = &scores.richness {
                            ui.strong(tr("Richness breakdown"));
                            for (i, item) in r.items.iter().enumerate() {
                                item_row(ui, i, item, &mut clicked);
                            }
                        }
                    });
                    ScrollArea::vertical().id_salt("scores-health-list").auto_shrink([false, false]).show(&mut cols[1], |ui| {
                        health_breakdown(ui, &scores.health, &mut clicked);
                    });
                });
            }
        }
        if let Some(targets) = clicked {
            self.score_select(targets);
        }
    }

    /// Selects a breakdown line's targets: an object, army or building in the Scene and the
    /// Inspector, a cell highlighted on the canvas (and centred on the first), or a file's line.
    fn score_select(&mut self, targets: Vec<Target>) {
        self.canvas.score_cells.clear();
        for t in targets {
            match t {
                Target::Hero => self.select(Selection::Hero),
                Target::Army(id) => self.select(Selection::Army(id)),
                Target::Building(id) => self.select(Selection::Building(id)),
                Target::Point(i) => self.select(Selection::Point(i)),
                Target::Object(i) => self.select(Selection::Object(i)),
                Target::Cell(x, y) => {
                    if self.canvas.score_cells.is_empty() {
                        self.canvas.centre = Some((x as f32 + 0.5, y as f32 + 0.5));
                    }
                    self.canvas.score_cells.push((x, y));
                }
                Target::Line { file, line } => {
                    self.open = Some(file);
                    *self.jump = Some(Jump { line, range: None });
                }
            }
        }
    }
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
