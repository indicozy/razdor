//! Backlog 1605, step 5 (stage 1): the GPU's placement of the map sprites (`sprite_gpu`: the billboard's terms
//! as uniforms, each corner as a world offset from its foot) against the CPU's (`standing_model` + `picture_quad`),
//! on the real install's maps, for every decoration and building, flat and 2.5D at several zooms, pitches and yaws.
//! Skipped without `RAZDOR_DT_DIR`. The largest corner error is printed (`cargo test … -- --nocapture`).

use super::*;
use crate::ui::sprite_gpu;
use razdor::dt::install::DtInstall;
use razdor::rules::content::{Content, HeroClass};
use std::sync::Arc;

/// The world packing of a building, from the map alone (as `building_sprite_origin` and `building_foot` place it).
fn building_packing(l: &Location, sprite: (f32, f32), grid: Grid) -> (Vec2, Vec2) {
    let rh = grid.row_height();
    let (x, y) = grid.center(l.anchor);
    let (sum, n) = l.cells().fold((Vec2::ZERO, 0u32), |(s, n), t| {
        let (cx, cy) = grid.center(t);
        (s + vec2(cx, cy), n + 1)
    });
    let foot = if n == 0 { vec2(x, y) } else { sum / n as f32 };
    let raise = if l.size.0 > l.size.1 { 11.0 } else { 0.0 };
    let (dx, dy) = bridge_offset(l.picture);
    let corner = vec2(x + 0.5, y + rh / 2.0) + vec2(dx - sprite.0 - 0.5, dy - sprite.1 - raise - 0.5) / PX;
    (foot, corner - foot)
}

/// The GPU's four corners of a sprite under a tilted camera: each corner is the billboard's point of the foot's ground
/// plus the corner's world offset (`sprite_gpu::Billboard::point`), projected through the window's view.
fn gpu_corners(cam: &Camera, foot_w: Vec2, rel_tl: Vec2, sprite: (f32, f32)) -> Option<[Vec2; 4]> {
    let tilt = cam.tilt?;
    let rel = sprite_gpu::quad_rel(rel_tl, vec2(sprite.0, sprite.1), PX);
    let bb = sprite_gpu::Billboard::new(tilt.cam.pitch_deg, tilt.cam.yaw_deg, sprite_size());
    let view_proj = tilt.cam.view_proj(cam.view);
    let foot3 = vec3(foot_w.x, 0.0, foot_w.y);
    let mut out = [Vec2::ZERO; 4];
    for (k, r) in rel.iter().enumerate() {
        out[k] = sprite_gpu::project(view_proj, bb.point(foot3, *r), cam.view)?;
    }
    Some(out)
}

/// The CPU's four corners of the same sprite under the same camera: `standing_model` and `picture_quad`, as the draw
/// places it (a map sprite's base is its foot).
fn cpu_corners(cam: &Camera, foot2: Vec2, origin_of: impl Fn(&Camera) -> Vec2, sprite: (f32, f32)) -> Option<[Vec2; 4]> {
    let cs = cam.tilted_space();
    let tilt = cam.tilt?;
    let origin = origin_of(&cs);
    let zoom = cs.scale / PX;
    let picture = Rect::new(origin.x, origin.y, sprite.0 * zoom, sprite.1 * zoom);
    let (model, _) = standing_model(&cs, &tilt, foot2, foot2, false);
    picture_quad(&tilt, cam.view, model, picture)
}

/// The largest difference of the corners (px), or None when neither places the sprite on the view (both behind the eye).
/// Only a sprite whose box meets the window (the cull's margin) can change a pixel, so a pair far off it is not an error.
/// A corner far off the window (the sprites near the eye's depth plane project to millions of pixels) is compared
/// as a relative error: f32 rounding of a world point is amplified there by 1/w, and the pixels are not affected.
fn corner_error(cpu: Option<[Vec2; 4]>, gpu: Option<[Vec2; 4]>, window: Rect) -> Option<f32> {
    let near = |q: &[Vec2; 4]| {
        let b = quad_bounds(*q);
        b.x < window.x + window.w + OFF_WINDOW_MARGIN && b.x + b.w > window.x - OFF_WINDOW_MARGIN && b.y < window.y + window.h + OFF_WINDOW_MARGIN && b.y + b.h > window.y - OFF_WINDOW_MARGIN
    };
    match (cpu, gpu) {
        (Some(c), Some(g)) => {
            if !near(&c) && !near(&g) {
                return None;
            }
            Some((0..4).map(|k| (g[k] - c[k]).abs().max_element()).fold(0.0, f32::max))
        }
        (None, None) => None,
        _ => Some(f32::INFINITY),
    }
}

#[test]
fn the_gpu_placement_matches_the_cpu_on_every_sprite() {
    let Some(dir) = std::env::var_os(razdor::dt::install::ENV_VAR) else { return };
    let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
    let objects = dt.map_objects().expect("objects");
    let content = Arc::new(Content::from_dt(&dt));
    let view = Rect::new(0.0, 0.0, 1024.0, 768.0);
    // Every error above the tolerance, with its sprite (the worst are printed when the test fails).
    let mut bad: Vec<(f32, String)> = Vec::new();
    let mut worst = (0.0f32, String::new());
    let mut count = 0usize;
    let mut note = |e: Option<f32>, what: &dyn Fn() -> String| {
        if let Some(e) = e {
            if e > worst.0 {
                worst = (e, what());
            }
            if e >= 0.01 {
                bad.push((e, what()));
            }
        }
    };
    for entry in dt.maps.iter().take(3) {
        let scenario = entry.load().expect("map loads");
        let game = Game::from_scenario(content.clone(), &scenario, HeroClass::Knight);
        let grid = game.world.map.grid;
        for zoom_scale in [1.6f32, 6.4, 16.0, 32.0, 48.0] {
            let at = (game.world.map.w as f32 / 2.0, game.world.map.h as f32 / 2.0 * grid.row_height());
            let origin = vec2(at.0 * zoom_scale, at.1 * zoom_scale) - vec2(view.w, view.h) / 2.0;
            let mk = |tilt: Option<Tilt>| Camera { origin, view, draw: view, scale: zoom_scale, grid, tilt };
            let centre = mk(None).centre();
            let mut cams: Vec<(String, Camera)> = vec![(format!("flat scale {zoom_scale}"), mk(None))];
            for pitch in [35.0f32, 55.0, 75.0] {
                for yaw in [0.0f32, 37.0, 90.0, 180.0, 271.0] {
                    let t = view25::TiltCamera::new(Vec2::from(centre), zoom_scale, view, pitch, yaw);
                    let tilt = Tilt { cam: t, plan: t.ground_footprint(view, view), cull: t.ground_footprint(view, view) };
                    cams.push((format!("2.5D scale {zoom_scale} pitch {pitch} yaw {yaw}"), mk(Some(tilt))));
                }
            }
            for (name, cam) in &cams {
                for o in &game.world.map.objects {
                    let Some(sp) = objects.decoration(o.class, o.sprite) else { continue };
                    let s = (sp.image.width as f32, sp.image.height as f32);
                    let (foot_w, rel_tl) = deco_packing(o, s, grid);
                    count += 1;
                    let what = || format!("{name}: deco class {} sprite {} tile {:?}", o.class, o.sprite, o.tile);
                    if cam.tilt.is_none() {
                        // Flat: the picture is the 2D rectangle at its origin (`draw_object`), each corner the world point
                        // scaled, moved by the origin and the view.
                        let zoom = cam.scale / PX;
                        let cpu_o = object_sprite_origin(o, s.0, s.1, cam);
                        let cpu = [cpu_o, cpu_o + vec2(s.0 * zoom, 0.0), cpu_o + vec2(s.0 * zoom, s.1 * zoom), cpu_o + vec2(0.0, s.1 * zoom)];
                        let tl = (foot_w + rel_tl) * cam.scale - vec2(cam.origin.x, cam.origin.y) + vec2(cam.view.x, cam.view.y);
                        let gpu_q = sprite_gpu::quad_rel(Vec2::ZERO, vec2(s.0, s.1), PX).map(|d| tl + d * cam.scale);
                        note(corner_error(Some(cpu), Some(gpu_q), cam.view), &what);
                        continue;
                    }
                    let cs = cam.tilted_space();
                    let foot2 = object_foot(o, &cs);
                    // The CPU's foot in world units must be the packed one (a check of the packing itself).
                    let foot_cpu_w = (foot2 + cs.origin) / cs.scale;
                    note(Some((foot_cpu_w - foot_w).abs().max_element() * cs.scale), &|| format!("{}: foot", what()));
                    let cpu = cpu_corners(cam, foot2, |c| object_sprite_origin(o, s.0, s.1, c), s);
                    note(corner_error(cpu, gpu_corners(cam, foot_w, rel_tl, s), cam.view), &what);
                }
                for l in &game.world.locations {
                    let Some(sp) = objects.building(l.picture.0, l.picture.1) else { continue };
                    let s = (sp.image.width as f32, sp.image.height as f32);
                    let (foot_w, rel_tl) = building_packing(l, s, grid);
                    count += 1;
                    let what = || format!("{name}: building {:?} picture {:?}", l.anchor, l.picture);
                    if cam.tilt.is_none() {
                        let zoom = cam.scale / PX;
                        let cpu_o = building_sprite_origin(l, s.0, s.1, cam);
                        let cpu = [cpu_o, cpu_o + vec2(s.0 * zoom, 0.0), cpu_o + vec2(s.0 * zoom, s.1 * zoom), cpu_o + vec2(0.0, s.1 * zoom)];
                        let tl = (foot_w + rel_tl) * cam.scale - vec2(cam.origin.x, cam.origin.y) + vec2(cam.view.x, cam.view.y);
                        let gpu_q = sprite_gpu::quad_rel(Vec2::ZERO, vec2(s.0, s.1), PX).map(|d| tl + d * cam.scale);
                        note(corner_error(Some(cpu), Some(gpu_q), cam.view), &what);
                        continue;
                    }
                    let cs = cam.tilted_space();
                    let foot2 = building_foot(l, &cs);
                    let cpu = cpu_corners(cam, foot2, |c| building_sprite_origin(l, s.0, s.1, c), s);
                    note(corner_error(cpu, gpu_corners(cam, foot_w, rel_tl, s), cam.view), &what);
                }
            }
        }
    }
    bad.sort_by(|a, b| b.0.total_cmp(&a.0));
    eprintln!("gpu placement: {count} sprite-camera pairs checked, {} above 0.01 px, worst {:.6} px ({})", bad.len(), worst.0, worst.1);
    for (e, w) in bad.iter().take(12) {
        eprintln!("  {e:.4} px  {w}");
    }
    assert!(bad.is_empty(), "{} sprite-camera pairs above 0.01 px; worst {:.6} px at {}", bad.len(), worst.0, worst.1);
}
