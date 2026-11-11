//! The full-state snapshot: an exact copy of the live game, from which a replay continues the
//! game as the live one does. A save is not one: a load reseeds the main generator, sets the
//! event generator from the clock and resets the AI's records (the original's behaviour, kept
//! for saves, see `rules::save::restore`), so a loaded game draws and decides differently from
//! the live one. A snapshot holds what the serialised game does not: the generators, the
//! music's wait, the AI's per-army runtime fields, the step and wait bookkeeping, the settings
//! the rules read (the eight outside the save), and the in-flight parts of a step that is at rest.
//! The map is referenced by its hash and the install's copy, never embedded.
//!
//! [`state_hash`] is a stable hash of the same rules state (no timings, no UI), for drift
//! detection. Format: bzip2 of a JSON body `{version, game, rules, timing}`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::dt::dtm::Scenario;
use crate::rules::battle::{patch_globals, set_patch_globals, PatchGlobals};
use crate::rules::content::Content;
use crate::rules::events::EventEngine;
use crate::rules::formation::{Formation, Slot};
use crate::rules::game::{Event, Game};
use crate::rules::map::Tile;
use crate::rules::magic::Reading;
use crate::rules::rng::{EventRng, Rng};
use crate::rules::save::{fnv1a, Install, ScenarioRef};
use crate::rules::world::World;

/// The snapshot format's version: the body's `version` field.
pub const SNAPSHOT_VERSION: u32 = 1;

/// The settings the rules read that a save does not hold (the eight outside the save, and the
/// walk pace they set): the game continues under the same ones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Settings {
    pub improved_ai: bool,
    pub max_ai: bool,
    pub max_guard: bool,
    pub keep_xp_on_promotion: bool,
    pub keep_formation: bool,
    pub allow_unit_benching: bool,
    pub pursue: bool,
    pub walk_pace: f32,
}

/// The runtime fields of one army (the AI's record and the army's flags), in the order the
/// armies are listed: the field, then the inactive ones, then the respawning ones.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ArmyState {
    pub idle_offset: i32,
    pub idle_restarts: u32,
    pub fallen: bool,
    /// The steps the army took in the stretch under way: the screen's display, and what the
    /// held events of a step wait for (`Game::step_playing`).
    pub walk: crate::rules::world::Walk,
    /// The path's step weights (`AiMind::path_weights`, not in a save): the cost of the step after
    /// the next one.
    pub path_weights: Vec<u8>,
}

/// What the rules keep outside the serialised game.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rules {
    pub rng: u32,
    pub event_rng: u32,
    pub music_wait: Option<u32>,
    pub stretches: u64,
    pub asks: u32,
    pub wait_ticks: u32,
    pub endless_wait: Option<u32>,
    pub reading: Option<Reading>,
    pub queued_casts: Vec<u32>,
    pub step_base: Option<u32>,
    pub step_from: Option<Tile>,
    pub battle_troops: Vec<usize>,
    pub click_buildings: (Option<usize>, Option<usize>),
    pub waiting_entry: Option<usize>,
    pub formation_before_battle: Option<Vec<Slot>>,
    pub walk_flags: (bool, bool),
    pub held: Vec<(u64, Event)>,
    pub ai_events: Vec<Event>,
    pub finished_events: Vec<(u64, Event)>,
    pub armies: Vec<ArmyState>,
    pub settings: Settings,
    /// The Community patch's globals, which outlive a battle (`battle::patch_globals`).
    pub patch: PatchGlobals,
}

impl Settings {
    /// The settings a game plays with now.
    pub fn of(g: &Game) -> Settings {
        Settings {
            improved_ai: g.improved_ai,
            max_ai: g.max_ai,
            max_guard: g.max_guard,
            keep_xp_on_promotion: g.keep_xp_on_promotion,
            keep_formation: g.keep_formation,
            allow_unit_benching: g.allow_unit_benching,
            pursue: g.pursue,
            walk_pace: g.walk_pace,
        }
    }
}

/// The timings of the frame loop. Not part of the state hash: the rules do not depend on how
/// the frames split the time (the frame-rate tests), so a replay drives its own clock.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Timing {
    pub step_elapsed: f32,
    pub since_step: f32,
    pub stretch_minutes: f32,
    pub snap_due: bool,
    pub snapped: bool,
    pub hero_glide: Option<((f32, f32), (f32, f32))>,
}

#[derive(Serialize)]
struct BodyOut<'a> {
    version: u32,
    game: &'a Game,
    rules: &'a Rules,
    #[serde(skip_serializing_if = "Option::is_none")]
    timing: Option<&'a Timing>,
}

#[derive(Deserialize)]
struct BodyIn {
    version: u32,
    game: Game,
    rules: Rules,
    timing: Timing,
}

fn army_states(g: &Game) -> Vec<ArmyState> {
    let w = &g.world;
    let armies = w.armies.iter().chain(w.inactive.iter()).chain(w.respawns.iter().map(|r| &r.army));
    armies.map(|a| ArmyState { idle_offset: a.mind.idle_offset, idle_restarts: a.mind.idle_restarts, fallen: a.mind.fallen, walk: a.walk.clone(), path_weights: a.mind.path_weights.clone() }).collect()
}

fn rules_of(g: &Game) -> Rules {
    Rules {
        rng: g.rng.state(),
        event_rng: g.event_rng.state(),
        music_wait: g.music_wait,
        stretches: g.stretches,
        asks: g.asks,
        wait_ticks: g.wait_ticks,
        endless_wait: g.endless_wait,
        reading: g.reading,
        queued_casts: g.queued_casts.clone(),
        step_base: g.step_base,
        step_from: g.step_from,
        battle_troops: g.battle_troops.clone(),
        click_buildings: g.click_buildings,
        waiting_entry: g.waiting_entry,
        formation_before_battle: g.formation_before_battle.clone(),
        walk_flags: g.walk_flags,
        held: g.held.clone(),
        ai_events: g.ai_events.clone(),
        finished_events: g.finished_events.clone(),
        armies: army_states(g),
        patch: patch_globals(),
        settings: Settings::of(g),
    }
}

fn timing_of(g: &Game) -> Timing {
    Timing {
        step_elapsed: g.step_elapsed,
        since_step: g.since_step,
        stretch_minutes: g.stretch_minutes,
        snap_due: g.snap_due,
        snapped: g.snapped,
        hero_glide: g.hero_glide,
    }
}

/// Refuses a game that is not at rest: a step whose AI moves are unfinished, or an event window
/// whose flight is still held (both are interface state the snapshot does not carry).
fn at_rest(g: &Game) -> Result<(), String> {
    if g.ai_pending() {
        return Err("the game is mid-step (its AI moves are unfinished)".into());
    }
    if !g.held_shown.is_empty() {
        return Err("an event's flight is held for its window".into());
    }
    if g.world.armies.iter().chain(g.world.inactive.iter()).any(|a| a.mind.contact.is_some()) {
        return Err("an army's arrival is still being played".into());
    }
    Ok(())
}

/// A full-state snapshot of a game (bzip2 of its JSON body).
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    bytes: Vec<u8>,
}

impl Snapshot {
    /// The snapshot of `g` now. Refused while the game is not at rest.
    pub fn take(g: &Game) -> Result<Snapshot, String> {
        at_rest(g)?;
        let json = body_json(g, true)?;
        Ok(Snapshot { bytes: crate::rules::save::pack(&json).map_err(|e| e.to_string())? })
    }

    /// The bytes of the file a snapshot is stored as.
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// A snapshot read from bytes [`Snapshot::bytes`] gave (checked when restored).
    pub fn from_bytes(bytes: Vec<u8>) -> Snapshot {
        Snapshot { bytes }
    }

    /// The game the snapshot holds, rebuilt as the live one was: the content (the demo's, or the
    /// install's for a map), the world's and the event engine's statics from the scenario, and
    /// the generators, AI fields and settings as they were. Nothing is reseeded, reset or
    /// re-drawn (unlike a load). A map must be in `install` with the hash the snapshot names.
    pub fn restore(&self, demo: Arc<Content>, install: Option<&Install>) -> Result<Game, String> {
        let json = unpack(&self.bytes)?;
        let body: BodyIn = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
        if body.version != SNAPSHOT_VERSION {
            return Err(format!("a snapshot of version {}", body.version));
        }
        let mut game = body.game;
        let rules = body.rules;
        let (content, fresh, engine, map_bytes) = match game.origin.clone() {
            Some(ScenarioRef::Map { file, hash }) => {
                let install = install.ok_or("a map snapshot needs the install")?;
                let bytes = install_map(install.dir, &file, hash)?;
                let scenario = Scenario::from_file_bytes(&bytes).map_err(|e| e.to_string())?;
                let world = World::from_scenario(&scenario, &install.content);
                (install.content.clone(), world, Some(EventEngine::new(&scenario)), Some(bytes))
            }
            _ => (demo.clone(), World::standard(&demo), None, None),
        };
        game.world.restore_statics(fresh).map_err(|e| e.to_string())?;
        match (game.script.as_deref_mut(), engine) {
            (Some(e), Some(fresh)) => e.restore_statics(fresh)?,
            (None, _) => {}
            (Some(_), None) => return Err("events saved for the demo".into()),
        }
        // The content the way a load builds it (the row width the game was started with, the
        // experimental rules): the same rules as `save::restore`, none of its draws.
        let wide = Formation::WIDE;
        game.content = if game.wide_row == (content.formation == wide) {
            content
        } else {
            Arc::new(content.with_formation(if game.wide_row { wide } else { Formation::VANILLA }))
        };
        game.content = Arc::new(game.content.with_experimental(game.experimental));
        game.map_bytes = map_bytes.map(Into::into);
        game.rng = Rng::new(rules.rng);
        game.event_rng = EventRng::new(rules.event_rng);
        game.music_wait = rules.music_wait;
        game.stretches = rules.stretches;
        game.asks = rules.asks;
        game.wait_ticks = rules.wait_ticks;
        game.endless_wait = rules.endless_wait;
        game.reading = rules.reading;
        game.queued_casts = rules.queued_casts;
        game.step_base = rules.step_base;
        game.step_from = rules.step_from;
        game.battle_troops = rules.battle_troops;
        game.click_buildings = rules.click_buildings;
        game.waiting_entry = rules.waiting_entry;
        game.formation_before_battle = rules.formation_before_battle;
        game.walk_flags = rules.walk_flags;
        game.held = rules.held;
        game.ai_events = rules.ai_events;
        game.finished_events = rules.finished_events;
        set_army_states(&mut game, &rules.armies)?;
        let s = &rules.settings;
        game.improved_ai = s.improved_ai;
        game.max_ai = s.max_ai;
        game.max_guard = s.max_guard;
        game.keep_xp_on_promotion = s.keep_xp_on_promotion;
        game.keep_formation = s.keep_formation;
        game.allow_unit_benching = s.allow_unit_benching;
        game.pursue = s.pursue;
        game.walk_pace = s.walk_pace;
        set_patch_globals(rules.patch);
        game.step_elapsed = body.timing.step_elapsed;
        game.since_step = body.timing.since_step;
        game.stretch_minutes = body.timing.stretch_minutes;
        game.snap_due = body.timing.snap_due;
        game.snapped = body.timing.snapped;
        game.hero_glide = body.timing.hero_glide;
        Ok(game)
    }
}

fn set_army_states(g: &mut Game, states: &[ArmyState]) -> Result<(), String> {
    let w = &mut g.world;
    let armies = w.armies.iter_mut().chain(w.inactive.iter_mut()).chain(w.respawns.iter_mut().map(|r| &mut r.army));
    let mut n = 0;
    for (a, s) in armies.zip(states) {
        a.mind.idle_offset = s.idle_offset;
        a.mind.idle_restarts = s.idle_restarts;
        a.mind.fallen = s.fallen;
        a.walk = s.walk.clone();
        a.mind.path_weights = s.path_weights.clone();
        n += 1;
    }
    if n != states.len() || n != g.world.armies.len() + g.world.inactive.len() + g.world.respawns.len() {
        return Err(format!("{} army records for {n} armies", states.len()));
    }
    Ok(())
}

/// The map file's bytes from the install, when they hash to `hash`.
fn install_map(dir: &std::path::Path, file: &str, hash: u64) -> Result<Vec<u8>, String> {
    let maps = crate::dt::install::list_maps(dir).map_err(|e| e.to_string())?;
    let entry = maps.iter().find(|m| m.name == file).ok_or_else(|| format!("map {file} is not in the install"))?;
    let bytes = std::fs::read(&entry.path).map_err(|e| e.to_string())?;
    if fnv1a(&bytes) != hash {
        return Err(format!("map {file} has changed since the snapshot"));
    }
    Ok(bytes)
}

fn body_json(g: &Game, with_timing: bool) -> Result<Vec<u8>, String> {
    let mut rules = rules_of(g);
    if !with_timing {
        // How many arrivals a frame asked for depends on the frames' budgets, not on the rules.
        rules.asks = 0;
        // Events not yet delivered are one queue whichever frame produced them: an event a step
        // left for the next tick (`finished_events`) and one a frame holds for its window (`held`)
        // carry the stretch they were made in, so they are the same entries.
        rules.held.append(&mut rules.finished_events);
    }
    let timing = timing_of(g);
    let body = BodyOut { version: SNAPSHOT_VERSION, game: g, rules: &rules, timing: with_timing.then_some(&timing) };
    serde_json::to_vec(&body).map_err(|e| e.to_string())
}

fn unpack(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut json = Vec::new();
    bzip2::read::BzDecoder::new(bytes).read_to_end(&mut json).map_err(|e| e.to_string())?;
    Ok(json)
}

/// The state's snapshot JSON and its hash, at rest: a keyframe (the writer packs the JSON). The
/// hash is [`state_hash`]'s.
pub fn capture(g: &Game) -> Result<(Vec<u8>, u64), String> {
    at_rest(g)?;
    let json = body_json(g, true)?;
    let hash = fnv1a(&body_json(g, false)?);
    Ok((json, hash))
}

/// A stable hash of the game's rules state: the serialised game, the generators, the AI's
/// fields, the settings, and the in-flight parts of a step; no timings, no UI. Two processes
/// that play the same inputs get the same value.
pub fn state_hash(g: &Game) -> u64 {
    fnv1a(&body_json(g, false).expect("a game serialises"))
}
