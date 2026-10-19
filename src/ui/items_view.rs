//! The hero and army screen: gear, backpack, promotion.
use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::rules::content::{Content, ItemId, Stat, UnitId};
use razdor::rules::experience::is_percent_stat;
use razdor::rules::game::{Game, PACK_SIZE};
use razdor::rules::items::{EquipError, Given, Press};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::audio::{cue, Cue};
use super::building_view::{service_error, BuildingView};
use super::screens::attack_line;
use super::unit_drag::{grid_press, GridPress};
use super::chrome;
use super::unit_sheet;
use super::widgets::*;
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

thread_local! {
    /// The unit whose Dismiss (or Bury) was pressed: confirm or cancel (0x4c3744).
    static CONFIRM: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
    /// A battle effect playing over a unit's card (0x4b11cc).
    static CARD_FX: std::cell::Cell<Option<CardFx>> = const { std::cell::Cell::new(None) };
    /// The card sliding after a press on an empty cell.
    static SLIDE: std::cell::Cell<Option<Slide>> = const { std::cell::Cell::new(None) };
    /// The class a promotion portrait under the pointer put on the left panel (0x4c39d8
    /// repaints it as a fresh unit of that type); it stays when the pointer leaves, as the
    /// original's, until a card is hovered or pressed.
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

/// A press on pack cell `i`, as the original's (0x4c24f4): the item there is picked up onto
/// the pointer, or the item held is put there (an empty cell anywhere keeps it) or swapped
/// with it.
fn pack_press(game: &mut Game, i: usize) {
    let press = game.press_pack(i);
    press_sound(game, press);
}

/// A press on worn slot `slot` of squad member `unit`'s panel (0x4c24f4): the item is taken
/// off onto the pointer, or the item held is put on (the wear test's refusal is the message).
fn worn_press(game: &mut Game, unit: usize, slot: usize) -> Option<String> {
    match game.press_worn(unit, slot) {
        Ok(press) => {
            press_sound(game, press);
            None
        }
        Err(e) => Some(equip_error(e)),
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
/// so with 5 rows shown the top row goes down to length div 5 − 4), in `cols` columns.
fn pack_rows(len: usize, cols: usize) -> usize {
    (len / cols.max(1) + 1).min(PACK_SIZE.div_ceil(cols.max(1)))
}

/// The backpack: 5 columns of the original's inventory squares, the pack's cells in order a
/// row at a time, scrolling. Returns the cell pressed (an empty one too: an item put there
/// stays there, 0x4c24f4).
fn pack_view(game: &Game, assets: &Assets, r: Rect, scroll: &mut usize, hover: &mut Option<ItemId>) -> Option<usize> {
    let k = chrome::k();
    let cell = ((r.w - 18.0 * k) / PACK_COLS as f32).floor();
    let rows_shown = ((r.h / cell).floor() as usize).max(1);
    let rows = pack_rows(game.pack.len(), PACK_COLS).max(rows_shown);
    let max_scroll = rows.saturating_sub(rows_shown);
    if mouse_in(r.x, r.y, r.w, r.h) {
        let w = wheel();
        if w < 0.0 {
            *scroll = (*scroll + 1).min(max_scroll);
        } else if w > 0.0 {
            *scroll = scroll.saturating_sub(1);
        }
    }
    *scroll = (*scroll).min(max_scroll);
    let inv = chrome::win("Inventory");
    let mut hit = None;
    for row in 0..rows_shown {
        for col in 0..PACK_COLS {
            let i = (*scroll + row) * PACK_COLS + col;
            let cr = Rect::new(r.x + col as f32 * cell, r.y + row as f32 * cell, cell, cell);
            match &inv {
                Some(t) => {
                    let s = t.width() / 5.0;
                    chrome::tex_src(t, Rect::new(col as f32 * s, ((row + *scroll) % 5) as f32 * s, s, s), cr, WHITE);
                }
                None => {
                    chrome::surface(cr, chrome::Skin::Paper);
                    draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 1.0, Color::new(0.5, 0.35, 0.2, 0.8));
                }
            }
            if i >= PACK_SIZE {
                continue;
            }
            let cell_at = i;
            let item = game.pack.get(i);
            if let Some(item) = item {
                assets.draw_item(item, cr.x + 2.0, cr.y + 2.0, cell - 4.0);
            }
            if mouse_in(cr.x, cr.y, cr.w, cr.h) {
                if item.is_some() || game.held_item.is_some() {
                    draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 2.0, chrome::GOLD);
                }
                if item.is_some() {
                    *hover = item;
                }
                if clicked() {
                    hit = Some(cell_at);
                }
            }
        }
    }
    // The scroll bar.
    let bx = r.x + PACK_COLS as f32 * cell + 4.0 * k;
    let bh = rows_shown as f32 * cell;
    draw_rectangle(bx, r.y, 12.0 * k, bh, Color::new(0.05, 0.05, 0.05, 0.8));
    let th = bh * rows_shown as f32 / rows.max(1) as f32;
    let ty = r.y + (bh - th) * *scroll as f32 / max_scroll.max(1) as f32;
    draw_rectangle(bx + 1.0, ty, 12.0 * k - 2.0, th, chrome::SILVER);
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
    /// slide ends ([`ArmySel::slide_done`]).
    fn press(&mut self, pressed: Option<usize>) -> GridPress {
        let p = grid_press(self.selected, pressed);
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
            GridPress::Nothing => self.shown = None,
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

fn now_ms() -> i64 {
    (get_time() * 1000.0) as i64
}

/// The hero and army screen, as the original's (refs 11 and 12): the selected unit's panel
/// with its four item slots on the left; the backpack (or the upgrade tree) and the item
/// description at the top; the army's 2×6 cards below. Click a card to select it, a pack
/// item to wear or drink it, a worn item to take it off; drag a pack or worn item onto a
/// card to give it to that unit, or a worn one onto the pack to take it off. Cards are
/// pressed as in the original ([`ArmySel::press`]) or dragged (Razdor's). `back` is the
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
            *message = Some(match game.promote(p.unit, p.to) {
                Ok(()) => razdor::trf!("{name} is now a {class} (level 1, XP 0).", name = game.squad_label(p.unit), class = game.content.unit(p.to).name),
                Err(_) => tr("Not possible.").into(),
            });
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
    let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
    let win = Rect::new(((sw - ww) / 2.0).round(), ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round(), ww, wh);
    let title = chrome::ui_text("Army", "Title").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr("The hero's characteristics and army").to_string());
    let (_, close) = chrome::window(win, &title, chrome::Skin::Marble, true);
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::new(win.x + x * k, win.y + y * k, w * k, h * k);
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
    let mut status = Vec::new();
    if !u.potions.is_empty() {
        let names: Vec<&str> = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect();
        status.push((razdor::trf!("Until the next battle: {names}", names = names.join(", ")), chrome::BLUE_TEXT));
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
        power: stats[Stat::MagicPower],
        wage: if showing_type { class_wage(&c, u.def) } else { game.wage(sel) },
        items: u.items,
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        unpaid: !showing_type && u.unpaid,
        building: bd,
        hero,
        status,
        battle: false,
    };
    let sheet_rect = at(2.0, 27.0, 244.0, 570.0);
    // A living unit's first `personal` worn slots are his own: not taken up (0x4c24f4), and
    // hovering one says so (0x4c280c, `[Army] ItemI`).
    let locked = |slot: usize| showing_type || (u.alive() && slot < u.personal as usize);
    let pressed_slot = unit_sheet::draw(assets, &c, sheet_rect, &sheet, true, &mut hover);
    let personal_hover = hover.is_some_and(|h| u.items.iter().enumerate().any(|(s, i)| *i == Some(h) && locked(s)));
    if let Some(slot) = pressed_slot.filter(|_| !showing_type) {
        *message = worn_press(game, sel, slot);
    }
    draw_line(win.x + 247.0 * k, win.y + 27.0 * k, win.x + 247.0 * k, win.y + wh - 2.0, 1.5 * k, chrome::SILVER);

    // Top middle, as the original switches it: the hero's backpack, or the selected unit's
    // upgrade tree under its title.
    let show_tree = tree_of.is_some();
    let head = (13.0 * k).round();
    let own = |key: &str, ours: &'static str| chrome::ui_text("Army", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
    if show_tree {
        let title = own("UpgradeTree", n_("Upgrade tree"));
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            chrome::shadow_centered(&title, win.x + 408.0 * k, win.y + 40.0 * k + head * 0.36, 15.0 * k, chrome::CREAM);
        });
    }
    let content = at(258.0, 54.0, 300.0, 242.0);
    let mut tree_hint = None;
    if let Some(t) = tree_of {
        let tu = game.squad[t].clone();
        if let Some(to) = tree_view(game, assets, t, &tu, content, promo.filter(|p| p.unit == t)) {
            PANEL_TYPE.with(|p| p.set(Some(to)));
            // The hint line (0x4c39d8): `[Army]` line 4 when it lacks the level, 5 to choose,
            // 6 when it has no further class.
            let tree = tu.upgrade_tree(&c);
            let n = if tree.is_empty() { 6 } else if tu.level <= 1 { 4 } else { 5 };
            let who = game.squad_label(t);
            tree_hint = chrome::ui_line("Army", n).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).map(|line| line.replace("#NAME1", &format!("\"{who}\"")));
        }
    } else if let Some(cell) = pack_view(game, assets, content, scroll, &mut hover) {
        *message = None;
        pack_press(game, cell);
    }

    // Top right: the item under the mouse; under it, for a unit other than the hero, its
    // face and the button that sends it away.
    let item_title = own("ItemDescript", n_("Item description"));
    super::dt_font::with_face(super::dt_font::Face::Title, || {
        chrome::shadow_centered(&item_title, win.x + 697.0 * k, win.y + 40.0 * k + head * 0.36, 15.0 * k, chrome::CREAM);
    });
    let desc = if show_tree { at(570.0, 54.0, 256.0, 172.0) } else { at(570.0, 54.0, 256.0, 242.0) };
    match hover {
        Some(item) => super::item_text::draw(game, assets, item, desc, super::item_text::ItemBox::Inventory, k),
        None => chrome::text_box(desc),
    }
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
        let b = Rect::new(face.x + face.w + 12.0 * k, row.y + 14.0 * k, row.w - face.w - 24.0 * k, row.h - 28.0 * k);
        // One confirmation step, as the original's confirm and cancel buttons; all three in its
        // red (0x4dc118 makes them with 0x4dba90: Btn3 for Dismiss, Btn1 for the pair).
        let confirming = CONFIRM.with(|c| c.get()) == Some(sel);
        if !confirming {
            if red_button(b.x, b.y, b.w, b.h, label, true) {
                CONFIRM.with(|c| c.set(Some(sel)));
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

    // The strip under the top half: the last message, or what to do (drawn after the grid,
    // which says what the pointer is over).
    let strip = at(248.0, 302.0, 586.0, 20.0);

    // The army: the cards as in battle, the selected one lit.
    let f = c.formation;
    let lines = f.display_lines() as f32;
    let cs = 1.0f32.min(2.0 / lines).min(6.0 / f.display_cols() as f32);
    let (card, pitch) = (vec2(88.0 * cs * k, 128.0 * cs * k).round(), vec2(96.0 * cs * k, 133.0 * cs * k));
    let grid_w = f.display_cols() as f32 * pitch.x - 8.0 * cs * k;
    let gx = (strip.x + (strip.w - grid_w) / 2.0).round();
    let cell_at = |slot: razdor::rules::formation::Slot| {
        let (line, col) = f.display(slot);
        vec2(gx + col as f32 * pitch.x, strip.y + strip.h + 10.0 * k + line as f32 * pitch.y).round()
    };
    // A press on a card or an empty cell, acted on after the grid is drawn.
    let mut pressed = None;
    for slot in f.slots() {
        if game.squad.iter().any(|u| u.slot == slot) {
            continue;
        }
        let p = cell_at(slot);
        chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(f, slot), true);
        if mouse_in(p.x, p.y, card.x, card.y) && clicked() {
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
        unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[Stat::MagicPower], unit_sheet::caster(&c, v.def), unit_sheet::strip_place(f, v.slot), v.hp, super::building_view::back_row_def(&c, v.slot), game.hero_building_defence(), selected.selected == Some(i), unit_sheet::StripPanel::of_squad(i, v.named));
        if !v.alive() {
            chrome::death_veil(sq);
        } else if v.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        // The original's signs from the top left (493a64): the promotion, then a drunk potion
        // (the hero's helm, Razdor's, in the first place).
        let upgrade = i > 0 && v.upgrade_tree(&c).iter().any(|&(_, _, ok)| ok);
        chrome::card_signs(sq, true, &[(i == 0, "SI_Helm", chrome::GOLD), (upgrade, "Sign-Upgrade", GREEN), (!v.potions.is_empty(), "sign-potion", GREEN)]);
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
        if selected.selected == Some(i) {
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
    let army_line = |n: usize, who: usize| chrome::ui_line("Army", n).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).map(|t| t.replace("#NAME1", &format!("\"{}\"", game.squad_label(who))));
    let row_text = |slot: razdor::rules::formation::Slot| {
        let key = match unit_sheet::strip_place(f, slot) {
            1..=4 => "Line1",
            7..=10 => "Line2",
            _ => "Line3",
        };
        chrome::ui_text("Army", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru)
    };
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
        match (card_under, selected.selected) {
            (Some(i), Some(s)) if i == s => army_line(0, s),
            (Some(_), Some(s)) => army_line(2, s),
            (Some(i), None) => army_line(1, i),
            (None, _) => empty_under.and_then(row_text),
        }
    };
    let (hint, hc) = match (message.as_ref(), pointed) {
        (Some(m), _) => (m.clone(), chrome::GOLD),
        (None, Some(p)) => (p, Color::new(1.0, 0.55, 0.25, 1.0)),
        (None, None) if show_tree => (tr("Click a unit to select it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
        (None, None) => (tr("Click an item to pick it up, then a unit, a slot or a cell to put it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
    };
    chrome::hint_strip(strip, &hint, hc);
    // The press, as the original's (0x4c346c), ignored while a card slides (busy 0x68dc63).
    if let (Some((Some(to), _)), None, true) = (pressed, slide, game.held_item.is_some()) {
        // An item held, pressed on a unit's card: it is given to him (0x4c346c → 0x4979c4);
        // no selection, swap or slide.
        *message = card_press(game, to);
    } else if let (Some((on, slot)), None) = (pressed, slide) {
        *message = None;
        match selected.press(on) {
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
    if let Some((unit, slot)) = super::unit_drag::update(assets, &cells, card) {
        game.move_unit(unit, slot);
        // A drag (Razdor's) ends as the original's swap: nothing selected, the pack up.
        *selected = ArmySel::default();
    }

    // The item held is the pointer; closing the window puts it back in the pack (the
    // screen change in `App::frame`, 0x4b8d28).
    draw_held(game, assets);

    if close || key(KeyCode::Escape) || key(KeyCode::A) {
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
    }
    next
}

/// The wage line of a promotion portrait's class card: the original's fresh unit of that type
/// is painted in a game, so 4924c9 writes its recruit wage (4a163c of the type's Cost), as
/// for a hire offer; none for a hero class or an Undead type.
pub(super) fn class_wage(c: &Content, id: UnitId) -> i32 {
    let undead = c.unit(id).nature == razdor::dt::data::Nature::Undead;
    if razdor::rules::content::HeroClass::of_unit(id).is_some() || undead { 0 } else { c.wage(id) }
}

/// The hero window's state (side window 5, built by 0x4d8638): the pack's scroll, the
/// diary's picked entry (0x6668bf) and how far its list and its entry are scrolled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HeroView {
    pub scroll: usize,
    pub pick: usize,
    pub list_scroll: usize,
    pub text_scroll: usize,
}

/// The hero window's right side is laid out in the original's pixels (window-relative, x from
/// 264, y from 28) scaled by this: the right part of Razdor's window (586 px) over the
/// original's (632 px).
const HERO_S: f32 = 586.0 / 632.0;

/// The rows of the hero window's diary: the quests under way on this map, oldest first, as
/// the original's list (0x68e500, a quest added by 0x49c170 as it is received and taken out
/// by 0x49c2b8 when it is completed).
fn diary_rows(game: &Game) -> Vec<razdor::rules::journal::Row> {
    let mut rows = game.journal_rows(razdor::rules::journal::Tab::Active);
    rows.reverse();
    rows
}

/// The diary entry's lines (0x49c388): the quest's title (white, centred), an empty line,
/// `[Hero] QuestFullDescript` (centred), the description (justified), an empty line,
/// `[Hero] QuestGetTime` (centred) and the time since the quest was received (centred).
/// Each with its font (4dbc26 loads them: the title in white Benguiat 0xae2498, the headings
/// in SanSerif_Bold less 20, 90, 180, an orange, 0xae24f4, the text in SanSerif less 25, 25,
/// 125, a yellow, 0xae24c0), its colour and size.
fn diary_entry(row: &razdor::rules::journal::Row, room: f32, title: f32, body: f32) -> Vec<DiaryLine> {
    use super::dt_font::{with_face, Face};
    use razdor::dt::markup::{Align, Ink};
    let own = |key: &str, ours: &'static str| chrome::ui_text("Hero", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
    let rows = |s: &str, ink: Color, align: Align, face: Face, size: f32| -> Vec<DiaryLine> {
        with_face(face, || {
            let width = |c: char| measure(c.encode_utf8(&mut [0; 4]), size).width;
            razdor::dt::markup::wrap(s, room, width).into_iter().map(|r| DiaryLine { row: MarkupRow { text: r.text, ink: Ink::Plain, align, last: r.last }, face, color: ink, size }).collect()
        })
    };
    let (white, orange, yellow) = (Color::from_rgba(255, 255, 255, 255), Color::from_rgba(235, 165, 75, 255), Color::from_rgba(230, 230, 130, 255));
    let blank = || DiaryLine { row: MarkupRow { text: String::new(), ink: Ink::Plain, align: Align::Centre, last: true }, face: Face::Body, color: yellow, size: body };
    let mut out = rows(&row.title, white, Align::Centre, Face::Title, title);
    out.push(blank());
    out.extend(rows(&own("QuestFullDescript", n_("Description:")), orange, Align::Centre, Face::Bold, body));
    for line in row.text.lines().map(str::trim).filter(|l| !l.is_empty()) {
        out.extend(rows(&format!("{}{line}", razdor::dt::markup::INDENT), yellow, Align::Justify, Face::Body, body));
    }
    out.push(blank());
    out.extend(rows(&own("QuestGetTime", n_("Time since the quest was received:")), orange, Align::Centre, Face::Bold, body));
    let since = row.elapsed.map_or_else(|| tr("unknown").to_string(), |m| razdor::rules::clock::duration_label(m as f64));
    out.extend(rows(&since, yellow, Align::Centre, Face::Body, body));
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
    use super::dt_font::{sheet_size, with_face, Face};
    let bar = super::world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Hero));
    let c = game.content.clone();
    let k = chrome::k();
    let ru = razdor::i18n::lang() == razdor::i18n::Lang::Ru;
    let own = |key: &str, ours: &'static str| chrome::ui_text("Hero", key).filter(|_| ru).unwrap_or_else(|| tr(ours).to_string());
    let (sw, sh) = (screen_width(), screen_height());
    let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
    let win = Rect::new(((sw - ww) / 2.0).round(), ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round(), ww, wh);
    let title = own("Title", n_("The hero's characteristics and inventory"));
    let (_, close) = chrome::window(win, &title, chrome::Skin::Marble, true);
    // The original's right side, window-relative.
    let s = HERO_S * k;
    let o = |x: f32, y: f32, w: f32, h: f32| Rect::new(win.x + 248.0 * k + (x - 264.0) * s, win.y + 28.0 * k + (y - 28.0) * s, w * s, h * s);
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
        power: stats[Stat::MagicPower],
        wage: game.wage(0),
        items: u.items,
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        unpaid: u.unpaid,
        building: 0,
        hero,
        status: if u.potions.is_empty() { Vec::new() } else { vec![(razdor::trf!("Until the next battle: {names}", names = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect::<Vec<_>>().join(", ")), chrome::BLUE_TEXT)] },
        battle: false,
    };
    let sheet_rect = Rect::new(win.x + 2.0 * k, win.y + 27.0 * k, 244.0 * k, 570.0 * k);
    // A press on a worn slot picks its item up or puts the held one on (0x4c24f4; the hero
    // window shows the hero's slots: 0x4b8d28 selects him as it opens).
    if let Some(slot) = unit_sheet::draw(assets, &c, sheet_rect, &sheet, true, &mut hover) {
        *message = worn_press(game, 0, slot);
    }
    draw_line(win.x + 247.0 * k, win.y + 27.0 * k, win.x + 247.0 * k, win.y + win.h - 2.0, 1.5 * k, chrome::SILVER);

    // The pack: 25 squares of 55 px from (274, 37), the scroll bar right of them.
    let pack = o(274.0, 37.0, 275.0 + 18.0 / HERO_S, 275.0);
    if let Some(cell) = pack_view(game, assets, pack, &mut view.scroll, &mut hover) {
        *message = None;
        pack_press(game, cell);
    }

    // A box's title over it, as 0x48e750 draws them: centred, in Benguiat.
    let head = sheet_size(Face::Title, s);
    let heading = |key: &str, ours: &'static str, r: Rect| {
        let t = own(key, ours);
        with_face(Face::Title, || chrome::shadow_centered(&t, r.x + r.w / 2.0, r.y - 7.0 * s, head, ACCENT));
    };

    // The item description box (310 × 164 at 58): the item under the pointer.
    let desc = o(576.0, 58.0, 310.0, 164.0);
    heading("ItemDescript", n_("Item description"), desc);
    match hover.filter(|_| game.held_item.is_none()) {
        Some(item) => super::item_text::draw(game, assets, item, desc, super::item_text::ItemBox::Inventory, s),
        None => chrome::text_box(desc),
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
        with_face(Face::Title, || {
            chrome::shadow_text(&name, at.x, at.y + head * 0.8, head, ink_color(Ink::Plain));
            chrome::shadow_text(value, at.x + 20.0 * s, at.y + 20.0 * s + head * 0.8, head, ink_color(Ink::Star));
        });
    }

    // The diary: the list of the quests under way over the picked one's entry.
    let rows = diary_rows(game);
    if view.pick >= rows.len() {
        view.pick = 0;
    }
    let list = o(576.0, 252.0, 292.0, 128.0);
    let list_bar = o(868.0, 252.0, 18.0, 128.0);
    let list_box = Rect::new(list.x, list.y, list_bar.x + list_bar.w - list.x, list.h);
    heading("QuestList", n_("Diary contents"), list_box);
    chrome::text_box(list_box);
    let row_h = (head * 1.25).round();
    let fits = ((list.h - 8.0 * s) / row_h).floor().max(1.0) as usize;
    if mouse_in(list.x, list.y, list.w, list.h) {
        let w = wheel();
        if w < 0.0 && view.list_scroll + fits < rows.len() {
            view.list_scroll += 1;
        } else if w > 0.0 {
            view.list_scroll = view.list_scroll.saturating_sub(1);
        }
    }
    scrollbar_always(list_bar, &mut view.list_scroll, fits, rows.len());
    for (n, row) in rows.iter().enumerate().skip(view.list_scroll).take(fits) {
        let ry = list.y + 4.0 * s + (n - view.list_scroll) as f32 * row_h;
        if n == view.pick {
            // The picked row lit (the list's highlight, 0x262626 added).
            draw_rectangle(list.x + 2.0 * s, ry, list.w - 4.0 * s, row_h, Color::new(0.15, 0.15, 0.15, 0.6));
        }
        let mut title = row.title.clone();
        while measure(&title, head).width > list.w - 12.0 * s && title.chars().count() > 3 {
            title.pop();
        }
        with_face(Face::Title, || chrome::shadow_centered(&title, list.x + list.w / 2.0, ry + row_h * 0.78, head, ink_color(Ink::Plain)));
        if mouse_in(list.x, ry, list.w, row_h) && clicked() && view.pick != n {
            view.pick = n;
            view.text_scroll = 0;
        }
    }
    let entry = o(576.0, 410.0, 292.0, 220.0);
    let entry_bar = o(868.0, 410.0, 18.0, 220.0);
    let entry_box = Rect::new(entry.x, entry.y, entry_bar.x + entry_bar.w - entry.x, entry.h);
    heading("QuestDescript", n_("Diary entry"), entry_box);
    chrome::text_box(entry_box);
    let body = sheet_size(Face::Body, s);
    let room = entry.w - 16.0 * s;
    let lines = rows.get(view.pick).map_or_else(Vec::new, |row| diary_entry(row, room, head, body));
    let line_h = (body * 1.15).round();
    let shown = ((entry.h - 12.0 * s) / line_h).floor().max(1.0) as usize;
    if mouse_in(entry.x, entry.y, entry.w, entry.h) {
        let w = wheel();
        if w < 0.0 && view.text_scroll + shown < lines.len() {
            view.text_scroll += 1;
        } else if w > 0.0 {
            view.text_scroll = view.text_scroll.saturating_sub(1);
        }
    }
    scrollbar_always(entry_bar, &mut view.text_scroll, shown, lines.len());
    for (n, line) in lines.iter().skip(view.text_scroll).take(shown).enumerate() {
        let y = entry.y + 6.0 * s + n as f32 * line_h + body * 0.85;
        draw_markup_row_face(&line.row, entry.x + 8.0 * s, y, room, line.size, line.face, Some(line.color));
    }

    // The item held is the pointer; closing the window puts it back in the pack (the screen
    // change in `App::frame`, 0x4b8d28).
    draw_held(game, assets);
    if close || key(KeyCode::Escape) {
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
    use super::{class_wage, hero_resources, promo_ease, promo_look, promo_ms, ArmySel, Content, GridPress, UnitId};

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
        assert_eq!(s.press(Some(3)), GridPress::Select(3));
        assert_eq!(s, sel(Some(3), Some(3)));
        assert_eq!(s.tree(), Some(3));
        // Pressed again: deselected, but 0x4c346c jumps past 0x498d0c, so its tree stays up.
        assert_eq!(s.press(Some(3)), GridPress::Deselect);
        assert_eq!(s, sel(None, Some(3)));
        assert_eq!(s.tree(), Some(3));
        // An empty cell with nothing selected: 0x498d0c(0), the pack.
        assert_eq!(s.press(None), GridPress::Nothing);
        assert_eq!(s, sel(None, None));
    }

    #[test]
    fn the_hero_selected_is_not_nothing_selected() {
        // Selecting the hero shows the pack, as nothing selected (unit < 2)...
        let mut s = ArmySel::default();
        assert_eq!(s.press(Some(0)), GridPress::Select(0));
        assert_eq!(s.tree(), None);
        // ...but a press on another unit swaps it with the hero; nothing selected, the pack up.
        assert_eq!(s.press(Some(4)), GridPress::Swap { selected: 0, pressed: 4 });
        assert_eq!(s, sel(None, None));
    }

    #[test]
    fn a_slide_keeps_the_unit_selected_until_it_ends() {
        let mut s = ArmySel::default();
        s.press(Some(2));
        assert_eq!(s.press(None), GridPress::Slide(2));
        assert_eq!(s, sel(Some(2), Some(2)));
        // 0x4b0c04 clears the selection at its end and does not switch the right side.
        s.slide_done();
        assert_eq!(s, sel(None, Some(2)));
    }

    #[test]
    fn a_dismissal_drops_indices_beyond_the_army() {
        let mut s = sel(Some(5), Some(5));
        s.clamp(5);
        assert_eq!(s, ArmySel::default());
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
