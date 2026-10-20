//! The item lists' view order (a Razdor extra, the owner's wish): the hero's backpack and the
//! market's lists can show one group of items, and sorted by name or price. This is only the
//! order and the choice of the shown cells; the pack itself (`game.pack`) never changes.

use std::cmp::Ordering;

use super::content::{ArtefactType, Content, ItemId};
use super::custom::{self, Category};

/// The groups an item list is shown by: everything, one of the six classes of
/// [`custom::Category`], or the rest (potions and trade goods).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ItemGroup {
    #[default]
    All,
    Weapons,
    Armour,
    Helms,
    Shields,
    Rings,
    Amulets,
    /// Every kind none of the six named groups holds (potions and trade goods).
    Other,
}

impl ItemGroup {
    pub const EVERY: [ItemGroup; 8] = [
        ItemGroup::All,
        ItemGroup::Weapons,
        ItemGroup::Armour,
        ItemGroup::Helms,
        ItemGroup::Shields,
        ItemGroup::Rings,
        ItemGroup::Amulets,
        ItemGroup::Other,
    ];

    /// The six named groups, each a [`Category`] (`Other` is what none of them holds).
    fn named(self) -> Option<Category> {
        match self {
            ItemGroup::All | ItemGroup::Other => None,
            ItemGroup::Weapons => Some(Category::Weapons),
            ItemGroup::Armour => Some(Category::Armour),
            ItemGroup::Helms => Some(Category::Helms),
            ItemGroup::Shields => Some(Category::Shields),
            ItemGroup::Rings => Some(Category::Rings),
            ItemGroup::Amulets => Some(Category::Amulets),
        }
    }

    /// An item of `kind` is shown under this group.
    pub fn holds(self, kind: ArtefactType) -> bool {
        match self {
            ItemGroup::All => true,
            ItemGroup::Other => ItemGroup::EVERY.into_iter().all(|g| g == ItemGroup::All || g == ItemGroup::Other || !g.holds(kind)),
            named => named.named().is_some_and(|c| c.holds(kind)),
        }
    }

    /// The group after this one, cycling back to `All` after `Other`.
    pub fn next(self) -> ItemGroup {
        let i = ItemGroup::EVERY.iter().position(|&g| g == self).unwrap_or(0);
        ItemGroup::EVERY[(i + 1) % ItemGroup::EVERY.len()]
    }

    /// The group before this one, cycling to `Other` before `All`.
    pub fn prev(self) -> ItemGroup {
        let i = ItemGroup::EVERY.iter().position(|&g| g == self).unwrap_or(0);
        ItemGroup::EVERY[(i + ItemGroup::EVERY.len() - 1) % ItemGroup::EVERY.len()]
    }
}

/// The order an item list shows its items in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PackSort {
    /// The pack's own order.
    #[default]
    Shown,
    /// Name A to Z.
    NameUp,
    /// Name Z to A.
    NameDown,
    /// The dearest first.
    PriceDown,
    /// The cheapest first.
    PriceUp,
}

impl PackSort {
    /// The order after this one, cycling back to the pack's own order.
    pub fn next(self) -> PackSort {
        match self {
            PackSort::Shown => PackSort::NameUp,
            PackSort::NameUp => PackSort::NameDown,
            PackSort::NameDown => PackSort::PriceDown,
            PackSort::PriceDown => PackSort::PriceUp,
            PackSort::PriceUp => PackSort::Shown,
        }
    }
}

/// A name's key in alphabetical order, case left out; «ё» right after «е» (the market's
/// title sort uses it too).
pub fn alphabet_key(s: &str) -> Vec<(char, bool)> {
    s.chars().flat_map(char::to_lowercase).map(|c| if c == 'ё' { ('е', true) } else { (c, false) }).collect()
}

/// The entries (pack index and item) of `group`, in `sort`'s order. Equal names or prices keep
/// their order in `entries` (a stable sort).
pub fn pack_view_order(content: &Content, entries: impl IntoIterator<Item = (usize, ItemId)>, group: ItemGroup, sort: PackSort) -> Vec<(usize, ItemId)> {
    let mut out: Vec<(usize, ItemId)> = entries.into_iter().filter(|&(_, item)| group.holds(content.item(item).kind)).collect();
    match sort {
        PackSort::Shown => {}
        PackSort::NameUp => out.sort_by_cached_key(|&(_, item)| alphabet_key(&content.item(item).name)),
        PackSort::NameDown => out.sort_by(|&(_, a), &(_, b)| alphabet_key(&content.item(b).name).cmp(&alphabet_key(&content.item(a).name))),
        PackSort::PriceDown => out.sort_by(|&(_, a), &(_, b)| price_cmp(content, b, a)),
        PackSort::PriceUp => out.sort_by(|&(_, a), &(_, b)| price_cmp(content, a, b)),
    }
    out
}

fn price_cmp(content: &Content, a: ItemId, b: ItemId) -> Ordering {
    custom::price(content, a).cmp(&custom::price(content, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn content() -> Content {
        Content::builtin()
    }

    /// One item of every kind in the builtin content, the first one of each kind.
    fn first_of(c: &Content, kind: ArtefactType) -> Option<ItemId> {
        c.items.iter().find(|d| d.kind == kind).map(|d| ItemId(d.id))
    }

    #[test]
    fn every_kind_belongs_to_exactly_one_non_all_group() {
        for kind in ArtefactType::ALL {
            let holders: Vec<ItemGroup> = ItemGroup::EVERY.into_iter().filter(|&g| g != ItemGroup::All && g.holds(kind)).collect();
            assert_eq!(holders.len(), 1, "{kind:?} is in {holders:?}");
            assert!(ItemGroup::All.holds(kind));
        }
    }

    #[test]
    fn other_holds_potions_and_trade_goods_only() {
        let other: Vec<ArtefactType> = ArtefactType::ALL.into_iter().filter(|&k| ItemGroup::Other.holds(k)).collect();
        assert_eq!(other, vec![ArtefactType::Potion, ArtefactType::Item]);
        assert!(ItemGroup::Weapons.holds(ArtefactType::Staff));
        assert!(!ItemGroup::Weapons.holds(ArtefactType::Armor));
    }

    #[test]
    fn groups_cycle_in_order_both_ways() {
        let mut g = ItemGroup::All;
        for _ in 0..ItemGroup::EVERY.len() {
            g = g.next();
        }
        assert_eq!(g, ItemGroup::All);
        assert_eq!(ItemGroup::All.prev(), ItemGroup::Other);
        assert_eq!(ItemGroup::Other.next(), ItemGroup::All);
        assert_eq!(ItemGroup::Weapons.next().prev(), ItemGroup::Weapons);
    }

    #[test]
    fn sorts_cycle_back_to_the_pack_order() {
        let mut s = PackSort::Shown;
        let seen: Vec<PackSort> = (0..5).map(|_| {
            s = s.next();
            s
        }).collect();
        assert_eq!(seen, vec![PackSort::NameUp, PackSort::NameDown, PackSort::PriceDown, PackSort::PriceUp, PackSort::Shown]);
    }

    #[test]
    fn the_shown_order_keeps_the_pack_and_all_keeps_everything() {
        let c = content();
        let items: Vec<(usize, ItemId)> = c.items.iter().take(6).enumerate().map(|(i, d)| (i * 3, ItemId(d.id))).collect();
        let kept = pack_view_order(&c, items.clone(), ItemGroup::All, PackSort::Shown);
        assert_eq!(kept, items);
    }

    #[test]
    fn grouping_keeps_only_the_group_in_pack_order() {
        let c = content();
        let (Some(ring), Some(helm)) = (first_of(&c, ArtefactType::Ring), first_of(&c, ArtefactType::Helm)) else { return };
        let entries = vec![(4, helm), (1, ring), (7, helm), (2, ring)];
        let rings = pack_view_order(&c, entries.clone(), ItemGroup::Rings, PackSort::Shown);
        assert_eq!(rings, vec![(1, ring), (2, ring)]);
        let helms = pack_view_order(&c, entries, ItemGroup::Helms, PackSort::Shown);
        assert_eq!(helms, vec![(4, helm), (7, helm)]);
    }

    #[test]
    fn names_sort_both_ways_and_stay_stable_for_equal_names() {
        let c = content();
        let Some(a) = c.items.iter().find(|d| d.name.starts_with('А')).or_else(|| c.items.iter().find(|d| d.name.starts_with('A'))) else { return };
        let mut same: Vec<ItemId> = c.items.iter().filter(|d| d.name == a.name).map(|d| ItemId(d.id)).collect();
        same.push(ItemId(a.id));
        let entries: Vec<(usize, ItemId)> = same.iter().copied().enumerate().collect();
        let up = pack_view_order(&c, entries.clone(), ItemGroup::All, PackSort::NameUp);
        let names: Vec<String> = up.iter().map(|&(_, i)| c.item(i).name.to_lowercase()).collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
        let down = pack_view_order(&c, entries.clone(), ItemGroup::All, PackSort::NameDown);
        let back: Vec<usize> = down.iter().map(|e| e.0).collect();
        assert_eq!(back.len(), entries.len());
        // Equal names keep their pack order in both directions.
        let equal: Vec<usize> = down.iter().filter(|&&(_, i)| c.item(i).name == a.name).map(|e| e.0).collect();
        assert!(equal.windows(2).all(|w| w[0] < w[1]));
    }

    #[test]
    fn prices_sort_dearest_and_cheapest_first() {
        let c = content();
        let entries: Vec<(usize, ItemId)> = c.items.iter().take(12).enumerate().map(|(i, d)| (i, ItemId(d.id))).collect();
        let up = pack_view_order(&c, entries.clone(), ItemGroup::All, PackSort::PriceUp);
        assert!(up.windows(2).all(|w| custom::price(&c, w[0].1) <= custom::price(&c, w[1].1)));
        let down = pack_view_order(&c, entries.clone(), ItemGroup::All, PackSort::PriceDown);
        assert!(down.windows(2).all(|w| custom::price(&c, w[0].1) >= custom::price(&c, w[1].1)));
        assert_eq!(down.len(), entries.len());
        // Equal prices keep the pack order.
        let equal = pack_view_order(&c, entries.clone(), ItemGroup::All, PackSort::PriceUp);
        for w in equal.windows(2) {
            if custom::price(&c, w[0].1) == custom::price(&c, w[1].1) {
                assert!(w[0].0 < w[1].0);
            }
        }
    }

    #[test]
    fn an_empty_pack_gives_an_empty_order() {
        let c = content();
        for g in ItemGroup::EVERY {
            for s in [PackSort::Shown, PackSort::NameUp, PackSort::NameDown, PackSort::PriceDown, PackSort::PriceUp] {
                assert!(pack_view_order(&c, std::iter::empty(), g, s).is_empty());
            }
        }
    }

    #[test]
    fn alphabet_key_puts_yo_after_ye_and_ignores_case() {
        // «ё» sorts right after «е» (after «еж» and «ешь», before «ж»).
        assert!(alphabet_key("еж") < alphabet_key("Ёж"));
        assert!(alphabet_key("Ёж") > alphabet_key("ешь"));
        assert!(alphabet_key("Ёж") < alphabet_key("ж"));
        assert_eq!(alphabet_key("AbC"), alphabet_key("abc"));
    }
}
