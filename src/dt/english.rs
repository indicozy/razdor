//! Translated texts for the install's data, made by the map translator (`razdor-translate`):
//! `<Prefix>_Units.ini`, `<Prefix>_Artefacts.ini`, `<Prefix>_Spells.ini` and
//! `<Prefix>_DiscordTimes.ini` next to the Russian files, with the same sections and only the
//! translated keys. English is `Eng_*.ini`; another language's prefix is named after it
//! (`Hindi-native_Units.ini`, `German_Units.ini`) and its files start with a comment naming
//! the language ([`HEADER`], [`discover`]). Not part of the original game, which reads only
//! the Russian files; Razdor uses them when the player picks that language (asked for by the
//! owner).
//!
//! When the install has no English files, Razdor's own copies made from the Community
//! Update's files are used (`data/lang/en/game/`, [`bundled`]); a file whose sections do not
//! line up with the install's Russian one (another version, a mod) is left out.
//!
//! The Russian data is loaded as it is (unit upgrades name their targets by the Russian name,
//! 0x4e0448); only the names and descriptions shown are swapped afterwards, by the Russian
//! text they translate ([`Dictionary::english`]).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use super::data::{ArtefactDef, SpellDef, UnitDef};
use super::ini::Ini;
use super::install::{find_path, ARTEFACTS_FILE, SETTINGS_FILE, SPELLS_FILE, UNITS_FILE};

/// The prefix of the English files.
pub const ENGLISH: &str = "Eng";

/// The start of the first line of a translation's files, followed by the language's name for
/// the language list: `; Razdor translation: Hindi (native letters)`.
pub const HEADER: &str = "; Razdor translation: ";

/// The file of language `prefix` for a Russian data file: `Rus_Units.ini` → `Hindi-native_Units.ini`.
pub fn data_file(prefix: &str, rus: &str) -> String {
    match rus.split_once('_') {
        Some((_, rest)) => format!("{prefix}_{rest}"),
        None => format!("{prefix}_{rus}"),
    }
}

/// The English file of a Russian data file: `Rus_Units.ini` → `Eng_Units.ini`.
pub fn english_file(rus: &str) -> String {
    data_file(ENGLISH, rus)
}

/// The language named by a file's first line ([`HEADER`]), if it is a translation's.
pub fn header_name(first_line: &str) -> Option<String> {
    let line = first_line.trim_start_matches('\u{feff}').trim();
    let name = line.strip_prefix(HEADER.trim_end())?.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// The prefix of a units file other than the Russian one: `Hindi-native_Units.ini` →
/// `Hindi-native` (`_Units.ini` in any case).
pub fn units_prefix(file: &str) -> Option<&str> {
    let tail = data_file("", UNITS_FILE);
    let cut = file.len().checked_sub(tail.len())?;
    let (prefix, end) = (file.get(..cut)?, file.get(cut..)?);
    let russian = UNITS_FILE.split('_').next().unwrap_or_default();
    (end.eq_ignore_ascii_case(&tail) && !prefix.is_empty() && !prefix.eq_ignore_ascii_case(russian)).then_some(prefix)
}

/// A translation of the game data found in an install.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DataLanguage {
    /// Its files' prefix (`Hindi-native`).
    pub prefix: String,
    /// The language's name from its files' first line (`Hindi (native letters)`).
    pub name: String,
}

/// The translations of the data in the install at `dir` other than English (always there:
/// Razdor's own copies): each `<Prefix>_Units.ini` whose first line names its language
/// ([`HEADER`]), sorted by name.
pub fn discover(dir: &Path) -> Vec<DataLanguage> {
    // A mod's merged folder lists the base's files under it too (`install::read_dir_overlaid`).
    let Ok(read) = super::install::read_dir_overlaid(dir) else { return Vec::new() };
    let mut out: Vec<DataLanguage> = read
        .into_iter()
        .filter_map(|e| {
            let file = e.file_name().to_string_lossy().into_owned();
            let prefix = units_prefix(&file)?;
            if prefix.eq_ignore_ascii_case(ENGLISH) {
                return None;
            }
            let bytes = std::fs::read(e.path()).ok()?;
            let first = bytes.split(|&b| b == b'\r' || b == b'\n').next().unwrap_or_default();
            let name = header_name(&super::text::decode(first))?;
            Some(DataLanguage { prefix: prefix.to_string(), name })
        })
        .collect();
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then_with(|| a.prefix.cmp(&b.prefix)));
    out
}

/// Razdor's own English file `name` (`data/lang/en/game/`), made by razdor-translate from the
/// Community Update's Russian files.
pub fn bundled(name: &str) -> Option<&'static [u8]> {
    const FILES: [(&str, &[u8]); 4] = [
        ("Eng_Units.ini", include_bytes!("../../data/lang/en/game/Eng_Units.ini")),
        ("Eng_Artefacts.ini", include_bytes!("../../data/lang/en/game/Eng_Artefacts.ini")),
        ("Eng_Spells.ini", include_bytes!("../../data/lang/en/game/Eng_Spells.ini")),
        ("Eng_DiscordTimes.ini", include_bytes!("../../data/lang/en/game/Eng_DiscordTimes.ini")),
    ];
    FILES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, b)| *b)
}

fn read(dir: &Path, name: &str) -> Option<Ini> {
    let bytes = std::fs::read(find_path(dir, name).ok()?).ok()?;
    Some(Ini::from_cp1251(&bytes))
}

/// The file of language `prefix` for the Russian data file `rus`: the install's own, else
/// (English only) Razdor's copy.
pub fn data_ini(dir: &Path, prefix: &str, rus: &str) -> Option<Ini> {
    let name = data_file(prefix, rus);
    read(dir, &name).or_else(|| bundled(&name).map(Ini::from_cp1251))
}

/// The English file of the Russian data file `rus`: the install's own, else Razdor's copy.
pub fn english_ini(dir: &Path, rus: &str) -> Option<Ini> {
    data_ini(dir, ENGLISH, rus)
}

/// Where language `prefix`'s file for the Russian data file `rus` of the install at `dir`
/// goes: next to the Russian one.
pub fn data_path(dir: &Path, prefix: &str, rus: &str) -> Option<PathBuf> {
    find_path(dir, rus).ok().map(|p| p.with_file_name(data_file(prefix, rus)))
}

/// A translation of the install's data (English, or another language's files): by Russian
/// text for names and descriptions, by section and key for the interface texts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Dictionary {
    texts: HashMap<String, String>,
    interface: Option<Ini>,
}

impl Dictionary {
    /// The English files of the install at `dir`; `None` when it has none.
    pub fn load(dir: &Path) -> Option<Dictionary> {
        Dictionary::load_prefix(dir, ENGLISH)
    }

    /// The files of language `prefix` (`Eng`, `Hindi-native`) of the install at `dir`;
    /// `None` when it has none.
    pub fn load_prefix(dir: &Path, prefix: &str) -> Option<Dictionary> {
        let mut d = Dictionary { interface: data_ini(dir, prefix, SETTINGS_FILE), ..Dictionary::default() };
        for rus in [UNITS_FILE, ARTEFACTS_FILE, SPELLS_FILE] {
            let (Some(ru), Some(tr)) = (read(dir, rus), data_ini(dir, prefix, rus)) else { continue };
            d.add_pairs(&ru, &tr);
        }
        (!d.texts.is_empty() || d.interface.is_some()).then_some(d)
    }

    /// Pairs each translated value of `en` with the value of the same key in the section at
    /// the same place in `ru` (section names repeat: every spell is `[Заклинание]`); a file
    /// whose sections do not line up is left out.
    fn add_pairs(&mut self, ru: &Ini, en: &Ini) {
        if en.sections.len() != ru.sections.len() || en.sections.iter().zip(&ru.sections).any(|(e, r)| e.name != r.name) {
            crate::diag!("english: a file's sections do not match its Russian file; it is not used");
            return;
        }
        for (s, r) in en.sections.iter().zip(&ru.sections) {
            for (k, v) in &s.entries {
                if let Some(rv) = r.get(k).filter(|rv| !rv.trim().is_empty()) {
                    self.texts.insert(rv.trim().to_string(), v.trim().to_string());
                }
            }
        }
    }

    /// The English of a Russian name or description, if translated.
    pub fn english(&self, ru: &str) -> Option<&str> {
        self.texts.get(ru.trim()).map(String::as_str)
    }

    /// The English interface text of `[section] key`, if translated.
    pub fn interface(&self, section: &str, key: &str) -> Option<&str> {
        self.interface.as_ref()?.section(section)?.get_nonempty(key)
    }

    /// `units`, `items` and `spells` with their names and descriptions in English where
    /// translated (the rest as they are).
    pub fn apply(&self, units: &mut [UnitDef], items: &mut [ArtefactDef], spells: &mut [SpellDef]) {
        let swap = |s: &mut String| {
            if let Some(en) = self.english(s) {
                *s = en.to_string();
            }
        };
        for u in units {
            swap(&mut u.name);
            swap(&mut u.description);
        }
        for a in items {
            swap(&mut a.name);
            swap(&mut a.description);
        }
        for s in spells {
            swap(&mut s.name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The install's English files (its own, else Razdor's copies): they line up with the
    /// Russian ones and name every unit, item and spell.
    #[test]
    fn real_english_files_cover_the_data() {
        let Some(dir) = std::env::var_os(super::super::install::ENV_VAR) else { return };
        let dir = std::path::PathBuf::from(dir);
        let dt = super::super::install::DtInstall::load(&dir).unwrap();
        let d = dt.english.as_ref().expect("the English files load");
        let missing: Vec<&str> = dt
            .units
            .iter()
            .map(|u| u.name.as_str())
            .chain(dt.artefacts.iter().map(|a| a.name.as_str()))
            .chain(dt.spells.iter().map(|s| s.name.as_str()))
            .filter(|n| d.english(n).is_none())
            .collect();
        assert!(missing.is_empty(), "untranslated: {missing:?}");
    }

    #[test]
    fn razdor_ships_the_four_english_files() {
        for rus in [UNITS_FILE, ARTEFACTS_FILE, SPELLS_FILE, SETTINGS_FILE] {
            let ini = Ini::from_cp1251(bundled(&english_file(rus)).expect("bundled"));
            assert!(!ini.sections.is_empty(), "{rus}");
        }
    }

    #[test]
    fn english_file_names() {
        assert_eq!(english_file("Rus_Units.ini"), "Eng_Units.ini");
        assert_eq!(english_file("Rus_DiscordTimes.ini"), "Eng_DiscordTimes.ini");
        assert_eq!(data_file("Hindi-native", "Rus_Spells.ini"), "Hindi-native_Spells.ini");
    }

    #[test]
    fn prefixes_and_names_from_files() {
        assert_eq!(units_prefix("Hindi-native_Units.ini"), Some("Hindi-native"));
        assert_eq!(units_prefix("German_units.INI"), Some("German"));
        assert_eq!(units_prefix("Eng_Units.ini"), Some("Eng"));
        assert_eq!(units_prefix("Rus_Units.ini"), None, "the Russian file is the game itself");
        assert_eq!(units_prefix("_Units.ini"), None);
        assert_eq!(units_prefix("Hindi_Spells.ini"), None);
        assert_eq!(header_name("; Razdor translation: Hindi (native letters)\r").as_deref(), Some("Hindi (native letters)"));
        assert_eq!(header_name("\u{feff};Razdor translation:German").as_deref(), None, "the header as written");
        assert_eq!(header_name("; Razdor translation:   "), None);
        assert_eq!(header_name("; English texts for Razdor, made by razdor-translate from the Russian file."), None);
    }

    /// A scratch install folder with a Russian units file and the files given.
    fn scratch(name: &str, files: &[(&str, &[u8])]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("razdor-english-{name}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(dir.join(UNITS_FILE), crate::dt::text::encode("[Unit1]\r\nName=Мечник\r\nDescript=Воин\r\n[Unit2]\r\nName=Лучник\r\n")).unwrap();
        for (f, b) in files {
            std::fs::write(dir.join(f), b).unwrap();
        }
        dir
    }

    #[test]
    fn finds_the_translations_named_by_their_first_line() {
        let hindi = "; Razdor translation: Hindi (native letters)\r\n\r\n[Unit1]\r\nName=तलवारबाज़\r\n\r\n[Unit2]\r\n";
        let dir = scratch(
            "discover",
            &[
                ("Hindi-native_Units.ini", hindi.as_bytes()),
                ("German_Units.ini", b"; Razdor translation: German\r\n[Unit1]\r\nName=Schwertkampfer\r\n[Unit2]\r\n"),
                ("Eng_Units.ini", b"; Razdor translation: English\r\n"),
                ("Mod_Units.ini", b"[Unit1]\r\nName=X\r\n"),
            ],
        );
        let found = discover(&dir);
        let got: Vec<(&str, &str)> = found.iter().map(|l| (l.prefix.as_str(), l.name.as_str())).collect();
        assert_eq!(got, [("German", "German"), ("Hindi-native", "Hindi (native letters)")], "English is always there; a file with no header is not a translation");
        let d = Dictionary::load_prefix(&dir, "Hindi-native").expect("its files load");
        assert_eq!(d.english("Мечник"), Some("तलवारबाज़"));
        assert_eq!(d.english("Лучник"), None);
        assert_eq!(Dictionary::load_prefix(&dir, "German").and_then(|d| d.english("Мечник").map(str::to_string)).as_deref(), Some("Schwertkampfer"));
        assert!(Dictionary::load_prefix(&dir, "French").is_none(), "no files: none (no bundled copy but English)");
        assert_eq!(data_path(&dir, "German", UNITS_FILE), Some(dir.join("German_Units.ini")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Every translation the install has (beyond English) lines up with the Russian files
    /// and names its language (skipped without `RAZDOR_DT_DIR` or such files).
    #[test]
    fn real_translations_line_up() {
        let Some(dir) = std::env::var_os(super::super::install::ENV_VAR) else { return };
        let dir = std::path::PathBuf::from(dir);
        for l in discover(&dir) {
            let d = Dictionary::load_prefix(&dir, &l.prefix).unwrap_or_else(|| panic!("{}: no files load", l.prefix));
            assert!(!d.texts.is_empty(), "{}: its sections do not line up with the Russian files", l.prefix);
        }
    }

    #[test]
    fn pairs_by_section_and_key() {
        let ru = Ini::parse("[Unit1]\nName=Мечник\nDescript=Воин\nCost=5\n[Unit2]\nName=Лучник\n[S]\nName=Лечение\n[S]\nName=Молния\n");
        let en = Ini::parse("; comment\n[Unit1]\nName=Swordsman\nDescript=Warrior\n[Unit2]\n[S]\nName=Healing\n[S]\nName=Lightning\n");
        let mut d = Dictionary::default();
        d.add_pairs(&ru, &en);
        assert_eq!(d.english("Мечник"), Some("Swordsman"));
        assert_eq!(d.english(" Воин "), Some("Warrior"));
        assert_eq!(d.english("Лучник"), None, "not translated: stays Russian");
        assert_eq!((d.english("Лечение"), d.english("Молния")), (Some("Healing"), Some("Lightning")), "repeated names pair by place");
        let mut other = Dictionary::default();
        other.add_pairs(&ru, &Ini::parse("[Unit1]\nName=Swordsman\n"));
        assert_eq!(other.english("Мечник"), None, "sections out of line: not used");
    }
}
