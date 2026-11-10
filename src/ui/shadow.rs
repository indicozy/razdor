use macroquad::prelude::*;
use razdor::dt::cards::Card;
use razdor::dt::gfx::Image;

/// The colour a shadow takes at full strength: the ground is mixed toward this, a cool dark (not
/// black), so a shadow reads as shade on the grass. The shadow texture is white in its colour channels
/// (`DtArt::shadow_texture`), so the picture's colour is this colour and its alpha the strength times
/// the silhouette: the default pipeline's source-over blend darkens the ground toward `TINT`.
pub const TINT: [f32; 3] = [0.35, 0.38, 0.48];

/// The vertex colour of a shadow quad of `strength` (0 to 1, the light's alpha times the caster's
/// fog and water factors).
pub fn strength_colour(strength: f32) -> Color {
    Color::new(TINT[0], TINT[1], TINT[2], strength)
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
    /// the picture), its atlas rectangle `src` (`atlas` is the atlas's size, the uvs follow `src`).
    /// `None` when the foot is at the picture's top.
    pub fn picture_mesh(&self, src: Rect, atlas: Vec2, color: Color) -> Option<(Vec<Vertex>, Vec<u16>)> {
        let bottom = self.foot_row.clamp(0.0, src.h);
        if bottom <= 0.0 {
            return None;
        }
        let corners = [vec2(0.0, 0.0), vec2(src.w, 0.0), vec2(src.w, bottom), vec2(0.0, bottom)];
        let vertices = corners.map(|px| self.vertex(px, src, atlas, color)).to_vec();
        Some((vertices, vec![0, 1, 2, 0, 2, 3]))
    }

    /// The shadow of one of a building's cards (`Card`, its outline's triangles in sprite px): each vertex
    /// cast from the card's own foot (`card.foot`, in place of `foot_row`), so a village's houses each cast
    /// their own. The rows below the card's foot cast nothing (clamped per vertex).
    pub fn card_mesh(&self, card: &Card, src: Rect, atlas: Vec2, color: Color) -> (Vec<Vertex>, Vec<u16>) {
        let by_card = Projection { foot_row: card.foot.1, ..*self };
        let vertices = card.verts.iter().map(|&(x, y)| by_card.vertex(vec2(x, y), src, atlas, color)).collect();
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
        let (v, i) = on(8.0).picture_mesh(src, atlas, WHITE).expect("a foot inside the picture");
        assert_eq!(v.len(), 4);
        assert_eq!(i, vec![0, 1, 2, 0, 2, 3]);
        assert!((v[2].uv - vec2(26.0, 28.0) / atlas).length() < 1e-5, "the bottom-right uv at the foot row: {:?}", v[2].uv);
        assert!(on(0.0).picture_mesh(src, atlas, WHITE).is_none());
        // A foot below the picture (a hill's centre) covers the whole picture.
        let (tall, _) = on(40.0).picture_mesh(src, atlas, WHITE).expect("covers the picture");
        assert!((tall[2].uv - vec2(26.0, 32.0) / atlas).length() < 1e-5);
    }
}
