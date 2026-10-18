//! One map string on its way to the model and back: what must survive translation is
//! hidden behind placeholders, and the answer is checked before it is taken.
//!
//! What a map string may hold that is not prose (see `docs/reference/dtm-format.md` §7, §9 and
//! `crate::dt::markup`):
//! - line breaks: CR LF, or `#\` (the original's markup break);
//! - `#` words: `#HERONAME`, `#ARMYNAME`, archetype suffixes such as `#рыцарь`;
//! - markup marks `*`, `|`, `@`, `^`, which pick a line's font and alignment.
//!
//! The model sees CR LF as `\n`, `#\` as `<br>` and each `#` word as `{N}`; the marks are left
//! in the text and their counts must come back unchanged.

/// A source string ready for the model: `text` with `{N}` for `tokens[N]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prepared {
    pub text: String,
    pub tokens: Vec<String>,
}

use crate::i18n::tr;
use crate::trf;

/// The markup marks whose counts a translation must keep.
const MARKS: [char; 4] = ['*', '|', '@', '^'];

pub fn has_cyrillic(s: &str) -> bool {
    s.chars().any(|c| ('\u{0400}'..='\u{04FF}').contains(&c))
}

/// Hides line breaks and `#` words of `src` from the model.
pub fn prepare(src: &str) -> Prepared {
    let s = src.replace("\r\n", "\n").replace("#\\", "<br>");
    let mut text = String::with_capacity(s.len());
    let mut tokens = Vec::new();
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c == '#' && it.peek().is_some_and(|n| n.is_alphanumeric() || *n == '_') {
            let mut tok = String::from('#');
            while let Some(&n) = it.peek().filter(|n| n.is_alphanumeric() || **n == '_') {
                tok.push(n);
                it.next();
            }
            text.push_str(&format!("{{{}}}", tokens.len()));
            tokens.push(tok);
        } else {
            text.push(c);
        }
    }
    Prepared { text, tokens }
}

/// Why an answer for one text was refused, with what the source had and what came back, so
/// the translator's log can say it in the player's words ([`Refusal::message`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// Nothing but blanks.
    Empty,
    /// Placeholder `ph` (once in the source) came back `got` times.
    Placeholder { ph: String, got: usize },
    /// Cyrillic letters left in an answer in another language.
    Cyrillic,
    /// Counts of `<br>` (the markup break `#\`).
    Breaks { src: usize, got: usize },
    /// Counts of lines.
    Lines { src: usize, got: usize },
    /// Counts of a markup mark.
    Mark { mark: char, src: usize, got: usize },
    /// A character Windows-1251 lacks (the game's fonts and files cannot hold it).
    Character(char),
}

impl Refusal {
    /// The reason in the interface language, e.g. "markup mark `*`: 1 in the original, 0 in
    /// the translation".
    pub fn message(&self) -> String {
        match self {
            Refusal::Empty => tr("the answer is empty").to_string(),
            Refusal::Placeholder { ph, got } => trf!("placeholder {ph}: 1 in the original, {got} in the translation", ph, got),
            Refusal::Cyrillic => tr("Cyrillic letters left in the translation").to_string(),
            Refusal::Breaks { src, got } => trf!("line breaks <br>: {src} in the original, {got} in the translation", src, got),
            Refusal::Lines { src, got } => trf!("lines: {src} in the original, {got} in the translation", src, got),
            Refusal::Mark { mark, src, got } => trf!("markup mark `{mark}`: {src} in the original, {got} in the translation", mark, src, got),
            Refusal::Character(c) => trf!("character «{c}» (U+{code}) the game cannot show", c, code = format!("{:04X}", *c as u32)),
        }
    }
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// The start of `s` for a log line: its words on one line, up to `n` characters, `…` when cut.
pub fn excerpt(s: &str, n: usize) -> String {
    let one_line = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= n {
        one_line
    } else {
        one_line.chars().take(n).collect::<String>() + "…"
    }
}

/// The model's `answer` for `src`, checked and turned back into a map string: placeholders
/// replaced, `<br>` back to `#\`, line breaks in the source's style, punctuation the game's
/// fonts may lack made plain ASCII. Errors say what the answer broke.
pub fn restore(src: &str, prepared: &Prepared, answer: &str, language: &str, any_script: bool) -> Result<String, Refusal> {
    let answer = plain_punctuation(answer.trim_matches(|c: char| c == ' ' || c == '\t'));
    if answer.trim().is_empty() {
        return Err(Refusal::Empty);
    }
    let mut out = answer.clone();
    for (i, tok) in prepared.tokens.iter().enumerate() {
        let ph = format!("{{{i}}}");
        match out.matches(&ph).count() {
            1 => out = out.replace(&ph, tok),
            got => return Err(Refusal::Placeholder { ph, got }),
        }
    }
    if !is_cyrillic_language(language) && has_cyrillic(&strip_tokens(&out, &prepared.tokens)) {
        return Err(Refusal::Cyrillic);
    }
    let count = |s: &str, p: &str| s.matches(p).count();
    let (src_breaks, got_breaks) = (count(&prepared.text, "<br>"), count(&answer, "<br>"));
    if got_breaks != src_breaks {
        return Err(Refusal::Breaks { src: src_breaks, got: got_breaks });
    }
    let lines = |s: &str| s.trim_end_matches('\n').matches('\n').count() + 1;
    let (src_lines, got_lines) = (lines(&prepared.text), lines(&answer));
    if got_lines != src_lines {
        return Err(Refusal::Lines { src: src_lines, got: got_lines });
    }
    for mark in MARKS {
        let (src, got) = (prepared.text.matches(mark).count(), answer.matches(mark).count());
        if got != src {
            return Err(Refusal::Mark { mark, src, got });
        }
    }
    // Maps and data files keep their text in Windows-1251 (`crate::dt::text`), as the original's
    // fonts do: a character outside it cannot be stored.
    if let Some(bad) = out.chars().find(|&c| !any_script && !in_cp1251(c) && !prepared.tokens.iter().any(|t| t.contains(c))) {
        return Err(Refusal::Character(bad));
    }
    // Trailing line breaks of the source come back as they were.
    let out = out.trim_end_matches('\n').to_string() + &"\n".repeat(prepared.text.len() - prepared.text.trim_end_matches('\n').len());
    let out = out.replace("<br>", "#\\");
    Ok(if src.contains("\r\n") { out.replace('\n', "\r\n") } else { out })
}

pub use crate::dt::text::in_cp1251;

/// A language written in Cyrillic (an answer in it is not "untranslated").
pub fn is_cyrillic_language(language: &str) -> bool {
    let l = language.trim().to_lowercase();
    ["rus", "рус", "ukr", "укр", "belar", "белор", "bulg", "болг", "serb", "серб", "maced", "макед"].iter().any(|k| l.contains(k))
}

fn strip_tokens(s: &str, tokens: &[String]) -> String {
    tokens.iter().fold(s.to_string(), |acc, t| acc.replace(t.as_str(), ""))
}

/// Typographic punctuation as plain ASCII (the original's fonts are cp1251 bitmaps; plain
/// ASCII is safe in all of them).
pub fn plain_punctuation(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{00AB}' | '\u{00BB}' => out.push('"'),
            '\u{2018}' | '\u{2019}' | '\u{201A}' => out.push('\''),
            '\u{2013}' | '\u{2014}' | '\u{2212}' => out.push('-'),
            '\u{2026}' => out.push_str("..."),
            '\u{00A0}' | '\u{2009}' | '\u{202F}' => out.push(' '),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hides_breaks_and_hash_words() {
        let p = prepare("Привет, #HERONAME!\r\n*Конец#\\^Да");
        assert_eq!(p.text, "Привет, {0}!\n*Конец<br>^Да");
        assert_eq!(p.tokens, ["#HERONAME"]);
        let p = prepare("Страж#рыцарь и # один");
        assert_eq!(p.text, "Страж{0} и # один");
        assert_eq!(p.tokens, ["#рыцарь"]);
    }

    #[test]
    fn restores_a_good_answer_in_the_source_style() {
        let src = "Привет, #HERONAME!\r\n*Конец#\\^Да";
        let p = prepare(src);
        let got = restore(src, &p, "Hello, {0}!\n*The end<br>^Yes", "English", false).unwrap();
        assert_eq!(got, "Hello, #HERONAME!\r\n*The end#\\^Yes");
        // A source without CR keeps bare line breaks; a trailing break survives.
        let src = "Строка\nдругая\n";
        assert_eq!(restore(src, &prepare(src), "Line\nanother", "English", false).unwrap(), "Line\nanother\n");
    }

    #[test]
    fn rejects_broken_answers() {
        let src = "Привет, #HERONAME!\r\n*Конец";
        let p = prepare(src);
        assert!(restore(src, &p, "Hello!\n*The end", "English", false).is_err(), "lost placeholder");
        assert!(restore(src, &p, "Hello, {0}! *The end", "English", false).is_err(), "lost line");
        assert!(restore(src, &p, "Hello, {0}!\nThe end", "English", false).is_err(), "lost mark");
        assert!(restore(src, &p, "Привет, {0}!\n*The end", "English", false).is_err(), "Cyrillic left");
        assert!(restore(src, &p, "  ", "English", false).is_err(), "empty");
        // A Cyrillic `#` word is kept, not counted as untranslated.
        let src = "Страж#рыцарь";
        assert_eq!(restore(src, &prepare(src), "Guard{0}", "English", false).unwrap(), "Guard#рыцарь");
    }

    #[test]
    fn refusals_say_what_was_expected_and_what_came() {
        let src = "Привет, #HERONAME!\r\n*Конец#\\^Да";
        let p = prepare(src);
        let refused = |answer: &str| restore(src, &p, answer, "English", false).unwrap_err();
        assert_eq!(refused("Hello!\n*The end<br>^Yes"), Refusal::Placeholder { ph: "{0}".into(), got: 0 });
        assert_eq!(refused("Hello, {0} {0}!\n*The end<br>^Yes"), Refusal::Placeholder { ph: "{0}".into(), got: 2 });
        assert_eq!(refused("Hello, {0}!\n*The end ^Yes"), Refusal::Breaks { src: 1, got: 0 });
        assert_eq!(refused("Hello, {0}! *The end<br>^Yes"), Refusal::Lines { src: 2, got: 1 });
        assert_eq!(refused("Hello, {0}!\nThe end<br>^Yes"), Refusal::Mark { mark: '*', src: 1, got: 0 });
        assert_eq!(refused("Hello, {0}!\n*The end<br>^Yes ^"), Refusal::Mark { mark: '^', src: 1, got: 2 });
        assert_eq!(refused("Привет, {0}!\n*The end<br>^Yes"), Refusal::Cyrillic);
        assert_eq!(refused(" \t"), Refusal::Empty);
        assert_eq!(refused("Hello, {0}!\n*The end<br>^Ja\u{00E4}"), Refusal::Character('\u{00E4}'));
        // The messages (English while the library runs in English; the Russian is the catalog's).
        assert_eq!(Refusal::Mark { mark: '*', src: 1, got: 0 }.message(), "markup mark `*`: 1 in the original, 0 in the translation");
        assert_eq!(Refusal::Lines { src: 2, got: 1 }.to_string(), "lines: 2 in the original, 1 in the translation");
        assert_eq!(Refusal::Placeholder { ph: "{0}".into(), got: 2 }.message(), "placeholder {0}: 1 in the original, 2 in the translation");
        assert_eq!(Refusal::Character('\u{00E4}').message(), "character «\u{00E4}» (U+00E4) the game cannot show");
        let ru = crate::i18n::tr_in(crate::i18n::Lang::Ru, "markup mark `{mark}`: {src} in the original, {got} in the translation");
        assert_eq!(crate::i18n::fill(ru, &[("mark", &'*'), ("src", &1), ("got", &0)]), "знак разметки `*`: 1 в оригинале, 0 в переводе");
    }

    #[test]
    fn excerpts_are_one_short_line() {
        assert_eq!(excerpt("Old Fili\nbegs  by the road", 100), "Old Fili begs by the road");
        assert_eq!(excerpt("Old Fili begs by the road", 8), "Old Fili…");
        assert_eq!(excerpt("Старый Фили", 6), "Старый…");
    }

    #[test]
    fn typographic_punctuation_becomes_ascii() {
        let src = "Он сказал";
        assert_eq!(restore(src, &prepare(src), "He said: \u{201C}Go\u{201D} \u{2014} now\u{2026}", "English", false).unwrap(), "He said: \"Go\" - now...");
        assert!(restore(src, &prepare(src), "He said \u{00E9}", "French", false).is_err(), "no accents in Windows-1251");
        assert!(restore(src, &prepare(src), "Он сказал: да", "Ukrainian", false).is_ok(), "Cyrillic for a Cyrillic language");
        assert!(restore(src, &prepare(src), "Он сказал", "German", false).is_err(), "Cyrillic left for another");
        assert!(restore(src, &prepare(src), "\u{0935}\u{0939}", "Hindi", false).is_err(), "Devanagari cannot be stored");
        assert_eq!(restore(src, &prepare(src), "\u{0935}\u{0939}", "Hindi", true).unwrap(), "\u{0935}\u{0939}", "any script: Razdor only");
    }
}
