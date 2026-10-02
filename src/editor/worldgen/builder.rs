//! The random army builder (worldgen.md §6, 0x57fe6c) and the editor's unit strength
//! (0x57f4a8), which it prices units with.

use std::collections::HashMap;

use crate::rules::content::{Bonus, Content, MagicDirection, MagicSchool, Stat, UnitId};
use crate::rules::rng::Rng;
use crate::rules::units::Stats;

use super::super::newmap::ext::Ext;

/// The themes by number (0x5bec3c): 1 Normal … 8 Vampires; the `[AIArmyGeneration]` keys.
pub const THEMES: [&str; 8] = ["Normal", "HolyArmy", "Piesant", "Rogue", "Assasin", "Undead", "Hero", "Vampires"];
pub const NORMAL: usize = 1;
pub const HOLY_ARMY: usize = 2;
pub const PEASANTS: usize = 3;
pub const ROGUE: usize = 4;
pub const ASSASSINS: usize = 5;
pub const UNDEAD: usize = 6;
pub const HERO: usize = 7;
pub const VAMPIRES: usize = 8;

/// The unit roles of the cost function's mode 1 (+0x1b2).
pub const MELEE: u8 = 4;
pub const SHOOTER: u8 = 7;
pub const CASTER: u8 = 17;

/// What the strength formula reads of a unit (its stat block, level 0, no items).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StatBlock {
    pub hits: i32,
    pub attack_blow: i32,
    pub defence_blow: i32,
    pub attack_shot: i32,
    pub defence_shot: i32,
    /// The magic school is Elemental (school byte 2).
    pub elemental: bool,
    pub magic_power: i32,
    /// The direction byte: 1 at enemies, 0 at all, 2 at allies.
    pub direction: u8,
    pub protect_life: i32,
    pub protect_death: i32,
    pub protect_elemental: i32,
    pub regen: i32,
    pub vampirism: i32,
    pub initiative: i32,
    pub manevres: i32,
    /// The vanilla bonus number (1 SpearDefense … 21 FlankStrike), 0 for none or a newer one.
    pub bonus: u8,
}

/// The vanilla number of a bonus; the Community's newer bonuses match none of the formula's
/// tests.
fn bonus_number(b: &Bonus) -> u8 {
    use Bonus as B;
    let order = [
        B::SpearDefense,
        B::HorseAtack,
        B::ArmorIgnore,
        B::ArmyMedic,
        B::Merchant,
        B::DeathCurse,
        B::GodAnger,
        B::GodStrike,
        B::Unvulnerabe,
        B::VampirsGist,
        B::OldVampirsGist,
        B::Evasive,
        B::Ghost,
        B::Artillery,
        B::Garrison,
        B::AddPayment,
        B::Poison,
        B::Dead,
        B::FastDead,
        B::Counterblow,
        B::FlankStrike,
    ];
    order.iter().position(|o| o == b).map_or(0, |p| p as u8 + 1)
}

impl StatBlock {
    pub fn of(s: &Stats) -> StatBlock {
        StatBlock {
            hits: s[Stat::Hits],
            attack_blow: s[Stat::AttackBlow],
            defence_blow: s[Stat::DefenceBlow],
            attack_shot: s[Stat::AttackShot],
            defence_shot: s[Stat::DefenceShot],
            elemental: s.magic == Some(MagicSchool::Elemental),
            magic_power: s[Stat::MagicPower],
            direction: match s.direction {
                Some(MagicDirection::ToEnemy) => 1,
                Some(MagicDirection::ToAlly) => 2,
                _ => 0,
            },
            protect_life: s[Stat::ProtectLife],
            protect_death: s[Stat::ProtectDeath],
            protect_elemental: s[Stat::ProtectElemental],
            regen: s[Stat::Regen],
            vampirism: s[Stat::Vampirizm],
            initiative: s[Stat::Initiative],
            manevres: s[Stat::Manevres],
            bonus: s.bonuses.first().map_or(0, bonus_number),
        }
    }

    /// The role the cost function's mode 1 gives (0x57faf8): melee if the melee attack
    /// beats both others, shooter if the ranged one does, caster (this test last, so it wins)
    /// if the magic power beats both others divided by 2.5; none for tied stats.
    pub fn role(&self) -> u8 {
        let (ab, sh, mp) = (self.attack_blow as i64, self.attack_shot as i64, self.magic_power as i64);
        let mut role = 0;
        if sh < ab && mp < ab {
            role = MELEE;
        }
        if ab < sh && mp < sh {
            role = SHOOTER;
        }
        // x ÷ 2.5 < mp, exactly: 2x < 5·mp.
        if 2 * ab < 5 * mp && 2 * sh < 5 * mp {
            role = CASTER;
        }
        role
    }
}

// The formula's non-integer constants as the editor holds them, at full 80-bit precision
// (0x57fa40–0x57faec); the game holds them rounded to 64 bits (§6.5).
const C1_17: Ext = Ext::from_parts(false, 0x3fff, 0x95c2_8f5c_28f5_c28f);
const C30_3: Ext = Ext::from_parts(false, 0x4003, 0xf266_6666_6666_6666);
const C1_07: Ext = Ext::from_parts(false, 0x3fff, 0x88f5_c28f_5c28_f5c3);
const C1_15: Ext = Ext::from_parts(false, 0x3fff, 0x9333_3333_3333_3333);
const C1_7: Ext = Ext::from_parts(false, 0x3fff, 0xd999_9999_9999_999a);
const C1_4: Ext = Ext::from_parts(false, 0x3fff, 0xb333_3333_3333_3333);
const C0_8: Ext = Ext::from_parts(false, 0x3ffe, 0xcccc_cccc_cccc_cccd);
const C0_2: Ext = Ext::from_parts(false, 0x3ffc, 0xcccc_cccc_cccc_cccd);
const C1_2: Ext = Ext::from_parts(false, 0x3fff, 0x9999_9999_9999_999a);
const C0_15: Ext = Ext::from_parts(false, 0x3ffc, 0x9999_9999_9999_999a);
const C3_2: Ext = Ext::from_parts(false, 0x4000, 0xcccc_cccc_cccc_cccd);
const C1_1: Ext = Ext::from_parts(false, 0x3fff, 0x8ccc_cccc_cccc_cccd);

fn int(v: i64) -> Ext {
    Ext::int(v)
}

/// `a ÷ b`; the divisors are the formula's constants and positive counts.
fn div(a: Ext, b: Ext) -> Ext {
    a.div(b).unwrap_or(Ext::ZERO)
}

/// The editor's strength of a unit (0x57f4a8): the game's formula (experience.md §1)
/// operation by operation in 80-bit precision, with the vanilla bonus tests and `Exp` as
/// Delphi's; ÷ 3 from `ShotWeaponRange` on (not Artillery), 0 made 1.
pub fn strength(b: &StatBlock, bd: i32, shot_range: i32) -> i32 {
    strength_with(b, bd, shot_range, Ext::exp)
}

/// [`strength`] with another `Exp` (the tests compare it with the processor's own).
pub fn strength_with(b: &StatBlock, bd: i32, shot_range: i32, exp: impl Fn(Ext) -> Ext) -> i32 {
    if b.hits == 0 {
        return 0;
    }
    let h = int(b.hits as i64);
    let (ab, sh, mp) = (b.attack_blow, b.attack_shot, b.magic_power);
    let bonus = b.bonus;
    let half = |v: i32| div(int(v as i64), int(2));
    // A shooter or a caster (the role byte not 4) loses 5 of melee defence and gains 5 of
    // ranged; the caster test divides by 2.
    let mut role = 4;
    if ab > sh && ab > mp {
        role = 4;
    }
    if sh > ab && sh > mp {
        role = 7;
    }
    if half(ab) < int(mp as i64) && half(sh) < int(mp as i64) {
        role = 17;
    }
    let c21_5 = div(int(43), int(2));
    let (x, y_add) = if role != 4 {
        let x = b.defence_blow + bd;
        (if x > 5 { int((x - 5) as i64) } else { Ext::ZERO }, b.defence_shot + bd + 5)
    } else {
        (int((b.defence_blow + bd) as i64), b.defence_shot + bd)
    };
    let e1 = div(exp(div(x, c21_5)), C1_17);
    let y = div(int(b.regen as i64).mul(h), int(100)).add(int(y_add as i64));
    let e2 = div(exp(div(y, C30_3)), C1_07);
    let mut d = int(e2.add(e1).mul(h).round_int());
    d = match bonus {
        1 => d.mul(C1_15),
        9 | 13 => h.mul(int(10)),
        10..=12 => d.mul(div(int(3), int(2))),
        15 => d.mul(int(2)),
        _ => d,
    };
    let mut t = h.add(d);
    if matches!(bonus, 18 | 19) {
        t = t.mul(C1_7);
    }
    let prot = int((b.protect_life + b.protect_death) as i64).add(int(b.protect_elemental as i64).mul(div(int(3), int(2))));
    t = div(prot, int(560)).mul(t).add(t);
    let mut a = Ext::ZERO;
    if ab >= sh && ab >= mp {
        a = int(ab as i64);
    }
    if sh >= ab && sh >= mp {
        a = int(sh as i64).mul(C1_4);
    }
    if matches!(bonus, 3 | 10 | 11 | 14) {
        a = a.add(int(18));
    }
    let man = b.manevres;
    if man > 0 && mp > ab && mp > sh {
        a = int(mp as i64);
        if b.direction == 1 {
            let p = div(C0_8.mul(a).mul(int((man - 1) as i64)), int(man as i64));
            a = p.add(a.mul(C0_2));
            if b.elemental {
                a = a.add(int(25));
            }
        }
        if b.direction == 0 {
            a = a.mul(C1_2);
        }
    }
    if bonus == 15 {
        a = a.mul(int(2));
    }
    if bonus == 7 {
        a = a.add(int(10));
    }
    if bonus == 8 {
        a = a.add(int(20));
    }
    if bonus == 20 {
        a = int(ab as i64).add(a);
    }
    if bonus == 21 {
        a = int((ab / 3) as i64).add(a);
    }
    let fast = if matches!(bonus, 2 | 11 | 19) { a.mul(C0_15) } else { Ext::ZERO };
    a = int(man as i64).mul(a).add(fast);
    a = if b.initiative > 0 { div(int(b.initiative as i64).mul(a), int(100)).add(a) } else { Ext::ZERO };
    let v = div(int(b.vampirism as i64), int(100)).mul(t);
    t = v.mul(div(a, h)).add(t);
    let mut st = div(C3_2.mul(t).mul(a.add(int(1))), int(200));
    if matches!(bonus, 6 | 13) {
        st = st.add(int(150));
    }
    if bonus == 17 {
        st = st.mul(C1_1);
    }
    let mut s = st.round_int() as i32;
    if sh >= shot_range && bonus != 14 {
        s /= 3;
    }
    if s == 0 {
        1
    } else {
        s
    }
}

/// What the builder knows of a unit type.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UnitFacts {
    /// Its price (`Cost`), for the wages.
    pub price: i32,
    /// Strength × `CostMultipler` div 100 at level 0 (0x57faf8).
    pub value: i32,
    pub role: u8,
}

/// The units, themes and wage divisor the builder reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Units {
    facts: HashMap<u32, UnitFacts>,
    /// Theme `t`'s unit ids at index `t` (1–8; 0 unused).
    themes: [Vec<u32>; 9],
    /// `CostRecrutDiv` of `_Global.ini`.
    pub recruit_div: i32,
}

impl Units {
    /// From units and themes given directly (`themes[t − 1]` for theme t).
    pub fn new(facts: HashMap<u32, UnitFacts>, themes: [Vec<u32>; 8], recruit_div: i32) -> Units {
        let mut t: [Vec<u32>; 9] = Default::default();
        for (k, list) in themes.into_iter().enumerate() {
            t[k + 1] = list;
        }
        Units { facts, themes: t, recruit_div }
    }

    /// The editor's tables from the game data (0x582ac0 for the themes): each theme's
    /// `[AIArmyGeneration]` list up to its first 0.
    pub fn from_content(c: &Content) -> Units {
        let range = c.options.shot_weapon_range;
        let facts = c
            .units
            .iter()
            .map(|u| {
                let st = StatBlock::of(&Stats::of_level(c, UnitId(u.id), 1));
                let value = (strength(&st, 0, range) as i64 * u.cost_multiplier as i64 / 100) as i32;
                (u.id, UnitFacts { price: u.cost, value, role: st.role() })
            })
            .collect();
        let themes = THEMES.map(|name| {
            let list = c.options.army_generation.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_slice()).unwrap_or(&[]);
            list.iter().copied().take_while(|&id| id != 0).collect()
        });
        Units::new(facts, themes, c.options.cost_recrut_div)
    }

    pub fn theme(&self, t: usize) -> &[u32] {
        self.themes.get(t).map_or(&[], |v| v.as_slice())
    }

    /// The facts of unit `id`; an id the tables do not have (0, an empty theme's pick) is an
    /// empty record: strength 0, price 0.
    pub fn facts(&self, id: u32) -> UnitFacts {
        self.facts.get(&id).copied().unwrap_or_default()
    }

    /// The daily wage of a hired unit of price `price` (0x581504): `Round(price ÷
    /// CostRecrutDiv × f)`, f = ¼ up to 50, ½ up to 100, ¾ up to 150, else 1 (no factor);
    /// `None` for a divisor of 0 (a division by zero in the original).
    pub fn wage(&self, price: i32) -> Option<i32> {
        let q = Ext::int(price as i64).div(Ext::int(self.recruit_div as i64))?;
        let quarter = |k: i64| Ext::int(k).div(Ext::int(4)).expect("4");
        let v = match price {
            ..=50 => q.mul(quarter(1)),
            51..=100 => q.mul(quarter(2)),
            101..=150 => q.mul(quarter(3)),
            _ => q,
        };
        Some(v.round_int() as i32)
    }
}

/// Why a pick never ends: its slot and window when the window stopped changing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hang {
    pub slot: u8,
    pub lo: i32,
    pub hi: i32,
}

/// Picks far beyond any the original makes before the window covers a unit.
const PICK_LIMIT: u64 = 20_000_000;

/// One unit for `slot` of theme `theme` with the cost window `[lo, hi]` (0x57fc48). Slot 0
/// and the even slots double the window; a draw in the window of a unit with a role keeps it
/// on a further draw by the slot's leaning (§6.3); after 26 misses in a row the window
/// widens, `lo × 80 div 100` and `hi × 120 div 100`. A window that cannot widen any more
/// (its high end 4 or less) and holds no unit of the theme is the original's endless loop
/// (quirk 18): `Err` then.
pub fn pick(units: &Units, rng: &mut Rng, slot: u8, theme: usize, mut lo: i32, mut hi: i32) -> Result<u32, Hang> {
    let m = (slot as i32 + 1) % 4;
    if m == 1 || m == 3 {
        lo *= 2;
        hi *= 2;
    }
    let list = units.theme(theme);
    // An empty theme reads the zero that ends its list: unit 0, the empty record.
    let draw_from: &[u32] = if list.is_empty() { &[0] } else { list };
    let mut tries = 0;
    for _ in 0..PICK_LIMIT {
        let r = rng.random(list.len() as i32) as usize;
        let id = draw_from[r];
        let f = units.facts(id);
        let mut ok = (lo..=hi).contains(&f.value);
        if ok && slot > 0 && f.role != 0 {
            ok = match (m, f.role) {
                (0 | 2, MELEE) => rng.random(4) > 0,
                (0 | 2, _) => rng.random(4) == 0,
                (1, SHOOTER) => rng.random(8) > 0,
                (1, _) => rng.random(8) == 0,
                (_, CASTER) => rng.random(4) > 0,
                (_, _) => rng.random(4) == 0,
            };
        }
        tries += 1;
        if !ok && tries > 25 {
            tries = 0;
            let (nlo, nhi) = (lo * 80 / 100, hi * 120 / 100);
            if (nlo, nhi) == (lo, hi) && !draw_from.iter().any(|&id| (lo..=hi).contains(&units.facts(id).value)) {
                return Err(Hang { slot, lo, hi });
            }
            (lo, hi) = (nlo, nhi);
        }
        if ok {
            return Ok(id);
        }
    }
    Err(Hang { slot, lo, hi })
}

/// A random army of theme `theme` for budget `budget` (0x57fe6c): the leader at 40–50 % of
/// the budget (window `[B/5, B/4]`, doubled), from Hero for a Normal army (quirk 16); the
/// rest shared out over `|R(8) − R(3)| + 2` units (one unit of 50 when nothing is left),
/// each picked with the window `[share/2, share × 3/2]` (low end 0 under 40), share = rest
/// × 3 div (4n). Returns the leader first.
pub fn build(units: &Units, rng: &mut Rng, theme: usize, budget: i32) -> Result<Vec<u32>, Hang> {
    let leader_theme = if theme == NORMAL { HERO } else { theme };
    let leader = pick(units, rng, 0, leader_theme, budget / 5, budget / 4)?;
    let mut rest = budget - units.facts(leader).value;
    let n = if rest < 1 {
        rest = 50;
        1
    } else {
        let a = rng.random(8);
        (a - rng.random(3)).abs() + 2
    };
    let share = rest * 3 / (n << 2);
    let hi = share * 3 / 2;
    let lo = if share / 2 < 40 { 0 } else { share / 2 };
    let mut out = vec![leader];
    for slot in 1..=n {
        out.push(pick(units, rng, slot as u8, theme, lo, hi)?);
    }
    Ok(out)
}
