//! The original's stream container (saves-data.md §9, read 0x473188): a 12-byte header and
//! bzip2 data. The `.DTm` maps use its legacy layout:
//!
//! | off | type | value |
//! |---|---|---|
//! | 0 | char[4] | magic: accepted when bytes 0, 2, 3 are `A`, `p`, `f` |
//! | 4 | u16 | legacy (byte 1 = `I`): ignored (`\r\n` in the maps); new layout: block size, KiB |
//! | 6 | u8 | legacy: compression code (above 10: bzip2, level code − 10; 19 in all shipped maps) |
//! | 7 | u8 | legacy: scramble mode (0 in everything shipped) |
//! | 8 | u32 | size of the uncompressed payload |
//! | 12 | … | legacy: one bzip2 stream to EOF; new: chunks of (u32 length, bzip2 stream) |

use super::DtError;
use std::io::{Read, Write};

/// Magic bytes this module writes, as the shipped maps have them.
pub const MAGIC: &[u8; 6] = b"AIpf\r\n";
/// Header length before the compressed data.
pub const HEADER_LEN: usize = 12;

/// A decoded container.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    /// Bytes 6–7 (compression code and scramble mode), kept to write the file back alike.
    pub version: u16,
    pub payload: Vec<u8>,
}

/// The magic the reader accepts: `A`, any byte, `p`, `f`.
pub fn has_magic(bytes: &[u8]) -> bool {
    bytes.len() >= 4 && bytes[0] == b'A' && bytes[2] == b'p' && bytes[3] == b'f'
}

fn bunzip(data: &[u8]) -> Result<Vec<u8>, DtError> {
    let mut out = Vec::new();
    bzip2::read::BzDecoder::new(data).read_to_end(&mut out).map_err(|e| DtError::Bzip2(e.to_string()))?;
    Ok(out)
}

/// Undo the scramble of mode 1: byte i was XORed with (i + 1) mod 256 (0x471be0). Mode 2
/// XORs with `Random(256)` from wherever the game's generator stood, which cannot be undone;
/// nothing the game ships or writes is scrambled.
fn unscramble(data: &mut [u8], mode: u8) -> Result<(), DtError> {
    match mode {
        0 => Ok(()),
        1 => {
            data.iter_mut().enumerate().for_each(|(i, b)| *b ^= (i as u8).wrapping_add(1));
            Ok(())
        }
        _ => Err(DtError::Bzip2(format!("scramble mode {mode} cannot be read"))),
    }
}

/// Parse a container and decompress its payload as the original's reader does. A payload
/// whose size differs from the header's is kept: the reader only raises a flag that no
/// loader checks.
pub fn decode(bytes: &[u8]) -> Result<Container, DtError> {
    if bytes.len() < HEADER_LEN {
        if bytes.len() < 4 || has_magic(bytes) {
            return Err(DtError::Truncated { what: "container header", offset: bytes.len() });
        }
        return Err(DtError::BadMagic { what: "AIpf container" });
    }
    if !has_magic(bytes) {
        return Err(DtError::BadMagic { what: "AIpf container" });
    }
    let version = u16::from_le_bytes([bytes[6], bytes[7]]);
    let size = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]) as usize;
    let payload = if bytes[1] == b'I' {
        // Legacy layout: the rest of the file is one stream, unscrambled first.
        let mut data = bytes[HEADER_LEN..].to_vec();
        unscramble(&mut data, bytes[7])?;
        bunzip(&data)?
    } else {
        // New layout (the game writes `AEpf`): with no block size one chunk follows, else
        // `size div block + 1` chunks.
        let block = u16::from_le_bytes([bytes[4], bytes[5]]) as usize * 1024;
        if bytes[6] >> 6 != 0 {
            return Err(DtError::Bzip2("scrambled chunks cannot be read".into()));
        }
        let chunks = if block == 0 { 1 } else { size / block + 1 };
        let mut at = HEADER_LEN;
        let mut out = Vec::with_capacity(size);
        for _ in 0..chunks {
            let len = bytes.get(at..at + 4).ok_or(DtError::Truncated { what: "container chunk", offset: at })?;
            let len = u32::from_le_bytes([len[0], len[1], len[2], len[3]]) as usize;
            let data = bytes.get(at + 4..at + 4 + len).ok_or(DtError::Truncated { what: "container chunk", offset: at + 4 })?;
            out.extend(bunzip(data)?);
            at += 4 + len;
        }
        out
    };
    Ok(Container { version, payload })
}

/// Build a container around `payload` (used by tests and future writers).
pub fn encode(version: u16, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER_LEN + payload.len() / 4);
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&version.to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    let mut enc = bzip2::write::BzEncoder::new(out, bzip2::Compression::best());
    enc.write_all(payload).expect("writing to a Vec cannot fail");
    enc.finish().expect("writing to a Vec cannot fail")
}

/// The Community editor's stream reader (DTMapEdit 0x4d54c4): a file shorter than 12 bytes
/// or without the `AIpf` magic is the raw payload; otherwise the scramble of byte 7 is undone
/// and byte 6 picks the decompressor: below 10 zlib, 10 to 19 bzip2 (code `C` counts as 0).
/// The size at byte 8 is not checked (the reader sets a flag nothing reads).
pub fn decode_editor(bytes: &[u8]) -> Result<Vec<u8>, DtError> {
    if bytes.len() < HEADER_LEN || &bytes[..4] != b"AIpf" {
        return Ok(bytes.to_vec());
    }
    let mut data = bytes[HEADER_LEN..].to_vec();
    unscramble(&mut data, bytes[7])?;
    match bytes[6] {
        b'C' | 0..=9 => {
            let mut out = Vec::new();
            flate2::read::ZlibDecoder::new(&data[..]).read_to_end(&mut out).map_err(|e| DtError::Zlib(e.to_string()))?;
            Ok(out)
        }
        10..=19 => bunzip(&data),
        code => Err(DtError::BadValue { section: "container".into(), key: "compression code".into(), value: code.to_string() }),
    }
}

/// A Community editor demo map's container (0x4d5790 mode 7): `AIpf`, code 9 (zlib deflate
/// at level 9), scramble mode 1, then the stream XORed byte by byte with (i + 1) mod 256.
pub fn encode_demo(payload: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::new(9));
    enc.write_all(payload).expect("writing to a Vec cannot fail");
    let mut data = enc.finish().expect("writing to a Vec cannot fail");
    data.iter_mut().enumerate().for_each(|(i, b)| *b ^= (i as u8).wrapping_add(1));
    let mut out = Vec::with_capacity(HEADER_LEN + data.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&[9, 1]);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend(data);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let payload = b"MapLDV V.4\r\nhello hello hello".to_vec();
        let bytes = encode(19, &payload);
        assert_eq!(&bytes[..6], MAGIC);
        assert_eq!(&bytes[12..15], b"BZh");
        let c = decode(&bytes).unwrap();
        assert_eq!(c, Container { version: 19, payload });
    }

    #[test]
    fn empty_payload() {
        let c = decode(&encode(1, &[])).unwrap();
        assert!(c.payload.is_empty());
    }

    #[test]
    fn rejects_bad_magic() {
        let mut bytes = encode(19, b"x");
        bytes[0] = b'B';
        assert!(matches!(decode(&bytes), Err(DtError::BadMagic { .. })));
        // Only bytes 0, 2 and 3 are checked; the rest of the legacy header is not.
        let mut bytes = encode(19, b"x");
        bytes[3] = b'F';
        assert!(matches!(decode(&bytes), Err(DtError::BadMagic { .. })));
        let mut bytes = encode(19, b"xyz");
        bytes[4..6].copy_from_slice(b"??");
        assert_eq!(decode(&bytes).unwrap().payload, b"xyz");
    }

    #[test]
    fn rejects_short_header() {
        assert!(matches!(decode(b"AIpf\r\n\x13"), Err(DtError::Truncated { .. })));
    }

    #[test]
    fn a_size_mismatch_is_not_checked() {
        // 0x473188 only sets a flag no loader reads.
        let mut bytes = encode(19, b"abcdef");
        bytes[8] = 5;
        assert_eq!(decode(&bytes).unwrap().payload, b"abcdef");
    }

    fn bz(data: &[u8]) -> Vec<u8> {
        let mut enc = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::fast());
        enc.write_all(data).unwrap();
        enc.finish().unwrap()
    }

    #[test]
    fn the_legacy_layout_may_be_scrambled_by_mode_1() {
        // Byte i of the stream XORed with (i + 1) mod 256 (0x471be0).
        let mut bytes = encode(19, b"scrambled payload");
        bytes[7] = 1;
        for (i, b) in bytes[HEADER_LEN..].iter_mut().enumerate() {
            *b ^= (i as u8).wrapping_add(1);
        }
        assert_eq!(decode(&bytes).unwrap().payload, b"scrambled payload");
        bytes[7] = 2;
        assert!(decode(&bytes).is_err(), "mode 2 used the game's random numbers");
    }

    #[test]
    fn the_new_layout_reads_chunks() {
        // `AEpf`: block size in KiB at 4, then `size div block + 1` chunks of (u32 length,
        // bzip2); with no block size, a single chunk.
        let payload: Vec<u8> = (0..3000u32).map(|i| (i % 251) as u8).collect();
        let head = |kib: u16| {
            let mut h = b"AEpf".to_vec();
            h.extend_from_slice(&kib.to_le_bytes());
            h.extend_from_slice(&[0x11, 0]);
            h.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            h
        };
        let chunk = |out: &mut Vec<u8>, data: &[u8]| {
            let z = bz(data);
            out.extend_from_slice(&(z.len() as u32).to_le_bytes());
            out.extend_from_slice(&z);
        };
        let mut one = head(0);
        chunk(&mut one, &payload);
        assert_eq!(decode(&one).unwrap().payload, payload);
        let mut blocks = head(1);
        for part in payload.chunks(1024) {
            chunk(&mut blocks, part);
        }
        assert_eq!(decode(&blocks).unwrap().payload, payload, "3000 div 1024 + 1 = 3 chunks");
        // A size of exactly one block: the writer writes one chunk, the reader wants two.
        let mut exact = head(1);
        exact[8..12].copy_from_slice(&1024u32.to_le_bytes());
        chunk(&mut exact, &payload[..1024]);
        assert!(matches!(decode(&exact), Err(DtError::Truncated { .. })));
    }

    #[test]
    fn the_editor_reads_raw_bzip2_and_zlib_maps() {
        let payload = b"MapLDV V.4\r\nsome map".to_vec();
        // No container, or too short for one: the bytes are the payload.
        assert_eq!(decode_editor(&payload).unwrap(), payload);
        assert_eq!(decode_editor(b"AIpf").unwrap(), b"AIpf");
        // The normal save: code 19, no scramble.
        assert_eq!(decode_editor(&encode(19, &payload)).unwrap(), payload);
        // The demo save: code 9, zlib, scramble 1.
        let demo = encode_demo(&payload);
        assert_eq!((&demo[..6], demo[6], demo[7]), (&MAGIC[..], 9, 1));
        assert_eq!(&demo[8..12], &(payload.len() as u32).to_le_bytes());
        // The first stream byte, a zlib header 0x78, is XORed with 1.
        assert_eq!(demo[12], 0x78 ^ 1);
        assert_eq!(decode_editor(&demo).unwrap(), payload);
        // Code 'C' is zlib too; codes from 20 are not known.
        let mut c = demo.clone();
        c[6] = b'C';
        assert_eq!(decode_editor(&c).unwrap(), payload);
        c[6] = 20;
        assert!(decode_editor(&c).is_err());
        // The game's reader takes only bzip2.
        assert!(decode(&demo).is_err());
    }

    #[test]
    fn rejects_corrupt_stream() {
        let mut bytes = encode(19, b"some payload bytes");
        let n = bytes.len();
        bytes.truncate(n - 8);
        assert!(matches!(decode(&bytes), Err(DtError::Bzip2(_))));
    }
}
