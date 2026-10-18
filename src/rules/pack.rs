//! The hero's pack, as the original keeps it (0x68dce0, magic-items.md §5.1): 256 cells,
//! each empty or holding one item, with holes where items were taken out. An item stays in
//! the cell it was put in; the grid shows the cells in order, five to a row.
//!
//! - A gained item (an event, loot, a purchase) goes into the first empty cell (0x49a85c);
//!   with all 256 taken the original loses it without a word. Razdor's pack (the owner's
//!   decision, 2026-10-18, a deviation) has no last cell: it grows by a cell past the last one
//!   used, so nothing is ever lost to a full pack.
//! - Removing an item by its number takes the first cell holding it (0x49a8a8).
//! - The pack's length is its last used cell + 1 (0x49a718); the scroll runs over it.
//!
//! Saved as a list of cells, `null` for a hole, trailing holes left out: a save of the older
//! packed list (`[5, 7]`) reads as the same items in the first cells.

use super::content::ItemId;

/// Cells of the original's pack (0x68dce0: 256 entries). Razdor's pack has no such bound
/// (a deviation, see the module's note); the number stays for the specs and the tests.
pub const PACK_SIZE: usize = 256;

/// The hero's pack: cells in order, `None` an empty one; never a trailing empty cell.
#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "Vec<Option<ItemId>>", into = "Vec<Option<ItemId>>")]
pub struct Pack {
    cells: Vec<Option<ItemId>>,
}

impl From<Vec<Option<ItemId>>> for Pack {
    fn from(cells: Vec<Option<ItemId>>) -> Pack {
        let mut p = Pack { cells };
        p.trim();
        p
    }
}

impl From<Pack> for Vec<Option<ItemId>> {
    fn from(p: Pack) -> Self {
        p.cells
    }
}

impl Pack {
    pub fn new() -> Pack {
        Pack::default()
    }

    /// The item in cell `i`, if any.
    pub fn get(&self, i: usize) -> Option<ItemId> {
        self.cells.get(i).copied().flatten()
    }

    /// The pack's length: its last used cell + 1, 0 when empty (0x49a718).
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Items held.
    pub fn count(&self) -> usize {
        self.cells.iter().flatten().count()
    }

    /// No empty cell left: never in Razdor's growing pack (a deviation: the original's is
    /// full at 256 items, 0x49a85c). Kept so the callers read as the original's checks.
    pub fn is_full(&self) -> bool {
        false
    }

    /// Puts `item` in the first empty cell (0x49a85c), or in a new cell past the last one
    /// (Razdor's deviation: the original loses the item with all 256 cells taken). Always true.
    pub fn add(&mut self, item: ItemId) -> bool {
        let i = (0..self.cells.len()).find(|&i| self.get(i).is_none()).unwrap_or(self.cells.len());
        self.set(i, Some(item));
        true
    }

    /// Takes the item out of cell `i`, leaving it empty (the click that picks it up, 0x4c24f4;
    /// a sale).
    pub fn take(&mut self, i: usize) -> Option<ItemId> {
        let item = self.cells.get_mut(i)?.take();
        self.trim();
        item
    }

    /// Puts `item` (or nothing) in cell `i`; returns what was there.
    pub fn set(&mut self, i: usize, item: Option<ItemId>) -> Option<ItemId> {
        if i >= self.cells.len() {
            item?;
            self.cells.resize(i + 1, None);
        }
        let was = std::mem::replace(&mut self.cells[i], item);
        self.trim();
        was
    }

    /// Takes `item` out of the first cell holding it (0x49a8a8's pack part). False when the
    /// pack has none.
    pub fn remove_first(&mut self, item: ItemId) -> bool {
        match self.cells.iter().position(|&c| c == Some(item)) {
            Some(i) => {
                self.take(i);
                true
            }
            None => false,
        }
    }

    pub fn contains(&self, item: ItemId) -> bool {
        self.cells.contains(&Some(item))
    }

    /// The items in cell order, holes skipped.
    pub fn iter(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.cells.iter().flatten().copied()
    }

    /// The used cells and their items, in cell order.
    pub fn entries(&self) -> impl Iterator<Item = (usize, ItemId)> + '_ {
        self.cells.iter().enumerate().filter_map(|(i, c)| c.map(|item| (i, item)))
    }

    /// The items in cell order, holes skipped.
    pub fn items(&self) -> Vec<ItemId> {
        self.iter().collect()
    }

    pub fn clear(&mut self) {
        self.cells.clear();
    }

    fn trim(&mut self) {
        while self.cells.last() == Some(&None) {
            self.cells.pop();
        }
    }
}

/// The item in a used cell (panics on an empty one: take [`Pack::get`] when it may be).
impl std::ops::Index<usize> for Pack {
    type Output = ItemId;
    fn index(&self, i: usize) -> &ItemId {
        self.cells[i].as_ref().expect("an empty pack cell")
    }
}

/// A pack equals a list when its cells hold the list's items from the first on, no holes.
impl PartialEq<Vec<ItemId>> for Pack {
    fn eq(&self, other: &Vec<ItemId>) -> bool {
        self.cells.len() == other.len() && self.cells.iter().zip(other).all(|(c, i)| *c == Some(*i))
    }
}

/// The items in the first cells, in order (no holes).
impl From<Vec<ItemId>> for Pack {
    fn from(items: Vec<ItemId>) -> Pack {
        Pack { cells: items.into_iter().map(Some).collect() }
    }
}

impl FromIterator<ItemId> for Pack {
    fn from_iter<T: IntoIterator<Item = ItemId>>(iter: T) -> Pack {
        Pack { cells: iter.into_iter().map(Some).collect() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_taken_item_leaves_a_hole_and_a_new_one_fills_the_first() {
        // 0x4c24f4 empties the clicked cell; 0x49a85c fills the first empty one.
        let mut p = Pack::from(vec![ItemId(1), ItemId(2), ItemId(3)]);
        assert_eq!(p.take(1), Some(ItemId(2)));
        assert_eq!((p.get(0), p.get(1), p.get(2), p.len()), (Some(ItemId(1)), None, Some(ItemId(3)), 3));
        assert!(p.add(ItemId(9)));
        assert_eq!(p.get(1), Some(ItemId(9)));
        // A cell anywhere keeps its item; the length is the last used cell + 1 (0x49a718).
        p.set(12, Some(ItemId(4)));
        assert_eq!((p.len(), p.count()), (13, 4));
        p.take(12);
        assert_eq!(p.len(), 3, "trailing holes are not counted");
    }

    #[test]
    fn deviation_the_pack_grows_past_256_and_the_first_match_is_removed() {
        // The original loses an item with all 256 cells taken (0x49a85c); Razdor's pack grows.
        let mut p: Pack = std::iter::repeat_n(ItemId(5), PACK_SIZE).collect();
        assert!(!p.is_full() && p.add(ItemId(6)));
        assert_eq!((p.get(PACK_SIZE), p.len()), (Some(ItemId(6)), PACK_SIZE + 1));
        p.set(1000, Some(ItemId(7)));
        assert_eq!(p.get(1000), Some(ItemId(7)), "any cell takes an item");
        let mut p = Pack::from(vec![ItemId(5), ItemId(6), ItemId(6)]);
        assert!(p.remove_first(ItemId(6)));
        assert_eq!((p.get(1), p.get(2)), (None, Some(ItemId(6))), "0x49a8a8: the first match");
        assert!(!p.remove_first(ItemId(7)));
    }

    #[test]
    fn the_pack_saves_its_holes_and_reads_the_older_packed_list() {
        let mut p = Pack::from(vec![ItemId(5), ItemId(7)]);
        p.set(4, Some(ItemId(8)));
        p.take(0);
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(json, "[null,7,null,null,8]");
        assert_eq!(serde_json::from_str::<Pack>(&json).unwrap(), p);
        // An older save's pack, packed: the same items from the first cell on.
        let old: Pack = serde_json::from_str("[5,7]").unwrap();
        assert_eq!(old, Pack::from(vec![ItemId(5), ItemId(7)]));
    }
}
