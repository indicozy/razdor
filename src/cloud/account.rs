//! The account key (`docs/superpowers/specs/2026-11-22-cloud-saves-design.md`, Account): a random
//! secret the game keeps in its cloud folder (0600 on unix) and sends with every keyed request. It is
//! never the analytics install id. The recovery phrase is the same key written so a player can type
//! it: 52 base-31 digits (31^52 > 2^256) and one check digit, in the link code's alphabet.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use razdor_wire::ids::AccountKey;
use razdor_wire::limits::LINK_ALPHABET;
use sha2::{Digest, Sha256};

/// The key file's name in the cloud folder.
pub const KEY_FILE: &str = "account.key";
/// Bytes of a key: 256 random bits (the server takes at least 128).
pub const KEY_BYTES: usize = 32;
/// Base-31 digits of a key (31^52 > 2^256).
const DATA_DIGITS: usize = 52;
/// Characters of a phrase: the digits and the check digit.
pub const PHRASE_CHARS: usize = DATA_DIGITS + 1;

/// A new random key, as lowercase hex (64 digits).
pub fn new_key() -> Result<String, String> {
    let mut raw = [0u8; KEY_BYTES];
    getrandom::fill(&mut raw).map_err(|e| format!("random: {e}"))?;
    Ok(hex(&raw))
}

/// The device file's name in the cloud folder.
pub const DEVICE_FILE: &str = "device.id";

/// This device's id: 16 random bytes as 32 lowercase hex, made once and kept in the cloud folder. A
/// missing or bad file gets a new id (the account sees a new device, and the old device's rows stay
/// as they are: they are never deleted by this one).
pub fn device_id(cloud_dir: &Path) -> Result<String, String> {
    let path = cloud_dir.join(DEVICE_FILE);
    if let Ok(text) = std::fs::read_to_string(&path) {
        let id = text.trim();
        if id.len() == 32 && id.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return Ok(id.to_string());
        }
    }
    let mut raw = [0u8; 16];
    getrandom::fill(&mut raw).map_err(|e| format!("random: {e}"))?;
    let id = hex(&raw);
    std::fs::create_dir_all(cloud_dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, &id).map_err(|e| e.to_string())?;
    Ok(id)
}

/// The key file's path in `cloud_dir`.
pub fn key_path(cloud_dir: &Path) -> PathBuf {
    cloud_dir.join(KEY_FILE)
}

/// The stored key, if there is one and it is well formed.
pub fn load_key(cloud_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(key_path(cloud_dir)).ok()?;
    let key = text.trim().to_string();
    AccountKey::parse(&key).ok()?;
    Some(key)
}

/// Writes the key: a temporary file created 0600 on unix, then renamed over the key file.
pub fn store_key(cloud_dir: &Path, key: &str) -> std::io::Result<()> {
    AccountKey::parse(key).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    std::fs::create_dir_all(cloud_dir)?;
    let tmp = cloud_dir.join(format!("{KEY_FILE}.tmp"));
    let _ = std::fs::remove_file(&tmp);
    {
        let mut file = open_private(&tmp)?;
        file.write_all(key.as_bytes())?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, key_path(cloud_dir))
}

/// Removes the key (the account is gone, or this device left it). A missing file is not an error.
pub fn forget_key(cloud_dir: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(key_path(cloud_dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(unix)]
pub(crate) fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

#[cfg(not(unix))]
pub(crate) fn open_private(path: &Path) -> std::io::Result<std::fs::File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

/// The recovery phrase of a 32-byte key (64 hex digits): 53 characters in groups of four, the last
/// one the check digit.
pub fn phrase(key: &str) -> Result<String, String> {
    let raw = unhex(key).filter(|b| b.len() == KEY_BYTES).ok_or("the key is not 32 bytes")?;
    let mut n = raw.clone();
    let mut digits = Vec::with_capacity(DATA_DIGITS + 1);
    for _ in 0..DATA_DIGITS {
        digits.push(div_small(&mut n, 31));
    }
    if n.iter().any(|&b| b != 0) {
        return Err("the key is too big for the phrase".into());
    }
    digits.reverse();
    digits.push(check_digit(&raw));
    let chars: Vec<char> = digits.iter().map(|&d| LINK_ALPHABET.as_bytes()[d as usize] as char).collect();
    Ok(chars.chunks(4).map(|c| c.iter().collect::<String>()).collect::<Vec<_>>().join("-"))
}

/// The key a recovery phrase gives back. Spaces, dashes and case do not matter; a typo is refused
/// by the check digit.
pub fn key_from_phrase(text: &str) -> Result<String, String> {
    let chars: Vec<char> = text
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if chars.len() != PHRASE_CHARS {
        return Err(format!("the phrase has {} characters, {PHRASE_CHARS} expected", chars.len()));
    }
    let mut values = Vec::with_capacity(PHRASE_CHARS);
    for c in &chars {
        let i = LINK_ALPHABET.find(*c).ok_or_else(|| format!("'{c}' is not in a phrase"))?;
        values.push(i as u8);
    }
    let (data, check) = values.split_at(DATA_DIGITS);
    let mut n = vec![0u8; KEY_BYTES + 1];
    for &d in data {
        if mul_add(&mut n, 31, d) {
            return Err("the phrase is out of range".into());
        }
    }
    if n[0] != 0 {
        return Err("the phrase is out of range".into());
    }
    let raw = &n[1..];
    if check_digit(raw) != check[0] {
        return Err("the phrase has a typo (its check digit does not match)".into());
    }
    let key = hex(raw);
    AccountKey::parse(&key).map_err(|e| e.to_string())?;
    Ok(key)
}

/// The check digit: the first byte of the key's SHA-256, modulo 31.
fn check_digit(raw: &[u8]) -> u8 {
    Sha256::digest(raw)[0] % 31
}

/// Divides a big-endian number by `d` in place; returns the remainder.
fn div_small(n: &mut [u8], d: u32) -> u8 {
    let mut rem = 0u32;
    for b in n.iter_mut() {
        let cur = rem * 256 + u32::from(*b);
        *b = (cur / d) as u8;
        rem = cur % d;
    }
    rem as u8
}

/// `n = n * m + a` on a big-endian number; true when the result does not fit.
fn mul_add(n: &mut [u8], m: u32, a: u8) -> bool {
    let mut carry = u32::from(a);
    for b in n.iter_mut().rev() {
        let cur = u32::from(*b) * m + carry;
        *b = (cur & 0xff) as u8;
        carry = cur >> 8;
    }
    carry != 0
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-cloud-account-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn phrase_round_trips_fixed_and_random_keys() {
        let fixed = [
            "00".repeat(32),
            "ff".repeat(32),
            "0123456789abcdef".repeat(4),
            "80".to_string() + &"00".repeat(31),
        ];
        for key in fixed.iter() {
            let p = phrase(key).expect("encodes");
            assert_eq!(p.chars().filter(|c| *c != '-').count(), PHRASE_CHARS);
            assert_eq!(&key_from_phrase(&p).expect("decodes"), key);
        }
        for _ in 0..50 {
            let key = new_key().expect("random");
            let p = phrase(&key).expect("encodes");
            assert_eq!(key_from_phrase(&p).expect("decodes"), key);
            // Spaces, lower case and missing dashes are read the same.
            let loose = p.replace('-', " ").to_lowercase();
            assert_eq!(key_from_phrase(&loose).expect("decodes loosely"), key);
        }
    }

    #[test]
    fn phrase_refuses_a_typo_a_short_phrase_and_a_foreign_character() {
        // A fixed key: the check digit detects this typo for every one of the 31 possible digits
        // only by chance, so the test uses a key whose result is known.
        let key = "0123456789abcdef".repeat(4);
        let p = phrase(&key).expect("encodes");
        let mut chars: Vec<char> = p.chars().collect();
        let i = chars.iter().position(|c| *c != '-').expect("a digit");
        chars[i] = if chars[i] == '2' { '3' } else { '2' };
        let bad: String = chars.into_iter().collect();
        assert!(key_from_phrase(&bad).is_err(), "a one-character typo must not decode to another key");
        assert!(key_from_phrase(&p[..p.len() - 3]).is_err());
        assert!(key_from_phrase(&p.replace(p.chars().next().expect("a char"), "0")).is_err());
    }

    #[test]
    fn phrase_needs_a_full_key() {
        assert!(phrase("abcd").is_err());
        assert!(phrase(&"zz".repeat(32)).is_err());
    }

    #[test]
    fn key_file_round_trips_and_is_private() {
        let dir = temp("file");
        assert_eq!(load_key(&dir), None);
        let key = new_key().expect("random");
        store_key(&dir, &key).expect("stores");
        assert_eq!(load_key(&dir).as_deref(), Some(key.as_str()));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(key_path(&dir)).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
        forget_key(&dir).expect("forgets");
        assert_eq!(load_key(&dir), None);
        forget_key(&dir).expect("a missing key is not an error");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_garbled_key_file_is_no_key() {
        let dir = temp("garbled");
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(key_path(&dir), "not a key").expect("write");
        assert_eq!(load_key(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
