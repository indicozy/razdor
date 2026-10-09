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
use super::unit_drag::{grid_press, GridPress};
use super::chrome;
use super::item_filter;
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
    /// A battle effect playing over a unit's card (0x4b11cc).
    static CARD_FX: std::cell::Cell<Option<CardFx>> = const { std::cell::Cell::new(None) };
    /// The card sliding after a press on an empty cell.
    static SLIDE: std::cell::Cell<Option<Slide>> = const { std::cell::Cell::new(None) };
    /// What the backpack's filter line holds (`ui::item_filter`).
    static FILTER: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    /// The class a promotion portrait under the pointer put on the left panel (0x4c39d8
    /// repaints it as a fresh unit of that type); it stays when the pointer leaves, as the
    /// original's, until a card is hovered or pressed.
    static PANEL_TYPE: std::cell::Cell<Option<razdor::rules::content::UnitId>> = const { std::cell::Cell::new(None) };
}

/// The backpack filter's field.
const FILTER_KEY: &str = "army:pack-filter";

/// Puts `query` into the backpack's filter (debug snapshots).
pub(super) fn set_pack_filter(query: &str) {
    FILTER.with(|f| query.clone_into(&mut f.borrow_mut()));
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

/// Squad member `unit` wears or drinks the pack item at `i`; the message to show, if any.
fn use_pack_item(game: &mut Game, unit: usize, i: usize) -> Option<String> {
    let c = game.content.clone();
    let item = *game.pack.get(i)?;
    let kind = c.item(item).kind;
    let name = game.squad.get(unit)?.name(&c).to_string();
    if kind == ArtefactType::Potion {
        let drunk = game.drink(unit, i);
        if drunk.is_ok() {
            card_fx(5, unit, false);
        }
        Some(match drunk {
            Ok(healed) if healed > 0 => razdor::trf!("{name} drinks it: +{healed} hits.", name, healed),
            Ok(_) => razdor::trf!("{name} drinks it. The effect lasts until the next battle ends.", name),
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
fn tree_view(game: &mut Game, assets: &Assets, sel: usize, u: &Unit, r: Rect, message: &mut Option<String>) -> Option<razdor::rules::content::UnitId> {
    let mut pointed = None;
    let c = game.content.clone();
    let k = chrome::k();
    let tree = u.upgrade_tree(&c);
    let locked = !tree.iter().any(|&(_, _, ok)| ok);
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
        if locked {
            assets.draw_portrait_locked(to, Team::Player, o);
        } else {
            assets.draw_portrait(to, Team::Player, o);
        }
        draw_rectangle_lines(o.x, o.y, o.w, o.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let label = razdor::trf!("Lv {level}", level);
        chrome::shadow_centered(&label, o.x + o.w / 2.0, o.y + o.h - 4.0 * k, (12.0 * k).round(), if ok { chrome::GOLD } else { chrome::CREAM });
        let over = mouse_in(o.x, o.y, o.w, o.h);
        if ok {
            chrome::glow_frame(o, Color::new(0.35, 1.0, 0.35, if over { 1.0 } else { 0.6 }), over);
        }
        if over {
            pointed = Some(to);
            if ok && clicked() {
                *message = Some(match game.promote(sel, to) {
                    Ok(()) => cued(Cue::Upgrade, razdor::trf!("{name} is now a {class}.", name = game.squad_label(sel), class = c.unit(to).name)),
                    Err(_) => tr("Not possible.").into(),
                });
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
    pointed
}

/// The backpack: 5 columns of the original's inventory squares, scrolling; with a filter, only
/// the pack indices `kept`, in their order. Returns the pack index pressed.
fn pack_view(game: &Game, assets: &Assets, r: Rect, scroll: &mut usize, hover: &mut Option<ItemId>, kept: Option<&[usize]>) -> Option<usize> {
    let k = chrome::k();
    // As many of the original's squares as the width takes (5 in its 300 px), then as many
    // rows as fill the height, the squares shrunk to fit them: no empty band under the pack.
    let room = r.w - 18.0 * k;
    let wide = (room / ((room / (56.0 * k)).floor().max(PACK_COLS as f32))).floor();
    let rows_shown = ((r.h / wide).round() as usize).max(1);
    let cell = wide.min(r.h / rows_shown as f32).floor();
    let cols = ((room / cell).floor() as usize).max(PACK_COLS);
    let rows = kept.map_or(PACK_SIZE, |v| v.len().max(1)).div_ceil(cols).max(rows_shown);
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
            let Some(i) = kept.map_or(Some(i), |v| v.get(i).copied()) else { continue };
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
    let bx = r.x + cols as f32 * cell + 4.0 * k;
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
    let c = game.content.clone();
    let k = chrome::k();
    let (sw, sh) = (screen_width(), screen_height());
    let (ww, wh) = ((836.0 * k).round(), (600.0 * k).round());
    let win = Rect::new(((sw - ww) / 2.0).round(), ((sh - chrome::bar_height() - wh) / 2.0).max(2.0).round(), ww, wh);
    let title = chrome::ui_text("Army", "Title").filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).unwrap_or_else(|| tr("The hero's characteristics and army").to_string());
    let (_, close) = chrome::window(win, &title, chrome::Skin::Marble, true);
    let at = |x: f32, y: f32, w: f32, h: f32| Rect::new(win.x + x * k, win.y + y * k, w * k, h * k);
    let mut hover = None;
    // The left panel: the selected unit, else the hero; items go to it (0x4c280c).
    let sel = selected.selected.unwrap_or(0);
    // The right side: the pack, or this unit's promotion tree and Dismiss row (0x498d0c).
    let tree_of = selected.tree();
    let u = game.squad[sel].clone();
    let mut held = HELD.with(|h| h.get());
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
        wage: if showing_type { 0 } else { game.wage(sel) },
        items: u.items,
        back_row: u.slot.row == razdor::rules::formation::Row::Back,
        building: 0,
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
    if let Some(slot) = pressed_slot.filter(|&s| !locked(s)) {
        if let Some(item) = u.items[slot] {
            // An item taken up plays its sound, and again where it goes (interface.md §14).
            cue(Cue::Item(c.item(item).kind));
            held = Some(Held { from: From::Worn(sel, slot), item, at: pointer().into(), moved: false });
        }
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
    // Razdor gives the backpack the whole top right (the user's choice, 2026-10-09: no item
    // description box; an item's description pops up by the pointer, as a spell badge's).
    let content = if show_tree { at(258.0, 54.0, 300.0, 242.0) } else { at(258.0, 54.0, 568.0, 242.0) };
    // The backpack's filter line above it: typing a letter starts it.
    let mut query = FILTER.with(|f| f.borrow().clone());
    let mut filter = item_filter::Reply::default();
    let mut kept = Vec::new();
    if show_tree {
        if has_focus(FILTER_KEY) {
            clear_focus();
        }
    } else {
        let all: Vec<(usize, ItemId)> = game.pack.iter().copied().enumerate().collect();
        let before = item_filter::keep(&c, all.iter().copied(), &query).len();
        filter = item_filter::field(FILTER_KEY, &mut query, at(258.0, 31.0, 300.0, 20.0), &[KeyCode::A, KeyCode::N], before, game.pack.len());
        kept = item_filter::keep(&c, all, &query);
        FILTER.with(|f| query.clone_into(&mut f.borrow_mut()));
    }
    let filtering = !query.trim().is_empty();
    let kept_ids: Vec<usize> = kept.iter().map(|(i, _)| *i).collect();
    let mut tree_hint = None;
    if let Some(t) = tree_of {
        let tu = game.squad[t].clone();
        if let Some(to) = tree_view(game, assets, t, &tu, content, message) {
            PANEL_TYPE.with(|p| p.set(Some(to)));
            // The hint line (0x4c39d8): `[Army]` line 4 when it lacks the level, 5 to choose,
            // 6 when it has no further class.
            let tree = tu.upgrade_tree(&c);
            let n = if tree.is_empty() { 6 } else if tu.level <= 1 { 4 } else { 5 };
            let who = game.squad_label(t);
            tree_hint = chrome::ui_line("Army", n).filter(|_| razdor::i18n::lang() == razdor::i18n::Lang::Ru).map(|line| line.replace("#NAME1", &format!("\"{who}\"")));
        }
    } else if let Some(i) = pack_view(game, assets, content, scroll, &mut hover, filtering.then_some(&kept_ids[..])) {
        cue(Cue::Item(c.item(game.pack[i]).kind));
        held = Some(Held { from: From::Pack(i), item: game.pack[i], at: pointer().into(), moved: false });
    }
    // Enter wears or drinks the first match on the selected unit.
    if filter.pick {
        if let Some(&first) = kept_ids.first() {
            *message = use_pack_item(game, sel, first);
        }
    }
    if let Some(h) = held.as_mut() {
        h.moved |= Vec2::from(pointer()).distance(h.at) > DRAG_START;
        if h.moved {
            hover = Some(h.item);
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
        let b = Rect::new(face.x + face.w + 12.0 * k, row.y + 14.0 * k, row.w - face.w - 24.0 * k, row.h - 28.0 * k);
        // One confirmation step, as the original's confirm and cancel buttons.
        let confirming = CONFIRM.with(|c| c.get()) == Some(sel);
        if !confirming {
            if button(b.x, b.y, b.w, b.h, label, true) {
                CONFIRM.with(|c| c.set(Some(sel)));
                // Its worn items go with it: said before it is confirmed.
                let worn = u.items.iter().flatten().count();
                if worn > 0 {
                    *message = Some(razdor::trf!("{name} wears {n} item(s): they will be lost with it. Take them off first to keep them.", name = u.name(&c), n = worn));
                }
            }
        } else {
            let half = (b.w - 6.0 * k) / 2.0;
            if button(b.x, b.y, half, b.h, tr("Confirm"), true) {
                CONFIRM.with(|c| c.set(None));
                // It leaves when the effect over its card ends (0x4b11cc kind 2).
                card_fx(2, sel, true);
            } else if button(b.x + half + 6.0 * k, b.y, half, b.h, tr("Cancel"), true) {
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
        draw_rectangle(p.x + 4.0 * k, p.y + 4.0 * k, card.x, card.y, Color::new(0.0, 0.0, 0.0, 0.45));
        assets.draw_portrait(v.def, Team::Player, sq);
        chrome::wounds(sq, v.hp, v.max_hp(&c));
        draw_rectangle_lines(sq.x, sq.y, sq.w, sq.h, 1.0, Color::new(0.85, 0.85, 0.85, 0.8));
        let vs = v.stats(&c);
        unit_sheet::stat_strip(Rect::new(p.x, p.y + card.x, card.x, card.y - card.x), &vs, &vs, vs[Stat::MagicPower], unit_sheet::caster(&c, v.def), unit_sheet::strip_place(f, v.slot), v.hp, super::building_view::back_row_def(&c, v.slot), selected.selected == Some(i), unit_sheet::StripPanel::of_squad(i, v.named));
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
        if let Some(fx) = CARD_FX.with(|f| f.get()).filter(|fx| fx.unit == i) {
            super::battle_view::draw_effect(&c, fx.effect, sq, (now_ms() - fx.t0_ms) as f32 / fx.ms as f32);
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
        (None, None) => (tr("Drag an item onto a unit to give it; Esc returns").to_string(), Color::new(1.0, 0.55, 0.25, 1.0)),
    };
    chrome::hint_strip(strip, &hint, hc);
    // The press, as the original's (0x4c346c), ignored while a card slides (busy 0x68dc63).
    if let (Some((on, slot)), None) = (pressed, slide) {
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
                    super::unit_drag::press(i, game.squad[i].def);
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

    // The item under the pointer, in the pack or worn: its description pops up by the pointer
    // (not while one is carried).
    if let Some(item) = hover.filter(|_| held.is_none()) {
        let (pw, ph) = (272.0 * k, 240.0 * k);
        let (mx, my) = pointer();
        let px = if mx + 21.0 * k + pw > sw { mx - 21.0 * k - pw } else { mx + 21.0 * k };
        let py = (my + 37.0 * k).min(sh - ph - 4.0).max(4.0);
        draw_rectangle(px + 6.0 * k, py + 6.0 * k, pw, ph, Color::new(0.0, 0.0, 0.0, 0.45));
        super::building_view::item_description(game, assets, item, px, py, pw, ph);
    }

    if close || (!filter.keys_taken && (key(KeyCode::Escape) || key(KeyCode::A))) {
        HELD.with(|c| c.set(None));
        FILTER.with(|f| f.borrow_mut().clear());
        super::unit_drag::cancel();
        SLIDE.with(|c| c.set(None));
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
        SLIDE.with(|c| c.set(None));
        super::unit_drag::cancel();
        FILTER.with(|f| f.borrow_mut().clear());
    }
    next
}

#[cfg(test)]
mod tests {
    use super::{ArmySel, GridPress};

    fn sel(selected: Option<usize>, shown: Option<usize>) -> ArmySel {
        ArmySel { selected, shown }
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
}
