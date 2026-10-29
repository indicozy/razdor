//! The session slot (`save::SESSION`, Razdor's, the owner's decision): the game kept in a save
//! of its own while it is played, so that after the OS ended the app (or the game crashed) the
//! next launch offers to continue. A marker file (`save::session_marker`) says a session is under
//! way and is removed when the game is left (the main menu) or the app closes.
//!
//! The newest savable state is kept in [`LATEST`]: refreshed at a rest frame (the hero stands) at
//! most every [`SNAP_EVERY`] seconds, while the game changed (a cheap key first, so most looks
//! serialise nothing), and every [`EVERY`] seconds regardless; a walk takes no look. The writes go to a thread of their own, in the order they were
//! sent. The hooks that write at once when the app is left ([`flush_now`]: Android `onPause`,
//! iOS's background notification, the desktop's SIGTERM) are the only writes at that moment: the
//! frame loop does not run while an app is in the background.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

use razdor::rules::game::Game;
use razdor::rules::save::{self, SaveMeta};

/// Seconds of play between two looks at the game (a snapshot is taken when its key changed).
const SNAP_EVERY: f64 = 2.0;
/// Seconds of play between two writes of the slot, and between two full serialisations.
const EVERY: f64 = 30.0;

/// The newest savable state of the session, as the writer and the pause hooks write it.
#[derive(Clone)]
struct Snapshot {
    dir: PathBuf,
    meta: SaveMeta,
    json: Vec<u8>,
    hash: u64,
}

impl Snapshot {
    fn new(dir: PathBuf, meta: SaveMeta, json: Vec<u8>) -> Self {
        let hash = hash_of(&json);
        Snapshot { dir, meta, json, hash }
    }
}

/// The newest savable state (`None` when no session is under way).
static LATEST: Mutex<Option<Snapshot>> = Mutex::new(None);
/// The hash of the state last written to disk in this session (`None` after the session ended).
static WRITTEN: Mutex<Option<u64>> = Mutex::new(None);
/// Held around every write of the slot and every removal of the marker: the writer thread and
/// the hooks never write at once (they share one `.tmp`). Taken before [`LATEST`] and [`WRITTEN`].
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// A lock that a panic elsewhere cannot leave unusable: the hooks must not panic in a callback.
fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

enum Job {
    /// Writes [`LATEST`] when it is not on disk yet.
    Write,
    /// The marker goes: the session ended.
    End { dir: PathBuf },
    /// Replies when every job sent before it is done.
    Flush(Sender<()>),
}

/// The writer thread's job queue, the thread started at the first job.
fn writer() -> &'static Sender<Job> {
    static WRITER: OnceLock<Sender<Job>> = OnceLock::new();
    WRITER.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::spawn(move || {
            for job in rx {
                match job {
                    Job::Write => flush_now(),
                    Job::End { dir } => {
                        let _guard = lock(&WRITE_LOCK);
                        let _ = std::fs::remove_file(save::session_marker(&dir));
                    }
                    Job::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        tx
    })
}

/// Writes the newest savable state now when it is not on disk yet. Callable from any thread
/// (no macroquad calls): the platform hooks call it when the app is left, and the desktop's
/// signal watcher. A state already written does nothing; no state (the session ended) does nothing.
pub fn flush_now() {
    let _guard = lock(&WRITE_LOCK);
    let Some(snapshot) = lock(&LATEST).clone() else { return };
    if *lock(&WRITTEN) == Some(snapshot.hash) {
        return;
    }
    match save::write_session(&snapshot.dir, &snapshot.meta, &snapshot.json) {
        Ok(_) => *lock(&WRITTEN) = Some(snapshot.hash),
        Err(e) => razdor::diag!("session: {e}"),
    }
}

/// Whether the newest savable state is not on disk yet.
fn unwritten() -> bool {
    let hash = lock(&LATEST).as_ref().map(|s| s.hash);
    hash.is_some() && hash != *lock(&WRITTEN)
}

/// Drops the newest state and what was written, so that the next session writes again (the
/// marker is gone, and an identical game must put it back). Waits for a write under way.
fn forget_latest() {
    let _guard = lock(&WRITE_LOCK);
    *lock(&LATEST) = None;
    *lock(&WRITTEN) = None;
}

/// A cheap summary of the game: its clock, the hero's place and purse, the army's state. No
/// allocation. A change that moves none of these (an item moved, an army's hit points) is seen
/// by the full serialisation every [`EVERY`] seconds instead.
fn key_of(game: &Game) -> [u64; 8] {
    let squad = game.squad.iter().fold(0u64, |a, u| a.wrapping_mul(31).wrapping_add((u.hp as i64 + u.xp as i64 * 7 + u.level as i64 * 13) as u64));
    [
        game.clock.total_minutes().to_bits(),
        game.pos.0.to_bits() as u64,
        game.pos.1.to_bits() as u64,
        game.gold as i64 as u64,
        game.mana as i64 as u64,
        game.location.map_or(0, |l| l as u64 + 1),
        squad,
        game.squad.len() as u64,
    ]
}

/// Notes that the window lost its focus (macroquad's "minimized" is sent on focus changes).
struct Lost(bool);

impl macroquad::miniquad::EventHandler for Lost {
    fn update(&mut self) {}
    fn draw(&mut self) {}
    fn window_minimized_event(&mut self) {
        self.0 = true;
    }
}

/// Keeps the session slot up to date. One on the App (`App::session`).
pub struct Keeper {
    /// The clock time of the last write gate (`ui::clock::now`): the periodic write, the focus.
    last_write: f64,
    /// The clock time of the last look at the game (every [`SNAP_EVERY`] seconds of play).
    last_look: f64,
    /// The clock time of the last full serialisation of the game.
    last_full: f64,
    /// The key of the game at the last full serialisation (`None`: the next look serialises).
    key: Option<[u64; 8]>,
    /// The marker is there: a session is under way.
    live: bool,
    /// Its input subscriber, for the focus (registered at the first look).
    focus_sub: Option<usize>,
}

impl Default for Keeper {
    fn default() -> Self {
        Keeper { last_write: f64::NEG_INFINITY, last_look: f64::NEG_INFINITY, last_full: f64::NEG_INFINITY, key: None, live: false, focus_sub: None }
    }
}

impl Keeper {
    /// Called once per frame. `game` is the game on screen (`None` on the main menu and in the
    /// editor and custom battles); `savable` says the session may be written now (not in a
    /// battle, a test play, or while a window of the game is over the map). Most frames do
    /// nothing here but the time check and the focus.
    pub fn tick(&mut self, game: Option<&Game>, savable: bool, now: f64) {
        let lost = self.focus_lost();
        let Some(game) = game else {
            if self.live {
                self.end_session();
            }
            return;
        };
        // The look only on a rest frame (the hero stands, no step's window is playing): a walk
        // costs nothing here. The first rest frame after a walk takes it, so the state is the
        // last stop's; the pause hooks write that.
        if savable && !game.moving() && !game.step_playing() && now - self.last_look >= SNAP_EVERY {
            self.last_look = now;
            self.look(game, now);
        }
        // The focus writes the last savable state even when the game is not savable now.
        if lost || (savable && now - self.last_write >= EVERY) {
            self.last_write = now;
            if unwritten() {
                self.send_write();
            }
        }
    }

    /// Writes the session slot now (the battle start does, with the foe taken off: `saves::battle`).
    pub fn save_now(&mut self, game: &Game) {
        let Ok((meta, json)) = save::session_json(game) else { return };
        let Some(dir) = save::games_dir() else { return };
        let now = crate::ui::clock::now();
        self.last_write = now;
        self.last_look = now;
        self.last_full = now;
        self.key = Some(key_of(game));
        *lock(&LATEST) = Some(Snapshot::new(dir, meta, json));
        self.send_write();
    }

    /// The app closes: the marker goes once the pending writes are done (the process exits
    /// after this, `main::exit_now`).
    pub fn end(&mut self) {
        self.end_session();
        let (done, wait) = channel();
        let _ = writer().send(Job::Flush(done));
        let _ = wait.recv();
    }

    /// Takes the newest state of the game when its key changed, or every [`EVERY`] seconds.
    fn look(&mut self, game: &Game, now: f64) {
        let key = key_of(game);
        if self.key == Some(key) && now - self.last_full < EVERY {
            return;
        }
        let Ok((meta, json)) = save::session_json(game) else { return };
        let Some(dir) = save::games_dir() else { return };
        self.key = Some(key);
        self.last_full = now;
        self.live = true;
        *lock(&LATEST) = Some(Snapshot::new(dir, meta, json));
    }

    /// Asks the writer thread to write [`LATEST`]; the marker is there from then on.
    fn send_write(&mut self) {
        self.live = true;
        let _ = writer().send(Job::Write);
    }

    /// The session is over: the newest state is dropped before the marker goes, so a pause
    /// hook after this writes nothing, and the marker goes after the writes sent before it.
    fn end_session(&mut self) {
        self.live = false;
        self.key = None;
        forget_latest();
        if let Some(dir) = save::games_dir() {
            let _ = writer().send(Job::End { dir });
        }
    }

    /// Whether the window lost its focus since the last look.
    fn focus_lost(&mut self) -> bool {
        let sub = *self.focus_sub.get_or_insert_with(macroquad::input::utils::register_input_subscriber);
        let mut lost = Lost(false);
        macroquad::input::utils::repeat_all_miniquad_input(&mut lost, sub);
        lost.0
    }
}

/// Desktop (Linux, macOS): SIGTERM and SIGHUP (the session logs out, the machine shuts down,
/// `kill`) write the session slot and end the process, leaving the marker, so the next launch
/// offers to continue. The handler only stores the signal number; a watcher thread does the
/// writing, outside the signal context. SIGINT is left alone.
#[cfg(all(unix, not(any(target_os = "android", target_os = "ios"))))]
pub fn install_exit_hooks() {
    use std::sync::atomic::{AtomicI32, Ordering};
    static SIGNAL: AtomicI32 = AtomicI32::new(0);
    extern "C" fn on_signal(sig: libc::c_int) {
        SIGNAL.store(sig, Ordering::SeqCst);
    }
    for sig in [libc::SIGTERM, libc::SIGHUP] {
        // SAFETY: a zeroed sigaction gets the handler and an empty mask, as sigaction takes it.
        unsafe {
            let mut action: libc::sigaction = std::mem::zeroed();
            action.sa_sigaction = on_signal as libc::sighandler_t;
            action.sa_flags = libc::SA_RESTART;
            libc::sigemptyset(&mut action.sa_mask);
            libc::sigaction(sig, &action, std::ptr::null_mut());
        }
    }
    std::thread::spawn(|| loop {
        std::thread::sleep(std::time::Duration::from_millis(50));
        let sig = SIGNAL.load(Ordering::SeqCst);
        if sig != 0 {
            flush_now();
            razdor::diag!("session: signal {sig}, the session slot written and the app ends");
            // SAFETY: `_exit` only ends the process; no Rust state is used after it (as `main::exit_now`).
            unsafe { libc::_exit(128 + sig) }
        }
    });
}

/// The hash of a save's JSON from its game on: the meta (the time it was saved) comes before
/// the `"game"` field, so an unchanged game hashes the same from one look to the next.
fn hash_of(json: &[u8]) -> u64 {
    // A quote inside a JSON string is escaped, so `,"game":` marks the field.
    let from = json.windows(8).position(|w| w == b",\"game\":").unwrap_or(0);
    let mut h = DefaultHasher::new();
    json[from..].hash(&mut h);
    h.finish()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use razdor::rules::content::{Content, HeroClass};
    use razdor::rules::game::Game;
    use razdor::rules::save;

    use std::sync::Mutex;

    use super::{flush_now, forget_latest, hash_of, lock, Snapshot, LATEST, WRITTEN};

    /// The tests share the process's newest state: one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// An unchanged game hashes the same when only its meta's time moved; a changed one does not.
    #[test]
    fn the_hash_ignores_the_meta_time_but_not_the_game() {
        let at = |secs: u32, gold: u32| format!(r#"{{"meta":{{"name":"Last session","saved_at":{secs}}},"game":{{"gold":{gold}}}}}"#);
        assert_eq!(hash_of(at(1, 5).as_bytes()), hash_of(at(2, 5).as_bytes()));
        assert_ne!(hash_of(at(1, 5).as_bytes()), hash_of(at(1, 6).as_bytes()));
    }

    /// A fresh folder for one test (never the player's saves).
    fn test_dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("razdor-test-session-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// The newest state is written once: a second flush of the same state writes nothing.
    #[test]
    fn flush_now_writes_the_newest_state_once() {
        let _serial = lock(&SERIAL);
        let dir = test_dir("once");
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let (meta, json) = save::session_json(&game).unwrap();
        *lock(&LATEST) = Some(Snapshot::new(dir.clone(), meta, json));
        flush_now();
        assert!(save::session_path(&dir).exists(), "the newest state is written");
        assert!(save::session_marker(&dir).exists(), "with the marker");
        std::fs::remove_file(save::session_path(&dir)).unwrap();
        flush_now();
        assert!(!save::session_path(&dir).exists(), "the same state is not written twice");
        forget_latest();
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Once the session ended (its newest state dropped), a pause hook writes nothing and does
    /// not bring the marker back.
    #[test]
    fn after_the_session_ended_flush_now_writes_nothing() {
        let _serial = lock(&SERIAL);
        let dir = test_dir("ended");
        let game = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        let (meta, json) = save::session_json(&game).unwrap();
        *lock(&LATEST) = Some(Snapshot::new(dir.clone(), meta, json));
        forget_latest();
        assert!(lock(&LATEST).is_none() && lock(&WRITTEN).is_none(), "the session's state is gone");
        flush_now();
        assert!(!save::session_path(&dir).exists() && !save::session_marker(&dir).exists(), "nothing is written");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
