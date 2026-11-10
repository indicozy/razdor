//! The depth workers. A thread of its own loads the model, then starts the workers, which share
//! its one plan (tract runs one image on one thread, and a plan keeps no state between runs). The
//! main thread submits sprites while the workers have room, and drains their answers each frame.
//! Each worker cuts one sprite at a time and runs at the lowest priority the platform gives it, so
//! the game keeps its cores.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};

use super::queue::{Done, Job};
use super::{cards_for, DepthModel};

/// The workers' side of the work: jobs in, answers out.
pub struct Service {
    jobs: mpsc::Sender<Job>,
    done: mpsc::Receiver<Done>,
    /// The most jobs out at once: two per worker, so a worker that finishes has the next one ready.
    limit: usize,
    in_flight: usize,
}

impl Service {
    /// Starts loading the model at `model` (on a thread of its own, so the frame does not wait) and
    /// the workers after it. Results are cached under `cache` as well. `None` if no thread could be
    /// started.
    pub fn start(model: PathBuf, cache: Option<PathBuf>) -> Option<Service> {
        let (jobs, job_rx) = mpsc::channel::<Job>();
        let (done_tx, done) = mpsc::channel::<Done>();
        let threads = worker_count();
        let job_rx = Arc::new(Mutex::new(job_rx));
        std::thread::Builder::new()
            .name("depth-load".into())
            .spawn(move || {
                // Without a model, the loader ends here and drops the job queue: later submits fail.
                let Some(model) = DepthModel::load(&model) else { return };
                let model = Arc::new(model);
                for i in 0..threads {
                    let (model, cache, job_rx, done_tx) = (model.clone(), cache.clone(), job_rx.clone(), done_tx.clone());
                    let _ = std::thread::Builder::new().name(format!("depth-{i}")).spawn(move || worker(&model, cache.as_deref(), &job_rx, &done_tx));
                }
            })
            .ok()?;
        Some(Service { jobs, done, limit: threads * 2, in_flight: 0 })
    }

    /// How many more jobs may be handed in now.
    pub fn room(&self) -> usize {
        self.limit.saturating_sub(self.in_flight)
    }

    /// Hands a sprite to the workers. `false` when no worker is left to take it (the model did not
    /// load); the job is then dropped.
    pub fn submit(&mut self, job: Job) -> bool {
        if self.jobs.send(job).is_ok() {
            self.in_flight += 1;
            true
        } else {
            false
        }
    }

    /// The answers that came back since the last call.
    pub fn drain(&mut self) -> Vec<Done> {
        let mut out = Vec::new();
        while let Ok(done) = self.done.try_recv() {
            self.in_flight = self.in_flight.saturating_sub(1);
            out.push(done);
        }
        out
    }

    /// Jobs handed in and not yet answered.
    pub fn in_flight(&self) -> usize {
        self.in_flight
    }
}

/// The workers: half the desktop's cores (at least 1, at most 4); the phones run 2.
pub fn worker_count() -> usize {
    if cfg!(any(target_os = "android", target_os = "ios")) {
        2
    } else {
        std::thread::available_parallelism().map_or(1, |n| (n.get() / 2).clamp(1, 4))
    }
}

fn worker(model: &DepthModel, cache: Option<&Path>, jobs: &Mutex<mpsc::Receiver<Job>>, done: &mpsc::Sender<Done>) {
    lower_priority();
    loop {
        // The lock is held only while waiting for the next job (one worker at a time takes one).
        let job = match jobs.lock().unwrap_or_else(|e| e.into_inner()).recv() {
            Ok(job) => job,
            Err(_) => return,
        };
        let cards = cards_for(model, &job.img, cache);
        if done.send(Done { id: job.id, cards }).is_err() {
            return;
        }
    }
}

/// Lowers the calling thread's priority where the platform sets it per thread. Linux and Android:
/// the thread's nice value 19. Windows: the thread's priority lowest. macOS and iOS: the background
/// quality of service (not tried on a Mac: see `docs/OPEN_QUESTIONS.md`). A refusal keeps the default.
fn lower_priority() {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // SAFETY: gettid takes no arguments and cannot fail.
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::id_t;
        // SAFETY: on Linux PRIO_PROCESS with a thread's id sets that thread's nice value alone.
        unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, 19) };
    }
    #[cfg(windows)]
    {
        const THREAD_PRIORITY_LOWEST: i32 = -2;
        unsafe extern "system" {
            fn GetCurrentThread() -> isize;
            fn SetThreadPriority(thread: isize, priority: i32) -> i32;
        }
        // SAFETY: the calling thread's pseudo-handle; the call only changes this thread's priority.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_LOWEST) };
    }
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    {
        const QOS_CLASS_BACKGROUND: u32 = 0x09;
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos: u32, relative_priority: i32) -> i32;
        }
        // SAFETY: sets the calling thread's quality of service only.
        unsafe { pthread_set_qos_class_self_np(QOS_CLASS_BACKGROUND, 0) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::depth::cached_in;
    use crate::dt::cards::{builtin, sprite_key};
    use crate::dt::gfx::{decode_objects_ugs, Image, ObjectSprite, MAP_OBJECTS};
    use crate::dt::install::ENV_VAR as DT_ENV;

    #[test]
    fn the_workers_are_half_the_cores_at_most_four_and_at_least_one() {
        let n = worker_count();
        assert!((1..=4).contains(&n), "{n}");
    }

    /// The end-to-end run: three base-game building sprites, each with one pixel changed (so no
    /// shipped cut matches them), cut by the workers and cached; then the cache answers them with no
    /// model. Runs with `RAZDOR_DEPTH_MODEL` (the model file) and `RAZDOR_DT_DIR` (the install) set;
    /// prints the timings.
    #[test]
    fn workers_cut_unknown_sprites_and_the_cache_answers_them_after() {
        let (Some(model), Some(dt)) = (std::env::var_os(super::super::fetch::ENV_VAR), std::env::var_os(DT_ENV)) else {
            return;
        };
        let bytes = std::fs::read(Path::new(&dt).join(MAP_OBJECTS)).expect("Objects.ugs is readable");
        let sprites = decode_objects_ugs(&bytes).expect("Objects.ugs decodes");
        // Buildings whose shipped cut has two cards or more, so the model has something to find.
        let buildings: Vec<&ObjectSprite> = sprites
            .iter()
            .filter(|s| {
                s.section == ObjectSprite::BUILDINGS
                    && builtin().get(&sprite_key(&s.image)).is_some_and(|k| k.w == s.image.width && k.h == s.image.height && k.cards.len() >= 2)
            })
            .collect();
        assert!(buildings.len() >= 3, "the install has three buildings with two shipped cards");
        // Spread through the list, with one opaque pixel's red changed by one.
        let picks: Vec<(Image, (u32, u32, u32))> = [0, buildings.len() / 2, buildings.len() - 1]
            .iter()
            .map(|&i| {
                let s = buildings[i];
                let mut img = s.image.clone();
                let px = img.rgba.chunks_exact_mut(4).find(|p| p[3] == 255).expect("an opaque pixel");
                px[0] = px[0].wrapping_add(1);
                assert!(builtin().get(&sprite_key(&img)).is_none(), "the changed sprite has no shipped cut");
                (img, (s.section, s.cat, s.idx))
            })
            .collect();

        let dir = std::env::temp_dir().join(format!("razdor-depth-work-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let started = std::time::Instant::now();
        let mut service = Service::start(PathBuf::from(model), Some(dir.clone())).expect("a thread starts");
        for (img, id) in &picks {
            assert!(service.submit(Job { id: *id, img: img.clone() }), "the loader accepts jobs");
        }
        let mut answers = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
        while answers.len() < picks.len() {
            assert!(std::time::Instant::now() < deadline, "the workers answer in time");
            answers.extend(service.drain());
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        println!("{} sprites cut by {} workers in {:.2} s (model load included)", picks.len(), worker_count(), started.elapsed().as_secs_f64());
        assert_eq!(service.in_flight(), 0, "every job answered");
        assert!(answers.iter().any(|d| d.cards.len() >= 2), "the model finds two cards on a sprite the install cuts in two");
        for done in &answers {
            let (img, _) = picks.iter().find(|(_, id)| *id == done.id).expect("an answer for a job");
            let cached = cached_in(&dir, img).expect("the cut is cached, empty or not");
            assert_eq!(cached, done.cards, "the cache holds what the worker answered");
            println!("picture {:?}: {} card(s) at {}x{}", done.id, done.cards.len(), img.width, img.height);
        }
        // The second run: the cache answers, no model is loaded.
        for (img, _) in &picks {
            assert!(cached_in(&dir, img).is_some());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
