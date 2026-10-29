//! The session slot (`save::SESSION`, Razdor's, the owner's decision): the game kept in a save
//! of its own while it is played, so that after the OS ended the app in the background (or the
//! game crashed) the next launch offers to continue. The slot is written every
//! [`EVERY`] seconds of play, when the window loses its focus, and at battle start; a marker file
//! (`save::session_marker`) says a session is under way and is removed when the game is left
//! (the main menu) or the app closes. The writes (bzip2 of a late game is most of their time)
//! go to a thread of their own, in the order they were sent.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::OnceLock;

use razdor::rules::game::Game;
use razdor::rules::save::{self, SaveMeta};

/// Seconds of play between two looks at the game (written when it changed).
const EVERY: f64 = 30.0;

enum Job {
    /// The session slot and its marker, from a session's JSON.
    Write { dir: PathBuf, meta: SaveMeta, json: Vec<u8> },
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
                    Job::Write { dir, meta, json } => {
                        if let Err(e) = save::write_session(&dir, &meta, &json) {
                            razdor::diag!("session: {e}");
                        }
                    }
                    Job::End { dir } => {
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
    /// The clock time of the last look at the game (`ui::clock::now`).
    last_write: f64,
    /// The hash of the last JSON written (`None` after the game was left: the next one is written).
    last_hash: Option<u64>,
    /// The marker is there: a session is under way.
    live: bool,
    /// Its input subscriber, for the focus (registered at the first look).
    focus_sub: Option<usize>,
}

impl Default for Keeper {
    fn default() -> Self {
        Keeper { last_write: f64::NEG_INFINITY, last_hash: None, live: false, focus_sub: None }
    }
}

impl Keeper {
    /// Called once per frame. `game` is the game on screen (`None` on the main menu and in the
    /// editor and custom battles); `savable` says the session may be written now (not in a
    /// battle, a test play, or while a window of the game is over the map).
    pub fn tick(&mut self, game: Option<&Game>, savable: bool, now: f64) {
        let lost = self.focus_lost();
        let Some(game) = game else {
            if self.live {
                self.end_session();
            }
            return;
        };
        if !savable || !(lost || now - self.last_write >= EVERY) {
            return;
        }
        self.last_write = now;
        // The game's JSON that changed; the meta's time alone does not count.
        let Ok((meta, json)) = save::session_json(game) else { return };
        let hash = hash_of(&json);
        if self.last_hash == Some(hash) {
            return;
        }
        self.last_hash = Some(hash);
        self.send_write(meta, json);
    }

    /// Writes the session slot now (the battle start does, with the foe taken off: `saves::battle`).
    pub fn save_now(&mut self, game: &Game) {
        let Ok((meta, json)) = save::session_json(game) else { return };
        self.last_write = crate::ui::clock::now();
        self.last_hash = Some(hash_of(&json));
        self.send_write(meta, json);
    }

    /// The app closes: the marker goes once the pending writes are done (the process exits
    /// after this, `main::exit_now`).
    pub fn end(&mut self) {
        self.end_session();
        let (done, wait) = channel();
        let _ = writer().send(Job::Flush(done));
        let _ = wait.recv();
    }

    fn send_write(&mut self, meta: SaveMeta, json: Vec<u8>) {
        let Some(dir) = save::games_dir() else { return };
        self.live = true;
        let _ = writer().send(Job::Write { dir, meta, json });
    }

    /// The session is over: the marker goes (after the writes sent before it).
    fn end_session(&mut self) {
        self.live = false;
        self.last_hash = None;
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
    use super::hash_of;

    /// An unchanged game hashes the same when only its meta's time moved; a changed one does not.
    #[test]
    fn the_hash_ignores_the_meta_time_but_not_the_game() {
        let at = |secs: u32, gold: u32| format!(r#"{{"meta":{{"name":"Last session","saved_at":{secs}}},"game":{{"gold":{gold}}}}}"#);
        assert_eq!(hash_of(at(1, 5).as_bytes()), hash_of(at(2, 5).as_bytes()));
        assert_ne!(hash_of(at(1, 5).as_bytes()), hash_of(at(1, 6).as_bytes()));
    }
}
