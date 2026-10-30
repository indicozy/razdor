//! The hero and army screen: gear, backpack, promotion.
use razdor::local;
use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::rules::content::{Content, ItemId, Stat, UnitId};
use razdor::rules::experience::is_percent_stat;
use razdor::rules::game::Game;
use razdor::rules::items::{EquipError, Given, Press};
use razdor::rules::journal::Tab;
use razdor::rules::pack_order::{self, ItemGroup, PackSort};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::building_view::{service_error, BuildingView};
use super::screens::attack_line;
use super::unit_drag::{grid_press_in, Controls, GridPress};
use super::chrome;
use super::item_filter;
use super::keymap::{self, Action};
use super::unit_sheet;
use super::widgets::*;
use super::window_layout;
use super::Screen;


/// What a class gains per level: "+5 hits, +2 melee, +5% life prot.". Percent stats close
/// that share of the gap to 100.
pub(super) fn level_gains(content: &Content, kind: UnitId) -> String {
    let def = content.unit(kind);
    let label = |st: Stat| match st {
        Stat::Hits => tr("hits"),
        Stat::AttackBlow => tr("melee"),
        Stat::DefenceBlow => tr("melee def."),
        Stat::AttackShot => tr("ranged"),
        Stat::DefenceShot => tr("ranged def."),
        Stat::MagicPower => tr("magic"),
        Stat::Initiative => tr("initiative"),
        Stat::Manevres => tr("actions"),
        Stat::ProtectLife => tr("life prot."),
        Stat::ProtectDeath => tr("death prot."),
        Stat::ProtectElemental => tr("elem. prot."),
        Stat::Regen => tr("regen"),
        Stat::Vampirizm => tr("vampirism"),
    };
    let parts: Vec<String> = Stat::ALL
        .into_iter()
        .filter_map(|st| def.level_up.get(&st).filter(|&&d| d != 0 && (st != Stat::MagicPower || def.magic.is_some())).map(|&d| (st, d)))
        .map(|(st, d)| if is_percent_stat(st) { format!("+{d}% {}", label(st)) } else { format!("{d:+} {}", label(st)) })
        .collect();
    if parts.is_empty() {
        tr("nothing").into()
    } else {
        parts.join(", ")
    }
}

/// The unit's full stat list, as in the original's unit panel.
pub(super) fn unit_stat_lines(content: &Content, u: &Unit, wage: i32) -> Vec<String> {
    let s = u.stats(content);
    let need = u.xp_to_next(content);
    let mut lines = vec![
        razdor::trf!("{level}   next level in {left} XP", level = level_label(u.level, u.xp, need), left = (need - u.xp).max(0)),
        razdor::trf!("Per level: {gains}", gains = level_gains(content, u.def)),
        razdor::trf!("Hits {hp}/{max}   {attack}", hp = u.hp, max = s.max_hp(), attack = attack_line(&s)),
        razdor::trf!("Defence {blow} melee / {shot} ranged", blow = s[Stat::DefenceBlow], shot = s[Stat::DefenceShot]),
        razdor::trf!("Initiative {ini}   Actions {actions}", ini = s[Stat::Initiative], actions = s[Stat::Manevres]),
        razdor::trf!("Magic prot. life {life}% / elem. {elem}% / death {death}%", life = s[Stat::ProtectLife], elem = s[Stat::ProtectElemental], death = s[Stat::ProtectDeath]),
    ];
    let mut extra = Vec::new();
    if s[Stat::Regen] > 0 {
        extra.push(razdor::trf!("regen {v}%", v = s[Stat::Regen]));
    }
    if s[Stat::Vampirizm] > 0 {
        extra.push(razdor::trf!("vampirism {v}%", v = s[Stat::Vampirizm]));
    }
    extra.extend(s.bonuses.iter().map(super::unit_sheet::bonus_title));
    if !extra.is_empty() {
        lines.push(extra.join(", "));
    }
    if wage > 0 {
        lines.push(razdor::trf!("Daily wage {wage} gold", wage));
    }
    lines
}


fn equip_error(e: EquipError) -> String {
    match e {
        EquipError::NoFreeSlot => tr("No free slot.").into(),
        EquipError::SameType => tr("Already wears an item of that type.").into(),
        EquipError::DuplicateItem => tr("Already wears this item. Duplicate items are disabled.").into(),
        EquipError::PotionLimit => tr("This character has already drunk two potions. The limit resets after a battle.").into(),
        EquipError::SecondWeapon => tr("Only one weapon or staff at a time.").into(),
        EquipError::WrongClass => tr("This unit cannot use that (warrior, shooter or mage only).").into(),
        EquipError::Unholy => tr("The undead cannot hold holy things.").into(),
        EquipError::WrongSchool => tr("Only a unit of that school of magic can use it.").into(),
        EquipError::NotAllowed => tr("Only the hero and a few noble units may wear it.").into(),
        EquipError::NotWearable => tr("That cannot be worn.").into(),
        EquipError::Dead => tr("The dead hold nothing.").into(),
        EquipError::NotAPotion => tr("That is not a potion.").into(),
        EquipError::PackFull => tr("The pack is full.").into(),
        EquipError::NoSuchItem => tr("Nothing there.").into(),
    }
}

/// Backpack grid: 5 columns as in the original, scrolling.
const PACK_COLS: usize = 5;

local! {
    /// The unit whose Dismiss (or Bury) was pressed: confirm or cancel (0x4c3744).
    static CONFIRM: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// A battle effect playing over a unit's card (0x4b11cc).
    static CARD_FX: std::cell::Cell<Option<CardFx>> = const { std::cell::Cell::new(None) };
    /// The card sliding after a press on an empty cell.
    static SLIDE: std::cell::Cell<Option<Slide>> = const { std::cell::Cell::new(None) };
    /// What the backpack's filter line holds (`ui::item_filter`).
    static FILTER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    /// The backpack's shown group and order (Razdor's, `rules::pack_order`: a view only, the
    /// pack keeps its order; reset by `reset_pack_view` when a game starts or a save loads).
    static GROUP: std::cell::Cell<ItemGroup> = const { std::cell::Cell::new(ItemGroup::All) };
    static SORT: std::cell::Cell<PackSort> = const { std::cell::Cell::new(PackSort::Shown) };
    /// The class a promotion portrait under the pointer put on the left panel (0x4c39d8
    /// repaints it as a fresh unit of that type) while the pointer is on it (Razdor's: the
    /// original's keeps it after, until a card is hovered or pressed, 0x4c3cf4).
    static PANEL_TYPE: std::cell::Cell<Option<razdor::rules::content::UnitId>> = const { std::cell::Cell::new(None) };
    /// The promotion playing (0x4b1a04).
    static PROMO: std::cell::Cell<Option<Promo>> = const { std::cell::Cell::new(None) };
    /// The unit the left panel was last painted for when nothing is selected: the promoted
    /// one, as the original's end of a promotion paints it (0x492f24) and then deselects it;
    /// it stays until a card is hovered or pressed.
    static PANEL_UNIT: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// A promotion playing on the army window (0x4b1a04, scheduled by the portrait's click
/// 0x4c3d38): squad member `unit`, of class `from`, becomes `to`, the portrait at Razdor's
/// tree place `pick` (the original's portrait `n`, 1..3, whose distance sets the time).
/// `done` once the class has changed, 100 ms before the end.
#[derive(Clone, Copy)]
struct Promo {
    unit: usize,
    from: UnitId,
    to: UnitId,
    pick: usize,
    t0_ms: i64,
    ms: i64,
    done: bool,
}

/// How long a promotion from the original's portrait `n` (1..3) plays (0x4b1b45): 7 ms per
/// pixel from that portrait to the unit's own, plus 500, at most 8191. The portraits are 94 px
/// squares, the options at (286 + 106·(n − 1), 57) in the window, the unit's at (392, 200)
/// (0x4da5e1): 1501 ms for the middle option, 1746 for a side one.
fn promo_ms(n: usize) -> i64 {
    let (dx, dy) = ((n as f64 - 2.0) * 106.0, 143.0);
    ((dx * dx + dy * dy).sqrt() * 7.0).round() as i64 + 500
}

/// The original's slow-fast-slow ease (0x4b1918): of `t` in `total`, the share
/// `r = min(1000, 1000·t / total)` through slopes 1, 2, 3, 2, 1 on 0–100–300–700–900–1000
/// (2200 in all), given back in the units of `total`.
fn promo_ease(t: i64, total: i64) -> i64 {
    let r = (t * 1000 / total.max(1)).min(1000);
    let mut v = if r <= 100 { r } else if r <= 300 { (r - 100) * 2 + 100 } else { 500 };
    if r > 300 {
        v += if r < 700 { (r - 300) * 3 } else { 1200 };
    }
    if r > 700 {
        v += if r < 900 { (r - 700) * 2 } else { 400 };
    }
    if r > 900 && r <= 1000 {
        v += r - 900;
    }
    v * total / 2200
}

/// What a promotion shows `t` ms into its `ms` (0x4b1a04).
#[derive(Clone, Copy, Debug, PartialEq)]
struct PromoLook {
    /// The frame of battle effect 6 (`--UPGRADE`, 50 frames): 0..15 over the first 500 ms on
    /// the picked portrait, then 16..49 as it travels to the unit's own.
    frame: usize,
    /// How far along that way, 0 at the picked portrait, 1 at the unit's.
    along: f32,
    /// The green glow (red 0, green `a`, blue `a`/2, added) over the picked portrait,
    /// fading from 160 over the first 400 ms.
    pick_glow: u8,
    /// The same glow over all four portraits, up from 0 to 200 in the 100 ms before the class
    /// changes and down again in the last 100.
    all_glow: u8,
}

fn promo_look(t: i64, ms: i64) -> PromoLook {
    let travel = (ms - 500).max(1);
    let (frame, along) = if t < 500 {
        ((t * 15 / 500).min(15), 0.0)
    } else if t < ms {
        ((t - 500) * 33 / travel + 16, promo_ease(t - 500, travel) as f32 / travel as f32)
    } else {
        ((t - 500) * 33 / travel + 16, 1.0)
    };
    let pick_glow = if t < 400 { (400 - t) * 160 / 400 } else { 0 };
    let all_glow = if ms - 200 < t && t <= ms - 100 {
        (t - (ms - 200)) * 2
    } else if ms - 100 < t && t <= ms {
        (ms - t) * 2
    } else {
        0
    };
    PromoLook { frame: frame.clamp(0, 49) as usize, along, pick_glow: pick_glow.clamp(0, 255) as u8, all_glow: all_glow.clamp(0, 255) as u8 }
}

/// The promotion glow of strength `a` over portrait `r` (4768f8 mode 0 with 476374(0, a, a/2)).
fn promo_glow(r: Rect, a: u8) {
    if a > 0 {
        let a = a as f32 / 255.0;
        chrome::additive(|| draw_rectangle(r.x, r.y, r.w, r.h, Color::new(0.0, a, a / 2.0, 1.0)));
    }
}

/// The backpack filter's field.
const FILTER_KEY: &str = "army:pack-filter";

/// Puts `query` into the backpack's filter (debug snapshots).
pub(super) fn set_pack_filter(query: &str) {
    FILTER.with(|f| query.clone_into(&mut f.borrow_mut()));
}

/// Puts the backpack's group and order (debug snapshots).
pub(super) fn set_pack_view(group: ItemGroup, sort: PackSort) {
    GROUP.with(|g| g.set(group));
    SORT.with(|s| s.set(sort));
}

/// A new game, a campaign map or a loaded save starts the backpack's group and order at the
/// default (All, as found): the choice is for one game and is not stored in a save (a player's
/// report).
pub(super) fn reset_pack_view() {
    set_pack_view(ItemGroup::All, PackSort::Shown);
}

/// The backpack's filter as the army screen is left by any way (its own buttons clear it;
/// F9 or a battle leave by the app's).
pub(super) fn clear_pack_filter() {
    FILTER.with(|f| f.borrow_mut().clear());
}

/// The army window's timed card actions (0x4b11cc): a potion drunk (`--POTION`, 420 ms) or
/// a unit sent away (`--PAR`, 350 ms, the unit leaving when it ends); the window takes no
/// input meanwhile.
#[derive(Clone, Copy)]
struct CardFx {
    effect: usize,
    unit: usize,
    t0_ms: i64,
    ms: i64,
    dismiss: bool,
}

/// Starts a card action's effect over squad member `unit`, with its sound: `Battle-Cure` for a
/// potion, `Card-Move` for a dismissal (0x4b11cc).
fn card_fx(effect: usize, unit: usize, dismiss: bool) {
    let ms = if dismiss { 350 } else { 420 };
    cue(if dismiss { Cue::CardMove } else { Cue::Cure });
    CARD_FX.with(|f| f.set(Some(CardFx { effect, unit, t0_ms: now_ms(), ms, dismiss })));
}

/// The item's sound as a press picks it up or puts it down (0x4c24f4 → 0x49a994: on a swap,
/// the one picked up).
fn press_sound(game: &Game, press: Press) {
    let item = match press {
        Press::Picked(i) | Press::Put(i) | Press::Swapped { picked: i, .. } => i,
        Press::Nothing => return,
    };
    if let Some(def) = game.content.try_item(item) {
        cue(Cue::Item(def.kind));
    }
}

/// A press on the pack, as the original's (0x4c24f4): on cell `i` the item there is picked
/// up onto the pointer, or the item held is put there (an empty cell anywhere keeps it) or
/// swapped with it. `None` is an empty row of the filtered list (Razdor's filter): the item
/// held goes into the pack's first empty cell.
fn pack_press(game: &mut Game, cell: Option<usize>) {
    match cell {
        Some(i) => {
            let press = game.press_pack(i);
            press_sound(game, press);
        }
        None => {
            if let Some(item) = game.held_item.filter(|_| !game.pack.is_full()) {
                game.drop_held();
                press_sound(game, Press::Put(item));
            }
        }
    }
}

/// A press on worn slot `slot` of squad member `unit`'s panel (0x4c24f4): the item is taken
/// off onto the pointer, or the item held is put on (the wear test's refusal is the message).
fn worn_press(game: &mut Game, unit: usize, slot: usize) -> Option<String> {
    if shift_down() && game.held_item.is_none() {
        let item = game.squad.get(unit)?.items[slot]?;
        return match game.unequip(unit, slot) {
            Ok(()) => { press_sound(game, Press::Picked(item)); None }
            Err(e) => Some(equip_error(e)),
        };
    }
    match game.press_worn(unit, slot) {
        Ok(press) => {
            press_sound(game, press);
            None
        }
        Err(e) => Some(equip_error(e)),
    }
}

fn shift_down() -> bool {
    [KeyCode::LeftShift, KeyCode::RightShift].into_iter().any(super::input::is_key_down)
}

fn pack_click(game: &mut Game, cell: Option<usize>) -> Option<String> {
    if shift_down() && game.held_item.is_none() {
        let cell = cell?;
        let item = game.pack.get(cell)?;
        let kind = game.content.item(item).kind;
        let result = if kind == razdor::rules::content::ArtefactType::Potion {
            game.drink(0, cell).map(|_| ())
        } else {
            game.equip(0, cell)
        };
        match result {
            Ok(()) => { cue(Cue::Item(kind)); None }
            Err(e) => Some(equip_error(e)),
        }
    } else {
        pack_press(game, cell);
        None
    }
}

/// The item on the pointer drawn as the pointer, the original's 55 px picture centred on it
/// (the record at 0xae1da0, hotspot (−27, −27), takes the item's picture: 0x4c24f4).
fn draw_held(game: &Game, assets: &Assets) {
    if let Some(item) = game.held_item {
        let (mx, my) = pointer();
        let s = 55.0 * chrome::k();
        assets.draw_item(item, mx - s / 2.0, my - s / 2.0, s);
        super::cursor::set(super::cursor::Shape::Hidden);
    }
}

/// A press with an item held on squad member `to`'s card ([`Game::give_held`], 0x4c346c →
/// 0x4979c4): a potion is drunk, the hero's card sends anything else to the pack, another
/// unit wears it; a refusal keeps it on the pointer. The message to show, if any.
fn card_press(game: &mut Game, to: usize) -> Option<String> {
    let c = game.content.clone();
    let item = game.held_item?;
    let kind = c.item(item).kind;
    let name = game.squad.get(to)?.name(&c).to_string();
    match game.give_held(to) {
        Ok(Given::Drunk(healed)) => {
            card_fx(5, to, false);
            Some(if healed > 0 {
                razdor::trf!("{name} drinks it: +{healed} hits.", name, healed)
            } else {
                razdor::trf!("{name} drinks it. The effect lasts until the next battle ends.", name)
            })
        }
        Ok(_) => {
            cue(Cue::Item(kind));
            None
        }
        Err(e) => Some(equip_error(e)),
    }
}

/// The promotion tree of squad member `sel` (not the hero) in `r`, as the original's: the
/// current class at the bottom, arrows up to its options (portraits; open now, they glow and
/// promote on a click, free of charge). A unit that cannot be promoted, at its first level
/// or of a class with no next type, has every portrait locked, its own included (494340).
/// While a promotion plays ([`Promo`]) the picked portrait is empty until the class changes,
/// with the glows and the effect travelling down to the unit's own (0x4b1a04).
fn tree_view(game: &Game, assets: &Assets, sel: usize, u: &Unit, r: Rect, promo: Option<Promo>) -> Option<razdor::rules::content::UnitId> {
    let mut pointed = None;
    let c = game.content.clone();
    let k = chrome::k();
    let tree = u.upgrade_tree(&c);
    let locked = !tree.iter().any(|&(_, _, ok)| ok);
    if let Some(t) = chrome::win_fx("UpgradeTree", chrome::Fx::KeyBlack) {
        chrome::tex(&t, r, WHITE);
    }
    // The pane's frame, as the original's tree box: a light line over a dark one (seen in
    // the original at 1024×768; the description box beside it has the same).
    draw_rectangle_lines(r.x - 2.0 * k, r.y - 2.0 * k, r.w + 4.0 * k, r.h + 4.0 * k, 2.0 * k, Color::new(0.0, 0.0, 0.0, 0.7));
    draw_rectangle_lines(r.x - 1.0 * k, r.y - 1.0 * k, r.w + 2.0 * k, r.h + 2.0 * k, 1.0 * k, Color::new(0.78, 0.8, 0.82, 0.9));
    let s = (r.w * 0.28).min(r.h * 0.36);
    let cur = Rect::new(r.x + (r.w - s) / 2.0, r.y + r.h * 0.95 - s, s, s);
    let slots = [0.17, 0.5, 0.83];
    let place = |i: usize| Rect::new(r.x + r.w * slots[i] - s / 2.0, r.y + r.h * 0.04, s, s);
    let look = promo.map(|p| (p, promo_look(now_ms() - p.t0_ms, p.ms)));
    // Each option stands at its own place, `NextUnit1` left, 2 in the middle, 3 right
    // (494340 reads the type's option of portrait n: +0xa4 + n·8); an empty place shows the
    // tree's own picture. Two options sit in places 1 and 3 ([`normalise_upgrade_slots`]).
    let def = c.unit(u.def);
    let slot_of = |to: razdor::rules::content::UnitId, n: usize| {
        def.upgrades.iter().find(|x| x.target == Some(to.0)).map_or(n, |x| (x.slot.clamp(1, 3) - 1) as usize)
    };
    for (n, &(to, _level, ok)) in tree.iter().take(3).enumerate() {
        let i = slot_of(to, n);
        let o = place(i);
        // The picked portrait is drawn as an empty option (494340 for the hero, who has none)
        // from the click until the class changes.
        if let Some((_, l)) = look.filter(|(p, _)| !p.done && p.pick == i) {
            promo_glow(o, l.pick_glow.max(l.all_glow));
            continue;
        }
        if chrome::win("UpgradeTree").is_none() {
            draw_line(cur.x + cur.w / 2.0, cur.y, o.x + o.w / 2.0, o.y + o.h, 3.0, if ok { chrome::GOLD } else { DIM });
        }
        draw_rectangle(o.x - 2.0, o.y - 2.0, o.w + 4.0, o.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
        if locked {
            assets.draw_portrait_locked(to, Team::Player, o);
        } else {
            assets.draw_portrait(to, Team::Player, o);
        }
        // No level caption: the original's portraits carry none (494340).
        draw_rectangle_lines(o.x, o.y, o.w, o.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let over = mouse_in(o.x, o.y, o.w, o.h);
        if ok {
            chrome::glow_frame(o, Color::new(0.35, 1.0, 0.35, if over { 1.0 } else { 0.6 }), over);
        }
        if let Some((_, l)) = look {
            promo_glow(o, l.all_glow);
        }
        if over {
            pointed = Some(to);
            // The click starts the promotion, its sound at once (0x4c3d38 → 0x4b1a04); the
            // class changes as it ends.
            if ok && clicked() && promo.is_none() {
                cue(Cue::Upgrade);
                PROMO.with(|p| p.set(Some(Promo { unit: sel, from: u.def, to, pick: i, t0_ms: now_ms(), ms: promo_ms(promo_portrait(&c, u.def, to)), done: false })));
            }
        }
    }
    draw_rectangle(cur.x - 2.0, cur.y - 2.0, cur.w + 4.0, cur.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
    // The original draws no note here (no such text in the ini): the locked portraits say it.
    if locked {
        assets.draw_portrait_locked(u.def, Team::Player, cur);
    } else {
        assets.draw_portrait(u.def, Team::Player, cur);
    }
    chrome::wounds(cur, u.hp, u.max_hp(&c));
    draw_rectangle_lines(cur.x, cur.y, cur.w, cur.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
    if mouse_in(cur.x, cur.y, cur.w, cur.h) {
        pointed = Some(u.def);
    }
    if let Some((p, l)) = look {
        promo_glow(cur, l.all_glow);
        // Effect 6 over everything, its 220 px square centred on the portrait (63 px out of
        // the 94 px one, 0x668c5c), eased from the picked portrait to the unit's.
        let from = place(p.pick.min(2)).center();
        draw_effect_frame(&c, 6, l.frame, from.lerp(cur.center(), l.along), s / 94.0);
    }
    pointed
}

/// The original's portrait (1..3) of the option `to` in class `def`'s upgrade tree: its
/// `NextUnitN` slot as normalised at load, a lone option in the middle (2).
fn promo_portrait(c: &Content, def: UnitId, to: UnitId) -> usize {
    c.unit(def).upgrades.iter().position(|u| u.target == Some(to.0)).map_or(2, |i| i + 1)
}

/// Frame `i` of battle effect `n` centred on `at`, `px` screen pixels to the original's
/// (48e058: the 220×110 frame with its rows doubled, a 220 px square).
fn draw_effect_frame(c: &Content, n: usize, i: usize, at: Vec2, px: f32) {
    let Some(frames) = c.options.battle_effects.get(n).and_then(chrome::battle_effect) else { return };
    let size = 220.0 * px;
    chrome::premultiplied(|| {
        draw_texture_ex(&frames[i.min(frames.len() - 1)], at.x - size / 2.0, at.y - size / 2.0, WHITE, DrawTextureParams { dest_size: Some(vec2(size, size)), ..Default::default() });
    });
}

/// How many rows the pack scrolls over: the rows down to the one holding the cell after the
/// last used one (0x49a718: the first shown cell is 5 × Round(max(0, length div 5 − 4) × s),
/// so with 5 rows shown the top row goes down to length div 5 − 4), in `cols` columns. The
/// original stops at its 256th cell; Razdor's pack grows (a deviation), so there is always an
/// empty row after the last used one.
fn pack_rows(len: usize, cols: usize) -> usize {
    len / cols.max(1) + 1
}

/// The backpack: the original's inventory squares, the pack's cells in order a row at a time,
/// scrolling; with a filter, only the cells `kept`, in their order. Returns the press: on a
/// cell (`Some(i)`, an empty one too: an item put there stays there, 0x4c24f4), or on an empty
/// row of the filtered list (`None`).
fn pack_view(game: &Game, assets: &Assets, r: Rect, scroll: &mut usize, hover: &mut Option<ItemId>, kept: Option<&[usize]>) -> Option<Option<usize>> {
    // A cell's item: the pack's own cell `i`, or with a filter the `i`th kept one.
    let item = |i: usize| match kept {
        Some(v) => v.get(i).and_then(|&p| game.pack.get(p)),
        None => game.pack.get(i),
    };
    let rows_of = |cols: usize| kept.map_or_else(|| pack_rows(game.pack.len(), cols), |v| v.len().max(1).div_ceil(cols));
    let cell = item_squares(assets, r, scroll, rows_of, item, game.held_item.is_some(), hover)?;
    Some(kept.map_or(Some(cell), |v| v.get(cell).copied()))
}

/// The original's inventory squares (the backpack, the building's store): the cells `item(i)` in
/// order (`None` an empty one), a row at a time, as many across as the width takes (the original's
/// five in its 300 px, more in a wider grid) and `rows_of(cols)` rows in all, scrolling by `scroll`
/// with the original's scroll bar right of them. A swipe or the wheel over it scrolls a row a step.
/// The cell under the pointer shows gold when it holds an item (or when `held` says an item is
/// held), and its item goes to `hover`. Returns the cell pressed, `Some(i)`, empty cells too.
pub(crate) fn item_squares(assets: &Assets, r: Rect, scroll: &mut usize, rows_of: impl Fn(usize) -> usize, item: impl Fn(usize) -> Option<ItemId>, held: bool, hover: &mut Option<ItemId>) -> Option<usize> {
    let k = chrome::k();
    // As many of the original's squares as the width takes (5 in its 300 px), then as many
    // rows as fill the height, the squares shrunk to fit them: no empty band under the pack.
    let room = r.w - 18.0 * k;
    let wide = (room / ((room / (56.0 * k)).floor().max(PACK_COLS as f32))).floor();
    let rows_shown = ((r.h / wide).round() as usize).max(1);
    let cell = wide.min(r.h / rows_shown as f32).floor();
    let cols = ((room / cell).floor() as usize).max(PACK_COLS);
    let rows = rows_of(cols).max(rows_shown);
    let max_scroll = rows.saturating_sub(rows_shown);
    swipe_rows(r, cell);
    if mouse_in(r.x, r.y, r.w, r.h) {
        let w = wheel();
        if w < 0.0 {
            *scroll = (*scroll + 1).min(max_scroll);
        } else if w > 0.0 {
            *scroll = scroll.saturating_sub(1);
        }
    }
    // The scroll bar: the original's (with its ▲ and ▼ arrows and dark track), as the army
    // window shows it right of the pack (the original's screenshots; the widget's address is
    // not yet traced, see docs/OPEN_QUESTIONS.md). Its rows are the pack's rows (49a718); the
    // arrows move one row, a track click a page, and the knob drags.
    let bar = Rect::new(r.x + cols as f32 * cell + 4.0 * k, r.y, 12.0 * k, rows_shown as f32 * cell);
    scrollbar_always(bar, scroll, rows_shown, rows);
    *scroll = (*scroll).min(max_scroll);
    let inv = chrome::win("Inventory");
    let mut hit = None;
    for row in 0..rows_shown {
        for col in 0..cols {
            let i = (*scroll + row) * cols + col;
            let cr = Rect::new(r.x + col as f32 * cell, r.y + row as f32 * cell, cell, cell);
            match &inv {
                Some(t) => {
                    let s = t.width() / 5.0;
                    chrome::tex_src(t, Rect::new((col % 5) as f32 * s, ((row + *scroll) % 5) as f32 * s, s, s), cr, WHITE);
                }
                None => {
                    chrome::surface(cr, chrome::Skin::Paper);
                    draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 1.0, Color::new(0.5, 0.35, 0.2, 0.8));
                }
            }
            let it = item(i);
            if let Some(it) = it {
                assets.draw_item(it, cr.x + 2.0, cr.y + 2.0, cell - 4.0);
            }
            if mouse_in(cr.x, cr.y, cr.w, cr.h) {
                if it.is_some() || held {
                    draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 2.0, chrome::GOLD);
                }
                if it.is_some() {
                    *hover = it;
                }
                if clicked() {
                    hit = Some(i);
                }
            }
        }
    }
    hit
}

/// The army window's selection (0x668a08: `None`, or a squad index, the hero 0) and the unit
/// its right side and Dismiss row were last switched to (0x498d0c; `None` the pack).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ArmySel {
    pub selected: Option<usize>,
    pub shown: Option<usize>,
}

impl ArmySel {
    /// A press on card `pressed` (`None`: an empty cell), as 0x4c346c: the selection changes
    /// and the right side is switched to the selected unit (0x498d0c), except on a
    /// deselection, which the original jumps past: the deselected unit's promotion tree stays
    /// up (original behaviour) until a later press switches it. A swap leaves nothing
    /// selected and the pack up; a slide keeps the unit selected (and its tree up) until the
    /// slide ends ([`ArmySel::slide_done`]). With dragging only (`controls`, Razdor's setting,
    /// the owner's decision) a press only selects: a unit pressed is selected and its side
    /// shown, an empty cell clears the selection and shows the pack.
    fn press(&mut self, controls: Controls, pressed: Option<usize>) -> GridPress {
        let p = grid_press_in(controls, self.selected, pressed);
        match p {
            GridPress::Deselect => self.selected = None,
            GridPress::Select(u) => {
                self.selected = Some(u);
                self.shown = Some(u);
            }
            GridPress::Swap { .. } => {
                self.selected = None;
                self.shown = None;
            }
            GridPress::Slide(s) => self.shown = Some(s),
            // As the original's nothing selected; with dragging only a unit may be selected.
            GridPress::Nothing => {
                self.selected = None;
                self.shown = None;
            }
        }
        p
    }

    /// The end of a slide (0x4b0c04) clears the selection without switching the right side.
    fn slide_done(&mut self) {
        self.selected = None;
    }

    /// Indices beyond the army (after a dismissal) are dropped.
    fn clamp(&mut self, len: usize) {
        self.selected = self.selected.filter(|&i| i < len);
        self.shown = self.shown.filter(|&i| i < len);
    }

    /// The unit whose promotion tree the right side shows: none for the hero (unit < 2 in
    /// 0x498d0c: no unit or the hero, the pack).
    fn tree(&self) -> Option<usize> {
        self.shown.filter(|&i| i > 0)
    }
}

/// A card sliding to its new cell after a press on an empty cell (0x4b0c04): the squad index,
/// where it starts (screen px), when (ms of the clock) and for how long. The move itself is
/// already made; presses on the grid wait for the slide (busy flag 0x68dc63).
#[derive(Clone, Copy)]
struct Slide {
    unit: usize,
    from: Vec2,
    t0_ms: i64,
    ms: i64,
}

/// The strip's line when a promotion ends: none on success, the original shows no line on a
/// promotion (it changes the class at 0x4b1dcb, again at 0x4b1f42 if not yet, and hides the
/// hint at 0x4b1fff); a refused one shows «Not possible.».
fn promotion_message<E>(r: Result<(), E>) -> Option<String> {
    match r {
        Ok(()) => None,
        Err(_) => Some(tr("Not possible.").into()),
    }
}

fn now_ms() -> i64 {
    (crate::ui::clock::now() * 1000.0) as i64
}

/// The hero and army screen, as the original's (refs 11 and 12): the selected unit's panel
/// with its four item slots on the left; the backpack (or the upgrade tree) and the item
/// description at the top; the army's 2×6 cards below. Click a card to select it, a pack
/// item to wear or drink it, a worn item to take it off; drag a pack or worn item onto a
/// card to give it to that unit, or a worn one onto the pack to take it off. Cards are
/// dragged (Razdor's), and pressed as in the original ([`ArmySel::press`]) or, with dragging
/// only (Razdor's default setting), pressed only to select. `back` is the
/// building window to return to, if it was opened from one.
pub fn squad(
    game: &mut Game,
    assets: &Assets,
    selected: &mut ArmySel,
    scroll: &mut usize,
    back: &Option<BuildingView>,
    message: &mut Option<String>,
) -> Option<Screen> {
    let bar = super::world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Squad));
    selected.clamp(game.squad.len());
    // A slide that has ended clears the selection (0x4b0c04).
    let slide = SLIDE.with(|s| s.get()).filter(|s| s.unit < game.squad.len());
    let slide = match slide {
        Some(s) if now_ms() - s.t0_ms < s.ms => Some(s),
        Some(_) => {
            selected.slide_done();
            None
        }
        None => None,
    };
    SLIDE.with(|s| s.set(slide));
    // A card action that has ended: a dismissed unit leaves now, nothing selected, the pack up
    // (0x4b1778).
    if let Some(fx) = CARD_FX.with(|f| f.get()) {
        if now_ms() - fx.t0_ms >= fx.ms {
            CARD_FX.with(|f| f.set(None));
            if fx.dismiss {
                if let Some(u) = game.squad.get(fx.unit).cloned() {
                    let name = u.name(&game.content).to_string();
                    *message = Some(match game.dismiss(fx.unit) {
                        Ok(()) if u.alive() => razdor::trf!("{name} leaves your army.", name),
                        Ok(()) => razdor::trf!("{name} is laid to rest.", name),
                        Err(e) => service_error(e),
                    });
                }
                *selected = ArmySel::default();
                selected.clamp(game.squad.len());
            }
        } else {
            set_input_blocked(true);
        }
    }
    // A promotion playing (0x4b1a04) takes no input. Its unit takes the new class once past
    // 100 ms before the end (0x4b1dcb); at the end the unit's panel is painted (0x492f24),
    // nothing is selected and its tree stays up (0x498d0c).
    let mut promo = PROMO.with(|p| p.get()).filter(|p| game.squad.get(p.unit).is_some_and(|u| u.def == if p.done { p.to } else { p.from }));
    if let Some(p) = promo.as_mut() {
        let t = now_ms() - p.t0_ms;
        if !p.done && t > p.ms - 100 {
            p.done = true;
            *message = promotion_message(game.promote(p.unit, p.to));
        }
        if t >= p.ms {
            *selected = ArmySel { selected: None, shown: Some(p.unit) };
            PANEL_TYPE.with(|t| t.set(None));
            PANEL_UNIT.with(|u| u.set(Some(p.unit)));
            promo = None;
        } else {
            set_input_blocked(true);
        }
    }
    PROMO.with(|p| p.set(promo));
    let c = game.content.clone();
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let win = window_layout::window();
    let title = chrome::ui_text("Army", "Title").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr("The hero's characteristics and army").to_string());
    let (_, close) = chrome::window(win, &title, chrome::Skin::Marble, true);
    let close = close_button(win) || close;
    window_layout::panel_frame(window_layout::upper(win));
    window_layout::panel_frame(window_layout::lower(win));
    let at = |x: f32, y: f32, w: f32, h: f32| window_layout::at(win, x, y, w, h);
    let mut hover = None;
    // The left panel: the selected unit, else the hero (or the unit just promoted); items go
    // to it (0x4c280c).
    let sel = selected.selected.or_else(|| PANEL_UNIT.with(|u| u.get()).filter(|&i| i < game.squad.len())).unwrap_or(0);
    // The right side: the pack, or this unit's promotion tree and Dismiss row (0x498d0c).
    let tree_of = selected.tree();
    let u = game.squad[sel].clone();
    // A promotion portrait hovered shows its class on the left panel, as a fresh unit of it
    // (0x4c39d8: level 1, no items, no status or row bonus).
    let panel_type = PANEL_TYPE.with(|p| p.get()).filter(|_| tree_of.is_some());
    let u = match panel_type {
        Some(id) if c.try_unit(id).is_some() => {
            let mut fresh = Unit::new(&c, id, u.slot);
            fresh.heal_full(&c);
            fresh
        }
        _ => u,
    };
    let showing_type = panel_type.is_some();

    // The unit's panel; a press on a worn slot picks its item up or puts the held one on
    // (0x4c24f4).
    // In a building of his the hero's units show its defence with their own, "15 + 20"
    // (army +0x378c, as the cards: 0x49462c); a class preview shows none.
    let bd = if showing_type { 0 } else { game.hero_building_defence() };
    let mut stats = u.stats(&c);
    stats[Stat::DefenceBlow] += bd;
    stats[Stat::DefenceShot] += bd;
    let hero = (sel == 0).then(|| razdor::rules::content::HeroClass::ALL.into_iter().find(|h| h.unit() == u.def)).flatten();
    let mut status = potion_status(&c, &u);
    if !showing_type && game.allow_unit_benching && u.battle_disabled && sel > 0 {
        status.push((tr("Not fighting").to_string(),chrome::ORANGE_TEXT));
    }
    if !showing_type && c.experimental.two_potions {
        status.push((razdor::trf!("Potions before next battle: {used}/2",used=u.potions_drunk),chrome::BLUE_TEXT));
    }
    // Dead and unpaid are the original's trait lines (`unit_sheet::state_lines`).
    let label = if showing_type { c.unit(u.def).name.clone() } else { game.squad_label(sel) };
    let sheet = unit_sheet::Sheet {
        kind: u.def,
        name: &label,
        named: !showing_type && u.named > 0,
        level: u.level,
        xp: u.xp,
        need: u.xp_to_next(&c),
        hp: u.hp,
        now: &stats,
        start: &stats,
        bleed: 0,
        power: stats[Stat::MagicPower],
        wage: if showing_type { class_wage(&c, u.def) } else { game.wage(sel) },
        items: if showing_type { [None; razdor::rules::items::MAX_SLOTS] } else { u.items },
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        unpaid: !showing_type && u.unpaid,
        building: bd,
        hero,
        status,
    };
    // A living unit's first `personal` worn slots are his own: not taken up (0x4c24f4), and
    // hovering one says so (0x4c280c, `[Army] ItemI`).
    let locked = |slot: usize| showing_type || (u.alive() && slot < u.personal as usize);
    let pressed_slot = super::dt_font::with_native_metrics(|| draw_sheet(assets, &c, win, &sheet, panel_slots(showing_type), &mut hover));
    let personal_hover = hover.is_some_and(|h| u.items.iter().enumerate().any(|(s, i)| *i == Some(h) && locked(s)));
    if let Some(slot) = pressed_slot.filter(|_| !showing_type) {
        *message = worn_press(game, sel, slot);
    }
    // Stacked (a phone held upright) the seam is drawn across the window (`window_layout::seam`).
    if window_layout::portrait() {
        window_layout::seam(win);
    } else {
        draw_line(win.x + 247.0 * k, win.y + 27.0 * k, win.x + 247.0 * k, win.y + win.h - 2.0, 1.5 * k, chrome::SILVER);
    }

    // Top middle, as the original switches it: the hero's backpack, or the selected unit's
    // upgrade tree under its title.
    let show_tree = tree_of.is_some();
    let head = (13.0 * k).round();
    let own = |key: &str, ours: &'static str| chrome::ui_text("Army", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
    if show_tree {
        let title = own("UpgradeTree", n_("Upgrade tree"));
        let tree = window_layout::point(win, 408.0, 40.0);
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            chrome::shadow_centered(&title, tree.x, tree.y + head * 0.36, 15.0 * k, chrome::CREAM);
        });
    }
    // Razdor gives the backpack the whole top right (the user's choice, 2026-10-09: no item
    // description box; an item's description pops up by the pointer, as a spell badge's).
    // Under the row of group and search buttons (below) the pack starts at y 66.
    let content = if show_tree { at(258.0, 54.0, 300.0, 242.0) } else { at(258.0, 66.0, 568.0, 230.0) };
    let (mut group, mut sort) = (GROUP.with(|g| g.get()), SORT.with(|s| s.get()));
    // The row over the pack: nine equal buttons across the backpack, the last one the search.
    let row = item_filter::row_cells::<9>(at(258.0, 31.0, 568.0, 30.0));
    if !show_tree {
        // The row (Razdor's, view only, the owner's approved mockup): eight group buttons (a
        // picture each, a click shows that group, a click on the lit one shows all), then the
        // search button, which is the filter (below). A right click on it goes on to the next
        // order. A change starts the pack at its top.
        let picked = item_filter::group_row(&c, assets, &row[..8], group);
        let mut sorted = sort;
        if right_clicked() && mouse_in(row[8].x, row[8].y, row[8].w, row[8].h) {
            sorted = sort.next();
        }
        if picked != group || sorted != sort {
            (group, sort) = (picked, sorted);
            GROUP.with(|g| g.set(group));
            SORT.with(|s| s.set(sort));
            *scroll = 0;
        }
    }
    // The backpack's filter line above it: typing a letter starts it.
    let mut query = FILTER.with(|f| f.borrow().clone());
    let mut filter = item_filter::Reply::default();
    let mut kept = Vec::new();
    if show_tree {
        if has_focus(FILTER_KEY) {
            clear_focus();
        }
    } else {
        // The pack's cells in the group and order shown (the pack itself keeps its order); the
        // filter counts within them.
        let ordered = pack_order::pack_view_order(&c, game.pack.entries(), group, sort);
        let of = ordered.len();
        let before = item_filter::keep(&c, ordered.iter().copied(), &query).len();
        filter = item_filter::search_button(FILTER_KEY, &mut query, row[8], &[keymap::key(Action::HeroArmy), keymap::key(Action::Music)], before, of, sort);
        kept = item_filter::keep(&c, ordered, &query);
        FILTER.with(|f| query.clone_into(&mut f.borrow_mut()));
    }
    // A group or an order other than the pack's own shows the pack through `kept` too.
    let arranged = !show_tree && (group != ItemGroup::All || sort != PackSort::Shown);
    let filtering = !query.trim().is_empty();
    let kept_ids: Vec<usize> = kept.iter().map(|(i, _)| *i).collect();
    let mut tree_hint = None;
    if let Some(t) = tree_of {
        let tu = game.squad[t].clone();
        let shown = tree_view(game, assets, t, &tu, content, promo.filter(|p| p.unit == t));
        // Razdor's: the panel goes back to the unit when the pointer leaves the portrait, where
        // the original's keeps the class (0x4c3cf4; a player's request, 2026-10-12).
        PANEL_TYPE.with(|p| p.set(shown));
        if shown.is_some() {
            // The hint line (0x4c39d8): `[Army]` line 4 when it lacks the level, 5 to choose,
            // 6 when it has no further class.
            let tree = tu.upgrade_tree(&c);
            let n = if tree.is_empty() { 6 } else if tu.level <= 1 { 4 } else { 5 };
            let who = game.squad_label(t);
            tree_hint = chrome::ui_line("Army", n).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).map(|line| line.replace("#NAME1", &format!("\"{who}\"")));
        }
    } else if let Some(cell) = pack_view(game, assets, content, scroll, &mut hover, (filtering || arranged).then_some(&kept_ids[..])) {
        *message = None;
        *message = pack_click(game, cell);
    }
    // Enter (Razdor's filter) picks the first match up onto the pointer.
    if filter.pick && game.held_item.is_none() {
        if let Some(&first) = kept_ids.first() {
            pack_press(game, Some(first));
        }
    }

    // Top right: for a unit other than the hero, its face and the button that sends it away.
    let row = at(570.0, 234.0, 256.0, 62.0);
    if let Some(sel) = tree_of {
        let u = game.squad[sel].clone();
        draw_rectangle(row.x, row.y, row.w, row.h, Color::new(0.25, 0.04, 0.02, 0.55));
        chrome::silver_frame(row, 1.0);
        let face = Rect::new(row.x + 4.0 * k, row.y + 4.0 * k, row.h - 8.0 * k, row.h - 8.0 * k);
        assets.draw_portrait(u.def, Team::Player, face);
        chrome::wounds(face, u.hp, u.max_hp(&c));
        let label = if u.alive() { own("Dismiss", n_("Dismiss")) } else { own("Bury", n_("Bury")) };
        let label = label.as_str();
        let bench_control = game.allow_unit_benching && sel > 0 && u.alive();
        let b = Rect::new(face.x + face.w + 12.0 * k,
            row.y + if bench_control { 5.0*k } else { 14.0*k },
            row.w - face.w - 24.0 * k, if bench_control { 24.0*k } else { row.h - 28.0*k });
        // One confirmation step, as the original's confirm and cancel buttons; all three in its
        // red (0x4dc118 makes them with 0x4dba90: Btn3 for Dismiss, Btn1 for the pair).
        let confirming = CONFIRM.with(|c| c.get()) == Some(sel);
        if bench_control && !confirming {
            let toggle=Rect::new(b.x,row.y+33.0*k,b.w,24.0*k);
            let label=if u.battle_disabled { tr("Return to battle") } else { tr("Exclude from battles") };
            if red_button(toggle.x,toggle.y,toggle.w,toggle.h,label,true) {
                game.set_unit_battle_disabled(sel,!u.battle_disabled);
            }
        }
        if !confirming {
            if red_button(b.x, b.y, b.w, b.h, label, true) {
                CONFIRM.with(|c| c.set(Some(sel)));
                // Its worn items go with it: said before it is confirmed.
                let worn = u.items.iter().flatten().count();
                if worn > 0 {
                    *message = Some(razdor::trf!("{name} wears {n} item(s): they will be lost with it. Take them off first to keep them.", name = u.name(&c), n = worn));
                }
            }
        } else {
            let half = (b.w - 6.0 * k) / 2.0;
            if red_button(b.x, b.y, half, b.h, tr("Confirm"), true) {
                CONFIRM.with(|c| c.set(None));
                // It leaves when the effect over its card ends (0x4b11cc kind 2).
                card_fx(2, sel, true);
            } else if red_button(b.x + half + 6.0 * k, b.y, half, b.h, tr("Cancel"), true) {
                CONFIRM.with(|c| c.set(None));
            }
        }
    }

    // The switch above the Dismiss row, in the empty space right of the tree: the tree's own
    // bottom-left corner is too small for its text at a phone's size (no text under 12 sp).
    // It hides the «+» of this unit only, at its level (`Unit::set_sign_off`). Shown while the
    // unit can be promoted now; the tree keeps offering the promotion.
    if let Some(t) = tree_of.filter(|&t| game.squad[t].promotion_ready(&c)) {
        let on = game.squad[t].sign_off();
        // A phone's label is 12.5 px per dp (its capital about 12.5 sp, the check's floor of 12
        // sp; a fixed px size would shrink on a denser phone); the desktop keeps the army
        // window's own size. The row sits in the empty block above the Dismiss row, 76 px tall,
        // its bottom at 226.
        let size = match super::display::dp_px() {
            Some(dp) => 12.5 * dp,
            None => 16.0 * k,
        };
        let row = at(574.0, 150.0, 248.0, 76.0);
        if let Some(off) = super::widgets::checkbox_row(row, size, tr("Don't remind"), on) {
            game.squad[t].set_sign_off(off);
        }
    }

    // The strip under the top half: the last message, or what to do (drawn after the grid,
    // which says what the pointer is over).
    let strip = window_layout::divider(win);

    // The army: the cards as in battle, the selected one lit.
    let f = c.formation;
    let lines = f.display_lines() as f32;
    let pane = window_layout::lower(win);
    let cols = f.display_cols() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / cols)
        .min((pane.w - 16.0*k) / ((cols*96.0-8.0)*k))
        .min((pane.h - 16.0*k) / (((lines-1.0)*133.0+128.0)*k));
    let (card, pitch) = (vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k));
    let gx = window_layout::army_grid_left(window_layout::lower(win), f.display_cols(), pitch.x, 8.0 * cs * k);
    let cell_at = |slot: razdor::rules::formation::Slot| {
        let (line, col) = f.display(slot);
        vec2(gx + col as f32 * pitch.x, window_layout::lower_content(win).y + line as f32 * pitch.y).round()
    };
    // A press on a card or an empty cell, acted on after the grid is drawn.
    let mut pressed = None;
    // The cell a card slides into stays drawn empty until the card lands on it.
    let landing = slide.map(|s| game.squad[s.unit].slot);
    for slot in f.slots() {
        let taken = game.squad.iter().any(|u| u.slot == slot);
        if taken && landing != Some(slot) {
            continue;
        }
        let p = cell_at(slot);
        chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(f, slot), true);
        if !taken && mouse_in(p.x, p.y, card.x, card.y) && clicked() {
            pressed = Some((None, slot));
        }
    }
    let empty_under = f.slots().filter(|&slot| !game.squad.iter().any(|u| u.slot == slot)).find(|&slot| {
        let p = cell_at(slot);
        mouse_in(p.x, p.y, card.x, card.y)
    });
    let mut card_under = None;
    for (i, v) in game.squad.iter().enumerate() {
        let mut p = cell_at(v.slot);
        if let Some(s) = slide.filter(|s| s.unit == i) {
            p = s.from.lerp(p, (now_ms() - s.t0_ms) as f32 / s.ms.max(1) as f32).round();
        }
        let sq = Rect::new(p.x, p.y, card.x, card.x);
        chrome::unit_shadow(Rect::new(p.x, p.y, card.x, card.y));
        assets.draw_portrait(v.def, Team::Player, sq);
        chrome::wounds(sq, v.hp, v.max_hp(&c));
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let vs = v.stats(&c);
        super::dt_font::with_native_metrics(|| unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[Stat::MagicPower], unit_sheet::caster(&c, v.def), unit_sheet::strip_place(f, v.slot), v.hp, super::building_view::back_row_def(&c, v.slot), game.hero_building_defence(), selected.selected == Some(i), unit_sheet::StripPanel::of_squad(i, v.named)));
        if !v.alive() {
            chrome::death_veil(sq);
        } else if v.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        if i > 0 && game.allow_unit_benching && v.battle_disabled {
            draw_rectangle(sq.x,sq.y,sq.w,sq.h,Color::new(0.0,0.0,0.0,0.48));
            chrome::shadow_centered(tr("Not fighting"),sq.center().x,sq.bottom()-5.0*k,10.0*k,chrome::ORANGE_TEXT);
        }
        // The original's signs from the top left (493a64): the promotion, then a drunk potion.
        // Razdor's deviation (the owner's decision): no helmet sign on the hero's card, which has
        // its own coloured field; Razdor drew the helmet here, not the original.
        let upgrade = i > 0 && v.promotion_sign(&c);
        chrome::card_signs(sq, true, &[(upgrade, "Sign-Upgrade", GREEN), (!v.potions.is_empty(), "sign-potion", GREEN)]);
        super::spell_badges::draw(sq, &v.spells, v.drain, game.clock.total_minutes() as u64, &c);
        if let Some(fx) = CARD_FX.with(|f| f.get()).filter(|fx| fx.unit == i) {
            super::battle_view::draw_effect(&c, fx.effect, sq, (now_ms() - fx.t0_ms) as f32 / fx.ms as f32);
        }
        // The promoted unit's card darkened under the running `exp` strip (4b05a8).
        if let Some(p) = promo.filter(|p| p.unit == i) {
            chrome::xp_glow(sq, chrome::xp_strip_row(now_ms() - p.t0_ms));
        }
        if super::unit_drag::dragged() == Some(i) {
            draw_rectangle(p.x, p.y, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.55));
        }
        let over = mouse_in(p.x, p.y, card.x, card.y);
        if over {
            card_under = Some(i);
        }
        // An item held over a card lights it (0x4c280c, 0x4c2f54, 0x4c57bc).
        if over && game.held_item.is_some() {
            chrome::card_highlight(sq, held_card_lit(game.held_refusal(i).is_some()), now_ms() as u64);
        } else if selected.selected == Some(i) {
            chrome::glow_frame(sq, Color::new(0.35, 1.0, 0.35, 1.0), true);
        } else if over {
            chrome::glow_frame(sq, Color::new(0.35, 0.55, 1.0, 0.9), false);
        }
        if over && clicked() {
            pressed = Some((Some(i), v.slot));
        }
    }
    // The hint line (0x4c2f54): `[Army]` line 0 over the selected unit, line 2 over another
    // one while a unit is selected (naming the selected one), line 1 over a unit with none
    // selected, the quoted name in #NAME1; over an empty cell its row's text by the fixed
    // card numbers (Line1 for cards 1–4, Line2 for 7–10, Line3 for 0, 5, 6, 11). A personal
    // item under the pointer says it is his own (0x4c280c).
    // Hovering or pressing a card puts its own unit back on the left panel.
    if card_under.is_some() {
        PANEL_TYPE.with(|p| p.set(None));
        PANEL_UNIT.with(|u| u.set(None));
    }
    let pointed = if let Some(t) = tree_hint {
        Some(t)
    } else if personal_hover && !showing_type {
        chrome::ui_text("Army", "ItemI").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).or_else(|| Some(tr("His own item: it cannot be taken off while he lives.").to_string()))
    } else {
        match card_under {
            Some(i) => {
                let (line, named) = card_line(game.held_item.is_some(), super::unit_drag::controls(), i, selected.selected);
                army_line(line, &game.squad_label(named))
            }
            None => empty_under.and_then(|slot| row_text(f, slot)),
        }
    };
    // Holding an item over a card with a refusal shows the refusal (0x4c280c, `Item<n>`) in
    // the warning colour, before any press; a press's message stays until the next press.
    let hover_refusal = card_under.filter(|_| game.held_item.is_some()).and_then(|i| game.held_refusal(i)).map(equip_error);
    // The refusal's line is the warning ink (ae24f8), a press's refusal too while the item is held.
    let warn = game.held_item.is_some() && (message.is_some() || hover_refusal.is_some());
    let (hint, hc) = match (message.as_ref().or(hover_refusal.as_ref()), pointed) {
        (Some(m), _) => (m.clone(), held_hint_ink(warn)),
        (None, Some(p)) => (p, Color::new(1.0, 0.55, 0.25, 1.0)),
        (None, None) if show_tree => (tr("Click a unit to select it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
        // Clicks only: the original's line; with dragging too (Razdor's), it says a unit is dragged.
        (None, None) if super::unit_drag::controls() == Controls::Clicks => (tr("Click an item to pick it up, then a unit, a slot or a cell to put it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
        (None, None) => (tr("Click an item to pick it up, then a unit to give it; drag a unit to another cell to move it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
    };
    chrome::army_divider_fill(strip);
    super::dt_font::with_native_metrics(|| {
        let ink = if super::dt_font::native_metrics() && message.is_none() && hover_refusal.is_none() { super::typography::Ink::Green.color() } else { hc };
        chrome::hint_text(strip, &hint, ink);
    });
    // The press, as the original's (0x4c346c), ignored while a card slides (busy 0x68dc63).
    // With «Расстановка армии: Перетаскивание» (Razdor's default, the owner's decision) it
    // only selects (no swap or slide; units move by dragging), where the original's second
    // press moves the unit.
    if let (Some((Some(to), _)), None, true) = (pressed, slide, game.held_item.is_some()) {
        // An item held, pressed on a unit's card: it is given to him (0x4c346c → 0x4979c4);
        // no selection, swap or slide.
        *message = card_press(game, to);
    } else if let (Some((on, slot)), None) = (pressed, slide) {
        *message = None;
        match selected.press(super::unit_drag::controls(), on) {
            GridPress::Swap { selected: s, pressed: p } => {
                // At once, no slide; no drag starts from this press.
                cue(Cue::CardMove);
                game.move_unit(s, game.squad[p].slot);
            }
            GridPress::Slide(s) => {
                cue(Cue::CardMove);
                let from = cell_at(game.squad[s].slot);
                let ms = super::unit_drag::slide_ms(from, cell_at(slot), k);
                game.move_unit(s, slot);
                SLIDE.with(|c| c.set(Some(Slide { unit: s, from, t0_ms: now_ms(), ms })));
            }
            // Razdor's drag: a card pressed and moved while held goes to another cell.
            GridPress::Select(_) | GridPress::Deselect => {
                if let Some(i) = on {
                    super::unit_drag::press(i, game.squad[i].def, (game.squad[i].hp, game.squad[i].max_hp(&c)));
                }
            }
            GridPress::Nothing => {}
        }
    }
    let cells: Vec<(razdor::rules::formation::Slot, Rect)> = f.slots().map(|s| (s, Rect::new(cell_at(s).x, cell_at(s).y, card.x, card.y))).collect();
    // A slide over the cards drags a unit, not the window (phones held upright, `ui::pan`).
    super::pan::drag_grid(cells.iter().map(|&(_, r)| r));
    if let Some((unit, slot)) = super::unit_drag::update(assets, &cells, card) {
        game.move_unit(unit, slot);
        // A drag (Razdor's) ends as the original's swap: nothing selected, the pack up.
        *selected = ArmySel::default();
    }

    window_layout::frame(win);

    // The item under the pointer, in the pack or worn: its description pops up by the pointer
    // (not while one is held: 0x4c280c shows the item text with nothing held). Drawn after the
    // frame lines, so the window's steel lines do not cross it. On a phone it shows on a long
    // press only (`input::hint_hover`).
    if let Some(item) = hover.filter(|_| game.held_item.is_none() && super::input::hint_hover()) {
        // The original's item tooltip (0x4cbab0): 272 wide, as tall as its text plus 32.
        let (pw, ph) = super::item_text::tooltip_size(game, item, k);
        let (mx, my) = pointer();
        let px = if mx + 21.0 * k + pw > sw { mx - 21.0 * k - pw } else { mx + 21.0 * k };
        let px = px.clamp(6.0, (sw - pw - 6.0).max(6.0));
        let py = (my + 37.0 * k).min(sh - ph - 4.0).max(4.0);
        draw_rectangle(px + 6.0 * k, py + 6.0 * k, pw, ph, Color::new(0.0, 0.0, 0.0, 0.45));
        super::item_text::draw(game, assets, item, Rect::new(px, py, pw, ph), super::item_text::ItemBox::Tooltip, k);
    }
    // The group button's name, queued by the row (drawn after the frame, as the item's text).
    draw_later();
    // The item held is the pointer; closing the window puts it back in the pack (the
    // screen change in `App::frame`, 0x4b8d28).
    draw_held(game, assets);

    if close || (!filter.keys_taken && (key(KeyCode::Escape) || key(keymap::key(Action::HeroArmy)))) {
        FILTER.with(|f| f.borrow_mut().clear());
        super::unit_drag::cancel();
        SLIDE.with(|c| c.set(None));
        PANEL_UNIT.with(|u| u.set(None));
        *message = None;
        return Some(match back {
            Some(v) => Screen::Building(v.clone()),
            None => Screen::WorldMap,
        });
    }
    // The bar's army button closes the screen too (back to the building it came from).
    let next = match bar {
        Some(Screen::WorldMap) => {
            *message = None;
            Some(back.clone().map_or(Screen::WorldMap, Screen::Building))
        }
        other => other,
    };
    if next.is_some() {
        SLIDE.with(|c| c.set(None));
        PANEL_UNIT.with(|u| u.set(None));
        super::unit_drag::cancel();
        FILTER.with(|f| f.borrow_mut().clear());
    }
    next
}

/// The unit's potions still working, as its panel's status line (blue).
/// The wage line of a promotion portrait's class card: the original's fresh unit of that type
/// is painted in a game, so 4924c9 writes its recruit wage (4a163c of the type's Cost), as
/// for a hire offer; none for a hero class or an Undead type.
pub(super) fn class_wage(c: &Content, id: UnitId) -> i32 {
    let undead = c.unit(id).nature == razdor::dt::data::Nature::Undead;
    if razdor::rules::content::HeroClass::of_unit(id).is_some() || undead { 0 } else { c.wage(id) }
}

/// Whether the army window's panel shows the four worn-item slots: only for the unit itself.
/// A promotion portrait hovered (0x4c39d8) paints its type as a template (492f24 with the
/// fourth argument set), and a template skips the slot loop altogether, so no slot, empty
/// or filled, is drawn on the previewed type's card.
/// The stacked army and hero windows' «Закрыть» under the formation, in thumb reach: the same
/// close as the title's cross (`true` when pressed). Landscape has none.
fn close_button(win: Rect) -> bool {
    if !window_layout::portrait() {
        return false;
    }
    let Some(r) = bottom_row_rects(win).close else { return false };
    super::widgets::button(r.x, r.y, r.w, r.h, tr("Close"), true)
}

/// The phone's bottom buttons of the hero window: «Весь список» (`whole`), «Закрыть» (`close`:
/// held upright only, sideways the window has none) and «Развернуть» (`expand`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct BottomRow {
    whole: Rect,
    close: Option<Rect>,
    expand: Rect,
}

/// The phone's bottom buttons of the hero window: its height is the stacked close's
/// (`window_layout::army_close`), on the close's line when held upright and at the window's foot
/// when sideways. Sideways they stack under the diary's column ([`sideways_row`]), so they never
/// cover the hero's panel.
fn bottom_row_rects(win: Rect) -> BottomRow {
    let k = chrome::k();
    let base = window_layout::army_close(win);
    if window_layout::portrait() {
        let (whole, close, expand) = bottom_row(win, base.y, base.h, 12.0 * k, 8.0 * k);
        return BottomRow { whole, close: Some(close), expand };
    }
    let (whole, expand) = sideways_row(win, k, win.bottom() - base.h - 8.0 * k, base.h);
    BottomRow { whole, close: None, expand }
}

/// Sideways, the phone's diary buttons stack under the diary's column (a phone choice, not in the
/// original), so that their labels fit the narrow column: «Развернуть» at `y`, `h` tall, and
/// «Весь список» above it with a gap of 8 px. Both are as wide as the entry box (design x 576 to
/// 868 of the hero view's `o` mapping: the origin 248 plus (x - 264) times `HERO_S`). Returns
/// (whole list, expand). Pure, so the test can check it without the screen's state.
fn sideways_row(win: Rect, k: f32, y: f32, h: f32) -> (Rect, Rect) {
    let left = win.x + (248.0 + (576.0 - 264.0) * HERO_S) * k;
    let right = win.x + (248.0 + (868.0 - 264.0) * HERO_S) * k;
    let expand = Rect::new(left, y, right - left, h);
    let whole = Rect::new(left, y - 8.0 * k - h, right - left, h);
    (whole, expand)
}

/// The sideways diary entry box `entry` shortened to end 4 px above the phone's upper bottom
/// button (`row.whole`), so the button does not cover its text (a phone choice, not in the original).
fn entry_above_row(entry: Rect, row: &BottomRow, k: f32) -> Rect {
    Rect::new(entry.x, entry.y, entry.w, entry.h.min((row.whole.y - 4.0 * k - entry.y).max(0.0)))
}

/// The unit's panel of the army and hero windows: the landscape column on the left, or stacked
/// (a phone held upright) the band under the title, its picture left and its stats right.
fn draw_sheet(assets: &Assets, c: &razdor::rules::content::Content, win: Rect, sheet: &unit_sheet::Sheet, slots: bool, hover: &mut Option<razdor::rules::content::ItemId>) -> Option<usize> {
    if window_layout::portrait() {
        unit_sheet::draw_band(assets, c, window_layout::army_band(win), sheet, slots, hover)
    } else {
        unit_sheet::draw(assets, c, window_layout::left(win), sheet, slots, hover)
    }
}

fn panel_slots(showing_type: bool) -> bool {
    !showing_type
}

pub(super) fn potion_status(c: &Content, u: &Unit) -> Vec<(String, Color)> {
    if u.potions.is_empty() {
        return Vec::new();
    }
    let names: Vec<&str> = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect();
    vec![(razdor::trf!("Until the next battle: {names}", names = names.join(", ")), chrome::BLUE_TEXT)]
}

/// The `[Army]` hint line over the card of unit `under` with `selected` selected, and whose
/// name it quotes: as the original's (0x4c2f54, 0x4c612c) line 0 "click to deselect" over the
/// selected unit, line 2 "click to move" (naming the selected one) over another, line 1
/// "click to select" with none selected. With dragging only (Razdor's setting, the owner's
/// decision) a click only selects, so every card says line 1 with its own name.
pub(super) fn pointed_line<T: PartialEq + Copy>(controls: super::unit_drag::Controls, under: T, selected: Option<T>) -> (usize, T) {
    match (controls, selected) {
        (super::unit_drag::Controls::Drag, _) | (_, None) => (1, under),
        (_, Some(s)) if s == under => (0, s),
        (_, Some(s)) => (2, s),
    }
}

/// The state a card takes while an item is held over it (4c2f54 sets state 4 for a refusal,
/// else state 2; 4c57bc draws them): red (4) where the unit refuses, blue (2) where it takes it.
/// The state table at 0xadd194 is read in `chrome::Lit`; the original's green card is the
/// current unit (state 5), not the acceptance.
pub(super) fn held_card_lit(refused: bool) -> chrome::Lit {
    if refused { chrome::Lit::Enemy } else { chrome::Lit::CanAct }
}

/// The hint line's ink while an item is held (4c2f54): the warning ink for a refusal, the
/// normal line colour otherwise.
pub(super) fn held_hint_ink(warn: bool) -> Color {
    if warn { chrome::WARNING_INK } else { chrome::GOLD }
}

/// The card's hint line while an item is held: line 3 offers the item to the unit under the
/// pointer (0x4c2f54, the acceptance); with nothing held it is [`pointed_line`].
pub(super) fn card_line<T: PartialEq + Copy>(held: bool, controls: super::unit_drag::Controls, under: T, selected: Option<T>) -> (usize, T) {
    if held { (3, under) } else { pointed_line(controls, under, selected) }
}

/// `[Army]` hint line `n` with `name` quoted in #NAME1 (0x4c2f54), in a Russian game only.
pub(super) fn army_line(n: usize, name: &str) -> Option<String> {
    chrome::ui_line("Army", n).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).map(|t| t.replace("#NAME1", &format!("\"{name}\"")))
}

/// The hint over an empty cell: its row's text by the fixed card numbers (Line1 for cards
/// 1–4, Line2 for 7–10, Line3 for 0, 5, 6, 11), in a Russian game only.
pub(super) fn row_text(f: razdor::rules::formation::Formation, slot: razdor::rules::formation::Slot) -> Option<String> {
    let key = match unit_sheet::strip_place(f, slot) {
        1..=4 => "Line1",
        7..=10 => "Line2",
        _ => "Line3",
    };
    chrome::ui_text("Army", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru)
}

/// The hero window's state (side window 5, built by 0x4d8638): the pack's scroll, the
/// diary's tab, its picked entry (0x6668bf) and how far its list and its entry are scrolled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroView {
    pub scroll: usize,
    /// The diary's tab: the quests under way (the original's diary), or Razdor's history of
    /// completed quests, rumours and messages (asked for by the owner).
    pub tab: Tab,
    pub pick: usize,
    pub list_scroll: usize,
    pub text_scroll: usize,
    /// The phone's diary reader is open (a tap on the entry): the entry's full text over almost
    /// the whole screen, scrolled by `reader_scroll`.
    pub reader: bool,
    pub reader_scroll: usize,
    /// The phone's list reader is open («Весь список»): every line of the tab over the screen.
    /// An entry opened from it (`reader`) returns to it when closed.
    pub list_reader: bool,
    pub list_reader_scroll: usize,
}

impl Default for HeroView {
    fn default() -> Self {
        HeroView { scroll: 0, tab: Tab::Active, pick: 0, list_scroll: 0, text_scroll: 0, reader: false, reader_scroll: 0, list_reader: false, list_reader_scroll: 0 }
    }
}

impl HeroView {
    /// A phone's reader is open (the entry's or the whole list's): the window below takes no input.
    fn reading(&self) -> bool {
        self.reader || self.list_reader
    }
}

/// The phone's bottom row of the hero window, one line of height `h` at `y` inside `win` (`pad`
/// from each side, `gap` between): «Весь список» left, «Закрыть» centred (about 36% of the row),
/// «Развернуть» right. Returns the three rects in that order.
fn bottom_row(win: Rect, y: f32, h: f32, pad: f32, gap: f32) -> (Rect, Rect, Rect) {
    let row = (win.w - 2.0 * pad).max(0.0);
    let close_w = row * 0.36;
    let side_w = ((row - close_w) / 2.0 - gap).max(0.0);
    let left = Rect::new(win.x + pad, y, side_w, h);
    let close = Rect::new(left.right() + gap, y, close_w, h);
    let right = Rect::new(close.right() + gap, y, side_w, h);
    (left, close, right)
}

/// «Развернуть» is enabled when an entry is shown (`pick` is a row of the tab).
fn expand_entry_enabled(rows_len: usize, pick: usize) -> bool {
    pick < rows_len
}

/// «Весь список» is enabled when the tab has entries.
fn whole_list_enabled(rows_len: usize) -> bool {
    rows_len > 0
}

/// The hero window's right side is laid out in the original's pixels (window-relative, x from
/// 264, y from 28) scaled by this: the right part of Razdor's window (586 px) over the
/// original's (632 px).
const HERO_S: f32 = 586.0 / 632.0;

/// The hero window's right column, top down: the artifact description box (`desc`), the quest
/// list (`list`) and the diary entry (`entry`). `desc` is `None` when it is not drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
struct DiaryColumn {
    desc: Option<Rect>,
    list: Rect,
    entry: Rect,
}

/// The hero window's right column (the original's three boxes, 0x49c388 and 0x4d8638, at
/// `desc`, `list` and `entry`). On a phone, while a diary entry is shown and no artifact is
/// described, the description box is not drawn: the quest list moves up to its top and the
/// entry takes the description box's height and the gap under it (Razdor's phone choice: the
/// entry's text is larger there). The desktop and a described artifact keep the three boxes.
fn diary_column(desc: Rect, list: Rect, entry: Rect, phone: bool, described: bool, entry_shown: bool) -> DiaryColumn {
    if !phone || described || !entry_shown {
        return DiaryColumn { desc: Some(desc), list, entry };
    }
    // The description box's height and the gap under it: the list's top less the box's top.
    let up = list.y - desc.y;
    DiaryColumn { desc: None, list: Rect::new(list.x, list.y - up, list.w, list.h), entry: Rect::new(entry.x, entry.y - up, entry.w, entry.h + up) }
}

/// The rows of the hero window's diary for `tab`. The quests under way, oldest first, are the
/// original's list (0x68e500, a quest added by 0x49c170 as it is received and taken out by
/// 0x49c2b8 when it is completed); the other tabs are Razdor's history (asked for by the
/// owner): completed quests, rumours and messages of the whole campaign, newest first.
fn diary_rows(game: &Game, tab: Tab) -> Vec<razdor::rules::journal::Row> {
    let mut rows = game.journal_rows(tab);
    if tab == Tab::Active {
        rows.reverse();
    }
    rows
}

/// A diary tab's short title (the four share the width of the original's list title).
fn diary_tab_label(tab: Tab) -> &'static str {
    match tab {
        Tab::Active => n_("Quests"),
        Tab::Completed => n_("Done"),
        Tab::Rumours => n_("Rumours"),
        Tab::Messages => n_("Messages"),
    }
}

/// One line group of a diary entry, in the order it is drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EntryPart {
    /// The quest's title (white, centred).
    Title,
    /// An empty line.
    Blank,
    /// `[Hero] QuestFullDescript` (centred), the heading over the description.
    Description,
    /// The text (justified), one paragraph per line of the quest's text.
    Text,
    /// `[Hero] QuestGetTime` (centred), the heading over the time since the quest was received.
    TimeHeading,
    /// The time since the quest was received (centred).
    Time,
    /// A history entry's date (centred): when the quest was completed, or when the rumour or
    /// message happened.
    Date,
}

/// The parts of a diary entry, per tab. The desktop keeps the original's layout (0x49c388 for
/// the quests under way). On a phone (Razdor's choice, the player's suggestion) the quest's
/// title and the «Описание:» heading are dropped: the quest list above already names the quest,
/// and the box is small. The time line stays at the end. A completed quest on a phone keeps its
/// completion date and its text. Rumours and messages keep their title on a phone.
fn entry_parts(tab: Tab, phone: bool) -> Vec<EntryPart> {
    use EntryPart::*;
    match (tab, phone) {
        (Tab::Active, false) => vec![Title, Blank, Description, Text, Blank, TimeHeading, Time],
        (Tab::Active, true) => vec![Text, Blank, TimeHeading, Time],
        (Tab::Completed, true) => vec![Date, Blank, Text],
        _ => vec![Title, Blank, Date, Blank, Text],
    }
}

/// A diary entry's lines for `tab` (the quests under way, or a history tab), each with its font,
/// colour and size, in the order of [`entry_parts`]. The fonts are 4dbc26's: the title in white
/// Benguiat 0xae2498, the headings in SanSerif_Bold less 20, 90, 180, an orange, 0xae24f4, the
/// text in SanSerif less 25, 25, 125, a yellow, 0xae24c0; a history entry's date is in the
/// heading's orange and font.
fn diary_lines(row: &razdor::rules::journal::Row, tab: Tab, phone: bool, room: f32, title: f32, body: f32) -> Vec<DiaryLine> {
    diary_lines_of(row, tab, &entry_parts(tab, phone), room, title, body)
}

/// [`diary_lines`] for the given `parts` (the phone's entry, or the reader's full one).
fn diary_lines_of(row: &razdor::rules::journal::Row, tab: Tab, parts: &[EntryPart], room: f32, title: f32, body: f32) -> Vec<DiaryLine> {
    use super::dt_font::{with_original_face, Face};
    use razdor::dt::markup::{Align, Ink};
    let own = |key: &str, ours: &'static str| chrome::ui_text("Hero", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
    let rows = |s: &str, ink: Color, align: Align, face: Face, size: f32| -> Vec<DiaryLine> {
        with_original_face(face, || {
            let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
            razdor::dt::markup::wrap(s, room, width).into_iter().map(|r| DiaryLine { row: MarkupRow { text: r.text, ink: Ink::Plain, align, last: r.last }, face, color: ink, size }).collect()
        })
    };
    let (white, orange, yellow) = (Color::from_rgba(255, 255, 255, 255), Color::from_rgba(235, 165, 75, 255), Color::from_rgba(230, 230, 130, 255));
    let blank = || DiaryLine { row: MarkupRow { text: String::new(), ink: Ink::Plain, align: Align::Centre, last: true }, face: Face::Body, color: yellow, size: body };
    let mut out = Vec::new();
    for &part in parts {
        match part {
            EntryPart::Title => out.extend(rows(&row.title, white, Align::Centre, Face::Subtitle, title)),
            EntryPart::Blank => out.push(blank()),
            EntryPart::Description => out.extend(rows(&own("QuestFullDescript", n_("Description:")), orange, Align::Centre, Face::Bold, body)),
            EntryPart::Text => {
                for line in row.text.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    out.extend(rows(&format!("{}{line}", razdor::dt::markup::INDENT), yellow, Align::Justify, Face::Body, body));
                }
            }
            EntryPart::TimeHeading => out.extend(rows(&own("QuestGetTime", n_("Time since the quest was received:")), orange, Align::Centre, Face::Bold, body)),
            EntryPart::Time => {
                let since = row.elapsed.map_or_else(|| tr("unknown").to_string(), |m| razdor::rules::clock::duration_label(m as f64));
                out.extend(rows(&since, yellow, Align::Centre, Face::Body, body));
            }
            EntryPart::Date => {
                let date = row.date.map_or_else(|| tr("Date unknown").to_string(), |d| d.label());
                let when = if tab == Tab::Completed { format!("{}: {date}", tr(super::story::QUEST_COMPLETED)) } else { date };
                out.extend(rows(&when, orange, Align::Centre, Face::Bold, body));
            }
        }
    }
    out
}

/// A line of the diary entry with its font, colour and size.
struct DiaryLine {
    row: MarkupRow,
    face: super::dt_font::Face,
    color: Color,
    size: f32,
}

/// The resource values of the hero window (4d1814): gold, mana, "+ income", "- wages", each
/// zero written "0".
fn hero_resources(gold: i32, mana: i32, income: i32, wages: i32) -> [String; 4] {
    [
        gold.to_string(),
        mana.to_string(),
        if income == 0 { "0".to_string() } else { format!("+ {income}") },
        if wages == 0 { "0".to_string() } else { format!("- {wages}") },
    ]
}

/// Share of the message windows' text size (`Face::Subtitle`) that the phone's diary body takes
/// in the hero window. A Razdor choice (the owner's decision, 2026-10-30): at the message size
/// the body was larger than the rest of the window.
const DIARY_PHONE_BODY: f32 = 0.8;

/// The diary entry's line size (`QuestDescript`, its body text). The original draws the body in
/// its small SanSerif (`Face::Body`) and the desktop keeps that. A Razdor choice for phones (the
/// players' request: the phone's diary text is too small): there the body is
/// `DIARY_PHONE_BODY` of the message windows' Benguiat text (`Face::Subtitle`,
/// `dialog::event_layout`), so it grows with «Размер текста» like they do.
fn diary_body_size(phone: bool, ts: f32) -> f32 {
    use super::dt_font::{sheet_size, Face};
    if phone {
        DIARY_PHONE_BODY * sheet_size(Face::Subtitle, ts)
    } else {
        sheet_size(Face::Body, ts)
    }
}

/// The phone's full-screen diary reader («Развернуть») keeps the message windows' size
/// (`Face::Subtitle`), not the smaller hero-window body (`diary_body_size`).
fn diary_reader_body_size(ts: f32) -> f32 {
    use super::dt_font::{sheet_size, Face};
    sheet_size(Face::Subtitle, ts)
}

/// The diary entry's title size: on a phone it is as large as the quest list's titles (`list_t`),
/// the desktop keeps the entry's own heading size (`head * ts / s`).
fn diary_title_size(phone: bool, list_t: f32, head: f32, ts: f32, s: f32) -> f32 {
    if phone {
        list_t
    } else {
        head * ts / s
    }
}

/// A quest row's text size on a phone: the largest size in the 0.9-step sequence from `full` whose
/// width (`width_at`) fits `room`, never below `floor` (the original's title size). A text that
/// fits at `full` keeps it; one that never fits gets `floor`, and the caller cuts it there.
fn fit_row_size(full: f32, floor: f32, width_at: impl Fn(f32) -> f32, room: f32) -> f32 {
    let mut size = full;
    while width_at(size) > room && size > floor {
        size = (size * 0.9).max(floor);
    }
    size
}

/// The phone diary reader's margin from the screen's edges and its safe area, px.
const READER_MARGIN: f32 = 8.0;

/// The parts the phone's diary reader shows: the desktop's full entry (the reader has the room,
/// so the title and the «Описание:» heading come back), for every tab.
fn reader_parts(tab: Tab) -> Vec<EntryPart> {
    entry_parts(tab, false)
}

/// The phone diary reader's window: the whole screen less the safe area and [`READER_MARGIN`] on
/// each side. It covers the bottom bar too: it is a window of its own.
fn diary_reader_rect((sw, sh): (f32, f32), safe: super::display::SafeArea) -> Rect {
    let m = READER_MARGIN;
    Rect::new(safe.left + m, safe.top + m, (sw - safe.left - safe.right - 2.0 * m).max(0.0), (sh - safe.top - safe.bottom - 2.0 * m).max(0.0))
}

/// The phone's diary reader (a tap on the entry, the owner's and a player's request): the entry's
/// full text ([`reader_parts`]) at the phone's sizes (`title`, `body`), in a window over almost the
/// whole screen, scrolled by drag and the wheel. Its close button, its window's cross or Esc closes
/// it: returns whether it closed this frame.
fn diary_reader(row: &razdor::rules::journal::Row, tab: Tab, title: f32, body: f32, scroll: &mut usize) -> bool {
    let k = chrome::k();
    let win = diary_reader_rect((screen_width(), screen_height()), super::display::safe_area());
    let (inner, closed) = chrome::window(win, tr(n_("Diary entry")), chrome::Skin::Marble, true);
    let pad = 8.0 * k;
    let bar_w = 18.0 * k;
    // A finger-sized close button (48 dp on a phone), centred under the text.
    let btn_h = super::display::dp_px().map_or(40.0 * k, |dp| 48.0 * dp);
    let body_y = inner.y + pad;
    let body_h = (inner.h - btn_h - 3.0 * pad).max(0.0);
    let bar = Rect::new(inner.right() - pad - bar_w, body_y, bar_w, body_h);
    let text = Rect::new(inner.x + pad, body_y, (bar.x - 4.0 * k - (inner.x + pad)).max(0.0), body_h);
    let room = text.w - 16.0 * k;
    let lines = super::dt_font::legacy_metrics_if(true, || diary_lines_of(row, tab, &reader_parts(tab), room, title, body));
    let line_h = (body * 1.15).round();
    let shown = ((text.h - 12.0 * k) / line_h).floor().max(1.0) as usize;
    chrome::text_box_base(text);
    swipe_rows(text, line_h);
    if mouse_in(text.x, text.y, text.w, text.h) {
        let w = wheel();
        if w < 0.0 && *scroll + shown < lines.len() {
            *scroll += 1;
        } else if w > 0.0 {
            *scroll = scroll.saturating_sub(1);
        }
    }
    *scroll = (*scroll).min(lines.len().saturating_sub(shown));
    scrollbar_always(bar, scroll, shown, lines.len());
    chrome::text_box_sides(text);
    chrome::silver_frame(text, k.max(1.0));
    super::dt_font::legacy_metrics_if(true, || {
        for (n, line) in lines.iter().skip(*scroll).take(shown).enumerate() {
            let y = text.y + 6.0 * k + n as f32 * line_h + body * 0.85;
            draw_markup_row_face(&line.row, text.x + 8.0 * k, y, room, line.size, line.face, Some(line.color));
        }
    });
    let bw = (inner.w * 0.5).min(360.0 * k);
    let pressed = button(inner.center().x - bw / 2.0, inner.bottom() - pad - btn_h, bw, btn_h, tr("Close"), true);
    closed || pressed || key(KeyCode::Escape)
}

/// The phone's list reader («Весь список», the owner's and a player's request): every line of the
/// tab (`lines`, the list's own: chapter headings and rows) over the same window as the entry
/// reader, rows at the list's phone size (`list_t`, shrunk to `row_floor` before they are cut),
/// scrolled by drag and the wheel. A tap on a row returns its index (the caller opens its entry);
/// Close, the cross, Esc or Back close it: returns `(closed, picked row)`.
fn diary_list_reader(game: &Game, rows: &[razdor::rules::journal::Row], lines: &[super::story::Line], tab: Tab, (list_t, row_floor): (f32, f32), scroll: &mut usize, pick: usize) -> (bool, Option<usize>) {
    use super::dt_font::{legacy_metrics_if, sheet_size, with_original_face, Face};
    use razdor::dt::markup::Ink;
    let k = chrome::k();
    let win = diary_reader_rect((screen_width(), screen_height()), super::display::safe_area());
    let (inner, closed) = chrome::window(win, tr(diary_tab_label(tab)), chrome::Skin::Marble, true);
    let pad = 8.0 * k;
    let bar_w = 18.0 * k;
    // The same finger-sized close as the entry reader's (48 dp on a phone).
    let btn_h = super::display::dp_px().map_or(40.0 * k, |dp| 48.0 * dp);
    let body_y = inner.y + pad;
    let body_h = (inner.h - btn_h - 3.0 * pad).max(0.0);
    let bar = Rect::new(inner.right() - pad - bar_w, body_y, bar_w, body_h);
    let list = Rect::new(inner.x + pad, body_y, (bar.x - 4.0 * k - (inner.x + pad)).max(0.0), body_h);
    let row_h = (list_t * 1.25).round();
    let fits = ((list.h - 8.0 * k) / row_h).floor().max(1.0) as usize;
    chrome::text_box_base(list);
    swipe_rows(list, row_h);
    if mouse_in(list.x, list.y, list.w, list.h) {
        let w = wheel();
        if w < 0.0 && *scroll + fits < lines.len() {
            *scroll += 1;
        } else if w > 0.0 {
            *scroll = scroll.saturating_sub(1);
        }
    }
    *scroll = (*scroll).min(lines.len().saturating_sub(fits));
    scrollbar_always(bar, scroll, fits, lines.len());
    let first = *scroll;
    let room = list.w - 12.0 * k;
    if let Some(n) = lines.iter().position(|l| *l == super::story::Line::Row(pick)).filter(|n| (first..first + fits).contains(n)) {
        let ry = list.y + 4.0 * k + (n - first) as f32 * row_h;
        draw_rectangle(list.x + 2.0 * k, ry, list.w - 4.0 * k, row_h, Color::new(0.15, 0.15, 0.15, 0.6));
    }
    chrome::text_box_sides(list);
    chrome::silver_frame(list, k.max(1.0));
    if rows.is_empty() {
        with_original_face(Face::Body, || chrome::shadow_centered(super::story::empty_note(tab), list.x + list.w / 2.0, list.y + list.h / 2.0, sheet_size(Face::Title, k) * 0.9, DIM));
    }
    let mut picked = None;
    legacy_metrics_if(true, || {
        for (n, line) in lines.iter().enumerate().skip(first).take(fits) {
            let ry = list.y + 4.0 * k + (n - first) as f32 * row_h;
            let center = list.x + list.w / 2.0;
            match *line {
                super::story::Line::Heading(chapter) => {
                    let heading = game.journal.chapter_heading(chapter);
                    let size = fit_row_size(list_t, row_floor, |z| with_original_face(Face::Bold, || measure(&heading, z).width), room);
                    with_original_face(Face::Bold, || {
                        let text = ellipsize(&heading, room, size);
                        chrome::shadow_centered(&text, center, ry + row_h * 0.78, size, Color::from_rgba(235, 165, 75, 255));
                    });
                }
                super::story::Line::Row(r) => {
                    let title = &rows[r].title;
                    let size = fit_row_size(list_t, row_floor, |z| with_original_face(Face::Title, || measure(title, z).width), room);
                    with_original_face(Face::Title, || {
                        let text = ellipsize(title, room, size);
                        chrome::shadow_centered(&text, center, ry + row_h * 0.78, size, ink_color(Ink::Plain));
                    });
                    if mouse_in(list.x, ry, list.w, row_h) && ry >= list.y && ry + row_h <= list.bottom() && clicked() {
                        picked = Some(r);
                    }
                }
            }
        }
    });
    let bw = (inner.w * 0.5).min(360.0 * k);
    let pressed = button(inner.center().x - bw / 2.0, inner.bottom() - pad - btn_h, bw, btn_h, tr("Close"), true);
    (closed || pressed || key(KeyCode::Escape), picked)
}

/// The hero window («Личные характеристики и инвентарь Героя»), the bottom bar's flag (side
/// window 5; built by 0x4d8638, its fixed parts drawn by 0x4d1814): the hero's panel on the
/// left as in the army window; the pack's 5×5 squares with its scroll bar; the item
/// description box (`ItemDescript`); the four resource lines (4d1814: `ResFrame` with its
/// colours shifted by 0, −15, −22; `res0`–`res3` with their own alpha (0x4dcfd1); the label, line
/// 0–3 of `[Hero]`, and 20 px right of and below it the value); and the diary: its list
/// (`QuestList`, the quests under way) over the picked one's entry (`QuestDescript`,
/// 0x49c388). Razdor's journal (all quests, rumours and messages of the campaign) stays on J.
pub fn hero_window(game: &mut Game, assets: &Assets, view: &mut HeroView, message: &mut Option<String>) -> Option<Screen> {
    use razdor::dt::markup::Ink;
    use super::dt_font::{sheet_size, with_original_face, Face};
    // A phone's diary reader (`view.reader`) covers this window: the window below takes no input
    // while it is open (the reader's own input is taken at its end).
    let phone = super::display::dp_px().is_some();
    let below_blocked = input_blocked();
    let reading = phone && view.reading();
    if reading {
        set_input_blocked(true);
    }
    let bar = super::world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Hero));
    let c = game.content.clone();
    let k = chrome::k();
    let ru = razdor::i18n::lang() == razdor::i18n::Lang::Ru;
    let own = |key: &str, ours: &'static str| chrome::ui_text("Hero", key).filter(|_| ru).unwrap_or_else(|| tr(ours).to_string());
    let win = window_layout::window();
    let title = own("Title", n_("The hero's characteristics and inventory"));
    let (_, close) = chrome::window(win, &title, chrome::Skin::Marble, true);
    let close = close_button(win) || close;
    // The original's right side, window-relative.
    let s = HERO_S * k;
    // Its origin is the right side's (248, 28) as the window places it (below the left column when stacked).
    let origin = window_layout::point(win, 248.0, 28.0);
    let o = |x: f32, y: f32, w: f32, h: f32| Rect::new(origin.x + (x - 264.0) * s, origin.y + (y - 28.0) * s, w * s, h * s);
    let mut hover = None;

    // The hero's panel, as the army window paints him (0x492f24).
    let u = game.squad[0].clone();
    let stats = u.stats(&c);
    let hero = razdor::rules::content::HeroClass::ALL.into_iter().find(|h| h.unit() == u.def);
    let label = game.squad_label(0);
    let sheet = unit_sheet::Sheet {
        kind: u.def,
        name: &label,
        named: false,
        level: u.level,
        xp: u.xp,
        need: u.xp_to_next(&c),
        hp: u.hp,
        now: &stats,
        start: &stats,
        bleed: 0,
        power: stats[Stat::MagicPower],
        wage: game.wage(0),
        items: u.items,
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        unpaid: u.unpaid,
        building: 0,
        hero,
        status: potion_status(&c, &u),
    };
    // A press on a worn slot picks its item up or puts the held one on (0x4c24f4; the hero
    // window shows the hero's slots: 0x4b8d28 selects him as it opens).
    if let Some(slot) = draw_sheet(assets, &c, win, &sheet, true, &mut hover) {
        *message = worn_press(game, 0, slot);
    }
    // Stacked (a phone held upright) the seam is drawn across the window (`window_layout::seam`).
    if window_layout::portrait() {
        window_layout::seam(win);
    } else {
        draw_line(win.x + 247.0 * k, win.y + 27.0 * k, win.x + 247.0 * k, win.y + win.h - 2.0, 1.5 * k, chrome::SILVER);
    }

    // The pack: 25 squares of 55 px from (274, 37), the scroll bar right of them.
    let pack = o(274.0, 37.0, 275.0 + 18.0 / HERO_S, 275.0);
    if let Some(cell) = pack_view(game, assets, pack, &mut view.scroll, &mut hover, None) {
        *message = None;
        *message = pack_click(game, cell);
    }

    // A box's title over it, as 0x48e750 draws them: centred, in Benguiat.
    let head = sheet_size(Face::Title, s);
    let heading = |key: &str, ours: &'static str, r: Rect| {
        let t = own(key, ours);
        with_original_face(Face::Title, || chrome::shadow_centered(&t, r.x + r.w / 2.0, r.y - 7.0 * s, head, ACCENT));
    };

    // The item description box (310 × 164 at 58): the item under the pointer. On a phone, with a
    // diary entry shown and no item described, the diary takes its place (`diary_column`).
    // On a phone the description shows on a long press only (`input::hint_hover`).
    let described = hover.filter(|_| game.held_item.is_none() && super::input::hint_hover());
    let entry_shown = diary_rows(game, view.tab).get(view.pick).is_some();
    let list0 = o(576.0, 252.0, 292.0, 128.0);
    let list_bar0 = o(868.0, 252.0, 18.0, 128.0);
    let entry0 = o(576.0, 410.0, 292.0, 220.0);
    let entry_bar0 = o(868.0, 410.0, 18.0, 220.0);
    let mut column = diary_column(o(576.0, 58.0, 310.0, 164.0), list0, entry0, phone, described.is_some(), entry_shown);
    // Sideways a phone's bottom buttons stack under the diary's right column (no «Закрыть» there):
    // the entry box gives up that strip, so «Весь список» does not cover its text.
    if phone && !window_layout::portrait() {
        column.entry = entry_above_row(column.entry, &bottom_row_rects(win), k);
    }
    if let Some(desc) = column.desc {
        heading("ItemDescript", n_("Item description"), desc);
        match described {
            Some(item) => super::item_text::draw(game, assets, item, desc, super::item_text::ItemBox::Inventory, s),
            None => chrome::text_box_no_corners(desc),
        }
    }

    // The four resource lines, 78 px apart from 325.
    let values = hero_resources(game.gold, game.mana, game.daily_income(), game.daily_wages());
    let ours = [n_("Gold reserve"), n_("Mana reserve"), n_("Income from castles"), n_("Army upkeep")];
    let frame = chrome::win_scaled("ResFrame", [1024; 3], [0, -15, -22]);
    for (i, value) in values.iter().enumerate() {
        let y = 325.0 + 78.0 * i as f32;
        if let Some(t) = &frame {
            chrome::tex(t, o(271.0, y, t.width(), t.height()), WHITE);
        }
        if let Some(t) = chrome::win(&format!("res{i}")) {
            chrome::tex(&t, o(280.0, y + 4.0, t.width(), t.height()), WHITE);
        }
        let name = chrome::ui_line("Hero", i).filter(|_| ru).unwrap_or_else(|| tr(ours[i]).to_string());
        let at = o(358.0, y + 17.0, 0.0, 0.0);
        with_original_face(Face::Title, || {
            chrome::shadow_text(&name, at.x, at.y + head * 0.8, head, ink_color(Ink::Plain));
            chrome::shadow_text(value, at.x + 20.0 * s, at.y + 20.0 * s + head * 0.8, head, ink_color(Ink::Star));
        });
    }

    // The diary: the list of the tab's entries over the picked one's entry. In place of the
    // list's title (`QuestList`) Razdor shows its tabs (asked for by the owner): the quests
    // under way (the original's diary), then the history of completed quests, rumours and
    // messages; Left / Right change them.
    let list = column.list;
    let list_bar = Rect::new(list_bar0.x, list.y, list_bar0.w, list.h);
    let list_box = Rect::new(list.x, list.y, list_bar.x + list_bar.w - list.x, list.h);
    let at = Tab::ALL.iter().position(|&t| t == view.tab).unwrap_or(0);
    let mut switch = None;
    let cell = list_box.w / Tab::ALL.len() as f32;
    for (i, tab) in Tab::ALL.into_iter().enumerate() {
        let r = Rect::new(list_box.x + i as f32 * cell, list_box.y - head * 1.45, cell, head * 1.4);
        let over = mouse_in(r.x, r.y, r.w, r.h);
        let ink = if tab == view.tab { ACCENT } else if over { INK } else { DIM };
        // In the diary's bold face (the original's bitmap fonts come in one size each, and the
        // four titles do not fit in the title face).
        let label = tr(diary_tab_label(tab));
        let size = sheet_size(Face::Bold, s);
        with_original_face(Face::Bold, || chrome::shadow_centered(label, r.x + r.w / 2.0, list_box.y - 7.0 * s, size, ink));
        if tab == view.tab {
            let w = with_original_face(Face::Bold, || measure(label, size).width);
            draw_line(r.x + (r.w - w) / 2.0, list_box.y - 4.0 * s, r.x + (r.w + w) / 2.0, list_box.y - 4.0 * s, 1.0 * s, ACCENT);
        }
        if over && clicked() {
            switch = Some(tab);
        }
    }
    if key(KeyCode::Right) {
        switch = Some(Tab::ALL[(at + 1) % Tab::ALL.len()]);
    }
    if key(KeyCode::Left) {
        switch = Some(Tab::ALL[(at + Tab::ALL.len() - 1) % Tab::ALL.len()]);
    }
    if let Some(tab) = switch.filter(|&t| t != view.tab) {
        cue(Cue::Button);
        *view = HeroView { scroll: view.scroll, tab, ..HeroView::default() };
    }
    let rows = diary_rows(game, view.tab);
    if view.pick >= rows.len() {
        view.pick = 0;
    }
    if key(KeyCode::Down) && view.pick + 1 < rows.len() {
        view.pick += 1;
        view.text_scroll = 0;
    }
    if key(KeyCode::Up) && view.pick > 0 {
        view.pick -= 1;
        view.text_scroll = 0;
    }
    // A campaign's history runs over several maps: each map's entries under its title.
    let lines = super::story::journal_lines(&rows);
    chrome::text_box_base(list_box);
    // The quest list's titles take the phone's text scale too (`display::text_for`); the boxes'
    // own headings stay at the window's.
    let list_t = sheet_size(Face::Title, super::display::text_for(s, crate::ui::dialog::TEXT_REF));
    // On a phone the list is drawn at its own size, as the entry below.
    // A phone's row shrinks to fit the box down to the window's title size before it is cut.
    let row_floor = sheet_size(Face::Title, s).min(list_t);
    let row_h = (list_t * 1.25).round();
    let fits = ((list.h - 8.0 * s) / row_h).floor().max(1.0) as usize;
    swipe_rows(list, row_h);
    if mouse_in(list.x, list.y, list.w, list.h) {
        let w = wheel();
        if w < 0.0 && view.list_scroll + fits < lines.len() {
            view.list_scroll += 1;
        } else if w > 0.0 {
            view.list_scroll = view.list_scroll.saturating_sub(1);
        }
    }
    // The picked entry kept in sight when the keys move it.
    let picked_line = lines.iter().position(|l| *l == super::story::Line::Row(view.pick)).unwrap_or(0);
    if key(KeyCode::Down) || key(KeyCode::Up) {
        if picked_line < view.list_scroll {
            view.list_scroll = picked_line;
        } else if picked_line >= view.list_scroll + fits {
            view.list_scroll = picked_line + 1 - fits;
        }
    }
    view.list_scroll = view.list_scroll.min(lines.len().saturating_sub(fits));
    scrollbar_always(list_bar, &mut view.list_scroll, fits, lines.len());
    if let Some(n) = lines.iter().enumerate().skip(view.list_scroll).take(fits).find_map(|(n, line)| (*line == super::story::Line::Row(view.pick)).then_some(n)) {
        let ry = list.y + 4.0 * s + (n - view.list_scroll) as f32 * row_h;
        draw_rectangle(list.x + 2.0 * s, ry, list.w - 4.0 * s, row_h, Color::new(0.15, 0.15, 0.15, 0.6));
    }
    chrome::text_box_sides(list_box);
    chrome::silver_frame(list_box, chrome::k().max(1.0));
    if rows.is_empty() {
        with_original_face(Face::Body, || chrome::shadow_centered(super::story::empty_note(view.tab), list.x + list.w / 2.0, list.y + list.h / 2.0, head * 0.9, DIM));
    }
    for (n, line) in lines.iter().enumerate().skip(view.list_scroll).take(fits) {
        let ry = list.y + 4.0 * s + (n - view.list_scroll) as f32 * row_h;
        let k = match *line {
            super::story::Line::Heading(chapter) => {
                let heading = game.journal.chapter_heading(chapter);
                let room = list.w - 12.0 * s;
                let size = if phone { super::dt_font::legacy_metrics_if(true, || fit_row_size(list_t, row_floor, |z| with_original_face(Face::Bold, || measure(&heading, z).width), room)) } else { list_t };
                let head_text = super::dt_font::legacy_metrics_if(phone, || ellipsize(&heading, room, size));
                super::dt_font::legacy_metrics_if(phone, || with_original_face(Face::Bold, || chrome::shadow_centered(&head_text, list.x + list.w / 2.0, ry + row_h * 0.78, size, Color::from_rgba(235, 165, 75, 255))));
                continue;
            }
            super::story::Line::Row(k) => k,
        };
        let row = &rows[k];
        let mut title = row.title.clone();
        // Measured in the face it is drawn in (Benguiat's title): on a phone the row first shrinks
        // to fit, and only then is a title that still does not fit cut.
        let room = list.w - 12.0 * s;
        let size = if phone { super::dt_font::legacy_metrics_if(true, || fit_row_size(list_t, row_floor, |z| with_original_face(Face::Title, || measure(&title, z).width), room)) } else { list_t };
        while super::dt_font::legacy_metrics_if(phone, || with_original_face(Face::Title, || measure(&title, size).width)) > room && title.chars().count() > 3 {
            title.pop();
        }
        super::dt_font::legacy_metrics_if(phone, || with_original_face(Face::Title, || chrome::shadow_centered(&title, list.x + list.w / 2.0, ry + row_h * 0.78, size, ink_color(Ink::Plain))));
        if mouse_in(list.x, ry, list.w, row_h) && clicked() && view.pick != k {
            view.pick = k;
            view.text_scroll = 0;
        }
    }
    let entry = column.entry;
    let entry_bar = Rect::new(entry_bar0.x, entry.y, entry_bar0.w, entry.h);
    let entry_box = Rect::new(entry.x, entry.y, entry_bar.x + entry_bar.w - entry.x, entry.h);
    heading("QuestDescript", n_("Diary entry"), entry_box);
    chrome::text_box_base(entry_box);
    // Razdor's text size (the owner's request: phones' text too small): the entry's text
    // larger, its titles with it; its box scrolls, so it takes any size.
    // On a phone the text base is one px per dp (`display::text_for`, the 1.0.2 large-window
    // scale), so the diary reads as large as the messages do; the desktop is unchanged.
    // Under native metrics (the whole frame) the sheet fonts ignore the size passed, so the
    // phone's diary (its wrap and its drawing) runs with the legacy metrics, which take it.
    let ts = super::display::text_for(s, crate::ui::dialog::TEXT_REF);
    let body = diary_body_size(phone, ts);
    let room = entry.w - 16.0 * s;
    // On a phone the entry's title is as large as the quest list's titles (`list_t`).
    let entry_title = diary_title_size(phone, list_t, head, ts, s);
    let lines = super::dt_font::legacy_metrics_if(phone, || {
        rows.get(view.pick).map_or_else(Vec::new, |row| diary_lines(row, view.tab, phone, room, entry_title, body))
    });
    let line_h = (body * 1.15).round();
    let shown = ((entry.h - 12.0 * s) / line_h).floor().max(1.0) as usize;
    swipe_rows(entry, line_h);
    if mouse_in(entry.x, entry.y, entry.w, entry.h) {
        let w = wheel();
        if w < 0.0 && view.text_scroll + shown < lines.len() {
            view.text_scroll += 1;
        } else if w > 0.0 {
            view.text_scroll = view.text_scroll.saturating_sub(1);
        }
    }
    scrollbar_always(entry_bar, &mut view.text_scroll, shown, lines.len());
    chrome::text_box_sides(entry_box);
    chrome::silver_frame(entry_box, chrome::k().max(1.0));
    super::dt_font::legacy_metrics_if(phone, || {
        for (n, line) in lines.iter().skip(view.text_scroll).take(shown).enumerate() {
            let y = entry.y + 6.0 * s + n as f32 * line_h + body * 0.85;
            draw_markup_row_face(&line.row, entry.x + 8.0 * s, y, room, line.size, line.face, Some(line.color));
        }
    });

    // The phone's bottom buttons: «Весь список» (the whole tab in its reader) left, «Закрыть» between
    // (portrait), «Развернуть» (the picked entry in its reader) right; sideways «Весь список» stands
    // above «Развернуть». Drawn before the readers, which cover them while they are open.
    if phone {
        let row = bottom_row_rects(win);
        let (left, right) = (row.whole, row.expand);
        if button(left.x, left.y, left.w, left.h, tr("Whole list"), whole_list_enabled(rows.len())) {
            view.list_reader = true;
            view.list_reader_scroll = 0;
        }
        if button(right.x, right.y, right.w, right.h, tr("Expand"), expand_entry_enabled(rows.len(), view.pick)) {
            view.reader = true;
            view.reader_scroll = 0;
        }
    }

    // The readers, when open: they take this frame's input (the window below took none), so the
    // window's own keys and clicks see nothing after them. The entry's reader is over the list's
    // when both are open; closing the entry returns to the list (`list_reader` stays set).
    if reading {
        set_input_blocked(below_blocked);
        if view.reader {
            let closed = match rows.get(view.pick) {
                Some(row) => diary_reader(row, view.tab, entry_title, diary_reader_body_size(ts), &mut view.reader_scroll),
                None => true,
            };
            if closed {
                view.reader = false;
            }
        } else if view.list_reader {
            let all = super::story::journal_lines(&rows);
            let (closed, picked) = diary_list_reader(game, &rows, &all, view.tab, (list_t, row_floor), &mut view.list_reader_scroll, view.pick);
            if let Some(k) = picked {
                view.pick = k;
                view.text_scroll = 0;
                view.reader = true;
                view.reader_scroll = 0;
            }
            if closed {
                view.list_reader = false;
            }
        }
        swallow_input();
    }

    // The item held is the pointer; closing the window puts it back in the pack (the screen
    // change in `App::frame`, 0x4b8d28).
    draw_held(game, assets);
    if close || key(KeyCode::Escape) || key(keymap::key(Action::Journal)) {
        *message = None;
        return Some(Screen::WorldMap);
    }
    if bar.is_some() {
        *message = None;
    }
    bar
}

#[cfg(test)]
mod tests {
    use super::{card_line, diary_body_size, diary_column, diary_reader_body_size, diary_title_size, held_card_lit, held_hint_ink, class_wage, hero_resources, panel_slots, pointed_line, promo_ease, promo_look, promo_ms, reset_pack_view, set_pack_view, ArmySel, Content, Controls, GridPress, ItemGroup, PackSort, GROUP, SORT, UnitId};
    use macroquad::prelude::Rect;

    fn column_boxes() -> (Rect, Rect, Rect) {
        // The hero window's three boxes at the desktop's scale 1 (58, 252, 410 down the column).
        (Rect::new(576.0, 58.0, 310.0, 164.0), Rect::new(576.0, 252.0, 292.0, 128.0), Rect::new(576.0, 410.0, 292.0, 220.0))
    }

    #[test]
    fn the_desktop_keeps_the_three_boxes_whatever_is_shown() {
        let (desc, list, entry) = column_boxes();
        for (described, shown) in [(false, false), (false, true), (true, true)] {
            let c = diary_column(desc, list, entry, false, described, shown);
            assert_eq!(c, super::DiaryColumn { desc: Some(desc), list, entry });
        }
    }

    #[test]
    fn a_phone_with_a_diary_entry_and_no_artifact_drops_the_description_box() {
        let (desc, list, entry) = column_boxes();
        let c = diary_column(desc, list, entry, true, false, true);
        assert_eq!(c.desc, None);
        // The list moves to the description box's top, the entry takes its height and the gap.
        assert_eq!(c.list, Rect::new(576.0, 58.0, 292.0, 128.0));
        assert_eq!(c.entry, Rect::new(576.0, 216.0, 292.0, 414.0));
        assert_eq!(c.entry.y + c.entry.h, entry.y + entry.h);
    }

    #[test]
    fn a_phone_keeps_the_three_boxes_while_an_artifact_is_described_or_no_entry_is_shown() {
        let (desc, list, entry) = column_boxes();
        assert_eq!(diary_column(desc, list, entry, true, true, true), super::DiaryColumn { desc: Some(desc), list, entry });
        assert_eq!(diary_column(desc, list, entry, true, false, false), super::DiaryColumn { desc: Some(desc), list, entry });
    }

    #[test]
    fn a_phone_s_diary_body_is_four_fifths_of_the_message_text() {
        // Razdor's phone choice (the owner's decision, 2026-10-30): the hero window's diary body
        // is 80% of the message windows' Subtitle size on a phone; the reader («Развернуть»)
        // keeps the full Subtitle size; the desktop keeps the original's SanSerif (Face::Body).
        use super::super::dt_font::{sheet_size, Face};
        let t = 3.0;
        let message = sheet_size(Face::Subtitle, t);
        assert!((diary_body_size(true, t) - 0.8 * message).abs() < 1e-3);
        assert_eq!(diary_body_size(false, t), sheet_size(Face::Body, t));
        assert_eq!(diary_reader_body_size(t), message);
    }

    #[test]
    fn a_long_quest_title_shrinks_before_it_is_cut() {
        // Razdor's phone choice: a quest row shrinks to fit its box (0.9 steps) down to the
        // window's title size; only a title that never fits is cut, at that floor.
        use super::fit_row_size;
        let width = |z: f32| z * 10.0;
        // Fits at full: full.
        assert_eq!(fit_row_size(20.0, 10.0, width, 300.0), 20.0);
        // Needs shrinking: a smaller size that fits.
        let shrunk = fit_row_size(20.0, 10.0, width, 150.0);
        assert!(shrunk < 20.0 && width(shrunk) <= 150.0, "{shrunk}");
        // Never below the floor: even the floor is too wide, so the floor.
        assert_eq!(fit_row_size(20.0, 15.0, width, 100.0), 15.0);
        // A full size already below the floor is not grown.
        assert_eq!(fit_row_size(12.0, 15.0, width, 100.0), 12.0);
    }

    #[test]
    fn a_phone_s_diary_title_is_as_large_as_the_list_s_titles() {
        // Razdor's phone choice: the entry's title takes the quest list's title size on a phone;
        // the desktop keeps the entry's own heading size (head * ts / s).
        assert_eq!(diary_title_size(true, 20.0, 30.0, 3.0, 2.0), 20.0);
        assert_eq!(diary_title_size(false, 20.0, 30.0, 3.0, 2.0), 30.0 * 3.0 / 2.0);
    }

    #[test]
    fn the_desktop_diary_entry_keeps_the_original_s_lines() {
        // 0x49c388: the title, a blank, the description heading, the text, a blank, the time
        // heading and the time. A completed quest: the title, a blank, the date, a blank, the text.
        use super::{entry_parts, EntryPart::*, Tab};
        assert_eq!(entry_parts(Tab::Active, false), vec![Title, Blank, Description, Text, Blank, TimeHeading, Time]);
        assert_eq!(entry_parts(Tab::Completed, false), vec![Title, Blank, Date, Blank, Text]);
        assert_eq!(entry_parts(Tab::Rumours, false), vec![Title, Blank, Date, Blank, Text]);
        assert_eq!(entry_parts(Tab::Messages, false), vec![Title, Blank, Date, Blank, Text]);
    }

    #[test]
    fn a_phone_s_diary_entry_drops_the_repeated_title_and_the_description_heading() {
        // The player's suggestion: the quest list names the quest and the box is small, so the
        // entry is the text, a blank and the time (its heading and value); the time stays last.
        use super::{entry_parts, EntryPart::*, Tab};
        assert_eq!(entry_parts(Tab::Active, true), vec![Text, Blank, TimeHeading, Time]);
        // A completed quest keeps its completion date and its text; the title and its blank go.
        assert_eq!(entry_parts(Tab::Completed, true), vec![Date, Blank, Text]);
        // Rumours and messages are not the quest list's: they keep their title on a phone.
        assert_eq!(entry_parts(Tab::Rumours, true), vec![Title, Blank, Date, Blank, Text]);
        assert_eq!(entry_parts(Tab::Messages, true), vec![Title, Blank, Date, Blank, Text]);
    }

    /// The phone's reader shows the desktop's full entry for every tab: the title and the
    /// «Описание:» heading come back (a phone's small box drops them), the time line stays.
    #[test]
    fn the_phone_reader_shows_the_desktop_s_full_entry() {
        use super::EntryPart::*;
        for tab in razdor::rules::journal::Tab::ALL {
            assert_eq!(super::reader_parts(tab), super::entry_parts(tab, false), "{tab:?}");
        }
        assert_eq!(super::reader_parts(razdor::rules::journal::Tab::Active), vec![Title, Blank, Description, Text, Blank, TimeHeading, Time]);
        assert_ne!(super::reader_parts(razdor::rules::journal::Tab::Active), super::entry_parts(razdor::rules::journal::Tab::Active, true));
    }

    /// The reader covers the screen less the safe area and a small margin on each side, so the
    /// phone's cutouts and rounded corners stay clear of it; with no insets it is the screen less
    /// the margin.
    #[test]
    fn the_reader_is_the_screen_less_the_safe_area_and_a_margin() {
        use super::super::display::SafeArea;
        let none = SafeArea::default();
        let r = super::diary_reader_rect((1080.0, 2400.0), none);
        assert_eq!((r.x, r.y, r.w, r.h), (8.0, 8.0, 1064.0, 2384.0));
        // A landscape phone with a cutout on each side, its bottom corners and its bottom inset.
        let safe = SafeArea { left: 110.0, right: 110.0, bottom: 40.0, corner_bl: 40.0, corner_br: 40.0, ..SafeArea::default() };
        let r = super::diary_reader_rect((2400.0, 1080.0), safe);
        assert_eq!((r.x, r.y), (118.0, 8.0));
        assert_eq!((r.right(), r.bottom()), (2400.0 - 118.0, 1080.0 - 48.0));
        // A portrait phone with a top inset: the reader starts under it.
        let top = SafeArea { top: 80.0, ..SafeArea::default() };
        let r = super::diary_reader_rect((1080.0, 2400.0), top);
        assert_eq!(r.y, 88.0);
    }

    /// The phone's bottom row: «Весь список», «Закрыть» and «Развернуть» side by side inside the
    /// window, not overlapping, each a finger-sized 44 dp tall at 2.75 px per dp.
    #[test]
    fn the_phone_s_bottom_row_fits_the_window_with_three_finger_sized_buttons() {
        let dp = 2.75;
        let h = 44.0 * dp;
        let win = Rect::new(4.0, 130.0, 1080.0, 1900.0);
        let (left, close, right) = super::bottom_row(win, 1700.0, h, 12.0, 8.0);
        for r in [left, close, right] {
            assert!(r.x >= win.x && r.right() <= win.right(), "{r:?} inside the window");
            assert!(r.h >= 44.0 * dp, "{r:?} at least 44 dp tall");
        }
        assert!(left.right() < close.x && close.right() < right.x, "no overlap, gaps between");
        assert!(close.w >= 200.0, "«Закрыть» stays a wide button");
    }

    /// Sideways, the phone's bottom buttons start right of the hero's left panel (design x 246) and
    /// lie under the diary's column, inside the window.
    #[test]
    fn a_landscape_phone_s_bottom_row_starts_off_the_hero_panel() {
        let k = 1.0;
        let win = Rect::new(0.0, 0.0, 836.0, 600.0);
        let panel_right = win.x + 246.0 * k;
        let h = 44.0 * 2.75;
        let (whole, expand) = super::sideways_row(win, k, 540.0, h);
        assert!(whole.x >= panel_right, "«Весь список» at {whole:?} covers the hero's panel (right edge {panel_right})");
        assert!(whole.x > win.x + win.w / 2.0, "the diary's column is the right half: {whole:?}");
        for r in [whole, expand] {
            assert!(r.x >= win.x && r.right() <= win.right(), "{r:?} inside the window");
        }
    }

    /// Sideways, «Весь список» and «Развернуть» stack under the diary's column, each as wide as the
    /// entry box, not overlapping, «Весь список» above, and the entry box ends above both.
    #[test]
    fn a_landscape_phone_s_diary_buttons_stack_under_the_entry_box() {
        let k = 1.0;
        let win = Rect::new(0.0, 0.0, 836.0, 600.0);
        let h = 44.0 * 2.75;
        let (whole, expand) = super::sideways_row(win, k, 540.0, h);
        let entry_x = win.x + (248.0 + (576.0 - 264.0) * super::HERO_S) * k;
        let entry_right = win.x + (248.0 + (868.0 - 264.0) * super::HERO_S) * k;
        for r in [whole, expand] {
            assert!((r.x - entry_x).abs() < 0.5 && (r.right() - entry_right).abs() < 0.5, "{r:?} is the entry box's width");
            assert!(r.h >= h - 0.5, "{r:?} keeps the finger-sized height");
        }
        assert!(whole.bottom() <= expand.y, "«Весь список» above «Развернуть» with no overlap: {whole:?} {expand:?}");
        assert!((expand.y - 540.0).abs() < 0.5, "«Развернуть» stays on the row's line");
        assert!(expand.y - whole.bottom() >= 8.0 * k - 0.5, "a small gap between them");
        let row = super::BottomRow { whole, close: None, expand };
        let entry = Rect::new(entry_x, win.y + 300.0, entry_right - entry_x, 400.0);
        let cut = super::entry_above_row(entry, &row, k);
        assert!(cut.bottom() <= whole.y - 4.0 * k + 0.5, "the entry box ends above «Весь список»: {cut:?} vs {whole:?}");
        assert_eq!(cut.y, entry.y, "its top stays");
        assert_eq!((cut.x, cut.w), (entry.x, entry.w), "its width stays");
    }

    /// «Развернуть» needs a picked entry on the tab; «Весь список» needs any entry on it.
    #[test]
    fn the_expand_buttons_are_enabled_only_with_an_entry_to_show() {
        assert!(!super::expand_entry_enabled(0, 0), "no entries: grey");
        assert!(super::expand_entry_enabled(3, 2), "a picked entry: enabled");
        assert!(!super::expand_entry_enabled(3, 3), "a pick past the list: grey");
        assert!(!super::whole_list_enabled(0));
        assert!(super::whole_list_enabled(1));
    }

    /// The entry opened from the whole-list reader closes back to the list, not to the window.
    #[test]
    fn closing_an_entry_opened_from_the_list_reader_returns_to_the_list_reader() {
        let mut v = super::HeroView { list_reader: true, ..super::HeroView::default() };
        v.reader = true;
        assert!(v.reading());
        v.reader = false;
        assert!(v.list_reader && v.reading(), "back in the list reader");
        v.list_reader = false;
        assert!(!v.reading(), "closing the list returns to the window");
    }

    #[test]
    fn an_item_held_over_a_card_is_offered_on_line_3() {
        // 0x4c2f54: with an item held the card's line is line 3 (the offer, naming the unit);
        // with nothing held it is the select / move line.
        assert_eq!(card_line(true, Controls::Both, 4, None), (3, 4));
        assert_eq!(card_line(true, Controls::Both, 3, Some(3)), (3, 3));
        assert_eq!(card_line(true, Controls::Drag, 4, Some(3)), (3, 4));
        assert_eq!(card_line(false, Controls::Both, 4, Some(3)), pointed_line(Controls::Both, 4, Some(3)));
    }

    #[test]
    fn a_held_item_lights_the_card_red_for_a_refusal_and_blue_for_an_acceptance() {
        // 4c2f54 writes state 4 (red, the enemy highlight) for a refusal and state 2 (blue) for
        // an acceptance; 4c57bc draws them with the colours of the table built at 4de82e (the
        // state-2 block at 0xadd084 is the blue formula: 255, 255, 255 − 16 × level). Green is
        // state 5, the current unit, which an acceptance keeps (4c2f54: "if not 5, set 2").
        use super::super::chrome::Lit;
        assert_eq!(held_card_lit(true), Lit::Enemy);
        assert_eq!(held_card_lit(false), Lit::CanAct);
        assert_eq!(Lit::Enemy.ring(15, 0), [240, 0, 0], "red frame");
        assert_eq!(Lit::CanAct.ring(15, 0), [0, 0, 240], "blue frame");
    }

    #[test]
    fn a_refusal_line_is_in_the_warning_ink_and_an_offer_in_the_normal_one() {
        // 4c280c / 4c2f54 set the warning font ae24f8 (4dc051: white becomes 255, 103, 96);
        // an acceptance's line 3 keeps the normal line colour.
        assert_eq!(held_hint_ink(true), super::super::chrome::WARNING_INK);
        let red = super::super::chrome::WARNING_INK;
        assert_eq!((red.r * 255.0).round() as u8, 255);
        assert_eq!((red.g * 255.0).round() as u8, 103);
        assert_eq!((red.b * 255.0).round() as u8, 96);
        assert_eq!(held_hint_ink(false), super::super::chrome::GOLD);
    }

    /// The original shows no line when a promotion succeeds (0x4b1dcb, 0x4b1f42, hint hidden
    /// 0x4b1fff); a refused one keeps «Not possible.».
    #[test]
    fn a_promotion_leaves_no_line_in_the_strip() {
        assert_eq!(super::promotion_message::<()>(Ok(())), None);
        assert_eq!(super::promotion_message(Err::<(), _>("refused")), Some(razdor::i18n::tr("Not possible.").to_string()));
    }

    /// The panel's item slots belong to the unit only: a hovered promotion portrait's type
    /// card has none (492f24 skips them for a template, 0x4c3b5f / 0x4c3bc2 pass it).
    #[test]
    fn a_promotion_preview_has_no_item_slots() {
        assert!(panel_slots(false));
        assert!(!panel_slots(true));
    }

    fn sel(selected: Option<usize>, shown: Option<usize>) -> ArmySel {
        ArmySel { selected, shown }
    }

    /// The hero window's resource values as 4d1814 writes them: gold and mana as they are,
    /// income "+ n", upkeep "- n", a zero "0".
    #[test]
    fn the_hero_window_writes_its_resources_as_the_original() {
        assert_eq!(hero_resources(450, 100, 120, 52), ["450", "100", "+ 120", "- 52"].map(String::from));
        assert_eq!(hero_resources(0, 0, 0, 0), ["0", "0", "0", "0"].map(String::from));
    }

    /// A promotion portrait's class card carries the type's recruit wage, as the original's
    /// fresh unit painted in a game does (0x4c39d8 → 0x492f24 → 0x4924c9 → 0x4a163c); a hero
    /// class and an Undead type have none.
    #[test]
    fn a_tree_class_card_shows_the_type_s_recruit_wage() {
        let c = Content::builtin();
        let paid = c.units.iter().map(|u| UnitId(u.id)).find(|&id| razdor::rules::content::HeroClass::of_unit(id).is_none() && c.unit(id).nature != razdor::dt::data::Nature::Undead && c.unit(id).cost > 0).unwrap();
        assert_eq!(class_wage(&c, paid), c.wage(paid));
        assert!(class_wage(&c, paid) > 0);
        assert_eq!(class_wage(&c, razdor::rules::content::HeroClass::Knight.unit()), 0);
        if let Some(undead) = c.units.iter().map(|u| UnitId(u.id)).find(|&id| c.unit(id).nature == razdor::dt::data::Nature::Undead) {
            assert_eq!(class_wage(&c, undead), 0);
        }
    }

    #[test]
    fn a_press_switches_the_right_side_as_the_original() {
        // A unit with nothing selected: selected, its tree up (0x498d0c(unit)).
        let mut s = ArmySel::default();
        assert_eq!(s.press(Controls::Both, Some(3)), GridPress::Select(3));
        assert_eq!(s, sel(Some(3), Some(3)));
        assert_eq!(s.tree(), Some(3));
        // Pressed again: deselected, but 0x4c346c jumps past 0x498d0c, so its tree stays up.
        assert_eq!(s.press(Controls::Both, Some(3)), GridPress::Deselect);
        assert_eq!(s, sel(None, Some(3)));
        assert_eq!(s.tree(), Some(3));
        // An empty cell with nothing selected: 0x498d0c(0), the pack.
        assert_eq!(s.press(Controls::Both, None), GridPress::Nothing);
        assert_eq!(s, sel(None, None));
    }

    #[test]
    fn the_hero_selected_is_not_nothing_selected() {
        // Selecting the hero shows the pack, as nothing selected (unit < 2)...
        let mut s = ArmySel::default();
        assert_eq!(s.press(Controls::Both, Some(0)), GridPress::Select(0));
        assert_eq!(s.tree(), None);
        // ...but a press on another unit swaps it with the hero; nothing selected, the pack up.
        assert_eq!(s.press(Controls::Both, Some(4)), GridPress::Swap { selected: 0, pressed: 4 });
        assert_eq!(s, sel(None, None));
    }

    #[test]
    fn a_slide_keeps_the_unit_selected_until_it_ends() {
        let mut s = ArmySel::default();
        s.press(Controls::Both, Some(2));
        assert_eq!(s.press(Controls::Both, None), GridPress::Slide(2));
        assert_eq!(s, sel(Some(2), Some(2)));
        // 0x4b0c04 clears the selection at its end and does not switch the right side.
        s.slide_done();
        assert_eq!(s, sel(None, Some(2)));
    }

    /// «Расстановка армии: Перетаскивание» (Razdor's default): presses only select; the
    /// right side follows the pressed unit, an empty cell brings the pack back.
    #[test]
    fn with_dragging_only_presses_only_select() {
        let mut s = ArmySel::default();
        assert_eq!(s.press(Controls::Drag, Some(3)), GridPress::Select(3));
        assert_eq!(s, sel(Some(3), Some(3)));
        // Another unit: selected in its place, no swap.
        assert_eq!(s.press(Controls::Drag, Some(5)), GridPress::Select(5));
        assert_eq!(s, sel(Some(5), Some(5)));
        // The same unit again deselects it, as the original's press on the selected unit
        // (4c346c, §9.6; a player's report).
        assert_eq!(s.press(Controls::Drag, Some(5)), GridPress::Deselect);
        assert_eq!(s.press(Controls::Drag, Some(5)), GridPress::Select(5));
        // An empty cell: no slide; nothing selected, the pack up.
        assert_eq!(s.press(Controls::Drag, None), GridPress::Nothing);
        assert_eq!(s, ArmySel::default());
    }

    #[test]
    fn the_card_hint_follows_the_arrangement_setting() {
        // The original's lines (0x4c2f54): 0 over the selected, 2 over another naming the
        // selected, 1 with none selected.
        assert_eq!(pointed_line(Controls::Both, 3, Some(3)), (0, 3));
        assert_eq!(pointed_line(Controls::Both, 4, Some(3)), (2, 3));
        assert_eq!(pointed_line(Controls::Both, 4, None), (1, 4));
        // Dragging only: a click only selects, so always "click to select" this one.
        assert_eq!(pointed_line(Controls::Drag, 3, Some(3)), (1, 3));
        assert_eq!(pointed_line(Controls::Drag, 4, Some(3)), (1, 4));
        assert_eq!(pointed_line(Controls::Drag, (true, 1), Some((false, 2))), (1, (true, 1)));
    }

    #[test]
    fn a_dismissal_drops_indices_beyond_the_army() {
        let mut s = sel(Some(5), Some(5));
        s.clamp(5);
        assert_eq!(s, ArmySel::default());
    }

    /// A new game starts the backpack at its default group and order; within one game the choice stays.
    #[test]
    fn a_new_game_starts_the_backpack_at_the_default_group_and_order() {
        set_pack_view(ItemGroup::Potions, PackSort::PriceUp);
        assert_eq!((GROUP.with(|g| g.get()), SORT.with(|s| s.get())), (ItemGroup::Potions, PackSort::PriceUp));
        reset_pack_view();
        assert_eq!(GROUP.with(|g| g.get()), ItemGroup::All);
        assert_eq!(SORT.with(|s| s.get()), PackSort::Shown);
    }

    #[test]
    fn a_promotion_lasts_by_the_portraits_distance() {
        assert_eq!(promo_ms(2), 1501, "143 px straight down");
        assert_eq!(promo_ms(1), 1746, "106 across, 143 down");
        assert_eq!(promo_ms(3), 1746);
    }

    #[test]
    fn the_promotion_ease_is_slow_fast_slow() {
        assert_eq!(promo_ease(0, 2200), 0);
        assert_eq!(promo_ease(220, 2200), 100, "slope 1 to a tenth");
        assert_eq!(promo_ease(660, 2200), 500, "slope 2 to 300");
        assert_eq!(promo_ease(1540, 2200), 1700, "slope 3 to 700");
        assert_eq!(promo_ease(2200, 2200), 2200);
        assert_eq!(promo_ease(5000, 2200), 2200, "held at the end");
    }

    #[test]
    fn a_promotion_glows_travels_and_flashes_as_the_original() {
        let ms = 1501;
        let l = promo_look(0, ms);
        assert_eq!((l.frame, l.along, l.pick_glow, l.all_glow), (0, 0.0, 160, 0));
        let l = promo_look(399, ms);
        assert_eq!((l.frame, l.pick_glow), (11, 0));
        assert_eq!(promo_look(499, ms).frame, 14);
        let l = promo_look(500, ms);
        assert_eq!((l.frame, l.along), (16, 0.0));
        assert_eq!(promo_look(ms - 1, ms).frame, 48);
        // The flash: up over the 100 ms before the class changes, down over the last 100.
        assert_eq!(promo_look(ms - 200, ms).all_glow, 0);
        assert_eq!(promo_look(ms - 150, ms).all_glow, 100);
        assert_eq!(promo_look(ms - 100, ms).all_glow, 200);
        assert_eq!(promo_look(ms - 50, ms).all_glow, 100);
        let l = promo_look(ms, ms);
        assert_eq!((l.frame, l.along, l.all_glow), (49, 1.0, 0));
    }
}

