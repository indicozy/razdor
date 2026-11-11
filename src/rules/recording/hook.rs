//! The engine's hooks: where the rules report what real time decided, and where a replay
//! supplies it again. The state is per thread (like the battle's patch globals), so a game
//! plays under one recorder or one playback at a time, and nothing is attached by default:
//! with neither installed every hook does what the rules did before (the clock seeds the event
//! generator, the battle AI searches, the patch globals start as they are).
//!
//! - The recorder ([`Recorder`]) is handed every [`Entry`] the rules produce.
//! - The playback ([`Playback`]) holds the records of an earlier game. The rules take the
//!   recorded event seed, the recorded «Максимум» move and the recorded patch globals instead
//!   of computing them. The music's draws are not taken from the records: the driver makes
//!   them when [`playback_music_due`] says so, and the draw itself is the rules' own.

use std::collections::{BTreeMap, VecDeque};

use super::super::battle::{patch_globals, set_patch_globals};
use super::super::rng::EventRng;
use super::battle_start::BattleStart;
use super::stream::{Entry, EngineInput, Input, MaxCode, PatchRecord, Stamp};
use crate::local;

/// Where a recorder puts what the rules report.
pub trait Recorder {
    fn record(&mut self, entry: Entry);
}

impl Recorder for Vec<Entry> {
    fn record(&mut self, entry: Entry) {
        self.push(entry);
    }
}

/// The records of an earlier game, in the order they were made.
#[derive(Default)]
pub struct Playback {
    starts: VecDeque<(u32, PatchRecord)>,
    music: VecDeque<(Stamp, u8)>,
    max: BTreeMap<(u32, u32), MaxCode>,
    patch: BTreeMap<u32, PatchRecord>,
}

impl Playback {
    /// The records in `entries` (player actions are the driver's, not the rules').
    pub fn new(entries: &[Entry]) -> Playback {
        let mut p = Playback::default();
        for e in entries {
            match e.input {
                Input::Engine(EngineInput::GameStart { seed, patch, .. }) => p.starts.push_back((seed, patch)),
                Input::Engine(EngineInput::Music { pick }) => p.music.push_back((e.at, pick)),
                Input::Engine(EngineInput::Max { battle, decision, code }) => {
                    p.max.insert((battle, decision), code);
                }
                Input::Engine(EngineInput::Patch(rec)) => {
                    p.patch.insert(rec.battle, rec);
                }
                Input::Engine(EngineInput::Check { .. }) => {}
                Input::Player(_) => {}
            }
        }
        p
    }
}

struct Hook {
    recorder: Option<Box<dyn Recorder>>,
    playback: Option<Playback>,
    /// Where the rules are now (the stamp of the entries they report).
    now: Stamp,
    /// How many player actions are on the stack (`actions::scope`): only the outermost records.
    actions: u32,
    /// How many engine scopes are on the stack (a tick, a script run, a restore): nothing they
    /// do is a player's action.
    engine: u32,
    /// A keyframe was asked for (a battle started), for the recorder's next pump.
    keyframe: bool,
    /// The starts of the battles begun since the recorder's last pump (the cut-outs' state).
    battle_starts: Vec<BattleStart>,
}

local! {
    static HOOK: std::cell::RefCell<Hook> = const { std::cell::RefCell::new(Hook { recorder: None, playback: None, now: Stamp { stretch: 0, ask: 0 }, actions: 0, engine: 0, keyframe: false, battle_starts: Vec::new() }) };
}

/// Attaches a recorder to this thread (replacing one).
pub fn install_recorder(r: Box<dyn Recorder>) {
    HOOK.with_borrow_mut(|h| h.recorder = Some(r));
}

/// Detaches this thread's recorder and returns it.
pub fn take_recorder() -> Option<Box<dyn Recorder>> {
    HOOK.with_borrow_mut(|h| h.recorder.take())
}

/// Whether a recorder is attached to this thread.
pub fn recording() -> bool {
    HOOK.with_borrow(|h| h.recorder.is_some())
}

/// Attaches a playback of `entries` to this thread (replacing one).
pub fn install_playback(entries: &[Entry]) {
    HOOK.with_borrow_mut(|h| h.playback = Some(Playback::new(entries)));
}

/// Detaches this thread's playback; whatever it did not use is dropped.
pub fn take_playback() -> Option<Playback> {
    HOOK.with_borrow_mut(|h| h.playback.take())
}

/// Whether a playback is attached to this thread.
pub fn playing() -> bool {
    HOOK.with_borrow(|h| h.playback.is_some())
}

/// Where the rules are now: the stamp of what they report next (the game sets it).
pub(crate) fn set_now(at: Stamp) {
    HOOK.with_borrow_mut(|h| h.now = at);
}

/// The stamp the rules are at.
pub fn now() -> Stamp {
    HOOK.with_borrow(|h| h.now)
}

/// Reports a player action or any other input at `at` (nothing without a recorder).
pub(crate) fn record_input(at: Option<Stamp>, input: Input) {
    HOOK.with_borrow_mut(|h| {
        let at = at.unwrap_or(h.now);
        if let Some(r) = h.recorder.as_mut() {
            r.record(Entry { at, input });
        }
    });
}

/// Enters a player action; true when it is the outermost one on the stack.
pub(crate) fn enter_action() -> bool {
    HOOK.with_borrow_mut(|h| {
        h.actions += 1;
        h.actions == 1
    })
}

/// Leaves a player action.
pub(crate) fn leave_action() {
    HOOK.with_borrow_mut(|h| h.actions = h.actions.saturating_sub(1));
}

/// Whether the rules run inside an engine scope (a tick, a script run, a restore).
pub(crate) fn in_engine() -> bool {
    HOOK.with_borrow(|h| h.engine > 0)
}

/// Enters an engine scope (undone by [`EngineScope`]'s drop).
pub(crate) fn engine_scope() -> EngineScope {
    HOOK.with_borrow_mut(|h| h.engine += 1);
    EngineScope { _private: () }
}

/// An engine scope: while it lives, nothing the rules do counts as a player's action.
pub(crate) struct EngineScope {
    _private: (),
}

impl Drop for EngineScope {
    fn drop(&mut self) {
        HOOK.with_borrow_mut(|h| h.engine = h.engine.saturating_sub(1));
    }
}

/// A battle began: its start state goes to the recorder at its next pump.
pub(crate) fn record_battle_start(start: BattleStart) {
    HOOK.with_borrow_mut(|h| h.battle_starts.push(start));
}

/// The battles begun since the last call, oldest first.
pub fn take_battle_starts() -> Vec<BattleStart> {
    HOOK.with_borrow_mut(|h| std::mem::take(&mut h.battle_starts))
}

/// Asks the recorder for a keyframe at its next pump (a battle started).
pub(crate) fn request_keyframe() {
    HOOK.with_borrow_mut(|h| h.keyframe = true);
}

/// Whether a keyframe was asked for since the last call (the recorder takes it).
pub fn take_keyframe_request() -> bool {
    HOOK.with_borrow_mut(|h| std::mem::take(&mut h.keyframe))
}

/// Reports an engine input at the current stamp (nothing without a recorder).
pub(crate) fn record(input: EngineInput) {
    record_at(None, input);
}

/// Reports an engine input at `at`, or at the current stamp (nothing without a recorder).
pub(crate) fn record_at(at: Option<Stamp>, input: EngineInput) {
    HOOK.with_borrow_mut(|h| {
        let at = at.unwrap_or(h.now);
        if let Some(r) = h.recorder.as_mut() {
            r.record(Entry { at, input: Input::Engine(input) });
        }
    });
}

/// A game starts, a new game (`load` false) or a load (`load` true): the event generator's
/// state and the patch globals it starts with. Without a playback, the clock seeds the generator
/// (the original's and Razdor's default) and the globals stay as they are. With one, its next
/// start record gives both. Either way the start is reported to a recorder. Opcode 18 draws from
/// the generator only.
pub(crate) fn game_start(load: bool) -> EventRng {
    let start = HOOK.with_borrow_mut(|h| h.playback.as_mut().and_then(|p| p.starts.pop_front()));
    let rng = match start {
        Some((seed, patch)) => {
            set_patch_globals(patch.globals());
            EventRng::new(seed)
        }
        None => EventRng::from_clock(),
    };
    if recording() {
        record(EngineInput::GameStart { load, seed: rng.state(), patch: PatchRecord::of(0, patch_globals()) });
    }
    rng
}

/// The recorded «Максимум» move of decision `decision` of battle `battle`, if a playback has it.
pub(crate) fn playback_max(battle: u32, decision: u32) -> Option<MaxCode> {
    HOOK.with_borrow(|h| h.playback.as_ref().and_then(|p| p.max.get(&(battle, decision)).copied()))
}

/// The recorded patch globals at the start of battle `battle`, if a playback has them.
pub(crate) fn playback_patch(battle: u32) -> Option<PatchRecord> {
    HOOK.with_borrow(|h| h.playback.as_ref().and_then(|p| p.patch.get(&battle).copied()))
}

/// The stamp of the next recorded music change, if a playback has one left.
pub fn playback_music_next() -> Option<Stamp> {
    HOOK.with_borrow(|h| h.playback.as_ref().and_then(|p| p.music.front().map(|m| m.0)))
}

/// The recorded pick of the music change at `at` when the playback has one there, taken off the
/// playback: the driver then makes the draw (`Game::music_rotate`) at that place.
pub fn playback_music_due(at: Stamp) -> Option<usize> {
    HOOK.with_borrow_mut(|h| {
        let p = h.playback.as_mut()?;
        match p.music.front() {
            Some(&(when, pick)) if when == at => {
                p.music.pop_front();
                Some(pick as usize)
            }
            _ => None,
        }
    })
}
