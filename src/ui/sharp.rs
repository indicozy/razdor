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

use razdor::local;
use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams, TextureId};
use macroquad::prelude::*;

use super::memo::Recent;
use super::daylight;
use super::lights::{self, SLOTS};

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

/// GLSL ES 1.00 for the relief («Освещение», `daylight::relief_at`), pasted into a shader that
/// declares the uniforms `Light` (vec3), `Strength` and `Cavity` (floats) before it. Each texture
/// sample gives a height h (its luminance); the four neighbours at ±[`RELIEF_R`] texels give the
/// slope, and the four at three times that give the cavities. `relief_shade` takes the samples and
/// returns the lit colour. With `Strength` 0 the caller skips it: the colour is then unchanged.
pub const RELIEF_GLSL: &str = r#"
const float RELIEF_R = 1.5;
const float RELIEF_K = 2.0;
float luma(vec3 c) {
    return dot(c, vec3(0.299, 0.587, 0.114));
}
// `rgb` lit by the slope from the heights `xp`, `xm` (at +x, -x), `yp`, `ym` (at +y, -y) around
// the centre height `h`, and the cavity from `deep`, the mean height at ±3 R.
vec3 relief_shade(vec3 rgb, float h, float xp, float xm, float yp, float ym, float deep) {
    vec3 n = normalize(vec3(-(xp - xm) * RELIEF_K, -(yp - ym) * RELIEF_K, 1.0));
    float f = dot(n, Light) / Light.z;
    float cav = clamp(deep - h, 0.0, 0.25) * 4.0;
    return clamp(rgb * mix(1.0, f, Strength) * (1.0 - Cavity * cav), 0.0, 1.0);
}
"#;

/// GLSL ES 1.00 for the point lights («Качество света», `lights`), pasted after [`RELIEF_GLSL`] into a
/// shader that declares `uniform vec4 LightA[8]` (each light: x, y in the pass's 2D pixels, radius in
/// pixels, strength), `uniform vec4 LightI` (the lights in use, the ones raking the relief, the night's
/// factor, the building band's share of the texture's height) and `varying highp vec2 spos` (the
/// pixel's 2D point, [`SHARP_VERTEX`]). A light's pool is the falloff to its radius; the relief is the
/// difference of the light's direction from the flat one (zero on flat ground). Night 0 switches it all off.
pub const POINT_GLSL: &str = r#"
const float POINT_K = 0.6;
const float EMIT_K = 0.45;
const vec3 EMIT_WARM = vec3(1.0, 0.72, 0.40);
float point_falloff(float d, float r) {
    float x = d / r;
    float t = max(1.0 - x * x, 0.0);
    return t * t;
}
// The multiplier of the relief from the point lights at `at` with normal `n` (1 where none rakes it; the
// faces turned away from a light keep their colour, so the grain of the stone never darkens).
float point_shade(vec2 at, vec3 n) {
    float s = 0.0;
    for (int i = 0; i < 8; i++) {
        if (float(i) >= LightI.y) break;
        vec4 L = LightA[i];
        float d = distance(at, L.xy);
        if (d < L.z) {
            vec3 l = normalize(vec3(L.xy - at, 0.5 * L.z));
            s += point_falloff(d, L.z) * L.w * max(dot(n, l) - l.z, 0.0);
        }
    }
    return clamp(1.0 + POINT_K * LightI.z * s, 0.0, 1.8);
}
// The night's glow of the lights at `at`, 0 to 1 (a building's openings take it).
float glow_at(vec2 at) {
    float e = 0.0;
    for (int i = 0; i < 8; i++) {
        if (float(i) >= LightI.x) break;
        vec4 L = LightA[i];
        e += point_falloff(distance(at, L.xy), L.z) * L.w;
    }
    return min(e, 1.0);
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
varying highp vec2 uv;
varying lowp vec4 color;
// The vertex's 2D point before the model and projection: the pass's own pixels, the space the lights are in.
varying highp vec2 spos;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
    spos = position.xy;
}"#;

/// A fragment shader with `sharp_uv` and the uniforms `TexSize` and `Scale` declared, then
/// `extra` (more declarations, e.g. `uniform float Rotation;`), then `main` as the body of
/// `void main()`. In `main`: `uv` and `color` are the varyings, `sharp_tex(at)` samples
/// `Texture` at texture coordinate `at` with sharp sampling. The precision is highp where the
/// device has it; without highp in the fragment stage this fails to compile and the caller
/// gets the plain draw.
pub fn fragment(extra: &str, main: &str) -> String {
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
uniform vec2 Scale;
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
pub fn material(fragment: &str, pipeline_params: PipelineParams, mut extra: Vec<UniformDesc>) -> Result<Material, macroquad::Error> {
    extra.push(UniformDesc::new("TexSize", UniformType::Float2));
    extra.push(UniformDesc::new("Scale", UniformType::Float2));
    let m = load_material(
        ShaderSource::Glsl { vertex: SHARP_VERTEX, fragment },
        MaterialParams { uniforms: extra, pipeline_params, ..Default::default() },
    )?;
    m.set_uniform("TexSize", Vec2::ONE);
    m.set_uniform("Scale", Vec2::ONE);
    Ok(m)
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
    /// The draw material's last uniforms (texture size, scale) set in the session.
    static LAST: std::cell::Cell<Option<(Vec2, Vec2)>> = const { std::cell::Cell::new(None) };
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

/// [`session`] with the relief of the frame («Освещение», `daylight::relief_at`): the pass's sharp
/// draws use the lit material (the relief on the colour, the painted shadows' alpha, the point lights'
/// relief and the glowing openings of the building band, `points`). Its uniforms are set once here, not
/// per draw, so the pass batches as [`session`]'s does. `None` (the light is off) is [`session`] itself,
/// and so is a lit material that does not compile. `points` None: no point light (Low, or no light).
pub fn session_lit<R>(relief: Option<daylight::Relief>, points: Option<&lights::Uniforms>, draw: impl FnOnce() -> R) -> R {
    let (Some(r), Some(m)) = (relief, lit_material()) else { return session(draw) };
    m.set_uniform("Light", vec3(r.light[0], r.light[1], r.light[2]));
    m.set_uniform("Strength", r.strength);
    m.set_uniform("Cavity", r.cavity);
    m.set_uniform("ShadowK", r.shadow_k);
    let points = points.copied().unwrap_or(lights::Uniforms { lights: [Vec4::ZERO; lights::SLOTS], info: Vec4::ZERO });
    m.set_uniform_array("LightA", &points.lights[..]);
    m.set_uniform("LightI", points.info);
    session_with(m, draw)
}

fn session_with<R>(m: Material, draw: impl FnOnce() -> R) -> R {
    struct End;
    impl Drop for End {
        fn drop(&mut self) {
            BASE.with(|b| b.borrow_mut().take());
            LAST.with(|l| l.set(None));
            PLAIN.with(|p| p.set(false));
            gl_use_default_material();
        }
    }
    if in_session() {
        return draw();
    }
    BASE.with(|b| *b.borrow_mut() = Some(m.clone()));
    LAST.with(|l| l.set(None));
    PLAIN.with(|p| p.set(false));
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


/// The lit sharp draw's main: the colour lit by its relief (the neighbours of a transparent pixel
/// count as its own height, so a sprite's edge has no rim), then the painted shadows: a pixel of
/// alpha 0 < a ≤ 0.76 (a building's shadow) has its alpha times `ShadowK`.
const LIT_MAIN: &str = r#"
    vec4 c = sharp_tex(uv);
    float a = c.a;
    vec3 rgb = c.rgb;
    // How much darker this pixel is than its surroundings (a cavity: the openings of a building).
    float cavity_here = 0.0;
    if (a > 0.0) {
        vec2 ox = vec2(RELIEF_R, 0.0) / TexSize;
        vec2 oy = vec2(0.0, RELIEF_R) / TexSize;
        float h = luma(c.rgb);
        vec4 sxp = sharp_tex(uv + ox);
        vec4 sxm = sharp_tex(uv - ox);
        vec4 syp = sharp_tex(uv + oy);
        vec4 sym = sharp_tex(uv - oy);
        float xp = sxp.a < 0.5 ? h : luma(sxp.rgb);
        float xm = sxm.a < 0.5 ? h : luma(sxm.rgb);
        float yp = syp.a < 0.5 ? h : luma(syp.rgb);
        float ym = sym.a < 0.5 ? h : luma(sym.rgb);
        vec2 ox3 = ox * 3.0;
        vec2 oy3 = oy * 3.0;
        vec4 fxp = sharp_tex(uv + ox3);
        vec4 fxm = sharp_tex(uv - ox3);
        vec4 fyp = sharp_tex(uv + oy3);
        vec4 fym = sharp_tex(uv - oy3);
        float deep = ((fxp.a < 0.5 ? h : luma(fxp.rgb)) + (fxm.a < 0.5 ? h : luma(fxm.rgb)) + (fyp.a < 0.5 ? h : luma(fyp.rgb)) + (fym.a < 0.5 ? h : luma(fym.rgb))) * 0.25;
        rgb = relief_shade(rgb, h, xp, xm, yp, ym, deep);
        cavity_here = 0.25 * (xp + xm + yp + ym) - h;
        // The point lights rake the relief at night (`LightI.y` of them, `LightI.z` the night).
        if (LightI.y > 0.0 && LightI.z > 0.0) {
            rgb = clamp(rgb * point_shade(spos, normalize(vec3(-(xp - xm) * RELIEF_K, -(yp - ym) * RELIEF_K, 1.0))), 0.0, 1.0);
        }
    }
    if (a > 0.0 && a <= 0.76) {
        a = min(a * ShadowK, 1.0);
    }
    // A building picture's openings (the band above `LightI.w`): dark cavities of the solid body glow warm
    // by the lights' reach at night. Decorations and figures never do (below the band, or another material).
    if (c.a >= 0.8 && uv.y < LightI.w && LightI.z > 0.0 && LightI.x > 0.0) {
        vec3 raw = c.rgb;
        float mx = max(raw.r, max(raw.g, raw.b));
        float mn = min(raw.r, min(raw.g, raw.b));
        float sat = mx > 0.001 ? (mx - mn) / mx : 0.0;
        // Dark (luma 0.14 to 0.22 raw), darker than the four pixels 1.5 texels away (a cavity), and not a warm
        // colour (saturation 0.36 to 0.44). The art has no window layer: the seams of a wall's corners pass too.
        float opening = (1.0 - smoothstep(0.14, 0.22, luma(raw))) * smoothstep(0.03, 0.10, cavity_here) * (1.0 - smoothstep(0.36, 0.44, sat));
        rgb = min(rgb + EMIT_WARM * (EMIT_K * LightI.z * opening * glow_at(spos)), vec3(1.0));
    }
    gl_FragColor = color * vec4(rgb, a);
"#;

razdor::local! {
    static LIT_MATERIAL: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// The lit sharp material, made once. `None` when it does not compile (then the pass is unlit).
fn lit_material() -> Option<Material> {
    LIT_MATERIAL.with(|m| {
        m.get_or_init(|| {
            let extra = format!("uniform vec3 Light;\nuniform float Strength;\nuniform float Cavity;\nuniform float ShadowK;\nuniform vec4 LightA[{SLOTS}];\nuniform vec4 LightI;\nvarying highp vec2 spos;\n{RELIEF_GLSL}{POINT_GLSL}");
            let uniforms = vec![
                UniformDesc::new("Light", UniformType::Float3),
                UniformDesc::new("Strength", UniformType::Float1),
                UniformDesc::new("Cavity", UniformType::Float1),
                UniformDesc::new("ShadowK", UniformType::Float1),
                UniformDesc::new("LightA", UniformType::Float4).array(SLOTS),
                UniformDesc::new("LightI", UniformType::Float4),
            ];
            material(&fragment(&extra, LIT_MAIN), alpha_pipeline(), uniforms)
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
        gl_use_default_material();
    }
}

/// Makes the session's material current for a sharp draw inside a session (it may be off after
/// a plain draw).
fn session_material() {
    if PLAIN.with(|p| p.replace(false)) {
        if let Some(m) = BASE.with(|b| b.borrow().clone()) {
            gl_use_material(&m);
        }
    }
}

/// Ends a blend context: back to the session's material, or the default one outside a session.
pub fn leave_blend() {
    PLAIN.with(|p| p.set(false));
    match BASE.with(|b| b.borrow().clone()) {
        Some(m) => gl_use_material(&m),
        None => gl_use_default_material(),
    }
}

/// Sets the draw material's `TexSize` and `Scale` unless they are the values already set in
/// this session.
fn set_cached(m: &Material, tex_size: Vec2, scale: Vec2) {
    if LAST.with(|l| l.get()) != Some((tex_size, scale)) {
        m.set_uniform("TexSize", tex_size);
        m.set_uniform("Scale", scale);
        LAST.with(|l| l.set(Some((tex_size, scale))));
    }
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
fn draw_material() -> Option<Material> {
    DRAW_MATERIAL.with(|m| {
        m.get_or_init(|| {
            material(&fragment("", "gl_FragColor = color * sharp_tex(uv);"), alpha_pipeline(), Vec::new())
                .map_err(|e| razdor::diag!("sharp material: {e}"))
                .ok()
        })
        .clone()
    })
}

/// Sets `TexSize` (the texture's size in texels) and `Scale` (the draw's screen pixels per
/// texel) on a material that includes [`SHARP_UV_GLSL`], for the draw `params` of `tex`.
pub fn set_uniforms(m: &Material, tex: &Texture2D, params: &DrawTextureParams) {
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
    match blend_context() {
        Some(Some(m)) => {
            set_uniforms(&m, tex, &params);
            draw_texture_ex(tex, x, y, color, params);
        }
        Some(None) => draw_texture_ex(tex, x, y, color, params),
        None => match session_base() {
            Some(m) => {
                let tex_size = texels(tex);
                set_cached(&m, tex_size, draw_scale(tex_size, &params));
                session_material();
                draw_texture_ex(tex, x, y, color, params);
            }
            None => match draw_material() {
                Some(m) => {
                    set_uniforms(&m, tex, &params);
                    gl_use_material(&m);
                    draw_texture_ex(tex, x, y, color, params);
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
    let set = |m: &Material| {
        if let Some(tex) = mesh.texture.as_ref() {
            m.set_uniform("TexSize", texels(tex));
        }
        m.set_uniform("Scale", scale.abs() * RASTER.with(std::cell::Cell::get));
    };
    match blend_context() {
        Some(Some(m)) => {
            set(&m);
            draw_mesh(mesh);
        }
        Some(None) => draw_mesh(mesh),
        None => match session_base() {
            Some(m) if mesh.texture.is_some() => {
                let tex_size = mesh.texture.as_ref().map_or(Vec2::ONE, texels);
                set_cached(&m, tex_size, scale.abs());
                session_material();
                draw_mesh(mesh);
            }
            Some(_) => draw_mesh(mesh),
            None => match draw_material() {
                Some(m) if mesh.texture.is_some() => {
                    set(&m);
                    gl_use_material(&m);
                    draw_mesh(mesh);
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
