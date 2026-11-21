//! The language of a map: Russian, English or unknown, read from the map's own player-visible text
//! (its title, campaign name, description, and its events' titles, questions and messages). Pure:
//! no UI, no files. The list filters by it (`ui::new_game`, «Все языки» / «Русский» / «English»).
//!
//! Order of the sources (`language_of`): the hub's catalog says the language of a file (`lang`,
//! the hub's own), else the text decides, else the file name (a tiebreak for a map whose text is
//! too short to tell, e.g. `..._EN.DTm`, `ENG_...`, `Eng_...`).

use super::dtm::Scenario;

/// The language of a map.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MapLang {
    Ru,
    En,
    /// Too little text to tell, or no clear majority: shown under every choice.
    #[default]
    Unknown,
}

impl MapLang {
    /// The code the hub's catalog uses (`ru`, `en`); None for unknown.
    pub fn code(self) -> Option<&'static str> {
        match self {
            MapLang::Ru => Some("ru"),
            MapLang::En => Some("en"),
            MapLang::Unknown => None,
        }
    }

    /// A catalog's code (`ru`, `en`, any case); anything else is unknown.
    pub fn of_code(code: &str) -> MapLang {
        match code.trim().to_ascii_lowercase().as_str() {
            "ru" => MapLang::Ru,
            "en" => MapLang::En,
            _ => MapLang::Unknown,
        }
    }
}

/// The fewest letters a text must have to tell its language.
pub const MIN_LETTERS: usize = 20;

/// The share (in tenths) one alphabet must reach to decide the language: 70%.
const MAJORITY_TENTHS: usize = 7;

/// The letters of `texts`: (Cyrillic, Latin). Anything else (digits, marks, punctuation) is not
/// counted, so a number or a markup mark does not decide.
pub fn count_letters<'a>(texts: impl IntoIterator<Item = &'a str>) -> (usize, usize) {
    let (mut cyr, mut lat) = (0, 0);
    for c in texts.into_iter().flat_map(str::chars) {
        if !c.is_alphabetic() {
            continue;
        }
        if ('\u{0400}'..='\u{04FF}').contains(&c) {
            cyr += 1;
        } else if c.is_ascii_alphabetic() || ('\u{00C0}'..='\u{024F}').contains(&c) {
            lat += 1;
        }
    }
    (cyr, lat)
}

/// The language a count of letters gives: unknown below [`MIN_LETTERS`], and unknown when neither
/// alphabet reaches [`MAJORITY_TENTHS`] tenths of the letters.
pub fn decide(cyr: usize, lat: usize) -> MapLang {
    let total = cyr + lat;
    if total < MIN_LETTERS {
        MapLang::Unknown
    } else if cyr * 10 >= total * MAJORITY_TENTHS {
        MapLang::Ru
    } else if lat * 10 >= total * MAJORITY_TENTHS {
        MapLang::En
    } else {
        MapLang::Unknown
    }
}

/// The language of the given texts.
pub fn detect<'a>(texts: impl IntoIterator<Item = &'a str>) -> MapLang {
    let (cyr, lat) = count_letters(texts);
    decide(cyr, lat)
}

/// The language of a scenario's own text: its title, campaign name and description, and each
/// event's title (the flag script and editor note left out, as the game shows it), question and
/// message.
pub fn of_scenario(s: &Scenario) -> MapLang {
    let mut texts: Vec<&str> = vec![s.title.as_str(), s.campaign_name.as_str(), s.description.as_str()];
    for e in &s.events {
        texts.push(e.display_title());
        texts.push(e.question.as_str());
        texts.push(e.message.as_str());
    }
    detect(texts)
}

/// The language a file name hints at: `en`/`eng` (or `ru`/`rus`) as its first or last segment
/// of `_`-separated parts, any case (`..._EN.DTm`, `ENG_...`, `Eng_...`, `..._ENG`). Unknown
/// otherwise.
pub fn of_file_name(name: &str) -> MapLang {
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    let parts: Vec<String> = stem.split('_').map(str::to_ascii_lowercase).collect();
    let (Some(first), Some(last)) = (parts.first(), parts.last()) else { return MapLang::Unknown };
    let is = |p: &str, set: &[&str]| set.contains(&p);
    let en = ["en", "eng"];
    let ru = ["ru", "rus"];
    if is(first, &en) || is(last, &en) {
        MapLang::En
    } else if is(first, &ru) || is(last, &ru) {
        MapLang::Ru
    } else {
        MapLang::Unknown
    }
}

/// The language of a map: the hub's `catalog_code` when it names one, else the text's, else the
/// file name's (the tiebreak for a text too short to tell).
pub fn language_of(catalog_code: Option<&str>, text: MapLang, file_name: &str) -> MapLang {
    if let Some(code) = catalog_code.map(MapLang::of_code).filter(|l| *l != MapLang::Unknown) {
        return code;
    }
    match text {
        MapLang::Unknown => of_file_name(file_name),
        known => known,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RU_TEXT: &str = "Огромное подземелье на юге континента притягивает исследователей со всего света.";
    const EN_TEXT: &str = "A huge dungeon in the south of the continent draws explorers from all over the world.";

    #[test]
    fn a_russian_text_is_ru_and_an_english_one_is_en() {
        assert_eq!(detect([RU_TEXT]), MapLang::Ru);
        assert_eq!(detect([EN_TEXT]), MapLang::En);
    }

    #[test]
    fn too_few_letters_are_unknown_whatever_they_are() {
        assert_eq!(detect(["Ок"]), MapLang::Unknown, "two Cyrillic letters");
        assert_eq!(detect(["Hi"]), MapLang::Unknown, "two Latin letters");
        assert_eq!(detect(["12345 ***"]), MapLang::Unknown, "no letters");
        assert_eq!(decide(MIN_LETTERS - 1, 0), MapLang::Unknown);
        assert_eq!(decide(MIN_LETTERS, 0), MapLang::Ru);
    }

    #[test]
    fn a_mixed_text_needs_a_clear_majority() {
        assert_eq!(decide(60, 40), MapLang::Unknown, "60% is not clear");
        assert_eq!(decide(70, 30), MapLang::Ru, "70% is");
        assert_eq!(decide(30, 70), MapLang::En);
        assert_eq!(detect(["Dungeon Вуальшпиль, Dungeon Вуальшпиль, Dungeon Вуальшпиль"]), MapLang::Unknown);
    }

    #[test]
    fn digits_and_markup_marks_do_not_count() {
        assert_eq!(detect(["*|@ 1234567890 ***  ||| ++ 9999 <<>> %%"]), MapLang::Unknown);
        assert_eq!(detect([RU_TEXT, "1 2 3 4 5 6 7 8 9 0 * * * * * *"]), MapLang::Ru);
    }

    #[test]
    fn the_letters_of_several_texts_add_up() {
        assert_eq!(detect(["Вуальшпиль", RU_TEXT]), MapLang::Ru);
        assert_eq!(detect(["Dungeon", EN_TEXT]), MapLang::En);
    }

    #[test]
    fn a_file_name_hint_reads_the_language_tags() {
        assert_eq!(of_file_name("hristos0_-_VeilspielDungeon_1_5_EN.DTm"), MapLang::En);
        assert_eq!(of_file_name("ENG_-_hristos0_-_PeasantSimulator.dtm"), MapLang::En);
        assert_eq!(of_file_name("Eng_hristos0_-_CaravanDefence.DTm"), MapLang::En);
        assert_eq!(of_file_name("Vlad-Unholy_Cult-1_ENG.DTm"), MapLang::En);
        assert_eq!(of_file_name("hristos0_-_VeilspielDungeon_1_5_1.DTm"), MapLang::Unknown, "a version number is no tag");
        assert_eq!(of_file_name("Caravan_RU.DTm"), MapLang::Ru);
        assert_eq!(of_file_name("english_map.DTm"), MapLang::Unknown, "a word is not a segment");
    }

    #[test]
    fn the_catalog_says_first_then_the_text_then_the_name() {
        assert_eq!(language_of(Some("ru"), MapLang::En, "x_EN.DTm"), MapLang::Ru, "the hub's own tag wins");
        assert_eq!(language_of(Some("xx"), MapLang::En, "x_RU.DTm"), MapLang::En, "a code it does not know is no tag");
        assert_eq!(language_of(None, MapLang::Ru, "x_EN.DTm"), MapLang::Ru, "a clear text beats the name");
        assert_eq!(language_of(None, MapLang::Unknown, "x_EN.DTm"), MapLang::En, "the name breaks a short text");
        assert_eq!(language_of(None, MapLang::Unknown, "hristos0_-_VeilspielDungeon_1_5_1.DTm"), MapLang::Unknown);
    }

    /// Two hub files of one map (the catalog's names, no hub tag): the text tells Russian from
    /// English, and the name's `_EN` tells the same. Synthetic texts, not the map's own.
    #[test]
    fn a_pair_of_versions_of_one_map_comes_out_ru_and_en() {
        let ru = "Подземелье огромно. Каждый, кто вернулся оттуда живым, рассказывал иное.";
        let en = "The dungeon is huge. Everyone who came back alive told a different story.";
        let (ru_file, en_file) = ("hristos0_-_VeilspielDungeon_1_5_1.DTm", "hristos0_-_VeilspielDungeon_1_5_EN.DTm");
        assert_eq!(language_of(None, detect([ru]), ru_file), MapLang::Ru);
        assert_eq!(language_of(None, detect([en]), en_file), MapLang::En);
        assert_eq!(language_of(None, MapLang::Unknown, en_file), MapLang::En, "a text too short to tell: the name");
    }
}
