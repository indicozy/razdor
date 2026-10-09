//! What the interface plays at a game moment, by name: the sound effects (their `_Sounds.ini`
//! keys), the music tracks (`[Backgrounds]` keys) and the animations, in a vocabulary shared
//! with the diff test's trace of the original (`tools/difftest/av.py`).
//!
//! The interface decides what to play (`ui::audio`, `ui::battle_view`); the choices that
//! depend on the rules live here so that the replay (`difftest`), which has no window, logs
//! the same names at the same points without drawing ([`AvLog`]). Nothing here plays or
//! draws anything.

use serde::Serialize;

use crate::dt::data::ArtefactType;
use crate::rules::content::Stat;
use crate::rules::battle::{ActionKind, Battle, Team};

/// The battle themes (`[Backgrounds]`): against a building's garrison, against an army.
pub const BATTLE: [&str; 2] = ["BkgBattle1", "BkgBattle2"];
/// A won battle's piece, looped over the map until the next map track.
pub const TRIUMPH: &str = "BkgTriumph";
/// The game lost.
pub const DEFEAT: &str = "BkgDefeat";

/// The sound of a battle action (interface.md §12): melee and long strike `Battle-Fight`, a
/// shot `Battle-Shoot`, or `Battle-Strike` (cannon) when the shooter's ranged attack is at
/// least `ShotWeaponRange`, heal `Battle-Cure`, bless `Battle-Bless`, curse and magic strike
/// `Battle-Sorcery`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BattleSound {
    Fight,
    Shoot,
    Cannon,
    Cure,
    Bless,
    Sorcery,
}

impl BattleSound {
    /// The sound of `actor`'s action `kind` in `battle`.
    pub fn of(battle: &Battle, actor: usize, kind: ActionKind) -> BattleSound {
        match kind {
            ActionKind::Melee | ActionKind::LongStrike => BattleSound::Fight,
            ActionKind::Shot if battle.fighters[actor].stats[Stat::AttackShot] >= battle.content().options.shot_weapon_range => BattleSound::Cannon,
            ActionKind::Shot => BattleSound::Shoot,
            ActionKind::Heal => BattleSound::Cure,
            ActionKind::Bless => BattleSound::Bless,
            ActionKind::Strike | ActionKind::Curse => BattleSound::Sorcery,
        }
    }

    /// The `[SFX-Effects]` key.
    pub fn key(self) -> &'static str {
        match self {
            BattleSound::Fight => "Battle-Fight",
            BattleSound::Shoot => "Battle-Shoot",
            BattleSound::Cannon => "Battle-Strike",
            BattleSound::Cure => "Battle-Cure",
            BattleSound::Bless => "Battle-Bless",
            BattleSound::Sorcery => "Battle-Sorcery",
        }
    }
}

/// The class of the effect picture Razdor draws on the target of an action
/// (`ui::battle_view::effect_number`), named as the original's effect numbers (0x4afe7c: 0 shot,
/// 1 melee, 2 magic, 3 bless, 4 cure).
pub fn battle_effect(kind: ActionKind) -> &'static str {
    match kind {
        ActionKind::Melee | ActionKind::LongStrike => "melee",
        ActionKind::Shot => "shot",
        ActionKind::Strike | ActionKind::Curse => "magic",
        ActionKind::Heal => "cure",
        ActionKind::Bless => "bless",
    }
}

/// What follows the effect on the target of an action (interface.md §12, 0x4c43e8 result
/// codes 6 and 0x10).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Echo {
    /// A counterblow or a preventive strike: the slide back from the target to the actor and
    /// the action's effect, with its sound, on the actor.
    Counter,
    /// The target was a `DeathCurse` (or `Ghost`) unit and its death killed the actor: the
    /// sorcery effect on the actor, with `Battle-Sorcery`, no slide.
    Curse,
}

/// The second part of the animation of `hit`, done by `actor` (the battle as it stands after
/// the action).
pub fn echo(battle: &Battle, actor: usize, hit: &crate::rules::battle::Hit) -> Option<Echo> {
    use crate::rules::content::Bonus;
    if hit.counter.is_some() {
        return Some(Echo::Counter);
    }
    let (a, t) = (&battle.fighters[actor], &battle.fighters[hit.target]);
    let ghost = t.base.has(&Bonus::Ghost) && a.stats[Stat::ProtectDeath] < 30 * t.base[Stat::Manevres];
    (hit.killed && !a.alive() && (t.base.has(&Bonus::DeathCurse) || ghost)).then_some(Echo::Curse)
}

/// The won battle's hold (0x4b09e8): the battle screen stays up this long, the experience on
/// the cards, before it closes and the report follows.
pub const BATTLE_END_HOLD_MS: u32 = 2500;
/// Then the screen closes and the report (the chained event step, 0x4af658) follows this
/// much later.
pub const BATTLE_REPORT_GAP_MS: u32 = 250;
/// A pass in battle (0x4afb54): a pause with the busy pointer.
pub const BATTLE_PASS_MS: u32 = 100;

/// The sound of an item of type `kind` (`Item-<Type>`; trade goods `Item-Item`).
pub fn item_sound(kind: ArtefactType) -> String {
    format!("Item-{kind:?}")
}

/// The event chord `k` (the game generator's `Random(3)`): `Global-Event-1..3`.
pub fn chord(k: u32) -> String {
    format!("Global-Event-{}", k % 3 + 1)
}

/// A battle card as the diff test names it: `side:row:col` (side 1 the player's, 2 the
/// enemy's; rows 1 front, 2 back, 3 reserve; columns 1–6).
pub fn card(battle: &Battle, fighter: usize) -> String {
    let f = &battle.fighters[fighter];
    let side = if f.team == Team::Player { 1 } else { 2 };
    format!("{side}:{}:{}", f.slot.row.number(), f.slot.col as i32 + 1)
}

/// What kind of thing is played.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum AvKind {
    /// A sound effect, by its `[SFX-Effects]` key.
    Sfx,
    /// A music track started, by its `[Backgrounds]` key.
    Music,
    /// An animation started (`battle_slide`, `battle_effect:melee`, `card_slide`, `walk`...).
    Anim,
}

/// One sound, track or animation, with its target when it has one (a battle card, a cell).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct AvEvent {
    pub k: AvKind,
    pub n: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
}

/// The sounds and animations of a stretch of play, in order.
#[derive(Clone, Debug, Default)]
pub struct AvLog(Vec<AvEvent>);

impl AvLog {
    pub fn push(&mut self, k: AvKind, n: impl Into<String>, t: Option<String>) {
        self.0.push(AvEvent { k, n: n.into(), t });
    }

    pub fn sfx(&mut self, n: impl Into<String>) {
        self.push(AvKind::Sfx, n, None);
    }

    pub fn music(&mut self, n: impl Into<String>) {
        self.push(AvKind::Music, n, None);
    }

    pub fn anim(&mut self, n: impl Into<String>, t: Option<String>) {
        self.push(AvKind::Anim, n, t);
    }

    /// The events so far, emptying the log.
    pub fn take(&mut self) -> Vec<AvEvent> {
        std::mem::take(&mut self.0)
    }
}
