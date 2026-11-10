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

/// The halo: the value of the ring around a kept opening (0.35 of 255), and the opening's (1.0).
pub const HALO: u8 = 90;
pub const LIT: u8 = 255;

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
        if let Some(ring) = judge(&comp, w, h, &opaque, &lum, boxes) {
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
    out
}

/// Whether a candidate component is an opening, and if so its ring (its outer 1-px ring, which may
/// hold out-of-picture pixels). The rules: the area and bounding box, then the ring. A window's ring is
/// all opaque and at least [`RING_BRIGHT_MIN`] brighter than the component; a door (taller than wide)
/// may have ground under its bottom row. A boxed window needs only [`BOX_ENCLOSURE_MIN`] of its ring.
fn judge(comp: &[usize], w: usize, h: usize, opaque: &[bool], lum: &[f32], boxes: &[Rect4]) -> Option<Vec<(i32, i32)>> {
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
    }
    Some(ring)
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
        let lit_count = out.iter().filter(|&&v| v == LIT).count();
        assert_eq!(lit_count, 27, "three 3×3 panes");
        assert_eq!(lit(&out, 24, 4, 5), LIT);
        assert_eq!(lit(&out, 24, 11, 6), LIT);
        assert_eq!(lit(&out, 24, 2, 5), HALO, "the wall beside a pane is the halo");
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
        assert_eq!(lit(&out, 24, 9, 6), LIT, "the door is kept");
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
        assert_eq!(lit(&boxed, 24, 1, 5), LIT, "boxed: the looser rule keeps it");
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
