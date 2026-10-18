//! Decoders for the original's images: `.ugs` sprite sequences, `.lit` DCT images and `.spi`
//! editor icons, all to plain RGBA. Formats are in `docs/reference/graphics-formats.md`.
//!
//! Pure: no macroquad. Nothing decoded here is ever written back into the repo; the images come
//! from the player's install at runtime (see the `DtInstall` helpers at the end).

use super::install::{find_path, DtInstall};
use super::DtError;
use std::fmt;
use std::path::{Path, PathBuf};

/// An RGBA image, 8 bits per channel, rows top-down, not premultiplied.
#[derive(Clone, PartialEq, Eq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl fmt::Debug for Image {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Image({}x{})", self.width, self.height)
    }
}

impl Image {
    /// The RGBA value at (x, y). Panics outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        assert!(x < self.width && y < self.height, "pixel ({x}, {y}) outside {self:?}");
        let i = ((y * self.width + x) * 4) as usize;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }

    /// True when every pixel has alpha 0.
    pub fn fully_transparent(&self) -> bool {
        self.rgba.chunks_exact(4).all(|p| p[3] == 0)
    }
}

/// Spell and some battle effects store colour with alpha 0 everywhere; they are meant to be drawn
/// additively (black = nothing). True when every frame is like that and some pixel has colour.
pub fn is_additive(frames: &[Image]) -> bool {
    !frames.is_empty()
        && frames.iter().all(Image::fully_transparent)
        && frames.iter().any(|f| f.rgba.chunks_exact(4).any(|p| p[..3] != [0, 0, 0]))
}

fn bad(what: &'static str, offset: usize) -> DtError {
    DtError::Image { what, offset }
}

fn u16_at(d: &[u8], o: usize) -> Option<u16> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]]))
}

fn u32_at(d: &[u8], o: usize) -> Option<u32> {
    d.get(o..o + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

// ------------------------------------------------------------------------------------------
// UGS
// ------------------------------------------------------------------------------------------

/// One UGS pixel. The file stores `rotl16(ARGB4444, 3) ^ 0xAAAA`; 0xAAAA is fully transparent.
pub fn ugs_pixel(v: u16) -> [u8; 4] {
    let p = v.rotate_right(3) ^ 0x5555; // 0x5555 == rotr16(0xAAAA, 3)
    let n = |shift: u16| ((p >> shift) & 15) as u8 * 17;
    [n(8), n(4), n(0), n(12)]
}

/// `w * h` UGS pixels starting at `o`; the caller has checked the bounds.
fn ugs_pixels(d: &[u8], o: usize, w: usize, h: usize) -> Image {
    let mut rgba = Vec::with_capacity(w * h * 4);
    for px in d[o..o + w * h * 2].chunks_exact(2) {
        rgba.extend_from_slice(&ugs_pixel(u16::from_le_bytes([px[0], px[1]])));
    }
    Image { width: w as u32, height: h as u32, rgba }
}

/// A plain UGS sequence: `[u16 w][u16 h][w*h u16 pixels]` repeated to the end of the file.
/// `Persones.ugs` repeats the size (`[u16 w][u16 h][u16 w][u16 h][pixels]`); that is detected
/// from the first frame. `Objects.ugs` has its own layout, see [`decode_objects_ugs`].
pub fn decode_ugs(d: &[u8]) -> Result<Vec<Image>, DtError> {
    let (Some(w), Some(h)) = (u16_at(d, 0), u16_at(d, 2)) else {
        return Err(DtError::Truncated { what: "UGS frame header", offset: 0 });
    };
    let n = w as usize * h as usize * 2;
    let repeated = u16_at(d, 4) == Some(w) && u16_at(d, 6) == Some(h) && 8 + n <= d.len();
    // A plain file whose first two pixels happen to equal (w, h) still has the next frame's
    // header right after `4 + n` bytes.
    let plain_next = 4 + n < d.len() && u16_at(d, 4 + n) == Some(w) && u16_at(d, 6 + n) == Some(h);
    let hdr = if repeated && !plain_next { 8 } else { 4 };
    let mut frames = Vec::new();
    let mut o = 0;
    while o + hdr <= d.len() {
        let w = u16_at(d, o).unwrap_or(0) as usize;
        let h = u16_at(d, o + 2).unwrap_or(0) as usize;
        if w == 0 || h == 0 || o + hdr + w * h * 2 > d.len() {
            return Err(bad("UGS frame", o));
        }
        frames.push(ugs_pixels(d, o + hdr, w, h));
        o += hdr + w * h * 2;
    }
    Ok(frames)
}

/// The single-frame stills of `Graphics/Windows` (`MB2`, `MM_Icons`, `Title_RUS`) are not
/// scrambled: each pixel is one grey byte and one alpha byte. The game tints them (the menu
/// buttons, the minimap symbols by owner).
pub const UGS_GREY_STILLS: [&str; 3] = ["mb2.ugs", "mm_icons.ugs", "title_rus.ugs"];

/// The event window's defeat and victory pictures (`Stnd-1`, `Stnd-2`): `[u16 w][u16 h]`
/// then one little-endian u16 per pixel, ARGB 4444. The window splits each pixel into its
/// low 12 bits (the colour, through the 4096-entry table 0xae2b50) and its top 4 (the
/// alpha, a colour above 0 with alpha 0 drawn at 8 of 31): 0x4d16ef–0x4d1753.
pub const UGS_ARGB4444_STILLS: [&str; 2] = ["stnd-1.ugs", "stnd-2.ugs"];

/// An ARGB 4444 still (see [`UGS_ARGB4444_STILLS`]).
pub fn decode_ugs_argb4444(d: &[u8]) -> Result<Image, DtError> {
    let (Some(w), Some(h)) = (u16_at(d, 0), u16_at(d, 2)) else {
        return Err(DtError::Truncated { what: "UGS still header", offset: 0 });
    };
    let n = w as usize * h as usize;
    let px = d.get(4..4 + n * 2).ok_or(DtError::Truncated { what: "UGS still pixels", offset: 4 })?;
    let rgba = px
        .chunks_exact(2)
        .flat_map(|p| {
            let v = u16::from_le_bytes([p[0], p[1]]);
            let nib = |s: u16| ((v >> s) & 15) as u8 * 17;
            let (r, g, b, mut a) = (nib(8), nib(4), nib(0), nib(12));
            if a == 0 && v & 0xfff != 0 {
                a = (8 * 255 / 31) as u8;
            }
            [r, g, b, a]
        })
        .collect();
    Ok(Image { width: w as u32, height: h as u32, rgba })
}

/// A grey + alpha still (see [`UGS_GREY_STILLS`]): `[u16 w][u16 h]` then `w*h` pairs of
/// (grey, alpha) bytes; alpha counts from the background's.
pub fn decode_ugs_grey(d: &[u8]) -> Result<Image, DtError> {
    let (Some(w), Some(h)) = (u16_at(d, 0), u16_at(d, 2)) else {
        return Err(DtError::Truncated { what: "UGS still header", offset: 0 });
    };
    let n = w as usize * h as usize;
    let px = d.get(4..4 + n * 2).ok_or(DtError::Truncated { what: "UGS still pixels", offset: 4 })?;
    // `Title_RUS` fills its background with alpha 15, which the game does not show: the
    // corner pixel's alpha is taken as the floor.
    let floor = px.get(1).copied().unwrap_or(0).min(254) as u32;
    let alpha = |a: u8| ((a as u32).saturating_sub(floor) * 255 / (255 - floor)) as u8;
    let rgba = px.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], alpha(p[1])]).collect();
    Ok(Image { width: w as u32, height: h as u32, rgba })
}

/// One sprite of `Objects/Objects.ugs`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectSprite {
    /// 0: terrain decorations (hills, mountains, stones, trees); 1: buildings.
    pub section: u32,
    /// Section 0: the map object class of `.DTm` objects. Section 1: the building picture type.
    pub cat: u32,
    /// Section 0: the map object sprite id. Section 1: the picture variant.
    pub idx: u32,
    /// Texture atlas size: the power of two >= width.
    pub cell: u32,
    /// Section 0: B, G, R average colour + one byte (M/L). Section 1: two u32, likely a footprint.
    pub extra: Vec<u8>,
    pub image: Image,
}

impl ObjectSprite {
    pub const DECORATIONS: u32 = 0;
    pub const BUILDINGS: u32 = 1;

    /// A building sprite's footprint in cells (x, y): the two u32 of its extra field. Every
    /// building of the 15 shipped maps has exactly this size (bytes 289, 290), so it is the
    /// size an editor gives a new building of this picture.
    pub fn footprint(&self) -> Option<(u8, u8)> {
        if self.section != ObjectSprite::BUILDINGS || self.extra.len() != 8 {
            return None;
        }
        let a = u32::from_le_bytes(self.extra[0..4].try_into().ok()?);
        let b = u32::from_le_bytes(self.extra[4..8].try_into().ok()?);
        Some((u8::try_from(a).ok()?, u8::try_from(b).ok()?))
    }
}

/// `Objects/Objects.ugs`: a slot table. A slot is a u32 0 (empty) or
/// `u32 cat, u32 idx, u32 cell, u32 w, u32 h, extra[..], u16 w, u16 h, w*h pixels`.
/// The length of `extra` is found by searching for the repeated size; a change of that length
/// starts the next section.
pub fn decode_objects_ugs(d: &[u8]) -> Result<Vec<ObjectSprite>, DtError> {
    let mut out = Vec::new();
    let mut o = 0;
    let mut section = 0;
    let mut prev: Option<usize> = None;
    while o < d.len() {
        let Some(first) = u32_at(d, o) else { return Err(bad("Objects.ugs slot", o)) };
        if first == 0 {
            o += 4;
            continue;
        }
        let field = |i: usize| u32_at(d, o + 4 * i).ok_or(bad("Objects.ugs record", o));
        let (cat, idx, cell, w, h) = (field(0)?, field(1)?, field(2)?, field(3)?, field(4)?);
        let (wu, hu) = (w as usize, h as usize);
        let k = (5..14)
            .find(|&k| {
                let p = o + 4 * k;
                u16_at(d, p).map(u32::from) == Some(w)
                    && u16_at(d, p + 2).map(u32::from) == Some(h)
                    && p + 4 + wu * hu * 2 <= d.len()
            })
            .ok_or(bad("Objects.ugs record", o))?;
        let extra = d[o + 20..o + 4 * k].to_vec();
        if prev.is_some_and(|p| p != extra.len()) {
            section += 1;
        }
        prev = Some(extra.len());
        let image = ugs_pixels(d, o + 4 * k + 4, wu, hu);
        out.push(ObjectSprite { section, cat, idx, cell, extra, image });
        o += 4 * k + 4 + wu * hu * 2;
    }
    Ok(out)
}

// ------------------------------------------------------------------------------------------
// LIT
// ------------------------------------------------------------------------------------------

/// LIT flag: raw interleaved Y, Cb, Cr bytes.
pub const LIT_RAW: u32 = 4;
/// LIT flag: chroma 4:2:0, planes padded to 16.
pub const LIT_420: u32 = 2;
/// LIT flag: a fourth (alpha) plane.
pub const LIT_ALPHA: u32 = 8;

/// The orthonormal 8-point DCT basis, `C[k][n]`.
fn dct_basis() -> [[f64; 8]; 8] {
    let mut c = [[0.0; 8]; 8];
    for (k, row) in c.iter_mut().enumerate() {
        let s = if k == 0 { (1.0f64 / 8.0).sqrt() } else { (2.0f64 / 8.0).sqrt() };
        for (n, v) in row.iter_mut().enumerate() {
            *v = s * (((2 * n + 1) * k) as f64 * std::f64::consts::PI / 16.0).cos();
        }
    }
    c
}

/// One DCT plane of `pw * ph` samples at `o`. Returns the samples (unclamped) and the offset
/// after the plane.
fn lit_plane(d: &[u8], o: usize, pw: usize, ph: usize, c: &[[f64; 8]; 8]) -> Result<(Vec<f64>, usize), DtError> {
    let (bw, bh) = (pw / 8, ph / 8);
    let nb = bw * bh;
    let end = o + 128 + 64 * nb;
    if end > d.len() {
        return Err(DtError::Truncated { what: "LIT plane", offset: o });
    }
    let quant = &d[o..o + 64];
    let rank = &d[o + 64..o + 128];
    if let Some(r) = rank.iter().position(|&r| r >= 64) {
        return Err(bad("LIT coefficient rank", o + 64 + r));
    }
    let data = &d[o + 128..end];
    let mut out = vec![0.0; pw * ph];
    let mut coef = [[0.0f64; 8]; 8];
    let mut nonzero: Vec<(usize, usize, f64)> = Vec::with_capacity(64);
    for b in 0..nb {
        for pos in 0..64 {
            // Every stored byte is signed, the DC too (0x4778b4: CBW). The encoder keeps DC
            // bytes under 128, but slot 0 does not always hold the DC: in `Res-Magic` it holds
            // an AC coefficient, whose negative values read unsigned drew grey stripes.
            let byte = data[rank[pos] as usize * nb + b];
            let v = byte as i8 as f64;
            coef[pos / 8][pos % 8] = v * quant[pos] as f64;
        }
        let (bx, by) = (b % bw, b / bw);
        // Sum of C[k][i]·coef[k][l]·C[l][j] over (k, l) in raster order, as numpy's einsum does,
        // which kept the output bit-identical to the Python reference decoder. Zero coefficients add
        // exactly nothing, so they are skipped (most AC coefficients are zero).
        nonzero.clear();
        for (k, row) in coef.iter().enumerate() {
            for (l, &v) in row.iter().enumerate() {
                if v != 0.0 {
                    nonzero.push((k, l, v));
                }
            }
        }
        for i in 0..8 {
            for j in 0..8 {
                let mut s = 0.0;
                for &(k, l, v) in &nonzero {
                    s += c[k][i] * v * c[l][j];
                }
                out[(by * 8 + i) * pw + bx * 8 + j] = s;
            }
        }
    }
    Ok((out, end))
}

/// Round half to even and clamp to a byte.
fn to_byte(v: f64) -> u8 {
    v.round_ties_even().clamp(0.0, 255.0) as u8
}

/// JFIF YCbCr to RGB, Cb and Cr centred on 128.
fn ycc(y: f64, cb: f64, cr: f64) -> [u8; 3] {
    let (cb, cr) = (cb - 128.0, cr - 128.0);
    [to_byte(y + 1.402 * cr), to_byte(y - 0.344136 * cb - 0.714136 * cr), to_byte(y + 1.772 * cb)]
}

/// The same in single precision (the raw mode of the reference decoder works in f32).
fn ycc_f32(y: f32, cb: f32, cr: f32) -> [u8; 3] {
    let (cb, cr) = (cb - 128.0, cr - 128.0);
    let b = |v: f32| v.round_ties_even().clamp(0.0, 255.0) as u8;
    [b(y + 1.402 * cr), b(y - 0.344136 * cb - 0.714136 * cr), b(y + 1.772 * cb)]
}

/// A LIT image: `"LIT\0"`, u32 width, u32 height, u32 flags, then raw YCbCr or DCT planes
/// Y, Cb, Cr[, A]. A file that is really a BMP (`Windows/Win-black.lit`) is decoded as a BMP.
pub fn decode_lit(d: &[u8]) -> Result<Image, DtError> {
    if d.starts_with(b"BM") {
        return decode_bmp(d);
    }
    if !d.starts_with(b"LIT\0") {
        return Err(DtError::BadMagic { what: "LIT image" });
    }
    let (Some(w), Some(h), Some(flags)) = (u32_at(d, 4), u32_at(d, 8), u32_at(d, 12)) else {
        return Err(DtError::Truncated { what: "LIT header", offset: 0 });
    };
    let (w, h) = (w as usize, h as usize);
    if w == 0 || h == 0 || w > 1 << 14 || h > 1 << 14 {
        return Err(bad("LIT size", 4));
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    if flags & LIT_RAW != 0 {
        let src = d.get(16..16 + w * h * 3).ok_or(DtError::Truncated { what: "LIT raw pixels", offset: 16 })?;
        for p in src.chunks_exact(3) {
            rgba.extend_from_slice(&ycc_f32(p[0] as f32, p[1] as f32, p[2] as f32));
            rgba.push(255);
        }
        return Ok(Image { width: w as u32, height: h as u32, rgba });
    }
    let sub = flags & LIT_420 != 0;
    let align = if sub { 16 } else { 8 };
    let (pw, ph) = (w.div_ceil(align) * align, h.div_ceil(align) * align);
    let (cw, ch) = if sub { (pw / 2, ph / 2) } else { (pw, ph) };
    let c = dct_basis();
    let (y, o) = lit_plane(d, 16, pw, ph, &c)?;
    let (cb, o) = lit_plane(d, o, cw, ch, &c)?;
    let (cr, o) = lit_plane(d, o, cw, ch, &c)?;
    let (alpha, o) = if flags & LIT_ALPHA != 0 {
        let (a, o) = lit_plane(d, o, pw, ph, &c)?;
        (Some(a), o)
    } else {
        (None, o)
    };
    if o != d.len() {
        return Err(DtError::TrailingBytes { offset: o, count: d.len().saturating_sub(o) });
    }
    // The original keeps every plane as bytes (0x477850), smooths the chroma's block edges
    // (0x477ca4) and, at 4:2:0, doubles the chroma and smooths it 1-2-1 (0x477de4).
    let y = plane_bytes(&y);
    let mut chroma = [plane_bytes(&cb), plane_bytes(&cr)];
    for p in &mut chroma {
        deblock(p, cw, ch);
        if sub {
            *p = upsample(p, cw, ch);
        }
    }
    let [cb, cr] = chroma;
    for row in 0..h {
        for col in 0..w {
            let i = row * pw + col;
            rgba.extend_from_slice(&ycc(y[i] as f64, cb[i] as f64, cr[i] as f64));
            rgba.push(alpha.as_ref().map_or(255, |a| plane_byte(a[i])));
        }
    }
    Ok(Image { width: w as u32, height: h as u32, rgba })
}

/// A decoded sample as the original stores it (0x477850): its IDCT works at 8×, rounds,
/// drops negatives, then `(v + 4) >> 3`, 255 at most.
fn plane_byte(v: f64) -> u8 {
    let v8 = (v * 8.0).round_ties_even().max(0.0) as i64;
    ((v8 + 4) >> 3).min(255) as u8
}

fn plane_bytes(p: &[f64]) -> Vec<u8> {
    p.iter().map(|&v| plane_byte(v)).collect()
}

/// The chroma's block edges brought a quarter of their step together (0x477ca4): first the
/// edges between block rows, then between block columns, each pair moving toward the other
/// by `(a - b) / 4` rounded toward zero.
fn deblock(p: &mut [u8], w: usize, h: usize) {
    let step = |p: &mut [u8], a: usize, b: usize| {
        let q = (p[a] as i32 - p[b] as i32) / 4;
        p[a] = (p[a] as i32 - q) as u8;
        p[b] = (p[b] as i32 + q) as u8;
    };
    for edge in (8..h).step_by(8) {
        for x in 0..w {
            step(p, (edge - 1) * w + x, edge * w + x);
        }
    }
    for edge in (8..w).step_by(8) {
        for y in 0..h {
            step(p, y * w + edge - 1, y * w + edge);
        }
    }
}

/// 4:2:0 chroma at full size (0x477de4): every sample doubled both ways, then each row and
/// then each column smoothed `(l + 2c + r) >> 2`, the ends kept.
fn upsample(p: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (w2, h2) = (w * 2, h * 2);
    let mut out = vec![0u8; w2 * h2];
    for y in 0..h2 {
        for x in 0..w2 {
            out[y * w2 + x] = p[(y / 2) * w + x / 2];
        }
    }
    let smooth = |out: &mut [u8], at: &dyn Fn(usize) -> usize, n: usize| {
        let mut prev = out[at(0)] as u32;
        for k in 1..n.saturating_sub(1) {
            let cur = out[at(k)] as u32;
            out[at(k)] = ((cur * 2 + prev + out[at(k + 1)] as u32) >> 2) as u8;
            prev = cur;
        }
    };
    for y in 0..h2 {
        smooth(&mut out, &|x| y * w2 + x, w2);
    }
    for x in 0..w2 {
        smooth(&mut out, &|y| y * w2 + x, h2);
    }
    out
}

// ------------------------------------------------------------------------------------------
// SPI, BMP
// ------------------------------------------------------------------------------------------

/// `Editor/*.spi`: a headerless square BGRA image (22×22 in the shipped files), rows top-down.
pub fn decode_spi(d: &[u8]) -> Result<Image, DtError> {
    let n = ((d.len() / 4) as f64).sqrt().round() as usize;
    if n == 0 || n * n * 4 > d.len() {
        return Err(bad("SPI size", 0));
    }
    let mut rgba = Vec::with_capacity(n * n * 4);
    for p in d[..n * n * 4].chunks_exact(4) {
        rgba.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
    }
    Ok(Image { width: n as u32, height: n as u32, rgba })
}

/// An uncompressed BMP (water frames, `Win-black.lit`, a mod's pictures saved as BMP under a
/// `.lit` name). Alpha is always 255. 24 and 32 bits per pixel, and 8 bits with a palette as
/// the original's BMP loader reads them (0x47754c: the palette at 0x36, `biClrUsed` colours or
/// 256 when 0, the pixels right after the palette, rows bottom-up padded to 4 bytes).
pub fn decode_bmp(d: &[u8]) -> Result<Image, DtError> {
    if !d.starts_with(b"BM") {
        return Err(DtError::BadMagic { what: "BMP image" });
    }
    let trunc = DtError::Truncated { what: "BMP header", offset: 0 };
    let data_off = u32_at(d, 10).ok_or(trunc)? as usize;
    let w = u32_at(d, 18).map(|v| v as i32);
    let h = u32_at(d, 22).map(|v| v as i32);
    let (Some(w), Some(h), Some(bpp), Some(compression)) = (w, h, u16_at(d, 28), u32_at(d, 30)) else {
        return Err(DtError::Truncated { what: "BMP header", offset: 0 });
    };
    if w <= 0 || h == 0 || !(bpp == 8 || bpp == 24 || bpp == 32) || compression != 0 {
        return Err(bad("BMP (only uncompressed 8/24/32-bit)", 0));
    }
    let (w, top_down, h) = (w as usize, h < 0, h.unsigned_abs() as usize);
    // A broken or hostile header (a mod's file) must not reserve gigabytes before the pixel
    // checks below; the same bound as a LIT's.
    if w > 1 << 14 || h > 1 << 14 {
        return Err(bad("BMP size", 18));
    }
    let mut rgba = Vec::with_capacity(w * h * 4);
    if bpp == 8 {
        // 0x47754c: the palette right after the 40-byte header, whatever bfOffBits says.
        let colours = match u32_at(d, 0x2e).ok_or(DtError::Truncated { what: "BMP header", offset: 0 })? as usize {
            0 => 256,
            n => n.min(256),
        };
        let palette = d.get(0x36..0x36 + colours * 4).ok_or(DtError::Truncated { what: "BMP palette", offset: 0x36 })?;
        let pixels = 0x36 + colours * 4;
        let stride = w.div_ceil(4) * 4;
        if pixels + stride * h > d.len() {
            return Err(DtError::Truncated { what: "BMP pixels", offset: pixels });
        }
        for row in 0..h {
            let src = if top_down { row } else { h - 1 - row };
            for &i in &d[pixels + src * stride..][..w] {
                // An index past the palette's colours reads as black.
                let p = palette.get(i as usize * 4..i as usize * 4 + 3).unwrap_or(&[0, 0, 0]);
                rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
            }
        }
        return Ok(Image { width: w as u32, height: h as u32, rgba });
    }
    let bytes = bpp as usize / 8;
    let stride = (w * bytes).div_ceil(4) * 4;
    if data_off + stride * h > d.len() {
        return Err(DtError::Truncated { what: "BMP pixels", offset: data_off });
    }
    for row in 0..h {
        let src = if top_down { row } else { h - 1 - row };
        let line = &d[data_off + src * stride..][..w * bytes];
        for p in line.chunks_exact(bytes) {
            rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
        }
    }
    Ok(Image { width: w as u32, height: h as u32, rgba })
}

/// Rounds each pixel to the original's 16-bit surfaces: 5, 6 and 5 bits (0x476374 packs them
/// from byte tables), widened back as the screen shows them.
pub fn quantize_565(img: &mut Image) {
    for p in img.rgba.chunks_exact_mut(4) {
        let (r, g, b) = (p[0] >> 3, p[1] >> 2, p[2] >> 3);
        p[0] = (r << 3) | (r >> 2);
        p[1] = (g << 2) | (g >> 4);
        p[2] = (b << 3) | (b >> 2);
    }
}

/// A picture the original loads without its alpha and draws with a colour key. The ability
/// icons (`Bonus<N>`, `Bonus-2Row`, `HeroBonus<N>`) are loaded by 0x48d02c, which frees the
/// mask 0x48cd90 made, whatever the file (LIT or BMP, 0x47754c); the unit and hero windows put
/// them into their text list by 0x48e8f0, which takes the 16-bit colour of the picture's
/// bottom-left pixel (0x4764c0 at (0, h − 1)) and copies only the pixels of another colour
/// (0x47669c). So any background (the shipped icons' violet, a mod's white BMP) is
/// transparent, compared at 5-6-5 bits. The picture comes out at those levels, the keyed
/// pixels transparent black.
pub fn key_bottom_left_565(img: &mut Image) {
    quantize_565(img);
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 {
        return;
    }
    let at = (h - 1) * w * 4;
    let key = [img.rgba[at], img.rgba[at + 1], img.rgba[at + 2]];
    for p in img.rgba.chunks_exact_mut(4) {
        if p[..3] == key {
            p.copy_from_slice(&[0, 0, 0, 0]);
        }
    }
}

/// Decode any graphics file by extension (`.ugs`, `.lit`, `.spi`, `.bmp`). `Objects.ugs` gives
/// its sprites in slot order.
pub fn decode_file(path: &Path) -> Result<Vec<Image>, DtError> {
    let d = std::fs::read(path).map_err(|source| DtError::Io { path: path.to_path_buf(), source })?;
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    let name = path.file_name().map(|n| n.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "lit" => Ok(vec![decode_lit(&d)?]),
        "spi" => Ok(vec![decode_spi(&d)?]),
        "bmp" => Ok(vec![decode_bmp(&d)?]),
        "ugs" if name == "objects.ugs" => Ok(decode_objects_ugs(&d)?.into_iter().map(|s| s.image).collect()),
        "ugs" if UGS_GREY_STILLS.contains(&name.as_str()) => Ok(vec![decode_ugs_grey(&d)?]),
        "ugs" if UGS_ARGB4444_STILLS.contains(&name.as_str()) => Ok(vec![decode_ugs_argb4444(&d)?]),
        "ugs" => decode_ugs(&d),
        _ => Err(DtError::BadMagic { what: "known graphics file (.ugs/.lit/.spi/.bmp)" }),
    }
}

// ------------------------------------------------------------------------------------------
// Locating art in an install
// ------------------------------------------------------------------------------------------

/// Colour unit busts (92×92), frame = `GlobalIndex − 1`.
pub const UNIT_PORTRAITS: &str = "Graphics/Objects/Icons.ugs";
/// Sepia full-body unit portraits, same order as [`UNIT_PORTRAITS`].
pub const UNIT_FIGURES: &str = "Graphics/Objects/Persones.ugs";
/// Artefact icons (53×53), frame from `Icon=Axxx.Tga`.
pub const ITEM_ICONS: &str = "Graphics/Objects/Items.ugs";
/// Map decorations and building sprites.
pub const MAP_OBJECTS: &str = "Graphics/Objects/Objects.ugs";
pub const TEXTURES_DIR: &str = "Graphics/Textures";

/// Terrain texture file (in [`TEXTURES_DIR`]) by `.DTm` terrain code, from the exe's name table (M).
pub const TERRAIN_TEXTURES: [&str; 16] = [
    "Shallow.lit",
    "Water.lit",
    "DeepWater.lit",
    "FlameLand.lit",
    "Road.lit",
    "LowLand.lit",
    "land.lit",
    "plain.lit",
    "Swamp.lit",
    "DeepSwamp.lit",
    "Desert.lit",
    "Badground.lit",
    "Rock.lit",
    "Dust.lit",
    "Snow.lit",
    "Ice.lit",
];

/// Animated water frames, 64×64 24-bit BMPs loaded at 0x4cdbb0 as `Graphics\Textures\water\`
/// `texture00<i>.bmp` (i < 10) or `texture0<i>.bmp`, i.e. three digits.
pub const WATER_FRAMES: usize = 32;

/// The install path of water frame `i` (0..[`WATER_FRAMES`]).
pub fn water_frame_file(i: usize) -> String {
    format!("{TEXTURES_DIR}/Water/TEXTURE{i:03}.BMP")
}

/// The water frame shown at clock `ms`: `(t div 100) mod 32` (0x4c8db2).
pub fn water_frame(ms: i64) -> usize {
    (ms.div_euclid(100)).rem_euclid(WATER_FRAMES as i64) as usize
}

/// Frame of [`UNIT_PORTRAITS`] / [`UNIT_FIGURES`] for a unit `GlobalIndex` (not `IconIndex`).
pub fn portrait_frame(unit_id: u32) -> Option<usize> {
    (unit_id as usize).checked_sub(1)
}

/// Frame of [`ITEM_ICONS`] for an artefact's `Icon=` value (`A012.Tga` → 12).
pub fn item_icon_frame(icon: &str) -> Option<usize> {
    let stem = icon.trim().split('.').next()?;
    let digits = stem.strip_prefix('A').or_else(|| stem.strip_prefix('a'))?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok()
}

/// Packs images into one atlas `width` pixels wide (shelves, tallest first). Each image has
/// a 1 px ring around it repeating its edge pixels, so a sprite scaled with linear filtering
/// never blends in the empty atlas around it (seams between sprites drawn side by side, a
/// bridge's pieces, at a zoom between whole steps). Returns the atlas and each image's
/// top-left corner, in input order. `None` if an image is wider than the atlas.
pub fn pack_atlas(images: &[&Image], width: u32) -> Option<(Image, Vec<(u32, u32)>)> {
    let mut order: Vec<usize> = (0..images.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse((images[i].height, images[i].width)));
    let mut pos = vec![(0, 0); images.len()];
    let (mut x, mut y, mut shelf) = (0u32, 0u32, 0u32);
    for &i in &order {
        let im = images[i];
        if im.width + 2 > width {
            return None;
        }
        if x + im.width + 2 > width {
            x = 0;
            y += shelf + 2;
            shelf = 0;
        }
        pos[i] = (x + 1, y + 1);
        x += im.width + 2;
        shelf = shelf.max(im.height);
    }
    let height = (y + shelf + 2).max(1);
    let mut rgba = vec![0u8; (width * height * 4) as usize];
    for (im, &(px, py)) in images.iter().zip(&pos) {
        if im.width == 0 || im.height == 0 {
            continue;
        }
        // Rows −1..=h, columns −1..=w, each from the nearest pixel of the image.
        for row in -1..=im.height as i64 {
            let sy = row.clamp(0, im.height as i64 - 1) as u32;
            for col in -1..=im.width as i64 {
                let sx = col.clamp(0, im.width as i64 - 1) as u32;
                let src = ((sy * im.width + sx) * 4) as usize;
                let dst = ((((py as i64 + row) as u32) * width + (px as i64 + col) as u32) * 4) as usize;
                rgba[dst..dst + 4].copy_from_slice(&im.rgba[src..src + 4]);
            }
        }
    }
    Some((Image { width, height, rgba }, pos))
}

/// The decoded `Objects.ugs`, with lookups by map object and building picture.
#[derive(Clone, Debug, Default)]
pub struct ObjectSprites {
    pub sprites: Vec<ObjectSprite>,
}

impl ObjectSprites {
    pub fn parse(d: &[u8]) -> Result<ObjectSprites, DtError> {
        Ok(ObjectSprites { sprites: decode_objects_ugs(d)? })
    }

    fn find(&self, section: u32, cat: u32, idx: u32) -> Option<&ObjectSprite> {
        self.sprites.iter().find(|s| s.section == section && s.cat == cat && s.idx == idx)
    }

    /// Sprite of a `.DTm` map object (class, sprite id).
    pub fn decoration(&self, class: u8, sprite: u8) -> Option<&ObjectSprite> {
        self.find(ObjectSprite::DECORATIONS, class.into(), sprite.into())
    }

    /// Sprite of a building by its picture type and variant (`.DTm` building bytes 5 and 4).
    pub fn building(&self, picture_type: u8, variant: u8) -> Option<&ObjectSprite> {
        self.find(ObjectSprite::BUILDINGS, picture_type.into(), variant.into())
    }
}

impl DtInstall {
    /// A file under the install, `/`-separated, matching each component ignoring case.
    pub fn file(&self, rel: &str) -> Result<PathBuf, DtError> {
        find_path(&self.dir, rel)
    }

    fn read(&self, rel: &str) -> Result<Vec<u8>, DtError> {
        let path = self.file(rel)?;
        std::fs::read(&path).map_err(|source| DtError::Io { path, source })
    }

    /// All unit busts, indexed by [`portrait_frame`].
    pub fn unit_portraits(&self) -> Result<Vec<Image>, DtError> {
        decode_ugs(&self.read(UNIT_PORTRAITS)?)
    }

    /// All full-body unit portraits, indexed by [`portrait_frame`].
    pub fn unit_figures(&self) -> Result<Vec<Image>, DtError> {
        decode_ugs(&self.read(UNIT_FIGURES)?)
    }

    /// All artefact icons, indexed by [`DtInstall::artefact_icon_frame`] (`GlobalIndex − 1`).
    pub fn item_icons(&self) -> Result<Vec<Image>, DtError> {
        decode_ugs(&self.read(ITEM_ICONS)?)
    }

    /// Frame of [`ITEM_ICONS`] for an artefact `GlobalIndex`: `GlobalIndex − 1`, one frame per
    /// artefact in file order, as the unit portraits (checked against the sheet: the helmets
    /// A43–A48 are frames 42–47). The `Icon=Axxx.Tga` field names the original's separate
    /// picture files and is not a frame of the sheet (A030 there would be a pair of arrows).
    pub fn artefact_icon_frame(&self, artefact_id: u32) -> Option<usize> {
        self.artefact(artefact_id)?;
        (artefact_id as usize).checked_sub(1)
    }

    /// Map decorations and building sprites.
    pub fn map_objects(&self) -> Result<ObjectSprites, DtError> {
        ObjectSprites::parse(&self.read(MAP_OBJECTS)?)
    }

    /// The world-map texture for a terrain code (0..16).
    pub fn terrain_texture(&self, code: u8) -> Result<Image, DtError> {
        let name = TERRAIN_TEXTURES.get(code as usize).ok_or(bad("terrain code", code as usize))?;
        decode_lit(&self.read(&format!("{TEXTURES_DIR}/{name}"))?)
    }

    /// The 32 animated water frames ([`water_frame_file`]); an error if any is missing.
    pub fn water_frames(&self) -> Result<Vec<Image>, DtError> {
        (0..WATER_FRAMES).map(|i| decode_bmp(&self.read(&water_frame_file(i))?)).collect()
    }

    /// Any graphics file of the install by relative path, see [`decode_file`].
    pub fn graphic(&self, rel: &str) -> Result<Vec<Image>, DtError> {
        decode_file(&self.file(rel)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_packs_images_without_overlap() {
        let img = |w: u32, h: u32, v: u8| Image { width: w, height: h, rgba: vec![v; (w * h * 4) as usize] };
        let (a, b, c) = (img(3, 2, 10), img(4, 3, 20), img(2, 1, 30));
        let (atlas, pos) = pack_atlas(&[&a, &b, &c], 12).unwrap();
        assert_eq!(pos, vec![(7, 1), (1, 1), (1, 6)]);
        assert_eq!((atlas.width, atlas.height), (12, 8));
        assert_eq!(atlas.pixel(7, 1)[0], 10);
        assert_eq!(atlas.pixel(4, 3)[0], 20);
        assert_eq!(atlas.pixel(1, 6)[0], 30);
        // Each image's ring repeats its edge: no empty pixel is ever sampled at its border.
        assert_eq!(atlas.pixel(5, 1)[0], 20, "b's right ring");
        assert_eq!(atlas.pixel(6, 1)[0], 10, "a's left ring");
        assert_eq!(atlas.pixel(0, 0)[0], 20, "b's corner");
        assert_eq!(atlas.pixel(10, 1)[0], 10, "a's right ring");
        assert_eq!(atlas.pixel(11, 1)[0], 0, "beyond the rings");
        assert!(pack_atlas(&[&b], 5).is_none());
    }

    /// Encode an ARGB4444 value the way UGS stores it.
    fn enc(argb: u16) -> u16 {
        argb.rotate_left(3) ^ 0xAAAA
    }

    fn frame(w: u16, h: u16, px: &[u16], repeat_header: bool) -> Vec<u8> {
        let mut v = Vec::new();
        let reps = if repeat_header { 2 } else { 1 };
        for _ in 0..reps {
            v.extend_from_slice(&w.to_le_bytes());
            v.extend_from_slice(&h.to_le_bytes());
        }
        for p in px {
            v.extend_from_slice(&enc(*p).to_le_bytes());
        }
        v
    }

    /// Position-weighted byte sum; the real-file tests compare it with the reference decoder.
    fn weighted_sum(img: &Image) -> u64 {
        img.rgba
            .iter()
            .enumerate()
            .fold(0u64, |s, (i, &b)| s.wrapping_add((b as u64).wrapping_mul(i as u64 % 65521 + 1)))
    }

    #[test]
    fn ugs_pixel_format() {
        assert_eq!(ugs_pixel(0xAAAA), [0, 0, 0, 0]);
        assert_eq!(ugs_pixel(enc(0xF000)), [0, 0, 0, 255]);
        assert_eq!(ugs_pixel(enc(0x1234)), [34, 51, 68, 17]);
        assert_eq!(ugs_pixel(enc(0xFF00)), [255, 0, 0, 255]);
        assert_eq!(ugs_pixel(enc(0x80F0)), [0, 255, 0, 136]);
        for v in [0u16, 1, 0x1234, 0xFFFF] {
            let [r, g, b, a] = ugs_pixel(enc(v));
            let back = ((a as u16 / 17) << 12) | ((r as u16 / 17) << 8) | ((g as u16 / 17) << 4) | (b as u16 / 17);
            assert_eq!(back, v);
        }
    }

    #[test]
    fn ugs_plain_sequence() {
        let mut d = frame(2, 1, &[0xFF00, 0x0000], false);
        d.extend(frame(1, 2, &[0xF00F, 0x8888], false));
        let frames = decode_ugs(&d).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!((frames[0].width, frames[0].height), (2, 1));
        assert_eq!(frames[0].pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(frames[0].pixel(1, 0), [0, 0, 0, 0]);
        assert_eq!(frames[1].pixel(0, 1), [136, 136, 136, 136]);
        assert!(!is_additive(&frames));
    }

    #[test]
    fn ugs_repeated_header() {
        let mut d = frame(2, 2, &[0xF111, 0xF222, 0xF333, 0xF444], true);
        d.extend(frame(1, 1, &[0xFFFF], true));
        let frames = decode_ugs(&d).unwrap();
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0].pixel(1, 1), [68, 68, 68, 255]);
        assert_eq!(frames[1].pixel(0, 0), [255; 4]);
    }

    #[test]
    fn ugs_first_pixels_like_a_header_stay_plain() {
        // Pixels that decode to the frame size must not be taken as a repeated header.
        let size_as_pixel = (2u16 ^ 0xAAAA).rotate_right(3);
        let mut d = frame(2, 1, &[size_as_pixel, (1u16 ^ 0xAAAA).rotate_right(3)], false);
        d.extend(frame(2, 1, &[0, 0], false));
        assert_eq!(&d[4..8], &[2, 0, 1, 0]);
        assert_eq!(decode_ugs(&d).unwrap().len(), 2);
    }

    #[test]
    fn stnd_stills_are_argb4444() {
        // 2×2: transparent, the dark red of Stnd-1's inside (0xF700), its ivory (0xFFFC),
        // and a colour with alpha 0 (drawn at 8 of 31).
        let mut d = vec![2, 0, 2, 0];
        for v in [0x0000u16, 0xF700, 0xFFFC, 0x0F00] {
            d.extend(v.to_le_bytes());
        }
        let img = decode_ugs_argb4444(&d).unwrap();
        assert_eq!(img.pixel(0, 0), [0, 0, 0, 0]);
        assert_eq!(img.pixel(1, 0), [119, 0, 0, 255]);
        assert_eq!(img.pixel(0, 1), [255, 255, 204, 255]);
        assert_eq!(img.pixel(1, 1), [255, 0, 0, 65]);
        assert!(decode_ugs_argb4444(&[2, 0, 2, 0, 1]).is_err());
    }

    #[test]
    fn ugs_errors_and_additive() {
        assert!(matches!(decode_ugs(&[1]), Err(DtError::Truncated { .. })));
        let d = frame(4, 4, &[0; 3], false);
        assert!(matches!(decode_ugs(&d), Err(DtError::Image { offset: 0, .. })));
        let fx = decode_ugs(&frame(2, 1, &[0x0F00, 0x0000], false)).unwrap();
        assert!(is_additive(&fx));
    }

    #[test]
    fn objects_slot_table() {
        let mut d = vec![0u8; 8]; // two empty slots
        let rec = |cat: u32, idx: u32, extra: &[u8], px: &[u16]| {
            let mut v = Vec::new();
            for x in [cat, idx, 2, px.len() as u32, 1] {
                v.extend_from_slice(&x.to_le_bytes());
            }
            v.extend_from_slice(extra);
            v.extend(frame(px.len() as u16, 1, px, false));
            v
        };
        d.extend(rec(9, 12, &[1, 2, 3, 4], &[0xF00F, 0xF0F0]));
        d.extend(rec(9, 13, &[5, 6, 7, 8], &[0xFF00, 0xFF00]));
        d.extend([0; 4]);
        d.extend(rec(3, 1, &[2, 0, 0, 0, 3, 0, 0, 0], &[0xFFFF, 0x8000]));
        let o = ObjectSprites::parse(&d).unwrap();
        assert_eq!(o.sprites.len(), 3);
        assert_eq!(o.sprites.iter().map(|s| s.section).collect::<Vec<_>>(), [0, 0, 1]);
        let tree = o.decoration(9, 13).unwrap();
        assert_eq!((tree.extra.as_slice(), tree.image.pixel(0, 0)), (&[5u8, 6, 7, 8][..], [255, 0, 0, 255]));
        let castle = o.building(3, 1).unwrap();
        assert_eq!(castle.image.pixel(1, 0), [0, 0, 0, 136]);
        assert!(o.decoration(3, 1).is_none() && o.building(9, 12).is_none());
        assert!(ObjectSprites::parse(&[1, 0, 0, 0, 5]).is_err());
    }

    fn lit_header(w: u32, h: u32, flags: u32) -> Vec<u8> {
        let mut v = b"LIT\0".to_vec();
        for x in [w, h, flags] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v
    }

    #[test]
    fn lit_raw() {
        let mut d = lit_header(2, 1, LIT_RAW);
        d.extend([128, 128, 128, 76, 85, 255]); // grey; JFIF pure red
        let img = decode_lit(&d).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.pixel(0, 0), [128, 128, 128, 255]);
        let [r, g, b, a] = img.pixel(1, 0);
        assert!(r >= 253 && g <= 2 && b <= 2 && a == 255, "{:?}", img.pixel(1, 0));
        assert!(matches!(decode_lit(&d[..20]), Err(DtError::Truncated { .. })));
        assert!(matches!(decode_lit(b"XYZ\0...."), Err(DtError::BadMagic { .. })));
    }

    /// A DCT plane whose blocks are flat with the given DC bytes (quant 16 at DC, so value =
    /// 2·DC; the bytes are signed, so a DC byte stays under 128).
    fn flat_plane(dcs: &[u8]) -> Vec<u8> {
        let mut v = vec![1u8; 64];
        v[0] = 16;
        v.extend(0..64u8); // identity rank
        v.extend_from_slice(dcs);
        v.extend(std::iter::repeat_n(0, 63 * dcs.len()));
        v
    }

    #[test]
    fn lit_dct_444_and_alpha() {
        // 10×3 pads to 16×8: two blocks per plane.
        let mut d = lit_header(10, 3, LIT_ALPHA);
        d.extend(flat_plane(&[50, 100]));
        d.extend(flat_plane(&[64, 64]));
        d.extend(flat_plane(&[64, 64]));
        d.extend(flat_plane(&[127, 4]));
        let img = decode_lit(&d).unwrap();
        assert_eq!((img.width, img.height), (10, 3));
        assert_eq!(img.pixel(0, 0), [100, 100, 100, 254]);
        assert_eq!(img.pixel(9, 2), [200, 200, 200, 8]);
        let mut long = d.clone();
        long.push(0);
        assert!(matches!(decode_lit(&long), Err(DtError::TrailingBytes { .. })));
    }

    #[test]
    fn lit_dct_slot_zero_is_signed() {
        // `Res-Magic`'s luma stores the first horizontal AC (pos 1) in slot 0 and the DC in
        // slot 1. A negative AC there must ramp up to the right, as the original reads every
        // byte signed (0x4778b4); read unsigned it made the grey stripes over the hand.
        let mut d = lit_header(8, 8, 0);
        let mut y = flat_plane(&[0]);
        y.swap(64, 65); // rank: pos 0 at slot 1, pos 1 at slot 0
        y.truncate(128);
        y.extend([0xEC, 50]); // slot 0: AC −20; slot 1: DC 50 (value 100)
        y.extend(std::iter::repeat_n(0, 62));
        d.extend(y);
        d.extend(flat_plane(&[64]));
        d.extend(flat_plane(&[64]));
        let img = decode_lit(&d).unwrap();
        let (left, right) = (img.pixel(0, 0)[0], img.pixel(7, 0)[0]);
        assert!(left < right && left > 80 && right < 120, "{left} {right}");
    }

    #[test]
    fn lit_chroma_smoothing() {
        // Bytes as the original's 8× IDCT keeps them: negatives dropped, then round half up.
        assert_eq!((plane_byte(-3.0), plane_byte(2.5), plane_byte(2.4), plane_byte(300.0)), (0, 3, 2, 255));
        // A block edge between 100 and 40 closes by a quarter of the step, toward zero.
        let mut p: Vec<u8> = (0..16 * 8).map(|i| if i % 16 < 8 { 100 } else { 40 }).collect();
        deblock(&mut p, 16, 8);
        assert_eq!(&p[6..10], &[100, 85, 55, 40]);
        let mut p: Vec<u8> = (0..16 * 8).map(|i| if i % 16 < 8 { 40 } else { 101 }).collect();
        deblock(&mut p, 16, 8);
        assert_eq!(&p[7..9], &[55, 86]); // (40 − 101) / 4 = −15
        // 4:2:0: doubled, then 1-2-1 along rows, the ends kept.
        let up = upsample(&[0, 100, 0, 100], 2, 2);
        assert_eq!(&up[..4], &[0, 25, 75, 100]);
        assert_eq!(up.len(), 16);
    }

    #[test]
    fn lit_dct_420_ac() {
        // 16×16 4:2:0: four Y blocks, one Cb and one Cr block.
        let mut d = lit_header(16, 16, LIT_420);
        let mut y = flat_plane(&[32, 32, 32, 32]);
        // First horizontal AC coefficient (pos 1, stored at rank 1) of block 0.
        y[128 + 4] = 20;
        d.extend(y);
        d.extend(flat_plane(&[64]));
        d.extend(flat_plane(&[114])); // Cr + 100: red tint
        let img = decode_lit(&d).unwrap();
        let (left, right) = (img.pixel(0, 0), img.pixel(7, 0));
        assert!(left[0] > right[0], "AC ramp {left:?} {right:?}");
        assert_eq!(img.pixel(8, 8), [204, 0, 64, 255]);
    }

    #[test]
    fn spi_and_bmp() {
        let mut d = Vec::new();
        for i in 0..4u8 {
            d.extend([i, 10, 20, 30 + i]);
        }
        let img = decode_spi(&d).unwrap();
        assert_eq!((img.width, img.height, img.pixel(1, 1)), (2, 2, [20, 10, 3, 33]));
        assert!(decode_spi(&[]).is_err());

        // 2×2 24-bit bottom-up BMP, rows padded to 8 bytes.
        let mut b = b"BM".to_vec();
        b.extend([0; 8]);
        b.extend(54u32.to_le_bytes());
        b.extend(40u32.to_le_bytes());
        b.extend(2i32.to_le_bytes());
        b.extend(2i32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(24u16.to_le_bytes());
        b.extend([0; 24]);
        b.extend([255, 0, 0, 0, 255, 0, 0, 0]); // bottom row: blue, green
        b.extend([0, 0, 255, 9, 9, 9, 0, 0]); // top row: red, grey
        let img = decode_bmp(&b).unwrap();
        assert_eq!(img.pixel(0, 0), [255, 0, 0, 255]);
        assert_eq!(img.pixel(1, 0), [9, 9, 9, 255]);
        assert_eq!(img.pixel(0, 1), [0, 0, 255, 255]);
        assert_eq!(decode_lit(&b).unwrap(), img);
    }

    /// A 3×2 8-bit BMP with a 2-colour palette (`biClrUsed` = 2), rows bottom-up padded to 4.
    fn bmp8(pixels_bottom_up: [[u8; 3]; 2], palette: [[u8; 3]; 2]) -> Vec<u8> {
        let mut b = b"BM".to_vec();
        b.extend([0; 8]);
        b.extend(999u32.to_le_bytes()); // bfOffBits: the original does not read it (0x47754c)
        b.extend(40u32.to_le_bytes());
        b.extend(3i32.to_le_bytes());
        b.extend(2i32.to_le_bytes());
        b.extend(1u16.to_le_bytes());
        b.extend(8u16.to_le_bytes());
        b.extend([0; 16]);
        b.extend(2u32.to_le_bytes()); // biClrUsed at 0x2e
        b.extend([0; 4]);
        for [r, g, bl] in palette {
            b.extend([bl, g, r, 0]);
        }
        for row in pixels_bottom_up {
            b.extend(row);
            b.push(0);
        }
        b
    }

    /// A mod's ability icon saved as an 8-bit BMP under a `.lit` name (a player's report):
    /// the original's BMP loader reads it (0x47754c).
    #[test]
    fn an_8_bit_bmp_reads_through_its_palette() {
        let b = bmp8([[0, 1, 0], [1, 1, 0]], [[255, 255, 255], [200, 30, 10]]);
        let img = decode_lit(&b).unwrap();
        assert_eq!((img.width, img.height), (3, 2));
        assert_eq!(img.pixel(0, 0), [200, 30, 10, 255], "the top row is the last stored");
        assert_eq!(img.pixel(2, 0), [255, 255, 255, 255]);
        assert_eq!(img.pixel(1, 1), [200, 30, 10, 255]);
        assert_eq!(img.pixel(0, 1), [255, 255, 255, 255]);
    }

    /// A header claiming a giant picture is refused before any pixel buffer is reserved.
    #[test]
    fn a_giant_bmp_header_is_refused_up_front() {
        let mut b = bmp8([[0, 1, 0], [1, 1, 0]], [[255, 255, 255], [200, 30, 10]]);
        b[18..22].copy_from_slice(&i32::MAX.to_le_bytes());
        b[22..26].copy_from_slice(&i32::MIN.wrapping_add(1).to_le_bytes());
        assert!(decode_bmp(&b).is_err());
    }

    /// The ability icons' key (0x48e8f0 → 0x4764c0, 0x47669c): the bottom-left pixel's
    /// 16-bit colour is transparent wherever it is, whatever it is (a mod's white, the shipped
    /// violet); a colour that differs only below 5-6-5 precision is keyed too, one that differs
    /// in it is not.
    #[test]
    fn ability_icons_key_their_bottom_left_colour_at_16_bits() {
        let mut white = decode_bmp(&bmp8([[0, 1, 0], [1, 1, 0]], [[255, 255, 255], [200, 30, 10]])).unwrap();
        key_bottom_left_565(&mut white);
        assert_eq!(white.pixel(0, 1), [0, 0, 0, 0], "the corner itself");
        assert_eq!(white.pixel(2, 0), [0, 0, 0, 0], "white elsewhere too");
        assert_eq!(white.pixel(1, 0), [206, 28, 8, 255], "the symbol kept, at 5-6-5 levels");
        let mut violet = Image { width: 2, height: 2, rgba: vec![129, 0, 127, 255, 133, 3, 120, 255, 128, 0, 127, 255, 137, 0, 127, 255] };
        key_bottom_left_565(&mut violet);
        assert_eq!(violet.pixel(0, 1), [0, 0, 0, 0]);
        assert_eq!(violet.pixel(0, 0), [0, 0, 0, 0], "129 and 128 are one 5-bit red");
        assert_eq!(violet.pixel(1, 0), [0, 0, 0, 0], "133, 3, 120: the same 5-6-5 colour");
        assert_eq!(violet.pixel(1, 1)[3], 255, "137 is the next 5-bit red: kept");
    }

    #[test]
    fn water_frame_names_and_clock() {
        assert_eq!(water_frame_file(0), "Graphics/Textures/Water/TEXTURE000.BMP");
        assert_eq!(water_frame_file(31), "Graphics/Textures/Water/TEXTURE031.BMP");
        assert_eq!(water_frame(0), 0);
        assert_eq!(water_frame(99), 0);
        assert_eq!(water_frame(100), 1);
        assert_eq!(water_frame(3199), 31);
        assert_eq!(water_frame(3200), 0);
    }

    #[test]
    fn icon_frames() {
        assert_eq!(item_icon_frame("A000.Tga"), Some(0));
        assert_eq!(item_icon_frame("A166.tga"), Some(166));
        assert_eq!(item_icon_frame(""), None);
        assert_eq!(item_icon_frame("Bxx.tga"), None);
        assert_eq!(portrait_frame(1), Some(0));
        assert_eq!(portrait_frame(0), None);
    }

    // ---------------------------------------------------------------- real files

    fn install() -> Option<DtInstall> {
        let dir = std::env::var_os(super::super::install::ENV_VAR)?;
        let mut dt = DtInstall::load(Path::new(&dir)).expect("install loads");
        dt.maps.retain(|m| !crate::dt::install::is_translated_copy(&m.name));
        Some(dt)
    }

    fn graphics_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                graphics_files(&p, out);
            } else if p.extension().is_some_and(|e| {
                let e = e.to_string_lossy().to_ascii_lowercase();
                e == "ugs" || e == "lit" || e == "spi"
            }) {
                out.push(p);
            }
        }
    }

    #[test]
    fn building_sprite_footprint() {
        let image = Image { width: 1, height: 1, rgba: vec![0; 4] };
        let mut extra = 4u32.to_le_bytes().to_vec();
        extra.extend(3u32.to_le_bytes());
        let s = ObjectSprite { section: ObjectSprite::BUILDINGS, cat: 9, idx: 0, cell: 1, extra, image };
        assert_eq!(s.footprint(), Some((4, 3)));
        let deco = ObjectSprite { section: ObjectSprite::DECORATIONS, extra: vec![1, 2, 3, 4], ..s.clone() };
        assert_eq!(deco.footprint(), None);
    }

    #[test]
    fn real_every_graphics_file_decodes() {
        let Some(dt) = install() else { return };
        let mut files = Vec::new();
        graphics_files(&dt.dir, &mut files);
        assert_eq!(files.len(), 421);
        let mut frames = 0;
        for f in &files {
            let imgs = decode_file(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
            assert!(!imgs.is_empty());
            frames += imgs.len();
            let name = f.file_name().unwrap().to_string_lossy().into_owned();
            let spell_fx = f.parent().is_some_and(|p| p.ends_with("Spells")) && name.ends_with(".ugs");
            if spell_fx || name == "--PAR.ugs" || name == "--RAYS.ugs" {
                assert!(is_additive(&imgs), "{name} should be additive");
            } else {
                assert!(!is_additive(&imgs), "{name} should not be additive");
            }
        }
        assert_eq!(frames, EXPECTED_TOTAL_FRAMES);
    }

    /// Output of the Python reference decoder (`gfx_decode.py`), computed once; numbers only, no
    /// content. (file, frames, first frame size, weighted sum of the first and the last frame,
    /// per-channel sums of the first frame). Covers every container variant and every LIT flag
    /// combination (0, 2, 4, 6, 8, 10). The Rust port matched it bit for bit on all 421 files;
    /// the DCT `.lit` rows were since redone for the original's byte planes and chroma
    /// smoothing (0x477850, 0x477ca4, 0x477de4), which the Python decoder lacks.
    #[allow(clippy::type_complexity)]
    const REFERENCE: &[(&str, usize, (u32, u32), u64, u64, [u64; 4])] = &[
    ("Graphics/Objects/Icons.ugs", 102, (92, 92), 77383125265, 68009623530, [1454809, 1221535, 1137878, 602531]),
    ("Graphics/Objects/Persones.ugs", 102, (115, 379), 408975547006, 341370711700, [3150015, 2436389, 1761829, 5206998]),
    ("Graphics/Objects/Items.ugs", 167, (53, 53), 3910203116, 3545636603, [129489, 121941, 114971, 240244]),
    ("Graphics/Objects/Objects.ugs", 370, (64, 50), 6523339144, 16568609938, [155312, 268464, 88468, 544935]),
    ("Graphics/Units/Knight.ugs", 64, (64, 64), 2699989334, 2928986389, [36737, 42636, 68051, 182104]),
    ("Graphics/Units/Ship-Pirat.ugs", 64, (128, 128), 20736281675, 23844361840, [113611, 111010, 99076, 271082]),
    ("Graphics/Spells/S-Fire.ugs", 50, (128, 128), 812918529, 134966689, [12665, 3944, 0, 0]),
    ("Graphics/Battle/--UPGRADE.ugs", 50, (220, 110), 44991272259, 47005134402, [262004, 232577, 285447, 145197]),
    ("Graphics/Battle/--RAYS.ugs", 25, (220, 110), 22109095204, 16476064624, [217566, 217566, 217566, 0]),
    // Grey + alpha (not in the Python decoder, which read it as a scrambled ARGB4444 frame).
    ("Graphics/Windows/Title_RUS.ugs", 1, (896, 128), 2728719528234, 2728719528234, [24779047, 24779047, 24779047, 8587170]),
    ("Graphics/Windows/Way_Arrows.ugs", 8, (32, 22), 124856534, 128991002, [23749, 23749, 23749, 40970]),
    ("Graphics/Textures/land.lit", 1, (256, 242), 802412639050, 802412639050, [3319887, 5835477, 675137, 15797760]),
    ("Graphics/Textures/plain.lit", 1, (256, 242), 903798187061, 903798187061, [6519060, 5687273, 879290, 15797760]),
    ("Graphics/Textures/detail.lit", 1, (256, 256), 1321690410939, 1321690410939, [7877932, 7877932, 7877932, 16711680]),
    ("Graphics/Windows/Hero0.lit", 1, (160, 160), 459766882056, 459766882056, [4008582, 3200106, 2714682, 6528000]),
    ("Logo/AE_Logo.lit", 1, (427, 589), 3994941246726, 3994941246726, [26056563, 20565478, 11666899, 64133265]),
    ("Graphics/Windows/Win-black.lit", 1, (480, 480), 2674358021228, 2674358021228, [7772338, 7772338, 7772338, 58752000]),
    ("Graphics/Editor/0.spi", 1, (22, 22), 183559428, 183559428, [20643, 40212, 28851, 99864]),
    ("Graphics/Editor/32.spi", 1, (22, 22), 148548932, 148548932, [17787, 17680, 17787, 99864]),
    ("Graphics/Spells/BigMoon.lit", 1, (100, 100), 74669189400, 74669189400, [391466, 391466, 391466, 2550000]),
    ("Graphics/Windows/Benguiat.lit", 1, (331, 120), 434788999716, 434788999716, [1227908, 1403917, 2317261, 10128600]),
    ("Graphics/Spells/Fireball.lit", 1, (100, 100), 74142934458, 74142934458, [429181, 429181, 429181, 2550000]),
    ("Graphics/Windows/DTSplashPBar.lit", 1, (666, 40), 139761721020, 139761721020, [641117, 641117, 641117, 4033196]),
    ];
    const EXPECTED_TOTAL_FRAMES: usize = 3059;

    /// The shipped ability icons on their violet: keyed by their bottom-left pixel as the
    /// original keys them (0x48e8f0), the background goes and the symbol stays.
    #[test]
    fn real_ability_icons_lose_their_background_to_the_corner_key() {
        let Some(dt) = install() else { return };
        let names = (1..=52).map(|n| format!("Graphics/Windows/Bonus{n}.lit")).chain((1..=3).map(|n| format!("Graphics/Windows/HeroBonus{n}.lit")));
        for rel in names.chain(["Graphics/Windows/Bonus-2Row.lit".to_string()]) {
            let mut img = dt.graphic(&rel).unwrap().swap_remove(0);
            key_bottom_left_565(&mut img);
            let n = img.rgba.len() / 4;
            let clear = img.rgba.chunks_exact(4).filter(|p| p[3] == 0).count();
            let violet = img.rgba.chunks_exact(4).filter(|p| p[3] != 0 && p[0] > 100 && p[1] < 40 && p[2] > 100).count();
            assert!(clear * 8 > n, "{rel}: the background goes ({clear} of {n})");
            assert!(clear * 10 < n * 9, "{rel}: the symbol stays ({clear} of {n})");
            // A few icons keep some violet: their matte is not one 16-bit colour in the
            // file (Bonus32, 34, 52), and the original keeps those pixels too.
            assert!(violet * 5 < n, "{rel}: {violet} violet pixels left");
        }
    }

    #[test]
    fn real_files_match_reference_decoder() {
        let Some(dt) = install() else { return };
        for &(rel, n, size, first, last, sums) in REFERENCE {
            let imgs = dt.graphic(rel).unwrap();
            assert_eq!(imgs.len(), n, "{rel}");
            assert_eq!((imgs[0].width, imgs[0].height), size, "{rel}");
            assert_eq!(weighted_sum(&imgs[0]), first, "{rel} first frame");
            assert_eq!(weighted_sum(imgs.last().unwrap()), last, "{rel} last frame");
            let mut got = [0u64; 4];
            for p in imgs[0].rgba.chunks_exact(4) {
                for (g, &b) in got.iter_mut().zip(p) {
                    *g += b as u64;
                }
            }
            assert_eq!(got, sums, "{rel} channel sums");
        }
    }

    #[test]
    fn real_res_magic_has_no_grey_stripes() {
        // The status bar's mana hand (0x4dcd8d, drawn by 0x49d3e5): its luma keeps an AC
        // coefficient in slot 0. Read unsigned, 47 opaque pixels came out light grey in two
        // vertical stripes; read signed, as the original does, none.
        let Some(dt) = install() else { return };
        let img = &dt.graphic("Graphics/Windows/Res-Magic.lit").unwrap()[0];
        let grey = img.rgba.chunks_exact(4).filter(|p| {
            let (hi, lo) = (p[0].max(p[1]).max(p[2]), p[0].min(p[1]).min(p[2]));
            p[3] > 200 && hi - lo < 40 && (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3 > 90
        });
        assert_eq!(grey.count(), 0);
    }

    #[test]
    fn real_resource_icons_carry_their_own_alpha() {
        // The status bar draws the four with nothing but their alpha planes (0x49d3e5), so
        // each must cut its own shape out: no box around it.
        let Some(dt) = install() else { return };
        for n in ["Magic", "Money", "Income", "Payment"] {
            let img = &dt.graphic(&format!("Graphics/Windows/Res-{n}.lit")).unwrap()[0];
            let clear = img.rgba.chunks_exact(4).filter(|p| p[3] < 32).count();
            assert!(clear * 10 >= img.rgba.len() / 4, "Res-{n}: {clear} clear pixels");
        }
    }

    #[test]
    fn real_art_lookups() {
        let Some(dt) = install() else { return };
        let portraits = dt.unit_portraits().unwrap();
        assert_eq!(portraits.len(), dt.units.len());
        assert!(portraits.iter().all(|p| (p.width, p.height) == (92, 92)));
        let figures = dt.unit_figures().unwrap();
        assert_eq!(figures.len(), dt.units.len());
        let items = dt.item_icons().unwrap();
        assert_eq!(items.len(), 167);
        // One frame per artefact, in GlobalIndex order (as the unit portraits): the helmets
        // A43–A48 are frames 42–47. Their `Icon=` numbers (A030 …) point elsewhere: frame 30
        // is a pair of arrows.
        for a in &dt.artefacts {
            let f = dt.artefact_icon_frame(a.id).unwrap_or_else(|| panic!("artefact {} icon {:?}", a.id, a.icon));
            assert_eq!(f, a.id as usize - 1, "artefact {}", a.id);
            assert!(f < items.len());
        }
        assert_eq!(dt.artefact_icon_frame(43), Some(42));
        for code in 0..16 {
            let t = dt.terrain_texture(code).unwrap();
            assert_eq!((t.width, t.height), (256, 242), "terrain {code}");
        }
        assert!(dt.terrain_texture(16).is_err());
        let water = dt.water_frames().unwrap();
        assert_eq!(water.len(), WATER_FRAMES);
        assert!(water.iter().all(|w| (w.width, w.height) == (64, 64)));
        let objects = dt.map_objects().unwrap();
        assert_eq!(objects.sprites.len(), 370);
        assert_eq!(objects.sprites.iter().filter(|s| s.section == ObjectSprite::BUILDINGS).count(), 85);
        // Every object and building of every map has a sprite: (class, sprite id) and
        // (picture type, variant) are the Objects.ugs keys.
        for m in &dt.maps {
            let s = m.load().unwrap();
            for o in &s.objects {
                assert!(objects.decoration(o.class, o.sprite).is_some(), "{}: object {o:?}", m.name);
            }
            for b in &s.buildings {
                let sprite = objects.building(b.picture_type, b.picture_variant);
                assert!(sprite.is_some(), "{}: building picture {} {}", m.name, b.picture_type, b.picture_variant);
                // The footprint of every building is its sprite's.
                assert_eq!(sprite.unwrap().footprint(), Some((b.size_x, b.size_y)), "{}: building footprint", m.name);
            }
        }
    }
}
