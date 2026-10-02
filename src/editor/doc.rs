//! The document being edited: a [`Scenario`], its file, and the undo history.
//!
//! Undo model: before a command runs, the scenario parts it may change ([`Sections`]) are
//! copied into an undo entry. Undo swaps those copies with the current parts (the swapped-out
//! parts become the redo entry), so undo and redo are exact and cheap. Commands that change
//! nothing leave no entry. A *group* (one brush stroke, one drag) collects many commands
//! into one entry; a *merge key* folds successive edits of the same field (typing a name)
//! into one entry.

use std::path::{Path, PathBuf};

use crate::dt::dtm::{Army, Building, CustomArtefact, Event, MapObject, Point, Scenario};
use crate::dt::DtError;
use crate::i18n::{n_, tr};
use crate::trf;

use super::command::{Command, ObjectFilter, Sections, Settings};
use super::defaults::{new_army, new_building, new_point, new_scenario, NewMap};
use super::geometry::{brush_indices, flood_region, is_massif, rect_indices, CellRect, Footprint};
use super::mapfile::{self, Container};
use super::records;
use super::refs;
use super::validate::{has_errors, self_check, validate, Issue, MAX_RECORDS};
use super::{Names, Palette};

/// Where the document came from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Made with "new map".
    New,
    /// A map of the game's folder (never saved back there by default).
    Game(PathBuf),
    /// Any other file (the user's own maps).
    File(PathBuf),
}

/// Why an edit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    OutOfMap { x: i64, y: i64 },
    /// A building's footprint would reach outside the map.
    FootprintOutside,
    NoSuchBuilding(u16),
    NoSuchArmy(u8),
    NoSuchPoint(u8),
    NoSuchNamedCharacter(u8),
    NoSuchEvent(u16),
    /// Armies have no local events.
    NoEventList,
    /// The record list is full (ids are single bytes).
    Full(&'static str),
    /// Settings may not change the map size.
    Resize,
    /// A record's id must stay its position.
    IdChanged,
    /// A building lists more than 64 events or a point more than 5: the original's event
    /// delete and move stop with a range error there.
    EventListTooLong,
    /// The copy's name has its only `#` at the end: the original's copy stops with a range
    /// error.
    CopyNameEndsInHash,
    /// The event copied is the last row of a filtered list but not the last event: the
    /// original's copy reads past its list and stops with a range error.
    CopyWithoutNextRow,
}

impl std::fmt::Display for EditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EditError::OutOfMap { x, y } => f.write_str(&trf!("({x}, {y}) is outside the map", x, y)),
            EditError::FootprintOutside => f.write_str(tr("the building would reach outside the map")),
            EditError::NoSuchBuilding(id) => f.write_str(&trf!("there is no building {id}", id)),
            EditError::NoSuchArmy(id) => f.write_str(&trf!("there is no army {id}", id)),
            EditError::NoSuchPoint(id) => f.write_str(&trf!("there is no point {id}", id)),
            EditError::NoSuchNamedCharacter(id) => f.write_str(&trf!("there is no named character {id}", id)),
            EditError::NoSuchEvent(id) => f.write_str(&trf!("there is no event {id}", id)),
            EditError::NoEventList => f.write_str(tr("only buildings and points have local events")),
            EditError::Full(what) => f.write_str(&trf!("no room for more {what}", what = tr(what))),
            EditError::Resize => f.write_str(tr("the map size cannot change here")),
            EditError::IdChanged => f.write_str(tr("a record's id is its position and cannot change")),
            EditError::EventListTooLong => f.write_str(tr("a building lists more than 64 events or a point more than 5 (the original stops with a range error)")),
            EditError::CopyNameEndsInHash => f.write_str(tr("the name ends in # (the original's copy stops with a range error)")),
            EditError::CopyWithoutNextRow => f.write_str(tr("the event is the last one the list shows but not the last event (the original's copy stops with a range error)")),
        }
    }
}

/// Why saving failed.
#[derive(Debug)]
pub enum SaveError {
    /// Validation found errors (all issues are listed).
    Invalid(Vec<Issue>),
    Io(std::io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Invalid(issues) => {
                let n = issues.iter().filter(|i| i.severity == super::Severity::Error).count();
                f.write_str(&trf!("the map has errors ({n}); fix them first", n))
            }
            SaveError::Io(e) => f.write_str(&trf!("cannot write the map: {e}", e)),
        }
    }
}

/// What a successful command made.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The id of a placed record.
    pub new_id: Option<u32>,
    /// Whether anything changed.
    pub changed: bool,
}

/// A copy of one part of the scenario.
#[derive(Clone, Debug, PartialEq)]
enum Snap {
    Terrain(Vec<u8>),
    Objects(Vec<MapObject>),
    Buildings(Vec<Building>),
    Armies(Vec<Army>),
    Points(Vec<Point>),
    Events(Vec<Event>),
    Meta(Box<Settings>),
}

impl Snap {
    fn section(&self) -> Sections {
        match self {
            Snap::Terrain(_) => Sections::TERRAIN,
            Snap::Objects(_) => Sections::OBJECTS,
            Snap::Buildings(_) => Sections::BUILDINGS,
            Snap::Armies(_) => Sections::ARMIES,
            Snap::Points(_) => Sections::POINTS,
            Snap::Events(_) => Sections::EVENTS,
            Snap::Meta(_) => Sections::META,
        }
    }
}

#[derive(Clone, Debug)]
struct Entry {
    label: String,
    merge_key: Option<String>,
    /// Unique, for the "saved" marker.
    serial: u64,
    snaps: Vec<Snap>,
}

/// An open map.
pub struct EditorDoc {
    pub scenario: Scenario,
    pub origin: Origin,
    /// The file this document was last saved to (the user's folder or, by explicit action,
    /// the game's), used by "save" without a dialog.
    pub saved_path: Option<PathBuf>,
    /// The map's custom artefacts, as the original's loader appends them to its artefact
    /// list; the next save drops them.
    pub custom_artefacts: Vec<CustomArtefact>,
    /// What opening the map had to say (a large map, the text dump).
    pub load_notes: Vec<String>,
    undo: Vec<Entry>,
    redo: Vec<Entry>,
    group: Option<Entry>,
    next_serial: u64,
    saved_serial: u64,
    /// Bumped by every change, undo and redo (for views that cache what they draw).
    pub revision: u64,
}

/// Events a point can hold in the original editor.
pub const POINT_EVENTS: usize = 5;

/// The original places a building only while fewer than this many exist (0x595390).
pub const MAX_BUILDINGS: usize = 254;
/// Armies the original places at most.
pub const MAX_ARMIES: usize = 255;

/// Undo steps kept.
pub const UNDO_LIMIT: usize = 200;

fn settings_of(s: &Scenario) -> Settings {
    Settings {
        header: s.header.clone(),
        title: s.title.clone(),
        description: s.description.clone(),
        campaign_name: s.campaign_name.clone(),
        next_map: s.next_map.clone(),
        named_characters: s.named_characters.clone(),
        scenario_picture: s.scenario_picture.clone(),
    }
}

fn put_settings(s: &mut Scenario, m: Settings) {
    s.header = m.header;
    s.title = m.title;
    s.description = m.description;
    s.campaign_name = m.campaign_name;
    s.next_map = m.next_map;
    s.named_characters = m.named_characters;
    s.scenario_picture = m.scenario_picture;
}

impl EditorDoc {
    fn with(scenario: Scenario, origin: Origin) -> EditorDoc {
        EditorDoc {
            scenario,
            origin,
            saved_path: None,
            custom_artefacts: Vec::new(),
            load_notes: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            group: None,
            next_serial: 1,
            saved_serial: 0,
            revision: 0,
        }
    }

    /// A new, empty map.
    pub fn new_map(o: NewMap) -> EditorDoc {
        let mut d = EditorDoc::with(new_scenario(o), Origin::New);
        // A new map is unsaved.
        d.saved_serial = u64::MAX;
        d
    }

    /// Opens a map as the original editor does ([`mapfile::load`]), without the install's
    /// palette (footprints stay as stored). `game_dir` is the install's maps folder: a map
    /// from there is marked as a game map so saving goes to the user's folder.
    pub fn open(path: &Path, game_dir: Option<&Path>) -> Result<EditorDoc, DtError> {
        EditorDoc::open_with(path, game_dir, None, 0)
    }

    /// [`EditorDoc::open`] with the install's palette, which gives buildings their
    /// footprints, and the size of its artefact list (for a text dump's artefact blocks).
    pub fn open_with(path: &Path, game_dir: Option<&Path>, palette: Option<&Palette>, base_artefacts: usize) -> Result<EditorDoc, DtError> {
        let loaded = mapfile::load(path, palette, base_artefacts)?;
        let file = loaded.path;
        let from_game = game_dir.is_some_and(|g| super::files::is_inside(&file, g));
        let origin = if from_game { Origin::Game(file.clone()) } else { Origin::File(file.clone()) };
        let mut d = EditorDoc::with(loaded.scenario, origin);
        d.custom_artefacts = loaded.custom_artefacts;
        d.load_notes = loaded.notes;
        if !from_game {
            d.saved_path = Some(file);
        }
        // An old version's signature marks the map modified at once.
        if loaded.modified {
            d.saved_serial = u64::MAX;
        }
        Ok(d)
    }

    /// The name to offer in "save as": the file's stem, else the title.
    pub fn suggested_name(&self) -> String {
        let path = match &self.origin {
            Origin::Game(p) | Origin::File(p) => Some(p),
            Origin::New => None,
        };
        let stem = self.saved_path.as_ref().or(path).and_then(|p| p.file_stem()).map(|s| s.to_string_lossy().into_owned());
        stem.unwrap_or_else(|| if self.scenario.title.trim().is_empty() { tr("New map").into() } else { self.scenario.title.trim().to_string() })
    }

    // ---------------------------------------------------------------------------------
    // Saving
    // ---------------------------------------------------------------------------------

    /// All validation issues.
    pub fn issues(&self, names: Option<&Names>, palette: Option<&Palette>) -> Vec<Issue> {
        validate(&self.scenario, names, palette)
    }

    /// The scenario as a save to a map of `container` leaves it ([`mapfile::prepare_save`]),
    /// if Razdor's integrity check passes it.
    fn prepared(&self, container: Container, names: Option<&Names>, palette: Option<&Palette>) -> Result<Scenario, SaveError> {
        let mut s = self.scenario.clone();
        mapfile::prepare_save(&mut s, container == Container::Demo);
        let issues = validate(&s, names, palette);
        if has_errors(&issues) {
            return Err(SaveError::Invalid(issues));
        }
        self_check(&s).map_err(|_| SaveError::Invalid(issues))?;
        Ok(s)
    }

    /// The bytes a normal save would write (`AIpf` container around the payload), if the map
    /// has no errors. The document is not changed.
    pub fn file_bytes(&self, names: Option<&Names>, palette: Option<&Palette>) -> Result<Vec<u8>, SaveError> {
        Ok(mapfile::encode(&self.prepared(Container::Normal, names, palette)?, Container::Normal))
    }

    /// Saves as the original editor does to the name `path` (chosen by
    /// [`super::files::plan_save`]): the extension picks the file actually written
    /// ([`mapfile::save_target`]), the save's changes stay in the document as one undo step,
    /// and a `.DTD` name also writes the text dump. Returns the map file written.
    pub fn save_to(&mut self, path: &Path, names: Option<&Names>, palette: Option<&Palette>) -> Result<PathBuf, SaveError> {
        let target = mapfile::save_target(path);
        let s = self.prepared(target.container, names, palette)?;
        super::files::write_atomically(&target.map, &mapfile::encode(&s, target.container)).map_err(SaveError::Io)?;
        if target.dump {
            let text = super::dump::dump_path(&target.map, &s.title);
            super::files::write_atomically(&text, &super::dump::write_dump(&s)).map_err(SaveError::Io)?;
        }
        self.commit(n_("Save"), s);
        self.custom_artefacts.clear();
        self.saved_path = Some(target.map.clone());
        self.saved_serial = self.current_serial();
        Ok(target.map)
    }

    /// Scores the map as the original's score button does ([`super::playability::score`])
    /// and stores the score and the quest count in the header (header 0x122 and 0x126), as
    /// one undo step. Saving and loading never score.
    pub fn score_playability(&mut self, names: &Names) -> Result<super::playability::Score, super::playability::ScoreError> {
        let r = super::playability::score(&self.scenario, names, &self.custom_artefacts)?;
        let mut s = self.scenario.clone();
        s.header.set_playability(r.score);
        s.header.set_quest_count(r.quests);
        self.commit(n_("Playability"), s);
        Ok(r)
    }

    /// The original's emergency save after a failed map drawing (0x5a5fa4): the map, as a
    /// normal save makes it, to `ErrorSave.DTm` in `dir`. Razdor's integrity check still
    /// applies.
    pub fn emergency_save(&mut self, dir: &Path, names: Option<&Names>, palette: Option<&Palette>) -> Result<PathBuf, SaveError> {
        self.save_to(&dir.join("ErrorSave.DTm"), names, palette)
    }

    /// Replaces the scenario as one undo step (if anything changed).
    pub(crate) fn commit(&mut self, label: &str, s: Scenario) {
        self.end_group();
        let before = self.snapshot(Sections(127));
        self.scenario = s;
        let before = self.changed(before);
        if !before.is_empty() {
            self.push(Entry { label: label.to_string(), merge_key: None, serial: 0, snaps: before });
            self.revision += 1;
        }
    }

    fn current_serial(&self) -> u64 {
        self.undo.last().map_or(0, |e| e.serial)
    }

    /// Changed since the last save (or never saved).
    pub fn dirty(&self) -> bool {
        self.group.is_some() || self.current_serial() != self.saved_serial
    }

    // ---------------------------------------------------------------------------------
    // Undo
    // ---------------------------------------------------------------------------------

    fn snapshot(&self, sections: Sections) -> Vec<Snap> {
        let s = &self.scenario;
        Sections::ALL
            .into_iter()
            .filter(|x| sections.contains(*x))
            .map(|x| match x {
                Sections::TERRAIN => Snap::Terrain(s.terrain.clone()),
                Sections::OBJECTS => Snap::Objects(s.objects.clone()),
                Sections::BUILDINGS => Snap::Buildings(s.buildings.clone()),
                Sections::ARMIES => Snap::Armies(s.armies.clone()),
                Sections::POINTS => Snap::Points(s.points.clone()),
                Sections::EVENTS => Snap::Events(s.events.clone()),
                _ => Snap::Meta(Box::new(settings_of(s))),
            })
            .collect()
    }

    /// Puts `snap` into the scenario and returns what it replaced.
    fn swap_in(&mut self, snap: Snap) -> Snap {
        let s = &mut self.scenario;
        match snap {
            Snap::Terrain(v) => Snap::Terrain(std::mem::replace(&mut s.terrain, v)),
            Snap::Objects(v) => Snap::Objects(std::mem::replace(&mut s.objects, v)),
            Snap::Buildings(v) => Snap::Buildings(std::mem::replace(&mut s.buildings, v)),
            Snap::Armies(v) => Snap::Armies(std::mem::replace(&mut s.armies, v)),
            Snap::Points(v) => Snap::Points(std::mem::replace(&mut s.points, v)),
            Snap::Events(v) => Snap::Events(std::mem::replace(&mut s.events, v)),
            Snap::Meta(m) => {
                let old = settings_of(s);
                put_settings(s, *m);
                Snap::Meta(Box::new(old))
            }
        }
    }

    /// The snaps that differ from the current scenario.
    fn changed(&self, snaps: Vec<Snap>) -> Vec<Snap> {
        snaps.into_iter().filter(|snap| self.snapshot(snap.section()).first() != Some(snap)).collect()
    }

    fn push(&mut self, mut entry: Entry) {
        entry.serial = self.next_serial;
        self.next_serial += 1;
        self.undo.push(entry);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    /// Label of the step undo would revert.
    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|e| e.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|e| e.label.as_str())
    }

    fn step(&mut self, from_undo: bool) -> bool {
        self.end_group();
        let entry = if from_undo { self.undo.pop() } else { self.redo.pop() };
        let Some(mut entry) = entry else { return false };
        let snaps = std::mem::take(&mut entry.snaps);
        entry.snaps = snaps.into_iter().map(|s| self.swap_in(s)).collect();
        entry.merge_key = None;
        if from_undo {
            self.redo.push(entry);
        } else {
            self.undo.push(entry);
        }
        self.revision += 1;
        true
    }

    pub fn undo(&mut self) -> bool {
        self.step(true)
    }

    pub fn redo(&mut self) -> bool {
        self.step(false)
    }

    /// Starts collecting commands into one undo step (a brush stroke, a drag).
    pub fn begin_group(&mut self, label: &str) {
        self.end_group();
        self.group = Some(Entry { label: label.to_string(), merge_key: None, serial: 0, snaps: Vec::new() });
    }

    /// Ends the group; it becomes one undo step if anything changed.
    pub fn end_group(&mut self) {
        if let Some(mut g) = self.group.take() {
            g.snaps = self.changed(g.snaps);
            if !g.snaps.is_empty() {
                self.push(g);
            }
        }
    }

    pub fn in_group(&self) -> bool {
        self.group.is_some()
    }

    // ---------------------------------------------------------------------------------
    // Commands
    // ---------------------------------------------------------------------------------

    /// Runs a command as its own undo step (or inside the open group).
    pub fn apply(&mut self, cmd: Command) -> Result<Applied, EditError> {
        self.apply_merging(cmd, None)
    }

    /// Runs a command; if the last undo step has the same `merge_key` (and nothing came
    /// between), the two become one step.
    pub fn apply_merging(&mut self, cmd: Command, merge_key: Option<&str>) -> Result<Applied, EditError> {
        let sections = cmd.sections();
        let label = cmd.label();
        // Parts not yet copied into the open group.
        let before = match &self.group {
            Some(g) => {
                let have = g.snaps.iter().fold(Sections(0), |acc, s| acc | s.section());
                self.snapshot(Sections(sections.0 & !have.0))
            }
            None => self.snapshot(sections),
        };
        let mut applied = match self.execute(cmd) {
            Ok(a) => a,
            Err(e) => {
                for s in before {
                    self.swap_in(s);
                }
                return Err(e);
            }
        };
        if let Some(g) = self.group.as_mut() {
            g.snaps.extend(before);
            applied.changed = true;
            self.revision += 1;
            return Ok(applied);
        }
        let before = self.changed(before);
        if before.is_empty() {
            return Ok(Applied { changed: false, ..applied });
        }
        applied.changed = true;
        self.revision += 1;
        let top_matches = merge_key.is_some() && self.redo.is_empty() && self.undo.last().is_some_and(|e| e.merge_key.as_deref() == merge_key);
        if top_matches {
            let top = self.undo.last_mut().expect("checked");
            let have = top.snaps.iter().fold(Sections(0), |acc, s| acc | s.section());
            top.snaps.extend(before.into_iter().filter(|s| !have.contains(s.section())));
            // A merged step is a new state: it is not the saved one any more.
            top.serial = self.next_serial;
            self.next_serial += 1;
            return Ok(applied);
        }
        self.push(Entry { label: label.to_string(), merge_key: merge_key.map(str::to_string), serial: 0, snaps: before });
        Ok(applied)
    }

    fn check_cell(&self, x: i64, y: i64) -> Result<(), EditError> {
        let s = &self.scenario;
        if x < 0 || y < 0 || x >= s.width() as i64 || y >= s.height() as i64 {
            return Err(EditError::OutOfMap { x, y });
        }
        Ok(())
    }

    fn check_footprint(&self, x: u16, y: u16, size: (u8, u8)) -> Result<(), EditError> {
        self.check_cell(x as i64, y as i64)?;
        if !Footprint::of(x as i32, y as i32, size.0, size.1).inside(self.scenario.width(), self.scenario.height()) {
            return Err(EditError::FootprintOutside);
        }
        Ok(())
    }

    fn building_index(&self, id: u16) -> Result<usize, EditError> {
        (id as usize).checked_sub(1).filter(|i| *i < self.scenario.buildings.len()).ok_or(EditError::NoSuchBuilding(id))
    }

    fn army_index(&self, id: u8) -> Result<usize, EditError> {
        (id as usize).checked_sub(1).filter(|i| *i < self.scenario.armies.len()).ok_or(EditError::NoSuchArmy(id))
    }

    fn point_index(&self, id: u8) -> Result<usize, EditError> {
        (id as usize).checked_sub(1).filter(|i| *i < self.scenario.points.len()).ok_or(EditError::NoSuchPoint(id))
    }

    fn event_index(&self, id: u16) -> Result<usize, EditError> {
        (id as usize).checked_sub(1).filter(|i| *i < self.scenario.events.len()).ok_or(EditError::NoSuchEvent(id))
    }

    /// The local list of a building or point: (slots offered, count).
    fn event_list(&mut self, place: Target) -> Result<(&mut [u16], &mut u8), EditError> {
        match place {
            Target::Building(id) => {
                let i = self.building_index(id)?;
                let b = &mut self.scenario.buildings[i];
                Ok((&mut b.event_slots[..], &mut b.event_count))
            }
            Target::Point(id) => {
                let i = self.point_index(id)?;
                let p = &mut self.scenario.points[i];
                // The original editor attaches at most 5 events to a point.
                Ok((&mut p.event_slots[..POINT_EVENTS], &mut p.event_count))
            }
            Target::Army(_) => Err(EditError::NoEventList),
        }
    }

    fn execute(&mut self, cmd: Command) -> Result<Applied, EditError> {
        let (w, h) = (self.scenario.width(), self.scenario.height());
        let mut out = Applied::default();
        match cmd {
            Command::PaintTerrain { x, y, size, code } => {
                for i in brush_indices(w, h, x, y, size) {
                    self.scenario.terrain[i] = code.min(15);
                }
            }
            Command::FillTerrain { x, y, code } => {
                self.check_cell(x as i64, y as i64)?;
                for i in flood_region(&self.scenario.terrain, w, h, x, y) {
                    self.scenario.terrain[i] = code.min(15);
                }
            }
            Command::RectTerrain { from, to, code } => {
                for i in rect_indices(w, h, CellRect::spanning(from, to)) {
                    self.scenario.terrain[i] = code.min(15);
                }
            }
            Command::PlaceObjects { x, y, size, class, sprite } => {
                let Some(r) = CellRect::brush(x, y, size).clip(w, h) else { return Err(EditError::OutOfMap { x: x as i64, y: y as i64 }) };
                for (cx, cy) in r.cells() {
                    let o = MapObject { x: cx as u16, y: cy as u16, sprite, class };
                    if !self.scenario.objects.contains(&o) {
                        insert_object(&mut self.scenario.objects, o);
                    }
                }
            }
            Command::EraseObjects { x, y, size, filter } => {
                let r = CellRect::brush(x, y, size);
                self.scenario.objects.retain(|o| {
                    let hit = r.contains(o.x as i32, o.y as i32);
                    let kind = match filter {
                        ObjectFilter::All => true,
                        ObjectFilter::Massifs => is_massif(o.class),
                        ObjectFilter::Plants => !is_massif(o.class),
                    };
                    !(hit && kind)
                });
            }
            Command::PlaceBuilding { x, y, kind, picture_type, variant, size } => {
                if self.scenario.buildings.len() >= MAX_BUILDINGS {
                    return Err(EditError::Full(n_("buildings (at most 254)")));
                }
                self.check_footprint(x, y, size)?;
                let b = new_building(&self.scenario.header, x, y, kind, picture_type, variant, size);
                self.scenario.buildings.push(b);
                out.new_id = Some(self.scenario.buildings.len() as u32);
            }
            Command::MoveBuilding { id, x, y } => {
                let i = self.building_index(id)?;
                let b = &self.scenario.buildings[i];
                self.check_footprint(x, y, (b.size_x, b.size_y))?;
                let b = &mut self.scenario.buildings[i];
                (b.x, b.y) = (x, y);
            }
            Command::DeleteBuilding { id } => {
                self.building_index(id)?;
                refs::remove_building(&mut self.scenario, id);
            }
            Command::SetBuilding { id, building } => {
                let i = self.building_index(id)?;
                self.scenario.buildings[i] = *building;
            }
            Command::PlaceArmy { x, y, model } => {
                if self.scenario.armies.len() >= MAX_ARMIES {
                    return Err(EditError::Full(n_("armies (at most 255)")));
                }
                self.check_cell(x as i64, y as i64)?;
                let id = self.scenario.armies.len() as u8 + 1;
                let a = new_army(id, x, y, model);
                self.scenario.armies.push(a);
                out.new_id = Some(id as u32);
            }
            Command::MoveArmy { id, x, y } => {
                let i = self.army_index(id)?;
                self.check_cell(x as i64, y as i64)?;
                let a = &mut self.scenario.armies[i];
                (a.x, a.y) = (x, y);
            }
            Command::DeleteArmy { id } => {
                self.army_index(id)?;
                refs::remove_army(&mut self.scenario, id);
            }
            Command::SetArmy { id, army } => {
                let i = self.army_index(id)?;
                if army.id != id {
                    return Err(EditError::IdChanged);
                }
                self.scenario.armies[i] = *army;
            }
            Command::PlacePoint { x, y, lantern } => {
                if self.scenario.points.len() >= MAX_RECORDS {
                    return Err(EditError::Full(n_("points (at most 255)")));
                }
                self.check_cell(x as i64, y as i64)?;
                let id = self.scenario.points.len() as u8 + 1;
                let serial = self.scenario.points.iter().map(|p| p.serial).max().unwrap_or(0).saturating_add(1);
                self.scenario.points.push(new_point(id, serial, x, y, lantern));
                out.new_id = Some(id as u32);
            }
            Command::MovePoint { id, x, y } => {
                let i = self.point_index(id)?;
                self.check_cell(x as i64, y as i64)?;
                let p = &mut self.scenario.points[i];
                (p.x, p.y) = (x, y);
            }
            Command::DeletePoint { id } => {
                self.point_index(id)?;
                refs::remove_point(&mut self.scenario, id);
            }
            Command::SetPoint { id, point } => {
                let i = self.point_index(id)?;
                if point.id != id {
                    return Err(EditError::IdChanged);
                }
                self.scenario.points[i] = *point;
            }
            Command::SetSettings(m) => {
                let hd = &self.scenario.header;
                if (m.header.width, m.header.height) != (hd.width, hd.height) {
                    return Err(EditError::Resize);
                }
                // A new start date moves every event's start with it (records.md §8.4).
                let old = hd.start_time;
                records::shift_event_starts(&mut self.scenario.events, old, m.header.start_time);
                put_settings(&mut self.scenario, *m);
            }
            Command::AddNamedCharacter { unit, name } => {
                if self.scenario.named_characters.len() >= 32 {
                    return Err(EditError::Full(n_("named characters (32)")));
                }
                let k = self.scenario.named_characters.len();
                self.scenario.header.named_character_slots[k] = unit;
                self.scenario.named_characters.push(crate::dt::dtm::NamedCharacter { unit, name });
                out.new_id = Some(k as u32 + 1);
            }
            Command::RemoveNamedCharacter { index } => {
                if !refs::remove_named_character(&mut self.scenario, index) {
                    return Err(EditError::NoSuchNamedCharacter(index));
                }
            }
            Command::NewEvent { kind, repeat } => {
                let e = super::events::new_event(&self.scenario, kind, repeat);
                out.new_id = Some(self.push_event(e)? as u32);
            }
            Command::DuplicateEvent { id, next } => {
                out.new_id = Some(self.copy_event(id, next)? as u32);
            }
            Command::MoveEvent { from, to } => {
                self.event_index(from)?;
                self.event_index(to)?;
                refs::move_event(&mut self.scenario, from, to).map_err(|_| EditError::EventListTooLong)?;
            }
            Command::DeleteEvent { id } => {
                self.event_index(id)?;
                refs::remove_event(&mut self.scenario, id).map_err(|_| EditError::EventListTooLong)?;
            }
            Command::SetEvent { id, mut event } => {
                let i = self.event_index(id)?;
                // The parsed flag script always follows the title.
                event.flags = crate::dt::dtm::FlagScript::from_title(&event.title);
                self.scenario.events[i] = *event;
            }
            Command::AttachEvent { place, event } => {
                self.event_index(event)?;
                let (slots, count) = self.event_list(place)?;
                let used = records::used_events(slots, *count);
                if !used.contains(&event) && !records::add_event(slots, count, event) {
                    return Err(EditError::Full(n_("events in this list")));
                }
            }
            Command::DetachEvent { place, event } => {
                let (slots, count) = self.event_list(place)?;
                while let Some(k) = records::used_events(slots, *count).iter().position(|x| *x == event) {
                    records::remove_event(slots, count, k);
                }
            }
        }
        Ok(out)
    }

    /// The original's copy (0x539214); returns the copy's id.
    fn copy_event(&mut self, id: u16, next: Option<u16>) -> Result<u16, EditError> {
        let i = self.event_index(id)?;
        let name = super::events::split_title(&self.scenario.events[i].title).name;
        let name = super::events::copy_name(&name).map_err(|_| EditError::CopyNameEndsInHash)?;
        let n = self.scenario.events.len() as u16;
        let (at, from) = if id == n {
            (self.push_event(self.scenario.events[i].clone())?, i)
        } else {
            // Appended, then moved to the place of the event on the next row; the record is
            // then taken from the event just before that place, which is the original only
            // when the next row shows the next event (the original's quirk, kept).
            let target = next.ok_or(EditError::CopyWithoutNextRow)?;
            self.event_index(target)?;
            let copy = self.push_event(self.scenario.events[i].clone())?;
            refs::move_event(&mut self.scenario, copy, target).map_err(|_| EditError::EventListTooLong)?;
            (target, target as usize - 2)
        };
        let mut e = self.scenario.events[from].clone();
        e.custom_picture = None;
        e.title = super::events::with_name(&e.title, &name);
        e.flags = crate::dt::dtm::FlagScript::from_title(&e.title);
        self.scenario.events[at as usize - 1] = e;
        Ok(at)
    }

    fn push_event(&mut self, e: Event) -> Result<u16, EditError> {
        if self.scenario.events.len() >= super::events::MAX_EVENTS {
            return Err(EditError::Full(n_("events (at most 5000)")));
        }
        self.scenario.events.push(e);
        Ok(self.scenario.events.len() as u16)
    }

    /// The scenario settings, to edit and apply with [`Command::SetSettings`].
    pub fn settings(&self) -> Settings {
        settings_of(&self.scenario)
    }

    /// What stands on a cell, top first: an army, a point, a building covering it.
    pub fn hit(&self, x: i32, y: i32) -> Option<Target> {
        let s = &self.scenario;
        if let Some(a) = s.armies.iter().rposition(|a| (a.x as i32, a.y as i32) == (x, y)) {
            return Some(Target::Army(a as u8 + 1));
        }
        if let Some(p) = s.points.iter().rposition(|p| (p.x as i32, p.y as i32) == (x, y)) {
            return Some(Target::Point(p as u8 + 1));
        }
        s.buildings
            .iter()
            .rposition(|b| Footprint::of(b.x as i32, b.y as i32, b.size_x, b.size_y).contains(x, y))
            .map(|b| Target::Building(b as u16 + 1))
    }

    /// Objects on a cell, in file order.
    pub fn objects_at(&self, x: u16, y: u16) -> impl Iterator<Item = &MapObject> + '_ {
        self.scenario.objects.iter().filter(move |o| o.x == x && o.y == y)
    }
}

/// A selectable record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Building(u16),
    Army(u8),
    Point(u8),
}

/// Inserts `o` keeping the (y, x) order, after the objects already on its cell.
fn insert_object(objects: &mut Vec<MapObject>, o: MapObject) {
    let at = objects.partition_point(|p| (p.y, p.x) <= (o.y, o.x));
    objects.insert(at, o);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::files::tests::temp_dir;
    use crate::dt::dtm::{ARMY_SIZE, BUILDING_SIZE, EVENT_SIZE, POINT_SIZE};
    use crate::editor::palette::Names;

    fn doc() -> EditorDoc {
        EditorDoc::new_map(NewMap { width: 20, height: 20, fill: 6 })
    }

    /// Places an army and saves its window as the original's save button does (a new army
    /// has no faction until then).
    fn place_army(d: &mut EditorDoc, x: u16, y: u16) {
        let id = d.apply(Command::PlaceArmy { x, y, model: 4 }).unwrap().new_id.unwrap() as u8;
        let a = records::save_army(d.scenario.army(id).unwrap(), None, 2);
        d.apply(Command::SetArmy { id, army: Box::new(a) }).unwrap();
    }

    fn cell(d: &EditorDoc, x: u32, y: u32) -> u8 {
        d.scenario.terrain_at(x, y).unwrap()
    }

    #[test]
    fn paint_undo_redo() {
        let mut d = doc();
        assert!(!d.can_undo());
        let a = d.apply(Command::PaintTerrain { x: 5, y: 5, size: 3, code: 1 }).unwrap();
        assert!(a.changed);
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 1).count(), 9);
        assert_eq!(d.undo_label(), Some("Paint terrain"));
        assert!(d.undo());
        assert!(d.scenario.terrain.iter().all(|c| *c == 6));
        assert!(d.redo());
        assert_eq!(cell(&d, 4, 4), 1);
        assert!(!d.redo());
        // Painting the same again changes nothing and leaves no step.
        let a = d.apply(Command::PaintTerrain { x: 5, y: 5, size: 1, code: 1 }).unwrap();
        assert!(!a.changed);
        assert!(d.undo());
        assert!(!d.can_undo());
    }

    #[test]
    fn a_new_command_clears_redo() {
        let mut d = doc();
        d.apply(Command::PaintTerrain { x: 1, y: 1, size: 1, code: 2 }).unwrap();
        d.undo();
        assert!(d.can_redo());
        d.apply(Command::PaintTerrain { x: 2, y: 2, size: 1, code: 3 }).unwrap();
        assert!(!d.can_redo());
    }

    #[test]
    fn strokes_are_one_step() {
        let mut d = doc();
        d.begin_group("Paint terrain");
        for x in 0..10 {
            d.apply(Command::PaintTerrain { x, y: 0, size: 1, code: 4 }).unwrap();
        }
        d.end_group();
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 4).count(), 10);
        d.undo();
        assert!(d.scenario.terrain.iter().all(|c| *c == 6));
        assert!(!d.can_undo());
        // An empty stroke leaves nothing.
        d.begin_group("nothing");
        d.end_group();
        assert!(!d.can_undo());
    }

    #[test]
    fn fill_and_rectangle() {
        let mut d = doc();
        // A road across the map splits it; filling the top half leaves the bottom.
        d.apply(Command::RectTerrain { from: (0, 10), to: (19, 10), code: 4 }).unwrap();
        d.apply(Command::FillTerrain { x: 0, y: 0, code: 1 }).unwrap();
        assert_eq!((cell(&d, 19, 9), cell(&d, 5, 10), cell(&d, 5, 11)), (1, 4, 6));
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 1).count(), 200);
        assert!(d.apply(Command::FillTerrain { x: 20, y: 0, code: 1 }).is_err());
        // Rectangles clip to the map; corners in any order.
        d.apply(Command::RectTerrain { from: (25, 25), to: (18, 18), code: 10 }).unwrap();
        assert_eq!(d.scenario.terrain.iter().filter(|c| **c == 10).count(), 4);
    }

    #[test]
    fn objects_stack_and_erase() {
        let mut d = doc();
        d.apply(Command::PlaceObjects { x: 3, y: 3, size: 1, class: 9, sprite: 1 }).unwrap();
        d.apply(Command::PlaceObjects { x: 3, y: 3, size: 1, class: 5, sprite: 20 }).unwrap();
        d.apply(Command::PlaceObjects { x: 1, y: 1, size: 3, class: 9, sprite: 2 }).unwrap();
        // The same object twice on a cell is not stacked.
        let again = d.apply(Command::PlaceObjects { x: 3, y: 3, size: 1, class: 9, sprite: 1 }).unwrap();
        assert!(!again.changed);
        assert_eq!(d.objects_at(3, 3).count(), 2);
        assert_eq!(d.scenario.objects.len(), 11);
        // Sorted by (y, x); a cell's stack keeps its order.
        let keys: Vec<(u16, u16)> = d.scenario.objects.iter().map(|o| (o.y, o.x)).collect();
        assert!(keys.windows(2).all(|w| w[0] <= w[1]));
        assert_eq!(d.objects_at(3, 3).map(|o| o.class).collect::<Vec<_>>(), [9, 5]);
        d.apply(Command::EraseObjects { x: 3, y: 3, size: 1, filter: ObjectFilter::Plants }).unwrap();
        assert_eq!(d.objects_at(3, 3).map(|o| o.class).collect::<Vec<_>>(), [5]);
        d.apply(Command::EraseObjects { x: 1, y: 1, size: 9, filter: ObjectFilter::All }).unwrap();
        assert!(d.scenario.objects.is_empty());
        d.undo();
        d.undo();
        assert_eq!(d.objects_at(3, 3).count(), 2);
    }

    #[test]
    fn buildings_place_move_delete() {
        let mut d = doc();
        let a = d.apply(Command::PlaceBuilding { x: 5, y: 5, kind: 3, picture_type: 3, variant: 1, size: (4, 4) }).unwrap();
        assert_eq!(a.new_id, Some(1));
        // Footprints must fit: 4x4 anchored at (2, 5) reaches x = -1.
        assert_eq!(d.apply(Command::PlaceBuilding { x: 2, y: 5, kind: 3, picture_type: 3, variant: 0, size: (4, 4) }), Err(EditError::FootprintOutside));
        assert_eq!(d.apply(Command::PlaceBuilding { x: 30, y: 5, kind: 3, picture_type: 3, variant: 0, size: (1, 1) }), Err(EditError::OutOfMap { x: 30, y: 5 }));
        d.apply(Command::PlaceBuilding { x: 12, y: 12, kind: 2, picture_type: 2, variant: 0, size: (3, 3) }).unwrap();
        d.apply(Command::MoveBuilding { id: 1, x: 8, y: 8 }).unwrap();
        assert_eq!((d.scenario.buildings[0].x, d.scenario.buildings[0].y), (8, 8));
        assert_eq!(d.apply(Command::MoveBuilding { id: 1, x: 1, y: 1 }), Err(EditError::FootprintOutside));
        assert_eq!(d.apply(Command::MoveBuilding { id: 9, x: 8, y: 8 }), Err(EditError::NoSuchBuilding(9)));
        assert_eq!(d.hit(6, 6), Some(Target::Building(1)));
        assert_eq!(d.hit(10, 10), Some(Target::Building(2)));
        assert_eq!(d.hit(0, 0), None);
        // Deleting building 1 renumbers building 2 and remaps the preset's start building.
        let mut st = d.settings();
        st.header.heroes[0].start_building = 2;
        d.apply(Command::SetSettings(Box::new(st))).unwrap();
        d.apply(Command::DeleteBuilding { id: 1 }).unwrap();
        assert_eq!(d.scenario.buildings.len(), 1);
        assert_eq!(d.scenario.header.heroes[0].start_building, 1);
        d.undo();
        assert_eq!(d.scenario.buildings.len(), 2);
        assert_eq!(d.scenario.header.heroes[0].start_building, 2);
    }

    #[test]
    fn the_original_limits() {
        let mut d = EditorDoc::new_map(NewMap { width: 100, height: 100, fill: 6 });
        for k in 0..MAX_BUILDINGS {
            d.scenario.buildings.push(crate::dt::dtm::Building { x: (k % 90) as u16 + 2, y: (k / 90) as u16 + 2, size_x: 1, size_y: 1, ..Default::default() });
        }
        assert_eq!(d.apply(Command::PlaceBuilding { x: 50, y: 50, kind: 3, picture_type: 3, variant: 0, size: (1, 1) }), Err(EditError::Full("buildings (at most 254)")));
        d.scenario.buildings.pop();
        assert!(d.apply(Command::PlaceBuilding { x: 50, y: 50, kind: 3, picture_type: 3, variant: 0, size: (1, 1) }).is_ok());
        for _ in 0..MAX_ARMIES {
            d.apply(Command::PlaceArmy { x: 1, y: 1, model: 4 }).unwrap();
        }
        assert_eq!(d.apply(Command::PlaceArmy { x: 1, y: 1, model: 4 }), Err(EditError::Full("armies (at most 255)")));
    }

    #[test]
    fn armies_and_points() {
        let mut d = doc();
        assert_eq!(d.apply(Command::PlaceArmy { x: 3, y: 4, model: 4 }).unwrap().new_id, Some(1));
        assert_eq!(d.apply(Command::PlaceArmy { x: 5, y: 4, model: 4 }).unwrap().new_id, Some(2));
        d.apply(Command::PlaceBuilding { x: 12, y: 12, kind: 3, picture_type: 3, variant: 0, size: (2, 2) }).unwrap();
        let mut b = d.scenario.buildings[0].clone();
        b.owner_army = 2;
        d.apply(Command::SetBuilding { id: 1, building: Box::new(b) }).unwrap();
        d.apply(Command::MoveArmy { id: 2, x: 6, y: 6 }).unwrap();
        assert_eq!(d.hit(6, 6), Some(Target::Army(2)));
        d.apply(Command::DeleteArmy { id: 1 }).unwrap();
        assert_eq!(d.scenario.armies.len(), 1);
        assert_eq!(d.scenario.armies[0].id, 1);
        assert_eq!(d.scenario.buildings[0].owner_army, 1);
        let mut a = d.scenario.armies[0].clone();
        a.id = 5;
        assert_eq!(d.apply(Command::SetArmy { id: 1, army: Box::new(a) }), Err(EditError::IdChanged));
        assert_eq!(d.apply(Command::PlacePoint { x: 1, y: 1, lantern: true }).unwrap().new_id, Some(1));
        d.apply(Command::PlacePoint { x: 2, y: 1, lantern: false }).unwrap();
        assert_eq!(d.scenario.points.iter().map(|p| (p.id, p.serial, p.model)).collect::<Vec<_>>(), [(1, 1, 8), (2, 2, 9)]);
        d.apply(Command::DeletePoint { id: 1 }).unwrap();
        assert_eq!(d.scenario.points[0].id, 1);
        assert_eq!(d.hit(2, 1), Some(Target::Point(1)));
        assert_eq!(d.apply(Command::MovePoint { id: 3, x: 1, y: 1 }), Err(EditError::NoSuchPoint(3)));
    }

    #[test]
    fn typing_merges_into_one_step() {
        let mut d = doc();
        d.apply(Command::PlaceBuilding { x: 5, y: 5, kind: 5, picture_type: 5, variant: 0, size: (2, 2) }).unwrap();
        for name in ["T", "Ta", "Tav"] {
            let mut b = d.scenario.buildings[0].clone();
            b.name = name.into();
            d.apply_merging(Command::SetBuilding { id: 1, building: Box::new(b) }, Some("b1:name")).unwrap();
        }
        let mut b = d.scenario.buildings[0].clone();
        b.gold_per_day = 10;
        d.apply_merging(Command::SetBuilding { id: 1, building: Box::new(b) }, Some("b1:gold")).unwrap();
        assert_eq!(d.scenario.buildings[0].name, "Tav");
        d.undo();
        assert_eq!((d.scenario.buildings[0].name.as_str(), d.scenario.buildings[0].gold_per_day), ("Tav", 0));
        d.undo();
        assert_eq!(d.scenario.buildings[0].name, "");
        assert_eq!(d.undo_label(), Some("Place building"));
    }

    #[test]
    fn settings_and_named_characters() {
        let mut d = doc();
        let mut st = d.settings();
        st.title = "Поход".into();
        st.header.victory_event = 0;
        d.apply(Command::SetSettings(Box::new(st.clone()))).unwrap();
        assert_eq!(d.scenario.title, "Поход");
        st.header.width = 30;
        assert_eq!(d.apply(Command::SetSettings(Box::new(st))), Err(EditError::Resize));
        d.apply(Command::AddNamedCharacter { unit: 7, name: "A".into() }).unwrap();
        d.apply(Command::AddNamedCharacter { unit: 8, name: "B".into() }).unwrap();
        d.apply(Command::PlaceArmy { x: 1, y: 1, model: 4 }).unwrap();
        let mut a = d.scenario.armies[0].clone();
        a.named_character = 2;
        d.apply(Command::SetArmy { id: 1, army: Box::new(a) }).unwrap();
        d.apply(Command::RemoveNamedCharacter { index: 1 }).unwrap();
        assert_eq!(d.scenario.armies[0].named_character, 2, "as the original: not renumbered");
        assert_eq!(d.scenario.named_characters[0].name, "B");
        assert_eq!(d.apply(Command::RemoveNamedCharacter { index: 5 }), Err(EditError::NoSuchNamedCharacter(5)));
    }

    #[test]
    fn a_new_start_date_moves_the_events() {
        let mut d = doc();
        d.apply(Command::NewEvent { kind: 1, repeat: false }).unwrap();
        d.apply(Command::NewEvent { kind: 1, repeat: false }).unwrap();
        let mut e = d.scenario.events[1].clone();
        crate::editor::events::set_relative(&mut e, true, 0);
        d.apply(Command::SetEvent { id: 2, event: Box::new(e) }).unwrap();
        let start = d.scenario.header.start_time;
        let mut st = d.settings();
        st.header.start_time += 1440;
        d.apply(Command::SetSettings(Box::new(st))).unwrap();
        assert_eq!((d.scenario.events[0].start_time, d.scenario.events[1].start_time), (start + 1440, crate::dt::dtm::RELATIVE_START));
        d.undo();
        assert_eq!(d.scenario.events[0].start_time, start, "one undo step");
    }

    #[test]
    fn dirty_tracking() {
        let dir = temp_dir("dirty");
        let mut d = doc();
        assert!(d.dirty(), "a new map is unsaved");
        let path = dir.join("m.DTm");
        d.save_to(&path, None, None).unwrap();
        assert!(!d.dirty());
        d.apply(Command::PaintTerrain { x: 0, y: 0, size: 1, code: 3 }).unwrap();
        assert!(d.dirty());
        d.undo();
        assert!(!d.dirty(), "back at the saved state");
        d.redo();
        assert!(d.dirty());
    }

    #[test]
    fn save_open_roundtrip() {
        let dir = temp_dir("roundtrip");
        let mut d = doc();
        d.apply(Command::PaintTerrain { x: 5, y: 5, size: 5, code: 12 }).unwrap();
        d.apply(Command::PlaceObjects { x: 5, y: 5, size: 1, class: 5, sprite: 20 }).unwrap();
        d.apply(Command::PlaceBuilding { x: 10, y: 10, kind: 3, picture_type: 3, variant: 0, size: (4, 4) }).unwrap();
        place_army(&mut d, 15, 15);
        let mut a = d.scenario.armies[0].clone();
        (a.leader_unit, a.name) = (1, "Отряд".into());
        d.apply(Command::SetArmy { id: 1, army: Box::new(a) }).unwrap();
        d.apply(Command::PlacePoint { x: 2, y: 2, lantern: true }).unwrap();
        let path = dir.join("Новая.DTm");
        d.save_to(&path, None, None).unwrap();
        let back = EditorDoc::open(&path, None).unwrap();
        assert_eq!(back.scenario, d.scenario);
        assert_eq!(back.origin, Origin::File(path.clone()));
        assert_eq!(back.saved_path.as_deref(), Some(path.as_path()));
        assert!(std::fs::read(&path).unwrap().starts_with(crate::dt::container::MAGIC));
        // The game reads the same bytes back.
        let s = Scenario::load(&path).unwrap();
        assert_eq!(s.armies[0].name, "Отряд");
    }

    #[test]
    fn saves_follow_the_extension_rules() {
        let dir = temp_dir("variants");
        let mut d = doc();
        place_army(&mut d, 3, 3);
        // Uncompressed: the raw payload under the normal name.
        let written = d.save_to(&dir.join("m.DTZ"), None, None).unwrap();
        assert_eq!(written, dir.join("m.DTm"));
        let raw = std::fs::read(&written).unwrap();
        assert!(raw.starts_with(b"MapLDV V.4"));
        assert_eq!(Scenario::from_file_bytes(&raw).unwrap().header.save_counter(), 1);
        assert_eq!(d.saved_path.as_deref(), Some(written.as_path()));
        // Demo: zlib, scramble 1, the demo order; it opens as a demo map and saves back as
        // a normal one.
        let demo = d.save_to(&dir.join("m.DTS"), None, None).unwrap();
        assert_eq!(demo, dir.join("m.DTs"));
        let bytes = std::fs::read(&demo).unwrap();
        assert_eq!((bytes[6], bytes[7]), (9, 1));
        assert_eq!(d.scenario.header.demo_flag(), 1, "the flag stays in memory");
        let back = EditorDoc::open(&dir.join("m.DTS"), None).unwrap();
        assert_eq!((back.scenario.armies.len(), back.scenario.header.save_counter()), (1, 2));
        assert_eq!(back.saved_path.as_deref(), Some(demo.as_path()));
        let mut back = back;
        assert_eq!(back.save_to(&demo, None, None).unwrap(), dir.join("m.DTm"));
        assert_eq!(back.scenario.header.demo_flag(), 0);
        // Dump: the map and its text file.
        let mut st = d.settings();
        st.title = "Trip".into();
        d.apply(Command::SetSettings(Box::new(st))).unwrap();
        assert_eq!(d.save_to(&dir.join("t.DTD"), None, None).unwrap(), dir.join("t.DTm"));
        let text = std::fs::read(dir.join("t.Eng")).unwrap();
        assert!(text.starts_with(b"[Head]\r\nTrip\r\n"));
        // Opening the .DTD name reads the map and the texts.
        std::fs::write(dir.join("t.Eng"), b"[Head]\r\nTrip\r\nNew description\r\n").unwrap();
        let t = EditorDoc::open(&dir.join("t.DTD"), None).unwrap();
        assert_eq!(t.scenario.description, "New description");
        assert!(t.load_notes.iter().any(|n| n.contains("t.Eng")));
        // A save is one undo step with the save's changes.
        assert_eq!(d.undo_label(), Some("Save"));
        assert!(!d.dirty());
    }

    #[test]
    fn the_emergency_save_writes_error_save() {
        let dir = temp_dir("emergency");
        let mut d = doc();
        place_army(&mut d, 2, 2);
        let path = d.emergency_save(&dir, None, None).unwrap();
        assert_eq!(path, dir.join("ErrorSave.DTm"));
        assert_eq!(Scenario::load(&path).unwrap().armies.len(), 1);
    }

    #[test]
    fn old_versions_open_modified() {
        let dir = temp_dir("old");
        let mut d = doc();
        d.apply(Command::PlaceArmy { x: 3, y: 3, model: 4 }).unwrap();
        let mut p = d.scenario.to_payload();
        p[9] = b'3';
        let path = dir.join("old.DTm");
        std::fs::write(&path, &p).unwrap();
        let old = EditorDoc::open(&path, None).unwrap();
        assert!(old.dirty(), "an old signature marks the map modified");
        assert_eq!(old.scenario.armies[0].garrison_strength, if d.scenario.armies[0].garrison_strength == 0 { 50 } else { 0 });
        assert!(Scenario::load(&path).is_err(), "the game does not load it");
    }

    #[test]
    fn invalid_maps_are_not_saved() {
        let dir = temp_dir("invalid");
        let mut d = doc();
        d.scenario.header.victory_event = 3;
        let path = dir.join("bad.DTm");
        match d.save_to(&path, None, None) {
            Err(SaveError::Invalid(issues)) => assert!(issues.iter().any(|i| i.message.contains("victory event"))),
            other => panic!("{other:?}"),
        }
        assert!(!path.exists());
        // Content checks when names are given.
        let names = Names::from_content(&crate::rules::content::Content::builtin());
        d.scenario.header.victory_event = 0;
        d.scenario.header.heroes[2].artifacts[0] = 250;
        assert!(d.file_bytes(Some(&names), None).is_err());
        assert!(d.file_bytes(None, None).is_ok());
    }

    #[test]
    fn opening_marks_game_maps() {
        let game = temp_dir("gamefolder");
        let mut d = doc();
        let p = game.join("Shipped.DTm");
        d.save_to(&p, None, None).unwrap();
        let g = EditorDoc::open(&p, Some(&game)).unwrap();
        assert_eq!(g.origin, Origin::Game(p.clone()));
        assert_eq!(g.saved_path, None, "a game map has no save path of its own");
        assert_eq!(g.suggested_name(), "Shipped");
        assert_eq!(doc().suggested_name(), "New scenario");
    }

    /// Offsets of the sections in a payload: (buildings, armies, points).
    fn section_offsets(p: &[u8]) -> (usize, usize, usize) {
        let u32_at = |o: usize| u32::from_le_bytes(p[o..o + 4].try_into().unwrap()) as usize;
        let b = crate::dt::dtm::HEADER_SIZE + u32_at(0x1C) + u32_at(0x20);
        let a = b + u32_at(0x24);
        (b, a, a + u32_at(0x28))
    }

    #[test]
    fn property_edits_land_at_documented_offsets() {
        let mut d = doc();
        d.apply(Command::PlaceBuilding { x: 10, y: 10, kind: 3, picture_type: 3, variant: 2, size: (4, 4) }).unwrap();
        d.apply(Command::PlaceBuilding { x: 16, y: 16, kind: 2, picture_type: 2, variant: 0, size: (3, 3) }).unwrap();
        d.apply(Command::PlaceArmy { x: 3, y: 4, model: 4 }).unwrap();
        d.apply(Command::PlacePoint { x: 7, y: 8, lantern: true }).unwrap();
        let mut b = d.scenario.buildings[1].clone();
        b.gold_per_day = 0x1234;
        b.gold_max = 700;
        b.owner_army = 1;
        b.linked_building = 1;
        b.garrison[1] = crate::dt::dtm::Troop { unit: 12, level: 2, count: 5 };
        b.barracks[0] = crate::dt::dtm::RecruitSlot { unit: 6, start_count: 3, max_count: 9 };
        b.garrison_extra_defence = 15;
        b.price_min = 25;
        b.price_max = 1500;
        b.faction = 2;
        b.relations = [3, -1, 0, -3];
        b.mana_per_day = 40;
        b.mana_max = 200;
        b.start_for = [0, 1, 0];
        b.recruit_all_types = 1;
        b.garrison_ai_only = 1;
        b.random_artifacts_for_sale = 6;
        b.spells_for_sale = [4, 9, 0, 0, 0, 0];
        b.artifact_slots[0] = 146;
        b.event_count = 1;
        b.event_slots[0] = 0x0102;
        d.apply(Command::SetBuilding { id: 2, building: Box::new(b) }).unwrap();
        let mut a = d.scenario.armies[0].clone();
        a.speed_correction = -2;
        a.leader_unit = 5;
        a.leader_level = 3;
        a.troops[0] = crate::dt::dtm::Troop { unit: 7, level: 1, count: 4 };
        a.home_building = 2;
        a.artifacts = [11, 0, 12];
        a.named_character = 0;
        a.behaviour = 1;
        a.patrols = 1;
        a.patrol_radius = 12;
        a.no_money = 1;
        a.inactive = 1;
        a.faction = 3;
        a.relations = [1, -2, 3, 0];
        a.aggression = -25;
        a.respawn_days = 4;
        a.exp_correction = 130;
        a.ship = 2;
        a.ignored_by_ai = 1;
        a.hunts_player_only = 1;
        a.no_random_targets = 1;
        a.no_socialising = 1;
        a.no_building_interest = 1;
        a.garrison_strength = 60;
        a.respawn_all = 1;
        a.spell = 3;
        a.target_model = 4;
        a.gold_income = 300;
        a.hire_bonus_exp = 50;
        a.exp_like_player = 1;
        d.apply(Command::SetArmy { id: 1, army: Box::new(a) }).unwrap();
        let mut p = d.scenario.points[0].clone();
        p.radius = 9;
        p.active = 0;
        p.event_count = 2;
        p.event_slots[..2].copy_from_slice(&[3, 0x0405]);
        d.apply(Command::SetPoint { id: 1, point: Box::new(p) }).unwrap();

        let pay = d.scenario.to_payload();
        let (bo, ao, po) = section_offsets(&pay);
        let b = &pay[bo + BUILDING_SIZE..bo + 2 * BUILDING_SIZE];
        assert_eq!(&b[0..7], &[16, 0, 16, 0, 0, 2, 2]);
        assert_eq!(&b[8..10], &[0x02, 0x01]);
        assert_eq!(&b[136..138], &146u16.to_le_bytes());
        assert_eq!(&b[264..267], &[6, 3, 9]);
        assert_eq!(&b[282..286], &[0x34, 0x12, 0xBC, 0x02]);
        assert_eq!(&b[288..291], &[1, 3, 3]);
        assert_eq!(&b[292..296], &[1, 1, 0, 6]);
        assert_eq!(&b[308..310], &[4, 9]);
        assert_eq!(&b[317..320], &[12, 2, 5]);
        assert_eq!(b[332], 15);
        assert_eq!(&b[333..337], &[25, 0, 0xDC, 0x05]);
        assert_eq!(&b[337..342], &[2, 3, 0xFF, 0, 0xFD]);
        assert_eq!(&b[350..352], &[40, 200]);
        assert_eq!(&b[353..358], &[0, 1, 0, 1, 1]);
        let a = &pay[ao..ao + ARMY_SIZE];
        assert_eq!(&a[0..6], &[3, 0, 4, 0, 1, 4]);
        assert_eq!(a[13], 0xFE);
        assert_eq!(a[14], 1);
        assert_eq!(&a[17..21], &[0x2C, 0x01, 50, 0]);
        assert_eq!(&a[25..31], &[2, 5, 3, 7, 1, 4]);
        assert_eq!(&a[50..53], &[11, 0, 12]);
        assert_eq!(&a[58..72], &[0, 1, 1, 12, 1, 1, 3, 1, 0xFE, 3, 0, 0xE7, 4, 130]);
        assert_eq!(a[72], 2);
        assert_eq!(&a[76..80], &[1, 1, 1, 1]);
        assert_eq!(&a[81..86], &[1, 60, 1, 3, 4]);
        let p = &pay[po..po + POINT_SIZE];
        assert_eq!(&p[0..6], &[7, 0, 8, 0, 1, 8]);
        assert_eq!(&p[8..12], &[3, 0, 0x05, 0x04]);
        assert_eq!(&p[38..41], &[9, 2, 0]);
        // And the file reads back to the same records.
        let back = Scenario::parse_payload(&pay).unwrap();
        assert_eq!((back.buildings, back.armies, back.points), (d.scenario.buildings.clone(), d.scenario.armies.clone(), d.scenario.points.clone()));
    }

    #[test]
    fn settings_land_in_the_header() {
        let mut d = doc();
        let mut st = d.settings();
        st.header.heroes[2].gold = 1234;
        st.header.heroes[2].x = 17;
        st.header.heroes[2].start_building = 0;
        st.header.heroes[2].troops[0] = crate::dt::dtm::Troop { unit: 3, level: 1, count: 2 };
        st.header.heroes[2].artifacts = [5, 6, 7];
        st.header.heroes[2].spells[0] = 8;
        st.header.relations[3] = [-3, -2, -1, 0];
        st.header.victory_event = 0;
        st.header.scenario_kind = 1;
        st.header.carry_over = [1, 0, 1, 0, 1, 0, 1];
        st.campaign_name = "Кампания".into();
        st.next_map = "next.DTm".into();
        d.apply(Command::SetSettings(Box::new(st))).unwrap();
        d.apply(Command::AddNamedCharacter { unit: 42, name: "Имя".into() }).unwrap();
        let pay = d.scenario.to_payload();
        let h = 0x3C + 2 * 50;
        assert_eq!(&pay[h + 8..h + 12], &1234u32.to_le_bytes());
        assert_eq!(&pay[h + 19..h + 22], &[3, 1, 2]);
        assert_eq!(&pay[h + 37..h + 39], &[17, 0]);
        assert_eq!(&pay[h + 41..h + 45], &[5, 6, 7, 8]);
        assert_eq!(&pay[0xDE + 12..0xDE + 16], &[0xFD, 0xFE, 0xFF, 0]);
        assert_eq!((pay[0xEE], pay[0xEF], pay[0x10F]), (1, 42, 1));
        assert_eq!(&pay[0x110..0x117], &[1, 0, 1, 0, 1, 0, 1]);
        let back = Scenario::parse_payload(&pay).unwrap();
        assert_eq!((back.campaign_name.as_str(), back.next_map.as_str(), back.named_characters[0].name.as_str()), ("Кампания", "next.DTm", "Имя"));
    }

    fn events_offset(p: &[u8]) -> usize {
        let (_, _, po) = section_offsets(p);
        po + u32::from_le_bytes(p[0x2C..0x30].try_into().unwrap()) as usize
    }

    #[test]
    fn events_new_duplicate_delete_undo() {
        let mut d = doc();
        assert_eq!(d.apply(Command::NewEvent { kind: 1, repeat: false }).unwrap().new_id, Some(1));
        assert_eq!(d.apply(Command::NewEvent { kind: 3, repeat: false }).unwrap().new_id, Some(2));
        let mut e = d.scenario.events[1].clone();
        e.title = "Quest%+Q".into();
        e.custom_picture = Some(vec![1, 0, 1, 0, 9, 9]);
        d.apply(Command::SetEvent { id: 2, event: Box::new(e) }).unwrap();
        assert_eq!(d.scenario.events[1].flags.as_ref().and_then(|f| f.set.as_deref()), Some("Q"), "flags follow the title");
        assert_eq!(d.apply(Command::DuplicateEvent { id: 2, next: None }).unwrap().new_id, Some(3));
        // The copy is numbered and has no picture of its own.
        let (a, b) = (&d.scenario.events[1], &d.scenario.events[2]);
        assert_eq!((b.title.as_str(), b.custom_picture.as_ref(), b.flags.as_ref().and_then(|f| f.set.as_deref())), ("Quest #1%+Q", None, Some("Q")));
        assert_eq!(Event { title: a.title.clone(), custom_picture: None, flags: a.flags.clone(), ..b.clone() }, Event { custom_picture: None, ..a.clone() });
        let mut e = d.scenario.events[2].clone();
        e.title = "Quest%+Q".into();
        d.apply(Command::SetEvent { id: 3, event: Box::new(e) }).unwrap();
        assert_eq!(d.apply(Command::DuplicateEvent { id: 7, next: None }), Err(EditError::NoSuchEvent(7)));
        // Event 1 completes quest 3; building 1 and point 1 list event 3; it is the victory.
        let mut e1 = d.scenario.events[0].clone();
        e1.results.completes_quest = 3;
        e1.conditions.not_happened = [2, 3];
        d.apply(Command::SetEvent { id: 1, event: Box::new(e1) }).unwrap();
        d.apply(Command::PlaceBuilding { x: 5, y: 5, kind: 3, picture_type: 3, variant: 0, size: (2, 2) }).unwrap();
        d.apply(Command::PlacePoint { x: 9, y: 9, lantern: false }).unwrap();
        d.apply(Command::AttachEvent { place: Target::Building(1), event: 3 }).unwrap();
        d.apply(Command::AttachEvent { place: Target::Point(1), event: 2 }).unwrap();
        d.apply(Command::AttachEvent { place: Target::Point(1), event: 3 }).unwrap();
        let mut st = d.settings();
        st.header.victory_event = 3;
        st.header.defeat_event = 1;
        d.apply(Command::SetSettings(Box::new(st))).unwrap();
        let before = d.scenario.clone();
        d.apply(Command::DeleteEvent { id: 2 }).unwrap();
        let s = &d.scenario;
        assert_eq!(s.events.len(), 2);
        assert_eq!((s.events[0].results.completes_quest, s.events[0].conditions.not_happened), (2, [0, 2]));
        assert_eq!((s.buildings[0].event_count, s.buildings[0].event_slots[0]), (1, 2));
        // The point's entry is zeroed in place and the count drops: as the original.
        assert_eq!((s.points[0].event_count, &s.points[0].event_slots[..2]), (1, &[0, 2][..]));
        assert_eq!((s.header.victory_event, s.header.defeat_event), (2, 1));
        assert_eq!(d.undo_label(), Some("Delete event"));
        d.undo();
        assert_eq!(d.scenario, before, "undo restores events, lists and the header");
        d.redo();
        assert_eq!(d.scenario.events.len(), 2);
        assert_eq!(d.apply(Command::DeleteEvent { id: 9 }), Err(EditError::NoSuchEvent(9)));
    }

    #[test]
    fn copies_go_after_the_original() {
        let mut d = doc();
        for k in 0..4 {
            d.apply(Command::NewEvent { kind: 1, repeat: true }).unwrap();
            let mut e = d.scenario.events[k].clone();
            e.group_colour = k as u8;
            d.apply(Command::SetEvent { id: k as u16 + 1, event: Box::new(e) }).unwrap();
        }
        assert_eq!(d.scenario.events[0].title, "Event 1");
        // Event 4 refers to event 3; the victory is event 4.
        let mut e = d.scenario.events[3].clone();
        e.results.chained_event = 3;
        d.apply(Command::SetEvent { id: 4, event: Box::new(e) }).unwrap();
        let mut st = d.settings();
        st.header.victory_event = 4;
        d.apply(Command::SetSettings(Box::new(st))).unwrap();
        // Copying event 2 with the whole list shown: the copy is event 3, later ones move.
        assert_eq!(d.apply(Command::DuplicateEvent { id: 2, next: Some(3) }).unwrap().new_id, Some(3));
        let groups: Vec<u8> = d.scenario.events.iter().map(|e| e.group_colour).collect();
        assert_eq!(groups, [0, 1, 1, 2, 3]);
        assert_eq!((d.scenario.events[2].title.as_str(), d.scenario.events[4].results.chained_event, d.scenario.header.victory_event), ("Event 2 #1", 4, 5));
        // With a filter hiding events 2 and 3, the next row after event 1 is event 4: the copy
        // takes the place of event 4 and the record of event 3, under event 1's new name.
        assert_eq!(d.apply(Command::DuplicateEvent { id: 1, next: Some(4) }).unwrap().new_id, Some(4));
        let c = &d.scenario.events[3];
        assert_eq!((c.title.as_str(), c.group_colour), ("Event 1 #1", 1));
        // The last row of a filtered list that is not the last event: the original stops.
        assert_eq!(d.apply(Command::DuplicateEvent { id: 2, next: None }), Err(EditError::CopyWithoutNextRow));
        let mut e = d.scenario.events[0].clone();
        e.title = "Сон #".into();
        d.apply(Command::SetEvent { id: 1, event: Box::new(e) }).unwrap();
        assert_eq!(d.apply(Command::DuplicateEvent { id: 1, next: Some(2) }), Err(EditError::CopyNameEndsInHash));
        assert_eq!(d.scenario.events.len(), 6, "a refused copy leaves nothing");
        // Moving and its undo.
        let before = d.scenario.clone();
        d.apply(Command::MoveEvent { from: 6, to: 1 }).unwrap();
        assert_eq!(d.scenario.header.victory_event, 1);
        d.undo();
        assert_eq!(d.scenario, before);
    }

    #[test]
    fn attaching_and_detaching_local_events() {
        let mut d = doc();
        for _ in 0..7 {
            d.apply(Command::NewEvent { kind: 2, repeat: false }).unwrap();
        }
        d.apply(Command::PlacePoint { x: 1, y: 1, lantern: false }).unwrap();
        d.apply(Command::PlaceBuilding { x: 5, y: 5, kind: 3, picture_type: 3, variant: 0, size: (2, 2) }).unwrap();
        d.apply(Command::PlaceArmy { x: 8, y: 8, model: 4 }).unwrap();
        for id in 1..=5 {
            d.apply(Command::AttachEvent { place: Target::Point(1), event: id }).unwrap();
        }
        // A point holds five, as in the original editor; the same event is listed once.
        assert_eq!(d.apply(Command::AttachEvent { place: Target::Point(1), event: 6 }), Err(EditError::Full("events in this list")));
        assert!(!d.apply(Command::AttachEvent { place: Target::Point(1), event: 3 }).unwrap().changed);
        assert_eq!(d.apply(Command::AttachEvent { place: Target::Point(1), event: 8 }), Err(EditError::NoSuchEvent(8)));
        assert_eq!(d.apply(Command::AttachEvent { place: Target::Army(1), event: 1 }), Err(EditError::NoEventList));
        assert_eq!(d.apply(Command::AttachEvent { place: Target::Building(4), event: 1 }), Err(EditError::NoSuchBuilding(4)));
        d.apply(Command::DetachEvent { place: Target::Point(1), event: 2 }).unwrap();
        let p = &d.scenario.points[0];
        assert_eq!((p.event_count, &p.event_slots[..5]), (4, &[1, 3, 4, 5, 0][..]));
        for id in [7, 6, 7] {
            d.apply(Command::AttachEvent { place: Target::Building(1), event: id }).unwrap();
        }
        let b = &d.scenario.buildings[0];
        assert_eq!((b.event_count, &b.event_slots[..2]), (2, &[7, 6][..]));
        assert_eq!(crate::editor::events::places_of(&d.scenario, 7), (vec![1], vec![]));
        // Undo takes the attachment back.
        d.undo();
        assert_eq!(d.scenario.buildings[0].event_count, 1);
        assert!(!d.apply(Command::DetachEvent { place: Target::Building(1), event: 5 }).unwrap().changed);
    }

    #[test]
    fn typing_an_event_title_merges() {
        let mut d = doc();
        d.apply(Command::NewEvent { kind: 1, repeat: false }).unwrap();
        for t in ["В", "Во", "Вол"] {
            let mut e = d.scenario.events[0].clone();
            e.title = crate::editor::events::with_name(&e.title, t);
            d.apply_merging(Command::SetEvent { id: 1, event: Box::new(e) }, Some("e1:title")).unwrap();
        }
        assert_eq!(d.scenario.events[0].title, "Вол");
        d.undo();
        assert_eq!(d.undo_label(), Some("New event"));
    }

    #[test]
    fn event_fields_land_at_documented_offsets() {
        use crate::editor::events::*;
        let mut d = doc();
        d.apply(Command::NewEvent { kind: 1, repeat: false }).unwrap();
        d.apply(Command::NewEvent { kind: 3, repeat: false }).unwrap();
        let mut e = d.scenario.events[1].clone();
        e.group_colour = 4;
        e.kind = 2;
        e.start_time = 0x0102_0304;
        set_repeat_days(&mut e, 2);
        set_duration_hours(&mut e, 5);
        e.archetype = 3;
        let c = &mut e.conditions;
        c.squad_count = Threshold { at_least: false, value: 3 }.raw();
        c.army_strength = Threshold { at_least: true, value: 900 }.raw();
        c.army_inactive = 7;
        c.stats_check = 1;
        c.level = Threshold { at_least: true, value: 4 }.raw();
        c.gold = Threshold { at_least: false, value: 300 }.raw();
        c.holiness_mana = 25;
        c.buildings_check = 1;
        c.buildings = [1, 2, 3];
        c.buildings_owner = [1, 6, 3];
        c.units_check = 1;
        c.units = [10, 11, 12];
        c.units_named = [1, 0, 2];
        c.units_owner = [2, 4, 5];
        c.artifacts_check = 1;
        c.artifacts = [20, 21, 22];
        c.artifacts_owner = [1, 1, 6];
        c.defeated_check = 1;
        c.defeated_armies = [5, 6];
        c.happened_yes_check = 1;
        c.happened_yes = [0x0201, 1];
        c.not_happened_check = 1;
        c.not_happened = [2, 0x0403];
        c.beaten_check = 1;
        c.beaten_armies = [8, 9];
        c.happened_no_check = 1;
        c.happened_no = [0x0605, 7];
        c.meet_army = 11;
        c.army_active = 12;
        c.confirm_question = 1;
        c.army_at_home = 13;
        let r = &mut e.results;
        r.patrol_army = 14;
        r.patrol_delta = -6;
        r.relative_event = 0x0908;
        r.relative_delay_hours = 48;
        r.cast_spell = 15;
        r.picture = PICTURE_VICTORY;
        r.experience = -100;
        r.gold = 0x1234;
        r.mana = 77;
        r.spells_learned = [1, 2, 3, 4];
        r.units_add = [30, 31, 32, 33];
        r.units_add_named = [1, 2, 0, 1];
        r.units_remove = [34, REMOVE_ADDED_UNIT, REMOVE_ANY_UNIT, 0];
        r.units_remove_named = [0, 0, 0, 2];
        r.artifacts_add = [40, 41, 42, 43];
        r.artifacts_remove = [44, 45, 46, 47];
        r.activate_armies = [16, 17];
        r.deactivate_army = 18;
        r.completes_quest = 0x0B0A;
        r.delay_hours = 6;
        r.light_lanterns = [1, 2, 3, 0x0102];
        r.removed_units_to_army = 19;
        r.new_hero_class = 50;
        r.chained_event = 0x0D0C;
        r.units_from_army = 20;
        r.move_to_hero = 1;
        r.show_army = 21;
        r.hero_one_hp = 1;
        r.start_battle_with = 22;
        r.no_meeting = 1;
        r.repeat_after_yes = 1;
        e.subordinate = 1;
        set_repeatable(&mut e, true);
        e.generate_battle_army = 1;
        e.title = with_flags("Сделка", "+Сделка", "/Обман");
        e.question = "Да?".into();
        e.message = "Готово.".into();
        e.custom_picture = Some(picture_from_rgba(1, 1, &[0, 255, 0, 255]).unwrap());
        d.apply(Command::SetEvent { id: 2, event: Box::new(e.clone()) }).unwrap();
        let pay = d.scenario.to_payload();
        let o = events_offset(&pay) + EVENT_SIZE;
        let b = &pay[o..o + EVENT_SIZE];
        assert_eq!(&b[0..2], &[4, 2]);
        assert_eq!(&b[2..6], &[4, 3, 2, 1]);
        assert_eq!(&b[6..10], &(2880u16).to_le_bytes().into_iter().chain(300u16.to_le_bytes()).collect::<Vec<_>>()[..]);
        assert_eq!(b[10], 3);
        assert_eq!(&b[11..13], &(-3i16).to_le_bytes());
        assert_eq!(&b[13..15], &900i16.to_le_bytes());
        assert_eq!(&b[15..19], &[7, 14, (-6i8) as u8, 1]);
        assert_eq!(&b[19..21], &4i16.to_le_bytes());
        assert_eq!(&b[21..23], &(-300i16).to_le_bytes());
        assert_eq!(&b[25..27], &25i16.to_le_bytes());
        assert_eq!(&b[29..36], &[1, 1, 2, 3, 1, 6, 3]);
        assert_eq!(&b[36..46], &[1, 10, 11, 12, 1, 0, 2, 2, 4, 5]);
        assert_eq!(&b[46..53], &[1, 20, 21, 22, 1, 1, 6]);
        assert_eq!(&b[53..56], &[1, 5, 6]);
        assert_eq!(&b[56..61], &[1, 1, 2, 1, 0]);
        assert_eq!(&b[61..66], &[1, 2, 0, 3, 4]);
        assert_eq!(&b[66..69], &[1, 8, 9]);
        assert_eq!(&b[69..74], &[1, 5, 6, 7, 0]);
        assert_eq!(&b[74..77], &[11, 12, 1]);
        assert_eq!(&b[77..83], &[8, 9, 48, 0, 15, 201]);
        assert_eq!(&b[83..87], &[0x9C, 0xFF, 0x34, 0x12]);
        assert_eq!(&b[89..91], &77i16.to_le_bytes());
        assert_eq!(&b[93..97], &[1, 2, 3, 4]);
        assert_eq!(&b[97..105], &[30, 31, 32, 33, 1, 2, 0, 1]);
        assert_eq!(&b[105..113], &[34, 0xFE, 0xFF, 0, 0, 0, 0, 2]);
        assert_eq!(&b[113..121], &[40, 41, 42, 43, 44, 45, 46, 47]);
        assert_eq!(&b[121..128], &[16, 17, 18, 0x0A, 0x0B, 6, 0]);
        assert_eq!(&b[128..136], &[1, 0, 2, 0, 3, 0, 2, 1]);
        assert_eq!(&b[136..140], &[19, 50, 0x0C, 0x0D]);
        assert_eq!(&b[140..151], &[1, 0, 20, 1, 21, 1, 13, 22, 1, 1, 1]);
        assert_eq!(&b[163..165], &(4u16 + 128 * 128 * 2).to_le_bytes());
        // The texts and the picture: title with its flag script, question, message.
        let back = Scenario::parse_payload(&pay).unwrap();
        assert_eq!(back.events[1], d.scenario.events[1]);
        assert_eq!(back.events[1].title, "Сделка%+Сделка=/Обман");
        let f = back.events[1].flags.clone().unwrap();
        assert_eq!((f.set.as_deref(), f.require_unset.as_deref()), (Some("Сделка"), Some("Обман")));
        // Opcode arguments share the XP, gold and mana bytes.
        let mut op = d.scenario.events[0].clone();
        set_opcode(&mut op, Some(19));
        set_opcode_args(&mut op, [3, 19, 11]);
        d.apply(Command::SetEvent { id: 1, event: Box::new(op) }).unwrap();
        let pay = d.scenario.to_payload();
        let b = &pay[events_offset(&pay)..events_offset(&pay) + EVENT_SIZE];
        assert_eq!((b[17], b[148]), (19, 1));
        assert_eq!(&b[83..87], &[3, 0, 19, 0]);
        assert_eq!(&b[89..91], &[11, 0]);
        // A relative event stores the "never" start.
        let mut rel = d.scenario.events[0].clone();
        set_relative(&mut rel, true, 0);
        d.apply(Command::SetEvent { id: 1, event: Box::new(rel) }).unwrap();
        let pay = d.scenario.to_payload();
        let o = events_offset(&pay);
        assert_eq!(&pay[o + 2..o + 6], &1_036_800_000u32.to_le_bytes());
    }

    #[test]
    fn a_new_map_plays() {
        // The in-memory scenario builds a game as the test-play button does.
        let mut d = doc();
        d.apply(Command::PlaceBuilding { x: 12, y: 12, kind: 3, picture_type: 3, variant: 0, size: (2, 2) }).unwrap();
        let content = std::sync::Arc::new(crate::rules::content::Content::builtin());
        let g = crate::rules::game::Game::from_scenario(content, &d.scenario, crate::rules::content::HeroClass::Knight);
        assert_eq!(g.world.locations.len(), 1);
        assert_eq!(g.tile(), (10, 10));
    }
}
