//! Opening and saving maps as the original editor does (DTMapEdit 0x5a6c20 and 0x5a4650,
//! docs/reference/editor/mapcheck-files.md §3–§4): the file variants picked by the name's
//! extension, what the loader repairs and fills in, and every change a save makes.
//!
//! Razdor's own safety stays around it: saves go through [`super::files`] (the user's folder,
//! confirmations for the game's folder, a temporary file) and its integrity check refuses a
//! file that would not read back.

use std::path::{Path, PathBuf};

use crate::dt::container;
use crate::dt::dtm::{CustomArtefact, EditorRead, Scenario, SectionOrder};
use crate::dt::DtError;
use crate::i18n::{n_, tr};
use crate::trf;

use super::grid::{rebuild_objects, MAX_OBJECT_CLASS};
use super::Palette;

/// Delphi's `ChangeFileExt`: the file name's extension (from its last `.`) replaced by
/// `ext`, or `ext` appended when it has none.
pub fn change_ext(path: &Path, ext: &str) -> PathBuf {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem = name.rfind('.').map_or(name.as_str(), |i| &name[..i]);
    path.with_file_name(format!("{stem}{ext}"))
}

/// `ChangeFileExt` on a plain name (the next-map field): `""` becomes `".DTs"`.
fn change_ext_str(name: &str, ext: &str) -> String {
    let cut = name.rfind(['.', '\\', '/', ':']).filter(|i| name.as_bytes()[*i] == b'.');
    format!("{}{ext}", cut.map_or(name, |i| &name[..i]))
}

/// The extension of a path as the original compares it: the text from the last `.` of the
/// file name, case-sensitively.
fn extension(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    name.rfind('.').map(|i| name[i..].to_string()).unwrap_or_default()
}

/// The save dialog's four file types, by position (0x5a0194).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    /// The typed name: a normal `.DTm`.
    Normal,
    /// `.DTD`: the map as `.DTm` and its text dump.
    Dump,
    /// `.DTZ`: the payload without a container, written as `.DTm`.
    Uncompressed,
    /// `.DTS`: a demo map, written as `.DTs`.
    Demo,
}

impl SaveFormat {
    pub const ALL: [SaveFormat; 4] = [SaveFormat::Normal, SaveFormat::Dump, SaveFormat::Uncompressed, SaveFormat::Demo];

    /// The extension the dialog gives the name.
    pub fn extension(self) -> &'static str {
        match self {
            SaveFormat::Normal => ".DTm",
            SaveFormat::Dump => ".DTD",
            SaveFormat::Uncompressed => ".DTZ",
            SaveFormat::Demo => ".DTS",
        }
    }

    pub fn label(self) -> &'static str {
        tr(match self {
            SaveFormat::Normal => n_("Map (.DTm)"),
            SaveFormat::Dump => n_("Map and text dump (.DTD)"),
            SaveFormat::Uncompressed => n_("Uncompressed map (.DTZ)"),
            SaveFormat::Demo => n_("Demo map (.DTS)"),
        })
    }
}

/// The open dialog's three file types, by position (0x59ffdc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenFormat {
    /// The name as typed.
    Normal,
    /// `.DTD`: the map and its text dump.
    Dump,
    /// `.DTS`: a demo map.
    Demo,
}

impl OpenFormat {
    pub const ALL: [OpenFormat; 3] = [OpenFormat::Normal, OpenFormat::Dump, OpenFormat::Demo];

    /// The name the loader is handed for `path`.
    pub fn request(self, path: &Path) -> PathBuf {
        match self {
            OpenFormat::Normal => path.to_path_buf(),
            OpenFormat::Dump => change_ext(path, ".DTD"),
            OpenFormat::Demo => change_ext(path, ".DTS"),
        }
    }

    pub fn label(self) -> &'static str {
        tr(match self {
            OpenFormat::Normal => n_("Map"),
            OpenFormat::Dump => n_("Map and its text dump"),
            OpenFormat::Demo => n_("Demo map (.DTs)"),
        })
    }
}

/// What the loader reads for a name (0x5a6c20), by its extension compared case-sensitively:
/// `.DTD` reads `<name>.DTm` and imports the dump, `.DTS` reads `<name>.DTs`, anything else
/// `<name>.DTm`. So a demo map opened by its own name `x.DTs` reads `x.DTm` (the original's
/// behaviour: only the dialog's demo entry asks for `.DTS`).
pub fn open_target(path: &Path) -> (PathBuf, bool) {
    match extension(path).as_str() {
        ".DTD" => (change_ext(path, ".DTm"), true),
        ".DTS" => (change_ext(path, ".DTs"), false),
        _ => (change_ext(path, ".DTm"), false),
    }
}

/// The container a save writes (0x4d5790).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Container {
    /// `AIpf`, code 19 (bzip2 level 9), no scramble.
    Normal,
    /// No container: the raw payload.
    Raw,
    /// `AIpf`, code 9 (zlib level 9), scramble mode 1, the demo section order.
    Demo,
}

/// What a save writes for the name it is given (0x5a46b4).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveTarget {
    pub map: PathBuf,
    pub container: Container,
    /// Also write the text dump.
    pub dump: bool,
}

/// The save's extension rules, compared case-sensitively: `.DTD` writes `<name>.DTm` and the
/// dump; `.DTZ` an uncompressed `<name>.DTm`; `.DTS` a demo `<name>.DTs`; anything else
/// `<name>.DTm`. So re-saving a map opened as `x.DTs` writes a normal `x.DTm`.
pub fn save_target(path: &Path) -> SaveTarget {
    match extension(path).as_str() {
        ".DTD" => SaveTarget { map: change_ext(path, ".DTm"), container: Container::Normal, dump: true },
        ".DTZ" => SaveTarget { map: change_ext(path, ".DTm"), container: Container::Raw, dump: false },
        ".DTS" => SaveTarget { map: change_ext(path, ".DTs"), container: Container::Demo, dump: false },
        _ => SaveTarget { map: change_ext(path, ".DTm"), container: Container::Normal, dump: false },
    }
}

/// A map as the loader leaves it.
#[derive(Clone, Debug)]
pub struct Loaded {
    pub scenario: Scenario,
    /// The file the bytes came from.
    pub path: PathBuf,
    /// The map's custom artefacts (until a save drops them).
    pub custom_artefacts: Vec<CustomArtefact>,
    /// The signature is not exactly the version-4 one: the original marks the map modified.
    pub modified: bool,
    /// What the loader tells the user (a large map, the dump's fate).
    pub notes: Vec<String>,
}

/// Removes every leading and trailing byte of 32 or below (`SysUtils.Trim`, 0x460f9c).
pub fn trim(s: &str) -> String {
    s.trim_matches(|c: char| c <= ' ').to_string()
}

/// A shortstring field of 64 characters.
fn cut64(s: &str) -> String {
    s.chars().take(64).collect()
}

/// Every string trimmed as the loader reads it (0x5a6a98); the title and the named
/// characters' names cut to 64 characters.
///
/// The loader then looks for the first event with a non-empty string starting with a
/// control character and would cut the event list there (0x5a757b); after the trimming no
/// string can start so, so no map is cut and that step is left out.
pub fn trim_strings(s: &mut Scenario, custom: &mut [CustomArtefact]) {
    s.title = cut64(&trim(&s.title));
    for t in [&mut s.description, &mut s.campaign_name, &mut s.next_map] {
        *t = trim(t);
    }
    for b in &mut s.buildings {
        for t in [&mut b.name, &mut b.owner_name, &mut b.description] {
            *t = trim(t);
        }
    }
    for a in &mut s.armies {
        for t in [&mut a.name, &mut a.leader_name, &mut a.description] {
            *t = trim(t);
        }
    }
    for e in &mut s.events {
        for t in [&mut e.title, &mut e.question, &mut e.message] {
            *t = trim(t);
        }
        e.flags = crate::dt::dtm::FlagScript::from_title(&e.title);
    }
    for c in custom {
        for t in [&mut c.name, &mut c.description] {
            *t = trim(t);
        }
    }
    for n in &mut s.named_characters {
        n.name = cut64(&trim(&n.name));
    }
}

/// What the loader repairs while it builds its cell grid (0x5a8810):
/// - objects of class 13 or more are dropped; a hill or mountain (classes 1–8) of size
///   n = sprite div 10 left of or above column and row n − 1 is moved there; then one object
///   of each slot per cell, the later record winning (see [`super::grid::ObjectGrid`]);
/// - a building beyond the right or bottom edge is moved to the last column or row; one
///   whose cell already holds a building picture moves one step right and down (once,
///   without a second test); its footprint is set from the picture's size, when the
///   install's palette knows the picture; towns and ruins get byte 357 set;
/// - an army's experience correction of 0 becomes 100.
pub fn normalise(s: &mut Scenario, palette: Option<&Palette>) {
    for o in &mut s.objects {
        if (1..=8).contains(&o.class) {
            let n = (o.sprite / 10) as u16;
            if n > 0 {
                o.x = o.x.max(n - 1);
                o.y = o.y.max(n - 1);
            }
        }
    }
    s.objects.retain(|o| o.class <= MAX_OBJECT_CLASS);
    s.objects = rebuild_objects(s);
    let (w, h) = (s.width(), s.height());
    let mut taken = std::collections::HashSet::new();
    for b in &mut s.buildings {
        b.x = b.x.min(w.saturating_sub(1).min(u16::MAX as u32) as u16);
        b.y = b.y.min(h.saturating_sub(1).min(u16::MAX as u32) as u16);
        if taken.contains(&(b.x, b.y)) {
            b.x = b.x.saturating_add(1);
            b.y = b.y.saturating_add(1);
        }
        // An empty picture word (type 0, variant 0) leaves the cell free.
        if (b.picture_type, b.picture_variant) != (0, 0) {
            taken.insert((b.x, b.y));
        }
        // The size table has types 1–15 and variants 0–19; any other picture stops the
        // original with a range error, so its footprint is kept here.
        if let Some(p) = palette.filter(|p| p.from_install && (1..=15).contains(&b.picture_type) && b.picture_variant <= 19) {
            if let Some(pic) = p.picture(b.picture_type, b.picture_variant) {
                (b.size_x, b.size_y) = pic.size;
            }
        }
        if matches!(b.kind, 1 | 12) {
            b.garrison_ai_only = 1;
        }
    }
    for a in &mut s.armies {
        if a.exp_correction == 0 {
            a.exp_correction = 100;
        }
    }
}

/// Reads `path` as the original's loader does: the file by [`open_target`], the editor's
/// stream reader (raw, zlib or bzip2), the payload with its version upgrades, the strings
/// trimmed, the text dump when asked for, and the grid repairs of [`normalise`].
pub fn load(path: &Path, palette: Option<&Palette>, base_artefacts: usize) -> Result<Loaded, DtError> {
    let (file, dump) = open_target(path);
    let bytes = std::fs::read(&file).map_err(|source| DtError::Io { path: file.clone(), source })?;
    let payload = container::decode_editor(&bytes)?;
    let EditorRead { mut scenario, signature_is_current, mut custom_artefacts, .. } = Scenario::parse_editor_payload(&payload)?;
    trim_strings(&mut scenario, &mut custom_artefacts);
    let mut notes = Vec::new();
    if scenario.width() > 200 {
        notes.push(tr("Maps wider than 200 cells may be slow in the game.").to_string());
    }
    if dump {
        let text = super::dump::dump_path(&file, &scenario.title);
        match std::fs::read(&text) {
            Ok(b) => match super::dump::read_dump(&mut scenario, &mut custom_artefacts, base_artefacts, &b) {
                Ok(()) => notes.push(trf!("Texts read from {path}.", path = text.display())),
                Err(super::dump::DumpError::NoHead) => notes.push(trf!("{path} does not start with [Head]; it was not read.", path = text.display())),
                Err(super::dump::DumpError::BadNumber(line) | super::dump::DumpError::ZeroNumber(line)) => {
                    notes.push(trf!("Error in the text file {path} at line {line}; the texts after it were not read.", path = text.display(), line))
                }
            },
            Err(_) => notes.push(trf!("There is no text file {path}.", path = text.display())),
        }
    }
    normalise(&mut scenario, palette);
    Ok(Loaded { scenario, path: file, custom_artefacts, modified: !signature_is_current, notes })
}

/// The changes a save makes to the map in memory, in the original's order (0x5a47c1); they
/// stay after the save:
/// 1. a building of type 0 takes its picture type; a house (8) of picture variant 2–4
///    becomes an obelisk (15), of variant 5–6 ruins (12);
/// 2. every army, in id order, with a home building and active at start makes that
///    building its own: owner, faction and attitudes (a later army on the same home wins);
/// 3. a building whose first goods slot is empty has slots 2–6 moved one place left, once
///    (the original's loop stops at the next empty slot, so a later gap stays); an owner
///    1–254 gives the building its faction and attitudes again;
/// 4. the custom artefacts are dropped (header 0x34 = 0);
/// 5. the objects are written from the grid ([`rebuild_objects`]);
/// 6. the save counter (header 0x124) goes up by one, the demo flag is set for a demo save
///    and cleared otherwise; the title and named characters' names are the editor's
///    64-character fields.
///
/// References past the record lists (a home building or an owner that does not exist) make
/// the original write into or read from unused slots; they are skipped here.
pub fn prepare_save(s: &mut Scenario, demo: bool) {
    for b in &mut s.buildings {
        if b.kind == 0 {
            b.kind = b.picture_type;
        }
        if b.kind == 8 {
            match b.picture_variant {
                2..=4 => b.kind = 15,
                5..=6 => b.kind = 12,
                _ => {}
            }
        }
    }
    for (i, a) in s.armies.iter().enumerate() {
        if a.home_building != 0 && a.inactive == 0 {
            if let Some(b) = s.buildings.get_mut(a.home_building as usize - 1) {
                b.owner_army = (i + 1) as u8;
                b.faction = a.faction;
                b.relations = a.relations;
            }
        }
    }
    for b in &mut s.buildings {
        let slots = &mut b.artifact_slots;
        // The original's loop over the 1-based slots; past slot 64 it would stop with a
        // range error, so it ends there.
        let mut i = 1;
        loop {
            if slots[i - 1] == 0 && i < 6 {
                slots.copy_within(i..6, i - 1);
                slots[5] = 0;
            } else {
                i += 1;
            }
            if i > 64 || slots[i - 1] == 0 {
                break;
            }
        }
        if (1..=254).contains(&b.owner_army) {
            if let Some(a) = s.armies.get(b.owner_army as usize - 1) {
                b.faction = a.faction;
                b.relations = a.relations;
            }
        }
    }
    s.header.unknown_0x34 = 0;
    s.objects = rebuild_objects(s);
    s.header.set_save_counter(s.header.save_counter().wrapping_add(1));
    s.header.set_demo_flag(demo as u8);
    s.title = cut64(&s.title);
    for n in &mut s.named_characters {
        n.name = cut64(&n.name);
    }
}

/// The file bytes of a map already prepared by [`prepare_save`]: in a demo map the next-map
/// name gets the `.DTs` extension (an empty one becomes `.DTs`, as `ChangeFileExt` makes it).
pub fn encode(s: &Scenario, container: Container) -> Vec<u8> {
    match container {
        Container::Normal => container::encode(crate::dt::dtm::CONTAINER_VERSION, &s.to_payload()),
        Container::Raw => s.to_payload(),
        Container::Demo => {
            let mut demo = s.clone();
            demo.next_map = change_ext_str(&s.next_map, ".DTs");
            container::encode_demo(&demo.to_payload_in(SectionOrder::Demo))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dt::dtm::{Army, Building, MapObject};
    use crate::editor::palette::BuildingPicture;

    #[test]
    fn extensions_pick_the_file_case_sensitively() {
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(open_target(&p("/m/a.DTD")), (p("/m/a.DTm"), true));
        assert_eq!(open_target(&p("/m/a.DTS")), (p("/m/a.DTs"), false));
        assert_eq!(open_target(&p("/m/a.DTs")), (p("/m/a.DTm"), false), "the demo map's own name reads the .DTm");
        assert_eq!(open_target(&p("/m/a.dtm")), (p("/m/a.DTm"), false));
        assert_eq!(open_target(&p("/m/a")), (p("/m/a.DTm"), false));
        let t = save_target(&p("/m/a.DTZ"));
        assert_eq!((t.map, t.container, t.dump), (p("/m/a.DTm"), Container::Raw, false));
        let t = save_target(&p("/m/a.DTD"));
        assert_eq!((t.map, t.container, t.dump), (p("/m/a.DTm"), Container::Normal, true));
        let t = save_target(&p("/m/a.DTS"));
        assert_eq!((t.map, t.container), (p("/m/a.DTs"), Container::Demo));
        let t = save_target(&p("/m/a.DTs"));
        assert_eq!((t.map, t.container), (p("/m/a.DTm"), Container::Normal), "re-saving a demo map writes a .DTm");
        assert_eq!(OpenFormat::Demo.request(&p("/m/a.DTs")), p("/m/a.DTS"));
        assert_eq!(change_ext_str("", ".DTs"), ".DTs");
        assert_eq!(change_ext_str("next.DTm", ".DTs"), "next.DTs");
        assert_eq!(change_ext_str("dir.x\\next", ".DTs"), "dir.x\\next.DTs");
    }

    #[test]
    fn strings_are_trimmed_and_cut() {
        let mut s = Scenario { title: format!("  {}\r\n", "т".repeat(70)), description: "\t text \u{1}".into(), ..Scenario::default() };
        s.events.push(crate::dt::dtm::Event { title: " Имя%+Ф ".into(), question: "\r\n".into(), ..Default::default() });
        s.named_characters.push(crate::dt::dtm::NamedCharacter { unit: 1, name: "n".repeat(80) });
        trim_strings(&mut s, &mut []);
        assert_eq!(s.title.chars().count(), 64);
        assert_eq!(s.description, "text");
        assert_eq!((s.events[0].title.as_str(), s.events[0].question.as_str()), ("Имя%+Ф", ""));
        assert_eq!(s.events[0].flags.as_ref().and_then(|f| f.set.as_deref()), Some("Ф"));
        assert_eq!(s.named_characters[0].name.len(), 64);
    }

    fn map(w: u32) -> Scenario {
        let mut s = Scenario::default();
        s.header.width = w;
        s.header.height = w;
        s.terrain = vec![6; (w * w) as usize];
        s
    }

    #[test]
    fn the_loader_repairs_objects_buildings_and_armies() {
        let mut s = map(10);
        s.objects = vec![
            MapObject { x: 0, y: 5, sprite: 31, class: 1 }, // size 3: x raised to 2
            MapObject { x: 4, y: 4, sprite: 5, class: 13 },  // dropped
            MapObject { x: 6, y: 6, sprite: 1, class: 9 },
            MapObject { x: 6, y: 6, sprite: 2, class: 10 }, // replaces the tree
        ];
        let castle = Building { x: 15, y: 3, kind: 3, picture_type: 3, picture_variant: 1, size_x: 9, size_y: 9, ..Building::default() };
        let town = Building { x: 9, y: 3, kind: 1, picture_type: 1, picture_variant: 0, ..Building::default() };
        let ruins = Building { x: 2, y: 2, kind: 12, picture_type: 12, picture_variant: 0, ..Building::default() };
        s.buildings = vec![castle, town, ruins];
        s.armies = vec![Army { exp_correction: 0, ..Army::default() }, Army { exp_correction: 130, ..Army::default() }];
        let palette = Palette { objects: vec![], buildings: vec![BuildingPicture { picture_type: 3, variant: 1, size: (4, 3) }], from_install: true };
        normalise(&mut s, Some(&palette));
        assert_eq!(s.objects, [MapObject { x: 2, y: 5, sprite: 31, class: 1 }, MapObject { x: 6, y: 6, sprite: 2, class: 10 }]);
        // The castle is moved to the last column; the town, now on the same cell, one step
        // right and down (off the map: the original does not test again).
        assert_eq!((s.buildings[0].x, s.buildings[0].y, s.buildings[0].size_x, s.buildings[0].size_y), (9, 3, 4, 3));
        assert_eq!((s.buildings[1].x, s.buildings[1].y), (10, 4));
        assert_eq!((s.buildings[1].size_x, s.buildings[1].size_y), (0, 0), "a picture the palette lacks keeps its size");
        assert_eq!(s.buildings.iter().map(|b| b.garrison_ai_only).collect::<Vec<_>>(), [0, 1, 1]);
        assert_eq!((s.armies[0].exp_correction, s.armies[1].exp_correction), (100, 130));
    }

    #[test]
    fn a_save_derives_types_owners_and_goods() {
        let mut s = map(10);
        let b = |kind: u8, picture_type: u8, variant: u8| Building { kind, picture_type, picture_variant: variant, owner_army: 0xFF, ..Building::default() };
        s.buildings = vec![b(0, 5, 0), b(8, 8, 3), b(8, 8, 6), b(8, 8, 1), b(3, 3, 0)];
        s.buildings[0].artifact_slots[..6].copy_from_slice(&[0, 0, 7, 8, 0, 9]);
        s.buildings[1].artifact_slots[..6].copy_from_slice(&[0, 4, 0, 5, 6, 0]);
        s.buildings[2].artifact_slots[..6].copy_from_slice(&[1, 0, 2, 0, 0, 0]);
        s.buildings[3].artifact_slots[..8].copy_from_slice(&[1, 2, 3, 4, 5, 6, 7, 0]);
        // Army 1 is active with home 5; army 2 is inactive with home 5; army 3 active with
        // home 5 too, and wins. Building 4 is owned by army 1.
        let army = |home: u8, inactive: u8, faction: u8| Army { home_building: home, inactive, faction, relations: [faction as i8; 4], ..Army::default() };
        s.armies = vec![army(5, 0, 1), army(5, 1, 2), army(5, 0, 3)];
        s.buildings[3].owner_army = 1;
        s.header.set_save_counter(41);
        prepare_save(&mut s, false);
        assert_eq!(s.buildings.iter().map(|b| b.kind).collect::<Vec<_>>(), [5, 15, 12, 8, 3]);
        assert_eq!((s.buildings[4].owner_army, s.buildings[4].faction, s.buildings[4].relations), (3, 3, [3; 4]));
        assert_eq!((s.buildings[3].faction, s.buildings[3].relations), (1, [1; 4]));
        // Only an empty first slot closes up, once.
        assert_eq!(&s.buildings[0].artifact_slots[..6], &[0, 7, 8, 0, 9, 0]);
        assert_eq!(&s.buildings[1].artifact_slots[..6], &[4, 0, 5, 6, 0, 0]);
        assert_eq!(&s.buildings[2].artifact_slots[..6], &[1, 0, 2, 0, 0, 0]);
        assert_eq!(&s.buildings[3].artifact_slots[..8], &[1, 2, 3, 4, 5, 6, 7, 0]);
        assert_eq!((s.header.save_counter(), s.header.demo_flag(), s.header.unknown_0x34), (42, 0, 0));
        prepare_save(&mut s, true);
        assert_eq!((s.header.save_counter(), s.header.demo_flag()), (43, 1));
    }

    #[test]
    fn the_three_containers_read_back() {
        let mut s = map(10);
        s.title = "Демо".into();
        prepare_save(&mut s, true);
        let demo = encode(&s, Container::Demo);
        assert_eq!((&demo[..4], demo[6], demo[7]), (&b"AIpf"[..], 9, 1));
        let back = Scenario::parse_editor_payload(&container::decode_editor(&demo).unwrap()).unwrap().scenario;
        assert_eq!(back.next_map, ".DTs");
        assert_eq!((back.title.as_str(), back.header.demo_flag(), back.terrain.len()), ("Демо", 1, 100));
        prepare_save(&mut s, false);
        let raw = encode(&s, Container::Raw);
        assert!(raw.starts_with(b"MapLDV V.4"));
        let normal = encode(&s, Container::Normal);
        assert_eq!((normal[6], normal[7]), (19, 0));
        assert_eq!(Scenario::from_file_bytes(&normal).unwrap(), Scenario::from_file_bytes(&raw).unwrap());
    }
}
