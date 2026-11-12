//! The decode store: each install picture is decoded once and shared by every caller, and
//! pictures a screen is about to need are decoded ahead by two background workers
//! ([`preload`]). A caller that asks for a picture a worker is decoding waits for that worker
//! rather than decoding it again; one not yet started is taken off the queue and decoded here.
//! Decoded pictures come from the persistent cache ([`super::picture_cache`]) when it has them,
//! and are written there in the background otherwise.
//!
//! Only plain RGBA leaves this module. Textures are made on the main thread (macroquad), so
//! callers upload what is ready there (see `chrome::warm`).
//!
//! On the phones (and with the `RAZDOR_DROP_PICTURES=1` test switch) a picture leaves the store
//! once nothing holds it and its cache entry exists: its pixels are then in the GPU's shared
//! memory, and a later need re-reads the cache ([`kept`], [`hold`], [`release`]). The desktop
//! keeps every picture, as before.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard};

use super::gfx::{self, Image};
use super::picture_cache;
use super::DtError;
use crate::platform;

/// Most decoded picture bytes the queue keeps: the same caps as the disk cache (1 GiB on the
/// desktop, 256 MiB on the phones, [`picture_cache::CAP_BYTES`]). A queued picture that would go
/// past it is not kept, and the rest of the queue is dropped (those decode on demand).
fn default_cap() -> usize {
    usize::try_from(picture_cache::CAP_BYTES).unwrap_or(usize::MAX)
}

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
    /// Bytes of the ready pictures' frames (RGBA), against [`default_cap`].
    ready_bytes: usize,
    /// The first queue of the process ([`preload`]): its start, for the log of its end.
    started: Option<std::time::Instant>,
    /// Pictures the queue has stored as ready since [`preload`] started.
    queued_done: usize,
    /// The end of the queue is logged once per [`preload`] round.
    done_logged: bool,
    /// Phones (or the test switch): a picture nothing holds leaves the store once its cache entry
    /// exists ([`kept`]). Set once, from [`drops_by_default`].
    drop_pictures: bool,
    /// Pending builds per picture ([`hold`]): a held picture stays in the store.
    holds: HashMap<PathBuf, usize>,
    /// Pictures whose cache entry exists: read from it or written to it. Phones only.
    cached: HashSet<PathBuf>,
    /// Pictures that have left the store at least once: their next read is a re-read of the cache.
    dropped: HashSet<PathBuf>,
}

/// Whether pictures leave the store once their cache entry exists: on the phones, or with
/// `RAZDOR_DROP_PICTURES=1`, the switch that runs the phone path on the desktop (for tests and
/// measurements).
fn drops_by_default() -> bool {
    platform::MOBILE || std::env::var_os("RAZDOR_DROP_PICTURES").is_some_and(|v| v.to_str() == Some("1"))
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(|| Mutex::new(Store { drop_pictures: drops_by_default(), ..Default::default() }));
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
/// on the files' modification times, so it misses for the new files on its own. The holds go
/// too: the queues that made them belong to the old install.
pub fn forget_ready() {
    let mut s = lock();
    s.ready.clear();
    s.ready_bytes = 0;
    s.generation += 1;
    s.holds.clear();
    s.cached.clear();
    s.dropped.clear();
}

/// Whether a picture stays in the store: while a pending build holds it, or until its cache entry
/// exists (then a later need re-reads the cache). Without the drop switch (the desktop), always.
fn kept(s: &Store, path: &Path) -> bool {
    !s.drop_pictures || s.holds.get(path).is_some_and(|n| *n > 0) || !s.cached.contains(path)
}

/// Takes `path` out of the store's ready pictures: its next need re-reads the cache.
fn drop_ready(s: &mut Store, path: &Path) {
    if let Some(old) = s.ready.remove(path) {
        s.ready_bytes -= frames_bytes(&old);
        s.dropped.insert(path.to_path_buf());
    }
}

/// Marks each picture of `paths` as needed by one pending build (a queue's entry, say): while any
/// hold stands, the picture stays in the store, even on the phones. Each hold is given back by one
/// [`release`]. Holding does not decode: a picture not in the store yet is decoded when it is
/// asked for or queued ([`preload`]).
pub fn hold(paths: &[PathBuf]) {
    let mut s = lock();
    for p in paths {
        *s.holds.entry(p.clone()).or_insert(0) += 1;
    }
}

/// Gives back one [`hold`] of `path`, once the build that needed it has made its texture. On the
/// phones the picture leaves the store now if nothing else holds it and its cache entry exists.
pub fn release(path: &Path) {
    let mut s = lock();
    if let Some(n) = s.holds.get_mut(path) {
        *n -= 1;
        if *n == 0 {
            s.holds.remove(path);
        }
    }
    if !kept(&s, path) {
        drop_ready(&mut s, path);
    }
}

/// The ready pictures: how many, and their RGBA bytes (the decode store's memory).
pub fn stored() -> (usize, usize) {
    let s = lock();
    (s.ready.len(), s.ready_bytes)
}

/// Whether a queued picture of `bytes` fits beside `ready` bytes under `cap`.
fn fits(ready: usize, bytes: usize, cap: usize) -> bool {
    ready.saturating_add(bytes) <= cap
}

fn frames_bytes(frames: &[Image]) -> usize {
    frames.iter().map(|f| f.rgba.len()).sum()
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
        let reread = s.dropped.contains(path);
        drop(s);
        let started = std::time::Instant::now();
        let loaded = load(path);
        if reread && crate::diag::profiling() {
            crate::diag!("slow load: re-read {} from the picture cache, {:.1} ms (dropped from the store)", path.display(), started.elapsed().as_secs_f64() * 1000.0);
        }
        finish(path, loaded, generation, false)
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
        s.started.get_or_insert_with(std::time::Instant::now);
        s.done_logged = false;
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
        let _ = finish(&path, loaded, generation, true);
        let mut s = lock();
        if s.queue.is_empty() && s.pending.is_empty() && !s.done_logged && s.queued_done > 0 {
            s.done_logged = true;
            if let Some(started) = s.started {
                crate::diag!("art ahead: queue done, {} pictures, {:.1} MiB decoded, {:.1} s", s.ready.len(), s.ready_bytes as f64 / 1048576.0, started.elapsed().as_secs_f64());
            }
        }
    }
}

/// Where a loaded picture's frames came from.
enum Source {
    /// The persistent cache had them: its entry exists.
    Cache,
    /// Decoded from the install, due to be written to this cache entry (folder, key), if the cache is on.
    Decoded(Miss),
}

/// The frames of `path`: from the persistent cache if it has them, else decoded (and due to be
/// written to the cache). A later need of a dropped picture comes through here too: a re-read of
/// its cache entry, no decode.
fn load(path: &Path) -> Result<(Vec<Image>, Source), DtError> {
    let cache = picture_cache::cache_dir().zip(picture_cache::source_key(path));
    if let Some((dir, key)) = &cache {
        if let Some(frames) = picture_cache::read(dir, *key) {
            return Ok((frames, Source::Cache));
        }
    }
    let frames = gfx::decode_uncached(path)?;
    Ok((frames, Source::Decoded(cache)))
}

/// Stores a loaded picture as ready (unless [`forget_ready`] ran since `generation` was taken, or
/// [`kept`] says nothing holds it on the phones), wakes the waiters and writes a decoded one to the
/// cache. A queued picture (`queued`) is not stored past [`default_cap`]: the queue is dropped then.
fn finish(path: &Path, loaded: Result<(Vec<Image>, Source), DtError>, generation: u64, queued: bool) -> Result<Arc<Vec<Image>>, DtError> {
    let mut s = lock();
    s.pending.remove(path);
    let current = s.generation == generation;
    let out = loaded.map(|(frames, source)| {
        let bytes = frames_bytes(&frames);
        let a = Arc::new(frames);
        let mut to_write = None;
        match source {
            Source::Cache => {
                if current && s.drop_pictures {
                    s.cached.insert(path.to_path_buf());
                }
            }
            Source::Decoded(miss) => to_write = miss,
        }
        if current && (!queued || fits(s.ready_bytes, bytes, default_cap())) {
            if queued {
                s.queued_done += 1;
            }
            if kept(&s, path) {
                if let Some(old) = s.ready.insert(path.to_path_buf(), a.clone()) {
                    s.ready_bytes -= frames_bytes(&old);
                }
                s.ready_bytes += bytes;
            }
        } else if current && queued {
            for p in std::mem::take(&mut s.queue) {
                s.pending.remove(&p);
            }
            crate::diag!("art ahead: the {} MiB cap is reached, the rest of the queue decodes on demand", default_cap() >> 20);
        }
        if let Some((dir, key)) = to_write {
            let frames = a.clone();
            let mark = s.drop_pictures.then(|| path.to_path_buf());
            let _ = std::thread::Builder::new().name("art cache".into()).spawn(move || {
                picture_cache::write(&dir, key, &frames);
                if let Some(path) = mark {
                    if picture_cache::entry_path(&dir, key).is_file() {
                        mark_cached(&path, generation);
                    }
                }
            });
        }
        a
    });
    CHANGED.notify_all();
    out
}

/// The cache entry of `path` now exists (written in the background, see [`finish`]): on the phones a
/// picture nothing holds leaves the store. Ignored after [`forget_ready`].
fn mark_cached(path: &Path, generation: u64) {
    let mut s = lock();
    if s.generation != generation {
        return;
    }
    s.cached.insert(path.to_path_buf());
    if !kept(&s, path) {
        drop_ready(&mut s, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forgetting_drops_the_store_and_a_decode_started_before_is_not_kept() {
        let _tests = store_test();
        let path = PathBuf::from("/razdor-test/forget-ready.lit");
        lock().ready.insert(path.clone(), Arc::new(Vec::new()));
        assert!(is_ready(&path));
        forget_ready();
        assert!(!is_ready(&path), "the old install's pictures are dropped");
        // A decode that started before the forget (a swap in between) is handed back, not stored.
        let before = lock().generation;
        forget_ready();
        let _ = finish(&path, Ok((Vec::new(), Source::Decoded(None))), before, false);
        assert!(!is_ready(&path));
        // One started after it is stored as usual.
        let after = lock().generation;
        let _ = finish(&path, Ok((Vec::new(), Source::Decoded(None))), after, true);
        assert!(is_ready(&path));
        forget_ready();
    }

    /// The tests that look at the one store take this in turn: each one sets its mode and its pictures.
    static STORE_TESTS: Mutex<()> = Mutex::new(());

    fn store_test() -> MutexGuard<'static, ()> {
        STORE_TESTS.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The store in the phones' mode, for one test: the mode is put back when the guard drops.
    struct PhoneMode {
        _tests: MutexGuard<'static, ()>,
        was: bool,
    }

    impl PhoneMode {
        fn on() -> PhoneMode {
            let tests = store_test();
            let was = std::mem::replace(&mut lock().drop_pictures, true);
            PhoneMode { _tests: tests, was }
        }
    }

    impl Drop for PhoneMode {
        fn drop(&mut self) {
            lock().drop_pictures = self.was;
        }
    }

    fn frames_of(v: u8) -> Vec<Image> {
        vec![Image { width: 2, height: 2, rgba: vec![v; 16] }]
    }

    /// The phones' rule: a picture is dropped once its last hold is released and its cache entry
    /// exists; while a hold stands, or with no cache entry, it is kept.
    #[test]
    fn a_picture_is_dropped_after_its_last_build_and_kept_while_one_is_pending() {
        let _mode = PhoneMode::on();
        let path = PathBuf::from("/razdor-test/phone-rule.lit");
        forget_ready();
        // Two pending builds hold it: a decode from the install, written to the cache.
        hold(&[path.clone(), path.clone()]);
        let g = lock().generation;
        let _ = finish(&path, Ok((frames_of(7), Source::Decoded(None))), g, false);
        assert!(is_ready(&path), "held: kept, even with no cache entry");
        // The cache entry appears (the writer's mark): still held, still kept.
        mark_cached(&path, g);
        assert!(is_ready(&path), "held: kept after the cache entry exists");
        release(&path);
        assert!(is_ready(&path), "one build still pending: kept");
        release(&path);
        assert!(!is_ready(&path), "the last build made its texture: dropped");
        assert_eq!(stored().0, 0, "the bytes are given back");
        // A later need re-reads the cache (here: a cache hit) and the picture is kept again.
        let back = finish(&path, Ok((frames_of(7), Source::Cache)), g, false).expect("re-read");
        assert_eq!(back[0].rgba, vec![7; 16], "the same bytes");
        assert!(!is_ready(&path), "not held and cached: the store keeps no copy, the caller has it");
        forget_ready();
    }

    /// A picture with no cache entry (cache off, or the folder full) is kept whatever holds it
    /// leaves: nothing else could give it back.
    #[test]
    fn a_picture_with_no_cache_entry_is_kept_on_the_phones() {
        let _mode = PhoneMode::on();
        let path = PathBuf::from("/razdor-test/phone-no-cache.lit");
        forget_ready();
        let g = lock().generation;
        let _ = finish(&path, Ok((frames_of(3), Source::Decoded(None))), g, false);
        release(&path);
        assert!(is_ready(&path));
        forget_ready();
    }

    /// The desktop keeps every picture: no drop, whatever is released or cached.
    #[test]
    fn the_desktop_never_drops() {
        let _tests = store_test();
        let path = PathBuf::from("/razdor-test/desktop-keeps.lit");
        let was = std::mem::replace(&mut lock().drop_pictures, false);
        forget_ready();
        let g = lock().generation;
        hold(std::slice::from_ref(&path));
        let _ = finish(&path, Ok((frames_of(5), Source::Cache)), g, false);
        mark_cached(&path, g);
        release(&path);
        release(&path);
        assert!(is_ready(&path), "the desktop keeps it");
        assert!(!lock().dropped.contains(&path));
        forget_ready();
        lock().drop_pictures = was;
    }

    /// The pictures of the player's install: a picture dropped on the phones and read again from the
    /// cache gives the same bytes as the on-demand decode, and the re-read is counted as one. Skipped
    /// without `RAZDOR_DT_DIR`.
    #[test]
    fn a_dropped_picture_reads_back_the_same_bytes() {
        let Some((_, paths)) = install_pictures() else { return };
        let _mode = PhoneMode::on();
        let Some(path) = paths.first() else { return };
        forget_ready();
        let on_demand = gfx::decode_uncached(path).expect("decodes");
        // Its first decode writes the cache entry in the background: wait for it, then drop it.
        let first = decode_shared(path).expect("decodes through the store");
        assert!(*first == on_demand);
        for _ in 0..200 {
            if lock().cached.contains(path) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(lock().cached.contains(path), "the cache entry is written");
        drop(first);
        release(path);
        assert!(!is_ready(path), "nothing holds it and it is cached: dropped");
        let again = decode_shared(path).expect("re-read");
        assert!(*again == on_demand, "the re-read is the same picture");
        // Its cache entry is there now, so it is not kept again (a hit is handed back, not stored).
        assert!(!is_ready(path));
        forget_ready();
    }

    #[test]
    fn a_queued_picture_past_the_cap_is_not_kept() {
        assert!(fits(0, 100, 100), "exactly at the cap is kept");
        assert!(!fits(1, 100, 100), "one byte past the cap is not");
        assert!(fits(usize::MAX - 1, 1, usize::MAX));
        assert!(!fits(usize::MAX, 1, usize::MAX - 1), "no overflow past the cap");
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
        let _tests = store_test();
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
