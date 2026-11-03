//! Placing a new building, army, lantern or object in a map file: the new table's TOML, with a
//! unique id (`<type>_<n>` for buildings and armies), the required fields filled with valid
//! defaults and a text key of its own (`<map>.<kind>.<id>.name`, which the package does not define
//! yet: the checks warn about it). The table is appended by one `Edit::Insert` (one undo step).

use std::collections::BTreeSet;

use toml_edit::Value;

use super::edit::{Edit, Kind, Locator};
use super::Workspace;

/// What to place, and where (`cell` is the cell the canvas was clicked on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place<'a> {
    /// A `[[building]]` of the building type `kind` (`village`, `castle` …).
    Building { kind: &'a str, cell: (u32, u32) },
    /// An `[[army]]` of the model `model` (`feudal`, `bandits` …).
    Army { model: &'a str, cell: (u32, u32) },
    /// A `[[point]]`, a lantern.
    Point { cell: (u32, u32) },
    /// A `[[object]]` of the kind `kind` (`trees`, `hills` …).
    Object { kind: &'a str, cell: (u32, u32) },
}

impl Place<'_> {
    /// The table kind the place adds.
    pub fn kind(&self) -> Kind {
        match self {
            Place::Building { .. } => Kind::Building,
            Place::Army { .. } => Kind::Army,
            Place::Point { .. } => Kind::Point,
            Place::Object { .. } => Kind::Object,
        }
    }
}

/// The unit a new army starts with: the base game's militiaman, a valid troop on every install.
const NEW_ARMY_TROOP: &str = "dt:militiaman";

/// The first `<type>_<n>` (n from 1) that no table of the kind has.
pub fn unique_id(kind_word: &str, taken: &BTreeSet<String>) -> String {
    (1..).map(|n| format!("{kind_word}_{n}")).find(|id| !taken.contains(id)).unwrap_or_default()
}

/// The TOML text of the new table (its header included, one trailing newline).
pub fn fragment(map: &str, place: Place, id: Option<&str>) -> String {
    let cell = |(x, y): (u32, u32)| format!("[{x}, {y}]");
    let quote = |s: &str| Value::from(s).to_string();
    match place {
        Place::Building { kind, cell: c } => {
            let id = id.unwrap_or_default();
            format!(
                "[[building]]\nid = {}\ntype = {}\nname = {}\ncell = {}\nfaction = \"neighbour\"\n",
                quote(id),
                quote(kind),
                quote(&format!("{map}.building.{id}.name")),
                cell(c)
            )
        }
        Place::Army { model, cell: c } => {
            let id = id.unwrap_or_default();
            format!(
                "[[army]]\nid = {}\nname = {}\ncell = {}\nfaction = \"neighbour\"\nmodel = {}\ntroops = [{{ unit = \"{NEW_ARMY_TROOP}\", level = 1, count = 3 }}]\n",
                quote(id),
                quote(&format!("{map}.army.{id}.name")),
                cell(c),
                quote(model)
            )
        }
        Place::Point { cell: c } => format!("[[point]]\ncell = {}\nradius = 3\nactive = true\n", cell(c)),
        Place::Object { kind, cell: c } => format!("[[object]]\nkind = {}\ncell = {}\n", quote(kind), cell(c)),
    }
}

/// Appends the place to the map `map` as one edit. Returns the new table's id (buildings and
/// armies) or `None` (lanterns and objects have none). The map's file must parse.
pub fn place(ws: &mut Workspace, map: &str, what: Place) -> Result<Option<String>, String> {
    let file = format!("maps/{map}/map.toml");
    let kind = what.kind();
    let taken: BTreeSet<String> = ws.spans(&file)?.into_iter().filter(|(l, _)| l.kind == kind).filter_map(|(l, _)| l.id).collect();
    let id = match what {
        Place::Building { kind: type_name, .. } => Some(unique_id(type_name, &taken)),
        Place::Army { .. } => Some(unique_id("army", &taken)),
        Place::Point { .. } | Place::Object { .. } => None,
    };
    let fragment = fragment(map, what, id.as_deref());
    let at = Locator { file, kind, id: id.clone(), index: 0 };
    ws.apply(Edit::Insert { at, fragment })?;
    Ok(id)
}
