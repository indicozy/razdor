//! Text matching for Razdor's search boxes (the inventory filter, the cheat console's search and its item and unit names): case-insensitive in any script, with Ё read as
//! Е, so "ежевика" finds "Ёжевика" and "МЕЧ" finds "Меч".

/// One character as the matcher compares it: lower case, `ё` as `е`.
pub fn fold_char(c: char) -> char {
    let lower = c.to_lowercase().next().unwrap_or(c);
    if lower == 'ё' {
        'е'
    } else {
        lower
    }
}

/// The text as the matcher compares it ([`fold_char`] on every character).
pub fn fold(s: &str) -> String {
    s.chars().map(fold_char).collect()
}

/// Where `needle` first stands in `hay`, ignoring case (and Ё / Е): the byte range in `hay`,
/// for drawing the matched part. An empty needle matches nothing.
pub fn find(hay: &str, needle: &str) -> Option<std::ops::Range<usize>> {
    let needle: Vec<char> = needle.chars().map(fold_char).collect();
    if needle.is_empty() {
        return None;
    }
    let chars: Vec<(usize, char)> = hay.char_indices().map(|(i, c)| (i, fold_char(c))).collect();
    if chars.len() < needle.len() {
        return None;
    }
    (0..=chars.len() - needle.len()).find(|&s| chars[s..s + needle.len()].iter().map(|&(_, c)| c).eq(needle.iter().copied())).map(|s| {
        let end = chars.get(s + needle.len()).map_or(hay.len(), |&(i, _)| i);
        chars[s].0..end
    })
}

/// The words of a query: split at spaces, empty ones dropped.
pub fn words(query: &str) -> Vec<&str> {
    query.split_whitespace().collect()
}

/// A record that matches a query ([`matches`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// The part of the name to light up: the first query word found in the name.
    pub name_range: Option<std::ops::Range<usize>>,
}

/// How a record matches a query: every word of the query stands in its name or in one of its
/// other fields (type, bonus words, ids). `None` when some word is missing; an empty query
/// matches everything with no highlight.
pub fn matches(query: &str, name: &str, other: &[&str]) -> Option<Match> {
    let mut name_range = None;
    for w in words(query) {
        match find(name, w) {
            Some(r) => {
                name_range.get_or_insert(r);
            }
            None if other.iter().any(|f| find(f, w).is_some()) => {}
            None => return None,
        }
    }
    Some(Match { name_range })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_in_any_case_and_script() {
        assert_eq!(find("Long Sword", "sword"), Some(5..10));
        assert_eq!(find("Long Sword", "SWO"), Some(5..8));
        assert_eq!(find("Меч героя", "МЕЧ"), Some(0..6), "Cyrillic, bytes of the original");
        assert_eq!(find("Ёжевика", "ежев"), Some(0..8), "Ё is Е");
        assert_eq!(find("Ежевика", "ЁЖ"), Some(0..4));
        assert_eq!(find("Меч", "мечи"), None);
        assert_eq!(find("Меч", ""), None);
        assert_eq!(fold("ЁЛКА"), "елка");
    }

    #[test]
    fn every_word_must_stand_somewhere() {
        assert_eq!(matches("", "Sword", &[]), Some(Match { name_range: None }), "an empty query keeps everything");
        assert_eq!(matches("sw", "Sword", &[]).unwrap().name_range, Some(0..2));
        assert!(matches("sword weapon", "Sword", &["Weapon", "+3 attack"]).is_some(), "a word in the type");
        assert!(matches("attack", "Sword", &["Weapon", "+3 attack"]).unwrap().name_range.is_none(), "found in a bonus only");
        assert!(matches("sword staff", "Sword", &["Weapon"]).is_none(), "staff is nowhere");
        assert_eq!(matches("weapon sw", "Sword", &["Weapon"]).unwrap().name_range, Some(0..2), "the name's word is lit");
    }
}
