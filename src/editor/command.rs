//! Typed edits of a scenario. Every change the editor makes is one of these, applied by
//! [`super::EditorDoc::apply`] so it can be undone.

use std::sync::Arc;

use crate::dt::dtm::{Army, Building, Event, Header, NamedCharacter, Point};

use crate::i18n::n_;

use super::brush::{Held, Page};
use super::doc::Target;
use super::naming::NamePools;
use super::palette::ForestFacts;

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
    /// The original's terrain brush: the square of side `size` ending at the brush centre
    /// `(x, y)` ([`super::brush::paint_terrain`]).
    PaintTerrain { x: i32, y: i32, size: u32, code: u8 },
    /// The 4-connected region of the cell's surface becomes `code` (Razdor's), each cell as
    /// the terrain brush paints it.
    FillTerrain { x: u32, y: u32, code: u8 },
    /// A rectangle of `code` between two corners (Razdor's), each cell as the terrain brush
    /// paints it.
    RectTerrain { from: (i32, i32), to: (i32, i32), code: u8 },
    /// The hills and forests brush ([`super::brush::place_object`]).
    PlaceObject { x: i32, y: i32, size: u32, class: u8, sprite: u8, facts: ForestFacts, replace: bool },
    /// The buildings brush ([`super::brush::place_building`]): picture, footprint, the
    /// picture's brush and the name lists.
    PlaceBuilding { x: i32, y: i32, picture_type: u8, variant: u8, size: (u8, u8), brush: u32, names: Option<Arc<NamePools>> },
    /// The items brush ([`super::brush::place_item`]): kind 1–3 hero start, 4–7 army, 8–10
    /// point.
    PlaceItem { x: u16, y: u16, kind: u8 },
    /// The delete brush of a page at a cell ([`super::brush::delete_at`]).
    DeleteAt { x: u16, y: u16, page: Page },
    /// A held object dropped with its anchor at `(x, y)` ([`super::brush::drop`]).
    Drop { held: Held, x: i32, y: i32 },
    /// A lantern's radius from the number dialog ([`super::brush::lantern_radius`]).
    LanternRadius { id: u16, radius: u8, placed: bool },
    /// "Burn everything" ([`super::brush::burn`]).
    Burn { facts: ForestFacts },
    /// Removes a building; later ids shift down and references follow ([`super::refs`]).
    DeleteBuilding { id: u16 },
    /// Replaces a building's record (its property panel).
    SetBuilding { id: u16, building: Box<Building> },
    DeleteArmy { id: u8 },
    SetArmy { id: u8, army: Box<Army> },
    DeletePoint { id: u16 },
    SetPoint { id: u16, point: Box<Point> },
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
            Command::PlaceObject { .. } => n_("Place objects"),
            Command::PlaceBuilding { .. } => n_("Place building"),
            Command::PlaceItem { .. } => n_("Place item"),
            Command::DeleteAt { .. } => n_("Delete"),
            Command::Drop { .. } => n_("Move"),
            Command::LanternRadius { .. } => n_("Lantern radius"),
            Command::Burn { .. } => n_("Burn everything"),
            Command::DeleteBuilding { .. } => n_("Delete building"),
            Command::SetBuilding { .. } => n_("Edit building"),
            Command::DeleteArmy { .. } => n_("Delete army"),
            Command::SetArmy { .. } => n_("Edit army"),
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
            Command::PaintTerrain { .. } | Command::FillTerrain { .. } | Command::RectTerrain { .. } => S::TERRAIN | S::OBJECTS,
            Command::PlaceObject { .. } => S::OBJECTS,
            Command::PlaceBuilding { .. } => S::BUILDINGS | S::OBJECTS,
            Command::PlaceItem { .. } => S::META | S::ARMIES | S::POINTS,
            Command::DeleteAt { .. } => S::OBJECTS | S::BUILDINGS | S::ARMIES | S::POINTS | S::EVENTS | S::META,
            Command::Drop { .. } => S::BUILDINGS | S::ARMIES | S::POINTS,
            Command::LanternRadius { .. } => S::POINTS,
            Command::Burn { .. } => S::TERRAIN | S::OBJECTS | S::BUILDINGS,
            Command::SetBuilding { .. } => S::BUILDINGS,
            Command::DeleteBuilding { .. } => S::BUILDINGS | S::ARMIES | S::EVENTS | S::META,
            Command::SetArmy { .. } => S::ARMIES,
            Command::DeleteArmy { .. } => S::ARMIES | S::BUILDINGS | S::EVENTS,
            Command::SetPoint { .. } => S::POINTS,
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
