//! The studio's map canvas: the package's map (its compiled `.DTm` scenario) drawn as the world map
//! draws it (the original art via [`DtArt`] when the install is there, placeholder colours and
//! shapes otherwise), and the camera that looks at it. The Map tab draws the editing overlays
//! (the marks, the tools' previews, the selection) over it (`map_tab`).

use macroquad::prelude::*;

use razdor::dt::dtm::{BuildingType, Scenario};
use razdor::i18n::{n_, tr};
use razdor::studio::geometry::{CellRect, Footprint};

use crate::ui::dt_art::DtArt;
use crate::ui::widgets::*;
use crate::ui::world_view::{bridge_offset, draw_wrapped, figure_stem, surface_color};

/// A cell's size on screen at zoom 1: the original's 32×22 px.
pub const CW: f32 = 32.0;
pub const CH: f32 = 22.0;
pub const ZOOM_MIN: f32 = 0.12;
pub const ZOOM_MAX: f32 = 2.5;

/// Where the view looks. Cell `(x, y)` covers `[x, x+1) × [y, y+1)` in cell units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cam {
    pub zoom: f32,
    /// The cell-unit point at the centre of the view.
    pub centre: Vec2,
    /// The screen rectangle of the map view.
    pub view: Rect,
}

impl Cam {
    pub fn new(view: Rect, w: u32, h: u32) -> Cam {
        Cam { zoom: 1.0, centre: vec2(w as f32 / 2.0, h as f32 / 2.0), view }
    }

    pub fn cell_size(&self) -> Vec2 {
        vec2(CW * self.zoom, CH * self.zoom)
    }

    /// Screen position of a point in cell units.
    pub fn to_screen(self, p: Vec2) -> Vec2 {
        self.view.center() + (p - self.centre) * self.cell_size()
    }

    /// The point in cell units under a screen position.
    pub fn to_cells(self, s: Vec2) -> Vec2 {
        self.centre + (s - self.view.center()) / self.cell_size()
    }

    /// The cell under a screen position (may be outside the map).
    pub fn cell_at(&self, s: Vec2) -> (i32, i32) {
        let p = self.to_cells(s);
        (p.x.floor() as i32, p.y.floor() as i32)
    }

    /// The screen rectangle of a cell rectangle.
    pub fn rect(&self, r: CellRect) -> Rect {
        let a = self.to_screen(vec2(r.x0 as f32, r.y0 as f32));
        let b = self.to_screen(vec2(r.x1 as f32 + 1.0, r.y1 as f32 + 1.0));
        Rect::new(a.x, a.y, b.x - a.x, b.y - a.y)
    }

    /// Cells in view (one more on each side), clipped to the map.
    pub fn visible(&self, w: u32, h: u32) -> Option<CellRect> {
        let a = self.to_cells(vec2(self.view.x, self.view.y));
        let b = self.to_cells(vec2(self.view.right(), self.view.bottom()));
        CellRect { x0: a.x.floor() as i32 - 1, y0: a.y.floor() as i32 - 1, x1: b.x.ceil() as i32 + 1, y1: b.y.ceil() as i32 + 1 }.clip(w, h)
    }

    /// Zooms by `factor`, keeping the point under screen position `at` in place.
    pub fn zoom_at(&mut self, factor: f32, at: Vec2) {
        let before = self.to_cells(at);
        self.zoom = (self.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
        let after = self.to_cells(at);
        self.centre += before - after;
    }

    /// Moves by a screen-pixel delta.
    pub fn pan(&mut self, delta: Vec2) {
        self.centre -= delta / self.cell_size();
    }

    /// Keeps the centre over the map.
    pub fn clamp(&mut self, w: u32, h: u32) {
        self.centre.x = self.centre.x.clamp(0.0, w as f32);
        self.centre.y = self.centre.y.clamp(0.0, h as f32);
    }

    /// The zoom that shows the whole map.
    pub fn fit(&mut self, w: u32, h: u32) {
        self.centre = vec2(w as f32 / 2.0, h as f32 / 2.0);
        let z = (self.view.w / (w as f32 * CW)).min(self.view.h / (h as f32 * CH));
        self.zoom = z.clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

/// A texture of the terrain, one texel per cell, rebuilt when the document changes. Used
/// when zoomed far out, without art, and for the minimap.
#[derive(Default)]
pub struct Overview {
    key: Option<u64>,
    tex: Option<Texture2D>,
}

impl Overview {
    /// The texture of `s`, made again when `key` (the map's serial) changes.
    pub fn texture(&mut self, s: &Scenario, key: u64) -> Option<Texture2D> {
        if self.key != Some(key) || self.tex.is_none() {
            let (w, h) = (s.width().min(u16::MAX as u32) as u16, s.height().min(u16::MAX as u32) as u16);
            if w == 0 || h == 0 || s.terrain.len() != w as usize * h as usize {
                return None;
            }
            let mut rgba = Vec::with_capacity(s.terrain.len() * 4);
            for &c in &s.terrain {
                let col: [u8; 4] = surface_color(c).into();
                rgba.extend_from_slice(&col);
            }
            let tex = Texture2D::from_rgba8(w, h, &rgba);
            tex.set_filter(FilterMode::Nearest);
            self.tex = Some(tex);
            self.key = Some(key);
        }
        self.tex.clone()
    }
}

/// A faction's colour on the canvas: player green, ally blue, neighbour yellow, enemy red.
fn faction_color(faction: u8) -> Color {
    let c = |r, g, b| Color::from_rgba(r, g, b, 255);
    match faction {
        1 => c(70, 200, 90),
        2 => c(70, 130, 230),
        3 => c(230, 200, 60),
        4 => c(220, 60, 50),
        _ => c(200, 200, 200),
    }
}

/// The label of a building type (the first letter of it is drawn on a building without art).
fn building_type_label(kind: u8) -> &'static str {
    tr(match BuildingType::from_code(kind) {
        Some(BuildingType::Palace) => n_("Palace"),
        Some(BuildingType::Town) => n_("Town"),
        Some(BuildingType::Village) => n_("Village"),
        Some(BuildingType::Castle) => n_("Castle"),
        Some(BuildingType::Fort) => n_("Fort"),
        Some(BuildingType::Tavern) => n_("Tavern"),
        Some(BuildingType::Market) => n_("Market"),
        Some(BuildingType::Church) => n_("Church"),
        Some(BuildingType::Smithy) => n_("Smithy / house"),
        Some(BuildingType::Shipyard) => n_("Shipyard"),
        Some(BuildingType::Altar) => n_("Altar"),
        Some(BuildingType::DungeonEntrance) => n_("Dungeon entrance"),
        Some(BuildingType::Ruins) => n_("Ruins"),
        Some(BuildingType::StoneBridge) => n_("Stone bridge"),
        Some(BuildingType::WoodenBridge) => n_("Wooden bridge"),
        Some(BuildingType::Obelisk) => n_("Obelisk"),
        None => n_("Unknown building"),
    })
}

fn draw_terrain(s: &Scenario, art: Option<&DtArt>, cam: &Cam, vis: CellRect, overview: Option<&Texture2D>) {
    let cells = (vis.width() * vis.height()) as usize;
    let layer = art.and_then(|a| a.terrain_layer());
    // The blended layer costs the same at any size; square cells cost a draw call each.
    let textured = art.is_some() && cam.zoom >= 0.3 && (layer.is_some() || cells < 9000);
    if !textured {
        if let Some(tex) = overview {
            let r = cam.rect(CellRect { x0: 0, y0: 0, x1: s.width() as i32 - 1, y1: s.height() as i32 - 1 });
            draw_texture_ex(tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
        }
        return;
    }
    if let Some(layer) = layer {
        // The map area in view, in cell units; the shader counts from cell centres.
        let (tl, br) = (cam.to_cells(cam.view.point()), cam.to_cells(cam.view.point() + cam.view.size()));
        let (tl, br) = (tl.max(Vec2::ZERO), br.min(vec2(s.width() as f32, s.height() as f32)));
        if tl.x < br.x && tl.y < br.y {
            let (a, b) = (cam.to_screen(tl), cam.to_screen(br));
            let view = vec4(tl.x - 0.5, tl.y - 0.5, br.x - 0.5, br.y - 0.5);
            layer.draw(&s.terrain, (s.width(), s.height()), Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), view, vec2(CW, CH), None);
        }
        return;
    }
    let size = cam.cell_size();
    let mut present = [false; 16];
    for (x, y) in vis.cells() {
        present[(s.terrain[(y as u32 * s.width() + x as u32) as usize] & 15) as usize] = true;
    }
    for code in (0..16u8).filter(|c| present[*c as usize]) {
        let tex = art.and_then(|a| a.terrain(code));
        for (x, y) in vis.cells() {
            if s.terrain[(y as u32 * s.width() + x as u32) as usize] != code {
                continue;
            }
            let p = cam.to_screen(vec2(x as f32, y as f32));
            let dest = Rect::new(p.x, p.y, size.x, size.y);
            match &tex {
                Some(tex) => draw_wrapped(tex, dest, vec2(x as f32 * CW, y as f32 * CH), vec2(CW, CH)),
                None => draw_rectangle(dest.x, dest.y, dest.w + 0.5, dest.h + 0.5, surface_color(code)),
            }
        }
    }
}

enum Item {
    Object(usize),
    Building(usize),
    Army(usize),
}

fn placeholder_object(class: u8, sprite: u8, base: Vec2, s: f32) {
    let rgb = |r, g, b| Color::from_rgba(r, g, b, 255);
    match class {
        9 | 12 => draw_triangle(vec2(base.x, base.y - s * 1.0), vec2(base.x - s * 0.33, base.y - s * 0.1), vec2(base.x + s * 0.33, base.y - s * 0.1), rgb(40, 100, 44)),
        10 => draw_triangle(vec2(base.x, base.y - s * 0.9), vec2(base.x - s * 0.3, base.y - s * 0.1), vec2(base.x + s * 0.3, base.y - s * 0.1), rgb(110, 90, 50)),
        11 => draw_circle(base.x, base.y - s * 0.4, s * 0.42, rgb(24, 64, 30)),
        5 | 6 => {
            let k = 1.0 + (sprite / 10) as f32 * 0.5;
            draw_triangle(vec2(base.x, base.y - s * 0.9 * k), vec2(base.x - s * 0.6 * k, base.y), vec2(base.x + s * 0.6 * k, base.y), rgb(120, 116, 112));
        }
        8 => draw_circle(base.x, base.y - s * 0.2, s * 0.25, rgb(150, 150, 150)),
        _ => {
            let k = 1.0 + (sprite / 10) as f32 * 0.4;
            draw_ellipse(base.x, base.y - s * 0.2 * k, s * 0.55 * k, s * 0.25 * k, 0.0, rgb(120, 140, 70));
        }
    }
}

/// Draws the building picture (or a placeholder) at its anchor cell `(x, y)`, as the original
/// editor does (RedrawMap 0x5ab588): the picture's bottom-right corner at the anchor cell's,
/// moved by the game's bridge offsets for bridges (its own copy of the table, rounded). Unlike
/// the game it neither raises a building wider than tall nor takes off half a pixel.
#[allow(clippy::too_many_arguments)]
pub fn draw_building_sprite(art: Option<&DtArt>, cam: &Cam, x: u16, y: u16, size: (u8, u8), picture: (u8, u8), label: &str, alpha: f32) {
    let tint = Color::new(1.0, 1.0, 1.0, alpha);
    if let Some((atlas, r)) = art.and_then(|a| a.map_atlas()).and_then(|at| Some((at, at.building(picture.0, picture.1)?))) {
        let (dx, dy) = bridge_offset(picture);
        let at = cam.to_screen(vec2(x as f32 + 1.0, y as f32 + 1.0)) + vec2(dx.round() - r.w, dy.round() - r.h) * cam.zoom;
        draw_texture_ex(&atlas.texture, at.x, at.y, tint, DrawTextureParams { dest_size: Some(vec2(r.w * cam.zoom, r.h * cam.zoom)), source: Some(r), ..Default::default() });
        return;
    }
    let fr = cam.rect(Footprint::of(x as i32, y as i32, size.0, size.1).main);
    draw_rectangle(fr.x, fr.y, fr.w, fr.h, Color::new(0.8, 0.7, 0.55, 0.85 * alpha));
    draw_rectangle_lines(fr.x, fr.y, fr.w, fr.h, 1.5, Color::new(0.3, 0.25, 0.2, alpha));
    let size = (fr.h * 0.5).clamp(9.0, 28.0);
    text_centered(label, fr.center().x, fr.center().y + size * 0.35, size, Color::new(0.1, 0.08, 0.05, alpha));
}

fn draw_army_figure(art: Option<&DtArt>, cam: &Cam, x: u16, y: u16, model: u8, faction: u8) {
    let p = cam.to_screen(vec2(x as f32 + 0.5, y as f32 + 0.5));
    let size = 64.0 * cam.zoom;
    let sheet = art.and_then(|a| a.figure_sheet(figure_stem(model)));
    draw_ellipse(p.x, p.y + size * 0.1, size * 0.22, size * 0.08, 0.0, Color::new(0.0, 0.0, 0.0, 0.3));
    match sheet {
        Some(sheet) => draw_texture_ex(
            &sheet,
            p.x - size / 2.0,
            p.y - size * 0.8,
            WHITE,
            DrawTextureParams { dest_size: Some(vec2(size, size)), source: Some(Rect::new(0.0, 5.0 * 64.0, 64.0, 64.0)), ..Default::default() },
        ),
        None => {
            let r = (9.0 * cam.zoom).max(3.0);
            draw_circle(p.x, p.y - r, r, faction_color(faction));
            draw_circle_lines(p.x, p.y - r, r, 1.5, BLACK);
        }
    }
    draw_circle_lines(p.x, p.y + 3.0 * cam.zoom, 8.0 * cam.zoom + 2.0, 2.0, faction_color(faction));
}

/// The map: terrain, then objects, buildings and armies in painter's order.
/// [`draw_map`] with the layers that show: terrain, objects, buildings and armies (`shown`, in that order;
/// a hidden layer is not drawn, the studio's eye in the Слои tab).
pub fn draw_map_layers(s: &Scenario, art: Option<&DtArt>, cam: &Cam, overview: Option<&Texture2D>, shown: [bool; 4]) {
    let Some(vis) = cam.visible(s.width(), s.height()) else { return };
    let [terrain, objects, buildings, armies] = shown;
    if terrain {
        draw_terrain(s, art, cam, vis, overview);
    }
    // Sprites stand on their cell and reach up to ~10 cells above it.
    let (below, side) = (12, 10);
    let reach = CellRect { x0: vis.x0 - side, y0: vis.y0, x1: vis.x1 + side, y1: vis.y1 + below };
    let far = cam.zoom < 0.3;
    let mut items: Vec<(f32, Item)> = Vec::new();
    if objects && (!far || s.objects.len() < 20_000) {
        for (i, o) in s.objects.iter().enumerate() {
            if reach.contains(o.x as i32, o.y as i32) {
                items.push((o.y as f32, Item::Object(i)));
            }
        }
    }
    for (i, b) in s.buildings.iter().enumerate().filter(|_| buildings) {
        let f = Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y).bounds();
        if f.overlaps(&reach) {
            let key = if matches!(b.kind, 13 | 14) { b.y as f32 - 1000.0 } else { b.y as f32 + 0.01 };
            items.push((key, Item::Building(i)));
        }
    }
    for (i, a) in s.armies.iter().enumerate().filter(|_| armies) {
        if reach.contains(a.x as i32, a.y as i32) {
            items.push((a.y as f32 + 0.02, Item::Army(i)));
        }
    }
    items.sort_by(|a, b| a.0.total_cmp(&b.0));
    let atlas = art.and_then(|a| a.map_atlas());
    let unit = cam.cell_size().x;
    for (_, it) in &items {
        match it {
            Item::Object(i) => {
                let o = &s.objects[*i];
                let base = cam.to_screen(vec2(o.x as f32 + 0.5, o.y as f32 + 1.0));
                match atlas.and_then(|at| Some((at, at.decoration(o.class, o.sprite)?))) {
                    Some((at, r)) => {
                        let (w, h) = (r.w * cam.zoom, r.h * cam.zoom);
                        draw_texture_ex(&at.texture, base.x - w / 2.0, base.y - h, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), source: Some(r), ..Default::default() });
                    }
                    None => placeholder_object(o.class, o.sprite, base, unit),
                }
            }
            Item::Building(i) => {
                let b = &s.buildings[*i];
                let label = building_type_label(b.kind).chars().next().map(String::from).unwrap_or_default();
                draw_building_sprite(art, cam, b.x, b.y, (b.size_x, b.size_y), (b.picture_type, b.picture_variant), &label, 1.0);
            }
            Item::Army(i) => {
                let a = &s.armies[*i];
                draw_army_figure(art, cam, a.x, a.y, a.model, a.faction);
                if !a.is_active() {
                    let p = cam.to_screen(vec2(a.x as f32 + 0.5, a.y as f32));
                    text_centered("z", p.x + 12.0 * cam.zoom, p.y - 20.0 * cam.zoom, (18.0 * cam.zoom).max(10.0), Color::new(0.8, 0.8, 1.0, 1.0));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camera_maps_cells_and_screen() {
        let mut c = Cam::new(Rect::new(100.0, 50.0, 640.0, 440.0), 50, 50);
        // The view's centre shows the map's centre.
        assert_eq!(c.to_screen(vec2(25.0, 25.0)), vec2(420.0, 270.0));
        assert_eq!(c.cell_at(vec2(420.0, 270.0)), (25, 25));
        assert_eq!(c.cell_at(vec2(419.0, 269.0)), (24, 24));
        assert_eq!(c.rect(CellRect { x0: 25, y0: 25, x1: 26, y1: 25 }), Rect::new(420.0, 270.0, 64.0, 22.0));
        let v = c.visible(50, 50).unwrap();
        assert!(v.contains(15, 15) && v.contains(34, 34) && !v.contains(5, 5));
        // Zooming keeps the point under the mouse.
        let m = vec2(200.0, 100.0);
        let before = c.to_cells(m);
        c.zoom_at(2.0, m);
        assert!((c.to_cells(m) - before).length() < 1e-3);
        assert_eq!(c.zoom, 2.0);
        c.zoom_at(100.0, m);
        assert_eq!(c.zoom, ZOOM_MAX);
        c.pan(vec2(64.0 * ZOOM_MAX / 2.0, 0.0));
        c.centre = vec2(-10.0, 80.0);
        c.clamp(50, 50);
        assert_eq!(c.centre, vec2(0.0, 50.0));
        c.fit(200, 200);
        assert!((c.zoom - (440.0 / (200.0 * CH)).max(ZOOM_MIN)).abs() < 1e-6);
    }
}
