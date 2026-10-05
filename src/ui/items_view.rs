//! The hero and army screen: gear, backpack, promotion.
use macroquad::prelude::*;

use razdor::i18n::{n_, tr};
use razdor::rules::battle::Team;
use razdor::rules::content::{ArtefactType, Content, ItemId, Stat, UnitId};
use razdor::rules::experience::is_percent_stat;
use razdor::rules::game::{Game, PACK_SIZE};
use razdor::rules::items::{bonus_name, EquipError, Given, ItemFrom};
use razdor::rules::units::Unit;

use super::assets::Assets;
use super::audio::{cue, cued, Cue};
use super::building_view::{service_error, BuildingView};
use super::screens::attack_line;
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
    extra.extend(s.bonuses.iter().map(bonus_name));
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

/// Where a held item was picked up.
#[derive(Clone, Copy)]
enum From {
    Pack(usize),
    /// Squad member, item slot.
    Worn(usize, usize),
}

/// An item under the pressed mouse button. Let go where it was picked up, it is a click
/// (wear or drink it on the selected unit, or take it off); moved, it is dragged, as in the
/// original, onto a unit's card to hand it over.
#[derive(Clone, Copy)]
struct Held {
    from: From,
    item: ItemId,
    at: Vec2,
    moved: bool,
}

/// Pixels the mouse must move before a press becomes a drag.
const DRAG_START: f32 = 5.0;

thread_local! {
    static HELD: std::cell::Cell<Option<Held>> = const { std::cell::Cell::new(None) };
    /// The unit whose Dismiss (or Bury) was pressed: confirm or cancel (0x4c3744).
    static CONFIRM: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Squad member `unit` wears or drinks the pack item at `i`; the message to show, if any.
fn use_pack_item(game: &mut Game, unit: usize, i: usize) -> Option<String> {
    let c = game.content.clone();
    let item = *game.pack.get(i)?;
    let kind = c.item(item).kind;
    let name = game.squad.get(unit)?.name(&c).to_string();
    if kind == ArtefactType::Potion {
        Some(match game.drink(unit, i) {
            Ok(healed) if healed > 0 => cued(Cue::Item(kind), razdor::trf!("{name} drinks it: +{healed} hits.", name, healed)),
            Ok(_) => cued(Cue::Item(kind), razdor::trf!("{name} drinks it. The effect lasts until the next battle ends.", name)),
            Err(e) => equip_error(e),
        })
    } else {
        let done = game.equip(unit, i);
        if done.is_ok() {
            cue(Cue::Item(kind));
        }
        done.err().map(equip_error)
    }
}

/// An item dropped on squad member `to`'s card ([`Game::give_item`]): a potion is drunk,
/// the hero's card sends anything else to the pack, another unit wears it. The message to
/// show, if any.
fn give_on_card(game: &mut Game, from: From, to: usize) -> Option<String> {
    let c = game.content.clone();
    let source = match from {
        From::Pack(i) => ItemFrom::Pack(i),
        From::Worn(unit, slot) => ItemFrom::Worn { unit, slot },
    };
    let item = match source {
        ItemFrom::Pack(i) => game.pack.get(i).copied(),
        ItemFrom::Worn { unit, slot } => game.squad.get(unit).and_then(|u| u.items[slot]),
    }?;
    let kind = c.item(item).kind;
    let name = game.squad.get(to)?.name(&c).to_string();
    match game.give_item(source, to) {
        Ok(Given::Drunk(healed)) if healed > 0 => Some(cued(Cue::Item(kind), razdor::trf!("{name} drinks it: +{healed} hits.", name, healed))),
        Ok(Given::Drunk(_)) => Some(cued(Cue::Item(kind), razdor::trf!("{name} drinks it. The effect lasts until the next battle ends.", name))),
        Ok(_) => {
            cue(Cue::Item(kind));
            None
        }
        Err(e) => Some(equip_error(e)),
    }
}

/// The promotion tree of squad member `sel` in `r`, as the original's: the current class at
/// the bottom, arrows up to its options (portraits; the ones open now glow and promote on a
/// click, free of charge).
fn tree_view(game: &mut Game, assets: &Assets, sel: usize, u: &Unit, r: Rect, message: &mut Option<String>) {
    let c = game.content.clone();
    let k = chrome::k();
    let tree = if sel == 0 { Vec::new() } else { u.upgrade_tree(&c) };
    if let Some(t) = chrome::win_fx("UpgradeTree", chrome::Fx::KeyBlack) {
        chrome::tex(&t, r, WHITE);
    }
    let s = (r.w * 0.28).min(r.h * 0.36);
    let cur = Rect::new(r.x + (r.w - s) / 2.0, r.y + r.h * 0.95 - s, s, s);
    let slots = [0.17, 0.5, 0.83];
    for (i, &(to, level, ok)) in tree.iter().take(3).enumerate() {
        let o = Rect::new(r.x + r.w * slots[i] - s / 2.0, r.y + r.h * 0.04, s, s);
        if chrome::win("UpgradeTree").is_none() {
            draw_line(cur.x + cur.w / 2.0, cur.y, o.x + o.w / 2.0, o.y + o.h, 3.0, if ok { chrome::GOLD } else { DIM });
        }
        draw_rectangle(o.x - 2.0, o.y - 2.0, o.w + 4.0, o.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
        assets.draw_portrait(to, Team::Player, o);
        if !ok {
            draw_rectangle(o.x, o.y, o.w, o.h, Color::new(0.0, 0.0, 0.0, 0.45));
        }
        draw_rectangle_lines(o.x, o.y, o.w, o.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let label = razdor::trf!("Lv {level}", level);
        chrome::shadow_centered(&label, o.x + o.w / 2.0, o.y + o.h - 4.0 * k, (12.0 * k).round(), if ok { chrome::GOLD } else { chrome::CREAM });
        let over = mouse_in(o.x, o.y, o.w, o.h);
        if ok {
            chrome::glow_frame(o, Color::new(0.35, 1.0, 0.35, if over { 1.0 } else { 0.6 }), over);
        }
        if over {
            tooltip(&[(c.unit(to).name.clone(), chrome::GOLD), (level_gains(&c, to), chrome::CREAM)]);
            if ok && clicked() {
                *message = Some(match game.promote(sel, to) {
                    Ok(()) => cued(Cue::Upgrade, razdor::trf!("{name} is now a {class} (level 1, XP 0).", name = u.name(&c), class = c.unit(to).name)),
                    Err(_) => tr("Not possible.").into(),
                });
            }
        }
    }
    draw_rectangle(cur.x - 2.0, cur.y - 2.0, cur.w + 4.0, cur.h + 4.0, Color::new(0.0, 0.0, 0.0, 0.6));
    assets.draw_portrait(u.def, Team::Player, cur);
    chrome::wounds(cur, u.hp, u.max_hp(&c));
    draw_rectangle_lines(cur.x, cur.y, cur.w, cur.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
    let note = if sel == 0 {
        tr("The hero rises by levels only.")
    } else if tree.is_empty() {
        tr("The final class: it improves by levels only.")
    } else if tree.iter().any(|&(_, _, ok)| ok) {
        tr("Click a lit class to promote (free; back to level 1).")
    } else {
        tr("Not enough experience to promote yet.")
    };
    for (i, line) in wrap(note, r.w - 12.0, (12.0 * k).round()).iter().enumerate() {
        chrome::shadow_centered(line, r.x + r.w / 2.0, r.y + r.h * 0.5 + i as f32 * 14.0 * k, (12.0 * k).round(), chrome::CREAM);
    }
}

/// The backpack: 5 columns of the original's inventory squares, scrolling. Returns the
/// pack index pressed.
fn pack_view(game: &Game, assets: &Assets, r: Rect, scroll: &mut usize, hover: &mut Option<ItemId>) -> Option<usize> {
    let k = chrome::k();
    let cell = ((r.w - 18.0 * k) / PACK_COLS as f32).floor();
    let rows_shown = ((r.h / cell).floor() as usize).max(1);
    let rows = PACK_SIZE.div_ceil(PACK_COLS);
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
            let Some(&item) = game.pack.get(i) else { continue };
            assets.draw_item(item, cr.x + 2.0, cr.y + 2.0, cell - 4.0);
            if mouse_in(cr.x, cr.y, cr.w, cr.h) {
                *hover = Some(item);
                draw_rectangle_lines(cr.x, cr.y, cr.w, cr.h, 2.0, chrome::GOLD);
                if clicked() {
                    hit = Some(i);
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

/// The selection after a press on card `i` with `sel` selected (0x4c346c): pressing the
/// selected unit deselects it, any other unit is selected. With nothing selected the original
/// shows the pack and the hero's panel, as with the hero selected (0x498d0c: unit < 2), so
/// Razdor's "nothing selected" is the hero, index 0. Without this the pack was out of reach
/// behind a selected unit's promotion tree until the hero's card was pressed.
fn pressed_selection(sel: usize, i: usize) -> usize {
    if i == sel { 0 } else { i }
}

/// The hero and army screen, as the original's (refs 11 and 12): the selected unit's panel
/// with its four item slots on the left; the backpack (or the upgrade tree) and the item
/// description at the top; the army's 2×6 cards below. Click a card to select it, a pack
/// item to wear or drink it, a worn item to take it off; drag a pack or worn item onto a
/// card to give it to that unit, or a worn one onto the pack to take it off. `back` is the
/// building window to return to, if it was opened from one.
pub fn squad(
    game: &mut Game,
    assets: &Assets,
    selected: &mut usize,
    scroll: &mut usize,
    back: &Option<BuildingView>,
    message: &mut Option<String>,
) -> Option<Screen> {
    let bar = super::world_view::window_backdrop(game, assets, Some(super::game_bar::BarButton::Squad));
    *selected = (*selected).min(game.squad.len() - 1);
    let c = game.content.clone();
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
    let win = Rect::new(((sw - ww) / 2.0).round(), ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round(), ww, wh);
    let title = chrome::ui_text("Army", "Title").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr("The hero's characteristics and army").to_string());
    let (_, close) = chrome::window(win, &title, chrome::Skin::Marble, true);
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::new(win.x + x * k, win.y + y * k, w * k, h * k);
    let mut hover = None;
    let sel = *selected;
    let u = game.squad[sel].clone();
    let mut held = HELD.with(|h| h.get());

    // The unit's panel; a click on a worn item takes it off.
    let stats = u.stats(&c);
    let hero = (sel == 0).then(|| razdor::rules::content::HeroClass::ALL.into_iter().find(|h| h.unit() == u.def)).flatten();
    let mut status = Vec::new();
    if !u.potions.is_empty() {
        let names: Vec<&str> = u.potions.iter().map(|&p| c.item(p).name.as_str()).collect();
        status.push((razdor::trf!("Until the next battle: {names}", names = names.join(", ")), chrome::BLUE_TEXT));
    }
    if !u.alive() {
        status.push((tr("Dead").to_string(), chrome::RED_TEXT));
    } else if u.unpaid {
        status.push((tr("Unpaid: refuses to fight").to_string(), chrome::RED_TEXT));
    }
    let sheet = unit_sheet::Sheet {
        kind: u.def,
        name: u.name(&c),
        level: u.level,
        xp: u.xp,
        need: u.xp_to_next(&c),
        hp: u.hp,
        now: &stats,
        start: &stats,
        power: stats[Stat::MagicPower],
        wage: game.wage(sel),
        items: u.items,
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        building: 0,
        hero,
        status,
        battle: false,
    };
    let sheet_rect = at(2.0, 27.0, 244.0, 570.0);
    if let Some(slot) = unit_sheet::draw(assets, &c, sheet_rect, &sheet, true, &mut hover) {
        if let Some(item) = u.items[slot] {
            // An item taken up plays its sound, and again where it goes (interface.md §14).
            cue(Cue::Item(c.item(item).kind));
            held = Some(Held { from: From::Worn(sel, slot), item, at: pointer().into(), moved: false });
        }
    }
    draw_line(win.x + 247.0 * k, win.y + 27.0 * k, win.x + 247.0 * k, win.y + wh - 2.0, 1.5 * k, chrome::SILVER);

    // Top middle, as the original switches it: the hero's backpack, or the selected unit's
    // upgrade tree under its title.
    let show_tree = sel > 0;
    let head = (13.0 * k).round();
    let own = |key: &str, ours: &'static str| chrome::ui_text("Army", key).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr(ours).to_string());
    if show_tree {
        let title = own("UpgradeTree", n_("Upgrade tree"));
        super::dt_font::with_face(super::dt_font::Face::Title, || {
            chrome::shadow_centered(&title, win.x + 408.0 * k, win.y + 40.0 * k + head * 0.36, 15.0 * k, chrome::CREAM);
        });
    }
    let content = at(258.0, 54.0, 300.0, 242.0);
    if show_tree {
        tree_view(game, assets, sel, &u, content, message);
    } else if let Some(i) = pack_view(game, assets, content, scroll, &mut hover) {
        cue(Cue::Item(c.item(game.pack[i]).kind));
        held = Some(Held { from: From::Pack(i), item: game.pack[i], at: pointer().into(), moved: false });
    }
    if let Some(h) = held.as_mut() {
        h.moved |= Vec2::from(pointer()).distance(h.at) > DRAG_START;
        if h.moved {
            hover = Some(h.item);
        }
    }

    // Top right: the item under the mouse; under it, for a unit other than the hero, its
    // face and the button that sends it away.
    let item_title = own("ItemDescript", n_("Item description"));
    super::dt_font::with_face(super::dt_font::Face::Title, || {
        chrome::shadow_centered(&item_title, win.x + 697.0 * k, win.y + 40.0 * k + head * 0.36, 15.0 * k, chrome::CREAM);
    });
    let desc = if sel > 0 { at(570.0, 54.0, 256.0, 172.0) } else { at(570.0, 54.0, 256.0, 242.0) };
    match hover {
        Some(item) => super::building_view::item_description(game, assets, item, desc.x, desc.y, desc.w, desc.h),
        None => chrome::text_box(desc),
    }
    let row = at(570.0, 234.0, 256.0, 62.0);
    if sel > 0 {
        draw_rectangle(row.x, row.y, row.w, row.h, Color::new(0.25, 0.04, 0.02, 0.55));
        chrome::silver_frame(row, 1.0);
        let face = Rect::new(row.x + 4.0 * k, row.y + 4.0 * k, row.h - 8.0 * k, row.h - 8.0 * k);
        assets.draw_portrait(u.def, Team::Player, face);
        chrome::wounds(face, u.hp, u.max_hp(&c));
        let label = if u.alive() { own("Dismiss", n_("Dismiss")) } else { own("Bury", n_("Bury")) };
        let label = label.as_str();
        let b = Rect::new(face.x + face.w + 12.0 * k, row.y + 14.0 * k, row.w - face.w - 24.0 * k, row.h - 28.0 * k);
        // One confirmation step, as the original's confirm and cancel buttons.
        let confirming = CONFIRM.with(|c| c.get()) == Some(sel);
        if !confirming {
            if button(b.x, b.y, b.w, b.h, label, true) {
                CONFIRM.with(|c| c.set(Some(sel)));
            }
        } else {
            let half = (b.w - 6.0 * k) / 2.0;
            if button(b.x, b.y, half, b.h, tr("Confirm"), true) {
                CONFIRM.with(|c| c.set(None));
                let name = u.name(&c).to_string();
                *message = Some(match game.dismiss(sel) {
                    Ok(()) if u.alive() => razdor::trf!("{name} leaves your army.", name),
                    Ok(()) => razdor::trf!("{name} is laid to rest.", name),
                    Err(e) => service_error(e),
                });
                // The hero is selected after it.
                *selected = 0;
            } else if button(b.x + half + 6.0 * k, b.y, half, b.h, tr("Cancel"), true) {
                CONFIRM.with(|c| c.set(None));
            }
        }
    }

    // The strip: the last message, or what to do.
    let strip = at(248.0, 302.0, 586.0, 20.0);
    let (hint, hc) = match message {
        Some(m) => (m.clone(), chrome::GOLD),
        None if show_tree => (tr("Click a unit to select it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
        None => (tr("Drag an item onto a unit to give it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
    };
    chrome::hint_strip(strip, &hint, hc);

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
    for slot in f.slots() {
        if game.squad.iter().any(|u| u.slot == slot) {
            continue;
        }
        let p = cell_at(slot);
        chrome::empty_cell(Rect::new(p.x, p.y, card.x, card.y), chrome::CellIcon::of(f, slot), true);
    }
    let mut card_under = None;
    for (i, v) in game.squad.iter().enumerate() {
        let p = cell_at(v.slot);
        let sq = Rect::new(p.x, p.y, card.x, card.x);
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.45));
        assets.draw_portrait(v.def, Team::Player, sq);
        chrome::wounds(sq, v.hp, v.max_hp(&c));
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let vs = v.stats(&c);
        unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[Stat::MagicPower], unit_sheet::caster(&c, v.def), unit_sheet::strip_place(f, v.slot), v.hp, super::building_view::back_row_def(&c, v.slot), i == sel);
        if !v.alive() {
            draw_rectangle(sq.x, sq.y, sq.w, sq.h, Color::new(0.0, 0.0, 0.0, 0.55));
            draw_line(sq.x + 10.0, sq.y + 10.0, sq.x + sq.w - 10.0, sq.y + sq.h - 10.0, 3.0, RED);
            draw_line(sq.x + sq.w - 10.0, sq.y + 10.0, sq.x + 10.0, sq.y + sq.h - 10.0, 3.0, RED);
        } else if v.unpaid {
            chrome::badge("sign-payment", sq.x + sq.w - 12.0 * k, sq.y + 12.0 * k, 20.0 * k, RED);
        }
        // The original's signs from the top left (493a64): the promotion, then a drunk potion
        // (the hero's helm, Razdor's, in the first place).
        let upgrade = i > 0 && v.upgrade_tree(&c).iter().any(|&(_, _, ok)| ok);
        chrome::card_signs(sq, true, &[(i == 0, "SI_Helm", chrome::GOLD), (upgrade, "Sign-Upgrade", GREEN), (!v.potions.is_empty(), "sign-potion", GREEN)]);
        super::spell_badges::draw(sq, &v.spells, v.drain, game.clock.total_minutes() as u64, &c);
        if super::unit_drag::dragged() == Some(i) {
            draw_rectangle(p.x, p.y, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.55));
        }
        let over = mouse_in(p.x, p.y, card.x, card.y);
        if over {
            card_under = Some(i);
        }
        if i == sel {
            chrome::glow_frame(sq, Color::new(0.35, 1.0, 0.35, 1.0), true);
        } else if over {
            chrome::glow_frame(sq, Color::new(0.35, 0.55, 1.0, 0.9), false);
        }
        if over && clicked() {
            // A press: select the unit, or deselect it when it is the selected one; moved
            // while held, it is dragged to another cell.
            super::unit_drag::press(i, v.def);
            *selected = pressed_selection(sel, i);
            *message = None;
        }
    }
    let cells: Vec<(razdor::rules::formation::Slot, Rect)> = f.slots().map(|s| (s, Rect::new(cell_at(s).x, cell_at(s).y, card.x, card.y))).collect();
    if let Some((unit, slot)) = super::unit_drag::update(assets, &cells, card) {
        game.move_unit(unit, slot);
        // The original's swap (0x4c346c) and slide (0x4b0c04) both end with nothing
        // selected, so the right side goes back to the pack (0x498d0c).
        *selected = 0;
    }

    // The held item follows the mouse; let go, it goes to the card or the pack under it.
    if let Some(h) = held {
        let (mx, my) = pointer();
        if h.moved {
            let s = 48.0 * k;
            assets.draw_item(h.item, mx - s / 2.0, my - s / 2.0, s);
        }
        if is_mouse_button_down(MouseButton::Left) {
            HELD.with(|c| c.set(Some(h)));
        } else {
            HELD.with(|c| c.set(None));
            let over_pack = !show_tree && content.contains(vec2(mx, my));
            // A drop on a card is the army window's (0x4979c4); on the unit panel, the hero
            // window's: it wears the item.
            let card = if h.moved { card_under } else { None };
            let on_sheet = h.moved && card.is_none() && sheet_rect.contains(vec2(mx, my));
            match (h.from, h.moved, card) {
                (From::Pack(i), false, _) => *message = use_pack_item(game, sel, i),
                (From::Worn(unit, slot), false, _) => *message = game.unequip(unit, slot).err().map(equip_error),
                (from, true, Some(t)) => *message = give_on_card(game, from, t),
                (From::Pack(i), true, None) if on_sheet => *message = use_pack_item(game, sel, i),
                (From::Worn(unit, slot), true, None) if on_sheet && sel != unit => {
                    let done = game.give(unit, slot, sel);
                    if done.is_ok() {
                        cue(Cue::Item(c.item(h.item).kind));
                    }
                    *message = done.err().map(equip_error);
                }
                (From::Worn(unit, slot), true, None) if over_pack => *message = game.unequip(unit, slot).err().map(equip_error),
                _ => {}
            }
        }
    }

    if close || key(KeyCode::Escape) || key(KeyCode::A) {
        HELD.with(|c| c.set(None));
        super::unit_drag::cancel();
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
        HELD.with(|c| c.set(None));
    }
    next
}

#[cfg(test)]
mod tests {
    use super::pressed_selection;

    #[test]
    fn pressing_the_selected_unit_gives_the_pack_back() {
        // 0x4c346c: pressed = selected → deselect; 0x498d0c(0) shows the pack.
        assert_eq!(pressed_selection(3, 3), 0);
        assert_eq!(pressed_selection(0, 0), 0);
        // Another unit, or one with nothing (the hero) selected, is selected.
        assert_eq!(pressed_selection(0, 3), 3);
        assert_eq!(pressed_selection(3, 5), 5);
        assert_eq!(pressed_selection(3, 0), 0);
    }
}
