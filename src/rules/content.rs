//! Game content: unit, item and spell definitions plus the battle and economy options.
//!
//! The rules only ever see a [`Content`]. It is built either from the player's Discord Times
//! install ([`Content::from_dt`]) or from the built-in demo data in `data/` ([`Content::builtin`]),
//! which is our own content written in the same ini schema. Definitions reuse the reader types
//! of [`crate::dt::data`]; field meanings are in `docs/reference/mechanics.md`.

use std::collections::{BTreeMap, HashMap};

pub use crate::dt::data::{
    ArtefactDef, ArtefactType, Bonus, Build, DrainRule, GlobalOptions, MagicDirection, MagicSchool, Nature, SpellDef, Stat,
    StatMods, UnitDef, Upgrade,
};
use crate::dt::data::{parse_artefacts, parse_spells, parse_units};
use crate::dt::ini::Ini;
use crate::dt::install::DtInstall;

use super::formation::Formation;

const BUILTIN_UNITS: &str = include_str!("../../data/units.ini");
const BUILTIN_ITEMS: &str = include_str!("../../data/items.ini");
const BUILTIN_SPELLS: &str = include_str!("../../data/spells.ini");

/// Community cap on the XP a unit gains from one battle (experience.md §3).
pub const MAX_XP_GAIN: i32 = super::experience::MAX_BATTLE_XP;

/// A unit type: its `GlobalIndex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct UnitId(pub u32);

/// An item type: its `GlobalIndex`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
pub struct ItemId(pub u32);

/// The three hero classes. Their unit ids are 1–3 in the original and in the demo.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum HeroClass {
    /// Army takes 10% less physical damage.
    Knight,
    /// Casts world spells faster and cheaper.
    Archmage,
    /// Faster on the map, heals the army daily.
    Ranger,
}

impl HeroClass {
    pub const ALL: [HeroClass; 3] = [HeroClass::Knight, HeroClass::Archmage, HeroClass::Ranger];

    pub fn unit(self) -> UnitId {
        UnitId(match self {
            HeroClass::Knight => 1,
            HeroClass::Archmage => 2,
            HeroClass::Ranger => 3,
        })
    }

    pub fn of_unit(id: UnitId) -> Option<HeroClass> {
        HeroClass::ALL.into_iter().find(|h| h.unit() == id)
    }
}

/// The hiring kinds of the wage code (original-mechanics/economy.md §1). The kind is kept
/// per unit; it does not depend on the unit type.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum WageKind {
    /// Kind 0: the hero and the AI army leaders. Never paid.
    Leader,
    /// Kind 1: `round(Cost / CostRecrutDiv × f)` with the cost brackets. Everything the
    /// player hires, the starting army, the garrisons and the AI's troops at load.
    Recruit,
    /// Kind 2: `Cost / CostMercenaryDiv`. Only an AI army hiring in a building it does not own.
    Mercenary,
    /// Kind 3: a unit an event added. Never paid.
    Event,
}

impl WageKind {
    /// The kind of a unit hired or present at load: a recruit, whatever its type (the
    /// original does not look at `Nature`).
    pub fn of(_def: &UnitDef) -> WageKind {
        WageKind::Recruit
    }

    /// Kinds 0 and 3 draw no wage.
    pub fn is_paid(self) -> bool {
        matches!(self, WageKind::Recruit | WageKind::Mercenary)
    }
}

/// Where an item can turn up in the demo world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Market,
    Loot,
    Tribute,
}

impl Source {
    fn parse(s: &str) -> Option<Source> {
        match s {
            "market" => Some(Source::Market),
            "loot" => Some(Source::Loot),
            "tribute" => Some(Source::Tribute),
            _ => None,
        }
    }
}

/// Rules chosen for one playthrough. Missing fields in older saves mean classic rules.
///
/// `unlimited_items` is the «Без ограничений на предметы» switch: any wearable item goes on any
/// living fighter while a cell is free (`items::slot_for`). Older saves had four item switches
/// (multiple rings, multiple amulets, multiple weapons, duplicate items); [`RulesWire`] maps
/// them to this one.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(from = "RulesWire", into = "RulesWire")]
pub struct ExperimentalRules {
    pub simultaneous_bonuses: bool,
    pub two_potions: bool,
    /// Six item cells per unit instead of the classic four (the «Шесть ячеек снаряжения»
    /// switch). Fixed for a playthrough; see [`ExperimentalRules::from`] for old saves.
    pub six_cells: bool,
    /// «Без ограничений на предметы»: no class, school, holy, one-weapon or one-per-kind check,
    /// and no duplicate check; potions and goods stay unwearable, the dead wear nothing but a
    /// reviving potion, and the crown keeps its own refusal (`items::slot_for`).
    pub unlimited_items: bool,
    /// «Бесплатные юниты»: the player's units ask no daily wages, gold or mana
    /// (`Game::unit_wage`). Hiring, healing and refunds keep their prices.
    pub free_units: bool,
    /// «Автоматическая прокачка»: a player's unit takes an open upgrade-tree promotion at once,
    /// to a random option (`Game::auto_promote_squad`).
    pub auto_promotion: bool,
    /// «Юниты не умирают»: the player's dead units come back with 1 HP after a battle that
    /// their army survives (`Battle::player_results`), as the hero does. A map spell on the
    /// army (`Game::spell_on_player`, the enemy's or an event's) is covered the same way.
    pub undying_units: bool,
    /// «Ускоренный ИИ» («Faster AI»): on maps of 300×300 cells or more, each AI plan floods only a
    /// square window around the army (`ai::window`); and a plan with exactly one target cell is an
    /// A* path to it (`ai::direct`) instead of the flood. Armies move a little differently from the
    /// original. See `ai::search_for`.
    pub fast_ai: bool,
    /// «Благословение раз в ход» («Bless once a turn»): a unit is blessed at most once a turn
    /// (`Battle::option_at`, `friendly_case`, the AI's bless pick). Off: the original's rule
    /// (48555b, 4857fb → 484b64, 48a8d3–48a93c), where blessings stack. Razdor's rule: on.
    pub bless_once: bool,
    /// «Яд не действует на нежить» («Poison spares the Undead»): no poison takes hold on an
    /// Undead unit, and the AI counts none (`Battle::poisonable`). Off: the original poisons every
    /// nature. Razdor's rule: on.
    pub undead_poison_immune: bool,
    /// «Без предела книги заклинаний» («No spell book limit»): every spell of the book is
    /// castable, and the shop sells into any book. Off: only the first 15 spells can be cast, and
    /// the shop refuses at exactly 15 (`magic::check_cast`, `town::learn_spell`). Razdor's rule: on.
    pub unlimited_spells: bool,
    /// «Скидка торговца от предмета» («Merchant discount from an item»): a Merchant item in the
    /// hero's pack gives the Merchant prices (`Game::has_merchant`). Off: only a unit of the army
    /// with the bonus does. Razdor's rule: on.
    pub merchant_item: bool,
    /// «Интендантство всегда −30%» («Rear Service always −30%»): Rear Service cuts every bill by
    /// ×178/256 whatever the player's stored income. Off: ×78/256 while that income is 0
    /// (`Game::rear_service_factor`). Razdor's rule: on.
    pub rear_service_flat: bool,
    /// «Склад в своих зданиях» («Store in your own buildings»): the owner's store tab in a
    /// building he owns (`Tab::Store`, `Game::take_from_store`) shows the loot his garrison could
    /// not wear and takes it into the pack. Off: no store tab, nothing is taken out. The original
    /// never lets the owner take items out of his own building (only 0x4c50ec loots a building
    /// the hero beat, 0x4a473c keeps the loot in the record), so off is the original's rule.
    /// Razdor's own (the owner's request), on in old saves.
    pub building_store: bool,
}

impl ExperimentalRules {
    pub fn for_new_game() -> Self { Self::default() }

    /// The rules of a save with no `experimental` block at all (before the rules were saved):
    /// the five parity switches were Razdor's rules then, so they stay on; the eight others
    /// were off.
    pub fn before_the_rules_were_saved() -> Self {
        Self { bless_once: true, undead_poison_immune: true, unlimited_spells: true, merchant_item: true, rear_service_flat: true, building_store: true, ..Self::default() }
    }

    /// The first rule that is on, by its catalog name (the one the scenario list's yellow mark
    /// gives as its reason, `rules::marks`); `None` with the classic rules.
    pub fn first_on(&self) -> Option<&'static str> {
        [
            (self.simultaneous_bonuses, crate::i18n::n_("All special bonuses simultaneously")),
            (self.unlimited_items, crate::i18n::n_("No item limits")),
            (self.two_potions, crate::i18n::n_("At most two potions per character")),
            (self.six_cells, crate::i18n::n_("Six equipment cells")),
            (self.free_units, crate::i18n::n_("Free units")),
            (self.auto_promotion, crate::i18n::n_("Automatic promotion")),
            (self.undying_units, crate::i18n::n_("Units don't die")),
            (self.fast_ai, crate::i18n::n_("Faster AI")),
            (self.bless_once, crate::i18n::n_("Bless once a turn")),
            (self.undead_poison_immune, crate::i18n::n_("Poison spares the Undead")),
            (self.unlimited_spells, crate::i18n::n_("No spell book limit")),
            (self.merchant_item, crate::i18n::n_("Merchant discount from an item")),
            (self.rear_service_flat, crate::i18n::n_("Rear Service always −30%")),
            (self.building_store, crate::i18n::n_("Store in your own buildings")),
        ]
        .into_iter()
        .find(|(on, _)| *on)
        .map(|(_, name)| name)
    }
}

/// The save's form of [`ExperimentalRules`]. Written: the current fields only. Read: also the
/// four item switches of older builds (`skip_serializing`), which map to `unlimited_items`.
#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct RulesWire {
    simultaneous_bonuses: bool,
    two_potions: bool,
    /// Missing in saves from before the switch: a save with the old rings or amulets rule had six
    /// cells, so that rule's old meaning is kept (see the `From` impl).
    six_cells: Option<bool>,
    unlimited_items: bool,
    free_units: bool,
    auto_promotion: bool,
    undying_units: bool,
    /// Missing (false) in saves from before the planner switch.
    fast_ai: bool,
    /// The five parity switches. Missing in a save from before them: the rule was Razdor's then,
    /// so it reads ON (see the `From` impl). A new game writes them all.
    bless_once: Option<bool>,
    undead_poison_immune: Option<bool>,
    unlimited_spells: Option<bool>,
    merchant_item: Option<bool>,
    rear_service_flat: Option<bool>,
    /// Missing in a save from before the store switch: the store was Razdor's then, so it reads ON.
    building_store: Option<bool>,
    /// Older switches, read only (see the `From` impl).
    #[serde(skip_serializing)]
    multiple_rings: bool,
    #[serde(skip_serializing)]
    multiple_amulets: bool,
    #[serde(skip_serializing)]
    multiple_weapons: bool,
    #[serde(skip_serializing)]
    allow_duplicate_items: Option<bool>,
    /// The inverse of `allow_duplicate_items` in saves from before that switch.
    #[serde(skip_serializing)]
    no_duplicate_items: Option<bool>,
}

impl From<RulesWire> for ExperimentalRules {
    fn from(w: RulesWire) -> Self {
        // Duplicates allowed by an older save (explicitly: the switch on, or its inverse off).
        // A save without either key had the classic restriction and stays so.
        let old_duplicates_allowed = match (w.allow_duplicate_items, w.no_duplicate_items) {
            (Some(allow), _) => Some(allow),
            (None, Some(no_duplicate)) => Some(!no_duplicate),
            (None, None) => None,
        } == Some(true);
        // Before the switch, six cells came with the multiple rings or amulets rule; a save
        // without the key keeps the six cells it had, so no worn item is lost.
        let six_cells = w.six_cells.unwrap_or(w.multiple_rings || w.multiple_amulets);
        // The four switches of older builds become the one: any of them on (the weapon or the
        // duplicate rule lifted, or a repeated ring or amulet) is the lifted rules now.
        let unlimited_items = w.unlimited_items || w.multiple_rings || w.multiple_amulets || w.multiple_weapons || old_duplicates_allowed;
        Self {
            simultaneous_bonuses: w.simultaneous_bonuses,
            two_potions: w.two_potions,
            six_cells,
            unlimited_items,
            free_units: w.free_units,
            auto_promotion: w.auto_promotion,
            undying_units: w.undying_units,
            fast_ai: w.fast_ai,
            bless_once: w.bless_once.unwrap_or(true),
            undead_poison_immune: w.undead_poison_immune.unwrap_or(true),
            unlimited_spells: w.unlimited_spells.unwrap_or(true),
            merchant_item: w.merchant_item.unwrap_or(true),
            rear_service_flat: w.rear_service_flat.unwrap_or(true),
            building_store: w.building_store.unwrap_or(true),
        }
    }
}

impl From<ExperimentalRules> for RulesWire {
    fn from(r: ExperimentalRules) -> Self {
        Self {
            simultaneous_bonuses: r.simultaneous_bonuses,
            two_potions: r.two_potions,
            six_cells: Some(r.six_cells),
            unlimited_items: r.unlimited_items,
            free_units: r.free_units,
            auto_promotion: r.auto_promotion,
            undying_units: r.undying_units,
            fast_ai: r.fast_ai,
            bless_once: Some(r.bless_once),
            undead_poison_immune: Some(r.undead_poison_immune),
            unlimited_spells: Some(r.unlimited_spells),
            merchant_item: Some(r.merchant_item),
            rear_service_flat: Some(r.rear_service_flat),
            building_store: Some(r.building_store),
            multiple_rings: false,
            multiple_amulets: false,
            multiple_weapons: false,
            allow_duplicate_items: None,
            no_duplicate_items: None,
        }
    }
}

/// Everything the rules need to know about units, items, spells and options.
#[derive(Clone, Debug)]
pub struct Content {
    /// Runtime copy of the playthrough's rules, shared by every unit and army.
    pub experimental: ExperimentalRules,
    pub units: Vec<UnitDef>,
    pub items: Vec<ArtefactDef>,
    pub spells: Vec<SpellDef>,
    /// `_Global.ini` values: magic divisors, drain and floors, `Row2Def`, `BattleEndTurn`,
    /// XP constants, wage divisors, loot, sale price …
    pub options: GlobalOptions,
    pub formation: Formation,
    unit_index: HashMap<u32, usize>,
    item_index: HashMap<u32, usize>,
    tactical: TacticalCache,
}

/// Tactical cost by (unit, level): the AI asks for it all the time. A clone starts empty,
/// so a changed copy never sees stale values.
#[derive(Debug, Default)]
struct TacticalCache(std::sync::Mutex<HashMap<(u32, i32), i32>>);

impl Clone for TacticalCache {
    fn clone(&self) -> Self {
        TacticalCache::default()
    }
}

/// No units, items or spells: the placeholder a loaded save holds until its content is set.
impl Default for Content {
    fn default() -> Content {
        Content::new(Vec::new(), Vec::new(), Vec::new(), GlobalOptions::default(), Formation::WIDE)
    }
}

impl Content {
    pub fn new(
        units: Vec<UnitDef>,
        items: Vec<ArtefactDef>,
        spells: Vec<SpellDef>,
        options: GlobalOptions,
        formation: Formation,
    ) -> Content {
        let unit_index = units.iter().enumerate().map(|(i, u)| (u.id, i)).collect();
        let item_index = items.iter().enumerate().map(|(i, a)| (a.id, i)).collect();
        Content { experimental: ExperimentalRules::default(), units, items, spells, options, formation, unit_index, item_index, tactical: TacticalCache::default() }
    }

    /// An isolated copy for a playthrough, never changing the install's shared content.
    pub fn with_experimental(&self, experimental: ExperimentalRules) -> Content {
        Content { experimental, tactical: TacticalCache::default(), ..self.clone() }
    }

    /// The same content fought in `formation`.
    pub fn with_formation(&self, formation: Formation) -> Content {
        Content { formation, tactical: TacticalCache::default(), ..self.clone() }
    }

    /// Content read from a Discord Times install. The formation is the Community wide row
    /// (2 × 6) when the player's `OptValue11` is on (as in his install), else the vanilla
    /// 3 × 4 (0x4b8974: 6 or 4 per row).
    ///
    /// Battle XP is paid at the install's `HeroExpirienceModificator`, as the original does
    /// (0x4ed3f0; the Community Update ships 50, which halves every gain).
    pub fn from_dt(dt: &DtInstall) -> Content {
        let options = dt.options.clone();
        let formation = if dt.settings.wide_row { Formation::WIDE } else { Formation::VANILLA };
        Content::new(dt.units.clone(), dt.artefacts.clone(), dt.spells.clone(), options, formation)
    }

    /// [`Content::from_dt`] with the names and descriptions in the data language `data` (the
    /// prefix of its files, `Eng` for English; `None`: the install's Russian) where the
    /// install has its texts, English where another language's files lack one
    /// (`crate::dt::english`; Razdor's own, not the original's).
    pub fn from_dt_in(dt: &DtInstall, data: Option<&str>) -> Content {
        let mut c = Content::from_dt(dt);
        c.translate_data(dt, data);
        c
    }

    /// This content with the names and descriptions of `dt`'s data in Russian, or in the data
    /// language `data` (as [`Content::from_dt_in`]); everything else kept (a running game
    /// after the language was changed).
    pub fn retitled(&self, dt: &DtInstall, data: Option<&str>) -> Content {
        let mut c = self.clone();
        for u in &mut c.units {
            if let Some(d) = dt.units.iter().find(|d| d.id == u.id) {
                (u.name, u.description) = (d.name.clone(), d.description.clone());
            }
        }
        for a in &mut c.items {
            if let Some(d) = dt.artefacts.iter().find(|d| d.id == a.id) {
                (a.name, a.description) = (d.name.clone(), d.description.clone());
            }
        }
        for s in &mut c.spells {
            if let Some(d) = dt.spells.iter().find(|d| d.id == s.id) {
                s.name = d.name.clone();
            }
        }
        c.translate_data(dt, data);
        c
    }

    /// The Russian names and descriptions swapped for language `data`'s, then, for a language
    /// other than English, the ones it lacks for English (the interface is English then).
    fn translate_data(&mut self, dt: &DtInstall, data: Option<&str>) {
        let Some(p) = data else { return };
        if let Some(d) = dt.dictionary(p) {
            d.apply(&mut self.units, &mut self.items, &mut self.spells);
        }
        if !p.eq_ignore_ascii_case(crate::dt::english::ENGLISH) {
            if let Some(d) = dt.english.as_ref() {
                d.apply(&mut self.units, &mut self.items, &mut self.spells);
            }
        }
    }

    /// The built-in demo: `data/units.ini`, `data/items.ini` and `data/spells.ini` (our own
    /// content). The options are the vanilla defaults except for faster XP (the player's
    /// modifier and the difficulty factor at 100%), so the short demo shows levels.
    pub fn builtin() -> Content {
        let mut units = parse_units(&Ini::parse(BUILTIN_UNITS)).strict().unwrap_or_else(|e| panic!("data/units.ini: {e}"));
        let mut items = parse_artefacts(&Ini::parse(BUILTIN_ITEMS)).strict().unwrap_or_else(|e| panic!("data/items.ini: {e}"));
        let mut spells = parse_spells(&Ini::parse(BUILTIN_SPELLS)).strict().unwrap_or_else(|e| panic!("data/spells.ini: {e}"));
        // The demo's texts in the interface language: `NameRu=` / `DescriptRu=` (our own).
        if crate::i18n::lang() == crate::i18n::Lang::Ru {
            let ru = |extra: &BTreeMap<String, String>, key: &str, text: &mut String| {
                if let Some(t) = extra.get(key) {
                    t.clone_into(text);
                }
            };
            for u in &mut units {
                ru(&u.extra, "NameRu", &mut u.name);
                ru(&u.extra, "DescriptRu", &mut u.description);
            }
            for a in &mut items {
                ru(&a.extra, "NameRu", &mut a.name);
                ru(&a.extra, "DescriptRu", &mut a.description);
            }
            for s in &mut spells {
                ru(&s.extra, "NameRu", &mut s.name);
            }
        }
        let options = GlobalOptions { hero_experience_modificator: 100, difficulty_factor: 100, ..GlobalOptions::default() };
        Content::new(units, items, spells, options, Formation::WIDE)
    }

    pub fn try_unit(&self, id: UnitId) -> Option<&UnitDef> {
        self.unit_index.get(&id.0).map(|&i| &self.units[i])
    }

    /// Unit definition. Panics on an unknown id (ids come from this content).
    pub fn unit(&self, id: UnitId) -> &UnitDef {
        self.try_unit(id).unwrap_or_else(|| panic!("no unit {}", id.0))
    }

    pub fn try_item(&self, id: ItemId) -> Option<&ArtefactDef> {
        self.item_index.get(&id.0).map(|&i| &self.items[i])
    }

    /// Item definition. Panics on an unknown id.
    pub fn item(&self, id: ItemId) -> &ArtefactDef {
        self.try_item(id).unwrap_or_else(|| panic!("no item {}", id.0))
    }

    /// Spell definition by its 1-based id.
    pub fn spell(&self, id: u32) -> Option<&SpellDef> {
        self.spells.iter().find(|s| s.id == id)
    }

    pub fn unit_ids(&self) -> impl Iterator<Item = UnitId> + '_ {
        self.units.iter().map(|u| UnitId(u.id))
    }

    pub fn item_ids(&self) -> impl Iterator<Item = ItemId> + '_ {
        self.items.iter().map(|a| ItemId(a.id))
    }

    /// Stable short name: the demo's `Key=`, else `unit<id>`.
    pub fn unit_key(&self, id: UnitId) -> String {
        self.unit(id).extra.get("Key").cloned().unwrap_or_else(|| format!("unit{}", id.0))
    }

    /// Stable short name: the demo's `Key=`, else `item<id>`.
    pub fn item_key(&self, id: ItemId) -> String {
        self.item(id).extra.get("Key").cloned().unwrap_or_else(|| format!("item{}", id.0))
    }

    pub fn unit_by_key(&self, key: &str) -> Option<UnitId> {
        self.unit_ids().find(|&id| self.unit_key(id) == key)
    }

    pub fn item_by_key(&self, key: &str) -> Option<ItemId> {
        self.item_ids().find(|&id| self.item_key(id) == key)
    }

    /// Where an item turns up. The demo lists it (`Sources=`); for an install: potions
    /// everywhere, trade goods as loot, other priced items in markets and as loot, personal
    /// (negative price) items nowhere.
    pub fn sources(&self, id: ItemId) -> Vec<Source> {
        let def = self.item(id);
        if let Some(list) = def.extra.get("Sources") {
            return list.split_whitespace().filter_map(Source::parse).collect();
        }
        match def.kind {
            _ if def.cost <= 0 => Vec::new(),
            ArtefactType::Potion => vec![Source::Market, Source::Loot, Source::Tribute],
            ArtefactType::Item => vec![Source::Loot],
            _ => vec![Source::Market, Source::Loot],
        }
    }

    /// The highest item `Cost` (0x4ed474, the item loader's), at least 0.
    pub fn dearest_item(&self) -> i32 {
        self.items.iter().map(|a| a.cost).max().unwrap_or(0).max(0)
    }

    pub fn items_from(&self, source: Source) -> Vec<ItemId> {
        self.item_ids().filter(|&i| self.sources(i).contains(&source)).collect()
    }

    /// Gold a hero class starts with (demo `StartGold=`, else 100).
    pub fn start_gold(&self, hero: HeroClass) -> i32 {
        self.try_unit(hero.unit()).and_then(|u| u.extra.get("StartGold")).and_then(|g| g.parse().ok()).unwrap_or(100)
    }

    /// Spells a demo hero class has in its book at the start (demo `StartSpells=`).
    pub fn start_spells(&self, hero: HeroClass) -> Vec<u8> {
        let list = self.try_unit(hero.unit()).and_then(|u| u.extra.get("StartSpells"));
        list.map_or_else(Vec::new, |l| l.split_whitespace().filter_map(|x| x.parse().ok()).collect())
    }

    /// XP needed to go from `level` (1 = as hired) to the next:
    /// `round(StartExpirience × (LevelMultipler/100)^(level−1))` (experience.md §2).
    pub fn xp_to_next(&self, id: UnitId, level: i32) -> i32 {
        let u = self.unit(id);
        super::experience::xp_to_next(u.start_experience, u.level_multiplier, level)
    }

    /// Tactical cost of a type at `level` with no items and no building: the strength of
    /// its level stats × `CostMultipler`/100 (experience.md §1).
    pub fn tactical_cost(&self, id: UnitId, level: i32) -> i32 {
        let key = (id.0, level.max(1));
        if let Some(&v) = self.tactical.0.lock().ok().and_then(|m| m.get(&key).copied()).as_ref() {
            return v;
        }
        let s = super::units::Stats::of_level(self, id, key.1);
        let v = super::experience::tactical(self, id, &s, 0);
        if let Ok(mut m) = self.tactical.0.lock() {
            m.insert(key, v);
        }
        v
    }

    /// Daily wage of a unit type hired as `kind` (mechanics.md 1.5). The hero is free; the
    /// caller handles that and `AddPayment`.
    pub fn wage_for(&self, id: UnitId, kind: WageKind) -> i32 {
        match kind {
            WageKind::Recruit => self.wage(id),
            WageKind::Mercenary => self.unit(id).cost.max(0) / self.options.cost_mercenary_div.max(1),
            WageKind::Leader | WageKind::Event => 0,
        }
    }

    /// Community Update: `Nature=Elemental` units are hired, healed, resurrected and paid in
    /// mana instead of gold.
    pub fn paid_in_mana(&self, id: UnitId) -> bool {
        self.unit(id).nature == Nature::Elemental
    }

    /// Daily wage of a unit type (mechanics.md 1.5, hiring kind 1):
    /// `Round(Cost / CostRecrutDiv × f)`, f = 0.25 / 0.5 / 0.75 / 1 for cost ≤50 / ≤100 / ≤150 / more.
    /// The hero is free; the caller handles that and `AddPayment`.
    pub fn wage(&self, id: UnitId) -> i32 {
        let c = self.unit(id).cost.max(0);
        let quarters = match c {
            0..=50 => 1,
            51..=100 => 2,
            101..=150 => 3,
            _ => 4,
        };
        let div = self.options.cost_recrut_div.max(1);
        // Round(c / div × quarters / 4), halves to even as Delphi's Round does (22.5 → 22).
        super::economy::round_ratio((c * quarters) as i64, (4 * div) as i64) as i32
    }
}

/// Helpers for tests: blank definitions and a synthetic content.
#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use std::collections::BTreeMap;

    pub fn unit(id: u32, name: &str) -> UnitDef {
        UnitDef {
            id,
            name: name.to_string(),
            description: String::new(),
            icon_index: 0,
            cost: 50,
            cost_multiplier: 100,
            cost_gold_div: 1,
            start_experience: 60,
            level_multiplier: 140,
            hits: 50,
            attack_blow: 0,
            attack_shot: 0,
            magic_power: 0,
            defence_blow: 0,
            defence_shot: 0,
            protect_life: 0,
            protect_death: 0,
            protect_elemental: 0,
            initiative: 10,
            manevres: 1,
            regen: 0,
            vampirism: 0,
            magic: None,
            magic_direction: None,
            nature: Nature::Normal,
            bonus: None,
            surrender: 0,
            upgrades: Vec::new(),
            level_up: StatMods::new(),
            evasion: None,
            min_magic_power: None,
            mana_drain: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn warrior(id: u32, atk: i32, def: i32) -> UnitDef {
        UnitDef { attack_blow: atk, defence_blow: def, defence_shot: def, ..unit(id, &format!("warrior{id}")) }
    }

    pub fn shooter(id: u32, atk: i32) -> UnitDef {
        UnitDef { attack_shot: atk, ..unit(id, &format!("shooter{id}")) }
    }

    pub fn mage(id: u32, power: i32, school: MagicSchool, dir: MagicDirection) -> UnitDef {
        UnitDef { magic_power: power, magic: Some(school), magic_direction: Some(dir), ..unit(id, &format!("mage{id}")) }
    }

    pub fn item(id: u32, kind: ArtefactType) -> ArtefactDef {
        ArtefactDef {
            id,
            name: format!("item{id}"),
            description: String::new(),
            icon: String::new(),
            cost: 100,
            kind,
            bonus: None,
            magic: None,
            add: StatMods::new(),
            percent: StatMods::new(),
            fixed: StatMods::new(),
            extra: BTreeMap::new(),
        }
    }

    /// A spell with id `id` that costs `gold` to learn.
    pub fn spell(id: u32, gold: i32) -> SpellDef {
        SpellDef {
            id,
            name: format!("spell{id}"),
            cost_gold: gold,
            cost_mana: 50,
            school: Some(MagicSchool::Life),
            time_work: None,
            time_cast: Some(1),
            target: None,
            icons: Default::default(),
            effects: Default::default(),
            delta_fixed_hits: Some(10),
            delta_percent_hits: None,
            add: StatMods::new(),
            percent: StatMods::new(),
            life_lose_percent: None,
            extra: BTreeMap::new(),
        }
    }

    pub fn content(units: Vec<UnitDef>, items: Vec<ArtefactDef>) -> Content {
        Content::new(units, items, Vec::new(), GlobalOptions::default(), Formation::WIDE)
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;

    #[test]
    fn the_demo_has_russian_names_and_descriptions() {
        // Built in English (tests never switch the language): the Russian texts stay in `extra`.
        let c = Content::builtin();
        let russian = |s: Option<&String>| s.is_some_and(|s| s.chars().any(|ch| ('а'..='я').contains(&ch.to_lowercase().next().unwrap_or(ch))));
        for u in &c.units {
            assert!(russian(u.extra.get("NameRu")), "unit {} has no NameRu", u.name);
            assert!(u.description.is_empty() || russian(u.extra.get("DescriptRu")), "unit {} has no DescriptRu", u.name);
        }
        for a in &c.items {
            assert!(russian(a.extra.get("NameRu")), "item {} has no NameRu", a.name);
            assert!(a.description.is_empty() || russian(a.extra.get("DescriptRu")), "item {} has no DescriptRu", a.name);
        }
        for s in &c.spells {
            assert!(russian(s.extra.get("NameRu")), "spell {} has no NameRu", s.name);
        }
    }

    #[test]
    fn builtin_parses_our_demo_data() {
        let c = Content::builtin();
        assert_eq!(c.units.len(), 10);
        assert_eq!(c.items.len(), 15);
        assert_eq!(c.spells.len(), 5);
        assert!(c.spells.iter().all(|s| s.extra.keys().all(|k| k == "NameRu")), "unknown spell keys");
        for h in HeroClass::ALL {
            assert!(c.try_unit(h.unit()).is_some(), "{h:?}");
        }
        assert!(c.units.iter().all(|u| u.extra.keys().all(|k| matches!(k.as_str(), "Key" | "StartGold" | "StartSpells" | "NameRu" | "DescriptRu"))), "unknown unit keys");
        assert!(c.items.iter().all(|i| i.extra.keys().all(|k| matches!(k.as_str(), "Key" | "Sources" | "NameRu" | "DescriptRu"))), "unknown item keys");
        let spear = c.unit_by_key("spearman").unwrap();
        assert_eq!(c.unit(spear).bonus, Some(Bonus::SpearDefense));
        assert_eq!(c.unit(spear).upgrades[0].target, c.unit_by_key("swordsman").map(|u| u.0));
        assert_eq!(c.start_gold(HeroClass::Archmage), 120);
        assert_eq!(c.start_spells(HeroClass::Archmage), vec![1, 4]);
        assert!(c.start_spells(HeroClass::Knight).is_empty());
        for s in [Source::Market, Source::Loot, Source::Tribute] {
            assert!(!c.items_from(s).is_empty(), "{s:?}");
        }
        let candle = c.item_by_key("candlestick").unwrap();
        assert_eq!(c.item(candle).kind, ArtefactType::Item);
    }

    #[test]
    fn xp_to_next_level_is_geometric() {
        // 60, then ×1.4 each level: 60, 84, 118 (mechanics.md 1.4).
        let c = content(vec![unit(1, "militia")], vec![]);
        let need: Vec<i32> = (1..=4).map(|l| c.xp_to_next(UnitId(1), l)).collect();
        assert_eq!(need, vec![60, 84, 118, 165]);
    }

    #[test]
    fn wages_follow_the_cost_brackets() {
        let mut units = vec![];
        for (id, cost) in [(1, 50), (2, 100), (3, 150), (4, 280), (5, 0)] {
            units.push(UnitDef { cost, ..unit(id, "u") });
        }
        let c = content(units, vec![]);
        // 50/2×0.25 = 6.25, 100/2×0.5 = 25, 150/2×0.75 = 56.25, 280/2 = 140.
        let wages: Vec<i32> = (1..=5).map(|i| c.wage(UnitId(i))).collect();
        assert_eq!(wages, vec![6, 25, 56, 140, 0]);
        // Kind 2 (mercenary): Cost / CostMercenaryDiv, no brackets.
        let merc: Vec<i32> = (1..=5).map(|i| c.wage_for(UnitId(i), WageKind::Mercenary)).collect();
        assert_eq!(merc, vec![25, 50, 75, 140, 0]);
        assert_eq!(c.wage_for(UnitId(1), WageKind::Recruit), 6);
    }

    #[test]
    fn every_hired_unit_is_a_recruit_and_elementals_are_paid_in_mana() {
        let rogue = UnitDef { cost: 60, nature: Nature::Rogue, ..unit(1, "rogue") };
        let elemental = UnitDef { nature: Nature::Elemental, ..unit(2, "golem") };
        let c = content(vec![rogue, elemental, unit(3, "militia")], vec![]);
        assert_eq!(WageKind::of(c.unit(UnitId(1))), WageKind::Recruit, "Nature does not make a mercenary");
        assert_eq!(WageKind::of(c.unit(UnitId(3))), WageKind::Recruit);
        assert_eq!((c.wage_for(UnitId(1), WageKind::Leader), c.wage_for(UnitId(1), WageKind::Event)), (0, 0), "kinds 0 and 3 are free");
        assert!(c.paid_in_mana(UnitId(2)) && !c.paid_in_mana(UnitId(1)));
    }

    #[test]
    fn default_sources_for_install_items() {
        let mut personal = item(3, ArtefactType::Ring);
        personal.cost = -500;
        let c = content(vec![], vec![item(1, ArtefactType::Potion), item(2, ArtefactType::Armor), personal, item(4, ArtefactType::Item)]);
        assert_eq!(c.sources(ItemId(1)), vec![Source::Market, Source::Loot, Source::Tribute]);
        assert_eq!(c.sources(ItemId(2)), vec![Source::Market, Source::Loot]);
        assert!(c.sources(ItemId(3)).is_empty());
        assert_eq!(c.sources(ItemId(4)), vec![Source::Loot]);
    }

    #[test]
    fn real_install_content_loads() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dt = DtInstall::load(std::path::Path::new(&dir)).expect("install loads");
        let c = Content::from_dt(&dt);
        assert_eq!((c.units.len(), c.items.len(), c.spells.len()), (dt.units.len(), dt.artefacts.len(), dt.spells.len()));
        assert!(c.units.len() >= 100 && c.items.len() >= 150 && c.spells.len() >= 30);
        assert!(c.unit_ids().all(|id| c.try_unit(id).is_some()));
        assert!(c.item_ids().all(|id| c.try_item(id).is_some()));
        for h in HeroClass::ALL {
            assert!(c.try_unit(h.unit()).is_some());
        }
        assert_eq!(c.formation, Formation::WIDE);
        assert_eq!(c.options.battle_end_turn, 25);
        // Every unit's XP table and wage are computable.
        assert!(c.unit_ids().all(|id| c.xp_to_next(id, 1) > 0 && c.wage(id) >= 0));
        assert!(!c.items_from(Source::Market).is_empty());
    }

    #[test]
    fn real_xp_tables_and_strength() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let c = Content::from_dt(&DtInstall::load(std::path::Path::new(&dir)).expect("install loads"));
        for id in c.unit_ids() {
            // Positive, increasing XP steps.
            let need: Vec<i32> = (1..=12).map(|l| c.xp_to_next(id, l)).collect();
            assert!(need[0] > 0 && need.windows(2).all(|w| w[1] > w[0]), "unit {}: {need:?}", id.0);
            // Strength is positive at every level. It need not grow: a cannon (ranged attack
            // from ShotWeaponRange on) counts a third, and a warrior-priest whose magic
            // overtakes its melee is valued by its (weaker) spells.
            assert!((1..=8).all(|l| c.tactical_cost(id, l) >= 1), "unit {}", id.0);
        }
        // The footage: unit 28 fresh at 0/580, unit 14 at level 2 needs 560, hero 2 at
        // level 5 needs 590.
        assert_eq!((c.xp_to_next(UnitId(28), 1), c.xp_to_next(UnitId(14), 2), c.xp_to_next(UnitId(2), 5)), (580, 560, 590));
    }
}

#[cfg(test)]
mod experimental_rules_save_tests {
    use super::ExperimentalRules;

    /// A save from before the item switches with the classic restriction (no duplicates key
    /// set, or `no_duplicate_items` true) keeps the item rules classic: `unlimited_items` off.
    #[test]
    fn an_old_save_with_the_classic_duplicate_rule_stays_classic() {
        let on: ExperimentalRules = serde_json::from_str(r#"{"no_duplicate_items": true}"#).unwrap();
        assert!(!on.unlimited_items);
        let missing: ExperimentalRules = serde_json::from_str("{}").unwrap();
        assert!(!missing.unlimited_items);
    }

    /// Old saves with any of the lifted item rules (multiple rings, multiple amulets, multiple
    /// weapons, or duplicates allowed explicitly) load with `unlimited_items` on.
    #[test]
    fn an_old_save_with_a_lifted_item_rule_loads_unlimited_items() {
        for old in [
            r#"{"multiple_rings": true}"#,
            r#"{"multiple_amulets": true}"#,
            r#"{"multiple_weapons": true}"#,
            r#"{"allow_duplicate_items": true}"#,
            r#"{"no_duplicate_items": false}"#,
        ] {
            let rules: ExperimentalRules = serde_json::from_str(old).unwrap();
            assert!(rules.unlimited_items, "{old}");
        }
        let explicit_off: ExperimentalRules = serde_json::from_str(r#"{"multiple_rings": false, "allow_duplicate_items": false, "no_duplicate_items": true}"#).unwrap();
        assert!(!explicit_off.unlimited_items);
    }

    /// The whole old save shape from the 1.3.6 build (every switch written, the new-game
    /// defaults) maps to the new rules, and the old keys are not written back.
    #[test]
    fn an_old_1_3_6_save_maps_to_the_new_rules_and_is_rewritten_without_the_old_keys() {
        let old = r#"{"simultaneous_bonuses": true, "multiple_rings": true, "multiple_amulets": false,
            "allow_duplicate_items": false, "no_duplicate_items": true, "two_potions": true,
            "six_cells": true, "multiple_weapons": false, "free_units": true, "auto_promotion": false,
            "undying_units": false}"#;
        let rules: ExperimentalRules = serde_json::from_str(old).unwrap();
        assert!(rules.unlimited_items && rules.six_cells && rules.simultaneous_bonuses && rules.two_potions && rules.free_units);
        assert!(!rules.auto_promotion && !rules.undying_units);
        let json = serde_json::to_value(rules).unwrap();
        for gone in ["multiple_rings", "multiple_amulets", "multiple_weapons", "allow_duplicate_items", "no_duplicate_items"] {
            assert!(json.get(gone).is_none(), "{gone} is not written");
        }
        assert_eq!(json["unlimited_items"], true);
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), rules);
    }

    /// The defaults are classic (the switch off), and a save writes the switch explicitly.
    #[test]
    fn the_default_is_classic_and_the_save_writes_the_switch() {
        assert!(!ExperimentalRules::default().unlimited_items);
        assert_eq!(ExperimentalRules::for_new_game(), ExperimentalRules::default());
        let unlimited = ExperimentalRules { unlimited_items: true, ..Default::default() };
        let json = serde_json::to_value(unlimited).unwrap();
        assert_eq!(json["unlimited_items"], true);
        let back: ExperimentalRules = serde_json::from_value(json).unwrap();
        assert_eq!(back, unlimited);
    }

    /// A save from before the six-cell switch keeps the six cells it had: the rings or amulets
    /// rule used to bring them, so such a save maps to six cells; without either rule, four.
    #[test]
    fn an_old_save_without_the_key_maps_six_cells_from_the_rings_or_amulets_rules() {
        let rings: ExperimentalRules = serde_json::from_str(r#"{"multiple_rings": true}"#).unwrap();
        assert!(rings.six_cells && rings.unlimited_items);
        let amulets: ExperimentalRules = serde_json::from_str(r#"{"multiple_amulets": true, "two_potions": true}"#).unwrap();
        assert!(amulets.six_cells);
        let both_off: ExperimentalRules = serde_json::from_str(r#"{"multiple_rings": false, "multiple_amulets": false}"#).unwrap();
        assert!(!both_off.six_cells);
        let missing: ExperimentalRules = serde_json::from_str("{}").unwrap();
        assert!(!missing.six_cells);
    }

    /// A save written with the switch carries it explicitly, whatever the other switches say, so
    /// the mapping above never applies to it.
    #[test]
    fn a_new_save_writes_the_six_cell_switch_explicitly() {
        let four = ExperimentalRules { unlimited_items: true, ..Default::default() };
        let json = serde_json::to_value(four).unwrap();
        assert_eq!(json["six_cells"], false);
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), four);
        let six = ExperimentalRules { six_cells: true, ..Default::default() };
        let json = serde_json::to_value(six).unwrap();
        assert_eq!(json["six_cells"], true);
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), six);
    }

    /// The newer switches are written and read back as they are; an old save without them
    /// reads them off (classic rules).
    #[test]
    fn the_newer_switches_round_trip_and_default_off_in_old_saves() {
        let all = ExperimentalRules { unlimited_items: true, free_units: true, auto_promotion: true, undying_units: true, ..Default::default() };
        let json = serde_json::to_value(all).unwrap();
        assert_eq!((json["free_units"].as_bool(), json["unlimited_items"].as_bool()), (Some(true), Some(true)));
        assert_eq!((json["auto_promotion"].as_bool(), json["undying_units"].as_bool()), (Some(true), Some(true)));
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), all);
        let old: ExperimentalRules = serde_json::from_str(r#"{"simultaneous_bonuses": true, "no_duplicate_items": true}"#).unwrap();
        assert!(!old.free_units && !old.auto_promotion && !old.undying_units && !old.unlimited_items);
        assert!(old.simultaneous_bonuses);
    }

    /// The planner switch («Ускоренный ИИ») is written and read back; an older save without it
    /// reads it off, so the AI plans as before.
    #[test]
    fn the_planner_switch_round_trips_and_defaults_off_in_old_saves() {
        let on = ExperimentalRules { fast_ai: true, ..Default::default() };
        let json = serde_json::to_value(on).unwrap();
        assert_eq!(json["fast_ai"].as_bool(), Some(true));
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), on);
        let old: ExperimentalRules = serde_json::from_str(r#"{"unlimited_items": true, "six_cells": true}"#).unwrap();
        assert!(!old.fast_ai);
        assert!(old.unlimited_items && old.six_cells);
    }

    /// The five parity switches: a new game has them off (the original's rules); a save or a
    /// hand-over without their keys was Razdor's rules then, so they read ON; each written key
    /// reads back as it is.
    #[test]
    fn the_parity_switches_default_on_in_old_saves_and_off_in_new_games() {
        let new_game = ExperimentalRules::for_new_game();
        assert!(!(new_game.bless_once || new_game.undead_poison_immune || new_game.unlimited_spells || new_game.merchant_item || new_game.rear_service_flat));
        let old: ExperimentalRules = serde_json::from_str(r#"{"fast_ai": true, "two_potions": true}"#).unwrap();
        assert!(old.bless_once && old.undead_poison_immune && old.unlimited_spells && old.merchant_item && old.rear_service_flat);
        assert!(old.fast_ai && old.two_potions && !old.free_units);
        let off = ExperimentalRules { bless_once: false, merchant_item: false, ..Default::default() };
        let json = serde_json::to_value(off).unwrap();
        assert_eq!((json["bless_once"].as_bool(), json["merchant_item"].as_bool()), (Some(false), Some(false)));
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), off);
        let json = serde_json::to_value(ExperimentalRules::default()).unwrap();
        assert_eq!(json["rear_service_flat"].as_bool(), Some(false));
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), ExperimentalRules::default());
    }

    /// A save with no rules block at all keeps the five parity switches on and the eight others off.
    #[test]
    fn a_save_with_no_rules_block_is_razdors_five_parity_rules() {
        let before = ExperimentalRules::before_the_rules_were_saved();
        assert!(before.bless_once && before.undead_poison_immune && before.unlimited_spells && before.merchant_item && before.rear_service_flat);
        assert!(!before.simultaneous_bonuses && !before.unlimited_items && !before.two_potions && !before.six_cells && !before.free_units && !before.auto_promotion && !before.undying_units && !before.fast_ai);
    }

    /// Each of the five names the rules window and the scenario list's mark (`first_on`).
    #[test]
    fn each_parity_switch_has_its_own_name() {
        let names: Vec<_> = [
            ExperimentalRules { bless_once: true, ..Default::default() },
            ExperimentalRules { undead_poison_immune: true, ..Default::default() },
            ExperimentalRules { unlimited_spells: true, ..Default::default() },
            ExperimentalRules { merchant_item: true, ..Default::default() },
            ExperimentalRules { rear_service_flat: true, ..Default::default() },
        ]
        .iter()
        .map(|r| r.first_on())
        .collect();
        assert_eq!(names, [Some("Bless once a turn"), Some("Poison spares the Undead"), Some("No spell book limit"), Some("Merchant discount from an item"), Some("Rear Service always −30%")]);
    }

    /// «Склад в своих зданиях» (`building_store`): a new game has it off (the original's rule: the
    /// owner takes nothing out of his building); a save or a hand-over without its key was Razdor's
    /// rule then, so it reads ON; before the rules were saved it is ON too; its name is the rules
    /// window's and the scenario mark's.
    #[test]
    fn the_store_switch_is_off_in_new_games_and_on_in_old_saves() {
        assert!(!ExperimentalRules::for_new_game().building_store);
        let old: ExperimentalRules = serde_json::from_str(r#"{"fast_ai": true}"#).unwrap();
        assert!(old.building_store);
        let missing: ExperimentalRules = serde_json::from_str("{}").unwrap();
        assert!(missing.building_store);
        assert!(ExperimentalRules::before_the_rules_were_saved().building_store);
        let off = ExperimentalRules { building_store: false, fast_ai: true, ..Default::default() };
        let json = serde_json::to_value(off).unwrap();
        assert_eq!(json["building_store"].as_bool(), Some(false));
        assert_eq!(serde_json::from_value::<ExperimentalRules>(json).unwrap(), off);
        let on = ExperimentalRules { building_store: true, ..Default::default() };
        assert_eq!(on.first_on(), Some("Store in your own buildings"));
        assert_eq!(ExperimentalRules::default().first_on(), None);
    }
}
