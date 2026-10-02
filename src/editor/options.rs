//! The original editor's options window (records.md §11): the text size and boldness of the
//! event window's message and question boxes, and whether new events repeat. The original
//! keeps them in the `[Option]` section of its own ini as `TextFontSize` (8, 10 or 12) and
//! `TextFontBold` / `NewEventsRepeat` (`Y` or `N`). Razdor keeps them in a file of the same
//! form in its own editor folder; until that exists, it reads the install's editor ini
//! (never writing there).

use std::path::{Path, PathBuf};

use crate::dt::ini::Ini;

/// The options file in Razdor's editor folder (and the install's editor ini, read only).
pub const FILE: &str = "DTMapEdit.Ini";
/// Razdor's editor folder: `RAZDOR_EDITOR_DIR`, else `razdor/editor` in the platform data
/// folder. The unit and artefact lists are exported there too.
pub const DIR_ENV: &str = "RAZDOR_EDITOR_DIR";

pub fn editor_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os(DIR_ENV) {
        return Some(PathBuf::from(d));
    }
    dirs::data_dir().map(|d| d.join("razdor").join("editor"))
}

/// The three options.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Text size in points: 8, 10 or 12.
    pub text_size: u8,
    pub bold: bool,
    /// A new event starts as "many times" (its "once" byte is the inverse).
    pub new_events_repeat: bool,
}

/// The sizes the window offers.
pub const SIZES: [u8; 3] = [8, 10, 12];

impl Default for Options {
    /// The shipped ini's values: 10 points, not bold, new events repeat.
    fn default() -> Self {
        Options { text_size: 10, bold: false, new_events_repeat: true }
    }
}

impl Options {
    /// The `[Option]` section as the window reads it (0x55d7b0): a size that is not 8, 10 or 12
    /// shows as 8; a flag is on when its value starts with `Y`. A missing key keeps the
    /// default.
    pub fn from_ini(ini: &Ini) -> Options {
        let mut o = Options::default();
        let Some(sec) = ini.section("Option") else { return o };
        if let Some(v) = sec.get("TextFontSize") {
            let n = v.trim().parse::<u8>().unwrap_or(0);
            o.text_size = if SIZES.contains(&n) { n } else { 8 };
        }
        let yes = |v: &str| v.starts_with('Y');
        if let Some(v) = sec.get("TextFontBold") {
            o.bold = yes(v);
        }
        if let Some(v) = sec.get("NewEventsRepeat") {
            o.new_events_repeat = yes(v);
        }
        o
    }

    /// The section as the window's OK writes it (0x55d984).
    pub fn to_ini_text(&self) -> String {
        let yn = |b: bool| if b { "Y" } else { "N" };
        format!("[Option]\r\nTextFontSize={}\r\nTextFontBold={}\r\nNewEventsRepeat={}\r\n", self.text_size, yn(self.bold), yn(self.new_events_repeat))
    }

    /// Razdor's options file in `dir`, else the install's editor ini in `install`, else the
    /// defaults.
    pub fn load(dir: Option<&Path>, install: Option<&Path>) -> Options {
        let read = |p: PathBuf| std::fs::read(p).ok().map(|b| Options::from_ini(&Ini::from_cp1251(&b)));
        dir.and_then(|d| read(d.join(FILE))).or_else(|| install.and_then(|i| read(i.join(FILE)))).unwrap_or_default()
    }

    /// Writes the options to Razdor's file in `dir`.
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(FILE);
        super::files::write_atomically(&path, &crate::dt::text::encode(&self.to_ini_text()))?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_read_and_write_as_the_original() {
        let o = Options::from_ini(&Ini::parse("[Option]\nWorkMap=x\nNewEventsRepeat=Y\nTextFontSize=12\nTextFontBold=Yes\n"));
        assert_eq!(o, Options { text_size: 12, bold: true, new_events_repeat: true });
        let odd = Options::from_ini(&Ini::parse("[Option]\nTextFontSize=11\nNewEventsRepeat=N\n"));
        assert_eq!((odd.text_size, odd.new_events_repeat, odd.bold), (8, false, false), "an unknown size shows as 8");
        assert_eq!(Options::from_ini(&Ini::parse("")), Options::default());
        let text = Options { text_size: 8, bold: false, new_events_repeat: true }.to_ini_text();
        assert_eq!(text, "[Option]\r\nTextFontSize=8\r\nTextFontBold=N\r\nNewEventsRepeat=Y\r\n");
        assert_eq!(Options::from_ini(&Ini::parse(&text)).text_size, 8);
    }

    #[test]
    fn razdor_writes_its_own_file() {
        let own = crate::editor::files::tests::temp_dir("options-own");
        let install = crate::editor::files::tests::temp_dir("options-install");
        std::fs::write(install.join(FILE), b"[Option]\r\nTextFontSize=12\r\nNewEventsRepeat=N\r\n").unwrap();
        // Read from the install until Razdor has its own; saving never touches the install.
        let o = Options::load(Some(&own), Some(&install));
        assert_eq!((o.text_size, o.new_events_repeat), (12, false));
        let mine = Options { text_size: 10, bold: true, new_events_repeat: true };
        assert_eq!(mine.save(&own).unwrap(), own.join(FILE));
        assert_eq!(Options::load(Some(&own), Some(&install)), mine);
        assert_eq!(std::fs::read(install.join(FILE)).unwrap(), b"[Option]\r\nTextFontSize=12\r\nNewEventsRepeat=N\r\n");
    }
}
