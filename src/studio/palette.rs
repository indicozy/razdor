//! Fuzzy matching for the studio's command palette and Ctrl+P (UX6, spec §10): a query matches a name
//! when its characters come in order in the name, ignoring case and `ё` (as `е`). The ranking prefers a
//! name that starts with the query, then one that holds it as a run, then a match on word starts and
//! short names. Russian and English names rank the same way; the caller passes every name of an item
//! (its id, its English and its Russian name) and the best one counts.

/// The name folded for matching: lowercase, `ё` as `е`.
fn fold(text: &str) -> Vec<char> {
    text.chars().flat_map(char::to_lowercase).map(|c| if c == 'ё' { 'е' } else { c }).collect()
}

/// Whether `at` begins a word of `name` (the start, or after a character that is not a letter or digit).
fn word_start(name: &[char], at: usize) -> bool {
    at == 0 || !name[at - 1].is_alphanumeric()
}

/// The score of `query` against one `name` (higher is better), or `None` when the query does not match.
/// An empty query matches everything with score 0.
pub fn score(query: &str, name: &str) -> Option<i32> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Some(0);
    }
    let n = fold(name);
    if q.len() > n.len() {
        return None;
    }
    // The extra length of the name: a shorter name with the same match ranks first.
    let extra = (n.len() - q.len()).min(200) as i32;
    if let Some(at) = (0..=n.len() - q.len()).find(|&i| n[i..i + q.len()] == q[..]) {
        let start = if at == 0 {
            500
        } else if word_start(&n, at) {
            300
        } else {
            0
        };
        return Some(2000 + start - 2 * at as i32 - extra);
    }
    // Otherwise the characters must come in order: each one is taken at its first place after the last.
    let mut pos = Vec::with_capacity(q.len());
    let mut from = 0;
    for &c in &q {
        let at = (from..n.len()).find(|&i| n[i] == c)?;
        pos.push(at);
        from = at + 1;
    }
    let mut total = 1000 - 3 * pos[0] as i32 - extra;
    for (k, &p) in pos.iter().enumerate() {
        if word_start(&n, p) {
            total += 40;
        }
        if k > 0 {
            let gap = p - pos[k - 1] - 1;
            if gap == 0 {
                total += 15;
            } else {
                total -= 2 * gap as i32;
            }
        }
    }
    Some(total)
}

/// The best score of `query` over an item's names (`None` when no name matches).
pub fn best<S: AsRef<str>>(query: &str, names: &[S]) -> Option<i32> {
    names.iter().filter_map(|n| score(query, n.as_ref())).max()
}

/// The items that match `query`, best first: each as its index and its best score. Ties keep the order
/// the items came in. An empty query keeps every item in its order.
pub fn rank<S: AsRef<str>>(query: &str, items: &[Vec<S>]) -> Vec<(usize, i32)> {
    let mut hits: Vec<(usize, i32)> = items.iter().enumerate().filter_map(|(i, names)| best(query, names).map(|s| (i, s))).collect();
    hits.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_query_matches_everything_in_order() {
        assert_eq!(score("", "Save"), Some(0));
        assert_eq!(score("   ", "Save"), Some(0));
        let items = vec![vec!["b"], vec!["a"]];
        assert_eq!(rank("", &items), vec![(0, 0), (1, 0)], "an empty query keeps the order");
    }

    #[test]
    fn the_characters_must_come_in_order() {
        assert!(score("toll", "Toll gate").is_some());
        assert!(score("toll", "Tax").is_none());
        assert!(score("lot", "Toll").is_none(), "the order is kept");
        assert!(score("tollgates", "Toll").is_none(), "a longer query never matches");
    }

    #[test]
    fn a_name_that_starts_with_the_query_ranks_above_one_that_holds_it() {
        let start = score("sav", "Save").unwrap();
        let inside = score("sav", "Autosave file").unwrap();
        assert!(start > inside, "{start} > {inside}");
    }

    #[test]
    fn a_word_start_ranks_above_a_mid_word_match() {
        let word = score("gate", "Toll gate").unwrap();
        let mid = score("gate", "Stargate").unwrap();
        assert!(word > mid, "{word} > {mid}");
    }

    #[test]
    fn a_subsequence_matches_across_words_and_ranks_below_a_run() {
        let run = score("sa", "Save all").unwrap();
        let spread = score("sa", "Select all").unwrap();
        assert!(run > spread, "{run} > {spread}");
        assert!(score("sa", "Select all").is_some());
    }

    #[test]
    fn a_shorter_name_with_the_same_match_ranks_first() {
        assert!(score("save", "Save").unwrap() > score("save", "Save all").unwrap());
    }

    #[test]
    fn russian_names_match_in_their_own_letters() {
        // «лес» starts a word and is a run of the name «Лесная опушка», and holds inside «Перелесок».
        let prefix = score("лес", "Лесная опушка").unwrap();
        let inside = score("лес", "Перелесок").unwrap();
        assert!(prefix > inside, "{prefix} > {inside}");
        assert!(score("лес", "Карта").is_none());
        // Case and `ё` do not count: «ёлка» finds «Елка».
        assert!(score("ёлка", "Елка у дороги").is_some());
        assert!(score("ЛЕС", "лесной").is_some(), "case is ignored");
    }

    #[test]
    fn the_best_of_an_items_names_counts_and_the_ids_are_names_too() {
        // The id of a trigger matches when the Russian name does not.
        let names = vec!["gate_toll".to_string(), "Пошлина".to_string()];
        assert!(best("toll", &names).is_some());
        assert!(best("пошл", &names).is_some());
        assert!(best("zzz", &names).is_none());
    }

    #[test]
    fn rank_orders_the_best_first_and_drops_the_misses() {
        let items: Vec<Vec<&str>> = vec![vec!["Перелесок"], vec!["Карта"], vec!["Лесной массив"]];
        let ranked = rank("лес", &items);
        assert_eq!(ranked.iter().map(|(i, _)| *i).collect::<Vec<_>>(), vec![2, 0], "the prefix first, the miss dropped");
    }
}
