//! The trailer's shots and fly mode (dev-only; nothing here is reachable without
//! `razdor --trailer`, `--plan`, `--preview` or `RAZDOR_TRAILER_FLY=1`).
//!
//! A shot (`razdor::trailer`, the format in `docs/trailer-shots.md`) starts a game on its map,
//! then each frame of the snapshot clock (`clock::set_shot`, one frame is `1 / fps` seconds)
//! sets the camera from its keyframes, issues the timed world actions (walks, passes, reveals),
//! and is drawn with the UI and the fog hidden. The world's own rules run as they do in play:
//! the map frame ticks the game, events open their windows and those are answered here (the
//! shot's `event` choices, else the first button), and a battle is fought by the battle AI on
//! both sides to its end (`battles map`, the default: on the map; `battles show`: its screen).
//! After each frame the picture is recorded (ffmpeg pipe, PNG frames or per-frame hashes).
//!
//! Fly mode (`RAZDOR_TRAILER_FLY=1`) is the same camera driven by the keys and the mouse, on the
//! live clock, with F8 printing the camera as a keyframe line.

use std::cell::RefCell;
use std::collections::VecDeque;
use razdor::local;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, OnceLock};

use macroquad::prelude::*;
use super::input::{is_key_down, is_key_pressed, is_mouse_button_down, mouse_wheel};
use razdor::rules::content::HeroClass;
use razdor::rules::game::{BattleResult, Game};
use razdor::trailer::{self as shotfile, Action, MapRef, Shot, Target, Values};

use super::audio::Settings;
use super::battle_view::BattleView;
use super::dialog::{Close, Dialog};
use super::view25::{Trailer, FOV_DEG};
use super::{App, Screen};

/// What this run does with a shot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Render the shot to the outputs.
    Render,
    /// Render nothing; print the planner's report (where the hero and the armies are at each
    /// keyframe, the events and their answers, the warnings). Runs the same frames at a
    /// window of 64 px, so the report is the render's own.
    Plan,
    /// Render at 640 px wide, every `every`-th frame.
    Preview,
    /// The live fly mode (no shot).
    Fly,
}

/// The run's settings from the command line or the environment.
pub struct Config {
    pub mode: Mode,
    pub shot: Option<Shot>,
    pub shot_path: Option<PathBuf>,
    /// The MP4 to write (ffmpeg; the picture by NVENC or libx264, the sound muxed at the end).
    pub out: Option<PathBuf>,
    /// A folder of numbered PNG frames.
    pub frames: Option<PathBuf>,
    /// A file of `frame sha256` lines, one per frame (the determinism check).
    pub hashes: Option<PathBuf>,
    /// The encoder: `h264_nvenc` (default) or `libx264`.
    pub encoder: String,
    /// The window's size: the shot's, or the preview's and the plan's.
    pub size: (u32, u32),
    /// Preview: the frames kept, every this many.
    pub every: u64,
}

static CONFIG: OnceLock<Option<Config>> = OnceLock::new();

/// The per-frame state of a run.
#[derive(Default)]
struct Runtime {
    /// The map is set up (the first frame).
    started: bool,
    /// The timed actions started so far (the shot's `actions` are sorted by time).
    next_action: usize,
    /// The cells left of the walk under way.
    route: VecDeque<(i32, i32)>,
    /// The camera and target this frame (`world_view` reads them).
    cam: Option<Trailer>,
    target: Option<(f32, f32)>,
    /// The next keyframe the planner reports.
    next_report: usize,
    /// The encoder and the other sinks, once a render starts.
    sinks: Option<Sinks>,
    /// Frames whose pictures were written.
    written: u64,
    /// The fly mode's camera.
    fly: Option<Fly>,
    /// A battle shown on its screen is played to its end (once).
    battle_played: bool,
    /// The run ended (the window closes after the frame).
    done: bool,
    /// The sound of a render (`record_sound`), once the render starts.
    sound: Option<SoundRec>,
    /// A `pass` under way: the game's total minutes it ends at. The game's wait stops at each
    /// day report (as it does for a player), so the wait is taken up again until the end.
    pass_until: Option<f64>,
    /// The warning that a target's army is gone was given (once a run).
    gone_warned: bool,
}

/// The sound of a render: the game's jukebox run as the game runs it (the screens' moods, the
/// map's rotation), and the sound effects the screens asked for, logged instead of played.
struct SoundRec {
    dir: PathBuf,
    table: razdor::dt::sound::SoundTable,
    jukebox: super::jukebox::Jukebox,
    log: razdor::trailer::SoundLog,
    music: bool,
}

/// Whether a render records the sound (instead of playing it).
pub fn records_sound() -> bool {
    with(|r| r.sound.is_some())
}

/// The game asked for these sounds this frame, under `mood`: the effects are logged, and the
/// music is run through the jukebox as the audio player runs it (a track change is logged).
pub fn record_sound(mood: super::jukebox::Mood, cues: &[super::audio::Cue]) {
    let frame = super::clock::frame_index();
    let now = super::clock::now();
    with(|r| {
        let Some(s) = r.sound.as_mut() else { return };
        for &c in cues {
            if c == super::audio::Cue::Triumph {
                s.jukebox.triumph();
            } else if let Some(file) = s.table.effect(&c.key()) {
                s.log.push(frame, razdor::trailer::SoundEvent::Effect(file.to_string()));
            }
        }
        if !s.music {
            return;
        }
        if let Some(super::jukebox::Change::Play(track)) = s.jukebox.update(mood, now) {
            let file = s.table.background(track).unwrap_or_default().to_string();
            let secs = razdor::dt::sound::read_sound(&s.dir, &file, razdor::dt::sound::raw_rate_from_env()).map_or(0.0, |p| p.duration());
            s.jukebox.started(track, now, secs);
            s.log.push(frame, razdor::trailer::SoundEvent::Music(file));
        }
    });
}

/// The map's track rotation (`Audio::set_map_track`), as the jukebox takes it during a render.
pub fn map_track(track: &'static str) {
    with(|r| {
        if let Some(s) = r.sound.as_mut() {
            s.jukebox.set_map_track(track);
        }
    });
}

/// The sound of a render's shot: the install's sound table, the jukebox over its music.
fn sound_for(app: &App, shot: &Shot) -> Option<SoundRec> {
    let dir = app.assets.dt.as_ref()?.install.dir.clone();
    let table = razdor::dt::sound::read_table(&dir).ok()?;
    let jukebox = super::jukebox::Jukebox::new(|t| table.background(t).is_some());
    Some(SoundRec { dir, table, jukebox, log: Default::default(), music: shot.music })
}

/// Mixes the render's sound (effects, and the music tracks until the next one starts) into a
/// WAV and muxes it into the MP4, which then replaces the video-only file.
fn mux_sound(out: &Path, s: &SoundRec, fps: u32, total: f64) -> Result<(), String> {
    use razdor::dt::sound::{read_sound, raw_rate_from_env, Pcm};
    use razdor::trailer::{mix_wav, Placed, SoundEvent};
    let mut pcm: std::collections::HashMap<String, Pcm> = std::collections::HashMap::new();
    let starts = s.log.starts(fps);
    let music_starts: Vec<f64> = starts.iter().filter(|(_, e)| matches!(e, SoundEvent::Music(_))).map(|(t, _)| *t).collect();
    let mut placed_music = 0usize;
    let mut clips = Vec::new();
    for (start, event) in &starts {
        let (file, gain) = match event {
            SoundEvent::Effect(f) => (f, 0.9),
            SoundEvent::Music(f) => (f, 0.6),
        };
        if !pcm.contains_key(file) {
            match read_sound(&s.dir, file, raw_rate_from_env()) {
                Ok(p) => {
                    pcm.insert(file.clone(), p);
                }
                Err(e) => {
                    warn(format!("sound {file}: {e}"));
                    continue;
                }
            }
        }
        let max_secs = if matches!(event, SoundEvent::Music(_)) {
            let next = music_starts.iter().copied().find(|t| *t > *start + 1e-9).unwrap_or(total);
            placed_music += 1;
            Some((next - start).min(total - start))
        } else {
            None
        };
        clips.push((start, file.clone(), gain, max_secs));
    }
    let placed: Vec<Placed> = clips.iter().filter_map(|(start, file, gain, max_secs)| pcm.get(file).map(|p| Placed { start: **start, pcm: p, gain: *gain, max_secs: *max_secs })).collect();
    let wav = mix_wav(&placed, total);
    let wav_path = out.with_extension("mix.wav");
    std::fs::write(&wav_path, &wav).map_err(|e| format!("{}: {e}", wav_path.display()))?;
    let muxed = out.with_extension("muxed.mp4");
    let status = Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(out)
        .arg("-i")
        .arg(&wav_path)
        .args(["-map", "0:v", "-map", "1:a", "-c:v", "copy", "-c:a", "aac", "-b:a", "192k", "-shortest"])
        .arg(&muxed)
        .status()
        .map_err(|e| format!("cannot start ffmpeg for the sound: {e}"))?;
    if !status.success() {
        return Err(format!("ffmpeg muxing the sound ended with {status}"));
    }
    std::fs::rename(&muxed, out).map_err(|e| format!("{}: {e}", out.display()))?;
    let _ = std::fs::remove_file(&wav_path);
    log(format!("sound: {} events, {placed_music} music tracks, muxed into {}", starts.len(), out.display()));
    Ok(())
}

local! {
    static RUNTIME: RefCell<Runtime> = RefCell::new(Runtime::default());
}

fn with<T>(f: impl FnOnce(&mut Runtime) -> T) -> T {
    RUNTIME.with(|r| f(&mut r.borrow_mut()))
}

/// The fly mode's camera, in the map's own units (the target is a point of the map's centres).
#[derive(Clone, Copy, Debug)]
struct Fly {
    target: (f32, f32),
    pitch: f32,
    yaw: f32,
    dist: f32,
    fov: f32,
    speed: f32,
}

/// Reads `razdor --trailer …` arguments (or the environment) into the run's configuration and
/// prints any error of the shot file. Returns an exit code when the run must end here.
pub fn configure(args: &[String]) -> Option<i32> {
    let cfg = match from_args(args) {
        Ok(cfg) => cfg,
        Err(e) => {
            eprintln!("{e}");
            return Some(2);
        }
    };
    if let Some(c) = &cfg {
        if let Some(shot) = &c.shot {
            shotfile_log(c, shot);
        }
    }
    let _ = CONFIG.set(cfg);
    if let Some(c) = config() {
        if let Some(shot) = &c.shot {
            super::clock::set_shot(shot.fps);
        }
    }
    None
}

fn shotfile_log(c: &Config, shot: &Shot) {
    razdor::diag!("trailer: {:?} of {:?}: {} frames at {} fps, {}x{}", c.mode, c.shot_path, shot.frame_count(), shot.fps, c.size.0, c.size.1);
}

/// The arguments read: `--trailer <shot>`, `--plan`, `--preview`, `--out <mp4>`, `--frames <dir>`,
/// `--hashes <file>`, `--encoder <name>`, `--every <n>`. The environment asks the same with
/// `RAZDOR_TRAILER_SHOT`, `RAZDOR_TRAILER_FLY`, `RAZDOR_TRAILER_RECORD` (an MP4 path).
fn from_args(args: &[String]) -> Result<Option<Config>, String> {
    let value = |flag: &str| args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).cloned();
    let fly = std::env::var("RAZDOR_TRAILER_FLY").is_ok_and(|v| v.trim() == "1");
    let shot_path = value("--trailer").or_else(|| std::env::var("RAZDOR_TRAILER_SHOT").ok().filter(|s| !s.is_empty()));
    if fly && shot_path.is_none() {
        return Ok(Some(Config { mode: Mode::Fly, shot: None, shot_path: None, out: None, frames: None, hashes: None, encoder: String::new(), size: (1280, 720), every: 1 }));
    }
    let Some(path) = shot_path else { return Ok(None) };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: cannot read the shot file: {e}"))?;
    let shot = match shotfile::parse(&text) {
        Ok(shot) => shot,
        Err(errors) => return Err(format!("{path}:\n  {}", errors.join("\n  "))),
    };
    let plan = args.iter().any(|a| a == "--plan");
    let preview = args.iter().any(|a| a == "--preview");
    let mode = match (plan, preview) {
        (true, _) => Mode::Plan,
        (_, true) => Mode::Preview,
        _ => Mode::Render,
    };
    let size = match mode {
        Mode::Plan => (64, 64.max(64 * shot.height / shot.width.max(1))),
        Mode::Preview => (640, (640 * shot.height / shot.width.max(1)) & !1),
        _ => (shot.width, shot.height),
    };
    let out = value("--out").or_else(|| std::env::var("RAZDOR_TRAILER_RECORD").ok()).map(PathBuf::from);
    let frames = value("--frames").map(PathBuf::from);
    let hashes = value("--hashes").map(PathBuf::from);
    let every = value("--every").and_then(|v| v.parse().ok()).unwrap_or(10u64).max(1);
    let encoder = value("--encoder").unwrap_or_else(|| "h264_nvenc".to_string());
    Ok(Some(Config { mode, shot: Some(shot), shot_path: Some(PathBuf::from(path)), out, frames, hashes, encoder, size, every }))
}

fn config() -> Option<&'static Config> {
    CONFIG.get().and_then(|c| c.as_ref())
}

/// Whether a trailer run is on (a shot, its plan or preview, or the fly mode).
pub fn active() -> bool {
    config().is_some()
}

/// Whether the interface is hidden: every trailer run hides it, unless its shot says `ui on`.
pub fn hides_ui() -> bool {
    active() && !config().and_then(|c| c.shot.as_ref()).is_some_and(|s| s.ui)
}

/// Whether the fog of war is hidden: every trailer run hides it, unless its shot says `fog on`.
pub fn hides_fog() -> bool {
    active() && !config().and_then(|c| c.shot.as_ref()).is_some_and(|s| s.fog)
}

/// Whether the shot is the classic flat map (`view flat`): its camera is the player's flat one.
pub fn flat_view() -> bool {
    config().and_then(|c| c.shot.as_ref()).is_some_and(|s| s.flat)
}

/// The window's size a run asks for (its shot's), if one does.
pub fn window_size() -> Option<(u32, u32)> {
    config().map(|c| c.size)
}

/// Whether a battle is drawn on its screen (`battles show`).
pub fn battles_show() -> bool {
    config().and_then(|c| c.shot.as_ref()).is_some_and(|s| s.battles_show)
}

/// The fitted distance the last frame's camera had (`world_view::trailer_camera` reports it).
pub fn note_fitted_distance(d: f32) {
    FITTED.with(|f| f.set(d));
}

local! {
    static FITTED: std::cell::Cell<f32> = const { std::cell::Cell::new(0.0) };
}

/// The camera of this frame, for the world's drawing (the phase-1 keys of `view25::Trailer`).
pub fn camera() -> Option<Trailer> {
    if !active() {
        return None;
    }
    with(|r| r.cam)
}

/// The point the camera looks at this frame, in the map's centre units (`Game::display_pos`).
pub fn target() -> Option<(f32, f32)> {
    if !active() {
        return None;
    }
    with(|r| r.target)
}

/// The shot's event answers: the answer for a question of event `id` (`true` for Yes): the
/// first button unless the shot picks another.
pub fn auto_close(d: &Dialog) -> Option<Close> {
    if !active() {
        return None;
    }
    if d.waiting(super::clock::now()) {
        return None;
    }
    let shot = config().and_then(|c| c.shot.as_ref());
    let choice = |id: Option<u16>| shot.and_then(|s| id.and_then(|i| s.events.get(&(i as u32)).copied())).unwrap_or(0);
    if d.question || d.offer {
        let yes = choice(d.event) == 0;
        if let Some(event) = d.event {
            log(format!("event {event}: {}", if yes { "yes" } else { "no" }));
        }
        return Some(if yes { Close::Yes } else { Close::No });
    }
    Some(Close::Ok)
}

/// The battle met on the map is fought at once, by the battle AI on both sides, to its end (the
/// same resolution as the battle window's end, `BattleView::close_won` and `close_lost`). Not
/// when `battles show` draws it. Returns the screen after the fight, if it changes.
pub fn battle_on_map(game: &mut Game, message: &mut Option<String>, dialogs: &mut VecDeque<Dialog>) -> Option<Screen> {
    if !active() || battles_show() {
        return None;
    }
    let mut battle = game.start_battle();
    battle.auto_play_to_end();
    let result = game.resolve_battle(&battle);
    let what = match result {
        BattleResult::Victory { .. } => "won",
        BattleResult::Defeat => "lost",
        BattleResult::Withdrew { .. } => "withdrew",
    };
    log(format!("battle on the map at {:.2}s: {what}", clock_now()));
    match result {
        BattleResult::Defeat => {
            dialogs.push_back(Dialog::defeat(false));
            Some(Screen::WorldMap)
        }
        BattleResult::Victory { .. } if game.won() => Some(Screen::Victory),
        victory @ BattleResult::Victory { .. } => {
            dialogs.extend(Dialog::victory(&victory));
            *message = None;
            Some(Screen::WorldMap)
        }
        BattleResult::Withdrew { .. } => Some(Screen::WorldMap),
    }
}

fn clock_now() -> f64 {
    super::clock::now()
}

/// Writes a line of the run's report: to stdout for the planner, and to the log.
fn log(line: String) {
    if config().is_some_and(|c| c.mode == Mode::Plan) {
        println!("{line}");
    }
    razdor::diag!("trailer: {line}");
}

/// Warnings go to stderr too, so a render's problems show without the log.
fn warn(line: String) {
    eprintln!("trailer warning: {line}");
    log(format!("warning: {line}"));
}

/// Sets up the run's map at the first frame, then the camera and the timed actions of this
/// frame. Called before the app draws the frame.
pub fn before_frame(app: &mut App) {
    let Some(cfg) = config() else { return };
    if !with(|r| r.started) {
        with(|r| r.started = true);
        if let Err(e) = start(app, cfg) {
            eprintln!("trailer: {e}");
            with(|r| r.done = true);
            return;
        }
        if cfg.mode == Mode::Render || cfg.mode == Mode::Preview {
            if let Err(e) = open_sinks(cfg) {
                eprintln!("trailer: {e}");
                with(|r| r.done = true);
                return;
            }
            if let Some(shot) = cfg.shot.as_ref() {
                let rec = sound_for(app, shot);
                if rec.is_none() {
                    warn("no sound: the install's sound table cannot be read".to_string());
                }
                with(|r| r.sound = rec);
            }
        }
    }
    super::world_view::set_tilt_view(!flat_view());
    if cfg.mode == Mode::Fly {
        fly_frame(app);
        return;
    }
    let Some(shot) = cfg.shot.as_ref() else { return };
    let f = super::clock::frame_index();
    let t = shotfile::frame_time(f, shot.fps);
    if let (Screen::Battle(view), Some(pace)) = (&mut app.screen, shot.battle_pace(t)) {
        view.set_trailer_pace(pace);
    }
    let values = shotfile::sample_values(&shot.keys, t);
    let target = shotfile::sample_target(&shot.keys, t);
    let cam = camera_of(&values);
    if shot.flat {
        // The classic flat map: the player's own camera, its zoom set by the shot (locked, as the zoom
        // setting is when it is locked).
        super::world_view::set_zoom_prefs(values.zoom.unwrap_or(1.0), true);
    }
    if let (Some((a, b, _)), Some(game)) = (target, app.game.as_ref()) {
        for t in [a, b] {
            if let Some(id) = gone_army(game, t) {
                if !with(|r| r.gone_warned) {
                    with(|r| r.gone_warned = true);
                    warn(format!("target army A{id} is gone at {t:?}, {:.2}s: the camera has no target there", shotfile::frame_time(f, shot.fps)));
                }
            }
        }
    }
    let game_target = target.and_then(|(a, b, e)| {
        let game = app.game.as_ref()?;
        let (pa, pb) = (resolve(game, a)?, resolve(game, b)?);
        Some((pa.0 + (pb.0 - pa.0) * e, pa.1 + (pb.1 - pa.1) * e))
    });
    with(|r| {
        r.cam = (!shot.flat).then_some(cam);
        r.target = game_target;
    });
    if cfg.mode == Mode::Plan {
        report_keys(app, shot, t);
    }
    start_due_actions(app, shot, t);
    continue_walk(app);
    continue_pass(app);
}

/// The camera a shot's values give: the phase-1 keys, the player's defaults where the shot
/// names none.
fn camera_of(v: &Values) -> Trailer {
    let mut t = Trailer::default();
    t.pitch = v.pitch.map(|p| p.min(super::view25::FREE_PITCH_MAX));
    t.fov = v.fov.or(Some(FOV_DEG));
    t.dist = v.dist;
    t.fit = v.fit;
    t.bend = v.bend.unwrap_or(0.0);
    t.flat = v.flat.unwrap_or(t.flat);
    t.radius = v.radius.unwrap_or(t.radius);
    t.width = v.width.unwrap_or(t.width);
    t.ahead = v.ahead.unwrap_or(t.ahead);
    t.behind = v.behind.unwrap_or(t.behind);
    t.near = v.near.unwrap_or(t.near);
    t.yaw = v.yaw;
    // A 0/1 key is on from the middle of its segment (the sampled value's half).
    t.eye = v.eye.is_some_and(|e| e >= 0.5);
    t.horizon = v.horizon.is_some_and(|h| h >= 0.5);
    t.height = v.height.unwrap_or(t.height);
    t
}

/// A target in the map's centre units: a cell (bilinear between the four cells around a
/// fractional one), or the hero's cell (`display_pos`, which glides between cells) plus an
/// offset in cells.
fn resolve(game: &Game, t: Target) -> Option<(f32, f32)> {
    let map = &game.world.map;
    match t {
        Target::Cell(x, y) => {
            let (fx, fy) = (x.floor(), y.floor());
            let (ix, iy) = (fx as i32, fy as i32);
            let c = |dx: i32, dy: i32| map.center((ix + dx, iy + dy));
            let (ax, ay) = (x - fx, y - fy);
            let (c00, c10, c01, c11) = (c(0, 0), c(1, 0), c(0, 1), c(1, 1));
            let lerp2 = |a: (f32, f32), b: (f32, f32), u: f32| (a.0 + (b.0 - a.0) * u, a.1 + (b.1 - a.1) * u);
            let top = lerp2(c00, c10, ax);
            let bottom = lerp2(c01, c11, ax);
            Some(lerp2(top, bottom, ay))
        }
        Target::Hero(dx, dy) => {
            let here = game.display_pos();
            let tile = game.tile();
            let base = map.center(tile);
            let off = map.center((tile.0 + dx.round() as i32, tile.1 + dy.round() as i32));
            Some((here.0 + off.0 - base.0, here.1 + off.1 - base.1))
        }
        // The army's displayed place (it glides between cells as it marches), its id as the file
        // names it. None once the army is gone (`warn_gone_armies` says so).
        Target::Army(id, dx, dy) => {
            let a = game.world.armies.iter().find(|a| a.id == id)?;
            let here = game.army_display_pos(a);
            let tile = a.tile(map);
            let base = map.center(tile);
            let off = map.center((tile.0 + dx.round() as i32, tile.1 + dy.round() as i32));
            Some((here.0 + off.0 - base.0, here.1 + off.1 - base.1))
        }
    }
}

/// The army a target names, when the map has no such army any more (a clash can remove one without
/// a report).
fn gone_army(game: &Game, t: Target) -> Option<u8> {
    match t {
        Target::Army(id, _, _) if !game.world.armies.iter().any(|a| a.id == id) => Some(id),
        _ => None,
    }
}

/// Starts the run's map: a new game on the scenario (with the shot's hero and seed) or the
/// save, the first frame's opening events waiting for the map frame to show them.
fn start(app: &mut App, cfg: &Config) -> Result<(), String> {
    // The fly mode starts a map of its own (`RAZDOR_TRAILER_FLY_MAP`, the first campaign map by
    // default): a shot names its own.
    let fly_map = std::env::var("RAZDOR_TRAILER_FLY_MAP").ok().filter(|m| !m.trim().is_empty()).unwrap_or_else(|| "РК1-Начало пути".to_string());
    let (map, hero, seed) = match cfg.shot.as_ref() {
        Some(shot) if cfg.mode != Mode::Fly => (shot.map.clone().ok_or("the shot names no map")?, shot.hero.clone(), shot.seed),
        _ => (MapRef::Scenario(fly_map), "knight".to_string(), 0x5EED),
    };
    let class = match hero.as_str() {
        "archmage" => HeroClass::Archmage,
        "ranger" => HeroClass::Ranger,
        _ => HeroClass::Knight,
    };
    let install = app.assets.dt.as_ref().zip(app.dt_content.clone()).map(|(d, content)| razdor::rules::save::Install { dir: &d.install.dir, content });
    let mut game = match &map {
        MapRef::Scenario(name) => {
            let index = app.scenarios.iter().position(|e| e.file == *name).ok_or_else(|| format!("no map {name} in the install"))?;
            let content = app.dt_content.clone().ok_or("no install content")?;
            let content = Arc::new(content.with_experimental(super::new_game::experimental_rules()));
            let mut game = Game::from_scenario(content, &app.scenarios[index].scenario, class);
            game.seed_events(seed);
            game
        }
        MapRef::Save(path) => {
            let mut game = razdor::rules::save::load(Path::new(path), app.demo.clone(), install.as_ref()).map_err(|e| format!("cannot load the save {path}: {e}"))?;
            game.seed_events(seed);
            game
        }
    };
    game.look_around();
    game.pos = game.world.map.center(game.tile());
    if cfg.mode == Mode::Fly {
        let here = game.display_pos();
        with(|r| r.fly = Some(Fly { target: here, pitch: 55.0, yaw: 0.0, dist: 30.0, fov: FOV_DEG, speed: 6.0 }));
        log(format!("fly: map {map:?}; WASD, Q/E, right-drag, wheel; F8 prints the camera"));
    } else {
        log(format!("start: map {map:?}, hero {hero}, seed {seed}"));
    }
    let mut battle_screen = None;
    if let Some(shot) = cfg.shot.as_ref().filter(|_| cfg.mode != Mode::Fly) {
        if let Some(hour) = shot.hour {
            set_start_hour(&mut game, hour);
            log(format!("hour: {hour} (the clock {})", game.clock.label()));
        }
        if shot.clock_hold {
            let held = clock_hour(&game);
            super::daylight::set_hold(Some(held));
            log(format!("clock hold at {held:.2} h"));
        }
        apply_look(&mut app.audio.settings, &shot.look);
        if !shot.look.is_empty() {
            log(format!("look: {:?} (for this run only)", shot.look));
        }
        if let Some(id) = shot.battle {
            // The battle starts at frame 0: the hero's army against the army `battle` names. On the
            // screen (`battles show`) it opens now, as a save of a fight does; on the map the fight
            // is resolved by `battle_on_map` at the first frame the map is free.
            let i = game.world.armies.iter().position(|a| u32::from(a.id) == id).ok_or_else(|| format!("battle: no army A{id} on the map"))?;
            game.foe = Some(razdor::rules::game::Foe::Army(i));
            log(format!("battle: A{id} against the hero at {:?}, {}", game.tile(), if shot.battles_show { "on its screen" } else { "on the map" }));
            if shot.battles_show {
                battle_screen = Some(BattleView::new(game.start_battle()));
            }
        }
    }
    app.assets.set_content(game.content.clone());
    app.screen = match battle_screen {
        Some(view) => Screen::Battle(Box::new(view)),
        None => Screen::WorldMap,
    };
    app.game = Some(game);
    razdor::diag!("trailer: renderer: {}", gl_renderer());
    Ok(())
}

/// The shot's `hour`: the game's clock moves to that hour of its day. No time passes: the armies
/// stay where they are and no event fires.
fn set_start_hour(game: &mut Game, hour: f64) {
    let day_start = (game.clock.total_minutes() / 1440.0).floor() * 1440.0;
    game.clock = razdor::rules::clock::Clock::at_minutes((day_start + hour * 60.0).round() as u64);
}

/// The hour of the game's clock, fractions included.
fn clock_hour(game: &Game) -> f32 {
    (game.clock.total_minutes().rem_euclid(1440.0) / 60.0) as f32
}

/// The shot's `look` lines, taken over for this run: the settings of the run are changed in memory
/// only, the settings file is never written. `preset` (the «Качество графики» presets) goes first,
/// then the keys in file order.
fn apply_look(s: &mut Settings, look: &[(String, String)]) {
    use super::daylight::{LightMode, ShadowMode};
    use super::map_anim::Mode as Anim;
    use super::mist::MistMode;
    use super::quality_preset::{self, Preset};
    use super::water::{FrameRate, Quality};
    if let Some((_, p)) = look.iter().rev().find(|(k, _)| k == "preset") {
        let preset = match p.as_str() {
            "original" | "оригинал" => Preset::Original,
            "low" => Preset::Low,
            "medium" => Preset::Medium,
            _ => Preset::High,
        };
        quality_preset::set(s, preset);
    }
    for (k, v) in look {
        match (k.as_str(), v.as_str()) {
            ("lighting", "off") => s.map_light = LightMode::Off,
            ("lighting", "on") => s.map_light = LightMode::GameClock,
            ("lighting", "cycle") => s.map_light = LightMode::Cycle,
            ("lighting", "fixed") => s.map_light = LightMode::Fixed,
            ("shadows", "painted") => s.map_shadows = ShadowMode::Painted,
            ("shadows", "sun") => s.map_shadows = ShadowMode::Sun,
            ("water", on) => s.map_live_water = on == "on",
            ("water_rate", "every") => s.map_water_rate = FrameRate::EveryFrame,
            ("water_rate", "other") => s.map_water_rate = FrameRate::EveryOther,
            ("water_rate", "third") => s.map_water_rate = FrameRate::EveryThird,
            ("water_quality", "high") => s.map_water_quality = Quality::High,
            ("water_quality", "medium") => s.map_water_quality = Quality::Medium,
            ("water_quality", "low") => s.map_water_quality = Quality::Low,
            ("weather", on) => s.map_weather = on == "on",
            ("living", on) => s.map_living = on == "on",
            ("mist", "off") => s.map_mist = MistMode::Off,
            ("mist", "flat") => s.map_mist = MistMode::Flat,
            ("mist", "clouds") => s.map_mist = MistMode::Clouds,
            ("anim", "on") => s.map_anim = Anim::On,
            ("anim", "near") => s.map_anim = Anim::Near,
            ("anim", "off") => s.map_anim = Anim::Off,
            _ => {}
        }
    }
}

/// The GL renderer's name (`GL_RENDERER`), for the run's log: the GPU that draws the frames (under
/// VirtualGL, the EGL device it opened). Empty when the library cannot be read. Called once the
/// context is current, at the first frame.
fn gl_renderer() -> String {
    type GlGetString = unsafe extern "C" fn(u32) -> *const std::ffi::c_char;
    const GL_RENDERER: u32 = 0x1F01;
    // SAFETY: the symbol is `glGetString` from the GL library miniquad has loaded (or loads here);
    // it is called with a valid name while the context is current.
    unsafe {
        let mut lib = libc::dlopen(c"libGL.so.1".as_ptr(), libc::RTLD_NOW | libc::RTLD_NOLOAD);
        if lib.is_null() {
            lib = libc::dlopen(c"libGL.so.1".as_ptr(), libc::RTLD_NOW);
        }
        if lib.is_null() {
            return String::new();
        }
        let sym = libc::dlsym(lib, c"glGetString".as_ptr());
        if sym.is_null() {
            return String::new();
        }
        let get: GlGetString = std::mem::transmute::<*mut libc::c_void, GlGetString>(sym);
        let s = get(GL_RENDERER);
        if s.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned()
    }
}

/// The timed actions due at `t` start now (each once).
fn start_due_actions(app: &mut App, shot: &Shot, t: f64) {
    loop {
        let Some(action) = shot.actions.get(with(|r| r.next_action)) else { break };
        if action.t() > t + 1e-9 {
            break;
        }
        with(|r| r.next_action += 1);
        let Some(game) = app.game.as_mut() else { break };
        match action {
            Action::Walk { route, t, .. } => {
                log(format!("walk at {t:.2}s: {route:?}"));
                with(|r| r.route = route.iter().copied().collect());
                let map_w = game.world.map.w;
                let map_h = game.world.map.h;
                for &(x, y) in route.iter().filter(|&&(x, y)| x < 0 || y < 0 || x >= map_w || y >= map_h) {
                    warn(format!("walk at {t:.2}s: cell {x},{y} is off the map"));
                }
            }
            Action::Pass { hours, t, .. } => {
                if game.foe.is_some() {
                    warn(format!("pass at {t:.2}s: a battle is pending, nothing passes"));
                    continue;
                }
                log(format!("pass at {t:.2}s: {hours} hours"));
                game.begin_wait(*hours);
                with(|r| r.pass_until = Some(game.clock.total_minutes() + 60.0 * *hours as f64));
            }
            Action::Reveal { cell, radius, t, .. } => {
                log(format!("reveal at {t:.2}s: {cell:?} radius {radius}"));
                game.reveal_area(*cell, *radius);
            }
        }
    }
}

/// The `pass` under way: when the game's wait has stopped short (a day report, a window), it is
/// taken up again for the hours that remain, once no window is open and the hero stands still.
fn continue_pass(app: &mut App) {
    let Some(until) = with(|r| r.pass_until) else { return };
    let Some(game) = app.game.as_mut() else { return };
    let now = game.clock.total_minutes();
    if now + 1e-6 >= until {
        with(|r| r.pass_until = None);
        log(format!("pass done: {}", game.clock.label()));
        return;
    }
    if game.waiting() || game.moving() || game.foe.is_some() || !app.dialogs.is_empty() {
        return;
    }
    let hours = ((until - now) / 60.0).ceil() as u32;
    game.begin_wait(hours.max(1));
}

/// The walk under way: the next cell of its route once the hero stands on the last one, or a
/// warning when the way is blocked.
fn continue_walk(app: &mut App) {
    let Some(game) = app.game.as_mut() else { return };
    let tile = game.tile();
    with(|r| {
        while r.route.front() == Some(&tile) {
            r.route.pop_front();
        }
    });
    if game.moving() || game.foe.is_some() {
        return;
    }
    let Some(next) = with(|r| r.route.front().copied()) else { return };
    if !game.set_destination(next) {
        warn(format!("the way to {next:?} is blocked (the hero stands at {tile:?})"));
        with(|r| r.route.clear());
    }
}

/// The planner's report at a keyframe: where the hero and every army stand.
fn report_keys(app: &App, shot: &Shot, t: f64) {
    let Some(game) = app.game.as_ref() else { return };
    loop {
        let next = with(|r| r.next_report);
        let Some(key) = shot.keys.get(next) else { return };
        if t + 1e-9 < key.t {
            return;
        }
        with(|r| r.next_report += 1);
        let map = &game.world.map;
        let fitted = FITTED.with(|f| f.get());
        let mut line = format!("keyframe {:.2}s: hero at {:?}, date {}, fitted distance {fitted:.1}", key.t, game.tile(), game.clock.label());
        for a in &game.world.armies {
            let side = if a.hostile() { "enemy" } else { "friend" };
            line.push_str(&format!("\n    army A{} {side} at {:?}", a.id, a.tile(map)));
        }
        log(line);
        if let Some(id) = key.target.and_then(|t| gone_army(game, t)) {
            warn(format!("keyframe {:.2}s: target army A{id} is gone (no army with that id on the map)", key.t));
        }
    }
}

/// The fly mode's frame: the keys and the mouse move the camera, F8 prints it.
fn fly_frame(app: &mut App) {
    let dt = get_frame_time().min(0.1);
    let fly = with(|r| r.fly);
    let Some(mut fly) = fly else { return };
    let fast = is_key_down(KeyCode::LeftShift) || is_key_down(KeyCode::RightShift);
    let speed = fly.speed * if fast { 4.0 } else { 1.0 } * (fly.dist / 30.0).max(0.2);
    let (fwd, right) = (super::view25::forward(fly.yaw), super::view25::right(fly.yaw));
    let mut step = (0.0f32, 0.0f32);
    let mut add = |v: Vec2, k: f32| {
        step.0 += v.x * k;
        step.1 += v.y * k;
    };
    if is_key_down(KeyCode::W) {
        add(fwd, 1.0);
    }
    if is_key_down(KeyCode::S) {
        add(fwd, -1.0);
    }
    if is_key_down(KeyCode::D) {
        add(right, 1.0);
    }
    if is_key_down(KeyCode::A) {
        add(right, -1.0);
    }
    fly.target.0 += step.0 * speed * dt;
    fly.target.1 += step.1 * speed * dt;
    if is_key_down(KeyCode::Q) {
        fly.dist = (fly.dist * (1.0 + dt * 1.5)).min(400.0);
    }
    if is_key_down(KeyCode::E) {
        fly.dist = (fly.dist / (1.0 + dt * 1.5)).max(2.0);
    }
    if is_mouse_button_down(MouseButton::Right) {
        let (dx, dy) = mouse_delta_position().into();
        fly.yaw = (fly.yaw + dx * 180.0).rem_euclid(360.0);
        fly.pitch = (fly.pitch - dy * 90.0).clamp(super::view25::FREE_PITCH_MIN, super::view25::FREE_PITCH_MAX);
    }
    let (_, wheel) = mouse_wheel();
    if wheel != 0.0 {
        fly.fov = (fly.fov - wheel.signum() * 2.0).clamp(5.0, 120.0);
    }
    with(|r| {
        r.cam = Some(Trailer { pitch: Some(fly.pitch), fov: Some(fly.fov), dist: Some(fly.dist), yaw: Some(fly.yaw), ..Trailer::default() });
        r.target = Some(fly.target);
        r.fly = Some(fly);
    });
    if is_key_pressed(KeyCode::F8) {
        let line = format!(
            "at {:.2}s pitch={:.1} yaw={:.1} dist={:.1} fov={:.1} target={:.2},{:.2}",
            super::clock::now(),
            fly.pitch,
            fly.yaw,
            fly.dist,
            fly.fov,
            fly.target.0,
            fly.target.1
        );
        println!("{line}");
        razdor::diag!("trailer fly: {line}");
    }
    let _ = app;
}

/// After the frame is drawn: records it, plays a battle shown on its screen to its end, and says
/// whether the run ends (the window then closes). Called before the clock moves on: the frame
/// just drawn is `frame_index()`.
pub fn after_frame(app: &mut App) -> bool {
    let Some(cfg) = config() else { return false };
    if cfg.mode == Mode::Fly {
        return false;
    }
    // A battle on its screen ends at once (the instant end, the default) unless the shot paces it
    // (`battle_speed`): then the AI plays it on screen, frame by frame, to its own end.
    let paced = cfg.shot.as_ref().is_some_and(Shot::battle_paced);
    if let Screen::Battle(view) = &mut app.screen {
        if battles_show() && !paced && !with(|r| r.battle_played) {
            with(|r| r.battle_played = true);
            view.play_to_end_for_trailer();
        }
    }
    if !with(|r| r.done) && !matches!(app.screen, Screen::WorldMap | Screen::Battle(_) | Screen::Building(_)) {
        warn(format!("the screen left the map ({}); the shot ends here", app.screen_name()));
        with(|r| r.done = true);
    }
    let f = super::clock::frame_index();
    // The battle's state once a second, for the proof that two sizes show the same fight.
    if let (Screen::Battle(view), Some(shot)) = (&app.screen, cfg.shot.as_ref()) {
        if f.is_multiple_of(u64::from(shot.fps)) {
            log(format!("battle at {:.2}s: {}", shotfile::frame_time(f, shot.fps), view.state_line()));
        }
    }
    let shot_frames = cfg.shot.as_ref().map_or(0, Shot::frame_count);
    if cfg.mode == Mode::Plan && f.is_multiple_of(30) {
        let rss = std::fs::read_to_string("/proc/self/status").ok().and_then(|s| s.lines().find(|l| l.starts_with("VmRSS")).map(str::to_string));
        log(format!("frame {f}: {}", rss.unwrap_or_default().trim()));
    }
    if !with(|r| r.done) {
        if cfg.mode == Mode::Render || (cfg.mode == Mode::Preview && f.is_multiple_of(cfg.every)) {
            capture(f);
        }
        if f + 1 >= shot_frames {
            // The keyframes at the shot's very end (after its last frame) are reported with this state.
            if let Some(shot) = cfg.shot.as_ref().filter(|_| cfg.mode == Mode::Plan) {
                report_keys(app, shot, shot.duration);
            }
            with(|r| r.done = true);
        }
    }
    if with(|r| r.done) {
        return finish();
    }
    false
}

/// The pixels of the frame just drawn, opaque, as an image.
fn grab() -> macroquad::prelude::Image {
    let mut image = get_screen_data();
    for px in image.bytes.chunks_exact_mut(4) {
        px[3] = 255;
    }
    image
}

/// Writes the frame `f` to the sinks: the encoder's pipe, a PNG, or its hash line.
fn capture(f: u64) {
    let image = grab();
    with(|r| {
        let Some(sinks) = r.sinks.as_mut() else { return };
        if sinks.write(f, &image) {
            r.written += 1;
        } else {
            r.done = true;
        }
    });
}

/// The run's outputs, opened before the first frame of a render.
struct Sinks {
    child: Option<Child>,
    frames: Option<PathBuf>,
    hashes: Option<std::fs::File>,
    hash_path: Option<PathBuf>,
    out: Option<PathBuf>,
    started: std::time::Instant,
    frame_times: Vec<f64>,
}

impl Sinks {
    /// Writes frame `f`; false when the encoder no longer takes frames (the run then ends).
    fn write(&mut self, f: u64, image: &macroquad::prelude::Image) -> bool {
        if let Some(dir) = &self.frames {
            image.export_png(&dir.join(format!("frame_{f:06}.png")).to_string_lossy());
        }
        if let Some(child) = self.child.as_mut() {
            if let Some(stdin) = child.stdin.as_mut() {
                // The screen read comes bottom row first (macroquad's `export_png` flips it for the PNG
                // frames): the encoder takes the top row first, so the rows go in reverse.
                let stride = image.width as usize * 4;
                if image.bytes.chunks_exact(stride).rev().any(|row| stdin.write_all(row).is_err()) {
                    warn(format!("the encoder stopped taking frames at frame {f}; the render ends"));
                    return false;
                }
            }
        }
        if let Some(h) = self.hashes.as_mut() {
            use sha2::Digest;
            let digest = sha2::Sha256::digest(&image.bytes);
            let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
            let _ = writeln!(h, "{f} {hex}");
        }
        self.frame_times.push(self.started.elapsed().as_secs_f64());
        true
    }
}

fn open_sinks(cfg: &Config) -> Result<(), String> {
    let shot = cfg.shot.as_ref().ok_or("no shot")?;
    let (w, h) = cfg.size;
    let fps = shot.fps;
    let mut child = None;
    if let Some(out) = &cfg.out {
        if let Some(parent) = out.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let rate = if cfg.mode == Mode::Preview { format!("{fps}/{}", cfg.every) } else { fps.to_string() };
        // NVENC's H.264 stops at 4096 pixels a side (a 5K picture has no encoder there): libx264 then.
        let encoder = if cfg.encoder.starts_with("h264_nvenc") && (w > 4096 || h > 4096) { "libx264".to_string() } else { cfg.encoder.clone() };
        if encoder != cfg.encoder {
            log(format!("encoder: {encoder} (the NVENC H.264 encoder stops at 4096 pixels a side)"));
        }
        let mut cmd = Command::new("ffmpeg");
        cmd.args(["-y", "-loglevel", "error", "-f", "rawvideo", "-pix_fmt", "rgba", "-s"])
            .arg(format!("{w}x{h}"))
            .args(["-r", &rate, "-i", "-"]);
        if encoder == "libx264" {
            cmd.args(["-c:v", "libx264", "-preset", "medium", "-crf", "16"]);
        } else {
            cmd.args(["-c:v", &encoder, "-preset", "p6", "-cq", "18", "-rc", "vbr"]);
        }
        cmd.args(["-pix_fmt", "yuv420p", "-r", &rate]).arg(out).stdin(Stdio::piped());
        // The GPU of the trailer runs: the RTX 4060 only (nvidia-smi index 1, PCI order).
        cmd.env("CUDA_DEVICE_ORDER", "PCI_BUS_ID").env("CUDA_VISIBLE_DEVICES", "1");
        let spawned = cmd.spawn().map_err(|e| format!("cannot start ffmpeg: {e}"))?;
        child = Some(spawned);

    }
    if let Some(dir) = &cfg.frames {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let hashes = match &cfg.hashes {
        Some(p) => Some(std::fs::File::create(p).map_err(|e| format!("{}: {e}", p.display()))?),
        None => None,
    };
    with(|r| {
        r.sinks = Some(Sinks {
            child,
            frames: cfg.frames.clone(),
            hashes,
            hash_path: cfg.hashes.clone(),
            out: cfg.out.clone(),
            started: std::time::Instant::now(),
            frame_times: Vec::new(),
        })
    });
    log(format!("render: {w}x{h} at {fps} fps, encoder {}", cfg.encoder));
    Ok(())
}

/// Closes the sinks: the encoder finishes its file, the sound is muxed in. True: the run ends.
fn finish() -> bool {
    let Some(mut sinks) = with(|r| r.sinks.take()) else { return true };
    if let Some(mut child) = sinks.child.take() {
        drop(child.stdin.take());
        match child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => warn(format!("ffmpeg ended with {status}")),
            Err(e) => warn(format!("ffmpeg: {e}")),
        }
    }
    let sound = with(|r| r.sound.take());
    if let (Some(out), Some(s), Some(shot)) = (sinks.out.as_ref(), sound.as_ref(), config().and_then(|c| c.shot.as_ref())) {
        if let Err(e) = mux_sound(out, s, shot.fps, shot.duration) {
            warn(e);
        }
    }
    if let (Some(path), Some(h)) = (sinks.hash_path.take(), sinks.hashes.take()) {
        drop(h);
        razdor::diag!("trailer: hashes written to {}", path.display());
    }
    let total = sinks.started.elapsed().as_secs_f64();
    let frames = sinks.frame_times.len();
    if frames > 0 {
        log(format!("render: {frames} frames in {total:.1} s, {:.1} ms a frame", total * 1000.0 / frames as f64));
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_camera_of_no_keys_is_the_players_default() {
        let t = camera_of(&Values::default());
        assert_eq!(t.pitch, None);
        assert_eq!(t.bend, 0.0);
        assert_eq!(t.fov, Some(FOV_DEG));
    }

    #[test]
    fn the_shot_pitch_of_ninety_is_the_top_down_view() {
        let t = camera_of(&Values { pitch: Some(90.0), ..Values::default() });
        assert_eq!(t.pitch, Some(super::super::view25::FREE_PITCH_MAX));
    }

    #[test]
    fn a_shot_camera_carries_its_yaw_and_bend() {
        let t = camera_of(&Values { yaw: Some(15.0), bend: Some(78.0), ..Values::default() });
        assert_eq!(t.yaw, Some(15.0));
        assert_eq!(t.bend, 78.0);
    }
}
