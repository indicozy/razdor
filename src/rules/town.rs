//! Building services: the tabs of a town, castle, fort, village or church window and what
//! each tab does (mechanics.md 1.6, 3.1, 4, 5.3): hiring, paid healing and resurrection,
//! garrisons, the market, the sanctuary (spell shop) and village tribute.
//!
//! Only the building the hero stands in can be used. There is no attitude test anywhere
//! (economy.md §7): an ill-disposed building trades, hires and heals, dearer at its market
//! (see `economy::relation_price`).

use super::clock::MINUTES_PER_DAY;
use super::content::{Content, SpellDef};
use super::formation::Slot;
use super::game::{Event, Game, Price, SPELL_BOOK_SIZE};
use super::units::Stats;
use super::world::{EventId, Location, LocationKind, Stationed};

/// A tab of a building window. "Exit" is the UI's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    /// Description, quests and rumours.
    MainHall,
    /// Hire, heal and (towns, churches) resurrect.
    Barracks,
    /// Leave or take troops (the player's castles and forts).
    Garrison,
    /// Buy and sell items.
    Market,
    /// Learn spells.
    Sanctuary,
    /// A village's tribute and its alternatives.
    Tribute,
    /// Not a tab: a shipyard opens the original's small ship window instead of the building
    /// window (0x4bbc84, 0x4d3ec0), with "Нанять корабль" (`rules::ships`) and "Отмена".
    Shipyard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceError {
    /// This building does not offer it (or is hostile, or the hero is on the road).
    NotHere,
    /// Not enough gold, or mana for units paid in mana.
    CannotAfford,
    NotWounded,
    NotDead,
    SquadFull,
    GarrisonFull,
    /// The hero cannot be left, dismissed or healed away from his army.
    Hero,
    /// A named character cannot be left in a garrison.
    Named,
    /// An unpaid garrison unit costs this much gold to take back.
    Unpaid(i32),
    AlreadyKnown,
    BookFull,
    NoSuchUnit,
}

/// The tabs location `l` shows the player, in the original's order (economy.md §7,
/// 0x4bbc84), with no attitude test:
/// - every building with a window: the main hall;
/// - the hire tab ([`Location::hires`]), where healing and (towns, churches) resurrection
///   live too;
/// - the player's castles and forts: the garrison;
/// - a building with goods (only towns, markets and churches keep them): the market; with
///   spells: the sanctuary;
/// - villages: the tribute.
///
/// A shipyard has no building window: only [`Tab::Shipyard`], the original's ship window,
/// whatever its attitude, owner, barracks, goods or spells (0x4bbc84 opens it for every
/// type-9 building and tests neither attitude nor owner). Bridges, the obelisk, the demo's
/// camps and a garrison still to be beaten have none.
pub fn tabs(l: &Location, c: &Content) -> Vec<Tab> {
    if l.kind.is_bridge() || matches!(l.kind, LocationKind::Obelisk | LocationKind::Camp) || l.defended() {
        return Vec::new();
    }
    if l.kind == LocationKind::Shipyard {
        return vec![Tab::Shipyard];
    }
    let mut tabs = vec![Tab::MainHall];
    if l.hires(c) {
        tabs.push(Tab::Barracks);
    }
    if l.takes_garrison() {
        tabs.push(Tab::Garrison);
    }
    if l.shop.is_some() {
        tabs.push(Tab::Market);
    }
    if !l.spells.is_empty() {
        tabs.push(Tab::Sanctuary);
    }
    if l.kind == LocationKind::Village {
        tabs.push(Tab::Tribute);
    }
    tabs
}

/// The tab a building window opens on: a village's tribute, a shipyard's ship window, else
/// the main hall.
pub fn first_tab(l: &Location, c: &Content) -> Option<Tab> {
    let t = tabs(l, c);
    t.iter().copied().find(|&t| matches!(t, Tab::Tribute | Tab::Shipyard)).or_else(|| t.first().copied())
}

impl Game {
    fn here(&self) -> Option<&Location> {
        self.location.map(|l| &self.world.locations[l])
    }

    /// Tabs of the building the hero stands in: none in a shipyard while he is at sea (the
    /// original opens nothing there, 0x4bbc84).
    pub fn tabs_here(&self) -> Vec<Tab> {
        match self.here() {
            Some(l) if l.kind == LocationKind::Shipyard && self.aboard() => Vec::new(),
            Some(l) => tabs(l, &self.content),
            None => Vec::new(),
        }
    }

    /// The window building `l` opens for the hero, by its first tab ([`first_tab`]): none
    /// for a shipyard while he is at sea (0x4bbc84 tests the at-sea flag there and opens
    /// nothing; on land it opens the ship window).
    pub fn window_at(&self, l: usize) -> Option<Tab> {
        let loc = &self.world.locations[l];
        if loc.kind == LocationKind::Shipyard && self.aboard() {
            return None;
        }
        first_tab(loc, &self.content)
    }

    fn offers(&self, tab: Tab) -> bool {
        self.tabs_here().contains(&tab)
    }

    /// Quests and rumours listed in the main hall here (see [`World::local_events`]).
    ///
    /// [`World::local_events`]: super::world::World::local_events
    pub fn local_events(&self) -> Vec<EventId> {
        self.location.map_or_else(Vec::new, |l| self.world.local_events(l))
    }

    /// Paid healing is possible here: wherever the hire tab is shown.
    pub fn heals_here(&self) -> bool {
        self.offers(Tab::Barracks)
    }

    /// Resurrection is possible here: the hire tab of a town or church.
    pub fn resurrects_here(&self) -> bool {
        self.offers(Tab::Barracks) && self.here().is_some_and(|l| matches!(l.kind, LocationKind::Town | LocationKind::Church))
    }

    /// Whether the heal or raise button of squad member `i` is enabled: its price against
    /// the currency it is paid in. Razdor fixes the original's bug (0xc260c5): its heal
    /// button compared even an elemental's price, paid in mana, with the gold.
    pub fn can_pay_service(&self, price: Price) -> bool {
        match price.currency {
            super::game::Currency::Gold => self.gold >= price.amount,
            super::game::Currency::Mana => self.mana >= price.amount,
        }
    }

    /// Pays a heal or a resurrection in `price`'s currency, clamped at 0 (0x4ab150).
    fn pay_service(&mut self, price: Price) {
        match price.currency {
            super::game::Currency::Gold => self.gold = (self.gold - price.amount).max(0),
            super::game::Currency::Mana => self.mana = (self.mana - price.amount).max(0),
        }
    }

    /// Heals squad member `i` to full HP for [`Game::heal_price`], paid at once. No game
    /// time passes (the exe uses `HealingTime` only for AI armies). The returned events are
    /// always empty; the signature keeps room for a timed service.
    pub fn heal(&mut self, i: usize) -> Result<Vec<Event>, ServiceError> {
        if !self.heals_here() {
            return Err(ServiceError::NotHere);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if !u.alive() {
            return Err(ServiceError::NotWounded);
        }
        let price = self.heal_price(i).ok_or(ServiceError::NotWounded)?;
        if !self.can_pay_service(price) {
            return Err(ServiceError::CannotAfford);
        }
        self.pay_service(price);
        let c = self.content.clone();
        self.squad[i].heal_full(&c);
        self.scan_on_close = true;
        Ok(Vec::new())
    }

    /// Raises the corpse of squad member `i` in a town or church for
    /// [`Game::resurrect_price`], with no time limit (`MaxTimeResurection` is the AI's). It
    /// comes back at once with full HP, marked paid (its last pay unchanged).
    pub fn resurrect(&mut self, i: usize) -> Result<Vec<Event>, ServiceError> {
        if !self.resurrects_here() {
            return Err(ServiceError::NotHere);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if u.alive() {
            return Err(ServiceError::NotDead);
        }
        let price = self.resurrect_price(i).ok_or(ServiceError::NotDead)?;
        if !self.can_pay_service(price) {
            return Err(ServiceError::CannotAfford);
        }
        self.pay_service(price);
        let c = self.content.clone();
        let u = &mut self.squad[i];
        u.died_at = None;
        u.unpaid = false;
        u.heal_full(&c);
        self.scan_on_close = true;
        Ok(Vec::new())
    }

    /// The player's troops in the garrison here.
    pub fn garrison_here(&self) -> &[Stationed] {
        match self.here() {
            Some(l) if l.takes_garrison() => &l.stationed,
            _ => &[],
        }
    }

    /// Opens the garrison tab here (0x4ba854 tab 2): every unit of the hero's army is
    /// stamped with now, and the paid mark of every garrison unit is set, the only place it
    /// is: paid when last paid less than 1441 minutes ago, or when 1440 minutes or more
    /// passed since its stamp (units placed at map load have none, so they are always paid).
    /// A unit put into the garrison during the visit keeps the mark it had in the army.
    pub fn open_garrison(&mut self) {
        let Some(l) = self.location.filter(|_| self.offers(Tab::Garrison)) else { return };
        let now = self.clock.total_minutes() as u64;
        for u in self.squad.iter_mut() {
            u.seen = now;
        }
        for s in self.world.locations[l].stationed.iter_mut() {
            let u = &mut s.unit;
            u.unpaid = !(now.saturating_sub(u.last_paid) < MINUTES_PER_DAY + 1 || now.saturating_sub(u.seen) >= MINUTES_PER_DAY);
        }
    }

    /// The garrison here, if the tab is offered, and its free cell for a unit of `row` that
    /// wants `cell`: that cell when given and free, else its own cell or the first free one.
    fn free_cell(units: &[Slot], f: super::formation::Formation, want: Slot, row: super::formation::Row) -> Option<Slot> {
        if units.len() >= f.capacity() {
            return None;
        }
        if units.contains(&want) { f.free_slot(units, row) } else { Some(want) }
    }

    /// Leaves squad member `i` in the garrison of the castle or fort here, at `cell` (an empty
    /// one; `None`: its own or the first free). The hero and named units are refused
    /// (0x4c6f50); a corpse can be left. The whole record moves, paid mark and last pay
    /// included; a garrison is never paid, and it heals `GarrisonAutoHeal`% every midnight.
    pub fn leave_in_garrison(&mut self, i: usize, cell: Option<Slot>) -> Result<(), ServiceError> {
        if !self.offers(Tab::Garrison) {
            return Err(ServiceError::NotHere);
        }
        if i == 0 {
            return Err(ServiceError::Hero);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if u.named > 0 {
            return Err(ServiceError::Named);
        }
        let l = self.location.ok_or(ServiceError::NotHere)?;
        let taken: Vec<Slot> = self.world.locations[l].stationed.iter().map(|s| s.unit.slot).collect();
        let row = Stats::of_level(&self.content, u.def, u.level).preferred_row();
        let slot = Self::free_cell(&taken, self.content.formation, cell.unwrap_or(u.slot), row).ok_or(ServiceError::GarrisonFull)?;
        let mut unit = self.squad.remove(i);
        unit.slot = slot;
        self.world.locations[l].stationed.push(Stationed { unit });
        Ok(())
    }

    /// What taking garrison unit `j` into an empty cell of the army costs: nothing when it
    /// is paid, else one day's kind-1 wage of its type (0x4acff4).
    pub fn garrison_price(&self, j: usize) -> Option<i32> {
        let u = &self.garrison_here().get(j)?.unit;
        Some(if u.unpaid { self.content.wage(u.def) } else { 0 })
    }

    /// Takes garrison unit `j` of the castle or fort here into an empty cell of the army
    /// (`None`: its own or the first free; the army must have room). A paid unit comes free,
    /// its last pay now; an unpaid one only with `pay`, for [`Game::garrison_price`], asked
    /// only when that is strictly below the gold, and it comes paid with its last pay now.
    pub fn take_from_garrison(&mut self, j: usize, cell: Option<Slot>, pay: bool) -> Result<(), ServiceError> {
        if !self.offers(Tab::Garrison) {
            return Err(ServiceError::NotHere);
        }
        let l = self.location.ok_or(ServiceError::NotHere)?;
        let price = self.garrison_price(j).ok_or(ServiceError::NoSuchUnit)?;
        if price > 0 && !pay {
            return Err(ServiceError::Unpaid(price));
        }
        if price > 0 && price >= self.gold {
            return Err(ServiceError::CannotAfford);
        }
        if self.squad.len() >= self.max_squad() {
            return Err(ServiceError::SquadFull);
        }
        let s = &self.world.locations[l].stationed[j];
        let taken: Vec<Slot> = self.squad.iter().map(|u| u.slot).collect();
        let row = Stats::of_level(&self.content, s.unit.def, s.unit.level).preferred_row();
        let slot = Self::free_cell(&taken, self.content.formation, cell.unwrap_or(s.unit.slot), row).ok_or(ServiceError::SquadFull)?;
        let mut unit = self.world.locations[l].stationed.remove(j).unit;
        unit.slot = slot;
        if price > 0 {
            self.gold -= price;
            unit.unpaid = false;
        }
        unit.last_paid = self.clock.total_minutes() as u64;
        self.squad.push(unit);
        Ok(())
    }

    /// Swaps squad member `i` with garrison unit `j` (0x4c653c, 0x4c6f50): the two records
    /// change places, each taking the other's cell, with no price and no question, so an
    /// unpaid garrison unit comes in unpaid; it is also the only way into a full army. The
    /// army's unit must not be the hero or a named unit. Only when the garrison unit was
    /// selected first does it get its last pay now, and only if it is paid (the original's
    /// two click handlers differ).
    pub fn swap_with_garrison(&mut self, i: usize, j: usize, garrison_first: bool) -> Result<(), ServiceError> {
        if !self.offers(Tab::Garrison) {
            return Err(ServiceError::NotHere);
        }
        let l = self.location.ok_or(ServiceError::NotHere)?;
        if i == 0 {
            return Err(ServiceError::Hero);
        }
        let u = self.squad.get(i).ok_or(ServiceError::NoSuchUnit)?;
        if u.named > 0 {
            return Err(ServiceError::Named);
        }
        let g = &mut self.world.locations[l].stationed;
        if j >= g.len() {
            return Err(ServiceError::NoSuchUnit);
        }
        let (army_cell, guard_cell) = (self.squad[i].slot, g[j].unit.slot);
        std::mem::swap(&mut self.squad[i], &mut g[j].unit);
        self.squad[i].slot = army_cell;
        g[j].unit.slot = guard_cell;
        if garrison_first && !self.squad[i].unpaid {
            self.squad[i].last_paid = self.clock.total_minutes() as u64;
        }
        Ok(())
    }

    /// Moves garrison unit `j` to `cell` of the garrison here, swapping with a unit there
    /// (only the formation changes).
    pub fn move_guard(&mut self, j: usize, cell: Slot) -> bool {
        let Some(l) = self.location.filter(|_| self.offers(Tab::Garrison)) else { return false };
        let g = &mut self.world.locations[l].stationed;
        if j >= g.len() || !self.content.formation.slots().any(|s| s == cell) {
            return false;
        }
        let from = g[j].unit.slot;
        if let Some(other) = g.iter().position(|s| s.unit.slot == cell) {
            g[other].unit.slot = from;
        }
        g[j].unit.slot = cell;
        true
    }

    /// Spells the sanctuary here teaches.
    pub fn spells_here(&self) -> Vec<&SpellDef> {
        if !self.offers(Tab::Sanctuary) {
            return Vec::new();
        }
        let l = self.here().expect("offers a sanctuary");
        l.spells.iter().filter_map(|&id| self.content.spells.iter().find(|s| s.id == id as u32)).collect()
    }

    pub fn knows_spell(&self, id: u32) -> bool {
        self.spells.iter().any(|&s| s as u32 == id)
    }

    /// Learns spell `id` in the sanctuary here for its `CostGold`; it goes into the hero's
    /// book (which holds [`SPELL_BOOK_SIZE`] spells), to be cast on the map (`rules::magic`).
    pub fn learn_spell(&mut self, id: u32) -> Result<(), ServiceError> {
        let spell = self.spells_here().into_iter().find(|s| s.id == id).ok_or(ServiceError::NotHere)?;
        // Exactly `CostGold`, a negative one too (it then pays the hero).
        let price = Price::gold(spell.cost_gold);
        // The shop's tests in its order (0x4ba078): known, then gold, then a full book of 15.
        // Razdor fixes the original's bug: it refused only a book of exactly 15, so a book
        // an event pushed past 15 bought on.
        if self.knows_spell(id) {
            return Err(ServiceError::AlreadyKnown);
        }
        if !self.can_afford(price) {
            return Err(ServiceError::CannotAfford);
        }
        if self.spells.len() >= SPELL_BOOK_SIZE {
            return Err(ServiceError::BookFull);
        }
        self.spend(price);
        self.spells.push(id as u8);
        Ok(())
    }

    /// Sends squad member `i` away (the army screen's "dismiss"), or buries a corpse: the
    /// same action (0x4b1778). Only the hero is refused; named and event units go too. No
    /// refund, no cost, and its worn items are lost with it: the original's Army_RemoveUnit
    /// moves nothing to the pack.
    pub fn dismiss(&mut self, i: usize) -> Result<(), ServiceError> {
        if i == 0 {
            return Err(ServiceError::Hero);
        }
        if i >= self.squad.len() {
            return Err(ServiceError::NoSuchUnit);
        }
        self.squad.remove(i);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::dt::dtm::{BuildingType, RecruitSlot, Scenario};
    use crate::rules::game::Tribute;
    use crate::rules::battle::{Battle, Team};
    use crate::rules::content::testkit as ck;
    use crate::rules::content::{ArtefactType, Bonus, Content, HeroClass, ItemId, MagicDirection, MagicSchool, Nature, UnitDef, UnitId};
    use crate::rules::game::{BattleResult, Currency, DayReport, Foe, Game};
    use crate::rules::world::testkit::{army, building, hero, scenario, troop};

    /// Units: 1–3 heroes; 4 militia (cost 50); 5 archer (cost 90); 6 priest (cost 60,
    /// Surrender 20); 7 merchant (cost 100, Merchant); 8 golem (cost 80, Elemental); 9 bandit
    /// (cost 55, Rogue); 10 quartermaster (cost 100, AddPayment); 11 medic (cost 40,
    /// ArmyMedic). Items: 20 sword (100), 21 ring (40), 22 potion (60), 23 ring (300), 24 amulet
    /// (1000), 25 trinket (1), 135 furs (1000, goods). Spells 1 (200 gold, +30 hits), 2 (500
    /// gold), 3 (a 4-hour blessing, d-DefenceBlow +2).
    fn content() -> Content {
        let mut units = vec![
            ck::warrior(1, 20, 5),
            ck::mage(2, 10, MagicSchool::Elemental, MagicDirection::ToEnemy),
            ck::shooter(3, 15),
            UnitDef { cost: 50, hits: 40, ..ck::warrior(4, 10, 2) },
            UnitDef { cost: 90, ..ck::shooter(5, 8) },
            UnitDef { cost: 60, surrender: 20, ..ck::mage(6, 8, MagicSchool::Life, MagicDirection::ToAlly) },
            UnitDef { cost: 100, bonus: Some(Bonus::Merchant), ..ck::warrior(7, 5, 1) },
            UnitDef { cost: 80, nature: Nature::Elemental, ..ck::warrior(8, 12, 4) },
            UnitDef { cost: 55, nature: Nature::Rogue, ..ck::warrior(9, 9, 1) },
            UnitDef { cost: 100, bonus: Some(Bonus::AddPayment), ..ck::warrior(10, 5, 1) },
            UnitDef { cost: 40, bonus: Some(Bonus::ArmyMedic), ..ck::warrior(11, 5, 1) },
        ];
        for u in &mut units[..3] {
            u.cost = 0;
        }
        let item = |id, kind, cost| crate::rules::content::ArtefactDef { cost, ..ck::item(id, kind) };
        let items = vec![
            item(20, ArtefactType::BlowWeapon, 100),
            item(21, ArtefactType::Ring, 40),
            item(22, ArtefactType::Potion, 60),
            item(23, ArtefactType::Ring, 300),
            item(24, ArtefactType::Amulet, 1000),
            item(25, ArtefactType::Ring, 1),
            item(135, ArtefactType::Item, 1000),
        ];
        let heal = crate::rules::content::SpellDef { delta_fixed_hits: Some(30), ..ck::spell(1, 200) };
        let bless = crate::rules::content::SpellDef {
            delta_fixed_hits: None,
            time_work: Some(4),
            add: crate::rules::content::StatMods::from([(crate::rules::content::Stat::DefenceBlow, 2)]),
            ..ck::spell(3, 100)
        };
        let spells = vec![heal, ck::spell(2, 500), bless];
        Content::new(units, items, spells, Default::default(), crate::rules::formation::Formation::WIDE)
    }

    /// A 24×8 grass map; the knight starts at (2, 2) with 1000 gold and two militia.
    fn map() -> Scenario {
        let mut s = scenario(24, 8);
        s.header.heroes[0] = hero(2, 2, 1000, &[troop(4, 0, 2)]);
        s
    }

    fn start(s: &Scenario) -> Game {
        Game::from_scenario(Arc::new(content()), s, HeroClass::Knight)
    }

    fn town(kind: BuildingType, x: u16, y: u16, attitude: i8) -> crate::dt::dtm::Building {
        let mut b = building(kind, x, y, (1, 1));
        b.relations = [attitude, 0, 0, 0];
        b.faction = if attitude < 0 { 4 } else { 3 };
        b
    }

    /// [`town`] with a barracks slot (militia, none left): the hire tab, where healing is.
    fn hall(kind: BuildingType, x: u16, y: u16, attitude: i8) -> crate::dt::dtm::Building {
        let mut b = town(kind, x, y, attitude);
        b.barracks[0] = RecruitSlot { unit: 4, start_count: 0, max_count: 0 };
        b
    }

    /// A game standing in building 0 of `s`.
    fn inside(s: &Scenario) -> Game {
        let mut g = start(s);
        g.location = Some(0);
        g
    }

    #[test]
    fn tabs_follow_the_building_type() {
        let mut s = map();
        let mut t = town(BuildingType::Town, 5, 5, 1);
        t.barracks[0] = RecruitSlot { unit: 4, start_count: 2, max_count: 4 };
        t.random_artifacts_for_sale = 3;
        t.spells_for_sale[0] = 1;
        let mut castle = town(BuildingType::Castle, 8, 5, 3);
        castle.faction = 1;
        castle.owner_army = 0; // the owner byte makes it the player's
        let mut fort = town(BuildingType::Fort, 11, 5, -2);
        fort.barracks[2] = RecruitSlot { unit: 5, start_count: 1, max_count: 1 };
        fort.random_artifacts_for_sale = 4; // wiped at load: only towns, markets, churches sell
        let village = town(BuildingType::Village, 14, 5, 1);
        let mut church = town(BuildingType::Church, 17, 5, 1);
        church.spells_for_sale[0] = 2;
        let mut tavern = town(BuildingType::Tavern, 20, 5, 1);
        tavern.barracks[0] = RecruitSlot { unit: 4, start_count: 1, max_count: 1 };
        tavern.barracks[1] = RecruitSlot { unit: 9, start_count: 1, max_count: 1 };
        let mut market = town(BuildingType::Market, 22, 5, 1);
        market.random_artifacts_for_sale = 5;
        let mut hostile = town(BuildingType::Town, 5, 7, -2);
        hostile.random_artifacts_for_sale = 3;
        hostile.garrison[0] = troop(4, 0, 1);
        let bridge = town(BuildingType::StoneBridge, 3, 7, 1);
        let mut altar = town(BuildingType::Altar, 7, 7, 1);
        altar.barracks[0] = RecruitSlot { unit: 9, start_count: 1, max_count: 1 };
        altar.recruit_all_types = 1;
        altar.random_artifacts_for_sale = 2;
        let obelisk = town(BuildingType::Obelisk, 9, 7, 1);
        let mut yard = town(BuildingType::Shipyard, 11, 7, -2);
        yard.barracks[0] = RecruitSlot { unit: 9, start_count: 1, max_count: 1 };
        yard.recruit_all_types = 1;
        s.buildings = vec![t, castle, fort, village, church, tavern, market, hostile, bridge, altar, obelisk, yard];
        let g = start(&s);
        let c = g.content.clone();
        let tabs: Vec<Vec<Tab>> = g.world.locations.iter().map(|l| tabs(l, &c)).collect();
        use Tab::*;
        assert_eq!(tabs[0], [MainHall, Barracks, Market, Sanctuary]);
        assert_eq!(tabs[1], [MainHall, Garrison], "the player's castle: no barracks slot, no hire tab");
        assert_eq!(tabs[2], [MainHall, Barracks], "an ill-disposed fort still hires: no attitude test");
        assert_eq!(tabs[3], [MainHall, Tribute]);
        assert_eq!(tabs[4], [MainHall, Sanctuary]);
        assert_eq!(tabs[5], [MainHall], "a rogue in the barracks closes the hire tab");
        assert_eq!(tabs[6], [MainHall, Market]);
        assert_eq!(tabs[7], [MainHall, Market], "an ill-disposed town trades (its garrison is AI-only)");
        assert!(tabs[8].is_empty());
        assert_eq!(tabs[9], [MainHall, Barracks], "the all-types byte opens it; an altar keeps no goods");
        assert!(tabs[10].is_empty(), "the obelisk has no window");
        assert!(g.world.locations[11].hostile());
        assert_eq!(tabs[11], [Shipyard], "an ill-disposed shipyard opens its ship window, no attitude test, and nothing else even with a barracks slot (0x4bbc84)");
    }

    #[test]
    fn heal_costs_a_share_of_the_unit_cost_over_f_and_no_time() {
        let mut s = map();
        s.buildings = vec![hall(BuildingType::Town, 2, 2, 1)];
        let mut g = inside(&s);
        assert_eq!(g.difficulty(), 120, "no impossible difficulty");
        assert_eq!(g.heal_price(1), None, "unhurt");
        g.squad[1].hp = 10; // 30 of 40 missing: 30/40 × 50 × 50% × 100/120 = 15.6 → 16
        assert_eq!(g.heal_price(1), Some(Price::gold(16)));
        g.squad[2].hp = 39; // 0.52 → 1, and never below 1
        assert_eq!(g.heal_price(2), Some(Price::gold(1)));
        let (gold, t) = (g.gold, g.clock.total_minutes());
        assert_eq!(g.heal(1), Ok(Vec::new()));
        assert_eq!((g.squad[1].hp, g.gold), (40, gold - 16));
        assert_eq!(g.clock.total_minutes(), t, "no game time passes");
        assert_eq!(g.heal(1), Err(ServiceError::NotWounded));
        g.gold = 0;
        assert_eq!(g.heal(2), Err(ServiceError::CannotAfford));
        g.location = None;
        assert_eq!(g.heal(2), Err(ServiceError::NotHere));
    }

    #[test]
    fn healing_and_resurrection_take_no_time_even_before_noon() {
        let mut s = map();
        s.buildings = vec![hall(BuildingType::Church, 2, 2, 1)];
        let mut g = inside(&s);
        g.pass_time(2.5 * 60.0, &mut Vec::new()); // 09:00 -> 11:30
        g.squad[1].hp = 1;
        let t = g.clock.total_minutes();
        let events = g.heal(1).unwrap();
        assert!(events.is_empty() && g.clock.total_minutes() == t, "{events:?}");
    }

    #[test]
    fn no_healing_without_a_hire_tab() {
        let mut s = map();
        s.buildings = vec![town(BuildingType::Tavern, 6, 2, 1), town(BuildingType::Village, 9, 2, 1), hall(BuildingType::Church, 12, 2, -2)];
        let mut g = inside(&s);
        g.squad[1].hp = 1;
        for l in 0..2 {
            g.location = Some(l);
            assert_eq!(g.heal(1), Err(ServiceError::NotHere), "{l}");
        }
        g.location = Some(2);
        assert!(g.heal(1).is_ok(), "an ill-disposed church still heals");
    }

    fn bandit_army() -> crate::rules::world::Army {
        let mut s = map();
        s.armies = vec![army(1, 20, 6, -2, &[troop(9, 0, 1)])];
        start(&s).world.armies.remove(0)
    }

    /// Kills squad member `i` in a won battle against a one-bandit army.
    fn lose_unit_in_battle(g: &mut Game, i: usize) {
        g.world.armies.push(bandit_army());
        g.foe = Some(Foe::Army(g.world.armies.len() - 1));
        let mut b = g.start_battle();
        b.begin();
        for f in b.fighters.iter_mut() {
            if f.team == Team::Enemy || f.squad_index == Some(i) {
                f.hp = 0;
            }
        }
        assert!(matches!(g.resolve_battle(&b), BattleResult::Victory { lost: 1, .. }));
    }

    #[test]
    fn the_dead_stay_as_corpses_and_can_be_raised_in_a_town() {
        let mut s = map();
        s.buildings = vec![hall(BuildingType::Town, 2, 2, 1), hall(BuildingType::Castle, 6, 2, 2)];
        let mut g = inside(&s);
        g.squad[1].items[0] = Some(ItemId(20));
        lose_unit_in_battle(&mut g, 1);
        assert_eq!(g.squad.len(), 3, "the corpse stays in the army");
        let corpse = &g.squad[1];
        assert!(!corpse.alive() && corpse.died_at.is_some());
        assert!(corpse.items[0] == Some(ItemId(20)) && !g.pack.contains(&ItemId(20)), "Razdor's: the corpse keeps its items");
        assert_eq!(g.heal_price(1), None);
        // Resurrection: Round(Cost × 300% × 100 / F) = 50 × 3 / 1.2.
        assert_eq!(g.resurrect_price(1), Some(Price::gold(125)));
        g.location = Some(1);
        assert_eq!(g.resurrect(1), Err(ServiceError::NotHere), "castles heal but do not resurrect");
        g.location = Some(0);
        let gold = g.gold;
        g.resurrect(1).unwrap();
        assert_eq!((g.squad[1].hp, g.squad[1].died_at, g.gold), (40, None, gold - 125));
        assert_eq!(g.resurrect(1), Err(ServiceError::NotDead));
        // Corpses do not fight, and the wage bill does not count them (0x4a184a).
        lose_unit_in_battle(&mut g, 2);
        assert_eq!((g.wage(2), g.daily_wages()), (0, 6));
        let b = g.start_battle();
        assert!(b.fighters.iter().all(|f| f.squad_index != Some(2)));
    }

    #[test]
    fn the_players_corpses_are_never_buried_and_raised_paid() {
        let mut s = map();
        s.buildings = vec![hall(BuildingType::Church, 2, 2, 1)];
        let mut g = inside(&s);
        g.mana = 1000;
        lose_unit_in_battle(&mut g, 1);
        g.squad[1].unpaid = true;
        // `MaxTimeResurection` is the AI's: a month later the body is still there.
        g.pass_time(30.0 * 24.0 * 60.0, &mut Vec::new());
        assert_eq!(g.squad.len(), 3);
        assert!(g.resurrect_price(1).is_some());
        let last_paid = g.squad[1].last_paid;
        g.gold = 1000;
        g.resurrect(1).unwrap();
        assert!(g.squad[1].alive() && !g.squad[1].unpaid, "raised and marked paid");
        assert_eq!(g.squad[1].last_paid, last_paid, "its last pay unchanged");
    }

    #[test]
    fn a_cost_of_2_mod_256_is_resurrected_for_gold() {
        let mut c = content();
        c.units.iter_mut().find(|u| u.id == 4).unwrap().cost = 2050; // as unit 56
        let mut s = map();
        s.buildings = vec![hall(BuildingType::Town, 2, 2, 1)];
        let mut g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        g.location = Some(0);
        g.squad[1].hp = 0;
        // Round(2050 × 300% × 100 / 120) = 5125, in gold: the Community's bug read the Cost's
        // low byte (2) as "pay in mana".
        assert_eq!(g.resurrect_price(1), Some(Price { amount: 5125, currency: Currency::Gold }));
        (g.gold, g.mana) = (5000, 9000);
        assert_eq!(g.resurrect(1), Err(ServiceError::CannotAfford));
        g.gold = 6000;
        g.resurrect(1).unwrap();
        assert_eq!((g.gold, g.mana), (6000 - 5125, 9000));
    }

    #[test]
    fn barracks_stock_goes_down_and_regrows_by_chance() {
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, 1);
        t.barracks[0] = RecruitSlot { unit: 4, start_count: 1, max_count: 5 };
        t.barracks[1] = RecruitSlot { unit: 5, start_count: 0, max_count: 1 };
        s.buildings = vec![t];
        let mut g = inside(&s);
        assert_eq!(g.recruits_here(), vec![UnitId(4)], "no archers yet");
        g.hire(UnitId(4)).unwrap();
        assert_eq!(g.gold, 950);
        assert_eq!(g.hire(UnitId(4)), Err(crate::rules::game::HireError::NotOffered), "sold out");
        // MaxDayCountForNewUnit = 10: 5 militia gain one with chance 1/2 a day, an archer
        // (max 1) with 1/10.
        let day = 24.0 * 60.0;
        let (mut militia, mut archers) = (Vec::new(), Vec::new());
        g.pass_time(15.0 * 60.0, &mut Vec::new()); // the first midnight
        for _ in 0..60 {
            militia.push(g.world.locations[0].recruits[0].stock.unwrap());
            archers.push(g.world.locations[0].recruits[1].stock.unwrap());
            g.pass_time(day, &mut Vec::new());
        }
        assert!(militia.windows(2).all(|w| w[1] - w[0] <= 1 && w[1] >= w[0]), "one at a time: {militia:?}");
        let full = militia.iter().position(|&n| n == 5).expect("full in 60 days");
        assert!((4..30).contains(&full), "about 2 days a unit: {militia:?}");
        assert_eq!(*militia.last().unwrap(), 5, "capped at the maximum");
        assert_eq!(*archers.last().unwrap(), 1);
    }

    #[test]
    fn elementals_are_hired_healed_and_paid_in_mana() {
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, 1);
        t.barracks[0] = RecruitSlot { unit: 8, start_count: 3, max_count: 3 };
        t.recruit_all_types = 1; // an elemental is not of ordinary Nature
        s.buildings = vec![t];
        let mut g = inside(&s);
        assert_eq!(g.hire_price(UnitId(8)), Price { amount: 80, currency: Currency::Mana });
        assert_eq!(g.hire(UnitId(8)), Err(crate::rules::game::HireError::NotEnoughGold), "no mana");
        g.mana = 100;
        g.hire(UnitId(8)).unwrap();
        assert_eq!((g.mana, g.gold), (20, 1000));
        g.squad[3].hp = 25; // of 50: 1/2 × 80 × 50% × 100/120 = 16.7 → 17 mana
        assert_eq!(g.heal_price(3), Some(Price { amount: 17, currency: Currency::Mana }));
        // The heal is checked against the mana it is paid in (the original's bug compared it
        // with the gold).
        g.gold = 0;
        g.heal(3).unwrap();
        assert_eq!((g.mana, g.gold), (3, 0));
        g.gold = 1000;
        // Wage 80/2 × ½ = 20 mana a day.
        assert_eq!((g.daily_wages(), g.daily_mana_wages()), (12, 20));
    }

    #[test]
    fn noon_pays_recruits_whatever_their_nature_and_reports_the_nominal_income() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 100, &[troop(4, 0, 1), troop(9, 0, 1)]);
        let mut fort = town(BuildingType::Fort, 8, 2, 3);
        fort.faction = 1;
        fort.owner_army = 0; // the owner byte makes it the player's
        fort.gold_per_day = 40;
        fort.mana_per_day = 7;
        let mut village = town(BuildingType::Village, 12, 2, 1);
        village.gold_per_day = 500; // tribute, not income
        s.buildings = vec![fort, village];
        let mut g = start(&s);
        g.first_noon_today();
        // Militia 50/2 × ¼ = 6.25 → 6. The bandit (a rogue) is a recruit too: 55/2 × ½ = 13.75 → 14.
        assert_eq!((g.wage(0), g.wage(1), g.wage(2)), (0, 6, 14));
        assert_eq!(g.squad[0].wage_kind, crate::rules::content::WageKind::Leader);
        // The report shows the fort's 40 × F/100, but the noon pays its stock, which a fort
        // with no maximum never has, and no building pays mana.
        assert_eq!((g.daily_income(), g.daily_wages()), (48, 20));
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events); // 09:00 -> 12:00
        let day = g.clock.day_index();
        let want = DayReport { day, income: 48, wages: 20, mana_wages: 0, unpaid: 0, deserted: vec![], gold: 100, mana_total: 0 };
        assert_eq!(events, vec![Event::NewDay(want)], "the gold before the payment");
        assert_eq!((g.gold, g.mana), (80, 0));
        assert_eq!(g.stored_income, 40, "the fort's nominal income, without F");
    }

    #[test]
    fn castles_and_forts_pay_their_grown_stock_and_towns_nothing() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 100, &[]);
        let mut fort = town(BuildingType::Fort, 8, 2, 3);
        (fort.faction, fort.gold_per_day, fort.gold_max, fort.mana_per_day, fort.mana_max) = (1, 30, 90, 5, 20);
        let mut t = town(BuildingType::Town, 12, 2, 3);
        (t.faction, t.gold_per_day, t.gold_max) = (1, 50, 100);
        (fort.owner_army, t.owner_army) = (0, 0); // the owner byte makes them the player's
        s.buildings = vec![fort, t];
        let mut g = start(&s);
        // Both start at 0 and grow from the first midnight, every building with a maximum.
        assert_eq!(g.world.locations[0].tribute_gold, 0);
        g.pass_time(15.0 * 60.0 + 1.0, &mut Vec::new()); // past midnight
        assert_eq!((g.world.locations[0].tribute_gold, g.world.locations[0].tribute_mana, g.world.locations[1].tribute_gold), (30, 5, 50));
        let mut events = Vec::new();
        g.pass_time(12.0 * 60.0, &mut events); // the first noon
        // The report: 30 + 50 nominal × 1.2; the pay: the fort's stock 30 × 1.2, the town nothing.
        assert!(matches!(&events[..], [Event::NewDay(DayReport { income: 96, gold: 100, .. })]), "{events:?}");
        assert_eq!((g.gold, g.mana), (136, 0), "no mana at noon");
        let l = &g.world.locations;
        assert_eq!((l[0].tribute_gold, l[0].tribute_mana, l[1].tribute_gold), (0, 5, 50));
    }

    #[test]
    fn unpaid_units_sit_out_and_desert_a_week_after_their_last_pay() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 0, &[troop(4, 0, 2)]);
        s.buildings = vec![town(BuildingType::Village, 12, 2, 1)];
        let mut g = start(&s);
        g.first_noon_today();
        g.squad[1].items[0] = Some(ItemId(21));
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events);
        let Event::NewDay(r) = &events[0] else { panic!() };
        assert_eq!((r.wages, r.unpaid, r.gold), (12, 2, 0));
        assert!(g.squad[1].unpaid && g.squad[2].unpaid);
        let b = g.start_battle();
        assert_eq!(b.fighters.iter().filter(|f| f.team == Team::Player).count(), 1, "only the hero fights");
        // 6 gold for a bill of 12: the cheapest (the first of two equals) gets his back.
        g.gold = 6;
        events.clear();
        g.pass_time(24.0 * 60.0, &mut events);
        let Event::NewDay(r) = &events[0] else { panic!() };
        assert_eq!((r.wages, r.unpaid, r.gold, g.gold), (12, 1, 6, 0));
        assert!(g.squad[1].unpaid && !g.squad[2].unpaid);
        // MaxTimeNotUpkeep = 7 days since the last pay: the first militia (last paid at the
        // start, 09:00) leaves at the 8th noon, his ring with him (Army_RemoveUnit moves
        // nothing to the pack). The other was paid a day later.
        events.clear();
        g.pass_time(5.0 * 24.0 * 60.0, &mut events);
        assert_eq!(g.squad.len(), 3, "not yet");
        g.pass_time(24.0 * 60.0, &mut events);
        let deserted: Vec<UnitId> = events.iter().flat_map(|e| match e {
            Event::NewDay(r) => r.deserted.clone(),
            _ => vec![],
        }).collect();
        assert_eq!(deserted, vec![UnitId(4)]);
        assert_eq!(g.squad.len(), 2);
        assert!(!g.pack.contains(&ItemId(21)));
    }

    #[test]
    fn a_short_noon_refunds_the_cheapest_and_keeps_the_dearest_paid() {
        let mut s = map();
        // Militia 6, archer 90/2 × ½ = 22.5 → 22, merchant 100/2 × ½ = 25.
        s.header.heroes[0] = hero(2, 2, 30, &[troop(4, 0, 1), troop(5, 0, 1), troop(7, 0, 1)]);
        let mut g = start(&s);
        g.first_noon_today();
        assert_eq!(g.daily_wages(), 53);
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events);
        // 30 − 53 = −23: the militia's 6 back (−17), the archer's 22 back (+5): gold set to 0.
        // The report shows the bill and the gold before the payment.
        let Event::NewDay(r) = &events[0] else { panic!() };
        assert_eq!((r.wages, r.unpaid, r.gold, g.gold), (53, 2, 30, 0));
        assert!(g.squad[1].unpaid && g.squad[2].unpaid && !g.squad[3].unpaid);
    }

    #[test]
    fn the_refunds_are_full_wages_corpses_included_never_elementals() {
        let mut s = map();
        // Golem (an elemental, 20 mana), quartermaster 25 (Rear Service), archer 22, militia 6.
        s.header.heroes[0] = hero(2, 2, 0, &[troop(8, 0, 1), troop(10, 0, 1), troop(5, 0, 1), troop(4, 0, 1)]);
        let mut g = start(&s);
        g.first_noon_today();
        g.mana = 100;
        g.squad[4].hp = 0; // the militia is a corpse: not billed
        g.gold = 10;
        assert_eq!(g.daily_wages(), 47);
        // No income: 47 × 78 div 256 = 14 is deducted, 10 − 14 = −4. The corpse's full 6 comes
        // back first, which is enough.
        g.pass_time(3.0 * 60.0, &mut Vec::new());
        assert_eq!(g.gold, 0);
        assert_eq!(g.squad.iter().map(|u| u.unpaid).collect::<Vec<_>>(), [false, false, false, false, true]);
        assert_eq!(g.mana, 80, "the golem is paid in mana");
        // 0 − 14: the corpse (6), then the archer (22): the golem is never refunded.
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert_eq!(g.squad.iter().map(|u| u.unpaid).collect::<Vec<_>>(), [false, false, false, true, true]);
    }

    #[test]
    fn with_no_mana_the_elementals_go_unpaid_and_everyone_else_is_paid() {
        let mut s = map();
        // Golem (an elemental, 20 mana) and two militia.
        s.header.heroes[0] = hero(2, 2, 0, &[troop(8, 0, 1), troop(4, 0, 2)]);
        let mut g = start(&s);
        g.first_noon_today();
        // A short noon: the militia refunded and unpaid.
        g.pass_time(3.0 * 60.0, &mut Vec::new());
        assert!(g.squad[2].unpaid && g.squad[3].unpaid);
        // Enough gold, but no mana: the golem goes unpaid, the militia are paid again and the
        // flag is cleared. The original's bug (0xc25f7d) left every other unit's old mark,
        // so the militia stayed unpaid though their wages were paid, and the flag stayed up.
        g.mana = 0;
        g.gold = 100;
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert!(!g.mana_short);
        assert_eq!(g.squad.iter().map(|u| u.unpaid).collect::<Vec<_>>(), [false, true, false, false]);
        let now = g.clock.total_minutes() as u64;
        assert!(g.squad.iter().all(|u| u.last_paid + 60 > now), "paid now, so nobody deserts");
        // With mana an enough-gold noon pays everyone.
        g.mana = 100;
        g.gold = 100;
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert!(!g.mana_short && g.squad.iter().all(|u| !u.unpaid));
    }

    #[test]
    fn event_units_and_the_hero_are_free_and_garrisons_are_never_paid() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 2, 2, 3);
        castle.faction = 1;
        castle.owner_army = 0; // the owner byte makes it the player's
        s.buildings = vec![castle];
        let mut g = inside(&s);
        g.squad[2].wage_kind = crate::rules::content::WageKind::Event;
        assert_eq!((g.wage(0), g.wage(1), g.wage(2)), (0, 6, 0));
        g.leave_in_garrison(1, None).unwrap();
        assert_eq!(g.daily_wages(), 0);
    }

    #[test]
    fn rear_service_cuts_the_whole_bill_and_more_without_stored_income() {
        let mut s = map();
        // Quartermaster 25, archer 22, two militia 6: a bill of 59.
        s.header.heroes[0] = hero(2, 2, 1000, &[troop(10, 0, 1), troop(5, 0, 1), troop(4, 0, 2)]);
        let mut fort = town(BuildingType::Fort, 8, 2, 3);
        fort.faction = 1;
        fort.owner_army = 0; // the owner byte makes it the player's
        fort.gold_per_day = 10;
        s.buildings = vec![fort];
        let mut g = start(&s);
        g.first_noon_today();
        g.mana = 1000;
        // The shown wages are the full bill.
        assert_eq!((g.wage(1), g.daily_wages()), (25, 59));
        // The fort's nominal income is stored: 59 × 178 div 256 = 41 (per unit it would be 40).
        g.pass_time(3.0 * 60.0, &mut Vec::new());
        assert_eq!((g.gold, g.stored_income), (959, 10));
        // Without it the stored income is 0: 59 × 78 div 256 = 17.
        g.world.locations[0].owner = crate::rules::world::Owner::Neutral;
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert_eq!((g.gold, g.stored_income), (942, 0));
        // A dead quartermaster still counts.
        g.squad[1].hp = 0;
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert_eq!(g.gold, 942 - 34 * 78 / 256);
    }

    #[test]
    fn elementals_go_unpaid_when_the_mana_runs_out() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 100, &[troop(8, 0, 2), troop(4, 0, 1)]);
        let mut g = start(&s);
        g.first_noon_today();
        g.mana = 30; // two golems want 20 each
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events);
        let Event::NewDay(r) = &events[0] else { panic!() };
        assert_eq!((g.mana, r.unpaid, r.wages), (0, 2, 6));
        assert!(g.squad[1].unpaid && g.squad[2].unpaid && !g.squad[3].unpaid);
    }

    #[test]
    fn villages_linked_to_the_players_castle_pay_their_gold_into_his_noon() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 8, 2, 3);
        castle.faction = 1;
        castle.owner_army = 0; // the owner byte makes it the player's
        let mut v = town(BuildingType::Village, 14, 2, 1);
        v.gold_per_day = 30;
        v.gold_max = 90;
        v.mana_per_day = 4;
        v.mana_max = 10;
        v.linked_building = 1;
        s.buildings = vec![castle, v];
        let mut g = start(&s);
        g.first_noon_today();
        assert_eq!(g.daily_income(), 0, "the report shows castles, not village stocks");
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events);
        assert!(matches!(&events[..], [Event::NewDay(DayReport { income: 0, wages: 12, .. })]), "{events:?}");
        assert_eq!(g.gold, 1000 + 30 - 12);
        assert_eq!(g.stored_income, 30, "its stock goes into the stored income");
        assert_eq!((g.world.locations[1].tribute_gold, g.world.locations[1].tribute_mana), (0, 4), "gold only");
    }

    #[test]
    fn a_medic_heals_ten_percent_at_midnight() {
        let mut s = map();
        s.header.heroes[0] = hero(2, 2, 1000, &[troop(4, 0, 1), troop(11, 0, 1)]);
        let mut g = start(&s);
        g.squad[1].hp = 10; // of 40
        g.pass_time(14.0 * 60.0, &mut Vec::new()); // 09:00 -> 23:00: noon is not a heal
        assert_eq!(g.squad[1].hp, 10);
        g.pass_time(60.0, &mut Vec::new());
        assert_eq!(g.squad[1].hp, 14);
        // A dead medic still counts (0x4a1dca).
        g.squad[2].hp = 0;
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert_eq!(g.squad[1].hp, 18);
    }

    /// The hero walks from (2, 2) into a village at (8, 2) holding 40 gold and 5 mana, with
    /// `rng` seed `seed`. Returns the game, the events of the walk, and his mana before it.
    fn walk_into_village(seed: u32, attitude: i8) -> (Game, Vec<Event>, i32) {
        let mut s = map();
        let mut v = town(BuildingType::Village, 8, 2, attitude);
        (v.gold_per_day, v.gold_max, v.mana_per_day, v.mana_max) = (40, 40, 5, 5);
        s.buildings = vec![v];
        let mut g = start(&s);
        g.rng = crate::rules::rng::Rng::new(seed);
        let mana = g.mana;
        assert!(g.set_destination(g.world.locations[0].tile));
        let mut events = Vec::new();
        for _ in 0..2000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        assert_eq!(g.location, Some(0));
        (g, events, mana)
    }

    #[test]
    fn entering_a_village_collects_its_tribute_at_once() {
        // world.md / economy.md §3 (0x4c6000) and the footage: no button, the hero takes it all.
        let seed = (0..200).find(|&s| walk_into_village(s, 1).0.village_offer().is_none()).unwrap();
        let (g, events, mana) = walk_into_village(seed, 1);
        assert!(events.contains(&Event::Tribute { at: 0, paid: Tribute::Gold(40), mana: 5 }), "{events:?}");
        assert_eq!((g.world.locations[0].tribute_gold, g.world.locations[0].tribute_mana), (0, 0));
        assert_eq!(g.mana, mana + 5);
        assert_eq!(g.tribute_available(), None, "already collected");
    }

    #[test]
    fn a_click_on_the_village_he_stands_in_pays_what_has_refilled() {
        // 0x4cd0aa → 0x4bbc84: the click enters it again, so the refilled stock is paid
        // (Razdor 0.4.4 showed "already taken" and left it in the village).
        let seed = (0..200).find(|&s| walk_into_village(s, 1).0.village_offer().is_none()).unwrap();
        let (mut g, _, _) = walk_into_village(seed, 1);
        (g.world.locations[0].tribute_gold, g.world.locations[0].tribute_mana) = (40, 5);
        let (gold, mana) = (g.gold, g.mana);
        let events = g.reenter_building();
        assert!(events.contains(&Event::Arrived(0)), "{events:?}");
        if g.village_offer().is_none() {
            assert!(events.contains(&Event::Tribute { at: 0, paid: Tribute::Gold(40), mana: 5 }), "{events:?}");
            assert_eq!((g.gold, g.mana), (gold + 40, mana + 5));
        }
    }

    #[test]
    fn with_an_offer_the_tribute_waits_for_the_answer() {
        let seed = (0..200).find(|&s| walk_into_village(s, 1).0.village_offer().is_some()).unwrap();
        let (mut g, events, _) = walk_into_village(seed, 1);
        assert!(!events.iter().any(|e| matches!(e, Event::Tribute { .. })), "not before the answer");
        assert_eq!(g.world.locations[0].tribute_gold, 40);
        let gold = g.gold;
        assert_eq!(g.decline_offer(), Some(Tribute::Gold(40)), "no thanks: the tribute instead");
        assert_eq!((g.gold, g.world.locations[0].tribute_gold), (gold + 40, 0));
        assert_eq!(g.village_offer(), None);
    }

    #[test]
    fn an_ill_disposed_village_stepped_on_is_his_unless_guarded() {
        // World.md §4.2 (0x4ad94c): an unguarded village is taken by stepping on it, whatever
        // its owner, so it pays.
        let (g, events, _) = walk_into_village(1, -1);
        assert!(events.contains(&Event::Captured(0)), "{events:?}");
        assert!(g.world.locations[0].owned());
        assert!(g.village_offer().is_some() || events.iter().any(|e| matches!(e, Event::Tribute { .. })), "{events:?}");
        // Guarded by an ill-disposed army standing in it (on another of its cells), the
        // guard is met instead. Only an army in the building guards it (+0x3788, the
        // building it stands in), not one whose home it is that is away.
        let walk = |guard_at: (u16, u16)| {
            let mut s = map();
            let mut v = crate::rules::world::testkit::building(BuildingType::Village, 8, 2, (1, 2));
            v.relations = [-1, 0, 0, 0];
            v.faction = 4;
            (v.gold_per_day, v.gold_max) = (40, 40);
            s.buildings = vec![v];
            let mut guard = crate::rules::world::testkit::army(1, guard_at.0, guard_at.1, -2, &[crate::rules::world::testkit::troop(4, 0, 1)]);
            guard.home_building = 1;
            (guard.patrols, guard.patrol_radius) = (1, 0);
            s.armies = vec![guard];
            let mut g = start(&s);
            let cell = (8, 2);
            assert_eq!(g.world.location_covering(cell), Some(0));
            assert!(g.set_destination(cell));
            let mut events = Vec::new();
            while g.moving() {
                events.extend(g.tick(0.05));
            }
            (g, events)
        };
        let other = (8, 1);
        let (g, events) = walk(other);
        assert_eq!(g.world.location_covering((8, 1)), Some(0));
        assert_eq!(events.last(), Some(&Event::Encounter(0)), "{events:?}");
        assert!(!g.world.locations[0].owned() && g.location.is_none());
        assert_eq!(g.world.locations[0].tribute_gold, 40);
        let (g, events) = walk((12, 6));
        assert!(events.contains(&Event::Captured(0)), "its army is away: {events:?}");
        assert!(g.world.locations[0].owned());
    }

    /// A village with `gold`/`mana` waiting, the hero entering it with `rng` seed `seed`.
    fn visit_village(gold: u16, mana: u8, seed: u32, setup: &dyn Fn(&mut Game)) -> Game {
        let mut s = map();
        let mut v = town(BuildingType::Village, 2, 2, 1);
        (v.gold_per_day, v.gold_max, v.mana_per_day, v.mana_max) = (gold, gold, mana, mana);
        s.buildings = vec![v];
        let mut g = inside(&s);
        g.rng = crate::rules::rng::Rng::new(seed);
        setup(&mut g);
        g.visit_village(0);
        g
    }

    #[test]
    fn a_village_makes_at_most_one_offer_by_its_rolls_and_conditions() {
        use crate::rules::economy::{OfferResult, VillageOffer};
        let mut seen = std::collections::BTreeMap::new();
        for seed in 0..400 {
            let g = visit_village(40, 5, seed, &|_| {});
            *seen.entry(g.village_offer()).or_insert(0) += 1;
        }
        // No one unpaid or hurt: never the innkeeper or the priest. The witch needs 3 spells.
        assert!(!seen.contains_key(&Some(VillageOffer::Innkeeper)) && !seen.contains_key(&Some(VillageOffer::Priest)));
        assert!(!seen.contains_key(&Some(VillageOffer::Witch)));
        let bless = seen[&Some(VillageOffer::Blessing)];
        assert!((40..110).contains(&bless), "about 1 in 6: {seen:?}");
        assert!(seen[&Some(VillageOffer::Furs)] > 30 && seen[&None] > 200, "{seen:?}");

        // The unpaid, broke army: the innkeeper (1 in 2) pays everyone and the village is emptied.
        let broke = |g: &mut Game| {
            g.gold = 0;
            g.squad.iter_mut().skip(1).for_each(|u| u.unpaid = true);
        };
        let mut g = (0..50).map(|seed| visit_village(40, 5, seed, &broke)).find(|g| g.village_offer() == Some(VillageOffer::Innkeeper)).unwrap();
        assert_eq!(g.accept_offer(), Some(OfferResult::Paid(3)));
        assert!(g.squad.iter().all(|u| !u.unpaid));
        assert_eq!((g.world.locations[0].tribute_gold, g.world.locations[0].tribute_mana), (0, 0));
        assert_eq!(g.village_offer(), None);

        // The wounded army: the priest casts spell 1 (+30 each).
        let hurt = |g: &mut Game| g.squad.iter_mut().for_each(|u| u.hp = 5);
        let mut g = (0..50).map(|seed| visit_village(40, 5, seed, &hurt)).find(|g| g.village_offer() == Some(VillageOffer::Priest)).unwrap();
        assert!(matches!(g.accept_offer(), Some(OfferResult::Healed(h)) if h > 0));
        assert_eq!(g.squad[1].hp, 35);

        // Furs: item 135. The witch: 300–500 mana, when mana < gold and 3 spells are known.
        let mut g = (0..80).map(|seed| visit_village(40, 5, seed, &|_| {})).find(|g| g.village_offer() == Some(VillageOffer::Furs)).unwrap();
        assert_eq!(g.accept_offer(), Some(OfferResult::Furs(ItemId(135))));
        assert_eq!(g.pack, vec![ItemId(135)]);
        let learned = |g: &mut Game| g.spells = vec![1, 2, 3];
        let mut g = (0..200).map(|seed| visit_village(40, 5, seed, &learned)).find(|g| g.village_offer() == Some(VillageOffer::Witch)).unwrap();
        let mana = g.mana;
        let Some(OfferResult::Mana(m)) = g.accept_offer() else { panic!() };
        assert!((300..=500).contains(&m) && m % 50 == 0 && g.mana == mana + m);
    }

    #[test]
    fn the_village_blessing_lasts_ten_times_its_time() {
        use crate::rules::economy::{OfferResult, VillageOffer};
        let blessing = |seed| visit_village(40, 5, seed, &|_| {});
        let mut g = (0..400).map(blessing).find(|g| g.village_offer() == Some(VillageOffer::Blessing) && g.offer_roll == 0).unwrap();
        let now = g.clock.total_minutes() as u64;
        assert_eq!(g.accept_offer(), Some(OfferResult::Blessing(3)), "spell 3 + 2·0");
        // TimeWork 4 h × 10.
        assert_eq!(g.active_spells().iter().map(|e| (e.spell, e.until)).collect::<Vec<_>>(), [(3, now + 40 * 60)]);
    }

    /// The blessing's spell and the witch's mana are rolled as the offer's window is built
    /// (0x4aca80: `Random(5)` before the window's chord), not at the answer: a yes draws
    /// nothing. The blessing is spell 3 + 2·r even when the install lacks it (not cast then).
    #[test]
    fn the_offers_roll_is_drawn_as_it_opens() {
        use crate::rules::economy::{OfferResult, VillageOffer};
        let learned = |g: &mut Game| g.spells = vec![1, 2, 3];
        for (kind, setup) in [(VillageOffer::Witch, &learned as &dyn Fn(&mut Game)), (VillageOffer::Blessing, &|_: &mut Game| {})] {
            let mut g = (0..400).map(|seed| visit_village(40, 5, seed, setup)).find(|g| g.village_offer() == Some(kind) && g.offer_roll > 0).unwrap();
            let r = g.offer_roll;
            let state = g.rng.state();
            let mana = g.mana;
            let result = g.accept_offer();
            assert_eq!(g.rng.state(), state, "the yes draws nothing");
            match result {
                Some(OfferResult::Mana(m)) => assert_eq!((m, g.mana), (300 + 50 * r, mana + m)),
                Some(OfferResult::Blessing(id)) => {
                    assert_eq!(id, 3 + 2 * r as u32);
                    assert!(g.active_spells().is_empty(), "spell {id} is not in the test content");
                }
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn the_village_that_made_the_last_offer_makes_none_until_its_tribute_is_taken() {
        use crate::rules::economy::VillageOffer;
        let seed = (0..80).find(|&seed| visit_village(40, 5, seed, &|_| {}).village_offer().is_some()).unwrap();
        let mut g = visit_village(40, 5, seed, &|_| {});
        let first = g.village_offer().unwrap();
        for _ in 0..30 {
            g.visit_village(0);
            assert_eq!(g.village_offer(), None, "the same village again");
        }
        assert!(g.collect_tribute().is_some());
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        // Visit by visit: a kind is never offered on the very next visit after it, but an
        // empty visit in between clears "last" (the original's behaviour).
        let mut visits = vec![Some(first)];
        for _ in 0..60 {
            g.visit_village(0);
            let o = g.village_offer();
            visits.push(o);
            if o.is_some() {
                g.collect_tribute();
                g.pass_time(24.0 * 60.0, &mut Vec::new());
            }
        }
        assert!(visits.windows(2).all(|w| w[0].is_none() || w[0] != w[1]), "{visits:?}");
        assert!(visits.windows(3).any(|w| w[1].is_none() && w[0].is_some() && w[0] == w[2]), "a kind back after an empty visit: {visits:?}");
        let _ = VillageOffer::Witch;
    }

    #[test]
    fn a_village_rolls_every_offer_and_forgets_the_last_after_an_empty_visit() {
        use crate::rules::economy::VillageOffer;
        use crate::rules::rng::Rng;
        // No one unpaid or hurt and two spells known: only the blessing and the furs can
        // pass. The blessing was the last offer, so its roll is drawn but cannot pass.
        let last = |g: &mut Game| g.last_offer = Some(VillageOffer::Blessing);
        let (mut blessed, mut empty) = (false, false);
        for seed in 0..200 {
            let g = visit_village(40, 5, seed, &last);
            let mut r = Rng::new(seed);
            r.random(2);
            r.random(3);
            blessed |= r.random(6) == 0;
            let furs = r.random(6) == 0;
            if !furs {
                r.random(6);
            }
            assert_eq!(g.rng.state(), r.state(), "seed {seed}: every roll up to the first that passes");
            let want = furs.then_some(VillageOffer::Furs);
            assert_eq!(g.village_offer(), want, "seed {seed}");
            // "Last" becomes the offer, or none after an empty visit.
            assert_eq!(g.last_offer, want);
            empty |= want.is_none();
        }
        assert!(blessed && empty);
        // After the empty visit the blessing may come again.
        let seed = (0..200).find(|&s| visit_village(40, 5, s, &|_| {}).village_offer() == Some(VillageOffer::Blessing)).unwrap();
        let none = |g: &mut Game| g.last_offer = None;
        assert_eq!(visit_village(40, 5, seed, &none).village_offer(), Some(VillageOffer::Blessing));
    }

    #[test]
    fn the_innkeeper_and_the_priest_compare_with_the_army_size_div_2() {
        use crate::rules::economy::VillageOffer;
        use crate::rules::rng::Rng;
        // An army of 3: one unpaid unit is enough (3 div 2 = 1).
        let seed = (0..200).find(|&s| Rng::new(s).random(2) == 0).unwrap();
        let broke = |g: &mut Game| {
            g.gold = 0;
            g.squad.truncate(3);
            g.squad.iter_mut().for_each(|u| u.unpaid = false);
            g.squad[2].unpaid = true;
        };
        let g = visit_village(40, 5, seed, &broke);
        assert_eq!(g.squad.len(), 3);
        assert_eq!(g.village_offer(), Some(VillageOffer::Innkeeper));
        // The priest counts the living (not the wounded) against the size div 2: in an
        // army of 7, three wounded units missing over 50 HP in all are enough (7 div 2 = 3),
        // though they are fewer than half.
        let seed = (0..200).find(|&s| {
            let mut r = Rng::new(s);
            r.random(2) != 0 && r.random(3) == 0
        });
        let seed = seed.unwrap();
        let hurt = |g: &mut Game| {
            g.squad.truncate(2);
            while g.squad.len() < 7 {
                g.squad.push(g.squad[1].clone());
            }
            (1..4).for_each(|k| g.squad[k].hp = 1);
        };
        let g = visit_village(40, 5, seed, &hurt);
        let missing: i32 = g.squad.iter().map(|u| u.max_hp(&g.content) - u.hp).sum();
        assert!(missing > 50 && g.squad.iter().filter(|u| u.hp < u.max_hp(&g.content)).count() == 3);
        assert_eq!(g.village_offer(), Some(VillageOffer::Priest));
    }

    #[test]
    fn a_rogue_hero_gets_nothing_from_villages() {
        let mut g = visit_village(40, 5, 1, &|_| {});
        assert!(g.tribute_available().is_some());
        g.squad[0].def = UnitId(9);
        assert_eq!(g.tribute_available(), None);
        assert_eq!(g.collect_tribute(), None);
    }

    #[test]
    fn village_tribute_accumulates_to_its_cap_and_is_collected_on_a_visit() {
        let mut s = map();
        let mut v = town(BuildingType::Village, 2, 2, 1);
        v.gold_per_day = 30;
        v.gold_max = 70;
        v.mana_per_day = 10;
        v.mana_max = 25;
        s.buildings = vec![v];
        let mut g = inside(&s);
        assert_eq!(g.tribute_available(), Some(30), "one day's worth at the start");
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        let v = &g.world.locations[0];
        // 30 + 30 × √(1 − 30/70) = 52.7 → 53; mana 10 + 10 × √(1 − 10/25) = 17.7 → 18.
        assert_eq!((v.tribute_gold, v.tribute_mana), (53, 18), "slower as it fills");
        g.pass_time(4.0 * 24.0 * 60.0, &mut Vec::new());
        let v = &g.world.locations[0];
        assert_eq!((v.tribute_gold, v.tribute_mana), (70, 25), "capped");
        let (gold, mana) = (g.gold, g.mana);
        assert!(g.collect_tribute().is_some());
        assert_eq!((g.gold, g.mana), (gold + 70, mana + 25));
        assert_eq!(g.tribute_available(), None);
    }

    #[test]
    fn garrisons_take_troops_who_are_never_paid_and_heal_at_midnight() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 2, 2, 3);
        castle.faction = 1;
        castle.owner_army = 0; // the owner byte makes it the player's
        s.buildings = vec![castle, town(BuildingType::Castle, 8, 2, 1)];
        let mut g = inside(&s);
        g.first_noon_today();
        assert_eq!(g.leave_in_garrison(0, None), Err(ServiceError::Hero));
        g.squad[2].hp = 20; // of 40
        g.leave_in_garrison(2, None).unwrap();
        assert_eq!((g.squad.len(), g.garrison_here().len()), (2, 1));
        assert_eq!(g.daily_wages(), 6, "the garrison is not paid");
        let mut events = Vec::new();
        g.pass_time(3.0 * 60.0, &mut events); // noon
        assert!(matches!(&events[..], [Event::NewDay(DayReport { wages: 6, .. })]), "{events:?}");
        assert_eq!(g.garrison_here()[0].unit.hp, 20, "no heal at noon");
        g.pass_time(12.0 * 60.0, &mut events); // midnight
        assert_eq!(g.garrison_here()[0].unit.hp, 24, "GarrisonAutoHeal 10%");
        g.take_from_garrison(0, None, false).unwrap();
        assert_eq!((g.squad.len(), g.squad[2].hp), (3, 24));
        assert_eq!(g.take_from_garrison(0, None, false), Err(ServiceError::NoSuchUnit));
        g.location = Some(1);
        assert_eq!(g.leave_in_garrison(1, None), Err(ServiceError::NotHere), "not the player's castle");
    }

    /// The knight in his own castle (building 0) with 1000 gold and two militia.
    fn in_own_castle() -> Game {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 2, 2, 3);
        castle.faction = 1;
        castle.owner_army = 0; // the owner byte makes it the player's
        s.buildings = vec![castle];
        inside(&s)
    }

    #[test]
    fn an_unpaid_unit_parked_for_less_than_a_day_costs_a_days_wage_to_take_back() {
        let mut g = in_own_castle();
        let day = MINUTES_PER_DAY;
        g.pass_time(3.0 * day as f32, &mut Vec::new());
        let now = g.clock.total_minutes() as u64;
        // Left unpaid by short noons: last paid three days ago.
        (g.squad[1].unpaid, g.squad[1].last_paid) = (true, now - 3 * day);
        g.open_garrison();
        assert!(g.squad.iter().all(|u| u.seen == now), "the tab stamps the hero's army");
        g.leave_in_garrison(1, None).unwrap();
        assert!(g.garrison_here()[0].unit.unpaid, "it keeps its mark until the tab opens again");
        // Opening again the same day: last paid over 1440 minutes ago, stamped under a day ago.
        g.pass_time(600.0, &mut Vec::new());
        g.open_garrison();
        assert!(g.garrison_here()[0].unit.unpaid);
        // One day's kind-1 wage of its type: militia 6, asked only below the gold.
        assert_eq!(g.garrison_price(0), Some(6));
        assert_eq!(g.take_from_garrison(0, None, false), Err(ServiceError::Unpaid(6)));
        g.gold = 6;
        assert_eq!(g.take_from_garrison(0, None, true), Err(ServiceError::CannotAfford), "strictly below the gold");
        g.gold = 7;
        g.take_from_garrison(0, None, true).unwrap();
        let now = g.clock.total_minutes() as u64;
        assert_eq!((g.gold, g.squad[2].unpaid, g.squad[2].last_paid), (1, false, now));
        // Parked a whole day since its stamp, it is paid again: free.
        (g.squad[2].unpaid, g.squad[2].last_paid) = (true, now - 3 * day);
        g.open_garrison();
        g.leave_in_garrison(2, None).unwrap();
        g.pass_time(day as f32, &mut Vec::new());
        g.open_garrison();
        assert_eq!(g.garrison_price(0), Some(0));
        g.take_from_garrison(0, None, false).unwrap();
        assert_eq!(g.squad[2].last_paid, g.clock.total_minutes() as u64);
    }

    #[test]
    fn a_swap_with_the_garrison_is_free_and_named_units_and_corpses() {
        let mut g = in_own_castle();
        g.squad[2].hp = 0; // a corpse can be left
        g.leave_in_garrison(2, None).unwrap();
        g.squad[1].named = 1;
        assert_eq!(g.leave_in_garrison(1, None), Err(ServiceError::Named), "named units stay");
        g.squad[1].named = 0;
        let l = g.location.unwrap();
        g.world.locations[l].stationed[0].unit.hp = 40;
        let guard = &mut g.world.locations[l].stationed[0].unit;
        (guard.unpaid, guard.last_paid) = (true, 5);
        let (army_cell, guard_cell) = (g.squad[1].slot, g.garrison_here()[0].unit.slot);
        // The two records change places, each in the other's cell, with no price: the unpaid
        // guard comes in unpaid, and (the army's unit clicked first) nobody's pay refreshed.
        let gold = g.gold;
        g.swap_with_garrison(1, 0, false).unwrap();
        assert_eq!((g.squad[1].unpaid, g.squad[1].last_paid, g.squad[1].slot, g.gold), (true, 5, army_cell, gold));
        assert_eq!(g.garrison_here()[0].unit.slot, guard_cell);
        assert_eq!(g.swap_with_garrison(0, 0, true), Err(ServiceError::Hero));
        // The garrison's unit clicked first: it gets its last pay now, if it is paid.
        g.swap_with_garrison(1, 0, true).unwrap();
        assert_eq!(g.squad[1].last_paid, g.clock.total_minutes() as u64);
        g.squad[1].unpaid = true;
        g.squad[1].last_paid = 7;
        g.swap_with_garrison(1, 0, false).unwrap();
        g.swap_with_garrison(1, 0, true).unwrap();
        assert_eq!(g.squad[1].last_paid, 7, "unpaid: not refreshed");
    }

    #[test]
    fn a_scenario_garrison_of_an_own_castle_is_the_players_and_free() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 2, 2, 3);
        castle.faction = 1;
        castle.owner_army = 0; // the owner byte makes it the player's
        castle.garrison[0] = troop(4, 0, 3);
        s.buildings = vec![castle];
        let g = inside(&s);
        assert_eq!(g.garrison_here().len(), 3);
        assert!(g.world.locations[0].garrison.is_empty());
        assert_eq!(g.daily_wages(), 12, "only the two in the squad");
    }

    fn shop_town(attitude: i8) -> Scenario {
        let mut s = map();
        let mut t = town(BuildingType::Market, 2, 2, attitude);
        t.artifact_slots[0] = 24;
        t.random_artifacts_for_sale = 3;
        t.price_min = 50;
        t.price_max = 400;
        s.buildings = vec![t];
        s
    }

    #[test]
    fn a_map_load_starts_the_generator_at_1_so_fresh_markets_are_always_the_same() {
        let s = shop_town(2);
        let (a, b) = (start(&s), start(&s));
        assert_eq!(a.world.locations[0].shop.as_ref().unwrap().places, b.world.locations[0].shop.as_ref().unwrap().places);
        // State 1, the restock, then the world music's Random(90000).
        let mut want = start(&s);
        want.rng = crate::rules::rng::Rng::new(1);
        want.world.locations[0].shop.as_mut().unwrap().timer = 1;
        want.restock_markets();
        want.rng.random(90_000);
        assert_eq!(a.rng.state(), want.rng.state());
        assert_eq!(a.world.locations[0].shop.as_ref().unwrap().places, want.world.locations[0].shop.as_ref().unwrap().places);
    }

    /// A trade (or a heal, or a raise) asks for the events to be checked as the building
    /// window closes (0x4ed440 → 0x4b8f63): an event the purchase allows opens then, not at
    /// the next step; a window closed without one checks nothing.
    #[test]
    fn a_trade_has_the_events_checked_as_the_window_closes() {
        use crate::dt::dtm::EventKind;
        use crate::rules::events::EventOutcome;
        let mut s = shop_town(2);
        let mut e = crate::dt::dtm::Event { kind: EventKind::Global as u8, repeat: 1440, duration: 1440, once: 1, title: "t".into(), message: "m".into(), ..Default::default() };
        (e.conditions.artifacts_check, e.conditions.artifacts, e.conditions.artifacts_owner) = (1, [24, 0, 0], [1, 0, 0]);
        s.events = vec![e];
        let mut g = start(&s);
        g.drain_events();
        g.gold = 10_000;
        assert_eq!(g.location, Some(0), "he starts in the market");
        assert!(g.window_closed().is_empty(), "no trade: no check");
        let at = g.market_here().unwrap().iter().position(|i| i.0 == 24).unwrap();
        g.buy(at).unwrap();
        let fired = |ev: &[Event]| ev.iter().any(|e| matches!(e, Event::Script(EventOutcome::Fired { event: 1, .. })));
        assert!(fired(&g.window_closed()), "the amulet bought: the event opens as the window closes");
        assert!(g.window_closed().is_empty(), "asked once");
    }

    #[test]
    fn midnight_draws_building_by_building_its_market_then_its_barracks() {
        // Building 0 a market, building 1 a castle whose militia (max 5) grow when
        // Random(10 div 5) is 0: that draw comes after the market's restock.
        let mut s = shop_town(2);
        let mut castle = town(BuildingType::Castle, 10, 2, 1);
        castle.barracks[0] = RecruitSlot { unit: 4, start_count: 0, max_count: 5 };
        s.buildings.push(castle);
        let mut order_shows = false;
        for state in 1..100 {
            // Both restocks due.
            let due = |g: &mut Game| g.world.locations[0].shop.as_mut().unwrap().timer = 1;
            let mut g = start(&s);
            due(&mut g);
            g.rng = crate::rules::rng::Rng::new(state);
            g.economy_midnight();
            let mut want = start(&s);
            due(&mut want);
            want.rng = crate::rules::rng::Rng::new(state);
            want.restock_market(0);
            want.restock_market(1);
            let grows = want.rng.random(2) == 0;
            assert_eq!(g.world.locations[1].recruits[0].stock, Some(if grows { 1 } else { 0 }), "state {state}");
            assert_eq!(g.rng.state(), want.rng.state());
            order_shows |= grows != (crate::rules::rng::Rng::new(state).random(2) == 0);
        }
        assert!(order_shows, "some states grow only in one order");
    }

    #[test]
    fn markets_stock_fixed_goods_and_random_items_in_their_price_range() {
        let g = inside(&shop_town(2));
        let stock = g.market_here().unwrap();
        // The count (3) includes the fixed amulet: two random goods.
        assert_eq!(stock.len(), 3);
        assert_eq!(stock[0], ItemId(24), "the fixed good in its place, whatever its price: nothing is sorted");
        // A market sells no potions: of the items between 50 and 400 (or 401), the sword
        // (100) and the ring (300).
        assert!(stock[1..].iter().all(|i| [20, 23].contains(&i.0)), "{stock:?}");
    }

    #[test]
    fn fixed_goods_do_not_come_back_but_the_random_ones_are_drawn_every_midnight() {
        let mut g = inside(&shop_town(2));
        g.gold = 100_000;
        let amulet = g.market_here().unwrap().iter().position(|&i| i == ItemId(24)).unwrap();
        g.buy(amulet).unwrap();
        assert_eq!(g.pack, vec![ItemId(24)]);
        g.buy(0).unwrap();
        assert_eq!(g.market_here().unwrap().len(), 1);
        g.pass_time(14.0 * 60.0, &mut Vec::new()); // 09:00 -> 23:00
        assert_eq!(g.market_here().unwrap().len(), 1, "not yet");
        g.pass_time(60.0, &mut Vec::new());
        let stock = g.market_here().unwrap();
        assert_eq!(stock.len(), 3, "no fixed goods left: three random ones");
        assert!(!stock.contains(&ItemId(24)), "the fixed amulet is sold for good");
    }

    #[test]
    fn a_restock_sets_its_timer_twelve_hours_on() {
        // Loaded at 20:00: the load's restock is due again at 08:00, so the first midnight
        // passes it by; the second redraws it.
        let mut s = shop_town(2);
        s.header.start_time = s.header.start_time / 1440 * 1440 + 20 * 60;
        let mut g = inside(&s);
        let now = g.clock.total_minutes() as u64;
        assert_eq!(g.world.locations[0].shop.as_ref().unwrap().timer, now + 720);
        let before = g.world.locations[0].shop.clone();
        g.pass_time(4.0 * 60.0 + 1.0, &mut Vec::new()); // past midnight
        assert_eq!(g.world.locations[0].shop, before, "not due");
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        // Redrawn in the slice that reached midnight: 12 hours from then.
        let midnight = (g.clock.total_minutes() as u64) / 1440 * 1440;
        let timer = g.world.locations[0].shop.as_ref().unwrap().timer;
        assert!((midnight + 720..midnight + 760).contains(&timer), "{timer}");
    }

    #[test]
    fn a_town_with_only_map_goods_gets_a_healing_potion_every_midnight() {
        let mut c = content();
        c.items.extend([98, 99, 100].map(|id| crate::rules::content::ArtefactDef { cost: 50, ..ck::item(id, ArtefactType::Potion) }));
        let c = Content::new(c.units.clone(), c.items.clone(), c.spells.clone(), Default::default(), crate::rules::formation::Formation::WIDE);
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, 1);
        (t.artifact_slots[0], t.artifact_slots[2]) = (20, 23);
        s.buildings = vec![t];
        let mut g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        g.location = Some(0);
        // No random goods: no restock at load, the map's goods in their places.
        assert_eq!(g.market_here().unwrap(), [ItemId(20), ItemId(23)]);
        // R = 0 − 2: 0 div 5 + 1 = 1 healing potion, into the first empty place, each midnight
        // (the timer stays 1); the previous one is dropped as a random good.
        g.pass_time(15.0 * 60.0 + 1.0, &mut Vec::new());
        let goods = g.market_here().unwrap();
        assert_eq!((goods.len(), goods[0], goods[2]), (3, ItemId(20), ItemId(23)));
        assert!((98..=100).contains(&goods[1].0), "{goods:?}");
        g.pass_time(24.0 * 60.0, &mut Vec::new());
        assert_eq!(g.market_here().unwrap().len(), 3);
        assert_eq!(g.world.locations[0].shop.as_ref().unwrap().timer, 1);
    }

    #[test]
    fn a_map_good_of_negative_cost_pays_its_buyer() {
        let mut c = content();
        c.items.push(crate::rules::content::ArtefactDef { cost: -100, ..ck::item(40, ArtefactType::Ring) });
        let c = Arc::new(Content::new(c.units.clone(), c.items.clone(), c.spells.clone(), Default::default(), crate::rules::formation::Formation::WIDE));
        let mut s = map();
        let mut t = town(BuildingType::Market, 2, 2, 0);
        t.artifact_slots[0] = 40;
        s.buildings = vec![t];
        let mut g = Game::from_scenario(c, &s, HeroClass::Knight);
        g.location = Some(0);
        // Round(−100 × 1.1) = −110 (0x4b9e18 takes the sign off the goods id, not the Cost).
        assert_eq!(g.buy_price(ItemId(40)), -110);
        let gold = g.gold;
        g.buy(0).unwrap();
        assert_eq!(g.gold, gold + 110);
        assert!(!g.can_sell(ItemId(40)), "and it cannot be sold back");
    }

    #[test]
    fn the_map_load_sets_the_price_window() {
        use crate::rules::world::Shop;
        // The top capped at the dearest item, the bottom 0 unless below the top; 0 means
        // the dearest.
        assert_eq!(Shop::from_map(vec![], 3, (50, 400), 1000).price, (50, 400));
        assert_eq!(Shop::from_map(vec![], 3, (1000, 2000), 1000).price, (0, 1000));
        assert_eq!(Shop::from_map(vec![], 3, (200, 0), 1000).price, (0, 1000));
        assert_eq!(Shop::from_map(vec![], 0, (200, 0), 1000).price, (200, 0), "only with random goods");
    }

    #[test]
    fn churches_sell_amulets_and_potions_but_no_death_items() {
        let mut c = content();
        c.items.push(crate::rules::content::ArtefactDef { cost: 120, magic: Some(MagicSchool::Death), ..ck::item(30, ArtefactType::Amulet) });
        c.items.push(crate::rules::content::ArtefactDef { cost: 150, ..ck::item(31, ArtefactType::Amulet) });
        c.items.push(crate::rules::content::ArtefactDef { cost: 140, ..ck::item(32, ArtefactType::Potion) });
        let c = Arc::new(Content::new(c.units.clone(), c.items.clone(), c.spells.clone(), Default::default(), crate::rules::formation::Formation::WIDE));
        for (kind, want) in [(BuildingType::Church, vec![31, 32]), (BuildingType::Market, vec![20, 30]), (BuildingType::Town, vec![20, 31])] {
            let mut s = map();
            let mut t = town(kind, 2, 2, 1);
            t.random_artifacts_for_sale = 12;
            (t.price_min, t.price_max) = (100, 150);
            s.buildings = vec![t];
            let g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
            let mut goods: Vec<u32> = g.world.locations[0].shop.as_ref().unwrap().goods().iter().map(|i| i.0).filter(|&i| i < 95).collect();
            goods.sort();
            goods.dedup();
            assert_eq!(goods, want, "{kind:?}");
        }
    }

    #[test]
    fn towns_stock_healing_potions_first() {
        // Items 98–100 are the healing potions the exe gives towns, 95–97, 114 and 115 the
        // others.
        let mut c = content();
        c.items.extend([95, 96, 97, 98, 99, 100, 114, 115].map(|id| crate::rules::content::ArtefactDef { cost: 50, ..ck::item(id, ArtefactType::Potion) }));
        let c = Content::new(c.units.clone(), c.items.clone(), c.spells.clone(), Default::default(), crate::rules::formation::Formation::WIDE);
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, 1);
        t.random_artifacts_for_sale = 10;
        (t.price_min, t.price_max) = (1000, 2000);
        s.buildings = vec![t];
        let g = Game::from_scenario(Arc::new(c), &s, HeroClass::Knight);
        let stock = g.world.locations[0].shop.as_ref().unwrap().goods();
        // 10 div 5 + 1 = 3 potions, and 7 left (more than 6): one of them is one of the
        // others. The 7 others from the window, which the load opened to 0..1000 (its top
        // above the dearest item, 1000, and the bottom not below it): four items fit, each
        // once (the top is above 500) until the 26th try stocks the last drawn anyway.
        let healing = stock.iter().filter(|i| (98..=100).contains(&i.0)).count();
        let others = stock.iter().filter(|i| crate::rules::economy::TOWN_EXTRAS.contains(&i.0)).count();
        assert_eq!((healing, others, stock.len()), (2, 1, 10), "{stock:?}");
        assert_eq!(&stock[..3].iter().filter(|i| i.0 >= 95).count(), &3, "the potions first: {stock:?}");
        assert!(stock[3..].iter().all(|i| [20, 21, 23, 24].contains(&i.0)), "no potions from the window: {stock:?}");
    }

    #[test]
    fn prices_follow_relation_merchant_and_sale_percent() {
        for (attitude, want) in [(3, 750), (2, 900), (1, 1000), (0, 1100), (-1, 1250), (-2, 1450), (-3, 1700)] {
            let g = inside(&shop_town(attitude));
            assert_eq!(g.buy_price(ItemId(24)), want, "attitude {attitude}");
        }
        let mut s = shop_town(-2);
        s.header.heroes[0] = hero(2, 2, 5000, &[troop(7, 0, 1)]);
        let mut g = inside(&s);
        assert_eq!(g.buy_price(ItemId(24)), 1450 - 1450 * 30 / 100, "Merchant: −30%");
        g.pack = vec![ItemId(23), ItemId(25)];
        // Cost × ItemSaleCost × F / 10000 = 300 × 25 × 120 / 10000 = 90; Merchant + 45.
        assert_eq!(g.sell_price(ItemId(23)), 135);
        assert_eq!(g.sell(1), Err(crate::rules::game::TradeError::NotForSale), "worth 1: not sold");
        assert_eq!(g.sell(0), Ok(135));
        let g = inside(&shop_town(1));
        assert_eq!(g.sell_price(ItemId(23)), 90, "the relation does not count");
    }

    #[test]
    fn ill_disposed_buildings_trade_hire_and_pay_tribute() {
        // No attitude test anywhere (economy.md §7, §3).
        let g = inside(&shop_town(-1));
        assert_eq!(g.market_here().map(|g| g.len()), Some(3));
        let mut s = map();
        let mut t = town(BuildingType::Town, 2, 2, -1);
        t.barracks[0] = RecruitSlot { unit: 4, start_count: 3, max_count: 3 };
        let mut v = town(BuildingType::Village, 6, 2, -1);
        v.gold_per_day = 20;
        s.buildings = vec![t, v];
        let mut g = inside(&s);
        assert_eq!(g.recruits_here(), vec![UnitId(4)]);
        g.location = Some(1);
        assert_eq!(g.tribute_available(), Some(20));
    }

    #[test]
    fn walking_into_an_empty_hostile_fort_takes_it() {
        let mut s = map();
        let mut fort = town(BuildingType::Fort, 8, 2, -2);
        fort.gold_per_day = 25;
        s.buildings = vec![fort];
        let mut g = start(&s);
        assert!(g.set_destination(g.world.locations[0].tile));
        let mut events = Vec::new();
        for _ in 0..1000 {
            if !g.moving() {
                break;
            }
            events.extend(g.tick(0.05));
        }
        assert!(events.contains(&Event::Captured(0)), "{events:?}");
        assert!(g.world.locations[0].owned() && g.foe.is_none());
        assert_eq!(g.daily_income(), 30, "25 × F/100");
    }

    #[test]
    fn sanctuaries_teach_spells_for_gold_into_the_book() {
        let mut s = map();
        let mut church = town(BuildingType::Church, 2, 2, 1);
        church.spells_for_sale = [1, 2, 0, 0, 0, 0];
        s.buildings = vec![church];
        let mut g = inside(&s);
        assert_eq!(g.spells_here().iter().map(|sp| (sp.id, sp.cost_gold)).collect::<Vec<_>>(), [(1, 200), (2, 500)]);
        g.learn_spell(1).unwrap();
        assert_eq!((g.gold, g.spells.clone()), (800, vec![1]));
        assert_eq!(g.learn_spell(1), Err(ServiceError::AlreadyKnown));
        g.gold = 499;
        assert_eq!(g.learn_spell(2), Err(ServiceError::CannotAfford));
        g.gold = 500;
        g.spells = (3..18).collect();
        assert_eq!(g.learn_spell(2), Err(ServiceError::BookFull), "15 cells in the book");
        g.gold = 499;
        assert_eq!(g.learn_spell(2), Err(ServiceError::CannotAfford), "the gold is tested before the room");
        // A book an event filled past 15 is full too (the original's bug sold into it).
        g.gold = 500;
        g.spells = (3..19).collect();
        assert_eq!(g.learn_spell(2), Err(ServiceError::BookFull));
        g.gold = 0;
        g.spells = vec![1];
        assert_eq!(g.learn_spell(7), Err(ServiceError::NotHere));
        // Exactly CostGold is taken (0x4ba3b0): a negative one pays the hero.
        let mut c = (*g.content).clone();
        c.spells.iter_mut().find(|sp| sp.id == 1).unwrap().cost_gold = -50;
        g.content = std::sync::Arc::new(c);
        g.spells.clear();
        assert_eq!((g.learn_spell(1), g.gold), (Ok(()), 50));
    }

    #[test]
    fn a_dismissed_unit_takes_its_worn_items() {
        // Army_RemoveUnit moves nothing to the pack (0x4b1778); no refund, no cost.
        let mut g = start(&map());
        g.squad[1].items[0] = Some(ItemId(20));
        g.squad[1].named = 1;
        assert_eq!(g.dismiss(0), Err(ServiceError::Hero));
        let gold = g.gold;
        g.dismiss(1).unwrap();
        assert_eq!((g.squad.len(), g.pack.clone(), g.gold), (2, vec![], gold));
        g.squad[1].hp = 0;
        g.dismiss(1).unwrap();
        assert_eq!(g.squad.len(), 1, "a corpse is buried the same way");
    }

    #[test]
    fn ai_victory_gold_is_all_below_the_minimum_else_half() {
        let g = start(&map());
        // VictoryGoldDiv 2, MinVictoryGold 25: a threshold, not a floor.
        assert_eq!([0, 10, 24, 25, 30, 50, 120].map(|x| g.victory_gold(x)), [0, 10, 24, 12, 15, 25, 60]);
    }

    #[test]
    fn beating_an_army_whose_home_castle_is_empty_takes_the_castle() {
        let mut s = map();
        let mut castle = town(BuildingType::Castle, 12, 2, -2);
        castle.faction = 4;
        let mut foe = army(1, 20, 6, -2, &[]);
        foe.leader_unit = 9;
        foe.gold_income = 11;
        foe.home_building = 1;
        s.armies = vec![foe];
        s.buildings = vec![castle];
        let mut g = start(&s);
        assert_eq!(g.world.armies[0].home, Some(0));
        g.foe = Some(Foe::Army(0));
        let mut b = g.start_battle();
        b.begin();
        b.fighters.iter_mut().filter(|f| f.team == Team::Enemy).for_each(|f| f.hp = 0);
        let gold = g.world.armies[0].gold;
        let r = g.resolve_battle(&b);
        // Half its gold, no minimum; its leader draws no wage.
        assert!(matches!(r, BattleResult::Victory { reward, captured: Some(0), .. } if reward == gold / 2), "{r:?}");
        assert!(g.world.locations[0].owned());
    }

    #[test]
    fn capturing_a_fort_pays_a_day_of_income_and_surrender_mana_and_raises_income() {
        let mut s = map();
        let mut fort = town(BuildingType::Fort, 8, 2, -1);
        fort.gold_per_day = 30;
        fort.garrison[0] = troop(9, 0, 2);
        fort.garrison[1] = troop(6, 0, 1);
        s.buildings = vec![fort];
        let mut g = start(&s);
        g.location = Some(0);
        g.foe = Some(Foe::Garrison(0));
        let mut b: Battle = g.start_battle();
        b.begin();
        // The bandits fall; the remaining priest (Surrender 20) gives up after the next action.
        b.fighters.iter_mut().filter(|f| f.team == Team::Enemy && f.surrender == 0).for_each(|f| f.hp = 0);
        b.pass();
        let (gold, mana) = (g.gold, g.mana);
        let r = g.resolve_battle(&b);
        assert!(matches!(r, BattleResult::Victory { reward: 30, mana: 20, captured: Some(0), .. }), "{r:?}");
        assert_eq!((g.gold, g.mana), (gold + 30, mana + 20));
        assert_eq!(g.daily_income(), 36, "its income (× F/100) counts at once");
        assert_eq!(g.tabs_here(), vec![Tab::MainHall, Tab::Garrison], "no barracks slot, no hire tab");
    }
}

#[cfg(test)]
mod real_maps {
    //! Checks against the player's install; skipped without `RAZDOR_DT_DIR`. Only numbers
    //! and ids are compared, all read from the player's files.
    use std::sync::Arc;

    use super::*;
    use crate::dt::install::DtInstall;
    use crate::rules::content::{Content, HeroClass, ItemId, UnitId};
    use crate::rules::world::LocationKind;

    #[test]
    fn rk1_home_castle_and_the_first_friendly_church_offer_their_file_stock() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Arc::new(Content::from_dt(&dt));
        let s = dt.maps.iter().find(|m| m.name.starts_with("РК1")).unwrap().load().unwrap();
        let mut g = Game::from_scenario(c.clone(), &s, HeroClass::Knight);
        g.world.armies.clear();

        // Messages on the way stop the walk, and the fog hides the far ground: walk on.
        let walk_into = |g: &mut Game, l: usize| {
            for _ in 0..10 {
                if g.location == Some(l) {
                    break;
                }
                g.walk_through_fog(g.world.locations[l].tile);
            }
            assert_eq!(g.location, Some(l), "arrived");
        };

        // His own castle, next to his start: barracks with the file's stock, a garrison.
        let home = g.world.locations.iter().position(|l| l.owned() && l.kind == LocationKind::Castle).expect("a home castle");
        walk_into(&mut g, home);
        let b = &s.buildings[home];
        assert!(g.world.locations[home].owned());
        assert_eq!(g.tabs_here()[..3], [Tab::MainHall, Tab::Barracks, Tab::Garrison]);
        let file: Vec<(u32, i32, i32)> = b
            .barracks
            .iter()
            .filter(|r| r.unit != 0)
            .map(|r| (r.unit as u32, r.start_count as i32, r.max_count.max(r.start_count) as i32))
            .collect();
        let ours: Vec<(u32, i32, i32)> = g.world.locations[home].recruits.iter().map(|r| (r.unit.0, r.stock.unwrap(), r.max)).collect();
        assert_eq!(ours, file);

        // Walk into the nearest friendly building that hires.
        let (l, _) = g
            .world
            .nearest_location(g.tile(), |l| l.hires(&g.content) && !l.hostile() && !l.owned() && l.kind != LocationKind::Castle)
            .expect("a friendly town or church");
        walk_into(&mut g, l);
        let b = &s.buildings[l];
        let tabs = g.tabs_here();
        assert!(tabs.contains(&Tab::Barracks));
        let offered: Vec<UnitId> = b.barracks.iter().filter(|r| r.unit != 0 && r.start_count > 0).map(|r| UnitId(r.unit as u32)).collect();
        assert_eq!(g.recruits_here(), offered);
        if b.random_artifacts_for_sale > 0 || b.artifacts().next().is_some() {
            assert!(tabs.contains(&Tab::Market));
            let stock = g.market_here().unwrap();
            let fixed: Vec<ItemId> = b.artifact_slots[..12].iter().filter(|&&i| i != 0).map(|&i| ItemId(i as u32)).filter(|&i| c.try_item(i).is_some()).collect();
            // The building's count includes the fixed goods; towns add healing potions first.
            let mut r = b.random_artifacts_for_sale as i32 - fixed.len() as i32;
            if g.world.locations[l].kind == LocationKind::Town {
                r -= r / 5 + 1;
            }
            assert!(fixed.iter().all(|i| stock.contains(i)));
            assert!(stock.len() <= 12, "{stock:?}");
            let potions = |i: &ItemId| (98..=100).contains(&i.0) || crate::rules::economy::TOWN_EXTRAS.contains(&i.0);
            // With more than one good to draw the bands stay in the window the load set.
            let shop = g.world.locations[l].shop.as_ref().unwrap();
            let (lo, hi) = (shop.price.0.max(5), shop.price.1.min(5000) + 1);
            for item in stock.iter().filter(|i| r > 1 && !fixed.contains(i) && !potions(i)) {
                let cost = c.item(*item).cost;
                assert!((lo.min(hi)..=hi).contains(&cost), "{cost} outside the range");
            }
            let loc = &g.world.locations[l];
            for &item in &stock {
                let want = crate::rules::economy::relation_price(c.item(item).cost, loc.attitude, loc.owned());
                let want = if g.squad.iter().any(|u| u.alive() && u.stats(&c).has(&crate::rules::content::Bonus::Merchant)) { crate::rules::economy::merchant_price(want) } else { want };
                assert_eq!(g.buy_price(item), want);
            }
        }
        // "Impossible difficulty" is on in this install: F = 100.
        assert_eq!(g.difficulty(), 100);
        let wounded = g.squad.iter().position(|u| u.alive()).unwrap();
        let u = &mut g.squad[wounded];
        let max = u.max_hp(&c);
        u.hp = max / 2;
        let cost = c.unit(u.def).cost as i64;
        let want = crate::rules::economy::round_ratio((max - max / 2) as i64 * cost * c.options.healing_const as i64, max as i64 * 100).max(1);
        if g.heals_here() {
            assert_eq!(g.heal_price(wounded).map(|p| p.amount as i64), Some(want));
        }
        let spells: Vec<u32> = b.spells_for_sale.iter().filter(|&&x| x != 0).map(|&x| x as u32).collect();
        assert_eq!(g.spells_here().iter().map(|sp| sp.id).collect::<Vec<_>>(), spells);
        assert_eq!(tabs.contains(&Tab::Sanctuary), !spells.is_empty());
    }
}
