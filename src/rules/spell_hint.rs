//! The spell badges on the unit cards and their hint (original-mechanics/interface.md §9.4,
//! 493a64, 49ece8, 49d044, 49b63c): which of a unit's spells get a badge, and the hint's
//! texts. Labels and words come through a lookup by the install's ini key (`[Skills]`,
//! `[Time]`), so the interface can give the install's own texts or a translation.

use crate::dt::data::{SpellTarget, Stat};

use super::content::SpellDef;
use super::units::SpellSlot;

/// Badges are this far apart on a card, in the original's pixels.
pub const BADGE_PITCH: f32 = 23.0;
/// A badge's side, in the original's pixels.
pub const BADGE_SIZE: f32 = 22.0;
/// The badge row's top below the portrait's top (card + 0x47, the portrait at card + 1).
pub const BADGE_TOP: f32 = 70.0;
/// The hint box's width (0x1a4).
pub const HINT_WIDTH: f32 = 420.0;
/// From this many minutes left on, the hint says `RemainedTimeOfEffectAll` instead of a
/// duration (the float 4 000 000 centi-minutes at 0x49f884).
pub const UNKNOWN_FROM_MINUTES: u64 = 40_000;

/// The spells of `slots` that show a badge, in slot order: those still running (their end
/// after `now`) that cost mana. A unit has four slots, so at most four badges.
pub fn badge_spells<'a>(slots: &[Option<SpellSlot>], now: u64, spell: impl Fn(u32) -> Option<&'a SpellDef>) -> Vec<(SpellSlot, &'a SpellDef)> {
    slots
        .iter()
        .flatten()
        .filter(|s| s.until > now)
        .filter_map(|s| Some((*s, spell(s.spell)?)))
        .filter(|(_, d)| d.cost_mana > 0)
        .take(4)
        .collect()
}

/// The duration words of 49d044: months, days and hours joined by ", ", zero parts left
/// out; under an hour the `cLessAtHour` word. `word` gives the `[Time]` texts by key.
///
/// Original behaviour kept: the month part writes the count **plus one** (copied from the
/// calendar date at 0x49cbf0), and months wrap at 12 with no year part. The hint never
/// reaches a month (it stops at [`UNKNOWN_FROM_MINUTES`]), but the function is the
/// original's.
pub fn duration_words(minutes: u64, word: &dyn Fn(&'static str) -> String) -> String {
    if minutes < 60 {
        return word("cLessAtHour");
    }
    let mut out = String::new();
    let months = (minutes / 43_200) % 12;
    if months != 0 {
        out.push_str(&format!("{} {}", months + 1, word("cMounth")));
    }
    for (n, key) in [((minutes / 1440) % 30, "cDay"), ((minutes / 60) % 24, "cHour")] {
        if n != 0 {
            if !out.is_empty() {
                out.push_str(", ");
            }
            out.push_str(&format!("{n} {}", word(key)));
        }
    }
    out
}

/// The hint's time line: `RemainedTimeOfEffect` and the duration of `minutes`, or
/// `RemainedTimeOfEffectAll` from [`UNKNOWN_FROM_MINUTES`] on (49ece8).
pub fn time_left_line(minutes: u64, word: &dyn Fn(&'static str) -> String) -> String {
    let left = if minutes < UNKNOWN_FROM_MINUTES { duration_words(minutes, word) } else { word("RemainedTimeOfEffectAll") };
    format!("{} {left}", word("RemainedTimeOfEffect"))
}

/// The hint's `LifeLost` line, "LifeLost: n %", for a unit that has lost `drain` percent of
/// its life when the spell has a `p-LifeLose` value (49ece8).
pub fn life_lost_line(spell: &SpellDef, drain: i32, word: &dyn Fn(&'static str) -> String) -> Option<String> {
    (drain > 0 && spell.life_lose_percent.unwrap_or(0) != 0).then(|| format!("{}: {drain} %", word("LifeLost")))
}

/// The effect text's colour: blue for a spell on the own army (`Target=Hero`, the default),
/// else red (49ece8, 49ba84).
pub fn on_own_army(spell: &SpellDef) -> bool {
    matches!(spell.target, None | Some(SpellTarget::Hero))
}

/// A spell's effect text (49b63c): "label value " entries, "+" before a positive value,
/// "%" after the percentages, zero values left out. `label` gives the `[Skills]` texts.
///
/// Original behaviour kept: `DeltaPercentHits` takes its label from the sign of
/// `DeltaFixedHits` (a spell with only a negative percentage reads as a heal); equal melee
/// and ranged values merge into one `SPhysicalAttack` / `SPhysicalDefence` entry (both 0
/// merge to nothing); the protections merge into `SProtectAllMagic` only when all three are
/// equal and not 0, else they come as Life, Elemental, Death.
pub fn effect_text(spell: &SpellDef, label: &dyn Fn(&'static str) -> String) -> String {
    effect_entries(spell, label).into_iter().map(|e| e.text + " ").collect()
}

/// One entry of a spell's effect text: "label value" (without the original's trailing
/// space) and whether it is good for the unit the spell lands on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectEntry {
    pub text: String,
    pub good: bool,
}

/// Whether an effect entry of `value` is good for the unit holding the spell (Razdor's
/// colouring, the owner's decision 2026-10-18; the original colours the whole effect text by
/// the spell's target instead, 0x49bb36). Every value the effect text can show is one where
/// more is better for its holder: hits (fixed, percentage and maximum), melee and ranged
/// attack and defence, magic power, the magic protections, regeneration (negative is
/// poison), vampirism, initiative and actions. Mana cost and casting time are not effect
/// entries. So an entry is good exactly when its value is positive, whatever its label says
/// (the original's «Лечит (+хиты) -15%» quirk is bad) and whoever cast it (a curse on an
/// enemy is bad for that enemy).
pub fn good_for_holder(value: i32) -> bool {
    value > 0
}

/// The entries of [`effect_text`], in its order.
pub fn effect_entries(spell: &SpellDef, label: &dyn Fn(&'static str) -> String) -> Vec<EffectEntry> {
    let mut out = Vec::new();
    let mut entry = |v: i32, key: &'static str, suffix: &str| {
        if v != 0 {
            let plus = if v > 0 { "+" } else { "" };
            out.push(EffectEntry { text: format!("{} {plus}{v}{suffix}", label(key)), good: good_for_holder(v) });
        }
    };
    let fixed = spell.delta_fixed_hits.unwrap_or(0);
    let heal_key = if fixed < 0 { "CurseHit" } else { "CureHit" };
    entry(fixed, heal_key, "");
    entry(spell.delta_percent_hits.unwrap_or(0), heal_key, "%");
    for (mods, suffix) in [(&spell.add, ""), (&spell.percent, "%")] {
        let v = |s: Stat| mods.get(&s).copied().unwrap_or(0);
        entry(v(Stat::Hits), "SHit", suffix);
        if v(Stat::AttackBlow) == v(Stat::AttackShot) {
            entry(v(Stat::AttackBlow), "SPhysicalAttack", suffix);
        } else {
            entry(v(Stat::AttackBlow), "SAttackBlow", suffix);
            entry(v(Stat::AttackShot), "SAttackShot", suffix);
        }
        if v(Stat::DefenceBlow) == v(Stat::DefenceShot) {
            entry(v(Stat::DefenceBlow), "SPhysicalDefence", suffix);
        } else {
            entry(v(Stat::DefenceBlow), "SDefenceBlow", suffix);
            entry(v(Stat::DefenceShot), "SDefenceShot", suffix);
        }
        entry(v(Stat::MagicPower), "SMagicPower", suffix);
        if suffix.is_empty() {
            // The loader reads no d-Protect*, d-Regen or d-Vampirizm (magic-items.md §2).
            entry(v(Stat::Initiative), "SInitiative", suffix);
            entry(v(Stat::Manevres), "SManevres", suffix);
            continue;
        }
        let (life, death, elemental) = (v(Stat::ProtectLife), v(Stat::ProtectDeath), v(Stat::ProtectElemental));
        if life == death && death == elemental && elemental != 0 {
            entry(elemental, "SProtectAllMagic", suffix);
        } else {
            entry(life, "SProtectLife", suffix);
            entry(elemental, "SProtectElemental", suffix);
            entry(death, "SProtectDeath", suffix);
        }
        let regen = v(Stat::Regen);
        entry(regen, if regen < 0 { "SPoison" } else { "SRegen" }, suffix);
        entry(v(Stat::Vampirizm), "SVampirizm", suffix);
        entry(v(Stat::Initiative), "SInitiative", suffix);
        entry(v(Stat::Manevres), "SManevres", suffix);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::content::testkit;

    fn key(k: &'static str) -> String {
        k.to_string()
    }

    fn words(k: &'static str) -> String {
        match k {
            "cMounth" => "month",
            "cDay" => "day",
            "cHour" => "hour",
            "cLessAtHour" => "less than an hour",
            "RemainedTimeOfEffect" => "Time left:",
            "RemainedTimeOfEffectAll" => "Unknown",
            "LifeLost" => "Drains life",
            other => other,
        }
        .to_string()
    }

    #[test]
    fn durations_are_days_and_hours_with_the_originals_quirks() {
        assert_eq!(duration_words(0, &words), "less than an hour");
        assert_eq!(duration_words(59, &words), "less than an hour");
        assert_eq!(duration_words(60, &words), "1 hour");
        assert_eq!(duration_words(600, &words), "10 hour", "as the original's screen: «10 час»");
        assert_eq!(duration_words(1440, &words), "1 day", "zero hours left out");
        assert_eq!(duration_words(1440 + 3 * 60 + 59, &words), "1 day, 3 hour", "minutes dropped");
        // Never shown by the hint, but the original's arithmetic: a month writes one more,
        // days wrap at 30 and months at 12, no year.
        assert_eq!(duration_words(43_200, &words), "2 month");
        assert_eq!(duration_words(43_200 + 1440 + 60, &words), "2 month, 1 day, 1 hour");
        assert_eq!(duration_words(12 * 43_200 + 60, &words), "1 hour", "a whole year wraps away");
    }

    #[test]
    fn the_time_line_says_unknown_from_40000_minutes_on() {
        assert_eq!(time_left_line(600, &words), "Time left: 10 hour");
        assert_eq!(time_left_line(39_999, &words), "Time left: 27 day, 18 hour");
        assert_eq!(time_left_line(40_000, &words), "Time left: Unknown");
        assert_eq!(time_left_line(156_588, &words), "Time left: Unknown", "Community opcode 11's spells");
    }

    #[test]
    fn badges_go_to_running_spells_that_cost_mana_in_slot_order() {
        let mut free = testkit::spell(3, 10);
        free.cost_mana = 0;
        let spells = [testkit::spell(1, 10), testkit::spell(2, 10), free, testkit::spell(4, 10), testkit::spell(5, 10)];
        let find = |id: u32| spells.iter().find(|s| s.id == id);
        let slot = |spell, until| Some(SpellSlot { spell, until });
        let ids = |v: Vec<(SpellSlot, &SpellDef)>| v.into_iter().map(|(s, _)| s.spell).collect::<Vec<_>>();
        // Slot order, empty slots skipped, a spell without a mana cost skipped.
        assert_eq!(ids(badge_spells(&[None, slot(4, 200), slot(3, 200), slot(1, 200)], 100, find)), [4, 1]);
        // Ended at `now`: no badge; an unknown spell: none either.
        assert_eq!(ids(badge_spells(&[slot(1, 100), slot(2, 101), slot(9, 500), None], 100, find)), [2]);
        // Four slots, four badges at most.
        let all = [slot(1, 200), slot(2, 200), slot(4, 200), slot(5, 200), slot(1, 200)];
        assert_eq!(ids(badge_spells(&all, 0, find)), [1, 2, 4, 5]);
    }

    #[test]
    fn effect_text_is_the_originals_entries() {
        let mut s = testkit::spell(1, 10);
        s.delta_fixed_hits = None;
        s.add.insert(Stat::DefenceBlow, 5);
        s.add.insert(Stat::DefenceShot, 5);
        s.percent.insert(Stat::DefenceBlow, 10);
        s.percent.insert(Stat::DefenceShot, 10);
        // «Укрепление Брони» as the original writes it.
        assert_eq!(effect_text(&s, &key), "SPhysicalDefence +5 SPhysicalDefence +10% ");

        let mut s = testkit::spell(2, 10);
        s.delta_fixed_hits = Some(-20);
        s.delta_percent_hits = Some(-10);
        s.add.insert(Stat::AttackBlow, -3);
        s.add.insert(Stat::Initiative, -2);
        s.percent.insert(Stat::Regen, -15);
        s.percent.insert(Stat::ProtectLife, 20);
        s.percent.insert(Stat::ProtectDeath, 20);
        s.percent.insert(Stat::ProtectElemental, 20);
        assert_eq!(effect_text(&s, &key), "CurseHit -20 CurseHit -10% SAttackBlow -3 SInitiative -2 SProtectAllMagic +20% SPoison -15% ");

        // Only a negative percentage: labelled as a heal (the label follows DeltaFixedHits).
        let mut s = testkit::spell(3, 10);
        s.delta_fixed_hits = None;
        s.delta_percent_hits = Some(-25);
        s.percent.insert(Stat::ProtectLife, 10);
        s.percent.insert(Stat::ProtectDeath, 30);
        assert_eq!(effect_text(&s, &key), "CureHit -25% SProtectLife +10% SProtectDeath +30% ");
        assert!(on_own_army(&s));
        s.target = Some(SpellTarget::Enemy);
        assert!(!on_own_army(&s));
    }

    /// Deviation (the owner's decision, 2026-10-18): each entry is good or bad for the unit
    /// holding the spell by its sign, not by the spell's target and not by its label.
    #[test]
    fn deviation_entries_are_good_by_their_sign_for_the_holder() {
        let goods = |s: &SpellDef| effect_entries(s, &key).into_iter().map(|e| (e.text, e.good)).collect::<Vec<_>>();
        // A spell on the own army with a cost: «жизнь −15% сразу» is bad even on the own side.
        let mut s = testkit::spell(1, 10);
        s.delta_fixed_hits = None;
        s.delta_percent_hits = Some(-15);
        s.percent.insert(Stat::Vampirizm, 35);
        assert!(on_own_army(&s));
        assert_eq!(goods(&s), [("CureHit -15%".to_string(), false), ("SVampirizm +35%".to_string(), true)]);
        // A curse on the enemy: its lowered values are bad for the enemy holding it, poison too.
        let mut s = testkit::spell(2, 10);
        s.target = Some(SpellTarget::Enemy);
        s.delta_fixed_hits = Some(-20);
        s.add.insert(Stat::Initiative, -2);
        s.percent.insert(Stat::Regen, -15);
        s.percent.insert(Stat::ProtectLife, 10);
        assert_eq!(
            goods(&s),
            [("CurseHit -20".to_string(), false), ("SInitiative -2".to_string(), false), ("SProtectLife +10%".to_string(), true), ("SPoison -15%".to_string(), false)]
        );
        assert_eq!(effect_text(&s, &key), "CurseHit -20 SInitiative -2 SProtectLife +10% SPoison -15% ");
    }

    /// The spell card of the report (2026-10-18) from the install, with its own labels: equal
    /// melee and ranged defences read «Физическая защита» once per kind (0x49b63c), as the
    /// original's screen shows it.
    #[test]
    fn real_install_spell_cards() {
        let Some(dir) = std::env::var_os(crate::dt::install::ENV_VAR) else { return };
        let dir = std::path::Path::new(&dir);
        let dt = crate::dt::install::DtInstall::load(dir).expect("install loads");
        let path = crate::dt::install::find_path(dir, crate::dt::install::SETTINGS_FILE).expect("interface texts");
        let ini = crate::dt::ini::Ini::from_cp1251(&std::fs::read(path).expect("read"));
        let skills = ini.section("Skills").expect("[Skills]");
        let label = |k: &'static str| skills.get(k).unwrap_or(k).to_string();
        let spell = |name: &str| dt.spells.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name}"));
        assert_eq!(effect_text(spell("Доспехи Веры"), &label), "Физическая защита +10 Физическая защита +20% ");
        // Every spell's text is built without a label the install lacks.
        for s in &dt.spells {
            let missing = std::cell::Cell::new(None);
            effect_text(s, &|k| {
                if skills.get(k).is_none() {
                    missing.set(Some(k));
                }
                k.to_string()
            });
            assert_eq!(missing.get(), None, "{}", s.name);
        }
    }

    #[test]
    fn the_life_lost_line_needs_a_drain_and_the_spells_life_lose() {
        let mut s = testkit::spell(1, 10);
        assert_eq!(life_lost_line(&s, 30, &words), None);
        s.life_lose_percent = Some(-10);
        assert_eq!(life_lost_line(&s, 0, &words), None);
        assert_eq!(life_lost_line(&s, 30, &words).as_deref(), Some("Drains life: 30 %"));
    }
}
