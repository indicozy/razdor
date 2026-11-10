//! Razdor's point lights («Освещение», `daylight`; «Качество света», `Quality`). Style only: no rule,
//! path, AI or save reads them.
//!
//! One light per building (at its footprint's centre, radius about the sprite's size) and one per
//! figure in view, a torch in the hand of the hero and of every army (about 0.4 of the figure's height
//! from the head, forward along its facing, 60° above horizontal). Each light has a warm pool on the
//! ground and the sprites (`world_view::draw_daylight`). The nearest few to the view's centre (by the
//! quality) also light the relief of the sprites and the ground, lay their silhouettes on the ground
//! (`world_view::draw_shadows`), and make a building's openings glow (`sharp`'s lit material).
//!
//! Positions are in world units (cells, `Camera::to_screen` divided by the scale) until a frame draws
//! them: the 2D space of the drawing (`Camera::tilted_space`) is the world times the scale, so the
//! shaders take the same points and the radii in the same pixels.

use macroquad::prelude::*;
use razdor::local;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;

/// «Качество света»: how many lights cast shadows, light the relief and make the openings glow.
/// High on the desktop, Medium on a phone (`default_quality`); Low keeps the warm pools only.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Quality {
    #[default]
    High,
    Medium,
    Low,
}

impl Quality {
    /// The next level in the settings row: High, Medium, Low, High.
    pub fn next(self) -> Quality {
        match self {
            Quality::High => Quality::Medium,
            Quality::Medium => Quality::Low,
            Quality::Low => Quality::High,
        }
    }

    /// The lights that cast shadows, light the relief and make the openings glow: the nearest this many.
    pub fn lit(self) -> usize {
        match self {
            Quality::High => SLOTS,
            Quality::Medium => 4,
            Quality::Low => 0,
        }
    }

    /// The nearest lights that light the relief (a subset of `lit`): 2 on Medium.
    pub fn relief(self) -> usize {
        match self {
            Quality::High => SLOTS,
            Quality::Medium => 2,
            Quality::Low => 0,
        }
    }

    /// The name `RAZDOR_LIGHT_QUALITY` takes.
    pub fn parse(s: &str) -> Option<Quality> {
        match s.trim().to_ascii_lowercase().as_str() {
            "high" => Some(Quality::High),
            "medium" => Some(Quality::Medium),
            "low" => Some(Quality::Low),
            _ => None,
        }
    }
}

/// The level a player gets with no choice made: High on the desktop, Medium on a phone.
pub fn default_quality(phone: bool) -> Quality {
    if phone {
        Quality::Medium
    } else {
        Quality::High
    }
}

/// The lights a shader's arrays hold (`sharp`'s and `terrain`'s `LightA`).
pub const SLOTS: usize = 8;

/// The warm colour of a light (its pool, its emission).
pub const WARM: [f32; 3] = [1.0, 0.72, 0.40];

/// A torch's reach in cells, and a building's reach as a fraction of its sprite's size (1: the sprite's own).
pub const TORCH_RADIUS: f32 = 1.6;
pub const BUILDING_RADIUS: f32 = 1.0;

/// A torch's position on a figure of `size` pixels (its sprite square) for its `heading` (a unit vector on
/// the screen, zero when it stands still or has no heading: the torch then stands straight up). The
/// head is 0.6 of the square above the feet; the torch is 0.4 of the figure's height from the head, along
/// the heading and 60° above horizontal. Returns the offset from the feet in pixels (y down).
pub fn torch_offset(heading: Vec2, size: f32) -> Vec2 {
    let figure = 0.6 * size;
    let reach = 0.4 * figure;
    let (cos60, sin60) = (0.5, 0.866_025_4);
    let forward = if heading.length_squared() > 1e-6 { heading.normalize() } else { Vec2::ZERO };
    vec2(forward.x * reach * cos60, -figure - reach * sin60 + forward.y * reach * cos60)
}

/// The falloff of a light at distance `d` out of its radius `r`: 1 at the light, 0 at and past `r`,
/// smooth at both ends ((1 − (d/r)²)²).
pub fn falloff(d: f32, r: f32) -> f32 {
    if r <= 0.0 || d >= r {
        return 0.0;
    }
    let x = d / r;
    let t = 1.0 - x * x;
    t * t
}

/// What a light belongs to: a building (by its location index, which casts no shadow of its own) or a
/// figure's torch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Building(usize),
    Torch,
}

/// One point light of a frame, in world units (cells).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Light {
    /// Its ground point (a building's footprint centre; a torch's point on the screen, see `torch_offset`).
    pub at: Vec2,
    /// Its reach in cells.
    pub radius: f32,
    /// 0 to 1: the night's factor (`daylight::Light::glow`), with a torch's flicker.
    pub strength: f32,
    pub owner: Owner,
}

/// The lights of one frame, nearest the view's centre first, and the quality they are used at.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub lights: Vec<Light>,
    pub night: f32,
    pub quality: Quality,
}

impl Frame {
    /// The frame's lights sorted nearest the view's centre (`centre`, in cells) first.
    pub fn new(mut lights: Vec<Light>, centre: Vec2, night: f32, quality: Quality) -> Frame {
        lights.sort_by(|a, b| a.at.distance_squared(centre).total_cmp(&b.at.distance_squared(centre)));
        Frame { lights, night, quality }
    }

    /// The lights that cast shadows, light the relief and glow (the nearest `quality.lit()`).
    pub fn lit(&self) -> &[Light] {
        &self.lights[..self.lights.len().min(self.quality.lit())]
    }
}

/// The silhouette a light lays on the ground from one caster: its direction on the ground (a unit vector,
/// away from the light), its length per unit of the caster's height, and its alpha at full strength.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cast {
    pub dir: Vec2,
    pub len: f32,
    pub alpha: f32,
}

/// The alpha of a light's silhouette at full strength and at the light.
const CAST_ALPHA: f32 = 0.8;

/// The silhouette of a light (at `at`, reaching `radius`, of `strength`) on a caster whose foot is `foot`
/// (the same units). None past its reach and at the light's own point (the caller leaves out a light's own
/// building). Nearer the light, the shadow is longer and darker; it fades to nothing at the reach.
pub fn cast(at: Vec2, radius: f32, foot: Vec2, strength: f32) -> Option<Cast> {
    let d = foot.distance(at);
    if d < 1e-3 || d >= radius {
        return None;
    }
    let f = falloff(d, radius);
    Some(Cast { dir: (foot - at) / d, len: 0.4 + 1.0 * (1.0 - d / radius), alpha: CAST_ALPHA * f * strength })
}

/// The uniforms of the lit shaders for one pass: each light as (x, y, radius, strength) in the pass's 2D
/// pixels, and the counts/factors (`info`: lights, relief lights, night; the fourth is unused).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Uniforms {
    pub lights: [Vec4; SLOTS],
    pub info: Vec4,
}

/// The shader uniforms for `frame`'s lit lights, the pass's 2D point of each light (`to_px`) and the scale
/// of its pixels per cell.
pub fn uniforms(frame: &Frame, to_px: impl Fn(Vec2) -> Vec2, scale: f32) -> Uniforms {
    let mut lights = [Vec4::ZERO; SLOTS];
    let lit = frame.lit();
    for (slot, l) in lights.iter_mut().zip(lit) {
        let p = to_px(l.at);
        *slot = vec4(p.x, p.y, l.radius * scale, l.strength);
    }
    let relief = lit.len().min(frame.quality.relief());
    Uniforms { lights, info: vec4(lit.len() as f32, relief as f32, frame.night, 0.0) }
}

local! {
    /// The lights of this frame (`set_frame`), None when the light is off or not yet drawn.
    static FRAME: RefCell<Option<Frame>> = const { RefCell::new(None) };
}

/// Sets the frame's lights (`None` when the light is off).
pub fn set_frame(frame: Option<Frame>) {
    FRAME.with(|f| *f.borrow_mut() = frame);
}

/// Runs `f` on the frame's lights (None when the light is off).
pub fn with_frame<R>(f: impl FnOnce(Option<&Frame>) -> R) -> R {
    FRAME.with(|cell| f(cell.borrow().as_ref()))
}

/// The level in use: `RAZDOR_LIGHT_QUALITY=high|medium|low` when set (a snapshot's, read once), else the
/// setting (`daylight::set_prefs`).
pub fn quality() -> Quality {
    crate::ui::daylight::quality()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn light(at: Vec2, strength: f32, owner: Owner) -> Light {
        Light { at, radius: 2.0, strength, owner }
    }

    #[test]
    fn each_level_keeps_its_lights_and_the_relief_subset() {
        assert_eq!((Quality::High.lit(), Quality::High.relief()), (8, 8));
        assert_eq!((Quality::Medium.lit(), Quality::Medium.relief()), (4, 2));
        assert_eq!((Quality::Low.lit(), Quality::Low.relief()), (0, 0));
        assert_eq!(default_quality(false), Quality::High);
        assert_eq!(default_quality(true), Quality::Medium);
        assert_eq!(Quality::High.next(), Quality::Medium);
        assert_eq!(Quality::Low.next(), Quality::High);
        assert_eq!(Quality::parse(" Medium "), Some(Quality::Medium));
        assert_eq!(Quality::parse("ultra"), None);
    }

    #[test]
    fn the_frame_picks_the_lights_nearest_the_view_centre_by_quality() {
        let lights: Vec<Light> = (0..12).map(|i| light(vec2(i as f32 * 10.0, 0.0), 1.0, Owner::Torch)).collect();
        let f = Frame::new(lights, vec2(51.0, 0.0), 1.0, Quality::Medium);
        let lit: Vec<f32> = f.lit().iter().map(|l| l.at.x).collect();
        assert_eq!(lit, vec![50.0, 60.0, 40.0, 70.0], "the four nearest, nearest first");
        let high = Frame { quality: Quality::High, ..f.clone() };
        assert_eq!(high.lit().len(), 8);
        let low = Frame { quality: Quality::Low, ..f };
        assert!(low.lit().is_empty(), "low keeps the pools only");
    }

    #[test]
    fn a_torch_is_forward_and_up_from_the_head_at_each_of_eight_headings() {
        let size = 100.0;
        let figure = 0.6 * size;
        let reach = 0.4 * figure;
        for k in 0..8 {
            let a = k as f32 * std::f32::consts::FRAC_PI_4;
            let heading = vec2(a.cos(), a.sin());
            let off = torch_offset(heading, size);
            // Along the heading by reach·cos60, and above the head by reach·sin60.
            assert!((off - vec2(heading.x * reach * 0.5, -figure - reach * 0.866_025_4 + heading.y * reach * 0.5)).length() < 1e-3, "heading {k}: {off:?}");
            // The part along the heading is the horizontal reach (the torch leads the way, not behind).
            let along = (off.x * heading.x + (off.y + figure + reach * 0.866_025_4) * heading.y).max(0.0);
            assert!((along - reach * 0.5).abs() < 1e-3, "heading {k} along {along}");
        }
    }

    #[test]
    fn a_torch_with_no_heading_stands_straight_above_the_head() {
        let off = torch_offset(Vec2::ZERO, 100.0);
        assert!(off.x.abs() < 1e-5, "{off:?}");
        assert!(off.y < -60.0, "above the head: {off:?}");
    }

    #[test]
    fn falloff_is_one_at_the_light_zero_at_the_reach_and_falls_smoothly() {
        assert!((falloff(0.0, 2.0) - 1.0).abs() < 1e-6);
        assert_eq!(falloff(2.0, 2.0), 0.0);
        assert_eq!(falloff(3.0, 2.0), 0.0);
        let mut prev = 1.0;
        for i in 1..=20 {
            let v = falloff(i as f32 * 0.1, 2.0);
            assert!(v <= prev + 1e-6, "monotone at {i}");
            prev = v;
        }
    }

    #[test]
    fn a_silhouette_points_away_from_its_light_longer_and_darker_when_nearer() {
        let near = cast(Vec2::ZERO, 2.0, vec2(0.5, 0.0), 1.0).expect("inside the reach");
        let far = cast(Vec2::ZERO, 2.0, vec2(1.5, 0.0), 1.0).expect("inside the reach");
        assert!((near.dir - vec2(1.0, 0.0)).length() < 1e-5, "away from the light: {:?}", near.dir);
        assert!(near.len > far.len, "nearer is longer: {} {}", near.len, far.len);
        assert!(near.alpha > far.alpha, "nearer is darker");
        assert!(cast(Vec2::ZERO, 2.0, vec2(2.0, 0.0), 1.0).is_none(), "the reach ends it");
        assert!(cast(Vec2::ZERO, 2.0, Vec2::ZERO, 1.0).is_none(), "nothing at the light's own point");
        let dim = cast(Vec2::ZERO, 2.0, vec2(0.5, 0.0), 0.5).expect("inside");
        assert!((dim.alpha - near.alpha * 0.5).abs() < 1e-6, "the night's strength scales it");
    }

    #[test]
    fn the_uniforms_hold_the_lit_lights_in_pixels_and_count_the_relief_ones() {
        let lights: Vec<Light> = (0..10).map(|i| light(vec2(i as f32, 0.0), 0.5 + 0.01 * i as f32, Owner::Torch)).collect();
        let f = Frame::new(lights, Vec2::ZERO, 0.8, Quality::Medium);
        let u = uniforms(&f, |p| p * 32.0, 32.0);
        assert_eq!(u.info, vec4(4.0, 2.0, 0.8, 0.0));
        let first = u.lights[0];
        assert!((first.x - 0.0).abs() < 1e-5 && (first.z - 2.0 * 32.0).abs() < 1e-3, "{first:?}");
        assert_eq!(u.lights[4], Vec4::ZERO, "the slots past the lit lights stay empty");
    }
}
