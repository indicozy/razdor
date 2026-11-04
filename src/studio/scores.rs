//! The studio's two scores (milestone 7): Richness (`richness.rs`, DT's playability formula on the
//! package) and Health (`health.rs`, the play-test and design checks). Both are computed off the
//! window's thread: [`Worker`] runs a job per rebuild, with the same newest-only rule as the build
//! thread (`worker.rs`), and keeps the health cache of the last maps it saw.
//!
//! Every breakdown item names where it is: its [`Target`]s are what the Scores tab selects (an
//! object, army, building, point, a cell on the canvas, or a line of a file).

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crate::dt::install::DtInstall;
use crate::pack::load::Package;

use super::health::{self, Cache, Health};
use super::richness::{self, Score};

/// Where a breakdown item or a finding points.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Target {
    /// The hero of the shown map.
    Hero,
    /// An army or a building of the shown map, by its id in `map.toml`.
    Army(String),
    Building(String),
    /// A point or a terrain object, by its index in the map file's list.
    Point(usize),
    Object(usize),
    /// A cell of the map.
    Cell(i64, i64),
    /// A line of a package file (a trigger, a dialog, the victory).
    Line { file: String, line: u32 },
}

/// One line of a breakdown: its term and note (catalogue keys, translated where shown), the name of
/// what it counts (an id or a label), the numbers it used, its points and its targets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub term: &'static str,
    pub note: &'static str,
    pub name: String,
    pub figures: String,
    pub points: i64,
    pub targets: Vec<Target>,
}

impl Item {
    pub fn new(term: &'static str, note: &'static str, points: i64, name: impl Into<String>, figures: impl Into<String>, targets: Vec<Target>) -> Item {
        Item { term, note, name: name.into(), figures: figures.into(), points, targets }
    }
}

/// Both scores of one map.
#[derive(Clone, Debug)]
pub struct Scores {
    /// The map they are of.
    pub map: String,
    /// Richness, or why the original would give none (a map narrower than 50 cells, say).
    pub richness: Result<Score, String>,
    pub health: Health,
}

/// One scoring job: the package as the window has it, the shown map, the language and the install.
pub struct Job {
    pub pkg: Arc<Package>,
    pub map: String,
    pub lang: String,
    pub install: Option<Arc<DtInstall>>,
}

/// Both scores of a job. Without an install neither can be computed (the compile needs the game's
/// files), and the reason is the error.
pub fn compute(job: &Job, cache: &mut Cache) -> Result<Scores, String> {
    let install = job.install.as_ref().ok_or_else(|| "The scores need the game's files: set RAZDOR_DT_DIR.".to_string())?;
    let richness = richness::richness(&job.map, &job.pkg, install, &job.lang).map(|(s, _)| s);
    let health = health::health(&job.map, &job.pkg, install, &job.lang, cache)?;
    Ok(Scores { map: job.map.clone(), richness, health })
}

/// The scores thread: jobs go in with a generation, results come out in order. When several are
/// queued only the newest runs; the window applies a result only when it is newer than the last one
/// applied.
pub struct Worker {
    jobs: Sender<(u64, Job)>,
    results: Receiver<(u64, Result<Scores, String>)>,
}

impl Worker {
    /// Starts the thread. It ends when the worker is dropped.
    pub fn start() -> Worker {
        let (jobs, job_rx) = channel::<(u64, Job)>();
        let (result_tx, results) = channel::<(u64, Result<Scores, String>)>();
        thread::Builder::new()
            .name("studio-scores".into())
            .spawn(move || {
                let mut cache = Cache::default();
                while let Ok(mut next) = job_rx.recv() {
                    while let Ok(newer) = job_rx.try_recv() {
                        next = newer;
                    }
                    let (gen, job) = next;
                    if result_tx.send((gen, compute(&job, &mut cache))).is_err() {
                        break;
                    }
                }
            })
            .expect("the studio's scores thread starts");
        Worker { jobs, results }
    }

    /// Queues a job of generation `gen`.
    pub fn send(&self, gen: u64, job: Job) {
        let _ = self.jobs.send((gen, job));
    }

    /// A finished job, if one is ready.
    pub fn try_recv(&self) -> Option<(u64, Result<Scores, String>)> {
        self.results.try_recv().ok()
    }

    /// Waits up to `timeout` for a finished job (the snapshot waits for its scores).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<(u64, Result<Scores, String>)> {
        self.results.recv_timeout(timeout).ok()
    }
}
