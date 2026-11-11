//! Replay phase 1: the snapshot continues a live game exactly, the state hash does not depend
//! on the frames or the process, recording changes nothing, and a recorded session plays back
//! from its records on another clock. The real-file tests skip without `RAZDOR_DT_DIR`.

use std::cell::RefCell;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use super::hook;
use super::snapshot::{state_hash, Snapshot};
use super::stream::{Entry, EngineInput, Input, PatchRecord, PlayerAction, Stamp};
use crate::dt::install::DtInstall;
use crate::rules::battle::{MaxPoll, MaxThinker, Outcome, Team};
use crate::rules::clock::MINUTES_PER_DAY;
use crate::rules::content::{Content, HeroClass};
use crate::rules::events::EventOutcome;
use crate::rules::game::{Event, Foe, Game};
use crate::rules::save::{self, SaveKind};

fn base() -> Option<DtInstall> {
    crate::dt::install::load_dotenv();
    let dir = std::env::var_os(crate::dt::install::ENV_VAR)?;
    DtInstall::load(Path::new(&dir)).ok()
}

fn map_named(dt: &DtInstall, prefix: &str) -> crate::dt::install::MapEntry {
    dt.maps.iter().find(|m| m.name.starts_with(prefix)).expect("the map").clone()
}

/// A new game on `map`, its origin set as a play's game is. With `seed`, the event generator
/// starts from it (through a playback, so the opening events draw from it too: the clock's seed
/// is nobody's to compare); with `None`, from whatever playback is attached.
/// The patch globals a process starts with (`battle::PatchGlobals::START`).
fn fresh_patch() -> PatchRecord {
    PatchRecord { battle: 0, hunger_counter: 3, hunger_seen: 3, screen_living: 0, side_blocks: [0, 0] }
}

fn start(c: &Arc<Content>, map: &crate::dt::install::MapEntry, seed: Option<u32>) -> Game {
    let s = map.load().unwrap();
    if let Some(seed) = seed {
        hook::install_playback(&[Entry { at: Stamp::default(), input: Input::Engine(EngineInput::GameStart { load: false, seed, patch: fresh_patch() }) }]);
    }
    let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
    if seed.is_some() {
        hook::take_playback();
    }
    let (origin, bytes) = save::ScenarioRef::of_map(&map.path, &map.name).unwrap();
    g.set_map(origin, bytes);
    g.drain_events();
    g
}

/// The frame pattern a driver plays: the frame's real time and how many arrivals of its step
/// the frame's budget lets through (the UI's 8 ms budget splits a step's AI moves this way).
struct Frames {
    dt: &'static [f32],
    budget: &'static [u32],
    at: usize,
}

impl Frames {
    fn new(dt: &'static [f32], budget: &'static [u32]) -> Frames {
        Frames { dt, budget, at: 0 }
    }

    fn next(&mut self) -> (f32, u32) {
        let k = self.at;
        self.at += 1;
        (self.dt[k % self.dt.len()], self.budget[k % self.budget.len()])
    }
}

/// Plays a game the way the screen does: frames, the events they raise, the battles, the
/// walks and waits. `music` draws the map music's changes at a wall clock that runs at the
/// frames' pace (live play); a playback instead draws them where its records put them.
struct Driver {
    g: Game,
    live: bool,
    music: bool,
    max: bool,
    wall: f64,
    due: f64,
    pick: usize,
}

const AUTO_BATTLE_STEPS: usize = 3000;

impl Driver {
    fn new(g: Game, live: bool) -> Driver {
        let first = crate::rules::music::WORLD_THEME;
        Driver { g, live, music: false, max: false, wall: 0.0, due: 0.0, pick: first }
    }

    /// One frame: the tick within its budget, then the music's due change (live) or the
    /// recorded changes (playback), and the events it raised.
    fn frame(&mut self, frames: &mut Frames) -> Vec<Event> {
        let (dt, budget) = frames.next();
        if !self.live {
            // Playback stops the frame at the ask its record names, so the change lands there.
            self.g.replay_stop = match hook::playback_music_next() {
                Some(t) if t.stretch == self.g.stretches && t.ask > self.g.asks => Some(t.ask),
                _ => None,
            };
        }
        let mut asked = 0u32;
        let events = self.g.tick_shown_within(dt, &mut || {
            asked += 1;
            asked < budget
        });
        self.g.replay_stop = None;
        if self.live {
            self.wall += dt as f64;
            if self.music && self.wall >= self.due {
                let (pick, ms) = self.g.music_rotate(self.pick);
                self.pick = pick;
                self.due = self.wall + ms as f64 / 1000.0;
            }
        } else {
            while let Some(pick) = hook::playback_music_due(self.g.stamp()) {
                let (next, _) = self.g.music_rotate(self.pick);
                assert_eq!(next, pick, "the draw gives the recorded track");
                self.pick = next;
            }
        }
        events
    }

    /// The events a frame raised, answered and read as the screen would.
    fn settle(&mut self, mut events: Vec<Event>) {
        for _ in 0..500 {
            let mut next = Vec::new();
            for e in events.drain(..) {
                if let Event::Script(EventOutcome::Question(_)) = e {
                    next.extend(self.g.answer_question(true));
                }
            }
            if next.is_empty() && self.g.script().is_some_and(|s| s.holds_window()) {
                next.extend(self.g.event_window_closed());
            }
            next.extend(self.g.drain_events());
            if next.is_empty() && self.g.foe.is_none() {
                break;
            }
            if self.g.foe.is_some() && self.g.pending_question().is_none() {
                self.battle();
            }
            events = next;
        }
    }

    /// The battle the game is set up for, played to its end: the enemy by «Максимум» when the
    /// game has it (its decisions come from the thinker, or a playback's records), the rest by
    /// the battle AI.
    fn battle(&mut self) -> Outcome {
        let mut b = self.g.start_battle();
        b.begin();
        let mut thinker = MaxThinker::new(Team::Enemy);
        for _ in 0..AUTO_BATTLE_STEPS {
            if b.outcome() != Outcome::Ongoing {
                break;
            }
            let Some(id) = b.active() else { break };
            if b.max_ai() && b.fighters[id].team == Team::Enemy {
                loop {
                    match thinker.poll(&b) {
                        MaxPoll::Ready(r) => {
                            b.apply_max(r);
                            break;
                        }
                        MaxPoll::Thinking => std::thread::sleep(Duration::from_millis(1)),
                        MaxPoll::Failed => {
                            b.auto_step();
                            break;
                        }
                    }
                }
            } else {
                b.auto_step();
            }
        }
        let outcome = b.outcome();
        self.g.resolve_battle(&b);
        outcome
    }

    /// Walks to the building nearest the hero that has a way on foot (the first in order of
    /// distance), and waits there until the walk ends. Nothing when none has one.
    fn walk_to_nearest(&mut self, frames: &mut Frames) {
        let here = self.g.tile();
        let mut near: Vec<_> = self.g.world.locations.iter().map(|l| l.tile).filter(|&t| t != here).collect();
        near.sort_by_key(|&t| (self.g.world.map.distance(t, here), t));
        if !near.into_iter().any(|t| self.g.set_destination(t)) {
            return;
        }
        for _ in 0..200_000 {
            let ev = self.frame(frames);
            self.settle(ev);
            if !self.g.moving() && self.g.foe.is_none() && !self.g.waiting() {
                self.g.finish_pause();
                let ev = self.g.tick_shown_within(0.0, &mut || true);
                if !ev.is_empty() || !self.g.held.is_empty() {
                    self.settle(ev);
                    continue;
                }
                if !self.g.moving() && self.g.foe.is_none() && !self.g.waiting() {
                    break;
                }
            }
        }
    }

    /// `days` game days: the hero waits 4 hours at a time, and fights whatever comes. The days
    /// end at rest (not waiting, not walking), whatever the frame times, so the state at a day's
    /// end is the same for every frame schedule.
    fn play_days(&mut self, frames: &mut Frames, days: u64) {
        let end = self.g.clock.total_minutes() + (days * MINUTES_PER_DAY) as f64;
        let mut calls = 0;
        loop {
            calls += 1;
            assert!(calls < 400_000, "the days do not end");
            if self.g.pending_question().is_some() {
                let next = self.g.answer_question(true);
                self.settle(next);
                continue;
            }
            if self.g.foe.is_some() {
                self.battle();
                continue;
            }
            if self.g.moving() || self.g.waiting() || !self.g.held.is_empty() {
                let ev = self.frame(frames);
                self.settle(ev);
                continue;
            }
            // At rest: the paused step (if any) finishes before the hero's next decision, and the
            // events it raised reach the screen first (a zero-time frame hands them over).
            self.g.finish_pause();
            let ev = self.g.tick_shown_within(0.0, &mut || true);
            if !ev.is_empty() || !self.g.held.is_empty() {
                self.settle(ev);
                continue;
            }
            if self.g.moving() || self.g.waiting() || self.g.foe.is_some() || self.g.pending_question().is_some() {
                continue;
            }
            if self.g.clock.total_minutes() >= end {
                break;
            }
            self.g.begin_wait(4);
        }
    }

    fn hash(&mut self) -> u64 {
        self.g.finish_pause();
        self.g.armies_snap();
        state_hash(&self.g)
    }
}

/// The state hashes of a session: its opening, a walk, then five days with the battles, at
/// the end of each day. `max` turns «Максимум» on (the first battle is fought against army 0
/// so that one happens).
fn session(d: &mut Driver, frames: &mut Frames, days: u64) -> Vec<u64> {
    let mut out = Vec::new();
    d.g.max_ai = d.max;
    let opening = d.g.drain_events();
    d.settle(opening);
    d.walk_to_nearest(frames);
    out.push(d.hash());
    if d.max {
        d.g.foe = Some(Foe::Army(0));
        d.battle();
    }
    for _ in 0..days {
        d.play_days(frames, 1);
        out.push(d.hash());
    }
    out
}

const DT_A: &[f32] = &[1.0 / 60.0, 1.0 / 30.0, 1.0 / 144.0, 1.0 / 20.0];
const BUDGET_A: &[u32] = &[3, 1, 5, 2];
const DT_B: &[f32] = &[1.0 / 30.0, 1.0 / 120.0, 1.0 / 45.0];
const BUDGET_B: &[u32] = &[2, 7, 1];

/// Install maps the tests play: the base's first campaign map and its second (ДС2).
const MAPS: &[&str] = &["РК1", "ДС2"];

#[test]
fn a_snapshot_continues_exactly_like_the_live_game() {
    let Some(dt) = base() else { return };
    let c = Arc::new(Content::from_dt(&dt));
    let install = crate::rules::save::Install { dir: &dt.dir, content: c.clone() };
    for prefix in MAPS {
        let map = map_named(&dt, prefix);
        let mut live = Driver::new(start(&c, &map, Some(0x1234_5678)), true);
        let mut frames = Frames::new(DT_A, BUDGET_A);
        live.g.max_ai = true;
        let opening = live.g.drain_events();
        live.settle(opening);
        live.walk_to_nearest(&mut frames);
        live.play_days(&mut frames, 2);
        live.g.finish_pause();
        let snap = Snapshot::take(&live.g).unwrap_or_else(|e| panic!("{prefix}: {e}"));
        let restored = snap.restore(c.clone(), Some(&install)).unwrap_or_else(|e| panic!("{prefix}: {e}"));
        assert_eq!(state_hash(&restored), state_hash(&live.g), "{prefix}: the restored state hashes as the live one");
        let mut copy = Driver::new(restored, false);
        let mut frames_live = Frames::new(DT_A, BUDGET_A);
        let mut frames_copy = Frames::new(DT_A, BUDGET_A);
        for day in 0..4 {
            live.play_days(&mut frames_live, 1);
            copy.play_days(&mut frames_copy, 1);
            assert_eq!(copy.hash(), live.hash(), "{prefix}: day {day} after the snapshot");
        }
        // The snapshot's own bytes round trip too.
        let again = Snapshot::from_bytes(snap.bytes().to_vec());
        assert_eq!(state_hash(&again.restore(c.clone(), Some(&install)).unwrap()), state_hash(&snap.restore(c.clone(), Some(&install)).unwrap()));
    }
}

/// A session of `days` days on `map` with frames of `dt` and budgets `budget` (cycled): each
/// day the hero walks to the building nearest him, every third day he fights an army (so there
/// are walks, waits and battles), and the hash is taken at each day's end.
fn long_session(dt: &DtInstall, c: &Arc<Content>, prefix: &str, dts: &'static [f32], budget: &'static [u32], days: u64) -> Vec<u64> {
    let map = map_named(dt, prefix);
    let mut d = Driver::new(start(c, &map, Some(7)), true);
    let mut f = Frames::new(dts, budget);
    let opening = d.g.drain_events();
    d.settle(opening);
    let armies = d.g.world.armies.len().max(1);
    let mut out = Vec::new();
    for day in 0..days {
        d.walk_to_nearest(&mut f);
        if day % 3 == 2 && d.g.foe.is_none() {
            d.g.foe = Some(Foe::Army(((day / 3) as usize) % armies));
            d.battle();
        }
        d.play_days(&mut f, 1);
        out.push(d.hash());
    }
    out
}

/// The frame schedules the tests run: whole frames at 2 to 240 fps, and split frames (a budget
/// of arrivals a frame may play: fixed, and pseudo-random cycles). A frame longer than a step
/// (0.15 s, below about 6.7 fps) plays several steps at once.
const SCHEDULE_DTS: &[&[f32]] = &[&[1.0 / 2.0], &[1.0 / 5.0], &[1.0 / 10.0], &[1.0 / 20.0], &[1.0 / 60.0], &[1.0 / 144.0], &[1.0 / 240.0], &[1.0 / 30.0, 1.0 / 7.0, 1.0 / 90.0]];
const SCHEDULE_BUDGETS: &[&[u32]] = &[&[u32::MAX], &[1], &[2], &[3], &[7], &[4, 1, 9, 2, 6, 1, 3, 8, 2, 5, 1, 7], &[1, 2, 1, 3, 1]];

/// Every frame schedule gives the reference's states, day by day, on РК1 and ДС2 for 30 days
/// with walks, waits and battles (hole 9: a split step; hole 10: several steps in one frame).
/// The reference is whole frames at 60 fps; every other schedule must match it at each day's end.
#[test]
fn every_frame_schedule_gives_the_same_states_day_by_day() {
    let Some(dt) = base() else { return };
    let c = Arc::new(Content::from_dt(&dt));
    const DAYS: u64 = 30;
    const WHOLE_60: &[f32] = &[1.0 / 60.0];
    const UNBUDGETED: &[u32] = &[u32::MAX];
    let mut failed = Vec::new();
    for prefix in MAPS {
        let reference = long_session(&dt, &c, prefix, WHOLE_60, UNBUDGETED, DAYS);
        let mut schedules: Vec<(&'static [f32], &'static [u32])> = Vec::new();
        for dts in SCHEDULE_DTS {
            schedules.push((dts, UNBUDGETED));
        }
        for budget in SCHEDULE_BUDGETS {
            schedules.push((WHOLE_60, budget));
            schedules.push((SCHEDULE_DTS[1], budget));
            schedules.push((SCHEDULE_DTS[7], budget));
        }
        for (dts, budget) in schedules {
            let got = long_session(&dt, &c, prefix, dts, budget, DAYS);
            if got != reference {
                let day = got.iter().zip(&reference).position(|(a, b)| a != b).unwrap_or(0);
                failed.push(format!("{prefix}: frames {dts:?} budget {budget:?} first differs at day {day}"));
            }
        }
    }
    assert!(failed.is_empty(), "{} schedules differ from the reference:\n{}", failed.len(), failed.join("\n"));
}

/// The child process's part of the cross-process test: it plays the session and prints it.
const CHILD_ENV: &str = "RAZDOR_REPLAY_TEST_CHILD";

fn session_report(dt: &DtInstall) -> Vec<u64> {
    let c = Arc::new(Content::from_dt(dt));
    let mut d = Driver::new(start(&c, &map_named(dt, "РК1"), Some(99)), true);
    let mut f = Frames::new(DT_A, BUDGET_A);
    session(&mut d, &mut f, 2)
}

#[test]
fn the_same_session_hashes_the_same_in_another_process() {
    let Some(dt) = base() else { return };
    if std::env::var_os(CHILD_ENV).is_some() {
        let text: Vec<String> = session_report(&dt).iter().map(|h| format!("{h:016x}")).collect();
        println!("REPLAY_HASHES {}", text.join(" "));
        return;
    }
    let mine: Vec<String> = session_report(&dt).iter().map(|h| format!("{h:016x}")).collect();
    let exe = std::env::current_exe().unwrap();
    let out = Command::new(exe)
        .args(["--exact", "rules::recording::tests::the_same_session_hashes_the_same_in_another_process", "--nocapture", "--test-threads=1"])
        .env(CHILD_ENV, "1")
        .env("RAZDOR_PERF_BUDGET_SCALE", "2")
        .output()
        .expect("the child process runs");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    // The harness writes its own «test …» text on the same line, so the marker is searched for.
    let at = text.find("REPLAY_HASHES ").unwrap_or_else(|| panic!("no hashes from the child:\n{text}"));
    let line: Vec<String> = text[at + "REPLAY_HASHES ".len()..].split_whitespace().take(mine.len()).map(str::to_string).collect();
    assert_eq!(line, mine);
}

/// The recorder's entries, shared with the test after the session.
#[derive(Clone, Default)]
struct Shared(Rc<RefCell<Vec<Entry>>>);

impl super::hook::Recorder for Shared {
    fn record(&mut self, entry: Entry) {
        self.0.borrow_mut().push(entry);
    }
}

fn normal_save_of(g: &Game) -> Vec<u8> {
    let mut meta = save::meta_of(g, SaveKind::Manual, "replay").unwrap();
    meta.saved_at = 0;
    save::encode_json(&meta, g).unwrap()
}

#[test]
fn recording_does_not_change_play() {
    let Some(dt) = base() else { return };
    let c = Arc::new(Content::from_dt(&dt));
    let map = map_named(&dt, "РК1");
    let seed = [Entry { at: Stamp::default(), input: Input::Engine(EngineInput::GameStart { load: false, seed: 4242, patch: fresh_patch() }) }];
    // The seed comes from the playback in both runs (the clock's would differ between them).
    hook::install_playback(&seed);
    let mut plain = Driver::new(start(&c, &map, None), true);
    let mut f = Frames::new(DT_A, BUDGET_A);
    let h_plain = session(&mut plain, &mut f, 2);
    let save_plain = normal_save_of(&plain.g);
    hook::take_playback();

    let shared = Shared::default();
    hook::install_playback(&seed);
    hook::install_recorder(Box::new(shared.clone()));
    let mut recorded = Driver::new(start(&c, &map, None), true);
    let mut f = Frames::new(DT_A, BUDGET_A);
    let h_rec = session(&mut recorded, &mut f, 2);
    let save_rec = normal_save_of(&recorded.g);
    hook::take_recorder();
    hook::take_playback();

    assert_eq!(h_rec, h_plain, "the same states with a recorder");
    assert_eq!(save_rec, save_plain, "the same normal save bytes");
    assert!(!shared.0.borrow().is_empty(), "and the recorder saw the patch globals of the battles");
}

#[test]
fn a_recorded_session_plays_back_from_its_records_on_another_clock() {
    let Some(dt) = base() else { return };
    let c = Arc::new(Content::from_dt(&dt));
    let map = map_named(&dt, "РК1");
    // Live: the music changes at a wall clock, «Максимум» on, the frames split by budgets.
    let shared = Shared::default();
    hook::install_recorder(Box::new(shared.clone()));
    let mut live = Driver::new(start(&c, &map, None), true);
    live.music = true;
    live.max = true;
    let mut f = Frames::new(DT_A, BUDGET_A);
    let h_live = session(&mut live, &mut f, 2);
    hook::take_recorder();
    let entries = shared.0.borrow().clone();
    let count = |want: fn(&Input) -> bool| entries.iter().filter(|e| want(&e.input)).count();
    assert_eq!(count(|i| matches!(i, Input::Engine(EngineInput::GameStart { .. }))), 1, "one new game, one start");
    assert!(count(|i| matches!(i, Input::Engine(EngineInput::Music { .. }))) >= 1, "the music changed");
    assert!(count(|i| matches!(i, Input::Engine(EngineInput::Max { .. }))) >= 1, "«Максимум» decided");
    assert!(count(|i| matches!(i, Input::Engine(EngineInput::Patch(_)))) >= 1, "the battles' patch globals");

    // Playback: the records give the seed, the music's places and «Максимум»'s moves; the
    // frames run on other times and budgets (a slower or faster clock).
    hook::install_playback(&entries);
    let mut back = Driver::new(start(&c, &map, None), false);
    back.max = true;
    let mut f = Frames::new(DT_B, BUDGET_B);
    let h_back = session(&mut back, &mut f, 2);
    assert!(hook::playback_music_next().is_none(), "every music change was made");
    hook::take_playback();
    assert_eq!(h_back, h_live, "the playback reproduces the live states");
}

#[test]
fn a_game_mid_step_has_no_snapshot() {
    let Some(dt) = base() else { return };
    let c = Arc::new(Content::from_dt(&dt));
    let map = map_named(&dt, "РК1");
    let mut d = Driver::new(start(&c, &map, Some(3)), true);
    let mut f = Frames::new(DT_A, &[1]);
    let opening = d.g.drain_events();
    d.settle(opening);
    let mut refused = false;
    for _ in 0..2000 {
        if d.g.ai_pending() {
            assert!(Snapshot::take(&d.g).is_err(), "a paused step is refused");
            refused = true;
            break;
        }
        let ev = d.frame(&mut f);
        d.settle(ev);
        if d.g.foe.is_none() && !d.g.moving() && !d.g.waiting() {
            d.g.begin_wait(4);
        }
    }
    assert!(refused, "a budget of one arrival pauses some step");
}

#[test]
fn the_engine_takes_its_seeds_from_a_playback_then_the_clock() {
    let entries = [
        Entry { at: Stamp::default(), input: Input::Engine(EngineInput::GameStart { load: false, seed: 11, patch: fresh_patch() }) },
        Entry { at: Stamp::default(), input: Input::Player(PlayerAction::Ok) },
        Entry { at: Stamp::default(), input: Input::Engine(EngineInput::GameStart { load: true, seed: 22, patch: fresh_patch() }) },
    ];
    hook::install_playback(&entries);
    assert_eq!(hook::game_start(false).state(), 11);
    assert_eq!(hook::game_start(true).state(), 22);
    assert_ne!(hook::game_start(true).state(), 0, "past the records: the clock");
    hook::take_playback();
}

/// Frame-by-frame: the first frame where a split run and an unsplit run differ (when neither
/// has a step paused), with the game's JSON around the first differing byte.
#[test]
fn the_state_hash_follows_the_rules() {
    let Some(dt) = base() else { return };
    let c = Arc::new(Content::from_dt(&dt));
    let mut a = Driver::new(start(&c, &map_named(&dt, "РК1"), Some(7)), true);
    let before = state_hash(&a.g);
    a.g.rng.random(7);
    assert_ne!(state_hash(&a.g), before);
}
