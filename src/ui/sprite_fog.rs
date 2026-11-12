//! The map sprites' fog as a texture (backlog 1605, step 4): one texel per map cell holding the visibility of
//! standing sprites on that cell, the war fog and the reveals composed as `Shade::at` had them. The sprite shader
//! reads it at a sprite's foot cell (`sharp::FOG_GLSL`), so a map sprite's vertices need not change when the fog
//! does; the CPU reads the same table for the cull (a sprite on a hidden cell is not drawn and not picked).
//!
//! The table is made again only when its inputs move: the shown darkness (`minimap::shown_stamp`, which moves with
//! every texel the ground's fog draws differently) or the reveals (`veil`, a key of the showings and whether each is
//! open). The texture is uploaded only when the table's values change.
//!
//! Encoding: 16 bits per cell, the visibility times 65535, in the texel's red (high byte) and green (low byte), alpha
//! 255. Nearest sampling on a `w` × `h` texture (no padding: GLES 2 takes any size with clamped edges and no mipmaps).

use std::cell::RefCell;

use macroquad::miniquad::TextureWrap;
use macroquad::prelude::*;
use razdor::local;
use razdor::rules::fog::Fog;
use razdor::rules::map::Tile;

use super::minimap;

/// The visibility of a standing sprite on a cell: 1 less the darkness the ground shows there (`dark`, 0 to 1),
/// eased by a smoothstep from 0.15 to 0.85, so an edge sprite is not ghostly for long and a deep one is gone.
pub fn visibility(dark: f32) -> f32 {
    let x = (1.0 - dark).clamp(0.0, 1.0);
    let t = ((x - 0.15) / 0.7).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// What the fog gives a map sprite whose foot is on a cell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cell {
    /// No fog applies (the fog is off, or the trailer's no-fog switch): the sprite is drawn as it is.
    Clear,
    /// The texture coordinate of the sprite's cell: the shader multiplies the sprite's alpha by the visibility there.
    At(Vec2),
    /// The cell's visibility is 0, or the foot is past the map's edge with no edge fog: the sprite is not drawn.
    Hidden,
}

/// The visibility of every cell of a `w` × `h` map, 16 bits each (row by row). Pure: no texture.
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    w: i32,
    h: i32,
    vis: Vec<u16>,
}

impl Table {
    /// The table from the shown darkness of each cell (`shown`, 0 lit … 255 black, row by row, unrounded; empty for
    /// none) and the reveals' darkness per cell (`reveal`, 0 to 1, row by row; missing cells count as 0). The darkness
    /// composes as `1 − (1 − fog)(1 − reveal)`, the same as `Shade::at` composed them.
    pub fn compose(w: i32, h: i32, shown: &[f32], reveal: &[f32]) -> Table {
        let n = (w.max(0) * h.max(0)) as usize;
        let vis = (0..n)
            .map(|i| {
                let fog = shown.get(i).map_or(0.0, |&v| v / 255.0);
                let dark = 1.0 - (1.0 - fog) * (1.0 - reveal.get(i).copied().unwrap_or(0.0));
                (visibility(dark) * 65535.0).round() as u16
            })
            .collect();
        Table { w, h, vis }
    }

    /// The map's size in cells.
    pub fn size(&self) -> (i32, i32) {
        (self.w, self.h)
    }

    /// The visibility of cell `c` (a cell of the map).
    pub fn vis(&self, c: Tile) -> u16 {
        self.vis[(c.1 * self.w + c.0) as usize]
    }

    /// What a sprite whose foot is on `t` takes: the cell, or the nearest edge cell when `extend` (the map goes on past
    /// its edges, «Продолжать карту за краем»), else hidden past the edge. A hidden cell draws nothing.
    pub fn at(&self, t: Tile, extend: bool) -> Cell {
        let (w, h) = (self.w, self.h);
        let inside = t.0 >= 0 && t.1 >= 0 && t.0 < w && t.1 < h;
        let on = if inside {
            Some(t)
        } else if extend {
            Some((t.0.clamp(0, w - 1), t.1.clamp(0, h - 1)))
        } else {
            None
        };
        let Some(c) = on else { return Cell::Hidden };
        if self.vis(c) == 0 {
            return Cell::Hidden;
        }
        Cell::At(vec2((c.0 as f32 + 0.5) / w as f32, (c.1 as f32 + 0.5) / h as f32))
    }

    /// The texels: the visibility's high byte in red, its low byte in green, opaque.
    pub fn texels(&self) -> Vec<u8> {
        self.vis.iter().flat_map(|&v| [(v >> 8) as u8, (v & 0xff) as u8, 0, 255]).collect()
    }
}

local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

#[derive(Default)]
struct State {
    /// The table of the last sync; None while the fog is off.
    table: Option<Table>,
    /// The shown darkness's stamp the table was made from.
    stamp: Option<u64>,
    /// The reveals the table's `reveal` came from: the veil's key and the map's size.
    reveal_key: Option<(u64, i32, i32)>,
    reveal: Vec<f32>,
    tex: Option<Texture2D>,
}

/// Brings the table up to date with the fog (`active`: the fog draws, else no table), the shown darkness and the
/// reveals. `veil` keys the reveals (a change of it makes the reveals again); `live`: a showing is opening, so its
/// reveal changes with every frame and the table is made again; `reveal` gives a cell's darkness from the reveals (0
/// for none).
pub fn sync(fog: &Fog, active: bool, veil: u64, live: bool, reveal: &dyn Fn(Tile) -> f32) {
    STATE.with_borrow_mut(|st| st.sync(fog, active, veil, live, reveal));
}

impl State {
    fn sync(&mut self, fog: &Fog, active: bool, veil: u64, live: bool, reveal: &dyn Fn(Tile) -> f32) {
        if !active || fog.w <= 0 || fog.h <= 0 {
            self.table = None;
            self.stamp = None;
            return;
        }
        let (w, h) = (fog.w, fog.h);
        if live || self.reveal_key != Some((veil, w, h)) {
            let n = (w * h) as usize;
            self.reveal = (0..n).map(|i| reveal((i as i32 % w, i as i32 / w))).collect();
            self.reveal_key = Some((veil, w, h));
            self.stamp = None;
        }
        let stamp = minimap::shown_stamp();
        if !live && self.stamp == Some(stamp) && self.table.as_ref().is_some_and(|t| t.size() == (w, h)) {
            return;
        }
        let table = minimap::with_shown_darkness(fog, |shown| Table::compose(w, h, shown, &self.reveal));
        if self.table.as_ref() != Some(&table) || self.tex.as_ref().is_none_or(|t| t.width() != w as f32 || t.height() != h as f32) {
            self.upload(&table);
        }
        self.table = Some(table);
        self.stamp = Some(stamp);
    }

    fn upload(&mut self, table: &Table) {
        let (w, h) = table.size();
        let bytes = table.texels();
        match self.tex.as_ref() {
            Some(tex) if tex.width() == w as f32 && tex.height() == h as f32 => tex.update_from_bytes(w as u32, h as u32, &bytes),
            _ => self.tex = Some(make_texture(w as u16, h as u16, &bytes)),
        }
        razdor::frame_probe::tex_rebuilt();
    }
}

/// A texture of the table: nearest sampling, clamped edges (the foot cells are inside it, and the shader never samples
/// past them).
fn make_texture(w: u16, h: u16, bytes: &[u8]) -> Texture2D {
    let tex = Texture2D::from_rgba8(w, h, bytes);
    tex.set_filter(FilterMode::Nearest);
    // SAFETY: called on the main thread while macroquad runs, as every texture call.
    let gl = unsafe { get_internal_gl() };
    gl.quad_context.texture_set_wrap(tex.raw_miniquad_id(), TextureWrap::Clamp, TextureWrap::Clamp);
    tex
}

/// The table's texture for the sprite shader (a 1×1 opaque white, full visibility, until the fog is synced).
pub fn texture() -> Texture2D {
    STATE.with_borrow_mut(|st| st.tex.get_or_insert_with(|| make_texture(1, 1, &[255, 255, 255, 255])).clone())
}

/// What the fog gives a sprite whose foot is on the table's cell `t` (see [`Table::at`]); `Clear` while no table is.
pub fn at(t: Tile, extend: bool) -> Cell {
    STATE.with_borrow(|st| match st.table.as_ref() {
        Some(table) => table.at(t, extend),
        None => Cell::Clear,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The table's texel decoded as the shader decodes it (`sharp::FOG_GLSL`): the visibility in 0..1.
    fn decode(red: u8, green: u8) -> f32 {
        (red as f32 * 256.0 + green as f32) / 65535.0
    }

    /// The CPU's shade as `Shade::at` had it, unrounded: the reference the table is held to.
    fn legacy(fog: f32, reveal: f32) -> f32 {
        visibility(1.0 - (1.0 - fog) * (1.0 - reveal))
    }

    /// The shown darkness of whole bytes, as the fog's cells hold it (0..255).
    fn shown(bytes: &[u8]) -> Vec<f32> {
        bytes.iter().map(|&b| b as f32).collect()
    }

    #[test]
    fn the_table_is_the_cpu_shade_to_the_16th_bit() {
        // Every darkness byte with a few reveals: the table's decoded value is the CPU's shade within a 16-bit step.
        for fog in 0..=255u8 {
            for &r in &[0.0f32, 0.3, 0.7, 1.0] {
                let texels = Table::compose(1, 1, &shown(&[fog]), &[r]).texels();
                let got = decode(texels[0], texels[1]);
                let want = legacy(fog as f32 / 255.0, r);
                assert!((got - want).abs() <= 1.0 / 65535.0 + 1e-6, "fog {fog}, reveal {r}: {got} vs {want}");
            }
        }
    }

    #[test]
    fn a_cell_fully_dark_is_hidden_and_a_lit_one_is_plain() {
        let t = Table::compose(2, 1, &shown(&[255, 0]), &[0.0, 0.0]);
        assert_eq!(t.at((0, 0), false), Cell::Hidden, "unexplored: gone");
        assert_eq!(t.at((1, 0), false), Cell::At(vec2(0.75, 0.5)), "lit: the cell's own texel");
    }

    #[test]
    fn a_reveal_keeps_the_darkness_it_draws_over_the_fog() {
        // The reveal's value is the darkness it still keeps (`Showing::alpha_at`): where it keeps the cell dark, the
        // cell stays dark, and a reveal of full darkness closes a lit cell, as `Shade::at` composed them.
        assert_eq!(Table::compose(1, 1, &shown(&[255]), &[0.0]).vis((0, 0)), 0, "fog dark, nothing opens it");
        assert_eq!(Table::compose(1, 1, &shown(&[255]), &[1.0]).vis((0, 0)), 0, "fog dark, the reveal keeps it dark");
        assert_eq!(Table::compose(1, 1, &shown(&[0]), &[1.0]).vis((0, 0)), 0, "lit, a reveal of full darkness closes it");
        assert_eq!(Table::compose(1, 1, &shown(&[0]), &[0.0]).vis((0, 0)), 65535, "lit, nothing over it: plain");
    }

    #[test]
    fn past_the_edge_hides_unless_the_map_goes_on_past_it() {
        let t = Table::compose(2, 2, &shown(&[0, 0, 0, 0]), &[]);
        assert_eq!(t.at((-1, 0), false), Cell::Hidden, "off the map: no fog there, the sprite stays hidden");
        assert_eq!(t.at((-1, 1), true), Cell::At(vec2(0.25, 0.75)), "extended: the edge cell's fog");
        assert_eq!(t.at((5, 5), true), Cell::At(vec2(0.75, 0.75)), "extended past the corner: the corner cell");
    }

    #[test]
    fn a_reveal_mid_run_changes_only_the_cells_it_opened() {
        // The fog before and after a cell opens: the other cells' visibility is the same bit for bit, so their
        // sprites appear exactly as before.
        let before = Table::compose(3, 1, &shown(&[255, 200, 255]), &[0.0; 3]);
        let after = Table::compose(3, 1, &shown(&[255, 0, 255]), &[0.0; 3]);
        assert_eq!(before.vis((0, 0)), after.vis((0, 0)));
        assert_eq!(before.vis((2, 0)), after.vis((2, 0)));
        assert_ne!(before.vis((1, 0)), after.vis((1, 0)), "the opened cell changes");
        assert_eq!(after.vis((1, 0)), 65535, "and is lit");
    }

    #[test]
    fn the_texels_decode_back_to_the_table() {
        let t = Table::compose(3, 2, &shown(&[0, 40, 80, 120, 160, 255]), &[0.1; 6]);
        let texels = t.texels();
        for i in 0..6 {
            let (x, y) = ((i % 3) as i32, (i / 3) as i32);
            let want = t.vis((x, y)) as f32 / 65535.0;
            assert!((decode(texels[i * 4], texels[i * 4 + 1]) - want).abs() < 1e-6);
            assert_eq!(texels[i * 4 + 3], 255);
        }
    }
}
