//! Ground textures (spec §3.6): the LTEX table, the per-cell VTEX block order and the TGA writer.
//!
//! LTEX index = surface code for codes 0..=15; index 16 is rock (mountains and the border ridge),
//! 17 is packed dirt (building footprints). A VTEX value is the LTEX index + 1; 0 is the default.

/// One LTEX entry: a DT surface (or an extra one) with its flat colour.
pub struct Surface {
    pub name: &'static str,
    pub rgb: [u8; 3],
}

/// The 18 LTEX entries, indices 0..=17. Names follow the JSON legend (`shallows_fords`, …).
pub const SURFACES: [Surface; 18] = [
    Surface { name: "shallows_fords", rgb: [70, 130, 170] },
    Surface { name: "coastal_water", rgb: [40, 90, 150] },
    Surface { name: "deep_sea", rgb: [20, 50, 110] },
    Surface { name: "lava_fields", rgb: [200, 60, 20] },
    Surface { name: "road", rgb: [130, 95, 60] },
    Surface { name: "grass_lowland", rgb: [80, 120, 50] },
    Surface { name: "grass_plain", rgb: [100, 140, 60] },
    Surface { name: "dry_plain", rgb: [150, 140, 80] },
    Surface { name: "marsh", rgb: [70, 90, 60] },
    Surface { name: "impassable_swamp", rgb: [50, 60, 40] },
    Surface { name: "sand_dunes", rgb: [210, 190, 130] },
    Surface { name: "clay_soil", rgb: [150, 100, 70] },
    Surface { name: "stony_soil", rgb: [120, 115, 105] },
    Surface { name: "scorched_land", rgb: [60, 45, 40] },
    Surface { name: "snowy_ground", rgb: [230, 235, 240] },
    Surface { name: "impassable_snowdrifts", rgb: [245, 250, 255] },
    Surface { name: "rock", rgb: [110, 105, 100] },
    Surface { name: "dirt", rgb: [120, 90, 55] },
];

/// LTEX index of rock.
pub const ROCK: u8 = 16;
/// LTEX index of packed dirt.
pub const DIRT: u8 = 17;
/// Surface code of deep sea: the texels and tiles outside the map use it.
pub const DEEP_SEA: u8 = 2;
/// Texture size in texels (square, flat colour).
pub const TEXTURE_SIZE: u16 = 64;

/// The LTEX record id of entry `index`.
pub fn ltex_id(index: usize) -> String {
    format!("rz_{}", SURFACES[index].name)
}

/// The LTEX DATA path of entry `index`, relative to `textures/`.
pub fn texture_path(index: usize) -> String {
    format!("razdor/surf_{}.tga", SURFACES[index].name)
}

/// Orders one cell's 16x16 texels (`texels[row][col]`, row 0 = south, values = LTEX index) into
/// the 256 VTEX values of the file. The file keeps 4x4 blocks of 4x4 (spec §2.2): the value at
/// position `(y1·4 + x1)·16 + (y2·4 + x2)` belongs to row `y1·4 + y2`, column `x1·4 + x2`.
/// The stored value is the LTEX index + 1.
pub fn vtex(texels: &[[u8; 16]; 16]) -> [u16; 256] {
    let mut out = [0u16; 256];
    for (row, line) in texels.iter().enumerate() {
        for (col, &index) in line.iter().enumerate() {
            out[file_position(row, col)] = u16::from(index) + 1;
        }
    }
    out
}

/// The position in the VTEX array of texel `(row, col)`.
pub fn file_position(row: usize, col: usize) -> usize {
    let (y1, y2) = (row / 4, row % 4);
    let (x1, x2) = (col / 4, col % 4);
    (y1 * 4 + x1) * 16 + (y2 * 4 + x2)
}

/// An uncompressed 24-bit TGA of one flat colour (top-left origin).
pub fn tga(size: u16, rgb: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(18 + usize::from(size) * usize::from(size) * 3);
    out.extend_from_slice(&[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&size.to_le_bytes());
    out.extend_from_slice(&size.to_le_bytes());
    out.push(24);
    out.push(0x20);
    for _ in 0..(usize::from(size) * usize::from(size)) {
        out.extend_from_slice(&[rgb[2], rgb[1], rgb[0]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_has_18_entries_with_the_surface_names() {
        assert_eq!(SURFACES.len(), 18);
        assert_eq!(SURFACES[0].name, "shallows_fords");
        assert_eq!(SURFACES[15].name, "impassable_snowdrifts");
        assert_eq!(SURFACES[usize::from(ROCK)].name, "rock");
        assert_eq!(SURFACES[usize::from(DIRT)].name, "dirt");
        assert_eq!(texture_path(2), "razdor/surf_deep_sea.tga");
        assert_eq!(ltex_id(4), "rz_road");
    }

    #[test]
    fn file_positions_match_the_block_rule() {
        // Row 1, column 0 is in the first block: y2 = 1, x2 = 0 -> 4.
        assert_eq!(file_position(1, 0), 4);
        // Row 0, column 4 starts the second block of the first row of blocks: 1 * 16 = 16.
        assert_eq!(file_position(0, 4), 16);
        // Row 1, column 4: block (y1 0, x1 1) at 16, inside it y2 1, x2 0 -> 20.
        assert_eq!(file_position(1, 4), 20);
        // Last texel.
        assert_eq!(file_position(15, 15), 255);
        // Row 4, column 0 starts the third block (y1 1, x1 0) at 64.
        assert_eq!(file_position(4, 0), 64);
    }

    #[test]
    fn vtex_writes_the_texels_in_block_order() {
        // texels[row][col] = index (row * 16 + col) % 18, so each value is easy to place by hand.
        let mut texels = [[0u8; 16]; 16];
        for (row, line) in texels.iter_mut().enumerate() {
            for (col, v) in line.iter_mut().enumerate() {
                *v = ((row * 16 + col) % 18) as u8;
            }
        }
        let out = vtex(&texels);
        // Hand-written: file position -> (row, col) it must hold.
        let expected = [(0usize, 0usize, 0usize), (0, 1, 1), (1, 0, 4), (1, 1, 5), (0, 4, 16), (1, 4, 20), (15, 15, 255)];
        for (row, col, pos) in expected {
            let index = (row * 16 + col) % 18;
            assert_eq!(usize::from(out[pos]), index + 1, "row {row} col {col} at {pos}");
        }
        // Every texel appears exactly once.
        let mut seen = [false; 256];
        for row in 0..16 {
            for col in 0..16 {
                seen[file_position(row, col)] = true;
            }
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn a_tga_has_the_header_and_bgr_pixels() {
        let bytes = tga(4, [10, 20, 30]);
        assert_eq!(bytes.len(), 18 + 4 * 4 * 3);
        assert_eq!(bytes[2], 2); // uncompressed true colour
        assert_eq!(&bytes[12..16], &[4, 0, 4, 0]);
        assert_eq!(bytes[16], 24);
        assert_eq!(&bytes[18..21], &[30, 20, 10]);
    }
}
