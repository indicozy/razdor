//! The original's 16-bit subtraction: 0x476790 with its flag set (the list widget's text draw
//! mode 5, the parchment's grey bands and ornaments, the mode-1 blits). A screen pixel loses
//! each non-zero source pixel through two lookup tables that 0x474de4 builds for the screen's
//! masks (0x475ad0 sets them: red 0xF800, green 0x07E0, blue 0x001F, the 5-6-5 format; the
//! table code takes the branch of 0x4eceac < 8, which 5 bits of blue give).
//!
//! The pixel is taken as two bytes. The low table is indexed by the dst and src low bytes
//! (dst << 8 | src) and gives a 16-bit entry: the blue difference, saturated at 0, plus the
//! green's low three bits' difference, which borrows 0x300 when it goes negative. The high
//! table is indexed by the high bytes and read at byte offset `entry >> 8`, so a borrow of
//! 0x300 moves the read two bytes on, to the second of its two 16-bit words. Each word holds
//! the red difference (saturated) and the green's high three bits' difference, less one in
//! the second word, and a low byte that is 0xFF when the green stays positive and 0x1F when
//! it clamps, which also clears the clamped green bits. The result's high byte is the high
//! table's, its low byte the high table's AND the low table's.
//!
//! So red, green and blue all clamp at 0. The 0x300 borrow is green's, not blue's: a blue
//! underflow saturates just the same, and the grey parts of a glyph with a dark blue come out
//! as the original shows them.
//!
//! The byte-level layout is kept as the original has it (table words stored little-endian,
//! read at any byte offset), so the entries are the original's, not a per-channel rule.

use razdor::local;
use std::cell::OnceCell;

/// A 5-6-5 pixel from 8-bit channels, truncated as `gfx::quantize_565` rounds them.
pub fn pack565(r: u8, g: u8, b: u8) -> u16 {
    (((r >> 3) as u16) << 11) | (((g >> 2) as u16) << 5) | ((b >> 3) as u16)
}

/// The 8-bit channels a 5-6-5 pixel shows as: its top bits repeated into the low ones, as the
/// rest of Razdor widens 16-bit art.
pub fn unpack565(p: u16) -> [u8; 3] {
    let (r, g, b) = ((p >> 11) as u8, ((p >> 5) & 63) as u8, (p & 31) as u8);
    [(r << 3) | (r >> 2), (g << 2) | (g >> 4), (b << 3) | (b >> 2)]
}

/// The low table's entry for a dst byte `d` and a src byte `s`: blue saturated at 0, and the
/// green's low three bits (bits 5 to 7) taken with a 0x300 borrow when they go negative.
fn low_entry(d: u8, s: u8) -> u16 {
    let (dg, sg) = ((d & 0xE0) as u16, (s & 0xE0) as u16);
    let green = if dg < sg { 0x300 + dg - sg } else { dg - sg };
    let blue = (d & 31).saturating_sub(s & 31) as u16;
    green + blue
}

/// One word of the high table for the high bytes `d` and `s`: the red difference (bits 11 to
/// 15, saturated) and the green's high three bits' difference (bits 8 to 10). `second` takes
/// one more off the green (the word the 0x300 borrow reads). The low byte is 0xFF while the
/// green stays positive, 0x1F when it clamps (which also clears the green's low bits).
fn high_entry(d: u8, s: u8, second: bool) -> u16 {
    let red = ((d & 0xF8).saturating_sub(s & 0xF8) as u16) << 8;
    let (dg, sg) = ((d & 7) as u16, (s & 7) as u16 + second as u16);
    let (green, fill) = if dg >= sg { ((dg - sg) << 8, 0xFF) } else { (0, 0x1F) };
    (red | green) | fill
}

struct Tables {
    low: Vec<u16>,
    /// Four bytes per index (two words), plus room for the borrow's offset past the last one.
    high: Vec<u8>,
}

fn build() -> Tables {
    let mut low = vec![0u16; 1 << 16];
    let mut high = vec![0u8; 4 * (1 << 16) + 4];
    for i in 0..1usize << 16 {
        let (d, s) = ((i >> 8) as u8, i as u8);
        low[i] = low_entry(d, s);
        high[4 * i..4 * i + 2].copy_from_slice(&high_entry(d, s, false).to_le_bytes());
        high[4 * i + 2..4 * i + 4].copy_from_slice(&high_entry(d, s, true).to_le_bytes());
    }
    Tables { low, high }
}

local! {
    static TABLES: OnceCell<Tables> = const { OnceCell::new() };
}

/// `dst` with the 5-6-5 pixel `src` taken off, as 0x476790 does for one non-zero source pixel
/// in its subtractive mode. A zero source leaves `dst` as it is (the original skips it).
pub fn sub565(dst: u16, src: u16) -> u16 {
    if src == 0 {
        return dst;
    }
    TABLES.with(|t| {
        let t = t.get_or_init(build);
        let low = t.low[(((dst & 0xFF) << 8) | (src & 0xFF)) as usize];
        let at = 4 * ((((dst >> 8) << 8) | (src >> 8)) as usize) + (low >> 8) as usize;
        let high = u16::from_le_bytes([t.high[at], t.high[at + 1]]);
        (high & 0xFF00) | ((high & 0xFF) & (low & 0xFF))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5-6-5 pixel from its channels as the screen holds them (5, 6, 5 bits).
    fn px(r: u16, g: u16, b: u16) -> u16 {
        (r << 11) | (g << 5) | b
    }

    #[test]
    fn packing_truncates_and_widens_as_the_16_bit_art_does() {
        assert_eq!(pack565(240, 185, 125), px(30, 46, 15));
        assert_eq!(pack565(155, 255, 155), px(19, 63, 19));
        assert_eq!(unpack565(px(31, 63, 31)), [255, 255, 255]);
        assert_eq!(unpack565(px(0, 0, 0)), [0, 0, 0]);
    }

    #[test]
    fn a_zero_source_pixel_leaves_the_screen_alone() {
        assert_eq!(sub565(px(30, 46, 15), 0), px(30, 46, 15));
    }

    #[test]
    fn without_an_underflow_it_is_a_plain_channel_subtraction() {
        // Red 20-5, green 42-8 (with no borrow out of the low bits), blue 25-7.
        assert_eq!(sub565(px(20, 42, 25), px(5, 8, 7)), px(15, 34, 18));
    }

    #[test]
    fn green_borrowing_from_its_high_bits_is_still_a_plain_subtraction() {
        // Green 42-12: the low three bits borrow (0x300), the high ones come out 3 (30 in all).
        assert_eq!(sub565(px(20, 42, 25), px(5, 12, 7)), px(15, 30, 18));
    }

    #[test]
    fn red_and_green_clamp_at_zero() {
        // Red 4-9 clamps to 0; blue 10-3 = 7; green 16-0 = 16.
        assert_eq!(sub565(px(4, 16, 10), px(9, 0, 3)), px(0, 16, 7));
        // Green 9-20 clamps to 0 (its low bits are cleared with the high ones), red 10-2 = 8,
        // blue 20-3 = 17.
        assert_eq!(sub565(px(10, 9, 20), px(2, 20, 3)), px(8, 0, 17));
    }

    #[test]
    fn blue_underflow_saturates_at_zero_and_the_green_borrow_takes_the_ink() {
        // Paper (240, 185, 125) = 5-6-5 (30, 46, 15) minus the rumour's green font (155, 255,
        // 155) = (19, 63, 19). Hand trace of the tables:
        //  - low entry for blue 15 - 19: saturates to 0; green's low bits 110 - 111 (0xC0 -
        //    0xE0) borrow: 0x300 - 0x20 = 0x2E0, so the high byte read is 2 and the low
        //    byte 0xE0.
        //  - the high word read at byte offset 2 (the second word): red 30 - 19 = 11 (0x5800),
        //    green's high bits 5 against 7 + 1, which clamp: fill 0x1F.
        //  - result: high byte 0x58 (red 11, green high 0), low byte 0x1F & 0xE0 = 0.
        // So the result is (11, 0, 0) = 0x5800: blue stays 0 and does not wrap to 28.
        assert_eq!(sub565(px(30, 46, 15), px(19, 63, 19)), 0x5800);
        // The quest's yellow font (31, 63, 19) takes the red and blue to 0 as well.
        assert_eq!(sub565(px(30, 46, 15), px(31, 63, 19)), 0x0000);
    }

    #[test]
    fn the_stored_words_are_the_rules_at_their_indices() {
        let t = build();
        for &(d, s) in &[(0u8, 0u8), (0xCF, 0xF3), (0xF5, 0x9F), (0xFF, 0x00), (0x00, 0xFF)] {
            let i = ((d as usize) << 8) | s as usize;
            assert_eq!(t.low[i], low_entry(d, s));
            assert_eq!(&t.high[4 * i..4 * i + 2], &high_entry(d, s, false).to_le_bytes());
            assert_eq!(&t.high[4 * i + 2..4 * i + 4], &high_entry(d, s, true).to_le_bytes());
        }
    }
}
