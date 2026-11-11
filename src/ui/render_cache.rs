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
    /// The target the 2D space and the 3D views draw into while a picture is filled, and the 2D
    /// space it is filled in (the screen's, for [`fill`]).
    static FILLING: std::cell::RefCell<Option<(RenderTarget, Rect)>> = const { std::cell::RefCell::new(None) };
    /// The copy composite's material, made once (`None` when the shader does not compile).
    static COPY: std::cell::OnceCell<Option<Material>> = const { std::cell::OnceCell::new() };
    /// The map's base target while the world is drawn at a lower resolution (`map_scale`): the
    /// screen's 2D space drawn into it. [`back_to_2d`] and the end of a fill return to it, not to
    /// the screen, so every camera reset during the world drawing stays in the target.
    static BASE: std::cell::RefCell<Option<(RenderTarget, Rect)>> = const { std::cell::RefCell::new(None) };
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

/// The target a picture is being filled into, else `None`.
pub fn target() -> Option<RenderTarget> {
    FILLING.with(|f| f.borrow().as_ref().map(|(rt, _)| rt.clone()))
}

/// The target everything drawn now goes to: the fill in progress, else the map's base target,
/// else `None` (the screen). The 3D views take their render pass from it.
pub fn draw_target() -> Option<RenderTarget> {
    target().or_else(|| BASE.with(|b| b.borrow().as_ref().map(|(rt, _)| rt.clone())))
}

/// Back to the 2D space: the target of the fill in progress, else the map's base target, else
/// the screen.
pub fn back_to_2d() {
    super::sharp::count_break();
    let filling = FILLING.with(|f| f.borrow().clone());
    if let Some((rt, space)) = filling {
        set_camera(&target_camera(&rt, space));
        return;
    }
    match BASE.with(|b| b.borrow().clone()) {
        Some((rt, space)) => set_camera(&target_camera(&rt, space)),
        None => set_default_camera(),
    }
}

/// Runs `f` with the base target set to `rt` (its 2D space `space`, the screen's): every camera
/// reset inside `f` draws into `rt`. The screen's camera is back after.
pub fn with_base<R>(rt: &RenderTarget, space: Rect, f: impl FnOnce() -> R) -> R {
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            BASE.with(|b| b.borrow_mut().take());
            set_default_camera();
        }
    }
    BASE.with(|b| *b.borrow_mut() = Some((rt.clone(), space)));
    let _done = Done;
    back_to_2d();
    f()
}

/// The pixel size of the map's base target (`with_base`), if one is set: what a screen-sized
/// composite reads its `gl_FragCoord` against.
pub fn base_pixels() -> Option<(f32, f32)> {
    BASE.with(|b| b.borrow().as_ref().map(|(rt, _)| (rt.texture.width(), rt.texture.height())))
}

/// Draws `tex` over the whole screen, as it is (no blend: the copy composite), flipped as a
/// render target's texture comes out (see the module's note).
pub fn blit_screen(tex: &macroquad::texture::Texture2D) {
    let (sw, sh) = (screen_width(), screen_height());
    let params = DrawTextureParams { dest_size: Some(vec2(sw, sh)), flip_y: true, ..Default::default() };
    super::sharp::count_draw(Some(tex), 4);
    super::sharp::count_break();
    match copy_material() {
        Some(m) => {
            gl_use_material(&m);
            draw_texture_ex(tex, 0.0, 0.0, WHITE, params);
            gl_use_default_material();
        }
        None => draw_texture_ex(tex, 0.0, 0.0, WHITE, params),
    }
}

/// The 2D camera of a target: the 2D space `space` (the screen's, for a fill of the screen),
/// drawn into `rt`.
fn target_camera(rt: &RenderTarget, space: Rect) -> Camera2D {
    Camera2D { render_target: Some(rt.clone()), ..Camera2D::from_display_rect(space) }
}

/// Runs `f` with every camera drawing into `rt`; the screen's camera is back after. Used by the
/// terrain cache too (`terrain`).
pub fn fill(rt: &RenderTarget, f: impl FnOnce()) {
    fill_space(rt, Rect::new(0.0, 0.0, screen_width(), screen_height()), f);
}

/// [`fill`] for a picture of its own 2D space `space` (world pixels of the bent view's ground,
/// in `rt` at whatever size it has): the 2D cameras map `space` onto the whole target.
pub fn fill_space(rt: &RenderTarget, space: Rect, f: impl FnOnce()) {
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            FILLING.with(|c| c.borrow_mut().take());
            // The base target, if one is set (`with_base`), else the screen.
            back_to_2d();
        }
    }
    FILLING.with(|c| *c.borrow_mut() = Some((rt.clone(), space)));
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
        blit_screen(&rt.texture);
    }
}
