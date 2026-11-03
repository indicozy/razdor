//! The map tools' rules, as edits of a map file and cell codes of its terrain: the brush sizes,
//! hills and forests (DTMapEdit's pages 1 and 2, `docs/reference/editor/main-window.md` §6.1,
//! §8.3, §10.2, §10.3, §11), burning (§15), the flood fill and the rectangle, and the mountains
//! check a building is placed with (§14.2). The planners read the map file's text and return
//! edits; [`commit`] applies them as one undo step. Nothing is drawn here.
//!
//! The package keeps no marks: a hill's footprint is its sprite's tens (`sprite / 10` cells
//! square, ending at its cell), as the original's. A forest sprite is `12 × family + variant`;
//! the variant is picked per cell from twelve, where the original counts the art it has loaded
//! (the studio does not read the install's art, so it writes the twelve base variants).

use std::collections::{BTreeSet, VecDeque};

use toml_edit::{DocumentMut, Item, Value};

use super::edit::{Edit, Kind, Locator};
use super::geometry::CellRect;
use super::Workspace;
use crate::rules::rng::Rng;
use crate::rules::world::footprint_cells;

/// The brush sizes, in cells a side (DTMapEdit's 1–6, and 9 for big hills and forests).
pub const BRUSH_SIZES: [u32; 7] = [1, 2, 3, 4, 5, 6, 9];

/// The terrain code of scorched land (`pack::terrain::DEFAULT_PALETTE`).
pub const SCORCHED: u8 = 13;

/// One `[[object]]` table of a map file's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectRow {
    /// The table's index among the `[[object]]` tables (file order).
    pub index: usize,
    pub kind: String,
    pub cell: (i64, i64),
    pub sprite: Option<i64>,
}

impl ObjectRow {
    /// The footprint of a massif (hills, mountains, stones): `sprite / 10` cells square, anchored
    /// at the cell (bottom-right). The default sprite of the kind when the table has none.
    pub fn massif_size(&self) -> i64 {
        (self.sprite.unwrap_or(10) / 10).max(1)
    }
}

/// Whether an object kind is a plant (`trees`, `dead_trees`, `thicket`): DT's second object slot.
pub fn is_plant(kind: &str) -> bool {
    matches!(kind, "trees" | "dead_trees" | "thicket")
}

/// Whether an object kind is a massif of DT's first object slot (hills, mountains, stones).
pub fn is_massif(kind: &str) -> bool {
    matches!(kind, "hills" | "mountains" | "stones")
}

/// The `[[object]]` tables of a map file's text, in file order. A text that does not parse has none.
pub fn object_rows(text: &str) -> Vec<ObjectRow> {
    let Ok(table) = text.parse::<toml::Table>() else { return Vec::new() };
    let Some(list) = table.get("object").and_then(toml::Value::as_array) else { return Vec::new() };
    list.iter()
        .enumerate()
        .filter_map(|(index, v)| {
            let t = v.as_table()?;
            let kind = t.get("kind")?.as_str()?.to_string();
            let cell = t.get("cell")?.as_array()?;
            let x = cell.first()?.as_integer()?;
            let y = cell.get(1)?.as_integer()?;
            let sprite = t.get("sprite").and_then(toml::Value::as_integer);
            Some(ObjectRow { index, kind, cell: (x, y), sprite })
        })
        .collect()
}

/// A forest brush's choice: a family of trees (0–9: 0–7 live trees, 8–9 dead ones, the families
/// of the original's art) or a thicket.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Forest {
    Family(u8),
    Thicket,
}

impl Forest {
    /// The object kind the choice places.
    pub fn kind(self) -> &'static str {
        match self {
            Forest::Family(f) if f < 8 => "trees",
            Forest::Family(_) => "dead_trees",
            Forest::Thicket => "thicket",
        }
    }

    /// The sprite of variant `v` (0–11) of the choice; a thicket has none.
    pub fn sprite(self, v: u8) -> Option<u8> {
        match self {
            Forest::Family(f) => Some(f.min(9) * 12 + v % 12),
            Forest::Thicket => None,
        }
    }

    /// Whether tree replacement may re-roll a plant of kind `old` with this choice: live and dead
    /// trees are one family, thickets only replace thickets (`main-window.md` §10.3).
    pub fn replaces(self, old: &str) -> bool {
        match self {
            Forest::Family(_) => matches!(old, "trees" | "dead_trees"),
            Forest::Thicket => old == "thicket",
        }
    }
}

/// The variant of the forest sprite at a cell: a fixed draw per cell, so the same cell always
/// gets the same tree and a drag over it does not change its look.
pub fn variant_at(x: i64, y: i64) -> u8 {
    let seed = (x as u32).wrapping_mul(0x9E37_79B9) ^ (y as u32).wrapping_mul(0x85EB_CA6B) | 1;
    Rng::new(seed).random(12) as u8
}

/// The cells of a brush of `size` whose bottom-right centre is `centre` (the brush rule: the
/// square ends at the centre, `CellRect::brush`), as `(x, y)` pairs, row by row.
pub fn square(x: i64, y: i64, size: u32) -> Vec<(i64, i64)> {
    let r = CellRect::brush(x as i32, y as i32, size);
    r.cells().map(|(x, y)| (i64::from(x), i64::from(y))).collect()
}

/// The centre a hill of `size` placed with the hovered cell `(x, y)` gets: the square's
/// bottom-right cell (DT §6.1: an even size reaches one cell further right and down).
pub fn hill_centre(x: i64, y: i64, size: u32) -> (i64, i64) {
    let r = CellRect::brush(x as i32, y as i32, size);
    (i64::from(r.x1), i64::from(r.y1))
}

/// The edits of a remove-and-insert: the removals of the objects at `removals` (descending, so each
/// index is still right when it is removed), then one insert of the `inserts` fragments.
fn finish(file: &str, mut removals: Vec<usize>, inserts: Vec<String>) -> Vec<Edit> {
    removals.sort_unstable();
    removals.dedup();
    let mut out: Vec<Edit> = removals
        .into_iter()
        .rev()
        .map(|index| Edit::Remove { at: Locator { file: file.to_string(), kind: Kind::Object, id: None, index } })
        .collect();
    // One insert for the whole stroke: each insert re-reads the file, and a brush covers many cells.
    if !inserts.is_empty() {
        out.push(Edit::Insert { at: Locator { file: file.to_string(), kind: Kind::Object, id: None, index: 0 }, fragment: inserts.concat() });
    }
    out
}

/// The TOML of a new `[[object]]`.
fn object_fragment(kind: &str, cell: (i64, i64), sprite: Option<u8>) -> String {
    let sprite = sprite.map(|s| format!("sprite = {s}\n")).unwrap_or_default();
    format!("[[object]]\nkind = \"{kind}\"\ncell = [{}, {}]\n{sprite}", cell.0, cell.1)
}

/// A hill of `size` at `centre`: the massif on that cell (hills, mountains or stones) it replaces
/// goes (one per cell and slot, as the original keeps them), then the hill with sprite `10 × size`.
pub fn hill_edits(file: &str, text: &str, centre: (i64, i64), size: u32) -> Vec<Edit> {
    let rows = object_rows(text);
    let removals = rows.iter().filter(|r| is_massif(&r.kind) && r.cell == centre).map(|r| r.index).collect();
    let sprite = u8::try_from(10 * size.min(25)).unwrap_or(250);
    finish(file, removals, vec![object_fragment("hills", centre, Some(sprite))])
}

/// The hills whose footprint covers `cell` go (DT §11: every one; the search reaches 8 cells).
pub fn hill_erase_edits(file: &str, text: &str, cell: (i64, i64)) -> Vec<Edit> {
    let removals = object_rows(text)
        .iter()
        .filter(|r| r.kind == "hills")
        .filter(|r| {
            let k = r.massif_size();
            let (x, y) = r.cell;
            cell.0 <= x && cell.0 > x - k && cell.1 <= y && cell.1 > y - k
        })
        .map(|r| r.index)
        .collect();
    finish(file, removals, Vec::new())
}

/// The forest brush over `cells` (in the order the stroke painted them): a plant of the choice on
/// each cell. With tree replacement on and a brush above 1, only the cells with a plant the choice
/// replaces are re-rolled, and the others are left alone; otherwise the cell's plants go and the
/// new one takes its place (the second object slot, as the original).
pub fn forest_edits(file: &str, text: &str, cells: &[(i64, i64)], forest: Forest, replace: bool, size: u32) -> Vec<Edit> {
    let rows = object_rows(text);
    let replacing = replace && size >= 2;
    let mut removals = Vec::new();
    let mut inserts = Vec::new();
    for &cell in dedup(cells).iter() {
        let here: Vec<&ObjectRow> = rows.iter().filter(|r| r.cell == cell && is_plant(&r.kind)).collect();
        let gone: Vec<usize> = if replacing {
            let old: Vec<usize> = here.iter().filter(|r| forest.replaces(&r.kind)).map(|r| r.index).collect();
            if old.is_empty() {
                continue;
            }
            old
        } else {
            here.iter().map(|r| r.index).collect()
        };
        removals.extend(gone);
        inserts.push(object_fragment(forest.kind(), cell, forest.sprite(variant_at(cell.0, cell.1))));
    }
    finish(file, removals, inserts)
}

/// Removes the plants on `cells` (DT's delete on the forests page: the cell's forest object).
pub fn plant_erase_edits(file: &str, text: &str, cells: &[(i64, i64)]) -> Vec<Edit> {
    let rows = object_rows(text);
    let set: BTreeSet<(i64, i64)> = cells.iter().copied().collect();
    let removals = rows.iter().filter(|r| is_plant(&r.kind) && set.contains(&r.cell)).map(|r| r.index).collect();
    finish(file, removals, Vec::new())
}

/// Burn (§15), the trees: each live tree becomes a dead one of the families 8 and 9 (sprites
/// 96 to 119). Thickets and the hills are kept (the studio has no scorched massif art to pick).
pub fn burn_tree_edits(file: &str, text: &str) -> Vec<Edit> {
    let mut out = Vec::new();
    for r in object_rows(text).into_iter().filter(|r| r.kind == "trees") {
        let at = Locator { file: file.to_string(), kind: Kind::Object, id: None, index: r.index };
        let family = 12 * i64::from(variant_at(r.cell.1, r.cell.0) % 2);
        let sprite = 96 + family + i64::from(variant_at(r.cell.0, r.cell.1));
        out.push(Edit::SetValue { at: at.clone(), key: "kind".into(), value: Value::from("dead_trees") });
        out.push(Edit::SetValue { at, key: "sprite".into(), value: Value::from(sprite) });
    }
    out
}

/// Burn (§15), the ground: every cell above the water and lava (codes 4 and up) becomes scorched
/// land. Returns the cells that change, with their new code.
pub fn burn_cells(codes: &[u8], width: u32) -> Vec<(i64, i64, u8)> {
    codes
        .iter()
        .enumerate()
        .filter(|(_, &c)| (4..=15).contains(&c) && c != SCORCHED)
        .map(|(i, _)| ((i as u32 % width) as i64, (i as u32 / width) as i64, SCORCHED))
        .collect()
}

/// The cells connected to `start` (4-neighbours) that have its code, the flood fill's region. Empty
/// outside the map. A fill with the region's own code changes nothing and is the caller's to skip.
pub fn flood(codes: &[u8], width: u32, height: u32, start: (u32, u32)) -> Vec<(u32, u32)> {
    if start.0 >= width || start.1 >= height {
        return Vec::new();
    }
    let at = |x: u32, y: u32| codes[(y * width + x) as usize];
    let want = at(start.0, start.1);
    let mut seen = vec![false; (width * height) as usize];
    let mut out = Vec::new();
    let mut queue = VecDeque::from([start]);
    seen[(start.1 * width + start.0) as usize] = true;
    while let Some((x, y)) = queue.pop_front() {
        out.push((x, y));
        let mut push = |nx: i64, ny: i64, queue: &mut VecDeque<(u32, u32)>| {
            if nx < 0 || ny < 0 || nx >= i64::from(width) || ny >= i64::from(height) {
                return;
            }
            let (nx, ny) = (nx as u32, ny as u32);
            let i = (ny * width + nx) as usize;
            if !seen[i] && at(nx, ny) == want {
                seen[i] = true;
                queue.push_back((nx, ny));
            }
        };
        push(i64::from(x) - 1, i64::from(y), &mut queue);
        push(i64::from(x) + 1, i64::from(y), &mut queue);
        push(i64::from(x), i64::from(y) - 1, &mut queue);
        push(i64::from(x), i64::from(y) + 1, &mut queue);
    }
    out
}

/// The cells of a rectangle, clipped to the map, row by row.
pub fn rect_cells(a: (i32, i32), b: (i32, i32), width: u32, height: u32) -> Vec<(i64, i64)> {
    CellRect::spanning(a, b).clip(width, height).map_or_else(Vec::new, |r| r.cells().map(|(x, y)| (i64::from(x), i64::from(y))).collect())
}

/// Whether a building of `size` at `anchor` covers a mountain (a `mountains` object's footprint,
/// DT's class 5). Rocks and thickets are not tested, as the original does not test them.
pub fn covers_mountains(text: &str, anchor: (i64, i64), size: (u32, u32)) -> bool {
    let building: BTreeSet<(i32, i32)> = footprint_cells((anchor.0 as i32, anchor.1 as i32), (size.0.max(1) as i32, size.1.max(1) as i32)).collect();
    object_rows(text).iter().filter(|r| r.kind == "mountains").any(|r| {
        let k = r.massif_size() as i32;
        let (x, y) = (r.cell.0 as i32, r.cell.1 as i32);
        (y - k + 1..=y).any(|yy| (x - k + 1..=x).any(|xx| building.contains(&(xx, yy))))
    })
}

/// A building of the compiled map: its id, its anchor (bottom-right cell) and its footprint size.
pub type BuildingFootprint = (String, (i32, i32), (i32, i32));

/// The `[[building]]` ids whose footprint covers `cell` (DT §11: each one, the search reaches 8
/// cells right and down). `buildings` are the compiled map's (`ui::studio::map_tab::buildings_of`).
pub fn building_erase_ids(buildings: &[BuildingFootprint], cell: (i32, i32)) -> Vec<String> {
    buildings
        .iter()
        .filter(|(_, anchor, size)| footprint_cells(*anchor, *size).any(|c| c == cell))
        .map(|(id, _, _)| id.clone())
        .collect()
}

/// Applies a tool's edits as one undo step: the map file's edits (in order) and the terrain cells
/// (`x`, `y`, code), on `map`. A failure puts the document back as it was and says why.
pub fn commit(ws: &mut Workspace, label: &str, map: &str, edits: Vec<Edit>, cells: Vec<(i64, i64, u8)>) -> Result<(), String> {
    ws.begin_group();
    let result = (|| -> Result<(), String> {
        if !cells.is_empty() {
            ws.paint_codes(map, cells)?;
        }
        for edit in edits {
            ws.apply(edit)?;
        }
        Ok(())
    })();
    match result {
        Ok(()) => {
            ws.end_group(label);
            Ok(())
        }
        Err(e) => {
            ws.cancel_group();
            Err(e)
        }
    }
}

/// The TOML of one table of a stamp or a copy, with its `cell` set to `cell` and, when given, its
/// `id` replaced (an army's leader loses its name: a named character is one per package). `None`
/// when the text is not one table.
pub fn retarget(fragment: &str, cell: (i64, i64), id: Option<&str>) -> Option<String> {
    let mut doc: DocumentMut = fragment.parse().ok()?;
    let key = doc.iter().map(|(k, _)| k.to_string()).next()?;
    let Item::ArrayOfTables(list) = doc.get_mut(&key)? else { return None };
    let t = list.get_mut(0)?;
    let mut at = toml_edit::Array::new();
    at.push(cell.0);
    at.push(cell.1);
    t.insert("cell", Item::Value(Value::Array(at)));
    if let Some(id) = id {
        t.insert("id", toml_edit::value(id));
    }
    if key == "army" {
        if let Some(leader) = t.get_mut("leader").and_then(Item::as_value_mut).and_then(Value::as_inline_table_mut) {
            leader.remove("name");
        }
    }
    Some(doc.to_string())
}

/// The cell of one table's TOML (`cell = [x, y]`), `None` when it has none.
pub fn cell_of_fragment(fragment: &str) -> Option<(i64, i64)> {
    let t: toml::Table = fragment.parse().ok()?;
    let table = t.values().next()?.as_array()?.first()?.as_table()?;
    let cell = table.get("cell")?.as_array()?;
    Some((cell.first()?.as_integer()?, cell.get(1)?.as_integer()?))
}

/// The cells of a list without repeats, in the order they first came.
fn dedup(cells: &[(i64, i64)]) -> Vec<(i64, i64)> {
    let mut seen = BTreeSet::new();
    cells.iter().copied().filter(|c| seen.insert(*c)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "title = \"t\"\n\n[[object]]\nkind = \"hills\"\ncell = [5, 5]\nsprite = 20\n\n[[object]]\nkind = \"trees\"\ncell = [6, 6]\nsprite = 3\n\n[[object]]\nkind = \"thicket\"\ncell = [6, 6]\n";

    #[test]
    fn brush_squares_end_at_the_centre() {
        assert_eq!(square(10, 10, 1), vec![(10, 10)]);
        assert_eq!(square(10, 10, 2), vec![(10, 10), (11, 10), (10, 11), (11, 11)], "an even size reaches right and down");
        assert_eq!(square(10, 10, 3).len(), 9);
        assert_eq!(square(10, 10, 9).len(), 81);
        assert_eq!(square(10, 10, 9).first(), Some(&(6, 6)));
        assert_eq!(hill_centre(10, 10, 2), (11, 11), "the hill's anchor is the square's bottom-right");
        assert_eq!(hill_centre(10, 10, 3), (11, 11), "an odd size reaches one cell right and down too: the centre is hover + size/2");
        assert_eq!(hill_centre(10, 10, 1), (10, 10));
    }

    #[test]
    fn object_rows_read_the_kind_cell_and_sprite_in_file_order() {
        let rows = object_rows(MAP);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0], ObjectRow { index: 0, kind: "hills".into(), cell: (5, 5), sprite: Some(20) });
        assert_eq!(rows[1].sprite, Some(3));
        assert_eq!(rows[2].sprite, None);
        assert_eq!(rows[0].massif_size(), 2);
        assert!(object_rows("[[object").is_empty());
    }

    #[test]
    fn a_hill_replaces_the_massif_on_its_cell_and_takes_the_brush_size() {
        let edits = hill_edits("maps/x/map.toml", MAP, (5, 5), 3);
        // The hill on (5, 5) goes (index 0), then the new hill with sprite 30 is inserted.
        assert!(matches!(&edits[0], Edit::Remove { at } if at.index == 0));
        let Edit::Insert { fragment, .. } = &edits[1] else { panic!("an insert") };
        assert!(fragment.contains("kind = \"hills\"") && fragment.contains("sprite = 30") && fragment.contains("cell = [5, 5]"), "{fragment}");
        assert_eq!(edits.len(), 2, "the tree on (6, 6) is not a massif: it stays");
    }

    #[test]
    fn hill_erase_takes_every_hill_whose_footprint_covers_the_cell() {
        // The 2x2 hill ends at (5, 5): it covers (4..=5, 4..=5).
        let edits = hill_erase_edits("maps/x/map.toml", MAP, (4, 4));
        assert_eq!(edits.len(), 1);
        assert!(matches!(&edits[0], Edit::Remove { at } if at.index == 0 && at.kind == Kind::Object));
        assert!(hill_erase_edits("maps/x/map.toml", MAP, (6, 6)).is_empty(), "a tree is not a hill");
    }

    #[test]
    fn forests_paint_one_plant_per_cell_and_replace_only_what_they_replace() {
        let cells = [(6, 6), (7, 6)];
        // Not replacing: the thicket and the tree on (6, 6) go, the new plant goes in.
        let all = forest_edits("f", MAP, &cells, Forest::Family(2), false, 2);
        let removed: Vec<usize> = all.iter().filter_map(|e| if let Edit::Remove { at } = e { Some(at.index) } else { None }).collect();
        assert_eq!(removed, vec![2, 1], "descending, both plants of (6, 6)");
        // Replacing: only the cells with a tree or a thicket the family replaces are re-rolled.
        let rep = forest_edits("f", MAP, &cells, Forest::Family(2), true, 2);
        let inserts = rep.iter().filter(|e| matches!(e, Edit::Insert { .. })).count();
        assert_eq!(inserts, 1, "(7, 6) has no plant to replace");
        // A thicket replaces only a thicket.
        let thicket = forest_edits("f", MAP, &cells, Forest::Thicket, true, 3);
        assert!(thicket.iter().any(|e| matches!(e, Edit::Remove { at } if at.index == 2)));
        assert!(!thicket.iter().any(|e| matches!(e, Edit::Remove { at } if at.index == 1)), "a tree stays");
        // At size 1 replacement does not apply.
        let one = forest_edits("f", MAP, &[(7, 6)], Forest::Family(2), true, 1);
        assert_eq!(one.len(), 1, "a single tree is placed on an empty cell");
    }

    #[test]
    fn family_sprites_stay_in_their_family_and_kinds_follow_it() {
        assert_eq!(Forest::Family(3).kind(), "trees");
        assert_eq!(Forest::Family(9).kind(), "dead_trees");
        assert_eq!(Forest::Family(3).sprite(5), Some(41));
        assert_eq!(Forest::Thicket.sprite(0), None);
        for v in 0..40 {
            assert!(variant_at(v, 7) < 12);
        }
    }

    #[test]
    fn burning_turns_trees_dead_and_scorches_the_ground_above_water() {
        let edits = burn_tree_edits("f", MAP);
        assert_eq!(edits.len(), 2, "one tree, two values");
        assert!(matches!(&edits[1], Edit::SetValue { key, .. } if key == "sprite"));
        // Codes: 0..=3 water and lava stay; 4..=12 and 14, 15 scorch; 13 is already scorched.
        let codes: Vec<u8> = (0..16).collect();
        let cells = burn_cells(&codes, 4);
        assert_eq!(cells.len(), 11, "codes 4..=12 (9) and 14, 15 (2): not 13, already scorched");
        assert!(cells.iter().all(|&(_, _, c)| c == SCORCHED));
        assert!(!cells.iter().any(|&(x, y, _)| (x + 4 * y) < 4), "water and lava stay");
    }

    #[test]
    fn flood_fill_stays_inside_its_kind_and_the_map() {
        // A 4x3 map: a wall of code 1 across column 2, the rest 0.
        let codes = [0u8, 0, 1, 0, 0, 0, 1, 0, 0, 0, 1, 0];
        let region = flood(&codes, 4, 3, (0, 0));
        assert_eq!(region.len(), 6, "the left two columns");
        assert!(region.iter().all(|&(x, _)| x < 2));
        assert_eq!(flood(&codes, 4, 3, (2, 0)).len(), 3, "the wall is its own region");
        assert!(flood(&codes, 4, 3, (9, 9)).is_empty());
    }

    #[test]
    fn rectangles_are_clipped_to_the_map() {
        assert_eq!(rect_cells((3, 3), (1, 2), 4, 4).len(), 3 * 2, "any corner order, 3 by 2");
        assert_eq!(rect_cells((2, 2), (9, 9), 4, 4).len(), 4, "clipped to 2..=3");
        assert!(rect_cells((9, 9), (10, 10), 4, 4).is_empty());
    }

    #[test]
    fn a_building_on_a_mountain_is_flagged_and_one_beside_it_is_not() {
        let text = "[[object]]\nkind = \"mountains\"\ncell = [8, 8]\nsprite = 20\n";
        // The mountain covers (7..=8, 7..=8).
        assert!(covers_mountains(text, (9, 9), (2, 2)), "the building's footprint reaches (8, 8)");
        assert!(!covers_mountains(text, (12, 12), (2, 2)));
        assert!(!covers_mountains(MAP, (9, 9), (2, 2)), "hills are not mountains");
    }

    #[test]
    fn a_building_erase_names_the_buildings_whose_footprint_covers_the_cell() {
        let buildings = vec![("a".to_string(), (5, 5), (2, 2)), ("b".to_string(), (20, 20), (1, 1))];
        assert_eq!(building_erase_ids(&buildings, (4, 4)), vec!["a".to_string()]);
        assert!(building_erase_ids(&buildings, (10, 10)).is_empty());
    }

    #[test]
    fn retarget_moves_a_table_and_gives_it_an_id() {
        let frag = "[[army]]\nid = \"toll\"\ncell = [1, 1]\nmodel = \"feudal\"\nleader = { unit = \"dt:x\" }\n";
        let out = retarget(frag, (7, 9), Some("toll_2")).expect("one table");
        assert!(out.contains("id = \"toll_2\"") && out.contains("cell = [7, 9]") && !out.contains("name ="), "{out}");
        assert!(out.contains("unit = \"dt:x\""), "the leader keeps its unit: {out}");
        assert_eq!(cell_of_fragment(&out), Some((7, 9)));
        let b = retarget("[[building]]\nid = \"a\"\ncell = [3, 3]\n", (1, 2), None).unwrap();
        assert!(b.contains("id = \"a\"") && b.contains("cell = [1, 2]"));
    }
}
