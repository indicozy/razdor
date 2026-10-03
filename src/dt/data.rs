//! Unit, artefact and spell definitions and the global options, read from `Rus_Units.ini`,
//! `Rus_Artefacts.ini`, `Rus_Spells.ini` and `_Global.ini`.
//!
//! Field meanings are documented in `docs/reference/mechanics.md` (sections 1.1, 3.2, 4 and
//! the `_Global.ini` appendix). Keys this module does not know are kept in each definition's
//! `extra` map, so community additions never make loading fail.
//!
//! Mods edit these files by hand, and the original reads them leniently: a value it cannot
//! read counts as absent. So does this module: such a value takes the key's default, an entry
//! without a usable `GlobalIndex` (or an artefact without a `Type`) is skipped, and each case
//! is reported as a warning in [`Loaded::warnings`] instead of failing the whole file.

use super::ini::{Ini, Section};
use std::cell::RefCell;
use std::collections::BTreeMap;

// ------------------------------------------------------------------------------------------
// Enums
// ------------------------------------------------------------------------------------------

/// A stat that levels, items and spells can modify (`d-`, `p-`, `f-` prefixes).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stat {
    Hits,
    AttackBlow,
    DefenceBlow,
    AttackShot,
    DefenceShot,
    MagicPower,
    Initiative,
    /// Actions per battle turn.
    Manevres,
    ProtectLife,
    ProtectDeath,
    ProtectElemental,
    Regen,
    Vampirizm,
}

impl Stat {
    pub const ALL: [Stat; 13] = [
        Stat::Hits,
        Stat::AttackBlow,
        Stat::DefenceBlow,
        Stat::AttackShot,
        Stat::DefenceShot,
        Stat::MagicPower,
        Stat::Initiative,
        Stat::Manevres,
        Stat::ProtectLife,
        Stat::ProtectDeath,
        Stat::ProtectElemental,
        Stat::Regen,
        Stat::Vampirizm,
    ];

    /// The ini spelling (`Hits`, `AttackBlow`, …).
    pub fn key(self) -> &'static str {
        match self {
            Stat::Hits => "Hits",
            Stat::AttackBlow => "AttackBlow",
            Stat::DefenceBlow => "DefenceBlow",
            Stat::AttackShot => "AttackShot",
            Stat::DefenceShot => "DefenceShot",
            Stat::MagicPower => "MagicPower",
            Stat::Initiative => "Initiative",
            Stat::Manevres => "Manevres",
            Stat::ProtectLife => "ProtectLife",
            Stat::ProtectDeath => "ProtectDeath",
            Stat::ProtectElemental => "ProtectElemental",
            Stat::Regen => "Regen",
            Stat::Vampirizm => "Vampirizm",
        }
    }

    /// Parse the ini spelling, exactly.
    pub fn from_key(key: &str) -> Option<Stat> {
        Stat::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// Stat modifiers keyed by stat: flat (`d-`), percent (`p-`) or fixed (`f-`) values.
pub type StatMods = BTreeMap<Stat, i32>;

/// Magic school. Units and artefacts write `LifeMagic`/`ElementalMagic`/`DeathMagic`;
/// spells write `Life`/`Elemental`/`Death`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MagicSchool {
    Life,
    Elemental,
    Death,
}

impl MagicSchool {
    /// A unit's or an artefact's `Magic`, compared exactly as the loader does (0x48efd0):
    /// anything else is no school.
    pub fn parse(s: &str) -> Option<MagicSchool> {
        match s {
            "LifeMagic" => Some(MagicSchool::Life),
            "ElementalMagic" => Some(MagicSchool::Elemental),
            "DeathMagic" => Some(MagicSchool::Death),
            _ => None,
        }
    }

    /// A spell's `Type` (0x49ac64): `Life` and `Death` exactly, **anything else is
    /// Elemental**.
    pub fn of_spell(s: &str) -> MagicSchool {
        match s {
            "Life" => MagicSchool::Life,
            "Death" => MagicSchool::Death,
            _ => MagicSchool::Elemental,
        }
    }
}

/// Whom a caster's magic targets (editor: all / foreign / own).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MagicDirection {
    ToAll,
    ToEnemy,
    ToAlly,
}

impl MagicDirection {
    /// Exact names. The exe knows only `ToEnemy` and `ToAlly`: `ToAll` (or anything else)
    /// reads 0, which its battle code treats as "all", the meaning of Razdor's `ToAll` and of
    /// no direction, so the outcome is the same.
    pub fn parse(s: &str) -> Option<MagicDirection> {
        [MagicDirection::ToAll, MagicDirection::ToEnemy, MagicDirection::ToAlly].into_iter().find(|m| format!("{m:?}") == s)
    }

    /// The caster can use hostile magic.
    pub fn hits_enemies(self) -> bool {
        self != MagicDirection::ToAlly
    }

    /// The caster can use friendly magic.
    pub fn helps_allies(self) -> bool {
        self != MagicDirection::ToEnemy
    }
}

/// Creature type, in editor order (`Normal` is the default when the key is absent).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Nature {
    #[default]
    Normal,
    Undead,
    Elemental,
    Rogue,
    Animal,
    Hero,
    People,
}

impl Nature {
    pub const ALL: [Nature; 7] = [
        Nature::Normal,
        Nature::Undead,
        Nature::Elemental,
        Nature::Rogue,
        Nature::Animal,
        Nature::Hero,
        Nature::People,
    ];

    /// The loader's exact compare (0x48efd0): Undead, Elemental, Rogue, Animal and Hero;
    /// anything else, `People` included (the editor's name for 9 shipped units), is 0, the
    /// ordinary nature.
    pub fn parse(s: &str) -> Option<Nature> {
        [Nature::Undead, Nature::Elemental, Nature::Rogue, Nature::Animal, Nature::Hero].into_iter().find(|n| format!("{n:?}") == s)
    }

    /// Editor index (0 = Normal … 6 = People).
    pub fn index(self) -> u8 {
        Nature::ALL.iter().position(|n| *n == self).expect("in ALL") as u8
    }
}

/// A unit's special ability. The 21 vanilla bonuses in UI order (`BonusN` index 1..=21), then
/// the 31 Community Update bonuses (index 22..=52, in the order the executable lists their
/// tokens), then any other token.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Bonus {
    SpearDefense,
    HorseAtack,
    ArmorIgnore,
    ArmyMedic,
    Merchant,
    DeathCurse,
    /// UI "Кара": +10 damage ignoring defence.
    GodAnger,
    /// UI "Гнев": +20 damage ignoring defence.
    GodStrike,
    Unvulnerabe,
    VampirsGist,
    OldVampirsGist,
    Evasive,
    Ghost,
    Artillery,
    Garrison,
    AddPayment,
    Poison,
    Dead,
    FastDead,
    Counterblow,
    FlankStrike,
    // Community Update (mechanics.md 1.3 and 8).
    /// Kills heal it to full HP.
    Hunger,
    /// More damage the more it is wounded.
    Berserk,
    /// Its hostile magic lowers the target's magic protection, cumulatively.
    Exhaustion,
    /// Its hostile magic also takes a share of the target's max HP, ignoring protection.
    Drying,
    /// Poisons whoever strikes it.
    CtrPoison,
    /// Dies after its own attack.
    Suicide,
    /// Global spells of its army cost less mana and time (world map).
    Caster,
    /// Hits the target for 80% and its row neighbours for 40%.
    Splash,
    /// Physical defence grows every battle turn.
    Fortify,
    /// Undocumented; see mechanics.md 8.
    Dominate,
    /// Strong poison: 25% of max HP per turn.
    PoisonS,
    /// Magic power grows during the battle.
    Concentration,
    /// Its magic ignores protection.
    Potent,
    /// Its hits lower the target's initiative by 25%.
    Stun,
    /// Gets the very first move of the battle.
    FirstShot,
    /// In a castle or fort: stats x3, half physical damage, army defence +10.
    Bastion,
    /// Can attack any enemy from any row.
    Flying,
    /// Its hits make the target bleed.
    Bleed,
    /// Strikes an attacker first.
    PreventiveStrike,
    /// Damage +-25% by army size against the enemy's.
    Flock,
    /// Its hits lower the target's defence by 30%.
    ArmorBreaker,
    /// Targets it wounds heal no more.
    NoHeal,
    /// +1 action on the first two battle turns.
    FasterAttack,
    /// Piercing (building defence still counts) and a 10% poison.
    PoisonArmorIgnore,
    /// Documented as not working; no effect.
    HoldLine,
    /// Its hits strip the target's bonuses.
    Neutralize,
    /// Finishes a target left below 25% HP.
    KillingStrike,
    /// A kill gives back one action.
    BloodThrist,
    /// Storming a building: stats x2, physical damage taken x0.7.
    Assault,
    /// Its blessings and curses last the whole battle.
    EternalGift,
    /// Once per battle survives a lethal blow, healed and stronger.
    FateGift,
    /// Any other token, kept verbatim.
    Other(String),
}

impl Bonus {
    /// The vanilla bonuses in UI order; `VANILLA[i]` has index `i + 1`.
    pub const VANILLA: [Bonus; 21] = [
        Bonus::SpearDefense,
        Bonus::HorseAtack,
        Bonus::ArmorIgnore,
        Bonus::ArmyMedic,
        Bonus::Merchant,
        Bonus::DeathCurse,
        Bonus::GodAnger,
        Bonus::GodStrike,
        Bonus::Unvulnerabe,
        Bonus::VampirsGist,
        Bonus::OldVampirsGist,
        Bonus::Evasive,
        Bonus::Ghost,
        Bonus::Artillery,
        Bonus::Garrison,
        Bonus::AddPayment,
        Bonus::Poison,
        Bonus::Dead,
        Bonus::FastDead,
        Bonus::Counterblow,
        Bonus::FlankStrike,
    ];

    /// The Community Update bonuses; `COMMUNITY[i]` has index `i + 22` [exe: token table order].
    pub const COMMUNITY: [Bonus; 31] = [
        Bonus::Hunger,
        Bonus::Berserk,
        Bonus::Exhaustion,
        Bonus::Drying,
        Bonus::CtrPoison,
        Bonus::Suicide,
        Bonus::Caster,
        Bonus::Splash,
        Bonus::Fortify,
        Bonus::Dominate,
        Bonus::PoisonS,
        Bonus::Concentration,
        Bonus::Potent,
        Bonus::Stun,
        Bonus::FirstShot,
        Bonus::Bastion,
        Bonus::Flying,
        Bonus::Bleed,
        Bonus::PreventiveStrike,
        Bonus::Flock,
        Bonus::ArmorBreaker,
        Bonus::NoHeal,
        Bonus::FasterAttack,
        Bonus::PoisonArmorIgnore,
        Bonus::HoldLine,
        Bonus::Neutralize,
        Bonus::KillingStrike,
        Bonus::BloodThrist,
        Bonus::Assault,
        Bonus::EternalGift,
        Bonus::FateGift,
    ];

    /// Every known bonus, by index.
    pub fn known() -> impl Iterator<Item = &'static Bonus> {
        Bonus::VANILLA.iter().chain(Bonus::COMMUNITY.iter())
    }

    /// Parse an ini token, exactly; unknown tokens become [`Bonus::Other`].
    pub fn parse(s: &str) -> Bonus {
        Bonus::known_token(s).unwrap_or_else(|| Bonus::Other(s.to_string()))
    }

    /// One of the 52 names the loader knows, compared exactly (0x48efd0); anything else is 0,
    /// no bonus.
    pub fn known_token(s: &str) -> Option<Bonus> {
        Bonus::known().find(|b| b.token() == s).cloned()
    }

    /// The ini token.
    pub fn token(&self) -> &str {
        match self {
            Bonus::SpearDefense => "SpearDefense",
            Bonus::HorseAtack => "HorseAtack",
            Bonus::ArmorIgnore => "ArmorIgnore",
            Bonus::ArmyMedic => "ArmyMedic",
            Bonus::Merchant => "Merchant",
            Bonus::DeathCurse => "DeathCurse",
            Bonus::GodAnger => "GodAnger",
            Bonus::GodStrike => "GodStrike",
            Bonus::Unvulnerabe => "Unvulnerabe",
            Bonus::VampirsGist => "VampirsGist",
            Bonus::OldVampirsGist => "OldVampirsGist",
            Bonus::Evasive => "Evasive",
            Bonus::Ghost => "Ghost",
            Bonus::Artillery => "Artillery",
            Bonus::Garrison => "Garrison",
            Bonus::AddPayment => "AddPayment",
            Bonus::Poison => "Poison",
            Bonus::Dead => "Dead",
            Bonus::FastDead => "FastDead",
            Bonus::Counterblow => "Counterblow",
            Bonus::FlankStrike => "FlankStrike",
            Bonus::Hunger => "Hunger",
            Bonus::Berserk => "Berserk",
            Bonus::Exhaustion => "Exhaustion",
            Bonus::Drying => "Drying",
            Bonus::CtrPoison => "CtrPoison",
            Bonus::Suicide => "Suicide",
            Bonus::Caster => "Caster",
            Bonus::Splash => "Splash",
            Bonus::Fortify => "Fortify",
            Bonus::Dominate => "Dominate",
            Bonus::PoisonS => "PoisonS",
            Bonus::Concentration => "Concentration",
            Bonus::Potent => "Potent",
            Bonus::Stun => "Stun",
            Bonus::FirstShot => "FirstShot",
            Bonus::Bastion => "Bastion",
            Bonus::Flying => "Flying",
            Bonus::Bleed => "Bleed",
            Bonus::PreventiveStrike => "PreventiveStrike",
            Bonus::Flock => "Flock",
            Bonus::ArmorBreaker => "ArmorBreaker",
            Bonus::NoHeal => "NoHeal",
            Bonus::FasterAttack => "FasterAttack",
            Bonus::PoisonArmorIgnore => "PoisonArmorIgnore",
            Bonus::HoldLine => "HoldLine",
            Bonus::Neutralize => "Neutralize",
            Bonus::KillingStrike => "KillingStrike",
            Bonus::BloodThrist => "BloodThrist",
            Bonus::Assault => "Assault",
            Bonus::EternalGift => "EternalGift",
            Bonus::FateGift => "FateGift",
            Bonus::Other(s) => s,
        }
    }

    /// UI index 1..=21 for vanilla bonuses, `None` for community tokens.
    pub fn vanilla_index(&self) -> Option<u8> {
        Bonus::VANILLA.iter().position(|b| b == self).map(|i| i as u8 + 1)
    }

    /// Index 1..=52 of a known bonus (vanilla 1..=21, Community 22..=52), `None` for others.
    pub fn index(&self) -> Option<u8> {
        Bonus::known().position(|b| b == self).map(|i| i as u8 + 1)
    }

    /// A Community Update bonus.
    pub fn is_community(&self) -> bool {
        Bonus::COMMUNITY.contains(self)
    }
}

/// Artefact slot kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ArtefactType {
    /// Warriors only.
    BlowWeapon,
    /// Shooters only.
    ShotWeapon,
    /// Mages only.
    Staff,
    Armor,
    Helm,
    Shield,
    Ring,
    Amulet,
    /// Used from the army screen, not kept in a slot.
    Potion,
    /// Trade goods; cannot be equipped.
    Item,
}

impl ArtefactType {
    pub const ALL: [ArtefactType; 10] = [
        ArtefactType::BlowWeapon,
        ArtefactType::ShotWeapon,
        ArtefactType::Staff,
        ArtefactType::Armor,
        ArtefactType::Helm,
        ArtefactType::Shield,
        ArtefactType::Ring,
        ArtefactType::Amulet,
        ArtefactType::Potion,
        ArtefactType::Item,
    ];

    /// The loader's exact, case-sensitive compare (0x4990cc).
    pub fn parse(s: &str) -> Option<ArtefactType> {
        ArtefactType::ALL.into_iter().find(|t| format!("{t:?}") == s)
    }

    /// Weapons and staffs: a unit holds only one of these.
    pub fn is_weapon(self) -> bool {
        matches!(self, ArtefactType::BlowWeapon | ArtefactType::ShotWeapon | ArtefactType::Staff)
    }
}

/// Whom a world-map spell is cast on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpellTarget {
    /// The hero's own army.
    Hero,
    /// A whole enemy army.
    Enemy,
    /// Known to the exe, unused in vanilla data.
    OneEnemy,
}

impl SpellTarget {
    /// The loader's exact, case-sensitive compare (0x49ac64): anything but `Enemy` and
    /// `OneEnemy` is the hero's own army.
    pub fn parse(s: &str) -> Option<SpellTarget> {
        [SpellTarget::Hero, SpellTarget::Enemy, SpellTarget::OneEnemy].into_iter().find(|t| format!("{t:?}") == s)
    }

    /// `Target` as the loader keeps it: missing, empty or unknown is the hero's army (1).
    pub fn read(s: &str) -> SpellTarget {
        SpellTarget::parse(s).unwrap_or(SpellTarget::Hero)
    }
}

// ------------------------------------------------------------------------------------------
// Loaded values and the field reader
// ------------------------------------------------------------------------------------------

/// A file's definitions plus what had to be ignored or skipped to read them.
#[derive(Clone, Debug)]
pub struct Loaded<T> {
    pub value: T,
    /// One line per ignored value or skipped entry (`[section] Key=value: …`).
    pub warnings: Vec<String>,
}

impl<T> Loaded<T> {
    /// The value, or every warning when there was any: for our own data, which must be clean.
    pub fn strict(self) -> Result<T, String> {
        if self.warnings.is_empty() {
            Ok(self.value)
        } else {
            Err(self.warnings.join("; "))
        }
    }
}

/// Tracks which keys were consumed (the rest becomes `extra`) and collects warnings for
/// values the loader does not know; those read as absent.
struct Fields<'a> {
    sec: &'a Section,
    used: RefCell<Vec<String>>,
    warnings: RefCell<Vec<String>>,
}

impl<'a> Fields<'a> {
    fn new(sec: &'a Section) -> Self {
        Fields { sec, used: RefCell::new(Vec::new()), warnings: RefCell::new(Vec::new()) }
    }

    fn warn(&self, key: &str, value: &str, what: &str) {
        self.warnings.borrow_mut().push(format!("[{}] {key}={value}: {what}", self.sec.name));
    }

    /// Hand the collected warnings over to `out`.
    fn warnings_into(&self, out: &mut Vec<String>) {
        out.append(&mut self.warnings.borrow_mut());
    }

    fn mark(&self, key: &str) {
        self.used.borrow_mut().push(key.to_string());
    }

    fn str(&self, key: &str) -> Option<&'a str> {
        self.mark(key);
        self.sec.get_nonempty(key)
    }

    fn string(&self, key: &str) -> String {
        self.str(key).unwrap_or_default().to_string()
    }

    /// An integer read loosely, as the original reads every number ([`crate::dt::ini::loose_int`]: `1.5`
    /// is 15, `abc` is 0); `None` when absent or empty, which the original reads as 0.
    fn opt_int(&self, key: &str) -> Option<i32> {
        self.mark(key);
        self.sec.get_int(key)
    }

    fn int(&self, key: &str) -> i32 {
        self.opt_int(key).unwrap_or(0)
    }

    /// The first `N` fields of a comma-separated list, each read loosely, a field past the
    /// end reading 0 (0x472874); `None` when the key is absent or empty.
    fn int_array<const N: usize>(&self, key: &str) -> Option<[i32; N]> {
        self.mark(key);
        self.sec.get_nonempty(key)?;
        Some(std::array::from_fn(|k| self.sec.field_int(key, k + 1)))
    }

    /// An enum value; `None` when absent, empty or unknown (with a warning).
    fn enum_opt<T>(&self, key: &str, parse: impl Fn(&str) -> Option<T>) -> Option<T> {
        let v = self.str(key)?;
        let parsed = parse(v);
        if parsed.is_none() {
            self.warn(key, v, "unknown value, read as none");
        }
        parsed
    }

    /// All `<prefix><Stat>` keys with a value.
    fn mods(&self, prefix: &str) -> StatMods {
        let mut out = StatMods::new();
        for stat in Stat::ALL {
            let key = format!("{prefix}{}", stat.key());
            if let Some(v) = self.opt_int(&key) {
                out.insert(stat, v);
            }
        }
        out
    }

    /// `GlobalIndex` as an id, read loosely. The loader stores the entry at record
    /// `GlobalIndex − 1`; below 1 that is before its table (it writes over other memory),
    /// which Razdor cannot follow: such an entry is skipped with a warning.
    fn global_index(&self) -> Result<u32, String> {
        self.mark("GlobalIndex");
        let v = self.sec.int("GlobalIndex");
        u32::try_from(v)
            .ok()
            .filter(|&id| id >= 1)
            .ok_or_else(|| format!("[{}] GlobalIndex={}: not an id, entry skipped", self.sec.name, self.sec.get("GlobalIndex").unwrap_or("")))
    }

    /// Entries not consumed by the typed fields, empty values dropped.
    fn extra(&self) -> BTreeMap<String, String> {
        let used = self.used.borrow();
        self.sec
            .entries
            .iter()
            .filter(|(k, v)| !v.is_empty() && !used.contains(k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }
}

/// Keeps one definition per id: a later section with the same `GlobalIndex` overwrites the
/// record, as the loader writes it again.
fn last_per_id<T>(defs: Vec<T>, id: impl Fn(&T) -> u32) -> Vec<T> {
    let ids: Vec<u32> = defs.iter().map(&id).collect();
    defs.into_iter().enumerate().filter(|(i, _)| !ids[i + 1..].contains(&ids[*i])).map(|(_, d)| d).collect()
}

// ------------------------------------------------------------------------------------------
// Units
// ------------------------------------------------------------------------------------------

/// One entry of a unit's upgrade tree (`NextUnitN` / `NextUnitNLevel`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Upgrade {
    /// The target class as written in the file: the `Name` of another unit.
    pub target_name: String,
    /// The target's `GlobalIndex`, resolved by name once all units are loaded.
    pub target: Option<u32>,
    /// Level the unit must reach (always 1 in vanilla). The original's AI checks it
    /// against its 0-based level; the player's promotion ignores it.
    pub level: i32,
    /// Which of `NextUnit1`..`NextUnit3` (1–3) holds it after the original's loader moved
    /// the options ([`normalise_upgrade_slots`]).
    pub slot: u8,
}

/// The original's unit loader rearranges the upgrade options (0x4e0448, ai.md §11): a lone
/// option in slot 1 or 3 moves to slot 2; of two options, one in slot 2 moves to slot 3 when
/// the other is in slot 1, to slot 1 when the other is in slot 3. So two options always sit
/// in slots 1 and 3, which the AI's Militia and Infantry picks rely on.
pub fn normalise_upgrade_slots(upgrades: &mut [Upgrade]) {
    let has = |n: u8, u: &[Upgrade]| u.iter().any(|x| x.slot == n);
    let (one, two, three) = (has(1, upgrades), has(2, upgrades), has(3, upgrades));
    let moves: &[(u8, u8)] = match (one, two, three) {
        (true, false, false) => &[(1, 2)],
        (false, false, true) => &[(3, 2)],
        (true, true, false) => &[(2, 3)],
        (false, true, true) => &[(2, 1)],
        _ => &[],
    };
    for &(from, to) in moves {
        for u in upgrades.iter_mut().filter(|u| u.slot == from) {
            u.slot = to;
        }
    }
}

/// A unit type from `Rus_Units.ini`. See mechanics.md 1.1 for every field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnitDef {
    /// `GlobalIndex`: 1..=102 in vanilla; 1–3 are the hero classes.
    pub id: u32,
    pub name: String,
    pub description: String,
    /// Portrait index.
    pub icon_index: i32,
    /// Hire price; base for wages, healing, resurrection and XP value.
    pub cost: i32,
    /// Percent correction of the tactical cost.
    pub cost_multiplier: i32,
    pub cost_gold_div: i32,
    /// XP needed for the first level-up.
    pub start_experience: i32,
    /// Percent growth of the XP requirement per level.
    pub level_multiplier: i32,
    pub hits: i32,
    pub attack_blow: i32,
    pub attack_shot: i32,
    pub magic_power: i32,
    pub defence_blow: i32,
    pub defence_shot: i32,
    /// Percent resistances to hostile magic.
    pub protect_life: i32,
    pub protect_death: i32,
    pub protect_elemental: i32,
    pub initiative: i32,
    /// Actions per battle turn.
    pub manevres: i32,
    /// Percent regeneration.
    pub regen: i32,
    /// Percent vampirism.
    pub vampirism: i32,
    pub magic: Option<MagicSchool>,
    pub magic_direction: Option<MagicDirection>,
    pub nature: Nature,
    /// `Nature=People`: the editor's own nature 6 (0x5baf84), which the game reads as 0.
    pub people: bool,
    pub bonus: Option<Bonus>,
    /// The bonus as the map editor reads it, with its own name table (DTMapEdit 0x5baf1c,
    /// docs/reference/editor/testers.md §4): the editor knows four names the game does not,
    /// ids 22–25 ([`EDITOR_BONUSES`]); only the editor's battle tester uses them.
    pub editor_bonus: Option<u8>,
    /// Editor "captivity" value.
    pub surrender: i32,
    pub upgrades: Vec<Upgrade>,
    /// Per-level stat gains (`d-*`).
    pub level_up: StatMods,
    /// Community: percent of physical damage ignored.
    pub evasion: Option<i32>,
    /// Community: floor of magic power in battle.
    pub min_magic_power: Option<i32>,
    /// Community: magic power lost per battle turn.
    pub mana_drain: Option<i32>,
    /// Keys not listed above.
    pub extra: BTreeMap<String, String>,
}

impl UnitDef {
    /// The unit of `sec`, or `None` (with a warning) when it has no usable `GlobalIndex`.
    fn from_section(sec: &Section, warnings: &mut Vec<String>) -> Option<UnitDef> {
        let f = Fields::new(sec);
        let id = f.global_index().map_err(|w| warnings.push(w)).ok()?;
        // The upgrades are read by the second pass, by section position ([`parse_units`]).
        for n in 1..=3u8 {
            f.mark(&format!("NextUnit{n}"));
            f.mark(&format!("NextUnit{n}Level"));
        }
        let unit = UnitDef {
            id,
            name: f.string("Name"),
            description: f.string("Descript"),
            icon_index: f.int("IconIndex"),
            cost: f.int("Cost"),
            cost_multiplier: f.int("CostMultipler"),
            cost_gold_div: f.int("CostGoldDiv"),
            start_experience: f.int("StartExpirience"),
            level_multiplier: f.int("LevelMultipler"),
            hits: f.int("Hits"),
            attack_blow: f.int("AttackBlow"),
            attack_shot: f.int("AttackShot"),
            magic_power: f.int("MagicPower"),
            defence_blow: f.int("DefenceBlow"),
            defence_shot: f.int("DefenceShot"),
            protect_life: f.int("ProtectLife"),
            protect_death: f.int("ProtectDeath"),
            protect_elemental: f.int("ProtectElemental"),
            initiative: f.int("Initiative"),
            manevres: f.int("Manevres"),
            regen: f.int("Regen"),
            vampirism: f.int("Vampirizm"),
            magic: f.enum_opt("Magic", MagicSchool::parse),
            magic_direction: f.enum_opt("MagicDirection", MagicDirection::parse),
            // `People`, the editor's name, is no name the exe knows: 0, as is no value.
            nature: f.enum_opt("Nature", |v| if v == "People" { Some(Nature::Normal) } else { Nature::parse(v) }).unwrap_or_default(),
            people: f.str("Nature") == Some("People"),
            bonus: f.enum_opt("Bonus", Bonus::known_token),
            editor_bonus: f.str("Bonus").and_then(editor_bonus),
            surrender: f.int("Surrender"),
            upgrades: Vec::new(),
            level_up: f.mods("d-"),
            evasion: f.opt_int("Evasion"),
            min_magic_power: f.opt_int("MinMagicPower"),
            mana_drain: f.opt_int("ManaDrain"),
            extra: BTreeMap::new(),
        };
        f.warnings_into(warnings);
        Some(UnitDef { extra: f.extra(), ..unit })
    }

    /// The nature as the map editor reads it: `People` is its own (6), not Normal.
    pub fn editor_nature(&self) -> Nature {
        if self.people {
            Nature::People
        } else {
            self.nature
        }
    }

    /// The unit's value of a base stat.
    pub fn stat(&self, stat: Stat) -> i32 {
        match stat {
            Stat::Hits => self.hits,
            Stat::AttackBlow => self.attack_blow,
            Stat::DefenceBlow => self.defence_blow,
            Stat::AttackShot => self.attack_shot,
            Stat::DefenceShot => self.defence_shot,
            Stat::MagicPower => self.magic_power,
            Stat::Initiative => self.initiative,
            Stat::Manevres => self.manevres,
            Stat::ProtectLife => self.protect_life,
            Stat::ProtectDeath => self.protect_death,
            Stat::ProtectElemental => self.protect_elemental,
            Stat::Regen => self.regen,
            Stat::Vampirizm => self.vampirism,
        }
    }
}

/// The editor's bonus names past the 21 vanilla ones, ids 22–25 (DTMapEdit 0x5baf1c); the
/// game uses those ids for Hunger, Berserk, Exhaustion and Drying.
pub const EDITOR_BONUSES: [&str; 4] = ["OldVampiressGist", "Chatty", "Terrible", "Parrying"];

/// A `Bonus` token's id in the editor's name table: 1–21 the vanilla bonuses, 22–25
/// [`EDITOR_BONUSES`], `None` for any other (the Community tokens included).
pub fn editor_bonus(token: &str) -> Option<u8> {
    Bonus::known_token(token)
        .and_then(|b| b.vanilla_index())
        .or_else(|| EDITOR_BONUSES.iter().position(|n| *n == token).map(|i| i as u8 + 22))
}

/// A section the unit loader takes (0x4e0448): its `Name`, `StartExpirience` and `Cost` are
/// all non-empty; `GlobalIndex` is not part of the test.
fn is_unit(sec: &Section) -> bool {
    ["Name", "StartExpirience", "Cost"].iter().all(|k| sec.get_nonempty(k).is_some())
}

/// The `NextUnitN` options of a section in their file slots, targets unresolved.
fn upgrade_options(sec: &Section) -> Vec<Upgrade> {
    (1..=3u8)
        .filter_map(|n| {
            let name = sec.get_nonempty(&format!("NextUnit{n}"))?;
            Some(Upgrade { target_name: name.to_string(), target: None, level: sec.int(&format!("NextUnit{n}Level")), slot: n })
        })
        .collect()
}

/// All units of `Rus_Units.ini`, in file order, as the loader's two passes read them
/// (0x4e0448):
/// - pass 1 takes every section with a `Name`, `StartExpirience` and `Cost` and stores it at
///   its `GlobalIndex` (a later section with the same index overwrites it);
/// - pass 2 gives the `NextUnitN` options of the section at **position** p (all sections
///   counted) to the type with id p + 1, not to the section's own `GlobalIndex` (the shipped
///   file has `GlobalIndex` = position + 1 everywhere, so the two agree there). Each target
///   name is matched exactly against every type's `Name` in id order, the last match winning;
///   a name that matches nothing is no option. Then the slots are normalised
///   ([`normalise_upgrade_slots`]).
///
/// An empty `NextUnitN` also matches the empty name of an unfilled record in the original
/// (a non-unit section or a hole in the numbering); Razdor has no such records, so it stays
/// empty (as it does with the shipped file).
pub fn parse_units(ini: &Ini) -> Loaded<Vec<UnitDef>> {
    let mut warnings = Vec::new();
    let units = ini.sections.iter().filter(|s| is_unit(s)).filter_map(|s| UnitDef::from_section(s, &mut warnings)).collect();
    let mut units = last_per_id(units, |u: &UnitDef| u.id);
    let mut by_id: Vec<&UnitDef> = units.iter().collect();
    by_id.sort_by_key(|u| u.id);
    let by_name: BTreeMap<String, u32> = by_id.iter().map(|u| (u.name.clone(), u.id)).collect();
    let options: Vec<Option<Vec<Upgrade>>> = ini.sections.iter().map(|s| is_unit(s).then(|| upgrade_options(s))).collect();
    for u in &mut units {
        let mut upgrades = options.get(u.id as usize - 1).cloned().flatten().unwrap_or_default();
        upgrades.retain_mut(|up| {
            up.target = by_name.get(&up.target_name).copied();
            if up.target.is_none() {
                warnings.push(format!("{} (GlobalIndex {}): upgrade to {}: no unit of that name, no option", u.name, u.id, up.target_name));
            }
            up.target.is_some()
        });
        normalise_upgrade_slots(&mut upgrades);
        u.upgrades = upgrades;
    }
    Loaded { value: units, warnings }
}

// ------------------------------------------------------------------------------------------
// Artefacts
// ------------------------------------------------------------------------------------------

/// An artefact or item from `Rus_Artefacts.ini`. See mechanics.md 4.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ArtefactDef {
    /// `GlobalIndex`.
    pub id: u32,
    pub name: String,
    pub description: String,
    /// Image file name.
    pub icon: String,
    /// Base price. Negative marks a personal item that cannot be taken away.
    pub cost: i32,
    pub kind: ArtefactType,
    /// Bonus granted while worn.
    pub bonus: Option<Bonus>,
    /// Magic school granted or changed while worn.
    pub magic: Option<MagicSchool>,
    /// `d-*`: flat additions.
    pub add: StatMods,
    /// `p-*`: percent changes.
    pub percent: StatMods,
    /// `f-*`: fixed values (on potions: the healing amount).
    pub fixed: StatMods,
    pub extra: BTreeMap<String, String>,
}

impl ArtefactDef {
    /// The artefact of `sec` (an item section, [`is_item`]), or `None` (with a warning) when
    /// it has no usable `GlobalIndex`.
    fn from_section(sec: &Section, warnings: &mut Vec<String>) -> Option<ArtefactDef> {
        let f = Fields::new(sec);
        let id = f.global_index().map_err(|w| warnings.push(w)).ok()?;
        // A type the loader does not know reads as a potion (0x4990cc).
        let kind = f.str("Type").and_then(ArtefactType::parse).unwrap_or(ArtefactType::Potion);
        let def = ArtefactDef {
            id,
            name: f.string("Name"),
            description: f.string("Descript"),
            icon: f.string("Icon"),
            cost: f.int("Cost"),
            kind,
            bonus: f.enum_opt("Bonus", Bonus::known_token),
            magic: f.enum_opt("Magic", MagicSchool::parse),
            add: f.mods("d-"),
            percent: f.mods("p-"),
            fixed: f.mods("f-"),
            extra: BTreeMap::new(),
        };
        f.warnings_into(warnings);
        Some(ArtefactDef { extra: f.extra(), ..def })
    }

    /// A personal item (negative price): cannot be sold or taken away.
    pub fn is_personal(&self) -> bool {
        self.cost < 0
    }
}

/// A section the artefact loader fills (0x498eb4): its `Icon`, `Name`, `Type` and `Cost` are
/// all non-empty. (`Icon` is only tested; the picture comes from `Items.ugs`.) Any other
/// section keeps an empty record and is no item.
fn is_item(sec: &Section) -> bool {
    ["Icon", "Name", "Type", "Cost"].iter().all(|k| sec.get_nonempty(k).is_some())
}

/// All artefacts of `Rus_Artefacts.ini`, in file order, each stored at its `GlobalIndex`
/// (a later section with the same index overwrites it).
pub fn parse_artefacts(ini: &Ini) -> Loaded<Vec<ArtefactDef>> {
    let mut warnings = Vec::new();
    let defs = ini.sections.iter().filter(|s| is_item(s)).filter_map(|s| ArtefactDef::from_section(s, &mut warnings)).collect();
    Loaded { value: last_per_id(defs, |a: &ArtefactDef| a.id), warnings }
}

// ------------------------------------------------------------------------------------------
// Spells
// ------------------------------------------------------------------------------------------

/// One visual layer of a spell (`EffectN=file,r,g,b,duration_ms,y_offset,scale×1000,start_ms`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellEffect {
    pub file: String,
    pub rgb: [i32; 3],
    pub duration_ms: i32,
    pub y_offset: i32,
    /// Scale × 1000.
    pub scale_milli: i32,
    pub start_ms: i32,
}

impl SpellEffect {
    fn parse(s: &str) -> Option<SpellEffect> {
        let parts: Vec<&str> = s.split(',').map(str::trim).collect();
        let [file, rest @ ..] = parts.as_slice() else { return None };
        // A blank number counts as 0: the Evolution mod leaves the start time empty
        // (`Effect2=S-Light-Back,255,0,0,2000,47,1500,`) and the original plays it.
        let n: Vec<i32> = rest.iter().map(|x| if x.is_empty() { Some(0) } else { x.parse().ok() }).collect::<Option<_>>()?;
        let [r, g, b, duration_ms, y_offset, scale_milli, start_ms] = n.as_slice().try_into().ok()?;
        Some(SpellEffect { file: file.to_string(), rgb: [r, g, b], duration_ms, y_offset, scale_milli, start_ms })
    }
}

/// One icon layer: image name plus its colour tint.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SpellIcon {
    pub image: Option<String>,
    pub tint: Option<[i32; 3]>,
}

/// A world-map spell or prayer from `Rus_Spells.ini`. See mechanics.md 3.2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpellDef {
    /// 1-based position in the file; maps and events refer to spells by it.
    pub id: u32,
    pub name: String,
    /// Price to learn.
    pub cost_gold: i32,
    /// Mana to cast.
    pub cost_mana: i32,
    pub school: Option<MagicSchool>,
    /// Duration in game hours at caster level 0; `None` = instant. 9999 ≈ permanent.
    pub time_work: Option<i32>,
    /// Casting time in game hours at caster level 0.
    pub time_cast: Option<i32>,
    pub target: Option<SpellTarget>,
    /// `Icon1..3` with `ColorC1..3`.
    pub icons: [SpellIcon; 3],
    /// `Effect1..3`: bottom, back, front.
    pub effects: [Option<SpellEffect>; 3],
    /// Instant HP change (+ heals, − damages).
    pub delta_fixed_hits: Option<i32>,
    /// Instant percent HP change.
    pub delta_percent_hits: Option<i32>,
    /// `d-*` for the duration.
    pub add: StatMods,
    /// `p-*` for the duration.
    pub percent: StatMods,
    /// `p-LifeLose`: percent life drain.
    pub life_lose_percent: Option<i32>,
    pub extra: BTreeMap<String, String>,
}

impl SpellDef {
    fn from_section(sec: &Section, id: u32, warnings: &mut Vec<String>) -> SpellDef {
        let f = Fields::new(sec);
        let mut icons: [SpellIcon; 3] = Default::default();
        let mut effects: [Option<SpellEffect>; 3] = Default::default();
        for i in 0..3 {
            let n = i + 1;
            icons[i].image = f.str(&format!("Icon{n}")).map(str::to_string);
            icons[i].tint = f.int_array(&format!("ColorC{n}"));
            let effect_key = format!("Effect{n}");
            if let Some(v) = f.str(&effect_key) {
                effects[i] = SpellEffect::parse(v);
                if effects[i].is_none() {
                    f.warn(&effect_key, v, "not an effect, ignored");
                }
            }
        }
        let spell = SpellDef {
            id,
            name: f.string("Name"),
            cost_gold: f.int("CostGold"),
            cost_mana: f.int("CostMana"),
            school: f.str("Type").map(MagicSchool::of_spell),
            time_work: f.opt_int("TimeWork"),
            time_cast: f.opt_int("TimeCast"),
            target: Some(SpellTarget::read(f.str("Target").unwrap_or(""))),
            icons,
            effects,
            delta_fixed_hits: f.opt_int("DeltaFixedHits"),
            delta_percent_hits: f.opt_int("DeltaPercentHits"),
            add: f.mods("d-"),
            percent: f.mods("p-"),
            life_lose_percent: f.opt_int("p-LifeLose"),
            extra: BTreeMap::new(),
        };
        f.warnings_into(warnings);
        SpellDef { extra: f.extra(), ..spell }
    }
}

/// A section the spell loader fills (0x4e0c60): its `Name`, `Type` and `CostGold` are all
/// non-empty.
fn is_spell(sec: &Section) -> bool {
    ["Name", "Type", "CostGold"].iter().all(|k| sec.get_nonempty(k).is_some())
}

/// All spells of `Rus_Spells.ini`. A spell's id is its section's 1-based **position among
/// all sections** (0x4e0c60), so a section that is no spell (the trailing empty template,
/// `[MapEditorSpecialOptions]`, anything in between) leaves its id unused.
pub fn parse_spells(ini: &Ini) -> Loaded<Vec<SpellDef>> {
    let mut warnings = Vec::new();
    let value = ini
        .sections
        .iter()
        .enumerate()
        .filter(|(_, s)| is_spell(s))
        .map(|(i, s)| SpellDef::from_section(s, i as u32 + 1, &mut warnings))
        .collect();
    Loaded { value, warnings }
}

// ------------------------------------------------------------------------------------------
// Global options
// ------------------------------------------------------------------------------------------

/// Number of AI target-selection models (standard, aggressive, passive, hoarding, trading).
pub const AI_MODELS: usize = 5;

/// One priority per AI target model.
pub type ModelPriorities = [i32; AI_MODELS];

/// AI target priorities from `[GlobalOptions]` (mechanics.md 5.6). `None` when absent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AiTargets {
    pub min_attack_army: Option<ModelPriorities>,
    pub min_attack_castle: Option<ModelPriorities>,
    pub min_random: Option<ModelPriorities>,
    pub min_talking: Option<ModelPriorities>,
    /// Read from `MinHealingTarget`, as the exe does. The shipped file spells it
    /// `MixHealingTarget`, which the exe ignores; that line lands in `extra`.
    pub min_healing: Option<ModelPriorities>,
    pub max_healing: Option<ModelPriorities>,
    pub min_garrison: Option<ModelPriorities>,
    pub max_garrison: Option<ModelPriorities>,
    pub min_purchase: Option<ModelPriorities>,
    pub max_purchase: Option<ModelPriorities>,
    pub gold_purchase: Option<ModelPriorities>,
    pub min_village: Option<ModelPriorities>,
    pub max_village: Option<ModelPriorities>,
    pub gold_village: Option<ModelPriorities>,
}

/// Gameplay constants from `_Global.ini` (mechanics.md appendix).
/// [`Default`] gives the vanilla values, for content without the file (the built-in demo).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GlobalOptions {
    /// Elemental bless/curse initiative divisor.
    pub wizard_main_spell: i32,
    pub bless_main_spell: i32,
    pub bless_next_spell: i32,
    pub curse_main_spell: i32,
    pub curse_next_spell: i32,
    /// Per-turn magic power loss by school.
    pub dec_spell_life: i32,
    pub dec_spell_death: i32,
    pub dec_spell_elemental: i32,
    /// Magic power floor by school.
    pub min_spell_life: i32,
    pub min_spell_death: i32,
    pub min_spell_elemental: i32,
    /// Read but unused by the original.
    pub crazy_ai: i32,
    /// Back-row defence bonus against shots.
    pub row2_def: i32,
    /// Battle turn limit.
    pub battle_end_turn: i32,
    pub zero_density: i32,
    /// Heal cost in percent of unit cost.
    pub healing_const: i32,
    /// Heal time in minutes.
    pub healing_time: i32,
    /// Resurrection cost in percent of unit cost.
    pub resurect_const: i32,
    /// Minutes after death within which resurrection is possible.
    pub max_time_resurection: i32,
    /// Days of wages the AI keeps in reserve.
    pub need_upkeep_day: i32,
    /// Minutes an army keeps going unpaid.
    pub max_time_not_upkeep: i32,
    pub victory_gold_div: i32,
    pub min_victory_gold: i32,
    pub cost_recrut_div: i32,
    pub cost_mercenary_div: i32,
    /// Barracks restock speed in days.
    pub max_day_count_for_new_unit: i32,
    /// Percent HP healed per day in a garrison.
    pub garrison_auto_heal: i32,
    /// Ranged attack from which a shooter counts as a cannon.
    pub shot_weapon_range: i32,
    /// AI view ranges.
    pub ai_distance: [i32; 3],
    pub ai_get_path_distance: i32,
    /// XP pool percent of the enemy tactical cost.
    pub main_exp_correction: i32,
    /// XP skew by strength ratio.
    pub exp_correction: i32,
    pub ai_experience_percent: i32,
    pub hero_experience_modificator: i32,
    /// The original's difficulty factor F: 120, or 100 with "impossible difficulty"
    /// (`OptValue10` in `Rus_DiscordTimes.ini`, not `_Global.ini`). It scales the player's
    /// battle XP.
    pub difficulty_factor: i32,
    /// Item sell price in percent.
    pub item_sale_cost: i32,
    /// `[Costs] ShipCost`: ship rent.
    pub ship_cost: i32,
    pub ai_targets: AiTargets,
    /// `[AIArmyGeneration]`: army theme → unit ids, in file order.
    pub army_generation: Vec<(String, Vec<u32>)>,
    /// Other `[GlobalOptions]` keys.
    pub extra: BTreeMap<String, String>,
}

impl Default for GlobalOptions {
    fn default() -> Self {
        GlobalOptions {
            wizard_main_spell: 7,
            bless_main_spell: 6,
            bless_next_spell: 12,
            curse_main_spell: 5,
            curse_next_spell: 10,
            dec_spell_life: 2,
            dec_spell_death: 2,
            dec_spell_elemental: 5,
            min_spell_life: 15,
            min_spell_death: 0,
            min_spell_elemental: 15,
            crazy_ai: 0,
            row2_def: 5,
            battle_end_turn: 25,
            zero_density: 5,
            healing_const: 50,
            healing_time: 60,
            resurect_const: 300,
            max_time_resurection: 10080,
            need_upkeep_day: 5,
            max_time_not_upkeep: 10080,
            victory_gold_div: 2,
            min_victory_gold: 25,
            cost_recrut_div: 2,
            cost_mercenary_div: 2,
            max_day_count_for_new_unit: 10,
            garrison_auto_heal: 10,
            shot_weapon_range: 60,
            ai_distance: [100, 50, 25],
            ai_get_path_distance: 5,
            main_exp_correction: 30,
            exp_correction: 50,
            ai_experience_percent: 100,
            hero_experience_modificator: 50,
            difficulty_factor: 120,
            item_sale_cost: 25,
            ship_cost: 250,
            ai_targets: AiTargets::default(),
            army_generation: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl GlobalOptions {
    /// Read `_Global.ini` as the original does: every key read loosely, and a **missing key
    /// reads 0**, not its vanilla value ([`Default`] holds those for content without the
    /// file). Sections are selected the way the reader selects them over a session: first
    /// `[GlobalOptions]` (at start), then `[BattleEffects]` (at the first map load), then
    /// `[Costs]` (at a shipyard); a missing one leaves the one before selected, so its reads
    /// come from there (saves-data.md §1, rule 3).
    pub fn from_ini(ini: &Ini) -> Loaded<GlobalOptions> {
        let mut o = GlobalOptions::default();
        let mut warnings = Vec::new();
        if let Some(sec) = ini.section("AIArmyGeneration") {
            // Razdor's own reading: the exe has no such section.
            for (k, v) in &sec.entries {
                let mut ids = Vec::new();
                for x in v.split(',').map(str::trim).filter(|x| !x.is_empty()) {
                    match x.parse() {
                        Ok(id) => ids.push(id),
                        Err(_) => warnings.push(format!("[{}] {k}={v}: {x} is not a unit id, ignored", sec.name)),
                    }
                }
                o.army_generation.push((k.clone(), ids));
            }
        }
        // The reader starts with its first section selected.
        let first = (!ini.sections.is_empty()).then_some(0);
        let general = ini.select("GlobalOptions", first);
        let costs = ini.select("Costs", ini.select("BattleEffects", general));
        let none = Section::default();
        let at = |i: Option<usize>| i.map_or(&none, |i| &ini.sections[i]);
        o.ship_cost = at(costs).int("ShipCost");
        let f = Fields::new(at(general));
        let set = |v: &mut i32, key: &str| *v = f.int(key);
        set(&mut o.wizard_main_spell, "WizardMainSpell");
        set(&mut o.bless_main_spell, "BlessMainSpell");
        set(&mut o.bless_next_spell, "BlessNextSpell");
        set(&mut o.curse_main_spell, "CurseMainSpell");
        set(&mut o.curse_next_spell, "CurseNextSpell");
        set(&mut o.dec_spell_life, "DecSpellLife");
        set(&mut o.dec_spell_death, "DecSpellDeath");
        // Spelt so in the exe (and in the shipped file).
        set(&mut o.dec_spell_elemental, "DecSpellelemental");
        set(&mut o.min_spell_life, "MinSpellLife");
        set(&mut o.min_spell_death, "MinSpellDeath");
        set(&mut o.min_spell_elemental, "MinSpellElemental");
        set(&mut o.crazy_ai, "CrazyAI");
        set(&mut o.row2_def, "Row2Def");
        set(&mut o.battle_end_turn, "BattleEndTurn");
        set(&mut o.zero_density, "ZeroDensity");
        set(&mut o.healing_const, "HealingConst");
        set(&mut o.healing_time, "HealingTime");
        set(&mut o.resurect_const, "ResurectConst");
        set(&mut o.max_time_resurection, "MaxTimeResurection");
        set(&mut o.need_upkeep_day, "NeedUpkeepDay");
        set(&mut o.max_time_not_upkeep, "MaxTimeNotUpkeep");
        set(&mut o.victory_gold_div, "VictoryGoldDiv");
        set(&mut o.min_victory_gold, "MinVictoryGold");
        set(&mut o.cost_recrut_div, "CostRecrutDiv");
        set(&mut o.cost_mercenary_div, "CostMercenaryDiv");
        set(&mut o.max_day_count_for_new_unit, "MaxDayCountForNewUnit");
        set(&mut o.garrison_auto_heal, "GarrisonAutoHeal");
        set(&mut o.shot_weapon_range, "ShotWeaponRange");
        for (i, d) in o.ai_distance.iter_mut().enumerate() {
            set(d, &format!("AIDistance{i}"));
        }
        set(&mut o.ai_get_path_distance, "AIGetPathDistance");
        set(&mut o.main_exp_correction, "MainExpCorrection");
        set(&mut o.exp_correction, "ExpCorrection");
        set(&mut o.ai_experience_percent, "AIExpiriencePercent");
        set(&mut o.hero_experience_modificator, "HeroExpirienceModificator");
        set(&mut o.item_sale_cost, "ItemSaleCost");
        let prio = |key: &str| -> Option<ModelPriorities> { f.int_array(key) };
        o.ai_targets = AiTargets {
            min_attack_army: prio("MinAtackArmyTarget"),
            min_attack_castle: prio("MinAtackCastleTarget"),
            min_random: prio("MinRandomTarget"),
            min_talking: prio("MinTalkingTarget"),
            min_healing: prio("MinHealingTarget"),
            max_healing: prio("MaxHealingTarget"),
            min_garrison: prio("MinGarrisonTarget"),
            max_garrison: prio("MaxGarrisonTarget"),
            min_purchase: prio("MinPurchaseTarget"),
            max_purchase: prio("MaxPurchaseTarget"),
            gold_purchase: prio("GoldPurchaseTarget"),
            min_village: prio("MinVillageTarget"),
            max_village: prio("MaxVillageTarget"),
            gold_village: prio("GoldVillageTarget"),
        };
        o.extra = f.extra();
        f.warnings_into(&mut warnings);
        Loaded { value: o, warnings }
    }

    /// Per-turn magic power loss for a school.
    pub fn dec_spell(&self, school: MagicSchool) -> i32 {
        match school {
            MagicSchool::Life => self.dec_spell_life,
            MagicSchool::Death => self.dec_spell_death,
            MagicSchool::Elemental => self.dec_spell_elemental,
        }
    }

    /// Magic power floor for a school.
    pub fn min_spell(&self, school: MagicSchool) -> i32 {
        match school {
            MagicSchool::Life => self.min_spell_life,
            MagicSchool::Death => self.min_spell_death,
            MagicSchool::Elemental => self.min_spell_elemental,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNITS: &str = "\
[1 Hero]\r\n\
GlobalIndex=1\r\n\
Name=Hero\r\n\
Cost=280\r\n\
StartExpirience=100\r\n\
LevelMultipler=150\r\n\
Hits=80\r\n\
AttackBlow=45\r\n\
d-Hits=8\r\n\
d-AttackBlow=3\r\n\
d-Bogus=1\r\n\
Bonus=SpearDefense\r\n\
NextUnit1=Squire\r\n\
NextUnit1Level=1\r\n\
NextUnit2=Nobody\r\n\
NextUnit2Level=2\r\n\
Custom=yes\r\n\
[2 Squire]\r\n\
GlobalIndex=2\r\n\
Name=Squire\r\n\
Cost=40\r\n\
StartExpirience=20\r\n\
MagicPower=20\r\n\
Magic=DeathMagic\r\n\
MagicDirection=ToAlly\r\n\
Nature=Undead\r\n\
Bonus=Splash\r\n\
Evasion=15\r\n";

    #[test]
    fn units_parse_fields_mods_and_upgrades() {
        let loaded = parse_units(&Ini::parse(UNITS));
        assert_eq!(loaded.warnings, ["Hero (GlobalIndex 1): upgrade to Nobody: no unit of that name, no option"]);
        let units = loaded.value;
        assert_eq!(units.len(), 2);
        let h = &units[0];
        assert_eq!((h.id, h.cost, h.hits, h.attack_blow, h.start_experience, h.level_multiplier), (1, 280, 80, 45, 100, 150));
        assert_eq!(h.stat(Stat::AttackBlow), 45);
        assert_eq!(h.level_up, StatMods::from([(Stat::Hits, 8), (Stat::AttackBlow, 3)]));
        assert_eq!(h.bonus, Some(Bonus::SpearDefense));
        assert_eq!(h.nature, Nature::Normal);
        assert_eq!(h.magic, None);
        // The unknown name is no option, so the lone option left moves to the middle slot.
        assert_eq!(h.upgrades.len(), 1);
        assert_eq!((h.upgrades[0].target, h.upgrades[0].level, h.upgrades[0].slot), (Some(2), 1, 2));
        assert_eq!(h.extra.get("Custom").map(String::as_str), Some("yes"));
        assert_eq!(h.extra.get("d-Bogus").map(String::as_str), Some("1"));
        assert_eq!(h.extra.len(), 2);
        let s = &units[1];
        assert_eq!(s.magic, Some(MagicSchool::Death));
        assert_eq!(s.magic_direction, Some(MagicDirection::ToAlly));
        assert_eq!(s.nature, Nature::Undead);
        assert_eq!(s.bonus, Some(Bonus::Splash));
        assert_eq!(s.evasion, Some(15));
        assert!(s.extra.is_empty());
    }

    #[test]
    fn the_loader_moves_upgrade_options_as_the_original() {
        // 0x4e0448: a lone option goes to slot 2, two options always end in slots 1 and 3.
        let up = |slot| Upgrade { target_name: String::new(), target: None, level: 0, slot };
        let slots = |given: &[u8]| {
            let mut u: Vec<Upgrade> = given.iter().map(|&n| up(n)).collect();
            normalise_upgrade_slots(&mut u);
            u.iter().map(|u| u.slot).collect::<Vec<_>>()
        };
        assert_eq!(slots(&[1]), [2]);
        assert_eq!(slots(&[2]), [2]);
        assert_eq!(slots(&[3]), [2]);
        assert_eq!(slots(&[1, 2]), [1, 3]);
        assert_eq!(slots(&[2, 3]), [1, 3]);
        assert_eq!(slots(&[1, 3]), [1, 3]);
        assert_eq!(slots(&[1, 2, 3]), [1, 2, 3]);
    }

    #[test]
    fn values_are_read_loosely_and_names_exactly() {
        // saves-data.md §1 rule 5 and §3: digits picked out of the text, exact enum names.
        let ini = Ini::parse(
            "[x]\nGlobalIndex=1\nName=x\nStartExpirience=1\nNature=Martian\nHits=lots\nd-Hits=2\nd-AttackBlow=?\n\
             Cost=40\nInitiative=1.5\nManevres=- 2\nMagic=deathmagic\nBonus=splash\n",
        );
        let loaded = parse_units(&ini);
        let u = &loaded.value[0];
        assert_eq!((u.nature, u.hits, u.cost, u.initiative, u.manevres), (Nature::Normal, 0, 40, 15, -2));
        assert_eq!(u.level_up, StatMods::from([(Stat::Hits, 2), (Stat::AttackBlow, 0)]));
        assert_eq!((u.magic, u.bonus.as_ref()), (None, None), "names are case-sensitive; unknown is none");
        assert_eq!(
            loaded.warnings,
            ["[x] Magic=deathmagic: unknown value, read as none", "[x] Nature=Martian: unknown value, read as none", "[x] Bonus=splash: unknown value, read as none"]
        );
        // `People` (the editor's name) and `ToAll` are no names the exe knows: 0, the
        // ordinary nature and "all" directions, without a warning for `People`.
        let ini = Ini::parse("[x]\nGlobalIndex=1\nName=x\nStartExpirience=1\nCost=1\nNature=People\nMagicDirection=ToAll\n");
        let loaded = parse_units(&ini);
        assert_eq!(loaded.value[0].nature, Nature::Normal);
        assert!(loaded.value[0].magic_direction.is_none_or(|d| d.hits_enemies() && d.helps_allies()));
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(MagicSchool::parse("Life"), None, "units and items write LifeMagic");
        assert_eq!(MagicSchool::parse("LifeMagic"), Some(MagicSchool::Life));
    }

    #[test]
    fn a_unit_is_a_section_with_name_start_experience_and_cost() {
        let unit = |gi: &str| format!("GlobalIndex={gi}\nName=n{gi}\nStartExpirience=1\nCost=1\n");
        let ini = Ini::parse(&format!(
            "[a]\n{}[b]\n{}[c]\n{}[d]\n{}[e]\nGlobalIndex=9\nName=x\nCost=1\n[f]\n{}[g]\n{}",
            unit("abc"),
            unit("-4"),
            unit(""),
            unit("7"),
            unit("x8"),
            unit("7").replace("Cost=1", "Cost=2"),
        ));
        let loaded = parse_units(&ini);
        // abc and the empty index read 0 and -4 reads -4: records before the table, skipped;
        // x8 reads 8; [e] has no StartExpirience; the second 7 overwrites the first.
        let got: Vec<(u32, i32)> = loaded.value.iter().map(|u| (u.id, u.cost)).collect();
        assert_eq!(got, [(8, 1), (7, 2)]);
        assert_eq!(loaded.warnings.len(), 3, "{:?}", loaded.warnings);
        assert!(loaded.strict().is_err());
    }

    #[test]
    fn upgrades_go_by_section_position() {
        // Pass 2 gives the options of the section at position p to the type with id p + 1.
        let ini = Ini::parse(
            "[first]\nGlobalIndex=2\nName=A\nStartExpirience=1\nCost=1\nNextUnit1=B\nNextUnit1Level=1\n\
             [second]\nGlobalIndex=1\nName=B\nStartExpirience=1\nCost=1\nNextUnit1=A\nNextUnit3=A\n",
        );
        let units = parse_units(&ini).strict().unwrap();
        let (a, b) = (units.iter().find(|u| u.name == "A").unwrap(), units.iter().find(|u| u.name == "B").unwrap());
        assert_eq!((b.id, b.upgrades.iter().map(|u| (u.target, u.slot)).collect::<Vec<_>>()), (1, vec![(Some(1), 2)]), "B gets the first section's option");
        assert_eq!(a.upgrades.iter().map(|u| (u.target, u.slot)).collect::<Vec<_>>(), [(Some(2), 1), (Some(2), 3)]);
        // A name matches the last type of that name in id order.
        let ini = Ini::parse(
            "[a]\nGlobalIndex=1\nName=X\nStartExpirience=1\nCost=1\nNextUnit2=Twin\n\
             [b]\nGlobalIndex=3\nName=Twin\nStartExpirience=1\nCost=1\n\
             [c]\nGlobalIndex=2\nName=Twin\nStartExpirience=1\nCost=1\n",
        );
        let units = parse_units(&ini).strict().unwrap();
        assert_eq!(units[0].upgrades[0].target, Some(3));
    }

    #[test]
    fn bonus_indices() {
        assert_eq!(Bonus::VANILLA.len(), 21);
        assert_eq!(Bonus::parse("SpearDefense").vanilla_index(), Some(1));
        assert_eq!(Bonus::parse("FlankStrike").vanilla_index(), Some(21));
        assert_eq!(Bonus::parse("GodStrike").vanilla_index(), Some(8));
        assert_eq!(Bonus::parse("Berserk").vanilla_index(), None);
        assert_eq!(Bonus::parse("Berserk").token(), "Berserk");
        assert_eq!(Bonus::known_token("Berserk"), Some(Bonus::Berserk));
        assert_eq!(Bonus::known_token("berserk"), None, "exact names only");
        assert_eq!(Bonus::COMMUNITY.len(), 31);
        assert_eq!(Bonus::Hunger.index(), Some(22));
        assert_eq!(Bonus::PoisonS.index(), Some(32));
        assert_eq!(Bonus::PoisonArmorIgnore.index(), Some(45));
        assert_eq!(Bonus::HoldLine.index(), Some(46));
        assert_eq!(Bonus::Assault.index(), Some(50));
        assert_eq!(Bonus::FateGift.index(), Some(52));
        assert!(Bonus::Flying.is_community() && !Bonus::Poison.is_community());
        for b in Bonus::known() {
            assert_eq!(&Bonus::parse(b.token()), b);
        }
        assert_eq!(Bonus::parse("Telepathy"), Bonus::Other("Telepathy".into()));
        assert_eq!(Bonus::parse("Telepathy").index(), None);
        assert_eq!(Nature::People.index(), 6);
    }

    #[test]
    fn artefacts_parse() {
        let ini = Ini::parse(
            "[5 Sword]\nGlobalIndex=5\nName=Sword\nIcon=A005.Tga\nCost=-1700\nType=BlowWeapon\n\
             f-AttackBlow=55\nd-Initiative=-1\np-Hits=10\nMagic=LifeMagic\nBonus=ArmorIgnore\n",
        );
        let a = &parse_artefacts(&ini).strict().unwrap()[0];
        assert_eq!((a.id, a.cost, a.kind), (5, -1700, ArtefactType::BlowWeapon));
        assert!(a.is_personal() && a.kind.is_weapon());
        assert_eq!(a.fixed, StatMods::from([(Stat::AttackBlow, 55)]));
        assert_eq!(a.add, StatMods::from([(Stat::Initiative, -1)]));
        assert_eq!(a.percent, StatMods::from([(Stat::Hits, 10)]));
        assert_eq!(a.magic, Some(MagicSchool::Life));
        assert_eq!(a.bonus, Some(Bonus::ArmorIgnore));
    }

    #[test]
    fn an_item_needs_icon_name_type_and_cost() {
        // 0x498eb4: other sections keep an empty record; an unknown Type is a potion and the
        // compare is case-sensitive (0x4990cc).
        let item = |gi: u32, ty: &str| format!("[{gi}]\nGlobalIndex={gi}\nName=i{gi}\nIcon=x\nCost=1\nType={ty}\n");
        let ini = Ini::parse(&format!(
            "{}{}{}{}[9]\nGlobalIndex=9\nName=i9\nCost=1\nType=Ring\n[10]\nGlobalIndex=10\nName=i10\nIcon=x\nCost=\nType=Ring\n",
            item(5, ""),
            item(6, "Spoon"),
            item(7, "BlowWeapon"),
            item(8, "shotweapon"),
        ));
        let loaded = parse_artefacts(&ini);
        let kinds = [(6, ArtefactType::Potion), (7, ArtefactType::BlowWeapon), (8, ArtefactType::Potion)];
        assert_eq!(loaded.value.iter().map(|a| (a.id, a.kind)).collect::<Vec<_>>(), kinds);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    }

    const SPELLS: &str = "\
[Spell]\r\n\
Name=One\r\n\
CostGold=150\r\n\
CostMana=200\r\n\
Type=Life\r\n\
TimeWork=\r\n\
TimeCast=4\r\n\
Target=Hero\r\n\
Icon1=Life-2\r\n\
Icon2=\r\n\
ColorC1=160,40,100\r\n\
ColorC2=\r\n\
Effect1=P-Flare,70,150,70,3000,65,1200,0\r\n\
Effect2=\r\n\
DeltaFixedHits=30\r\n\
[Spell]\r\n\
Name=Two\r\n\
CostGold=1\r\n\
Type=Death\r\n\
TimeWork=10\r\n\
Target=Enemy\r\n\
p-AttackBlow=-20\r\n\
d-Manevres=-1\r\n\
p-LifeLose=5\r\n\
----------\r\n\
[Spell]\r\n\
Name=Template\r\n\
CostGold=\r\n\
CostMana=\r\n\
Type=\r\n\
TimeWork=\r\n\
TimeCast=\r\n\
Target=\r\n\
d-Hits=\r\n\
[MapEditorSpecialOptions]\r\n\
Generated=1\r\n";

    #[test]
    fn spells_by_order_skipping_template() {
        let spells = parse_spells(&Ini::parse(SPELLS)).strict().unwrap();
        assert_eq!(spells.len(), 2);
        let a = &spells[0];
        assert_eq!((a.id, a.cost_gold, a.cost_mana, a.time_work, a.time_cast), (1, 150, 200, None, Some(4)));
        assert_eq!((a.school, a.target), (Some(MagicSchool::Life), Some(SpellTarget::Hero)));
        assert_eq!(a.icons[0], SpellIcon { image: Some("Life-2".into()), tint: Some([160, 40, 100]) });
        assert_eq!(a.icons[1], SpellIcon::default());
        let e = a.effects[0].as_ref().unwrap();
        assert_eq!((e.file.as_str(), e.rgb, e.duration_ms, e.y_offset, e.scale_milli, e.start_ms), ("P-Flare", [70, 150, 70], 3000, 65, 1200, 0));
        assert_eq!(a.effects[1], None);
        assert_eq!(a.delta_fixed_hits, Some(30));
        assert!(a.extra.is_empty(), "{:?}", a.extra);
        let b = &spells[1];
        assert_eq!((b.id, b.time_work, b.target), (2, Some(10), Some(SpellTarget::Enemy)));
        assert_eq!(SpellTarget::parse("enemy"), None, "case-sensitive: the hero's army");
        assert_eq!(b.percent, StatMods::from([(Stat::AttackBlow, -20)]));
        assert_eq!(b.add, StatMods::from([(Stat::Manevres, -1)]));
        assert_eq!(b.life_lose_percent, Some(5));
        assert!(b.extra.is_empty(), "{:?}", b.extra);
    }

    #[test]
    fn a_spell_is_its_section_position() {
        // 0x4e0c60: the record is the section's position among all sections; a spell needs
        // Name, Type and CostGold. Type: Life and Death, anything else Elemental; Target:
        // Enemy and OneEnemy, anything else (or none) the hero's army.
        let ini = Ini::parse(
            "[A]\nName=a\nType=Fire\nCostGold=1\n[junk]\nName=j\nCostGold=1\n[B]\nName=b\nType=death\nCostGold=2\nTarget=OneEnemy\n",
        );
        let spells = parse_spells(&ini).strict().unwrap();
        let got: Vec<(u32, Option<MagicSchool>, Option<SpellTarget>)> = spells.iter().map(|s| (s.id, s.school, s.target)).collect();
        assert_eq!(got, [(1, Some(MagicSchool::Elemental), Some(SpellTarget::Hero)), (3, Some(MagicSchool::Elemental), Some(SpellTarget::OneEnemy))]);
    }

    #[test]
    fn bad_effect_is_ignored() {
        let ini = Ini::parse("[S]\nName=x\nType=Life\nCostGold=1\nEffect1=file,1,2\n");
        let loaded = parse_spells(&ini);
        assert_eq!(loaded.value[0].effects[0], None);
        assert_eq!(loaded.warnings, ["[S] Effect1=file,1,2: not an effect, ignored"]);
    }

    #[test]
    fn blank_effect_number_is_zero() {
        let ini = Ini::parse("[S]\nName=x\nType=Life\nCostGold=1\nEffect2=file,255,0,0,2000,47,1500,\n");
        let spell = &parse_spells(&ini).strict().unwrap()[0];
        assert_eq!(spell.effects[1].as_ref().map(|e| (e.scale_milli, e.start_ms)), Some((1500, 0)));
    }

    #[test]
    fn global_options_are_read_as_the_original() {
        let ini = Ini::parse(
            "[Costs]\nShipCost=300\n[GlobalOptions]\nRow2Def=7\nDecSpellelemental=9\nDecSpellLife=1.5\n\
             MinAtackArmyTarget=1,1,100,50,50\nMixHealingTarget=50,150,1,50,50\nMinRandomTarget=1,2\nNewKey=3\n\
             [AIArmyGeneration]\nNormal=4,5,6\nUndead=43\n",
        );
        let o = GlobalOptions::from_ini(&ini).strict().unwrap();
        assert_eq!((o.ship_cost, o.row2_def, o.dec_spell_elemental, o.dec_spell_life), (300, 7, 9, 15));
        assert_eq!(o.dec_spell(MagicSchool::Elemental), 9);
        assert_eq!(o.battle_end_turn, 0, "a missing key reads 0, not its vanilla 25");
        assert_eq!(o.difficulty_factor, 120, "not a _Global.ini value");
        assert_eq!(o.ai_targets.min_attack_army, Some([1, 1, 100, 50, 50]));
        assert_eq!(o.ai_targets.min_random, Some([1, 2, 0, 0, 0]), "fields past the end read 0");
        assert_eq!(o.ai_targets.min_healing, None);
        assert!(o.extra.contains_key("MixHealingTarget") && o.extra.contains_key("NewKey"));
        assert_eq!(o.extra.len(), 2);
        assert_eq!(o.army_generation, vec![("Normal".to_string(), vec![4, 5, 6]), ("Undead".to_string(), vec![43])]);
        // The exe asks for `DecSpellelemental`: the usual spelling is another key.
        let o = GlobalOptions::from_ini(&Ini::parse("[GlobalOptions]\nDecSpellElemental=9\n")).value;
        assert_eq!(o.dec_spell_elemental, 0);
        let bad = Ini::parse("[GlobalOptions]\nRow2Def=x\n[AIArmyGeneration]\nNormal=4,x,6,\n");
        let loaded = GlobalOptions::from_ini(&bad);
        assert_eq!(loaded.value.row2_def, 0);
        assert_eq!(loaded.value.army_generation, vec![("Normal".to_string(), vec![4, 6])]);
        assert_eq!(loaded.warnings, ["[AIArmyGeneration] Normal=4,x,6,: x is not a unit id, ignored"]);
    }

    #[test]
    fn a_missing_section_reads_from_the_one_selected_before() {
        // No [Costs]: ShipCost comes from [BattleEffects], selected at the first map load;
        // without that too, from [GlobalOptions]. No [GlobalOptions]: the first section.
        let o = GlobalOptions::from_ini(&Ini::parse("[GlobalOptions]\nShipCost=7\n[BattleEffects]\nShipCost=8\n")).value;
        assert_eq!(o.ship_cost, 8);
        let o = GlobalOptions::from_ini(&Ini::parse("[GlobalOptions]\nShipCost=7\nRow2Def=3\n")).value;
        assert_eq!((o.ship_cost, o.row2_def), (7, 3));
        let o = GlobalOptions::from_ini(&Ini::parse("[Other]\nRow2Def=4\n[Costs]\nShipCost=1\n")).value;
        assert_eq!((o.row2_def, o.ship_cost), (4, 1));
        let o = GlobalOptions::from_ini(&Ini::parse("")).value;
        assert_eq!((o.row2_def, o.ship_cost), (0, 0));
    }
}

#[cfg(test)]
mod real_install {
    //! The player's install; skipped without `RAZDOR_DT_DIR`. Numbers only.
    use super::*;
    use crate::dt::install::{DtInstall, ENV_VAR};

    #[test]
    fn every_bonus_token_of_the_install_is_known() {
        let Some(dir) = std::env::var_os(ENV_VAR) else { return };
        let dir = std::path::Path::new(&dir);
        let (mut known, mut community, mut unknown) = (0, 0, 0);
        for file in [crate::dt::install::UNITS_FILE, crate::dt::install::ARTEFACTS_FILE] {
            let path = crate::dt::install::find_path(dir, file).expect("file present");
            let ini = Ini::from_cp1251(&std::fs::read(path).expect("file reads"));
            for token in ini.sections.iter().filter_map(|s| s.get_nonempty("Bonus")) {
                match Bonus::known_token(token) {
                    Some(b) => {
                        known += 1;
                        community += usize::from(b.is_community());
                    }
                    None => unknown += 1,
                }
            }
        }
        let dt = DtInstall::load(dir).expect("install loads");
        let on_defs = dt.units.iter().filter(|u| u.bonus.is_some()).count() + dt.artefacts.iter().filter(|a| a.bonus.is_some()).count();
        println!("{known} bonuses on units and items ({community} Community), {unknown} unknown tokens");
        assert_eq!(unknown, 0, "unknown bonus tokens");
        assert!(known > 0);
        assert_eq!(on_defs, known);
    }
}
