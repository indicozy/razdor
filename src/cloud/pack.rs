//! The save bytes on the wire: zstd on the client (the server stores what it is sent), the SHA-256
//! of the packed bytes (the wire's `sha256`), and the limits both ways.

use std::io::Read;

use razdor_wire::limits::MAX_SAVE_BODY;
use sha2::{Digest, Sha256};

/// zstd's level: saves are small and rarely re-packed, so the better ratio is worth the time.
const LEVEL: i32 = 9;
/// The most a save may grow to when unpacked (a guard against a bad or hostile file).
const MAX_UNPACKED: u64 = 64 * 1024 * 1024;

/// The packed bytes of a save file. Deterministic for the same input and zstd version, so the same
/// save packs to the same bytes (and the same hash) on every device.
pub fn pack(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let packed = zstd::encode_all(bytes, LEVEL).map_err(|e| format!("zstd: {e}"))?;
    if packed.len() > MAX_SAVE_BODY {
        return Err(format!("the save packs to {} bytes, over the {MAX_SAVE_BODY} limit", packed.len()));
    }
    Ok(packed)
}

/// The save file a packed body holds.
pub fn unpack(packed: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    let decoder = zstd::stream::read::Decoder::new(packed).map_err(|e| format!("zstd: {e}"))?;
    decoder
        .take(MAX_UNPACKED + 1)
        .read_to_end(&mut out)
        .map_err(|e| format!("zstd: {e}"))?;
    if out.len() as u64 > MAX_UNPACKED {
        return Err("the unpacked save is over the limit".into());
    }
    Ok(out)
}

/// The lowercase hex SHA-256 of `bytes`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_round_trips_and_packs_the_same_way_twice() {
        let save: Vec<u8> = (0..50_000u32).map(|i| (i % 251) as u8).collect();
        let packed = pack(&save).expect("packs");
        assert!(packed.len() < save.len(), "a repetitive save shrinks");
        assert_eq!(unpack(&packed).expect("unpacks"), save);
        assert_eq!(pack(&save).expect("packs again"), packed, "the same bytes pack to the same bytes");
        assert_eq!(sha256_hex(b"abc").len(), 64);
        assert_eq!(&sha256_hex(b"abc")[..8], "ba7816bf");
    }

    #[test]
    fn garbage_is_refused_by_unpack() {
        assert!(unpack(b"not zstd at all").is_err());
    }
}
