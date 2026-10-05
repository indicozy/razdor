//! The original's text markup (AddMarkupText 0x48e438 with its inner AddMarkupLine 0x48e1cc)
//! and the text list's wrapping and justification (TextList.AddText 0x47e46c,
//! TextDrawJustified 0x478a9c), as pure functions: no drawing, glyph widths come from the
//! caller.
//!
//! The original reads markup in four windows only: the event window's own text (an event's
//! message or question, 0x4a9d88 → 0x4aa3e6 / 0x4aa427; the tutorial offer, the village
//! offers and the unit's pay demand are event windows too), the restart box (0x4bf818) and the
//! delete-save box (0x4c05ac). Everywhere else (the journal 0x49c388, the victory and defeat
//! texts of the event window, 0x47ea80) the marks are drawn as typed.

/// A line's font: the original's four tints of `Benguiat.lit` (white glyphs, the loader
/// 0x4dbb0c adds a per-channel delta, clamped).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ink {
    /// No mark: font 0xae24a0, Benguiat with blue − 100 (255, 255, 155).
    Plain,
    /// `*`: font 0xae2498, the untinted Benguiat (white).
    Star,
    /// `|`: font 0xae24a8, red − 200, green − 75 (55, 180, 255).
    Bar,
    /// `@`: font 0xae24a4, green − 80, blue − 170 (255, 175, 85).
    At,
}

impl Ink {
    /// The colour of a full-white glyph in this font.
    pub fn rgb(self) -> [u8; 3] {
        match self {
            Ink::Plain => [255, 255, 155],
            Ink::Star => [254, 254, 254],
            Ink::Bar => [55, 180, 255],
            Ink::At => [255, 175, 85],
        }
    }
}

/// How a line is laid out in the text list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    /// `^`: centred (align 2).
    Centre,
    /// No `^`: justified (align 4), the text after [`INDENT`]; the last row of the line is
    /// left aligned (0x47e46c).
    Justify,
}

/// What 0x48e1cc puts before a line without `^`: six underscores. The font has no `_` glyph
/// and reuses the space's (0x47866c), so it is an indent of six spaces that justification
/// does not stretch (only real spaces stretch).
pub const INDENT: &str = "______";

/// One line of a marked-up text, its marks taken out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    /// The text as the original stores it: with [`INDENT`] in front for a justified line.
    pub text: String,
    pub ink: Ink,
    pub align: Align,
}

/// Splits `text` into lines and reads their marks, as AddMarkupText 0x48e438:
///
/// - lines end at CR LF or at `#\`; an empty text gives no line, a text ending in a break
///   gives an empty last line (it shows as a blank row);
/// - in each line every `*`, `|`, `@` and `^` is removed, wherever it stands; the font is that
///   of the last kind found in the order `*`, `|`, `@` (so `@` beats `|` beats `*`), else
///   [`Ink::Plain`]; a `^` anywhere centres the line, else the line is justified behind
///   [`INDENT`].
///
/// The original copies a line together with its break's first character (the CR or the `#`,
/// 0x48e1cc copies up to and including it); neither has a glyph, so it draws as nothing, and
/// it is left out here.
pub fn parse(text: &str) -> Vec<Line> {
    // 0x48e438: `if text <> nil`.
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut rest = text;
    loop {
        let brk = [rest.find("\r\n"), rest.find("#\\")].into_iter().flatten().min();
        match brk {
            Some(i) => {
                lines.push(line(&rest[..i]));
                rest = &rest[i + 2..];
            }
            None => {
                lines.push(line(rest));
                return lines;
            }
        }
    }
}

/// AddMarkupLine 0x48e1cc for one line.
fn line(raw: &str) -> Line {
    let mut ink = Ink::Plain;
    // The marks are looked for in this order, each found kind overwriting the font.
    for (mark, kind) in [('*', Ink::Star), ('|', Ink::Bar), ('@', Ink::At)] {
        if raw.contains(mark) {
            ink = kind;
        }
    }
    let text: String = raw.chars().filter(|c| !matches!(c, '*' | '|' | '@' | '^')).collect();
    if raw.contains('^') {
        Line { text, ink, align: Align::Centre }
    } else {
        Line { text: format!("{INDENT}{text}"), ink, align: Align::Justify }
    }
}

/// One row of the text list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub text: String,
    /// The last row of its line: a justified line's last row is left aligned.
    pub last: bool,
}

/// Word-wraps `text` into rows no wider than `max`, as TextList.AddText 0x47e46c: characters
/// are added until the width passes `max`, then the row ends after the last space seen and
/// loses its trailing spaces. An empty text is one empty row.
///
/// Kept from the original: the last space's place is not reset between rows, so a row with
/// no space of its own ends after the stale place (or after one character on the first row);
/// and after a run of spaces only the first one is skipped, the others start the next row.
pub fn wrap(text: &str, max: f32, width: impl Fn(char) -> f32) -> Vec<Row> {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return vec![Row { text: String::new(), last: true }];
    }
    let mut rows = Vec::new();
    let (mut start, mut space) = (0usize, 0usize);
    loop {
        let (mut w, mut k) = (0.0f32, 0usize);
        let end = loop {
            let c = chars.get(start + k).copied();
            if c == Some(' ') {
                space = k;
            }
            w += c.map_or(0.0, &width);
            if w > max || c.is_none() {
                break c.is_none();
            }
            k += 1;
        };
        let mut n = if end { k } else { space + 1 };
        n = n.min(chars.len() - start);
        let row = &chars[start..start + n];
        let text: String = if row.last() == Some(&' ') {
            let kept = row.iter().rposition(|&c| c != ' ').map_or(0, |p| p + 1);
            n = kept + 1;
            row[..kept].iter().collect()
        } else {
            row.iter().collect()
        };
        rows.push(Row { text, last: end });
        start += n;
        if end || start >= chars.len() {
            if !end {
                // A stale space place ran the row past the text's end, where 0x47e46c would
                // read on beyond the terminator: end with an empty last row instead.
                rows.push(Row { text: String::new(), last: true });
            }
            return rows;
        }
    }
}

/// The x of each character of a justified row filling `room`, as TextDrawJustified 0x478a9c:
/// the free room (`room` − the widths of the characters that are not spaces) is shared out
/// equally between the spaces in floating point, and each space puts the pen at the running
/// total rounded with RoundHalfUp 0x471d9c; other characters advance it by their width.
pub fn justify(text: &str, room: f32, width: impl Fn(char) -> f32) -> Vec<f32> {
    let spaces = text.chars().filter(|&c| c == ' ').count();
    let ink: f32 = text.chars().filter(|&c| c != ' ').map(&width).sum();
    let per = (room - ink) / spaces as f32;
    let (mut pos, mut at) = (0.0f32, 0.0f32);
    text.chars()
        .map(|c| {
            let x = at;
            if c == ' ' {
                pos += per;
                at = round_half_up(pos);
            } else {
                let w = width(c);
                pos += w;
                at += w;
            }
            x
        })
        .collect()
}

/// RoundHalfUp 0x471d9c: up when the first decimal is 5 or more, toward 0 otherwise.
pub fn round_half_up(v: f32) -> f32 {
    let i = v.trunc();
    if ((v - i) * 10.0).trunc() >= 5.0 {
        i + 1.0
    } else {
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn l(text: &str, ink: Ink, align: Align) -> Line {
        Line { text: text.to_string(), ink, align }
    }

    #[test]
    fn breaks_at_crlf_and_hash_backslash() {
        let lines = parse("a\r\nb#\\c");
        assert_eq!(lines, vec![l("______a", Ink::Plain, Align::Justify), l("______b", Ink::Plain, Align::Justify), l("______c", Ink::Plain, Align::Justify)]);
        // A bare LF or CR is no break.
        assert_eq!(parse("a\nb").len(), 1);
        assert_eq!(parse("a\rb").len(), 1);
    }

    #[test]
    fn empty_text_has_no_line_and_a_final_break_an_empty_one() {
        assert!(parse("").is_empty());
        assert_eq!(parse("a\r\n"), vec![l("______a", Ink::Plain, Align::Justify), l("______", Ink::Plain, Align::Justify)]);
        assert_eq!(parse("\r\n\r\n").len(), 3);
    }

    #[test]
    fn marks_go_wherever_they_stand() {
        assert_eq!(parse("*^Title"), vec![l("Title", Ink::Star, Align::Centre)]);
        assert_eq!(parse("^*Title"), vec![l("Title", Ink::Star, Align::Centre)]);
        assert_eq!(parse("a*b*c"), vec![l("______abc", Ink::Star, Align::Justify)]);
        assert_eq!(parse("end^"), vec![l("end", Ink::Plain, Align::Centre)]);
    }

    #[test]
    fn the_last_kind_in_the_order_star_bar_at_wins() {
        assert_eq!(parse("@*x")[0].ink, Ink::At);
        assert_eq!(parse("|*x")[0].ink, Ink::Bar);
        assert_eq!(parse("@|x")[0].ink, Ink::At);
        assert_eq!(parse("*x")[0].ink, Ink::Star);
    }

    #[test]
    fn the_delete_save_text_reads_as_the_original() {
        // The install's DeleteSave_Text, its #SAVENAME filled in first (0x471f0c).
        let lines = parse("^Вы действительно желаете удалить сохраненную игру #\\^|\"Слот 1\"?");
        assert_eq!(lines, vec![l("Вы действительно желаете удалить сохраненную игру ", Ink::Plain, Align::Centre), l("\"Слот 1\"?", Ink::Bar, Align::Centre)]);
    }

    #[test]
    fn wraps_after_the_last_space_and_trims() {
        let w = |_: char| 1.0;
        let rows = wrap("aaa bbb ccc", 5.0, w);
        assert_eq!(rows, vec![Row { text: "aaa".into(), last: false }, Row { text: "bbb".into(), last: false }, Row { text: "ccc".into(), last: true }]);
        assert_eq!(wrap("short", 50.0, w), vec![Row { text: "short".into(), last: true }]);
        assert_eq!(wrap("", 50.0, w), vec![Row { text: String::new(), last: true }]);
    }

    #[test]
    fn a_run_of_spaces_skips_only_one() {
        // "aaa   bbb", room 5: the break comes at the last space (index 5) → row "aaa",
        // and the next row starts after the first space only (0x47e46c's trim).
        let rows = wrap("aaa   bbb", 5.0, |_| 1.0);
        assert_eq!(rows[0].text, "aaa");
        assert_eq!(rows[1].text, "  bbb");
    }

    #[test]
    fn justify_shares_the_room_between_the_spaces() {
        // "ab cd ef": ink 6, room 12 → 3 per space.
        let xs = justify("ab cd ef", 12.0, |_| 1.0);
        assert_eq!(xs, vec![0.0, 1.0, 2.0, 5.0, 6.0, 7.0, 10.0, 11.0]);
        // Underscores (the indent) are not stretched.
        let xs = justify("__a b", 6.0, |_| 1.0);
        assert_eq!(xs, vec![0.0, 1.0, 2.0, 3.0, 5.0]);
    }

    #[test]
    fn round_half_up_reads_the_first_decimal() {
        assert_eq!(round_half_up(2.5), 3.0);
        assert_eq!(round_half_up(2.49), 2.0);
        assert_eq!(round_half_up(2.0), 2.0);
    }
}
