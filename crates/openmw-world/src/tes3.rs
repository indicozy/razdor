//! The TES3 plugin format (`.omwaddon`): the writer for the records of M1 (TES3, LTEX, CELL, LAND),
//! a reader of the same framing for tests, and the Windows-1251 string encoding.
//!
//! Layouts are little-endian (spec §2.2): a record is `name[4] size u32 zero u32 flags u32`, a
//! subrecord is `name[4] size u32` and its data. Strings are single-byte cp1251.

use std::fmt;

/// Vertices per land side (65x65 per cell).
pub const LAND_VERTS: usize = 65;
/// Heights are stored as multiples of this many units (VHGT counts in these steps).
pub const HEIGHT_STEP: i32 = 8;
/// World-map cells sampled per WNAM side (the 9x9 grid).
pub const WNAM_SIDE: usize = 9;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sub {
    pub name: [u8; 4],
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub name: [u8; 4],
    pub flags: u32,
    pub subs: Vec<Sub>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Tes3Error {
    Truncated,
    /// Two neighbouring heights differ by more than the 127 steps VHGT can hold.
    HeightStep { row: usize, col: usize, diff: i64 },
    BadLength { name: &'static str, expected: usize, got: usize },
}

impl fmt::Display for Tes3Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Tes3Error::Truncated => write!(f, "plugin data is truncated"),
            Tes3Error::HeightStep { row, col, diff } => {
                write!(f, "land vertex ({col}, {row}) differs from its neighbour by {diff} units (max 1016)")
            }
            Tes3Error::BadLength { name, expected, got } => {
                write!(f, "{name} is {got} bytes, expected {expected}")
            }
        }
    }
}

impl Record {
    pub fn new(name: &[u8; 4]) -> Record {
        Record { name: *name, flags: 0, subs: Vec::new() }
    }

    pub fn sub(mut self, name: &[u8; 4], data: Vec<u8>) -> Record {
        self.subs.push(Sub { name: *name, data });
        self
    }

    /// The data of the first subrecord `name`.
    pub fn get(&self, name: &[u8; 4]) -> Option<&[u8]> {
        self.subs.iter().find(|s| &s.name == name).map(|s| s.data.as_slice())
    }
}

/// Encodes records into plugin bytes.
pub fn encode(records: &[Record]) -> Vec<u8> {
    let mut out = Vec::new();
    for rec in records {
        let size: usize = rec.subs.iter().map(|s| 8 + s.data.len()).sum();
        out.extend_from_slice(&rec.name);
        out.extend_from_slice(&(size as u32).to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&rec.flags.to_le_bytes());
        for s in &rec.subs {
            out.extend_from_slice(&s.name);
            out.extend_from_slice(&(s.data.len() as u32).to_le_bytes());
            out.extend_from_slice(&s.data);
        }
    }
    out
}

/// Decodes plugin bytes into records (the inverse of [`encode`]).
pub fn decode(bytes: &[u8]) -> Result<Vec<Record>, Tes3Error> {
    let mut records = Vec::new();
    let mut pos = 0;
    while pos < bytes.len() {
        let header = take(bytes, &mut pos, 16)?;
        let name = [header[0], header[1], header[2], header[3]];
        let size = u32_at(header, 4) as usize;
        let flags = u32_at(header, 12);
        let body = take(bytes, &mut pos, size)?;
        let mut subs = Vec::new();
        let mut at = 0;
        while at < body.len() {
            let sh = take(body, &mut at, 8)?;
            let sname = [sh[0], sh[1], sh[2], sh[3]];
            let len = u32_at(sh, 4) as usize;
            let data = take(body, &mut at, len)?;
            subs.push(Sub { name: sname, data: data.to_vec() });
        }
        records.push(Record { name, flags, subs });
    }
    Ok(records)
}

fn take<'a>(bytes: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], Tes3Error> {
    let end = pos.checked_add(n).ok_or(Tes3Error::Truncated)?;
    if end > bytes.len() {
        return Err(Tes3Error::Truncated);
    }
    let out = &bytes[*pos..end];
    *pos = end;
    Ok(out)
}

fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn i32_at(b: &[u8], at: usize) -> i32 {
    u32_at(b, at) as i32
}

/// Windows-1251 bytes of `text`: ASCII, the Cyrillic letters А–я, Ё and ё; any other character
/// becomes `?`.
pub fn encode_cp1251(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| match c {
            '\0' => b'?',
            c if (c as u32) < 0x80 => c as u8,
            'Ё' => 0xA8,
            'ё' => 0xB8,
            c @ '\u{410}'..='\u{44F}' => (c as u32 - 0x410 + 0xC0) as u8,
            _ => b'?',
        })
        .collect()
}

/// The text of Windows-1251 bytes (the inverse of [`encode_cp1251`] on its own alphabet).
pub fn decode_cp1251(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0..=0x7F => b as char,
            0xA8 => 'Ё',
            0xB8 => 'ё',
            0xC0..=0xFF => char::from_u32(u32::from(b) - 0xC0 + 0x410).unwrap_or('?'),
            _ => '?',
        })
        .collect()
}

/// A zero-terminated cp1251 string.
pub fn zstring(text: &str) -> Vec<u8> {
    let mut v = encode_cp1251(text);
    v.push(0);
    v
}

/// The string of a zero-terminated cp1251 field.
pub fn read_zstring(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    decode_cp1251(&bytes[..end])
}

/// The TES3 header record: HEDR (version 1.3, plugin type 0, the description, the number of records
/// after this one) and one master.
pub fn tes3_header(num_records: u32, master: &str, master_size: u64) -> Record {
    let mut hedr = Vec::with_capacity(300);
    hedr.extend_from_slice(&1.3f32.to_le_bytes());
    hedr.extend_from_slice(&0u32.to_le_bytes());
    hedr.extend_from_slice(&fixed(&[], 32));
    hedr.extend_from_slice(&fixed(&encode_cp1251("Razdor: DT map converted to OpenMW"), 256));
    hedr.extend_from_slice(&num_records.to_le_bytes());
    Record::new(b"TES3")
        .sub(b"HEDR", hedr)
        .sub(b"MAST", zstring(master))
        .sub(b"DATA", master_size.to_le_bytes().to_vec())
}

/// Pads or cuts `bytes` to `n` bytes, keeping at least one zero at the end.
fn fixed(bytes: &[u8], n: usize) -> Vec<u8> {
    let mut v: Vec<u8> = bytes.iter().copied().take(n - 1).collect();
    v.resize(n, 0);
    v
}

/// An LTEX record: its id, its index and its texture path (relative to `textures/`).
pub fn ltex(index: u32, id: &str, path: &str) -> Record {
    Record::new(b"LTEX")
        .sub(b"NAME", zstring(id))
        .sub(b"INTV", index.to_le_bytes().to_vec())
        .sub(b"DATA", zstring(path))
}

/// An exterior CELL with no references: an empty name, flags 0x02 (has water), its grid cell.
pub fn cell(gx: i32, gy: i32) -> Record {
    let mut data = Vec::with_capacity(12);
    data.extend_from_slice(&0x02u32.to_le_bytes());
    data.extend_from_slice(&gx.to_le_bytes());
    data.extend_from_slice(&gy.to_le_bytes());
    Record::new(b"CELL").sub(b"NAME", vec![0]).sub(b"DATA", data)
}

/// One reference of a CELL (spec §2.2): its plugin-wide index (from 1), its base object's id, its
/// position in units and its yaw in radians (the only rotation we write). Scale 1 writes no XSCL.
#[derive(Debug, Clone, PartialEq)]
pub struct Reference {
    pub frmr: u32,
    pub base: String,
    pub pos: [f32; 3],
    pub yaw: f32,
}

/// An exterior CELL with its references (FRMR, NAME, DATA per reference, in the order given).
pub fn cell_with_refs(gx: i32, gy: i32, refs: &[Reference]) -> Record {
    let mut rec = cell(gx, gy);
    for r in refs {
        let mut data = Vec::with_capacity(24);
        for v in [r.pos[0], r.pos[1], r.pos[2], 0.0, 0.0, r.yaw] {
            data.extend_from_slice(&v.to_le_bytes());
        }
        rec = rec
            .sub(b"FRMR", r.frmr.to_le_bytes().to_vec())
            .sub(b"NAME", zstring(&r.base))
            .sub(b"DATA", data);
    }
    rec
}

/// A STAT record: its id and the model path relative to `meshes/`.
pub fn stat(id: &str, model: &str) -> Record {
    Record::new(b"STAT").sub(b"NAME", zstring(id)).sub(b"MODL", zstring(model))
}

/// A LAND record of cell `(cx, cy)`: `heights` are 65x65 vertices in units, row-major (row 0 =
/// south, index `row * 65 + col`); `texels` are the 256 VTEX values, already in file order.
pub fn land(cx: i32, cy: i32, heights: &[i32], texels: &[u16; 256]) -> Result<Record, Tes3Error> {
    let flat = vec![[0i8, 0, 127]; LAND_VERTS * LAND_VERTS];
    land_with_normals(cx, cy, heights, &flat, texels)
}

/// [`land`] with the vertex normals given (`normals[row * 65 + col]` as x, y, z; OpenMW's VNML).
pub fn land_with_normals(
    cx: i32,
    cy: i32,
    heights: &[i32],
    normals: &[[i8; 3]],
    texels: &[u16; 256],
) -> Result<Record, Tes3Error> {
    let n = LAND_VERTS * LAND_VERTS;
    if heights.len() != n {
        return Err(Tes3Error::BadLength { name: "LAND heights", expected: n, got: heights.len() });
    }
    if normals.len() != n {
        return Err(Tes3Error::BadLength { name: "LAND normals", expected: n, got: normals.len() });
    }
    let mut intv = Vec::with_capacity(8);
    intv.extend_from_slice(&cx.to_le_bytes());
    intv.extend_from_slice(&cy.to_le_bytes());
    let vnml: Vec<u8> = normals.iter().flat_map(|v| v.iter().map(|&c| c as u8)).collect();
    let mut vtex = Vec::with_capacity(512);
    for v in texels {
        vtex.extend_from_slice(&v.to_le_bytes());
    }
    Ok(Record::new(b"LAND")
        .sub(b"INTV", intv)
        .sub(b"DATA", 5u32.to_le_bytes().to_vec())
        .sub(b"VNML", vnml)
        .sub(b"VHGT", encode_vhgt(heights)?)
        .sub(b"WNAM", wnam(heights))
        .sub(b"VTEX", vtex))
}

/// VHGT (spec §2.2): `offset` (f32) is the first height in steps of 8; each row's first delta is
/// taken from the row below's first vertex, every other delta from the vertex to its left. Heights
/// are rounded to steps of 8 first.
pub fn encode_vhgt(heights: &[i32]) -> Result<Vec<u8>, Tes3Error> {
    let n = LAND_VERTS * LAND_VERTS;
    if heights.len() != n {
        return Err(Tes3Error::BadLength { name: "VHGT", expected: n, got: heights.len() });
    }
    let q: Vec<i64> = heights.iter().map(|&h| (i64::from(h) + 4).div_euclid(i64::from(HEIGHT_STEP))).collect();
    let mut deltas = vec![0i64; n];
    for row in 0..LAND_VERTS {
        for col in 0..LAND_VERTS {
            let at = row * LAND_VERTS + col;
            let d = if col > 0 {
                q[at] - q[at - 1]
            } else if row > 0 {
                q[at] - q[at - LAND_VERTS]
            } else {
                0
            };
            if !(-128..=127).contains(&d) {
                return Err(Tes3Error::HeightStep { row, col, diff: d * i64::from(HEIGHT_STEP) });
            }
            deltas[at] = d;
        }
    }
    let mut out = Vec::with_capacity(4 + n + 3);
    out.extend_from_slice(&(q[0] as f32).to_le_bytes());
    out.extend(deltas.iter().map(|&d| d as i8 as u8));
    out.extend_from_slice(&[0, 0, 0]);
    Ok(out)
}

/// Heights (units) of a VHGT subrecord, the inverse of [`encode_vhgt`].
pub fn decode_vhgt(data: &[u8]) -> Result<Vec<i32>, Tes3Error> {
    let n = LAND_VERTS * LAND_VERTS;
    if data.len() != 4 + n + 3 {
        return Err(Tes3Error::BadLength { name: "VHGT", expected: 4 + n + 3, got: data.len() });
    }
    let offset = f32::from_le_bytes([data[0], data[1], data[2], data[3]]).round() as i64;
    let mut out = vec![0i32; n];
    let mut row_start = offset;
    for row in 0..LAND_VERTS {
        let first = row * LAND_VERTS;
        if row > 0 {
            row_start += i64::from(data[4 + first] as i8);
        }
        let mut run = row_start;
        out[first] = (run * i64::from(HEIGHT_STEP)) as i32;
        for col in 1..LAND_VERTS {
            run += i64::from(data[4 + first + col] as i8);
            out[first + col] = (run * i64::from(HEIGHT_STEP)) as i32;
        }
    }
    Ok(out)
}

/// The 9x9 WNAM of a land (spec §2.2): one sample every 8 vertices, height / 128 above zero and
/// / 16 below, clamped to i8. OpenMW ignores it on load.
pub fn wnam(heights: &[i32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(WNAM_SIDE * WNAM_SIDE);
    for r in 0..WNAM_SIDE {
        for c in 0..WNAM_SIDE {
            let h = i64::from(heights[(r * 8) * LAND_VERTS + c * 8]);
            let v = if h > 0 { h / 128 } else { h / 16 };
            out.push(v.clamp(-128, 127) as i8 as u8);
        }
    }
    out
}

/// Reads the heights back from a LAND record (VHGT).
pub fn land_heights(rec: &Record) -> Result<Vec<i32>, Tes3Error> {
    decode_vhgt(rec.get(b"VHGT").ok_or(Tes3Error::Truncated)?)
}

/// Reads the (cx, cy) of a LAND record (INTV).
pub fn land_cell(rec: &Record) -> Result<(i32, i32), Tes3Error> {
    let d = rec.get(b"INTV").ok_or(Tes3Error::Truncated)?;
    if d.len() != 8 {
        return Err(Tes3Error::BadLength { name: "INTV", expected: 8, got: d.len() });
    }
    Ok((i32_at(d, 0), i32_at(d, 4)))
}

/// Reads the 256 VTEX values of a LAND record.
pub fn land_vtex(rec: &Record) -> Result<Vec<u16>, Tes3Error> {
    let d = rec.get(b"VTEX").ok_or(Tes3Error::Truncated)?;
    if d.len() != 512 {
        return Err(Tes3Error::BadLength { name: "VTEX", expected: 512, got: d.len() });
    }
    Ok(d.chunks(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect())
}

/// The texture index of an LTEX record (INTV), for tests.
pub fn ltex_index(rec: &Record) -> Option<u32> {
    rec.get(b"INTV").filter(|d| d.len() == 4).map(|d| u32_at(d, 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(h: i32) -> Vec<i32> {
        vec![h; LAND_VERTS * LAND_VERTS]
    }

    /// A surface with the steps of a slope and a bump, all differences under 1016.
    fn rough() -> Vec<i32> {
        let mut v = vec![0i32; LAND_VERTS * LAND_VERTS];
        for row in 0..LAND_VERTS {
            for col in 0..LAND_VERTS {
                v[row * LAND_VERTS + col] = 96 + (col as i32) * 8 - (row as i32) * 16 + ((row * 7 + col * 3) % 5) as i32 * 24;
            }
        }
        v
    }

    #[test]
    fn vhgt_round_trips_a_flat_land() {
        let h = flat(96);
        assert_eq!(decode_vhgt(&encode_vhgt(&h).unwrap()).unwrap(), h);
    }

    #[test]
    fn vhgt_round_trips_a_rough_land() {
        let h = rough();
        let bytes = encode_vhgt(&h).unwrap();
        assert_eq!(bytes.len(), 4232);
        assert_eq!(decode_vhgt(&bytes).unwrap(), h);
    }

    #[test]
    fn vhgt_rounds_to_steps_of_8_and_refuses_big_steps() {
        let mut h = flat(96);
        h[5] = 101; // rounds to 104
        let back = decode_vhgt(&encode_vhgt(&h).unwrap()).unwrap();
        assert_eq!(back[5], 104);
        let mut steep = flat(0);
        steep[65 + 1] = 2000; // 250 steps from its neighbours
        assert!(matches!(encode_vhgt(&steep), Err(Tes3Error::HeightStep { .. })));
    }

    #[test]
    fn a_land_record_reads_back_its_fields() {
        let mut texels = [0u16; 256];
        texels[7] = 3;
        let rec = land(2, -1, &rough(), &texels).unwrap();
        assert_eq!(land_cell(&rec).unwrap(), (2, -1));
        assert_eq!(land_heights(&rec).unwrap(), rough());
        assert_eq!(land_vtex(&rec).unwrap()[7], 3);
        assert_eq!(rec.get(b"VNML").unwrap().len(), 65 * 65 * 3);
        assert_eq!(&rec.get(b"VNML").unwrap()[..3], &[0, 0, 127]);
        assert_eq!(rec.get(b"WNAM").unwrap().len(), 81);
        assert_eq!(rec.get(b"DATA").unwrap(), &5u32.to_le_bytes());
    }

    #[test]
    fn every_record_written_reads_back_equal() {
        let mut texels = [0u16; 256];
        texels[0] = 18;
        let records = vec![
            tes3_header(4, "template.omwgame", 12345),
            ltex(0, "rz_deep_sea", "razdor/surf_deep_sea.tga"),
            ltex(16, "rz_rock", "razdor/surf_rock.tga"),
            cell(0, 0),
            cell(-1, 3),
            land(0, 0, &rough(), &texels).unwrap(),
            land(-1, 3, &flat(-400), &[0u16; 256]).unwrap(),
        ];
        let bytes = encode(&records);
        let back = decode(&bytes).unwrap();
        assert_eq!(back, records);
        assert_eq!(encode(&back), bytes);
        assert_eq!(ltex_index(&back[2]), Some(16));
        assert_eq!(read_zstring(back[1].get(b"NAME").unwrap()), "rz_deep_sea");
    }

    #[test]
    fn the_header_has_300_byte_hedr_and_the_master() {
        let rec = tes3_header(7, "template.omwgame", 99);
        let hedr = rec.get(b"HEDR").unwrap();
        assert_eq!(hedr.len(), 300);
        assert_eq!(f32::from_le_bytes([hedr[0], hedr[1], hedr[2], hedr[3]]), 1.3);
        assert_eq!(u32_at(hedr, 296), 7);
        assert_eq!(read_zstring(rec.get(b"MAST").unwrap()), "template.omwgame");
        assert_eq!(rec.get(b"DATA").unwrap(), &99u64.to_le_bytes());
    }

    #[test]
    fn a_cell_is_exterior_with_flags_and_grid() {
        let c = cell(3, -2);
        assert_eq!(c.get(b"NAME").unwrap(), &[0u8]);
        let d = c.get(b"DATA").unwrap();
        assert_eq!(u32_at(d, 0), 0x02);
        assert_eq!(i32_at(d, 4), 3);
        assert_eq!(i32_at(d, 8), -2);
    }

    #[test]
    fn truncated_input_is_an_error() {
        let bytes = encode(&[cell(0, 0)]);
        assert_eq!(decode(&bytes[..bytes.len() - 1]), Err(Tes3Error::Truncated));
    }

    #[test]
    fn cp1251_round_trips_cyrillic_and_ascii() {
        let text = "Раздор: Ёж ёлка, ЯЩЕРИЦА я! ABC 123";
        let bytes = encode_cp1251(text);
        assert_eq!(bytes.len(), text.chars().count());
        assert_eq!(bytes[text.chars().position(|c| c == 'Ё').unwrap()], 0xA8);
        assert_eq!(decode_cp1251(&bytes), text);
    }

    #[test]
    fn other_characters_become_question_marks() {
        assert_eq!(encode_cp1251("a€ß中"), b"a???".to_vec());
    }
}
