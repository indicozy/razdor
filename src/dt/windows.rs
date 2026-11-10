//! Lit windows: the emissive layer of a building picture, computed once from the picture itself.
//!
//! An opening (a window's pane, a door) is a small dark rectangle inside the opaque body of a
//! building, enclosed by lighter stone. [`emissive`] marks those components and nothing else: a
//! component that touches transparency (an outline, a crenellation gap) is not enclosed, and one
//! that is long or sparse (a seam, a line of roof shading) is not compact, so both are rejected.
//! Pure: no macroquad. The map atlas's building mask stores the result (`ui::dt_art`).

use super::gfx::Image;
use std::collections::HashMap;
use std::sync::OnceLock;

/// The alpha a pixel needs to be part of a building's body (0.8 of 255).
const OPAQUE: u8 = 204;
/// A candidate is darker than the picture's median luminance by this much.
const DARK_BELOW_MEDIAN: f32 = 0.10;
/// The ring around a component is brighter than the component's mean by this much.
const RING_BRIGHTER: f32 = 0.08;
/// Window components: 3 to 80 px; doors (taller than wide): up to 200 px.
const WINDOW_AREA: (usize, usize) = (3, 80);
const DOOR_AREA_MAX: usize = 200;
/// Bounding box aspect (width over height) and fill (area over box area).
const ASPECT: (f32, f32) = (0.25, 4.0);
const FILL_MIN: f32 = 0.45;
/// The share of the ring that must be brighter, for a component not in a boxed window.
const RING_BRIGHT_MIN: f32 = 0.7;
/// A boxed window (from `data/lights/objects.json`): at least 2 px, and half its ring enclosed.
const BOX_AREA_MIN: usize = 2;
const BOX_ENCLOSURE_MIN: f32 = 0.5;
/// An opening: its pixels (flat indices) and its outer ring.
type Kept = (Vec<usize>, Vec<(i32, i32)>);

/// A non-boxed component's ring is brighter than it on average by at least this much (contrast).
const MIN_CONTRAST: f32 = 0.12;
/// Roofs: the upper share of the picture's opaque height, and how close a hue must be to the roof's to be
/// one (degrees), with at least this saturation.
const ROOF_ZONE: f32 = 0.45;
const ROOF_HUE_TOL: f32 = 25.0;
const ROOF_SAT_MIN: f32 = 0.15;
/// A roof speck is brighter than this (a window of the upper wall is darker).
const ROOF_MEAN_MIN: f32 = 0.18;
/// The halo: the value of the ring around a kept opening (0.35 of 255), and the opening's (1.0).
pub const HALO: u8 = 90;
pub const LIT: u8 = 255;
/// The feather's gain: the blur halves an opening's edge, so the cores are lifted to keep their strength.
const FEATHER_GAIN: f32 = 1.25;
/// The blur's passes.
const FEATHER_PASSES: usize = 1;

/// A rectangle in a sprite's pixels, half open (`x0..x1`, `y0..y1`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect4 {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}

impl Rect4 {
    fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

/// The emissive layer of a building picture, one byte a pixel (row by row, as the picture): [`LIT`] on
/// each kept opening, [`HALO`] on the ring around it, 0 elsewhere. `boxes` are the picture's window and
/// door boxes (`boxes_of`): an opening inside one is kept with the looser rules.
pub fn emissive(img: &Image, boxes: &[Rect4]) -> Vec<u8> {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut out = vec![0u8; w * h];
    if w == 0 || h == 0 || img.rgba.len() < w * h * 4 {
        return out;
    }
    let opaque: Vec<bool> = (0..w * h).map(|i| img.rgba[i * 4 + 3] >= OPAQUE).collect();
    let lum: Vec<f32> = (0..w * h)
        .map(|i| {
            let p = &img.rgba[i * 4..i * 4 + 4];
            (0.299 * f32::from(p[0]) + 0.587 * f32::from(p[1]) + 0.114 * f32::from(p[2])) / 255.0
        })
        .collect();
    let mut body: Vec<f32> = (0..w * h).filter(|&i| opaque[i]).map(|i| lum[i]).collect();
    if body.is_empty() {
        return out;
    }
    body.sort_by(|a, b| a.total_cmp(b));
    let median = body[body.len() / 2];
    // The roof's zone: the top of the body, and the roof's dominant hue within it.
    let (ymin, ymax) = (0..w * h).filter(|&i| opaque[i]).fold((usize::MAX, 0usize), |(lo, hi), i| (lo.min(i / w), hi.max(i / w)));
    let upper = ymin + ((ymax - ymin + 1) as f32 * ROOF_ZONE) as usize;
    let roof = roof_hue(&img.rgba, w, &opaque, upper);
    let ctx = Ctx { w, h, opaque: &opaque, lum: &lum, rgba: &img.rgba, roof, upper };
    let cand: Vec<bool> = (0..w * h).map(|i| opaque[i] && lum[i] < median - DARK_BELOW_MEDIAN).collect();

    let mut seen = vec![false; w * h];
    let mut kept: Vec<Kept> = Vec::new();
    for start in 0..w * h {
        if !cand[start] || seen[start] {
            continue;
        }
        // The 4-connected component of candidates from `start`.
        let mut comp = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < comp.len() {
            let i = comp[k];
            k += 1;
            let (x, y) = (i % w, i / w);
            let mut push = |nx: usize, ny: usize| {
                let j = ny * w + nx;
                if cand[j] && !seen[j] {
                    seen[j] = true;
                    comp.push(j);
                }
            };
            if x > 0 {
                push(x - 1, y);
            }
            if x + 1 < w {
                push(x + 1, y);
            }
            if y > 0 {
                push(x, y - 1);
            }
            if y + 1 < h {
                push(x, y + 1);
            }
        }
        if let Some(ring) = judge(&comp, &ctx, boxes) {
            kept.push((comp, ring));
        }
    }
    // The halo first, so an opening's own pixels overwrite it.
    for (_, ring) in &kept {
        for &(x, y) in ring {
            if x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h {
                let j = y as usize * w + x as usize;
                if opaque[j] && out[j] == 0 {
                    out[j] = HALO;
                }
            }
        }
    }
    for (comp, _) in &kept {
        for &j in comp {
            out[j] = LIT;
        }
    }
    feather(&mut out, w, h, &opaque);
    out
}

/// Softens the layer's edges: one pass of a 3-tap blur ([1 2 1] / 4, across then down), so an opening's
/// glow falls off over about 1 px outside it and keeps its shape (two passes made the panes into blobs). Clipped to the picture's body (the
/// transparent pixels stay clear), and lifted by [`FEATHER_GAIN`] so a pane keeps its strength.
fn feather(out: &mut [u8], w: usize, h: usize, opaque: &[bool]) {
    let mut f: Vec<f32> = out.iter().map(|&v| f32::from(v)).collect();
    let mut tmp = vec![0f32; f.len()];
    for _ in 0..FEATHER_PASSES {
        for y in 0..h {
            for x in 0..w {
                let at = |xx: isize| if xx < 0 || xx >= w as isize { 0.0 } else { f[y * w + xx as usize] };
                let x = x as isize;
                tmp[y * w + x as usize] = (at(x - 1) + 2.0 * at(x) + at(x + 1)) / 4.0;
            }
        }
        for y in 0..h {
            for x in 0..w {
                let at = |yy: isize| if yy < 0 || yy >= h as isize { 0.0 } else { tmp[yy as usize * w + x] };
                let y = y as isize;
                f[y as usize * w + x] = (at(y - 1) + 2.0 * at(y) + at(y + 1)) / 4.0;
            }
        }
    }
    for (i, v) in out.iter_mut().enumerate() {
        *v = if opaque[i] { (f[i] * FEATHER_GAIN).round().min(255.0) as u8 } else { 0 };
    }
}

/// Whether a candidate component is an opening, and if so its ring (its outer 1-px ring, which may
/// hold out-of-picture pixels). The rules: the area and bounding box, then the ring. A window's ring is
/// all opaque and at least [`RING_BRIGHT_MIN`] brighter than the component; a door (taller than wide)
/// may have ground under its bottom row. A boxed window needs only [`BOX_ENCLOSURE_MIN`] of its ring.
fn judge(comp: &[usize], ctx: &Ctx, boxes: &[Rect4]) -> Option<Vec<(i32, i32)>> {
    let Ctx { w, h, opaque, lum, .. } = *ctx;
    let (mut minx, mut maxx, mut miny, mut maxy) = (usize::MAX, 0usize, usize::MAX, 0usize);
    for &i in comp {
        let (x, y) = (i % w, i / w);
        minx = minx.min(x);
        maxx = maxx.max(x);
        miny = miny.min(y);
        maxy = maxy.max(y);
    }
    let (bw, bh) = (maxx - minx + 1, maxy - miny + 1);
    let area = comp.len();
    let fill = area as f32 / (bw * bh) as f32;
    let aspect = bw as f32 / bh as f32;
    if aspect < ASPECT.0 || aspect > ASPECT.1 || fill < FILL_MIN {
        return None;
    }
    let door = bh > bw;
    let centre = ((minx + maxx) as i32 / 2, (miny + maxy) as i32 / 2);
    let boxed = boxes.iter().any(|b| b.contains(centre.0, centre.1));
    if boxed {
        if !(BOX_AREA_MIN..=DOOR_AREA_MAX).contains(&area) {
            return None;
        }
    } else {
        let max = if door { DOOR_AREA_MAX } else { WINDOW_AREA.1 };
        if area < WINDOW_AREA.0 || area > max {
            return None;
        }
    }
    let mean = comp.iter().map(|&i| lum[i]).sum::<f32>() / area as f32;
    // A roof: the component's colour is the roof's, and it lies in the roof's zone (the top of the picture).
    // Roof shading is mid-dark; a window is darker than the roof tiles and so is never a roof speck.
    if !boxed && miny < ctx.upper && mean >= ROOF_MEAN_MIN {
        if let Some(roof) = ctx.roof {
            let (r, g, b) = comp.iter().fold((0.0f32, 0.0f32, 0.0f32), |(r, g, b), &i| {
                let p = &ctx.rgba[i * 4..i * 4 + 3];
                (r + f32::from(p[0]), g + f32::from(p[1]), b + f32::from(p[2]))
            });
            let (hue, sat, _) = hsv(r / (255.0 * area as f32), g / (255.0 * area as f32), b / (255.0 * area as f32));
            if sat >= ROOF_SAT_MIN && hue_dist(hue, roof) <= ROOF_HUE_TOL {
                return None;
            }
        }
    }

    // The outer ring: the pixels 4-adjacent to the component and not part of it, once each.
    let member = |x: i32, y: i32| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h && cand_in(comp, w, x as usize, y as usize);
    let mut ring: Vec<(i32, i32)> = Vec::new();
    for &i in comp {
        let (x, y) = ((i % w) as i32, (i / w) as i32);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if !member(nx, ny) {
                ring.push((nx, ny));
            }
        }
    }
    ring.sort_unstable();
    ring.dedup();

    // A ring pixel is bright when it is picture (opaque) and brighter than the component by RING_BRIGHTER.
    let mut transparent = 0usize;
    let mut bright = 0usize;
    let mut counted = 0usize;
    for &(x, y) in &ring {
        let inside = x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h;
        let is_opaque = inside && opaque[y as usize * w + x as usize];
        // A door's bottom ring row may be ground or the picture's edge.
        let ground_ok = door && y == maxy as i32 + 1;
        if !is_opaque {
            if !ground_ok {
                transparent += 1;
                counted += 1;
            }
            continue;
        }
        counted += 1;
        if lum[y as usize * w + x as usize] >= mean + RING_BRIGHTER {
            bright += 1;
        }
    }
    let ring_len = ring.len().max(1);
    if boxed {
        if (bright as f32) / (ring_len as f32) < BOX_ENCLOSURE_MIN {
            return None;
        }
    } else if transparent > 0 || counted == 0 || (bright as f32) / (counted as f32) < RING_BRIGHT_MIN {
        return None;
    } else {
        // The ring is on average brighter than the component by the contrast floor.
        let ring_lum: f32 = ring
            .iter()
            .filter(|&&(x, y)| x >= 0 && y >= 0 && (x as usize) < w && (y as usize) < h)
            .map(|&(x, y)| lum[y as usize * w + x as usize])
            .sum::<f32>()
            / ring.len().max(1) as f32;
        if ring_lum - mean < MIN_CONTRAST {
            return None;
        }
    }
    Some(ring)
}

/// The picture's data for the judging of its components.
struct Ctx<'a> {
    w: usize,
    h: usize,
    opaque: &'a [bool],
    lum: &'a [f32],
    rgba: &'a [u8],
    /// The roof's dominant hue (degrees), if the top of the picture has enough saturated colour.
    roof: Option<f32>,
    /// The first row below the roof zone.
    upper: usize,
}

/// The roof's hue: the most common 30° hue bucket (its centre) among the saturated opaque pixels above
/// `upper`. None with too few such pixels.
fn roof_hue(rgba: &[u8], w: usize, opaque: &[bool], upper: usize) -> Option<f32> {
    let mut hist = [0u32; 12];
    let mut n = 0u32;
    for (i, &op) in opaque.iter().enumerate() {
        if !op || i / w >= upper {
            continue;
        }
        let p = &rgba[i * 4..i * 4 + 3];
        let (hue, sat, _) = hsv(f32::from(p[0]) / 255.0, f32::from(p[1]) / 255.0, f32::from(p[2]) / 255.0);
        if sat >= ROOF_SAT_MIN {
            hist[((hue / 30.0) as usize).min(11)] += 1;
            n += 1;
        }
    }
    if n < 20 {
        return None;
    }
    let (best, _) = hist.iter().enumerate().max_by_key(|(_, c)| **c)?;
    Some(best as f32 * 30.0 + 15.0)
}

/// Hue (degrees, 0..360), saturation and value of an RGB colour in 0..1.
fn hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let sat = if mx > 0.0 { d / mx } else { 0.0 };
    let hue = if d <= 0.0 {
        0.0
    } else if mx == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (hue, sat, mx)
}

/// The distance between two hues on the circle (degrees, 0..180).
fn hue_dist(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(360.0);
    d.min(360.0 - d)
}

/// Whether the component holds the pixel (`comp` is a list of flat indices).
fn cand_in(comp: &[usize], w: usize, x: usize, y: usize) -> bool {
    comp.contains(&(y * w + x))
}

/// The bundled window and door boxes (`data/lights/objects.json`, the detector's output for the
/// install's building sprites), by the sprite's pixel key (`cards::sprite_key`). Parsed once.
pub fn boxes_of(key: u64) -> &'static [Rect4] {
    static BOXES: OnceLock<HashMap<u64, Vec<Rect4>>> = OnceLock::new();
    BOXES.get_or_init(|| parse_boxes(include_str!("../../data/lights/objects.json")))
        .get(&key)
        .map_or(&[], Vec::as_slice)
}

/// The boxes of a sprite by key, from the detector's JSON: `sprites` maps a 16-hex-digit key to its
/// `points`, and the `window` and `door` points carry a box `[x0, y0, x1, y1]`.
fn parse_boxes(text: &str) -> HashMap<u64, Vec<Rect4>> {
    let mut out = HashMap::new();
    let Ok(root) = serde_json::from_str::<serde_json::Value>(text) else { return out };
    let Some(sprites) = root.get("sprites").and_then(|s| s.as_object()) else { return out };
    for (key, entry) in sprites {
        let Ok(key) = u64::from_str_radix(key, 16) else { continue };
        let Some(points) = entry.get("points").and_then(|p| p.as_array()) else { continue };
        let boxes: Vec<Rect4> = points
            .iter()
            .filter(|p| matches!(p.get("kind").and_then(|k| k.as_str()), Some("window" | "door")))
            .filter_map(|p| {
                let b: Vec<i64> = p.get("box")?.as_array()?.iter().filter_map(|v| v.as_i64()).collect();
                (b.len() == 4).then(|| Rect4 { x0: b[0] as i32, y0: b[1] as i32, x1: b[2] as i32, y1: b[3] as i32 })
            })
            .collect();
        if !boxes.is_empty() {
            out.insert(key, boxes);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `w` × `h` picture of an opaque wall of luma `wall`, with `hole(x, y)` pixels set to luma `dark`
    /// and `open(x, y)` pixels made transparent.
    fn wall(w: u32, h: u32, wall: u8, dark: u8, hole: impl Fn(u32, u32) -> bool, open: impl Fn(u32, u32) -> bool) -> Image {
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h {
            for x in 0..w {
                let i = ((y * w + x) * 4) as usize;
                let v = if hole(x, y) { dark } else { wall };
                rgba[i..i + 3].copy_from_slice(&[v, v, v]);
                rgba[i + 3] = if open(x, y) { 0 } else { 255 };
            }
        }
        Image { width: w, height: h, rgba }
    }

    fn lit(out: &[u8], w: u32, x: u32, y: u32) -> u8 {
        out[(y * w + x) as usize]
    }

    #[test]
    fn a_wall_with_three_windows_keeps_three_openings() {
        // A 24×20 wall; three 3×3 panes, each enclosed by the wall.
        let panes = |x: u32, y: u32| (3..6).contains(&x) && (4..7).contains(&y) || (10..13).contains(&x) && (4..7).contains(&y) || (17..20).contains(&x) && (4..7).contains(&y);
        let img = wall(24, 20, 150, 40, panes, |_, _| false);
        let out = emissive(&img, &[]);
        let glowing = out.iter().filter(|&&v| v >= 200).count();
        assert!(glowing >= 3, "three panes glow at their cores: {glowing}");
        assert!(lit(&out, 24, 4, 5) >= 200);
        assert!(lit(&out, 24, 11, 6) >= 200);
        let halo = lit(&out, 24, 2, 5);
        assert!((1..200).contains(&halo), "the wall beside a pane is feathered, not lit: {halo}");
        assert!(lit(&out, 24, 1, 5) < halo, "the glow falls off outward");
        assert_eq!(lit(&out, 24, 0, 0), 0, "stone far away is nothing");
    }

    #[test]
    fn an_outline_along_the_edge_keeps_nothing() {
        // A dark 1-px line on row 1, under a transparent row 0: the line touches transparency and is
        // 21 px long and 1 px tall (far from compact).
        let line = |x: u32, y: u32| y == 1 && (1..23).contains(&x);
        let img = wall(24, 20, 150, 40, line, |_, y| y == 0);
        let out = emissive(&img, &[]);
        assert!(out.iter().all(|&v| v != LIT), "an edge line is not an opening");
    }

    #[test]
    fn a_crenellation_gap_touching_transparency_keeps_nothing() {
        // A dark 2×2 notch at the top of the wall, open onto transparency above it: compact enough, but
        // not enclosed.
        let notch = |x: u32, y: u32| (10..12).contains(&x) && (1..3).contains(&y);
        let img = wall(24, 20, 150, 40, notch, |x, y| y == 0 && (10..12).contains(&x));
        let out = emissive(&img, &[]);
        assert!(out.iter().all(|&v| v != LIT), "a notch on the edge is not an opening");
    }

    #[test]
    fn a_tall_door_is_kept_when_its_bottom_touches_the_ground() {
        // A 4×8 door (taller than wide) whose bottom ring row is the ground: transparent below it.
        let door = |x: u32, y: u32| (8..12).contains(&x) && (4..12).contains(&y);
        let ground = |x: u32, y: u32| y == 12 && (8..12).contains(&x);
        let img = wall(24, 20, 150, 40, door, ground);
        let out = emissive(&img, &[]);
        assert!(lit(&out, 24, 9, 6) >= 200, "the door is kept");
    }

    #[test]
    fn a_boxed_window_is_kept_with_the_looser_rules() {
        // A 2×2 opening next to the picture's edge: area 4, its ring is half transparent.
        let spot = |x: u32, y: u32| (1..3).contains(&x) && (5..7).contains(&y);
        let img = wall(24, 20, 150, 40, spot, |x, _| x == 0);
        let boxes = [Rect4 { x0: 0, y0: 3, x1: 6, y1: 9 }];
        let plain = emissive(&img, &[]);
        assert!(plain.iter().all(|&v| v != LIT), "unboxed: enclosed rule rejects it");
        let boxed = emissive(&img, &boxes);
        assert!(lit(&boxed, 24, 1, 5) > 0, "boxed: the looser rule keeps it (feathered, so not at full strength)");
    }

    /// A 24×20 picture: a red roof over the top 14 rows (most of the body, so the median is the roof's) with
    /// `dark` pixels in it, and grey wall below with its dark windows.
    fn roofed(dark: impl Fn(u32, u32) -> bool) -> Image {
        let mut rgba = vec![0u8; 24 * 20 * 4];
        for y in 0..20u32 {
            for x in 0..24u32 {
                let i = ((y * 24 + x) * 4) as usize;
                let roof = y < 14;
                let c = if dark(x, y) {
                    if roof { [110, 50, 40] } else { [40, 40, 40] }
                } else if roof {
                    [200, 60, 40]
                } else {
                    [150, 150, 150]
                };
                rgba[i..i + 3].copy_from_slice(&c);
                rgba[i + 3] = 255;
            }
        }
        Image { width: 24, height: 20, rgba }
    }

    #[test]
    fn a_dark_speck_on_the_roof_is_not_a_window_unless_a_box_marks_it() {
        // A roof speck (a shadow on the tiles, roof-coloured) at the top; a real window on the wall below.
        let img = roofed(|x, y| (5..8).contains(&x) && (3..6).contains(&y) || (5..8).contains(&x) && (15..18).contains(&y));
        let out = emissive(&img, &[]);
        assert_eq!(lit(&out, 24, 6, 4), 0, "the roof speck is rejected");
        assert!(lit(&out, 24, 6, 16) >= 200, "the wall window is kept");
        let boxed = emissive(&img, &[Rect4 { x0: 4, y0: 2, x1: 9, y1: 7 }]);
        assert!(lit(&boxed, 24, 6, 4) >= 200, "a detector box on the roof speck keeps it");
    }

    #[test]
    fn a_speck_with_too_little_contrast_is_not_a_window() {
        // Wall 120, a speck of 92: the ring is only 0.11 brighter than the speck, under the 0.12 floor.
        let mut rgba = vec![0u8; 24 * 20 * 4];
        for y in 0..20u32 {
            for x in 0..24u32 {
                let i = ((y * 24 + x) * 4) as usize;
                let v = if (10..13).contains(&x) && (8..11).contains(&y) { 92 } else { 120 };
                rgba[i..i + 3].copy_from_slice(&[v, v, v]);
                rgba[i + 3] = 255;
            }
        }
        let img = Image { width: 24, height: 20, rgba };
        assert!(emissive(&img, &[]).iter().all(|&v| v == 0), "a faint speck is no window");
    }

    #[test]
    fn a_picture_with_no_body_gives_nothing() {
        let img = wall(8, 8, 0, 0, |_, _| false, |_, _| true);
        assert!(emissive(&img, &[]).iter().all(|&v| v == 0));
    }

    #[test]
    fn the_bundled_boxes_parse_and_look_up_by_key() {
        let json = r#"{"sprites":{"00000000000000ff":{"w":10,"h":10,"points":[{"kind":"window","box":[1,2,3,4]},{"kind":"chimney","box":[0,0,1,1]}]}}}"#;
        let map = parse_boxes(json);
        assert_eq!(map.get(&0xff), Some(&vec![Rect4 { x0: 1, y0: 2, x1: 3, y1: 4 }]), "only windows and doors");
        assert!(boxes_of(0xdead_beef).is_empty());
    }
}
