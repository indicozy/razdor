//! Playback of a recorded input stream: the game is taken to each recorded place in turn and the
//! recorded action is applied there by the entry point it came from (`actions::apply_game`,
//! `actions::apply_battle`). Between two places the rules run as the screen runs them: frames of
//! `dt` seconds (any frame times give the same states, see `docs/flows/replays.md`), the map's
//! music changes at the recorded places, and a battle's enemy plays its turns by «Максимум»'s
//! recorded moves or the battle AI. The caller installs the stream's engine records first
//! ([`super::hook::install_playback`]).

use super::actions::{apply_battle, apply_game};
use super::hook;
use super::stream::{Entry, EngineInput, Input, PlayerAction, Stamp};
use crate::rules::battle::{Battle, MaxPoll, MaxThinker, Outcome, Team};
use crate::rules::game::Game;
use crate::rules::music::WORLD_THEME;

/// Frames a playback may take to reach one place before it is a drift.
const MAX_FRAMES: u64 = 4_000_000;
/// Battle steps a playback may take to reach the player's next turn.
const MAX_BATTLE_STEPS: usize = 20_000;

/// The playback's state: the game, the battle under way, and the map music's track.
pub struct Player {
    pub game: Game,
    battle: Option<(Battle, MaxThinker)>,
    music_pick: usize,
    /// The frame time of the playback (its own clock).
    dt: f32,
    /// How many frames were played (the drift guard counts them).
    frames: u64,
}

impl Player {
    pub fn new(game: Game, dt: f32) -> Player {
        Player { game, battle: None, music_pick: WORLD_THEME, dt, frames: 0 }
    }

    /// The game at its recorded place: time runs (frames of `dt`) until the game stands at the end
    /// of the recorded stretch (`target.stretch`), with the events the screen holds for its glide
    /// delivered (the screen decides only then, as the driver does). An action or a check is
    /// recorded after its stretch's paused step is finished (`actions::scope`), so the place is the
    /// stretch's end: the arrivals taken inside it are not compared, as a frame's budget only
    /// changes how the stretch is split. A music change is the exception: it falls at its exact
    /// arrival (the ask), and the time stops there to draw it.
    pub fn advance_to(&mut self, target: Stamp) -> Result<(), String> {
        loop {
            if self.game.stretches == target.stretch && self.game.held.is_empty() {
                // The paused step of the recorded stretch, if any, is finished: the action (or the
                // check) does the same at its place.
                self.game.replay_stop_before = Some(target.stretch + 1);
                self.game.finish_pause();
                self.game.replay_stop_before = None;
                if self.game.stretches == target.stretch && !self.game.ai_pending() && self.game.held.is_empty() {
                    // A contact the walk reaches at this stretch's end is part of it (no time, so
                    // it is not a stretch): it is played before the place.
                    if !self.game.zero_time_next() {
                        return Ok(());
                    }
                    self.tick_toward(target.stretch, None)?;
                    continue;
                }
            }
            self.tick_toward(target.stretch, None)?;
        }
    }

    /// The game at a recorded arrival: time runs until the game has taken exactly `target.ask`
    /// arrivals in the recorded stretch. A music change falls there, mid-step, as it did live
    /// (`Game::music_rotate` records its stamp), so the time stops at the arrival and the change is
    /// drawn by the caller.
    pub fn advance_exact(&mut self, target: Stamp) -> Result<(), String> {
        loop {
            if self.game.stamp() == target && self.game.held.is_empty() {
                return Ok(());
            }
            if self.game.stretches == target.stretch && self.game.asks > target.ask {
                return Err(format!("the game passed the recorded place {target:?} (now {:?})", self.game.stamp()));
            }
            let ask = (self.game.stretches == target.stretch && target.ask > self.game.asks).then_some(target.ask);
            self.tick_toward(target.stretch, ask)?;
        }
    }

    /// One frame of the game's time, stopped before the stretch after `stretch` begins and, when
    /// given, at the arrival `ask` of the stretch.
    fn tick_toward(&mut self, stretch: u64, ask: Option<u32>) -> Result<(), String> {
        if self.game.stretches > stretch {
            return Err(format!("the game passed the recorded stretch {stretch} (now {:?})", self.game.stamp()));
        }
        self.frames += 1;
        if self.frames > MAX_FRAMES {
            return Err(format!("the game never reached stretch {stretch} (now {:?})", self.game.stamp()));
        }
        self.game.replay_stop_before = Some(stretch + 1);
        self.game.replay_stop = ask;
        let _ = self.game.tick_shown_within(self.dt, &mut || true);
        self.game.replay_stop = None;
        self.game.replay_stop_before = None;
        Ok(())
    }

    /// Applies one recorded entry (an engine record is consumed by the hooks, or is a check).
    pub fn apply(&mut self, e: &Entry) -> Result<(), String> {
        match &e.input {
            Input::Engine(EngineInput::Check { hash }) => {
                self.advance_to(e.at)?;
                self.game.finish_pause();
                self.game.armies_snap();
                if let Some((b, _)) = self.battle.as_ref() {
                    self.game.battle_write_back(b);
                }
                let got = super::snapshot::state_hash(&self.game);
                if got != *hash {
                    return Err(format!("drift at {:?}: the state hashes {got:016x}, recorded {hash:016x}", e.at));
                }
                Ok(())
            }
            Input::Engine(EngineInput::Music { pick }) => {
                // The change is drawn at its place in the stream (after the entries before it).
                self.advance_exact(e.at)?;
                let (next, _) = self.game.music_rotate(self.music_pick);
                if next != *pick as usize {
                    return Err(format!("the music track is {next}, recorded {pick}"));
                }
                self.music_pick = next;
                Ok(())
            }
            Input::Engine(_) => Ok(()),
            Input::Player(PlayerAction::StartBattle) => {
                self.advance_to(e.at)?;
                let mut b = self.game.start_battle();
                b.begin();
                self.battle = Some((b, MaxThinker::new(Team::Enemy)));
                self.run_enemy()
            }
            Input::Player(PlayerAction::ResolveBattle) => {
                self.advance_to(e.at)?;
                let (b, _) = self.battle.take().ok_or("a battle to resolve, none under way")?;
                self.game.resolve_battle(&b);
                Ok(())
            }
            Input::Player(PlayerAction::Battle(act)) => {
                let (b, _) = self.battle.as_mut().ok_or("a battle's action, none under way")?;
                apply_battle(b, act);
                self.run_enemy()
            }
            Input::Player(a) => {
                self.advance_to(e.at)?;
                apply_game(&mut self.game, a);
                Ok(())
            }
        }
    }

    /// The enemy's turns, as the screen plays them between the player's actions: «Максимум»'s
    /// recorded moves where the battle is set to it, the battle AI elsewhere. Stops at the
    /// player's turn or at the end.
    fn run_enemy(&mut self) -> Result<(), String> {
        let Some((b, thinker)) = self.battle.as_mut() else { return Ok(()) };
        // Every action is written back into the armies (the screen does so each frame, the
        // interface after each action: `Game::battle_write_back`).
        self.game.battle_write_back(b);
        if b.outcome() == Outcome::Ongoing && b.active().is_none() {
            return Ok(());
        }
        for _ in 0..MAX_BATTLE_STEPS {
            self.game.battle_write_back(b);
            if b.outcome() != Outcome::Ongoing {
                return Ok(());
            }
            let Some(id) = b.active() else { return Ok(()) };
            if b.fighters[id].team != Team::Enemy {
                return Ok(());
            }
            if b.max_ai() {
                match thinker.poll(b) {
                    MaxPoll::Ready(r) => {
                        b.apply_max(r);
                    }
                    MaxPoll::Thinking => return Err("«Максимум» has no recorded move here".into()),
                    MaxPoll::Failed => {
                        b.ai_step();
                    }
                }
            } else {
                b.ai_step();
            }
        }
        Err("the battle does not end".into())
    }

    /// The game as the playback left it (a battle still under way is dropped).
    pub fn finish(self) -> Game {
        self.game
    }

    /// Whether a battle is under way (the replay's battle view would show it).
    pub fn in_battle(&self) -> bool {
        self.battle.is_some()
    }
}

/// Plays `entries` from `game` (the state the stream starts from), installing their engine records
/// for the duration. Returns the game at the stream's end.
pub fn play(game: Game, entries: &[Entry], dt: f32) -> Result<Game, String> {
    hook::install_playback(entries);
    let mut p = Player::new(game, dt);
    let mut result = Ok(());
    for (i, e) in entries.iter().enumerate() {
        if let Err(err) = p.apply(e) {
            result = Err(format!("entry {i} ({:?}, {:?}): {err}", e.at, e.input));
            break;
        }
    }
    hook::take_playback();
    result?;
    Ok(p.finish())
}
