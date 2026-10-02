//! Typed edits of a scenario. Every change the editor makes is one of these, applied by
//! [`super::EditorDoc::apply`] so it can be undone.

use crate::dt::dtm::{Army, Building, Event, Header, NamedCharacter, Point};

use crate::i18n::n_;

use super::doc::Target;

/// Which objects an erase removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectFilter {
    All,
    /// Hills, mountains, stones (classes 1–8).
    Massifs,
    /// Trees and thickets (classes 9–12).
    Plants,
}

/// The scenario settings edited together: the header (minus the size, which never changes
/// here) and the scenario's own strings.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub header: Header,
    pub title: String,
    pub description: String,
    pub campaign_name: String,
    pub next_map: String,
    pub named_characters: Vec<NamedCharacter>,
    /// The scenario's own picture (raw, as the file holds it).
    pub scenario_picture: Option<Vec<u8>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// A square brush of terrain `code` centred on the cell.
    PaintTerrain { x: i32, y: i32, size: u32, code: u8 },
    /// The 4-connected region of the cell's surface becomes `code`.
    FillTerrain { x: u32, y: u32, code: u8 },
    /// A rectangle of `code` between two corners.
    RectTerrain { from: (i32, i32), to: (i32, i32), code: u8 },
    /// One object on every cell of the brush (a cell holding this very object is skipped).
    PlaceObjects { x: i32, y: i32, size: u32, class: u8, sprite: u8 },
    /// Every object (of the filter) standing on a cell of the brush.
    EraseObjects { x: i32, y: i32, size: u32, filter: ObjectFilter },
    /// A new building with the defaults of its type; `(x, y)` is the bottom-right cell.
    PlaceBuilding { x: u16, y: u16, kind: u8, picture_type: u8, variant: u8, size: (u8, u8) },
    MoveBuilding { id: u16, x: u16, y: u16 },
    /// Removes a building; later ids shift down and references follow ([`super::refs`]).
    DeleteBuilding { id: u16 },
    /// Replaces a building's record (its property panel).
    SetBuilding { id: u16, building: Box<Building> },
    /// A new army of map model `model` (4–7) with the original's defaults.
    PlaceArmy { x: u16, y: u16, model: u8 },
    MoveArmy { id: u8, x: u16, y: u16 },
    DeleteArmy { id: u8 },
    SetArmy { id: u8, army: Box<Army> },
    /// A new point of model `model`: 8 lantern, 9 event point, 10 AI target point.
    PlacePoint { x: u16, y: u16, model: u8 },
    MovePoint { id: u8, x: u16, y: u16 },
    DeletePoint { id: u8 },
    SetPoint { id: u8, point: Box<Point> },
    /// Replaces the scenario settings; a new start date moves the events' starts.
    SetSettings(Box<Settings>),
    /// A named character at the end of the list.
    AddNamedCharacter { unit: u8, name: String },
    /// Removes named character `index` (1-based); as in the original, armies and events keep
    /// their numbers ([`super::refs::remove_named_character`]).
    RemoveNamedCharacter { index: u8 },
    /// A new event of type `kind` (1 global … 4 rumour) at the end of the list, with the
    /// editor option "new events repeat" ([`super::events::new_event`]).
    NewEvent { kind: u8, repeat: bool },
    /// The original's copy of event `id` (0x539214): numbered by [`super::events::copy_name`],
    /// without its own picture, placed where the event on the list's next row is (`next`,
    /// the list as filtered; `None` when `id` is the last row), with the record of the event
    /// just before that place.
    DuplicateEvent { id: u16, next: Option<u16> },
    /// Moves event `from` to position `to`, renumbering ([`super::refs::move_event`]).
    MoveEvent { from: u16, to: u16 },
    /// Removes an event as the original's delete does ([`super::refs::remove_event`]).
    DeleteEvent { id: u16 },
    /// Replaces an event's record (its property panel).
    SetEvent { id: u16, event: Box<Event> },
    /// Adds event `event` to a building's or point's local list.
    AttachEvent { place: Target, event: u16 },
    /// Takes event `event` out of a building's or point's list.
    DetachEvent { place: Target, event: u16 },
}

impl Command {
    /// A short label for the undo list (English; shown through `i18n::tr`).
    pub fn label(&self) -> &'static str {
        match self {
            Command::PaintTerrain { .. } => n_("Paint terrain"),
            Command::FillTerrain { .. } => n_("Fill terrain"),
            Command::RectTerrain { .. } => n_("Terrain rectangle"),
            Command::PlaceObjects { .. } => n_("Place objects"),
            Command::EraseObjects { .. } => n_("Erase objects"),
            Command::PlaceBuilding { .. } => n_("Place building"),
            Command::MoveBuilding { .. } => n_("Move building"),
            Command::DeleteBuilding { .. } => n_("Delete building"),
            Command::SetBuilding { .. } => n_("Edit building"),
            Command::PlaceArmy { .. } => n_("Place army"),
            Command::MoveArmy { .. } => n_("Move army"),
            Command::DeleteArmy { .. } => n_("Delete army"),
            Command::SetArmy { .. } => n_("Edit army"),
            Command::PlacePoint { .. } => n_("Place point"),
            Command::MovePoint { .. } => n_("Move point"),
            Command::DeletePoint { .. } => n_("Delete point"),
            Command::SetPoint { .. } => n_("Edit point"),
            Command::SetSettings(_) => n_("Edit scenario settings"),
            Command::AddNamedCharacter { .. } => n_("Add named character"),
            Command::RemoveNamedCharacter { .. } => n_("Remove named character"),
            Command::NewEvent { .. } => n_("New event"),
            Command::DuplicateEvent { .. } => n_("Duplicate event"),
            Command::MoveEvent { .. } => n_("Move event"),
            Command::DeleteEvent { .. } => n_("Delete event"),
            Command::SetEvent { .. } => n_("Edit event"),
            Command::AttachEvent { .. } => n_("Attach event"),
            Command::DetachEvent { .. } => n_("Detach event"),
        }
    }

    /// The parts of the scenario the command may change (for undo snapshots).
    pub(super) fn sections(&self) -> Sections {
        use Sections as S;
        match self {
            Command::PaintTerrain { .. } | Command::FillTerrain { .. } | Command::RectTerrain { .. } => S::TERRAIN,
            Command::PlaceObjects { .. } | Command::EraseObjects { .. } => S::OBJECTS,
            Command::PlaceBuilding { .. } | Command::MoveBuilding { .. } | Command::SetBuilding { .. } => S::BUILDINGS,
            Command::DeleteBuilding { .. } => S::BUILDINGS | S::ARMIES | S::EVENTS | S::META,
            Command::PlaceArmy { .. } | Command::MoveArmy { .. } | Command::SetArmy { .. } => S::ARMIES,
            Command::DeleteArmy { .. } => S::ARMIES | S::BUILDINGS | S::EVENTS,
            Command::PlacePoint { .. } | Command::MovePoint { .. } | Command::SetPoint { .. } => S::POINTS,
            Command::DeletePoint { .. } => S::POINTS | S::EVENTS,
            Command::SetSettings(_) => S::META | S::EVENTS,
            Command::AddNamedCharacter { .. } | Command::RemoveNamedCharacter { .. } => S::META,
            Command::NewEvent { .. } | Command::SetEvent { .. } => S::EVENTS,
            Command::DeleteEvent { .. } | Command::DuplicateEvent { .. } | Command::MoveEvent { .. } => S::EVENTS | S::BUILDINGS | S::POINTS | S::META,
            Command::AttachEvent { place, .. } | Command::DetachEvent { place, .. } => match place {
                Target::Building(_) => S::BUILDINGS,
                Target::Point(_) => S::POINTS,
                // Armies hold no event lists; the command fails.
                Target::Army(_) => S::EVENTS,
            },
        }
    }
}

/// A set of scenario parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sections(pub(super) u8);

impl Sections {
    pub const TERRAIN: Sections = Sections(1);
    pub const OBJECTS: Sections = Sections(2);
    pub const BUILDINGS: Sections = Sections(4);
    pub const ARMIES: Sections = Sections(8);
    pub const POINTS: Sections = Sections(16);
    pub const EVENTS: Sections = Sections(32);
    /// Header and scenario strings.
    pub const META: Sections = Sections(64);
    pub const ALL: [Sections; 7] =
        [Sections::TERRAIN, Sections::OBJECTS, Sections::BUILDINGS, Sections::ARMIES, Sections::POINTS, Sections::EVENTS, Sections::META];

    pub fn contains(self, o: Sections) -> bool {
        self.0 & o.0 == o.0
    }
}

impl std::ops::BitOr for Sections {
    type Output = Sections;
    fn bitor(self, o: Sections) -> Sections {
        Sections(self.0 | o.0)
    }
}
