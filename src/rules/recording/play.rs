//! Playback of a recorded input stream: the game is taken to each recorded place in turn and the
//! recorded action is applied there by the entry point it came from (`actions::apply_battle`,
//! `actions::apply_game_events`). Between two places the rules run as the screen runs them: frames
//! of `dt` seconds (any frame times give the same states, see `docs/flows/replays.md`), the map's
//! music changes at the recorded places, and a battle's enemy plays its turns by «Максимум»'s
//! recorded moves or the battle AI. The caller installs the stream's engine records first
//! ([`super::hook::install_playback`]).
//!
//! Each step takes a budget of game time, so a screen can play a replay in real time: a step that
//! runs out of budget is taken up again by the next call, at the same place (the states do not
//! depend on how the time is split).

use super::actions::{apply_battle, apply_game_events, BattleAct};
use super::hook;
use super::stream::{Entry, EngineInput, Input, PlayerAction, Stamp};
use crate::rules::battle::{Battle, MaxPoll, MaxThinker, Outcome, Team};
use crate::rules::game::{Event, Game};
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
    /// The events the frames and the recorded actions raised, taken by the screen (`take_events`).
    events: Vec<Event>,
}

impl Player {
    pub fn new(game: Game, dt: f32) -> Player {
        Player { game, battle: None, music_pick: WORLD_THEME, dt, frames: 0, events: Vec::new() }
    }

    /// The game at its recorded place: time runs (frames of `dt`) until the game stands at the end
    /// of the recorded stretch (`target.stretch`), with the events the screen holds for its glide
    /// delivered (the screen decides only then, as the driver does). An action or a check is
    /// recorded after its stretch's paused step is finished (`actions::scope`), so the place is the
    /// stretch's end: the arrivals taken inside it are not compared, as a frame's budget only
    /// changes how the stretch is split. A music change is the exception: it falls at its exact
    /// arrival (the ask), and the time stops there to draw it.
    pub fn advance_to(&mut self, target: Stamp) -> Result<(), String> {
        let mut left = f32::INFINITY;
        self.reach_stretch(target, &mut left, true).map(|_| ())
    }

    /// The game at a recorded arrival: time runs until the game has taken exactly `target.ask`
    /// arrivals in the recorded stretch. A music change falls there, mid-step, as it did live
    /// (`Game::music_rotate` records its stamp), so the time stops at the arrival and the change is
    /// drawn by the caller.
    pub fn advance_exact(&mut self, target: Stamp) -> Result<(), String> {
        let mut left = f32::INFINITY;
        self.reach_exact(target, &mut left).map(|_| ())
    }

    /// As [`Player::advance_to`], with `left` game seconds at most: `Ok(true)` when the game stands
    /// at `target`, `Ok(false)` when the budget ran out first.
    /// `contacts`: a zero-time contact the walk reaches at the place is played before it (an
    /// action does so, as the live hero's next frame does). A check is a look at the state where
    /// it was taken, and the live driver took it before that contact, so a check does not play it.
    fn reach_stretch(&mut self, target: Stamp, left: &mut f32, contacts: bool) -> Result<bool, String> {
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
                    if !contacts || !self.game.zero_time_next() {
                        return Ok(true);
                    }
                }
            }
            if *left <= 0.0 {
                return Ok(false);
            }
            self.tick_toward(target.stretch, None)?;
            *left -= self.dt;
        }
    }

    /// As [`Player::advance_exact`], with `left` game seconds at most.
    fn reach_exact(&mut self, target: Stamp, left: &mut f32) -> Result<bool, String> {
        loop {
            if self.game.stamp() == target && self.game.held.is_empty() {
                return Ok(true);
            }
            if self.game.stretches == target.stretch && self.game.asks > target.ask {
                return Err(format!("the game passed the recorded place {target:?} (now {:?})", self.game.stamp()));
            }
            if *left <= 0.0 {
                return Ok(false);
            }
            let ask = (self.game.stretches == target.stretch && target.ask > self.game.asks).then_some(target.ask);
            self.tick_toward(target.stretch, ask)?;
            *left -= self.dt;
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
        let raised = self.game.tick_shown_within(self.dt, &mut || true);
        self.events.extend(raised);
        self.game.replay_stop = None;
        self.game.replay_stop_before = None;
        Ok(())
    }

    /// Applies one recorded entry (an engine record is consumed by the hooks, or is a check).
    pub fn apply(&mut self, e: &Entry) -> Result<(), String> {
        let mut left = f32::INFINITY;
        self.apply_budget(e, &mut left).map(|_| ())
    }

    /// Applies `e` with `left` game seconds at most: `Ok(true)` when it was applied, `Ok(false)` when
    /// the budget ran out before its place (the same entry is taken up by the next call).
    pub fn apply_budget(&mut self, e: &Entry, left: &mut f32) -> Result<bool, String> {
        match &e.input {
            Input::Engine(EngineInput::Check { hash }) => {
                if !self.reach_stretch(e.at, left, false)? {
                    return Ok(false);
                }
                self.game.finish_pause();
                self.game.armies_snap();
                if let Some((b, _)) = self.battle.as_ref() {
                    self.game.battle_write_back(b);
                }
                let got = super::snapshot::state_hash(&self.game);
                if got != *hash {
                    return Err(format!("drift at {:?}: the state hashes {got:016x}, recorded {hash:016x}", e.at));
                }
                Ok(true)
            }
            Input::Engine(EngineInput::Music { pick }) => {
                // The change is drawn at its place in the stream (after the entries before it).
                if !self.reach_exact(e.at, left)? {
                    return Ok(false);
                }
                let (next, _) = self.game.music_rotate(self.music_pick);
                if next != *pick as usize {
                    return Err(format!("the music track is {next}, recorded {pick}"));
                }
                self.music_pick = next;
                Ok(true)
            }
            Input::Engine(_) => Ok(true),
            Input::Player(PlayerAction::StartBattle) => {
                if !self.reach_stretch(e.at, left, true)? {
                    return Ok(false);
                }
                let b = self.start_battle();
                self.battle = Some((b, MaxThinker::new(Team::Enemy)));
                self.run_enemy()?;
                Ok(true)
            }
            Input::Player(PlayerAction::ResolveBattle) => {
                if !self.reach_stretch(e.at, left, true)? {
                    return Ok(false);
                }
                let (b, _) = self.battle.take().ok_or("a battle to resolve, none under way")?;
                self.game.resolve_battle(&b);
                Ok(true)
            }
            Input::Player(PlayerAction::Battle(act)) => {
                let (b, _) = self.battle.as_mut().ok_or("a battle's action, none under way")?;
                apply_battle(b, act);
                self.run_enemy()?;
                Ok(true)
            }
            Input::Player(a) => {
                if !self.reach_stretch(e.at, left, true)? {
                    return Ok(false);
                }
                let raised = apply_game_events(&mut self.game, a);
                self.events.extend(raised);
                Ok(true)
            }
        }
    }

    /// A battle's start at the entry's place: the battle is made and begun as the playback makes
    /// it, with no enemy turn played, for the caller's view to play. `left` as in `apply_budget`.
    pub fn start_battle_for_view(&mut self, e: &Entry, left: &mut f32) -> Result<Option<Battle>, String> {
        if !self.reach_stretch(e.at, left, true)? {
            return Ok(None);
        }
        Ok(Some(self.start_battle()))
    }

    /// The battle made by [`Player::start_battle_for_view`] comes back to the playback, from the
    /// view's state: the actions it did not play (`rest`, when the battle was skipped) are played
    /// from there as the stream plays them, then the battle is resolved.
    pub fn return_battle(&mut self, b: Battle, rest: &[BattleAct]) -> Result<(), String> {
        self.battle = Some((b, MaxThinker::new(Team::Enemy)));
        self.run_enemy()?;
        for act in rest {
            let (b, _) = self.battle.as_mut().ok_or("a battle to play on, none under way")?;
            apply_battle(b, act);
            self.run_enemy()?;
        }
        let (b, _) = self.battle.take().ok_or("a battle to resolve, none under way")?;
        if b.outcome() == Outcome::Ongoing {
            return Err("the battle is not over at its recorded end".into());
        }
        self.game.resolve_battle(&b);
        Ok(())
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

    /// The battle as the playback starts it: made and begun (the battle's own start).
    fn start_battle(&mut self) -> Battle {
        let mut b = self.game.start_battle();
        b.begin();
        b
    }

    /// The events raised since the last call (the screen's windows and messages).
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
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
