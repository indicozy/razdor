use macroquad::prelude::*;
use razdor::dt::cards::Card;
use razdor::dt::gfx::Image;

/// The colour a shadow takes at full strength: the ground is mixed toward this, a cool dark (not
/// black), so a shadow reads as shade on the grass. The shadow texture is white in its colour channels
/// (`DtArt::shadow_texture`), so the picture's colour is this colour and its alpha the strength times
/// the silhouette: the default pipeline's source-over blend darkens the ground toward `TINT`.
/// The tone a shadow multiplies what it falls on toward (at full strength): a dark, slightly cool shade
/// (about 45 % darker, with the blue a little higher), so grass in shadow stays green.
pub const TINT: [f32; 3] = [0.48, 0.50, 0.62];

/// The shadows' alpha is scaled by this (1: as drawn; the sun's shadow is 0.6 by day, `daylight::SUN_ALPHA`).
pub const EASE: f32 = 1.0;

/// The vertex colour of a shadow quad of `strength` (0 to 1, the light's alpha times the caster's
/// fog and water factors).
pub fn strength_colour(strength: f32) -> Color {
    Color::new(TINT[0], TINT[1], TINT[2], strength * EASE)
}

/// The multiply pass of the shadows: each pixel of a shadow quad is multiplied by mix(1, tone, a), where a is
/// the strength (the vertex alpha, [`strength_colour`]) times the silhouette (the texture's alpha). Drawn with the
/// multiply blend (dst × src, no alpha-over), so a shadow darkens grass and keeps its colour.
const MULTIPLY_MAIN: &str = r#"
    vec4 s = texture2D(Texture, uv);
    float a = clamp(color.a * s.a, 0.0, 1.0);
    gl_FragColor = vec4(mix(vec3(1.0), color.rgb, a), 1.0);
"#;

razdor::local! {
    static MULTIPLY_MATERIAL: std::cell::OnceCell<Option<macroquad::material::Material>> = const { std::cell::OnceCell::new() };
}

/// Draws a shadow quad (or card mesh) by the multiply pass ([`MULTIPLY_MAIN`]). Outside a session, with the
/// default material otherwise. Without the material (it did not compile), the mesh is drawn as before.
pub fn draw_multiply(mesh: &macroquad::models::Mesh) {
    use macroquad::miniquad::{BlendFactor, BlendState, BlendValue, Equation, PipelineParams};
    let material = MULTIPLY_MATERIAL.with(|m| {
        m.get_or_init(|| {
            let pipeline = PipelineParams {
                color_blend: Some(BlendState::new(Equation::Add, BlendFactor::Zero, BlendFactor::Value(BlendValue::SourceColor))),
                ..Default::default()
            };
            super::sharp::material_in(super::sharp::SHARP_VERTEX, &super::sharp::fragment("", MULTIPLY_MAIN), pipeline, Vec::new(), Vec::new())
                .map_err(|e| razdor::diag!("shadow multiply material: {e}"))
                .ok()
        })
        .clone()
    });
    match material {
        Some(m) => {
            macroquad::prelude::gl_use_material(&m);
            macroquad::prelude::draw_mesh(mesh);
            macroquad::prelude::gl_use_default_material();
        }
        None => macroquad::prelude::draw_mesh(mesh),
    }
}

/// Blurs the alpha channel of an RGBA picture in place, `passes` times, by a separable [1 2 1] / 4 kernel
/// (each pass about one pixel of softening). The colour channels are left alone (the shadow atlas is white).
pub fn blur_alpha(rgba: &mut [u8], w: usize, h: usize, passes: usize) {
    if w == 0 || h == 0 || rgba.len() < w * h * 4 {
        return;
    }
    let mut a: Vec<f32> = rgba.chunks_exact(4).map(|p| f32::from(p[3])).collect();
    let mut tmp = vec![0f32; a.len()];
    for _ in 0..passes {
        for y in 0..h {
            for x in 0..w {
                let l = if x > 0 { a[y * w + x - 1] } else { 0.0 };
                let r = if x + 1 < w { a[y * w + x + 1] } else { 0.0 };
                tmp[y * w + x] = (l + 2.0 * a[y * w + x] + r) * 0.25;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let u = if y > 0 { tmp[(y - 1) * w + x] } else { 0.0 };
                let d = if y + 1 < h { tmp[(y + 1) * w + x] } else { 0.0 };
                a[y * w + x] = (u + 2.0 * tmp[y * w + x] + d) * 0.25;
            }
        }
    }
    for (p, v) in rgba.chunks_exact_mut(4).zip(a) {
        p[3] = v.round().clamp(0.0, 255.0) as u8;
    }
}

/// The alpha a picture's pixel needs to cast (0.8 of 255): the opaque body, not its soft edges.
const CAST_MIN_ALPHA: u8 = 204;

/// The silhouette of a picture for its shadow: its pixels of alpha at least 0.8 (`CAST_MIN_ALPHA`),
/// blurred by about 2 px (a separable [1 2 3 2 1] / 9 kernel, zero outside the picture). One byte a
/// pixel, row by row, as the picture's own pixels.
pub fn silhouette(img: &Image) -> Vec<u8> {
    const KERNEL: [f32; 5] = [1.0 / 9.0, 2.0 / 9.0, 3.0 / 9.0, 2.0 / 9.0, 1.0 / 9.0];
    let (w, h) = (img.width as usize, img.height as usize);
    let src: Vec<f32> = img.rgba.chunks_exact(4).map(|p| if p[3] >= CAST_MIN_ALPHA { 1.0 } else { 0.0 }).collect();
    let mut across = vec![0.0f32; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for (i, k) in KERNEL.iter().enumerate() {
                let xx = x as isize + i as isize - 2;
                if (0..w as isize).contains(&xx) {
                    sum += k * src[y * w + xx as usize];
                }
            }
            across[y * w + x] = sum;
        }
    }
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut sum = 0.0;
            for (i, k) in KERNEL.iter().enumerate() {
                let yy = y as isize + i as isize - 2;
                if (0..h as isize).contains(&yy) {
                    sum += k * across[yy as usize * w + x];
                }
            }
            out[y * w + x] = (sum * 255.0).round().clamp(0.0, 255.0) as u8;
        }
    }
    out
}

/// How a picture's shadow falls on the ground for one light: the 2D point of the picture's top-left
/// (`origin`), its scale (`zoom`, 2D px per sprite px), the row of its foot (`foot_row`, sprite px), and
/// the light's shadow (`dir`, a unit vector on the map; `len` per unit of height).
#[derive(Clone, Copy, Debug)]
pub struct Projection {
    pub origin: Vec2,
    pub zoom: f32,
    pub foot_row: f32,
    pub dir: Vec2,
    pub len: f32,
}

impl Projection {
    /// Where a picture's pixel `px` (sprite px, origin top-left, y down) lands on the ground's 2D space:
    /// it keeps its x, and it is pushed along `dir` by its height above the foot (`foot_row - y`, never
    /// below 0) times `len`. A point on the foot's row casts nothing.
    pub fn point(&self, px: Vec2) -> Vec2 {
        let height = (self.foot_row - px.y).max(0.0);
        self.origin + vec2(px.x, self.foot_row) * self.zoom + self.dir * (height * self.len * self.zoom)
    }

    /// The shadow of a whole picture: the quad of its rows down to the foot (`0..=foot_row`, clamped to
    /// the picture), its atlas rectangle `src` (`atlas` is the atlas's size, the uvs follow `src`), each
    /// vertex's colour from `colour_at(t)`, where `t` is its height above the foot as a share of the
    /// picture's (0 at the foot, 1 at the top): the penumbra's ramp, sharp near the base, soft at the tip.
    /// `None` when the foot is at the picture's top.
    pub fn picture_mesh_ramp(&self, src: Rect, atlas: Vec2, colour_at: impl Fn(f32) -> Color) -> Option<(Vec<Vertex>, Vec<u16>)> {
        let bottom = self.foot_row.clamp(0.0, src.h);
        if bottom <= 0.0 {
            return None;
        }
        let corners = [vec2(0.0, 0.0), vec2(src.w, 0.0), vec2(src.w, bottom), vec2(0.0, bottom)];
        let vertices = corners.map(|px| self.vertex(px, src, atlas, colour_at(1.0 - px.y / bottom))).to_vec();
        Some((vertices, vec![0, 1, 2, 0, 2, 3]))
    }

    /// The shadow of one of a building's cards (`Card`, its outline's triangles in sprite px): each vertex
    /// cast from the card's own foot (`card.foot`, in place of `foot_row`), with the penumbra's ramp (`t` is 0
    /// at the card's own foot and 1 at its top). The rows below the card's foot cast nothing (clamped per vertex).
    pub fn card_mesh_ramp(&self, card: &Card, src: Rect, atlas: Vec2, colour_at: impl Fn(f32) -> Color) -> (Vec<Vertex>, Vec<u16>) {
        let by_card = Projection { foot_row: card.foot.1, ..*self };
        let foot = card.foot.1.max(1e-3);
        let vertices = card
            .verts
            .iter()
            .map(|&(x, y)| by_card.vertex(vec2(x, y), src, atlas, colour_at((1.0 - y / foot).clamp(0.0, 1.0))))
            .collect();
        (vertices, card.tris.clone())
    }

    fn vertex(&self, px: Vec2, src: Rect, atlas: Vec2, color: Color) -> Vertex {
        let p = self.point(px);
        let uv = vec2(src.x + px.x, src.y + px.y) / atlas;
        Vertex::new(p.x, p.y, 0.0, uv.x, uv.y, color)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32, opaque: impl Fn(u32, u32) -> bool) -> Image {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                if opaque(x, y) {
                    rgba[((y * w + x) * 4 + 3) as usize] = 255;
                }
            }
        }
        Image { width: w, height: h, rgba }
    }

    #[test]
    fn a_silhouette_is_the_opaque_body_blurred_by_about_two_pixels() {
        let img = image(20, 20, |x, y| (5..15).contains(&x) && (5..15).contains(&y));
        let m = silhouette(&img);
        let at = |x: usize, y: usize| m[y * 20 + x];
        assert!(at(10, 10) > 250, "the body's middle {}", at(10, 10));
        assert!(at(4, 10) > 0 && at(4, 10) < 255, "one pixel outside the edge is in the blur: {}", at(4, 10));
        assert_eq!(at(0, 10), 0, "far outside stays clear");
    }

    #[test]
    fn a_faint_pixel_casts_nothing() {
        let mut img = image(8, 8, |_, _| false);
        img.rgba[((4 * 8 + 4) * 4 + 3) as usize] = 150; // alpha 0.59, below 0.8
        assert!(silhouette(&img).iter().all(|&v| v == 0));
    }

    #[test]
    fn a_point_on_the_foot_row_stays_on_it_and_one_above_moves_along_the_shadow() {
        let p = Projection { origin: vec2(100.0, 50.0), zoom: 2.0, foot_row: 10.0, dir: vec2(-1.0, 0.0), len: 1.5 };
        // The foot at row 10 of the picture (zoom 2): the point on the row is the foot's own x.
        let on_foot = p.point(vec2(4.0, 10.0));
        assert!((on_foot - vec2(108.0, 70.0)).length() < 1e-4, "{on_foot:?}");
        // Four sprite px above the foot, the shadow at len 1.5 moves 4 * 1.5 * 2 = 12 px to the west,
        // and the pixel keeps its x (8 px from the origin, as on the foot).
        let up = p.point(vec2(4.0, 6.0));
        assert!((up - vec2(96.0, 70.0)).length() < 1e-4, "{up:?}");
    }

    #[test]
    fn rows_below_the_foot_cast_nothing() {
        let p = Projection { origin: Vec2::ZERO, zoom: 1.0, foot_row: 5.0, dir: vec2(0.0, -1.0), len: 2.0 };
        let below = p.point(vec2(3.0, 9.0));
        assert!((below - vec2(3.0, 5.0)).length() < 1e-4, "{below:?}");
    }

    #[test]
    fn a_picture_quad_stops_at_its_foot_and_is_none_without_one() {
        let src = Rect::new(10.0, 20.0, 16.0, 12.0);
        let atlas = vec2(64.0, 64.0);
        let on = |foot_row: f32| Projection { origin: Vec2::ZERO, zoom: 1.0, foot_row, dir: vec2(-1.0, 0.0), len: 1.0 };
        let (v, i) = on(8.0).picture_mesh_ramp(src, atlas, |_| WHITE).expect("a foot inside the picture");
        assert_eq!(v.len(), 4);
        assert_eq!(i, vec![0, 1, 2, 0, 2, 3]);
        assert!((v[2].uv - vec2(26.0, 28.0) / atlas).length() < 1e-5, "the bottom-right uv at the foot row: {:?}", v[2].uv);
        assert!(on(0.0).picture_mesh_ramp(src, atlas, |_| WHITE).is_none());
        // A foot below the picture (a hill's centre) covers the whole picture.
        let (tall, _) = on(40.0).picture_mesh_ramp(src, atlas, |_| WHITE).expect("covers the picture");
        assert!((tall[2].uv - vec2(26.0, 32.0) / atlas).length() < 1e-5);
    }
}

#[cfg(test)]
mod penumbra_tests {
    use super::*;

    #[test]
    fn a_blur_softens_an_edge_by_about_a_pixel_a_pass_and_keeps_the_body() {
        // A 9×5 picture with a solid 3×3 body in its middle row band (its vertical neighbours are the body too).
        let (w, h) = (9usize, 5usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 1..4 {
            for x in 3..6 {
                rgba[(y * w + x) * 4 + 3] = 255;
            }
        }
        let mut once = rgba.clone();
        blur_alpha(&mut once, w, h, 1);
        let a = |img: &[u8], x: usize, y: usize| img[(y * w + x) * 4 + 3];
        assert!(a(&once, 4, 2) > 200, "the body stays strong: {}", a(&once, 4, 2));
        assert!(a(&once, 2, 2) > 0 && a(&once, 2, 2) < 128, "one pixel out is a soft edge: {}", a(&once, 2, 2));
        assert_eq!(a(&once, 0, 2), 0, "far out stays clear");
        let mut twice = rgba.clone();
        blur_alpha(&mut twice, w, h, 2);
        assert!(a(&twice, 1, 2) > 0, "two passes spread the edge further");
    }

    #[test]
    fn the_penumbra_is_sharp_at_the_foot_and_soft_at_the_tip_and_the_tint_is_warm() {
        let p = Projection { origin: Vec2::ZERO, zoom: 1.0, foot_row: 10.0, dir: vec2(1.0, 0.0), len: 1.0 };
        let src = Rect::new(0.0, 0.0, 4.0, 10.0);
        // The colour's alpha: the sharp layer at weight 1 - t, the soft at t.
        let (v, _) = p.picture_mesh_ramp(src, vec2(20.0, 20.0), |t| Color::new(0.0, 0.0, 0.0, 1.0 - t)).expect("a foot");
        // Vertices 0 and 1 are the top (t = 1), 2 and 3 the foot (t = 0).
        // Vertex colours are bytes: the top's sharp weight is 0, the foot's is full.
        assert!(v[0].color[3] < 3 && v[2].color[3] > 250, "top is soft, foot sharp: {:?} {:?}", v[0].color, v[2].color);
        let warm = strength_colour(1.0);
        // Warmer than the old blue-grey: its red is a larger share of its blue (0.93 against 0.73).
        assert!(warm.r / warm.b > 0.35 / 0.48, "the tint is warmer than the old blue-grey: {warm:?}");
        assert!((warm.a - EASE).abs() < 1e-6, "scaled by EASE");
    }
}
