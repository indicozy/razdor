//! Cleans the text of a crash report before it leaves the machine: the home folder, the install
//! folder, the saves folder and the save file names become `<home>`, `<dt>`, `<saves>` and `<save>`.
//! The longest names go first, so `<dt>` wins over `<home>` for a folder inside the home folder.
//! Both separators count (`/` and `\`), and ASCII letters match in any case (Windows paths).

use std::path::Path;

/// The names to replace, with what each becomes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Paths {
    /// (needle, replacement), longest needle first, every separator spelling included.
    pairs: Vec<(String, &'static str)>,
}

/// The shortest save name worth replacing: a shorter one would also match ordinary words.
const MIN_SAVE_NAME: usize = 4;

impl Paths {
    /// The names of this machine: the home folder, the install folder (`dt`), the saves folder and
    /// the save files' names (their stems and file names).
    pub fn new(home: Option<&Path>, dt: Option<&Path>, saves: Option<&Path>, save_names: &[String]) -> Paths {
        let mut raw: Vec<(String, &'static str)> = Vec::new();
        if let Some(h) = home {
            raw.push((h.to_string_lossy().into_owned(), "<home>"));
        }
        if let Some(d) = dt {
            raw.push((d.to_string_lossy().into_owned(), "<dt>"));
        }
        if let Some(s) = saves {
            raw.push((s.to_string_lossy().into_owned(), "<saves>"));
        }
        for name in save_names {
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(stem, _)| stem);
            for n in [name.as_str(), stem] {
                if n.chars().count() >= MIN_SAVE_NAME {
                    raw.push((n.to_string(), "<save>"));
                }
            }
        }
        let mut pairs = Vec::new();
        for (needle, repl) in raw {
            let needle = needle.trim_end_matches(['/', '\\']).to_string();
            if needle.is_empty() {
                continue;
            }
            for variant in [needle.replace('\\', "/"), needle.replace('/', "\\"), needle.clone()] {
                if !pairs.iter().any(|(n, _): &(String, &str)| *n == variant) {
                    pairs.push((variant, repl));
                }
            }
        }
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Paths { pairs }
    }

    /// The names of the running game: the home folder, `RAZDOR_DT_DIR` (when set) and the saves
    /// folder given, with the save names in it.
    pub fn for_this_machine(dt: Option<&Path>, saves: Option<&Path>, save_names: &[String]) -> Paths {
        Paths::new(dirs::home_dir().as_deref(), dt, saves, save_names)
    }

    /// `text` with every name replaced.
    pub fn clean(&self, text: &str) -> String {
        let mut out = text.to_string();
        for (needle, repl) in &self.pairs {
            out = replace_ci(&out, needle, repl);
        }
        out
    }
}

/// Replaces every `needle` in `text`, ASCII letters matching in any case. Lower-casing ASCII keeps the
/// byte positions, so the match is cut from the original text.
fn replace_ci(text: &str, needle: &str, repl: &str) -> String {
    let hay = text.to_ascii_lowercase();
    let want = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in hay.match_indices(&want) {
        out.push_str(&text[last..at]);
        out.push_str(repl);
        last = at + want.len();
    }
    out.push_str(&text[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn the_install_and_saves_folders_win_over_the_home_folder() {
        let home = PathBuf::from("/home/alice");
        let dt = PathBuf::from("/home/alice/Games/DT");
        let saves = PathBuf::from("/home/alice/Games/DT/saves");
        let p = Paths::new(Some(&home), Some(&dt), Some(&saves), &["Мой сейв.rzsave".to_string()]);
        let line = "load /home/alice/Games/DT/saves/Мой сейв.rzsave from /home/alice/Games/DT/maps/x.DTm";
        assert_eq!(p.clean(line), "load <saves>/<save> from <dt>/maps/x.DTm");
    }

    #[test]
    fn the_home_folder_is_replaced_and_the_rest_stays() {
        let home = PathBuf::from("/home/alice");
        let p = Paths::new(Some(&home), None, None, &[]);
        assert_eq!(p.clean("open /home/alice/.config/razdor failed, /home/alicex stays"), "open <home>/.config/razdor failed, <home>x stays");
    }

    #[test]
    fn windows_paths_match_in_both_separators_and_any_case() {
        let home = PathBuf::from("C:\\Users\\Alice");
        let dt = PathBuf::from("C:/Games/DT");
        let p = Paths::new(Some(&home), Some(&dt), None, &[]);
        let text = "a c:\\users\\alice\\AppData and C:\\Games\\DT\\x and C:/Users/Alice/y";
        assert_eq!(p.clean(text), "a <home>\\AppData and <dt>\\x and <home>/y");
    }

    #[test]
    fn short_save_names_are_not_replaced() {
        let p = Paths::new(None, None, None, &["a.rzsave".to_string()]);
        assert_eq!(p.clean("a day"), "a day");
    }

    #[test]
    fn a_trailing_separator_on_a_folder_still_matches() {
        let dt = PathBuf::from("/games/DT/");
        let p = Paths::new(None, Some(&dt), None, &[]);
        assert_eq!(p.clean("/games/DT/maps"), "<dt>/maps");
    }
}
