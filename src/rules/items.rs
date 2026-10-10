//! Items (artefacts): slots, class limits, stat modifiers and potions (mechanics.md 4).
//!
//! A unit wears up to 4 items, only one weapon (melee weapon, bow or staff), never two of the
//! same type. Melee weapons need a warrior, ranged weapons a shooter, staffs a mage. Potions
//! and trade goods are not worn: potions are drunk from the army screen, their healing is
//! instant and their other modifiers last until the end of the next battle.

use crate::i18n::tr;
use super::content::{ArtefactDef, ArtefactType, Bonus, Content, Nature, SpellDef, Stat, StatMods, WageKind};
pub use super::content::{ItemId, Source};
use super::experience::is_percent_stat;
use super::units::{Stats, Unit};

/// Item slots every unit has.
pub const SLOTS: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EquipError {
    NoFreeSlot,
    /// Already wears an item of this type.
    SameType,
    /// Already holds a weapon or staff.
    SecondWeapon,
    /// A melee weapon or shield on a non-warrior, a bow on a non-shooter (or on artillery),
    /// or a staff on a non-mage.
    WrongClass,
    /// A holy item on an undead unit.
    Unholy,
    /// An item of a magic school on a unit of another school (or of none).
    WrongSchool,
    /// The crown on a unit that may not wear it.
    NotAllowed,
    /// Potions and trade goods cannot be worn.
    NotWearable,
    /// The dead cannot hold items.
    Dead,
    NotAPotion,
    PackFull,
    NoSuchItem,
}

/// The potions a unit drank since its army's last battle, as the one block the original
/// keeps (unit +0x44, 0x48fdd0): the `d-` and `p-` of Hits, attacks, defences, magic power,
/// Initiative and Manevres add up; for the protections, regeneration and vampirism an `f-`
/// value replaces what is stored, then the `p-` value adds. Razdor fixes the original's
/// bug: its drink stored the magic power only when a school byte of the block was set, and
/// nothing set it, so a potion's magic power never took effect.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PotionBlock {
    pub add: StatMods,
    pub percent: StatMods,
}

impl PotionBlock {
    pub fn of(content: &Content, potions: &[ItemId]) -> PotionBlock {
        let mut b = PotionBlock::default();
        for d in potions.iter().filter_map(|&p| content.try_item(p)) {
            for (&st, &v) in &d.add {
                if !is_percent_stat(st) && v != 0 {
                    *b.add.entry(st).or_default() += v;
                }
            }
            for st in Stat::ALL {
                let p = d.percent.get(&st).copied().unwrap_or(0);
                if is_percent_stat(st) {
                    if let Some(&f) = d.fixed.get(&st).filter(|&&f| f != 0) {
                        b.percent.insert(st, f);
                    }
                }
                if p != 0 {
                    *b.percent.entry(st).or_default() += p;
                }
            }
        }
        b
    }
}

/// The original's stat rebuild of a unit (0x4908a8, magic-items.md §6), from its level
/// stats: each worn item's positive `f-` replaces its stat (a later slot wins); then the
/// `d-` of the potions, of the items, of the spells; Initiative and Manevres go to
/// hundredths; then the `p-` of the potions, of each item, of each spell, compounding one
/// at a time (`x + x·p/100`, truncated) — on the protections, regeneration and vampirism
/// a `p-` adds points either way; then the drain cuts the maximum HP. Initiative comes back
/// truncated, Manevres rounded half up unless above its level value. Magic power is touched
/// only with a school. A unit with no melee attack, ranged attack or magic power at its
/// level keeps none; protections are held to 0..99, regeneration and vampirism to at most
/// 99, nothing else has a floor. A hero type at 1 HP has Initiative 1. The bonus is the
/// type's, overwritten by each worn item that has one (the last wins); an item's `Magic`
/// does not change the school.
pub fn rebuild_stats(content: &Content, unit: &Unit, spells: &[&SpellDef]) -> Stats {
    let lvl = unit.base_stats(content);
    let mut s = lvl.clone();
    let school = lvl.magic.is_some();
    let touches = |st: Stat| st != Stat::MagicPower || school;
    let worn: Vec<&ArtefactDef> = unit.items.iter().flatten().filter_map(|&i| content.try_item(i)).collect();
    let potions = PotionBlock::of(content, &unit.potions);
    for d in &worn {
        for (&st, &v) in &d.fixed {
            if v > 0 && touches(st) {
                s[st] = v;
            }
        }
    }
    // The `d-` blocks hold no protections, regeneration or vampirism.
    let flat = |s: &mut Stats, mods: &StatMods| {
        for (&st, &v) in mods {
            if !is_percent_stat(st) && touches(st) {
                s[st] = s[st].wrapping_add(v);
            }
        }
    };
    flat(&mut s, &potions.add);
    for d in &worn {
        flat(&mut s, &d.add);
    }
    for sp in spells {
        flat(&mut s, &sp.add);
    }
    s[Stat::Initiative] = s[Stat::Initiative].wrapping_mul(100);
    s[Stat::Manevres] = s[Stat::Manevres].wrapping_mul(100);
    let percent = |s: &mut Stats, mods: &StatMods| {
        for (&st, &p) in mods {
            if is_percent_stat(st) {
                s[st] = s[st].wrapping_add(p);
            } else if touches(st) {
                s[st] = s[st].wrapping_add(s[st].wrapping_mul(p) / 100);
            }
        }
    };
    percent(&mut s, &potions.percent);
    for d in &worn {
        percent(&mut s, &d.percent);
    }
    for sp in spells {
        percent(&mut s, &sp.percent);
    }
    if unit.drain > 0 {
        s[Stat::Hits] -= s[Stat::Hits].wrapping_mul(unit.drain) / 100;
    }
    s[Stat::Initiative] = (s[Stat::Initiative] / 100).max(0);
    let m = s[Stat::Manevres];
    s[Stat::Manevres] = if m > lvl[Stat::Manevres].wrapping_mul(100) { m / 100 } else { (m + 50) / 100 }.max(0);
    for st in [Stat::AttackBlow, Stat::AttackShot, Stat::MagicPower] {
        if lvl[st] == 0 {
            s[st] = 0;
        }
    }
    for st in [Stat::ProtectLife, Stat::ProtectDeath, Stat::ProtectElemental] {
        s[st] = s[st].clamp(0, 99);
    }
    for st in [Stat::Regen, Stat::Vampirizm] {
        s[st] = s[st].min(99);
    }
    // The hero types are GlobalIndex 1–3 (unit type < 3).
    if (1..=3).contains(&unit.def.0) && unit.hp == 1 {
        s[Stat::Initiative] = 1;
    }
    let bonus = worn.iter().filter_map(|d| d.bonus.clone()).next_back().or_else(|| lvl.bonuses.first().cloned());
    s.bonuses = bonus.into_iter().collect();
    s
}

/// Items the undead cannot wear (0x49765c bit set 0x4979b4, by item number).
const HOLY: [u32; 13] = [12, 46, 59, 72, 73, 74, 75, 76, 77, 85, 94, 120, 131];
/// «Королевская корона»: on a unit other than the hero or an army leader, only for these
/// unit types (`GlobalIndex`; the bit set 0x4979a4).
const CROWN: u32 = 154;
const CROWN_TYPES: [u32; 23] = [1, 2, 3, 11, 13, 15, 36, 42, 45, 46, 48, 49, 53, 56, 58, 69, 70, 72, 73, 77, 89, 97, 99];

/// A potion that raises the dead: `f-Hits` of at least 1000.
fn revives(def: &ArtefactDef) -> bool {
    def.kind == ArtefactType::Potion && heal_amount(def) >= 1000
}

/// Puts `item` into item slot `slot` of `unit` and rebuilds it: its HP follows the new
/// maximum ([`Unit::follow_max`]).
pub fn put_on(content: &Content, unit: &mut Unit, slot: usize, item: ItemId) {
    let before = unit.max_hp(content);
    unit.items[slot] = Some(item);
    unit.follow_max(content, before);
}

/// Takes off the item in slot `slot` of `unit` and rebuilds it.
pub fn take_off(content: &Content, unit: &mut Unit, slot: usize) -> Option<ItemId> {
    let before = unit.max_hp(content);
    let item = unit.items.get_mut(slot)?.take();
    unit.follow_max(content, before);
    item
}

/// The original's wear test (0x49765c, magic-items.md §5.3): the slot `item` would go into
/// on `unit` (the lowest empty one), or the first rule it breaks, in the original's order:
/// the crown on a unit other than the hero or a leader needs one of its unit types (Razdor
/// fixes the original's off-by-one bug: the code compared the type's index, GlobalIndex − 1,
/// with the list, so the listed numbers + 1 could wear it); the dead take nothing
/// but a reviving potion; potions and trade goods are not worn; a shield needs melee
/// attack at the unit's level; no holy item on a unit of Nature Undead; a melee weapon needs
/// melee attack, a ranged weapon ranged attack and a type `AttackShot` not above
/// `ShotWeaponRange`, a staff magic power; an item with a school only on a unit of that
/// school; one weapon (staffs included); one item of each type.
pub fn slot_for(content: &Content, unit: &Unit, item: ItemId) -> Result<usize, EquipError> {
    let def = content.try_item(item).ok_or(EquipError::NoSuchItem)?;
    if item.0 == CROWN && unit.wage_kind != WageKind::Leader && !CROWN_TYPES.contains(&unit.def.0) {
        return Err(EquipError::NotAllowed);
    }
    if !unit.alive() && !revives(def) {
        return Err(EquipError::Dead);
    }
    if matches!(def.kind, ArtefactType::Potion | ArtefactType::Item) {
        return Err(EquipError::NotWearable);
    }
    let lvl = unit.base_stats(content);
    if def.kind == ArtefactType::Shield && lvl[Stat::AttackBlow] == 0 {
        return Err(EquipError::WrongClass);
    }
    if lvl.nature == Nature::Undead && HOLY.contains(&item.0) {
        return Err(EquipError::Unholy);
    }
    let blocked = match def.kind {
        ArtefactType::BlowWeapon => lvl[Stat::AttackBlow] == 0,
        ArtefactType::ShotWeapon => lvl[Stat::AttackShot] == 0 || content.unit(unit.def).attack_shot > content.options.shot_weapon_range,
        ArtefactType::Staff => lvl[Stat::MagicPower] == 0,
        _ => false,
    };
    if blocked {
        return Err(EquipError::WrongClass);
    }
    if def.magic.is_some_and(|m| lvl.magic != Some(m)) {
        return Err(EquipError::WrongSchool);
    }
    let worn: Vec<&ArtefactDef> = unit.items.iter().flatten().filter_map(|&i| content.try_item(i)).collect();
    if def.kind.is_weapon() && worn.iter().any(|w| w.kind.is_weapon()) {
        return Err(EquipError::SecondWeapon);
    }
    if worn.iter().any(|w| w.kind == def.kind) {
        return Err(EquipError::SameType);
    }
    unit.items.iter().position(Option::is_none).ok_or(EquipError::NoFreeSlot)
}

/// Healing of a potion (`f-Hits`).
pub fn heal_amount(def: &ArtefactDef) -> i32 {
    def.fixed.get(&Stat::Hits).copied().unwrap_or(0)
}

/// A potion leaves something in the unit's potion block (0x48fdd0 sets its flag only for a
/// non-zero lasting value; magic power is never stored).
fn has_lasting_effect(def: &ArtefactDef) -> bool {
    def.add.iter().any(|(st, &v)| !is_percent_stat(*st) && v != 0)
        || def.percent.iter().any(|(_, &v)| v != 0)
        || def.fixed.iter().any(|(st, &v)| is_percent_stat(*st) && v != 0)
}

/// Drinks potion `item` on `unit` (0x48fdd0, magic-items.md §7.1): a living unit (unhurt
/// counts as full) gains `f-Hits`, dies below 1 and is unhurt at its maximum; a dead unit
/// drinking a potion of `f-Hits` ≥ 1000 comes back with `max × f-Hits / 10000` HP
/// (truncated: it stays dead below 1), any other potion is refused it. The lasting part goes
/// into its potion block ([`PotionBlock`]) until the end of its army's next battle; then it
/// is rebuilt, its HP following its maximum. Returns the HP gained.
pub fn drink(content: &Content, unit: &mut Unit, item: ItemId, now: u64) -> Result<i32, EquipError> {
    let def = content.try_item(item).ok_or(EquipError::NoSuchItem)?;
    if def.kind != ArtefactType::Potion {
        return Err(EquipError::NotAPotion);
    }
    if !unit.alive() && !revives(def) {
        return Err(EquipError::Dead);
    }
    let max = unit.max_hp(content);
    let before = unit.hp;
    let f = heal_amount(def);
    if unit.hp != 0 {
        unit.hp = unit.hp.wrapping_add(f);
        if unit.hp < 1 {
            unit.hp = 0;
            unit.died_at = Some(now);
        } else if unit.hp >= max {
            unit.hp = max;
        }
    }
    if unit.hp == 0 && f > 999 {
        unit.hp = max.wrapping_mul(f) / 10000;
        if unit.hp > 0 {
            unit.died_at = None;
        }
    }
    if has_lasting_effect(def) {
        unit.potions.push(item);
    }
    // The rebuild: a wounded unit's HP follows the maximum; one revived above it is unhurt.
    unit.follow_max(content, max);
    Ok(unit.hp - before)
}

/// Price the market pays before the difficulty factor and a Merchant: `ItemSaleCost`% of the
/// price (`Game::sell_price` has the whole rule).
pub fn sell_price(content: &Content, item: ItemId) -> i32 {
    (content.item(item).cost * content.options.item_sale_cost / 100).max(0)
}

pub fn kind_name(kind: ArtefactType) -> &'static str {
    match kind {
        ArtefactType::BlowWeapon => tr("melee weapon"),
        ArtefactType::ShotWeapon => tr("ranged weapon"),
        ArtefactType::Staff => tr("staff"),
        ArtefactType::Armor => tr("armour"),
        ArtefactType::Helm => tr("helm"),
        ArtefactType::Shield => tr("shield"),
        ArtefactType::Ring => tr("ring"),
        ArtefactType::Amulet => tr("amulet"),
        ArtefactType::Potion => tr("potion"),
        ArtefactType::Item => tr("trade goods"),
    }
}

/// Short label of a stat for descriptions and cards.
pub fn stat_label(s: Stat) -> &'static str {
    match s {
        Stat::Hits => tr("hits"),
        Stat::AttackBlow => tr("attack"),
        Stat::DefenceBlow => tr("defence"),
        Stat::AttackShot => tr("shot"),
        Stat::DefenceShot => tr("shot defence"),
        Stat::MagicPower => tr("magic"),
        Stat::Initiative => tr("initiative"),
        Stat::Manevres => tr("actions"),
        Stat::ProtectLife => tr("life prot."),
        Stat::ProtectDeath => tr("death prot."),
        Stat::ProtectElemental => tr("elem. prot."),
        Stat::Regen => tr("regen %"),
        Stat::Vampirizm => tr("vampirism %"),
    }
}

/// Short name of a unit bonus for descriptions ("Long weapon"); the ini token for tokens
/// the game does not know.
pub fn bonus_name(b: &Bonus) -> String {
    let name = match b {
        Bonus::SpearDefense => tr("Long weapon"),
        Bonus::HorseAtack => tr("Fast attack"),
        Bonus::ArmorIgnore => tr("Piercing blow"),
        Bonus::ArmyMedic => tr("Healer"),
        Bonus::Merchant => tr("Expert trader"),
        Bonus::DeathCurse => tr("Death's curse"),
        Bonus::GodAnger => tr("Wrath of God"),
        Bonus::GodStrike => tr("Anger of God"),
        Bonus::Unvulnerabe => tr("Invulnerable"),
        Bonus::VampirsGist => tr("Dark gift"),
        Bonus::OldVampirsGist => tr("Dark art"),
        Bonus::Evasive => tr("Evasive"),
        Bonus::Ghost => tr("Ghost"),
        Bonus::Artillery => tr("Barrage"),
        Bonus::Garrison => tr("Garrison"),
        Bonus::AddPayment => tr("Quartermaster"),
        Bonus::Poison => tr("Poisoned weapon"),
        Bonus::Dead => tr("Undead"),
        Bonus::FastDead => tr("Fast undead"),
        Bonus::Counterblow => tr("Counterblow"),
        Bonus::FlankStrike => tr("Flank strike"),
        Bonus::Hunger => tr("Hunger"),
        Bonus::Berserk => tr("Berserk"),
        Bonus::Exhaustion => tr("Exhausting magic"),
        Bonus::Drying => tr("Withering magic"),
        Bonus::CtrPoison => tr("Poisonous body"),
        Bonus::Suicide => tr("Last strike"),
        Bonus::Caster => tr("Spellcaster"),
        Bonus::Splash => tr("Sweeping blow"),
        Bonus::Fortify => tr("Entrenchment"),
        Bonus::Dominate => tr("Dominance"),
        Bonus::PoisonS => tr("Strong poison"),
        Bonus::Concentration => tr("Concentration"),
        Bonus::Potent => tr("Potent magic"),
        Bonus::Stun => tr("Stunning blow"),
        Bonus::FirstShot => tr("First shot"),
        Bonus::Bastion => tr("Bastion"),
        Bonus::Flying => tr("Flying"),
        Bonus::Bleed => tr("Bleeding wounds"),
        Bonus::PreventiveStrike => tr("Preventive strike"),
        Bonus::Flock => tr("Strength in numbers"),
        Bonus::ArmorBreaker => tr("Armour breaker"),
        Bonus::NoHeal => tr("Festering wounds"),
        Bonus::FasterAttack => tr("Swift assault"),
        Bonus::PoisonArmorIgnore => tr("Poisoned piercing"),
        Bonus::HoldLine => tr("Hold the line"),
        Bonus::Neutralize => tr("Neutralising blow"),
        Bonus::KillingStrike => tr("Killing strike"),
        Bonus::BloodThrist => tr("Bloodthirst"),
        Bonus::Assault => tr("Assault"),
        Bonus::EternalGift => tr("Lasting gift"),
        Bonus::FateGift => tr("Gift of fate"),
        other => other.token(),
    };
    name.to_string()
}

/// Short summary, e.g. "melee weapon, attack +6, initiative -1".
pub fn describe(content: &Content, item: ItemId) -> String {
    describe_with(content, item, &bonus_name)
}

/// [`describe`] with the bonus named by `bonus` (the UI passes the install's names, the
/// original's `[Army] Bonus<N>`).
pub fn describe_with(content: &Content, item: ItemId, bonus: &dyn Fn(&Bonus) -> String) -> String {
    let d = content.item(item);
    let mut parts = vec![kind_name(d.kind).to_string()];
    for (&st, &v) in &d.fixed {
        parts.push(if d.kind == ArtefactType::Potion && st == Stat::Hits {
            crate::trf!("heals {v}", v)
        } else {
            format!("{} = {v}", stat_label(st))
        });
    }
    for (&st, &v) in &d.add {
        parts.push(format!("{} {v:+}", stat_label(st)));
    }
    for (&st, &v) in &d.percent {
        parts.push(format!("{} {v:+}%", stat_label(st)));
    }
    if let Some(b) = &d.bonus {
        parts.push(bonus(b));
    }
    parts.join(", ")
}

/// How item `item` matches the inventory filter's `query` ([`crate::search::matches`]):
/// by its name, and by the words of its summary ([`describe`]: its type, its stats and its
/// bonus, named by `bonus` as [`describe_with`]) and description.
pub fn filter_match(content: &Content, item: ItemId, query: &str, bonus: &dyn Fn(&Bonus) -> String) -> Option<crate::search::Match> {
    let d = content.item(item);
    crate::search::matches(query, &d.name, &[&describe_with(content, item, bonus), &d.description])
}

/// Where an item dropped on a unit's card was picked up on the army screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ItemFrom {
    /// The pack, at this index.
    Pack(usize),
    /// Squad member `unit`'s item slot `slot`.
    Worn { unit: usize, slot: usize },
}

/// What a drop on a card did ([`Game::give_item`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Given {
    /// The potion was drunk: the HP gained.
    Drunk(i32),
    /// The hero's card: the item went (or stayed) in the pack.
    ToPack,
    /// Worn by the unit, in its lowest free slot.
    Worn,
}

impl super::game::Game {
    /// The army window's drop of an item on squad member `to`'s card (0x4c346c → 0x4979c4,
    /// magic-items.md §5.2): a potion is drunk; on the **hero**'s card any other item goes
    /// to the pack (the hero equips only through the hero window, here the unit panel's
    /// slots); on another unit's the wear test runs and the item goes to its lowest free
    /// slot. A refusal changes nothing (the original leaves the item on the cursor).
    pub fn give_item(&mut self, from: ItemFrom, to: usize) -> Result<Given, EquipError> {
        let item = match from {
            ItemFrom::Pack(i) => self.pack.get(i).copied(),
            ItemFrom::Worn { unit, slot } => self.squad.get(unit).and_then(|u| u.items.get(slot).copied().flatten()),
        }
        .ok_or(EquipError::NoSuchItem)?;
        if to >= self.squad.len() {
            return Err(EquipError::NoSuchItem);
        }
        let c = self.content.clone();
        let kind = c.try_item(item).ok_or(EquipError::NoSuchItem)?.kind;
        match (from, kind) {
            (ItemFrom::Pack(i), ArtefactType::Potion) => self.drink(to, i).map(Given::Drunk),
            (ItemFrom::Pack(_), _) if to == 0 => Ok(Given::ToPack),
            (ItemFrom::Worn { unit, slot }, _) if to == 0 => self.unequip(unit, slot).map(|()| Given::ToPack),
            (ItemFrom::Pack(i), _) => self.equip(to, i).map(|()| Given::Worn),
            (ItemFrom::Worn { unit, slot }, _) => {
                // Taken off first, as the original's held item is: dropped back on its own
                // unit it goes to that unit's lowest free slot.
                let taken = take_off(&c, &mut self.squad[unit], slot).ok_or(EquipError::NoSuchItem)?;
                match slot_for(&c, &self.squad[to], taken) {
                    Ok(free) => {
                        put_on(&c, &mut self.squad[to], free, taken);
                        Ok(Given::Worn)
                    }
                    Err(e) => {
                        put_on(&c, &mut self.squad[unit], slot, taken);
                        Err(e)
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod give_tests {
    use super::*;
    use crate::rules::content::HeroClass;
    use crate::rules::game::Game;
    use crate::rules::world::demo_unit;
    use std::sync::Arc;

    /// A demo game with the hero (squad 0) and a spearman (squad 1).
    fn game() -> Game {
        let mut g = Game::new(Arc::new(Content::builtin()), HeroClass::Knight);
        g.world.armies.clear();
        let taken: Vec<_> = g.squad.iter().map(|u| u.slot).collect();
        let slot = g.content.formation.free_slot(&taken, crate::rules::formation::Row::Front).unwrap();
        let spear = demo_unit(&g.content, "spearman");
        g.squad.truncate(1);
        g.squad.push(Unit::new(&g.content, spear, slot));
        g
    }

    fn item(g: &Game, key: &str) -> ItemId {
        g.content.item_by_key(key).unwrap()
    }

    #[test]
    fn on_the_heros_card_an_item_goes_to_the_pack_and_a_potion_is_drunk() {
        let mut g = game();
        let (shield, potion) = (item(&g, "oak_shield"), item(&g, "heal_potion"));
        // From another unit's slot: into the pack, not onto the hero.
        g.pack = vec![shield];
        g.equip(1, 0).unwrap();
        let at = g.squad[1].items.iter().position(|i| *i == Some(shield)).unwrap();
        assert_eq!(g.give_item(ItemFrom::Worn { unit: 1, slot: at }, 0), Ok(Given::ToPack));
        assert_eq!(g.pack, vec![shield]);
        assert!(!g.squad[1].items.contains(&Some(shield)) && !g.hero().items.contains(&Some(shield)));
        // From the pack: it stays there.
        assert_eq!(g.give_item(ItemFrom::Pack(0), 0), Ok(Given::ToPack));
        assert_eq!(g.pack, vec![shield]);
        assert!(!g.hero().items.contains(&Some(shield)));
        // From the hero's own slot: back to the pack.
        g.equip_at(0, 0, Some(2)).unwrap();
        assert_eq!(g.give_item(ItemFrom::Worn { unit: 0, slot: 2 }, 0), Ok(Given::ToPack));
        assert_eq!((g.pack.clone(), g.hero().items[2]), (vec![shield], None));
        // A full pack refuses: the item stays worn.
        g.equip_at(1, 0, Some(1)).unwrap();
        g.pack = vec![potion; crate::rules::game::PACK_SIZE];
        assert_eq!(g.give_item(ItemFrom::Worn { unit: 1, slot: 1 }, 0), Err(EquipError::PackFull));
        assert_eq!(g.squad[1].items[1], Some(shield));
        // A potion is drunk by the hero.
        g.pack = vec![potion];
        g.squad[0].hp = 10;
        assert!(matches!(g.give_item(ItemFrom::Pack(0), 0), Ok(Given::Drunk(n)) if n > 0));
        assert!(g.pack.is_empty() && g.hero().hp > 10);
    }

    #[test]
    fn on_another_units_card_the_wear_test_runs_and_a_potion_is_drunk() {
        let mut g = game();
        let (shield, sword, potion) = (item(&g, "oak_shield"), item(&g, "short_sword"), item(&g, "heal_potion"));
        g.pack = vec![shield, potion];
        assert_eq!(g.give_item(ItemFrom::Pack(0), 1), Ok(Given::Worn));
        assert_eq!((g.squad[1].items[0], g.pack.clone()), (Some(shield), vec![potion]), "the lowest free slot");
        g.squad[1].hp = 5;
        assert!(matches!(g.give_item(ItemFrom::Pack(0), 1), Ok(Given::Drunk(n)) if n > 0));
        // From the hero's slot to the unit; a second shield is refused and stays put.
        g.pack = vec![sword, item(&g, "oak_shield")];
        g.equip_at(0, 1, Some(3)).unwrap();
        assert_eq!(g.give_item(ItemFrom::Worn { unit: 0, slot: 3 }, 1), Err(EquipError::SameType));
        assert_eq!(g.hero().items[3], Some(shield), "a refusal keeps the item where it was");
        assert_eq!(g.give_item(ItemFrom::Pack(0), 1), Ok(Given::Worn));
        assert!(g.squad[1].items.contains(&Some(sword)));
        // Dropped back on its own unit: its lowest free slot.
        let at = g.squad[1].items.iter().position(|i| *i == Some(sword)).unwrap();
        g.squad[1].items.swap(at, 3);
        assert_eq!(g.give_item(ItemFrom::Worn { unit: 1, slot: 3 }, 1), Ok(Given::Worn));
        assert_eq!(g.squad[1].items[1], Some(sword));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::*;
    use crate::rules::content::{Bonus, MagicDirection, MagicSchool, StatMods, UnitDef, UnitId};
    use crate::rules::content::testkit::spell;
    use crate::rules::formation::{Row, Slot};

    /// The UI names the bonus with the install's text; the summary takes its name as given.
    #[test]
    fn the_summary_takes_the_bonus_name_given() {
        let mut ring = item(1, ArtefactType::Ring);
        ring.bonus = Some(Bonus::Fortify);
        let c = Content::new(vec![warrior(5, 20, 0)], vec![ring], Vec::new(), Default::default(), crate::rules::formation::Formation::WIDE);
        assert_eq!(describe(&c, ItemId(1)), "ring, Entrenchment");
        assert_eq!(describe_with(&c, ItemId(1), &|_| "Install name".to_string()), "ring, Install name");
        assert!(filter_match(&c, ItemId(1), "install", &|_| "Install name".to_string()).is_some());
    }

    #[test]
    fn the_inventory_filter_reads_names_types_and_bonuses() {
        let mut sword = item(1, ArtefactType::BlowWeapon);
        sword.name = "Long Sword".into();
        sword.add = StatMods::from([(Stat::AttackBlow, 3)]);
        sword.bonus = Some(Bonus::ArmorIgnore);
        let mut amulet = item(2, ArtefactType::Amulet);
        amulet.name = "Ёлочный амулет".into();
        amulet.description = "Пахнет хвоей".into();
        let c = Content::new(vec![warrior(5, 20, 0)], vec![sword, amulet], Vec::new(), Default::default(), crate::rules::formation::Formation::WIDE);
        let names = |q: &str| [1, 2].into_iter().filter(|&i| filter_match(&c, ItemId(i), q, &bonus_name).is_some()).collect::<Vec<_>>();
        assert_eq!(names(""), [1, 2], "an empty filter keeps all");
        assert_eq!(names("SWORD"), [1]);
        assert_eq!(names("melee"), [1], "by type");
        assert_eq!(names("piercing"), [1], "by bonus");
        assert_eq!(names("attack"), [1], "by stat");
        assert_eq!(names("елоч"), [2], "Cyrillic, Ё as Е");
        assert_eq!(names("хвоей"), [2], "by description");
        assert_eq!(names("amulet long"), Vec::<u32>::new(), "every word must match");
        assert_eq!(filter_match(&c, ItemId(1), "melee sw", &bonus_name).unwrap().name_range, Some(5..7));
    }

    /// A warrior (type 1 unless given) with the given protections, wearing `items`.
    fn wearing(c: &Content, def: u32, items: &[u32]) -> Unit {
        let mut u = Unit::new(c, UnitId(def), Slot::new(Row::Front, 0));
        for (k, &i) in items.iter().enumerate() {
            u.items[k] = Some(ItemId(i));
        }
        u
    }

    #[test]
    fn a_p_value_on_a_protection_adds_points_either_way_held_to_99() {
        // «Святое писание» (p-ProtectDeath=30) and the like: points, not percent, both ways
        // (0x490864); protections 0..99, regeneration and vampirism at most 99.
        let mut amulet = item(1, ArtefactType::Amulet);
        amulet.percent = StatMods::from([(Stat::ProtectDeath, 30), (Stat::ProtectLife, -50), (Stat::Vampirizm, 25), (Stat::Regen, -30), (Stat::AttackBlow, 25)]);
        let mut ring = item(2, ArtefactType::Ring);
        ring.percent = StatMods::from([(Stat::ProtectDeath, 20)]);
        let base = UnitDef { protect_life: 60, ..warrior(5, 40, 0) };
        let c = content(vec![base, UnitDef { protect_death: 44, ..warrior(6, 40, 0) }, UnitDef { protect_death: 90, ..warrior(7, 40, 0) }], vec![amulet, ring]);
        let s = wearing(&c, 5, &[1]).stats(&c);
        assert_eq!(s[Stat::ProtectDeath], 30);
        assert_eq!(s[Stat::ProtectLife], 10, "60 − 50 points, not halved");
        assert_eq!((s[Stat::Vampirizm], s[Stat::Regen]), (25, -30), "regeneration may stay below 0");
        assert_eq!(s[Stat::AttackBlow], 50, "other stats by percent");
        assert_eq!(wearing(&c, 6, &[2]).stats(&c)[Stat::ProtectDeath], 64, "44 + 20 points");
        assert_eq!(wearing(&c, 7, &[1]).stats(&c)[Stat::ProtectDeath], 99, "held to 99");
    }

    #[test]
    fn spells_d_values_come_before_every_p_value() {
        // 0x4908a8: the potions', items' and spells' d- first, then all the p-.
        let mut ring = item(1, ArtefactType::Ring);
        ring.percent = StatMods::from([(Stat::AttackBlow, 50)]);
        let fury = SpellDef { add: StatMods::from([(Stat::AttackBlow, 10)]), ..spell(1, 0) };
        let c = Content::new(vec![warrior(5, 20, 0)], vec![ring], vec![fury], Default::default(), crate::rules::formation::Formation::WIDE);
        let mut u = wearing(&c, 5, &[1]);
        u.spells[0] = Some(crate::rules::units::SpellSlot { spell: 1, until: 600 });
        assert_eq!(u.stats(&c)[Stat::AttackBlow], 45, "(20 + 10) × 1.5, not 20 × 1.5 + 10");
    }

    #[test]
    fn initiative_and_manevres_are_kept_in_hundredths() {
        // Initiative 5 with two +30% items: 500 → 650 → 845 → 8 (whole numbers would give 7).
        // Manevres 2 with a −30% potion: 140 is not above 200, so (140 + 50) / 100 = 1; with
        // +75%, 350 is above it and truncated to 3.
        let mut a = item(1, ArtefactType::Amulet);
        a.percent = StatMods::from([(Stat::Initiative, 30)]);
        let mut r = item(2, ArtefactType::Ring);
        r.percent = StatMods::from([(Stat::Initiative, 30)]);
        let mut slow = item(3, ArtefactType::Potion);
        slow.percent = StatMods::from([(Stat::Manevres, -30)]);
        let mut fast = item(4, ArtefactType::Potion);
        fast.percent = StatMods::from([(Stat::Manevres, 75)]);
        let c = content(vec![UnitDef { initiative: 5, manevres: 2, ..warrior(5, 20, 0) }], vec![a, r, slow, fast]);
        let mut u = wearing(&c, 5, &[1, 2]);
        assert_eq!(u.stats(&c)[Stat::Initiative], 8);
        u.potions = vec![ItemId(3)];
        assert_eq!(u.stats(&c)[Stat::Manevres], 1, "rounded half up");
        u.potions = vec![ItemId(4)];
        assert_eq!(u.stats(&c)[Stat::Manevres], 3);
    }

    #[test]
    fn no_floors_but_level_zero_attacks_stay_zero_and_a_hero_at_1_hp_has_initiative_1() {
        let mut cursed = item(1, ArtefactType::Ring);
        cursed.add = StatMods::from([(Stat::DefenceBlow, -50), (Stat::AttackBlow, 5), (Stat::Hits, -60)]);
        let c = content(vec![warrior(1, 30, 5), shooter(8, 10)], vec![cursed]);
        let s = wearing(&c, 8, &[1]).stats(&c);
        assert_eq!(s[Stat::DefenceBlow], -50, "not held at 0");
        assert_eq!(s[Stat::AttackBlow], 0, "no melee at its level: an item gives none");
        assert_eq!(s.max_hp(), -10, "no floor on Hits either");
        let mut hero = wearing(&c, 1, &[]);
        assert_eq!(hero.stats(&c)[Stat::Initiative], 10);
        hero.hp = 1;
        assert_eq!(hero.stats(&c)[Stat::Initiative], 1, "a hero type (GlobalIndex 1–3) at 1 HP");
    }

    #[test]
    fn an_items_bonus_overwrites_the_units_and_its_school_changes_nothing() {
        let mut ring = item(1, ArtefactType::Ring);
        ring.bonus = Some(Bonus::ArmorIgnore);
        ring.magic = Some(MagicSchool::Life);
        let merchant = UnitDef { bonus: Some(Bonus::Merchant), ..warrior(5, 20, 0) };
        let c = content(vec![merchant], vec![ring]);
        let s = wearing(&c, 5, &[1]).stats(&c);
        assert_eq!(s.bonuses, vec![Bonus::ArmorIgnore], "one bonus byte: the last item's");
        assert_eq!(s.magic, None, "Magic restricts the wearer, it grants no school");
    }

    fn gear() -> Vec<ArtefactDef> {
        let mut sword = item(1, ArtefactType::BlowWeapon);
        sword.add = StatMods::from([(Stat::AttackBlow, 5)]);
        let mut bow = item(2, ArtefactType::ShotWeapon);
        bow.add = StatMods::from([(Stat::AttackShot, 4)]);
        let staff = item(3, ArtefactType::Staff);
        let mut armour = item(4, ArtefactType::Armor);
        armour.fixed = StatMods::from([(Stat::DefenceBlow, 26)]);
        armour.add = StatMods::from([(Stat::DefenceBlow, 2)]);
        let mut ring = item(5, ArtefactType::Ring);
        ring.percent = StatMods::from([(Stat::AttackBlow, 50), (Stat::DefenceBlow, 10)]);
        ring.bonus = Some(Bonus::ArmorIgnore);
        let mut potion = item(6, ArtefactType::Potion);
        potion.fixed = StatMods::from([(Stat::Hits, 30)]);
        let mut might = item(7, ArtefactType::Potion);
        might.add = StatMods::from([(Stat::AttackBlow, 10)]);
        let armour2 = item(8, ArtefactType::Armor);
        let goods = item(9, ArtefactType::Item);
        vec![sword, bow, staff, armour, ring, potion, might, armour2, goods]
    }

    fn setup() -> (Content, Unit, Unit) {
        let c = content(
            vec![warrior(1, 20, 5), shooter(2, 10), mage(3, 10, MagicSchool::Death, MagicDirection::ToEnemy)],
            gear(),
        );
        let w = Unit::new(&c, UnitId(1), Slot::new(Row::Front, 0));
        let s = Unit::new(&c, UnitId(2), Slot::new(Row::Back, 0));
        (c, w, s)
    }

    #[test]
    fn modifiers_apply_fixed_then_flat_then_percent() {
        let (c, mut w, _) = setup();
        w.items = [Some(ItemId(1)), Some(ItemId(4)), Some(ItemId(5)), None];
        let s = w.stats(&c);
        // attack: (20 + 5) × 1.5; defence: fixed 26, +2, then +10%.
        assert_eq!((s[Stat::AttackBlow], s[Stat::DefenceBlow]), (37, 30));
        assert!(s.has(&Bonus::ArmorIgnore));
    }

    #[test]
    fn item_percentages_compound_one_item_at_a_time() {
        let (c, mut w, _) = setup();
        let mut ring2 = item(10, ArtefactType::Amulet);
        ring2.percent = StatMods::from([(Stat::AttackBlow, 50)]);
        let c = content(c.units.clone(), c.items.iter().cloned().chain([ring2]).collect());
        w.items = [Some(ItemId(5)), Some(ItemId(10)), None, None];
        // 20 → +50% = 30 → +50% = 45 (summed it would be 40).
        assert_eq!(w.stats(&c)[Stat::AttackBlow], 45);
    }

    #[test]
    fn one_weapon_one_per_type_and_class_limits() {
        let (c, mut w, s) = setup();
        assert_eq!(slot_for(&c, &w, ItemId(1)), Ok(0));
        assert_eq!(slot_for(&c, &w, ItemId(2)), Err(EquipError::WrongClass), "bow on a warrior");
        assert_eq!(slot_for(&c, &w, ItemId(3)), Err(EquipError::WrongClass), "staff on a warrior");
        assert_eq!(slot_for(&c, &s, ItemId(1)), Err(EquipError::WrongClass), "sword on a shooter");
        assert_eq!(slot_for(&c, &s, ItemId(2)), Ok(0));
        let m = Unit::new(&c, UnitId(3), Slot::new(Row::Back, 1));
        assert_eq!(slot_for(&c, &m, ItemId(3)), Ok(0));
        w.items[0] = Some(ItemId(1));
        assert_eq!(slot_for(&c, &w, ItemId(1)), Err(EquipError::SecondWeapon));
        w.items[1] = Some(ItemId(4));
        assert_eq!(slot_for(&c, &w, ItemId(8)), Err(EquipError::SameType));
        assert_eq!(slot_for(&c, &w, ItemId(6)), Err(EquipError::NotWearable));
        assert_eq!(slot_for(&c, &w, ItemId(9)), Err(EquipError::NotWearable));
        w.hp = 0;
        assert_eq!(slot_for(&c, &w, ItemId(5)), Err(EquipError::Dead));
    }

    #[test]
    fn the_originals_wear_rules_for_shields_artillery_holy_things_and_the_crown() {
        let undead = UnitDef { nature: Nature::Undead, ..warrior(7, 20, 5) };
        let dead_bonus = UnitDef { bonus: Some(Bonus::Dead), ..warrior(9, 20, 5) };
        let cannon = shooter(6, 70);
        let (shield, bow, holy, crown) = (item(30, ArtefactType::Shield), item(31, ArtefactType::ShotWeapon), item(73, ArtefactType::Amulet), item(154, ArtefactType::Helm));
        let units = vec![warrior(5, 20, 5), shooter(8, 10), cannon, undead, dead_bonus, warrior(1, 30, 5), warrior(2, 30, 5), warrior(11, 30, 5), warrior(12, 30, 5)];
        let c = content(units, vec![shield, bow, holy, crown]);
        let at = Slot::new(Row::Front, 0);
        let u = |id| Unit::new(&c, UnitId(id), at);
        assert_eq!(slot_for(&c, &u(5), ItemId(30)), Ok(0), "a shield for a warrior");
        assert_eq!(slot_for(&c, &u(8), ItemId(30)), Err(EquipError::WrongClass), "not for a shooter");
        assert_eq!(slot_for(&c, &u(8), ItemId(31)), Ok(0));
        assert_eq!(slot_for(&c, &u(6), ItemId(31)), Err(EquipError::WrongClass), "artillery: ranged 70 > ShotWeaponRange 60");
        assert_eq!(slot_for(&c, &u(5), ItemId(73)), Ok(0));
        assert_eq!(slot_for(&c, &u(7), ItemId(73)), Err(EquipError::Unholy), "Nature Undead: «Святое писание» is holy");
        assert_eq!(slot_for(&c, &u(9), ItemId(73)), Ok(0), "the Dead bonus alone is no bar");
        // The crown's list names GlobalIndex 1 and 11 (the original's off-by-one bug compared
        // it with GlobalIndex − 1, so 2 and 12 wore it instead).
        assert_eq!(slot_for(&c, &u(5), ItemId(154)), Err(EquipError::NotAllowed), "type 5 may not wear the crown");
        assert_eq!(slot_for(&c, &u(1), ItemId(154)), Ok(0), "GlobalIndex 1 as an ordinary unit");
        assert_eq!(slot_for(&c, &u(12), ItemId(154)), Err(EquipError::NotAllowed));
        assert_eq!(slot_for(&c, &u(11), ItemId(154)), Ok(0));
        let mut leader = u(5);
        leader.wage_kind = crate::rules::content::WageKind::Leader;
        assert_eq!(slot_for(&c, &leader, ItemId(154)), Ok(0), "the hero or a leader may");
    }

    #[test]
    fn an_item_of_a_school_is_worn_only_by_a_unit_of_that_school() {
        let mut life = item(1, ArtefactType::Staff);
        life.magic = Some(MagicSchool::Life);
        let mut death = item(2, ArtefactType::Staff);
        death.magic = Some(MagicSchool::Death);
        let mut amulet = item(3, ArtefactType::Amulet);
        amulet.magic = Some(MagicSchool::Death);
        let c = content(vec![mage(3, 10, MagicSchool::Death, MagicDirection::ToEnemy), warrior(5, 20, 0)], vec![life, death, amulet]);
        let at = Slot::new(Row::Back, 0);
        let (m, w) = (Unit::new(&c, UnitId(3), at), Unit::new(&c, UnitId(5), at));
        assert_eq!(slot_for(&c, &m, ItemId(1)), Err(EquipError::WrongSchool));
        assert_eq!(slot_for(&c, &m, ItemId(2)), Ok(0));
        assert_eq!(slot_for(&c, &w, ItemId(3)), Err(EquipError::WrongSchool), "a warrior has no school");
        assert_eq!(slot_for(&c, &m, ItemId(3)), Ok(0));
    }

    #[test]
    fn hp_follows_the_maximum_with_a_carried_fraction() {
        // 0x4908a8 step 14: an unhurt unit stays unhurt; a wounded one is scaled in single
        // floats, the fraction carried to the next rebuild.
        let mut belt = item(1, ArtefactType::Amulet);
        belt.add = StatMods::from([(Stat::Hits, 10)]);
        let c = content(vec![UnitDef { hits: 70, ..warrior(5, 20, 0) }], vec![belt]);
        let mut u = wearing(&c, 5, &[]);
        put_on(&c, &mut u, 0, ItemId(1));
        assert_eq!(u.hp, 80, "70/70 wearing +10 is 80/80");
        take_off(&c, &mut u, 0);
        u.hp = 60;
        put_on(&c, &mut u, 0, ItemId(1));
        assert_eq!(u.hp, 68, "80 × 60 / 70 = 68.57");
        assert!((u.carry.0 - 0.571_426).abs() < 1e-5, "{}", u.carry.0);
        take_off(&c, &mut u, 0);
        // 70 × 68.5714264 / 80 = 59.99999809, exactly halfway between two singles: to even.
        assert_eq!(u.hp, 60);
        u.hp = 0;
        put_on(&c, &mut u, 1, ItemId(1));
        assert_eq!(u.hp, 0, "the dead stay dead");
    }

    #[test]
    fn a_strong_potion_raises_the_dead_and_a_bad_one_kills() {
        let mut elixir = item(1, ArtefactType::Potion);
        elixir.fixed = StatMods::from([(Stat::Hits, 1000)]);
        let mut poison = item(2, ArtefactType::Potion);
        poison.fixed = StatMods::from([(Stat::Hits, -100)]);
        let mut heal = item(3, ArtefactType::Potion);
        heal.fixed = StatMods::from([(Stat::Hits, 30)]);
        let c = content(vec![warrior(5, 20, 0), UnitDef { hits: 9, ..warrior(6, 20, 0) }], vec![elixir, poison, heal]);
        let mut u = wearing(&c, 5, &[]);
        u.hp = 0;
        u.died_at = Some(5);
        assert_eq!(slot_for(&c, &u, ItemId(1)), Err(EquipError::NotWearable), "past the dead test, not worn");
        assert_eq!(drink(&c, &mut u, ItemId(3), 9), Err(EquipError::Dead), "no other potion for the dead");
        assert_eq!(drink(&c, &mut u, ItemId(1), 9), Ok(5), "50 × 1000 / 10000");
        assert!(u.alive() && u.died_at.is_none());
        let mut small = wearing(&c, 6, &[]);
        small.hp = 0;
        assert_eq!(drink(&c, &mut small, ItemId(1), 9), Ok(0), "9 × 1000 < 10000: it stays dead");
        assert_eq!(drink(&c, &mut u, ItemId(1), 9), Ok(45), "a living unit is healed by f-Hits");
        assert_eq!(drink(&c, &mut u, ItemId(2), 9), Ok(-50));
        assert_eq!((u.hp, u.died_at), (0, Some(9)), "below 1 it dies");
    }

    #[test]
    fn potions_add_up_in_one_block_and_raise_magic_power() {
        // Two p-Hits +20 potions make one +40% (50 → 70), not 50 → 60 → 72. f-ProtectLife
        // replaces what is stored, then p- adds: 30 + 5, then 10 replaces it.
        let mut vigour = item(1, ArtefactType::Potion);
        vigour.percent = StatMods::from([(Stat::Hits, 20)]);
        let mut ward = item(2, ArtefactType::Potion);
        ward.fixed = StatMods::from([(Stat::ProtectLife, 30)]);
        ward.percent = StatMods::from([(Stat::ProtectLife, 5)]);
        let mut ward2 = item(3, ArtefactType::Potion);
        ward2.fixed = StatMods::from([(Stat::ProtectLife, 10)]);
        let mut mind = item(4, ArtefactType::Potion);
        mind.add = StatMods::from([(Stat::MagicPower, 5)]);
        mind.percent = StatMods::from([(Stat::MagicPower, 50)]);
        let c = content(vec![warrior(5, 20, 0), mage(3, 10, MagicSchool::Life, MagicDirection::ToAlly)], vec![vigour, ward, ward2, mind]);
        let mut u = wearing(&c, 5, &[]);
        drink(&c, &mut u, ItemId(1), 0).unwrap();
        drink(&c, &mut u, ItemId(1), 0).unwrap();
        assert_eq!((u.stats(&c).max_hp(), u.hp), (70, 70));
        drink(&c, &mut u, ItemId(2), 0).unwrap();
        assert_eq!(u.stats(&c)[Stat::ProtectLife], 35);
        drink(&c, &mut u, ItemId(3), 0).unwrap();
        assert_eq!(u.stats(&c)[Stat::ProtectLife], 10);
        let mut m = Unit::new(&c, UnitId(3), Slot::new(Row::Back, 0));
        drink(&c, &mut m, ItemId(4), 0).unwrap();
        // (10 + 5) × 150% (the original's bug never let a potion touch the magic power).
        assert_eq!(m.potions, vec![ItemId(4)]);
        assert_eq!(m.stats(&c)[Stat::MagicPower], 22);
    }

    #[test]
    fn full_slots_are_reported() {
        let (c, mut w, _) = setup();
        let c = content(c.units.clone(), c.items.iter().cloned().chain([item(10, ArtefactType::Amulet), item(11, ArtefactType::Helm)]).collect());
        w.items = [Some(ItemId(1)), Some(ItemId(4)), Some(ItemId(5)), Some(ItemId(10))];
        assert_eq!(slot_for(&c, &w, ItemId(11)), Err(EquipError::NoFreeSlot));
    }

    #[test]
    fn potions_heal_now_and_buff_until_the_next_battle_ends() {
        let (c, mut w, _) = setup();
        w.hp = 10;
        assert_eq!(drink(&c, &mut w, ItemId(6), 0), Ok(30));
        assert!(w.potions.is_empty(), "healing only: nothing lasts");
        assert_eq!(drink(&c, &mut w, ItemId(6), 0), Ok(10), "capped at max HP");
        assert_eq!(drink(&c, &mut w, ItemId(7), 0), Ok(0));
        assert_eq!(w.stats(&c)[Stat::AttackBlow], 30);
        assert_eq!(drink(&c, &mut w, ItemId(1), 0), Err(EquipError::NotAPotion));
    }

    #[test]
    fn sale_price_and_descriptions() {
        let (c, _, _) = setup();
        assert_eq!(sell_price(&c, ItemId(1)), 25, "ItemSaleCost 25%");
        assert_eq!(describe(&c, ItemId(4)), "armour, defence = 26, defence +2");
        assert_eq!(describe(&c, ItemId(6)), "potion, heals 30");
        assert_eq!(describe(&c, ItemId(5)), "ring, attack +50%, defence +10%, Piercing blow");
    }
}
