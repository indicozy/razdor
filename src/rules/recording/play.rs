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

/// Whether a recorded battle act plays a computer turn itself: the hero's auto-step (`hero_auto_step`),
/// the rest of the battle by the hero's rules or by «Максимум», or the forced end. The screen records
/// these for the computer's turns it plays itself (`BattleView::ai_move`, a «Максимум» move it failed
/// to get), so such a record IS the computer's turn: the playback applies it and runs no AI for that
/// turn (`run_enemy`). A battle recorded without them (the tests' drivers play the enemy unrecorded)
/// keeps the AI's turns.
pub fn plays_turn(act: &BattleAct) -> bool {
    matches!(act, BattleAct::HeroAutoStep | BattleAct::HeroAutoPlay | BattleAct::MaxPlay | BattleAct::ForceEnd { .. })
}

/// The next recorded battle act after the entry before `entries`: the first `Battle` act, skipping the
/// other records (the checks, the «Максимум» moves, the music draws), and none when the battle's span
/// ends first (its `StartBattle`, `ResolveBattle` or `RetreatBattle`).
pub fn next_battle_act(entries: &[Entry]) -> Option<&BattleAct> {
    for e in entries {
        match &e.input {
            Input::Player(PlayerAction::StartBattle | PlayerAction::ResolveBattle | PlayerAction::RetreatBattle) => return None,
            Input::Player(PlayerAction::Battle(act)) => return Some(act),
            _ => {}
        }
    }
    None
}

/// Whether the entry takes the battle's next act as its hint (`Player::set_next_battle_act`): only a
/// battle's start and its acts run the computer's turns.
pub fn needs_next_act(e: &Entry) -> bool {
    matches!(e.input, Input::Player(PlayerAction::StartBattle | PlayerAction::Battle(_)))
}

/// Whether the playback holds a recorded «Максимум» move for the battle's decision now: the thinker
/// takes it (`MaxThinker::poll`) instead of searching, which a turn with no record would start.
pub fn recorded_max_move(b: &Battle) -> bool {
    hook::playback_max(b.ordinal, b.decisions).is_some()
}

/// How a battle the view played ends in the stream: its resolve, or its retreat.
#[derive(Clone, Copy)]
enum Ending {
    Resolve,
    Retreat,
}

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
    /// The battle act the record has after the entry about to be applied (`next_battle_act`, set by the
    /// caller with `set_next_battle_act`): a computer turn that act plays itself is that act's (`plays_turn`).
    next: Option<BattleAct>,
}

impl Player {
    pub fn new(game: Game, dt: f32) -> Player {
        Player { game, battle: None, music_pick: WORLD_THEME, dt, frames: 0, events: Vec::new(), next: None }
    }

    /// The battle act the record has after the entry the next `apply` / `apply_budget` applies (`None`
    /// when none). The caller sets it for a battle's start and its acts (`needs_next_act`); a retry of
    /// a budgeted entry sets it again, the same.
    pub fn set_next_battle_act(&mut self, next: Option<&BattleAct>) {
        self.next = next.cloned();
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
                let next = self.next.clone();
                self.run_enemy(next.as_ref())?;
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
                let next = self.next.clone();
                self.battle_act(act, next.as_ref())?;
                Ok(true)
            }
            Input::Player(PlayerAction::RetreatBattle) => {
                if !self.reach_stretch(e.at, left, true)? {
                    return Ok(false);
                }
                let (b, _) = self.battle.take().ok_or("a retreat, none under way")?;
                if !self.game.retreat_battle(&b) {
                    return Err("the retreat the recording made is not offered".into());
                }
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
        self.give_back(b, rest, Ending::Resolve)
    }

    /// As [`Player::return_battle`], for a battle the recording ends by a retreat («Отступить»):
    /// the battle is dropped and the hero goes back, as the stream's retreat does.
    pub fn return_retreat(&mut self, b: Battle, rest: &[BattleAct]) -> Result<(), String> {
        self.give_back(b, rest, Ending::Retreat)
    }

    fn give_back(&mut self, b: Battle, rest: &[BattleAct], ending: Ending) -> Result<(), String> {
        self.battle = Some((b, MaxThinker::new(Team::Enemy)));
        self.run_enemy(rest.first())?;
        for (i, act) in rest.iter().enumerate() {
            self.battle_act(act, rest.get(i + 1))?;
        }
        let (b, _) = self.battle.take().ok_or("a battle to end, none under way")?;
        match ending {
            Ending::Resolve => {
                if b.outcome() == Outcome::Ongoing {
                    return Err("the battle is not over at its recorded end".into());
                }
                self.game.resolve_battle(&b);
            }
            Ending::Retreat => {
                if !self.game.retreat_battle(&b) {
                    return Err("the retreat the recording made is not offered".into());
                }
            }
        }
        Ok(())
    }

    /// The music track the map's rotation is at (the last change the record made before the
    /// playback's place: `set_music_pick`, see `ReplayPlayer::start_at`).
    pub fn set_music_pick(&mut self, pick: usize) {
        self.music_pick = pick;
    }

    /// One recorded action of the player's battle, then the enemy's turns up to the next recorded
    /// act (`next`). A battle recorded before the deployment step has no `Begin`: its first action
    /// began it, after the enemy's turns that came before it (the old start, `run_enemy` first).
    fn battle_act(&mut self, act: &BattleAct, next: Option<&BattleAct>) -> Result<(), String> {
        // A deployment's own acts (the arrangement, the sit-outs) do not begin it.
        let legacy = self.battle.as_ref().is_some_and(|(b, _)| b.is_deploying() && !matches!(act, BattleAct::Begin | BattleAct::MoveCard { .. } | BattleAct::SitOut { .. }));
        if legacy {
            if let Some((b, _)) = self.battle.as_mut() {
                b.begin();
            }
            self.run_enemy(Some(act))?;
        }
        let (b, _) = self.battle.as_mut().ok_or("a battle's action, none under way")?;
        apply_battle(b, act);
        self.run_enemy(next)
    }

    /// The enemy's turns, as the screen plays them between the player's actions: «Максимум»'s
    /// recorded moves where the battle is set to it, the battle AI elsewhere. Stops at the
    /// player's turn or at the end.
    ///
    /// `next` is the battle act the record has after the one being applied. When it plays a computer
    /// turn itself (`plays_turn`: the screen's `hero_auto_step` for an enemy turn, a failed «Максимум»
    /// move, the quick battle), this turn is that act's: the function stops and the caller applies it,
    /// with no AI here, so the turn is not played twice. A «Максимум» move recorded for this turn comes
    /// first (`recorded_max_move`), as the screen plays it. Without such an act the AI plays as before
    /// (the drivers of the tests play the enemy unrecorded).
    fn run_enemy(&mut self, next: Option<&BattleAct>) -> Result<(), String> {
        let claimed = next.is_some_and(plays_turn);
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
            // Checked before any «Максимум» poll: a turn with no recorded move would start a search.
            if claimed && !(b.max_ai() && recorded_max_move(b)) {
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
                // The battle AI's step of the enemy: the same plan as `hero_auto_step` gives an enemy
                // (`plan(hero)` only changes the player's units), which the screen records for it.
                b.ai_step();
            }
        }
        Err("the battle does not end".into())
    }

    /// The battle as the playback starts it. A deployed battle (its record shows the deployment) is
    /// made, not begun: its `Begin` (or its first action) begins it, as on the screen, after the
    /// player's deployment. Any other is begun at its start, as the game began it before the
    /// deployment step (`hook::playback_deploys`).
    fn start_battle(&mut self) -> Battle {
        let mut b = self.game.start_battle();
        if !hook::playback_deploys(b.ordinal) {
            b.begin();
        }
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
        if needs_next_act(e) {
            p.set_next_battle_act(next_battle_act(&entries[i + 1..]));
        }
        if let Err(err) = p.apply(e) {
            result = Err(format!("entry {i} ({:?}, {:?}): {err}", e.at, e.input));
            break;
        }
    }
    hook::take_playback();
    result?;
    Ok(p.finish())
}
