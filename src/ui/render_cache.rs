//! A picture drawn once into a render target and shown again while its key stays the same.
//!
//! [`Cached::show`] runs the drawing into the target when the key differs from the last fill,
//! then draws the target on the screen. While a fill runs, the 2D space and every 3D view
//! ([`back_to_2d`], [`pass`]) draw into the target, so the code that draws the picture does not
//! change: its cameras are redirected here. The target is the screen's size in physical pixels
//! and its 2D space is the screen's, so the picture lands on the same pixels as a direct draw.
//! Rows: macroquad's Camera2D with a target keeps the screen's projection, so the texture comes
//! out upside down and is drawn with `flip_y`.

use macroquad::camera::{set_camera, set_default_camera, Camera2D};
use macroquad::material::{gl_use_default_material, gl_use_material, load_material, Material, MaterialParams};
use macroquad::math::{vec2, Rect};
use macroquad::miniquad::{BlendFactor, BlendState, Equation, PipelineParams, ShaderSource};
use macroquad::prelude::WHITE;
use macroquad::texture::{draw_texture_ex, render_target, DrawTextureParams, FilterMode, RenderTarget};
use macroquad::window::{screen_height, screen_width};

/// The largest target side in physical pixels; larger pictures are drawn live.
const MAX_SIDE: f32 = 8192.0;

/// The copy composite: the texel as it is, its colour written over the screen without blending.
/// The picture's own alpha is below 1 where its shading is translucent (the fog, the dim), so
/// an alpha blend would mix in what the screen held before; a copy gives the colour as drawn.
const COPY_FRAGMENT: &str = r#"#version 100
precision lowp float;
varying lowp vec4 color;
varying highp vec2 uv;
uniform sampler2D Texture;
void main() {
    gl_FragColor = texture2D(Texture, uv) * color;
}"#;

razdor::local! {
    /// The target the 2D space and the 3D views draw into while a picture is filled.
    static FILLING: std::cell::RefCell<Option<RenderTarget>> = const { std::cell::RefCell::new(None) };
    /// The copy composite's material, made once (`None` when the shader does not compile).
    static COPY: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
}

/// The copy material, made once.
fn copy_material() -> Option<Material> {
    COPY.with(|m| {
        m.get_or_init(|| {
            let params = PipelineParams {
                color_blend: Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::Zero)),
                alpha_blend: Some(BlendState::new(Equation::Add, BlendFactor::One, BlendFactor::Zero)),
                ..Default::default()
            };
            load_material(
                ShaderSource::Glsl { vertex: super::sharp::SHARP_VERTEX, fragment: COPY_FRAGMENT },
                MaterialParams { pipeline_params: params, ..Default::default() },
            )
            .map_err(|e| razdor::diag!("copy material: {e}"))
            .ok()
        })
        .clone()
    })
}

/// The target the 3D views draw into (their render pass is its `render_pass`): the target while
/// a picture is filled, else `None` (the screen).
pub fn target() -> Option<RenderTarget> {
    FILLING.with(|f| f.borrow().clone())
}

/// Back to the 2D space: the screen, or the target of the fill in progress.
pub fn back_to_2d() {
    let filling = FILLING.with(|f| f.borrow().clone());
    match filling {
        Some(rt) => set_camera(&target_camera(&rt)),
        None => set_default_camera(),
    }
}

/// The 2D camera of a target: the screen's own space, drawn into `rt`.
fn target_camera(rt: &RenderTarget) -> Camera2D {
    Camera2D {
        render_target: Some(rt.clone()),
        ..Camera2D::from_display_rect(Rect::new(0.0, 0.0, screen_width(), screen_height()))
    }
}

/// Runs `f` with every camera drawing into `rt`; the screen's camera is back after.
fn fill(rt: &RenderTarget, f: impl FnOnce()) {
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            FILLING.with(|c| c.borrow_mut().take());
            set_default_camera();
        }
    }
    FILLING.with(|c| *c.borrow_mut() = Some(rt.clone()));
    let _done = Done;
    back_to_2d();
    f();
}

/// A picture kept between frames: the target it is drawn into and the key it was drawn for.
#[derive(Default)]
pub struct Cached {
    target: Option<RenderTarget>,
    key: Option<u64>,
}

impl Cached {
    /// Shows the picture `draw` makes for `key` on the screen. `draw` runs again only when the
    /// key differs from the last fill (or the screen's physical size changed). Without a usable
    /// target `draw` runs on the screen, as before.
    pub fn show(&mut self, key: u64, draw: impl FnOnce()) {
        let (w, h) = macroquad::miniquad::window::screen_size();
        if !(w >= 1.0 && h >= 1.0 && w <= MAX_SIDE && h <= MAX_SIDE) {
            self.target = None;
            self.key = None;
            draw();
            return;
        }
        let (pw, ph) = (w as u32, h as u32);
        let sized = self.target.as_ref().is_some_and(|t| t.texture.width() as u32 == pw && t.texture.height() as u32 == ph);
        if !sized {
            let rt = render_target(pw, ph);
            rt.texture.set_filter(FilterMode::Nearest);
            self.target = Some(rt);
            self.key = None;
        }
        let rt = self.target.clone().expect("target made above");
        if self.key != Some(key) {
            fill(&rt, draw);
            self.key = Some(key);
        }
        let (sw, sh) = (screen_width(), screen_height());
        let params = DrawTextureParams { dest_size: Some(vec2(sw, sh)), flip_y: true, ..Default::default() };
        match copy_material() {
            Some(m) => {
                gl_use_material(&m);
                draw_texture_ex(&rt.texture, 0.0, 0.0, WHITE, params);
                gl_use_default_material();
            }
            None => draw_texture_ex(&rt.texture, 0.0, 0.0, WHITE, params),
        }
    }
}
