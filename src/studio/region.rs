//! Copy and paste of a part of a map (its terrain cells and the tables inside it), and the stamp
//! library: a copied region saved in the package's `.editor/stamps/` (the engine ignores
//! `.editor/`). A paste is one undo step. Buildings and armies that are pasted get new ids; a
//! pasted army's named leader loses its name (a named character is one per package). The
//! hero's start is never copied.
//!
//! UX4 (spec §5.6): a copy takes a selection (a [`Mask`]) instead of a rectangle; the copy keeps
//! the selected terrain cells (`Clip::mask`) and the tables whose cell is selected. Copy, cut, paste
//! and clear act on a [`Target`]: the visible unlocked layers, and the cells a paint is limited to.

use std::fs;
use std::path::Path;

use super::brush::{cell_of_fragment, retarget};
use super::edit::{self, Edit, Kind, Locator};
use super::geometry::CellRect;
use super::guarded;
use super::place::unique_id;
use super::select::Mask;
use super::Workspace;
use crate::pack::terrain::{self, Palette, DEFAULT_PALETTE};

/// The terrain a cleared cell takes: the palette's Plain grass, the first choice of the brush (the
/// spec leaves the cleared ground open; this is the owner's to change).
pub const CLEARED: u8 = 6;

/// A copied region: its size, its terrain codes (row by row) and the tables inside it, each as
/// TOML with its `cell` relative to the region's top left corner. `mask` (a selection's copy)
/// tells which cells' terrain the clip holds; `None` is a whole rectangle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Clip {
    pub w: u32,
    pub h: u32,
    pub codes: Vec<u8>,
    pub tables: Vec<ClipTable>,
    pub mask: Option<Vec<bool>>,
}

/// One table of a clip: its kind and its TOML (header included).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClipTable {
    pub kind: Kind,
    pub text: String,
}

/// A saved stamp: its name and its clip.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Stamp {
    pub name: String,
    pub clip: Clip,
}

/// Where a copy, a paste or a clear acts: the terrain and the tables of each kind it may touch (the
/// visible, unlocked layers), and the cells it is limited to (`within`; `None` is every cell).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub terrain: bool,
    pub objects: bool,
    pub buildings: bool,
    pub armies: bool,
    pub points: bool,
    pub within: Option<Mask>,
}

impl Target {
    /// Every layer and every cell.
    pub fn all() -> Target {
        Target { terrain: true, objects: true, buildings: true, armies: true, points: true, within: None }
    }

    /// Whether a table of this kind is on a layer the target may change (the hero never is).
    pub fn allows(&self, kind: Kind) -> bool {
        match kind {
            Kind::Object => self.objects,
            Kind::Building => self.buildings,
            Kind::Army => self.armies,
            Kind::Point => self.points,
            Kind::Hero => false,
        }
    }

    /// Whether the cell is inside the cells the target is limited to.
    pub fn cell_ok(&self, x: i64, y: i64) -> bool {
        self.within.as_ref().is_none_or(|m| m.get(x, y))
    }
}

/// Copies the rectangle `rect` of map `map`: its terrain and the tables whose cell is inside it
/// (buildings by their bottom-right cell). The rectangle is clipped to the map; nothing inside it
/// is an error.
pub fn copy(ws: &mut Workspace, map: &str, rect: CellRect) -> Result<Clip, String> {
    let i = ws.open_terrain(map)?;
    let (mw, mh) = (ws.terrains[i].terrain.width, ws.terrains[i].terrain.height);
    let r = rect.clip(mw, mh).ok_or("the region is outside the map")?;
    let (w, h) = (r.width() as u32, r.height() as u32);
    let codes = ws.terrains[i].rect(r.x0 as u32, r.y0 as u32, w, h);
    let file = format!("maps/{map}/map.toml");
    let text = ws.text_of(&file)?;
    let mut tables = Vec::new();
    for (loc, range) in edit::spans(&file, &text)? {
        if loc.kind == Kind::Hero {
            continue;
        }
        let fragment = &text[range];
        let Some((x, y)) = cell_of_fragment(fragment) else { continue };
        if !r.contains(x as i32, y as i32) {
            continue;
        }
        let Some(text) = retarget(fragment, (x - i64::from(r.x0), y - i64::from(r.y0)), None) else { continue };
        tables.push(ClipTable { kind: loc.kind, text: tidy(&text) });
    }
    Ok(Clip { w, h, codes, tables, mask: None })
}

/// Copies a selection of map `map`: the bounding box of `sel`, the terrain of its selected cells
/// (when the target has the terrain layer) and the tables of the allowed layers whose cell is
/// selected. Nothing is changed in the map.
pub fn copy_selection(ws: &mut Workspace, map: &str, sel: &Mask, target: &Target) -> Result<Clip, String> {
    let bounds = sel.bounds().ok_or("nothing is selected")?;
    let i = ws.open_terrain(map)?;
    let (mw, mh) = (ws.terrains[i].terrain.width, ws.terrains[i].terrain.height);
    let r = bounds.clip(mw, mh).ok_or("the selection is outside the map")?;
    let (w, h) = (r.width() as u32, r.height() as u32);
    let codes = ws.terrains[i].rect(r.x0 as u32, r.y0 as u32, w, h);
    let mut mask = Vec::with_capacity(codes.len());
    for dy in 0..h {
        for dx in 0..w {
            let (x, y) = (i64::from(r.x0) + i64::from(dx), i64::from(r.y0) + i64::from(dy));
            mask.push(target.terrain && sel.get(x, y));
        }
    }
    let file = format!("maps/{map}/map.toml");
    let text = ws.text_of(&file)?;
    let mut tables = Vec::new();
    for (loc, range) in edit::spans(&file, &text)? {
        if !target.allows(loc.kind) {
            continue;
        }
        let fragment = &text[range];
        let Some((x, y)) = cell_of_fragment(fragment) else { continue };
        if !sel.get(x, y) {
            continue;
        }
        let Some(text) = retarget(fragment, (x - i64::from(r.x0), y - i64::from(r.y0)), None) else { continue };
        tables.push(ClipTable { kind: loc.kind, text: tidy(&text) });
    }
    Ok(Clip { w, h, codes, tables, mask: Some(mask) })
}

/// Pastes `clip` with its top left corner on cell `at` of map `map`: the terrain is painted (cells
/// off the map are left out), and each table whose cell lands on the map is added with a new id
/// when it has one. Returns the number of tables added. One undo step; a failure undoes it all.
pub fn paste(ws: &mut Workspace, map: &str, clip: &Clip, at: (i64, i64)) -> Result<usize, String> {
    paste_to(ws, map, clip, at, &Target::all())
}

/// [`paste`] on the target's layers and cells: the terrain of a cell is pasted when the target has
/// the terrain layer, the cell is inside its `within` and the clip holds the cell's terrain; a table
/// is added when its layer is allowed and its cell is inside `within`.
pub fn paste_to(ws: &mut Workspace, map: &str, clip: &Clip, at: (i64, i64), target: &Target) -> Result<usize, String> {
    let file = format!("maps/{map}/map.toml");
    let i = ws.open_terrain(map)?;
    let (mw, mh) = (i64::from(ws.terrains[i].terrain.width), i64::from(ws.terrains[i].terrain.height));
    ws.begin_group();
    let result = (|| -> Result<usize, String> {
        let mut cells = Vec::with_capacity(clip.codes.len());
        if target.terrain {
            for y in 0..clip.h {
                for x in 0..clip.w {
                    let idx = (y * clip.w + x) as usize;
                    let (tx, ty) = (at.0 + i64::from(x), at.1 + i64::from(y));
                    if clip.mask.as_ref().is_some_and(|m| !m[idx]) || !target.cell_ok(tx, ty) {
                        continue;
                    }
                    cells.push((tx, ty, clip.codes[idx]));
                }
            }
        }
        ws.paint_codes(map, cells)?;
        let mut added = 0;
        for t in &clip.tables {
            if !target.allows(t.kind) {
                continue;
            }
            let Some((x, y)) = cell_of_fragment(&t.text) else { continue };
            let (cx, cy) = (at.0 + x, at.1 + y);
            if cx < 0 || cy < 0 || cx >= mw || cy >= mh || !target.cell_ok(cx, cy) {
                continue;
            }
            let taken: std::collections::BTreeSet<String> = ws.spans(&file)?.into_iter().filter(|(l, _)| l.kind == t.kind).filter_map(|(l, _)| l.id).collect();
            let word = match t.kind {
                Kind::Building => field(&t.text, "type").unwrap_or_else(|| "building".into()),
                Kind::Army => "army".into(),
                _ => "point".into(),
            };
            let has_id = field(&t.text, "id").is_some();
            let id = has_id.then(|| unique_id(&word, &taken));
            let Some(fragment) = retarget(&t.text, (cx, cy), id.as_deref()) else { continue };
            ws.apply(Edit::Insert { at: Locator { file: file.clone(), kind: t.kind, id: None, index: 0 }, fragment })?;
            added += 1;
        }
        Ok(added)
    })();
    match result {
        Ok(added) => {
            ws.end_group("paste");
            Ok(added)
        }
        Err(e) => {
            ws.cancel_group();
            Err(e)
        }
    }
}

/// Clears the target's part of the selection `sel` of map `map`, as one undo step named `label`:
/// the selected terrain cells become [`CLEARED`] (when the target has the terrain layer), and the
/// tables of the allowed layers whose cell is selected are removed. Returns the number of tables
/// removed. The hero is never removed.
pub fn clear(ws: &mut Workspace, map: &str, sel: &Mask, target: &Target, label: &str) -> Result<usize, String> {
    ws.begin_group();
    match clear_inner(ws, map, sel, target) {
        Ok(n) => {
            ws.end_group(label);
            Ok(n)
        }
        Err(e) => {
            ws.cancel_group();
            Err(e)
        }
    }
}

/// Copies the selection (see [`copy_selection`]) and clears it on the target's layers, as one undo
/// step named `cut`. The copy is returned; a failure changes nothing.
pub fn cut(ws: &mut Workspace, map: &str, sel: &Mask, target: &Target) -> Result<Clip, String> {
    let clip = copy_selection(ws, map, sel, target)?;
    ws.begin_group();
    match clear_inner(ws, map, sel, target) {
        Ok(_) => {
            ws.end_group("cut");
            Ok(clip)
        }
        Err(e) => {
            ws.cancel_group();
            Err(e)
        }
    }
}

/// [`clear`]'s changes, inside the group the caller opened.
fn clear_inner(ws: &mut Workspace, map: &str, sel: &Mask, target: &Target) -> Result<usize, String> {
    let file = format!("maps/{map}/map.toml");
    if target.terrain {
        let cells: Vec<(i64, i64, u8)> = sel.cells().into_iter().filter(|&(x, y)| target.cell_ok(x, y)).map(|(x, y)| (x, y, CLEARED)).collect();
        if !cells.is_empty() {
            ws.paint_codes(map, cells)?;
        }
    }
    let text = ws.text_of(&file)?;
    // Removed last to first in the file, so each table's position is still right when it is removed.
    let mut doomed: Vec<(usize, Locator)> = Vec::new();
    for (loc, range) in edit::spans(&file, &text)? {
        if !target.allows(loc.kind) {
            continue;
        }
        let Some((x, y)) = cell_of_fragment(&text[range.clone()]) else { continue };
        if sel.get(x, y) && target.cell_ok(x, y) {
            doomed.push((range.start, loc));
        }
    }
    doomed.sort_by_key(|d| std::cmp::Reverse(d.0));
    let n = doomed.len();
    for (_, at) in doomed {
        ws.apply(Edit::Remove { at })?;
    }
    Ok(n)
}

/// A table's text as the clips keep it: no trailing blank, one newline at its end (so a stamp
/// reads back as it was copied).
fn tidy(text: &str) -> String {
    format!("{}\n", text.trim_end())
}

/// The value of a string key of one table's TOML (`type = "village"`).
fn field(fragment: &str, key: &str) -> Option<String> {
    let t: toml::Table = fragment.parse().ok()?;
    let table = t.values().next()?.as_array()?.first()?.as_table()?;
    table.get(key)?.as_str().map(str::to_string)
}

/// Whether `name` is a stamp's name: 1 to 40 letters, digits, spaces, `-` or `_` (no path parts).
pub fn valid_name(name: &str) -> bool {
    let n = name.trim();
    !n.is_empty() && n.chars().count() <= 40 && n.chars().all(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
}

/// The folder the stamps live in: `.editor/stamps` of the package.
pub fn stamps_dir(root: &Path) -> std::path::PathBuf {
    root.join(".editor").join("stamps")
}

/// Saves `clip` as stamp `name` (`<name>.toml` with its tables, `<name>.png` with its terrain), and
/// returns the stamp's path. A name that exists is replaced. A selection's clip keeps its mask
/// (`mask = ["##.", …]`, one row of text per row of cells).
pub fn save_stamp(root: &Path, name: &str, clip: &Clip) -> Result<(), String> {
    let name = name.trim();
    if !valid_name(name) {
        return Err(format!("{name:?} is not a stamp name: letters, digits, spaces, - and _ (up to 40)"));
    }
    if clip.codes.len() != (clip.w * clip.h) as usize {
        return Err("the stamp's terrain does not match its size".into());
    }
    let mut text = format!("# A stamp of Razdor Studio: a copied part of a map. The engine ignores .editor/.\nwidth = {}\nheight = {}\n", clip.w, clip.h);
    if let Some(mask) = &clip.mask {
        let rows: Vec<String> = (0..clip.h)
            .map(|y| (0..clip.w).map(|x| if mask[(y * clip.w + x) as usize] { '#' } else { '.' }).collect::<String>())
            .map(|row| format!("{row:?}"))
            .collect();
        text.push_str(&format!("mask = [{}]\n", rows.join(", ")));
    }
    for t in &clip.tables {
        text.push('\n');
        text.push_str(&t.text);
        if !t.text.ends_with('\n') {
            text.push('\n');
        }
    }
    // Any size: a stamp is not a map (the map pictures are 50, 100 or 200 cells square). Written
    // through `studio::guarded`: a link on the way, or a link as the target, is refused.
    let png = terrain::encode_rgb(clip.w, clip.h, |x, y| DEFAULT_PALETTE[usize::from(clip.codes[(y * clip.w + x) as usize]).min(15)].1);
    guarded::write(root, &format!("stamps/{name}.png"), &png).map_err(|e| format!("cannot write the stamp's picture: {e}"))?;
    guarded::write(root, &format!("stamps/{name}.toml"), text.as_bytes()).map_err(|e| format!("cannot write the stamp: {e}"))?;
    Ok(())
}

/// The stamps of the package, by name. A stamp whose files do not read is left out.
pub fn list_stamps(root: &Path) -> Vec<Stamp> {
    // A stamps folder that is a link (or has one on the way) lists nothing.
    if !guarded::real_folder(root, "stamps") {
        return Vec::new();
    }
    let dir = stamps_dir(root);
    let Ok(read) = fs::read_dir(&dir) else { return Vec::new() };
    let mut names: Vec<String> = read
        .flatten()
        .filter_map(|e| {
            let p = e.path();
            (p.extension().is_some_and(|x| x == "toml") && p.is_file()).then(|| p.file_stem()?.to_str().map(str::to_string))?
        })
        .collect();
    names.sort();
    names.into_iter().filter_map(|name| read_stamp(root, &name)).collect()
}

fn read_stamp(root: &Path, name: &str) -> Option<Stamp> {
    let text = String::from_utf8(guarded::read(root, &format!("stamps/{name}.toml"))?).ok()?;
    let table: toml::Table = text.parse().ok()?;
    let w = u32::try_from(table.get("width")?.as_integer()?).ok()?;
    let h = u32::try_from(table.get("height")?.as_integer()?).ok()?;
    let mask = match table.get("mask") {
        None => None,
        Some(rows) => Some(read_mask(rows, w, h)?),
    };
    let bytes = guarded::read(root, &format!("stamps/{name}.png"))?;
    let codes = decode_codes(&bytes, w, h)?;
    let tables = edit::spans(name, &text)
        .ok()?
        .into_iter()
        .filter(|(l, _)| l.kind != Kind::Hero)
        .map(|(l, r)| ClipTable { kind: l.kind, text: tidy(&text[r]) })
        .collect();
    Some(Stamp { name: name.to_string(), clip: Clip { w, h, codes, tables, mask } })
}

/// A stamp's mask: `h` rows of `w` characters, `#` for a copied cell and `.` for one left out.
fn read_mask(rows: &toml::Value, w: u32, h: u32) -> Option<Vec<bool>> {
    let rows = rows.as_array()?;
    if rows.len() != h as usize {
        return None;
    }
    let mut bits = Vec::with_capacity((w * h) as usize);
    for row in rows {
        let s = row.as_str()?;
        if s.chars().count() != w as usize {
            return None;
        }
        bits.extend(s.chars().map(|c| c == '#'));
    }
    Some(bits)
}

/// The terrain codes of a stamp's picture (`w` by `h`, RGB in the default palette's colours); `None`
/// when the picture is not that or a pixel is no terrain colour.
fn decode_codes(bytes: &[u8], w: u32, h: u32) -> Option<Vec<u8>> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).ok()?;
    if (info.width, info.height) != (w, h) || info.color_type != png::ColorType::Rgb || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    let palette = Palette::default();
    buf[..(w * h * 3) as usize].chunks_exact(3).map(|p| palette.code_of([p[0], p[1], p[2]])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_plain() {
        assert!(valid_name("Ford stone"));
        assert!(valid_name("башня_2"));
        assert!(!valid_name(""));
        assert!(!valid_name("../x"));
        assert!(!valid_name("a/b"));
        assert!(!valid_name(&"x".repeat(41)));
    }

    #[test]
    fn a_target_allows_its_layers_and_never_the_hero() {
        let mut t = Target::all();
        assert!(t.allows(Kind::Army) && t.allows(Kind::Object) && !t.allows(Kind::Hero));
        t.armies = false;
        assert!(!t.allows(Kind::Army) && t.cell_ok(0, 0));
        t.within = Some(Mask::of_rect(4, 4, CellRect { x0: 1, y0: 1, x1: 1, y1: 1 }));
        assert!(t.cell_ok(1, 1) && !t.cell_ok(0, 0));
    }
}
