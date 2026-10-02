//! Card battles, following the original's rules (original-mechanics/battle.md).
//!
//! Each side stands in a [`Formation`] (front, back and reserve rows). The turn order is the
//! original's descending initiative threshold: at each threshold the player's units are
//! scanned in army order, then the enemy's, and a unit whose initiative reaches the threshold
//! spends all its actions (`Manevres`) in a row. Every action costs one: an attack, a spell, a
//! step or a pass. Damage is deterministic, and so is the AI. Blessings, curses and other
//! modifiers last until the next turn starts. The battle ends when a side is gone, when a side
//! has only units that surrender, or after the first action of turn `BattleEndTurn`, which the
//! player wins if any of his units stand. There is no retreat.

use std::sync::Arc;

use crate::i18n::tr;
use super::content::{Bonus, Content, HeroClass, ItemId, MagicSchool, Nature, Stat, UnitId};
use super::experience::{self, Role, SideUnit};
use super::formation::{Formation, Row, Slot};
use super::units::{Stats, Unit};

/// Turn 1 starts its initiative scan here (4840ec); later turns start where the first unit of
/// the turn before acted.
const TURN_ONE_THRESHOLD: i32 = 75;
/// The knight's army takes this % of physical damage: the exe sets 80 when `[GlobalOptions]`
/// loads (4e4501), whatever the ini says.
const KNIGHT_PERCENT: i32 = 80;
/// Regeneration a poison sets: `Poison`, `PoisonS`, `PoisonArmorIgnore` (at most).
const POISON_REGEN: i32 = -20;
const STRONG_POISON_REGEN: i32 = -25;
const PIERCING_POISON_REGEN: i32 = -10;
/// `CtrPoison`: regeneration its melee attacker loses per hit (stacks).
const CTR_POISON_STEP: i32 = 20;
/// A `Poison` mage poisons when its power after protection is above this.
const MAGE_POISON_POWER: i32 = 15;
/// `Exhaustion`: points of every magic protection lost per hostile spell.
const EXHAUSTION_POINTS: i32 = 10;
/// `Drying`: extra damage of a hostile spell, % of the target's max HP.
const DRYING_PERCENT: i32 = 8;
/// `Fortify`: defence bonus per turn after the first, % of DefenceBlow, for up to 5 turns.
const FORTIFY_PERCENT: i32 = 25;
const FORTIFY_TURNS: i32 = 5;
/// Most steps [`Battle::auto_play_to_end`] plays (as the AI's simulated battles).
pub const AUTO_PLAY_STEPS: usize = 20_000;
/// `Splash`: the first hit's ×80% and the follow-ups' ×40%, the patch's 32-bit multiply-high
/// constants (see [`splash_scale`]).
const SPLASH_MAIN: u64 = 0xCCCC_CCCD;
const SPLASH_SIDE: u64 = 0x6666_6666;
/// The Splash states (c26f16): the kind of action being splashed.
const SPLASH_MELEE: u8 = 1;
const SPLASH_SHOT: u8 = 2;
const SPLASH_MAGIC: u8 = 3;
const SPLASH_FRIENDLY: u8 = 4;
/// `Suicide`: the regeneration its unit is left with, which removes it at the next turn start.
const SUICIDE_REGEN: i32 = -99;
/// A side has at most 12 battle records.
const RECORDS: usize = 12;
/// The drain loader's floor for a type with magic power but no school: the unused slot of the
/// floor table holds these bytes (c28480).
const SCHOOLLESS_FLOOR: i32 = 16_777_215;
/// Community c25b63: the battle AI's factor for a melee target with 0 Manevres, 1.5 × the
/// number the four bytes at c25bc1 make (1 164 546 049).
const MANEVRES_0_FACTOR: f64 = 1_164_546_049.0 * 1.5;
/// `KillingStrike`: a target left at or below this % of max HP dies.
const KILLING_STRIKE_PERCENT: i32 = 25;
/// `Bleed`: the bleeding value a hit sets; each action start costs this % of AB + AS + MP.
const BLEED_PERCENT: i32 = 75;
/// `Stun`: initiative modifier lost per hit, % of the current initiative.
const STUN_PERCENT: i32 = 30;
/// `Berserk`: attack modifier = this % of AB × the share of HP lost.
const BERSERK_PERCENT: i32 = 75;
/// `Flock`: attack modifier ± this % of AB (or AS).
const FLOCK_PERCENT: i32 = 25;
/// `Artillery` and `FirstShot`: initiative on turn 1, twice with building defence ≥ 10.
const FIRST_TURN_INITIATIVE: i32 = 30;
/// `FateGift`: protections +20, regeneration +20, max HP +20%, initiative modifier +5.
const FATE_PROTECTION: i32 = 20;
const FATE_REGEN: i32 = 20;
const FATE_HP_PERCENT: i32 = 20;
const FATE_INITIATIVE: i32 = 5;
/// Undead Death casters' magic power floor is raised by this.
const UNDEAD_DEATH_FLOOR: i32 = 25;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Team {
    Player,
    Enemy,
}

impl Team {
    pub fn other(self) -> Team {
        match self {
            Team::Player => Team::Enemy,
            Team::Enemy => Team::Player,
        }
    }

    fn index(self) -> usize {
        self as usize
    }

    const BOTH: [Team; 2] = [Team::Player, Team::Enemy];
}

/// How the battle went. There is no draw: at the turn limit the player wins if any of his
/// units stand (4c50ec has no other branch).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ongoing,
    Victory,
    Defeat,
}

/// Why a finished battle ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// A side has no units left.
    Wiped,
    /// The first action of turn `BattleEndTurn` was made.
    TurnLimit,
    /// Every remaining unit of this side has `Surrender > 0`: it gave up.
    Surrender(Team),
}

/// What a unit does to a target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionKind {
    /// Warrior strike at an enemy front cell in columns c−1..c+1.
    Melee,
    /// Warrior strike through three empty cells at the nearest front unit on either side:
    /// halves the target's defence; `FlankStrike` doubles the attack.
    LongStrike,
    Shot,
    /// Hostile magic damage: on a target that already has a negative modifier this turn.
    Strike,
    /// Hostile magic debuff: on a target without one.
    Curse,
    /// Friendly magic on a wounded ally.
    Heal,
    /// Friendly magic on anyone else.
    Bless,
}

impl ActionKind {
    pub fn is_physical(self) -> bool {
        matches!(self, ActionKind::Melee | ActionKind::LongStrike | ActionKind::Shot)
    }

    pub fn is_melee(self) -> bool {
        matches!(self, ActionKind::Melee | ActionKind::LongStrike)
    }

    pub fn is_hostile(self) -> bool {
        !matches!(self, ActionKind::Heal | ActionKind::Bless)
    }

    pub fn label(self) -> &'static str {
        match self {
            ActionKind::Melee => tr("strike"),
            ActionKind::LongStrike => tr("long strike"),
            ActionKind::Shot => tr("shoot"),
            ActionKind::Strike => tr("magic strike"),
            ActionKind::Curse => tr("curse"),
            ActionKind::Heal => tr("heal"),
            ActionKind::Bless => tr("bless"),
        }
    }
}

/// Stat changes: of a blessing (positive) or curse (negative), or a unit's per-turn
/// modifiers (`actions` unused there).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Buff {
    /// Added to melee and ranged attack (only those the unit has).
    pub attack: i32,
    /// Added to melee and ranged defence.
    pub defence: i32,
    pub initiative: i32,
    /// Actions left this turn (Elemental magic).
    pub actions: i32,
}

impl Buff {
    pub fn is_empty(&self) -> bool {
        *self == Buff::default()
    }

    /// "Attack -3, Defence -5".
    pub fn describe(&self) -> String {
        let parts: Vec<String> = [
            (tr("Attack"), self.attack),
            (tr("Defence"), self.defence),
            (tr("Initiative"), self.initiative),
            (tr("Actions"), self.actions),
        ]
        .iter()
        .filter(|(_, v)| *v != 0)
        .map(|(n, v)| format!("{n} {v:+}"))
        .collect();
        parts.join(", ")
    }

    fn negative(&self) -> bool {
        self.attack < 0 || self.defence < 0 || self.initiative < 0
    }
}

/// The expected effect of an action, for hover previews.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    Damage(i32),
    Heal(i32),
    Buff(Buff),
}

/// The battle AI's view of a unit (4836cc).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AiRole {
    Warrior,
    Shooter,
    Mage,
}

#[derive(Clone, Debug)]
pub struct Fighter {
    pub unit: UnitId,
    pub name: String,
    pub team: Team,
    pub hp: i32,
    pub slot: Slot,
    /// The player's hero (squad index 0).
    pub is_hero: bool,
    /// Index into the player's squad, for writing results back.
    pub squad_index: Option<usize>,
    pub level: i32,
    /// XP towards the next level as the battle began (for display).
    pub xp: i32,
    /// Battle stats: the start of the battle (level, items, potions, spells) plus what the
    /// battle changed for good (EternalGift, ArmorBreaker, Bastion, Exhaustion …).
    pub base: Stats,
    /// Current stats: base with this turn's modifiers and the drained magic power.
    pub stats: Stats,
    /// The stats the unit began the battle with (after the deployment): the cards show
    /// every gain or loss against them.
    pub at_start: Stats,
    /// Magic power left after the per-turn drain.
    pub power: i32,
    /// This turn's attack, defence and initiative modifiers (blessings, curses, Stun,
    /// Berserk, Fortify, Flock …). Every turn start sets them to 0.
    pub mods: Buff,
    /// The items the unit wears (an enemy's too), for the panel.
    pub items: [Option<ItemId>; crate::rules::items::SLOTS],
    /// Blessed or cursed this turn.
    pub blessed: bool,
    pub cursed: bool,
    /// Actions left this turn.
    pub actions: i32,
    /// The current initiative (+0x95), which the turn order reads with the modifier: the base
    /// initiative as of the turn start, with the turn-1 Artillery and FirstShot +30. It is
    /// not a modifier, so it does not count where the original reads the initiative modifier
    /// (the Elemental AI's haste test). An EternalGift change to the base initiative waits
    /// for the next turn start (the original's).
    cur_initiative: i32,
    /// Regeneration % per turn; a poison replaces it with a negative value.
    pub regen: i32,
    /// Community `Bleed`: % of AB + AS + MP lost at each action start (0 = not bleeding).
    pub bleed: i32,
    /// May still move into or out of the reserve this turn.
    reserve_move: bool,
    /// Community `Suicide`: struck its blow and waits for its removal. Its HP is 0 (unless its
    /// vampirism gave some back), it has no actions, it cannot be targeted, but it still
    /// holds its cell and counts in its side's list until a removal.
    pub suicided: bool,
    /// `Surrender` of its type; a side left with only such units gives up.
    pub surrender: i32,
    /// Left the field by surrendering.
    pub surrendered: bool,
    /// Tactical cost at the start (experience.md §1), for the sides' strength.
    pub tactical: i32,
    /// Role in the side's strength sum, set at the start.
    pub role: Role,
    /// For the XP share: attacks and spells made, all actions taken (moves and passes too)
    /// and hit points lost through the damage routine (48a354): counter blows, preventive
    /// strikes, poison, bleeding and the Community side effects write the HP directly and do
    /// not count, as in the original.
    pub useful: i32,
    pub taken: i32,
    pub lost: i32,
    ai_power: i32,
    ai_role: AiRole,
    /// The editor's ruleset: its bonus 22–25, if any (the game has none of them).
    extra: Option<EditorBonus>,
    /// The editor's ruleset: a Parrying unit's guard, up after its pass (+0xa9).
    guard: bool,
    /// The strength field the side's strength and the auto-arrange weigh, when it is not the
    /// tactical cost: the battle tester's catalogue units count their gold cost.
    value: Option<i32>,
}

impl Fighter {
    fn new(content: &Content, unit: &Unit, team: Team, squad_index: Option<usize>, editor: bool) -> Fighter {
        let mut base = unit.stats(content);
        let mut extra = None;
        if editor {
            // The editor reads bonus tokens with its own name table: the Community ones are no
            // bonus, so they neither act nor overwrite; ids 22–25 come from the unit's type,
            // unless an item's bonus overwrites it.
            let own = usize::from(content.unit(unit.def).bonus.is_some());
            let from_items = base.bonuses.iter().skip(own).any(|b| b.vanilla_index().is_some());
            base.bonuses.retain(|b| b.vanilla_index().is_some());
            base.evasion = 0;
            extra = content.unit(unit.def).editor_bonus.and_then(EditorBonus::of_id).filter(|_| !from_items);
        }
        // One bonus per unit: each worn item with a bonus overwrites the unit's, the last one
        // wins (4919f0).
        if let Some(b) = base.bonuses.last().cloned() {
            base.bonuses = vec![b];
        }
        Fighter {
            unit: unit.def,
            name: unit.name(content).to_string(),
            team,
            hp: unit.hp.min(base.max_hp()),
            slot: unit.slot,
            is_hero: squad_index == Some(0),
            squad_index,
            level: unit.level,
            xp: unit.xp,
            power: base[Stat::MagicPower],
            regen: base[Stat::Regen],
            stats: base.clone(),
            at_start: base.clone(),
            base,
            mods: Buff::default(),
            items: unit.items,
            blessed: false,
            cursed: false,
            actions: 0,
            cur_initiative: 0,
            bleed: 0,
            reserve_move: true,
            suicided: false,
            // A byte: the side test is "not 0" and the mana adds the bytes (48b6ba, 48bfb4).
            surrender: content.unit(unit.def).surrender as u8 as i32,
            surrendered: false,
            tactical: 1,
            role: Role::Melee,
            useful: 0,
            taken: 0,
            lost: 0,
            ai_power: 0,
            ai_role: AiRole::Warrior,
            extra,
            guard: false,
            value: None,
        }
    }

    pub fn alive(&self) -> bool {
        self.hp > 0
    }

    /// Still in its side's list of records: alive, or a `Suicide` unit not removed yet.
    pub fn listed(&self) -> bool {
        self.alive() || self.suicided
    }

    /// On the field and able to be targeted or to act: alive and not waiting for removal.
    fn standing(&self) -> bool {
        self.alive() && !self.suicided
    }

    pub fn max_hp(&self) -> i32 {
        self.stats.max_hp()
    }

    /// Poisoned (or otherwise losing HP each turn).
    pub fn poisoned(&self) -> bool {
        self.regen < 0
    }

    /// Any negative attack, defence or initiative modifier this turn: a hostile mage strikes
    /// it instead of cursing.
    pub fn weakened(&self) -> bool {
        self.mods.negative()
    }

    fn is_warrior(&self) -> bool {
        self.base[Stat::AttackBlow] > 0
    }

    fn is_shooter(&self) -> bool {
        self.base[Stat::AttackShot] > 0
    }

    fn has_attack(&self) -> bool {
        self.is_warrior() || self.is_shooter()
    }

    fn has(&self, b: Bonus) -> bool {
        self.base.has(&b)
    }

    fn wounded(&self) -> bool {
        self.hp < self.max_hp()
    }
}

/// The result of one action, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub target: usize,
    pub kind: ActionKind,
    /// Damage dealt or HP restored.
    pub amount: i32,
    /// Blessing or curse applied.
    pub buff: Buff,
    pub killed: bool,
    /// Counterblow (after) and PreventiveStrike (before) damage taken by the actor, summed
    /// over the Splash follow-ups.
    pub counter: Option<i32>,
    /// The actor is down: killed a `DeathCurse`/`Ghost` unit, fell to the counterblow, the
    /// preventive strike or its bleeding, or it is a `Suicide` unit at 0 HP.
    pub actor_died: bool,
    /// Community `Splash`: the neighbours' damage or healing (fighter, amount).
    pub splash: Vec<(usize, i32)>,
}

impl Hit {
    fn new(target: usize, kind: ActionKind) -> Hit {
        Hit { target, kind, amount: 0, buff: Buff::default(), killed: false, counter: None, actor_died: false, splash: Vec::new() }
    }
}

/// One AI action, for the UI to animate.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Step {
    Act { actor: usize, hit: Hit },
    Move { actor: usize, from: Slot, to: Slot },
    /// One action passed.
    Wait { actor: usize },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ActionError {
    NotDeploying,
    InvalidTarget,
    NotYourTurn,
}

/// A unit's share of the XP after the battle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct XpAward {
    pub fighter: usize,
    pub xp: i32,
}

/// A side as the battle began, for the XP pool.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SideStart {
    pub strength: i64,
    pub hp: i64,
    pub count: usize,
}

/// Where a player's unit ended up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FighterResult {
    pub squad_index: usize,
    /// 0 when dead. A fallen hero whose army survived comes back with 1.
    pub hp: i32,
    pub slot: Slot,
}

/// A cell the AI's front-row fallback may pick (489549).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FrontPick {
    Move(Slot),
    Own,
    Cast(usize, ActionKind),
}

/// What the AI does with the active unit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    Act(usize, ActionKind),
    Move(Slot),
    Pass,
    /// The editor's pull (map code 3) of this enemy back-row unit.
    Pull(usize),
}

/// The Community `Splash` state, globals in `.mod` (c26f16 …): the kind of action being
/// splashed (0 for none), the primary target's side, row and column, the follow-ups made, the
/// two neighbours already taken, and the attacker's column at its last melee hit (c273c4).
#[derive(Clone, Copy, Debug, Default)]
struct Splash {
    state: u8,
    line: Option<(Team, Row)>,
    col: i32,
    count: u8,
    left: bool,
    right: bool,
    actor_col: i32,
}

/// The Community patch's globals that outlive a battle (`.mod`), shared by every battle, the
/// AI's off-screen ones included. Nothing resets them between battles and saves do not hold
/// them, as in the original.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PatchGlobals {
    /// `Hunger`: the living count stored at each removal (c252e4) and the value the Hunger
    /// units last saw (c2525f). Both are 3 in the file.
    pub hunger_counter: usize,
    pub hunger_seen: usize,
    /// Both living counts summed, of the battle object of the battle on screen (668cf8): the
    /// player's battle and its pre-simulation. An off-screen battle's removal stores this.
    pub screen_living: usize,
    /// The two side blocks the battle on screen is built from (669df8, 66a64c), whose living
    /// counts `Flock` compares: refreshed after each of its actions only (48bb10).
    pub side_blocks: [usize; 2],
}

impl PatchGlobals {
    const START: PatchGlobals = PatchGlobals { hunger_counter: 3, hunger_seen: 3, screen_living: 0, side_blocks: [0; 2] };
}

thread_local! {
    static PATCH: std::cell::Cell<PatchGlobals> = const { std::cell::Cell::new(PatchGlobals::START) };
}

/// The Community patch's globals now.
pub fn patch_globals() -> PatchGlobals {
    PATCH.with(|g| g.get())
}

fn patch_update(f: impl FnOnce(&mut PatchGlobals)) {
    PATCH.with(|g| {
        let mut v = g.get();
        f(&mut v);
        g.set(v);
    });
}

/// `x` × 80% or × 40% as the patch computes it (c270ae, c27337, c27374): a 32-bit unsigned
/// multiply-high. 80% is exactly ⌊0.8x⌋; the 40% constant is a hair low, so a multiple of 5
/// gives one less (10 → 3, 100 → 39). A negative `x` wraps around: ×80% gives a huge negative
/// value, ×40% about +1.7 billion (the original's).
fn splash_scale(x: i32, factor: u64) -> i32 {
    ((x as u32 as u64 * factor) >> 32) as u32 as i32
}

/// What one run of an action's case did to its target.
struct Struck {
    kind: ActionKind,
    amount: i32,
    buff: Buff,
    killed: bool,
}

impl Struck {
    fn new(kind: ActionKind) -> Struck {
        Struck { kind, amount: 0, buff: Buff::default(), killed: false }
    }
}

#[derive(Clone)]
pub struct Battle {
    content: Arc<Content>,
    pub formation: Formation,
    pub fighters: Vec<Fighter>,
    /// Battle turn, 1-based once the fight starts.
    pub round: u32,
    pub log: Vec<String>,
    /// The side that started the fight. The original gives its +1 initiative to the player
    /// whoever attacks, so this is for information only.
    pub attacker: Team,
    building_defence: [i32; 2],
    /// A side whose army's first unit is of the Knight type (49855c): it takes
    /// [`KNIGHT_PERCENT`] of physical damage, an AI lord's army as well as the player's.
    knight: [bool; 2],
    /// Each side's grid cells that exist (not blocked, −1 in the original), by row and
    /// column. The enemy keeps the wide row's blocks (48395c); the player's grid is rebuilt
    /// from his army's formation, which turns its blocked cells into open ones (4d2233). A
    /// collapse copies a row's blocks forward with its units (48a170).
    cells: [[[bool; 6]; 3]; 2],
    /// Mean base initiative of each side at the start of this turn (1 when 0), for the
    /// Elemental AI (4840ec).
    mean_initiative: [f64; 2],
    deploying: bool,
    /// Ended by the turn limit or a surrender (a wiped-out side needs no flag).
    ended: Option<EndReason>,
    /// The initiative scan: threshold, the threshold of the turn's first actor, and the
    /// cursor (side, position among the side's living units).
    threshold: i32,
    first_threshold: i32,
    cursor: (usize, usize),
    current: Option<usize>,
    /// Both sides at the start ([`Battle::begin`]).
    start: [SideStart; 2],
    /// The beaten army's experience correction for the player's XP (100 for a garrison).
    xp_correction: i32,
    /// A battle on screen (4ed424): only there do melee, shots and hostile spells record a
    /// Splash, and only its actions refresh Flock's side blocks. False for AI-vs-AI battles
    /// and the pre-simulation.
    interactive: bool,
    /// Played in the battle object of the battle on screen (668cf8): the player's battle and
    /// its pre-simulation, not the AI's off-screen battles. Its removals set Hunger's counter
    /// from its own living counts.
    screen_object: bool,
    /// Community `Splash` in progress.
    splash: Splash,
    /// Community `NoHeal` marks, by side and record index (c29600): they stay on the index
    /// when a death moves the records, so a mark can pass to another unit.
    crippled: [[bool; RECORDS]; 2],
    /// The AI's level (battle B+5): 1 normally, 2 with "improved enemy AI" (`OptValue9`), 0
    /// between AI armies. It decides when a target counts as killable.
    pub(crate) ai_level: u8,
    /// Mana a side's surrender gives the winner.
    surrender_mana: [i32; 2],
    /// The pre-simulation (48b75c): played before the first turn unless switched off.
    predict: bool,
    /// Each side's HP lost in the pre-simulation, the predicted loss of the XP pool (side +8).
    predicted: [i64; 2],
    /// Each side's HP lost this turn, and the most it lost in one turn before this one (side
    /// +0x10, +0x14; the turn in progress at the end is never counted).
    turn_lost: [i64; 2],
    max_turn_lost: [i64; 2],
    /// Cells of the player's army formation held by units that do not fight (the dead, and
    /// the unpaid when he attacks): the original's start fix sees them (4d2141).
    bench: Vec<Slot>,
    /// The game's rules, or the map editor's battle engine with its switches.
    rules: Rules,
}

/// A fighter in its side's strength sum.
fn side_unit(f: &Fighter) -> SideUnit {
    SideUnit { tactical: f.tactical, hp: f.hp, max_hp: f.max_hp(), row: f.slot.row, role: f.role }
}

/// Bonuses that give +1 action on the first turn (48431a).
const FAST_START: [Bonus; 3] = [Bonus::HorseAtack, Bonus::OldVampirsGist, Bonus::FastDead];
/// Piercing: the unit's own defence counts 0 (485908, hooks c2a27c and c2a3bf). Building
/// defence (and Row2Def against shots) still count.
const PIERCE_MELEE: [Bonus; 4] = [Bonus::ArmorIgnore, Bonus::VampirsGist, Bonus::OldVampirsGist, Bonus::PoisonArmorIgnore];
const PIERCE_SHOT: [Bonus; 3] = [Bonus::ArmorIgnore, Bonus::Artillery, Bonus::PoisonArmorIgnore];

/// A blessing of power `p` in `school` on a unit that has an attack (before the target's
/// nature and attack are taken into account).
pub fn bless_effect(o: &super::content::GlobalOptions, school: MagicSchool, p: i32) -> Buff {
    let (bm, bn, w) = (o.bless_main_spell.max(1), o.bless_next_spell.max(1), o.wizard_main_spell.max(1));
    match school {
        MagicSchool::Life => Buff { defence: 3 * p / (2 * bm) + 1, attack: 3 * p / (2 * bn), ..Buff::default() },
        MagicSchool::Elemental => Buff { actions: actions_of_power(p), initiative: p / w + 1, ..Buff::default() },
        MagicSchool::Death => Buff { attack: p / bm + 1, defence: p / bn, ..Buff::default() },
    }
}

/// A curse of hostile power `p` (already reduced by the target's protection) in `school`,
/// on a unit that has an attack.
pub fn curse_effect(o: &super::content::GlobalOptions, school: MagicSchool, p: i32) -> Buff {
    let (cm, cn, w) = (o.curse_main_spell.max(1), o.curse_next_spell.max(1), o.wizard_main_spell.max(1));
    // Life divides by the integer ⅔ of CurseMainSpell (4ed3a8) and a fixed 10 (4ed3b0).
    let life = (2 * cm / 3).max(1);
    match school {
        MagicSchool::Life => Buff { defence: -(p / life + 1), attack: -(p / 10), ..Buff::default() },
        MagicSchool::Elemental => Buff { actions: -actions_of_power(p), initiative: -(1 + p / w), ..Buff::default() },
        MagicSchool::Death => Buff { attack: -(1 + p / cm), defence: -(p / cn), ..Buff::default() },
    }
}

/// `f(P)` of mechanics.md 3.3: actions added or removed by Elemental magic.
fn actions_of_power(p: i32) -> i32 {
    match p {
        ..=19 => 0,
        20..=44 => 1,
        45..=99 => 2,
        _ => 3,
    }
}

fn god_bonus(s: &Stats) -> i32 {
    10 * i32::from(s.has(&Bonus::GodAnger)) + 20 * i32::from(s.has(&Bonus::GodStrike))
}

/// `n / d` rounded half to even, as Delphi's `Round`.
fn round_even(n: i64, d: i64) -> i64 {
    let (q, r) = (n.div_euclid(d), n.rem_euclid(d));
    match (2 * r).cmp(&d) {
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Equal => q + (q & 1),
    }
}

impl Battle {
    /// `player` entries are (squad index, unit); squad index 0 is the hero. Starts in the
    /// deploy phase; call [`Battle::begin`] to fight.
    pub fn new(content: Arc<Content>, player: &[(usize, &Unit)], enemies: &[Unit], attacker: Team) -> Battle {
        Battle::with_rules(content, player, enemies, attacker, Rules::Game)
    }

    /// [`Battle::new`] under `rules`: the game's, or the map editor's battle engine
    /// (testers.md §4), which the editor's battle tester and AI viewer run.
    pub fn with_rules(content: Arc<Content>, player: &[(usize, &Unit)], enemies: &[Unit], attacker: Team, rules: Rules) -> Battle {
        let editor = rules != Rules::Game;
        let mut fighters: Vec<Fighter> =
            player.iter().map(|&(idx, u)| Fighter::new(&content, u, Team::Player, Some(idx), editor)).collect();
        fighters.extend(enemies.iter().map(|u| Fighter::new(&content, u, Team::Enemy, None, editor)));
        let knight_led = |u: Option<&Unit>| u.is_some_and(|u| HeroClass::of_unit(u.def) == Some(HeroClass::Knight));
        let knight = [knight_led(player.first().map(|p| p.1)), knight_led(enemies.first())];
        let formation = content.formation;
        let mut cells = [[[false; 6]; 3]; 2];
        for (t, side) in cells.iter_mut().enumerate() {
            for (r, &row) in [Row::Front, Row::Back, Row::Reserve].iter().enumerate() {
                for c in 0..formation.cols.min(6) {
                    let open = formation.rows().contains(&row);
                    // The editor's sides are placed by hand, with no blocked cells in their grids
                    // (its cell test is the shape, `is_open`); the auto-arrange blocks them.
                    side[r][c as usize] = open && (editor || t == Team::Player.index() || formation.contains(Slot::new(row, c)));
                }
            }
        }
        let mut b = Battle {
            formation: content.formation,
            content,
            fighters,
            round: 0,
            log: Vec::new(),
            attacker,
            building_defence: [0; 2],
            knight,
            cells,
            mean_initiative: [1.0; 2],
            deploying: true,
            ended: None,
            threshold: 0,
            first_threshold: 0,
            cursor: (0, 0),
            current: None,
            start: [SideStart::default(); 2],
            xp_correction: 100,
            interactive: true,
            screen_object: true,
            splash: Splash::default(),
            crippled: [[false; RECORDS]; 2],
            ai_level: 1,
            surrender_mana: [0; 2],
            predict: true,
            predicted: [0; 2],
            turn_lost: [0; 2],
            max_turn_lost: [0; 2],
            bench: Vec::new(),
            rules,
        };
        if editor {
            // No Community globals: nothing the editor's battles do reaches them.
            b.interactive = false;
            b.screen_object = false;
        }
        b.fit_to_formation();
        b
    }

    /// Units standing outside the formation (a blocked cell of the wide row, an old save) or
    /// on a taken cell move to the first free one.
    fn fit_to_formation(&mut self) {
        for team in Team::BOTH {
            let mut taken: Vec<Slot> = Vec::new();
            for i in 0..self.fighters.len() {
                let f = &self.fighters[i];
                if f.team != team {
                    continue;
                }
                let slot = if self.formation.contains(f.slot) && !taken.contains(&f.slot) {
                    Some(f.slot)
                } else {
                    self.formation.free_slot(&taken, f.base.preferred_row())
                };
                if let Some(s) = slot {
                    self.fighters[i].slot = s;
                    taken.push(s);
                }
            }
        }
    }

    /// The experience correction (percent) of the army the player fights: it scales the
    /// player's XP (experience.md §3). A garrison's is 100.
    pub fn set_xp_correction(&mut self, percent: i32) {
        self.xp_correction = percent;
    }

    /// A battle between AI armies, played off screen in a battle object of its own: no
    /// `Splash` follow-ups but for heals and blessings, and the AI counts a target as
    /// killable only by one hit (B+5 = 0).
    pub fn set_simulation(&mut self) {
        self.interactive = false;
        self.screen_object = false;
        self.ai_level = 0;
    }

    /// The cells of the player's army formation whose units stay out of the battle (the dead,
    /// the unpaid of an attack). They count for the start fix in [`Battle::begin`].
    pub fn set_bench(&mut self, cells: Vec<Slot>) {
        self.bench = cells;
    }

    /// No pre-simulation: for battles whose XP nobody receives (the AI's target scoring),
    /// where its only result, the predicted loss, would go unused.
    pub fn skip_prediction(&mut self) {
        self.predict = false;
    }

    /// "Improved enemy AI in battle" (`OptValue9`): the enemy also counts a target as
    /// killable when its actions left can do it.
    pub fn set_improved_ai(&mut self, on: bool) {
        self.ai_level = if on { 2 } else { 1 };
    }

    /// The original's auto-arrange of a side (483b3c), for the enemy of a battle on screen
    /// and both sides off screen. The side's grid gets the wide row's blocks (48395c), then:
    /// the unit with the highest "front value" goes to the front row; up to 5 times the
    /// strongest unplaced non-warrior to the back row; then the rest, strongest first, a
    /// warrior to the front, else the reserve, else the back row, anyone else to the back,
    /// else the reserve, else the front. Each row fills its first free cell in the preferred
    /// column order; ties go to list order. The strength is the unit's tactical value, the
    /// roles and values are those of the side as built, its Garrison doubling included. Call
    /// after the building defence is set.
    pub fn auto_arrange(&mut self, team: Team) {
        let formation = self.formation;
        let side = &mut self.cells[team.index()];
        for (r, &row) in [Row::Front, Row::Back, Row::Reserve].iter().enumerate() {
            for c in 0..formation.cols.min(6) {
                side[r][c as usize] = formation.contains(Slot::new(row, c));
            }
        }
        let building = self.building_defence[team.index()];
        let ids = self.living_ids(team);
        let built: Vec<Stats> = ids
            .iter()
            .map(|&i| {
                let f = &self.fighters[i];
                let mut s = f.base.clone();
                s[Stat::MagicPower] = f.power;
                if f.has(Bonus::Garrison) && building >= 10 {
                    for st in [Stat::AttackBlow, Stat::DefenceBlow, Stat::DefenceShot] {
                        s[st] *= 2;
                    }
                }
                s
            })
            .collect();
        let roles: Vec<AiRole> = built.iter().map(|s| ai_power_role(s).1).collect();
        let strength: Vec<i64> = ids.iter().map(|&i| self.strength_field(i, building) as i64).collect();
        let mut cell: Vec<Option<Slot>> = vec![None; ids.len()];
        let order = formation.col_order();
        let place = |cell: &mut Vec<Option<Slot>>, k: usize, row: Row, open: &dyn Fn(Slot) -> bool| -> bool {
            let free = order.iter().map(|&c| Slot::new(row, c)).find(|&s| open(s) && !cell.contains(&Some(s)));
            if let Some(s) = free {
                cell[k] = Some(s);
            }
            free.is_some()
        };
        let open = |s: Slot| self.is_open(team, s);
        // Step 1: the front value, `HP × (Manevres × AB + DB, + DS for a warrior, ÷3 for a
        // shooter, ÷5 for a mage) + 1`; a Ghost's is 6 − Manevres.
        let front = |k: usize| {
            let (s, f) = (&built[k], &self.fighters[ids[k]]);
            let mut v = (s[Stat::Manevres] * s[Stat::AttackBlow] + s[Stat::DefenceBlow]) as i64;
            match roles[k] {
                AiRole::Warrior => v += s[Stat::DefenceShot] as i64,
                AiRole::Shooter => v /= 3,
                AiRole::Mage => v /= 5,
            }
            if f.has(Bonus::Ghost) { 6 - s[Stat::Manevres] as i64 } else { f.hp as i64 * v + 1 }
        };
        let best = |cell: &Vec<Option<Slot>>, value: &dyn Fn(usize) -> i64, ok: &dyn Fn(usize) -> bool| {
            let mut top: Option<(i64, usize)> = None;
            for (k, c) in cell.iter().enumerate() {
                let v = value(k);
                if c.is_none() && ok(k) && v > top.map_or(0, |t| t.0) {
                    top = Some((v, k));
                }
            }
            top.map(|t| t.1)
        };
        if let Some(k) = best(&cell, &front, &|_| true) {
            place(&mut cell, k, Row::Front, &open);
        }
        // Step 2: up to 5 tries, the strongest non-warrior to the back row (a try that finds
        // the back row full places nobody).
        for _ in 0..5 {
            if cell.iter().all(Option::is_some) {
                break;
            }
            if let Some(k) = best(&cell, &|k| strength[k], &|k| roles[k] != AiRole::Warrior) {
                place(&mut cell, k, Row::Back, &open);
            }
        }
        // Step 3: everyone else, strongest first.
        while let Some(k) = best(&cell, &|k| strength[k], &|_| true) {
            let rows = if roles[k] == AiRole::Warrior { [Row::Front, Row::Reserve, Row::Back] } else { [Row::Back, Row::Reserve, Row::Front] };
            if !rows.iter().any(|&row| place(&mut cell, k, row, &open)) {
                break;
            }
        }
        for (k, &i) in ids.iter().enumerate() {
            if let Some(s) = cell[k] {
                self.fighters[i].slot = s;
            }
        }
    }

    /// The strength field of fighter `i` (unit +0x6c): its tactical cost in `building`, or
    /// the value set with [`Battle::set_values`].
    fn strength_field(&self, i: usize, building: i32) -> i32 {
        let f = &self.fighters[i];
        f.value.unwrap_or_else(|| experience::tactical(&self.content, f.unit, &f.base, building))
    }

    /// Both sides as the battle began.
    pub fn start_of(&self, team: Team) -> SideStart {
        self.start[team.index()]
    }

    pub fn content(&self) -> &Content {
        &self.content
    }

    /// The stats of fighter `i` as the cards and the panel show them: its current stats with
    /// its side's building defence added to both defences, as the damage formula adds it,
    /// and, once the fighting has begun, the actions it has left this turn for its
    /// manoeuvres (more with haste or a first-turn bonus, fewer as it acts or when slowed).
    pub fn shown_stats(&self, i: usize) -> Stats {
        let f = &self.fighters[i];
        let mut s = f.stats.clone();
        let b = self.building_defence[f.team.index()];
        s[Stat::DefenceBlow] += b;
        s[Stat::DefenceShot] += b;
        if !self.deploying && self.round > 0 && f.alive() {
            s[Stat::Manevres] = f.actions.max(0);
        }
        s
    }

    /// The defence bonus `team` has from standing in its own building (0 in the open).
    pub fn building_defence(&self, team: Team) -> i32 {
        self.building_defence[team.index()]
    }

    /// Extra defence of the building `team` fights in (garrisons). Set before [`Battle::begin`].
    pub fn set_building_defence(&mut self, team: Team, defence: i32) {
        self.building_defence[team.index()] = defence;
    }

    pub fn is_deploying(&self) -> bool {
        self.deploying
    }

    /// Fighter of `team` holding `slot`: a living one, or a `Suicide` unit waiting for its
    /// removal (its grid cell still holds it, c2633c).
    pub fn at(&self, team: Team, slot: Slot) -> Option<usize> {
        self.fighters.iter().position(|f| f.listed() && f.team == team && f.slot == slot)
    }

    /// Deploy phase: move a player card to `to`, swapping with whoever is there.
    pub fn move_card(&mut self, from: Slot, to: Slot) -> Result<(), ActionError> {
        if !self.deploying {
            return Err(ActionError::NotDeploying);
        }
        if !self.formation.contains(to) {
            return Err(ActionError::InvalidTarget);
        }
        let a = self.at(Team::Player, from).ok_or(ActionError::InvalidTarget)?;
        if let Some(b) = self.at(Team::Player, to) {
            self.fighters[b].slot = from;
        }
        self.fighters[a].slot = to;
        Ok(())
    }

    /// End deployment and start turn 1.
    pub fn begin(&mut self) {
        if !self.deploying {
            return;
        }
        self.deploying = false;
        // The battle window's fix of the player's formation (4d2141): with nobody in front,
        // his back row moves into the front row, same columns, and stays there after the
        // battle. The reserve does not move, and the enemy needs no fix. There is no other
        // collapse before the first action. The test reads the army's formation, so a corpse
        // or a unit sitting out in the front row counts as somebody there (the original's).
        // The editor's battles have no such window, so no fix.
        if self.rules == Rules::Game && !self.row_occupied(Team::Player, Row::Front) && !self.bench.iter().any(|s| s.row == Row::Front) {
            for f in self.fighters.iter_mut().filter(|f| f.alive() && f.team == Team::Player && f.slot.row == Row::Back) {
                f.slot.row = Row::Front;
            }
        }
        for f in &mut self.fighters {
            f.at_start = f.stats.clone();
        }
        // Strength at the start, from the stats the units bring (items, spells) and the
        // building they stand in.
        for i in 0..self.fighters.len() {
            let tactical = self.strength_field(i, self.building_defence[self.fighters[i].team.index()]);
            let f = &mut self.fighters[i];
            f.tactical = tactical;
            f.role = experience::role(&f.base);
        }
        for team in Team::BOTH {
            let side: Vec<&Fighter> = self.fighters.iter().filter(|f| f.team == team && f.alive()).collect();
            self.start[team.index()] = SideStart {
                strength: experience::side_strength(&side.iter().map(|f| side_unit(f)).collect::<Vec<_>>()),
                hp: side.iter().map(|f| f.hp as i64).sum(),
                count: side.len(),
            };
        }
        for i in 0..self.fighters.len() {
            let building = self.building_defence[self.fighters[i].team.index()];
            let f = &mut self.fighters[i];
            // Garrison in a strong building: AB, DB and DS ×2; not AS (49861d).
            if f.has(Bonus::Garrison) && building >= 10 {
                for st in [Stat::AttackBlow, Stat::DefenceBlow, Stat::DefenceShot] {
                    f.base[st] *= 2;
                }
            }
            // Side 1, the player, gets +1 initiative whoever attacks (48b917).
            if f.team == Team::Player {
                f.base[Stat::Initiative] += 1;
            }
            // The battle AI's roles, from the setup's stats (483ecc → 4836cc).
            (f.ai_power, f.ai_role) = ai_power_role(&f.base);
        }
        // The battle on screen is built from its two side blocks, whose living counts Flock
        // reads until the first action refreshes them.
        let counts = Team::BOTH.map(|t| self.living(t).count());
        if self.screen_object {
            patch_update(|g| g.screen_living = counts[0] + counts[1]);
        }
        if self.interactive {
            patch_update(|g| g.side_blocks = counts);
        }
        // The pre-simulation (48b75c): the whole battle is first played once with the AI on
        // both sides and in the same battle object, without the on-screen flag (set only
        // afterwards), so with no Splash follow-ups but for heals and blessings; only each
        // side's HP lost is kept, as the XP pool's predicted loss.
        if self.predict {
            let mut sim = self.clone();
            sim.interactive = false;
            sim.start_turn();
            sim.advance();
            for _ in 0..AUTO_PLAY_STEPS {
                if sim.outcome() != Outcome::Ongoing {
                    break;
                }
                if sim.ai_step().is_none() {
                    sim.skip();
                }
            }
            for team in Team::BOTH {
                self.predicted[team.index()] = sim.side_lost(team);
            }
            // The battle object is restored after it (Hunger's counter keeps what the
            // pre-simulation's removals stored).
            if self.screen_object {
                patch_update(|g| g.screen_living = counts[0] + counts[1]);
            }
        }
        self.start_turn();
        self.advance();
    }

    fn opt(&self) -> &super::content::GlobalOptions {
        &self.content.options
    }

    /// `team`'s list of records: its living units and any `Suicide` unit not removed yet.
    fn living(&self, team: Team) -> impl Iterator<Item = &Fighter> {
        self.fighters.iter().filter(move |f| f.listed() && f.team == team)
    }

    fn living_ids(&self, team: Team) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&i| self.fighters[i].listed() && self.fighters[i].team == team).collect()
    }

    /// The record index (0-based) of fighter `i` in its side's list.
    fn record_index(&self, i: usize) -> usize {
        let team = self.fighters[i].team;
        self.fighters[..i].iter().filter(|f| f.listed() && f.team == team).count()
    }

    /// Fighter `i` stands on a slot marked by a `NoHeal` weapon.
    pub fn crippled(&self, i: usize) -> bool {
        let f = &self.fighters[i];
        self.crippled[f.team.index()].get(self.record_index(i)).copied().unwrap_or(false)
    }

    fn turn_limit(&self) -> u32 {
        self.opt().battle_end_turn.max(1) as u32
    }

    pub fn outcome(&self) -> Outcome {
        let player = self.living(Team::Player).next().is_some();
        let enemy = self.living(Team::Enemy).next().is_some();
        match (player, enemy) {
            (false, _) => Outcome::Defeat,
            (true, false) => Outcome::Victory,
            (true, true) if self.ended.is_some() => Outcome::Victory,
            _ => Outcome::Ongoing,
        }
    }

    /// Why the battle ended, once it has.
    pub fn end_reason(&self) -> Option<EndReason> {
        match self.outcome() {
            Outcome::Ongoing => None,
            _ => Some(self.ended.unwrap_or(EndReason::Wiped)),
        }
    }

    /// Mana `team` gets from the other side's surrender: the sum of the surrendered units'
    /// `Surrender` (units killed before give none).
    pub fn surrender_mana(&self, team: Team) -> i32 {
        self.surrender_mana[team.index()]
    }

    /// Fighter whose turn it is; `None` while deploying or once the battle is over.
    pub fn active(&self) -> Option<usize> {
        if self.deploying || self.outcome() != Outcome::Ongoing {
            return None;
        }
        self.current.filter(|&i| self.fighters[i].alive())
    }

    /// Actions the active fighter has left.
    pub fn actions_left(&self) -> i32 {
        self.active().map_or(0, |i| self.fighters[i].actions)
    }

    /// Current initiative: base with this turn's modifier.
    fn initiative(&self, i: usize) -> i32 {
        self.fighters[i].stats[Stat::Initiative]
    }

    /// The expected order of the fighters still to act this turn, the active one first.
    pub fn queue(&self) -> impl Iterator<Item = usize> + '_ {
        let (t, (side, pos)) = (self.threshold, self.cursor);
        let mut rest: Vec<(i32, usize, usize, usize)> = Vec::new();
        for (s, team) in Team::BOTH.into_iter().enumerate() {
            for (p, i) in self.living_ids(team).into_iter().enumerate() {
                let (ini, f) = (self.initiative(i), &self.fighters[i]);
                if Some(i) == self.active() || f.actions <= 0 || ini <= 0 {
                    continue;
                }
                let this_pass = (s, p) > (side, pos) && ini >= t;
                rest.push((if this_pass { t } else { ini.min(t - 1) }, s, p, i));
            }
        }
        rest.sort_by_key(|&(at, s, p, _)| (-at, s, p));
        self.active().into_iter().chain(rest.into_iter().map(|r| r.3))
    }

    // ------------------------------------------------------------------------------------
    // Turns (4840ec, 489ca0)
    // ------------------------------------------------------------------------------------

    /// Starts the next battle turn (4840ec). Unit by unit in list order, the player's side
    /// first: modifiers, actions, initiative and the reserve move reset, the turn-1 bonuses,
    /// the Community turn-start bonuses, the blessed and cursed flags cleared, then from
    /// turn 2 the magic drain and the regeneration or poison, which can kill the unit (and
    /// collapse its rows) before the next unit's bonuses. No collapse runs otherwise.
    fn start_turn(&mut self) {
        self.round += 1;
        let round = self.round;
        self.threshold = if round == 1 { TURN_ONE_THRESHOLD } else { self.first_threshold };
        self.first_threshold = 0;
        self.cursor = (0, 0);
        self.log.push(crate::trf!("-- Turn {round} --", round));
        for t in 0..2 {
            self.max_turn_lost[t] = self.max_turn_lost[t].max(self.turn_lost[t]);
            self.turn_lost[t] = 0;
        }
        let mut initiative = [0.0f64; 2];
        for i in 0..self.fighters.len() {
            if !self.fighters[i].listed() {
                continue;
            }
            let f = &mut self.fighters[i];
            // The side's mean initiative sums the base initiatives, skipping a term that
            // would leave the sum at 0 or below.
            let sum = &mut initiative[f.team.index()];
            if *sum + f.base[Stat::Initiative] as f64 > 0.0 {
                *sum += f.base[Stat::Initiative] as f64;
            }
            f.mods = Buff::default();
            f.cur_initiative = f.base[Stat::Initiative];
            f.reserve_move = true;
            f.actions = f.base[Stat::Manevres] + i32::from(round == 1 && f.base.has_any(&FAST_START));
            self.turn_bonus(i);
            let f = &mut self.fighters[i];
            f.blessed = false;
            f.cursed = false;
            if round >= 2 {
                self.drain(i);
                self.refresh(i);
                self.regenerate(i);
            }
        }
        if round == 1 && self.rules != Rules::Game {
            self.terrible();
        }
        for team in Team::BOTH {
            let n = self.living(team).count();
            let mean = if n > 0 { initiative[team.index()] / n as f64 } else { initiative[team.index()] };
            self.mean_initiative[team.index()] = if mean == 0.0 { 1.0 } else { mean };
        }
        for i in 0..self.fighters.len() {
            self.refresh(i);
        }
    }

    /// Magic power drain from turn 2 (Community c2851a): `max(MP − drain, floor)`, at least
    /// 0, for units with power; the floor also raises weak casters. Concentration adds the
    /// drain instead.
    fn drain(&mut self, i: usize) {
        if self.fighters[i].power <= 0 {
            return;
        }
        if self.rules != Rules::Game {
            self.editor_drain(i);
            return;
        }
        let (dec, floor) = self.drain_of(self.fighters[i].unit);
        let f = &mut self.fighters[i];
        f.power = if f.has(Bonus::Concentration) { f.power.wrapping_add(dec) } else { f.power.wrapping_sub(dec) };
        f.power = f.power.max(floor).max(0);
    }

    /// The drain and floor of a unit type, as the patch computes them once at load (c283dc):
    /// `ManaDrain` and `MinMagicPower` when not 0, else the school's `DecSpell…` and
    /// `MinSpell…`, the latter +25 for an undead Death type (only on this default path). A
    /// type without magic power of its own has neither, even if a unit of it gains some later;
    /// a type with power but no school drains 0 and gets the floor table's unused slot.
    fn drain_of(&self, unit: UnitId) -> (i32, i32) {
        let def = self.content.unit(unit);
        if def.stat(Stat::MagicPower) == 0 {
            return (0, 0);
        }
        let o = self.opt();
        let dec = def.mana_drain.filter(|&v| v != 0).unwrap_or_else(|| def.magic.map_or(0, |s| o.dec_spell(s)));
        let floor = match def.min_magic_power.filter(|&v| v != 0) {
            Some(v) => v,
            None => match def.magic {
                None => SCHOOLLESS_FLOOR,
                Some(school) => o.min_spell(school) + if school == MagicSchool::Death && def.nature == Nature::Undead { UNDEAD_DEATH_FLOOR } else { 0 },
            },
        };
        (dec, floor)
    }

    /// Regeneration and poison from turn 2: `HP += round(maxHP × regen / 100)`, capped at
    /// max HP; a unit at 0 or less dies (4846c1).
    fn regenerate(&mut self, i: usize) {
        let f = &mut self.fighters[i];
        let max = f.base.max_hp();
        let delta = round_even(max as i64 * f.regen as i64, 100) as i32;
        if delta == 0 || (delta > 0 && f.hp >= max) {
            return;
        }
        let new = (f.hp + delta).min(max);
        let change = new - f.hp;
        f.hp = new;
        if change < 0 {
            let msg = crate::trf!("{name} loses {loss} to poison", name = f.name, loss = -change);
            self.log.push(msg);
            if !self.fighters[i].alive() {
                self.died(i);
            }
        } else {
            let msg = crate::trf!("{name} regenerates +{change}", name = f.name, change);
            self.log.push(msg);
        }
    }

    /// The turn-1 initiative bonuses and the Community turn-start bonuses of unit `i` (the
    /// hook chain in 4840ec), before its own regeneration: Berserk reads the HP the unit has
    /// before this turn's regeneration or poison.
    fn turn_bonus(&mut self, i: usize) {
        let round = self.round as i32;
        let team = self.fighters[i].team;
        let own_building = self.building_defence[team.index()];
        let their_building = self.building_defence[team.other().index()];
        if round == 1 {
            // Cleared on turn 1, once per unit (c29fbb, c2a5ca).
            self.crippled = [[false; RECORDS]; 2];
            for f in &mut self.fighters {
                f.bleed = 0;
            }
        }
        let g = patch_globals();
        let f = &mut self.fighters[i];
        if round == 1 && (f.has(Bonus::Artillery) || f.has(Bonus::FirstShot)) {
            // To the current initiative, not to the modifier (484365, c28935).
            f.cur_initiative += FIRST_TURN_INITIATIVE * if own_building >= 10 { 2 } else { 1 };
        }
        // Hunger: a removal since the last look (anywhere: the counter is global) heals it to
        // full, except on turn 1, where it only looks. One look for all Hunger units, so only
        // the first one processed heals (c25370).
        if f.has(Bonus::Hunger) && g.hunger_counter != g.hunger_seen {
            patch_update(|g| g.hunger_seen = g.hunger_counter);
            if round != 1 {
                f.hp = f.base.max_hp();
            }
        }
        if f.has(Bonus::Berserk) {
            f.mods.attack = berserk(f);
        }
        if round >= 2 && f.has(Bonus::Fortify) {
            f.mods.defence += (f.base[Stat::DefenceBlow] * FORTIFY_PERCENT / 100).max(1) * (round - 1).min(FORTIFY_TURNS);
        }
        // The Community Garrison fix: +AttackShot to the attack modifier.
        if f.has(Bonus::Garrison) && own_building == 10 && self.rules == Rules::Game {
            f.mods.attack += f.base[Stat::AttackShot];
        }
        // Bastion doubles its attacks and defences every turn, with no building check; the
        // 32-bit values wrap after enough turns.
        if f.has(Bonus::Bastion) {
            for st in [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot] {
                f.base[st] = f.base[st].wrapping_mul(2);
            }
        }
        if round <= 2 && f.has(Bonus::FasterAttack) {
            f.actions += 1;
        }
        if round == 1 && f.has(Bonus::Assault) && their_building >= 10 {
            for st in [Stat::AttackBlow, Stat::AttackShot, Stat::DefenceBlow, Stat::DefenceShot] {
                f.base[st] *= 2;
            }
        }
        // Flock compares the living counts of the side blocks of the battle on screen, as of
        // its last action (c29d08): deaths since, a turn start's included, are not seen yet.
        // The step divides unsigned, so a negative attack gives a huge step (the original's).
        if f.has(Bonus::Flock) {
            let (own, other) = (g.side_blocks[team.index()], g.side_blocks[team.other().index()]);
            let of = if f.base[Stat::AttackBlow] != 0 { f.base[Stat::AttackBlow] } else { f.base[Stat::AttackShot] };
            let step = (of.wrapping_mul(FLOCK_PERCENT) as u32 / 100) as i32;
            match own.cmp(&other) {
                std::cmp::Ordering::Greater => f.mods.attack = f.mods.attack.wrapping_add(step),
                std::cmp::Ordering::Less => f.mods.attack = f.mods.attack.wrapping_sub(step),
                std::cmp::Ordering::Equal => {}
            }
        }
    }

    /// Picks the next actor with the threshold scan (489ca0), starting new turns as needed.
    fn advance(&mut self) {
        self.current = None;
        if self.outcome() != Outcome::Ongoing {
            return;
        }
        let mut lists = [self.living_ids(Team::Player), self.living_ids(Team::Enemy)];
        loop {
            if self.cursor == (0, 0) && self.threshold > 0 {
                // A pass that finds nobody only lowers the threshold: jump over those.
                let top = lists.iter().flatten().filter(|&&i| self.fighters[i].actions > 0).map(|&i| self.initiative(i)).max().unwrap_or(0);
                self.threshold = self.threshold.min(top.max(0));
            }
            if self.threshold <= 0 {
                if self.round >= self.turn_limit() {
                    // Nobody can act any more: the limit ends it.
                    self.ended = Some(EndReason::TurnLimit);
                    return;
                }
                self.start_turn();
                if self.outcome() != Outcome::Ongoing {
                    return;
                }
                lists = [self.living_ids(Team::Player), self.living_ids(Team::Enemy)];
                continue;
            }
            let (side, pos) = self.cursor;
            let list = &lists[side];
            if let Some(&i) = list.get(pos) {
                if self.initiative(i) >= self.threshold && self.fighters[i].actions > 0 {
                    if self.first_threshold == 0 {
                        self.first_threshold = self.threshold;
                    }
                    self.current = Some(i);
                    return;
                }
            }
            if pos + 1 < list.len() {
                self.cursor.1 += 1;
            } else if side == 0 {
                self.cursor = (1, 0);
            } else {
                self.cursor = (0, 0);
                self.threshold -= 1;
            }
        }
    }

    /// Recomputes current stats from base, drain and this turn's modifiers.
    fn refresh(&mut self, i: usize) {
        let started = self.round > 0;
        let f = &mut self.fighters[i];
        let mut s = f.base.clone();
        s[Stat::MagicPower] = f.power;
        s[Stat::Regen] = f.regen;
        for st in [Stat::AttackBlow, Stat::AttackShot] {
            if s[st] > 0 {
                s[st] = s[st].wrapping_add(f.mods.attack);
            }
        }
        s[Stat::DefenceBlow] = s[Stat::DefenceBlow].wrapping_add(f.mods.defence);
        s[Stat::DefenceShot] = s[Stat::DefenceShot].wrapping_add(f.mods.defence);
        // Before the first turn start the current initiative is not set yet: the base shows.
        let current = if started { f.cur_initiative } else { s[Stat::Initiative] };
        s[Stat::Initiative] = current.wrapping_add(f.mods.initiative);
        s.clamp();
        s[Stat::Regen] = f.regen;
        f.stats = s;
    }

    fn row_occupied(&self, team: Team, row: Row) -> bool {
        self.living(team).any(|f| f.slot.row == row)
    }

    /// Row collapse (48a170): with rows 1 and 2 empty the reserve moves to row 1 (same
    /// column) and loses its remaining actions; with only row 1 empty row 2 moves up and
    /// keeps them.
    fn collapse(&mut self, team: Team) {
        if self.row_occupied(team, Row::Front) || !self.switches().collapse {
            return;
        }
        let back = self.row_occupied(team, Row::Back);
        let from = if back { Row::Back } else { Row::Reserve };
        if !back && !self.row_occupied(team, from) {
            return;
        }
        // The whole grid row is copied, its blocked cells too, and the row left behind is
        // all open: the wide row's blocks move forward (48a170).
        let side = &mut self.cells[team.index()];
        let r = (from.number() - 1) as usize;
        side[0] = side[r];
        side[r] = [true; 6];
        let mut moved = false;
        for f in self.fighters.iter_mut().filter(|f| f.listed() && f.team == team && f.slot.row == from) {
            f.slot.row = Row::Front;
            if from == Row::Reserve {
                f.actions = 0;
            }
            moved = true;
        }
        if moved {
            let msg = match (team == Team::Player, back) {
                (true, true) => tr("Your rear steps forward"),
                (true, false) => tr("Your reserve steps forward"),
                (false, true) => tr("The enemy rear steps forward"),
                (false, false) => tr("The enemy reserve steps forward"),
            };
            self.log.push(msg.to_string());
        }
    }

    /// Unit `i` is removed (489f50): it leaves its side's list, Hunger's counter takes the
    /// living count of the battle on screen, and its side's rows may collapse. The bleed
    /// values move with the records (c2a95a), but the shift always copies one value: when
    /// the player's 12th record goes, the enemy's first unit stops bleeding (the original's).
    fn died(&mut self, i: usize) {
        let team = self.fighters[i].team;
        let index = self.record_index(i);
        self.fighters[i].suicided = false;
        if team == Team::Player && index == RECORDS - 1 {
            if let Some(&e) = self.living_ids(Team::Enemy).first() {
                self.fighters[e].bleed = 0;
            }
        }
        let living = self.fighters.iter().filter(|f| f.listed()).count();
        let own = self.screen_object;
        if self.rules == Rules::Game {
            patch_update(|g| {
                if own {
                    g.screen_living = living;
                }
                g.hunger_counter = g.screen_living;
            });
        }
        self.collapse(team);
    }

    /// The end check after every action (48b67b): a side gone, the turn limit, or a side
    /// whose every unit has `Surrender > 0`, which then gives up.
    fn end_check(&mut self) {
        let standing = Team::BOTH.map(|t| self.living(t).next().is_some());
        let limit = self.round >= self.turn_limit();
        // Each side that still has units is tested on its own, whether the other side is
        // gone or not (48b6ba): a player who wins with only surrender-capable units left
        // surrenders all the same, and that is a defeat. The original's, kept.
        let giving_up: Vec<Team> = Team::BOTH
            .into_iter()
            .filter(|&t| standing[t.index()] && self.living(t).all(|f| f.surrender > 0))
            .collect();
        for &team in &giving_up {
            let mut mana = 0;
            for f in self.fighters.iter_mut().filter(|f| f.listed() && f.team == team) {
                mana += f.surrender;
                f.surrendered = true;
                f.hp = 0;
            }
            self.surrender_mana[team.other().index()] += mana;
            let msg = if team == Team::Player { tr("Your army surrenders") } else { tr("The enemy surrenders") };
            self.log.push(msg.to_string());
        }
        if !standing.iter().all(|&s| s) {
            // A side wiped out: the battle is over anyway (a beaten player stays beaten).
            if giving_up.contains(&Team::Player) {
                self.ended = Some(EndReason::Surrender(Team::Player));
            }
        } else if let Some(&team) = giving_up.first() {
            self.ended = Some(EndReason::Surrender(team));
        } else if limit {
            self.ended = Some(EndReason::TurnLimit);
            self.log.push(crate::trf!("Turn {round} ends the battle", round = self.round));
        }
    }

    // ------------------------------------------------------------------------------------
    // Reach (484c4c)
    // ------------------------------------------------------------------------------------

    /// Occupied front cells of `team` in columns c−1..c+1.
    fn front_near(&self, team: Team, col: u8) -> Vec<u8> {
        self.living(team).filter(|f| f.slot.row == Row::Front && f.slot.col.abs_diff(col) <= 1).map(|f| f.slot.col).collect()
    }

    /// Long-strike targets from column `col`: the nearest front unit of `team` to the right
    /// (c+2, c+3 …) and to the left (c−2, c−3 …).
    fn long_strike_targets(&self, team: Team, col: u8) -> Vec<usize> {
        let front = |c: i32| (0..self.formation.cols as i32).contains(&c).then(|| self.at(team, Slot::new(Row::Front, c as u8))).flatten();
        let c = col as i32;
        let right = (c + 2..self.formation.cols as i32).find_map(front);
        let left = (0..=c - 2).rev().find_map(front);
        [right, left].into_iter().flatten().collect()
    }

    /// The enemy front row is "clear" opposite `col` (484ac8): the cells c−1, c and c+1 all
    /// exist and are empty; a blocked cell is not empty. So a unit in the first or last
    /// column never has a clear front.
    fn front_clear(&self, team: Team, col: u8) -> bool {
        col > 0
            && col + 1 < self.formation.cols
            && self.front_near(team, col).is_empty()
            && (col - 1..=col + 1).all(|c| self.is_open(team, Slot::new(Row::Front, c)))
    }

    /// The cell exists on `team`'s grid (it is not blocked).
    pub fn is_open(&self, team: Team, s: Slot) -> bool {
        // The editor's cell test is the grid's shape (0x4f752c), on top of the blocked cells.
        let shaped = self.rules == Rules::Game || self.formation.contains(s);
        shaped && s.col < self.formation.cols && self.cells[team.index()][(s.row.number() - 1) as usize][s.col as usize]
    }

    /// The cells of `team`'s grid that exist, row by row.
    fn grid(&self, team: Team) -> impl Iterator<Item = Slot> + '_ {
        let cols = self.formation.cols;
        [Row::Front, Row::Back, Row::Reserve]
            .into_iter()
            .flat_map(move |row| (0..cols).map(move |col| Slot::new(row, col)))
            .filter(move |&s| self.is_open(team, s))
    }

    /// What `id`, standing on `from`, would do to `target`: one action per cell, as in the
    /// original's cell map. Where several fit, later ones win as there: melee, then a shot,
    /// then hostile magic, then Flying's melee (c29150), then a Ghost's cast (48555b).
    fn option_at(&self, id: usize, from: Slot, target: usize) -> Option<ActionKind> {
        use ActionKind::*;
        let (f, t) = (&self.fighters[id], &self.fighters[target]);
        // A Suicide unit waiting for its removal is neither an actor nor a target (guess: the
        // original's grid cell still holds it, community-patches.md, Unknowns).
        if !f.standing() || !t.standing() {
            return None;
        }
        // The editor's OldVampiressGist cannot be targeted on turn 1 while it still has
        // actions: its cell keeps only the codes 0–3 (0x4f7c98).
        if self.round == 1 && t.extra == Some(EditorBonus::OldVampiressGist) && t.actions > 0 {
            return None;
        }
        let s = &f.stats;
        let sw = self.switches();
        if t.team != f.team {
            if !t.slot.row.is_active() {
                return None;
            }
            let near = self.front_near(t.team, from.col);
            let adjacent = t.slot.row == Row::Front && near.contains(&t.slot.col);
            let clear = self.front_clear(t.team, from.col);
            let magic = if t.weakened() { Strike } else { Curse };
            let mut kind = None;
            if f.is_warrior() && from.row == Row::Front {
                if adjacent {
                    kind = Some(Melee);
                } else if sw.long_strike && near.is_empty() && self.long_strike_targets(t.team, from.col).contains(&target) {
                    kind = Some(LongStrike);
                }
            }
            // The editor's short-range switch: the whole enemy rows 1–2 become their cells in
            // columns c−1..c+1, for shooters and casters of both rows (0x4f84f2 …).
            let whole = !sw.short_range || t.slot.col.abs_diff(from.col) <= 1;
            // From the front row a shooter reaches everyone only past a clear front, else just
            // the occupied cells c−1..c+1; a mage there casts only past a clear front.
            if f.is_shooter() && (((from.row == Row::Back || (from.row == Row::Front && clear)) && whole) || (from.row == Row::Front && !clear && adjacent)) {
                kind = Some(Shot);
            }
            if s.is_mage() && s.magic_direction().hits_enemies() && (from.row == Row::Back || (from.row == Row::Front && clear)) && whole {
                kind = Some(magic);
            }
            // Flying writes melee on the three front cells opposite after the shots and spells,
            // so a flying shooter or mage strikes there instead (its test of the attack type
            // is always true, c29150).
            if f.has(Bonus::Flying) && from.row.is_active() && adjacent {
                kind = Some(Melee);
            }
            // A Ghost casts at the three front cells opposite from any row, whatever its
            // direction. Its power test reads only the low byte of the magic power, as a
            // signed byte, as the original does (48555b): 128..255 fails it.
            if f.has(Bonus::Ghost) && (f.power as u8 as i8) > 0 && adjacent {
                kind = Some(magic);
            }
            kind
        } else {
            if !(s.is_mage() && s.magic_direction().helps_allies()) {
                return None;
            }
            if from.row == Row::Reserve {
                // A caster in the reserve tends the reserve, and nothing else.
                if t.slot.row != Row::Reserve {
                    return None;
                }
                // Its own cell is a self-cast only while it is wounded or unblessed, else a
                // pass (48555b end); the other reserve units have no such test.
                if target == id && t.blessed && !t.wounded() {
                    return None;
                }
            } else {
                // NoHeal's mark is tested only here: a reserve caster still tends a marked
                // reserve unit (c2967a).
                if !t.slot.row.is_active() || (t.blessed && !t.wounded()) || self.crippled(target) {
                    return None;
                }
                if self.school(id) == Some(MagicSchool::Elemental) && t.base.magic == Some(MagicSchool::Elemental) && !t.wounded() {
                    return None;
                }
            }
            Some(if t.wounded() && self.heal_amount(id, target, f.power) > 0 { Heal } else { Bless })
        }
    }

    /// What the fighter `id` can do to `target` from where it stands (at most one action).
    pub fn options(&self, id: usize, target: usize) -> Vec<ActionKind> {
        self.option_at(id, self.fighters[id].slot, target).into_iter().collect()
    }

    pub fn can_target(&self, id: usize, target: usize) -> bool {
        !self.options(id, target).is_empty()
    }

    pub fn targets(&self, id: usize) -> Vec<usize> {
        (0..self.fighters.len()).filter(|&t| self.can_target(id, t)).collect()
    }

    fn all_options(&self, id: usize) -> Vec<(usize, ActionKind)> {
        (0..self.fighters.len()).filter_map(|t| self.option_at(id, self.fighters[id].slot, t).map(|k| (t, k))).collect()
    }

    /// A unit that could attack but cannot from where it stands (e.g. a warrior in the back
    /// row).
    pub fn helpless(&self, id: usize) -> bool {
        let f = &self.fighters[id];
        let s = &f.stats;
        let could = f.is_warrior() || f.is_shooter() || (s.is_mage() && s.magic_direction().hits_enemies());
        could && !self.all_options(id).iter().any(|o| o.1.is_hostile())
    }

    /// Empty own cells the fighter could step to (484c4c): from row 1 or 2, columns c−1..c+1
    /// of rows 1 and 2; any reserve cell while it may still use the reserve this turn. From
    /// the reserve (with the same permission): any cell of rows 1 and 2. No swaps.
    pub fn moves(&self, id: usize) -> Vec<Slot> {
        let f = &self.fighters[id];
        if !f.standing() {
            return Vec::new();
        }
        let from = f.slot;
        self.grid(f.team)
            .filter(|&s| s != from && self.at(f.team, s).is_none())
            .filter(|s| match (from.row, s.row) {
                (Row::Reserve, Row::Reserve) => false,
                (Row::Reserve, _) | (_, Row::Reserve) => f.reserve_move,
                _ => s.col.abs_diff(from.col) <= 1,
            })
            .collect()
    }

    // ------------------------------------------------------------------------------------
    // Damage and magic (485908, 485b3c)
    // ------------------------------------------------------------------------------------

    fn has_knight(&self, team: Team) -> bool {
        self.knight[team.index()]
    }

    /// The Splash factor on `a`'s attack for a physical hit of `kind` (c270ae, c2731a,
    /// c27337), ungated, so in every battle and in the AI's estimates: ×40% for any attacker
    /// while a splash of that kind is under way (a melee splash for blows, any splash for
    /// shots), else ×80% for a `Splash` unit.
    fn attack_factor(&self, a: usize, kind: ActionKind) -> Option<u64> {
        let state = self.splash.state;
        let follow_up = if kind == ActionKind::Shot { state != 0 } else { state == SPLASH_MELEE };
        if follow_up {
            Some(SPLASH_SIDE)
        } else {
            self.fighters[a].has(Bonus::Splash).then_some(SPLASH_MAIN)
        }
    }

    /// Physical damage of `a` on `t` (before capping at the target's HP), 485908: the same
    /// routine for blows, shots, counter blows, preventive strikes and the AI's estimates.
    pub fn physical_damage(&self, a: usize, t: usize, kind: ActionKind) -> i32 {
        let (af, tf) = (&self.fighters[a], &self.fighters[t]);
        let (s, ts) = (&af.stats, &tf.stats);
        let shot = kind == ActionKind::Shot;
        let building = self.building_defence[tf.team.index()];
        // The attack modifier is added even to an attack of 0 (a counter blow of a unit
        // without one, a flying shooter's blow).
        let mut atk = (if shot { af.base[Stat::AttackShot] } else { af.base[Stat::AttackBlow] }).wrapping_add(af.mods.attack);
        if let Some(factor) = self.attack_factor(a, kind) {
            atk = splash_scale(atk, factor);
        }
        let mut def = if shot { ts[Stat::DefenceShot] } else { ts[Stat::DefenceBlow] };
        if shot {
            if s.has_any(&PIERCE_SHOT) {
                def = 0;
            }
            if tf.slot.row == Row::Back {
                def += self.opt().row2_def;
            }
        } else {
            if self.round == 1 && ts.has(&Bonus::SpearDefense) {
                def *= 3;
            }
            if s.has_any(&PIERCE_MELEE) {
                def = 0;
            }
            if kind == ActionKind::LongStrike {
                def /= 2;
                if s.has(&Bonus::FlankStrike) {
                    atk = atk.wrapping_mul(2);
                }
            }
        }
        def += building;
        let mut dmg = if atk > def { atk.wrapping_sub(def) } else { 1 };
        // Assault's ×2/3 tests a misaligned dword of the attacker's record (c2a403): its top
        // byte is the attacker's building defence, the three below are the top three bytes of
        // its initiative modifier, and the test is "≥ 16". So it applies from a building of
        // 1–127, or in the open with a modifier below 0 or from 4096 (the original's).
        let probe = (((self.building_defence[af.team.index()] as u8 as u32) << 24) | (af.mods.initiative as u32 >> 8)) as i32;
        let assaulted = ts.has(&Bonus::Assault) && probe >= 16;
        if ts.has_any(&[Bonus::Evasive, Bonus::VampirsGist, Bonus::OldVampirsGist]) || assaulted || tf.extra == Some(EditorBonus::OldVampiressGist) {
            dmg = dmg.wrapping_mul(2) / 3;
        }
        if ts.has(&Bonus::Garrison) && building >= 10 {
            dmg = dmg.wrapping_mul(2) / 3;
        }
        if shot && ts.has_any(&[Bonus::Dead, Bonus::FastDead]) {
            dmg = dmg.wrapping_mul(3) / 10;
        }
        if self.has_knight(tf.team) {
            let percent = if self.rules == Rules::Game { KNIGHT_PERCENT } else { EDITOR_KNIGHT_PERCENT };
            dmg = dmg.wrapping_mul(percent) / 100;
        }
        // The invulnerable (and ghosts, immune to weapons) are hit for 1, whatever the blow
        // pierces; GodAnger and GodStrike still add their 10 or 20 on top (485a8e).
        if ts.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]) {
            dmg = 1;
        }
        dmg = dmg.wrapping_add(god_bonus(s));
        // The editor's Parrying: a guarded unit takes 1 from anything but a shot (0x4f9428).
        if !shot && tf.extra == Some(EditorBonus::Parrying) && tf.guard {
            dmg = 1;
        }
        if dmg == 0 {
            dmg = 1;
        }
        // Evasion, last (c2a802): the type's value is a byte, so the ini's modulo 256, and it
        // divides unsigned, so a value above 100 gives garbage (the original's).
        let evasion = ts.evasion as u8 as i32;
        if evasion != 0 {
            dmg = (dmg.wrapping_mul(100 - evasion) as u32 / 100) as i32;
            if dmg == 0 {
                dmg = 1;
            }
        }
        dmg
    }

    /// The caster's school. A caster with power but no school (a Ghost can be one) is not
    /// reduced by protection, has no nature table, heals by its whole power, and its
    /// blessings and curses only set the flag (485b3c, 48a87c, 48ac7e).
    fn school(&self, a: usize) -> Option<MagicSchool> {
        self.fighters[a].stats.magic
    }

    /// Caster power `p` against `t` for hostile magic: reduced by the target's protection %,
    /// except for a Community `Potent` caster.
    fn hostile_power_of(&self, a: usize, t: usize, p: i32) -> i32 {
        if self.fighters[a].has(Bonus::Potent) {
            return p;
        }
        let Some(school) = self.school(a) else { return p };
        let prot = self.fighters[t].stats.protection(school).clamp(0, 100);
        // `Round(P × (1 − prot/100))` on the FPU: Delphi's Round, half to even (485b84). The
        // FPU precision is unknown (engine.md), so the product is taken as exact.
        round_even(p as i64 * (100 - prot) as i64, 100) as i32
    }

    /// The power `a` casts with (485b3c, c27374), for every kind of spell: ×40% for any caster
    /// while a splash is under way, else ×80% for a `Splash` caster.
    fn cast_power(&self, a: usize) -> i32 {
        let p = self.fighters[a].power;
        if self.splash.state != 0 {
            splash_scale(p, SPLASH_SIDE)
        } else if self.fighters[a].has(Bonus::Splash) {
            splash_scale(p, SPLASH_MAIN)
        } else {
            p
        }
    }

    /// The "power after protection" both mage poisons test against 15 (c26c9f): the caster's
    /// magic power, without Splash or Potent, × (99 − the target's protection) / 100, or / 114
    /// against Elemental protection. Life (and a caster without a school) and Elemental
    /// divide unsigned, so a protection above 99 makes it huge and the poison works; Death
    /// divides signed (the original's).
    fn poison_power(&self, a: usize, t: usize) -> i32 {
        let mp = self.fighters[a].power;
        let ts = &self.fighters[t].stats;
        match self.school(a) {
            Some(MagicSchool::Death) => mp.wrapping_mul(99 - ts[Stat::ProtectDeath]) / 100,
            Some(MagicSchool::Elemental) => (mp.wrapping_mul(99 - ts[Stat::ProtectElemental]) as u32 / 114) as i32,
            _ => (mp.wrapping_mul(99 - ts[Stat::ProtectLife]) as u32 / 100) as i32,
        }
    }

    /// Magic strike damage of hostile power `p`, before capping at HP: Life ×2 on undead,
    /// Death ×½ on undead, Elemental ¾; ¾ on elementals for Life and Death (not for a
    /// `Potent` caster); plus GodAnger/GodStrike.
    fn strike_damage(&self, a: usize, t: usize, p: i32) -> i32 {
        let nature = self.fighters[t].stats.nature;
        let editor = self.rules != Rules::Game;
        let dmg = match (self.school(a), nature) {
            _ if self.fighters[a].has(Bonus::Potent) => p,
            // The editor's table (0x4f9688): no ×3/4 on elementals for Life and Death.
            (Some(MagicSchool::Life), Nature::Undead) if editor => 2 * p,
            (Some(MagicSchool::Death), Nature::Undead) if editor => p / 2,
            (Some(MagicSchool::Elemental), _) if editor => p * 3 / 4,
            _ if editor => p,
            (None, _) => p,
            (Some(MagicSchool::Life), Nature::Undead) => 2 * p,
            (Some(MagicSchool::Death), Nature::Undead) => p / 2,
            (Some(MagicSchool::Elemental), _) | (_, Nature::Elemental) => p * 3 / 4,
            _ => p,
        };
        // GodAnger and GodStrike are added whenever the caster has magic power, even to a
        // strike whose power the protection or the nature took to 0 (485b3c).
        if self.fighters[a].power > 0 {
            dmg + god_bonus(&self.fighters[a].stats)
        } else {
            dmg
        }
    }

    /// A magic strike's damage on `t` as it would be cast now, `Drying` included.
    pub fn magic_strike(&self, a: usize, t: usize) -> i32 {
        let p = self.hostile_power_of(a, t, self.cast_power(a));
        self.strike_damage(a, t, p) + self.drying(a, t)
    }

    /// Community `Drying`: 8% of the target's max HP on every hostile spell, ignoring
    /// protection (at least 1).
    fn drying(&self, a: usize, t: usize) -> i32 {
        if self.fighters[a].has(Bonus::Drying) {
            (self.fighters[t].max_hp() * DRYING_PERCENT / 100).max(1)
        } else {
            0
        }
    }

    /// HP a heal of power `p` restores before capping: Life P (not on undead or elementals),
    /// Elemental P/2, Death P on undead only.
    fn heal_amount(&self, a: usize, t: usize, p: i32) -> i32 {
        let nature = self.fighters[t].stats.nature;
        if self.rules != Rules::Game {
            // The editor's (0x4f9688): Life 0 only on undead, Death 0 on Normal and Hero.
            return match self.school(a) {
                Some(MagicSchool::Life) if nature == Nature::Undead => 0,
                Some(MagicSchool::Life) | None => p,
                Some(MagicSchool::Elemental) => p / 2,
                Some(MagicSchool::Death) if vampire_nature(nature) => 0,
                Some(MagicSchool::Death) => p,
            };
        }
        match self.school(a) {
            Some(MagicSchool::Life) if matches!(nature, Nature::Undead | Nature::Elemental) => 0,
            Some(MagicSchool::Life) | None => p,
            Some(MagicSchool::Elemental) => p / 2,
            Some(MagicSchool::Death) if nature == Nature::Undead => p,
            Some(MagicSchool::Death) => 0,
        }
    }

    /// Blessing of power `p` by school (friendly: power not reduced). The attack modifier is
    /// given to a unit without an attack too: it counts in its counter blows.
    fn bless_of(&self, a: usize, t: usize, p: i32) -> Buff {
        let target = &self.fighters[t];
        match self.school(a) {
            // The editor's Life blessing changes only Normal and Hero units (0x501054).
            Some(MagicSchool::Life) if self.rules != Rules::Game && !vampire_nature(target.stats.nature) => Buff::default(),
            Some(MagicSchool::Life) if matches!(target.stats.nature, Nature::Undead | Nature::Elemental) => Buff::default(),
            Some(school) => bless_effect(self.opt(), school, p),
            None => Buff::default(),
        }
    }

    /// Curse of hostile power `p` by school.
    fn curse_of(&self, a: usize, p: i32) -> Buff {
        self.school(a).map_or(Buff::default(), |school| curse_effect(self.opt(), school, p))
    }

    /// The blessing `a` would give `t` now, as the hover shows it: no attack gain for a
    /// unit without an attack (485d58).
    pub fn bless_buff(&self, a: usize, t: usize) -> Buff {
        let mut b = self.bless_of(a, t, self.cast_power(a));
        if !self.fighters[t].has_attack() {
            b.attack = 0;
        }
        b
    }

    /// The curse `a` would put on `t` now, as the hover shows it.
    pub fn curse_buff(&self, a: usize, t: usize) -> Buff {
        let mut b = self.curse_of(a, self.hostile_power_of(a, t, self.cast_power(a)));
        if !self.fighters[t].has_attack() {
            b.attack = 0;
        }
        b
    }

    /// Expected effect of `kind` by `a` on `t`, for hover previews.
    pub fn preview(&self, a: usize, t: usize, kind: ActionKind) -> Preview {
        let target = &self.fighters[t];
        match kind {
            ActionKind::Strike => Preview::Damage(self.magic_strike(a, t).min(target.hp)),
            ActionKind::Curse => Preview::Buff(self.curse_buff(a, t)),
            ActionKind::Heal => Preview::Heal(self.heal_amount(a, t, self.cast_power(a)).min(target.max_hp() - target.hp)),
            ActionKind::Bless => Preview::Buff(self.bless_buff(a, t)),
            k => Preview::Damage(self.physical_damage(a, t, k).min(target.hp)),
        }
    }

    // ------------------------------------------------------------------------------------
    // Actions (48a5c4)
    // ------------------------------------------------------------------------------------

    /// Every action starts by spending one action; a bleeding unit then bleeds, and dies
    /// before acting if that kills it. False if the actor died.
    fn start_action(&mut self, id: usize) -> bool {
        let cost = self.switches().initiative_cost;
        let f = &mut self.fighters[id];
        f.actions -= 1;
        f.taken += 1;
        // The editor's: every action drops the Parrying guard, and with the initiative-cost
        // switch also costs 1 current initiative (0x501054).
        f.guard = false;
        if cost {
            f.cur_initiative -= 1;
            self.refresh(id);
        }
        let f = &mut self.fighters[id];
        if f.bleed > 0 {
            // `(AB + AS + MP) × bleed / 100`, divided unsigned: a negative sum (EternalGift
            // curses) bleeds about 43 million, so the unit dies (c2a53c, the original's).
            let sum = f.base[Stat::AttackBlow].wrapping_add(f.base[Stat::AttackShot]).wrapping_add(f.power);
            let loss = (sum.wrapping_mul(f.bleed) as u32 / 100) as i32;
            let loss = loss.min(f.hp);
            if loss > 0 {
                f.hp -= loss;
                let msg = crate::trf!("{name} bleeds for {loss}", name = f.name, loss);
                self.log.push(msg);
                if !self.fighters[id].alive() {
                    // Removed plainly (no on-kill effects), and its bleed is cleared.
                    self.fighters[id].bleed = 0;
                    self.died(id);
                    return false;
                }
            }
        }
        true
    }

    /// After every action: the actor's side collapses once it has used its last action
    /// (48b5ac), the end check runs, the battle on screen copies its sides' living counts into
    /// the side blocks (48bb10, which Flock reads) and the next actor is picked.
    fn finish_action(&mut self, id: usize) {
        if self.fighters[id].alive() && self.fighters[id].actions <= 0 {
            let team = self.fighters[id].team;
            self.collapse(team);
        }
        self.end_check();
        if self.interactive {
            let counts = Team::BOTH.map(|t| self.living(t).count());
            patch_update(|g| g.side_blocks = counts);
        }
        self.advance();
    }

    /// Step the active fighter to an empty own cell; costs one action. Stepping into or out
    /// of the reserve uses up that unit's reserve move for the turn.
    pub fn move_active(&mut self, to: Slot) -> Result<(), ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.moves(id).contains(&to) {
            return Err(ActionError::InvalidTarget);
        }
        if self.start_action(id) {
            let f = &mut self.fighters[id];
            if (f.slot.row == Row::Reserve) != (to.row == Row::Reserve) {
                f.reserve_move = false;
            }
            f.slot = to;
            let msg = crate::trf!("{name} moves", name = f.name);
            self.log.push(msg);
        }
        self.finish_action(id);
        Ok(())
    }

    /// The active fighter passes one action (a click on its own cell).
    pub fn pass(&mut self) {
        if let Some(id) = self.active() {
            if self.start_action(id) && self.fighters[id].extra == Some(EditorBonus::Parrying) {
                // The editor's Parrying: a pass raises its guard (0x5012e7).
                self.fighters[id].guard = true;
            }
            self.finish_action(id);
        }
    }

    /// The original's own-cell action, of a click on the active unit's own card or the space
    /// key (4c4f8c): a self-cast when its cell offers one, else a pass. One action either way.
    pub fn own_cell(&mut self) -> Option<Hit> {
        let id = self.active()?;
        match self.options(id, id).first() {
            Some(&kind) => self.act_with(id, kind).ok(),
            None => {
                self.pass();
                None
            }
        }
    }

    /// The active fighter passes all its remaining actions (Razdor's, for the quick battle
    /// and tests; the original has no such key).
    pub fn skip(&mut self) {
        if let Some(id) = self.active() {
            self.log.push(crate::trf!("{name} waits", name = self.fighters[id].name));
            for _ in 0..self.fighters[id].actions {
                if self.active() != Some(id) {
                    break;
                }
                self.pass();
            }
        }
    }

    /// The active fighter's action on `target`; costs one action.
    pub fn act(&mut self, target: usize) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        let kind = *self.options(id, target).first().ok_or(ActionError::InvalidTarget)?;
        self.act_with(target, kind)
    }

    /// The active fighter does `kind` to `target` (it must be the cell's action); costs one
    /// action. The case of that kind runs (48a5c4), then the Splash neighbour loop.
    pub fn act_with(&mut self, target: usize, kind: ActionKind) -> Result<Hit, ActionError> {
        let id = self.active().ok_or(ActionError::NotYourTurn)?;
        if !self.options(id, target).contains(&kind) {
            return Err(ActionError::InvalidTarget);
        }
        let mut hit = Hit::new(target, kind);
        if self.start_action(id) {
            self.fighters[id].useful += 1;
            let s = self.run_case(id, target, kind, &mut hit.counter);
            (hit.kind, hit.amount, hit.buff, hit.killed) = (s.kind, s.amount, s.buff, s.killed);
            self.splash_follow_ups(id, kind, &mut hit);
        }
        hit.actor_died = !self.fighters[id].alive();
        self.finish_action(id);
        Ok(hit)
    }

    /// One run of an action's case on `target`. Hostile magic decides between a strike and a
    /// curse again, and friendly magic between a heal and a blessing, at each run.
    fn run_case(&mut self, id: usize, target: usize, kind: ActionKind, counter: &mut Option<i32>) -> Struck {
        match kind {
            ActionKind::Melee | ActionKind::LongStrike => self.melee_case(id, target, kind, counter),
            ActionKind::Shot => self.shot_case(id, target, counter),
            ActionKind::Strike | ActionKind::Curse => self.magic_case(id, target),
            ActionKind::Heal | ActionKind::Bless => self.friendly_case(id, target),
        }
    }

    /// The Splash neighbour loop at the end of a hit action (c26fd2). While a splash is
    /// recorded, the primary target's side is scanned in record order from the first record
    /// each time, for a unit in the saved row (for melee also within one column of the
    /// attacker) that is the left neighbour (column − 1, not taken yet) or else the right one
    /// (+ 1): so the two are hit in record order, each once. The chosen one gets the whole
    /// case again, with no action spent: its preventive strike, the full chain, its counter
    /// blow and the kill check. The second follow-up's recording ends the splash, so its own
    /// counter blow is at full strength.
    fn splash_follow_ups(&mut self, id: usize, kind: ActionKind, hit: &mut Hit) {
        while self.splash.state != 0 {
            // The original goes on with the slot index of a removed attacker, now another
            // unit's (unknown); Razdor ends the splash.
            let Some((team, row)) = self.splash.line.filter(|_| self.fighters[id].listed()) else { break };
            let (c, melee) = (self.splash.col, self.splash.state == SPLASH_MELEE);
            let mut next = None;
            for n in self.living_ids(team) {
                let f = &self.fighters[n];
                let col = f.slot.col as i32;
                if !f.standing() || f.slot.row != row || (melee && (self.splash.actor_col - col).abs() >= 2) {
                    continue;
                }
                if !self.splash.left && col == c - 1 {
                    self.splash.left = true;
                } else if !self.splash.right && col == c + 1 {
                    self.splash.right = true;
                } else {
                    continue;
                }
                next = Some(n);
                break;
            }
            let Some(n) = next else { break };
            let again = match self.splash.state {
                SPLASH_MELEE => kind,
                SPLASH_SHOT => ActionKind::Shot,
                SPLASH_MAGIC => ActionKind::Curse,
                _ => ActionKind::Heal,
            };
            let s = self.run_case(id, n, again, &mut hit.counter);
            hit.splash.push((n, s.amount));
        }
        self.splash = Splash::default();
    }

    /// A Splash unit's hit records itself (c26f2d, c270e5, c2718d, c27274): the first one of
    /// its kind saves the target's row and column and sets the state; a follow-up counts, and
    /// the second one ends the splash. Melee, shots and hostile spells record only in the
    /// battle on screen; heals and blessings record in any battle (their hook has no gate).
    fn splash_record(&mut self, id: usize, target: usize, state: u8) {
        if !self.fighters[id].has(Bonus::Splash) || (state != SPLASH_FRIENDLY && !self.interactive) {
            return;
        }
        let t = &self.fighters[target];
        let sp = &mut self.splash;
        if sp.state != state {
            sp.line = Some((t.team, t.slot.row));
            sp.col = t.slot.col as i32;
            // An add, from 0 (c26f84).
            sp.state += state;
        } else {
            sp.count += 1;
            if sp.count == 2 {
                (sp.state, sp.count, sp.left, sp.right) = (0, 0, false, false);
            }
        }
    }

    /// Community `PreventiveStrike` (c2a181, c28aea): before a melee on it, it strikes first
    /// (a blow with an AttackBlow, else a shot); before a shot, it shoots first if it has an
    /// AttackShot. Never before a spell: that copy (c28c00) has no caller. The full damage
    /// formula with it as the attacker (Splash scaling included), no side effects, no limit
    /// per turn. False if the attacker died: removed with no on-kill effects (its action is
    /// over, the neighbour loop still runs).
    fn preventive_strike(&mut self, id: usize, target: usize, kind: ActionKind, counter: &mut Option<i32>) -> bool {
        let t = &self.fighters[target];
        if !t.alive() || !t.has(Bonus::PreventiveStrike) {
            return true;
        }
        let answer = if kind.is_melee() {
            Some(if t.base[Stat::AttackBlow] != 0 { ActionKind::Melee } else { ActionKind::Shot })
        } else {
            (t.base[Stat::AttackShot] != 0).then_some(ActionKind::Shot)
        };
        let Some(answer) = answer else { return true };
        let dmg = self.physical_damage(target, id, answer).min(self.fighters[id].hp.max(0));
        self.fighters[id].hp -= dmg;
        *counter = Some(counter.unwrap_or(0) + dmg);
        self.log.push(crate::trf!("{name} strikes first for {dmg}", name = self.fighters[target].name, dmg));
        if self.fighters[id].hp <= 0 {
            self.died(id);
            return false;
        }
        true
    }

    /// The melee case (cell codes 4 and 5, 48b2f6): the target's preventive strike, the blow
    /// and vanilla Poison, Berserk of the target, CtrPoison, Suicide and PoisonS, the Splash
    /// recording, Stun, the on-hit block, vampirism, Hunger, the counter blow and the kill
    /// check, in that order.
    fn melee_case(&mut self, id: usize, target: usize, kind: ActionKind, counter: &mut Option<i32>) -> Struck {
        let mut out = Struck::new(kind);
        if !self.preventive_strike(id, target, kind, counter) {
            return out;
        }
        let raw = self.physical_damage(id, target, kind);
        out.amount = raw.min(self.fighters[target].hp);
        self.wound(target, out.amount);
        if self.rules != Rules::Game {
            self.editor_melee(id, target, raw);
            return self.melee_end(id, target, kind, out, counter);
        }
        if raw > 1 && self.fighters[id].has(Bonus::Poison) {
            self.fighters[target].regen = POISON_REGEN;
        }
        self.berserk_target(target);
        // CtrPoison: each blow on it costs the striker 20 regeneration (stacks).
        if self.fighters[target].has(Bonus::CtrPoison) {
            self.fighters[id].regen -= CTR_POISON_STEP;
            self.refresh(id);
        }
        self.suicide(id);
        if raw > 1 && self.fighters[id].has(Bonus::PoisonS) {
            self.fighters[target].regen = STRONG_POISON_REGEN;
        }
        self.splash.actor_col = self.fighters[id].slot.col as i32;
        self.splash_record(id, target, SPLASH_MELEE);
        self.stun(id, target);
        self.on_hit(id, target, raw, false);
        // Vampirism on the uncapped damage, not from undead or elementals; after a melee or
        // long strike only: the shot path never reaches it (48b3ce).
        let vamp = self.fighters[id].stats[Stat::Vampirizm];
        if vamp > 0 && !matches!(self.fighters[target].stats.nature, Nature::Undead | Nature::Elemental) {
            let f = &mut self.fighters[id];
            f.hp = f.hp.wrapping_add(raw.wrapping_mul(vamp) / 100).min(f.max_hp());
        }
        // Hunger: a melee kill heals the striker to full (c252e9). The original's test reads
        // the HP at a wrong address (unknown outcome); this is the intended reading.
        if !self.fighters[target].alive() && self.fighters[id].has(Bonus::Hunger) {
            let f = &mut self.fighters[id];
            f.hp = f.max_hp();
        }
        self.melee_end(id, target, kind, out, counter)
    }

    /// The melee case's end: the message, the counter blow and the kill check.
    fn melee_end(&mut self, id: usize, target: usize, kind: ActionKind, mut out: Struck, counter: &mut Option<i32>) -> Struck {
        let long = kind == ActionKind::LongStrike;
        let (name, tname) = (&self.fighters[id].name, &self.fighters[target].name);
        let killed = !self.fighters[target].alive();
        let dealt = out.amount;
        let msg = match (killed, long) {
            (true, false) => crate::trf!("{name} kills {tname} ({dealt})", name, tname, dealt),
            (true, true) => crate::trf!("{name} kills {tname} with a long strike ({dealt})", name, tname, dealt),
            (false, false) => crate::trf!("{name} hits {tname} for {dealt}", name, tname, dealt),
            (false, true) => crate::trf!("{name} hits {tname} with a long strike for {dealt}", name, tname, dealt),
        };
        self.log.push(msg);
        // Counterblow: a surviving target answers with a blow, even a Suicide striker waiting
        // for its removal; a striker it kills is removed with no on-kill effects (48b4b3).
        let t = &self.fighters[target];
        if t.alive() && t.has(Bonus::Counterblow) && self.switches().counterblow {
            let dmg = self.physical_damage(target, id, ActionKind::Melee).min(self.fighters[id].hp.max(0));
            self.fighters[id].hp -= dmg;
            *counter = Some(counter.unwrap_or(0) + dmg);
            self.log.push(crate::trf!("{name} hits back for {dmg}", name = self.fighters[target].name, dmg));
            if self.fighters[id].hp <= 0 {
                self.died(id);
            }
            return out;
        }
        out.killed = self.kill_check(target, id);
        out
    }

    /// The shot case (code 7, 48b214): the target's preventive shot, the shot and vanilla
    /// Poison, Berserk of the target, Suicide and PoisonS, the Splash recording, Stun, the
    /// on-hit block and the kill check. No vampirism, no counter blow.
    fn shot_case(&mut self, id: usize, target: usize, counter: &mut Option<i32>) -> Struck {
        let mut out = Struck::new(ActionKind::Shot);
        if !self.preventive_strike(id, target, ActionKind::Shot, counter) {
            return out;
        }
        let raw = self.physical_damage(id, target, ActionKind::Shot);
        out.amount = raw.min(self.fighters[target].hp);
        self.wound(target, out.amount);
        if self.rules != Rules::Game {
            self.editor_poison(id, target, raw);
        } else if raw > 1 && self.fighters[id].has(Bonus::Poison) {
            self.fighters[target].regen = POISON_REGEN;
        }
        self.berserk_target(target);
        self.suicide(id);
        if raw != 1 && self.fighters[id].has(Bonus::PoisonS) {
            self.fighters[target].regen = STRONG_POISON_REGEN;
        }
        self.splash_record(id, target, SPLASH_SHOT);
        self.stun(id, target);
        self.on_hit(id, target, raw, true);
        let (name, tname, dealt) = (&self.fighters[id].name, &self.fighters[target].name, out.amount);
        let msg = if self.fighters[target].alive() {
            crate::trf!("{name} hits {tname} for {dealt}", name, tname, dealt)
        } else {
            crate::trf!("{name} kills {tname} ({dealt})", name, tname, dealt)
        };
        self.log.push(msg);
        out.killed = self.kill_check(target, id);
        out
    }

    /// The hostile magic case (code 8, 48ac7e): a strike on a target with a negative modifier
    /// this turn, else a curse; then Berserk of the target, Drying, the mage Poison, Suicide,
    /// Exhaustion, PoisonS, Stun, the Splash recording, the on-hit block and the kill check.
    /// The value the chain tests is the spell's power, for a curse too.
    fn magic_case(&mut self, id: usize, target: usize) -> Struck {
        let p = self.hostile_power_of(id, target, self.cast_power(id));
        let school = self.school(id);
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        let mut out = Struck::new(ActionKind::Curse);
        let d;
        if self.fighters[target].weakened() {
            out.kind = ActionKind::Strike;
            let raw = self.strike_damage(id, target, p);
            d = raw;
            out.amount = raw.min(self.fighters[target].hp);
            self.wound(target, out.amount);
            self.log.push(crate::trf!("{name} hits {tname} with magic for {dealt}", name, tname, dealt = out.amount));
            // Vampirism on magic: Death strikes only, not from undead or elementals.
            let vamp = self.fighters[id].stats[Stat::Vampirizm];
            if self.rules != Rules::Game {
                if school == Some(MagicSchool::Death) && vamp > 0 {
                    self.editor_vampirism(id, target, raw);
                }
            } else if school == Some(MagicSchool::Death) && vamp > 0 && !matches!(self.fighters[target].stats.nature, Nature::Undead | Nature::Elemental) {
                let f = &mut self.fighters[id];
                f.hp = f.hp.wrapping_add(raw.wrapping_mul(vamp) / 100).min(f.max_hp());
            }
        } else {
            d = p;
            out.buff = self.curse_of(id, p);
            self.apply_buff(id, target, out.buff, false);
            self.fighters[target].cursed = true;
            self.log.push(crate::trf!("{name} curses {tname}: {what}", name, tname, what = out.buff.describe()));
            // An undead caster's Elemental or Death curse drains life to it.
            if self.fighters[id].base.nature == Nature::Undead && matches!(school, Some(MagicSchool::Elemental | MagicSchool::Death)) {
                // The target's loss is capped at its HP, but the caster gains the whole
                // amount (48afce, 48b100).
                let drain = p / self.opt().curse_main_spell.max(1) / 2 + 1;
                let loss = drain.min(self.fighters[target].hp);
                self.wound(target, loss);
                out.amount += loss;
                let f = &mut self.fighters[id];
                f.hp = (f.hp + drain).min(f.max_hp());
            }
        }
        // Berserk sees the HP before Drying's loss (c258bf).
        self.berserk_target(target);
        let dry = self.drying(id, target).min(self.fighters[target].hp);
        if dry > 0 {
            self.fighters[target].hp -= dry;
            out.amount += dry;
        }
        // The mage poison is the Community's: the editor's spells never poison.
        let poisoned = self.rules == Rules::Game && self.poison_power(id, target) > MAGE_POISON_POWER;
        if poisoned && self.fighters[id].has(Bonus::Poison) {
            self.fighters[target].regen = POISON_REGEN;
        }
        self.suicide(id);
        if self.fighters[id].has(Bonus::Exhaustion) {
            let t = &mut self.fighters[target];
            for st in [Stat::ProtectLife, Stat::ProtectDeath, Stat::ProtectElemental] {
                t.base[st] = (t.base[st] - EXHAUSTION_POINTS).max(0);
            }
        }
        if poisoned && self.fighters[id].has(Bonus::PoisonS) {
            self.fighters[target].regen = STRONG_POISON_REGEN;
        }
        self.stun(id, target);
        self.splash_record(id, target, SPLASH_MAGIC);
        // A Bleed caster's spell on a target that already bleeds makes the patch fault (an
        // access violation, c29a14); Razdor keeps the bleeding at 75 instead (a deviation).
        self.on_hit(id, target, d, false);
        out.killed = self.kill_check(target, id);
        if out.killed {
            self.log.push(crate::trf!("{name} kills {tname} with magic", name, tname));
        }
        out
    }

    /// The heal and bless case (code 9, 48a87c): a heal of a wounded ally (if the heal does
    /// anything), else a blessing, then the Splash recording. No Community on-hit effects.
    fn friendly_case(&mut self, id: usize, target: usize) -> Struck {
        let p = self.cast_power(id);
        let (name, tname) = (self.fighters[id].name.clone(), self.fighters[target].name.clone());
        let t = &self.fighters[target];
        let heal = self.heal_amount(id, target, p);
        let mut out = Struck::new(ActionKind::Heal);
        if t.wounded() && heal > 0 {
            out.amount = heal.min(t.max_hp() - t.hp);
            self.fighters[target].hp += out.amount;
            self.log.push(crate::trf!("{name} heals {tname} +{healed}", name, tname, healed = out.amount));
        } else {
            out.kind = ActionKind::Bless;
            out.buff = self.bless_of(id, target, p);
            self.apply_buff(id, target, out.buff, true);
            self.fighters[target].blessed = true;
            self.log.push(crate::trf!("{name} blesses {tname}: {what}", name, tname, what = out.buff.describe()));
        }
        self.splash_record(id, target, SPLASH_FRIENDLY);
        out
    }

    /// A blessing or curse: added to this turn's modifiers, or with `EternalGift` to the
    /// battle stats (which lasts and stacks; its Life blessing lowers the defences, a bug of
    /// the original). The change of actions left counts for this turn only; a curse cannot
    /// take them below 0.
    fn apply_buff(&mut self, caster: usize, target: usize, b: Buff, bless: bool) {
        let eternal = self.fighters[caster].has(Bonus::EternalGift);
        let life = self.school(caster) == Some(MagicSchool::Life);
        let t = &mut self.fighters[target];
        if eternal {
            // AB, or AS when AB is 0: an AB cursed below 0 still takes it (c29ea3 …).
            let attack = if t.base[Stat::AttackBlow] != 0 { Stat::AttackBlow } else { Stat::AttackShot };
            t.base[attack] += b.attack;
            let defence = if bless && life { -b.defence } else { b.defence };
            t.base[Stat::DefenceBlow] += defence;
            t.base[Stat::DefenceShot] += defence;
            t.base[Stat::Initiative] += b.initiative;
        } else {
            t.mods.attack += b.attack;
            t.mods.defence += b.defence;
            t.mods.initiative += b.initiative;
        }
        t.actions = (t.actions + b.actions).max(0);
        self.refresh(target);
    }

    /// Community `Berserk` on the unit hit: its attack modifier is set anew from its HP lost,
    /// overwriting a blessing's (c25777, c2581b, c258bf).
    fn berserk_target(&mut self, target: usize) {
        let t = &mut self.fighters[target];
        if t.has(Bonus::Berserk) {
            t.mods.attack = berserk(t);
            self.refresh(target);
        }
    }

    /// Community `Stun` (c27e5a, c2899a, c26e1f): every hit takes 30% of the target's current
    /// initiative (its base as of the turn start with the turn-1 Artillery or FirstShot bonus,
    /// not lowered by the earlier Stuns) off its initiative modifier, so each hit takes the
    /// same amount.
    fn stun(&mut self, id: usize, target: usize) {
        if self.fighters[id].has(Bonus::Stun) {
            let t = &mut self.fighters[target];
            t.mods.initiative -= t.cur_initiative * STUN_PERCENT / 100;
            self.refresh(target);
        }
    }

    /// Community `Suicide` after its unit's hostile hit (c2633c, c262a1, c261c1): HP 0,
    /// regeneration −99, no manoeuvres and no actions left, off its record's row and column.
    /// It is not removed: it waits in its side's list until a counter blow removes it or the
    /// next turn start's regeneration tick does (the original's).
    fn suicide(&mut self, id: usize) {
        let f = &mut self.fighters[id];
        if !f.has(Bonus::Suicide) {
            return;
        }
        f.hp = 0;
        f.regen = SUICIDE_REGEN;
        f.base[Stat::Manevres] = 0;
        f.actions = 0;
        f.suicided = true;
        let msg = crate::trf!("{name} gives its life", name = f.name);
        self.log.push(msg);
        self.refresh(id);
    }

    /// The Community on-hit block (c291d1, c296bc, c29984), in its order. `d` is the damage
    /// before capping at the HP, or the spell's power. With `d > 1`: PoisonArmorIgnore,
    /// Bleed (after a shot only on a target still alive), ArmorBreaker (each defence keeps
    /// `x − x/4`, rounded up) and KillingStrike. Then FateGift saves a target at 0 HP, before
    /// Neutralize can erase it; Neutralize and NoHeal work on every hit; BloodThrist gives an
    /// action per killing hit.
    fn on_hit(&mut self, id: usize, target: usize, d: i32, shot: bool) {
        let a = self.fighters[id].base.clone();
        let mark = (self.fighters[target].team.index(), self.record_index(target));
        let finish = crate::trf!("{name} finishes {tname}", name = self.fighters[id].name, tname = self.fighters[target].name);
        let t = &mut self.fighters[target];
        if d > 1 {
            if a.has(&Bonus::PoisonArmorIgnore) {
                t.regen = t.regen.min(PIERCING_POISON_REGEN);
            }
            if a.has(&Bonus::Bleed) && (!shot || t.alive()) {
                t.bleed = t.bleed.max(BLEED_PERCENT);
            }
            if a.has(&Bonus::ArmorBreaker) {
                for st in [Stat::DefenceShot, Stat::DefenceBlow] {
                    t.base[st] -= t.base[st] * 25 / 100;
                }
            }
            if a.has(&Bonus::KillingStrike) && t.base.max_hp() * KILLING_STRIKE_PERCENT / 100 >= t.hp {
                if t.alive() {
                    self.log.push(finish);
                }
                t.hp = 0;
            }
        }
        let t = &mut self.fighters[target];
        if t.has(Bonus::FateGift) && !t.alive() {
            t.base.bonuses.clear();
            t.actions = t.base[Stat::Manevres];
            for st in [Stat::ProtectLife, Stat::ProtectDeath, Stat::ProtectElemental] {
                t.base[st] += FATE_PROTECTION;
            }
            t.regen += FATE_REGEN;
            t.base[Stat::Hits] += t.base[Stat::Hits] * FATE_HP_PERCENT / 100;
            t.hp = t.base.max_hp();
            t.mods.initiative += FATE_INITIATIVE;
            let msg = crate::trf!("{name} is spared by fate", name = t.name);
            self.log.push(msg);
        }
        let t = &mut self.fighters[target];
        if a.has(&Bonus::Neutralize) {
            t.base.bonuses.clear();
        }
        if a.has(&Bonus::NoHeal) {
            t.regen = t.regen.min(0);
            if let Some(m) = self.crippled[mark.0].get_mut(mark.1) {
                *m = true;
            }
        }
        if a.has(&Bonus::BloodThrist) && !self.fighters[target].alive() {
            self.fighters[id].actions += 1;
        }
        self.refresh(target);
    }

    /// `i` loses `amount` hit points (already capped at its HP) through the damage routine
    /// (48a354): they count in its side's damage taken.
    fn wound(&mut self, i: usize, amount: i32) {
        let f = &mut self.fighters[i];
        f.hp -= amount;
        f.lost += amount;
        self.turn_lost[f.team.index()] += amount as i64;
    }

    /// HP `team` lost through the damage routine so far (side +0xC).
    fn side_lost(&self, team: Team) -> i64 {
        self.fighters.iter().filter(|f| f.team == team).map(|f| f.lost as i64).sum()
    }

    /// The kill check at the end of a hit (48a3f0): true if the target is at 0 HP, and then
    /// the on-kill effects and its removal. The killer of a `DeathCurse` unit dies; the killer
    /// of a `Ghost` dies if its Death protection is below 30 × the ghost's actions.
    fn kill_check(&mut self, i: usize, killer: usize) -> bool {
        if self.fighters[i].alive() {
            return false;
        }
        if self.fighters[killer].listed() {
            let dead = &self.fighters[i];
            let curse = dead.has(Bonus::DeathCurse)
                || (dead.has(Bonus::Ghost) && self.fighters[killer].stats[Stat::ProtectDeath] < 30 * dead.base[Stat::Manevres]);
            if curse {
                let kf = &mut self.fighters[killer];
                let hp = kf.hp.max(0);
                kf.lost += hp;
                self.turn_lost[kf.team.index()] += hp as i64;
                kf.hp = 0;
                let msg = crate::trf!("{name} dies by {caster}'s curse", name = self.fighters[killer].name, caster = self.fighters[i].name);
                self.log.push(msg);
                self.died(killer);
            }
        }
        self.died(i);
        true
    }

    // ------------------------------------------------------------------------------------
    // AI (4864e0; notes in battle.md §4)
    // ------------------------------------------------------------------------------------

    /// "Killable" in the AI's melee and shot scores (486d03, 486feb). With the improved AI,
    /// or for the player's side at the normal level, the actions left can do it:
    /// `HP ≤ actions × dmg`. Otherwise the test is meant to be `HP ≤ dmg`, but the original
    /// reads the HP of the unit with the target's list index on the actor's *own* side: an
    /// original bug, kept. Past the end of that list the record is empty (HP 0, killable).
    fn killable(&self, id: usize, t: usize, dmg: i32) -> bool {
        let f = &self.fighters[id];
        if self.ai_level == 2 || (self.ai_level == 1 && f.team == Team::Player) {
            return self.fighters[t].hp <= f.actions * dmg;
        }
        let index = self.living_ids(self.fighters[t].team).iter().position(|&i| i == t).unwrap_or(0);
        let hp = self.living_ids(f.team).get(index).map_or(0, |&i| self.fighters[i].hp);
        hp <= dmg
    }

    /// The best cell by the original's picker (4860cc): rows front to back, columns in the
    /// preferred order, the first strictly higher score wins, and 0 or less never does.
    fn pick<T: Copy>(&self, cands: impl IntoIterator<Item = (Slot, f64, T)>) -> Option<(f64, T)> {
        let order = self.formation.col_order();
        let mut v: Vec<(Slot, f64, T)> = cands.into_iter().collect();
        v.sort_by_key(|(s, _, _)| (s.row, order.iter().position(|&c| c == s.col).unwrap_or(usize::MAX)));
        let mut best: Option<(f64, T)> = None;
        for (_, score, x) in v {
            if score > 0.0 && best.is_none_or(|(b, _)| score > b) {
                best = Some((score, x));
            }
        }
        best
    }

    fn pick_target(&self, cands: impl IntoIterator<Item = (usize, f64)>) -> Option<(f64, usize)> {
        self.pick(cands.into_iter().map(|(t, s)| (self.fighters[t].slot, s, t)))
    }

    /// The target's answer in the AI's melee score: its melee (warrior) or shot (shooter)
    /// damage on the actor, else its power.
    fn return_threat(&self, id: usize, t: usize) -> i32 {
        match self.fighters[t].ai_role {
            AiRole::Warrior => self.physical_damage(t, id, ActionKind::Melee),
            AiRole::Shooter => self.physical_damage(t, id, ActionKind::Shot),
            AiRole::Mage => self.fighters[t].ai_power,
        }
    }

    /// The AI's poison bonus: a vanilla `Poison` attacker (not `PoisonS`) whose hit of more
    /// than 1 would poison a target that is not poisoned yet.
    fn poisons(&self, id: usize, t: usize, dmg: i64) -> bool {
        self.fighters[id].has(Bonus::Poison) && self.fighters[t].regen >= 0 && dmg > 1
    }

    /// Hits `u` needs to kill the enemy in the front row of `col` (4863e8): a blow if it has
    /// no AttackShot, else a shot, whether it could reach that cell or not;
    /// `⌊HP / dmg⌋ + 1`, one too many when the HP is a multiple of the damage. 0 with nobody
    /// there.
    fn hits_to_kill(&self, u: usize, col: u8) -> i64 {
        let Some(e) = self.at(self.fighters[u].team.other(), Slot::new(Row::Front, col)) else { return 0 };
        let kind = if self.fighters[u].base[Stat::AttackShot] < 1 { ActionKind::Melee } else { ActionKind::Shot };
        let dmg = self.physical_damage(u, e, kind).max(1);
        (self.fighters[e].hp / dmg + 1) as i64
    }

    fn ai_plan(&self) -> Option<Plan> {
        let id = self.active()?;
        let opts = self.all_options(id);
        // A reserve unit goes straight to the moves (4864e0), a Ghost's cast included.
        if self.fighters[id].slot.row == Row::Reserve {
            return Some(self.ai_move(id, &opts));
        }
        if let Some(plan) = self.ai_pull(id) {
            return Some(plan);
        }
        if let Some(plan) = self.ai_retreat(id, &opts) {
            return Some(plan);
        }
        let pick = |kinds: &[ActionKind], score: &dyn Fn(usize, ActionKind) -> i64| {
            self.pick(opts.iter().filter(|o| kinds.contains(&o.1)).map(|&(t, k)| (self.fighters[t].slot, score(t, k) as f64, (t, k))))
        };
        // Melee on the enemy front row (486bb9): `dmg × round((R + 1) × M)`, ×2 for a poison;
        // a kill replaces it with `100 × round((R + 1) × M)`, all in 32 bits. M is the
        // target's Manevres, plus half its actions left when those are negative.
        let melee = |t: usize, k: ActionKind| {
            let dmg = self.physical_damage(id, t, k);
            let tf = &self.fighters[t];
            let manevres = tf.base[Stat::Manevres];
            let m = if self.rules != Rules::Game {
                // The editor's code adds half the actions left when there are some (0x4fab03);
                // the game's, only when negative, so never.
                manevres as f64 + if tf.actions > 0 { tf.actions as f64 / 2.0 } else { 0.0 }
            } else if manevres == 0 {
                // Community c25b63: 1.5 × K for a target with 0 Manevres, K being four bytes
                // that are mostly the next instruction, so about 1.75e9. Only the low 32 bits
                // of the rounded product are kept and the score wraps (the original's).
                MANEVRES_0_FACTOR
            } else if tf.actions < 0 {
                manevres as f64 + tf.actions as f64 / 2.0
            } else {
                manevres as f64
            };
            let r = experience::round_half_even((self.return_threat(id, t) as f64 + 1.0) * m) as i32;
            let mut s = dmg.wrapping_mul(r);
            if self.poisons(id, t, dmg as i64) {
                s = s.wrapping_mul(2);
            }
            if self.killable(id, t, dmg) {
                s = r.wrapping_mul(100);
            }
            s as i64
        };
        if let Some((_, (t, k))) = pick(&[ActionKind::Melee, ActionKind::LongStrike], &melee) {
            return Some(Plan::Act(t, k));
        }
        // Shots, in integers after the first rounding (486f90).
        let shot = |t: usize, _| {
            let dmg = self.physical_damage(id, t, ActionKind::Shot) as i64;
            let tf = &self.fighters[t];
            let m = tf.base[Stat::Manevres] as f64 + if tf.actions > 0 { (tf.actions as f64).sqrt() } else { 0.0 };
            let mut s = experience::round_half_even((tf.ai_power + 1) as f64 * dmg as f64 * m);
            if self.poisons(id, t, dmg) {
                s *= 2;
            }
            if self.killable(id, t, dmg as i32) {
                s *= 4;
            }
            if tf.slot.row == Row::Back {
                s = match tf.ai_role {
                    AiRole::Warrior => s / 3,
                    AiRole::Shooter => s * 3 / 2,
                    AiRole::Mage => {
                        // Only a back-row mage is halved for a hostile-only direction with the
                        // actor's nature, and again for a single Manevres (487193).
                        let mut v = s * 7 / 4;
                        if tf.stats.magic_direction() == super::content::MagicDirection::ToEnemy && tf.base.nature == self.fighters[id].base.nature {
                            v /= 2;
                        }
                        if tf.base[Stat::Manevres] == 1 {
                            v /= 2;
                        }
                        v
                    }
                };
            }
            s
        };
        if let Some((_, (t, k))) = pick(&[ActionKind::Shot], &shot) {
            return Some(Plan::Act(t, k));
        }
        if let Some(plan) = self.ai_magic(id, &opts) {
            return Some(plan);
        }
        Some(self.ai_move(id, &opts))
    }

    /// A front-row unit that is not a warrior, with more than one action and an AttackBlow
    /// not above both its power and its AttackShot, steps back behind the healthiest own
    /// front unit, if another own unit stands in front (or it is the side's lone mage).
    ///
    /// With no back-row cell to step to, the original's edge rule (4ed390 on) scores 1 on the
    /// own front cell of the first column when the unit stands in the second and an enemy
    /// stands in the last front column, and on the last column's cell when it stands in the
    /// last but one and an enemy stands in the first; that needs another own front unit.
    /// The cell's own code is then what happens: a step there when it is free and next to
    /// the unit, a heal or blessing on an ally there, else an action spent for nothing.
    fn ai_retreat(&self, id: usize, opts: &[(usize, ActionKind)]) -> Option<Plan> {
        let f = &self.fighters[id];
        let (ab, sh, mp) = (f.base[Stat::AttackBlow], f.base[Stat::AttackShot], f.power);
        if f.slot.row != Row::Front || f.has(Bonus::Ghost) || f.actions <= 1 || f.ai_role == AiRole::Warrior || (ab > mp && ab > sh) {
            return None;
        }
        let others = self.living(f.team).filter(|o| o.slot.row == Row::Front).count() > 1;
        let alone_mage = self.living(f.team).count() == 1 && f.ai_role == AiRole::Mage;
        let moves = self.moves(id);
        let mut cands: Vec<(Slot, f64, Slot)> = Vec::new();
        if others || alone_mage {
            cands.extend(moves.iter().filter(|m| m.row == Row::Back).map(|&m| {
                let front = self.at(f.team, Slot::new(Row::Front, m.col)).filter(|&o| o != id);
                (m, 1000.0 + front.map_or(0, |o| self.fighters[o].hp) as f64, m)
            }));
        }
        let cols = self.formation.cols;
        if !self.switches().long_strike {
            self.retreat_scores(id, &mut cands);
        }
        let enemy_front = |c: u8| self.at(f.team.other(), Slot::new(Row::Front, c)).is_some();
        if self.switches().long_strike && others && f.slot.col == 1 && enemy_front(cols - 1) {
            cands.push((Slot::new(Row::Front, 0), 1.0, Slot::new(Row::Front, 0)));
        }
        if self.switches().long_strike && others && f.slot.col + 2 == cols && enemy_front(0) {
            cands.push((Slot::new(Row::Front, cols - 1), 1.0, Slot::new(Row::Front, cols - 1)));
        }
        let (_, to) = self.pick(cands)?;
        Some(if moves.contains(&to) {
            Plan::Move(to)
        } else if let Some(&(t, k)) = self.at(f.team, to).and_then(|t| opts.iter().find(|o| o.0 == t)) {
            Plan::Act(t, k)
        } else {
            // A cell with no code: the action is spent and nothing happens, as a pass.
            Plan::Pass
        })
    }

    /// The strike power of `a` on `t` (485b3c for a strike): protection, the nature table
    /// and GodAnger/GodStrike, without Drying.
    fn strike_power(&self, a: usize, t: usize) -> i64 {
        self.strike_damage(a, t, self.hostile_power_of(a, t, self.fighters[a].power)) as i64
    }

    /// Magic by the caster's school (486bb9: Life, 487e99: Elemental, 488928: Death). It only
    /// picks a cell; the cell's own action (heal or bless, curse or strike) is what is cast.
    /// Every school scores the cells of rows 1 and 2.
    fn ai_magic(&self, id: usize, opts: &[(usize, ActionKind)]) -> Option<Plan> {
        let f = &self.fighters[id];
        if !f.stats.is_mage() {
            return None;
        }
        let in_rows = |t: &usize| self.fighters[*t].slot.row.is_active();
        let hostile: Vec<usize> = opts.iter().filter(|o| matches!(o.1, ActionKind::Strike | ActionKind::Curse)).map(|o| o.0).filter(in_rows).collect();
        let friendly: Vec<usize> = opts.iter().filter(|o| matches!(o.1, ActionKind::Heal | ActionKind::Bless)).map(|o| o.0).filter(in_rows).collect();
        let act = |t: usize| {
            let k = opts.iter().find(|o| o.0 == t).map(|o| o.1).expect("an option");
            Plan::Act(t, k)
        };
        let mp = f.power as i64;
        let dir = f.stats.magic_direction();
        let cms = self.opt().curse_main_spell.max(1) as i64;
        let wound = |t: usize| (self.fighters[t].max_hp() - self.fighters[t].hp) as i64;
        let shielded = |t: usize| self.fighters[t].base.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]);
        match self.school(id)? {
            MagicSchool::Life => {
                let ghostly = hostile.iter().any(|&t| shielded(t));
                let shooters = hostile.iter().any(|&t| self.fighters[t].base[Stat::AttackShot] >= 1);
                if dir.helps_allies() && !ghostly {
                    // The biggest wound, but not one below a quarter of the power, nor an
                    // undead's.
                    let heal = self.pick_target(friendly.iter().map(|&t| {
                        let v = wound(t);
                        (t, if v * 4 < mp || self.fighters[t].stats.nature == Nature::Undead { 0.0 } else { v as f64 })
                    }));
                    if let Some((_, t)) = heal {
                        return Some(act(t));
                    }
                    let bless = self.pick_target(friendly.iter().map(|&t| {
                        let tf = &self.fighters[t];
                        if tf.mods.defence >= 1 || tf.actions <= 0 {
                            return (t, 0.0);
                        }
                        let d = (tf.base[Stat::DefenceBlow] + 20 + tf.base[Stat::DefenceShot]).max(1) as i64;
                        let mut v = tf.ai_power as i64 * 100 * tf.base[Stat::Manevres] as i64 / d;
                        if !shooters {
                            if tf.slot.row == Row::Back {
                                v /= 5;
                            }
                        } else if tf.ai_role == AiRole::Mage {
                            v *= 2;
                        } else if tf.slot.row == Row::Front {
                            v *= 3;
                        }
                        (t, v as f64)
                    }));
                    if let Some((_, t)) = bless {
                        return Some(act(t));
                    }
                }
                if !dir.hits_enemies() {
                    return None;
                }
                let strike = self.pick_target(hostile.iter().map(|&t| (t, self.life_strike_score(id, t) as f64)));
                strike.map(|(_, t)| act(t))
            }
            MagicSchool::Elemental => self.ai_elemental(id, &hostile, &friendly).map(act),
            MagicSchool::Death => {
                if dir.hits_enemies() {
                    // Nearly dead and too weak to strike well: it picks its own cell, a
                    // self-cast if one is offered, else a pass.
                    let best = self.pick_target(hostile.iter().map(|&t| (t, self.strike_power(id, t) as f64)));
                    if let Some((p, _)) = best {
                        let (hp, max) = (f.hp as i64, f.max_hp() as i64);
                        if p as i64 <= mp / cms && hp <= max / 4 && max - mp >= hp {
                            return Some(if friendly.contains(&id) { act(id) } else { Plan::Pass });
                        }
                    }
                    let strike = self.pick_target(hostile.iter().map(|&t| {
                        let tf = &self.fighters[t];
                        let p = self.strike_power(id, t);
                        if p <= 0 {
                            return (t, 0.0);
                        }
                        let n = f.actions as i64;
                        let v = if tf.weakened() { n * p } else { p / cms / 2 + 1 + (n - 1) * p };
                        let kill = if tf.hp as i64 <= v { 20 - 2 * (tf.hp as i64 / p) } else { 0 };
                        let m = tf.base[Stat::Manevres] as i64;
                        // The threat: the target's damage on the caster times its Manevres; with
                        // none, the role's minimum.
                        let threat = |dmg: i64, top: i64, low: i64| {
                            let d = dmg * m;
                            if d > 0 { (top - f.hp as i64 / d).max(low) } else { low }
                        };
                        let th = match tf.ai_role {
                            AiRole::Shooter => threat(self.physical_damage(t, id, ActionKind::Shot) as i64, 16, 3),
                            AiRole::Mage => threat(self.strike_power(t, id), 12, 2),
                            AiRole::Warrior => threat(self.physical_damage(t, id, ActionKind::Melee) as i64, 8, 1),
                        };
                        (t, (tf.ai_power as i64 * ((kill + th) * v)) as f64)
                    }));
                    if let Some((_, t)) = strike {
                        return Some(act(t));
                    }
                }
                if !dir.helps_allies() {
                    return None;
                }
                let heal = self.pick_target(friendly.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    (t, if matches!(tf.stats.nature, Nature::Undead | Nature::Elemental) { wound(t) as f64 } else { 0.0 })
                }));
                if let Some((_, t)) = heal {
                    return Some(act(t));
                }
                let bless = self.pick_target(friendly.iter().map(|&t| {
                    let tf = &self.fighters[t];
                    let ok = !tf.blessed && tf.actions > 0 && tf.ai_role != AiRole::Mage;
                    (t, if ok { (tf.actions as i64 * tf.ai_power as i64 * tf.hp as i64) as f64 } else { 0.0 })
                }));
                bless.map(|(_, t)| act(t))
            }
        }
    }

    /// A Life mage's score for an enemy cell (4879c0..487c93): the curse or strike value V
    /// times the target's worth, ×3 for a cursed target V can kill, by nature.
    fn life_strike_score(&self, id: usize, t: usize) -> i64 {
        let tf = &self.fighters[t];
        let life = (2 * self.opt().curse_main_spell / 3).max(1) as i64;
        let p = self.strike_power(id, t);
        let (db, ds) = (tf.base[Stat::DefenceBlow] as i64, tf.base[Stat::DefenceShot] as i64);
        let v = if p < 1 {
            0
        } else if !tf.cursed {
            // The second term compares DefenceShot but adds DefenceBlow (4879c0), an original
            // slip, kept.
            let q = p / life;
            3 * ((if db < q { db } else { q }) + (if ds < q { db } else { q })) + 1
        } else {
            self.fighters[id].actions as i64 * p
        };
        let shielded = tf.base.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost]);
        let mut s = if shielded { 3 * tf.ai_power as i64 } else { db + ds + tf.ai_power as i64 * tf.actions as i64 };
        s *= v;
        if tf.hp as i64 <= v && tf.cursed && !tf.base.has_any(&[Bonus::DeathCurse, Bonus::Ghost]) {
            s *= 3;
        }
        match tf.stats.nature {
            Nature::Undead => s,
            Nature::Elemental => s * 2 / 3,
            _ => s / 3,
        }
    }

    /// Elemental magic (487c93, 487e99): per side a main and an alternative value, the cells
    /// scanned row by row and column by column (not in the picker's order), a strictly
    /// higher value winning. Own side: haste (main) or heal (alternative); enemy side: slow
    /// (main) or strike (alternative). The alternative replaces the main when strictly
    /// higher, and the own side wins only when strictly higher than the enemy's.
    fn ai_elemental(&self, id: usize, hostile: &[usize], friendly: &[usize]) -> Option<usize> {
        let f = &self.fighters[id];
        let team = f.team;
        let mp = f.power as i64;
        let tier = actions_of_power(f.power) as i64;
        let by_cell = |v: &[usize]| {
            let mut v = v.to_vec();
            v.sort_by_key(|&t| (self.fighters[t].slot.row, self.fighters[t].slot.col));
            v
        };
        let all_ghost = self.living(team).all(|u| u.has(Bonus::Ghost));
        let enemies = self.living(team.other()).count() as i64;
        let shields = self.living(team.other()).filter(|e| e.base.has_any(&[Bonus::Unvulnerabe, Bonus::Ghost])).count() as i64;
        // ÷10 for the own side when half the enemies are shielded, unless the target has
        // GodAnger or GodStrike, or it is a Life or Elemental mage while the *caster* can
        // reach enemies (the original reads the caster's direction there).
        let caster_hostile = f.stats.magic_direction().hits_enemies();
        let shield_rule = |t: usize, v: i64| {
            let tf = &self.fighters[t];
            let life_or_elemental = matches!(tf.base.magic, Some(MagicSchool::Life | MagicSchool::Elemental));
            if shields > 0 && enemies / 2 <= shields && !tf.base.has_any(&[Bonus::GodAnger, Bonus::GodStrike]) && (!life_or_elemental || !caster_hostile) {
                v / 10
            } else {
                v
            }
        };
        let (mut main, mut alt) = ([(0i64, None::<usize>); 2], [(0i64, None::<usize>); 2]);
        let better = |slot: &mut (i64, Option<usize>), v: i64, t: usize| {
            if v > slot.0 {
                *slot = (v, Some(t));
            }
        };
        let own = by_cell(friendly);
        let mean = self.mean_initiative[team.other().index()];
        for &t in &own {
            let tf = &self.fighters[t];
            // Haste: not the caster itself, not hasted yet, with an action left (a unit of 0
            // Manevres passes the action test, Community c25da0).
            if t == id || tf.mods.initiative >= 1 || (tf.actions <= 0 && tf.base[Stat::Manevres] != 0) {
                continue;
            }
            let mut v = experience::round_half_even((tier * tf.ai_power as i64 * tf.base[Stat::Initiative] as i64) as f64 / mean);
            if tf.slot.row == Row::Front {
                // Scaled down by the hits it needs to kill the enemies facing it (4863e8).
                let c = tf.slot.col as i32;
                let s: i64 = (c - 1..=c + 1).filter(|&k| (0..self.formation.cols as i32).contains(&k)).map(|k| self.hits_to_kill(t, k as u8)).sum();
                let l = tier + tf.actions as i64;
                if s < l && l != 0 {
                    v = experience::round_half_even(v as f64 * s as f64 / l as f64);
                }
            }
            better(&mut main[0], shield_rule(t, v), t);
        }
        for &t in &own {
            let tf = &self.fighters[t];
            if tf.wounded() {
                let v = (2 * mp / 3).min((tf.max_hp() - tf.hp) as i64);
                better(&mut alt[0], shield_rule(t, v), t);
            }
        }
        for &t in &by_cell(hostile) {
            let tf = &self.fighters[t];
            let s = actions_of_power(self.hostile_power_of(id, t, f.power)) as i64 * tf.ai_power as i64;
            let ten = if all_ghost && tf.cursed { 10 } else { 1 };
            if s < 1 || tf.mods.initiative < 0 || tf.actions < 1 {
                better(&mut alt[1], self.strike_power(id, t) * ten, t);
            } else {
                let slow = tf.actions as i64 * s;
                better(&mut main[1], slow, t);
                if f.actions > 1 {
                    better(&mut alt[1], (slow + self.strike_power(id, t)) * ten, t);
                }
            }
        }
        for side in 0..2 {
            if main[side].0 < alt[side].0 {
                main[side] = alt[side];
            }
        }
        let side = if main[1].0 < main[0].0 { 0 } else { 1 };
        main[side].1.filter(|_| main[side].0 > 0)
    }

    /// The AI's own-cell action: a self-cast when its cell offers one, else a pass.
    fn own_cell_plan(&self, id: usize) -> Plan {
        match self.options(id, id).first() {
            Some(&kind) => Plan::Act(id, kind),
            None => Plan::Pass,
        }
    }

    /// Moves when nothing else scored, and everything a reserve unit does (489549). The AI
    /// never moves into the reserve; with nothing better it takes its own cell.
    fn ai_move(&self, id: usize, opts: &[(usize, ActionKind)]) -> Plan {
        let f = &self.fighters[id];
        let (team, from) = (f.team, f.slot);
        let moves = self.moves(id);
        let enemy_at = |row: Row, c: u8| self.at(team.other(), Slot::new(row, c)).is_some();
        let cols = self.formation.cols;
        let best = match from.row {
            // A pure warrior behind steps forward: `3·|MP| + AS` of the own back-row unit in
            // that column (another column than its own), +2 facing an enemy, +1.
            Row::Back if f.base[Stat::AttackShot] == 0 && f.power == 0 => self.pick(moves.iter().filter(|m| m.row == Row::Front).map(|&m| {
                let behind = self.at(team, Slot::new(Row::Back, m.col)).filter(|_| m.col != from.col);
                let support = behind.map_or(0, |o| 3 * self.fighters[o].power.abs() + self.fighters[o].base[Stat::AttackShot]);
                let facing = if enemy_at(Row::Front, m.col) { 2 } else { 0 };
                (m, (support + facing + 1) as f64, Some(m))
            })),
            // Any other back-row unit looks for a cell code that is never written.
            Row::Back => None,
            // Front row: `2·(4 − |d|)` per enemy front unit at d columns and `4 − |d|` per enemy
            // back unit, negative far away (no floor). Only the cells with a code of their own
            // keep their score: the cells it can step to, its own cell (a pass or a self-cast)
            // and, for a friendly caster, an ally's front cell it could heal or bless, in any
            // column; picking that one casts on the ally (489549 zeroes only the cells of code
            // 0).
            Row::Front => {
                let score = |c: u8| {
                    (0..cols)
                        .map(|e| {
                            let d = c.abs_diff(e) as i32;
                            i32::from(enemy_at(Row::Front, e)) * 2 * (4 - d) + i32::from(enemy_at(Row::Back, e)) * (4 - d)
                        })
                        .sum::<i32>()
                };
                let allies = opts.iter().filter(|o| o.0 != id && matches!(o.1, ActionKind::Heal | ActionKind::Bless) && self.fighters[o.0].slot.row == Row::Front);
                let cells = moves
                    .iter()
                    .filter(|m| m.row == Row::Front)
                    .map(|&m| (m, FrontPick::Move(m)))
                    .chain(std::iter::once((from, FrontPick::Own)))
                    .chain(allies.map(|&(t, k)| (self.fighters[t].slot, FrontPick::Cast(t, k))));
                match self.pick(cells.map(|(s, p)| (s, score(s.col) as f64, p))) {
                    Some((_, FrontPick::Move(m))) => return Plan::Move(m),
                    Some((_, FrontPick::Cast(t, k))) => return Plan::Act(t, k),
                    _ => None,
                }
            }
            Row::Reserve => {
                // A mage tends the most wounded reserve unit it can target.
                if f.ai_role == AiRole::Mage {
                    let tend = opts.iter().filter(|o| matches!(o.1, ActionKind::Heal | ActionKind::Bless) && self.fighters[o.0].slot.row == Row::Reserve);
                    let heal = self.pick(tend.map(|&(t, k)| {
                        let tf = &self.fighters[t];
                        (tf.slot, (tf.max_hp() - tf.hp) as f64, (t, k))
                    }));
                    if let Some((_, (t, k))) = heal {
                        return Plan::Act(t, k);
                    }
                }
                if f.ai_role == AiRole::Warrior {
                    // 2 for the front row, 1 for the back row.
                    self.pick(moves.iter().filter(|m| m.row.is_active()).map(|&m| (m, (3 - m.row.number()) as f64, Some(m))))
                } else {
                    // The nearest back-row cell; with 6 columns the Community starts the scan at
                    // the second column (c26c7e).
                    let skip_first = cols == 6 && self.rules == Rules::Game;
                    self.pick(moves.iter().filter(|m| m.row == Row::Back && !(skip_first && m.col == 0)).map(|&m| (m, (3 - m.col.abs_diff(from.col) as i32) as f64, Some(m))))
                }
            }
        };
        match best {
            Some((_, Some(m))) => Plan::Move(m),
            _ => self.own_cell_plan(id),
        }
    }

    /// The action the AI would take with the active fighter, if it attacks or casts.
    pub fn ai_choice(&self) -> Option<(usize, ActionKind)> {
        match self.ai_plan()? {
            Plan::Act(t, k) => Some((t, k)),
            _ => None,
        }
    }

    /// Plays one action of the active fighter automatically.
    pub fn ai_step(&mut self) -> Option<Step> {
        let actor = self.active()?;
        match self.ai_plan()? {
            Plan::Act(t, kind) => {
                let hit = self.act_with(t, kind).ok()?;
                Some(Step::Act { actor, hit })
            }
            Plan::Move(to) => {
                let from = self.fighters[actor].slot;
                self.move_active(to).ok()?;
                Some(Step::Move { actor, from, to })
            }
            Plan::Pass => {
                self.pass();
                Some(Step::Wait { actor })
            }
            Plan::Pull(t) => {
                let from = self.fighters[t].slot;
                self.pull(t, true).ok()?;
                Some(Step::Move { actor: t, from, to: self.fighters[t].slot })
            }
        }
    }

    /// Quick battle (a Razdor extra the players asked for, like the auto-combat of other
    /// games): the battle is played out at once with the battle AI on both sides, the
    /// player's units under the same rules as any AI side (the reserve rules, no simulation
    /// shortcuts; see [`Battle::ai_step`]). Only the watching is skipped: the outcome, the
    /// log and the XP shares are those of a battle played step by step. Deploys as the
    /// cards stand when still deploying. Deterministic; stops after [`AUTO_PLAY_STEPS`]
    /// steps at most (the turn limit ends every battle long before). Returns the outcome.
    pub fn auto_play_to_end(&mut self) -> Outcome {
        self.begin();
        for _ in 0..AUTO_PLAY_STEPS {
            if self.outcome() != Outcome::Ongoing {
                break;
            }
            // A plan that cannot be carried out still ends the unit's turn.
            if self.ai_step().is_none() {
                self.skip();
            }
        }
        self.outcome()
    }

    // ------------------------------------------------------------------------------------
    // After the battle
    // ------------------------------------------------------------------------------------

    /// `team`'s strength now: its living units with their current HP and rows. A side that
    /// surrendered has none left, so its strength is 0 (483ecc tests the flag).
    pub fn strength_now(&self, team: Team) -> i64 {
        let side: Vec<SideUnit> = self.fighters.iter().filter(|f| f.team == team && f.alive()).map(side_unit).collect();
        experience::side_strength(&side)
    }

    /// Each survivor's share of `team`'s XP pool once the battle is over, before any
    /// modifier (experience.md §3): pool = the enemy's starting strength div 20 × the share
    /// of `team`'s starting HP not lost; share = [`experience::share`] by row and activity.
    /// The dead get nothing but count in the divisor. A side that surrendered gets nothing:
    /// the original works out its shares, then takes its units off the field (48bb10).
    pub fn xp_awards(&self, team: Team) -> Vec<XpAward> {
        if self.deploying || self.outcome() == Outcome::Ongoing {
            return Vec::new();
        }
        let own = self.start[team.index()];
        let pool = self.pool(team);
        (0..self.fighters.len())
            .filter(|&i| self.fighters[i].team == team && self.fighters[i].alive())
            .map(|i| {
                let f = &self.fighters[i];
                XpAward { fighter: i, xp: experience::share(pool, own.count, f.slot.row, f.useful, f.taken, f.actions.max(0)) }
            })
            .collect()
    }

    /// `team`'s XP pool (experience.md §3): from the enemy's starting strength, the HP it
    /// lost, the pre-simulation's predicted loss and its largest loss in one turn.
    pub fn pool(&self, team: Team) -> i64 {
        let t = team.index();
        experience::battle_pool(self.start[team.other().index()].strength, self.start[t].hp, self.side_lost(team), self.predicted[t], self.max_turn_lost[t])
    }

    /// The pre-simulation's HP loss of `team` (0 before [`Battle::begin`] or without one).
    pub fn predicted_loss(&self, team: Team) -> i64 {
        self.predicted[team.index()]
    }

    /// What the player's survivors gain: only after a victory, each share ×
    /// `HeroExpirienceModificator` × the difficulty factor × the beaten army's correction,
    /// capped by the Community limit ([`experience::player_gain`]).
    pub fn player_xp(&self) -> Vec<XpAward> {
        if self.outcome() != Outcome::Victory {
            return Vec::new();
        }
        let o = self.opt();
        self.xp_awards(Team::Player)
            .into_iter()
            .map(|a| XpAward { xp: experience::player_gain(a.xp, o.hero_experience_modificator, o.difficulty_factor, self.xp_correction), ..a })
            .collect()
    }

    /// What an AI side gains in a battle between AI armies: each share ×
    /// `AIExpiriencePercent` / 100, for a side that still has strength at the end.
    pub fn ai_xp(&self, team: Team) -> Vec<XpAward> {
        if self.strength_now(team) <= 0 {
            return Vec::new();
        }
        let pct = self.opt().ai_experience_percent;
        self.xp_awards(team).into_iter().map(|a| XpAward { xp: experience::ai_gain(a.xp, pct), ..a }).collect()
    }

    /// Final state of the player's fighters. The hero cannot die while a unit of his army
    /// survives: he comes back with 1 HP (4906a0). Slots are where they stand at the end:
    /// the army's formation is rebuilt from the battle grid (4988c0, see
    /// `Game::settle_battle` for the units without a cell).
    pub fn player_results(&self) -> Vec<FighterResult> {
        let survivors = self.living(Team::Player).next().is_some();
        self.fighters
            .iter()
            .filter_map(|f| {
                let squad_index = f.squad_index?;
                let hp = if f.is_hero && !f.alive() && survivors { 1 } else { f.hp.max(0) };
                Some(FighterResult { squad_index, hp, slot: f.slot })
            })
            .collect()
    }
}

/// The battle AI's strength and role of a unit (4836cc): the best of AB, AS and MP plus a
/// third of the other two, with the bonus additions; a mage if its MP reaches two thirds of
/// that, else a shooter if its AS does, else a warrior (no attack at all makes a mage).
fn ai_power_role(s: &Stats) -> (i32, AiRole) {
    let (ab, sh, mp) = (s[Stat::AttackBlow], s[Stat::AttackShot], s[Stat::MagicPower]);
    let top = ab.max(sh).max(mp);
    let mut power = top + (ab + sh + mp - top) / 3;
    power += [(Bonus::GodAnger, 10), (Bonus::ArmorIgnore, 15), (Bonus::GodStrike, 20), (Bonus::Counterblow, ab), (Bonus::FlankStrike, 10)]
        .iter()
        .filter(|(b, _)| s.has(b))
        .map(|(_, v)| v)
        .sum::<i32>();
    let role = match experience::role(s) {
        Role::Mage => AiRole::Mage,
        Role::Shooter => AiRole::Shooter,
        Role::Melee => AiRole::Warrior,
    };
    (power, role)
}

/// Community `Berserk` (c256c8): the attack modifier is `((maxHP − HP) × 75 × AB / maxHP) / 100`,
/// two truncating divisions, the product in 32 bits (it wraps for a Bastion's huge AB).
fn berserk(f: &Fighter) -> i32 {
    let max = f.base.max_hp().max(1);
    (max - f.hp.clamp(0, max)).wrapping_mul(BERSERK_PERCENT).wrapping_mul(f.base[Stat::AttackBlow]) / max / 100
}

mod editor;
pub use editor::{EditorBonus, Rules, Switches};
use editor::{vampire_nature, EDITOR_KNIGHT_PERCENT};

#[cfg(test)]
mod tests;
