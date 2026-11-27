//! Spells (M8, spec "## M8" and "### M8 spike result"): the SPEL records and the NPCS lists.
//!
//! Two kinds of spell. A unit's casts are built per unit card and level from its school, direction
//! and magic power (DT has no unit-to-spell table, battle.md §0, §3): `rz_strike_u<unit>_l<level>`,
//! `rz_heal_…`, `rz_bless_…` and `rz_shot_…`. The hero's world spells come from the units JSON's
//! `spells` array: `rz_spell_<id>`. The SPEL layout (NAME, FNAM, SPDT 12 B, one ENAM 24 B per
//! effect) was checked with esmtool and OpenMW in the spike; the effect ids are OpenMW's
//! `ESM::MagicEffect` numbers, checked against the Game Template's MGEF records (their `Index:`).

use crate::actors::Body;
use crate::tes3::{self, Record};
use crate::units::{Unit, Units, WorldSpell};
use std::collections::BTreeMap;

/// OpenMW's `ESM::MagicEffect` ids (the Game Template's MGEF `Index:` numbers).
pub mod effect {
    pub const SHIELD: u16 = 3;
    pub const FIRE_DAMAGE: u16 = 14;
    pub const DAMAGE_ATTRIBUTE: u16 = 22;
    pub const DAMAGE_HEALTH: u16 = 23;
    pub const WEAKNESS_TO_FIRE: u16 = 28;
    pub const WEAKNESS_TO_FROST: u16 = 29;
    pub const WEAKNESS_TO_SHOCK: u16 = 30;
    pub const WEAKNESS_TO_MAGICKA: u16 = 31;
    pub const WEAKNESS_TO_POISON: u16 = 35;
    pub const RESTORE_HEALTH: u16 = 75;
    pub const FORTIFY_ATTRIBUTE: u16 = 79;
    pub const FORTIFY_HEALTH: u16 = 80;
    pub const RESIST_FIRE: u16 = 90;
    pub const RESIST_FROST: u16 = 91;
    pub const RESIST_SHOCK: u16 = 92;
    pub const RESIST_MAGICKA: u16 = 93;
    pub const RESIST_POISON: u16 = 97;
    pub const FORTIFY_ATTACK: u16 = 117;
}

/// `ESM::Attribute` index of Speed (the attribute order of OpenMW: strength, intelligence,
/// willpower, agility, speed, ...).
pub const ATTRIBUTE_SPEED: i8 = 4;
/// "No attribute" and "no skill" in an ENAM.
pub const NONE: i8 = -1;
/// SPDT flag "always succeeds" (OpenMW's spell flags).
pub const FLAG_ALWAYS_SUCCEEDS: u32 = 0x4;
/// The cost of every unit spell.
pub const UNIT_SPELL_COST: u32 = 1;
/// The range of an effect on the caster (`0`) or on the target (`2`).
pub const RANGE_SELF: u32 = 0;
pub const RANGE_TARGET: u32 = 2;
/// The placeholder initiative of the bodies without a units file (their interval, combat_data).
pub const PLACEHOLDER_INITIATIVE: i64 = 10;
/// The magicka of a body that casts, at least (spec M8).
pub const CASTER_MAGICKA: i64 = 500;

/// One effect of a SPEL (an ENAM subrecord).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Effect {
    pub id: u16,
    /// The attribute of Fortify/Damage Attribute, `NONE` otherwise.
    pub attribute: i8,
    pub range: u32,
    pub duration: u32,
    pub min: u32,
    pub max: u32,
}

impl Effect {
    /// An effect without an attribute: `min` and `max` are clamped to 0 and up.
    pub fn new(id: u16, range: u32, duration: u32, min: i64, max: i64) -> Effect {
        Effect { id, attribute: NONE, range, duration, min: magnitude(min), max: magnitude(max) }
    }

    /// The same effect on one attribute.
    pub fn on_attribute(mut self, attribute: i8) -> Effect {
        self.attribute = attribute;
        self
    }

    /// The 24 bytes of the ENAM subrecord: effect u16, skill i8 (-1), attribute i8, range, area (0),
    /// duration, min, max (u32 each).
    pub fn enam(&self) -> Vec<u8> {
        let mut v = Vec::with_capacity(24);
        v.extend_from_slice(&self.id.to_le_bytes());
        v.push(NONE as u8);
        v.push(self.attribute as u8);
        for x in [self.range, 0, self.duration, self.min, self.max] {
            v.extend_from_slice(&x.to_le_bytes());
        }
        v
    }
}

fn magnitude(v: i64) -> u32 {
    v.clamp(0, i64::from(u32::MAX)) as u32
}

/// A SPEL record: NAME (the id), FNAM (the name, cp1251), SPDT (type 0 = spell, cost, flags) and one
/// ENAM per effect.
pub fn spel(id: &str, name: &str, cost: u32, flags: u32, effects: &[Effect]) -> Record {
    let mut spdt = Vec::with_capacity(12);
    spdt.extend_from_slice(&0u32.to_le_bytes());
    spdt.extend_from_slice(&cost.to_le_bytes());
    spdt.extend_from_slice(&flags.to_le_bytes());
    let mut rec = Record::new(b"SPEL")
        .sub(b"NAME", tes3::zstring(id))
        .sub(b"FNAM", tes3::zstring(name))
        .sub(b"SPDT", spdt);
    for e in effects {
        rec = rec.sub(b"ENAM", e.enam());
    }
    rec
}

/// The NPCS subrecord that names one spell of a body: 32 bytes, the id zero-padded.
pub fn npcs(id: &str) -> Vec<u8> {
    let mut v = vec![0u8; 32];
    let n = id.len().min(32);
    v[..n].copy_from_slice(&id.as_bytes()[..n]);
    v
}

/// A pair (blow or shot) as the combat numbers take it: (half rounded down, at least 1; the value),
/// or `None` for no attack. The same rule as `actors::real_stats`.
pub fn pair(value: i64) -> Option<(i64, i64)> {
    (value > 0).then(|| ((value / 2).max(1), value))
}

/// A unit's casts at a 0-based level: the strike (`school`, magnitude), the heal and the bless
/// magnitudes. Each is `None` when the unit casts nothing of that kind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UnitMagic {
    pub strike: Option<(&'static str, i64)>,
    pub heal: Option<i64>,
    pub bless: Option<i64>,
}

/// The magic of a unit (spec M8 table). Strikes need direction `to_enemy` or `to_all`; heal and
/// bless need `to_ally` or `to_all`. The school sets the kind: elemental strikes are Fire Damage at
/// `3P/4`, death and life strikes Damage Health at `P`; life heals `P`, elemental heals `P/2`,
/// death blesses `P/6 + 1`. A unit with no power, or a magnitude of 0, casts nothing of that kind.
pub fn unit_magic(u: &Unit, level: i64) -> UnitMagic {
    let power = u.magic_power_at(level);
    let direction = u.magic_direction.as_deref().unwrap_or("");
    let school = u.magic.as_deref().unwrap_or("");
    let mut m = UnitMagic::default();
    if power <= 0 {
        return m;
    }
    if matches!(direction, "to_enemy" | "to_all") {
        m.strike = match school {
            "elemental" => Some(("elemental", power * 3 / 4)),
            "death" => Some(("death", power)),
            "life" => Some(("life", power)),
            _ => None,
        }
        .filter(|(_, mag)| *mag > 0);
    }
    if matches!(direction, "to_ally" | "to_all") {
        m.heal = match school {
            "life" => Some(power),
            "elemental" => Some(power / 2),
            _ => None,
        }
        .filter(|mag| *mag > 0);
        m.bless = match school {
            "death" => Some(power / 6 + 1),
            _ => None,
        }
        .filter(|mag| *mag > 0);
    }
    m
}

/// One unit spell: its record id, its display name and its effects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitSpell {
    pub id: String,
    pub name: String,
    pub effects: Vec<Effect>,
}

/// The unit spells of a unit card at a 0-based level (spec M8 table): strike, heal, bless and, for a
/// unit with a ranged attack, shot (Damage Health between the shot pair, range target).
pub fn unit_spells(u: &Unit, level: i64) -> Vec<UnitSpell> {
    let m = unit_magic(u, level);
    let tag = format!("u{}_l{}", u.id, level);
    let label = format!("{} {}", u.id, level);
    let mut out = Vec::new();
    if let Some((school, mag)) = m.strike {
        let id = if school == "elemental" { effect::FIRE_DAMAGE } else { effect::DAMAGE_HEALTH };
        out.push(UnitSpell {
            id: format!("rz_strike_{tag}"),
            name: format!("Strike {label}"),
            effects: vec![Effect::new(id, RANGE_TARGET, 0, mag, mag)],
        });
    }
    if let Some(mag) = m.heal {
        out.push(UnitSpell {
            id: format!("rz_heal_{tag}"),
            name: format!("Heal {label}"),
            effects: vec![Effect::new(effect::RESTORE_HEALTH, RANGE_SELF, 1, mag, mag)],
        });
    }
    if let Some(mag) = m.bless {
        out.push(UnitSpell {
            id: format!("rz_bless_{tag}"),
            name: format!("Bless {label}"),
            effects: vec![Effect::new(effect::FORTIFY_ATTACK, RANGE_SELF, 30, mag, mag)],
        });
    }
    if let Some((lo, hi)) = pair(u.attack_shot_at(level)) {
        out.push(UnitSpell {
            id: format!("rz_shot_{tag}"),
            name: format!("Shot {label}"),
            effects: vec![Effect::new(effect::DAMAGE_HEALTH, RANGE_TARGET, 0, lo, hi)],
        });
    }
    out
}

/// The ids of a unit's spells at a level (the NPCS list of its body).
pub fn spell_ids(u: &Unit, level: i64) -> Vec<String> {
    unit_spells(u, level).into_iter().map(|s| s.id).collect()
}

/// The record id of a world spell: `rz_spell_<id>`.
pub fn world_id(id: i64) -> String {
    format!("rz_spell_{id}")
}

/// Whether a world spell is in the hero's magic menu. A damage spell on the hero is an event spell
/// (the events cast it on the player); every other spell is in the menu.
pub fn world_menu(s: &WorldSpell) -> bool {
    !(s.kind == "damage" && s.target.as_deref() == Some("hero"))
}

/// The duration of a world spell in real seconds: `time_work_hours` of game time at the clock's
/// scale, capped at one game day. A missing or zero duration is instant (0).
pub fn world_duration(s: &WorldSpell, time_scale: i64) -> u32 {
    let scale = time_scale.max(1);
    let day = 86_400 / scale;
    match s.time_work_hours {
        Some(h) if h > 0 => (h.saturating_mul(3600) / scale).min(day) as u32,
        _ => 0,
    }
}

/// The Fortify/Shield/... effects of a positive stat change on the caster (key: a unit stat name).
fn positive(key: &str) -> Vec<Effect> {
    let make = |id: u16, attr: Option<i8>| {
        let e = Effect::new(id, RANGE_SELF, 0, 0, 0);
        match attr {
            Some(a) => e.on_attribute(a),
            None => e,
        }
    };
    match key {
        "attack_blow" | "attack_shot" => vec![make(effect::FORTIFY_ATTACK, None)],
        "defence_blow" | "defence_shot" => vec![make(effect::SHIELD, None)],
        "hit_points" => vec![make(effect::FORTIFY_HEALTH, None)],
        "initiative" => vec![make(effect::FORTIFY_ATTRIBUTE, Some(ATTRIBUTE_SPEED))],
        "regeneration" => vec![make(effect::RESTORE_HEALTH, None)],
        "protect_life" => vec![make(effect::RESIST_MAGICKA, None)],
        "protect_death" => vec![make(effect::RESIST_POISON, None)],
        "protect_elemental" => vec![
            make(effect::RESIST_FIRE, None),
            make(effect::RESIST_FROST, None),
            make(effect::RESIST_SHOCK, None),
        ],
        _ => Vec::new(),
    }
}

/// The negative effects of a stat change on an enemy (Damage Attribute, Weakness).
fn negative(key: &str) -> Vec<Effect> {
    let make = |id: u16, attr: Option<i8>| {
        let e = Effect::new(id, RANGE_TARGET, 0, 0, 0);
        match attr {
            Some(a) => e.on_attribute(a),
            None => e,
        }
    };
    match key {
        "initiative" => vec![make(effect::DAMAGE_ATTRIBUTE, Some(ATTRIBUTE_SPEED))],
        "protect_life" => vec![make(effect::WEAKNESS_TO_MAGICKA, None)],
        "protect_death" => vec![make(effect::WEAKNESS_TO_POISON, None)],
        "protect_elemental" => vec![
            make(effect::WEAKNESS_TO_FIRE, None),
            make(effect::WEAKNESS_TO_FROST, None),
            make(effect::WEAKNESS_TO_SHOCK, None),
        ],
        _ => Vec::new(),
    }
}

/// The effects of a world spell (spec M8, "World spells"). `heal`: Restore Health on self, instant.
/// `modifier`: each stat key of `add` and `percent` is one effect for the duration. On a hero target
/// a positive value gives the positive effect on self; on an enemy target a negative value gives
/// the negative effect on the target (1 point per percent). `damage`: Damage Health, instant, on the
/// target (or on the hero for an event spell). Other kinds have no effects.
pub fn world_effects(s: &WorldSpell, time_scale: i64) -> Vec<Effect> {
    let enemy = matches!(s.target.as_deref(), Some("enemy") | Some("one_enemy"));
    let dur = world_duration(s, time_scale);
    match s.kind.as_str() {
        "heal" => {
            let mag = s.delta_fixed_hits.unwrap_or(0);
            if mag > 0 {
                vec![Effect::new(effect::RESTORE_HEALTH, RANGE_SELF, 0, mag, mag)]
            } else {
                Vec::new()
            }
        }
        "damage" => {
            let mag = -s.delta_fixed_hits.unwrap_or(0);
            let range = if enemy { RANGE_TARGET } else { RANGE_SELF };
            if mag > 0 {
                vec![Effect::new(effect::DAMAGE_HEALTH, range, 0, mag, mag)]
            } else {
                Vec::new()
            }
        }
        "modifier" => {
            let mut out = Vec::new();
            for (key, &v) in s.add.iter().chain(s.percent.iter()) {
                if enemy {
                    if v < 0 {
                        for e in negative(key) {
                            out.push(Effect { duration: dur, min: magnitude(-v), max: magnitude(-v), ..e });
                        }
                    }
                } else if v > 0 {
                    for e in positive(key) {
                        out.push(Effect { duration: dur, min: magnitude(v), max: magnitude(v), ..e });
                    }
                }
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The SPEL record of a world spell: cost = `cost_mana`, flags 0.
pub fn world_record(s: &WorldSpell, time_scale: i64) -> Record {
    let cost = s.cost_mana.clamp(0, i64::from(u32::MAX)) as u32;
    spel(&world_id(s.id), &s.name, cost, 0, &world_effects(s, time_scale))
}

/// Every SPEL of the plugin, sorted by id: the world spells of the units file and the unit spells of
/// every body with a card in the file. Without a units file there are none.
pub fn records(units: Option<&Units>, bodies: &BTreeMap<String, Body>, time_scale: i64) -> Vec<Record> {
    let mut by_id: BTreeMap<String, Record> = BTreeMap::new();
    if let Some(us) = units {
        for s in us.spells() {
            by_id.insert(world_id(s.id), world_record(s, time_scale));
        }
        for b in bodies.values() {
            let Some(u) = us.get(b.unit) else { continue };
            for sp in unit_spells(u, b.level) {
                by_id
                    .entry(sp.id.clone())
                    .or_insert_with(|| spel(&sp.id, &sp.name, UNIT_SPELL_COST, FLAG_ALWAYS_SUCCEEDS, &sp.effects));
            }
        }
    }
    by_id.into_values().collect()
}

/// The interval of a body's actions in seconds: `max(1.0, 4.0 - 0.15 * initiative)` (spike result).
pub fn interval(initiative: i64) -> f64 {
    (4.0 - 0.15 * initiative as f64).max(1.0)
}

/// The combat numbers of a unit card at a level (combat_data.lua): melee and shot pairs, the strike
/// and the heal and bless magnitudes, and the interval.
#[derive(Debug, Clone, PartialEq)]
pub struct Combat {
    pub melee: Option<(i64, i64)>,
    pub shot: Option<(i64, i64)>,
    pub strike: Option<(&'static str, i64)>,
    pub heal: Option<i64>,
    pub bless: Option<i64>,
    pub interval: f64,
}

/// The combat numbers of a unit card at a 0-based level.
pub fn combat_of(u: &Unit, level: i64) -> Combat {
    let m = unit_magic(u, level);
    Combat {
        melee: pair(u.attack_blow_at(level)),
        shot: pair(u.attack_shot_at(level)),
        strike: m.strike,
        heal: m.heal,
        bless: m.bless,
        interval: interval(u.initiative_at(level)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{Units, FORMAT, VERSION};

    /// A synthetic unit card with the casting fields the tests vary (no game data).
    fn unit(id: i64, blow: i64, shot: i64, power: i64, school: Option<&str>, direction: Option<&str>) -> serde_json::Value {
        serde_json::json!({
            "id": id, "name": format!("Test {id}"), "cost": 1, "hit_points": 60,
            "attack": {"blow": blow, "shot": shot}, "defence": {"blow": 10, "shot": 4},
            "magic_power": power, "magic": school, "magic_direction": direction,
            "protection": {"life": 20, "death": 20, "elemental": 20}, "initiative": 10,
            "actions": 1, "regeneration": 0, "vampirism": 0, "ranged": shot > 0, "flying": false,
            "nature": "normal", "bonus": null, "evasion": null, "min_magic_power": null,
            "mana_drain": null, "experience": {"start": 60, "growth_percent": 140},
            "level_up": {}
        })
    }

    fn units_of(units: Vec<serde_json::Value>, spells: serde_json::Value) -> Units {
        let text = serde_json::json!({
            "format": FORMAT, "version": VERSION, "language": "ru", "count": units.len(),
            "units": units, "spell_count": 0, "spells": spells,
        })
        .to_string();
        Units::from_json(&text).unwrap()
    }

    fn only(units: &Units, id: i64) -> &Unit {
        units.get(id).unwrap()
    }

    /// The effects of a unit's spell with the given id prefix.
    fn spell_of(units: &Units, id: i64, level: i64, prefix: &str) -> Option<UnitSpell> {
        unit_spells(only(units, id), level).into_iter().find(|s| s.id.starts_with(prefix))
    }

    #[test]
    fn a_spel_record_has_the_esmtool_layout() {
        let e = Effect::new(effect::FIRE_DAMAGE, RANGE_TARGET, 0, 3, 5);
        let rec = spel("rz_spell_1", "Тест", 7, FLAG_ALWAYS_SUCCEEDS, &[e]);
        assert_eq!(rec.name, *b"SPEL");
        assert_eq!(rec.get(b"NAME").unwrap(), &b"rz_spell_1\0"[..]);
        assert_eq!(tes3::read_zstring(rec.get(b"FNAM").unwrap()), "Тест");
        let spdt = rec.get(b"SPDT").unwrap();
        assert_eq!(spdt, &[0, 0, 0, 0, 7, 0, 0, 0, 4, 0, 0, 0][..], "type 0, cost 7, flags 4");
        let enam = rec.get(b"ENAM").unwrap();
        assert_eq!(enam.len(), 24);
        assert_eq!(&enam[0..2], &14u16.to_le_bytes()[..], "effect id");
        assert_eq!((enam[2], enam[3]), (0xFF, 0xFF), "skill -1, attribute -1");
        assert_eq!(&enam[4..8], &2u32.to_le_bytes()[..], "range: target");
        assert_eq!(&enam[8..12], &0u32.to_le_bytes()[..], "area");
        assert_eq!(&enam[12..16], &0u32.to_le_bytes()[..], "duration");
        assert_eq!(&enam[16..20], &3u32.to_le_bytes()[..], "min");
        assert_eq!(&enam[20..24], &5u32.to_le_bytes()[..], "max");
        // Round trip through the plugin bytes.
        let back = tes3::decode(&tes3::encode(std::slice::from_ref(&rec))).unwrap();
        assert_eq!(back[0], rec);
    }

    #[test]
    fn an_attribute_effect_writes_its_attribute() {
        let e = Effect::new(effect::FORTIFY_ATTRIBUTE, RANGE_SELF, 0, 4, 4).on_attribute(ATTRIBUTE_SPEED);
        let bytes = e.enam();
        assert_eq!(bytes[2], 0xFF, "skill -1");
        assert_eq!(bytes[3], 4, "attribute speed");
        // Negative magnitudes are clamped to 0.
        assert_eq!(Effect::new(1, 0, 0, -5, -1).min, 0);
    }

    #[test]
    fn the_strike_follows_the_school_and_the_direction() {
        let units = units_of(
            vec![
                unit(1, 0, 0, 20, Some("elemental"), Some("to_enemy")),
                unit(2, 0, 0, 20, Some("death"), Some("to_enemy")),
                unit(3, 0, 0, 20, Some("life"), Some("to_all")),
                unit(4, 0, 0, 20, Some("elemental"), Some("to_ally")),
                unit(5, 0, 0, 20, Some("death"), Some("to_ally")),
                unit(6, 0, 0, 20, Some("life"), Some("to_ally")),
                unit(7, 0, 0, 20, None, Some("to_enemy")),
                unit(8, 0, 0, 20, Some("life"), None),
                unit(9, 0, 0, 0, Some("elemental"), Some("to_enemy")),
            ],
            serde_json::json!([]),
        );
        // Elemental to enemies: Fire Damage at 3P/4 on the target.
        let s = spell_of(&units, 1, 0, "rz_strike_").unwrap();
        assert_eq!(s.id, "rz_strike_u1_l0");
        assert_eq!(s.effects, vec![Effect::new(effect::FIRE_DAMAGE, RANGE_TARGET, 0, 15, 15)]);
        // Death to enemies: Damage Health at P.
        let s = spell_of(&units, 2, 0, "rz_strike_").unwrap();
        assert_eq!(s.effects, vec![Effect::new(effect::DAMAGE_HEALTH, RANGE_TARGET, 0, 20, 20)]);
        // Life to all: a strike and a heal (P, on self, 1 s).
        assert!(spell_of(&units, 3, 0, "rz_strike_").is_some());
        let heal = spell_of(&units, 3, 0, "rz_heal_").unwrap();
        assert_eq!(heal.effects, vec![Effect::new(effect::RESTORE_HEALTH, RANGE_SELF, 1, 20, 20)]);
        // Elemental to allies: a heal at P/2, and no strike.
        assert!(spell_of(&units, 4, 0, "rz_strike_").is_none());
        assert_eq!(spell_of(&units, 4, 0, "rz_heal_").unwrap().effects[0].min, 10);
        // Death to allies: a bless (Fortify Attack at P/6 + 1, 30 s), no heal.
        assert!(spell_of(&units, 5, 0, "rz_heal_").is_none());
        let bless = spell_of(&units, 5, 0, "rz_bless_").unwrap();
        assert_eq!(bless.id, "rz_bless_u5_l0");
        assert_eq!(bless.effects, vec![Effect::new(effect::FORTIFY_ATTACK, RANGE_SELF, 30, 4, 4)]);
        // Life to allies: a heal only.
        assert!(spell_of(&units, 6, 0, "rz_strike_").is_none());
        assert!(spell_of(&units, 6, 0, "rz_heal_").is_some());
        // No school, a power of 0, or no direction: nothing.
        assert!(unit_spells(only(&units, 7), 0).is_empty());
        assert!(unit_spells(only(&units, 8), 0).is_empty());
        assert!(unit_spells(only(&units, 9), 0).is_empty());
    }

    #[test]
    fn a_shooter_gets_a_shot_between_its_shot_pair() {
        let units = units_of(vec![unit(7, 0, 10, 0, None, None)], serde_json::json!([]));
        let shot = spell_of(&units, 7, 0, "rz_shot_").unwrap();
        assert_eq!(shot.id, "rz_shot_u7_l0");
        assert_eq!(shot.effects, vec![Effect::new(effect::DAMAGE_HEALTH, RANGE_TARGET, 0, 5, 10)]);
        // A unit without a ranged attack has no shot.
        let units = units_of(vec![unit(4, 25, 0, 0, None, None)], serde_json::json!([]));
        assert!(spell_of(&units, 4, 0, "rz_shot_").is_none());
    }

    #[test]
    fn the_unit_spells_follow_the_level() {
        // Power 20 at level 0 grows by 5 per level above the first: 30 at level 2.
        let mut u = unit(3, 0, 0, 20, Some("elemental"), Some("to_enemy"));
        u["level_up"] = serde_json::json!({"magic_power": 5});
        let units = units_of(vec![u], serde_json::json!([]));
        assert_eq!(spell_of(&units, 3, 2, "rz_strike_").unwrap().effects[0].min, 22, "3 * 30 / 4");
        assert_eq!(spell_of(&units, 3, 2, "rz_strike_").unwrap().id, "rz_strike_u3_l2");
    }

    #[test]
    fn a_version_1_file_loads_with_no_spells() {
        let units = units_of(vec![unit(4, 25, 0, 0, None, None)], serde_json::json!([]));
        assert!(units.spells().is_empty());
        let v1 = serde_json::json!({
            "format": FORMAT, "version": 1, "language": "ru", "count": 0, "units": [],
            "spells": [{ "id": 1, "name": "X", "kind": "none" }]
        })
        .to_string();
        let units = Units::from_json(&v1).unwrap();
        assert!(units.spells().is_empty(), "version 1 carries no spells");
    }

    #[test]
    fn world_spells_map_to_their_effects() {
        let text = serde_json::json!({
            "format": FORMAT, "version": VERSION, "language": "ru", "count": 0, "units": [],
            "spell_count": 4,
            "spells": [
                {"id": 1, "name": "Лечение", "cost_gold": 5, "cost_mana": 10, "school": "life",
                 "target": "hero", "time_work_hours": null, "kind": "heal", "delta_fixed_hits": 15,
                 "add": {}, "percent": {}},
                {"id": 2, "name": "Удар", "cost_gold": 5, "cost_mana": 10, "school": "elemental",
                 "target": "enemy", "time_work_hours": null, "kind": "damage", "delta_fixed_hits": -8,
                 "add": {}, "percent": {}},
                {"id": 3, "name": "Щит", "cost_gold": 5, "cost_mana": 10, "school": "elemental",
                 "target": "hero", "time_work_hours": 9999, "kind": "modifier", "delta_fixed_hits": null,
                 "add": {"defence_blow": 5, "initiative": 3}, "percent": {"protect_elemental": 10}},
                {"id": 4, "name": "Порча", "cost_gold": 5, "cost_mana": 10, "school": "death",
                 "target": "enemy", "time_work_hours": 2, "kind": "modifier", "delta_fixed_hits": null,
                 "add": {"initiative": -4}, "percent": {}}
            ]
        })
        .to_string();
        let units = Units::from_json(&text).unwrap();
        let ts = 120;
        let heal = &units.spells()[0];
        assert_eq!(world_effects(heal, ts), vec![Effect::new(effect::RESTORE_HEALTH, RANGE_SELF, 0, 15, 15)]);
        assert!(world_menu(heal));
        let hit = &units.spells()[1];
        assert_eq!(world_effects(hit, ts), vec![Effect::new(effect::DAMAGE_HEALTH, RANGE_TARGET, 0, 8, 8)]);
        assert!(world_menu(hit), "damage on an enemy is in the menu");
        // The modifier on the hero: Shield 5 and Fortify Attribute (Speed) 3, plus the three resists
        // at 10 each, all for 9999 h, capped at one game day (86400 / 120 = 720 real s).
        let shield = world_effects(&units.spells()[2], ts);
        assert_eq!(shield.len(), 5);
        assert_eq!(shield[0], Effect::new(effect::SHIELD, RANGE_SELF, 720, 5, 5));
        assert_eq!(shield[1], Effect::new(effect::FORTIFY_ATTRIBUTE, RANGE_SELF, 720, 3, 3).on_attribute(ATTRIBUTE_SPEED));
        assert_eq!(shield[2].id, effect::RESIST_FIRE);
        // The enemy modifier: a negative speed is Damage Attribute (Speed) for 2 h = 7200 game s = 60 real s.
        let weak = world_effects(&units.spells()[3], ts);
        assert_eq!(weak, vec![Effect::new(effect::DAMAGE_ATTRIBUTE, RANGE_TARGET, 60, 4, 4).on_attribute(ATTRIBUTE_SPEED)]);
        assert_eq!(world_duration(&units.spells()[3], 120), 60);
        // A damage spell on the hero is an event spell: not in the menu, on self.
        let mut ev = units.spells()[1].clone();
        ev.target = Some("hero".to_string());
        assert!(!world_menu(&ev));
        assert_eq!(world_effects(&ev, ts)[0].range, RANGE_SELF);
    }

    #[test]
    fn the_spell_records_are_sorted_by_id_and_carry_the_mana_cost() {
        let text = serde_json::json!({
            "format": FORMAT, "version": VERSION, "language": "ru", "count": 0, "units": [],
            "spell_count": 2,
            "spells": [
                {"id": 2, "name": "B", "cost_gold": 0, "cost_mana": 12, "school": null,
                 "target": "hero", "time_work_hours": null, "kind": "none", "delta_fixed_hits": null,
                 "add": {}, "percent": {}},
                {"id": 1, "name": "A", "cost_gold": 0, "cost_mana": 3, "school": null,
                 "target": "hero", "time_work_hours": null, "kind": "none", "delta_fixed_hits": null,
                 "add": {}, "percent": {}}
            ]
        })
        .to_string();
        let units = Units::from_json(&text).unwrap();
        let recs = records(Some(&units), &BTreeMap::new(), 120);
        let ids: Vec<&[u8]> = recs.iter().map(|r| r.get(b"NAME").unwrap()).collect();
        assert_eq!(ids, vec![&b"rz_spell_1\0"[..], &b"rz_spell_2\0"[..]]);
        assert_eq!(&recs[1].get(b"SPDT").unwrap()[4..8], &12u32.to_le_bytes()[..]);
        assert!(records(None, &BTreeMap::new(), 120).is_empty(), "no units file, no spells");
    }

    #[test]
    fn combat_numbers_follow_the_unit_mapping() {
        let units = units_of(
            vec![unit(20, 25, 0, 0, None, None), unit(21, 0, 10, 0, None, None), unit(22, 0, 0, 20, Some("elemental"), Some("to_enemy"))],
            serde_json::json!([]),
        );
        let melee = combat_of(only(&units, 20), 0);
        assert_eq!((melee.melee, melee.shot, melee.strike), (Some((12, 25)), None, None));
        assert!((melee.interval - 2.5).abs() < 1e-9, "initiative 10: 4 - 1.5");
        let shooter = combat_of(only(&units, 21), 0);
        assert_eq!((shooter.melee, shooter.shot), (None, Some((5, 10))));
        let caster = combat_of(only(&units, 22), 0);
        assert_eq!(caster.strike, Some(("elemental", 15)));
        assert_eq!(interval(60), 1.0, "at least 1 second");
    }
}
