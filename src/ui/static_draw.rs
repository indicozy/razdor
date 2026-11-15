//! Retained draw calls for the flat view's static decorations (A′: `RAZDOR_STATIC_DRAW=1`, the lab line `static-draw`;
//! off by default, no look change; `docs/flows/testing-and-tools.md`, the lab's switches).
//!
//! The flat view's standing decorations (trees, rocks, ...) draw the same vertices from frame to frame while the view,
//! the fog and the frame's values stay. The key of a frame is built from exactly those (`Key`). A frame whose key
//! repeats the last frame's is a still frame: the one after the first repeat records what each decoration's draw put
//! in the batch (its vertices after the CPU model, as the batch has them; its indices; its state). Later frames with
//! that key replay the record: each run of consecutive recorded decorations is drawn with ONE retained draw call
//! (`QuadGl::draw_static`, from the static buffers), after the run's first decoration has been drawn as usual (it sets
//! the material, texture and uniforms the retained draw takes; a state that differs from the recorded one draws the
//! run as usual). Nothing of a run is copied or uploaded on those frames, and the draws keep the pass's order.
//!
//! Not retained: trees while the map animates (their sway reads the clock), the figures, the buildings, the 2.5D view.
//! Those break a run. The light and the lit material are in the key, so a changed light records again.

use std::cell::RefCell;
use std::ops::Range;

use macroquad::models::Vertex;
use macroquad::window::get_internal_gl;
use macroquad::{DrawState, StaticGeometry};
use razdor::lab;
use razdor::local;
use razdor::rules::map::Grid;

/// The most vertices one static geometry holds: its indices are u16.
const MAX_VERTICES: usize = u16::MAX as usize + 1;

/// The switch is on for this run, and the replay floor and the sprite retain (the other two ways of keeping the batch)
/// are off: the replay floor and the retain copy the batch's draw calls, which a retained draw call cannot be copied into.
pub fn enabled() -> bool {
    lab::static_draw() && !lab::replay_floor() && !super::sharp::sprite_retain_enabled()
}

/// Logs the switch once at start, so a lab run can check which switches it had.
pub fn note_startup() {
    if lab::static_draw() {
        let line = if enabled() { "static draw: on" } else { "static draw: off (the replay floor or the sprite retain is on)" };
        eprintln!("{line}");
    }
}

/// Everything a frame's static draws read besides the map's objects: the bits of each float and flag (exact equality,
/// so a repeated key means the same draws) and the grid. The caller builds it from the view.
#[derive(Clone, Debug, PartialEq)]
pub struct Key {
    pub bits: Vec<u64>,
    pub grid: Grid,
}

/// Where a decoration's draws are in the record: its state's index, and its indices' place in its chunk.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Span {
    /// It drew nothing this frame (too small to see): it does not break a run.
    Empty,
    Drawn { state: usize, chunk: usize, first: usize, count: usize },
}

/// What one decoration drew in the recording frame, copied out of the batch.
struct Captured {
    state: DrawState,
    vertices: Vec<Vertex>,
    indices: Vec<u16>,
}

/// One decoration of the recording frame: nothing, its draw, or a draw the record cannot keep (it stays as usual).
enum Capture {
    Empty,
    Drawn(Box<Captured>),
    Unrecorded,
}

/// The record of a key: per object index its span (None: not recorded, always drawn as usual), the distinct states,
/// and the uploaded chunks of vertices and indices.
struct Record {
    key: Key,
    spans: Vec<Option<Span>>,
    states: Vec<DrawState>,
    chunks: Vec<StaticGeometry>,
}

#[derive(Default)]
struct Store {
    /// The last frame's key: a repeat makes the frame a recording one.
    last: Option<Key>,
    record: Option<Record>,
    /// Geometry no draw uses any more, with the submission count at its retirement. Freed once a later submission has
    /// drawn the frames that used it.
    retired: Vec<(u64, StaticGeometry)>,
}

local! {
    static STORE: RefCell<Store> = RefCell::new(Store::default());
}

/// What a frame does with one item of its list (see [`plan`]).
#[derive(Clone, Debug, PartialEq)]
enum Step {
    /// Drawn as usual.
    Normal,
    /// The run's first drawn decoration: drawn as usual, then checked against the recorded state `state`.
    Lead { run: usize, state: usize },
    /// Another decoration of the run: not drawn (the retained draw has it), unless the run broke at its lead.
    Skip { run: usize },
    /// The run's last item: the retained draw of the run's decorations after the lead (`indices` of chunk `chunk`).
    Emit { run: usize, chunk: usize, indices: Range<usize> },
}

/// One frame's static draws (see the module).
pub struct Pass {
    mode: Mode,
    /// The run whose lead drew another state than recorded: its items are drawn as usual.
    broken: Option<usize>,
}

enum Mode {
    /// Not in use (the switch is off, or the 2.5D view): the items are drawn as usual.
    Off,
    /// The key is new, or has no record yet: drawn as usual.
    Plain,
    /// A still frame: each candidate is captured from the batch as it is drawn.
    Record { key: Key, captured: Vec<(u32, Capture)> },
    /// The key has a record: this frame's steps.
    Replay { steps: Vec<Step> },
}

impl Pass {
    /// Not in use: the frame draws as before.
    pub fn off() -> Pass {
        Pass { mode: Mode::Off, broken: None }
    }

    /// Draws item `k` of the pass. `object` is its object index when it is a candidate (a decoration the flat view draws
    /// the same from frame to frame), None otherwise; `body` draws it as usual.
    pub fn object(&mut self, k: usize, object: Option<u32>, body: impl FnOnce()) {
        match &mut self.mode {
            Mode::Off | Mode::Plain => body(),
            Mode::Record { captured, .. } => match object {
                Some(i) => captured.push((i, capture(body))),
                None => body(),
            },
            Mode::Replay { steps } => match steps[k].clone() {
                Step::Normal => body(),
                Step::Lead { run, state } => {
                    if !lead_matches(state, body) {
                        self.broken = Some(run);
                    }
                }
                Step::Skip { run } => {
                    if self.broken == Some(run) {
                        body();
                    }
                }
                Step::Emit { run, chunk, indices } => {
                    if self.broken != Some(run) {
                        // SAFETY: the frame's draws run on the main thread while macroquad runs (as every draw).
                        let gl = unsafe { get_internal_gl() };
                        STORE.with_borrow(|s| {
                            if let Some(rec) = &s.record {
                                gl.quad_gl.draw_static(&rec.chunks[chunk], indices);
                            }
                        });
                    }
                }
            },
        }
    }

    /// Ends the frame's pass. A recording frame stores its record here (its batch is drawn later, unchanged).
    /// `objects` is the map's object count.
    pub fn finish(self, objects: usize) {
        if let Mode::Record { key, captured } = self.mode {
            build(key, captured, objects);
        }
    }
}

/// Starts a frame's static draws. `key` is the frame's key (None when the caller does not build one: the switch is off,
/// or the view is not flat); `candidates` is the item list, each item's object index when it is a candidate and None
/// for the others.
pub fn begin(key: Option<Key>, candidates: impl Iterator<Item = Option<u32>>, tilted: bool) -> Pass {
    let Some(key) = key.filter(|_| !tilted && enabled()) else {
        return Pass::off();
    };
    // SAFETY: as in `Pass::object`.
    let gl = unsafe { get_internal_gl() };
    let now = gl.quad_gl.batch_position().flushes;
    let done: Vec<StaticGeometry> = STORE.with_borrow_mut(|s| {
        let (done, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut s.retired).into_iter().partition(|(at, _)| *at < now);
        s.retired = keep;
        done.into_iter().map(|(_, g)| g).collect()
    });
    for g in done {
        g.delete(gl.quad_context);
    }
    let mode = STORE.with_borrow_mut(|s| {
        let repeat = s.last.as_ref() == Some(&key);
        s.last = Some(key.clone());
        let same = s.record.as_ref().is_some_and(|r| r.key == key);
        if same {
            let candidates: Vec<Option<u32>> = candidates.collect();
            let rec = s.record.as_ref();
            return Mode::Replay { steps: plan(rec.map_or(&[], |r| r.spans.as_slice()), &candidates) };
        }
        if let Some(old) = s.record.take() {
            s.retired.extend(old.chunks.into_iter().map(|g| (now, g)));
        }
        if repeat {
            Mode::Record { key, captured: Vec::new() }
        } else {
            Mode::Plain
        }
    });
    Pass { mode, broken: None }
}

/// The steps of a replayed frame. A run is a maximal stretch of candidates whose recorded spans are drawn decorations
/// of one state and one chunk with contiguous indices (empty ones join it), or empty. Its first drawn decoration is the
/// lead, the rest of the run is skipped, and the run's last item emits the retained draw. A run with fewer than two
/// drawn decorations stays as usual, and so does every item that is not a candidate or has no record.
fn plan(spans: &[Option<Span>], candidates: &[Option<u32>]) -> Vec<Step> {
    let span = |k: usize| candidates[k].and_then(|i| spans.get(i as usize).copied().flatten());
    let n = candidates.len();
    let mut steps = vec![Step::Normal; n];
    let mut run_id = 0;
    let mut k = 0;
    while k < n {
        if span(k).is_none() {
            k += 1;
            continue;
        }
        let mut j = k;
        let (mut state, mut chunk, mut next) = (None, None, None);
        let mut drawn: Vec<usize> = Vec::new();
        let mut second_first = 0;
        while j < n {
            let Some(sp) = span(j) else { break };
            if let Span::Drawn { state: s, chunk: c, first, count } = sp {
                if state.is_some_and(|x| x != s) || chunk.is_some_and(|x| x != c) || next.is_some_and(|x| x != first) {
                    break;
                }
                if drawn.len() == 1 {
                    second_first = first;
                }
                state = Some(s);
                chunk = Some(c);
                next = Some(first + count);
                drawn.push(j);
            }
            j += 1;
        }
        if drawn.len() >= 2 {
            let lead = drawn[0];
            steps[lead] = Step::Lead { run: run_id, state: state.unwrap_or(0) };
            for step in steps.iter_mut().take(j).skip(lead + 1) {
                *step = Step::Skip { run: run_id };
            }
            steps[j - 1] = Step::Emit { run: run_id, chunk: chunk.unwrap_or(0), indices: second_first..next.unwrap_or(second_first) };
            run_id += 1;
        }
        k = j;
    }
    steps
}

/// Draws one decoration as usual and says what it put in the batch: nothing, one new draw call of vertices and indices
/// with no flush in between (its state is the draw call's), or something the record cannot keep.
fn capture(body: impl FnOnce()) -> Capture {
    // SAFETY: as in `Pass::object`.
    let gl = unsafe { get_internal_gl() };
    gl.quad_gl.batch_break();
    let from = gl.quad_gl.batch_position();
    body();
    let to = gl.quad_gl.batch_position();
    if to.flushes != from.flushes {
        return Capture::Unrecorded;
    }
    if to.draws == from.draws && to.verts == from.verts && to.indices == from.indices {
        return Capture::Empty;
    }
    if to.draws != from.draws + 1 || to.indices == from.indices {
        return Capture::Unrecorded;
    }
    let (Some(state), Some((vertices, indices))) = (gl.quad_gl.draw_state(from.draws), gl.quad_gl.batch_span(from, to)) else {
        return Capture::Unrecorded;
    };
    Capture::Drawn(Box::new(Captured { state, vertices: vertices.to_vec(), indices: indices.to_vec() }))
}

/// Whether a lead drew as recorded: new vertices in no flush, in the batch's last draw call, whose state is the record's
/// `state`. The lead's vertices are in that call whether it was opened for them or joined to one of the same state, so
/// the state is the one the retained draw takes.
fn lead_matches(state: usize, body: impl FnOnce()) -> bool {
    // SAFETY: as in `Pass::object`.
    let gl = unsafe { get_internal_gl() };
    let from = gl.quad_gl.batch_position();
    body();
    let to = gl.quad_gl.batch_position();
    if to.flushes != from.flushes || to.verts == from.verts || to.draws == 0 {
        return false;
    }
    let Some(now) = gl.quad_gl.draw_state(to.draws - 1) else { return false };
    STORE.with_borrow(|s| s.record.as_ref().and_then(|r| r.states.get(state)) == Some(&now))
}

/// Stores a recording frame's captures as the key's record: the vertices and indices of the drawn decorations in chunks
/// of at most `MAX_VERTICES` (each decoration's indices rebased to its chunk), uploaded once.
fn build(key: Key, captured: Vec<(u32, Capture)>, objects: usize) {
    let mut spans: Vec<Option<Span>> = vec![None; objects];
    let mut states: Vec<DrawState> = Vec::new();
    let mut chunks: Vec<(Vec<Vertex>, Vec<u16>)> = Vec::new();
    for (object, capture) in captured {
        let Some(span) = spans.get_mut(object as usize) else { continue };
        match capture {
            Capture::Empty => *span = Some(Span::Empty),
            Capture::Unrecorded => {}
            Capture::Drawn(c) if c.indices.is_empty() || c.vertices.len() > MAX_VERTICES => {}
            Capture::Drawn(c) => {
                let state = match states.iter().position(|s| *s == c.state) {
                    Some(s) => s,
                    None => {
                        states.push(c.state);
                        states.len() - 1
                    }
                };
                if chunks.last().is_none_or(|(v, _)| v.len() + c.vertices.len() > MAX_VERTICES) {
                    chunks.push((Vec::new(), Vec::new()));
                }
                let chunk = chunks.len() - 1;
                let base = chunks[chunk].0.len();
                *span = Some(Span::Drawn { state, chunk, first: chunks[chunk].1.len(), count: c.indices.len() });
                chunks[chunk].0.extend_from_slice(&c.vertices);
                chunks[chunk].1.extend(c.indices.iter().map(|x| (*x as usize + base) as u16));
            }
        }
    }
    // SAFETY: as in `Pass::object`.
    let gl = unsafe { get_internal_gl() };
    let mut geometry = Vec::with_capacity(chunks.len());
    let (mut vertices, mut indices) = (0usize, 0usize);
    for (v, idx) in &chunks {
        vertices += v.len();
        indices += idx.len();
        geometry.push(StaticGeometry::new(gl.quad_context, v, idx));
    }
    let drawn = spans.iter().filter(|s| matches!(s, Some(Span::Drawn { .. }))).count();
    eprintln!("static draw: recorded decorations={drawn} states={} chunks={} vertices={vertices} indices={indices}", states.len(), geometry.len());
    let now = gl.quad_gl.batch_position().flushes;
    STORE.with_borrow_mut(|s| {
        if let Some(old) = s.record.take() {
            s.retired.extend(old.chunks.into_iter().map(|g| (now, g)));
        }
        s.record = Some(Record { key, spans, states, chunks: geometry });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawn(state: usize, chunk: usize, first: usize, count: usize) -> Option<Span> {
        Some(Span::Drawn { state, chunk, first, count })
    }

    /// A run is led by its first drawn decoration, skips the middle, and emits at its last item. Runs split at a state
    /// change, at a chunk change, at a gap in the indices and at an item that is not a candidate.
    #[test]
    fn runs_are_led_skipped_and_emitted_at_their_last_item() {
        let spans = vec![drawn(0, 0, 0, 6), drawn(0, 0, 6, 6), drawn(0, 0, 12, 3), Some(Span::Empty), drawn(1, 0, 15, 6), drawn(0, 0, 40, 6), drawn(0, 0, 46, 6)];
        let candidates: Vec<Option<u32>> = (0..7).map(|i| Some(i as u32)).chain([None, Some(0)]).collect();
        let steps = plan(&spans, &candidates);
        assert_eq!(steps[0], Step::Lead { run: 0, state: 0 });
        assert_eq!(steps[1], Step::Skip { run: 0 });
        assert_eq!(steps[2], Step::Skip { run: 0 });
        assert_eq!(steps[3], Step::Emit { run: 0, chunk: 0, indices: 6..15 }, "the empty one closes the run, the drawn ones after the lead are its range");
        assert_eq!(steps[4], Step::Normal, "another state: a run of one");
        assert_eq!(steps[5], Step::Lead { run: 1, state: 0 });
        assert_eq!(steps[6], Step::Emit { run: 1, chunk: 0, indices: 46..52 });
        assert_eq!(steps[7], Step::Normal, "not a candidate");
        assert_eq!(steps[8], Step::Normal, "a run of one");
    }

    #[test]
    fn a_run_of_one_drawn_decoration_stays_as_usual() {
        let steps = plan(&[drawn(0, 0, 0, 6), Some(Span::Empty)], &[Some(0), Some(1)]);
        assert_eq!(steps, vec![Step::Normal, Step::Normal]);
    }

    #[test]
    fn an_unrecorded_decoration_breaks_its_run() {
        let steps = plan(&[drawn(0, 0, 0, 6), None, drawn(0, 0, 6, 6)], &[Some(0), Some(1), Some(2)]);
        assert_eq!(steps, vec![Step::Normal, Step::Normal, Step::Normal]);
    }

    #[test]
    fn a_gap_in_the_indices_starts_a_new_run() {
        let steps = plan(&[drawn(0, 0, 0, 6), drawn(0, 0, 9, 6), drawn(0, 0, 15, 6)], &[Some(0), Some(1), Some(2)]);
        assert_eq!(steps[0], Step::Normal);
        assert_eq!(steps[1], Step::Lead { run: 0, state: 0 });
        assert_eq!(steps[2], Step::Emit { run: 0, chunk: 0, indices: 15..21 });
    }
}
