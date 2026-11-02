//! The terrain picture of a package map: `maps/<id>/terrain.png`, one colour per terrain kind.
//!
//! Each terrain kind is one of the engine's [`Surface`]s, named as in the palette table below.
//! A pixel's colour must be one of the palette's (exact RGB; a transparent pixel is no colour).
//! `map.toml` may change a kind's colour under `[terrain_palette]`; two kinds may not share one.
//!
//! The map's size is the picture's: 50, 100 or 200 cells on each side, the sizes the engine
//! loads (`docs/reference/dtm-format.md`).

use std::collections::BTreeMap;

use crate::dt::dtm::Surface;

/// The terrain kinds, in the order of the engine's codes, with their default colours.
pub const DEFAULT_PALETTE: [(&str, [u8; 3]); 16] = [
    ("shallows_fords", [0x8f, 0xd0, 0xe0]),
    ("coastal_water", [0x3a, 0x7f, 0xbf]),
    ("deep_sea", [0x1d, 0x4f, 0x8a]),
    ("lava_fields", [0xc8, 0x3a, 0x1a]),
    ("road", [0xb0, 0x88, 0x50]),
    ("grass_lowland", [0x9c, 0xc4, 0x6a]),
    ("grass_plain", [0x6f, 0xa8, 0x4a]),
    ("dry_plain", [0xc9, 0xb5, 0x6a]),
    ("marsh", [0x5f, 0x7a, 0x4a]),
    ("impassable_swamp", [0x3d, 0x4f, 0x32]),
    ("sand_dunes", [0xe6, 0xd2, 0x8a]),
    ("clay_soil", [0xa0, 0x70, 0x4a]),
    ("stony_soil", [0x8a, 0x8a, 0x80]),
    ("scorched_land", [0x4a, 0x3a, 0x33]),
    ("snowy_ground", [0xf0, 0xf4, 0xf8]),
    ("impassable_snowdrifts", [0xc8, 0xd8, 0xe8]),
];

/// The sizes a map may have on each side, in cells.
pub const MAP_SIZES: [u32; 3] = [50, 100, 200];

/// The name of a terrain kind in `map.toml` and in the palette.
pub fn surface_name(s: Surface) -> &'static str {
    DEFAULT_PALETTE[s as usize].0
}

/// The terrain kind named `name`.
pub fn surface_by_name(name: &str) -> Option<Surface> {
    DEFAULT_PALETTE.iter().position(|(n, _)| *n == name).and_then(|i| Surface::from_code(i as u8))
}

/// The colour of each terrain kind, by the engine's code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Palette {
    colours: [[u8; 3]; 16],
}

impl Default for Palette {
    fn default() -> Palette {
        Palette { colours: DEFAULT_PALETTE.map(|(_, c)| c) }
    }
}

/// `#rrggbb` (the `#` optional, case-insensitive) as RGB.
pub fn parse_hex(text: &str) -> Option<[u8; 3]> {
    let hex = text.strip_prefix('#').unwrap_or(text);
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let v = u32::from_str_radix(hex, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8])
}

/// `#rrggbb`, for messages.
pub fn hex(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

impl Palette {
    /// The default palette with the map's overrides. An error names the key or colour at fault.
    pub fn with_overrides(overrides: &BTreeMap<String, String>) -> Result<Palette, String> {
        let mut p = Palette::default();
        for (name, value) in overrides {
            let Some(s) = surface_by_name(name) else {
                return Err(format!("`{name}` is not a terrain kind: use one of {}", DEFAULT_PALETTE.map(|(n, _)| n).join(", ")));
            };
            let Some(rgb) = parse_hex(value) else {
                return Err(format!("`{value}` is not a colour: write it as #rrggbb, e.g. \"#60a040\""));
            };
            p.colours[s as usize] = rgb;
        }
        for (i, a) in p.colours.iter().enumerate() {
            for (j, b) in p.colours[..i].iter().enumerate() {
                if a == b {
                    let (x, y) = (DEFAULT_PALETTE[i].0, DEFAULT_PALETTE[j].0);
                    return Err(format!("{x} and {y} have the same colour {}: give each terrain kind its own", hex(*a)));
                }
            }
        }
        Ok(p)
    }

    /// The terrain code of an exact colour.
    pub fn code_of(&self, rgb: [u8; 3]) -> Option<u8> {
        self.colours.iter().position(|c| *c == rgb).map(|i| i as u8)
    }

    /// The colour of a terrain kind.
    pub fn colour_of(&self, s: Surface) -> [u8; 3] {
        self.colours[s as usize]
    }

    /// Every colour with its kind's name, for messages.
    pub fn entries(&self) -> Vec<(&'static str, [u8; 3])> {
        DEFAULT_PALETTE.iter().zip(self.colours).map(|((n, _), c)| (*n, c)).collect()
    }
}

/// A decoded terrain: the size and the codes, row 0 at the top (the `.DTm` order).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Terrain {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<u8>,
}

/// A problem in the picture: where it is (a pixel, or the whole picture) and what is wrong.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TerrainProblem {
    pub at: Option<(u32, u32)>,
    pub message: String,
    pub hint: String,
}

/// How many distinct unknown colours are reported (each once, at its first pixel).
const MAX_UNKNOWN: usize = 8;

/// Decodes a terrain PNG with `palette`. Every unknown colour is reported once, at its first
/// pixel; a size the engine does not load and a picture that is not RGB or indexed are reported
/// as a whole.
pub fn decode(png_bytes: &[u8], palette: &Palette) -> Result<Terrain, Vec<TerrainProblem>> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(png_bytes));
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().map_err(|e| vec![whole(format!("terrain.png is not a PNG picture ({e})"), "save the terrain as a PNG")])?;
    let mut buf = vec![0; reader.output_buffer_size()];
    let info = reader.next_frame(&mut buf).map_err(|e| vec![whole(format!("terrain.png cannot be read ({e})"), "save the terrain as an 8-bit RGB or indexed PNG")])?;
    let (w, h) = (info.width, info.height);
    let mut problems = Vec::new();
    if !MAP_SIZES.contains(&w) || !MAP_SIZES.contains(&h) {
        problems.push(whole(
            format!("terrain is {w}×{h} cells: a map is 50, 100 or 200 cells on each side"),
            "resize the picture to 50×50, 100×100 or 200×200 (one pixel is one cell)",
        ));
    }
    let channels = match info.color_type {
        png::ColorType::Rgb | png::ColorType::Indexed => 3,
        png::ColorType::Rgba => 4,
        other => {
            problems.push(whole(
                format!("terrain.png is {other:?}: the terrain must be an RGB or an indexed picture"),
                "save it as RGB or indexed colour (a palette), one colour per terrain kind",
            ));
            return Err(problems);
        }
    };
    if !problems.is_empty() {
        return Err(problems);
    }
    let data = &buf[..info.buffer_size()];
    let mut cells = Vec::with_capacity((w * h) as usize);
    let mut unknown: Vec<(u32, u32, String)> = Vec::new();
    let mut seen: Vec<[u8; 4]> = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let at = (y * w + x) as usize * channels;
            let px = &data[at..at + channels];
            let rgba = [px[0], px[1], px[2], if channels == 4 { px[3] } else { 255 }];
            let code = if rgba[3] == 255 { palette.code_of([rgba[0], rgba[1], rgba[2]]) } else { None };
            match code {
                Some(c) => cells.push(c),
                None => {
                    cells.push(0);
                    if !seen.contains(&rgba) {
                        seen.push(rgba);
                        let what = if rgba[3] == 255 {
                            hex([rgba[0], rgba[1], rgba[2]])
                        } else {
                            format!("{} with alpha {}", hex([rgba[0], rgba[1], rgba[2]]), rgba[3])
                        };
                        unknown.push((x, y, what));
                    }
                }
            }
        }
    }
    for (x, y, what) in unknown.iter().take(MAX_UNKNOWN) {
        problems.push(TerrainProblem {
            at: Some((*x, *y)),
            message: format!("pixel ({x}, {y}) is {what}: not a terrain colour"),
            hint: "use one of the palette's colours (docs/reference/rzpack-format.md) or set this kind's colour under [terrain_palette]".to_string(),
        });
    }
    if problems.is_empty() {
        Ok(Terrain { width: w, height: h, cells })
    } else {
        Err(problems)
    }
}

fn whole(message: String, hint: &str) -> TerrainProblem {
    TerrainProblem { at: None, message, hint: hint.to_string() }
}

/// A picture of the given size, all cells `fill`, for tests and the example's generator.
#[cfg(test)]
pub fn encode_rgb(w: u32, h: u32, colour_of: impl Fn(u32, u32) -> [u8; 3]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgb);
        enc.set_depth(png::BitDepth::Eight);
        let mut writer = enc.write_header().unwrap();
        let mut data = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h {
            for x in 0..w {
                data.extend_from_slice(&colour_of(x, y));
            }
        }
        writer.write_image_data(&data).unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grass(_: u32, _: u32) -> [u8; 3] {
        DEFAULT_PALETTE[6].1
    }

    #[test]
    fn default_palette_names_every_surface_once() {
        for s in Surface::ALL {
            assert_eq!(surface_by_name(surface_name(s)), Some(s));
            assert_eq!(Palette::default().colour_of(s), DEFAULT_PALETTE[s as usize].1);
        }
        let p = Palette::default();
        assert_eq!(p.code_of(DEFAULT_PALETTE[1].1), Some(1));
        assert_eq!(p.code_of([1, 2, 3]), None);
    }

    #[test]
    fn hex_colours() {
        assert_eq!(parse_hex("#60A040"), Some([0x60, 0xa0, 0x40]));
        assert_eq!(parse_hex("60a040"), Some([0x60, 0xa0, 0x40]));
        assert_eq!(parse_hex("#60a04"), None);
        assert_eq!(parse_hex("#60a04g"), None);
        assert_eq!(hex([0x60, 0xa0, 0x40]), "#60a040");
    }

    #[test]
    fn overrides_change_one_kind_and_reject_clashes() {
        let mut o = BTreeMap::new();
        o.insert("grass_plain".to_string(), "#60a040".to_string());
        let p = Palette::with_overrides(&o).unwrap();
        assert_eq!(p.colour_of(Surface::GrassPlain), [0x60, 0xa0, 0x40]);
        assert_eq!(p.code_of([0x60, 0xa0, 0x40]), Some(6));
        assert_eq!(p.code_of(DEFAULT_PALETTE[6].1), None);

        let mut bad = BTreeMap::new();
        bad.insert("grass".to_string(), "#60a040".to_string());
        assert!(Palette::with_overrides(&bad).unwrap_err().contains("not a terrain kind"));
        let mut bad = BTreeMap::new();
        bad.insert("grass_plain".to_string(), "green".to_string());
        assert!(Palette::with_overrides(&bad).unwrap_err().contains("not a colour"));
        let mut clash = BTreeMap::new();
        clash.insert("dry_plain".to_string(), "#6fa84a".to_string());
        assert!(Palette::with_overrides(&clash).unwrap_err().contains("same colour"));
    }

    #[test]
    fn a_picture_decodes_to_codes() {
        let png = encode_rgb(50, 50, |x, y| if x < 25 { DEFAULT_PALETTE[1].1 } else if y == 0 { DEFAULT_PALETTE[4].1 } else { grass(x, y) });
        let t = decode(&png, &Palette::default()).unwrap();
        assert_eq!((t.width, t.height), (50, 50));
        assert_eq!(t.cells[0], 1);
        assert_eq!(t.cells[30], 4, "row 0, x 30 is a road");
        assert_eq!(t.cells[50 + 30], 6, "row 1 is grass");
    }

    #[test]
    fn an_unknown_colour_names_its_pixel() {
        let png = encode_rgb(50, 50, |x, y| if (x, y) == (7, 9) { [1, 2, 3] } else { grass(x, y) });
        let problems = decode(&png, &Palette::default()).unwrap_err();
        assert_eq!(problems.len(), 1);
        assert_eq!(problems[0].at, Some((7, 9)));
        assert!(problems[0].message.contains("pixel (7, 9) is #010203"), "{}", problems[0].message);
    }

    #[test]
    fn a_bad_size_is_an_error() {
        let png = encode_rgb(51, 50, grass);
        let problems = decode(&png, &Palette::default()).unwrap_err();
        assert!(problems.iter().any(|p| p.message.contains("51×50 cells")), "{problems:?}");
        let png = encode_rgb(100, 200, grass);
        assert!(decode(&png, &Palette::default()).is_ok());
    }

    #[test]
    fn a_non_png_is_an_error() {
        let problems = decode(b"not a picture", &Palette::default()).unwrap_err();
        assert!(problems[0].message.contains("not a PNG"), "{problems:?}");
    }
}
