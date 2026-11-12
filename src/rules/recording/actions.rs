//! The player's actions, as the rules record them at their entry points (`Game` and `Battle`
//! methods the screen calls), how they are encoded in the stream, and how a replay applies them
//! by calling the same methods. A method records itself with [`scope`] (or [`battle_scope`]):
//! the first such call on the stack records, nested calls do not, and nothing is recorded inside
//! an engine scope (a tick, a script run, a restore), so only what the player did is recorded.

use super::super::battle::{Battle, Team, MAX_QUICK_BUDGET};
use super::super::content::UnitId;
use super::super::formation::{Formation, Row, Slot};
use super::super::game::{Event, Game};
use super::super::magic::CastTarget;
use super::snapshot::Settings;
use super::hook;
use super::stream::{put_i, put_u, Reader};

/// What the player did on the map, in a town, with the squad, or in a battle (`Battle`).
#[derive(Clone, Debug, PartialEq)]
pub enum PlayerAction {
    /// A walk to a cell (`Game::set_destination`).
    Walk { x: i32, y: i32 },
    /// Waits 1 or 4 hours (`Game::begin_wait`); the endless wait (F4) and its end.
    Wait { hours: u32 },
    EndlessWait,
    EndEndlessWait,
    /// The look-around of the map (`Game::look_around`).
    LookAround,
    /// The answer to a scenario question, the closing of an event's window, the release of an
    /// event the screen did not show, a forced event (the cheat console).
    Answer { yes: bool },
    WindowClosed,
    ReleaseUnshown { shown: bool },
    /// The event chord a window opens with (`Game::event_chord`): one draw of the generator, made
    /// by the screen as the window opens (a playback makes the same draw there).
    Chord,
    /// A window over the map was closed (`Game::window_closed`): the events its change allows are checked.
    MapWindowClosed,
    /// The events the engine produced outside a tick are taken (`Game::drain_events`, with events
    /// waiting).
    DrainEvents,
    ForceEvent { id: u16 },
    /// A hall's quest or rumour taken (`Game::take_hall_entry`), re-entering and entering a building.
    TakeHall { id: u16 },
    Reenter,
    EnterWaiting,
    /// A spell read on the map (`Game::begin_cast`), or cast at once (`Game::cast`).
    BeginCast { spell: u32, target: CastTarget },
    Cast { spell: u32, target: CastTarget },
    /// The army window opened (it recounts the hero's army, `Game::army_window_opened`).
    ArmyWindowOpened,
    /// Items: pressed in the pack or on a worn slot, given to a unit, dropped, drunk, equipped.
    PressPack { slot: u32 },
    PressWorn { unit: u32, slot: u32 },
    GiveHeld { to: u32 },
    DropHeld,
    Drink { unit: u32, pack: u32 },
    Equip { unit: u32, pack: u32 },
    Unequip { unit: u32, slot: u32 },
    /// Towns: market, barracks, healing, resurrection, sanctuary, garrison, guards, offers, tribute, ships.
    Buy { stock: u32 },
    Sell { pack: u32 },
    Hire { unit: u32 },
    Heal { unit: u32 },
    Resurrect { unit: u32 },
    LearnSpell { id: u32 },
    OpenGarrison,
    LeaveInGarrison { unit: u32, cell: Option<Slot> },
    TakeFromStore { index: u32 },
    TakeFromGarrison { j: u32, cell: Option<Slot>, pay: bool },
    SwapWithGarrison { i: u32, j: u32, garrison_first: bool },
    MoveGuard { j: u32, cell: Slot },
    Dismiss { i: u32 },
    AcceptOffer,
    DeclineOffer,
    FinishOffer,
    CollectTribute,
    RentShip,
    /// The squad: benching in battle, the front row's width, the hero's name, a unit's cell,
    /// a promotion.
    SetBenched { index: u32, disabled: bool },
    SetFormation { cols: u8, reserve: bool },
    SetHeroName { name: String },
    MoveUnit { unit: u32, cell: Slot },
    Promote { unit: u32, to: u32 },
    /// A battle starts against the foe the game set (`Game::start_battle`) and is resolved
    /// (`Game::resolve_battle`).
    StartBattle,
    ResolveBattle,
    /// «Отступить» before the battle's first action (`Game::retreat_battle`): the battle is dropped
    /// and the hero goes back to the cell he came from.
    RetreatBattle,
    /// The rule settings the game follows (`Game::set_rule_settings`), changed in the game.
    Settings(Settings),
    /// A player's action in the battle under way.
    Battle(BattleAct),
}

/// A player's action in a battle (`Battle` methods the screen calls).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BattleAct {
    MoveCard { from: Slot, to: Slot },
    MoveActive { to: Slot },
    Pass,
    Act { target: u32 },
    /// The active unit does the action `kind` (an `ActionKind` code) to `target`
    /// (`Battle::act_with`: the screen's card press).
    ActWith { target: u32, kind: u8 },
    AutoStep,
    HeroAutoStep,
    HeroAutoPlay,
    /// «Максимум» plays the rest of the battle (`Battle::max_play_to_end`, the quick budget).
    MaxPlay,
    /// The battle ends with `winner` (the retreat and the cheat console).
    ForceEnd { enemy_wins: bool },
    /// A click on the active unit's own card or the space key (`Battle::own_cell`).
    OwnCell,
    /// The player's first action: the deployment ends (`Battle::start_play`). A battle recorded
    /// before the deployment step had no such entry; its first action began it (`play.rs`).
    Begin,
}

/// Records an action when it is the outermost one and recording is on; dropping it ends the
/// scope.
pub struct Scope {
    _private: (),
}

impl Drop for Scope {
    fn drop(&mut self) {
        hook::leave_action();
    }
}

/// The scope of a `Game` entry point: when it is the outermost action of a non-engine call, the
/// paused step (if any) is finished first, whether or not a recorder is attached (an action never
/// sees a half-played step, with or without recording: the recording changes nothing), and then
/// `make()` is recorded at that place when recording is on.
pub fn scope(g: &mut Game, make: impl FnOnce() -> PlayerAction) -> Scope {
    if hook::enter_action() && !hook::in_engine() {
        g.finish_pause();
        // What the action records (its own engine records too) is at the place it came at.
        hook::set_now(g.stamp());
        if hook::recording() {
            let at = g.stamp();
            hook::record_input(Some(at), super::stream::Input::Player(make()));
        }
    }
    Scope { _private: () }
}

/// The scope of a `Battle` entry point (the battle's own time has no game stamp: the entry takes
/// the stamp of the game's last tick, and the order of the stream places it).
pub fn battle_scope(make: impl FnOnce() -> BattleAct) -> Scope {
    if hook::enter_action() && hook::recording() && !hook::in_engine() {
        let at = hook::now();
        hook::record_input(Some(at), super::stream::Input::Player(PlayerAction::Battle(make())));
    }
    Scope { _private: () }
}

/// Applies a recorded `Game` action by calling the entry point it came from, and gives the events
/// it raised (the windows and messages a screen shows). `StartBattle` and `ResolveBattle` and the
/// battle's own actions are the caller's (they need the battle).
pub fn apply_game_events(g: &mut Game, a: &PlayerAction) -> Vec<Event> {
    match a {
        PlayerAction::Answer { yes } => g.answer_question(*yes),
        PlayerAction::WindowClosed => g.event_window_closed(),
        PlayerAction::ReleaseUnshown { shown } => g.release_unshown_window(*shown),
        PlayerAction::MapWindowClosed => g.window_closed(),
        PlayerAction::DrainEvents => g.drain_events(),
        PlayerAction::ForceEvent { id } => g.force_event(*id),
        PlayerAction::TakeHall { id } => g.take_hall_entry(*id).unwrap_or_default(),
        _ => {
            apply_game(g, a);
            Vec::new()
        }
    }
}

/// Applies a recorded `Game` action that gives no events (see [`apply_game_events`]).
pub fn apply_game(g: &mut Game, a: &PlayerAction) {
    match a {
        PlayerAction::Walk { x, y } => {
            g.set_destination((*x, *y));
        }
        PlayerAction::Wait { hours } => g.begin_wait(*hours),
        PlayerAction::EndlessWait => g.begin_endless_wait(),
        PlayerAction::EndEndlessWait => g.end_endless_wait(),
        PlayerAction::LookAround => {
            g.look_around();
        }
        PlayerAction::Answer { .. }
        | PlayerAction::WindowClosed
        | PlayerAction::ReleaseUnshown { .. }
        | PlayerAction::MapWindowClosed
        | PlayerAction::DrainEvents
        | PlayerAction::ForceEvent { .. }
        | PlayerAction::TakeHall { .. } => {
            let _ = apply_game_events(g, a);
        }
        PlayerAction::Chord => {
            g.event_chord();
        }
        PlayerAction::Reenter => {
            g.reenter_building();
        }
        PlayerAction::EnterWaiting => {
            g.enter_waiting_building();
        }
        PlayerAction::BeginCast { spell, target } => {
            let _ = g.begin_cast(*spell, *target);
        }
        PlayerAction::Cast { spell, target } => {
            let _ = g.cast(*spell, *target);
        }
        PlayerAction::ArmyWindowOpened => g.army_window_opened(),
        PlayerAction::PressPack { slot } => {
            let _ = g.press_pack(*slot as usize);
        }
        PlayerAction::PressWorn { unit, slot } => {
            let _ = g.press_worn(*unit as usize, *slot as usize);
        }
        PlayerAction::GiveHeld { to } => {
            let _ = g.give_held(*to as usize);
        }
        PlayerAction::DropHeld => {
            g.drop_held();
        }
        PlayerAction::Drink { unit, pack } => {
            let _ = g.drink(*unit as usize, *pack as usize);
        }
        PlayerAction::Equip { unit, pack } => {
            let _ = g.equip(*unit as usize, *pack as usize);
        }
        PlayerAction::Unequip { unit, slot } => {
            let _ = g.unequip(*unit as usize, *slot as usize);
        }
        PlayerAction::Buy { stock } => {
            let _ = g.buy(*stock as usize);
        }
        PlayerAction::Sell { pack } => {
            let _ = g.sell(*pack as usize);
        }
        PlayerAction::Hire { unit } => {
            let _ = g.hire(UnitId(*unit));
        }
        PlayerAction::Heal { unit } => {
            let _ = g.heal(*unit as usize);
        }
        PlayerAction::Resurrect { unit } => {
            let _ = g.resurrect(*unit as usize);
        }
        PlayerAction::LearnSpell { id } => {
            let _ = g.learn_spell(*id);
        }
        PlayerAction::OpenGarrison => g.open_garrison(),
        PlayerAction::LeaveInGarrison { unit, cell } => {
            let _ = g.leave_in_garrison(*unit as usize, *cell);
        }
        PlayerAction::TakeFromStore { index } => {
            let _ = g.take_from_store(*index as usize);
        }
        PlayerAction::TakeFromGarrison { j, cell, pay } => {
            let _ = g.take_from_garrison(*j as usize, *cell, *pay);
        }
        PlayerAction::SwapWithGarrison { i, j, garrison_first } => {
            let _ = g.swap_with_garrison(*i as usize, *j as usize, *garrison_first);
        }
        PlayerAction::MoveGuard { j, cell } => {
            g.move_guard(*j as usize, *cell);
        }
        PlayerAction::Dismiss { i } => {
            let _ = g.dismiss(*i as usize);
        }
        PlayerAction::AcceptOffer => {
            g.accept_offer();
        }
        PlayerAction::DeclineOffer => {
            g.decline_offer();
        }
        PlayerAction::FinishOffer => g.finish_offer(),
        PlayerAction::CollectTribute => {
            g.collect_tribute();
        }
        PlayerAction::RentShip => {
            let _ = g.rent_ship();
        }
        PlayerAction::SetBenched { index, disabled } => {
            g.set_unit_battle_disabled(*index as usize, *disabled);
        }
        PlayerAction::SetFormation { cols, reserve } => g.set_formation(Formation { cols: *cols, reserve: *reserve }),
        PlayerAction::SetHeroName { name } => g.set_hero_name(name),
        PlayerAction::MoveUnit { unit, cell } => {
            g.move_unit(*unit as usize, *cell);
        }
        PlayerAction::Promote { unit, to } => {
            let _ = g.promote(*unit as usize, UnitId(*to));
        }
        PlayerAction::Settings(s) => g.set_rule_settings(s.clone()),
        // The playback drops the battle and calls `Game::retreat_battle` itself (it needs the battle).
        PlayerAction::StartBattle | PlayerAction::ResolveBattle | PlayerAction::RetreatBattle | PlayerAction::Battle(_) => {}
    }
}

/// Applies a recorded battle action to the battle under way.
pub fn apply_battle(b: &mut Battle, a: &BattleAct) {
    match a {
        BattleAct::MoveCard { from, to } => {
            let _ = b.move_card(*from, *to);
        }
        BattleAct::MoveActive { to } => {
            let _ = b.move_active(*to);
        }
        BattleAct::Pass => b.pass(),
        BattleAct::Act { target } => {
            let _ = b.act(*target as usize);
        }
        BattleAct::ActWith { target, kind } => {
            if let Some(kind) = crate::rules::battle::ActionKind::from_code(*kind) {
                let _ = b.act_with(*target as usize, kind);
            }
        }
        BattleAct::AutoStep => {
            b.auto_step();
        }
        BattleAct::HeroAutoStep => {
            b.hero_auto_step();
        }
        BattleAct::HeroAutoPlay => {
            b.hero_auto_play_to_end();
        }
        BattleAct::MaxPlay => {
            b.max_play_to_end(MAX_QUICK_BUDGET);
        }
        BattleAct::ForceEnd { enemy_wins } => b.force_end(if *enemy_wins { Team::Enemy } else { Team::Player }),
        BattleAct::OwnCell => {
            b.own_cell();
        }
        BattleAct::Begin => b.begin(),
    }
}

/// `Row`'s number in the stream (front 0, back 1, reserve 2).
fn row_code(row: Row) -> u64 {
    match row {
        Row::Front => 0,
        Row::Back => 1,
        Row::Reserve => 2,
    }
}

fn row_of(code: u64) -> Result<Row, String> {
    Ok(match code {
        0 => Row::Front,
        1 => Row::Back,
        2 => Row::Reserve,
        n => return Err(format!("a row of {n}")),
    })
}

fn put_slot(out: &mut Vec<u8>, s: Slot) {
    put_u(out, row_code(s.row));
    put_u(out, s.col as u64);
}

fn get_slot(r: &mut Reader) -> Result<Slot, String> {
    let row = row_of(r.u()?)?;
    Ok(Slot { row, col: r.u()? as u8 })
}

fn put_opt_slot(out: &mut Vec<u8>, s: Option<Slot>) {
    put_u(out, s.is_some() as u64);
    if let Some(s) = s {
        put_slot(out, s);
    }
}

fn get_opt_slot(r: &mut Reader) -> Result<Option<Slot>, String> {
    Ok(if r.u()? != 0 { Some(get_slot(r)?) } else { None })
}

fn put_target(out: &mut Vec<u8>, t: CastTarget) {
    match t {
        CastTarget::Own => put_u(out, 0),
        CastTarget::Army(uid) => {
            put_u(out, 1);
            put_u(out, uid as u64);
        }
    }
}

fn get_target(r: &mut Reader) -> Result<CastTarget, String> {
    Ok(match r.u()? {
        0 => CastTarget::Own,
        1 => CastTarget::Army(r.u()? as u32),
        n => return Err(format!("a target of {n}")),
    })
}

fn put_flag(out: &mut Vec<u8>, b: bool) {
    put_u(out, b as u64);
}

fn get_flag(r: &mut Reader) -> Result<bool, String> {
    match r.u()? {
        0 => Ok(false),
        1 => Ok(true),
        n => Err(format!("a flag of {n}")),
    }
}

fn put_battle_act(out: &mut Vec<u8>, a: &BattleAct) {
    match a {
        BattleAct::MoveCard { from, to } => {
            put_u(out, 1);
            put_slot(out, *from);
            put_slot(out, *to);
        }
        BattleAct::MoveActive { to } => {
            put_u(out, 2);
            put_slot(out, *to);
        }
        BattleAct::Pass => put_u(out, 3),
        BattleAct::Act { target } => {
            put_u(out, 4);
            put_u(out, *target as u64);
        }
        BattleAct::ActWith { target, kind } => {
            put_u(out, 11);
            put_u(out, *target as u64);
            put_u(out, *kind as u64);
        }
        BattleAct::AutoStep => put_u(out, 5),
        BattleAct::HeroAutoStep => put_u(out, 6),
        BattleAct::HeroAutoPlay => put_u(out, 7),
        BattleAct::MaxPlay => put_u(out, 8),
        BattleAct::ForceEnd { enemy_wins } => {
            put_u(out, 9);
            put_flag(out, *enemy_wins);
        }
        BattleAct::OwnCell => put_u(out, 10),
        BattleAct::Begin => put_u(out, 12),
    }
}

fn get_battle_act(r: &mut Reader) -> Result<BattleAct, String> {
    Ok(match r.u()? {
        1 => BattleAct::MoveCard { from: get_slot(r)?, to: get_slot(r)? },
        2 => BattleAct::MoveActive { to: get_slot(r)? },
        3 => BattleAct::Pass,
        4 => BattleAct::Act { target: r.u()? as u32 },
        11 => BattleAct::ActWith { target: r.u()? as u32, kind: r.u()? as u8 },
        5 => BattleAct::AutoStep,
        6 => BattleAct::HeroAutoStep,
        7 => BattleAct::HeroAutoPlay,
        8 => BattleAct::MaxPlay,
        9 => BattleAct::ForceEnd { enemy_wins: get_flag(r)? },
        10 => BattleAct::OwnCell,
        12 => BattleAct::Begin,
        n => return Err(format!("a battle action of {n}")),
    })
}

/// The bytes of an action: its tag, then its fields as varints.
pub fn put_action(out: &mut Vec<u8>, a: &PlayerAction) {
    use PlayerAction::*;
    let u = |out: &mut Vec<u8>, t: u64| put_u(out, t);
    match a {
        Walk { x, y } => {
            u(out, 1);
            put_i(out, *x as i64);
            put_i(out, *y as i64);
        }
        Wait { hours } => {
            u(out, 2);
            put_u(out, *hours as u64);
        }
        EndlessWait => u(out, 3),
        EndEndlessWait => u(out, 4),
        LookAround => u(out, 5),
        Answer { yes } => {
            u(out, 6);
            put_flag(out, *yes);
        }
        WindowClosed => u(out, 7),
        ReleaseUnshown { shown } => {
            u(out, 8);
            put_flag(out, *shown);
        }
        Chord => u(out, 51),
        MapWindowClosed => u(out, 52),
        DrainEvents => u(out, 53),
        ForceEvent { id } => {
            u(out, 9);
            put_u(out, *id as u64);
        }
        TakeHall { id } => {
            u(out, 10);
            put_u(out, *id as u64);
        }
        Reenter => u(out, 11),
        EnterWaiting => u(out, 12),
        BeginCast { spell, target } => {
            u(out, 13);
            put_u(out, *spell as u64);
            put_target(out, *target);
        }
        Cast { spell, target } => {
            u(out, 14);
            put_u(out, *spell as u64);
            put_target(out, *target);
        }
        ArmyWindowOpened => u(out, 15),
        PressPack { slot } => {
            u(out, 16);
            put_u(out, *slot as u64);
        }
        PressWorn { unit, slot } => {
            u(out, 17);
            put_u(out, *unit as u64);
            put_u(out, *slot as u64);
        }
        GiveHeld { to } => {
            u(out, 18);
            put_u(out, *to as u64);
        }
        DropHeld => u(out, 19),
        Drink { unit, pack } => {
            u(out, 20);
            put_u(out, *unit as u64);
            put_u(out, *pack as u64);
        }
        Equip { unit, pack } => {
            u(out, 21);
            put_u(out, *unit as u64);
            put_u(out, *pack as u64);
        }
        Unequip { unit, slot } => {
            u(out, 22);
            put_u(out, *unit as u64);
            put_u(out, *slot as u64);
        }
        Buy { stock } => {
            u(out, 23);
            put_u(out, *stock as u64);
        }
        Sell { pack } => {
            u(out, 24);
            put_u(out, *pack as u64);
        }
        Hire { unit } => {
            u(out, 25);
            put_u(out, *unit as u64);
        }
        Heal { unit } => {
            u(out, 26);
            put_u(out, *unit as u64);
        }
        Resurrect { unit } => {
            u(out, 27);
            put_u(out, *unit as u64);
        }
        LearnSpell { id } => {
            u(out, 28);
            put_u(out, *id as u64);
        }
        OpenGarrison => u(out, 29),
        LeaveInGarrison { unit, cell } => {
            u(out, 30);
            put_u(out, *unit as u64);
            put_opt_slot(out, *cell);
        }
        TakeFromStore { index } => {
            u(out, 31);
            put_u(out, *index as u64);
        }
        TakeFromGarrison { j, cell, pay } => {
            u(out, 32);
            put_u(out, *j as u64);
            put_opt_slot(out, *cell);
            put_flag(out, *pay);
        }
        SwapWithGarrison { i, j, garrison_first } => {
            u(out, 33);
            put_u(out, *i as u64);
            put_u(out, *j as u64);
            put_flag(out, *garrison_first);
        }
        MoveGuard { j, cell } => {
            u(out, 34);
            put_u(out, *j as u64);
            put_slot(out, *cell);
        }
        Dismiss { i } => {
            u(out, 35);
            put_u(out, *i as u64);
        }
        AcceptOffer => u(out, 36),
        DeclineOffer => u(out, 37),
        FinishOffer => u(out, 38),
        CollectTribute => u(out, 39),
        RentShip => u(out, 40),
        SetBenched { index, disabled } => {
            u(out, 41);
            put_u(out, *index as u64);
            put_flag(out, *disabled);
        }
        SetFormation { cols, reserve } => {
            u(out, 42);
            put_u(out, *cols as u64);
            put_flag(out, *reserve);
        }
        SetHeroName { name } => {
            u(out, 43);
            put_u(out, name.len() as u64);
            out.extend_from_slice(name.as_bytes());
        }
        MoveUnit { unit, cell } => {
            u(out, 44);
            put_u(out, *unit as u64);
            put_slot(out, *cell);
        }
        Promote { unit, to } => {
            u(out, 45);
            put_u(out, *unit as u64);
            put_u(out, *to as u64);
        }
        StartBattle => u(out, 46),
        ResolveBattle => u(out, 47),
        RetreatBattle => u(out, 54),
        Settings(s) => {
            u(out, 48);
            put_flag(out, s.improved_ai);
            put_flag(out, s.max_ai);
            put_flag(out, s.max_guard);
            put_flag(out, s.keep_xp_on_promotion);
            put_flag(out, s.keep_formation);
            put_flag(out, s.allow_unit_benching);
            put_flag(out, s.pursue);
            put_u(out, s.walk_pace.to_bits() as u64);
        }
        Battle(act) => {
            u(out, 50);
            put_battle_act(out, act);
        }
    }
}

/// The action [`put_action`] wrote.
pub fn get_action(r: &mut Reader) -> Result<PlayerAction, String> {
    use PlayerAction::*;
    Ok(match r.u()? {
        1 => Walk { x: r.i()? as i32, y: r.i()? as i32 },
        2 => Wait { hours: r.u()? as u32 },
        3 => EndlessWait,
        4 => EndEndlessWait,
        5 => LookAround,
        6 => Answer { yes: get_flag(r)? },
        7 => WindowClosed,
        8 => ReleaseUnshown { shown: get_flag(r)? },
        51 => Chord,
        52 => MapWindowClosed,
        53 => DrainEvents,
        9 => ForceEvent { id: r.u()? as u16 },
        10 => TakeHall { id: r.u()? as u16 },
        11 => Reenter,
        12 => EnterWaiting,
        13 => BeginCast { spell: r.u()? as u32, target: get_target(r)? },
        14 => Cast { spell: r.u()? as u32, target: get_target(r)? },
        15 => ArmyWindowOpened,
        16 => PressPack { slot: r.u()? as u32 },
        17 => PressWorn { unit: r.u()? as u32, slot: r.u()? as u32 },
        18 => GiveHeld { to: r.u()? as u32 },
        19 => DropHeld,
        20 => Drink { unit: r.u()? as u32, pack: r.u()? as u32 },
        21 => Equip { unit: r.u()? as u32, pack: r.u()? as u32 },
        22 => Unequip { unit: r.u()? as u32, slot: r.u()? as u32 },
        23 => Buy { stock: r.u()? as u32 },
        24 => Sell { pack: r.u()? as u32 },
        25 => Hire { unit: r.u()? as u32 },
        26 => Heal { unit: r.u()? as u32 },
        27 => Resurrect { unit: r.u()? as u32 },
        28 => LearnSpell { id: r.u()? as u32 },
        29 => OpenGarrison,
        30 => LeaveInGarrison { unit: r.u()? as u32, cell: get_opt_slot(r)? },
        31 => TakeFromStore { index: r.u()? as u32 },
        32 => TakeFromGarrison { j: r.u()? as u32, cell: get_opt_slot(r)?, pay: get_flag(r)? },
        33 => SwapWithGarrison { i: r.u()? as u32, j: r.u()? as u32, garrison_first: get_flag(r)? },
        34 => MoveGuard { j: r.u()? as u32, cell: get_slot(r)? },
        35 => Dismiss { i: r.u()? as u32 },
        36 => AcceptOffer,
        37 => DeclineOffer,
        38 => FinishOffer,
        39 => CollectTribute,
        40 => RentShip,
        41 => SetBenched { index: r.u()? as u32, disabled: get_flag(r)? },
        42 => SetFormation { cols: r.u()? as u8, reserve: get_flag(r)? },
        43 => {
            let n = r.u()? as usize;
            let name = String::from_utf8(r.bytes(n)?).map_err(|e| e.to_string())?;
            SetHeroName { name }
        }
        44 => MoveUnit { unit: r.u()? as u32, cell: get_slot(r)? },
        45 => Promote { unit: r.u()? as u32, to: r.u()? as u32 },
        46 => StartBattle,
        47 => ResolveBattle,
        54 => RetreatBattle,
        48 => PlayerAction::Settings(super::snapshot::Settings {
            improved_ai: get_flag(r)?,
            max_ai: get_flag(r)?,
            max_guard: get_flag(r)?,
            keep_xp_on_promotion: get_flag(r)?,
            keep_formation: get_flag(r)?,
            allow_unit_benching: get_flag(r)?,
            pursue: get_flag(r)?,
            walk_pace: f32::from_bits(r.u()? as u32),
        }),
        50 => Battle(get_battle_act(r)?),
        n => return Err(format!("an action of {n}")),
    })
}


/// Every public method that changes the game (`&mut self`) in the files the screen's entry points
/// live in, is either recorded (its body opens an action scope, `scope` or `battle_scope`) or is
/// listed here as not a player's action: time, the engine, the setup of a game or a battle, the
/// tools and tests, and reads that take `&mut self` for a cache. A new entry point must be one or
/// the other, so the test fails until it is.
#[cfg(test)]
const NOT_PLAYER_GAME: &[&str] = &[
    // Time and the frame loop: the engine's own, recorded as its stamps.
    "tick", "tick_shown", "tick_shown_within", "tick_within", "finish_pause", "wait", "pass_time", "begin_pass",
    "take_music_wait", "music_rotate", "refresh_meeting_marks", "take_clashes",
    
    // Setup and tools: a map or a game set up for play, the cheat console's own state, the UI snapshots.
    "set_origin", "set_map", "seed_events", "reveal_area", "reveal", "note_settings",
    "note_cheat", "keep_run_mark", "set_xp_correction", "set_simulation", "set_building_defence", "auto_arrange",
    "set_bench", "set_improved_ai", "set_bare_strengths", "set_strength_defence", "reseat_all",
    "apply_carry_over", "from_campaign", "restart",
    "stop", "cut_walk", "cut_wait", "apply_max", "play_max_move", "max_step", "set_max_stakes",
    // The battle's engine, reached through the battle's own actions (recorded there).
    "battle_write_back", "skip_prediction",
    // The building window's and the events' engine.
    "apply_spell_to_army", "apply_spell_to_army_ext", "record", "start_round", "end_round",
    
    "set_level", "add", "remove", "wear", "take_off", "fit",
    "set_idle_offsets", "ai_init", "ai_move_begin", "ai_move_run", "ai_move_end", "ai_walk",
    "heal_full",
    
    
    
    "land", "walk_tail", "wait_tail", "fly_tail", "slice_tail", "hero_step",
    "pay_noon", "midnight", "new_day", "auto_tribute", "meet", "arrive",
    "stand_on_bridge", "look_around",
];

/// The battle's public methods that change it and are not a player's action (the AI's and the
/// setup's), the rest must be recorded.
#[cfg(test)]
const NOT_PLAYER_BATTLE: &[&str] = &[
    "begin", "ai_step", "auto_play_to_end", "set_max_ai", "set_max_guard", "set_bench", "set_xp_correction",
    "set_improved_ai", "set_building_defence", "set_bare_strengths", "auto_arrange", "set_simulation",
    "skip", "skip_prediction", "set_max_stakes", "max_choice_with", "max_choice",
    "max_step", "play_max_move", "apply_max",
    "fit_to_formation", "set_unit_battle_disabled", "set_strength_defence",
    // The screen's language (`Content::retitled`): the names follow it, nothing the player did in the battle.
    "retitle",
];

/// The methods of `file` taking `&mut self` (`pub fn`), with whether their body records an action.
#[cfg(test)]
fn scan(path: &str, owner: &str) -> Vec<(String, bool)> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(root.join(path)).unwrap_or_default();
    let re = regex_free_fns(&text, owner);
    re.into_iter()
        .map(|(name, body)| {
            let records = body.contains("actions::scope(") || body.contains("actions::battle_scope(");
            (name, records)
        })
        .collect()
}

/// `(name, body)` of each `pub fn name(&mut self` in `text`; a body runs to the next item at the
/// same indentation.
#[cfg(test)]
fn regex_free_fns(text: &str, owner: &str) -> Vec<(String, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut out = Vec::new();
    let mut in_owner = false;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with("impl ") {
            in_owner = line.starts_with(&format!("impl {owner} "));
            continue;
        }
        if !in_owner {
            continue;
        }
        let Some(rest) = line.strip_prefix("    pub fn ") else { continue };
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        if !rest[name.len()..].starts_with("(&mut self") {
            continue;
        }
        let mut body = String::new();
        for l in &lines[i..] {
            if !body.is_empty() && (l.starts_with("    pub ") || l.starts_with("    fn ") || l.starts_with("}") || l.starts_with("    #[cfg(test)]")) {
                break;
            }
            body.push_str(l);
            body.push('\n');
        }
        out.push((name, body));
    }
    out
}

#[test]
fn every_entry_point_that_changes_the_game_records_or_is_listed() {
    let mut missing = Vec::new();
    for path in ["src/rules/game.rs", "src/rules/script.rs", "src/rules/magic.rs", "src/rules/items.rs", "src/rules/town.rs", "src/rules/economy.rs", "src/rules/ships.rs"] {
        for (name, records) in scan(path, "Game") {
            if !records && !NOT_PLAYER_GAME.contains(&name.as_str()) {
                missing.push(format!("{path}: {name}"));
            }
        }
    }
    for path in ["src/rules/battle.rs", "src/rules/battle/max_ai.rs"] {
        for (name, records) in scan(path, "Battle") {
            if !records && !NOT_PLAYER_BATTLE.contains(&name.as_str()) {
                missing.push(format!("{path}: {name}"));
            }
        }
    }
    assert!(missing.is_empty(), "neither recorded nor listed as not a player's action:\n{}", missing.join("\n"));
}
