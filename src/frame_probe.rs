//! The frame probe: where a frame's time goes, summarised as percentiles, for the 8.3 ms
//! (120 Hz phone) target. Dev-only: on when [`crate::diag::profiling`] is, that is
//! `RAZDOR_PROFILE` set or the app folder's `profile` file present (Android has no env vars,
//! so there the file turns it on).
//!
//! Each frame of the main loop ends with [`end_frame`], which stores a [`Sample`]: the frame's
//! [`Kind`] (set by the map, see [`set_kind`]), its work (the game's frame, `main.rs`), its
//! whole time (vsync included), the CPU time of each [`Stage`] ([`time`]) and the texture
//! rebuilds ([`tex_rebuilt`]). Every 5 s it logs one summary line per kind that had frames
//! ([`summary_lines`]); [`flush`] logs the pending ones at exit.
//!
//! Desktop only: `RAZDOR_PROFILE_FRAMES=<csv path>` also writes one row per frame
//! ([`csv_row`], header [`CSV_HEADER`]). Rows are buffered and appended at each summary and at
//! [`flush`], never per frame. The file is truncated when the first rows are written.
//!
//! The CSV also has the draw calls estimated for each frame (`draws`, [`draw`]: the batches the map's
//! sprite and ground draws make, see there) and their vertices (`verts`). With `RAZDOR_GPU_STAGES=1` on
//! desktop, the map frames' GPU stages are timed per stage (`gpu_<stage>_ms`, [`GPU_STAGES`]): by GL
//! timer queries read back a frame or two later, or by `glFinish` on GL 2 and GLES
//! (`ui::gpu_probe::gpu_wait`). Those timers are off by default and cost nothing then.
//!
//! Debug mode, GPU samples (phones and desktop): while «Режим отладки» is on, one frame in every
//! [`GPU_SAMPLE_EVERY`] runs its GPU waits (`ui::gpu_probe::gpu_wait`: flush and wait for the GL queue, the
//! time as its stage). Those frames are slower by design, so they are summarised apart, as `gpu-sample`
//! lines, and the other lines stay the frames as they are. The `profile-gpu` file on Android still waits
//! every frame.
//!
//! The map's effects have a CPU stage each (`water`, `mist`, `light`, `sun-shadow`, `weather`, `ground-cache`,
//! `freeze`: [`EFFECT_STAGES`]) and a GPU stage each (`gpu-water` … `gpu-minimap`: [`GPU_STAGES`]). The summary
//! of a map kind also has the draw calls and vertices estimated for its frames ([`draw`]).
//!
//! The live water is timed as `water` only where the ground cache draws it: the water's picture and the overlay
//! (`world_view::draw_ground`, the cache's branch, with the overlay timed only while the water is alive). Its
//! other path, the ground with no cache (`draw_ground`'s `None` branch, `Mode::Full`), draws the water inside the
//! terrain's own time, so `water` reads zero there (a limit of the stage split, not a missing timer).
//!
//! The stage times are CPU time spent building the frame. Macroquad batches draws and issues
//! the GL calls in `next_frame`, so GL submission and the wait for vsync (`eglSwapBuffers`)
//! land in "present" (total minus work). GPU time itself is not visible here, except for the
//! battle frames' `gpu-backdrop` and `gpu-window` stages and the map frames' `gpu-sky` to
//! `gpu-ui` stages on Android (`ui::gpu_probe`): there the GL queue is flushed and waited for
//! (`glFinish`), which serialises the GPU while profiling.
//!
//! Battle frames have the kinds `battle` (no effect drawn) and `battle-effect` (an effect drawn,
//! see [`mark_effect`]), each with the view it was drawn in (`flat` or `25d`), and the stages
//! `backdrop`, `battle-window` and `effect` (inside the window).

use crate::local;
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

/// The per-frame CSV's columns (`RAZDOR_PROFILE_FRAMES`): the times in ms, `tick` the game's
/// tick, `world` the map with `terrain` inside it, `rebuilds` the texture rebuilds, `draws` the
/// draw calls estimated for the frame and `verts` its vertices ([`draw`]), and the last columns the
/// GPU time of each GPU stage (`GPU_STAGES`, empty when not timed, see `ui::gpu_probe`).
pub const CSV_HEADER: &str = "frame,kind,work_ms,total_ms,tick,terrain,world,fog,minimap,route,look,armies,script,day,snap,autosave,session,ai_move,ai_plan,rebuilds,backdrop,battle_window,effect,gpu_backdrop,gpu_window,view,draws,verts,water,mist,light,sun_shadow,weather,ground_cache,freeze,gpu_backdrop_ms,gpu_window_ms,gpu_sky_ms,gpu_terrain_ms,gpu_marks_ms,gpu_sprites_ms,gpu_fog_ms,gpu_ui_ms,gpu_water_ms,gpu_mist_ms,gpu_light_ms,gpu_sun_shadow_ms,gpu_weather_ms,gpu_ground_cache_ms,gpu_freeze_ms,gpu_minimap_ms";

/// The GPU stages timed per frame, in the CSV's column order (`gpu_<name>_ms`): each is the GPU's
/// time between the marker before it and its own marker (`ui::gpu_probe::gpu_wait`).
pub const GPU_STAGES: [Stage; 16] = [
    Stage::GpuBackdrop,
    Stage::GpuWindow,
    Stage::GpuSky,
    Stage::GpuTerrain,
    Stage::GpuMarks,
    Stage::GpuSprites,
    Stage::GpuFog,
    Stage::GpuUi,
    Stage::GpuWater,
    Stage::GpuMist,
    Stage::GpuLight,
    Stage::GpuSunShadow,
    Stage::GpuWeather,
    Stage::GpuGroundCache,
    Stage::GpuFreeze,
    Stage::GpuMinimap,
];

/// The map's effects' CPU stages, in the CSV's column order (after `verts`).
pub const EFFECT_STAGES: [Stage; 7] = [Stage::Water, Stage::Mist, Stage::Light, Stage::SunShadow, Stage::Weather, Stage::GroundCache, Stage::Freeze];

/// The period of the debug mode's GPU samples: one frame in this many seconds runs its GPU waits.
pub const GPU_SAMPLE_EVERY: Duration = Duration::from_secs(10);

/// The longest batch of vertices macroquad issues as one draw call (`draw_call_vertex_capacity`).
const MAX_BATCH_VERTS: u32 = 10000;

/// The parts of a frame that are timed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Stage {
    /// The game's tick (`Game::tick_shown_within`): the hero's walk, the armies, the AI.
    Tick,
    /// The terrain layer of the map (inside [`Stage::World`]).
    Terrain,
    /// The whole world map: terrain, objects and sprites (`draw_world`).
    World,
    /// The fog layer over the map.
    Fog,
    /// The minimap window.
    Minimap,
    /// The hero's route: the army he follows and the reroute around armies (`begin_walk`, in the
    /// tick).
    Route,
    /// The fog reveal around the hero (`look_around`, in the tick).
    Look,
    /// The AI armies' other moves (the gangs), the midnights among them and the rest of the
    /// slice (`slice_tail`, in the tick's time).
    Armies,
    /// The scenario's events (`run_script`, in the tick's time).
    Script,
    /// The noon payment and report (`new_day`, in the tick's time).
    Day,
    /// The armies' snap due from the last stop (`armies_snap`, at the tick's start).
    Snap,
    /// The autosave the noon report asks for. It runs outside the tick (in the UI's frame).
    Autosave,
    /// The session slot's look at the game (`ui::session::Keeper::tick`). It runs outside the
    /// tick (in the UI's frame).
    Session,
    /// The AI armies' moves and plans (`ai_move_run`, inside [`Stage::Armies`]). Includes
    /// [`Stage::AiPlan`].
    AiMove,
    /// The AI's plans of the armies that replanned (`ai_plan`, inside [`Stage::AiMove`]).
    AiPlan,
    /// The battle's map backdrop (`world_view::backdrop`, behind the battle window).
    Backdrop,
    /// The battle's own window: the cards, the bar, the effects (`BattleView::draw`). Includes
    /// [`Stage::Effect`].
    BattleWindow,
    /// The battle's effect pictures (`draw_effect`, inside [`Stage::BattleWindow`]).
    Effect,
    /// Android only: the GL queue flushed and waited for after the backdrop (the GPU's time for
    /// it, and the flush's CPU time).
    GpuBackdrop,
    /// Android only: the GL queue flushed and waited for after the battle window.
    GpuWindow,
    /// Android only, map frames: the GL queue flushed and waited for after the sky (2.5D).
    GpuSky,
    /// Android only, map frames: after the terrain (the ground pass).
    GpuTerrain,
    /// Android only, map frames: after the marks, the route and the border (ground passes).
    GpuMarks,
    /// Android only, map frames: after the standing sprites (the sorted pass).
    GpuSprites,
    /// Android only, map frames: after the fog.
    GpuFog,
    /// Android only, map frames: after everything else the frame draws (the minimap, the bar,
    /// the windows and the cursor), at the frame's end.
    GpuUi,
    /// The map's live water: the ship wakes and the water's picture (CPU).
    Water,
    /// The mist over the unexplored land (CPU).
    Mist,
    /// The lights: the night's light over the world, the lit windows, the light pools and their shadows (CPU).
    Light,
    /// The sun's shadows, the baked map (CPU).
    SunShadow,
    /// The weather: rain, snow, haze and cloud shadows (CPU).
    Weather,
    /// The ground cache: its rebuild and its blit (CPU).
    GroundCache,
    /// The map under a window (`map_freeze`): the copy shown in place of the world (CPU).
    Freeze,
    /// GPU waits after the effects above, and the minimap window (`gpu-water` … `gpu-minimap`): the GPU
    /// time of the draws since the previous wait (`ui::gpu_probe::gpu_wait`).
    GpuWater,
    GpuMist,
    GpuLight,
    GpuSunShadow,
    GpuWeather,
    GpuGroundCache,
    GpuFreeze,
    GpuMinimap,
}

impl Stage {
    pub const COUNT: usize = 41;
}

/// What the frame was: the map with the hero walking, or not, or a battle (`tilt`: drawn in
/// the 2.5D view).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    /// The map, the hero not walking (`tilt`: drawn in the 2.5D view).
    Idle { tilt: bool },
    /// The hero walking, no step this frame.
    Walk { tilt: bool },
    /// A frame in which the hero moved to another cell.
    WalkStep { tilt: bool },
    /// A battle frame with no effect drawn.
    Battle { tilt: bool },
    /// A battle frame in which an effect was drawn.
    BattleEffect { tilt: bool },
    /// Any other screen.
    Other,
}

/// The kinds in the summary's order.
const KINDS: [Kind; 11] = [
    Kind::WalkStep { tilt: false },
    Kind::WalkStep { tilt: true },
    Kind::Walk { tilt: false },
    Kind::Walk { tilt: true },
    Kind::Idle { tilt: false },
    Kind::Idle { tilt: true },
    Kind::Other,
    Kind::Battle { tilt: false },
    Kind::BattleEffect { tilt: false },
    Kind::Battle { tilt: true },
    Kind::BattleEffect { tilt: true },
];

/// One frame's numbers, in milliseconds.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub kind: Kind,
    pub work_ms: f32,
    pub total_ms: f32,
    pub stage_ms: [f32; Stage::COUNT],
    pub rebuilds: u32,
    /// Standing sprites drawn this frame, and their summed area in pixels of the pass's 2D space.
    pub sprites: u32,
    pub sprite_px: f32,
    /// The terrain layer's summed draw area in pixels of the pass's 2D space.
    pub terrain_px: f32,
    /// Draw calls estimated for the frame, and the vertices drawn ([`draw`]).
    pub draws: u32,
    pub verts: u32,
    /// The frame ran its GPU waits as a debug-mode sample ([`GPU_SAMPLE_EVERY`]): summarised apart.
    pub gpu_sampled: bool,
}

struct State {
    /// This frame's time per stage (calls of one stage add up).
    stage: [Duration; Stage::COUNT],
    rebuilds: u32,
    sprites: u32,
    sprite_px: f32,
    terrain_px: f32,
    kind: Kind,
    /// An effect was drawn this frame ([`mark_effect`]).
    effect: bool,
    samples: Vec<Sample>,
    /// When the samples since the last summary began.
    since: Option<Instant>,
    /// Frames probed so far (the CSV's `frame` column counts from 1).
    frames: u64,
    /// Draw calls estimated this frame ([`draw`]) and the vertices drawn.
    draws: u32,
    verts: u32,
    /// The texture of the open batch ([`draw`]), its vertices, and whether a state change broke it
    /// ([`batch_break`]) since its last draw.
    batch_key: Option<u64>,
    batch_verts: u32,
    batch_dirty: bool,
    /// The GPU time of each of [`GPU_STAGES`] by frame, as the frame's markers measured it.
    gpu_frames: BTreeMap<u64, [f32; GPU_STAGES.len()]>,
    /// CSV rows not yet appended to the file (`RAZDOR_PROFILE_FRAMES`): the frame and its sample.
    csv_rows: Vec<(u64, Sample)>,
    /// The GPU times of the timed terrain draws (`ui::gpu_probe`), with the kind of their frame,
    /// since the last summary.
    gpu: Vec<(Kind, f32)>,
    /// This frame runs its GPU waits as a debug-mode sample ([`gpu_sampled_now`]), and when the last
    /// sample began.
    gpu_sampling: bool,
    sample_last: Option<Instant>,
}

impl State {
    const fn new() -> State {
        State {
            stage: [Duration::ZERO; Stage::COUNT],
            rebuilds: 0,
            sprites: 0,
            sprite_px: 0.0,
            terrain_px: 0.0,
            kind: Kind::Other,
            effect: false,
            samples: Vec::new(),
            since: None,
            frames: 0,
            draws: 0,
            verts: 0,
            batch_key: None,
            batch_verts: 0,
            batch_dirty: false,
            gpu_frames: BTreeMap::new(),
            csv_rows: Vec::new(),
            gpu: Vec::new(),
            gpu_sampling: false,
            sample_last: None,
        }
    }
}

local! {
    static STATE: RefCell<State> = const { RefCell::new(State::new()) };
    /// The CSV file has its header and rows: the next write appends.
    static CSV_STARTED: Cell<bool> = const { Cell::new(false) };
}

/// The time between two summaries.
const PERIOD: Duration = Duration::from_secs(5);

/// The probe is on (the same switch as the profiling log, [`crate::diag::profiling`]).
pub fn on() -> bool {
    crate::diag::profiling()
}

/// Runs `f`; with the probe on, adds its time to this frame's slot for `stage`.
pub fn time<T>(stage: Stage, f: impl FnOnce() -> T) -> T {
    if !on() {
        return f();
    }
    let started = Instant::now();
    let r = f();
    let spent = started.elapsed();
    STATE.with_borrow_mut(|s| s.stage[stage as usize] += spent);
    r
}

/// A texture was rebuilt this frame (the minimap's and the fog's caches).
pub fn tex_rebuilt() {
    if on() {
        STATE.with_borrow_mut(|s| s.rebuilds += 1);
    }
}

/// A standing sprite of `w`×`h` pixels (in the pass's 2D space) was drawn this frame: the
/// probe's count and area, for the overdraw estimate.
pub fn sprite(w: f32, h: f32) {
    if on() {
        STATE.with_borrow_mut(|s| {
            s.sprites += 1;
            s.sprite_px += w * h;
        });
    }
}

/// The terrain layer was drawn over `w`×`h` pixels (in the pass's 2D space) this frame.
pub fn terrain(w: f32, h: f32) {
    if on() {
        STATE.with_borrow_mut(|s| s.terrain_px += w * h);
    }
}

/// A draw of `verts` vertices from the texture `key` (a hash of its id; 0 for none) was submitted
/// this frame. The draw calls are estimated the way macroquad's batcher issues them: a draw call
/// opens at a draw whose texture differs from the open batch's, after a [`batch_break`], or when the
/// batch would pass its vertex capacity. Macroquad's own count is private, so this is the probe's
/// estimate; it counts the draws the map's code makes (sprites, ground, sheets, shadows), not every
/// draw of the frame. Off with the probe, it does nothing.
pub fn draw(key: u64, verts: u32) {
    if on() {
        STATE.with_borrow_mut(|s| {
            if s.batch_dirty || s.batch_key != Some(key) || s.batch_verts + verts > MAX_BATCH_VERTS {
                s.draws += 1;
                s.batch_verts = 0;
            }
            s.batch_key = Some(key);
            s.batch_dirty = false;
            s.batch_verts += verts;
            s.verts += verts;
        });
    }
}

/// The batch is broken by a state change macroquad batches on: a material or uniform set, a camera
/// or render pass change (see [`draw`]).
pub fn batch_break() {
    if on() {
        STATE.with_borrow_mut(|s| s.batch_dirty = true);
    }
}

/// The frame being drawn, counted from 1 as the CSV's `frame` column (for [`record_gpu_stage`]).
pub fn current_frame() -> u64 {
    STATE.with_borrow(|s| s.frames + 1)
}

/// The GPU time of `stage` (one of [`GPU_STAGES`]) in frame `frame`, in ms: the CSV fills in the
/// frame's `gpu_<stage>_ms` column when its rows are written, so a result may come a frame or two
/// late. Other stages are ignored.
pub fn record_gpu_stage(frame: u64, stage: Stage, ms: f32) {
    if let Some(i) = GPU_STAGES.iter().position(|&g| g == stage) {
        if on() {
            STATE.with_borrow_mut(|s| {
                let slot = &mut s.gpu_frames.entry(frame).or_insert([f32::NAN; GPU_STAGES.len()])[i];
                *slot = merge_gpu_ms(*slot, ms);
            });
        }
    }
}

/// A GPU stage's time in a frame after another mark of it: the first mark sets it (`NAN` before), a second adds
/// to it, as the CPU stages do (the water's picture, then its overlay, are one stage in one frame).
fn merge_gpu_ms(slot: f32, ms: f32) -> f32 {
    if slot.is_nan() { ms } else { slot + ms }
}

/// This frame is a map frame (its kind is a map kind, see [`is_map`]) and the probe is on: the
/// map's last GPU wait (`gpu-ui`) applies.
pub fn map_frame_now() -> bool {
    on() && STATE.with_borrow(|s| is_map(s.kind))
}

/// The map kinds: the hero not walking, walking, or stepping. Their lines carry the map's GPU
/// stages and the sprites.
pub fn is_map(kind: Kind) -> bool {
    matches!(kind, Kind::Idle { .. } | Kind::Walk { .. } | Kind::WalkStep { .. })
}

/// The terrain options' label for the map lines (`opt=1+2`), set once by the UI at start.
static MAP_OPTIONS: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Names the terrain options in effect: the map kinds' summary lines carry them (`idle[25d opt=1+2]`).
pub fn set_map_options(label: &str) {
    let _ = MAP_OPTIONS.set(label.to_string());
}

/// The kind of the frame being drawn.
pub fn current_kind() -> Kind {
    STATE.with_borrow(|s| s.kind)
}

/// The GPU timing of the terrain (`ui::gpu_probe::timed`) is on: the probe is on and,
/// on desktop, `RAZDOR_GPU_STAGES=1` (Android uses its own GPU waits, see [`map_frame_now`]).
pub fn gpu_timers_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    on() && *ON.get_or_init(|| !cfg!(target_os = "android") && std::env::var("RAZDOR_GPU_STAGES").is_ok_and(|v| v == "1"))
}

/// The map's GPU waits on Android (`ui::gpu_probe::gpu_wait`): with the probe on and the app folder's
/// `profile-gpu` file too. Off by default: each wait flushes the queue and stalls the CPU until the
/// GPU is done, which slows every frame (40-55 ms a frame on a 2020 Adreno driver, a player's
/// report), so a plain `profile` measures the frames as they are. Never on the desktop.
pub fn gpu_waits_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    if !cfg!(target_os = "android") {
        // Desktop: the stage markers of `RAZDOR_GPU_STAGES=1` (`gpu_timers_on`).
        return gpu_timers_on();
    }
    on() && *ON.get_or_init(|| crate::platform::app_dir().is_some_and(|d| d.join("profile-gpu").is_file()))
}

/// This frame is a debug-mode GPU sample: its GPU waits run (`ui::gpu_probe::gpu_wait`). Set at the end of the
/// previous frame ([`end_frame`]), once per [`GPU_SAMPLE_EVERY`] while the debug mode is on.
pub fn gpu_sampled_now() -> bool {
    on() && STATE.with_borrow(|s| s.gpu_sampling)
}

/// Whether an event that comes every `every` is due at `now`, given its last time (`None`: never yet).
pub fn due(last: Option<Instant>, now: Instant, every: Duration) -> bool {
    last.is_none_or(|t| now.saturating_duration_since(t) >= every)
}

/// A timed GPU draw of `ms` milliseconds, in a frame of `kind`: counted in its summary line.
pub fn record_gpu(kind: Kind, ms: f32) {
    if on() {
        STATE.with_borrow_mut(|s| s.gpu.push((kind, ms)));
    }
}

/// Sharp sampling is off (the profiling A/B of the pixel-art sampler, `ui::sharp`): the
/// texture is sampled plainly. Read once; `profile-nosharp` in the app folder (Android) or
/// `RAZDOR_NO_SHARP=1` (desktop) turns it on, and a line says so when it is.
pub fn sharp_off() -> bool {
    static OFF: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *OFF.get_or_init(|| {
        let off = if cfg!(target_os = "android") {
            crate::platform::app_dir().is_some_and(|d| d.join("profile-nosharp").is_file())
        } else {
            std::env::var("RAZDOR_NO_SHARP").is_ok_and(|v| v == "1")
        };
        if off {
            crate::diag::log("sharp sampling off (profiling)");
        }
        off
    })
}

/// Sets this frame's kind; the default each frame is [`Kind::Other`].
pub fn set_kind(kind: Kind) {
    if on() {
        STATE.with_borrow_mut(|s| s.kind = kind);
    }
}

/// An effect was drawn this frame: a battle frame becomes [`Kind::BattleEffect`].
pub fn mark_effect() {
    if on() {
        STATE.with_borrow_mut(|s| s.effect = true);
    }
}

/// The kind a frame is summarised under: a battle frame in which an effect was drawn is a
/// battle-effect frame; every other kind is as set.
fn settled_kind(kind: Kind, effect: bool) -> Kind {
    match kind {
        Kind::Battle { tilt } if effect => Kind::BattleEffect { tilt },
        k => k,
    }
}

/// Stores this frame's sample and resets the per-frame slots and the kind. Once 5 s have
/// passed since the last summary, logs the summary and clears the samples.
pub fn end_frame(work: Duration, total: Duration) {
    if !on() {
        return;
    }
    let now = Instant::now();
    let csv = csv_path().is_some();
    let (due, rows, gpu) = STATE.with_borrow_mut(|s| {
        let since = *s.since.get_or_insert(now);
        let sample = Sample {
            kind: settled_kind(s.kind, s.effect),
            work_ms: ms(work),
            total_ms: ms(total),
            stage_ms: s.stage.map(ms),
            rebuilds: s.rebuilds,
            sprites: s.sprites,
            sprite_px: s.sprite_px,
            terrain_px: s.terrain_px,
            draws: s.draws,
            verts: s.verts,
            gpu_sampled: s.gpu_sampling,
        };
        s.frames += 1;
        if csv {
            s.csv_rows.push((s.frames, sample));
        }
        s.samples.push(sample);
        s.stage = [Duration::ZERO; Stage::COUNT];
        s.rebuilds = 0;
        s.sprites = 0;
        s.sprite_px = 0.0;
        s.terrain_px = 0.0;
        s.draws = 0;
        s.verts = 0;
        s.batch_key = None;
        s.batch_verts = 0;
        s.batch_dirty = false;
        s.kind = Kind::Other;
        s.effect = false;
        // The next frame is a GPU sample once the period has passed (debug mode only: the flag is set on
        // every platform, the waits themselves are the GPU waits of `ui::gpu_probe`).
        s.gpu_sampling = crate::diag::debug_mode() && due(s.sample_last, now, GPU_SAMPLE_EVERY);
        if s.gpu_sampling {
            s.sample_last = Some(now);
        }
        let elapsed = now.duration_since(since);
        if elapsed < PERIOD {
            return (None, Vec::new(), BTreeMap::new());
        }
        s.since = Some(now);
        let rows = std::mem::take(&mut s.csv_rows);
        let gpu = take_gpu_frames(s, rows.last().map(|(f, _)| *f));
        (Some((elapsed.as_secs_f32(), std::mem::take(&mut s.samples), std::mem::take(&mut s.gpu))), rows, gpu)
    });
    write_csv(&rows, &gpu);
    if let Some((secs, samples, gpu)) = due {
        log_lines(secs, &samples, &gpu);
    }
}

/// The GPU results of the frames up to `last` (the rows about to be written), taken out of the state;
/// later frames keep theirs for their own rows.
fn take_gpu_frames(s: &mut State, last: Option<u64>) -> BTreeMap<u64, [f32; GPU_STAGES.len()]> {
    let Some(last) = last else { return BTreeMap::new() };
    let later = s.gpu_frames.split_off(&(last + 1));
    std::mem::replace(&mut s.gpu_frames, later)
}

/// Logs the pending samples' summary now (at exit), and appends the pending CSV rows. Does
/// nothing when there are none.
pub fn flush() {
    if !on() {
        return;
    }
    let (secs, samples, rows, gpu, gpu_frames) = STATE.with_borrow_mut(|s| {
        let secs = s.since.map_or(0.0, |t| t.elapsed().as_secs_f32());
        s.since = None;
        let rows = std::mem::take(&mut s.csv_rows);
        let gpu_frames = take_gpu_frames(s, rows.last().map(|(f, _)| *f));
        (secs, std::mem::take(&mut s.samples), rows, std::mem::take(&mut s.gpu), gpu_frames)
    });
    write_csv(&rows, &gpu_frames);
    log_lines(secs, &samples, &gpu);
}

fn log_lines(secs: f32, samples: &[Sample], gpu: &[(Kind, f32)]) {
    for line in summary_lines(secs, samples, gpu) {
        crate::diag::log(&line);
    }
}

/// The per-frame CSV's file: `RAZDOR_PROFILE_FRAMES` (desktop; no env vars on Android).
fn csv_path() -> Option<PathBuf> {
    static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| std::env::var_os("RAZDOR_PROFILE_FRAMES").filter(|p| !p.is_empty()).map(PathBuf::from)).clone()
}

/// A kind's name in the CSV and in the summary lines (the view is added there).
fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::WalkStep { .. } => "walk-step",
        Kind::Walk { .. } => "walk",
        Kind::Idle { .. } => "idle",
        Kind::Battle { .. } => "battle",
        Kind::BattleEffect { .. } => "battle-effect",
        Kind::Other => "other",
    }
}

/// The view a map or battle kind was drawn in: `25d` or `flat`; None for the other kinds.
fn view_name(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Battle { tilt } | Kind::BattleEffect { tilt } | Kind::Walk { tilt } | Kind::WalkStep { tilt } | Kind::Idle { tilt } => Some(if tilt { "25d" } else { "flat" }),
        Kind::Other => None,
    }
}

/// A kind's name in the summary lines: the map and battle kinds carry their view (`battle[25d]`),
/// and `nosharp` when the sharp sampling is off (`idle[25d nosharp]`).
fn summary_name(kind: Kind) -> String {
    match view_name(kind) {
        Some(view) => {
            let off = if sharp_off() { " nosharp" } else { "" };
            // The map kinds name the terrain options in effect (`opt=1+2`).
            let opt = match (is_map(kind), MAP_OPTIONS.get()) {
                (true, Some(label)) => format!(" {label}"),
                _ => String::new(),
            };
            format!("{}[{view}{off}{opt}]", kind_name(kind))
        }
        None => kind_name(kind).to_string(),
    }
}

/// One CSV row of frame `frame` (the probe's count, from 1), without GPU times (see [`csv_row_gpu`]).
pub fn csv_row(frame: u64, s: &Sample) -> String {
    csv_row_gpu(frame, s, None)
}

/// One CSV row of frame `frame` with its GPU times in [`GPU_STAGES`] order (empty columns when `gpu`
/// is None or a stage was not timed). The kind's name is the CSV's (`walk-step`, `walk`, `idle`,
/// `battle`, `battle-effect`, `other`); times in ms with 3 decimals; the view of the map and battle
/// kinds comes after the rebuilds and stages (`flat` or `25d`, empty otherwise), then the draws and
/// vertices and the GPU columns.
pub fn csv_row_gpu(frame: u64, s: &Sample, gpu: Option<&[f32; GPU_STAGES.len()]>) -> String {
    let stage = |st: Stage| s.stage_ms[st as usize];
    let mut row = format!(
        "{frame},{},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{:.3},{},{:.3},{:.3},{:.3},{:.3},{:.3},{},{},{}",
        kind_name(s.kind),
        s.work_ms,
        s.total_ms,
        stage(Stage::Tick),
        stage(Stage::Terrain),
        stage(Stage::World),
        stage(Stage::Fog),
        stage(Stage::Minimap),
        stage(Stage::Route),
        stage(Stage::Look),
        stage(Stage::Armies),
        stage(Stage::Script),
        stage(Stage::Day),
        stage(Stage::Snap),
        stage(Stage::Autosave),
        stage(Stage::Session),
        stage(Stage::AiMove),
        stage(Stage::AiPlan),
        s.rebuilds,
        stage(Stage::Backdrop),
        stage(Stage::BattleWindow),
        stage(Stage::Effect),
        stage(Stage::GpuBackdrop),
        stage(Stage::GpuWindow),
        view_name(s.kind).unwrap_or(""),
        s.draws,
        s.verts,
    );
    for st in EFFECT_STAGES {
        row.push_str(&format!(",{:.3}", stage(st)));
    }
    for i in 0..GPU_STAGES.len() {
        row.push(',');
        if let Some(ms) = gpu.map(|g| g[i]).filter(|ms| !ms.is_nan()) {
            row.push_str(&format!("{ms:.3}"));
        }
    }
    row
}

/// Appends rows to the CSV file: the header first, the file truncated, on the first write of
/// the run. Each row takes the GPU times its frame has in `gpu`. Errors are logged, once per write.
fn write_csv(rows: &[(u64, Sample)], gpu: &BTreeMap<u64, [f32; GPU_STAGES.len()]>) {
    if rows.is_empty() {
        return;
    }
    let Some(path) = csv_path() else { return };
    let first = !CSV_STARTED.get();
    let mut text = String::new();
    if first {
        text.push_str(CSV_HEADER);
        text.push('\n');
    }
    for (frame, sample) in rows {
        text.push_str(&csv_row_gpu(*frame, sample, gpu.get(frame)));
        text.push('\n');
    }
    let result = if first {
        std::fs::write(&path, text.as_bytes())
    } else {
        std::fs::OpenOptions::new().append(true).open(&path).and_then(|mut f| f.write_all(text.as_bytes()))
    };
    match result {
        Ok(()) => CSV_STARTED.set(true),
        Err(e) => crate::diag!("RAZDOR_PROFILE_FRAMES: cannot write {}: {e}", path.display()),
    }
}

fn ms(d: Duration) -> f32 {
    d.as_secs_f32() * 1000.0
}

/// The summary of `samples`, taken over `secs` seconds: one line per kind that has frames, in
/// the order of [`KINDS`]. Empty when there are no frames.
pub fn summary_lines(secs: f32, samples: &[Sample], gpu: &[(Kind, f32)]) -> Vec<String> {
    // The frames as they are, then the debug mode's GPU samples under their own label (`gpu-sample`), so their
    // waits do not change the frames' numbers.
    [(false, ""), (true, "gpu-sample ")]
        .into_iter()
        .flat_map(|(sampled, group)| {
            KINDS.into_iter().filter_map(move |kind| {
                let of: Vec<&Sample> = samples.iter().filter(|s| s.gpu_sampled == sampled && s.kind == kind).collect();
                let times: Vec<f32> = if sampled { Vec::new() } else { gpu.iter().filter(|(k, _)| *k == kind).map(|(_, ms)| *ms).collect() };
                (!of.is_empty()).then(|| kind_line(secs, kind, &of, &times, group))
            })
        })
        .collect()
}

fn kind_line(secs: f32, kind: Kind, of: &[&Sample], gpu_terrain: &[f32], group: &str) -> String {
    let name = format!("{group}{}", summary_name(kind));
    let rebuilds: u32 = of.iter().map(|s| s.rebuilds).sum();
    let stage = |st: Stage| dist(of.iter().map(|s| s.stage_ms[st as usize])).narrow();
    // The effects' GPU stages, to two decimals (see `Dist::narrow2`).
    let stage2 = |st: Stage| dist(of.iter().map(|s| s.stage_ms[st as usize])).narrow2();
    // The battle kinds split their window's time: the backdrop, the window, its effects and the
    // GPU waits (Android; zero elsewhere).
    let battle = if matches!(kind, Kind::Battle { .. } | Kind::BattleEffect { .. }) {
        format!(
            "backdrop {} | battle-window {} | effect {} | gpu-backdrop {} | gpu-window {} | ",
            stage(Stage::Backdrop),
            stage(Stage::BattleWindow),
            stage(Stage::Effect),
            stage(Stage::GpuBackdrop),
            stage(Stage::GpuWindow),
        )
    } else {
        String::new()
    };
    // The map kinds split their frame's GPU waits (Android; zero elsewhere) and count the
    // standing sprites and the terrain's area (the overdraw estimate, pixels of the 2D space).
    let map = if is_map(kind) {
        let mean = |f: fn(&Sample) -> f32| of.iter().map(|s| f(s)).sum::<f32>() / of.len() as f32;
        let max = |f: fn(&Sample) -> f32| of.iter().map(|s| f(s)).fold(0.0, f32::max);
        // The effects' means (CPU and GPU), and the draws the frames made: one value each, with the frames' mean.
        let avg = |st: Stage| of.iter().map(|s| s.stage_ms[st as usize]).sum::<f32>() / of.len() as f32;
        let effects = format!(
            "water {:.2} | mist {:.2} | light {:.2} | sun-shadow {:.2} | weather {:.2} | ground-cache {:.2} | freeze {:.2} | gpu-water {} | gpu-mist {} | gpu-light {} | gpu-sun-shadow {} | gpu-weather {} | gpu-ground-cache {} | gpu-freeze {} | gpu-minimap {} | draws {:.0} verts {:.0} | ",
            avg(Stage::Water),
            avg(Stage::Mist),
            avg(Stage::Light),
            avg(Stage::SunShadow),
            avg(Stage::Weather),
            avg(Stage::GroundCache),
            avg(Stage::Freeze),
            stage2(Stage::GpuWater),
            stage2(Stage::GpuMist),
            stage2(Stage::GpuLight),
            stage2(Stage::GpuSunShadow),
            stage2(Stage::GpuWeather),
            stage2(Stage::GpuGroundCache),
            stage2(Stage::GpuFreeze),
            stage2(Stage::GpuMinimap),
            mean(|s| s.draws as f32),
            mean(|s| s.verts as f32),
        );
        let base = format!(
            "gpu-sky {} | gpu-terrain {} | gpu-marks {} | gpu-sprites {} | gpu-fog {} | gpu-ui {} | terrain-gpu {} | sprites n={:.0} area={:.2}Mpx (max {:.2}Mpx) | terrain area={:.2}Mpx | ",
            stage(Stage::GpuSky),
            stage(Stage::GpuTerrain),
            stage(Stage::GpuMarks),
            stage(Stage::GpuSprites),
            stage(Stage::GpuFog),
            stage(Stage::GpuUi),
            // The terrain draws GPU time (desktop, `RAZDOR_GPU_STAGES=1`, `glFinish` brackets): p50/p99 ms.
            if gpu_terrain.is_empty() { "-".to_string() } else { let d = dist(gpu_terrain.iter().copied()); format!("p50 {:.3} p99 {:.3} max {:.3} n={}", d.p50, d.p99, d.max, gpu_terrain.len()) },
            mean(|s| s.sprites as f32),
            mean(|s| s.sprite_px) / 1e6,
            max(|s| s.sprite_px) / 1e6,
            mean(|s| s.terrain_px) / 1e6,
        );
        format!("{base}{effects}")
    } else {
        String::new()
    };
    format!(
        "frame probe {secs:.1} s {name} n={} | total {} | work {} | present {} | tick {} | route {} | look {} | armies {} | ai-move {} | ai-plan {} | script {} | day {} | snap {} | tick-rest {} | terrain {} | objects {} | fog {} | minimap {} | autosave {} | rest {} | {battle}{map}tex rebuilds {rebuilds}",
        of.len(),
        dist(of.iter().map(|s| s.total_ms)).wide(),
        dist(of.iter().map(|s| s.work_ms)).wide(),
        dist(of.iter().map(|s| present_ms(s))).wide(),
        stage(Stage::Tick),
        stage(Stage::Route),
        stage(Stage::Look),
        stage(Stage::Armies),
        stage(Stage::AiMove),
        stage(Stage::AiPlan),
        stage(Stage::Script),
        stage(Stage::Day),
        stage(Stage::Snap),
        dist(of.iter().map(|s| tick_rest_ms(s))).narrow(),
        stage(Stage::Terrain),
        dist(of.iter().map(|s| objects_ms(s))).narrow(),
        stage(Stage::Fog),
        stage(Stage::Minimap),
        stage(Stage::Autosave),
        dist(of.iter().map(|s| rest_ms(s))).narrow(),
    )
}

/// Time outside the game's work: the vsync wait and the GL submission, `total - work`.
fn present_ms(s: &Sample) -> f32 {
    s.total_ms - s.work_ms
}

/// The map's objects (sprites, markers): the world minus its terrain and the GPU waits inside
/// the world (sky, terrain, marks, sprites).
fn objects_ms(s: &Sample) -> f32 {
    let inside = [
            Stage::Terrain,
            Stage::GpuSky,
            Stage::GpuTerrain,
            Stage::GpuMarks,
            Stage::GpuSprites,
            Stage::Water,
            Stage::Mist,
            Stage::Light,
            Stage::SunShadow,
            Stage::Weather,
            Stage::GpuMist,
            Stage::GpuLight,
            Stage::GpuSunShadow,
        Stage::GpuWeather,
    ];
    let waits: f32 = inside.iter().map(|st| s.stage_ms[*st as usize]).sum();
    (s.stage_ms[Stage::World as usize] - waits).max(0.0)
}

/// The tick's time outside its timed parts (route, look, armies, script, day, snap), clamped at
/// zero.
fn tick_rest_ms(s: &Sample) -> f32 {
    let parts: f32 = [Stage::Route, Stage::Look, Stage::Armies, Stage::Script, Stage::Day, Stage::Snap]
        .iter()
        .map(|st| s.stage_ms[*st as usize])
        .sum();
    (s.stage_ms[Stage::Tick as usize] - parts).max(0.0)
}

/// What the work spends outside the timed stages, clamped at zero. The tick counts as one stage
/// (its parts are inside it); the autosave and the session's look are outside the tick; the
/// battle window and the backdrop count as one each (the effects are inside the window), and so
/// do the GPU waits.
fn rest_ms(s: &Sample) -> f32 {
    let timed = s.stage_ms[Stage::Tick as usize]
        + s.stage_ms[Stage::World as usize]
        + s.stage_ms[Stage::Fog as usize]
        + s.stage_ms[Stage::Minimap as usize]
        + s.stage_ms[Stage::Autosave as usize]
        + s.stage_ms[Stage::Session as usize]
        + s.stage_ms[Stage::Backdrop as usize]
        + s.stage_ms[Stage::BattleWindow as usize]
        + s.stage_ms[Stage::GpuBackdrop as usize]
        + s.stage_ms[Stage::GpuWindow as usize]
        + s.stage_ms[Stage::GpuFog as usize]
        + s.stage_ms[Stage::GpuUi as usize];
    (s.work_ms - timed).max(0.0)
}

/// Percentiles and the maximum of one column of values.
struct Dist {
    p50: f32,
    p90: f32,
    p99: f32,
    max: f32,
}

fn dist(values: impl Iterator<Item = f32>) -> Dist {
    let mut v: Vec<f32> = values.collect();
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    Dist {
        p50: percentile(&v, 50.0),
        p90: percentile(&v, 90.0),
        p99: percentile(&v, 99.0),
        max: v.last().copied().unwrap_or(0.0),
    }
}

impl Dist {
    /// "p50 a p90 b p99 c max d".
    fn wide(&self) -> String {
        format!("p50 {:.1} p90 {:.1} p99 {:.1} max {:.1}", self.p50, self.p90, self.p99, self.max)
    }

    /// "p50 a p99 b max c".
    fn narrow(&self) -> String {
        format!("p50 {:.1} p99 {:.1} max {:.1}", self.p50, self.p99, self.max)
    }

    /// "p50 a p99 b max c" to two decimals: the effects' GPU waits are a few hundredths of a ms, which one
    /// decimal shows as 0.0.
    fn narrow2(&self) -> String {
        format!("p50 {:.2} p99 {:.2} max {:.2}", self.p50, self.p99, self.max)
    }
}

/// The nearest-rank percentile `p` (0 to 100) of `sorted` (ascending); 0 when it is empty.
pub fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (p * sorted.len() as f32 / 100.0).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(kind: Kind, work_ms: f32, total_ms: f32, stage_ms: [f32; Stage::COUNT]) -> Sample {
        Sample { kind, work_ms, total_ms, stage_ms, rebuilds: 0, sprites: 0, sprite_px: 0.0, terrain_px: 0.0, draws: 0, verts: 0, gpu_sampled: false }
    }

    #[test]
    fn the_gpu_sample_is_due_at_first_then_every_ten_seconds() {
        let t0 = Instant::now();
        assert!(due(None, t0, GPU_SAMPLE_EVERY), "the first sample at once");
        assert!(!due(Some(t0), t0 + Duration::from_secs(9), GPU_SAMPLE_EVERY));
        assert!(due(Some(t0), t0 + Duration::from_secs(10), GPU_SAMPLE_EVERY));
    }

    #[test]
    fn the_gpu_samples_are_summarised_apart_from_the_frames() {
        let plain = sample(Kind::Idle { tilt: false }, 3.0, 4.0, [0.0; Stage::COUNT]);
        let mut sampled = sample(Kind::Idle { tilt: false }, 3.0, 40.0, [0.0; Stage::COUNT]);
        sampled.gpu_sampled = true;
        let lines = summary_lines(5.0, &[plain, sampled], &[]);
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("frame probe 5.0 s idle[flat] n=1 "), "{}", lines[0]);
        assert!(lines[1].starts_with("frame probe 5.0 s gpu-sample idle[flat] n=1 "), "{}", lines[1]);
        assert!(lines[0].contains(" | water 0.00 | mist 0.00 | "), "{}", lines[0]);
        assert!(lines[0].contains(" | gpu-minimap p50 0.00 p99 0.00 max 0.00 | draws 0 verts 0 | "), "{}", lines[0]);
    }

    #[test]
    fn a_gpu_stage_marked_twice_in_a_frame_adds_up() {
        assert_eq!(merge_gpu_ms(f32::NAN, 1.5), 1.5, "the first mark sets the stage");
        assert_eq!(merge_gpu_ms(1.5, 2.0), 3.5, "the second mark adds to it");
    }

    #[test]
    fn percentiles_are_nearest_rank() {
        let data: Vec<f32> = (1..=10).map(|n| n as f32).collect();
        assert_eq!(percentile(&data, 50.0), 5.0);
        assert_eq!(percentile(&data, 90.0), 9.0);
        assert_eq!(percentile(&data, 99.0), 10.0);
        assert_eq!(percentile(&data, 100.0), 10.0);
        assert_eq!(percentile(&data, 0.0), 1.0);
        assert_eq!(percentile(&[7.5], 50.0), 7.5);
        assert_eq!(percentile(&[], 50.0), 0.0);
    }

    /// A stage array with the given stages' times; the others are zero.
    fn stages(list: &[(Stage, f32)]) -> [f32; Stage::COUNT] {
        let mut a = [0.0; Stage::COUNT];
        for (st, ms) in list {
            a[*st as usize] = *ms;
        }
        a
    }

    #[test]
    fn objects_and_rest_are_worked_out_per_frame() {
        // Tick 6 (route 1, look 0.5, armies 2, script 0.5 inside it), terrain 2, world 5 (terrain
        // inside it), fog 1, minimap 0.5, autosave 3 (outside the tick).
        let s = sample(
            Kind::Walk { tilt: false },
            20.0,
            22.0,
            stages(&[(Stage::Tick, 6.0), (Stage::Route, 1.0), (Stage::Look, 0.5), (Stage::Armies, 2.0), (Stage::Script, 0.5), (Stage::Terrain, 2.0), (Stage::World, 5.0), (Stage::Fog, 1.0), (Stage::Minimap, 0.5), (Stage::Autosave, 3.0)]),
        );
        assert_eq!(objects_ms(&s), 3.0);
        assert_eq!(tick_rest_ms(&s), 2.0);
        assert_eq!(rest_ms(&s), 4.5);
        assert_eq!(present_ms(&s), 2.0);
        // Stages that add up to more than the work: the rest is clamped at zero.
        let over = sample(Kind::Walk { tilt: false }, 5.0, 6.0, stages(&[(Stage::Tick, 2.0), (Stage::World, 4.0), (Stage::Fog, 2.0)]));
        assert_eq!(rest_ms(&over), 0.0);
        // The tick's parts above the tick: its rest is clamped at zero too.
        let parts_over = sample(Kind::Walk { tilt: false }, 5.0, 6.0, stages(&[(Stage::Tick, 2.0), (Stage::Route, 3.0)]));
        assert_eq!(tick_rest_ms(&parts_over), 0.0);
    }

    #[test]
    fn a_kind_without_frames_has_no_line() {
        let samples = [sample(Kind::Walk { tilt: false }, 4.0, 5.0, [0.0; Stage::COUNT]), sample(Kind::Walk { tilt: false }, 6.0, 7.0, [0.0; Stage::COUNT])];
        let lines = summary_lines(5.0, &samples, &[]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("frame probe 5.0 s walk[flat] n=2 "), "{}", lines[0]);
        assert!(summary_lines(5.0, &[], &[]).is_empty());
    }

    #[test]
    fn the_lines_follow_the_order_and_carry_the_rebuilds() {
        let mut step = sample(Kind::WalkStep { tilt: false }, 3.0, 4.0, stages(&[(Stage::Tick, 1.0), (Stage::Terrain, 1.0), (Stage::World, 2.0)]));
        step.rebuilds = 2;
        let idle = sample(Kind::Idle { tilt: true }, 2.0, 3.0, [0.0; Stage::COUNT]);
        let lines = summary_lines(5.0, &[idle, step], &[]);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains(" walk-step[flat] n=1 "), "{}", lines[0]);
        assert!(lines[0].ends_with("tex rebuilds 2"), "{}", lines[0]);
        assert!(lines[0].contains(" | tick-rest p50 "), "{}", lines[0]);
        assert!(lines[1].contains(" idle[25d] n=1 "), "{}", lines[1]);
    }

    #[test]
    fn a_csv_row_has_the_header_columns_in_order() {
        let mut step = sample(
            Kind::WalkStep { tilt: false },
            3.0,
            4.25,
            stages(&[(Stage::Tick, 1.0), (Stage::Terrain, 0.5), (Stage::World, 2.0), (Stage::Fog, 0.25), (Stage::Minimap, 0.125), (Stage::Route, 0.5), (Stage::Look, 0.25), (Stage::Armies, 0.75), (Stage::Autosave, 0.125), (Stage::Session, 0.25), (Stage::AiMove, 0.5), (Stage::AiPlan, 0.25)]),
        );
        step.rebuilds = 2;
        step.draws = 5;
        step.verts = 120;
        let row = csv_row(7, &step);
        assert!(row.starts_with("7,walk-step,3.000,4.250,1.000,0.500,2.000,0.250,0.125,0.500,0.250,0.750,0.000,0.000,0.000,0.125,0.250,0.500,0.250,2,0.000,0.000,0.000,0.000,0.000,flat,5,120,"), "{row}");
        // The effects' CPU columns (zero here), then the GPU columns, all empty when not timed.
        assert!(row.contains(",flat,5,120,0.000,0.000,0.000,0.000,0.000,0.000,0.000,,,,"), "{row}");
        assert_eq!(row.split(',').count(), CSV_HEADER.split(',').count());
        // The GPU columns follow the stages' order and are filled when timed.
        let mut gpu = [f32::NAN; GPU_STAGES.len()];
        gpu[3] = 1.5;
        gpu[7] = 0.25;
        let timed = csv_row_gpu(7, &step, Some(&gpu));
        assert!(timed.ends_with(",120,0.000,0.000,0.000,0.000,0.000,0.000,0.000,,,,1.500,,,,0.250,,,,,,,,"), "{timed}");
        assert_eq!(timed.split(',').count(), CSV_HEADER.split(',').count());
        let other = sample(Kind::Other, 0.5, 1.0, [0.0; Stage::COUNT]);
        assert!(csv_row(1, &other).starts_with("1,other,0.500,1.000,0.000,"));
        assert!(csv_row(1, &other).ends_with(","));
    }

    #[test]
    fn a_battle_frame_with_an_effect_is_a_battle_effect_frame() {
        assert_eq!(settled_kind(Kind::Battle { tilt: true }, false), Kind::Battle { tilt: true });
        assert_eq!(settled_kind(Kind::Battle { tilt: true }, true), Kind::BattleEffect { tilt: true });
        assert_eq!(settled_kind(Kind::Battle { tilt: false }, true), Kind::BattleEffect { tilt: false });
        // Only a battle frame turns into a battle-effect one.
        assert_eq!(settled_kind(Kind::Walk { tilt: false }, true), Kind::Walk { tilt: false });
        assert_eq!(settled_kind(Kind::Other, true), Kind::Other);
    }

    #[test]
    fn the_battle_lines_carry_the_view_and_the_battle_stages() {
        let fx = sample(Kind::BattleEffect { tilt: true }, 9.0, 10.0, stages(&[(Stage::Backdrop, 2.0), (Stage::BattleWindow, 5.0), (Stage::Effect, 3.0), (Stage::GpuWindow, 1.0)]));
        let flat = sample(Kind::Battle { tilt: false }, 4.0, 5.0, stages(&[(Stage::Backdrop, 1.0), (Stage::BattleWindow, 2.0)]));
        let lines = summary_lines(5.0, &[fx, flat], &[]);
        assert_eq!(lines.len(), 2);
        assert!(lines[0].starts_with("frame probe 5.0 s battle[flat] n=1 "), "{}", lines[0]);
        assert!(lines[1].starts_with("frame probe 5.0 s battle-effect[25d] n=1 "), "{}", lines[1]);
        assert!(lines[1].contains(" | backdrop p50 2.0 p99 2.0 max 2.0 | battle-window p50 5.0 "), "{}", lines[1]);
        assert!(lines[1].contains(" | effect p50 3.0 "), "{}", lines[1]);
        assert!(lines[1].ends_with(" | tex rebuilds 0"), "{}", lines[1]);
        // The walk lines keep their old shape: no battle stages.
        let walk = summary_lines(5.0, &[sample(Kind::Walk { tilt: false }, 1.0, 2.0, [0.0; Stage::COUNT])], &[]);
        assert!(!walk[0].contains("battle-window"), "{}", walk[0]);
        assert!(walk[0].contains(" | gpu-sky p50 0.0 p99 0.0 max 0.0 | gpu-terrain "), "{}", walk[0]);
        // The window's time less its backdrop and GPU waits is the rest.
        let rest = sample(Kind::Battle { tilt: true }, 9.0, 10.0, stages(&[(Stage::Backdrop, 2.0), (Stage::BattleWindow, 5.0), (Stage::GpuWindow, 1.0)]));
        assert_eq!(rest_ms(&rest), 1.0);
    }

    #[test]
    fn the_csv_names_the_kinds_as_the_summary_does() {
        let s = sample(Kind::Walk { tilt: false }, 1.0, 1.0, [0.0; Stage::COUNT]);
        assert!(csv_row(1, &s).starts_with("1,walk,"));
        assert!(summary_lines(5.0, &[s], &[]).iter().all(|l| l.contains(" walk[flat] n=1 ")));
    }
}
