//! The original's unit panel (battle and army screens): the unit's full-body sepia figure on
//! parchment, worn items in the top corners, its name, the long stat list and its traits
//! with their icons (video notes §3, refs 09 and 11).

use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::{bless_effect, curse_effect, Buff};
use razdor::rules::content::{Bonus, Content, HeroClass, ItemId, MagicDirection, MagicSchool, Stat, UnitId};
use razdor::rules::formation::{Formation, Slot};
use razdor::rules::units::Stats;

use super::assets::Assets;
use super::chrome::{self, k, shadow_centered, shadow_right, shadow_text, BLUE_TEXT, CREAM, ORANGE_TEXT, RED_TEXT};
use super::widgets::{measure, mouse_in, wrap};

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
    /// Stats with items, as the battle began.
    pub start: &'a Stats,
    /// Current magic power (it drains during a battle).
    pub power: i32,
    pub wage: i32,
    pub items: [Option<ItemId>; 4],
    pub back_row: bool,
    /// The defence the building adds to both defences (0 outside one); `now` includes it.
    pub building: i32,
    pub hero: Option<HeroClass>,
    /// Status lines under the name (actions left, poison, this turn's modifiers).
    pub status: Vec<(String, Color)>,
    /// The battle's panel: the name and stats run over the figure's lower part. Otherwise
    /// (the army screen) they stand under the full figure.
    pub battle: bool,
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

fn trait_line(b: &Bonus) -> (String, String) {
    match bonus_number(b) {
        Some(n) => (format!("Bonus{n}"), chrome::ui_text("Army", &format!("Bonus{n}")).unwrap_or_else(|| bonus_english(b))),
        None => ("Bonus1".to_string(), bonus_english(b)),
    }
}

fn hero_trait(h: HeroClass) -> (String, String) {
    let n = HeroClass::ALL.iter().position(|&c| c == h).unwrap_or(0) + 1;
    let english = match h {
        HeroClass::Knight => tr("The army of this hero takes 10% less damage from enemy attacks (magic excepted)."),
        HeroClass::Archmage => tr("The Archmage casts spells twice as fast for 50% less mana, but his army gets no bonuses."),
        HeroClass::Ranger => tr("The army of this hero travels 20% faster, and the wounded heal 20% of their hits every day."),
    };
    (format!("HeroBonus{n}"), chrome::ui_text("NewHero", &format!("Bonus{n}")).unwrap_or_else(|| english.to_string()))
}

/// The traits the panel lists: the hero's class, the unit's bonuses (its items' too), the
/// back row's and the building's.
fn traits_of(s: &Sheet) -> Vec<(String, String)> {
    let mut traits: Vec<(String, String)> = s.start.bonuses.iter().map(trait_line).collect();
    if let Some(h) = s.hero {
        traits.insert(0, hero_trait(h));
    }
    if s.back_row {
        traits.push(("Bonus-2Row".into(), chrome::ui_text("Army", "Hint1").unwrap_or_else(|| tr("In the second row the unit gets a bonus to its ranged defence!").into())));
    }
    if s.building > 0 {
        traits.push(("Bonus-InCastle".into(), chrome::ui_text("Army", "Hint2").unwrap_or_else(|| tr("In its own building the unit gets a bonus to all defences!").into())));
    }
    traits
}

/// The height a trait of `lines` wrapped lines takes in the list (as `draw` lays it out).
fn trait_height(lines: usize, slh: f32, k: f32) -> f32 {
    5.0 * k + slh * lines as f32 + if lines == 1 { slh * 0.6 } else { 0.0 }
}

/// A stat line: label, value, colour.
type Line = (String, String, Color);

/// A stat's name: the install's own (`[Skills] <key>`) in Russian, else ours.
fn label(key: &str, english: &str) -> String {
    let own = (razdor::i18n::lang() == razdor::i18n::Lang::Ru).then(|| chrome::ui_text("Skills", key)).flatten();
    own.unwrap_or_else(|| tr(english).to_string())
}

fn signed(v: i32) -> String {
    if v > 0 {
        format!("+{v}")
    } else {
        v.to_string()
    }
}

/// Colour of a value against its start-of-battle value: blue raised, red lowered.
fn cmp_color(now: i32, start: i32) -> Color {
    match now.cmp(&start) {
        std::cmp::Ordering::Greater => BLUE_TEXT,
        std::cmp::Ordering::Less => RED_TEXT,
        std::cmp::Ordering::Equal => CREAM,
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

/// The stat list of `s`, as the original's panel orders it.
fn stat_lines(content: &Content, s: &Sheet) -> Vec<Line> {
    let (now, start) = (s.now, s.start);
    let naked = Stats::of_level(content, s.kind, s.level);
    let mut lines: Vec<Line> = Vec::new();
    let max = now.max_hp();
    let hits = if s.hp < max { format!("{} / {max}", s.hp.max(0)) } else { max.to_string() };
    lines.push((label("SHit", n_("Hits")), hits, if s.hp < max { RED_TEXT } else { cmp_color(max, start.max_hp()) }));
    // "base + bonus" for what items and traits add, as the original writes it.
    let split = |st: Stat| -> (String, Color) {
        let (n, b) = (naked[st], start[st] - naked[st]);
        let v = if b != 0 && now[st] == start[st] { format!("{n} {} {}", if b > 0 { "+" } else { "-" }, b.abs()) } else { now[st].to_string() };
        (v, cmp_color(now[st], start[st]))
    };
    if start[Stat::AttackBlow] > 0 || now[Stat::AttackBlow] > 0 {
        let (v, c) = split(Stat::AttackBlow);
        lines.push((label("SAttackBlow", n_("Melee attack")), v, c));
    }
    if start[Stat::AttackShot] > 0 || now[Stat::AttackShot] > 0 {
        let (v, c) = split(Stat::AttackShot);
        lines.push((label("SAttackShot", n_("Ranged attack")), v, c));
    }
    // In a building its defence is written apart, "15 + 12", as the original does; the
    // back row's `Row2Def` joins the building's on the ranged defence (0x491fa4).
    let defence = |st: Stat| -> (String, Color) {
        let extra = s.building + if st == Stat::DefenceShot { row2_extra(content, s) } else { 0 };
        if extra > 0 {
            (format!("{} + {}", now[st] - s.building, extra), cmp_color(now[st], start[st]))
        } else {
            split(st)
        }
    };
    let (v, c) = defence(Stat::DefenceBlow);
    lines.push((label("SDefenceBlow", n_("Melee defence")), v, c));
    let (v, c) = defence(Stat::DefenceShot);
    lines.push((label("SDefenceShot", n_("Ranged defence")), v, c));
    if now.is_mage() || start.is_mage() {
        let school = now.magic.unwrap_or(MagicSchool::Elemental);
        let p = s.power;
        let o = &content.options;
        let pc = cmp_color(p, start[Stat::MagicPower]);
        let dir = now.magic_direction();
        if matches!(dir, MagicDirection::ToEnemy | MagicDirection::ToAll) {
            lines.push((label("StrikeHit", n_("Magic strike (- hits)")), format!("-{p}"), pc));
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
    for (label, st) in [
        (label("SProtectLife", n_("Life magic protection")), Stat::ProtectLife),
        (label("SProtectElemental", n_("Elemental magic protection")), Stat::ProtectElemental),
        (label("SProtectDeath", n_("Death magic protection")), Stat::ProtectDeath),
        (label("SRegen", n_("Regeneration")), Stat::Regen),
        (label("SVampirizm", n_("Vampirism")), Stat::Vampirizm),
    ] {
        if now[st] != 0 || start[st] != 0 {
            lines.push((label, format!("{}%", now[st]), cmp_color(now[st], start[st])));
        }
    }
    lines.push((label("SInitiative", n_("Initiative")), now[Stat::Initiative].to_string(), cmp_color(now[Stat::Initiative], start[Stat::Initiative])));
    lines.push((label("SManevres", n_("Actions")), now[Stat::Manevres].to_string(), cmp_color(now[Stat::Manevres], start[Stat::Manevres])));
    if s.wage > 0 {
        lines.push((label("DailyPayment", n_("Daily wage")), s.wage.to_string(), ORANGE_TEXT));
    }
    lines
}

/// The stat strip's text.
/// A named character's name on his card (the original's ae24a0 font, gold).
const NAMED_INK: Color = Color::new(1.0, 0.86, 0.35, 1.0);
const STRIP_INK: Color = Color::new(0.98, 0.92, 0.72, 1.0);
const HITS_INK: Color = Color::new(1.0, 0.78, 0.35, 1.0);

/// Blue for a stat above its start value, red below.
fn strip_color(cur: i32, base: i32) -> Color {
    match cur.cmp(&base) {
        std::cmp::Ordering::Greater => BLUE_TEXT,
        std::cmp::Ordering::Less => RED_TEXT,
        std::cmp::Ordering::Equal => STRIP_INK,
    }
}

/// The card's attack piece (0x49462c): "Pwr:" with the magic power for a caster (attack kind
/// 0x11) whose melee attack is 0 or that stands outside the original's places 1–4 (see
/// [`strip_place`]); for anyone else "A:" with the ranged attack when it is above the melee
/// attack, else the melee attack. Blue when any of the three attacks is above its value in
/// `base`, else red when any is below.
fn attack_piece(s: &Stats, base: &Stats, power: i32, caster: bool, place: usize) -> (String, Color) {
    let (ab, sh) = (s[Stat::AttackBlow], s[Stat::AttackShot]);
    let now = [ab, sh, power];
    let was = [base[Stat::AttackBlow], base[Stat::AttackShot], base[Stat::MagicPower]];
    let color = if now.iter().zip(was).any(|(n, w)| *n > w) {
        BLUE_TEXT
    } else if now.iter().zip(was).any(|(n, w)| *n < w) {
        RED_TEXT
    } else {
        STRIP_INK
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

/// The strip under a card's portrait, as the original's: "A: 45  D: 35/40", "Mnvr: 1
/// Ini: 12", "Hits: 70" (or "Hits: 45/70"). `row2` is added to the ranged defence shown
/// (the back row's `Row2Def`, 0 elsewhere: 0x49462c). `lit` reddens it (the unit acting, or
/// the one selected on the army screen). `caster` and `place` choose the attack piece
/// ([`caster`], [`strip_place`]).
#[allow(clippy::too_many_arguments)]
pub fn stat_strip(strip: Rect, now: &Stats, base: &Stats, power: i32, caster: bool, place: usize, hp: i32, row2: i32, lit: bool, panel: StripPanel) {
    let k = k();
    chrome::surface(strip, chrome::Skin::Strip);
    // The original recolours the strip itself (0x48da5c): red for the hero, blue for a named
    // character; a tint over it here.
    match panel {
        StripPanel::Plain => {}
        StripPanel::Hero => draw_rectangle(strip.x, strip.y, strip.w, strip.h, Color::new(0.85, 0.08, 0.02, 0.55)),
        StripPanel::Named => draw_rectangle(strip.x, strip.y, strip.w, strip.h, Color::new(0.05, 0.3, 0.95, 0.6)),
    }
    if lit {
        draw_rectangle(strip.x, strip.y, strip.w, strip.h, Color::new(0.75, 0.25, 0.0, 0.4));
    }
    draw_rectangle_lines(strip.x, strip.y, strip.w, strip.h, 1.0, Color::new(0.0, 0.0, 0.0, 0.5));
    let fs = (strip.h * 0.30).round().max(9.0);
    let lh = strip.h / 3.0;
    let (x0, x1) = (strip.x + 3.0 * k, strip.x + strip.w - 3.0 * k);
    let (att, ac) = attack_piece(now, base, power, caster, place);
    shadow_text(&att, x0, strip.y + lh - 2.0 * k, fs, ac);
    let d = razdor::trf!("D: {blow}/{shot}", blow = now[Stat::DefenceBlow], shot = now[Stat::DefenceShot] + row2);
    let dc = strip_color(now[Stat::DefenceBlow] + now[Stat::DefenceShot], base[Stat::DefenceBlow] + base[Stat::DefenceShot]);
    shadow_right(&d, x1, strip.y + lh - 2.0 * k, fs, dc);
    let (mn, ini) = (now[Stat::Manevres], now[Stat::Initiative]);
    shadow_text(&razdor::trf!("Mnvr: {mn}", mn), x0, strip.y + 2.0 * lh - 2.0 * k, fs, strip_color(mn, base[Stat::Manevres]));
    shadow_right(&razdor::trf!("Ini: {ini}", ini), x1, strip.y + 2.0 * lh - 2.0 * k, fs, strip_color(ini, base[Stat::Initiative]));
    let max = now.max_hp();
    let hits = if hp < max { razdor::trf!("Hits: {hp}/{max}", hp = hp.max(0), max) } else { razdor::trf!("Hits: {max}", max) };
    shadow_centered(&hits, strip.x + strip.w / 2.0, strip.y + 3.0 * lh - 2.5 * k, fs, if hp < max { Color::new(1.0, 0.6, 0.4, 1.0) } else { HITS_INK });
}

/// Rects of the four item slots around the figure: two at the top left, two at the top
/// right (as in the army screen).
pub fn slot_rects(r: Rect) -> [Rect; 4] {
    let s = 44.0 * k();
    let m = 8.0 * k();
    [
        Rect::new(r.x + m, r.y + m, s, s),
        Rect::new(r.x + m, r.y + m + s + 6.0 * k(), s, s),
        Rect::new(r.x + r.w - m - s, r.y + m, s, s),
        Rect::new(r.x + r.w - m - s, r.y + m + s + 6.0 * k(), s, s),
    ]
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

/// Draws the panel in `r`. `slots` shows the four item slots (empty ones too) and returns
/// the one clicked; the item under the mouse goes to `hover`.
pub fn draw(assets: &Assets, content: &Content, r: Rect, s: &Sheet, slots: bool, hover: &mut Option<ItemId>) -> Option<usize> {
    let k = k();
    chrome::parchment(r, true);
    // The figure, behind the text.
    let fig_h = r.h * if s.battle { 0.66 } else { 0.6 };
    let top = r.y + 10.0 * k;
    let fig_bottom = match assets.figure(s.kind) {
        Some(t) => {
            let h = (t.height() * 0.9375 * k).min(fig_h);
            let w = t.width() * h / t.height();
            chrome::tex(&t, Rect::new(r.x + (r.w - w) / 2.0, top, w, h), Color::new(1.0, 1.0, 1.0, 0.92));
            top + h
        }
        None => {
            silhouette(Rect::new(r.x + r.w * 0.2, r.y + 12.0 * k, r.w * 0.6, fig_h * 0.95));
            top + fig_h * 0.95
        }
    };
    // Below the figure's shoulders the parchment turns dark brown (the video's panel: light
    // gold down to about a quarter, then (115, 62, 0)); the figure's lower part shows through.
    // In battle the name and the stats run over the figure's lower part; on the army screen
    // they stand under the figure.
    let natural_y = if s.battle { r.y + r.h * 0.375 } else { fig_bottom + 6.0 * k };
    // The traits always get their room: when the name, the stats and the traits do not fit
    // under the figure, the text starts higher, over the figure (a caster's long stat list
    // with a trait, a mod's long trait texts).
    let traits = traits_of(s);
    let (small, slh, icon) = ((12.0 * k).round(), 13.2 * k, 24.0 * k);
    let trait_w = r.w - 44.0 * k - icon;
    let traits_h: f32 = traits.iter().map(|(_, line)| trait_height(wrap(line, trait_w, small).len(), slh, k)).sum();
    let lh = 13.6 * k;
    let text_h = 17.0 * k + lh * (1 + stat_lines(content, s).len() + s.status.len()) as f32 + 6.0 * k + traits_h;
    let bottom = r.y + r.h - 6.0 * k;
    let name_y = natural_y.min(bottom - text_h).max(r.y + 0.25 * r.h);
    // From 50 px above the name the parchment turns dark brown (both screens of the video:
    // light gold, then (115, 62, 0)); the figure shows through.
    let (fade0, fade1) = (name_y - 50.0 * k, name_y - 15.0 * k);
    // Multiplying (230, 178, 115) parchment by this gives the video's (115, 62, 0).
    let dark = |t: f32| Color::new(1.0 - 0.5 * t, 1.0 - 0.65 * t, 1.0 - t, 1.0);
    chrome::multiply(|| {
        let steps = 12;
        for i in 0..steps {
            let t = (i as f32 + 0.5) / steps as f32;
            let y0 = fade0 + (fade1 - fade0) * i as f32 / steps as f32;
            draw_rectangle(r.x, y0, r.w, (fade1 - fade0) / steps as f32 + 0.5, dark(t));
        }
        draw_rectangle(r.x, fade1, r.w, r.y + r.h - fade1, dark(1.0));
    });
    // Items worn.
    let mut clicked = None;
    for (i, sr) in slot_rects(r).iter().enumerate() {
        let item = s.items[i];
        if !slots && item.is_none() {
            continue;
        }
        if slots {
            draw_rectangle(sr.x, sr.y, sr.w, sr.h, Color::new(0.2, 0.12, 0.05, 0.35));
            draw_rectangle_lines(sr.x, sr.y, sr.w, sr.h, 1.0, Color::new(0.45, 0.3, 0.15, 0.8));
        }
        if let Some(it) = item {
            assets.draw_item(it, sr.x, sr.y, sr.w);
        }
        if mouse_in(sr.x, sr.y, sr.w, sr.h) {
            if item.is_some() {
                *hover = item;
            }
            if super::widgets::clicked() {
                clicked = Some(i);
            }
        }
    }
    let x0 = r.x + 24.0 * k;
    let x1 = r.x + r.w - 16.0 * k;
    let name_size = (17.0 * k).round();
    let mut y = name_y;
    let name_ink = if s.named { NAMED_INK } else { CREAM };
    super::dt_font::with_face(super::dt_font::Face::Title, || shadow_centered(s.name, r.x + r.w / 2.0, y, name_size, name_ink));
    y += 17.0 * k;
    let size = (12.0 * k).round();
    // Level and experience on one line.
    shadow_text(&razdor::trf!("Level {level}", level = s.level), x0, y, size, CREAM);
    shadow_right(&razdor::trf!("XP {xp} / {need}", xp = s.xp, need = s.need), x1, y, size, CREAM);
    y += lh;
    for (label, value, color) in stat_lines(content, s) {
        let c = if color == CREAM { CREAM } else { color };
        let room = x1 - x0 - measure(&value, size).width - 6.0 * k;
        shadow_text(&label, x0, y, super::widgets::fit_size(&label, room, size), c);
        shadow_right(&value, x1, y, size, c);
        y += lh;
    }
    for (line, color) in &s.status {
        shadow_text(line, x0, y, super::widgets::fit_size(line, x1 - x0, size), *color);
        y += lh;
    }
    // The description in what the traits leave, then the traits with their icons.
    y += 6.0 * k;
    // Any unit but the hero shows its class's description (0x492f24: the type's text, the
    // `Descript` of Rus_Units.ini), in battle and in the army and building windows alike.
    let desc = if s.hero.is_none() { content.unit(s.kind).description.as_str() } else { "" };
    let desc_bottom = bottom - traits_h;
    let desc_lines = wrap(desc, r.w - 44.0 * k, small);
    let fits = ((desc_bottom - y) / slh).floor().max(0.0) as usize;
    for (i, line) in desc_lines.iter().take(fits).enumerate() {
        // A description cut short ends in an ellipsis.
        let line = if i + 1 == fits && fits < desc_lines.len() { format!("{}…", line.trim_end()) } else { line.clone() };
        shadow_text(&line, x0 - 4.0 * k, y, small, CREAM);
        y += slh;
    }
    for (art, line) in traits {
        y += 5.0 * k;
        if y + slh > bottom + slh * 0.5 {
            break;
        }
        chrome::trait_icon(&art, x0 - 8.0 * k, y - slh + 2.0 * k, icon);
        let lines = wrap(&line, trait_w, small);
        for (i, l) in lines.iter().enumerate() {
            if y > bottom {
                break;
            }
            let lx = if i < 2 { x0 + icon - 4.0 * k } else { x0 - 8.0 * k };
            shadow_text(l, lx, y, small, BLUE_TEXT);
            y += slh;
        }
        if lines.len() == 1 {
            y += slh * 0.6;
        }
    }
    let _ = measure;
    clicked
}

#[cfg(test)]
mod tests {
    use super::*;

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
            power: 0,
            wage: 0,
            items: [None; 4],
            back_row,
            building,
            hero: None,
            status: Vec::new(),
            battle: true,
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
        assert_eq!(attack_piece(&s, &start, 0, false, 1).1, BLUE_TEXT);
        start[Stat::AttackShot] = 6;
        assert_eq!(attack_piece(&s, &start, 0, false, 1).1, RED_TEXT);
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
}
