//! The install's own texts (unit, item and spell names and descriptions, the game's interface
//! texts) for the translator, and the files it writes for Razdor in the translator's target
//! language: `Eng_Units.ini` (English) or `<Prefix>_Units.ini` ([`data_prefix`]) and the
//! like next to the Russian ones, with the same sections and only the translated keys, the
//! first line naming the language (`crate::dt::english::HEADER`). The Russian files stay as
//! they are (the original reads only them, and unit upgrades name their targets by the
//! Russian name, 0x4e0448); Razdor reads a language's files when the player picks it
//! (`crate::dt::english`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use super::job::{copy_suffix, Job};
use super::text::has_cyrillic;
use crate::dt::english::{data_path, ENGLISH, HEADER};
use crate::dt::install::{find_path, ARTEFACTS_FILE, SETTINGS_FILE, SPELLS_FILE, UNITS_FILE};

/// The prefix of a language's data files: `Eng` for English (the files Razdor had before
/// languages), else [`copy_suffix`] with a capital (`Hindi-native`, `German`). Never the
/// Russian files' `Rus`.
pub fn data_prefix(language: &str, any_script: bool) -> String {
    let s = copy_suffix(language, any_script);
    if s == "en" {
        return ENGLISH.to_string();
    }
    let mut c = s.chars();
    let p: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    let russian = UNITS_FILE.split('_').next().unwrap_or_default();
    if p.eq_ignore_ascii_case(russian) { format!("{p}-tr") } else { p }
}

/// A language's name for Razdor's language list: as the player wrote it with a capital, and
/// "(native letters)" with its own letters (`Hindi (native letters)`).
pub fn language_name(language: &str, any_script: bool) -> String {
    if copy_suffix(language, any_script) == "en" {
        return "English".to_string();
    }
    let l = language.trim();
    let mut c = l.chars();
    let name: String = c.next().map(|f| f.to_uppercase().chain(c).collect()).unwrap_or_default();
    if any_script { format!("{name} (native letters)") } else { name }
}

/// A data file: the Russian one, and the keys translated (`None`: every value with
/// Cyrillic in it) with whether each is a name. Its translated copy is
/// [`crate::dt::english::data_file`].
pub struct DataFile {
    pub rus: &'static str,
    keys: Option<&'static [(&'static str, bool)]>,
}

const NAME_AND_TEXT: &[(&str, bool)] = &[("Name", true), ("Descript", false)];

pub const FILES: [DataFile; 4] = [
    DataFile { rus: UNITS_FILE, keys: Some(NAME_AND_TEXT) },
    DataFile { rus: ARTEFACTS_FILE, keys: Some(NAME_AND_TEXT) },
    DataFile { rus: SPELLS_FILE, keys: Some(&[("Name", true)]) },
    DataFile { rus: SETTINGS_FILE, keys: None },
];

/// One text of a data file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Text {
    pub file: usize,
    /// Its section's place among all sections of the file (a name may repeat: every spell
    /// of `Rus_Spells.ini` is `[Заклинание]`).
    pub section_no: usize,
    pub section: String,
    pub key: String,
    pub value: String,
    pub name: bool,
}

/// The install's translatable texts and the section names of each data file in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DataTexts {
    pub sections: Vec<Vec<String>>,
    pub texts: Vec<Text>,
}

/// The section names of an ini text in order, and its `key=value` lines with their
/// section's place (lines before the first section are in none, as the original reads them).
pub fn ini_lines(text: &str) -> (Vec<String>, Vec<(usize, String, String)>) {
    let ini = crate::dt::ini::Ini::parse(text);
    let names = ini.sections.iter().map(|s| s.name.clone()).collect();
    let lines = ini.sections.iter().enumerate().flat_map(|(no, s)| s.entries.iter().map(move |(k, v)| (no, k.trim().to_string(), v.clone()))).collect();
    (names, lines)
}

/// The section names and translatable texts of file `file` (an index into [`FILES`]) from
/// its contents.
pub fn texts_of(file: usize, contents: &str) -> (Vec<String>, Vec<Text>) {
    let keys = FILES[file].keys;
    let (sections, lines) = ini_lines(contents);
    let texts = lines
        .into_iter()
        .filter_map(|(section_no, key, value)| {
            if !has_cyrillic(&value) {
                return None;
            }
            let name = match keys {
                Some(keys) => keys.iter().find(|(k, _)| k.eq_ignore_ascii_case(&key))?.1,
                // An interface text: short one-liners are names (labels, bonus names).
                None => value.trim().chars().count() <= 40 && !value.contains("#\\"),
            };
            Some(Text { file, section_no, section: sections[section_no].clone(), key, value, name })
        })
        .collect();
    (sections, texts)
}

/// The install's translatable texts, from the data files it has.
pub fn install_texts(install: &Path) -> DataTexts {
    let mut out = DataTexts::default();
    for (i, f) in FILES.iter().enumerate() {
        let bytes = find_path(install, f.rus).ok().and_then(|p| std::fs::read(p).ok()).unwrap_or_default();
        let (sections, texts) = texts_of(i, &crate::dt::text::decode(&bytes));
        out.sections.push(sections);
        out.texts.extend(texts);
    }
    out
}

/// The job for the install's texts into `language` (with its own letters when
/// `any_script`): the saved one's translations kept.
pub fn open_job(data: &DataTexts, language: &str, any_script: bool) -> (Job, Option<PathBuf>) {
    let mut job = Job::from_texts(data.texts.iter().map(|t| (t.value.trim(), t.name)));
    job.language = language.trim().to_string();
    job.any_script = any_script;
    if job.language.is_empty() {
        job.language = "English".to_string();
    }
    let path = data_job_path(&job.language, any_script);
    if let Some(old) = path.as_deref().and_then(Job::load) {
        job.take_over(&old);
    }
    (job, path)
}

/// Where the game data's job in `language` is kept (its name cannot be a map's: maps end in
/// `.DTm`): `_game data.json` for English (as before languages), else
/// `_game data.<suffix>.json`, like maps' ([`Job::path_for`]).
pub fn data_job_path(language: &str, any_script: bool) -> Option<PathBuf> {
    let name = match copy_suffix(language, any_script).as_str() {
        "en" => "_game data.json".to_string(),
        l => format!("_game data.{l}.json"),
    };
    Some(Job::dir()?.join(name))
}

/// The contents of each file of the job's language: the [`HEADER`] line naming it, then every
/// section of the Russian file in its order (so Razdor pairs them by place: names repeat),
/// with the translated keys under it, CR LF lines. Each line is cp1251 when it can be, else
/// UTF-8 (`crate::dt::text::encode_any`), so the section names stay as in the Russian file.
/// A text not translated yet is left out (Razdor then shows the Russian).
pub fn data_files(job: &Job, data: &DataTexts) -> Vec<(usize, Vec<u8>)> {
    let mut out = Vec::new();
    for (i, sections) in data.sections.iter().enumerate() {
        let mut any = false;
        let mut lines = vec![format!("{HEADER}{}", language_name(&job.language, job.any_script))];
        for (no, name) in sections.iter().enumerate() {
            lines.push(String::new());
            lines.push(format!("[{name}]"));
            for t in data.texts.iter().filter(|t| t.file == i && t.section_no == no) {
                let core = t.value.trim();
                let Some(en) = job.english_of(core) else { continue };
                let lead = &t.value[..t.value.len() - t.value.trim_start().len()];
                let trail = &t.value[lead.len() + core.len()..];
                lines.push(format!("{}={lead}{en}{trail}", t.key));
                any = true;
            }
        }
        if any {
            let mut bytes = Vec::new();
            for l in &lines {
                bytes.extend(crate::dt::text::encode_any(l));
                bytes.extend_from_slice(b"\r\n");
            }
            out.push((i, bytes));
        }
    }
    out
}

/// Writes the job's language's files next to the Russian ones; returns them.
pub fn write(job: &Job, data: &DataTexts, install: &Path) -> Result<Vec<PathBuf>, String> {
    let prefix = data_prefix(&job.language, job.any_script);
    let mut written = Vec::new();
    for (i, bytes) in data_files(job, data) {
        let target = data_path(install, &prefix, FILES[i].rus).ok_or_else(|| format!("{}: not found", FILES[i].rus))?;
        crate::fsutil::write_atomically(&target, &bytes).map_err(|e| format!("{}: {e}", target.display()))?;
        written.push(target);
    }
    Ok(written)
}

/// The language's units file exists in the install (the data is translated into it).
pub fn translated(install: &Path, language: &str, any_script: bool) -> bool {
    data_path(install, &data_prefix(language, any_script), UNITS_FILE).is_some_and(|p| p.exists())
}

/// The names of the game data already translated into `language` (Russian, translation),
/// for the glossary of map jobs in that language: a map's texts then call a unit what its
/// card calls it.
pub fn glossary(language: &str, any_script: bool) -> Vec<(String, String)> {
    let Some(job) = data_job_path(language, any_script).as_deref().and_then(Job::load) else { return Vec::new() };
    let mut seen = BTreeMap::new();
    for e in job.entries.iter().filter(|e| e.name) {
        if let Some(en) = &e.en {
            seen.entry(e.src.clone()).or_insert_with(|| en.clone());
        }
    }
    seen.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const UNITS: &str = "[Unit1]\r\nGlobalIndex=1\r\nName=Мечник\r\nDescript=Воин с мечом.#\\Крепкий.\r\nNextUnit1=Рыцарь\r\nCost=10\r\n\r\n[Empty]\r\nCost=1\r\n[Unit2]\r\nName=Рыцарь\r\nDescript= Конный воин \r\n";

    fn data_of(file: usize, contents: &str) -> DataTexts {
        let (s, texts) = texts_of(file, contents);
        let mut sections = vec![Vec::new(); FILES.len()];
        sections[file] = s;
        DataTexts { sections, texts }
    }

    #[test]
    fn takes_only_the_text_keys() {
        let (sections, t) = texts_of(0, UNITS);
        assert_eq!(sections, ["Unit1", "Empty", "Unit2"]);
        let got: Vec<(usize, &str, &str, bool)> = t.iter().map(|t| (t.section_no, t.key.as_str(), t.value.as_str(), t.name)).collect();
        assert_eq!(
            got,
            [(0, "Name", "Мечник", true), (0, "Descript", "Воин с мечом.#\\Крепкий.", false), (2, "Name", "Рыцарь", true), (2, "Descript", " Конный воин", false)],
            "NextUnit (a reference by name) and numbers are left alone; trailing blanks go, as the original reads lines"
        );
        let (_, ui) = texts_of(3, "[Buttons]\nOk=Да\nLong=Очень длинный текст, который никак не может быть просто названием кнопки\nN=5\n");
        assert_eq!(ui.iter().map(|t| t.name).collect::<Vec<_>>(), [true, false]);
    }

    #[test]
    fn writes_every_section_in_place_so_razdor_pairs_them() {
        let data = data_of(2, "[Заклинание]\nName=Лечение\n[Заклинание]\nName=Молния\n");
        let mut job = Job::from_texts(data.texts.iter().map(|t| (t.value.trim(), t.name)));
        let batch: Vec<usize> = (0..job.entries.len()).collect();
        let answers: HashMap<String, String> = [("1", "Healing"), ("2", "Lightning")].map(|(k, v)| (k.to_string(), v.to_string())).into();
        assert_eq!(job.accept(&batch, &answers), 2);
        let files = data_files(&job, &data);
        let text = crate::dt::text::decode(&files[0].1);
        assert!(text.ends_with("\r\n[Заклинание]\r\nName=Healing\r\n\r\n[Заклинание]\r\nName=Lightning\r\n"), "{text:?}");
    }

    #[test]
    fn writes_the_english_file_by_section() {
        let data = data_of(0, UNITS);
        let mut job = Job::from_texts(data.texts.iter().map(|t| (t.value.trim(), t.name)));
        let batch: Vec<usize> = (0..job.entries.len()).collect();
        let en = |s: &str| match s {
            "Мечник" => "Swordsman",
            "Рыцарь" => "Knight",
            "Воин с мечом.#\\Крепкий." => "A warrior with a sword.<br>Sturdy.",
            _ => "A mounted warrior",
        };
        let answers: HashMap<String, String> = batch.iter().map(|&i| ((i + 1).to_string(), en(&job.entries[i].src).to_string())).collect();
        assert_eq!(job.accept(&batch, &answers), 4);
        let files = data_files(&job, &data);
        assert_eq!(files.len(), 1);
        let text = crate::dt::text::decode(&files[0].1);
        assert!(
            text.ends_with("\r\n[Unit1]\r\nName=Swordsman\r\nDescript=A warrior with a sword.#\\Sturdy.\r\n\r\n[Empty]\r\n\r\n[Unit2]\r\nName=Knight\r\nDescript= A mounted warrior\r\n"),
            "{text:?}"
        );
        assert!(text.starts_with("; Razdor translation: English\r\n"), "{text:?}");
    }

    #[test]
    fn each_language_has_its_prefix_name_and_job_file() {
        assert_eq!(data_prefix("English", false), "Eng");
        assert_eq!(data_prefix("", true), "Eng", "English has no native-letters copy");
        assert_eq!(data_prefix("Hindi", true), "Hindi-native");
        assert_eq!(data_prefix("german", false), "German");
        assert_eq!(data_prefix("Rus", false), "Rus-tr", "never the Russian files");
        assert_eq!(language_name("hindi", true), "Hindi (native letters)");
        assert_eq!(language_name("German", false), "German");
        assert_eq!(language_name("en", false), "English");
        let name = |l: &str, n: bool| data_job_path(l, n).map(|p| p.file_name().unwrap().to_string_lossy().into_owned());
        if Job::dir().is_some() {
            assert_eq!(name("English", false).as_deref(), Some("_game data.json"), "English keeps its file");
            assert_eq!(name("Hindi", true).as_deref(), Some("_game data.hindi-native.json"));
        }
    }

    #[test]
    fn a_native_letters_file_keeps_its_sections_in_cp1251_and_reads_back() {
        let data = data_of(2, "[Заклинание]\nName=Лечение\n[Заклинание]\nName=Молния\n");
        let mut job = Job::from_texts(data.texts.iter().map(|t| (t.value.trim(), t.name)));
        (job.language, job.any_script) = ("Hindi".into(), true);
        let batch: Vec<usize> = (0..job.entries.len()).collect();
        let answers: HashMap<String, String> = [("1", "उपचार"), ("2", "बिजली")].map(|(k, v)| (k.to_string(), v.to_string())).into();
        assert_eq!(job.accept(&batch, &answers), 2);
        let bytes = &data_files(&job, &data)[0].1;
        assert!(bytes.starts_with(b"; Razdor translation: Hindi (native letters)\r\n"));
        let section = crate::dt::text::encode("[Заклинание]");
        assert!(bytes.windows(section.len()).any(|w| w == section.as_slice()), "section names stay cp1251");
        let ini = crate::dt::ini::Ini::from_cp1251(bytes);
        let names: Vec<&str> = ini.sections.iter().filter_map(|s| s.get("Name")).collect();
        assert_eq!(names, ["उपचार", "बिजली"]);
        assert_eq!(ini.sections[0].name, "Заклинание");
    }
}
