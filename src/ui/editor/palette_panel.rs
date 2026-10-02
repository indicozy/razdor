//! The tool palette on the right: the tools, and for each its options (surfaces, brush
//! sizes, object sprites, building pictures).

use macroquad::prelude::*;

use razdor::editor::geometry::BRUSH_SIZES;
use razdor::editor::palette::{self, building_type_label, object_class_label, Palette};
use razdor::editor::{ObjectFilter, TerrainShape, Tool, ToolState};
use razdor::i18n::{n_, tr};
use razdor::trf;

use crate::ui::dt_art::DtArt;
use crate::ui::widgets::*;
use crate::ui::world_view::{draw_wrapped, surface_color};

/// The options each tool had last, so switching tools keeps them.
pub struct PaletteState {
    pub code: u8,
    pub shape: TerrainShape,
    pub class: u8,
    pub sprite: u8,
    pub size: u32,
    pub erase_size: u32,
    pub filter: ObjectFilter,
    pub kind: u8,
    pub variant: u8,
    /// The model of a new point: 8 lantern, 9 event point, 10 AI target.
    pub point_model: u8,
    /// The map model of a new army (4 feudal, 5 rogue, 6 peasant, 7 inactive).
    pub army_model: u8,
    /// Scroll of the sprite grids, in rows.
    pub scroll: usize,
}

impl Default for PaletteState {
    fn default() -> Self {
        PaletteState {
            code: 6,
            shape: TerrainShape::Brush(3),
            class: 9,
            sprite: 0,
            size: 1,
            erase_size: 1,
            filter: ObjectFilter::All,
            kind: 3,
            variant: 0,
            point_model: 8,
            army_model: 4,
            scroll: 0,
        }
    }
}

/// The tool buttons, in order, with their shortcut keys.
pub const TOOL_KEYS: [(&str, KeyCode); 7] = [
    (n_("Select (V)"), KeyCode::V),
    (n_("Terrain (T)"), KeyCode::T),
    (n_("Objects (O)"), KeyCode::O),
    (n_("Erase (E)"), KeyCode::E),
    (n_("Building (B)"), KeyCode::B),
    (n_("Army (A)"), KeyCode::A),
    (n_("Point (P)"), KeyCode::P),
];

impl PaletteState {
    /// Tool `k` of [`TOOL_KEYS`] with this palette's options.
    pub fn tool(&self, k: usize) -> Tool {
        match k {
            1 => Tool::Terrain { code: self.code, shape: self.shape },
            2 => Tool::Objects { class: self.class, sprite: self.sprite, size: self.size },
            3 => Tool::Erase { size: self.erase_size, filter: self.filter },
            4 => Tool::Building { kind: self.kind, picture_type: self.kind, variant: self.variant },
            5 => Tool::Army { model: self.army_model },
            6 => Tool::Point { model: self.point_model },
            _ => Tool::Select,
        }
    }

    /// Makes the object and building choices exist in `palette`.
    pub fn fit(&mut self, p: &Palette) {
        if !p.has_object(self.class, self.sprite) {
            if let Some(o) = p.sprites_of(self.class).next().or_else(|| p.objects.first().copied()) {
                (self.class, self.sprite) = (o.class, o.sprite);
            }
        }
        if p.picture(self.kind, self.variant).is_none() {
            if let Some(b) = p.pictures_of(self.kind).next().or_else(|| p.buildings.first().copied()) {
                (self.kind, self.variant) = (b.picture_type, b.variant);
            }
        }
    }
}

/// A hint paragraph wrapped to `w`; returns the height used.
fn note(s: &str, x: f32, y: f32, w: f32, color: Color) -> f32 {
    let lines = wrap(s, w, 15.0);
    for (i, line) in lines.iter().enumerate() {
        text(line, x, y + 16.0 + i as f32 * 18.0, 15.0, color);
    }
    lines.len() as f32 * 18.0
}

fn tool_index(t: &Tool) -> usize {
    match t {
        Tool::Select | Tool::HeroStart(_) => 0,
        Tool::Terrain { .. } => 1,
        Tool::Objects { .. } => 2,
        Tool::Erase { .. } => 3,
        Tool::Building { .. } => 4,
        Tool::Army { .. } => 5,
        Tool::Point { .. } => 6,
    }
}

fn size_buttons(x: f32, y: f32, w: f32, current: u32) -> Option<u32> {
    let bw = (w - 3.0 * 4.0) / 4.0;
    let mut out = None;
    for (i, s) in BRUSH_SIZES.iter().enumerate() {
        if toggle_button(x + i as f32 * (bw + 4.0), y, bw, 26.0, &format!("{s}x{s}"), current == *s) {
            out = Some(*s);
        }
    }
    out
}

/// A grid of sprite thumbnails; returns the index clicked. `cell` px squares, scrolled by
/// `scroll` rows with the wheel over it.
fn sprite_grid(r: Rect, count: usize, cell: f32, scroll: &mut usize, selected: Option<usize>, draw: &dyn Fn(usize, Rect)) -> Option<usize> {
    let cols = ((r.w + 4.0) / (cell + 4.0)).floor().max(1.0) as usize;
    let rows_fit = ((r.h + 4.0) / (cell + 4.0)).floor().max(1.0) as usize;
    let rows = count.div_ceil(cols);
    if mouse_in(r.x, r.y, r.w, r.h) && !popup_open() {
        let wh = wheel();
        if wh > 0.0 {
            *scroll = scroll.saturating_sub(1);
        } else if wh < 0.0 {
            *scroll += 1;
        }
    }
    *scroll = (*scroll).min(rows.saturating_sub(rows_fit));
    let mut out = None;
    for i in (*scroll * cols)..count.min((*scroll + rows_fit) * cols) {
        let k = i - *scroll * cols;
        let cr = Rect::new(r.x + (k % cols) as f32 * (cell + 4.0), r.y + (k / cols) as f32 * (cell + 4.0), cell, cell);
        let hover = mouse_in(cr.x, cr.y, cr.w, cr.h);
        draw_rectangle(cr.x, cr.y, cr.w, cr.h, if hover { Color::new(0.3, 0.26, 0.2, 1.0) } else { Color::new(0.16, 0.15, 0.13, 1.0) });
        draw(i, cr);
        let sel = selected == Some(i);
        draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, if sel { 2.5 } else { 1.0 }, if sel { ACCENT } else { DIM });
        if hover && clicked() {
            out = Some(i);
        }
    }
    if rows > rows_fit {
        text(&trf!("wheel: more ({page}/{pages})", page = *scroll + 1, pages = rows - rows_fit + 1), r.x, r.bottom() + 14.0, 14.0, DIM);
    }
    out
}

/// Draws an atlas rectangle scaled to fit `cell`.
fn thumb(atlas_tex: &Texture2D, src: Rect, cell: Rect) {
    let k = ((cell.w - 4.0) / src.w).min((cell.h - 4.0) / src.h).min(1.5);
    let (w, h) = (src.w * k, src.h * k);
    draw_texture_ex(atlas_tex, cell.x + (cell.w - w) / 2.0, cell.y + (cell.h - h) / 2.0, WHITE, DrawTextureParams { dest_size: Some(vec2(w, h)), source: Some(src), ..Default::default() });
}

/// The whole palette column; changes the tool in `tools`.
pub fn tool_panel(state: &mut PaletteState, tools: &mut ToolState, palette: &Palette, art: Option<&DtArt>, r: Rect) {
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 1.0));
    let (x, w) = (r.x + 8.0, r.w - 16.0);
    let mut y = r.y + 6.0;
    let current = tool_index(&tools.tool);
    let bw = (w - 4.0) / 2.0;
    for (k, (label, _)) in TOOL_KEYS.iter().enumerate() {
        let (bx, by) = (x + (k % 2) as f32 * (bw + 4.0), y + (k / 2) as f32 * 30.0);
        if toggle_button(bx, by, bw, 26.0, tr(label), current == k) {
            tools.set_tool(state.tool(k));
        }
    }
    y += 4.0 * 30.0 + 6.0;
    draw_line(x, y, x + w, y, 1.0, DIM);
    y += 8.0;
    let atlas = art.and_then(|a| a.map_atlas());
    let mut changed = false;
    match tools.tool {
        Tool::Terrain { .. } => {
            text(tr("Surface"), x, y + 14.0, 17.0, ACCENT);
            y += 22.0;
            let (cw, ch) = ((w - 12.0) / 4.0, 36.0);
            for code in 0..16u8 {
                let (cx, cy) = (x + (code % 4) as f32 * (cw + 4.0), y + (code / 4) as f32 * (ch + 4.0));
                let dest = Rect::new(cx, cy, cw, ch);
                match art.and_then(|a| a.terrain(code)) {
                    Some(tex) => draw_wrapped(&tex, dest, vec2(0.0, 0.0), vec2(cw, ch)),
                    None => draw_rectangle(cx, cy, cw, ch, surface_color(code)),
                }
                let sel = state.code == code;
                let hover = mouse_in(cx, cy, cw, ch);
                draw_rectangle_lines(cx, cy, cw, ch, if sel { 3.0 } else { 1.0 }, if sel { ACCENT } else if hover { INK } else { DIM });
                if hover && clicked() {
                    state.code = code;
                    changed = true;
                }
            }
            y += 4.0 * (ch + 4.0) + 2.0;
            text_fit(tr(palette::SURFACE_LABELS[state.code as usize & 15]), x, y + 14.0, w, 17.0, INK);
            y += 26.0;
            text(tr("Brush"), x, y + 14.0, 17.0, ACCENT);
            y += 22.0;
            let size = match state.shape {
                TerrainShape::Brush(s) => s,
                _ => 0,
            };
            if let Some(s) = size_buttons(x, y, w, size) {
                state.shape = TerrainShape::Brush(s);
                changed = true;
            }
            y += 30.0;
            if toggle_button(x, y, bw, 26.0, tr("Flood fill"), state.shape == TerrainShape::Fill) {
                state.shape = TerrainShape::Fill;
                changed = true;
            }
            if toggle_button(x + bw + 4.0, y, bw, 26.0, tr("Rectangle"), state.shape == TerrainShape::Rect) {
                state.shape = TerrainShape::Rect;
                changed = true;
            }
        }
        Tool::Objects { .. } => {
            let classes: Vec<(i64, String)> = palette.classes().into_iter().map(|c| (c as i64, object_class_label(c))).collect();
            if let Some(c) = dropdown("palette:class", x, y, w, state.class as i64, &classes) {
                state.class = c as u8;
                state.scroll = 0;
                if let Some(o) = palette.sprites_of(state.class).next() {
                    state.sprite = o.sprite;
                }
                changed = true;
            }
            y += 30.0;
            text_fit(tr("Brush"), x, y + 14.0, 56.0, 16.0, DIM);
            if let Some(s) = size_buttons(x + 60.0, y, w - 60.0, state.size) {
                state.size = s;
                changed = true;
            }
            y += 32.0;
            let sprites: Vec<_> = palette.sprites_of(state.class).collect();
            let sel = sprites.iter().position(|o| o.sprite == state.sprite);
            let grid = Rect::new(x, y, w, r.bottom() - y - 40.0);
            let draw = |i: usize, cell: Rect| {
                let o = sprites[i];
                match atlas.and_then(|a| Some((a, a.decoration(o.class, o.sprite)?))) {
                    Some((a, src)) => thumb(&a.texture, src, cell),
                    None => text_centered(&o.sprite.to_string(), cell.center().x, cell.center().y + 6.0, 16.0, INK),
                }
            };
            if let Some(i) = sprite_grid(grid, sprites.len(), 62.0, &mut state.scroll, sel, &draw) {
                state.sprite = sprites[i].sprite;
                changed = true;
            }
            let label = format!("{} {}", object_class_label(state.class), state.sprite);
            text(&label, x, r.bottom() - 10.0, 16.0, INK);
        }
        Tool::Erase { .. } => {
            text(tr("Brush"), x, y + 14.0, 17.0, ACCENT);
            y += 22.0;
            if let Some(s) = size_buttons(x, y, w, state.erase_size) {
                state.erase_size = s;
                changed = true;
            }
            y += 34.0;
            text(tr("Erase"), x, y + 14.0, 17.0, ACCENT);
            y += 22.0;
            for (f, label) in [(ObjectFilter::All, tr("Everything")), (ObjectFilter::Massifs, tr("Hills, mountains, stones")), (ObjectFilter::Plants, tr("Trees and thickets"))] {
                if toggle_button(x, y, w, 26.0, label, state.filter == f) {
                    state.filter = f;
                    changed = true;
                }
                y += 30.0;
            }
            note(tr("Buildings, armies and points: select them and press Delete."), x, y, w, DIM);
        }
        Tool::Building { .. } => {
            let mut kinds: Vec<u8> = palette.buildings.iter().map(|b| b.picture_type).collect();
            kinds.dedup();
            let options: Vec<(i64, String)> = kinds.iter().map(|&k| (k as i64, building_type_label(k).to_string())).collect();
            if let Some(k) = dropdown("palette:kind", x, y, w, state.kind as i64, &options) {
                state.kind = k as u8;
                state.variant = palette.pictures_of(state.kind).next().map_or(0, |p| p.variant);
                state.scroll = 0;
                changed = true;
            }
            y += 32.0;
            let pics: Vec<_> = palette.pictures_of(state.kind).collect();
            let sel = pics.iter().position(|p| p.variant == state.variant);
            let grid = Rect::new(x, y, w, r.bottom() - y - 40.0);
            let draw = |i: usize, cell: Rect| {
                let p = pics[i];
                if let Some((a, src)) = atlas.and_then(|a| Some((a, a.building(p.picture_type, p.variant)?))) {
                    thumb(&a.texture, src, cell);
                }
                text(&format!("{}x{}", p.size.0, p.size.1), cell.x + 3.0, cell.bottom() - 4.0, 14.0, INK);
            };
            if let Some(i) = sprite_grid(grid, pics.len(), 84.0, &mut state.scroll, sel, &draw) {
                state.variant = pics[i].variant;
                changed = true;
            }
            text_fit(tr("Click: the cell is the bottom-right corner."), x, r.bottom() - 10.0, w, 14.0, DIM);
        }
        Tool::Point { .. } => {
            // The original's three kinds of point.
            let kinds = [(8, tr("Lantern (reveals an area)")), (9, tr("Event point")), (10, tr("AI target point"))];
            for (i, (m, label)) in kinds.iter().enumerate() {
                if toggle_button(x, y + i as f32 * 30.0, w, 26.0, label, state.point_model == *m) {
                    state.point_model = *m;
                    changed = true;
                }
            }
            note(tr("Click the map to place one."), x, y + 94.0, w, DIM);
        }
        Tool::Army { .. } => {
            // The original's army menu: the model of the new army.
            let models = [(4, palette::ARMY_MODELS[3].1), (5, palette::ARMY_MODELS[4].1), (6, palette::ARMY_MODELS[5].1), (7, palette::ARMY_MODELS[6].1)];
            for (i, (m, label)) in models.iter().enumerate() {
                if toggle_button(x, y + i as f32 * 30.0, w, 26.0, tr(label), state.army_model == *m) {
                    state.army_model = *m;
                    changed = true;
                }
            }
            note(tr("Click the map to place an army; set it up in its panel."), x, y + 124.0, w, DIM);
        }
        Tool::HeroStart(k) => {
            note(&trf!("Click the start cell of the {class}.", class = tr(palette::HERO_CLASSES[k])), x, y, w, ACCENT);
        }
        Tool::Select => {
            let mut ny = y;
            for para in [
                tr("Click a building, army or point to edit it; drag to move it. Delete: remove it."),
                tr("Wheel: zoom. Right or middle drag, or arrow keys: move the view. G grid, H hill cover, R patrols, Home: whole map."),
            ] {
                ny += note(para, x, ny, w, DIM) + 18.0;
            }
        }
    }
    if changed {
        let k = tool_index(&tools.tool);
        tools.set_tool(state.tool(k));
    }
}
