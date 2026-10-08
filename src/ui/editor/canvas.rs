//! The editor's map view: the scenario drawn as the world map draws it (the original art via
//! [`DtArt`] when the install is there, placeholder colours and shapes otherwise), with the
//! editor's overlays: grid, footprints, brush and placement previews, hero starts, lantern
//! radii and patrol areas. And the minimap.

use macroquad::prelude::*;

use razdor::dt::dtm::Scenario;
use razdor::editor::geometry::{object_cover, CellRect, Footprint};
use razdor::editor::{EditorDoc, Palette, TerrainShape, Tool, ToolState, Target};

use crate::ui::dt_art::DtArt;
use crate::ui::widgets::*;
use crate::ui::world_view::{bridge_offset, draw_wrapped, faction_color, figure_stem, surface_color};

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
    key: Option<(u64, usize)>,
    tex: Option<Texture2D>,
}

impl Overview {
    pub fn texture(&mut self, doc: &EditorDoc) -> Option<Texture2D> {
        let s = &doc.scenario;
        let key = (doc.revision, s.terrain.len());
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

/// What the view shows besides the map itself.
pub struct Overlays {
    pub grid: bool,
    /// Squares covered by hills and mountains.
    pub cover: bool,
    /// Patrol radii of every army (the selected one's is always shown).
    pub patrols: bool,
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
            layer.draw(&s.terrain, (s.width(), s.height()), Rect::new(a.x, a.y, b.x - a.x, b.y - a.y), view, vec2(CW, CH));
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
pub fn draw_map(doc: &EditorDoc, art: Option<&DtArt>, cam: &Cam, overview: Option<&Texture2D>) {
    let s = &doc.scenario;
    let Some(vis) = cam.visible(s.width(), s.height()) else { return };
    draw_terrain(s, art, cam, vis, overview);
    // Sprites stand on their cell and reach up to ~10 cells above it.
    let (below, side) = (12, 10);
    let reach = CellRect { x0: vis.x0 - side, y0: vis.y0, x1: vis.x1 + side, y1: vis.y1 + below };
    let far = cam.zoom < 0.3;
    let mut items: Vec<(f32, Item)> = Vec::new();
    if !far || s.objects.len() < 20_000 {
        for (i, o) in s.objects.iter().enumerate() {
            if reach.contains(o.x as i32, o.y as i32) {
                items.push((o.y as f32, Item::Object(i)));
            }
        }
    }
    for (i, b) in s.buildings.iter().enumerate() {
        let f = Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y).bounds();
        if f.overlaps(&reach) {
            let key = if matches!(b.kind, 13 | 14) { b.y as f32 - 1000.0 } else { b.y as f32 + 0.01 };
            items.push((key, Item::Building(i)));
        }
    }
    for (i, a) in s.armies.iter().enumerate() {
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
                let label = razdor::editor::palette::building_type_label(b.kind).chars().next().map(String::from).unwrap_or_default();
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

fn outline(cam: &Cam, r: CellRect, thick: f32, color: Color) {
    let s = cam.rect(r);
    draw_rectangle_lines(s.x, s.y, s.w, s.h, thick, color);
}

fn circle_cells(cam: &Cam, x: u16, y: u16, radius: f32, color: Color) {
    let c = cam.to_screen(vec2(x as f32 + 0.5, y as f32 + 0.5));
    let cs = cam.cell_size();
    // Radius in cells; cells are wider than tall, so the circle is an ellipse on screen.
    let (rx, ry) = (radius * cs.x, radius * cs.y);
    let n = 48;
    for k in 0..n {
        let (a0, a1) = (k as f32 / n as f32 * std::f32::consts::TAU, (k + 1) as f32 / n as f32 * std::f32::consts::TAU);
        draw_line(c.x + rx * a0.cos(), c.y + ry * a0.sin(), c.x + rx * a1.cos(), c.y + ry * a1.sin(), 1.5, color);
    }
}

/// Grid, points, hero starts, selection, patrols and the tool's preview under the mouse.
pub fn draw_overlays(doc: &EditorDoc, tools: &ToolState, palette: &Palette, art: Option<&DtArt>, cam: &Cam, hover: Option<(i32, i32)>, o: &Overlays) {
    let s = &doc.scenario;
    let Some(vis) = cam.visible(s.width(), s.height()) else { return };
    let cs = cam.cell_size();
    if o.grid && cs.x >= 8.0 {
        let line = Color::new(0.0, 0.0, 0.0, 0.25);
        for x in vis.x0..=vis.x1 + 1 {
            let a = cam.to_screen(vec2(x as f32, vis.y0 as f32));
            let b = cam.to_screen(vec2(x as f32, vis.y1 as f32 + 1.0));
            draw_line(a.x, a.y, b.x, b.y, 1.0, line);
        }
        for y in vis.y0..=vis.y1 + 1 {
            let a = cam.to_screen(vec2(vis.x0 as f32, y as f32));
            let b = cam.to_screen(vec2(vis.x1 as f32 + 1.0, y as f32));
            draw_line(a.x, a.y, b.x, b.y, 1.0, line);
        }
    }
    if o.cover {
        for ob in s.objects.iter().filter(|ob| razdor::editor::geometry::is_massif(ob.class)) {
            let r = object_cover(ob);
            if r.overlaps(&vis) {
                let sr = cam.rect(r);
                draw_rectangle(sr.x, sr.y, sr.w, sr.h, Color::new(0.8, 0.2, 0.1, 0.18));
            }
        }
    }
    // Points: lanterns yellow with their radius, event points blue.
    for (i, p) in s.points.iter().enumerate() {
        if !vis.contains(p.x as i32, p.y as i32) {
            continue;
        }
        let c = cam.to_screen(vec2(p.x as f32 + 0.5, p.y as f32 + 0.5));
        let r = (6.0 * cam.zoom).max(3.0);
        if p.model == 8 {
            draw_circle(c.x, c.y, r, Color::new(1.0, 0.85, 0.2, if p.active != 0 { 1.0 } else { 0.5 }));
            if p.radius > 0 {
                circle_cells(cam, p.x, p.y, p.radius as f32, Color::new(1.0, 0.85, 0.2, 0.5));
            }
        } else {
            draw_poly(c.x, c.y, 4, r * 1.2, 45.0, Color::new(0.35, 0.6, 1.0, 1.0));
        }
        if cam.zoom >= 0.6 {
            text(&format!("{}", i + 1), c.x + r + 2.0, c.y - r, 14.0, WHITE);
        }
    }
    // Hero starts: the initials of the classes (K, A, R).
    for (k, h) in s.header.heroes.iter().enumerate() {
        let c = cam.to_screen(vec2(h.x as f32 + 0.5, h.y as f32 + 0.5));
        let r = (9.0 * cam.zoom).max(5.0);
        draw_circle(c.x, c.y, r, Color::new(0.2, 0.8, 0.3, 0.85));
        draw_circle_lines(c.x, c.y, r, 1.5, BLACK);
        let initial: String = razdor::editor::palette::HERO_CLASSES.get(k).map(|n| razdor::i18n::tr(n)).and_then(|n| n.chars().next()).map(String::from).unwrap_or_default();
        text_centered(&initial, c.x, c.y + r * 0.45, r * 1.3, BLACK);
    }
    for a in s.armies.iter() {
        if o.patrols && a.patrols != 0 && a.patrol_radius > 0 {
            circle_cells(cam, a.x, a.y, a.patrol_radius as f32, Color::new(1.0, 0.4, 0.3, 0.35));
        }
    }
    match tools.selected {
        Some(Target::Building(id)) => {
            if let Some(b) = s.building(id) {
                let f = Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y);
                outline(cam, f.main, 2.5, ACCENT);
                if let Some(e) = f.extra_row {
                    outline(cam, e, 1.5, Color::new(0.95, 0.78, 0.3, 0.6));
                }
            }
        }
        Some(Target::Army(id)) => {
            if let Some(a) = s.army(id) {
                outline(cam, CellRect::brush(a.x as i32, a.y as i32, 1), 2.5, ACCENT);
                if a.patrol_radius > 0 {
                    circle_cells(cam, a.x, a.y, a.patrol_radius as f32, Color::new(1.0, 0.5, 0.3, 0.8));
                }
            }
        }
        Some(Target::Point(id)) => {
            if let Some(p) = s.points.get(id as usize - 1) {
                outline(cam, CellRect::brush(p.x as i32, p.y as i32, 1), 2.5, ACCENT);
            }
        }
        None => {}
    }
    let Some((hx, hy)) = hover else { return };
    let inside = hx >= 0 && hy >= 0 && (hx as u32) < s.width() && (hy as u32) < s.height();
    let white = Color::new(1.0, 1.0, 1.0, 0.8);
    match tools.tool {
        Tool::Terrain { shape: TerrainShape::Brush(size), .. } | Tool::Objects { size, .. } | Tool::Erase { size, .. } => {
            outline(cam, CellRect::brush(hx, hy, size), 1.5, white);
        }
        Tool::Terrain { shape: TerrainShape::Rect, .. } => {
            let from = tools.rect_start().unwrap_or((hx, hy));
            outline(cam, CellRect::spanning(from, (hx, hy)), 2.0, white);
        }
        Tool::Terrain { shape: TerrainShape::Fill, .. } | Tool::Army | Tool::Point { .. } | Tool::HeroStart(_) => {
            outline(cam, CellRect::brush(hx, hy, 1), 1.5, white);
        }
        Tool::Building { kind, picture_type, variant } if inside => {
            let size = palette.footprint(picture_type, variant);
            let f = Footprint::of(hx, hy, size.0, size.1);
            let ok = f.inside(s.width(), s.height());
            draw_building_sprite(art, cam, hx as u16, hy as u16, size, (picture_type, variant), razdor::editor::palette::building_type_label(kind), 0.55);
            let col = if ok { Color::new(0.3, 1.0, 0.4, 0.9) } else { Color::new(1.0, 0.25, 0.2, 0.9) };
            outline(cam, f.main, 2.0, col);
            if let Some(e) = f.extra_row {
                outline(cam, e, 1.0, col);
            }
        }
        Tool::Select => {
            if let Some(t) = doc.hit(hx, hy) {
                let r = match t {
                    Target::Building(id) => s.building(id).map(|b| Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y).bounds()),
                    Target::Army(id) => s.army(id).map(|a| CellRect::brush(a.x as i32, a.y as i32, 1)),
                    Target::Point(id) => s.points.get(id as usize - 1).map(|p| CellRect::brush(p.x as i32, p.y as i32, 1)),
                };
                if let Some(r) = r {
                    outline(cam, r, 1.5, white);
                }
            } else {
                outline(cam, CellRect::brush(hx, hy, 1), 1.0, Color::new(1.0, 1.0, 1.0, 0.4));
            }
        }
        _ => {}
    }
}

/// The minimap in `area`: the whole map and the view's rectangle. Returns the cell-unit point
/// clicked or dragged to, to centre the view there.
pub fn minimap(doc: &EditorDoc, overview: Option<&Texture2D>, cam: &Cam, area: Rect) -> Option<Vec2> {
    let s = &doc.scenario;
    let (w, h) = (s.width().max(1) as f32, s.height().max(1) as f32);
    // Keep the map's proportions (cells are 32×22).
    let aspect = (w * CW) / (h * CH);
    let (mw, mh) = if area.w / area.h > aspect { (area.h * aspect, area.h) } else { (area.w, area.w / aspect) };
    let r = Rect::new(area.x + (area.w - mw) / 2.0, area.y + (area.h - mh) / 2.0, mw, mh);
    draw_rectangle(area.x, area.y, area.w, area.h, Color::new(0.05, 0.05, 0.05, 1.0));
    if let Some(tex) = overview {
        draw_texture_ex(tex, r.x, r.y, WHITE, DrawTextureParams { dest_size: Some(vec2(r.w, r.h)), ..Default::default() });
    }
    let k = vec2(r.w / w, r.h / h);
    for b in &s.buildings {
        let c = vec2(r.x + (b.x as f32 + 0.5) * k.x, r.y + (b.y as f32 + 0.5) * k.y);
        draw_rectangle(c.x - 1.5, c.y - 1.5, 3.0, 3.0, faction_color(b.faction));
    }
    for a in &s.armies {
        let c = vec2(r.x + (a.x as f32 + 0.5) * k.x, r.y + (a.y as f32 + 0.5) * k.y);
        draw_circle(c.x, c.y, 1.8, WHITE);
    }
    let a = cam.to_cells(vec2(cam.view.x, cam.view.y));
    let b = cam.to_cells(vec2(cam.view.right(), cam.view.bottom()));
    let (vx0, vy0) = (r.x + a.x.max(0.0) * k.x, r.y + a.y.max(0.0) * k.y);
    let (vx1, vy1) = (r.x + b.x.min(w) * k.x, r.y + b.y.min(h) * k.y);
    draw_rectangle_lines(vx0, vy0, (vx1 - vx0).max(2.0), (vy1 - vy0).max(2.0), 1.5, WHITE);
    draw_rectangle_lines(area.x, area.y, area.w, area.h, 1.0, DIM);
    let m = Vec2::from(crate::ui::widgets::pointer());
    if r.contains(m) && !input_blocked() && is_mouse_button_down(MouseButton::Left) {
        return Some(vec2((m.x - r.x) / k.x, (m.y - r.y) / k.y));
    }
    None
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
