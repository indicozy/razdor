//! The studio's battle tester («Тест боя»), a main screen: two sides (a map's army, a garrison, the
//! hero's squad, or a custom list of unit types), N battles over seeds 1..N by the game's battle
//! engine on a worker thread (with progress and Cancel), the results with each side's losses, the
//! per-seed table, and «Посмотреть бой»: one seed's battle in the game's own battle view, both sides
//! by the AI (the view's watch mode). The core is `razdor::studio::testers`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

use egui::{Color32, ScrollArea, Ui};
use egui_macroquad::egui;
use razdor::dt::install::DtInstall;
use razdor::i18n::tr;
use razdor::pack::load::Package;
use razdor::rules::battle::Team;
use razdor::rules::custom::{self, Control, Pick, MAX_LEVEL};
use razdor::studio::testers::{self, Choice, Kind, MapGame, Matchup, Run, Source, Summary};
use razdor::trf;

use crate::ui::battle_view::BattleView;

/// Battles run when the box is not changed.
pub const DEFAULT_BATTLES: u64 = 100;
const MAX_BATTLES: u64 = 2000;

/// One side's choice in the editor: the dropdown's index (the map's sources, then Custom) and the
/// custom list being built.
#[derive(Clone, Debug, Default)]
pub struct SideEdit {
    pub choice: usize,
    pub picks: Vec<Pick>,
    /// The unit type the add row takes, its level and how many to add.
    pub add_unit: usize,
    pub add_level: i32,
    pub add_count: u32,
}

impl SideEdit {
    fn new(choice: usize) -> SideEdit {
        SideEdit { choice, picks: Vec::new(), add_unit: 0, add_level: 1, add_count: 1 }
    }
}

enum Msg {
    Progress(usize, usize),
    Done(Result<Option<Vec<Run>>, String>),
}

struct Job {
    rx: Receiver<Msg>,
    cancel: Arc<AtomicBool>,
}

/// The tester's state. The options and unit names come from the map as last opened.
pub struct BattleTest {
    pub a: SideEdit,
    pub b: SideEdit,
    pub battles: u64,
    pub improved: bool,
    /// The map the options were made for, and the sides' sources the map offers (the last one is
    /// Custom).
    pub map: Option<String>,
    pub options: Vec<Choice>,
    /// The unit types a custom side can take: the id and the name.
    pub units: Vec<(razdor::rules::content::UnitId, String)>,
    job: Option<Job>,
    progress: (usize, usize),
    /// The last finished sweep: its runs and their summary.
    pub results: Option<(Vec<Run>, Summary)>,
    pub error: Option<String>,
    pub watch_seed: u64,
}

impl Default for BattleTest {
    fn default() -> BattleTest {
        BattleTest {
            a: SideEdit::new(0),
            b: SideEdit::new(1),
            battles: DEFAULT_BATTLES,
            improved: false,
            map: None,
            options: Vec::new(),
            units: Vec::new(),
            job: None,
            progress: (0, 0),
            results: None,
            error: None,
            watch_seed: 1,
        }
    }
}

impl BattleTest {
    pub fn running(&self) -> bool {
        self.job.is_some()
    }

    /// The map's sources and unit types (the options of both sides), opened from `map`.
    pub fn load(&mut self, mg: &MapGame, map: &str) {
        let mut options = testers::map_sources(&mg.game);
        options.push(Choice { kind: Kind::Custom, name: String::new(), troops: 0, source: Source::Custom(Vec::new()) });
        self.options = options;
        self.units = custom::choices(&mg.content).into_iter().map(|u| (u, mg.content.unit(u).name.clone())).collect();
        self.map = Some(map.to_string());
        self.results = None;
        self.error = None;
        let last = self.options.len().saturating_sub(1);
        self.a.choice = self.a.choice.min(last);
        self.b.choice = self.b.choice.min(last);
    }

    /// The source a side is set to now.
    fn source(&self, side: &SideEdit) -> Source {
        match self.options.get(side.choice).map(|o| &o.source) {
            Some(Source::Custom(_)) | None => Source::Custom(side.picks.clone()),
            Some(s) => s.clone(),
        }
    }

    /// The matchup as the two sides are set now.
    pub fn matchup(&self) -> Matchup {
        Matchup { a: self.source(&self.a), b: self.source(&self.b) }
    }

    /// Starts the sweep of the current matchup on a worker thread.
    pub fn start(&mut self, pkg: Arc<Package>, install: Arc<DtInstall>, map: &str) {
        self.cancel();
        self.results = None;
        self.error = None;
        self.progress = (0, 0);
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let (map, matchup, battles, improved) = (map.to_string(), self.matchup(), self.battles, self.improved);
        std::thread::spawn(move || {
            let outcome = (|| -> Result<Option<Vec<Run>>, String> {
                let mg = testers::open_map(&pkg, &install, &map).map_err(|r| r.to_text())?;
                let progress = tx.clone();
                testers::run_seeds(&mg.content, Some(&mg.game), &matchup, improved, 1..=battles, &stop, &mut |done, total| {
                    let _ = progress.send(Msg::Progress(done, total));
                })
            })();
            let _ = tx.send(Msg::Done(outcome));
        });
        self.job = Some(Job { rx, cancel });
    }

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
                Ok(Msg::Progress(done, total)) => self.progress = (done, total),
                Ok(Msg::Done(Ok(Some(runs)))) => {
                    self.job = None;
                    let summary = Summary::of(&runs);
                    self.results = Some((runs, summary));
                    return;
                }
                Ok(Msg::Done(Ok(None))) => {
                    self.job = None;
                    return;
                }
                Ok(Msg::Done(Err(text))) => {
                    self.job = None;
                    self.error = Some(text.lines().next().unwrap_or_default().to_string());
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

    /// Runs one battle of the current matchup in the calling thread (the snapshot's results).
    pub fn run_now(&mut self, mg: &MapGame, battles: u64) -> Result<(), String> {
        let matchup = self.matchup();
        let runs = testers::run_seeds(&mg.content, Some(&mg.game), &matchup, self.improved, 1..=battles, &AtomicBool::new(false), &mut |_, _| {})?.ok_or("cancelled")?;
        let summary = Summary::of(&runs);
        self.results = Some((runs, summary));
        Ok(())
    }

    /// The battle of `seed` for the game's battle view, both sides by the AI.
    /// The battle view of the current matchup for the game's watch mode (both sides by the AI).
    /// One battle is the same at every seed (see `razdor::studio::testers`), so no seed is given.
    pub fn watch(&self, pkg: &Package, install: &DtInstall, map: &str) -> Result<Box<BattleView>, String> {
        let mg = testers::open_map(pkg, install, map).map_err(|r| r.to_text())?;
        let battle = testers::watch_battle(&mg.content, Some(&mg.game), &self.matchup())?;
        Ok(Box::new(BattleView::custom(battle, [Control::Ai, Control::Ai])))
    }

    /// The screen: the two sides, the box and the start, the results and the watch.
    pub fn show(&mut self, ui: &mut Ui, ready: Result<(), &'static str>) -> Option<Req> {
        let mut req = None;
        if let Err(why) = ready {
            ui.weak(why);
            return None;
        }
        ui.heading(tr("Battle tester"));
        ui.weak(tr("Two armies fight by the game's battle engine, both sides by the AI, over seeds 1..N."));
        ui.add_space(4.0);
        ui.columns(2, |cols| {
            req = side_editor(&mut cols[0], "A", &mut self.a, &self.options, &self.units).or(req);
            req = side_editor(&mut cols[1], "B", &mut self.b, &self.options, &self.units).or(req);
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(tr("Battles"));
            ui.add(egui::DragValue::new(&mut self.battles).range(1..=MAX_BATTLES));
            ui.checkbox(&mut self.improved, tr("Improved AI"));
            if self.running() {
                if ui.button(tr("Cancel")).clicked() {
                    req = Some(Req::Cancel);
                }
            } else if ui.button(tr("Run battles")).clicked() {
                req = Some(Req::Start);
            }
        });
        if self.running() {
            let (done, total) = self.progress;
            let fraction = if total == 0 { 0.0 } else { done as f32 / total as f32 };
            ui.add(egui::ProgressBar::new(fraction).text(trf!("{done} of {total} battles", done = done, total = total)));
        }
        if let Some(error) = &self.error {
            ui.colored_label(Color32::from_rgb(0xe0, 0x5a, 0x5a), error.as_str());
        }
        if let Some((runs, summary)) = &self.results {
            results_view(ui, summary);
            ui.horizontal(|ui| {
                ui.label(tr("Watch seed"));
                ui.add(egui::DragValue::new(&mut self.watch_seed).range(1..=MAX_BATTLES));
                if ui.button(tr("Watch the battle")).clicked() {
                    req = Some(Req::Watch(self.watch_seed));
                }
            });
            ScrollArea::vertical().id_salt("battle-seeds").max_height(220.0).auto_shrink([false, true]).show(ui, |ui| {
                egui::Grid::new("battle-seeds-grid").striped(true).num_columns(7).show(ui, |ui| {
                    for h in [tr("Seed"), tr("Winner"), tr("Rounds"), "A", tr("A lost HP"), "B", tr("B lost HP")] {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for r in runs {
                        if ui.selectable_label(r.seed == self.watch_seed, r.seed.to_string()).clicked() {
                            self.watch_seed = r.seed;
                        }
                        ui.label(match r.winner {
                            Some(Team::Player) => "A",
                            Some(Team::Enemy) => "B",
                            None => "-",
                        });
                        ui.label(r.rounds.to_string());
                        ui.label(r.a.units.to_string());
                        ui.label(r.a.hp.to_string());
                        ui.label(r.b.units.to_string());
                        ui.label(r.b.hp.to_string());
                        ui.end_row();
                    }
                });
            });
        }
        req
    }
}

/// What the screen asks of the studio's worker (the frame runs it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Req {
    Start,
    Cancel,
    Watch(u64),
}

/// One side's editor: the source dropdown, and for a custom side its list and the add row.
fn side_editor(ui: &mut Ui, name: &str, side: &mut SideEdit, options: &[Choice], units: &[(razdor::rules::content::UnitId, String)]) -> Option<Req> {
    ui.heading(trf!("Side {name}", name = name));
    let shown = options.get(side.choice).map_or(String::new(), choice_label);
    egui::ComboBox::from_id_salt(("battle-side", name)).selected_text(shown).show_ui(ui, |ui| {
        for (i, c) in options.iter().enumerate() {
            ui.selectable_value(&mut side.choice, i, choice_label(c));
        }
    });
    if matches!(options.get(side.choice).map(|o| &o.kind), Some(Kind::Custom) | None) {
        ui.add_space(2.0);
        let mut remove = None;
        for (i, p) in side.picks.iter().enumerate() {
            let unit = units.iter().find(|(u, _)| *u == p.unit).map_or("?", |(_, n)| n.as_str());
            ui.horizontal(|ui| {
                ui.label(format!("{unit} · {} {}", tr("level"), p.level));
                if ui.small_button(tr("Remove")).clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            side.picks.remove(i);
        }
        if side.picks.len() >= 12 {
            ui.weak(tr("An army holds up to 12 units."));
        }
        ui.horizontal(|ui| {
            let current = units.get(side.add_unit).map_or("", |(_, n)| n.as_str()).to_string();
            egui::ComboBox::from_id_salt(("battle-unit", name)).selected_text(current).show_ui(ui, |ui| {
                for (i, (_, label)) in units.iter().enumerate() {
                    ui.selectable_value(&mut side.add_unit, i, label.as_str());
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(tr("level"));
            ui.add(egui::DragValue::new(&mut side.add_level).range(1..=MAX_LEVEL));
            ui.label(tr("count"));
            ui.add(egui::DragValue::new(&mut side.add_count).range(1..=12));
            if ui.add_enabled(!units.is_empty() && side.picks.len() < 12, egui::Button::new(tr("Add"))).clicked() {
                if let Some((unit, _)) = units.get(side.add_unit) {
                    for _ in 0..side.add_count.clamp(1, 12) {
                        if side.picks.len() >= 12 {
                            break;
                        }
                        side.picks.push(Pick { unit: *unit, level: side.add_level.clamp(1, MAX_LEVEL), items: Vec::new() });
                    }
                }
            }
        });
    }
    None
}

/// A choice as the dropdown shows it.
fn choice_label(c: &Choice) -> String {
    match c.kind {
        Kind::Army => trf!("Army {name} ({troops} troops)", name = if c.name.is_empty() { "?" } else { c.name.as_str() }, troops = c.troops),
        Kind::Garrison => trf!("Garrison {name} ({troops} troops)", name = if c.name.is_empty() { "?" } else { c.name.as_str() }, troops = c.troops),
        Kind::Squad => tr("Hero's squad").to_string(),
        Kind::Custom => tr("Custom army").to_string(),
    }
}

/// The totals and the spread of each side's losses.
fn results_view(ui: &mut Ui, s: &Summary) {
    ui.add_space(4.0);
    let line = trf!("A wins {a} % · B wins {b} % · draws {d} % · {runs} battles · average {rounds} rounds", a = format!("{:.1}", s.a_percent()), b = format!("{:.1}", s.b_percent()), d = format!("{:.1}", s.draw_percent()), runs = s.runs, rounds = format!("{:.1}", s.avg_rounds));
    ui.label(line);
    if s.same_every_seed && s.runs > 1 {
        ui.weak(tr("The battle draws nothing: every seed fought the same battle."));
    }
    egui::Grid::new("battle-losses").striped(true).num_columns(5).show(ui, |ui| {
        for h in [tr("Side"), tr("Units lost, average"), tr("Units lost, range"), tr("HP lost, average"), tr("HP lost, range")] {
            ui.strong(h);
        }
        ui.end_row();
        for (name, sp) in [("A", &s.a), ("B", &s.b)] {
            ui.label(name);
            ui.label(format!("{:.1}", sp.avg_units));
            ui.label(format!("{}–{}", sp.min_units, sp.max_units));
            ui.label(format!("{:.1}", sp.avg_hp));
            ui.label(format!("{}–{}", sp.min_hp, sp.max_hp));
            ui.end_row();
        }
    });
}

