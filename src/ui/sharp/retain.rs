//! Retained GPU buffers for a flat map pass's textured pieces (A5, the spike; `RAZDOR_SPRITE_RETAIN=1`, off by default).
//!
//! The pass ([`super::session_pass`]) records the pieces its sharp draws hand to macroquad: the vertices after the CPU
//! model transform, the indices, the texture and its texel size, in painter order. Every other draw the pass makes
//! through this module (a blend, a plain draw, an untextured draw, a scissor change; [`event`]) closes a segment: the
//! pieces since the last close are one segment, drawn from the retained buffers at its close, before the draw that
//! closed it, so the painter order is kept.
//!
//! On the next frame, while every piece equals the recorded one, the projection and the fog table's upload count are
//! unchanged, and each event comes at the recorded place with the recorded reason and scissor, the pieces are not
//! batched by macroquad: each segment is drawn from Razdor's own miniquad buffers with Razdor's pipeline (the sharp
//! fragment shader with the fog, the same blend), after macroquad's batch is flushed. Any difference sends the pieces
//! of the current segment through macroquad in their order and drops the recording.
//!
//! The check compares the vertices, so the CPU still builds every piece: the spike measures what macroquad's batching
//! and copying cost, not the vertex work. Draws this module does not see are not covered by the check (the proof is
//! the byte comparison of the snapshots).

use macroquad::miniquad::{
    Bindings, BufferId, BufferLayout, BufferSource, BufferType, BufferUsage, PassAction, Pipeline, RenderingBackend,
    ShaderMeta, ShaderSource, TextureId, UniformBlockLayout, UniformDesc, UniformType, VertexAttribute, VertexFormat,
};
use macroquad::prelude::*;

/// Floats in the uniform block: `Model` (16), `Projection` (16), `TexSize` (2), in the order the pipeline declares them.
const UNIFORM_FLOATS: usize = 34;

type Clip = Option<(i32, i32, i32, i32)>;

/// Where a retained pass's state must still be the same for its buffers to be drawn.
#[derive(Clone, Copy, PartialEq)]
struct Key {
    proj: [u32; 16],
    fog: u64,
}

/// One textured piece: its vertices and indices in the recording's arrays, its texture, and the texel size set for it.
struct Piece {
    tex: Texture2D,
    id: TextureId,
    tex_size: Vec2,
    vert: (usize, usize),
    idx: (usize, usize),
}

/// A segment as recorded: the pieces `start..end`, the event that closed it (`reason`), and the scissor it was drawn in.
struct SegRec {
    start: usize,
    end: usize,
    reason: &'static str,
    clip: Clip,
}

#[derive(Default)]
struct Recording {
    key: Option<Key>,
    verts: Vec<Vertex>,
    idx: Vec<u16>,
    pieces: Vec<Piece>,
    segs: Vec<SegRec>,
    open_start: usize,
    open_clip: Clip,
}

/// A run of consecutive pieces with one texture: one ranged draw (`first`, `count` in the absolute index buffer).
struct Group {
    id: TextureId,
    tex_size: Vec2,
    first: i32,
    count: i32,
}

/// A segment with its draws.
struct Seg {
    start: usize,
    end: usize,
    reason: &'static str,
    clip: Clip,
    groups: Vec<Group>,
}

struct Retained {
    rec: Recording,
    vb: BufferId,
    ib: BufferId,
    segs: Vec<Seg>,
}

enum Mode {
    Idle,
    Record(Recording),
    /// `seg` is the segment being matched; `cursor` the next piece; `pending` the matched pieces of that segment
    /// (drawn by the replay at its close, or sent through macroquad on a divergence).
    Replay { seg: usize, cursor: usize, pending: Vec<usize>, diverged: bool },
}

struct State {
    mode: Mode,
    retained: Option<Retained>,
}

razdor::local! {
    static STATE: std::cell::RefCell<State> = const { std::cell::RefCell::new(State { mode: Mode::Idle, retained: None }) };
    /// The scissor macroquad was last given (`super::note_clip`), in pixels.
    static CLIP: std::cell::Cell<Clip> = const { std::cell::Cell::new(None) };
    static PIPELINE: std::cell::RefCell<Option<Pipeline>> = const { std::cell::RefCell::new(None) };
}

/// The switch: `RAZDOR_SPRITE_RETAIN=1` (read once), or at compile time `RAZDOR_SPRITE_RETAIN_BUILD=1` (an A/B
/// build: Android has no env vars, so the Samsung lab builds with it set). Off, no pass is retained and the draws
/// are as before.
pub fn enabled() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| built_in() || std::env::var("RAZDOR_SPRITE_RETAIN").is_ok_and(|v| v.trim() == "1"))
}

/// The compile-time switch (`RAZDOR_SPRITE_RETAIN_BUILD=1` when the binary is built).
fn built_in() -> bool {
    option_env!("RAZDOR_SPRITE_RETAIN_BUILD") == Some("1")
}

/// Logs the switch once at start, so a lab run can check which build it is (`sprite retain: on (A/B build)`).
pub fn note_startup() {
    let line = match (enabled(), built_in()) {
        (true, true) => "sprite retain: on (A/B build)",
        (true, false) => "sprite retain: on",
        (false, _) => "sprite retain: off",
    };
    razdor::diag::log(line);
}

/// Whether a retained pass is running (its pieces go through [`piece`]).
pub fn active() -> bool {
    STATE.with_borrow(|s| !matches!(s.mode, Mode::Idle))
}

fn current_key() -> Option<Key> {
    // SAFETY: the main thread, as every macroquad call.
    let gl = unsafe { get_internal_gl() };
    if gl.quad_gl.get_active_render_pass().is_some() {
        return None;
    }
    Some(Key {
        proj: gl.quad_gl.get_projection_matrix().to_cols_array().map(f32::to_bits),
        fog: super::super::sprite_fog::upload_count(),
    })
}

/// Starts a retained pass: a replay when the recording of the last pass is still valid for it, else a recording.
/// `false` when the pass cannot be retained (a render pass is set); then nothing is retained and [`end`] is not called.
pub fn begin() -> bool {
    let Some(key) = current_key() else { return false };
    let clip = CLIP.with(std::cell::Cell::get);
    STATE.with_borrow_mut(|s| {
        if !matches!(s.mode, Mode::Idle) {
            return false;
        }
        let replay = s.retained.as_ref().is_some_and(|r| r.rec.key == Some(key) && r.segs.first().is_none_or(|g| g.clip == clip));
        s.mode = if replay {
            Mode::Replay { seg: 0, cursor: 0, pending: Vec::new(), diverged: false }
        } else {
            Mode::Record(Recording { key: Some(key), open_clip: clip, ..Recording::default() })
        };
        true
    })
}

/// The bits of a vertex that reach the GPU (the position, uv, colour and normal slot).
pub(super) fn vkey(v: &Vertex) -> [u32; 10] {
    let (p, t, n) = (v.position.to_array(), v.uv.to_array(), v.normal.to_array());
    [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), t[0].to_bits(), t[1].to_bits(), n[0].to_bits(), n[1].to_bits(), n[2].to_bits(), n[3].to_bits(), u32::from_le_bytes(v.color)]
}

fn same(p: &Piece, rec: &Recording, verts: &[Vertex], idx: &[u16], id: TextureId, tex_size: Vec2) -> bool {
    // Timing experiment only (`RAZDOR_RETAIN_NOCMP=1`, read once): the vertex compare is skipped, so the output is
    // not checked. It measures what the compare costs; never a shipping setting.
    static NOCMP: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    let nocmp = *NOCMP.get_or_init(|| std::env::var_os("RAZDOR_RETAIN_NOCMP").is_some());
    p.id == id
        && p.tex_size == tex_size
        && p.vert.1 == verts.len()
        && p.idx.1 == idx.len()
        && (nocmp
            || rec.verts[p.vert.0..p.vert.0 + p.vert.1].iter().zip(verts).all(|(a, b)| vkey(a) == vkey(b)))
        && (nocmp || rec.idx[p.idx.0..p.idx.0 + p.idx.1] == *idx)
}

fn push(rec: &mut Recording, verts: &[Vertex], idx: &[u16], tex: &Texture2D, tex_size: Vec2) {
    let (v0, i0) = (rec.verts.len(), rec.idx.len());
    rec.verts.extend_from_slice(verts);
    rec.idx.extend_from_slice(idx);
    rec.pieces.push(Piece { tex: tex.clone(), id: tex.raw_miniquad_id(), tex_size, vert: (v0, verts.len()), idx: (i0, idx.len()) });
}

/// Sends the recorded piece `i` through macroquad, as `super::piece` would have (its texel size, then the draw).
fn emit(r: &Retained, i: usize) {
    let p = &r.rec.pieces[i];
    let mesh = Mesh {
        vertices: r.rec.verts[p.vert.0..p.vert.0 + p.vert.1].to_vec(),
        indices: r.rec.idx[p.idx.0..p.idx.0 + p.idx.1].to_vec(),
        texture: Some(p.tex.clone()),
    };
    if let Some(base) = super::session_base() {
        super::set_tex_size(&base, p.tex_size);
        super::session_material();
    }
    draw_mesh(&mesh);
}

/// One textured piece of the pass. `true`: the replay takes it (it is drawn at its segment's close). `false`: the caller
/// draws it now; the replay's pending pieces were sent through macroquad first, so they come before it.
pub fn piece(verts: &[Vertex], idx: &[u16], tex: &Texture2D, tex_size: Vec2) -> bool {
    let id = tex.raw_miniquad_id();
    STATE.with_borrow_mut(|s| {
        let State { mode, retained } = &mut *s;
        match mode {
            Mode::Idle => false,
            Mode::Record(rec) => {
                push(rec, verts, idx, tex, tex_size);
                false
            }
            Mode::Replay { seg, cursor, pending, diverged } => {
                if *diverged {
                    return false;
                }
                let Some(r) = retained.as_ref() else {
                    *diverged = true;
                    return false;
                };
                let end = r.segs.get(*seg).map_or(0, |s| s.end);
                if *cursor < end && same(&r.rec.pieces[*cursor], &r.rec, verts, idx, id, tex_size) {
                    pending.push(*cursor);
                    *cursor += 1;
                    return true;
                }
                *diverged = true;
                for &i in pending.iter() {
                    emit(r, i);
                }
                pending.clear();
                false
            }
        }
    })
}

/// Something this module does not see in the pass (a blend, a plain draw, an untextured draw, a scissor change)
/// comes now: the pieces before it are one segment. A recording closes its segment here; a replay draws its
/// segment from the retained buffers if the event comes where, and as, it was recorded, else it diverges.
pub fn event(why: &'static str) {
    let clip = CLIP.with(std::cell::Cell::get);
    STATE.with_borrow_mut(|s| {
        let State { mode, retained } = &mut *s;
        match mode {
            Mode::Idle => {}
            Mode::Record(rec) => close(rec, why),
            Mode::Replay { seg, cursor, pending, diverged } => {
                if *diverged {
                    return;
                }
                let Some(r) = retained.as_ref() else {
                    *diverged = true;
                    return;
                };
                let ok = r.segs.get(*seg).is_some_and(|s| s.reason == why && *cursor == s.end)
                    && r.segs.get(*seg + 1).is_none_or(|n| n.clip == clip);
                if ok {
                    draw_segment(r, *seg);
                    pending.clear();
                    *seg += 1;
                } else {
                    *diverged = true;
                    for &i in pending.iter() {
                        emit(r, i);
                    }
                    pending.clear();
                }
            }
        }
    });
}

/// Closes the open segment of a recording with `why`; the next one opens here, in the scissor now set.
fn close(rec: &mut Recording, why: &'static str) {
    let end = rec.pieces.len();
    rec.segs.push(SegRec { start: rec.open_start, end, reason: why, clip: rec.open_clip });
    rec.open_start = end;
    rec.open_clip = CLIP.with(std::cell::Cell::get);
}

/// The scissor macroquad was given (`pan`, `main_menu`): a change is an event (the segment before it is drawn in the
/// scissor it was recorded in).
pub fn note_clip(clip: Clip) {
    if CLIP.with(std::cell::Cell::get) == clip {
        return;
    }
    CLIP.with(|c| c.set(clip));
    event("clip");
}

/// Ends a retained pass (inside its session): a recording becomes the retained buffers; a replay draws its last
/// segment, or, when it diverged or stopped short, has sent its pending pieces through macroquad and drops the buffers.
pub fn end() {
    let mode = STATE.with_borrow_mut(|s| std::mem::replace(&mut s.mode, Mode::Idle));
    match mode {
        Mode::Idle => {}
        Mode::Record(mut rec) => {
            close(&mut rec, "end");
            let n = rec.pieces.len();
            let new = (n > 0 && rec.verts.len() <= u16::MAX as usize).then(|| build(rec));
            if enabled() {
                eprintln!("retain: record pieces={n} groups={}", new.as_ref().map_or(0, |r| r.segs.iter().map(|s| s.groups.len()).sum::<usize>()));
            }
            let old = STATE.with_borrow_mut(|s| std::mem::replace(&mut s.retained, new));
            if let Some(o) = old {
                drop_retained(o);
            }
        }
        Mode::Replay { seg, cursor, pending, diverged } => {
            let Some(r) = STATE.with_borrow_mut(|s| s.retained.take()) else { return };
            let whole = !diverged && seg + 1 == r.segs.len() && cursor == r.rec.pieces.len() && r.segs[seg].reason == "end";
            if whole {
                if enabled() {
                    eprintln!("retain: replay pieces={cursor} segments={}", r.segs.len());
                }
                draw_segment(&r, seg);
                STATE.with_borrow_mut(|s| s.retained = Some(r));
            } else {
                if enabled() {
                    eprintln!("retain: diverged={diverged} seg={seg}/{} matched={cursor}/{}", r.segs.len(), r.rec.pieces.len());
                }
                if !diverged {
                    for &i in &pending {
                        emit(&r, i);
                    }
                }
                drop_retained(r);
            }
        }
    }
}

fn build(rec: Recording) -> Retained {
    let mut abs: Vec<u16> = Vec::with_capacity(rec.idx.len());
    let mut segs: Vec<Seg> = Vec::new();
    for sr in &rec.segs {
        let mut groups: Vec<Group> = Vec::new();
        for p in &rec.pieces[sr.start..sr.end] {
            let first = abs.len() as i32;
            let base = p.vert.0 as u16;
            abs.extend(rec.idx[p.idx.0..p.idx.0 + p.idx.1].iter().map(|&i| i + base));
            match groups.last_mut() {
                Some(g) if g.id == p.id => g.count += p.idx.1 as i32,
                _ => groups.push(Group { id: p.id, tex_size: p.tex_size, first, count: p.idx.1 as i32 }),
            }
        }
        segs.push(Seg { start: sr.start, end: sr.end, reason: sr.reason, clip: sr.clip, groups });
    }
    // SAFETY: the main thread, as every macroquad call.
    let gl = unsafe { get_internal_gl() };
    let ctx = gl.quad_context;
    let vb = ctx.new_buffer(BufferType::VertexBuffer, BufferUsage::Immutable, BufferSource::slice(&rec.verts));
    let ib = ctx.new_buffer(BufferType::IndexBuffer, BufferUsage::Immutable, BufferSource::slice(&abs));
    Retained { rec, vb, ib, segs }
}

fn drop_retained(r: Retained) {
    // SAFETY: as `build`.
    let gl = unsafe { get_internal_gl() };
    gl.quad_context.delete_buffer(r.vb);
    gl.quad_context.delete_buffer(r.ib);
}

/// Razdor's pipeline for the retained draws: the sharp vertex and fragment shaders of the flat draw material
/// (`SHARP_VERTEX_SCALE`, `fragment_fog_scaled` with `DRAW_MAIN`), macroquad's vertex layout and alpha blend.
fn pipeline(ctx: &mut dyn RenderingBackend) -> Option<Pipeline> {
    if let Some(p) = PIPELINE.with_borrow(Option::clone) {
        return Some(p);
    }
    let fragment = super::fragment_fog_scaled("", super::DRAW_MAIN);
    let shader = ctx
        .new_shader(
            ShaderSource::Glsl { vertex: super::SHARP_VERTEX_SCALE, fragment: &fragment },
            ShaderMeta {
                images: vec!["Texture".to_string(), "FogVis".to_string()],
                uniforms: UniformBlockLayout {
                    uniforms: vec![
                        UniformDesc::new("Model", UniformType::Mat4),
                        UniformDesc::new("Projection", UniformType::Mat4),
                        UniformDesc::new("TexSize", UniformType::Float2),
                    ],
                },
            },
        )
        .ok()?;
    let attributes = [
        VertexAttribute::new("position", VertexFormat::Float3),
        VertexAttribute::new("texcoord", VertexFormat::Float2),
        VertexAttribute::new("color0", VertexFormat::Byte4),
        VertexAttribute::new("normal", VertexFormat::Float4),
    ];
    let pipe = ctx.new_pipeline(&[BufferLayout::default()], &attributes, shader, super::alpha_pipeline());
    PIPELINE.with_borrow_mut(|p| *p = Some(pipe));
    Some(pipe)
}

/// Draws segment `seg` of a retained recording: macroquad's batch is flushed first (what was queued before the segment
/// comes before it), then each group is one ranged draw with its texture and texel size, in the segment's scissor.
fn draw_segment(r: &Retained, seg: usize) {
    let s = &r.segs[seg];
    if s.groups.is_empty() {
        return;
    }
    // SAFETY: as `build`.
    let mut gl = unsafe { get_internal_gl() };
    gl.flush();
    let proj = gl.quad_gl.get_projection_matrix().to_cols_array();
    let ctx = gl.quad_context;
    let Some(pipe) = pipeline(ctx) else {
        // No pipeline on this driver: the segment's pieces go through macroquad, as before.
        for i in s.start..s.end {
            emit(r, i);
        }
        return;
    };
    let (w, h) = macroquad::miniquad::window::screen_size();
    let (w, h) = (w as i32, h as i32);
    let fog = super::super::sprite_fog::texture().raw_miniquad_id();
    let model = Mat4::IDENTITY.to_cols_array();
    ctx.begin_default_pass(PassAction::Nothing);
    ctx.apply_pipeline(&pipe);
    ctx.apply_viewport(0, 0, w, h);
    match s.clip {
        Some((x, y, cw, ch)) => ctx.apply_scissor_rect(x, h - (y + ch), cw, ch),
        None => ctx.apply_scissor_rect(0, 0, w, h),
    }
    for g in &s.groups {
        let bindings = Bindings { vertex_buffers: vec![r.vb], index_buffer: r.ib, images: vec![g.id, fog] };
        let mut u = [0f32; UNIFORM_FLOATS];
        u[..16].copy_from_slice(&model);
        u[16..32].copy_from_slice(&proj);
        u[32] = g.tex_size.x;
        u[33] = g.tex_size.y;
        ctx.apply_bindings(&bindings);
        ctx.apply_uniforms_from_bytes(u.as_ptr().cast(), std::mem::size_of_val(&u));
        ctx.draw(g.first, g.count, 1);
    }
    ctx.end_render_pass();
    super::after_raw_draw();
}
