//! The terrain document: a map's `terrain.png` held as its cells, painted with the terrain brush.
//!
//! The picture is decoded with the map's palette (`pack::terrain::decode`). It is dirty when its
//! cells differ from the cells on disk; a clean document hands out the bytes it was read with, so a
//! picture that nothing painted is never rewritten (byte for byte). A dirty one is encoded from
//! its cells (`pack::terrain::encode`), with the same palette.

use crate::pack::terrain::{self, Palette, Terrain};

/// One map's terrain, in memory.
#[derive(Clone, Debug)]
pub struct TerrainDoc {
    /// The map's id (`maps/<map>/terrain.png`).
    pub map: String,
    /// Package-relative path of the picture.
    pub path: String,
    pub palette: Palette,
    pub terrain: Terrain,
    /// The cells on disk (or as last saved): the document is dirty when `terrain.cells` differs.
    saved: Vec<u8>,
    /// The bytes read from disk (or written by the last save), kept while the terrain is clean.
    original: Vec<u8>,
}

impl TerrainDoc {
    /// Decodes a picture read from `maps/<map>/terrain.png`. An error lists the picture's problems.
    pub fn open(map: &str, bytes: Vec<u8>, palette: Palette) -> Result<TerrainDoc, String> {
        let terrain = terrain::decode(&bytes, &palette).map_err(|ps| {
            let msgs: Vec<String> = ps.iter().map(|p| p.message.clone()).collect();
            format!("maps/{map}/terrain.png cannot be painted: {}", msgs.join("; "))
        })?;
        Ok(TerrainDoc {
            map: map.to_string(),
            path: format!("maps/{map}/terrain.png"),
            palette,
            saved: terrain.cells.clone(),
            original: bytes,
            terrain,
        })
    }

    /// Whether the cells differ from the cells on disk.
    pub fn is_dirty(&self) -> bool {
        self.terrain.cells != self.saved
    }

    /// The code of cell `(x, y)`, `None` outside the map.
    pub fn code_at(&self, x: u32, y: u32) -> Option<u8> {
        (x < self.terrain.width && y < self.terrain.height).then(|| self.terrain.cells[(y * self.terrain.width + x) as usize])
    }

    /// Sets cell `(x, y)` to `code` (the caller checked the cell is on the map); returns the code
    /// it had.
    pub fn set(&mut self, x: u32, y: u32, code: u8) -> u8 {
        let i = (y * self.terrain.width + x) as usize;
        std::mem::replace(&mut self.terrain.cells[i], code)
    }

    /// The codes of a rectangle, row by row (cells outside the map read as 0).
    pub fn rect(&self, x: u32, y: u32, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::with_capacity((w * h) as usize);
        for yy in y..y + h {
            for xx in x..x + w {
                out.push(self.code_at(xx, yy).unwrap_or(0));
            }
        }
        out
    }

    /// Writes the codes of a rectangle back (undo and redo of a stroke).
    pub fn set_rect(&mut self, x: u32, y: u32, w: u32, h: u32, codes: &[u8]) {
        let mut it = codes.iter();
        for yy in y..y + h {
            for xx in x..x + w {
                if let Some(&c) = it.next() {
                    if xx < self.terrain.width && yy < self.terrain.height {
                        self.set(xx, yy, c);
                    }
                }
            }
        }
    }

    /// The file's bytes as they should be on disk now: the bytes read while clean, else the encoded
    /// cells.
    pub fn bytes(&self) -> Vec<u8> {
        if self.is_dirty() {
            terrain::encode(&self.terrain, &self.palette)
        } else {
            self.original.clone()
        }
    }

    /// The cells are on disk as `bytes` (a save wrote them).
    pub fn mark_saved(&mut self, bytes: Vec<u8>) {
        self.saved = self.terrain.cells.clone();
        self.original = bytes;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> TerrainDoc {
        let png = terrain::encode_rgb(50, 50, |_, _| terrain::DEFAULT_PALETTE[6].1);
        TerrainDoc::open("x", png, Palette::default()).unwrap()
    }

    #[test]
    fn a_clean_picture_hands_out_its_own_bytes() {
        let png = terrain::encode_rgb(50, 50, |x, _| if x < 25 { terrain::DEFAULT_PALETTE[1].1 } else { terrain::DEFAULT_PALETTE[6].1 });
        let d = TerrainDoc::open("x", png.clone(), Palette::default()).unwrap();
        assert!(!d.is_dirty());
        assert_eq!(d.bytes(), png, "an unpainted picture is never rewritten");
    }

    #[test]
    fn a_painted_cell_is_dirty_and_back_is_clean() {
        let mut d = doc();
        let before = d.set(3, 4, 4);
        assert_eq!(before, 6);
        assert!(d.is_dirty());
        d.set(3, 4, 6);
        assert!(!d.is_dirty(), "the cells equal the disk again");
    }

    #[test]
    fn painted_bytes_decode_to_the_painted_cells() {
        let mut d = doc();
        d.set(10, 20, 1);
        let bytes = d.bytes();
        let back = terrain::decode(&bytes, &Palette::default()).unwrap();
        assert_eq!(back.cells, d.terrain.cells);
        assert_eq!(back.cells[(20 * 50 + 10) as usize], 1);
    }

    #[test]
    fn a_rectangle_reads_and_writes_back() {
        let mut d = doc();
        let saved = d.rect(2, 2, 3, 2);
        d.set_rect(2, 2, 3, 2, &[1, 1, 1, 1, 1, 1]);
        assert_eq!(d.code_at(4, 3), Some(1));
        d.set_rect(2, 2, 3, 2, &saved);
        assert_eq!(d.rect(2, 2, 3, 2), saved);
        assert!(!d.is_dirty());
    }
}
