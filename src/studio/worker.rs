//! The studio's build thread: runs [`super::build::run`] off the window's thread. Jobs carry a
//! generation number; when several are queued only the newest runs, and the window applies a
//! result only when it is newer than the last one applied (a stale result is dropped).

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread;
use std::time::Duration;

use super::build::{self, Built, Job};

/// A finished rebuild and the generation it was asked with.
pub type Done = (u64, Result<Built, String>);

/// The worker: jobs go in, results come out, in generation order.
pub struct Worker {
    jobs: Sender<(u64, Job)>,
    results: Receiver<Done>,
}

impl Worker {
    /// Starts the thread. It ends when the worker is dropped.
    pub fn start() -> Worker {
        let (jobs, job_rx) = channel::<(u64, Job)>();
        let (result_tx, results) = channel::<Done>();
        thread::Builder::new()
            .name("studio-build".into())
            .spawn(move || {
                while let Ok(mut next) = job_rx.recv() {
                    while let Ok(newer) = job_rx.try_recv() {
                        next = newer;
                    }
                    let (gen, job) = next;
                    if result_tx.send((gen, build::run(job))).is_err() {
                        break;
                    }
                }
            })
            .expect("the studio's build thread starts");
        Worker { jobs, results }
    }

    /// Queues a job of generation `gen`.
    pub fn send(&self, gen: u64, job: Job) {
        // A worker whose thread ended has nothing to run the job: the result never comes, and the
        // window keeps its last good state.
        let _ = self.jobs.send((gen, job));
    }

    /// A finished job, if one is ready.
    pub fn try_recv(&self) -> Option<Done> {
        self.results.try_recv().ok()
    }

    /// Waits up to `timeout` for a finished job (the snapshot waits for its build).
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Done> {
        self.results.recv_timeout(timeout).ok()
    }
}
