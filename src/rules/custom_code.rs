//! The custom battle's code (a Razdor extra, players' request of 2026-10-17): the whole setup
//! of a custom battle as one line of text to paste in a chat, read back by another player's
//! game. `RZB1:` and then, in base58 (letters and digits only: nothing a chat's markup eats,
//! and a double click selects it whole), a small binary record:
//!
//! - the content's fingerprint (2 bytes): which install or mod the code was made on, to warn
//!   when the other game's units and items may differ;
//! - flags (1 byte): bit 0 the wide row, bits 1 and 2 the AI plays the side below / above,
//!   bits 3–4 the battle AI's strength (0 normal, 1 improved, 2 maximum);
//! - each army, the side below first: its unit count (1 byte), then per unit its type id
//!   (LEB128), level (1 byte), item count (1 byte) and item ids (LEB128);
//! - a CRC-16 (CCITT) checksum of all of the above (2 bytes), so a cut or mistyped code is
//!   caught instead of loading something else.
//!
//! The number after `RZB` is the format's version: a later Razdor that changes the record
//! bumps it, and this one says so instead of misreading it.

use super::content::{Content, ItemId, UnitId};
use super::custom::{Control, Pick, Setup, MAX_LEVEL};

/// The code's start, its version included.
pub const PREFIX: &str = "RZB1:";
/// The format's version ([`PREFIX`]).
pub const VERSION: u32 = 1;
/// The most units an army may hold in a code (the formations' 12 cells).
const MAX_UNITS: usize = 12;
/// The most items a unit may wear in a code.
const MAX_ITEMS: usize = 16;
/// The longest record a code can hold: fingerprint, flags, two full armies of units with a
/// 5-byte type id, level, item count and 16 items of 5-byte ids each, checksum.
const MAX_RECORD: usize = 2 + 1 + 2 * (1 + MAX_UNITS * (5 + 1 + 1 + MAX_ITEMS * 5)) + 2;
/// Its base58 length with a margin (a byte is log 256 / log 58 ≈ 1.366 letters): a longer
/// body is refused before decoding, whose time grows with the square of the length (a huge
/// paste must not freeze the game).
pub const MAX_BODY: usize = MAX_RECORD * 138 / 100 + 2;

/// The bitcoin alphabet: no 0, O, I or l to mistake for each other.
const ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// Why a code was not read. Nothing was changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeError {
    /// The text holds no battle code.
    NotACode,
    /// A code of a later format (its version).
    Newer(u32),
    /// Cut, mistyped or altered: the checksum or the record does not hold.
    Damaged,
}

/// A code read: the setup (unknown units and items left out), the battle AI's strength
/// (0 normal, 1 improved, 2 maximum) and what did not fit this game.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decoded {
    pub setup: Setup,
    pub ai: u8,
    /// Units of types this content lacks (left out, with their items).
    pub unknown_units: usize,
    /// Items this content lacks, on the units kept (left out).
    pub unknown_items: usize,
    /// The code was made on content with other units or items (another install or mod).
    pub other_content: bool,
}

/// A short hash of the content's units and items (ids, hit points, kinds, prices): equal on
/// the same install and mod, different when a mod changed them.
pub fn fingerprint(content: &Content) -> u16 {
    let mut h: u32 = 0x811c_9dc5;
    let mut eat = |v: u32| {
        for b in v.to_le_bytes() {
            h ^= b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
    };
    for u in &content.units {
        eat(u.id);
        eat(u.hits as u32);
    }
    eat(0xffff_ffff);
    for d in &content.items {
        eat(d.id);
        eat(d.kind as u32);
        eat(d.cost as u32);
    }
    (h ^ (h >> 16)) as u16
}

/// The code of `setup` made on `content`, with the battle AI at strength `ai` (0–2).
pub fn encode(setup: &Setup, content: &Content, ai: u8) -> String {
    let mut b = Vec::new();
    b.extend_from_slice(&fingerprint(content).to_le_bytes());
    let mut flags = setup.wide as u8;
    if setup.control[0] == Control::Ai {
        flags |= 2;
    }
    if setup.control[1] == Control::Ai {
        flags |= 4;
    }
    flags |= ai.min(2) << 3;
    b.push(flags);
    for army in &setup.armies {
        let army = &army[..army.len().min(MAX_UNITS)];
        b.push(army.len() as u8);
        for p in army {
            put_varint(&mut b, p.unit.0);
            b.push(p.level.clamp(1, MAX_LEVEL) as u8);
            let items = &p.items[..p.items.len().min(MAX_ITEMS)];
            b.push(items.len() as u8);
            for i in items {
                put_varint(&mut b, i.0);
            }
        }
    }
    let sum = crc16(&b);
    b.extend_from_slice(&sum.to_le_bytes());
    format!("{PREFIX}{}", base58_encode(&b))
}

/// The code in `text` (a chat line pasted whole is fine: the code is found in it), read on
/// `content`.
pub fn decode(text: &str, content: &Content) -> Result<Decoded, CodeError> {
    let body = find_code(text)?;
    let bytes = base58_decode(&body).ok_or(CodeError::Damaged)?;
    if bytes.len() < 2 + 1 + 2 + 2 {
        return Err(CodeError::Damaged);
    }
    let (data, sum) = bytes.split_at(bytes.len() - 2);
    if crc16(data).to_le_bytes() != sum {
        return Err(CodeError::Damaged);
    }
    let mut r = Reader { b: data, at: 0 };
    let fp = u16::from_le_bytes([r.byte()?, r.byte()?]);
    let flags = r.byte()?;
    let ai = flags >> 3;
    if ai > 2 {
        return Err(CodeError::Damaged);
    }
    let control = |bit: u8| if flags & bit != 0 { Control::Ai } else { Control::Player };
    let mut out = Decoded {
        setup: Setup { armies: [Vec::new(), Vec::new()], control: [control(2), control(4)], wide: flags & 1 != 0 },
        ai,
        unknown_units: 0,
        unknown_items: 0,
        other_content: fp != fingerprint(content),
    };
    for side in 0..2 {
        let n = r.byte()? as usize;
        if n > MAX_UNITS {
            return Err(CodeError::Damaged);
        }
        for _ in 0..n {
            let unit = UnitId(r.varint()?);
            let level = r.byte()? as i32;
            if !(1..=MAX_LEVEL).contains(&level) {
                return Err(CodeError::Damaged);
            }
            let k = r.byte()? as usize;
            if k > MAX_ITEMS {
                return Err(CodeError::Damaged);
            }
            let mut items = Vec::with_capacity(k);
            for _ in 0..k {
                items.push(ItemId(r.varint()?));
            }
            if content.try_unit(unit).is_none() {
                out.unknown_units += 1;
                continue;
            }
            let known = items.len();
            items.retain(|&i| content.try_item(i).is_some());
            out.unknown_items += known - items.len();
            out.setup.armies[side].push(Pick { unit, level, items });
        }
    }
    if r.at != data.len() {
        return Err(CodeError::Damaged);
    }
    Ok(out)
}

/// The base58 part of the first code in `text`: after `RZB<version>:` (any case), the run of
/// base58 letters, line breaks skipped (a chat that wraps a long line).
fn find_code(text: &str) -> Result<String, CodeError> {
    let lower = text.to_ascii_lowercase();
    let mut newer = None;
    let mut from = 0;
    while let Some(at) = lower[from..].find("rzb") {
        let start = from + at + 3;
        from = start;
        let digits: String = lower[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() || !lower[start + digits.len()..].starts_with(':') {
            continue;
        }
        let version: u32 = digits.parse().unwrap_or(u32::MAX);
        if version != VERSION {
            if version > VERSION && newer.is_none() {
                newer = Some(version);
            }
            continue;
        }
        let body: String = text[start + digits.len() + 1..].chars().filter(|&c| c != '\n' && c != '\r').take_while(|&c| c.is_ascii() && ALPHABET.contains(&(c as u8))).take(MAX_BODY + 1).collect();
        return if body.is_empty() || body.len() > MAX_BODY { Err(CodeError::Damaged) } else { Ok(body) };
    }
    Err(newer.map_or(CodeError::NotACode, CodeError::Newer))
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, CodeError> {
        let v = *self.b.get(self.at).ok_or(CodeError::Damaged)?;
        self.at += 1;
        Ok(v)
    }

    fn varint(&mut self) -> Result<u32, CodeError> {
        let mut v: u64 = 0;
        for shift in (0..35).step_by(7) {
            let b = self.byte()?;
            v |= ((b & 0x7f) as u64) << shift;
            if b & 0x80 == 0 {
                return u32::try_from(v).map_err(|_| CodeError::Damaged);
            }
        }
        Err(CodeError::Damaged)
    }
}

fn put_varint(b: &mut Vec<u8>, mut v: u32) {
    while v >= 0x80 {
        b.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    b.push(v as u8);
}

/// CRC-16/CCITT-FALSE.
fn crc16(b: &[u8]) -> u16 {
    let mut crc: u16 = 0xffff;
    for &x in b {
        crc ^= (x as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

fn base58_encode(bytes: &[u8]) -> String {
    let zeros = bytes.iter().take_while(|&&b| b == 0).count();
    // Base-58 digits, least significant first.
    let mut digits: Vec<u8> = Vec::new();
    for &byte in &bytes[zeros..] {
        let mut carry = byte as u32;
        for d in digits.iter_mut() {
            carry += (*d as u32) << 8;
            *d = (carry % 58) as u8;
            carry /= 58;
        }
        while carry > 0 {
            digits.push((carry % 58) as u8);
            carry /= 58;
        }
    }
    let mut s = String::with_capacity(zeros + digits.len());
    s.extend(std::iter::repeat_n('1', zeros));
    s.extend(digits.iter().rev().map(|&d| ALPHABET[d as usize] as char));
    s
}

fn base58_decode(s: &str) -> Option<Vec<u8>> {
    let zeros = s.bytes().take_while(|&c| c == b'1').count();
    // Bytes, least significant first.
    let mut bytes: Vec<u8> = Vec::new();
    for c in s.bytes().skip(zeros) {
        let mut carry = ALPHABET.iter().position(|&a| a == c)? as u32;
        for b in bytes.iter_mut() {
            carry += *b as u32 * 58;
            *b = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            bytes.push(carry as u8);
            carry >>= 8;
        }
    }
    let mut out = vec![0; zeros];
    out.extend(bytes.iter().rev());
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::battle::Team;
    use crate::rules::content::testkit::*;
    use crate::rules::content::ArtefactType;
    use crate::rules::custom::choices;

    fn sample() -> (Content, Setup) {
        let c = content(
            vec![warrior(10, 30, 5), shooter(11, 20), warrior(300, 50, 9)],
            vec![item(200, ArtefactType::BlowWeapon), item(201, ArtefactType::Amulet), item(5000, ArtefactType::Ring)],
        );
        let mut s = Setup { armies: [vec![Pick::new(UnitId(10)), Pick::new(UnitId(300))], vec![Pick::new(UnitId(11))]], control: [Control::Player, Control::Ai], wide: false };
        s.set_level(Team::Player, 1, 30);
        s.set_level(Team::Enemy, 0, 7);
        s.wear(&c, Team::Player, 0, ItemId(200)).unwrap();
        s.wear(&c, Team::Player, 0, ItemId(201)).unwrap();
        s.wear(&c, Team::Player, 0, ItemId(5000)).unwrap();
        (c, s)
    }

    #[test]
    fn a_code_brings_back_the_whole_setup() {
        let (c, s) = sample();
        for ai in 0..3 {
            let code = encode(&s, &c, ai);
            assert!(code.starts_with(PREFIX));
            assert!(code[PREFIX.len()..].chars().all(|ch| ch.is_ascii_alphanumeric()), "chat-safe: {code}");
            let d = decode(&code, &c).unwrap();
            assert_eq!(d, Decoded { setup: s.clone(), ai, unknown_units: 0, unknown_items: 0, other_content: false });
        }
        let mut both = s.clone();
        both.control = [Control::Ai, Control::Player];
        both.wide = true;
        assert_eq!(decode(&encode(&both, &c, 1), &c).unwrap().setup, both);
    }

    #[test]
    fn a_full_setup_on_the_demo_stays_short() {
        let c = Content::builtin();
        let mut s = Setup::new(&c);
        let types = choices(&c);
        for (n, &u) in types.iter().cycle().take(24).enumerate() {
            s.add(if n % 2 == 0 { Team::Player } else { Team::Enemy }, u);
        }
        for t in Team::BOTH {
            for i in 0..s.army(t).len() {
                s.set_level(t, i, 1 + i as i32 * 2);
            }
        }
        let code = encode(&s, &c, 2);
        assert!(code.len() < 120, "{} chars: {code}", code.len());
        assert_eq!(decode(&code, &c).unwrap().setup, s);
    }

    #[test]
    fn a_code_is_found_in_a_chat_line() {
        let (c, s) = sample();
        let code = encode(&s, &c, 0);
        let (head, tail) = code.split_at(12);
        for text in [format!("  {code}\n"), format!("смотри какой бой: `{code}` удачи"), format!("{head}\n{tail}"), format!("rzb1:{}", &code[PREFIX.len()..])] {
            assert_eq!(decode(&text, &c).unwrap().setup, s, "{text:?}");
        }
    }

    #[test]
    fn a_broken_or_foreign_code_is_refused() {
        let (c, s) = sample();
        let code = encode(&s, &c, 0);
        assert_eq!(decode("", &c), Err(CodeError::NotACode));
        assert_eq!(decode("hello there", &c), Err(CodeError::NotACode));
        assert_eq!(decode("RZB:abc", &c), Err(CodeError::NotACode));
        assert_eq!(decode("RZB7:3mJr7AoUXx2Wqd", &c), Err(CodeError::Newer(7)));
        assert_eq!(decode("RZB1:", &c), Err(CodeError::Damaged));
        assert_eq!(decode(&code[..code.len() - 3], &c), Err(CodeError::Damaged), "cut short");
        assert_eq!(decode(&format!("{code}2"), &c), Err(CodeError::Damaged), "a letter too many");
        // Every single-letter typo is caught.
        let body = &code[PREFIX.len()..];
        for (i, ch) in body.char_indices() {
            let other = if ch == 'z' { 'y' } else { 'z' };
            let typo = format!("{PREFIX}{}{other}{}", &body[..i], &body[i + 1..]);
            assert!(decode(&typo, &c).is_err(), "typo at {i}: {typo}");
        }
        // A well-formed record with a nonsense value: refused, not clamped.
        let mut b = vec![0, 0, 0, 1];
        put_varint(&mut b, 10);
        b.extend_from_slice(&[99, 0, 0]);
        let sum = crc16(&b);
        b.extend_from_slice(&sum.to_le_bytes());
        assert_eq!(decode(&format!("{PREFIX}{}", base58_encode(&b)), &c), Err(CodeError::Damaged), "level 99");
    }

    #[test]
    fn units_and_items_of_another_install_are_reported_and_left_out() {
        let (c, s) = sample();
        let code = encode(&s, &c, 1);
        let other = content(vec![warrior(10, 30, 5), shooter(11, 20)], vec![item(200, ArtefactType::BlowWeapon), item(201, ArtefactType::Amulet)]);
        let d = decode(&code, &other).unwrap();
        assert!(d.other_content);
        assert_eq!((d.unknown_units, d.unknown_items), (1, 1), "unit 300 and ring 5000");
        assert_eq!(d.setup.army(Team::Player).len(), 1);
        assert_eq!(d.setup.army(Team::Player)[0].items, vec![ItemId(200), ItemId(201)]);
        assert_eq!(d.setup.army(Team::Enemy), s.army(Team::Enemy));
        assert_eq!(d.ai, 1);
    }

    #[test]
    fn the_largest_code_reads_and_an_oversized_one_is_refused_at_once() {
        // The largest record: two full armies of the biggest ids, each unit with 16 items.
        let units: Vec<_> = (0..12).map(|n| warrior(u32::MAX - n, 30, 5)).collect();
        let items: Vec<_> = (0..16).map(|n| item(u32::MAX - n, ArtefactType::Ring)).collect();
        let c = content(units, items);
        let pick = |n: u32| Pick { unit: UnitId(u32::MAX - n), level: MAX_LEVEL, items: (0..16).map(|i| ItemId(u32::MAX - i)).collect() };
        let army: Vec<Pick> = (0..12).map(pick).collect();
        let s = Setup { armies: [army.clone(), army], control: [Control::Ai, Control::Ai], wide: true };
        let code = encode(&s, &c, 2);
        assert!(code.len() - PREFIX.len() <= MAX_BODY, "{} > {MAX_BODY}", code.len() - PREFIX.len());
        assert_eq!(decode(&code, &c).unwrap().setup, s);
        // A paste of megabytes: refused without decoding it.
        let huge = format!("{PREFIX}{}", "z".repeat(4_000_000));
        let t = std::time::Instant::now();
        assert_eq!(decode(&huge, &c), Err(CodeError::Damaged));
        assert_eq!(decode(&format!("{PREFIX}{}", "z".repeat(MAX_BODY + 1)), &c), Err(CodeError::Damaged));
        assert!(t.elapsed().as_millis() < 1000, "{:?}", t.elapsed());
    }

    #[test]
    fn base58_round_trips() {
        for b in [vec![], vec![0], vec![0, 0, 1], vec![255; 40], (0..=255).collect::<Vec<u8>>()] {
            assert_eq!(base58_decode(&base58_encode(&b)).unwrap(), b);
        }
        assert_eq!(base58_decode("0OIl"), None);
    }
}
