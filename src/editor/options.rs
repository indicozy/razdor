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

    /// Writes the options to Razdor's file in `dir`, keeping its other keys.
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let yn = |b: bool| if b { "Y" } else { "N" }.to_string();
        update(dir, &[("TextFontSize", self.text_size.to_string()), ("TextFontBold", yn(self.bold)), ("NewEventsRepeat", yn(self.new_events_repeat))])
    }
}

/// `text` with the keys `keys` of section `[section]` set: an existing key line is replaced,
/// a missing key added at the end of its section, a missing section added at the end (as the
/// Windows ini writer the original uses does).
pub fn set_keys(text: &str, section: &str, keys: &[(&str, String)]) -> String {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let header = format!("[{section}]");
    let start = match lines.iter().position(|l| l.trim().eq_ignore_ascii_case(&header)) {
        Some(i) => i,
        None => {
            lines.push(header);
            lines.len() - 1
        }
    };
    for (key, value) in keys {
        let end = lines[start + 1..].iter().position(|l| l.trim_start().starts_with('[')).map_or(lines.len(), |i| start + 1 + i);
        let line = format!("{key}={value}");
        match lines[start + 1..end].iter().position(|l| l.split_once('=').is_some_and(|(k, _)| k.trim().eq_ignore_ascii_case(key))) {
            Some(i) => lines[start + 1 + i] = line,
            None => lines.insert(end, line),
        }
    }
    let mut out = lines.join("\r\n");
    out.push_str("\r\n");
    out
}

/// Sets keys of `[Option]` in Razdor's file in `dir`, which is made if it is not there.
fn update(dir: &Path, keys: &[(&str, String)]) -> std::io::Result<PathBuf> {
    update_section(dir, "Option", keys)
}

/// Sets keys of `[section]` in Razdor's file in `dir`, which is made if it is not there.
pub fn update_section(dir: &Path, section: &str, keys: &[(&str, String)]) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(FILE);
    let old = std::fs::read(&path).map(|b| crate::dt::text::decode(&b)).unwrap_or_default();
    super::files::write_atomically(&path, &crate::dt::text::encode(&set_keys(&old, section, keys)))?;
    Ok(path)
}

/// What the original keeps in `[Option]` for itself (main-window.md §1.2 step 13, §1.3): the
/// last map, `WorkMap`, and the building-place check, `FindBuildingPlace`. Razdor keeps them
/// in its own file (never the install's, whose `WorkMap` names the author's own folders).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    pub work_map: Option<PathBuf>,
    pub find_building_place: bool,
}

impl Session {
    /// `WorkMap` when not empty; `FindBuildingPlace` true when its first four letters are
    /// `TRUE` in any case.
    pub fn from_ini(ini: &Ini) -> Session {
        let Some(sec) = ini.section("Option") else { return Session::default() };
        let work_map = sec.get("WorkMap").map(str::trim).filter(|v| !v.is_empty()).map(PathBuf::from);
        let find = sec.get("FindBuildingPlace").is_some_and(|v| v.chars().take(4).collect::<String>().to_uppercase() == "TRUE");
        Session { work_map, find_building_place: find }
    }

    /// Razdor's file in `dir`, else the defaults.
    pub fn load(dir: Option<&Path>) -> Session {
        let read = |p: PathBuf| std::fs::read(p).ok().map(|b| Session::from_ini(&Ini::from_cp1251(&b)));
        dir.and_then(|d| read(d.join(FILE))).unwrap_or_default()
    }

    /// What closing writes (§1.3): `WorkMap` only when there is a map file, and
    /// `FindBuildingPlace` as a boolean word. `PathMap` is never written.
    pub fn save(&self, dir: &Path) -> std::io::Result<PathBuf> {
        let mut keys = Vec::new();
        if let Some(p) = &self.work_map {
            keys.push(("WorkMap", p.display().to_string()));
        }
        keys.push(("FindBuildingPlace", if self.find_building_place { "True" } else { "False" }.to_string()));
        update(dir, &keys)
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
    fn the_session_keys_read_and_write_as_the_original() {
        let s = Session::from_ini(&Ini::parse("[Option]\nWorkMap=C:\\Maps\\a.DTm\nFindBuildingPlace=true!\n"));
        assert_eq!((s.work_map, s.find_building_place), (Some(PathBuf::from("C:\\Maps\\a.DTm")), true));
        assert!(!Session::from_ini(&Ini::parse("[Option]\nFindBuildingPlace=Tru\nWorkMap=\n")).find_building_place);
        let dir = crate::editor::files::tests::temp_dir("session");
        let opts = Options { text_size: 12, bold: true, new_events_repeat: false };
        opts.save(&dir).unwrap();
        Session { work_map: Some(PathBuf::from("/maps/x.DTm")), find_building_place: true }.save(&dir).unwrap();
        // Each writer keeps the other's keys.
        assert_eq!(Options::load(Some(&dir), None), opts);
        let back = Session::load(Some(&dir));
        assert_eq!((back.work_map, back.find_building_place), (Some(PathBuf::from("/maps/x.DTm")), true));
        // No map file: WorkMap stays as it was.
        Session { work_map: None, find_building_place: false }.save(&dir).unwrap();
        assert_eq!(Session::load(Some(&dir)), Session { work_map: Some(PathBuf::from("/maps/x.DTm")), find_building_place: false });
        assert_eq!(set_keys("[MakeMap]\r\nW0=1\r\n[Option]\r\nA=1", "Option", &[("a", "2".into()), ("B", "3".into())]), "[MakeMap]\r\nW0=1\r\n[Option]\r\na=2\r\nB=3\r\n");
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
