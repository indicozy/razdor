//! The game's recording (`rules::recording::session`): while the setting «Записывать игру» is on,
//! the game records to a replay file beside its saves. A game without a session gets one (or
//! resumes the file of the save it was loaded from), the setting off ends it, and each frame
//! writes what is due.

use razdor::rules::game::Game;
use razdor::rules::recording::header::{install_hash, ReplayHeader};
use razdor::rules::recording::session::{replays_dir, Session};

use super::audio::Settings;

/// Keeps the game's recording in step with the setting (every frame).
pub fn follow(game: &mut Option<Game>, settings: &Settings) {
    let Some(g) = game.as_mut() else { return };
    if !settings.record_game {
        if let Some(s) = g.replay.take() {
            s.finish();
        }
        return;
    }
    match g.replay.clone() {
        Some(s) => s.pump(g),
        None => {
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
                g.replay = Some(s);
            }
        }
    }
}
