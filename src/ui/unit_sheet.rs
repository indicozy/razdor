//! The original's unit panel (battle and army screens): the unit's full-body sepia figure on
//! parchment, worn items in the top corners, its name, the long stat list and its traits
//! with their icons (video notes §3, refs 09 and 11).

use razdor::local;
use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::{bless_effect, curse_effect, Buff};
use razdor::rules::content::{Bonus, Content, HeroClass, ItemId, MagicDirection, MagicSchool, Stat, UnitId};
use razdor::rules::formation::{Formation, Slot};
use razdor::rules::items::{slot_count, MAX_SLOTS};
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::chrome::{self, k, BLUE_TEXT, CREAM, ORANGE_TEXT, RED_TEXT};
use super::window_layout;
use super::widgets::{measure, measure_exact, mouse_in};

local! {
    static TRAIT_SCROLL: std::cell::RefCell<std::collections::HashMap<String, f32>> = std::cell::RefCell::new(std::collections::HashMap::new());
}

/// The army's font tints from the original's startup palette (4dbb0c), applied only
/// in the native army presentation. Other screens retain their current palette.
fn army_ink(color: Color) -> Color {
    if !super::dt_font::native_metrics() { return color; }
    if color == NAMED_INK { super::typography::Ink::Yellow.color() }
    else { super::typography::color(color) }
}

// The panel's text lists all have the shadow font in a game (0x492f24: ae24d8, the bold
// wage line ae2510; the name ae24b0): drawn 1 px down and right first.
fn shadow_text(s: &str, x: f32, y: f32, size: f32, color: Color) {
    chrome::shadow_text(s, x, y, size, army_ink(color));
}

fn shadow_right(s: &str, x: f32, y: f32, size: f32, color: Color) {
    chrome::shadow_right(s, x, y, size, army_ink(color));
}

fn shadow_centered(s: &str, x: f32, y: f32, size: f32, color: Color) {
    chrome::shadow_centered(s, x, y, size, army_ink(color));
}

/// What the panel shows of one unit.
pub struct Sheet<'a> {
    pub kind: UnitId,
    pub name: &'a str,
    /// A named character's name is written in its own font (0x492f24: ae24a0).
    pub named: bool,
    pub level: i32,
    pub xp: i32,
    pub need: i32,
    pub hp: i32,
    /// Stats now (with blessings and curses).
    pub now: &'a Stats,
    /// The current value each stat is compared with: the stats with items as the battle
    /// began (outside battle, the same as `now`). A building's defence is in it too.
    pub start: &'a Stats,
    /// The unit's bleeding in battle (% lost at each action start; 0 outside battle).
    pub bleed: i32,
    /// Current magic power (it drains during a battle).
    pub power: i32,
    pub wage: i32,
    pub items: [Option<ItemId>; MAX_SLOTS],
    pub back_row: bool,
    /// Unpaid, in the player's army outside battle (not a type's preview): the no-payment
    /// line (492f24: paid flag +0x1a5 clear, not a template, not in battle).
    pub unpaid: bool,
    /// The defence the building adds to both defences (0 outside one); `now` includes it.
    pub building: i32,
    pub hero: Option<HeroClass>,
    /// Status lines under the name (actions left, poison, this turn's modifiers).
    pub status: Vec<(String, Color)>,
}

/// One-based number of a bonus in the original's list (`Bonus<N>.lit`, `[Army] Bonus<N>`).
fn bonus_number(b: &Bonus) -> Option<usize> {
    Bonus::known().position(|x| x == b).map(|i| i + 1)
}

/// English stand-ins for the trait texts, used without an install.
fn bonus_english(b: &Bonus) -> String {
    let s = match b {
        Bonus::SpearDefense => tr("Long weapon: triple melee defence on the first turn of a battle"),
        Bonus::HorseAtack => tr("Fast attack: +1 action on the first turn of a battle"),
        Bonus::ArmorIgnore => tr("Piercing blow: ignores the enemy's defence (not a building's)"),
        Bonus::ArmyMedic => tr("Healer: the army heals 15% of its wounds every day"),
        Bonus::Merchant => tr("Expert trader: +50% when selling, -30% when buying"),
        Bonus::DeathCurse => tr("Death's curse: whoever kills this unit dies too"),
        Bonus::GodAnger => tr("Wrath of God: +10 damage past any defence"),
        Bonus::GodStrike => tr("Anger of God: +20 damage past any defence"),
        Bonus::Unvulnerabe => tr("Invulnerable: loses only 1 hit per blow"),
        Bonus::VampirsGist => tr("Dark gift: ignores armour and absorbs 30% of a blow"),
        Bonus::OldVampirsGist => tr("Dark art: ignores armour, absorbs 30%, +1 action on the first turn"),
        Bonus::Evasive => tr("Evasive: only 70% of physical damage gets through"),
        Bonus::Ghost => tr("Ghost: immune to weapons; its killer dies"),
        Bonus::Artillery => tr("Barrage: always acts first and ignores defence"),
        Bonus::Garrison => tr("Garrison: double strength inside a castle or fort"),
        Bonus::AddPayment => tr("Quartermaster: the army's wages are 30% lower"),
        Bonus::Poison => tr("Poisoned weapon: the target loses 15% of its life each turn"),
        Bonus::Dead => tr("Undead: arrows do 70% less damage"),
        Bonus::FastDead => tr("Fast undead: arrows do 70% less, +1 action on the first turn"),
        Bonus::Counterblow => tr("Counterblow: strikes back when struck"),
        Bonus::FlankStrike => tr("Flank strike: double attack through an empty cell"),
        other => return razdor::rules::items::bonus_name(other),
    };
    s.to_string()
}

pub(super) fn trait_line(b: &Bonus) -> (String, String) {
    match bonus_number(b) {
        Some(n) => (format!("Bonus{n}"), chrome::ui_text("Army", &format!("Bonus{n}")).unwrap_or_else(|| bonus_english(b))),
        None => ("Bonus1".to_string(), bonus_english(b)),
    }
}

/// The bonus's name as the original's item text writes it (499eb0): its `[Army] Bonus<N>`
/// text up to " - ", trimmed (the table 4e36cc fills at c35000); ours without an install.
pub(super) fn bonus_title(b: &Bonus) -> String {
    bonus_number(b)
        .and_then(|n| chrome::ui_text("Army", &format!("Bonus{n}")))
        .map(|t| title_of(&t).to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| razdor::rules::items::bonus_name(b))
}

/// The name part of a bonus text: up to the first " - ", spaces trimmed (0x49a5c6).
fn title_of(text: &str) -> &str {
    razdor::rules::item_text::bonus_title(text)
}

fn hero_trait(h: HeroClass) -> (String, String, Color) {
    let n = HeroClass::ALL.iter().position(|&c| c == h).unwrap_or(0) + 1;
    let english = match h {
        HeroClass::Knight => tr("The army of this hero takes 10% less damage from enemy attacks (magic excepted)."),
        HeroClass::Archmage => tr("The Archmage casts spells twice as fast for 50% less mana, but his army gets no bonuses."),
        HeroClass::Ranger => tr("The army of this hero travels 20% faster, and the wounded heal 20% of their hits every day."),
    };
    // In the yellow SanSerif (0x492f24 → 0x492e70 with ae24c0).
    (format!("HeroBonus{n}"), chrome::ui_text("NewHero", &format!("Bonus{n}")).unwrap_or_else(|| english.to_string()), StripInk::Yellow.color())
}

/// The state lines closing the panel's traits (492f24), each with its icon of the table
/// c36000.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StateLine {
    /// `Bonus-Ressurrect` (c36000[3]) with `[Army] Hint3`.
    Resurrect,
    /// `Bonus-NoPayment` (c36000[4]) with ae6c0c.
    NoPayment,
    /// `Bonus-2Row` (c36000[6]) with `Hint1`.
    BackRow,
    /// `Bonus-InCastle` (c36000[5]) with `Hint2`.
    InCastle,
}

/// Which state lines the panel shows (492f24): a dead unit (HP 0) only the resurrection
/// line, else an unpaid one only the no-payment line, else the back row's, then the
/// building's.
fn state_lines(dead: bool, unpaid: bool, back_row: bool, in_building: bool) -> Vec<StateLine> {
    if dead {
        return vec![StateLine::Resurrect];
    }
    if unpaid {
        return vec![StateLine::NoPayment];
    }
    let mut v = Vec::new();
    if back_row {
        v.push(StateLine::BackRow);
    }
    if in_building {
        v.push(StateLine::InCastle);
    }
    v
}

fn state_trait(line: StateLine) -> (String, String, Color) {
    let own = |key: &str| chrome::ui_text("Army", key);
    let (art, text) = match line {
        StateLine::Resurrect => ("Bonus-Ressurrect".into(), own("Hint3").unwrap_or_else(|| tr("A dead unit can be raised only in a church or a town!").into())),
        // The original's no-payment text ae6c0c is never set: `Hint4` is read into c35000[0]
        // instead (4e3dad), so the line is the icon over an empty row (bug kept).
        StateLine::NoPayment => ("Bonus-NoPayment".into(), String::new()),
        StateLine::BackRow => ("Bonus-2Row".into(), own("Hint1").unwrap_or_else(|| tr("In the second row the unit gets a bonus to its ranged defence!").into())),
        StateLine::InCastle => ("Bonus-InCastle".into(), own("Hint2").unwrap_or_else(|| tr("In its own building the unit gets a bonus to all defences!").into())),
    };
    // The dead and the unpaid in red (ae24c8), the back row and the own building in blue
    // (ae24cc), as 0x492f24 passes them to 0x492e70.
    let ink = if matches!(line, StateLine::Resurrect | StateLine::NoPayment) { RED_TEXT } else { BLUE_TEXT };
    (art, text, ink)
}

/// The traits the panel lists: the hero's class, the unit's bonuses (its items' too), then
/// its state ([`state_lines`]).
fn traits_of(s: &Sheet) -> Vec<(String, String, Color)> {
    let mut traits: Vec<(String, String, Color)> = s.start.bonuses.iter().map(|bonus| {
        let (art, text) = trait_line(bonus);
        (art, text, super::typography::Ink::Green.color())
    }).collect();
    if let Some(h) = s.hero {
        traits.insert(0, hero_trait(h));
    }
    traits.extend(state_lines(s.hp <= 0, s.unpaid, s.back_row, s.building > 0).into_iter().map(state_trait));
    traits
}

/// The six spaces the original's loaders put before a unit's description (0x48efd0, the
/// string at 0x48f820) and before every trait and hint text (0x4e36cc: 0x4e3d44–0x4e3ead,
/// the string at 0x4e4e40): the first row of each indents, the icon stands in it.
const INDENT: &str = "      ";

/// How far a band row's text moves right so it starts after its trait icon. The icon stands at
/// 16 px and is 25 px wide (right edge 41 px); the text should start 3 px past it (44 px), and
/// starts at 12 px plus the indent's width (`indent_w`, at the band's font size). Zero where the
/// indent is already wide enough (the original's own look). Design px times `base`.
fn icon_text_shift(indent_w: f32, base: f32) -> f32 {
    let need = (16.0 + 25.0 + 3.0 - 12.0) * base;
    (need - indent_w).max(0.0)
}

/// One row of the panel's description list (0x667bdc): its text, ink and the icon drawn
/// beside it (the first row of a trait).
#[derive(Clone, Debug, PartialEq)]
struct ListRow {
    text: String,
    ink: Color,
    icon: Option<String>,
}

/// The description list as 0x492f24 fills it: the unit's description (`desc`, none for a
/// hero), then each trait (0x492e70): an empty row first when the list already has rows
/// (0x47e944), then its text, its icon by its first row. Every text but an empty one
/// starts with [`INDENT`]; each is word-wrapped to `room` as TextList.AddText 0x47e46c does
/// (`width`: a character's advance), left aligned.
fn list_rows(desc: Option<&str>, traits: &[(String, String, Color)], room: f32, width: impl Fn(char) -> f32 + Copy) -> Vec<ListRow> {
    let indented = |t: &str| if t.is_empty() { String::new() } else { format!("{INDENT}{t}") };
    let wrap = |t: &str| razdor::dt::markup::wrap(&indented(t), room, width).into_iter().map(|r| r.text);
    let mut rows: Vec<ListRow> = desc.map(|d| wrap(d).map(|text| ListRow { text, ink: CREAM, icon: None }).collect()).unwrap_or_default();
    for (art, text, ink) in traits {
        if !rows.is_empty() {
            rows.push(ListRow { text: String::new(), ink: *ink, icon: None });
        }
        for (i, text) in wrap(text).enumerate() {
            rows.push(ListRow { text, ink: *ink, icon: (i == 0).then(|| art.clone()) });
        }
    }
    rows
}

/// The panel's fill is 262 of the original's px wide (0x492f24: X0+2..X0+0x108).
const PANEL_W: f32 = 262.0;

/// The panel's text blocks in the original's px, stacked up from the bottom of its fill
/// (0x492f24 with the lists placed at 0x4d9060): the description list (x 12 in, 238 wide,
/// 14 px a row, its bottom at the fill's bottom, `rows`·14 + 12 tall), over it the stat
/// lists (`stats`+1)·14 + 8 tall (8 px higher for a hero), the level line 13 px over them,
/// the name list (16 + 18 px) over that. Returns the tops of the description, the stats and
/// the name, in px above the fill's bottom.
fn panel_tops(rows: usize, stats: usize, hero: bool) -> (f32, f32, f32) {
    let desc = rows as f32 * 14.0 + 12.0;
    let stat = desc + (stats + 1) as f32 * 14.0 + 8.0 + if hero { 8.0 } else { 0.0 };
    (desc, stat, stat + 16.0 + 18.0)
}

/// Trait rows the panel shows at once under the simultaneous bonuses rule: the abilities
/// region scrolls, and this many rows fit its place (the description above it never does).
const ABILITY_ROWS: usize = 8;

/// The rows the list shows: the description's `desc` rows, always all of them, and the trait
/// rows, all of them, or on the simultaneous bonuses rule at most [`ABILITY_ROWS`] (the rest
/// scroll inside the region). Returns `(desc, traits)`.
fn shown_rows(desc: usize, traits: usize, simultaneous: bool) -> (usize, usize) {
    (desc, if simultaneous { traits.min(ABILITY_ROWS) } else { traits })
}

/// A stat line: label, value, colour.
type Line = (String, String, Color);

/// A stat's name: the install's own (`[Skills] <key>`) in Russian, else ours.
fn label(key: &str, english: &str) -> String {
    let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("Skills", key)).flatten();
    own.unwrap_or_else(|| tr(english).to_string())
}

/// The vampirism line's label. A negative value (an item's −10%) takes HP from the attacker:
/// Razdor's deviation (the owner's decision, 2026-10-20), its own «Кровопотеря» (blood loss);
/// the original keeps «Вампиризм» for it. The Community bleed line («Кровотечение») is a
/// separate stat and keeps its own label.
fn vampirism_label(shown: i32) -> String {
    if shown < 0 {
        tr(n_("Blood loss")).to_string()
    } else {
        label("SVampirizm", n_("Vampirism"))
    }
}

fn signed(v: i32) -> String {
    if v > 0 {
        format!("+{v}")
    } else {
        v.to_string()
    }
}

fn buff_lines(lines: &mut Vec<Line>, b: Buff, raise: bool) {
    let (ad, ini, act) = if raise {
        (label("Bless_Atk_Def", n_("Adds to attack/defence")), label("BlessIni", n_("Adds to initiative")), label("BlessMov", n_("Hastens (+ actions)")))
    } else {
        (label("Curse_Atk_Def", n_("Lowers attack/defence")), label("CurseIni", n_("Lowers initiative")), label("CurseMov", n_("Slows (- actions)")))
    };
    if b.attack != 0 || b.defence != 0 {
        lines.push((ad.clone(), format!("{}/{}", signed(b.attack), signed(b.defence)), CREAM));
    }
    if b.initiative != 0 {
        lines.push((ini.clone(), signed(b.initiative), CREAM));
    }
    if b.actions != 0 {
        lines.push((act, signed(b.actions), CREAM));
    }
}

/// The ranged defence a unit of the back row gets against shots (`Row2Def`), as the
/// original's panel and cards add it to the shown value (0 outside the back row).
pub fn row2_extra(content: &Content, s: &Sheet) -> i32 {
    if s.back_row {
        content.options.row2_def
    } else {
        0
    }
}

/// The colour of a stat line's label and value (0x491a58): plain when the shown value equals
/// the level value (the type at the unit's level, no items), blue when above it, red when
/// below it or below the current value (red wins over blue).
pub(super) fn shown_ink(shown: i32, level: i32, current: i32) -> Color {
    if shown < level || shown < current {
        RED_TEXT
    } else if shown > level {
        BLUE_TEXT
    } else {
        CREAM
    }
}

/// The stat list of `s`, as the original's panel orders it (0x491fa4). Every value is the
/// shown one (`now`: the current stats outside battle, with items and spells; the battle
/// values in battle), coloured against its level value and the current one (`start`).
/// Blue above the level value, red below it or below the current value (`shown_ink`).
fn stat_lines(content: &Content, s: &Sheet) -> Vec<Line> {
    let (now, start) = (s.now, s.start);
    let naked = Stats::of_level(content, s.kind, s.level);
    let mut lines: Vec<Line> = Vec::new();
    // Hits: the current HP, or the max HP when unhurt; a wounded unit's Hits are red, as the
    // current HP is compared with the max HP.
    let max = now.max_hp();
    let hits = if s.hp < max { s.hp.max(0).to_string() } else { max.to_string() };
    let hits_ink = if s.hp < max { RED_TEXT } else { shown_ink(max, naked.max_hp(), start.max_hp()) };
    lines.push((label("SHit", n_("Hits")), hits, hits_ink));
    // An attack's line: left out when shown and current are both 0 (491a58).
    let attack = |st: Stat| -> Option<(String, Color)> {
        (now[st] != 0 || start[st] != 0).then(|| (now[st].to_string(), shown_ink(now[st], naked[st], start[st])))
    };
    // A defence never shows below 0. In a building its defence n is written apart, "v + n":
    // v is the defence without it, and only v is compared with the level and current values.
    // The back row's `Row2Def` joins n on the ranged defence. The line is kept while v + n is
    // above 0 (491fa4, 491b84).
    let defence = |st: Stat| -> Option<(String, Color)> {
        let extra = s.building + if st == Stat::DefenceShot { row2_extra(content, s) } else { 0 };
        let level = naked[st].max(0);
        let own = (now[st] - s.building).max(0);
        if extra > 0 && own + extra > 0 {
            let current = (start[st] - s.building).max(0);
            Some((format!("{own} + {extra}"), shown_ink(own, level, current)))
        } else {
            let shown = now[st].max(0);
            let current = start[st].max(0);
            (shown != 0 || current != 0).then(|| (shown.to_string(), shown_ink(shown, level, current)))
        }
    };
    // The original's order: melee attack and defence, then ranged attack and defence (491fa4).
    if let Some((v, c)) = attack(Stat::AttackBlow) {
        lines.push((label("SAttackBlow", n_("Melee attack")), v, c));
    }
    if let Some((v, c)) = defence(Stat::DefenceBlow) {
        lines.push((label("SDefenceBlow", n_("Melee defence")), v, c));
    }
    if let Some((v, c)) = attack(Stat::AttackShot) {
        lines.push((label("SAttackShot", n_("Ranged attack")), v, c));
    }
    if let Some((v, c)) = defence(Stat::DefenceShot) {
        lines.push((label("SDefenceShot", n_("Ranged defence")), v, c));
    }
    if now.is_mage() || start.is_mage() {
        let school = now.magic.unwrap_or(MagicSchool::Elemental);
        let p = s.power;
        let o = &content.options;
        // The magic lines are compared with the level power only.
        let level_power = naked[Stat::MagicPower];
        let pc = shown_ink(p, level_power, level_power);
        let dir = now.magic_direction();
        if matches!(dir, MagicDirection::ToEnemy | MagicDirection::ToAll) {
            // The kind of magic it strikes with in the brackets, not "(−hits)" (Razdor's: the
            // original says only that hits are lost; the defences name the three kinds).
            let kind = match school {
                MagicSchool::Life => tr("Life"),
                MagicSchool::Elemental => tr("Elemental"),
                MagicSchool::Death => tr("Death"),
            };
            let strike = label("StrikeHit", n_("Magic strike (- hits)"));
            let strike = format!("{} ({kind})", strike.split('(').next().unwrap_or(&strike).trim_end());
            lines.push((strike, format!("-{p}"), pc));
            buff_lines(&mut lines, curse_effect(o, school, p), false);
        }
        if matches!(dir, MagicDirection::ToAlly | MagicDirection::ToAll) {
            let heal = match school {
                MagicSchool::Life => p,
                MagicSchool::Elemental => p / 2,
                MagicSchool::Death => 0,
            };
            if heal > 0 {
                lines.push((label("CureHit", n_("Heals (+ hits)")), format!("+{heal}"), pc));
            }
            buff_lines(&mut lines, bless_effect(o, school, p), true);
        }
    }
    // The three protections as one `SProtectAllMagic` line when they are equal and above 0
    // (491fa4; the label 0x4e4f3c, read at 4e3fd3).
    let protections = [Stat::ProtectLife, Stat::ProtectElemental, Stat::ProtectDeath];
    let all = now[Stat::ProtectLife];
    let merged = all > 0 && protections.iter().all(|&st| now[st] == all);
    if merged {
        let c = protections.iter().map(|&st| shown_ink(now[st], naked[st], start[st])).find(|&c| c != CREAM).unwrap_or(CREAM);
        lines.push((label("SProtectAllMagic", n_("Immunity to magic")), format!("{all}%"), c));
    }
    // Negative regeneration is poison (`SPoison`, 491fa4).
    let regen = if now[Stat::Regen] < 0 { label("SPoison", n_("Poison")) } else { label("SRegen", n_("Regeneration")) };
    for (label, st) in [
        (label("SProtectLife", n_("Life magic protection")), Stat::ProtectLife),
        (label("SProtectElemental", n_("Elemental magic protection")), Stat::ProtectElemental),
        (label("SProtectDeath", n_("Death magic protection")), Stat::ProtectDeath),
        (regen, Stat::Regen),
    ] {
        if merged && protections.contains(&st) {
            continue;
        }
        if now[st] != 0 || start[st] != 0 {
            lines.push((label, format!("{}%", now[st]), shown_ink(now[st], naked[st], start[st])));
        }
    }
    // The Community bleed line comes after regeneration (4924c1) and before vampirism. It has
    // no level value in the original, so it is plain.
    if s.bleed != 0 {
        lines.push((label("SBleed", n_("Bleeding")), format!("{}%", s.bleed), CREAM));
    }
    if now[Stat::Vampirizm] != 0 || start[Stat::Vampirizm] != 0 {
        let st = Stat::Vampirizm;
        lines.push((vampirism_label(now[st]), format!("{}%", now[st].abs()), shown_ink(now[st], naked[st], start[st])));
    }
    // Initiative never shows below 0.
    let (ini, ini_now, ini_start) = (Stat::Initiative, now[Stat::Initiative].max(0), start[Stat::Initiative].max(0));
    if ini_now != 0 || ini_start != 0 {
        lines.push((label("SInitiative", n_("Initiative")), ini_now.to_string(), shown_ink(ini_now, naked[ini].max(0), ini_start)));
    }
    if let Some((v, c)) = attack(Stat::Manevres) {
        lines.push((label("SManevres", n_("Actions")), v, c));
    }
    if s.wage > 0 {
        lines.push((label("DailyPayment", n_("Daily wage")), s.wage.to_string(), ORANGE_TEXT));
    }
    lines
}

/// A named character's name on the panel (the original's ae24a0 font, yellow Benguiat).
const NAMED_INK: Color = Color::new(1.0, 0.86, 0.35, 1.0);

/// The SanSerif tints a card's strip is written in (fonts loaded at 0x4dbc26: white less the
/// colour given; 0x49462c picks them). The strip has no shadow font (0x478978 mode 1).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripInk {
    /// ae24bc, untinted: the attack and the manoeuvres.
    White,
    /// ae24c0, less (25, 25, 125): the defence and the initiative.
    Yellow,
    /// ae24c4, less (20, 90, 180): the hits of an unhurt unit.
    Orange,
    /// ae24c8, less (0, 152, 159): a value below its own, the hits of a wounded unit.
    Red,
    /// ae24cc, less (180, 70, 0): a value above its own.
    Blue,
}

impl StripInk {
    pub fn color(self) -> Color {
        let [r, g, b] = self.rgb();
        Color::from_rgba(r, g, b, 255)
    }

    pub fn rgb(self) -> [u8; 3] {
        match self {
            StripInk::White => [255, 255, 255],
            StripInk::Yellow => [230, 230, 130],
            StripInk::Orange => [235, 165, 75],
            StripInk::Red => [255, 103, 96],
            StripInk::Blue => [75, 185, 255],
        }
    }
}

/// The tint of a strip value (0x49462c): blue when above its own value, red when below,
/// else `normal`. `blue` is false where the original has no blue (the manoeuvres and the
/// initiative are only ever red or their own tint).
fn strip_ink(cur: i32, base: i32, normal: StripInk, blue: bool) -> StripInk {
    match cur.cmp(&base) {
        std::cmp::Ordering::Greater if blue => StripInk::Blue,
        std::cmp::Ordering::Less => StripInk::Red,
        _ => normal,
    }
}

/// The card's attack piece (0x49462c): "Pwr:" with the magic power for a caster (attack kind
/// 0x11) whose melee attack is 0 or that stands outside the original's places 1–4 (see
/// [`strip_place`]); for anyone else "A:" with the ranged attack when it is above the melee
/// attack, else the melee attack. Blue when any of the three attacks is above its value in
/// `base`, else red when any is below.
fn attack_piece(s: &Stats, base: &Stats, power: i32, caster: bool, place: usize) -> (String, StripInk) {
    let (ab, sh) = (s[Stat::AttackBlow], s[Stat::AttackShot]);
    let now = [ab, sh, power];
    let was = [base[Stat::AttackBlow], base[Stat::AttackShot], base[Stat::MagicPower]];
    let color = if now.iter().zip(was).any(|(n, w)| *n > w) {
        StripInk::Blue
    } else if now.iter().zip(was).any(|(n, w)| *n < w) {
        StripInk::Red
    } else {
        StripInk::White
    };
    let text = if caster && (ab <= 0 || !(1..=4).contains(&place)) {
        razdor::trf!("Pwr: {power}", power)
    } else {
        razdor::trf!("A: {v}", v = if ab < sh { sh } else { ab })
    };
    (text, color)
}

/// The original's card place (0..11, 0x492940) of `slot`: the six places of the front line,
/// then the six of the back line, as the formation draws them. The stat strip's "Pwr:" test
/// reads places 1–4 as the front (so on the wide row the front's two outer places are not).
pub fn strip_place(f: Formation, slot: Slot) -> usize {
    let (line, col) = f.display(slot);
    line * 6 + col as usize
}

/// Whether `unit`'s type is a caster (attack kind 0x11) for the stat strip.
pub fn caster(c: &Content, unit: UnitId) -> bool {
    razdor::rules::ai::attack_kind(c, unit) == 0x11
}

/// The panel a card's strip is drawn on (0x49462c): in the hero's army and in battle the
/// hero (and in battle the enemy's first unit) has the red one (ae269c), a named character
/// the blue one (ae26a0); everyone else, and any garrison outside battle, the plain strip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StripPanel {
    Plain,
    Hero,
    Named,
}

impl StripPanel {
    /// The panel of a unit of the hero's army: index 0 is the hero.
    pub fn of_squad(index: usize, named: u8) -> StripPanel {
        if index == 0 {
            StripPanel::Hero
        } else if named > 0 {
            StripPanel::Named
        } else {
            StripPanel::Plain
        }
    }
}

/// The card's defence piece (0x49462c): the melee and the ranged defence with the
/// building defence of the unit's army (army +0x378c) added to both and the back row's
/// `Row2Def` to the ranged one; none when both come to 0 or less (the original then leaves
/// "D:" out: a healer's card reads only "Pwr: 15").
pub fn defence_piece(now: &Stats, row2: i32, building: i32) -> Option<(i32, i32)> {
    let (blow, shot) = (now[Stat::DefenceBlow] + building, now[Stat::DefenceShot] + row2 + building);
    (blow > 0 || shot > 0).then_some((blow, shot))
}

/// The original's card is 94 px wide; its strip (94×39) starts under the 94 px portrait.
const CARD_W: f32 = 94.0;

/// Where the strip's pieces stand, as the original's (0x494f93–0x49565f), in screen px of a
/// strip `strip` (scaled from the original's 94 px card): the three lines' cell tops 2, 14
/// and 26 px under the portrait; the left pieces 2 px in, the right ones ending 1 px before
/// the card's right edge (x + 94 − 1 − width), the hits centred (x + (94 − width) / 2).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StripLayout {
    pub tops: [f32; 3],
    pub left: f32,
    pub right_end: f32,
    scale: f32,
    x: f32,
    w: f32,
}

impl StripLayout {
    pub fn of(strip: Rect) -> StripLayout {
        let s = strip.w / CARD_W;
        StripLayout {
            tops: [2.0, 14.0, 26.0].map(|t| strip.y + t * s),
            left: strip.x + 2.0 * s,
            right_end: strip.x + strip.w - s,
            scale: s,
            x: strip.x,
            w: strip.w,
        }
    }

    /// The baseline of a line's text (0x494f93–0x49565f): the line's cell top plus the body
    /// face's baseline offset `body_base` (see [`cell_baseline`]). Whatever size the text is
    /// drawn at, a line keeps its place: a shrunk first line does not rise over the portrait.
    pub fn baseline(&self, line: usize, body_base: f32) -> f32 {
        self.tops[line] + body_base
    }

    /// The x of a centred piece `width` wide (the original's halving drops the odd pixel).
    pub fn centred(&self, width: f32) -> f32 {
        self.x + (((self.w - width) / self.scale) / 2.0).floor() * self.scale
    }
}

/// The strip under a card's portrait, as the original's: "A: 45  D: 35/40", "Mnvr: 1
/// Ini: 12", "Hits: 70" (or "Hits: 45/70"). `row2` is added to the ranged defence shown
/// (the back row's `Row2Def`, 0 elsewhere) and `building` (the defence of the building the
/// unit's army or garrison stands in, army +0x378c) to both ([`defence_piece`]). `lit`
/// reddens it (the unit acting, or the one selected on the army screen). `caster` and
/// `place` choose the attack piece ([`caster`], [`strip_place`]).
#[allow(clippy::too_many_arguments)]
pub fn stat_strip(strip: Rect, now: &Stats, base: &Stats, power: i32, caster: bool, place: usize, hp: i32, row2: i32, building: i32, lit: bool, panel: StripPanel) {
    stat_strip_impl(strip, now, base, power, caster, place, hp, row2, building, lit, panel);
}

/// [`stat_strip`] in battle: `now` already holds the side's building defence
/// (`Battle::shown_stats`).
#[allow(clippy::too_many_arguments)]
pub fn battle_stat_strip(strip: Rect, now: &Stats, base: &Stats, power: i32, caster: bool, place: usize, hp: i32, row2: i32, lit: bool, panel: StripPanel) {
    stat_strip_impl(strip, now, base, power, caster, place, hp, row2, 0, lit, panel);
}

/// The defence piece's tint (0x495460–0x495510): yellow, or, from the ranged defence when the
/// back row adds to it, else from the melee one, blue above its own value and red below.
fn defence_ink(now: &Stats, base: &Stats, row2: i32) -> StripInk {
    let st = if row2 > 0 { Stat::DefenceShot } else { Stat::DefenceBlow };
    strip_ink(now[st], base[st], StripInk::Yellow, true)
}

#[allow(clippy::too_many_arguments)]
fn stat_strip_impl(strip: Rect, now: &Stats, base: &Stats, power: i32, caster: bool, place: usize, hp: i32, row2: i32, building: i32, lit: bool, panel: StripPanel) {
    // The strip's own picture, made from the paper at start-up (0x4de0f0,
    // `chrome::strip_picture`): dark brown, the hero's deep red-brown, a named character's
    // deep blue, in battle and on the army and building screens alike (0x49462c draws
    // ae2698, then ae269c or ae26a0 over it).
    match chrome::strip_texture(panel) {
        Some(t) => chrome::tex(&t, strip, WHITE),
        None => chrome::surface(strip, chrome::Skin::Strip),
    }
    if lit {
        draw_rectangle(strip.x, strip.y, strip.w, strip.h, Color::new(0.75, 0.25, 0.0, 0.4));
    }
    super::typography::Role::Body.with(|| strip_text(strip, now, base, power, caster, place, hp, row2, building));
}

#[allow(clippy::too_many_arguments)]
fn strip_text(strip: Rect, now: &Stats, base: &Stats, power: i32, caster: bool, place: usize, hp: i32, row2: i32, building: i32) {
    let l = StripLayout::of(strip);
    // SanSerif at the card's own scale, snapped to whole sheet pixels when near.
    let fs = super::dt_font::sheet_size(super::dt_font::Face::Body, super::dt_font::whole_scale(l.scale));
    let (att, ac) = attack_piece(now, base, power, caster, place);
    let d = defence_piece(now, row2, building).map(|(blow, shot)| razdor::trf!("D: {blow}/{shot}", blow, shot));
    // The two pieces of the first line: the original draws them at the other lines' size,
    // the attack at the left edge and the defence against the right (0x49462c, no shrink).
    // Here they keep that size when they fit with a gap between them, else only as much
    // smaller as needed (another font mode's or a translation's wider text).
    // The first line is fitted and drawn at one size: the native metrics fix the drawn size
    // (a requested one is not kept there), so the fit uses `measure_exact` and the draw
    // `text_exact` at the size the fit settled on. Measuring at one size and drawing at
    // another let "Pwr: 55" run into "D: 36/39" (a player's report).
    let fs_drawn = super::dt_font::native_size(super::dt_font::face(), fs);
    let fs1 = first_line_size(fs_drawn, l.right_end - l.left, |size| {
        let gap = measure_exact(" ", size).width.max(size * 0.3);
        measure_exact(&att, size).width + d.as_ref().map_or(0.0, |d| gap + measure_exact(d, size).width)
    });
    // The lines' places come from the body size, not the size a first line shrinks to, so a
    // long first line does not move up.
    let body_base = cell_baseline(fs);
    let put = |s: &str, x: f32, line: usize, size: f32, ink: StripInk| {
        super::widgets::text(s, x, l.baseline(line, body_base), size, ink.color());
    };
    super::widgets::text_exact(&att, l.left, l.baseline(0, body_base), fs1, ac.color());
    if let Some(d) = d {
        let x = l.right_end - measure_exact(&d, fs1).width;
        super::widgets::text_exact(&d, x, l.baseline(0, body_base), fs1, defence_ink(now, base, row2).color());
    }
    let (mn, ini) = (now[Stat::Manevres], now[Stat::Initiative]);
    put(&razdor::trf!("Mnvr: {mn}", mn), l.left, 1, fs, strip_ink(mn, base[Stat::Manevres], StripInk::White, false));
    let ini_text = razdor::trf!("Ini: {ini}", ini);
    put(&ini_text, l.right_end - measure(&ini_text, fs).width, 1, fs, strip_ink(ini, base[Stat::Initiative], StripInk::Yellow, false));
    let max = now.max_hp();
    let (hits, ink) = if hp < max {
        (razdor::trf!("Hits: {hp}/{max}", hp = hp.max(0), max), StripInk::Red)
    } else {
        (razdor::trf!("Hits: {max}", max), StripInk::Orange)
    };
    put(&hits, l.centred(measure(&hits, fs).width), 2, fs, ink);
}

/// Screen px from a text cell's top (where the original places its text) down to the
/// baseline (where Razdor draws it) at `size` in the current face.
fn cell_baseline(size: f32) -> f32 {
    super::dt_font::cell_baseline(size).unwrap_or_else(|| measure("H", size).offset_y)
}

/// The size a card's first line is drawn at: `fs` when `width(fs)` (the two pieces and the gap
/// between them) fits in `room`, else the largest size below it that fits, down to 7.
fn first_line_size(fs: f32, room: f32, width: impl Fn(f32) -> f32) -> f32 {
    let mut size = fs;
    while size > 7.0 && width(size) > room {
        size -= 0.5;
    }
    size.max(7.0)
}

/// Rects of the item slots around the figure: two at the top left, two at the top right (as
/// in the army screen), and with six cells a third pair under them. Only the first
/// `slot_count` are shown.
pub fn slot_rects(r: Rect) -> [Rect; MAX_SLOTS] {
    slot_rects_at(r, 0.9375 * k())
}

/// The worn-item slots of a panel whose fill is `r`, `px` screen pixels to one of the
/// original's (0x4d8638 at 4d88ad..4d8994): 55 px buttons 8 px in from the fill's top and
/// sides, the second of each side right under the first (55 px down), the left pair at
/// the fill's left, the right pair against its right. The six-cell rule adds the third
/// pair (cells 4 and 5) under the first two, touching them.
pub fn slot_rects_at(r: Rect, px: f32) -> [Rect; MAX_SLOTS] {
    let (s, m) = (55.0 * px, 8.0 * px);
    let (left, right) = (r.x + m, r.x + r.w - m - s);
    let row = |i: f32| r.y + m + i * s;
    [
        Rect::new(left, row(0.0), s, s),
        Rect::new(left, row(1.0), s, s),
        Rect::new(right, row(0.0), s, s),
        Rect::new(right, row(1.0), s, s),
        Rect::new(left, row(2.0), s, s),
        Rect::new(right, row(2.0), s, s),
    ]
}

/// Razdor's army and battle windows are the original's at 15/16: the panel's 262×609 px
/// fill (0x492f24: X0+2..X0+0x108, Y0+0x1d..Y0+0x27e) is 244×570 here.
const FIGURE_SCALE: f32 = 0.9375;
/// The column's height in design px (its 571 under the title, the stacked band's scale).
const COLUMN_H: f32 = 571.0;
/// The height of the original's figure box under the panel's top, in its pixels (0x1b4).
const FIGURE_BOX: f32 = 436.0;

/// Where the unit's figure, `w`×`h` in the picture's own pixels, stands on the panel `r`, as
/// the original's (0x492f24 → 0x476a3c): unscaled, its picture (not its body) centred across
/// the panel (x = X0 + (0x10a − w) / 2, the middle of the fill X0+2..X0+0x108), and centred
/// down a 436 px box from the fill's top (y = Y0 + 0x1d + (0x1b4 − h) / 2), so its middle
/// stands 218 px under the top whatever its height. No size limit: a taller picture would
/// rise above the box.
fn figure_rect(r: Rect, w: f32, h: f32, k: f32) -> Rect {
    let s = FIGURE_SCALE * k;
    let (w, h) = (w * s, h * s);
    Rect::new(r.x + (r.w - w) / 2.0, r.y + (FIGURE_BOX * s - h) / 2.0, w, h)
}

/// Where the figure, `w`×`h` in the picture's own pixels at `s` screen px each, stands centred in
/// the band's picture half `r` (Razdor's stacked band, not parity; the landscape keeps `figure_rect`).
fn band_figure_rect(r: Rect, w: f32, h: f32, s: f32) -> Rect {
    let (w, h) = (w * s, h * s);
    Rect::new(r.x + (r.w - w) / 2.0, r.y + (r.h - h) / 2.0, w, h)
}

/// Where the band's frame picture (`UnitFrame`) is stretched: over the band's picture half, the
/// parchment the figure stands on, as the landscape column's frame surrounds its own parchment
/// (Razdor's phone choice, not parity). Stretched over the whole band, its pieces sat at the
/// band's corners, the right one over the text half.
fn band_frame_rect(band: Rect) -> Rect {
    window_layout::band_halves(band).0
}

/// A placeholder figure: a dark sepia silhouette.
fn silhouette(r: Rect) {
    let c = Color::new(0.30, 0.18, 0.08, 0.55);
    let (cx, top) = (r.x + r.w / 2.0, r.y);
    let h = r.h;
    draw_circle(cx, top + h * 0.12, h * 0.075, c);
    let (a, b) = (vec2(cx - h * 0.09, top + h * 0.22), vec2(cx + h * 0.09, top + h * 0.22));
    let (cc, d) = (vec2(cx + h * 0.16, top + h * 0.95), vec2(cx - h * 0.16, top + h * 0.95));
    draw_triangle(a, b, cc, c);
    draw_triangle(a, cc, d, c);
    draw_triangle(a, vec2(cx - h * 0.2, top + h * 0.5), vec2(cx - h * 0.1, top + h * 0.3), c);
    draw_triangle(b, vec2(cx + h * 0.2, top + h * 0.5), vec2(cx + h * 0.1, top + h * 0.3), c);
}

/// The worn-item slots (`rects`, one per slot of the unit's content): an item is its picture alone
/// on the parchment, an empty slot the original's empty-slot picture (0xae2860, built at start-up: a
/// steel ring over a 40% black square, `chrome::empty_item_slot`), set by 0x492f24 only for an empty
/// slot. Returns the slot pressed and its rectangle; the item under the mouse goes to `hover`.
fn worn_slots(assets: &Assets, content: &Content, rects: [Rect; MAX_SLOTS], s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<(usize, Rect)> {
    let mut clicked = None;
    for (i, sr) in rects.iter().take(slot_count(content)).enumerate() {
        let item = s.items[i];
        if !slots && item.is_none() {
            continue;
        }
        match item {
            Some(it) => assets.draw_item(it, sr.x, sr.y, sr.w),
            None => chrome::empty_item_slot(*sr),
        }
        // Razdor's: a worn item of the army and hero windows (`slots`) is pulled out of its slot
        // (`item_pull`), and a pulled item dropped on a slot is a press there, as a click.
        if slots && item.is_some() {
            super::pan::pull_source(*sr, None);
        }
        if mouse_in(sr.x, sr.y, sr.w, sr.h) {
            if item.is_some() {
                *hover = item;
            }
            if super::widgets::clicked() || (slots && super::item_pull::dropped()) {
                clicked = Some((i, *sr));
            }
        }
    }
    clicked
}

/// The level and experience line at `level_y`, then the stat lines and the status lines, a row
/// `pitch` apart from `stat_y` down (the column's 14 px, the band's spread rows); the column and the
/// band share it.
#[allow(clippy::too_many_arguments)]
fn stat_block(s: &Sheet, stats: Vec<Line>, x0: f32, x1: f32, level_y: f32, stat_y: f32, size: f32, bold: f32, pitch: f32, share_line: bool) {
    use super::dt_font::Face;
    let k = k();
    let line = |top: f32| top + cell_baseline(size);
    shadow_text(&razdor::trf!("Level {level}", level = s.level), x0, line(level_y), size, CREAM);
    shadow_right(&razdor::trf!("XP {xp} / {need}", xp = s.xp, need = s.need), x1, line(level_y), size, CREAM);
    let mut y = stat_y;
    for (label, value, color) in stats {
        // The daily wage's label and value are in the bold orange SanSerif over its shadow
        // (0x4924c9: ae24f4, shadow ae2510).
        let wage = color == ORANGE_TEXT;
        let (face, fs) = if wage { (Face::Bold, bold) } else { (Face::Body, size) };
        super::dt_font::with_face(face, || {
            let room = x1 - x0 - measure(&value, fs).width - 6.0 * k;
            let label_size = super::widgets::fit_size(&label, room, fs);
            let label = super::widgets::ellipsize(&label, room, label_size);
            shadow_text(&label, x0, y + cell_baseline(label_size), label_size, color);
            // The band's label may shrink to fit beside its value: the value then shares its line.
            let value_line = if share_line { cell_baseline(label_size) } else { cell_baseline(fs) };
            shadow_right(&value, x1, y + value_line, fs, color);
        });
        y += pitch;
    }
    for (status, color) in &s.status {
        let fitted = super::widgets::fit_size(status, x1 - x0, size);
        let status = super::widgets::ellipsize(status, x1 - x0, fitted);
        shadow_text(&status, x0, y + cell_baseline(fitted), fitted, *color);
        y += pitch;
    }
}

/// The band's text scale on the column's text px `base`: the largest of 1.00 … 2.00 (steps of 0.05)
/// at which the band's text fits its half (`band_text_fits`). Never below the column's size.
fn band_text_scale(s: &Sheet, content: &Content, txt: Rect, base: f32) -> f32 {
    let mut best = 1.0;
    for step in 0..=20 {
        let m = 1.0 + step as f32 * 0.05;
        if band_text_fits(s, content, txt, base, base * m) {
            best = m;
        }
    }
    best
}

/// Whether the stat lines (label and value side by side) fit the half's width, and the stats with
/// at least the traits' first rows fit its height, at the text px `px`.
fn band_text_fits(s: &Sheet, content: &Content, txt: Rect, base: f32, px: f32) -> bool {
    use super::dt_font::{sheet_size, whole_scale, Face};
    let k = k();
    let size = sheet_size(Face::Body, whole_scale(px));
    let stats = stat_lines(content, s);
    let fits_width = super::dt_font::with_face(Face::Body, || {
        stats.iter().all(|(label, value, _)| measure(label, size).width + measure(value, size).width + 6.0 * k <= 206.0 * base * 0.85)
            && measure(s.name, sheet_size(Face::Subtitle, whole_scale(px))).width <= txt.w - 16.0 * k
    });
    let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
    let shift = icon_text_shift(measure(INDENT, size).width, base);
    let trait_rows = list_rows(None, &traits_of(s), 238.0 * base - shift, width).len();
    let row_h = 14.0 * px;
    // The name's block (34 px) under its top and the level line (13 px) above the first stat row.
    let stat_y = txt.y + 46.0 * px;
    let natural_end = stat_y + (stats.len() + s.status.len()) as f32 * row_h;
    let fits_height = natural_end + (trait_rows as f32 + 1.0) * row_h <= txt.bottom();
    fits_width && fits_height
}

/// The band (the army and hero windows stacked) and a pane's card (the hire tab's unit over the
/// upper pane): the unit's picture on the band's left half with its worn items, and on its right
/// half the name, the level, the stats and the traits, top down. The picture is the column's
/// figure fitted to the band's height, the text at the band's scale (1.2 times the column's on
/// the army band).
pub fn draw_band(assets: &Assets, content: &Content, band: Rect, s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<(usize, Rect)> {
    // The band's text grows with its half (`band_text_scale`): the original's sheet fonts take the
    // asked size only outside the frame's native metrics (as the phone's diary text, 1307).
    super::dt_font::legacy_metrics_if(true, || super::typography::Role::Body.with(|| draw_band_in_face(assets, content, band, s, slots, hover)))
}

fn draw_band_in_face(assets: &Assets, content: &Content, band: Rect, s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<(usize, Rect)> {
    use super::dt_font::{sheet_size, whole_scale, Face};
    let k = k();
    let (pic, txt) = window_layout::band_halves(band);
    chrome::unit_parchment_framed(band, band_frame_rect(band));
    // Screen px per design px of the figure and of the worn-item slots.
    let fig = FIGURE_SCALE * band.h / COLUMN_H;
    // Razdor's phone choice, not parity: the figure is centred in the picture half of the band
    // (its height), not in the landscape's 436 px box from the top, which left it high in the band.
    match assets.figure(s.kind) {
        Some(t) => chrome::tex(&t, band_figure_rect(pic, t.width(), t.height(), fig), WHITE),
        None => silhouette(band_figure_rect(pic, 0.6 * pic.w / fig, 0.95 * FIGURE_BOX, fig)),
    }
    let clicked = worn_slots(assets, content, slot_rects_at(pic, fig), s, slots, hover);
    // The original's px on the text half (the column's px is its width over the same 262), scaled
    // up to fill the half (`band_text_scale`; never below the column's size).
    let base = txt.w / PANEL_W;
    let px = base * band_text_scale(s, content, txt, base);
    let size = sheet_size(Face::Body, whole_scale(px));
    let bold = sheet_size(Face::Bold, whole_scale(px));
    let name_size = sheet_size(Face::Subtitle, whole_scale(px));
    // The text half is the column's dark brown text ground (multiplied parchment).
    chrome::multiply(|| draw_rectangle(txt.x, txt.y, txt.w, txt.h, Color::new(0.5, 0.35, 0.0, 1.0)));
    let name_ink = if s.named { NAMED_INK } else { CREAM };
    let name_top = txt.y + 12.0 * px;
    super::typography::Role::Name.with(|| {
        let name = super::widgets::ellipsize(s.name, txt.w - 16.0 * k, name_size);
        shadow_centered(&name, txt.x + txt.w / 2.0, name_top + cell_baseline(name_size), name_size, name_ink);
    });
    // The columns keep the column's width (`base`); only the text grows with `px`.
    let (x0, x1) = (txt.x + 28.0 * base, txt.x + 234.0 * base);
    let level_y = name_top + 21.0 * px;
    let stat_y = level_y + 13.0 * px;
    let stats = stat_lines(content, s);
    let row_h = 14.0 * px;
    // The description, then the traits, at the half's bottom. The description goes when the whole
    // list does not fit the band's text half; what still does not fit is cut at its end with "…".
    let traits = traits_of(s);
    let desc = s.hero.is_none().then(|| content.unit(s.kind).description.as_str());
    let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
    let room = 238.0 * base;
    let desc_rows = list_rows(desc, &[], room, width);
    // A trait's text starts after its icon (`icon_text_shift`): its wrap room shrinks by the same.
    let shift = icon_text_shift(measure(INDENT, size).width, base);
    let trait_rows = list_rows(None, &traits, room - shift, width);
    let rows_n = stats.len() + s.status.len();
    let natural_end = stat_y + rows_n as f32 * row_h;
    let avail = ((txt.bottom() - 16.0 * px - natural_end) / row_h).floor().max(0.0) as usize;
    let both = !desc_rows.is_empty() && !trait_rows.is_empty();
    let all = desc_rows.len() + usize::from(both) + trait_rows.len();
    let mut rows: Vec<ListRow> = if all <= avail {
        let mut rows = desc_rows;
        if both {
            rows.push(ListRow { text: String::new(), ink: CREAM, icon: None });
        }
        rows.extend(trait_rows);
        rows
    } else {
        trait_rows
    };
    if rows.len() > avail {
        rows.truncate(avail);
        if let Some(last) = rows.last_mut() {
            last.text = format!("{}…", last.text.trim_end());
        }
    }
    // The list sits at the half's bottom; the stat lines spread evenly from the level line down to it.
    let list_top = txt.bottom() - 8.0 * px - rows.len() as f32 * row_h;
    let pitch = if rows_n > 1 { ((list_top - 8.0 * px - stat_y) / rows_n as f32).max(row_h) } else { row_h };
    stat_block(s, stats, x0, x1, level_y, stat_y, size, bold, pitch, true);
    let _clip = super::widgets::Clip::new(txt);
    let line = |top: f32| top + cell_baseline(size);
    for (i, row) in rows.iter().enumerate() {
        if let Some(art) = &row.icon {
            chrome::trait_icon(art, txt.x + 16.0 * base, list_top + i as f32 * row_h - 11.0 * px, 25.0 * base);
        }
    }
    for (i, row) in rows.iter().enumerate() {
        if !row.text.trim().is_empty() {
            let x = txt.x + 12.0 * base + if row.icon.is_some() { shift } else { 0.0 };
            shadow_text(&row.text, x, line(list_top + i as f32 * row_h), size, row.ink);
        }
    }
    clicked
}

/// Draws the panel in `r`. `slots` shows the four item slots (empty ones too) and returns
/// the one clicked; the item under the mouse goes to `hover`.
pub fn draw(assets: &Assets, content: &Content, r: Rect, s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<(usize, Rect)> {
    super::typography::Role::Body.with(|| draw_in_face(assets, content, r, r, s, slots, hover))
}

/// Extend the parchment behind overlaid battle controls without moving the unit's text.
pub fn draw_extended(assets: &Assets, content: &Content, r: Rect, background: Rect, s: &Sheet, hover: &mut Option<ItemId>) {
    super::typography::Role::Body.with(|| draw_in_face(assets, content, r, background, s, false, hover));
}

fn draw_in_face(assets: &Assets, content: &Content, r: Rect, background: Rect, s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<(usize, Rect)> {
    use super::dt_font::{sheet_size, whole_scale, Face};
    let k = k();
    chrome::unit_parchment(background);
    // The figure, behind the text: at its own size, centred in the original's box
    // (`figure_rect`, 492f24 → 0x476a3c).
    match assets.figure(s.kind) {
        Some(t) => {
            // Drawn with the picture's own alpha only, at full strength (492f24 → 0x476a3c);
            // an extra overall transparency washed every engraving out.
            chrome::tex(&t, figure_rect(r, t.width(), t.height(), k), WHITE);
        }
        None => silhouette(figure_rect(r, 0.6 * r.w / (FIGURE_SCALE * k), 0.95 * FIGURE_BOX, k)),
    }
    // The original's px on this panel, and SanSerif at that scale (whole sheet pixels when
    // near).
    let px = r.w / PANEL_W;
    let size = sheet_size(Face::Body, whole_scale(px));
    let bold = sheet_size(Face::Bold, whole_scale(px));
    let name_size = sheet_size(Face::Subtitle, whole_scale(px));
    // The description list (0x667bdc): the type's description for any unit but the hero
    // (0x492f24: the type's `Descript` of Rus_Units.ini), then the traits with their icons.
    let simultaneous = content.experimental.simultaneous_bonuses;
    let traits = traits_of(s);
    let desc = s.hero.is_none().then(|| content.unit(s.kind).description.as_str());
    let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
    let list_width = (238.0 - if simultaneous { 16.0 } else { 0.0 }) * px;
    let mut rows = list_rows(desc, &traits, list_width, width);
    let stats = stat_lines(content, s);
    let stat_rows = stats.len() + s.status.len();
    // The description's rows are the first of the list, as wrapped at the list's width.
    let mut desc_rows = desc.map_or(0, |d| list_rows(Some(d), &[], list_width, width).len());
    let trait_rows = rows.len() - desc_rows;
    // The original stacks it all up from the bottom with no limit. Razdor keeps the name
    // under the top quarter: a mod's long texts cut the description short (a choice). The
    // count is of what the panel shows, so the ability rows' overflow never cuts it.
    let fits = |cut: usize| {
        let (d, t) = shown_rows(desc_rows - cut, trait_rows, simultaneous);
        r.y + r.h - panel_tops(d + t, stat_rows, s.hero.is_some()).2 * px < r.y + 0.25 * r.h
    };
    let mut cut = 0;
    while cut < desc_rows && fits(cut) {
        cut += 1;
    }
    if cut > 0 {
        rows.drain(desc_rows - cut..desc_rows);
        if let Some(last) = (desc_rows > cut).then(|| &mut rows[desc_rows - cut - 1]) {
            last.text = format!("{}…", last.text.trim_end());
        }
        desc_rows -= cut;
    }
    let (desc_shown, traits_shown) = shown_rows(desc_rows, rows.len() - desc_rows, simultaneous);
    let view_rows = desc_shown + traits_shown;
    let (desc_top, stat_top, name_top) = panel_tops(view_rows, stat_rows, s.hero.is_some());
    let bottom = r.y + r.h;
    let (desc_top, stat_top, name_top) = (bottom - desc_top * px, bottom - stat_top * px, bottom - name_top * px);
    // From the name's top down the parchment is dark brown, fading in over the 56 px above
    // (0x492f24: 28 steps of 2 rows); the figure shows through.
    let fade0 = name_top - 56.0 * px;
    chrome::unit_panel_fade(Rect::new(r.x, fade0, r.w, name_top - fade0));
    chrome::multiply(|| {
        // Multiplying (230, 178, 115) parchment by this gives the video's (115, 62, 0).
        draw_rectangle(r.x, name_top, r.w, background.bottom() - name_top, Color::new(0.5, 0.35, 0.0, 1.0));
    });
    // Items worn.
    let clicked = worn_slots(assets, content, slot_rects(r), s, slots, hover);
    // The name in Benguiat over its shadow, centred (the name list 0x667e64, align 2).
    let name_ink = if s.named { NAMED_INK } else { CREAM };
    super::typography::Role::Name.with(|| {
        let name = super::widgets::ellipsize(s.name, r.w - 16.0 * k, name_size);
        shadow_centered(&name, r.x + r.w / 2.0, name_top + cell_baseline(name_size), name_size, name_ink);
    });
    // The level and experience 13 px over the stat lists: from X0+30 and against X0+236
    // (0x492f24, the label list 0x667cb4 at X0+0x1e, the value list 0xa6 on and 0x28 wide).
    let (x0, x1) = (r.x + 28.0 * px, r.x + 234.0 * px);
    let line = |top: f32| top + cell_baseline(size);
    stat_block(s, stats, x0, x1, stat_top - 13.0 * px, stat_top, size, bold, 14.0 * px, false);
    // The description list from 12 px in, a row every 14 px; a trait's icon 16 px in and
    // 11 px over its first row (0x48e8f0), drawn under the text.
    let list_x = r.x + 12.0 * px;
    // The abilities region sits under the description, which never moves: only the trait rows
    // scroll inside it (on the simultaneous bonuses rule; else the whole list shows).
    let region = Rect::new(r.x + 8.0 * px, desc_top + desc_shown as f32 * 14.0 * px - 11.0 * px, r.w - 16.0 * px, (traits_shown as f32 * 14.0 + 19.0) * px);
    let most = (rows.len() - desc_rows - traits_shown) as f32 * 14.0 * px;
    let key = format!("{}:{}:{}:{}:{:?}:{:?}", r.x, r.y, s.kind.0, s.name, s.start.bonuses, s.items);
    let mut scroll = TRAIT_SCROLL.with(|p| p.borrow().get(&key).copied().unwrap_or(0.0));
    if simultaneous && most > 0.0 {
        super::widgets::swipe_rows(region, 42.0 * px);
        if region.contains(super::widgets::pointer().into()) { scroll -= super::widgets::wheel() * 42.0 * px; }
        scroll = scroll.clamp(0.0, most);
        super::widgets::scrollbar_px(Rect::new(region.right() - 12.0 * px, region.y, 12.0 * px, region.h), &mut scroll, most, region.h, 14.0 * px);
        TRAIT_SCROLL.with(|p| { let mut p = p.borrow_mut(); if p.len() > 32 { p.clear(); } p.insert(key, scroll); });
    } else {
        scroll = 0.0;
    }
    let row_top = |i: usize| {
        let top = desc_top + i as f32 * 14.0 * px;
        if i < desc_rows { top } else { top - scroll }
    };
    for (i, row) in rows[..desc_rows].iter().enumerate() {
        if !row.text.trim().is_empty() {
            shadow_text(&row.text, list_x, line(row_top(i)), size, row.ink);
        }
    }
    let _traits_clip = simultaneous.then(|| super::widgets::Clip::new(region));
    for (i, row) in rows.iter().enumerate().skip(desc_rows) {
        if let Some(art) = &row.icon {
            chrome::trait_icon(art, r.x + 16.0 * px, row_top(i) - 11.0 * px, 25.0 * px);
        }
    }
    for (i, row) in rows.iter().enumerate().skip(desc_rows) {
        if !row.text.trim().is_empty() {
            shadow_text(&row.text, list_x, line(row_top(i)), size, row.ink);
        }
    }
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stacked band's figure stands inside its picture half and is centred in it both ways
    /// (a player's phone report: the figure sat in the frame's top-left, not centred).
    #[test]
    fn the_band_figure_is_inside_its_parchment_and_centred() {
        let band = Rect::new(4.0, 300.0, 586.0 * 2.0, 420.0 * 2.0);
        let (pic, _) = window_layout::band_halves(band);
        let fig = FIGURE_SCALE * band.h / COLUMN_H;
        for (w, h) in [(120.0, 300.0), (60.0, 90.0), (200.0, 500.0)] {
            let r = band_figure_rect(pic, w, h, fig);
            assert!(r.x >= pic.x && r.right() <= pic.right(), "{r:?} inside the picture half {pic:?}");
            assert!(r.y >= pic.y && r.bottom() <= pic.bottom(), "{r:?} inside the picture half {pic:?}");
            assert!((r.x + r.w / 2.0 - (pic.x + pic.w / 2.0)).abs() < 0.5, "horizontally centred");
            assert!((r.y + r.h / 2.0 - (pic.y + pic.h / 2.0)).abs() < 0.5, "vertically centred");
        }
    }

    /// The band's frame picture surrounds the picture half (the figure's parchment) and nothing of
    /// the text half: the same rect as the parchment, inside the band (a phone report: the frame's
    /// pieces sat at the band's corners, not around the parchment).
    #[test]
    fn the_band_frame_surrounds_its_parchment_half() {
        let band = Rect::new(4.0, 300.0, 586.0 * 2.0, 420.0 * 2.0);
        let (pic, txt) = window_layout::band_halves(band);
        let frame = band_frame_rect(band);
        assert_eq!(frame, pic, "the frame is stretched over the picture half");
        assert!(frame.x == band.x && frame.y == band.y && frame.h == band.h, "{frame:?} at the band's left and top");
        assert!(frame.right() <= txt.x + 0.5, "{frame:?} stays off the text half {txt:?}");
    }

    /// A band row's text starts after its icon: a narrow indent moves the text right so it clears
    /// the icon's right edge (41 px) by 3 px; a wide indent keeps the original's look.
    #[test]
    fn a_narrow_indent_moves_the_trait_text_past_its_icon() {
        let shift = icon_text_shift(10.0, 1.0);
        assert!(shift > 0.0);
        assert!(12.0 + 10.0 + shift >= 41.0 + 3.0 - 1e-4);
        assert_eq!(icon_text_shift(32.0, 1.0), 0.0);
        assert_eq!(icon_text_shift(60.0, 1.0), 0.0);
        assert!((icon_text_shift(0.0, 2.0) - 64.0).abs() < 1e-4, "scales with the band's base");
    }

    /// Razdor's deviation: a negative vampirism reads «Кровопотеря», a positive one keeps the
    /// original's «Вампиризм» (the install's own text, or the English without it).
    #[test]
    fn a_negative_vampirism_is_blood_loss() {
        assert_eq!(vampirism_label(-10), tr("Blood loss"));
        assert_ne!(vampirism_label(-10), vampirism_label(10));
        assert_eq!(vampirism_label(10), label("SVampirizm", n_("Vampirism")));
        assert_eq!(vampirism_label(0), vampirism_label(10), "0 keeps the vampirism line");
    }

    /// The worn-item slots as 0x4d8638 places them on the original's 262 px fill: 55 px,
    /// 8 px in from the top and sides, each pair touching (55 px apart down).
    #[test]
    fn the_item_slots_sit_as_the_originals() {
        let r = Rect::new(0.0, 0.0, 262.0, 609.0);
        let s = slot_rects_at(r, 1.0);
        assert_eq!((s[0].x, s[0].y, s[0].w, s[0].h), (8.0, 8.0, 55.0, 55.0));
        assert_eq!((s[1].x, s[1].y), (8.0, 63.0));
        assert_eq!((s[2].x, s[2].y), (262.0 - 8.0 - 55.0, 8.0));
        assert_eq!((s[3].x, s[3].y), (199.0, 63.0));
    }

    /// The figure as the original places it (0x492f24 → 0x476a3c): its own size (at the
    /// window's 15/16), the picture centred across the panel and its middle 218 original px
    /// under the panel's top, whatever its height; no longer pinned 10 px under the top.
    #[test]
    fn the_figure_is_centred_in_the_originals_box() {
        let r = Rect::new(10.0, 20.0, 244.0, 570.0);
        let f = figure_rect(r, 160.0, 320.0, 1.0);
        assert_eq!((f.w, f.h), (150.0, 300.0));
        assert_eq!(f.x + f.w / 2.0, r.x + r.w / 2.0);
        assert_eq!(f.y + f.h / 2.0, r.y + 218.0 * 0.9375);
        assert!(f.y - r.y > 50.0, "the head clears the top: {}", f.y - r.y);
        // A shorter figure stands lower; the scale follows the window's.
        let short = figure_rect(r, 100.0, 200.0, 1.0);
        assert_eq!(short.y + short.h / 2.0, f.y + f.h / 2.0);
        let big = figure_rect(Rect::new(0.0, 0.0, 488.0, 1140.0), 160.0, 320.0, 2.0);
        assert_eq!((big.y, big.h), (2.0 * (f.y - r.y), 600.0));
    }

    #[test]
    fn the_first_line_keeps_its_size_when_it_fits_and_shrinks_only_as_needed() {
        // A width growing in rounded steps, as TrueType sizes are drawn.
        let width = |size: f32| (size as u16 & !1) as f32 * 6.0;
        assert_eq!(first_line_size(16.0, 100.0, width), 16.0);
        let s = first_line_size(16.0, 80.0, width);
        assert!(width(s) <= 80.0 && s >= 13.0, "only as small as needed: {s}");
        assert_eq!(first_line_size(16.0, 10.0, width), 7.0);
    }

    /// The first line fits its strip at every interface scale, the case a player's 2880x1620
    /// screen shows ("Pwr: 55" run into "D: 36/39"): the text is 20% too wide at the body
    /// size, and the shrink brings it inside the room at scales 1 to 3 (the room is the card's
    /// width less its margins, 91 px at 1x, so it grows with the scale).
    #[test]
    fn the_first_line_fits_its_room_at_large_scales() {
        for k in [1.0_f32, 1.25, 1.5, 2.0, 2.5, 3.0] {
            let l = StripLayout::of(Rect::new(0.0, 0.0, CARD_W * k, 39.0 * k));
            let room = l.right_end - l.left;
            assert!((room - 91.0 * k).abs() < 1e-3, "room {room} at {k}x");
            let fs = 11.0 * k;
            let width = |size: f32| size * 1.2 * room / fs;
            let s = first_line_size(fs, room, width);
            assert!(s < fs && s >= 7.0, "the first line shrinks at {k}x: {s}");
            assert!(width(s) <= room + 1e-3 || s == 7.0, "fits at {k}x: {} > {room}", width(s));
        }
    }

    fn ranged_defence(content: &Content, back_row: bool, building: i32) -> String {
        let kind = UnitId(content.units[0].id);
        let mut now = Stats::of_level(content, kind, 0);
        now[Stat::DefenceBlow] += building;
        now[Stat::DefenceShot] += building;
        let sheet = Sheet {
            kind,
            name: "",
            named: false,
            level: 0,
            xp: 0,
            need: 0,
            hp: now.max_hp(),
            now: &now,
            start: &now,
            bleed: 0,
            power: 0,
            wage: 0,
            items: [None; MAX_SLOTS],
            back_row,
            unpaid: false,
            building,
            hero: None,
            status: Vec::new(),
        };
        let label = label("SDefenceShot", "Ranged defence");
        stat_lines(content, &sheet).into_iter().find(|l| l.0 == label).map(|l| l.1).unwrap()
    }

    /// The panel adds the back row's `Row2Def` to the ranged defence as "v + n", with the
    /// building's defence when there is one, as the original's card does (0x491fa4).
    #[test]
    fn back_row_shows_row2_def_on_ranged_defence() {
        let content = Content::builtin();
        let base = Stats::of_level(&content, UnitId(content.units[0].id), 0)[Stat::DefenceShot];
        let r2 = content.options.row2_def;
        assert!(r2 > 0);
        assert_eq!(ranged_defence(&content, false, 0), base.to_string());
        assert_eq!(ranged_defence(&content, true, 0), format!("{base} + {r2}"));
        assert_eq!(ranged_defence(&content, true, 3), format!("{base} + {}", 3 + r2));
        assert_eq!(ranged_defence(&content, false, 3), format!("{base} + 3"));
    }

    /// The strip's attack piece as the original's (0x49462c): "A:" with the larger attack for
    /// a fighter or a shooter, "Pwr:" for a caster without melee or outside places 1–4.
    #[test]
    fn the_strip_writes_a_for_shooters_and_pwr_for_casters_off_the_front() {
        let content = Content::builtin();
        let mut s = Stats::of_level(&content, UnitId(content.units[0].id), 0);
        s[Stat::AttackBlow] = 4;
        s[Stat::AttackShot] = 9;
        s[Stat::MagicPower] = 0;
        let piece = |s: &Stats, caster, place| attack_piece(s, s, 7, caster, place).0;
        assert_eq!(piece(&s, false, 8), "A: 9");
        s[Stat::AttackShot] = 4;
        assert_eq!(piece(&s, false, 8), "A: 4");
        assert_eq!(piece(&s, true, 2), "A: 4");
        assert_eq!(piece(&s, true, 0), "Pwr: 7");
        assert_eq!(piece(&s, true, 5), "Pwr: 7");
        assert_eq!(piece(&s, true, 8), "Pwr: 7");
        s[Stat::AttackBlow] = 0;
        assert_eq!(piece(&s, true, 2), "Pwr: 7");
        assert_eq!(piece(&s, false, 2), "A: 4");
        // Colours: any of the three attacks above its start is blue, else any below red.
        let mut start = s.clone();
        start[Stat::AttackShot] = 2;
        assert_eq!(attack_piece(&s, &start, 0, false, 1).1, StripInk::Blue);
        start[Stat::AttackShot] = 6;
        assert_eq!(attack_piece(&s, &start, 0, false, 1).1, StripInk::Red);
    }

    /// The card's "D:" (0x49462c): the army's building defence on both numbers, `Row2Def`
    /// on the ranged one, and no piece at all when both come to 0 or less.
    #[test]
    fn the_card_adds_the_building_and_leaves_out_a_zero_defence() {
        let content = Content::builtin();
        let mut s = Stats::of_level(&content, UnitId(content.units[0].id), 0);
        s[Stat::DefenceBlow] = 0;
        s[Stat::DefenceShot] = 0;
        assert_eq!(defence_piece(&s, 0, 0), None, "a healer's card reads only Pwr:");
        assert_eq!(defence_piece(&s, 0, 20), Some((20, 20)), "a militia in its castle: D: 20/20");
        assert_eq!(defence_piece(&s, 5, 20), Some((20, 25)), "the back row's Row2Def");
        assert_eq!(defence_piece(&s, 5, 0), Some((0, 5)));
        s[Stat::DefenceBlow] = 15;
        s[Stat::DefenceShot] = 10;
        assert_eq!(defence_piece(&s, 0, 20), Some((35, 30)), "a knight in the castle: D: 35/30");
    }

    fn sheet_lines(content: &Content, now: &Stats) -> Vec<String> {
        let kind = UnitId(content.units[0].id);
        let sheet = Sheet {
            kind,
            name: "",
            named: false,
            level: 0,
            xp: 0,
            need: 0,
            hp: now.max_hp(),
            now,
            start: now,
            bleed: 0,
            power: 0,
            wage: 0,
            items: [None; MAX_SLOTS],
            back_row: false,
            unpaid: false,
            building: 0,
            hero: None,
            status: Vec::new(),
        };
        stat_lines(content, &sheet).into_iter().map(|l| l.0).collect()
    }

    /// The panel's list as the original's (491fa4, 491a58): melee attack and defence before
    /// the ranged pair, a line of 0 left out, and the three protections as one
    /// `SProtectAllMagic` line when they are equal and above 0.
    #[test]
    fn the_panel_orders_drops_zeros_and_merges_equal_protections() {
        let content = Content::builtin();
        let mut s = Stats::of_level(&content, UnitId(content.units[0].id), 0);
        s.magic = None;
        s[Stat::MagicPower] = 0;
        s[Stat::AttackBlow] = 40;
        s[Stat::DefenceBlow] = 5;
        s[Stat::AttackShot] = 0;
        s[Stat::DefenceShot] = 0;
        for st in [Stat::ProtectLife, Stat::ProtectElemental, Stat::ProtectDeath, Stat::Regen, Stat::Vampirizm] {
            s[st] = 0;
        }
        let lines = sheet_lines(&content, &s);
        let at = |key: &str, en: &str| lines.iter().position(|l| *l == label(key, en));
        assert!(at("SAttackBlow", "Melee attack").unwrap() < at("SDefenceBlow", "Melee defence").unwrap());
        assert_eq!(at("SDefenceShot", "Ranged defence"), None, "a ranged defence of 0 is left out");
        assert_eq!(at("SAttackShot", "Ranged attack"), None);
        for st in [Stat::ProtectLife, Stat::ProtectElemental, Stat::ProtectDeath] {
            s[st] = 15;
        }
        let lines = sheet_lines(&content, &s);
        assert!(lines.contains(&label("SProtectAllMagic", "Immunity to magic")));
        assert!(!lines.contains(&label("SProtectLife", "Life magic protection")));
        s[Stat::ProtectDeath] = 20;
        let lines = sheet_lines(&content, &s);
        assert!(!lines.contains(&label("SProtectAllMagic", "Immunity to magic")));
        assert!(lines.contains(&label("SProtectDeath", "Death magic protection")));
    }

    /// The original's state lines (492f24): death shuts out the rest, then no payment, else
    /// the back row before the building.
    #[test]
    fn a_dead_or_unpaid_unit_shows_only_its_state() {
        use StateLine::*;
        assert_eq!(state_lines(true, true, true, true), vec![Resurrect]);
        assert_eq!(state_lines(false, true, true, true), vec![NoPayment]);
        assert_eq!(state_lines(false, false, true, true), vec![BackRow, InCastle]);
        assert_eq!(state_lines(false, false, false, true), vec![InCastle]);
        assert!(state_lines(false, false, false, false).is_empty());
        assert_eq!(state_trait(NoPayment).1, "", "the original never sets the text");
    }

    /// A bonus text's name part (499eb0): up to the first " - ", trimmed; the whole text
    /// when it has none.
    #[test]
    fn a_bonus_title_is_the_text_before_its_dash() {
        assert_eq!(title_of("Name One - what it does - more"), "Name One");
        assert_eq!(title_of("  Name  "), "Name");
        assert_eq!(title_of("Name-Two - x"), "Name-Two");
    }

    /// Every bonus is named from the install's `[Army] Bonus<N>`, as the original's item text
    /// (skipped without `RAZDOR_DT_DIR`).
    #[test]
    fn bonus_titles_come_from_the_install() {
        let Ok(install) = razdor::dt::install::DtInstall::from_env() else { return };
        chrome::set_install(install.dir.clone());
        for b in Bonus::known() {
            let n = bonus_number(b).unwrap();
            let Some(text) = chrome::ui_text("Army", &format!("Bonus{n}")) else { continue };
            assert_eq!(bonus_title(b), title_of(&text), "Bonus{n}");
            assert!(!bonus_title(b).is_empty(), "Bonus{n}");
        }
    }

    /// The original's places: the front line 0–5, the back line 6–11, as drawn.
    #[test]
    fn strip_places_follow_the_drawn_lines() {
        use razdor::rules::formation::Row;
        let (wide, short) = (Formation::WIDE, Formation::VANILLA);
        assert_eq!(strip_place(wide, Slot::new(Row::Front, 0)), 0);
        assert_eq!(strip_place(wide, Slot::new(Row::Front, 5)), 5);
        assert_eq!(strip_place(wide, Slot::new(Row::Back, 1)), 7);
        assert_eq!(strip_place(short, Slot::new(Row::Front, 0)), 1);
        assert_eq!(strip_place(short, Slot::new(Row::Front, 3)), 4);
        assert_eq!(strip_place(short, Slot::new(Row::Back, 0)), 7);
    }

    /// The strip's pieces stand where the original's do (0x494f93–0x49565f): cell tops 2, 14
    /// and 26 px under the portrait, the left pieces 2 px in, the right ones ending at
    /// x + 94 − 1, the hits centred with the odd pixel dropped; scaled with the card.
    #[test]
    fn the_strip_is_laid_out_as_the_originals() {
        let l = StripLayout::of(Rect::new(10.0, 100.0, 94.0, 39.0));
        assert_eq!(l.tops, [102.0, 114.0, 126.0]);
        assert_eq!((l.left, l.right_end), (12.0, 103.0));
        // A caster's card and a warrior's are the same frame: the same places, whatever the
        // strip's text (0x494f93: the first line keeps its cell under a long "Pwr: 28 D: 0/10").
        let caster = StripLayout::of(Rect::new(0.0, 94.0, 94.0, 40.0));
        let warrior = StripLayout::of(Rect::new(0.0, 94.0, 94.0, 40.0));
        assert_eq!(caster.tops, [96.0, 108.0, 120.0]);
        assert_eq!(caster.tops, warrior.tops);
        assert_eq!(caster.baseline(0, 5.0), 101.0, "the first line's place ignores its size");
        assert_eq!(caster.baseline(2, 5.0), 125.0);
        assert_eq!(l.centred(40.0), 37.0);
        assert_eq!(l.centred(41.0), 36.0);
        let big = StripLayout::of(Rect::new(0.0, 0.0, 188.0, 78.0));
        assert_eq!(big.tops, [4.0, 28.0, 52.0]);
        assert_eq!((big.left, big.right_end), (4.0, 186.0));
    }

    /// The strip's tints are the original's SanSerif fonts (white less the colour given at
    /// 0x4dbc26), and each piece takes its own (0x49462c): the manoeuvres and the initiative
    /// never blue; the defence judged by the ranged defence when the back row adds to it.
    #[test]
    fn the_strip_pieces_take_the_originals_tints() {
        assert_eq!(StripInk::White.rgb(), [255, 255, 255]);
        assert_eq!(StripInk::Yellow.rgb(), [255 - 25, 255 - 25, 255 - 125]);
        assert_eq!(StripInk::Orange.rgb(), [255 - 20, 255 - 90, 255 - 180]);
        assert_eq!(StripInk::Red.rgb(), [255, 255 - 152, 255 - 159]);
        assert_eq!(StripInk::Blue.rgb(), [255 - 180, 255 - 70, 255]);
        assert_eq!(strip_ink(2, 1, StripInk::White, false), StripInk::White);
        assert_eq!(strip_ink(0, 1, StripInk::White, false), StripInk::Red);
        assert_eq!(strip_ink(14, 14, StripInk::Yellow, false), StripInk::Yellow);
        let content = Content::builtin();
        let base = Stats::of_level(&content, UnitId(content.units[0].id), 0);
        let mut now = base.clone();
        assert_eq!(defence_ink(&now, &base, 0), StripInk::Yellow);
        now[Stat::DefenceShot] += 3;
        assert_eq!(defence_ink(&now, &base, 0), StripInk::Yellow, "the melee defence decides outside the back row");
        assert_eq!(defence_ink(&now, &base, 5), StripInk::Blue);
        now[Stat::DefenceBlow] -= 1;
        assert_eq!(defence_ink(&now, &base, 0), StripInk::Red);
    }

    /// The strip pictures as the original makes them from Win-paper (0x4de0f0–0x4de1f7): on
    /// a paper of (230, 178, 115) a dark brown, the hero's deep red-brown, a named
    /// character's deep blue, each 94×39, the named one cut from x = 220.
    #[test]
    fn the_strip_pictures_are_made_from_the_paper_as_the_originals() {
        let (w, h) = (320u32, 80u32);
        let mut rgba = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                // From x = 220, where only the named strip reads, a paper a little less green.
                rgba.extend_from_slice(&if x < 220 { [230, 178, 115, 255] } else { [230, 170, 115, 255] });
            }
        }
        let paper = razdor::dt::gfx::Image { width: w, height: h, rgba };
        let px = |panel| {
            let img = chrome::strip_picture(&paper, panel);
            assert_eq!((img.width, img.height), (94, 39));
            let p = img.pixel(50, 20);
            [p[0], p[1], p[2]]
        };
        assert_eq!(px(StripPanel::Plain), [82, 44, 0]);
        assert_eq!(px(StripPanel::Hero), [107, 32, 0]);
        // Red and blue swapped, then green and blue scaled: from the paper at x = 220.
        assert_eq!(px(StripPanel::Named), [0, 32, 115]);
    }

    /// The description list as 0x492f24 fills it: the loaders' six spaces before the
    /// description and each trait (0x48f820, 0x4e4e40), an empty row between entries
    /// (0x492e70 → 0x47e944), the icon by a trait's first row; the no-payment line's text is
    /// empty and gets no spaces.
    #[test]
    fn the_description_list_indents_and_spaces_as_the_originals() {
        let width = |_c: char| 1.0;
        let traits = vec![
            ("Bonus15".to_string(), "Garrison text".to_string(), RED_TEXT),
            ("Bonus-NoPayment".to_string(), String::new(), BLUE_TEXT),
        ];
        let rows = list_rows(Some("Knows every corner"), &traits, 14.0, width);
        let texts: Vec<&str> = rows.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(texts, ["      Knows", "every corner", "", "      Garrison", "text", "", ""]);
        assert_eq!(rows[3].icon.as_deref(), Some("Bonus15"));
        assert_eq!(rows[4].icon, None);
        assert_eq!(rows[6].icon.as_deref(), Some("Bonus-NoPayment"));
        // A hero has no description: its first trait opens the list without an empty row.
        let hero = list_rows(None, &traits[..1], 100.0, width);
        assert_eq!(hero[0].text, "      Garrison text");
    }

    /// The blocks stack up from the panel's bottom as the original's (0x492f24, the lists at
    /// 0x4d9060): the description rows·14 + 12, the stats (n+1)·14 + 8 over it (8 more for a
    /// hero), the name 34 over those.
    #[test]
    fn the_panel_stacks_its_text_as_the_originals() {
        assert_eq!(panel_tops(4, 8, false), (68.0, 68.0 + 134.0, 68.0 + 134.0 + 34.0));
        assert_eq!(panel_tops(4, 8, true), (68.0, 68.0 + 142.0, 68.0 + 142.0 + 34.0));
    }

    /// A sheet of `kind` at `level` for the colour and value tests: `now` is what it shows,
    /// `start` what it is compared with (the same stats outside battle).
    fn panel<'a>(kind: UnitId, level: i32, now: &'a Stats, start: &'a Stats, hp: i32, building: i32) -> Sheet<'a> {
        Sheet {
            kind,
            name: "",
            named: false,
            level,
            xp: 0,
            need: 0,
            hp,
            now,
            start,
            bleed: 0,
            power: 0,
            wage: 0,
            items: [None; MAX_SLOTS],
            back_row: false,
            unpaid: false,
            building,
            hero: None,
            status: Vec::new(),
        }
    }

    /// With many abilities (simultaneous bonuses) the description keeps every row and only the
    /// ability rows are limited to the region; classic lists show all rows.
    #[test]
    fn the_description_is_never_scrolled_out_by_the_abilities() {
        assert_eq!(shown_rows(5, 40, true), (5, ABILITY_ROWS));
        assert_eq!(shown_rows(5, 3, true), (5, 3));
        assert_eq!(shown_rows(5, 40, false), (5, 40));
        assert_eq!(shown_rows(0, 0, true), (0, 0));
    }

    /// Six cells: the third pair (cells 4 and 5) sits under the first two, touching them, and
    /// the classic four are where they were.
    #[test]
    fn the_third_pair_of_cells_sits_under_the_first_two() {
        let s = slot_rects_at(Rect::new(0.0, 0.0, 262.0, 609.0), 1.0);
        assert_eq!((s[4].x, s[4].y, s[4].w), (8.0, 118.0, 55.0));
        assert_eq!((s[5].x, s[5].y), (199.0, 118.0));
        assert_eq!((s[3].x, s[3].y), (199.0, 63.0));
    }


    fn line_of(content: &Content, sheet: &Sheet, key: &str, en: &str) -> Line {
        stat_lines(content, sheet).into_iter().find(|l| l.0 == label(key, en)).expect("the stat's line")
    }

    /// The colour rule (491a58): plain at the level value, blue above it, red below it or
    /// below the current value, red winning over blue.
    #[test]
    fn a_shown_value_is_blue_above_its_level_and_red_below_it_or_its_current() {
        assert_eq!(shown_ink(20, 20, 20), CREAM);
        assert_eq!(shown_ink(25, 20, 25), BLUE_TEXT);
        assert_eq!(shown_ink(15, 20, 15), RED_TEXT);
        assert_eq!(shown_ink(25, 20, 30), RED_TEXT, "below the current value: red wins over blue");
        assert_eq!(shown_ink(20, 20, 12), CREAM, "above the current value only: plain at the level");
    }

    /// A worn sword's +3 melee attack is the shown total (not "base + 3"), in blue for the
    /// label and the value alike (0x491a58); unworn, the base alone and plain.
    #[test]
    fn an_item_raising_attack_shows_the_total_in_blue() {
        let content = Content::builtin();
        let kind = UnitId(content.units[0].id);
        let sword = content.item_by_key("short_sword").expect("the demo sword");
        let mut u = razdor::rules::units::Unit::new(&content, kind, Slot::new(razdor::rules::formation::Row::Front, 0));
        let base = Stats::of_level(&content, kind, u.level)[Stat::AttackBlow];
        let attack = |u: &razdor::rules::units::Unit| {
            let stats = u.stats(&content);
            let sheet = panel(kind, u.level, &stats, &stats, u.hp, 0);
            line_of(&content, &sheet, "SAttackBlow", "Melee attack")
        };
        assert_eq!(attack(&u), (label("SAttackBlow", "Melee attack"), base.to_string(), CREAM));
        u.items[0] = Some(sword);
        assert_eq!(attack(&u), (label("SAttackBlow", "Melee attack"), (base + 3).to_string(), BLUE_TEXT));
    }

    /// A wounded unit's Hits are red (its current HP is compared with the max HP); unhurt,
    /// the max HP alone.
    #[test]
    fn a_wounded_unit_shows_its_hits_in_red() {
        let content = Content::builtin();
        let kind = UnitId(content.units[0].id);
        let stats = Stats::of_level(&content, kind, 0);
        let max = stats.max_hp();
        let hits = |hp: i32| {
            let sheet = panel(kind, 0, &stats, &stats, hp, 0);
            line_of(&content, &sheet, "SHit", "Hits")
        };
        assert_eq!(hits(max), (label("SHit", "Hits"), max.to_string(), CREAM));
        assert_eq!(hits(max - 3), (label("SHit", "Hits"), (max - 3).to_string(), RED_TEXT));
    }

    /// In battle a cursed attack (now below its battle value) is red, a blessed one blue.
    #[test]
    fn a_cursed_stat_in_battle_is_red() {
        let content = Content::builtin();
        let kind = UnitId(content.units[0].id);
        let start = Stats::of_level(&content, kind, 0);
        let base = start[Stat::AttackBlow];
        let mut cursed = start.clone();
        cursed[Stat::AttackBlow] = base - 4;
        let sheet = panel(kind, 0, &cursed, &start, cursed.max_hp(), 0);
        assert_eq!(line_of(&content, &sheet, "SAttackBlow", "Melee attack"), (label("SAttackBlow", "Melee attack"), (base - 4).to_string(), RED_TEXT));
        let mut blessed = start.clone();
        blessed[Stat::AttackBlow] = base + 4;
        let sheet = panel(kind, 0, &blessed, &start, blessed.max_hp(), 0);
        assert_eq!(line_of(&content, &sheet, "SAttackBlow", "Melee attack").2, BLUE_TEXT);
    }

    /// In a building the defence is written "v + n" (n the building's defence), and only v is
    /// coloured: plain at the level value, red when v is cursed below its current value.
    #[test]
    fn a_building_defence_is_written_v_plus_n_and_coloured_by_v() {
        let content = Content::builtin();
        let kind = UnitId(content.units[0].id);
        let naked = Stats::of_level(&content, kind, 0);
        let v = naked[Stat::DefenceBlow];
        let mut now = naked.clone();
        now[Stat::DefenceBlow] = v + 20;
        now[Stat::DefenceShot] = naked[Stat::DefenceShot] + 20;
        // The current values carry the building too (the army window, and the battle view).
        let sheet = panel(kind, 0, &now, &now, now.max_hp(), 20);
        assert_eq!(line_of(&content, &sheet, "SDefenceBlow", "Melee defence"), (label("SDefenceBlow", "Melee defence"), format!("{v} + 20"), CREAM));
        let mut cursed = now.clone();
        cursed[Stat::DefenceBlow] = v + 20 - 4;
        let sheet = panel(kind, 0, &cursed, &now, cursed.max_hp(), 20);
        assert_eq!(line_of(&content, &sheet, "SDefenceBlow", "Melee defence"), (label("SDefenceBlow", "Melee defence"), format!("{} + 20", v - 4), RED_TEXT));
    }

}
