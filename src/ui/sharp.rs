//! Sharp bilinear sampling for Discord Times' art.
//!
//! Plain bilinear filtering blurs every original pixel into its neighbours when the picture
//! is enlarged, and at uneven scales the pixels come out with different widths. The sharp
//! sampler keeps each original pixel a crisp square of the same size: inside a texel the
//! colour is that texel's own; only a thin band at each texel edge (one screen pixel wide)
//! is blended, which the hardware's bilinear filter does for us.
//!
//! [`SHARP_UV_GLSL`] is the GLSL function; [`fragment`] builds a fragment shader that samples
//! with it, [`material`] loads one, and [`draw`] is a drop-in for `draw_texture_ex`. The
//! textures stay `FilterMode::Linear`: the function needs the hardware bilinear step inside
//! the blend band.
//!
//! Blend modes (`chrome::additive` and the like) set their material with [`in_blend`]; a
//! sharp draw inside it sets its uniforms on that material instead of replacing it.

use razdor::frame_probe;
use razdor::local;
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams, TextureId};
use macroquad::prelude::*;

use super::memo::Recent;

/// The probe's draw count for a draw of `verts` vertices from `tex` (`frame_probe::draw`; the texture
/// is keyed by its id). Does nothing with the probe off.
pub fn count_draw(tex: Option<&Texture2D>, verts: usize) {
    if frame_probe::on() {
        let key = tex.map_or(0, |t| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            t.raw_miniquad_id().hash(&mut h);
            h.finish()
        });
        frame_probe::draw(key, verts as u32);
    }
}

/// The probe's batch break for a material or uniform change (`frame_probe::batch_break`).
pub fn count_break() {
    frame_probe::batch_break();
}

/// GLSL ES 1.00 source of `sharp_uv`, pasted into fragment shaders (the caller declares
/// the precision and the `TexSize` / `Scale` uniforms). `uv` is the texture coordinate,
/// `tex_size` the texture's size in texels, `scale` the draw's screen pixels per texel.
/// Scale below 1 (a picture drawn smaller than its texels) gives plain bilinear.
pub const SHARP_UV_GLSL: &str = r#"
vec2 sharp_uv(vec2 uv, vec2 tex_size, vec2 scale) {
    vec2 ts = max(tex_size, vec2(1.0));
    vec2 s = max(scale, vec2(1.0));
    vec2 texel = uv * ts;
    vec2 base = floor(texel - 0.5) + 0.5;
    vec2 f = texel - base;
    vec2 t = clamp((f - 0.5) * s + 0.5, 0.0, 1.0);
    return (base + t) / ts;
}
"#;

/// Rust mirror of [`SHARP_UV_GLSL`], line for line, for the tests. Keep the two in step.
#[cfg(test)]
fn sharp_uv_ref(uv: Vec2, tex_size: Vec2, scale: Vec2) -> Vec2 {
    let ts = tex_size.max(Vec2::ONE);
    let s = scale.max(Vec2::ONE);
    let texel = uv * ts;
    let base = (texel - 0.5).floor() + 0.5;
    let f = texel - base;
    let t = ((f - 0.5) * s + 0.5).clamp(Vec2::ZERO, Vec2::ONE);
    (base + t) / ts
}

/// Screen pixels per texel on each axis for a draw: the destination size over the source
/// rectangle's size (the whole texture when there is no source rectangle). The sign of a
/// flipped destination is ignored.
pub fn draw_scale(tex_size: Vec2, params: &DrawTextureParams) -> Vec2 {
    let source = params.source.map(|r| r.size()).unwrap_or(tex_size);
    let dest = params.dest_size.unwrap_or(source);
    dest.abs() / source * PERSPECTIVE.with(std::cell::Cell::get) * RASTER.with(std::cell::Cell::get)
}

/// The raster factor in effect (`set_raster`'s last value, 1 when none).
pub fn raster() -> f32 {
    RASTER.with(std::cell::Cell::get)
}

/// Sets the target's pixels per screen pixel while the world is drawn at a lower resolution
/// (`map_scale`; 1 otherwise): the draws' `Scale` follows the pixels they land on.
pub fn set_raster(f: f32) {
    RASTER.with(|r| r.set(f));
}

local! {
    /// The screen pixels per 2D pixel of a perspective draw (the dev-only trailer camera's standing
    /// sprites: a sprite nearer the eye than the pivot is magnified, and its blend band must follow
    /// the magnification, not the 2D size). 1 everywhere else.
    static PERSPECTIVE: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
    /// The target pixels per screen pixel of the map drawn at a lower resolution (`set_raster`). 1 elsewhere.
    static RASTER: std::cell::Cell<f32> = const { std::cell::Cell::new(1.0) };
}

/// Runs `draw` with the perspective factor `k` (screen pixels per 2D pixel) for its sharp draws.
pub fn with_perspective<R>(k: f32, draw: impl FnOnce() -> R) -> R {
    let prev = PERSPECTIVE.with(|p| p.replace(k));
    let r = draw();
    PERSPECTIVE.with(|p| p.set(prev));
    r
}

/// The vertex stage of every sharp material: as `chrome`'s own, with a highp `uv`.
pub const SHARP_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
// The fog cell of a map sprite in the normal's z and w (`sprite_fog`, 0 for none): see `FOG_GLSL`.
attribute vec4 normal;
varying highp vec2 uv;
varying lowp vec4 color;
varying highp vec2 FogAt;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    FogAt = normal.zw;
}"#;

/// [`SHARP_VERTEX`] with the draw's scale in the vertex's normal slot (`normal.xy`, screen pixels per texel),
/// passed on as the `Scale` varying. The sprite draws use it: each sprite's scale is its own vertices' data,
/// not a uniform set per draw, so consecutive sprites of one texture share one draw call (macroquad breaks its
/// batch at every uniform set). The quad pipeline has the normal attribute.
pub const SHARP_VERTEX_SCALE: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying highp vec2 uv;
varying lowp vec4 color;
varying highp vec2 Scale;
varying highp vec2 FogAt;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    Scale = normal.xy;
    FogAt = normal.zw;
}"#;

/// [`SHARP_VERTEX`] with the 2D point of each vertex (`spos`) for the lit material only: the other sharp
/// materials keep the base vertex shader, so the lighting off compiles exactly what it did.
pub const SHARP_VERTEX_SPOS: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
attribute vec4 normal;
varying highp vec2 uv;
varying lowp vec4 color;
// The vertex's 2D point before the model and projection: the pass's own pixels, the space the lights are in.
varying highp vec2 spos;
varying highp vec2 FogAt;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    spos = position.xy;
    FogAt = normal.zw;
}"#;

/// A fragment shader with `sharp_uv` and the uniforms `TexSize` and `Scale` declared, then
/// `extra` (more declarations, e.g. `uniform float Rotation;`), then `main` as the body of
/// `void main()`. In `main`: `uv` and `color` are the varyings, `sharp_tex(at)` samples
/// `Texture` at texture coordinate `at` with sharp sampling. The precision is highp where the
/// device has it; without highp in the fragment stage this fails to compile and the caller
/// gets the plain draw.
pub fn fragment(extra: &str, main: &str) -> String {
    fragment_with(extra, main, "uniform vec2 Scale;")
}

/// The fog of a map sprite, in the fragment stage (`sprite_fog`): the sampler `FogVis` (the table's texture, nearest)
/// and `fog_vis()`, the visibility of the sprite's foot cell, 1 where no fog applies (`FogAt` is 0). The table
/// holds 16 bits a cell, in the texel's red (high byte) and green (low byte) (`sprite_fog::decode`).
pub const FOG_GLSL: &str = r#"
uniform sampler2D FogVis;
varying highp vec2 FogAt;
float fog_vis() {
    if (FogAt.x <= 0.0) {
        return 1.0;
    }
    vec4 t = texture2D(FogVis, FogAt);
    float v = (floor(t.r * 255.0 + 0.5) * 256.0 + floor(t.g * 255.0 + 0.5)) / 65535.0;
    // Truncated to 8 bits, as the sprite's colour alpha was on the CPU (`faded`): the pictures keep their bytes.
    return floor(v * 255.0) / 255.0;
}
"#;

/// [`fragment`] with the map sprites' fog (`FOG_GLSL`): `main` may call `fog_vis()`. Its material binds `FogVis`
/// ([`material_fog`] or [`bind_fog`]).
pub fn fragment_fog(extra: &str, main: &str) -> String {
    fragment_with(&format!("{FOG_GLSL}{extra}"), main, "uniform vec2 Scale;")
}

/// [`fragment_fog`] for a [`material_scaled_with`] material (`Scale` from the vertex stage).
pub fn fragment_fog_scaled(extra: &str, main: &str) -> String {
    fragment_with(&format!("{FOG_GLSL}{extra}"), main, "varying highp vec2 Scale;")
}

fn fragment_with(extra: &str, main: &str, scale: &str) -> String {
    // The profiling A/B (`frame_probe::sharp_off`): the same material with plain sampling.
    let sample = if razdor::frame_probe::sharp_off() { "return texture2D(Texture, at);" } else { "return texture2D(Texture, sharp_uv(at, TexSize, Scale));" };
    format!(
        r#"#version 100
#ifdef GL_FRAGMENT_PRECISION_HIGH
precision highp float;
#else
precision mediump float;
#endif
varying highp vec2 uv;
varying lowp vec4 color;
uniform sampler2D Texture;
uniform vec2 TexSize;
{scale}
{extra}
{SHARP_UV_GLSL}
vec4 sharp_tex(vec2 at) {{
    {sample}
}}
void main() {{
{main}
}}"#
    )
}

/// Loads a sharp material: [`SHARP_VERTEX`] and `fragment` (from [`fragment`]), with the
/// given pipeline and the extra uniforms. `TexSize` and `Scale` start at 1, so a draw that
/// does not set them (a flat colour) samples a sane grid.
pub fn material(fragment: &str, pipeline_params: PipelineParams, extra: Vec<UniformDesc>) -> Result<Material, macroquad::Error> {
    material_with_textures(fragment, pipeline_params, extra, Vec::new())
}

/// [`material`] with extra sampler `textures` (their names, bound by the caller with `set_texture`).
pub fn material_with_textures(fragment: &str, pipeline_params: PipelineParams, extra: Vec<UniformDesc>, textures: Vec<String>) -> Result<Material, macroquad::Error> {
    material_in(SHARP_VERTEX, fragment, pipeline_params, extra, textures)
}

/// [`material_with_textures`] with the vertex stage `vertex` (a sharp vertex shader).
pub fn material_in(vertex: &str, fragment: &str, pipeline_params: PipelineParams, mut extra: Vec<UniformDesc>, textures: Vec<String>) -> Result<Material, macroquad::Error> {
    extra.push(UniformDesc::new("TexSize", UniformType::Float2));
    extra.push(UniformDesc::new("Scale", UniformType::Float2));
    let m = load_material(
        ShaderSource::Glsl { vertex, fragment },
        MaterialParams { uniforms: extra, textures, pipeline_params },
    )?;
    m.set_uniform("TexSize", Vec2::ONE);
    m.set_uniform("Scale", Vec2::ONE);
    Ok(m)
}

/// A sharp material whose `Scale` comes per vertex ([`SHARP_VERTEX_SCALE`], [`fragment_fog_scaled`]): only the texture's
/// `TexSize` is a uniform, so the draws of one texture batch together. `extra` are its own uniforms; `textures` are
/// their samplers' names, bound by the caller.
pub fn material_scaled_with(fragment: &str, pipeline_params: PipelineParams, mut extra: Vec<UniformDesc>, textures: Vec<String>) -> Result<Material, macroquad::Error> {
    extra.push(UniformDesc::new("TexSize", UniformType::Float2));
    let m = load_material(
        ShaderSource::Glsl { vertex: SHARP_VERTEX_SCALE, fragment },
        MaterialParams { uniforms: extra, textures, pipeline_params },
    )?;
    m.set_uniform("TexSize", Vec2::ONE);
    Ok(m)
}

/// A sharp material with the map sprites' fog (`fragment_fog`, `FogVis` bound by [`bind_fog`]).
pub fn material_fog(fragment: &str, pipeline_params: PipelineParams, extra: Vec<UniformDesc>) -> Result<Material, macroquad::Error> {
    material_in(SHARP_VERTEX, fragment, pipeline_params, extra, vec!["FogVis".to_string()])
}

/// Binds the fog table's texture to a fog material (`FogVis`). The table's texture object changes with the map's size,
/// so the materials are bound again whenever they are used for a sprite pass.
pub fn bind_fog(m: &Material) {
    m.set_texture("FogVis", super::sprite_fog::texture());
}

/// The colour blend of macroquad's own pipeline (source alpha over), for the draw material.
pub fn alpha_pipeline() -> PipelineParams {
    PipelineParams {
        color_blend: Some(BlendState::new(
            Equation::Add,
            BlendFactor::Value(BlendValue::SourceAlpha),
            BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
        )),
        ..Default::default()
    }
}

razdor::local! {
    static DRAW_MATERIAL: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    /// The blend contexts set by [`in_blend`], innermost last: `Some(material)` for a sharp
    /// blend, `None` for a plain one (text).
    static BLEND: std::cell::RefCell<Vec<Option<Material>>> = const { std::cell::RefCell::new(Vec::new()) };
    /// The sharp material kept set by [`session`] (`None` outside one).
    static BASE: std::cell::RefCell<Option<Material>> = const { std::cell::RefCell::new(None) };
    /// The draw material's last `TexSize` set in the session.
    static LAST: std::cell::Cell<Option<Vec2>> = const { std::cell::Cell::new(None) };
    /// A plain draw in the session set the default material and the session's material is not
    /// current until the next sharp draw ([`plain_draw`], [`session_material`]).
    static PLAIN: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Runs `draw` with the sharp material kept set: its sharp draws share one material switch,
/// and a uniform that did not change is not set again. Macroquad breaks its batch at every
/// material switch and at every uniform set, even an unchanged one, so this keeps the draws
/// of a pass batched as before. Inside, only sharp draws and white shapes (`draw_circle`,
/// lines) may draw: a plain textured draw would sample with the sharp uniforms. A blend
/// context ends by going back to this material ([`leave_blend`]).
pub fn session<R>(draw: impl FnOnce() -> R) -> R {
    let Some(m) = draw_material() else { return draw() };
    session_with(m, draw)
}

/// [`session`] with the painted shadows' factor of the frame («Освещение», `daylight::frame_shadow_k`): the
/// pass's sharp draws use the lit material (a building's painted shadow times `shadow_k`). Its uniform is set
/// once here, not per draw, so the pass batches as [`session`]'s does. `None` (the light is off) is [`session`]
/// itself, and so is a lit material that does not compile.
pub fn session_lit<R>(shadow_k: Option<f32>, draw: impl FnOnce() -> R) -> R {
    let (Some(k), Some(m)) = (shadow_k, lit_material()) else { return session(draw) };
    count_break();
    m.set_uniform("ShadowK", k);
    session_with(m, draw)
}

fn session_with<R>(m: Material, draw: impl FnOnce() -> R) -> R {
    struct End;
    impl Drop for End {
        fn drop(&mut self) {
            BASE.with(|b| b.borrow_mut().take());
            LAST.with(|l| l.set(None));
            PLAIN.with(|p| p.set(false));
            count_break();
            gl_use_default_material();
        }
    }
    if in_session() {
        return draw();
    }
    BASE.with(|b| *b.borrow_mut() = Some(m.clone()));
    LAST.with(|l| l.set(None));
    PLAIN.with(|p| p.set(false));
    count_break();
    bind_fog(&m);
    gl_use_material(&m);
    let _end = End;
    draw()
}

fn in_session() -> bool {
    BASE.with(|b| b.borrow().is_some())
}

/// The session's material (`None` outside a session): the lit one in a lit session.
fn session_base() -> Option<Material> {
    BASE.with(|b| b.borrow().clone())
}


/// The lit sharp draw's main: the painted shadows (a building's, alpha 0 < a ≤ 0.76) have their alpha times
/// `ShadowK` (`daylight::frame_shadow_k`); the colour is the picture's own. The ground and the sprites take no
/// relief (the grooves looked overcooked, the owner's report), so this is the only thing the light changes here.
const LIT_MAIN: &str = r#"
    vec4 c = sharp_tex(uv);
    float a = c.a;
    if (a > 0.0 && a <= 0.76) {
        a = min(a * ShadowK, 1.0);
    }
    vec4 col = color;
    col.a *= fog_vis();
    gl_FragColor = col * vec4(c.rgb, a);
"#;

razdor::local! {
    static LIT_MATERIAL: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// The lit sharp material, made once. `None` when it does not compile (then the pass is unlit).
fn lit_material() -> Option<Material> {
    LIT_MATERIAL.with(|m| {
        m.get_or_init(|| {
            let uniforms = vec![UniformDesc::new("ShadowK", UniformType::Float1)];
            material_scaled_with(&fragment_fog_scaled("uniform float ShadowK;", LIT_MAIN), alpha_pipeline(), uniforms, vec!["FogVis".to_string()])
                .map_err(|e| razdor::diag!("lit sharp material: {e}"))
                .ok()
        })
        .clone()
    })
}

/// Before a plain draw (text, a texture drawn without sharp sampling, or a helper's own
/// material outside a blend) inside a session: the default material is set for it. Consecutive
/// plain draws share that material; the next sharp draw sets the session's again
/// ([`session_material`]). Outside a session, or inside a blend context (whose material the
/// plain draw keeps), it does nothing. White shapes (`draw_rectangle`, lines, circles) need no
/// call: they draw with either material.
pub fn plain_draw() {
    if !in_session() || blend_context().is_some() {
        return;
    }
    if !PLAIN.with(|p| p.replace(true)) {
        count_break();
        gl_use_default_material();
    }
}

/// Makes the session's material current for a sharp draw inside a session (it may be off after
/// a plain draw).
fn session_material() {
    if PLAIN.with(|p| p.replace(false)) {
        if let Some(m) = BASE.with(|b| b.borrow().clone()) {
            count_break();
            gl_use_material(&m);
        }
    }
}

/// Ends a blend context: back to the session's material, or the default one outside a session.
pub fn leave_blend() {
    PLAIN.with(|p| p.set(false));
    count_break();
    match BASE.with(|b| b.borrow().clone()) {
        Some(m) => gl_use_material(&m),
        None => gl_use_default_material(),
    }
}

/// Sets the draw material's `TexSize` unless it is the value already set in this session. The scale of
/// each draw is in its vertices ([`sprite_mesh`]), so a change of texture is the only break here.
fn set_tex_size(m: &Material, tex_size: Vec2) {
    if LAST.with(|l| l.get()) != Some(tex_size) {
        count_break();
        m.set_uniform("TexSize", tex_size);
        LAST.with(|l| l.set(Some(tex_size)));
    }
}

local! {
    /// The quad of each sharp draw ([`draw`]): indices fixed, vertices and texture refilled per sprite (no
    /// allocation per sprite; the vertex buffer keeps its capacity).
    static SPRITE_MESH: std::cell::RefCell<Mesh> = std::cell::RefCell::new(Mesh { vertices: Vec::new(), indices: vec![0, 1, 2, 0, 2, 3], texture: None });
    /// The copy of a mesh with the sharp scale in its normals ([`draw_mesh_sharp`]), refilled per draw.
    static SCALED_MESH: std::cell::RefCell<Mesh> = std::cell::RefCell::new(Mesh { vertices: Vec::new(), indices: Vec::new(), texture: None });
}

razdor::local! {
    /// The fog cell of the map sprite being drawn: its table texture coordinate (`sprite_fog`), which the sharp draws
    /// put in their vertices' normal slot (`fog_normal`). (0, 0) for a draw with no fog of its own (the default).
    static FOG_CELL: std::cell::Cell<Vec2> = const { std::cell::Cell::new(Vec2::ZERO) };
}

/// Runs `draw` with the fog cell `cell` for its sharp draws (None: none). A map sprite's draws run in it, so the
/// sprite's alpha takes the table's visibility in the shader; any other draw runs with none.
pub fn with_fog_cell<R>(cell: Option<Vec2>, draw: impl FnOnce() -> R) -> R {
    let prev = FOG_CELL.with(|f| f.replace(cell.unwrap_or(Vec2::ZERO)));
    let r = draw();
    FOG_CELL.with(|f| f.set(prev));
    r
}

/// The vertices' normal slot for a draw: the scale in `xy` and the fog cell in `zw`.
fn fog_normal(scale: Vec2) -> Vec4 {
    let f = FOG_CELL.with(std::cell::Cell::get);
    vec4(scale.x, scale.y, f.x, f.y)
}

/// Draws the quad of a sharp draw ([`sprite_mesh`]'s vertices, in the shared [`SPRITE_MESH`]).
fn draw_sprite_mesh(tex: &Texture2D, x: f32, y: f32, color: Color, params: &DrawTextureParams, scale: Vec2) {
    SPRITE_MESH.with(|m| {
        let mut m = m.borrow_mut();
        fill_sprite_mesh(&mut m, tex, x, y, color, params, scale);
        draw_mesh(&m);
        m.texture = None;
    });
}

/// The quad of a sharp draw: exactly [`draw_texture_ex`]'s vertices and indices (the same arithmetic, so the
/// same picture), with `scale` in each vertex's normal slot for [`SHARP_VERTEX_SCALE`]. Refills `mesh`
/// (its indices are the quad's).
fn fill_sprite_mesh(mesh: &mut Mesh, tex: &Texture2D, x: f32, y: f32, color: Color, params: &DrawTextureParams, scale: Vec2) {
    let [width, height] = tex.size().to_array();
    let Rect { x: sx, y: sy, w: sw, h: sh } = params.source.unwrap_or(Rect { x: 0., y: 0., w: width, h: height });
    let (mut w, mut h) = match params.dest_size {
        Some(dst) => (dst.x, dst.y),
        _ => (sw, sh),
    };
    let (mut x, mut y) = (x, y);
    if params.flip_x {
        x += w;
        w = -w;
    }
    if params.flip_y {
        y += h;
        h = -h;
    }
    let pivot = params.pivot.unwrap_or(vec2(x + w / 2., y + h / 2.));
    let r = params.rotation;
    // The sine and cosine of the rotation, made once for the four corners (the same values `r.cos()` and `r.sin()`).
    let (sin_r, cos_r) = (r.sin(), r.cos());
    let corner = |px: f32, py: f32| {
        let p = vec2(px, py) - pivot;
        vec2(p.x * cos_r - p.y * sin_r, p.x * sin_r + p.y * cos_r) + pivot
    };
    let normal = fog_normal(scale);
    let c: [u8; 4] = color.into();
    let vertex = |p: Vec2, u: f32, v: f32| Vertex { position: vec3(p.x, p.y, 0.0), uv: vec2(u, v), color: c, normal };
    let (u0, v0) = (sx / width, sy / height);
    let (u1, v1) = ((sx + sw) / width, (sy + sh) / height);
    mesh.vertices.clear();
    mesh.vertices.extend([vertex(corner(x, y), u0, v0), vertex(corner(x + w, y), u1, v0), vertex(corner(x + w, y + h), u1, v1), vertex(corner(x, y + h), u0, v1)]);
    mesh.indices.clear();
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    mesh.texture = Some(tex.clone());
}

local! {
    /// The copy of a mesh with the fog cell in its normals ([`draw_fogged`]), refilled per draw.
    static FOGGED_MESH: std::cell::RefCell<Mesh> = std::cell::RefCell::new(Mesh { vertices: Vec::new(), indices: Vec::new(), texture: None });
}

/// Draws `mesh` (in a blend whose material takes the fog) with the fog cell of the sprite being drawn in its normals;
/// the mesh as it is when no fog applies.
fn draw_fogged(mesh: &Mesh) {
    let f = FOG_CELL.with(std::cell::Cell::get);
    if f == Vec2::ZERO {
        return draw_mesh(mesh);
    }
    FOGGED_MESH.with(|m| {
        let mut m = m.borrow_mut();
        m.vertices.clear();
        m.vertices.extend(mesh.vertices.iter().map(|v| Vertex { normal: vec4(v.normal.x, v.normal.y, f.x, f.y), ..*v }));
        m.indices.clear();
        m.indices.extend_from_slice(&mesh.indices);
        m.texture = mesh.texture.clone();
        draw_mesh(&m);
        m.texture = None;
    });
}

/// Draws `mesh` with `scale` in every vertex's normal slot (see [`fill_sprite_mesh`]), from the shared
/// [`SCALED_MESH`] (no allocation per draw once it has grown).
fn draw_scaled_mesh(mesh: &Mesh, scale: Vec2) {
    SCALED_MESH.with(|m| {
        let mut m = m.borrow_mut();
        let normal = fog_normal(scale);
        m.vertices.clear();
        m.vertices.extend(mesh.vertices.iter().map(|v| Vertex { normal, ..*v }));
        m.indices.clear();
        m.indices.extend_from_slice(&mesh.indices);
        m.texture = mesh.texture.clone();
        draw_mesh(&m);
        m.texture = None;
    });
}

local! {
    /// The sizes of the last textures drawn, by their miniquad id. A texture's size does not change,
    /// and miniquad never reuses an id, so a remembered size is the texture's own.
    static SIZES: std::cell::RefCell<Recent<TextureId, Vec2, 8>> = std::cell::RefCell::new(Recent::new());
}

/// The size of `tex` in texels: what `tex.width()` and `tex.height()` give, from [`SIZES`] (each of
/// those looks the texture up in the GL context).
pub fn texels(tex: &Texture2D) -> Vec2 {
    let id = tex.raw_miniquad_id();
    SIZES.with(|s| s.borrow_mut().get_or_insert_with(id, || vec2(tex.width(), tex.height())))
}

/// The sharp draw material, made once. `None` when the shader does not compile on this GPU
/// or driver: the draws then fall back to plain `draw_texture_ex` (the look before this
/// module), and the reason is logged once.
/// The sharp draw's main: the colour with its alpha times the map sprite's fog (`fog_vis`, 1 for the others).
const DRAW_MAIN: &str = "vec4 c = color; c.a *= fog_vis(); gl_FragColor = c * sharp_tex(uv);";

fn draw_material() -> Option<Material> {
    DRAW_MATERIAL.with(|m| {
        m.get_or_init(|| {
            material_scaled_with(&fragment_fog_scaled("", DRAW_MAIN), alpha_pipeline(), Vec::new(), vec!["FogVis".to_string()])
                .map_err(|e| razdor::diag!("sharp material: {e}"))
                .ok()
        })
        .clone()
    })
}

/// Sets `TexSize` (the texture's size in texels) and `Scale` (the draw's screen pixels per
/// texel) on a material that includes [`SHARP_UV_GLSL`], for the draw `params` of `tex`.
pub fn set_uniforms(m: &Material, tex: &Texture2D, params: &DrawTextureParams) {
    count_break();
    let tex_size = texels(tex);
    m.set_uniform("TexSize", tex_size);
    m.set_uniform("Scale", draw_scale(tex_size, params));
}

/// Runs `draw` inside a blend context set by the caller (`chrome::with_material`). With
/// `Some(material)` (a sharp blend) a sharp draw inside it sets its uniforms on that material
/// and keeps the blend; with `None` (a plain blend, text) it is the plain draw.
pub fn in_blend<R>(material: Option<Material>, draw: impl FnOnce() -> R) -> R {
    struct Leave;
    impl Drop for Leave {
        fn drop(&mut self) {
            BLEND.with(|b| {
                b.borrow_mut().pop();
            });
        }
    }
    BLEND.with(|b| b.borrow_mut().push(material));
    let _leave = Leave;
    draw()
}

/// The innermost blend context: `None` outside any blend, `Some(None)` in a plain blend,
/// `Some(Some(m))` in a sharp one.
fn blend_context() -> Option<Option<Material>> {
    BLEND.with(|b| b.borrow().last().cloned())
}

/// `draw_texture_ex` with sharp sampling: the same placement and parameters, each original
/// pixel drawn as a crisp square. Every call is its own draw (the uniforms change per draw).
pub fn draw(tex: &Texture2D, x: f32, y: f32, color: Color, params: DrawTextureParams) {
    count_draw(Some(tex), 4);
    match blend_context() {
        Some(Some(m)) => {
            set_uniforms(&m, tex, &params);
            // The same quad as `draw_texture_ex` (`fill_sprite_mesh`), with the fog cell in its vertices.
            let scale = draw_scale(texels(tex), &params);
            draw_sprite_mesh(tex, x, y, color, &params, scale);
        }
        Some(None) => draw_texture_ex(tex, x, y, color, params),
        None => match session_base() {
            Some(m) => {
                let tex_size = texels(tex);
                set_tex_size(&m, tex_size);
                session_material();
                draw_sprite_mesh(tex, x, y, color, &params, draw_scale(tex_size, &params));
            }
            None => match draw_material() {
                Some(m) => {
                    let tex_size = texels(tex);
                    count_break();
                    m.set_uniform("TexSize", tex_size);
                    bind_fog(&m);
                    gl_use_material(&m);
                    draw_sprite_mesh(tex, x, y, color, &params, draw_scale(tex_size, &params));
                    gl_use_default_material();
                }
                None => draw_texture_ex(tex, x, y, color, params),
            },
        },
    }
}

/// `draw_mesh` with sharp sampling, for a mesh whose texture is drawn at `scale` screen pixels
/// per texel on each axis (the swaying trees: their atlas quad). Same blend rule as [`draw`].
pub fn draw_mesh_sharp(mesh: &Mesh, scale: Vec2) {
    count_draw(mesh.texture.as_ref(), mesh.vertices.len());
    match blend_context() {
        Some(Some(m)) => {
            count_break();
            if let Some(tex) = mesh.texture.as_ref() {
                m.set_uniform("TexSize", texels(tex));
            }
            m.set_uniform("Scale", scale.abs() * RASTER.with(std::cell::Cell::get));
            draw_fogged(mesh);
        }
        Some(None) => draw_mesh(mesh),
        None => match session_base() {
            Some(m) if mesh.texture.is_some() => {
                set_tex_size(&m, mesh.texture.as_ref().map_or(Vec2::ONE, texels));
                session_material();
                draw_scaled_mesh(mesh, scale.abs());
            }
            Some(_) => draw_mesh(mesh),
            None => match draw_material() {
                Some(m) if mesh.texture.is_some() => {
                    count_break();
                    if let Some(tex) = mesh.texture.as_ref() {
                        m.set_uniform("TexSize", texels(tex));
                    }
                    bind_fog(&m);
                    gl_use_material(&m);
                    draw_scaled_mesh(mesh, scale.abs() * RASTER.with(std::cell::Cell::get));
                    gl_use_default_material();
                }
                _ => draw_mesh(mesh),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-5;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a - b).abs().max_element() < EPS
    }

    #[test]
    fn sharp_uv_texel_centre_unchanged() {
        let tex = vec2(4.0, 4.0);
        let uv = vec2(0.375, 0.625);
        assert!(close(sharp_uv_ref(uv, tex, Vec2::splat(3.0)), uv));
    }

    #[test]
    fn sharp_uv_inside_texel_snaps_to_centre() {
        let tex = vec2(4.0, 4.0);
        let uv = vec2(0.3, 0.55); // texel (1.2, 2.2)
        assert!(close(sharp_uv_ref(uv, tex, Vec2::splat(3.0)), vec2(0.375, 0.625)));
    }

    #[test]
    fn sharp_uv_edge_band_blends() {
        let tex = vec2(4.0, 4.0);
        // 0.05 texel past the edge at x = 2.0: inside the blend band (2.0 ± 1/6).
        let near = sharp_uv_ref(vec2(2.05 / 4.0, 0.625), tex, Vec2::splat(3.0));
        let x = near.x * 4.0;
        assert!(x > 1.5 && x < 2.5, "texel x {x} should lie strictly between two centres");
        // 0.3 texel past the edge: outside the band, exactly the next texel's centre.
        let far = sharp_uv_ref(vec2(2.3 / 4.0, 0.625), tex, Vec2::splat(3.0));
        assert!((far.x * 4.0 - 2.5).abs() < EPS);
    }

    #[test]
    fn sharp_uv_minified_is_plain_bilinear() {
        let tex = vec2(4.0, 4.0);
        for s in [0.6_f32, 1.0] {
            for uv in [vec2(0.1, 0.9), vec2(0.37, 0.52), vec2(0.99, 0.01)] {
                let out = sharp_uv_ref(uv, tex, Vec2::splat(s));
                assert!(out.is_finite(), "scale {s} gave {out:?}");
                assert!(close(out, uv), "scale {s}: {uv:?} -> {out:?}");
            }
        }
    }

    #[test]
    fn scale_uses_source_rect() {
        let s = draw_scale(
            vec2(512.0, 512.0),
            &DrawTextureParams {
                source: Some(Rect::new(0.0, 0.0, 64.0, 32.0)),
                dest_size: Some(vec2(192.0, 96.0)),
                ..Default::default()
            },
        );
        assert!(close(s, vec2(3.0, 3.0)), "{s:?}");
    }

    #[test]
    fn sharp_uv_zero_size_is_finite() {
        // A flat colour's draw leaves the size uniforms at their start value; a zero size
        // must not divide by zero.
        let out = sharp_uv_ref(vec2(0.3, 0.6), Vec2::ZERO, Vec2::splat(2.0));
        assert!(out.is_finite(), "{out:?}");
    }

    #[test]
    fn scale_ignores_flip_sign() {
        let s = draw_scale(
            vec2(64.0, 64.0),
            &DrawTextureParams { dest_size: Some(vec2(-128.0, 128.0)), ..Default::default() },
        );
        assert!(close(s, vec2(2.0, 2.0)), "{s:?}");
    }
}

razdor::local! {
    static WINDOWS_MATERIAL: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// The lit windows' main (`windows_pass`): the emissive layer (the mask's G, `dt::windows`) as a candle
/// glow at night, added over what is drawn (additive: the pass's blend), after the map's tint.
const WINDOWS_MAIN: &str = r#"
    vec4 bm = texture2D(BuildingMask, uv);
    float hsh = fract(sin(dot(floor(spos / 40.0), vec2(127.1, 311.7))) * 43758.5453);
    float flick = 0.94 + 0.06 * sin(mod(Clock, 1000.0) * 2.3 + hsh * 6.2832);
    // Candle-orange: the core a warm orange (yellower on some buildings, by the hash), the halo a deeper
    // amber; the emissive value picks between them. Intensity 1.2 by the night, the sprite's fade with it.
    vec3 core = mix(vec3(1.0, 0.70, 0.30), vec3(1.0, 0.78, 0.34), hsh);
    vec3 halo = vec3(0.9, 0.45, 0.15);
    vec3 col = mix(halo, core, smoothstep(0.35, 1.0, bm.g));
    gl_FragColor = vec4(col * (bm.g * Night * flick * 1.2 * color.a * fog_vis()), 1.0);
"#;

/// The additive windows material, made once. `None` when it does not compile (no windows then).
fn windows_material() -> Option<Material> {
    WINDOWS_MATERIAL.with(|m| {
        m.get_or_init(|| {
            let extra = "uniform sampler2D BuildingMask;\nuniform float Night;\nuniform float Clock;\nvarying highp vec2 spos;";
            let pipeline = PipelineParams {
                color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Value(BlendValue::SourceAlpha), BlendFactor::One)),
                ..Default::default()
            };
            let uniforms = vec![UniformDesc::new("Night", UniformType::Float1), UniformDesc::new("Clock", UniformType::Float1)];
            material_in(SHARP_VERTEX_SPOS, &fragment_fog(extra, WINDOWS_MAIN), pipeline, uniforms, vec!["BuildingMask".to_string(), "FogVis".to_string()])
                .map_err(|e| razdor::diag!("windows material: {e}"))
                .ok()
        })
        .clone()
    })
}

/// Runs `draw` (the building pictures' draws) as the lit windows: an additive pass with the building mask
/// bound, after the map's tint (the glow must not be darkened by the night's colour). `night` is the frame's
/// night factor; the sprites' own alpha fades them into the fog as usual.
pub fn windows_pass<R>(mask: &Texture2D, night: f32, draw: impl FnOnce() -> R) -> R {
    count_break();
    let Some(m) = windows_material() else { return draw() };
    m.set_texture("BuildingMask", mask.clone());
    bind_fog(&m);
    m.set_uniform("Night", night);
    m.set_uniform("Clock", super::clock::now() as f32);
    struct End;
    impl Drop for End {
        fn drop(&mut self) {
            gl_use_default_material();
        }
    }
    gl_use_material(&m);
    let _end = End;
    in_blend(Some(m), draw)
}
