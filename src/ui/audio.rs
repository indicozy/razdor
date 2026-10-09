//! Sound effects and music from the player's install (`_Sounds.ini`, `Sounds/`), played with
//! macroquad's audio.
//!
//! - Screens ask for effects with [`cue`]: a small per-frame queue that [`Audio::frame`]
//!   drains, so screens need no audio handle and the rules know nothing of sound.
//! - Music follows the [`Mood`] the app derives from the current screen ([`super::jukebox`]).
//!   Effects are loaded once at start; a music track is decoded when it starts and dropped
//!   when the next one does (a few MB each).
//! - Silent without an install, with `RAZDOR_NO_AUDIO=1`, when built without the `audio`
//!   feature, or when no sound device opens. `RAZDOR_AUDIO_LOG=1` prints every sound played.
//! - Volumes and mutes ([`Settings`]) are kept in `audio.json` in the save folder.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use macroquad::audio::{load_sound_from_bytes, play_sound, set_sound_volume, stop_sound, PlaySoundParams, Sound};
use serde::{Deserialize, Serialize};

use razdor::dt::data::ArtefactType;
use razdor::dt::install::DtInstall;
use razdor::dt::sound::{self, SoundTable};

pub use super::jukebox::Mood;
use super::jukebox::{self, Change, Jukebox};

/// A sound effect asked for by a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cue {
    /// Any button (`InterfaceButtonDown`).
    Button,
    /// A bottom panel icon is pressed (`InterfacePanelDown`; only there in the original,
    /// interface.md §14), or a Razdor window opens without one.
    Panel,
    /// A main menu item or a hero class is pressed (`MainMenuPress`).
    MenuPress,
    /// The pointer comes onto a main menu item (`MainMenuSelect-1`: the original reads only
    /// that key, for all five, interface.md §4).
    MenuSelect,
    /// A world spell is cast, or a building window's tab highlighted (`InterfaceCastSpell`),
    /// then a spell's effect on the own army
    /// (`Spell-Good`) or on an enemy army (`Spell-Evil`).
    CastSpell,
    SpellGood,
    SpellEvil,
    /// A battle begins (`Global-Battle`, the horn).
    BattleHorn,
    /// An event window, the village or the shipyard window opens (`Global-Event-1..3`): the
    /// chord the game's generator drew (`Game::event_chord`).
    Event(u8),
    /// A card moves in deployment or battle (`Card-Move`).
    CardMove,
    /// A level gained or a promotion (`Unit-Upgrade`).
    Upgrade,
    /// Battle actions: melee (`Battle-Fight`), bow or crossbow (`Battle-Shoot`), cannon
    /// (`Battle-Strike`), heal (`Battle-Cure`), bless (`Battle-Bless`), curse or magic strike
    /// (`Battle-Sorcery`).
    Fight,
    Shoot,
    Cannon,
    Cure,
    Bless,
    Sorcery,
    /// An item of this type bought, equipped or drunk (`Item-<Type>`).
    Item(ArtefactType),
    /// A money button (trade, hire, heal, learn, a ship) or a village's tribute taken
    /// (`Item-Gold`).
    Gold,
    /// A battle won: the triumph music, looped until the next map track.
    Triumph,
}

impl Cue {
    /// The `[SFX-Effects]` key.
    fn key(self) -> String {
        let key = match self {
            Cue::Button => "InterfaceButtonDown",
            Cue::Panel => "InterfacePanelDown",
            Cue::MenuPress => "MainMenuPress",
            Cue::MenuSelect => "MainMenuSelect-1",
            Cue::CastSpell => "InterfaceCastSpell",
            Cue::SpellGood => "Spell-Good",
            Cue::SpellEvil => "Spell-Evil",
            Cue::BattleHorn => "Global-Battle",
            Cue::Event(k) => return format!("Global-Event-{}", k % 3 + 1),
            Cue::CardMove => "Card-Move",
            Cue::Upgrade => "Unit-Upgrade",
            Cue::Fight => "Battle-Fight",
            Cue::Shoot => "Battle-Shoot",
            Cue::Cannon => "Battle-Strike",
            Cue::Cure => "Battle-Cure",
            Cue::Bless => "Battle-Bless",
            Cue::Sorcery => "Battle-Sorcery",
            Cue::Item(kind) => return format!("Item-{kind:?}"),
            Cue::Gold => "Item-Gold",
            Cue::Triumph => jukebox::TRIUMPH,
        };
        key.to_string()
    }
}

thread_local! {
    static CUES: RefCell<Vec<Cue>> = const { RefCell::new(Vec::new()) };
}

/// Asks for a sound effect; it plays at the start of the next frame.
pub fn cue(c: Cue) {
    CUES.with(|q| q.borrow_mut().push(c));
}

/// [`cue`]s `c` and gives back `value` (for use inside expressions).
pub fn cued<T>(c: Cue, value: T) -> T {
    cue(c);
    value
}

thread_local! {
    static ON_RELEASE: RefCell<Option<Cue>> = const { RefCell::new(None) };
}

/// Asks for `c` again when the left mouse button that pressed a button is let go: the
/// original's hire button plays `Item-Gold` on its press and again in its click action, which
/// runs on the release (0x4c7370, 0x4c7380), so the one buffer restarts there.
pub fn cue_on_release(c: Cue) {
    ON_RELEASE.with(|r| *r.borrow_mut() = Some(c));
}

/// The cue waiting for the release, once the button is up (`held` false), else nothing.
fn released_cue(waiting: &mut Option<Cue>, held: bool) -> Option<Cue> {
    if held {
        None
    } else {
        waiting.take()
    }
}

/// The cues of this frame, each once, in order.
fn take_cues() -> Vec<Cue> {
    let mut cues = CUES.with(|q| std::mem::take(&mut *q.borrow_mut()));
    let mut seen = Vec::new();
    cues.retain(|c| {
        let new = !seen.contains(c);
        seen.push(*c);
        new
    });
    cues
}

/// Volumes (0..1) and mutes, whether the frame rate shows in the corner and the battle AI's
/// level, saved in `audio.json` in the save folder.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub music_volume: f32,
    pub sfx_volume: f32,
    pub music_muted: bool,
    pub sfx_muted: bool,
    pub show_fps: bool,
    /// The enemy's battle AI: `Some(true)` expert (the original's "improved enemy AI in
    /// battle"), `Some(false)` easy; `None` until chosen: the install's `OptValue9`.
    pub expert_ai: Option<bool>,
    /// The front row's width for new games: `Some(true)` 6 cells (the Community's wide row),
    /// `Some(false)` 4 (its two edge places are the reserve's, as the back row's are);
    /// `None` until chosen: the install's `OptValue11`. A save keeps the width it was
    /// started with.
    pub wide_row: Option<bool>,
    /// Windowed, borderless or full screen.
    pub display: super::display::DisplayMode,
    /// The interface scale (`display::SCALES`); `0` is Auto, the largest that fits.
    pub ui_scale: f32,
    /// The minimap window's size dragged by the player, in pixels of the 960×720 video;
    /// `None`: the original's square.
    pub minimap_size: Option<(f32, f32)>,
    /// Advanced: stepping onto a friendly army with no event for the meeting lets the hero
    /// pass (`Game::friends_let_pass`); off, the original's battle.
    pub friends_let_pass: bool,
    /// Advanced: new releases are offered (`Ask`), put in place without a word (`Always`), or
    /// not looked for (`Off`); `razdor::update`.
    pub updates: razdor::update::Mode,
    /// The map's own zoom (Razdor's): the wheel steps from it and 0 comes back to it.
    pub map_zoom: f32,
    /// The wheel and the +/- keys leave the map's zoom alone: it stays `map_zoom`.
    pub zoom_locked: bool,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { music_volume: 0.6, sfx_volume: 0.8, music_muted: false, sfx_muted: false, show_fps: false, expert_ai: None, wide_row: None, display: Default::default(), ui_scale: 0.0, minimap_size: None, friends_let_pass: false, updates: Default::default(), map_zoom: 1.0, zoom_locked: false }
    }
}

/// One volume step of the menu's buttons and +/- keys.
pub const VOLUME_STEP: f32 = 0.1;

fn step_volume(v: f32, steps: i32) -> f32 {
    ((v / VOLUME_STEP).round() + steps as f32).clamp(0.0, 1.0 / VOLUME_STEP) * VOLUME_STEP
}

impl Settings {
    fn path() -> Option<PathBuf> {
        razdor::rules::save::default_dir().map(|d| d.join("audio.json"))
    }

    /// The saved settings, or the defaults.
    pub fn load() -> Settings {
        let read = Settings::path().and_then(|p| std::fs::read(p).ok());
        read.and_then(|b| serde_json::from_slice::<Settings>(&b).ok()).unwrap_or_default().clamped()
    }

    fn clamped(self) -> Settings {
        let fix = |v: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.5 };
        let ui_scale = if super::display::SCALES.contains(&self.ui_scale) { self.ui_scale } else { 0.0 };
        let (lo, hi) = super::world_view::ZOOM_RANGE;
        let map_zoom = if self.map_zoom.is_finite() { self.map_zoom.clamp(lo, hi) } else { 1.0 };
        Settings { music_volume: fix(self.music_volume), sfx_volume: fix(self.sfx_volume), ui_scale, map_zoom, ..self }
    }

    fn save(&self) {
        let Some(path) = Settings::path() else { return };
        let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| {
            std::fs::write(&path, serde_json::to_vec_pretty(self).unwrap_or_default())
        });
        if let Err(e) = written {
            razdor::diag!("{}: {e}", path.display());
        }
    }

    pub fn step_music(&mut self, steps: i32) {
        self.music_volume = step_volume(self.music_volume, steps);
    }

    pub fn step_sfx(&mut self, steps: i32) {
        self.sfx_volume = step_volume(self.sfx_volume, steps);
    }

    pub fn music_gain(&self) -> f32 {
        if self.music_muted { 0.0 } else { self.music_volume }
    }

    pub fn sfx_gain(&self) -> f32 {
        if self.sfx_muted { 0.0 } else { self.sfx_volume }
    }
}

/// The mixer's rate: sounds are resampled to it here, as its own resampler repeats samples.
const OUTPUT_RATE: u32 = 44100;

/// Loads a sound from WAV bytes. On native targets macroquad's loader never waits, so one
/// poll finishes it; a decoder panic is caught and gives `None`.
fn load_now(wav: &[u8]) -> Option<Sound> {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};
    let run = std::panic::AssertUnwindSafe(|| {
        let mut f = std::pin::pin!(load_sound_from_bytes(wav));
        match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(r) => r.ok(),
            Poll::Pending => None,
        }
    });
    razdor::diag::quiet(|| std::panic::catch_unwind(run)).ok().flatten()
}

/// Whether a playback device opens (the audio thread of quad-snd panics without one, and
/// every sound sent to it is lost).
#[cfg(all(feature = "audio", target_os = "linux"))]
fn device_ok() -> bool {
    use std::os::raw::{c_char, c_int, c_void};
    #[link(name = "asound")]
    extern "C" {
        fn snd_pcm_open(pcm: *mut *mut c_void, name: *const c_char, stream: c_int, mode: c_int) -> c_int;
        fn snd_pcm_close(pcm: *mut c_void) -> c_int;
    }
    const PLAYBACK: c_int = 0;
    const NONBLOCK: c_int = 1;
    // The devices quad-snd tries.
    [c"default", c"pipewire"].iter().any(|name| {
        let mut pcm = std::ptr::null_mut();
        // SAFETY: plain ALSA calls with a valid out-pointer and a NUL-terminated name; the
        // handle is closed right away.
        unsafe {
            let ok = snd_pcm_open(&mut pcm, name.as_ptr(), PLAYBACK, NONBLOCK) >= 0;
            if ok {
                snd_pcm_close(pcm);
            }
            ok
        }
    })
}

#[cfg(all(feature = "audio", not(target_os = "linux")))]
fn device_ok() -> bool {
    true
}

#[cfg(not(feature = "audio"))]
fn device_ok() -> bool {
    false
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The loaded sounds of an install.
struct Backend {
    dir: PathBuf,
    table: SoundTable,
    raw_rate: u32,
    /// Effects by `[SFX-Effects]` key.
    sfx: HashMap<String, Sound>,
    music: Option<Sound>,
    /// The music volume last applied.
    gain: f32,
    jukebox: Jukebox,
}

pub struct Audio {
    backend: Option<Backend>,
    pub settings: Settings,
    saved: Settings,
    log: bool,
}

impl Audio {
    /// The map rotation's track from now on (`rules::music`): the world theme at a map start
    /// or load, then the app's picks. A triumph still playing ends.
    pub fn set_map_track(&mut self, track: &'static str) {
        if let Some(b) = self.backend.as_mut() {
            b.jukebox.set_map_track(track);
        }
    }

    /// No sound at all.
    pub fn silent() -> Audio {
        let settings = Settings::load();
        Audio { backend: None, settings, saved: settings, log: env_flag("RAZDOR_AUDIO_LOG") }
    }

    /// Loads the effects of `install` (if any) and gets its music ready.
    pub fn new(install: Option<&DtInstall>) -> Audio {
        let mut audio = Audio::silent();
        let why = match install {
            _ if !cfg!(feature = "audio") => "built without the audio feature",
            _ if env_flag("RAZDOR_NO_AUDIO") => "RAZDOR_NO_AUDIO is set",
            None => "no install",
            Some(_) if !device_ok() => "no sound device",
            Some(dt) => match (std::time::Instant::now(), Backend::load(&dt.dir)) {
                (t0, Ok(b)) => {
                    if audio.log {
                        let (fx, tracks, ms) = (b.sfx.len(), b.table.backgrounds.len(), t0.elapsed().as_millis());
                        razdor::diag!("audio: {fx} effects loaded in {ms} ms, {tracks} music tracks, raw music at {} Hz", b.raw_rate);
                    }
                    audio.backend = Some(b);
                    return audio;
                }
                (_, Err(e)) => {
                    razdor::diag!("{}: {e}; playing without sound", sound::SOUNDS_INI);
                    "no sound table"
                }
            },
        };
        if audio.log || why == "no sound device" {
            razdor::diag!("audio: silent ({why})");
        }
        audio
    }

    /// Stops the music and writes changed settings (the app is quitting).
    pub fn shutdown(&mut self) {
        if let Some(b) = self.backend.as_mut() {
            b.stop_music();
        }
        if self.settings != self.saved {
            self.settings.clamped().save();
            self.saved = self.settings;
        }
        if self.log {
            razdor::diag!("audio: shut down");
        }
    }

    /// Plays this frame's cues and keeps the music of `mood` going.
    pub fn frame(&mut self, mood: Mood) {
        let held = macroquad::input::is_mouse_button_down(macroquad::input::MouseButton::Left);
        if let Some(c) = ON_RELEASE.with(|r| released_cue(&mut r.borrow_mut(), held)) {
            cue(c);
        }
        let cues = take_cues();
        if self.settings != self.saved {
            self.settings = self.settings.clamped();
            self.settings.save();
            self.saved = self.settings;
        }
        let Some(b) = self.backend.as_mut() else {
            if self.log {
                for c in cues {
                    razdor::diag!("audio (silent): {}", c.key());
                }
            }
            return;
        };
        for c in cues {
            if c == Cue::Triumph {
                b.jukebox.triumph();
            } else {
                b.play_effect(c, &self.settings, self.log);
            }
        }
        let change = b.jukebox.update(mood, macroquad::time::get_time());
        b.apply(change, &self.settings, self.log);
        let gain = self.settings.music_gain();
        if gain != b.gain {
            if let Some(m) = &b.music {
                set_sound_volume(m, gain);
            }
            b.gain = gain;
        }
    }
}

impl Backend {
    fn load(dir: &Path) -> Result<Backend, razdor::dt::DtError> {
        let table = sound::read_table(dir)?;
        let raw_rate = sound::raw_rate_from_env();
        let mut sfx = HashMap::new();
        for (key, file) in &table.effects {
            match sound::read_sound(dir, file, raw_rate) {
                Ok(pcm) => match load_now(&pcm.resampled(OUTPUT_RATE).to_wav()) {
                    Some(s) => {
                        sfx.insert(key.to_ascii_lowercase(), s);
                    }
                    None => razdor::diag!("{file}: cannot be played"),
                },
                Err(e) => razdor::diag!("{key}={file}: {e}"),
            }
        }
        let jukebox = Jukebox::new(|t| table.background(t).is_some());
        Ok(Backend { dir: dir.to_path_buf(), table, raw_rate, sfx, music: None, gain: 0.0, jukebox })
    }

    fn play_effect(&mut self, c: Cue, settings: &Settings, log: bool) {
        let key = c.key();
        let gain = settings.sfx_gain();
        match self.sfx.get(&key.to_ascii_lowercase()) {
            Some(s) if gain > 0.0 => {
                // One buffer per sound, as the original's (engine.md §8): playing a sound that
                // is still playing restarts it instead of layering a second copy on top.
                stop_sound(s);
                play_sound(s, PlaySoundParams { looped: false, volume: gain });
                if log {
                    razdor::diag!("audio: sfx {key} ({}) at {gain:.1}", self.table.effect(&key).unwrap_or("?"));
                }
            }
            Some(_) if log => razdor::diag!("audio: sfx {key} muted"),
            None if log => razdor::diag!("audio: sfx {key} has no sound"),
            _ => {}
        }
    }

    fn stop_music(&mut self) {
        if let Some(m) = self.music.take() {
            stop_sound(&m);
        }
    }

    fn apply(&mut self, change: Option<Change>, settings: &Settings, log: bool) {
        match change {
            None => {}
            Some(Change::Stop) => {
                self.stop_music();
                if log {
                    razdor::diag!("audio: music stops");
                }
            }
            Some(Change::Play(track)) => {
                self.stop_music();
                let file = self.table.background(track).unwrap_or_default().to_string();
                let t0 = std::time::Instant::now();
                let loaded = sound::read_sound(&self.dir, &file, self.raw_rate)
                    .map_err(|e| e.to_string())
                    .and_then(|pcm| load_now(&pcm.resampled(OUTPUT_RATE).to_wav()).map(|s| (s, pcm.duration())).ok_or_else(|| "cannot be played".into()));
                match loaded {
                    Ok((s, secs)) => {
                        let gain = settings.music_gain();
                        play_sound(&s, PlaySoundParams { looped: false, volume: gain });
                        self.gain = gain;
                        self.music = Some(s);
                        self.jukebox.started(track, macroquad::time::get_time(), secs);
                        if log {
                            razdor::diag!("audio: music {track} ({file}, {secs:.1} s, decoded in {} ms) at {gain:.1}", t0.elapsed().as_millis());
                        }
                    }
                    Err(e) => {
                        razdor::diag!("{track}={file}: {e}");
                        self.jukebox.failed(track);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cue_keys_match_the_ini() {
        assert_eq!(Cue::Item(ArtefactType::BlowWeapon).key(), "Item-BlowWeapon");
        assert_eq!(Cue::Item(ArtefactType::Potion).key(), "Item-Potion");
        assert_eq!(Cue::Cannon.key(), "Battle-Strike");
        let chords: Vec<String> = (0..3).map(|k| Cue::Event(k).key()).collect();
        assert_eq!(chords, ["Global-Event-1", "Global-Event-2", "Global-Event-3"]);
    }

    #[test]
    fn cues_are_played_once_per_frame() {
        cue(Cue::Button);
        cue(Cue::Panel);
        cue(Cue::Button);
        assert_eq!(take_cues(), [Cue::Button, Cue::Panel]);
        assert!(take_cues().is_empty());
    }

    /// The hire's second `Item-Gold` waits while the button is held and comes once, at the
    /// release (a press and release in one frame give one play: the restart is at once).
    #[test]
    fn a_release_cue_waits_for_the_button_to_go_up() {
        let mut waiting = Some(Cue::Gold);
        assert_eq!(released_cue(&mut waiting, true), None);
        assert_eq!(released_cue(&mut waiting, true), None);
        assert_eq!(released_cue(&mut waiting, false), Some(Cue::Gold));
        assert_eq!(released_cue(&mut waiting, false), None);
        cue(Cue::Gold);
        cue(Cue::Gold);
        assert_eq!(take_cues(), [Cue::Gold]);
    }

    #[test]
    fn volume_steps_and_settings_file() {
        let mut s = Settings::default();
        s.step_music(5);
        assert!((s.music_volume - 1.0).abs() < 1e-6);
        s.step_music(-3);
        assert!((s.music_volume - 0.7).abs() < 1e-6);
        s.step_sfx(-20);
        assert_eq!(s.sfx_volume, 0.0);
        s.music_muted = true;
        s.show_fps = true;
        s.expert_ai = Some(true);
        assert_eq!(s.music_gain(), 0.0);
        let back: Settings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
        // Missing fields take the defaults; out-of-range volumes are clamped.
        let partial: Settings = serde_json::from_str(r#"{"music_volume": 3.0}"#).unwrap();
        assert_eq!(partial.clamped(), Settings { music_volume: 1.0, ..Settings::default() });
    }

    /// The front row's width: the install's `OptValue11` until chosen, then the choice; an
    /// older file without it keeps following the install.
    #[test]
    fn the_front_row_setting_falls_back_on_the_install() {
        use crate::ui::main_menu::wide_row;
        let old: Settings = serde_json::from_str(r#"{"music_volume": 0.5, "expert_ai": true}"#).unwrap();
        assert_eq!(old.wide_row, None);
        assert!(wide_row(&old, true) && !wide_row(&old, false));
        let four = Settings { wide_row: Some(false), ..Settings::default() };
        assert!(!wide_row(&four, true));
        let back: Settings = serde_json::from_str(&serde_json::to_string(&four).unwrap()).unwrap();
        assert_eq!(back.wide_row, Some(false));
    }

    #[test]
    fn every_cue_has_a_real_sound() {
        let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(Path::new(&dir)).unwrap();
        let t = dt.sound_table().unwrap();
        let kinds = [
            ArtefactType::BlowWeapon, ArtefactType::ShotWeapon, ArtefactType::Staff, ArtefactType::Armor,
            ArtefactType::Helm, ArtefactType::Shield, ArtefactType::Ring, ArtefactType::Amulet,
            ArtefactType::Potion, ArtefactType::Item,
        ];
        let mut cues = vec![
            Cue::Button, Cue::Panel, Cue::MenuPress, Cue::MenuSelect, Cue::CastSpell, Cue::SpellGood, Cue::SpellEvil,
            Cue::BattleHorn, Cue::CardMove, Cue::Upgrade, Cue::Fight, Cue::Shoot, Cue::Cannon,
            Cue::Cure, Cue::Bless, Cue::Sorcery, Cue::Gold,
        ];
        cues.extend(kinds.map(Cue::Item));
        for c in cues {
            assert!(t.effect(&c.key()).is_some(), "{c:?}");
        }
        assert!((0..3).all(|k| t.effect(&Cue::Event(k).key()).is_some()));
        let tracks = [jukebox::MENU, jukebox::AUTHORS, jukebox::TRIUMPH, jukebox::DEFEAT].into_iter().chain(jukebox::MAP).chain(jukebox::BATTLE);
        for track in tracks {
            assert!(t.background(track).is_some(), "{track}");
        }
    }
}
