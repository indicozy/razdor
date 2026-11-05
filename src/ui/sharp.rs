//! Sharp bilinear sampling for Discord Times' art.
//!
//! Plain bilinear filtering blurs every original pixel into its neighbours when the picture
//! is enlarged, and at uneven scales the pixels come out with different widths. The sharp
//! sampler keeps each original pixel a crisp square of the same size: inside a texel the
//! colour is that texel's own; only a thin band at each texel edge (one screen pixel wide)
//! is blended, which the hardware's bilinear filter does for us.
//!
//! [`SHARP_UV_GLSL`] is the GLSL function; [`draw`] is a drop-in for `draw_texture_ex` that
//! samples with it. The textures stay `FilterMode::Linear`: the function needs the hardware
//! bilinear step inside the blend band.

use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
use macroquad::prelude::*;

/// GLSL ES 1.00 source of `sharp_uv`, pasted into fragment shaders (the caller declares
/// the precision and the `TexSize` / `Scale` uniforms). `uv` is the texture coordinate,
/// `tex_size` the texture's size in texels, `scale` the draw's screen pixels per texel.
/// Scale below 1 (a picture drawn smaller than its texels) gives plain bilinear.
pub const SHARP_UV_GLSL: &str = r#"
vec2 sharp_uv(vec2 uv, vec2 tex_size, vec2 scale) {
    vec2 s = max(scale, vec2(1.0));
    vec2 texel = uv * tex_size;
    vec2 base = floor(texel - 0.5) + 0.5;
    vec2 f = texel - base;
    vec2 t = clamp((f - 0.5) * s + 0.5, 0.0, 1.0);
    return (base + t) / tex_size;
}
"#;

/// Rust mirror of [`SHARP_UV_GLSL`], line for line, for the tests. Keep the two in step.
#[cfg(test)]
fn sharp_uv_ref(uv: Vec2, tex_size: Vec2, scale: Vec2) -> Vec2 {
    let s = scale.max(Vec2::ONE);
    let texel = uv * tex_size;
    let base = (texel - 0.5).floor() + 0.5;
    let f = texel - base;
    let t = ((f - 0.5) * s + 0.5).clamp(Vec2::ZERO, Vec2::ONE);
    (base + t) / tex_size
}

/// Screen pixels per texel on each axis for a draw: the destination size over the source
/// rectangle's size (the whole texture when there is no source rectangle). The sign of a
/// flipped destination is ignored.
pub fn draw_scale(tex_size: Vec2, params: &DrawTextureParams) -> Vec2 {
    let source = params.source.map(|r| r.size()).unwrap_or(tex_size);
    let dest = params.dest_size.unwrap_or(source);
    dest.abs() / source
}

const SHARP_VERTEX: &str = r#"#version 100
attribute vec3 position;
attribute vec2 texcoord;
attribute vec4 color0;
varying highp vec2 uv;
varying lowp vec4 color;
uniform mat4 Model;
uniform mat4 Projection;
void main() {
    gl_Position = Projection * Model * vec4(position, 1);
    color = color0 / 255.0;
    uv = texcoord;
}"#;

/// The fragment source: `uv` stays highp (the texel maths needs it on large textures), and
/// the precision of the other values follows the device. A device without highp in the
/// fragment stage fails to compile this and gets the plain draw (see [`material`]).
fn sharp_fragment() -> String {
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
{SHARP_UV_GLSL}
void main() {{
    gl_FragColor = color * texture2D(Texture, sharp_uv(uv, TexSize, Scale));
}}"#
    )
}

razdor::local! {
    static SHARP: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// The sharp material, made once. `None` when the shader does not compile on this GPU or
/// driver: the draws then fall back to plain `draw_texture_ex` (the look before this module),
/// and the reason is logged once.
fn material() -> Option<Material> {
    SHARP.with(|m| {
        m.get_or_init(|| {
            // The default pipeline blends with no blend state at all, so the picture's
            // transparent pixels would draw black: the same colour blend macroquad's own
            // pipeline uses (its alpha blend stays off, as there).
            let pipeline_params = PipelineParams {
                color_blend: Some(BlendState::new(
                    Equation::Add,
                    BlendFactor::Value(BlendValue::SourceAlpha),
                    BlendFactor::OneMinusValue(BlendValue::SourceAlpha),
                )),
                ..Default::default()
            };
            load_material(
                ShaderSource::Glsl { vertex: SHARP_VERTEX, fragment: &sharp_fragment() },
                MaterialParams {
                    uniforms: vec![
                        UniformDesc::new("TexSize", UniformType::Float2),
                        UniformDesc::new("Scale", UniformType::Float2),
                    ],
                    pipeline_params,
                    ..Default::default()
                },
            )
            .map_err(|e| razdor::diag!("sharp material: {e}"))
            .ok()
        })
        .clone()
    })
}

/// Sets `TexSize` (the texture's size in texels) and `Scale` (the draw's screen pixels per
/// texel) on a material that includes [`SHARP_UV_GLSL`], for the draw `params` of `tex`.
pub fn set_uniforms(m: &Material, tex: &Texture2D, params: &DrawTextureParams) {
    let tex_size = vec2(tex.width(), tex.height());
    m.set_uniform("TexSize", tex_size);
    m.set_uniform("Scale", draw_scale(tex_size, params));
}

razdor::local! {
    /// How many blend materials (`chrome::additive` and the like) are set around the draw.
    static BLEND_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Runs `draw` with a blend material set by the caller (`chrome::with_material`). Sharp draws
/// inside it are plain: their own material would replace the blend, and the blend's shader
/// does not sample sharp yet.
pub fn in_blend<R>(draw: impl FnOnce() -> R) -> R {
    struct Leave;
    impl Drop for Leave {
        fn drop(&mut self) {
            BLEND_DEPTH.with(|d| d.set(d.get() - 1));
        }
    }
    BLEND_DEPTH.with(|d| d.set(d.get() + 1));
    let _leave = Leave;
    draw()
}

fn in_blend_now() -> bool {
    BLEND_DEPTH.with(|d| d.get() > 0)
}

/// `draw_texture_ex` with sharp sampling: the same placement and parameters, each original
/// pixel drawn as a crisp square. Every call is its own draw (the uniforms change per draw).
/// Inside a blend material ([`in_blend`]) it is the plain draw.
pub fn draw(tex: &Texture2D, x: f32, y: f32, color: Color, params: DrawTextureParams) {
    let material = if in_blend_now() { None } else { material() };
    match material {
        Some(m) => {
            set_uniforms(&m, tex, &params);
            gl_use_material(&m);
            draw_texture_ex(tex, x, y, color, params);
            gl_use_default_material();
        }
        None => draw_texture_ex(tex, x, y, color, params),
    }
}

/// `draw_mesh` with sharp sampling, for a mesh whose texture is drawn at `scale` screen pixels
/// per texel on each axis (the swaying trees: their atlas quad). Same blend rule as [`draw`].
pub fn draw_mesh_sharp(mesh: &Mesh, scale: Vec2) {
    let material = if in_blend_now() { None } else { material() };
    match (material, mesh.texture.as_ref()) {
        (Some(m), Some(tex)) => {
            m.set_uniform("TexSize", vec2(tex.width(), tex.height()));
            m.set_uniform("Scale", scale.abs());
            gl_use_material(&m);
            draw_mesh(mesh);
            gl_use_default_material();
        }
        _ => draw_mesh(mesh),
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
    fn scale_ignores_flip_sign() {
        let s = draw_scale(
            vec2(64.0, 64.0),
            &DrawTextureParams { dest_size: Some(vec2(-128.0, 128.0)), ..Default::default() },
        );
        assert!(close(s, vec2(2.0, 2.0)), "{s:?}");
    }
}
