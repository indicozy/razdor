//! A translation job: the maps of one scenario or campaign, the distinct Russian strings in
//! them, and the English found so far. Saved after every answer, so a long campaign can be
//! stopped and taken up again.
//!
//! Strings are translated in two passes: names first (buildings, owners, armies, leaders,
//! named characters, titles), short and repeated everywhere, then the prose (descriptions,
//! questions, messages), each batch with the names it mentions as a glossary, so a name is
//! spelled the same in every text.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::text::{self, has_cyrillic, prepare};
use crate::dt::container;
use crate::dt::dtm::{FlagScript, Scenario};
use crate::dt::DtError;
use crate::trf;

/// What a map string is, for where it may be cut and which pass translates it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    /// A name or title: the first pass.
    Name,
    /// An army's name: the part after the first `#` is the editor's numbering (`Призрак#2`)
    /// or an archetype suffix, never shown (0x4973a0), kept as it is.
    ArmyName,
    /// An event's title: the part from the first `%` is its flag script, kept as it is.
    EventTitle,
    /// Prose: the second pass.
    Text,
    /// The next map's file name: relinked to the next chapter's English copy.
    NextMap,
}

/// Every string of `s` with what it is, in file order.
fn fields(s: &mut Scenario) -> Vec<(&mut String, Field)> {
    let mut v: Vec<(&mut String, Field)> = vec![
        (&mut s.title, Field::Name),
        (&mut s.description, Field::Text),
        (&mut s.campaign_name, Field::Name),
        (&mut s.next_map, Field::NextMap),
    ];
    for b in &mut s.buildings {
        v.push((&mut b.name, Field::Name));
        v.push((&mut b.owner_name, Field::Name));
        v.push((&mut b.description, Field::Text));
    }
    for a in &mut s.armies {
        v.push((&mut a.name, Field::ArmyName));
        v.push((&mut a.leader_name, Field::Name));
        v.push((&mut a.description, Field::Text));
    }
    for e in &mut s.events {
        v.push((&mut e.title, Field::EventTitle));
        v.push((&mut e.question, Field::Text));
        v.push((&mut e.message, Field::Text));
    }
    for n in &mut s.named_characters {
        v.push((&mut n.name, Field::Name));
    }
    v
}

/// A string cut into what is kept before, the translatable middle, and what is kept after:
/// the blanks around the middle, and the tail of army names and event titles.
fn split(s: &str, f: Field) -> (&str, &str, &str) {
    let cut = match f {
        Field::ArmyName => s.find('#'),
        Field::EventTitle => s.find('%'),
        _ => None,
    }
    .unwrap_or(s.len());
    let body = &s[..cut];
    let lead = body.len() - body.trim_start().len();
    let core = body.trim();
    (&s[..lead], core, &s[lead + core.len()..])
}

fn rebuild(s: &str, f: Field, en: &str) -> String {
    let (lead, _, rest) = split(s, f);
    format!("{lead}{en}{rest}")
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// The Russian text (the translatable middle of one or more map strings).
    pub src: String,
    /// A name (first pass) rather than prose.
    pub name: bool,
    /// The English, once an answer passed the checks.
    pub en: Option<String>,
    /// Answers refused so far.
    pub tries: u8,
    /// Why the last answer was refused.
    #[serde(default)]
    pub last_error: String,
    /// The last refused answer itself, for the log (not saved).
    #[serde(skip)]
    pub last_answer: Option<String>,
}

/// Why an answer has no text for `key`: the keys it does have, or that it has none (its raw
/// text is in the request log).
pub fn missing(key: &str, answers: &HashMap<String, String>) -> String {
    let mut keys: Vec<&str> = answers.keys().map(String::as_str).collect();
    if keys.is_empty() {
        return trf!("no text under key \"{key}\": the answer holds no texts", key);
    }
    keys.sort_by_key(|k| (k.parse::<u64>().unwrap_or(u64::MAX), *k));
    const SHOWN: usize = 12;
    let mut list = keys.iter().take(SHOWN).copied().collect::<Vec<_>>().join(", ");
    if keys.len() > SHOWN {
        list.push_str(", …");
    }
    trf!("no text under key \"{key}\"; the answer has keys {keys}", key, keys = list)
}

/// A map of the job: its file in the maps folder and the English copy's.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobMap {
    pub file: String,
    pub out: String,
}

fn english() -> String {
    "English".to_string()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// The language translated into (the player's setting; English before it existed).
    #[serde(default = "english")]
    pub language: String,
    /// The language's own letters, stored as UTF-8 (Razdor only; [`super::llm::Settings::any_script`]).
    #[serde(default)]
    pub any_script: bool,
    pub maps: Vec<JobMap>,
    pub entries: Vec<Entry>,
    /// The model, style and context the translations were made with (none for a job from
    /// before this was kept): when they change, the window says the texts are the old ones.
    #[serde(default)]
    pub made_with: Option<MadeWith>,
    /// Names translated elsewhere (the game data's units, items and spells), offered in the
    /// glossary like the job's own; not saved with the job.
    #[serde(skip)]
    pub outside_glossary: Vec<(String, String)>,
}

/// What a job's translations were made with.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MadeWith {
    pub model: String,
    pub style: String,
    pub notes: String,
}

impl MadeWith {
    pub fn of(s: &super::llm::Settings) -> MadeWith {
        MadeWith { model: s.model.trim().into(), style: s.style.trim().into(), notes: s.notes.trim().into() }
    }
}

impl Job {
    /// The translations were made with another model, style or context than `s`.
    pub fn made_otherwise(&self, s: &super::llm::Settings) -> bool {
        self.done() > 0 && self.made_with.as_ref().is_some_and(|m| *m != MadeWith::of(s))
    }
}

/// An entry is given up on (left for "retry") after this many refused answers.
pub const MAX_TRIES: u8 = 3;

/// The suffix of a translation's files: the language's ([`lang_suffix`]), with `-native` for
/// its own letters (Razdor only) unless it is English.
pub fn copy_suffix(language: &str, any_script: bool) -> String {
    let l = lang_suffix(language);
    if any_script && l != "en" { format!("{l}-native") } else { l }
}

/// A language's suffix for file names: `en` for English, else the name's Latin letters in
/// lower case (`Hindi` → `hindi`), `tr` when it has none.
pub fn lang_suffix(language: &str) -> String {
    let l = language.trim().to_lowercase();
    if l.is_empty() || l == "english" || l == "en" || l == "английский" {
        return "en".into();
    }
    let s: String = l.chars().filter(|c| c.is_ascii_alphanumeric()).take(20).collect();
    if s.is_empty() { "tr".into() } else { s }
}

/// The translated copy's file name: `Name.DTm` → `Name_en.DTm` (English), `Name_german.DTm`.
pub fn translated_file(file: &str, language: &str) -> String {
    copy_file(file, &lang_suffix(language))
}

/// `Name.DTm` → `Name_<suffix>.DTm`.
pub fn copy_file(file: &str, suffix: &str) -> String {
    let p = Path::new(file);
    let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let ext = p.extension().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "DTm".into());
    format!("{stem}_{suffix}.{ext}")
}

/// The English copy's file name: `Name.DTm` → `Name_en.DTm`.
pub fn english_file(file: &str) -> String {
    translated_file(file, "English")
}

/// A translated copy made by this tool: its name ends in `_` and a Latin suffix
/// ([`lang_suffix`]) after a name that is not all Latin (the shipped maps' names are Russian).
pub fn is_translated_file(file: &str) -> bool {
    let Some(stem) = Path::new(file).file_stem().map(|s| s.to_string_lossy().into_owned()) else { return false };
    let Some((name, suffix)) = stem.rsplit_once('_') else { return false };
    (2..=27).contains(&suffix.len()) && suffix.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-') && !name.is_ascii()
}

/// An English copy made by this tool.
pub fn is_english_file(file: &str) -> bool {
    Path::new(file).file_stem().is_some_and(|s| s.to_string_lossy().ends_with("_en"))
}

fn same_file(a: &str, b: &str) -> bool {
    let norm = |s: &str| {
        let s = s.trim().to_lowercase();
        if s.ends_with(".dtm") { s } else { s + ".dtm" }
    };
    norm(a) == norm(b)
}

/// The chapters of the campaign that holds `maps[at]`, in play order: back along the
/// next-map links to the first chapter, then forward. A map in no chain is alone.
pub fn campaign(maps: &[(String, Scenario)], at: usize) -> Vec<usize> {
    let next = |i: usize| -> Option<usize> {
        let n = maps[i].1.next_map.trim();
        if n.is_empty() {
            return None;
        }
        maps.iter().position(|(f, _)| same_file(f, n))
    };
    let mut head = at;
    let mut seen = vec![at];
    while let Some(prev) = (0..maps.len()).find(|&j| !seen.contains(&j) && next(j) == Some(head)) {
        seen.push(prev);
        head = prev;
    }
    let mut chain = vec![head];
    let mut cur = head;
    while let Some(n) = next(cur).filter(|n| !chain.contains(n)) {
        chain.push(n);
        cur = n;
    }
    chain
}

impl Job {
    /// The job for `maps` (file name, scenario), in play order.
    pub fn new(maps: &[(String, Scenario)], language: &str, any_script: bool) -> Job {
        let mut texts = Vec::new();
        for (_, s) in maps {
            let mut s = s.clone();
            for (value, f) in fields(&mut s) {
                if f != Field::NextMap {
                    texts.push((split(value, f).1.to_string(), f != Field::Text));
                }
            }
        }
        let mut job = Job::from_texts(texts.iter().map(|(t, n)| (t.as_str(), *n)));
        let suffix = copy_suffix(language, any_script);
        job.maps = maps.iter().map(|(f, _)| JobMap { file: f.clone(), out: copy_file(f, &suffix) }).collect();
        job.language = language.trim().to_string();
        job.any_script = any_script;
        job
    }

    /// The job for `texts` (text, whether it is a name): each distinct text with Cyrillic in
    /// it once, a name only if it is a name everywhere.
    pub fn from_texts<'a>(texts: impl IntoIterator<Item = (&'a str, bool)>) -> Job {
        let mut entries: Vec<Entry> = Vec::new();
        let mut index: HashMap<&str, usize> = HashMap::new();
        for (core, name) in texts {
            if !has_cyrillic(core) {
                continue;
            }
            match index.get(core) {
                Some(&i) => entries[i].name &= name,
                None => {
                    index.insert(core, entries.len());
                    entries.push(Entry { src: core.to_string(), name, ..Entry::default() });
                }
            }
        }
        Job { entries, language: english(), ..Job::default() }
    }

    /// The English of `src`, once translated.
    pub fn english_of(&self, src: &str) -> Option<&str> {
        self.entries.iter().find(|e| e.src == src)?.en.as_deref()
    }

    /// Takes over the English of `old` for the strings both jobs share.
    pub fn take_over(&mut self, old: &Job) {
        self.made_with = old.made_with.clone();
        let known: HashMap<&str, &Entry> = old.entries.iter().map(|e| (e.src.as_str(), e)).collect();
        for e in &mut self.entries {
            if let Some(o) = known.get(e.src.as_str()) {
                e.en = o.en.clone();
                e.tries = o.tries;
                e.last_error = o.last_error.clone();
            }
        }
    }

    pub fn done(&self) -> usize {
        self.entries.iter().filter(|e| e.en.is_some()).count()
    }

    pub fn failed(&self) -> usize {
        self.entries.iter().filter(|e| e.en.is_none() && e.tries >= MAX_TRIES).count()
    }

    pub fn complete(&self) -> bool {
        self.entries.iter().all(|e| e.en.is_some())
    }

    /// Characters of Russian still to send.
    pub fn pending_chars(&self) -> usize {
        self.entries.iter().filter(|e| e.en.is_none()).map(|e| e.src.chars().count()).sum()
    }

    /// Forgets every translation, to translate all again (another model or style).
    pub fn forget_all(&mut self) {
        self.made_with = None;
        for e in &mut self.entries {
            (e.en, e.tries) = (None, 0);
            e.last_error.clear();
        }
    }

    /// Gives the given-up entries another round.
    pub fn retry_failed(&mut self) {
        for e in self.entries.iter_mut().filter(|e| e.en.is_none()) {
            e.tries = 0;
        }
    }

    /// The next batch to send: up to `budget` characters and `max_items` entries, names
    /// while any are left. An entry refused before goes alone.
    pub fn next_batch(&self, budget: usize, max_items: usize) -> Vec<usize> {
        let open = |e: &Entry| e.en.is_none() && e.tries < MAX_TRIES;
        let names_left = self.entries.iter().any(|e| open(e) && e.name);
        let mut batch = Vec::new();
        let mut chars = 0;
        for (i, e) in self.entries.iter().enumerate() {
            if !open(e) || e.name != names_left {
                continue;
            }
            let n = e.src.chars().count();
            if e.tries > 0 {
                if batch.is_empty() {
                    return vec![i];
                }
                continue;
            }
            if !batch.is_empty() && (chars + n > budget || batch.len() >= max_items) {
                break;
            }
            batch.push(i);
            chars += n;
        }
        batch
    }

    /// Names already translated that the batch's texts seem to mention (each word of the
    /// name, its ending cut off for the Russian cases, found at the start of a word).
    pub fn glossary(&self, batch: &[usize], limit: usize) -> Vec<(String, String)> {
        let words = |s: &str| -> Vec<String> {
            s.to_lowercase().split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_string).collect()
        };
        let stem = |w: &str| -> String {
            let n = w.chars().count();
            let keep = if n >= 6 { n - 2 } else if n >= 4 { n - 1 } else { n };
            w.chars().take(keep).collect()
        };
        let text_words: Vec<String> = batch.iter().flat_map(|&i| words(&self.entries[i].src)).collect();
        let own = self
            .entries
            .iter()
            .enumerate()
            .filter(|(i, e)| e.name && !batch.contains(i))
            .filter_map(|(_, e)| Some((e.src.as_str(), e.en.as_deref()?)));
        let outside = self.outside_glossary.iter().map(|(ru, en)| (ru.as_str(), en.as_str()));
        let mut out: Vec<(String, String)> = Vec::new();
        for (ru, en) in own.chain(outside) {
            if ru.chars().count() > 60 || out.iter().any(|(r, _)| r == ru) {
                continue;
            }
            let name_words: Vec<String> = words(ru).into_iter().filter(|w| w.chars().count() >= 3).collect();
            if !name_words.is_empty() && name_words.iter().all(|w| text_words.iter().any(|t| t.starts_with(&stem(w)))) {
                out.push((ru.to_string(), en.to_string()));
                if out.len() >= limit {
                    break;
                }
            }
        }
        out
    }

    /// Takes the model's answers for `batch` (keyed by the batch's ids, see
    /// [`super::llm::user_prompt`]); refused ones count a try. Returns how many were taken.
    pub fn accept(&mut self, batch: &[usize], answers: &HashMap<String, String>) -> usize {
        let mut taken = 0;
        for (k, &i) in batch.iter().enumerate() {
            let e = &mut self.entries[i];
            let key = (k + 1).to_string();
            let Some(answer) = answers.get(&key) else {
                e.tries += 1;
                e.last_error = missing(&key, answers);
                e.last_answer = None;
                continue;
            };
            match text::restore(&e.src, &prepare(&e.src), answer, &self.language, self.any_script) {
                Ok(en) => {
                    e.en = Some(en);
                    e.last_error.clear();
                    e.last_answer = None;
                    taken += 1;
                }
                Err(why) => {
                    e.tries += 1;
                    e.last_error = why.message();
                    e.last_answer = Some(answer.clone());
                }
            }
        }
        taken
    }

    /// Counts a try for every entry of a batch whose answer could not be read at all.
    pub fn refuse(&mut self, batch: &[usize], why: &str) {
        for &i in batch {
            self.entries[i].tries += 1;
            self.entries[i].last_error = why.to_string();
            self.entries[i].last_answer = None;
        }
    }

    /// `s` (the map `file` of this job) in English: every translated string replaced, the
    /// next-map link pointing at the next chapter's English copy. Untranslated strings stay.
    pub fn english(&self, s: &Scenario) -> Scenario {
        let en: HashMap<&str, &str> =
            self.entries.iter().filter_map(|e| e.en.as_deref().map(|en| (e.src.as_str(), en))).collect();
        let mut out = s.clone();
        for (value, f) in fields(&mut out) {
            if f == Field::NextMap {
                if let Some(m) = self.maps.iter().find(|m| !value.trim().is_empty() && same_file(&m.file, value)) {
                    *value = m.out.clone();
                }
                continue;
            }
            let (_, core, _) = split(value, f);
            if let Some(t) = en.get(core) {
                *value = rebuild(value, f, t);
            }
        }
        for e in &mut out.events {
            e.flags = FlagScript::from_title(&e.title);
        }
        out
    }

    /// Where jobs are kept: `razdor/translate` in the platform data folder.
    pub fn dir() -> Option<PathBuf> {
        dirs::data_dir().map(|d| d.join("razdor").join("translate"))
    }

    /// The job file for the maps starting with `first`, translated into `language` (English
    /// keeps the name it had before languages existed).
    pub fn path_for(first: &str, language: &str, any_script: bool) -> Option<PathBuf> {
        let stem = Path::new(first).file_stem()?.to_string_lossy().into_owned();
        let name = match copy_suffix(language, any_script).as_str() {
            "en" => format!("{stem}.json"),
            l => format!("{stem}.{l}.json"),
        };
        Some(Job::dir()?.join(name))
    }

    pub fn load(path: &Path) -> Option<Job> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        crate::editor::files::write_atomically(path, json.as_bytes())
    }
}

/// The bytes of `s` written as `like` was: in a container of the same version, or bare.
pub fn file_bytes(s: &Scenario, like: &[u8]) -> Result<Vec<u8>, DtError> {
    let payload = s.to_payload();
    if container::has_magic(like) {
        let version = container::decode(like)?.version;
        return Ok(container::encode(version, &payload));
    }
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Event};
    use crate::editor::defaults::new_building;

    fn scenario(title: &str, next: &str) -> Scenario {
        let mut s = Scenario { title: title.into(), next_map: next.into(), ..Scenario::default() };
        s.description = "Озеро у #HERONAME.\r\nКонец".into();
        let mut b = new_building(&s.header, 1, 1, 1, 0, 0, (1, 1));
        (b.name, b.owner_name, b.description) = ("Мельница".into(), "Иван".into(), "Мельница Ивана".into());
        s.buildings.push(b);
        s.armies.push(Army { name: "Разбойники#2".into(), leader_name: " Иван ".into(), ..Default::default() });
        let mut e = Event { title: "Встреча%+a=b".into(), message: "Мельница Ивана сгорела".into(), ..Default::default() };
        e.flags = FlagScript::from_title(&e.title);
        s.events.push(e);
        s
    }

    #[test]
    fn gathers_distinct_strings_names_first() {
        let job = Job::new(&[("A.DTm".into(), scenario("Озеро", "B.DTm")), ("B.DTm".into(), scenario("Озеро 2", ""))], "English", false);
        let src: Vec<(&str, bool)> = job.entries.iter().map(|e| (e.src.as_str(), e.name)).collect();
        assert_eq!(
            src,
            [
                ("Озеро", true),
                ("Озеро у #HERONAME.\r\nКонец", false),
                ("Мельница", true),
                ("Иван", true),
                ("Мельница Ивана", false),
                ("Разбойники", true),
                ("Встреча", true),
                ("Мельница Ивана сгорела", false),
                ("Озеро 2", true),
            ]
        );
        assert_eq!(job.maps[1], JobMap { file: "B.DTm".into(), out: "B_en.DTm".into() });
        // Names go first, then prose.
        let first = job.next_batch(10_000, 100);
        assert!(first.iter().all(|&i| job.entries[i].name));
        assert_eq!(first.len(), 6);
    }

    #[test]
    fn writes_english_keeping_scripts_suffixes_and_links() {
        let maps = [("A.DTm".to_string(), scenario("Озеро", "b.dtm")), ("B.DTm".to_string(), scenario("Озеро 2", ""))];
        let mut job = Job::new(&maps, "English", false);
        let answers = |job: &Job, batch: &[usize]| -> HashMap<String, String> {
            let en = |s: &str| match s {
                "Озеро" => "Lake",
                "Озеро 2" => "Lake 2",
                "Мельница" => "Mill",
                "Иван" => "Ivan",
                "Разбойники" => "Robbers",
                "Встреча" => "Meeting",
                "Мельница Ивана" => "Ivan's mill",
                "Мельница Ивана сгорела" => "Ivan's mill burned down",
                _ => "The lake by {0}.\nThe end",
            };
            batch.iter().enumerate().map(|(k, &i)| ((k + 1).to_string(), en(&job.entries[i].src).to_string())).collect()
        };
        while !job.complete() {
            let batch = job.next_batch(10_000, 100);
            let a = answers(&job, &batch);
            assert_eq!(job.accept(&batch, &a), batch.len());
        }
        let out = job.english(&maps[0].1);
        assert_eq!(out.title, "Lake");
        assert_eq!(out.next_map, "B_en.DTm");
        assert_eq!(out.description, "The lake by #HERONAME.\r\nThe end");
        assert_eq!(out.armies[0].name, "Robbers#2");
        assert_eq!(out.armies[0].leader_name, " Ivan ");
        assert_eq!(out.events[0].title, "Meeting%+a=b");
        assert_eq!(out.events[0].flags, FlagScript::from_title("Meeting%+a=b"));
        // The English map reads back the same.
        let back = Scenario::parse_payload(&out.to_payload()).unwrap();
        assert_eq!(back.events[0].message, "Ivan's mill burned down");
    }

    #[test]
    fn the_glossary_finds_names_in_inflected_text() {
        let mut job = Job::new(&[("A.DTm".into(), scenario("Озеро", ""))], "English", false);
        let names = job.next_batch(10_000, 100);
        let a: HashMap<String, String> = names.iter().enumerate().map(|(k, _)| ((k + 1).to_string(), "X".to_string())).collect();
        job.accept(&names, &a);
        let prose = job.next_batch(10_000, 100);
        let g: Vec<String> = job.glossary(&prose, 50).into_iter().map(|(ru, _)| ru).collect();
        // "Ивана" and "Озеро у" are matched; "Встреча" is not mentioned.
        assert_eq!(g, ["Озеро", "Мельница", "Иван"]);
    }

    #[test]
    fn refused_answers_retry_alone_then_give_up() {
        let mut job = Job::new(&[("A.DTm".into(), scenario("Озеро", ""))], "English", false);
        let batch = job.next_batch(10_000, 100);
        job.accept(&batch, &HashMap::new());
        assert_eq!(job.next_batch(10_000, 100), vec![batch[0]]);
        for _ in 1..MAX_TRIES {
            job.refuse(&[batch[0]], "bad");
        }
        assert_ne!(job.next_batch(10_000, 100), vec![batch[0]]);
        assert!(job.failed() >= 1);
        job.retry_failed();
        assert_eq!(job.failed(), 0);
    }

    #[test]
    fn a_refusal_says_which_key_was_missing_and_keeps_the_refused_answer() {
        let mut job = Job::from_texts([("Озеро", true), ("Мельница", true), ("*Конец", true)]);
        let batch: Vec<usize> = (0..3).collect();
        let answers: HashMap<String, String> = [("1", "Lake"), ("10", "Mill"), ("3", "The end")].map(|(k, v)| (k.to_string(), v.to_string())).into();
        assert_eq!(job.accept(&batch, &answers), 1);
        let e = &job.entries[1];
        assert_eq!(e.last_error, "no text under key \"2\"; the answer has keys 1, 3, 10");
        assert_eq!(e.last_answer, None);
        let e = &job.entries[2];
        assert_eq!(e.last_error, "markup mark `*`: 1 in the original, 0 in the translation");
        assert_eq!(e.last_answer.as_deref(), Some("The end"));
        assert_eq!(job.entries[0].en.as_deref(), Some("Lake"));
        // An answer with no texts at all.
        assert_eq!(missing("1", &HashMap::new()), "no text under key \"1\": the answer holds no texts");
        let many: HashMap<String, String> = (1..=20).map(|k| (k.to_string(), String::new())).collect();
        assert_eq!(missing("21", &many), "no text under key \"21\"; the answer has keys 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, …");
    }

    #[test]
    fn campaigns_follow_the_next_map_links() {
        let maps = vec![
            ("C.DTm".to_string(), scenario("c", "")),
            ("A.DTm".to_string(), scenario("a", "b.DTm")),
            ("B.DTm".to_string(), scenario("b", "C")),
            ("Solo.DTm".to_string(), scenario("s", "")),
        ];
        assert_eq!(campaign(&maps, 2), [1, 2, 0]);
        assert_eq!(campaign(&maps, 0), [1, 2, 0]);
        assert_eq!(campaign(&maps, 3), [3]);
    }

    /// Every map of the install, "translated" by a stand-in that keeps all a model must keep
    /// (Cyrillic letters become `x`), comes back from its file with nothing changed but its
    /// texts: the flag scripts, army tails, events, armies and terrain are the original's.
    #[test]
    fn real_maps_keep_everything_but_their_texts() {
        let Ok(dir) = std::env::var(crate::dt::install::ENV_VAR) else { return };
        let maps = crate::translate::install_maps(Path::new(&dir));
        assert!(!maps.is_empty());
        let blank = |s: &Scenario| {
            let mut s = s.clone();
            for (v, f) in fields(&mut s) {
                if f != Field::NextMap {
                    v.clear();
                }
            }
            s
        };
        // The shipped maps (translated copies made by this tool are not sources).
        for (file, path, s) in maps.iter().filter(|(f, _, _)| !is_translated_file(f)) {
            let mut job = Job::new(&[(file.clone(), s.clone())], "English", false);
            let batch: Vec<usize> = (0..job.entries.len()).collect();
            let answers: HashMap<String, String> = batch
                .iter()
                .map(|&i| {
                    let fake: String = prepare(&job.entries[i].src).text.chars().map(|c| if has_cyrillic(&c.to_string()) { 'x' } else { c }).collect();
                    ((i + 1).to_string(), fake)
                })
                .collect();
            assert_eq!(job.accept(&batch, &answers), batch.len(), "{file}: {:?}", job.entries.iter().find(|e| e.en.is_none()));
            let like = std::fs::read(path).unwrap();
            let en = Scenario::from_file_bytes(&file_bytes(&job.english(s), &like).unwrap()).unwrap();
            assert_eq!(blank(&en), blank(s), "{file}: more than the texts changed");
            for (a, b) in en.events.iter().zip(&s.events) {
                assert_eq!(a.flags, b.flags, "{file}: event {:?}", b.title);
            }
            for (a, b) in en.armies.iter().zip(&s.armies) {
                assert_eq!(a.name.find('#').map(|i| &a.name[i..]), b.name.find('#').map(|i| &b.name[i..]), "{file}");
            }
        }
    }

    #[test]
    fn english_file_names() {
        assert_eq!(english_file("Проклятое озеро.DTm"), "Проклятое озеро_en.DTm");
        assert!(is_english_file("Проклятое озеро_en.DTm"));
        assert!(!is_english_file("Проклятое озеро.DTm"));
        assert_eq!(translated_file("Проклятое озеро.DTm", " Hindi "), "Проклятое озеро_hindi.DTm");
        assert_eq!(translated_file("Проклятое озеро.DTm", "english"), "Проклятое озеро_en.DTm");
        assert_eq!(translated_file("Проклятое озеро.DTm", "Українська"), "Проклятое озеро_tr.DTm");
        assert!(is_translated_file("Проклятое озеро_hindi.DTm") && is_translated_file("РК1-Начало пути_en.DTm"));
        assert!(!is_translated_file("Проклятое озеро.DTm") && !is_translated_file("my_map.DTm") && !is_translated_file("РК1-Начало_пути.DTm"));
        assert_eq!(Job::path_for("A.DTm", "English", false).unwrap().file_name().unwrap(), "A.json");
        assert_eq!(Job::path_for("A.DTm", "German", false).unwrap().file_name().unwrap(), "A.german.json");
        assert_eq!(Job::path_for("A.DTm", "Hindi", true).unwrap().file_name().unwrap(), "A.hindi-native.json");
        assert_eq!(copy_file("Озеро.DTm", &copy_suffix("Hindi", true)), "Озеро_hindi-native.DTm");
        assert_eq!(copy_suffix("English", true), "en");
        assert!(is_translated_file("Озеро_hindi-native.DTm"));
    }
}
