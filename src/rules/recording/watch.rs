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

use super::battle_start::BattleStart;
use super::file::{read, ReplayFile, Segment};
use super::header::ReplayHeader;
use super::hook;
use super::play::Player;
use super::snapshot::{state_hash, Snapshot};
use crate::rules::battle::Team;
use crate::rules::content::Content;
use crate::rules::game::Game;
use crate::rules::save::Install;

/// The game time of one playback frame (the states do not depend on it, see `play`).
const FRAME_DT: f32 = 1.0 / 30.0;

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
        let r = self.step();
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
            if !self.step()? {
                return Ok(self.place());
            }
        }
    }

    fn seek_place_inner(&mut self, target: Place) -> Result<(), Drift> {
        let (at, bytes) = self.checkpoint_before(target).ok_or_else(|| self.fail(target, "no checkpoint before the place".into()))?;
        self.start_at(at, &bytes)?;
        while self.place() < target {
            if !self.step()? {
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

    /// One step of the playback (see [`ReplayPlayer::advance`]).
    fn step(&mut self) -> Result<bool, Drift> {
        if let Some(d) = self.drift.clone() {
            return Err(d);
        }
        loop {
            if self.player.is_none() {
                if self.seg >= self.segments.len() {
                    return Ok(false);
                }
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
                // A segment's end: the next segment starts from its own state (a load).
                self.player = None;
                self.seg += 1;
                hook::take_playback();
                continue;
            }
            let here = self.place();
            let entry = &self.segments[self.seg].entries[self.idx];
            let player = self.player.as_mut().expect("a started game");
            match player.apply(entry) {
                Ok(()) => {
                    self.idx += 1;
                    self.last_good = self.place();
                    return Ok(true);
                }
                Err(reason) => {
                    let day = player.game.clock.day_index();
                    let d = Drift { day, version: self.header.razdor.clone(), at: here, reason };
                    self.drift = Some(d.clone());
                    return Err(d);
                }
            }
        }
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
