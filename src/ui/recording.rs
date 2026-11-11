//! The game's recording (`rules::recording::session`): while the setting «Записывать игру» is on,
//! the game records to a replay file beside its saves. A game without a session gets one (or
//! resumes the file of the save it was loaded from), the setting off ends it, and each frame
//! writes what is due. The game's end (a return to the menu, a new game, a load, the quit)
//! closes the session: its pending entries are written, so at most the day in play is at risk.

use std::cell::RefCell;

use razdor::rules::game::Game;
use razdor::rules::recording::header::{install_hash, ReplayHeader};
use razdor::rules::recording::session::{replays_dir, Session};

use super::audio::Settings;

thread_local! {
    /// The session whose file is open: the current game's, or one a game left that is not closed yet.
    static LIVE: RefCell<Option<Session>> = const { RefCell::new(None) };
}

/// Closes the live session (its entries are written and its file synced). `game` is the game it
/// belonged to, when it is still at hand: its end state is written as the last keyframe.
fn close_live(game: Option<&mut Game>) {
    if let Some(s) = LIVE.with_borrow_mut(|l| l.take()) {
        s.close(game);
    }
}

/// Keeps the game's recording in step with the setting (every frame).
pub fn follow(game: &mut Option<Game>, settings: &Settings) {
    let Some(g) = game.as_mut() else {
        close_live(None);
        return;
    };
    if !settings.record_game {
        close_live(Some(g));
        g.replay = None;
        return;
    }
    match g.replay.clone() {
        Some(s) => s.pump(g),
        None => {
            // A game's file is closed before another one's opens: a resume truncates the file.
            close_live(None);
            let (Some(games), Some(install)) = (razdor::rules::save::games_dir(), razdor::rules::save::install_dir()) else { return };
            let Ok(hash) = install_hash(&install) else { return };
            let header = ReplayHeader::of(g, hash);
            let dir = replays_dir(&games);
            let resume = g.replay_resume.take();
            let made = match resume {
                Some(r) => Session::resume(&dir, &r.file, r.pos, g, &header),
                None => Session::start(&dir, g, &header),
            };
            if let Ok(s) = made {
                LIVE.with_borrow_mut(|l| *l = Some(s.clone()));
                g.replay = Some(s);
            }
        }
    }
}

/// The app's exit: the game's session (if any) is closed with the game's end state.
pub fn quit(game: &mut Option<Game>) {
    match game.as_mut() {
        Some(g) => {
            close_live(Some(g));
            g.replay = None;
        }
        None => close_live(None),
    }
}
