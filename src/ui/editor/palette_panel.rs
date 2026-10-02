//! The tool panel on the right, as the original editor's (docs/reference/editor/main-window.md
//! §2, §4): the five tool pages, the size, delete and Info buttons, the move mode and the two
//! mode check boxes, then the page's palette.

use macroquad::prelude::*;

use razdor::editor::palette::{self, building_type_label, object_class_label, Palette};
use razdor::editor::tools::{palette_cells, PaletteCells, DELETE, INFO, MAX_SIZE};
use razdor::editor::{Page, TerrainShape, ToolState};
use razdor::i18n::{n_, tr};
use razdor::trf;

use crate::ui::dt_art::DtArt;
use crate::ui::widgets::*;
use crate::ui::world_view::{draw_wrapped, surface_color};

/// The palette's scroll, in rows.
#[derive(Default)]
pub struct PaletteState {
    pub scroll: usize,
}

/// The pages, in the original's order.
pub const PAGES: [(Page, &str); 5] = [(Page::Terrain, n_("Terrain")), (Page::Hills, n_("Hills")), (Page::Forests, n_("Forests")), (Page::Buildings, n_("Buildings")), (Page::Items, n_("Items"))];

/// The items page's nine kinds (1–9).
pub const ITEMS: [&str; 9] = [
    n_("Knight's start"),
    n_("Archmage's start"),
    n_("Ranger's start"),
    n_("Feudal army"),
    n_("Robber band"),
    n_("Peasant crowd"),
    n_("Inactive army"),
    n_("Lantern"),
    n_("Event point"),
];

/// What the panel asks the window for.
#[derive(Default)]
pub struct PanelAction {
    /// "Burn everything".
    pub burn: bool,
}

/// A grid of palette cells; returns the index clicked. `cell` px squares, scrolled by
/// `scroll` rows with the wheel over it.
fn grid(r: Rect, count: usize, cell: f32, scroll: &mut usize, selected: Option<usize>, draw: &dyn Fn(usize, Rect)) -> Option<usize> {
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

/// The label of a size, delete or Info button.
fn brush_label(b: i32) -> String {
    match b {
        DELETE => tr("Del").to_string(),
        INFO => tr("Info").to_string(),
        n => n.to_string(),
    }
}

/// The whole tool column.
pub fn tool_panel(state: &mut PaletteState, tools: &mut ToolState, palette: &Palette, art: Option<&DtArt>, r: Rect) -> PanelAction {
    let mut action = PanelAction::default();
    draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.1, 0.095, 0.09, 1.0));
    let (x, w) = (r.x + 8.0, r.w - 16.0);
    let mut y = r.y + 6.0;
    // The five pages.
    let pw = (w - 4.0 * 3.0) / 5.0;
    for (k, (page, label)) in PAGES.iter().enumerate() {
        if toggle_button(x + k as f32 * (pw + 3.0), y, pw, 26.0, tr(label), tools.page == *page) && tools.page != *page {
            tools.choose_page(*page, palette);
            state.scroll = 0;
        }
    }
    y += 32.0;
    // Sizes 1–6, delete, Info.
    let brushes: Vec<i32> = (1..=MAX_SIZE).chain([DELETE, INFO]).collect();
    let bw = (w - 3.0 * (brushes.len() - 1) as f32) / brushes.len() as f32;
    for (k, b) in brushes.iter().enumerate() {
        let bx = x + k as f32 * (bw + 3.0);
        let label = brush_label(*b);
        let on = tools.brush == *b && tools.held.is_none();
        let hit = if tools.size_enabled(*b) { toggle_button(bx, y, bw, 26.0, &label, on) } else { small_button(bx, y, bw, 26.0, &label, false) };
        if hit {
            tools.press_size(*b);
            state.scroll = 0;
        }
    }
    y += 32.0;
    if toggle_button(x, y, 70.0, 24.0, tr("Move"), tools.move_mode) && !tools.move_mode {
        tools.start_move();
    }
    if let Some(on) = checkbox(x + 78.0, y + 1.0, w - 78.0, tr("Tree replacement"), tools.tree_replace) {
        tools.tree_replace = on;
    }
    y += 28.0;
    if checkbox(x + 78.0, y, w - 78.0, tr("Ignore mountains"), tools.ignore_mountains).is_some() {
        tools.toggle_ignore_mountains();
    }
    y += 28.0;
    draw_line(x, y, x + w, y, 1.0, DIM);
    y += 8.0;
    let atlas = art.and_then(|a| a.map_atlas());
    // Room at the foot for the selection's name, the burn button and the playability line.
    let bottom = r.bottom() - 64.0;
    match palette_cells(tools, palette) {
        PaletteCells::Terrain => {
            let (cw, ch) = ((w - 12.0) / 4.0, 36.0);
            for code in 0..16u8 {
                let (cx, cy) = (x + (code % 4) as f32 * (cw + 4.0), y + (code / 4) as f32 * (ch + 4.0));
                let dest = Rect::new(cx, cy, cw, ch);
                match art.and_then(|a| a.terrain(code)) {
                    Some(tex) => draw_wrapped(&tex, dest, vec2(0.0, 0.0), vec2(cw, ch)),
                    None => draw_rectangle(cx, cy, cw, ch, surface_color(code)),
                }
                let sel = tools.terrain == code;
                let hover = mouse_in(cx, cy, cw, ch);
                draw_rectangle_lines(cx, cy, cw, ch, if sel { 3.0 } else { 1.0 }, if sel { ACCENT } else if hover { INK } else { DIM });
                if hover && clicked() {
                    tools.click_palette(code as usize, palette);
                }
            }
            y += 4.0 * (ch + 4.0) + 2.0;
            text_fit(tr(palette::SURFACE_LABELS[tools.terrain as usize & 15]), x, y + 14.0, w, 17.0, INK);
            y += 26.0;
            // Razdor's own shapes besides the original's brush.
            let sw = (w - 8.0) / 3.0;
            for (k, (shape, label)) in [(TerrainShape::Brush, tr("Brush")), (TerrainShape::Fill, tr("Flood fill")), (TerrainShape::Rect, tr("Rectangle"))].into_iter().enumerate() {
                if toggle_button(x + k as f32 * (sw + 4.0), y, sw, 26.0, label, tools.shape == shape) {
                    tools.shape = shape;
                }
            }
        }
        PaletteCells::Objects(list) => {
            let sel = list.iter().position(|o| Some(*o) == if tools.page == Page::Hills { tools.hill } else { tools.forest });
            let area = Rect::new(x, y, w, bottom - y - 36.0);
            let draw = |i: usize, cell: Rect| {
                let o = list[i];
                match atlas.and_then(|a| Some((a, a.decoration(o.class, o.sprite)?))) {
                    Some((a, src)) => thumb(&a.texture, src, cell),
                    None => text_centered(&o.sprite.to_string(), cell.center().x, cell.center().y + 6.0, 16.0, INK),
                }
            };
            if let Some(i) = grid(area, list.len(), 62.0, &mut state.scroll, sel, &draw) {
                tools.click_palette(i, palette);
            }
            if list.is_empty() {
                text_fit(tr("Nothing of this size."), x, y + 16.0, w, 15.0, DIM);
            }
            let chosen = if tools.page == Page::Hills { tools.hill } else { tools.forest };
            if let Some(o) = chosen {
                text_fit(&format!("{} {}", object_class_label(o.class), o.sprite), x, bottom - 4.0, w, 16.0, INK);
            }
        }
        PaletteCells::Buildings(pics) => {
            let sel = tools.building.and_then(|b| pics.iter().position(|p| (p.picture_type, p.variant) == (b.picture_type, b.variant)));
            let area = Rect::new(x, y, w, bottom - y - 36.0);
            let draw = |i: usize, cell: Rect| {
                let p = pics[i];
                if let Some((a, src)) = atlas.and_then(|a| Some((a, a.building(p.picture_type, p.variant)?))) {
                    thumb(&a.texture, src, cell);
                }
                text(&format!("{}x{}", p.size.0, p.size.1), cell.x + 3.0, cell.bottom() - 4.0, 14.0, INK);
            };
            if let Some(i) = grid(area, pics.len(), 84.0, &mut state.scroll, sel, &draw) {
                tools.click_palette(i, palette);
            }
            if let Some(b) = tools.building {
                text_fit(building_type_label(b.picture_type), x, bottom - 4.0, w, 16.0, INK);
            }
        }
        PaletteCells::Items => {
            let (cw, ch) = ((w - 8.0) / 3.0, 40.0);
            for (k, label) in ITEMS.iter().enumerate() {
                let (cx, cy) = (x + (k % 3) as f32 * (cw + 4.0), y + (k / 3) as f32 * (ch + 4.0));
                if toggle_button(cx, cy, cw, ch, tr(label), tools.item == k as u8 + 1) {
                    tools.click_palette(k, palette);
                }
            }
        }
    }
    if small_button(x, r.bottom() - 58.0, w, 24.0, tr("Burn everything"), true) {
        action.burn = true;
    }
    action
}
