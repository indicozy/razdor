//! The decode store: each install picture is decoded once and shared by every caller, and
//! pictures a screen is about to need are decoded ahead by two background workers
//! ([`preload`]). A caller that asks for a picture a worker is decoding waits for that worker
//! rather than decoding it again; one not yet started is taken off the queue and decoded here.
//! Decoded pictures come from the persistent cache ([`super::picture_cache`]) when it has them,
//! and are written there in the background otherwise.
//!
//! Only plain RGBA leaves this module. Textures are made on the main thread (macroquad), so
//! callers upload what is ready there (see `chrome::warm`).

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};

use super::gfx::{self, Image};
use super::picture_cache;
use super::DtError;

/// Background decoders. Two is enough to keep up with the windows' art while the game runs.
const WORKERS: usize = 2;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Queued,
    Running,
}

/// A cache entry to write once a picture is decoded: its folder and key.
type Miss = Option<(PathBuf, u64)>;

#[derive(Default)]
struct Store {
    ready: HashMap<PathBuf, Arc<Vec<Image>>>,
    pending: HashMap<PathBuf, State>,
    queue: VecDeque<PathBuf>,
    workers: usize,
    /// Bumped by [`forget_ready`]: a picture decoded before it was forgotten is not stored as ready.
    generation: u64,
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(Default::default);
/// Signalled when a picture becomes ready or a worker has work.
static CHANGED: Condvar = Condvar::new();

fn lock() -> MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether `path` is decoded already (so [`decode_shared`] will not decode it).
pub fn is_ready(path: &Path) -> bool {
    lock().ready.contains_key(path)
}

/// Drops every decoded picture from the store: the install's files were replaced (a game import,
/// `dt::game_import`), and the store is keyed by path alone. Pictures still being decoded are
/// stored only if they were started after this call (the generation). The persistent cache keys
/// on the files' modification times, so it misses for the new files on its own.
pub fn forget_ready() {
    let mut s = lock();
    s.ready.clear();
    s.generation += 1;
}

/// The frames of the picture at `path`, decoded once and shared. Waits for a worker that is
/// decoding it; decodes it here otherwise. Logged as a slow load when it took over 2 ms
/// (`RAZDOR_PROFILE`), waiting included.
pub fn decode_shared(path: &Path) -> Result<Arc<Vec<Image>>, DtError> {
    crate::diag::timed(|| path.display().to_string(), || {
        let mut s = lock();
        loop {
            if let Some(a) = s.ready.get(path) {
                return Ok(a.clone());
            }
            match s.pending.get(path).copied() {
                Some(State::Running) => s = CHANGED.wait(s).unwrap_or_else(|e| e.into_inner()),
                Some(State::Queued) => {
                    s.pending.remove(path);
                    s.queue.retain(|p| p != path);
                    break;
                }
                None => break,
            }
        }
        s.pending.insert(path.to_path_buf(), State::Running);
        let generation = s.generation;
        drop(s);
        let loaded = load(path);
        finish(path, loaded, generation)
    })
}

/// Queues `paths` for the background workers, in order, skipping those already decoded or
/// queued. A no-op in snapshot runs (decoding stays on demand there, so the frames are the same
/// in every run).
pub fn preload(paths: Vec<PathBuf>) {
    if std::env::var_os("RAZDOR_SNAPSHOT").is_some() {
        return;
    }
    let spawn = {
        let mut s = lock();
        for p in paths {
            if s.ready.contains_key(&p) || s.pending.contains_key(&p) {
                continue;
            }
            s.pending.insert(p.clone(), State::Queued);
            s.queue.push_back(p);
        }
        let missing = WORKERS.saturating_sub(s.workers);
        s.workers += missing;
        missing
    };
    for _ in 0..spawn {
        let _ = std::thread::Builder::new().name("art ahead".into()).spawn(worker);
    }
    CHANGED.notify_all();
}

/// Hands `upload` the ready items of `queue`, in order, while `elapsed_ms` is under `budget_ms`
/// (checked before each upload, so a frame starts at least one upload when there is one). Items
/// not ready yet stay in the queue for a later frame, in their order. Returns the uploads made.
pub fn upload_within_budget<T>(
    queue: &mut VecDeque<T>,
    mut ready: impl FnMut(&T) -> bool,
    mut upload: impl FnMut(&T),
    mut elapsed_ms: impl FnMut() -> f64,
    budget_ms: f64,
) -> usize {
    let mut kept = VecDeque::with_capacity(queue.len());
    let mut done = 0;
    while let Some(item) = queue.pop_front() {
        if !ready(&item) {
            kept.push_back(item);
            continue;
        }
        if elapsed_ms() >= budget_ms {
            kept.push_back(item);
            break;
        }
        upload(&item);
        done += 1;
    }
    kept.append(queue);
    *queue = kept;
    done
}

fn worker() {
    loop {
        let (path, generation) = {
            let mut s = lock();
            loop {
                if let Some(p) = s.queue.pop_front() {
                    s.pending.insert(p.clone(), State::Running);
                    break (p, s.generation);
                }
                s = CHANGED.wait(s).unwrap_or_else(|e| e.into_inner());
            }
        };
        let loaded = load(&path);
        let _ = finish(&path, loaded, generation);
    }
}

/// The frames of `path`: from the persistent cache if it has them, else decoded (and due to be
/// written to the cache).
fn load(path: &Path) -> Result<(Vec<Image>, Miss), DtError> {
    let cache = picture_cache::cache_dir().zip(picture_cache::source_key(path));
    if let Some((dir, key)) = &cache {
        if let Some(frames) = picture_cache::read(dir, *key) {
            return Ok((frames, None));
        }
    }
    let frames = gfx::decode_uncached(path)?;
    Ok((frames, cache))
}

/// Stores a loaded picture as ready (unless [`forget_ready`] ran since `generation` was taken),
/// wakes the waiters and writes a decoded one to the cache.
fn finish(path: &Path, loaded: Result<(Vec<Image>, Miss), DtError>, generation: u64) -> Result<Arc<Vec<Image>>, DtError> {
    let mut s = lock();
    s.pending.remove(path);
    let current = s.generation == generation;
    let out = loaded.map(|(frames, miss)| {
        let a = Arc::new(frames);
        if current {
            s.ready.insert(path.to_path_buf(), a.clone());
        }
        if let Some((dir, key)) = miss {
            let frames = a.clone();
            let _ = std::thread::Builder::new().name("art cache".into()).spawn(move || picture_cache::write(&dir, key, &frames));
        }
        a
    });
    CHANGED.notify_all();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgetting_drops_the_store_and_a_decode_started_before_is_not_kept() {
        let path = PathBuf::from("/razdor-test/forget-ready.lit");
        lock().ready.insert(path.clone(), Arc::new(Vec::new()));
        assert!(is_ready(&path));
        forget_ready();
        assert!(!is_ready(&path), "the old install's pictures are dropped");
        // A decode that started before the forget (a swap in between) is handed back, not stored.
        let before = lock().generation;
        forget_ready();
        let _ = finish(&path, Ok((Vec::new(), None)), before);
        assert!(!is_ready(&path));
        // One started after it is stored as usual.
        let after = lock().generation;
        let _ = finish(&path, Ok((Vec::new(), None)), after);
        assert!(is_ready(&path));
        forget_ready();
    }

    #[test]
    fn uploads_stop_at_the_budget_and_keep_the_order() {
        // Each check costs 1.5 ms: with a 4 ms budget a frame makes three uploads.
        let mut clock = 0.0;
        let mut queue: VecDeque<u32> = (0..10).collect();
        let mut made = Vec::new();
        let n = upload_within_budget(
            &mut queue,
            |_| true,
            |x| made.push(*x),
            || {
                clock += 1.5;
                clock - 1.5
            },
            4.0,
        );
        assert_eq!(n, 3);
        assert_eq!(made, vec![0, 1, 2]);
        assert_eq!(queue.into_iter().collect::<Vec<_>>(), (3..10).collect::<Vec<_>>());
    }

    #[test]
    fn items_not_ready_wait_and_keep_their_place() {
        let mut queue: VecDeque<u32> = (0..6).collect();
        let mut made = Vec::new();
        let ready = |x: &u32| x % 2 == 1;
        let n = upload_within_budget(&mut queue, ready, |x| made.push(*x), || 0.0, 4.0);
        assert_eq!(n, 3);
        assert_eq!(made, vec![1, 3, 5]);
        assert_eq!(queue.into_iter().collect::<Vec<_>>(), vec![0, 2, 4]);
    }

    #[test]
    fn nothing_is_uploaded_past_a_spent_budget() {
        let mut queue: VecDeque<u32> = (0..3).collect();
        let n = upload_within_budget(&mut queue, |_| true, |_| panic!("no upload"), || 9.0, 4.0);
        assert_eq!(n, 0);
        assert_eq!(queue.len(), 3);
    }

    /// The pictures of the player's install, the windows' first and a terrain texture.
    fn install_pictures() -> Option<(PathBuf, Vec<PathBuf>)> {
        let dir = PathBuf::from(std::env::var_os(crate::dt::install::ENV_VAR)?);
        let mut rels = vec!["Graphics/Windows/Win-paper.lit".to_string(), "Graphics/Windows/Castle.lit".to_string()];
        rels.push(format!("{}/{}", gfx::TEXTURES_DIR, gfx::TERRAIN_TEXTURES[0]));
        let paths = rels.iter().filter_map(|rel| crate::dt::install::find_path(&dir, rel).ok()).collect();
        Some((dir, paths))
    }

    /// Stored pictures, the background decode included, are the on-demand decode, byte for byte,
    /// and the disk cache returns the same pixels. Skipped without `RAZDOR_DT_DIR`.
    #[test]
    fn stored_pictures_are_the_on_demand_decode() {
        let Some((_, paths)) = install_pictures() else { return };
        assert!(!paths.is_empty());
        preload(paths.clone());
        for path in &paths {
            let on_demand = gfx::decode_uncached(path).expect("decodes");
            let stored = decode_shared(path).expect("decodes through the store");
            assert!(*stored == on_demand, "{} differs from the on-demand decode", path.display());
            let key = picture_cache::source_key(path).expect("the install file is there");
            let dir = std::env::temp_dir().join(format!("razdor-pc-real-{}", std::process::id()));
            picture_cache::write(&dir, key, &on_demand);
            let back = picture_cache::read(&dir, key).expect("the entry reads back");
            assert!(back == on_demand, "{} differs after the cache", path.display());
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Decode time against the cache read for the big pictures of the player's log (the
    /// numbers are in the flow doc). Ignored: run with `--ignored --nocapture`.
    #[test]
    #[ignore]
    fn decode_against_cache_read() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR).map(PathBuf::from) else { return };
        let dir_tmp = std::env::temp_dir().join(format!("razdor-pc-time-{}", std::process::id()));
        for rel in ["Graphics/Windows/Win-paper.lit", "Graphics/Windows/Castle.lit", "Graphics/Textures/Ice.lit", "Graphics/Objects/Persones.ugs"] {
            let Ok(path) = crate::dt::install::find_path(&dir, rel) else { continue };
            let t = std::time::Instant::now();
            let frames = gfx::decode_uncached(&path).expect("decodes");
            let decode = t.elapsed().as_secs_f64() * 1000.0;
            let key = picture_cache::source_key(&path).expect("key");
            picture_cache::write(&dir_tmp, key, &frames);
            let t = std::time::Instant::now();
            let back = picture_cache::read(&dir_tmp, key).expect("reads");
            let read = t.elapsed().as_secs_f64() * 1000.0;
            assert!(back == frames);
            let bytes: usize = frames.iter().map(|f| f.rgba.len()).sum();
            println!("{rel}: decode {decode:.1} ms, cache read {read:.1} ms, {} frames, {} KiB", frames.len(), bytes / 1024);
        }
        let _ = std::fs::remove_dir_all(&dir_tmp);
    }
}
