//! Which music track plays when (interface.md §13): the menu theme, the credits theme, the
//! map track the rotation picked, the battle theme by the opponent, the triumph after a won
//! battle and the defeat piece. Pure logic (no macroquad): the player ([`super::audio`]) asks
//! for a change each frame, starts the track and reports how long it lasts. macroquad has no
//! "sound ended" callback, so the end is the start time plus the track's length from its
//! sample count; a looping track is started again there.
//!
//! The map rotation's picks are draws of the game's generator, so the app makes them
//! (`rules::music`, `App::rotate_music`) and hands the track over ([`Jukebox::set_map_track`]).
//!
//! A change crossfades as in the original (0x49d774, 0x4819cc): the old track fades out and the
//! new one in, linearly, over 2 s ([`SCREEN_FADE`]), or 4 s for the rotation's picks
//! ([`ROTATION_FADE`]); the program's first track starts without one. The levels ([`Fade`]) are
//! advanced by the frames' lengths and scale the music volume.
//!
//! Tracks are named by their `_Sounds.ini` keys (`[Backgrounds]`).

pub use razdor::rules::music::ROTATION as MAP;

pub const MENU: &str = "BkgMenuMain";
pub const AUTHORS: &str = "BkgAuthors";
/// The battle themes, the triumph and the defeat piece (shared with the replay's log).
pub use razdor::av::{BATTLE, DEFEAT, TRIUMPH};

/// A crossfade's length on a screen change (a battle, its end, the menus), in seconds.
pub const SCREEN_FADE: f32 = 2.0;
/// A crossfade's length when the map rotation picks the next track, in seconds.
pub const ROTATION_FADE: f32 = 4.0;

/// One track's loudness in a crossfade (0x4819cc): its level, a share of the music volume,
/// goes in a straight line from `from` to `to` over `secs`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fade {
    from: f32,
    to: f32,
    secs: f32,
    /// Seconds since it began, at most `secs`.
    t: f32,
}

impl Fade {
    /// The full music volume, no fade.
    pub const FULL: Fade = Fade { from: 1.0, to: 1.0, secs: 0.0, t: 0.0 };

    /// From silence up to the full music volume over `secs`.
    pub fn fade_in(secs: f32) -> Fade {
        Fade { from: 0.0, to: 1.0, secs: secs.max(0.0), t: 0.0 }
    }

    /// A newer fade toward `to` over `secs`, from the level this one has reached: it replaces
    /// this one, as the original's does.
    pub fn toward(self, to: f32, secs: f32) -> Fade {
        Fade { from: self.level(), to, secs: secs.max(0.0), t: 0.0 }
    }

    /// The share of the music volume now (0..1).
    pub fn level(&self) -> f32 {
        if self.t >= self.secs {
            self.to
        } else {
            self.from + (self.to - self.from) * self.t / self.secs
        }
    }

    /// `dt` seconds (a frame) have gone by.
    pub fn step(&mut self, dt: f32) {
        self.t = (self.t + dt.max(0.0)).min(self.secs);
    }

    /// It has faded out: the track can stop.
    pub fn silent(&self) -> bool {
        self.t >= self.secs && self.to <= 0.0
    }
}

/// What the current screen wants to hear.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mood {
    Silent,
    /// Scenario and class select: the menu theme, looped.
    Menu,
    /// The credits: their theme, looped.
    Credits,
    /// World map and its windows: the rotation's track, looped.
    Map,
    /// A battle: `BkgBattle1` against a garrison, `BkgBattle2` against an army, looped.
    Battle { garrison: bool },
    /// The scenario is won: the triumph piece once.
    Won,
    /// The hero has fallen: the defeat piece once.
    Lost,
}

/// A request to the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Change {
    /// Stop whatever plays and start this track.
    Play(&'static str),
    Stop,
}

#[derive(Clone, Copy, Debug)]
struct Current {
    track: &'static str,
    /// When it ends, in the player's clock; infinite until [`Jukebox::started`].
    ends: f64,
    /// Played once (the end pieces), not looped.
    once: bool,
}

pub struct Jukebox {
    /// Tracks that have a file and have not failed.
    available: Vec<&'static str>,
    mood: Mood,
    current: Option<Current>,
    /// The Won/Lost piece has played: silence until the mood changes.
    finished: bool,
    /// The map rotation's track: the world theme until the app picks another.
    map_track: &'static str,
    /// A battle was won: the triumph plays, looped, until the next map track (or battle).
    triumph: bool,
}

impl Jukebox {
    /// `has(track)`: whether the install names a file for the track.
    pub fn new(has: impl Fn(&str) -> bool) -> Jukebox {
        let all = [MENU, AUTHORS, TRIUMPH, DEFEAT].into_iter().chain(MAP).chain(BATTLE);
        let available: Vec<&'static str> = all.filter(|t| has(t)).collect();
        Jukebox { available, mood: Mood::Silent, current: None, finished: false, map_track: MAP[razdor::rules::music::WORLD_THEME], triumph: false }
    }

    /// The track playing (or being started), if any.
    #[cfg(test)]
    pub fn current(&self) -> Option<&'static str> {
        self.current.map(|c| c.track)
    }

    fn has(&self, track: &str) -> bool {
        self.available.contains(&track)
    }

    /// The map rotation moved on (or a map started): its track plays from now on, and a
    /// triumph still playing ends.
    pub fn set_map_track(&mut self, track: &'static str) {
        self.map_track = track;
        self.triumph = false;
    }

    /// A battle is won: the triumph starts at once and loops over the map until the next
    /// map track, as in the original.
    pub fn triumph(&mut self) {
        self.triumph = true;
    }

    /// The mood's track, and whether it loops.
    fn wanted(&self) -> Option<(&'static str, bool)> {
        let (track, looped) = match self.mood {
            Mood::Silent => return None,
            _ if self.triumph && matches!(self.mood, Mood::Map | Mood::Battle { .. } | Mood::Won) => (TRIUMPH, true),
            Mood::Menu => (MENU, true),
            Mood::Credits => (AUTHORS, true),
            Mood::Map => (self.map_track, true),
            Mood::Battle { garrison } => (BATTLE[if garrison { 0 } else { 1 }], true),
            Mood::Won | Mood::Lost if self.finished => return None,
            Mood::Won => (TRIUMPH, false),
            Mood::Lost => (DEFEAT, false),
        };
        self.has(track).then_some((track, looped))
    }

    /// What to change for `mood` at time `now` (seconds).
    pub fn update(&mut self, mood: Mood, now: f64) -> Option<Change> {
        if mood != self.mood {
            self.mood = mood;
            self.finished = false;
            // A new battle or the menus end a triumph; the battle that won the scenario
            // keeps it as its end piece.
            if matches!(mood, Mood::Battle { .. } | Mood::Menu | Mood::Credits | Mood::Lost) {
                self.triumph = false;
            }
        }
        if let Some(c) = self.current.filter(|c| now >= c.ends) {
            self.current = None;
            if c.once {
                self.finished = true;
                return None;
            }
            // A looping track that ended: it starts again.
            if self.wanted().is_some_and(|(t, _)| t == c.track) {
                self.current = Some(Current { track: c.track, ends: f64::INFINITY, once: false });
                return Some(Change::Play(c.track));
            }
        }
        match self.wanted() {
            None => self.current.take().map(|_| Change::Stop),
            Some((track, _)) if self.current.is_some_and(|c| c.track == track) => None,
            Some((track, looped)) => {
                self.current = Some(Current { track, ends: f64::INFINITY, once: !looped });
                Some(Change::Play(track))
            }
        }
    }

    /// The player started `track` at `now`; it lasts `secs`.
    pub fn started(&mut self, track: &'static str, now: f64, secs: f64) {
        if let Some(c) = self.current.as_mut().filter(|c| c.track == track) {
            c.ends = now + secs;
        }
    }

    /// `track` could not be played: it is never picked again.
    pub fn failed(&mut self, track: &'static str) {
        self.available.retain(|t| *t != track);
        if self.current.is_some_and(|c| c.track == track) {
            self.current = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Jukebox {
        Jukebox::new(|_| true)
    }

    /// Applies `update` like the player: a started track lasts `secs`.
    fn step(j: &mut Jukebox, mood: Mood, now: f64, secs: f64) -> Option<Change> {
        let c = j.update(mood, now);
        if let Some(Change::Play(t)) = c {
            j.started(t, now, secs);
        }
        c
    }

    #[test]
    fn menu_theme_loops() {
        let mut j = all();
        assert_eq!(step(&mut j, Mood::Menu, 0.0, 10.0), Some(Change::Play(MENU)));
        assert_eq!(step(&mut j, Mood::Menu, 9.9, 10.0), None);
        assert_eq!(step(&mut j, Mood::Menu, 10.0, 10.0), Some(Change::Play(MENU)));
        assert_eq!(step(&mut j, Mood::Credits, 11.0, 10.0), Some(Change::Play(AUTHORS)), "the credits have their theme");
        assert_eq!(step(&mut j, Mood::Silent, 12.0, 10.0), Some(Change::Stop));
        assert_eq!(step(&mut j, Mood::Silent, 30.0, 10.0), None);
    }

    #[test]
    fn the_map_plays_the_world_theme_then_the_rotations_track() {
        let mut j = all();
        assert_eq!(step(&mut j, Mood::Map, 0.0, 10.0), Some(Change::Play("BkgMap2")));
        assert_eq!(step(&mut j, Mood::Map, 10.0, 10.0), Some(Change::Play("BkgMap2")), "it loops");
        j.set_map_track(AUTHORS);
        assert_eq!(step(&mut j, Mood::Map, 11.0, 10.0), Some(Change::Play(AUTHORS)), "the credits theme is in the rotation");
        assert_eq!(step(&mut j, Mood::Map, 12.0, 10.0), None);
    }

    #[test]
    fn battle_music_by_the_opponent() {
        let mut j = all();
        step(&mut j, Mood::Map, 0.0, 100.0);
        assert_eq!(step(&mut j, Mood::Battle { garrison: true }, 1.0, 20.0), Some(Change::Play("BkgBattle1")));
        assert_eq!(step(&mut j, Mood::Battle { garrison: true }, 21.0, 20.0), Some(Change::Play("BkgBattle1")), "it loops");
        step(&mut j, Mood::Map, 30.0, 100.0);
        assert_eq!(step(&mut j, Mood::Battle { garrison: false }, 31.0, 20.0), Some(Change::Play("BkgBattle2")));
    }

    #[test]
    fn the_triumph_loops_until_the_next_map_track() {
        let mut j = all();
        step(&mut j, Mood::Battle { garrison: false }, 0.0, 100.0);
        j.triumph();
        assert_eq!(step(&mut j, Mood::Battle { garrison: false }, 1.0, 8.0), Some(Change::Play(TRIUMPH)), "at the win");
        assert_eq!(step(&mut j, Mood::Map, 2.0, 8.0), None, "over the map");
        assert_eq!(step(&mut j, Mood::Map, 9.0, 8.0), Some(Change::Play(TRIUMPH)), "looped");
        j.set_map_track("BkgMap6");
        assert_eq!(step(&mut j, Mood::Map, 10.0, 8.0), Some(Change::Play("BkgMap6")));
    }

    #[test]
    fn a_battle_interrupts_the_triumph() {
        let mut j = all();
        step(&mut j, Mood::Map, 0.0, 100.0);
        j.triumph();
        step(&mut j, Mood::Map, 1.0, 50.0);
        assert_eq!(step(&mut j, Mood::Battle { garrison: true }, 2.0, 20.0), Some(Change::Play("BkgBattle1")));
        assert_eq!(step(&mut j, Mood::Map, 3.0, 20.0), Some(Change::Play("BkgMap2")));
    }

    #[test]
    fn end_pieces_play_once() {
        let mut j = all();
        assert_eq!(step(&mut j, Mood::Lost, 0.0, 5.0), Some(Change::Play(DEFEAT)));
        assert_eq!(step(&mut j, Mood::Lost, 5.0, 5.0), None);
        assert_eq!(step(&mut j, Mood::Lost, 60.0, 5.0), None);
        assert_eq!(j.current(), None);
        assert_eq!(step(&mut j, Mood::Menu, 61.0, 5.0), Some(Change::Play(MENU)));
        assert_eq!(step(&mut j, Mood::Won, 62.0, 5.0), Some(Change::Play(TRIUMPH)));
        assert_eq!(step(&mut j, Mood::Won, 70.0, 5.0), None);
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-5
    }

    #[test]
    fn a_fade_is_linear_over_its_length() {
        let mut f = Fade::fade_in(SCREEN_FADE);
        assert_eq!(f.level(), 0.0);
        f.step(0.5);
        assert!(close(f.level(), 0.25));
        f.step(1.0);
        assert!(close(f.level(), 0.75));
        f.step(10.0);
        assert_eq!(f.level(), 1.0);
        assert!(!f.silent());
        let mut out = f.toward(0.0, ROTATION_FADE);
        out.step(1.0);
        assert!(close(out.level(), 0.75));
        out.step(-1.0);
        assert!(close(out.level(), 0.75), "time does not run back");
        out.step(3.0);
        assert!(out.silent());
        assert_eq!(Fade::FULL.level(), 1.0);
        assert!(!Fade::FULL.silent());
    }

    /// A track faded halfway out and brought back rises from where it was, over the whole
    /// length (the newer fade starts from the current volume).
    #[test]
    fn a_newer_fade_starts_from_the_current_level() {
        let mut f = Fade::FULL.toward(0.0, 2.0);
        f.step(1.0);
        let mut back = f.toward(1.0, 2.0);
        assert!(close(back.level(), 0.5));
        back.step(1.0);
        assert!(close(back.level(), 0.75));
        back.step(1.0);
        assert_eq!(back.level(), 1.0);
        assert!(Fade::FULL.toward(0.0, 0.0).silent(), "a zero length is at its end at once");
    }

    #[test]
    fn missing_and_failed_tracks_are_skipped() {
        let mut j = Jukebox::new(|t| t == "BkgMap2" || t == "BkgMap5" || t == DEFEAT);
        assert_eq!(step(&mut j, Mood::Menu, 0.0, 5.0), None);
        assert_eq!(step(&mut j, Mood::Battle { garrison: true }, 0.0, 5.0), None);
        assert_eq!(step(&mut j, Mood::Map, 1.0, 5.0), Some(Change::Play("BkgMap2")));
        j.failed("BkgMap2");
        assert_eq!(step(&mut j, Mood::Map, 2.0, 5.0), None);
        j.set_map_track("BkgMap5");
        assert_eq!(step(&mut j, Mood::Map, 3.0, 5.0), Some(Change::Play("BkgMap5")));
        j.set_map_track("BkgMap1");
        assert_eq!(step(&mut j, Mood::Map, 4.0, 5.0), Some(Change::Stop), "no file: silence");
        assert_eq!(step(&mut j, Mood::Lost, 5.0, 10.0), Some(Change::Play(DEFEAT)));
    }
}
