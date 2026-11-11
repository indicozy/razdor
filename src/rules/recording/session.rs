//! A game's recording session: the replay file the game's actions and engine records go to. The
//! session is attached to the game (`Game::replay`) and to the thread's hook (its recorder).
//! The entries of a game day are written when the day changes; keyframes (the state's hash and
//! its snapshot) every 10 game days and at each battle's start; a battle's start state goes to
//! its block. A save records the file and the length it had then (`ReplayRef`), so loading the
//! save resumes the file there: `resume` cuts it at that length and starts a new segment from the
//! loaded game's state.

use std::path::Path;
use std::sync::{Arc, Mutex};

use super::file::{ReplayRef, Writer};
use super::header::ReplayHeader;
use super::hook::{self, Recorder};
use super::snapshot::capture;
use super::stream::Entry;
use crate::rules::game::Game;

/// Game days between two keyframes.
pub const KEYFRAME_DAYS: u64 = 10;

struct Inner {
    writer: Writer,
    name: String,
    pending: Vec<Entry>,
    /// The game day the pending entries belong to.
    day: u64,
    /// The day of the last keyframe (a segment's start counts).
    keyframe_day: u64,
}

impl Inner {
    fn flush(&mut self) {
        let entries = std::mem::take(&mut self.pending);
        self.writer.entries(&entries);
    }
}

/// A recording session (cheap to clone: the game and the hook share it).
#[derive(Clone)]
pub struct Session {
    inner: Arc<Mutex<Inner>>,
}

/// The hook's side of a session: what the rules report is pending until the day is written.
struct SessionRecorder(Arc<Mutex<Inner>>);

impl Recorder for SessionRecorder {
    fn record(&mut self, entry: Entry) {
        if let Ok(mut g) = self.0.lock() {
            g.pending.push(entry);
        }
    }
}

fn new_name() -> String {
    let millis = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis());
    format!("replay-{millis}.rzreplay")
}

impl Session {
    /// A new file in `dir`, starting from `game`'s state (a segment's start).
    pub fn start(dir: &Path, game: &Game, header: &ReplayHeader) -> Result<Session, String> {
        let name = new_name();
        Session::open(dir, name, None, game, header)
    }

    /// `file` cut at `pos` (a save's place in it), then a new segment from `game`'s state (a load).
    pub fn resume(dir: &Path, file: &str, pos: u64, game: &Game, header: &ReplayHeader) -> Result<Session, String> {
        Session::open(dir, file.to_string(), Some(pos), game, header)
    }

    fn open(dir: &Path, name: String, cut: Option<u64>, game: &Game, header: &ReplayHeader) -> Result<Session, String> {
        let writer = Writer::open(&dir.join(&name), header, cut)?;
        let day = game.clock.day_index();
        let inner = Inner { writer, name, pending: Vec::new(), day, keyframe_day: day };
        let (json, hash) = capture(game)?;
        inner.writer.keyframe(true, game.stamp(), hash, json);
        let inner = Arc::new(Mutex::new(inner));
        hook::install_recorder(Box::new(SessionRecorder(inner.clone())));
        Ok(Session { inner })
    }

    /// Where a save made now is in the file: the day's entries are written and synced first.
    pub fn save_point(&self) -> ReplayRef {
        let mut g = self.inner.lock().expect("the session lock");
        g.flush();
        let pos = g.writer.sync();
        ReplayRef { file: g.name.clone(), pos }
    }

    /// The name of the file this session writes.
    pub fn file_name(&self) -> String {
        self.inner.lock().expect("the session lock").name.clone()
    }

    /// Called every frame: writes a finished day's entries, the battles begun since the last
    /// call, and the keyframes due (every [`KEYFRAME_DAYS`] days, or asked for). A keyframe needs
    /// the game at rest; if it is not, it is asked for again.
    pub fn pump(&self, game: &Game) {
        let mut g = self.inner.lock().expect("the session lock");
        let day = game.clock.day_index();
        if day != g.day {
            g.flush();
            g.day = day;
        }
        for start in hook::take_battle_starts() {
            g.writer.battle(&start);
        }
        let due = day >= g.keyframe_day + KEYFRAME_DAYS;
        if due || hook::take_keyframe_request() {
            match capture(game) {
                Ok((json, hash)) => {
                    g.flush();
                    g.writer.keyframe(false, game.stamp(), hash, json);
                    g.keyframe_day = day;
                }
                Err(_) => hook::request_keyframe(),
            }
        }
    }

    /// Ends the session: the pending entries are written and the hook lets go of it.
    pub fn finish(&self) {
        if let Ok(mut g) = self.inner.lock() {
            g.flush();
            g.writer.sync();
        }
        hook::take_recorder();
    }
}

/// The folder replays live in, beside a game's saves.
pub fn replays_dir(games: &Path) -> std::path::PathBuf {
    games.join("replays")
}
