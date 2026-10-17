//! An item's text as the original writes it (original-mechanics/interface.md §9.2, 0x499eb0
//! with its entry writer 0x499d30): the usage line, the effect text and the bonus line, as
//! pure functions. Labels come through a lookup by the install's `[Skills]` key (the label
//! table 0xae6c80 filled by 0x4e36cc), so the interface can give the install's own texts or a
//! translation. The same builder fills the market's description (0x4b9c40), the army and hero
//! windows' item description (0x4c280c) and the event window's item tooltip (0x4cbab0).

use crate::dt::data::{ArtefactType, MagicSchool, Stat};

use super::content::ArtefactDef;

/// The `[Skills]` key of the usage line, or none (0x499eb0): only for an item with a
/// description. Each test replaces the one before: by type, `OnlyForWarriors` for a melee
/// weapon (type 0) or a shield (4), `OnlyForBowmans` for a ranged weapon (1), `OnlyForMages`
/// for a staff (5); then `OnlyForMages` for any item with a school (+0x3b ≠ 0), and
/// `NotForUse` for a Death school item (+0x3b = 3).
pub fn usage_key(d: &ArtefactDef) -> Option<&'static str> {
    if d.description.is_empty() {
        return None;
    }
    let mut key = match d.kind {
        ArtefactType::BlowWeapon | ArtefactType::Shield => Some("OnlyForWarriors"),
        ArtefactType::ShotWeapon => Some("OnlyForBowmans"),
        ArtefactType::Staff => Some("OnlyForMages"),
        _ => None,
    };
    if d.magic.is_some() {
        key = Some("OnlyForMages");
    }
    if d.magic == Some(MagicSchool::Death) {
        key = Some("NotForUse");
    }
    key
}

/// The effect text (0x499eb0 → 0x499d30): one paragraph of entries, each "label value " (the
/// label, a space, the prefix, the number, the suffix and a space), in this order:
///
/// - **f-** (fixed) values with the prefix "=": `SHit`, `SAttackBlow`, `SDefenceBlow`,
///   `SAttackShot`, `SDefenceShot`, `SMagicPower`, the protections with "%" (one
///   `SProtectAllMagic` entry when the three are equal and not 0, else `SProtectLife`,
///   `SProtectElemental`, `SProtectDeath`), regeneration with "%" (`SPoison` when negative,
///   else `SRegen`), `SVampirizm` with "%", `SInitiative`, `SManevres`. A potion's f-Hits is a
///   heal instead: under 1000 `SCurrentHit` with "+" (no "="); from 1000 on `Resurrect` alone,
///   with no number.
/// - **d-** (added) values: `SHit`, `SAttackBlow`, `SAttackShot`, then the defences: equal
///   d-defences make one `SPhysicalDefence` entry, followed by "+p%" when the p-defences are
///   equal too and both values are not 0; else `SDefenceBlow` and `SDefenceShot`, each followed
///   by its own "+p%" when its d- and p-value are not 0. Then `SMagicPower`, `SInitiative`,
///   `SManevres`.
/// - **p-** (percent) values with "%": `SHit`, `SAttackBlow`, `SAttackShot`, the defences (one
///   `SPhysicalDefence` entry when the p-defences are equal and both d-defences are 0; when they
///   differ, each one whose d-value is 0), `SMagicPower`, the protections (merged as above),
///   regeneration, `SVampirizm`, `SInitiative`, `SManevres`.
///
/// A value of 0 writes nothing; a positive value without a prefix gets "+". The f- prefix "="
/// stands before the sign, so a negative fixed value reads "=-5".
///
/// Original behaviour kept: the defence merging loses p-defences (equal d-defences that are not
/// 0 with unequal p-defences write neither; unequal d-defences, one of them 0, with equal
/// p-defences lose the p-value of the one whose d-value is 0), and attacks never merge.
pub fn effect_text(d: &ArtefactDef, label: &dyn Fn(&'static str) -> String) -> String {
    let raises = d.kind == ArtefactType::Potion && d.fixed.get(&Stat::Hits).is_some_and(|&h| h >= 1000);
    // A potion's f-Hits from 1000 on: label 0x1a, the label and a space, no number (0x499d95).
    let mut out = if raises { format!("{} ", label("Resurrect")) } else { String::new() };
    // 0x499d30(value, label, prefix, suffix): nothing for 0; "+" for a positive value without
    // a prefix; the label is left out for the "+p%" tails (label index −1).
    let mut entry = |v: i32, key: Option<&'static str>, prefix: &str, suffix: &str| {
        if v == 0 {
            return;
        }
        let prefix = if v > 0 && prefix.is_empty() { "+" } else { prefix };
        if let Some(key) = key {
            out.push_str(&label(key));
            out.push(' ');
        }
        out.push_str(&format!("{prefix}{v}{suffix} "));
    };
    let f = |s: Stat| d.fixed.get(&s).copied().unwrap_or(0);
    let a = |s: Stat| d.add.get(&s).copied().unwrap_or(0);
    let p = |s: Stat| d.percent.get(&s).copied().unwrap_or(0);

    // f-: the item record's +0x27..+0x59.
    let hits = f(Stat::Hits);
    if d.kind == ArtefactType::Potion {
        if !raises {
            entry(hits, Some("SCurrentHit"), "", "");
        }
    } else {
        entry(hits, Some("SHit"), "=", "");
    }
    entry(f(Stat::AttackBlow), Some("SAttackBlow"), "=", "");
    entry(f(Stat::DefenceBlow), Some("SDefenceBlow"), "=", "");
    entry(f(Stat::AttackShot), Some("SAttackShot"), "=", "");
    entry(f(Stat::DefenceShot), Some("SDefenceShot"), "=", "");
    entry(f(Stat::MagicPower), Some("SMagicPower"), "=", "");
    protections(&mut entry, f, "=");
    let regen = f(Stat::Regen);
    entry(regen, Some(if regen < 0 { "SPoison" } else { "SRegen" }), "=", "%");
    entry(f(Stat::Vampirizm), Some("SVampirizm"), "=", "%");
    entry(f(Stat::Initiative), Some("SInitiative"), "=", "");
    entry(f(Stat::Manevres), Some("SManevres"), "=", "");

    // d-: +0x67..+0x99 (the loader reads no d- protections, regeneration or vampirism).
    let (d_blow, d_shot, p_blow, p_shot) = (a(Stat::DefenceBlow), a(Stat::DefenceShot), p(Stat::DefenceBlow), p(Stat::DefenceShot));
    entry(a(Stat::Hits), Some("SHit"), "", "");
    entry(a(Stat::AttackBlow), Some("SAttackBlow"), "", "");
    entry(a(Stat::AttackShot), Some("SAttackShot"), "", "");
    if d_blow == d_shot {
        entry(d_blow, Some("SPhysicalDefence"), "", "");
        if p_blow == p_shot && p_blow != 0 && d_blow != 0 {
            entry(p_blow, None, "", "%");
        }
    } else {
        entry(d_blow, Some("SDefenceBlow"), "", "");
        if p_blow != 0 && d_blow != 0 {
            entry(p_blow, None, "", "%");
        }
        entry(d_shot, Some("SDefenceShot"), "", "");
        if p_shot != 0 && d_shot != 0 {
            entry(p_shot, None, "", "%");
        }
    }
    entry(a(Stat::MagicPower), Some("SMagicPower"), "", "");
    entry(a(Stat::Initiative), Some("SInitiative"), "", "");
    entry(a(Stat::Manevres), Some("SManevres"), "", "");

    // p-: +0xa7..+0xd9.
    entry(p(Stat::Hits), Some("SHit"), "", "%");
    entry(p(Stat::AttackBlow), Some("SAttackBlow"), "", "%");
    entry(p(Stat::AttackShot), Some("SAttackShot"), "", "%");
    if p_blow == p_shot {
        if d_blow == d_shot && d_blow == 0 {
            entry(p_blow, Some("SPhysicalDefence"), "", "%");
        }
    } else {
        if d_blow == 0 {
            entry(p_blow, Some("SDefenceBlow"), "", "%");
        }
        if d_shot == 0 {
            entry(p_shot, Some("SDefenceShot"), "", "%");
        }
    }
    entry(p(Stat::MagicPower), Some("SMagicPower"), "", "%");
    protections(&mut entry, p, "");
    let regen = p(Stat::Regen);
    entry(regen, Some(if regen < 0 { "SPoison" } else { "SRegen" }), "", "%");
    entry(p(Stat::Vampirizm), Some("SVampirizm"), "", "%");
    entry(p(Stat::Initiative), Some("SInitiative"), "", "%");
    entry(p(Stat::Manevres), Some("SManevres"), "", "%");
    out
}

/// The entry writer of [`effect_text`]: value, label key (none for a "+p%" tail), prefix, suffix.
type Entry<'a> = dyn FnMut(i32, Option<&'static str>, &str, &str) + 'a;

/// The protections' entries with "%": one `SProtectAllMagic` entry (the elemental value) when
/// life, death and elemental are equal and not 0, else life, elemental, death.
fn protections(entry: &mut Entry, v: impl Fn(Stat) -> i32, prefix: &str) {
    let (life, death, elemental) = (v(Stat::ProtectLife), v(Stat::ProtectDeath), v(Stat::ProtectElemental));
    if life == death && death == elemental && elemental != 0 {
        entry(elemental, Some("SProtectAllMagic"), prefix, "%");
    } else {
        entry(life, Some("SProtectLife"), prefix, "%");
        entry(elemental, Some("SProtectElemental"), prefix, "%");
        entry(death, Some("SProtectDeath"), prefix, "%");
    }
}

/// The bonus line (0x49a5b9): `GetBonus`, a space and the bonus's name in double quotes. The
/// name is its `[Army] Bonus<N>` text cut before the first " - " and trimmed of spaces
/// ([`bonus_title`]).
pub fn bonus_line(name: &str, label: &dyn Fn(&'static str) -> String) -> String {
    format!("{} \"{name}\"", label("GetBonus"))
}

/// The name part of a bonus text (0x49a5c6): up to the first " - ", spaces trimmed. A text
/// without " - " is kept whole (the original cuts it to nothing and then reads before the
/// string's start; no shipped bonus text lacks the dash).
pub fn bonus_title(text: &str) -> &str {
    text.split(" - ").next().unwrap_or(text).trim_matches(' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit::item;
    use crate::rules::content::StatMods;

    fn key(k: &'static str) -> String {
        k.to_string()
    }

    /// The install's Russian `[Skills]` texts the report's screenshot shows.
    fn ru(k: &'static str) -> String {
        match k {
            "SVampirizm" => "Вампиризм",
            "SInitiative" => "Инициатива",
            "SManevres" => "Количество действий",
            "GetBonus" => "Дает бонус",
            other => other,
        }
        .to_string()
    }

    #[test]
    fn the_blood_for_a_vampire_reads_as_the_originals() {
        // [97 Кровь для Вампира]: f-Vampirizm=50, d-Initiative=10, p-Manevres=50, a potion.
        let mut blood = item(97, ArtefactType::Potion);
        blood.fixed = StatMods::from([(Stat::Vampirizm, 50)]);
        blood.add = StatMods::from([(Stat::Initiative, 10)]);
        blood.percent = StatMods::from([(Stat::Manevres, 50)]);
        // The original's screen: «Вампиризм =50% Инициатива +10 Количество действий +50%».
        assert_eq!(effect_text(&blood, &ru), "Вампиризм =50% Инициатива +10 Количество действий +50% ");
    }

    #[test]
    fn fixed_values_take_equals_before_the_sign_and_potions_heal_or_raise() {
        let mut ring = item(1, ArtefactType::Ring);
        ring.fixed = StatMods::from([(Stat::Hits, 40), (Stat::Initiative, -2), (Stat::Regen, -5), (Stat::ProtectLife, 30), (Stat::ProtectDeath, 30), (Stat::ProtectElemental, 30)]);
        assert_eq!(effect_text(&ring, &key), "SHit =40 SProtectAllMagic =30% SPoison =-5% SInitiative =-2 ");
        let mut potion = item(2, ArtefactType::Potion);
        potion.fixed = StatMods::from([(Stat::Hits, 75)]);
        potion.percent = StatMods::from([(Stat::Hits, 10)]);
        assert_eq!(effect_text(&potion, &key), "SCurrentHit +75 SHit +10% ");
        potion.fixed = StatMods::from([(Stat::Hits, 1000)]);
        potion.percent = StatMods::new();
        assert_eq!(effect_text(&potion, &key), "Resurrect ");
    }

    #[test]
    fn protections_merge_only_when_all_three_are_equal() {
        let mut amulet = item(1, ArtefactType::Amulet);
        amulet.percent = StatMods::from([(Stat::ProtectLife, 10), (Stat::ProtectDeath, 20), (Stat::ProtectElemental, 30)]);
        assert_eq!(effect_text(&amulet, &key), "SProtectLife +10% SProtectElemental +30% SProtectDeath +20% ");
    }

    #[test]
    fn the_defences_merge_with_the_originals_lost_values() {
        let mut shield = item(1, ArtefactType::Shield);
        let set = |s: &mut ArtefactDef, d: (i32, i32), p: (i32, i32)| {
            s.add = StatMods::from([(Stat::DefenceBlow, d.0), (Stat::DefenceShot, d.1)]);
            s.percent = StatMods::from([(Stat::DefenceBlow, p.0), (Stat::DefenceShot, p.1)]);
        };
        set(&mut shield, (3, 3), (10, 10));
        assert_eq!(effect_text(&shield, &key), "SPhysicalDefence +3 +10% ");
        set(&mut shield, (3, 5), (10, 0));
        assert_eq!(effect_text(&shield, &key), "SDefenceBlow +3 +10% SDefenceShot +5 ");
        set(&mut shield, (0, 0), (10, 10));
        assert_eq!(effect_text(&shield, &key), "SPhysicalDefence +10% ");
        set(&mut shield, (0, 0), (10, 5));
        assert_eq!(effect_text(&shield, &key), "SDefenceBlow +10% SDefenceShot +5% ");
        // Original behaviour: equal d-defences with unequal p-defences write no percentage.
        set(&mut shield, (3, 3), (10, 5));
        assert_eq!(effect_text(&shield, &key), "SPhysicalDefence +3 ");
        // Original behaviour: unequal d-defences, one 0, equal p-defences: that one's is lost.
        set(&mut shield, (0, 4), (10, 10));
        assert_eq!(effect_text(&shield, &key), "SDefenceShot +4 +10% ");
        // Attacks never merge.
        shield.add = StatMods::from([(Stat::AttackBlow, 2), (Stat::AttackShot, 2)]);
        shield.percent = StatMods::new();
        assert_eq!(effect_text(&shield, &key), "SAttackBlow +2 SAttackShot +2 ");
    }

    #[test]
    fn the_usage_line_needs_a_description_and_the_school_overrides_the_type() {
        let with = |kind, magic| {
            let mut d = item(1, kind);
            d.description = "text".into();
            d.magic = magic;
            usage_key(&d)
        };
        assert_eq!(with(ArtefactType::BlowWeapon, None), Some("OnlyForWarriors"));
        assert_eq!(with(ArtefactType::Shield, None), Some("OnlyForWarriors"));
        assert_eq!(with(ArtefactType::ShotWeapon, None), Some("OnlyForBowmans"));
        assert_eq!(with(ArtefactType::Staff, None), Some("OnlyForMages"));
        assert_eq!(with(ArtefactType::Ring, None), None);
        assert_eq!(with(ArtefactType::Potion, None), None);
        assert_eq!(with(ArtefactType::BlowWeapon, Some(MagicSchool::Life)), Some("OnlyForMages"));
        assert_eq!(with(ArtefactType::Ring, Some(MagicSchool::Death)), Some("NotForUse"));
        assert_eq!(usage_key(&item(1, ArtefactType::BlowWeapon)), None, "no description, no usage line");
    }

    #[test]
    fn the_bonus_line_quotes_the_name_before_the_dash() {
        assert_eq!(bonus_title("  Длинное оружие  - Воин бьёт первым"), "Длинное оружие");
        assert_eq!(bonus_line(bonus_title("Вампиризм - пьёт кровь"), &ru), "Дает бонус \"Вампиризм\"");
    }

    /// The report's potion from the install, with the install's own labels.
    #[test]
    fn real_install_blood_for_a_vampire() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dir = std::path::Path::new(&dir);
        let dt = crate::dt::install::DtInstall::load(dir).expect("install loads");
        let path = crate::dt::install::find_path(dir, crate::dt::install::SETTINGS_FILE).expect("interface texts");
        let ini = crate::dt::ini::Ini::from_cp1251(&std::fs::read(path).expect("read"));
        let skills = ini.section("Skills").expect("[Skills]");
        let label = |k: &'static str| skills.get(k).unwrap_or(k).to_string();
        let blood = dt.artefacts.iter().find(|a| a.name == "Кровь для Вампира").expect("the potion");
        assert_eq!(effect_text(blood, &label), "Вампиризм =50% Инициатива +10 Количество действий +50% ");
        assert_eq!(usage_key(blood), None);
        // Every item's text is built without a label the install lacks.
        for a in &dt.artefacts {
            let missing = std::cell::Cell::new(None);
            let check = |k: &'static str| {
                if skills.get(k).is_none() {
                    missing.set(Some(k));
                }
                k.to_string()
            };
            effect_text(a, &check);
            if let Some(k) = usage_key(a) {
                check(k);
            }
            assert_eq!(missing.get(), None, "{}", a.name);
        }
    }
}
