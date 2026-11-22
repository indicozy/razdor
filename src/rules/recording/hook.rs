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
    /// Per battle (its ordinal): whether its record shows the deployment (see [`Playback::new`]).
    deploys: BTreeMap<u32, bool>,
}

impl Playback {
    /// The records in `entries` (player actions are the driver's, not the rules'). A battle is
    /// deployed (waits for its first action, `Battle::start_play`) when its record shows that: a
    /// `Begin`, a rearrangement or a sit-out before the battle ends (a retreat is no deployment by itself). A record without
    /// any of them is a battle begun at its start, as the game began them before the deployment step
    /// (and as the difftests' and the tests' battles still do); its checks compare that state.
    pub fn new(entries: &[Entry]) -> Playback {
        use super::actions::{BattleAct, PlayerAction};
        let mut p = Playback::default();
        // The battle being read: its ordinal (from its patch record) and whether it is deployed.
        let mut open: Option<(Option<u32>, bool)> = None;
        for e in entries {
            match e.input {
                Input::Engine(EngineInput::GameStart { seed, patch, .. }) => p.starts.push_back((seed, patch)),
                Input::Engine(EngineInput::Music { pick }) => p.music.push_back((e.at, pick)),
                Input::Engine(EngineInput::Max { battle, decision, code }) => {
                    p.max.insert((battle, decision), code);
                }
                Input::Engine(EngineInput::Patch(rec)) => {
                    p.patch.insert(rec.battle, rec);
                    if let Some((ordinal @ None, _)) = open.as_mut() {
                        *ordinal = Some(rec.battle);
                    }
                }
                Input::Engine(EngineInput::Check { .. }) => {}
                Input::Player(PlayerAction::StartBattle) => {
                    if let Some((Some(n), deployed)) = open.take() {
                        p.deploys.insert(n, deployed);
                    }
                    open = Some((None, false));
                }
                Input::Player(PlayerAction::Battle(BattleAct::Begin | BattleAct::MoveCard { .. } | BattleAct::SitOut { .. })) => {
                    if let Some((_, deployed)) = open.as_mut() {
                        *deployed = true;
                    }
                }
                // A retreat undoes the battle: it is no deployment in itself (its recorded moves, if any, make one).
                Input::Player(PlayerAction::RetreatBattle) => {
                    if let Some((Some(n), deployed)) = open.take() {
                        p.deploys.insert(n, deployed);
                    }
                }
                Input::Player(PlayerAction::ResolveBattle) => {
                    if let Some((Some(n), deployed)) = open.take() {
                        p.deploys.insert(n, deployed);
                    }
                }
                Input::Player(_) => {}
            }
        }
        if let Some((Some(n), deployed)) = open {
            p.deploys.insert(n, deployed);
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
    /// How many read-only guards are alive (`read_only`): while any is, nothing is recorded.
    paused: u32,
    /// The `now` stamp and the keyframe flag when the first guard was taken, restored when the last one goes.
    saved: Option<(Stamp, bool)>,
    /// Whether the battles are captured for the telemetry's battle record (`arm_battle_tap`).
    tap_armed: bool,
    /// The battle being captured: a copy of its start and of the entries reported since, beside the recorder.
    tap: Option<Tap>,
}

/// A battle's capture (see [`arm_battle_tap`]).
struct Tap {
    start: BattleStart,
    entries: Vec<Entry>,
}

/// A battle's capture as the telemetry takes it ([`take_battle_tap`]): its start, then every entry
/// reported from the start to the battle's end.
pub struct CapturedBattle {
    pub start: BattleStart,
    pub entries: Vec<Entry>,
}

local! {
    static HOOK: std::cell::RefCell<Hook> = const { std::cell::RefCell::new(Hook { recorder: None, playback: None, now: Stamp { stretch: 0, ask: 0 }, actions: 0, engine: 0, keyframe: false, battle_starts: Vec::new(), paused: 0, saved: None, tap_armed: false, tap: None }) };
}

/// Attaches a recorder to this thread (replacing one).
pub fn install_recorder(r: Box<dyn Recorder>) {
    HOOK.with_borrow_mut(|h| h.recorder = Some(r));
}

/// Detaches this thread's recorder and returns it.
pub fn take_recorder() -> Option<Box<dyn Recorder>> {
    HOOK.with_borrow_mut(|h| h.recorder.take())
}

/// Whether something records this thread's game and recording is not paused (see [`read_only`]):
/// a recorder is attached, or the battles are captured ([`arm_battle_tap`]).
pub fn recording() -> bool {
    HOOK.with_borrow(|h| h.paused == 0 && (h.recorder.is_some() || h.tap_armed))
}

/// Arms (or disarms) the capture of the battles for the telemetry's battle record. The capture is
/// a copy taken beside the recorder: an attached recorder (the replay session) keeps its own entries,
/// and nothing the rules do changes.
pub fn arm_battle_tap(on: bool) {
    HOOK.with_borrow_mut(|h| {
        h.tap_armed = on;
        if !on {
            h.tap = None;
        }
    });
}

/// Whether a battle is being captured now (its start was reported and its end not yet taken).
pub fn battle_tap_open() -> bool {
    HOOK.with_borrow(|h| h.tap.is_some())
}

/// The battle being captured ends: its start and the entries reported since it began. None when no
/// battle was being captured.
pub fn take_battle_tap() -> Option<CapturedBattle> {
    HOOK.with_borrow_mut(|h| h.tap.take().map(|t| CapturedBattle { start: t.start, entries: t.entries }))
}

/// A read-only section: while the guard lives, whatever the rules do on this thread is not
/// recorded (a playback's actions and restores would otherwise be written into the live record
/// attached to the thread). Guards nest; the state the first one saved (`now` and the keyframe
/// flag) comes back when the last one goes. The guard is not `Send`: it belongs to the thread.
pub struct ReadOnly {
    _thread_bound: std::marker::PhantomData<*const ()>,
}

/// Starts a read-only section on this thread (see [`ReadOnly`]).
pub fn read_only() -> ReadOnly {
    HOOK.with_borrow_mut(|h| {
        if h.paused == 0 {
            h.saved = Some((h.now, h.keyframe));
        }
        h.paused += 1;
    });
    ReadOnly { _thread_bound: std::marker::PhantomData }
}

impl Drop for ReadOnly {
    fn drop(&mut self) {
        HOOK.with_borrow_mut(|h| {
            h.paused = h.paused.saturating_sub(1);
            if h.paused == 0 {
                if let Some((now, keyframe)) = h.saved.take() {
                    h.now = now;
                    h.keyframe = keyframe;
                }
            }
        });
    }
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
        if h.paused != 0 {
            return;
        }
        let entry = Entry { at, input };
        if let Some(t) = h.tap.as_mut() {
            t.entries.push(entry.clone());
        }
        if let Some(r) = h.recorder.as_mut() {
            r.record(entry);
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
    HOOK.with_borrow_mut(|h| {
        if h.paused == 0 {
            if h.tap_armed {
                h.tap = Some(Tap { start: start.clone(), entries: Vec::new() });
            }
            if h.recorder.is_some() {
                h.battle_starts.push(start);
            }
        }
    });
}

/// The battles begun since the last call, oldest first.
pub fn take_battle_starts() -> Vec<BattleStart> {
    HOOK.with_borrow_mut(|h| std::mem::take(&mut h.battle_starts))
}

/// Asks the recorder for a keyframe at its next pump (a battle started).
pub(crate) fn request_keyframe() {
    HOOK.with_borrow_mut(|h| h.keyframe |= h.paused == 0 && h.recorder.is_some());
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
        if h.paused != 0 {
            return;
        }
        let entry = Entry { at, input: Input::Engine(input) };
        if let Some(t) = h.tap.as_mut() {
            t.entries.push(entry.clone());
        }
        if let Some(r) = h.recorder.as_mut() {
            r.record(entry);
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

/// Whether battle `battle` waits for its first action in the playback (its record shows the
/// deployment, see [`Playback::new`]). True when no playback is attached or the record does not say.
pub(crate) fn playback_deploys(battle: u32) -> bool {
    HOOK.with_borrow(|h| h.playback.as_ref().and_then(|p| p.deploys.get(&battle).copied()).unwrap_or(true))
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

#[cfg(test)]
mod tap_tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    /// Counts what a recorder is handed.
    struct Count(Rc<RefCell<usize>>);

    impl Recorder for Count {
        fn record(&mut self, _entry: Entry) {
            *self.0.borrow_mut() += 1;
        }
    }

    fn start(ordinal: u32) -> BattleStart {
        serde_json::from_value(serde_json::json!({
            "ordinal": ordinal,
            "attacker": crate::rules::battle::Team::Player,
            "player": [],
            "enemy": [],
            "bench": [],
            "xp_correction": 0,
            "improved_ai": false,
            "max_ai": false,
            "max_guard": false,
            "god": false,
            "bare_enemy": false,
            "building_defence": [0, 0],
            "patch": patch_globals(),
        }))
        .unwrap()
    }

    fn check(hash: u64) -> EngineInput {
        EngineInput::Check { hash }
    }

    #[test]
    fn the_tap_copies_one_battle_from_its_start_and_leaves_the_recorder_its_own() {
        let seen = Rc::new(RefCell::new(0));
        install_recorder(Box::new(Count(seen.clone())));
        arm_battle_tap(true);
        assert!(recording());
        // Before a battle begins nothing is captured.
        record(check(1));
        assert!(!battle_tap_open());
        record_battle_start(start(1));
        assert!(battle_tap_open());
        record(check(2));
        record_input(None, Input::Engine(check(3)));
        // A read-only section records nothing, in the tap or the recorder.
        {
            let _guard = read_only();
            record(check(4));
        }
        let captured = take_battle_tap().expect("a battle was captured");
        assert_eq!(captured.start, start(1));
        assert_eq!(captured.entries.len(), 2);
        assert!(!battle_tap_open(), "the capture is taken once");
        // The recorder gets every entry (the one before the battle too); the battle start goes to its pump.
        assert_eq!(*seen.borrow(), 3);
        assert_eq!(take_battle_starts().len(), 1);
        take_recorder();
        arm_battle_tap(false);
        assert!(!recording());
    }

    #[test]
    fn nothing_is_captured_without_the_arm_and_a_new_start_replaces_an_open_capture() {
        arm_battle_tap(false);
        record_battle_start(start(1));
        assert!(!battle_tap_open());
        arm_battle_tap(true);
        record_battle_start(start(2));
        record(check(1));
        record_battle_start(start(3));
        let captured = take_battle_tap().unwrap();
        assert_eq!(captured.start.ordinal, 3);
        assert!(captured.entries.is_empty());
        arm_battle_tap(false);
    }
}
