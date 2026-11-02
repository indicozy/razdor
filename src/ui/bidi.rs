//! Right-to-left text (Razdor's own, for the community's translated maps and mods: the
//! original's texts are Russian and Latin). A line with a strong right-to-left character
//! (Hebrew, Arabic, Syriac and the other R or AL classes of `unicode-bidi`) is laid out by the
//! Unicode bidi algorithm: its runs in visual order, left to right on screen. A right-to-left
//! run's own neutral characters (spaces, brackets) are drawn reversed and mirrored
//! ([`mirror`]); its letters are shaped by rustybuzz, which already gives their glyphs in
//! visual order (`script_font`). A line with no such character never reaches this module
//! (`has_rtl` decides), so Russian and English keep their own path.
//!
//! Pure: no drawing, no fonts (the pieces of a line, by font, are `script_font::visual_pieces`).

use std::ops::Range;

use unicode_bidi::{bidi_class, BidiClass, BidiInfo};

/// Whether `c` is a strong right-to-left character: a letter of Hebrew, Arabic, Syriac, Thaana
/// and the other right-to-left scripts (bidi classes R and AL).
pub fn strong_rtl(c: char) -> bool {
    matches!(bidi_class(c), BidiClass::R | BidiClass::AL)
}

/// Whether `s` has a strong right-to-left character: such a line takes the bidi path.
pub fn has_rtl(s: &str) -> bool {
    s.chars().any(strong_rtl)
}

/// Whether the paragraph of `s` reads right to left: its first strong character is one (UAX #9,
/// rules P2 and P3). A line of neutrals only reads left to right.
pub fn base_rtl(s: &str) -> bool {
    for c in s.chars() {
        match bidi_class(c) {
            BidiClass::L => return false,
            BidiClass::R | BidiClass::AL => return true,
            _ => {}
        }
    }
    false
}

/// The visual order of `s` read as one line: its runs of one embedding level, in the order they
/// stand on screen from left to right, each as its byte range in `s` and whether it is right to
/// left. `abc` gives one left-to-right run; Arabic with digits and Latin gives the runs of
/// `unicode-bidi`'s `visual_runs`, the digits and the Latin word left to right in their place.
pub fn visual_order(s: &str) -> Vec<(Range<usize>, bool)> {
    let info = BidiInfo::new(s, None);
    let mut out = Vec::new();
    for para in &info.paragraphs {
        let (levels, runs) = info.visual_runs(para, para.range.clone());
        out.extend(runs.into_iter().map(|run| {
            let rtl = levels[run.start].is_rtl();
            (run, rtl)
        }));
    }
    out
}

/// The mirrored form of a bracket or a quotation mark, drawn for a character read the other way
/// round (BidiMirroring): `(` for `)`, `«` for `»`. Other characters are themselves.
pub fn mirror(c: char) -> char {
    match c {
        '(' => ')',
        ')' => '(',
        '[' => ']',
        ']' => '[',
        '{' => '}',
        '}' => '{',
        '<' => '>',
        '>' => '<',
        '«' => '»',
        '»' => '«',
        '‹' => '›',
        '›' => '‹',
        c => c,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts<'s>(s: &'s str, order: &[(Range<usize>, bool)]) -> Vec<(&'s str, bool)> {
        order.iter().map(|(r, rtl)| (&s[r.clone()], *rtl)).collect()
    }

    #[test]
    fn latin_is_one_left_to_right_run() {
        let s = "abc";
        assert_eq!(visual_order(s), vec![(0..3, false)]);
        assert!(!has_rtl(s) && !base_rtl(s));
    }

    #[test]
    fn arabic_alone_is_one_right_to_left_run() {
        let s = "مرحبا بك";
        assert_eq!(visual_order(s), vec![(0..s.len(), true)]);
        assert!(has_rtl(s) && base_rtl(s));
    }

    #[test]
    fn arabic_with_digits_and_latin_keeps_the_digits_and_the_word_left_to_right() {
        let s = "مرحبا 123 Razdor";
        // The Arabic word and the space before the digits are one right-to-left run; the
        // digits and the Latin word are left-to-right runs, the Latin word first on screen.
        assert_eq!(texts(s, &visual_order(s)), vec![("Razdor", false), (" ", true), ("123", false), ("مرحبا ", true)]);
        assert!(base_rtl(s));
    }

    #[test]
    fn hebrew_with_parentheses_is_one_right_to_left_run_and_digits_go_left() {
        let s = "שלום (עולם) 123";
        assert_eq!(texts(s, &visual_order(s)), vec![("123", false), ("שלום (עולם) ", true)]);
    }

    #[test]
    fn a_latin_line_with_an_arabic_word_reads_left_to_right_with_the_word_right_to_left() {
        let s = "Hero مرحبا";
        assert_eq!(texts(s, &visual_order(s)), vec![("Hero ", false), ("مرحبا", true)]);
        assert!(!base_rtl(s) && has_rtl(s));
    }

    #[test]
    fn neutrals_and_digits_do_not_make_a_line_right_to_left() {
        let s = "123 ()!";
        assert!(!has_rtl(s) && !base_rtl(s));
        // Arabic-Indic digits are numbers, not letters: no right-to-left text by themselves.
        assert!(!has_rtl("٠١٢"));
    }

    #[test]
    fn brackets_and_quotes_mirror_in_pairs() {
        assert_eq!(mirror('('), ')');
        assert_eq!(mirror(')'), '(');
        assert_eq!(mirror('['), ']');
        assert_eq!(mirror(']'), '[');
        assert_eq!(mirror('«'), '»');
        assert_eq!(mirror('»'), '«');
        assert_eq!(mirror('a'), 'a');
        assert_eq!(mirror('.'), '.');
    }
}
