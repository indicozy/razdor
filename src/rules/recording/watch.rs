//! The replay's player, without a window: it opens a `.rzreplay`, starts each segment from its
//! snapshot, plays its records (`play::Player`), checks every checkpoint, and follows the
//! segments across the loads. It seeks to a day through the nearest keyframe before it, and it
//! builds the timeline's marks from the file (the days, the battles' starts, the towns taken, the
//! hero's fall and the victory: the last three at keyframe precision, as the keyframes are the
//! only states the file holds). A checkpoint that does not match stops the playback at the last
//! good place, with a [`Drift`] that names the version which recorded the replay.
//! Design: `docs/superpowers/specs/2026-11-11-replays-design.md`; the flow: `docs/flows/replays.md`.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::actions::BattleAct;
use super::battle_start::BattleStart;
use super::file::{read, ReplayFile, Segment};
use super::header::ReplayHeader;
use super::hook;
use super::play::Player;
use super::snapshot::{state_hash, Snapshot};
use super::stream::{Entry, EngineInput, Input, PlayerAction};
use crate::rules::battle::{Battle, Outcome, Team};
use crate::rules::content::Content;
use crate::rules::game::{Event, Game};
use crate::rules::save::Install;

/// The game time of one playback frame (the states do not depend on it, see `play`).
const FRAME_DT: f32 = 1.0 / 30.0;

/// What a [`ReplayPlayer::pump`] stopped at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pump {
    /// The budget is spent, or a seek is under way and reached its place: call again.
    Playing,
    /// A battle begins at the place: the caller takes it (`take_battle`), plays its actions
    /// (`next_battle_act`) and gives it back (`return_battle`).
    Battle,
    /// The file is played to its end.
    Ended,
}

/// What one budgeted entry step came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    /// An entry was applied.
    Applied,
    /// The budget ran out before the entry's place.
    Budget,
    /// A battle begins and is handed to the view.
    Battle,
    /// The file has no entries left.
    End,
}

/// A seek under way: the pumps play on until the game is in this day or at this place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Seek {
    Day(u64),
    Place(Place),
}

/// A place in the file: a segment and how many of its entries come before it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Place {
    pub seg: usize,
    pub index: usize,
}

/// What a timeline mark is.
#[derive(Clone, Debug, PartialEq)]
pub enum MarkKind {
    /// A checkpoint's day (a segment's start, or a keyframe every 10 game days).
    Day,
    /// A battle begins: the attacker, and the unit names on each side.
    Battle { attacker: Team, player: Vec<String>, enemy: Vec<String> },
    /// A town (or other building) became the player's, at the keyframe that first shows it.
    Town { name: String },
    /// The hero fell.
    HeroDown,
    /// The campaign's victory (its camps are cleared).
    Victory,
}

/// A timeline mark: its day, what it is, and the place to seek to.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    pub day: u64,
    pub kind: MarkKind,
    pub at: Place,
}

/// A checkpoint that does not match: the playback stopped there. `version` is the Razdor that
/// recorded the replay (the message «Запись сделана в версии {version} и дальше расходится»).
#[derive(Clone, Debug, PartialEq)]
pub struct Drift {
    pub day: u64,
    pub version: String,
    /// The place of the checkpoint that failed.
    pub at: Place,
    pub reason: String,
}

/// The player of one replay file.
pub struct ReplayPlayer {
    pub header: ReplayHeader,
    segments: Vec<Segment>,
    content: Arc<Content>,
    install: Option<PathBuf>,
    /// The timeline's marks, in file order.
    pub marks: Vec<Mark>,
    /// The game being played (none before a start, or after the file's end).
    player: Option<Player>,
    /// The segment and entry the playback is at (the next entry to play).
    seg: usize,
    idx: usize,
    /// The place of the last entry played without a mismatch.
    last_good: Place,
    /// Set once a checkpoint did not match.
    pub drift: Option<Drift>,
    /// The seek under way (`begin_seek_day`, `begin_seek_place`), if any.
    seek: Option<Seek>,
    /// A battle the pump reached and handed out (`take_battle`), not yet taken.
    pending_battle: Option<Battle>,
}

impl ReplayPlayer {
    /// Opens the replay file at `path`. `install` is the game's install folder: a map's snapshots
    /// read the map from it (the demo needs none).
    pub fn open(path: &Path, content: Arc<Content>, install: Option<&Path>) -> Result<ReplayPlayer, String> {
        Ok(ReplayPlayer::new(read(path)?, content, install))
    }

    /// The player of a file already read (the tests change a file before playing it).
    pub fn new(file: ReplayFile, content: Arc<Content>, install: Option<&Path>) -> ReplayPlayer {
        let mut p = ReplayPlayer {
            header: file.header,
            segments: file.segments,
            content,
            install: install.map(Path::to_path_buf),
            marks: Vec::new(),
            player: None,
            seg: 0,
            idx: 0,
            last_good: Place { seg: 0, index: 0 },
            drift: None,
            seek: None,
            pending_battle: None,
        };
        p.marks = p.build_marks();
        p
    }

    /// The game at the playback's place (none before the first seek or advance).
    pub fn game(&self) -> Option<&Game> {
        self.player.as_ref().map(|p| &p.game)
    }

    /// The place the playback is at: the next entry to play.
    pub fn place(&self) -> Place {
        Place { seg: self.seg, index: self.idx }
    }

    /// The place of the last entry played without a mismatch (where a drift stops the game).
    pub fn last_good(&self) -> Place {
        self.last_good
    }

    /// Whether the file is played to its end.
    pub fn ended(&self) -> bool {
        self.seg >= self.segments.len()
    }

    /// The file's segments, for the tests and the marks.
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// Plays the next entry. `Ok(true)` when one was played, `Ok(false)` at the file's end. A
    /// checkpoint that does not match stops the playback (`drift`) and gives its [`Drift`].
    pub fn advance(&mut self) -> Result<bool, Drift> {
        self.arm();
        let r = self.step_all();
        hook::take_playback();
        r
    }

    /// The game's day of the file's first checkpoint (the clock's day at the start of the replay):
    /// a day counted from the start is the game's day minus this.
    pub fn first_day(&self) -> u64 {
        self.segments.first().map_or(0, |s| s.start_day)
    }

    /// Installs the playback of the current segment for the calls that follow (the hook is per
    /// thread, and each public call takes it away again, so the replays of other players never meet).
    fn arm(&self) {
        if self.player.is_some() && self.seg < self.segments.len() {
            hook::install_playback(&self.segments[self.seg].entries);
        }
    }

    /// Plays to the file's end (the whole replay), or to its first drift.
    pub fn play_to_end(&mut self) -> Result<(), Drift> {
        while self.advance()? {}
        Ok(())
    }

    /// Goes to the state of the latest checkpoint at or before `day` (the nearest keyframe, or a
    /// segment's start), then plays on until the game is in `day` or later. Returns the place it
    /// stops at (the file's end when `day` is past it).
    pub fn seek_day(&mut self, day: u64) -> Result<Place, Drift> {
        let r = self.seek_day_inner(day);
        hook::take_playback();
        r
    }

    /// Goes to `target` (a place a keyframe or an entry of the file names): from the latest
    /// checkpoint before it, played on to it.
    pub fn seek_place(&mut self, target: Place) -> Result<(), Drift> {
        let r = self.seek_place_inner(target);
        hook::take_playback();
        r
    }

    fn seek_day_inner(&mut self, day: u64) -> Result<Place, Drift> {
        let (at, bytes) = self.checkpoint_before_day(day).ok_or_else(|| self.fail(Place { seg: 0, index: 0 }, "no checkpoint".into()))?;
        self.start_at(at, &bytes)?;
        loop {
            if self.player.as_ref().is_some_and(|p| p.game.clock.day_index() >= day) {
                return Ok(self.place());
            }
            if !self.step_all()? {
                return Ok(self.place());
            }
        }
    }

    fn seek_place_inner(&mut self, target: Place) -> Result<(), Drift> {
        let (at, bytes) = self.checkpoint_before(target).ok_or_else(|| self.fail(target, "no checkpoint before the place".into()))?;
        self.start_at(at, &bytes)?;
        while self.place() < target {
            if !self.step_all()? {
                break;
            }
        }
        Ok(())
    }

    /// The last checkpoint (a segment's start or a keyframe) whose day is `day` or earlier.
    fn checkpoint_before_day(&self, day: u64) -> Option<(Place, Vec<u8>)> {
        let mut best = None;
        for (s, seg) in self.segments.iter().enumerate() {
            if seg.start_day <= day {
                best = Some((Place { seg: s, index: 0 }, seg.snapshot.clone()));
            }
            for k in seg.keyframes.iter().filter(|k| k.day <= day) {
                best = Some((Place { seg: s, index: k.after }, k.snapshot.clone()));
            }
        }
        best
    }

    /// The last checkpoint at or before `target` in the file's order.
    fn checkpoint_before(&self, target: Place) -> Option<(Place, Vec<u8>)> {
        let mut best = None;
        for (s, seg) in self.segments.iter().enumerate().take(target.seg + 1) {
            let start = Place { seg: s, index: 0 };
            if start <= target {
                best = Some((start, seg.snapshot.clone()));
            }
            for k in &seg.keyframes {
                let at = Place { seg: s, index: k.after };
                if at <= target {
                    best = Some((at, k.snapshot.clone()));
                }
            }
        }
        best
    }

    /// Restores the game of a checkpoint at `at` and starts playing from there.
    fn start_at(&mut self, at: Place, bytes: &[u8]) -> Result<(), Drift> {
        let game = self.restore(bytes).map_err(|e| self.fail(at, e))?;
        hook::install_playback(&self.segments[at.seg].entries);
        self.player = Some(Player::new(game, FRAME_DT));
        self.seg = at.seg;
        self.idx = at.index;
        self.last_good = at;
        self.drift = None;
        Ok(())
    }

    /// One budgeted step of the playback: the next entry, with `left` game seconds at most. A
    /// battle's start is handed out instead of played when `watch` is set.
    fn step(&mut self, left: &mut f32, watch: bool) -> Result<Step, Drift> {
        if let Some(d) = self.drift.clone() {
            return Err(d);
        }
        loop {
            if self.seg >= self.segments.len() {
                return Ok(Step::End);
            }
            if self.player.is_none() {
                let at = Place { seg: self.seg, index: 0 };
                let bytes = self.segments[self.seg].snapshot.clone();
                let start_hash = self.segments[self.seg].start_hash;
                self.start_at(at, &bytes)?;
                let got = state_hash(&self.player.as_ref().expect("a started game").game);
                if got != start_hash {
                    return Err(self.fail(at, format!("the segment's start restores to {got:016x}, recorded {start_hash:016x}")));
                }
            }
            if self.idx >= self.segments[self.seg].entries.len() {
                if self.seg + 1 == self.segments.len() {
                    // The file's last segment is played out: its game stays (the screen shows the end).
                    self.seg += 1;
                    return Ok(Step::End);
                }
                // A segment's end: the next segment starts from its own state (a load).
                self.player = None;
                self.seg += 1;
                hook::take_playback();
                continue;
            }
            let here = self.place();
            let entry = self.segments[self.seg].entries[self.idx].clone();
            let player = self.player.as_mut().expect("a started game");
            if watch && matches!(entry.input, Input::Player(PlayerAction::StartBattle)) {
                return match player.start_battle_for_view(&entry, left) {
                    Ok(Some(b)) => {
                        self.pending_battle = Some(b);
                        self.idx += 1;
                        self.last_good = self.place();
                        Ok(Step::Battle)
                    }
                    Ok(None) => Ok(Step::Budget),
                    Err(reason) => Err(self.stop_at(here, reason)),
                };
            }
            match player.apply_budget(&entry, left) {
                Ok(true) => {
                    self.idx += 1;
                    self.last_good = self.place();
                    return Ok(Step::Applied);
                }
                Ok(false) => return Ok(Step::Budget),
                Err(reason) => return Err(self.stop_at(here, reason)),
            }
        }
    }

    /// The drift at `at` (the playback stops there, see [`ReplayPlayer::advance`]).
    fn stop_at(&mut self, at: Place, reason: String) -> Drift {
        let d = self.fail(at, reason);
        self.drift = Some(d.clone());
        d
    }

    /// Plays one step with no budget: a checkpoint that does not match is a drift.
    fn step_all(&mut self) -> Result<bool, Drift> {
        let mut left = f32::INFINITY;
        self.arm();
        let r = self.step(&mut left, false);
        hook::take_playback();
        match r? {
            Step::Applied => Ok(true),
            Step::End => Ok(false),
            Step::Budget | Step::Battle => unreachable!("an unbudgeted step plays its entry"),
        }
    }

    /// Plays the file on in real time: up to `budget` game seconds (the screen's frame times
    /// times its speed), stopping at the budget's end, at a battle's start (when `watch_battles`
    /// and no seek is under way: the battle is handed out, see [`ReplayPlayer::take_battle`]), or
    /// at the file's end. A seek under way ([`ReplayPlayer::begin_seek_day`]) is played on the same
    /// way and ends when its place is reached. A drift stops it with `Err`.
    pub fn pump(&mut self, budget: f32, watch_battles: bool) -> Result<Pump, Drift> {
        let mut left = budget;
        loop {
            if let Some(seek) = self.seek {
                if self.seek_reached(seek) {
                    self.seek = None;
                    return Ok(Pump::Playing);
                }
            }
            if left <= 0.0 {
                return Ok(Pump::Playing);
            }
            self.arm();
            let watch = watch_battles && self.seek.is_none();
            let r = self.step(&mut left, watch);
            hook::take_playback();
            match r? {
                Step::Applied => {}
                Step::Budget => return Ok(Pump::Playing),
                Step::Battle => return Ok(Pump::Battle),
                Step::End => {
                    self.seek = None;
                    return Ok(Pump::Ended);
                }
            }
        }
    }

    /// Whether a seek has reached its day or its place.
    fn seek_reached(&self, seek: Seek) -> bool {
        match seek {
            Seek::Day(day) => self.player.as_ref().is_some_and(|p| p.game.clock.day_index() >= day),
            Seek::Place(place) => self.place() >= place,
        }
    }

    /// Starts a seek to `day` on the timeline: the game is restored from the checkpoint before it
    /// (nothing plays yet), and [`ReplayPlayer::pump`] plays on to the day.
    pub fn begin_seek_day(&mut self, day: u64) -> Result<(), Drift> {
        let (at, bytes) = self.checkpoint_before_day(day).ok_or_else(|| self.fail(Place { seg: 0, index: 0 }, "no checkpoint".into()))?;
        self.begin_seek(at, &bytes, Seek::Day(day))
    }

    /// Starts a seek to a place of the file (a mark's place): see [`ReplayPlayer::begin_seek_day`].
    pub fn begin_seek_place(&mut self, target: Place) -> Result<(), Drift> {
        let (at, bytes) = self.checkpoint_before(target).ok_or_else(|| self.fail(target, "no checkpoint before the place".into()))?;
        self.begin_seek(at, &bytes, Seek::Place(target))
    }

    fn begin_seek(&mut self, at: Place, bytes: &[u8], seek: Seek) -> Result<(), Drift> {
        let r = self.start_at(at, bytes);
        hook::take_playback();
        r?;
        self.seek = Some(seek);
        Ok(())
    }

    /// Whether a seek is under way (the screen shows «Перемотка…»).
    pub fn seeking(&self) -> bool {
        self.seek.is_some()
    }

    /// The game as the playback stands (none before the first seek or pump).
    pub fn game_mut(&mut self) -> Option<&mut Game> {
        self.player.as_mut().map(|p| &mut p.game)
    }

    /// Runs `f` on the game with the file's records installed for the call (a battle's view plays
    /// its own enemy turns, and its «Максимум» moves come from the records). None before a start.
    pub fn in_playback<R>(&mut self, f: impl FnOnce(&mut Game) -> R) -> Option<R> {
        self.arm();
        let r = self.player.as_mut().map(|p| f(&mut p.game));
        hook::take_playback();
        r
    }

    /// The events the playback raised since the last call (windows, messages, the day's report).
    pub fn take_events(&mut self) -> Vec<Event> {
        self.player.as_mut().map(Player::take_events).unwrap_or_default()
    }

    /// The battle a pump handed out (`Pump::Battle`), once. The view plays it: the battle is
    /// begun by the view, and its actions come from [`ReplayPlayer::next_battle_act`].
    pub fn take_battle(&mut self) -> Option<Battle> {
        self.pending_battle.take()
    }

    /// The next recorded action of the battle under way, when the entry at the place is one (the
    /// view applies it on the player's turn). None at the battle's resolve or the file's end.
    pub fn next_battle_act(&mut self) -> Option<BattleAct> {
        let (i, e) = self.battle_entry()?;
        let Input::Player(PlayerAction::Battle(act)) = &e.input else { return None };
        let act = act.clone();
        self.idx = i + 1;
        self.last_good = self.place();
        Some(act)
    }

    /// The first entry at or after the place that is not one of the battle's own engine records
    /// (the «Максимум» moves and the patch globals: the playback finds those by their key): its
    /// index and the entry.
    fn battle_entry(&self) -> Option<(usize, &Entry)> {
        let entries = &self.segments.get(self.seg)?.entries;
        let i = (self.idx..entries.len()).find(|&i| !matches!(entries[i].input, Input::Engine(EngineInput::Max { .. } | EngineInput::Patch(_))))?;
        Some((i, &entries[i]))
    }

    /// Whether the battle under way has no more actions: its resolve is the next entry.
    pub fn battle_resolves_next(&self) -> bool {
        self.battle_entry().is_some_and(|(_, e)| matches!(e.input, Input::Player(PlayerAction::ResolveBattle)))
    }

    /// Gives a battle back: `b` is the view's battle, to the end of its actions or stopped
    /// (skipped) at any point. At the battle's resolve it is resolved here; otherwise the rest of
    /// the battle is played from its state at once, as the stream plays it. A battle that ended
    /// before its recorded actions did is a drift.
    pub fn return_battle(&mut self, b: Battle) -> Result<(), Drift> {
        let at_resolve = self.battle_resolves_next();
        let resolve_at = self.battle_entry().map(|(i, _)| i);
        let here = self.place();
        if !at_resolve && b.outcome() != Outcome::Ongoing {
            return Err(self.stop_at(here, "the battle ended before its recorded actions".into()));
        }
        self.arm();
        let r = match self.player.as_mut() {
            Some(p) => p.return_battle(b, at_resolve),
            None => Err("no game to give the battle back to".into()),
        };
        hook::take_playback();
        r.map_err(|reason| self.stop_at(here, reason))?;
        if let (true, Some(i)) = (at_resolve, resolve_at) {
            self.idx = i + 1;
            self.last_good = self.place();
        }
        Ok(())
    }

    /// The last day the marks show (the file's end on the timeline), or the first day when none.
    pub fn last_day(&self) -> u64 {
        self.marks.iter().map(|m| m.day).max().unwrap_or_else(|| self.first_day())
    }

    fn fail(&self, at: Place, reason: String) -> Drift {
        let day = self.player.as_ref().map_or(0, |p| p.game.clock.day_index());
        Drift { day, version: self.header.razdor.clone(), at, reason }
    }

    /// A checkpoint's game, restored from its packed snapshot (a map from the install).
    fn restore(&self, bytes: &[u8]) -> Result<Game, String> {
        let install = self.install.as_deref().map(|dir| Install { dir, content: self.content.clone() });
        Snapshot::from_bytes(bytes.to_vec()).restore(self.content.clone(), install.as_ref())
    }

    /// The timeline's marks: one pass over the checkpoints, restoring each. Battles come from the
    /// file's battle blocks, the rest from the states of the keyframes.
    fn build_marks(&self) -> Vec<Mark> {
        let mut marks = Vec::new();
        let mut owned: BTreeSet<u16> = BTreeSet::new();
        let mut hero_down = false;
        let mut victory = false;
        let mut first = true;
        for (s, seg) in self.segments.iter().enumerate() {
            let mut points: Vec<(Place, u64, &[u8])> = vec![(Place { seg: s, index: 0 }, seg.start_day, seg.snapshot.as_slice())];
            points.extend(seg.keyframes.iter().map(|k| (Place { seg: s, index: k.after }, k.day, k.snapshot.as_slice())));
            for (at, day, bytes) in points {
                marks.push(Mark { day, kind: MarkKind::Day, at });
                let Ok(g) = self.restore(bytes) else { continue };
                let now_owned: BTreeSet<u16> = g.world.locations.iter().filter(|l| l.owned()).map(|l| l.id).collect();
                if !first {
                    for loc in g.world.locations.iter().filter(|l| l.owned() && !owned.contains(&l.id)) {
                        marks.push(Mark { day, kind: MarkKind::Town { name: loc.name.clone() }, at });
                    }
                    if !hero_down && !g.squad[0].alive() {
                        hero_down = true;
                        marks.push(Mark { day, kind: MarkKind::HeroDown, at });
                    }
                    if !victory && g.won() {
                        victory = true;
                        marks.push(Mark { day, kind: MarkKind::Victory, at });
                    }
                }
                owned = now_owned;
                first = false;
            }
            for (after, start) in &seg.battles {
                marks.push(Mark { day: start.day, kind: battle_mark(start, &self.content), at: Place { seg: s, index: *after } });
            }
        }
        marks.sort_by_key(|m| m.at);
        marks
    }
}

/// The battle mark of a battle's start: its attacker and the unit names of both sides.
fn battle_mark(start: &BattleStart, content: &Content) -> MarkKind {
    let name = |def| content.try_unit(def).map_or_else(|| "?".to_string(), |d| d.name.clone());
    MarkKind::Battle {
        attacker: start.attacker,
        player: start.player.iter().map(|(_, u)| name(u.def)).collect(),
        enemy: start.enemy.iter().map(|u| name(u.def)).collect(),
    }
}
