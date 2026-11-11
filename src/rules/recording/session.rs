//! A game's recording session: the replay file the game's actions and engine records go to. The
//! session is attached to the game (`Game::replay`) and to the thread's hook (its recorder).
//! The entries of a game day are written when the day changes; keyframes (the state's hash and
//! its snapshot) every 10 game days and at each battle's start; a battle's start state goes to
//! its block. A save records the file and the length it had then (`ReplayRef`), so loading the
//! save resumes the file there: `resume` cuts it at that length and starts a new segment from the
//! loaded game's state.

use std::path::Path;
use std::sync::{Arc, Mutex};

use super::file::{replay_name_ok, ReplayRef, Writer};
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
        Session::fresh(dir, game, header)
    }

    /// `file` cut at `pos` (a save's place in it), then a new segment from `game`'s state (a load).
    /// The name is checked before any file is touched; a name the game does not make, a missing
    /// file, or one that is not a replay starts a fresh file instead.
    pub fn resume(dir: &Path, file: &str, pos: u64, game: &Game, header: &ReplayHeader) -> Result<Session, String> {
        if replay_name_ok(file) {
            if let Ok(writer) = Writer::open(&dir.join(file), header, Some(pos)) {
                return Session::begin(writer, file.to_string(), game);
            }
        }
        Session::fresh(dir, game, header)
    }

    fn fresh(dir: &Path, game: &Game, header: &ReplayHeader) -> Result<Session, String> {
        let name = new_name();
        let writer = Writer::open(&dir.join(&name), header, None)?;
        Session::begin(writer, name, game)
    }

    fn begin(writer: Writer, name: String, game: &Game) -> Result<Session, String> {
        let day = game.clock.day_index();
        let inner = Inner { writer, name, pending: Vec::new(), day, keyframe_day: day };
        let (json, hash) = capture(game)?;
        inner.writer.keyframe(true, game.stamp(), day, hash, json);
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
            // A keyframe is a checkpoint: the playback compares it after the stop's snap (which
            // only runs when one is due), so it is taken when no snap is due, and the next frame
            // asks again otherwise.
            match capture(game) {
                Ok((json, hash)) if !game.snap_due => {
                    g.flush();
                    g.writer.keyframe(false, game.stamp(), day, hash, json);
                    g.keyframe_day = day;
                }
                _ => hook::request_keyframe(),
            }
        }
    }

    /// Ends the session at the game's end (a return to the menu, a quit, a new game or a load):
    /// the pending entries are written, and the game's state is written as a last keyframe when
    /// it can be taken (the playback checks it as it checks the others: the step paused and the
    /// snap due are finished first, as the screen does before a save). The file is synced and the
    /// hook lets go of the session.
    pub fn close(&self, mut game: Option<&mut Game>) {
        if let Ok(mut g) = self.inner.lock() {
            // What the step and the snap record is part of the day's entries, so it comes first.
            if let Some(game) = game.as_deref_mut() {
                game.finish_pause();
                game.armies_snap();
            }
            g.flush();
            if let Some(game) = game {
                if let Ok((json, hash)) = capture(game) {
                    let day = game.clock.day_index();
                    g.writer.keyframe(false, game.stamp(), day, hash, json);
                }
            }
            g.writer.sync();
        }
        hook::take_recorder();
    }

    /// Ends the session without the game (the entries are written, the file synced).
    pub fn finish(&self) {
        self.close(None);
    }
}

/// The folder replays live in, beside a game's saves.
pub fn replays_dir(games: &Path) -> std::path::PathBuf {
    games.join("replays")
}
